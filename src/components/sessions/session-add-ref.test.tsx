import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionRefAddReq, SessionRefCandidateVm } from "@/lib/ipc/client";

const sessionsRefCandidates = vi.fn();
const sessionsRefAdd = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsRefCandidates: (root: unknown, session: unknown, query: unknown) =>
    sessionsRefCandidates(root, session, query),
  sessionsRefAdd: (root: unknown, session: unknown, req: unknown) =>
    sessionsRefAdd(root, session, req),
}));

import {
  SESSION_ADD_REF_ALIAS_LABEL,
  SESSION_ADD_REF_CONFIRM,
  SESSION_ADD_REF_FAILED,
  SESSION_ADD_REF_FILE_LABEL,
  SESSION_ADD_REF_LABEL,
  SESSION_ADD_REF_LIST_LABEL,
  SESSION_ADD_REF_NEW_FILE_SUFFIX,
  SESSION_ADD_REF_NONE,
  SESSION_ADD_REF_PROMOTE_LABEL,
  SESSION_ADD_REF_ROW_TESTID,
  SESSION_ADD_REF_SEARCH_LABEL,
  SESSION_ADD_REF_TRUNCATED,
  SESSION_ADD_REF_URL_LABEL,
  SessionAddRef,
} from "@/components/sessions/session-add-ref";

const NOW = Date.now();

function candidate(over: Partial<SessionRefCandidateVm> & Pick<SessionRefCandidateVm, "target">) {
  return {
    kind: "file",
    label: over.target,
    detail: "",
    tags: [],
    mtimeMs: NOW - 60_000,
    promotable: false,
    ...over,
  } satisfies SessionRefCandidateVm;
}

/** One of each source, with the workspace file the promotion offer is about. */
function candidates(): SessionRefCandidateVm[] {
  return [
    candidate({ target: "artifacts/report.pdf", label: "report.pdf", detail: "artifacts" }),
    candidate({
      target: "workspace/draft.md",
      label: "draft.md",
      detail: "workspace",
      promotable: true,
    }),
    candidate({
      kind: "note",
      target: "Keeper architecture",
      label: "Keeper architecture",
      detail: "10-notes",
      tags: ["project/keeper"],
    }),
    candidate({
      kind: "recording",
      target: "Standup 2026-08-12",
      label: "Standup 2026-08-12",
      detail: "30-recordings",
    }),
  ];
}

function mount(over: Partial<React.ComponentProps<typeof SessionAddRef>> = {}) {
  const onChanged = vi.fn();
  const result = render(
    <SessionAddRef
      rootId="tgdrive"
      sessionId="active/2026-08-10-keeper"
      onChanged={onChanged}
      {...over}
    />,
  );
  return { ...result, onChanged };
}

/** Open the dialog and wait for the first candidate read to land. */
async function opened(over: Partial<React.ComponentProps<typeof SessionAddRef>> = {}) {
  const mounted = mount(over);
  fireEvent.click(screen.getByRole("button", { name: SESSION_ADD_REF_LABEL }));
  const dialog = await screen.findByRole("dialog");
  await waitFor(() => expect(sessionsRefCandidates).toHaveBeenCalled());
  await screen.findByTestId(`${SESSION_ADD_REF_ROW_TESTID}-artifacts/report.pdf`);
  return { ...mounted, dialog };
}

/** The one request the component sent, typed as the command's own argument. */
function sentRequest(): SessionRefAddReq {
  return sessionsRefAdd.mock.calls[0]?.[2] as SessionRefAddReq;
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  sessionsRefCandidates.mockResolvedValue({
    candidates: candidates(),
    targets: ["references.md"],
    defaultTarget: "references.md",
    truncated: false,
  });
  sessionsRefAdd.mockResolvedValue({
    file: "references.md",
    line: "- [report.pdf](artifacts/report.pdf)",
    promoted: null,
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("SessionAddRef", () => {
  it("reads nothing until the dialog is open", () => {
    mount();
    expect(sessionsRefCandidates).not.toHaveBeenCalled();
  });

  it("sends a picked file's target back unchanged", async () => {
    const { dialog, onChanged } = await opened();
    fireEvent.click(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-artifacts/report.pdf`),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    // Verbatim, and no label invented for it: the frontend composes nothing
    // (AD-65) and keeper does not name somebody else's reference.
    expect(sentRequest()).toEqual({
      kind: "file",
      target: "artifacts/report.pdf",
      label: null,
      file: "references.md",
      promote: false,
    });
    expect(onChanged).toHaveBeenCalled();
  });

  it("addresses a note by its title, which is what a wikilink names", async () => {
    const { dialog } = await opened();
    fireEvent.click(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-Keeper architecture`),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    expect(sentRequest().kind).toBe("note");
    expect(sentRequest().target).toBe("Keeper architecture");
  });

  it("sends what was typed in the search box, and does not filter the reply", async () => {
    const { dialog } = await opened();
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_SEARCH_LABEL), {
      target: { value: "tag:project" },
    });
    // `tag:` is a question about the tag hierarchy, and the index answers it.
    await waitFor(() =>
      expect(sessionsRefCandidates).toHaveBeenLastCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "tag:project",
      ),
    );
    // Every row the reply carried is still on screen: no second filter here.
    expect(within(dialog).getAllByRole("listitem")).toHaveLength(candidates().length);
  });

  it("ignores a reply a later keystroke has superseded", async () => {
    const { dialog } = await opened();
    let settleFirst: (vm: unknown) => void = () => {};
    sessionsRefCandidates.mockReturnValueOnce(
      new Promise((resolve) => {
        settleFirst = resolve;
      }),
    );
    const search = within(dialog).getByLabelText(SESSION_ADD_REF_SEARCH_LABEL);
    fireEvent.change(search, { target: { value: "re" } });
    await vi.advanceTimersByTimeAsync(150);
    sessionsRefCandidates.mockResolvedValueOnce({
      candidates: [candidate({ target: "report.md", label: "report.md" })],
      targets: ["references.md"],
      defaultTarget: "references.md",
      truncated: false,
    });
    fireEvent.change(search, { target: { value: "report" } });
    await vi.advanceTimersByTimeAsync(150);
    await screen.findByTestId(`${SESSION_ADD_REF_ROW_TESTID}-report.md`);
    // The slow answer to "re" lands last and must not win.
    settleFirst({
      candidates: candidates(),
      targets: ["references.md"],
      defaultTarget: "references.md",
      truncated: false,
    });
    await vi.advanceTimersByTimeAsync(50);
    expect(
      screen.queryByTestId(`${SESSION_ADD_REF_ROW_TESTID}-Keeper architecture`),
    ).not.toBeInTheDocument();
  });

  it("offers the promotion only on a workspace row, and sends it", async () => {
    const { dialog } = await opened();
    expect(within(dialog).queryByLabelText(SESSION_ADD_REF_PROMOTE_LABEL)).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-workspace/draft.md`));
    // Checked by default: a reference into scratch is a dangling link with a
    // date on it, so the safe answer is the one already made.
    expect(within(dialog).getByLabelText(SESSION_ADD_REF_PROMOTE_LABEL)).toBeChecked();
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    expect(sentRequest().promote).toBe(true);
  });

  it("lets the operator decline the promotion and point at the scratch file", async () => {
    const { dialog } = await opened();
    fireEvent.click(within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-workspace/draft.md`));
    fireEvent.click(within(dialog).getByLabelText(SESSION_ADD_REF_PROMOTE_LABEL));
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    expect(sentRequest().promote).toBe(false);
    expect(sentRequest().target).toBe("workspace/draft.md");
  });

  it("never promotes a typed link — a URL is in nobody's workspace", async () => {
    const { dialog } = await opened();
    fireEvent.click(within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-workspace/draft.md`));
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_URL_LABEL), {
      target: { value: "https://example.test/spec" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    expect(sentRequest()).toMatchObject({
      kind: "external",
      target: "https://example.test/spec",
      promote: false,
    });
  });

  it("hides the list while a link is typed, so two controls never compete", async () => {
    const { dialog } = await opened();
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_URL_LABEL), {
      target: { value: "https://example.test/spec" },
    });
    expect(within(dialog).queryByLabelText(SESSION_ADD_REF_LIST_LABEL)).not.toBeInTheDocument();
    expect(within(dialog).getByLabelText(SESSION_ADD_REF_SEARCH_LABEL)).toBeDisabled();
  });

  it("sends the operator's own words as the label when they gave one", async () => {
    const { dialog } = await opened();
    fireEvent.click(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-artifacts/report.pdf`),
    );
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_ALIAS_LABEL), {
      target: { value: "  The quarterly report  " },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    expect(sentRequest().label).toBe("The quarterly report");
  });

  it("marks a destination keeper would have to create, and still offers it", async () => {
    sessionsRefCandidates.mockResolvedValue({
      candidates: candidates(),
      targets: [],
      defaultTarget: "references.md",
      truncated: false,
    });
    const { dialog } = await opened();
    const menu = within(dialog).getByLabelText(SESSION_ADD_REF_FILE_LABEL);
    const options = within(menu as HTMLElement).getAllByRole("option");
    // Never an empty menu (UX-DR44) — and the one option says it does not exist.
    expect(options.map((o) => o.textContent)).toEqual([
      `references.md${SESSION_ADD_REF_NEW_FILE_SUFFIX}`,
    ]);
    fireEvent.click(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-artifacts/report.pdf`),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    await waitFor(() => expect(sessionsRefAdd).toHaveBeenCalled());
    expect(sentRequest().file).toBe("references.md");
  });

  it("keeps the file the operator chose when the list is read again", async () => {
    sessionsRefCandidates.mockResolvedValue({
      candidates: candidates(),
      targets: ["references.md", "reading.md"],
      defaultTarget: "references.md",
      truncated: false,
    });
    const { dialog } = await opened();
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_FILE_LABEL), {
      target: { value: "reading.md" },
    });
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_SEARCH_LABEL), {
      target: { value: "report" },
    });
    await vi.advanceTimersByTimeAsync(200);
    expect(within(dialog).getByLabelText(SESSION_ADD_REF_FILE_LABEL)).toHaveValue("reading.md");
  });

  it("shows the line keeper wrote and stays open for the next one", async () => {
    sessionsRefAdd.mockResolvedValue({
      file: "references.md",
      line: "- [The draft](artifacts/draft.md)",
      promoted: "artifacts/draft.md",
    });
    const { dialog } = await opened();
    fireEvent.click(within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-workspace/draft.md`));
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    // The line as written, not "Added" — the only version that shows keeper
    // wrote what was meant. And the copy is named, because bytes moved.
    expect(
      await within(dialog).findByText("- [The draft](artifacts/draft.md)"),
    ).toBeInTheDocument();
    expect(within(dialog).getByRole("status")).toHaveTextContent("artifacts/draft.md");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    // Cleared for the next one, which is why the dialog stayed open.
    expect(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-workspace/draft.md`),
    ).not.toHaveAttribute("aria-current");
    expect(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM })).toBeDisabled();
  });

  it("cannot add with nothing picked and nothing typed", async () => {
    const { dialog } = await opened();
    expect(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM })).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText(SESSION_ADD_REF_URL_LABEL), {
      target: { value: "   " },
    });
    expect(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM })).toBeDisabled();
  });

  it("says what to try when a search matched nothing", async () => {
    sessionsRefCandidates.mockResolvedValue({
      candidates: [],
      targets: ["references.md"],
      defaultTarget: "references.md",
      truncated: false,
    });
    mount();
    fireEvent.click(screen.getByRole("button", { name: SESSION_ADD_REF_LABEL }));
    expect(await screen.findByText(SESSION_ADD_REF_NONE)).toBeInTheDocument();
  });

  it("says the list is a prefix rather than letting it look complete", async () => {
    sessionsRefCandidates.mockResolvedValue({
      candidates: candidates(),
      targets: ["references.md"],
      defaultTarget: "references.md",
      truncated: true,
    });
    const { dialog } = await opened();
    expect(within(dialog).getByText(SESSION_ADD_REF_TRUNCATED)).toBeInTheDocument();
  });

  it("says keeper's refusal in keeper's own words", async () => {
    const { dialog } = await opened();
    sessionsRefAdd.mockRejectedValue({
      message: "workspace/draft.md is inside a session's workspace.",
    });
    fireEvent.click(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-artifacts/report.pdf`),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    expect(
      await within(dialog).findByText("workspace/draft.md is inside a session's workspace."),
    ).toBeInTheDocument();
    expect(within(dialog).queryByText(SESSION_ADD_REF_FAILED)).not.toBeInTheDocument();
  });

  it("falls back to keeper's sentence when the refusal carries none", async () => {
    const { dialog } = await opened();
    sessionsRefAdd.mockRejectedValue({});
    fireEvent.click(
      within(dialog).getByTestId(`${SESSION_ADD_REF_ROW_TESTID}-artifacts/report.pdf`),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_ADD_REF_CONFIRM }));
    expect(await within(dialog).findByText(SESSION_ADD_REF_FAILED)).toBeInTheDocument();
  });
});

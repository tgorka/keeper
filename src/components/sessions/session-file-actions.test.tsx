import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionEntryVm } from "@/lib/ipc/client";

const sessionsFileNew = vi.fn();
const sessionsFileNewKind = vi.fn();
const sessionsLogToday = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsFileNew: (root: unknown, session: unknown, parent: unknown, t: unknown, k: unknown) =>
    sessionsFileNew(root, session, parent, t, k),
  sessionsFileNewKind: (root: unknown, session: unknown, tag: unknown, slug: unknown) =>
    sessionsFileNewKind(root, session, tag, slug),
  sessionsLogToday: (root: unknown, session: unknown) => sessionsLogToday(root, session),
}));

import {
  SESSION_FILE_NEW_CONFIRM,
  SESSION_FILE_NEW_FOLDER_LABEL,
  SESSION_FILE_NEW_LABEL,
  SESSION_FILE_NEW_LOG_FAILED,
  SESSION_FILE_NEW_LOG_LABEL,
  SESSION_FILE_NEW_NAME_LABEL,
  SESSION_FILE_NEW_PROMPT_LABEL,
  SESSION_FILE_ROOT_LABEL,
  SessionFileActions,
} from "@/components/sessions/session-file-actions";
import { panelsStore } from "@/lib/stores/panels";

const NOW = Date.now();

/** The fence's own sentence (AD-113), abbreviated — only its presence matters here. */
const LOCK_SENTENCE = "…is inside a session's workspace — scratch that dies with the session.";

function entry(over: Partial<SessionEntryVm> & Pick<SessionEntryVm, "name">): SessionEntryVm {
  const relPath = over.relPath ?? over.name;
  return {
    relPath,
    parent: "",
    depth: 1,
    isDir: false,
    subpath: `60-sessions/active/2026-08-10-keeper/${relPath}`,
    absolutePath: `/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/${relPath}`,
    size: { bytes: 2048, label: "2.0 kB" },
    mtimeMs: NOW - 60_000,
    sync: { status: "synced", detail: null },
    locked: null,
    undeletable: null,
    ...over,
  };
}

/** One writable folder, one fenced one, and a file — the folder menu's whole question. */
function entries(): SessionEntryVm[] {
  return [
    entry({ name: "artifacts", isDir: true, size: null }),
    entry({ name: "workspace", isDir: true, size: null, locked: LOCK_SENTENCE }),
    entry({ name: "about.md" }),
  ];
}

function mount(over: Partial<React.ComponentProps<typeof SessionFileActions>> = {}) {
  const onChanged = vi.fn();
  const result = render(
    <SessionFileActions
      rootId="tgdrive"
      sessionId="active/2026-08-10-keeper"
      shape="flat"
      entries={entries()}
      onChanged={onChanged}
      {...over}
    />,
  );
  return { ...result, onChanged };
}

beforeEach(() => {
  panelsStore.setState(panelsStore.getInitialState(), true);
  sessionsFileNew.mockResolvedValue("60-sessions/active/2026-08-10-keeper/a-thought.md");
  sessionsFileNewKind.mockResolvedValue(
    "60-sessions/active/2026-08-10-keeper/2026-08-14-0930-log.md",
  );
  sessionsLogToday.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionFileActions", () => {
  it("writes a flat session's log as a tagged file and opens it", async () => {
    const { onChanged } = mount();
    screen.getByRole("button", { name: SESSION_FILE_NEW_LOG_LABEL }).click();
    await waitFor(() => {
      // The tag, not a name: keeper chooses both, which is what makes the file
      // findable by the zone's own Log space.
      expect(sessionsFileNewKind).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "log",
        "",
      );
    });
    expect(sessionsLogToday).not.toHaveBeenCalled();
    // Written AND opened — through the one file target, on Rust's own subpath.
    await waitFor(() => {
      expect(panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target).toEqual({
        kind: "file",
        profileId: "tgdrive",
        relativePath: "60-sessions/active/2026-08-10-keeper/2026-08-14-0930-log.md",
      });
    });
    expect(onChanged).toHaveBeenCalled();
  });

  it("appends to the README's own log when the session is folder-shaped", async () => {
    const { onChanged } = mount({ shape: "folder" });
    screen.getByRole("button", { name: SESSION_FILE_NEW_LOG_LABEL }).click();
    await waitFor(() => {
      expect(sessionsLogToday).toHaveBeenCalledWith("tgdrive", "active/2026-08-10-keeper");
    });
    // Same button, same words, the other contract's command.
    expect(sessionsFileNewKind).not.toHaveBeenCalled();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    // Nothing to open: the README was already reachable.
    expect(panelsStore.getState().panels.find((p) => p.target?.kind === "file")).toBeUndefined();
  });

  it("offers no prompt button where a prompt is a folder, not a tag", () => {
    mount({ shape: "folder" });
    expect(
      screen.queryByRole("button", { name: SESSION_FILE_NEW_PROMPT_LABEL }),
    ).not.toBeInTheDocument();
    // The other two survive: a folder session still logs and still adds files.
    expect(screen.getByRole("button", { name: SESSION_FILE_NEW_LOG_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: SESSION_FILE_NEW_LABEL })).toBeInTheDocument();
  });

  it("writes a prompt with the prompt tag", async () => {
    mount();
    screen.getByRole("button", { name: SESSION_FILE_NEW_PROMPT_LABEL }).click();
    await waitFor(() => {
      expect(sessionsFileNewKind).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "prompt",
        "",
      );
    });
  });

  it("creates a named file in the picked folder and opens what came back", async () => {
    const { onChanged } = mount();
    screen.getByRole("button", { name: SESSION_FILE_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(SESSION_FILE_NEW_NAME_LABEL), {
      target: { value: "A thought" },
    });
    fireEvent.change(within(dialog).getByLabelText(SESSION_FILE_NEW_FOLDER_LABEL), {
      target: { value: "artifacts" },
    });
    within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM }).click();
    await waitFor(() => {
      // The title goes over as typed — the slug is Rust's business (AD-65).
      expect(sessionsFileNew).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "artifacts",
        "A thought",
        "md",
      );
    });
    await waitFor(() => {
      expect(panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target).toEqual({
        kind: "file",
        profileId: "tgdrive",
        relativePath: "60-sessions/active/2026-08-10-keeper/a-thought.md",
      });
    });
    expect(onChanged).toHaveBeenCalled();
  });

  it("cannot create a file with no title", async () => {
    mount();
    screen.getByRole("button", { name: SESSION_FILE_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM })).toBeDisabled();
    // Whitespace is not a title either.
    fireEvent.change(within(dialog).getByLabelText(SESSION_FILE_NEW_NAME_LABEL), {
      target: { value: "   " },
    });
    expect(within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM })).toBeDisabled();
  });

  it("offers the session root and every folder keeper may write to, and no other", async () => {
    mount();
    screen.getByRole("button", { name: SESSION_FILE_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    const folder = within(dialog).getByLabelText(SESSION_FILE_NEW_FOLDER_LABEL);
    const options = within(folder as HTMLElement).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([SESSION_FILE_ROOT_LABEL, "artifacts"]);
    // `workspace/` is the fence keeper never writes through — offering it would
    // be a control that exists to fail.
    expect(options.map((o) => (o as HTMLOptionElement).value)).not.toContain("workspace");
  });

  it("says keeper's refusal in keeper's own words, once", async () => {
    mount();
    sessionsFileNewKind.mockRejectedValue({ message: "That session has no root on disk." });
    screen.getByRole("button", { name: SESSION_FILE_NEW_LOG_LABEL }).click();
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("That session has no root on disk.");
    expect(screen.queryByText(SESSION_FILE_NEW_LOG_FAILED)).not.toBeInTheDocument();
    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("falls back to keeper's sentence when the refusal carries none", async () => {
    mount();
    sessionsFileNewKind.mockRejectedValue({});
    screen.getByRole("button", { name: SESSION_FILE_NEW_LOG_LABEL }).click();
    expect(await screen.findByRole("status")).toHaveTextContent(SESSION_FILE_NEW_LOG_FAILED);
  });
});

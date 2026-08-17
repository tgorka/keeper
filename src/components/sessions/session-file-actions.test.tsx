import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionEntryVm } from "@/lib/ipc/client";

const sessionsFileNew = vi.fn();
const sessionsFileNewKind = vi.fn();
const sessionsLogToday = vi.fn();
const sessionsDirNew = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsFileNew: (root: unknown, session: unknown, parent: unknown, t: unknown, k: unknown) =>
    sessionsFileNew(root, session, parent, t, k),
  sessionsFileNewKind: (root: unknown, session: unknown, tag: unknown, slug: unknown) =>
    sessionsFileNewKind(root, session, tag, slug),
  sessionsLogToday: (root: unknown, session: unknown) => sessionsLogToday(root, session),
  sessionsDirNew: (root: unknown, session: unknown, rel: unknown) =>
    sessionsDirNew(root, session, rel),
}));

import {
  SESSION_DIR_NEW_FAILED,
  SESSION_DIR_NEW_LABEL,
  SESSION_DIR_NEW_NAME_LABEL,
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

/**
 * The heading under a parent that owns the one create-in-flight flag.
 *
 * `busy` is a PROP now, not this component's state: every writable space below
 * offers a create through the same command and with the same empty title, so
 * the flag that removes the colliding press has to span both surfaces and
 * `SessionDetail` holds it. This harness plays that parent. That the two
 * components actually share ONE flag is `session-detail.test.tsx`'s claim.
 */
function Harness(over: Partial<React.ComponentProps<typeof SessionFileActions>>) {
  const [busy, setBusy] = useState(false);
  return (
    <SessionFileActions
      rootId="tgdrive"
      sessionId="active/2026-08-10-keeper"
      shape="flat"
      entries={entries()}
      busy={busy}
      onBusy={setBusy}
      onChanged={() => {}}
      {...over}
    />
  );
}

function mount(over: Partial<React.ComponentProps<typeof SessionFileActions>> = {}) {
  const onChanged = vi.fn();
  const result = render(<Harness onChanged={onChanged} {...over} />);
  return { ...result, onChanged };
}

beforeEach(() => {
  panelsStore.setState(panelsStore.getInitialState(), true);
  sessionsFileNew.mockResolvedValue("60-sessions/active/2026-08-10-keeper/a-thought.md");
  sessionsFileNewKind.mockResolvedValue(
    "60-sessions/active/2026-08-10-keeper/2026-08-14-0930-log.md",
  );
  sessionsLogToday.mockResolvedValue(undefined);
  sessionsDirNew.mockResolvedValue(undefined);
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

  /**
   * The gate this replaces (Story 50.1). `New prompt` used to be absent under
   * `shape === "folder"`, and the reason recorded beside it — "a folder-shaped
   * session keeps its prompts in `prompts/`, where the kind is the directory;
   * a tagged file there would be filed twice" — was never true of the reader:
   * `pool::read_one` derives a kind from tags alone (AD-120). The real reason
   * was the writer, which put its stamped file in the session ROOT; it now asks
   * `shape::kind_dir` and writes into `prompts/`, which is exactly where that
   * shape's pool reads. So the button is offered, and it sends the same kind it
   * sends on a flat session — where the file lands is Rust's answer and
   * `shape.rs`'s own tests own it.
   *
   * This case is what fails if someone re-adds the gate.
   */
  it("offers the prompt button on a folder-shaped session, where prompts/ is read", async () => {
    mount({ shape: "folder" });

    const prompt = screen.getByRole("button", { name: SESSION_FILE_NEW_PROMPT_LABEL });
    expect(prompt).toBeInTheDocument();
    prompt.click();

    await waitFor(() => {
      expect(sessionsFileNewKind).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "prompt",
        "",
      );
    });
    // The other two are unchanged: a folder session still logs — through the
    // README, which the case above owns — and still adds files.
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

  /**
   * Row 12. The verb is offered beside its three siblings and sends the path as
   * typed: the fold (`Interview Kit` → `interview-kit`) is Rust's, and a slug
   * composed here would be the second namer (AD-65).
   */
  it("creates a folder from the path typed, and re-reads the tree", async () => {
    const { onChanged, rerender } = mount();
    screen.getByRole("button", { name: SESSION_DIR_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(SESSION_DIR_NEW_NAME_LABEL), {
      target: { value: "Interview Kit" },
    });
    within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM }).click();
    await waitFor(() => {
      expect(sessionsDirNew).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "Interview Kit",
      );
    });
    // Nothing to open: a folder is a row in the tree, not a document.
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(panelsStore.getState().panels.find((p) => p.target?.kind === "file")).toBeUndefined();
    expect(sessionsFileNew).not.toHaveBeenCalled();

    // …and the other half of row 12, which the re-read assertion alone does not
    // buy: the row has to become a DESTINATION. `entries` is the parent's, so
    // this plays the re-read the component just asked for and then creates a
    // file into what came back. Without it, "the new folder is a row, and a
    // file can be created into it" was asserted nowhere but in the dev mock.
    rerender(
      <Harness
        onChanged={onChanged}
        entries={[...entries(), entry({ name: "interview-kit", isDir: true, size: null })]}
      />,
    );
    screen.getByRole("button", { name: SESSION_FILE_NEW_LABEL }).click();
    const second = await screen.findByRole("dialog");
    const folder = within(second).getByLabelText<HTMLSelectElement>(SESSION_FILE_NEW_FOLDER_LABEL);
    const offered = within(folder).getAllByRole<HTMLOptionElement>("option");
    expect(offered.map((option) => option.value)).toEqual(["", "artifacts", "interview-kit"]);
    fireEvent.change(within(second).getByLabelText(SESSION_FILE_NEW_NAME_LABEL), {
      target: { value: "Questions" },
    });
    fireEvent.change(folder, { target: { value: "interview-kit" } });
    within(second).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM }).click();
    await waitFor(() => {
      expect(sessionsFileNew).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "interview-kit",
        "Questions",
        "md",
      );
    });
  });

  /**
   * Offered under both contracts, and it WORKS under both: a container is a
   * container in either.
   *
   * Driven to the command rather than stopping at `toBeInTheDocument`, because
   * this button is rendered unconditionally — an assertion that it exists could
   * not fail unless somebody deleted it, and the thing being guarded is the
   * absence of a `shape === "flat"` gate like the one *New prompt* used to
   * carry. Pressing it through is what a re-introduced gate would fail.
   */
  it("makes a folder on a folder-shaped session too", async () => {
    mount({ shape: "folder" });
    screen.getByRole("button", { name: SESSION_DIR_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(SESSION_DIR_NEW_NAME_LABEL), {
      target: { value: "log" },
    });
    within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM }).click();
    await waitFor(() => {
      expect(sessionsDirNew).toHaveBeenCalledWith("tgdrive", "active/2026-08-10-keeper", "log");
    });
  });

  it("cannot create a folder with no path", async () => {
    mount();
    screen.getByRole("button", { name: SESSION_DIR_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM })).toBeDisabled();
    // Whitespace is not a path either — Rust is never asked a question nobody
    // answered.
    fireEvent.change(within(dialog).getByLabelText(SESSION_DIR_NEW_NAME_LABEL), {
      target: { value: "   " },
    });
    expect(within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM })).toBeDisabled();
    expect(sessionsDirNew).not.toHaveBeenCalled();
  });

  /**
   * The fence's own words reach the operator (AD-113). The dialog stays open on
   * a refusal, because the path that was typed is the thing to correct.
   */
  it("says the workspace refusal in Rust's words and keeps the dialog open", async () => {
    mount();
    sessionsDirNew.mockRejectedValue({
      message:
        "workspace is inside the session's workspace — scratch that is not versioned, not synced, and dies with the session.",
    });
    screen.getByRole("button", { name: SESSION_DIR_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(SESSION_DIR_NEW_NAME_LABEL), {
      target: { value: "workspace" },
    });
    within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM }).click();
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("scratch that is not versioned");
    expect(screen.queryByText(SESSION_DIR_NEW_FAILED)).not.toBeInTheDocument();
    // Said once: the sentence lives in the dialog while the dialog is open.
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("falls back to keeper's sentence when the folder refusal carries none", async () => {
    mount();
    sessionsDirNew.mockRejectedValue({});
    screen.getByRole("button", { name: SESSION_DIR_NEW_LABEL }).click();
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(SESSION_DIR_NEW_NAME_LABEL), {
      target: { value: "log" },
    });
    within(dialog).getByRole("button", { name: SESSION_FILE_NEW_CONFIRM }).click();
    expect(await screen.findByRole("status")).toHaveTextContent(SESSION_DIR_NEW_FAILED);
  });
});

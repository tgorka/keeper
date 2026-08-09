import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteWriteVm, RecordingNoteTargetVm } from "@/lib/ipc/client";

const notesSave =
  vi.fn<
    (id: string, text: string, rev: string, frontmatter: string | null) => Promise<NoteWriteVm>
  >();
const recordingNoteTargets =
  vi.fn<(sessionId: string) => Promise<RecordingNoteTargetVm[] | null>>();
const revealPath = vi.fn();
const recordingOpenPath = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  notesSave: (id: string, text: string, rev: string, frontmatter: string | null) =>
    notesSave(id, text, rev, frontmatter),
  recordingNoteTargets: (sessionId: string) => recordingNoteTargets(sessionId),
  revealPath: (path: string) => revealPath(path),
  recordingOpenPath: (path: string) => recordingOpenPath(path),
}));

import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { PropertiesPanel, readFrontmatter } from "./properties-panel";

/** A block holding a key keeper has never heard of, written in a style keeper
 *  would not have chosen. Both must survive an unrelated edit. */
const BLOCK = [
  "---",
  "id: 01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "tags:",
  "  - work",
  "  - clients/acme",
  "pinned: false",
  "mood:   'pensive, mostly'",
  "---",
  "",
].join("\n");

/** The buffer, which the panel never edits but always writes. */
const BODY = "\n# Standing meeting\n\nunsaved keystrokes\n";

beforeEach(() => {
  vi.clearAllMocks();
  notesSave.mockResolvedValue({
    rev: "rev-2",
    path: "notes/standing.md",
    frontmatter: BLOCK,
    conflictCopy: null,
  });
  recordingNoteTargets.mockResolvedValue(null);
  revealPath.mockResolvedValue(undefined);
  recordingOpenPath.mockResolvedValue(undefined);
  // jsdom lacks a clipboard by default.
  Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

/**
 * A recording note's keeper-owned tail (Story 42.4): the immutable session id,
 * the session folder, and its files — every path relative to the recordings
 * destination root, because FR-145 keeps absolute ones out of a synced file.
 */
const RECORDING_BLOCK = [
  "---",
  "title: Standup",
  "session: 01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS",
  "recording: recordings/2026/2026-08-08 1552 standup",
  "files:",
  "  - recordings/2026/2026-08-08 1552 standup/screen-0000.mov",
  "  - recordings/2026/2026-08-08 1552 standup/manifest.json",
  "---",
  "",
].join("\n");

/** Where Rust says that session is NOW — a folder Story 40.4 has since renamed. */
const ROOT = "/Users/alice/Movies/keeper";
const FOLDER = "recordings/2026/2026-08-08 1552 standup retitled";
const TARGETS: RecordingNoteTargetVm[] = [
  { relativePath: FOLDER, absolutePath: `${ROOT}/${FOLDER}`, kind: "folder" },
  {
    relativePath: `${FOLDER}/screen-0000.mov`,
    absolutePath: `${ROOT}/${FOLDER}/screen-0000.mov`,
    kind: "video",
  },
  {
    relativePath: `${FOLDER}/manifest.json`,
    absolutePath: `${ROOT}/${FOLDER}/manifest.json`,
    kind: "file",
  },
];

/** The dropdown for one of the note's paths, opened. Radix opens on pointer-down. */
async function openActions(relativePath: string): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", { name: `Actions for ${relativePath}` });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

function renderPanel(frontmatter: string = BLOCK) {
  return render(
    <PropertiesPanel
      frontmatter={frontmatter}
      body={BODY}
      subscriptionId="sub-1"
      baseRev="rev-1"
      onSaved={() => {}}
    />,
  );
}

describe("readFrontmatter", () => {
  it("infers a control from each value's shape", () => {
    const parsed = readFrontmatter(BLOCK);
    expect(parsed.unparsed).toBe(false);
    expect(parsed.entries.map((entry) => [entry.key, entry.kind])).toEqual([
      ["id", "text"],
      ["tags", "list"],
      ["pinned", "boolean"],
      ["mood", "text"],
    ]);
    expect(parsed.entries[1].items).toEqual(["work", "clients/acme"]);
    expect(parsed.entries[3].text).toBe("pensive, mostly");
  });

  it("reads a block delivered on its own, exactly as it reads one at the head of a note", () => {
    const alone = readFrontmatter(BLOCK);
    const inDocument = readFrontmatter(`${BLOCK}\n# body\n`);
    expect(inDocument.entries.map((entry) => entry.valueFrom)).toEqual(
      alone.entries.map((entry) => entry.valueFrom),
    );
  });

  it("reports a block it will not touch rather than rewriting it", () => {
    const parsed = readFrontmatter("---\nweird: !!str [a\n---\n");
    expect(parsed.unparsed).toBe(true);
  });
});

describe("PropertiesPanel", () => {
  it("preserves every other key byte-for-byte when one key is edited", async () => {
    renderPanel();

    fireEvent.click(screen.getByRole("switch", { name: "pinned" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const [subscription, body, baseRev, written] = notesSave.mock.calls[0];
    expect(subscription).toBe("sub-1");
    expect(baseRev).toBe("rev-1");
    // The block is the fourth argument, because the block is what this panel owns.
    expect(written).not.toBeNull();
    const block = written ?? "";
    // The one key that changed, changed.
    expect(block).toContain("pinned: true");
    // Everything else — including a key keeper does not know, its odd spacing
    // and its single quotes — is exactly the bytes that came in (FR-121).
    expect(block).toContain("mood:   'pensive, mostly'");
    expect(block).toContain("id: 01ARZ3NDEKTSV4RRFFQ69G5FAV");
    expect(block).toContain("tags:\n  - work\n  - clients/acme\n");
    expect(block.replace("pinned: true", "pinned: false")).toBe(BLOCK);
    // And the body goes along untouched: one write covers the whole note, so a
    // property edit must not discard what the user has typed since the last save.
    expect(body).toBe(BODY);
    expect(block).not.toContain("Standing meeting");
  });

  it("writes a list edit in the style the note already used", async () => {
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: "Remove work from tags" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const block = notesSave.mock.calls[0][3] ?? "";
    expect(block).toContain("tags:\n  - clients/acme\npinned: false");
    expect(block).toContain("mood:   'pensive, mostly'");
  });

  it("creates a block for a note that has none, without touching the body", async () => {
    renderPanel("");

    fireEvent.change(screen.getByRole("textbox", { name: "New property name" }), {
      target: { value: "pinned" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const [, body, , written] = notesSave.mock.calls[0];
    expect(written).toBe('---\npinned: ""\n---\n');
    expect(body).toBe(BODY);
  });

  it("never edits the ULID, because links resolve through it", () => {
    renderPanel();
    expect(screen.queryByRole("textbox", { name: "id" })).toBeNull();
  });
});

describe("PropertiesPanel — a recording note's file actions", () => {
  const FILE = "recordings/2026/2026-08-08 1552 standup/screen-0000.mov";
  const MANIFEST = "recordings/2026/2026-08-08 1552 standup/manifest.json";
  const NOTE_FOLDER = "recordings/2026/2026-08-08 1552 standup";

  it("leaves a note that is not about a recording exactly as it was", async () => {
    renderPanel();

    await waitFor(() => expect(screen.getByRole("switch", { name: "pinned" })).toBeInTheDocument());
    // No `session:`, so no dropdown anywhere — and `tags:`, a list like
    // `files:`, keeps its own editable chips.
    expect(screen.queryByRole("button", { name: /^Actions for/ })).toBeNull();
    expect(recordingNoteTargets).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Remove work from tags" })).toBeInTheDocument();
  });

  it("offers one dropdown per path and resolves them by session id, not by path", async () => {
    recordingNoteTargets.mockResolvedValue(TARGETS);
    renderPanel(RECORDING_BLOCK);

    await waitFor(() => expect(recordingNoteTargets).toHaveBeenCalledTimes(1));
    expect(recordingNoteTargets).toHaveBeenCalledWith(
      "01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS",
    );
    // One per path: the folder and its two files. `title:` and `session:` are
    // not paths and get nothing.
    expect(await screen.findAllByRole("button", { name: /^Actions for/ })).toHaveLength(3);
    // The visible text is the note's own relative path even though the folder
    // has since been renamed (FR-145): no absolute path is ever on screen.
    expect(screen.getByText(FILE)).toBeInTheDocument();
    expect(screen.queryByText(new RegExp(ROOT))).toBeNull();
  });

  it("offers Preview for a video and for nothing else", async () => {
    recordingNoteTargets.mockResolvedValue(TARGETS);
    renderPanel(RECORDING_BLOCK);

    const video = await openActions(FILE);
    expect(within(video).getByRole("menuitem", { name: "Preview" })).toBeInTheDocument();
    fireEvent.click(within(video).getByRole("menuitem", { name: "Preview" }));
    // The file Rust says is there NOW, not the one the note remembers.
    expect(recordingOpenPath).toHaveBeenCalledWith(`${ROOT}/${FOLDER}/screen-0000.mov`);

    // A manifest opens a text editor, which is not a preview of anything.
    const manifest = await openActions(MANIFEST);
    expect(within(manifest).queryByRole("menuitem", { name: "Preview" })).toBeNull();
    expect(
      within(manifest).getByRole("menuitem", { name: "Reveal in Finder" }),
    ).toBeInTheDocument();
    fireEvent.keyDown(manifest, { key: "Escape" });

    // And a folder is what Reveal is for.
    const folder = await openActions(NOTE_FOLDER);
    expect(within(folder).queryByRole("menuitem", { name: "Preview" })).toBeNull();
    fireEvent.click(within(folder).getByRole("menuitem", { name: "Reveal in Finder" }));
    expect(revealPath).toHaveBeenCalledWith(`${ROOT}/${FOLDER}`);
  });

  it("hides Reveal where there is no file manager to reveal into", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });
    recordingNoteTargets.mockResolvedValue(TARGETS);
    renderPanel(RECORDING_BLOCK);

    const menu = await openActions(FILE);

    expect(within(menu).queryByRole("menuitem", { name: "Reveal in Finder" })).toBeNull();
    // The two actions that do not need a file manager are untouched.
    expect(within(menu).getByRole("menuitem", { name: "Preview" })).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Copy path" })).toBeInTheDocument();
  });

  it("copies the absolute path, never the relative text the note shows", async () => {
    recordingNoteTargets.mockResolvedValue(TARGETS);
    renderPanel(RECORDING_BLOCK);

    const menu = await openActions(FILE);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy path" }));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(`${ROOT}/${FOLDER}/screen-0000.mov`);
    expect(navigator.clipboard.writeText).not.toHaveBeenCalledWith(FILE);
  });

  it("offers nothing that would open nothing for a session the index cannot place", async () => {
    recordingNoteTargets.mockResolvedValue(null);
    renderPanel(RECORDING_BLOCK);

    // The note still says what it says.
    expect(await screen.findByText(FILE)).toBeInTheDocument();
    const menu = await openActions(FILE);

    // A Reveal that opens nothing tells the reader the recording is there and
    // then fails at the moment they believed it; absence says the true thing.
    expect(within(menu).queryByRole("menuitem", { name: "Reveal in Finder" })).toBeNull();
    expect(within(menu).queryByRole("menuitem", { name: "Preview" })).toBeNull();
    // Copy path survives, because copying what is on screen is never wrong.
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy path" }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(FILE);
  });
});

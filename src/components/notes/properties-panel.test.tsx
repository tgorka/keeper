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
const tagsVocabulary = vi.fn<() => Promise<{ entries: { path: string; count: number }[] }>>();
const recordingSessionMeta = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  notesSave: (id: string, text: string, rev: string, frontmatter: string | null) =>
    notesSave(id, text, rev, frontmatter),
  recordingNoteTargets: (sessionId: string) => recordingNoteTargets(sessionId),
  revealPath: (path: string) => revealPath(path),
  recordingOpenPath: (path: string) => recordingOpenPath(path),
  recordingSessionMeta: (folder: string) => recordingSessionMeta(folder),
  tagsVocabulary: () => tagsVocabulary(),
}));

import { tagComboboxAlreadyChosen, tagComboboxCreate } from "@/components/notes/tag-combobox";
import { OVERFLOW_PANEL_LABEL, OVERFLOW_TRIGGER_LABEL } from "@/components/ui/overflow-value";
import {
  COLUMN_FITTED_VALUE_TEXT,
  COLUMN_RESIZER_LABEL,
  COLUMN_TEMPLATE_VAR,
} from "@/components/ui/resizable-columns";
import { COLUMN_WIDTH_COOKIE, MIN_COLUMN_WIDTH, readColumnWidths } from "@/lib/column-widths";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { recordingMetaStore } from "@/lib/stores/recording-meta";
import { ELLIPSIS } from "@/lib/truncate";
import { withRect, withTextLayout } from "@/test/layout";
import {
  ADD_NOTE_TAG,
  PROPERTIES_COLUMN_LABEL,
  PROPERTY_KEY_COLUMN,
  PropertiesPanel,
  RECORD_ANOTHER_FAULT_TESTID,
  RECORD_ANOTHER_TESTID,
  RECORD_ANOTHER_UNREADABLE,
  readFrontmatter,
  recordingsTagRefusal,
  UNPARSED_BLOCK_LABEL,
} from "./properties-panel";

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
  tagsVocabulary.mockResolvedValue({ entries: [] });
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

/**
 * Story 44.12 — the panel the owner met AD-83's failure in.
 *
 * Two things here are not provable in jsdom and are asserted through the
 * closest real thing instead. Fitting is `fit-content(50%)` handed to the
 * layout engine, so the assertion is that keeper asks for it — whether the
 * engine then measures the glyphs correctly is the engine's contract, not
 * keeper's. And overflow is `scrollWidth > clientWidth`, which jsdom answers
 * `0 > 0` for every element ever rendered; `withTextLayout` answers those two
 * properties from the element's own text so the component's REAL effect, REAL
 * comparison and REAL conditional render run. Nothing about the component is
 * stubbed. What remains unproven is named at the bottom of the spec.
 */
describe("PropertiesPanel — columns you can size", () => {
  afterEach(() => {
    // The rule guards production code against clobbering the cookie jar. Here
    // the whole point is to clear the width this suite persisted, so the next
    // test starts from the default rather than from its neighbour's drag.
    // biome-ignore lint/suspicious/noDocumentCookie: clearing is the intent
    document.cookie = `${COLUMN_WIDTH_COOKIE}=; path=/; max-age=0`;
  });

  /** The seam, with a known position, ready to be dragged from x. */
  function seamAt(left: number): HTMLElement {
    const seam = screen.getByRole("separator", {
      name: `${COLUMN_RESIZER_LABEL} ${PROPERTIES_COLUMN_LABEL}`,
    });
    withRect(seam, left);
    return seam;
  }

  /** The grid whose template is the whole visible result of a resize. */
  function template(seam: HTMLElement): string {
    const grid = seam.parentElement;
    if (grid === null) {
      throw new Error("the seam is not inside the grid it sizes");
    }
    return grid.style.getPropertyValue(COLUMN_TEMPLATE_VAR);
  }

  it("fits the key column to its content until somebody says otherwise", () => {
    renderPanel();

    // Not a number, and deliberately: the fitted width is the one the layout
    // engine measures from the real glyphs. A `w-32` here is the bug.
    expect(template(seamAt(0))).toContain("fit-content(50%)");
    expect(readColumnWidths(document.cookie)).toEqual({});
  });

  it("moves the boundary to where the pointer took it, and remembers", () => {
    const panel = renderPanel();
    const seam = seamAt(160);

    fireEvent.pointerDown(seam, { button: 0, pointerId: 1, clientX: 160 });
    fireEvent.pointerMove(seam, { pointerId: 1, clientX: 260 });
    fireEvent.pointerUp(seam, { pointerId: 1 });

    expect(template(seam)).toBe("260px 0px minmax(0, 1fr)");
    expect(readColumnWidths(document.cookie)[PROPERTY_KEY_COLUMN]).toBe(260);

    // The reload jsdom has. Nothing is cached in module scope, so a fresh mount
    // reads the cookie exactly as a relaunched window would.
    panel.unmount();
    renderPanel();

    expect(template(seamAt(260))).toBe("260px 0px minmax(0, 1fr)");
  });

  it("ignores a pointer that never grabbed the seam", () => {
    renderPanel();
    const seam = seamAt(160);

    // A drag that started somewhere else, passing over. Without the guard the
    // column snaps to the cursor of any pointer that crosses it.
    fireEvent.pointerMove(seam, { pointerId: 1, clientX: 400 });

    expect(template(seam)).toContain("fit-content(50%)");
  });

  it("moves the boundary from the keyboard, and back to fitted", () => {
    renderPanel();
    const seam = seamAt(160);

    fireEvent.keyDown(seam, { key: "ArrowRight" });
    expect(template(seam)).toBe("168px 0px minmax(0, 1fr)");

    fireEvent.keyDown(seam, { key: "ArrowLeft", shiftKey: true });
    expect(template(seam)).toBe("136px 0px minmax(0, 1fr)");
    expect(seam).toHaveAttribute("aria-valuenow", "136");

    // Home is the door out of a width somebody regrets, and it has to clear the
    // cookie too — otherwise the next launch restores the regret.
    fireEvent.keyDown(seam, { key: "Home" });
    expect(template(seam)).toContain("fit-content(50%)");
    expect(seam).toHaveAttribute("aria-valuetext", COLUMN_FITTED_VALUE_TEXT);
    expect(readColumnWidths(document.cookie)).toEqual({});
  });

  it("refuses to drag a column down to nothing", () => {
    renderPanel();
    const seam = seamAt(160);

    fireEvent.pointerDown(seam, { button: 0, pointerId: 1, clientX: 160 });
    fireEvent.pointerMove(seam, { pointerId: 1, clientX: -600 });

    // A column at zero has swallowed its own content AND the handle that would
    // bring it back.
    expect(template(seam)).toBe(`${MIN_COLUMN_WIDTH}px 0px minmax(0, 1fr)`);
  });
});

describe("PropertiesPanel — content you can read", () => {
  let restoreLayout: (() => void) | null = null;

  afterEach(() => {
    restoreLayout?.();
    restoreLayout = null;
  });

  /** Give the pane `px` of room and let the text measure itself into it. */
  function pane(px: number): void {
    restoreLayout = withTextLayout(px);
  }

  it("says nothing extra about a value that fits", () => {
    pane(1000);
    renderPanel();

    // The affordance is a tab stop. A panel that grows one per property is a
    // worse panel than the one with the tooltips.
    expect(screen.queryByRole("button", { name: "01ARZ3NDEKTSV4RRFFQ69G5FAV" })).toBeNull();
    expect(screen.getByText("01ARZ3NDEKTSV4RRFFQ69G5FAV")).toBeInTheDocument();
  });

  it("offers the whole of a value the column cut", () => {
    // 26 characters of ULID at 8px each, in 120px of column.
    pane(120);
    renderPanel();

    const trigger = screen.getByRole("button", { name: "01ARZ3NDEKTSV4RRFFQ69G5FAV" });
    fireEvent.click(trigger);

    const full = screen.getByLabelText(`${OVERFLOW_PANEL_LABEL}: id`);
    expect(full).toHaveTextContent("01ARZ3NDEKTSV4RRFFQ69G5FAV");
  });

  it("shows a recording path in full, from a control and not a tooltip", async () => {
    pane(120);
    recordingNoteTargets.mockResolvedValue(TARGETS);
    renderPanel(RECORDING_BLOCK);

    const file = "recordings/2026/2026-08-08 1552 standup/screen-0000.mov";
    const trigger = await screen.findByRole("button", { name: file });

    // The failure this replaces: `title=` is a tooltip, and a tooltip does not
    // exist for a keyboard, a touch screen, or a hand that is not hovering.
    expect(trigger).not.toHaveAttribute("title");
    expect(document.querySelector("[title]")).toBeNull();

    // What a browser dispatches when Enter is pressed on a focused button.
    trigger.focus();
    expect(document.activeElement).toBe(trigger);
    fireEvent.click(trigger);

    const panel = await screen.findByRole("dialog");
    // The COMPLETE path, not the visible head of it.
    expect(within(panel).getByLabelText(new RegExp(OVERFLOW_PANEL_LABEL))).toHaveTextContent(file);

    // And a value taller than the panel is reachable: the region scrolls and
    // takes focus, so arrow keys reach the bottom of it.
    const region = within(panel).getByLabelText(new RegExp(OVERFLOW_PANEL_LABEL));
    expect(region).toHaveAttribute("tabindex", "0");
    expect(region.className).toContain("overflow-y-auto");
    expect(region.className).toContain("max-h-64");

    // Escape closes it and gives focus back, so the keyboard is never stranded.
    fireEvent.keyDown(panel, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(trigger);
  });

  it("previews an unreadable block without cutting a character in half", () => {
    // 500 thumbs after an odd-length prefix, so code unit 400 lands INSIDE one.
    const block = `---\nweird: !!str [ab\n${"👍".repeat(500)}\n---\n`;
    // The bug being fixed, stated as a fact about the old implementation.
    expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/.test(block.slice(0, 400))).toBe(true);

    renderPanel(block);

    const preview = screen.getByText(/👍/);
    expect(preview.textContent).not.toContain("\uFFFD");
    expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/.test(preview.textContent ?? "")).toBe(false);
    expect(preview.textContent).toContain(ELLIPSIS);

    // And the rest of it is not gone — it is behind one control.
    fireEvent.click(
      screen.getByRole("button", { name: `${OVERFLOW_TRIGGER_LABEL} ${UNPARSED_BLOCK_LABEL}` }),
    );
    expect(
      screen.getByLabelText(`${OVERFLOW_PANEL_LABEL}: ${UNPARSED_BLOCK_LABEL}`),
    ).toHaveTextContent("👍");
  });
});

/**
 * Story 44.14 — the recording note's tags are the note's tags (FR-170).
 *
 * The block below is the shape `keeper-core`'s stub writer produces: what the
 * user typed leads, keeper's own bookkeeping trails it, and keeper's kind tag
 * is appended to the session's own tags rather than prepended. Every write
 * assertion below is stated as "the original block with exactly this one line
 * added or removed", because byte-preservation (FR-121) is the promise that
 * breaks silently and a `toContain` would not notice it breaking.
 */
describe("PropertiesPanel — a recording note's tags", () => {
  const SESSION = "01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS";

  /** A stub as written, carrying two tags of the user's and keeper's own. */
  const TAGGED = [
    "---",
    "title: Standup",
    "date: 2026-08-08",
    "participants: Alice, Bob",
    "tags:",
    "  - work",
    "  - client/acme",
    "  - recordings",
    `session: ${SESSION}`,
    "recording: recordings/2026/2026-08-08 1552 standup",
    "files:",
    "  - recordings/2026/2026-08-08 1552 standup/manifest.json",
    "---",
    "",
  ].join("\n");

  /** The vault's vocabulary, as 42.5's `tags_vocabulary` hands it over. */
  const VOCABULARY = ["work", "client/acme", "client/anvil", "recordings"];

  /** Open the chooser and hand back the field, once the vocabulary has landed. */
  async function openChooser(): Promise<HTMLElement> {
    fireEvent.click(screen.getByRole("button", { name: ADD_NOTE_TAG }));
    await waitFor(() => expect(tagsVocabulary).toHaveBeenCalled());
    return await screen.findByRole("combobox", { name: ADD_NOTE_TAG });
  }

  beforeEach(() => {
    tagsVocabulary.mockResolvedValue({
      entries: VOCABULARY.map((path) => ({ path, count: 1 })),
    });
  });

  it("adds a tag, and the rest of the block is the same bytes", async () => {
    renderPanel(TAGGED);
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "client/an" } });
    expect(await screen.findByRole("option", { name: "client/anvil" })).toBeInTheDocument();
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    // One write, through the panel's own path: the whole note, at the revision
    // the buffer opened at. A second write path is what this story refused to
    // add, and this is the assertion that would catch one.
    expect(notesSave.mock.calls[0][0]).toBe("sub-1");
    expect(notesSave.mock.calls[0][1]).toBe(BODY);
    expect(notesSave.mock.calls[0][2]).toBe("rev-1");
    // The list keeps the block style the note already used, the new tag goes
    // last, and removing that one line gives the file back exactly (FR-121).
    expect(notesSave.mock.calls[0][3] ?? "").toContain("  - recordings\n  - client/anvil\n");
    expect((notesSave.mock.calls[0][3] ?? "").replace("  - client/anvil\n", "")).toBe(TAGGED);
  });

  it("removes a tag the user put there, and the rest of the block is the same bytes", async () => {
    renderPanel(TAGGED);

    fireEvent.click(screen.getByRole("button", { name: "Remove work from tags" }));

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][3] ?? "").toBe(TAGGED.replace("  - work\n", ""));
  });

  it("refuses to remove the tag that makes the note findable, and says why", async () => {
    renderPanel(TAGGED);

    fireEvent.click(screen.getByRole("button", { name: "Remove recordings from tags" }));

    // Not a disabled × and not a silent no-op: the consequence is invisible
    // from this panel, so the panel is where it has to be said.
    expect(await screen.findByRole("alert")).toHaveTextContent(recordingsTagRefusal("recordings"));
    expect(notesSave).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Remove recordings from tags" })).toBeInTheDocument();
  });

  it("refuses whatever spelling of it the note carries", async () => {
    // Rust folds `Recordings`, `RECORDINGS` and `recordings ` onto one tag, so
    // a `===` here would have protected one of them and left the others
    // looking identical on screen and removable.
    renderPanel(TAGGED.replace("  - recordings", "  - Recordings"));

    fireEvent.click(screen.getByRole("button", { name: "Remove Recordings from tags" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(recordingsTagRefusal("Recordings"));
    expect(notesSave).not.toHaveBeenCalled();
  });

  it("lets an ordinary note drop a tag it happens to call recordings", async () => {
    // No `session:`, so this is somebody's own note and keeper owns no row in
    // it — the same rule that keeps a stranger's `files:` key a plain control.
    renderPanel(TAGGED.replace(`session: ${SESSION}\n`, ""));

    fireEvent.click(screen.getByRole("button", { name: "Remove recordings from tags" }));

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("never offers to edit the session id", async () => {
    renderPanel(TAGGED);

    // Everything about the recording resolves through it and none of that is
    // visible from here, so there is no control to type a typo into.
    expect(screen.queryByRole("textbox", { name: "session" })).toBeNull();
    expect(screen.queryByRole("spinbutton", { name: "session" })).toBeNull();
    // It is still readable and still copyable — protected, not hidden.
    expect(await screen.findByText(SESSION)).toBeInTheDocument();
  });

  it("will not make a second tag out of a different casing of one already here", async () => {
    renderPanel(TAGGED);
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "Work" } });

    // No create row, because `Work` is `work` written twice — and the vault's
    // vocabulary is where that is known, which is the point of reading it.
    expect(screen.queryByText(tagComboboxCreate("Work"))).toBeNull();
    expect(screen.getByText(tagComboboxAlreadyChosen("Work"))).toBeInTheDocument();

    fireEvent.keyDown(field, { key: "Enter" });
    expect(notesSave).not.toHaveBeenCalled();
  });

  it("writes the vault's spelling of a tag, not the one that was typed", async () => {
    renderPanel(TAGGED);
    const field = await openChooser();

    // A whole tag, spelled the way somebody's shift key spelled it. No offer
    // to create: the vault already has this tag and a second casing of it is
    // the duplicate this story's AC is about.
    fireEvent.change(field, { target: { value: "CLIENT/ANVIL" } });
    expect(await screen.findByRole("option", { name: "client/anvil" })).toBeInTheDocument();
    expect(screen.queryByText(tagComboboxCreate("CLIENT/ANVIL"))).toBeNull();
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    const block = notesSave.mock.calls[0][3] ?? "";
    expect(block).toContain("  - client/anvil\n");
    expect(block).not.toContain("CLIENT/ANVIL");
  });

  it("still creates a tag the vault has never seen, verbatim", async () => {
    renderPanel(TAGGED);
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "client/newco" } });
    expect(await screen.findByText(tagComboboxCreate("client/newco"))).toBeInTheDocument();
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect((notesSave.mock.calls[0][3] ?? "").replace("  - client/newco\n", "")).toBe(TAGGED);
  });

  it("does not read the vocabulary until the chooser is asked for", async () => {
    renderPanel(TAGGED);

    await waitFor(() => expect(recordingNoteTargets).toHaveBeenCalled());
    // The panel is on screen for as long as someone is reading the note; the
    // vocabulary is wanted for the seconds they are picking from it.
    expect(tagsVocabulary).not.toHaveBeenCalled();
  });

  it("keeps the chooser usable when the vocabulary cannot be read", async () => {
    tagsVocabulary.mockRejectedValue(new Error("no vault"));
    renderPanel(TAGGED);
    const field = await openChooser();

    // Nothing to browse costs the completion, never the edit — creating is
    // allowed here, so the tag the user can already name still goes in.
    fireEvent.change(field, { target: { value: "offline" } });
    expect(await screen.findByText(tagComboboxCreate("offline"))).toBeInTheDocument();
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect((notesSave.mock.calls[0][3] ?? "").replace("  - offline\n", "")).toBe(TAGGED);
  });
});

/**
 * Story 45.17: the tag row is the tag row on ANY note, not only on one whose
 * `tags:` key is already a list.
 *
 * 44.14 admitted three shapes — a block list, a flow list, and an empty value
 * — which between them leave out the two commonest notes there are: one whose
 * single tag was written inline, and one with no `tags:` key at all. Both got
 * the generic text box, which has no vocabulary, so a second casing of a tag
 * the vault already had silently became a second tag. That is the exact defect
 * 44.13's chooser exists to prevent, still live on most of the vault.
 */
describe("PropertiesPanel — any note's tags", () => {
  const VOCABULARY = ["work", "client/acme", "standup"];

  /** A note with a `tags:` key written inline, holding one tag. */
  const SCALAR = ["---", "title: Monday", "tags: standup", "---", ""].join("\n");

  /** A note with frontmatter and no `tags:` key, which is most notes. */
  const UNTAGGED = ["---", "title: Monday", "pinned: false", "---", ""].join("\n");

  async function openChooser(): Promise<HTMLElement> {
    fireEvent.click(screen.getByRole("button", { name: ADD_NOTE_TAG }));
    await waitFor(() => expect(tagsVocabulary).toHaveBeenCalled());
    return await screen.findByRole("combobox", { name: ADD_NOTE_TAG });
  }

  beforeEach(() => {
    tagsVocabulary.mockResolvedValue({
      entries: VOCABULARY.map((path) => ({ path, count: 1 })),
    });
  });

  it("reads an inline tag as a tag, and renders it as a chip", () => {
    renderPanel(SCALAR);

    // A chip with a remove control, not a text box. Before this story the
    // scalar fell through to the generic control and the tag was a string in
    // an input — editable, but with no vocabulary behind it.
    expect(screen.getByRole("button", { name: "Remove standup from tags" })).toBeInTheDocument();
    // The witness, in the same fixture and the same representation: `title` is
    // an ordinary scalar and DOES get the generic text box, named after its
    // key. Without it the line below is an absence with nothing to compare to —
    // change `PropertyControl`'s `aria-label={entry.key}` convention and the
    // "not a text box" claim, which is the whole point of this story's scalar
    // handling, would pass while testing nothing.
    expect(screen.getByRole("textbox", { name: "title" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "tags" })).not.toBeInTheDocument();
  });

  /**
   * The chooser is HANDED the note's tags, and this asks the chooser rather
   * than the panel. Passing `chosen={[]}` would render identically — same
   * chips, same field — and would offer to add a tag the note already has.
   */
  it("tells the chooser what an inline tag already put on the note", async () => {
    renderPanel(SCALAR);
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "standup" } });
    expect(await screen.findByText(tagComboboxAlreadyChosen("standup"))).toBeInTheDocument();
  });

  /**
   * One tag on the key's own line cannot hold two, so it becomes a flow list —
   * still on the key's own line. Promoting it to three lines would be a bigger
   * edit to the file than the edit that was asked for.
   */
  it("promotes an inline tag to a flow list rather than to three lines", async () => {
    renderPanel(SCALAR);
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "work" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][0]).toBe("sub-1");
    expect(notesSave.mock.calls[0][1]).toBe(BODY);
    expect(notesSave.mock.calls[0][2]).toBe("rev-1");
    expect(notesSave.mock.calls[0][3]).toBe(
      ["---", "title: Monday", "tags: [standup, work]", "---", ""].join("\n"),
    );
  });

  /**
   * Two in the fixture on purpose: a removal that dropped the whole list, or
   * kept only the first item, is invisible against a one-tag note.
   */
  it("removes one tag of a flow list and leaves the other where it was", async () => {
    const flow = ["---", "title: Monday", "tags: [standup, work]", "---", ""].join("\n");
    renderPanel(flow);

    fireEvent.click(screen.getByRole("button", { name: "Remove standup from tags" }));

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][3]).toBe(
      ["---", "title: Monday", "tags: [work]", "---", ""].join("\n"),
    );
  });

  /**
   * The headline case. A note with no `tags:` key gets the row anyway, and the
   * first tag writes the key — so tagging a note never requires knowing that
   * "tags" is the name of a frontmatter field.
   */
  it("offers a chooser with no tags key, and writes the key on the first tag", async () => {
    renderPanel(UNTAGGED);
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "client/acme" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][1]).toBe(BODY);
    expect(notesSave.mock.calls[0][3]).toBe(
      ["---", "title: Monday", "pinned: false", "tags:", "  - client/acme", "---", ""].join("\n"),
    );
  });

  /** And on a note with no frontmatter at all, the block is created for it. */
  it("creates the block for a note that has none", async () => {
    renderPanel("");
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "work" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][3]).toBe("---\ntags:\n  - work\n---\n");
  });

  /**
   * An indented map under `tags:` is not a tag list. The panel renders nested
   * values read-only everywhere else, and a chooser writing a flat list over
   * somebody's nested map would destroy it.
   */
  it("leaves a nested tags key alone and writes a new key beside it", async () => {
    const nested = ["---", "tags:", "  work: true", "---", ""].join("\n");
    renderPanel(nested);

    // One chooser, and no chip made out of the map's own key.
    expect(screen.getAllByRole("button", { name: ADD_NOTE_TAG })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Remove work from tags" })).not.toBeInTheDocument();

    // Neither of those two says WHICH row it is: a chooser handed the nested
    // entry renders no chips either, because a map's value is not a list. Only
    // the write tells them apart — and the difference is a user's map surviving
    // or being spliced over by a flat list. A mutation dropping the `nested`
    // guard survived both assertions above and is killed by this one.
    const field = await openChooser();
    fireEvent.change(field, { target: { value: "standup" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][3]).toContain("  work: true");
    expect(notesSave.mock.calls[0][3]).toContain("tags:\n  - standup");
  });

  /**
   * A hand-edited block can carry `tags:` twice. Exactly one row may be the tag
   * row — two would write to two spans, and the second write would land on
   * offsets the first had already moved.
   */
  it("makes the first tags key the tag row and no other", () => {
    const twice = ["---", "tags: standup", "title: Monday", "tags: work", "---", ""].join("\n");
    renderPanel(twice);

    expect(screen.getAllByRole("button", { name: ADD_NOTE_TAG })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Remove standup from tags" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove work from tags" })).not.toBeInTheDocument();
  });

  /**
   * Taking the last tag off an inline key empties the value rather than
   * deleting the key. The panel is a lens over the block and does not own its
   * keys — and `tags:` with nothing after it is what Obsidian leaves too.
   */
  it("empties an inline tags key rather than removing it", async () => {
    renderPanel(SCALAR);

    fireEvent.click(screen.getByRole("button", { name: "Remove standup from tags" }));

    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(notesSave.mock.calls[0][3]).toBe(
      ["---", "title: Monday", 'tags: ""', "---", ""].join("\n"),
    );
  });
});

describe("PropertiesPanel — record another like this (Story 45.19, FR-197)", () => {
  /** What that session's manifest holds. Two tags and TWO custom rows: a copy
   *  that kept only the first element of either would pass a one-item fixture
   *  and lose half the setup the person asked to reuse. */
  const STORED = {
    title: "Standup",
    participants: "Ada, Grace",
    note: "weekly",
    tags: "standup, q3",
    custom: [
      { name: "Ticket", value: "KPR-1" },
      { name: "Room", value: "Blue" },
    ],
  };

  beforeEach(() => {
    recordingSessionMeta.mockReset();
    recordingSessionMeta.mockResolvedValue(STORED);
    recordingMetaStore.setState({
      fields: { title: "", participants: "", note: "", tags: "", custom: [] },
      last: null,
    });
    primaryViewStore.getState().setView("notes");
  });

  it("fills every field of the next-session form and shows the Recording pane", async () => {
    recordingNoteTargets.mockResolvedValue(TARGETS);
    renderPanel(RECORDING_BLOCK);

    fireEvent.click(await screen.findByTestId(RECORD_ANOTHER_TESTID));

    // Asserted on the CALL: the folder Rust resolved, which follows a Story
    // 40.4 rename — reading the note's own (older) `recording:` text would copy
    // a session that is no longer there.
    await waitFor(() =>
      expect(recordingSessionMeta).toHaveBeenCalledWith(TARGETS[0]?.absolutePath),
    );
    await waitFor(() =>
      expect(recordingMetaStore.getState().fields).toEqual({
        title: "Standup",
        participants: "Ada, Grace",
        note: "weekly",
        tags: "standup, q3",
        custom: [
          { name: "Ticket", value: "KPR-1" },
          { name: "Room", value: "Blue" },
        ],
      }),
    );
    expect(primaryViewStore.getState().view).toBe("recording");
  });

  it("is absent on a note that is not about a recording", async () => {
    renderPanel();
    // No `session:` key at all, so the panel never even asks where the
    // recording is — the predicate is the note's own frontmatter.
    await waitFor(() => expect(screen.getByLabelText("New property name")).toBeInTheDocument());
    expect(recordingNoteTargets).not.toHaveBeenCalled();
    expect(screen.queryByTestId(RECORD_ANOTHER_TESTID)).toBeNull();
  });

  it("is absent when the session is not on this machine", async () => {
    // No archive row, no folder on disk, or no archive at all — all `null`, and
    // an action that opens a form over a session keeper cannot find is worse
    // than an absent one.
    recordingNoteTargets.mockResolvedValue(null);
    renderPanel(RECORDING_BLOCK);
    await waitFor(() => expect(recordingNoteTargets).toHaveBeenCalled());
    expect(screen.queryByTestId(RECORD_ANOTHER_TESTID)).toBeNull();
  });

  it("says so, fills nothing and navigates nowhere when the manifest will not load", async () => {
    recordingNoteTargets.mockResolvedValue(TARGETS);
    recordingSessionMeta.mockResolvedValue(null);
    renderPanel(RECORDING_BLOCK);

    fireEvent.click(await screen.findByTestId(RECORD_ANOTHER_TESTID));

    expect(await screen.findByTestId(RECORD_ANOTHER_FAULT_TESTID)).toHaveTextContent(
      RECORD_ANOTHER_UNREADABLE,
    );
    expect(recordingMetaStore.getState().fields.title).toBe("");
    // Still in Notes: moving the user to an empty recorder would make them work
    // out for themselves that nothing was copied.
    expect(primaryViewStore.getState().view).toBe("notes");
  });
});

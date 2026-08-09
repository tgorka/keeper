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

import { OVERFLOW_PANEL_LABEL, OVERFLOW_TRIGGER_LABEL } from "@/components/ui/overflow-value";
import {
  COLUMN_FITTED_VALUE_TEXT,
  COLUMN_RESIZER_LABEL,
  COLUMN_TEMPLATE_VAR,
} from "@/components/ui/resizable-columns";
import { COLUMN_WIDTH_COOKIE, MIN_COLUMN_WIDTH, readColumnWidths } from "@/lib/column-widths";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { ELLIPSIS } from "@/lib/truncate";
import { withRect, withTextLayout } from "@/test/layout";
import {
  PROPERTIES_COLUMN_LABEL,
  PROPERTY_KEY_COLUMN,
  PropertiesPanel,
  readFrontmatter,
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

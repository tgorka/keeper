/**
 * A file has a Save button, and the bar it sits in does not move (Story 46.13,
 * FR-216, AD-104).
 *
 * **Why this file mounts the frame directly, where `text-file-viewer.test.tsx`
 * deliberately goes through the registry.** That suite's subject is the binding —
 * "declared and never mounted" is DW-172, and only a test that resolves the
 * component out of the table can see it. This suite's subject is what the frame
 * does with a `dirty` flag and a `save` function, and those arrive as one object
 * from 45.6's hook. Handing the frame a hand-built {@link UseTextFileResult} is
 * what lets a test say "the buffer differs from the disk" without typing into
 * CodeMirror and hoping — and the four states that matter (clean, dirty,
 * read-only format, oversize) are states a real vault produces on demand and
 * cannot produce on request.
 *
 * The layout claims here are the same three 46.4 could make and no more: jsdom
 * lays nothing out, so what is asserted is the structure that CAUSES a shift —
 * the caption is not a width-variable participant in the row holding the button —
 * and never a measured pixel. See `pane-header.test.tsx`.
 *
 * **The last describe is the exception, and it is here for the same reason.**
 * Story 52.3's seam is the properties FORM's block reaching the live pane, and
 * the two states that seam gets wrong are states only a hand-held read can
 * produce: a read still in flight, and a read that refused. So that block mounts
 * the real panel over a real editor over a buffer the test owns, and drives the
 * frontmatter commands itself.
 */
import "@codemirror/lang-markdown";
import "@codemirror/language";
import "@codemirror/state";
import "@/components/notes/editor/indent-keymap";
import "@/components/notes/editor/live-preview";
// Note mode's mount awaits three more chunks, and `settle()` drains microtasks
// rather than frames — so they are warmed here for `raw-rendered-view.test.tsx`'s
// reason: a cold `import()` would not have resolved by the time it returns.
import "@/components/notes/editor/writing-tools";
import { EditorView } from "@codemirror/view";
import { act, fireEvent, type RenderResult, render, screen, within } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { TextFileVm } from "@/lib/ipc/client";
import type { ViewerEntry } from "@/lib/viewers";
import { withRangeRects } from "@/test/layout";

/** Story 52.3's seam needs both halves of the properties address in the test's
 *  hands: the block the form is holding is what the panes hide, and only a test
 *  can hold a read open or refuse it. */
const syncReadFrontmatter = vi.fn<(profileId: string, subpath: string) => Promise<string>>();
const syncWriteFrontmatter =
  vi.fn<(profileId: string, subpath: string, expected: string, block: string) => Promise<string>>();

vi.mock("@/lib/ipc/client", () => ({
  notesCsvRead: vi.fn(),
  notesCsvSetCell: vi.fn(),
  notesTree: vi.fn(),
  notesResolveLink: vi.fn(),
  notesVaultSetActive: vi.fn(),
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  revealPath: vi.fn(async () => undefined),
  syncOpenEntry: vi.fn(async () => undefined),
  syncReadText: vi.fn(),
  syncWriteEntry: vi.fn(),
  // Story 50.4's panel reaches the client through the frame's import graph now.
  // A factory that omits an export makes the IMPORT throw, not the call, so
  // every wrapper `properties-panel` names has to be here even for the tests
  // that never mount it.
  notesSave: vi.fn(),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => undefined),
  recordingSessionMeta: vi.fn(),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
  syncReadFrontmatter: (profileId: string, subpath: string) =>
    syncReadFrontmatter(profileId, subpath),
  syncWriteFrontmatter: (profileId: string, subpath: string, expected: string, block: string) =>
    syncWriteFrontmatter(profileId, subpath, expected, block),
  sessionsFileRename: vi.fn(async () => ""),
}));

import { FOLD_STRIP } from "@/components/layout/fold-strip";
import {
  PANE_HEADER_ACTIONS_SLOT,
  PANE_HEADER_FRAME_SLOT,
  PANE_HEADER_IDENTITY_SLOT,
  PANE_HEADER_STATUS_SLOT,
} from "@/components/layout/pane-header";
import { PROPERTIES_LABEL } from "@/components/notes/properties-panel";
import {
  FILE_FRAME_FOLD_COOKIE,
  fileFrameFoldCookie,
  readFileFrameFold,
  resetFileFrameFoldForTest,
} from "@/lib/stores/file-frame-fold";
import {
  FILE_SAVE_CLEAN_TITLE,
  FILE_SAVE_LABEL,
  FILE_SAVE_SIZERS,
  type FilePropertiesCoordinates,
  fileSaveWord,
  TEXT_FILE_CAVEAT_LABEL,
  TEXT_FILE_CAVEAT_TESTID,
  TextFileFrame,
  type TextFileFrameProps,
} from "./text-file-frame";
import type { UseTextFileResult } from "./use-text-file";

/** A `.md` as the registry describes one: text, writable, with a rendered half. */
const MARKDOWN: ViewerEntry = {
  viewer: "text",
  format: "markdown",
  label: "Markdown",
  icon: "file-text",
  rendered: "markdown",
  language: "markdown",
  writable: true,
};

/**
 * A text-shaped format keeper must not rewrite.
 *
 * Built by hand because no `viewer: "text"` row is non-writable today — which is
 * exactly why the frame guards it, and why the guard needs a fixture the registry
 * cannot currently produce.
 */
const LOCKED: ViewerEntry = { ...MARKDOWN, writable: false, label: "Locked" };

function vm(over: Partial<TextFileVm> = {}): TextFileVm {
  return {
    text: "# Meeting\n",
    sizeLabel: "10 bytes",
    oversize: false,
    binary: false,
    detail: null,
    ...over,
  } as TextFileVm;
}

/** 45.6's hook result, as the frame consumes it. */
function state(over: Partial<UseTextFileResult> = {}): UseTextFileResult {
  return {
    vm: vm(),
    content: "# Meeting\n",
    setContent: vi.fn(),
    dirty: false,
    save: vi.fn(async () => {}),
    reload: vi.fn(async () => {}),
    error: null,
    loading: false,
    // Which file the loader read. The frame does not decide it and does not read
    // it — it hands it to the views, which key their editors on it.
    loadedFrom: { profileOrVaultId: "p1", relativePath: "60-sessions/active/s/README.md" },
    ...over,
  };
}

/** The sync-profile address a Files panel would hand over (Story 50.4). */
const ADDRESS = { profileId: "p1", relativePath: "60-sessions/active/s/README.md" };

/**
 * AD-102's caveat as Rust composes it, both forms (Story 46.14, Story 53.3).
 *
 * Verbatim from `WriteScope::unmanaged_caveat` and `unmanaged_caveat_short`, so
 * what these tests assert is the sentence a reader actually gets — and so a
 * webview that clipped the long one instead of rendering the short one is
 * visible: the two are not prefixes of one another.
 */
const CAVEAT_FULL =
  "AGENTS.md is not one of keeper's notes — it is outside Vault's notes vault (10-notes). " +
  "keeper saves it straight to the file and sends a delete to this computer's trash: no note " +
  "history, no search index and no conflict copy. Nothing about how Vault syncs this folder " +
  "changes.";
const CAVEAT_SHORT =
  "AGENTS.md is not one of keeper's notes: no note history, no search index and no conflict copy.";

/** The panel's own controls, as a host that gave up its row hands them down
 *  (Story 53.3). One button, named the way `panel-strip.tsx` names its fold, so
 *  a test can find it and press it after it has travelled. */
const FRAME_CONTROL_LABEL = "Fold panel";
const FRAME_CONTROLS = (
  <button type="button" aria-label={FRAME_CONTROL_LABEL}>
    x
  </button>
);

function mount(
  over: Partial<UseTextFileResult> = {},
  entry: ViewerEntry = MARKDOWN,
  properties: FilePropertiesCoordinates | null = null,
  extra: Partial<TextFileFrameProps> = {},
): RenderResult {
  return render(
    <TextFileFrame
      fileName="readme.md"
      entry={entry}
      state={state(over)}
      csv={null}
      properties={properties}
      preview={{ vaultId: null }}
      {...extra}
    />,
  );
}

function bar(): HTMLElement | null {
  return document.querySelector("header");
}

function group(slot: string): HTMLElement {
  const found = bar()?.querySelector<HTMLElement>(`:scope > [data-slot="${slot}"]`) ?? null;
  if (found === null) {
    throw new Error(`the save bar drew no ${slot} group`);
  }
  return found;
}

/** What the slot actually SAYS, as against what it reserves.
 *
 * The sizers are inside the slot and are `aria-hidden` and `invisible`, so the
 * slot's own `textContent` reads the same in every state — which would make an
 * assertion on it pass for a caption that never rendered at all. */
function shownWord(): string {
  const shown = group(PANE_HEADER_STATUS_SLOT).querySelector(":scope > :not([aria-hidden='true'])");
  if (shown === null) {
    throw new Error("the status slot rendered no caption element");
  }
  return shown.textContent ?? "";
}

/** The status slot's box, order-insensitively — Tailwind class order is the
 *  formatter's business, the SET is the claim. */
function box(element: Element): string {
  return Array.from(element.classList).sort().join(" ");
}

/**
 * jsdom has no `Range.getClientRects`, and the live pane's measure pass calls it
 * on any animation frame that elapses. Without this the run throws at a time that
 * depends on how slow the machine was — a suite that is green until it is not.
 */
let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

beforeEach(() => {
  syncReadFrontmatter.mockReset();
  // A file with no frontmatter, which is what every fixture outside the last
  // describe is — and what makes the panel appear at all.
  syncReadFrontmatter.mockResolvedValue("");
  syncWriteFrontmatter.mockReset();
  syncWriteFrontmatter.mockResolvedValue("");
  // Story 53.3's folds live in a store and a cookie, both of which outlive a
  // test: one test's fold would otherwise be the next one's restore, and the
  // hydrate runs once per document.
  resetFileFrameFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this suite's subject
  document.cookie = `${FILE_FRAME_FOLD_COOKIE}=; path=/; max-age=0`;
});

/**
 * Drain the microtasks the live pane's mount rides on, and nothing else.
 *
 * Deliberately not `waitFor`, which advances timers: a frame starts CodeMirror's
 * measure pass, and `raw-rendered-view.test.tsx` records why that is a trade
 * about jsdom rather than about the feature.
 */
async function settle(): Promise<void> {
  await act(async () => {
    for (let tick = 0; tick < 8; tick += 1) {
      await Promise.resolve();
    }
  });
}

/** The live view inside whichever pane is mounted, asserted rather than assumed. */
function paneView(): EditorView {
  const content = document.querySelector<HTMLElement>(".cm-content");
  expect(content, "no pane mounted a CodeMirror").not.toBeNull();
  const view = EditorView.findFromDOM(content as HTMLElement);
  expect(view, "no EditorView is mounted in that content DOM").not.toBeNull();
  return view as EditorView;
}

/** Type at the caret, one character per transaction — how an edit really
 *  arrives, and the only way a view rebuilt between keystrokes is visible. */
async function typeAtCaret(view: EditorView, text: string): Promise<void> {
  for (const character of text) {
    await act(async () => {
      const at = view.state.selection.main.head;
      view.dispatch({
        changes: { from: at, insert: character },
        selection: { anchor: at + character.length },
        userEvent: "input.type",
      });
    });
    await settle();
  }
}

/**
 * The frame over a buffer the TEST owns, which is what 45.6's hook really is.
 *
 * `mount` above hands over a frozen `UseTextFileResult`, and that is right for
 * every question about a `dirty` flag. It cannot answer this one: whether a
 * properties write that lands over text somebody typed keeps the text. That needs
 * `setContent` to move the buffer, `dirty` to follow it, and `reload` to behave
 * the way `read` does — set `loading`, then replace the buffer with the disk.
 */
function LiveFrame({
  initial,
  disk,
  sets,
  reads,
  address = ADDRESS,
}: {
  initial: string;
  /** What a re-read would answer with, as the test's own mutable disk. */
  disk: { text: string };
  /** Every value the frame put in the buffer, in order. The last one is what a
   *  Save would write. */
  sets: string[];
  /** One entry per re-read the frame asked for. */
  reads: string[];
  /** Which file the panel is addressed at. A panel replaces its target in place,
   *  so a test can hand the same mount a second file. */
  address?: FilePropertiesCoordinates;
}): React.ReactElement {
  const [content, setContentState] = useState(initial);
  const [loading, setLoading] = useState(false);
  const persisted = useRef(initial);
  return (
    <TextFileFrame
      fileName="readme.md"
      entry={MARKDOWN}
      state={{
        ...state(),
        content,
        loading,
        dirty: content !== persisted.current,
        setContent: (next) => {
          sets.push(next);
          setContentState(next);
        },
        reload: async () => {
          reads.push(disk.text);
          setLoading(true);
          await Promise.resolve();
          persisted.current = disk.text;
          setContentState(disk.text);
          setLoading(false);
        },
      }}
      csv={null}
      properties={address}
      // The real Files host always passes one (Story 52.2), and the panel calls
      // `onWritten` instead when it does not — a different path, asserted where
      // that story's tests are.
      onPropertiesRenamed={() => {}}
      preview={{ vaultId: null }}
    />
  );
}

describe("the Save control", () => {
  it("is reachable, and says why it cannot act when it cannot", () => {
    mount();

    const save = screen.getByRole("button", { name: FILE_SAVE_LABEL });
    // Disabled rather than absent, which is the opposite of what this pane does
    // for a control that cannot act — because "nothing has changed" is a state
    // the reader leaves by typing, and a Save that vanished whenever the buffer
    // matched the disk would be a control nobody could find on purpose. The
    // sentence is what makes the disabled state honest.
    expect(save).toBeDisabled();
    expect(save).toHaveAttribute("title", FILE_SAVE_CLEAN_TITLE);
    // And the caption is quiet: there is nothing to act on and nothing to say.
    // The VISIBLE element, not the slot — the slot always contains the sizer, so
    // reading its `textContent` would pass for both states and prove nothing.
    expect(shownWord()).toBe("");
  });

  it("wakes up when the buffer differs from the disk, and says so", () => {
    mount({ dirty: true });

    const save = screen.getByRole("button", { name: FILE_SAVE_LABEL });
    expect(save).toBeEnabled();
    // No tooltip: there is nothing to explain about a control that works.
    expect(save).not.toHaveAttribute("title");
    // The informative state is the OPPOSITE of the note editor's, and
    // deliberately: a note autosaves, so the fact worth carrying is that the
    // write landed. A file does not, so the fact worth carrying is the one the
    // reader can still act on.
    expect(shownWord()).toBe(fileSaveWord(true));
    // On `title` too, so a caption long enough to ellipsise is still readable.
    expect(screen.getByTitle(fileSaveWord(true))).toHaveTextContent(fileSaveWord(true));
  });

  it("saves the buffer the hook is holding, and nothing else", async () => {
    const save = vi.fn(async () => {});
    mount({ dirty: true, save });

    screen.getByRole("button", { name: FILE_SAVE_LABEL }).click();

    expect(save).toHaveBeenCalledTimes(1);
    // `Mod-s` and this button are the same call. The hook owns the text, so
    // there is nothing for the button to pass and nothing it could pass that
    // would differ from what the editor last reported.
    expect(save).toHaveBeenCalledWith();
  });

  it("is not offered over a format keeper will not write", () => {
    mount({ dirty: true }, LOCKED);

    // A Save over a file the frame is already refusing to make editable would be
    // a control that announces its own refusal.
    expect(screen.queryByRole("button", { name: FILE_SAVE_LABEL })).toBeNull();
    // And with no host controls to carry, there is no row at all — the note
    // embed's shape. A panel that gave up its own row gets one anyway, which is
    // Story 53.3's promise and is asserted in its own describe below.
    expect(bar()).toBeNull();
  });

  it("is not offered over a file only the first part of which was read", () => {
    mount({ dirty: true, vm: vm({ oversize: true, sizeLabel: "40 MB" }) });

    // The loader declines this save with a sentence about truncation. A button
    // that could only ever produce that sentence is worse than no button: it
    // says keeper will write the file, and keeper will not.
    expect(screen.queryByRole("button", { name: FILE_SAVE_LABEL })).toBeNull();
    expect(bar()).toBeNull();
  });

  it("draws no bar before the file has finished opening, or when it never does", () => {
    mount({ loading: true });
    expect(bar()).toBeNull();

    render(
      <TextFileFrame
        fileName="gone.md"
        entry={MARKDOWN}
        state={state({ vm: null, error: "keeper could not read gone.md." })}
        csv={null}
        properties={null}
        preview={{ vaultId: null }}
      />,
    );
    expect(bar()).toBeNull();
  });
});

describe("the bar the Save control sits in", () => {
  it("is 46.4's three groups, with the caption out of the buttons' shrink context", () => {
    mount({ dirty: true });

    const row = bar();
    if (row === null) {
      throw new Error("the frame drew no save bar");
    }
    // The second consumer of AD-104's header, and the reason it was extracted:
    // the same variable-width status element beside the same kind of controls.
    expect(Array.from(row.children).map((child) => child.getAttribute("data-slot"))).toEqual([
      PANE_HEADER_IDENTITY_SLOT,
      PANE_HEADER_STATUS_SLOT,
      PANE_HEADER_ACTIONS_SLOT,
    ]);
    expect(Array.from(row.children).filter((child) => child.tagName === "BUTTON")).toHaveLength(0);
    expect(Array.from(row.children).filter((child) => child.classList.contains("flex-1"))).toEqual([
      group(PANE_HEADER_IDENTITY_SLOT),
    ]);
    expect(group(PANE_HEADER_STATUS_SLOT)).toHaveClass("shrink-0");
    // The file's own name, which nothing else inside this frame renders — and the
    // one identity the note-embed host, which has no header of its own, can rely
    // on.
    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveTextContent("readme.md");
  });

  it("keeps the same box, and the same reservation, across a save", () => {
    const { rerender } = mount({ dirty: true });
    const dirtyBox = box(group(PANE_HEADER_STATUS_SLOT));
    const dirtyReservation = Array.from(
      group(PANE_HEADER_STATUS_SLOT).querySelectorAll(":scope > [aria-hidden]"),
    ).map((sizer) => sizer.textContent);
    expect(shownWord()).toBe(fileSaveWord(true));

    rerender(
      <TextFileFrame
        fileName="readme.md"
        entry={MARKDOWN}
        state={state({ dirty: false })}
        csv={null}
        properties={null}
        preview={{ vaultId: null }}
      />,
    );

    // The caption emptied and the box did not change. This is the property that
    // stops the Save button moving under the reader's pointer as they type — the
    // Files pane's version of the jump 46.4 removed from the note editor.
    expect(shownWord()).toBe("");
    expect(box(group(PANE_HEADER_STATUS_SLOT))).toBe(dirtyBox);
    expect(
      Array.from(group(PANE_HEADER_STATUS_SLOT).querySelectorAll(":scope > [aria-hidden]")).map(
        (sizer) => sizer.textContent,
      ),
    ).toEqual(dirtyReservation);
  });

  it("reserves the box from the string the caption can actually show", () => {
    mount();

    // Produced by `fileSaveWord`, not written out beside it, so a change to the
    // wording cannot change what is shown without changing what is reserved.
    expect(FILE_SAVE_SIZERS).toEqual([fileSaveWord(true)]);
    expect(
      Array.from(group(PANE_HEADER_STATUS_SLOT).querySelectorAll(":scope > [aria-hidden]")).map(
        (sizer) => sizer.textContent,
      ),
    ).toEqual([...FILE_SAVE_SIZERS]);
    // The clean state reserves nothing of its own, because it says nothing.
    expect(fileSaveWord(false)).toBe("");
  });

  it("leaves the error banner where it was, under the bar rather than in it", () => {
    mount({ dirty: true, error: "keeper could not save readme.md." });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("keeper could not save readme.md.");
    // A refusal is a whole sentence composed in Rust and therefore unbounded. It
    // stays a banner rather than becoming the caption: the status slot is a fixed
    // box, and everything to its right is standing on that.
    expect(bar()?.contains(alert)).toBe(false);
  });
});

/**
 * Story 50.4's half of the frame: WHETHER a file's own properties are offered.
 *
 * What they say, and every rule about what may be edited, is
 * `properties-panel`'s and is asserted there. What this frame owes is that the
 * panel appears exactly where a save can land over prose, and nowhere else — the
 * same equality 50.3's writing tools stand on.
 *
 * Story 53.3 put the panel behind a fold, and Story 54.2 opens that fold by
 * default: "offered" means the CONTROL is on the bar and the form is on screen
 * under it. The predicate is unchanged and is what these still assert: where no
 * save can land there is neither.
 */
describe("a file's own properties", () => {
  it("are offered for a writable markdown file the surface can address", async () => {
    mount({}, MARKDOWN, ADDRESS);

    // OPEN on arrival, with no cookie and no press (Story 54.2, row 1). This
    // asserted `aria-expanded="false"` and a null region while the file surface
    // copied the notes surface's closed default — and on a file that default put
    // the `---` block into the reader's prose, because the buffer here IS the
    // whole file.
    const control = screen.getByRole("button", { name: PROPERTIES_LABEL });
    expect(control).toHaveAttribute("aria-expanded", "true");
    const region = await screen.findByRole("region", { name: PROPERTIES_LABEL });
    expect(region).toBeInTheDocument();
    // The promise `aria-expanded` makes, kept: the id it names is the box the
    // form is in, so a screen reader's "go to the controlled region" lands on it.
    const controls = control.getAttribute("aria-controls");
    expect(controls).not.toBeNull();
    expect(document.getElementById(controls as string)?.contains(region)).toBe(true);

    fireEvent.click(control);

    // And the fold still folds. The box stays in the document — hidden, so it is
    // out of the accessibility tree and out of the column's height — which is
    // what keeps the form holding the block while it is off screen.
    expect(control).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(document.getElementById(controls as string)).not.toBeNull();
  });

  it("names itself on the bar, in a row whose height it does not change", () => {
    mount({}, MARKDOWN, ADDRESS);

    // The defect this control shipped with: a 32px ghost carrying
    // `SlidersHorizontal` and an `aria-label`, next to Save, identical whether
    // the file had three properties or none. Nothing an eye reads said
    // properties existed at all.
    const control = screen.getByRole("button", { name: PROPERTIES_LABEL });
    expect(control.textContent).toContain(PROPERTIES_LABEL);
    // The visible word IS the accessible name, rather than a second copy of it
    // that could drift (WCAG 2.5.3).
    expect(control.getAttribute("aria-label")).toBeNull();
    // A disclosure's chevron, the pair the caveat fold beneath uses.
    expect(control.querySelectorAll("svg").length).toBe(2);

    // Zero vertical pixels: the row's height is `PaneHeader`'s `h-10` and the
    // control is `h-6`, the same height as Save beside it. jsdom lays nothing
    // out, so this is the class the height comes from and not a measurement —
    // the pixels are a thing to look at on the real machine.
    expect(bar()?.className).toContain("h-10");
    expect(control.className).toContain("h-6");
    expect(within(group(PANE_HEADER_ACTIONS_SLOT)).getByText(PROPERTIES_LABEL)).toBeVisible();
  });

  it("row 9: are not offered over a CSV, which has no frontmatter to show", () => {
    // The registry's own CSV row, verbatim: a text viewer, writable, and not
    // markdown. A `.csv` has no frontmatter, so there is nothing to show.
    const csvRow: ViewerEntry = {
      viewer: "text",
      format: "csv",
      label: "CSV",
      icon: "file-table",
      rendered: "table",
      language: "csv",
      writable: true,
    };
    mount({}, csvRow, ADDRESS);

    expect(screen.queryByRole("button", { name: PROPERTIES_LABEL })).toBeNull();
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });

  it("are not offered where no save could follow them", () => {
    // The format keeper will not rewrite, and the file only the first megabyte
    // of which was read. Both already take the Save button away; a panel whose
    // every control announced its own refusal would be strictly worse than not
    // being there — and so would a fold over one.
    mount({}, LOCKED, ADDRESS);
    expect(screen.queryByRole("button", { name: PROPERTIES_LABEL })).toBeNull();
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();

    mount(
      { vm: vm({ oversize: true, detail: "readme.md is larger than 1.0 MB." }) },
      MARKDOWN,
      ADDRESS,
    );
    expect(screen.queryByRole("button", { name: PROPERTIES_LABEL })).toBeNull();
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });

  it("are not offered by a host that holds no sync-profile address", () => {
    // The note embed. It has a vault id and a vault-relative target, which is a
    // different identifier over overlapping bytes, and deriving one from the
    // other in the webview is what AD-65 forbids.
    mount({}, MARKDOWN, null);

    expect(screen.queryByRole("button", { name: PROPERTIES_LABEL })).toBeNull();
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });
});

/**
 * Story 51.5's half of the frame: WHETHER a markdown file is offered Note mode
 * (FR-294).
 *
 * Deliberately the same predicate as the writing tools and the properties panel,
 * and asserted here rather than in the view because this is the layer that holds
 * all three of its inputs — the registry's format, the size guard, and Rust's
 * own refusal to write the location.
 */
describe("Note mode", () => {
  it("is offered for a writable markdown file, beside Preview and Source", () => {
    mount({}, MARKDOWN, ADDRESS);

    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Preview",
      "Source",
      "Note",
    ]);
  });

  it("row 7: is absent for a `workspace/` file, whose refusal is unchanged", () => {
    // Rust's own sentence, carried on the listing row (AD-113). The tab is
    // ABSENT rather than present-and-refusing: an editor over a buffer every
    // write refuses is a control that announces its own refusal.
    const refusal = "keeper does not write inside workspace/, which is scratch space";
    render(
      <TextFileFrame
        fileName="notes.md"
        entry={MARKDOWN}
        state={state()}
        writeRefusal={refusal}
        csv={null}
        properties={ADDRESS}
        preview={{ vaultId: null }}
      />,
    );

    expect(screen.queryByRole("tab", { name: "Note" })).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(refusal);
  });

  it("row 8: a file with no rendered view has neither a Preview nor a Note tab", () => {
    // The registry's own `.rs` row, verbatim: a text viewer, writable, one
    // language and no rendered half. Unchanged by this story — there is no
    // live-preview view of a `.rs` to make editable.
    const rust: ViewerEntry = {
      viewer: "text",
      format: "source",
      label: "Rust source",
      icon: "file-code",
      rendered: null,
      language: "rust",
      writable: true,
    };
    mount({}, rust, ADDRESS);

    expect(screen.queryByRole("tablist")).toBeNull();
  });

  it("row 9: is absent for an oversize file, of which only a prefix was read", () => {
    mount(
      { vm: vm({ oversize: true, detail: "readme.md is larger than 1.0 MB." }) },
      MARKDOWN,
      ADDRESS,
    );

    // The same rule the Save button follows: the loader declines a save that
    // would truncate the rest of the file, so there is nothing for a third
    // editing tab to save.
    expect(screen.queryByRole("tab", { name: "Note" })).toBeNull();
    expect(screen.getByRole("tab", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Source" })).toBeInTheDocument();
  });

  it("is absent for a format keeper will not rewrite", () => {
    mount({}, LOCKED, ADDRESS);

    expect(screen.queryByRole("tab", { name: "Note" })).toBeNull();
  });
});

/**
 * The seam Story 52.3 created, and the line that decides WHEN to hide (FR-304).
 *
 * `frontmatterInForm` used to be "this frame mounted a `FileProperties`", and the
 * pane then re-parsed the live BUFFER to guess which bytes that form was drawing.
 * Two recognisers over two sources — Rust's `block_of` over the file on disk, and
 * `readFrontmatter` over the buffer — and nothing in either suite mounted the
 * frame with a read it could hold open, so the disagreement was invisible.
 *
 * These mount the real panel over the real pane and drive the frontmatter
 * commands, because the two states that got it wrong are a read still in flight
 * and a read that refused, and neither is a state a fixture can be.
 *
 * **Nothing arranges the fold any more (Story 54.2).** All four of these ran
 * under a `beforeEach` that hydrated `{ properties: false }` — which is the fold
 * OPEN, arranged, because 53.3 had defaulted it closed. That is a state no fresh
 * install was ever in: the guards written for the 52.3 request were disarmed by
 * the same commit that broke it, and with the arrangement deleted on that build
 * every one of them would have failed. The form is open by default again, so
 * what they test is the default; the FOLDED case is asserted at the end of this
 * block rather than arranged away.
 */
describe("the panes hide the block the form is holding, and only that (Story 52.3)", () => {
  /** A block as `file_properties` writes one, and the body under it. `status`
   *  rather than `title`: a title write is a RENAME, which is a different path. */
  const BLOCK = "---\nstatus: draft\n---\n";
  const LANDED = "---\nstatus: done\n---\n";
  const BODY = "# Weekly\n\nalpha\n";

  it("draws the block as document text until the form is holding it", async () => {
    // The read `sync_ipc.rs` documents as a stat plus a read — hundreds of ms on a
    // pendrive — held open here. Hiding on "a form was mounted" hid the block from
    // the FIRST frame, so for the whole of that window it was in neither the form
    // nor the text.
    let land: (block: string) => void = () => {};
    syncReadFrontmatter.mockReturnValue(
      new Promise<string>((resolve) => {
        land = resolve;
      }),
    );
    render(<LiveFrame initial={BLOCK + BODY} disk={{ text: BLOCK + BODY }} sets={[]} reads={[]} />);
    await settle();

    expect(screen.getByRole("tab", { name: "Note" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(paneView().state.doc.toString()).toBe(BLOCK + BODY);

    await act(async () => {
      land(BLOCK);
    });
    await settle();

    // Now, and not before: the form holds those bytes, so the pane stops drawing
    // them.
    expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
    expect(paneView().state.doc.toString()).toBe(BODY);
  });

  it("keeps drawing the block when the form's read refused", async () => {
    // `FileProperties` renders nothing when the read rejects — a `workspace/` file,
    // a permission Rust will not cross — and reports `null`. Hiding on "a form was
    // mounted" hid the block from a document that had no form above it AT ALL, for
    // as long as the panel stayed open.
    syncReadFrontmatter.mockRejectedValue(new Error("keeper will not read that here"));
    render(<LiveFrame initial={BLOCK + BODY} disk={{ text: BLOCK + BODY }} sets={[]} reads={[]} />);
    await settle();

    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(paneView().state.doc.toString()).toBe(BLOCK + BODY);
  });

  it("keeps a typed paragraph when a property write lands in the form above it", async () => {
    // The sequence 52.3 made ordinary: a savable markdown file now opens with a
    // caret in Note mode, so "type a paragraph, then set a property above it" is
    // how this pane is used. The unconditional re-read after a properties write
    // destroyed the paragraph — `reload` is `read`, which replaces the buffer with
    // no dirty check, silently and with no prompt.
    syncReadFrontmatter.mockResolvedValue(BLOCK);
    syncWriteFrontmatter.mockResolvedValue(LANDED);
    const sets: string[] = [];
    const reads: string[] = [];
    const disk = { text: BLOCK + BODY };
    render(<LiveFrame initial={BLOCK + BODY} disk={disk} sets={sets} reads={reads} />);
    await settle();
    expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();

    const view = paneView();
    expect(view.state.doc.toString()).toBe(BODY);
    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "beta\n");
    expect(view.state.doc.toString()).toBe(`${BODY}beta\n`);

    // Rust wrote the block and preserved every other byte, so this is what the
    // file now says — and what a re-read would have put in the buffer.
    disk.text = LANDED + BODY;
    const status = screen.getByLabelText("status");
    fireEvent.change(status, { target: { value: "done" } });
    fireEvent.blur(status);
    await settle();

    expect(syncWriteFrontmatter).toHaveBeenCalledWith("p1", ADDRESS.relativePath, BLOCK, LANDED);
    // Not one re-read: the file was not read over somebody's unsaved text.
    expect(reads).toEqual([]);
    // The paragraph is still where he typed it, and the buffer a Save would write
    // is the block that LANDED in front of it — not the block that was there when
    // he started typing, which is what a later Save would otherwise have put back.
    expect(paneView().state.doc.toString()).toBe(`${BODY}beta\n`);
    expect(sets[sets.length - 1]).toBe(`${LANDED}${BODY}beta\n`);
  });

  it("re-reads the file when a property write lands over a buffer nobody typed in", async () => {
    // The other half of the same rule, and why the answer is not simply "never
    // re-read": with nothing to lose, the file itself is the truthful repair —
    // it advances what the loader believes is on disk, which a splice cannot, and
    // that is what keeps the Save button honest.
    //
    // Both commands answer from the same `disk` here, deliberately: a re-read
    // tears the panel down and back up (`loading` empties the frame), so it asks
    // for the block again, and a fixture that kept answering with the OLD block
    // would have the form and the file disagreeing for ever — which is a livelock
    // rather than a test.
    const disk = { text: BLOCK + BODY, block: BLOCK };
    syncReadFrontmatter.mockImplementation(async () => disk.block);
    syncWriteFrontmatter.mockResolvedValue(LANDED);
    const sets: string[] = [];
    const reads: string[] = [];
    render(<LiveFrame initial={BLOCK + BODY} disk={disk} sets={sets} reads={reads} />);
    await settle();
    expect(paneView().state.doc.toString()).toBe(BODY);

    disk.text = LANDED + BODY;
    disk.block = LANDED;
    const status = screen.getByLabelText("status");
    fireEvent.change(status, { target: { value: "done" } });
    fireEvent.blur(status);
    await settle();

    // Once. A splice AND a re-read would be the file read twice for one property.
    expect(reads).toEqual([LANDED + BODY]);
    // Nothing was spliced over the buffer: the read is what put the new block in
    // it, which is why `dirty` did not go true for a property nobody typed.
    expect(sets).toEqual([]);
    expect(paneView().state.doc.toString()).toBe(BODY);
  });

  it("does not re-read when the panel is pointed at a second file", async () => {
    // A panel replaces its target IN PLACE, so this frame outlives the file it is
    // showing and the host's own loader is already re-reading. The second file's
    // first block must read as a FIRST block: treating it as this file's block
    // changing would spend a read on a file that had just been read.
    const disk = { text: BLOCK + BODY, block: BLOCK };
    syncReadFrontmatter.mockImplementation(async () => disk.block);
    const sets: string[] = [];
    const reads: string[] = [];
    const view = render(<LiveFrame initial={BLOCK + BODY} disk={disk} sets={sets} reads={reads} />);
    await settle();
    expect(paneView().state.doc.toString()).toBe(BODY);

    // The other file, with a block of its own.
    disk.block = LANDED;
    view.rerender(
      <LiveFrame
        initial={BLOCK + BODY}
        disk={disk}
        sets={sets}
        reads={reads}
        address={{ profileId: "p1", relativePath: "60-sessions/active/s/OTHER.md" }}
      />,
    );
    await settle();

    expect(syncReadFrontmatter).toHaveBeenLastCalledWith("p1", "60-sessions/active/s/OTHER.md");
    expect(reads).toEqual([]);
    expect(sets).toEqual([]);
  });

  it("keeps hiding the block from a form the reader folded away by hand", async () => {
    // The load-bearing half of Story 54.2, and the half a new default does not
    // buy. On the shipped build, folding the form put
    // `---\nstatus: draft\n---` back at the top of the reader's document —
    // `frontmatterInForm` was `null` while the fold was closed, because the form
    // unmounted with it. Folding is a display choice about the FORM; it is never
    // an instruction to paste YAML into somebody's prose, and the fold is the
    // gesture the control exists for.
    syncReadFrontmatter.mockResolvedValue(BLOCK);
    render(<LiveFrame initial={BLOCK + BODY} disk={{ text: BLOCK + BODY }} sets={[]} reads={[]} />);
    await settle();
    expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
    expect(paneView().state.doc.toString()).toBe(BODY);

    fireEvent.click(screen.getByRole("button", { name: PROPERTIES_LABEL }));
    await settle();

    // Off the screen and out of the accessibility tree, still mounted, still
    // holding the block — which is the whole of why the prose is unchanged.
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(paneView().state.doc.toString()).toBe(BODY);
    // And it cost no second read: the form never went away, so folding and
    // unfolding is free.
    expect(syncReadFrontmatter).toHaveBeenCalledTimes(1);
  });
});

/**
 * Story 53.3: the two bands above the file fold, and the fold is remembered
 * (FR-316, FR-318). Story 54.2: the properties fold opens OPEN, and folding it
 * never hands the block back to the prose (FR-325, FR-326).
 *
 * **The restore is asserted HERE because this component is the mount point.**
 * `hydrateFileFrameFold` is called by `TextFileFrame` and nowhere else, and a
 * store-level test passes unchanged on a build where that call was deleted
 * (DW-172) — the defect epic 44 shipped with three tray listeners. So the fold
 * tests below arrange a real cookie, mount the real frame, and read the state
 * off the control.
 */
describe("the properties fold", () => {
  /**
   * Re-anchored by Story 54.2, and deliberately not deleted.
   *
   * This was called *"folds the form away and back, and the pane takes the block
   * back with it"*, and while the form was folded it asserted `BLOCK + BODY` in
   * the pane — the owner's own defect, written down as an assertion, from 53.3's
   * reasoning that with no form on screen the document had to draw the block.
   * The FOLD is still right and is still what this covers. The block coming back
   * is not: the form is mounted either way now, so it holds the block either
   * way, and 53.3's objection is answered by the control, which is named
   * Properties and says it is closed.
   */
  it("folds the form away and back, and never hands the block to the prose", async () => {
    const BLOCK = "---\nstatus: draft\n---\n";
    const BODY = "# Weekly\n\nalpha\n";
    syncReadFrontmatter.mockResolvedValue(BLOCK);
    render(<LiveFrame initial={BLOCK + BODY} disk={{ text: BLOCK + BODY }} sets={[]} reads={[]} />);
    await settle();

    // Open on arrival, with no cookie and no press: the grid is there and the
    // pane holds his document rather than his document with its own metadata
    // pasted at the top.
    const control = screen.getByRole("button", { name: PROPERTIES_LABEL });
    expect(control).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
    expect(paneView().state.doc.toString()).toBe(BODY);

    fireEvent.click(control);
    await settle();

    // Folded: the form is off the screen, and the prose is still the prose.
    expect(control).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(paneView().state.doc.toString()).toBe(BODY);

    fireEvent.click(control);
    await settle();

    // And back, with no re-read behind it.
    expect(control).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
    expect(paneView().state.doc.toString()).toBe(BODY);
  });

  it("survives a remount, because the frame outlives the file it shows", async () => {
    const first = mount({}, MARKDOWN, ADDRESS);
    await settle();
    // FOLDING is the answer to remember now (Story 54.2): open is the default,
    // so a test that pressed the control to open it and then asserted it was
    // open would pass on a build that remembered nothing at all.
    fireEvent.click(screen.getByRole("button", { name: PROPERTIES_LABEL }));
    await settle();
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    // Written where a reload can find it, and the encoding is the shared one.
    expect(readFileFrameFold(document.cookie).properties).toBe(true);

    // A folded panel unmounts its body and a panel replaces its target in place,
    // so this is the ordinary way a file pane goes away and comes back — and
    // `useState` in the frame lost the answer both times.
    first.unmount();
    mount({}, MARKDOWN, ADDRESS);
    await settle();

    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(screen.getByRole("button", { name: PROPERTIES_LABEL })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("comes up folded when the cookie the last run left says so", async () => {
    // The mount point's own claim, and the only thing that can fail here: the
    // store's suite calls the hydrate itself and would pass on a frame that
    // never did (DW-172). No store arrangement — a real cookie and a real mount.
    //
    // The cookie says FOLDED because open is the default now (Story 54.2): a
    // frame that never hydrated would show the form, and a cookie saying "open"
    // could no longer tell the two apart.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = fileFrameFoldCookie({ properties: true, caveat: true });

    mount({}, MARKDOWN, ADDRESS);
    await settle();

    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(screen.getByRole("button", { name: PROPERTIES_LABEL })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });
});

/**
 * Story 53.3: AD-102's caveat folds to ONE Rust-composed line, and never to
 * nothing (FR-318).
 *
 * The decision being narrowed is `files_write.rs:675-679` — the standing fact has
 * to be on screen BEFORE the first keystroke, because a person who finds out
 * after saving that this file has no history has already lost what history would
 * have given them. The fold keeps that: what is on screen by default names what
 * is missing, and the full four sentences are one press away.
 */
describe("the caveat fold", () => {
  /** The frame over a file keeper will write and does not manage. */
  function mountUnmanaged(): RenderResult {
    return mount({}, MARKDOWN, null, {
      writeCaveat: CAVEAT_FULL,
      writeCaveatShort: CAVEAT_SHORT,
    });
  }

  it("stands on the short line before the first keystroke, and it names what is missing", () => {
    mountUnmanaged();

    const band = screen.getByTestId(TEXT_FILE_CAVEAT_TESTID);
    // Rust's own short sentence, character for character — NOT a prefix of the
    // long one, which is what a webview that clipped the text would produce.
    expect(band).toHaveTextContent(CAVEAT_SHORT);
    expect(CAVEAT_FULL.startsWith(CAVEAT_SHORT)).toBe(false);
    // And what it still says, which is the whole of why AD-102 survives the fold.
    for (const absent of ["no note history", "no search index", "no conflict copy"]) {
      expect(band).toHaveTextContent(absent);
    }
    // The band is above the editor, where the standing fact belongs.
    const editor = document.querySelector(".cm-content, [role='tablist']");
    expect(editor).not.toBeNull();
    expect(band.compareDocumentPosition(editor as Node) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("shows the whole sentence on request, and folds back", () => {
    mountUnmanaged();
    const control = screen.getByRole("button", { name: TEXT_FILE_CAVEAT_LABEL });
    expect(control).toHaveAttribute("aria-expanded", "false");
    // The region the control names is on screen in both states, so the promise is
    // one this surface can keep — a dangling `aria-controls` is a promise it
    // cannot.
    const region = control.getAttribute("aria-controls");
    expect(document.getElementById(region as string)).not.toBeNull();

    fireEvent.click(control);

    const band = screen.getByTestId(TEXT_FILE_CAVEAT_TESTID);
    expect(band).toHaveTextContent(CAVEAT_FULL);
    expect(control).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(control);

    expect(screen.getByTestId(TEXT_FILE_CAVEAT_TESTID)).toHaveTextContent(CAVEAT_SHORT);
  });

  it("remembers the fold across a remount, and per surface rather than per file", () => {
    const first = mountUnmanaged();
    fireEvent.click(screen.getByRole("button", { name: TEXT_FILE_CAVEAT_LABEL }));
    expect(screen.getByTestId(TEXT_FILE_CAVEAT_TESTID)).toHaveTextContent(CAVEAT_FULL);
    expect(readFileFrameFold(document.cookie).caveat).toBe(false);

    first.unmount();
    // A different file, with its own sentence: the preference is about how this
    // reader reads files, not about `AGENTS.md`.
    mount({}, MARKDOWN, null, {
      writeCaveat: `other.md${CAVEAT_FULL.slice("AGENTS.md".length)}`,
      writeCaveatShort: `other.md${CAVEAT_SHORT.slice("AGENTS.md".length)}`,
    });

    expect(screen.getByTestId(TEXT_FILE_CAVEAT_TESTID)).toHaveTextContent("other.md is not one of");
    expect(screen.getByTestId(TEXT_FILE_CAVEAT_TESTID)).toHaveTextContent(
      "Nothing about how Vault syncs this folder changes.",
    );
  });

  it("says nothing at all about a file keeper manages", () => {
    mount({}, MARKDOWN, null);

    expect(screen.queryByTestId(TEXT_FILE_CAVEAT_TESTID)).toBeNull();
    expect(screen.queryByRole("button", { name: TEXT_FILE_CAVEAT_LABEL })).toBeNull();
  });

  it("keeps a caveat whole when its host carries no short form", () => {
    // A host written before this story, or one Rust answered with only the long
    // sentence. The fact stays on screen in full rather than folding to nothing:
    // the fold is the preference and the sentence is the invariant.
    mount({}, MARKDOWN, null, { writeCaveat: CAVEAT_FULL });

    expect(screen.getByTestId(TEXT_FILE_CAVEAT_TESTID)).toHaveTextContent(CAVEAT_FULL);
    expect(screen.queryByRole("button", { name: TEXT_FILE_CAVEAT_LABEL })).toBeNull();
  });
});

/**
 * Story 53.3: one title bar, for a host that gave up its own (FR-317).
 *
 * The five states a naive port of Story 50.1 strands. `panel-strip.tsx` gives up
 * its row on this component's promise, so what is asserted here is the promise
 * itself: handed a `frame`, this frame draws a header in EVERY state it can
 * render — including the four in which it used to draw none.
 */
describe("the merged title bar", () => {
  /** The row's fourth group, which is where a host's controls land. */
  function frameGroup(): HTMLElement | null {
    return (
      bar()?.querySelector<HTMLElement>(`:scope > [data-slot="${PANE_HEADER_FRAME_SLOT}"]`) ?? null
    );
  }

  it("carries the name, the save word, Save and the host's controls in ONE row", () => {
    mount({ dirty: true }, MARKDOWN, ADDRESS, { frame: FRAME_CONTROLS });

    // One header, and everything the panel's row used to say is in it.
    expect(document.querySelectorAll("header")).toHaveLength(1);
    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveTextContent("readme.md");
    expect(shownWord()).toBe(fileSaveWord(true));
    expect(screen.getByRole("button", { name: FILE_SAVE_LABEL })).toBeEnabled();
    // The host's controls are in the fourth group, never in the surface's own —
    // group 3 may demote a member into an overflow menu, and the way out of a
    // panel must not be somewhere that depends on how wide the panel is.
    const fourth = frameGroup();
    expect(fourth).not.toBeNull();
    expect(within(fourth as HTMLElement).getByRole("button", { name: FRAME_CONTROL_LABEL }));
    expect(
      within(group(PANE_HEADER_ACTIONS_SLOT)).queryByRole("button", { name: FRAME_CONTROL_LABEL }),
    ).toBeNull();
  });

  it("draws the row while the file is still opening", () => {
    mount({ loading: true }, MARKDOWN, ADDRESS, { frame: FRAME_CONTROLS });

    // The state that would otherwise leave a panel with no title, no fold and no
    // close for the whole of a pendrive's read.
    expect(bar()).not.toBeNull();
    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveTextContent("readme.md");
    expect(screen.getByRole("button", { name: FRAME_CONTROL_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("opening readme.md");
    // Nothing to save and nothing to say about a buffer that has not arrived.
    expect(screen.queryByRole("button", { name: FILE_SAVE_LABEL })).toBeNull();
  });

  it("draws the row for a file it could not read at all", () => {
    render(
      <TextFileFrame
        fileName="gone.md"
        entry={MARKDOWN}
        state={state({ vm: null, error: "keeper could not read gone.md." })}
        csv={null}
        properties={null}
        preview={{ vaultId: null }}
        frame={FRAME_CONTROLS}
      />,
    );

    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveTextContent("gone.md");
    expect(screen.getByRole("button", { name: FRAME_CONTROL_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("keeper could not read gone.md.");
  });

  it("draws the row over bytes that are not text", () => {
    mount({ vm: vm({ binary: true, detail: "readme.md is not text." }) }, MARKDOWN, ADDRESS, {
      frame: FRAME_CONTROLS,
    });

    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveTextContent("readme.md");
    expect(screen.getByRole("button", { name: FRAME_CONTROL_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("readme.md is not text.");
  });

  it("draws the row for a file no save can follow, and reserves nothing there", () => {
    mount({ dirty: true }, LOCKED, ADDRESS, { frame: FRAME_CONTROLS });

    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveTextContent("readme.md");
    expect(screen.getByRole("button", { name: FRAME_CONTROL_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: FILE_SAVE_LABEL })).toBeNull();
    // No status group at all, rather than an empty reserved one: this row exists
    // because the host handed its controls over, and there is no buffer here that
    // can be dirty (`pane-header.tsx` on why the two are different).
    expect(bar()?.querySelector(`:scope > [data-slot="${PANE_HEADER_STATUS_SLOT}"]`)).toBeNull();
  });

  /** The name element itself, which is what carries the treatment. */
  function nameElement(): Element {
    const found = group(PANE_HEADER_IDENTITY_SLOT).firstElementChild;
    if (found === null) {
      throw new Error("the identity group drew no name");
    }
    return found;
  }

  it("names the file in the panel-title typography every other panel title wears", () => {
    mount({ dirty: true }, MARKDOWN, ADDRESS, { frame: FRAME_CONTROLS });

    // `FOLD_STRIP.titleClass` itself rather than a copy of its words: this row IS
    // the panel's title row now, `panel-strip.tsx` gave up its own on that basis,
    // and a strip holding `notes.md`, `report.pdf` and a note must not show three
    // treatments for one thing. Folding the `.md` used to change the size of its
    // own name, because the folded strip draws it in this class and the bar drew
    // it in another.
    const name = nameElement();
    expect(name).toHaveTextContent("readme.md");
    expect(name).toHaveClass(...FOLD_STRIP.titleClass.split(" "));
    // And not the subordinate treatment it wore while it was a SECOND bar under
    // the panel's own title row: 12px/500 beside every other panel's 15px/600.
    expect(name).not.toHaveClass("text-xs");
    expect(name).not.toHaveClass("font-medium");
  });

  it("leaves the name small for a host that draws its own row", () => {
    // The note embed (`file-embed-host.tsx`), which passes no `frame`. This bar
    // is not a panel title there — it is a label inside somebody's document, and
    // a 15px heading would outshout the prose around it.
    mount({ dirty: true }, MARKDOWN, ADDRESS);

    const name = nameElement();
    expect(name).toHaveTextContent("readme.md");
    expect(name).toHaveClass("font-medium", "text-xs");
    expect(name).not.toHaveClass("font-heading");
    expect(name).not.toHaveClass("text-title");
  });
});

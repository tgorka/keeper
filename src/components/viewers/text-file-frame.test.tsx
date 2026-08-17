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
import { act, fireEvent, type RenderResult, render, screen } from "@testing-library/react";
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

import {
  PANE_HEADER_ACTIONS_SLOT,
  PANE_HEADER_IDENTITY_SLOT,
  PANE_HEADER_STATUS_SLOT,
} from "@/components/layout/pane-header";
import { PROPERTIES_LABEL } from "@/components/notes/properties-panel";
import {
  FILE_SAVE_CLEAN_TITLE,
  FILE_SAVE_LABEL,
  FILE_SAVE_SIZERS,
  type FilePropertiesCoordinates,
  fileSaveWord,
  TextFileFrame,
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

function mount(
  over: Partial<UseTextFileResult> = {},
  entry: ViewerEntry = MARKDOWN,
  properties: FilePropertiesCoordinates | null = null,
): RenderResult {
  return render(
    <TextFileFrame
      fileName="readme.md"
      entry={entry}
      state={state(over)}
      csv={null}
      properties={properties}
      preview={{ vaultId: null }}
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
 */
describe("a file's own properties", () => {
  it("are offered for a writable markdown file the surface can address", async () => {
    mount({}, MARKDOWN, ADDRESS);

    expect(await screen.findByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
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

    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });

  it("are not offered where no save could follow them", () => {
    // The format keeper will not rewrite, and the file only the first megabyte
    // of which was read. Both already take the Save button away; a panel whose
    // every control announced its own refusal would be strictly worse than not
    // being there.
    mount({}, LOCKED, ADDRESS);
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();

    mount(
      { vm: vm({ oversize: true, detail: "readme.md is larger than 1.0 MB." }) },
      MARKDOWN,
      ADDRESS,
    );
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });

  it("are not offered by a host that holds no sync-profile address", () => {
    // The note embed. It has a vault id and a vault-relative target, which is a
    // different identifier over overlapping bytes, and deriving one from the
    // other in the webview is what AD-65 forbids.
    mount({}, MARKDOWN, null);

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
});

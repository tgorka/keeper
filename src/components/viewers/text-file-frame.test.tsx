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
 */
import { type RenderResult, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TextFileVm } from "@/lib/ipc/client";
import type { ViewerEntry } from "@/lib/viewers";

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
  syncReadFrontmatter: vi.fn(async () => ""),
  syncWriteFrontmatter: vi.fn(async () => ""),
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

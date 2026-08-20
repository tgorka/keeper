/**
 * One component, a file's views, and a toggle that remembers per format
 * (Story 45.4, FR-177, AD-88, UX-DR67; Story 51.5, FR-294).
 *
 * **Raw is always editable, and it is the same bytes.** AD-88 exists so keeper
 * never grows a read path and a write path that can disagree about what a file
 * says. There is one buffer here: the raw editor holds it, the CSV table writes
 * through Rust and asks the host to re-read it, and nothing in this file ever
 * re-serialises a format. A rendered view that cannot be drawn falls back to
 * raw **out loud** — with the reason and, where there is one, the line — because
 * a silent fallback is how a reader concludes their file changed.
 *
 * **Nothing here classifies a file.** The format, the rendered view, the syntax
 * and whether the format may be written all arrive from 45.2's registry row.
 * A second extension list in a viewer is the exact defect 45.2 was written to
 * prevent, and this component switches on `entry.rendered` and `entry.format`
 * and on nothing else.
 *
 * **Nothing here configures a CodeMirror.** The raw half is 45.6's editor,
 * passed in as {@link RawRenderedViewProps.editor}. Injected rather than
 * imported so that this module has exactly one call site to change if that
 * component moves, and so the tests below drive a real controlled input rather
 * than a mock of a module — the difference between testing this toggle and
 * testing a stub of somebody else's editor.
 *
 * **The CSV table is 44.16's, unmodified.** `renderCsvTableInto` is mounted as
 * it stands; the cell grammar, the ragged-row rule, the revision check and the
 * byte-identical splice all stay in `keeper-core::notes::csv`, which is the
 * only thing that writes a CSV. What this file adds is the one thing that
 * module could not know: after a cell lands, the raw buffer the host is holding
 * is stale, so {@link RawRenderedViewProps.onExternalWrite} asks for a re-read.
 *
 * **Markdown has a third view, and it is not a third buffer.** Story 51.5's
 * Note tab is the same live-preview layer the Preview tab mounts, editable
 * (`markdown-preview.ts` records which half of its old refusal that keeps).
 * Everything AD-88 asks for is unchanged: an edit in Note mode is the same
 * `onChange` the Source tab reports, `Mod-s` is the same `onSave` it calls, and
 * there is no autosave on either. What the two panes must NOT share is a
 * remount-on-text, and what they must both be keyed on is the FILE rather than
 * its display name — see {@link MarkdownPane}, which records both and why.
 *
 * **And it is where a person writes, so it is the default and it has the
 * toolbar.** Story 52.3: Note mode mounts the same writing tools the Notes
 * surface has, its toolbar is rendered by {@link MarkdownPane} because that is
 * what holds the live view, and a savable markdown file with no remembered
 * choice opens in it. A remembered choice still wins — the jar is read first and
 * only an absent answer takes the default — and that memory is PER FORMAT, which
 * is `view-mode.ts`'s own rule rather than a per-file one: the answer a reader
 * gave for markdown is the answer for every markdown file, and no stored answer
 * moves under anyone. A `note` choice this file cannot honour still lights
 * Preview. The two live panes also stop drawing the leading `---` block the
 * properties FORM above them is HOLDING — the form's own bytes, never a second
 * parse of the buffer — without changing one byte of what a save writes.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CsvTableOptions } from "@/components/notes/editor/csv-table";
import { renderCsvTableInto } from "@/components/notes/editor/csv-table";
import type { FormatAction } from "@/components/notes/editor/format-commands";
import { FormatToolbar } from "@/components/notes/format-toolbar";
import { readFrontmatter } from "@/components/notes/properties-panel";
import { writeCookie } from "@/components/ui/cookie-writer";
import { useWindowedRows } from "@/components/ui/window-list";
import { notesCsvSetCell } from "@/lib/ipc/client";
import type { LanguageId, RenderedView, ViewerFormat } from "@/lib/viewers";
import { buildHtmlView, spliceText, TEXT_RUN_ATTR } from "./html-view";
import type { JsonParseError, JsonRow, JsonStructure } from "./json-structure";
import { parseJsonlStructure, parseJsonStructure } from "./json-structure";
import type { MarkdownEditing, MarkdownPreview, MarkdownPreviewOptions } from "./markdown-preview";
import { mountMarkdownPreview } from "./markdown-preview";
import type { FileOrigin } from "./use-text-file";
import type { OfferedViews, ViewMode } from "./view-mode";
import { viewModeCookie, viewModeFor } from "./view-mode";

/**
 * What the raw half needs. Story 45.6 owns the component; this is the shape it
 * publishes, restated structurally so this module does not import a file it
 * does not own in order to name a type.
 */
export interface RawEditorProps {
  /** Controlled. The exact bytes, in; `onChange` gives the exact bytes back. */
  content: string;
  /** The registry row's syntax, never derived from the name here. */
  language: LanguageId | null;
  /** Display and aria only. */
  fileName: string;
  /** Rust's formatted size, for the oversize banner 45.6 renders. Absent, that
   *  banner cannot name a size, which is a worse message — so a caller holding
   *  a `TextFileVm` passes `vm.sizeLabel`. */
  sizeLabel?: string;
  readOnly?: boolean;
  onChange?: (next: string) => void;
  onSave?: (next: string) => void | Promise<void>;
  path?: string;
  vault?: string | null;
  /** Whether this buffer gets Story 50.3's markdown writing tools. The Source
   *  tab's editor is the only place they can mount, which is why this shape
   *  names them at all: the rendered half is read-only (AD-88). */
  writingTools?: boolean;
  /**
   * Which file these bytes came from, for the editor to key its view on. The
   * identity and not the label: `path` and `vault` above are what the raw
   * editor announces to a screen reader, and a name is not an identity — two
   * files with one basename in two directories are one ordinary session layout
   * (story 51.1), and a panel replaces its target in place.
   */
  loadedFrom?: FileOrigin;
}

/** What 44.16's CSV commands understand. A **notes vault** id and a target
 *  inside it — which is not a sync profile id and is never derived from one. */
export interface CsvCoordinates {
  readonly vaultId: string;
  readonly target: string;
}

export interface RawRenderedViewProps {
  /** The file's own name. Display and aria only; never a path. */
  fileName: string;
  /**
   * Which file the buffer in front of the reader came from (story 51.5's fix).
   *
   * Not a second address to write through — the host owns every command — and
   * not display: it is what the two live views are rebuilt on, so a new FILE
   * gets a new editor while new BYTES do not. {@link fileName} cannot answer
   * that question, which is the defect this prop exists for.
   */
  loadedFrom: FileOrigin;
  /** The registry row's format — the key the remembered view is stored under. */
  format: ViewerFormat;
  /** The registry row's rendered view, or null when raw is the only one. */
  rendered: RenderedView | null;
  /** The registry row's syntax id, handed straight to the raw editor. */
  language: LanguageId | null;
  /** The file's text. The host loads it; this component never reads a disk. */
  content: string;
  /** Rust's formatted size, passed straight through to the raw editor. Never
   *  computed here: `keeper_core::size::format_file_size` is the one formatter,
   *  and a TypeScript one would disagree with the Files pane about the same
   *  file. */
  sizeLabel?: string;
  /** Neither view may write. Both of 45.2's questions have to say yes first. */
  readOnly?: boolean;
  /** Why writing is refused — a finished sentence, shown rather than implied.
   *  A surface that disables an action without saying why teaches distrust. */
  readOnlyReason?: string | null;
  onChange?: (next: string) => void;
  onSave?: (next: string) => void | Promise<void>;
  /** Where 44.16's CSV table should read and write, or null when this file is
   *  not inside a notes vault. Null renders a sentence, never an empty table. */
  csv?: CsvCoordinates | null;
  /** What the markdown preview resolves embeds against. */
  preview?: MarkdownPreviewOptions;
  /**
   * Whether the Source tab's editor gets the markdown writing tools — the format
   * toolbar, the slash menu and emoji completion (Story 50.3, FR-233).
   *
   * Passed straight through to the raw editor and nowhere else. That IS the
   * Source/rendered split doing the deciding: the rendered half of a markdown
   * file is a read-only preview (AD-88), and it is not mounted at the same time
   * as the editor, so a tab switch removes the tools by removing the view they
   * were mounted in rather than by a second rule about tabs.
   */
  writingTools?: boolean;
  /**
   * Whether this markdown file is offered Story 51.5's Note tab (FR-294).
   *
   * The caller's verdict and never re-derived here, for the same reason
   * {@link writingTools} is: it is the registry's `format`, the size guard and
   * Rust's own write refusal, and this component holds none of the three. What
   * it does hold is the second half of the question — whether there is a
   * rendered markdown view to be editable AT ALL — so the two are combined
   * below and nowhere else.
   *
   * It is deliberately the same predicate the writing tools stand on rather
   * than a looser one: Note mode is a way of writing text, and offering it
   * where no save can follow would be a control that announces its own
   * refusal — the shape 45.2 spent a paragraph rejecting.
   */
  noteMode?: boolean;
  /**
   * The `---` block the properties FORM above these views is holding, or `null`
   * when there is no form, when its read has not landed yet, and when that read
   * refused (Story 52.3, FR-304).
   *
   * The BLOCK, and never a boolean about whether a form was mounted. The form's
   * content is Rust's `file_properties::block_of` over the file ON DISK; a
   * boolean made this component re-parse the live BUFFER to guess which bytes
   * that form was drawing, and two recognisers over two sources is exactly how a
   * form and a pane come to disagree. They did, four ways: the block was hidden
   * from the first frame, before the read that fills the form had landed — and
   * for good if that read refused; a file carrying a byte-order mark had its
   * block drawn TWICE, because Rust skips the mark and `readFrontmatter` wants
   * `---` at byte zero; a document whose first line is a `---` thematic break had
   * its own first heading hidden from every view but Source; and typing `---` ⏎
   * `---` at the top of a document made the shown text shorten under the caret,
   * mid-edit.
   *
   * It is a DISPLAY concern and only that. The block stays in {@link content},
   * the Source tab still shows every character of the file, and an edit made in
   * Note mode is reported with the block back in front of it, byte for byte —
   * which is what keeps AD-88's one buffer true. On a note the frontmatter is a
   * separate store field and this question never arises; on a file the buffer IS
   * the whole file, which is why the duplicate could exist at all.
   */
  frontmatterInForm?: string | null;
  /** The rendered view wrote the file; the host's buffer is now stale. */
  onExternalWrite?: () => void;
  /** 45.6's editor. See the module comment for why it is injected. */
  editor: React.ComponentType<RawEditorProps>;
  /** Test seam for 44.16's backend. Defaults to the real commands. */
  csvOptions?: CsvTableOptions;
  /** Where the remembered view lives. Defaults to `document.cookie`; a test
   *  passing its own string is testing the memory rather than the jar. */
  cookie?: { read: () => string; write: (assignment: string) => void };
}

/** What each view is called on its tab. The rendered one says what it IS —
 *  "Rendered" would make the reader guess, and the guess differs per format. */
const RENDERED_LABEL: Record<RenderedView, string> = {
  markdown: "Preview",
  table: "Table",
  structure: "Structure",
  html: "Page",
};

/** The raw tab. "Source", not "Raw": every editor a person has used calls the
 *  characters in the file the source, and this view is editable. */
const RAW_LABEL = "Source";

/** The third tab (Story 51.5). The owner's own word for it, and the word the
 *  app already uses for the surface it behaves like. */
const NOTE_LABEL = "Note";

/** Rows the structure list assumes before one has been measured, in px. */
const STRUCTURE_ROW_HEIGHT = 22;

/** One line of the structure view: a value, or the reason a line is not one. */
type StructureItem =
  | { readonly at: number; readonly row: JsonRow; readonly error?: undefined }
  | { readonly at: number; readonly row?: undefined; readonly error: JsonParseError };

/** A parse failure worded for the banner, with the line it is about. */
function errorSentence(error: JsonParseError): string {
  return `line ${error.line}, column ${error.column}: ${error.message}`;
}

/** What a row shows in its value column. A container states its size, because
 *  the members are the rows beneath it and repeating them here says nothing. */
function valueLabel(row: JsonRow): string {
  if (row.kind === "object") {
    return `${row.count ?? 0} ${row.count === 1 ? "property" : "properties"}`;
  }
  if (row.kind === "array") {
    return `${row.count ?? 0} ${row.count === 1 ? "item" : "items"}`;
  }
  if (row.kind === "string" && row.text === "") {
    // A blank cell reads as a missing value; this one is present and empty.
    return "(empty text)";
  }
  return row.text ?? "";
}

/** What a row is called: its key, its index, or the document itself. */
function nameLabel(row: JsonRow): string {
  if (row.key !== null) {
    return row.key;
  }
  if (row.index !== null) {
    return `[${row.index}]`;
  }
  return "(the whole file)";
}

/** A notice the reader is meant to read, not an error. */
function Notice({ children }: { children: React.ReactNode }): React.ReactElement {
  return (
    <p className="px-3 py-1.5 text-muted-foreground text-xs" role="status">
      {children}
    </p>
  );
}

/**
 * JSON and JSONL, drawn as the values they hold.
 *
 * Windowed with 44.10's list, because {@link MAX_STRUCTURE_ROWS} is five
 * thousand and five thousand DOM rows is a pane that stops responding — the
 * kind of thing that passes every test and fails on the first real export.
 */
/**
 * The rendered half of an HTML file (Story 55.5).
 *
 * Editable text and nothing else: a reader may retype a paragraph, and an
 * attribute, an element or the shape of the document are the Source tab's — the
 * one tab that can change anything (AD-88).
 *
 * **Rebuilt on the file, not on the buffer.** Every keystroke changes
 * `content`, and rebuilding the view under a caret would move it to the start
 * of the document on every character. So the built view is the one made when
 * this file was opened, and `content` is read imperatively at the moment an
 * edit is spliced — which is also the moment {@link spliceText} verifies that
 * the bytes it is about to replace are still the ones it was built from.
 */
function HtmlPane({
  content,
  readOnly,
  onChange,
  onRefusal,
}: {
  content: string;
  readOnly: boolean;
  onChange: ((next: string) => void) | undefined;
  onRefusal: (reason: string) => void;
}): React.ReactElement {
  const hostRef = useRef<HTMLDivElement | null>(null);
  // Read imperatively: see the doc above. A dependency on `content` would
  // remount the view on every keystroke.
  const latest = useRef({ content, onChange, onRefusal });
  latest.current = { content, onChange, onRefusal };
  // The source the view was built from, which is what a splice is checked
  // against — not the buffer, which the reader's own typing has already moved.
  const builtFrom = useRef(content);

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return;
    }
    builtFrom.current = latest.current.content;
    const view = buildHtmlView(latest.current.content);
    host.replaceChildren(view.node);
    if (readOnly) {
      return () => host.replaceChildren();
    }
    for (const span of Array.from(host.querySelectorAll<HTMLElement>(`[${TEXT_RUN_ATTR}]`))) {
      // A run of pure whitespace is a text node between two blocks — the
      // newline after a `</p>`. It is a real run and keeps its index, because
      // the mapping is by order, but it is not something a reader can see, and
      // an invisible edit target is a place to lose a keystroke.
      if ((span.textContent ?? "").trim() === "") {
        continue;
      }
      // `setAttribute`, not the property: the property setter is not
      // implemented everywhere (jsdom does nothing with it, silently), and the
      // attribute is what the engine reads either way.
      //
      // `plaintext-only` rather than `true`: a reader retypes words, and a
      // paste of rich text would be a second way to write markup into a file
      // whose markup only the Source tab may change. An engine that does not
      // know the value treats it as `inherit` and the text is simply not
      // editable — a lost capability, never a wrong edit.
      span.setAttribute("contenteditable", "plaintext-only");
      span.spellcheck = false;
    }
    const onInput = (event: Event): void => {
      const span = (event.target as HTMLElement | null)?.closest<HTMLElement>(`[${TEXT_RUN_ATTR}]`);
      const run = view.runs[Number(span?.getAttribute(TEXT_RUN_ATTR))];
      if (span === null || span === undefined || run === undefined) {
        return;
      }
      const spliced = spliceText(builtFrom.current, run, span.textContent ?? "");
      if (!spliced.ok) {
        latest.current.onRefusal(spliced.reason);
        return;
      }
      // The new file becomes what the next edit is checked against, so a second
      // edit in another paragraph splices into the document as it now is.
      builtFrom.current = spliced.text;
      latest.current.onChange?.(spliced.text);
    };
    host.addEventListener("input", onInput);
    return () => {
      host.removeEventListener("input", onInput);
      host.replaceChildren();
    };
    // `content` is deliberately absent from the dependencies: rebuilding on
    // every keystroke would move the caret to the top of the document on every
    // character, so the buffer is read imperatively through `latest` instead.
    // (No suppression needed — the rule is not on here, and a suppression that
    // suppresses nothing is a comment pretending to be a guarantee.)
  }, [readOnly]);

  return (
    <div
      ref={hostRef}
      data-testid={HTML_PANE_TESTID}
      className="keeper-html-page h-full min-h-0 overflow-auto px-3 py-2 text-sm"
    />
  );
}

/** Test id for the page view, on the same terms as the PDF's. */
export const HTML_PANE_TESTID = "html-page";

function StructurePane({ structure }: { structure: JsonStructure }): React.ReactElement {
  const items = useMemo<StructureItem[]>(() => {
    const merged: StructureItem[] = structure.rows.map((row, at) => ({ at, row }));
    for (const error of structure.errors) {
      // After every value on the same line: the values are what parsed, the
      // error is where the line stopped parsing.
      const before = merged.filter((item) => (item.row?.line ?? 0) <= error.line).length;
      merged.splice(before, 0, { at: merged.length, error });
    }
    return merged;
  }, [structure]);

  const getKey = useCallback((index: number) => index, []);
  const list = useWindowedRows({ count: items.length, getKey, rowHeight: STRUCTURE_ROW_HEIGHT });

  return (
    <div className="h-full min-h-0 overflow-auto" {...list.viewportProps}>
      <ul className="relative m-0 list-none p-0" style={{ height: `${list.totalSize}px` }}>
        {list.rows.map((windowed) => {
          const item = items[windowed.index];
          const props = list.rowProps(windowed);
          if (item.error !== undefined) {
            return (
              <li
                key={windowed.key}
                {...props}
                className="flex gap-2 px-3 text-destructive text-xs"
              >
                {errorSentence(item.error)}
              </li>
            );
          }
          const { row } = item;
          return (
            <li
              key={windowed.key}
              {...props}
              className="flex items-baseline gap-2 px-3 font-mono text-xs"
              data-structure-depth={row.depth}
            >
              <span
                className="shrink-0 truncate text-foreground"
                style={{ paddingLeft: `${row.depth * 14}px` }}
              >
                {nameLabel(row)}
              </span>
              <span className="shrink-0 text-muted-foreground">{row.kind}</span>
              <span className="truncate text-muted-foreground">{valueLabel(row)}</span>
              {row.duplicate ? (
                // A repeated key is not an error and is not dropped: it is a
                // fact about the file, and the later one is the one every JSON
                // reader downstream will keep. A sentence, so it leaves the
                // column face the name/kind/value cells beside it are set in.
                <span className="shrink-0 font-sans text-destructive">
                  repeated key — this one wins
                </span>
              ) : null}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** 44.16's table, mounted as it stands, with the one thing it cannot know:
 *  that a landed cell makes the host's raw buffer stale. */
function CsvPane({
  coordinates,
  options,
  onExternalWrite,
}: {
  coordinates: CsvCoordinates;
  options: CsvTableOptions | undefined;
  onExternalWrite: (() => void) | undefined;
}): React.ReactElement {
  const hostRef = useRef<HTMLDivElement | null>(null);
  // Read imperatively so a caller who rebuilds these objects every render does
  // not re-mount the table under a half-typed cell.
  const latest = useRef({ options, onExternalWrite });
  latest.current = { options, onExternalWrite };

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return;
    }
    let cancelled = false;
    void renderCsvTableInto(host, coordinates.vaultId, coordinates.target, {
      ...latest.current.options,
      cancelled: () => cancelled,
      setCell: async (vaultId, target, rev, row, column, value) => {
        const write = latest.current.options?.setCell ?? notesCsvSetCell;
        const next = await write(vaultId, target, rev, row, column, value);
        // Announced only after Rust confirmed the write. A refusal — a stale
        // revision, a column the row does not have — must not make the host
        // discard what the reader has in the raw editor.
        latest.current.onExternalWrite?.();
        return next;
      },
    });
    return () => {
      cancelled = true;
      host.replaceChildren();
    };
  }, [coordinates.vaultId, coordinates.target]);

  return <div ref={hostRef} className="h-full min-h-0 overflow-auto px-3 py-2 text-xs" />;
}

/**
 * What `livePreview` was built with, as one comparable value.
 *
 * `mountMarkdownPreview` reads its options once, at construction, so a change to
 * one of them cannot reach a live view: the pane has to be rebuilt for it to
 * take, and {@link MarkdownPane}'s effect is keyed on this.
 *
 * The vault id is a value and compares as one. The five callbacks are compared
 * by PRESENCE and deliberately not by identity: what the reader sees differs
 * between a host that can follow a wikilink and one that cannot, while a host
 * that spells a closure inline would rebuild the pane on every render if
 * identity counted — destroying the caret and the undo stack that dropping the
 * `[text]` key exists to keep. A host that changes what a callback DOES without
 * changing whether it has one is therefore not adopted; that is the one gap this
 * leaves, and both real hosts memoise their closures per vault.
 */
function previewShape(options: MarkdownPreviewOptions | undefined): string {
  const answers =
    (options?.assetUrl === undefined ? "" : "a") +
    (options?.onOpenLink === undefined ? "" : "l") +
    (options?.onOpenUrl === undefined ? "" : "u") +
    (options?.listFolder === undefined ? "" : "f") +
    (options?.mountWidget === undefined ? "" : "w");
  // The vault id last, so nothing it can hold looks like one of the answers.
  return `${answers}:${options?.vaultId ?? ""}`;
}

/**
 * The bytes at the head of `content` that ARE `block` — the block itself, or a
 * byte-order mark and then the block — or `null` when the buffer does not carry
 * that block at all (Story 52.3, FR-304).
 *
 * `block` is the properties form's own, so nothing here recognises where a block
 * starts or ends: this only asks whether the buffer still begins with the bytes
 * the form is holding. That is the whole difference from the `readFrontmatter`
 * parse this replaced — a pane may hide bytes because something else on screen
 * is drawing them, and never because a second parser thinks they look like
 * frontmatter.
 *
 * The mark is admitted because Rust deliberately leaves it out of its answer
 * (`file_properties::block_of` says why, and `replace_block` re-attaches it), so
 * without this a file Excel had touched would show its block in the form AND in
 * the pane. It belongs to the PREFIX rather than to the body, so that what goes
 * back in front of an edit is exactly what came off the front of it.
 *
 * Exported for `text-file-frame.tsx`, which splices a landed block over this
 * same prefix rather than re-reading the file over somebody's unsaved text.
 */
export function leadingFormBlock(content: string, block: string | null | undefined): string | null {
  if (block === null || block === undefined || block === "") {
    return null;
  }
  if (content.startsWith(block)) {
    return block;
  }
  const mark = "\u{feff}";
  return content.startsWith(mark) && content.startsWith(block, mark.length) ? mark + block : null;
}

/**
 * The note editor's own live-preview layer over a file — read-only for the
 * Preview tab, editable for Note mode — reporting a refusal upward rather than
 * leaving an empty box behind.
 *
 * One component for both, because they are one view with one parameter between
 * them (Story 51.5). A second component would be a second place for the file
 * surface and the note surface to disagree about what markdown looks like,
 * which is what `markdown-preview.ts` refuses in its first paragraph.
 *
 * It also renders Note mode's format toolbar (Story 52.3), because the control
 * has to sit with the view it acts on: the extensions and their translation live
 * inside `mountMarkdownPreview`, and a handle passed upward is how a press lands
 * in a view that has since been replaced by the Preview tab.
 */
function MarkdownPane({
  fileName,
  loadedFrom,
  text,
  options,
  editing,
  onOutcome,
}: {
  /** Display and aria only. Which file this is is {@link loadedFrom}. */
  fileName: string;
  /** Which file the buffer came from. See the effect below. */
  loadedFrom: FileOrigin;
  text: string;
  options: MarkdownPreviewOptions | undefined;
  /**
   * Where an edit goes, or null for the read-only Preview tab.
   *
   * Without the label: the editable region is named from `fileName` below, so
   * a host cannot hand down a name for a file it is not showing.
   */
  editing: Omit<MarkdownEditing, "label"> | null;
  onOutcome: (failure: string | null) => void;
}): React.ReactElement {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const mountRef = useRef<MarkdownPreview | null>(null);
  // Whether there is a live view behind `mountRef` *right now*, as state rather
  // than as the ref itself, because the toolbar's presence is a render decision
  // and a ref does not cause one. The same flag and the same reason
  // `text-viewer.tsx` records for the Source tab's toolbar: the mount is several
  // dynamic imports deep and the cleanup nulls the ref again at the start of
  // every rebuild, so a toolbar drawn from `editable` alone is clickable over no
  // editor for the whole of both windows, and every press in them is swallowed.
  const [mounted, setMounted] = useState(false);
  const latest = useRef({ text, options, editing, onOutcome });
  latest.current = { text, options, editing, onOutcome };
  const editable = editing !== null;
  const shape = previewShape(options);

  const runFormat = useCallback((action: FormatAction) => {
    // Optional twice over, and both are defence rather than a live path: the
    // toolbar is drawn only when the pane is editable AND mounted, so there is a
    // view, and the same `editing` that made it editable is what put the writing
    // extensions in it, so its translation is non-null.
    mountRef.current?.runFormat?.(action);
  }, []);

  // What rebuilds this pane and what does not, because the next person will ask.
  //
  // REBUILDS. The MODE (`editable`), because the extension list is fixed at
  // construction and there is no compartment here. The FILE — both halves of
  // `loadedFrom`, and `fileName` with them because the editable region is named
  // from it — because an undo stack that reaches back into a previous file's
  // text is one ⌘Z and one ⌘S away from writing that file's bytes over this
  // one. A panel replaces its target in place, and story 51.1 made two markdown
  // files with one basename in two directories an ordinary session layout, so
  // the display name cannot answer which file this is. The decoration layer's
  // own OPTIONS (`shape`), because `livePreview` reads them once: a file inside
  // a vault learns its vault a frame after the first paint (`text-file-viewer`
  // hydrates the mirror in an effect), and while nothing was keyed on them that
  // file rendered the out-of-vault degrade — and resolved every wikilink against
  // `""` — for the life of the panel.
  //
  // DOES NOT REBUILD. The BUFFER, which is what this effect used to be keyed on.
  // An editable pane reports every keystroke upward and gets the identical
  // string straight back as a prop, so a text key tore the view down on every
  // character — caret, undo stack and scroll position with it. The buffer flows
  // through `setContent` below instead, the way the raw editor has always
  // adopted it. Nor the CALLBACKS in `editing` and `onOutcome`: they are reached
  // through `latest` because the view outlives every render.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `loadedFrom`'s two halves and `shape` are rebuild triggers, not reads — see above.
  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return;
    }
    let disposed = false;
    void (async () => {
      const pane = await mountMarkdownPreview(host, latest.current.text, {
        vaultId: null,
        ...latest.current.options,
        // Indirected through the ref because the view outlives every render: a
        // handler captured here would report this pane's edits to the first
        // render's host forever.
        editing: editable
          ? {
              label: `Note of ${fileName}`,
              onChange: (next) => latest.current.editing?.onChange(next),
              onSave: (next) => latest.current.editing?.onSave(next),
            }
          : undefined,
      });
      if (disposed) {
        pane.destroy();
        return;
      }
      mountRef.current = pane;
      // The buffer may have moved while the editor chunk was in flight. A no-op
      // when it has not, which is the ordinary case.
      const refused = pane.setContent(latest.current.text);
      // One report for both refusals, because the reader's position is the same
      // either way: this document is not drawn and the source is.
      latest.current.onOutcome(pane.failure ?? refused);
      // Beside the assignment and never apart from it: this is the render the
      // toolbar is allowed to appear on, and the whole point is that it is the
      // one where a press has somewhere to go.
      setMounted(true);
    })();
    return () => {
      disposed = true;
      mountRef.current?.destroy();
      mountRef.current = null;
      host.replaceChildren();
      // Cleared for the rebuild case, which is the guaranteed one: a switch into
      // Preview, a new file or a hydrated vault tears the view down while
      // `editable` may still be true, and without this the toolbar would sit
      // over an empty host for the whole of the next async mount.
      setMounted(false);
    };
  }, [editable, fileName, loadedFrom.profileOrVaultId, loadedFrom.relativePath, shape]);

  useEffect(() => {
    // Reported only when the adoption refused. A `null` here would clear a
    // construction failure that is still true of these bytes — the host keys its
    // refusal on the text it was about, and this effect runs for text that
    // never reached a view.
    const refused = mountRef.current?.setContent(text);
    if (refused != null) {
      latest.current.onOutcome(refused);
    }
  }, [text]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Directly over the text it formats, which is the note editor's own
          placement and the Source tab's — a person who learned the toolbar in
          Notes finds it in the same place over a session log. Here rather than in
          the tab strip above for `text-viewer.tsx`'s reason: a toolbar acts on a
          live view, and this component is the one that mounts it. */}
      {editable && mounted ? <FormatToolbar onAction={runFormat} /> : null}
      <div ref={hostRef} className="min-h-0 flex-1 overflow-auto" />
    </div>
  );
}

/** The remembered view, in the one durable store the frontend uses for a pane
 *  preference. Defaulted here rather than at every call site. */
const DOCUMENT_COOKIE = {
  read: () => (typeof document === "undefined" ? "" : document.cookie),
  write: (assignment: string) => {
    if (typeof document !== "undefined") {
      writeCookie(assignment);
    }
  },
};

export function RawRenderedView({
  fileName,
  loadedFrom,
  format,
  rendered,
  language,
  content,
  sizeLabel,
  readOnly,
  readOnlyReason,
  onChange,
  onSave,
  csv,
  preview,
  writingTools,
  noteMode,
  frontmatterInForm,
  onExternalWrite,
  editor: Editor,
  csvOptions,
  cookie = DOCUMENT_COOKIE,
}: RawRenderedViewProps): React.ReactElement {
  // Story 51.5's third mode, and both halves of the question are here. WHETHER
  // this file may be written is `noteMode` — the frame's verdict, which reads
  // the registry's format, the size guard and Rust's own write refusal, and is
  // never re-derived in this component. WHETHER there is a live-preview view to
  // make editable is `rendered === "markdown"`, which is this component's own
  // question and the registry's answer to it.
  //
  // `readOnly` is belt as well as braces. A caller that passes `readOnly` and
  // `noteMode` together is contradicting itself, and the safe reading of a
  // contradiction is the one that does not put an editor over a buffer nothing
  // will accept.
  //
  // Read before the remembered view rather than after it (Story 52.3): this is
  // also the predicate the DEFAULT is a function of, so it has to be known
  // before the first render picks one.
  const noteOffered = rendered === "markdown" && noteMode === true && readOnly !== true;

  // What this file can offer, for `viewModeFor` to default from. A fresh literal
  // at both call sites rather than a memo: it is read during a render and never
  // compared, so a stable identity would buy nothing.
  const offered: OfferedViews = { note: noteOffered };

  // The jar first, the default second — `viewModeFor` records why that order is
  // the whole of how changing the default leaves a reader's own answer alone.
  const [chosen, setChosen] = useState<ViewMode>(() => viewModeFor(cookie.read(), format, offered));
  // Derived from a prop, so it is adopted during render rather than in an
  // effect: an effect would paint one frame of the previous format's view.
  //
  // What is deliberately NOT adopted here is a mid-life change to what is
  // OFFERED. Both hosts settle `noteMode` before this component is mounted at
  // all — the frame renders "opening …" until the loader has a VM, and every
  // input to its verdict rides in on that VM — so a flip would mean a caller
  // changed its mind about a file it was already showing, and re-defaulting
  // under the reader on that would move the view he is looking at. The reverse
  // direction is handled below, where a `note` choice this file cannot honour
  // lights Preview instead.
  const lastFormat = useRef(format);
  if (lastFormat.current !== format) {
    lastFormat.current = format;
    setChosen(viewModeFor(cookie.read(), format, offered));
  }

  // Story 52.3's second ask, and it is a DISPLAY concern only.
  //
  // On a file the buffer IS the whole file, YAML block included — unlike a note,
  // where frontmatter is a separate store field — so a host that draws the block
  // as a form above these views was drawing it twice: once as controls and once
  // as document text. `frontmatterInForm` is the block that form is HOLDING,
  // which is the only thing that can say which bytes the second drawing is.
  //
  // Three things must be true to hide anything, and each one is a way this got it
  // wrong while it was a boolean about whether a form was mounted:
  //
  // 1. The form holds a block. Nothing is hidden while its read is in flight —
  //    a stat and a read that is hundreds of ms on a pendrive — or after it
  //    refused, where the block would otherwise have been in neither the form nor
  //    the text, for good.
  // 2. The buffer's first bytes ARE that block (`leadingFormBlock`, which admits
  //    the byte-order mark Rust leaves out of its answer). Nothing is hidden from
  //    a buffer that has been typed into since, or from one that never matched:
  //    hiding a length rather than a known prefix is what shortened the shown
  //    text under the caret when somebody typed `---` ⏎ `---` at the top.
  // 3. The form DRAWS those bytes. `readFrontmatter` here is the same call
  //    `PropertiesPanel` makes on the same string, so it is the form's own answer
  //    to "will the reader see these characters above the pane" and not a second
  //    opinion about where a block is. It says no for a leading block that holds
  //    no `key: value` line at all — `---\n# Heading\n---\nbody`, whose first
  //    line is a thematic break — and that document's heading has to stay on
  //    screen: the panel shows a tag row and nothing of it.
  //
  // The prefix is kept rather than discarded, because every byte of it goes back
  // in front of an edit before it reaches `onChange` or `onSave` below. The
  // Source tab is untouched: it is the file's characters, and hiding any of them
  // there would be a lie about what a save writes.
  const inForm = rendered === "markdown" ? (frontmatterInForm ?? null) : null;
  const carried = leadingFormBlock(content, inForm);
  const drawnByForm = inForm === null ? null : readFrontmatter(inForm);
  const hiddenBlock =
    carried !== null &&
    drawnByForm !== null &&
    (drawnByForm.unparsed || drawnByForm.entries.length > 0)
      ? carried
      : "";
  /** What the live-preview panes are given. The whole buffer when nothing is
   *  hidden, which is every host that draws no properties form. */
  const shown = hiddenBlock === "" ? content : content.slice(hiddenBlock.length);

  /** A preview that refused, and the text it refused — so the refusal does not
   *  outlive the bytes it was about. */
  const [refusal, setRefusal] = useState<{ text: string; message: string } | null>(null);
  const previewFailure = refusal !== null && refusal.text === shown ? refusal : null;

  const structure = useMemo<JsonStructure | null>(() => {
    if (rendered !== "structure") {
      return null;
    }
    return format === "jsonl" ? parseJsonlStructure(content) : parseJsonStructure(content);
  }, [rendered, format, content]);

  /**
   * Why the rendered view cannot be shown, or null.
   *
   * JSON and JSONL differ here, and the difference is principled. A JSON file
   * is ONE document: if it did not parse there is no structure, and drawing the
   * fragment that parsed before the failure would be a picture of a file that
   * does not exist. A JSONL file is one document per line, so the lines that
   * parsed are whole and true, and withholding them because line 4,000 was
   * truncated throws away most of why the format exists.
   */
  const structureRefusal =
    structure === null || structure.errors.length === 0
      ? null
      : format === "jsonl"
        ? structure.rows.length === 0
          ? errorSentence(structure.errors[0])
          : null
        : errorSentence(structure.errors[0]);

  const csvRefusal =
    rendered === "table" && (csv === null || csv === undefined)
      ? "keeper can only show a CSV as a table inside a notes vault, so this file opens as its source"
      : null;

  const refusalMessage = previewFailure?.message ?? structureRefusal ?? csvRefusal;

  // What the reader asked for, resolved against what THIS file can offer. A jar
  // holding `note` for a file with no Note tab — a `workspace/` markdown file,
  // an oversize one — lights the Preview tab rather than lighting nothing at
  // all, and the jar is left holding `note` so the next writable markdown file
  // still honours it.
  const selected: ViewMode = chosen === "note" && !noteOffered ? "rendered" : chosen;

  // The choice the reader made is NOT changed by a file that cannot be drawn.
  // The next file of this format still opens in the view they asked for. A
  // document the renderer refuses cannot host the note editor either, so that
  // fallback is Source — the one view that is always editable (AD-88).
  const showing: ViewMode =
    refusalMessage !== null || rendered === null || selected === "raw"
      ? "raw"
      : selected === "note"
        ? "note"
        : "rendered";

  const choose = (next: ViewMode): void => {
    setChosen(next);
    cookie.write(viewModeCookie(cookie.read(), format, next));
  };

  const onOutcome = useCallback(
    // The text the PANE was given, which is the buffer minus anything hidden from
    // it: a refusal has to stop applying when the document the renderer choked on
    // changes, and the document it was handed is what it choked on.
    (failure: string | null) => {
      setRefusal(failure === null ? null : { text: shown, message: failure });
    },
    [shown],
  );

  const renderedLabel = rendered === null ? null : RENDERED_LABEL[rendered];

  /** The tabs this file has, in reading order: what the file looks like, then
   *  its characters, then — for writable markdown — writing in the first. */
  const tabs =
    renderedLabel === null
      ? []
      : [
          { mode: "rendered" as const, label: renderedLabel },
          { mode: "raw" as const, label: RAW_LABEL },
          ...(noteOffered ? [{ mode: "note" as const, label: NOTE_LABEL }] : []),
        ];

  return (
    <div className="flex h-full min-h-0 flex-col">
      {renderedLabel === null ? null : (
        <div
          className="flex shrink-0 gap-1 border-b px-2 py-1"
          role="tablist"
          aria-label={fileName}
        >
          {tabs.map((tab) => (
            <button
              key={tab.mode}
              type="button"
              role="tab"
              aria-selected={selected === tab.mode}
              className="rounded px-2 py-0.5 text-xs aria-selected:bg-muted"
              onClick={() => choose(tab.mode)}
            >
              {tab.label}
            </button>
          ))}
        </div>
      )}

      {refusalMessage === null ? null : (
        // `alert`, and it names the file's own words: the reader has to be able
        // to tell "keeper will not draw this" from "my file changed".
        <p className="shrink-0 border-b px-3 py-1.5 text-destructive text-xs" role="alert">
          {refusalMessage}
        </p>
      )}

      {readOnly !== true || readOnlyReason == null ? null : <Notice>{readOnlyReason}</Notice>}

      {showing === "rendered" && structure !== null && structure.empty ? (
        <Notice>this file is empty, so there is nothing to show as a structure</Notice>
      ) : null}

      {showing === "rendered" &&
      structure !== null &&
      structure.rows.length < structure.totalRows ? (
        <Notice>
          showing the first {structure.rows.length} of {structure.totalRows} values
        </Notice>
      ) : null}

      <div className="min-h-0 flex-1" role="tabpanel">
        {showing === "raw" ? (
          <Editor
            content={content}
            language={language}
            fileName={fileName}
            loadedFrom={loadedFrom}
            sizeLabel={sizeLabel}
            readOnly={readOnly}
            onChange={onChange}
            onSave={onSave}
            writingTools={writingTools}
          />
        ) : rendered === "markdown" ? (
          <MarkdownPane
            fileName={fileName}
            loadedFrom={loadedFrom}
            text={shown}
            options={preview}
            // One buffer and one Save, which is the whole of how Note mode adds
            // no write path: an edit here is the same `onChange` the Source tab
            // reports and `Mod-s` is the same `onSave` it calls.
            //
            // `hiddenBlock` goes back in front of the edit, byte for byte, and it
            // is `""` for every host that hides nothing. That is what keeps the
            // hiding a display concern: the pane is looking at the body, the
            // buffer this reports is the whole file, and a save writes the block
            // it was never shown.
            editing={
              showing === "note"
                ? {
                    onChange: (next) => onChange?.(hiddenBlock + next),
                    onSave: (next) => void onSave?.(hiddenBlock + next),
                  }
                : null
            }
            onOutcome={onOutcome}
          />
        ) : rendered === "table" && csv != null ? (
          <CsvPane coordinates={csv} options={csvOptions} onExternalWrite={onExternalWrite} />
        ) : rendered === "html" ? (
          <HtmlPane
            content={content}
            readOnly={readOnly === true}
            onChange={onChange}
            // The banner Story 45.4 already built for a preview that refused:
            // a refused splice is the same kind of fact — keeper declining to
            // act on bytes, with a sentence — and it clears when the buffer
            // changes, which is exactly when the refusal stops being true.
            onRefusal={onOutcome}
          />
        ) : structure !== null ? (
          <StructurePane structure={structure} />
        ) : null}
      </div>
    </div>
  );
}

/**
 * One component, two views, a toggle that remembers per format
 * (Story 45.4, FR-177, AD-88, UX-DR67).
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
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CsvTableOptions } from "@/components/notes/editor/csv-table";
import { renderCsvTableInto } from "@/components/notes/editor/csv-table";
import { writeCookie } from "@/components/ui/cookie-writer";
import { useWindowedRows } from "@/components/ui/window-list";
import { notesCsvSetCell } from "@/lib/ipc/client";
import type { LanguageId, RenderedView, ViewerFormat } from "@/lib/viewers";
import type { JsonParseError, JsonRow, JsonStructure } from "./json-structure";
import { parseJsonlStructure, parseJsonStructure } from "./json-structure";
import type { MarkdownPreview, MarkdownPreviewOptions } from "./markdown-preview";
import { mountMarkdownPreview } from "./markdown-preview";
import type { ViewMode } from "./view-mode";
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
};

/** The raw tab. "Source", not "Raw": every editor a person has used calls the
 *  characters in the file the source, and this view is editable. */
const RAW_LABEL = "Source";

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

/** The note editor's preview over a file, reporting a refusal upward rather
 *  than leaving an empty box behind. */
function MarkdownPane({
  text,
  options,
  onOutcome,
}: {
  text: string;
  options: MarkdownPreviewOptions | undefined;
  onOutcome: (failure: string | null) => void;
}): React.ReactElement {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const latest = useRef({ options, onOutcome });
  latest.current = { options, onOutcome };

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return;
    }
    let disposed = false;
    let preview: MarkdownPreview | null = null;
    void (async () => {
      const mounted = await mountMarkdownPreview(host, text, {
        vaultId: null,
        ...latest.current.options,
      });
      if (disposed) {
        mounted.destroy();
        return;
      }
      preview = mounted;
      latest.current.onOutcome(mounted.failure);
    })();
    return () => {
      disposed = true;
      preview?.destroy();
      host.replaceChildren();
    };
  }, [text]);

  return <div ref={hostRef} className="h-full min-h-0 overflow-auto" />;
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
  onExternalWrite,
  editor: Editor,
  csvOptions,
  cookie = DOCUMENT_COOKIE,
}: RawRenderedViewProps): React.ReactElement {
  const [chosen, setChosen] = useState<ViewMode>(() => viewModeFor(cookie.read(), format));
  // Derived from a prop, so it is adopted during render rather than in an
  // effect: an effect would paint one frame of the previous format's view.
  const lastFormat = useRef(format);
  if (lastFormat.current !== format) {
    lastFormat.current = format;
    setChosen(viewModeFor(cookie.read(), format));
  }

  /** A preview that refused, and the text it refused — so the refusal does not
   *  outlive the bytes it was about. */
  const [refusal, setRefusal] = useState<{ text: string; message: string } | null>(null);
  const previewFailure = refusal !== null && refusal.text === content ? refusal : null;

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
  // The choice the reader made is NOT changed by a file that cannot be drawn.
  // The next file of this format still opens in the view they asked for.
  const showing: ViewMode =
    rendered === null || chosen === "raw" || refusalMessage !== null ? "raw" : "rendered";

  const choose = (next: ViewMode): void => {
    setChosen(next);
    cookie.write(viewModeCookie(cookie.read(), format, next));
  };

  const onOutcome = useCallback(
    (failure: string | null) => {
      setRefusal(failure === null ? null : { text: content, message: failure });
    },
    [content],
  );

  const renderedLabel = rendered === null ? null : RENDERED_LABEL[rendered];

  return (
    <div className="flex h-full min-h-0 flex-col">
      {renderedLabel === null ? null : (
        <div
          className="flex shrink-0 gap-1 border-b px-2 py-1"
          role="tablist"
          aria-label={fileName}
        >
          <button
            type="button"
            role="tab"
            aria-selected={chosen === "rendered"}
            className="rounded px-2 py-0.5 text-xs aria-selected:bg-muted"
            onClick={() => choose("rendered")}
          >
            {renderedLabel}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={chosen === "raw"}
            className="rounded px-2 py-0.5 text-xs aria-selected:bg-muted"
            onClick={() => choose("raw")}
          >
            {RAW_LABEL}
          </button>
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
            sizeLabel={sizeLabel}
            readOnly={readOnly}
            onChange={onChange}
            onSave={onSave}
            writingTools={writingTools}
          />
        ) : rendered === "markdown" ? (
          <MarkdownPane text={content} options={preview} onOutcome={onOutcome} />
        ) : rendered === "table" && csv != null ? (
          <CsvPane coordinates={csv} options={csvOptions} onExternalWrite={onExternalWrite} />
        ) : structure !== null ? (
          <StructurePane structure={structure} />
        ) : null}
      </div>
    </div>
  );
}

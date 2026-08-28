/**
 * The one viewer registry: kind and format to viewer, for every surface
 * (Story 45.2, FR-174, AD-87, AD-91).
 *
 * A `.csv` opened from the Files pane, embedded in a note, and attached to a
 * quick capture is the same renderer over the same bytes. The alternative
 * writes three CSV widgets that disagree about a ragged row — and 44.16 put
 * that decision in `keeper-core::notes::csv` precisely so it could be answered
 * once. This module is the same move for the layer above: **a surface asks the
 * registry; a surface never switches on an extension.**
 *
 * ## What this keys on, and why it is not a second classifier
 *
 * It keys on BOTH, with the kind strictly dominant, in three cases:
 *
 * 1. `folder` — the folder row. The extension is never consulted, because a
 *    directory is known from the dirent that listed it and no extension table
 *    can tell `2026.08` from `notes.zip`.
 * 2. `video`, `image`, `audio` — that row, and **the extension is never
 *    consulted**. Rust decided; TypeScript does not second-guess it. A `.heic`
 *    added to `IMAGE_EXTENSIONS` in `keeper-core` renders as an image here the
 *    day it lands, with no change in this file.
 * 3. `file` — and ONLY here, the lowercased last extension refines the answer
 *    against {@link FILE_FORMATS}. A miss, a name with no extension, an empty
 *    name: {@link UNKNOWN_ENTRY}.
 *
 * **Why case 3 is not the second classifier this story exists to prevent.**
 * `file` is 43.5's *declared* catch-all: `kind_for_file_name` documents it as
 * "every extension not named above", the bucket that means *keeper has no
 * element for this*. Refining inside that bucket answers a question 43.5
 * deliberately did not ask — a `.csv` and a `.md` are both `file` there and
 * always will be, because neither is a `<video>`, an `<img>` or an `<audio>`.
 * It therefore cannot contradict the classifier: the kind decides first and
 * this table never gets a chance to disagree with it.
 *
 * That is an argument, and an argument is not a guarantee, so
 * `classifier-agreement.test.ts` parses `VIDEO_EXTENSIONS`,
 * `IMAGE_EXTENSIONS` and `AUDIO_EXTENSIONS` straight out of
 * `recordings_fts.rs` and asserts {@link FILE_FORMATS} shares no extension
 * with any of them. If a later story widens the Rust tables over an extension
 * named here, that test fails and names it — instead of this file quietly
 * keeping a row that can never be reached.
 *
 * ## Adding a format is a row
 *
 * One entry in {@link FILE_FORMAT_ROWS}: the extensions, the viewer that shows
 * it, an icon, whether it has a rendered half, its editor syntax, and whether
 * its bytes may be written. No surface changes. That is the shape AD-87 asks
 * for, and the reason `viewer` is coarse (see {@link ViewerId}).
 *
 * This module imports nothing but its own types: no React, no IPC, no store.
 * It is a pure function over a frozen table, which is what lets 45.5 call it
 * once per row of a virtualised tree.
 */

import type {
  IconName,
  LanguageId,
  RenderedView,
  ViewerEntry,
  ViewerFormat,
  ViewerId,
  ViewerSubject,
} from "./types";

/** Freeze a row so no caller can mutate the shared table in place — the rows
 *  are singletons handed to every surface, and one surface editing a label
 *  would edit it everywhere. */
function row(
  entry: Omit<ViewerEntry, "rendered" | "language"> &
    Partial<Pick<ViewerEntry, "rendered" | "language">>,
): ViewerEntry {
  return Object.freeze({ rendered: null, language: null, ...entry });
}

/**
 * The row for a format keeper cannot show — a first-class answer, not a
 * failure (AD-91).
 *
 * Exported because a surface may want to ask whether it got the fallback
 * without comparing strings, and because the unknown viewer's own test asserts
 * the identity rather than the shape.
 */
export const UNKNOWN_ENTRY: ViewerEntry = row({
  viewer: "unknown",
  format: "unknown",
  label: "Unknown file",
  icon: "file-question",
  writable: false,
});

/**
 * The rows the KIND alone decides, by the kind's own name.
 *
 * A total `Record` over the wire type, so a sixth kind added to
 * `RecordingNoteTargetKind` in Rust fails this file to compile rather than
 * silently resolving to nothing — the same guard `files-pane.tsx`'s
 * `KIND_ICON` already uses, for the same reason.
 *
 * Media is `writable: false`: the epic edits text-shaped formats, and a viewer
 * that offered to save a re-encoded `.mov` would be lossy in a way nobody
 * asked for.
 */
const KIND_ENTRIES: Readonly<Record<Exclude<ViewerSubject["kind"], "file">, ViewerEntry>> =
  Object.freeze({
    video: row({
      viewer: "video",
      format: "video",
      label: "Video",
      icon: "file-video",
      writable: false,
    }),
    image: row({
      viewer: "image",
      format: "image",
      label: "Image",
      icon: "file-image",
      writable: false,
    }),
    audio: row({
      viewer: "audio",
      format: "audio",
      label: "Audio",
      icon: "file-audio",
      writable: false,
    }),
    folder: row({
      viewer: "folder",
      format: "folder",
      label: "Folder",
      icon: "folder",
      writable: false,
    }),
  });

/** One row of {@link FILE_FORMATS}: the extensions it claims, and the entry
 *  every one of them resolves to. */
interface FileFormatRow {
  /** Lowercase, no leading dot. Asserted by the registry's own test, because a
   *  `".md"` key here would be a row that can never be reached. */
  readonly extensions: readonly string[];
  readonly entry: ViewerEntry;
}

/** A text-shaped row: the raw half is always a text editor over the real bytes
 *  (AD-88), so `language` is never null and `writable` is always true. */
function textRow(
  extensions: readonly string[],
  entry: {
    format: ViewerFormat;
    label: string;
    icon: IconName;
    language: LanguageId;
    rendered?: RenderedView;
  },
): FileFormatRow {
  return {
    extensions,
    entry: row({
      viewer: "text",
      format: entry.format,
      label: entry.label,
      icon: entry.icon,
      rendered: entry.rendered ?? null,
      language: entry.language,
      writable: true,
    }),
  };
}

/** A document row: rendered by its own viewer, and its bytes are read-only —
 *  a lossy round trip through a document container is how people lose work. */
function documentRow(
  extensions: readonly string[],
  format: ViewerFormat,
  label: string,
  /** The glyph, when the format has one of its own. Defaults to the generic
   *  typed-document page, which is what a format keeps until somebody decides
   *  it is worth telling apart in a list. */
  icon: IconName = "file-document",
): FileFormatRow {
  return {
    extensions,
    entry: row({
      viewer: "document",
      format,
      label,
      icon,
      writable: false,
    }),
  };
}

/** A source row: raw only, one language, one label. */
function sourceRow(extensions: readonly string[], language: LanguageId, label: string) {
  return textRow(extensions, { format: "source", label, icon: "file-code", language });
}

/**
 * Every format keeper knows, INSIDE the kind `file` (see the module header for
 * why that qualifier is the whole design).
 *
 * The formats this epic names — Markdown, CSV, JSON, JSONL (45.4, 45.12);
 * text, config and source (45.6); PDF, DOCX, PPTX, XLSX (45.8) — plus the
 * plain-text neighbours a person would be baffled to find unopenable beside
 * them.
 */
const FILE_FORMAT_ROWS: readonly FileFormatRow[] = [
  textRow(["md", "markdown", "mdown", "mkd"], {
    format: "markdown",
    label: "Markdown",
    icon: "file-text",
    rendered: "markdown",
    language: "markdown",
  }),
  textRow(["csv"], {
    format: "csv",
    label: "CSV",
    icon: "file-table",
    rendered: "table",
    language: "csv",
  }),
  textRow(["json"], {
    format: "json",
    label: "JSON",
    icon: "file-json",
    rendered: "structure",
    language: "json",
  }),
  // JSONL and NDJSON are one format under two spellings — the same
  // one-object-per-line file, named differently by different tools. Two rows
  // would be two labels for one thing and an invitation to render them
  // differently.
  textRow(["jsonl", "ndjson"], {
    format: "jsonl",
    label: "JSON Lines",
    icon: "file-json",
    rendered: "structure",
    language: "json",
  }),
  textRow(["txt", "text", "log"], {
    format: "plain",
    label: "Plain text",
    icon: "file-text",
    language: "plain",
  }),
  sourceRow(["toml"], "toml", "TOML"),
  sourceRow(["yaml", "yml"], "yaml", "YAML"),
  sourceRow(["ini", "cfg", "conf"], "ini", "Config"),
  sourceRow(["rs"], "rust", "Rust source"),
  sourceRow(["ts", "tsx", "mts", "cts"], "typescript", "TypeScript source"),
  sourceRow(["js", "jsx", "mjs", "cjs"], "javascript", "JavaScript source"),
  sourceRow(["py"], "python", "Python source"),
  sourceRow(["go"], "go", "Go source"),
  sourceRow(["sh", "bash", "zsh", "fish"], "shell", "Shell script"),
  sourceRow(["sql"], "sql", "SQL"),
  // Not a `sourceRow` since Story 55.5: HTML is the one source format with a
  // reading of its own — the page it describes — and until then keeper could
  // show a `.html` only as angle brackets. The raw half is unchanged and is
  // still the only half that can change anything (AD-88).
  textRow(["html", "htm"], {
    format: "html",
    label: "HTML",
    icon: "file-code",
    rendered: "html",
    language: "html",
  }),
  sourceRow(["css", "scss", "less"], "css", "Stylesheet"),
  sourceRow(["xml", "plist"], "xml", "XML"),
  sourceRow(["java"], "java", "Java source"),
  sourceRow(["c", "h"], "c", "C source"),
  sourceRow(["cpp", "cc", "hpp", "hh"], "cpp", "C++ source"),
  sourceRow(["rb"], "ruby", "Ruby source"),
  sourceRow(["php"], "php", "PHP source"),
  sourceRow(["lua"], "lua", "Lua source"),
  sourceRow(["swift"], "swift", "Swift source"),
  sourceRow(["kt", "kts"], "kotlin", "Kotlin source"),
  // Four formats that all drew the same page until now, which is how a folder
  // of LOIs, decks and CVs came to look like one repeated file. `documentRow`
  // still names the default; these three say what they are instead.
  documentRow(["pdf"], "pdf", "PDF", "file-pdf"),
  documentRow(["docx"], "docx", "Word document"),
  documentRow(["pptx"], "pptx", "Presentation", "file-slides"),
  // The same glyph as CSV, deliberately. A spreadsheet and a comma-separated
  // file are both a table, and giving this one a chart would say something
  // about the contents that opening it might not bear out.
  documentRow(["xlsx"], "xlsx", "Spreadsheet", "file-table"),
];

/**
 * Extension to row, for the kind `file` only.
 *
 * **A `Map`, not an object literal, and that is not a style choice.**
 * `formats["constructor"]` on a plain object returns `Object`'s constructor —
 * so a file honestly named `payload.constructor` would resolve to a function
 * instead of a row, and "resolution is total" would be false for a name a user
 * can create with `touch`. A `Map` has no prototype chain to fall through.
 */
export const FILE_FORMATS: ReadonlyMap<string, ViewerEntry> = new Map(
  FILE_FORMAT_ROWS.flatMap((format) =>
    format.extensions.map((extension) => [extension, format.entry] as const),
  ),
);

/** Every row of {@link FILE_FORMATS} once, in table order — what a test walks,
 *  and what a surface listing the formats keeper opens would render. */
export const FILE_FORMAT_ENTRIES: readonly ViewerEntry[] = Object.freeze(
  FILE_FORMAT_ROWS.map((format) => format.entry),
);

/**
 * The lowercased last extension of a file name, or `null` when it has none.
 *
 * **`Path::extension`'s rule, deliberately, to the case.** The same name must
 * mean the same thing on both sides of IPC, so: only the LAST path component
 * decides (`2026/a.mov/notes.txt` is a `.txt`); the LAST extension decides
 * (`clip.mov.bak` is a `.bak`, which is why it is not a video in Rust either);
 * a name that begins with a dot and has no other dot has no extension
 * (`.gitignore` is a file called `.gitignore`, not a `gitignore` file); and a
 * name with no dot at all has none.
 *
 * One deliberate divergence, and it changes no outcome: Rust's
 * `Path::new("a.").extension()` is `Some("")`, and this returns `null`. Both
 * miss every table — Rust's `listed("")` is false and this map has no empty
 * key — so the resolved row is identical. `null` is returned because the
 * unknown viewer RENDERS this, and "the extension is `.`" is not a sentence
 * anyone should read.
 *
 * Lowercased because a file copied in from another machine may be `.MD`, and a
 * format that changes with the spelling of its extension reads as a bug — the
 * same reason `kind_for_file_name` compares case-insensitively.
 */
export function extensionOf(name: string): string | null {
  const lastSeparator = name.lastIndexOf("/");
  const fileName = lastSeparator === -1 ? name : name.slice(lastSeparator + 1);
  if (fileName === "" || fileName === "." || fileName === "..") {
    return null;
  }
  const lastDot = fileName.lastIndexOf(".");
  // `<= 0` is both cases at once: -1 is "no dot", and 0 is a leading dot with
  // nothing before it, which `Path::extension` also calls no extension.
  if (lastDot <= 0 || lastDot === fileName.length - 1) {
    return null;
  }
  return fileName.slice(lastDot + 1).toLowerCase();
}

/**
 * Which row shows this file. **Total**: every input resolves to a row, no
 * input yields `undefined`, and nothing here throws.
 *
 * Pure and table-driven, so two surfaces asking about the same file get the
 * very same frozen object — which is what makes "it opens in Files but not in
 * a note" impossible rather than merely unlikely.
 *
 * See the module header for the resolution order and why the extension is
 * consulted only inside kind `file`.
 */
export function resolveViewer(file: ViewerSubject): ViewerEntry {
  if (file.kind !== "file") {
    // Indexing a `Record` with a value from outside its key union is
    // `undefined` at runtime however well typed it is, and a build whose
    // bindings are older than the Rust enum is exactly when that happens. The
    // fallback keeps this total in the one case the type system cannot see.
    return KIND_ENTRIES[file.kind] ?? UNKNOWN_ENTRY;
  }
  const extension = extensionOf(file.name);
  if (extension === null) {
    return UNKNOWN_ENTRY;
  }
  return FILE_FORMATS.get(extension) ?? UNKNOWN_ENTRY;
}

/** Every viewer id the table can produce — what a bindings table must cover,
 *  asserted rather than kept in step by hand. */
export function registeredViewerIds(): ReadonlySet<ViewerId> {
  const ids = new Set<ViewerId>([UNKNOWN_ENTRY.viewer]);
  for (const entry of Object.values(KIND_ENTRIES)) {
    ids.add(entry.viewer);
  }
  for (const entry of FILE_FORMAT_ENTRIES) {
    ids.add(entry.viewer);
  }
  return ids;
}

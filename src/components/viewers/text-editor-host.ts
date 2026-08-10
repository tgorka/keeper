/**
 * The one CodeMirror configuration a file viewer gets (Story 45.6, FR-179).
 *
 * # Why this is a module and not a component
 *
 * `note-editor.tsx` already configures an `EditorView`, and the epic's own
 * wording for this story is that "a second editor configuration is how two
 * surfaces end up with different tab behaviour". Story 43.1 fixed Tab once, in
 * `editor/indent-keymap.ts`, and it should not need fixing twice — so this
 * module imports that exact binding set rather than restating it, and the same
 * is true of history, the default keymap and completion acceptance. What the
 * note editor adds on top (live preview, wikilinks, the slash menu, the notes
 * store) is markdown-and-a-note specific and deliberately stays there; what is
 * common is here.
 *
 * # Everything CodeMirror is behind a dynamic import, and that is load-bearing
 *
 * The editor packages are several hundred kilobytes. `note-editor.tsx` keeps
 * them behind one `import()` for NFR-27 — quick capture has 300 ms and imports
 * none of this — and a module that pulled them in statically would defeat that
 * from a second direction, because 45.4's raw/rendered chrome and 45.5's row
 * icons both want to ask [`isOversizeForEditing`] without mounting an editor.
 * So the only `@codemirror/*` values here are inside `mountTextEditor`, and
 * every top-level import is `import type`, which the compiler erases.
 *
 * # Two tables that cannot contradict each other
 *
 * `src/lib/viewers` (Story 45.2) maps an extension to a language **id**. This
 * module maps an id to a **grammar**. Neither table mentions the other's
 * vocabulary, so there is no way for them to disagree about what a `.rs` is —
 * which is the whole point of 45.2's guard test, applied one layer further out.
 * An id this module does not know opens as plain text and says so at INFO; a
 * viewer row pointing at a grammar nobody wired is then a visible line rather
 * than a silently monochrome file (DW-172's lesson).
 *
 * # Legacy modes are stream modes, not grammars
 *
 * TOML, YAML, Rust, shell and the rest come from `@codemirror/legacy-modes`,
 * which is a bag of CodeMirror **5** stream tokenisers wrapped in
 * `StreamLanguage`. They colour text; they do not build a syntax tree. Nothing
 * structural can be built on them — no "select this TOML table", no
 * format-on-save, no `syntaxTree()` queries — and a story that wants any of
 * that for one of these formats needs a real Lezer grammar for it first. The
 * four that DO have real grammars (markdown, JavaScript/TypeScript, CSS, HTML)
 * are already in the dependency tree via `@codemirror/lang-markdown`, so they
 * cost nothing and are used in preference wherever they apply.
 */
import type { StreamParser } from "@codemirror/language";
import type * as CodeMirrorState from "@codemirror/state";
import type { Extension } from "@codemirror/state";
import type * as CodeMirrorView from "@codemirror/view";
import type { EditorView } from "@codemirror/view";

/**
 * The largest file this surface will edit, in bytes.
 *
 * A mirror of `keeper_core::text_file::TEXT_EDIT_MAX_BYTES`, and the mirror is
 * checked: `text-editor-host.test.ts` parses the constant out of the Rust
 * source and fails if the two drift. Rust owns the number — it is the one that
 * decides what to read and what to send — but a surface holding only a buffer
 * (45.4's raw view over an embedded CSV, say) has no VM to read `oversize` off
 * and still must not offer a save it will refuse.
 *
 * Decimal, so it renders as `1.0 MB` through `keeper_core::size` and the limit
 * a person is told matches the size they are shown.
 */
export const TEXT_EDIT_MAX_BYTES = 1_000_000;

/**
 * Whether a buffer is past the editing limit, measured the way Rust measures it.
 *
 * UTF-8 bytes, not UTF-16 code units: `content.length` would call a 600 kB file
 * of Japanese oversize and a 1.4 MB file of ASCII fine, and the two answers
 * would disagree with the banner Rust composed for the same file.
 */
export function isOversizeForEditing(content: string): boolean {
  return new TextEncoder().encode(content).byteLength > TEXT_EDIT_MAX_BYTES;
}

/**
 * Language ids that are text but deliberately have no grammar.
 *
 * These must not be logged as unwired. `plain` is the registry's word for "a
 * text file with no particular syntax" and `csv` has its structure rendered by
 * 45.4's table rather than coloured by a tokeniser — a comma highlighter would
 * add nothing and would fight the table view for the meaning of the file.
 * Distinguishing them from an id nobody wired is the difference between a log
 * line that means something and one people learn to ignore.
 */
export const PLAIN_LANGUAGE_IDS: readonly string[] = ["plain", "csv"];

/**
 * The language ids this host can colour.
 *
 * Published here rather than in `src/lib/viewers` because this module is what
 * has to be able to load one: an id in the registry's table that is not in this
 * list (and not in [`PLAIN_LANGUAGE_IDS`]) opens as plain text and says so at
 * INFO. The registry is free to have fewer; when it has more, the console names
 * which — `php` is one today, because `@codemirror/legacy-modes` has no PHP
 * tokeniser and `@codemirror/lang-php` would be a second dependency for one row.
 *
 * `null` — no id at all — is also a first-class answer and also means plain
 * text, still fully editable.
 */
export const TEXT_LANGUAGE_IDS = [
  "markdown",
  "javascript",
  "typescript",
  "jsx",
  "tsx",
  "json",
  "css",
  "html",
  "xml",
  "rust",
  "toml",
  "yaml",
  "ini",
  "properties",
  "python",
  "shell",
  "sql",
  "go",
  "c",
  "cpp",
  "java",
  "csharp",
  "kotlin",
  "swift",
  "ruby",
  "lua",
  "perl",
  "powershell",
  "haskell",
  "dockerfile",
  "diff",
] as const;

export type TextLanguageId = (typeof TEXT_LANGUAGE_IDS)[number];

/**
 * Load the grammar for one language id.
 *
 * Split from the mount so the failure is isolated and testable: a chunk that
 * will not fetch — offline, a stale cache, a bad deploy — must leave a plain,
 * editable, saveable file behind, never a broken pane. The caller logs and
 * carries on.
 *
 * Every arm is its own `import()` so the bundler emits one chunk per grammar: a
 * user who never opens a `.toml` never downloads the TOML tokeniser.
 */
async function grammarFor(id: string): Promise<Extension | null> {
  // Dynamic on purpose, and the one exception the no-dynamic-import rule names:
  // this whole module exists to keep several hundred kilobytes of editor out of
  // the main bundle, exactly as `note-editor.tsx` does for NFR-27. A static
  // import here would put every grammar in every chunk that can see a file row.
  const { StreamLanguage } = await import("@codemirror/language");
  /**
   * A CodeMirror 5 stream mode, wrapped as an extension.
   *
   * The cast is unavoidable and narrow: each mode is declared as
   * `StreamParser<ItsOwnPrivateState>`, and those state types are neither
   * exported nor mutually assignable, so no signature can name them all.
   * `StreamLanguage.define` treats the state as opaque, which is why this is
   * safe rather than merely convenient.
   */
  const stream = async (
    load: () => Promise<Record<string, unknown>>,
    key: string,
  ): Promise<Extension> => StreamLanguage.define((await load())[key] as StreamParser<unknown>);

  switch (id) {
    // The four real Lezer grammars, already in the tree through
    // `@codemirror/lang-markdown` -> `lang-html` -> `lang-javascript` +
    // `lang-css`. Preferred over the legacy modes of the same names because
    // they produce a syntax tree, which is what `format-commands.ts` and the
    // live-preview layer read.
    case "markdown": {
      const md = await import("@codemirror/lang-markdown");
      return md.markdown({ base: md.markdownLanguage });
    }
    case "javascript":
      return (await import("@codemirror/lang-javascript")).javascript();
    case "jsx":
      return (await import("@codemirror/lang-javascript")).javascript({ jsx: true });
    case "typescript":
      return (await import("@codemirror/lang-javascript")).javascript({ typescript: true });
    case "tsx":
      return (await import("@codemirror/lang-javascript")).javascript({
        jsx: true,
        typescript: true,
      });
    case "css":
      return (await import("@codemirror/lang-css")).css();
    case "html":
      return (await import("@codemirror/lang-html")).html();

    // JSON through the legacy JavaScript tokeniser's `json` variant rather than
    // a second package for one format: `@codemirror/lang-json` would be a whole
    // dependency to colour braces this already colours.
    case "json":
      return stream(() => import("@codemirror/legacy-modes/mode/javascript"), "json");

    case "xml":
      return stream(() => import("@codemirror/legacy-modes/mode/xml"), "xml");
    case "rust":
      return stream(() => import("@codemirror/legacy-modes/mode/rust"), "rust");
    case "toml":
      return stream(() => import("@codemirror/legacy-modes/mode/toml"), "toml");
    case "yaml":
      return stream(() => import("@codemirror/legacy-modes/mode/yaml"), "yaml");
    case "python":
      return stream(() => import("@codemirror/legacy-modes/mode/python"), "python");
    case "shell":
      return stream(() => import("@codemirror/legacy-modes/mode/shell"), "shell");
    // `.ini` and `.properties` are the same key/value/section shape and the
    // registry spells the id `ini`; both land on the one tokeniser rather than
    // one of them quietly having no colour.
    case "ini":
    case "properties":
      return stream(() => import("@codemirror/legacy-modes/mode/properties"), "properties");
    case "sql":
      return stream(() => import("@codemirror/legacy-modes/mode/sql"), "standardSQL");
    case "go":
      return stream(() => import("@codemirror/legacy-modes/mode/go"), "go");
    case "c":
      return stream(() => import("@codemirror/legacy-modes/mode/clike"), "c");
    case "cpp":
      return stream(() => import("@codemirror/legacy-modes/mode/clike"), "cpp");
    case "java":
      return stream(() => import("@codemirror/legacy-modes/mode/clike"), "java");
    case "csharp":
      return stream(() => import("@codemirror/legacy-modes/mode/clike"), "csharp");
    case "kotlin":
      return stream(() => import("@codemirror/legacy-modes/mode/clike"), "kotlin");
    case "swift":
      return stream(() => import("@codemirror/legacy-modes/mode/swift"), "swift");
    case "ruby":
      return stream(() => import("@codemirror/legacy-modes/mode/ruby"), "ruby");
    case "lua":
      return stream(() => import("@codemirror/legacy-modes/mode/lua"), "lua");
    case "perl":
      return stream(() => import("@codemirror/legacy-modes/mode/perl"), "perl");
    case "powershell":
      return stream(() => import("@codemirror/legacy-modes/mode/powershell"), "powerShell");
    case "haskell":
      return stream(() => import("@codemirror/legacy-modes/mode/haskell"), "haskell");
    case "dockerfile":
      return stream(() => import("@codemirror/legacy-modes/mode/dockerfile"), "dockerFile");
    case "diff":
      return stream(() => import("@codemirror/legacy-modes/mode/diff"), "diff");

    default:
      return null;
  }
}

/** What the React surface needs from a mounted editor, and nothing more. */
export interface TextEditorMount {
  /**
   * Adopt text that came from outside this buffer.
   *
   * A no-op when the document already reads that way, which is what makes the
   * controlled-prop pattern safe: React re-renders with the same string on
   * every keystroke (the surface reports the edit, the parent stores it, the
   * prop comes back), and dispatching that identical string would reset the
   * selection on every character typed.
   */
  setContent: (next: string) => void;
  /** Turn editing on or off without rebuilding the view or losing the caret. */
  setReadOnly: (readOnly: boolean) => void;
  destroy: () => void;
  /** The live view. Exposed so a test can drive the real thing. */
  view: EditorView;
}

export interface TextEditorMountOptions {
  parent: HTMLElement;
  content: string;
  /** The registry's language id, or `null` for plain text. */
  language: string | null;
  readOnly: boolean;
  /** Every document change, as the exact buffer. Never called when read-only. */
  onChange: (next: string) => void;
  /** `Mod-s`. The surface decides what saving means; this only asks. */
  onSave: () => void;
  /**
   * Called once the grammar has landed, or has failed to. Exists so a test can
   * wait for the asynchronous half instead of sleeping, and so the surface can
   * drop a stale load when the file changes under it.
   */
  onLanguageSettled?: (id: string | null, loaded: boolean) => void;
}

/**
 * Both halves of "this file cannot be changed here", because one is not enough.
 *
 * `EditorView.editable` takes `contenteditable` off the content DOM, which
 * stops typing. It does **not** stop `Backspace`, `Enter`, cut or paste: those
 * arrive as keymap commands and DOM event handlers, and CodeMirror gates them
 * on `EditorState.readOnly` instead. A view with only the first would let a
 * person paste four kilobytes into a file the surface just told them was
 * read-only — and, for an oversize file, that pasted buffer is a truncated
 * prefix a save would write over the whole file.
 *
 * Set together, in one compartment, so no later edit can reconfigure one and
 * leave the other behind.
 */
function readOnlyExtensions(
  view: typeof CodeMirrorView,
  state: typeof CodeMirrorState,
  readOnly: boolean,
): Extension {
  return [view.EditorView.editable.of(!readOnly), state.EditorState.readOnly.of(readOnly)];
}

/**
 * Build and mount the editor.
 *
 * The view is created **synchronously with respect to the grammar**: plain text
 * first, colour a moment later through a `Compartment`. That ordering is not an
 * optimisation, it is the fallback path — if the grammar chunk never arrives,
 * what is on screen is already a working, editable, saveable editor rather than
 * a spinner waiting on a fetch that will not complete.
 */
export async function mountTextEditor(options: TextEditorMountOptions): Promise<TextEditorMount> {
  // Dynamic, and the exception is the same one `note-editor.tsx` takes: the
  // `@codemirror/*` packages are several hundred kilobytes that a user who never
  // opens a file should not download, and quick capture (NFR-27, 300 ms) must
  // not pay for them at all. A static import here would pull the whole editor
  // into any chunk that can render a file row.
  const [state, view, commands, language, indent] = await Promise.all([
    import("@codemirror/state"),
    import("@codemirror/view"),
    import("@codemirror/commands"),
    import("@codemirror/language"),
    import("../notes/editor/indent-keymap"),
  ]);

  const grammar = new state.Compartment();
  const editable = new state.Compartment();
  // One flag rather than reading the compartment back: the update listener runs
  // for programmatic dispatches too, and `setContent` must not be reported as
  // the user's edit.
  let adopting = false;

  const editorView = new view.EditorView({
    parent: options.parent,
    state: state.EditorState.create({
      doc: options.content,
      extensions: [
        view.EditorView.lineWrapping,
        // A gutter, unlike the note editor. A note is prose and a line number
        // beside a paragraph is noise; a config file is something people are
        // told about by line ("the error is on line 47"), and 45.4's parse-error
        // banner points at one.
        view.lineNumbers(),
        view.highlightActiveLine(),
        commands.history(),
        language.indentOnInput(),
        language.bracketMatching(),
        // `fallback: true` so plain text and stream modes still get the theme's
        // colours rather than nothing at all.
        language.syntaxHighlighting(language.defaultHighlightStyle, { fallback: true }),
        view.keymap.of([
          ...commands.defaultKeymap,
          ...commands.historyKeymap,
          // Story 43.1's binding, imported and not restated. Tab is claimed
          // here for exactly the reason it is claimed in a note: an unclaimed
          // Tab escapes to the web view, which edits the DOM under CodeMirror.
          ...indent.indentBindings,
          {
            key: "Mod-s",
            preventDefault: true,
            run: () => {
              options.onSave();
              return true;
            },
          },
        ]),
        grammar.of([]),
        editable.of(readOnlyExtensions(view, state, options.readOnly)),
        view.EditorView.updateListener.of((update) => {
          if (!update.docChanged || adopting) {
            return;
          }
          options.onChange(update.state.doc.toString());
        }),
        // Line endings are the file's, not the editor's. Without this facet
        // CodeMirror splits on /\r\n?|\n/ and hands back "\n" for every line,
        // so opening a CRLF file and saving it untouched would rewrite every
        // line in it — a whole-file diff for a change nobody made, carried
        // straight into git by sync. Splitting only on "\n" leaves the "\r" in
        // the line's own text, so the document round-trips byte for byte.
        // The cost, stated plainly: a newline typed into a CRLF file is an LF,
        // so an edited file gains mixed endings. That is a change confined to
        // the lines actually touched, which is the smaller wrong.
        state.EditorState.lineSeparator.of("\n"),
      ],
    }),
  });

  // Fired and forgotten. The editor is already usable; this only adds colour.
  if (options.language !== null && !PLAIN_LANGUAGE_IDS.includes(options.language)) {
    const id = options.language;
    void grammarFor(id)
      .then((extension) => {
        if (extension === null) {
          // DW-172's lesson before the bug exists: a registry row pointing at a
          // grammar nobody wired is a visible line, not a monochrome mystery.
          console.info(
            `keeper: no CodeMirror grammar is wired for language id "${id}"; opening as plain text.`,
          );
          options.onLanguageSettled?.(id, false);
          return;
        }
        editorView.dispatch({ effects: grammar.reconfigure(extension) });
        options.onLanguageSettled?.(id, true);
      })
      .catch((error: unknown) => {
        // A chunk that will not fetch. The file stays open, editable and
        // saveable; only the colour is missing, and the reason is on the record
        // at INFO because `tracing::debug!`-grade logging never reaches a
        // packaged app (DW-162, applied to the browser console).
        console.info(
          `keeper: could not load the "${id}" syntax highlighting; opening as plain text.`,
          error,
        );
        options.onLanguageSettled?.(id, false);
      });
  } else {
    // `plain`, `csv` and no id at all: nothing to load and nothing to say.
    options.onLanguageSettled?.(options.language, false);
  }

  return {
    setContent: (next: string) => {
      if (editorView.state.doc.toString() === next) {
        return;
      }
      adopting = true;
      try {
        editorView.dispatch({
          changes: { from: 0, to: editorView.state.doc.length, insert: next },
        });
      } finally {
        adopting = false;
      }
    },
    setReadOnly: (readOnly: boolean) => {
      editorView.dispatch({
        effects: editable.reconfigure(readOnlyExtensions(view, state, readOnly)),
      });
    },
    destroy: () => editorView.destroy(),
    view: editorView,
  };
}

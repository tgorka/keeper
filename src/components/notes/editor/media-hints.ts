/**
 * Hints while a ` ```keeper-media ` block's source is open: the keys that may
 * stand where the caret is, each with a line saying what it does, the values a
 * key takes after its `=`, and Rust's refusal underlined on the key or line it
 * is about, its sentence on hover.
 *
 * **The grammar is Rust's.** Which keys exist, where each may stand, what its
 * value is and what it does come from `media_block_schema`; whether a body
 * reads, and where it does not, from `media_block_check`. What this module
 * reads of the body is only what a position needs: the table the caret's line
 * is under, and the keys already written in it, so a key is not offered twice
 * and a second source is not offered at all.
 *
 * **Values come from where keeper already keeps them.** A `session` is picked
 * from the recordings index, searched as the media picker searches it; a file
 * from the note's drive, listed folder by folder as the picker lists it, only
 * the files the key takes; a time is the block's player time, when the player
 * was somewhere before its source was opened, and the start.
 *
 * **Only inside a fence.** Outside a `keeper-media` body the source answers
 * nothing, so the note's other completion is untouched.
 */
import {
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
  startCompletion,
} from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import {
  type EditorState,
  type Extension,
  RangeSetBuilder,
  StateEffect,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  hoverTooltip,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";
import type {
  FilesEntryVm,
  FilesListingVm,
  MediaBlockProblemVm,
  MediaBlockSchemaVm,
  MediaKeyPlace,
  MediaKeyVm,
  RecordingHitVm,
} from "@/lib/ipc/client";
import { type MediaFence, mediaFences } from "./media-block";

/** What the hints ask of keeper. */
export interface MediaHintSources {
  /** The grammar; asked once per editor. */
  schema: () => Promise<MediaBlockSchemaVm>;
  /** Recordings matching `query`, newest first. */
  recordings: (query: string) => Promise<readonly RecordingHitVm[]>;
  /** The note's drive's folder `subpath` (`""` for its root), or null when the note has none. */
  files: (subpath: string) => Promise<FilesListingVm | null>;
  /** Where the player of the block at `fenceFrom` was when its source was opened, in seconds. */
  playerTime?: (view: EditorView, fenceFrom: number) => number | null;
}

/** Where the caret stands in a block body. */
export type MediaHintContext =
  | {
      kind: "key";
      fence: MediaFence;
      /** The table the caret's line is under; null for one the grammar does not have. */
      place: MediaKeyPlace | null;
      /** Keys already written in that table, on other lines than the caret's. */
      written: ReadonlySet<string>;
      /** Whether the root names a source by key, and whether the block has `[[part]]` tables. */
      rootSource: boolean;
      hasParts: boolean;
      /** Start of the word being typed, and the word. */
      from: number;
      typed: string;
    }
  | {
      kind: "value";
      fence: MediaFence;
      place: MediaKeyPlace | null;
      key: string;
      /** Start of the value (its opening quote, when typed), and the end of what a pick replaces. */
      from: number;
      to: number;
      /** What is typed of the value, without its quote. */
      typed: string;
    };

const HEADER = /^\s*\[\[?\s*([A-Za-z0-9_-]+)\s*\]\]?\s*(?:#.*)?$/;
const KEY_LINE = /^\s*([A-Za-z0-9_-]+)\s*=/;
const KEY_TYPED = /^(\s*)(\[{0,2}[A-Za-z0-9_-]*)$/;
const VALUE_TYPED = /^\s*([A-Za-z0-9_-]+)\s*=\s*("?)([^"]*)$/;

/**
 * What the caret at `pos` is writing in a block body, or null outside every
 * body. `sourceKeys` are the root keys that name what a block plays.
 */
export function mediaHintContext(
  state: EditorState,
  pos: number,
  sourceKeys: ReadonlySet<string>,
): MediaHintContext | null {
  const fence = mediaFences(state).find((each) => each.bodyFrom <= pos && pos <= each.bodyTo);
  if (fence === undefined) {
    return null;
  }
  const doc = state.doc;
  const line = doc.lineAt(pos);
  const first = doc.lineAt(fence.bodyFrom).number;
  const last = doc.lineAt(fence.bodyTo).number;
  // Each body line's table: its header's name, "" for the root.
  let table = "";
  let caretTable = "";
  const tableOf: string[] = [];
  let hasParts = false;
  for (let number = first; number <= last; number += 1) {
    const header = HEADER.exec(doc.line(number).text);
    if (header !== null) {
      table = header[1];
      hasParts ||= table === "part";
      tableOf.push("[");
    } else {
      tableOf.push(table);
    }
    if (number === line.number) {
      caretTable = table;
    }
  }
  // The caret's table runs from its header to the next one; a key after the
  // caret belongs to it as much as one before.
  let start = line.number;
  while (start > first && tableOf[start - 1 - first] === caretTable) start -= 1;
  let end = line.number;
  while (end < last && tableOf[end + 1 - first] === caretTable) end += 1;
  const written = new Set<string>();
  let rootSource = false;
  for (let number = first; number <= last; number += 1) {
    const key = number === line.number ? undefined : KEY_LINE.exec(doc.line(number).text)?.[1];
    if (key === undefined) continue;
    rootSource ||= tableOf[number - first] === "" && sourceKeys.has(key);
    if (number >= start && number <= end) written.add(key);
  }
  const place: MediaKeyPlace | null =
    caretTable === ""
      ? "root"
      : caretTable === "part" || caretTable === "marker"
        ? caretTable
        : null;
  const before = line.text.slice(0, pos - line.from);
  const value = VALUE_TYPED.exec(before);
  if (value !== null) {
    const quote = value[2];
    const closing =
      quote === "" ? "" : (/^[^"]*"/.exec(line.text.slice(pos - line.from))?.[0] ?? "");
    return {
      kind: "value",
      fence,
      place,
      key: value[1],
      from: pos - value[3].length - quote.length,
      to: pos + closing.length,
      typed: value[3],
    };
  }
  const key = KEY_TYPED.exec(before);
  if (key === null || line.text.slice(pos - line.from).trim() !== "") {
    return null;
  }
  return {
    kind: "key",
    fence,
    place,
    written,
    rootSource,
    hasParts,
    from: line.from + key[1].length,
    typed: key[2],
  };
}

/** Whether a file of the drive is one `kind` takes. */
function takes(kind: MediaKeyVm["value"], entry: FilesEntryVm): boolean {
  switch (kind) {
    case "transcript":
      return entry.name === "transcript.json" || entry.name.endsWith(".transcript.json");
    case "media":
      return entry.kind === "video" || entry.kind === "audio";
    case "video":
      return entry.kind === "video";
    case "config":
      return entry.name.toLowerCase().endsWith(".toml");
    default:
      return false;
  }
}

/** Replace `[from, to)` with `insert`, put the caret `back` characters before its end, and open the next menu. */
function insertAndContinue(insert: string, back: number, reopen: boolean): Completion["apply"] {
  return (view: EditorView, _completion: Completion, from: number, to: number) => {
    view.dispatch({
      changes: { from, to, insert },
      selection: { anchor: from + insert.length - back },
      userEvent: "input.complete",
    });
    if (reopen) {
      startCompletion(view);
    }
  };
}

function keyOptions(
  context: Extract<MediaHintContext, { kind: "key" }>,
  schema: MediaBlockSchemaVm,
): Completion[] {
  const typed = context.typed.replace(/^\[+/, "");
  const header = context.typed.startsWith("[");
  const options: Completion[] = [];
  for (const key of schema.keys) {
    if (!key.key.startsWith(typed)) {
      continue;
    }
    if (key.value === "tables") {
      // A header may follow any table, since it ends the one above it; only
      // a root that already names its source rules `[[part]]` out.
      if (!(key.key === "part" && context.rootSource)) {
        options.push({
          label: `[[${key.key}]]`,
          type: "class",
          info: key.doc,
          apply: `[[${key.key}]]`,
        });
      }
      continue;
    }
    if (header || key.place !== context.place || context.written.has(key.key)) {
      continue;
    }
    if (key.source && (context.rootSource || context.hasParts)) {
      continue;
    }
    // What a person writes first: `key = ""` with the caret between the quotes.
    const quoted = key.value !== "version" && key.value !== "track";
    options.push({
      label: key.key,
      type: "property",
      detail: key.values.length > 0 ? key.values.join(" | ") : key.value,
      info: key.doc,
      apply: insertAndContinue(`${key.key} = ${quoted ? '""' : ""}`, quoted ? 1 : 0, true),
    });
  }
  return options;
}

/**
 * Ask keeper once for the value being typed: `token` names what is asked —
 * the value's start and what it lists — and a keystroke that grows the same
 * token reuses the answer, filtered here, rather than asking again.
 */
type AskOnce = <T>(token: string, question: () => Promise<T>) => Promise<T>;

async function valueOptions(
  view: EditorView | undefined,
  context: Extract<MediaHintContext, { kind: "value" }>,
  key: MediaKeyVm,
  sources: MediaHintSources,
  ask: AskOnce,
): Promise<Completion[]> {
  const typed = context.typed;
  const quote = (text: string) => `"${text}"`;
  switch (key.value) {
    case "choice":
      return key.values
        .filter((value) => value.startsWith(typed))
        .map((value) => ({ label: quote(value), type: "enum", apply: quote(value) }));
    case "version":
      return [{ label: "1", type: "constant", apply: "1" }];
    case "track":
      return ["1", "2"].map((track) => ({ label: track, type: "constant", apply: track }));
    case "time": {
      const times: Completion[] = [];
      const seconds =
        view === undefined ? null : (sources.playerTime?.(view, context.fence.from) ?? null);
      if (seconds !== null) {
        const at = hhmmss(seconds);
        times.push({
          label: quote(at),
          detail: "player time",
          type: "constant",
          apply: quote(at),
          boost: 1,
        });
      }
      times.push({
        label: quote("00:00:00"),
        detail: "the start",
        type: "constant",
        apply: quote("00:00:00"),
      });
      return times.filter((time) => time.label.slice(1).startsWith(typed));
    }
    case "session": {
      const rows = await ask(`session@${context.from}`, () => sources.recordings(typed));
      const needle = typed.toLowerCase();
      return rows
        .filter((row) =>
          [row.title, row.relativePath, row.sessionId].some((text) =>
            text?.toLowerCase().includes(needle),
          ),
        )
        .map((row) => {
          const date =
            row.startedTs === null ? "" : ` · ${new Date(row.startedTs).toLocaleDateString()}`;
          return {
            label: `${row.title ?? row.relativePath}${date}`,
            detail: row.sessionId,
            type: "variable",
            apply: quote(row.sessionId),
          };
        });
    }
    case "transcript":
    case "media":
    case "video":
    case "config": {
      const slash = typed.lastIndexOf("/");
      const folder = slash < 0 ? "" : typed.slice(0, slash);
      const name = typed.slice(slash + 1).toLowerCase();
      const listing = await ask(`files@${context.from}:${folder}`, () => sources.files(folder));
      if (listing === null || listing.state !== "listed") {
        return [];
      }
      return (listing.entries ?? [])
        .filter((entry) => entry.name.toLowerCase().startsWith(name))
        .filter((entry) => entry.kind === "folder" || takes(key.value, entry))
        .map((entry) =>
          entry.kind === "folder"
            ? {
                label: `${entry.name}/`,
                type: "namespace",
                apply: insertAndContinue(`"${entry.relativePath}/`, 0, true),
              }
            : {
                label: entry.name,
                detail: entry.relativePath,
                type: "text",
                apply: quote(entry.relativePath),
              },
        );
    }
    default:
      return [];
  }
}

/** `seconds` as a block writes a time: `hh:mm:ss`, whole seconds. */
function hhmmss(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(whole / 3600))}:${pad(Math.floor(whole / 60) % 60)}:${pad(whole % 60)}`;
}

/**
 * Completion inside a `keeper-media` body: keys at a line's start, values after
 * `=`. One more source for the one `autocompletion()` over markdown.
 */
export function mediaBlockCompleteSource(sources: MediaHintSources): CompletionSource {
  let schema: Promise<MediaBlockSchemaVm> | null = null;
  let asked: { token: string; answer: Promise<unknown> } | null = null;
  const ask: AskOnce = <T>(token: string, question: () => Promise<T>) => {
    if (asked?.token === token) return asked.answer as Promise<T>;
    const answer = question();
    const held = { token, answer };
    asked = held;
    answer.catch(() => {
      if (asked === held) asked = null;
    });
    return answer;
  };
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    // Most keystrokes are outside every fence: the tree answers that without a scan.
    for (
      let node: SyntaxNode | null = syntaxTree(context.state).resolveInner(context.pos, -1);
      ;
      node = node.parent
    ) {
      if (node === null) return null;
      if (node.name === "FencedCode") break;
    }
    if (schema === null) {
      schema = sources.schema();
      schema.catch(() => {
        schema = null;
      });
    }
    const loaded = await schema;
    const sourceKeys = new Set(loaded.keys.filter((key) => key.source).map((key) => key.key));
    const hint = mediaHintContext(context.state, context.pos, sourceKeys);
    if (hint === null || hint.kind === "key") {
      // Out of a value: the next value typed is a new question.
      asked = null;
    }
    if (hint === null) {
      return null;
    }
    if (hint.kind === "key") {
      // An empty line is not a request; a typed letter, or Ctrl-Space, is.
      if (hint.typed === "" && !context.explicit) {
        return null;
      }
      const options = keyOptions(hint, loaded);
      return options.length === 0 ? null : { from: hint.from, options, filter: false };
    }
    const key = loaded.keys.find(
      (each) => each.key === hint.key && each.place === hint.place && each.value !== "tables",
    );
    if (key === undefined) {
      return null;
    }
    const options = await valueOptions(context.view, hint, key, sources, ask);
    if (context.aborted || options.length === 0) {
      return null;
    }
    return { from: hint.from, to: hint.to, options, filter: false };
  };
}

// --- Diagnostics -----------------------------------------------------------

/** A refusal, placed in the document. */
export interface MediaHintProblem {
  from: number;
  to: number;
  message: string;
}

/**
 * Where in the document Rust's `problem` about `fence`'s body lands: its line
 * of the body and its columns there, both clamped to the line; the whole line
 * for an empty range; the opening fence's line for the block as a whole.
 */
export function problemRange(
  state: EditorState,
  fence: MediaFence,
  problem: MediaBlockProblemVm,
): MediaHintProblem {
  const doc = state.doc;
  const opening = doc.lineAt(fence.from);
  const whole = { from: opening.from, to: opening.to, message: problem.message };
  if (problem.line === null) {
    return whole;
  }
  const number = doc.lineAt(fence.bodyFrom).number + problem.line - 1;
  if (fence.bodyFrom === fence.bodyTo || number > doc.lineAt(fence.bodyTo).number) {
    return whole;
  }
  const line = doc.line(number);
  const from = line.from + Math.min(problem.from, line.length);
  const to = line.from + Math.min(problem.to, line.length);
  return from < to
    ? { from, to, message: problem.message }
    : { from: line.from, to: line.to, message: problem.message };
}

const setProblems = StateEffect.define<readonly MediaHintProblem[]>();

const PROBLEM_CLASS = "cm-media-hint-problem";

const problemField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    let next = value.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(setProblems)) {
        const builder = new RangeSetBuilder<Decoration>();
        for (const problem of [...effect.value].sort((a, b) => a.from - b.from)) {
          builder.add(
            problem.from,
            problem.to,
            Decoration.mark({ class: PROBLEM_CLASS, problem: problem.message }),
          );
        }
        next = builder.finish();
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** How long typing must pause before a body is asked about. */
const CHECK_DELAY_MS = 300;

/**
 * Rust's refusal of each `keeper-media` body, underlined where it is about,
 * the sentence on hover. A body is asked about once per spelling.
 */
export function mediaBlockDiagnostics(
  check: (source: string) => Promise<MediaBlockProblemVm | null>,
): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      private readonly answers = new Map<string, MediaBlockProblemVm | null>();
      private timer: number | null = null;
      private generation = 0;

      constructor(private readonly view: EditorView) {
        this.schedule();
      }

      update(update: ViewUpdate): void {
        if (update.docChanged) {
          this.schedule();
        }
      }

      private schedule(): void {
        if (this.timer !== null) {
          window.clearTimeout(this.timer);
        }
        this.timer = window.setTimeout(() => {
          this.timer = null;
          void this.run();
        }, CHECK_DELAY_MS);
      }

      private async run(): Promise<void> {
        const generation = ++this.generation;
        const fences = mediaFences(this.view.state);
        for (const fence of fences) {
          if (!this.answers.has(fence.source)) {
            try {
              this.answers.set(fence.source, await check(fence.source));
            } catch {
              this.answers.set(fence.source, null);
            }
          }
        }
        if (generation !== this.generation) {
          return;
        }
        // Answers are kept only for bodies still in the note.
        const live = new Set(fences.map((fence) => fence.source));
        for (const source of this.answers.keys()) {
          if (!live.has(source)) this.answers.delete(source);
        }
        const state = this.view.state;
        const problems = mediaFences(state).flatMap((fence) => {
          const problem = this.answers.get(fence.source);
          return problem == null ? [] : [problemRange(state, fence, problem)];
        });
        this.view.dispatch({ effects: setProblems.of(problems) });
      }

      destroy(): void {
        this.generation += 1;
        if (this.timer !== null) {
          window.clearTimeout(this.timer);
        }
      }
    },
  );
  const hover = hoverTooltip((view, pos) => {
    let found: { from: number; to: number; message: string } | null = null;
    view.state.field(problemField).between(pos, pos, (from, to, decoration) => {
      found = { from, to, message: String(decoration.spec.problem) };
      return false;
    });
    if (found === null) {
      return null;
    }
    const { from, to, message } = found;
    return {
      pos: from,
      end: to,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.className = "cm-media-hint-tooltip";
        dom.textContent = message;
        return { dom };
      },
    };
  });
  return [
    problemField,
    plugin,
    hover,
    EditorView.baseTheme({
      [`.${PROBLEM_CLASS}`]: {
        textDecoration: "underline wavy var(--destructive)",
        textDecorationSkipInk: "none",
        textUnderlineOffset: "3px",
      },
      ".cm-media-hint-tooltip": {
        padding: "2px 6px",
        maxWidth: "32em",
        color: "var(--destructive)",
      },
    }),
  ];
}

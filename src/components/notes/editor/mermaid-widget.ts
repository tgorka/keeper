/**
 * The ` ```mermaid ` block widget (Story 37.8, FR-111, UX-DR44).
 *
 * Mermaid is the one component in the phase that *interprets* agent-authored
 * input, so it is the one component held at arm's length:
 *
 * - **Lazy.** The library is several hundred kilobytes and most notes contain
 *   no diagram, so it is loaded by `import()` the first time a fence is
 *   actually rendered — never at editor construction, and never at all in the
 *   quick-capture window, which imports none of this (NFR-27).
 * - **Strict.** `securityLevel: "strict"` and `htmlLabels: false`, top level and
 *   per-diagram. A diagram label is text, not a DOM sink; `<script>` in a note
 *   body stays what it always was, characters.
 * - **Bounded.** A render that has not finished inside
 *   {@link MERMAID_RENDER_TIMEOUT_MS} is abandoned. A pathological graph must
 *   cost a fence, not the editor.
 * - **Degrading.** Every failure — a parse error, a timeout, even the import
 *   itself failing — renders the fence's own source text with the reason above
 *   it. Never an empty box. An empty box tells the user their note is gone when
 *   the file on disk is intact, which is the most alarming lie a notes app can
 *   tell (UX-DR44).
 */
import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Extension, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";

/** How long a single diagram may take before it is abandoned. */
export const MERMAID_RENDER_TIMEOUT_MS = 4_000;

/**
 * The slice of mermaid's API this widget uses.
 *
 * Named rather than inferred so the renderer can be driven with a stub — the
 * failure paths are the interesting ones and they must be reachable without
 * loading a rendering engine into a test.
 */
export interface MermaidModule {
  initialize(config: Record<string, unknown>): void;
  render(id: string, source: string): Promise<{ svg: string }>;
}

/** How the renderer gets hold of mermaid. */
export type MermaidLoader = () => Promise<MermaidModule>;

export interface MermaidRenderOptions {
  /** Overridden in tests; production always lazy-imports the real library. */
  load?: MermaidLoader;
  /** Overridden in tests to keep the timeout path fast. */
  timeoutMs?: number;
  /** Whether to render for a dark surface. Defaults to the document's theme. */
  dark?: boolean;
}

/** Distinct ids per render: mermaid injects a `<style>` scoped to the id. */
let renderSeq = 0;

/** The default loader — the one place mermaid enters the bundle graph. */
const loadMermaid: MermaidLoader = async () => (await import("mermaid")).default;

/**
 * Render `source` into `host`, replacing whatever it held.
 *
 * Never rejects: a failure is a rendering outcome here, not an exception for
 * someone else to handle. The degraded node is the whole point of the function.
 */
export async function renderMermaidInto(
  host: HTMLElement,
  source: string,
  options: MermaidRenderOptions = {},
): Promise<void> {
  const load = options.load ?? loadMermaid;
  const timeoutMs = options.timeoutMs ?? MERMAID_RENDER_TIMEOUT_MS;
  const dark = options.dark ?? document.documentElement.classList.contains("dark");

  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const svg = await Promise.race([
      (async () => {
        const mermaid = await load();
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          htmlLabels: false,
          flowchart: { htmlLabels: false },
          theme: dark ? "dark" : "neutral",
        });
        renderSeq += 1;
        const { svg: rendered } = await mermaid.render(`keeper-mermaid-${renderSeq}`, source);
        return rendered;
      })(),
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error("diagram took too long to render")), timeoutMs);
      }),
    ]);
    host.replaceChildren();
    const figure = document.createElement("div");
    figure.className = "cm-mermaid-figure";
    // mermaid returns an SVG document fragment and there is no other way to
    // adopt it. `securityLevel: "strict"` is what makes this safe: mermaid
    // sanitises the diagram's own text before it reaches this string.
    figure.innerHTML = svg;
    host.append(figure);
  } catch (error) {
    host.replaceChildren(degraded(source, error));
  } finally {
    clearTimeout(timer);
  }
}

/** The fence as the user wrote it, with the reason it did not draw. */
function degraded(source: string, error: unknown): HTMLElement {
  const wrapper = document.createElement("div");
  wrapper.className = "cm-mermaid-error";

  const reason = document.createElement("p");
  reason.setAttribute("role", "alert");
  reason.className = "cm-mermaid-error-message";
  reason.textContent = error instanceof Error ? error.message : String(error);

  // `textContent`, never `innerHTML`: this is the path that renders text a
  // failed parser has already refused to understand.
  const code = document.createElement("pre");
  code.className = "cm-mermaid-error-source";
  code.textContent = source;

  wrapper.append(reason, code);
  return wrapper;
}

/**
 * The CodeMirror block widget that replaces a mermaid fence.
 *
 * Only ever constructed from the editor's lazy chunk, which is why importing
 * `@codemirror/view` here costs the capture window nothing.
 */
export class MermaidWidget extends WidgetType {
  constructor(private readonly source: string) {
    super();
  }

  /** Same source, same diagram: CodeMirror may reuse the rendered DOM. */
  eq(other: MermaidWidget): boolean {
    return other.source === this.source;
  }

  toDOM(): HTMLElement {
    const host = document.createElement("div");
    host.className = "cm-mermaid-block";
    // Fired and forgotten: the widget is in the document immediately, and the
    // render — or its degraded fallback — replaces the placeholder when it
    // resolves. Blocking `toDOM` on an async import would stall the editor.
    void renderMermaidInto(host, this.source);
    return host;
  }

  /** Let clicks through: selecting the diagram puts the caret in the fence,
   *  which is how the source is revealed (UX-DR40). */
  ignoreEvent(): boolean {
    return false;
  }
}

/** One mermaid fence: the lines it owns and the diagram between them. */
interface MermaidFence {
  from: number;
  to: number;
  source: string;
}

/**
 * Every mermaid fence in the document, as the line range it occupies.
 *
 * The info string is read from the parse tree rather than from a regex over the
 * line, because ` ~~~mermaid `, an indented fence and a fence opened inside a
 * list item are all mermaid fences and none of them start at column zero with
 * three backticks. `markdown-preview.ts` asks the same question of the same
 * tree for the same reason.
 */
function mermaidFences(state: EditorState): MermaidFence[] {
  const fences: MermaidFence[] = [];
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "FencedCode") {
        return undefined;
      }
      let info = "";
      for (let child = node.node.firstChild; child !== null; child = child.nextSibling) {
        if (child.name === "CodeInfo") {
          info = state.doc.sliceString(child.from, child.to).trim();
          break;
        }
      }
      if (info !== "mermaid") {
        return false;
      }
      const open = state.doc.lineAt(node.from);
      const close = state.doc.lineAt(node.to);
      // The body is what lies between the two fence lines. An unterminated
      // fence has `close === open`, and slicing then yields "" rather than a
      // negative range, which is the right diagram for a fence still being
      // typed: empty, not the fence's own backticks.
      const source = close.from > open.to ? state.doc.sliceString(open.to + 1, close.from - 1) : "";
      fences.push({ from: open.from, to: close.to, source });
      return false;
    },
  });
  return fences;
}

/**
 * The mermaid layer, as a `StateField` rather than as part of the renderer's
 * `ViewPlugin` — the fix for DW-165.
 *
 * **This was a crash, not a preference.** A mermaid fence is several lines
 * replaced by one element, so the decoration needs `block: true`, and
 * CodeMirror refuses a block decoration supplied by a `ViewPlugin` — it throws
 * out of `EditorView` construction. The renderer's plugin therefore could never
 * host this, and from story 37.8 until Story 45.10 any note containing a
 * ` ```mermaid ` fence made the editor throw on open. `galleryLayer` had
 * already met the same wall and taken the same way round it; this is that
 * shape, applied to the block that filed the ledger entry.
 *
 * Composed into {@link livePreview}'s extension array beside the plugin, so a
 * note still has exactly one renderer.
 *
 * The reveal rule is the renderer's, applied to a whole block: put the caret
 * anywhere in the fence and the source comes back, so a diagram stays editable
 * as the text it is.
 */
export function mermaidLayer(): Extension {
  // Scan and reveal are separated for the reason `galleryLayer` separates
  // them: moving the caret must not re-walk the parse tree, and only an edit
  // can change where the fences are.
  const paint = (fences: readonly MermaidFence[], state: EditorState): DecorationSet =>
    Decoration.set(
      fences
        .filter(
          (fence) =>
            !state.selection.ranges.some(
              (range) => range.from <= fence.to && range.to >= fence.from,
            ),
        )
        .map((fence) =>
          Decoration.replace({ widget: new MermaidWidget(fence.source), block: true }).range(
            fence.from,
            fence.to,
          ),
        ),
      true,
    );
  return StateField.define<{ fences: MermaidFence[]; decorations: DecorationSet }>({
    create(state) {
      const fences = mermaidFences(state);
      return { fences, decorations: paint(fences, state) };
    },
    update(value, transaction) {
      if (!transaction.docChanged && transaction.selection === undefined) {
        return value;
      }
      const fences = transaction.docChanged ? mermaidFences(transaction.state) : value.fences;
      return { fences, decorations: paint(fences, transaction.state) };
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}

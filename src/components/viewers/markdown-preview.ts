/**
 * Markdown's rendered view: the note editor's own preview, over a file
 * (Story 45.4, FR-177, AD-88, UX-DR67).
 *
 * **There is no second markdown renderer, and this file does not become one.**
 * `notes/editor/live-preview.ts` says in its first paragraph that it is the
 * renderer and that adding another would be a mistake, because two rendering
 * paths over the same markdown are two places for the document you read and the
 * document you edit to disagree. So the rendered half of a `.md` file is that
 * decoration layer, mounted read-only over the file's bytes, and everything
 * this module adds is the mounting.
 *
 * **Everything CodeMirror is behind one `import()`**, matching `note-editor.tsx`
 * (NFR-27): a reader who opens a CSV must not pay for the markdown grammar, and
 * the quick-capture window must not pay for any of it.
 *
 * ## The guard, and the defect it is standing in front of
 *
 * DW-165: `live-preview.ts` supplies `Decoration.replace({ …, block: true })`
 * from a `ViewPlugin`, and CodeMirror refuses a block decoration from a plugin.
 * The first time the decoration set contains a rendered ```mermaid fence the
 * `EditorView` **throws on construction** — so a note or a file with a diagram
 * in it cannot be opened at all. It has been that way since story 37.8 because
 * nothing in the suite ever assembled a real `EditorView` with the markdown
 * language AND the plugin; `markdown-preview.test.ts` now does, and pins it.
 *
 * The fix is 45.10's (lift that branch into a `StateField`, as `galleryLayer`
 * in the same extension list already is), not this story's: `live-preview.ts`
 * is the note editor's core and changing it mid-wave is something every other
 * agent in this wave builds against.
 *
 * Until then this module declines to render a document it knows the renderer
 * will throw on, **names the reason and the line**, and hands the reader the
 * raw view — which is editable, which is the point of AD-88. Declining out loud
 * beats a pane that is blank for a reason nobody can see.
 *
 * The detection uses the *same* markdown grammar the renderer uses, over the
 * `EditorState` before any view exists — not a regex over the text. A regex
 * would be a second opinion about what a fence is, and the two would eventually
 * disagree about an indented fence or a tilde one.
 *
 * The `try`/`catch` around the construction stays regardless of DW-165, and is
 * the reason this returns a failure instead of throwing: a rendered pane that
 * is blank because something threw is the single outcome this story forbids,
 * and that has to hold for the throw nobody has found yet.
 */
import type { ensureSyntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import type { NoteGalleryVm } from "@/lib/ipc/client";

/** What the decoration layer needs to resolve embeds inside the document. */
export interface MarkdownPreviewOptions {
  /**
   * The notes vault the file's embeds resolve against, or null when the file is
   * not in one.
   *
   * Null is a real state and is passed through rather than papered over: a
   * markdown file on a plain sync profile has no vault to resolve `![[…]]`
   * against, and the decoration layer's own degrade — the wikilink, with the
   * target on it — is the correct rendering of a link keeper cannot follow.
   */
  vaultId: string | null;
  /** A vault-relative asset path as a URL keeper is allowed to load. Absent, an
   *  embedded image renders as its alt text and the path it looked for. */
  assetUrl?: (relPath: string) => string;
  /** Follow a wikilink. Absent, a click does nothing rather than guessing. */
  onOpenLink?: (target: string) => void;
  /** List a folder for a gallery block. Absent, the block says keeper is not
   *  listing here rather than drawing an empty grid. */
  listFolder?: (folder: string) => Promise<NoteGalleryVm>;
}

/** A mounted preview, or the sentence explaining why there is not one. */
export interface MarkdownPreview {
  /** Null when the document rendered. A finished sentence when it did not. */
  failure: string | null;
  /** 1-based line the failure is about, when it is about one. */
  failureLine: number | null;
  /** Always safe to call, including after a failure. */
  destroy: () => void;
}

/**
 * How long the grammar gets to parse the whole document before the fence check
 * gives up and mounts guarded instead.
 *
 * CodeMirror parses lazily; asking for a tree over the whole document of a
 * large file can take real time. A timeout here is not a correctness hole — the
 * `try`/`catch` still catches the construction — it just means the reader gets
 * a caught throw rather than a named line on a pathological file.
 */
const PARSE_BUDGET_MS = 250;

/**
 * Mount `text` into `host` as the note editor's preview, read-only.
 *
 * Never rejects and never throws. A failure is a returned sentence, because
 * every caller's correct response to one is the same — say it and show the raw
 * view — and an exception would let one caller forget.
 */
export async function mountMarkdownPreview(
  host: HTMLElement,
  text: string,
  options: MarkdownPreviewOptions,
): Promise<MarkdownPreview> {
  const [state, view, language, markdown, preview] = await Promise.all([
    import("@codemirror/state"),
    import("@codemirror/view"),
    import("@codemirror/language"),
    import("@codemirror/lang-markdown"),
    import("@/components/notes/editor/live-preview"),
  ]);

  const editorState = state.EditorState.create({
    doc: text,
    extensions: [
      view.EditorView.lineWrapping,
      // Read-only, and not editable at all. Editing markdown IS the note
      // editor, which has a save path and a conflict story of its own; wiring
      // an arbitrary file into it would be a second write path, which AD-88
      // exists to prevent. The raw view is the editable one.
      state.EditorState.readOnly.of(true),
      view.EditorView.editable.of(false),
      markdown.markdown({ base: markdown.markdownLanguage }),
      preview.livePreview({
        // The decoration layer takes a value because the note editor is built
        // per vault. Outside a vault there is nothing to resolve against, and
        // the empty string is what every embed branch treats as "not found" —
        // which renders the wikilink with its target, the correct degrade.
        vaultId: options.vaultId ?? "",
        assetUrl: options.assetUrl ?? ((relPath) => relPath),
        onOpenLink: options.onOpenLink ?? (() => {}),
        listFolder: options.listFolder,
      }),
    ],
  });

  const fence = mermaidFenceLine(editorState, language.ensureSyntaxTree);
  if (fence !== null) {
    // INFO, not debug: `tracing::debug!` and `console.debug` never reach the
    // packaged app's log (DW-162), and a viewer that declined to render is
    // exactly the thing somebody will be asking about.
    console.info(
      `viewers: declining the rendered view of a markdown file — a mermaid fence on line ${fence} ` +
        "crashes the preview renderer (DW-165); showing the source instead",
    );
    return {
      failure:
        "keeper cannot draw this document yet: a mermaid diagram here crashes the preview " +
        "renderer (DW-165). The source below is the file, unchanged",
      failureLine: fence,
      destroy: () => {},
    };
  }

  try {
    const mounted = new view.EditorView({ parent: host, state: editorState });
    return { failure: null, failureLine: null, destroy: () => mounted.destroy() };
  } catch (error) {
    console.info(
      `viewers: the markdown preview could not be built, showing the source instead: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
    // The host may hold a half-built view's nodes. Emptied rather than left,
    // so what the reader sees is the raw view and not a fragment of a render.
    host.replaceChildren();
    return {
      failure:
        "keeper could not draw this document, so the source is below, unchanged: " +
        (error instanceof Error ? error.message : String(error)),
      failureLine: null,
      destroy: () => {},
    };
  }
}

/**
 * The 1-based line of the first ```mermaid fence, or null when there is none.
 *
 * Exported for the test that pins DW-165. Takes `ensureSyntaxTree` rather than
 * importing it so this module keeps its single lazy import boundary.
 */
export function mermaidFenceLine(
  editorState: EditorState,
  ensureFullTree: typeof ensureSyntaxTree,
): number | null {
  const tree = ensureFullTree(editorState, editorState.doc.length, PARSE_BUDGET_MS);
  if (tree === null) {
    return null;
  }
  let line: number | null = null;
  tree.iterate({
    enter: (node) => {
      if (line !== null) {
        return false;
      }
      if (node.name !== "FencedCode") {
        return undefined;
      }
      // The same node the renderer reads, so the two cannot disagree about
      // what counts as a mermaid fence.
      let info = "";
      node.node.cursor().iterate((child) => {
        if (child.name === "CodeInfo") {
          info = editorState.doc.sliceString(child.from, child.to).trim();
          return false;
        }
        return undefined;
      });
      if (info === "mermaid") {
        line = editorState.doc.lineAt(node.from).number;
      }
      return false;
    },
  });
  return line;
}

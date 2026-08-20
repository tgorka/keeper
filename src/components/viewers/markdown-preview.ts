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
 * ## Why this returns a failure instead of throwing
 *
 * A rendered pane that is blank because something threw is the single outcome
 * this story forbids, and that has to hold for the throw nobody has found yet.
 * So both places a document reaches a view are wrapped — the construction, and
 * the adoption {@link MarkdownPreview.setContent} performs — and a failure comes
 * back as a sentence the reader can act on with the raw, editable view
 * underneath it (AD-88). Never as an exception: `setContent` is called from a
 * host effect with no `try` around it, so a throw there takes the panel down.
 *
 * **DW-165 used to be caught here and no longer is.** Until Story 45.10 the
 * renderer supplied a block decoration from a `ViewPlugin`, CodeMirror refused
 * it, and any document containing a ```mermaid fence threw on construction; this
 * module declined such a document by name rather than letting the pane go
 * blank. 45.10 lifted that decoration into a `StateField` (`mermaidLayer`), so
 * a mermaid fence now renders, the pre-flight parse this module used to do is
 * gone, and diagrams are no longer a special case anywhere in the viewer.
 *
 * ## The clamp became a parameter, and the half with teeth still holds
 *
 * Until Story 51.5 the two facets below were unconditional, under a comment
 * saying that editing markdown IS the note editor and that wiring an arbitrary
 * file into it would be a second WRITE path, which AD-88 exists to prevent.
 *
 * That refusal had two halves and only one of them had teeth.
 *
 * The half that HOLDS is the write path, and this module still adds none.
 * Note mode (FR-294) reports its edits to the surface that owns the file's one
 * buffer and saves through that surface's one explicit Save — the same
 * `syncWriteEntry` the Source tab reaches, on the same keystroke. There is no
 * autosave here, no second conflict story, and nothing in this file writes.
 *
 * The half that does NOT hold is that rendering needs a note. It never did:
 * `livePreview`'s only required option is a `vaultId`, which this module
 * already supplies as `""` outside a vault, and note identity is needed by
 * persistence alone. So editability is a parameter — {@link
 * MarkdownPreviewOptions.editing}, whose absence is the clamp — rather than a
 * fact about the renderer.
 *
 * ## The editable half carries the writing tools
 *
 * Note mode is a place a person writes prose (Story 52.3, FR-303), so it mounts
 * the same `notes/editor/writing-tools.ts` the Notes surface and the raw file
 * editor mount — the slash menu, emoji completion, and the translation a
 * toolbar press needs — and never a second copy of any of them. Requested in the
 * same `import()` wave as the editing keymap and only when a destination for an
 * edit was named, so a reader who only ever previews still downloads no emoji
 * table (NFR-27).
 *
 * The toolbar itself is a React control and stays with the host, for the reason
 * `text-viewer.tsx` states where it mounts one: a toolbar acts on a live view,
 * and the view is this module's. What crosses the boundary is {@link
 * MarkdownPreview.runFormat} — non-null exactly when the extensions behind it
 * are in the view, so a host cannot draw a control over an editor that never
 * loaded the commands it presses.
 */
import type { ensureSyntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import type { FormatAction } from "@/components/notes/editor/format-commands";
import type { NoteWidgetOptions } from "@/components/notes/editor/note-widget";
import type { NoteGalleryVm } from "@/lib/ipc/client";

/**
 * Where an edit goes, for a caller that wants the editable pane (Story 51.5).
 *
 * One object rather than a boolean beside two optional callbacks: presence IS
 * the mode, so there is no state in which a view was made editable and its
 * edits have nowhere to land. A caller that cannot say where a keystroke goes
 * gets the read-only preview, which is the correct answer to that question.
 */
export interface MarkdownEditing {
  /**
   * Every document change, as the exact buffer.
   *
   * Not called for an adoption through `setContent`: text that came from the
   * host is not the reader's edit, and reporting it back would be a loop.
   */
  onChange: (next: string) => void;
  /**
   * `Mod-s`, with the document's own text at the moment the chord fired.
   *
   * The text is passed rather than left to the caller's copy so a save cannot
   * write a buffer the view has moved past — the same contract the raw editor
   * hands its host.
   */
  onSave: (next: string) => void;
  /**
   * The accessible name for the editable region.
   *
   * CodeMirror gives its content `role="textbox"` and no name, so an editable
   * pane without this announces itself as an unlabelled text box — the gap
   * Story 45.14 found in the note editor and fixed there the same way. The
   * read-only preview needs none: it is not a control.
   */
  label: string;
}

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
  /**
   * Follow a wikilink. Absent, a click does nothing rather than guessing —
   * and until Story 45.18 this module turned that absence into `() => {}`
   * before handing it down, which is a fabricated value standing in for a
   * missing one and made the decoration layer unable to tell the two apart.
   */
  onOpenLink?: (target: string) => void;
  /** Hand an ordinary link's destination to the OS. Absent, the link renders
   *  without a pressable cursor rather than lying about being pressable. */
  onOpenUrl?: (url: string) => void;
  /** List a folder for a gallery block. Absent, the block says keeper is not
   *  listing here rather than drawing an empty grid. */
  listFolder?: (folder: string) => Promise<NoteGalleryVm>;
  /**
   * Mount a widget block's panel (FR-264). Absent, the real React host is used
   * — the same default the editor takes.
   *
   * A widget in a preview is read-only in the way that matters: a board still
   * moves a card, because moving a card writes the card's own file rather than
   * this document, and refusing that here would make the same block behave
   * differently depending on which pane it was drawn in. What a preview does
   * not offer is editing the callout itself, which is the raw view's job
   * (AD-88) — the same rule the gallery's pins already follow.
   */
  mountWidget?: NoteWidgetOptions["mount"];
  /**
   * Where an edit goes, or absent for the read-only preview (Story 51.5).
   *
   * Absent is the default and stays the default: a person opening a file to
   * read it must not land in an editor.
   */
  editing?: MarkdownEditing;
}

/** A mounted preview, or the sentence explaining why there is not one. */
export interface MarkdownPreview {
  /** Null when the document rendered. A finished sentence when it did not. */
  failure: string | null;
  /**
   * Adopt text that came from outside this view: `null` when it landed, and a
   * finished sentence when it could not.
   *
   * The same contract as `TextEditorMount.setContent` in `text-editor-host.ts`,
   * including the no-op when the document already reads that way — which is
   * what makes Note mode's controlled prop safe: the pane reports every
   * keystroke upward, the host stores it, the identical string comes back, and
   * dispatching it would reset the selection on every character. Remounting on
   * `[text]`, which is what the pane used to do, destroys the caret, the undo
   * stack and the scroll position instead.
   *
   * **A throw from an update is reported the same way a throw from construction
   * is** — a sentence, in the {@link failure} shape, which the host renders
   * above the raw view (AD-88). It is the same reader in the same position:
   * this document cannot be drawn and the source below it can. The two
   * alternatives were both worse. Propagating leaves the exception to the
   * host's effect and takes the panel down, which is the one outcome this
   * module's first paragraph forbids. Swallowing leaves a live view showing the
   * PREVIOUS text with nothing on screen saying so, which is how a reader
   * concludes their file changed — and it was silently reachable only because
   * `setContent` is new: before Story 51.5 every text arrived through the
   * construction path, which has always been caught.
   *
   * Always safe to call, including after a failure.
   */
  setContent: (next: string) => string | null;
  /**
   * Run a toolbar press against this view, or `null` when there is no editable
   * view for one to land in (Story 52.3, FR-303).
   *
   * The same contract `TextEditorMount.runFormat` publishes, from the same
   * shared translation (`runFormatAction`): non-null exactly when the writing
   * extensions are in the view, so a surface cannot render a toolbar over an
   * editor that never loaded the commands behind it. `null` for the read-only
   * preview and for both failures — a document that was never drawn has nothing
   * to format.
   */
  runFormat: ((action: FormatAction) => void) | null;
  /** Always safe to call, including after a failure. */
  destroy: () => void;
}

/**
 * How long the grammar gets to parse the whole document before giving up.
 *
 * CodeMirror parses lazily; asking for a tree over the whole document of a
 * large file can take real time, so {@link mermaidFenceLine} is bounded rather
 * than allowed to block a pane.
 */
const PARSE_BUDGET_MS = 250;

/** What an exception has to say for itself. Four call sites — the log line and
 *  the reader's sentence, for a throw from construction and for a throw from an
 *  adoption — and they have to word an unknown the same way or the log and the
 *  banner describe one failure differently. */
function reason(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Mount `text` into `host` as the note editor's preview.
 *
 * Read-only unless {@link MarkdownPreviewOptions.editing} says where an edit
 * goes — see the module comment for which half of the old refusal that keeps.
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
  const editing = options.editing ?? null;
  // `.catch` and not a bare `await`: the seven chunks below are fetched over the
  // network in production and evaluated by a module runner in a test, and a
  // rejection from either — an offline reader, a deploy that moved the chunk, a
  // host torn down while the wave was in flight — would reject THIS promise and
  // break the contract three lines up that every caller was written against.
  // Normalised to a sentence here, because `reason` is the same wording the two
  // construction failures below already use and a caller cannot tell the three
  // apart usefully.
  const loaded = await Promise.all([
    import("@codemirror/state"),
    import("@codemirror/view"),
    import("@codemirror/lang-markdown"),
    import("@/components/notes/editor/live-preview"),
    // Unconditional, unlike the three below: a reader who never edits still has
    // to see a `==highlight==` as one. It is a delimiter table, not a feature.
    import("@/components/notes/editor/markdown-marks"),
    // In the same wave rather than after it, so Note mode costs one chunk fetch
    // and never a second round trip — and `null` when it was not asked for, so
    // a reader who only ever previews does not download an editing keymap to
    // not use it.
    editing === null ? null : import("@codemirror/commands"),
    editing === null ? null : import("@/components/notes/editor/indent-keymap"),
    // Story 52.3's writing tools, on the same terms and in the same wave: the
    // slash menu, emoji completion and the toolbar's translation, from the one
    // module that defines them. `null` when no edit has a destination, so the
    // ~45 KB emoji table is downloaded by a reader who can type into this
    // document and by nobody else (NFR-27).
    editing === null ? null : import("@/components/notes/editor/writing-tools"),
  ]).catch((error: unknown) => (error instanceof Error ? error.message : String(error)));
  if (typeof loaded === "string") {
    console.info(
      `viewers: the markdown preview's editor could not be loaded, showing the source instead: ${loaded}`,
    );
    return {
      failure:
        "keeper could not load its editor for this document, so the source is below, unchanged: " +
        loaded,
      // Nothing was mounted, so there is nothing to adopt into: the host is
      // showing the raw view, which holds the same buffer.
      setContent: () => null,
      // Nothing to press against, for the same reason.
      runFormat: null,
      destroy: () => {},
    };
  }
  const [state, view, markdown, preview, marks, commands, indent, writing] = loaded;

  // One flag rather than reading the document back: the update listener runs
  // for programmatic dispatches too, and an adoption of the host's own buffer
  // must not be reported back as the reader's edit — that is the loop a
  // controlled prop over a live view exists to avoid.
  let adopting = false;

  const editorState = state.EditorState.create({
    doc: text,
    extensions: [
      view.EditorView.lineWrapping,
      ...(editing === null || commands === null || indent === null || writing === null
        ? [
            // The clamp, reached when no caller named a destination for an edit
            // — and, by construction, only then: the three editing chunks are
            // requested in the same wave `editing` is read in, so the null
            // checks above are TypeScript's narrowing rather than a second
            // state. Read-only is the right way for that narrowing to fail.
            //
            // Both facets, never one: `editable` takes `contenteditable` off
            // the content DOM and stops typing, and `readOnly` is what stops
            // Backspace, Enter, cut and paste, which arrive as commands — the
            // pair `text-editor-host.ts` spells out.
            state.EditorState.readOnly.of(true),
            view.EditorView.editable.of(false),
          ]
        : [
            view.EditorView.contentAttributes.of({ "aria-label": editing.label }),
            commands.history(),
            view.keymap.of([
              ...commands.defaultKeymap,
              ...commands.historyKeymap,
              // Story 43.1's binding, imported and not restated. Tab is claimed
              // here for the reason it is claimed in a note and in the raw
              // editor: an unclaimed Tab escapes to the web view, which edits
              // the DOM under CodeMirror.
              ...indent.indentBindings,
              {
                // The Source tab's save, on the Source tab's chord, through the
                // surface that owns the buffer. Deliberately the only way text
                // leaves this view: there is no autosave (three recorded
                // refusals) and `syncWriteEntry` is last-write-wins with no
                // revision guard, so an explicit press is the guard.
                key: "Mod-s",
                preventDefault: true,
                run: (target) => {
                  editing.onSave(target.state.doc.toString());
                  return true;
                },
              },
            ]),
            view.EditorView.updateListener.of((update) => {
              if (!update.docChanged || adopting) {
                return;
              }
              editing.onChange(update.state.doc.toString());
            }),
            // Story 52.3's three tools, from the module that owns them, on the
            // editable branch alone: an absent extension cannot be triggered,
            // so there is no state in which `/` opens a menu over a preview a
            // reader was told they cannot change.
            writing.markdownWritingTools(),
          ]),
      markdown.markdown({
        base: markdown.markdownLanguage,
        // The same mark list the note editor loads; see `markdown-marks.ts`.
        extensions: [...marks.MARKDOWN_MARKS],
      }),
      preview.livePreview({
        // The decoration layer takes a value because the note editor is built
        // per vault. Outside a vault there is nothing to resolve against, and
        // the empty string is what every embed branch treats as "not found" —
        // which renders the wikilink with its target, the correct degrade.
        vaultId: options.vaultId ?? "",
        assetUrl: options.assetUrl ?? ((relPath) => relPath),
        onOpenLink: options.onOpenLink,
        onOpenUrl: options.onOpenUrl,
        listFolder: options.listFolder,
        mountWidget: options.mountWidget,
      }),
      // Line endings are the file's, not the editor's — `text-editor-host.ts`'s
      // facet and its reason: without it CodeMirror hands back "\n" for every
      // line, so saving an untouched CRLF file would rewrite every line in it.
      // Set for the read-only pane too, because `setContent`'s equality check
      // is what keeps a keystroke from resetting the caret, and an equality
      // check over a document that does not round-trip byte for byte is one
      // that always answers "different".
      state.EditorState.lineSeparator.of("\n"),
    ],
  });

  try {
    const mounted = new view.EditorView({ parent: host, state: editorState });
    return {
      failure: null,
      setContent: (next: string) => {
        if (mounted.state.doc.toString() === next) {
          return null;
        }
        adopting = true;
        try {
          mounted.dispatch({ changes: { from: 0, to: mounted.state.doc.length, insert: next } });
          return null;
        } catch (error) {
          // A `StateField` throwing during a dispatch is DW-165's shape arriving
          // through the one path that did not use to exist — and CodeMirror
          // swallows a view plugin's throw where it does not swallow a field's,
          // so this is the class of failure that reaches a caller at all.
          console.info(
            `viewers: the markdown preview could not adopt a change, showing the source instead: ${reason(error)}`,
          );
          return `keeper could not draw this document after it changed, so the source is below: ${reason(error)}`;
        } finally {
          adopting = false;
        }
      },
      // The same translation the note editor and the raw file editor perform,
      // from the same module — which is the whole of Story 50.3 reaching this
      // surface. Non-null exactly when the extensions are in the view, because
      // it is the same `writing` the extension branch above was built from.
      runFormat:
        writing === null
          ? null
          : (action: FormatAction) => writing.runFormatAction(mounted, action),
      destroy: () => mounted.destroy(),
    };
  } catch (error) {
    console.info(
      `viewers: the markdown preview could not be built, showing the source instead: ${reason(error)}`,
    );
    // The host may hold a half-built view's nodes. Emptied rather than left,
    // so what the reader sees is the raw view and not a fragment of a render.
    host.replaceChildren();
    return {
      failure:
        "keeper could not draw this document, so the source is below, unchanged: " + reason(error),
      // Nothing to adopt into, so nothing can fail: the host is showing the raw
      // view, which holds the same buffer this would have received.
      setContent: () => null,
      // Nor anything to format: there is no view.
      runFormat: null,
      destroy: () => {},
    };
  }
}

/**
 * The 1-based line of the first ```mermaid fence, or null when there is none.
 *
 * Kept, and still exported, because it is how `markdown-preview.test.ts` proves
 * that this module and the renderer read the SAME grammar — the property that
 * made DW-165 findable in the first place, and the one a regex over the text
 * would quietly break for an indented or tilde fence.
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

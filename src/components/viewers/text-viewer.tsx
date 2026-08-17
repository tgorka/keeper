/**
 * The raw text editor every text-shaped file is edited through (Story 45.6,
 * FR-179, AD-88).
 *
 * # One component, mounted by whoever needs an editor
 *
 * Story 45.4 binds the registry's `text` viewer and owns the raw/rendered
 * toggle; its raw half mounts this. Story 45.12 will mount it inside a note.
 * Nothing here reads bytes, resolves a path or knows what a vault is: it is a
 * controlled text area with CodeMirror behind it, and its whole contract is
 * "here are the characters, tell me when they change". That is what keeps AD-88
 * true — the rendered view and the raw view are looking at one buffer, so they
 * cannot come to disagree about what the file says.
 *
 * The loading, dirty-tracking and saving half is {@link
 * ./use-text-file.useTextFile}, deliberately in its own module so a surface can
 * take one without the other, and so 45.4's chrome does not pull the editor
 * packages in just to ask whether a file is too big.
 *
 * # Controlled, but reconciled rather than rebuilt
 *
 * React's controlled-input pattern and CodeMirror's document model do not
 * naturally agree: the view owns the text while a person types, and re-creating
 * it for every keystroke would destroy the caret, the selection and the undo
 * stack. So the prop is adopted only when it genuinely differs from the live
 * document ({@link TextEditorMount.setContent}), which makes the normal
 * round trip — keystroke, `onChange`, parent state, prop back in — a no-op, and
 * an outside write (45.4's CSV cell edit, a reload) a single minimal dispatch.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { FormatAction } from "@/components/notes/editor/format-commands";
import { FormatToolbar } from "@/components/notes/format-toolbar";
import { isOversizeForEditing, mountTextEditor, type TextEditorMount } from "./text-editor-host";
import type { FileOrigin } from "./use-text-file";

export interface TextEditorSurfaceProps {
  /** The buffer. Controlled: a change to this prop is adopted by the live view. */
  content: string;
  /**
   * The registry's language id (`entry.language`), or `null` for plain text.
   *
   * The id and not the file name, deliberately. `src/lib/viewers` owns
   * extension-to-language and its guard test proves it is the only table that
   * does; deriving a grammar from `fileName` here would be a second classifier
   * with nothing keeping it in step.
   */
  language: string | null;
  /** Display and accessible naming only. Never parsed. */
  fileName: string;
  /**
   * The file's size in the words `keeper_core::size` chose, for the read-only
   * banner. Absent means the banner cannot name a size — a worse message, so
   * callers holding a `TextFileVm` should always pass `vm.sizeLabel`.
   */
  sizeLabel?: string;
  /**
   * Whether the caller has already decided this file cannot be written — a
   * read-only volume, a file outside every profile, a format whose bytes keeper
   * will not touch.
   *
   * Note that the size guard below can force read-only even when this is false.
   */
  readOnly?: boolean;
  /** Every document change, as the exact buffer. Never fires when read-only. */
  onChange?: (next: string) => void;
  /** `Mod-s`, with the exact current text. Never fires when read-only. */
  onSave?: (next: string) => void | Promise<void>;
  /** Rendered in the accessible label so a screen reader can tell two open
   *  panels apart. Profile-relative, never absolute (FR-145). */
  path?: string;
  /** The profile or vault this file sits in, for the same labelling reason. */
  vault?: string | null;
  /**
   * Which file these bytes came from, or absent when they are not a file — a
   * paste, a scratch buffer, anything mounted over bytes that never came from
   * `sync_read_text`.
   *
   * Identity and not naming: `path` and `vault` above are what a screen reader
   * announces, and a name is not an identity. See the mount effect.
   */
  loadedFrom?: FileOrigin;
  /**
   * Whether this buffer is markdown a save can follow, and therefore gets the
   * format toolbar, the slash menu and emoji completion (Story 50.3, FR-233).
   *
   * The caller's verdict, not this component's: markdown is what the registry's
   * `format` says it is (AD-87), and whether a save can follow is the frame's
   * question. Absent means no — a surface that has not thought about it gets the
   * plain editor it had before this story.
   */
  writingTools?: boolean;
}

/**
 * A text editor over exactly these bytes.
 *
 * **The size guard is inside, not a caller's responsibility.** Rust decides
 * `oversize` when it reads a file, but this component is also mounted over
 * buffers that never came from `sync_read_text` — a note embed, a paste — and
 * "the pane froze" must not depend on which caller you are. So the buffer is
 * measured here too, against the same constant, and an oversize one is editable
 * by nobody: `onChange` cannot fire, so no code path exists that could hand a
 * truncated prefix to a save.
 */
export function TextEditorSurface({
  content,
  language,
  fileName,
  sizeLabel,
  readOnly = false,
  onChange,
  onSave,
  path,
  vault,
  loadedFrom,
  writingTools = false,
}: TextEditorSurfaceProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const mountRef = useRef<TextEditorMount | null>(null);
  // Whether there is a live editor behind `mountRef` *right now*, as state
  // rather than as the ref itself, because the toolbar's presence is a render
  // decision and a ref does not cause one. The two windows this closes are the
  // reason it exists: the boot effect assigns `mountRef.current` only after six
  // dynamic imports have resolved, and the cleanup nulls it again at the start
  // of every rebuild — so a toolbar drawn from `tools` alone is clickable over
  // no editor for the whole of both, and every press in that window is
  // swallowed. That is the exact "toolbar whose presses land nowhere" shape
  // {@link TextEditorMount.runFormat}'s null exists to prevent, and the null
  // alone cannot prevent it because it is not what the toolbar is drawn from.
  const [mounted, setMounted] = useState(false);
  // The callbacks and the buffer as the *editor* should see them right now.
  // Held in a ref rather than closed over at construction because the editor
  // outlives every render: a handler captured in the boot effect would report
  // edits to the first render's parent forever.
  const latest = useRef({ content, onChange, onSave, readOnly: false });

  const oversize = isOversizeForEditing(content);
  const locked = readOnly || oversize;
  latest.current = { content, onChange, onSave, readOnly: locked };

  // One value feeds both the extensions and the toolbar, so the control and the
  // commands behind it cannot come to disagree. `locked` is in it because a
  // buffer nobody can write is a buffer no formatting action can land in — and
  // that is the `workspace/` case (AD-113) as well as the oversize one.
  const tools = writingTools && !locked;

  const runFormat = useCallback((action: FormatAction) => {
    // Optional twice over, and both are defence rather than a live path: the
    // toolbar is drawn only when `tools` AND `mounted` are true, so there is a
    // mount, and the same `tools` built that mount with the writing extensions
    // in it, so its translation is non-null. The swallow is kept rather than a
    // throw because a press can only arrive between renders, never between a
    // cleanup and the render that follows it.
    mountRef.current?.runFormat?.(action);
  }, []);

  // Keyed on the grammar, on whether the tools are wanted, and on WHICH FILE
  // this is. An extension list is fixed at construction, so a buffer that
  // becomes read-only past the size limit has to be rebuilt to lose the tools
  // rather than keep a slash menu no insertion can follow, and a grammar cannot
  // be swapped into a live view without a compartment handle.
  //
  // The file is in the key because nothing above remounts this component when
  // the file changes — which is what the comment here used to claim, and it was
  // never true: `RawRenderedView` renders one editor in one position, so a panel
  // that replaces its target in place (story 51.5's `MarkdownPane` records the
  // same defect from the other side) hands this same view a different file's
  // bytes. Without it the undo history spans two files, and one undo followed by
  // one save writes the previous file's text into this one. Absent coordinates
  // mean the buffer is not a file, and then there is nothing to key on.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `loadedFrom`'s two halves are rebuild triggers, not reads — see above.
  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return;
    }
    let disposed = false;
    void (async () => {
      const mount = await mountTextEditor({
        parent: host,
        content: latest.current.content,
        language,
        readOnly: latest.current.readOnly,
        writingTools: tools,
        onChange: (next) => {
          // Belt as well as braces. `EditorView.editable` already blocks user
          // input, but a programmatic dispatch from a future extension would
          // not be, and a truncated prefix escaping as an edit is the one
          // failure that loses a file.
          if (latest.current.readOnly) {
            return;
          }
          latest.current.onChange?.(next);
        },
        onSave: () => {
          if (latest.current.readOnly) {
            return;
          }
          void latest.current.onSave?.(latest.current.content);
        },
      });
      if (disposed) {
        mount.destroy();
        return;
      }
      mountRef.current = mount;
      // The prop may have moved while the editor chunk was in flight.
      mount.setContent(latest.current.content);
      mount.setReadOnly(latest.current.readOnly);
      // Beside the assignment and never apart from it: this is the render the
      // toolbar is allowed to appear on, and the whole point is that it is the
      // one where a press has somewhere to go.
      setMounted(true);
    })();
    return () => {
      disposed = true;
      mountRef.current?.destroy();
      mountRef.current = null;
      // Cleared here for the rebuild case, which is the guaranteed one: a
      // change to `language` or `tools` tears the view down while `tools` may
      // still be true, and without this the toolbar would sit over an empty
      // host for the whole of the next async mount.
      setMounted(false);
    };
  }, [language, tools, loadedFrom?.profileOrVaultId, loadedFrom?.relativePath]);

  useEffect(() => {
    mountRef.current?.setContent(content);
  }, [content]);

  useEffect(() => {
    mountRef.current?.setReadOnly(locked);
  }, [locked]);

  const label = [fileName, path, vault].filter((part) => part !== undefined && part !== null);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {oversize ? (
        <p
          className="border-b px-3 py-1 text-meta text-muted-foreground"
          data-testid="text-viewer-oversize"
        >
          {sizeLabel === undefined
            ? "This file is too large to edit, so it is open read-only and only the first part of it is shown."
            : `This file is ${sizeLabel}, too large to edit, so it is open read-only and only the first part of it is shown.`}
        </p>
      ) : null}
      {/* Directly over the text it formats, below the oversize banner and above
          the editor — the note editor's own placement, so a person who learned
          the toolbar in Notes finds it in the same place over a session log.

          Here rather than in `TextFileFrame`'s save bar: a toolbar acts on a
          live view, and the view is mounted by this component. Hoisting the
          control two levels up would mean passing an editor handle upward, and
          the handle a parent holds is exactly how a press lands in a view that
          has since been replaced by the rendered tab (AD-88).

          `mounted` and not `tools` alone: `tools` is known at the first render
          and the editor is not, so drawing from `tools` puts a live control
          over nothing for the length of six dynamic imports, and again for the
          length of every rebuild. The toolbar appears when the thing it acts
          on does. */}
      {tools && mounted ? <FormatToolbar onAction={runFormat} /> : null}
      {/* `group`, not the `fieldset` the linter suggests: a fieldset groups
          form controls, and this is the mount point for a composite widget
          whose own `role="textbox"` CodeMirror puts on `.cm-content` inside.
          The name belongs on the container so a screen reader announces which
          file is being edited before it reaches the text. */}
      {/* biome-ignore lint/a11y/useSemanticElements: see the comment above */}
      <div
        ref={hostRef}
        role="group"
        aria-label={`${label.join(" in ")}${locked ? " (read-only)" : ""}`}
        className="min-h-0 flex-1 overflow-auto"
      />
    </div>
  );
}

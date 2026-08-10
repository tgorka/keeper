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
import { useEffect, useRef } from "react";
import { isOversizeForEditing, mountTextEditor, type TextEditorMount } from "./text-editor-host";

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
}: TextEditorSurfaceProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const mountRef = useRef<TextEditorMount | null>(null);
  // The callbacks and the buffer as the *editor* should see them right now.
  // Held in a ref rather than closed over at construction because the editor
  // outlives every render: a handler captured in the boot effect would report
  // edits to the first render's parent forever.
  const latest = useRef({ content, onChange, onSave, readOnly: false });

  const oversize = isOversizeForEditing(content);
  const locked = readOnly || oversize;
  latest.current = { content, onChange, onSave, readOnly: locked };

  // Keyed on `language` as well as nothing else: a grammar cannot be swapped
  // into a live view without a compartment handle, and swapping the FILE under
  // an editor is not a thing this component does — 45.4 remounts it. Rebuilding
  // on a language change is therefore both correct and rare.
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
    })();
    return () => {
      disposed = true;
      mountRef.current?.destroy();
      mountRef.current = null;
    };
  }, [language]);

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
          className="border-b px-3 py-1 text-[11px] text-muted-foreground"
          data-testid="text-viewer-oversize"
        >
          {sizeLabel === undefined
            ? "This file is too large to edit, so it is open read-only and only the first part of it is shown."
            : `This file is ${sizeLabel}, too large to edit, so it is open read-only and only the first part of it is shown.`}
        </p>
      ) : null}
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

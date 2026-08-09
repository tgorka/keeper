/**
 * The note editor (Story 37.6, UX-DR40).
 *
 * CodeMirror 6 in **live-preview mode only**. There is no preview toggle as a
 * primary affordance, because a toggle asks the user to hold two mental models
 * of one document and to keep pressing a button to move between them. The
 * decoration layer in `editor/live-preview.ts` *is* the renderer; source is
 * revealed on the line under the caret and nowhere else.
 *
 * **Everything CodeMirror is behind one `import()`.** The editor packages plus
 * mermaid are several hundred kilobytes that a user who never opens a note
 * should never pay for — and, more sharply, that the quick-capture window must
 * never pay for, because NFR-27 gives it 300 ms and it imports none of this.
 * The type-only imports at the top of this file are erased at compile time, so
 * the only runtime edge into `@codemirror/*` is the dynamic import in the boot
 * effect.
 *
 * The editor never renders the store's buffer back into CodeMirror. CodeMirror
 * owns the text while the user is typing; the store learns about edits through
 * `onEdit`. Only `base` — what Rust last delivered or acknowledged — flows the
 * other way, and it flows as the minimal splice, so an agent appending a
 * section at the end of the file does not move the caret at the top of it.
 *
 * History and conflict resolution replace the body **without unmounting it**:
 * the editor stays alive behind them so Escape returns to the caret it left,
 * which is a promise a remount could not keep.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useNotesBody } from "@/hooks/use-notes-body";
import { type NoteWriteVm, notesTagTree } from "@/lib/ipc/client";
import { markSaved, notesEditorStore, useNotesEditorStore } from "@/lib/stores/notes-editor";
import { BacklinksPanel } from "./backlinks-panel";
import { ConflictResolver } from "./conflict-resolver";
import { NoteDiffBar } from "./note-diff-bar";
import { NoteHistoryPanel } from "./note-history-panel";
import { PropertiesPanel, readFrontmatter, recordingSessionId } from "./properties-panel";

/** What the editor pane is showing. */
type EditorMode = "edit" | "history" | "conflict";

/**
 * The handful of operations the surface needs from the lazily loaded editor.
 *
 * Naming them here is what lets every `@codemirror/*` value stay inside the
 * boot closure: nothing outside it holds an editor type at runtime.
 */
interface EditorRuntime {
  /** Adopt text that came from outside this buffer, minimally and unrecorded. */
  applyExternal: (text: string) => void;
  /**
   * Put the caret at a byte offset, clamped.
   *
   * Separate from `applyExternal` because the two happen at different moments:
   * the document arrives over the channel, and the caret hint has to be applied
   * after it — the editor is constructed before either exists.
   */
  placeCaret: (at: number) => void;
  focus: () => void;
  destroy: () => void;
}

/** The note's title: its first body line, `#` stripped (FR-98). Derived from
 *  the buffer rather than a list row so it tracks what is being typed — and the
 *  buffer is the body, so there is no block to step over. */
export function deriveTitle(text: string): string {
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (trimmed !== "") {
      return trimmed.replace(/^#+\s*/, "");
    }
  }
  return "Untitled";
}

/**
 * Resolve a link written relative to the note into a vault-relative path.
 *
 * Rust re-derives and contains this path before reading anything (AD-59), so a
 * mistake here is a 404 and never an escape.
 */
export function vaultRelative(notePath: string | null, rel: string): string {
  const dir = notePath === null ? "" : notePath.slice(0, notePath.lastIndexOf("/") + 1);
  const segments: string[] = [];
  for (const part of `${rel.startsWith("/") ? "" : dir}${rel}`.split("/")) {
    if (part === "" || part === ".") {
      continue;
    }
    if (part === "..") {
      segments.pop();
      continue;
    }
    segments.push(part);
  }
  return segments.join("/");
}

export interface SaveState {
  saving: boolean;
  dirty: boolean;
  savedAtMs: number | null;
  error: string | null;
}

/**
 * The one caption the header shows about saving.
 *
 * There is no save button anywhere in the product, so this word is the entire
 * feedback surface — and it stays empty while the user types, because a word
 * that flickers on every keystroke is noise rather than information.
 */
export function saveStateWord(state: SaveState): string {
  if (state.error !== null) {
    return state.error;
  }
  if (state.saving) {
    return "Saving…";
  }
  if (state.dirty || state.savedAtMs === null) {
    return "";
  }
  const at = new Date(state.savedAtMs).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
  return `Saved · ${at}`;
}

export interface NoteEditorProps {
  vaultId: string;
  noteId: string | null;
  /** Open another note — a backlink row, or a wikilink that resolved. */
  onOpenNote?: (noteId: string) => void;
  /** Follow a wikilink by name. Resolution belongs to the surface that owns
   *  the list and the link graph, never to the decoration layer. */
  onFollowLink?: (target: string) => void;
}

export function NoteEditor({ vaultId, noteId, onOpenNote, onFollowLink }: NoteEditorProps) {
  const body = useNotesBody(vaultId, noteId);
  const base = useNotesEditorStore((state) => state.base);
  const rev = useNotesEditorStore((state) => state.rev);
  const frontmatter = useNotesEditorStore((state) => state.frontmatter);
  const path = useNotesEditorStore((state) => state.path);
  const subscriptionId = useNotesEditorStore((state) => state.subscriptionId);
  const savedAtMs = useNotesEditorStore((state) => state.savedAtMs);
  const conflictCopy = useNotesEditorStore((state) => state.conflictCopy);
  const error = useNotesEditorStore((state) => state.error);

  const hostRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<EditorRuntime | null>(null);
  const [mode, setMode] = useState<EditorMode>("edit");
  const [showProperties, setShowProperties] = useState(false);
  const [conflictTheirs, setConflictTheirs] = useState<string | null>(null);

  const openHistory = useCallback(() => setMode("history"), []);
  const toggleProperties = useCallback(() => setShowProperties((shown) => !shown), []);

  // Refs, not effect dependencies: rebuilding the editor because a callback
  // identity changed would throw away the document, the undo stack and the
  // caret. Every one of these is read at the moment it fires.
  const latest = useRef({ onEdit: body.onEdit, save: body.save, openHistory, toggleProperties });
  latest.current = { onEdit: body.onEdit, save: body.save, openHistory, toggleProperties };
  const pathRef = useRef(path);
  pathRef.current = path;
  const followRef = useRef(onFollowLink);
  followRef.current = onFollowLink;
  // The recording note's identity (Story 42.4), read from the block the
  // properties panel owns and by the predicate that panel decides with. It
  // reaches the decoration layer as a getter for `pathRef`'s reason: the editor
  // outlives the note in it, and a value captured at construction would make
  // every later note's embeds resolve against the first note's recording.
  const sessionId = useMemo(() => recordingSessionId(readFrontmatter(frontmatter)), [frontmatter]);
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;

  useEffect(() => {
    const host = hostRef.current;
    if (host === null || noteId === null) {
      return;
    }
    let disposed = false;

    void (async () => {
      const [state, view, commands, markdown, autocomplete, preview, wikilink, tags, slash] =
        await Promise.all([
          import("@codemirror/state"),
          import("@codemirror/view"),
          import("@codemirror/commands"),
          import("@codemirror/lang-markdown"),
          import("@codemirror/autocomplete"),
          import("./editor/live-preview"),
          import("./editor/wikilink"),
          import("./editor/tag-complete"),
          import("./editor/slash-menu"),
        ]);
      if (disposed) {
        return;
      }

      let cachedTags: string[] | null = null;
      const editorView = new view.EditorView({
        parent: host,
        state: state.EditorState.create({
          // Read imperatively: whatever the channel has delivered by the time
          // the chunk lands. Later revisions arrive through the reconcile
          // effect below rather than by rebuilding the editor.
          doc: notesEditorStore.getState().text,
          // A template's `{{cursor}}`, clamped to the document. Absent a hint the
          // caret goes to the end, which is where a person continuing a note wants
          // it — and offset 0 is now simply the first character of the body, since
          // the frontmatter block is not in this buffer at all.
          selection: {
            anchor: Math.max(
              0,
              Math.min(
                notesEditorStore.getState().cursor ?? notesEditorStore.getState().text.length,
                notesEditorStore.getState().text.length,
              ),
            ),
          },
          extensions: [
            view.EditorView.lineWrapping,
            commands.history(),
            view.keymap.of([
              ...commands.defaultKeymap,
              ...commands.historyKeymap,
              ...autocomplete.completionKeymap,
              {
                // ⌘S has no save semantic to attach to, so it force-flushes. A
                // bound key that does something honest beats a no-op that
                // teaches distrust.
                key: "Mod-s",
                preventDefault: true,
                run: () => {
                  void latest.current.save();
                  return true;
                },
              },
              {
                key: "Mod-Shift-d",
                preventDefault: true,
                run: () => {
                  latest.current.openHistory();
                  return true;
                },
              },
              {
                key: "Mod-Shift-p",
                preventDefault: true,
                run: () => {
                  latest.current.toggleProperties();
                  return true;
                },
              },
            ]),
            markdown.markdown({ base: markdown.markdownLanguage }),
            autocomplete.autocompletion({
              override: [
                wikilink.wikilinkSource(vaultId),
                tags.tagCompleteSource(async () => {
                  cachedTags ??= tags.tagPaths((await notesTagTree(vaultId)).nodes);
                  return cachedTags;
                }),
                slash.slashMenuSource(),
              ],
            }),
            preview.livePreview({
              assetUrl: (rel) =>
                `keeper-note://${vaultId}/${vaultRelative(pathRef.current, rel)
                  .split("/")
                  .map(encodeURIComponent)
                  .join("/")}`,
              onOpenLink: (target) => followRef.current?.(target),
              recordingSession: () => sessionRef.current,
            }),
            view.EditorView.updateListener.of((update) => {
              if (!update.docChanged) {
                return;
              }
              // A change keeper applied on someone else's behalf is not an edit
              // to report back; reporting it would loop it through Rust.
              const remote = update.transactions.some(
                (each) => each.annotation(state.Transaction.remote) === true,
              );
              if (!remote) {
                latest.current.onEdit(update.state.doc.toString());
              }
            }),
            view.EditorView.domEventHandlers({
              blur: () => {
                void latest.current.save();
                return false;
              },
            }),
          ],
        }),
      });

      runtimeRef.current = {
        applyExternal: (text: string) => {
          const splice = preview.spliceBetween(editorView.state.doc.toString(), text);
          if (splice === null) {
            return;
          }
          editorView.dispatch({
            changes: splice,
            // Someone else's edit must not pollute the user's undo history, and
            // must not be echoed back to Rust as a local edit.
            annotations: [
              state.Transaction.remote.of(true),
              state.Transaction.addToHistory.of(false),
            ],
          });
          preview.flashExternal(editorView, splice.from, splice.from + splice.insert.length);
        },
        placeCaret: (at: number) => {
          const clamped = Math.max(0, Math.min(at, editorView.state.doc.length));
          editorView.dispatch({
            selection: { anchor: clamped },
            annotations: [
              state.Transaction.remote.of(true),
              state.Transaction.addToHistory.of(false),
            ],
          });
        },
        focus: () => editorView.focus(),
        destroy: () => editorView.destroy(),
      };
      editorView.focus();
      // The document almost never exists yet when this chunk lands — the channel
      // delivers `Reset` after the lazy import resolves — so the caret hint is
      // consumed here, once the runtime is able to act on it.
      const opening = notesEditorStore.getState().cursor;
      if (opening !== null) {
        runtimeRef.current.placeCaret(opening);
      }
    })();

    return () => {
      disposed = true;
      runtimeRef.current?.destroy();
      runtimeRef.current = null;
    };
  }, [vaultId, noteId]);

  // The opening `Reset` usually lands AFTER the editor chunk, so this is the
  // effect that actually gets to honour the caret hint: the document has just
  // been spliced in, and only now is there anything to put a caret into.
  const openingCursor = body.cursor;
  useEffect(() => {
    runtimeRef.current?.applyExternal(base);
    if (openingCursor !== null) {
      runtimeRef.current?.placeCaret(openingCursor);
    }
  }, [base, openingCursor]);

  useEffect(() => {
    if (mode === "edit") {
      runtimeRef.current?.focus();
    }
  }, [mode]);

  const adoptPanelWrite = useCallback((text: string, write: NoteWriteVm) => {
    // A property edit goes straight to disk, and it writes the buffer with it, so
    // its result is the editor's new base and the block Rust hands back — `updated`
    // stamped — is the block the panel now renders.
    markSaved(text, write);
  }, []);

  const leaveMode = useCallback(() => {
    setConflictTheirs(null);
    setMode("edit");
  }, []);

  if (noteId === null) {
    return (
      <div className="flex h-full items-center justify-center px-6 text-sm text-muted-foreground">
        Pick a note, or write a new one.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b px-3 py-1.5">
        <h1 className="min-w-0 flex-1 truncate font-medium text-sm">{deriveTitle(body.text)}</h1>
        <span className="truncate font-mono text-[11px] text-muted-foreground">{path ?? ""}</span>
        <span className="text-[11px] text-muted-foreground">
          {saveStateWord({ saving: body.saving, dirty: body.dirty, savedAtMs, error })}
        </span>
        <Button size="sm" variant="ghost" onClick={toggleProperties}>
          Properties
        </Button>
        <Button size="sm" variant="ghost" onClick={openHistory}>
          History
        </Button>
      </header>

      <NoteDiffBar
        onShowChanges={openHistory}
        onResolve={() => {
          setConflictTheirs(body.pending?.text ?? null);
          setMode("conflict");
        }}
      />

      {conflictCopy === null ? null : (
        <p className="border-b px-3 py-1 text-[11px] text-muted-foreground">
          keeper kept the version that was on disk as {conflictCopy} before writing yours.
        </p>
      )}

      {body.gone ? (
        <p className="border-b px-3 py-1 text-[11px] text-muted-foreground">
          This note isn't on disk any more. Your text is still here and saving writes it back.
        </p>
      ) : null}

      {showProperties && mode === "edit" ? (
        <PropertiesPanel
          frontmatter={frontmatter}
          body={body.text}
          subscriptionId={subscriptionId}
          baseRev={rev}
          onSaved={adoptPanelWrite}
        />
      ) : null}

      {/* Hidden rather than unmounted: the caret, the selection and the undo
          stack all have to survive a trip through history or conflict mode. */}
      <div ref={hostRef} className={mode === "edit" ? "min-h-0 flex-1 overflow-auto" : "hidden"} />

      {mode === "history" ? (
        <div className="min-h-0 flex-1">
          <NoteHistoryPanel vaultId={vaultId} noteId={noteId} onBack={leaveMode} />
        </div>
      ) : null}

      {mode === "conflict" && conflictTheirs !== null ? (
        <div className="min-h-0 flex-1">
          <ConflictResolver
            vaultId={vaultId}
            noteId={noteId}
            mine={body.text}
            theirs={conflictTheirs}
            onResolved={leaveMode}
            onAbandon={leaveMode}
          />
        </div>
      ) : null}

      {mode === "edit" ? (
        <BacklinksPanel
          vaultId={vaultId}
          noteId={noteId}
          onOpen={(linked) => onOpenNote?.(linked)}
        />
      ) : null}
    </div>
  );
}

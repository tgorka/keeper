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
import { ExportNoteItem } from "@/components/export/export-note-item";
import { PaneHeader } from "@/components/layout/pane-header";
import { DropdownMenuItem, DropdownMenuSeparator } from "@/components/ui/dropdown-menu";
import { useNotesBody } from "@/hooks/use-notes-body";
import { type NoteWriteVm, notesGallery, notesTagTree } from "@/lib/ipc/client";
import { followExternalUrl, resolveWikilink } from "@/lib/notes/follow-link";
import { markSaved, readNoteDocument, useNoteDocument } from "@/lib/stores/notes-editor";
import { ensureNotesVaultsHydrated, useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { filePathForNote, SHOW_IN_FILES_LABEL, showNoteInFiles } from "@/lib/vault-link";
import { AttachFileButton } from "./attach-file-button";
import { ATTACHMENTS_LABEL, AttachmentsPanel } from "./attachments-panel";
import { BacklinksPanel } from "./backlinks-panel";
import { ConflictResolver } from "./conflict-resolver";
import type { FormatAction } from "./editor/format-commands";
import { FormatToolbar } from "./format-toolbar";
import { NoteActions } from "./note-actions";
import { NoteDiffBar } from "./note-diff-bar";
import { NOTE_HISTORY_LABEL, NoteHistoryPanel } from "./note-history-panel";
import {
  PROPERTIES_LABEL,
  PropertiesPanel,
  readFrontmatter,
  recordingSessionId,
} from "./properties-panel";
import { TemplateUpdateOffer } from "./template-update-offer";

/**
 * Test id for the sentence a link that went nowhere leaves behind (Story
 * 45.18). A slot rather than `role="status"` alone, because 45.13's attach
 * receipt is also a status and a test has to read one without matching the
 * other.
 */
export const LINK_NOTICE_SLOT = "note-link-notice";

/**
 * Story 46.4's three-group header row, and where it lives now.
 *
 * The row was extracted into {@link "@/components/layout/pane-header"} by Story
 * 46.13 on AD-104's rule of two, once the Files pane's Save caption became a
 * second real consumer of the same variable-width status element. The slot ids
 * a test scopes itself to are that module's `PANE_HEADER_*` constants; this
 * editor no longer owns names for them, because two headers answering to two
 * sets of names is exactly how a shared structure comes apart.
 */

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
  /**
   * Write text where the caret is, as the user's own edit (Story 43.7).
   *
   * The counterpart of `applyExternal`, and deliberately its opposite in every
   * respect: this one IS the user's edit, so it goes into the undo history and
   * it is reported back through `onEdit`. An insertion annotated `remote` would
   * be unrecorded by the history and unreported to Rust — the attachment would
   * appear in the buffer and never reach the file.
   */
  insertAtCursor: (text: string) => void;
  /**
   * Run a formatting action over the current selection (Story 44.9).
   *
   * The toolbar hands over a description rather than a command because it is
   * in the main bundle and every `@codemirror/*` value has to stay inside the
   * boot closure below. Translating the description is therefore the closure's
   * job, and it is the only place that holds both the view and the commands.
   */
  runFormat: (action: FormatAction) => void;
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

/**
 * An arbitrary instant, and the same instant twelve hours later. In every
 * timezone one of the two falls before noon and the other after, so a locale
 * that appends a day period contributes BOTH of its renderings to the
 * measurement below, rather than whichever one this machine happened to save
 * in.
 */
const SIZER_INSTANT_MS = Date.UTC(2001, 0, 1, 0, 0);
const SIZER_HALF_DAY_MS = 12 * 60 * 60 * 1000;

/**
 * Every string the caption slot has to be wide enough to hold (Story 46.4).
 *
 * The slot is one fixed box, because a caption that changes width moves every
 * control to its right — that is the jump this story removes. The box cannot be
 * a width we guessed: `toLocaleTimeString` is a different length in every
 * locale, and a box sized for `Saved · 14:32` in `en-GB` is a truncation bug on
 * a machine neither we nor the owner has. So the browser measures it instead.
 * These strings are rendered invisibly inside the slot in every save state, and
 * the widest of them — in the font, the locale and the clock this machine
 * actually has — is what the slot is wide.
 *
 * Produced by `saveStateWord` rather than written out, so a change to the
 * wording cannot change what the caption shows without also changing what the
 * slot reserves. The digits need no entry of their own: the slot is
 * `tabular-nums`, so every digit is exactly as wide as every other one.
 *
 * The error string is deliberately absent, and it is the one caption that
 * cannot be reserved for — it is Rust's message verbatim, so it is unbounded.
 * It is ellipsised inside the slot instead, with the whole of it left in the
 * DOM for a screen reader and on the element's `title` for a pointer.
 */
export const SAVE_CAPTION_SIZERS: readonly string[] = [
  saveStateWord({ saving: true, dirty: false, savedAtMs: null, error: null }),
  saveStateWord({ saving: false, dirty: false, savedAtMs: SIZER_INSTANT_MS, error: null }),
  saveStateWord({
    saving: false,
    dirty: false,
    savedAtMs: SIZER_INSTANT_MS + SIZER_HALF_DAY_MS,
    error: null,
  }),
  // A locale that renders both instants identically would otherwise hand React
  // two children under one key. Deduplicated here rather than at the render
  // site so the exported list is the list that gets rendered.
].filter((word, at, all) => all.indexOf(word) === at);

export interface NoteEditorProps {
  vaultId: string;
  noteId: string | null;
  /** Open another note — a backlink row, or a wikilink that resolved. */
  onOpenNote?: (noteId: string) => void;
}

export function NoteEditor({ vaultId, noteId, onOpenNote }: NoteEditorProps) {
  const body = useNotesBody(vaultId, noteId);
  // Story 46.12: every one of these names the note THIS editor is showing.
  // Two editors are two subscriptions to two documents in one store, and the
  // only thing that keeps them apart is that the key is a prop rather than a
  // module-scoped "current".
  const base = useNoteDocument(vaultId, noteId, (document) => document.base);
  const rev = useNoteDocument(vaultId, noteId, (document) => document.rev);
  const frontmatter = useNoteDocument(vaultId, noteId, (document) => document.frontmatter);
  const path = useNoteDocument(vaultId, noteId, (document) => document.path);
  const subscriptionId = useNoteDocument(vaultId, noteId, (document) => document.subscriptionId);
  const savedAtMs = useNoteDocument(vaultId, noteId, (document) => document.savedAtMs);
  const conflictCopy = useNoteDocument(vaultId, noteId, (document) => document.conflictCopy);
  const error = useNoteDocument(vaultId, noteId, (document) => document.error);
  // Story 45.18: the vault this note is in, for the one question the header
  // asks that the note itself cannot answer — where its file sits inside the
  // sync profile. Hydrated here rather than assumed, because this editor is
  // also mounted by `PanelStrip`, and a session that never opened the Notes tab
  // has never read the vault list.
  const vault = useNotesVaultsStore(
    (state) => state.vaults?.find((each) => each.id === vaultId) ?? null,
  );
  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

  const hostRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<EditorRuntime | null>(null);
  const [mode, setMode] = useState<EditorMode>("edit");
  const [showProperties, setShowProperties] = useState(false);
  const [showAttachments, setShowAttachments] = useState(false);
  const [conflictTheirs, setConflictTheirs] = useState<string | null>(null);
  // Story 45.13's sentence about what attaching just did — what was copied in,
  // what was already there. Rendered as a banner beside the conflict-copy one
  // below, because both are "here is what keeper did to your file after you
  // asked for something else".
  const [attachOutcome, setAttachOutcome] = useState<string | null>(null);
  // Story 45.18's answer to a link that did not go anywhere: the note nobody
  // has written, the scheme keeper will not open, the window with no opener
  // grant. Shown rather than swallowed, because a click that produces silence
  // is exactly what this story was sent to remove.
  const [linkNotice, setLinkNotice] = useState<string | null>(null);

  const openHistory = useCallback(() => setMode("history"), []);
  const toggleProperties = useCallback(() => setShowProperties((shown) => !shown), []);
  const toggleAttachments = useCallback(() => setShowAttachments((shown) => !shown), []);
  // Through the ref, because the runtime is built by an async effect: a panel
  // pressed before the editor chunk lands writes nothing rather than throwing,
  // and the same identity survives every re-render of the panel below.
  const insertAtCursor = useCallback((text: string) => {
    runtimeRef.current?.insertAtCursor(text);
  }, []);
  const runFormat = useCallback((action: FormatAction) => {
    runtimeRef.current?.runFormat(action);
  }, []);

  // Refs, not effect dependencies: rebuilding the editor because a callback
  // identity changed would throw away the document, the undo stack and the
  // caret. Every one of these is read at the moment it fires.
  const latest = useRef({ onEdit: body.onEdit, save: body.save, openHistory, toggleProperties });
  latest.current = { onEdit: body.onEdit, save: body.save, openHistory, toggleProperties };
  const pathRef = useRef(path);
  pathRef.current = path;
  // Story 45.18: following, at last. `onFollowLink` used to sit here, a prop no
  // caller ever passed — so a wikilink click reached a `?.()` on `undefined`
  // and did nothing, for five stories, under a `cursor: pointer`. Resolution is
  // the index's own (`notes_resolve_link`), and the note it names is opened
  // through the same `onOpenNote` a backlink row uses, so a link and a backlink
  // cannot land in different places.
  const followers = useRef({ onOpenNote, setLinkNotice });
  followers.current = { onOpenNote, setLinkNotice };
  const openWikilink = useCallback(
    (target: string) => {
      void resolveWikilink(vaultId, target).then((result) => {
        followers.current.setLinkNotice(result.reason);
        if (result.note !== null) {
          followers.current.onOpenNote?.(result.note.id);
        }
      });
    },
    [vaultId],
  );
  const openExternal = useCallback((url: string) => {
    void followExternalUrl(url).then((refusal) => {
      followers.current.setLinkNotice(refusal);
    });
  }, []);
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
      const [
        state,
        view,
        commands,
        markdown,
        autocomplete,
        preview,
        wikilink,
        tags,
        slash,
        indent,
        emoji,
        format,
      ] = await Promise.all([
        import("@codemirror/state"),
        import("@codemirror/view"),
        import("@codemirror/commands"),
        import("@codemirror/lang-markdown"),
        import("@codemirror/autocomplete"),
        import("./editor/live-preview"),
        import("./editor/wikilink"),
        import("./editor/tag-complete"),
        import("./editor/slash-menu"),
        import("./editor/indent-keymap"),
        import("./editor/emoji-complete"),
        import("./editor/format-commands"),
      ]);
      if (disposed) {
        return;
      }

      let cachedTags: string[] | null = null;
      // Story 46.12: read once, by note. Three reads of a module singleton
      // would have been three reads of whatever note the store was pointing at
      // when this chunk happened to land — which, with a second editor beside
      // this one, is not necessarily this one.
      const opened = readNoteDocument(vaultId, noteId);
      const editorView = new view.EditorView({
        parent: host,
        state: state.EditorState.create({
          // Read imperatively: whatever the channel has delivered by the time
          // the chunk lands. Later revisions arrive through the reconcile
          // effect below rather than by rebuilding the editor.
          doc: opened.text,
          // A template's `{{cursor}}`, clamped to the document. Absent a hint the
          // caret goes to the end, which is where a person continuing a note wants
          // it — and offset 0 is now simply the first character of the body, since
          // the frontmatter block is not in this buffer at all.
          selection: {
            anchor: Math.max(0, Math.min(opened.cursor ?? opened.text.length, opened.text.length)),
          },
          extensions: [
            view.EditorView.lineWrapping,
            // CodeMirror gives its content `role="textbox"` and no accessible
            // name, so the editable region announced itself as an unlabelled
            // text box on every surface that mounts this editor.
            //
            // Found by Story 45.14, and found only because that story DELETED
            // something: the quick-capture panel's textarea carried
            // `aria-label="Quick capture"`, and porting the panel to this
            // editor dropped a promise that was written down nowhere except as
            // an attribute on code that no longer exists. Named here rather
            // than in the capture window, because a capture-only label would
            // be a second configuration of one editor — the thing AD-93 exists
            // to prevent — and because the notes pane had the same gap.
            view.EditorView.contentAttributes.of({ "aria-label": "Note" }),
            commands.history(),
            view.keymap.of([
              ...commands.defaultKeymap,
              ...commands.historyKeymap,
              ...autocomplete.completionKeymap,
              ...indent.indentBindings,
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
                emoji.emojiCompleteSource(),
              ],
            }),
            // The other half of Story 45.11: a shortcode somebody typed in full
            // becomes its character as the closing colon lands, so `:tada:`
            // never has to be recognised as a menu interaction.
            emoji.emojiShortcodeCommit(),
            preview.livePreview({
              vaultId,
              assetUrl: (rel) =>
                `keeper-note://${vaultId}/${vaultRelative(pathRef.current, rel)
                  .split("/")
                  .map(encodeURIComponent)
                  .join("/")}`,
              onOpenLink: openWikilink,
              onOpenUrl: openExternal,
              recordingSession: () => sessionRef.current,
              // Handed the block's own folder text and nothing else: Rust
              // resolves it against the vault root, so no path is composed
              // here (AD-65).
              listFolder: (folder) => notesGallery(vaultId, folder),
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
        insertAtCursor: (text: string) => {
          // `replaceSelection` writes at every cursor and leaves the caret
          // after what it wrote, which is where a person who had typed those
          // characters would be. Plain and unannotated on purpose: this is the
          // user's edit, so the update listener reports it and ⌘Z undoes it.
          editorView.dispatch(editorView.state.replaceSelection(text));
          // The click that got here took focus out of the editor. Handing it
          // back is the difference between inserting and interrupting.
          editorView.focus();
        },
        // Unannotated, like `insertAtCursor` and for the same reason: a
        // formatting action IS the user's edit, so it belongs in the undo
        // history and it has to reach Rust through the update listener.
        runFormat: (action: FormatAction) => {
          format.formatCommand(action)(editorView);
        },
        focus: () => editorView.focus(),
        destroy: () => editorView.destroy(),
      };
      editorView.focus();
      // The document almost never exists yet when this chunk lands — the channel
      // delivers `Reset` after the lazy import resolves — so the caret hint is
      // consumed here, once the runtime is able to act on it.
      const opening = readNoteDocument(vaultId, noteId).cursor;
      if (opening !== null) {
        runtimeRef.current.placeCaret(opening);
      }
    })();

    return () => {
      disposed = true;
      runtimeRef.current?.destroy();
      runtimeRef.current = null;
    };
    // Both handlers are stable — `openExternal` has no deps and `openWikilink`
    // has only `vaultId`, which is already here — so naming them costs no extra
    // teardown of the editor and keeps the effect honest about what it closes over.
  }, [vaultId, noteId, openExternal, openWikilink]);

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

  const adoptPanelWrite = useCallback(
    (text: string, write: NoteWriteVm) => {
      if (noteId === null) {
        return;
      }
      // A property edit goes straight to disk, and it writes the buffer with it, so
      // its result is the editor's new base and the block Rust hands back — `updated`
      // stamped — is the block the panel now renders.
      markSaved(vaultId, noteId, text, write);
    },
    [vaultId, noteId],
  );

  const leaveMode = useCallback(() => {
    setConflictTheirs(null);
    setMode("edit");
  }, []);

  // Story 46.4: read once, because the header both shows this word and hangs it
  // on a `title` — an error is the one caption that can outgrow its slot.
  const saveWord = saveStateWord({ saving: body.saving, dirty: body.dirty, savedAtMs, error });

  if (noteId === null) {
    return (
      <div className="flex h-full items-center justify-center px-6 text-sm text-muted-foreground">
        Pick a note, or write a new one.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Story 46.4's three groups, Story 46.13's component (AD-104).

          The row is `flex` and does not wrap, so every sibling in it competes
          for the same width: a caption that grows by the width of a clock takes
          that width from whatever else is shrinkable, and in the capture window
          (`keeper_core::capture::CAPTURE_DEFAULT_SIZE`, 560px, and since story
          46.15 resizable by the user down to `CAPTURE_MIN_SIZE`) there is
          nothing spare to take it from. That is why a save moved the toolbar,
          and it is also why the toolbar moved once more per note open, when
          `Show in Files` resolved and appeared. The structural answer — and the
          group may change width — is documented on `PaneHeader` itself, so the
          note editor and the Files pane's Save bar cannot come to disagree about
          it. The capture window mounts this exact header, so one structure
          answers both hosts. */}
      <PaneHeader
        className="border-b px-3 py-1.5"
        // Group 1 — identity. The only member of the row allowed to give
        // ground; see the component's doc for why that is a property of the
        // wrapper and not of these two elements.
        identity={
          <>
            <h1 className="min-w-0 flex-1 truncate font-medium text-sm">
              {deriveTitle(body.text)}
            </h1>
            <span className="truncate font-mono text-[11px] text-muted-foreground">
              {path ?? ""}
            </span>
          </>
        }
        // Group 2 — status. One box for all three captions, reserved from the
        // strings this machine's own clock produces, so a save cannot widen it.
        status={{ sizers: SAVE_CAPTION_SIZERS, caption: saveWord }}
        // Group 3 — actions. Two controls, which is AD-104's rule of two for
        // this row: the one verb that starts outside keeper, and the menu that
        // holds the note's own verbs. What belongs in here is Story 46.5's
        // decision, not the wrapper's.
        actions={
          <>
            {/* Story 45.13. Beside Attachments rather than inside it: the panel
                lists what THIS note already has, and this brings in what it does
                not — including for a note that has no `files:` key and therefore
                no panel worth opening. */}
            <AttachFileButton
              vaultId={vaultId}
              body={body.text}
              onInsert={insertAtCursor}
              onOutcome={setAttachOutcome}
            />
            {/* Story 46.5. Everything in this menu opens a panel, a surface or a
                dialog; none of them is a per-keystroke verb, and six controls
                plus two spans do not fit the 560px quick-capture window. So they
                are menu items, the header keeps the one control that starts
                outside keeper (`AttachFileButton`, above), and the menu becomes
                the note's one obvious home for its own verbs — which is what
                makes Delete findable, structurally rather than by relabelling it.

                This reverses 45.17's "everything to the left changes what this
                pane SHOWS, and the menu acts on the note itself" and 45.18's
                "burying a one-press navigation in a dropdown is a regression".
                Both were right about the taxonomy and wrong about the budget:
                the row does not wrap, this cluster is its last child, and the
                cost of keeping them out here was paid by the item at the end.
                Order is preserved — panel, panel, surface, navigation — then a
                rule, then the note-level acts. */}
            <NoteActions vaultId={vaultId} noteId={noteId} title={deriveTitle(body.text)}>
              <DropdownMenuItem onSelect={toggleAttachments}>{ATTACHMENTS_LABEL}</DropdownMenuItem>
              <DropdownMenuItem onSelect={toggleProperties}>{PROPERTIES_LABEL}</DropdownMenuItem>
              <DropdownMenuItem onSelect={openHistory}>{NOTE_HISTORY_LABEL}</DropdownMenuItem>
              {/* Story 45.18: from a note, its file (FR-196, UX-DR79).

                  Absent rather than disabled when there is nothing to show — the
                  vault list has not arrived, the note has no path yet, or the
                  profile carries no vault subfolder. `filePathForNote` answers
                  "may this be offered" and `showNoteInFiles` answers "do it";
                  the same pure rule twice, deliberately, because a control whose
                  presence and whose effect came from different rules is a
                  control that can be present and fail.

                  Inside the menu since 46.5, which makes its absence harder to
                  assert honestly: a menu item that is not there because the menu
                  is shut looks exactly like one the predicate refused. The three
                  tests in `note-file-links.test.tsx` that read this absence open
                  the menu first, for that reason. */}
              {vault !== null && path !== null && filePathForNote(vault, path) !== null && (
                <DropdownMenuItem
                  onSelect={() => {
                    showNoteInFiles(vault, path);
                  }}
                >
                  {SHOW_IN_FILES_LABEL}
                </DropdownMenuItem>
              )}
              {/* Story 45.21: Export is a note-level act, and putting it here
                  rather than on the panel frame means a note open in a panel has
                  one Export and not two — the panel's could not flush this
                  buffer before Rust reads the file. The rule above it separates
                  what acts on the note from what only shows it; `NoteActions`
                  draws the second rule, above Delete. */}
              <DropdownMenuSeparator />
              <ExportNoteItem vaultId={vaultId} noteId={noteId} />
            </NoteActions>
          </>
        }
      />

      <NoteDiffBar
        vaultId={vaultId}
        noteId={noteId}
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

      {/* Story 45.13's receipt: which files were copied into the vault, and
          which were already in this note. `role="status"` because it answers
          something the person just did and they may not be looking here. */}
      {attachOutcome === null ? null : (
        <p role="status" className="border-b px-3 py-1 text-[11px] text-muted-foreground">
          {attachOutcome}
        </p>
      )}

      {/* Story 45.18's answer when a link went nowhere: the note nobody has
          written, the scheme keeper will not hand to the OS, the window with no
          opener grant. A slot as well as `role="status"`, because the receipt
          above is also a status and a test must be able to read one without
          matching the other. */}
      {linkNotice === null ? null : (
        <p
          role="status"
          data-slot={LINK_NOTICE_SLOT}
          className="border-b px-3 py-1 text-[11px] text-muted-foreground"
        >
          {linkNotice}
        </p>
      )}

      {body.gone ? (
        <p className="border-b px-3 py-1 text-[11px] text-muted-foreground">
          This note isn't on disk any more. Your text is still here and saving writes it back.
        </p>
      ) : null}

      {/* Only in edit mode, and below the honest-state banners: the offer is a
          consequence of editing THIS note, and it has no meaning while the pane
          is showing history or a conflict. It renders nothing at all unless the
          note is a template whose text changed in this session. */}
      {mode === "edit" ? <TemplateUpdateOffer vaultId={vaultId} noteId={noteId} rev={rev} /> : null}

      {showProperties && mode === "edit" ? (
        <PropertiesPanel
          frontmatter={frontmatter}
          body={body.text}
          subscriptionId={subscriptionId}
          baseRev={rev}
          onSaved={adoptPanelWrite}
        />
      ) : null}

      {/* Below the properties, because that is the order of the question it
          answers: the block says which files the note has, and this puts one
          of them in the body. Unmounted rather than hidden — unlike the editor
          it holds no state worth keeping, and its `files:` reading is a
          function of the props it is given each time. */}
      {showAttachments && mode === "edit" ? (
        <AttachmentsPanel
          vaultId={vaultId}
          frontmatter={frontmatter}
          body={body.text}
          onInsert={insertAtCursor}
        />
      ) : null}

      {/* Directly over the text it formats, and unmounted outside edit mode:
          in history or conflict mode there is no selection for a button to act
          on, and a toolbar that cannot do anything is a toolbar that lies. */}
      {mode === "edit" ? <FormatToolbar onAction={runFormat} /> : null}

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

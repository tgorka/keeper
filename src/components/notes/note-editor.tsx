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
import { Files, FolderSearch, History, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { CaptureNoteItem } from "@/components/capture/capture-note-item";
import { ExportNoteItem } from "@/components/export/export-note-item";
import { PaneHeader } from "@/components/layout/pane-header";
import { type PriorityAction, PriorityActions } from "@/components/layout/priority-actions";
import { Button } from "@/components/ui/button";
import {
  DropdownMenuCheckboxItem,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useNotesBody } from "@/hooks/use-notes-body";
import { type NoteWriteVm, notesGallery, notesRename, notesTagTree } from "@/lib/ipc/client";
import { followExternalUrl, resolveWikilink } from "@/lib/notes/follow-link";
import { markSaved, readNoteDocument, useNoteDocument } from "@/lib/stores/notes-editor";
import { ensureNotesVaultsHydrated, useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { filePathForNote, SHOW_IN_FILES_LABEL, showNoteInFiles } from "@/lib/vault-link";
import { AttachFileButton } from "./attach-file-button";
import { ATTACHMENTS_LABEL, AttachmentsPanel } from "./attachments-panel";
import { ConflictResolver } from "./conflict-resolver";
import type { FormatAction } from "./editor/format-commands";
import { FormatToolbar } from "./format-toolbar";
import { LinksPanel } from "./links-panel";
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

/** Test id for the sentence a panel leaves behind when the pane's mode is
 *  wrong for it (Story 48.5). */
export const PANEL_UNAVAILABLE_SLOT = "note-panel-unavailable";

/** The way back to the mode where the panel exists. */
export const PANEL_BACK_LABEL = "Back to the note";

/**
 * Why a panel the user just asked for is not on screen — or null, when it is
 * (Story 48.5).
 *
 * Both panels used to be `showX && mode === "edit"`, so a person who pressed
 * Properties while reading history got nothing at all: no panel, no sentence,
 * no hint that the mode was the reason. That is half of a 0.8.1 report about
 * tags on a recording note, which work, behind Properties, which does not open
 * in two of the pane's three modes. The other half was that Properties was a
 * menu item nobody found, which the header above now fixes; a control that is
 * findable and then silently does nothing would be the same report again with
 * a shorter path to it.
 *
 * Explained rather than rendered read-only, deliberately. `PropertiesPanel`
 * takes a null `subscriptionId` to mean "editing disabled", and what that does
 * is drop the write on the floor: the fields stay enabled, the chips stay
 * removable, and nothing says why the edit did not take. Between a panel that
 * lies about being editable and a sentence that says when it will be, the
 * sentence is the honest one.
 */
export function panelUnavailableReason(panel: string, mode: EditorMode): string | null {
  if (mode === "edit") {
    return null;
  }
  if (mode === "history") {
    return `${panel} belong to the note as it is now, and you are reading an older version of it. Go back to the note to see and change them.`;
  }
  return `${panel} are unavailable while you resolve this conflict — two versions of this note carry two sets of them. Resolve it, or abandon it, and they come back.`;
}

/**
 * The sentence, and — in history only — the press that resolves it.
 *
 * No way out is offered from a conflict. `leaveMode` abandons the resolution
 * as well as returning to the note, and a control called "Back to the note"
 * that quietly threw away a merge in progress would be a far worse defect than
 * the silence this replaces. The resolver draws its own two exits, both named
 * for what they do.
 */
function PanelUnavailable({
  panel,
  mode,
  onBack,
}: {
  panel: string;
  mode: EditorMode;
  onBack: () => void;
}) {
  const reason = panelUnavailableReason(panel, mode);
  if (reason === null) {
    return null;
  }
  return (
    <div
      data-slot={PANEL_UNAVAILABLE_SLOT}
      className="flex items-center gap-2 border-b px-3 py-1 text-meta text-muted-foreground"
    >
      <span className="min-w-0 flex-1">{reason}</span>
      {mode === "history" ? (
        <Button size="sm" variant="ghost" className="shrink-0" onClick={onBack}>
          {PANEL_BACK_LABEL}
        </Button>
      ) : null}
    </div>
  );
}

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
 * slot reserves. The digits need no entry of their own: the slot carries
 * `figures`, so every digit is exactly as wide as every other one.
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
  /**
   * The controls of the surface that HOLDS this editor — a panel's fold and
   * close — handed straight to the header's fourth group (Story 50.1).
   *
   * A note open in a panel had two header rows: the panel's, whose whole
   * content was the word `Note` and these two controls, and this one. The word
   * said nothing the note's own title does not say better and the band cost
   * 40px and a seam, so the panel gives up its row and passes its controls
   * down. Absent in the notes pane, in a capture window and in the prewarmed
   * draft, none of which is a frame — and the header renders no fourth group
   * at all when it is, so nothing reserves space for a group that is not there.
   *
   * The editor never composes these, only places them: what a panel's fold
   * looks like is `panel-strip.tsx`'s business, and a second spelling of it
   * here is a second glyph to keep in step.
   */
  frame?: ReactNode;
}

/**
 * The open note's column.
 *
 * `min-w-0` is defensive, and this comment exists to say so honestly: it is not
 * what stopped the note escaping its pane. It was shipped believing it was, on
 * a measurement of a container chain the app does not actually have — the
 * column's parent is a block box (`overflow-auto`), not a flex container, so
 * `min-width: auto` never applied to this element and setting it to zero
 * changed nothing. Measured again against the real chain, with and without the
 * class: 604px either way.
 *
 * It stays because the day this column becomes a flex item the floor is already
 * written, and it costs nothing. What actually fits the note to its pane is the
 * `ResizeObserver` below — see the comment beside it.
 */
export const NOTE_COLUMN_CLASS = "flex h-full min-h-0 min-w-0 flex-col";

export function NoteEditor({ vaultId, noteId, onOpenNote, frame }: NoteEditorProps) {
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
  // The two disclosure regions the header's Properties and Attachments controls
  // name. Ids rather than adjacency, because the controls are in the header and
  // the regions are several strips further down: nothing about where they sit
  // says which control opened them, so `aria-controls` has to.
  const propertiesRegionId = useId();
  const attachmentsRegionId = useId();
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
        cmSearch,
        preview,
        wikilink,
        tags,
        indent,
        writing,
        find,
        marks,
      ] = await Promise.all([
        import("@codemirror/state"),
        import("@codemirror/view"),
        import("@codemirror/commands"),
        import("@codemirror/lang-markdown"),
        import("@codemirror/autocomplete"),
        import("@codemirror/search"),
        import("./editor/live-preview"),
        import("./editor/wikilink"),
        import("./editor/tag-complete"),
        import("./editor/indent-keymap"),
        // The slash menu, emoji and the toolbar's translation, which this editor
        // no longer owns: Story 50.3 moved them into one module a file viewer
        // imports too, so a session log gets the same three tools rather than a
        // second copy of them.
        import("./editor/writing-tools"),
        // The find bar. In the closure with everything else so the search
        // panel's React tree is in the editor chunk, not the main bundle.
        import("./editor/find-panel"),
        import("./editor/markdown-marks"),
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
            // In-document find (FR-267). `⌘F` is claimed app-wide by the
            // shortcut hook, which stands down when an editor has focus — so
            // this panel gets the chord only while the caret is in it, and the
            // notes pane's own `⌘F` (focus the filter field) still fires from
            // everywhere else. CodeMirror's own keymap is what receives it,
            // rather than a synthesised event, because the panel it opens is
            // CodeMirror's and only its own commands can drive it.
            find.findBar(),
            cmSearch.highlightSelectionMatches(),
            view.keymap.of([
              ...commands.defaultKeymap,
              ...commands.historyKeymap,
              ...autocomplete.completionKeymap,
              ...cmSearch.searchKeymap,
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
            markdown.markdown({
              base: markdown.markdownLanguage,
              // `==highlight==`, which no base grammar defines. The same list
              // is passed by `markdown-preview.ts`, because a note and the
              // same bytes opened from Files must not render differently.
              extensions: [...marks.MARKDOWN_MARKS],
            }),
            writing.markdownWritingTools([
              wikilink.wikilinkSource(vaultId),
              tags.tagCompleteSource(async () => {
                cachedTags ??= tags.tagPaths((await notesTagTree(vaultId)).nodes);
                return cachedTags;
              }),
            ]),
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

      // This pane changes width without the window changing size — a column
      // folds, the strip re-divides between panels — and CodeMirror measures
      // its own width when it is created and when the window resizes, not when
      // the box it lives in is re-divided around it.
      //
      // Measured in the running app: the window was dragged 158px narrower and
      // the three columns beside this pane did not move a pixel, so the whole
      // change landed here. The text did not re-wrap, and there was no
      // horizontal scrollbar to reach the rest with. Every line kept the width
      // the pane used to have, and the pane clipped it — which is what "the
      // note does not fit the window" turned out to be.
      //
      // Guarded because jsdom does not always define it, in the shape the other
      // panes in this app already use.
      const resizes =
        typeof ResizeObserver === "undefined"
          ? null
          : new ResizeObserver(() => {
              editorView.requestMeasure();
            });
      resizes?.observe(host);

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
          writing.runFormatAction(editorView, action);
        },
        focus: () => editorView.focus(),
        destroy: () => {
          resizes?.disconnect();
          editorView.destroy();
        },
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

  /**
   * A retitle renames the note's file (FR-97), which until now nothing asked for.
   *
   * `notes_rename` has been built, registered and wrapped since FR-97 and had
   * **no call site anywhere in `src/`** — so every note in every vault has been
   * carrying whatever filename it was created with, however many times its title
   * changed. This is the call site.
   *
   * **Safe because the id is the identity, and the subscription proves it.**
   * `notes_open` follows its note by ULID and answers a moved file with
   * `NoteBodyBatch::Renamed`, the panel target is `{kind:"note", noteId}`, and
   * links, pins and unread marks all resolve through the same id. So the rename
   * needs no pointer rewriting and the editor does not even reload — which is the
   * opposite of a session file, whose path *is* its identity and whose rename is
   * therefore one journaled plan with a link rewrite in it.
   *
   * `null` while the pane has no note: there is nothing to rename, and the panel
   * treats that as "this address has no rename" rather than calling with an empty
   * id.
   */
  const renameNoteFile = useCallback(
    async (title: string) => {
      if (noteId === null) {
        return;
      }
      await notesRename(vaultId, noteId, title);
    },
    [vaultId, noteId],
  );

  const leaveMode = useCallback(() => {
    setConflictTheirs(null);
    setMode("edit");
  }, []);

  /**
   * Story 48.5's priority order: what the header shows first when it has the
   * room, and what it gives back to the ⋯ menu first when it does not.
   *
   * The order is a product decision and this is the argument for it. Against
   * 0.8.1 the owner reported three things as missing that all shipped and all
   * live in that menu — "I still see no way to delete notes", "I don't see
   * attachments", and editing tags on a recording note, which works and lives
   * behind Properties. Two of the three are the first two here, and the third
   * is Delete, which is never promoted at all: a destructive verb does not
   * belong in a toolbar (46.5's ruling, unchanged) and it is findable because
   * the menu is now the SHORT list rather than the list of everything.
   *
   * History and Show in Files come after because both are journeys away from
   * the note rather than things about it — and Show in Files is last of the
   * four because it is the only one with a second home: the Files pane can
   * reach the same file without this header at all.
   *
   * `Show in Files` is absent rather than disabled when there is nothing to
   * show, exactly as 45.18 left it — the vault list has not arrived, the note
   * has no path yet, or the profile carries no vault subfolder.
   * `filePathForNote` answers "may this be offered" and `showNoteInFiles`
   * answers "do it": the same pure rule twice, because a control whose presence
   * and whose effect came from different rules is one that can be present and
   * fail. Arriving late is why the group is keyed by id — the width of an item
   * that appears an instant after the vault list is measured when it appears.
   *
   * # The glyphs (Story 48.9)
   *
   * Each is picked to be unclaimed elsewhere in this app, because two controls
   * drawn the same are two controls a hand learns as one. `Files` is a stack of
   * documents — what this note HAS — against the paperclip beside it, which is
   * how one more gets in. `SlidersHorizontal` is the note's settable fields and
   * deliberately not the gear, which the nav already spends on the app's own
   * settings. `History` is the clock that runs backwards. `FolderSearch` is a
   * folder being looked into, which is literally the act: reveal this file
   * where it lies. None of the four appears in `sidebar-pane.tsx`'s nav or in
   * `format-toolbar.tsx`'s marks.
   */
  const headerActions = useMemo<readonly PriorityAction[]>(() => {
    // Story 49: the first two are the only members of this row that are not
    // verbs. They open a panel and press again to close it, and until now they
    // said so to nobody — no state on the control, in either direction, so the
    // only way to learn whether Properties was already open was to look at the
    // pane and recognise the panel. `expanded` is the whole fix; the `ghost`
    // variant paints it and `aria-controls` names what opened. `history` and
    // `show-in-files` carry neither, because neither discloses anything: one
    // replaces the pane and the other leaves the app.
    const acts: PriorityAction[] = [
      {
        id: "attachments",
        label: ATTACHMENTS_LABEL,
        icon: Files,
        onSelect: toggleAttachments,
        expanded: showAttachments,
        controls: showAttachments ? attachmentsRegionId : undefined,
      },
      { id: "history", label: NOTE_HISTORY_LABEL, icon: History, onSelect: openHistory },
    ];
    if (vault !== null && path !== null && filePathForNote(vault, path) !== null) {
      acts.push({
        id: "show-in-files",
        label: SHOW_IN_FILES_LABEL,
        icon: FolderSearch,
        onSelect: () => showNoteInFiles(vault, path),
      });
    }
    return acts;
  }, [vault, path, toggleAttachments, openHistory, showAttachments, attachmentsRegionId]);

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
    <div className={NOTE_COLUMN_CLASS}>
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
        // `PaneHeader` owns its own height (DESIGN.md's 40px pane-header) and
        // its own bottom edge. The only thing left for a host to say is the
        // horizontal gutter: a `border-b` here drew the seam twice at 2px, and
        // a `py-1` here set the height a second time from a different fact.
        className="px-3"
        // Group 1 — identity. The only member of the ROW allowed to give
        // ground; see the component's doc for why that is a property of the
        // wrapper and not of these two elements.
        //
        // Inside the group, the order in which they give it is this file's,
        // and Story 50.1 reverses it. `flex-1` off a zero basis used to be on
        // the TITLE, so the path — which is as long as a folder tree makes it
        // — took its natural width first and the title grew into whatever was
        // left. Measured in Chromium at a 560px pane, that left the title
        // **0.47px**: the row showed `journal/2026/2026-08-08.md` in full and
        // the note's own name as an ellipsis. Merging the panel's row into
        // this one costs 40–80px more, so the shape that was already wrong at
        // 560 would have been wrong at 640.
        //
        // So the basis-zero member is the PATH now. The title takes its
        // natural width, the path absorbs the slack and is the first thing
        // ellipsised, and a squeezed row says which note before it says where
        // the note lives — which is the same ruling `PaneHeader` makes one
        // level up about identity against the controls.
        identity={
          <>
            <h1 className="min-w-0 truncate font-heading text-title">{deriveTitle(body.text)}</h1>
            <span className="min-w-0 flex-1 truncate font-mono text-meta text-muted-foreground">
              {path ?? ""}
            </span>
          </>
        }
        // Group 2 — status. One box for all three captions, reserved from the
        // strings this machine's own clock produces, so a save cannot widen it.
        status={{ sizers: SAVE_CAPTION_SIZERS, caption: saveWord }}
        // Group 3 — actions, and how many of them there are (Story 48.5). The
        // function form is handed the pixels this row can spare; `budget` is
        // zero until a `ResizeObserver` has answered, and zero renders exactly
        // the shape 46.5 shipped, so the worst a machine with no observer can
        // do is leave the header as it was. What belongs in the group is still
        // 46.5's decision; how much of it is on screen is now the window's.
        actions={(budget) => (
          <PriorityActions
            budget={budget}
            items={headerActions}
            // Story 45.13, and never in the menu. Beside Attachments rather
            // than inside it: the panel lists what THIS note already has, and
            // this brings in what it does not — including for a note with no
            // `files:` key and therefore no panel worth opening. It is also the
            // one verb here that starts outside keeper, which is why 46.5 kept
            // it out when it put everything else away.
            leading={
              <>
                <AttachFileButton
                  vaultId={vaultId}
                  body={body.text}
                  onInsert={insertAtCursor}
                  onOutcome={setAttachOutcome}
                />
                {/* Leading rather than a candidate, which is the difference
                    between "shown when there is room" and "shown". It discloses
                    the region holding three of the four questions anybody asks
                    of an open note — its properties, what links here, what it
                    links to — and a disclosure control that disappears into a
                    menu at narrow widths takes the region with it: there is
                    nothing on screen left to say the region exists. The verbs
                    below still demote, because a verb in a menu is still a verb
                    you can reach by name. */}
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  aria-label={PROPERTIES_LABEL}
                  title={PROPERTIES_LABEL}
                  aria-expanded={showProperties}
                  aria-controls={showProperties ? propertiesRegionId : undefined}
                  onClick={toggleProperties}
                >
                  <SlidersHorizontal aria-hidden="true" className="size-4" />
                </Button>
              </>
            }
            // The menu is the row's overflow in the row's own order, and then
            // the verbs that never leave it. One rule, so nothing has to reason
            // about where an item lands when the list above it changes length:
            // what came back comes back in priority order, and what was always
            // here is always below it. `NoteActions` draws the last rule and
            // puts Delete under it.
            menu={(inMenu) => (
              <NoteActions vaultId={vaultId} noteId={noteId} title={deriveTitle(body.text)}>
                {headerActions.map((act) =>
                  inMenu(act.id) ? (
                    // A candidate that discloses a panel is a `menuitemcheckbox`
                    // down here, not a `menuitem`: the promoted control says
                    // "expanded" and the demoted one has to say the same fact in
                    // the vocabulary a menu has for it, or the state disappears
                    // at exactly the widths where the control did.
                    act.expanded === undefined ? (
                      <DropdownMenuItem key={act.id} onSelect={act.onSelect}>
                        {act.label}
                      </DropdownMenuItem>
                    ) : (
                      <DropdownMenuCheckboxItem
                        key={act.id}
                        checked={act.expanded}
                        onCheckedChange={act.onSelect}
                      >
                        {act.label}
                      </DropdownMenuCheckboxItem>
                    )
                  ) : null,
                )}
                {/* Story 45.15's second door, mounted at last (Story 48.3,
                    FR-191).

                    This one line is the whole of the owner's "nie mozna miec
                    wiecej niz jednej notatki" and "nie widze mozliwosci
                    otworzenia istniejacych notatek jak quick capture". 45.15
                    built the component, `openNoteAsCapture`,
                    `notes_capture_open` and `notes_window::open`, and rendered
                    the component nowhere: its only importer in the repository
                    was its own test. So the only capture window obtainable was
                    the prewarmed draft, which is one window holding one note —
                    two reports, one absent child.

                    ABOVE the separator, and not "beside Export" as 45.15's own
                    note proposed. The rule below separates what ACTS on the
                    note from what only SHOWS it, and a capture window only
                    shows it — it is a way of looking at a note, not a thing
                    done to one. Last among the things that show it rather than
                    between History and Show in Files, which is where 48.3 put
                    it: the four above are now a list whose length the window
                    decides, and an item interleaved into it would need a second
                    ordering rule for the widths at which its neighbours are not
                    there.

                    Offered in every host that mounts this header, including a
                    capture window's own document, and that is deliberate rather
                    than unconsidered. `notes_window::open` is idempotent by
                    identity — one label per note — so from a note's capture
                    window the verb raises the window you are in, and from the
                    prewarmed draft panel it promotes the quick note into a
                    window of its own that Escape does not hide. Both are true
                    readings of one label, which is why there is no second label
                    and no host test here. */}
                <CaptureNoteItem vaultId={vaultId} noteId={noteId} />
                {/* Story 45.21: Export is a note-level act, and putting it here
                    rather than on the panel frame means a note open in a panel
                    has one Export and not two — the panel's could not flush this
                    buffer before Rust reads the file. The rule above it
                    separates what acts on the note from what only shows it;
                    `NoteActions` draws the second rule, above Delete. */}
                <DropdownMenuSeparator />
                <ExportNoteItem vaultId={vaultId} noteId={noteId} />
              </NoteActions>
            )}
          />
        )}
        // Group 4 — the panel's own controls, when a panel is what is holding
        // this editor (Story 50.1). Placed and never composed here; undefined
        // in the three hosts that are not frames, and the row then has three
        // groups exactly as it did.
        frame={frame}
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

      {/* One band, one edge. These four strips are all the same kind of thing —
          "here is what keeper did to your file after you asked for something
          else" — and each carrying its own `border-b` said four times that
          something had ended and never once what had begun. The wrapper owns the
          single hairline; inside it the strips are told apart by their `py-1`
          rhythm, which is what separates rows of one kind. Unmounted rather than
          empty, so a pane with no notice draws no edge at all. */}
      {conflictCopy !== null || attachOutcome !== null || linkNotice !== null || body.gone ? (
        <div className="shrink-0 border-b">
          {conflictCopy === null ? null : (
            <p className="px-3 py-1 text-meta text-muted-foreground">
              keeper kept the version that was on disk as {conflictCopy} before writing yours.
            </p>
          )}

          {/* Story 45.13's receipt: which files were copied into the vault, and
              which were already in this note. `role="status"` because it answers
              something the person just did and they may not be looking here. */}
          {attachOutcome === null ? null : (
            <p role="status" className="px-3 py-1 text-meta text-muted-foreground">
              {attachOutcome}
            </p>
          )}

          {/* Story 45.18's answer when a link went nowhere: the note nobody has
              written, the scheme keeper will not hand to the OS, the window with
              no opener grant. A slot as well as `role="status"`, because the
              receipt above is also a status and a test must be able to read one
              without matching the other. */}
          {linkNotice === null ? null : (
            <p
              role="status"
              data-slot={LINK_NOTICE_SLOT}
              className="px-3 py-1 text-meta text-muted-foreground"
            >
              {linkNotice}
            </p>
          )}

          {body.gone ? (
            <p className="px-3 py-1 text-meta text-muted-foreground">
              This note isn't on disk any more. Your text is still here and saving writes it back.
            </p>
          ) : null}
        </div>
      ) : null}

      {/* Only in edit mode, and below the honest-state banners: the offer is a
          consequence of editing THIS note, and it has no meaning while the pane
          is showing history or a conflict. It renders nothing at all unless the
          note is a template whose text changed in this session. */}
      {mode === "edit" ? <TemplateUpdateOffer vaultId={vaultId} noteId={noteId} rev={rev} /> : null}

      {/* `display: contents` on the wrappers: the id is what the header control
          points `aria-controls` at, and a wrapper that generated a box would put
          a flex item between this column and the panel it holds. */}
      {showProperties ? (
        <div id={propertiesRegionId} className="min-h-0 shrink-0 border-t">
          {mode === "edit" ? (
            // Three questions about the open note that all want the same corner
            // of the screen, so they share one. "Linked from" used to live below
            // this region as a section of its own, appearing and disappearing
            // with the note's inbound links; as a tab it is always askable, and
            // its answer — including "nothing yet" — is a sentence rather than
            // an absence.
            <Tabs defaultValue="properties">
              <TabsList className="mx-3 mt-2">
                <TabsTrigger value="properties">{PROPERTIES_LABEL}</TabsTrigger>
                <TabsTrigger value="linked-from">Linked from</TabsTrigger>
                <TabsTrigger value="linked-to">Linked to</TabsTrigger>
              </TabsList>
              <TabsContent value="properties">
                <PropertiesPanel
                  frontmatter={frontmatter}
                  body={body.text}
                  subscriptionId={subscriptionId}
                  baseRev={rev}
                  onSaved={adoptPanelWrite}
                  rename={noteId === null ? null : renameNoteFile}
                />
              </TabsContent>
              <TabsContent value="linked-from">
                <LinksPanel
                  vaultId={vaultId}
                  noteId={noteId}
                  direction="from"
                  onOpen={(linked) => onOpenNote?.(linked)}
                />
              </TabsContent>
              <TabsContent value="linked-to">
                <LinksPanel
                  vaultId={vaultId}
                  noteId={noteId}
                  direction="to"
                  onOpen={(linked) => onOpenNote?.(linked)}
                />
              </TabsContent>
            </Tabs>
          ) : (
            <PanelUnavailable panel={PROPERTIES_LABEL} mode={mode} onBack={leaveMode} />
          )}
        </div>
      ) : null}

      {/* Below the properties, because that is the order of the question it
          answers: the block says which files the note has, and this puts one
          of them in the body. Unmounted rather than hidden — unlike the editor
          it holds no state worth keeping, and its `files:` reading is a
          function of the props it is given each time. */}
      {showAttachments ? (
        <div id={attachmentsRegionId} className="contents">
          {mode === "edit" ? (
            <AttachmentsPanel
              vaultId={vaultId}
              frontmatter={frontmatter}
              body={body.text}
              onInsert={insertAtCursor}
            />
          ) : (
            <PanelUnavailable panel={ATTACHMENTS_LABEL} mode={mode} onBack={leaveMode} />
          )}
        </div>
      ) : null}

      {/* Directly over the text it formats, and unmounted outside edit mode:
          in history or conflict mode there is no selection for a button to act
          on, and a toolbar that cannot do anything is a toolbar that lies. */}
      {mode === "edit" ? <FormatToolbar onAction={runFormat} /> : null}

      {/* Hidden rather than unmounted: the caret, the selection and the undo
          stack all have to survive a trip through history or conflict mode. */}
      <div
        ref={hostRef}
        // Named so a test can tell that an editor is booting here: its boot
        // awaits thirteen dynamic imports, and a file that walks away mid-flight
        // loses a race against the environment teardown. See
        // `src/test/note-editor-boot.ts`.
        data-slot="note-editor-host"
        className={mode === "edit" ? "min-h-0 flex-1 overflow-auto" : "hidden"}
      />

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
    </div>
  );
}

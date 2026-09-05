/**
 * The Notes surface on a phone (Epic 66, Story 66.4, FR-467…FR-469, AD-200).
 *
 * # One thing at a time
 *
 * The desktop's Notes pane is three columns — scope, list, editor — and a
 * 390px viewport has room for one. So the surface is two stack levels in
 * `PhoneShell`, the Bots shape (Story 62.2): {@link NotesPhoneList} is level 1
 * over the Inbox and {@link NotesPhoneNote} is level 2 over the list, and the
 * shell's own back button, edge-swipe and Escape pop them — this file adds no
 * navigation of its own. The scope column collapses to the vault switcher at
 * the head of the list and one search field: spaces, tags and the physical
 * tree are the desktop's lenses and stay there for now.
 *
 * # Reuse, and what is forked
 *
 * `VaultSwitcher`, `NoteList`, `NotesEmptyState`, `NoteDeleteDialog` and above
 * all `NoteEditor` — the CodeMirror chunk, its format toolbar, its history
 * panel, its properties — are the desktop components unchanged. A note on the
 * phone renders and edits in the same editor a Mac does, which is AD-200's
 * whole sentence. What is repeated is the desktop pane's glue: the list mirror
 * (`useNotesChanges`), the row-open → active panel target, the empty-state
 * arithmetic. That glue is inline in `notes-pane.tsx`, a file several stories
 * wire at once, and lifting it into a hook would be a refactor of a hotspot
 * rather than a phone surface.
 *
 * # What the editor is absent
 *
 * Nothing is hidden here by hand: the editor's own menu already leaves out
 * `Reveal` where `revealInFileManager` is false, and `Open in a capture
 * window` / `Export…` are absent on the reduced tier because a phone has no
 * second window and no destination picker (`capture-note-item.tsx`,
 * `note-editor.tsx`). History works: the panel's reads are answered
 * in-process on the phone (`keeper_sync::git::history`, AD-198).
 *
 * # The keyboard
 *
 * The note level pads its bottom by `--kb-inset` (Story 13.5's engine), so the
 * editor's scroll host ends above the keyboard and CodeMirror's own
 * scroll-into-view keeps the caret in sight. jsdom lays nothing out, so the
 * test is structural.
 */
import { FilePlus, NotebookPen, Search } from "lucide-react";
import { type Ref, useCallback, useEffect, useRef, useState } from "react";
import {
  PHONE_BACK_TO_INBOX,
  PHONE_INBOX_TITLE,
  PhoneBackBar,
} from "@/components/layout/phone-header";
import { NoteDeleteDialog } from "@/components/notes/note-delete-dialog";
import { NoteEditor } from "@/components/notes/note-editor";
import { NoteList } from "@/components/notes/note-list";
import { type NotesEmptyKind, NotesEmptyState } from "@/components/notes/notes-empty-state";
import { NEW_NOTE_LABEL, NOTES_COUNT_SLOT } from "@/components/notes/notes-pane";
import { VaultSwitcher } from "@/components/notes/vault-switcher";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { createNote, showCapture, useNotesActions } from "@/hooks/use-notes-actions";
import { useNotesChanges } from "@/hooks/use-notes-changes";
import { countLabel, NOTES } from "@/lib/count-label";
import type { NoteRowVm } from "@/lib/ipc/client";
import {
  emptyFilterReason,
  isFiltered,
  notesFiltersStore,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesListStore, useNotesListStore } from "@/lib/stores/notes-list";
import {
  ensureNotesVaultsHydrated,
  useActiveVault,
  useNotesVaultsStore,
} from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, usePanelsStore } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The list level's title, and the note level's back target. */
export const NOTES_PHONE_TITLE = "Notes";

/** The note level's back control: the level beneath it is the list. */
export const NOTES_PHONE_BACK_TO_LIST = `Back to ${NOTES_PHONE_TITLE}`;

/** The search field's accessible name. */
export const NOTES_PHONE_SEARCH_LABEL = "Search notes";

/** The header's way into quick capture: the sheet, not a window (AD-200). */
export const NOTES_PHONE_CAPTURE_LABEL = "Quick capture";

/** Names the note level's column, so a test can find its bands. */
export const NOTES_PHONE_NOTE_SLOT = "notes-phone-note";

/** What a failed verb reads as when the rejection carries no message. */
const NOTES_ACTION_FAILED = "keeper could not do that to this note.";

/** The active panel's note, when it is one. */
function useActiveNote(): { vaultId: string; noteId: string } | null {
  // The store's own target object, never a fresh one: a selector that built
  // an object per call would re-render on every store change forever.
  return usePanelsStore((s) => {
    const target = activePanel(s).target;
    return target?.kind === "note" ? target : null;
  });
}

/**
 * Level 1: the vault, the search field, the list.
 */
export function NotesPhoneList({
  onBack,
  backRef,
  onOpen,
}: {
  /** Pop to the Inbox (the shell's `onBack`). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
  /** Push the note level; the panel store already holds what to show. */
  onOpen: () => void;
}) {
  const vaults = useNotesVaultsStore((s) => s.vaults);
  const activeVaultId = useNotesVaultsStore((s) => s.activeVaultId);
  const activeVault = useActiveVault();
  const searchText = useNotesFiltersStore((s) => s.text);
  const filtered = useNotesFiltersStore(isFiltered);
  const filterReason = useNotesFiltersStore(emptyFilterReason);
  const searchNonce = useNotesFiltersStore((s) => s.searchNonce);
  const rows = useNotesListStore((s) => s.rows);
  const total = useNotesListStore((s) => s.total);
  const matched = useNotesListStore((s) => s.matched);
  const loaded = useNotesListStore((s) => s.loaded);
  const activeNote = useActiveNote();
  const actions = useNotesActions(activeVaultId);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

  // The list mirror follows the active vault and the search text.
  useNotesChanges(activeVaultId);

  // The palette's Open Note… / Search Notes land on this field, as they land on
  // the desktop pane's (UX-DR42): one find surface per tier.
  useEffect(() => {
    if (searchNonce > 0) {
      searchRef.current?.focus();
    }
  }, [searchNonce]);

  // A note that becomes the active target while this level is showing — the
  // palette's New Note, Today's Journal, a wikilink from somewhere — pushes the
  // note level. Only a CHANGE pushes: re-entering Notes with a note still
  // active from earlier lands on the list, which is the Bots rule too.
  const noteKey = activeNote === null ? null : `${activeNote.vaultId}/${activeNote.noteId}`;
  const seenNoteKey = useRef(noteKey);
  useEffect(() => {
    if (noteKey !== seenNoteKey.current) {
      seenNoteKey.current = noteKey;
      if (noteKey !== null) {
        onOpen();
      }
    }
  }, [noteKey, onOpen]);

  const report = useCallback((raw: unknown) => {
    setActionError(syncErrorMessage(raw, NOTES_ACTION_FAILED));
  }, []);

  const onCreate = useCallback(() => {
    setActionError(null);
    void createNote(null).catch(report);
  }, [report]);

  const openRow = useCallback(
    (row: NoteRowVm) => {
      if (activeVaultId !== null) {
        panelsStore.getState().setActiveTarget({
          kind: "note",
          vaultId: activeVaultId,
          noteId: row.id,
        });
        onOpen();
      }
    },
    [activeVaultId, onOpen],
  );

  const runVerb = useCallback(
    (row: NoteRowVm, verb: "e" | "p" | "u" | "r" | "d") => {
      if (verb === "d") {
        setDeletingId(row.id);
        return;
      }
      const run =
        verb === "e"
          ? actions.archive
          : verb === "p"
            ? actions.pin
            : verb === "u"
              ? actions.markRead
              : actions.reveal;
      void run(row).catch(report);
    },
    [actions, report],
  );

  const noVault = vaults !== null && vaults.length === 0;
  const scanning = activeVault !== null && !activeVault.indexed;
  let emptyKind: NotesEmptyKind | null = null;
  if (noVault) {
    emptyKind = "no-vault";
  } else if (loaded && rows.length === 0 && !scanning) {
    emptyKind =
      searchText.trim() !== "" ? "no-search-matches" : filtered ? "no-matches" : "empty-vault";
  }
  const emptyDetail =
    emptyKind === "no-matches" || emptyKind === "no-search-matches" ? filterReason : null;
  const onEmptyAction = () => {
    switch (emptyKind) {
      case "no-vault":
        primaryViewStore.getState().setView("sync");
        return;
      case "empty-vault":
        onCreate();
        return;
      case "no-search-matches":
        notesFiltersStore.getState().setText("");
        return;
      default:
        notesFiltersStore.getState().clearAll();
    }
  };

  return (
    <section
      aria-label={NOTES_PHONE_TITLE}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <PhoneBackBar
        backLabel={PHONE_BACK_TO_INBOX}
        backTitle={PHONE_INBOX_TITLE}
        backRef={backRef}
        onBack={onBack}
      >
        <h1 className="min-w-0 flex-1 truncate font-heading text-title">{NOTES_PHONE_TITLE}</h1>
        {/* Two verbs, both ≥44pt: a new note in the active vault, and quick
            capture — the sheet, which needs no vault chosen here because Rust
            resolves the page against the active vault. */}
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label={NOTES_PHONE_CAPTURE_LABEL}
          onClick={() => void showCapture()}
          className="size-11 shrink-0"
        >
          <NotebookPen aria-hidden="true" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label={NEW_NOTE_LABEL}
          disabled={activeVaultId === null}
          onClick={onCreate}
          className="size-11 shrink-0"
        >
          <FilePlus aria-hidden="true" />
        </Button>
      </PhoneBackBar>
      {!noVault && (
        <div className="flex shrink-0 flex-col gap-2 border-border border-b px-3 py-2">
          <VaultSwitcher />
          <div className="relative">
            <Search
              aria-hidden="true"
              className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
            />
            <Input
              ref={searchRef}
              type="search"
              aria-label={NOTES_PHONE_SEARCH_LABEL}
              placeholder={NOTES_PHONE_SEARCH_LABEL}
              value={searchText}
              onChange={(event) => notesFiltersStore.getState().setText(event.target.value)}
              // 44pt: the phone's minimum target, and the same row the header
              // buttons stand at.
              className="h-11 pl-9"
            />
          </div>
        </div>
      )}
      {scanning && (
        <div
          role="status"
          aria-label="Reading this vault"
          data-slot="notes-index-progress"
          className="h-0.5 w-full shrink-0 overflow-hidden bg-muted"
        >
          <div className="h-full w-1/3 bg-primary/60 motion-safe:animate-pulse" />
        </div>
      )}
      {actionError !== null && (
        <p role="alert" className="shrink-0 px-3 py-2 text-destructive text-xs">
          {actionError}
        </p>
      )}
      {!noVault && loaded && (
        <p
          role="status"
          data-slot={NOTES_COUNT_SLOT}
          className="shrink-0 border-border border-b px-3 py-1 text-muted-foreground text-xs"
        >
          {countLabel(total, NOTES, { of: matched })}
        </p>
      )}
      {emptyKind !== null ? (
        <NotesEmptyState kind={emptyKind} detail={emptyDetail} onAction={onEmptyAction} />
      ) : (
        <NoteList
          rows={rows}
          total={total}
          selectedId={activeNote?.vaultId === activeVaultId ? (activeNote?.noteId ?? null) : null}
          onSelect={openRow}
          // A phone has one panel: opening beside is opening.
          onSelectBeside={openRow}
          onToggleTag={(tag) => notesFiltersStore.getState().cycleTag(tag)}
          onVerb={runVerb}
          onGrow={() => notesListStore.getState().growWindow()}
        />
      )}
      {activeVaultId !== null && deletingId !== null && (
        <NoteDeleteDialog
          key={deletingId}
          vaultId={activeVaultId}
          noteId={deletingId}
          onClose={() => setDeletingId(null)}
          onDeleted={() => setDeletingId(null)}
        />
      )}
    </section>
  );
}

/**
 * Level 2: the note, in the editor. The column's one flexible region is the
 * editor's own scroll host; the back bar is bounded, and the bottom pads by the
 * keyboard's inset so the caret is never under it.
 */
export function NotesPhoneNote({
  onBack,
  backRef,
}: {
  /** Pop to the list (the shell's `onBack`). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
}) {
  const activeNote = useActiveNote();

  // The note this level shows was deleted or closed underneath it: pop, rather
  // than show an editor over nothing.
  useEffect(() => {
    if (activeNote === null) {
      onBack();
    }
  }, [activeNote, onBack]);

  return (
    <section
      aria-label={NOTES_PHONE_TITLE}
      data-slot={NOTES_PHONE_NOTE_SLOT}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background pb-[calc(var(--kb-inset,0px)_+_var(--safe-bottom))]"
    >
      <PhoneBackBar
        backLabel={NOTES_PHONE_BACK_TO_LIST}
        backTitle={NOTES_PHONE_TITLE}
        backRef={backRef}
        onBack={onBack}
      />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {activeNote !== null && (
          <NoteEditor
            vaultId={activeNote.vaultId}
            noteId={activeNote.noteId}
            // A link or a backlink opens its note in this same level: one
            // panel, so following replaces.
            onOpenNote={(noteId) =>
              panelsStore.getState().setActiveTarget({
                kind: "note",
                vaultId: activeNote.vaultId,
                noteId,
              })
            }
          />
        )}
      </div>
    </section>
  );
}

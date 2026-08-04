/**
 * The Notes primary view (Epic 37, Story 37.1, UX-DR36, UX-DR37, UX-DR41).
 *
 * Three columns in the frame keeper already has: a scope column, the note list,
 * and the editor. Notes is a top-level view inside the existing frame, not a
 * second app, which is why this adds no layout model and no navigation model of
 * its own — the pane the eye already uses for "the list of things" holds notes,
 * and the pane it uses for "the thing" holds the editor.
 *
 * The scope column lives here rather than in `sidebar-pane.tsx` because the app
 * sidebar is global: it carries the same rows in every view, and folding a
 * per-view tree into it would make a global control's contents depend on which
 * view happens to be open. So the column renders at the head of this surface,
 * with the account switcher's affordance for the vault row (UX-DR36) and the
 * sidebar's own type scale for everything below it.
 *
 * **Every row in the scope column is a filter, never a navigation** (UX-DR41).
 * Selecting a tag, a space, a folder or a different vault changes what pane 2
 * lists and leaves pane 3 alone. Two consequences are implemented rather than
 * hoped for:
 *
 *   - A filter that excludes the open note does NOT close it. The editor keeps
 *     the note; the list simply stops carrying its row.
 *   - Switching vault does not close it either. The open note is remembered with
 *     the vault it belongs to, so pane 3 shows nothing while you are over in
 *     another vault and the note is still open when you come back. A vault
 *     switch is a filter that happens to be wide.
 *
 * The one row in the column that is NOT a filter is Today: it opens or creates
 * today's journal entry (FR-99), which is an action on one note.
 */
import { CalendarDays, Inbox, NotebookPen, Pin } from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import { NoteEditor } from "@/components/notes/note-editor";
import { NoteFilterBar } from "@/components/notes/note-filter-bar";
import { NoteList } from "@/components/notes/note-list";
import { type NotesEmptyKind, NotesEmptyState } from "@/components/notes/notes-empty-state";
import { PhysicalTree } from "@/components/notes/physical-tree";
import { SpaceList } from "@/components/notes/space-list";
import { TagTree } from "@/components/notes/tag-tree";
import { VaultSwitcher } from "@/components/notes/vault-switcher";
import { Button } from "@/components/ui/button";
import {
  createNote,
  openJournalToday,
  saveFilterAsSpace,
  useNotesActions,
} from "@/hooks/use-notes-actions";
import { useNotesChanges } from "@/hooks/use-notes-changes";
import type { NoteRowVm } from "@/lib/ipc/client";
import {
  isFiltered,
  type NoteScope,
  notesFiltersStore,
  scopeLabel,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesListStore, useNotesListStore } from "@/lib/stores/notes-list";
import {
  ensureNotesVaultsHydrated,
  useActiveVault,
  useNotesVaultsStore,
} from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The scope rows above the data-driven groups, in the order the spine fixes. */
const SCOPE_ROWS: { scope: NoteScope; label: string; icon: typeof Inbox }[] = [
  { scope: { kind: "inbox" }, label: "Inbox", icon: Inbox },
  { scope: { kind: "journal" }, label: "Journal", icon: CalendarDays },
  { scope: { kind: "pinned" }, label: "Pinned", icon: Pin },
];

/** What a failed verb reads as when the rejection carries no message. */
const NOTES_ACTION_FAILED = "keeper could not do that to this note.";

/** The name a saved space gets when it is promoted from the chip bar. */
export const UNTITLED_SPACE_NAME = "Saved filter";

export function NotesPane() {
  const vaults = useNotesVaultsStore((s) => s.vaults);
  const activeVaultId = useNotesVaultsStore((s) => s.activeVaultId);
  const activeVault = useActiveVault();
  const scope = useNotesFiltersStore((s) => s.scope);
  const tags = useNotesFiltersStore((s) => s.tags);
  const searchText = useNotesFiltersStore((s) => s.text);
  const agentOnly = useNotesFiltersStore((s) => s.agentOnly);
  const filtered = useNotesFiltersStore(isFiltered);
  const searchNonce = useNotesFiltersStore((s) => s.searchNonce);
  const rows = useNotesListStore((s) => s.rows);
  const total = useNotesListStore((s) => s.total);
  const loaded = useNotesListStore((s) => s.loaded);
  const selected = useNotesListStore((s) => s.selected);
  const actions = useNotesActions(activeVaultId);
  const searchRef = useRef<HTMLInputElement | null>(null);
  // A verb's failure belongs to the surface that asked for it, so it is shown
  // here rather than swallowed or pushed into the read mirror.
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

  // The list mirror follows the active vault and the chip set; this is the only
  // thing in the view that reads or subscribes.
  useNotesChanges(activeVaultId);

  // The palette's Open Note… and Search Notes both land here: the vault's one
  // find surface is the field, and routing both to it beats two entries that
  // pretend to be different surfaces.
  useEffect(() => {
    if (searchNonce > 0) {
      searchRef.current?.focus();
    }
  }, [searchNonce]);

  const report = useCallback((raw: unknown) => {
    setActionError(syncErrorMessage(raw, NOTES_ACTION_FAILED));
  }, []);

  /**
   * Promote the chip set to a space note (`⌘⇧S`, FR-105, UX-DR37).
   *
   * It is named from the chips rather than from a prompt, because nothing in
   * this phase is a dialog (UX-DR35) and a space is an ordinary note — the name
   * is a first line the user can change like any other. Asking for it up front
   * would put a modal in front of a one-keystroke action, which is exactly the
   * friction that stops people saving filters at all.
   */
  const onSaveAsSpace = useCallback(() => {
    const parts = [
      scope.kind === "all" ? null : scopeLabel(scope),
      ...tags,
      agentOnly ? "Changed by agent" : null,
      searchText.trim() === "" ? null : `"${searchText.trim()}"`,
    ].filter((part): part is string => part !== null && part !== "");
    const name = parts.length === 0 ? UNTITLED_SPACE_NAME : parts.join(" · ");
    void saveFilterAsSpace(name).catch(report);
  }, [scope, tags, agentOnly, searchText, report]);

  // Two chords are scoped to this view by being mounted with it — nothing else
  // mounts this listener, so neither can fire from another surface.
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.altKey) {
        return;
      }
      if (!event.shiftKey && event.key === "f") {
        event.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (event.shiftKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        onSaveAsSpace();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onSaveAsSpace]);

  const openRow = useCallback(
    (row: NoteRowVm) => {
      if (activeVaultId !== null) {
        notesListStore.getState().select(activeVaultId, row.id);
      }
    },
    [activeVaultId],
  );

  const runVerb = useCallback(
    (row: NoteRowVm, verb: "e" | "p" | "u" | "r") => {
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

  // Esc walks the bar down one chip per press before it gives focus up, so the
  // last chip added is the first undone. Handled at the column so it works from
  // a row as well as from the chips.
  const onColumnKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && filtered) {
      event.preventDefault();
      notesFiltersStore.getState().dropLastChip();
    }
  };

  const noVault = vaults !== null && vaults.length === 0;
  // The open note is shown only while its own vault is the active one. It is not
  // forgotten in the meantime — that is the whole of "a vault switch is a filter".
  const openNoteId =
    selected !== null && selected.vaultId === activeVaultId ? selected.noteId : null;

  // A cold scan in progress is why the list can be empty and the vault not be.
  const scanning = activeVault !== null && !activeVault.indexed;

  let emptyKind: NotesEmptyKind | null = null;
  if (noVault) {
    emptyKind = "no-vault";
  } else if (loaded && rows.length === 0 && !scanning) {
    // "Nothing here" and "nothing matches" are different facts and get different
    // sentences; a search that matched nothing gets a third, because the way out
    // of it is the field rather than the chips. None of them may be shown while
    // the vault is still being read — "this vault is empty" said over a scan in
    // flight is simply false, and it is false for exactly as long as the user is
    // waiting to find out otherwise.
    emptyKind =
      searchText.trim() !== "" ? "no-search-matches" : filtered ? "no-matches" : "empty-vault";
  }

  const onEmptyAction = () => {
    switch (emptyKind) {
      case "no-vault":
        primaryViewStore.getState().setView("sync");
        return;
      case "empty-vault":
        void createNote().catch(report);
        return;
      case "no-search-matches":
        notesFiltersStore.getState().setText("");
        return;
      default:
        // The chips stay visible above the state, so this clears everything and
        // the one-chip walk-back stays available for the smaller correction.
        notesFiltersStore.getState().clearAll();
    }
  };

  return (
    <div className="flex min-h-0 flex-1">
      {/* Pane 1 — the scope column. */}
      <nav
        aria-label="Notes"
        className="flex h-full min-h-0 w-[240px] shrink-0 flex-col border-border border-r bg-sidebar"
      >
        <div className="shrink-0 p-2">
          <VaultSwitcher />
        </div>
        <ul className="flex shrink-0 flex-col gap-0.5 px-2">
          <li>
            {/* Not a filter: Today opens or creates one note (FR-99). */}
            <Button
              type="button"
              variant="ghost"
              className="w-full justify-start gap-2"
              onClick={() => {
                void openJournalToday().catch(report);
              }}
            >
              <NotebookPen aria-hidden="true" />
              Today
            </Button>
          </li>
          {SCOPE_ROWS.map((row) => {
            const Icon = row.icon;
            const active = scope.kind === row.scope.kind;
            return (
              <li key={row.label}>
                <Button
                  type="button"
                  variant="ghost"
                  aria-current={active ? "true" : undefined}
                  aria-pressed={active}
                  className={cn(
                    "w-full justify-start gap-2",
                    active && "bg-accent text-accent-foreground",
                  )}
                  onClick={() => notesFiltersStore.getState().setScope(row.scope)}
                >
                  <Icon aria-hidden="true" />
                  {row.label}
                </Button>
              </li>
            );
          })}
        </ul>
        {/* Both trees are unbounded, so each owns its own scroll container and
            everything below them stays reachable at every tree size (AD-34-4). */}
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pt-1">
          <SpaceList vaultId={activeVaultId} />
          <TagTree vaultId={activeVaultId} />
          <PhysicalTree vaultId={activeVaultId} />
        </div>
      </nav>

      {/* Pane 2 — the list. The primary surface (UX-DR37). */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: the column hosts the Esc chip-walk so it works from a row as well as from the bar; every control inside stays independently operable. */}
      <div
        onKeyDown={onColumnKeyDown}
        className="flex h-full min-h-0 w-[320px] shrink-0 flex-col border-border border-r bg-background"
      >
        {!noVault && <NoteFilterBar onSaveAsSpace={onSaveAsSpace} searchRef={searchRef} />}
        {/* A cold scan in flight, under the chip bar (FR-96, AD-57). The list
            stays interactive throughout — you can open the first note before the
            last one is found — so this is a thin bar and not a blocking state.
            A corrupt cache takes the same branch as an absent one: a rescan,
            never an error message. */}
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
        {emptyKind !== null ? (
          <NotesEmptyState kind={emptyKind} onAction={onEmptyAction} />
        ) : (
          <NoteList
            rows={rows}
            total={total}
            selectedId={openNoteId}
            onSelect={openRow}
            onToggleTag={(tag) => notesFiltersStore.getState().toggleTag(tag)}
            onVerb={runVerb}
            onGrow={() => notesListStore.getState().growWindow()}
          />
        )}
      </div>

      {/* Pane 3 — the editor. It owns everything about the open document; this
          view only tells it which note, and `null` means "nothing on screen"
          rather than "close and forget". */}
      <div className="flex h-full min-h-0 flex-1 flex-col bg-background">
        {activeVaultId !== null && (
          <NoteEditor
            vaultId={activeVaultId}
            noteId={openNoteId}
            onOpenNote={(noteId) => notesListStore.getState().select(activeVaultId, noteId)}
          />
        )}
      </div>
    </div>
  );
}

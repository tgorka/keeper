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
 * **Every row in the column is a space, a tag or a folder** (Story 44.3,
 * AD-79). There is no fixed rail above them: Inbox, Journal, Pinned and
 * Recordings are seeded notes under `spaces/`, listed by {@link SpaceList} with
 * every other space, editable and deleteable like every other space. Today is
 * gone (AD-80) — it never filtered anything, and opening today's journal entry
 * is still `⌘⌥J`, the tray and the palette.
 *
 * **A note can be created from the column** (Story 44.6, FR-160). Two places,
 * and they mean different things: the button at the head of the column makes a
 * note in the vault, and the `+` on a space row makes a note *that space will
 * list* — which is a promise about where it turns up, so Rust reads the
 * space's query and gives the note the tags, folder and flags it needs. When it
 * cannot, the create still happens and the sentence Rust composed is shown
 * above the list. Nothing here parses a query; the surface sends a space id.
 */
import { FilePlus } from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import { PanelStrip } from "@/components/layout/panel-strip";
import { useSurfaceColumn } from "@/components/layout/surface-column";
import { NoteDeleteDialog } from "@/components/notes/note-delete-dialog";
import { NoteFilterBar } from "@/components/notes/note-filter-bar";
import { NoteList } from "@/components/notes/note-list";
import { type NotesEmptyKind, NotesEmptyState } from "@/components/notes/notes-empty-state";
import { PhysicalTree } from "@/components/notes/physical-tree";
import { SpaceList } from "@/components/notes/space-list";
import { TagTree } from "@/components/notes/tag-tree";
import { VaultSwitcher } from "@/components/notes/vault-switcher";
import { createNote, saveFilterAsSpace, useNotesActions } from "@/hooks/use-notes-actions";
import { useNotesChanges } from "@/hooks/use-notes-changes";
import { countLabel, NOTES } from "@/lib/count-label";
import type { NoteRowVm } from "@/lib/ipc/client";
import {
  emptyFilterReason,
  isFiltered,
  isScopeOnly,
  notesFiltersStore,
  scopeLabel,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesListStore, useNotesListStore } from "@/lib/stores/notes-list";
import { hydrateNotesRailFold } from "@/lib/stores/notes-rail-fold";
import {
  ensureNotesVaultsHydrated,
  useActiveVault,
  useNotesVaultsStore,
} from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, usePanelsStore } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** What a failed verb reads as when the rejection carries no message. */
const NOTES_ACTION_FAILED = "keeper could not do that to this note.";

/** The name a saved space gets when it is promoted from the chip bar. */
export const UNTITLED_SPACE_NAME = "Saved filter";

/** The rail's create control, kept verbatim so a test names what a user reads. */
export const NEW_NOTE_LABEL = "New note";

/**
 * Test id for the line that says how many notes this lens holds (Story 44.11).
 * A slot rather than a text match, so a test asserts the NUMBER rather than
 * re-deriving the sentence the label module composes.
 */
export const NOTES_COUNT_SLOT = "notes-count";

/**
 * Test id for a sentence Rust composed about a create it could not fully
 * honour (Story 44.6). A slot rather than a text match, because the wording is
 * Rust's and asserting it here would put a second copy of the sentence in the
 * language that cannot produce it.
 */
export const NOTES_NOTICE_SLOT = "notes-create-notice";

/**
 * What an empty panel says on the Notes surface (Story 46.12).
 *
 * `PanelStrip`'s own default names the gesture that fills a panel in Files —
 * "Click a file to open it" — and beside a list of notes that is an instruction
 * to do something this surface does not offer. The sentence is the first thing
 * a fresh keeper shows on this tab, so it is the one place where naming the
 * wrong noun is most expensive.
 */
export const NOTES_PANEL_EMPTY_SENTENCE = "Nothing is open here yet. Click a note to open it.";

export function NotesPane() {
  // Both of this surface's fixed columns fold and resize (Story 48.1). The
  // panel strip does not: it is the flexible one, and a surface where every
  // column has a width is a surface with a gap in it.
  const rail = useSurfaceColumn("notes-rail");
  const list = useSurfaceColumn("notes-list");
  const vaults = useNotesVaultsStore((s) => s.vaults);
  const activeVaultId = useNotesVaultsStore((s) => s.activeVaultId);
  const activeVault = useActiveVault();
  const scope = useNotesFiltersStore((s) => s.scope);
  const tagTerms = useNotesFiltersStore((s) => s.tagTerms);
  const searchText = useNotesFiltersStore((s) => s.text);
  const agentOnly = useNotesFiltersStore((s) => s.agentOnly);
  const filtered = useNotesFiltersStore(isFiltered);
  // A string, so subscribing to it re-renders on value rather than on identity.
  const filterReason = useNotesFiltersStore(emptyFilterReason);
  const scopeOnly = useNotesFiltersStore(isScopeOnly);
  const searchNonce = useNotesFiltersStore((s) => s.searchNonce);
  const rows = useNotesListStore((s) => s.rows);
  const total = useNotesListStore((s) => s.total);
  const matched = useNotesListStore((s) => s.matched);
  const loaded = useNotesListStore((s) => s.loaded);
  // The note this pane is showing: the active panel's target, when it is a note.
  const activeNote = usePanelsStore((s) => {
    const active = activePanel(s);
    return active.target?.kind === "note" ? active.target : null;
  });
  const actions = useNotesActions(activeVaultId);
  const searchRef = useRef<HTMLInputElement | null>(null);
  // A verb's failure belongs to the surface that asked for it, so it is shown
  // here rather than swallowed or pushed into the read mirror.
  const [actionError, setActionError] = useState<string | null>(null);
  // What Rust had to say about a create that succeeded and still did not do
  // what was asked — a note in a space whose query no new note can satisfy.
  // Separate from `actionError` because it is not a failure: the note exists,
  // and rendering it as an error would send someone looking for a broken write.
  const [notices, setNotices] = useState<string[]>([]);
  // The row a confirmation is open for. The id alone, not the row: the list
  // re-streams constantly, and a held row object would keep a stale title on
  // screen while the dialog names what it is about to delete.
  const [deletingId, setDeletingId] = useState<string | null>(null);

  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

  // Restore which rail sections were folded (Story 47.3). Here and not in
  // `AppShell`, unlike the chat sidebar's fold: Spaces, Tags and Files render
  // nowhere but this pane, and this pane is unmounted whenever another primary
  // view is showing. Idempotent, so the double-invoked development effect and
  // every later remount restore exactly once and never overwrite a fold the
  // user has changed since.
  useEffect(() => {
    hydrateNotesRailFold(document.cookie);
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
   * Make a note and open it (FR-160, Story 44.6).
   *
   * `spaceId` is `null` from the rail's own button — a note in the vault, which
   * the default list shows — and a space's id from that space's `+`. The
   * difference is Rust's to act on; this only says which.
   *
   * Rust's notices are adopted whatever they are, including the empty list, so
   * a second create clears the first one's sentence rather than leaving a
   * stale explanation over a note it is not about.
   */
  const onCreate = useCallback(
    (spaceId: string | null) => {
      setActionError(null);
      void createNote(spaceId)
        .then((created) => setNotices(created?.notices ?? []))
        .catch(report);
    },
    [report],
  );

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
      ...tagTerms.map((chip) => (chip.term === "exclude" ? `not ${chip.tag}` : chip.tag)),
      agentOnly ? "Changed by agent" : null,
      searchText.trim() === "" ? null : `"${searchText.trim()}"`,
    ].filter((part): part is string => part !== null && part !== "");
    const name = parts.length === 0 ? UNTITLED_SPACE_NAME : parts.join(" · ");
    void saveFilterAsSpace(name).catch(report);
  }, [scope, tagTerms, agentOnly, searchText, report]);

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

  /**
   * A row's own click: the active panel now shows this note (Story 45.1).
   *
   * Single click replaces, double click opens beside — AD-90's gesture pair,
   * the same one the Files tree uses, and deliberately not a second contract of
   * this pane's own. Before Story 46.12 there was no twin here at all, because
   * the model refused a second note panel; the store handles the interleaving
   * (a double click is always preceded by a real single click, and `openPanel`
   * puts back what that click displaced).
   */
  const openRow = useCallback(
    (row: NoteRowVm) => {
      if (activeVaultId !== null) {
        panelsStore.getState().setActiveTarget({
          kind: "note",
          vaultId: activeVaultId,
          noteId: row.id,
        });
      }
    },
    [activeVaultId],
  );

  /** Double click: open this note BESIDE what is already open (Story 46.12). */
  const openRowBeside = useCallback(
    (row: NoteRowVm) => {
      if (activeVaultId !== null) {
        panelsStore.getState().openPanel({
          kind: "note",
          vaultId: activeVaultId,
          noteId: row.id,
        });
      }
    },
    [activeVaultId],
  );

  const runVerb = useCallback(
    (row: NoteRowVm, verb: "e" | "p" | "u" | "r" | "d") => {
      // `d` is the one verb that does not act. It asks, and the confirmation
      // is what acts — so it is handled here rather than joining the chain of
      // actions below, where every other entry runs on the keystroke.
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
  // The open note is the active panel's target, and it is shown only while its
  // own vault is the active one. It is not forgotten in the meantime — that is
  // the whole of "a vault switch is a filter" — and it is the panel list rather
  // than a cursor of this pane's own, so the Files surface and this one cannot
  // disagree about what is open (Story 45.1).
  const openNoteId =
    activeNote !== null && activeNote.vaultId === activeVaultId ? activeNote.noteId : null;

  // A cold scan in progress is why the list can be empty and the vault not be.
  const scanning = activeVault !== null && !activeVault.indexed;

  let emptyKind: NotesEmptyKind | null = null;
  if (noVault) {
    emptyKind = "no-vault";
  } else if (loaded && rows.length === 0 && !scanning) {
    // "Nothing here" and "nothing matches" are different facts and get different
    // sentences; a search that matched nothing gets a third, because the way out
    // of it is the field rather than the chips, and an empty lens a fourth,
    // because nothing the user can clear will fill it. None of them may be shown
    // while the vault is still being read — "this vault is empty" said over a
    // scan in flight is simply false, and it is false for exactly as long as the
    // user is waiting to find out otherwise.
    // The Recordings sentence follows the *space*, not a scope kind that no
    // longer exists: `defaultKey` is the identity keeper wrote into the note, so
    // renaming the seeded Recordings space to anything at all keeps it, and a
    // space of the user's own that happens to be called Recordings does not
    // borrow it.
    emptyKind =
      searchText.trim() !== ""
        ? "no-search-matches"
        : scope.kind === "space" && scope.defaultKey === "recordings" && scopeOnly
          ? "no-recordings"
          : filtered
            ? "no-matches"
            : "empty-vault";
  }

  // Which terms are narrowing, for the two states a filter can cause. The empty
  // vault and the empty lens are facts about the vault, not about the bar, so
  // naming terms under them would answer a question nobody asked.
  const emptyDetail =
    emptyKind === "no-matches" || emptyKind === "no-search-matches" ? filterReason : null;

  const onEmptyAction = () => {
    switch (emptyKind) {
      case "no-vault":
        primaryViewStore.getState().setView("sync");
        return;
      case "empty-vault":
        onCreate(null);
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
      {/* Pane 1 — the scope column (Story 48.1: it folds, and it resizes). */}
      <nav
        aria-label="Notes"
        {...rail.rootProps}
        className="flex h-full min-h-0 shrink-0 flex-col border-border border-r bg-sidebar"
      >
        {rail.chrome}
        {!rail.folded && (
          <>
            <div className="shrink-0 p-2">
              <VaultSwitcher />
            </div>
            {/* The rail's own create (Story 44.6). At the head of the column and
                not inside the Spaces group: this one makes a note in the vault,
                which the default list shows, while the `+` on a space row makes a
                note that space will list. Two different promises need two
                different controls. */}
            <div className="shrink-0 px-2 pb-2">
              <button
                type="button"
                disabled={activeVaultId === null}
                onClick={() => onCreate(null)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md border border-border px-2 py-1.5 text-left text-sm outline-none",
                  "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
                  "disabled:pointer-events-none disabled:opacity-50",
                )}
              >
                <FilePlus aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                {NEW_NOTE_LABEL}
              </button>
            </div>
            {/* Every group below is unbounded — spaces as much as tags, now that
                the four fixed rows are spaces too — so they share one scroll
                container and everything in it stays reachable at every size
                (AD-34-4). */}
            <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pt-1">
              <SpaceList vaultId={activeVaultId} onNewNote={(space) => onCreate(space.id)} />
              <TagTree vaultId={activeVaultId} />
              <PhysicalTree vaultId={activeVaultId} />
            </div>
          </>
        )}
      </nav>
      {rail.seam}

      {/* Pane 2 — the list. The primary surface (UX-DR37). */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: the column hosts the Esc chip-walk so it works from a row as well as from the bar; every control inside stays independently operable. */}
      <div
        onKeyDown={onColumnKeyDown}
        {...list.rootProps}
        className="flex h-full min-h-0 shrink-0 flex-col border-border border-r bg-background"
      >
        {list.chrome}
        {!list.folded && (
          <>
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
            {/* What Rust said about a create that could not be what the space asked
                for. `status` and not `alert`: the note was written, and the only
                thing wrong is where it is not. Each sentence is composed in Rust,
                because the reason names query terms and this surface does not read
                queries. */}
            {notices.map((notice) => (
              <p
                key={notice}
                role="status"
                data-slot={NOTES_NOTICE_SLOT}
                className="shrink-0 border-border border-b px-3 py-2 text-muted-foreground text-xs"
              >
                {notice}
              </p>
            ))}
            {/* How many notes this lens selects (Story 44.11, FR-166).

                Above the list rather than inside it, and a sibling of the empty
                state rather than a child of `NoteList`, because an empty set has to
                say zero. `NoteList` is not rendered at all when the vault or the
                filter comes up empty, and a count that vanishes exactly when the
                answer is "none" is a count that never answers the question anyone
                asks it.

                `total` and never `rows.length`: the list is windowed and the page
                is 200, so the array on screen is a screenful of a vault. */}
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
                selectedId={openNoteId}
                onSelect={openRow}
                onSelectBeside={openRowBeside}
                onToggleTag={(tag) => notesFiltersStore.getState().cycleTag(tag)}
                onVerb={runVerb}
                onGrow={() => notesListStore.getState().growWindow()}
              />
            )}
          </>
        )}
      </div>
      {list.seam}

      {/* Pane 3 — the panels (Story 46.12).

          It used to be one `NoteEditor` and a note id, which is the shape that
          could hold exactly one note. It is the same strip the Files surface
          hosts, because "N targets side by side" is a solved problem in this
          codebase and a second strip would be a second answer to it — with its
          own gesture contract, its own focus rule and its own cookie, all of
          which would drift.

          The panel list is global and singular, and that is the point rather
          than a compromise: a note opened here is still open when you go to
          Files, and the file its `Show in Files` opened is still open when you
          come back. Switching surfaces changes the browser beside the panels,
          never the panels — and a vault switch is the same act one level down,
          so a note panel is NOT hidden when its vault stops being the active
          one. Before this story it was, because a single editor slot had to be
          told which note; now the panel holds the note and the rail filters the
          list. `NotePanelBody` says so out loud if the vault is actually gone. */}
      <PanelStrip emptySentence={NOTES_PANEL_EMPTY_SENTENCE} />

      {/* The list's `Delete` key, confirmed. The same dialog and the same
          command the editor's actions menu and the sidebar's space rows use:
          three doors, one removal (Story 45.17). Keyed on the id so pressing
          Delete on a second row after cancelling the first asks about the
          second rather than re-rendering the first's plan. */}
      {activeVaultId !== null && deletingId !== null && (
        <NoteDeleteDialog
          key={deletingId}
          vaultId={activeVaultId}
          noteId={deletingId}
          onClose={() => setDeletingId(null)}
          // Nothing else to do: `deleteNote` closes any panel showing it, and
          // the list mirror is driven by the index, which the trash announced.
          onDeleted={() => setDeletingId(null)}
        />
      )}
    </div>
  );
}

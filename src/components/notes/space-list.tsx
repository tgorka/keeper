/**
 * The SPACES lens (Epic 37, Story 37.4, FR-105).
 *
 * One row per note under `spaces/`, each a saved query. A space is an ordinary
 * markdown note, which is why this list has a state no other sidebar group
 * needs: a space whose query does not parse is **expected**, not exceptional.
 * The note is agent-editable and hand-editable, so a broken one will happen, and
 * it must render as a row that says so rather than take the group down with it.
 *
 * A broken space matches nothing and says nothing else. It never falls back to
 * matching everything — a space is a surface people run bulk actions from, and a
 * query that silently widens is how a saved view becomes a data-loss story.
 *
 * Selecting a space is a filter (UX-DR41): the open note stays open.
 *
 * **A space can be edited** (Story 43.4). Every row carries a pencil beside it
 * that opens {@link SpaceEditor}; the icon a space was given is drawn where a
 * bare dot used to be, so a column of saved views is told apart by shape rather
 * than by reading eight labels. A space with no icon, and a space whose stored
 * icon is not in the set any more, both draw the fallback glyph — never a hole.
 *
 * **A note can be made in a space** (Story 44.6, FR-160). The `+` beside the
 * pencil creates a note *that space will list*, which is a different promise
 * from "create a note somewhere": the create carries the space's id and Rust
 * derives the tags, folder and flags the space's own query needs. This file
 * therefore knows nothing about the DSL — the row hands up a space and the pane
 * hands up an id.
 */
import { ChevronDown, ChevronRight, FilePlus, Pencil, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { FoldSection } from "@/components/layout/sidebar-group";
import { NoteDeleteDialog } from "@/components/notes/note-delete-dialog";
import { SpaceEditor } from "@/components/notes/space-editor";
import { spaceIcon } from "@/components/notes/space-icons";
import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpaces, notesSpacesRestoreDefaults } from "@/lib/ipc/client";
import {
  ALL_NOTES_SCOPE,
  notesFiltersStore,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesRailFoldStore, useNotesRailFold } from "@/lib/stores/notes-rail-fold";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The subtitle a space with an unparseable query carries, kept verbatim. */
export const SPACE_BROKEN_SUBTITLE = "This space's query can't be read";

/**
 * The subtitle a space whose `sort` or `order` keeper could not read carries
 * (Story 44.4), kept verbatim.
 *
 * Short and generic on purpose: the row is a sidebar entry and the sentences
 * Rust composed name a value and a fallback, which does not fit here. This is
 * the marker that something is wrong; the whole of it is on the row's `title`
 * and, for a keyboard, in the editor the pencil opens. What it must not be is
 * absent — a space quietly ignoring a line of its own file is the failure this
 * replaces.
 */
export const SPACE_SETTINGS_SUBTITLE = "Some of this space's settings can't be read";

/** The restore control's accessible name, kept verbatim. */
export const RESTORE_DEFAULTS = "Restore default spaces";

/**
 * What restore says when there was nothing to do.
 *
 * Said out loud rather than flashing a success at a no-op: the whole promise of
 * the control is that it never touches a space that is there, so the case where
 * it touched nothing is the case it most has to be believed about.
 */
export const RESTORE_NOTHING_MISSING = "Nothing was missing.";

/** What restore says when keeper could not write. */
export const RESTORE_FAILED = "keeper couldn't restore the default spaces.";

/**
 * The delete control's accessible name, suffixed with the space. Named because
 * a column of eight rows would otherwise offer eight controls all called
 * "Delete", and because the confirmation that follows is the only other place
 * the space is named.
 */
export const DELETE_SPACE = "Delete space";

export function SpaceList({
  vaultId,
  onNewNote,
}: {
  vaultId: string | null;
  /**
   * Make a note in this space. Absent means the group renders no `+` at all —
   * an affordance that cannot do anything is worse than none, and every caller
   * that can create passes one.
   */
  onNewNote?: (space: NoteSpaceVm) => void;
}) {
  const [spaces, setSpaces] = useState<NoteSpaceVm[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [restoreResult, setRestoreResult] = useState<string | null>(null);
  const activeSpaceId = useNotesFiltersStore((s) => (s.scope.kind === "space" ? s.scope.id : null));
  const folded = useNotesRailFold((state) => state.groups.spaces);

  const reload = useCallback(() => {
    if (vaultId === null) {
      setSpaces([]);
      return () => {};
    }
    let cancelled = false;
    void notesSpaces(vaultId)
      .then((next) => {
        if (!cancelled) {
          setSpaces(next);
        }
      })
      .catch(() => {
        // The group stays empty rather than breaking the column.
      });
    return () => {
      cancelled = true;
    };
  }, [vaultId]);

  useEffect(reload, [reload]);

  /**
   * Re-create the defaults this vault is missing (FR-156).
   *
   * Rust decides what "missing" means — a default is missing when no space
   * carries its key and none is already called what it would be called — so this
   * sends the ask and reports the count. Deciding it here would need the space
   * list *and* the seed ledger in the webview, and the ledger is a fact about
   * the vault on disk.
   *
   * A refusal carries Rust's own sentence rather than the generic one, because
   * the sentence names the file it could not read. That is the difference
   * between "keeper couldn't restore the default spaces" — which sends someone
   * to a bug report — and "`.keeper-spaces.json` could not be read (permission
   * denied)", which sends them to the file.
   */
  const restore = useCallback(() => {
    if (vaultId === null) {
      return;
    }
    setRestoring(true);
    setRestoreResult(null);
    void notesSpacesRestoreDefaults(vaultId)
      .then((count) => {
        setRestoreResult(
          count === 0
            ? RESTORE_NOTHING_MISSING
            : count === 1
              ? "Restored 1 space."
              : `Restored ${count} spaces.`,
        );
        reload();
      })
      .catch((raw: unknown) => setRestoreResult(syncErrorMessage(raw, RESTORE_FAILED)))
      .finally(() => setRestoring(false));
  }, [vaultId, reload]);

  const edited = spaces.find((space) => space.id === editing) ?? null;

  // Spaces are the rail now (Story 44.3), so this section renders even when the
  // vault has none: folding it AWAY when it is empty would hide the one control
  // that fills it, and a vault whose owner deleted every default would have no
  // way back. Story 47.3 makes it fold, and that reason still holds — a fold
  // hides the rows and never the header, so "Restore default spaces" and what
  // it reports both sit outside the folded region and stay reachable shut.
  return (
    <>
      <FoldSection
        label="Spaces"
        icon={folded ? ChevronRight : ChevronDown}
        folded={folded}
        onToggle={() => notesRailFoldStore.getState().toggleGroup("spaces")}
        id="notes-rail-spaces"
        className="shrink-0"
        as="ul"
        bodyClassName="flex flex-col gap-0.5"
        actions={
          <button
            type="button"
            aria-label={RESTORE_DEFAULTS}
            title={RESTORE_DEFAULTS}
            disabled={vaultId === null || restoring}
            onClick={restore}
            className={cn(
              "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
              "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
              "disabled:pointer-events-none disabled:opacity-50",
            )}
          >
            <RotateCcw aria-hidden="true" className="size-3.5" />
          </button>
        }
        notice={
          restoreResult !== null && (
            <p role="status" className="px-2 pb-1 text-muted-foreground text-xs">
              {restoreResult}
            </p>
          )
        }
      >
        {spaces.map((space) => {
          const broken = space.error !== null;
          // A presentation key keeper could not read is not a broken space: it
          // still selects what it selects, it is simply not obeying one line of
          // its own file. It gets its own quieter line rather than the parse
          // failure's, because sending someone to fix a query that is fine is
          // worse than saying nothing.
          const misread = space.warnings.length > 0;
          const subtitle = broken
            ? SPACE_BROKEN_SUBTITLE
            : misread
              ? SPACE_SETTINGS_SUBTITLE
              : null;
          const active = space.id === activeSpaceId;
          const Glyph = spaceIcon(space.icon);
          return (
            <li key={space.id} className="group flex items-start">
              <button
                type="button"
                aria-current={active ? "true" : undefined}
                aria-pressed={active}
                // The failure belongs in the accessible name too: a dot is not a
                // carrier on its own (UX-DR43).
                aria-label={subtitle === null ? space.name : `${space.name}, ${subtitle}`}
                // The whole sentence, for a pointer. The keyboard path to it is
                // the pencil beside this row, which lists every warning in full
                // — a row in a sidebar this narrow cannot hold one of them, and
                // the editor is where the value gets fixed anyway.
                title={misread ? space.warnings.join(" ") : undefined}
                className={cn(
                  "flex min-w-0 flex-1 items-start gap-2 rounded-md px-2 py-1.5 text-left outline-none",
                  "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                  active ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
                )}
                onClick={() =>
                  notesFiltersStore.getState().setScope({
                    kind: "space",
                    id: space.id,
                    name: space.name,
                    defaultKey: space.defaultKey,
                  })
                }
              >
                <span
                  aria-hidden="true"
                  data-slot="space-dot"
                  className={cn(
                    "mt-1.5 size-2 shrink-0 rounded-full",
                    broken ? "bg-bridge-degraded" : "bg-transparent",
                  )}
                />
                <Glyph
                  aria-hidden="true"
                  data-slot="space-icon"
                  data-space-icon={space.icon ?? "none"}
                  className="mt-0.5 size-4 shrink-0 text-muted-foreground"
                />
                <span className="flex min-w-0 flex-col">
                  <span className="truncate text-sm">{space.name}</span>
                  {subtitle !== null && (
                    <span
                      data-slot="space-subtitle"
                      className="truncate text-muted-foreground text-xs"
                    >
                      {subtitle}
                    </span>
                  )}
                </span>
              </button>
              {/* Always in the DOM, revealed on hover or focus, for the pencil's
                  reason. The accessible name carries the space, because a
                  column of eight rows would otherwise offer eight controls all
                  called "New note". */}
              {onNewNote !== undefined && (
                <button
                  type="button"
                  aria-label={`New note in ${space.name}`}
                  onClick={() => onNewNote(space)}
                  className={cn(
                    "mt-1 shrink-0 rounded-md p-1.5 text-muted-foreground outline-none",
                    "opacity-0 focus-visible:opacity-100 group-hover:opacity-100",
                    "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
                  )}
                >
                  <FilePlus aria-hidden="true" className="size-3.5" />
                </button>
              )}
              {/* Always in the DOM, revealed on hover or focus: an affordance
                  that only exists under a pointer is one a keyboard cannot
                  reach. */}
              <button
                type="button"
                aria-label={`Edit space ${space.name}`}
                onClick={() => setEditing(space.id)}
                className={cn(
                  "mt-1 shrink-0 rounded-md p-1.5 text-muted-foreground outline-none",
                  "opacity-0 focus-visible:opacity-100 group-hover:opacity-100",
                  "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
                )}
              >
                <Pencil aria-hidden="true" className="size-3.5" />
              </button>
              {/* Same reveal rule as the pencil, and after it: the destructive
                  control is last, so a hand travelling along the row reaches
                  edit before delete. */}
              <button
                type="button"
                aria-label={`${DELETE_SPACE} ${space.name}`}
                onClick={() => setDeleting(space.id)}
                className={cn(
                  "mt-1 shrink-0 rounded-md p-1.5 text-muted-foreground outline-none",
                  "opacity-0 focus-visible:opacity-100 group-hover:opacity-100",
                  "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
                )}
              >
                <Trash2 aria-hidden="true" className="size-3.5" />
              </button>
            </li>
          );
        })}
      </FoldSection>
      {vaultId !== null && edited !== null && (
        // Keyed on the space, so opening a second editor after the first seeds
        // its form from the right space rather than from stale state.
        <SpaceEditor
          key={edited.id}
          vaultId={vaultId}
          space={edited}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            reload();
          }}
        />
      )}
      {/* A space is a note, so this is the note dialog and the note command —
          `notes_delete` records a seeded default as offered, which is what
          stops the next refresh putting it back (Story 45.17, FR-195). A
          second removal path for spaces would be the one that forgot. */}
      {vaultId !== null && deleting !== null && (
        <NoteDeleteDialog
          key={deleting}
          vaultId={vaultId}
          noteId={deleting}
          onClose={() => setDeleting(null)}
          onDeleted={() => {
            // The rail must stop showing it, and a lens pointed at a space
            // that no longer exists selects nothing and cannot say why — so
            // the scope goes back to all notes when the deleted space was the
            // active one, and is left alone when it was not.
            if (deleting === activeSpaceId) {
              notesFiltersStore.getState().setScope(ALL_NOTES_SCOPE);
            }
            reload();
          }}
        />
      )}
    </>
  );
}

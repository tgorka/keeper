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
 */
import { Pencil, RotateCcw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { SpaceEditor, spaceIcon } from "@/components/notes/space-editor";
import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpaces, notesSpacesRestoreDefaults } from "@/lib/ipc/client";
import { notesFiltersStore, useNotesFiltersStore } from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

/** The subtitle a space with an unparseable query carries, kept verbatim. */
export const SPACE_BROKEN_SUBTITLE = "This space's query can't be read";

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

export function SpaceList({ vaultId }: { vaultId: string | null }) {
  const [spaces, setSpaces] = useState<NoteSpaceVm[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [restoreResult, setRestoreResult] = useState<string | null>(null);
  const activeSpaceId = useNotesFiltersStore((s) => (s.scope.kind === "space" ? s.scope.id : null));

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
      .catch(() => setRestoreResult(RESTORE_FAILED))
      .finally(() => setRestoring(false));
  }, [vaultId, reload]);

  const edited = spaces.find((space) => space.id === editing) ?? null;

  // The section renders even when the vault has none. Spaces are the rail now
  // (Story 44.3): folding it away when it is empty would hide the one control
  // that fills it, and a vault whose owner deleted every default would have no
  // way back.
  return (
    <section aria-label="Spaces" className="flex shrink-0 flex-col px-2 pb-1">
      <div className="flex items-center justify-between gap-1 px-2 py-1">
        <span className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
          Spaces
        </span>
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
      </div>
      {restoreResult !== null && (
        <p role="status" className="px-2 pb-1 text-muted-foreground text-xs">
          {restoreResult}
        </p>
      )}
      <ul className="flex flex-col gap-0.5">
        {spaces.map((space) => {
          const broken = space.error !== null;
          const active = space.id === activeSpaceId;
          const Glyph = spaceIcon(space.icon);
          return (
            <li key={space.id} className="group flex items-start">
              <button
                type="button"
                aria-current={active ? "true" : undefined}
                aria-pressed={active}
                // The parse failure belongs in the accessible name too: a dot is
                // not a carrier on its own (UX-DR43).
                aria-label={broken ? `${space.name}, ${SPACE_BROKEN_SUBTITLE}` : space.name}
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
                  {broken && (
                    <span className="truncate text-muted-foreground text-xs">
                      {SPACE_BROKEN_SUBTITLE}
                    </span>
                  )}
                </span>
              </button>
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
            </li>
          );
        })}
      </ul>
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
    </section>
  );
}

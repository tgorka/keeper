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
 */
import { useEffect, useState } from "react";
import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpaces } from "@/lib/ipc/client";
import { notesFiltersStore, useNotesFiltersStore } from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

/** The subtitle a space with an unparseable query carries, kept verbatim. */
export const SPACE_BROKEN_SUBTITLE = "This space's query can't be read";

export function SpaceList({ vaultId }: { vaultId: string | null }) {
  const [spaces, setSpaces] = useState<NoteSpaceVm[]>([]);
  const activeSpaceId = useNotesFiltersStore((s) => (s.scope.kind === "space" ? s.scope.id : null));

  useEffect(() => {
    if (vaultId === null) {
      setSpaces([]);
      return;
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

  if (spaces.length === 0) {
    return null;
  }

  return (
    <section aria-label="Spaces" className="flex shrink-0 flex-col px-2 pb-1">
      <span className="px-2 py-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
        Spaces
      </span>
      <ul className="flex flex-col gap-0.5">
        {spaces.map((space) => {
          const broken = space.error !== null;
          const active = space.id === activeSpaceId;
          return (
            <li key={space.id}>
              <button
                type="button"
                aria-current={active ? "true" : undefined}
                aria-pressed={active}
                // The parse failure belongs in the accessible name too: a dot is
                // not a carrier on its own (UX-DR43).
                aria-label={broken ? `${space.name}, ${SPACE_BROKEN_SUBTITLE}` : space.name}
                className={cn(
                  "flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left outline-none",
                  "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                  active ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
                )}
                onClick={() =>
                  notesFiltersStore
                    .getState()
                    .setScope({ kind: "space", id: space.id, name: space.name })
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
                <span className="flex min-w-0 flex-col">
                  <span className="truncate text-sm">{space.name}</span>
                  {broken && (
                    <span className="truncate text-muted-foreground text-xs">
                      {SPACE_BROKEN_SUBTITLE}
                    </span>
                  )}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

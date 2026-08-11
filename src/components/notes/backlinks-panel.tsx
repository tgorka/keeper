/**
 * "Linked from" at the foot of the editor (Story 37.7, FR-108).
 *
 * Derived, never stored: the list is a projection of the core link graph, which
 * resolves through each note's ULID rather than its filename — which is why
 * renaming a note keeps every backlink working (FR-97).
 *
 * Hidden entirely at zero. An empty section that never fills is furniture, and
 * a note with no inbound links is the common case.
 */
import { useEffect, useState } from "react";
import { type NoteRowVm, notesBacklinks } from "@/lib/ipc/client";

export interface BacklinksPanelProps {
  vaultId: string;
  noteId: string;
  /**
   * Bumped by the surface when a change event touches this vault, so a link
   * written by another process shows up without a reload.
   */
  refreshKey?: number;
  /** Open one of the linking notes. */
  onOpen: (noteId: string) => void;
}

export function BacklinksPanel({ vaultId, noteId, refreshKey, onOpen }: BacklinksPanelProps) {
  const [rows, setRows] = useState<NoteRowVm[]>([]);

  // `refreshKey` is a re-run trigger, not a read: the surface bumps it when a change
  // event touches this vault, and without it a link an agent wrote never appears
  // until the note is reopened.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-run trigger, not a read
  useEffect(() => {
    let live = true;
    void notesBacklinks(vaultId, noteId)
      .then((found) => {
        if (live) {
          setRows(found);
        }
      })
      .catch(() => {
        // A failed projection is an absent section, never an error card: the
        // note itself is fine and this is the least important thing on screen.
        if (live) {
          setRows([]);
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, noteId, refreshKey]);

  if (rows.length === 0) {
    return null;
  }

  return (
    <section aria-label="Linked from" className="border-t px-3 py-2">
      {/* `text-muted-foreground`, not `text-faint`: the label carries a count,
          and a figure is a fact however label-shaped its surroundings are. */}
      <h2 className="figures label-caps text-muted-foreground">Linked from ({rows.length})</h2>
      <ul className="mt-1 flex flex-col gap-0.5">
        {rows.map((row) => (
          <li key={row.id}>
            <button
              type="button"
              className="w-full truncate text-left text-xs hover:underline"
              onClick={() => onOpen(row.id)}
            >
              <span>{row.title}</span>
              <span className="ml-2 text-muted-foreground">{row.snippet}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

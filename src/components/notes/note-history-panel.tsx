/**
 * Per-note history and diff (Story 38.5/38.4, FR-114, AD-63).
 *
 * There is no history store. Every revision here is projected from the commits
 * the sync engine already writes, and the device, origin and source on each row
 * are the `Keeper-*` trailers `provenance.rs` has been stamping since story
 * 28.1 — read back for a user for the first time. keeper adds no parallel audit
 * log, because git already holds the answer.
 *
 * It renders **in place of the editor**, not over it: history is read while
 * scanning the list, and a dialog would cover the list and force a decision.
 * `Back to editor` and Escape both return.
 */
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { type NoteDiffVm, type NoteRevisionVm, notesDiff, notesHistory } from "@/lib/ipc/client";

/** Bounded by decision: a 400-revision note paginates rather than revwalking
 *  itself into memory. */
export const NOTE_HISTORY_PAGE = 50;

export interface NoteHistoryPanelProps {
  vaultId: string;
  noteId: string;
  /** Return to the editor at the caret it left. */
  onBack: () => void;
}

export function NoteHistoryPanel({ vaultId, noteId, onBack }: NoteHistoryPanelProps) {
  const [revisions, setRevisions] = useState<NoteRevisionVm[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [diff, setDiff] = useState<NoteDiffVm | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    void notesHistory(vaultId, noteId, NOTE_HISTORY_PAGE)
      .then((found) => {
        if (!live) {
          return;
        }
        setRevisions(found);
        setSelected(found.length > 0 ? found[0].rev : null);
      })
      .catch(() => {
        if (live) {
          setFailure("keeper couldn't read this note's history.");
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, noteId]);

  useEffect(() => {
    if (selected === null) {
      setDiff(null);
      return;
    }
    let live = true;
    // `toRev: null` means the working tree, which is what "what changed in this
    // revision" means for the newest row.
    void notesDiff(vaultId, noteId, selected, null)
      .then((found) => {
        if (live) {
          setDiff(found);
        }
      })
      .catch(() => {
        if (live) {
          setDiff(null);
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, noteId, selected]);

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: Escape-returns-to-the-
    // editor is a surface-wide affordance; `Back to editor` is its labelled twin.
    <section
      aria-label="Note history"
      className="flex h-full flex-col"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onBack();
        }
      }}
    >
      <div className="flex items-center gap-2 border-b px-3 py-1.5">
        <Button size="sm" variant="ghost" onClick={onBack}>
          Back to editor
        </Button>
      </div>
      {failure === null ? null : (
        <p className="px-3 py-2 text-xs text-muted-foreground">{failure}</p>
      )}
      {failure === null && revisions.length === 0 ? (
        <p className="px-3 py-2 text-xs text-muted-foreground">
          No versions yet — the first one is written when this vault next commits.
        </p>
      ) : null}
      <div className="flex min-h-0 flex-1">
        <ul className="w-56 shrink-0 overflow-y-auto border-r">
          {revisions.map((revision) => (
            <li key={revision.rev}>
              <button
                type="button"
                aria-current={revision.rev === selected}
                className={`w-full px-3 py-1.5 text-left text-xs ${
                  revision.rev === selected ? "bg-muted" : ""
                }`}
                onClick={() => setSelected(revision.rev)}
              >
                <span className="block truncate">{revision.subject}</span>
                <span className="block truncate text-[11px] text-muted-foreground">
                  {revision.device} · {revision.origin} · {revision.source}
                </span>
              </button>
            </li>
          ))}
        </ul>
        <div className="min-w-0 flex-1 overflow-auto px-3 py-2 font-mono text-[11px]">
          {diff === null
            ? null
            : diff.hunks.map((hunk) => (
                <pre key={`${hunk.oldStart}-${hunk.newStart}`} className="whitespace-pre-wrap">
                  {hunk.text}
                </pre>
              ))}
        </div>
      </div>
    </section>
  );
}

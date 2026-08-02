/**
 * Note-list subscription and query lifecycle (Epic 37, Stories 37.2–37.5,
 * AD-8, AD-58).
 *
 * The single owner of what is in the note list. Two effects, and the split
 * between them is the whole design:
 *
 *   1. **The query.** Whenever the vault, the chip set or the window size
 *      changes, re-read `notes_list` and reset the mirror. Rust evaluates the
 *      filter; nothing here inspects a row.
 *   2. **The stream.** One `notes_subscribe_changes` per vault, torn down on
 *      cleanup — StrictMode double-mount, vault switch, unmount — so streams
 *      never leak and no batch from the old vault can land in the new one's
 *      window.
 *
 * How a streamed batch is applied depends on whether a filter is active, and
 * that is deliberate rather than defensive. `notes_subscribe_changes` is scoped
 * to a VAULT, not to a query, so its ops describe the whole vault. With no chip
 * set the vault and the window are the same thing and the ops apply verbatim,
 * which is what keeps the default lens live under an agent writing into it. With
 * a chip set they are not, and applying a vault-wide insert into a filtered
 * window would put a row on screen that does not match what the bar says — so a
 * batch becomes an invalidation and the query is re-run. Re-deriving the
 * predicate here instead would fork the query semantics between Rust and
 * TypeScript, which is exactly what AD-20 ruled out for the inbox.
 *
 * Re-running is cheap by construction: Rust already coalesces batches to at most
 * one per 250 ms per subscription, and the filter is a predicate sweep over an
 * in-memory index (NFR-28).
 *
 * A failed read leaves the previous rows on screen: the list is a projection,
 * and blanking a working list because one poll faulted is worse than showing a
 * list that is a second stale.
 */
import { useEffect } from "react";
import type { NoteChangeBatch, NoteListVm } from "@/lib/ipc/client";
import {
  notesList,
  notesSubscribeChanges,
  notesTree,
  notesUnsubscribeChanges,
} from "@/lib/ipc/client";
import {
  isFiltered,
  isFolderScope,
  noteQueryFor,
  notesFiltersStore,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesListStore, useNotesListStore } from "@/lib/stores/notes-list";

/**
 * Read the window Rust composes for the current chip set.
 *
 * The physical lens is the one scope that does not go through `notes_list`: a
 * vault-relative directory is not one of `NoteQueryReq`'s axes, so FR-106's own
 * command serves those rows. One folder level IS the whole set, so its `total`
 * is its own length — there is no window to be honest about.
 */
async function readWindow(vaultId: string): Promise<NoteListVm> {
  const filters = notesFiltersStore.getState();
  if (isFolderScope(filters.scope)) {
    const folder = await notesTree(vaultId, filters.scope.path);
    return { rows: folder.notes, total: folder.notes.length, offset: 0 };
  }
  const { limit } = notesListStore.getState();
  return await notesList(vaultId, noteQueryFor(filters, 0, limit));
}

/**
 * Keep the note-list mirror in step with one vault. Pass `null` when no vault is
 * active — the mirror is cleared and nothing is subscribed.
 */
export function useNotesChanges(vaultId: string | null): void {
  // The chip set as one string, so the query effect re-runs on a real change
  // rather than on every render that rebuilds an equal array.
  const filterKey = useNotesFiltersStore((s) =>
    JSON.stringify([s.scope, s.tags, s.text.trim(), s.agentOnly, s.pinnedOnly]),
  );
  const limit = useNotesListStore((s) => s.limit);

  // `filterKey` and `limit` are dependencies rather than reads: `readWindow` pulls
  // both out of their stores imperatively, so the effect body carries no store
  // subscription and the analyser cannot see that a filter or window change must
  // re-run it. Dropping them freezes the list on the first chip set it ever had.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-run triggers, not reads
  useEffect(() => {
    if (vaultId === null) {
      notesListStore.getState().clear();
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const vm = await readWindow(vaultId);
        // A read for the vault or filter we have since left must not paint: it
        // would put the previous scope's rows under the current bar.
        if (!cancelled) {
          notesListStore.getState().reset(vm);
        }
      } catch {
        // Keep whatever is on screen.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [vaultId, filterKey, limit]);

  useEffect(() => {
    if (vaultId === null) {
      return;
    }
    let cancelled = false;
    let subscriptionId: string | null = null;

    const onBatch = (batch: NoteChangeBatch) => {
      // A batch for another vault is a stream that has not finished tearing
      // down; dropping it is cheaper than racing the unsubscribe.
      if (cancelled || batch.vaultId !== vaultId) {
        return;
      }
      if (isFiltered(notesFiltersStore.getState())) {
        void (async () => {
          try {
            const vm = await readWindow(vaultId);
            if (!cancelled) {
              notesListStore.getState().reset(vm);
            }
          } catch {
            // Keep whatever is on screen.
          }
        })();
        return;
      }
      notesListStore.getState().applyBatch(batch);
    };

    void notesSubscribeChanges(vaultId, onBatch)
      .then((id) => {
        if (cancelled) {
          void notesUnsubscribeChanges(id);
          return;
        }
        subscriptionId = id;
      })
      .catch(() => {
        // A vault whose stream will not start still lists: the query effect
        // above is what paints the rows, and this only keeps them fresh.
      });

    return () => {
      cancelled = true;
      if (subscriptionId !== null) {
        void notesUnsubscribeChanges(subscriptionId);
      }
    };
  }, [vaultId]);
}

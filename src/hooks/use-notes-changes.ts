import { useEffect, useRef } from "react";
import type { NoteListVm } from "@/lib/ipc/client";
import {
  notesList,
  notesSubscribeChanges,
  notesTree,
  notesUnsubscribeChanges,
} from "@/lib/ipc/client";
import {
  isFolderScope,
  type NotesFiltersState,
  noteQueryFor,
  notesFiltersStore,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesListStore, useNotesListStore } from "@/lib/stores/notes-list";
import { syncErrorMessage } from "@/lib/stores/sync";

/** Every query axis belongs in the same key used to reject obsolete replies. */
function queryKey(state: NotesFiltersState): string {
  return JSON.stringify([state.scope, noteQueryFor(state, 0, 0)]);
}

async function readWindow(vaultId: string): Promise<NoteListVm> {
  const filters = notesFiltersStore.getState();
  if (isFolderScope(filters.scope)) {
    const folder = await notesTree(filters.scope.vaultId, filters.scope.path);
    return {
      rows: folder.notes,
      total: folder.notes.length,
      matched: folder.notes.length,
      hidden: 0,
      private: 0,
      notice: null,
      offset: 0,
    };
  }
  return await notesList(
    filters.scope.kind === "space" ? filters.scope.vaultId : vaultId,
    noteQueryFor(filters, 0, notesListStore.getState().limit),
  );
}

/** One Rust-composed page; every selected vault's stream invalidates that page. */
export function useNotesChanges(vaultId: string | null, ready = true): void {
  const filterKey = useNotesFiltersStore(queryKey);
  const vaultKey = useNotesFiltersStore((state) => JSON.stringify(state.vaultIds));
  const scopeVaultId = useNotesFiltersStore((state) =>
    state.scope.kind === "all" ? null : state.scope.vaultId,
  );
  const limit = useNotesListStore((state) => state.limit);
  const epoch = useRef(0);
  const refresh = useRef<(() => void) | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: these keys invalidate the imperatively read store snapshot
  useEffect(() => {
    if (!ready) return;
    if (vaultId === null) {
      notesListStore.getState().clear();
      return;
    }
    let cancelled = false;
    const load = (initial = false) => {
      const request = ++epoch.current;
      const key = queryKey(notesFiltersStore.getState());
      const requestedLimit = notesListStore.getState().limit;
      const current = () =>
        !cancelled &&
        request === epoch.current &&
        key === queryKey(notesFiltersStore.getState()) &&
        requestedLimit === notesListStore.getState().limit;
      notesListStore.setState({ searching: true, searchError: null });
      void readWindow(vaultId)
        .then((vm) => {
          if (current()) notesListStore.getState().reset(vm);
        })
        .catch((error: unknown) => {
          if (!current()) return;
          const sentence = syncErrorMessage(error, "Search could not be read. Try again.");
          if (!initial && !notesFiltersStore.getState().text.trim()) {
            notesListStore.setState({ searching: false, searchError: sentence });
          } else notesListStore.getState().failSearch(sentence);
        });
    };
    refresh.current = load;
    // Previous rows/counts must never masquerade as this new query while pending.
    notesListStore.getState().clear();
    notesListStore.setState({ limit });
    load(true);
    return () => {
      cancelled = true;
      epoch.current += 1;
      refresh.current = null;
    };
  }, [vaultId, ready, filterKey, limit]);

  useEffect(() => {
    if (!ready || vaultId === null) return;
    const selected: string[] = JSON.parse(vaultKey);
    const ids = [...new Set([vaultId, ...selected, ...(scopeVaultId ? [scopeVaultId] : [])])];
    const subscriptions: string[] = [];
    let cancelled = false;
    for (const id of ids) {
      void notesSubscribeChanges(id, (batch) => {
        if (cancelled || batch.vaultId !== id) return;
        notesFiltersStore.getState().requestSpacesReload();
        if (!selected.length || selected.includes(id) || id === scopeVaultId) refresh.current?.();
      })
        .then((subscription) => {
          if (cancelled) void notesUnsubscribeChanges(subscription);
          else subscriptions.push(subscription);
        })
        .catch(() => {
          // Subscription failure does not discard the independently loaded query.
        });
    }
    return () => {
      cancelled = true;
      for (const subscription of subscriptions) void notesUnsubscribeChanges(subscription);
    };
  }, [vaultId, vaultKey, scopeVaultId, ready]);
}

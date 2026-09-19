import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NoteSearchStateVm } from "@/lib/ipc/client";

export interface NotesSearchState {
  byVault: Record<string, NoteSearchStateVm>;
  apply: (vm: NoteSearchStateVm) => void;
}

/** Latest Rust search availability, independent of query text and list folding. */
export const notesSearchStateStore = createStore<NotesSearchState>()((set) => ({
  byVault: {},
  apply: (vm) => set((state) => ({ byVault: { ...state.byVault, [vm.vaultId]: vm } })),
}));

export function useNotesSearchState<T>(selector: (state: NotesSearchState) => T): T {
  return useStore(notesSearchStateStore, selector);
}

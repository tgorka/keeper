/**
 * Draft-presence marker store (Story 7.1, AD-15).
 *
 * A vanilla zustand store created at module load *outside* React. It holds a
 * presence-only set of `` `${accountId} ${roomId}` `` keys — which chats carry a
 * pending composer draft, **not** the bodies (each body lives in `keeper.db` and is
 * loaded on demand by the composer via `loadDraft`), so this stays small. It feeds
 * only the inbox row's amber pencil + "Draft" marker.
 *
 * The `drafts` table in `keeper.db` is the source of truth; this store is a mirror
 * seeded at startup (`applyKeys` from `listDrafts`) and updated live by the composer
 * (`mark` on the debounced keystroke path / on send / on clear). Cross-account.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** The composite marker key for a chat: `` `${accountId} ${roomId}` ``. */
function draftKey(accountId: string, roomId: string): string {
  return `${accountId} ${roomId}`;
}

export interface DraftsState {
  /** Keys (`` `${accountId} ${roomId}` ``) of chats with a non-empty draft. */
  keys: ReadonlySet<string>;
  /** Replace the whole set wholesale (startup seed from `listDrafts`). */
  applyKeys: (keys: Iterable<[string, string]>) => void;
  /** Add or remove a single chat's marker (`present` toggles it). */
  mark: (accountId: string, roomId: string, present: boolean) => void;
  /** Reset to the empty state (on full sign-out / unsubscribe). */
  clear: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const draftsStore = createStore<DraftsState>()((set) => ({
  keys: new Set<string>(),
  applyKeys: (keys) => set({ keys: new Set(Array.from(keys, ([a, r]) => draftKey(a, r))) }),
  mark: (accountId, roomId, present) =>
    set((state) => {
      const key = draftKey(accountId, roomId);
      // Skip the state churn (and re-render) when the marker already matches.
      if (state.keys.has(key) === present) {
        return state;
      }
      const next = new Set(state.keys);
      if (present) {
        next.add(key);
      } else {
        next.delete(key);
      }
      return { keys: next };
    }),
  clear: () => set({ keys: new Set<string>() }),
}));

/**
 * React selector hook: whether `(accountId, roomId)` has a pending draft. Subscribes
 * to just that one key's membership so an unrelated draft change never re-renders the
 * row.
 */
export function useHasDraft(accountId: string, roomId: string): boolean {
  return useStore(draftsStore, (state) => state.keys.has(draftKey(accountId, roomId)));
}

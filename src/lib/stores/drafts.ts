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

/**
 * A remote (cross-device) draft observed via the `dev.keeper.draft` mirror (Story
 * 7.2). Held only to *offer* adoption — local always wins; the composer never
 * auto-replaces non-empty local text. `body` is always non-empty (a tombstone is
 * removed from the map, not stored). `updatedTs` is informational only.
 */
export interface RemoteDraft {
  /** The remote draft body (always non-empty). */
  body: string;
  /** Write time in ms since the Unix epoch (UTC). Informational only. */
  updatedTs: number;
}

export interface DraftsState {
  /** Keys (`` `${accountId} ${roomId}` ``) of chats with a non-empty draft. */
  keys: ReadonlySet<string>;
  /**
   * Live remote drafts observed via the mirror subscription (Story 7.2), keyed by
   * `` `${accountId} ${roomId}` ``. A tombstone (empty/cleared remote) removes the
   * key, so a present entry always carries an adoptable body.
   */
  remote: ReadonlyMap<string, RemoteDraft>;
  /** Replace the whole presence set wholesale (startup seed from `listDrafts`). */
  applyKeys: (keys: Iterable<[string, string]>) => void;
  /** Add or remove a single chat's marker (`present` toggles it). */
  mark: (accountId: string, roomId: string, present: boolean) => void;
  /**
   * Apply a live remote-draft edit (Story 7.2). A non-empty `body` sets the key's
   * remote draft; a `null`/empty `body` (tombstone) removes it. Fed by the app-wide
   * mirror subscription. Local persistence is untouched — this only offers adoption.
   */
  applyRemote: (accountId: string, roomId: string, body: string | null, updatedTs: number) => void;
  /**
   * Drop every marker and remote draft belonging to `accountId` (Story 7.2). Called
   * on sign-out so a signed-out account's unsent text does not linger in memory (the
   * `remote` map holds actual bodies) and its inbox markers are cleared. Cross-account
   * entries are untouched.
   */
  clearAccount: (accountId: string) => void;
  /** Reset to the empty state (on full sign-out / unsubscribe). */
  clear: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const draftsStore = createStore<DraftsState>()((set) => ({
  keys: new Set<string>(),
  remote: new Map<string, RemoteDraft>(),
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
  applyRemote: (accountId, roomId, body, updatedTs) =>
    set((state) => {
      const key = draftKey(accountId, roomId);
      const present = state.remote.get(key);
      // A tombstone (null / empty body) removes the key; skip if already absent.
      if (body === null || body.length === 0) {
        if (present === undefined) {
          return state;
        }
        const next = new Map(state.remote);
        next.delete(key);
        return { remote: next };
      }
      // Skip the state churn (and re-render) when the body already matches — the
      // dedupe echo carrying only a new timestamp converges without a re-render.
      if (present !== undefined && present.body === body) {
        return state;
      }
      const next = new Map(state.remote);
      next.set(key, { body, updatedTs });
      return { remote: next };
    }),
  clearAccount: (accountId) =>
    set((state) => {
      // Keys are `` `${accountId} ${roomId}` `` and account ids carry no space, so a
      // `"<accountId> "` prefix match selects exactly this account's entries.
      const prefix = `${accountId} `;
      const keys = new Set(Array.from(state.keys).filter((k) => !k.startsWith(prefix)));
      const remote = new Map(Array.from(state.remote).filter(([k]) => !k.startsWith(prefix)));
      // Only allocate new collections when something actually changed.
      if (keys.size === state.keys.size && remote.size === state.remote.size) {
        return state;
      }
      return { keys, remote };
    }),
  clear: () => set({ keys: new Set<string>(), remote: new Map<string, RemoteDraft>() }),
}));

/**
 * React selector hook: whether `(accountId, roomId)` has a pending draft. Subscribes
 * to just that one key's membership so an unrelated draft change never re-renders the
 * row.
 */
export function useHasDraft(accountId: string, roomId: string): boolean {
  return useStore(draftsStore, (state) => state.keys.has(draftKey(accountId, roomId)));
}

/**
 * React selector hook: the live remote draft for `(accountId, roomId)`, or `undefined`
 * when none is offered (Story 7.2). Subscribes to just that one key's remote entry so
 * an unrelated remote edit never re-renders the composer. Feeds the local-wins conflict
 * chip — the composer offers this for one-tap adoption, never auto-replacing local text.
 */
export function useRemoteDraft(accountId: string, roomId: string): RemoteDraft | undefined {
  return useStore(draftsStore, (state) => state.remote.get(draftKey(accountId, roomId)));
}

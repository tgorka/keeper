/**
 * Incognito effective-state mirror store (Story 8.1).
 *
 * A vanilla zustand store created at module load *outside* React. It mirrors the
 * Rust-resolved Incognito view models — it is NOT the source of truth. The three
 * scope values (global / per-Account / per-Chat) live in `keeper.db`; the effective
 * precedence (Chat > Account > Global) is resolved *inside* the Rust `signals` seam
 * and delivered as an {@link IncognitoVm}. This store only caches the last-observed
 * VM per `` `${accountId} ${roomId}` `` (plus the global bool and per-account values,
 * derived from those VMs) so the header chip and composer ring can render without a
 * fetch on every paint. Never resolve precedence here — always render `effective`.
 *
 * Any mutation (the settings switch, the account menu, the header chip) writes through
 * the IPC command and then calls {@link refreshIncognito} to re-read the authoritative
 * VM back into this mirror.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { type IncognitoVm, incognitoGet } from "@/lib/ipc/client";

/** The composite key for a chat: `` `${accountId} ${roomId}` ``. */
function chatKey(accountId: string, roomId: string): string {
  return `${accountId} ${roomId}`;
}

export interface IncognitoState {
  /**
   * Last-observed effective VM per `` `${accountId} ${roomId}` ``. Seeded/updated by
   * {@link refreshIncognito}; never authoritative (the Rust core resolves precedence).
   */
  byChat: ReadonlyMap<string, IncognitoVm>;
  /** The global default, mirrored from the most recent VM read (off by default). */
  global: boolean;
  /**
   * Per-account overrides, mirrored from the most recent VM read for each account
   * (`true`/`false` = explicit override, absent = inherit). Informational for the
   * account menu's tri-state item.
   */
  byAccount: ReadonlyMap<string, boolean | null>;
  /**
   * Monotonic counter bumped whenever a *broader* scope (global or per-account) is
   * mutated. The open chat's refresh effect depends on it, so a Settings or account-menu
   * toggle re-reads the currently-open chat's effective VM without waiting for a room
   * reopen — the chip and composer ring reconcile immediately.
   */
  policyVersion: number;
  /** Store a freshly-read VM for `(accountId, roomId)`, updating the derived scopes. */
  applyVm: (accountId: string, roomId: string, vm: IncognitoVm) => void;
  /** Store just the global default (settings switch mirror, before a chat re-read). */
  applyGlobal: (global: boolean) => void;
  /** Bump {@link policyVersion} to trigger an open-chat re-read after a broad-scope write. */
  bumpPolicyVersion: () => void;
  /** Reset to the empty state (on full sign-out). */
  clear: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const incognitoStore = createStore<IncognitoState>()((set) => ({
  byChat: new Map<string, IncognitoVm>(),
  global: false,
  byAccount: new Map<string, boolean | null>(),
  policyVersion: 0,
  applyVm: (accountId, roomId, vm) =>
    set((state) => {
      const key = chatKey(accountId, roomId);
      const byChat = new Map(state.byChat);
      byChat.set(key, vm);
      const byAccount = new Map(state.byAccount);
      byAccount.set(accountId, vm.account);
      return { byChat, global: vm.global, byAccount };
    }),
  applyGlobal: (global) => set((state) => (state.global === global ? state : { global })),
  bumpPolicyVersion: () => set((state) => ({ policyVersion: state.policyVersion + 1 })),
  clear: () =>
    set({
      byChat: new Map<string, IncognitoVm>(),
      global: false,
      byAccount: new Map<string, boolean | null>(),
    }),
}));

/**
 * Re-read the authoritative Incognito VM for `(accountId, roomId)` from the Rust core
 * and mirror it into the store (Story 8.1). Called after any scope mutation so the chip
 * and ring reflect the resolved effective state. Fire-and-forget on failure — a read
 * error leaves the last-observed VM in place rather than flashing a wrong state.
 */
export async function refreshIncognito(
  accountId: string,
  roomId: string,
  isCancelled?: () => boolean,
): Promise<void> {
  try {
    const vm = await incognitoGet(accountId, roomId);
    // Drop a read that resolved after its caller moved on (e.g. a fast room switch),
    // so a stale in-flight VM never clobbers the newer selection's mirrored state.
    if (isCancelled?.()) {
      return;
    }
    incognitoStore.getState().applyVm(accountId, roomId, vm);
  } catch {
    // Best-effort mirror refresh — never surface a UI error for a receipt-policy read.
  }
}

/**
 * React selector hook: the mirrored effective VM for `(accountId, roomId)`, or
 * `undefined` when none has been read yet. Subscribes to just that one key so an
 * unrelated chat's Incognito change never re-renders this consumer.
 */
export function useIncognito(
  accountId: string | null,
  roomId: string | null,
): IncognitoVm | undefined {
  return useStore(incognitoStore, (state) =>
    accountId === null || roomId === null
      ? undefined
      : state.byChat.get(chatKey(accountId, roomId)),
  );
}

/**
 * React selector hook: the mirrored global Incognito default. Subscribes to just the
 * global bool so an unrelated per-chat read never re-renders this consumer.
 */
export function useGlobalIncognito(): boolean {
  return useStore(incognitoStore, (state) => state.global);
}

/**
 * React selector hook: the monotonic {@link IncognitoState.policyVersion}. An open
 * chat depends on it so a broad-scope (global / per-account) toggle re-reads its
 * effective VM immediately, without waiting for a room reopen.
 */
export function useIncognitoPolicyVersion(): number {
  return useStore(incognitoStore, (state) => state.policyVersion);
}

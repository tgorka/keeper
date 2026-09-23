/**
 * The optional account's mirror (Epic 82, AD-308…AD-316).
 *
 * A vanilla zustand store created at module load, holding exactly what Rust
 * last pushed over `account_subscribe` (or answered from a command) — never a
 * source of truth, never a token. Every sentence a surface shows about the
 * account is `vm.sentence`, composed in Rust.
 *
 * Snapshots arrive by two roads — the subscription's pushes and each
 * command's own answer — and nothing orders one against the other: Settings
 * opened mid-sign-in can receive `account_state`'s `signingIn` after the
 * subscription has already pushed `syncing`. Rust stamps every snapshot with
 * a per-process `revision` that only grows, so the store keeps the newest and
 * drops an older one whichever road it came by.
 *
 * It also holds the one piece of state that is the webview's own: which setup
 * link the confirmation sheet is open on. Settings, the first-run wizard and a
 * `keeper://setup` deep link all open the SAME sheet through
 * {@link AccountState.openSetup}, so there is one sheet and one set of words
 * for "check these hosts before you continue", whichever way somebody arrived.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { OrgAccountVm } from "@/lib/ipc/client";

/**
 * What the store holds before Rust has answered, and what an install with no
 * account answers: not configured, nothing else. Surfaces read it as "no
 * account", which is the one state that must look exactly like keeper did
 * before accounts existed.
 */
export const NO_ACCOUNT: OrgAccountVm = {
  configured: false,
  id: null,
  name: null,
  issuerHost: null,
  repoHost: null,
  repoMode: null,
  state: "none",
  sentence: null,
  identity: null,
  device: null,
  devices: [],
  lastSyncedMs: null,
  forgeConnected: false,
  faults: [],
  revision: 0,
};

export interface AccountState {
  /** The account exactly as Rust last described it. */
  vm: OrgAccountVm;
  /** The link the setup sheet is open on, or `null` while it is closed. */
  setupLink: string | null;
  /** Record a snapshot from the subscription or a command's answer, unless a newer one is held. */
  setVm: (vm: OrgAccountVm) => void;
  /** Open the confirmation sheet on a pasted, scanned or deep-linked setup link. */
  openSetup: (link: string) => void;
  /** Close the confirmation sheet. */
  closeSetup: () => void;
}

export const accountStore = createStore<AccountState>()((set) => ({
  vm: NO_ACCOUNT,
  setupLink: null,
  setVm: (vm) => set((state) => (vm.revision < state.vm.revision ? state : { vm })),
  openSetup: (link) => set({ setupLink: link }),
  closeSetup: () => set({ setupLink: null }),
}));

/** React selector hook over {@link accountStore}. */
export function useAccountStore<T>(selector: (state: AccountState) => T): T {
  return useStore(accountStore, selector);
}

/**
 * Whether the account can stand in as a credential right now (AD-315): it is
 * signed in and nothing is refusing it. `syncing` counts — a repository fetch
 * in flight does not make the token any less good, and a choice that vanished
 * from a form for the seconds a sync takes would read as a bug. Offline,
 * sign-in-again and blocked do not: offering a credential that cannot be
 * obtained is offering a failure.
 */
export function accountUsable(vm: OrgAccountVm): boolean {
  return vm.identity !== null && (vm.state === "ready" || vm.state === "syncing");
}

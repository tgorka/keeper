/**
 * Per-account encryption-status mirror store (Story 3.1, AD-8).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the Rust-authoritative {@link EncryptionStatus} for every tracked account,
 * keyed by opaque account id — never a source of truth. An account absent from
 * the map (`undefined`) means "no status batch yet" (crypto not reported). It
 * also holds the session-scoped `bannerDismissed` nag preference (a pure UI
 * flag; NOT persisted — it resets on restart, re-nudging a still-unverified
 * device, which is acceptable security UX).
 *
 * The "verify this device" banner, the Settings badge, and the Settings
 * Encryption section are all pure projections of this single slice.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { EncryptionStatus } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";

export interface EncryptionStatusState {
  /** The current device-verification status per account id, exactly as Rust
   * streamed it. An account absent from the map has not delivered a batch yet. */
  statuses: Record<string, EncryptionStatus>;
  /** Whether the user dismissed the verify banner this session (session-scoped,
   * never persisted). Collapses the banner to the Settings badge. */
  bannerDismissed: boolean;
  /** Record one account's current status (from a streamed batch). */
  setStatus: (accountId: string, status: EncryptionStatus) => void;
  /** Drop one account's entry (sign-out / subscription teardown). */
  removeAccount: (accountId: string) => void;
  /** Dismiss the verify banner for this session. */
  dismissBanner: () => void;
  /** Clear every tracked account's status and reset the dismissal. */
  reset: () => void;
}

/** The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for verification state stays in Rust. */
export const encryptionStatusStore = createStore<EncryptionStatusState>()((set) => ({
  statuses: {},
  bannerDismissed: false,
  setStatus: (accountId, status) =>
    set((state) => {
      const statuses = { ...state.statuses, [accountId]: status };
      // A device *newly* entering the `unverified` state (a genuinely new
      // verification need — a re-login regression or an account added after an
      // earlier dismissal) re-surfaces the banner even if it was dismissed this
      // session. Without this, one account's dismissal would silently pre-hide
      // the nag for a later unverified account, leaving only the subtler badge.
      const newlyUnverified = status === "unverified" && state.statuses[accountId] !== "unverified";
      if (newlyUnverified && state.bannerDismissed) {
        return { statuses, bannerDismissed: false };
      }
      return { statuses };
    }),
  removeAccount: (accountId) =>
    set((state) => {
      if (!(accountId in state.statuses)) {
        return state;
      }
      const { [accountId]: _removed, ...rest } = state.statuses;
      return { statuses: rest };
    }),
  dismissBanner: () => set({ bannerDismissed: true }),
  reset: () => set({ statuses: {}, bannerDismissed: false }),
}));

/**
 * The encryption status for a single account, or `undefined` when no status
 * batch has arrived yet. A subscription hook over {@link encryptionStatusStore}.
 */
export function useEncryptionStatus(accountId: string): EncryptionStatus | undefined {
  return useStore(encryptionStatusStore, (s) => s.statuses[accountId]);
}

/**
 * Whether at least one signed-in account's device is `Unverified`. Ranges over
 * the signed-in account set; a `Verified`, `Unknown`, or pending (`undefined`)
 * account never trips this. The banner never shows on `Unknown` — only an
 * explicit `Unverified` counts (avoid a false nag before crypto has synced).
 */
export function useAnyUnverified(): boolean {
  const accounts = useAccountsStore((s) => s.accounts);
  const statuses = useStore(encryptionStatusStore, (s) => s.statuses);
  return accounts.some((a) => statuses[a.accountId] === "unverified");
}

/**
 * Whether the global verify banner should show: any account unverified AND the
 * banner has not been dismissed this session.
 */
export function useShowVerifyBanner(): boolean {
  const anyUnverified = useAnyUnverified();
  const dismissed = useStore(encryptionStatusStore, (s) => s.bannerDismissed);
  return anyUnverified && !dismissed;
}

/**
 * Whether the persistent Settings badge should show anywhere: any account
 * unverified AND the banner has been dismissed this session (it collapses to a
 * badge, not gone). Prefer {@link useShowVerifyBadgeForAccount} for a per-row
 * badge so a verified account never shows another account's badge.
 */
export function useShowVerifyBadge(): boolean {
  const anyUnverified = useAnyUnverified();
  const dismissed = useStore(encryptionStatusStore, (s) => s.bannerDismissed);
  return anyUnverified && dismissed;
}

/**
 * Whether the persistent verify badge should show for ONE specific account: that
 * account is explicitly `Unverified` AND the banner has been dismissed this
 * session. Account-scoped so a verified (or pending/unknown) account's switcher
 * row never displays a badge that belongs to a different, unverified account.
 */
export function useShowVerifyBadgeForAccount(accountId: string): boolean {
  const status = useStore(encryptionStatusStore, (s) => s.statuses[accountId]);
  const dismissed = useStore(encryptionStatusStore, (s) => s.bannerDismissed);
  return status === "unverified" && dismissed;
}

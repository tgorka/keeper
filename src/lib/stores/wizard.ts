/**
 * First-run wizard store (Story 6.8, FR-first-run).
 *
 * A vanilla zustand store created at module load *outside* React so the boot
 * auto-start (in `App`) and the Settings "Run setup again" entry can drive the
 * single always-reachable wizard surface imperatively via `getState()`, without
 * prop-drilling (mirrors {@link import("./add-account").addAccountStore} and
 * {@link import("./new-chat").newChatStore}).
 *
 * The state is session-scoped and intentionally throwaway — it is NEVER persisted.
 * `dismissed` is set only by this store's own {@link WizardState.finish} when the
 * user skips the whole flow with zero accounts, so it can never regress a
 * sign-out-of-last-account back-to-login path (only the wizard sets it). No token,
 * session, or homeserver material is ever held here — Rust is the source of truth.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { accountsStore } from "@/lib/stores/accounts";

/** The ordered steps of the first-run wizard. */
export type WizardStep = "welcome" | "addAccount" | "discovery" | "done";

export interface WizardState {
  /** Whether the wizard surface is currently shown (takes precedence over the shell). */
  active: boolean;
  /**
   * Whether the wizard was skipped/finished with zero accounts this session, so
   * `App` should land in an empty inbox instead of re-showing the login screen.
   * Set only by {@link WizardState.finish}; never persisted.
   */
  dismissed: boolean;
  /** The step currently rendered. */
  step: WizardStep;
  /** The account the discovery step runs for, or `null` before one is chosen. */
  accountId: string | null;
  /**
   * Start (or restart) the wizard. With zero accounts it opens at the welcome
   * step (genuine first run). When accounts already exist (Settings re-entry) it
   * opens directly at the discovery step for the first account, since the whole
   * point of re-entry is to set up more bridges for an account you already have —
   * the linear welcome→addAccount path would otherwise force a redundant new sign-in.
   */
  start: () => void;
  /** Move to a specific step. */
  goTo: (step: WizardStep) => void;
  /** Record the account whose bridges the discovery step should probe. */
  setAccountId: (id: string) => void;
  /** Close the wizard; with zero accounts also mark it dismissed for this session. */
  finish: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const wizardStore = createStore<WizardState>()((set) => ({
  active: false,
  dismissed: false,
  step: "welcome",
  accountId: null,
  start: () => {
    // Re-entry with an existing account lands on discovery for that account so
    // the user can set up bridges without a redundant sign-in; a true first run
    // (zero accounts) starts at welcome. Consistent with the intent contract's
    // "accountId defaults to first account" re-entry row.
    const existing = accountsStore.getState().accounts[0];
    set(
      existing
        ? { active: true, step: "discovery", dismissed: false, accountId: existing.accountId }
        : { active: true, step: "welcome", dismissed: false, accountId: null },
    );
  },
  goTo: (step) => set({ step }),
  setAccountId: (id) => set({ accountId: id }),
  finish: () =>
    set({
      active: false,
      // Only a skip/finish with zero accounts lands the user in an empty inbox;
      // otherwise the newly-added account renders the shell normally.
      dismissed: accountsStore.getState().accounts.length === 0,
    }),
}));

/**
 * React selector hook over {@link wizardStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useWizardStore<T>(selector: (state: WizardState) => T): T {
  return useStore(wizardStore, selector);
}

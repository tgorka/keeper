/**
 * Interactive device-verification modal store (Story 3.2, FR-14, AD-1).
 *
 * A vanilla zustand store created at module load *outside* React. It mirrors the
 * Rust-authoritative {@link VerificationFlowVm} for the single active flow (there
 * is at most one verification modal open at a time) and holds the pure UI open
 * state — never a source of truth for the flow's crypto, which lives entirely in
 * Rust. The modal is a pure renderer of `flow.phase`; every action is a one-shot
 * IPC call keyed by `flow.flowId`.
 *
 * `openFor(accountId)` opens the modal for an account (e.g. from the Settings
 * Verify button, or auto-opened by an incoming request). `close()` fires a
 * best-effort cancel of the active flow and clears it. `setFlow()` records the
 * latest streamed snapshot.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { VerificationFlowVm } from "@/lib/ipc/client";
import { verificationCancel } from "@/lib/ipc/client";

export interface VerificationState {
  /** The current flow snapshot exactly as Rust streamed it, or `null` when no
   * flow is active (waiting for the first snapshot after opening). */
  flow: VerificationFlowVm | null;
  /** Whether the verification modal is open. */
  modalOpen: boolean;
  /** The account the active modal belongs to, or `null` when closed. */
  activeAccountId: string | null;
  /** Open the modal for an account (Settings Verify, or an incoming request). */
  openFor: (accountId: string) => void;
  /** Close the modal: best-effort cancel the active flow, then clear it. */
  close: () => void;
  /** Record the latest streamed flow snapshot. */
  setFlow: (flow: VerificationFlowVm) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const verificationStore = createStore<VerificationState>()((set, get) => ({
  flow: null,
  modalOpen: false,
  activeAccountId: null,
  openFor: (accountId) => set({ modalOpen: true, activeAccountId: accountId, flow: null }),
  close: () => {
    const { activeAccountId, flow } = get();
    // Best-effort cancel of a still-cancellable flow so we never leave a dangling
    // request on the SDK when the user dismisses the modal (the Rust core no-ops a
    // missing/terminal flow). Failures are swallowed — the modal still closes.
    if (activeAccountId && flow && shouldCancelOnClose(flow.phase)) {
      void verificationCancel(activeAccountId, flow.flowId).catch(() => {});
    }
    set({ modalOpen: false, activeAccountId: null, flow: null });
  },
  setFlow: (flow) => set({ flow }),
}));

/**
 * Whether dismissing the modal should best-effort cancel the flow. Terminal
 * phases (`done`/`cancelled`/`failed`) need no cancel; `confirmed` is excluded too
 * because our SAS confirmation is already sent — the verification can still
 * complete after the modal closes, so cancelling would needlessly abort a
 * near-complete verification.
 */
function shouldCancelOnClose(phase: VerificationFlowVm["phase"]): boolean {
  return phase !== "done" && phase !== "cancelled" && phase !== "failed" && phase !== "confirmed";
}

/** Subscribe to the active flow snapshot (or `null`). */
export function useVerificationFlow(): VerificationFlowVm | null {
  return useStore(verificationStore, (s) => s.flow);
}

/** Subscribe to whether the verification modal is open. */
export function useVerificationModalOpen(): boolean {
  return useStore(verificationStore, (s) => s.modalOpen);
}

/** Subscribe to the account the active modal belongs to (or `null`). */
export function useActiveVerificationAccountId(): string | null {
  return useStore(verificationStore, (s) => s.activeAccountId);
}

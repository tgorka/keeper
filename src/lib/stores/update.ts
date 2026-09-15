/**
 * The app-update flow's one state, shared by the two surfaces that drive it.
 *
 * Before background updates there was only one driver — the About section's own
 * `useState` — and that was enough because nothing else could move the flow.
 * A background loop changes that: it checks and installs while Settings is
 * closed, and what it found has to still be there when the person opens About
 * ("Update 0.9.0 installed. Restart keeper to finish."). A store outside React
 * is also what lets the loop refuse to start a second download over one that is
 * already in flight from a click.
 *
 * Deliberately NOT the source of truth for whether an update exists — that is
 * `@tauri-apps/plugin-updater` and the signed manifest behind it. This is the
 * rendered state of the flow, and the pending {@link Update} handle rides along
 * so the two-step manual path can install exactly the build it detected.
 */
import type { Update } from "@tauri-apps/plugin-updater";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/**
 * The states of the update flow. Every path — including a failed check, a
 * failed download and a failed relaunch — is a rendered state; none is a
 * console-only error.
 *
 * `installedNeedsRestart` carries the version when the background loop
 * installed it (nobody watched that happen, so the sentence names the build)
 * and `null` after the manual two-step flow, whose sentence already followed a
 * click on the version it names.
 */
export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; version: string }
  | { kind: "installedNeedsRestart"; version: string | null }
  | { kind: "error"; message: string };

export interface UpdateState {
  /** The rendered state of the flow, wherever it was driven from. */
  phase: UpdatePhase;
  /**
   * The detected-but-not-installed update, held between the manual flow's two
   * clicks. Not rendered — consumed by the install step, and cleared as it is
   * consumed so a double-click cannot start two downloads of one handle.
   */
  pending: Update | null;
  /** Move the flow to `phase`. */
  setPhase: (phase: UpdatePhase) => void;
  /** Record (or clear) the update awaiting an explicit install click. */
  setPending: (pending: Update | null) => void;
  /** Back to the start: a fresh Settings open, with no stale prior result. */
  reset: () => void;
}

/**
 * The vanilla store instance, created once at module load so the background
 * loop and the About section read and write the same flow.
 */
export const updateStore = createStore<UpdateState>()((set) => ({
  phase: { kind: "idle" },
  pending: null,
  setPhase: (phase) => set({ phase }),
  setPending: (pending) => set({ pending }),
  reset: () => set({ phase: { kind: "idle" }, pending: null }),
}));

/**
 * Whether the flow is mid-flight — a check or a download somebody or something
 * else already started. Both drivers consult this before starting their own:
 * the background loop skips its cycle, and the manual buttons disable.
 */
export function isUpdateBusy(phase: UpdatePhase): boolean {
  return phase.kind === "checking" || phase.kind === "downloading";
}

/** React selector hook over {@link updateStore}. */
export function useUpdateStore<T>(selector: (state: UpdateState) => T): T {
  return useStore(updateStore, selector);
}

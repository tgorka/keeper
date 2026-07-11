/**
 * Frontend lifecycle-phase store (Story 14.5, AD-30) — the single frontend
 * lifecycle truth the media shed derives from.
 *
 * A vanilla zustand store created at module load *outside* React, mirroring the
 * {@link capabilitiesStore} pattern. It holds one datum — the current lifecycle
 * `phase` — written by {@link useAppLifecycle} from the SOLE `visibilitychange`
 * listener that hook already owns. There is deliberately no second listener: one
 * visibility signal, one lifecycle truth.
 *
 * The default `"foreground"` is load-bearing: on desktop (and before the reduced
 * tier hydrates) `useAppLifecycle` attaches no listener and never writes the
 * store, so the phase stays `"foreground"` forever and {@link useMediaShed}
 * reads `false` — desktop media rendering is byte-identical to today.
 *
 * The `phase === "background"` derivation expresses exactly "shed while the app
 * is backgrounded". It does NOT express a "shed while still foregrounded" state:
 * a future native `didReceiveMemoryWarning` plugin (deferred, AD-30) would need
 * to *extend* this store (e.g. an explicit shed flag independent of `phase`) to
 * cover a memory-warning-while-visible case — this store is a clean seam for
 * that path, not already-complete coverage of it.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** The two lifecycle phases the frontend distinguishes for media shedding. */
export type LifecyclePhase = "foreground" | "background";

export interface LifecycleState {
  /** The current lifecycle phase; `"foreground"` until a background transition. */
  phase: LifecyclePhase;
  /** Replace the current phase (called by {@link useAppLifecycle}). */
  setPhase: (phase: LifecyclePhase) => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the app.
 * It is a module-load singleton — any test that mutates it must reset it to
 * `"foreground"` afterward to avoid order-dependent cross-contamination.
 */
export const lifecycleStore = createStore<LifecycleState>()((set) => ({
  phase: "foreground",
  setPhase: (phase) => set({ phase }),
}));

/**
 * React selector hook over {@link lifecycleStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useLifecycleStore<T>(selector: (state: LifecycleState) => T): T {
  return useStore(lifecycleStore, selector);
}

/**
 * The media-shed selector: `true` while the app is backgrounded, so image
 * renderers drop their decoded-bitmap `src`. On desktop the phase never leaves
 * `"foreground"`, so this is permanently `false`.
 */
export function useMediaShed(): boolean {
  return useLifecycleStore((state) => state.phase === "background");
}

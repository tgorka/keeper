/**
 * Segmentation-settings mirror store (Story 17.5, FR-72).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `incognito.ts` precedent). It mirrors the Rust-resolved
 * {@link RecordingSettingsVm} — it is NOT the source of truth. Both values live
 * in `keeper.db` behind `keeper_core::registry` (defaulted + clamped there);
 * `recording_start` re-reads them from the registry at start time, so edits
 * apply to the next Recording Session only and never mutate a running one.
 *
 * Both settings surfaces (Settings → Recording and the pre-record "Segmenting"
 * setup card) bind to this one store, so editing either writes the same value
 * and both reflect it live. {@link ensureRecordingSettingsHydrated} lazily
 * hydrates once; {@link applyRecordingSettings} writes optimistically, replaces
 * the mirror with the effective (Rust-clamped) VM once the persist lands, and
 * reverts on failure — guarded by a monotonic write token so a stale rejection
 * never clobbers a newer edit.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type RecordingSettingsVm,
  recordingSettingsGet,
  recordingSettingsSet,
} from "@/lib/ipc/client";

/** Default segment size in MB (mirrors the Rust registry default). */
export const RECORDING_SEGMENT_MB_DEFAULT = 500;
/** Smallest accepted segment size in MB (mirrors the Rust clamp floor). */
export const RECORDING_SEGMENT_MB_MIN = 100;
/** Largest accepted segment size in MB (mirrors the Rust clamp ceiling). */
export const RECORDING_SEGMENT_MB_MAX = 5000;
/** Default duration cap in minutes (mirrors the Rust registry default). */
export const RECORDING_DURATION_CAP_MINUTES_DEFAULT = 30;
/** Smallest accepted duration cap in minutes (mirrors the Rust clamp floor). */
export const RECORDING_DURATION_CAP_MINUTES_MIN = 1;
/** Largest accepted duration cap in minutes (mirrors the Rust clamp ceiling). */
export const RECORDING_DURATION_CAP_MINUTES_MAX = 600;
/** Default capture frame rate (Story 19.5; mirrors the Rust registry default). */
export const RECORDING_FPS_DEFAULT = 30;
/** The only legal frame rates (Story 19.5; mirrors the Rust normalize set —
 * anything else is normalized to the default backend-side). */
export const RECORDING_FPS_ALLOWED: readonly number[] = [30, 60];

/** The legal codec set (Story 21.1) — mirror of the Rust normalization. */
export const RECORDING_CODEC_ALLOWED: readonly string[] = ["h264", "hevc"];

/** The legal capture-scale set (Story 21.2) — mirror of the Rust normalization. */
export const RECORDING_SCALE_ALLOWED: readonly number[] = [100, 75, 50];

export interface RecordingSettingsState {
  /**
   * The last-observed effective VM, or `null` before the first hydration
   * resolves (controls render disabled until then — never a fake value).
   */
  settings: RecordingSettingsVm | null;
  /** Replace the mirrored VM (hydration, effective-persist echo, or revert). */
  setSettings: (settings: RecordingSettingsVm | null) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const recordingSettingsStore = createStore<RecordingSettingsState>()((set) => ({
  settings: null,
  setSettings: (settings) => set({ settings }),
}));

/** In-flight hydration, deduped so concurrent surfaces trigger one read. */
let hydration: Promise<void> | null = null;

/**
 * Monotonic write token: bumped on every {@link applyRecordingSettings}, so a
 * slow persist (success echo or failure revert) that lost to a newer edit is
 * dropped instead of clobbering the newer optimistic value.
 */
let writeId = 0;

/**
 * The last Rust-confirmed VM (from hydration or an effective-persist echo). It
 * is the revert target on a failed write — reverting to the *live* store value
 * would restore a still-optimistic, never-confirmed value when two edits race.
 */
let lastConfirmed: RecordingSettingsVm | null = null;

/**
 * Lazily hydrate the mirror from `recordingSettingsGet` (once per app lifetime;
 * concurrent callers share one read). Called by each surface on mount/open.
 * Best-effort: a read failure leaves the store unhydrated (controls stay
 * disabled) and allows a retry on the next call.
 */
export async function ensureRecordingSettingsHydrated(): Promise<void> {
  if (recordingSettingsStore.getState().settings !== null) {
    return;
  }
  hydration ??= recordingSettingsGet()
    .then((vm) => {
      // Never clobber an optimistic edit that landed while hydrating (the
      // controls are `disabled` until hydration lands, so in practice
      // `writeId` is still 0 here).
      if (writeId === 0) {
        lastConfirmed = vm;
        recordingSettingsStore.getState().setSettings(vm);
      }
    })
    .catch(() => {
      // Allow a later retry rather than caching the failure forever.
      hydration = null;
    });
  await hydration;
}

/**
 * Persist new segmentation settings (Story 17.5): optimistic mirror update,
 * then `recordingSettingsSet`; on success the mirror is replaced with the
 * effective (Rust-clamped) VM, on failure it reverts to the prior value — both
 * only when no newer write superseded this one.
 */
export async function applyRecordingSettings(next: RecordingSettingsVm): Promise<void> {
  writeId += 1;
  const id = writeId;
  // Revert to the last *confirmed* value, not the live (possibly optimistic)
  // one, so a failed write during a rapid double-edit restores a real value.
  const revertTo = lastConfirmed;
  recordingSettingsStore.getState().setSettings(next);
  try {
    const effective = await recordingSettingsSet(next);
    if (id === writeId) {
      lastConfirmed = effective;
      recordingSettingsStore.getState().setSettings(effective);
    }
  } catch {
    if (id === writeId) {
      recordingSettingsStore.getState().setSettings(revertTo);
    }
  }
}

/**
 * React selector hook: the mirrored effective settings, or `null` while the
 * first hydration is still in flight.
 */
export function useRecordingSettings(): RecordingSettingsVm | null {
  return useStore(recordingSettingsStore, (state) => state.settings);
}

/** Test-only reset: clear the mirror and forget any in-flight hydration/write. */
export function resetRecordingSettingsForTest(): void {
  hydration = null;
  writeId = 0;
  lastConfirmed = null;
  recordingSettingsStore.getState().setSettings(null);
}

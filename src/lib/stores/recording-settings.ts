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
import { syncErrorMessage } from "@/lib/stores/sync";

/** Last-resort message when a save rejection carries no readable sentence. */
export const RECORDING_SETTINGS_UNKNOWN_ERROR = "keeper could not save these recording settings.";

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
export const RECORDING_FPS_ALLOWED: readonly number[] = [10, 15, 30, 60];

/** The legal codec set (Story 21.1) — mirror of the Rust normalization. */
export const RECORDING_CODEC_ALLOWED: readonly string[] = ["h264", "hevc"];

/** The legal capture-scale set (Story 21.2) — mirror of the Rust normalization. */
export const RECORDING_SCALE_ALLOWED: readonly number[] = [100, 75, 50, 25];

/**
 * The default recording path template (Story 40.2; mirrors the Rust
 * `DEFAULT_TEMPLATE`).
 *
 * Safe to mirror because it is UI copy, not a rule: it is what the template
 * field shows as its placeholder, which is honest precisely because a blank
 * template falls back to this same default in Rust. It is never a second
 * renderer — every rendered path, every fallback that actually decides
 * anything, and every refusal sentence comes from Rust, over
 * `recordingPathPreview` and `recordingSettingsSet`.
 */
export const RECORDING_PATH_TEMPLATE_DEFAULT = "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}";

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
 * Re-read the effective settings from Rust unconditionally, where
 * {@link ensureRecordingSettingsHydrated} reads at most once (Story 46.10).
 *
 * `ensureRecordingSettingsHydrated` deliberately reads once per app lifetime,
 * because every later movement of these keys goes through
 * {@link applyRecordingSettings} and is echoed back. That stopped being the whole
 * truth when the recordings SUBFOLDER became editable: it is a sync-profile
 * write, not a settings write, and it moves `destinationDir` — the resolved root
 * this mirror caches, and the root the session-folder preview is composed
 * against. Without a re-read the card would keep printing the old absolute path
 * after a head change it just made itself.
 *
 * Best-effort in the same shape as hydration: a failed read keeps the previous
 * VM rather than blanking a card that is on screen, and never rejects. A read
 * that lost a race to a settings WRITE is dropped, so an in-flight optimistic
 * value is never overwritten by an older truth.
 */
export async function refreshRecordingSettings(): Promise<void> {
  const id = writeId;
  try {
    const vm = await recordingSettingsGet();
    if (id === writeId) {
      lastConfirmed = vm;
      recordingSettingsStore.getState().setSettings(vm);
    }
  } catch {
    // Keep the value on screen: this read is a refinement of a VM the surface
    // already has, never its only source.
  }
}

/**
 * Persist new recording settings (Story 17.5): optimistic mirror update, then
 * `recordingSettingsSet`; on success the mirror is replaced with the effective
 * (Rust-clamped) VM, on failure it reverts to the prior value — both only when
 * no newer write superseded this one.
 *
 * Resolves `null` when the write landed, or the Rust-authored refusal sentence
 * when it was rejected (Story 40.2). The reason has to escape: `pathTemplate`
 * is the first setting that can be REFUSED rather than clamped, and the field
 * beside it has to print why — swallowing the rejection would leave the user
 * looking at a reverted template with no explanation. Never rejects, so the
 * `void applyRecordingSettings(...)` callers that have no field to print into
 * stay correct.
 */
export async function applyRecordingSettings(next: RecordingSettingsVm): Promise<string | null> {
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
    return null;
  } catch (raw) {
    if (id === writeId) {
      recordingSettingsStore.getState().setSettings(revertTo);
    }
    // `syncErrorMessage`, never `String(raw)`: an IPC rejection is a
    // `{ code, message }` object, and stringifying one prints
    // "[object Object]" exactly where the Rust-authored reason belongs.
    return syncErrorMessage(raw, RECORDING_SETTINGS_UNKNOWN_ERROR);
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

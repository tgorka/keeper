/**
 * Settings-layer mirror store (Story 46.7, AD-98).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `capabilities.ts` precedent). It mirrors the Rust-resolved
 * {@link ConfigLayersVm} — which settings a `keeper.toml` layer decides, and
 * every problem found in those files.
 *
 * **Why a store rather than local state in one section.** The list of
 * file-decided keys has two consumers with nothing else in common: the Settings
 * section that lists them, and every individual control whose key appears in
 * that list and therefore has to say so. AD-98's whole promise is the second
 * one — *a UI control that would be overridden says so instead of silently
 * losing* — and a control cannot say so from a value held inside a sibling
 * section.
 *
 * **Read on every open, not once per lifetime.** The layer stack itself is
 * installed once at boot and never reloaded, but `faults` is not static: the
 * shell's phase two pushes the `mainSyncFolder` fault after the sync engine
 * opens, and the folder tier's faults are a live snapshot refreshed each time a
 * profile is read. Caching the first answer would show a stack that had not
 * finished being wrong yet.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { type ConfigLayersVm, type ConfigOverrideVm, configLayers } from "@/lib/ipc/client";

/**
 * Shown when the read itself fails — the one case where the section cannot say
 * anything true about the files. Deliberately not "no settings files": claiming
 * an empty stack when the question went unanswered is exactly the silent
 * substitution this story removes.
 */
export const CONFIG_LAYERS_UNREADABLE =
  "keeper could not read where your settings come from, so this list may be incomplete.";

export interface ConfigLayersState {
  /**
   * The last-served VM, or `null` before the first read resolves. Kept distinct
   * from an empty stack: "not asked yet" and "no file sets anything" are
   * different claims, and only the second one is safe to render.
   */
  layers: ConfigLayersVm | null;
  /** Why the last read failed, or `null` when it did not. */
  error: string | null;
  /** Replace the mirror from a served VM, clearing any prior failure. */
  applySnapshot: (vm: ConfigLayersVm) => void;
  /** Record a failed read without discarding the last good answer. */
  applyFailure: (message: string) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const configLayersStore = createStore<ConfigLayersState>()((set) => ({
  layers: null,
  error: null,
  applySnapshot: (vm) => set({ layers: vm, error: null }),
  // The prior `layers` survives on purpose. A transient rejection should degrade
  // to a stale-but-true list plus a caption, never to a blank section that reads
  // as "nothing overrides anything".
  applyFailure: (message) => set({ error: message }),
}));

/** React selector hook over {@link configLayersStore}. */
export function useConfigLayersStore<T>(selector: (state: ConfigLayersState) => T): T {
  return useStore(configLayersStore, selector);
}

/**
 * Re-read the layer stack. Best-effort and never throws: this surface reports
 * problems, so it must not become one.
 */
export async function refreshConfigLayers(): Promise<void> {
  try {
    configLayersStore.getState().applySnapshot(await configLayers());
  } catch {
    // The rejection body is not rendered. It would be an IPC envelope
    // describing a command that cannot fail for any reason a user can act on,
    // and the one useful thing to say is that the list may be incomplete.
    configLayersStore.getState().applyFailure(CONFIG_LAYERS_UNREADABLE);
  }
}

/**
 * The layer that decides `key`, or `null` when no file does.
 *
 * A linear scan rather than a derived `Map`: the list is the number of settings
 * a person hand-edited, which is single digits in every real install, and a
 * `Map` rebuilt in a selector would hand React a new object on every render.
 * `find` returns the identical element reference while `layers` is unchanged,
 * which is what keeps {@link useSettingOverride} from re-rendering forever.
 */
export function overrideFor(state: ConfigLayersState, key: string): ConfigOverrideVm | null {
  return state.layers?.overrides.find((entry) => entry.key === key) ?? null;
}

/** React hook form of {@link overrideFor}, for a control marking itself. */
export function useSettingOverride(key: string): ConfigOverrideVm | null {
  return useConfigLayersStore((state) => overrideFor(state, key));
}

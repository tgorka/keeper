/**
 * Capabilities mirror store (Story 12.2, AD-20).
 *
 * A vanilla zustand store created at module load *outside* React. It holds the
 * per-platform {@link CapabilitiesVm} served by the Rust `capabilities` command
 * at startup — the single source of platform truth for the frontend. The UI must
 * NEVER derive platform facts from user-agent sniffing, build-time env flags, or
 * the Tauri OS plugin (the `no-user-agent-gating` convention test enforces
 * this); it only ever reads this Rust-authored mirror.
 *
 * The declared safe default reports every optional surface **absent** (`false`
 * means the surface does not exist on this build), so a failed hydration can
 * never advertise a desktop-only affordance on a platform that lacks it. This
 * story lands the mechanism only — Epic 13 consumes the flags to hide surfaces.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { CapabilitiesVm } from "@/lib/ipc/client";

/**
 * The declared safe default: every optional surface absent until Rust responds.
 * Frozen so no code path can mutate the shared fallback in place.
 */
export const DEFAULT_CAPABILITIES: CapabilitiesVm = Object.freeze({
  trayIcon: false,
  globalHotkey: false,
  launchAtLogin: false,
  inAppUpdater: false,
  nativeMenuBar: false,
  bridgeSidecar: false,
  revealInFileManager: false,
});

export interface CapabilitiesState {
  /** The mirrored per-platform capabilities, exactly as Rust served them. */
  capabilities: CapabilitiesVm;
  /** Whether the mirror has been hydrated from a resolved `capabilities()` call. */
  hydrated: boolean;
  /** Replace the mirror wholesale from the served {@link CapabilitiesVm}. */
  applySnapshot: (vm: CapabilitiesVm) => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for platform capabilities stays in Rust.
 */
export const capabilitiesStore = createStore<CapabilitiesState>()((set) => ({
  capabilities: DEFAULT_CAPABILITIES,
  hydrated: false,
  applySnapshot: (vm) => set({ capabilities: vm, hydrated: true }),
}));

/**
 * React selector hook over {@link capabilitiesStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useCapabilitiesStore<T>(selector: (state: CapabilitiesState) => T): T {
  return useStore(capabilitiesStore, selector);
}

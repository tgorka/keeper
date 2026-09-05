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
  recording: false,
  sync: false,
  notes: false,
  sessions: false,
  bots: false,
  botTools: false,
  overlayTitleBar: false,
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

/**
 * Flags that do not tell the tiers apart, and so are left out of the fold in
 * {@link isReducedCapabilityPlatform}.
 *
 * **`bots` is true on every tier (Epic 62, FR-396)** — a conversation is two
 * tables in `keeper.db`, which every platform opens — so its value says
 * nothing about which build this is. Folding it in would make the phone read
 * as a desktop the moment the pane existed, which would silently drop the
 * "On this iPhone" disclosure, the backup-exclusion line and the offline pill.
 *
 * **`sync` and `notes` are true on the phone too (Epic 66, AD-198, AD-200)**:
 * `keeper-sync` links on iOS and clones, fetches and fast-forwards with its
 * own engine, and a vault is a folder keeper syncs, so a folder on the phone
 * carries notes with it. The tier is told by what the OS *refuses* — a tray,
 * a global hotkey, a menu bar, an updater, a sidecar process, a Finder reveal
 * — never by what a folder can do. Before this epic the two flags happened to
 * be `cfg!(desktop)`-shaped only because nobody had linked the crate on the
 * phone; leaving them in the fold would turn the first phone that could sync
 * into a desktop the moment `capabilities()` answered.
 *
 * The drive half of the Bots surface, `botTools`, is `desktop && sync` and
 * stays in the fold. A flag belongs here only when the Rust side computes it
 * as true on both tiers; that is the whole membership rule.
 */
const TIER_NEUTRAL_FLAGS: Partial<Record<keyof CapabilitiesVm, true>> = {
  bots: true,
  sync: true,
  notes: true,
};

/**
 * Pure predicate: is this a capability-reduced platform (i.e. the phone tier)?
 *
 * True only when the mirror has hydrated (`hydrated === true`) AND every one of
 * the tier-telling surfaces is absent. Every flag outside
 * {@link TIER_NEUTRAL_FLAGS} is `cfg!(desktop)`-shaped in the Rust
 * `capabilities` command, so "every such flag `false`" equals "iOS" today while
 * staying a pure capability read — never a platform sniff. The `hydrated` term
 * is load-bearing: without it the all-`false` {@link DEFAULT_CAPABILITIES} safe
 * default would advertise the iOS-only disclosures on desktop for the one frame
 * before hydration resolves.
 *
 * The absence check derives from `Object.entries` over the (all-boolean) VM
 * rather than a hand-written flag list, so a capability added to
 * `CapabilitiesVm` is folded in automatically and can never silently desync the
 * predicate; only a flag that is true on both tiers has to opt out, by name.
 */
export function isReducedCapabilityPlatform(state: CapabilitiesState): boolean {
  const { hydrated, capabilities } = state;
  return (
    hydrated &&
    Object.entries(capabilities).every(
      ([flag, present]) => TIER_NEUTRAL_FLAGS[flag as keyof CapabilitiesVm] === true || !present,
    )
  );
}

/**
 * React hook wrapping {@link isReducedCapabilityPlatform} over the shared
 * {@link capabilitiesStore}. Drives the "On this iPhone" disclosure and the
 * Archive & Storage backup-exclusion line — the two capability-honest surfaces
 * that render only on the reduced (phone) tier — and, since Epic 65 (AD-189),
 * the tier itself: `useShellLayout` reports `phone` from this at every width.
 */
export function useIsReducedCapabilityPlatform(): boolean {
  return useCapabilitiesStore(isReducedCapabilityPlatform);
}

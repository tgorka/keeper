import { afterEach, describe, expect, it } from "vitest";
import type { CapabilitiesVm } from "@/lib/ipc/client";
import {
  capabilitiesStore,
  DEFAULT_CAPABILITIES,
  isReducedCapabilityPlatform,
} from "@/lib/stores/capabilities";

const desktopCapabilities: CapabilitiesVm = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
  recording: true,
  sync: true,
  notes: true,
  sessions: true,
  bots: true,
  botTools: true,
  overlayTitleBar: true,
};

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("capabilitiesStore", () => {
  it("declares the safe default: every optional surface absent, not hydrated", () => {
    const state = capabilitiesStore.getState();
    expect(state.hydrated).toBe(false);
    expect(state.capabilities).toEqual({
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
  });

  it("applySnapshot mirrors the served CapabilitiesVm wholesale and marks hydrated", () => {
    capabilitiesStore.getState().applySnapshot(desktopCapabilities);
    expect(capabilitiesStore.getState().capabilities).toEqual(desktopCapabilities);
    expect(capabilitiesStore.getState().hydrated).toBe(true);
  });

  it("a later snapshot replaces the mirror (no merge)", () => {
    capabilitiesStore.getState().applySnapshot(desktopCapabilities);
    const mobile: CapabilitiesVm = { ...DEFAULT_CAPABILITIES };
    capabilitiesStore.getState().applySnapshot(mobile);
    expect(capabilitiesStore.getState().capabilities).toEqual(mobile);
    expect(capabilitiesStore.getState().hydrated).toBe(true);
  });
});

describe("isReducedCapabilityPlatform", () => {
  it("desktop (all flags true, hydrated) is NOT reduced", () => {
    capabilitiesStore.getState().applySnapshot(desktopCapabilities);
    expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(false);
  });

  it("iOS (all flags false, hydrated) IS reduced", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES });
    expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(true);
  });

  it("pre-hydration (all flags false, NOT hydrated) is NOT reduced — the hydrated gate", () => {
    // The all-false safe default before the mirror resolves must never advertise the
    // reduced-platform disclosures on desktop.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
    expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(false);
  });

  it("a single tier-telling flag (hydrated) is NOT reduced — every flag must be absent", () => {
    // Derived from the VM's own keys so a capability added later is exercised
    // automatically: the hand-written list had already drifted past `sync`.
    // `bots` is the one flag true on every tier (Epic 62, FR-396), so it is the
    // one flag that must NOT flip the verdict; the case below owns it.
    const flags = (Object.keys(DEFAULT_CAPABILITIES) as Array<keyof CapabilitiesVm>).filter(
      (flag) => flag !== "bots",
    );
    for (const flag of flags) {
      capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, [flag]: true });
      expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(false);
    }
  });

  it("a phone that can hold a conversation (`bots` alone true) IS still reduced", () => {
    // Epic 62 puts the Bots pane on the phone, so `bots` is true there. If the
    // predicate folded it in, hydrating on iOS would read as desktop and the
    // "On this iPhone" disclosure, the backup-exclusion line and the offline
    // pill would all vanish the moment the pane existed.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
    expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(true);
  });

  it("the drive half (`botTools`) is tier-telling: alone true is NOT reduced", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, botTools: true });
    expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(false);
  });
});

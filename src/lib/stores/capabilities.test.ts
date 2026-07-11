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

  it("a single present flag (hydrated) is NOT reduced — every flag must be absent", () => {
    // Exercise each flag: with any one surface present, the platform is not reduced.
    const flags: Array<keyof CapabilitiesVm> = [
      "trayIcon",
      "globalHotkey",
      "launchAtLogin",
      "inAppUpdater",
      "nativeMenuBar",
      "bridgeSidecar",
      "revealInFileManager",
    ];
    for (const flag of flags) {
      capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, [flag]: true });
      expect(isReducedCapabilityPlatform(capabilitiesStore.getState())).toBe(false);
    }
  });
});

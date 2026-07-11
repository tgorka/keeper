import { afterEach, describe, expect, it } from "vitest";
import type { CapabilitiesVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

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

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { settingsUiStore, useSettingsOpen } from "@/lib/stores/settings-ui";

beforeEach(() => {
  settingsUiStore.getState().setSettingsOpen(false);
});

afterEach(() => {
  settingsUiStore.getState().setSettingsOpen(false);
});

describe("settingsUiStore", () => {
  it("starts closed", () => {
    expect(settingsUiStore.getState().settingsOpen).toBe(false);
  });

  it("opens and closes via setSettingsOpen", () => {
    settingsUiStore.getState().setSettingsOpen(true);
    expect(settingsUiStore.getState().settingsOpen).toBe(true);
    settingsUiStore.getState().setSettingsOpen(false);
    expect(settingsUiStore.getState().settingsOpen).toBe(false);
  });

  it("useSettingsOpen mirrors the store and updates reactively", () => {
    const { result } = renderHook(() => useSettingsOpen());
    expect(result.current).toBe(false);
    act(() => {
      settingsUiStore.getState().setSettingsOpen(true);
    });
    expect(result.current).toBe(true);
  });
});

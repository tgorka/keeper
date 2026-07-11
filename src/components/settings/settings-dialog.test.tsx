import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  encryptionPosture: vi.fn(() => Promise.resolve(null)),
  honorRemoteDeletions: vi.fn(() => Promise.resolve(false)),
  setHonorRemoteDeletions: vi.fn(() => Promise.resolve()),
  incognitoGetGlobal: vi.fn(() => Promise.resolve(false)),
  incognitoSetGlobal: vi.fn(() => Promise.resolve()),
  notifyGetPreviewEnabled: vi.fn(() => Promise.resolve(true)),
  notifySetPreviewEnabled: vi.fn(() => Promise.resolve()),
  dockBadgeModeGet: vi.fn(() => Promise.resolve("all")),
  dockBadgeModeSet: vi.fn(() => Promise.resolve()),
  notificationPermissionState: vi.fn(() => Promise.resolve("granted")),
  iosOpenAppSettings: vi.fn(() => Promise.resolve()),
  launchAtLoginGet: vi.fn(() => Promise.resolve(false)),
  launchAtLoginSet: vi.fn(() => Promise.resolve()),
  menuBarPresenceGet: vi.fn(() => Promise.resolve(false)),
  menuBarPresenceSet: vi.fn(() => Promise.resolve()),
  undoSendWindow: vi.fn(() => Promise.resolve(10)),
  setUndoSendWindow: vi.fn(() => Promise.resolve()),
  hotkeyGet: vi.fn(() =>
    Promise.resolve({
      accelerator: "Control+Alt+Space",
      isDefault: true,
      active: true,
      conflict: null,
    }),
  ),
  hotkeySet: vi.fn(() =>
    Promise.resolve({
      accelerator: "Control+Alt+Space",
      isDefault: true,
      active: true,
      conflict: null,
    }),
  ),
  egressList: vi.fn(() => Promise.resolve([])),
  verificationCancel: vi.fn(() => Promise.resolve()),
  iosSyncDisclosureShownGet: vi.fn(() => Promise.resolve(true)),
  iosSyncDisclosureShownSet: vi.fn(() => Promise.resolve()),
}));

// The About section (mounted by the dialog) imports the updater/process plugins;
// mock them so the dialog renders in jsdom without a Tauri runtime.
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(() => Promise.resolve()),
}));

import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import {
  BADGE_NOT_LIVE_SENTENCE,
  NO_BACKGROUND_SYNC_SENTENCE,
} from "@/components/settings/no-background-sync-disclosure";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import type { AccountVm } from "@/lib/ipc/client";
import {
  dockBadgeModeSet,
  encryptionPosture,
  type HotkeyVm,
  honorRemoteDeletions,
  hotkeyGet,
  hotkeySet,
  iosOpenAppSettings,
  launchAtLoginGet,
  menuBarPresenceGet,
  notificationPermissionState,
  notifyGetPreviewEnabled,
  notifySetPreviewEnabled,
  setHonorRemoteDeletions,
  setUndoSendWindow,
  undoSendWindow,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { keyBackupStore } from "@/lib/stores/key-backup";
import { verificationStore } from "@/lib/stores/verification";
import { wizardStore } from "@/lib/stores/wizard";

const mockPosture = vi.mocked(encryptionPosture);
const mockHonorGet = vi.mocked(honorRemoteDeletions);
const mockHonorSet = vi.mocked(setHonorRemoteDeletions);
const mockUndoGet = vi.mocked(undoSendWindow);
const mockUndoSet = vi.mocked(setUndoSendWindow);
const mockHotkeyGet = vi.mocked(hotkeyGet);
const mockHotkeySet = vi.mocked(hotkeySet);
const mockNotifyGet = vi.mocked(notifyGetPreviewEnabled);
const mockNotifySet = vi.mocked(notifySetPreviewEnabled);
const mockLaunchGet = vi.mocked(launchAtLoginGet);
const mockMenuBarGet = vi.mocked(menuBarPresenceGet);
const mockPermissionState = vi.mocked(notificationPermissionState);
const mockOpenAppSettings = vi.mocked(iosOpenAppSettings);
const mockBadgeModeSet = vi.mocked(dockBadgeModeSet);

const DEFAULT_HOTKEY_VM: HotkeyVm = {
  accelerator: "Control+Alt+Space",
  isDefault: true,
  active: true,
  conflict: null,
};

/** All seven capabilities present = the desktop tier (every surface renders). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
};

function account(id: string): AccountVm {
  return {
    accountId: id,
    userId: `@${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    mockPosture.mockClear();
    mockHonorGet.mockClear();
    mockHonorSet.mockClear();
    mockHonorGet.mockResolvedValue(false);
    mockHonorSet.mockResolvedValue(undefined);
    mockUndoGet.mockClear();
    mockUndoSet.mockClear();
    mockUndoGet.mockResolvedValue(10);
    mockUndoSet.mockResolvedValue(undefined);
    mockHotkeyGet.mockClear();
    mockHotkeySet.mockClear();
    mockHotkeyGet.mockResolvedValue(DEFAULT_HOTKEY_VM);
    mockHotkeySet.mockResolvedValue(DEFAULT_HOTKEY_VM);
    mockNotifyGet.mockClear();
    mockNotifySet.mockClear();
    mockNotifyGet.mockResolvedValue(true);
    mockNotifySet.mockResolvedValue(undefined);
    mockPermissionState.mockClear();
    mockPermissionState.mockResolvedValue("granted");
    mockOpenAppSettings.mockClear();
    mockOpenAppSettings.mockResolvedValue(undefined);
    mockBadgeModeSet.mockClear();
    mockBadgeModeSet.mockResolvedValue(undefined);
    accountsStore.getState().clear();
    encryptionStatusStore.getState().reset();
    keyBackupStore.getState().reset();
    verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
    // Default the mirror to the desktop tier so the capability-gated surfaces (the
    // Shortcuts section, the Launch-at-login / Keep-in-menu-bar rows) render for the
    // existing assertions; the reduced-platform cases opt in explicitly.
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
  });

  afterEach(() => {
    vi.clearAllMocks();
    accountsStore.getState().clear();
    encryptionStatusStore.getState().reset();
    keyBackupStore.getState().reset();
    verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  });

  it("shows the honest archive.db/keeper.db + FileVault copy when open", async () => {
    mockPosture.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(STORAGE_HONESTY_SENTENCE)).toBeInTheDocument();
    // Section heading present.
    expect(screen.getByText("Archive & Storage")).toBeInTheDocument();
  });

  it("reflects the encrypted SDK-store status when posture is on", async () => {
    mockPosture.mockResolvedValue(true);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(SDK_STORE_ENCRYPTED_STATUS)).toBeInTheDocument();
    expect(screen.queryByText(SDK_STORE_UNENCRYPTED_STATUS)).not.toBeInTheDocument();
  });

  it("reflects the FileVault-only SDK-store status when posture is off", async () => {
    mockPosture.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    await waitFor(() => expect(screen.getByText(SDK_STORE_UNENCRYPTED_STATUS)).toBeInTheDocument());
    expect(screen.queryByText(SDK_STORE_ENCRYPTED_STATUS)).not.toBeInTheDocument();
  });

  it("shows a Verify button only for an unverified account and opens the modal for it", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice"), account("bob")]);
    encryptionStatusStore.getState().setStatus("alice", "unverified");
    encryptionStatusStore.getState().setStatus("bob", "verified");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const verifyButtons = screen.getAllByRole("button", { name: "Verify" });
    expect(verifyButtons).toHaveLength(1);

    fireEvent.click(verifyButtons[0]);
    expect(verificationStore.getState().modalOpen).toBe(true);
    expect(verificationStore.getState().activeAccountId).toBe("alice");
  });

  it("shows no Verify button when accounts are verified or checking", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice"), account("bob")]);
    encryptionStatusStore.getState().setStatus("alice", "verified");
    // bob left as pending/unknown (no status set).
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.queryByRole("button", { name: "Verify" })).not.toBeInTheDocument();
  });

  it("shows a 'Set up backup' button for a disabled backup and opens enable", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    keyBackupStore.getState().setStatus("alice", "disabled");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getByText("Not set up")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Set up backup" }));
    expect(keyBackupStore.getState().modalOpen).toBe(true);
    expect(keyBackupStore.getState().mode).toBe("enable");
    expect(keyBackupStore.getState().accountId).toBe("alice");
  });

  it("shows a 'Restore' button for an incomplete backup and opens restore", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    keyBackupStore.getState().setStatus("alice", "incomplete");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getByText("Needs your recovery key")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(keyBackupStore.getState().modalOpen).toBe(true);
    expect(keyBackupStore.getState().mode).toBe("restore");
    expect(keyBackupStore.getState().accountId).toBe("alice");
  });

  it("shows 'Backup on' with no button for an enabled backup", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    keyBackupStore.getState().setStatus("alice", "enabled");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getByText("Backup on")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Set up backup" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
  });

  it("shows 'Checking…' for a pending backup status", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    // No backup status set → pending.
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getAllByText("Checking…").length).toBeGreaterThan(0);
  });

  it("renders the honor-remote-deletions toggle and reads its initial state", async () => {
    mockPosture.mockResolvedValue(false);
    mockHonorGet.mockResolvedValue(true);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", {
      name: "Honor remote deletions locally",
    });
    await waitFor(() => expect(toggle).toBeChecked());
    expect(mockHonorGet).toHaveBeenCalled();
  });

  it("persists the honor-remote-deletions toggle on change", async () => {
    mockPosture.mockResolvedValue(false);
    mockHonorGet.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", {
      name: "Honor remote deletions locally",
    });
    await waitFor(() => expect(toggle).not.toBeChecked());
    fireEvent.click(toggle);
    await waitFor(() => expect(mockHonorSet).toHaveBeenCalledWith(true));
    expect(toggle).toBeChecked();
  });

  // ── Notifications section (Story 10.1) ─────────────────────────────────────
  it("renders the message-previews toggle and reads its initial state", async () => {
    mockPosture.mockResolvedValue(false);
    mockNotifyGet.mockResolvedValue(true);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", { name: "Show message previews" });
    await waitFor(() => expect(toggle).toBeChecked());
    expect(mockNotifyGet).toHaveBeenCalled();
  });

  it("reflects previews-off in the toggle", async () => {
    mockPosture.mockResolvedValue(false);
    mockNotifyGet.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", { name: "Show message previews" });
    await waitFor(() => expect(toggle).not.toBeChecked());
  });

  it("persists the message-previews toggle on change", async () => {
    mockPosture.mockResolvedValue(false);
    mockNotifyGet.mockResolvedValue(true);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", { name: "Show message previews" });
    await waitFor(() => expect(toggle).toBeChecked());
    fireEvent.click(toggle);
    await waitFor(() => expect(mockNotifySet).toHaveBeenCalledWith(false));
    expect(toggle).not.toBeChecked();
  });

  it("reverts the message-previews toggle when the persist fails", async () => {
    mockPosture.mockResolvedValue(false);
    mockNotifyGet.mockResolvedValue(true);
    mockNotifySet.mockRejectedValue({
      code: "internal",
      message: "boom",
      accountId: null,
      retriable: false,
    });
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", { name: "Show message previews" });
    await waitFor(() => expect(toggle).toBeChecked());
    fireEvent.click(toggle);
    // Optimistic flip, then revert on the failed persist.
    await waitFor(() => expect(mockNotifySet).toHaveBeenCalledWith(false));
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("renders the Undo-Send window control and reads its initial value", async () => {
    mockPosture.mockResolvedValue(false);
    mockUndoGet.mockResolvedValue(15);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const input = await screen.findByLabelText("Undo-Send window (seconds)");
    await waitFor(() => expect(input).toHaveValue(15));
    expect(mockUndoGet).toHaveBeenCalled();
  });

  it("persists a changed Undo-Send window (clamping out-of-range input)", async () => {
    mockPosture.mockResolvedValue(false);
    mockUndoGet.mockResolvedValue(10);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const input = await screen.findByLabelText("Undo-Send window (seconds)");
    await waitFor(() => expect(input).toHaveValue(10));
    // An in-range value persists verbatim.
    fireEvent.change(input, { target: { value: "5" } });
    await waitFor(() => expect(mockUndoSet).toHaveBeenCalledWith(5));
    // An out-of-range value clamps to 60 before persisting.
    fireEvent.change(input, { target: { value: "99" } });
    await waitFor(() => expect(mockUndoSet).toHaveBeenCalledWith(60));
    expect(input).toHaveValue(60);
  });

  it("'Run setup again' starts the wizard and closes Settings", async () => {
    mockPosture.mockResolvedValue(false);
    const onOpenChange = vi.fn();
    render(<SettingsDialog open onOpenChange={onOpenChange} />);

    const runAgain = await screen.findByRole("button", { name: "Run setup again" });
    fireEvent.click(runAgain);

    expect(wizardStore.getState().active).toBe(true);
    expect(wizardStore.getState().step).toBe("welcome");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  // ── Shortcuts section (Story 9.4) ──────────────────────────────────────────
  it("renders the current summon hotkey binding as glyph chips", async () => {
    mockPosture.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText("Shortcuts")).toBeInTheDocument();
    // The default Control+Alt+Space renders as the macOS glyph string ⌃⌥Space.
    expect(await screen.findByText("⌃⌥Space")).toBeInTheDocument();
    expect(mockHotkeyGet).toHaveBeenCalled();
  });

  it("captures a new chord and reassigns via hotkeySet", async () => {
    mockPosture.mockResolvedValue(false);
    mockHotkeySet.mockResolvedValue({
      accelerator: "Control+Shift+K",
      isDefault: false,
      active: true,
      conflict: null,
    });
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const change = await screen.findByRole("button", { name: "Change…" });
    fireEvent.click(change);
    const capture = await screen.findByText(/Press a shortcut/);
    // Fire the captured chord ⌃⇧K.
    fireEvent.keyDown(capture, { code: "KeyK", key: "k", ctrlKey: true, shiftKey: true });

    await waitFor(() => {
      expect(mockHotkeySet).toHaveBeenCalledWith("Control+Shift+K");
    });
    // The new binding renders.
    expect(await screen.findByText("⌃⇧K")).toBeInTheDocument();
  });

  it("shows the soft conflict warning returned by the backend", async () => {
    mockPosture.mockResolvedValue(false);
    mockHotkeyGet.mockResolvedValue({
      accelerator: "Super+Space",
      isDefault: false,
      active: true,
      conflict: "May conflict with Spotlight (⌘Space).",
    });
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(/May conflict with Spotlight/)).toBeInTheDocument();
  });

  it("explains what to enable when the hotkey is not registered (active=false)", async () => {
    mockPosture.mockResolvedValue(false);
    mockHotkeyGet.mockResolvedValue({
      accelerator: "Control+Alt+Space",
      isDefault: true,
      active: false,
      conflict: null,
    });
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(/Privacy & Security/)).toBeInTheDocument();
  });

  it("resets to the default via hotkeySet", async () => {
    mockPosture.mockResolvedValue(false);
    // Start on a non-default binding so the Reset button is enabled.
    mockHotkeyGet.mockResolvedValue({
      accelerator: "Control+Shift+K",
      isDefault: false,
      active: true,
      conflict: null,
    });
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const reset = await screen.findByRole("button", { name: "Reset to default" });
    fireEvent.click(reset);

    await waitFor(() => {
      expect(mockHotkeySet).toHaveBeenCalledWith("Control+Alt+Space");
    });
  });

  // ── Capability gating (Story 13.7) ─────────────────────────────────────────
  it("desktop: renders the Shortcuts section, both background rows, and no reduced-platform disclosure", async () => {
    mockPosture.mockResolvedValue(false);
    // beforeEach already hydrated the desktop tier.
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText("Shortcuts")).toBeInTheDocument();
    expect(await screen.findByRole("switch", { name: "Launch at login" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Keep in menu bar" })).toBeInTheDocument();
    // No phone-tier Archive backup line on desktop.
    expect(screen.queryByText(/excluded from device backup/)).not.toBeInTheDocument();
    // Story 10.3's "Background & dock" section and its ⌘W/⌘Q copy stay exactly as-is.
    expect(screen.getByText("Background & dock")).toBeInTheDocument();
    expect(screen.getByText(/keeps keeper running in the background/)).toBeInTheDocument();
    // No iOS lifecycle-honesty copy on desktop (Story 14.2).
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
    expect(screen.queryByText(BADGE_NOT_LIVE_SENTENCE)).not.toBeInTheDocument();
  });

  it("iOS: hides the Shortcuts section and the whole Background & dock section, and shows the Archive backup line", async () => {
    mockPosture.mockResolvedValue(false);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    // The Archive backup-exclusion line renders (reduced platform, hydrated).
    expect(await screen.findByText(/excluded from device backup/)).toBeInTheDocument();
    // The gated surfaces are absent — no dead affordances.
    expect(screen.queryByText("Shortcuts")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "Launch at login" })).not.toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "Keep in menu bar" })).not.toBeInTheDocument();
    // The whole "Background & dock" section is gone on the phone tier (Story 14.2) —
    // no ⌘W-keeps-syncing background-delivery claim, no desktop Dock-badge control.
    expect(screen.queryByText("Background & dock")).not.toBeInTheDocument();
    expect(screen.queryByText(/keeps keeper running in the background/)).not.toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Dock badge mode" })).not.toBeInTheDocument();
    // The hidden rows never probe their desktop-only backends (no dead Unsupported IPC).
    expect(mockLaunchGet).not.toHaveBeenCalled();
    expect(mockMenuBarGet).not.toHaveBeenCalled();
  });

  it("iOS: the Notifications section shows the canonical only-while-open sentence and the badge note (Story 14.2)", async () => {
    mockPosture.mockResolvedValue(false);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(NO_BACKGROUND_SYNC_SENTENCE)).toBeInTheDocument();
    expect(screen.getByText(BADGE_NOT_LIVE_SENTENCE)).toBeInTheDocument();
  });

  // ── Story 14.3: iOS app-icon badge radio + permission-denied surface ─────────
  it("iOS: renders the App icon badge mode radio in the Notifications section (Story 14.3)", async () => {
    mockPosture.mockResolvedValue(false);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(
      await screen.findByRole("radiogroup", { name: "App icon badge mode" }),
    ).toBeInTheDocument();
  });

  it("iOS: permission denied shows the inline state and Open-Settings calls iosOpenAppSettings (Story 14.3)", async () => {
    mockPosture.mockResolvedValue(false);
    mockPermissionState.mockResolvedValue("denied");
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    // The fixed permission-denied sentence and the badge-needs-permission note render.
    expect(
      await screen.findByText("Notifications are off for keeper in iOS Settings."),
    ).toBeInTheDocument();
    expect(screen.getByText(/app icon badge needs the same permission/i)).toBeInTheDocument();

    // Tapping Open Settings routes through the Rust deep link (no re-prompt).
    fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(mockOpenAppSettings).toHaveBeenCalledTimes(1);
  });

  it("iOS: granted permission hides the inline permission state (Story 14.3)", async () => {
    mockPosture.mockResolvedValue(false);
    mockPermissionState.mockResolvedValue("granted");
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    // Wait for the badge radio to confirm the reduced-tier section has resolved.
    await screen.findByRole("radiogroup", { name: "App icon badge mode" });
    expect(
      screen.queryByText("Notifications are off for keeper in iOS Settings."),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open Settings" })).not.toBeInTheDocument();
  });

  it("iOS: unknown permission hides the inline permission state (Story 14.3)", async () => {
    mockPosture.mockResolvedValue(false);
    mockPermissionState.mockResolvedValue("unknown");
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    await screen.findByRole("radiogroup", { name: "App icon badge mode" });
    expect(
      screen.queryByText("Notifications are off for keeper in iOS Settings."),
    ).not.toBeInTheDocument();
  });

  it("desktop: shows neither the App icon badge radio nor the permission state (Story 14.3)", async () => {
    mockPosture.mockResolvedValue(false);
    // beforeEach hydrated the desktop tier.
    render(<SettingsDialog open onOpenChange={() => {}} />);

    // The desktop Dock badge radio (Background & dock) is present, but not the iOS one.
    await screen.findByRole("radiogroup", { name: "Dock badge mode" });
    expect(
      screen.queryByRole("radiogroup", { name: "App icon badge mode" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Notifications are off for keeper in iOS Settings."),
    ).not.toBeInTheDocument();
    // No permission probe on desktop (reduced-tier only).
    expect(mockPermissionState).not.toHaveBeenCalled();
  });

  it("pre-hydration: hides the desktop-only surfaces by the safe default but does NOT flash the Archive backup line", async () => {
    mockPosture.mockResolvedValue(false);
    // All-false default AND not hydrated.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
    render(<SettingsDialog open onOpenChange={() => {}} />);

    await screen.findByText(STORAGE_HONESTY_SENTENCE);
    // Desktop-only surfaces hidden by the safe default…
    expect(screen.queryByText("Shortcuts")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "Launch at login" })).not.toBeInTheDocument();
    // …but the iOS-only disclosure must NOT flash before the mirror resolves.
    expect(screen.queryByText(/excluded from device backup/)).not.toBeInTheDocument();
  });
});

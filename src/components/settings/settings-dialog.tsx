import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { AboutSection } from "@/components/settings/about-section";
import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_STATUS_LOADING,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import {
  BADGE_NOT_LIVE_SENTENCE,
  NO_BACKGROUND_SYNC_SENTENCE,
} from "@/components/settings/no-background-sync-disclosure";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Switch } from "@/components/ui/switch";
import { acceleratorFromEvent, DEFAULT_GLOBAL_HOTKEY, formatAccelerator } from "@/lib/hotkey";
import {
  type DockBadgeMode,
  dockBadgeModeGet,
  dockBadgeModeSet,
  encryptionPosture,
  type HotkeyVm,
  honorRemoteDeletions,
  hotkeyGet,
  hotkeySet,
  incognitoGetGlobal,
  incognitoSetGlobal,
  iosOpenAppSettings,
  launchAtLoginGet,
  launchAtLoginSet,
  menuBarPresenceGet,
  menuBarPresenceSet,
  type NotificationPermission,
  notificationPermissionState,
  notifyGetPreviewEnabled,
  notifySetPreviewEnabled,
  setHonorRemoteDeletions,
  setUndoSendWindow,
  undoSendWindow,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { useEncryptionStatus } from "@/lib/stores/encryption-status";
import { incognitoStore } from "@/lib/stores/incognito";
import { keyBackupStore, useKeyBackupStatus } from "@/lib/stores/key-backup";
import { verificationStore } from "@/lib/stores/verification";
import { wizardStore } from "@/lib/stores/wizard";

interface SettingsDialogProps {
  /** Whether the dialog is open (controlled by the caller). */
  open: boolean;
  /** Called to open/close the dialog. */
  onOpenChange: (open: boolean) => void;
}

/**
 * The reduced-platform (phone tier) Archive & Storage disclosure (Story 13.7).
 * Sentence case, no exclamation marks (project voice). Discloses that the phone's
 * Local Archive is excluded from device backup this phase while the Mac stays the
 * durable, exportable copy. The actual backup-exclusion file flagging is Epic 14-7;
 * 13.7 adds only this honest line.
 */
const ARCHIVE_BACKUP_EXCLUSION_SENTENCE =
  "This phone's Local Archive is excluded from device backup, so it is not copied off the phone. Your Mac remains the durable, exportable copy this phase.";

/**
 * Settings dialog with a read-only Archive & Storage section (Story 2.6, AD-22,
 * UX-DR17). States plainly that `keeper.db`/`archive.db` are not
 * passphrase-encrypted in this version and rely on FileVault, and reflects
 * whether the per-account Matrix SDK stores are passphrase-encrypted (loaded from
 * `encryptionPosture()` on open). No toggle — the posture is a first-run choice
 * only and is never re-prompted here.
 */
export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  // The OS-global summon hotkey is a desktop-only capability; hide the whole
  // Shortcuts section wherever the platform lacks it (the phone tier).
  const globalHotkey = useCapabilitiesStore((s) => s.capabilities.globalHotkey);
  // Screen recording is a desktop-macOS-≥13 capability (Story 16.3); render the
  // Recording section only where it exists — never a dead settings surface.
  const recording = useCapabilitiesStore((s) => s.capabilities.recording);
  // Whether this is the capability-reduced (phone) tier — drives the Archive
  // backup-exclusion line below, the "On this iPhone" list in About, and hides the
  // desktop "Background & dock" section (Story 14.2).
  const reducedPlatform = useIsReducedCapabilityPlatform();
  // `undefined` = still loading; otherwise the resolved posture (`null` unchosen,
  // or `true`/`false`). Keeping "loading" distinct from a resolved value stops the
  // status line from momentarily claiming "not encrypted" before the real posture
  // arrives, on both first open and reopen.
  const [posture, setPosture] = useState<boolean | null | undefined>(undefined);

  useEffect(() => {
    if (!open) {
      return;
    }
    // Reset to the loading state on every (re)open so a stale prior value never
    // flashes while the fresh read is in flight.
    setPosture(undefined);
    let cancelled = false;
    void encryptionPosture()
      .then((value) => {
        if (!cancelled) {
          setPosture(value);
        }
      })
      .catch(() => {
        // On a read failure, fall back to the honest FileVault-only status.
        if (!cancelled) {
          setPosture(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // While loading, show a neutral checking line — never a definitive (possibly
  // wrong) "not encrypted" claim. `true` ⇒ encrypted; `false`/`null` ⇒ FileVault.
  const sdkStatus =
    posture === undefined
      ? SDK_STORE_STATUS_LOADING
      : posture === true
        ? SDK_STORE_ENCRYPTED_STATUS
        : SDK_STORE_UNENCRYPTED_STATUS;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {/* The Settings body is taller than the viewport, so it must scroll. Override
          the shadcn DialogContent from `grid` to a height-capped `flex flex-col` that
          clips (`overflow-hidden`): the header sizes to content, and the body below is
          `flex-1 min-h-0` so it takes the remaining bounded height and scrolls within
          it. `min-h-0` is required — a flex child defaults to min-height:auto (= its
          content size), which would grow past the cap and bleed out of the dialog
          instead of scrolling. `min-w-0` lets the copy wrap rather than clip on the
          right. (An arbitrary `grid-rows-[…minmax(0,1fr)]` looked equivalent but the
          comma inside `minmax()` isn't emitted by the Tailwind arbitrary-value parser,
          so no rule was generated and the cap never applied — flex avoids that.) */}
      <DialogContent className="flex max-h-[85vh] flex-col gap-0 overflow-hidden">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>Archive &amp; Storage</DialogDescription>
        </DialogHeader>
        <div className="-mr-2 mt-2 flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-y-auto pr-2">
          <div className="flex min-w-0 flex-col gap-3 text-sm">
            <p>{sdkStatus}</p>
            <p className="text-muted-foreground">{STORAGE_HONESTY_SENTENCE}</p>
            {reducedPlatform && (
              <p className="text-muted-foreground">{ARCHIVE_BACKUP_EXCLUSION_SENTENCE}</p>
            )}
            <HonorRemoteDeletionsRow />
          </div>
          <NotificationsSection open={open} />
          {/* The desktop "Background & dock" section (⌘W/⌘Q mechanics, Dock badge,
              launch-at-login, menu bar) never renders on the reduced (phone) tier —
              its keeps-syncing-in-background copy would be a false background-delivery
              claim on iOS (Story 14.2, FR-53/FR-61). Desktop (Story 10.3) unchanged. */}
          {!reducedPlatform && <BackgroundSection open={open} />}
          <PrivacySection open={open} />
          {globalHotkey && <ShortcutsSection open={open} />}
          {/* The Recording section is desktop-macOS-≥13 only (Story 16.3): absent on
              every platform that cannot record, never a dead affordance. */}
          {recording && <RecordingSection />}
          <EncryptionSection />
          <SetupSection onOpenChange={onOpenChange} />
          <AboutSection open={open} />
        </div>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The plain disclosure for the honor-remote-deletions toggle (Story 5.2, FR-36,
 * UX-DR17). Sentence case, no exclamation marks (project voice).
 */
const HONOR_REMOTE_DELETIONS_SENTENCE =
  "keeper keeps local copies of remotely edited and deleted messages by default. Turning this on hides remotely deleted messages from history retrieval on this Mac; turning it off makes them retrievable again. The local copies are never erased.";

/**
 * The "Honor remote deletions locally" toggle in the Archive & Storage section
 * (Story 5.2, FR-36). Reads its initial state via `honorRemoteDeletions()` and
 * persists changes via `setHonorRemoteDeletions`. On a persist failure the toggle
 * reverts to its prior value (honest — never claims a state that was not saved).
 */
function HonorRemoteDeletionsRow() {
  // `undefined` = still loading; otherwise the resolved boolean.
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    void honorRemoteDeletions()
      .then((value) => {
        if (!cancelled) {
          setEnabled(value);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnabled(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Monotonic token so a failed persist only reverts when no newer toggle has
  // superseded it — prevents a stale-closure revert clobbering a rapid re-toggle.
  const writeId = useRef(0);

  const onCheckedChange = (next: boolean) => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = enabled ?? false;
    setEnabled(next);
    void setHonorRemoteDeletions(next).catch(() => {
      // Persist failed — revert, but only if this is still the latest toggle.
      if (id === writeId.current) {
        setEnabled(prev);
      }
    });
  };

  return (
    <div className="mt-1 flex flex-col gap-1.5 border-border border-t pt-3">
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="honor-remote-deletions">Honor remote deletions locally</Label>
        <Switch
          id="honor-remote-deletions"
          checked={enabled ?? false}
          disabled={enabled === undefined}
          onCheckedChange={onCheckedChange}
        />
      </div>
      <p className="text-muted-foreground">{HONOR_REMOTE_DELETIONS_SENTENCE}</p>
    </div>
  );
}

/**
 * The plain disclosure for the global Incognito default (Story 8.1). Sentence case,
 * no exclamation marks, honest consequence-naming (project voice).
 */
const INCOGNITO_GLOBAL_SENTENCE =
  "Reading a chat sends a private read receipt: your read position still syncs across your own devices, but the other person keeps seeing the message as unread. This is the default for every chat; you can override it per account or per chat.";

/** The default Undo-Send window in seconds (mirrors the Rust registry default). */
const UNDO_SEND_WINDOW_DEFAULT = 10;
/** The maximum Undo-Send window in seconds (values clamp to 0..=60). */
const UNDO_SEND_WINDOW_MAX = 60;
/** The honest copy explaining the Undo-Send window (Story 8.3). */
const UNDO_SEND_SENTENCE =
  "Each message you send waits this many seconds before it leaves, so you can undo it. Set to 0 to send immediately.";

/**
 * The plain disclosure for the message-previews toggle (Story 10.1). Sentence case,
 * no exclamation marks, honest consequence-naming (project voice).
 */
const NOTIFY_PREVIEWS_SENTENCE =
  "Native notifications for new messages show the sender and chat, plus a short preview of the message. Turn this off to hide the message content: notifications then show only the sender and chat, never the text. Notifications are always on-device and never leave this Mac.";

/** The persistent inline state when iOS notification permission is denied (Story 14.3).
 * Fixed copy; never re-prompts (UX-DR28). */
const NOTIFICATIONS_OFF_SENTENCE = "Notifications are off for keeper in iOS Settings.";

/** Note that the app-icon badge needs the same iOS notification permission (Story 14.3). */
const BADGE_NEEDS_PERMISSION_SENTENCE =
  "The app icon badge needs the same permission, so it will not show until you turn notifications on.";

/**
 * Notifications section (Story 10.1): the "Show message previews" `Switch`, bound to
 * `notifySetPreviewEnabled`. Reads its initial state via `notifyGetPreviewEnabled()` on
 * open. On a persist failure the toggle reverts (honest — never claims a state that was
 * not saved).
 *
 * On the reduced (phone) tier it additionally renders (Story 14.3): the "App icon badge"
 * mode radio (reusing the shared {@link DOCK_BADGE_OPTIONS} + `dockBadgeMode*` IPC), and a
 * persistent permission-denied inline state (queried via `notificationPermissionState()`
 * on open) with an Open-Settings deep link and a note the badge needs the same permission.
 * Never re-prompts.
 */
function NotificationsSection({ open }: { open: boolean }) {
  // The reduced (phone) tier gets the permanent lifecycle-honesty copy below (the
  // canonical only-while-open sentence plus the badge-not-live note, Story 14.2), the
  // "App icon badge" mode radio, and the permission-denied inline state (Story 14.3).
  const reducedPlatform = useIsReducedCapabilityPlatform();
  // `undefined` = still loading; otherwise the resolved previews state.
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);
  // The iOS app-icon badge mode (reuses the shared DockBadgeMode). `undefined` = loading.
  const [badgeMode, setBadgeMode] = useState<DockBadgeMode | undefined>(undefined);
  // The OS notification-permission state (Story 14.3). `undefined` = still loading; the
  // persistent "off" surface renders only when this resolves to `"denied"`.
  const [permission, setPermission] = useState<NotificationPermission | undefined>(undefined);

  useEffect(() => {
    if (!open) {
      return;
    }
    setEnabled(undefined);
    setBadgeMode(undefined);
    setPermission(undefined);
    let cancelled = false;
    void notifyGetPreviewEnabled()
      .then((value) => {
        if (!cancelled) {
          setEnabled(value);
        }
      })
      .catch(() => {
        // On a read failure, fall back to the honest default (previews on).
        if (!cancelled) {
          setEnabled(true);
        }
      });
    // The badge radio and permission surface are reduced-tier-only; only probe their
    // backends there so desktop opens make no dead round-trip.
    if (reducedPlatform) {
      void dockBadgeModeGet()
        .then((value) => {
          if (!cancelled) {
            setBadgeMode(value);
          }
        })
        .catch(() => {
          // On a read failure, fall back to the honest default (badge all unreads).
          if (!cancelled) {
            setBadgeMode("all");
          }
        });
      void notificationPermissionState()
        .then((value) => {
          if (!cancelled) {
            setPermission(value);
          }
        })
        .catch(() => {
          // On a read failure treat as unknown — hide the persistent "off" surface.
          if (!cancelled) {
            setPermission("unknown");
          }
        });
    }
    return () => {
      cancelled = true;
    };
  }, [open, reducedPlatform]);

  // Monotonic token so a failed persist only reverts when no newer toggle superseded it.
  const writeId = useRef(0);
  const badgeWriteId = useRef(0);

  const onCheckedChange = (next: boolean) => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = enabled ?? true;
    setEnabled(next);
    void notifySetPreviewEnabled(next).catch(() => {
      if (id === writeId.current) {
        setEnabled(prev);
      }
    });
  };

  const onBadgeModeChange = (next: string) => {
    const value = next as DockBadgeMode;
    badgeWriteId.current += 1;
    const id = badgeWriteId.current;
    const prev = badgeMode ?? "all";
    setBadgeMode(value);
    void dockBadgeModeSet(value).catch(() => {
      if (id === badgeWriteId.current) {
        setBadgeMode(prev);
      }
    });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Notifications</p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="notify-previews">Show message previews</Label>
        <Switch
          id="notify-previews"
          checked={enabled ?? false}
          disabled={enabled === undefined}
          onCheckedChange={onCheckedChange}
        />
      </div>
      <p className="text-muted-foreground">{NOTIFY_PREVIEWS_SENTENCE}</p>
      {reducedPlatform && (
        <>
          <div className="mt-1 flex flex-col gap-2">
            <Label>App icon badge</Label>
            <RadioGroup
              value={badgeMode ?? ""}
              onValueChange={onBadgeModeChange}
              aria-label="App icon badge mode"
              className="gap-2"
            >
              {DOCK_BADGE_OPTIONS.map((option) => (
                <div key={option.value} className="flex items-center gap-2">
                  <RadioGroupItem
                    id={`app-badge-${option.value}`}
                    value={option.value}
                    disabled={badgeMode === undefined}
                  />
                  <Label htmlFor={`app-badge-${option.value}`} className="font-normal">
                    {option.label}
                  </Label>
                </div>
              ))}
            </RadioGroup>
          </div>
          <p className="text-muted-foreground">{NO_BACKGROUND_SYNC_SENTENCE}</p>
          <p className="text-muted-foreground">{BADGE_NOT_LIVE_SENTENCE}</p>
          {permission === "denied" && (
            <div className="mt-1 flex flex-col gap-2 rounded-md border border-border p-2">
              <p className="text-muted-foreground">{NOTIFICATIONS_OFF_SENTENCE}</p>
              <p className="text-muted-foreground">{BADGE_NEEDS_PERMISSION_SENTENCE}</p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="self-start"
                onClick={() => {
                  // Best-effort deep link through the Rust opener; never re-prompts.
                  void iosOpenAppSettings().catch(() => {});
                }}
              >
                Open Settings
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** Honest disclosure of what ⌘W (background) and ⌘Q (quit) do (Story 10.3, UX-DR17).
 * Sentence case, no exclamation marks; never promises push or notifications while quit. */
const BACKGROUND_QUIT_SENTENCE =
  "Closing the window with ⌘W keeps keeper running in the background: it stays signed in, keeps syncing, and still shows notifications. Quitting with ⌘Q stops syncing and notifications until you open keeper again. keeper never runs a background push service, so it cannot notify you while it is quit.";

/** The dock-badge mode options and their honest labels (Story 10.3). */
const DOCK_BADGE_OPTIONS: ReadonlyArray<{ value: DockBadgeMode; label: string }> = [
  { value: "all", label: "All unreads" },
  { value: "mentions", label: "Mentions only" },
  { value: "off", label: "Off" },
];

/**
 * Background & dock section (Story 10.3, FR-53): a dock-badge-mode `RadioGroup` (all
 * unreads / mentions only / off), a "Launch at login" `Switch` (off by default, backed by
 * the autostart plugin), a "Keep in menu bar" `Switch` (off by default), and honest
 * quit-vs-background copy. Each control loads its state on open and reverts on a persist
 * failure (honest — never claims a state that was not saved).
 */
function BackgroundSection({ open }: { open: boolean }) {
  // Launch-at-login and menu-bar presence are desktop-only capabilities; hide each
  // row wherever the platform lacks it. The whole section is additionally gated
  // behind `!reducedPlatform` at its call site (Story 14.2) — on the phone tier the
  // ⌘W-keeps-syncing copy would be a false background-delivery claim.
  const launchAtLogin = useCapabilitiesStore((s) => s.capabilities.launchAtLogin);
  const trayIcon = useCapabilitiesStore((s) => s.capabilities.trayIcon);
  // `undefined` = still loading; otherwise the resolved dock-badge mode.
  const [mode, setMode] = useState<DockBadgeMode | undefined>(undefined);
  const modeWriteId = useRef(0);
  // Launch-at-login + menu-bar presence: `undefined` = loading; otherwise the boolean.
  const [launch, setLaunch] = useState<boolean | undefined>(undefined);
  const launchWriteId = useRef(0);
  const [menuBar, setMenuBar] = useState<boolean | undefined>(undefined);
  const menuBarWriteId = useRef(0);

  useEffect(() => {
    if (!open) {
      return;
    }
    setMode(undefined);
    setLaunch(undefined);
    setMenuBar(undefined);
    let cancelled = false;
    void dockBadgeModeGet()
      .then((value) => {
        if (!cancelled) {
          setMode(value);
        }
      })
      .catch(() => {
        // On a read failure, fall back to the honest default (badge all unreads).
        if (!cancelled) {
          setMode("all");
        }
      });
    // Only probe the launch-at-login / menu-bar backends where the row is actually
    // shown — on the phone tier these are `Unsupported` commands, so gating the fetch
    // on the same capability that gates the row avoids a dead round-trip per open.
    if (launchAtLogin) {
      void launchAtLoginGet()
        .then((value) => {
          if (!cancelled) {
            setLaunch(value);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setLaunch(false);
          }
        });
    }
    if (trayIcon) {
      void menuBarPresenceGet()
        .then((value) => {
          if (!cancelled) {
            setMenuBar(value);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setMenuBar(false);
          }
        });
    }
    return () => {
      cancelled = true;
    };
  }, [open, launchAtLogin, trayIcon]);

  const onModeChange = (next: string) => {
    const value = next as DockBadgeMode;
    modeWriteId.current += 1;
    const id = modeWriteId.current;
    const prev = mode ?? "all";
    setMode(value);
    void dockBadgeModeSet(value).catch(() => {
      if (id === modeWriteId.current) {
        setMode(prev);
      }
    });
  };

  const onLaunchChange = (next: boolean) => {
    launchWriteId.current += 1;
    const id = launchWriteId.current;
    const prev = launch ?? false;
    setLaunch(next);
    void launchAtLoginSet(next).catch(() => {
      if (id === launchWriteId.current) {
        setLaunch(prev);
      }
    });
  };

  const onMenuBarChange = (next: boolean) => {
    menuBarWriteId.current += 1;
    const id = menuBarWriteId.current;
    const prev = menuBar ?? false;
    setMenuBar(next);
    void menuBarPresenceSet(next).catch(() => {
      if (id === menuBarWriteId.current) {
        setMenuBar(prev);
      }
    });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Background &amp; dock</p>
      <div className="flex flex-col gap-2">
        <Label>Dock badge</Label>
        <RadioGroup
          value={mode ?? ""}
          onValueChange={onModeChange}
          aria-label="Dock badge mode"
          className="gap-2"
        >
          {DOCK_BADGE_OPTIONS.map((option) => (
            <div key={option.value} className="flex items-center gap-2">
              <RadioGroupItem
                id={`dock-badge-${option.value}`}
                value={option.value}
                disabled={mode === undefined}
              />
              <Label htmlFor={`dock-badge-${option.value}`} className="font-normal">
                {option.label}
              </Label>
            </div>
          ))}
        </RadioGroup>
      </div>
      {launchAtLogin && (
        <div className="mt-1 flex items-center justify-between gap-2">
          <Label htmlFor="launch-at-login">Launch at login</Label>
          <Switch
            id="launch-at-login"
            checked={launch ?? false}
            disabled={launch === undefined}
            onCheckedChange={onLaunchChange}
          />
        </div>
      )}
      {trayIcon && (
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="menu-bar-presence">Keep in menu bar</Label>
          <Switch
            id="menu-bar-presence"
            checked={menuBar ?? false}
            disabled={menuBar === undefined}
            onCheckedChange={onMenuBarChange}
          />
        </div>
      )}
      <p className="text-muted-foreground">{BACKGROUND_QUIT_SENTENCE}</p>
    </div>
  );
}

/**
 * Privacy section (Story 8.1): the global Incognito default `Switch`, bound to
 * `incognitoSetGlobal`. Reads its initial state via `incognitoGetGlobal()` on open and
 * mirrors the new value into the incognito store so open chats reflect it. On a persist
 * failure the toggle reverts (honest — never claims a state that was not saved).
 */
function PrivacySection({ open }: { open: boolean }) {
  // `undefined` = still loading; otherwise the resolved global default.
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    if (!open) {
      return;
    }
    setEnabled(undefined);
    let cancelled = false;
    void incognitoGetGlobal()
      .then((value) => {
        if (!cancelled) {
          setEnabled(value);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnabled(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Monotonic token so a failed persist only reverts when no newer toggle superseded it.
  const writeId = useRef(0);

  const onCheckedChange = (next: boolean) => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = enabled ?? false;
    setEnabled(next);
    // Mirror the new global into the store immediately for the global selector, then
    // bump the policy version once the write lands so any open chat re-reads its fully
    // resolved effective state (the chip/ring reconcile without a room reopen).
    incognitoStore.getState().applyGlobal(next);
    void incognitoSetGlobal(next)
      .then(() => {
        incognitoStore.getState().bumpPolicyVersion();
      })
      .catch(() => {
        if (id === writeId.current) {
          setEnabled(prev);
          incognitoStore.getState().applyGlobal(prev);
          incognitoStore.getState().bumpPolicyVersion();
        }
      });
  };

  // Undo-Send window in seconds (Story 8.3): `undefined` = still loading; otherwise the
  // resolved 0..=60 value (0 disables holding). Load-on-open + optimistic write with
  // revert, mirroring the Incognito toggle above.
  // Named `undoWindow` (not `window`) so it does not shadow the browser global in this
  // component's scope.
  const [undoWindow, setUndoWindow] = useState<number | undefined>(undefined);
  const windowWriteId = useRef(0);

  useEffect(() => {
    if (!open) {
      return;
    }
    setUndoWindow(undefined);
    let cancelled = false;
    void undoSendWindow()
      .then((value) => {
        if (!cancelled) {
          setUndoWindow(value);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setUndoWindow(UNDO_SEND_WINDOW_DEFAULT);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const onWindowChange = (raw: string) => {
    // Parse + clamp to 0..=60 locally so the field never shows an out-of-range value;
    // Rust clamps again defensively. A non-numeric entry is ignored (keeps the prior).
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) {
      return;
    }
    const clamped = Math.min(UNDO_SEND_WINDOW_MAX, Math.max(0, parsed));
    windowWriteId.current += 1;
    const id = windowWriteId.current;
    const prev = undoWindow ?? UNDO_SEND_WINDOW_DEFAULT;
    setUndoWindow(clamped);
    void setUndoSendWindow(clamped).catch(() => {
      if (id === windowWriteId.current) {
        setUndoWindow(prev);
      }
    });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Privacy</p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="incognito-global">Incognito by default</Label>
        <Switch
          id="incognito-global"
          checked={enabled ?? false}
          disabled={enabled === undefined}
          onCheckedChange={onCheckedChange}
        />
      </div>
      <p className="text-muted-foreground">{INCOGNITO_GLOBAL_SENTENCE}</p>
      <div className="mt-1 flex items-center justify-between gap-2">
        <Label htmlFor="undo-send-window">Undo-Send window (seconds)</Label>
        <Input
          id="undo-send-window"
          type="number"
          min={0}
          max={UNDO_SEND_WINDOW_MAX}
          className="w-20"
          value={undoWindow ?? ""}
          disabled={undoWindow === undefined}
          onChange={(e) => onWindowChange(e.target.value)}
        />
      </div>
      <p className="text-muted-foreground">{UNDO_SEND_SENTENCE}</p>
    </div>
  );
}

/** The honest copy explaining what verifying a device unlocks (Story 3.1). */
const ENCRYPTION_HONESTY_SENTENCE =
  "Verifying this device unlocks encrypted history and lets other people trust your messages.";

/**
 * Encryption section (Story 3.1 + 3.2): lists each signed-in account's device
 * state (Verified / Not verified) from the encryption-status store, plus the
 * honest sentence on what verifying unlocks. An account whose device is
 * `unverified` gets an interactive "Verify" button (Story 3.2) that opens the
 * device-verification modal for that account.
 */
function EncryptionSection() {
  const accounts = useAccountsStore((s) => s.accounts);

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Encryption</p>
      {accounts.length === 0 ? (
        <p className="text-muted-foreground">No accounts signed in.</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {accounts.map((account) => (
            <EncryptionAccountRow key={account.accountId} accountId={account.accountId}>
              {account.userId}
            </EncryptionAccountRow>
          ))}
        </ul>
      )}
      <p className="text-muted-foreground">{ENCRYPTION_HONESTY_SENTENCE}</p>
    </div>
  );
}

/**
 * Setup section (Story 6.8): a "Run setup again" entry that re-opens the
 * session-scoped first-run wizard over the shell and closes Settings. The wizard
 * is fully re-runnable; `accountId` defaults to the first account on re-entry.
 */
function SetupSection({ onOpenChange }: { onOpenChange: (open: boolean) => void }) {
  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Setup</p>
      <div className="flex items-center justify-between gap-2">
        <p className="text-muted-foreground">Walk through the first-run setup again.</p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            wizardStore.getState().start();
            onOpenChange(false);
          }}
        >
          Run setup again
        </Button>
      </div>
    </div>
  );
}

/** The honest copy explaining what to enable when the OS-global hotkey is not
 * currently registered (`active === false`) — Story 9.4, FR-50. macOS
 * `RegisterEventHotKey` does not require a specific permission API, so the copy points
 * at the general place to check rather than over-claiming a single toggle. */
const HOTKEY_PERMISSION_SENTENCE =
  "The summon hotkey isn't registered with macOS right now. Another app may already own this shortcut, or keeper may need permission — check System Settings → Privacy & Security (Accessibility) and Keyboard shortcuts, then reassign it below.";

/**
 * Settings → Shortcuts section (Story 9.4, FR-50). Shows the OS-global summon hotkey as
 * `Kbd` glyph chips, a "Change…" capture control that records the next chord
 * ({@link acceleratorFromEvent}) and reassigns via {@link hotkeySet}, a soft conflict
 * warning when the binding collides with a known macOS system shortcut, an honest
 * explanation when the binding is not registered with the OS (`active === false`), and a
 * "Reset to default" button. The VM is rendered as-is — conflict/registration state is
 * never derived in TS.
 */
function ShortcutsSection({ open }: { open: boolean }) {
  // `undefined` = still loading; otherwise the resolved binding VM.
  const [hotkey, setHotkey] = useState<HotkeyVm | undefined>(undefined);
  // Whether the capture control is armed and listening for the next chord.
  const [capturing, setCapturing] = useState(false);
  // The last reassignment error (OS refused / malformed), or `null`.
  const [error, setError] = useState<string | null>(null);
  const writeId = useRef(0);

  useEffect(() => {
    if (!open) {
      return;
    }
    setHotkey(undefined);
    setCapturing(false);
    setError(null);
    let cancelled = false;
    void hotkeyGet()
      .then((vm) => {
        if (!cancelled) {
          setHotkey(vm);
        }
      })
      .catch(() => {
        // On a read failure leave the section in its loading state rather than
        // asserting a (possibly wrong) binding.
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Persist a reassignment (from capture or reset), replacing the shown binding on
  // success and surfacing the error on a hard failure without losing the old binding.
  const assign = (accelerator: string) => {
    writeId.current += 1;
    const id = writeId.current;
    setError(null);
    void hotkeySet(accelerator)
      .then((vm) => {
        if (id === writeId.current) {
          setHotkey(vm);
        }
      })
      .catch((raw: unknown) => {
        if (id !== writeId.current) {
          return;
        }
        const message =
          typeof raw === "object" && raw !== null && "message" in raw
            ? String((raw as { message: unknown }).message)
            : "Could not set that shortcut.";
        setError(message);
      });
  };

  // While capturing, translate the next complete chord into an accelerator and assign
  // it. A bare modifier press yields `null` and keeps capturing; Escape cancels.
  const onCaptureKeyDown = (event: React.KeyboardEvent) => {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      setCapturing(false);
      return;
    }
    const accelerator = acceleratorFromEvent(event.nativeEvent);
    if (accelerator === null) {
      return;
    }
    setCapturing(false);
    assign(accelerator);
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Shortcuts</p>
      <div className="flex items-center justify-between gap-2">
        <Label>Summon keeper</Label>
        <div className="flex items-center gap-2">
          {capturing ? (
            <button
              type="button"
              // biome-ignore lint/a11y/noAutofocus: capture is an explicit user action; the field must receive the next keystroke immediately.
              autoFocus
              onKeyDown={onCaptureKeyDown}
              onBlur={() => setCapturing(false)}
              className="rounded-sm border border-ring px-2 py-0.5 text-muted-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              Press a shortcut… (Esc to cancel)
            </button>
          ) : (
            <Kbd aria-label={hotkey === undefined ? "Loading shortcut" : hotkey.accelerator}>
              {hotkey === undefined ? "…" : formatAccelerator(hotkey.accelerator)}
            </Kbd>
          )}
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={hotkey === undefined || capturing}
            onClick={() => {
              setError(null);
              setCapturing(true);
            }}
          >
            Change…
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={hotkey === undefined || hotkey.isDefault}
            onClick={() => assign(DEFAULT_GLOBAL_HOTKEY)}
          >
            Reset to default
          </Button>
        </div>
      </div>
      {hotkey?.conflict != null && (
        <p className="text-held text-xs" role="status">
          {hotkey.conflict}
        </p>
      )}
      {hotkey !== undefined && !hotkey.active && (
        <p className="text-held text-xs" role="status">
          {HOTKEY_PERMISSION_SENTENCE}
        </p>
      )}
      {error !== null && (
        <p className="text-held text-xs" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

/** The honest local-only disclosure for the Recording section (Story 16.3).
 * Recording voice: sentence case, no exclamation marks, honest local-only framing.
 * Recording adds zero network destinations. */
const RECORDING_LOCAL_ONLY_SENTENCE =
  "Screen recording saves to a folder on this Mac. Nothing uploads. Recording setup arrives in a later update.";

/**
 * Settings → Recording section (Story 16.3). Desktop-macOS-≥13 only — the whole
 * section is capability-gated at its call site so it is absent (never a dead
 * affordance) on platforms that cannot record. This story ships only the honest
 * placeholder shell (the real controls arrive in later Epic 16 stories),
 * following the {@link ShortcutsSection} idiom: a bordered section, a title, and
 * honest placeholder copy.
 */
function RecordingSection() {
  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Recording</p>
      <p className="text-muted-foreground">{RECORDING_LOCAL_ONLY_SENTENCE}</p>
    </div>
  );
}

/** One account's device-verification state row. Three honest states, never
 * over-claiming: `verified` reads "Verified"; an explicit `unverified` reads
 * "Not verified"; and `unknown`/pending (crypto not yet reported) reads a neutral
 * "Checking…" — the same "no false nag before crypto syncs" rule the banner
 * honors, so a device mid-sync is never labelled a problem. */
function EncryptionAccountRow({ accountId, children }: { accountId: string; children: ReactNode }) {
  const status = useEncryptionStatus(accountId);
  const label =
    status === "verified" ? "Verified" : status === "unverified" ? "Not verified" : "Checking…";
  // Only an explicit `unverified` gets the attention tone; verified and the
  // transient checking state stay muted.
  const tone = status === "unverified" ? "text-held text-xs" : "text-muted-foreground text-xs";
  return (
    <li className="flex flex-col gap-1">
      <div className="flex items-center justify-between gap-2">
        <span
          className="truncate font-mono text-xs"
          title={typeof children === "string" ? children : undefined}
        >
          {children}
        </span>
        <span className="flex items-center gap-2">
          <span className={tone}>{label}</span>
          {status === "unverified" ? (
            <Button
              type="button"
              variant="outline"
              size="xs"
              onClick={() => verificationStore.getState().openFor(accountId)}
            >
              Verify
            </Button>
          ) : null}
        </span>
      </div>
      <BackupAccountRow accountId={accountId} />
    </li>
  );
}

/** One account's key-backup state line (Story 3.3, FR-14, AC3): four honest
 * states sourced from the Rust core. `disabled` → a "Set up backup" button
 * (enable); `incomplete` → a "Restore" button (the fresh-login "Needs your
 * recovery key" case); `enabled` → "Backup on"; `unknown`/pending → "Checking…"
 * (no false claim before crypto syncs). */
function BackupAccountRow({ accountId }: { accountId: string }) {
  const status = useKeyBackupStatus(accountId);
  const label =
    status === "enabled"
      ? "Backup on"
      : status === "disabled"
        ? "Not set up"
        : status === "incomplete"
          ? "Needs your recovery key"
          : "Checking…";
  // Only `incomplete` (locked history awaiting restore) gets the attention tone.
  const tone = status === "incomplete" ? "text-held text-xs" : "text-muted-foreground text-xs";
  return (
    <div className="flex items-center justify-between gap-2 pl-1">
      <span className="text-muted-foreground text-xs">Key backup</span>
      <span className="flex items-center gap-2">
        <span className={tone}>{label}</span>
        {status === "disabled" ? (
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => keyBackupStore.getState().openEnable(accountId)}
          >
            Set up backup
          </Button>
        ) : null}
        {status === "incomplete" ? (
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => keyBackupStore.getState().openRestore(accountId)}
          >
            Restore
          </Button>
        ) : null}
      </span>
    </div>
  );
}

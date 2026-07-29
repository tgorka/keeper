import { type MouseEvent, useCallback, useRef } from "react";
import { ApprovalPane } from "@/components/approval/approval-pane";
import { NewChatDialog } from "@/components/chat/new-chat-dialog";
import { CheatSheetOverlay } from "@/components/cheat-sheet/cheat-sheet-overlay";
import { CommandPalette } from "@/components/command-palette/command-palette";
import { ExportDialog } from "@/components/export/export-dialog";
import { BridgesPane } from "@/components/layout/bridges-pane";
import { ChatListPane } from "@/components/layout/chat-list-pane";
import { ConversationPane } from "@/components/layout/conversation-pane";
import { DetailPanel } from "@/components/layout/detail-panel";
import { PhoneShell } from "@/components/layout/phone-shell";
import { RecordingPane } from "@/components/layout/recording-pane";
import { SIDEBAR_WIDTH_CLASS, SidebarPane } from "@/components/layout/sidebar-pane";
import { SyncPane } from "@/components/layout/sync-pane";
import { VerifyBanner } from "@/components/layout/verify-banner";
import { SearchOverlay } from "@/components/search/search-overlay";
import { DeviceVerificationDialog } from "@/components/settings/device-verification-dialog";
import { KeyBackupDialog } from "@/components/settings/key-backup-dialog";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAccountStatuses } from "@/hooks/use-account-statuses";
import { useApprovalShortcut } from "@/hooks/use-approval-shortcut";
import { useBridgeHealthSubscription } from "@/hooks/use-bridge-health";
import { useBridgesShortcut } from "@/hooks/use-bridges-shortcut";
import { useCheatSheetShortcut } from "@/hooks/use-cheat-sheet-shortcut";
import { useCommandPaletteShortcut } from "@/hooks/use-command-palette-shortcut";
import { useEncryptionStatuses } from "@/hooks/use-encryption-statuses";
import { useGlobalHotkey } from "@/hooks/use-global-hotkey";
import { useKeyBackupStatuses } from "@/hooks/use-key-backup-statuses";
import { useMenuActions } from "@/hooks/use-menu-actions";
import { useNewChatShortcut } from "@/hooks/use-new-chat-shortcut";
import { useQuickSwitcher } from "@/hooks/use-quick-switcher";
import { useRecordingHotkey } from "@/hooks/use-recording-hotkey";
import { useRecordingShortcut } from "@/hooks/use-recording-shortcut";
import { useSearchShortcuts } from "@/hooks/use-search-shortcuts";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useUnreadJump } from "@/hooks/use-unread-jump";
import { useVerification } from "@/hooks/use-verification";
import { useViewShortcuts } from "@/hooks/use-view-shortcuts";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { useDetailStore } from "@/lib/stores/detail-ui";
import { usePrimaryView } from "@/lib/stores/primary-view";
import { beginTitleBarDrag } from "@/lib/titlebar-drag";
import { cn } from "@/lib/utils";

export function AppShell() {
  const { phone, sidebarCollapsed, detailFloating } = useShellLayout();
  // Stream every account's connectivity into the per-account status store: the
  // switcher glyphs, the shell offline pill, and the "Queued" send caption are
  // all pure projections of that single map.
  useAccountStatuses();
  // Stream every account's device-verification status into the encryption store:
  // the verify banner and the Settings badge are pure projections of that map.
  useEncryptionStatuses();
  // Subscribe every account's interactive verification flow: an incoming request
  // auto-opens the device-verification modal, and keeper-started flows stream here.
  useVerification();
  // Stream every account's key-backup status into the key-backup store: the
  // Settings backup row is a pure projection of that map.
  useKeyBackupStatuses();
  // Stream live bridge-session health across every account into the bridge-health
  // store: the card dot + state word, the sidebar roll-up, the affected chat-row dot,
  // and the in-conversation re-link banner are pure projections of that map (Story 6.5).
  useBridgeHealthSubscription();
  // Wire the search entry points (⌘⇧F global, ⌘F in-chat) to the search surface.
  useSearchShortcuts();
  // Wire ⌘N to the new-chat dialog (Story 6.6).
  useNewChatShortcut();
  // Wire ⌘4 to the Bridges surface (Story 6.1).
  useBridgesShortcut();
  // Wire ⌘5 to the Recording surface (Story 16.3); a no-op unless the recording
  // capability is on (desktop macOS ≥ 13.0).
  useRecordingShortcut();
  // Wire ⌘3 to the Approval Pane (Story 7.3).
  useApprovalShortcut();
  // Wire ⌘K to toggle the command palette (Story 9.1).
  useCommandPaletteShortcut();
  // Wire ⌘? to toggle the shortcut cheat sheet (Story 9.3).
  useCheatSheetShortcut();
  // Route native-menu clicks through the shared palette dispatch (Story 9.3).
  useMenuActions();
  // Wire ⌘1/⌘2 to Inbox/Archive (Story 9.2), completing the ⌘1–4 view set.
  useViewShortcuts();
  // Wire ⌃Tab/⌃⇧Tab to cycle the open chat over the rendered window (Story 9.2).
  useQuickSwitcher();
  // Wire ⌥⌘↓/⌥⌘↑ to jump next/previous-unread in the rendered window (Story 9.2).
  useUnreadJump();
  // Listen for the OS-global summon hotkey (Story 9.4): a raise switches to Inbox and
  // moves keyboard focus into the chat list via the focus-request nonce store.
  useGlobalHotkey();
  // Listen for the optional OS-global Start/Stop Recording hotkey (Story 20.4):
  // a press toggles capture through the shared recording-control module. The
  // hook self-gates on the `recording` capability (inert everywhere else).
  useRecordingHotkey();
  // Which primary view the shell renders. "bridges" and "approval" each replace the
  // chat-list + conversation cluster with a full-surface pane (Story 6.1 / 7.3).
  const primaryView = usePrimaryView();
  // Detail-open lives in the lifted `detailStore` (Story 13.1) so the desktop
  // frame and the phone stack project one shared signal; the toggle-focus-return
  // on close stays here, wrapping the store's `closeDetail`.
  const detailOpen = useDetailStore((s) => s.open);
  const openDetail = useDetailStore((s) => s.openDetail);
  const storeCloseDetail = useDetailStore((s) => s.closeDetail);
  const toggleDetail = useDetailStore((s) => s.toggleDetail);
  const toggleRef = useRef<HTMLButtonElement>(null);
  // The ⌘? cheat-sheet overlay and the native menu bar are the two projections of
  // the same action registry (Story 9.3), so `nativeMenuBar` is the honest flag:
  // where there's no native menu bar (the phone tier) the cheat sheet is unmounted.
  // The `useCheatSheetShortcut()` hook stays wired above (rules-of-hooks); only the
  // overlay is gated, so an unmounted overlay simply cannot render.
  const nativeMenuBar = useCapabilitiesStore((s) => s.capabilities.nativeMenuBar);
  // Screen recording is a desktop-macOS-≥13 capability (Story 16.3): the ⌘5
  // Recording view renders only when the flag is on, so a stale "recording"
  // primary-view can never show the pane on a platform that cannot record.
  const recording = useCapabilitiesStore((s) => s.capabilities.recording);
  // Folder sync needs a usable `git` (Story 32.5, AD-41): same rule, so a stale
  // "sync" primary-view can never show the pane where sync cannot run.
  const sync = useCapabilitiesStore((s) => s.capabilities.sync);
  // Where the platform floats the window controls over the webview (desktop macOS,
  // via the macOS-only `titleBarStyle`/`hiddenTitle` keys) the app owes the window
  // its own drag region; under a real title bar the same band would be empty space
  // under chrome the OS already draws, so it is absent there (AD-34-2).
  const overlayTitleBar = useCapabilitiesStore((s) => s.capabilities.overlayTitleBar);

  const closeDetail = useCallback(() => {
    storeCloseDetail();
    // Return focus to the toggle control on close.
    toggleRef.current?.focus();
  }, [storeCloseDetail]);

  const handleSheetOpenChange = useCallback(
    (open: boolean) => {
      if (open) {
        openDetail();
      } else {
        closeDetail();
      }
    },
    [openDetail, closeDetail],
  );

  // The band starts the window drag itself rather than relying only on Tauri's
  // `data-tauri-drag-region` shim, which invokes the same `start_dragging` command
  // and then drops the promise — so a refused drag moves nothing and says nothing.
  // `beginTitleBarDrag` issues the call where its outcome can be recorded (Story
  // 34.3). Both columns keep the attribute: wherever this handler does not run,
  // the shim still behaves exactly as it did before.
  const handleBandMouseDown = useCallback((event: MouseEvent<HTMLDivElement>) => {
    // Primary button; a direct hit on the band itself, which is Tauri's own rule
    // for a bare `data-tauri-drag-region` and keeps a future child of the band
    // from becoming a drag handle; and the opening click only, because on macOS
    // the shim implements double-click-to-zoom on the following `mouseup` and a
    // drag started from the second `mousedown` would swallow that gesture.
    if (event.button !== 0 || event.detail !== 1 || event.target !== event.currentTarget) {
      return;
    }
    // Take the gesture over instead of letting the shim fire as well: one
    // `start_dragging` per mouse-down keeps the recorded outcome attributable to
    // this call, and `preventDefault` is what the shim spends the event on too —
    // suppressing the text cursor. Nothing here reads or writes React state, so
    // the mouse-down cannot re-render the band out from under the drag, and the
    // callback closes over nothing that could go stale.
    event.preventDefault();
    event.stopPropagation();
    void beginTitleBarDrag();
  }, []);

  return (
    <TooltipProvider>
      <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
        {/* One drag band, painted per column (AD-34-2, AD-34-3). It is the single
            element that both makes the window movable and clears the floating
            window controls, which is why no pane reserves an inset of its own. The
            band spans two panes with two different backgrounds, so it is painted
            per column to match what sits beneath it: one full-width `bg-background`
            strip above a `bg-sidebar` drawer reads as a seam in light mode and a
            black bar in dark, and that is the whole of the reported black strip. */}
        {overlayTitleBar && (
          <div className="flex h-7 shrink-0">
            {!phone && (
              // biome-ignore lint/a11y/noStaticElementInteractions: the band is window chrome with no accessible semantics — an empty 28px strip whose only affordance is dragging the window with a pointer, which has no keyboard or AT analogue (the OS moves windows through its own Window menu).
              <div
                data-tauri-drag-region
                onMouseDown={handleBandMouseDown}
                className={cn(
                  "h-full shrink-0 bg-sidebar",
                  sidebarCollapsed ? SIDEBAR_WIDTH_CLASS.collapsed : SIDEBAR_WIDTH_CLASS.expanded,
                )}
              />
            )}
            {/* biome-ignore lint/a11y/noStaticElementInteractions: same as the drawer
                column above — pointer-only window chrome, no accessible semantics. */}
            <div
              data-tauri-drag-region
              onMouseDown={handleBandMouseDown}
              className="h-full flex-1 bg-background"
            />
          </div>
        )}
        <VerifyBanner />
        <div className="flex min-h-0 flex-1">
          {phone ? (
            // Below 768px the single-pane stack replaces the sidebar + panes row
            // (Story 13.1); the global overlays/dialogs/shortcut hooks below stay
            // mounted in both arrangements.
            <PhoneShell />
          ) : (
            <>
              <SidebarPane collapsed={sidebarCollapsed} />
              {recording && primaryView === "recording" ? (
                <RecordingPane />
              ) : sync && primaryView === "sync" ? (
                <SyncPane />
              ) : primaryView === "bridges" ? (
                <BridgesPane />
              ) : primaryView === "approval" ? (
                <ApprovalPane />
              ) : (
                <>
                  <ChatListPane />
                  <ConversationPane
                    detailOpen={detailOpen}
                    onToggleDetail={toggleDetail}
                    toggleRef={toggleRef}
                  />
                  {detailOpen && !detailFloating && <DetailPanel />}
                </>
              )}
            </>
          )}
        </div>
      </div>

      <DeviceVerificationDialog />
      <KeyBackupDialog />
      <SearchOverlay />
      <ExportDialog />
      <NewChatDialog />
      <CommandPalette />
      {nativeMenuBar && <CheatSheetOverlay />}

      {detailFloating && !phone && (
        <Sheet open={detailOpen} onOpenChange={handleSheetOpenChange}>
          <SheetContent side="right" className="w-[320px] p-0 sm:max-w-[320px]">
            <SheetHeader className="sr-only">
              <SheetTitle>Details</SheetTitle>
            </SheetHeader>
            <DetailPanel floating />
          </SheetContent>
        </Sheet>
      )}
    </TooltipProvider>
  );
}

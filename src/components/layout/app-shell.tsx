import { useCallback, useRef } from "react";
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
import { SidebarPane } from "@/components/layout/sidebar-pane";
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
import { useRecordingShortcut } from "@/hooks/use-recording-shortcut";
import { useSearchShortcuts } from "@/hooks/use-search-shortcuts";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useUnreadJump } from "@/hooks/use-unread-jump";
import { useVerification } from "@/hooks/use-verification";
import { useViewShortcuts } from "@/hooks/use-view-shortcuts";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { useDetailStore } from "@/lib/stores/detail-ui";
import { usePrimaryView } from "@/lib/stores/primary-view";

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

  return (
    <TooltipProvider>
      <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
        {/* Draggable overlay titlebar band (~28px, standard macOS height) so the
            window stays movable and the traffic lights float over empty space
            above the panes rather than overlapping pane content in any state. */}
        <div data-tauri-drag-region className="h-7 shrink-0" />
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

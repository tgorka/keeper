import { useCallback, useRef, useState } from "react";
import { ChatListPane } from "@/components/layout/chat-list-pane";
import { ConversationPane } from "@/components/layout/conversation-pane";
import { DetailPanel } from "@/components/layout/detail-panel";
import { SidebarPane } from "@/components/layout/sidebar-pane";
import { VerifyBanner } from "@/components/layout/verify-banner";
import { SearchOverlay } from "@/components/search/search-overlay";
import { DeviceVerificationDialog } from "@/components/settings/device-verification-dialog";
import { KeyBackupDialog } from "@/components/settings/key-backup-dialog";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAccountStatuses } from "@/hooks/use-account-statuses";
import { useEncryptionStatuses } from "@/hooks/use-encryption-statuses";
import { useKeyBackupStatuses } from "@/hooks/use-key-backup-statuses";
import { useSearchShortcuts } from "@/hooks/use-search-shortcuts";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useVerification } from "@/hooks/use-verification";

export function AppShell() {
  const { sidebarCollapsed, detailFloating } = useShellLayout();
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
  // Wire the search entry points (⌘⇧F global, ⌘F in-chat) to the search surface.
  useSearchShortcuts();
  const [detailOpen, setDetailOpen] = useState(false);
  const toggleRef = useRef<HTMLButtonElement>(null);

  const openDetail = useCallback(() => setDetailOpen(true), []);
  const closeDetail = useCallback(() => {
    setDetailOpen(false);
    // Return focus to the toggle control on close.
    toggleRef.current?.focus();
  }, []);
  const toggleDetail = useCallback(() => {
    setDetailOpen((prev) => !prev);
  }, []);

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
          <SidebarPane collapsed={sidebarCollapsed} />
          <ChatListPane />
          <ConversationPane
            detailOpen={detailOpen}
            onToggleDetail={toggleDetail}
            toggleRef={toggleRef}
          />
          {detailOpen && !detailFloating && <DetailPanel />}
        </div>
      </div>

      <DeviceVerificationDialog />
      <KeyBackupDialog />
      <SearchOverlay />

      {detailFloating && (
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

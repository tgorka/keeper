import { Archive, Inbox, MessageSquare, Radio, Settings, WifiOff } from "lucide-react";
import { useState } from "react";
import { AccountFooter } from "@/components/layout/account-footer";
import { NetworksGroup } from "@/components/layout/networks-group";
import { SpacesGroup } from "@/components/layout/spaces-group";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BridgeHealth } from "@/lib/ipc/client";
import { useShellOffline } from "@/lib/stores/account-status";
import { useWorstBridgeHealth } from "@/lib/stores/bridge-health";
import { usePendingDraftCount } from "@/lib/stores/drafts";
import { primaryViewStore, usePrimaryView } from "@/lib/stores/primary-view";
import { cn } from "@/lib/utils";

interface SidebarView {
  label: string;
  icon: typeof MessageSquare;
}

const VIEWS: SidebarView[] = [
  { label: "Chats", icon: MessageSquare },
  { label: "Archive", icon: Archive },
  { label: "Approvals", icon: Inbox },
  { label: "Bridges", icon: Radio },
  { label: "Settings", icon: Settings },
];

/** The `--bridge-*` tint class for a rolled-up worst health (Story 6.5). */
const HEALTH_DOT_CLASS: Record<BridgeHealth, string> = {
  healthy: "bg-bridge-healthy",
  degraded: "bg-bridge-degraded",
  disconnected: "bg-bridge-disconnected",
};

interface SidebarPaneProps {
  collapsed: boolean;
}

/** Exact offline-pill copy (UX-DR18) — kept verbatim. Exported so the phone
 * pull-to-refresh (Story 13.6) resolves its spinner into the same pill copy. */
export const OFFLINE_PILL_TEXT =
  "Offline — showing your local archive. Messages queue until you're back.";

export function SidebarPane({ collapsed }: SidebarPaneProps) {
  const offline = useShellOffline();
  // Controlled state for the Settings dialog (Story 2.6). Only the Settings view
  // button opens it.
  const [settingsOpen, setSettingsOpen] = useState(false);
  // The active primary view (Story 4.2 / 6.1): "Chats" switches to the Unified
  // Inbox, "Archive" to the Archive window, "Bridges" to the Bridges surface.
  // Reflected as `aria-current` + accent styling.
  const primaryView = usePrimaryView();
  // The sidebar Bridges health roll-up (Story 6.5): the single worst state across
  // every monitored bridge session, rolled up from the Rust-authoritative
  // bridge-health store. `null` when nothing is monitored (no dot).
  const bridgeHealth = useWorstBridgeHealth();
  // The count of chats with a pending draft across all accounts (Story 7.3). Drives
  // the amber "Approvals" count badge — shown only when at least one draft is held.
  const pendingDraftCount = usePendingDraftCount();

  return (
    <nav
      aria-label="Views"
      className={cn(
        "flex h-full shrink-0 flex-col border-border border-r bg-sidebar",
        collapsed ? "w-12" : "w-[260px]",
      )}
    >
      {/* Reserve the macOS traffic-light inset (78x12px) in every state. */}
      <div className={cn("shrink-0", collapsed ? "pt-3 pl-3" : "pt-3 pl-[78px]")} />
      <ul className={cn("flex flex-col gap-1 p-2", collapsed && "items-center")}>
        {VIEWS.map((view) => {
          const Icon = view.icon;
          // "Chats", "Archive", and "Bridges" switch the primary view; Settings
          // opens the dialog.
          const onClick =
            view.label === "Settings"
              ? () => setSettingsOpen(true)
              : view.label === "Chats"
                ? () => primaryViewStore.getState().setView("inbox")
                : view.label === "Archive"
                  ? () => primaryViewStore.getState().setView("archive")
                  : view.label === "Approvals"
                    ? () => primaryViewStore.getState().setView("approval")
                    : view.label === "Bridges"
                      ? () => primaryViewStore.getState().setView("bridges")
                      : undefined;
          // Reflect the active primary view on the Chats/Archive/Approvals/Bridges
          // entries.
          const active =
            (view.label === "Chats" && primaryView === "inbox") ||
            (view.label === "Archive" && primaryView === "archive") ||
            (view.label === "Approvals" && primaryView === "approval") ||
            (view.label === "Bridges" && primaryView === "bridges");
          // The Bridges entry carries the worst-state health roll-up dot (Story
          // 6.1): shown only when at least one bridge reports non-null health.
          const healthDot =
            view.label === "Bridges" && bridgeHealth !== null ? (
              <span
                aria-hidden="true"
                data-slot="bridge-health-rollup"
                className={cn(
                  "ml-auto size-2 shrink-0 rounded-full",
                  HEALTH_DOT_CLASS[bridgeHealth],
                )}
              />
            ) : null;
          // The "Approvals" entry carries an amber count badge (Story 7.3): the
          // number of chats with a pending draft, shown only when > 0 ("written,
          // not sent"). Amber (`--held`) marks the badge — nothing else.
          const showApprovalBadge = view.label === "Approvals" && pendingDraftCount > 0;
          const approvalBadge = showApprovalBadge ? (
            <span
              data-slot="approval-count"
              aria-hidden="true"
              className="ml-auto inline-flex min-w-5 shrink-0 items-center justify-center rounded-full bg-held px-1.5 py-0.5 font-medium text-[11px] text-held-foreground leading-none"
            >
              {pendingDraftCount}
            </span>
          ) : null;
          if (collapsed) {
            return (
              <li key={view.label}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={
                        showApprovalBadge
                          ? `${view.label}, ${pendingDraftCount} pending`
                          : view.label
                      }
                      aria-current={active ? "page" : undefined}
                      className={cn(
                        "relative focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                        active && "bg-accent text-accent-foreground",
                      )}
                      onClick={onClick}
                    >
                      <Icon aria-hidden="true" />
                      {healthDot !== null && (
                        <span
                          aria-hidden="true"
                          data-slot="bridge-health-rollup"
                          className={cn(
                            "absolute top-1.5 right-1.5 size-2 rounded-full",
                            bridgeHealth !== null && HEALTH_DOT_CLASS[bridgeHealth],
                          )}
                        />
                      )}
                      {showApprovalBadge && (
                        <span
                          aria-hidden="true"
                          data-slot="approval-count"
                          className="absolute top-1 right-1 size-2 rounded-full bg-held"
                        />
                      )}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="right">
                    {showApprovalBadge ? `${view.label} (${pendingDraftCount})` : view.label}
                  </TooltipContent>
                </Tooltip>
              </li>
            );
          }
          return (
            <li key={view.label}>
              <Button
                type="button"
                variant="ghost"
                aria-current={active ? "page" : undefined}
                className={cn(
                  "w-full justify-start gap-2 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                  active && "bg-accent text-accent-foreground",
                )}
                onClick={onClick}
              >
                <Icon aria-hidden="true" />
                {view.label}
                {healthDot}
                {approvalBadge}
              </Button>
            </li>
          );
        })}
      </ul>
      {/* SPACES group (Story 4.5): a single-select list of the Matrix Spaces the
          user belongs to, filtering the Unified Inbox. Rendered after the primary
          views, before the footer. Hidden entirely when there are no Spaces, and
          suppressed on the collapsed rail (it needs labels + names). */}
      {!collapsed && <SpacesGroup />}
      {/* NETWORKS group (Story 4.6): a single-select list of the distinct bridged
          Networks connected across all accounts, filtering the Unified Inbox.
          Rendered immediately after SPACES. Hidden entirely when there are no
          bridged rooms, and suppressed on the collapsed rail (needs labels). */}
      {!collapsed && <NetworksGroup />}
      {/* Persistent sidebar-footer region (pushed to the bottom with `mt-auto`):
          the offline pill directly ABOVE the account row, both inside the footer
          region. The account row is always mounted while signed in; the pill
          shows only while disconnected. */}
      <div className="mt-auto flex shrink-0 flex-col">
        {/* Persistent offline pill (UX-DR18): shown only while disconnected, using
            the amber `held` tokens. Non-interactive and keyboard-irrelevant;
            `role="status"` announces the connectivity change without a toast. No
            toasts for connectivity, ever. */}
        {offline &&
          (collapsed ? (
            <div
              role="status"
              aria-label={OFFLINE_PILL_TEXT}
              className="flex shrink-0 items-center justify-center border-border border-t bg-held/10 p-3 text-held"
            >
              <WifiOff aria-hidden="true" className="size-5" />
              {/* Real text content in addition to aria-label so the `role="status"`
                  live region is reliably announced by screen readers that read a
                  live region's *content* (not its label) when the rail is
                  collapsed; visually hidden behind the icon. */}
              <span className="sr-only">{OFFLINE_PILL_TEXT}</span>
            </div>
          ) : (
            <div
              role="status"
              className="flex shrink-0 items-start gap-2 border-border border-t bg-held/10 p-3 text-held text-xs"
            >
              <WifiOff aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
              <span>{OFFLINE_PILL_TEXT}</span>
            </div>
          ))}
        <AccountFooter collapsed={collapsed} />
      </div>
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </nav>
  );
}

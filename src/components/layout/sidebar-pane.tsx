import { Archive, MessageSquare, Radio, Settings, WifiOff } from "lucide-react";
import { useState } from "react";
import { AccountFooter } from "@/components/layout/account-footer";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useShellOffline } from "@/lib/stores/account-status";
import { primaryViewStore, usePrimaryView } from "@/lib/stores/primary-view";
import { cn } from "@/lib/utils";

interface SidebarView {
  label: string;
  icon: typeof MessageSquare;
}

const VIEWS: SidebarView[] = [
  { label: "Chats", icon: MessageSquare },
  { label: "Archive", icon: Archive },
  { label: "Bridges", icon: Radio },
  { label: "Settings", icon: Settings },
];

interface SidebarPaneProps {
  collapsed: boolean;
}

/** Exact offline-pill copy (UX-DR18) — kept verbatim. */
const OFFLINE_PILL_TEXT = "Offline — showing your local archive. Messages queue until you're back.";

export function SidebarPane({ collapsed }: SidebarPaneProps) {
  const offline = useShellOffline();
  // Controlled state for the Settings dialog (Story 2.6). Only the Settings view
  // button opens it; Bridges stays inert.
  const [settingsOpen, setSettingsOpen] = useState(false);
  // The active primary view (Story 4.2): "Chats" switches to the Unified Inbox,
  // "Archive" to the Archive window. Reflected as `aria-current` + accent styling.
  const primaryView = usePrimaryView();

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
          // "Chats" and "Archive" switch the primary view; Settings opens the
          // dialog; Bridges stays inert until a later story wires it.
          const onClick =
            view.label === "Settings"
              ? () => setSettingsOpen(true)
              : view.label === "Chats"
                ? () => primaryViewStore.getState().setView("inbox")
                : view.label === "Archive"
                  ? () => primaryViewStore.getState().setView("archive")
                  : undefined;
          // Reflect the active primary view on the Chats/Archive entries.
          const active =
            (view.label === "Chats" && primaryView === "inbox") ||
            (view.label === "Archive" && primaryView === "archive");
          if (collapsed) {
            return (
              <li key={view.label}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={view.label}
                      aria-current={active ? "page" : undefined}
                      className={cn(
                        "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                        active && "bg-accent text-accent-foreground",
                      )}
                      onClick={onClick}
                    >
                      <Icon aria-hidden="true" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="right">{view.label}</TooltipContent>
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
              </Button>
            </li>
          );
        })}
      </ul>
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

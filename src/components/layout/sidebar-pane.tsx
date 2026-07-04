import { MessageSquare, Radio, Settings, WifiOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useConnectionStore } from "@/lib/stores/connection";
import { cn } from "@/lib/utils";

interface SidebarView {
  label: string;
  icon: typeof MessageSquare;
}

const VIEWS: SidebarView[] = [
  { label: "Chats", icon: MessageSquare },
  { label: "Bridges", icon: Radio },
  { label: "Settings", icon: Settings },
];

interface SidebarPaneProps {
  collapsed: boolean;
}

/** Exact offline-pill copy (UX-DR18) — kept verbatim. */
const OFFLINE_PILL_TEXT = "Offline — showing your local archive. Messages queue until you're back.";

export function SidebarPane({ collapsed }: SidebarPaneProps) {
  const offline = useConnectionStore((s) => s.status === "offline");

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
                      className="focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
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
                className="w-full justify-start gap-2 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <Icon aria-hidden="true" />
                {view.label}
              </Button>
            </li>
          );
        })}
      </ul>
      {/* Persistent offline pill (UX-DR18): a sidebar-footer element shown only
          while disconnected, using the amber `held` tokens. Non-interactive and
          keyboard-irrelevant; `role="status"` announces the connectivity change
          without a toast. No toasts for connectivity, ever. */}
      {offline &&
        (collapsed ? (
          <div
            role="status"
            aria-label={OFFLINE_PILL_TEXT}
            className="mt-auto flex shrink-0 items-center justify-center border-border border-t bg-held/10 p-3 text-held"
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
            className="mt-auto flex shrink-0 items-start gap-2 border-border border-t bg-held/10 p-3 text-held text-xs"
          >
            <WifiOff aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
            <span>{OFFLINE_PILL_TEXT}</span>
          </div>
        ))}
    </nav>
  );
}

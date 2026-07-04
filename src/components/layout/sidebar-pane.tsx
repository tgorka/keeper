import { MessageSquare, Radio, Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
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

export function SidebarPane({ collapsed }: SidebarPaneProps) {
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
    </nav>
  );
}

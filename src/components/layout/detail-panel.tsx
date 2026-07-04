import { cn } from "@/lib/utils";

interface DetailPanelProps {
  /** When floating inside a Sheet the width/border are supplied by the Sheet. */
  floating?: boolean;
}

export function DetailPanel({ floating = false }: DetailPanelProps) {
  return (
    <aside
      aria-label="Details"
      className={cn(
        "flex h-full flex-col bg-background",
        !floating && "w-[320px] shrink-0 border-border border-l",
      )}
    >
      <div className="flex flex-1 items-center justify-center p-6">
        <p className="max-w-[16rem] text-center text-muted-foreground text-sm">
          Conversation details will appear here.
        </p>
      </div>
    </aside>
  );
}

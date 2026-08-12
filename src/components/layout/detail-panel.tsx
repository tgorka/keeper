import { cn } from "@/lib/utils";

interface DetailPanelProps {
  /** When floating inside a Sheet the width is supplied by the Sheet. */
  floating?: boolean;
}

export function DetailPanel({ floating = false }: DetailPanelProps) {
  return (
    <aside
      aria-label="Details"
      // No `border-l`. The seam on this panel's left belongs to the pane before
      // it, which draws its own `border-r` (DESIGN.md → Elevation & Depth: the
      // earlier sibling owns its trailing edge). Identical pixels, one owner.
      className={cn("flex h-full flex-col bg-background", !floating && "w-[320px] shrink-0")}
    >
      <div className="flex flex-1 items-center justify-center p-6">
        <p className="max-w-[16rem] text-center text-muted-foreground text-sm">
          Conversation details will appear here.
        </p>
      </div>
    </aside>
  );
}

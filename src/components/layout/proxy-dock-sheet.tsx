/**
 * The tablet's assistant sheet shares the Notes dock's database sessions
 * mirror. Selection is local to the sheet, never the Agents window's.
 */
import { MessagesSquare } from "lucide-react";
import { useState } from "react";
import { DockBody } from "@/components/notes/notes-agent-dock";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet";
import { IconHint } from "@/components/ui/tooltip";

/** The trigger's name and the sheet's title. */
export const PROXY_DOCK_LABEL = "Assistant";

export function ProxyDockSheet() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <IconHint label={PROXY_DOCK_LABEL}>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label={PROXY_DOCK_LABEL}
          onClick={() => setOpen(true)}
          className="size-11 shrink-0"
        >
          <MessagesSquare aria-hidden="true" />
        </Button>
      </IconHint>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent
          side="right"
          // The conversation needs a reading width; a tablet has it.
          className="gap-0 p-0 pt-[var(--safe-top)] pb-[var(--safe-bottom)] data-[side=right]:w-[min(30rem,100vw)] data-[side=right]:sm:max-w-none"
          data-testid="proxy-dock-sheet"
        >
          <header className="flex h-13 shrink-0 items-center border-b px-3">
            <SheetTitle className="min-w-0 flex-1 truncate font-heading text-title">
              {PROXY_DOCK_LABEL}
            </SheetTitle>
          </header>
          {open && (
            <div className="flex min-h-0 flex-1 flex-col">
              <DockBody />
            </div>
          )}
        </SheetContent>
      </Sheet>
    </>
  );
}

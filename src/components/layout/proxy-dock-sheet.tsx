/**
 * The proxy dock on a tablet (Epic 98, story 98.3 #5, UX-DR146): the person's
 * proxy room — the DM with it by default, or one of its conversations — as a
 * sheet over the phone's surfaces, opened from the Inbox header.
 *
 * The same body as the desktop's Notes dock (`DockBody`), under stores of its
 * own, so opening it never changes which room the chat has selected. Mounted
 * only when the shell is a tablet (`useShellLayout().tablet`); a phone keeps
 * its proxy DM as one more room in the Inbox. Over the Inbox no note is in
 * front of the person, so the sheet tells the proxy none.
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
              <DockBody tellsNote={false} />
            </div>
          )}
        </SheetContent>
      </Sheet>
    </>
  );
}

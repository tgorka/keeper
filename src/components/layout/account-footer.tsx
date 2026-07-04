/**
 * Sidebar-footer account row with local sign-out (AD-10, Story 1.8).
 *
 * Shows the signed-in account's Matrix user id (truncated gracefully) and a
 * Sign out control. In the collapsed icon rail it renders an icon-only sign-out
 * affordance. Either control opens a shadcn {@link Dialog} confirming the intent;
 * the confirm button awaits {@link useSignOut} (which deletes the local session
 * and resets the stores → login screen) while cancel is a pure no-op. Baseline
 * a11y: accessible labels and visible focus rings on every control.
 *
 * Renders nothing when there is no signed-in account.
 */
import { LogOut } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useSignOut } from "@/hooks/use-sign-out";
import { useAccountsStore } from "@/lib/stores/accounts";
import { cn } from "@/lib/utils";

interface AccountFooterProps {
  collapsed: boolean;
}

const FOCUS_RING = "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

export function AccountFooter({ collapsed }: AccountFooterProps) {
  const userId = useAccountsStore((s) => s.currentAccount?.userId ?? null);
  const signOut = useSignOut();
  const [open, setOpen] = useState(false);
  const [signingOut, setSigningOut] = useState(false);

  if (userId === null) {
    return null;
  }

  async function handleConfirm() {
    setSigningOut(true);
    try {
      await signOut();
      // On success the shell unmounts (account cleared); no need to close.
    } catch {
      // A cleanup failure keeps the user signed in; reopen the row for a retry.
      setSigningOut(false);
      setOpen(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <div
        className={cn(
          "flex shrink-0 border-border border-t p-2",
          collapsed ? "justify-center" : "items-center gap-2",
        )}
      >
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={`Sign out ${userId}`}
                className={FOCUS_RING}
                onClick={() => setOpen(true)}
              >
                <LogOut aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">Sign out</TooltipContent>
          </Tooltip>
        ) : (
          <>
            <span className="min-w-0 flex-1 truncate text-muted-foreground text-sm" title={userId}>
              {userId}
            </span>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={`Sign out ${userId}`}
              className={cn("shrink-0", FOCUS_RING)}
              onClick={() => setOpen(true)}
            >
              <LogOut aria-hidden="true" />
            </Button>
          </>
        )}
      </div>

      <DialogContent>
        <DialogHeader>
          <DialogTitle>Sign out?</DialogTitle>
          <DialogDescription>
            You'll be signed out of {userId} on this device and returned to the login screen.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose asChild>
            <Button type="button" variant="outline" className={FOCUS_RING}>
              Cancel
            </Button>
          </DialogClose>
          <Button
            type="button"
            variant="destructive"
            className={FOCUS_RING}
            disabled={signingOut}
            onClick={handleConfirm}
          >
            {signingOut ? "Signing out…" : "Sign out"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

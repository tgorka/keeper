/**
 * Sidebar-footer account list with per-account local sign-out + Add Account
 * (AD-10, Story 2.1 — minimal).
 *
 * Lists every signed-in account (its Matrix user id, truncated) each with a
 * Sign out control, and an always-present "Add Account" button that opens the
 * login overlay in add mode. In the collapsed icon rail each account shows an
 * icon-only sign-out affordance and the add button is a `+` icon. A sign-out
 * confirmation {@link Dialog} awaits {@link useSignOut} bound to that account's
 * id (which deletes only that account's local session and drops its rows from
 * the merged inbox); other accounts keep syncing. Cancel is a pure no-op.
 *
 * Intentionally throwaway: Story 2.5 replaces this with the designed switcher
 * (avatars, hue dots, homeserver line, sync glyph, dropdown, filter).
 *
 * Renders only the Add Account button when there are no accounts (it is never
 * count-gated), and nothing else.
 */
import { LogOut, Plus } from "lucide-react";
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
import type { AccountVm } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";
import { cn } from "@/lib/utils";

interface AccountFooterProps {
  collapsed: boolean;
}

const FOCUS_RING = "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

/** One account row (expanded or collapsed) with its own sign-out dialog. */
function AccountRow({ account, collapsed }: { account: AccountVm; collapsed: boolean }) {
  const signOut = useSignOut();
  const [open, setOpen] = useState(false);
  const [signingOut, setSigningOut] = useState(false);
  const userId = account.userId;

  async function handleConfirm() {
    setSigningOut(true);
    try {
      await signOut(account.accountId);
      // On success this row unmounts (account removed); no need to close.
    } catch {
      // A cleanup failure keeps the account signed in; close for a retry.
      setSigningOut(false);
      setOpen(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <div className={cn("flex shrink-0", collapsed ? "justify-center" : "items-center gap-2")}>
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
            <TooltipContent side="right">{`Sign out ${userId}`}</TooltipContent>
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
            You'll be signed out of {userId} on this device. Your other accounts keep syncing.
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

export function AccountFooter({ collapsed }: AccountFooterProps) {
  const accounts = useAccountsStore((s) => s.accounts);
  const openAddAccount = useAddAccountStore((s) => s.openAddAccount);

  return (
    <div
      className={cn(
        "flex shrink-0 flex-col gap-1 border-border border-t p-2",
        collapsed && "items-center",
      )}
    >
      {accounts.map((account) => (
        <AccountRow key={account.accountId} account={account} collapsed={collapsed} />
      ))}

      {collapsed ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Add account"
              className={FOCUS_RING}
              onClick={openAddAccount}
            >
              <Plus aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="right">Add account</TooltipContent>
        </Tooltip>
      ) : (
        <Button
          type="button"
          variant="ghost"
          aria-label="Add account"
          className={cn("w-full justify-start gap-2", FOCUS_RING)}
          onClick={openAddAccount}
        >
          <Plus aria-hidden="true" />
          Add account
        </Button>
      )}
    </div>
  );
}

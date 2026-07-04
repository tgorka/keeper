/**
 * Sidebar-footer account switcher (Story 2.5, FR-4/FR-6, UX-DR18/UX-DR20).
 *
 * Lists every signed-in account as a switcher row: a hue-tinted initials
 * {@link Avatar}, a hue dot, the homeserver, and a 3-state sync glyph driven by
 * the per-account connection status (pending spinner / synced / offline gray).
 * Clicking a row filters the merged inbox to that account (click the active one
 * to clear). Each row carries a {@link DropdownMenu} with Settings (opens the
 * global {@link SettingsDialog}), Beeper coverage (Beeper accounts only, opens
 * {@link BeeperCoverageDisclosure} in a Dialog), and "Sign out…" opening an
 * {@link AlertDialog} defaulting to keep-local-archive sign-out via
 * {@link useSignOut}. An always-present, never-count-gated "Add Account" entry
 * sits below the rows. Collapsed, each row is an avatar-only button and the menu
 * / add controls become icon buttons.
 *
 * Renders only the Add Account button when there are no accounts.
 */
import { Check, CloudOff, Loader2, LogOut, MoreVertical, Plus, Settings } from "lucide-react";
import { useState } from "react";
import { BeeperCoverageDisclosure } from "@/components/auth/beeper-coverage-disclosure";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useSignOut } from "@/hooks/use-sign-out";
import { accountHueVar } from "@/lib/account-hue";
import { isBeeperAccount } from "@/lib/beeper";
import type { AccountVm, ConnectionStatus } from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";
import { cn } from "@/lib/utils";

interface AccountFooterProps {
  collapsed: boolean;
}

const FOCUS_RING = "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

/** The first character of the user id (without the leading `@`), uppercased, as
 * the avatar initials fallback. Empty ids fall back to `?`. */
function initials(userId: string): string {
  const stripped = userId.startsWith("@") ? userId.slice(1) : userId;
  const first = stripped.trim().charAt(0);
  return first ? first.toUpperCase() : "?";
}

/** The homeserver host for a resolved homeserver URL, or the raw string when it
 * cannot be parsed as a URL. */
function homeserverLabel(homeserverUrl: string): string {
  try {
    return new URL(homeserverUrl).host;
  } catch {
    return homeserverUrl;
  }
}

/**
 * The 3-state sync glyph, a passive projection of the account's connection
 * status: no batch yet (`undefined`) → a syncing spinner; `online` → a synced
 * check; `offline` → a gray offline cloud. Never a toast.
 */
function SyncGlyph({ status }: { status: ConnectionStatus | undefined }) {
  if (status === undefined) {
    return (
      <Loader2
        aria-label="Syncing"
        className="size-3.5 shrink-0 animate-spin text-muted-foreground"
      />
    );
  }
  if (status === "offline") {
    return <CloudOff aria-label="Offline" className="size-3.5 shrink-0 text-muted-foreground" />;
  }
  return <Check aria-label="Synced" className="size-3.5 shrink-0 text-muted-foreground" />;
}

/** The hue-tinted initials avatar for an account. */
function AccountAvatar({ account }: { account: AccountVm }) {
  return (
    <Avatar size="sm">
      <AvatarFallback
        style={{ backgroundColor: accountHueVar(account.hueIndex) }}
        className="font-medium text-white"
      >
        {initials(account.userId)}
      </AvatarFallback>
    </Avatar>
  );
}

/**
 * The per-Beeper-account coverage disclosure, opened from the row menu. Its own
 * Dialog is controlled here so it survives the DropdownMenu closing.
 */
function BeeperCoverageDialog({
  userId,
  open,
  onOpenChange,
}: {
  userId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-label={`Beeper coverage for ${userId}`}>
        <DialogHeader>
          <DialogTitle>Beeper coverage</DialogTitle>
          <DialogDescription>What keeper can and cannot sync for this Account.</DialogDescription>
        </DialogHeader>
        <BeeperCoverageDisclosure />
        <DialogFooter>
          <DialogClose asChild>
            <Button type="button" variant="outline" className={FOCUS_RING}>
              Close
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The keep-local-archive sign-out confirmation for one account (UX-DR20). */
function SignOutDialog({
  account,
  open,
  onOpenChange,
}: {
  account: AccountVm;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const signOut = useSignOut();
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
      onOpenChange(false);
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Sign out, keep local archive</AlertDialogTitle>
          <AlertDialogDescription>
            You'll be signed out of {userId} on this device. Your local archive stays on this Mac
            and your other accounts keep syncing.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className={FOCUS_RING}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            className={FOCUS_RING}
            disabled={signingOut}
            onClick={(event) => {
              // Keep the dialog mounted while the async sign-out runs.
              event.preventDefault();
              void handleConfirm();
            }}
          >
            {signingOut ? "Signing out…" : "Sign out, keep local archive"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** The per-row menu (Settings / Beeper coverage / Sign out…) plus the dialogs it
 * opens. Rendered in both collapsed and expanded rows. */
function AccountRowMenu({ account, collapsed }: { account: AccountVm; collapsed: boolean }) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [coverageOpen, setCoverageOpen] = useState(false);
  const [signOutOpen, setSignOutOpen] = useState(false);
  const userId = account.userId;
  const isBeeper = isBeeperAccount(account);
  const menuLabel = `Account menu for ${userId}`;

  return (
    <>
      <DropdownMenu>
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={menuLabel}
                  className={cn("shrink-0", FOCUS_RING)}
                >
                  <MoreVertical aria-hidden="true" />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent side="right">{menuLabel}</TooltipContent>
          </Tooltip>
        ) : (
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={menuLabel}
              className={cn("shrink-0", FOCUS_RING)}
            >
              <MoreVertical aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
        )}
        <DropdownMenuContent align="end" side="right">
          <DropdownMenuItem onSelect={() => setSettingsOpen(true)}>
            <Settings aria-hidden="true" />
            Settings
          </DropdownMenuItem>
          {isBeeper && (
            <DropdownMenuItem onSelect={() => setCoverageOpen(true)}>
              Beeper coverage
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={() => setSignOutOpen(true)}>
            <LogOut aria-hidden="true" />
            Sign out…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
      {isBeeper && (
        <BeeperCoverageDialog userId={userId} open={coverageOpen} onOpenChange={setCoverageOpen} />
      )}
      <SignOutDialog account={account} open={signOutOpen} onOpenChange={setSignOutOpen} />
    </>
  );
}

/** One account switcher row (expanded or collapsed). */
function AccountRow({ account, collapsed }: { account: AccountVm; collapsed: boolean }) {
  const status = useAccountStatus(account.accountId);
  const filterAccountId = useAccountsStore((s) => s.filterAccountId);
  const toggleFilter = useAccountsStore((s) => s.toggleFilter);
  const active = filterAccountId === account.accountId;
  const userId = account.userId;
  const homeserver = homeserverLabel(account.homeserverUrl);

  if (collapsed) {
    const filterLabel = active ? `Clear filter for ${userId}` : `Filter inbox to ${userId}`;
    return (
      <div className="flex shrink-0 flex-col items-center gap-1">
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label={filterLabel}
              aria-pressed={active}
              onClick={() => toggleFilter(account.accountId)}
              className={cn(
                "relative flex items-center justify-center rounded-md p-1",
                FOCUS_RING,
                active && "bg-accent",
              )}
            >
              <AccountAvatar account={account} />
              <span className="absolute right-0 bottom-0">
                <SyncGlyph status={status} />
              </span>
            </button>
          </TooltipTrigger>
          <TooltipContent side="right">{`${userId} — ${homeserver}`}</TooltipContent>
        </Tooltip>
        <AccountRowMenu account={account} collapsed={collapsed} />
      </div>
    );
  }

  const filterLabel = active ? `Clear filter for ${userId}` : `Filter inbox to ${userId}`;
  return (
    <div className={cn("flex shrink-0 items-center gap-2 rounded-md pr-1", active && "bg-accent")}>
      <button
        type="button"
        aria-label={filterLabel}
        aria-pressed={active}
        onClick={() => toggleFilter(account.accountId)}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-2 rounded-md p-1.5 text-left",
          FOCUS_RING,
        )}
      >
        <AccountAvatar account={account} />
        <span
          aria-hidden="true"
          className="size-2 shrink-0 rounded-full"
          style={{ backgroundColor: accountHueVar(account.hueIndex) }}
        />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-sm" title={userId}>
            {userId}
          </span>
          <span className="truncate text-muted-foreground text-xs" title={homeserver}>
            {homeserver}
          </span>
        </span>
        <SyncGlyph status={status} />
      </button>
      <AccountRowMenu account={account} collapsed={collapsed} />
    </div>
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

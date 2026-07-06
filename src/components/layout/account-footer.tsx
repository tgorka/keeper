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
import {
  Check,
  CloudOff,
  Loader2,
  LogOut,
  MoreVertical,
  Plus,
  Settings,
  VenetianMask,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
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
import { Badge } from "@/components/ui/badge";
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
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useSignOut } from "@/hooks/use-sign-out";
import { accountHueVar } from "@/lib/account-hue";
import { initials } from "@/lib/account-initials";
import { isBeeperAccount } from "@/lib/beeper";
import {
  type AccountVm,
  type ConnectionStatus,
  incognitoGetAccount,
  incognitoSetAccount,
} from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";
import { useShowVerifyBadgeForAccount } from "@/lib/stores/encryption-status";
import { incognitoStore } from "@/lib/stores/incognito";
import { settingsUiStore, useSettingsOpen } from "@/lib/stores/settings-ui";
import { cn } from "@/lib/utils";

interface AccountFooterProps {
  collapsed: boolean;
}

const FOCUS_RING = "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

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

/**
 * The sign-out confirmation for one account (UX-DR20, Story 5.7). Defaults to the
 * keep-local-archive path; a reversible destructive option arms the
 * "…and delete this Account's archive" path, gated behind typing the account
 * identity exactly. When armed, the title/description switch to a destructive
 * framing (never the keep-archive copy) and the arming control is a
 * secondary/non-destructive button; only the actual confirm is destructive.
 */
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
  // Whether the destructive delete-archive path is armed (reveals the identity
  // field and destructive framing). Reversible without closing the dialog.
  const [armed, setArmed] = useState(false);
  // The typed identity used to gate the destructive confirm (trimmed-equals).
  const [typedIdentity, setTypedIdentity] = useState("");
  // A dialog-local error for a sign-out FAILURE only (which keeps the account, so
  // the dialog stays mounted). Archive-purge failures are always surfaced via the
  // hook's toast — the account row unmounts before the purge resolves, so a
  // dialog-local error would never be seen.
  const [error, setError] = useState<string | null>(null);
  const userId = account.userId;
  // Guard against a degenerate empty `userId`: an empty confirm field must never
  // enable the destructive action.
  const identityMatches = userId.length > 0 && typedIdentity.trim() === userId;

  // Reset all destructive-path state whenever the dialog closes, so reopening it
  // always starts from the keep-archive default.
  function handleOpenChange(next: boolean) {
    if (!next) {
      setArmed(false);
      setTypedIdentity("");
      setError(null);
      setSigningOut(false);
    }
    onOpenChange(next);
  }

  async function handleKeepArchiveConfirm() {
    setSigningOut(true);
    setError(null);
    try {
      await signOut(account.accountId);
      // On success this row unmounts (account removed); no need to close.
    } catch {
      // A cleanup failure keeps the account signed in; close for a retry.
      setSigningOut(false);
      handleOpenChange(false);
    }
  }

  async function handleDeleteArchiveConfirm() {
    setSigningOut(true);
    setError(null);
    try {
      await signOut(account.accountId, { deleteArchive: true });
      // On success this row unmounts (account removed). A purge failure is NOT
      // thrown here (the hook removes the account first, then surfaces a purge
      // rejection via toast), so reaching a rejection means the sign-out itself
      // failed — the account stays; show a dialog-local retry error.
    } catch {
      setSigningOut(false);
      setError("Could not sign out. Please try again.");
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {armed ? "Delete this Account's archive" : "Sign out, keep local archive"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {armed ? (
              <>
                This permanently deletes {userId}'s entire local archive from this Mac — its
                messages and search history cannot be recovered. Your other accounts are unaffected.
                Type <span className="font-medium text-foreground">{userId}</span> to confirm.
              </>
            ) : (
              <>
                You'll be signed out of {userId} on this device. Your local archive stays on this
                Mac and your other accounts keep syncing. Content that was never synced and
                decrypted before you sign out is not recoverable.
              </>
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>

        {armed && (
          <div className="flex flex-col gap-2">
            <Input
              aria-label={`Type ${userId} to confirm deletion`}
              autoComplete="off"
              value={typedIdentity}
              onChange={(event) => setTypedIdentity(event.target.value)}
              className={FOCUS_RING}
            />
            {error && (
              <p role="alert" className="text-destructive text-sm">
                {error}
              </p>
            )}
          </div>
        )}

        <AlertDialogFooter className="sm:flex-col sm:items-stretch sm:gap-2">
          {armed ? (
            <>
              <AlertDialogAction
                variant="destructive"
                className={FOCUS_RING}
                disabled={signingOut || !identityMatches}
                onClick={(event) => {
                  event.preventDefault();
                  void handleDeleteArchiveConfirm();
                }}
              >
                {signingOut ? "Deleting…" : "Sign out and delete archive"}
              </AlertDialogAction>
              <div className="flex justify-between gap-2">
                <Button
                  type="button"
                  variant="outline"
                  className={FOCUS_RING}
                  disabled={signingOut}
                  onClick={() => {
                    // Reversible: return to the keep-archive choice in place.
                    setArmed(false);
                    setTypedIdentity("");
                    setError(null);
                  }}
                >
                  Keep archive instead
                </Button>
                <AlertDialogCancel className={FOCUS_RING}>Cancel</AlertDialogCancel>
              </div>
            </>
          ) : (
            <>
              <AlertDialogAction
                variant="destructive"
                className={FOCUS_RING}
                disabled={signingOut}
                onClick={(event) => {
                  // Keep the dialog mounted while the async sign-out runs.
                  event.preventDefault();
                  void handleKeepArchiveConfirm();
                }}
              >
                {signingOut ? "Signing out…" : "Sign out, keep local archive"}
              </AlertDialogAction>
              <div className="flex justify-between gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  className={FOCUS_RING}
                  disabled={signingOut}
                  onClick={() => setArmed(true)}
                >
                  …and delete this Account's archive
                </Button>
                <AlertDialogCancel className={FOCUS_RING}>Cancel</AlertDialogCancel>
              </div>
            </>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** The per-row menu (Settings / Beeper coverage / Sign out…) plus the dialogs it
 * opens. Rendered in both collapsed and expanded rows. */
/**
 * Per-Account Incognito tri-state submenu (Story 8.1). Reads the account's override
 * via `incognitoGetAccount` on menu open and writes the chosen scope via
 * `incognitoSetAccount`. Tri-state: "Inherit global" (`null`), "On" (`true`), "Off"
 * (`false`). The radio group's value encodes the tri-state as `"inherit" | "on" |
 * "off"`. Precedence still resolves in Rust — this only sets the account scope.
 */
function AccountIncognitoSubmenu({ accountId }: { accountId: string }) {
  // `undefined` = still loading; otherwise the tri-state override.
  const [value, setValue] = useState<boolean | null | undefined>(undefined);
  // Monotonic write id: only the newest write may revert on failure, so a slow
  // failed write can't clobber a newer successful selection (mirrors PrivacySection).
  const writeId = useRef(0);

  useEffect(() => {
    let cancelled = false;
    void incognitoGetAccount(accountId)
      .then((v) => {
        if (!cancelled) {
          setValue(v);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setValue(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [accountId]);

  const radio = value === undefined ? "inherit" : value === null ? "inherit" : value ? "on" : "off";

  const onSelect = (next: boolean | null) => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = value ?? null;
    setValue(next);
    void incognitoSetAccount(accountId, next)
      .then(() => {
        // Nudge any open chat for this account to re-read its effective state so the
        // header chip and composer ring reconcile without a room reopen (Story 8.1).
        incognitoStore.getState().bumpPolicyVersion();
      })
      .catch(() => {
        // Revert on a persist failure — but only if no newer write superseded this
        // one, so a stale failed write never clobbers a newer successful selection.
        if (id === writeId.current) {
          setValue(prev);
        }
      });
  };

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <VenetianMask aria-hidden="true" />
        Incognito
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        <DropdownMenuRadioGroup value={radio}>
          <DropdownMenuRadioItem value="inherit" onSelect={() => onSelect(null)}>
            Inherit global
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="on" onSelect={() => onSelect(true)}>
            On
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="off" onSelect={() => onSelect(false)}>
            Off
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}

function AccountRowMenu({ account, collapsed }: { account: AccountVm; collapsed: boolean }) {
  // The Settings dialog open-state is shared (Story 3.1) so the verify banner and
  // the UTD stub can open it too; the per-row menu drives the same store. The
  // single dialog instance is mounted once in {@link AccountFooter}.
  const setSettingsOpen = settingsUiStore.getState().setSettingsOpen;
  const [coverageOpen, setCoverageOpen] = useState(false);
  const [signOutOpen, setSignOutOpen] = useState(false);
  // The persistent verify badge: shown on THIS account's row once the banner is
  // dismissed while THIS device is still unverified (it collapses to a Settings
  // badge, not gone). Account-scoped so a verified account's row stays clean.
  const showVerifyBadge = useShowVerifyBadgeForAccount(account.accountId);
  const userId = account.userId;
  const isBeeper = isBeeperAccount(account);
  const menuLabel = `Account menu for ${userId}`;

  const trigger = (
    <DropdownMenuTrigger asChild>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        aria-label={menuLabel}
        className={cn("relative shrink-0", FOCUS_RING)}
      >
        <MoreVertical aria-hidden="true" />
        {showVerifyBadge && (
          <Badge
            aria-hidden="true"
            className="-top-0.5 -right-0.5 absolute size-2 rounded-full p-0"
          />
        )}
      </Button>
    </DropdownMenuTrigger>
  );

  return (
    <>
      <DropdownMenu>
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>{trigger}</TooltipTrigger>
            <TooltipContent side="right">{menuLabel}</TooltipContent>
          </Tooltip>
        ) : (
          trigger
        )}
        <DropdownMenuContent align="end" side="right">
          <DropdownMenuItem onSelect={() => setSettingsOpen(true)}>
            <Settings aria-hidden="true" />
            Settings
            {showVerifyBadge && (
              <Badge aria-hidden="true" className="ml-auto size-2 rounded-full p-0" />
            )}
          </DropdownMenuItem>
          {isBeeper && (
            <DropdownMenuItem onSelect={() => setCoverageOpen(true)}>
              Beeper coverage
            </DropdownMenuItem>
          )}
          <AccountIncognitoSubmenu accountId={account.accountId} />
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={() => setSignOutOpen(true)}>
            <LogOut aria-hidden="true" />
            Sign out…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

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
  // A single shared Settings dialog for the whole footer, driven by the shared
  // open-state store (Story 3.1) so the verify banner / UTD stub open the same
  // one — never one per account row.
  const settingsOpen = useSettingsOpen();
  const setSettingsOpen = settingsUiStore.getState().setSettingsOpen;

  return (
    <div
      className={cn(
        "flex shrink-0 flex-col gap-1 border-border border-t p-2",
        collapsed && "items-center",
      )}
    >
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
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

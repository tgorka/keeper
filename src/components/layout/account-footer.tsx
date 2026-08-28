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
  CloudOff,
  Loader2,
  LogOut,
  MoonStar,
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
  DropdownMenuCheckboxItem,
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
import { Lamp } from "@/components/ui/lamp";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useSignOut } from "@/hooks/use-sign-out";
import { accountHueVar } from "@/lib/account-hue";
import { initials } from "@/lib/account-initials";
import { isBeeperAccount } from "@/lib/beeper";
import {
  type AccountVm,
  type ConnectionStatus,
  dndGetGlobal,
  dndSetGlobal,
  incognitoGetAccount,
  incognitoSetAccount,
} from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";
import { useShowVerifyBadgeForAccount } from "@/lib/stores/encryption-status";
import { incognitoStore } from "@/lib/stores/incognito";
import { primaryViewStore } from "@/lib/stores/primary-view";
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
 * The sync glyph, a passive projection of the account's connection status: no
 * batch yet (`undefined`) → a syncing spinner; `offline` → a gray offline
 * cloud; `online` → nothing at all. Never a toast.
 *
 * `online` used to draw a check. It was on screen essentially always, which is
 * what made it useless: a mark that is present whenever nothing is wrong tells
 * a reader nothing when they look at it, and it cost a row that is now 130px
 * wide the space to say which account it is. The two states worth a glyph are
 * the two that are not fine, and those still have one.
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
  return null;
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
            <Button type="button" variant="outline">
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
                <AlertDialogCancel>Cancel</AlertDialogCancel>
              </div>
            </>
          ) : (
            <>
              <AlertDialogAction
                variant="destructive"
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
                  disabled={signingOut}
                  onClick={() => setArmed(true)}
                >
                  …and delete this Account's archive
                </Button>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
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

/**
 * Global Do-Not-Disturb toggle row (Story 10.2, FR-52). A single app-wide on/off
 * switch: when on, no notification posts on any account/Chat while unread still accrues
 * everywhere. Reads the global state via `dndGetGlobal` on mount and writes it via
 * `dndSetGlobal`; a monotonic `writeId` reverts only the newest failed write so a slow
 * failure never clobbers a newer successful toggle (mirrors the Incognito pattern).
 * `onSelect` returns `false` so picking it does not close the menu — the trailing check
 * updates in place, matching a durable toggle.
 */
function GlobalDndToggle() {
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);
  const writeId = useRef(0);

  useEffect(() => {
    let cancelled = false;
    void dndGetGlobal()
      .then((v) => {
        if (!cancelled) {
          setEnabled(v);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnabled(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onToggle = () => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = enabled ?? false;
    const next = !prev;
    setEnabled(next);
    void dndSetGlobal(next).catch(() => {
      if (id === writeId.current) {
        setEnabled(prev);
      }
    });
  };

  return (
    // A `menuitemcheckbox`, not a `menuitem` with a tick drawn on it (Story 49).
    // The tick was the whole of the state and a tick is a picture: to anyone not
    // looking at this menu, an app-wide switch that silences every notification
    // on every account reported nothing at all about which way it was set.
    // `aria-checked` is the same fact in the vocabulary a menu has for it, and
    // the indicator now comes from the primitive rather than from a `ml-auto`
    // glyph beside the words.
    <DropdownMenuCheckboxItem
      checked={enabled === true}
      onSelect={(e) => {
        // Keep the menu open so the trailing check reflects the new state in place.
        e.preventDefault();
      }}
      onCheckedChange={onToggle}
    >
      <MoonStar aria-hidden="true" />
      Do not disturb
    </DropdownMenuCheckboxItem>
  );
}

/** What an unverified device's lamp says, on the row trigger and in the menu. */
const VERIFY_NEEDED_WORD = "Verification needed";

/**
 * The row-menu trigger's reveal (Story 49).
 *
 * The owner's note was that the `⋮` beside every avatar is clutter and that the
 * avatar should open the menu instead. Half of that is right and half of it
 * would have cost a shipped function: **the avatar is already a control**. It
 * carries `aria-pressed` and it is the inbox account filter — one click to
 * filter to an account, one to clear — so a menu on the avatar's click is a
 * filter nobody can reach. What is actually wrong is that two permanently drawn
 * controls per account is two, worst on the folded rail where they stack and
 * each account costs 68px of a 48px-wide strip.
 *
 * So the `⋮` is still here, still a real button, still in the tab order and
 * still named — it is only quiet. It appears when the pointer is over its row,
 * when anything in that row has focus (which includes itself, so tabbing to it
 * shows it before it is reached), and while its own menu is open. `opacity-0`
 * alone would leave an invisible button eating clicks aimed at the avatar
 * underneath it on the folded rail, so it gives up pointer events with its
 * paint and takes them back with it; keyboard activation never depended on
 * either.
 *
 * **The cost, stated rather than buried:** a control revealed on hover is a
 * control a mouse user has to discover, and the first time is by accident. The
 * trade is one row of clutter per account against one discovery, and the menu
 * holds nothing that is only there (Settings, Incognito, DND and Sign out are
 * all reachable elsewhere or destructive-by-appointment). The one thing that
 * must never hide is the unverified-device lamp, which is why a row wearing it
 * opts out below. The other is a pointer that cannot hover: Tailwind compiles
 * `group-hover` inside `@media (hover: hover)`, so on a coarse pointer the
 * reveal would never fire and the control would be permanently invisible AND
 * unclickable. `pointer-coarse` puts it back on screen there, drawn as it
 * always was.
 */
const ROW_MENU_QUIET =
  "pointer-events-none opacity-0 transition-opacity group-hover/account-row:pointer-events-auto group-hover/account-row:opacity-100 group-focus-within/account-row:pointer-events-auto group-focus-within/account-row:opacity-100 aria-expanded:pointer-events-auto aria-expanded:opacity-100 pointer-coarse:pointer-events-auto pointer-coarse:opacity-100";

function AccountRowMenu({
  account,
  collapsed,
  children,
}: {
  account: AccountVm;
  collapsed: boolean;
  /** The row's own contents, when the ROW is the trigger.
   *
   * Given in both shapes since the folded rail's avatar became the trigger
   * too; the small corner dot is what remains for a caller that hands nothing
   * down. */
  children?: React.ReactNode;
}) {
  // The Settings dialog open-state is shared (Story 3.1) so the verify banner and
  // the UTD stub can open it too; the per-row menu drives the same store. The
  // single dialog instance is mounted once in {@link AccountFooter}.
  const [coverageOpen, setCoverageOpen] = useState(false);
  const [signOutOpen, setSignOutOpen] = useState(false);
  // The persistent verify badge: shown on THIS account's row once the banner is
  // dismissed while THIS device is still unverified (it collapses to a Settings
  // badge, not gone). Account-scoped so a verified account's row stays clean.
  const showVerifyBadge = useShowVerifyBadgeForAccount(account.accountId);
  const userId = account.userId;
  const isBeeper = isBeeperAccount(account);
  // The unverified-device state has to ride the trigger's own name: the button
  // carries an explicit `aria-label`, so anything inside it is overridden. It
  // used to be an `aria-hidden` accent dot and nothing else — a security state
  // announced to nobody, in the one colour the app uses for "this is fine".
  const menuLabel = showVerifyBadge
    ? `Account menu for ${userId}, ${VERIFY_NEEDED_WORD}`
    : `Account menu for ${userId}`;

  const filterAccountId = useAccountsStore((state) => state.filterAccountId);
  const toggleFilter = useAccountsStore((state) => state.toggleFilter);
  const filtered = filterAccountId === account.accountId;
  const filterLabel = filtered ? `Clear filter for ${userId}` : `Filter inbox to ${userId}`;

  // The ROW is the trigger when it hands us its contents.
  //
  // It used to be a three-dot button in a reserved gutter beside a row that did
  // something else entirely — toggled the inbox filter. Two controls, one of
  // them 24px wide, for one account. The row now opens everything the account
  // can do, and the filter is the first of those things rather than a hidden
  // second meaning of clicking the name.
  const rowTrigger = children !== undefined && (
    <DropdownMenuTrigger asChild>
      <button
        type="button"
        aria-label={menuLabel}
        className={cn(
          // The row is the control now (Story 49), and it had nothing to say
          // so on hover: no tint, no cursor change, nothing but a pointer
          // passing over text. `hover:bg-accent` is what every other clickable
          // row in the drawer does, so this one stops being the exception.
          "flex min-w-0 flex-1 items-center gap-2 rounded-md p-1.5 text-left hover:bg-accent",
          FOCUS_RING,
        )}
      >
        {children}
      </button>
    </DropdownMenuTrigger>
  );

  const trigger = (
    <DropdownMenuTrigger asChild>
      <Button
        type="button"
        variant="ghost"
        // On the folded rail it is an overlay on the avatar's free corner
        // rather than a second storey under it: two stacked 32px tiles per
        // account is what made the folded footer as tall as it is, and the
        // sync glyph already owns the other corner. `icon-xs` is 24px, which
        // is the smallest a pointer target is allowed to be (WCAG 2.5.8), and
        // it leaves the avatar's top and right free for the filter press.
        size={collapsed ? "icon-xs" : "icon-sm"}
        aria-label={menuLabel}
        className={cn(
          "relative shrink-0",
          collapsed && "-bottom-1 -left-1 absolute",
          // A row whose device is unverified keeps its menu drawn: the lamp on
          // this trigger IS the persistent verify signal once the banner is
          // dismissed, and a security state that appears on hover is a security
          // state nobody sees.
          !showVerifyBadge && ROW_MENU_QUIET,
        )}
      >
        <MoreVertical aria-hidden="true" />
        {showVerifyBadge && (
          <Lamp
            state="working"
            label={null}
            data-slot="verify-badge"
            className="-top-0.5 -right-0.5 absolute"
          />
        )}
      </Button>
    </DropdownMenuTrigger>
  );

  return (
    <>
      <DropdownMenu>
        {collapsed ? (
          // Folded, the tooltip is the only thing that says whose account this
          // is, so it wraps whichever trigger there is — the avatar when the row
          // handed one down, the old dot when it did not.
          <Tooltip>
            <TooltipTrigger asChild>{rowTrigger ?? trigger}</TooltipTrigger>
            <TooltipContent side="right">{menuLabel}</TooltipContent>
          </Tooltip>
        ) : (
          // The row when it gave us one, and the old small trigger only when it
          // did not — so an expanded footer has exactly one control per account.
          (rowTrigger ?? trigger)
        )}
        <DropdownMenuContent align="end" side="right">
          {/* First, because it is the thing the row itself used to do and the
              one action here that is about the inbox rather than the account.
              Its checked state is what the row's `aria-pressed` used to carry. */}
          <DropdownMenuCheckboxItem
            checked={filtered}
            onCheckedChange={() => toggleFilter(account.accountId)}
          >
            {filterLabel}
          </DropdownMenuCheckboxItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            // Named outright rather than left to the contents: a menu item's
            // name is concatenated from its children with each text node
            // trimmed first, so "Settings" and the lamp's word would arrive as
            // "SettingsVerification needed" — one token, announced once, read
            // by nobody as two facts.
            aria-label={showVerifyBadge ? `Settings, ${VERIFY_NEEDED_WORD}` : undefined}
            onSelect={() => primaryViewStore.getState().setView("settings")}
          >
            <Settings aria-hidden="true" />
            Settings
            {showVerifyBadge && (
              <Lamp state="working" label={null} data-slot="verify-badge" className="ml-auto" />
            )}
          </DropdownMenuItem>
          {isBeeper && (
            <DropdownMenuItem onSelect={() => setCoverageOpen(true)}>
              Beeper coverage
            </DropdownMenuItem>
          )}
          <AccountIncognitoSubmenu accountId={account.accountId} />
          <GlobalDndToggle />
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
  const active = filterAccountId === account.accountId;
  const userId = account.userId;
  const homeserver = homeserverLabel(account.homeserverUrl);

  if (collapsed) {
    // The avatar IS the control, exactly as the expanded row became one in
    // Story 49. It used to toggle the inbox filter while a separate dot in the
    // corner opened the menu — so the folded rail and the unfolded one answered
    // the same press differently, and the menu was behind a target a few pixels
    // wide. The filter is the menu's first item in both shapes now.
    return (
      <div className="group/account-row relative flex shrink-0 justify-center">
        <AccountRowMenu account={account} collapsed={collapsed}>
          <span
            className={cn(
              "relative flex items-center justify-center rounded-md p-1",
              active && "bg-accent",
            )}
          >
            <AccountAvatar account={account} />
            <span className="absolute right-0 bottom-0">
              <SyncGlyph status={status} />
            </span>
          </span>
        </AccountRowMenu>
      </div>
    );
  }

  return (
    <div
      className={cn(
        // No `pr-1`: it reserved the gutter the three-dot trigger sat in, and
        // with the row itself opening the menu that gutter is width the
        // account's own name can have back.
        "group/account-row flex shrink-0 items-center gap-2 rounded-md",
        active && "bg-accent",
      )}
    >
      <AccountRowMenu account={account} collapsed={collapsed}>
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
      </AccountRowMenu>
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
          className="w-full justify-start gap-2"
          onClick={openAddAccount}
        >
          <Plus aria-hidden="true" />
          Add account
        </Button>
      )}
    </div>
  );
}

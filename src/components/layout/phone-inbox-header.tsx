/**
 * Phone Inbox (level 0) header with status cluster (Story 13.3, FR-58, UX-DR23).
 *
 * The single 52px bar the `PhoneShell` renders above the `ChatListPane` at the
 * Inbox level. It is the phone's reachable home for everything the desktop
 * sidebar owns — surfaced through a leading avatar drawer trigger plus a
 * trailing action cluster — while staying **quiet when healthy**:
 *
 * - Leading: an avatar `button` (the drawer trigger) that opens the leading
 *   `Sheet` (the reused `SidebarPane`). It renders the active account-filter
 *   account's hue-initials avatar when a filter is set, else a neutral
 *   all-accounts avatar, with a worst-state bridge-health dot `AvatarBadge`
 *   overlay shown only for `degraded`/`disconnected` (hidden on `healthy`/`null`).
 * - Trailing: an amber Approval chip shown only when the pending-Draft count is
 *   > 0 (deep-links to the Approval Pane), a magnifier (opens the merged
 *   full-screen Search surface, Story 13.4), and a compose button.
 *
 * Every tappable target is ≥44pt with an accessible name. No forked sidebar and
 * no bottom tab bar — the drawer carries the nav.
 */
import { Pencil, Search, Users } from "lucide-react";
import type { Ref } from "react";
import { Avatar, AvatarBadge, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { accountHueVar } from "@/lib/account-hue";
import { initials } from "@/lib/account-initials";
import { BRIDGE_HEALTH_DOT_CLASS, BRIDGE_HEALTH_LABEL } from "@/lib/bridges";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useWorstBridgeHealth } from "@/lib/stores/bridge-health";
import { usePendingDraftCount } from "@/lib/stores/drafts";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { newChatStore } from "@/lib/stores/new-chat";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { searchSurfaceStore } from "@/lib/stores/search-surface";
import { cn } from "@/lib/utils";

const FOCUS_RING = "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

interface PhoneInboxHeaderProps {
  /** Forwarded to the avatar drawer button so the shell can focus it on close (UX-DR28). */
  drawerButtonRef?: Ref<HTMLButtonElement>;
  /** Forwarded to the magnifier so the shell can return focus to it on Search close (UX-DR28). */
  magnifierRef?: Ref<HTMLButtonElement>;
}

export function PhoneInboxHeader({ drawerButtonRef, magnifierRef }: PhoneInboxHeaderProps) {
  // The active account filter (Story 2.5): the avatar renders that account's cue
  // when set, else a neutral all-accounts avatar.
  const filterAccountId = useAccountsStore((s) => s.filterAccountId);
  const accounts = useAccountsStore((s) => s.accounts);
  const filteredAccount = accounts.find((a) => a.accountId === filterAccountId) ?? null;
  // Worst-state bridge health (Story 6.5): the dot shows only when unhealthy.
  const bridgeHealth = useWorstBridgeHealth();
  const showHealthDot = bridgeHealth === "degraded" || bridgeHealth === "disconnected";
  // The pending-draft count (Story 7.3): the amber Approval chip shows only > 0.
  const pendingDraftCount = usePendingDraftCount();

  return (
    // Safe-area top inset (Story 13.5, FR-59): the notch/status-bar band pads
    // *above* the 52px content row (total = safe-top + 52px), keeping every
    // ≥44pt target clear of the notch; --safe-top resolves to 0 off-phone.
    <header className="flex h-[calc(var(--safe-top)+var(--phone-header))] shrink-0 items-center gap-1 border-border border-b px-1 pt-[var(--safe-top)]">
      <button
        ref={drawerButtonRef}
        type="button"
        aria-label="Open navigation"
        onClick={() => leadingDrawerStore.getState().open()}
        className={cn("flex size-11 shrink-0 items-center justify-center rounded-full", FOCUS_RING)}
      >
        <Avatar>
          {filteredAccount ? (
            <AvatarFallback
              style={{ backgroundColor: accountHueVar(filteredAccount.hueIndex) }}
              className="font-medium text-white"
            >
              {initials(filteredAccount.userId)}
            </AvatarFallback>
          ) : (
            <AvatarFallback>
              <Users aria-hidden="true" className="size-4" />
            </AvatarFallback>
          )}
          {showHealthDot && bridgeHealth !== null && (
            <AvatarBadge
              data-slot="bridge-health-dot"
              aria-label={BRIDGE_HEALTH_LABEL[bridgeHealth]}
              className={cn(
                "bg-blend-normal ring-background",
                BRIDGE_HEALTH_DOT_CLASS[bridgeHealth],
              )}
            />
          )}
        </Avatar>
      </button>

      <div className="ml-auto flex shrink-0 items-center gap-1">
        {pendingDraftCount > 0 && (
          <button
            type="button"
            aria-label={`Approvals, ${pendingDraftCount} pending`}
            onClick={() => primaryViewStore.getState().setView("approval")}
            className={cn(
              "inline-flex h-11 min-w-11 items-center justify-center rounded-full bg-held px-3 font-medium text-held-foreground text-sm",
              FOCUS_RING,
            )}
          >
            {pendingDraftCount}
          </button>
        )}
        <Button
          ref={magnifierRef}
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Search"
          onClick={() => searchSurfaceStore.getState().open()}
          className={cn("size-11 shrink-0", FOCUS_RING)}
        >
          <Search aria-hidden="true" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="New chat"
          onClick={() => newChatStore.getState().open()}
          className={cn("size-11 shrink-0", FOCUS_RING)}
        >
          <Pencil aria-hidden="true" />
        </Button>
      </div>
    </header>
  );
}

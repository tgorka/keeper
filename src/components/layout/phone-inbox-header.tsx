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
 *   all-accounts avatar, with a worst-state bridge-health lamp overlaid on it,
 *   shown only for `degraded`/`disconnected` (hidden on `healthy`/`null`).
 * - Trailing: an amber Approval chip shown only when the pending-Draft count is
 *   > 0 (deep-links to the Approval Pane), a magnifier (opens the merged
 *   full-screen Search surface, Story 13.4), and a compose button.
 *
 * Every tappable target is ≥44pt with an accessible name. No forked sidebar and
 * no bottom tab bar — the drawer carries the nav.
 */
import { Pencil, Search, Users } from "lucide-react";
import type { Ref } from "react";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Lamp } from "@/components/ui/lamp";
import { accountHueVar } from "@/lib/account-hue";
import { initials } from "@/lib/account-initials";
import { BRIDGE_HEALTH_LABEL, BRIDGE_HEALTH_LAMP } from "@/lib/bridges";
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
  // A lamp inside a button with an explicit `aria-label` is mute: the label
  // replaces the contents rather than joining them. So the health rides the
  // button's own name, and the lamp beside it carries the shape.
  const drawerLabel =
    showHealthDot && bridgeHealth !== null
      ? `Open navigation, ${BRIDGE_HEALTH_LABEL[bridgeHealth]}`
      : "Open navigation";

  return (
    // Safe-area top inset (Story 13.5, FR-59): the notch/status-bar band pads
    // *above* the 52px content row (total = safe-top + 52px), keeping every
    // ≥44pt target clear of the notch; --safe-top resolves to 0 off-phone.
    <header className="flex h-[calc(var(--safe-top)+var(--phone-header))] shrink-0 items-center gap-1 border-border border-b px-1 pt-[var(--safe-top)]">
      <button
        ref={drawerButtonRef}
        type="button"
        aria-label={drawerLabel}
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
            /* The roll-up lamp, sitting where the avatar badge sat. That badge
               was a tint plus a bare `aria-label` on a role-less `<span>`,
               which has nothing to hang a name on and went unannounced. The
               lamp itself stays silent here for a different reason — the
               button's own `aria-label` overrides everything inside it — so the
               word rides the button name above and the shape rides this glyph.
               The background disc is what the hollow and dashed states show
               through, and it keeps the lamp legible against the avatar. */
            <Lamp
              state={BRIDGE_HEALTH_LAMP[bridgeHealth]}
              label={null}
              data-slot="bridge-health-dot"
              className="absolute right-0 bottom-0 z-10 rounded-full bg-background p-px ring-2 ring-background"
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
          className="size-11 shrink-0"
        >
          <Search aria-hidden="true" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="New chat"
          onClick={() => newChatStore.getState().open()}
          className="size-11 shrink-0"
        >
          <Pencil aria-hidden="true" />
        </Button>
      </div>
    </header>
  );
}

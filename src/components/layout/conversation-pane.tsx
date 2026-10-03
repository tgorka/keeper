/**
 * Conversation pane: the chat's open room (FR-8/FR-9, AD-4/AD-8/AD-19).
 *
 * The room is `roomsStore.selected`. The pane draws the chat's own header row
 * (identity, Incognito chip, export, the detail toggle) and the bridge health
 * banner, and hands the room to {@link ConversationBody} — the timeline and
 * composer the notes view's dock draws too — under the chat's singleton stores.
 */
import { Download, PanelRight } from "lucide-react";
import { type Ref, useState } from "react";
import { BridgeLoginSheet } from "@/components/bridges/bridge-login-sheet";
import { ConversationBody } from "@/components/chat/conversation-body";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import { Alert, AlertAction, AlertDescription } from "@/components/ui/alert";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { IconHint } from "@/components/ui/tooltip";
import { useCouplingCaveats } from "@/hooks/use-coupling-caveats";
import { useSelectedRoomVm } from "@/hooks/use-selected-room-vm";
import { accountHueVar } from "@/lib/account-hue";
import { initials } from "@/lib/account-initials";
import type { IncognitoVm } from "@/lib/ipc/client";
import { incognitoSetChat, releaseReceipt } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useBridgeHealth } from "@/lib/stores/bridge-health";
import { exportStore } from "@/lib/stores/export";
import { refreshIncognito, useIncognito } from "@/lib/stores/incognito";
import { useRoomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

interface ConversationPaneProps {
  detailOpen: boolean;
  onToggleDetail: () => void;
  toggleRef?: Ref<HTMLButtonElement>;
  /**
   * Whether to render the pane's own header row (Story 13.2). The phone stack
   * passes `false` because its `PhoneHeader` owns the single 52px bar (UX-DR21);
   * desktop callers omit it and keep the header exactly as before. Only the
   * header row is skipped — timeline, banner, and composer are unchanged.
   */
  showHeader?: boolean;
}

/**
 * The hue-tinted account-initial chip for the conversation header (Story 4.6),
 * reusing the account-footer's `AccountAvatar` pattern. Shows the selected room's
 * owning account so a two-account inbox is disambiguated in the header, matching
 * the row's hue edge bar.
 */
function AccountInitialChip({ userId, hueIndex }: { userId: string; hueIndex: number }) {
  return (
    <Avatar size="sm" data-testid="account-initial-chip">
      <AvatarFallback
        style={{ backgroundColor: accountHueVar(hueIndex) }}
        className="font-medium text-white"
      >
        {initials(userId)}
      </AvatarFallback>
    </Avatar>
  );
}

/**
 * The conversation header's identity block (Story 4.6, FR-24): the selected room's
 * {@link RoomAvatar} (the Network badge comes free) + display name + an
 * account-initial chip. When the room's VM is not in any streamed window, it
 * degrades to the account chip alone (looked up from {@link accountsStore} by the
 * selection's `accountId`) — never a crash. Renders nothing when no room is open or
 * the account is unknown. Exported so the phone stack's `PhoneHeader` (Story 13.2)
 * reuses the identical identity block — never a fork.
 */
export function ConversationHeaderIdentity({ accountId }: { accountId: string | null }) {
  const room = useSelectedRoomVm();
  const account = useAccountsStore((s) =>
    accountId === null ? null : (s.accounts.find((a) => a.accountId === accountId) ?? null),
  );

  if (room !== null) {
    return (
      <div className="flex min-w-0 items-center gap-2">
        <RoomAvatar room={room} size="lg" />
        <span className="truncate font-medium text-sm" title={room.displayName}>
          {room.displayName}
        </span>
        {account !== null && (
          <AccountInitialChip userId={account.userId} hueIndex={account.hueIndex} />
        )}
      </div>
    );
  }
  // No streamed VM for the selection: degrade to the account chip alone.
  if (account !== null) {
    return (
      <div className="flex min-w-0 items-center gap-2">
        <AccountInitialChip userId={account.userId} hueIndex={account.hueIndex} />
      </div>
    );
  }
  return null;
}

/** The effective-scope label carried by the header Incognito chip (Story 8.1). The
 * label always reflects *which* scope decided (Chat > Account > Global), not value
 * equality — a per-Chat override reads "this chat overrides account" even when its
 * value matches the account's. */
export function incognitoChipLabel(source: IncognitoVm["source"]): string {
  switch (source) {
    case "chat":
      return "Incognito — this chat overrides account";
    case "account":
      return "Incognito — account";
    default:
      return "Incognito — global";
  }
}

/**
 * The per-Chat Incognito control in the Chat header (Story 8.1 chip + Story 8.2
 * Popover, FR-44/FR-45). Resolved in Rust and mirrored via {@link useIncognito}.
 *
 * When Incognito is *effective* for the open chat, the violet chip
 * ({@link incognitoChipLabel} + the `--incognito` token) is the Popover trigger; the
 * body carries a "Mark read publicly" release action ({@link releaseReceipt}), a
 * control turning Incognito off for this chat, and — when the room's Network couples —
 * the inline coupling caveat. When it is *not* effective, a subtle ghost trigger lets
 * the user enable per-Chat Incognito; the coupling caveat surfaces inline in that
 * enable affordance too (FR-44: the caveat at toggle time). All caveat copy comes from
 * Rust ({@link useCouplingCaveats}); precedence is never re-resolved on the frontend.
 * The per-Chat scope is tri-state, matching the `Option<bool>` the IPC carries: while
 * an explicit override exists (`vm.chat !== null`), either branch also offers "Follow
 * the account/global setting", which writes `null` so the chat resumes inheriting.
 * Exported so the phone stack's `PhoneHeader` (Story 13.2) reuses the identical chip.
 */
export function ConversationIncognitoChip({
  accountId,
  roomId,
  networkId,
}: {
  accountId: string | null;
  roomId: string | null;
  networkId: string | null;
}) {
  const vm = useIncognito(accountId, roomId);
  const caveats = useCouplingCaveats(networkId);
  // Controlled Popover open state, so an action can close it after firing (and a
  // stale/rapid re-click can't double-fire while it's already dismissing).
  const [open, setOpen] = useState(false);
  if (accountId === null || roomId === null || vm === undefined) {
    return null;
  }

  const effective = vm.effective;
  const label = incognitoChipLabel(vm.source);

  // Write the per-Chat scope (`true`/`false` to override, `null` to clear back to
  // inheriting account/global), then re-read the authoritative VM. Best-effort: a
  // write failure is swallowed (never an unhandled rejection); the mirror keeps its
  // last-observed state and the next read reconciles.
  const setChat = (enabled: boolean | null) => {
    void incognitoSetChat(accountId, roomId, enabled)
      .then(() => refreshIncognito(accountId, roomId))
      .catch(() => {});
  };

  // Clearing only means something while an explicit per-Chat override exists; with
  // `vm.chat === null` the chat already follows account/global.
  const followSetting =
    vm.chat === null ? null : (
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={() => {
          setChat(null);
          setOpen(false);
        }}
      >
        Follow the account/global setting
      </Button>
    );

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        {effective ? (
          <Badge
            asChild
            className={cn(
              "bg-incognito text-incognito-foreground focus-visible:ring-incognito/50",
              "cursor-pointer",
            )}
          >
            <button type="button" aria-label={label} title={label}>
              {label}
            </button>
          </Badge>
        ) : (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label="Incognito — off for this chat"
            title="Incognito — off for this chat"
            className="h-6 text-muted-foreground text-xs"
          >
            Incognito off
          </Button>
        )}
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72 gap-3">
        {effective ? (
          <>
            <p className="font-medium text-sm">{label}</p>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => {
                // Explicit user release: dispatch exactly one public m.read. Best-effort
                // — a failure is swallowed (never a UI error). Close the popover so a
                // stale/rapid re-click can't double-fire the release.
                void releaseReceipt(accountId, roomId).catch(() => {});
                setOpen(false);
              }}
            >
              Mark read publicly
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                setChat(false);
                setOpen(false);
              }}
            >
              Turn Incognito off for this chat
            </Button>
            {followSetting}
            {/* Every coupling caveat for the open room's Network — each rendered
                verbatim from Rust on its own line ({@link useCouplingCaveats} returns
                all matches, not just the first). */}
            {caveats.map((caveat) => (
              <p key={caveat.text} className="text-muted-foreground text-xs">
                {caveat.text}
              </p>
            ))}
          </>
        ) : (
          <>
            <p className="font-medium text-sm">Incognito is off for this chat</p>
            <Button type="button" variant="secondary" size="sm" onClick={() => setChat(true)}>
              Turn Incognito on for this chat
            </Button>
            {followSetting}
            {caveats.map((caveat) => (
              <p key={caveat.text} className="text-muted-foreground text-xs">
                {caveat.text}
              </p>
            ))}
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}

/**
 * The non-dismissible in-conversation re-link banner (Story 6.5, FR-28, UX-DR11).
 *
 * Shown iff the open room's `(accountId, networkId)` matches an *unhealthy* bridge
 * session (both keys must match — the machine `networkId`, never the display label).
 * The banner is persistent until the session recovers — never a dismissible toast.
 * Its single "Re-link" action opens the shipped {@link BridgeLoginSheet} for that exact
 * `(accountId, networkId)` (the same `start_bridge_login` entry the card uses — no new
 * login flow). Renders nothing for a native room, a healthy/unmonitored session, or no
 * open room. The health state is Rust-authoritative — this is a pure projection.
 */
export function ConversationHealthBanner({
  accountId,
  networkId,
}: {
  accountId: string | null;
  networkId: string | null;
}) {
  const [relinkOpen, setRelinkOpen] = useState(false);
  const health = useBridgeHealth(accountId ?? "", networkId ?? "");

  if (accountId === null || networkId === null || health === undefined) {
    return null;
  }
  if (health.health === "healthy") {
    return null;
  }

  return (
    <div className="shrink-0 px-3 pt-2">
      {/* role="alert" (not "status") — an unhealthy session is a persistent, actionable
          problem the user must see. No dismiss control: it clears only on recovery. */}
      <Alert role="alert" variant="destructive" className="pr-28">
        <AlertDescription>
          {health.networkName} disconnected — messages may not arrive.
        </AlertDescription>
        <AlertAction>
          <Button type="button" variant="outline" size="xs" onClick={() => setRelinkOpen(true)}>
            Re-link
          </Button>
        </AlertAction>
      </Alert>
      <BridgeLoginSheet
        accountId={accountId}
        networkId={networkId}
        networkName={health.networkName}
        open={relinkOpen}
        onOpenChange={setRelinkOpen}
      />
    </div>
  );
}

export function ConversationPane({
  detailOpen,
  onToggleDetail,
  toggleRef,
  showHeader = true,
}: ConversationPaneProps) {
  const selected = useRoomsStore((s) => s.selected);
  const accountId = selected?.accountId ?? null;
  const selectedRoomId = selected?.roomId ?? null;
  // The open room's stable machine `networkId` (Story 6.5) — the health join key.
  // `null` for a native room or when the room's VM isn't in any streamed window.
  const selectedRoom = useSelectedRoomVm();
  const selectedNetworkId = selectedRoom?.networkId ?? null;

  return (
    // The seam against the detail panel is this pane's, not the panel's:
    // DESIGN.md → Elevation & Depth gives a boundary to the earlier sibling,
    // and `last:` cancels it in the far commoner arrangement where the detail
    // panel is closed or floating in a Sheet and there is nothing to the right
    // but the window. Same pixels either way; one owner, so the next person to
    // add a right-hand neighbour cannot accidentally draw a second line.
    <main className="flex h-full min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0">
      {showHeader && (
        <div className="flex shrink-0 items-center justify-between gap-2 border-border border-b p-2">
          <div className="flex min-w-0 items-center gap-2">
            <ConversationHeaderIdentity accountId={accountId} />
            <ConversationIncognitoChip
              // Key by roomId so a room switch remounts the chip: it can never leave a
              // Popover bound (open) to the previously selected chat.
              key={selectedRoomId ?? ""}
              accountId={accountId}
              roomId={selectedRoomId}
              networkId={selectedNetworkId}
            />
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {accountId !== null && selectedRoomId !== null && (
              <IconHint label="Export this chat">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label="Export this chat"
                  onClick={() =>
                    exportStore.getState().open({
                      scope: "chat",
                      accountId,
                      roomId: selectedRoomId,
                    })
                  }
                  className=""
                >
                  <Download aria-hidden="true" />
                </Button>
              </IconHint>
            )}
            <IconHint label="Toggle detail panel">
              <Button
                ref={toggleRef}
                type="button"
                variant="ghost"
                size="icon"
                aria-label="Toggle detail panel"
                aria-pressed={detailOpen}
                onClick={onToggleDetail}
                className="shrink-0"
              >
                <PanelRight aria-hidden="true" />
              </Button>
            </IconHint>
          </div>
        </div>
      )}
      {/* Non-dismissible in-conversation re-link banner (Story 6.5, UX-DR11): shown iff
          the open room's (accountId, networkId) session is unhealthy → opens the login
          stepper for that exact bridge. Persistent until the session recovers. */}
      <ConversationHealthBanner accountId={accountId} networkId={selectedNetworkId} />
      <ConversationBody
        accountId={accountId}
        roomId={selectedRoomId}
        followsSearch
        acceptsWindowDrops
      />
    </main>
  );
}

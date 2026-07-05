/**
 * A single Bridge card (Story 6.1 catalog join + Story 6.2 discovery status).
 *
 * Renders one discovered Network for one Account: the glyph avatar, the Network
 * name, the data-driven risk-tier {@link Badge}, the discovery status word + dot
 * (Connected / Action needed / Not set up, from the `status` prop), a separate
 * placeholder live-health dot (real health is Story 6.5), and a primary action. All
 * risk/badge/ack copy comes from the backend catalog {@link BridgeNetworkVm} — nothing
 * is hardcoded here; the status word comes from the discovery {@link BridgeStatus} via
 * the shared {@link BRIDGE_STATUS_LABEL} map. When the tier `requiresAck` (volatile /
 * conditional), the action opens an {@link AlertDialog} showing the tier badge + the
 * backend `ackCopy` and gates on an explicit confirm; otherwise it proceeds directly.
 * Proceeding (post-ack) opens the native login {@link BridgeLoginSheet} (Story 6.3),
 * which drives the provisioning login state machine natively.
 */
import { useState } from "react";
import { toast } from "sonner";
import { BridgeLoginSheet } from "@/components/bridges/bridge-login-sheet";
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
import { Card } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { BRIDGE_STATUS_LABEL } from "@/lib/bridges";
import type { BadgeStyle, BridgeNetworkVm, BridgeStatus } from "@/lib/ipc/client";
import { bridgeBotRoom } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

/** The shadcn Badge variant + `--bridge-*` tint for each backend badge style. */
const BADGE_STYLE: Record<BadgeStyle, { variant: "secondary" | "outline"; className?: string }> = {
  secondary: { variant: "secondary" },
  outlineDegraded: {
    variant: "outline",
    className: "border-bridge-degraded/50 text-bridge-degraded",
  },
  filledDisconnected: {
    variant: "outline",
    className: "border-transparent bg-bridge-disconnected text-white",
  },
  outline: { variant: "outline" },
};

/** The discovery status dot tint for each {@link BridgeStatus}. */
const STATUS_DOT_CLASS: Record<BridgeStatus, string> = {
  loggedIn: "bg-bridge-healthy",
  notLoggedIn: "bg-bridge-disconnected",
  configured: "bg-muted-foreground/50",
};

interface BridgeCardProps {
  /** The catalog network this card represents (glyph/name/tier/ack). */
  network: BridgeNetworkVm;
  /** The account id this card is keyed to (Network × Account). */
  accountId: string;
  /** The discovered setup/login status (Story 6.2). */
  status: BridgeStatus;
}

export function BridgeCard({ network, accountId, status }: BridgeCardProps) {
  const [ackOpen, setAckOpen] = useState(false);
  const [loginOpen, setLoginOpen] = useState(false);
  const badge = BADGE_STYLE[network.badgeStyle];

  // Data-driven action label: an ack-gated (volatile / conditional) Network is
  // "Set up"; a directly-connectable one is "Connect".
  const actionLabel = network.requiresAck ? "Set up" : "Connect";

  // Proceed (post-ack, or directly for a non-gated Network): close any gate and
  // open the native login Sheet, which drives the provisioning login (Story 6.3).
  const proceed = () => {
    setAckOpen(false);
    setLoginOpen(true);
  };

  const onAction = () => {
    if (network.requiresAck) {
      setAckOpen(true);
      return;
    }
    proceed();
  };

  // The manual escape hatch (UX-DR19): resolve-or-create the raw Bridge Bot DM room
  // and navigate straight to it (Inbox + select the room), keeping the bot reachable
  // even when native login isn't possible. A resolve failure is logged, not thrown —
  // the menu action is best-effort and must never crash the card.
  const openBotChat = async () => {
    try {
      const roomId = await bridgeBotRoom(accountId, network.networkId);
      primaryViewStore.getState().setView("inbox");
      roomsStore.getState().selectRoom({ accountId, roomId });
    } catch (error) {
      console.error("could not open the Bridge Bot chat", error);
      toast.error("Couldn't open the Bridge Bot chat. Try again.");
    }
  };

  const tierBadge = (
    <Badge variant={badge.variant} className={badge.className}>
      {network.tierLabel}
    </Badge>
  );

  return (
    <Card
      size="sm"
      data-account-id={accountId}
      data-network-id={network.networkId}
      className="flex-row items-center gap-3 px-4"
    >
      <Avatar size="sm">
        <AvatarFallback aria-hidden="true">{network.glyph}</AvatarFallback>
      </Avatar>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium">{network.name}</span>
          {tierBadge}
        </div>
        {/* Discovery status word + dot (Story 6.2) — the setup/login state, distinct
            from the placeholder live-health dot on the right (Story 6.5). */}
        <div className="flex items-center gap-1.5" data-slot="bridge-status">
          <span
            aria-hidden="true"
            className={cn("size-1.5 shrink-0 rounded-full", STATUS_DOT_CLASS[status])}
          />
          <span className="text-muted-foreground text-xs">{BRIDGE_STATUS_LABEL[status]}</span>
        </div>
      </div>
      {/* Placeholder health dot — neutral until the real state machine (Story 6.5). */}
      <span
        aria-hidden="true"
        data-slot="bridge-health-dot"
        className="size-2 shrink-0 rounded-full bg-muted-foreground/30"
      />
      <Button
        type="button"
        size="sm"
        variant="outline"
        onClick={onAction}
        aria-label={`${actionLabel} ${network.name}`}
      >
        {actionLabel}
      </Button>

      {/* Manage menu (UX-DR19): for now its only item is the manual escape hatch to
          the raw Bridge Bot chat; Re-link / Log out / View sessions arrive with
          Stories 6.5/6.6. */}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" size="sm" variant="ghost" aria-label={`Manage ${network.name}`}>
            Manage
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => void openBotChat()}>
            Open Bridge Bot chat
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      {network.requiresAck && (
        <AlertDialog open={ackOpen} onOpenChange={setAckOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle className="flex items-center gap-2">
                <span>Connect {network.name}?</span>
              </AlertDialogTitle>
              <div className={cn("flex items-center")}>{tierBadge}</div>
              <AlertDialogDescription>{network.ackCopy}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction onClick={proceed}>
                I understand the risk — connect
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      )}

      <BridgeLoginSheet
        accountId={accountId}
        networkId={network.networkId}
        networkName={network.name}
        open={loginOpen}
        onOpenChange={setLoginOpen}
      />
    </Card>
  );
}

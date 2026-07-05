/**
 * A single Bridge card (Story 6.1, FR-42).
 *
 * Renders one connectable Network for one Account: the glyph avatar, the Network
 * name, the data-driven risk-tier {@link Badge}, a placeholder health dot (real
 * health is Story 6.5), and a primary action. All risk/badge/ack copy comes from
 * the backend {@link BridgeNetworkVm} — nothing is hardcoded here. When the tier
 * `requiresAck` (volatile / conditional), the action opens an {@link AlertDialog}
 * showing the tier badge + the backend `ackCopy` and gates on an explicit confirm;
 * otherwise the action proceeds directly. No real login happens this story — the
 * confirm/proceed is a stub that just closes the gate (provisioning is Story 6.3).
 */
import { useState } from "react";
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
import type { BadgeStyle, BridgeNetworkVm } from "@/lib/ipc/client";
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

interface BridgeCardProps {
  /** The catalog network this card represents. */
  network: BridgeNetworkVm;
  /** The account id this card is keyed to (Network × Account). */
  accountId: string;
}

export function BridgeCard({ network, accountId }: BridgeCardProps) {
  const [ackOpen, setAckOpen] = useState(false);
  const badge = BADGE_STYLE[network.badgeStyle];

  // Data-driven action label: an ack-gated (volatile / conditional) Network is
  // "Set up"; a directly-connectable one is "Connect".
  const actionLabel = network.requiresAck ? "Set up" : "Connect";

  // The connect stub. No real login this story (provisioning is Story 6.3) —
  // proceeding just closes any gate. Keyed per Network × Account so a later story
  // can dispatch the real bot login from here.
  const proceed = () => {
    setAckOpen(false);
  };

  const onAction = () => {
    if (network.requiresAck) {
      setAckOpen(true);
      return;
    }
    proceed();
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
    </Card>
  );
}

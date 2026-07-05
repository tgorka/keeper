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
import { useEffect, useRef, useState } from "react";
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
import { BRIDGE_HEALTH_DOT_CLASS, BRIDGE_HEALTH_LABEL, BRIDGE_STATUS_LABEL } from "@/lib/bridges";
import { formatRoomTimestamp } from "@/lib/format-time";
import type { BadgeStyle, BridgeHealth, BridgeNetworkVm, BridgeStatus } from "@/lib/ipc/client";
import { bridgeBotRoom } from "@/lib/ipc/client";
import { useBridgeHealth } from "@/lib/stores/bridge-health";
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

/** Whether a live health is an unhealthy (surfaced) state. */
function isUnhealthy(health: BridgeHealth): boolean {
  return health === "degraded" || health === "disconnected";
}

export function BridgeCard({ network, accountId, status }: BridgeCardProps) {
  const [ackOpen, setAckOpen] = useState(false);
  const [loginOpen, setLoginOpen] = useState(false);
  const badge = BADGE_STYLE[network.badgeStyle];

  // The live health for this exact (accountId, networkId) session, from the
  // Rust-authoritative store (Story 6.5). `undefined` when the session is not
  // monitored (not logged in) — the card then shows no live-health dot/word/edge.
  const health = useBridgeHealth(accountId, network.networkId);
  const liveHealth = health?.health;

  // Pulse-twice-then-steady on a transition INTO an unhealthy state (UX-DR8): the dot
  // pulses only when the health newly becomes unhealthy, then settles to a steady tint
  // — a persistent (never dismissible) indicator. Tracked against the previous health
  // so a re-render on unrelated state never re-triggers the pulse.
  const prevHealthRef = useRef<BridgeHealth | undefined>(undefined);
  const [pulsing, setPulsing] = useState(false);
  useEffect(() => {
    const prev = prevHealthRef.current;
    prevHealthRef.current = liveHealth;
    if (liveHealth !== undefined && isUnhealthy(liveHealth) && prev !== liveHealth) {
      setPulsing(true);
      // Two pulses (~0.6 s each) then steady.
      const timer = setTimeout(() => setPulsing(false), 1200);
      return () => clearTimeout(timer);
    }
    if (liveHealth === undefined || !isUnhealthy(liveHealth)) {
      setPulsing(false);
    }
  }, [liveHealth]);

  const showRedEdge = liveHealth === "disconnected";

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
    // A disconnected session re-links straight into the existing login stepper
    // (Story 6.5, AD-16) — the session was already acknowledged at first login, so the
    // volatile/conditional ack gate is not re-shown on a re-link.
    if (liveHealth === "disconnected") {
      proceed();
      return;
    }
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
      className={cn(
        "relative flex-row items-center gap-3 px-4",
        // A 3 px disconnected-red left edge on a dead session (UX-DR11) — a
        // persistent, unmissable indicator.
        showRedEdge && "pl-[19px]",
      )}
    >
      {showRedEdge && (
        <span
          aria-hidden="true"
          data-slot="bridge-health-edge"
          className="absolute inset-y-0 left-0 w-[3px] rounded-l bg-bridge-disconnected"
        />
      )}
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
      {/* Live-health block (Story 6.5): the state word + a dot that pulses twice on a
          transition into an unhealthy state then goes steady, plus the last-checked
          time. Shown only for a monitored (logged-in) session; a neutral placeholder
          dot otherwise. Rust owns the state — this is a pure projection. */}
      {health !== undefined ? (
        <div className="flex shrink-0 flex-col items-end gap-0.5" data-slot="bridge-health">
          <div className="flex items-center gap-1.5">
            <span
              className="text-muted-foreground text-xs"
              data-slot="bridge-health-word"
              data-testid="bridge-health-word"
            >
              {BRIDGE_HEALTH_LABEL[health.health]}
            </span>
            <span
              aria-hidden="true"
              data-slot="bridge-health-dot"
              className={cn(
                "size-2 rounded-full",
                BRIDGE_HEALTH_DOT_CLASS[health.health],
                pulsing && "animate-pulse",
              )}
            />
          </div>
          <span
            className="text-[10px] text-muted-foreground/70"
            data-slot="bridge-health-checked"
            data-testid="bridge-health-checked"
          >
            Checked {formatRoomTimestamp(health.lastCheckedMs)}
          </span>
        </div>
      ) : (
        <span
          aria-hidden="true"
          data-slot="bridge-health-dot"
          className="size-2 shrink-0 rounded-full bg-muted-foreground/30"
        />
      )}
      <Button
        type="button"
        size="sm"
        variant={showRedEdge ? "default" : "outline"}
        onClick={onAction}
        aria-label={`${liveHealth === "disconnected" ? "Re-link" : actionLabel} ${network.name}`}
      >
        {liveHealth === "disconnected" ? "Re-link" : actionLabel}
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

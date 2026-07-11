/**
 * NETWORKS sidebar group (FR-24, UX-DR, Story 4.6; per-Network mute Story 10.2).
 *
 * A labeled `section-label` group of single-select rows, one per distinct bridged
 * Network connected across all accounts, in the Rust-authoritative (name-sorted)
 * order the {@link networksStore} mirror holds. Selecting a Network filters the
 * Unified Inbox to that Network's rooms across every account; the filtering itself
 * is computed in Rust (poked via {@link setNetworkFilter}) — this component only
 * records the selection and reflects it as `aria-current` / accent styling.
 *
 * Single-select toggle: clicking the active row clears the filter; clicking any
 * other row selects it. The group is hidden entirely (`return null`) when the
 * Network list is empty (no bridged rooms).
 *
 * Right-clicking a Network row opens a context menu with a "Mute Network" /
 * "Unmute Network" toggle (Story 10.2, FR-52). Muting is keeper-local, persisted in
 * `keeper.db`; every Chat bridged to that Network stops posting notifications while
 * unread still accrues. The muted state is Rust-authoritative — the row loads it via
 * {@link networkMuteGet} and reflects it with a bell-off glyph.
 */

import { BellOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { useLongPress } from "@/hooks/use-long-press";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { NetworkVm } from "@/lib/ipc/client";
import { networkMuteGet, networkMuteSet, setNetworkFilter } from "@/lib/ipc/client";
import { networksStore, useNetworksStore } from "@/lib/stores/networks";
import { cn } from "@/lib/utils";

/**
 * One Network row: the single-select filter chip wrapped in a mute context menu.
 * The muted state is loaded on mount (fail-open to "not muted") and reflected with a
 * bell-off glyph; a monotonic `writeId` guards a slow failed toggle from clobbering a
 * newer successful one.
 */
function NetworkRow({ network, isActive }: { network: NetworkVm; isActive: boolean }) {
  const [muted, setMuted] = useState<boolean | undefined>(undefined);
  const writeId = useRef(0);
  // Phone touch idiom (Story 13.6): a long-press opens the same mute-toggle
  // ContextMenu the desktop right-click does; the native callout is suppressed.
  const { phone } = useShellLayout();
  const longPress = useLongPress();

  useEffect(() => {
    let cancelled = false;
    void networkMuteGet(network.name)
      .then((v) => {
        if (!cancelled) {
          setMuted(v);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setMuted(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [network.name]);

  const onRowClick = () => {
    if (isActive) {
      // Toggle off: clear the selection and the Rust filter.
      networksStore.getState().setActiveNetwork(null);
      void setNetworkFilter(null).catch(() => {});
    } else {
      networksStore.getState().setActiveNetwork(network.name);
      void setNetworkFilter(network.name).catch(() => {});
    }
  };

  const onToggleMute = () => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = muted ?? false;
    const next = !prev;
    setMuted(next);
    void networkMuteSet(network.name, next).catch(() => {
      // Revert only if no newer toggle superseded this one.
      if (id === writeId.current) {
        setMuted(prev);
      }
    });
  };

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <button
          type="button"
          onClick={onRowClick}
          {...longPress}
          aria-current={isActive ? "true" : undefined}
          aria-pressed={isActive}
          className={cn(
            "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
            isActive ? "bg-accent text-accent-foreground" : "hover:bg-accent",
            // Long-press target (Story 13.6): suppress the native callout and
            // selection on the phone tier only.
            phone && "touch-callout-none select-none",
          )}
        >
          <Avatar size="sm">
            <AvatarFallback className="bg-secondary text-secondary-foreground">
              {[...network.name][0]?.toUpperCase() ?? ""}
            </AvatarFallback>
          </Avatar>
          <span className="truncate text-sm">{network.name}</span>
          {muted === true && (
            <BellOff
              aria-label="Muted"
              data-testid="network-mute-glyph"
              className="ml-auto size-3 shrink-0 text-muted-foreground"
            />
          )}
        </button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {muted ? (
          <ContextMenuItem className={phone ? "min-h-11" : undefined} onSelect={onToggleMute}>
            Unmute Network
          </ContextMenuItem>
        ) : (
          <ContextMenuItem className={phone ? "min-h-11" : undefined} onSelect={onToggleMute}>
            Mute Network
          </ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function NetworksGroup() {
  const networks = useNetworksStore((s) => s.networks);
  const activeNetwork = useNetworksStore((s) => s.activeNetwork);

  // Hidden entirely when there are no Networks (no bridged rooms): no label, no rows.
  if (networks.length === 0) {
    return null;
  }

  return (
    <section aria-label="Networks" className="flex flex-col px-2 pb-1">
      <span className="px-2 py-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
        Networks
      </span>
      <ul className="flex flex-col gap-0.5">
        {networks.map((network) => (
          <li key={network.name}>
            <NetworkRow network={network} isActive={activeNetwork === network.name} />
          </li>
        ))}
      </ul>
    </section>
  );
}

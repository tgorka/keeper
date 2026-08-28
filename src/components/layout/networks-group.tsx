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
 *
 * **It folds, and it survives the menu folding** (Story 45.20, UX-DR81), on the
 * same terms as SPACES: the group used to vanish from the collapsed rail, so
 * folding the drawer removed a navigation surface rather than shrinking one.
 * Each row keeps the Network's name as its accessible name in both renderings,
 * and the mute context menu rides along unchanged — a folded row is the same
 * control, drawn narrower.
 *
 * **`Radio` is this group's glyph and nothing else's.** It used to be drawn
 * twice in one window: here, and on the Bridges nav row six rows up. One glyph
 * standing for two concepts stands for neither, so Bridges took `Cable` — a
 * bridge is a link between two systems — and the arcs radiating from a point
 * stayed here, where they mean a network's signal and where they still read at
 * the 14px the folded header draws them at.
 */

import { BellOff, Radio } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { FOLD_STRIP } from "@/components/layout/fold-strip";
import { FoldableGroup } from "@/components/layout/sidebar-group";
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
function NetworkRow({
  network,
  isActive,
  collapsed,
}: {
  network: NetworkVm;
  isActive: boolean;
  collapsed: boolean;
}) {
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
          // The Network's own name, in both renderings, and it carries the mute
          // state with it. On the rail the `BellOff` glyph is the only thing
          // saying "muted" and a glyph is not a name, so the state joins the
          // name — otherwise folding the menu would hide a fact the unfolded
          // row states.
          aria-label={muted === true ? `${network.name}, muted` : network.name}
          className={cn(
            "flex items-center rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
            // On the rail this row is one item on the strip, at the strip's
            // item size. It used to be `p-1.5` around a 24px avatar with a
            // paragraph explaining that 24+6+6 is 36 — a sum that stops being
            // 36 the day the avatar changes, and a hover pill 4px narrower
            // than the one on the row directly above it when it did.
            collapsed ? cn("justify-center", FOLD_STRIP.controlClass) : "w-full gap-2 px-2 py-1.5",
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
          {!collapsed && <span className="truncate text-sm">{network.name}</span>}
          {muted === true && (
            <BellOff
              aria-hidden="true"
              data-testid="network-mute-glyph"
              className={cn(
                "size-3 shrink-0 text-muted-foreground",
                collapsed ? "-ml-2 self-start" : "ml-auto",
              )}
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

export function NetworksGroup({ collapsed = false }: { collapsed?: boolean }) {
  const networks = useNetworksStore((s) => s.networks);
  const activeNetwork = useNetworksStore((s) => s.activeNetwork);

  // Hidden entirely when there are no Networks (no bridged rooms): no label, no rows.
  if (networks.length === 0) {
    return null;
  }

  return (
    <FoldableGroup label="Networks" icon={Radio} group="networks" collapsed={collapsed}>
      {networks.map((network) => (
        <li key={network.name}>
          <NetworkRow
            network={network}
            isActive={activeNetwork === network.name}
            collapsed={collapsed}
          />
        </li>
      ))}
    </FoldableGroup>
  );
}

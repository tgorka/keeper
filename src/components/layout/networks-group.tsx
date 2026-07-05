/**
 * NETWORKS sidebar group (FR-24, UX-DR, Story 4.6).
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
 * Network list is empty (no bridged rooms). No bridge-health dot (deferred to
 * Epic 6).
 */

import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import type { NetworkVm } from "@/lib/ipc/client";
import { setNetworkFilter } from "@/lib/ipc/client";
import { networksStore, useNetworksStore } from "@/lib/stores/networks";
import { cn } from "@/lib/utils";

export function NetworksGroup() {
  const networks = useNetworksStore((s) => s.networks);
  const activeNetwork = useNetworksStore((s) => s.activeNetwork);

  // Hidden entirely when there are no Networks (no bridged rooms): no label, no rows.
  if (networks.length === 0) {
    return null;
  }

  const onRowClick = (network: NetworkVm) => {
    const isActive = activeNetwork === network.name;
    if (isActive) {
      // Toggle off: clear the selection and the Rust filter.
      networksStore.getState().setActiveNetwork(null);
      void setNetworkFilter(null).catch(() => {});
    } else {
      networksStore.getState().setActiveNetwork(network.name);
      void setNetworkFilter(network.name).catch(() => {});
    }
  };

  return (
    <section aria-label="Networks" className="flex flex-col px-2 pb-1">
      <span className="px-2 py-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
        Networks
      </span>
      <ul className="flex flex-col gap-0.5">
        {networks.map((network) => {
          const isActive = activeNetwork === network.name;
          return (
            <li key={network.name}>
              <button
                type="button"
                onClick={() => onRowClick(network)}
                aria-current={isActive ? "true" : undefined}
                aria-pressed={isActive}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                  isActive ? "bg-accent text-accent-foreground" : "hover:bg-accent",
                )}
              >
                <Avatar size="sm">
                  <AvatarFallback className="bg-secondary text-secondary-foreground">
                    {[...network.name][0]?.toUpperCase() ?? ""}
                  </AvatarFallback>
                </Avatar>
                <span className="truncate text-sm">{network.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

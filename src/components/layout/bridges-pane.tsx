/**
 * The Bridges primary view (Story 6.1 catalog + Story 6.2 discovery).
 *
 * Replaces 6.1's static catalog projection with real, per-Account zero-config
 * discovery. For each signed-in account it runs the three-source discovery pass
 * ({@link useBridgeDiscovery}) and renders a {@link BridgeCard} per *discovered*
 * Network — each joined to the 6.1 catalog by `networkId` for glyph/name/tier badge/
 * ack copy — keyed Network × Account. When an account discovers no catalog bridges it
 * shows "No bridges found on {homeserver}." with a companion-stack docs link. Loading
 * and (retriable) error states are honest and per-account. With zero accounts it shows
 * an add-account prompt. The catalog is fetched once ({@link useBridgeCatalog}) as the
 * presentation join table; a parse failure shows an error state. Nothing here performs
 * Matrix I/O directly — the Rust core owns discovery (real login is Story 6.3, live
 * health Story 6.5).
 */
import { BbctlPanel } from "@/components/bridges/bbctl-panel";
import { BridgeCard } from "@/components/bridges/bridge-card";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useBridgeCatalog } from "@/hooks/use-bridge-catalog";
import { useBridgeDiscovery } from "@/hooks/use-bridge-discovery";
import { COMPANION_STACK_DOCS_URL } from "@/lib/bridges";
import type { AccountVm, BridgeNetworkVm } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";

export function BridgesPane() {
  const accounts = useAccountsStore((s) => s.accounts);
  const { catalog, loading, error } = useBridgeCatalog();

  return (
    <section
      aria-label="Bridges"
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="shrink-0 border-border border-b px-6 py-4">
        <h1 className="font-heading font-medium text-lg">Bridges</h1>
        <p className="text-muted-foreground text-sm">
          Connect a network to bring its chats into keeper.
        </p>
      </header>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-6 p-6">
          {error !== null ? (
            <p role="alert" className="text-destructive text-sm">
              Bridges are unavailable right now: {error}
            </p>
          ) : accounts.length === 0 ? (
            <p className="text-muted-foreground text-sm">Add an account to set up bridges.</p>
          ) : loading || catalog === null ? (
            <p className="text-muted-foreground text-sm">Loading bridges…</p>
          ) : (
            accounts.map((account) => (
              <AccountBridges key={account.accountId} account={account} catalog={catalog} />
            ))
          )}
        </div>
      </ScrollArea>
    </section>
  );
}

/** The catalog entry for a network id, or `undefined` when uncatalogued. */
function catalogFor(catalog: BridgeNetworkVm[], networkId: string): BridgeNetworkVm | undefined {
  return catalog.find((n) => n.networkId === networkId);
}

interface AccountBridgesProps {
  account: AccountVm;
  catalog: BridgeNetworkVm[];
}

/**
 * One account's discovered bridges. Runs discovery for the account and renders a
 * card per discovered Network (catalog-joined), with per-account loading, retriable
 * error, and the "No bridges found on {homeserver}." empty state.
 */
function AccountBridges({ account, catalog }: AccountBridgesProps) {
  const { discovery, loading, error, retriable, retry } = useBridgeDiscovery(account.accountId);
  const isBeeper = account.provider === "beeper";
  // The bbctl "run your own bridge" panel is a desktop-only capability (it spawns a
  // local sidecar). Hide it wherever the platform lacks that capability — on the
  // phone tier the runner is absent, but discovery/provisioning/health stay.
  const bridgeSidecar = useCapabilitiesStore((s) => s.capabilities.bridgeSidecar);

  return (
    <div className="flex flex-col gap-3">
      <h2 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {account.userId}
      </h2>

      {error !== null ? (
        <div role="alert" className="flex flex-col items-start gap-2 text-sm">
          <p className="text-destructive">Could not discover bridges: {error}</p>
          {retriable && (
            <Button type="button" size="sm" variant="outline" onClick={retry}>
              Retry
            </Button>
          )}
        </div>
      ) : loading || discovery === null ? (
        <p className="text-muted-foreground text-sm">Discovering bridges…</p>
      ) : discovery.networks.length === 0 ? (
        <p className="text-muted-foreground text-sm">
          No bridges found on {discovery.homeserver}.{" "}
          <a
            href={COMPANION_STACK_DOCS_URL}
            target="_blank"
            rel="noreferrer"
            className="underline underline-offset-2"
          >
            Set up a companion stack
          </a>
          .
        </p>
      ) : (
        <div className="flex flex-col gap-2">
          {discovery.networks.map((discovered) => {
            const network = catalogFor(catalog, discovered.networkId);
            // Catalog-gated in the backend, but guard defensively: skip any
            // network the frontend catalog can't present.
            if (network === undefined) {
              return null;
            }
            return (
              <BridgeCard
                key={`${account.accountId}:${discovered.networkId}`}
                network={network}
                accountId={account.accountId}
                status={discovered.status}
              />
            );
          })}
        </div>
      )}

      {/* Beeper-only: run your own bridge via bbctl (Story 6.7). Non-Beeper accounts
          never render this section; the backend gate is defense-in-depth. On a
          successful run we re-run discovery so the new bridge card appears. Gated on
          the `bridgeSidecar` capability so the runner is absent on the phone tier. */}
      {isBeeper && bridgeSidecar && (
        <BbctlPanel accountId={account.accountId} onBridgeAdded={retry} />
      )}
    </div>
  );
}

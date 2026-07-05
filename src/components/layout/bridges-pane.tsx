/**
 * The Bridges primary view (Story 6.1, FR-42).
 *
 * A read-only surface of the data-driven bridge catalog. For each signed-in
 * account it renders a section, and within it a {@link BridgeCard} per catalog
 * Network — cards are keyed Network × Account. With zero accounts it shows an empty
 * state prompting the user to add one. With no accounts there are no cards. The
 * catalog is fetched once over IPC ({@link useBridgeCatalog}); a parse failure
 * shows an honest error state. Health and real provisioning are later stories (6.5
 * / 6.3) — nothing here performs Matrix or network I/O.
 */
import { BridgeCard } from "@/components/bridges/bridge-card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useBridgeCatalog } from "@/hooks/use-bridge-catalog";
import { useAccountsStore } from "@/lib/stores/accounts";

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
              <div key={account.accountId} className="flex flex-col gap-3">
                <h2 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
                  {account.userId}
                </h2>
                <div className="flex flex-col gap-2">
                  {catalog.map((network) => (
                    <BridgeCard
                      key={`${account.accountId}:${network.networkId}`}
                      network={network}
                      accountId={account.accountId}
                    />
                  ))}
                </div>
              </div>
            ))
          )}
        </div>
      </ScrollArea>
    </section>
  );
}

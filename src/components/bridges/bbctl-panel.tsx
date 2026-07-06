/**
 * The Beeper-only "Run your own bridge" section (Story 6.7, FR-29, AD-16).
 *
 * Rendered once per Beeper account (the parent {@link import("@/components/layout/bridges-pane").BridgesPane}
 * filters to `provider === "beeper"` — this is defense-in-depth in the backend gate,
 * not the UI's only gate). Fetches the `bbctl` self-host capability
 * ({@link bbctlAvailability}) and branches:
 *
 * - **available** — a network {@link Select} (supported self-hostable networks only)
 *   + a Run button that opens the run {@link BbctlRunSheet}.
 * - **unavailable** — the guided-install steps (keyed by index — steps may repeat)
 *   + the Beeper self-host docs link. Everything else in keeper keeps working.
 *
 * On a successful run the Sheet calls `onSuccess`, which refreshes the account's
 * bridge discovery so the new bridge card appears with status — no new list/status
 * path is invented. If the store is open for this account but the selected network is
 * gone from `availability.networks`, the store is closed rather than left stuck open
 * with no valid sheet.
 */
import { useEffect, useState } from "react";
import { BbctlRunSheet } from "@/components/bridges/bbctl-run-sheet";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { BbctlAvailabilityVm } from "@/lib/ipc/client";
import { bbctlAvailability } from "@/lib/ipc/client";
import { bbctlStore, useBbctlStore } from "@/lib/stores/bbctl";

interface BbctlPanelProps {
  /** The Beeper account id this section runs bridges for. */
  accountId: string;
  /** Refresh the account's bridge discovery so a newly-run bridge appears. */
  onBridgeAdded: () => void;
}

export function BbctlPanel({ accountId, onBridgeAdded }: BbctlPanelProps) {
  const [availability, setAvailability] = useState<BbctlAvailabilityVm | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    bbctlAvailability()
      .then((vm) => {
        if (!cancelled) {
          setAvailability(vm);
          setError(null);
        }
      })
      .catch((raw: { message?: string }) => {
        if (!cancelled) {
          setError(raw?.message ?? "Could not check for bbctl.");
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error !== null) {
    return (
      <section className="flex flex-col gap-2" aria-label="Run your own bridge">
        <SectionHeading />
        <p role="alert" className="text-destructive text-sm">
          Couldn't check for bbctl: {error}
        </p>
      </section>
    );
  }

  if (availability === null) {
    return (
      <section className="flex flex-col gap-2" aria-label="Run your own bridge">
        <SectionHeading />
        <p className="text-muted-foreground text-sm">Checking for bbctl…</p>
      </section>
    );
  }

  return (
    <section className="flex flex-col gap-3" aria-label="Run your own bridge">
      <SectionHeading />
      {availability.available ? (
        <AvailableBranch
          accountId={accountId}
          availability={availability}
          onBridgeAdded={onBridgeAdded}
        />
      ) : (
        <UnavailableBranch availability={availability} />
      )}
    </section>
  );
}

function SectionHeading() {
  return (
    <div className="flex flex-col gap-0.5">
      <h3 className="font-medium text-sm">Run your own bridge</h3>
      <p className="text-muted-foreground text-xs">
        Self-host a bridge for a network Beeper doesn't run for you.
      </p>
    </div>
  );
}

function AvailableBranch({
  accountId,
  availability,
  onBridgeAdded,
}: {
  accountId: string;
  availability: BbctlAvailabilityVm;
  onBridgeAdded: () => void;
}) {
  const networks = availability.networks;
  const [selected, setSelected] = useState<string>(() => networks[0]?.networkId ?? "");

  const isOpen = useBbctlStore((s) => s.isOpen);
  const storeAccountId = useBbctlStore((s) => s.accountId);
  const storeNetworkId = useBbctlStore((s) => s.selectedNetworkId);

  // If the store is open for THIS account but the selected network is no longer in
  // the availability set (e.g. the data changed under us), close it rather than leave
  // a stuck-open state with no valid sheet.
  const openForThisAccount = isOpen && storeAccountId === accountId;
  const selectedNetworkPresent =
    storeNetworkId !== null && networks.some((n) => n.networkId === storeNetworkId);
  useEffect(() => {
    if (openForThisAccount && !selectedNetworkPresent) {
      bbctlStore.getState().close();
    }
  }, [openForThisAccount, selectedNetworkPresent]);

  if (networks.length === 0) {
    return (
      <p className="text-muted-foreground text-sm">
        No self-hostable networks are available right now.
      </p>
    );
  }

  const runNetwork = networks.find((n) => n.networkId === storeNetworkId);
  const sheetOpen = openForThisAccount && selectedNetworkPresent && runNetwork !== undefined;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <Select value={selected} onValueChange={setSelected}>
          <SelectTrigger className="w-56" aria-label="Network to run">
            <SelectValue placeholder="Choose a network" />
          </SelectTrigger>
          <SelectContent>
            {networks.map((network) => (
              <SelectItem key={network.networkId} value={network.networkId}>
                {network.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          type="button"
          size="sm"
          disabled={selected === ""}
          onClick={() => bbctlStore.getState().open(accountId, selected)}
        >
          Run
        </Button>
      </div>

      {sheetOpen && runNetwork !== undefined && (
        <BbctlRunSheet
          accountId={accountId}
          networkId={runNetwork.networkId}
          networkName={runNetwork.name}
          open={sheetOpen}
          onOpenChange={(next) => {
            if (!next) {
              bbctlStore.getState().close();
            }
          }}
          onSuccess={onBridgeAdded}
        />
      )}
    </div>
  );
}

function UnavailableBranch({ availability }: { availability: BbctlAvailabilityVm }) {
  const { install } = availability;
  return (
    <div className="flex flex-col gap-2" data-slot="bbctl-install">
      <p className="text-muted-foreground text-sm">
        bbctl isn't installed. To run your own bridge:
      </p>
      {/* Key by index — the install steps may repeat prose. */}
      <ol className="flex list-decimal flex-col gap-1 pl-5 text-sm">
        {install.steps.map((step, index) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: steps may repeat; index is the stable key
          <li key={index} className="text-muted-foreground">
            {step}
          </li>
        ))}
      </ol>
      <a
        href={install.docsUrl}
        target="_blank"
        rel="noreferrer"
        className="text-sm underline underline-offset-2"
      >
        Beeper self-host docs
      </a>
    </div>
  );
}

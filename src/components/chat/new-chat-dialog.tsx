/**
 * The new-chat dialog (Story 6.6, FR-32).
 *
 * An always-mounted {@link Dialog} opened by ⌘N (via
 * {@link import("@/hooks/use-new-chat-shortcut").useNewChatShortcut}). The user picks
 * a Network + Account (defaulting to last used), enters an identifier (phone /
 * username / Matrix ID), and keeper resolves it through the bridge's provisioning
 * `resolve_identifier` ({@link resolveBridgeIdentifier}) with a visible resolving
 * state, then opens the resulting portal Chat with the composer focused.
 *
 * Resolve support is honest and data-driven ({@link bridgeResolveSupport}): a network
 * the bridge can't resolve is declared **unsupported upfront** (input disabled, "not
 * supported on {Network}" shown before any I/O), and an unresolvable identifier shows
 * an inline "Not found on {Network}" keeping the input for correction — never a late
 * failure, never a dismissed dialog.
 */
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useBridgeCatalog } from "@/hooks/use-bridge-catalog";
import type { BridgeNetworkVm, IpcError, ResolveSupportVm } from "@/lib/ipc/client";
import { bridgeResolveSupport, resolveBridgeIdentifier } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { composerStore } from "@/lib/stores/composer";
import { newChatStore, useNewChatStore } from "@/lib/stores/new-chat";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

export function NewChatDialog() {
  const isOpen = useNewChatStore((s) => s.isOpen);
  const close = () => newChatStore.getState().close();

  return (
    <Dialog open={isOpen} onOpenChange={(next) => !next && close()}>
      <DialogContent className="sm:max-w-md">{isOpen && <NewChatBody />}</DialogContent>
    </Dialog>
  );
}

/**
 * The dialog body, mounted only while open so its transient input/resolve state is
 * discarded on close (a fresh open starts clean).
 */
function NewChatBody() {
  const accounts = useAccountsStore((s) => s.accounts);
  const { catalog: rawCatalog } = useBridgeCatalog();
  const catalog = useMemo(() => rawCatalog ?? [], [rawCatalog]);
  const lastAccountId = useNewChatStore((s) => s.lastAccountId);
  const lastNetworkId = useNewChatStore((s) => s.lastNetworkId);

  // Default the pickers to the last-used selection, falling back to the first
  // available account / network.
  const defaultAccountId =
    accounts.find((a) => a.accountId === lastAccountId)?.accountId ?? accounts[0]?.accountId ?? "";
  const defaultNetworkId =
    catalog.find((n) => n.networkId === lastNetworkId)?.networkId ?? catalog[0]?.networkId ?? "";

  const [accountId, setAccountId] = useState(defaultAccountId);
  const [networkId, setNetworkId] = useState(defaultNetworkId);
  const [identifier, setIdentifier] = useState("");
  const [resolving, setResolving] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [support, setSupport] = useState<ResolveSupportVm | null>(null);

  // Adopt the catalog default once it loads (the first render had an empty catalog).
  useEffect(() => {
    if (networkId === "" && defaultNetworkId !== "") {
      setNetworkId(defaultNetworkId);
    }
  }, [networkId, defaultNetworkId]);

  // Adopt the account default once the accounts store hydrates (⌘N can open the
  // dialog before it has — without this the Account picker stays empty and Start
  // stays permanently disabled). Symmetric with the network default above.
  useEffect(() => {
    if (accountId === "" && defaultAccountId !== "") {
      setAccountId(defaultAccountId);
    }
  }, [accountId, defaultAccountId]);

  const network = useMemo<BridgeNetworkVm | undefined>(
    () => catalog.find((n) => n.networkId === networkId),
    [catalog, networkId],
  );
  const networkName = network?.name ?? networkId;

  // Fetch the data-driven resolve capability for the selected network (upfront gate,
  // before any resolve I/O). A stale-network guard drops a late resolve after the
  // network changes.
  useEffect(() => {
    if (networkId === "") {
      setSupport(null);
      return;
    }
    let cancelled = false;
    setSupport(null);
    setErrorMessage(null);
    bridgeResolveSupport(networkId)
      .then((vm) => {
        if (!cancelled) {
          setSupport(vm);
        }
      })
      .catch(() => {
        // Fail CLOSED: a capability read failure must not open the gate on a network
        // that could be unsupported. Synthesize an unsupported result so the dialog
        // points at the manual Bridge Bot escape hatch instead of attempting a resolve.
        if (!cancelled) {
          setSupport({
            networkId,
            supported: false,
            identifierHint:
              "Couldn't check whether this network supports new chats — open the Bridge Bot chat to start one.",
            placeholder: "",
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [networkId]);

  // Tri-state: `support === null` with a chosen network means the capability read is
  // still in flight (show a neutral checking state, Start disabled — never fail open).
  const supportLoading = networkId !== "" && support === null;
  const supported = support?.supported === true;
  const trimmed = identifier.trim();
  const canResolve =
    supported && trimmed !== "" && !resolving && accountId !== "" && networkId !== "";

  const resolve = async () => {
    if (!canResolve) {
      return;
    }
    setResolving(true);
    setErrorMessage(null);
    try {
      const { roomId } = await resolveBridgeIdentifier(accountId, networkId, trimmed);
      // Success: remember the selection, open the resolved chat, focus the composer,
      // and close the dialog.
      newChatStore.getState().rememberSelection(accountId, networkId);
      primaryViewStore.getState().setView("inbox");
      roomsStore.getState().selectRoom({ accountId, roomId });
      composerStore.getState().requestFocus();
      newChatStore.getState().close();
    } catch (raw) {
      // The dialog stays open and the identifier is retained for correction (FR-32) —
      // never a dismissal, never a cleared input. The bridge's own message rides in
      // the envelope for context.
      const message = (raw as IpcError)?.message ?? "";
      setErrorMessage(message);
    } finally {
      setResolving(false);
    }
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Start a new chat</DialogTitle>
        <DialogDescription>
          Pick a network and account, then enter a phone number, username, or Matrix ID.
        </DialogDescription>
      </DialogHeader>

      {accounts.length === 0 ? (
        <p className="text-muted-foreground text-sm" data-slot="new-chat-no-accounts">
          Add an account to start a new chat.
        </p>
      ) : (
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-chat-account">Account</Label>
            {/* Locked while a resolve is in flight so the result can't land on a
                network/account the user switched to mid-flight. */}
            <Select value={accountId} onValueChange={setAccountId} disabled={resolving}>
              <SelectTrigger id="new-chat-account" aria-label="Account">
                <SelectValue placeholder="Select an account" />
              </SelectTrigger>
              <SelectContent>
                {accounts.map((account) => (
                  <SelectItem key={account.accountId} value={account.accountId}>
                    {account.userId}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-chat-network">Network</Label>
            <Select value={networkId} onValueChange={setNetworkId} disabled={resolving}>
              <SelectTrigger id="new-chat-network" aria-label="Network">
                <SelectValue placeholder="Select a network" />
              </SelectTrigger>
              <SelectContent>
                {catalog.map((n) => (
                  <SelectItem key={n.networkId} value={n.networkId}>
                    {n.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {supportLoading ? (
            <p className="text-muted-foreground text-sm" data-slot="new-chat-checking">
              Checking whether {networkName} supports new chats…
            </p>
          ) : supported ? (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="new-chat-identifier">{support?.identifierHint ?? "Identifier"}</Label>
              <Input
                id="new-chat-identifier"
                aria-label="Identifier"
                value={identifier}
                placeholder={support?.placeholder ?? ""}
                disabled={resolving}
                onChange={(e) => {
                  setIdentifier(e.target.value);
                  if (errorMessage !== null) {
                    setErrorMessage(null);
                  }
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && canResolve) {
                    e.preventDefault();
                    void resolve();
                  }
                }}
              />
              {errorMessage !== null && (
                <p role="alert" className="text-destructive text-xs" data-slot="new-chat-error">
                  Not found on {networkName} — check the number or username.
                </p>
              )}
            </div>
          ) : (
            <p className="text-muted-foreground text-sm" data-slot="new-chat-unsupported">
              {support?.identifierHint ?? `Starting new chats isn't supported on ${networkName}`}
            </p>
          )}
        </div>
      )}

      <DialogFooter>
        <Button
          type="button"
          onClick={() => void resolve()}
          disabled={!canResolve}
          aria-label="Start chat"
        >
          {resolving ? "Resolving…" : "Start chat"}
        </Button>
      </DialogFooter>
    </>
  );
}

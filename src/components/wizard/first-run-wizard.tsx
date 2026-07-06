/**
 * First-Run Wizard (Story 6.8).
 *
 * A full-frame guided path (Welcome → Add Account → Bridge discovery + per-Bridge
 * login → Done) that **composes existing surfaces** rather than reimplementing any
 * of them: {@link LoginScreen} in `addMode`, the {@link BridgeCard} (which owns its
 * own risk-ack {@link AlertDialog} + {@link import("@/components/bridges/bridge-login-sheet").BridgeLoginSheet}),
 * and the `useBridgeCatalog`/`useBridgeDiscovery` hooks. It is driven by the
 * session-scoped {@link wizardStore} and is a *path, not a gate*: every step has a
 * Skip control and Esc asks once before leaving. Nothing here performs Matrix I/O
 * or reimplements login/discovery/QR/ack — Rust and the reused components own that.
 */
import { useEffect, useRef, useState } from "react";
import { LoginScreen } from "@/components/auth/login-screen";
import { BridgeCard } from "@/components/bridges/bridge-card";
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
import { Button } from "@/components/ui/button";
import { useBridgeCatalog } from "@/hooks/use-bridge-catalog";
import { useBridgeDiscovery } from "@/hooks/use-bridge-discovery";
import { COMPANION_STACK_DOCS_URL } from "@/lib/bridges";
import type { BridgeNetworkVm } from "@/lib/ipc/client";
import { accountsStore, useAccountsStore } from "@/lib/stores/accounts";
import { useWizardStore, type WizardStep } from "@/lib/stores/wizard";

/** The ordered steps rendered as progress dots. */
const STEPS: readonly WizardStep[] = ["welcome", "addAccount", "discovery", "done"];

/** The human label for each step, shown under the progress dots. */
const STEP_LABEL: Record<WizardStep, string> = {
  welcome: "Welcome",
  addAccount: "Add account",
  discovery: "Connect bridges",
  done: "Done",
};

/**
 * The full-frame first-run wizard. Renders the current `step` from
 * {@link wizardStore}; a single Esc-confirm {@link AlertDialog} guards leaving.
 */
export function FirstRunWizard() {
  const step = useWizardStore((s) => s.step);
  const finish = useWizardStore((s) => s.finish);
  // Whether the "Skip setup?" confirm is open. Esc opens it (asks once) rather
  // than exiting the wizard immediately.
  const [confirmOpen, setConfirmOpen] = useState(false);

  // Esc opens the confirm rather than leaving. Once the confirm is open, its own
  // AlertDialog owns Escape (closing the confirm), so this handler only fires for
  // the first press. It also stands down while a nested overlay is open — the
  // discovery step composes BridgeCard, which self-drives a bridge-login Sheet
  // (`role="dialog"`) and a risk-ack AlertDialog (`role="alertdialog"`); an Escape
  // meant to close one of those must reach it, not pop the wizard's skip-confirm.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape" || confirmOpen) {
        return;
      }
      // Let an open nested Radix overlay handle Escape first (it unmounts its
      // portal — with the roles below — while open).
      if (document.querySelector('[role="dialog"],[role="alertdialog"]') !== null) {
        return;
      }
      event.preventDefault();
      setConfirmOpen(true);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [confirmOpen]);

  const skip = () => setConfirmOpen(true);

  return (
    <section
      className="fixed inset-0 z-50 flex flex-col overflow-y-auto bg-background text-foreground"
      aria-label="First-run setup"
    >
      <ProgressDots step={step} />

      <div className="flex min-h-0 flex-1 flex-col">
        {step === "welcome" && <WelcomeStep />}
        {step === "addAccount" && <AddAccountStep />}
        {step === "discovery" && <DiscoveryStep />}
        {step === "done" && <DoneStep onFinish={finish} />}
      </div>

      {/* Persistent Skip control in the wizard chrome (not per-step): the
          Add-Account step composes LoginScreen, which owns a full-viewport
          `h-screen` layout, so an in-flow Skip would sit below the fold. A
          footer keeps Skip reachable on every non-terminal step. */}
      {step !== "done" && (
        <footer className="flex shrink-0 justify-center border-border border-t px-6 py-3">
          <Button type="button" variant="ghost" size="sm" onClick={skip}>
            Skip setup
          </Button>
        </footer>
      )}

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Skip setup?</AlertDialogTitle>
            <AlertDialogDescription>You can run it again from Settings.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep setting up</AlertDialogCancel>
            <AlertDialogAction onClick={finish}>Skip setup</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}

/** The step progress indicator: a dot per step plus the current step label. */
function ProgressDots({ step }: { step: WizardStep }) {
  const activeIndex = STEPS.indexOf(step);
  return (
    <header className="flex shrink-0 flex-col items-center gap-2 border-border border-b px-6 py-4">
      <div className="flex items-center gap-2" aria-hidden="true">
        {STEPS.map((s, i) => (
          <span
            key={s}
            data-slot="wizard-progress-dot"
            className={
              i <= activeIndex ? "size-2 rounded-full bg-primary" : "size-2 rounded-full bg-muted"
            }
          />
        ))}
      </div>
      <p className="text-muted-foreground text-sm">
        Step {activeIndex + 1} of {STEPS.length}: {STEP_LABEL[step]}
      </p>
    </header>
  );
}

/** Welcome: intro copy + Get started (Skip lives in the persistent wizard footer). */
function WelcomeStep() {
  const goTo = useWizardStore((s) => s.goTo);
  return (
    <div className="mx-auto flex w-full max-w-md flex-1 flex-col justify-center gap-6 p-6">
      <div className="flex flex-col gap-2 text-center">
        <h1 className="font-heading font-medium text-2xl">Welcome to keeper</h1>
        <p className="text-muted-foreground text-sm">
          Let's get you connected. First sign in to a Matrix account, then bring your other chat
          networks in through bridges. You can skip any step and finish later.
        </p>
      </div>
      <div className="flex flex-col gap-2">
        <Button type="button" onClick={() => goTo("addAccount")}>
          Get started
        </Button>
      </div>
    </div>
  );
}

/**
 * Add Account: mounts {@link LoginScreen} in `addMode`. On done we disambiguate
 * success from cancel by account-count growth against the baseline captured on
 * entry — in `addMode` `LoginScreen` calls `onDone` on *both* success (after a
 * synchronous `addAccount`) and cancel. Below it, an honest no-homeserver fork.
 */
function AddAccountStep() {
  const goTo = useWizardStore((s) => s.goTo);
  const setAccountId = useWizardStore((s) => s.setAccountId);
  // The account count on entry; growth ⇒ a real add, unchanged ⇒ a cancel.
  const baselineRef = useRef(accountsStore.getState().accounts.length);

  const handleAddDone = () => {
    const accounts = accountsStore.getState().accounts;
    if (accounts.length > baselineRef.current) {
      // Success: `addAccount` appends, so the newest account is last.
      const newest = accounts[accounts.length - 1];
      setAccountId(newest.accountId);
      goTo("discovery");
    } else {
      // Cancel: return to Welcome without advancing.
      goTo("welcome");
    }
  };

  // The no-homeserver fork renders as a slim banner ABOVE the login card: the
  // composed LoginScreen owns a full-viewport `h-screen` centered layout, so
  // anything placed after it would fall below the fold. A compact banner on top
  // stays visible without scrolling.
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <NoHomeserverFork />
      <LoginScreen addMode onDone={handleAddDone} />
    </div>
  );
}

/**
 * The honest no-homeserver fork — a slim banner at the top of the Add-Account
 * step. Links to the real companion-stack docs and points at the in-step Beeper
 * tab (rendered in the LoginScreen just below); no fabricated hosted sign-up
 * destination exists, so none is invented.
 */
function NoHomeserverFork() {
  return (
    <div className="mx-auto w-full max-w-md px-6 pt-6 text-sm">
      <div className="rounded-md border border-border p-3">
        <span className="font-medium">No homeserver yet? </span>
        <span className="text-muted-foreground">
          Sign in with the Beeper tab below to get started without running your own server, or{" "}
          <a
            href={COMPANION_STACK_DOCS_URL}
            target="_blank"
            rel="noreferrer"
            className="underline underline-offset-2"
          >
            set up a companion stack
          </a>{" "}
          to self-host.
        </span>
      </div>
    </div>
  );
}

/**
 * Discovery: reuse `useBridgeCatalog` + `useBridgeDiscovery` and render a
 * {@link BridgeCard} per discovered network (catalog-joined, same defensive skip
 * as `AccountBridges`). The card self-drives the ack gate + login Sheet. Resolves
 * the account to probe from the wizard's `accountId`, falling back to the first
 * signed-in account.
 */
function DiscoveryStep() {
  const goTo = useWizardStore((s) => s.goTo);
  const accountId = useWizardStore((s) => s.accountId);
  const accounts = useAccountsStore((s) => s.accounts);
  const resolvedAccountId = accountId ?? accounts[0]?.accountId ?? null;

  return (
    <div className="mx-auto flex w-full max-w-lg flex-1 flex-col gap-6 p-6">
      <div className="flex flex-col gap-2 text-center">
        <h1 className="font-heading font-medium text-2xl">Connect your networks</h1>
        <p className="text-muted-foreground text-sm">
          Connect a network to bring its chats into keeper. You can set these up later from the
          Bridges surface too.
        </p>
      </div>

      {resolvedAccountId === null ? (
        <p className="text-muted-foreground text-sm">Add an account first to set up bridges.</p>
      ) : (
        <DiscoveryList accountId={resolvedAccountId} />
      )}

      <div className="flex flex-col gap-2">
        <Button type="button" onClick={() => goTo("done")}>
          Continue
        </Button>
      </div>
    </div>
  );
}

/** The catalog entry for a network id, or `undefined` when uncatalogued. */
function catalogFor(catalog: BridgeNetworkVm[], networkId: string): BridgeNetworkVm | undefined {
  return catalog.find((n) => n.networkId === networkId);
}

/**
 * The discovered-bridges list for one account. Mirrors `AccountBridges` in
 * `bridges-pane.tsx`: catalog + discovery join, per-account loading, retriable
 * error, and the "No bridges found" empty state — all sourced from the reused
 * hooks so nothing is duplicated here.
 */
function DiscoveryList({ accountId }: { accountId: string }) {
  const { catalog, loading: catalogLoading, error: catalogError } = useBridgeCatalog();
  const { discovery, loading, error, retriable, retry } = useBridgeDiscovery(accountId);

  if (catalogError !== null) {
    return (
      <p role="alert" className="text-destructive text-sm">
        Bridges are unavailable right now: {catalogError}
      </p>
    );
  }
  if (error !== null) {
    return (
      <div role="alert" className="flex flex-col items-start gap-2 text-sm">
        <p className="text-destructive">Could not discover bridges: {error}</p>
        {retriable && (
          <Button type="button" size="sm" variant="outline" onClick={retry}>
            Retry
          </Button>
        )}
      </div>
    );
  }
  if (catalogLoading || catalog === null || loading || discovery === null) {
    return <p className="text-muted-foreground text-sm">Discovering bridges…</p>;
  }
  if (discovery.networks.length === 0) {
    return (
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
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {discovery.networks.map((discovered) => {
        const network = catalogFor(catalog, discovered.networkId);
        // Catalog-gated in the backend, but guard defensively: skip any network
        // the frontend catalog can't present.
        if (network === undefined) {
          return null;
        }
        return (
          <BridgeCard
            key={`${accountId}:${discovered.networkId}`}
            network={network}
            accountId={accountId}
            status={discovered.status}
          />
        );
      })}
    </div>
  );
}

/** Done: success copy + Enter keeper → `finish()`. */
function DoneStep({ onFinish }: { onFinish: () => void }) {
  return (
    <div className="mx-auto flex w-full max-w-md flex-1 flex-col justify-center gap-6 p-6 text-center">
      <div className="flex flex-col gap-2">
        <h1 className="font-heading font-medium text-2xl">You're all set</h1>
        <p className="text-muted-foreground text-sm">
          Your inbox is ready. You can add more accounts or set up more bridges any time from
          Settings.
        </p>
      </div>
      <Button type="button" onClick={onFinish}>
        Enter keeper
      </Button>
    </div>
  );
}

/**
 * The "Run your own bridge" run stepper Sheet (Story 6.7, FR-29, AD-16, UX-DR8).
 *
 * A `Sheet` over the Bridges surface that renders the log-free `bbctl` run state
 * machine driven by {@link useBbctlRun}: checking → registering → starting → running →
 * success / failure. Each {@link BbctlPhase} renders with the shared
 * {@link BBCTL_PHASE_LABEL} step word — recognized phase transitions only, never a raw
 * `bbctl` log line (no log viewer, v1.x). On the terminal `success` phase the Sheet
 * fires its success side-effect **at most once** (via a ref): a guarded discovery
 * refresh (a throwing refresh cannot strand the Sheet) then an auto-close. A `failure`
 * shows `bbctl`'s own message verbatim + Retry, retaining the selection.
 *
 * The bridge then surfaces through the existing discovery (6.2) + health (6.5)
 * machinery — this Sheet just asks the panel to refresh discovery on success.
 */
import { useEffect, useRef } from "react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useBbctlRun } from "@/hooks/use-bbctl-run";
import { BBCTL_PHASE_LABEL } from "@/lib/bridges";
import type { BbctlProgressVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** How long "Running ✓" shows before the Sheet auto-closes (ms). */
const SUCCESS_AUTO_ADVANCE_MS = 1500;

interface BbctlRunSheetProps {
  /** The account id the run is keyed to. */
  accountId: string;
  /** The network id being run. */
  networkId: string;
  /** The network's display name (for the Sheet title / instructions). */
  networkName: string;
  /** Whether the Sheet is open. */
  open: boolean;
  /** Called when the Sheet should close (Esc, backdrop, cancel, auto-advance). */
  onOpenChange: (open: boolean) => void;
  /** Refresh the account's bridge discovery so the new bridge card appears. */
  onSuccess: () => void;
}

export function BbctlRunSheet({
  accountId,
  networkId,
  networkName,
  open,
  onOpenChange,
  onSuccess,
}: BbctlRunSheetProps) {
  const { vm, start, cancel } = useBbctlRun(accountId, networkId, open);

  // Kick off the run when the Sheet opens.
  useEffect(() => {
    if (open) {
      start();
    }
  }, [open, start]);

  // Fire the success side-effect AT MOST ONCE per success via a ref, guarding the
  // refresh so a throwing refresh cannot strand the Sheet open.
  const successFiredRef = useRef(false);
  useEffect(() => {
    if (vm?.phase !== "success") {
      // Re-arm for the next run (a Retry that later succeeds must fire again).
      successFiredRef.current = false;
      return;
    }
    if (successFiredRef.current) {
      return;
    }
    successFiredRef.current = true;
    try {
      onSuccess();
    } catch (error) {
      console.error("bbctl run: discovery refresh failed", error);
    }
    const timer = setTimeout(() => onOpenChange(false), SUCCESS_AUTO_ADVANCE_MS);
    return () => clearTimeout(timer);
  }, [vm?.phase, onSuccess, onOpenChange]);

  const handleOpenChange = (next: boolean) => {
    if (!next) {
      cancel();
    }
    onOpenChange(next);
  };

  const stateWord = vm ? BBCTL_PHASE_LABEL[vm.phase] : BBCTL_PHASE_LABEL.checking;

  return (
    <Sheet open={open} onOpenChange={handleOpenChange}>
      <SheetContent className="flex flex-col gap-4">
        <SheetHeader>
          <SheetTitle>Run {networkName}</SheetTitle>
          <SheetDescription data-slot="bbctl-run-state-word">{stateWord}</SheetDescription>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
          <BbctlRunBody vm={vm} networkName={networkName} onRetry={start} />
        </div>
      </SheetContent>
    </Sheet>
  );
}

function BbctlRunBody({
  vm,
  networkName,
  onRetry,
}: {
  vm: BbctlProgressVm | null;
  networkName: string;
  onRetry: () => void;
}) {
  if (vm === null || vm.phase !== "failure") {
    // checking / registering / starting / running / success all render the progress
    // panel; only failure branches to the retry panel.
    if (vm?.phase === "success") {
      return <SuccessPanel networkName={networkName} />;
    }
    return <ProgressPanel vm={vm} />;
  }
  return <FailurePanel vm={vm} onRetry={onRetry} />;
}

function ProgressPanel({ vm }: { vm: BbctlProgressVm | null }) {
  // Log-free stepper: show the recognized phase LABEL only, never a raw bbctl line.
  const message = BBCTL_PHASE_LABEL[vm?.phase ?? "checking"];
  return (
    <div
      className="flex flex-col items-center gap-3 py-8 text-center"
      data-slot="bbctl-run-progress"
    >
      <span
        aria-hidden="true"
        className="size-6 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-foreground"
      />
      <p className="text-muted-foreground text-sm">{message}</p>
    </div>
  );
}

function SuccessPanel({ networkName }: { networkName: string }) {
  return (
    <div
      className="flex flex-col items-center gap-2 py-8 text-center"
      data-slot="bbctl-run-success"
    >
      <p className={cn("font-medium text-lg", "text-bridge-healthy")}>Running ✓</p>
      <p className="text-muted-foreground text-sm">{networkName} is running.</p>
    </div>
  );
}

function FailurePanel({ vm, onRetry }: { vm: BbctlProgressVm; onRetry: () => void }) {
  return (
    <div className="flex flex-col gap-4" data-slot="bbctl-run-failure">
      {/* bbctl's own error message, verbatim — keeper never rewrites it. */}
      <p className="text-bridge-disconnected text-sm" data-slot="bbctl-run-error">
        {vm.error ?? "The bridge could not be started."}
      </p>
      <Button type="button" variant="outline" onClick={onRetry}>
        Retry
      </Button>
    </div>
  );
}

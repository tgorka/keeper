/**
 * Interactive device self-verification modal (Story 3.2, FR-14, NFR-14, AD-1).
 *
 * An Element-X-style multi-step modal driven purely by the Rust-authoritative
 * {@link VerificationFlowVm} in the {@link verificationStore}. It renders each SDK
 * flow phase distinctly — waiting, comparing (emoji), confirmed, done, cancelled,
 * failed — using the SDK's own vocabulary; it never invents crypto UX and never
 * touches key material (all of that lives in Rust). Every action (start SAS,
 * "They match" / "They don't match", cancel) is a one-shot IPC call keyed by the
 * flow's `flowId`.
 *
 * Fully keyboard-operable (NFR-14): all controls are focusable buttons and the
 * shadcn `Dialog` (Radix) handles focus trapping and `Esc`. Closing the modal
 * (Esc / the close button / clicking away) cancels the active flow via the store's
 * `close()`.
 */
import { useEffect, useRef } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  verificationConfirm,
  verificationMismatch,
  verificationStart,
  verificationStartSas,
} from "@/lib/ipc/client";
import {
  useActiveVerificationAccountId,
  useVerificationFlow,
  useVerificationModalOpen,
  verificationStore,
} from "@/lib/stores/verification";

/** The honest, human copy for the "waiting for your other device" state. */
export const VERIFY_WAITING_TEXT = "Waiting for your other device…";

export function DeviceVerificationDialog() {
  const open = useVerificationModalOpen();
  const flow = useVerificationFlow();

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          verificationStore.getState().close();
        }
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Verify this device</DialogTitle>
          <DialogDescription>
            Compare with your other signed-in session (for example Element) to unlock encrypted
            history.
          </DialogDescription>
        </DialogHeader>
        <VerificationBody flow={flow} />
      </DialogContent>
    </Dialog>
  );
}

/** The phase-specific body. `flow === null` means the request is being created. */
function VerificationBody({ flow }: { flow: ReturnType<typeof useVerificationFlow> }) {
  const accountId = useActiveVerificationAccountId();
  // Guard so a keeper-initiated verification is requested at most once per opened
  // modal, even across StrictMode double-invokes or a transient null flow.
  const startedRef = useRef<string | null>(null);

  // When the modal opens for a keeper-initiated verification (no flow yet), kick
  // off the request once. An incoming request already carries a flow before the
  // first render, so this no-ops for that path.
  useEffect(() => {
    if (flow === null && accountId && startedRef.current !== accountId) {
      startedRef.current = accountId;
      void verificationStart(accountId).catch(() => {
        // Surface an honest failure instead of hanging on "Waiting…" (e.g. no other
        // session signed in, or the crypto identity isn't ready yet).
        if (verificationStore.getState().activeAccountId === accountId) {
          verificationStore.getState().setFlow({
            flowId: "",
            phase: "failed",
            emojis: null,
            qrCodeSvg: null,
            reason: "Couldn't start verification. Make sure another session is signed in.",
          });
        }
      });
    }
    if (accountId === null) {
      startedRef.current = null;
    }
  }, [flow, accountId]);

  if (flow === null) {
    return <p className="text-sm text-muted-foreground">{VERIFY_WAITING_TEXT}</p>;
  }

  switch (flow.phase) {
    case "requested":
    case "ready":
      return <WaitingOrReady flow={flow} accountId={accountId} />;
    case "comparing":
      return <Comparing flow={flow} accountId={accountId} />;
    case "confirmed":
      return <p className="text-sm text-muted-foreground">{VERIFY_WAITING_TEXT}</p>;
    case "done":
      return (
        <p className="text-sm text-emerald-600 dark:text-emerald-400">
          This device is now verified.
        </p>
      );
    case "cancelled":
      return <p className="text-sm text-muted-foreground">Verification cancelled.</p>;
    case "failed":
      return (
        <p className="text-sm text-held">
          Verification failed{flow.reason ? `: ${flow.reason}` : "."}
        </p>
      );
    default:
      return null;
  }
}

/** The waiting / ready phase: show keeper's QR (if any) and offer emoji SAS. */
function WaitingOrReady({
  flow,
  accountId,
}: {
  flow: NonNullable<ReturnType<typeof useVerificationFlow>>;
  accountId: string | null;
}) {
  return (
    <div className="flex flex-col items-center gap-4 text-sm">
      <p className="text-muted-foreground">{VERIFY_WAITING_TEXT}</p>
      {flow.qrCodeSvg ? (
        <img
          src={`data:image/svg+xml,${encodeURIComponent(flow.qrCodeSvg)}`}
          alt="Scan this QR code from your other device"
          className="size-40 rounded-md bg-white p-2"
        />
      ) : null}
      <DialogFooter className="w-full">
        <button
          type="button"
          className="inline-flex h-8 items-center justify-center rounded-md border border-border bg-background px-3 text-xs hover:bg-muted"
          disabled={!accountId}
          onClick={() => {
            if (accountId) {
              void verificationStartSas(accountId, flow.flowId).catch(() => {
                // Surface a rejected SAS start as an honest failure instead of
                // leaving the modal stuck on the "waiting" screen with a dead
                // button and no feedback.
                if (verificationStore.getState().activeAccountId === accountId) {
                  verificationStore.getState().setFlow({
                    flowId: flow.flowId,
                    phase: "failed",
                    emojis: null,
                    qrCodeSvg: null,
                    reason: "Couldn't start emoji verification. Try again.",
                  });
                }
              });
            }
          }}
        >
          Verify with emoji
        </button>
      </DialogFooter>
    </div>
  );
}

/** The comparing phase: show the 7 SAS emoji and the match / no-match buttons. */
function Comparing({
  flow,
  accountId,
}: {
  flow: NonNullable<ReturnType<typeof useVerificationFlow>>;
  accountId: string | null;
}) {
  const emojis = flow.emojis ?? [];

  // Before keys are exchanged the phase is "comparing" but no emoji are present
  // yet — keep the honest waiting copy until they arrive.
  if (emojis.length === 0) {
    return <p className="text-sm text-muted-foreground">{VERIFY_WAITING_TEXT}</p>;
  }

  return (
    <div className="flex flex-col gap-4 text-sm">
      <p className="text-muted-foreground">
        Confirm the same emoji appear in the same order on your other device.
      </p>
      <ul className="grid grid-cols-4 justify-items-center gap-3 sm:grid-cols-7">
        {emojis.map((emoji, index) => (
          <li
            // The SAS string is a fixed, ordered length-7 list where position is
            // meaningful and emoji can legitimately repeat, so the index is part of
            // the stable identity; the list only changes on a full flow reset.
            // biome-ignore lint/suspicious/noArrayIndexKey: positional SAS emoji identity
            key={`${emoji.symbol}-${index}`}
            className="flex flex-col items-center gap-1 text-center"
          >
            <span className="text-2xl" aria-hidden="true">
              {emoji.symbol}
            </span>
            <span className="font-mono text-[10px] text-muted-foreground">{emoji.name}</span>
          </li>
        ))}
      </ul>
      <DialogFooter>
        <button
          type="button"
          className="inline-flex h-8 items-center justify-center rounded-md border border-border bg-background px-3 text-xs hover:bg-muted"
          disabled={!accountId}
          onClick={() => {
            if (accountId) {
              void verificationMismatch(accountId, flow.flowId).catch(() => {});
            }
          }}
        >
          They don't match
        </button>
        <button
          type="button"
          className="inline-flex h-8 items-center justify-center rounded-md bg-primary px-3 text-primary-foreground text-xs hover:bg-primary/80"
          disabled={!accountId}
          onClick={() => {
            if (accountId) {
              void verificationConfirm(accountId, flow.flowId).catch(() => {});
            }
          }}
        >
          They match
        </button>
      </DialogFooter>
    </div>
  );
}

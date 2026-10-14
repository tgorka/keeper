import { useCallback, useEffect, useState } from "react";
import { NO_BACKGROUND_SYNC_SENTENCE } from "@/components/settings/no-background-sync-disclosure";
import { PushGatewayField } from "@/components/settings/push-gateway-field";
import { Button } from "@/components/ui/button";
import { logFailure } from "@/lib/failure";
import {
  listenUnifiedpushStatus,
  type UnifiedPushVm,
  unifiedpushConfirm,
  unifiedpushStatus,
} from "@/lib/ipc/client";
import { useFailureNotice } from "@/lib/stores/failure-notices";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The heading of Android's push status (98.4). */
export const UNIFIEDPUSH_LABEL = "Push";

/** What shows when confirming a distributor did not reach Rust. */
const UNIFIEDPUSH_CONFIRM_FAILED = "Could not confirm the distributor.";

/**
 * Settings → Notifications' push block on a phone or a tablet. Which push this build has
 * is Rust's answer (`unifiedpush_status`), never a sniff: Android answers where its
 * UnifiedPush push stands and what it brings, both in Rust's sentences (98.4), and shows
 * no gateway field — its gateway is found on the distributor's own server, never typed.
 * While no distributor was confirmed on this device, Rust offers each installed one by
 * name and package and nothing registers until the person confirms one here
 * (`unifiedpush_confirm`, R98UP1-02). The status is read again when Rust says it changed
 * (`unifiedpush://status`) and when keeper comes back to the front. Every other build
 * answers `null`, and the iPhone's typed gateway and its lifecycle sentence show (98.1).
 * Nothing renders until the answer arrives, so neither platform flashes the other's copy.
 */
export function PhonePush({ open }: { open: boolean }) {
  /** `undefined` = still asking; `null` = not Android. */
  const [android, setAndroid] = useState<UnifiedPushVm | null | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  // A registration Rust could not make in the background (R381), said under
  // the push setting as well as in the toast.
  const background = useFailureNotice("push");

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    const read = () => {
      void unifiedpushStatus()
        .then((status) => {
          if (!cancelled) {
            setAndroid(status ?? null);
          }
        })
        .catch((raw: unknown) => {
          logFailure("push.unifiedpushStatus")(raw);
          if (!cancelled) {
            setAndroid((held) => held ?? null);
          }
        });
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") {
        read();
      }
    };
    read();
    document.addEventListener("visibilitychange", onVisible);
    let unlisten: (() => void) | undefined;
    void listenUnifiedpushStatus(read)
      .then((stop) => {
        if (cancelled) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      // The visibility re-read still keeps the status current without it.
      .catch(logFailure("push.listenStatus"));
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", onVisible);
      unlisten?.();
    };
  }, [open]);

  const confirm = useCallback((pkg: string) => {
    setBusy(true);
    setFailure(null);
    void unifiedpushConfirm(pkg)
      .then((status) => {
        if (status) {
          setAndroid(status);
        }
      })
      .catch((error: unknown) => {
        logFailure("push.unifiedpushConfirm")(error);
        setFailure(syncErrorMessage(error, UNIFIEDPUSH_CONFIRM_FAILED));
      })
      .finally(() => {
        setBusy(false);
      });
  }, []);

  if (android === undefined) {
    return null;
  }
  if (android === null) {
    return (
      <>
        <PushGatewayField open={open} />
        {background !== null && (
          <p role="alert" className="text-destructive">
            {background}
          </p>
        )}
        <p className="text-muted-foreground">{NO_BACKGROUND_SYNC_SENTENCE}</p>
      </>
    );
  }
  return (
    <section aria-label={UNIFIEDPUSH_LABEL} className="mt-1 flex min-w-0 flex-col gap-2">
      <h4 className="font-medium">{UNIFIEDPUSH_LABEL}</h4>
      <p role="status">{android.status}</p>
      {android.offers.length > 0 && (
        <div className="flex min-w-0 flex-col items-start gap-2">
          {android.offers.map((offer) => (
            <Button
              key={offer.package}
              variant="outline"
              size="sm"
              className="h-auto max-w-full whitespace-normal text-left"
              disabled={busy}
              onClick={() => confirm(offer.package)}
            >
              {offer.confirm}
            </Button>
          ))}
        </div>
      )}
      {failure && <p role="alert">{failure}</p>}
      {background !== null && (
        <p role="alert" className="text-destructive">
          {background}
        </p>
      )}
      <p className="text-muted-foreground">{android.note}</p>
    </section>
  );
}

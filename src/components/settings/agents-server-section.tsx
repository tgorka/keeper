/**
 * Settings › Agents › Server (UX-DR-E106-1): the agents server's address, each database's
 * status line, this device and its pairing, *Add a device* (a code and its QR) and the
 * person's *Devices*, each with *Remove*, and the confirmation of a pairing link a scanned QR
 * opened.
 *
 * Sign-in enrolment (109.6): a person whose account keeper can sign in with gets no code field
 * and no *Add a device* — the device connects by signing in — and sees *New devices*, the
 * server's notices of their other devices signing in or pairing (S-ID-8). The code field stays
 * for anyone keeper cannot sign in for (no account, a server without sign-in, a refusal).
 *
 * Every sentence but the copy is Rust's (`HqStatusVm`, the commands' refusals): whether an
 * address is one keeper contacts, which databases this device holds and how each stands,
 * whether the device was removed, and the devices — which follow the server's replicated
 * directory while this is open. The token never reaches the webview. On every tier: a phone
 * pairs and syncs the same way.
 */
import { useCallback, useEffect, useId, useState } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { logFailure } from "@/lib/failure";
import {
  type HqEnrolmentVm,
  type HqPairCodeVm,
  type HqStatusVm,
  hqDeviceRevoke,
  hqEnrolments,
  hqPairOfferAccept,
  hqPairOfferDismiss,
  hqPairRedeem,
  hqPairStart,
  hqServerForget,
  hqServerSet,
  hqStatusSubscribe,
  hqStatusUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const AGENTS_SERVER_TITLE = "Agents server";
export const AGENTS_SERVER_NOTE =
  "The server your agents' conversations sync through. keeper has none of its own: set the " +
  "address of the one you run. Signed in to your account, this device connects by itself; " +
  "without a sign-in, pair it with a code from a device that is connected.";
export const AGENTS_SERVER_LABEL = "Server address";
export const AGENTS_SERVER_SAVE = "Save";
export const PAIR_CODE_LABEL = "Pairing code";
export const PAIR_REDEEM = "Pair";
export const ADD_DEVICE = "Add a device";
export const ADD_DEVICE_NOTE =
  "Type this code on the new device under Agents server, or scan it. It works once, for ten minutes.";
export const COPY_CODE = "Copy code";
export const DEVICES_TITLE = "Devices";
export const THIS_DEVICE = "This device";
export const REMOVE_DEVICE = "Remove";
export const REMOVED_DEVICE = "Removed";
export const FORGET_SERVER = "Remove this server's copies";
export const OFFER_PAIR = "Pair this device";
export const OFFER_DISMISS = "Not now";
export const NEW_DEVICES_TITLE = "New devices";
const PLACEHOLDER = "https://agents.example.org";
const READ_FAILED = "Could not read the agents server's status.";
const QR_ALT = "QR code of the pairing code";

function failed(scope: string, fallback: string) {
  return (raw: unknown) => {
    logFailure(scope)(raw);
    return syncErrorMessage(raw, fallback);
  };
}

export function AgentsServerSection({ open }: { open: boolean }) {
  const id = useId();
  const [status, setStatus] = useState<HqStatusVm | undefined>(undefined);
  const [draft, setDraft] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [alert, setAlert] = useState<string | null>(null);
  const [pairCode, setPairCode] = useState<HqPairCodeVm | null>(null);
  const [notices, setNotices] = useState<HqEnrolmentVm[]>([]);

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    let subscription: string | null = null;
    let first = true;
    void hqStatusSubscribe((next) => {
      if (cancelled) return;
      setStatus(next);
      if (first) {
        setDraft(next.server ?? "");
        first = false;
      }
    })
      .then((sid) => {
        if (cancelled) void hqStatusUnsubscribe(sid);
        else subscription = sid;
      })
      .catch((raw: unknown) => {
        if (!cancelled) setAlert(failed("hq.statusSubscribe", READ_FAILED)(raw));
      });
    return () => {
      cancelled = true;
      if (subscription !== null) void hqStatusUnsubscribe(subscription);
    };
  }, [open]);

  const paired = status?.phase === "paired";
  const signIn = status?.signIn.state ?? "unavailable";
  // A code is the way in only for someone keeper cannot sign in for.
  const byCode = signIn === "unavailable" || signIn === "refused";
  // A new device in the directory is a new notice: read them again when the list moves.
  const devicesKey = status?.devices.map((device) => device.clientId).join(",") ?? "";

  // biome-ignore lint/correctness/useExhaustiveDependencies: devicesKey is the request to read the notices again
  useEffect(() => {
    if (!open || !paired) {
      setNotices([]);
      return;
    }
    let cancelled = false;
    void hqEnrolments()
      .then((next) => {
        if (!cancelled) setNotices(next);
      })
      .catch(logFailure("hq.enrolments"));
    return () => {
      cancelled = true;
    };
  }, [open, paired, devicesKey]);

  const run = useCallback(<T,>(scope: string, call: () => Promise<T>, then: (value: T) => void) => {
    setBusy(true);
    setAlert(null);
    void call()
      .then(then)
      .catch((raw: unknown) => setAlert(failed(scope, "That did not work.")(raw)))
      .finally(() => setBusy(false));
  }, []);

  if (status === undefined) {
    return alert === null ? null : (
      <p role="alert" className="text-destructive text-sm">
        {alert}
      </p>
    );
  }

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <h3 id={`${id}-title`} className="font-medium">
          {AGENTS_SERVER_TITLE}
        </h3>
        <FileControlled settingKey="agents.server_url" />
      </div>
      <p className="text-muted-foreground text-xs">{AGENTS_SERVER_NOTE}</p>

      <div className="flex min-w-0 flex-col gap-2">
        <Label htmlFor={`${id}-server`}>{AGENTS_SERVER_LABEL}</Label>
        <form
          className="flex min-w-0 items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            run(
              "hq.serverSet",
              () => hqServerSet(draft),
              (next) => {
                setStatus(next);
                setDraft(next.server ?? "");
              },
            );
          }}
        >
          <Input
            id={`${id}-server`}
            inputMode="url"
            value={draft}
            placeholder={PLACEHOLDER}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            className="min-w-0"
          />
          <Button
            type="submit"
            variant="outline"
            size="sm"
            disabled={busy || draft.trim() === (status.server ?? "")}
          >
            {AGENTS_SERVER_SAVE}
          </Button>
        </form>
        <p role="status" className="[overflow-wrap:anywhere]">
          {status.sentence}
        </p>
        {status.signIn.sentence !== null && (
          <p
            data-sign-in={status.signIn.state}
            className="text-muted-foreground [overflow-wrap:anywhere]"
          >
            {status.signIn.sentence}
          </p>
        )}
      </div>

      {status.databases.length > 0 && (
        <ul aria-label="Databases" className="flex min-w-0 flex-col gap-1">
          {status.databases.map((db) => (
            <li
              key={db.name}
              aria-label={db.name}
              data-state={db.state}
              className="flex min-w-0 flex-wrap gap-x-2"
            >
              <span className="font-medium">{db.name}</span>
              <span className="text-muted-foreground [overflow-wrap:anywhere]">{db.sentence}</span>
            </li>
          ))}
        </ul>
      )}

      {status.offer !== null && (
        <fieldset aria-label={OFFER_PAIR} className="flex min-w-0 flex-col gap-2">
          <p className="[overflow-wrap:anywhere]">
            Pair this device with {status.offer}? It will sync your agents&apos; conversations with
            that server.
          </p>
          <div className="flex gap-2">
            <Button
              type="button"
              size="sm"
              disabled={busy}
              onClick={() => run("hq.pairOfferAccept", hqPairOfferAccept, setStatus)}
            >
              {OFFER_PAIR}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => run("hq.pairOfferDismiss", hqPairOfferDismiss, () => {})}
            >
              {OFFER_DISMISS}
            </Button>
          </div>
        </fieldset>
      )}

      {(status.phase === "notPaired" || status.phase === "noServer") && byCode && (
        <form
          className="flex min-w-0 flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            run(
              "hq.pairRedeem",
              () => hqPairRedeem(code),
              (next) => {
                setStatus(next);
                setCode("");
              },
            );
          }}
        >
          <Label htmlFor={`${id}-code`}>{PAIR_CODE_LABEL}</Label>
          <div className="flex min-w-0 items-center gap-2">
            <Input
              id={`${id}-code`}
              value={code}
              autoComplete="off"
              spellCheck={false}
              disabled={busy}
              onChange={(event) => setCode(event.target.value)}
              className="min-w-0 font-mono"
            />
            <Button type="submit" variant="outline" size="sm" disabled={busy || code.trim() === ""}>
              {PAIR_REDEEM}
            </Button>
          </div>
        </form>
      )}

      {status.device !== null && (
        <p>
          <span className="text-muted-foreground">{THIS_DEVICE}: </span>
          {status.device.name}
        </p>
      )}

      {paired && byCode && (
        <div className="flex min-w-0 flex-col gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="self-start"
            disabled={busy}
            onClick={() => run("hq.pairStart", hqPairStart, setPairCode)}
          >
            {ADD_DEVICE}
          </Button>
          {pairCode !== null && <PairCode code={pairCode} />}
        </div>
      )}

      {status.devices.length > 0 && (
        <div className="flex min-w-0 flex-col gap-2">
          <h4 className="font-medium">{DEVICES_TITLE}</h4>
          <ul aria-label={DEVICES_TITLE} className="flex min-w-0 flex-col gap-1">
            {status.devices.map((device) => (
              <li
                key={device.clientId}
                aria-label={device.name}
                className="flex min-w-0 items-center justify-between gap-2"
              >
                <span className="min-w-0 [overflow-wrap:anywhere]">
                  {device.name}
                  {device.thisDevice && (
                    <span className="text-muted-foreground"> · {THIS_DEVICE}</span>
                  )}
                  {device.addedAtMs !== null && (
                    <span className="text-muted-foreground">
                      {" "}
                      · {new Date(device.addedAtMs).toLocaleDateString()}
                    </span>
                  )}
                </span>
                {device.revoked ? (
                  <span className="text-muted-foreground">{REMOVED_DEVICE}</span>
                ) : (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    aria-label={`${REMOVE_DEVICE} ${device.name}`}
                    onClick={() =>
                      run("hq.deviceRevoke", () => hqDeviceRevoke(device.clientId), setStatus)
                    }
                  >
                    {REMOVE_DEVICE}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {notices.length > 0 && (
        <div className="flex min-w-0 flex-col gap-2">
          <h4 className="font-medium">{NEW_DEVICES_TITLE}</h4>
          <ul aria-label={NEW_DEVICES_TITLE} className="flex min-w-0 flex-col gap-1">
            {notices.map((notice) => (
              <li
                key={`${notice.clientId}-${notice.atMs}`}
                className="min-w-0 [overflow-wrap:anywhere]"
              >
                {notice.sentence}
                <span className="text-muted-foreground">
                  {" "}
                  · {new Date(notice.atMs).toLocaleString()}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {status.phase === "removed" && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="self-start"
          disabled={busy}
          onClick={() => run("hq.serverForget", hqServerForget, setStatus)}
        >
          {FORGET_SERVER}
        </Button>
      )}

      {alert !== null && (
        <p role="alert" className="text-destructive [overflow-wrap:anywhere]">
          {alert}
        </p>
      )}
    </section>
  );
}

/** A pairing code: Rust's QR on a white card, the code, and Copy. */
function PairCode({ code }: { code: HqPairCodeVm }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex min-w-0 flex-col items-start gap-2">
      <p className="text-muted-foreground text-xs">{ADD_DEVICE_NOTE}</p>
      {code.qrSvg !== null && (
        <div className="rounded-lg bg-white p-4">
          <img
            src={`data:image/svg+xml,${encodeURIComponent(code.qrSvg)}`}
            alt={QR_ALT}
            width={192}
            height={192}
            className="size-48"
          />
        </div>
      )}
      <p className="select-all rounded-md border border-border p-2 font-mono text-sm">
        {code.code}
      </p>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => {
          const clipboard: Clipboard | undefined = navigator.clipboard;
          void clipboard?.writeText(code.code).then(() => setCopied(true));
        }}
      >
        {copied ? "Copied" : COPY_CODE}
      </Button>
    </div>
  );
}

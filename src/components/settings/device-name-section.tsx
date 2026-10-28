/**
 * Settings › Account & device › This device's name (R-HQ-94).
 *
 * The name this device goes by: the host the agents server records for it, and the default of its
 * name in the account. Prefilled with the effective name, it says where that name comes from — the
 * system, or you — and Reset goes back to the system's. Rust decides all of it: the precedence, the
 * server's host rule and every refusal sentence (`keeper_core::device_name`), and what the agents
 * server does about a new name (`Hq::device_renamed`), whose sign-in line shows here once the name
 * has changed.
 */
import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { logFailure } from "@/lib/failure";
import {
  accountSignIn,
  type DeviceNameVm,
  deviceNameGet,
  deviceNameReset,
  deviceNameSet,
  type HqStatusVm,
  hqStatusSubscribe,
  hqStatusUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const DEVICE_NAME_TITLE = "This device's name";
export const DEVICE_NAME_NOTE =
  "The name your agents server, your account and your commits know this device by. On a phone the system " +
  "often says only “iPhone”: give it a name of its own here.";
export const DEVICE_NAME_SAVE = "Save";
export const DEVICE_NAME_RESET = "Reset";
export const DEVICE_NAME_SIGN_IN = "Sign in again";
const READ_FAILED = "Could not read this device's name.";

export function DeviceNameSection({ open }: { open: boolean }) {
  const id = useId();
  const [vm, setVm] = useState<DeviceNameVm | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [alert, setAlert] = useState<string | null>(null);
  const [status, setStatus] = useState<HqStatusVm | null>(null);
  /** Changed here since Settings opened: the agents server's answer to it is worth a line. */
  const [changed, setChanged] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    let live = true;
    void deviceNameGet()
      .then((read) => {
        if (!live) return;
        setVm(read);
        setDraft(read.name);
      })
      .catch((raw: unknown) => {
        logFailure("deviceName.get")(raw);
        if (live) setAlert(READ_FAILED);
      });
    let subscription: string | null = null;
    void hqStatusSubscribe((next) => {
      if (live) setStatus(next);
    })
      .then((sid) => {
        if (live) subscription = sid;
        else void hqStatusUnsubscribe(sid);
      })
      .catch(logFailure("deviceName.hqStatus"));
    return () => {
      live = false;
      if (subscription !== null) void hqStatusUnsubscribe(subscription);
    };
  }, [open]);

  const apply = (call: () => Promise<DeviceNameVm>, scope: string) => {
    setBusy(true);
    setAlert(null);
    void call()
      .then((next) => {
        setVm(next);
        // What was STORED, not what was typed: Rust trims.
        setDraft(next.name);
        setChanged(true);
      })
      .catch((raw: unknown) => {
        logFailure(scope)(raw);
        setAlert(syncErrorMessage(raw));
      })
      .finally(() => setBusy(false));
  };

  if (vm === null) {
    return alert === null ? null : (
      <p role="alert" className="text-destructive text-sm">
        {alert}
      </p>
    );
  }

  const signIn = status?.signIn ?? null;
  const renamed = signIn?.state === "renamed";
  const agentsLine = signIn !== null && signIn.sentence !== null && (renamed || changed);

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        {DEVICE_NAME_TITLE}
      </h3>
      <p className="text-muted-foreground text-xs">{DEVICE_NAME_NOTE}</p>
      <form
        className="flex min-w-0 flex-wrap items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          apply(() => deviceNameSet(draft), "deviceName.set");
        }}
      >
        <Input
          aria-labelledby={`${id}-title`}
          aria-describedby={`${id}-source`}
          className="min-w-0 flex-1 basis-40"
          value={draft}
          disabled={busy}
          onChange={(event) => {
            setAlert(null);
            setDraft(event.target.value);
          }}
        />
        <div className="flex gap-2">
          <Button
            type="submit"
            variant="outline"
            size="sm"
            disabled={busy || draft.trim() === vm.name}
          >
            {DEVICE_NAME_SAVE}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={busy || vm.source === "system"}
            onClick={() => apply(deviceNameReset, "deviceName.reset")}
          >
            {DEVICE_NAME_RESET}
          </Button>
        </div>
      </form>
      <p
        id={`${id}-source`}
        data-source={vm.source}
        className="text-muted-foreground text-xs [overflow-wrap:anywhere]"
      >
        {vm.sentence}
      </p>
      {alert !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {alert}
        </p>
      )}
      {agentsLine && (
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <p
            role="status"
            data-sign-in={signIn.state}
            className="min-w-0 text-xs [overflow-wrap:anywhere]"
          >
            {signIn.sentence}
          </p>
          {renamed && (
            <Button
              type="button"
              variant="outline"
              size="xs"
              onClick={() => {
                void accountSignIn().catch(logFailure("deviceName.signIn"));
              }}
            >
              {DEVICE_NAME_SIGN_IN}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}

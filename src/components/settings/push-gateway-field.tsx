import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { logFailure } from "@/lib/failure";
import { type PushGatewayVm, pushGatewayGet, pushGatewaySet } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The field's label (98.1, Q4). */
export const PUSH_GATEWAY_LABEL = "Push gateway";

/**
 * What the field is for and who contacts the address (98.1, Q4/Q6; D-1, D-35; R331; 106.6,
 * 108.2): the gateway is the person's; keeper hands it to the agents server as this device's
 * push target, and the agents server sends it ids only — an approval waiting for the person
 * and their own agent's answer, read on the phone; keeper never contacts it, and no
 * homeserver is given it any more. It needs a build signed by the person's paid Apple team.
 * Sentence case, no exclamation marks.
 */
export const PUSH_GATEWAY_NOTE =
  "The address of a Matrix push gateway you run. keeper gives it to your agents server as " +
  "this phone's push address, so your agents' approval requests and your own agent's " +
  "answers reach this phone while keeper is closed, and nothing else shows. The agents " +
  "server sends the gateway their ids, never their words, and keeper never contacts it. " +
  "Push needs a build signed by your own paid Apple team. Leave it empty for no push.";

const PUSH_GATEWAY_PLACEHOLDER = "https://push.example.org";
const PUSH_GATEWAY_SAVE = "Save";
const PUSH_GATEWAY_READ_FAILED = "Could not read the push gateway.";
const PUSH_GATEWAY_WRITE_FAILED = "Could not save the push gateway.";

/**
 * Settings → Notifications on the phone (98.1, Q4): `push.gateway_url`, typed and saved.
 * What is stored and whether keeper will register it are Rust's answers
 * (`push_gateway_get` / `push_gateway_set`): a refused address is shown in Rust's
 * sentence and nothing is written; a stored value keeper will not register — one a synced
 * `settings.toml` carried — is shown with its sentence beside it, and Save stays open so it
 * can be replaced or cleared. No layer file sets this key (R329). Saving puts back what
 * Rust stored, so a bare host comes back with the push path filled in.
 */
export function PushGatewayField({ open }: { open: boolean }) {
  const inputId = useId();
  /** `undefined` = still loading; otherwise what Rust holds. */
  const [held, setHeld] = useState<PushGatewayVm | undefined>(undefined);
  const [draft, setDraft] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    setHeld(undefined);
    setRefusal(null);
    let cancelled = false;
    void pushGatewayGet()
      .then((vm) => {
        if (!cancelled) {
          setHeld(vm);
          setDraft(vm.url ?? "");
        }
      })
      .catch((raw: unknown) => {
        logFailure("push.gatewayGet")(raw);
        if (!cancelled) {
          setHeld({ url: null, refusal: null });
          setDraft("");
          setRefusal(PUSH_GATEWAY_READ_FAILED);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const save = () => {
    setBusy(true);
    void pushGatewaySet(draft)
      .then((vm) => {
        setHeld(vm);
        setDraft(vm.url ?? "");
        setRefusal(null);
      })
      .catch((raw: unknown) => {
        logFailure("push.gatewaySet")(raw);
        setRefusal(syncErrorMessage(raw, PUSH_GATEWAY_WRITE_FAILED));
      })
      .finally(() => setBusy(false));
  };

  return (
    <div className="mt-1 flex flex-col gap-2">
      <Label htmlFor={inputId}>{PUSH_GATEWAY_LABEL}</Label>
      <form
        className="flex min-w-0 items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          save();
        }}
      >
        <Input
          id={inputId}
          inputMode="url"
          value={draft}
          placeholder={PUSH_GATEWAY_PLACEHOLDER}
          disabled={busy || held === undefined}
          onChange={(event) => setDraft(event.target.value)}
          className="min-w-0"
        />
        <Button
          type="submit"
          variant="outline"
          size="sm"
          disabled={
            busy ||
            held === undefined ||
            (held.refusal === null && draft.trim() === (held.url ?? ""))
          }
        >
          {PUSH_GATEWAY_SAVE}
        </Button>
      </form>
      <p className="text-muted-foreground">{PUSH_GATEWAY_NOTE}</p>
      {held !== undefined && held.refusal !== null && refusal === null && (
        <p role="status" className="text-destructive">
          {held.refusal}
        </p>
      )}
      {refusal !== null && (
        <p role="alert" className="text-destructive">
          {refusal}
        </p>
      )}
    </div>
  );
}

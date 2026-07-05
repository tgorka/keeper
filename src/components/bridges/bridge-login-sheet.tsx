/**
 * The native bridge-login stepper Sheet (Story 6.3, FR-26, AD-16, UX-DR8).
 *
 * A `Sheet` over the Bridges surface that renders the transport-agnostic login
 * state machine driven by {@link useBridgeLogin}: choosing method → waiting → QR
 * panel *or* code-entry → success / failure. Each {@link BridgeLoginPhase} renders
 * as a distinct native state with the shared {@link BRIDGE_LOGIN_PHASE_LABEL} live
 * state word:
 *
 * - **choosingMethod** — a {@link RadioGroup} of the bridge's login flows.
 * - **waiting** — a spinner + instruction line.
 * - **qr** — the Rust-rendered QR SVG on a mandatory white card ≥ 240 px with a
 *   quiet zone in *both* themes (white is required for scannability), the
 *   per-network instruction, and a subtle "QR refreshed" note on rotation.
 * - **codeEntry** — labeled {@link Input}s per non-secret field descriptor,
 *   client-side `pattern`-validated before submit; code/2fa fields render in an
 *   {@link InputGroup}.
 * - **success** — "Linked ✓" in bridge-healthy green, auto-advancing (~1.5 s).
 * - **failure** — the bridge's own error message **verbatim** + Retry (the
 *   unsupported-method state is a failure whose copy names the Bridge Bot chat).
 *
 * The QR is rendered as `<img src="data:image/svg+xml,…">` from the SVG string the
 * Rust core produced — never a JS QR lib, never `dangerouslySetInnerHTML`.
 */
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useBridgeLogin } from "@/hooks/use-bridge-login";
import { BRIDGE_LOGIN_PHASE_LABEL } from "@/lib/bridges";
import type { BridgeLoginVm, LoginFieldVm } from "@/lib/ipc/client";
import { bridgeBotRoom } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

/** How long "Linked ✓" shows before the Sheet auto-advances (ms). */
const SUCCESS_AUTO_ADVANCE_MS = 1500;

/** Whether a field type reads best as a segmented code input. */
function isCodeField(fieldType: string): boolean {
  return fieldType === "2fa_code" || fieldType === "code" || fieldType === "otp";
}

/** Whether a field's value should be masked (a password / secret). */
function isSecretField(fieldType: string): boolean {
  return fieldType === "password" || fieldType === "token";
}

interface BridgeLoginSheetProps {
  /** The account id the login is keyed to. */
  accountId: string;
  /** The network id being linked. */
  networkId: string;
  /** The network's display name (for the Sheet title / instructions). */
  networkName: string;
  /** Whether the Sheet is open. */
  open: boolean;
  /** Called when the Sheet should close (Esc, backdrop, cancel, auto-advance). */
  onOpenChange: (open: boolean) => void;
}

export function BridgeLoginSheet({
  accountId,
  networkId,
  networkName,
  open,
  onOpenChange,
}: BridgeLoginSheetProps) {
  const { vm, start, submit, cancel } = useBridgeLogin(accountId, networkId, open);

  // Kick off the login when the Sheet opens.
  useEffect(() => {
    if (open) {
      start();
    }
  }, [open, start]);

  // Auto-advance out of the success state after a beat.
  useEffect(() => {
    if (vm?.phase !== "success") {
      return;
    }
    const timer = setTimeout(() => onOpenChange(false), SUCCESS_AUTO_ADVANCE_MS);
    return () => clearTimeout(timer);
  }, [vm?.phase, onOpenChange]);

  const handleOpenChange = (next: boolean) => {
    if (!next) {
      cancel();
    }
    onOpenChange(next);
  };

  const stateWord = vm ? BRIDGE_LOGIN_PHASE_LABEL[vm.phase] : BRIDGE_LOGIN_PHASE_LABEL.waiting;

  return (
    <Sheet open={open} onOpenChange={handleOpenChange}>
      <SheetContent className="flex flex-col gap-4">
        <SheetHeader>
          <SheetTitle>Connect {networkName}</SheetTitle>
          <SheetDescription data-slot="bridge-login-state-word">{stateWord}</SheetDescription>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
          <BridgeLoginBody
            vm={vm}
            accountId={accountId}
            networkId={networkId}
            networkName={networkName}
            onChooseFlow={(flowId) => submit({ kind: "chooseFlow", flowId })}
            onSubmitFields={(values) => submit({ kind: "fields", values })}
            onRetry={start}
            onClose={() => onOpenChange(false)}
          />
        </div>
      </SheetContent>
    </Sheet>
  );
}

interface BridgeLoginBodyProps {
  vm: BridgeLoginVm | null;
  accountId: string;
  networkId: string;
  networkName: string;
  onChooseFlow: (flowId: string) => void;
  onSubmitFields: (values: Record<string, string>) => void;
  onRetry: () => void;
  onClose: () => void;
}

function BridgeLoginBody({
  vm,
  accountId,
  networkId,
  networkName,
  onChooseFlow,
  onSubmitFields,
  onRetry,
  onClose,
}: BridgeLoginBodyProps) {
  if (vm === null) {
    return <WaitingPanel instruction="Connecting…" />;
  }

  switch (vm.phase) {
    case "choosingMethod":
      // Key by the flow id set so a second choosingMethod step with different
      // flows remounts the panel, resetting its selection state.
      return (
        <ChoosingMethodPanel
          key={vm.flows.map((f) => f.id).join("|")}
          vm={vm}
          onChoose={onChooseFlow}
        />
      );
    case "waiting":
      return <WaitingPanel instruction={vm.instruction ?? "Waiting…"} />;
    case "qr":
      return <QrPanel vm={vm} networkName={networkName} />;
    case "codeEntry":
      // Key by the field id set so a second user_input step with different fields
      // remounts the panel, resetting stale field state to the new defaults.
      return (
        <CodeEntryPanel
          key={vm.fields.map((f) => f.id).join("|")}
          vm={vm}
          onSubmit={onSubmitFields}
        />
      );
    case "success":
      return <SuccessPanel networkName={networkName} />;
    case "failure":
      return (
        <FailurePanel
          vm={vm}
          accountId={accountId}
          networkId={networkId}
          onRetry={onRetry}
          onClose={onClose}
        />
      );
    default:
      return null;
  }
}

function WaitingPanel({ instruction }: { instruction: string }) {
  return (
    <div
      className="flex flex-col items-center gap-3 py-8 text-center"
      data-slot="bridge-login-waiting"
    >
      <span
        aria-hidden="true"
        className="size-6 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-foreground"
      />
      <p className="text-muted-foreground text-sm">{instruction}</p>
    </div>
  );
}

function ChoosingMethodPanel({
  vm,
  onChoose,
}: {
  vm: BridgeLoginVm;
  onChoose: (flowId: string) => void;
}) {
  const [selected, setSelected] = useState<string>(vm.flows[0]?.id ?? "");

  return (
    <div className="flex flex-col gap-4" data-slot="bridge-login-choosing">
      {vm.instruction && <p className="text-muted-foreground text-sm">{vm.instruction}</p>}
      <RadioGroup value={selected} onValueChange={setSelected}>
        {vm.flows.map((flow) => (
          <Label
            key={flow.id}
            className="flex cursor-pointer items-start gap-3 rounded-md border p-3"
          >
            <RadioGroupItem value={flow.id} className="mt-0.5" />
            <span className="flex flex-col gap-0.5">
              <span className="font-medium text-sm">{flow.name}</span>
              {flow.description && (
                <span className="text-muted-foreground text-xs">{flow.description}</span>
              )}
            </span>
          </Label>
        ))}
      </RadioGroup>
      <Button type="button" onClick={() => onChoose(selected)} disabled={selected === ""}>
        Continue
      </Button>
    </div>
  );
}

function QrPanel({ vm, networkName }: { vm: BridgeLoginVm; networkName: string }) {
  return (
    <div className="flex flex-col items-center gap-3" data-slot="bridge-login-qr">
      {/* Mandatory white card ≥ 240px with a quiet zone in BOTH themes — white is
          required for QR scannability, so it does NOT flip with the theme. */}
      <div className="flex size-60 items-center justify-center rounded-lg bg-white p-4">
        {vm.qrSvg ? (
          <img
            src={`data:image/svg+xml,${encodeURIComponent(vm.qrSvg)}`}
            alt={`Scan this QR code with ${networkName}`}
            className="size-full"
          />
        ) : null}
      </div>
      {vm.instruction && (
        <p className="text-center text-muted-foreground text-sm">{vm.instruction}</p>
      )}
      {vm.qrRefreshed && (
        <p className="text-muted-foreground text-xs" data-slot="bridge-login-qr-refreshed">
          QR refreshed
        </p>
      )}
    </div>
  );
}

function CodeEntryPanel({
  vm,
  onSubmit,
}: {
  vm: BridgeLoginVm;
  onSubmit: (values: Record<string, string>) => void;
}) {
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(vm.fields.map((f) => [f.id, f.defaultValue ?? ""])),
  );

  // Client-side pattern validation: every field with a `pattern` must match
  // before the submit is allowed (I/O matrix: validated before submit).
  const invalidField = useMemo(() => findInvalidField(vm.fields, values), [vm.fields, values]);
  const canSubmit = invalidField === null && vm.fields.every((f) => values[f.id] !== "");

  return (
    <form
      className="flex flex-col gap-4"
      data-slot="bridge-login-code-entry"
      onSubmit={(e) => {
        e.preventDefault();
        if (canSubmit) {
          onSubmit(values);
        }
      }}
    >
      {vm.instruction && <p className="text-muted-foreground text-sm">{vm.instruction}</p>}
      {vm.fields.map((field) => (
        <FieldInput
          key={field.id}
          field={field}
          value={values[field.id] ?? ""}
          onChange={(v) => setValues((prev) => ({ ...prev, [field.id]: v }))}
        />
      ))}
      <Button type="submit" disabled={!canSubmit}>
        Submit
      </Button>
    </form>
  );
}

function FieldInput({
  field,
  value,
  onChange,
}: {
  field: LoginFieldVm;
  value: string;
  onChange: (value: string) => void;
}) {
  const label = (
    <Label htmlFor={`bridge-login-field-${field.id}`} className="flex flex-col items-start gap-1">
      <span className="font-medium text-sm">{field.name}</span>
      {field.description && (
        <span className="text-muted-foreground text-xs">{field.description}</span>
      )}
    </Label>
  );

  return (
    <div className="flex flex-col gap-1.5">
      {label}
      {isCodeField(field.fieldType) ? (
        <InputGroup>
          <InputGroupInput
            id={`bridge-login-field-${field.id}`}
            inputMode="numeric"
            autoComplete="one-time-code"
            value={value}
            onChange={(e) => onChange(e.target.value)}
          />
        </InputGroup>
      ) : (
        <Input
          id={`bridge-login-field-${field.id}`}
          type={isSecretField(field.fieldType) ? "password" : "text"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      )}
    </div>
  );
}

function SuccessPanel({ networkName }: { networkName: string }) {
  return (
    <div
      className="flex flex-col items-center gap-2 py-8 text-center"
      data-slot="bridge-login-success"
    >
      <p className={cn("font-medium text-lg", "text-bridge-healthy")}>Linked ✓</p>
      <p className="text-muted-foreground text-sm">{networkName} is connected.</p>
    </div>
  );
}

function FailurePanel({
  vm,
  accountId,
  networkId,
  onRetry,
  onClose,
}: {
  vm: BridgeLoginVm;
  accountId: string;
  networkId: string;
  onRetry: () => void;
  onClose: () => void;
}) {
  // The manual escape hatch (UX-DR19): resolve-or-create the raw Bridge Bot DM room,
  // navigate to it (Inbox + select the room), and close the Sheet. A resolve failure
  // is logged, not thrown — the button is best-effort and must never crash the Sheet.
  const openBotChat = async () => {
    try {
      const roomId = await bridgeBotRoom(accountId, networkId);
      primaryViewStore.getState().setView("inbox");
      roomsStore.getState().selectRoom({ accountId, roomId });
      onClose();
    } catch (error) {
      console.error("could not open the Bridge Bot chat", error);
      toast.error("Couldn't open the Bridge Bot chat. Try again.");
    }
  };

  return (
    <div className="flex flex-col gap-4" data-slot="bridge-login-failure">
      {/* The bridge's own error message, verbatim — keeper never rewrites it. */}
      <p className="text-bridge-disconnected text-sm" data-slot="bridge-login-error">
        {vm.error ?? "The login failed."}
      </p>
      <Button type="button" variant="outline" onClick={onRetry}>
        Retry
      </Button>
      {/* The raw Bridge Bot chat stays reachable as the manual escape hatch. */}
      <Button type="button" variant="ghost" onClick={() => void openBotChat()}>
        Open Bridge Bot chat
      </Button>
    </div>
  );
}

/**
 * Return the first field whose entered value fails its `pattern`, or `null` when
 * every field with a pattern matches (empty values are gated separately by
 * `canSubmit`, so an empty value is not treated as a pattern failure here).
 */
function findInvalidField(fields: LoginFieldVm[], values: Record<string, string>): string | null {
  for (const field of fields) {
    if (!field.pattern) {
      continue;
    }
    const value = values[field.id] ?? "";
    if (value === "") {
      continue;
    }
    let regex: RegExp;
    try {
      // Anchor for a full-string match — an unanchored pattern substring-matches,
      // so e.g. `[0-9]{6}` would wrongly accept "123456x".
      regex = new RegExp(`^(?:${field.pattern})$`);
    } catch {
      // A malformed pattern from the bridge is not the user's fault — skip it.
      continue;
    }
    if (!regex.test(value)) {
      return field.id;
    }
  }
  return null;
}

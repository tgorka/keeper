/**
 * The setup confirmation sheet (Epic 82, UX-DR116 (2)).
 *
 * ONE sheet for every way an account is set up — a link pasted into Settings
 * or the first-run wizard, a scanned QR code, a `keeper://setup` link that
 * started keeper. It is mounted once at the app root and opens on
 * `accountStore.setupLink`, so none of those callers owns a copy that could
 * word the hosts differently.
 *
 * # Why the hosts are the point
 *
 * A setup link can point keeper at anybody's identity provider and anybody's
 * repository. So before anything is written the sheet names both hosts, in
 * mono and never truncated, and says that nothing happens until Continue.
 * Rust has already fetched and validated the descriptor by then
 * (`account_setup_resolve`); a refusal is Rust's sentence, verbatim.
 *
 * # After Continue
 *
 * Sign-in, the forge connection, the repository sync and applying its
 * settings all run in Rust; their progress is the mirror's `sentence`, read
 * live — but only from the first snapshot Rust publishes after Continue. The
 * snapshot already in the mirror describes whatever was there before (the
 * previous account's "Up to date.", or nothing), so until Rust speaks the
 * sheet shows a spinner and no words of its own.
 *
 * The outcome is the VM the confirm resolved with. It resolves, rather than
 * rejects, when the sign-in was refused, cancelled or could not reach the
 * issuer: those outcomes live in `vm.state`, so that is where the sheet reads
 * success or failure from. Closing the sheet mid-sign-in cancels the browser
 * round trip rather than leaving it waiting for five minutes behind a closed
 * surface.
 */
import { ScanQrCode } from "lucide-react";
import { lazy, Suspense, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  type AccountSetupVm,
  type AccountStateVm,
  accountCancelSignIn,
  accountSetupConfirm,
  accountSetupResolve,
  type DeviceClassVm,
  type OrgAccountVm,
} from "@/lib/ipc/client";
import { accountStore, useAccountStore } from "@/lib/stores/account";
import { syncErrorMessage } from "@/lib/stores/sync";

export const SETUP_SHEET_TITLE = "Set up an account";
export const SETUP_RESOLVING = "Reading the setup link…";
export const SETUP_DISCLOSURE =
  "Check both hosts before you continue. keeper writes nothing until you do.";
export const SETUP_ACCOUNT_LABEL = "Account";
export const SETUP_ISSUER_LABEL = "Signs in at";
export const SETUP_REPO_LABEL = "Keeps your settings at";
export const SETUP_DEVICE_LABEL = "This device's name";
export const SETUP_DEVICE_NOTE =
  "Names this device's files in your settings repository. You can rename it later.";
export const SETUP_DEVICE_REGISTERED_NOTE =
  "This device is already registered under this name. Rename it in Settings › Account.";
export const SETUP_CLASS_LABEL = "Kind of device";
export const SETUP_CONTINUE_LABEL = "Continue";
export const SETUP_CANCEL_LABEL = "Cancel";
export const SETUP_CANCEL_SIGN_IN_LABEL = "Cancel sign-in";
export const SETUP_DONE_LABEL = "Done";
export const SETUP_CLOSE_LABEL = "Close";
/** What a failure says when Rust gave no sentence of its own. */
export const SETUP_FAILED = "keeper couldn't set up the account.";

/**
 * The one sentence the confirm step adds when Continue would replace the
 * account this device already has (AD-319). Rust names the account
 * (`setup.replaces`); the sheet only words it.
 */
export function setupReplacesSentence(name: string): string {
  return `This replaces ${name} on this device: keeper signs you out of it first. Its files in the settings repository are kept.`;
}

/**
 * The states a confirm can resolve in that mean the account is NOT set up and
 * working: refused by policy, the grant needs renewing, the issuer or the
 * repository was unreachable, or the sign-in was cancelled.
 */
const FAILED_OUTCOME: Record<AccountStateVm, boolean> = {
  none: false,
  signingIn: false,
  syncing: false,
  ready: false,
  blocked: true,
  needsSignIn: true,
  offline: true,
  signedOut: true,
};

/** How each class reads. The wire word stays the wire word everywhere else. */
export const DEVICE_CLASS_LABEL: Record<DeviceClassVm, string> = {
  desktop: "Desktop",
  tablet: "Tablet",
  mobile: "Phone",
};

/** The sheet, mounted once at the app root. */
export function AccountSetupSheet() {
  const link = useAccountStore((s) => s.setupLink);
  return (
    <Sheet
      open={link !== null}
      onOpenChange={(next) => {
        if (!next) {
          accountStore.getState().closeSetup();
        }
      }}
    >
      <SheetContent className="flex flex-col gap-4">
        {/* Keyed by the link, so a second link arriving while the sheet is up
            starts over rather than confirming the first one's descriptor. */}
        {link !== null && <SetupFlow key={link} link={link} />}
      </SheetContent>
    </Sheet>
  );
}

export const SETUP_LINK_LABEL = "Paste a setup link";
export const SETUP_LINK_PLACEHOLDER = "keeper://setup?… or https://…";

export const SCAN_SETUP_CODE_LABEL = "Scan a QR code";
export const SCAN_HINT = "Hold the setup code up to the camera.";
export const SCAN_STARTING = "Starting the camera…";
export const SCAN_DENIED =
  "keeper can't use the camera. Allow it in System Settings › Privacy & Security › Camera, or paste the link instead.";
export const SCAN_OPEN_CAMERA_SETTINGS_LABEL = "Open Camera settings";
export const SCAN_NO_CAMERA = "No camera found. Paste the link instead.";
export const SCAN_FAILED = "The camera could not start. Paste the link instead.";

/**
 * Whether this webview can offer the scan at all (AD-317). WebKit exposes
 * `getUserMedia` only where the app declares a camera use — the macOS bundle
 * does, the iOS one does not — so where it is missing the control is absent,
 * not disabled (AD-27): the phone's own Camera app already opens a
 * `keeper://setup` code.
 */
export function canScanSetupCode(): boolean {
  return typeof navigator.mediaDevices?.getUserMedia === "function";
}

// Lazy so neither the decoder nor its wasm is in the main chunk: most people
// never scan, and those who do scan once.
const SetupCodeScanner = lazy(() =>
  import("@/components/account/setup-code-scanner").then((module) => ({
    default: module.SetupCodeScanner,
  })),
);

/**
 * The one entry field: Settings › Account, the wizard's optional step and the
 * footer's keeper-account dialog all mount this, and all hand the link to the
 * same sheet. Nothing is parsed here — `account_setup_resolve` owns the grammar
 * and its refusal sentence appears in the sheet; a scanned code's text takes
 * exactly the road a pasted link does. Continue is never disabled (UX-DR116
 * (1): the signed-out section shows no disabled control); an empty submit does
 * nothing. The paste input stays usable while the camera runs, so somebody who
 * gives up on the scan can paste without putting it away first.
 */
export function SetupLinkField({ id }: { id: string }) {
  const [link, setLink] = useState("");
  const [scanning, setScanning] = useState(false);
  const submit = (raw: string) => {
    const trimmed = raw.trim();
    if (trimmed !== "") {
      setScanning(false);
      setLink("");
      accountStore.getState().openSetup(trimmed);
    }
  };
  return (
    <form
      className="flex flex-col gap-1"
      onSubmit={(event) => {
        event.preventDefault();
        submit(link);
      }}
    >
      <Label htmlFor={id}>{SETUP_LINK_LABEL}</Label>
      <div className="flex flex-wrap gap-2">
        <Input
          id={id}
          className="min-w-48 flex-1 font-mono"
          autoComplete="off"
          spellCheck={false}
          placeholder={SETUP_LINK_PLACEHOLDER}
          value={link}
          onChange={(event) => setLink(event.target.value)}
        />
        <Button type="submit" variant="outline">
          {SETUP_CONTINUE_LABEL}
        </Button>
        {!scanning && canScanSetupCode() && (
          <Button type="button" variant="outline" onClick={() => setScanning(true)}>
            <ScanQrCode aria-hidden="true" />
            {SCAN_SETUP_CODE_LABEL}
          </Button>
        )}
      </div>
      {scanning && (
        <div className="pt-2">
          <Suspense fallback={<Waiting sentence={SCAN_STARTING} />}>
            <SetupCodeScanner onScanned={submit} onCancel={() => setScanning(false)} />
          </Suspense>
        </div>
      )}
    </form>
  );
}

type Phase =
  | { kind: "resolving" }
  | { kind: "refused"; sentence: string }
  | { kind: "confirm"; setup: AccountSetupVm }
  /** `before` is the mirror's snapshot at Continue: anything older is not this setup's progress. */
  | { kind: "working"; setup: AccountSetupVm; before: OrgAccountVm }
  | { kind: "finished"; setup: AccountSetupVm; sentence: string | null; failed: boolean };

function SetupFlow({ link }: { link: string }) {
  const [phase, setPhase] = useState<Phase>({ kind: "resolving" });
  const [deviceName, setDeviceName] = useState("");
  const live = useAccountStore((s) => s.vm);
  const close = accountStore.getState().closeSetup;

  useEffect(() => {
    let abandoned = false;
    void accountSetupResolve(link)
      .then((setup) => {
        if (!abandoned) {
          setDeviceName(setup.deviceName);
          setPhase({ kind: "confirm", setup });
        }
      })
      .catch((raw: unknown) => {
        if (!abandoned) {
          setPhase({ kind: "refused", sentence: syncErrorMessage(raw, SETUP_FAILED) });
        }
      });
    return () => {
      abandoned = true;
    };
  }, [link]);

  // Leaving while the browser round trip is open cancels it: the sheet is the
  // only surface that says a sign-in is waiting.
  useEffect(() => {
    if (phase.kind !== "working") {
      return;
    }
    return () => {
      if (accountStore.getState().vm.state === "signingIn") {
        void accountCancelSignIn().catch(() => {});
      }
    };
  }, [phase.kind]);

  const confirm = (setup: AccountSetupVm) => {
    setPhase({ kind: "working", setup, before: accountStore.getState().vm });
    void accountSetupConfirm(setup.setupId, deviceName.trim())
      .then((vm) => {
        accountStore.getState().setVm(vm);
        setPhase({
          kind: "finished",
          setup,
          sentence: vm.sentence,
          failed: FAILED_OUTCOME[vm.state],
        });
      })
      .catch((raw: unknown) => {
        setPhase({
          kind: "finished",
          setup,
          sentence: syncErrorMessage(raw, SETUP_FAILED),
          failed: true,
        });
      });
  };

  // Rust's progress, once Rust has published anything since Continue.
  const progress = phase.kind === "working" && live !== phase.before ? live : null;

  const setup = phase.kind === "resolving" || phase.kind === "refused" ? null : phase.setup;

  return (
    <>
      <SheetHeader>
        <SheetTitle>{setup === null ? SETUP_SHEET_TITLE : `Set up ${setup.name}`}</SheetTitle>
        <SheetDescription>{SETUP_DISCLOSURE}</SheetDescription>
      </SheetHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4 text-sm">
        {phase.kind === "resolving" && <Waiting sentence={SETUP_RESOLVING} />}

        {phase.kind === "refused" && (
          <>
            <p role="alert" className="text-destructive">
              {phase.sentence}
            </p>
            <Button type="button" variant="outline" className="w-fit" onClick={close}>
              {SETUP_CLOSE_LABEL}
            </Button>
          </>
        )}

        {setup !== null && <SetupFacts setup={setup} />}

        {phase.kind === "confirm" && (
          <form
            className="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (deviceName.trim() !== "") {
                confirm(phase.setup);
              }
            }}
          >
            {phase.setup.registered ? (
              <div className="flex flex-col gap-1">
                <p className="text-muted-foreground text-xs">{SETUP_DEVICE_LABEL}</p>
                <p className="font-mono">{phase.setup.deviceName}</p>
                <p className="text-muted-foreground text-xs">{SETUP_DEVICE_REGISTERED_NOTE}</p>
              </div>
            ) : (
              <div className="flex flex-col gap-1">
                <Label htmlFor="account-setup-device">{SETUP_DEVICE_LABEL}</Label>
                <Input
                  id="account-setup-device"
                  className="font-mono"
                  autoComplete="off"
                  spellCheck={false}
                  value={deviceName}
                  onChange={(event) => setDeviceName(event.target.value)}
                />
                <p className="text-muted-foreground text-xs">{SETUP_DEVICE_NOTE}</p>
              </div>
            )}
            {phase.setup.replaces !== null && (
              <p className="text-sm">{setupReplacesSentence(phase.setup.replaces)}</p>
            )}
            <div className="flex gap-2">
              <Button type="submit" disabled={deviceName.trim() === ""}>
                {SETUP_CONTINUE_LABEL}
              </Button>
              <Button type="button" variant="outline" onClick={close}>
                {SETUP_CANCEL_LABEL}
              </Button>
            </div>
          </form>
        )}

        {phase.kind === "working" && (
          <>
            <Waiting sentence={progress?.sentence ?? null} />
            {progress?.state === "signingIn" && (
              <Button
                type="button"
                variant="outline"
                className="w-fit"
                onClick={() => void accountCancelSignIn().catch(() => {})}
              >
                {SETUP_CANCEL_SIGN_IN_LABEL}
              </Button>
            )}
          </>
        )}

        {phase.kind === "finished" && (
          <>
            {phase.sentence !== null && (
              <p
                role={phase.failed ? "alert" : "status"}
                className={phase.failed ? "text-destructive" : undefined}
              >
                {phase.sentence}
              </p>
            )}
            <Button type="button" className="w-fit" onClick={close}>
              {phase.failed ? SETUP_CLOSE_LABEL : SETUP_DONE_LABEL}
            </Button>
          </>
        )}
      </div>
    </>
  );
}

/** The four facts a person is asked to check, as a definition list. */
function SetupFacts({ setup }: { setup: AccountSetupVm }) {
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2">
      <dt className="text-muted-foreground">{SETUP_ACCOUNT_LABEL}</dt>
      <dd>{setup.name}</dd>
      <dt className="text-muted-foreground">{SETUP_ISSUER_LABEL}</dt>
      <dd className="break-all font-mono">{setup.issuerHost}</dd>
      <dt className="text-muted-foreground">{SETUP_REPO_LABEL}</dt>
      <dd className="break-all font-mono">{setup.repoHost}</dd>
      <dt className="text-muted-foreground">{SETUP_CLASS_LABEL}</dt>
      <dd>{DEVICE_CLASS_LABEL[setup.deviceClass]}</dd>
    </dl>
  );
}

/**
 * A spinner beside a sentence, the bridge-login sheet's waiting shape. With no
 * sentence yet it is the spinner alone: the words are Rust's, and until Rust
 * has said anything the sheet does not invent a line for it.
 */
export function Waiting({ sentence }: { sentence: string | null }) {
  return (
    <div role="status" aria-busy="true" className="flex items-center gap-3">
      <span
        aria-hidden="true"
        className="size-4 shrink-0 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-foreground"
      />
      {sentence !== null && <p className="text-muted-foreground">{sentence}</p>}
    </div>
  );
}

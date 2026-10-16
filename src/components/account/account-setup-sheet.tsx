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
import { reportFrontendError } from "@/lib/crash-report";
import {
  type AccountSetupVm,
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
  "Check every host before you continue. keeper writes nothing until you do.";
export const SETUP_ACCOUNT_LABEL = "Account";
export const SETUP_ISSUER_LABEL = "Signs in at";
export const SETUP_REPO_LABEL = "Keeps your settings at";
/** The GitHub broker a descriptor names (Epic 86, AD-333): a third host to check. */
export const SETUP_BROKER_LABEL = "Gets GitHub access from";
/**
 * Every OTHER host keeper would hand a repository token to under this
 * descriptor (Epic 86 fix B1, Rust's `AccountSetupVm.forgeHosts`): the web
 * and API hosts of each GitHub source it adds — a broker adds github.com and
 * api.github.com. The account's own forge is the settings host above, and the
 * broker has its own row. A descriptor that pointed GitHub's API at somebody
 * else's server would show that server here, before anything is written.
 */
export const SETUP_FORGE_HOSTS_LABEL = "Also sends repository tokens to";
export const SETUP_DEVICE_LABEL = "This device's name";
export const SETUP_DEVICE_NOTE =
  "Names this device's files in your settings repository. You can rename it later.";
/**
 * The note under a device name the repository already knows (UX-DR119): a
 * reinstall keeps its name (AD-329), and the first sync brings the rest back.
 */
export const SETUP_DEVICE_REGISTERED_NOTE =
  "This device is already in your settings repository; keeper will restore its drives, bots and settings.";
export const SETUP_CLASS_LABEL = "Kind of device";
export const SETUP_CONTINUE_LABEL = "Continue";
export const SETUP_CANCEL_LABEL = "Cancel";
export const SETUP_CANCEL_SIGN_IN_LABEL = "Cancel sign-in";
export const SETUP_DONE_LABEL = "Done";
export const SETUP_CLOSE_LABEL = "Close";
/** What a failure says when Rust gave no sentence of its own. */
export const SETUP_FAILED = "keeper couldn't set up the account.";
/** What a failed "Cancel sign-in" says when Rust gave no sentence of its own. */
export const SETUP_CANCEL_SIGN_IN_FAILED = "keeper couldn't cancel the sign-in.";

/**
 * What the sheet says after the person closed the sign-in themselves. Quiet
 * (R381: "closed" is no problem), but never the signed-out instruction
 * alone, which reads as if nothing had been tried (100.1 #4), and never "Up
 * to date" for an account that was already signed in here (R387): `kept`
 * says the account stays as it was.
 */
export function setupClosedSentence(name: string, kept: boolean): string {
  return kept
    ? `The sign-in was closed before it finished. ${name} stays as it was on this device.`
    : `The sign-in was closed before it finished, so ${name} is not set up on this device yet. Sign in from Settings › Account when you are ready.`;
}

/**
 * The one sentence the confirm step adds when Continue would replace the
 * account this device already has (AD-319). Rust names the account
 * (`setup.replaces`); the sheet only words it.
 */
export function setupReplacesSentence(name: string): string {
  return `This replaces ${name} on this device: keeper signs you out of it first. Its files in the settings repository are kept.`;
}

/**
 * How a confirm's answer reads in the sheet: the outcome of this attempt as
 * Rust recorded it (`attempt`, R387), never inferred from the account's
 * state — a closed re-setup of an account already signed in here still reads
 * "Up to date." there. Only Rust's `done` is Done; an attempt Rust did not
 * call finished is never shown as one.
 */
function outcomeOf(
  vm: OrgAccountVm,
  setup: AccountSetupVm,
): { sentence: string | null; tone: Tone } {
  switch (vm.attempt) {
    case "done":
      return { sentence: vm.sentence, tone: "done" };
    case "closed":
      return { sentence: setupClosedSentence(setup.name, vm.identity !== null), tone: "closed" };
    default:
      return { sentence: vm.sentence, tone: "failed" };
  }
}

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

/** A finished setup: set up, failed (an alert), or closed by the person (quiet). */
type Tone = "done" | "failed" | "closed";

type Phase =
  | { kind: "resolving" }
  | { kind: "refused"; sentence: string }
  | { kind: "confirm"; setup: AccountSetupVm }
  /** `before` is the mirror's snapshot at Continue: anything older is not this setup's progress. */
  | { kind: "working"; setup: AccountSetupVm; before: OrgAccountVm }
  | { kind: "finished"; setup: AccountSetupVm; sentence: string | null; tone: Tone };

function SetupFlow({ link }: { link: string }) {
  const [phase, setPhase] = useState<Phase>({ kind: "resolving" });
  const [deviceName, setDeviceName] = useState("");
  const [cancelError, setCancelError] = useState<string | null>(null);
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
        reportFrontendError("account.setupResolve", raw, null);
        if (!abandoned) {
          setPhase({ kind: "refused", sentence: syncErrorMessage(raw, SETUP_FAILED) });
        }
      });
    return () => {
      abandoned = true;
    };
  }, [link]);

  // Leaving while the browser round trip is open cancels it: the sheet is the
  // only surface that says a sign-in is waiting. Nobody is left to read a
  // sentence by then, so a failed cancel goes to the log.
  useEffect(() => {
    if (phase.kind !== "working") {
      return;
    }
    return () => {
      if (accountStore.getState().vm.state === "signingIn") {
        void accountCancelSignIn().catch((raw: unknown) =>
          reportFrontendError("account.cancelSignIn", raw, null),
        );
      }
    };
  }, [phase.kind]);

  const cancelSignIn = () => {
    setCancelError(null);
    void accountCancelSignIn().catch((raw: unknown) => {
      reportFrontendError("account.cancelSignIn", raw, null);
      setCancelError(syncErrorMessage(raw, SETUP_CANCEL_SIGN_IN_FAILED));
    });
  };

  const confirm = (setup: AccountSetupVm) => {
    setPhase({ kind: "working", setup, before: accountStore.getState().vm });
    void accountSetupConfirm(setup.setupId, deviceName.trim())
      .then((vm) => {
        accountStore.getState().setVm(vm);
        setPhase({ kind: "finished", setup, ...outcomeOf(vm, setup) });
      })
      .catch((raw: unknown) => {
        reportFrontendError("account.setupConfirm", raw, null);
        setPhase({
          kind: "finished",
          setup,
          sentence: syncErrorMessage(raw, SETUP_FAILED),
          tone: "failed",
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
              <Button type="button" variant="outline" className="w-fit" onClick={cancelSignIn}>
                {SETUP_CANCEL_SIGN_IN_LABEL}
              </Button>
            )}
            {cancelError !== null && (
              <p role="alert" className="text-destructive">
                {cancelError}
              </p>
            )}
          </>
        )}

        {phase.kind === "finished" && (
          <>
            {phase.sentence !== null && (
              <p
                role={phase.tone === "failed" ? "alert" : "status"}
                className={phase.tone === "failed" ? "text-destructive" : undefined}
              >
                {phase.sentence}
              </p>
            )}
            <Button type="button" className="w-fit" onClick={close}>
              {phase.tone === "done" ? SETUP_DONE_LABEL : SETUP_CLOSE_LABEL}
            </Button>
          </>
        )}
      </div>
    </>
  );
}

/** The facts a person is asked to check, as a definition list. */
function SetupFacts({ setup }: { setup: AccountSetupVm }) {
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2">
      <dt className="text-muted-foreground">{SETUP_ACCOUNT_LABEL}</dt>
      <dd>{setup.name}</dd>
      <dt className="text-muted-foreground">{SETUP_ISSUER_LABEL}</dt>
      <dd className="break-all font-mono">{setup.issuerHost}</dd>
      <dt className="text-muted-foreground">{SETUP_REPO_LABEL}</dt>
      <dd className="break-all font-mono">{setup.repoHost}</dd>
      {setup.brokerHost !== null && (
        <>
          <dt className="text-muted-foreground">{SETUP_BROKER_LABEL}</dt>
          <dd className="break-all font-mono">{setup.brokerHost}</dd>
        </>
      )}
      {setup.forgeHosts.length > 0 && (
        <>
          <dt className="text-muted-foreground">{SETUP_FORGE_HOSTS_LABEL}</dt>
          <dd>
            <ul aria-label={SETUP_FORGE_HOSTS_LABEL} className="flex flex-col gap-0.5">
              {setup.forgeHosts.map((host) => (
                <li key={host} className="break-all font-mono">
                  {host}
                </li>
              ))}
            </ul>
          </dd>
        </>
      )}
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

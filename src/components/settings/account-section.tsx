/**
 * Settings › Account (Epic 82, UX-DR116 (1), (3), (8)).
 *
 * The FIRST section of Settings, on every tier, behind no capability: the
 * account is how settings arrive on a device, so the place to set one up must
 * be readable before anything else is — and it must be reachable on a build
 * where every optional surface is absent (AD-27's ungated precedent, the way
 * `ConfigSourceSection` is).
 *
 * # With no account, almost nothing
 *
 * The heading, one sentence saying keeper works fully without an account, and
 * the paste field. No disabled buttons, no empty device list, no "Sign in"
 * that has nothing to sign in to: an install that never meets an account must
 * look like keeper did before accounts existed.
 *
 * # With one
 *
 * Everything a person reads about its state is Rust's `sentence`, rendered
 * verbatim — "Up to date.", "Offline — using settings from 14:02.", the
 * blocked sentence naming the file that disagreed. This section composes no
 * state word of its own.
 *
 * Rename is an inline disclosure on this device's row (a modal over a list
 * hides the rows being compared — the Bots rule). Sign out and Forget are
 * AlertDialogs, and each names what goes and what stays: signing out keeps the
 * account set up and the repository's files where they are; forgetting
 * deletes this device's copy and never touches the server.
 *
 * # After a restore (Epic 85, UX-DR119)
 *
 * Under the status: Rust's restore sentence and each pending one ("Waiting
 * for /Volumes/… to restore …"), verbatim and absent when empty. When this
 * device had listening on and a restore left it off, one sentence says so
 * with "Turn listening on" and "Keep it off". The microphone is armed only
 * by the first tap (AD-168): nothing here runs on render, that tap reads the
 * switch first so a stale offer can never flip listening OFF, and "Keep it
 * off" writes the switch off as the person's choice and never arms it.
 */
import { useEffect, useId, useRef, useState } from "react";
import { DEVICE_CLASS_LABEL, SetupLinkField } from "@/components/account/account-setup-sheet";
import { AccountShareSheet } from "@/components/account/account-share-sheet";
import { toggleVoiceListening } from "@/components/bots/bot-listening-toggle";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  type AccountDeviceVm,
  accountCancelSignIn,
  accountForget,
  accountRenameDevice,
  accountSignIn,
  accountSignOut,
  accountState,
  accountSync,
  type OrgAccountVm,
  voiceWakeGet,
  voiceWakeSet,
} from "@/lib/ipc/client";
import { accountStore, useAccountStore } from "@/lib/stores/account";
import { syncErrorMessage } from "@/lib/stores/sync";
import { voiceStore } from "@/lib/stores/voice";

export const ACCOUNT_SECTION_TITLE = "Account";
export const ACCOUNT_SECTION_NOTE =
  "keeper works fully without an account. An organisation account signs you in once and brings your settings to every device you use.";
export const ACCOUNT_SIGN_IN_LABEL = "Sign in";
export const ACCOUNT_CANCEL_SIGN_IN_LABEL = "Cancel sign-in";
export const ACCOUNT_SYNC_NOW_LABEL = "Sync now";
export const ACCOUNT_SHARE_LABEL = "Add a device or a person";
export const ACCOUNT_SIGN_OUT_LABEL = "Sign out…";
export const ACCOUNT_FORGET_LABEL = "Forget this account…";
export const ACCOUNT_DEVICES_LABEL = "Devices";
export const ACCOUNT_THIS_DEVICE = "This device";
export const ACCOUNT_RENAME_LABEL = "Rename";
export const ACCOUNT_RENAME_SAVE_LABEL = "Save";
export const ACCOUNT_CANCEL_LABEL = "Cancel";
export const ACCOUNT_ROLES_LABEL = "Roles";
/** What a failed call says when Rust gave no sentence of its own. */
export const ACCOUNT_ACTION_FAILED = "keeper couldn't reach your account.";
/** Said when a restore left this device's listening off though it was on here (AD-329). */
export const ACCOUNT_LISTENING_OFF_SENTENCE =
  "Listening for your wake phrase was on for this device.";
export const ACCOUNT_LISTENING_ON_LABEL = "Turn listening on";
export const ACCOUNT_LISTENING_KEEP_OFF_LABEL = "Keep it off";
/** What a failed "Turn listening on" says when Rust gave no sentence of its own. */
export const ACCOUNT_LISTENING_FAILED = "keeper couldn't turn listening on.";
/** What a failed "Keep it off" says when Rust gave no sentence of its own. */
export const ACCOUNT_LISTENING_KEEP_OFF_FAILED = "keeper couldn't save your listening choice.";

/**
 * What signing out does, and — the question people actually have — what it
 * does not. Its verb is "signs out", never "forgets": "Forget this account…"
 * sits beside it and means deleting the account from this device.
 */
export function signOutSentence(name: string): string {
  return `keeper signs you out of ${name} on this device and stops applying your account's settings here. The account stays set up, so you can sign in again without a link. The files in the repository are kept, and your other devices stay signed in.`;
}

/** What forgetting does. The server repository is named because it is the thing people fear for. */
export function forgetSentence(name: string): string {
  return `keeper signs out of ${name}, deletes this device's copy of your account's settings and its account.toml, and stops applying them. The repository on the server is untouched, and so are your other devices and other people.`;
}

/**
 * The line under the status while settings are actually travelling (Epic 84,
 * UX-DR118; fix R18): that settings sync with the account, and — only when
 * there is something — what the person's other devices use that this one does
 * not. It carries no time: the status sentence above already says when. Each
 * count is left out at zero rather than said as "0 drives", and the whole
 * second sentence is absent when every count is (AD-27).
 */
export function accountSyncLine(vm: OrgAccountVm): string {
  const synced = "Your settings sync with this account.";
  const counts = [
    [vm.offers.drives.length, "drive", "drives"],
    [vm.offers.providers.length, "bot provider", "bot providers"],
    [vm.offers.matrix.length, "Matrix account", "Matrix accounts"],
  ] as const;
  const offered = counts
    .filter(([n]) => n > 0)
    .map(([n, one, many]) => `${n} ${n === 1 ? one : many}`);
  return offered.length === 0 ? synced : `${synced} On your other devices: ${offered.join(", ")}.`;
}

export function AccountSection({ open }: { open: boolean }) {
  const vm = useAccountStore((s) => s.vm);

  // "Whenever Settings opens" (spec §2.3): a hand-edited ~/.keeper/account.toml
  // is read here, so the section is never older than the file.
  useEffect(() => {
    if (!open) {
      return;
    }
    let abandoned = false;
    void accountState()
      .then((next) => {
        if (!abandoned) {
          accountStore.getState().setVm(next);
        }
      })
      .catch(() => {
        // The subscription's last snapshot stands.
      });
    return () => {
      abandoned = true;
    };
  }, [open]);

  return (
    <section
      aria-labelledby="settings-account-title"
      className="flex min-w-0 flex-col gap-2 border-border border-b pb-3 text-sm"
    >
      <p id="settings-account-title" className="font-medium">
        {ACCOUNT_SECTION_TITLE}
      </p>
      {vm.configured ? (
        <ConfiguredAccount vm={vm} />
      ) : (
        <>
          <p className="text-muted-foreground">{ACCOUNT_SECTION_NOTE}</p>
          <SetupLinkField id="settings-account-setup-link" />
          <Faults faults={vm.faults} />
        </>
      )}
    </section>
  );
}

type Confirming = "signOut" | "forget";

function ConfiguredAccount({ vm }: { vm: OrgAccountVm }) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [shareOpen, setShareOpen] = useState(false);
  // Whether the dialog is open and which one it is are kept apart, so the
  // dialog keeps its words while it animates closed: clearing the kind on
  // close would turn a closing Forget dialog into a Sign out one for the
  // length of the exit animation.
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirming, setConfirming] = useState<Confirming>("signOut");
  const name = vm.name ?? ACCOUNT_SECTION_TITLE;
  const signedIn = vm.identity !== null;
  const signingIn = vm.state === "signingIn";
  // Sign in whenever nobody is signed in — signed out, a sign-in refused by
  // policy (blocked, no identity) or cancelled — so a refusal the
  // administrator has since fixed is one click away, not a Forget and a new
  // link. A dead grant keeps its identity (it is still who the person is) but
  // can no longer fetch anything, so it offers Sign in too.
  const offerSignIn = !signingIn && (!signedIn || vm.state === "needsSignIn");
  // Sync now whenever someone is signed in. Rust single-flights it, so it is
  // never harmful, and offline, "not synced yet" and sign in again are exactly
  // the states a person reaches for it in.
  const offerSync = signedIn && !signingIn;

  const ask = (kind: Confirming) => {
    setConfirming(kind);
    setConfirmOpen(true);
  };

  /** Run one account call, mirror its answer, and say Rust's sentence if it refuses. */
  const run = (call: () => Promise<OrgAccountVm>) => {
    setBusy(true);
    setError(null);
    void call()
      .then((next) => accountStore.getState().setVm(next))
      .catch((raw: unknown) => setError(syncErrorMessage(raw, ACCOUNT_ACTION_FAILED)))
      .finally(() => setBusy(false));
  };

  /**
   * One of the two answers to the listening offer, then the account read
   * again so the offer goes. Both read the switch from Rust first.
   */
  const answerListening = (answer: () => Promise<void>, failed: string) => {
    setBusy(true);
    setError(null);
    void (async () => {
      await answer();
      accountStore.getState().setVm(await accountState());
    })()
      .catch((raw: unknown) => setError(syncErrorMessage(raw, failed)))
      .finally(() => setBusy(false));
  };

  /**
   * "Turn listening on": the listening verb every menu calls
   * (`voice_wake_toggle`, which asks for the recogniser and microphone by
   * name), but only once Rust says the switch is off — a toggle on an offer
   * older than the switch would turn listening off.
   */
  const turnListeningOn = () =>
    answerListening(async () => {
      const wake = await voiceWakeGet();
      if (wake.enabled) {
        voiceStore.getState().applyWake(wake);
      } else {
        await toggleVoiceListening();
      }
    }, ACCOUNT_LISTENING_FAILED);

  /**
   * "Keep it off" (Epic 85, F15): the person's choice written as one, so it
   * travels to their other devices. Never the toggle — a toggle can arm —
   * but `voice_wake_set(false, …)` with the phrases Rust holds, so nothing
   * the person did not type changes and the microphone is never armed.
   */
  const keepListeningOff = () =>
    answerListening(async () => {
      const wake = await voiceWakeGet();
      voiceStore.getState().applyWake(await voiceWakeSet(false, wake.phrase, wake.stopPhrase));
    }, ACCOUNT_LISTENING_KEEP_OFF_FAILED);

  return (
    <>
      {vm.identity !== null ? (
        <div className="flex flex-col gap-1">
          <p>
            <span className="font-medium">{vm.identity.displayName}</span>{" "}
            <span className="font-mono text-muted-foreground">{vm.identity.login}</span>
          </p>
          <p className="text-muted-foreground">
            {name}
            {vm.issuerHost !== null && (
              <>
                {" · "}
                <span className="break-all font-mono">{vm.issuerHost}</span>
              </>
            )}
          </p>
          {vm.identity.roles.length > 0 && (
            <ul aria-label={ACCOUNT_ROLES_LABEL} className="flex flex-wrap gap-1">
              {vm.identity.roles.map((role) => (
                <li key={role}>
                  <Badge variant="outline">{role}</Badge>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <p className="text-muted-foreground">
          {name}
          {vm.issuerHost !== null && (
            <>
              {" · "}
              <span className="break-all font-mono">{vm.issuerHost}</span>
            </>
          )}
        </p>
      )}

      {vm.sentence !== null && <p role="status">{vm.sentence}</p>}
      {vm.restore.sentence !== null && <p>{vm.restore.sentence}</p>}
      {vm.restore.pending.map((line) => (
        <p key={line}>{line}</p>
      ))}
      {vm.restore.listeningOff && (
        <div className="flex flex-col items-start gap-1">
          <p>{ACCOUNT_LISTENING_OFF_SENTENCE}</p>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={turnListeningOn}
            >
              {ACCOUNT_LISTENING_ON_LABEL}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={keepListeningOff}
            >
              {ACCOUNT_LISTENING_KEEP_OFF_LABEL}
            </Button>
          </div>
        </div>
      )}
      {/* Only where settings do travel: a blocked directory or a dead grant is
          exactly when they do not, and the status above says so. */}
      {(vm.state === "ready" || vm.state === "syncing" || vm.state === "offline") && (
        <p className="text-muted-foreground">{accountSyncLine(vm)}</p>
      )}
      <Faults faults={vm.faults} />
      {error !== null && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}

      <div className="flex flex-wrap gap-2">
        {signingIn && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() =>
              void accountCancelSignIn().catch((raw: unknown) =>
                setError(syncErrorMessage(raw, ACCOUNT_ACTION_FAILED)),
              )
            }
          >
            {ACCOUNT_CANCEL_SIGN_IN_LABEL}
          </Button>
        )}
        {offerSignIn && (
          <Button type="button" size="sm" disabled={busy} onClick={() => run(accountSignIn)}>
            {ACCOUNT_SIGN_IN_LABEL}
          </Button>
        )}
        {offerSync && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => run(() => accountSync(true))}
          >
            {ACCOUNT_SYNC_NOW_LABEL}
          </Button>
        )}
        {signedIn && (
          <Button type="button" variant="outline" size="sm" onClick={() => setShareOpen(true)}>
            {ACCOUNT_SHARE_LABEL}
          </Button>
        )}
      </div>

      {signedIn && vm.devices.length > 0 && (
        <DeviceList
          devices={vm.devices}
          onRename={(next) => run(() => accountRenameDevice(next))}
        />
      )}

      <div className="flex flex-wrap gap-2">
        {signedIn && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => ask("signOut")}
          >
            {ACCOUNT_SIGN_OUT_LABEL}
          </Button>
        )}
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={() => ask("forget")}
        >
          {ACCOUNT_FORGET_LABEL}
        </Button>
      </div>

      <AccountShareSheet open={shareOpen} onOpenChange={setShareOpen} />

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {confirming === "forget" ? `Forget ${name}?` : `Sign out of ${name}?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {confirming === "forget" ? forgetSentence(name) : signOutSentence(name)}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{ACCOUNT_CANCEL_LABEL}</AlertDialogCancel>
            {confirming === "forget" ? (
              <AlertDialogAction variant="destructive" onClick={() => run(accountForget)}>
                Forget account
              </AlertDialogAction>
            ) : (
              <AlertDialogAction onClick={() => run(accountSignOut)}>Sign out</AlertDialogAction>
            )}
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

/**
 * The person's devices from `<login>/devices/`, this one marked and renameable.
 *
 * Rename is a disclosure, so its toggle says whether it is open and what it
 * opens; opening moves focus into the field, and Save or Cancel hands it back
 * to the toggle rather than dropping it on `body` (EXPERIENCE.md §Accessibility).
 */
function DeviceList({
  devices,
  onRename,
}: {
  devices: AccountDeviceVm[];
  onRename: (name: string) => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");
  const formId = useId();
  const toggleRef = useRef<HTMLButtonElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (renaming) {
      fieldRef.current?.focus();
    }
  }, [renaming]);
  const finish = () => {
    setRenaming(false);
    toggleRef.current?.focus();
  };
  return (
    <div className="flex flex-col gap-1">
      <p className="text-muted-foreground text-xs">{ACCOUNT_DEVICES_LABEL}</p>
      <ul aria-label={ACCOUNT_DEVICES_LABEL} className="flex flex-col gap-1">
        {devices.map((device) => (
          <li key={device.slug} className="flex flex-col gap-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="min-w-40 flex-1">
                <span className="font-mono">{device.name}</span>
                {device.class !== null && (
                  <span className="text-muted-foreground">
                    {" "}
                    · {DEVICE_CLASS_LABEL[device.class]}
                  </span>
                )}
                {device.platform !== null && (
                  <span className="text-muted-foreground"> · {device.platform}</span>
                )}
              </span>
              {device.thisDevice && (
                <>
                  <Badge variant="secondary">{ACCOUNT_THIS_DEVICE}</Badge>
                  <Button
                    ref={toggleRef}
                    type="button"
                    variant="outline"
                    size="sm"
                    aria-expanded={renaming}
                    aria-controls={renaming ? formId : undefined}
                    onClick={() => {
                      setDraft(device.name);
                      setRenaming(!renaming);
                    }}
                  >
                    {ACCOUNT_RENAME_LABEL}
                  </Button>
                </>
              )}
            </div>
            {device.thisDevice && renaming && (
              <form
                id={formId}
                className="flex flex-wrap gap-2 rounded-md border border-border p-2"
                onSubmit={(event) => {
                  event.preventDefault();
                  const next = draft.trim();
                  if (next !== "" && next !== device.name) {
                    onRename(next);
                  }
                  finish();
                }}
              >
                <Input
                  ref={fieldRef}
                  aria-label={`${ACCOUNT_RENAME_LABEL} ${device.name}`}
                  className="min-w-40 flex-1 font-mono"
                  autoComplete="off"
                  spellCheck={false}
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                />
                <Button type="submit" size="sm" disabled={draft.trim() === ""}>
                  {ACCOUNT_RENAME_SAVE_LABEL}
                </Button>
                <Button type="button" variant="outline" size="sm" onClick={finish}>
                  {ACCOUNT_CANCEL_LABEL}
                </Button>
              </form>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Descriptor and account-file faults, each Rust's own sentence. */
function Faults({ faults }: { faults: string[] }) {
  if (faults.length === 0) {
    return null;
  }
  return (
    <ul className="flex flex-col gap-1">
      {faults.map((fault) => (
        <li key={fault} role="alert" className="text-destructive text-xs">
          {fault}
        </li>
      ))}
    </ul>
  );
}

import { useEffect, useRef, useState } from "react";
import { LoginScreen } from "@/components/auth/login-screen";
import { AppShell } from "@/components/layout/app-shell";
import { AtRestEncryptionChoice } from "@/components/settings/at-rest-encryption-choice";
import { Toaster } from "@/components/ui/sonner";
import { FirstRunWizard } from "@/components/wizard/first-run-wizard";
import { useSessionRestore } from "@/hooks/use-session-restore";
import { encryptionPosture } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";
import { useWizardStore, wizardStore } from "@/lib/stores/wizard";

function App() {
  // Attempt a one-shot boot session-restore before deciding what to render.
  useSessionRestore();
  const hydrated = useAccountsStore((s) => s.hydrated);
  const hasAccount = useAccountsStore((s) => s.accounts.length > 0);
  const addAccountOpen = useAddAccountStore((s) => s.open);
  const closeAddAccount = useAddAccountStore((s) => s.closeAddAccount);
  // First-run wizard (Story 6.8). `active` takes precedence over the `hasAccount`
  // gate below so adding an account mid-flow does not unmount the wizard;
  // `dismissed` lands a skipped fresh install in an empty inbox (not the login
  // screen). Both are session-scoped and never persisted.
  const wizardActive = useWizardStore((s) => s.active);
  const wizardDismissed = useWizardStore((s) => s.dismissed);

  // First-run at-rest-encryption gate (Story 2.6). Loaded once for a fresh
  // install (`!hasAccount`). `undefined` = still loading (hold the splash);
  // `null` = unchosen (show the choice); `true`/`false` = chosen (show login).
  // Distinguishing "still loading" (undefined) from "unchosen" (null) is
  // load-bearing so the choice never flashes before the posture resolves.
  const [postureChosen, setPostureChosen] = useState<boolean | null | undefined>(undefined);
  useEffect(() => {
    let cancelled = false;
    void encryptionPosture()
      .then((value) => {
        if (!cancelled) {
          setPostureChosen(value);
        }
      })
      .catch(() => {
        // On a read failure, treat the posture as chosen-off so the user is never
        // trapped before login (the honest default is FileVault only).
        if (!cancelled) {
          setPostureChosen(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // One-shot first-run auto-start of the wizard (Story 6.8). Fires at most once
  // (guarded by a ref) when the app has finished restoring, there are no accounts,
  // and the at-rest-encryption posture has resolved (not still loading / unchosen).
  // Deliberately NOT triggered by a later sign-out-of-last-account: the ref keeps
  // it a genuine first-run boot event only.
  const bootDecidedRef = useRef(false);
  useEffect(() => {
    // Only evaluate the first-run decision once the boot state is fully resolved
    // (hydrated + a resolved posture). This is a one-shot boot decision: the first
    // time we reach a resolved boot state we either auto-start (fresh install with
    // zero accounts) or lock the decision out forever. A later sign-out-of-last-
    // account therefore never auto-starts the wizard — the decision was already made
    // at boot, when an account was present.
    if (
      bootDecidedRef.current ||
      !hydrated ||
      postureChosen === undefined ||
      postureChosen === null
    ) {
      return;
    }
    bootDecidedRef.current = true;
    if (!hasAccount) {
      wizardStore.getState().start();
    }
  }, [hydrated, hasAccount, postureChosen]);

  // Decide the shell/login/splash content, then render it alongside a single
  // always-mounted <Toaster />. The Toaster lives ABOVE the hasAccount gate so a
  // toast survives the shell→login transition — e.g. when the LAST account is
  // signed out with an archive-delete that then fails, the surfacing toast must
  // outlive the unmounting shell + dialog (Story 5.7).
  const content = renderContent();

  return (
    <>
      <Toaster />
      {content}
    </>
  );

  function renderContent() {
    // Hold a minimal accessible splash until the restore attempt completes, so a
    // restorable user never flashes the login screen (no login-flash).
    if (!hydrated) {
      return (
        <div
          role="status"
          aria-label="Loading keeper"
          className="flex h-screen items-center justify-center bg-background text-foreground"
        >
          <span className="sr-only">Loading keeper</span>
        </div>
      );
    }

    // The first-run wizard's `active` flag takes precedence over the `hasAccount`
    // gate: adding an account mid-flow flips `hasAccount` true, but the wizard must
    // stay mounted through its discovery/login steps (Story 6.8, Design Notes).
    if (wizardActive) {
      return <FirstRunWizard />;
    }

    // No accounts yet → gate first sign-in behind the first-run encryption choice
    // when the posture is unchosen. Otherwise mount the shell, and layer the
    // add-account login overlay on top when the footer requests it (subsequent adds
    // are never gated — the addMode path below is unchanged).
    if (!hasAccount) {
      // Still loading the posture: keep holding the splash rather than flashing the
      // choice or the login form.
      if (postureChosen === undefined) {
        return (
          <div
            role="status"
            aria-label="Loading keeper"
            className="flex h-screen items-center justify-center bg-background text-foreground"
          >
            <span className="sr-only">Loading keeper</span>
          </div>
        );
      }
      if (postureChosen === null) {
        return <AtRestEncryptionChoice onResolved={() => setPostureChosen(false)} />;
      }
      // A resolved posture with the wizard dismissed (skipped/finished with zero
      // accounts) lands the user in an empty inbox — the shell (with its "Add an
      // account" footer) rather than the bare login screen. `dismissed` is set only
      // by the wizard's own finish(), so a sign-out-of-last-account still shows the
      // login screen here (it never sets `dismissed`). All other zero-account states
      // render the login screen unchanged.
      if (!wizardDismissed) {
        return <LoginScreen />;
      }
      // Fall through to the shell path below (empty inbox + reachable add-account
      // overlay).
    }

    return (
      <>
        <AppShell />
        {addAccountOpen && (
          <div className="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm">
            <LoginScreen addMode onDone={closeAddAccount} />
          </div>
        )}
      </>
    );
  }
}

export default App;

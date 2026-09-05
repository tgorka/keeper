import { useEffect, useRef, useState } from "react";
import { LoginScreen } from "@/components/auth/login-screen";
import { AppShell } from "@/components/layout/app-shell";
import { AtRestEncryptionChoice } from "@/components/settings/at-rest-encryption-choice";
import { NoBackgroundSyncDisclosure } from "@/components/settings/no-background-sync-disclosure";
import { Button } from "@/components/ui/button";
import { Toaster } from "@/components/ui/sonner";
import { FirstRunWizard } from "@/components/wizard/first-run-wizard";
import { useActiveChatReporter } from "@/hooks/use-active-chat-reporter";
import { useAppLifecycle } from "@/hooks/use-app-lifecycle";
import { useCapabilitiesHydrate } from "@/hooks/use-capabilities-hydrate";
import { useNavStatePersistence } from "@/hooks/use-nav-state-persistence";
import { useNotesOpenNote } from "@/hooks/use-notes-open-note";
import { useNotifyNavigate } from "@/hooks/use-notify-navigate";
import { useSessionRestore } from "@/hooks/use-session-restore";
import { useWebviewGuard } from "@/hooks/use-webview-guard";
import { encryptionPosture } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { useWizardStore, wizardStore } from "@/lib/stores/wizard";

/**
 * The way past the login screen with no account (Story 63.1, AD-180). A
 * conversation with a model needs no Matrix account, so the sign-in screen
 * stops being a wall: one control, under the form, that says what it opens
 * and what it does not close.
 */
export const NO_ACCOUNT_BOTS_LABEL = "Continue without an account";
export const NO_ACCOUNT_BOTS_NOTE =
  "Bots needs no Matrix account. You can sign in later from the menu.";

function App() {
  // Attempt a one-shot boot session-restore before deciding what to render.
  useSessionRestore();
  // Mirror the Rust-served per-platform capability handshake once at startup
  // (Story 12.2). Fire-and-forget: a failure keeps the safe default (every
  // optional surface absent) and never blocks boot.
  useCapabilitiesHydrate();
  // Subscribe once to the coarse notification-navigate seam (Story 10.4, Option B):
  // a notification click summons the app and lands it on the Inbox (message) or the
  // Bridges view (bridge alert). Coarse only — no exact-message deep landing in MVP.
  useNotifyNavigate();
  // Open the note the tray just created (Story 44.6, FR-160). At the root and
  // not in the notes view: the tray exists so the app window is optional for a
  // whole day (FR-102), so this event routinely arrives while another view is
  // on screen. Before this hook nothing listened, and the tray's New Note
  // created a note the user was never shown.
  useNotesOpenNote();
  // Drive the single Rust lifecycle entry from the webview `visibilitychange`
  // event on the reduced-capability (iOS) tier only (Epic 14-1): background
  // pauses each live sync loop gracefully, foreground routes through the same
  // sync-now kick as pull-to-refresh. Inert on desktop — Story 10.3 untouched.
  useAppLifecycle();
  // Report the currently-open Chat to the shared notify engine on the reduced-capability
  // (iOS) tier only (Story 14.3, AD-18): a foreground notification for the Chat already
  // on screen is suppressed. Inert on desktop — notification behavior is unchanged there.
  useActiveChatReporter();
  // Persist the last phone-stack level in Rust and restore it after a reload on the
  // reduced-capability (iOS) tier only (Story 14.4): a webview reload after a
  // content-process jettison lands the user exactly where they were, and a cold
  // launch starts fresh at the Inbox. Inert on desktop.
  useNavStatePersistence();
  // Reload a blank/frozen webview once (loop-guarded) on a resume that fails the
  // animation-frame liveness probe, on the reduced-capability (iOS) tier only
  // (Story 14.4, tauri#14371). Never reloads a healthy webview. Inert on desktop.
  useWebviewGuard();
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
  // Whether this build can talk to a model at all (AD-161): the no-account way
  // into Bots exists only where Bots does — absent, not disabled, otherwise.
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  // Whether sign-in was declined this session in favour of Bots (Story 63.1,
  // AD-180). Session-scoped like the wizard's `dismissed`, and its own flag
  // rather than that one: the wizard's means "the wizard was skipped", this
  // means "the login screen was", and each is set by the surface it names.
  const [signInSkipped, setSignInSkipped] = useState(false);

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
      {/* The one-time iOS lifecycle-honesty card (Story 14.2, FR-61). Mounted above
          the content gate, like the Toaster, so it overlays the shell the moment its
          own gates open (reduced tier + an Account + wizard closed + latch unshown);
          it renders null everywhere else. */}
      <NoBackgroundSyncDisclosure />
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
          className="flex h-dvh items-center justify-center bg-background text-foreground"
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
            className="flex h-dvh items-center justify-center bg-background text-foreground"
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
      // render the login screen — with, where this build has Bots, the one
      // explicit way past it (Story 63.1, AD-180): a conversation with a model
      // needs no account, and the phone that opens keeper for one must not
      // meet a wall. Signing in stays the form above it; the way past is a
      // control that says what it opens and that sign-in is still there.
      if (!wizardDismissed && !signInSkipped) {
        return (
          <>
            <LoginScreen />
            {bots && (
              <div className="fixed inset-x-0 bottom-0 flex flex-col items-center gap-1 px-6 pt-3 pb-[calc(var(--safe-bottom)+0.75rem)] text-center">
                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => {
                    // Land on Bots itself rather than the empty Inbox: the
                    // control names Bots, and an Inbox with nothing in it is
                    // not what was asked for.
                    primaryViewStore.getState().setView("bots");
                    setSignInSkipped(true);
                  }}
                >
                  {NO_ACCOUNT_BOTS_LABEL}
                </Button>
                <p className="text-muted-foreground text-xs">{NO_ACCOUNT_BOTS_NOTE}</p>
              </div>
            )}
          </>
        );
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

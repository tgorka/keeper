import { useEffect, useState } from "react";
import { LoginScreen } from "@/components/auth/login-screen";
import { AppShell } from "@/components/layout/app-shell";
import { AtRestEncryptionChoice } from "@/components/settings/at-rest-encryption-choice";
import { Toaster } from "@/components/ui/sonner";
import { useSessionRestore } from "@/hooks/use-session-restore";
import { encryptionPosture } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";

function App() {
  // Attempt a one-shot boot session-restore before deciding what to render.
  useSessionRestore();
  const hydrated = useAccountsStore((s) => s.hydrated);
  const hasAccount = useAccountsStore((s) => s.accounts.length > 0);
  const addAccountOpen = useAddAccountStore((s) => s.open);
  const closeAddAccount = useAddAccountStore((s) => s.closeAddAccount);

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
      return <LoginScreen />;
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

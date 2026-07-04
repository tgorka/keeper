import { LoginScreen } from "@/components/auth/login-screen";
import { AppShell } from "@/components/layout/app-shell";
import { useSessionRestore } from "@/hooks/use-session-restore";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useAddAccountStore } from "@/lib/stores/add-account";

function App() {
  // Attempt a one-shot boot session-restore before deciding what to render.
  useSessionRestore();
  const hydrated = useAccountsStore((s) => s.hydrated);
  const hasAccount = useAccountsStore((s) => s.accounts.length > 0);
  const addAccountOpen = useAddAccountStore((s) => s.open);
  const closeAddAccount = useAddAccountStore((s) => s.closeAddAccount);

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

  // No accounts yet → full-screen first sign-in. Otherwise mount the shell, and
  // layer the add-account login overlay on top when the footer requests it.
  if (!hasAccount) {
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

export default App;

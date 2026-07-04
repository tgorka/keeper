import { LoginScreen } from "@/components/auth/login-screen";
import { AppShell } from "@/components/layout/app-shell";
import { useSessionRestore } from "@/hooks/use-session-restore";
import { useAccountsStore } from "@/lib/stores/accounts";

function App() {
  // Attempt a one-shot boot session-restore before deciding what to render.
  useSessionRestore();
  const hydrated = useAccountsStore((s) => s.hydrated);
  const currentAccount = useAccountsStore((s) => s.currentAccount);

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

  return currentAccount ? <AppShell /> : <LoginScreen />;
}

export default App;

import { LoginScreen } from "@/components/auth/login-screen";
import { AppShell } from "@/components/layout/app-shell";
import { useAccountsStore } from "@/lib/stores/accounts";

function App() {
  const currentAccount = useAccountsStore((s) => s.currentAccount);
  return currentAccount ? <AppShell /> : <LoginScreen />;
}

export default App;

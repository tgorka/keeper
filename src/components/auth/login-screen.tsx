import { type FormEvent, useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { AccountVm, IpcError, IpcErrorCode } from "@/lib/ipc/client";
import { loginPassword } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";

/** Documentation link surfaced for the non-SSS error (Design Notes). */
const SSS_DOC_URL =
  "https://github.com/matrix-org/matrix-spec-proposals/blob/main/proposals/4186-simplified-sliding-sync.md";

/** Error codes rendered inline: the backend `IpcErrorCode`s plus a client-side
 * "missing fields" guard (the form is `noValidate`, so blank/whitespace input
 * would otherwise reach the backend as an opaque error). */
type FormErrorCode = IpcErrorCode | "missingFields";

/** Friendly, sentence-case copy per error code (no error codes shown). */
function errorCopy(code: FormErrorCode): string {
  switch (code) {
    case "missingFields":
      return "Enter your homeserver, username, and password.";
    case "slidingSyncUnsupported":
      return "This homeserver doesn't support Simplified Sliding Sync, which keeper requires.";
    case "invalidCredentials":
      return "Wrong username or password.";
    case "serverUnreachable":
      return "Couldn't reach that homeserver. Check the address and your connection.";
    case "unsupportedLoginType":
      return "This homeserver doesn't support password login.";
    default:
      return "Something went wrong signing in. Please try again.";
  }
}

/** Narrowing guard for the {@link IpcError} envelope thrown by the IPC client. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

/**
 * Full-screen password login (FR-1, FR-5).
 *
 * Collects homeserver + username + password, calls the typed `login_password`
 * command, renders the named error inline, and — on success — records the
 * returned non-secret {@link AccountVm} in the accounts store (which gates the
 * shell). The password field is cleared after every submit; the password never
 * lives in any store.
 */
export function LoginScreen() {
  const setCurrentAccount = useAccountsStore((s) => s.setCurrentAccount);
  const [homeserver, setHomeserver] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [errorCode, setErrorCode] = useState<FormErrorCode | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setErrorCode(null);
    // The form is `noValidate`; guard blank/whitespace-only input here so it
    // never reaches the backend as an opaque connection error.
    const trimmedHomeserver = homeserver.trim();
    const trimmedUsername = username.trim();
    if (trimmedHomeserver === "" || trimmedUsername === "" || password === "") {
      setErrorCode("missingFields");
      return;
    }
    setSubmitting(true);
    const submittedPassword = password;
    // Clear the password field immediately on submit — it never lingers in UI.
    setPassword("");
    try {
      const account: AccountVm = await loginPassword(
        trimmedHomeserver,
        trimmedUsername,
        submittedPassword,
      );
      setCurrentAccount(account);
    } catch (err) {
      setErrorCode(isIpcError(err) ? err.code : "internal");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="flex h-screen items-center justify-center bg-background p-6 text-foreground">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Sign in to keeper</CardTitle>
          <CardDescription>Connect your Matrix account to start chatting.</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col gap-4" onSubmit={handleSubmit} noValidate>
            <div className="flex flex-col gap-2">
              <Label htmlFor="homeserver">Homeserver</Label>
              <Input
                id="homeserver"
                name="homeserver"
                type="text"
                autoComplete="url"
                placeholder="example.org"
                value={homeserver}
                onChange={(e) => setHomeserver(e.target.value)}
                required
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                name="username"
                type="text"
                autoComplete="username"
                placeholder="alice"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                required
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                name="password"
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>

            {errorCode && (
              <Alert variant="destructive" className="bg-destructive/10">
                <AlertTitle>Couldn't sign in</AlertTitle>
                <AlertDescription>
                  {errorCopy(errorCode)}
                  {errorCode === "slidingSyncUnsupported" && (
                    <>
                      {" "}
                      <a href={SSS_DOC_URL} target="_blank" rel="noreferrer">
                        Learn more about Simplified Sliding Sync
                      </a>
                      .
                    </>
                  )}
                </AlertDescription>
              </Alert>
            )}

            <Button type="submit" disabled={submitting}>
              {submitting ? "Signing in…" : "Sign in"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}

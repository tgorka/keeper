import { type FormEvent, useEffect, useRef, useState } from "react";
import { BeeperCoverageDisclosure } from "@/components/auth/beeper-coverage-disclosure";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { AccountVm, IpcError, IpcErrorCode } from "@/lib/ipc/client";
import {
  beeperRequestCode,
  cancelBeeper,
  cancelOidc,
  loginBeeper,
  loginOidc,
  loginPassword,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";

/** Documentation link surfaced for the non-SSS error (Design Notes). */
const SSS_DOC_URL =
  "https://github.com/matrix-org/matrix-spec-proposals/blob/main/proposals/4186-simplified-sliding-sync.md";

/** Beeper status page linked from the "unavailable" failure state (Design Notes). */
const BEEPER_STATUS_URL = "https://status.beeper.com";

/** The permanent, non-dismissible subtitle on the Beeper tab (Acceptance). */
const BEEPER_SUBTITLE = "Unofficial API — may break without notice";

/** The distinct named failure copy for a Beeper login failure (Acceptance). */
const BEEPER_FAILURE = "Beeper login unavailable — this is an unofficial API and may have changed.";

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
    case "oauthUnsupported":
      return "This homeserver doesn't offer single sign-on (OIDC).";
    case "oauthTimedOut":
      return "Single sign-on timed out. It wasn't completed in the browser in time.";
    case "oauthFailed":
      return "Single sign-on didn't complete. Please try again.";
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

interface LoginScreenProps {
  /**
   * When `true`, this login adds another account to a signed-in session
   * (Story 2.1) rather than being the first sign-in. Renders a Cancel control
   * and calls {@link LoginScreenProps.onDone} on success or cancel.
   */
  addMode?: boolean;
  /** Called after a successful add, or when the user cancels (add mode only). */
  onDone?: () => void;
}

/**
 * Login surface (FR-1, FR-3, FR-5, AD-17).
 *
 * Two tabs: "Password" wraps the existing password + single-sign-on (OIDC) form
 * unchanged; "Beeper" drives the unofficial email-code flow. On success either
 * tab records the returned non-secret {@link AccountVm} in the accounts store
 * (`addAccount`, which gates/extends the shell) and, in `addMode`, calls
 * `onDone`. No token or session material lives in any store.
 */
export function LoginScreen({ addMode = false, onDone }: LoginScreenProps = {}) {
  const addAccount = useAccountsStore((s) => s.addAccount);

  return (
    <div className="flex h-screen items-center justify-center bg-background p-6 text-foreground">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>{addMode ? "Add an account" : "Sign in to keeper"}</CardTitle>
          <CardDescription>
            {addMode
              ? "Connect another Matrix account. Your other accounts keep syncing."
              : "Connect your Matrix account to start chatting."}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="password">
            <TabsList className="w-full">
              {/* Trigger text avoids the exact string "Password"/"Email" so a
                  tab panel's `aria-labelledby` name never collides with a field
                  label under Testing Library's `getByLabelText`. */}
              <TabsTrigger value="password">Password &amp; SSO</TabsTrigger>
              <TabsTrigger value="beeper">Beeper</TabsTrigger>
            </TabsList>
            <TabsContent value="password">
              <PasswordTab addMode={addMode} addAccount={addAccount} onDone={onDone} />
            </TabsContent>
            <TabsContent value="beeper">
              <BeeperTab addMode={addMode} addAccount={addAccount} onDone={onDone} />
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>
    </div>
  );
}

interface TabProps {
  addMode: boolean;
  addAccount: (account: AccountVm) => void;
  onDone?: () => void;
}

/**
 * Password + single-sign-on (OIDC) tab (FR-1, FR-5, Story 2.2). Collects
 * homeserver + username + password, calls `login_password`, or drives the OIDC
 * browser round-trip. The password field is cleared after every submit and never
 * lives in any store.
 */
function PasswordTab({ addMode, addAccount, onDone }: TabProps) {
  const [homeserver, setHomeserver] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [errorCode, setErrorCode] = useState<FormErrorCode | null>(null);
  const [submitting, setSubmitting] = useState(false);
  // `true` while an OIDC flow is pending (the browser round-trip). The form is
  // replaced with a "complete sign-in in your browser" state + Cancel.
  const [oidcPending, setOidcPending] = useState(false);

  // Mirror `oidcPending` into a ref so the unmount cleanup can read the latest
  // value without re-subscribing.
  const oidcPendingRef = useRef(false);
  useEffect(() => {
    oidcPendingRef.current = oidcPending;
  }, [oidcPending]);
  // On unmount (e.g. the add-account overlay is dismissed mid-flow), abort any
  // still-pending OIDC flow so no orphaned backend flow lingers until timeout.
  useEffect(
    () => () => {
      if (oidcPendingRef.current) {
        void cancelOidc().catch(() => {
          // Best-effort: the backend flow also times out on its own.
        });
      }
    },
    [],
  );

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
      addAccount(account);
      onDone?.();
    } catch (err) {
      setErrorCode(isIpcError(err) ? err.code : "internal");
    } finally {
      setSubmitting(false);
    }
  }

  async function handleOidc() {
    setErrorCode(null);
    // The homeserver is the only field OIDC needs; guard blank input.
    const trimmedHomeserver = homeserver.trim();
    if (trimmedHomeserver === "") {
      setErrorCode("missingFields");
      return;
    }
    setOidcPending(true);
    try {
      const account: AccountVm = await loginOidc(trimmedHomeserver);
      addAccount(account);
      onDone?.();
    } catch (err) {
      // A user cancel returns quietly to the form (no scary error); every other
      // named failure renders inline with Retry.
      if (isIpcError(err) && err.code === "oauthCancelled") {
        // no-op: fall through to the form
      } else {
        setErrorCode(isIpcError(err) ? err.code : "internal");
      }
    } finally {
      setOidcPending(false);
    }
  }

  async function handleCancelOidc() {
    // Ask the backend to abort the pending flow; the pending `loginOidc` above
    // then rejects with `oauthCancelled` and we return to the form.
    try {
      await cancelOidc();
    } catch {
      // Best-effort: even if the cancel command fails, the flow will time out.
    }
  }

  if (oidcPending) {
    return (
      <div className="flex flex-col gap-4">
        <p className="text-sm text-muted-foreground">Complete sign-in in your browser…</p>
        <Button type="button" variant="outline" onClick={handleCancelOidc}>
          Cancel
        </Button>
      </div>
    );
  }

  return (
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

      <div className="flex flex-col gap-2">
        <Button type="submit" disabled={submitting}>
          {submitting ? "Signing in…" : addMode ? "Add account" : "Sign in"}
        </Button>
        <Button type="button" variant="outline" disabled={submitting} onClick={handleOidc}>
          Sign in with single sign-on
        </Button>
        {addMode && (
          <Button type="button" variant="outline" disabled={submitting} onClick={() => onDone?.()}>
            Cancel
          </Button>
        )}
      </div>
    </form>
  );
}

/** The two-step state of the Beeper email-code flow. */
type BeeperStep = "email" | "code";

/**
 * Beeper unofficial email-code tab (FR-3, Story 2.3, AD-17). A permanent,
 * non-dismissible subtitle marks the API as unofficial. Step "email" asks only
 * for an email (no homeserver — it is fixed to `matrix.beeper.com`) and requests
 * a code; step "code" submits the emailed code. Every Beeper failure renders the
 * distinct named "unavailable" state with a Retry (returns to the email step) and
 * a status link. On unmount mid-flow the pending flow is cancelled.
 */
function BeeperTab({ addMode, addAccount, onDone }: TabProps) {
  const [step, setStep] = useState<BeeperStep>("email");
  const [email, setEmail] = useState("");
  const [code, setCode] = useState("");
  // `true` once a code has been requested / verification submitted and the login
  // has not yet resolved — mirrors the OIDC pending ref for the unmount cleanup.
  const [pending, setPending] = useState(false);
  // Client-side blank-input guard shown inline (mirrors the password tab).
  const [missingFields, setMissingFields] = useState(false);
  // `true` once a Beeper call has failed; renders the distinct named state.
  const [failed, setFailed] = useState(false);
  // Set to the returned account once `loginBeeper` succeeds; gates completion
  // behind the coverage disclosure (FR-7). While non-null the disclosure +
  // "I understand" acknowledgment is rendered instead of the form, and
  // `addAccount`/`onDone` run only on acknowledgment.
  const [pendingAccount, setPendingAccount] = useState<AccountVm | null>(null);

  // Mirror `pending` into a ref so the unmount cleanup reads the latest value.
  const pendingRef = useRef(false);
  useEffect(() => {
    pendingRef.current = pending;
  }, [pending]);
  // `true` once `beeper_request_code` has stored a request id in the backend
  // registry (we advanced to the code step) and the login has not yet completed.
  // The registry entry survives past `pending` going false (the code step sits
  // idle with `pending === false`), so the unmount cleanup must key off this, not
  // `pending`, or an abandoned code step would leak a registry entry.
  const flowStartedRef = useRef(false);
  // Guards the disclosure acknowledgment so a rapid double-click can't fire
  // `addAccount`/`onDone` twice before the surrounding UI tears down.
  const acknowledgedRef = useRef(false);
  // On unmount mid-flow (overlay dismissed, tab switched away), clear any pending
  // Beeper request id in the backend registry so no residue lingers.
  useEffect(
    () => () => {
      if (pendingRef.current || flowStartedRef.current) {
        void cancelBeeper().catch(() => {
          // Best-effort: an abandoned flow leaves only an in-memory entry.
        });
      }
    },
    [],
  );

  async function handleSendCode() {
    setMissingFields(false);
    setFailed(false);
    const trimmedEmail = email.trim();
    if (trimmedEmail === "") {
      setMissingFields(true);
      return;
    }
    setPending(true);
    try {
      await beeperRequestCode(trimmedEmail);
      // A request id now lives in the backend registry — mark the flow active so
      // an unmount before verification cancels it.
      flowStartedRef.current = true;
      setStep("code");
    } catch {
      // Every Beeper failure collapses into the one named unavailable state.
      setFailed(true);
    } finally {
      setPending(false);
    }
  }

  async function handleVerify() {
    setMissingFields(false);
    setFailed(false);
    const trimmedEmail = email.trim();
    const trimmedCode = code.trim();
    if (trimmedCode === "") {
      setMissingFields(true);
      return;
    }
    setPending(true);
    const submittedCode = trimmedCode;
    // Clear the code field immediately on submit — it never lingers in UI.
    setCode("");
    try {
      const account: AccountVm = await loginBeeper(trimmedEmail, submittedCode);
      // The backend consumed the request id (`take`) to complete login, so there
      // is no registry residue left to cancel on unmount.
      flowStartedRef.current = false;
      // Auth succeeded — hold the account and render the coverage disclosure
      // gate. `addAccount`/`onDone` run only on explicit acknowledgment (FR-7).
      setPendingAccount(account);
    } catch {
      setFailed(true);
    } finally {
      setPending(false);
    }
  }

  function handleRetry() {
    // Retry restarts at the email step (a fresh `beeper_request_code`), so a
    // stale/expired request id is simply replaced.
    setFailed(false);
    setCode("");
    setStep("email");
  }

  if (failed) {
    return (
      <div className="flex flex-col gap-4">
        <Alert variant="destructive" className="bg-destructive/10">
          <AlertTitle>Beeper login unavailable</AlertTitle>
          <AlertDescription>
            {BEEPER_FAILURE}{" "}
            <a href={BEEPER_STATUS_URL} target="_blank" rel="noreferrer">
              Check Beeper status
            </a>
            .
          </AlertDescription>
        </Alert>
        <div className="flex flex-col gap-2">
          <Button type="button" onClick={handleRetry}>
            Retry
          </Button>
          {addMode && (
            <Button type="button" variant="outline" onClick={() => onDone?.()}>
              Cancel
            </Button>
          )}
        </div>
      </div>
    );
  }

  if (pendingAccount !== null) {
    // Auth already succeeded and persisted in the backend; the only forward path
    // into the inbox is an explicit acknowledgment (no Cancel at this step).
    return (
      <div className="flex flex-col gap-4">
        <BeeperCoverageDisclosure />
        <Button
          type="button"
          onClick={() => {
            if (acknowledgedRef.current) {
              return;
            }
            acknowledgedRef.current = true;
            addAccount(pendingAccount);
            onDone?.();
          }}
        >
          I understand
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-muted-foreground">{BEEPER_SUBTITLE}</p>

      <div className="flex flex-col gap-2">
        <Label htmlFor="beeper-email">Email</Label>
        <Input
          id="beeper-email"
          name="beeper-email"
          type="email"
          autoComplete="email"
          placeholder="you@example.com"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          disabled={step === "code" || pending}
          required
        />
      </div>

      {step === "code" && (
        <div className="flex flex-col gap-2">
          <Label htmlFor="beeper-code">Login code</Label>
          <Input
            id="beeper-code"
            name="beeper-code"
            type="text"
            inputMode="numeric"
            autoComplete="one-time-code"
            placeholder="Enter the emailed code"
            value={code}
            onChange={(e) => setCode(e.target.value)}
            disabled={pending}
            required
          />
        </div>
      )}

      {missingFields && (
        <Alert variant="destructive" className="bg-destructive/10">
          <AlertTitle>Couldn't sign in</AlertTitle>
          <AlertDescription>
            {step === "email" ? "Enter your Beeper email." : "Enter the emailed code."}
          </AlertDescription>
        </Alert>
      )}

      <div className="flex flex-col gap-2">
        {step === "email" ? (
          <Button type="button" disabled={pending} onClick={handleSendCode}>
            {pending ? "Sending…" : "Send code"}
          </Button>
        ) : (
          <Button type="button" disabled={pending} onClick={handleVerify}>
            {pending ? "Verifying…" : "Verify"}
          </Button>
        )}
        {addMode && (
          <Button type="button" variant="outline" disabled={pending} onClick={() => onDone?.()}>
            Cancel
          </Button>
        )}
      </div>
    </div>
  );
}

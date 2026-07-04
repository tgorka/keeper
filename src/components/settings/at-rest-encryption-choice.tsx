import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { setEncryptionPosture } from "@/lib/ipc/client";

/**
 * First-run choice title (Story 2.6, UX-DR17). Voice rules (UX-DR10): sentence
 * case, no exclamation marks, glossary noun capitalized.
 */
export const CHOICE_TITLE = "Encrypt your Matrix stores at rest";

/**
 * Explains what the passphrase covers and that it is generated and kept only in
 * the Keychain. Names the consequence plainly; no softening.
 */
export const CHOICE_EXPLANATION =
  "Your Matrix session and crypto state can be encrypted at rest with a passphrase. Turning " +
  "this on generates a passphrase kept only in your Keychain; it applies to every Account you " +
  "add.";

/** The default-off switch label. Glossary noun Matrix stays capitalized. */
export const CHOICE_SWITCH_LABEL = "Encrypt Matrix stores with a passphrase";

/**
 * Shown when persisting the choice fails, so Continue is never a silent no-op —
 * the user gets honest feedback and can retry. Voice rules: no exclamation.
 */
export const CHOICE_SAVE_ERROR = "Could not save your choice. Try again.";

/**
 * The shared, honest storage-posture sentences reused by the first-run choice and
 * the Settings dialog (AD-22, UX-DR17). States plainly that `keeper.db` and
 * `archive.db` are not passphrase-encrypted in this version and rely on FileVault.
 */
export const STORAGE_HONESTY_SENTENCE =
  "keeper.db and archive.db are not passphrase-encrypted in this version and rely on your Mac's " +
  "FileVault.";

/** SDK-store status copy when the account stores are passphrase-encrypted. */
export const SDK_STORE_ENCRYPTED_STATUS = "Matrix stores are passphrase-encrypted.";

/** SDK-store status copy when the account stores rely on FileVault only. */
export const SDK_STORE_UNENCRYPTED_STATUS = "Matrix stores are not encrypted — FileVault only.";

/**
 * SDK-store status copy shown while the posture is still loading, so the Settings
 * surface never momentarily claims "not encrypted" before the real posture
 * resolves (an honesty-focused surface must not flash a wrong security claim).
 */
export const SDK_STORE_STATUS_LOADING = "Checking Matrix store encryption…";

interface AtRestEncryptionChoiceProps {
  /** Called once the posture has been persisted, to advance to the login form. */
  onResolved: () => void;
}

/**
 * First-run at-rest encryption choice (Story 2.6, AD-22, NFR-10). Shown before the
 * first Account is added on a fresh install, while the posture is unchosen. A
 * default-off Switch offers passphrase encryption for the per-account Matrix SDK
 * stores; Continue persists the chosen posture (`setEncryptionPosture`) and then
 * calls `onResolved`. Read-only afterwards — the posture is not re-prompted, and
 * the passphrase is generated in Rust and stored only in the Keychain (nothing
 * secret crosses IPC).
 */
export function AtRestEncryptionChoice({ onResolved }: AtRestEncryptionChoiceProps) {
  const [switchOn, setSwitchOn] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  // Guards a rapid double-click so Continue can only persist/advance once.
  const resolvedRef = useRef(false);

  async function handleContinue() {
    if (resolvedRef.current) {
      return;
    }
    resolvedRef.current = true;
    setSubmitting(true);
    setSaveFailed(false);
    try {
      await setEncryptionPosture(switchOn);
      onResolved();
    } catch {
      // Persisting the posture failed; surface it and allow a retry rather than
      // trapping the user before login with a Continue that silently does nothing.
      resolvedRef.current = false;
      setSubmitting(false);
      setSaveFailed(true);
    }
  }

  return (
    <div className="flex h-screen items-center justify-center bg-background p-6 text-foreground">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>{CHOICE_TITLE}</CardTitle>
          <CardDescription>{CHOICE_EXPLANATION}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex items-center justify-between gap-4">
            <Label htmlFor="at-rest-encryption">{CHOICE_SWITCH_LABEL}</Label>
            <Switch
              id="at-rest-encryption"
              checked={switchOn}
              onCheckedChange={setSwitchOn}
              disabled={submitting}
            />
          </div>
          <p className="text-sm text-muted-foreground">{STORAGE_HONESTY_SENTENCE}</p>
          {saveFailed && (
            <p role="alert" className="text-destructive text-sm">
              {CHOICE_SAVE_ERROR}
            </p>
          )}
        </CardContent>
        <CardFooter>
          <Button type="button" className="w-full" disabled={submitting} onClick={handleContinue}>
            Continue
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
}

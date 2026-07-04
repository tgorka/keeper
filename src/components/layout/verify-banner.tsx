/**
 * The global "verify this device" banner (Story 3.1, UX honesty).
 *
 * Shows only when at least one signed-in account's device is `Unverified` AND the
 * banner has not been dismissed this session ({@link useShowVerifyBanner}). Never
 * shows on `Unknown` (no nag before crypto has synced) and clears on `Verified`.
 * The CTA opens the global Settings dialog (the honest destination — the
 * interactive verify flow lands in Story 3.2); the dismiss button collapses the
 * banner to a persistent Settings badge for this session (never gone). Dismissal
 * is session-scoped (zustand only; no persistence), so a restart re-nudges a
 * still-unverified device.
 */
import { X } from "lucide-react";
import { Alert, AlertAction, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { encryptionStatusStore, useShowVerifyBanner } from "@/lib/stores/encryption-status";
import { settingsUiStore } from "@/lib/stores/settings-ui";

/** The honest, verbatim banner copy (Story 3.1 fixed string). */
export const VERIFY_BANNER_TEXT = "Verify this device to read encrypted history";

export function VerifyBanner() {
  const show = useShowVerifyBanner();
  if (!show) {
    return null;
  }

  return (
    <div className="shrink-0 px-3 pb-2">
      <Alert role="status" className="pr-24">
        <AlertDescription className="text-foreground">{VERIFY_BANNER_TEXT}</AlertDescription>
        <AlertAction className="flex items-center gap-1">
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => settingsUiStore.getState().setSettingsOpen(true)}
          >
            Verify
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label="Dismiss"
            onClick={() => encryptionStatusStore.getState().dismissBanner()}
          >
            <X aria-hidden="true" />
          </Button>
        </AlertAction>
      </Alert>
    </div>
  );
}

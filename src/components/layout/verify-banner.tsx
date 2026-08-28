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
import { primaryViewStore } from "@/lib/stores/primary-view";

/** The honest, verbatim banner copy (Story 3.1 fixed string). */
export const VERIFY_BANNER_TEXT = "Verify this device to read encrypted history";

export function VerifyBanner() {
  const show = useShowVerifyBanner();
  if (!show) {
    return null;
  }

  return (
    // `py-2`, not `pb-2`. The band had 12px either side and 8px below but ZERO
    // above, so the card's top edge sat glued to whatever band was above it —
    // an enclosed card whose four borders are the same 1px reads as thinner on
    // top when only three of them have any air. Equal gutters, DESIGN.md's own
    // 8px, so the edge is even on all four sides. There is no seam to add here:
    // an `Alert` is `rounded-lg border`, a self-enclosed object, and the panes
    // row below it is not its neighbour across a boundary.
    <div className="shrink-0 px-3 py-2">
      <Alert role="status" className="pr-24">
        <AlertDescription className="text-foreground">{VERIFY_BANNER_TEXT}</AlertDescription>
        <AlertAction className="flex items-center gap-1">
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => primaryViewStore.getState().setView("settings")}
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

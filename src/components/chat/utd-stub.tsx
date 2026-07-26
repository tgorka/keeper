/**
 * The undecryptable-event (UTD) timeline stub (Story 3.1, UX honesty).
 *
 * An event keeper could not decrypt yet renders this explicit, never-blank stub
 * instead of nothing. The honest text names the two recovery paths (verify this
 * device — Story 3.2 — or restore key backup — Story 3.3), and the inline
 * "Verify" action opens the global Settings dialog (the honest destination; the
 * interactive verify flow lands in 3.2). When room keys arrive the SDK re-maps
 * the item to a decrypted {@link MessageBubble} via a `Set` diff, so this stub is
 * transient.
 *
 * Not a live region: a `reset` batch can render many historical UTD rows at once,
 * and each would announce itself. Overrides the `Alert` primitive's default
 * assertive `role="alert"` with the static `note` role, per the
 * `history-boundary.tsx` precedent that only transient states are announced.
 */
import { Alert, AlertAction, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { settingsUiStore } from "@/lib/stores/settings-ui";

/** The honest, verbatim UTD copy (Story 3.1 fixed string). */
export const UTD_STUB_TEXT = "Can't decrypt yet — verify this device or restore key backup";

export function UtdStub() {
  return (
    <Alert role="note" className="my-3">
      <AlertDescription>{UTD_STUB_TEXT}</AlertDescription>
      <AlertAction>
        <Button
          type="button"
          variant="outline"
          size="xs"
          onClick={() => settingsUiStore.getState().setSettingsOpen(true)}
        >
          Verify
        </Button>
      </AlertAction>
    </Alert>
  );
}

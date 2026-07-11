import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { iosSyncDisclosureShownGet, iosSyncDisclosureShownSet } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { useWizardStore } from "@/lib/stores/wizard";

/**
 * The canonical iOS lifecycle-honesty sentence (Story 14.2, FR-61, UX-DR10/17).
 * The single source of truth: rendered verbatim by the one-time card and
 * permanently in Settings → Notifications, and transcribed one-to-one into
 * `docs/ios.md` by Story 15.2. Sentence case, no exclamation marks, honest
 * consequence-naming — never edit one copy without the others.
 */
export const NO_BACKGROUND_SYNC_SENTENCE =
  "On iPhone, keeper syncs and notifies only while open. Close it and messages wait on your " +
  "homeserver until you return — nothing is lost, and nothing here pretends to be push.";

/**
 * The badge-honesty note shown next to the canonical sentence in Settings →
 * Notifications (Story 14.2, FR-61). Disclosure copy only — the iOS app-icon
 * badge mechanism itself is Story 14.3. Sentence case, no exclamation marks.
 */
export const BADGE_NOT_LIVE_SENTENCE =
  "The app-icon badge is not a live count while keeper is closed; it reflects what keeper knew " +
  "when it was last open.";

/**
 * The one-time card's title (Story 14.2, FR-61). Sentence case, no exclamation
 * marks (project voice).
 */
export const NO_BACKGROUND_SYNC_TITLE = "Syncing on this iPhone";

/** The acknowledge button's label (Story 14.2). */
export const NO_BACKGROUND_SYNC_ACK_LABEL = "Got it";

/**
 * The one-time iOS lifecycle-honesty disclosure card (Story 14.2, FR-61).
 * Self-gating: renders `null` unless this is the reduced-capability (phone) tier,
 * at least one Account exists, the first-run wizard is not active, and the
 * device-global "shown" latch (persisted in the Rust `settings` k/v table) reads
 * unshown. One trigger covers both AC moments — the wizard's Done hand-off on a
 * fresh install and the first Inbox render for a restored Account both flip the
 * same gates. Best-effort and non-trapping: a failed latch read is treated as
 * already-shown (never nag), and acknowledging (or any dialog close) latches the
 * flag, swallows a persist failure, and hides the card for the session.
 */
export function NoBackgroundSyncDisclosure() {
  const reduced = useIsReducedCapabilityPlatform();
  const hasAccount = useAccountsStore((s) => s.accounts.length > 0);
  const wizardActive = useWizardStore((s) => s.active);
  // Tri-state persisted latch: `undefined` = still loading (render nothing),
  // `false` = never shown (the card is due), `true` = shown. A failed read maps to
  // `true` so the disclosure can never trap or nag on a broken settings table.
  const [shown, setShown] = useState<boolean | undefined>(undefined);
  // Session-scoped dismissal so a failed persist still hides the card until relaunch.
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    // Only probe the latch on the reduced tier — desktop never shows the card, so
    // it never pays the IPC round-trip either.
    if (!reduced) {
      return;
    }
    let cancelled = false;
    void iosSyncDisclosureShownGet()
      .then((value) => {
        if (!cancelled) {
          setShown(value);
        }
      })
      .catch(() => {
        // Read failure ⇒ treat as already shown (best-effort, never trap).
        if (!cancelled) {
          setShown(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [reduced]);

  if (!reduced || !hasAccount || wizardActive || dismissed || shown !== false) {
    return null;
  }

  const acknowledge = () => {
    setDismissed(true);
    // Best-effort one-way latch: a failed persist still hides the card for this
    // session (no toast); the card may then show once more after a relaunch.
    void iosSyncDisclosureShownSet().catch(() => {
      // Swallowed by design (non-trapping disclosure).
    });
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) {
          acknowledge();
        }
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{NO_BACKGROUND_SYNC_TITLE}</DialogTitle>
          <DialogDescription>{NO_BACKGROUND_SYNC_SENTENCE}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" onClick={acknowledge}>
            {NO_BACKGROUND_SYNC_ACK_LABEL}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

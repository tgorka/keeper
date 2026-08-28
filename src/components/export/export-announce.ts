/**
 * Saying what an export did (Story 45.21, FR-199, UX-DR83).
 *
 * # Why the two controls share this and not a component
 *
 * A note exports from a menu item and a file exports from a button, and the two
 * have no markup in common — but they must not have two vocabularies. A note
 * whose export was refused and a file whose export was refused are the same
 * event to the person who pressed the control, and this is the one function
 * that turns any outcome into something on screen.
 *
 * # A toast, and Reveal beside it
 *
 * An export leaves keeper. What somebody wants next is confirmation and a way
 * to go and look at the copy, which is exactly what the archive export does
 * (Story 5.5) — the same Sonner toast with the same Reveal-in-Finder action,
 * gated on `capabilities.revealInFileManager` so a platform with no file
 * manager gets no dead control.
 *
 * # Cancel says nothing
 *
 * A toast announcing that somebody closed a dialog is noise about their own
 * action. Nothing was called and nothing was written, so nothing is said.
 *
 * # Every sentence is Rust's
 *
 * The summary comes from `ExportReceiptVm` and the refusal from
 * `keeper_sync::export::ExportRefusal`. Nothing here counts files, names paths
 * or pluralises anything — see `keeper_sync::export`'s module doc for why the
 * words live where they can be tested on any machine.
 */

import { toast } from "sonner";
import type { ExportOutcome } from "@/lib/export/export-target";
import { revealPath } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";

/** The success toast's action, worded as every other Reveal in keeper. */
export const EXPORT_REVEAL_LABEL = "Reveal in Finder";

/** Turn one export outcome into what the person sees. */
export function announceExport(outcome: ExportOutcome): void {
  switch (outcome.status) {
    case "cancelled":
      return;
    case "refused":
      toast.error(outcome.reason);
      return;
    case "exported": {
      const { path, summary } = outcome.receipt;
      const canReveal = capabilitiesStore.getState().capabilities.revealInFileManager;
      toast.success(summary, {
        action: canReveal
          ? {
              label: EXPORT_REVEAL_LABEL,
              onClick: () => {
                // Best effort, like every other Reveal: a file manager that
                // will not open is not something to interrupt the person with
                // a second time.
                void revealPath(path).catch(() => undefined);
              },
            }
          : undefined,
      });
    }
  }
}

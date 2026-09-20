/**
 * The link between the Recording surfaces and the Notes vault's Recordings
 * space (Story 45.19, FR-197; the space itself is Story 44.3's, AD-79).
 *
 * Two questions, and nothing else:
 *
 * - **Are they linked?** {@link useRecordingsSpace} answers with the space, or
 *   `null`. A vault the user has not chosen, a vault whose space list will not
 *   read, and a vault whose Recordings space was deleted (Story 45.17 makes that
 *   possible, and 44.3's ledger makes it stick) are all the same answer, because
 *   the surface's response to all three is the same: no button. An affordance
 *   that navigates to a space that is not there is worse than an absent one.
 * - **Go there.** {@link openRecordingsSpace} switches the primary view and
 *   scopes the list.
 *
 * **The identity is `defaultKey`, never the name.** A default space is
 * renameable like any other (AD-79), so matching on "Recordings" would make the
 * button disappear for anyone who called theirs "Sessions" — a bug nobody would
 * connect to the rename. `notes-pane.tsx` reads the same key for the same
 * reason.
 */

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { openNotesSpace } from "@/hooks/use-notes-actions";
import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpaces } from "@/lib/ipc/client";
import {
  ensureNotesVaultsHydrated,
  notesVaultsStore,
  useNotesVaultsStore,
} from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * The seeded default key the Recordings space carries in its own frontmatter
 * (`keeper.default`), mirroring `keeper_core::notes::default_spaces`.
 */
export const RECORDINGS_SPACE_KEY = "recordings";

/**
 * The Recordings space of the active vault, or `null` when the two are not
 * linked.
 *
 * Re-read whenever the active vault changes, because switching vaults switches
 * which spaces exist — a vault with no Recordings space must take the button
 * away, not keep the previous vault's.
 */
export function useRecordingsSpace(): NoteSpaceVm | null {
  const vaultId = useNotesVaultsStore((state) => state.activeVaultId);
  const [space, setSpace] = useState<NoteSpaceVm | null>(null);

  // The Recording surface can be the first thing opened in a session, so the
  // vault list may never have been read. Hydration is idempotent and shared.
  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

  useEffect(() => {
    if (vaultId === null) {
      setSpace(null);
      return;
    }
    let live = true;
    void notesSpaces(vaultId)
      .then((spaces) => {
        if (live) {
          setSpace(spaces.find((one) => one.defaultKey === RECORDINGS_SPACE_KEY) ?? null);
        }
      })
      .catch(() => {
        if (live) {
          setSpace(null);
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId]);

  return space;
}

/** Show Notes and enter its saved query through the same touch/park path as the rail. */
export function openRecordingsSpace(space: NoteSpaceVm): void {
  primaryViewStore.getState().setView("notes");
  const vaultId = notesVaultsStore.getState().activeVaultId;
  if (vaultId === null) return;
  void openNotesSpace(vaultId, space).catch((error: unknown) => {
    toast.error(syncErrorMessage(error, "keeper couldn't open the Recordings space."));
  });
}

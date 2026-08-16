/**
 * The two actions the resolution rule makes possible (Story 45.18, FR-196,
 * UX-DR79).
 *
 * From a note, open its file in the Files pane. From a markdown file inside the
 * notes vault, switch to the Notes tab with that note open. Both are 45.1
 * targets, which is why this story is a rule and two functions rather than a
 * navigation system: the panel list already knows how to show either.
 *
 * **Both use `openPanel`, not `setActiveTarget`, and that is the whole
 * behavioural decision in this file.** `setActiveTarget` is the single-click
 * gesture: it REPLACES what the active panel shows. Pressing "Show in Files" on
 * the note you are reading would therefore close that note — the note panel is
 * the active one — so arriving in Files you would have lost the thing you came
 * from, and going back to Notes you would find nothing open. `openPanel` is the
 * open-beside gesture, and a deliberate press that says "also show me this" is
 * exactly that gesture: a panel already holding the target is focused, and
 * otherwise it opens beside what you were reading.
 *
 * Before Story 46.12 the second sentence read "the note panel is the active one
 * and `NOTE_PANEL_LIMIT` means there is exactly one". The limit is gone and
 * there may now be several, which changes nothing here: the panel you pressed
 * the control in is the active one, and replacing it is still the wrong answer.
 *
 * **Story 49.2 makes the gesture an argument on `openNoteForFile` alone,
 * still defaulting to open-beside.** A space row is not a deliberate "also
 * show me this" press; it is a list row, and AD-90 gives a single click on a
 * row the REPLACE gesture (`notes-pane.tsx:289-296`). Left as it was, the same
 * click would grow the panel strip by one inside a vault and replace one panel
 * outside it — one gesture, two meanings, decided by configuration the person
 * pressing cannot see. The rejected alternative was letting the caller resolve
 * the note and call `setActiveTarget` itself: that would put a second copy of
 * the vault-switch ordering below outside this file, which is the drift the
 * paragraph above exists to prevent.
 *
 * Living here rather than in the two components that press them keeps the pair
 * from drifting: they are one feature in two directions, and the day one of
 * them learns something about focus the other has to learn it too.
 */
import { type NoteFolderVm, notesTree } from "@/lib/ipc/client";
import { NOTES_UNKNOWN_ERROR, notesVaultsStore, setActiveVault } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";
import { filePathForNote, notePathForFile, type VaultLocation } from "./rule";

/** The control that takes you from a note to its file. */
export const SHOW_IN_FILES_LABEL = "Show in Files";

/** The control that takes you from a vault file to its note. */
export const OPEN_IN_NOTES_LABEL = "Open in Notes";

/**
 * What a file that is in the vault but not in the index says.
 *
 * This is not "no such note": the file is right there and the reader is looking
 * at it. It is the cold scan not having reached it yet (FR-95's `indexed` flag
 * is false for exactly this window), or the file being one the indexer skips.
 * Naming the path is what makes the difference legible — a bare "not found"
 * over a file on screen reads as a bug in keeper, which sometimes it will be.
 */
export function noteNotIndexedSentence(vaultPath: string): string {
  return `keeper has no note indexed at ${vaultPath} yet. If the vault is still being read, try again in a moment.`;
}

/**
 * What is said if the file turns out not to be in a vault after all.
 *
 * The surfaces do not offer the action in that case — that is the story's
 * "absent rather than present-and-failing" — so reaching this means the vault
 * list changed between the render and the press, which is a real race when a
 * folder is unflagged in Settings while a panel is open.
 */
export const FILE_HAS_NO_NOTE_SENTENCE =
  "This file is not inside a notes vault any more, so it has no note to open.";

/**
 * Open the file behind this note, in the Files tab, beside what is already
 * open.
 *
 * Returns whether it acted, so a caller can decide not to offer the control at
 * all — which is what both callers do. `false` means the vault carries no
 * subfolder, or the note's path is not a plain relative descendant; in neither
 * case is there a file to show.
 */
export function showNoteInFiles(vault: VaultLocation, notePath: string): boolean {
  const resolved = filePathForNote(vault, notePath);
  if (resolved === null) {
    return false;
  }
  panelsStore.getState().openPanel({
    kind: "file",
    profileId: resolved.profileId,
    relativePath: resolved.relativePath,
  });
  primaryViewStore.getState().setView("files");
  return true;
}

/**
 * Open the note behind this file, in the Notes tab.
 *
 * Resolves to nothing on success and to a finished sentence on failure, in the
 * shape `TextFileFrame`'s `refusal` uses: the caller renders it where the
 * person pressed, rather than a toast that has gone by the time they look up.
 *
 * **The note id comes from `notes_tree`, and only from there.** The rule
 * resolves a path, not an identity; a note id survives a rename (FR-97) and a
 * path does not, so the index is the only thing that can say which note this
 * file is. Listing the file's own vault directory rather than the whole vault
 * is the same restraint `panel-strip.tsx` shows in listing a file's own folder:
 * one directory read, containment already in Rust, and no second command that
 * would need its own containment rule.
 *
 * **The path match is exact, deliberately.** Both `vaultPath` and the index's
 * `path` are produced by walking the same filesystem, so their case already
 * agrees; a case-insensitive fallback would be a guess dressed as robustness,
 * and on a case-sensitive volume it would open a different note that happens to
 * differ in case.
 *
 * The vault is made active before the panel target is set, because the notes
 * pane only shows the open note while its vault is the active one — setting the
 * target first would flash a pane with nothing in it.
 *
 * `options.gesture` picks between AD-90's pair for the final navigation only:
 * `"beside"` (the default, and what both 45.18 controls press) or `"replace"`,
 * for a caller whose trigger is a single click on a list row.
 *
 * `options.stillWanted` is asked after the awaits and before anything moves.
 * The two mutations here — the vault switch and the panel target — happen
 * INSIDE this function, so a caller racing two resolutions cannot guard them
 * from outside; a superseded press would otherwise still take the active
 * vault, the strip and the primary view on its way past. Answering `false`
 * makes the call resolve to `null`: nothing moved and there is nothing to say,
 * which is what the caller's success branch already does with `null`.
 */
export async function openNoteForFile(
  vaults: readonly VaultLocation[],
  profileId: string,
  relativePath: string,
  options: {
    gesture?: "beside" | "replace";
    stillWanted?: () => boolean;
  } = {},
): Promise<string | null> {
  const { gesture = "beside", stillWanted } = options;
  const resolved = notePathForFile(vaults, profileId, relativePath);
  if (resolved === null) {
    return FILE_HAS_NO_NOTE_SENTENCE;
  }
  let folder: NoteFolderVm;
  try {
    folder = await notesTree(resolved.vaultId, resolved.vaultDir);
  } catch (raw) {
    return syncErrorMessage(raw, NOTES_UNKNOWN_ERROR);
  }
  const row = folder.notes.find((note) => note.path === resolved.vaultPath);
  if (row === undefined) {
    return noteNotIndexedSentence(resolved.vaultPath);
  }
  if (stillWanted !== undefined && !stillWanted()) {
    return null;
  }
  if (notesVaultsStore.getState().activeVaultId !== resolved.vaultId) {
    await setActiveVault(resolved.vaultId);
    // **Checked, because `setActiveVault` cannot fail out loud.** It swallows a
    // rejected `notes_vault_set_active` into the mirror's `error` slot and
    // returns normally, so awaiting it proves nothing. Without this check the
    // sequence is: the switch fails and writes a sentence, then this function
    // navigates and reports success — and the later producer wins. The reader
    // arrives in Notes with no note open, because the pane only shows one while
    // its vault is active, and with nothing on screen saying why.
    //
    // So the failure is read back from the mirror rather than inferred from the
    // absence of a throw, and nothing navigates. Leaving the user where they
    // are, with the reason, beats moving them somewhere empty.
    const state = notesVaultsStore.getState();
    if (state.activeVaultId !== resolved.vaultId) {
      return state.error ?? NOTES_UNKNOWN_ERROR;
    }
  }
  if (stillWanted !== undefined && !stillWanted()) {
    return null;
  }
  const target = { kind: "note", vaultId: resolved.vaultId, noteId: row.id } as const;
  const panels = panelsStore.getState();
  if (gesture === "replace") {
    panels.setActiveTarget(target);
  } else {
    panels.openPanel(target);
  }
  primaryViewStore.getState().setView("notes");
  return null;
}

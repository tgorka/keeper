/**
 * The registry's `text` viewer: a file in a sync profile, loaded, in whichever
 * of its two views the reader last chose (Story 45.4, FR-177, AD-88), and — as
 * of Story 45.18 — knowing whether that file is also a note (FR-196, UX-DR79).
 *
 * **This is a binding, and almost nothing else.** Loading, dirty tracking and
 * saving are 45.6's `useTextFile`; the four states above a loaded file and the
 * mapping onto the toggle are {@link TextFileFrame}, shared with the note embed
 * Story 45.12 mounts. What is left here is the two things only this surface
 * knows: that the file is addressed by a sync profile, and — the part 45.4 had
 * to leave open — whether that profile's vault contains it.
 *
 * **It reads no path and joins nothing.** `file.profileId` and
 * `file.relativePath` go to Rust as they arrived; Rust re-resolves them through
 * `keeper_sync::browse`'s containment on every call (AD-65). `absolutePath` is
 * never touched here, and never rendered anywhere (FR-145). The vault question
 * is answered by `@/lib/vault-link`, whose rule is authored in
 * `keeper_core::vault_link` and pinned to it by a shared vector table — so the
 * webview is consulting Rust's decision rather than making its own.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { followExternalUrl, resolveWikilink } from "@/lib/notes/follow-link";
import { ensureNotesVaultsHydrated, useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { notePathForFile, OPEN_IN_NOTES_LABEL, openNoteForFile } from "@/lib/vault-link";
import type { ViewerProps } from "@/lib/viewers";
import { TextFileFrame } from "./text-file-frame";
import { useTextFile } from "./use-text-file";

/**
 * Test id for the sentence this surface leaves behind when an action it offered
 * could not finish — the note that is not indexed yet, the link that went
 * nowhere. A slot, so a test reads it rather than re-deriving the wording.
 */
export const TEXT_FILE_NOTICE_SLOT = "text-file-notice";

export function TextFileViewer({ file, entry }: ViewerProps): React.ReactElement {
  const state = useTextFile({ profileId: file.profileId, subpath: file.relativePath });
  const vaults = useNotesVaultsStore((each) => each.vaults);
  const [notice, setNotice] = useState<string | null>(null);

  // The Files tab can be the first surface a session opens, and nothing in it
  // reads the vault list — before this story only the notes pane hydrated the
  // mirror. Without this the resolution below would answer "no vault" for every
  // file until the user visited Notes, which is the shape of a feature that
  // works for whoever wrote it and nobody else.
  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

  /**
   * Where this file lives inside a notes vault, or `null`.
   *
   * **This is what Story 45.4 was waiting for.** 44.16's `notes_csv_read` /
   * `notes_csv_set_cell` are addressed by a notes vault id plus a
   * vault-relative target; a panel holds a sync profile id plus a
   * profile-relative path. Those are different identifiers over overlapping
   * bytes, and 45.4 declined to derive one from the other in the webview
   * because that is the frontend deciding which folders are vaults. It is now
   * derived — by Rust's own rule, mirrored and pinned — so a CSV inside a vault
   * opens as a table here exactly as it does inside a note, and a CSV outside
   * one still says why it cannot.
   *
   * `null` while the mirror is unread is the honest answer and not a guess: the
   * table appears when the list arrives, which is one frame, and claiming a
   * vault before reading the list would be worse than being a frame late.
   */
  const inVault = useMemo(
    () =>
      file.profileId === null
        ? null
        : notePathForFile(vaults ?? [], file.profileId, file.relativePath),
    [vaults, file.profileId, file.relativePath],
  );

  const openInNotes = useCallback(() => {
    if (file.profileId === null) {
      return;
    }
    void openNoteForFile(vaults ?? [], file.profileId, file.relativePath).then(setNotice);
  }, [vaults, file.profileId, file.relativePath]);

  /**
   * Follow a wikilink written in this file (Story 45.18).
   *
   * The second host of the decoration layer, and the reason the following lives
   * in `editor/follow-link.ts` rather than in the note editor: a `.md` file
   * opened from Files renders through the very same `livePreview`, so a
   * wikilink in it was `cursor: pointer` over dead text for exactly as long as
   * one in the editor was. It opens the note BESIDE this panel rather than
   * replacing it — the note and the file it came from are two things worth
   * having on screen at once, which is the whole of what a panel strip is for.
   */
  /**
   * The one vault id this surface hands downwards.
   *
   * Read once and shared, because a mutation sweep found that blanking the
   * preview's copy alone survived every test: the wikilink follower and the
   * markdown preview must resolve against the SAME vault or a note's embeds
   * point somewhere its links do not, and two literals is how that happens.
   * `""` is the decoration layer's own "no vault" spelling and `null` is the
   * preview host's; both mean the same thing here.
   */
  const vaultId = inVault?.vaultId ?? null;

  const openWikilink = useCallback(
    (target: string) => {
      void resolveWikilink(vaultId ?? "", target).then((result) => {
        setNotice(result.reason);
        if (result.note !== null) {
          panelsStore.getState().openPanel({
            kind: "note",
            vaultId: result.note.vaultId,
            noteId: result.note.id,
          });
        }
      });
    },
    [vaultId],
  );

  const openExternal = useCallback((url: string) => {
    void followExternalUrl(url).then(setNotice);
  }, []);

  /**
   * What the markdown preview resolves against.
   *
   * Rebuilt as one object per resolution rather than assembled inline, because
   * `mountMarkdownPreview` reads it once at construction: a fresh identity on
   * every render would be harmless, and a stale `vaultId` inside a memo keyed
   * on the wrong thing would silently resolve a note's embeds against the
   * previous file's vault.
   */
  const preview = useMemo(
    () => ({
      vaultId,
      onOpenLink: openWikilink,
      onOpenUrl: openExternal,
    }),
    [vaultId, openWikilink, openExternal],
  );

  const csv = useMemo(
    () => (inVault === null ? null : { vaultId: inVault.vaultId, target: inVault.vaultPath }),
    [inVault],
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Story 45.18: from a vault file, its note (FR-196, UX-DR79).

          Only for markdown, and only inside a vault. A PNG in the vault is an
          attachment and has no note; a `.md` beside the vault has none either,
          and for that one the control is ABSENT rather than present-and-failing
          — which is the story's own sentence.

          `entry.format` and never the extension: the registry is the only thing
          that decides what a file is (AD-87), and this surface was handed its
          answer rather than re-deriving one. */}
      {entry.format === "markdown" && inVault !== null && (
        <div className="flex shrink-0 justify-end border-b px-2 py-1">
          <Button size="xs" variant="ghost" onClick={openInNotes}>
            {OPEN_IN_NOTES_LABEL}
          </Button>
        </div>
      )}
      {notice === null ? null : (
        <p
          role="status"
          data-slot={TEXT_FILE_NOTICE_SLOT}
          className="shrink-0 border-b px-3 py-1 text-meta text-muted-foreground"
        >
          {notice}
        </p>
      )}
      <div className="min-h-0 flex-1">
        <TextFileFrame
          fileName={file.name}
          entry={entry}
          state={state}
          writeCaveat={file.writeCaveat}
          // The location's verdict, straight off the listing row the panel
          // opened this file from — Rust's own refusal sentence, which is what
          // keeps a session's `workspace/` file (AD-113) read-only and toolless
          // from the first frame instead of from the first refused save.
          writeRefusal={file.writeRefusal}
          csv={csv}
          // Story 50.4: this host holds the sync-profile address, so it is the
          // one that can offer a file's own properties. A file outside every
          // profile has no `profileId` and therefore no properties surface —
          // the same condition that already leaves it with no loader.
          properties={
            file.profileId === null
              ? null
              : { profileId: file.profileId, relativePath: file.relativePath }
          }
          preview={preview}
        />
      </div>
    </div>
  );
}

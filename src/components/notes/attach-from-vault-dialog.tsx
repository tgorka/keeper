/**
 * Attach a file the vault already holds, from inside the note (Story 46.11,
 * FR-188, FR-189, AD-103).
 *
 * # The door that was missing
 *
 * Story 45.13 built three entry points and, read as doors rather than as code
 * paths, only one of them reached a file keeper already syncs — the Files pane's
 * multiselection into {@link AttachToNoteDialog}, which chooses the **note** for
 * files that have already been selected. From the note editor there was exactly
 * one door and it opened onto the operating system's file picker. So "attach the
 * photograph I already have in keeper" meant leaving the note, finding the
 * primary view, selecting the row, and choosing the note you were already in.
 *
 * This is the other direction: the note is given, and you browse for the file.
 *
 * # It copies nothing, and that is the whole ruling
 *
 * The vault already holds these files and the sync engine already carries them.
 * Copying one into `attachments/` would duplicate bytes that are on every
 * machine this vault reaches, and leave two files that drift. So an attach from
 * here inserts an embed pointing at the file **where it already lives** —
 * `![[photos/holiday.png]]` — and nothing is written to disk except the note.
 *
 * That is not a promise this component keeps by being careful. It is a promise
 * it keeps by construction: **`notes_attach_sources` is the only thing in this
 * app that copies a file into a vault, and this component does not call it.** It
 * cannot: that command takes absolute paths, and every path here is already
 * vault-relative because {@link notesGallery} answered with vault-relative paths
 * (FR-145, AD-65). There is no code path from this dialog to a copy.
 *
 * # Why `notes_gallery` and not a third file tree
 *
 * The vault needed browsing and this app already browses folders twice —
 * `files-pane.tsx` over sync profiles, and the gallery block over one vault
 * folder. A third tree was the wrong answer and so was extracting the first
 * one:
 *
 * - **`files-pane.tsx` browses the wrong thing.** It is keyed on a *sync profile
 *   id* with profile-relative paths; a note names a *vault-relative* path in its
 *   own vault, and the resolution between the two identifiers is Story 45.18's
 *   and lives in Rust. Its tree is also a virtualised multi-select surface with
 *   create, delete and reveal wired through it — none of which this needs — and
 *   it would offer files in other synced folders, which this dialog cannot
 *   attach by reference at all.
 * - **`notes_gallery` already IS the vault's directory reader.** It goes through
 *   `keeper_sync::browse`, the repo's one directory reader, so the containment
 *   test, the built-in noise filter, the entry cap and the folders-then-files
 *   order are the same ones the Files pane shows. It answers with the
 *   vault-relative `relPath` a note embeds and with the one classifier's `kind`
 *   (AD-73). Nothing new was needed in Rust for the browsing at all.
 *
 * So this is {@link AttachToNoteDialog}'s shape with a file chooser where its
 * note chooser is: a list of rows, one verb per row, the reason where the button
 * would have been for a row that cannot be offered, and one outcome slot.
 *
 * # What it declines to offer, and says so
 *
 * A vault folder is mostly notes, so "why can't I attach this" is the first
 * question this surface would otherwise raise. Three rows carry a reason instead
 * of a button:
 *
 * - **A note.** {@link namesANote} — `export::names_a_note`'s rule, pinned to it
 *   by `attach-vectors.json` — calls a `.md` and an extensionless name a note.
 *   Embedding one is a transclusion, which is an edge in the vault graph rather
 *   than an attachment, and the Attachments panel would not list it.
 * - **A name no wikilink can spell.** {@link wikilinkNameable}. Asked before the
 *   offer rather than after it, because a `#` in a filename is legal on macOS and
 *   a button that answers with a refusal is worse than a row that says why.
 * - **A file the note already embeds.** The same fact, in the same words, as the
 *   Attachments panel's row: {@link ATTACHMENT_PRESENT_LABEL}.
 *
 * The dialog stays open after an attach. Several files from one folder is a real
 * thing to want, and the row flipping to "in the note" is the receipt.
 */
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { type NoteGalleryVm, notesGallery } from "@/lib/ipc/client";
import {
  attachmentName,
  embeddedAttachmentNames,
  namesANote,
  planAttachments,
  wikilinkNameable,
} from "@/lib/notes/attach";
import { ATTACHMENT_PRESENT_LABEL } from "./attachments-panel";

/** The dialog's accessible name, and the words on the item that opens it. */
export const ATTACH_FROM_VAULT_LABEL = "From a folder you sync";

/**
 * The promise, stated before anything is chosen.
 *
 * The "Watch for" the story named: the two doors do different things to the
 * user's disk, and which one is which has to be legible *before* they act. This
 * one copies nothing. The other one says so too, on its own menu item.
 */
export const ATTACH_FROM_VAULT_PROMISE =
  "These files are already in this note's folder, so keeper links them where they are. Nothing is copied.";

/** What a row's button says. The same verb the whole story is about. */
export const ATTACH_FROM_VAULT_ACTION = "Attach";

/** The accessible name of the list, so a test can read what is on offer. */
export const ATTACH_FROM_VAULT_LIST_TESTID = "attach-from-vault-entries";

/** Test id for the sentence the dialog leaves behind. One slot, because a
 *  person reads one outcome. */
export const ATTACH_FROM_VAULT_OUTCOME_TESTID = "attach-from-vault-outcome";

/** How the breadcrumb names the vault root, which has no name of its own here:
 *  the absolute path is never on screen (FR-145) and the vault's own folder name
 *  is not something this surface is told. */
export const VAULT_ROOT_LABEL = "This note's folder";

/** The way back up. A word, because a bare `..` in a list of file names reads
 *  as a file called `..`. */
export const ATTACH_FROM_VAULT_UP_LABEL = "Up one folder";

/** What the list says while a folder is being read. Story 43.8's word for the
 *  same wait, in the same app. */
export const ATTACH_FROM_VAULT_READING = "Reading…";

/** What an empty folder says. */
export const ATTACH_FROM_VAULT_EMPTY = "This folder is empty.";

/** What the list says under a folder the entry cap cut short. Said, never
 *  hidden — `browse`'s cap is 1000 entries and a browser that silently drops
 *  the rest is worse than one that admits it. */
export const ATTACH_FROM_VAULT_TRUNCATED =
  "This folder holds more than keeper lists at once, so some files are not shown.";

/** Why a row offers no button. Each is a fact about the file, not a mode. */
const REASON_IS_A_NOTE = "a note, not a file";
const REASON_UNNAMEABLE = "keeper cannot name this in a note";

export interface AttachFromVaultDialogProps {
  /** The vault the note lives in — the folder being browsed, and the frame every
   *  path here is in. */
  vaultId: string;
  /**
   * The buffer as the editor has it *now*.
   *
   * The buffer and not the last save, for {@link planAttachments}' reason: a
   * file attached, then attached again before the autosave fires, is a duplicate
   * that a check against disk would not see. Live, so a row flips to "in the
   * note" on the insert rather than on the next save.
   */
  body: string;
  /** Put this text where the caret is. The editor owns the caret. */
  onInsert: (text: string) => void;
  /** Close, whether or not anything was attached. */
  onClose: () => void;
}

export function AttachFromVaultDialog({
  vaultId,
  body,
  onInsert,
  onClose,
}: AttachFromVaultDialogProps) {
  /** The folder being browsed, vault-relative; `""` is the vault root. */
  const [folder, setFolder] = useState("");
  // `undefined` is "not read yet" and a listing is the answer — the same two
  // states, spelled the same way, as the note chooser's search. Collapsing them
  // would make the dialog say "this folder is empty" for the first frame of
  // every folder, which is a claim and is not true yet.
  const [listing, setListing] = useState<NoteGalleryVm | undefined>(undefined);
  const [outcome, setOutcome] = useState<string | null>(null);

  useEffect(() => {
    setListing(undefined);
    let live = true;
    void notesGallery(vaultId, folder)
      .then((found) => {
        // The reply echoes the folder it listed, which is why it does: a reply
        // that arrives after the person has navigated on would otherwise render
        // one folder's files under another folder's breadcrumb.
        if (live && found.folder === folder) {
          setListing(found);
        }
      })
      .catch(() => {
        if (live) {
          // An empty listing plus a sentence, never a stale one: offering the
          // previous folder's files would attach a file the person is no longer
          // looking at.
          setListing({ folder, items: [], truncated: false, problem: null });
          setOutcome("keeper could not read that folder just now.");
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, folder]);

  const embedded = embeddedAttachmentNames(body);
  const segments = folder === "" ? [] : folder.split("/");

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{ATTACH_FROM_VAULT_LABEL}</DialogTitle>
          <DialogDescription>{ATTACH_FROM_VAULT_PROMISE}</DialogDescription>
        </DialogHeader>

        {/* Where you are, and the one way back. A breadcrumb rather than a tree,
            because a tree here would be the third one — see the module comment.
            The root's label is a phrase and not a path: this dialog is never
            told where the vault is and must not be (AD-65, FR-145). */}
        <p className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
          <span className="min-w-0 truncate font-mono">
            {[VAULT_ROOT_LABEL, ...segments].join(" / ")}
          </span>
          {folder === "" ? null : (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-6 shrink-0"
              onClick={() => setFolder(segments.slice(0, -1).join("/"))}
            >
              {ATTACH_FROM_VAULT_UP_LABEL}
            </Button>
          )}
        </p>

        <ul data-testid={ATTACH_FROM_VAULT_LIST_TESTID} className="max-h-64 overflow-auto text-sm">
          {listing === undefined ? (
            <li className="px-1 py-2 text-muted-foreground text-xs">{ATTACH_FROM_VAULT_READING}</li>
          ) : listing.problem !== null ? (
            // Rust's sentence, verbatim: the reason is Rust's — missing,
            // unreadable, or a path that escapes the vault — and this surface is
            // not told which of the three it was.
            <li className="px-1 py-2 text-muted-foreground text-xs">{listing.problem}</li>
          ) : listing.items.length === 0 ? (
            <li className="px-1 py-2 text-muted-foreground text-xs">{ATTACH_FROM_VAULT_EMPTY}</li>
          ) : (
            listing.items.map((item) =>
              item.kind === "folder" ? (
                <li key={item.relPath} className="flex min-w-0 items-center gap-2 px-1 py-1">
                  {/* The whole row is the affordance, because the only thing a
                      folder does here is open. Its accessible name carries the
                      folder's own name: a listing of nine folders is otherwise
                      nine identically-named controls. */}
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-6 min-w-0 flex-1 justify-start px-1"
                    onClick={() => {
                      setOutcome(null);
                      setFolder(item.relPath);
                    }}
                  >
                    <span className="min-w-0 truncate font-mono text-meta">{item.name}/</span>
                  </Button>
                </li>
              ) : (
                <li key={item.relPath} className="flex min-w-0 items-center gap-2 px-1 py-1">
                  <span
                    className="min-w-0 flex-1 truncate font-mono text-meta"
                    title={item.relPath}
                  >
                    {item.name}
                  </span>
                  {/* Rust's word for what this file is (AD-73), which this
                      surface may show where the Attachments panel's body rows
                      may not: there the kind came from a session index matched
                      by NAME, and here it came from the classifier looking at
                      this very entry. */}
                  <span className="shrink-0 text-muted-foreground text-xs">{item.kind}</span>
                  {embedded.has(attachmentName(item.relPath).toLowerCase()) ? (
                    <span className="shrink-0 text-muted-foreground text-xs">
                      {ATTACHMENT_PRESENT_LABEL}
                    </span>
                  ) : namesANote(item.relPath) ? (
                    <span className="shrink-0 text-muted-foreground text-xs">
                      {REASON_IS_A_NOTE}
                    </span>
                  ) : !wikilinkNameable(item.relPath) ? (
                    <span className="shrink-0 text-muted-foreground text-xs">
                      {REASON_UNNAMEABLE}
                    </span>
                  ) : (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      className="h-6 shrink-0"
                      // The path that will land in the note, not the verb: a
                      // folder of eight photographs is eight identical "Attach"
                      // buttons to anyone not looking at the screen.
                      aria-label={`${ATTACH_FROM_VAULT_ACTION} ${item.relPath}`}
                      onClick={() => {
                        // The whole of the in-vault attach. No IPC, because
                        // there is nothing left to ask: the path is already the
                        // one the note names, `browse` already proved it is in
                        // the vault, and copying is what this door exists not to
                        // do. `planAttachments` is the same decision the other
                        // three entry points make, so the bytes are the same.
                        const plan = planAttachments(body, [item.relPath]);
                        if (plan.text !== "") {
                          onInsert(plan.text);
                        }
                        setOutcome(
                          plan.refusal ??
                            `Put ${item.name} in this note, where it already lives — no copy.`,
                        );
                      }}
                    >
                      {ATTACH_FROM_VAULT_ACTION}
                    </Button>
                  )}
                </li>
              ),
            )
          )}
        </ul>

        {listing?.truncated === true ? (
          <p className="text-muted-foreground text-xs">{ATTACH_FROM_VAULT_TRUNCATED}</p>
        ) : null}

        {outcome === null ? null : (
          <p
            data-testid={ATTACH_FROM_VAULT_OUTCOME_TESTID}
            className="text-muted-foreground text-xs"
          >
            {outcome}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}

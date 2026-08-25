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
 *
 * # The whole folder, not only the notes part
 *
 * The owner pointed this dialog at the folder they sync and read the listing as
 * wrong: *"attaching from a folder must offer the WHOLE folder, not only the
 * notes part."* They were right, and the cause is two roots that are easy to
 * conflate. A vault **is** a notes-flagged sync profile (AD-54), and its root is
 * `local_path/subfolder` — so a profile synced at `~/tgdrive` with the notes
 * subfolder set lists `attachments/ company/ journal/ …` and never the
 * `photos/` folder sitting beside it, which is on every machine this profile
 * reaches and is exactly the thing a person wants to put in a note.
 *
 * So this dialog browses `local_path` — the synced folder — through
 * `notes_gallery`'s explicit `scope` parameter. The gallery block keeps the
 * vault root, because a gallery block's folder is written in a note and a note
 * names vault-relative paths.
 *
 * # Two frames, and the one an embed may name
 *
 * Widening the listing puts files on screen that a note **cannot show**, and
 * this is not a detail to be careful about — it is a hard refusal in Rust.
 * `keeper-note://` resolves against the vault root through
 * `notes_vault::contained`, which requires every component to be
 * `Component::Normal`; so `..` is refused, an embed can only ever name a
 * vault-relative path, and `![[../photos/holiday.png]]` would render nothing on
 * *this* machine, not merely on another one. A markdown link is no escape
 * either: `notes_open_file` would resolve it (FR-109) but nothing in the webview
 * calls that command, so the link would be inert.
 *
 * That is why each entry carries two paths and the offer hangs on the second:
 *
 * - `relPath` is relative to the folder that was **listed** — synced-folder
 *   relative here. It identifies the file on disk, so it is what navigation and
 *   the row's tooltip use.
 * - `vaultRelPath` is the file's **vault-relative** path, or `null` when the
 *   file lives above the vault root. It is the only thing an embed may name, and
 *   a row whose `vaultRelPath` is `null` carries {@link REASON_OUTSIDE_VAULT}
 *   where its button would have been.
 *
 * Both come from Rust, so no path arithmetic happens in the webview (AD-65) —
 * stripping the subfolder off a `relPath` here is precisely the arithmetic that
 * rule exists to prevent, and it would be wrong for a vault whose subfolder is
 * empty.
 *
 * A row that is shown and not offered is deliberate. Hiding it would answer
 * "offer the whole folder" with a listing that is once again not the whole
 * folder, and the person would be left wondering where `photos/holiday.png`
 * went. The row is there, the reason is where the button would be, and the
 * promise in the header stays true because it only ever described what is
 * offered.
 *
 * # A folder with thousands of files in it
 *
 * The synced folder is not vault-sized. The owner's holds 155,662 files, so
 * "browse it" has to survive being pointed at it.
 *
 * Rust is already bounded: `browse_root` lists one directory, never recursing,
 * and cuts it at `keeper_sync::browse::LISTING_CAP` — reporting the cut through
 * `truncated` rather than hiding it. So the reply is at most a thousand entries.
 *
 * A thousand rows is still a thousand mounted buttons inside a scroller that
 * shows eight of them, and no way to reach the one you want except dragging. So
 * the list is filtered by name and the rows are capped at
 * {@link ATTACH_FROM_VAULT_ROW_CAP}. The two go together and neither works
 * alone: a cap without a filter makes files unreachable, and a filter without a
 * cap still mounts a thousand rows before anything has been typed.
 *
 * The filter narrows what **arrived**, and is not pushed down into `browse`. A
 * filter in Rust would search a listing that had already been cut at the cap,
 * which reads as a search over the folder and is not one. Narrowing what arrived
 * is a claim this surface can keep, and the two sentences underneath the list say
 * which of the two limits is in force.
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
import { Input } from "@/components/ui/input";
import { type NoteGalleryScope, type NoteGalleryVm, notesGallery } from "@/lib/ipc/client";
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
 *
 * "The folder you sync" and no longer "this note's folder": the listing reaches
 * the whole synced folder now, so the narrower phrase would be a false claim
 * about most of what is on screen. It stays true of every row that carries a
 * button, which is all this sentence ever described — a file above the vault
 * root is shown with {@link REASON_OUTSIDE_VAULT} and cannot be attached at all.
 */
export const ATTACH_FROM_VAULT_PROMISE =
  "These files are already in the folder you sync, so keeper links them where they are. Nothing is copied.";

/** What a row's button says. The same verb the whole story is about. */
export const ATTACH_FROM_VAULT_ACTION = "Attach";

/** The accessible name of the list, so a test can read what is on offer. */
export const ATTACH_FROM_VAULT_LIST_TESTID = "attach-from-vault-entries";

/** Test id for the sentence the dialog leaves behind. One slot, because a
 *  person reads one outcome. */
export const ATTACH_FROM_VAULT_OUTCOME_TESTID = "attach-from-vault-outcome";

/** How the breadcrumb names the root of the folder being browsed, which has no
 *  name of its own here: the absolute path is never on screen (FR-145) and the
 *  profile's own folder name is not something this surface is told. The synced
 *  folder rather than the vault, because that is what is now listed — a vault
 *  with a notes subfolder appears as an ordinary segment below it. */
export const SYNCED_ROOT_LABEL = "The folder you sync";

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

/**
 * The root this dialog browses: item 10's whole point, and the reason
 * `notes_gallery` grew an explicit parameter rather than changing its default.
 *
 * Typed rather than passed as a bare string literal at the call site, so
 * renaming the variant in Rust fails this file's compile instead of silently
 * sending a scope Rust no longer understands.
 */
const SCOPE: NoteGalleryScope = "syncedFolder";

/**
 * Why a file above the vault root offers no button.
 *
 * Not a caution and not a preference: `notes_vault::contained` refuses any
 * component that is not `Component::Normal`, so `keeper-note://` will not serve
 * a path that climbs out of the vault and the embed would render nothing
 * anywhere. Worded as a fact about where the file is, because that is the only
 * part the person can act on — moving it under the notes folder makes it
 * attachable.
 */
export const REASON_OUTSIDE_VAULT = "outside the notes folder, so a note cannot show it";

/** The filter's accessible name and its placeholder. A folder of hundreds is
 *  unreachable by scrolling an eight-row window. */
export const ATTACH_FROM_VAULT_FILTER_LABEL = "Filter by name";

/** What the list says when the filter excluded everything. Distinct from
 *  {@link ATTACH_FROM_VAULT_EMPTY}, which is a claim about the folder. */
export const ATTACH_FROM_VAULT_NO_MATCH = "No file here matches that.";

/**
 * How many rows are mounted at once.
 *
 * `browse`'s own cap is a thousand, and a thousand rows is a thousand buttons in
 * a scroller eight rows tall — paid on every folder change, to render a list
 * nobody can navigate. Two hundred is far more than fits and far less than
 * hurts; the filter is what keeps the rest reachable, and
 * {@link ATTACH_FROM_VAULT_CAPPED} says the cap is in force and how to get past
 * it.
 */
export const ATTACH_FROM_VAULT_ROW_CAP = 200;

/** Said when the row cap is in force, with the remedy in it — unlike
 *  {@link ATTACH_FROM_VAULT_TRUNCATED}, which reports a cut this surface cannot
 *  undo. */
export const ATTACH_FROM_VAULT_CAPPED =
  "This folder holds more files than the dialog shows at once. Type part of a name to narrow the list.";

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
  /** The folder being browsed, relative to the synced folder root; `""` is that
   *  root. Not vault-relative — see the module comment's two frames. */
  const [folder, setFolder] = useState("");
  // `undefined` is "not read yet" and a listing is the answer — the same two
  // states, spelled the same way, as the note chooser's search. Collapsing them
  // would make the dialog say "this folder is empty" for the first frame of
  // every folder, which is a claim and is not true yet.
  const [listing, setListing] = useState<NoteGalleryVm | undefined>(undefined);
  const [outcome, setOutcome] = useState<string | null>(null);
  /** What the person has typed to narrow the list. Cleared on every folder
   *  change: a needle carried into the next folder would hide rows the person
   *  never asked to hide, and reads as an empty folder. */
  const [filter, setFilter] = useState("");

  useEffect(() => {
    setListing(undefined);
    setFilter("");
    let live = true;
    // Passed explicitly rather than relying on the omitted default, so the call
    // says which of the two roots it means — and so the gallery block's own call,
    // which wants the vault root, keeps working untouched.
    void notesGallery(vaultId, folder, SCOPE)
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
  const items = listing?.items ?? [];
  // Matched on the entry's own name and not its path: the path's folder part is
  // the breadcrumb the person is already looking at, so a needle would match it
  // in every row and narrow nothing.
  const needle = filter.trim().toLowerCase();
  const matched =
    needle === "" ? items : items.filter((item) => item.name.toLowerCase().includes(needle));
  // The cap is on what is MOUNTED, not on what was matched, so the sentence
  // below can tell the person there is more and that typing reaches it.
  const shown = matched.slice(0, ATTACH_FROM_VAULT_ROW_CAP);

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
            told where the synced folder is and must not be (AD-65, FR-145). */}
        <p className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
          <span className="min-w-0 truncate font-mono">
            {[SYNCED_ROOT_LABEL, ...segments].join(" / ")}
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

        {/* The way past {@link ATTACH_FROM_VAULT_ROW_CAP}, and the only way to
            reach a file the cap did not mount. Rendered unconditionally —
            through "Reading…", through an empty folder, through a folder Rust
            refused — because a control that comes and goes as each listing
            resolves moves the list under the hands of someone already reading
            it, and every folder change resolves. Its value is cleared by the
            effect above, not by this element. */}
        <Input
          type="search"
          value={filter}
          aria-label={ATTACH_FROM_VAULT_FILTER_LABEL}
          placeholder={ATTACH_FROM_VAULT_FILTER_LABEL}
          className="h-8"
          onChange={(event) => setFilter(event.target.value)}
        />

        <ul data-testid={ATTACH_FROM_VAULT_LIST_TESTID} className="max-h-64 overflow-auto text-sm">
          {listing === undefined ? (
            <li className="px-1 py-2 text-muted-foreground text-xs">{ATTACH_FROM_VAULT_READING}</li>
          ) : listing.problem !== null ? (
            // Rust's sentence, verbatim: the reason is Rust's — missing,
            // unreadable, or a path that escapes the synced folder — and this
            // surface is not told which of the three it was.
            <li className="px-1 py-2 text-muted-foreground text-xs">{listing.problem}</li>
          ) : items.length === 0 ? (
            <li className="px-1 py-2 text-muted-foreground text-xs">{ATTACH_FROM_VAULT_EMPTY}</li>
          ) : shown.length === 0 ? (
            // The filter's own emptiness, never the folder's: "this folder is
            // empty" would be a claim about the disk, and it is false here.
            <li className="px-1 py-2 text-muted-foreground text-xs">
              {ATTACH_FROM_VAULT_NO_MATCH}
            </li>
          ) : (
            shown.map((item) => {
              if (item.kind === "folder") {
                return (
                  <li key={item.relPath} className="flex min-w-0 items-center gap-2 px-1 py-1">
                    {/* The whole row is the affordance, because the only thing a
                        folder does here is open. Its accessible name carries the
                        folder's own name: a listing of nine folders is otherwise
                        nine identically-named controls. A folder is offered
                        whether or not it is inside the vault — browsing out of
                        the notes subfolder is the whole of item 10. */}
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
                );
              }
              // Bound to a local rather than read off `item` at each use: the
              // `!== null` test below narrows a local, and a narrowing on a
              // property does not survive into the button's own callback — which
              // is the one place the path must be a string.
              const target = item.vaultRelPath;
              return (
                <li key={item.relPath} className="flex min-w-0 items-center gap-2 px-1 py-1">
                  {/* The listed-root path in the tooltip and the vault-relative
                      one in the button's name, because they answer different
                      questions: where the file is, and what the note will say. */}
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
                  {target === null ? (
                    // Asked FIRST, before the three questions about the name,
                    // because it is the only one that is about where the file is
                    // and the only one no rewording of the name can fix. Every
                    // question below it needs a vault-relative path to ask, and
                    // this row has none.
                    <span className="shrink-0 text-muted-foreground text-xs">
                      {REASON_OUTSIDE_VAULT}
                    </span>
                  ) : embedded.has(attachmentName(target).toLowerCase()) ? (
                    <span className="shrink-0 text-muted-foreground text-xs">
                      {ATTACHMENT_PRESENT_LABEL}
                    </span>
                  ) : namesANote(target) ? (
                    <span className="shrink-0 text-muted-foreground text-xs">
                      {REASON_IS_A_NOTE}
                    </span>
                  ) : !wikilinkNameable(target) ? (
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
                      aria-label={`${ATTACH_FROM_VAULT_ACTION} ${target}`}
                      onClick={() => {
                        // The whole of the in-vault attach. No IPC, because
                        // there is nothing left to ask: `vaultRelPath` is
                        // already the path the note names, its being non-null is
                        // Rust saying the file is inside the vault, and copying
                        // is what this door exists not to do. `planAttachments`
                        // is the same decision the other three entry points
                        // make, so the bytes are the same.
                        const plan = planAttachments(body, [target]);
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
              );
            })
          )}
        </ul>

        {listing?.truncated === true ? (
          <p className="text-muted-foreground text-xs">{ATTACH_FROM_VAULT_TRUNCATED}</p>
        ) : null}

        {/* A second sentence and not a rewording of the first: the cut above is
            Rust's and cannot be undone from here, this one is the dialog's and
            typing gets past it. A folder can be under both at once. */}
        {matched.length > shown.length ? (
          <p className="text-muted-foreground text-xs">{ATTACH_FROM_VAULT_CAPPED}</p>
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

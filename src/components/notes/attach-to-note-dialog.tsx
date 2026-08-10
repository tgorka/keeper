/**
 * Put files into a note that is not open (Story 45.13, FR-188, FR-189,
 * UX-DR76).
 *
 * The Files pane's entry point into the one insertion path. A multiselection is
 * a set of files and no note, which is the one thing the other two entry points
 * never lack: the attachments panel is *inside* a note, and the editor's own
 * picker writes at a caret that already exists. So this dialog exists to supply
 * the missing half, and to do nothing else — the text it writes and the
 * duplicates it refuses are `@/lib/notes/attach`'s, identical to what the panel
 * writes for the same file.
 *
 * # The order of operations is the design
 *
 * Search, then choose, **then** resolve. Resolution is not free: a file outside
 * the vault is copied into `attachments/`, which is a change to the user's disk,
 * and doing it before a note is chosen would leave copies behind every time
 * somebody opened this and pressed Escape.
 *
 * That means the search filter is advisory and the write-time check is
 * authoritative. The list declines to offer a note whose body already embeds a
 * file with one of these names (Rust answers that, over the notes on disk); the
 * plan built against the chosen note's actual body then decides for real. The
 * two can only disagree in one direction — a file copied in under a
 * collision-free name (`photo-2.png`) is genuinely new although a `photo.png`
 * hid the note from the list — and being conservative about offering a note is
 * the harmless side of that.
 *
 * # Why the already-holding notes are shown rather than hidden
 *
 * The same choice Story 43.7 made for a row it will not insert: the note stays
 * in the list, without the button, saying which file it already has. A note
 * that silently vanished from a search reads as "keeper cannot find my note",
 * which is a different and much more alarming fact than the true one.
 */
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  type NoteAttachTargetVm,
  notesAttachSources,
  notesAttachTargets,
  notesBodyRead,
  notesBodyWrite,
} from "@/lib/ipc/client";
import { attachmentName, bodyWithAttachments, planAttachments } from "@/lib/notes/attach";

/** The dialog's accessible name, and the word on the control that opens it. */
export const ATTACH_TO_NOTE_LABEL = "Attach to note";

/** The search field's label. */
export const ATTACH_SEARCH_LABEL = "Find a note by title";

/** What a row's button says. The verb is the same one the whole story is about. */
export const ATTACH_ACTION_LABEL = "Attach";

/** Test id for the sentence the dialog leaves behind — the receipt, or the
 *  refusal. One slot, because a person reads one outcome. */
export const ATTACH_OUTCOME_TESTID = "attach-outcome";

/** Test id for the note list, so a test can count what is on offer. */
export const ATTACH_TARGETS_TESTID = "attach-targets";

/** What a note that already holds one of these files says instead of a button. */
export const ATTACH_HOLDS_PREFIX = "Already has";

/** What the list says while the search is in flight for the first time. */
export const ATTACH_SEARCHING_SENTENCE = "Looking…";

/** What the list says when the vault has no note matching the search. */
export const ATTACH_NO_MATCH_SENTENCE = "No note matches that.";

export interface AttachToNoteDialogProps {
  /** The vault the target note lives in. */
  vaultId: string;
  /**
   * Absolute paths of the files to attach, in the order they will be written.
   *
   * Absolute because that is what the shell hands the webview — a Files-pane
   * row's `absolutePath`, a picker's result. They never reach a note: Rust
   * turns each one into a vault-relative path first (FR-145, AD-65).
   */
  sources: readonly string[];
  /** Close, whether or not anything was attached. */
  onClose: () => void;
}

export function AttachToNoteDialog({ vaultId, sources, onClose }: AttachToNoteDialogProps) {
  const [query, setQuery] = useState("");
  // `undefined` is "not asked yet" and `[]` is "asked, nothing matched". One
  // state for both would make the dialog say "no note matches" for the first
  // frame of every search, which is a claim and is not true yet.
  const [targets, setTargets] = useState<NoteAttachTargetVm[] | undefined>(undefined);
  const [outcome, setOutcome] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The names are taken from the source paths rather than from a resolution,
  // because resolving copies files in and nothing has been chosen yet. A
  // basename is the same in both frames, so this is the same key Rust will use.
  const names = sources.map(attachmentName);
  const namesKey = names.join("\u0000");

  useEffect(() => {
    let live = true;
    void notesAttachTargets(vaultId, query, namesKey === "" ? [] : namesKey.split("\u0000"))
      .then((found) => {
        if (live) {
          setTargets(found);
        }
      })
      .catch(() => {
        // A failed search is an empty list plus a sentence, never a stale one:
        // offering the previous query's notes for this query would attach a
        // file to a note the person is no longer looking at.
        if (live) {
          setTargets([]);
          setOutcome("keeper could not search the vault just now.");
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, query, namesKey]);

  const attach = useCallback(
    async (target: NoteAttachTargetVm) => {
      setBusy(true);
      setOutcome(null);
      try {
        // Now, and not before: this is the call that copies an outside file
        // into the vault.
        const resolved = await notesAttachSources(vaultId, [...sources]);
        const relPaths = resolved
          .map((source) => source.relPath)
          .filter((path): path is string => path !== null);
        // Derived from "produced no path", NOT from "carries a refusal".
        //
        // `NoteAttachSourceVm`'s contract is that exactly one of `relPath` and
        // `refusal` is set, and filtering the two fields independently would
        // trust that contract with the one thing this story exists to prevent:
        // a source with neither would land in neither list and be dropped
        // without a word. Partitioning on the field that decides what happens
        // means every source the person offered is accounted for in the
        // sentence, whatever shape Rust sends — a sentence keeper composes
        // itself is a worse answer than Rust's, and a far better one than
        // silence.
        const refusedSources = resolved
          .filter((source) => source.relPath === null)
          .map(
            (source) =>
              source.refusal ?? `keeper did not attach ${source.name} and did not say why.`,
          );
        const copied = resolved.filter((source) => source.copied).length;

        const before = await notesBodyRead(vaultId, target.id);
        const plan = planAttachments(before.text, relPaths);
        if (plan.text !== "") {
          await notesBodyWrite(
            vaultId,
            target.id,
            bodyWithAttachments(before.text, plan),
            before.rev,
          );
        }
        setOutcome(
          [
            plan.inserted.length > 0
              ? `Put ${plan.inserted.length === 1 ? "1 file" : `${plan.inserted.length} files`} in ${target.title}.`
              : null,
            copied > 0
              ? `${copied === 1 ? "1 file was" : `${copied} files were`} outside the vault, so keeper copied ${copied === 1 ? "it" : "them"} into attachments/ — a note cannot name a file the other machines do not have.`
              : null,
            plan.refusal,
            ...refusedSources,
          ]
            .filter((clause): clause is string => clause !== null)
            .join(" "),
        );
        // The note now holds what was just written, so it stops being on
        // offer — recorded here rather than re-searched, because this is
        // knowledge the dialog has and Rust would only tell it back. The
        // dialog stays open: attaching the same files to a second note is a
        // real thing to want.
        setTargets((current) =>
          current?.map((candidate) =>
            candidate.id === target.id
              ? {
                  ...candidate,
                  holds: [
                    ...candidate.holds,
                    ...plan.inserted.map((path) => attachmentName(path).toLowerCase()),
                  ],
                }
              : candidate,
          ),
        );
      } catch (error) {
        setOutcome(
          `keeper could not attach to ${target.title}: ${
            error instanceof Error ? error.message : String(error)
          }`,
        );
      } finally {
        setBusy(false);
      }
    },
    [vaultId, sources],
  );

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{ATTACH_TO_NOTE_LABEL}</DialogTitle>
          <DialogDescription>
            {names.length === 1
              ? `${names[0]} goes into the note you pick.`
              : `${names.length} files go into the note you pick, in this order: ${names.join(", ")}.`}
          </DialogDescription>
        </DialogHeader>

        <Input
          aria-label={ATTACH_SEARCH_LABEL}
          placeholder={ATTACH_SEARCH_LABEL}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />

        <ul data-testid={ATTACH_TARGETS_TESTID} className="max-h-64 overflow-auto text-sm">
          {targets === undefined ? (
            <li className="px-1 py-2 text-muted-foreground text-xs">{ATTACH_SEARCHING_SENTENCE}</li>
          ) : targets.length === 0 ? (
            <li className="px-1 py-2 text-muted-foreground text-xs">{ATTACH_NO_MATCH_SENTENCE}</li>
          ) : (
            targets.map((target) => (
              <li key={target.id} className="flex min-w-0 items-center gap-2 px-1 py-1">
                <span className="min-w-0 flex-1 truncate" title={target.path}>
                  {target.title}
                </span>
                {target.holds.length > 0 ? (
                  // No button, and the reason where the button would have been
                  // — the same shape the attachments panel uses for a file the
                  // note already embeds.
                  <span className="shrink-0 text-muted-foreground text-xs">
                    {`${ATTACH_HOLDS_PREFIX} ${target.holds.join(", ")}`}
                  </span>
                ) : (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-6 shrink-0"
                    disabled={busy}
                    // The note's title, not the verb: a search of eight notes
                    // is eight identical "Attach" buttons to anyone not
                    // looking at the screen.
                    aria-label={`${ATTACH_ACTION_LABEL} to ${target.title}`}
                    onClick={() => void attach(target)}
                  >
                    {ATTACH_ACTION_LABEL}
                  </Button>
                )}
              </li>
            ))
          )}
        </ul>

        {outcome === null ? null : (
          <p data-testid={ATTACH_OUTCOME_TESTID} className="text-muted-foreground text-xs">
            {outcome}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}

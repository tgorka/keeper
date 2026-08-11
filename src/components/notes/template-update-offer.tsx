/**
 * The offer to update the notes made from a template that was just edited
 * (Story 44.8, FR-163, UX-DR59).
 *
 * # What this surface is not
 *
 * It is not a migration wizard and it has no "apply to all". Selection is one
 * checkbox per note and there is no control that ticks them together, which is a
 * decision rather than an omission: a single click that rewrites every note made
 * from a template is the destructive reading UX-DR59 exists to forbid, and a
 * "select all" is that click with a confirmation step in front of it. N notes
 * cost N deliberate acts.
 *
 * # What the user is shown before they can choose
 *
 * Per note: every change the template made, the lines that would leave, the
 * lines that would arrive, and the line it lands on — or, for a change that will
 * not be applied, the sentence saying why (Rust composes those; nothing here
 * turns a code into words). A note keeper cannot undo is listed with its changes
 * and cannot be ticked, so "wait a few seconds" is visible rather than mysterious.
 *
 * # The undo is the note's own history
 *
 * Applying reports, per note, the revision that held it a moment before. Undo is
 * {@link notesRestoreRevision} against exactly that revision — the same object
 * the history panel lists — so there is no private undo stack to get out of step
 * with git.
 *
 * The banner never opens the dialog by itself and never steals the caret: the
 * user is editing a template when it appears.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  TemplateUpdateNoteVm,
  TemplateUpdateOfferVm,
  TemplateUpdateResultVm,
} from "@/lib/ipc/client";
import {
  notesRestoreRevision,
  notesTemplateUpdateApply,
  notesTemplateUpdatePreview,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * How long the editor must be settled before keeper asks whether there is an
 * offer.
 *
 * The autosave fires 400 ms after typing stops, and answering the question costs
 * one file read per note made from the template — so asking on every autosave
 * would put a vault scan behind every pause for thought. Four seconds is past
 * the pause and well inside the moment someone finishes an edit and looks up.
 */
export const TEMPLATE_OFFER_IDLE_MS = 4_000;

/** What the banner says when keeper could not ask. */
export const TEMPLATE_OFFER_FAILED = "keeper couldn't check the notes made from this template.";

/** What the apply reports when it could not run at all. */
export const TEMPLATE_UPDATE_FAILED = "keeper couldn't apply the update.";

/** What undo says when the revision could not be written back. */
export const TEMPLATE_UNDO_FAILED = "keeper couldn't put that note back.";

export interface TemplateUpdateOfferProps {
  vaultId: string;
  noteId: string;
  /** The note's current content revision. A new one is a new question. */
  rev: string;
}

export function TemplateUpdateOffer({ vaultId, noteId, rev }: TemplateUpdateOfferProps) {
  const [offer, setOffer] = useState<TemplateUpdateOfferVm | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const asked = useRef<string | null>(null);

  // noteId is this effect's trigger, not its input. The body reads nothing and
  // exists only to run when the identity changes; dropping the dependency to
  // satisfy the rule would leave the previous note's offer on screen over a
  // different note.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reason above.
  useEffect(() => {
    // A different note is a different question; drop everything the last one
    // produced rather than showing its offer over this one.
    asked.current = null;
    setOffer(null);
    setFailure(null);
    setOpen(false);
    setDismissed(null);
  }, [noteId]);

  useEffect(() => {
    if (asked.current === rev) {
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      asked.current = rev;
      void notesTemplateUpdatePreview(vaultId, noteId)
        .then((found) => {
          if (!live) {
            return;
          }
          setOffer(found);
          setFailure(null);
        })
        .catch((raw: unknown) => {
          if (live) {
            setOffer(null);
            setFailure(syncErrorMessage(raw, TEMPLATE_OFFER_FAILED));
          }
        });
    }, TEMPLATE_OFFER_IDLE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [vaultId, noteId, rev]);

  if (failure !== null) {
    return (
      <p role="status" className="border-b px-3 py-1 text-[11px] text-muted-foreground">
        {failure}
      </p>
    );
  }
  if (offer === null || dismissed === rev) {
    return null;
  }

  // A refusal is said out loud rather than shown as an empty dialog. keeper
  // deciding to do nothing has to be distinguishable from keeper being broken.
  if (offer.declined !== null) {
    return (
      <p role="status" className="border-b px-3 py-1 text-[11px] text-muted-foreground">
        {offer.declined}
      </p>
    );
  }

  const count = offer.notes.length;
  return (
    <>
      <div className="flex items-center gap-2 border-b px-3 py-1">
        <p role="status" className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">
          {count === 1
            ? "1 note came from this template."
            : `${count} notes came from this template.`}
        </p>
        <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
          Review changes
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setDismissed(rev)}>
          Not now
        </Button>
      </div>
      {open ? (
        <TemplateUpdateDialog
          vaultId={vaultId}
          offer={offer}
          onClose={() => {
            setOpen(false);
            setDismissed(rev);
          }}
        />
      ) : null}
    </>
  );
}

interface DialogProps {
  vaultId: string;
  offer: TemplateUpdateOfferVm;
  onClose: () => void;
}

function TemplateUpdateDialog({ vaultId, offer, onClose }: DialogProps) {
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [result, setResult] = useState<TemplateUpdateResultVm | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const toggle = useCallback((noteId: string) => {
    setChosen((current) => {
      const next = new Set(current);
      if (!next.delete(noteId)) {
        next.add(noteId);
      }
      return next;
    });
  }, []);

  const apply = useCallback(() => {
    setBusy(true);
    setFailure(null);
    // A note contributes only the changes that would actually land: a skipped
    // change is not a thing the user can accept, so it never reaches the wire.
    const selections = offer.notes
      .filter((note) => chosen.has(note.noteId) && note.blocked === null)
      .map((note) => ({
        noteId: note.noteId,
        changes: note.changes.filter((change) => change.skipped === null).map((c) => c.index),
      }))
      .filter((selection) => selection.changes.length > 0);

    void notesTemplateUpdateApply(vaultId, {
      templatePath: offer.templatePath,
      selections,
    })
      .then(setResult)
      .catch((raw: unknown) => setFailure(syncErrorMessage(raw, TEMPLATE_UPDATE_FAILED)))
      .finally(() => setBusy(false));
  }, [vaultId, offer, chosen]);

  const selected = offer.notes.filter(
    (note) => chosen.has(note.noteId) && note.blocked === null,
  ).length;

  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
    >
      <DialogContent className="max-h-[80vh] max-w-3xl overflow-hidden">
        <DialogHeader>
          <DialogTitle>Update notes from “{offer.templateTitle}”</DialogTitle>
          <DialogDescription>
            Nothing here has happened. Tick a note to take the changes keeper can apply to it
            without writing over what you wrote; anything you have edited yourself is left as it is.
          </DialogDescription>
        </DialogHeader>

        {result === null ? (
          <div className="max-h-[50vh] overflow-y-auto pr-1">
            <ul className="flex flex-col gap-3">
              {offer.notes.map((note) => (
                <NoteRow
                  key={note.noteId}
                  note={note}
                  checked={chosen.has(note.noteId)}
                  onToggle={() => toggle(note.noteId)}
                />
              ))}
            </ul>
          </div>
        ) : (
          <TemplateUpdateResult vaultId={vaultId} result={result} />
        )}

        {failure === null ? null : (
          <p role="status" className="text-[11px] text-muted-foreground">
            {failure}
          </p>
        )}

        <DialogFooter>
          {result === null ? (
            <>
              <Button variant="ghost" onClick={onClose}>
                Not now
              </Button>
              <Button disabled={selected === 0 || busy} onClick={apply}>
                {selected === 1 ? "Update 1 note" : `Update ${selected} notes`}
              </Button>
            </>
          ) : (
            <Button onClick={onClose}>Done</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface NoteRowProps {
  note: TemplateUpdateNoteVm;
  checked: boolean;
  onToggle: () => void;
}

function NoteRow({ note, checked, onToggle }: NoteRowProps) {
  const appliable = note.changes.filter((change) => change.skipped === null).length;
  const id = `template-update-${note.noteId}`;
  return (
    <li className="rounded-md border p-2">
      <div className="flex items-start gap-2">
        <Checkbox
          id={id}
          checked={checked}
          disabled={note.blocked !== null || appliable === 0}
          onCheckedChange={onToggle}
          aria-label={note.title}
        />
        <div className="min-w-0 flex-1">
          <label htmlFor={id} className="block truncate font-medium text-sm">
            {note.title}
          </label>
          <p className="truncate text-[11px] text-muted-foreground">{note.path}</p>
          {note.stalePath === null ? null : (
            <p className="text-[11px] text-muted-foreground">
              This note records the template as {note.stalePath}; keeper matched it by the
              template's id instead, and is not rewriting the note's properties.
            </p>
          )}
          {note.blocked === null ? null : (
            <p className="text-[11px] text-muted-foreground">{note.blocked}</p>
          )}
        </div>
      </div>

      <ul className="mt-2 flex flex-col gap-2">
        {note.changes.map((change) => (
          <li key={change.index} className="font-mono text-[11px]">
            {change.skipped === null ? (
              <p className="font-sans text-muted-foreground">
                {change.atLine === null ? "Lands in this note" : `Lands at line ${change.atLine}`}
              </p>
            ) : (
              <p className="font-sans text-muted-foreground">{change.skipped}</p>
            )}
            {change.removed.map((line, at) => (
              // A diff line has no identity but its position, and two identical
              // lines in one hunk are a real and ordinary thing.
              // biome-ignore lint/suspicious/noArrayIndexKey: reason above.
              <pre key={`-${at}`} className="whitespace-pre-wrap text-destructive">
                {`- ${line}`}
              </pre>
            ))}
            {change.added.map((line, at) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: as above.
              <pre key={`+${at}`} className="whitespace-pre-wrap">
                {`+ ${line}`}
              </pre>
            ))}
          </li>
        ))}
      </ul>
    </li>
  );
}

interface ResultProps {
  vaultId: string;
  result: TemplateUpdateResultVm;
}

/**
 * What happened, and the way back.
 *
 * Undo is offered per note rather than as one button, for the same reason the
 * offer is: one control that reverses a batch is a control that can be pressed
 * without reading. Each `undoRev` is a real commit, so pressing Undo twice is
 * harmless — the second press restores the same bytes.
 */
function TemplateUpdateResult({ vaultId, result }: ResultProps) {
  const [undone, setUndone] = useState<Set<string>>(new Set());
  const [failure, setFailure] = useState<string | null>(null);

  return (
    <div className="max-h-[50vh] overflow-y-auto pr-1">
      <p className="text-sm">
        {result.updated.length === 1
          ? "Updated 1 note."
          : `Updated ${result.updated.length} notes.`}
      </p>
      <ul className="mt-2 flex flex-col gap-1">
        {result.updated.map((applied) => (
          <li key={applied.noteId} className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-sm">{applied.title}</span>
            <span className="text-[11px] text-muted-foreground">
              {applied.applied === 1 ? "1 change" : `${applied.applied} changes`}
            </span>
            <Button
              size="sm"
              variant="ghost"
              disabled={undone.has(applied.noteId)}
              onClick={() => {
                setFailure(null);
                void notesRestoreRevision(vaultId, applied.noteId, applied.undoRev)
                  .then(() => setUndone((current) => new Set(current).add(applied.noteId)))
                  .catch((raw: unknown) => setFailure(syncErrorMessage(raw, TEMPLATE_UNDO_FAILED)));
              }}
            >
              {undone.has(applied.noteId) ? "Undone" : "Undo"}
            </Button>
          </li>
        ))}
      </ul>
      {result.skipped.length === 0 ? null : (
        <ul className="mt-3 flex flex-col gap-1">
          {result.skipped.map((sentence) => (
            <li key={sentence} className="text-[11px] text-muted-foreground">
              {sentence}
            </li>
          ))}
        </ul>
      )}
      {failure === null ? null : (
        <p role="status" className="mt-2 text-[11px] text-muted-foreground">
          {failure}
        </p>
      )}
    </div>
  );
}

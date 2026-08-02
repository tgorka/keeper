/**
 * The inline diff bar (Story 38.2/38.4, FR-112, UX-DR39).
 *
 * This is the agent-cohabitation surface, and its shape is a decision, not a
 * style: it is a strip under the header, it never steals focus, and it is
 * **never a modal**. A modal at the moment an agent writes is a modal that
 * fires while the user is typing in another application, and the phase's
 * position on that is categorical.
 *
 * It appears under exactly one condition, and the condition lives in the store
 * rather than here: an external revision arrived that the buffer could not
 * silently absorb. A clean buffer takes the write live and this bar never
 * shows. A dirty buffer keeps every character the user typed and this bar says
 * what arrived.
 */
import { Button } from "@/components/ui/button";
import { notesMarkRead } from "@/lib/ipc/client";
import {
  acceptPending,
  keepMine,
  notesEditorStore,
  useNotesEditorStore,
} from "@/lib/stores/notes-editor";

/**
 * Accept: take the arrived revision, and acknowledge it.
 *
 * The acknowledgement is load-bearing rather than incidental. Accept is the
 * only path that clears an unread mark (UX-DR39 says so, and `NoteRowVm`
 * carries no revision, so the list could not do it even if it wanted to), and
 * the mark is one of the three surfaces the agent-changed state shows on. The
 * rev handed to Rust is the one the body stream delivered — never a timestamp
 * and never a guess.
 */
function acceptAndAcknowledge(): void {
  const { pending, vaultId, noteId } = notesEditorStore.getState();
  acceptPending();
  if (pending === null || vaultId === null || noteId === null) {
    return;
  }
  notesMarkRead(vaultId, noteId, pending.rev).catch(() => {
    // The mark stays set. That is the honest outcome: the user has read the
    // change, but keeper could not record that it had, and a mark that lingers
    // costs a second look while a mark wrongly cleared costs the review.
  });
}

export interface NoteDiffBarProps {
  /** Open the full diff. Absent surfaces simply omit the affordance. */
  onShowChanges?: () => void;
  /** Open conflict resolution; only offered when the hunks overlapped. */
  onResolve?: () => void;
}

/** How many lines the arriving revision adds and removes, for the one-liner. */
export function countChangedLines(
  before: string,
  after: string,
): { added: number; removed: number } {
  const from = before.split("\n");
  const to = after.split("\n");
  let head = 0;
  while (head < from.length && head < to.length && from[head] === to[head]) {
    head += 1;
  }
  let tailFrom = from.length;
  let tailTo = to.length;
  while (tailFrom > head && tailTo > head && from[tailFrom - 1] === to[tailTo - 1]) {
    tailFrom -= 1;
    tailTo -= 1;
  }
  return { added: tailTo - head, removed: tailFrom - head };
}

/** "3 additions, 1 removal" — pluralised, and never "0 additions". */
function summarise(added: number, removed: number): string {
  const parts: string[] = [];
  if (added > 0) {
    parts.push(`${added} addition${added === 1 ? "" : "s"}`);
  }
  if (removed > 0) {
    parts.push(`${removed} removal${removed === 1 ? "" : "s"}`);
  }
  return parts.length === 0 ? "no line changes" : parts.join(", ");
}

export function NoteDiffBar({ onShowChanges, onResolve }: NoteDiffBarProps) {
  const pending = useNotesEditorStore((state) => state.pending);
  const base = useNotesEditorStore((state) => state.base);

  if (pending === null) {
    return null;
  }

  const { added, removed } = countChangedLines(base, pending.text);
  const overlapped = pending.kind === "diverged";

  return (
    <div
      // `status`, not `alert`: assertive announcement while someone is typing
      // in another window is the audible equivalent of a modal.
      role="status"
      className="flex items-center gap-2 border-b bg-muted/60 px-3 py-1.5 text-xs"
    >
      <span className="flex-1 truncate">
        {overlapped ? "Changed on disk, and it overlaps your edits" : "Changed on disk"} ·{" "}
        {summarise(added, removed)}
      </span>
      {onShowChanges === undefined ? null : (
        <Button size="sm" variant="ghost" onClick={onShowChanges}>
          Show changes
        </Button>
      )}
      {overlapped && onResolve !== undefined ? (
        <Button size="sm" variant="ghost" onClick={onResolve}>
          Resolve
        </Button>
      ) : null}
      <Button size="sm" variant="ghost" onClick={keepMine}>
        Keep mine
      </Button>
      <Button size="sm" onClick={acceptAndAcknowledge}>
        {overlapped ? "Take theirs" : "Accept"}
      </Button>
    </div>
  );
}

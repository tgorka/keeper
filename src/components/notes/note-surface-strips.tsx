/**
 * What an agent is showing the person in this note (UX-DR131): its
 * highlight, with the control that puts it away, and its proposed edit, with
 * *Apply* and *Decline*.
 *
 * The same kind of strip as the change bar above it (`note-diff-bar.tsx`) and
 * for the same reason: an agent acts while the person may be typing, so this
 * is never a modal and never takes focus. The proposal shows the lines it
 * would replace and the lines it would write; nothing changes in the note
 * until *Apply*, which is the person's own edit through the editor.
 */
import { Button } from "@/components/ui/button";
import {
  declineSurfaceProposal,
  dismissSurfaceHighlight,
  linesLabel,
  surfaceNoteKey,
  useSurfaceStore,
} from "@/lib/agents/surface";

export const SURFACE_PROPOSAL_LABEL = "Proposed edit";
export const SURFACE_APPLY_LABEL = "Apply";
export const SURFACE_DECLINE_LABEL = "Decline";
export const SURFACE_DISMISS_HIGHLIGHT_LABEL = "Dismiss highlight";

export interface NoteSurfaceStripsProps {
  vaultId: string;
  noteId: string;
  /** Apply the proposal through this note's editor. */
  onApply: () => void;
}

/** One side of the proposal: what goes, or what comes. */
function DiffLines({ lines, kind }: { lines: string[]; kind: "del" | "ins" }) {
  const Tag = kind;
  return (
    <Tag
      className={
        kind === "del"
          ? "block text-muted-foreground line-through decoration-destructive/60"
          : "block text-foreground no-underline"
      }
    >
      {lines.map((line, at) => (
        // Lines of one fixed proposal: they never reorder.
        // biome-ignore lint/suspicious/noArrayIndexKey: positions are the identity here
        <span key={at} className="flex gap-2">
          <span aria-hidden className={kind === "del" ? "text-destructive" : "text-primary"}>
            {kind === "del" ? "−" : "+"}
          </span>
          <span className="min-w-0 whitespace-pre-wrap break-words">
            {line === "" ? " " : line}
          </span>
        </span>
      ))}
    </Tag>
  );
}

export function NoteSurfaceStrips({ vaultId, noteId, onApply }: NoteSurfaceStripsProps) {
  const key = surfaceNoteKey(vaultId, noteId);
  const highlight = useSurfaceStore((state) => state.highlights[key] ?? null);
  const proposal = useSurfaceStore((state) => state.proposals[key] ?? null);

  if (highlight === null && proposal === null) {
    return null;
  }

  return (
    <div className="shrink-0 border-b">
      {highlight === null ? null : (
        <div className="flex items-center gap-2 bg-muted/60 px-3 py-1 text-xs">
          <span
            aria-hidden
            className="h-3 w-[3px] shrink-0 rounded-full bg-ring"
            data-slot="surface-highlight-swatch"
          />
          <span role="status" className="min-w-0 flex-1">
            Your agent highlighted {linesLabel(highlight.span)}
          </span>
          <Button
            size="sm"
            variant="ghost"
            aria-label={SURFACE_DISMISS_HIGHLIGHT_LABEL}
            onClick={() => dismissSurfaceHighlight(vaultId, noteId)}
          >
            Dismiss
          </Button>
        </div>
      )}
      {proposal === null || proposal.range === null ? null : (
        <section
          aria-label={SURFACE_PROPOSAL_LABEL}
          className="flex flex-col gap-1.5 bg-muted/60 px-3 py-1.5 text-xs"
        >
          {/* Wraps rather than truncates: a panel can be 280px wide, and the
              sentence is what says which lines the buttons act on. */}
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span role="status" className="min-w-40 flex-1">
              Your agent proposes an edit to {linesLabel(proposal.range)}
            </span>
            <div className="ml-auto flex shrink-0 gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => declineSurfaceProposal(vaultId, noteId)}
              >
                {SURFACE_DECLINE_LABEL}
              </Button>
              <Button size="sm" onClick={onApply}>
                {SURFACE_APPLY_LABEL}
              </Button>
            </div>
          </div>
          <div className="max-h-40 overflow-auto rounded-sm border bg-background px-2 py-1 font-mono text-meta">
            <DiffLines lines={(proposal.expected ?? "").split("\n")} kind="del" />
            <DiffLines lines={(proposal.text ?? "").split("\n")} kind="ins" />
          </div>
        </section>
      )}
    </div>
  );
}

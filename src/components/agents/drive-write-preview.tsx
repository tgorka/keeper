/**
 * A `drive_write` request's change, drawn on its approval card (109.27, contract § 8): what it
 * does to which file of which drive, the line diff Rust computed from this device's checkout (an
 * edit or a replace), the new text (a create, an append past what fits a diff), the deleted text,
 * or from → to (a move); whether the chat read outside content, who else reads the drive, why
 * *Allow* is not offered here, and what became of it. Rust's view model only: every line is text,
 * never markup, and nothing here decides.
 */
import { ShieldAlert } from "lucide-react";
import type {
  AgentDriveWriteOpVm,
  AgentDriveWriteVm,
  AgentWriteExcerptVm,
  AgentWriteHunkVm,
  AgentWriteLineKindVm,
} from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** What each op does, as the card's first words. */
export const WRITE_OP_LABEL: Readonly<Record<AgentDriveWriteOpVm, string>> = {
  create: "Create",
  replace: "Replace",
  edit: "Edit",
  append: "Append to",
  move: "Move",
  delete: "Delete",
};

const LINE_MARK: Readonly<Record<AgentWriteLineKindVm, string>> = {
  context: " ",
  add: "+",
  remove: "−",
};

const LINE_TONE: Readonly<Record<AgentWriteLineKindVm, string>> = {
  context: "text-muted-foreground",
  add: "bg-primary/15",
  remove: "bg-destructive/15",
};

const LINE_WORD: Readonly<Record<AgentWriteLineKindVm, string>> = {
  context: "unchanged",
  add: "added",
  remove: "removed",
};

/** A line drawn in stored order, left to right, whatever its letters' direction (R606-06). */
const STORED_ORDER = "[direction:ltr] [unicode-bidi:bidi-override]";

const BLOCK =
  "my-2 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-sm bg-muted p-2 font-mono text-xs [overflow-wrap:anywhere]";

/** Each line of a hunk with its place on the side it is on: a key that names the line. */
function placed(hunk: AgentWriteHunkVm) {
  let [old, now] = [hunk.oldStart, hunk.newStart];
  return hunk.lines.map((line) => {
    const place =
      line.kind === "add"
        ? `+${now++}`
        : line.kind === "remove"
          ? `-${old++}`
          : `${old++}:${now++}`;
    return { line, place };
  });
}

function Hunk({ hunk }: { hunk: AgentWriteHunkVm }) {
  return (
    <div data-slot="drive-write-hunk">
      <div className="text-muted-foreground">
        Line {hunk.oldStart} → {hunk.newStart}
      </div>
      {placed(hunk).map(({ line, place }) => (
        <div key={place} data-kind={line.kind} className={cn("min-w-0", LINE_TONE[line.kind])}>
          <span aria-hidden="true">{LINE_MARK[line.kind]} </span>
          <span className="sr-only">{LINE_WORD[line.kind]}: </span>
          <span data-slot="drive-write-line-text" className={cn(line.rtl && STORED_ORDER)}>
            {line.text}
          </span>
          {line.note && (
            <span data-slot="drive-write-line-note" className="block text-muted-foreground">
              {line.note}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

function Excerpt({ label, excerpt }: { label: string; excerpt: AgentWriteExcerptVm }) {
  const numbered = excerpt.lines.map((text, at) => ({ text, number: at + 1 }));
  return (
    <figure aria-label={label}>
      <div className={cn(BLOCK, excerpt.rtl && STORED_ORDER)}>
        {numbered.map(({ text, number }) => (
          <div key={number}>{text}</div>
        ))}
      </div>
      {excerpt.more > 0 && (
        <figcaption className="text-muted-foreground text-xs">
          … and {excerpt.more} more {excerpt.more === 1 ? "line" : "lines"}
        </figcaption>
      )}
    </figure>
  );
}

export function DriveWritePreview({ write }: { write: AgentDriveWriteVm }) {
  const { preview } = write;
  return (
    <section data-slot="drive-write" aria-label="The change" className="my-2 min-w-0 text-sm">
      <p className="break-words [overflow-wrap:anywhere]">
        <span className="font-medium">{WRITE_OP_LABEL[write.op]}</span>{" "}
        <code className="font-mono text-xs">{write.path}</code>{" "}
        <span className="text-muted-foreground">in {write.drive}</span>
      </p>
      {write.op === "move" && write.to !== undefined && (
        <dl
          aria-label="Moves"
          className="my-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 rounded-sm bg-muted p-2 font-mono text-xs"
        >
          <dt className="text-muted-foreground">From</dt>
          <dd className="break-words [overflow-wrap:anywhere]">{write.path}</dd>
          <dt className="text-muted-foreground">To</dt>
          <dd className="break-words [overflow-wrap:anywhere]">{write.to}</dd>
        </dl>
      )}
      {preview.lineEndings && (
        <p data-slot="drive-write-line-endings" className="text-muted-foreground text-xs">
          {preview.lineEndings}
        </p>
      )}
      {preview.rtlNote && (
        <p data-slot="drive-write-rtl" className="text-muted-foreground text-xs">
          {preview.rtlNote}
        </p>
      )}
      {preview.tooLarge && (
        <p data-slot="drive-write-too-large" className="text-muted-foreground text-xs">
          {preview.tooLarge}
        </p>
      )}
      {preview.hunks.length > 0 && (
        <figure aria-label="Diff" className={BLOCK}>
          {preview.hunks.map((hunk) => (
            <Hunk key={`${hunk.oldStart}:${hunk.newStart}`} hunk={hunk} />
          ))}
        </figure>
      )}
      {preview.whole && (
        <Excerpt
          label={write.op === "append" ? "The file after the change" : "New text"}
          excerpt={preview.whole}
        />
      )}
      {preview.deleted && <Excerpt label="Deleted text" excerpt={preview.deleted} />}
      {write.untrusted && (
        <p className="flex items-center gap-1 text-held text-xs">
          <ShieldAlert aria-hidden="true" className="size-3.5 shrink-0" />
          Asked in a chat that read outside content.
        </p>
      )}
      {write.audience && <p className="text-muted-foreground text-xs">{write.audience}</p>}
      {write.blocked && (
        <p data-slot="drive-write-blocked" className="text-destructive text-xs">
          Allow is not offered: {write.blocked}
        </p>
      )}
      {write.outcome && (
        <p role="status" data-slot="drive-write-outcome" className="text-xs">
          {write.outcome}
        </p>
      )}
    </section>
  );
}

/**
 * What the model read, wrote and was refused — one row per tool call (Epic 61,
 * Story 61.11, FR-388, FR-389, FR-391, NFR-49).
 *
 * # Four decisions this file carries
 *
 * **The disclosure is part of the row, not a detail of it.** Every bound in
 * `keeper_core::bots::tools` is told to the model in the text of the result —
 * `"truncated at 65536 bytes of 1258291"` — because a silent truncation makes a
 * model confidently wrong (NFR-49). The person reading the conversation is
 * owed the same sentence, for the same reason: an answer drawn from the first
 * 64 kB of a 1.2 MB file is an answer about a prefix. So a bounded result says
 * so on the **collapsed** row, where it cannot be missed, and never only
 * behind the expander.
 *
 * **A refusal is a shape, not a colour.** `DESIGN.md`'s `Never`: no
 * colour-only status. A refused call carries its own glyph, the word
 * "Refused", a dashed rule down its left edge, and `data-refused` — four
 * channels, none of them hue — and its reason is the refusing layer's own
 * sentence, carried verbatim from `keeper-core`. A paraphrase of a security
 * refusal is a second, unproved wording of the rule.
 *
 * **OKF provenance is first-class.** `okf` on the row is the one thing keeper
 * can say here that a generic filesystem tool cannot: whether a *person* ever
 * looked at the note the model just read. It renders beside the outcome rather
 * than inside the expanded body, and `humanReviewed` is read as the answer
 * rather than inferred from `verifiedActor` — inferring it is how a
 * model-generated note comes to read as person-verified.
 *
 * **The DOM is bounded as well as the result.** A 64 kB read expanded into a
 * text node is 64 kB of layout in a pane a person is trying to scroll, so the
 * body and the arguments are each clipped to {@link BOT_TOOL_DOM_CHARS} with a
 * sentence saying what was clipped and that the model got all of it. The bound
 * here is about this pane; the bound in Rust is about the context window, and
 * neither substitutes for the other.
 *
 * # What is deliberately absent
 *
 * No live region. Tool rows arrive one after another mid-stream, and a
 * transcript that announces each one talks over the answer being read — the
 * pane's `role="status"` captions are for the turn's state, and `role="log"`
 * belongs to the transcript, not to seven rows inside one message (WCAG 2.2 SC
 * 4.1.3 covers status messages; a list of results is explicitly not one).
 * Every row is reachable as a real disclosure instead.
 */
import { Ban, ChevronDown, ChevronRight } from "lucide-react";
import { useId, useState } from "react";
import { formatFileSize } from "@/lib/file-size";
import type { BotToolCallVm, ToolName } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/**
 * The verb each tool is called by, in a person's words.
 *
 * Past tense, because a row is a record of something that already happened,
 * and a verb rather than the wire name: `drive_glob` is what the model called,
 * and "found files in" is what it did. The wire name is still shown for a verb
 * that does not exist, which is the only case where the model's own spelling is
 * the fact worth reporting.
 */
export const BOT_TOOL_VERB: Record<ToolName, string> = {
  list: "listed",
  read: "read",
  glob: "found files in",
  grep: "searched",
  stat: "checked",
  write: "wrote",
  edit: "edited",
};

/** What a row whose call did not happen says, beside its reason. */
export const BOT_TOOL_REFUSED_LABEL = "Refused";

/**
 * What a row refused by a grant says instead.
 *
 * The one distinction that changes what a person can do next: a grant is
 * theirs to widen, and a containment refusal is not.
 */
export const BOT_TOOL_GRANT_DENIED_LABEL = "Refused by this bot's grant";

/** The expanded row's two headings. */
export const BOT_TOOL_ARGUMENTS_LABEL = "What it asked for";
export const BOT_TOOL_RESULT_LABEL = "What the model was told";

/** What a row with no target says where the path would be. */
export const BOT_TOOL_NO_PATH = "a path it could not name";

/**
 * How many characters of one argument blob or one result body this pane puts in
 * the DOM.
 *
 * Two thousand is a screenful and a half of monospace — enough to see what came
 * back — where the result itself may be 80 kB. The clip is the pane's, not the
 * model's, and {@link botToolClipNotice} says so in as many words so nobody
 * reads a short body as a short answer.
 */
export const BOT_TOOL_DOM_CHARS = 2000;

/** The accessible name of the list of rows inside one answer. */
export const BOT_TOOL_LIST_LABEL = "What this answer did on your drive";

/**
 * The caption a replayed conversation shows.
 *
 * `bot_messages.tool_call_count` is a scalar column, so a conversation read
 * back from the store knows how many calls a turn made without holding the
 * calls themselves. Saying the count is honest; rendering nothing would make a
 * turn that spent itself on tools indistinguishable from one that answered
 * nothing.
 */
export function botToolCallCaption(count: number): string {
  return count === 1 ? "Called 1 tool." : `Called ${count} tools.`;
}

/** What this pane clipped, and who still got the whole thing. */
export function botToolClipNotice(shown: number, total: number): string {
  return `Showing the first ${shown} characters of ${total} in this pane. The model was given all of them.`;
}

/** The verb and the path, in a person's words: `read work/notes/a.md`. */
export function botToolSummary(call: BotToolCallVm): string {
  const path = call.displayPath ?? BOT_TOOL_NO_PATH;
  if (call.name === null) {
    // A verb that does not exist: the model's own spelling is the fact.
    return `asked for ${call.requestedName}`;
  }
  return `${BOT_TOOL_VERB[call.name]} ${path}`;
}

/**
 * What came back, in one phrase, or `null` for a call that never ran.
 *
 * Sizes go through {@link formatFileSize}, the mirror of
 * `keeper_core::size::format_file_size`, so one file size is spelled one way in
 * this pane, in Files and in Finder.
 */
export function botToolOutcomeWord(call: BotToolCallVm): string | null {
  switch (call.outcome) {
    case "text":
      return call.bytes === null ? "read" : `read ${formatFileSize(call.bytes)}`;
    case "entries":
      return call.entries === 1 ? "1 entry" : `${call.entries ?? 0} entries`;
    case "notMaterialized":
      // Reading did not fetch it, and the row says which act would.
      return call.ofBytes === null
        ? "not on this machine yet"
        : `not on this machine yet (${formatFileSize(call.ofBytes)})`;
    case "wrote":
      return call.bytes === null ? "wrote" : `wrote ${formatFileSize(call.bytes)}`;
    case "refused":
      return null;
    default:
      return null;
  }
}

/**
 * The bound that was hit, worded as the model was told it, or `null` when the
 * result was whole.
 *
 * **`null` when and only when nothing was cut.** A notice on an unbounded
 * result teaches a reader to ignore the notice.
 */
export function botToolTruncationNotice(call: BotToolCallVm): string | null {
  if (call.truncatedAtBytes !== null) {
    const of = call.ofBytes === null ? "" : ` of ${formatFileSize(call.ofBytes)}`;
    return `truncated at ${formatFileSize(call.truncatedAtBytes)}${of}`;
  }
  if (call.truncatedAtEntries !== null) {
    const of = call.ofEntries === null ? "" : ` of ${call.ofEntries}`;
    return `truncated at ${call.truncatedAtEntries} entries${of}`;
  }
  return null;
}

/**
 * A note's provenance, in one line, or `null` where the read carried none.
 *
 * The review clause reads `humanReviewed` and nothing else: a note generated by
 * a model and never looked at says so, and a note a person signed says who.
 * Deriving "a person reviewed this" from the presence of a `verified` block is
 * exactly the mistake this line exists to prevent.
 */
export function botToolProvenanceLine(call: BotToolCallVm): string | null {
  const okf = call.okf;
  if (okf === null) {
    return null;
  }
  const parts = [okf.docType === null ? "no OKF type" : `OKF type ${okf.docType}`];
  if (okf.generatedBy !== null) {
    const actor = okf.generatedActor === null ? "" : ` (${okf.generatedActor})`;
    parts.push(`generated by ${okf.generatedBy}${actor}`);
  }
  if (okf.humanReviewed) {
    parts.push(
      okf.verifiedBy === null ? "reviewed by a person" : `reviewed by a person (${okf.verifiedBy})`,
    );
  } else if (okf.verifiedActor !== null) {
    parts.push(`checked by ${okf.verifiedActor}, not by a person`);
  } else {
    parts.push("nobody has reviewed it");
  }
  return parts.join(" · ");
}

/** One argument blob or result body, clipped to what this pane will render. */
function clipped(text: string): { text: string; notice: string | null } {
  if (text.length <= BOT_TOOL_DOM_CHARS) {
    return { text, notice: null };
  }
  return {
    text: text.slice(0, BOT_TOOL_DOM_CHARS),
    notice: botToolClipNotice(BOT_TOOL_DOM_CHARS, text.length),
  };
}

/** One tool call: a collapsed disclosure that opens onto what was exchanged. */
export function BotToolCallRow({ call }: { call: BotToolCallVm }) {
  const [open, setOpen] = useState(false);
  const bodyId = useId();
  const refused = call.refusal !== null || call.outcome === "refused";
  const summary = botToolSummary(call);
  const outcome = botToolOutcomeWord(call);
  const truncation = botToolTruncationNotice(call);
  const provenance = botToolProvenanceLine(call);
  const args = clipped(call.arguments);
  const body = clipped(call.result);
  return (
    <li
      className={cn(
        "flex min-w-0 flex-col gap-0.5 border-l-2 pl-2",
        // Shape, not hue: a refused call is walled off by a dashed rule and a
        // solid one marks a call that happened.
        refused ? "border-dashed" : "border-solid",
      )}
      data-slot="bot-tool-call"
      data-refused={refused ? "true" : "false"}
      data-tool={call.name ?? call.requestedName}
    >
      <button
        type="button"
        aria-expanded={open}
        aria-controls={open ? bodyId : undefined}
        onClick={() => setOpen((shown) => !shown)}
        className="flex min-w-0 items-start gap-1 text-left text-muted-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        {open ? (
          <ChevronDown aria-hidden="true" className="mt-0.5 size-3 shrink-0" />
        ) : (
          <ChevronRight aria-hidden="true" className="mt-0.5 size-3 shrink-0" />
        )}
        {refused && <Ban aria-hidden="true" className="mt-0.5 size-3 shrink-0" />}
        <span className="min-w-0 break-words">
          <span className="text-foreground">{summary}</span>
          {refused ? (
            <span>
              {" "}
              — {call.grantDenied ? BOT_TOOL_GRANT_DENIED_LABEL : BOT_TOOL_REFUSED_LABEL}
            </span>
          ) : (
            outcome !== null && <span className="figures"> — {outcome}</span>
          )}
          {/* The bound belongs on the collapsed row: an answer about a prefix
              is a different answer, and a reader who has to expand to find that
              out has already believed the wrong thing. */}
          {truncation !== null && <span className="figures">, {truncation}</span>}
        </span>
      </button>
      {refused && call.refusal !== null && (
        // The refusing layer's own words. keeper adds none of its own.
        <p className="break-words pl-4 text-muted-foreground text-xs">{call.refusal}</p>
      )}
      {provenance !== null && (
        <p
          className="break-words pl-4 text-muted-foreground text-xs"
          data-slot="bot-tool-provenance"
        >
          {provenance}
        </p>
      )}
      {open && (
        <div className="flex min-w-0 flex-col gap-1 pl-4" id={bodyId}>
          <p className="text-muted-foreground text-xs">{BOT_TOOL_ARGUMENTS_LABEL}</p>
          <pre className="min-w-0 whitespace-pre-wrap break-words rounded-sm bg-muted px-2 py-1 font-mono text-xs">
            {args.text}
          </pre>
          {args.notice !== null && (
            <p className="figures text-muted-foreground text-xs">{args.notice}</p>
          )}
          {call.result.length > 0 && (
            <>
              <p className="text-muted-foreground text-xs">{BOT_TOOL_RESULT_LABEL}</p>
              <pre className="min-w-0 whitespace-pre-wrap break-words rounded-sm bg-muted px-2 py-1 font-mono text-xs">
                {body.text}
              </pre>
              {body.notice !== null && (
                <p className="figures text-muted-foreground text-xs">{body.notice}</p>
              )}
            </>
          )}
        </div>
      )}
    </li>
  );
}

/**
 * What one answer did on the drive.
 *
 * `calls` is the detail the turn recorded; `count` is the scalar column a
 * replayed conversation has instead. With rows, the rows; with only a count,
 * the count; with neither, nothing at all — an empty element in a flex column
 * still spends its parent's gap.
 */
export function BotToolCall({ count, calls = [] }: { count: number; calls?: BotToolCallVm[] }) {
  if (calls.length > 0) {
    return (
      <ul aria-label={BOT_TOOL_LIST_LABEL} className="flex min-w-0 flex-col gap-1">
        {calls.map((call) => (
          <BotToolCallRow call={call} key={call.id} />
        ))}
      </ul>
    );
  }
  if (count <= 0) {
    return null;
  }
  return <p className="figures text-muted-foreground text-xs">{botToolCallCaption(count)}</p>;
}

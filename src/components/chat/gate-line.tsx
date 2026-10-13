/**
 * A gate room's lines (UX-DR147): the outside system's requests and the gate's
 * word on each, as Rust reads them (`GateLineVm`).
 *
 * **Whose words are whose.** A request is the outside system's text: it is drawn
 * as quoted data on a dashed, uncoloured surface — never the agent's card surface
 * and never keeper's own words — under a head that says whose it is and where it
 * stands. The gate's word (an answer, a ticket, a result, a refusal) is the
 * agent's message with a head naming the request it speaks of. Every chip is
 * Rust's state; the request's name arrives quoted and cut by Rust, and nothing
 * here re-reads the message.
 *
 * A ticket names the session that will work it by its id; once its result is
 * in, that session worked it. A sync request shows the deadline the gate held
 * it to. The person is an observer in this room: nothing here is a control.
 */
import { CircleCheck, CircleX, Inbox, Reply, Ticket } from "lucide-react";
import type { GateLineVm, GateReasonVm, GateStepVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** A step as its chip says it. */
export const GATE_STEP_WORD: Record<GateStepVm, string> = {
  answered: "answered",
  ticket: "ticket",
  result: "result",
  refused: "refused",
};

/** Why a request was refused, as its chip says it. */
export const GATE_REASON_WORD: Record<GateReasonVm, string> = {
  busy: "busy",
  mode: "not taken in this mode",
  no_answer: "no answer",
  interrupted: "cut off",
  denied: "declined by a person",
  expired: "not decided in time",
  failed: "could not be handed on",
  withheld: "held back by its owner",
  unreleased: "not let out",
};

const STEP_ICON = {
  answered: Reply,
  ticket: Ticket,
  result: CircleCheck,
  refused: CircleX,
} as const;

/** The request's name, or that it is unnamed (an async request, named by its event). */
function requestName(line: GateLineVm): string {
  return line.request ?? "an unnamed request";
}

/** The chip's words: the step, and why when refused. */
export function gateChipText(line: GateLineVm): string {
  const step = GATE_STEP_WORD[line.state];
  return line.reason === null ? step : `${step} · ${GATE_REASON_WORD[line.reason]}`;
}

/**
 * The accessible name of a gate line's bubble. A request names its sender as
 * Rust quoted it (the outside system chose the name); `sender` is the room's
 * name for anyone else.
 */
export function gateLineName(line: GateLineVm, sender: string): string {
  return line.role === "request"
    ? `Request ${requestName(line)} from ${line.senderName ?? sender}, ${gateChipText(line)}`
    : `${sender}: ${gateChipText(line)} for ${requestName(line)}`;
}

/** A sync request's deadline, in seconds as a person reads them. */
const DEADLINE_FORMAT = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });

/** The bubble surface of a gate line. */
export function gateBubbleClass(line: GateLineVm): string {
  return line.role === "request"
    ? "border border-dashed border-muted-foreground/60 bg-background text-foreground"
    : "border border-border border-l-2 border-l-primary bg-card text-foreground";
}

/** The state chip. */
function StepChip({ line }: { line: GateLineVm }) {
  const Icon = STEP_ICON[line.state];
  return (
    <span
      data-slot="gate-chip"
      data-state={line.state}
      className={cn(
        "inline-flex shrink-0 items-center gap-1 rounded-full border px-1.5 py-px font-medium text-xs",
        line.state === "refused"
          ? "border-destructive/50 text-destructive"
          : line.state === "ticket"
            ? "border-held/60 text-held"
            : "border-border text-muted-foreground",
      )}
    >
      <Icon aria-hidden="true" className="size-3" />
      {gateChipText(line)}
    </span>
  );
}

/** Above a gate line's text: whose it is, which request, and where it stands. */
export function GateHead({ line }: { line: GateLineVm }) {
  return (
    <div className="mb-1 flex flex-col gap-0.5">
      <span className="flex min-w-0 flex-wrap items-center gap-1.5 text-muted-foreground text-xs">
        {line.role === "request" ? (
          <Inbox aria-hidden="true" className="size-3 shrink-0" />
        ) : (
          <Reply aria-hidden="true" className="size-3 shrink-0" />
        )}
        <span className="min-w-0 truncate">
          {line.role === "request" ? "Request " : "On request "}
          <span className="font-mono">{requestName(line)}</span>
        </span>{" "}
        <StepChip line={line} />
        {line.deadlineMs !== null && (
          <span data-slot="gate-deadline">
            within {DEADLINE_FORMAT.format(line.deadlineMs / 1000)} s
          </span>
        )}
      </span>
      {line.ticket !== null && (
        <span className="text-muted-foreground text-xs">
          {line.state === "result" ? "Worked in session " : "Ticket "}
          <span className="break-all font-mono" title={line.ticket}>
            {line.ticket}
          </span>
        </span>
      )}
    </div>
  );
}

/** A request's text: the outside system's words, quoted as data. */
export function GateRequestText({ body }: { body: string }) {
  return (
    <blockquote
      data-slot="gate-request-text"
      className="whitespace-pre-wrap break-words border-muted-foreground/40 border-l-2 pl-2 font-mono text-xs"
    >
      <span className="sr-only">The system sent: </span>
      {body}
    </blockquote>
  );
}

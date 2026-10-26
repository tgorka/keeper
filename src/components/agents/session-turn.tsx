import { Ban, ChevronDown, ChevronRight } from "lucide-react";
import { useId, useState } from "react";
import { AgentAnswer } from "@/components/agents/agent-answer";
import { SessionAttachments } from "@/components/agents/session-attachments";
import { countLabel, TOKENS } from "@/lib/count-label";
import type {
  AgentLiveTurnVm,
  AgentMessageDetailsVm,
  AgentMessageVm,
  AgentToolStepVm,
  AgentTurnFoldVm,
} from "@/lib/ipc/client";

/** No live region around token text: announce the state, not the whole answer per delta.
 * The turn's work and thinking are its fold's, drawn above this. */
export function SessionLiveTurn({ turn, agentName }: { turn: AgentLiveTurnVm; agentName: string }) {
  return (
    <li className="min-w-0" aria-label={`Answer from ${agentName}`}>
      <div className="mb-1 flex flex-wrap items-baseline gap-x-2 text-xs">
        <span className="font-medium">{agentName}</span>
        {!turn.stopped && (
          <span role="status" className="text-muted-foreground">
            Answering…
          </span>
        )}
      </div>
      <AgentAnswer body={turn.text} />
      {turn.stopped && (
        <p
          role="status"
          className="mt-3 border-border border-t pt-2 font-medium text-muted-foreground text-xs"
        >
          The answer stopped.
        </p>
      )}
    </li>
  );
}

/**
 * One turn's work, folded into one line above its answer (Q109-13 a): Rust's
 * summary sentence, then each refusal or failure as one line of its own, the
 * thinking where Rust sent it, and — opened — the lead-ins and tool calls in a
 * bounded scroll area. Opened by default only in the verbose view mode; the
 * answer is never inside it. Rust decides every word and what a viewer who
 * does not read the trace receives (no thinking, no steps).
 */
export function TurnFold({
  fold,
  leadIns,
  agentName,
  verbose,
}: {
  fold: AgentTurnFoldVm;
  leadIns: AgentMessageVm[];
  agentName: string;
  verbose: boolean;
}) {
  // A person's own toggle lasts until the view mode changes; a change
  // forgets it, set during render rather than after a paint.
  const [toggled, setToggled] = useState<boolean | null>(null);
  const [mode, setMode] = useState(verbose);
  if (mode !== verbose) {
    setMode(verbose);
    setToggled(null);
  }
  const bodyId = useId();
  const hasBody = leadIns.length > 0 || fold.steps.length > 0;
  const open = hasBody && (toggled ?? verbose);
  const running = fold.status === "running";
  const summary = (
    <span className="min-w-0 break-words [overflow-wrap:anywhere]">{fold.summary}</span>
  );
  return (
    <li
      className="min-w-0"
      data-slot="turn-fold"
      data-status={fold.status}
      aria-label={`Work of ${agentName}`}
    >
      {hasBody ? (
        <button
          type="button"
          className="flex min-h-11 w-full min-w-0 items-center gap-1 rounded-md px-1 text-left text-muted-foreground text-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={() => setToggled(!open)}
        >
          {open ? (
            <ChevronDown aria-hidden="true" className="size-4 shrink-0" />
          ) : (
            <ChevronRight aria-hidden="true" className="size-4 shrink-0" />
          )}
          {running ? <span role="status">{summary}</span> : summary}
        </button>
      ) : (
        <p
          role={running ? "status" : undefined}
          className="flex min-h-11 min-w-0 items-center text-muted-foreground text-xs"
        >
          {summary}
        </p>
      )}
      {fold.failures.length > 0 && (
        <ul aria-label="Refusals and failures" className="flex min-w-0 flex-col gap-1">
          {fold.failures.map((failure, index) => (
            <li
              // A close's error has no step; its place among the failures is its identity.
              key={failure.stepId || `close-${index}`}
              data-slot="turn-failure"
              className="flex min-w-0 items-start gap-1.5 text-muted-foreground text-xs"
            >
              <Ban aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
              <span className="min-w-0 break-words [overflow-wrap:anywhere]">
                {failure.sentence}
              </span>
            </li>
          ))}
        </ul>
      )}
      {fold.reasoning && (
        <section
          aria-label="Thinking"
          className="mt-2 max-h-64 min-w-0 overflow-y-auto overscroll-contain border-border border-l-2 pl-3"
        >
          <h3 className="mb-1 font-medium text-muted-foreground text-xs">Thinking</h3>
          <p className="whitespace-pre-wrap break-words text-muted-foreground text-sm [overflow-wrap:anywhere]">
            {fold.reasoning}
          </p>
        </section>
      )}
      {open && (
        <div
          id={bodyId}
          data-slot="turn-fold-body"
          className="mt-1 flex max-h-[min(24rem,40vh)] min-w-0 flex-col gap-3 overflow-y-auto overscroll-contain rounded-md border border-border p-3"
        >
          {leadIns.map((message) => (
            <div key={message.id} className="min-w-0 text-muted-foreground">
              <AgentAnswer body={message.body} />
              {!!message.attachments?.length && (
                <SessionAttachments attachments={message.attachments} />
              )}
            </div>
          ))}
          {fold.steps.length > 0 && <TurnSteps steps={fold.steps} />}
        </div>
      )}
    </li>
  );
}

/** Each call behind its own disclosure, its result bounded on its own. */
export function TurnSteps({ steps }: { steps: AgentToolStepVm[] }) {
  return (
    <ul aria-label="Tool calls" className="flex min-w-0 flex-col gap-1">
      {steps.map((step) => (
        <li key={step.id} className="min-w-0">
          <details className="rounded-md border border-border">
            <summary className="min-h-11 cursor-pointer break-all px-3 py-3 text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              {step.name} · {step.statusLabel}
            </summary>
            <div className="space-y-1 px-3 pb-3 text-xs">
              {step.startedAtMs !== undefined && (
                <p>Started {new Date(step.startedAtMs).toLocaleTimeString()}</p>
              )}
              {step.endedAtMs !== undefined && (
                // How it ended, in Rust's word: "Interrupted 12:03", never "Finished".
                <p>
                  {step.statusLabel} {new Date(step.endedAtMs).toLocaleTimeString()}
                </p>
              )}
              {step.result !== undefined && (
                <pre
                  data-slot="step-result"
                  className="max-h-64 overflow-y-auto whitespace-pre-wrap break-all font-mono"
                >
                  {step.result}
                </pre>
              )}
            </div>
          </details>
        </li>
      ))}
    </ul>
  );
}

/** A missing measurement produces no label and no invented zero. */
export function SessionAnswerDetails({ details }: { details: AgentMessageDetailsVm }) {
  const facts: { label: string; value: string }[] = [];
  if (details.model !== undefined) facts.push({ label: "Model", value: details.model });
  if (details.promptTokens !== undefined)
    facts.push({ label: "Prompt tokens", value: countLabel(details.promptTokens, TOKENS) });
  if (details.completionTokens !== undefined)
    facts.push({ label: "Completion tokens", value: countLabel(details.completionTokens, TOKENS) });
  if (details.totalTokens !== undefined)
    facts.push({ label: "Total tokens", value: countLabel(details.totalTokens, TOKENS) });
  if (details.ttftMs !== undefined)
    facts.push({ label: "Time to first token", value: `${details.ttftMs.toLocaleString()} ms` });
  if (details.durationMs !== undefined)
    facts.push({ label: "Duration", value: `${details.durationMs.toLocaleString()} ms` });
  if (details.finish !== undefined) facts.push({ label: "Finish", value: details.finish });
  if (details.error !== undefined) facts.push({ label: "Error", value: details.error });
  if (!facts.length) return null;
  return (
    <dl
      aria-label="Answer details"
      className="mt-3 grid min-w-0 grid-cols-[minmax(0,auto)_minmax(0,1fr)] gap-x-3 gap-y-1 text-muted-foreground text-xs"
    >
      {facts.map((fact) => (
        <div key={fact.label} className="contents">
          <dt>{fact.label}</dt>
          <dd className="figures break-words [overflow-wrap:anywhere]">{fact.value}</dd>
        </div>
      ))}
    </dl>
  );
}

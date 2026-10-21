import { AgentAnswer } from "@/components/agents/agent-answer";
import { countLabel, TOKENS } from "@/lib/count-label";
import type { AgentLiveTurnVm, AgentMessageDetailsVm, AgentToolStepVm } from "@/lib/ipc/client";

/** No live region around token text: announce the state, not the whole answer per delta. */
export function SessionLiveTurn({
  turn,
  agentName,
  readsTrace,
  showReasoning,
}: {
  turn: AgentLiveTurnVm;
  agentName: string;
  readsTrace: boolean;
  showReasoning: boolean;
}) {
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
      {turn.tool && (
        <p role="status" className="mt-2 break-all text-muted-foreground text-xs">
          {turn.stopped ? "Last tool" : "Calling"}: {turn.tool}
        </p>
      )}
      {readsTrace && (
        <SessionTrace reasoning={showReasoning ? turn.reasoning : undefined} steps={turn.steps} />
      )}
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

export function SessionTrace({
  reasoning,
  steps,
}: {
  reasoning?: string;
  steps?: AgentToolStepVm[];
}) {
  return (
    <>
      {reasoning && (
        <section aria-label="Thinking" className="mt-3 min-w-0 border-border border-l-2 pl-3">
          <h3 className="mb-1 font-medium text-muted-foreground text-xs">Thinking</h3>
          <p className="whitespace-pre-wrap break-words text-muted-foreground text-sm [overflow-wrap:anywhere]">
            {reasoning}
          </p>
        </section>
      )}
      {!!steps?.length && (
        <ul aria-label="Tool calls" className="mt-2 flex min-w-0 flex-col gap-1">
          {steps.map((step) => (
            <li key={step.id} className="min-w-0">
              <details className="rounded-md border border-border">
                <summary className="min-h-11 cursor-pointer break-all px-3 py-3 text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  {step.name} ·{" "}
                  {step.status === "started"
                    ? "Started"
                    : step.status === "done"
                      ? "Finished"
                      : "Refused"}
                </summary>
                <div className="space-y-1 px-3 pb-3 text-xs">
                  {step.startedAtMs !== undefined && (
                    <p>Started {new Date(step.startedAtMs).toLocaleTimeString()}</p>
                  )}
                  {step.endedAtMs !== undefined && (
                    <p>Finished {new Date(step.endedAtMs).toLocaleTimeString()}</p>
                  )}
                  {step.result !== undefined && (
                    <pre className="max-h-64 overflow-y-auto whitespace-pre-wrap break-all font-mono">
                      {step.result}
                    </pre>
                  )}
                </div>
              </details>
            </li>
          ))}
        </ul>
      )}
    </>
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

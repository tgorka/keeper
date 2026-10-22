/**
 * What waits for this person in the agents' databases (106.x): every approval
 * a held database lists, decided on its card ({@link DatabaseApprovalCard}),
 * and the open questions of a session, beside the field where a free-text
 * answer is written. The panel over an agent's Matrix room left keeper in
 * 108.2.
 */
import { useEffect, useState } from "react";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import { logFailure } from "@/lib/failure";
import {
  type AgentApprovalVm,
  type AgentMessageVm,
  agentApprovalsSubscribe,
  agentApprovalsUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** All pending database decisions, including sessions not currently open. */
export function DatabaseDecisionsPanel() {
  const [rows, setRows] = useState<AgentApprovalVm[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    let id: string | null = null;
    void agentApprovalsSubscribe((next) => {
      if (active) setRows(next);
    })
      .then((next) => {
        if (active) id = next;
        else void agentApprovalsUnsubscribe(next).catch(logFailure("agents.decisions.close"));
      })
      .catch((raw: unknown) => {
        if (active) setError(syncErrorMessage(raw, "keeper could not read pending decisions."));
      });
    return () => {
      active = false;
      if (id !== null)
        void agentApprovalsUnsubscribe(id).catch(logFailure("agents.decisions.close"));
    };
  }, []);
  if (error)
    return (
      <p role="alert" className="p-3 text-destructive text-sm">
        {error}
      </p>
    );
  if (rows.length === 0) return null;
  return (
    <details className="border-border border-b p-3">
      <summary className="cursor-pointer font-medium text-sm">
        Waiting for you · {rows.length}
      </summary>
      <div className="mt-3 flex flex-col gap-3">
        {rows.map((row) => (
          <DatabaseApprovalCard
            key={JSON.stringify([row.database, row.sessionId, row.id, row.bindingDigest])}
            approval={row}
          />
        ))}
      </div>
    </details>
  );
}

/** Rust's open-ask flags, repeated beside the field where a free-text answer is written. */
export function SessionQuestionsPanel({ messages }: { messages: readonly AgentMessageVm[] }) {
  const questions = messages.filter((message) => message.ask?.open);
  if (questions.length === 0) return null;
  return (
    <aside
      aria-label="Waiting for your answer"
      className="max-h-32 shrink-0 overflow-y-auto border-border border-t bg-muted/30 px-3 py-2"
    >
      <h3 className="font-medium text-sm">Waiting for your answer</h3>
      {questions.map((question) => (
        <p key={question.id} className="break-words text-sm [overflow-wrap:anywhere]">
          {question.body}
        </p>
      ))}
      <p className="text-muted-foreground text-xs">
        Choose an option in the message, or type your answer below.
      </p>
    </aside>
  );
}

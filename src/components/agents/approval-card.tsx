/**
 * An agent's approval request, as the agents' database holds it (AD-428): the
 * tier as a word with a weight (an icon and the card's left edge: thin and
 * quiet at T2, the held colour at T3, the destructive colour at T4 — the word
 * always says it, so no tier is told by colour alone), keeper's one-sentence
 * summary, the exact action and its binding digest, Rust's sentence, and the
 * decide buttons only where Rust says this device decides. A decision is the
 * approval row the runtime reads; the card changes when the replica has it.
 *
 * The card over an agent's Matrix room's events left keeper in 108.2.
 */
import type { LucideIcon } from "lucide-react";
import { OctagonAlert, Shield, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { type AgentApprovalVm, agentSessionApprovalDecide } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/**
 * Each tier's weight. T0/T1 never wait for a person and T5 is never done, so
 * only T2–T4 arrive; a tier outside them is drawn at the heaviest weight
 * rather than guessed lighter.
 */
const TIER_WEIGHT: Readonly<Record<number, { icon: LucideIcon; edge: string; tone: string }>> = {
  2: { icon: Shield, edge: "border-l-2 border-l-muted-foreground", tone: "text-muted-foreground" },
  3: { icon: ShieldAlert, edge: "border-l-4 border-l-held", tone: "text-held" },
  4: { icon: OctagonAlert, edge: "border-l-4 border-l-destructive", tone: "text-destructive" },
};
const HEAVIEST = TIER_WEIGHT[4];

export const APPROVAL_DECIDE_FAILED =
  "keeper could not send that decision. Nothing was decided; try again.";

/** A database approval is bound to the exact action and digest Rust supplied. */
export function DatabaseApprovalCard({ approval }: { approval: AgentApprovalVm }) {
  const [sending, setSending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: an authoritative state transition retires an earlier command failure
  useEffect(() => {
    setFailure(null);
  }, [approval.state, approval.canDecide, approval.sentence]);
  const decide = async (approve: boolean) => {
    if (sending || !approval.canDecide) return;
    setSending(true);
    setFailure(null);
    try {
      await agentSessionApprovalDecide(
        { database: approval.database, sessionId: approval.sessionId },
        approval.id,
        approval.bindingDigest,
        approve,
      );
    } catch (raw) {
      setFailure(syncErrorMessage(raw, APPROVAL_DECIDE_FAILED));
    } finally {
      setSending(false);
    }
  };
  const weight = TIER_WEIGHT[Number(approval.tier.replace("T", ""))] ?? HEAVIEST;
  const Icon = weight.icon;
  return (
    <article
      aria-label={`Approval: ${approval.summary}`}
      className={cn("min-w-0 rounded-md border border-border bg-card p-3", weight.edge)}
    >
      <div
        className={cn("mb-2 flex flex-wrap items-center gap-2 font-medium text-sm", weight.tone)}
      >
        <Icon className="size-4" aria-hidden="true" />
        <span>{approval.tier}</span>
        <span>{approval.toolName}</span>
        <span className="break-all text-muted-foreground text-xs">{approval.database}</span>
      </div>
      <p className="break-words text-sm">{approval.summary}</p>
      {approval.action !== undefined && (
        <figure aria-label="Exact action">
          <pre className="my-2 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-sm bg-muted p-2 text-xs [overflow-wrap:anywhere]">
            {approval.action}
          </pre>
        </figure>
      )}
      <details className="my-2 text-muted-foreground text-xs">
        <summary className="min-h-11 min-w-11 cursor-pointer content-center">
          Binding digest
        </summary>
        <p className="break-all">{approval.bindingDigest}</p>
      </details>
      <p role={approval.refused ? "alert" : "status"} className="text-muted-foreground text-xs">
        {approval.sentence}
      </p>
      {failure && !approval.refused && (
        <p role="alert" className="mt-2 text-destructive text-sm">
          {failure}
        </p>
      )}
      {approval.canDecide && (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            className="min-h-11 min-w-11"
            size="sm"
            disabled={sending}
            onClick={() => void decide(true)}
          >
            Approve once
          </Button>
          <Button
            size="sm"
            className="min-h-11 min-w-11"
            variant="outline"
            disabled={sending}
            onClick={() => void decide(false)}
          >
            Deny
          </Button>
        </div>
      )}
    </article>
  );
}

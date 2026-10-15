/**
 * What the decisions panel above an agent room's composer offers (Story 101.3,
 * R418, R430, R435): every approval this device may decide that still waits,
 * then every question waiting for this person that is answered here — each as
 * numbered options that send exactly what the timeline card's buttons, or
 * typing the answer, would send. No new protocol and no new decision: who may
 * decide is the card VM's `canDecide`, which questions wait, where each is
 * answered and where its answer stands are Rust's (`subscribeAgentAsks`).
 */
import { approvalUnseen } from "@/components/agents/approval-card";
import type { AgentAskVm } from "@/lib/ipc/gen/AgentAskVm";
import type { ApprovalCardVm } from "@/lib/ipc/gen/ApprovalCardVm";
import type { ApprovalDecision } from "@/lib/ipc/gen/ApprovalDecision";
import type { ApprovalScope } from "@/lib/ipc/gen/ApprovalScope";
import type { ApprovalVm } from "@/lib/ipc/gen/ApprovalVm";

/** One option of a decision. */
export type DecisionOption =
  | {
      kind: "approval";
      label: string;
      detail: string | null;
      decision: ApprovalDecision;
      scope: ApprovalScope;
    }
  | { kind: "answer"; label: string; detail: string | null; text: string };

/** One decision the panel offers. */
export interface Decision {
  /** The approval record's id or the ask's id. */
  id: string;
  kind: "approval" | "ask";
  /** What is being decided, in the card's or the agent's words. */
  question: string;
  /** Who asks. */
  askedBy: string;
  /** The approval's card, whose action the panel shows before any approve option. */
  card: ApprovalCardVm | null;
  /** A sentence the panel owes beside the options, if any. */
  note: string | null;
  /** The digest an approval's decision carries; `null` for a question. */
  bindingDigest: string | null;
}

/** A question whose answer the person already gave: it leaves when its proxy passes it on. */
export interface Settling {
  id: string;
  question: string;
  askedBy: string;
  state: "held" | "answered";
}

/** The deny option's label, the card's own. */
export const DECISION_DENY = "Deny";

/** An attached `run` is approved once its full action is shown here. */
export const DECISION_SHOW_RUN_FIRST =
  "Show the full action above to approve this run: it holds what the run is bound to and what it releases. You can deny it now.";

/** A question that offers no choices is answered in the message field. */
export const DECISION_TYPE_ANSWER = "Type your answer in the message field below.";

/** The answer an unattended run would take, marked beside its choice. */
export const DECISION_DEFAULT_MARK = "the default";

/** A held answer: Undo puts the question back. */
export const DECISION_HELD = "Your answer is waiting to send. Undo puts the question back.";

/** A sent answer, until the proxy passes it on. */
export const DECISION_ANSWERED = "Answered. The question leaves when your agent passes it on.";

/**
 * The decisions, in the order the panel offers them: approvals in the room's
 * order, then the questions still open, oldest first; `done` holds the
 * approvals this panel already decided, which leave at once rather than when the
 * room says so. Questions leave only when Rust says they did — a held answer is
 * no answer (R430 05).
 */
export function decisionsOf(
  approvals: readonly ApprovalVm[],
  asks: readonly AgentAskVm[],
  done: ReadonlySet<string>,
): Decision[] {
  const decisions: Decision[] = [];
  for (const request of approvals) {
    for (const card of request.cards) {
      if (card.state.state !== "pending" || !card.canDecide || done.has(card.id)) {
        continue;
      }
      decisions.push({
        id: card.id,
        kind: "approval",
        question: card.summary,
        askedBy: card.chain.map((person) => person.name).join(" → "),
        card,
        note: null,
        bindingDigest: card.bindingDigest,
      });
    }
  }
  for (const ask of asks) {
    if (ask.state !== "open") {
      continue;
    }
    decisions.push({
      id: ask.id,
      kind: "ask",
      question: ask.question,
      askedBy: ask.askedBy,
      card: null,
      note: ask.choices.length === 0 ? DECISION_TYPE_ANSWER : null,
      bindingDigest: null,
    });
  }
  return decisions;
}

/** The questions answered here whose answer is held or on its way. */
export function settlingOf(asks: readonly AgentAskVm[]): Settling[] {
  return asks.flatMap((ask) =>
    ask.state === "open"
      ? []
      : [{ id: ask.id, question: ask.question, askedBy: ask.askedBy, state: ask.state }],
  );
}

/**
 * A decision's options: a question's choices, or an approval's ways to approve
 * — none while its attached run is not shown (`shown`) — then Deny.
 */
export function optionsOf(
  decision: Decision,
  asks: readonly AgentAskVm[],
  shown: boolean,
): DecisionOption[] {
  const card = decision.card;
  if (card === null) {
    const ask = asks.find((candidate) => candidate.id === decision.id);
    return (ask?.choices ?? []).map((choice) => ({
      kind: "answer",
      label: choice,
      detail: choice === ask?.default ? DECISION_DEFAULT_MARK : null,
      text: choice,
    }));
  }
  const approve: DecisionOption[] = approvalUnseen(card, shown)
    ? []
    : card.scopes.map((offer) => ({
        kind: "approval",
        label: offer.label,
        detail: offer.detail,
        decision: "approve",
        scope: offer.scope,
      }));
  return [
    ...approve,
    { kind: "approval", label: DECISION_DENY, detail: null, decision: "deny", scope: "once" },
  ];
}

/**
 * Whether a number key belongs to the panel: never while the focus is in a
 * field that holds text (the key is that text), never while an input method is
 * composing (the number picks a candidate), nor with a modifier held.
 */
export function numberKeyIsThePanels(
  target: EventTarget | null,
  key: { metaKey: boolean; ctrlKey: boolean; altKey: boolean; isComposing: boolean },
): boolean {
  if (key.metaKey || key.ctrlKey || key.altKey || key.isComposing) {
    return false;
  }
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return target.value === "";
  }
  if (target instanceof HTMLElement && target.isContentEditable) {
    return (target.textContent ?? "") === "";
  }
  return true;
}

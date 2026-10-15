/**
 * The decisions panel above an agent room's composer (Story 101.3, R418, R430,
 * R435; the prior art is t3code's pending-input and approval panels): the first
 * decision waiting for this person — an approval this device may decide, or a
 * question answered in this room — with its options numbered.
 *
 * An approval is never decided from a summary: the panel draws the card's own
 * action — its tier, sentence, declassification, exact run, KVM inputs or
 * arguments, its irreversible warning — with the card's component
 * (`ApprovalAction`) above its options, and an attached run offers its approve
 * options only once its full action is shown here (R430 01).
 *
 * Keyboard: `1`–`9` pick an option whenever the focus is not in a field that
 * holds text (an empty composer included) and no input method is composing, and
 * move the focus to the options; `↑`/`↓` move, `Enter` sends the highlighted
 * one, `Escape` hides the panel to one line. Picking never sends by itself: an
 * approval is two keys, never one. The timeline card keeps its own buttons —
 * this is a second way to the same IPC (`agentApprovalDecide`, or the person's
 * own message for an answer).
 */
import { type KeyboardEvent, type RefObject, useEffect, useId, useRef, useState } from "react";
import { ApprovalAction, ApprovalOnly, approvalUnseen } from "@/components/agents/approval-card";
import {
  DECISION_ANSWERED,
  DECISION_HELD,
  DECISION_SHOW_RUN_FIRST,
  type Decision,
  type DecisionOption,
  decisionsOf,
  numberKeyIsThePanels,
  optionsOf,
  settlingOf,
} from "@/components/agents/decisions";
import { Button } from "@/components/ui/button";
import { logFailure } from "@/lib/failure";
import { type AgentAsksVm, type ApprovalVm, agentApprovalDecide, sendText } from "@/lib/ipc/client";
import { useConversationStores } from "@/lib/stores/conversation-stores";
import { cn } from "@/lib/utils";

/** The panel's accessible name. */
export const DECISIONS_LABEL = "Waiting for you";
/** The keys, said once under the options. */
export const DECISIONS_KEYS_HINT = "Press a number to pick, Enter to send, Esc to hide.";
/** What the panel says when a decision or an answer could not be sent (R381). */
export const DECISION_SEND_FAILED = "keeper couldn't send that. Nothing was decided; try again.";
/** The verbs of the hide/show toggle. */
export const DECISIONS_HIDE = "Hide";
export const DECISIONS_SHOW = "Show";

export function DecisionsPanel({
  accountId,
  roomId,
  approvals,
  asks,
  scope,
}: {
  accountId: string;
  roomId: string;
  approvals: readonly ApprovalVm[];
  /** The questions answered here, and what keeper could not read of them. */
  asks: AgentAsksVm;
  /** The conversation the panel belongs to: number keys from elsewhere are not its. */
  scope: RefObject<HTMLElement | null>;
}) {
  const [done, setDone] = useState<ReadonlySet<string>>(new Set());
  const [shown, setShown] = useState<ReadonlySet<string>>(new Set());
  const decisions = decisionsOf(approvals, asks.asks, done);
  const settling = settlingOf(asks.asks);
  const current: Decision | undefined = decisions[0];
  // An attached action counts as shown for the request and digest it was fetched
  // for, never for the next approval the panel advances to (R447).
  const shownKey = (id: string, digest: string | null) => `${id} ${digest ?? ""}`;
  const options =
    current === undefined
      ? []
      : optionsOf(current, asks.asks, shown.has(shownKey(current.id, current.bindingDigest)));
  const [collapsed, setCollapsed] = useState(false);
  const [highlighted, setHighlight] = useState(0);
  const [sending, setSending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const ids = useId();
  const { composer } = useConversationStores();
  const currentId = current?.id;
  const count = options.length;
  const highlight = Math.min(highlighted, Math.max(count - 1, 0));

  // A new decision starts at its first option, with no stale failure.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the decision's id triggers the reset
  useEffect(() => {
    setHighlight(0);
    setFailure(null);
  }, [currentId]);

  useEffect(() => {
    if (currentId === undefined || collapsed || count === 0) {
      return;
    }
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || !/^[1-9]$/.test(event.key)) {
        return;
      }
      const target = event.target;
      const here =
        target === document.body || (target instanceof Node && scope.current?.contains(target));
      const index = Number(event.key) - 1;
      if (!here || index >= count || !numberKeyIsThePanels(target, event)) {
        return;
      }
      event.preventDefault();
      setHighlight(index);
      listRef.current?.focus();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [currentId, collapsed, count, scope]);

  if (current === undefined && settling.length === 0 && asks.note === null) {
    return null;
  }

  const confirm = async (decision: Decision, option: DecisionOption) => {
    if (sending) {
      return;
    }
    setSending(true);
    setFailure(null);
    try {
      if (option.kind === "approval") {
        await agentApprovalDecide(accountId, roomId, {
          id: decision.id,
          bindingDigest: decision.bindingDigest ?? "",
          decision: option.decision,
          scope: option.scope,
          note: null,
        });
        setDone((was) => new Set(was).add(decision.id));
      } else {
        // The question stays until Rust says where its answer stands: held for
        // Undo-Send, sent, or relayed (R430 05).
        await sendText(accountId, roomId, option.text);
      }
      if (decisions.length === 1) {
        composer.getState().requestFocus();
      }
    } catch (raw) {
      logFailure(option.kind === "approval" ? "agents.panelDecide" : "agents.panelAnswer")(raw);
      setFailure(DECISION_SEND_FAILED);
    } finally {
      setSending(false);
    }
  };

  const optionId = (index: number) => `${ids}-option-${index}`;
  const onListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (current === undefined) {
      return;
    }
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        setHighlight((at) => (Math.min(at, count - 1) + step + count) % count);
        return;
      }
      case "Enter": {
        event.preventDefault();
        const option = options[highlight];
        if (option !== undefined) {
          void confirm(current, option);
        }
        return;
      }
      case "Escape":
        // Hiding is this key's whole meaning here; the app's Esc chain is not
        // also asked to close something else.
        event.preventDefault();
        event.stopPropagation();
        setCollapsed(true);
        composer.getState().requestFocus();
        return;
    }
  };

  const card = current?.card ?? null;
  const unseen =
    card !== null && approvalUnseen(card, shown.has(shownKey(card.id, card.bindingDigest)));
  const from = current?.kind === "approval" ? "Approval" : "Question";
  const counter = decisions.length > 1 ? ` · 1 of ${decisions.length}` : "";
  return (
    <section
      aria-label={DECISIONS_LABEL}
      className="mb-2 flex min-w-0 flex-col gap-2 rounded-md border border-border bg-card p-3 text-card-foreground text-sm [overflow-wrap:anywhere]"
    >
      {current !== undefined && (
        <div className="flex items-start justify-between gap-2">
          <p className="min-w-0 text-muted-foreground text-xs">
            {from} from {current.askedBy}
            {counter}
          </p>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            aria-expanded={!collapsed}
            aria-controls={`${ids}-body`}
            onClick={() => setCollapsed((was) => !was)}
          >
            {collapsed ? DECISIONS_SHOW : DECISIONS_HIDE}
          </Button>
        </div>
      )}
      {current !== undefined && collapsed && (
        <p className="truncate font-medium">{current.question}</p>
      )}
      {current !== undefined && !collapsed && (
        <div id={`${ids}-body`} className="flex min-w-0 flex-col gap-2">
          {card !== null ? (
            <div
              id={`${ids}-question`}
              className="flex max-h-[45vh] min-w-0 flex-col gap-2 overflow-y-auto"
            >
              <ApprovalAction
                card={card}
                accountId={accountId}
                roomId={roomId}
                onShown={() =>
                  setShown((was) => new Set(was).add(shownKey(card.id, card.bindingDigest)))
                }
              />
              <ApprovalOnly card={card} />
            </div>
          ) : (
            <p id={`${ids}-question`} className="font-medium">
              {current.question}
            </p>
          )}
          {options.length > 0 && (
            <div
              ref={listRef}
              role="listbox"
              tabIndex={0}
              aria-labelledby={`${ids}-question`}
              aria-activedescendant={optionId(highlight)}
              aria-busy={sending}
              onKeyDown={onListKeyDown}
              className="flex flex-col gap-1 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              {options.map((option, index) => (
                // The listbox holds the focus and the keys (aria-activedescendant);
                // a pointer picks and sends, like a card's button.
                // biome-ignore lint/a11y/useKeyWithClickEvents: keys are the listbox's
                <div
                  key={`${option.kind}:${option.label}`}
                  id={optionId(index)}
                  role="option"
                  // Focus stays on the listbox; an option is reached by its keys.
                  tabIndex={-1}
                  aria-selected={index === highlight}
                  onPointerMove={() => setHighlight(index)}
                  onClick={() => {
                    setHighlight(index);
                    void confirm(current, option);
                  }}
                  className={cn(
                    "flex min-h-9 cursor-pointer items-start gap-2 rounded-md border border-border px-2 py-1.5",
                    index === highlight && "border-primary bg-accent text-accent-foreground",
                  )}
                >
                  <kbd className="mt-px inline-flex size-5 shrink-0 items-center justify-center rounded border border-border font-mono text-xs">
                    {index + 1}
                  </kbd>
                  {/* The label and its explanation stack, so a long explanation never
                      squeezes the label at 420 px (R430 06). */}
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span>{option.label}</span>
                    {option.detail !== null && (
                      <span className="text-muted-foreground text-xs">{option.detail}</span>
                    )}
                  </span>
                </div>
              ))}
            </div>
          )}
          {unseen && <p className="text-muted-foreground text-xs">{DECISION_SHOW_RUN_FIRST}</p>}
          {current.note !== null && <p className="text-muted-foreground text-xs">{current.note}</p>}
          {options.length > 0 && (
            <p className="text-muted-foreground text-xs">{DECISIONS_KEYS_HINT}</p>
          )}
          {failure !== null && (
            <p role="alert" className="text-destructive text-xs">
              {failure}
            </p>
          )}
        </div>
      )}
      {settling.map((ask) => (
        <p key={ask.id} role="status" className="text-muted-foreground text-xs">
          <span className="font-medium text-foreground">{ask.question}</span>{" "}
          {ask.state === "held" ? DECISION_HELD : DECISION_ANSWERED}
        </p>
      ))}
      {asks.note !== null && (
        <p role="status" className="text-muted-foreground text-xs">
          {asks.note}
        </p>
      )}
    </section>
  );
}

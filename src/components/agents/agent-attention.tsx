/**
 * A session's state in the Agents list (story 101.6, R421): who needs you, who
 * failed, who is working, who finished since you last read them.
 *
 * Rust decides the state (`AgentSessionRowVm.attention`, keeper-core
 * `hq::sessions`) and this draws it, never deriving it: t3code's new
 * sidebar shape — an icon and a word where the row's time would be — in
 * Paseo's colours (needs you amber, failed red, working blue, done green).
 * *Unknown* is Rust saying it could not read all it needs to say nothing
 * waits for you (R381, R453): its sentence is the mark's tooltip and the
 * row's accessible name.
 * The word carries the meaning; the icon and its colour are the second and
 * third channels, so the mark reads in greyscale and to a dichromat.
 *
 * *Working* turns slowly; a person who asked for reduced motion gets the same
 * dashed ring standing still (`motion-reduce`).
 *
 * The mark is `aria-hidden`: it sits inside controls whose accessible name is
 * computed (a row button with its own `aria-label`, a select item), so the
 * call site puts {@link AGENT_ATTENTION_LABEL} into that name instead.
 */
import {
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CircleHelp,
  MessageCircleQuestion,
} from "lucide-react";
import type { AgentAttentionVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** The word each state shows. */
export const AGENT_ATTENTION_WORD: Record<AgentAttentionVm, string> = {
  needsYou: "Needs you",
  unknown: "Unknown",
  failed: "Failed",
  working: "Working",
  doneUnread: "Done",
};

/** What a screen reader hears of each state, appended to the control's name. */
export const AGENT_ATTENTION_LABEL: Record<AgentAttentionVm, string> = {
  needsYou: "needs you",
  unknown: "keeper could not read whether it needs you",
  failed: "failed",
  working: "working",
  doneUnread: "done, unread",
};

const ICON = {
  needsYou: MessageCircleQuestion,
  unknown: CircleHelp,
  failed: CircleAlert,
  working: CircleDashed,
  doneUnread: CircleCheck,
} as const;

const TONE: Record<AgentAttentionVm, string> = {
  needsYou: "text-held",
  unknown: "text-muted-foreground",
  failed: "text-bridge-disconnected",
  working: "text-bot-ink-lapis",
  doneUnread: "text-bridge-healthy",
};

/** What *Unknown* says where the person looks (R381). */
export const AGENT_ATTENTION_UNKNOWN =
  "keeper could not read all of this agent's rooms, so whether it needs you is unknown.";

/** The `, needs you` an agent row's accessible name ends with; empty for none. */
export function agentAttentionLabel(state: AgentAttentionVm | null | undefined): string {
  return state == null ? "" : `, ${AGENT_ATTENTION_LABEL[state]}`;
}

export function AgentAttentionMark({
  state,
  className,
}: {
  state: AgentAttentionVm;
  className?: string;
}) {
  const Icon = ICON[state];
  return (
    <span
      aria-hidden="true"
      data-testid="agent-attention"
      data-state={state}
      title={state === "unknown" ? AGENT_ATTENTION_UNKNOWN : undefined}
      className={cn(
        "inline-flex shrink-0 items-center gap-1 font-medium text-xs",
        TONE[state],
        className,
      )}
    >
      <Icon
        className={cn(
          "size-3.5 shrink-0",
          state === "working" &&
            "motion-safe:animate-[spin_3s_linear_infinite] motion-reduce:animate-none",
        )}
      />
      {AGENT_ATTENTION_WORD[state]}
    </span>
  );
}

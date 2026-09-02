/**
 * One answer's metadata — the compact caption and its expander (Epic 61, Story
 * 61.8, FR-384).
 *
 * # What is recorded, and what is shown
 *
 * Every number here was written by Story 61.4's stream driver onto the
 * `bot_messages` row: model, provider, the three token counts, the measured
 * time to the first token, the total time, the finish reason, the provider's
 * request id and the tool-call count. **Recording never depends on this
 * toggle.** Switching the caption on tomorrow still explains an answer that
 * arrived today, which is the whole reason the columns are not conditional.
 *
 * What the toggle owns is the *showing*. It defaults off because a conversation
 * is read for what the model said, and a row that leads with a request id is a
 * debugger rather than a reply. It is persisted in Rust
 * (`bots.message_details`, `Shape::Flag01`, default `"0"`) rather than in
 * component state or a cookie: it is a preference about the person, the same
 * answer on the next launch, and a `keeper.toml` may set it like any other key.
 *
 * # Absent is absent
 *
 * `BotMessageVm` makes every number nullable and zero-fills none of them. An
 * endpoint that omits `usage` is a fact about the endpoint, so an unreported
 * token count renders as **nothing at all** — not `0`, not `—`, not "unknown".
 * A `0` here would be a measurement keeper never took, and this is the app
 * whose sync rate stands empty rather than reading `0 B/s` on an idle wire
 * (AD-34-13). Every field is therefore dropped from both the caption and the
 * expander when it is `null`, and {@link metaFacts} is the one place that
 * decision is made so the two surfaces cannot disagree.
 *
 * `toolCallCount` is the exception that proves it: it is a non-nullable
 * `number`, so `0` there is a count keeper took and means "this turn called
 * nothing". It is shown as `0` and that is not a contradiction.
 *
 * # Formatting
 *
 * Nothing new is invented. Token counts go through {@link countLabel} with the
 * {@link TOKENS} noun — the same grouped-number wording the note list and the
 * Files tree use. The total time goes through {@link formatElapsed}, the
 * duration formatter the recordings row already renders `durationMs` with.
 *
 * The time to the first token does **not**, and that is deliberate: it is a
 * millisecond quantity, and `formatElapsed` resolves to whole seconds, so a
 * measured 340 ms would print `0:00` — the "zero that was really a
 * measurement" this file exists to refuse. It is printed in the unit it was
 * measured in, grouped with `toLocaleString` the way `countLabel` groups, and
 * labelled in words: "time to first token", never "TTFT".
 *
 * # Accessibility
 *
 * The caption is not hover-revealed. Open WebUI's usage affordance is
 * `hover-reveal` *and* `aria-hidden="true"`, so the one datum only a pointer
 * can reach is also the one removed from the accessibility tree; this caption
 * is in the flow, readable, and reached by Tab at its disclosure button.
 *
 * The expander is a real disclosure: a `<button>` carrying `aria-expanded` and
 * `aria-controls`, and a panel with the matching id.
 *
 * **Nothing here is a live region.** A row streams token by token, and an
 * `aria-live` caption over a growing answer would read the whole metadata line
 * out again on every delta. The arriving-answer announcement belongs to the one
 * `role="status"` line `bot-message.tsx` already owns; MDN's guidance is that a
 * transcript uses `role="log"` with `aria-atomic="false"`, and a per-message
 * caption is not a status message at all under WCAG 2.2 SC 4.1.3.
 */
import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { formatElapsed } from "@/hooks/use-recording-session";
import { countLabel, TOKENS } from "@/lib/count-label";
import type { BotMessageVm } from "@/lib/ipc/client";
import { botsMessageDetailsGet, botsMessageDetailsSet } from "@/lib/ipc/client";
import { botsStore, useBotsStore } from "@/lib/stores/bots";
import { cn } from "@/lib/utils";

/** The pane's toggle, worded as the thing it shows rather than as its state. */
export const BOT_META_TOGGLE_LABEL = "Answer details";

/** What the expander's trigger says while it is shut. */
export const BOT_META_MORE_LABEL = "More about this answer";

/** And while it is open. Distinct wording, so the two states cannot read alike. */
export const BOT_META_LESS_LABEL = "Less about this answer";

/**
 * What an assistant row says when the endpoint reported nothing measurable.
 *
 * Not silence: with the toggle on, a caption that simply is not there is
 * indistinguishable from a caption that failed to render. This names the
 * absence and where it came from.
 */
export const BOT_META_NOTHING_REPORTED = "This endpoint reported nothing about this answer.";

/** How the caption states a row the stream never closed. */
export const BOT_META_PARTIAL = "answer incomplete";

/** The accessible name of the expanded panel. */
export const BOT_META_PANEL_LABEL = "Answer details";

/** One labelled fact about an answer. */
export interface BotMetaFact {
  /** The label, in words a person reads — never an acronym. */
  label: string;
  /** The value, already formatted. */
  value: string;
}

/**
 * Every fact this row actually carries, in reading order.
 *
 * A `null` column produces no entry — the absence rule, made once here so the
 * caption and the expander cannot disagree about what was measured.
 */
export function metaFacts(message: BotMessageVm): BotMetaFact[] {
  const facts: BotMetaFact[] = [];
  if (message.model !== null) {
    facts.push({ label: "Model", value: message.model });
  }
  if (message.providerId !== null) {
    facts.push({ label: "Provider", value: message.providerId });
  }
  if (message.promptTokens !== null) {
    facts.push({ label: "Prompt tokens", value: countLabel(message.promptTokens, TOKENS) });
  }
  if (message.completionTokens !== null) {
    facts.push({ label: "Completion tokens", value: countLabel(message.completionTokens, TOKENS) });
  }
  if (message.totalTokens !== null) {
    facts.push({ label: "Total tokens", value: countLabel(message.totalTokens, TOKENS) });
  }
  if (message.ttftMs !== null) {
    // Milliseconds, in the unit it was measured in: see the module note on why
    // `formatElapsed` is wrong for this one number.
    facts.push({
      label: "Time to first token",
      value: `${message.ttftMs.toLocaleString()} ms`,
    });
  }
  if (message.durationMs !== null) {
    facts.push({ label: "Total time", value: formatElapsed(message.durationMs) });
  }
  if (message.finishReason !== null) {
    facts.push({ label: "Why it stopped", value: message.finishReason });
  }
  // A count keeper took: `0` means this turn called nothing, which is a fact
  // rather than a gap. Contrast every nullable field above.
  facts.push({ label: "Tool calls", value: message.toolCallCount.toLocaleString() });
  if (message.requestId !== null) {
    facts.push({ label: "Request id", value: message.requestId });
  }
  return facts;
}

/**
 * The compact line: the four facts worth reading at a glance, in that order,
 * each dropped when the endpoint did not report it.
 *
 * Shorter than {@link metaFacts} on purpose — the provider id, the split token
 * counts and the request id are things you go looking for, not things you read
 * past on every row.
 */
export function metaCaption(message: BotMessageVm): string {
  const parts: string[] = [];
  if (message.model !== null) {
    parts.push(message.model);
  }
  if (message.totalTokens !== null) {
    parts.push(countLabel(message.totalTokens, TOKENS));
  }
  if (message.ttftMs !== null) {
    parts.push(`first token after ${message.ttftMs.toLocaleString()} ms`);
  }
  if (message.durationMs !== null) {
    parts.push(`answered in ${formatElapsed(message.durationMs)}`);
  }
  if (message.partial) {
    parts.push(BOT_META_PARTIAL);
  }
  return parts.join(" · ");
}

export function BotMessageMeta({ message }: { message: BotMessageVm }) {
  const shown = useBotsStore((state) => state.metaShown);
  const [open, setOpen] = useState(false);
  const panelId = useId();

  // A question carries none of this. The model, the tokens and the finish
  // reason are facts about an answer, and a caption under what the person typed
  // would be a row of absences.
  if (!shown || message.role !== "assistant") {
    return null;
  }

  const facts = metaFacts(message);
  const caption = metaCaption(message);
  // `Tool calls` is always present, so "nothing reported" means nothing BUT
  // that — the endpoint answered and told keeper no more than the text.
  const nothingReported = caption === "";

  return (
    <div className="flex min-w-0 flex-col gap-1 text-muted-foreground text-xs">
      <div className="flex min-w-0 items-center gap-2">
        <p className="min-w-0 break-words">
          {nothingReported ? BOT_META_NOTHING_REPORTED : caption}
        </p>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={() => setOpen(!open)}
          className={cn(
            "flex shrink-0 items-center gap-1 rounded-sm px-1 py-0.5",
            "hover:bg-accent hover:text-accent-foreground",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          )}
        >
          {open ? (
            <ChevronDown aria-hidden="true" className="size-3" />
          ) : (
            <ChevronRight aria-hidden="true" className="size-3" />
          )}
          {open ? BOT_META_LESS_LABEL : BOT_META_MORE_LABEL}
        </button>
      </div>
      {open && (
        <dl
          id={panelId}
          aria-label={BOT_META_PANEL_LABEL}
          className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5"
        >
          {facts.map((fact) => (
            <div key={fact.label} className="contents">
              <dt className="text-muted-foreground">{fact.label}</dt>
              {/* `figures` for the tabular numerals the timestamp caption uses:
                  a token count and a duration are read against each other down
                  a column, and proportional digits make that harder. */}
              <dd className="figures min-w-0 break-words">{fact.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}

/**
 * The pane's switch for the caption above (Story 61.8, FR-384).
 *
 * An `aria-pressed` chip rather than a control that appears and disappears —
 * the notes filter bar's idiom, and for its reason: one persistent control
 * whose own paint says which way it sits.
 *
 * It hydrates itself on mount rather than being fed by the pane, because it is
 * the only thing that needs the value and the pane is unmounted by every
 * surface switch. A read that fails leaves the toggle off, which is the shipped
 * default and the honest floor: claiming the caption is on when keeper could
 * not read the setting would be the claim that lies.
 */
export function BotMetaToggle() {
  const shown = useBotsStore((state) => state.metaShown);

  useEffect(() => {
    let live = true;
    void botsMessageDetailsGet()
      .then((value) => {
        if (live) {
          botsStore.getState().setMetaShown(value);
        }
      })
      .catch(() => {
        // Off is already the state; nothing to correct and nothing to say.
      });
    return () => {
      live = false;
    };
  }, []);

  return (
    <button
      type="button"
      aria-pressed={shown}
      onClick={() => void toggleBotMessageDetails()}
      className={cn(
        "shrink-0 rounded-md border border-border px-2 py-1 text-xs",
        "hover:bg-accent hover:text-accent-foreground",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        shown && "bg-accent text-accent-foreground",
      )}
    >
      {BOT_META_TOGGLE_LABEL}
    </button>
  );
}

/**
 * Flip the persisted setting and the mirror, in that order.
 *
 * Exported because the command palette's entry is the other caller: one verb,
 * so a toggle reached from ⌘K and a toggle reached from the pane cannot become
 * two preferences that look like one (UX-DR42's one-verb rule). The store is
 * updated only after Rust accepted the write, so a refused write leaves the
 * chip showing what is actually stored.
 */
export async function toggleBotMessageDetails(): Promise<void> {
  // Read the stored value rather than the mirror: the palette can be opened
  // with no Bots pane mounted, so there may be nothing that has hydrated it.
  const current = await botsMessageDetailsGet();
  await botsMessageDetailsSet(!current);
  botsStore.getState().setMetaShown(!current);
}

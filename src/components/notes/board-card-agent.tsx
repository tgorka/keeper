/**
 * Who works a card and where (AD-386, UX-DR134): the agent block a session
 * board draws under a card's title when the card carries agent keys.
 *
 * **A run is a badge, never a column.** The card sits where its `status:` puts
 * it; `run:` is drawn here, beside the title, so a card an agent is working is
 * an ordinary card in an ordinary column with one more word on it. A `run:`
 * keeper cannot read is shown as unreadable with the file's own value — never
 * guessed into one of the six, and never dropped.
 *
 * **Compact by default, the rest one press away.** Four columns at 1280 px are
 * ~300 px each and one column at 420 px, so the card carries only what a person
 * scans for — the badge, the marks that ask for attention, one line naming the
 * agent, where it runs (or why it waits) and who asked, the schedule, and the
 * *Allow* a schedule an agent wrote is waiting for. Every key the file holds is
 * in the details list, under the names the file spells them with, because the
 * file is what a person edits to change them.
 *
 * Renders view-model values only: who allows a schedule, whether a schedule
 * parses and where a card runs are Rust's answers, not this file's.
 */
import type { LucideIcon } from "lucide-react";
import {
  Ban,
  CalendarClock,
  ChevronRight,
  CircleX,
  Clock,
  Eye,
  Hourglass,
  Play,
  ShieldAlert,
  TriangleAlert,
} from "lucide-react";
import { Fragment, type ReactNode, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { CardAgentVm, CardKeyVm } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/**
 * The six run words and how each is drawn. Colour is spent on the two a person
 * acts on — `running` (primary: work is happening now) and `failed`
 * (destructive) — and every badge carries its word and an icon, so no state is
 * told by colour alone.
 */
const RUN_BADGES: Readonly<
  Record<string, { icon: LucideIcon; variant: "default" | "secondary" | "outline" | "destructive" }>
> = {
  queued: { icon: Clock, variant: "secondary" },
  running: { icon: Play, variant: "default" },
  waiting: { icon: Hourglass, variant: "secondary" },
  blocked: { icon: Ban, variant: "outline" },
  review: { icon: Eye, variant: "outline" },
  failed: { icon: CircleX, variant: "destructive" },
};

/** What the badge says for a `run:` keeper cannot read. */
export const BOARD_RUN_UNREADABLE = "run unreadable";

/** Why an unreadable `run:` is shown as it is. */
export function boardRunUnreadableHint(value: string): string {
  return `The file says run: ${value}, which is not one of keeper's six run words. It is shown as written, never guessed — the card stays in the column its status: names.`;
}

/** The mark of a card made from outside content (Q17). */
export const BOARD_UNTRUSTED = "outside content";
export const BOARD_UNTRUSTED_HINT =
  "This card was made from outside content, so an agent that reads it is treated as reading untrusted text.";

/** The mark of keys the file holds that keeper cannot read. */
export function boardUnreadableKeys(keys: readonly string[]): string {
  return `Unreadable in the file: ${keys.join(", ")}. Shown as written; fix the keys in the file.`;
}

/** The mark of a schedule (or workflow) an agent wrote (Q16). */
export function boardScheduledBy(who: string): string {
  return `Scheduled by ${who} — not running until you allow it.`;
}

/** The action that answers it, and its accessible name. */
export const BOARD_ALLOW_LABEL = "Allow";
export function boardAllowName(title: string): string {
  return `Allow the schedule of ${title}`;
}

/** Where *Allow* is not offered (the phone: the command is desktop-only). */
export const BOARD_ALLOW_ELSEWHERE = "Allow it from keeper on your Mac.";

/** What is said when *Allow* is refused and keeper sent no sentence. */
export const BOARD_ALLOW_FAILED =
  "keeper could not allow that schedule. The card may have changed on disk since the board was drawn — reopen it.";

/** What a schedule that does not parse says, beside its value. */
export const BOARD_SCHEDULE_UNREADABLE = "schedule unreadable";

/** The details toggle. */
export const BOARD_AGENT_DETAILS = "Details";
export function boardAgentDetailsName(title: string): string {
  return `Agent details — ${title}`;
}

/** The suffix an unreadable value carries wherever it is drawn. */
const UNREADABLE = "(unreadable)";

/**
 * The file's keys, in the order a card states them, with the names the file
 * spells them with. `run` is the badge; the rest are the details list.
 */
const KEYS: ReadonlyArray<{ key: keyof CardAgentVm; name: string }> = [
  { key: "run", name: "run" },
  { key: "assignee", name: "assignee" },
  { key: "host", name: "host (pin)" },
  { key: "requestedBy", name: "requested_by" },
  { key: "schedule", name: "schedule" },
  { key: "lastRun", name: "last_run" },
  { key: "workflow", name: "workflow" },
  { key: "scheduledBy", name: "scheduled_by" },
  { key: "integrity", name: "integrity" },
];

/** The file spelling of a key, for the unreadable mark. */
const FILE_NAME: Partial<Record<keyof CardAgentVm, string>> = {
  host: "host",
};

function keyOf(agent: CardAgentVm, key: keyof CardAgentVm): CardKeyVm | null {
  const value = agent[key];
  return typeof value === "object" ? value : null;
}

/** A value as the card holds it, marked when it does not read. */
function Value({ value }: { value: CardKeyVm }) {
  return value.readable ? (
    value.value
  ) : (
    <>
      {value.value} <span className="text-destructive">{UNREADABLE}</span>
    </>
  );
}

function RunBadge({ run }: { run: CardKeyVm }) {
  const known = run.readable ? RUN_BADGES[run.value] : undefined;
  if (known === undefined) {
    // A readable word outside the six cannot arrive from Rust today; drawing it
    // as unreadable rather than as a seventh badge keeps "never guessed" true
    // if the grammar ever grows on one side first.
    return (
      // The value is whatever the file holds, of any length, and the shared
      // badge is a one-line pill (`h-5 whitespace-nowrap shrink-0`): here it
      // wraps inside the card instead, at any character, at the narrowest
      // column the board draws.
      <Badge
        variant="destructive"
        title={boardRunUnreadableHint(run.value)}
        className="h-auto min-h-5 max-w-full shrink justify-start whitespace-normal rounded-md text-left [overflow-wrap:anywhere]"
      >
        <TriangleAlert aria-hidden="true" className="shrink-0 self-start mt-0.5" />
        <span className="min-w-0">
          {BOARD_RUN_UNREADABLE}: {run.value}
        </span>
      </Badge>
    );
  }
  const Icon = known.icon;
  return (
    <Badge variant={known.variant}>
      <Icon aria-hidden="true" />
      <span className="sr-only">run: </span>
      {run.value}
    </Badge>
  );
}

export function BoardCardAgent({
  agent,
  title,
  onAllow,
}: {
  agent: CardAgentVm;
  /** The card's title, for the accessible names of its controls. */
  title: string;
  /** Allow the card's agent-written schedule, and re-read. Rejecting is how a
   *  refusal is reported. Absent where the command is unsupported. */
  onAllow?: () => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [allowing, setAllowing] = useState(false);
  /**
   * The agent block an *Allow* succeeded against. The write is done but the
   * board still draws the mark until the session's re-read arrives, and the
   * re-read is the parent's (`onChanged` only asks for it) — so the button
   * stays pending while this card still shows the very VM it allowed. Any new
   * read brings a new object: a mark still on it then is a mark keeper read
   * afresh, and *Allow* is offered again.
   */
  const [allowedFor, setAllowedFor] = useState<CardAgentVm | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const pending = allowing || allowedFor === agent;

  const allow = async () => {
    if (onAllow === undefined || pending) {
      return;
    }
    setRefusal(null);
    setAllowing(true);
    try {
      await onAllow();
      setAllowedFor(agent);
    } catch (error) {
      // keeper's own sentence — it knows whom to sign in as, and the board
      // does not (UX-DR43).
      setRefusal(syncErrorMessage(error, BOARD_ALLOW_FAILED));
    } finally {
      setAllowing(false);
    }
  };

  const unreadable = KEYS.filter(
    ({ key }) => key !== "run" && key !== "schedule" && keyOf(agent, key)?.readable === false,
  ).map(({ key, name }) => FILE_NAME[key] ?? name);
  const untrusted = agent.integrity?.readable === true;
  const where =
    agent.runningOn !== null
      ? `running on ${agent.runningOn}`
      : agent.waiting !== null
        ? `waiting: ${agent.waiting}`
        : null;
  const line: { key: string; node: ReactNode }[] = [];
  if (agent.assignee !== null) {
    line.push({
      key: "assignee",
      node: (
        <span className="text-foreground">
          <Value value={agent.assignee} />
        </span>
      ),
    });
  }
  if (where !== null) {
    line.push({ key: "where", node: where });
  }
  if (agent.requestedBy !== null) {
    line.push({
      key: "requested",
      node: (
        <>
          requested by <Value value={agent.requestedBy} />
        </>
      ),
    });
  }

  return (
    // A Matrix id has no break opportunity, and a column can be 186 px wide.
    <div className="flex min-w-0 flex-col gap-1 text-xs [overflow-wrap:anywhere]">
      {(agent.run !== null || untrusted || unreadable.length > 0) && (
        <div className="flex flex-wrap items-center gap-1">
          {agent.run !== null && <RunBadge run={agent.run} />}
          {untrusted && (
            <span
              title={BOARD_UNTRUSTED_HINT}
              className="inline-flex items-center gap-0.5 text-muted-foreground"
            >
              <ShieldAlert aria-hidden="true" className="size-3" />
              {BOARD_UNTRUSTED}
            </span>
          )}
          {unreadable.length > 0 && (
            <span title={boardUnreadableKeys(unreadable)} className="text-destructive">
              <TriangleAlert aria-label={boardUnreadableKeys(unreadable)} className="size-3" />
            </span>
          )}
        </div>
      )}
      {line.length > 0 && (
        <p className="text-muted-foreground">
          {line.map(({ key, node }) => (
            <Fragment key={key}>
              {/* Read aloud too: without it a screen reader runs the parts
                  together ("tola-greyrunning on electra"). */}
              {key !== line[0]?.key && " · "}
              {node}
            </Fragment>
          ))}
        </p>
      )}
      {agent.schedule !== null && (
        <p
          className={cn(
            "inline-flex items-center gap-1",
            agent.schedule.readable ? "text-muted-foreground" : "text-destructive",
          )}
        >
          <CalendarClock aria-hidden="true" className="size-3 shrink-0" />
          {agent.schedule.readable ? (
            <span>
              <span className="sr-only">schedule: </span>
              {agent.schedule.value}
            </span>
          ) : (
            <span>
              {BOARD_SCHEDULE_UNREADABLE}: {agent.schedule.value}
            </span>
          )}
        </p>
      )}
      {agent.scheduledBy !== null && (
        <div className="flex flex-wrap items-center gap-1.5 rounded-md border border-border border-dashed px-1.5 py-1">
          <span>{boardScheduledBy(agent.scheduledBy.value)}</span>
          {onAllow !== undefined ? (
            <Button
              type="button"
              size="xs"
              variant="outline"
              aria-label={boardAllowName(title)}
              disabled={pending}
              onClick={() => void allow()}
            >
              {BOARD_ALLOW_LABEL}
            </Button>
          ) : (
            <span className="text-muted-foreground">{BOARD_ALLOW_ELSEWHERE}</span>
          )}
          {refusal !== null && (
            <p role="status" className="w-full text-destructive">
              {refusal}
            </p>
          )}
        </div>
      )}
      <button
        type="button"
        aria-expanded={open}
        aria-label={boardAgentDetailsName(title)}
        onClick={() => setOpen((was) => !was)}
        className="inline-flex w-fit items-center gap-0.5 rounded-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronRight
          aria-hidden="true"
          className={cn("size-3 transition-transform", open && "rotate-90")}
        />
        {BOARD_AGENT_DETAILS}
      </button>
      {open && (
        <dl
          aria-label={boardAgentDetailsName(title)}
          className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-0.5"
        >
          {KEYS.map(({ key, name }) => {
            const value = keyOf(agent, key);
            return (
              value !== null && (
                <div key={key} className="contents">
                  <dt className="text-muted-foreground">{name}</dt>
                  <dd className="break-words">
                    <Value value={value} />
                  </dd>
                </div>
              )
            );
          })}
          {agent.runningOn !== null && (
            <div className="contents">
              <dt className="text-muted-foreground">running on</dt>
              <dd className="break-words">{agent.runningOn}</dd>
            </div>
          )}
          {agent.waiting !== null && (
            <div className="contents">
              <dt className="text-muted-foreground">waiting</dt>
              <dd className="break-words">{agent.waiting}</dd>
            </div>
          )}
        </dl>
      )}
    </div>
  );
}

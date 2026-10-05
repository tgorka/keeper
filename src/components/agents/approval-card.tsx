/**
 * An agent's approval request, drawn where it arrived in the room (AD-395,
 * UX-DR136): in the room view and in the notes dock alike, on every tier.
 *
 * **Read top to bottom the way a person decides.** The tier as a word with a
 * weight (an icon and the card's left edge: thin and quiet at T2, the held
 * colour at T3, the destructive colour at T4 — the word always says it, so no
 * tier is told by colour alone); keeper's own one-sentence summary (never the
 * model's words); the exact action, whole — a long one is a scroll box with
 * "Show all N lines", never cut, and one too large to travel inline says it is
 * attached and is fetched whole on request (Rust checks it against the digest);
 * who asked, through which agents; who can decide; then the state.
 *
 * **The decide buttons are Rust's answer.** They are drawn only when the card
 * says this device decides (`canDecide`); otherwise the card's own sentence
 * (`cannotDecide`) is shown, with the way into this device's verification when
 * `verify` says it is the remedy. What each approval grants is said beside it
 * before it is chosen. A T4 card's `only` sentence is shown to everyone. A
 * decision is sent once and the card changes only when the room echoes it
 * (`TimelineBatch.approvals`); until then it says the decision was sent. A
 * refusal is shown in keeper's own words, beside buttons that stay usable.
 *
 * A coalesced request (a gate's card) is one group of rows, each decided on its
 * own: a row's decision never touches another row.
 */
import type { LucideIcon } from "lucide-react";
import {
  CheckCheck,
  CircleCheck,
  CircleX,
  Clock,
  Hourglass,
  OctagonAlert,
  Paperclip,
  Shield,
  ShieldAlert,
} from "lucide-react";
import { useId, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  type ApprovalCardVm,
  type ApprovalDecision,
  type ApprovalPersonVm,
  type ApprovalScope,
  type ApprovalVm,
  agentApprovalDecide,
  agentApprovalPayload,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { verificationStore } from "@/lib/stores/verification";
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

/**
 * A payload longer than this many lines, or characters (one long string wraps
 * into many lines), is drawn as a scroll box with a toggle.
 */
const PAYLOAD_FOLD_LINES = 16;
const PAYLOAD_FOLD_CHARS = 1200;

export const APPROVAL_DENY = "Deny";
export const APPROVAL_SEND_DENY = "Send deny";
export const APPROVAL_CANCEL = "Cancel";
export const APPROVAL_NOTE_LABEL = "Note for the agent (optional)";
export const APPROVAL_VERIFY = "Verify this device";
export const APPROVAL_SENT = "Your decision was sent. The card changes when the room has it.";
export const APPROVAL_DECIDE_FAILED =
  "keeper could not send that decision. Nothing was decided; try again.";

/** A coalesced request's accessible name. */
export function approvalListName(count: number): string {
  return `Approval request: ${count} actions, each decided on its own`;
}

/** The declassification's question (UX-DR136's wording), drawn in place of the summary. */
export function declassifyQuestion(readers: ApprovalPersonVm[]): string {
  return `Let this one message reach ${readers.map((p) => p.name).join(", ")}?`;
}

/** keeper's summary marks names with backticks; draw those as code. */
function Summary({ text }: { text: string }) {
  return (
    <>
      {text.split("`").map((part, i) =>
        i % 2 === 1 ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: the parts of one fixed sentence, never reordered
          <code key={i} className="rounded-sm bg-muted px-1 font-mono text-[0.9em]">
            {part}
          </code>
        ) : (
          part
        ),
      )}
    </>
  );
}

/** The exact arguments, whole. A long one scrolls inside its box until shown all. */
function Payload({ tool, text }: { tool: string; text: string }) {
  const captionId = useId();
  const [open, setOpen] = useState(false);
  const lines = text.split("\n").length;
  const long = lines > PAYLOAD_FOLD_LINES || text.length > PAYLOAD_FOLD_CHARS;
  return (
    <figure aria-labelledby={captionId} className="flex min-w-0 flex-col gap-1">
      <figcaption id={captionId} className="text-muted-foreground text-xs">
        What will run: <code className="font-mono">{tool}</code>
      </figcaption>
      <pre
        className={cn(
          "whitespace-pre-wrap rounded-md border border-border bg-background p-2 font-mono text-xs [overflow-wrap:anywhere]",
          long && !open && "max-h-60 overflow-y-auto",
        )}
      >
        {text}
      </pre>
      {long && (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((was) => !was)}
          className="w-fit rounded-sm text-muted-foreground text-xs underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring"
        >
          {open
            ? "Show less"
            : lines > PAYLOAD_FOLD_LINES
              ? `Show all ${lines} lines`
              : `Show all ${text.length} characters`}
        </button>
      )}
    </figure>
  );
}

export const APPROVAL_SHOW_FULL = "Show the full action";
export const APPROVAL_HIDE_FULL = "Hide the full action";
export const APPROVAL_PAYLOAD_FAILED =
  "keeper could not show the attached action. Try again, or deny it.";

/**
 * An attached action, fetched whole on request: Rust checks the file against
 * the card's digest before answering, and says why when it cannot.
 */
function AttachedAction({
  card,
  accountId,
  roomId,
}: {
  card: ApprovalCardVm;
  accountId: string;
  roomId: string;
}) {
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [text, setText] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);

  const toggle = async () => {
    if (open || text !== null) {
      setOpen((was) => !was);
      return;
    }
    setRefusal(null);
    setLoading(true);
    try {
      setText(await agentApprovalPayload(accountId, roomId, card.id));
      setOpen(true);
    } catch (error) {
      setRefusal(syncErrorMessage(error, APPROVAL_PAYLOAD_FAILED));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-1.5 rounded-md border border-border border-dashed p-2">
      <p className="flex items-start gap-1.5 text-xs">
        <Paperclip aria-hidden="true" className="mt-px size-3.5 shrink-0" />
        <span>{card.attachment}</span>
      </p>
      <Button
        type="button"
        size="xs"
        variant="outline"
        className="w-fit"
        aria-expanded={open}
        aria-busy={loading}
        disabled={loading}
        onClick={() => void toggle()}
      >
        {open ? APPROVAL_HIDE_FULL : APPROVAL_SHOW_FULL}
      </Button>
      {open && text !== null && <Payload tool={card.tool} text={text} />}
      {refusal !== null && (
        <p role="status" className="text-destructive text-xs">
          {refusal}
        </p>
      )}
    </div>
  );
}

/** Where the card stands, as a word and an icon beside the tier. */
function StateBadge({ state }: { state: ApprovalCardVm["state"] }) {
  switch (state.state) {
    case "pending":
      return (
        <Badge variant="secondary">
          <Hourglass aria-hidden="true" />
          Waiting
        </Badge>
      );
    case "decided":
      return state.decision === "approve" ? (
        <Badge variant="default">
          <CircleCheck aria-hidden="true" />
          Approved
        </Badge>
      ) : (
        <Badge variant="destructive">
          <CircleX aria-hidden="true" />
          Denied
        </Badge>
      );
    case "consumed":
      return (
        <Badge variant="outline">
          <CheckCheck aria-hidden="true" />
          Used
        </Badge>
      );
    case "expired":
      return (
        <Badge variant="outline">
          <Clock aria-hidden="true" />
          Expired
        </Badge>
      );
  }
}

/** An expiry is a day and a time: a T2 card waits a day, so the hour alone would mislead. */
const EXPIRY_FORMAT = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

function When({ ms }: { ms: number }) {
  return <time dateTime={new Date(ms).toISOString()}>{EXPIRY_FORMAT.format(ms)}</time>;
}

/** The settled line: who decided, that it was used, or that it expired. */
function Outcome({ card }: { card: ApprovalCardVm }) {
  const { state } = card;
  switch (state.state) {
    case "pending":
      return null;
    case "decided":
      return (
        <p className="text-sm">
          {state.decision === "approve"
            ? state.scope === "session"
              ? `Approved for this session by ${state.byName}.`
              : `Approved once by ${state.byName}.`
            : `Denied by ${state.byName}.`}
        </p>
      );
    case "consumed":
      return (
        <p role="status" className="text-sm">
          Approved and used once. This does not confirm the action's outcome; that shows in the
          session.
        </p>
      );
    case "expired":
      return (
        <p role="status" className="text-sm">
          Expired at <When ms={card.expiresAt} /> before it was used, so it did not run.
        </p>
      );
  }
}

/** Approve (in each offered scope) or deny with an optional note. */
function Decide({
  card,
  accountId,
  roomId,
}: {
  card: ApprovalCardVm;
  accountId: string;
  roomId: string;
}) {
  const noteId = useId();
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [denying, setDenying] = useState(false);
  const [note, setNote] = useState("");

  const decide = async (decision: ApprovalDecision, scope: ApprovalScope) => {
    if (sending || sent) {
      return;
    }
    setRefusal(null);
    setSending(true);
    try {
      await agentApprovalDecide(accountId, roomId, {
        id: card.id,
        bindingDigest: card.bindingDigest,
        decision,
        scope,
        note: decision === "deny" && note.trim() !== "" ? note.trim() : null,
      });
      setSent(true);
    } catch (error) {
      setRefusal(syncErrorMessage(error, APPROVAL_DECIDE_FAILED));
    } finally {
      setSending(false);
    }
  };

  if (sent) {
    return (
      <p role="status" className="text-muted-foreground text-sm">
        {APPROVAL_SENT}
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      {denying ? (
        <div className="flex flex-col gap-1.5">
          <label htmlFor={noteId} className="text-xs">
            {APPROVAL_NOTE_LABEL}
          </label>
          <Textarea
            id={noteId}
            value={note}
            onChange={(e) => setNote(e.target.value)}
            disabled={sending}
            className="min-h-14"
          />
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              size="sm"
              variant="destructive"
              disabled={sending}
              onClick={() => void decide("deny", "once")}
            >
              {APPROVAL_SEND_DENY}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              disabled={sending}
              onClick={() => setDenying(false)}
            >
              {APPROVAL_CANCEL}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          {card.scopes.map((offer, i) => (
            <Button
              key={offer.scope}
              type="button"
              size="sm"
              variant={i === 0 ? "default" : "outline"}
              disabled={sending}
              aria-describedby={`${noteId}-${offer.scope}`}
              onClick={() => void decide("approve", offer.scope)}
            >
              {offer.label}
            </Button>
          ))}
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={sending}
            onClick={() => setDenying(true)}
          >
            {APPROVAL_DENY}
          </Button>
        </div>
      )}
      {!denying &&
        card.scopes.map((offer) => (
          <p
            key={offer.scope}
            id={`${noteId}-${offer.scope}`}
            className="text-muted-foreground text-xs"
          >
            {offer.detail}
          </p>
        ))}
      {refusal !== null && (
        <p role="status" className="text-destructive text-sm">
          {refusal}
        </p>
      )}
    </div>
  );
}

/** One record's card. */
function ApprovalCard({
  card,
  accountId,
  roomId,
  framed,
}: {
  card: ApprovalCardVm;
  accountId: string;
  roomId: string;
  /** A row of a coalesced request sits inside the request's frame. */
  framed: boolean;
}) {
  const weight = TIER_WEIGHT[card.tier] ?? HEAVIEST;
  const TierIcon = weight.icon;
  const waiting = card.state.state === "pending" || card.state.state === "decided";
  const head =
    card.declassify !== null ? declassifyQuestion(card.declassify.readers) : card.summary;
  // At T4 Rust's `only` already says who decides; its `cannotDecide` for anyone
  // else repeats that sentence, and a card says a thing once.
  const reason =
    card.cannotDecide !== null && card.only?.startsWith(card.cannotDecide) !== true
      ? card.cannotDecide
      : null;
  return (
    <article
      aria-label={`Approval request: ${head.split("`").join("")}`}
      className={cn(
        "flex min-w-0 flex-col gap-2 [overflow-wrap:anywhere]",
        framed
          ? "rounded-[14px] border border-border bg-card p-3 text-card-foreground text-sm"
          : "border-border border-t pt-3 first:border-t-0 first:pt-0",
        framed && weight.edge,
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <span className={cn("inline-flex items-start gap-1 font-medium text-xs", weight.tone)}>
          <TierIcon aria-hidden="true" className="mt-px size-3.5 shrink-0" />
          {card.tierWord}
        </span>
        <StateBadge state={card.state} />
      </div>
      <p className="font-medium">
        <Summary text={head} />
      </p>
      {card.declassify !== null && <p className="text-sm">{card.declassify.sentence}</p>}
      {card.payload !== null && <Payload tool={card.tool} text={card.payload} />}
      {card.attachment !== null && (
        <AttachedAction card={card} accountId={accountId} roomId={roomId} />
      )}
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-xs">
        <dt className="text-muted-foreground">Asked by</dt>
        <dd>{card.chain.map((p) => p.name).join(" → ")}</dd>
        <dt className="text-muted-foreground">Can decide</dt>
        <dd>
          {card.anyone
            ? "anyone who reads this room"
            : card.approvers.map((p) => p.name).join(", ")}
        </dd>
        {waiting && (
          <>
            <dt className="text-muted-foreground">Waits until</dt>
            <dd>
              <When ms={card.expiresAt} />
            </dd>
          </>
        )}
      </dl>
      {waiting && card.only !== null && <p className={cn("text-sm", weight.tone)}>{card.only}</p>}
      {waiting && card.canDecide && (
        <Decide key={card.state.state} card={card} accountId={accountId} roomId={roomId} />
      )}
      {waiting && !card.canDecide && (reason !== null || card.verify) && (
        <div className="flex flex-col items-start gap-2">
          {reason !== null && <p className="text-muted-foreground text-sm">{reason}</p>}
          {card.verify && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => verificationStore.getState().openFor(accountId)}
            >
              {APPROVAL_VERIFY}
            </Button>
          )}
        </div>
      )}
      <Outcome card={card} />
    </article>
  );
}

/**
 * One request in the stream: its card, or a coalesced request's rows.
 */
export function ApprovalRequest({
  approval,
  accountId,
  roomId,
}: {
  approval: ApprovalVm;
  accountId: string;
  roomId: string;
}) {
  const multiple = approval.cards.length > 1;
  const weight = TIER_WEIGHT[Math.max(...approval.cards.map((card) => card.tier))] ?? HEAVIEST;
  return (
    <section
      aria-label={multiple ? approvalListName(approval.cards.length) : undefined}
      className={cn(
        "my-2 max-w-[560px]",
        multiple &&
          "flex flex-col gap-3 rounded-[14px] border border-border bg-card p-3 text-card-foreground text-sm",
        multiple && weight.edge,
      )}
    >
      <p hidden={!multiple} className="text-muted-foreground text-xs">
        {approval.cards.length} actions in one request. Each is decided on its own.
      </p>
      <ul role={multiple ? undefined : "presentation"} className="flex flex-col gap-3">
        {approval.cards.map((card) => (
          <li key={card.id} role={multiple ? undefined : "presentation"}>
            <ApprovalCard card={card} accountId={accountId} roomId={roomId} framed={!multiple} />
          </li>
        ))}
      </ul>
    </section>
  );
}

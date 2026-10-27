/**
 * An agent's approval request, as the agents' database holds it (AD-428): the
 * tier as a word with a weight (an icon and the card's left edge: thin and
 * quiet at T2, the held colour at T3, the destructive colour at T4 — the word
 * always says it, so no tier is told by colour alone), keeper's one-sentence
 * summary, the exact action and its binding digest, Rust's sentence, and the
 * actions Rust lists for this person here (109.11): *Allow once* and *Deny* as
 * buttons, and every action in the `…` menu — *Always allow …* and *This
 * session only* only where Rust offers a grant (never at T4, in an untrusted
 * session, for `delegate`, a drive tool or a tool off the allow-list: left
 * out, not disabled). A grant is confirmed first in a dialog that says how
 * broad it is, in Rust's words. A decision is the approval row the runtime
 * reads; the card changes when the replica has it.
 *
 * The card over an agent's Matrix room's events left keeper in 108.2.
 */
import type { LucideIcon } from "lucide-react";
import { Ellipsis, OctagonAlert, Shield, ShieldAlert } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { DriveWritePreview } from "@/components/agents/drive-write-preview";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  type AgentApprovalActionVm,
  type AgentApprovalVm,
  type AgentGrantOfferVm,
  agentApprovalAlwaysAllow,
  agentSessionApprovalDecide,
} from "@/lib/ipc/client";
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

/** What each action reads as in the menu. */
export const ACTION_LABEL: Readonly<Record<AgentApprovalActionVm, string>> = {
  allowOnce: "Allow once",
  alwaysAllow: "Always allow …",
  sessionOnly: "This session only",
  deny: "Deny",
};

type GrantMode = "alwaysAllow" | "sessionOnly";

/** The confirm a grant takes: what it covers, how broad that is, and what revoking does. */
function GrantConfirm({
  offer,
  mode,
  onCancel,
  onConfirm,
  returnFocus,
}: {
  offer: AgentGrantOfferVm;
  mode: GrantMode;
  onCancel: () => void;
  onConfirm: (scope: string, expiresInDays: number | null) => void;
  /** Where focus goes once the confirm closes: it was opened from a menu that is gone. */
  returnFocus: () => void;
}) {
  const id = useId();
  const [scope, setScope] = useState(offer.scopes[0]?.id ?? "");
  const [expiry, setExpiry] = useState("never");
  const chosen = offer.scopes.find((s) => s.id === scope) ?? offer.scopes[0];
  const standing = mode === "alwaysAllow";
  return (
    <AlertDialog open onOpenChange={(open) => !open && onCancel()}>
      <AlertDialogContent
        className="max-h-[90dvh] overflow-y-auto"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          returnFocus();
        }}
      >
        <AlertDialogHeader>
          <AlertDialogTitle className="[overflow-wrap:anywhere]">
            {standing
              ? `Always allow ${offer.agentName} to use ${offer.tool}?`
              : `Allow ${offer.tool} for the rest of this session?`}
          </AlertDialogTitle>
          <AlertDialogDescription className="[overflow-wrap:anywhere]">
            {chosen ? (standing ? chosen.standing : chosen.session) : null}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {offer.scopes.length > 1 && (
          <RadioGroup
            aria-label="What it covers"
            value={scope}
            onValueChange={setScope}
            className="gap-1"
          >
            {offer.scopes.map((option) => (
              <div key={option.id} className="flex min-w-0 items-center gap-2">
                <RadioGroupItem id={`${id}-scope-${option.id}`} value={option.id} />
                <Label
                  htmlFor={`${id}-scope-${option.id}`}
                  className="min-h-11 min-w-0 content-center font-normal [overflow-wrap:anywhere]"
                >
                  {option.label}
                </Label>
              </div>
            ))}
          </RadioGroup>
        )}
        <RadioGroup
          aria-label="How long"
          value={expiry}
          onValueChange={setExpiry}
          className="gap-1"
        >
          {offer.expiries.map((option) => {
            const value = option.days === undefined ? "never" : String(option.days);
            return (
              <div key={value} className="flex min-w-0 items-center gap-2">
                <RadioGroupItem id={`${id}-expiry-${value}`} value={value} />
                <Label
                  htmlFor={`${id}-expiry-${value}`}
                  className="min-h-11 content-center font-normal"
                >
                  {option.label}
                </Label>
              </div>
            );
          })}
        </RadioGroup>
        <p className="text-muted-foreground text-sm [overflow-wrap:anywhere]">{offer.never}</p>
        <p className="text-muted-foreground text-sm [overflow-wrap:anywhere]">{offer.revokeNote}</p>
        <AlertDialogFooter>
          <AlertDialogCancel className="min-h-11">Cancel</AlertDialogCancel>
          <AlertDialogAction
            className="min-h-11"
            onClick={() => onConfirm(scope, expiry === "never" ? null : Number(expiry))}
          >
            {standing ? "Always allow" : "Allow in this session"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** A database approval is bound to the exact action and digest Rust supplied. */
export function DatabaseApprovalCard({
  approval,
  onDecided,
}: {
  approval: AgentApprovalVm;
  /** A decision on it was written: the card will move, so its holder places focus next. */
  onDecided?: (approval: AgentApprovalVm) => void;
}) {
  const [sending, setSending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<GrantMode | null>(null);
  const card = useRef<HTMLElement>(null);
  const more = useRef<HTMLButtonElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: an authoritative state transition retires an earlier command failure
  useEffect(() => {
    setFailure(null);
  }, [approval.state, approval.canDecide, approval.sentence]);
  const target = { database: approval.database, sessionId: approval.sessionId };
  const run = async (write: () => Promise<unknown>) => {
    if (sending || !approval.canDecide) return;
    setSending(true);
    setFailure(null);
    try {
      await write();
      onDecided?.(approval);
    } catch (raw) {
      setFailure(syncErrorMessage(raw, APPROVAL_DECIDE_FAILED));
    } finally {
      setSending(false);
    }
  };
  const act = (action: AgentApprovalActionVm) => {
    if (action === "alwaysAllow" || action === "sessionOnly") {
      if (approval.grant) setConfirming(action);
      return;
    }
    void run(() =>
      agentSessionApprovalDecide(
        target,
        approval.id,
        approval.bindingDigest,
        action === "allowOnce",
      ),
    );
  };
  const actions = approval.actions;
  const weight = TIER_WEIGHT[Number(approval.tier.replace("T", ""))] ?? HEAVIEST;
  const Icon = weight.icon;
  return (
    <article
      ref={card}
      data-approval-id={approval.id}
      tabIndex={-1}
      aria-label={`Approval: ${approval.summary}`}
      className={cn("min-w-0 rounded-md border border-border bg-card p-3", weight.edge)}
    >
      <div
        className={cn("mb-2 flex flex-wrap items-center gap-2 font-medium text-sm", weight.tone)}
      >
        <Icon className="size-4" aria-hidden="true" />
        <span>{approval.tier}</span>
        <span className="break-all">{approval.toolName}</span>
        <span className="break-all text-muted-foreground text-xs">{approval.database}</span>
      </div>
      <p className="break-words text-sm">{approval.summary}</p>
      {approval.write && <DriveWritePreview write={approval.write} />}
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
      {actions.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-2">
          {actions.includes("allowOnce") && (
            <Button
              className="min-h-11 min-w-11"
              size="sm"
              disabled={sending}
              onClick={() => act("allowOnce")}
            >
              {ACTION_LABEL.allowOnce}
            </Button>
          )}
          {actions.includes("deny") && (
            <Button
              size="sm"
              className="min-h-11 min-w-11"
              variant="outline"
              disabled={sending}
              onClick={() => act("deny")}
            >
              {ACTION_LABEL.deny}
            </Button>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                ref={more}
                size="icon"
                variant="ghost"
                className="size-11"
                aria-busy={sending}
                aria-label="More actions"
              >
                <Ellipsis aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {actions.map((action) => (
                <DropdownMenuItem key={action} className="min-h-11" onSelect={() => act(action)}>
                  {ACTION_LABEL[action]}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}
      {confirming && approval.grant && (
        <GrantConfirm
          offer={approval.grant}
          mode={confirming}
          onCancel={() => setConfirming(null)}
          returnFocus={() => {
            // The menu's trigger, or the card itself once a decision has taken the menu away.
            if (more.current?.isConnected) more.current.focus();
            else card.current?.focus();
          }}
          onConfirm={(scope, expiresInDays) => {
            setConfirming(null);
            void run(() =>
              agentApprovalAlwaysAllow(
                target,
                approval.id,
                approval.bindingDigest,
                scope,
                confirming === "sessionOnly",
                expiresInDays,
              ),
            );
          }}
        />
      )}
    </article>
  );
}

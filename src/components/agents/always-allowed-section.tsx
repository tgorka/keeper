/**
 * Settings › Agents › *Always allowed* (109.11, Q109-09 b): every standing grant this person made
 * in a held database, newest first, with its agent, what it covers (its breadth in Rust's words),
 * its database, the session it is bound to, its expiry and its state — and *Revoke* where Rust
 * says it can still be revoked. Revoking stops new uses, not the approvals a grant already issued,
 * and the section says so (`note`). A grant the server refused, and a refused revocation, show
 * Rust's plain sentence.
 *
 * Every word but the copy is Rust's (`AgentGrantsVm`). On every tier and behind no capability: a
 * phone grants and revokes the same way. Absent while the person holds no grant (AD-27).
 */
import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { logFailure } from "@/lib/failure";
import { formatRoomTimestamp } from "@/lib/format-time";
import {
  type AgentGrantsVm,
  agentGrantRevoke,
  agentGrantsSubscribe,
  agentGrantsUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

export const ALWAYS_ALLOWED_TITLE = "Always allowed";
const READ_FAILED = "Could not read what you always allow.";
const REVOKE_FAILED = "keeper could not revoke that grant. It still stands; try again.";

const STATE_LABEL = {
  active: "Active",
  waiting: "Waiting",
  revoked: "Revoked",
  ended: "Ended",
  expired: "Expired",
  inactive: "Inactive",
  refused: "Refused",
} as const;

export function AlwaysAllowedSection({ open }: { open: boolean }) {
  const id = useId();
  const [vm, setVm] = useState<AgentGrantsVm | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [alert, setAlert] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    let subscription: string | null = null;
    void agentGrantsSubscribe((next) => {
      if (!cancelled) setVm(next);
    })
      .then((sid) => {
        if (cancelled) void agentGrantsUnsubscribe(sid);
        else subscription = sid;
      })
      .catch((raw: unknown) => {
        logFailure("agents.grantsSubscribe")(raw);
        if (!cancelled) setAlert(syncErrorMessage(raw, READ_FAILED));
      });
    return () => {
      cancelled = true;
      if (subscription !== null) void agentGrantsUnsubscribe(subscription);
    };
  }, [open]);

  if ((vm === null || vm.grants.length === 0) && alert === null) return null;

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        {ALWAYS_ALLOWED_TITLE}
      </h3>
      {vm && <p className="text-muted-foreground text-xs">{vm.note}</p>}
      {alert !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {alert}
        </p>
      )}
      <ul aria-label={ALWAYS_ALLOWED_TITLE} className="flex min-w-0 flex-col gap-2">
        {vm?.grants.map((grant) => (
          <li
            key={`${grant.database}/${grant.id}`}
            data-state={grant.state}
            className="flex min-w-0 flex-col gap-1 rounded-md border p-3"
          >
            <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
              <span className="font-medium [overflow-wrap:anywhere]">{grant.agentName}</span>
              <span className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
                {grant.database}
              </span>
              <span
                className={cn(
                  "text-xs",
                  grant.state === "refused" ? "text-destructive" : "text-muted-foreground",
                )}
              >
                {STATE_LABEL[grant.state]}
              </span>
            </div>
            <p className="[overflow-wrap:anywhere]">{grant.scope}</p>
            {grant.session !== undefined && (
              <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
                This session only: {grant.session}
              </p>
            )}
            {grant.expiresAtMs !== undefined && (
              <p className="text-muted-foreground text-xs">
                {grant.state === "expired" ? "Expired" : "Expires"}{" "}
                {formatRoomTimestamp(grant.expiresAtMs)}
              </p>
            )}
            <p
              role={grant.state === "refused" ? "alert" : undefined}
              className={cn(
                "text-xs [overflow-wrap:anywhere]",
                grant.state === "refused" ? "text-destructive" : "text-muted-foreground",
              )}
            >
              {grant.sentence}
            </p>
            {grant.refused !== undefined && (
              <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
                {grant.refused}
              </p>
            )}
            {grant.canRevoke && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="min-h-11 self-start"
                disabled={busy !== null}
                aria-label={`Revoke: ${grant.agentName}, ${grant.scope}`}
                onClick={() => {
                  setBusy(grant.id);
                  setAlert(null);
                  void agentGrantRevoke(grant.database, grant.id)
                    .catch((raw: unknown) => {
                      logFailure("agents.grantRevoke")(raw);
                      setAlert(syncErrorMessage(raw, REVOKE_FAILED));
                    })
                    .finally(() => setBusy(null));
                }}
              >
                Revoke
              </Button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

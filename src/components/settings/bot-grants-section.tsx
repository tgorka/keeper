/**
 * Settings → Grants: every grant a bot holds, and every tool call that was
 * checked against one (Epic 61, Story 61.10, FR-386, FR-388, NFR-47).
 *
 * # One list, because that is the promise
 *
 * "What can this bot change?" must be answerable by reading a list, never by
 * remembering which dialogs were clicked through. So this section shows every
 * grant — live, revoked, and the rows this build cannot read — grouped by the
 * endpoint and the bot they speak for, each with the one act that ends it.
 * A revoked grant stays visible with its state, because
 * `bots_grants_list`'s own contract keeps the row so every audit line naming it
 * still resolves.
 *
 * # The log's reader is a human
 *
 * So each line names the path (`profile/sub/path`, composed in Rust), the tool
 * as the model called it, what the grant check concluded and what became of the
 * call. Claude Code's default telemetry redacts the file path entirely
 * (research §8.1); a log that cannot say which file was touched is not an audit
 * trail.
 *
 * **A pending row is rendered as pending.** The row is written and committed
 * *before* the effect (NFR-47), so a row that never completed is a call that
 * was in flight when the process stopped. That is evidence, not a defect, and
 * printing it as a success would destroy the only property the log has.
 *
 * # Nothing here rewrites Rust's words
 *
 * A refusal sentence stored on an audit row is rendered from the row. The only
 * sentences this file owns are its own captions.
 */
import { useCallback, useEffect, useState } from "react";
import {
  GRANT_PROVIDER_WIDE_NOTE,
  GRANT_REVOKE_LABEL,
  grantSentence,
} from "@/components/bots/bot-grant-bar";
import { Button } from "@/components/ui/button";
import type {
  BotAuditRowVm,
  BotGrantVm,
  BotProviderVm,
  BotVm,
  UnknownBotGrantVm,
} from "@/lib/ipc/client";
import {
  botsAuditList,
  botsBotsList,
  botsGrantRevoke,
  botsGrantsList,
  botsProvidersList,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const GRANTS_SECTION_TITLE = "Grants";

/** The standing explanation. */
export const GRANTS_SECTION_NOTE =
  "A grant is the only reason a bot may read or change a file in the folders you sync. Everything a bot can reach is on this list, and every line on it can be revoked.";

/** The empty states. */
export const GRANTS_SECTION_EMPTY =
  "No grant yet. No bot can read or write anything in the folders you sync.";
export const AUDIT_EMPTY = "No tool call has been checked against a grant yet.";

/** The audit log's own heading. */
export const AUDIT_TITLE = "Tool calls";

/** What a pending row means, said once and honestly. */
export const AUDIT_PENDING_CAPTION =
  "Pending — the row is written before the effect, so one still pending after a restart is a call that was in flight when keeper stopped.";

/** How a revoked grant reads. */
export const GRANT_REVOKED_CAPTION = "Revoked. It permits nothing.";

/** How a grant row this build cannot read reads, and its one action. */
export const GRANT_UNREADABLE_CAPTION =
  "keeper cannot read this grant, so it permits nothing. Revoking it is the only thing to do with it.";

/** How a group whose endpoint is gone is named. */
export const GRANTS_PROVIDER_GONE = "An endpoint keeper no longer holds";

/** How a grant covering every bot of an endpoint is named. */
export const GRANTS_EVERY_BOT = "Every bot of this endpoint";

/** What a failed read or revoke says when Rust gave no sentence. */
export const GRANTS_READ_FAILED = "keeper couldn't read your grants.";
export const GRANTS_REVOKE_FAILED = "keeper couldn't revoke that grant.";

/**
 * One audit line's verdict and outcome, in the words they are stored in.
 *
 * A verdict this build cannot read prints as unreadable rather than as one of
 * the three it knows — never defaulted, which would understate what happened.
 */
export function auditLine(row: BotAuditRowVm): string {
  const verdict = row.verdict ?? "unreadable";
  const effect = row.effect ?? "unreadable";
  return `${row.path} — ${row.tool} (${effect}), ${verdict}, ${row.outcome}`;
}

/** Settings → Grants. */
export function BotGrantsSection({ open }: { open: boolean }) {
  const [grants, setGrants] = useState<BotGrantVm[] | null>(null);
  const [unknown, setUnknown] = useState<UnknownBotGrantVm[]>([]);
  const [providers, setProviders] = useState<BotProviderVm[]>([]);
  const [bots, setBots] = useState<BotVm[]>([]);
  const [audit, setAudit] = useState<BotAuditRowVm[]>([]);
  const [error, setError] = useState<string | null>(null);

  // No dependencies, so the effect below can name it: the section re-reads when
  // the dialog opens and after a revocation, and at no other time.
  const refresh = useCallback(() => {
    void Promise.allSettled([
      botsGrantsList(),
      botsProvidersList(),
      botsBotsList(),
      botsAuditList(),
    ]).then(([grantRead, providerRead, botRead, auditRead]) => {
      if (grantRead.status === "fulfilled") {
        setGrants(grantRead.value.grants);
        setUnknown(grantRead.value.unknown);
      }
      if (providerRead.status === "fulfilled") {
        setProviders(providerRead.value);
      }
      if (botRead.status === "fulfilled") {
        setBots(botRead.value);
      }
      if (auditRead.status === "fulfilled") {
        setAudit(auditRead.value);
      }
      const failure = [grantRead, providerRead, botRead, auditRead].find(
        (read) => read.status === "rejected",
      );
      setError(
        failure === undefined || failure.status !== "rejected"
          ? null
          : syncErrorMessage(failure.reason, GRANTS_READ_FAILED),
      );
    });
  }, []);

  useEffect(() => {
    if (open) {
      refresh();
    }
  }, [open, refresh]);

  const grantList = grants ?? [];
  const revoke = (grantId: string) => {
    void botsGrantRevoke(grantId)
      .catch((raw: unknown) => setError(syncErrorMessage(raw, GRANTS_REVOKE_FAILED)))
      .finally(refresh);
  };

  // Grouped by the endpoint, then by the bot — the two levels a grant is
  // addressed at, in the order they were created.
  const providerIds = [...new Set(grantList.map((grant) => grant.providerId))];

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">{GRANTS_SECTION_TITLE}</p>
      <p className="text-muted-foreground">{GRANTS_SECTION_NOTE}</p>

      {error !== null && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}

      {grantList.length === 0 && unknown.length === 0 ? (
        <p className="text-muted-foreground">{GRANTS_SECTION_EMPTY}</p>
      ) : (
        <ul aria-label={GRANTS_SECTION_TITLE} className="flex flex-col gap-3">
          {providerIds.map((providerId) => {
            const provider = providers.find((row) => row.id === providerId) ?? null;
            const mine = grantList.filter((grant) => grant.providerId === providerId);
            const botIds = [...new Set(mine.map((grant) => grant.botId))];
            return (
              <li key={providerId} className="flex flex-col gap-2">
                <p className="font-medium text-xs">
                  {/* The host, never the full URL — the egress disclosure's
                      own shape. A grant can outlive the endpoint it names, and
                      that row says so rather than hiding. */}
                  {provider === null
                    ? `${GRANTS_PROVIDER_GONE} (${providerId})`
                    : `${provider.name} — ${provider.kind} at ${provider.host}`}
                </p>
                {botIds.map((botId) => {
                  const bot =
                    botId === null ? null : (bots.find((row) => row.id === botId) ?? null);
                  return (
                    <div key={botId ?? "every-bot"} className="flex flex-col gap-1 pl-3">
                      <p className="text-muted-foreground text-xs">
                        {botId === null
                          ? GRANTS_EVERY_BOT
                          : (bot?.name ?? `A bot keeper no longer holds (${botId})`)}
                      </p>
                      {mine
                        .filter((grant) => grant.botId === botId)
                        .map((grant) => (
                          <div key={grant.id} className="flex flex-col gap-1">
                            <div className="flex items-center gap-2">
                              <span className="min-w-0 flex-1 text-xs">
                                {grantSentence(grant)}
                                {grant.botId === null ? ` ${GRANT_PROVIDER_WIDE_NOTE}` : ""}
                              </span>
                              {grant.revokedMs === null && (
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  onClick={() => revoke(grant.id)}
                                >
                                  {GRANT_REVOKE_LABEL}
                                </Button>
                              )}
                            </div>
                            {grant.revokedMs !== null && (
                              <p className="text-muted-foreground text-xs">
                                {GRANT_REVOKED_CAPTION}
                              </p>
                            )}
                          </div>
                        ))}
                    </div>
                  );
                })}
              </li>
            );
          })}
          {unknown.map((row) => (
            <li key={row.id} className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 text-xs">
                  {row.providerId} — {row.scopeKind}, {row.mode}
                </span>
                <Button type="button" variant="outline" size="sm" onClick={() => revoke(row.id)}>
                  {GRANT_REVOKE_LABEL}
                </Button>
              </div>
              <p className="text-muted-foreground text-xs">{GRANT_UNREADABLE_CAPTION}</p>
            </li>
          ))}
        </ul>
      )}

      <p className="font-medium">{AUDIT_TITLE}</p>
      {audit.length === 0 ? (
        <p className="text-muted-foreground">{AUDIT_EMPTY}</p>
      ) : (
        <ul aria-label={AUDIT_TITLE} className="flex flex-col gap-1">
          {/* Newest first, as Rust returns them — no re-sort here, so the order
              on screen is the order the store read. */}
          {audit.map((row) => (
            <li key={row.id} className="flex flex-col">
              <span className="text-xs">{auditLine(row)}</span>
              {row.outcome === "pending" && (
                <span className="text-muted-foreground text-xs">{AUDIT_PENDING_CAPTION}</span>
              )}
              {row.reason !== null && (
                // Rust's sentence, verbatim: the log and the pane say the same
                // words about one refusal.
                <span className="text-muted-foreground text-xs">{row.reason}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * Settings › Agents › Server › Your domains (114.5, UX-DR-E114-1): the domains this device opens,
 * each with its kind and the people in it, the git repository one replaces, and the domains the
 * person left — beside the databases' status lines, as one more part of the agents server's
 * state. Never in Settings › Account: a domain is not the keeper account.
 *
 * A pure renderer of `DomainsVm`, which Rust builds from discovery and each domain's own
 * directory: whether a domain is listed, left or held after this device was removed, and who is
 * in it, are Rust's. This file only words the states — reading, empty, offline (the last known,
 * with its time), removed (every copy stays, none syncs), a domain of one person, and a left
 * domain: its copy deleted, or kept on this device with the writes only it holds.
 */
import { useEffect, useState } from "react";
import { logFailure } from "@/lib/failure";
import {
  type DomainsVm,
  type DomainVm,
  hqDomainsSubscribe,
  hqDomainsUnsubscribe,
  type LeftDomainVm,
} from "@/lib/ipc/client";

export const DOMAINS_TITLE = "Your domains";
export const DOMAINS_READING = "Reading your domains…";
export const DOMAINS_EMPTY = "This device opens no domain yet.";
export const DOMAINS_READ_FAILED = "Could not read your domains.";
export const DOMAINS_REMOVED =
  "This device was removed from the server. Its copies stay on it, readable, and none syncs.";
export const ONLY_YOU = "Only you";
export const PEOPLE_UNREAD = "People not on this device yet";
export const LEFT_TITLE = "Domains you left";

const KIND: Record<DomainVm["kind"], string> = {
  general: "Everyone",
  personal: "Personal",
  group: "Group",
};

export function offlineNote(readAtMs: number | null): string {
  const when = readAtMs === null ? "never" : new Date(readAtMs).toLocaleString();
  return `Offline: as the server last said, ${when}.`;
}

export function replacesNote(repository: string): string {
  return `Replaces the git repository ${repository}.`;
}

function counted(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

export function leftNote(left: LeftDomainVm): string {
  if (!left.kept) {
    return (
      "Its copy was deleted from this device. " +
      "Files already opened elsewhere and links not yet expired may remain."
    );
  }
  const held = [
    left.unsent > 0 ? counted(left.unsent, "write", "writes") : null,
    left.graphIntents > 0 ? counted(left.graphIntents, "graph change", "graph changes") : null,
  ].filter((part) => part !== null);
  const what =
    held.length === 0
      ? "Its copy is kept on this device: its unsent changes could not be counted."
      : `Its copy is kept on this device with ${held.join(" and ")} not yet sent.`;
  return `${what} They stay on this device and are sent if you are added to it again.`;
}

function people(domain: DomainVm): string {
  if (domain.members === null) return PEOPLE_UNREAD;
  const others = domain.members.filter((member) => !member.me);
  if (others.length === 0) return ONLY_YOU;
  return domain.members.map((member) => (member.me ? "You" : member.name)).join(", ");
}

function DomainRow({ domain }: { domain: DomainVm }) {
  return (
    <li aria-label={domain.displayName} data-kind={domain.kind} className="flex min-w-0 flex-col">
      <span className="flex min-w-0 flex-wrap gap-x-2">
        <span className="font-medium [overflow-wrap:anywhere]">{domain.displayName}</span>
        <span className="text-muted-foreground">{KIND[domain.kind]}</span>
      </span>
      <span className="text-muted-foreground [overflow-wrap:anywhere]">{people(domain)}</span>
      {domain.replacedRepository !== undefined && (
        <span className="text-muted-foreground [overflow-wrap:anywhere]">
          {replacesNote(domain.replacedRepository)}
        </span>
      )}
    </li>
  );
}

export function AgentsDomains({ open }: { open: boolean }) {
  const [view, setView] = useState<DomainsVm | undefined>(undefined);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    let subscription: string | null = null;
    void hqDomainsSubscribe((next) => {
      if (!cancelled) setView(next);
    })
      .then((sid) => {
        if (cancelled) void hqDomainsUnsubscribe(sid);
        else subscription = sid;
      })
      .catch((raw: unknown) => {
        logFailure("hq.domainsSubscribe")(raw);
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      if (subscription !== null) void hqDomainsUnsubscribe(subscription);
    };
  }, [open]);

  if (failed) {
    return (
      <p role="alert" className="text-destructive">
        {DOMAINS_READ_FAILED}
      </p>
    );
  }
  if (view === undefined || view.state === "notConnected") return null;

  const nothing = view.domains.length === 0;
  return (
    <div className="flex min-w-0 flex-col gap-2" data-state={view.state}>
      <h4 className="font-medium">{DOMAINS_TITLE}</h4>
      {view.state === "offline" && (
        <p role="status" className="text-muted-foreground [overflow-wrap:anywhere]">
          {offlineNote(view.readAtMs)}
        </p>
      )}
      {view.state === "removed" && (
        <p role="status" className="text-muted-foreground [overflow-wrap:anywhere]">
          {DOMAINS_REMOVED}
        </p>
      )}
      {nothing && view.state === "reading" && <p role="status">{DOMAINS_READING}</p>}
      {nothing && view.state !== "reading" && (
        <p className="text-muted-foreground">{DOMAINS_EMPTY}</p>
      )}
      {!nothing && (
        <ul aria-label={DOMAINS_TITLE} className="flex min-w-0 flex-col gap-2">
          {view.domains.map((domain) => (
            <DomainRow key={domain.name} domain={domain} />
          ))}
        </ul>
      )}
      {view.left.length > 0 && (
        <>
          <h4 className="font-medium">{LEFT_TITLE}</h4>
          <ul aria-label={LEFT_TITLE} className="flex min-w-0 flex-col gap-2">
            {view.left.map((left) => (
              <li
                key={left.name}
                aria-label={left.displayName}
                data-kept={left.kept}
                className="flex min-w-0 flex-col"
              >
                <span className="font-medium [overflow-wrap:anywhere]">
                  You left {left.displayName}
                </span>
                <span className="text-muted-foreground [overflow-wrap:anywhere]">
                  {leftNote(left)}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

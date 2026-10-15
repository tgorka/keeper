/**
 * Settings › Agents › Claims (R455): whether this Mac takes a session's claim
 * on the quarter-hour lease while it hosts the person's agents.
 *
 * Off by default, and never turned on by keeper: a host built before the
 * quarter-hour lease reads every claim as free after three minutes, and such a
 * host can come back from being offline at any time. So the person turns it on
 * — here for this Mac, `[claims] lease = "quarter-hour"` in `agentd.toml` for a
 * server — only once every device and keeper-agentd serving their agents runs
 * this version or later; even then Rust takes it only while every live host in
 * the control room says it reads it. What is stored, and what a settings file
 * decides instead, is Rust's answer; a read or save that fails says so here and
 * reaches `keeper.log`.
 */
import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useId, useState } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { logFailure } from "@/lib/failure";
import { agentsClaimsLeaseGet, agentsClaimsLeaseSet, type ClaimsLease } from "@/lib/ipc/client";

export const CLAIMS_LEASE_TITLE = "Claims";
export const CLAIMS_LEASE_LABEL = "Renew claims every 15 minutes";

/** The help beside the switch: when it is safe to turn on. */
export function claimsLeaseNote(version: string | null): string {
  const build = version === null ? "this version of keeper" : `keeper ${version}`;
  return `A host renews its claim on a conversation every minute, and each renewal is an event in the room. Every 15 minutes keeps quiet rooms quiet, but turn it on only after every device and keeper-agentd that serves your agents runs ${build} or later: an older one would take a conversation over while this Mac still writes it. Until every host says it reads it, claims stay on the minute.`;
}

/** Rust's sentence from a rejected command. */
function messageOf(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error as { message: unknown };
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return "Something went wrong. Try again.";
}

export function AgentsClaimsLeaseRow({ open }: { open: boolean }) {
  const id = useId();
  const [lease, setLease] = useState<ClaimsLease | undefined>(undefined);
  const [version, setVersion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    let live = true;
    agentsClaimsLeaseGet()
      .then((read) => {
        if (live) {
          setLease(read);
          setError(null);
        }
      })
      .catch((raw: unknown) => {
        logFailure("agents.claimsLeaseGet")(raw);
        if (live) {
          setError(messageOf(raw));
        }
      });
    getVersion()
      .then((read) => {
        if (live) {
          setVersion(read);
        }
      })
      // The note says "this version" instead.
      .catch(logFailure("agents.claimsLeaseVersion"));
    return () => {
      live = false;
    };
  }, [open]);

  const choose = (quarterHour: boolean) => {
    setSaving(true);
    agentsClaimsLeaseSet(quarterHour ? "quarter-hour" : "short")
      .then((stored) => {
        setLease(stored);
        setError(null);
      })
      .catch((raw: unknown) => {
        logFailure("agents.claimsLeaseSet")(raw);
        setError(messageOf(raw));
      })
      .finally(() => setSaving(false));
  };

  return (
    <div className="flex min-w-0 flex-col gap-1 border-t pt-2">
      <h4 className="font-medium">{CLAIMS_LEASE_TITLE}</h4>
      <div className="flex items-center gap-2">
        <Checkbox
          id={`${id}-lease`}
          checked={lease === "quarter-hour"}
          disabled={lease === undefined || saving}
          onCheckedChange={(checked) => choose(checked === true)}
        />
        <Label htmlFor={`${id}-lease`}>{CLAIMS_LEASE_LABEL}</Label>
        <FileControlled settingKey="agents.claims_lease" />
      </div>
      <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
        {claimsLeaseNote(version)}
      </p>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
    </div>
  );
}

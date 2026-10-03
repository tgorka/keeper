/**
 * Settings → Agents: this Mac as a host for your agents (Story 90.6, UX-DR128).
 *
 * One row per agent of your folders flagged for agents: "Sign in on this Mac"
 * with a password field, or "Signed in as nixi@hesperia". Everything shown is
 * Rust's — which drives are yours, which agents they hold, whether a copy is
 * signed in, and why one is not hosted here; this file decides none of it.
 *
 * **The pin (S-15).** This Mac hosts a drive's agents only under the owner,
 * readers and local-only setting you pinned here. The first sign-in for a
 * drive shows them and pins exactly what it showed: Rust refuses when
 * `_drive.toml` changed in between. When a pinned drive's file says something
 * else, the drive shows each difference and *Review readers*, which puts the
 * pinned and the new values side by side; only *Pin owner, readers and
 * local-only* re-pins. Nothing re-pins by itself.
 *
 * **Kept current.** The rows' sentences change on the host's own schedule (a
 * control room found, a drive that started to differ), so the section reads
 * them again every few seconds while the dialog is open.
 *
 * **Absent, not empty (AD-27).** No flagged folder with an agent renders
 * nothing, and the dialog renders the section only where `botTools` is true —
 * a phone is never a host.
 */
import { useCallback, useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type AgentCopyVm,
  type AgentPersonVm,
  type AgentPinVm,
  agentsCopies,
  agentsCopySignIn,
  agentsDriveRepin,
} from "@/lib/ipc/client";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const AGENTS_SECTION_TITLE = "Agents";

export const AGENTS_SECTION_NOTE =
  "Agents in your drives can answer from this Mac while it is awake. If you run another host, it takes over when this Mac is not.";

export const SIGN_IN_LABEL = "Sign in on this Mac";
export const REVIEW_READERS_LABEL = "Review readers";
export const PIN_LABEL = "Pin owner, readers and local-only";

/** How often the open section reads the rows again: the host rescans every 5 s. */
export const AGENTS_RELOAD_MS = 5000;

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

function personText(person: AgentPersonVm): string {
  return person.displayName === null
    ? person.matrixId
    : `${person.displayName} (${person.matrixId})`;
}

/** The owner, readers and local-only setting as one list, owner first. */
function People({
  owner,
  readers,
  localOnly,
}: {
  owner: AgentPersonVm | null;
  readers: AgentPersonVm[];
  localOnly: boolean | null;
}) {
  return (
    <ul className="flex min-w-0 flex-col gap-0.5 text-xs">
      {owner !== null && <li className="[overflow-wrap:anywhere]">Owner: {personText(owner)}</li>}
      {readers.map((reader) => (
        <li key={reader.matrixId} className="[overflow-wrap:anywhere]">
          Reader: {personText(reader)}
        </li>
      ))}
      {localOnly !== null && <li>Local models only: {localOnly ? "yes" : "no"}</li>}
    </ul>
  );
}

/** Every row, newest answer from Rust; `undefined` while the first read is out. */
export function AgentsSection({ open }: { open: boolean }) {
  const [rows, setRows] = useState<AgentCopyVm[] | undefined>(undefined);
  const [loadError, setLoadError] = useState<string | null>(null);
  const id = useId();

  const reload = useCallback(async () => {
    try {
      setRows(await agentsCopies());
      setLoadError(null);
    } catch (error) {
      setLoadError(messageOf(error));
    }
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }
    void reload();
    const timer = setInterval(() => void reload(), AGENTS_RELOAD_MS);
    return () => clearInterval(timer);
  }, [open, reload]);

  if (loadError === null && (rows === undefined || rows.length === 0)) {
    return null;
  }

  const drives = new Map<string, AgentCopyVm[]>();
  for (const row of rows ?? []) {
    const drive = drives.get(row.profileId);
    if (drive === undefined) {
      drives.set(row.profileId, [row]);
    } else {
      drive.push(row);
    }
  }

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        {AGENTS_SECTION_TITLE}
      </h3>
      <p className="text-muted-foreground text-xs">{AGENTS_SECTION_NOTE}</p>
      {loadError !== null && (
        <p role="alert" className="text-destructive text-xs">
          {loadError}
        </p>
      )}
      {[...drives.entries()].map(([profileId, agents]) => (
        <DriveAgents
          key={profileId}
          profileId={profileId}
          agents={agents}
          onRows={setRows}
          onChanged={reload}
        />
      ))}
    </section>
  );
}

function DriveAgents({
  profileId,
  agents,
  onRows,
  onChanged,
}: {
  profileId: string;
  agents: AgentCopyVm[];
  onRows: (rows: AgentCopyVm[]) => void;
  onChanged: () => Promise<void>;
}) {
  const first = agents[0] as AgentCopyVm;
  const pin = first.pin;
  return (
    <div className="flex min-w-0 flex-col gap-2 border-t pt-2">
      <h4 className="font-medium [overflow-wrap:anywhere]">{first.drive}</h4>
      {pin === null && first.problem !== null && (
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">{first.problem}</p>
      )}
      {pin?.state === "unpinned" && (
        <div className="flex min-w-0 flex-col gap-1">
          <p className="text-xs">
            Signing an agent in pins who may read {first.drive} on this Mac. It hosts this drive's
            agents only while its owner, readers and local-only setting stay these:
          </p>
          <People owner={pin.owner} readers={pin.readers} localOnly={pin.localOnly} />
        </div>
      )}
      {pin?.state === "differs" && (
        <PinReview profileId={profileId} drive={first.drive} pin={pin} onRows={onRows} />
      )}
      {pin !== null &&
        agents.map((agent) => (
          <AgentRow key={agent.agent} agent={agent} pin={pin} onChanged={onChanged} />
        ))}
    </div>
  );
}

function PinReview({
  profileId,
  drive,
  pin,
  onRows,
}: {
  profileId: string;
  drive: string;
  pin: AgentPinVm;
  onRows: (rows: AgentCopyVm[]) => void;
}) {
  const [reviewing, setReviewing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const repin = async () => {
    setBusy(true);
    setError(null);
    try {
      onRows(
        await agentsDriveRepin(profileId, {
          owner: pin.owner.matrixId,
          readers: pin.readers.map((reader) => reader.matrixId),
          localOnly: pin.localOnly,
        }),
      );
    } catch (failure) {
      setError(messageOf(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-2">
      <p className="text-xs">
        {drive}'s _drive.toml no longer says what this Mac pinned, so its agents are not hosted here
        until you review it.
      </p>
      <ul className="flex min-w-0 list-disc flex-col gap-0.5 pl-4 text-xs">
        {pin.differences.map((difference) => (
          <li key={difference} className="[overflow-wrap:anywhere]">
            {difference}
          </li>
        ))}
      </ul>
      {!reviewing && (
        <div>
          <Button size="sm" variant="outline" onClick={() => setReviewing(true)}>
            {REVIEW_READERS_LABEL}
          </Button>
        </div>
      )}
      {reviewing && (
        <div className="flex min-w-0 flex-col gap-2">
          <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-2">
            <div className="flex min-w-0 flex-col gap-1">
              <p className="font-medium text-xs">Pinned on this Mac</p>
              <People
                owner={pin.pinnedOwner}
                readers={pin.pinnedReaders}
                localOnly={pin.pinnedLocalOnly}
              />
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <p className="font-medium text-xs">Now in _drive.toml</p>
              <People owner={pin.owner} readers={pin.readers} localOnly={pin.localOnly} />
            </div>
          </div>
          <div className="flex min-w-0 flex-wrap gap-2">
            <Button size="sm" disabled={busy} onClick={() => void repin()}>
              {PIN_LABEL}
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setReviewing(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
    </div>
  );
}

function AgentRow({
  agent,
  pin,
  onChanged,
}: {
  agent: AgentCopyVm;
  pin: AgentPinVm;
  onChanged: () => Promise<void>;
}) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fieldId = useId();

  const signIn = async () => {
    setBusy(true);
    setError(null);
    const shown =
      pin.state === "unpinned"
        ? {
            owner: pin.owner.matrixId,
            readers: pin.readers.map((reader) => reader.matrixId),
            localOnly: pin.localOnly,
          }
        : null;
    try {
      await agentsCopySignIn(agent.profileId, agent.agent, password, shown);
      setPassword("");
      await onChanged();
    } catch (failure) {
      setError(messageOf(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <p className="[overflow-wrap:anywhere]">
        <span className="font-medium">{agent.name}</span>{" "}
        <span className="text-muted-foreground text-xs">{agent.matrixUser}</span>
      </p>
      {agent.signedIn ? (
        <p role="status" className="text-xs">
          {agent.host === null
            ? `Signed in as ${agent.agent}`
            : `Signed in as ${agent.agent}@${agent.host}`}
        </p>
      ) : (
        <form
          aria-label={`Sign ${agent.name} in on this Mac`}
          className="flex min-w-0 flex-wrap items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void signIn();
          }}
        >
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <Label htmlFor={fieldId} className="text-xs">
              Password for {agent.matrixUser}
            </Label>
            <Input
              id={fieldId}
              type="password"
              autoComplete="off"
              value={password}
              disabled={busy}
              onChange={(event) => setPassword(event.target.value)}
            />
          </div>
          <Button type="submit" size="sm" disabled={busy || password === ""}>
            {SIGN_IN_LABEL}
          </Button>
        </form>
      )}
      {agent.problem !== null && (
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">{agent.problem}</p>
      )}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
    </div>
  );
}

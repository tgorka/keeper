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
 * **Set up agents (Story 91.5, UX-DR133).** Each synced folder that keeps
 * agents (`[folder.agents]`) offers to seed its zone: the catalogue as
 * checkboxes, the drive's owner and readers (the zone's `_drive.toml`'s when
 * it has one, which the seed leaves), and the bot the agents run on, which
 * nothing picks for the person (S-20). The preview lists the files to write
 * and the files left; writing shows the written list and hands each seeded
 * agent to its sign-in row above. It signs nothing in itself. A folder this
 * Mac cannot seed shows Rust's sentence instead of the form.
 *
 * **Absent, not empty (AD-27).** No flagged folder renders nothing, and the
 * dialog renders the section only where `botTools` is true — a phone is never
 * a host.
 */
import { useCallback, useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Textarea } from "@/components/ui/textarea";
import {
  type AgentCopyVm,
  type AgentPersonVm,
  type AgentPinVm,
  type AgentSeedFolderVm,
  type AgentSeedOfferVm,
  type AgentSeedPlanVm,
  type AgentSeedReq,
  type AgentSeedResultVm,
  agentsCopies,
  agentsCopySignIn,
  agentsDriveRepin,
  agentsSeedApply,
  agentsSeedOffer,
  agentsSeedPlan,
} from "@/lib/ipc/client";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const AGENTS_SECTION_TITLE = "Agents";

export const AGENTS_SECTION_NOTE =
  "Agents in your drives can answer from this Mac while it is awake. If you run another host, it takes over when this Mac is not.";

export const SIGN_IN_LABEL = "Sign in on this Mac";
export const REVIEW_READERS_LABEL = "Review readers";
export const PIN_LABEL = "Pin owner, readers and local-only";

export const SET_UP_AGENTS_LABEL = "Set up agents";
export const SEED_PREVIEW_LABEL = "Show the files";
export const SEED_WRITE_LABEL = "Write the files";
export const SEED_NO_BOTS =
  "You have no bots yet. A bot you add in Settings › Bots is offered here.";

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
  const [offer, setOffer] = useState<AgentSeedOfferVm | undefined>(undefined);
  const [offerError, setOfferError] = useState<string | null>(null);
  const id = useId();

  const reload = useCallback(async () => {
    try {
      setRows(await agentsCopies());
      setLoadError(null);
    } catch (error) {
      setLoadError(messageOf(error));
    }
  }, []);

  const reloadOffer = useCallback(async () => {
    try {
      setOffer(await agentsSeedOffer());
      setOfferError(null);
    } catch (error) {
      setOfferError(messageOf(error));
    }
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }
    void reload();
    void reloadOffer();
    const timer = setInterval(() => void reload(), AGENTS_RELOAD_MS);
    return () => clearInterval(timer);
  }, [open, reload, reloadOffer]);

  const folders = offer?.folders ?? [];
  if (
    loadError === null &&
    offerError === null &&
    (rows === undefined || rows.length === 0) &&
    folders.length === 0
  ) {
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
  // A folder with nothing hosted yet is still offered *Set up agents*.
  for (const folder of folders) {
    if (!drives.has(folder.profileId)) {
      drives.set(folder.profileId, []);
    }
  }

  const rowId = (profileId: string, agent: string) => `${id}-row-${profileId}-${agent}`;
  // The hand-off from a seed to the agent's own sign-in row: its password
  // field when it has one, else the row.
  const goToRow = (profileId: string, agent: string) => {
    const row = document.getElementById(rowId(profileId, agent));
    if (row === null) {
      return;
    }
    row.scrollIntoView?.({ block: "nearest" });
    (row.querySelector<HTMLElement>("input") ?? row).focus();
  };
  const seeded = async () => {
    await Promise.all([reload(), reloadOffer()]);
  };

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
      {offerError !== null && (
        <p role="alert" className="text-destructive text-xs">
          {offerError}
        </p>
      )}
      {[...drives.entries()].map(([profileId, agents]) => (
        <DriveAgents
          key={profileId}
          agents={agents}
          folder={folders.find((folder) => folder.profileId === profileId)}
          offer={offer}
          rowId={(agent) => rowId(profileId, agent)}
          onRows={setRows}
          onChanged={reload}
          onSeeded={seeded}
          onGoToRow={(agent) => goToRow(profileId, agent)}
        />
      ))}
    </section>
  );
}

function DriveAgents({
  agents,
  folder,
  offer,
  rowId,
  onRows,
  onChanged,
  onSeeded,
  onGoToRow,
}: {
  agents: AgentCopyVm[];
  folder: AgentSeedFolderVm | undefined;
  offer: AgentSeedOfferVm | undefined;
  rowId: (agent: string) => string;
  onRows: (rows: AgentCopyVm[]) => void;
  onChanged: () => Promise<void>;
  onSeeded: () => Promise<void>;
  onGoToRow: (agent: string) => void;
}) {
  const headingId = useId();
  const first = agents[0];
  const pin = first?.pin ?? null;
  const drive = first?.drive ?? folder?.name ?? "";
  const rowProblem = first !== undefined && pin === null ? first.problem : null;
  return (
    <div className="flex min-w-0 flex-col gap-2 border-t pt-2">
      <h4 id={headingId} className="font-medium [overflow-wrap:anywhere]">
        {drive}
      </h4>
      {rowProblem !== null && (
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">{rowProblem}</p>
      )}
      {pin?.state === "unpinned" && (
        <div className="flex min-w-0 flex-col gap-1">
          <p className="text-xs">
            Signing an agent in pins who may read {drive} on this Mac. It hosts this drive's agents
            only while its owner, readers and local-only setting stay these:
          </p>
          <People owner={pin.owner} readers={pin.readers} localOnly={pin.localOnly} />
        </div>
      )}
      {first !== undefined && pin?.state === "differs" && (
        <PinReview profileId={first.profileId} drive={drive} pin={pin} onRows={onRows} />
      )}
      {pin !== null &&
        agents.map((agent) => (
          <AgentRow
            key={agent.agent}
            id={rowId(agent.agent)}
            agent={agent}
            pin={pin}
            onChanged={onChanged}
          />
        ))}
      {/* The same sentence from both reads is said once. */}
      {folder !== undefined && folder.problem !== null && folder.problem !== rowProblem && (
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">{folder.problem}</p>
      )}
      {folder !== undefined && folder.problem === null && offer !== undefined && (
        <SeedSetup
          folder={folder}
          offer={offer}
          rows={agents}
          headingId={headingId}
          onSeeded={onSeeded}
          onGoToRow={onGoToRow}
        />
      )}
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
  id,
  agent,
  pin,
  onChanged,
}: {
  id: string;
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
    <div id={id} tabIndex={-1} className="flex min-w-0 flex-col gap-1">
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

/** A titled list of zone-relative paths, named by its title. */
function FileList({ title, files }: { title: string; files: string[] }) {
  const id = useId();
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <p id={id} className="font-medium text-xs">
        {title} ({files.length})
      </p>
      <ul aria-labelledby={id} className="flex min-w-0 list-disc flex-col gap-0.5 pl-4 text-xs">
        {files.map((file) => (
          <li key={file} className="[overflow-wrap:anywhere]">
            {file}
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * *Set up agents* for one folder: closed, the form with its preview, or what
 * the write did. Every refusal is Rust's sentence; this decides none of them.
 */
function SeedSetup({
  folder,
  offer,
  rows,
  headingId,
  onSeeded,
  onGoToRow,
}: {
  folder: AgentSeedFolderVm;
  offer: AgentSeedOfferVm;
  rows: AgentCopyVm[];
  headingId: string;
  onSeeded: () => Promise<void>;
  onGoToRow: (agent: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [drive, setDrive] = useState(folder.drive);
  const [owner, setOwner] = useState(folder.owner);
  const [readers, setReaders] = useState(folder.readers.join("\n"));
  const [localOnly, setLocalOnly] = useState(folder.localOnly);
  const [picked, setPicked] = useState<string[]>([]);
  const [bot, setBot] = useState<string | null>(null);
  const [plan, setPlan] = useState<AgentSeedPlanVm | null>(null);
  const [result, setResult] = useState<AgentSeedResultVm | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const id = useId();

  const start = () => {
    setDrive(folder.drive);
    setOwner(folder.owner);
    setReaders(folder.readers.join("\n"));
    setLocalOnly(folder.localOnly);
    setPicked(folder.preselected);
    setBot(null);
    setPlan(null);
    setResult(null);
    setError(null);
    setOpen(true);
  };
  const close = () => {
    setOpen(false);
    setPlan(null);
    setResult(null);
    setError(null);
  };
  /** A change makes the shown preview stale, so it goes. */
  const edited = () => {
    setPlan(null);
    setError(null);
  };

  // A declared zone's drive, owner, readers and local_only are its `_drive.toml`'s.
  const request = (): AgentSeedReq => ({
    profileId: folder.profileId,
    drive: folder.declared ? folder.drive : drive.trim(),
    owner: folder.declared ? folder.owner : owner,
    readers: folder.declared
      ? folder.readers
      : readers
          .split("\n")
          .map((reader) => reader.trim())
          .filter((reader) => reader !== ""),
    localOnly: folder.declared ? folder.localOnly : localOnly,
    bot,
    with: offer.catalogue.map((agent) => agent.id).filter((agent) => picked.includes(agent)),
  });

  const preview = async () => {
    setBusy(true);
    setError(null);
    setPlan(null);
    try {
      setPlan(await agentsSeedPlan(request()));
    } catch (failure) {
      setError(messageOf(failure));
    } finally {
      setBusy(false);
    }
  };

  const write = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await agentsSeedApply(request()));
      setPlan(null);
      await onSeeded();
    } catch (failure) {
      setError(messageOf(failure));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <div>
        <Button size="sm" variant="outline" aria-describedby={headingId} onClick={start}>
          {SET_UP_AGENTS_LABEL}
        </Button>
      </div>
    );
  }

  if (result !== null) {
    const written = result.agents.map(
      (agent) =>
        offer.catalogue.find((entry) => entry.id === agent) ?? {
          id: agent,
          name: agent,
          kind: "",
          homeDrive: "",
        },
    );
    return (
      <div className="flex min-w-0 flex-col gap-2">
        <h5 className="font-medium text-xs">{SET_UP_AGENTS_LABEL}</h5>
        <p role="status" className="text-xs">
          Wrote {result.written.length === 1 ? "1 file" : `${result.written.length} files`} into{" "}
          {folder.name}'s agents zone. It signed nothing in.
        </p>
        {result.written.length > 0 && <FileList title="Written" files={result.written} />}
        {result.left.length > 0 && <FileList title="Left as they were" files={result.left} />}
        {written.length > 0 && (
          <div className="flex min-w-0 flex-col gap-1">
            <p className="text-xs">
              Each agent signs in on its own row, which pins the drive's readers on this Mac:
            </p>
            <ul className="flex min-w-0 flex-col gap-0.5 text-xs">
              {written.map((agent) => (
                <li key={agent.id} className="[overflow-wrap:anywhere]">
                  {rows.some((row) => row.agent === agent.id) ? (
                    <Button
                      size="sm"
                      variant="link"
                      className="h-auto p-0 text-xs"
                      onClick={() => onGoToRow(agent.id)}
                    >
                      Sign {agent.name} in
                    </Button>
                  ) : (
                    <span className="text-muted-foreground">
                      {agent.name}'s row shows here once this Mac has read the zone.
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </div>
        )}
        {written
          .filter((agent) => agent.kind === "proxy")
          .map((agent) => (
            <p key={agent.id} className="text-muted-foreground text-xs">
              {agent.name}'s room is made by keeper-agentd agents init on the server that hosts{" "}
              {agent.name}.
            </p>
          ))}
        <div>
          <Button size="sm" variant="ghost" onClick={close}>
            Done
          </Button>
        </div>
      </div>
    );
  }

  const owners = [...new Set([folder.owner, ...offer.accounts])];
  return (
    <div className="flex min-w-0 flex-col gap-3">
      <h5 className="font-medium text-xs">{SET_UP_AGENTS_LABEL}</h5>
      <p className="text-xs">
        Writes {folder.name}'s agents zone — its guide, rules, _drive.toml and template — and the
        agents you tick. A file already there is left as it is.
      </p>
      <fieldset className="flex min-w-0 flex-col gap-1.5">
        <legend className="mb-1 font-medium text-xs">Agents</legend>
        {offer.catalogue.map((agent) => (
          <Label key={agent.id} className="min-w-0 font-normal text-xs">
            <Checkbox
              checked={picked.includes(agent.id)}
              disabled={busy}
              onCheckedChange={(next) => {
                setPicked((current) =>
                  next === true
                    ? [...current, agent.id]
                    : current.filter((entry) => entry !== agent.id),
                );
                edited();
              }}
            />
            <span className="[overflow-wrap:anywhere]">
              {agent.name}{" "}
              <span className="text-muted-foreground">
                {agent.kind}, written for {agent.homeDrive}
              </span>
            </span>
          </Label>
        ))}
      </fieldset>
      {folder.declared ? (
        <div className="flex min-w-0 flex-col gap-1">
          <p className="text-xs">
            {folder.name}'s _drive.toml names the drive and who reads it, and the seed leaves it as
            it is:
          </p>
          <p className="text-xs [overflow-wrap:anywhere]">Drive: {folder.drive}</p>
          <People
            owner={{ matrixId: folder.owner, displayName: null }}
            readers={folder.readers.map((reader) => ({ matrixId: reader, displayName: null }))}
            localOnly={folder.localOnly}
          />
        </div>
      ) : (
        <>
          <div className="flex min-w-0 flex-col gap-1">
            <Label htmlFor={`${id}-drive`} className="text-xs">
              Drive id
            </Label>
            <Input
              id={`${id}-drive`}
              value={drive}
              disabled={busy}
              onChange={(event) => {
                setDrive(event.target.value);
                edited();
              }}
            />
          </div>
          <div className="flex min-w-0 flex-col gap-1.5">
            <p id={`${id}-owner`} className="font-medium text-xs">
              Owner
            </p>
            <RadioGroup
              aria-labelledby={`${id}-owner`}
              value={owner}
              onValueChange={(next) => {
                setOwner(next);
                edited();
              }}
              className="gap-1.5"
            >
              {owners.map((account, index) => (
                <div key={account} className="flex min-w-0 items-center gap-2">
                  <RadioGroupItem id={`${id}-owner-${index}`} value={account} disabled={busy} />
                  <Label
                    htmlFor={`${id}-owner-${index}`}
                    className="font-normal text-xs [overflow-wrap:anywhere]"
                  >
                    {account}
                  </Label>
                </div>
              ))}
            </RadioGroup>
          </div>
          <div className="flex min-w-0 flex-col gap-1">
            <Label htmlFor={`${id}-readers`} className="text-xs">
              Readers, one Matrix id per line
            </Label>
            <Textarea
              id={`${id}-readers`}
              value={readers}
              disabled={busy}
              spellCheck={false}
              onChange={(event) => {
                setReaders(event.target.value);
                edited();
              }}
            />
          </div>
          <Label className="min-w-0 font-normal text-xs">
            <Checkbox
              checked={localOnly}
              disabled={busy}
              onCheckedChange={(next) => {
                setLocalOnly(next === true);
                edited();
              }}
            />
            <span className="[overflow-wrap:anywhere]">
              Local models only: the agents may run only on a model on your own machines
            </span>
          </Label>
        </>
      )}
      <div className="flex min-w-0 flex-col gap-1.5">
        <p id={`${id}-bot`} className="font-medium text-xs">
          Bot the agents run on
        </p>
        {offer.bots.length === 0 ? (
          <p className="text-muted-foreground text-xs">{SEED_NO_BOTS}</p>
        ) : (
          <>
            <p className="text-muted-foreground text-xs">
              Nothing is picked for you: the agents run on the bot you choose.
            </p>
            <RadioGroup
              aria-labelledby={`${id}-bot`}
              value={bot ?? ""}
              onValueChange={(next) => {
                setBot(next);
                edited();
              }}
              className="gap-1.5"
            >
              {offer.bots.map((candidate, index) => (
                <div key={candidate.reference} className="flex min-w-0 items-center gap-2">
                  <RadioGroupItem
                    id={`${id}-bot-${index}`}
                    value={candidate.reference}
                    disabled={busy}
                  />
                  <Label
                    htmlFor={`${id}-bot-${index}`}
                    className="font-normal text-xs [overflow-wrap:anywhere]"
                  >
                    {candidate.name}{" "}
                    <span className="text-muted-foreground">{candidate.provider}</span>
                  </Label>
                </div>
              ))}
            </RadioGroup>
          </>
        )}
      </div>
      <div className="flex min-w-0 flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void preview()}>
          {SEED_PREVIEW_LABEL}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={close}>
          Cancel
        </Button>
      </div>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
      {plan !== null && (
        <div className="flex min-w-0 flex-col gap-2">
          {plan.write.length > 0 ? (
            <FileList title="Files to write" files={plan.write} />
          ) : (
            <p className="text-xs">Every file is there already, so there is nothing to write.</p>
          )}
          {plan.left.length > 0 && <FileList title="Files left as they are" files={plan.left} />}
          {plan.write.length > 0 && (
            <div>
              <Button size="sm" disabled={busy} onClick={() => void write()}>
                {SEED_WRITE_LABEL}
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

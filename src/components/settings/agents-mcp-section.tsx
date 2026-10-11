/**
 * Settings → Agents → *MCP servers* and *Sandbox* (Story 96.2, UX-DR140; R213):
 * this Mac's own servers and `[sandbox]` table, kept in `keeper.db` on this Mac
 * only — what `agentd.toml` says on a Linux host.
 *
 * Everything shown is Rust's: whether a server answers and why not, the tier of
 * each tool it lists as the cards name it, a program's floor, a role's fixed
 * table, and every refusal, drawn verbatim. This file checks nothing — Rust
 * checks a server with the one `[[mcp]]` check agentd's go through, and the
 * table with the one `[sandbox]` check. The sheet's tools are Rust's for the
 * entry as it is being edited (`agentsMcpDraft`): each with the tier a save of
 * it would give, under the role, rows and trust chosen in the sheet. Until Rust
 * has answered for the inputs on screen — or when it could not — no tier, row
 * or Save of the sheet is offered as theirs. The tier rows a role does not take
 * are Rust's too: the sheet shows them and drops them only when the person says
 * so, so a role chosen over rows is never saved with rows hidden. Every name
 * and refusal is shown — accessible names included — as Rust redacted and
 * bounded it; a tool's exact name only picks its row, and a name that cannot
 * travel has none.
 *
 * **A program's argv is kept as written.** One field per argument; nothing is
 * trimmed, split or dropped, so an empty argument, spaces and a line break
 * inside one are saved as they are. Once it answers, a program server is also
 * shown as the host started it — the absolute program its argv resolved to and
 * that file's SHA-256, what an approval of its tools binds.
 *
 * **Kept current.** What a server answered and what the sandbox's probe found
 * change on the host's schedule, so both sections read again every few seconds
 * while open — the open sheet's status included — without touching what the
 * person is typing. Rust shows a status only beside the settings it was
 * checked on: after a save, the old answer is not shown.
 *
 * **The token is write-only.** The sheet never shows one: it says the server is
 * saved with a token, takes a new one, or forgets it — and *Forget the token*
 * wins over a token typed in the same sheet.
 *
 * **Only the person vouches.** *Trust this server's annotations* is off unless
 * ticked here, and a tool's tier row is set only here (S-14).
 */
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Textarea } from "@/components/ui/textarea";
import {
  type AgentMcpDraftVm,
  type AgentMcpListVm,
  type AgentMcpRowVm,
  type AgentMcpServerReq,
  type AgentMcpServerVm,
  type AgentMcpTierVm,
  type AgentSandboxEnvVm,
  type AgentSandboxVm,
  agentsMcpDraft,
  agentsMcpList,
  agentsMcpRemove,
  agentsMcpSave,
  agentsSandboxGet,
  agentsSandboxSave,
} from "@/lib/ipc/client";

export const MCP_SECTION_TITLE = "MCP servers";
export const MCP_SECTION_NOTE =
  "Servers your agents on this Mac may use, when an agent's agent.toml names them in [tools].mcp. Kept on this Mac only.";
export const MCP_EMPTY = "No MCP server on this Mac yet.";
export const MCP_ADD_LABEL = "Add a server";
export const MCP_SAVE_LABEL = "Save server";
export const MCP_TRUST_LABEL = "Trust this server's annotations";
export const MCP_TRUST_NOTE =
  "A server describes its own tools. Trust that description only if you trust the server.";
export const MCP_NO_ROW = "Keeper's rule";
export const MCP_DROP_ROWS_LABEL = "Drop these rows";
export const MCP_FORGET_LABEL = "Forget the token";
export const MCP_ADD_ARGUMENT_LABEL = "Add an argument";
export const MCP_STARTED_LABEL = "Started as";
export const SANDBOX_SECTION_TITLE = "Sandbox";
export const SANDBOX_SECTION_NOTE =
  "Folders a command an agent runs on this Mac may read and run, and variables naming folders, for toolchains installed outside the system's folders.";
export const SANDBOX_SAVE_LABEL = "Save sandbox";

/** How often the open sections read again: the host asks each server every minute. */
export const MCP_RELOAD_MS = 5000;

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

const selectClass =
  "h-8 min-w-0 max-w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function AgentsMcpSection({ open }: { open: boolean }) {
  const [list, setList] = useState<AgentMcpListVm | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<AgentMcpServerVm | "new" | null>(null);
  const id = useId();

  const reload = useCallback(async () => {
    try {
      setList(await agentsMcpList());
      setError(null);
    } catch (failure) {
      setError(messageOf(failure));
    }
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }
    void reload();
    const timer = setInterval(() => void reload(), MCP_RELOAD_MS);
    return () => clearInterval(timer);
  }, [open, reload]);

  const remove = async (name: string) => {
    try {
      setList(await agentsMcpRemove(name));
      setError(null);
    } catch (failure) {
      setError(messageOf(failure));
    }
  };

  // The sheet's form starts from the server as it was when Edit was clicked;
  // its status and tools are the newest list's.
  const live =
    editing === null || editing === "new"
      ? null
      : (list?.servers.find((server) => server.name === editing.name) ?? null);

  return (
    <section aria-labelledby={`${id}-title`} className="flex min-w-0 flex-col gap-2 border-t pt-2">
      <h4 id={`${id}-title`} className="font-medium">
        {MCP_SECTION_TITLE}
      </h4>
      <p className="text-muted-foreground text-xs">{MCP_SECTION_NOTE}</p>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
      {list !== undefined && list.servers.length === 0 && (
        <p className="text-muted-foreground text-xs">{MCP_EMPTY}</p>
      )}
      {list !== undefined && list.servers.length > 0 && (
        <ul aria-label={MCP_SECTION_TITLE} className="flex min-w-0 flex-col gap-2">
          {list.servers.map((server) => (
            <ServerRow
              key={server.name}
              server={server}
              onEdit={() => setEditing(server)}
              onRemove={() => void remove(server.name)}
            />
          ))}
        </ul>
      )}
      <div>
        <Button size="sm" variant="outline" onClick={() => setEditing("new")}>
          {MCP_ADD_LABEL}
        </Button>
      </div>
      <Sheet open={editing !== null} onOpenChange={(next) => !next && setEditing(null)}>
        <SheetContent className="gap-0 overflow-y-auto p-0 data-[side=right]:w-full data-[side=right]:sm:max-w-[560px]">
          {editing !== null && list !== undefined && (
            <ServerSheet
              server={editing === "new" ? null : editing}
              live={live}
              tiers={list.tiers}
              onSaved={(saved) => {
                setList(saved);
                setEditing(null);
              }}
            />
          )}
        </SheetContent>
      </Sheet>
    </section>
  );
}

function ServerRow({
  server,
  onEdit,
  onRemove,
}: {
  server: AgentMcpServerVm;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const readers = server.anyone
    ? "Read by anyone"
    : `Read by ${server.readers
        .map((reader) =>
          reader.displayName === null
            ? reader.matrixId
            : `${reader.displayName} (${reader.matrixId})`,
        )
        .join(", ")}`;
  return (
    <li
      aria-label={server.name}
      className="flex min-w-0 flex-col gap-1 rounded-md border p-2 text-xs"
    >
      <p className="font-medium text-sm [overflow-wrap:anywhere]">{server.name}</p>
      <p className="font-mono [overflow-wrap:anywhere]">{server.url ?? server.command.join(" ")}</p>
      {server.started !== null && (
        <p className="[overflow-wrap:anywhere]">
          {MCP_STARTED_LABEL} <span className="font-mono">{server.started.path}</span>, SHA-256{" "}
          <span className="font-mono">{server.started.sha256}</span>
        </p>
      )}
      {server.role !== null && <p>Role: {server.role}</p>}
      <p className="[overflow-wrap:anywhere]">{readers}</p>
      <p
        role="status"
        className={
          server.answers
            ? "[overflow-wrap:anywhere]"
            : "text-muted-foreground [overflow-wrap:anywhere]"
        }
      >
        {server.answer}
      </p>
      {server.refusal !== null && (
        <p className="text-destructive [overflow-wrap:anywhere]">{server.refusal}</p>
      )}
      <div className="flex min-w-0 flex-wrap gap-2">
        <Button size="sm" variant="outline" onClick={onEdit}>
          Edit {server.name}
        </Button>
        <Button size="sm" variant="ghost" onClick={onRemove}>
          Remove {server.name}
        </Button>
      </div>
    </li>
  );
}

/** Rust's answer to the sheet's latest ask, and the inputs (`key`) it was asked for. */
type DraftAnswer = { key: string; draft: AgentMcpDraftVm | null; failure: string | null };

function ServerSheet({
  server,
  live,
  tiers,
  onSaved,
}: {
  /** The server as it was when Edit was clicked; `null` adds one. */
  server: AgentMcpServerVm | null;
  /** The same server in the newest list: what it answers now. */
  live: AgentMcpServerVm | null;
  tiers: AgentMcpTierVm[];
  onSaved: (list: AgentMcpListVm) => void;
}) {
  const id = useId();
  const [name, setName] = useState(server?.name ?? "");
  const [kind, setKind] = useState<"url" | "command">(
    server !== null && server.url === null ? "command" : "url",
  );
  const [url, setUrl] = useState(server?.url ?? "");
  const [argv, setArgv] = useState<string[]>(
    server !== null && server.command.length > 0 ? server.command : [""],
  );
  const [token, setToken] = useState("");
  const [forgetToken, setForgetToken] = useState(false);
  const [readers, setReaders] = useState(
    (server?.readers ?? []).map((reader) => reader.matrixId).join("\n"),
  );
  const [role, setRole] = useState(server?.role ?? "");
  const [fingerprint, setFingerprint] = useState(server?.fingerprint ?? "");
  const [trust, setTrust] = useState(server?.trustAnnotations ?? false);
  const [rows, setRows] = useState<AgentMcpRowVm[]>(server?.rows ?? []);
  const [newTool, setNewTool] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useMemo<AgentMcpServerReq>(
    () => ({
      name,
      url: kind === "url" ? url : null,
      command: kind === "command" ? argv : [],
      role: role === "" ? null : role,
      fingerprint: null,
      readers: [],
      trustAnnotations: trust,
      rows,
      token: null,
      forgetToken: false,
    }),
    [name, kind, url, argv, role, trust, rows],
  );
  const key = JSON.stringify(request);
  const [answer, setAnswer] = useState<DraftAnswer>({ key: "", draft: null, failure: null });
  const asked = useRef(0);

  const setRow = (tool: string, tier: string) => {
    const others = rows.filter((row) => row.tool !== tool);
    setRows(tier === "" ? others : [...others, { tool, tier }]);
  };

  // Rust's sheet for the entry as it now stands, asked again as the person
  // edits and as the host's answers change. An answer — a sheet or a failure
  // — overtaken by a later ask is dropped, so an older failure never lands
  // over a newer sheet and a newer sheet replaces the failure before it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: each newer list (`live`) is the host's newer answer, which Rust reads
  useEffect(() => {
    const ask = ++asked.current;
    const asking = key;
    agentsMcpDraft(request).then(
      (draft) => {
        if (ask === asked.current) {
          setAnswer({ key: asking, draft, failure: null });
        }
      },
      (failure: unknown) => {
        if (ask === asked.current) {
          setAnswer({ key: asking, draft: null, failure: messageOf(failure) });
        }
      },
    );
  }, [request, key, live]);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      onSaved(
        await agentsMcpSave({
          name,
          url: kind === "url" ? url : null,
          command: kind === "command" ? argv : [],
          role: role === "" ? null : role,
          fingerprint: fingerprint === "" ? null : fingerprint,
          readers: readers
            .split("\n")
            .map((line) => line.trim())
            .filter((line) => line !== ""),
          trustAnnotations: trust,
          rows,
          token: kind === "url" && !forgetToken && token !== "" ? token : null,
          forgetToken,
        }),
      );
    } catch (failure) {
      setError(messageOf(failure));
    } finally {
      setBusy(false);
    }
  };

  // Only an answer for the inputs on screen is the sheet's. Until they have
  // one, the last sheet stays in view, inert — none of its controls is the
  // entry's and nothing is saved; a failure shows no sheet at all.
  const settled = answer.key === key ? answer : null;
  const ready = settled?.draft ?? null;
  const presentation = ready ?? (settled === null ? answer.draft : null);
  const inert = ready === null;
  const editable = presentation !== null && presentation.fixed === null;
  return (
    <form
      aria-label={server === null ? MCP_ADD_LABEL : `Edit ${server.name}`}
      className="flex min-w-0 flex-col gap-3 p-4 text-sm"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <SheetHeader className="gap-1 p-0 pr-10">
        <SheetTitle>{server === null ? MCP_ADD_LABEL : server.name}</SheetTitle>
        <SheetDescription>{MCP_SECTION_NOTE}</SheetDescription>
      </SheetHeader>
      <div className="flex min-w-0 flex-col gap-1">
        <Label htmlFor={`${id}-name`}>Name</Label>
        <Input
          id={`${id}-name`}
          value={name}
          disabled={busy || server !== null}
          onChange={(event) => setName(event.target.value)}
        />
      </div>
      <RadioGroup
        aria-label="How keeper reaches it"
        value={kind}
        onValueChange={(value) => setKind(value as "url" | "command")}
        className="flex flex-wrap gap-4"
      >
        <div className="flex items-center gap-2">
          <RadioGroupItem id={`${id}-url-kind`} value="url" />
          <Label htmlFor={`${id}-url-kind`}>A URL</Label>
        </div>
        <div className="flex items-center gap-2">
          <RadioGroupItem id={`${id}-command-kind`} value="command" />
          <Label htmlFor={`${id}-command-kind`}>A program keeper starts</Label>
        </div>
      </RadioGroup>
      {kind === "url" ? (
        <>
          <div className="flex min-w-0 flex-col gap-1">
            <Label htmlFor={`${id}-url`}>URL</Label>
            <Input
              id={`${id}-url`}
              value={url}
              disabled={busy}
              onChange={(event) => setUrl(event.target.value)}
            />
          </div>
          <div className="flex min-w-0 flex-col gap-1">
            <Label htmlFor={`${id}-token`}>Bearer token</Label>
            <Input
              id={`${id}-token`}
              type="password"
              autoComplete="off"
              placeholder={server?.token ? "Saved with a token" : "None"}
              value={token}
              disabled={busy || forgetToken}
              onChange={(event) => setToken(event.target.value)}
            />
            {server?.token && (
              <div className="flex items-center gap-2">
                <Checkbox
                  id={`${id}-forget`}
                  checked={forgetToken}
                  onCheckedChange={(checked) => setForgetToken(checked === true)}
                />
                <Label htmlFor={`${id}-forget`} className="text-xs">
                  {MCP_FORGET_LABEL}
                </Label>
              </div>
            )}
          </div>
          <div className="flex min-w-0 flex-col gap-1">
            <Label htmlFor={`${id}-fingerprint`}>Certificate fingerprint (optional)</Label>
            <Input
              id={`${id}-fingerprint`}
              value={fingerprint}
              disabled={busy}
              onChange={(event) => setFingerprint(event.target.value)}
            />
          </div>
        </>
      ) : (
        <fieldset className="flex min-w-0 flex-col gap-1">
          <legend className="font-medium text-xs">The program, then each argument</legend>
          {argv.map((arg, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: arguments are edited in place by position
            <div key={index} className="flex min-w-0 items-start gap-2">
              <Textarea
                aria-label={index === 0 ? "Program" : `Argument ${index}`}
                rows={1}
                className="min-h-8 min-w-0 flex-1 font-mono text-xs"
                value={arg}
                disabled={busy}
                onChange={(event) =>
                  setArgv(argv.map((a, i) => (i === index ? event.target.value : a)))
                }
              />
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={busy || argv.length === 1}
                onClick={() => setArgv(argv.filter((_, i) => i !== index))}
              >
                Remove
              </Button>
            </div>
          ))}
          <div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => setArgv([...argv, ""])}
            >
              {MCP_ADD_ARGUMENT_LABEL}
            </Button>
          </div>
        </fieldset>
      )}
      <div className="flex min-w-0 flex-col gap-1">
        <Label htmlFor={`${id}-readers`}>
          Who reads what reaches it, one Matrix id per line (empty: anyone)
        </Label>
        <Textarea
          id={`${id}-readers`}
          value={readers}
          disabled={busy}
          onChange={(event) => setReaders(event.target.value)}
        />
      </div>
      <div className="flex min-w-0 flex-col gap-1">
        <Label htmlFor={`${id}-role`}>Role</Label>
        <select
          id={`${id}-role`}
          className={selectClass}
          value={role}
          disabled={busy}
          onChange={(event) => setRole(event.target.value)}
        >
          <option value="">None</option>
          <option value="paseo">paseo</option>
          <option value="screen">screen</option>
          {role !== "" && role !== "paseo" && role !== "screen" && (
            <option value={role}>{role}</option>
          )}
        </select>
      </div>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex items-center gap-2">
          <Checkbox
            id={`${id}-trust`}
            checked={trust}
            onCheckedChange={(checked) => setTrust(checked === true)}
          />
          <Label htmlFor={`${id}-trust`}>{MCP_TRUST_LABEL}</Label>
        </div>
        <p className="text-muted-foreground text-xs">{MCP_TRUST_NOTE}</p>
      </div>
      <fieldset aria-busy={settled === null} className="flex min-w-0 flex-col gap-2">
        <legend className="font-medium text-xs">Tools and their tiers</legend>
        {live !== null && !live.answers && (
          <p role="status" className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
            {live.answer}
          </p>
        )}
        {settled !== null && settled.failure !== null && (
          <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
            {settled.failure}
          </p>
        )}
        {presentation !== null && (
          <div
            className={
              inert ? "flex min-w-0 flex-col gap-2 opacity-60" : "flex min-w-0 flex-col gap-2"
            }
          >
            {presentation.fixed !== null && <p className="text-xs">{presentation.fixed}</p>}
            {presentation.floor !== null && <p className="text-xs">{presentation.floor}</p>}
            <ul className="flex min-w-0 flex-col gap-2">
              {presentation.tools.map((tool, index) => {
                const key = tool.tool;
                const row = rows.find((r) => r.tool === key);
                return (
                  <li
                    key={key ?? `unnamed-${index}`}
                    className="flex min-w-0 flex-col gap-1 text-xs"
                  >
                    <span className="font-mono [overflow-wrap:anywhere]">{tool.shown}</span>
                    {tool.word !== null && <span>{tool.word}</span>}
                    {tool.refusal !== null && (
                      <span className="text-muted-foreground [overflow-wrap:anywhere]">
                        {tool.refusal}
                      </span>
                    )}
                    {editable && key !== null && (
                      <select
                        aria-label={`Tier of ${tool.shown}`}
                        className={selectClass}
                        value={row?.tier ?? ""}
                        disabled={busy || inert}
                        onChange={(event) => setRow(key, event.target.value)}
                      >
                        <option value="">{MCP_NO_ROW}</option>
                        {tiers.map((tier) => (
                          <option key={tier.code} value={tier.code}>
                            {tier.word}
                          </option>
                        ))}
                      </select>
                    )}
                  </li>
                );
              })}
            </ul>
            {presentation.drop !== null && (
              <div className="flex min-w-0 flex-col gap-1 rounded-md border p-2 text-xs">
                <p className="[overflow-wrap:anywhere]">{presentation.drop}</p>
                <ul className="flex min-w-0 flex-col gap-1">
                  {presentation.conflicts.map((conflict) => (
                    <li key={conflict.tool} className="[overflow-wrap:anywhere]">
                      <span className="font-mono">{conflict.shown}</span>:{" "}
                      {tiers.find((tier) => tier.code === conflict.tier)?.word ?? conflict.tier}
                    </li>
                  ))}
                </ul>
                <div>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={busy || inert}
                    onClick={() =>
                      setRows(
                        rows.filter(
                          (r) =>
                            !presentation.conflicts.some((conflict) => conflict.tool === r.tool),
                        ),
                      )
                    }
                  >
                    {MCP_DROP_ROWS_LABEL}
                  </Button>
                </div>
              </div>
            )}
            {editable && (
              <div className="flex min-w-0 flex-wrap items-end gap-2">
                <div className="flex min-w-0 flex-1 flex-col gap-1">
                  <Label htmlFor={`${id}-tool`} className="text-xs">
                    A tool it lists
                  </Label>
                  <Input
                    id={`${id}-tool`}
                    value={newTool}
                    disabled={busy || inert}
                    onChange={(event) => setNewTool(event.target.value)}
                  />
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy || inert || newTool.trim() === ""}
                  onClick={() => {
                    setRow(newTool.trim(), "T3");
                    setNewTool("");
                  }}
                >
                  Add a row
                </Button>
              </div>
            )}
          </div>
        )}
      </fieldset>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
      <div>
        <Button type="submit" size="sm" disabled={busy || ready === null}>
          {MCP_SAVE_LABEL}
        </Button>
      </div>
    </form>
  );
}

export function AgentsSandboxSection({ open }: { open: boolean }) {
  const [table, setTable] = useState<AgentSandboxVm | undefined>(undefined);
  const [readExec, setReadExec] = useState<string[]>([]);
  const [env, setEnv] = useState<AgentSandboxEnvVm[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const id = useId();

  const show = useCallback((read: AgentSandboxVm) => {
    setTable(read);
    setReadExec(read.readExec);
    setEnv(read.env);
  }, []);

  // The form is read once; the probe's line and the stored table's refusal
  // are read again while open, leaving what the person types alone.
  useEffect(() => {
    if (!open) {
      return;
    }
    agentsSandboxGet()
      .then(show)
      .catch((failure: unknown) => setError(messageOf(failure)));
    const timer = setInterval(() => {
      agentsSandboxGet()
        .then(setTable)
        .catch((failure: unknown) => setError(messageOf(failure)));
    }, MCP_RELOAD_MS);
    return () => clearInterval(timer);
  }, [open, show]);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      show(await agentsSandboxSave({ readExec, env }));
    } catch (failure) {
      setError(messageOf(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby={`${id}-title`} className="flex min-w-0 flex-col gap-2 border-t pt-2">
      <h4 id={`${id}-title`} className="font-medium">
        {SANDBOX_SECTION_TITLE}
      </h4>
      <p className="text-muted-foreground text-xs">{SANDBOX_SECTION_NOTE}</p>
      {table !== undefined && (
        <p role="status" className="text-xs [overflow-wrap:anywhere]">
          {table.status}
        </p>
      )}
      {table?.refusal != null && (
        <p className="text-destructive text-xs [overflow-wrap:anywhere]">{table.refusal}</p>
      )}
      <form
        aria-label={SANDBOX_SECTION_TITLE}
        className="flex min-w-0 flex-col gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <fieldset className="flex min-w-0 flex-col gap-1">
          <legend className="font-medium text-xs">Folders a command may read and run</legend>
          {readExec.map((path, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are edited in place by position
            <div key={index} className="flex min-w-0 items-center gap-2">
              <Input
                aria-label={`Folder ${index + 1}`}
                className="min-w-0 flex-1 font-mono text-xs"
                value={path}
                disabled={busy}
                onChange={(event) =>
                  setReadExec(readExec.map((p, i) => (i === index ? event.target.value : p)))
                }
              />
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() => setReadExec(readExec.filter((_, i) => i !== index))}
              >
                Remove
              </Button>
            </div>
          ))}
          <div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => setReadExec([...readExec, ""])}
            >
              Add a folder
            </Button>
          </div>
        </fieldset>
        <fieldset className="flex min-w-0 flex-col gap-1">
          <legend className="font-medium text-xs">Variables</legend>
          {env.map((row, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are edited in place by position
            <div key={index} className="flex min-w-0 flex-wrap items-center gap-2">
              <Input
                aria-label={`Variable ${index + 1} name`}
                className="w-36 min-w-0 font-mono text-xs"
                value={row.name}
                disabled={busy}
                onChange={(event) =>
                  setEnv(env.map((r, i) => (i === index ? { ...r, name: event.target.value } : r)))
                }
              />
              <Input
                aria-label={`Variable ${index + 1} folder`}
                className="min-w-0 flex-1 font-mono text-xs"
                value={row.path}
                disabled={busy}
                onChange={(event) =>
                  setEnv(env.map((r, i) => (i === index ? { ...r, path: event.target.value } : r)))
                }
              />
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() => setEnv(env.filter((_, i) => i !== index))}
              >
                Remove
              </Button>
            </div>
          ))}
          <div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => setEnv([...env, { name: "", path: "" }])}
            >
              Add a variable
            </Button>
          </div>
        </fieldset>
        {error !== null && (
          <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
            {error}
          </p>
        )}
        <div>
          <Button type="submit" size="sm" disabled={busy || table === undefined}>
            {SANDBOX_SAVE_LABEL}
          </Button>
        </div>
      </form>
    </section>
  );
}

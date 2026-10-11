import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AgentsMcpSection,
  AgentsSandboxSection,
  MCP_ADD_ARGUMENT_LABEL,
  MCP_ADD_LABEL,
  MCP_DROP_ROWS_LABEL,
  MCP_FORGET_LABEL,
  MCP_RELOAD_MS,
  MCP_SAVE_LABEL,
  MCP_STARTED_LABEL,
  MCP_TRUST_LABEL,
  SANDBOX_SAVE_LABEL,
} from "@/components/settings/agents-mcp-section";
import type {
  AgentMcpDraftVm,
  AgentMcpListVm,
  AgentMcpServerReq,
  AgentMcpServerVm,
  AgentMcpToolVm,
  AgentSandboxVm,
} from "@/lib/ipc/client";
import {
  agentsMcpDraft,
  agentsMcpList,
  agentsMcpRemove,
  agentsMcpSave,
  agentsSandboxGet,
  agentsSandboxSave,
} from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsMcpList: vi.fn(),
  agentsMcpDraft: vi.fn(),
  agentsMcpSave: vi.fn(),
  agentsMcpRemove: vi.fn(),
  agentsSandboxGet: vi.fn(),
  agentsSandboxSave: vi.fn(),
}));

const T0 = "T0: it only reads or changes what can be put back";
const T3 = "T3: it reaches beyond this session: it sends, or changes what runs";
const FLOOR = "the floor of a program keeper starts";
const ROLE = "the role's own table";
const DROP = "the rows its role does not take";
const SECRET = "tok-never-shown-9f";
/** A listed tool's exact name, which Rust shows only redacted. */
const EXACT = "exact-name-never-shown-4d";
const REDACTED = "[REDACTED github_token]";
const LOCKED = "keeper.db could not be read: database is locked";

function server(over: Partial<AgentMcpServerVm> & { name: string }): AgentMcpServerVm {
  return {
    url: null,
    command: [],
    role: null,
    fingerprint: null,
    readers: [],
    anyone: true,
    trustAnnotations: false,
    token: false,
    rows: [],
    floor: null,
    fixed: null,
    answers: false,
    answer: "does not answer — not asked",
    started: null,
    refusal: null,
    ...over,
  };
}

const NOTES = server({
  name: "notes",
  url: "https://notes.example.org/mcp",
  token: true,
  answers: true,
  answer: "answers",
});
const KID = server({
  name: "kid",
  command: ["/opt/bin/kid", "--stdio"],
  readers: [{ matrixId: "@marta:tgorka.org", displayName: "Marta" }],
  anyone: false,
  rows: [{ tool: "echo", tier: "T0" }],
  floor: FLOOR,
  answer: "does not answer — it did not answer within 10 s",
});
const PASEO = server({
  name: "paseo",
  url: "http://broker:8765/mcp",
  role: "paseo",
  fixed: ROLE,
  answers: true,
  answer: "answers",
});

function list(servers: AgentMcpServerVm[]): AgentMcpListVm {
  return {
    servers,
    tiers: [0, 1, 2, 3, 4, 5].map((n) => ({
      code: `T${n}`,
      word: n === 0 ? T0 : n === 3 ? T3 : `T${n}: word`,
    })),
  };
}

function tool(name: string, word: string): AgentMcpToolVm {
  return { tool: name, shown: name, word, refusal: null };
}

/** What the host heard listed where each server is reached. */
let heard: Record<string, AgentMcpToolVm[]> = {};

/**
 * Rust's sheet for a draft, as `mac_tables::draft` shapes it: the tools heard
 * where the draft reaches, then its rows; a role's rows its conflicts. The
 * tiers themselves are Rust's, proved in Rust.
 */
function rustDraft(req: AgentMcpServerReq): Promise<AgentMcpDraftVm> {
  const role = req.role !== null;
  const listed = heard[req.url ?? req.command.join(" ")] ?? [];
  const rows = role
    ? []
    : req.rows
        .filter((row) => !listed.some((listedTool) => listedTool.tool === row.tool))
        .map((row) => tool(row.tool, `${row.tier}: word`));
  return Promise.resolve({
    floor: req.command.length > 0 ? FLOOR : null,
    fixed: role ? ROLE : null,
    tools: [...listed, ...rows],
    conflicts: role
      ? req.rows.map((row) => ({
          ...row,
          shown: listed.find((listedTool) => listedTool.tool === row.tool)?.shown ?? row.tool,
        }))
      : [],
    drop: role && req.rows.length > 0 ? DROP : null,
  });
}

/**
 * Every draft asked from now on, answered only when the test says: with
 * Rust's sheet, or refused with `message`. The executor form: the project's
 * `lib: ES2020` predates `Promise.withResolvers`.
 */
function heldDrafts(): { answer: () => void; refuse: (message: string) => void }[] {
  const held: { answer: () => void; refuse: (message: string) => void }[] = [];
  vi.mocked(agentsMcpDraft).mockImplementation(
    (req) =>
      new Promise((resolve, reject) => {
        held.push({
          answer: () => void rustDraft(req).then(resolve),
          refuse: (message) => reject({ code: "internal", message }),
        });
      }),
  );
  return held;
}

beforeEach(() => {
  heard = {
    [NOTES.url ?? ""]: [
      tool("search", T3),
      {
        tool: null,
        shown: "get file",
        word: null,
        refusal: "`get file` cannot travel as a function name",
      },
    ],
    [PASEO.url ?? ""]: [tool("list_agents", T0), tool("create_agent", T3)],
  };
  vi.mocked(agentsMcpList).mockReset();
  vi.mocked(agentsMcpDraft).mockReset();
  vi.mocked(agentsMcpDraft).mockImplementation(rustDraft);
  vi.mocked(agentsMcpSave).mockReset();
  vi.mocked(agentsMcpRemove).mockReset();
  vi.mocked(agentsSandboxGet).mockReset();
  vi.mocked(agentsSandboxSave).mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

function row(name: string): HTMLElement {
  return screen.getByRole("listitem", { name });
}

async function edit(name: string): Promise<HTMLElement> {
  fireEvent.click(await screen.findByRole("button", { name: `Edit ${name}` }));
  return await screen.findByRole("form", { name: `Edit ${name}` });
}

function lastSaved(): AgentMcpServerReq | undefined {
  const calls = vi.mocked(agentsMcpSave).mock.calls;
  return calls[calls.length - 1]?.[0];
}

/** Save once Rust has answered for what the sheet now holds. */
async function saveWhenAnswered(sheet: HTMLElement): Promise<void> {
  const save = within(sheet).getByRole("button", { name: MCP_SAVE_LABEL });
  await waitFor(() => expect(save).toBeEnabled());
  fireEvent.click(save);
}

/** Let `step` — a held draft's answer or refusal — land, and React draw it. */
async function landed(step: (() => void) | undefined): Promise<void> {
  await act(async () => {
    step?.();
    await new Promise((done) => setTimeout(done, 0));
  });
}

describe("AgentsMcpSection — this Mac's MCP servers (UX-DR140)", () => {
  it("lists nothing and offers to add one when this Mac has no server", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([]));
    render(<AgentsMcpSection open />);
    expect(await screen.findByRole("button", { name: MCP_ADD_LABEL })).toBeInTheDocument();
    await waitFor(() => expect(agentsMcpList).toHaveBeenCalled());
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("lists each server with how it is reached, its readers, and Rust's answer", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([KID, NOTES, PASEO]));
    render(<AgentsMcpSection open />);
    const notes = await screen.findByRole("listitem", { name: "notes" });
    expect(within(notes).getByRole("status")).toHaveTextContent(NOTES.answer);
    const kid = row("kid");
    expect(within(kid).getByText("/opt/bin/kid --stdio")).toBeInTheDocument();
    expect(within(kid).getByRole("status")).toHaveTextContent(KID.answer);
    expect(kid).toHaveTextContent("@marta:tgorka.org");
  });

  it("shows a program server as the host started it, beside its argv as written", async () => {
    const path = "/opt/homebrew/Cellar/peekaboo/3.9.8/bin/peekaboo";
    const sha256 = "9c1f0e7d4b2a8836f5e1c0b7a9d4e2f61b8c3a5d7e9f0a1b2c3d4e5f60718293";
    const peekaboo = server({
      name: "screen",
      command: ["peekaboo", "mcp"],
      role: "screen",
      fixed: ROLE,
      answers: true,
      answer: "answers",
      started: { path, sha256 },
    });
    vi.mocked(agentsMcpList).mockResolvedValue(list([KID, peekaboo]));
    render(<AgentsMcpSection open />);
    const screenRow = await screen.findByRole("listitem", { name: "screen" });
    expect(within(screenRow).getByText("peekaboo mcp")).toBeInTheDocument();
    expect(within(screenRow).getByText(path)).toBeInTheDocument();
    expect(within(screenRow).getByText(sha256)).toBeInTheDocument();
    expect(within(row("kid")).queryByText(MCP_STARTED_LABEL, { exact: false })).toBeNull();
  });

  it("adds a server through Rust; its token goes in the request and is never drawn back", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([]));
    const added = server({ name: "forge", url: "https://forge.example.org/mcp", token: true });
    vi.mocked(agentsMcpSave).mockResolvedValue(list([added]));
    render(<AgentsMcpSection open />);
    fireEvent.click(await screen.findByRole("button", { name: MCP_ADD_LABEL }));
    const sheet = await screen.findByRole("form", { name: MCP_ADD_LABEL });
    // Off unless the person ticks it.
    expect(within(sheet).getByRole("checkbox", { name: MCP_TRUST_LABEL })).not.toBeChecked();
    fireEvent.change(within(sheet).getByLabelText("Name"), { target: { value: "forge" } });
    fireEvent.change(within(sheet).getByLabelText("URL"), {
      target: { value: "https://forge.example.org/mcp" },
    });
    fireEvent.change(within(sheet).getByLabelText("Bearer token"), { target: { value: SECRET } });
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    expect(lastSaved()).toEqual({
      name: "forge",
      url: "https://forge.example.org/mcp",
      command: [],
      role: null,
      fingerprint: null,
      readers: [],
      trustAnnotations: false,
      rows: [],
      token: SECRET,
      forgetToken: false,
    });
    // Rust is asked for the draft's presentation, never with the token.
    for (const [draft] of vi.mocked(agentsMcpDraft).mock.calls) {
      expect(draft.token).toBeNull();
    }
    expect(await screen.findByRole("listitem", { name: "forge" })).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: MCP_ADD_LABEL })).toBeNull();
    const again = await edit("forge");
    expect(within(again).getByLabelText("Bearer token")).toHaveValue("");
    expect(document.body.textContent).not.toContain(SECRET);
  });

  it("forgets the token when Forget is ticked after a new token was typed", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([NOTES]));
    vi.mocked(agentsMcpSave).mockResolvedValue(list([{ ...NOTES, token: false }]));
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    fireEvent.change(within(sheet).getByLabelText("Bearer token"), { target: { value: SECRET } });
    fireEvent.click(within(sheet).getByRole("checkbox", { name: MCP_FORGET_LABEL }));
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    expect(lastSaved()?.forgetToken).toBe(true);
    expect(lastSaved()?.token).toBeNull();
  });

  it("draws Rust's refusal verbatim and keeps the sheet open", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([]));
    const sentence = '[[mcp]] "x" is refused: it names exactly one of `url` and `command`';
    vi.mocked(agentsMcpSave).mockRejectedValue({ code: "internal", message: sentence });
    render(<AgentsMcpSection open />);
    fireEvent.click(await screen.findByRole("button", { name: MCP_ADD_LABEL }));
    const sheet = await screen.findByRole("form", { name: MCP_ADD_LABEL });
    fireEvent.change(within(sheet).getByLabelText("Name"), { target: { value: "x" } });
    await saveWhenAnswered(sheet);
    expect(await within(sheet).findByRole("alert")).toHaveTextContent(sentence);
    expect(screen.getByRole("form", { name: MCP_ADD_LABEL })).toBeInTheDocument();
  });

  it("edits a program's tier rows beside its floor and sends the person's row", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([KID]));
    vi.mocked(agentsMcpSave).mockResolvedValue(list([KID]));
    render(<AgentsMcpSection open />);
    const sheet = await edit("kid");
    expect(await within(sheet).findByText(FLOOR)).toBeInTheDocument();
    const echo = await within(sheet).findByRole("combobox", { name: "Tier of echo" });
    expect(echo).toHaveValue("T0");
    fireEvent.change(echo, { target: { value: "T3" } });
    const newTool = within(sheet).getByLabelText("A tool it lists");
    await waitFor(() => expect(newTool).toBeEnabled());
    fireEvent.change(newTool, { target: { value: "wipe" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add a row" }));
    fireEvent.change(await within(sheet).findByRole("combobox", { name: "Tier of wipe" }), {
      target: { value: "T4" },
    });
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    const req = lastSaved();
    expect(req?.url).toBeNull();
    expect(req?.command).toEqual(["/opt/bin/kid", "--stdio"]);
    expect(req?.readers).toEqual(["@marta:tgorka.org"]);
    expect(req?.token).toBeNull();
    expect(req?.rows).toEqual(
      expect.arrayContaining([
        { tool: "echo", tier: "T3" },
        { tool: "wipe", tier: "T4" },
      ]),
    );
    expect(req?.rows).toHaveLength(2);
  });

  it("saves a program's argv exactly as written: an empty argument, spaces and a line break kept", async () => {
    const argv = ["/opt/bin/kid", "", "  two  ", "a\nb"];
    const odd = server({ ...KID, command: argv });
    vi.mocked(agentsMcpList).mockResolvedValue(list([odd]));
    vi.mocked(agentsMcpSave).mockResolvedValue(list([odd]));
    render(<AgentsMcpSection open />);
    const sheet = await edit("kid");
    expect(within(sheet).getByRole("textbox", { name: "Argument 3" })).toHaveValue("a\nb");
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    expect(lastSaved()?.command).toEqual(argv);

    // An argument added and typed is its own element, as typed.
    const again = await edit("kid");
    fireEvent.click(within(again).getByRole("button", { name: MCP_ADD_ARGUMENT_LABEL }));
    fireEvent.change(within(again).getByRole("textbox", { name: "Argument 4" }), {
      target: { value: " --flag value " },
    });
    await saveWhenAnswered(again);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(2));
    expect(lastSaved()?.command).toEqual([...argv, " --flag value "]);
  });

  it("shows what an answering server lists, each tool's tier as Rust names it or why it is not offered", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([NOTES]));
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    expect(await within(sheet).findByText(T3, { selector: "span" })).toBeInTheDocument();
    expect(
      within(sheet).getByText("`get file` cannot travel as a function name"),
    ).toBeInTheDocument();
    // A tool whose name cannot travel has no row to set.
    expect(within(sheet).getAllByRole("combobox", { name: /^Tier of/ })).toHaveLength(1);
  });

  it("shows a role server's tools with their tiers and lets nobody set them", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([PASEO]));
    render(<AgentsMcpSection open />);
    const paseo = await edit("paseo");
    expect(await within(paseo).findByText("create_agent")).toBeInTheDocument();
    expect(within(paseo).getByText(ROLE)).toBeInTheDocument();
    expect(within(paseo).getByText(T0, { selector: "span" })).toBeInTheDocument();
    expect(within(paseo).queryAllByRole("combobox", { name: /^Tier of/ })).toHaveLength(0);
    expect(within(paseo).queryByRole("button", { name: "Add a row" })).toBeNull();
  });

  it("names a tool's tier control and its role conflict as Rust shows it, and saves its exact name", async () => {
    heard[NOTES.url ?? ""] = [{ tool: EXACT, shown: REDACTED, word: T3, refusal: null }];
    vi.mocked(agentsMcpList).mockResolvedValue(list([NOTES]));
    vi.mocked(agentsMcpSave).mockResolvedValue(list([NOTES]));
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    fireEvent.change(await within(sheet).findByRole("combobox", { name: `Tier of ${REDACTED}` }), {
      target: { value: "T1" },
    });
    expect(document.body.innerHTML).not.toContain(EXACT);

    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "paseo" } });
    expect(await within(sheet).findByText(DROP)).toBeInTheDocument();
    // The listed tool and the row a role does not take.
    expect(within(sheet).getAllByText(REDACTED)).toHaveLength(2);
    expect(document.body.innerHTML).not.toContain(EXACT);

    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "" } });
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    expect(lastSaved()?.rows).toEqual([{ tool: EXACT, tier: "T1" }]);
  });

  it("presents nothing of a server's previous role while its new draft is unanswered or refused, and saves nothing", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([PASEO]));
    const held = heldDrafts();
    render(<AgentsMcpSection open />);
    const sheet = await edit("paseo");
    const save = within(sheet).getByRole("button", { name: MCP_SAVE_LABEL });
    const group = within(sheet).getByRole("group", { name: "Tools and their tiers" });
    expect(save).toBeDisabled();
    await landed(held[0]?.answer);
    expect(within(sheet).getByText(ROLE)).toBeInTheDocument();
    expect(save).toBeEnabled();

    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "" } });
    expect(group).toHaveAttribute("aria-busy", "true");
    expect(save).toBeDisabled();
    await landed(() => held[1]?.refuse(LOCKED));
    expect(within(sheet).getByRole("alert")).toHaveTextContent(LOCKED);
    expect(within(sheet).queryByText(ROLE)).toBeNull();
    expect(within(sheet).queryByText("list_agents")).toBeNull();
    expect(save).toBeDisabled();

    // Answered for the inputs now on screen, the sheet is theirs again.
    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "paseo" } });
    await landed(held[2]?.answer);
    expect(within(sheet).queryByRole("alert")).toBeNull();
    expect(within(sheet).getByText(ROLE)).toBeInTheDocument();
    expect(save).toBeEnabled();
  });

  it("offers no tier of the old URL's tools while a moved server's draft is unanswered or refused", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([NOTES]));
    const held = heldDrafts();
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    const save = within(sheet).getByRole("button", { name: MCP_SAVE_LABEL });
    await landed(held[0]?.answer);
    const search = within(sheet).getByRole("combobox", { name: "Tier of search" });
    expect(search).toBeEnabled();

    fireEvent.change(within(sheet).getByLabelText("URL"), {
      target: { value: "https://elsewhere.example.org/mcp" },
    });
    expect(search).toBeDisabled();
    expect(save).toBeDisabled();
    await landed(() => held[1]?.refuse(LOCKED));
    expect(within(sheet).getByRole("alert")).toHaveTextContent(LOCKED);
    expect(within(sheet).queryAllByRole("combobox", { name: /^Tier of/ })).toHaveLength(0);
    expect(within(sheet).queryByText("search")).toBeNull();
    expect(save).toBeDisabled();
  });

  it("drops a draft's refusal that a newer answer overtook", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([NOTES]));
    const held = heldDrafts();
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    const save = within(sheet).getByRole("button", { name: MCP_SAVE_LABEL });
    await landed(held[0]?.answer);
    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "paseo" } });
    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "" } });
    await landed(held[2]?.answer);
    await landed(() => held[1]?.refuse(LOCKED));
    expect(within(sheet).queryByRole("alert")).toBeNull();
    expect(within(sheet).getByRole("combobox", { name: "Tier of search" })).toBeEnabled();
    expect(save).toBeEnabled();
  });

  it("saves an ordinary server with tier rows as a role server once the person drops the rows Rust says it does not take", async () => {
    const notes = server({ ...NOTES, rows: [{ tool: "search", tier: "T0" }] });
    const fixed = '[[mcp]] "notes" [[mcp.tier]] is refused: a role\'s tiers are fixed';
    vi.mocked(agentsMcpList).mockResolvedValue(list([notes]));
    vi.mocked(agentsMcpSave).mockImplementation((req) =>
      req.role !== null && req.rows.length > 0
        ? Promise.reject({ code: "internal", message: fixed })
        : Promise.resolve(list([notes])),
    );
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    expect(await within(sheet).findByRole("combobox", { name: "Tier of search" })).toHaveValue(
      "T0",
    );
    fireEvent.change(within(sheet).getByLabelText("Role"), { target: { value: "paseo" } });
    expect(await within(sheet).findByText(DROP)).toBeInTheDocument();
    expect(within(sheet).queryAllByRole("combobox", { name: /^Tier of/ })).toHaveLength(0);

    // Saved as it stands, the rows go with it and Rust refuses them.
    await saveWhenAnswered(sheet);
    expect(await within(sheet).findByRole("alert")).toHaveTextContent(fixed);
    expect(lastSaved()?.rows).toEqual([{ tool: "search", tier: "T0" }]);

    fireEvent.click(within(sheet).getByRole("button", { name: MCP_DROP_ROWS_LABEL }));
    await waitFor(() => expect(within(sheet).queryByText(DROP)).toBeNull());
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(2));
    expect(lastSaved()).toMatchObject({ role: "paseo", rows: [] });
    await waitFor(() => expect(screen.queryByRole("form", { name: "Edit notes" })).toBeNull());
  });

  it("saves a role server made ordinary with the tier row the person sets", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([PASEO]));
    vi.mocked(agentsMcpSave).mockResolvedValue(list([PASEO]));
    render(<AgentsMcpSection open />);
    const paseo = await edit("paseo");
    fireEvent.change(within(paseo).getByLabelText("Role"), { target: { value: "" } });
    fireEvent.change(await within(paseo).findByRole("combobox", { name: "Tier of list_agents" }), {
      target: { value: "T1" },
    });
    await saveWhenAnswered(paseo);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    expect(lastSaved()).toMatchObject({
      role: null,
      url: PASEO.url,
      rows: [{ tool: "list_agents", tier: "T1" }],
    });
  });

  it("saves a URL server made a program: its argv, no URL, no token, and Rust's floor shown", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([NOTES]));
    vi.mocked(agentsMcpSave).mockResolvedValue(list([NOTES]));
    render(<AgentsMcpSection open />);
    const sheet = await edit("notes");
    fireEvent.change(within(sheet).getByLabelText("Bearer token"), { target: { value: SECRET } });
    fireEvent.click(within(sheet).getByRole("radio", { name: "A program keeper starts" }));
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Program" }), {
      target: { value: "/opt/bin/notes" },
    });
    expect(await within(sheet).findByText(FLOOR)).toBeInTheDocument();
    // What the URL listed is not the program's.
    await waitFor(() => expect(within(sheet).queryByText("search")).toBeNull());
    await saveWhenAnswered(sheet);
    await waitFor(() => expect(agentsMcpSave).toHaveBeenCalledTimes(1));
    expect(lastSaved()).toMatchObject({ url: null, command: ["/opt/bin/notes"], token: null });
  });

  it("refreshes the open sheet's tools: a server whose URL changed no longer shows the old one's tools", async () => {
    vi.useFakeTimers();
    const moved = server({
      ...NOTES,
      url: "https://elsewhere.example.org/mcp",
      answers: false,
      answer: "does not answer — asked again once the host is built",
    });
    let reads = 0;
    vi.mocked(agentsMcpList).mockImplementation(() => {
      reads += 1;
      if (reads === 1) {
        return Promise.resolve(list([NOTES]));
      }
      // The host's answer is of other settings now.
      heard = {};
      return Promise.resolve(list([moved]));
    });
    render(<AgentsMcpSection open />);
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Edit notes" }));
    await act(async () => {
      await Promise.resolve();
    });
    const sheet = screen.getByRole("form", { name: "Edit notes" });
    expect(within(sheet).getByText(T3, { selector: "span" })).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(MCP_RELOAD_MS);
    });
    expect(within(sheet).getByRole("status")).toHaveTextContent(moved.answer);
    expect(within(sheet).queryByText(T3, { selector: "span" })).toBeNull();
  });

  it("removes a server through Rust and draws the list it answers", async () => {
    vi.mocked(agentsMcpList).mockResolvedValue(list([KID, NOTES]));
    vi.mocked(agentsMcpRemove).mockResolvedValue(list([NOTES]));
    render(<AgentsMcpSection open />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove kid" }));
    await waitFor(() => expect(agentsMcpRemove).toHaveBeenCalledWith("kid"));
    await waitFor(() => expect(screen.queryByRole("listitem", { name: "kid" })).toBeNull());
    expect(row("notes")).toBeInTheDocument();
  });
});

describe("AgentsSandboxSection — this Mac's [sandbox] table (R213)", () => {
  const TABLE: AgentSandboxVm = {
    readExec: ["/opt/homebrew/bin"],
    env: [{ name: "CARGO_HOME", path: "/Users/tg/.cargo" }],
    status: "sandbox-exec ok",
    refusal: null,
  };

  it("shows the probe's line and saves the edited table through Rust", async () => {
    vi.mocked(agentsSandboxGet).mockResolvedValue(TABLE);
    vi.mocked(agentsSandboxSave).mockResolvedValue({
      ...TABLE,
      readExec: ["/opt/homebrew/bin", "/Users/tg/.cargo/bin"],
    });
    render(<AgentsSandboxSection open />);
    expect(await screen.findByRole("status")).toHaveTextContent(TABLE.status);
    fireEvent.click(screen.getByRole("button", { name: "Add a folder" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Folder 2" }), {
      target: { value: "/Users/tg/.cargo/bin" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Variable 1 folder" }), {
      target: { value: "/opt/cargo" },
    });
    fireEvent.click(screen.getByRole("button", { name: SANDBOX_SAVE_LABEL }));
    await waitFor(() =>
      expect(agentsSandboxSave).toHaveBeenCalledWith({
        readExec: ["/opt/homebrew/bin", "/Users/tg/.cargo/bin"],
        env: [{ name: "CARGO_HOME", path: "/opt/cargo" }],
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Folder 2" })).toHaveValue("/Users/tg/.cargo/bin"),
    );
  });

  it("follows the probe after a save while open, and leaves what the person is typing", async () => {
    vi.useFakeTimers();
    const pending = { ...TABLE, status: "probed again once the host is built" };
    const refused = { ...TABLE, status: "unavailable — [sandbox] cannot grant it: a drive" };
    vi.mocked(agentsSandboxGet).mockResolvedValueOnce(TABLE).mockResolvedValue(refused);
    vi.mocked(agentsSandboxSave).mockResolvedValue(pending);
    render(<AgentsSandboxSection open />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole("status")).toHaveTextContent(TABLE.status);
    fireEvent.click(screen.getByRole("button", { name: SANDBOX_SAVE_LABEL }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole("status")).toHaveTextContent(pending.status);
    fireEvent.change(screen.getByRole("textbox", { name: "Folder 1" }), {
      target: { value: "/opt/typing" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(MCP_RELOAD_MS);
    });
    expect(screen.getByRole("status")).toHaveTextContent(refused.status);
    expect(screen.getByRole("textbox", { name: "Folder 1" })).toHaveValue("/opt/typing");
  });

  it("draws Rust's refusal verbatim and keeps what the person typed", async () => {
    vi.mocked(agentsSandboxGet).mockResolvedValue(TABLE);
    const sentence = '[sandbox] `read_exec` is refused: "bin" is not an absolute path';
    vi.mocked(agentsSandboxSave).mockRejectedValue({ code: "internal", message: sentence });
    render(<AgentsSandboxSection open />);
    const folder = await screen.findByRole("textbox", { name: "Folder 1" });
    fireEvent.change(folder, { target: { value: "bin" } });
    fireEvent.click(screen.getByRole("button", { name: SANDBOX_SAVE_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent(sentence);
    expect(screen.getByRole("textbox", { name: "Folder 1" })).toHaveValue("bin");
  });
});

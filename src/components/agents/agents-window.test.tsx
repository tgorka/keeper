import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsWindow } from "@/components/agents/agents-window";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentApprovalVm, AgentSessionsBatch, AgentSessionViewVm } from "@/lib/ipc/client";
import { AGENT_PICKS_COOKIE, agentPicksCookie } from "@/lib/stores/agent-picks";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import { agentViewPrefsWriteStore } from "@/lib/stores/agent-view-prefs";
import { persistFold } from "@/lib/stores/fold-cookie";

const mock = vi.hoisted(() => ({
  list: null as ((batch: AgentSessionsBatch) => void) | null,
  views: new Map<string, (vm: AgentSessionViewVm) => void>(),
  send: vi.fn().mockResolvedValue(undefined),
  archive: vi.fn().mockResolvedValue(undefined),
  unarchive: vi.fn().mockResolvedValue(undefined),
  close: vi.fn().mockResolvedValue(undefined),
  page: vi.fn().mockResolvedValue(undefined),
  create: vi.fn().mockResolvedValue({ database: "beta", sessionId: "new.1" }),
  decide: vi.fn().mockResolvedValue({ state: "sent", sentence: "Sent" }),
  loadDraft: vi.fn(),
  prefsSet: vi.fn(),
  toolSet: vi.fn().mockResolvedValue(undefined),
  phone: false,
}));
vi.mock("@/hooks/use-shell-layout", () => ({ useShellLayout: () => ({ phone: mock.phone }) }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentsSessionsSubscribe: vi.fn(async (callback) => {
    mock.list = callback;
    return "list";
  }),
  agentsSessionsUnsubscribe: vi.fn().mockResolvedValue(undefined),
  agentSessionOpen: vi.fn(async (target, callback) => {
    mock.views.set(target.database, callback);
    return target.database;
  }),
  agentSessionClose: mock.close,
  agentSend: mock.send,
  agentSessionArchive: mock.archive,
  agentSessionUnarchive: mock.unarchive,
  agentSessionPageBack: mock.page,
  agentViewPrefsSet: mock.prefsSet,
  agentSessionToolSet: mock.toolSet,
  agentSessionNew: mock.create,
  agentSessionApprovalDecide: mock.decide,
  agentApprovalsSubscribe: vi.fn(async (callback) => {
    callback([]);
    return "approvals";
  }),
  agentApprovalsUnsubscribe: vi.fn().mockResolvedValue(undefined),
  agentsChoices: vi.fn().mockResolvedValue([
    {
      agentActor: "assistant",
      agentName: "Assistant",
      agentMark: "A",
      databases: ["alpha", "beta"],
      database: null,
      ask: true,
    },
    {
      agentActor: "researcher",
      agentName: "Researcher",
      agentMark: "R",
      databases: ["beta"],
      database: "beta",
      ask: false,
    },
  ]),
  loadDraft: mock.loadDraft,
}));
const row = (database: string, archived = false) => ({
  database,
  sessionId: "same.1",
  title: `${database} session`,
  titleWrite: { state: "confirmed" as const },
  displayName: `assistant · ${database} session`,
  handle: "jolly-pigeon-squeaks",
  createdAtMs: 1780000000000,
  trust: "trusted",
  instances: [],
  agentActor: "assistant",
  agentName: "Assistant",
  agentMark: "A",
  archived,
  lastActivityMs: 1780000000000,
  choosesDrives: false,
  attention: "working" as const,
});
function view(database: string, patch: Partial<AgentSessionViewVm> = {}): AgentSessionViewVm {
  const base: AgentSessionViewVm = {
    ...row(database),
    readOnly: false,
    hasEarlier: false,
    loadingEarlier: false,
    complete: true,
    readsTrace: true,
    untrusted: false,
    live: [],
    tools: [],
    prefs: { viewMode: "normal" },
    messages: [
      {
        id: "m1",
        role: "agent",
        authorActor: "assistant",
        authorName: "Assistant",
        body: `Message in ${database}`,
        createdAtMs: 1780000000000,
      },
    ],
    items: [],
    folds: [],
    approvals: [],
    ...patch,
  };
  // Rust's reading column, unless a test names its own: each message, then
  // each answer still being written.
  return {
    ...base,
    items: patch.items ?? [
      ...base.messages.map((m) => ({ kind: "message" as const, messageId: m.id })),
      ...base.live.map((t) => ({ kind: "liveAnswer" as const, turnId: t.turnId })),
    ],
  };
}
/** One turn's fold as Rust sends it; `patch` sets what a test is about. */
function fold(patch: Partial<Ipc.AgentTurnFoldVm> = {}): Ipc.AgentTurnFoldVm {
  return {
    turnId: "t1",
    leadInIds: [],
    reasoning: null,
    steps: [],
    failures: [],
    status: "done",
    statusLabel: "Finished",
    summary: "Worked for 3 s",
    durationMs: 3000,
    ...patch,
  };
}
function draw() {
  return render(
    <TooltipProvider>
      <AgentsWindow />
    </TooltipProvider>,
  );
}
async function list() {
  await waitFor(() => expect(mock.list).not.toBeNull());
  act(() =>
    mock.list?.({
      ops: [
        {
          op: "reset",
          rows: [
            row("alpha"),
            row("beta"),
            { ...row("alpha", true), sessionId: "archived.1", title: "Earlier work" },
          ],
        },
      ],
      total: 3,
    }),
  );
}
async function open(database: string, patch: Partial<AgentSessionViewVm> = {}) {
  fireEvent.click(
    screen.getByRole("button", { name: new RegExp(`^${row(database).displayName},`) }),
  );
  await waitFor(() => expect(mock.views.has(database)).toBe(true));
  act(() => mock.views.get(database)?.(view(database, patch)));
}
beforeEach(() => {
  vi.clearAllMocks();
  mock.list = null;
  mock.views.clear();
  mock.phone = false;
  mock.prefsSet.mockImplementation(async (patch) => ({ viewMode: "normal", ...patch }));
  agentViewPrefsWriteStore.setState({ busy: false, error: null, shadowed: false });
  agentSessionsStore.setState({
    rows: [],
    loaded: false,
    error: null,
    selected: null,
    newSession: null,
  });
});

describe("database Agents window", () => {
  it("keeps identical ids in different databases separate, including drafts and late pages", async () => {
    draw();
    await list();
    await open("alpha");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "alpha draft" },
    });
    await open("beta");
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("");
    act(() => mock.views.get("alpha")?.(view("alpha", { title: "Late alpha" })));
    expect(screen.getByText("Message in beta")).toBeInTheDocument();
    expect(screen.queryByText("Late alpha")).not.toBeInTheDocument();
    expect(mock.close).toHaveBeenCalledWith("alpha");
    expect(mock.loadDraft).not.toHaveBeenCalled();
  });
  it("filters archived rows and waits for Rust to confirm archive changes", async () => {
    draw();
    await list();
    await open("beta");
    fireEvent.click(screen.getByRole("button", { name: "Archive session" }));
    await waitFor(() =>
      expect(mock.archive).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }),
    );
    expect(screen.getByRole("button", { name: /^assistant · beta session,/ })).toBeInTheDocument();
    act(() => {
      mock.list?.({ ops: [{ op: "set", index: 1, row: row("beta", true) }], total: 3 });
      mock.views.get("beta")?.(view("beta", { archived: true, readOnly: true }));
    });
    expect(
      screen.queryByRole("button", { name: /^assistant · beta session,/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Message" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Archived" }));
    expect(screen.getByRole("button", { name: /^assistant · beta session,/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Unarchive session" }));
    await waitFor(() =>
      expect(mock.unarchive).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }),
    );
  });
  it("retains a refused message and renders waiting/sent/taken only from the stream", async () => {
    mock.send.mockRejectedValueOnce({ message: "Not open to this device", retriable: false });
    draw();
    await list();
    await open("alpha");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Keep these words" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Not open to this device");
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Keep these words");
    expect(mock.send).toHaveBeenCalledWith(
      { database: "alpha", sessionId: "same.1" },
      "Keep these words",
      [],
    );
    for (const state of ["waiting", "sent", "taken"] as const) {
      act(() =>
        mock.views.get("alpha")?.(
          view("alpha", {
            messages: [
              {
                id: "own",
                role: "person",
                authorActor: "ana",
                authorName: "Ana",
                body: "Saved locally",
                createdAtMs: 1780000000000,
                state,
              },
            ],
          }),
        ),
      );
      expect(
        within(screen.getByRole("list", { name: "Messages" })).getByText(state),
      ).toBeInTheDocument();
    }
  });
  it("moves phone focus to Back and restores the database-qualified row on Escape", async () => {
    mock.phone = true;
    draw();
    await list();
    screen.getByRole("button", { name: /^assistant · beta session,/ }).focus();
    await open("beta");
    expect(screen.getByRole("button", { name: "Back to sessions" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(screen.getByRole("button", { name: /^assistant · beta session,/ })).toHaveFocus();
    expect(screen.queryByRole("textbox", { name: "Message" })).not.toBeInTheDocument();
  });
  it("widens history once requested without replacing a database's address", async () => {
    draw();
    await list();
    await open("beta", { hasEarlier: true });
    fireEvent.click(screen.getByRole("button", { name: "Earlier messages" }));
    await waitFor(() =>
      expect(mock.page).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }),
    );
  });
});

it("requires the database choice for a multi-database agent before sending the first message", async () => {
  agentSessionsStore.setState({
    newSession: {
      agentActor: "assistant",
      agentName: "Assistant",
      database: null,
      sessionId: null,
    },
  });
  draw();
  await screen.findByRole("combobox", { name: "Database" });
  expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Database" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("option", { name: "beta" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "Begin in beta" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(mock.create).toHaveBeenCalledWith("beta", "assistant", "Begin in beta", null),
  );
  expect(agentSessionsStore.getState().selected).toEqual({ database: "beta", sessionId: "new.1" });
});

it("uses Rust's sole database without a picker", async () => {
  agentSessionsStore.setState({
    newSession: {
      agentActor: "researcher",
      agentName: "Researcher",
      database: "beta",
      sessionId: null,
    },
  });
  draw();
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled());
  expect(screen.queryByRole("combobox", { name: "Database" })).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "Research this" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(mock.create).toHaveBeenCalledWith("beta", "researcher", "Research this", null),
  );
});

it("hands this device's last pick for that agent, and only that agent's, to a new session", async () => {
  // Rust decides what of it the agent still offers (`settings::initial`).
  persistFold(
    agentPicksCookie({
      researcher: { model: "deep-model", effort: "high" },
      assistant: { model: "other-model", effort: null },
    }),
  );
  try {
    agentSessionsStore.setState({
      newSession: {
        agentActor: "researcher",
        agentName: "Researcher",
        database: "beta",
        sessionId: null,
      },
    });
    draw();
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled());
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Research this" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(mock.create).toHaveBeenCalledWith("beta", "researcher", "Research this", {
        model: "deep-model",
        effort: "high",
      }),
    );
  } finally {
    persistFold(`${AGENT_PICKS_COOKIE}=; path=/; max-age=0`);
  }
});

it("binds approval to the shown action and keeps the server's refusal visible", async () => {
  mock.decide.mockRejectedValueOnce({ message: "Decide from another host." });
  draw();
  await list();
  await open("beta", {
    approvals: [
      {
        id: "a1",
        database: "beta",
        sessionId: "same.1",
        toolName: "write",
        tier: "T4",
        summary: "Write a file",
        action: '{"path":"notes/a.md"}',
        bindingDigest: "digest-beta",
        expiresAtMs: 1800000000000,
        createdAtMs: 1780000000000,
        state: "pending",
        canDecide: true,
        sentence: "Waiting for you",
        actions: ["allowOnce", "deny"],
        declinedBySend: true,
      },
    ],
  });
  expect(screen.getByLabelText("Exact action")).toHaveTextContent("notes/a.md");
  fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("Decide from another host."),
  );
  expect(mock.decide).toHaveBeenCalledWith(
    { database: "beta", sessionId: "same.1" },
    "a1",
    "digest-beta",
    true,
  );
  expect(screen.getByRole("button", { name: "Allow once" })).toBeEnabled();
});

it("sends an ask answer through its database and waits for Rust to close its options", async () => {
  draw();
  await list();
  await open("beta", {
    messages: [
      {
        id: "ask",
        role: "agent",
        authorActor: "assistant",
        authorName: "Assistant",
        body: "Which folder?",
        createdAtMs: 1780000000000,
        ask: { options: ["Notes", "Research"], open: true },
      },
    ],
  });
  expect(screen.getByRole("complementary", { name: "Waiting for your answer" })).toHaveTextContent(
    "Which folder?",
  );
  fireEvent.click(screen.getByRole("button", { name: "Research" }));
  await waitFor(() =>
    expect(mock.send).toHaveBeenCalledWith(
      { database: "beta", sessionId: "same.1" },
      "Research",
      [],
    ),
  );
  expect(screen.getByRole("button", { name: "Research" })).toBeInTheDocument();
  act(() =>
    mock.views.get("beta")?.(
      view("beta", {
        messages: [
          {
            id: "ask",
            role: "agent",
            authorActor: "assistant",
            authorName: "Assistant",
            body: "Which folder?",
            createdAtMs: 1780000000000,
            ask: { options: ["Notes", "Research"], open: false },
          },
        ],
      }),
    ),
  );
  expect(screen.queryByRole("button", { name: "Research" })).not.toBeInTheDocument();
  expect(
    screen.queryByRole("complementary", { name: "Waiting for your answer" }),
  ).not.toBeInTheDocument();
});

it("reenables the same approval after Rust rejects a queued decision", async () => {
  mock.decide.mockResolvedValueOnce({ state: "waiting", sentence: "Queued locally" });
  const approval: AgentApprovalVm = {
    id: "queued",
    database: "alpha",
    sessionId: "same.1",
    toolName: "write",
    tier: "T3",
    summary: "Write notes",
    action: "{}",
    bindingDigest: "digest",
    expiresAtMs: 1800000000000,
    createdAtMs: 1780000000000,
    state: "pending",
    canDecide: true,
    sentence: "Waiting for you",
    actions: ["allowOnce", "deny"],
    declinedBySend: true,
  };
  const { rerender } = render(<DatabaseApprovalCard approval={approval} />);
  fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
  await waitFor(() => expect(mock.decide).toHaveBeenCalledTimes(1));
  rerender(
    <DatabaseApprovalCard
      approval={{
        ...approval,
        state: "approved",
        canDecide: false,
        sentence: "Queued locally",
        actions: [],
        declinedBySend: false,
      }}
    />,
  );
  expect(screen.queryByRole("button", { name: "Allow once" })).not.toBeInTheDocument();
  rerender(
    <DatabaseApprovalCard
      approval={{
        ...approval,
        refused: "Review it again.",
        sentence: "Decision rejected: review it again.",
      }}
    />,
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "Allow once" })).toBeEnabled());
  expect(screen.getByRole("alert")).toHaveTextContent("Decision rejected: review it again.");
});

it("keeps the first message and Rust's refusal when session creation is denied", async () => {
  mock.create.mockRejectedValueOnce({
    message: "This agent no longer serves beta.",
    retriable: false,
  });
  agentSessionsStore.setState({
    newSession: {
      agentActor: "researcher",
      agentName: "Researcher",
      database: "beta",
      sessionId: null,
    },
  });
  draw();
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled());
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "Keep the first message" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This agent no longer serves beta.");
  expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Keep the first message");
});

it("keeps phone Back focused when loading the last earlier page removes its control", async () => {
  mock.phone = true;
  draw();
  await list();
  await open("beta", { hasEarlier: true });
  const earlier = screen.getByRole("button", { name: "Earlier messages" });
  earlier.focus();
  await act(async () => {
    fireEvent.click(earlier);
  });
  act(() => mock.views.get("beta")?.(view("beta", { hasEarlier: false })));
  expect(screen.getByRole("button", { name: "Back to sessions" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  expect(screen.getByRole("button", { name: /^assistant · beta session,/ })).toHaveFocus();
});

describe("live answer controls", () => {
  it("grows the answer without replacing the draft and ends on the final row", async () => {
    draw();
    await list();
    const turn = { turnId: "t1", text: "First paragraph.", stopped: false };
    await open("alpha", { live: [turn] });
    const composer = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(composer, { target: { value: "My next question" } });
    composer.focus();
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", { live: [{ ...turn, text: "First paragraph.\n\nSecond paragraph." }] }),
      ),
    );
    expect(screen.getByText("Second paragraph.")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBe(composer);
    expect(composer).toHaveValue("My next question");
    expect(composer).toHaveFocus();
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          messages: [
            {
              id: "final",
              role: "agent",
              authorActor: "assistant",
              authorName: "Assistant",
              body: "First paragraph.\n\nSecond paragraph.",
              createdAtMs: 1780000000000,
            },
          ],
        }),
      ),
    );
    expect(screen.getAllByText("First paragraph.")).toHaveLength(1);
    expect(screen.queryByText("Answering…")).not.toBeInTheDocument();
    expect(composer).toHaveValue("My next question");
  });

  it("switches the view mode, keeps focus on it, and draws the thinking Rust then sends", async () => {
    draw();
    await list();
    const turn = { turnId: "t1", text: "An answer", stopped: false };
    const running = fold({ status: "running", statusLabel: "Working", summary: "Working…" });
    const column: Ipc.AgentTimelineItemVm[] = [
      { kind: "fold", turnId: "t1" },
      { kind: "liveAnswer", turnId: "t1" },
    ];
    await open("alpha", { live: [turn], folds: [running], items: column });
    const normal = screen.getByRole("radio", { name: "Normal" });
    const thinking = screen.getByRole("radio", { name: "Thinking" });
    expect(normal).toHaveAttribute("aria-checked", "true");
    // The executor form, not `Promise.withResolvers`: the project compiles
    // against `lib: ES2020`, where that constructor method does not exist.
    let saved: (value: Ipc.AgentViewPrefsVm) => void = () => {};
    mock.prefsSet.mockReturnValueOnce(new Promise((resolve) => (saved = resolve)));
    thinking.focus();
    fireEvent.click(thinking);
    await waitFor(() => expect(mock.prefsSet).toHaveBeenCalledWith({ viewMode: "thinking" }));
    // A browser drops focus from a control that turns disabled; jsdom does
    // not, so the write in flight must leave the mode enabled.
    expect(thinking).toBeEnabled();
    expect(thinking).toHaveFocus();
    await act(async () => saved({ viewMode: "thinking" }));
    expect(mock.prefsSet).toHaveBeenCalledTimes(1);
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          prefs: { viewMode: "thinking" },
          live: [turn],
          items: column,
          folds: [{ ...running, reasoning: "Private thought" }],
        }),
      ),
    );
    expect(thinking).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("region", { name: "Thinking" })).toHaveTextContent("Private thought");
    fireEvent.click(screen.getByRole("radio", { name: "Normal" }));
    await waitFor(() => expect(mock.prefsSet).toHaveBeenLastCalledWith({ viewMode: "normal" }));
    act(() =>
      mock.views.get("alpha")?.(view("alpha", { live: [turn], items: column, folds: [running] })),
    );
    expect(screen.getByRole("radio", { name: "Normal" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText("Private thought")).not.toBeInTheDocument();
  });

  it("follows a view mode switched elsewhere, and the next click writes the one chosen", async () => {
    // Switched on another device or in a settings file: Rust re-projects the
    // open session with the mode as it holds now.
    draw();
    await list();
    await open("alpha");
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          prefs: { viewMode: "verbose" },
          messages: [
            {
              id: "final",
              role: "agent",
              authorActor: "assistant",
              authorName: "Assistant",
              body: "Earlier answer",
              createdAtMs: 1780000000000,
              details: { model: "synced-model" },
            },
          ],
        }),
      ),
    );
    expect(screen.getByRole("radio", { name: "Verbose" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("synced-model")).toBeInTheDocument();
    act(() => mock.views.get("alpha")?.(view("alpha")));
    expect(screen.getByRole("radio", { name: "Normal" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText("synced-model")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Verbose" }));
    await waitFor(() => expect(mock.prefsSet).toHaveBeenLastCalledWith({ viewMode: "verbose" }));
  });

  it("draws the details Rust sends in the verbose mode, a measured zero and an error included", async () => {
    draw();
    await list();
    await open("alpha");
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          prefs: { viewMode: "verbose" },
          messages: [
            {
              id: "final",
              role: "agent",
              authorActor: "assistant",
              authorName: "Assistant",
              body: "Partial answer",
              createdAtMs: 1780000000000,
              details: {
                model: "example-model",
                ttftMs: 0,
                durationMs: 1240,
                finish: "error",
                error: "Provider disconnected",
              },
            },
          ],
        }),
      ),
    );
    expect(screen.getByText("example-model")).toBeInTheDocument();
    expect(screen.getByText("0 ms")).toBeInTheDocument();
    expect(screen.getByText("Provider disconnected")).toBeInTheDocument();
    expect(screen.queryByText("Total tokens")).not.toBeInTheDocument();
    expect(screen.queryByText("Prompt tokens")).not.toBeInTheDocument();
  });

  it("draws no tool switches in a session that cannot be changed here", async () => {
    draw();
    await list();
    await open("alpha", {
      archived: true,
      readOnly: true,
      tools: [{ name: "read_file", on: true }],
    });
    expect(screen.queryByRole("button", { name: "Tools" })).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders only offered switches, waits for their confirmed state, and exposes refusal", async () => {
    draw();
    await list();
    await open("alpha", { tools: [{ name: "read_file", on: true }] });
    fireEvent.click(screen.getByRole("button", { name: "Tools" }));
    const tool = screen.getByRole("switch", { name: "read_file" });
    // Held open (executor form: `lib: ES2020`) to look at the switch mid-write.
    let written: () => void = () => {};
    mock.toolSet.mockReturnValueOnce(new Promise<void>((resolve) => (written = resolve)));
    tool.focus();
    fireEvent.click(tool);
    await waitFor(() =>
      expect(mock.toolSet).toHaveBeenCalledWith(
        expect.objectContaining({ database: "alpha", sessionId: "same.1" }),
        "read_file",
        false,
      ),
    );
    expect(tool).toHaveAttribute("aria-busy", "true");
    expect(tool).toBeEnabled();
    expect(tool).toHaveFocus();
    fireEvent.click(tool);
    expect(mock.toolSet).toHaveBeenCalledTimes(1);
    await act(async () => written());
    expect(tool).toHaveAttribute("aria-checked", "true");
    act(() =>
      mock.views.get("alpha")?.(view("alpha", { tools: [{ name: "read_file", on: false }] })),
    );
    expect(tool).toHaveAttribute("aria-checked", "false");
    await waitFor(() => expect(tool).toHaveAttribute("aria-busy", "false"));
    expect(tool).toHaveFocus();
    fireEvent.click(tool);
    await waitFor(() =>
      expect(mock.toolSet).toHaveBeenLastCalledWith(
        { database: "alpha", sessionId: "same.1" },
        "read_file",
        true,
      ),
    );
    act(() =>
      mock.views.get("alpha")?.(view("alpha", { tools: [{ name: "read_file", on: true }] })),
    );
    await waitFor(() => expect(tool).toHaveAttribute("aria-busy", "false"));
    expect(tool).toHaveAttribute("aria-checked", "true");
    mock.toolSet.mockRejectedValueOnce({ message: "The definition no longer offers this tool." });
    fireEvent.click(tool);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The definition no longer offers this tool.",
    );
    expect(tool).toHaveAttribute("aria-checked", "true");
    expect(tool).toHaveFocus();
    act(() => mock.views.get("alpha")?.(view("alpha", { tools: [] })));
    expect(screen.queryByRole("button", { name: "Tools" })).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("keeps a refused view mode as it was and says when a settings file keeps it", async () => {
    draw();
    await list();
    await open("alpha");
    const verbose = screen.getByRole("radio", { name: "Verbose" });
    verbose.focus();
    mock.prefsSet.mockRejectedValueOnce({ message: "The preference could not be saved." });
    fireEvent.click(verbose);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The preference could not be saved.",
    );
    expect(screen.getByRole("radio", { name: "Normal" })).toHaveAttribute("aria-checked", "true");
    expect(verbose).toHaveFocus();
    // Saved, but what holds after is still normal: a settings file sets it.
    mock.prefsSet.mockResolvedValueOnce({ viewMode: "normal" });
    fireEvent.click(verbose);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Saved, but a settings file keeps this preference as it is.",
    );
    expect(screen.getByRole("radio", { name: "Normal" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(verbose);
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(mock.prefsSet).toHaveBeenLastCalledWith({ viewMode: "verbose" });
  });

  it("shows tool completion and a stopped answer without an endless answering state", async () => {
    draw();
    await list();
    const turn = { turnId: "t1", text: "Kept text", stopped: false };
    const column: Ipc.AgentTimelineItemVm[] = [
      { kind: "fold", turnId: "t1" },
      { kind: "liveAnswer", turnId: "t1" },
    ];
    await open("alpha", {
      untrusted: true,
      live: [turn],
      items: column,
      folds: [
        fold({
          status: "running",
          statusLabel: "Working",
          summary: "Calling read_file… · 1 step",
          steps: [{ id: "c1", name: "read_file", status: "started", statusLabel: "Started" }],
        }),
      ],
    });
    expect(screen.getByText("Untrusted")).toBeInTheDocument();
    // Said in words, not in a `title` touch and most screen readers never reach.
    expect(screen.getByText(/consequential actions need a person/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Calling read_file… · 1 step" }));
    expect(screen.getByText("read_file · Started")).toBeInTheDocument();
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          live: [{ ...turn, stopped: true }],
          items: column,
          folds: [
            fold({
              status: "interrupted",
              statusLabel: "Interrupted",
              summary: "Interrupted · called read_file 1 time",
              steps: [
                {
                  id: "c1",
                  name: "read_file",
                  status: "refused",
                  statusLabel: "Refused",
                  result: "Not allowed here",
                  endedAtMs: 1780000000000,
                },
              ],
              failures: [{ stepId: "c1", sentence: "read_file · Refused: Not allowed here" }],
            }),
          ],
        }),
      ),
    );
    expect(screen.getByText("Kept text")).toBeInTheDocument();
    expect(screen.getByText("The answer stopped.")).toBeInTheDocument();
    expect(screen.queryByText("Answering…")).not.toBeInTheDocument();
    expect(screen.getByText("read_file · Refused: Not allowed here")).toBeVisible();
    fireEvent.click(screen.getByText("read_file · Refused"));
    expect(screen.getByText("Not allowed here")).toBeVisible();
    // How the call ended, in Rust's word, never "Finished".
    expect(screen.getByText(/^Refused \d/)).toBeInTheDocument();
    expect(screen.queryByText(/^Finished /)).not.toBeInTheDocument();
  });
});

describe("the session header's instance line (106.5)", () => {
  it("names the instance Rust says holds the session and each instance's state, and follows Rust's next view", async () => {
    draw();
    await list();
    await open("alpha", {
      answering: {
        holder: "nixie.electra",
        sentence: "nixie@electra is answering.",
        instances: [
          {
            clientId: "nixie.electra",
            label: "nixie@electra",
            presence: "online",
            holds: true,
            lapsed: false,
          },
          {
            clientId: "nixie.hesperia",
            label: "nixie@hesperia",
            presence: "offline",
            holds: false,
            lapsed: false,
          },
        ],
      },
    });
    expect(screen.getByText("nixie@electra is answering.")).toBeInTheDocument();
    const line = screen.getByRole("list", { name: "Instances of Assistant" });
    const items = within(line).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "nixie@electraOnline· holds it",
      "nixie@hesperiaOffline",
    ]);
    // electra stopped renewing and its lease ran out; nobody holds the session yet.
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          answering: {
            sentence:
              "nixie@electra held this conversation; its lease ran out, so nixie@hesperia takes it over next.",
            instances: [
              {
                clientId: "nixie.electra",
                label: "nixie@electra",
                presence: "online",
                holds: false,
                lapsed: true,
              },
              {
                clientId: "nixie.hesperia",
                label: "nixie@hesperia",
                presence: "online",
                holds: false,
                lapsed: false,
              },
            ],
          },
        }),
      ),
    );
    expect(screen.queryByText(/is answering/)).toBeNull();
    expect(
      within(screen.getByRole("list", { name: "Instances of Assistant" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["nixie@electraOnline· its lease ran out", "nixie@hesperiaOnline"]);
    // The holder stopped, its lease lapsed and hesperia took the session over.
    act(() =>
      mock.views.get("alpha")?.(
        view("alpha", {
          answering: {
            holder: "nixie.hesperia",
            sentence: "nixie@hesperia is answering.",
            instances: [
              {
                clientId: "nixie.hesperia",
                label: "nixie@hesperia",
                presence: "online",
                holds: true,
                lapsed: false,
              },
              {
                clientId: "nixie.electra",
                label: "nixie@electra",
                presence: "offline",
                holds: false,
                lapsed: false,
              },
            ],
          },
        }),
      ),
    );
    expect(screen.getByText("nixie@hesperia is answering.")).toBeInTheDocument();
    expect(
      within(screen.getByRole("list", { name: "Instances of Assistant" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["nixie@hesperiaOnline· holds it", "nixie@electraOffline"]);
  });

  it("draws no instance line while Rust sends none", async () => {
    draw();
    await list();
    await open("beta");
    expect(screen.queryByRole("list", { name: "Instances of Assistant" })).not.toBeInTheDocument();
  });
});

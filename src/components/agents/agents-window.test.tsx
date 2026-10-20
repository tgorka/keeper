import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsWindow } from "@/components/agents/agents-window";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentApprovalVm, AgentSessionsBatch, AgentSessionViewVm } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

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
  agentActor: "assistant",
  agentName: "Assistant",
  agentMark: "A",
  archived,
  lastActivityMs: 1780000000000,
  attention: "working" as const,
});
function view(database: string, patch: Partial<AgentSessionViewVm> = {}): AgentSessionViewVm {
  return {
    ...row(database),
    readOnly: false,
    hasEarlier: false,
    loadingEarlier: false,
    complete: true,
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
    approvals: [],
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
  fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${database} session`) }));
  await waitFor(() => expect(mock.views.has(database)).toBe(true));
  act(() => mock.views.get(database)?.(view(database, patch)));
}
beforeEach(() => {
  vi.clearAllMocks();
  mock.list = null;
  mock.views.clear();
  mock.phone = false;
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
    expect(screen.getByRole("button", { name: /^beta session/ })).toBeInTheDocument();
    act(() => {
      mock.list?.({ ops: [{ op: "set", index: 1, row: row("beta", true) }], total: 3 });
      mock.views.get("beta")?.(view("beta", { archived: true, readOnly: true }));
    });
    expect(screen.queryByRole("button", { name: /^beta session/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Message" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Archived" }));
    expect(screen.getByRole("button", { name: /^beta session/ })).toBeInTheDocument();
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
    screen.getByRole("button", { name: /^beta session/ }).focus();
    await open("beta");
    expect(screen.getByRole("button", { name: "Back to sessions" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(screen.getByRole("button", { name: /^beta session/ })).toHaveFocus();
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
    expect(mock.create).toHaveBeenCalledWith("beta", "assistant", "Begin in beta"),
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
    expect(mock.create).toHaveBeenCalledWith("beta", "researcher", "Research this"),
  );
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
      },
    ],
  });
  expect(screen.getByLabelText("Exact action")).toHaveTextContent("notes/a.md");
  fireEvent.click(screen.getByRole("button", { name: "Approve once" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("Decide from another host."),
  );
  expect(mock.decide).toHaveBeenCalledWith(
    { database: "beta", sessionId: "same.1" },
    "a1",
    "digest-beta",
    true,
  );
  expect(screen.getByRole("button", { name: "Approve once" })).toBeEnabled();
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
    expect(mock.send).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, "Research"),
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
  };
  const { rerender } = render(<DatabaseApprovalCard approval={approval} />);
  fireEvent.click(screen.getByRole("button", { name: "Approve once" }));
  await waitFor(() => expect(mock.decide).toHaveBeenCalledTimes(1));
  rerender(
    <DatabaseApprovalCard
      approval={{ ...approval, state: "approved", canDecide: false, sentence: "Queued locally" }}
    />,
  );
  expect(screen.queryByRole("button", { name: "Approve once" })).not.toBeInTheDocument();
  rerender(
    <DatabaseApprovalCard
      approval={{
        ...approval,
        refused: "Review it again.",
        sentence: "Decision rejected: review it again.",
      }}
    />,
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "Approve once" })).toBeEnabled());
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
  expect(screen.getByRole("button", { name: /^beta session/ })).toHaveFocus();
});

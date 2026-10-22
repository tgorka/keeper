/**
 * A tapped notification against the whole Agents window (A106-02, A106-03): the real
 * `usePushOpen` hook and `AgentsWindow` mounted together over mocked IPC, so the
 * hook's selection and the window's own navigation meet as they do on the phone.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsWindow } from "@/components/agents/agents-window";
import { TooltipProvider } from "@/components/ui/tooltip";
import { usePushOpen } from "@/hooks/use-push-open";
import type * as Ipc from "@/lib/ipc/client";
import type {
  AgentSessionRowVm,
  AgentSessionsBatch,
  AgentSessionViewVm,
  HqStatusVm,
  PushOpenVm,
} from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

const mock = vi.hoisted(() => ({
  list: null as ((batch: AgentSessionsBatch) => void) | null,
  view: null as ((vm: AgentSessionViewVm) => void) | null,
  status: null as ((status: HqStatusVm) => void) | null,
  tapped: null as (() => void) | null,
  waiting: null as PushOpenVm | null,
  open: vi.fn(),
}));
vi.mock("@/hooks/use-shell-layout", () => ({ useShellLayout: () => ({ phone: true }) }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentsSessionsSubscribe: vi.fn(async (callback) => {
    mock.list = callback;
    return "list";
  }),
  agentsSessionsUnsubscribe: vi.fn().mockResolvedValue(undefined),
  agentSessionOpen: mock.open,
  agentSessionClose: vi.fn().mockResolvedValue(undefined),
  agentApprovalsSubscribe: vi.fn(async (callback) => {
    callback([]);
    return "approvals";
  }),
  agentApprovalsUnsubscribe: vi.fn().mockResolvedValue(undefined),
  agentsChoices: vi.fn().mockResolvedValue([
    {
      agentActor: "researcher",
      agentName: "Researcher",
      agentMark: "R",
      databases: ["beta"],
      database: "beta",
      ask: false,
    },
  ]),
  agentSessionNew: vi.fn(),
  hqStatusSubscribe: vi.fn(async (callback) => {
    mock.status = callback;
    return "status";
  }),
  hqStatusUnsubscribe: vi.fn().mockResolvedValue(undefined),
  pushOpenTake: vi.fn(async () => {
    const target = mock.waiting;
    mock.waiting = null;
    return target;
  }),
  listenPushOpen: vi.fn(async (handler) => {
    mock.tapped = handler;
    return () => {};
  }),
}));

const HELD = { database: "beta", sessionId: "held.1" };

function row(sessionId: string): AgentSessionRowVm {
  return {
    database: "beta",
    sessionId,
    title: `about ${sessionId}`,
    agentActor: "researcher",
    agentName: "Researcher",
    agentMark: "R",
    archived: false,
    lastActivityMs: 1780000000000,
  };
}

function view(sessionId: string): AgentSessionViewVm {
  return {
    ...row(sessionId),
    readOnly: false,
    hasEarlier: false,
    loadingEarlier: false,
    complete: true,
    readsTrace: true,
    untrusted: false,
    live: [],
    tools: [],
    prefs: { showReasoning: false, showDetails: false },
    messages: [
      {
        id: "m1",
        role: "agent",
        authorActor: "researcher",
        authorName: "Researcher",
        body: `The answer in ${sessionId}`,
        createdAtMs: 1780000000000,
      },
    ],
    approvals: [],
  };
}

function Phone() {
  usePushOpen();
  return (
    <TooltipProvider>
      <AgentsWindow />
    </TooltipProvider>
  );
}

async function tap(target: { database: string; sessionId: string }) {
  await waitFor(() => expect(mock.tapped).not.toBeNull());
  mock.waiting = { kind: "session", ...target };
  act(() => mock.tapped?.());
}

beforeEach(() => {
  vi.clearAllMocks();
  mock.list = null;
  mock.view = null;
  mock.status = null;
  mock.tapped = null;
  mock.waiting = null;
  mock.open.mockImplementation(async (_target, onView) => {
    mock.view = onView;
    return "view";
  });
  agentSessionsStore.setState({
    rows: [],
    loaded: false,
    error: null,
    selected: null,
    gone: null,
    newSession: null,
    creating: false,
    newDraft: null,
  });
});

describe("a tapped notification in the Agents window", () => {
  it("leaves an open New session for the session it names, and keeps what was written", async () => {
    render(<Phone />);
    await waitFor(() => expect(mock.list).not.toBeNull());
    act(() => mock.list?.({ ops: [{ op: "reset", rows: [row(HELD.sessionId)] }], total: 1 }));

    fireEvent.click(screen.getByRole("button", { name: "New session" }));
    fireEvent.keyDown(await screen.findByRole("combobox", { name: "Agent" }), {
      key: "ArrowDown",
    });
    fireEvent.click(await screen.findByRole("option", { name: "Researcher" }));
    const message = screen.getByRole("textbox", { name: "Message" });
    await waitFor(() => expect(message).toBeEnabled());
    fireEvent.change(message, { target: { value: "Half a thought" } });

    await tap(HELD);
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "New session" })).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(mock.open).toHaveBeenCalledTimes(1));
    expect(mock.open.mock.calls[0][0]).toEqual(HELD);
    act(() => mock.view?.(view(HELD.sessionId)));
    expect(screen.getByText("The answer in held.1")).toBeInTheDocument();

    // The words are where the person left them when New session opens again.
    fireEvent.click(screen.getByRole("button", { name: "Back to sessions" }));
    fireEvent.click(screen.getByRole("button", { name: "New session" }));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Half a thought"),
    );
    expect(screen.getByRole("combobox", { name: "Agent" })).toHaveTextContent("Researcher");

    // Cancel is the person leaving them.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "New session" }));
    await screen.findByRole("region", { name: "New session" });
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("");
  });
});

import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { DockBody } from "@/components/notes/notes-agent-dock";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { AgentSessionViewVm } from "@/lib/ipc/client";
import { agentSessionDefault } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

const mock = vi.hoisted(() => ({
  list: vi.fn(),
  views: [] as ((vm: AgentSessionViewVm) => void)[],
}));
vi.mock("@/lib/ipc/client", () => ({
  agentSessionDefault: vi.fn().mockResolvedValue({
    agentActor: "assistant",
    agentName: "Assistant",
    database: "beta",
    sessionId: "same.1",
  }),
  agentsSessionsSubscribe: mock.list.mockImplementation(async () => "list"),
  agentsSessionsUnsubscribe: vi.fn().mockResolvedValue(undefined),
  agentSessionOpen: vi.fn(async (_target, callback) => {
    mock.views.push(callback);
    return "view";
  }),
  agentSessionClose: vi.fn().mockResolvedValue(undefined),
}));
beforeEach(() => {
  vi.clearAllMocks();
  mock.views = [];
  agentSessionsStore.setState({
    selected: { database: "alpha", sessionId: "same.1" },
    rows: [],
    newSession: null,
  });
});
it("shares a list channel across docks but keeps the window's selection and each transcript separate", async () => {
  render(
    <TooltipProvider>
      <DockBody />
      <DockBody />
    </TooltipProvider>,
  );
  await waitFor(() => expect(mock.views).toHaveLength(2));
  const vm: AgentSessionViewVm = {
    database: "beta",
    sessionId: "same.1",
    title: "Beta work",
    displayName: "assistant · Beta work",
    handle: "jolly-pigeon-squeaks",
    agentActor: "assistant",
    agentName: "Assistant",
    agentMark: "A",
    archived: true,
    readOnly: true,
    complete: true,
    hasEarlier: false,
    loadingEarlier: false,
    readsTrace: false,
    untrusted: false,
    live: [],
    tools: [],
    prefs: { viewMode: "normal" },
    approvals: [],
    items: [{ kind: "message", messageId: "m" }],
    folds: [],
    messages: [
      {
        id: "m",
        role: "agent",
        authorActor: "assistant",
        authorName: "Assistant",
        body: "Only beta's message",
        createdAtMs: 1780000000000,
      },
    ],
  };
  act(() => mock.views[0](vm));
  expect(screen.getAllByText("Only beta's message")).toHaveLength(1);
  expect(agentSessionsStore.getState().selected).toEqual({
    database: "alpha",
    sessionId: "same.1",
  });
  expect(mock.list).toHaveBeenCalledTimes(1);
});

it("keeps an unavailable background assistant out of the Notes page's alert channel", async () => {
  vi.mocked(agentSessionDefault).mockRejectedValueOnce({
    message: "No agents server is paired here.",
  });
  render(
    <TooltipProvider>
      <DockBody />
    </TooltipProvider>,
  );
  expect(await screen.findByRole("status", { name: "Assistant availability" })).toHaveTextContent(
    "No agents server is paired here.",
  );
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Message" })).not.toBeInTheDocument();
});

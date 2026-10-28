import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { DockBody } from "@/components/notes/notes-agent-dock";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { AgentSessionViewVm } from "@/lib/ipc/client";
import { agentSessionDefault, agentSessionOpen } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

const mock = vi.hoisted(() => ({
  list: vi.fn(),
  views: [] as ((vm: AgentSessionViewVm) => void)[],
  command: vi.fn(),
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
  agentCommand: mock.command,
}));
/** Beta's archived session, as Rust's view draws it. */
const beta: AgentSessionViewVm = {
  database: "beta",
  sessionId: "same.1",
  title: "Beta work",
  displayName: "assistant · Beta work",
  handle: "jolly-pigeon",
  agentActor: "assistant",
  agentName: "Assistant",
  agentMark: "A",
  archived: true,
  readOnly: true,
  complete: true,
  hasEarlier: false,
  loadingEarlier: false,
  readsTrace: false,
  context: {
    used: null,
    window: null,
    label: "—",
    percent: null,
    meterPercent: null,
    ringBackground: "conic-gradient(currentColor 0deg, var(--muted) 0deg)",
    summary: "Context unknown: no answer has measured it yet.",
    parts: [],
  },
  footer: {
    model: "Default",
    effort: "Default",
    shortModel: "Default",
    effortCode: "D",
    choosesModel: false,
    choosesEffort: false,
  },
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
      mine: false,
      authorName: "Assistant",
      body: "Only beta's message",
      createdAtMs: 1780000000000,
    },
  ],
};
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
  act(() => mock.views[0](beta));
  expect(screen.getAllByText("Only beta's message")).toHaveLength(1);
  expect(agentSessionsStore.getState().selected).toEqual({
    database: "alpha",
    sessionId: "same.1",
  });
  expect(mock.list).toHaveBeenCalledTimes(1);
});

// R601-02: `/clean`, `/new` or the Untrusted chip's fresh chat starts a new session; the dock
// shows it, and the Agents window keeps showing what it showed (R601-04).
it("follows the session a command started, and leaves the window's selection alone", async () => {
  const next = { database: "beta", sessionId: "same.2" };
  mock.command.mockResolvedValue({ session: next, surface: null, message: "" });
  render(
    <TooltipProvider>
      <DockBody />
    </TooltipProvider>,
  );
  await waitFor(() => expect(mock.views).toHaveLength(1));
  act(() =>
    mock.views[0]({
      ...beta,
      archived: false,
      readOnly: false,
      untrusted: {
        sentence: "This chat read outside content.",
        freshChat: "Start a fresh chat: starts trusted, keeps the drives you can still tick",
      },
    }),
  );
  fireEvent.click(within(screen.getByRole("banner")).getByRole("button", { name: /^Untrusted:/ }));
  const popover = await screen.findByRole("dialog", { name: "Why commands ask" });
  fireEvent.click(within(popover).getByRole("button", { name: /^Start a fresh chat/ }));
  // The dock opens the new session's view in place of the old one.
  await waitFor(() => expect(mock.views).toHaveLength(2));
  expect(vi.mocked(agentSessionOpen).mock.lastCall?.[0]).toEqual(next);
  expect(agentSessionsStore.getState().selected).toEqual({
    database: "alpha",
    sessionId: "same.1",
  });
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

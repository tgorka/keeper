import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentsLaunchVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsDefaultGet: vi.fn(),
  agentsDefaultSet: vi.fn(),
  agentsDefaultChooserDismiss: vi.fn(),
  agentMainDmOpen: vi.fn(),
  frontendErrorReport: vi.fn(() => Promise.resolve()),
}));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), {
    error: vi.fn(),
    loading: vi.fn(() => "waiting"),
    dismiss: vi.fn(),
  }),
}));

import { toast } from "sonner";
import {
  DEFAULT_AGENT_CHOOSER_NOT_NOW,
  DEFAULT_AGENT_CHOOSER_TITLE,
  DefaultAgentChooser,
} from "@/components/agents/default-agent-chooser";
import {
  agentMainDmOpen,
  agentsDefaultChooserDismiss,
  agentsDefaultGet,
  agentsDefaultSet,
  frontendErrorReport,
} from "@/lib/ipc/client";
import { composerStore } from "@/lib/stores/composer";
import { defaultAgentStore } from "@/lib/stores/default-agent";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

const NIXI = "@nixi:example.org";
const TALA = "@tala:example.org";
const ACCOUNT = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

function vm(overrides: Partial<AgentsLaunchVm>): AgentsLaunchVm {
  return {
    view: "agents",
    room: null,
    defaultAgent: null,
    reason: null,
    announce: false,
    choices: [
      { agent: NIXI, name: "Nixi" },
      { agent: TALA, name: "Tala" },
    ],
    chooser: true,
    fileSet: null,
    ask: null,
    askable: [],
    ...overrides,
  };
}

function reset(): void {
  vi.mocked(agentsDefaultGet).mockReset();
  vi.mocked(agentsDefaultSet).mockReset();
  vi.mocked(agentsDefaultChooserDismiss).mockReset();
  vi.mocked(agentMainDmOpen).mockReset();
  vi.mocked(toast).mockClear();
  vi.mocked(toast.error).mockClear();
  roomsStore.getState().selectRoom(null);
  primaryViewStore.getState().setView("agents");
  composerStore.setState({ focusWhenReady: null });
  defaultAgentStore.setState({
    vm: null,
    readError: null,
    saveError: null,
    saving: false,
    navRestore: "pending",
    chooserOpen: false,
  });
}

beforeEach(reset);
afterEach(reset);

/** The launch said to ask (R463). */
function launched(answer: AgentsLaunchVm = vm({})): void {
  defaultAgentStore.setState({ vm: answer, chooserOpen: true });
}

describe("DefaultAgentChooser", () => {
  it("lists the person's agents as buttons, reachable from the keyboard, only once the launch asked", () => {
    defaultAgentStore.setState({ vm: vm({}) });
    const { rerender } = render(<DefaultAgentChooser />);
    expect(screen.queryByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeNull();

    act(() => defaultAgentStore.getState().openChooser());
    rerender(<DefaultAgentChooser />);
    const chooser = screen.getByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE });
    const nixi = screen.getByRole("button", { name: "Nixi" });
    expect(chooser).toContainElement(nixi);
    expect(screen.getByRole("button", { name: "Tala" })).toBeEnabled();
    nixi.focus();
    expect(nixi).toHaveFocus();
  });

  it("stores the choice and opens its conversation at once with the composer focused", async () => {
    const dm = { accountId: ACCOUNT, roomId: "!tala-dm:example.org" };
    vi.mocked(agentsDefaultSet).mockResolvedValue(
      vm({ defaultAgent: TALA, room: dm, chooser: false }),
    );
    launched();
    render(<DefaultAgentChooser />);

    fireEvent.click(screen.getByRole("button", { name: "Tala" }));
    await waitFor(() => expect(roomsStore.getState().selected).toEqual(dm));
    expect(agentsDefaultSet).toHaveBeenCalledWith(TALA);
    expect(defaultAgentStore.getState().chooserOpen).toBe(false);
    expect(screen.queryByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeNull();
    expect(composerStore.getState().focusWhenReady).toEqual(dm);
  });

  it("asks the person's own agent for a conversation when it has none here, and opens it", async () => {
    const ask = { accountId: ACCOUNT, agent: NIXI, name: "Nixi", opening: "Opening…" };
    const dm = { accountId: ACCOUNT, roomId: "!nixi-dm:example.org" };
    vi.mocked(agentsDefaultSet).mockResolvedValue(vm({ defaultAgent: NIXI, ask, chooser: false }));
    vi.mocked(agentMainDmOpen).mockResolvedValue(dm);
    launched();
    render(<DefaultAgentChooser />);

    fireEvent.click(screen.getByRole("button", { name: "Nixi" }));
    await waitFor(() => expect(roomsStore.getState().selected).toEqual(dm));
    expect(agentMainDmOpen).toHaveBeenCalledWith(ACCOUNT, NIXI);
  });

  it("stays, saying why, when the choice is not saved", async () => {
    vi.mocked(agentsDefaultSet).mockRejectedValue({ message: "the settings table is locked" });
    launched();
    render(<DefaultAgentChooser />);

    fireEvent.click(screen.getByRole("button", { name: "Nixi" }));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("the settings table is locked"));
    expect(roomsStore.getState().selected).toBeNull();
    expect(screen.getByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeInTheDocument();
  });

  it("Not now puts it away in Rust and takes it off screen; a failed write is said", async () => {
    vi.mocked(agentsDefaultChooserDismiss).mockResolvedValueOnce(undefined);
    launched();
    const { rerender } = render(<DefaultAgentChooser />);
    fireEvent.click(screen.getByRole("button", { name: DEFAULT_AGENT_CHOOSER_NOT_NOW }));
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeNull(),
    );
    expect(agentsDefaultChooserDismiss).toHaveBeenCalledTimes(1);
    expect(agentsDefaultSet).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();

    vi.mocked(agentsDefaultChooserDismiss).mockRejectedValueOnce({ message: "database is locked" });
    act(() => defaultAgentStore.getState().openChooser());
    rerender(<DefaultAgentChooser />);
    fireEvent.click(screen.getByRole("button", { name: DEFAULT_AGENT_CHOOSER_NOT_NOW }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("database is locked")),
    );
  });

  it("leaves when a default is chosen elsewhere meanwhile", () => {
    launched();
    const { rerender } = render(<DefaultAgentChooser />);
    expect(screen.getByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeInTheDocument();
    act(() => defaultAgentStore.setState({ vm: vm({ defaultAgent: NIXI, chooser: false }) }));
    rerender(<DefaultAgentChooser />);
    expect(screen.queryByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeNull();
  });

  /** A save that answers only when the test says so. */
  function heldSave(): (answer: AgentsLaunchVm) => Promise<void> {
    let answer: (vm: AgentsLaunchVm) => void = () => {};
    vi.mocked(agentsDefaultSet).mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    return async (vm) => {
      await act(async () => answer(vm));
    };
  }
  const TOLA_ROOM = { accountId: ACCOUNT, roomId: "!tola-review:example.org" };

  it("a slow save that answers with an existing conversation leaves the room the person opened meanwhile", async () => {
    const save = heldSave();
    launched();
    render(<DefaultAgentChooser />);
    fireEvent.click(screen.getByRole("button", { name: "Nixi" }));
    // The person opens Tola while the save is out (R397).
    act(() => roomsStore.getState().selectRoom(TOLA_ROOM));

    const nixiDm = { accountId: ACCOUNT, roomId: "!nixi-dm:example.org" };
    await save(vm({ defaultAgent: NIXI, room: nixiDm, chooser: false }));
    // Saved all the same; nothing opened over Tola, no focus asked for.
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(NIXI);
    expect(roomsStore.getState().selected).toEqual(TOLA_ROOM);
    expect(composerStore.getState().focusWhenReady).toBeNull();
  });

  it("a slow save whose conversation must be asked for never opens it over the person's newer room", async () => {
    const save = heldSave();
    const ask = { accountId: ACCOUNT, agent: NIXI, name: "Nixi", opening: "Opening…" };
    const nixiDm = { accountId: ACCOUNT, roomId: "!nixi-dm:example.org" };
    vi.mocked(agentMainDmOpen).mockResolvedValue(nixiDm);
    launched();
    render(<DefaultAgentChooser />);
    fireEvent.click(screen.getByRole("button", { name: "Nixi" }));
    act(() => roomsStore.getState().selectRoom(TOLA_ROOM));

    await save(vm({ defaultAgent: NIXI, ask, chooser: false }));
    await act(async () => {});
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(NIXI);
    expect(roomsStore.getState().selected).toEqual(TOLA_ROOM);
    expect(composerStore.getState().focusWhenReady).toBeNull();
  });

  it("a conversation that lands after the person moved on is not opened", async () => {
    const ask = { accountId: ACCOUNT, agent: NIXI, name: "Nixi", opening: "Opening…" };
    const nixiDm = { accountId: ACCOUNT, roomId: "!nixi-dm:example.org" };
    let land: (room: typeof nixiDm) => void = () => {};
    vi.mocked(agentMainDmOpen).mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    vi.mocked(agentsDefaultSet).mockResolvedValue(vm({ defaultAgent: NIXI, ask, chooser: false }));
    launched();
    render(<DefaultAgentChooser />);
    fireEvent.click(screen.getByRole("button", { name: "Nixi" }));
    await waitFor(() => expect(agentMainDmOpen).toHaveBeenCalledWith(ACCOUNT, NIXI));
    // The person goes to the Inbox while the conversation is being opened.
    act(() => primaryViewStore.getState().setView("inbox"));

    await act(async () => land(nixiDm));
    expect(roomsStore.getState().selected).toBeNull();
    expect(composerStore.getState().focusWhenReady).toBeNull();
  });

  it("opens nothing once the Agents window it was chosen in is gone", async () => {
    const save = heldSave();
    launched();
    const { unmount } = render(<DefaultAgentChooser />);
    fireEvent.click(screen.getByRole("button", { name: "Nixi" }));
    unmount();

    const nixiDm = { accountId: ACCOUNT, roomId: "!nixi-dm:example.org" };
    await save(vm({ defaultAgent: NIXI, room: nixiDm, chooser: false }));
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(NIXI);
    expect(roomsStore.getState().selected).toBeNull();
  });

  it("says a failed read of the default agent in the Agents list, chooser shown or not (R381)", async () => {
    vi.mocked(agentsDefaultGet).mockRejectedValue({ message: "the settings table is locked" });
    render(<DefaultAgentChooser />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("keeper could not read the default agent");
    expect(alert).toHaveTextContent("the settings table is locked");
    expect(screen.queryByRole("region", { name: DEFAULT_AGENT_CHOOSER_TITLE })).toBeNull();
    // And in keeper.log.
    expect(frontendErrorReport).toHaveBeenCalledWith(
      "agents.default.read",
      expect.any(String),
      null,
      null,
    );
  });
});

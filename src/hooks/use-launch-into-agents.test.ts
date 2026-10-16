import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, AgentsLaunchVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsLaunch: vi.fn(),
  agentsDefaultGet: vi.fn(),
  agentsDefaultSet: vi.fn(),
  agentMainDmOpen: vi.fn(),
  frontendErrorReport: vi.fn(() => Promise.resolve()),
  telemetryCapture: vi.fn(() => Promise.resolve()),
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
  launchFailedSentence,
  REASON_NOTICE_MS,
  useLaunchIntoAgents,
} from "@/hooks/use-launch-into-agents";
import {
  agentMainDmOpen,
  agentsLaunch,
  frontendErrorReport,
  telemetryCapture,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { agentRoomsStore } from "@/lib/stores/agent-rooms";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { composerStore } from "@/lib/stores/composer";
import { defaultAgentStore } from "@/lib/stores/default-agent";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

const mockLaunch = vi.mocked(agentsLaunch);

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

const DM = { accountId: account.accountId, roomId: "!nixi-dm:example.org" };

function answer(overrides: Partial<AgentsLaunchVm>): AgentsLaunchVm {
  return {
    view: "agents",
    room: null,
    defaultAgent: null,
    reason: null,
    announce: false,
    choices: [],
    fileSet: null,
    ask: null,
    askable: [],
    ...overrides,
  };
}

/** The desktop tier: a tier-telling capability present. */
const DESKTOP = { ...DEFAULT_CAPABILITIES, trayIcon: true, globalHotkey: true };

function boot({ phone = false }: { phone?: boolean } = {}): void {
  act(() => {
    capabilitiesStore.getState().applySnapshot(phone ? DEFAULT_CAPABILITIES : DESKTOP);
    accountsStore.getState().hydrateAll([account]);
    accountsStore.getState().markHydrated();
  });
}

/** Flush the effects and the launch answer's promise chain (microtasks only). */
async function settle(): Promise<void> {
  await act(async () => {
    for (let tick = 0; tick < 5; tick += 1) {
      await Promise.resolve();
    }
  });
}

function reset(): void {
  mockLaunch.mockReset();
  vi.mocked(agentMainDmOpen).mockReset();
  vi.mocked(toast).mockClear();
  vi.mocked(toast.loading).mockClear();
  vi.mocked(toast.error).mockClear();
  vi.mocked(frontendErrorReport).mockClear();
  vi.mocked(telemetryCapture).mockClear();
  accountsStore.getState().clear();
  accountsStore.setState({ hydrated: false });
  agentRoomsStore.getState().clear();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  primaryViewStore.getState().setView("inbox");
  roomsStore.getState().selectRoom(null);
  defaultAgentStore.setState({
    vm: null,
    readError: null,
    saveError: null,
    saving: false,
    navRestore: "pending",
  });
}

beforeEach(reset);
afterEach(reset);

describe("useLaunchIntoAgents", () => {
  it("asks once the accounts are hydrated, without waiting for the Agents window, and opens the default agent's room", async () => {
    mockLaunch.mockResolvedValue(answer({ room: DM, defaultAgent: "@nixi:example.org" }));
    const focusBefore = composerStore.getState().focusNonce;
    renderHook(() => useLaunchIntoAgents());
    await settle();
    expect(mockLaunch).not.toHaveBeenCalled();
    // Rust decides when it knows (R397): the Agents window has streamed
    // nothing yet, and the question is asked all the same.
    boot();
    await settle();
    expect(agentRoomsStore.getState().total).toBeNull();
    expect(mockLaunch).toHaveBeenCalledTimes(1);
    expect(primaryViewStore.getState().view).toBe("agents");
    expect(roomsStore.getState().selected).toEqual(DM);
    // After the room's composer mounted (the next macrotask).
    await vi.waitFor(() => expect(composerStore.getState().focusNonce).toBe(focusBefore + 1));
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe("@nixi:example.org");
    // Counted once as the closed surface, nothing about the agent or room (DW-1410).
    expect(telemetryCapture).toHaveBeenCalledTimes(1);
    expect(telemetryCapture).toHaveBeenCalledWith({
      kind: "interaction",
      durationMs: null,
      surface: "launchDefaultAgent",
    });
  });

  it("asks the default agent for its conversation when it has none here, and opens it when it lands (101.1 #4)", async () => {
    const ask = {
      accountId: account.accountId,
      agent: "@nixi:example.org",
      name: "Nixi",
      opening: "Opening a conversation with Nixi…",
    };
    let land: (room: typeof DM) => void = () => {};
    vi.mocked(agentMainDmOpen).mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    mockLaunch.mockResolvedValue(answer({ defaultAgent: ask.agent, ask }));
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    expect(primaryViewStore.getState().view).toBe("agents");
    expect(agentMainDmOpen).toHaveBeenCalledWith(account.accountId, ask.agent);
    expect(toast.loading).toHaveBeenCalledWith("Opening a conversation with Nixi…");
    expect(roomsStore.getState().selected).toBeNull();

    land(DM);
    await settle();
    expect(roomsStore.getState().selected).toEqual(DM);
  });

  it("leaves the person where they went while the conversation was being opened (101.1 #4)", async () => {
    const ask = {
      accountId: account.accountId,
      agent: "@nixi:example.org",
      name: "Nixi",
      opening: "Opening a conversation with Nixi…",
    };
    let land: (room: typeof DM) => void = () => {};
    vi.mocked(agentMainDmOpen).mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    mockLaunch.mockResolvedValue(answer({ defaultAgent: ask.agent, ask }));
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    const elsewhere = { accountId: account.accountId, roomId: "!marta:example.org" };
    act(() => roomsStore.getState().selectRoom(elsewhere));

    land(DM);
    await settle();
    expect(roomsStore.getState().selected).toEqual(elsewhere);
  });

  it("applies the answer once: a later return to the Inbox and new batches stay put", async () => {
    mockLaunch.mockResolvedValue(answer({ room: DM }));
    const { rerender } = renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    expect(primaryViewStore.getState().view).toBe("agents");

    act(() => {
      primaryViewStore.getState().setView("inbox");
      roomsStore.getState().selectRoom(null);
      agentRoomsStore.getState().applyBatch({ ops: [{ op: "reset", rooms: [] }], total: 3 });
    });
    rerender();
    await settle();
    expect(mockLaunch).toHaveBeenCalledTimes(1);
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(roomsStore.getState().selected).toBeNull();
  });

  it("does not ask again when the accounts hydrate again after the launch", async () => {
    mockLaunch.mockResolvedValue(answer({ room: DM }));
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    // An account added: the accounts re-hydrate; the person has not moved.
    act(() => {
      accountsStore.getState().clear();
      accountsStore.setState({ hydrated: false });
    });
    await settle();
    boot();
    await settle();
    expect(mockLaunch).toHaveBeenCalledTimes(1);
  });

  it("never overrides the person's navigation made before the answer lands", async () => {
    let resolve: (vm: AgentsLaunchVm) => void = () => {};
    mockLaunch.mockReturnValue(
      new Promise<AgentsLaunchVm>((done) => {
        resolve = done;
      }),
    );
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    const marta = { accountId: account.accountId, roomId: "!marta:example.org" };
    act(() => roomsStore.getState().selectRoom(marta));
    resolve(answer({ room: DM }));
    await settle();
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(roomsStore.getState().selected).toEqual(marta);
    expect(telemetryCapture).not.toHaveBeenCalled();
  });

  it("opens Agents with nothing selected and says why when the default has no room here", async () => {
    const reason = "Your default agent @tala:example.org has no conversation with you here.";
    mockLaunch.mockResolvedValue(
      answer({ defaultAgent: "@tala:example.org", reason, announce: true }),
    );
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    expect(primaryViewStore.getState().view).toBe("agents");
    expect(roomsStore.getState().selected).toBeNull();
    expect(toast).toHaveBeenCalledWith(reason, { duration: REASON_NOTICE_MS });
  });

  it("leaves the Inbox quietly when Rust found no agent room", async () => {
    // A finished look with no agent room (R398): the reason is Settings' only.
    const reason = "You have no agent rooms on this device yet, so keeper opens the Inbox.";
    mockLaunch.mockResolvedValue(
      answer({ view: "inbox", defaultAgent: "@nixi:example.org", reason }),
    );
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    expect(mockLaunch).toHaveBeenCalledTimes(1);
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(toast).not.toHaveBeenCalled();
    expect(telemetryCapture).not.toHaveBeenCalled();
  });

  it("keeps the Inbox and says why when the launch stopped waiting before it saw a room", async () => {
    // R100L3-01: the first look never finished, so the Inbox is not "no agent
    // rooms" — Rust announces its reason and the person reads it.
    const reason =
      "Your conversation with your default agent @nixi:example.org has not arrived yet, so keeper opens the Inbox. It is in Agents once it arrives.";
    mockLaunch.mockResolvedValue(
      answer({ view: "inbox", defaultAgent: "@nixi:example.org", reason, announce: true }),
    );
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(roomsStore.getState().selected).toBeNull();
    expect(toast).toHaveBeenCalledWith(reason, { duration: REASON_NOTICE_MS });
    expect(telemetryCapture).not.toHaveBeenCalled();
  });

  it("keeps the Inbox, says so and logs when Rust cannot answer", async () => {
    mockLaunch.mockRejectedValue({ message: "the settings table is locked" });
    renderHook(() => useLaunchIntoAgents());
    boot();
    await settle();
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(toast.error).toHaveBeenCalledWith(
      launchFailedSentence({ message: "the settings table is locked" }),
    );
    expect(frontendErrorReport).toHaveBeenCalledTimes(1);
  });

  it("on the phone waits for the restore and applies only when nothing was restored", async () => {
    mockLaunch.mockResolvedValue(answer({ room: DM }));
    const focusBefore = composerStore.getState().focusNonce;
    renderHook(() => useLaunchIntoAgents());
    boot({ phone: true });
    await settle();
    expect(mockLaunch).not.toHaveBeenCalled();

    vi.useFakeTimers({ toFake: ["setTimeout"] });
    try {
      act(() => defaultAgentStore.getState().setNavRestore("nothing"));
      await settle();
      expect(primaryViewStore.getState().view).toBe("agents");
      expect(roomsStore.getState().selected).toEqual(DM);
      act(() => vi.runAllTimers());
      // The phone never steals composer focus when a room opens.
      expect(composerStore.getState().focusNonce).toBe(focusBefore);
    } finally {
      vi.useRealTimers();
    }
  });

  it("on the phone leaves a restored room where it was", async () => {
    mockLaunch.mockResolvedValue(answer({ room: DM }));
    // The restore selected the room before this hook attached.
    const restored = { accountId: account.accountId, roomId: "!restored:example.org" };
    roomsStore.getState().selectRoom(restored);
    renderHook(() => useLaunchIntoAgents());
    boot({ phone: true });
    act(() => defaultAgentStore.getState().setNavRestore("restored"));
    await settle();
    expect(mockLaunch).not.toHaveBeenCalled();
    expect(roomsStore.getState().selected).toEqual(restored);
    expect(primaryViewStore.getState().view).toBe("inbox");
  });
});

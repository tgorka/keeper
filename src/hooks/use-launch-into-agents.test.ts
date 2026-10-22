import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useLaunchIntoAgents } from "@/hooks/use-launch-into-agents";
import type { AgentDefaultSessionVm } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { defaultAgentStore } from "@/lib/stores/default-agent";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

const launch = vi.hoisted(() => vi.fn());
vi.mock("@/lib/ipc/client", () => ({ agentSessionDefault: launch }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
const target: AgentDefaultSessionVm = {
  agentActor: "assistant",
  agentName: "Assistant",
  database: "beta",
  sessionId: "device.1",
};
beforeEach(() => {
  launch.mockReset().mockResolvedValue(target);
  capabilitiesStore.setState({
    capabilities: { ...DEFAULT_CAPABILITIES, trayIcon: true },
    hydrated: true,
  });
  defaultAgentStore.setState({ navRestore: "nothing" });
  agentSessionsStore.setState({ selected: null, newSession: null });
  primaryViewStore.getState().setView("inbox");
  roomsStore.getState().selectRoom(null);
});
describe("database default-agent launch", () => {
  it("opens Rust's database-qualified local default without a Matrix account", async () => {
    renderHook(() => useLaunchIntoAgents());
    await waitFor(() =>
      expect(agentSessionsStore.getState().selected).toEqual({
        database: "beta",
        sessionId: "device.1",
      }),
    );
    expect(primaryViewStore.getState().view).toBe("agents");
    expect(roomsStore.getState().selected).toBeNull();
  });
  it("does not replace navigation made while the answer was pending", async () => {
    let resolve: (value: AgentDefaultSessionVm) => void = () => {};
    launch.mockReturnValue(
      new Promise<AgentDefaultSessionVm>((done) => {
        resolve = done;
      }),
    );
    renderHook(() => useLaunchIntoAgents());
    act(() => primaryViewStore.getState().setView("settings"));
    await act(async () => resolve(target));
    expect(primaryViewStore.getState().view).toBe("settings");
    expect(agentSessionsStore.getState().selected).toBeNull();
  });
  it("restores a phone's navigation instead of launching over it", async () => {
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES });
    defaultAgentStore.setState({ navRestore: "restored" });
    renderHook(() => useLaunchIntoAgents());
    await act(async () => {});
    expect(launch).not.toHaveBeenCalled();
  });
  it("opens a new-session composer when the default has no session yet", async () => {
    launch.mockResolvedValue({ ...target, sessionId: null });
    renderHook(() => useLaunchIntoAgents());
    await waitFor(() =>
      expect(agentSessionsStore.getState().newSession).toEqual({ ...target, sessionId: null }),
    );
    expect(agentSessionsStore.getState().selected).toBeNull();
  });
});

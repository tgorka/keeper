import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recoveredSessionsList: vi.fn(),
  recoveredSessionAcknowledge: vi.fn(),
}));

import { useRecoveredSessions } from "@/hooks/use-recovered-sessions";
import type { RecordingSummaryVm } from "@/lib/ipc/client";
import { recoveredSessionAcknowledge, recoveredSessionsList } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const mockList = vi.mocked(recoveredSessionsList);
const mockAck = vi.mocked(recoveredSessionAcknowledge);

const SESSION_A: RecordingSummaryVm = {
  sessionFolder: "/Users/alice/Movies/keeper/keeper-rec a",
  screenSegmentCount: 2,
  totalBytes: 200_000_000,
};
const SESSION_B: RecordingSummaryVm = {
  sessionFolder: "/Users/alice/Movies/keeper/keeper-rec b",
  screenSegmentCount: 1,
  totalBytes: 50_000_000,
};

beforeEach(() => {
  mockList.mockReset();
  mockList.mockResolvedValue([SESSION_A, SESSION_B]);
  mockAck.mockReset();
  mockAck.mockResolvedValue(undefined);
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
});

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  vi.clearAllMocks();
});

describe("useRecoveredSessions", () => {
  it("fetches the recovered sessions on mount when recording is available", async () => {
    const { result } = renderHook(() => useRecoveredSessions());
    await waitFor(() => expect(result.current.sessions).toHaveLength(2));
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(result.current.sessions[0]).toEqual(SESSION_A);
  });

  it("does not fetch when recording is unavailable", async () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: false });
    const { result } = renderHook(() => useRecoveredSessions());
    // A tick to let any (unwanted) effect fire.
    await Promise.resolve();
    expect(mockList).not.toHaveBeenCalled();
    expect(result.current.sessions).toHaveLength(0);
  });

  it("acknowledge latches the notice and drops the session from local state", async () => {
    const { result } = renderHook(() => useRecoveredSessions());
    await waitFor(() => expect(result.current.sessions).toHaveLength(2));

    act(() => {
      result.current.acknowledge(SESSION_A.sessionFolder);
    });

    // Dropped locally at once.
    expect(result.current.sessions).toEqual([SESSION_B]);
    // And latched in the Rust seen-set.
    await waitFor(() => expect(mockAck).toHaveBeenCalledWith(SESSION_A.sessionFolder));
  });

  it("keeps the previous list when a refresh fetch fails", async () => {
    const { result } = renderHook(() => useRecoveredSessions());
    await waitFor(() => expect(result.current.sessions).toHaveLength(2));

    mockList.mockRejectedValueOnce(new Error("transient IPC noise"));
    act(() => {
      result.current.refresh();
    });
    // The previous list survives — never flashes to empty.
    await Promise.resolve();
    expect(result.current.sessions).toHaveLength(2);
  });
});

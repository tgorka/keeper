/**
 * FR-267 — the zone-search hook's streaming, supersession and cancellation.
 *
 * The IPC client is mocked so nothing touches Tauri: what is under test is the
 * hook's own contract — that a debounce collapses a burst of keystrokes into one
 * scan, that batches accumulate as they land rather than waiting for `done`,
 * that a scan superseded by unmount is cancelled, and that a scan whose id
 * arrives *after* the cleanup is cancelled with the id it was handed.
 */
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionSearchBatch, SessionSearchReq } from "@/lib/ipc/client";

type OnBatch = (batch: SessionSearchBatch) => void;

const sessionsSearch =
  vi.fn<(rootId: string, req: SessionSearchReq, onBatch: OnBatch) => Promise<string>>();
const sessionsSearchCancel = vi.fn<(subscriptionId: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  sessionsSearch: (rootId: string, req: SessionSearchReq, onBatch: OnBatch) =>
    sessionsSearch(rootId, req, onBatch),
  sessionsSearchCancel: (subscriptionId: string) => sessionsSearchCancel(subscriptionId),
}));

import { SESSION_SEARCH_DEBOUNCE_MS, useSessionsSearch } from "@/hooks/use-sessions-search";

function hit(file: string, line: number) {
  return {
    sessionId: "s1",
    sessionTitle: "One",
    file,
    subpath: `60-sessions/active/2026-08-14-one/${file}`,
    line,
    snippet: `a line with plan in it`,
  };
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  sessionsSearch.mockReset();
  sessionsSearchCancel.mockReset();
  sessionsSearchCancel.mockResolvedValue(undefined);
  sessionsSearch.mockResolvedValue("scan-1");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useSessionsSearch", () => {
  it("makes no call for an empty query or a null root", async () => {
    const { rerender } = renderHook(
      ({ rootId, query }: { rootId: string | null; query: string }) =>
        useSessionsSearch(rootId, query),
      { initialProps: { rootId: "r1" as string | null, query: "   " } },
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    expect(sessionsSearch).not.toHaveBeenCalled();

    rerender({ rootId: null, query: "plan" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    expect(sessionsSearch).not.toHaveBeenCalled();
  });

  it("debounces a burst of keystrokes into one scan", async () => {
    const { rerender } = renderHook(
      ({ query }: { query: string }) => useSessionsSearch("r1", query),
      {
        initialProps: { query: "p" },
      },
    );
    rerender({ query: "pl" });
    rerender({ query: "pla" });
    rerender({ query: "plan" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    expect(sessionsSearch).toHaveBeenCalledTimes(1);
    expect(sessionsSearch.mock.calls[0]?.[1].text).toBe("plan");
  });

  it("accumulates batches as they land and stops running on done", async () => {
    let deliver: OnBatch | null = null;
    sessionsSearch.mockImplementation(async (_rootId, _req, onBatch) => {
      deliver = onBatch;
      return "scan-1";
    });
    const { result } = renderHook(() => useSessionsSearch("r1", "plan"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    await waitFor(() => expect(deliver).not.toBeNull());
    expect(result.current.running).toBe(true);

    const send = deliver as unknown as OnBatch;
    act(() => {
      send({ done: false, hits: [hit("about.md", 3)] });
    });
    expect(result.current.hits).toHaveLength(1);
    // Still running: a first batch is not an answer, `done` is.
    expect(result.current.running).toBe(true);

    act(() => {
      send({ done: true, hits: [hit("2026-08-14-1030-plan.md", 7)] });
    });
    expect(result.current.hits).toHaveLength(2);
    expect(result.current.running).toBe(false);
    expect(result.current.hits[1]?.subpath).toBe(
      "60-sessions/active/2026-08-14-one/2026-08-14-1030-plan.md",
    );
  });

  it("cancels the running scan when the surface unmounts", async () => {
    const { unmount } = renderHook(() => useSessionsSearch("r1", "plan"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    await waitFor(() => expect(sessionsSearch).toHaveBeenCalledTimes(1));
    unmount();
    await waitFor(() => expect(sessionsSearchCancel).toHaveBeenCalledWith("scan-1"));
  });

  it("cancels a scan whose id arrives after the cleanup ran", async () => {
    let settle: ((id: string) => void) | null = null;
    sessionsSearch.mockImplementation(
      () =>
        new Promise<string>((resolve) => {
          settle = resolve;
        }),
    );
    const { unmount } = renderHook(() => useSessionsSearch("r1", "plan"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    await waitFor(() => expect(settle).not.toBeNull());
    // Unmount BEFORE Rust hands back the id: the cleanup has nothing to cancel
    // with, so the resolution itself must do it.
    unmount();
    await act(async () => {
      (settle as unknown as (id: string) => void)("scan-late");
    });
    await waitFor(() => expect(sessionsSearchCancel).toHaveBeenCalledWith("scan-late"));
  });

  it("surfaces a failed scan as an error and stops running", async () => {
    sessionsSearch.mockRejectedValue({ code: "internal", message: "zone vanished" });
    const { result } = renderHook(() => useSessionsSearch("r1", "plan"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_SEARCH_DEBOUNCE_MS * 2);
    });
    await waitFor(() => expect(result.current.error).toBe("zone vanished"));
    expect(result.current.running).toBe(false);
  });
});

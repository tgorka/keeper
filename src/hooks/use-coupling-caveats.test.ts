import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CouplingCaveatVm } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
const couplingCaveats = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  couplingCaveats: () => couplingCaveats(),
}));

import { useCouplingCaveats } from "@/hooks/use-coupling-caveats";

const whatsapp: CouplingCaveatVm = {
  networkId: "whatsapp",
  text: "you may also stop seeing others' read receipts",
  appliesTo: "read-receipts",
};

beforeEach(() => {
  couplingCaveats.mockReset();
  couplingCaveats.mockResolvedValue([whatsapp]);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("useCouplingCaveats", () => {
  it("returns the caveats that apply to a coupled network", async () => {
    const { result } = renderHook(() => useCouplingCaveats("whatsapp"));
    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(result.current[0]).toEqual(whatsapp);
  });

  it("returns [] for a network with no coupling caveat", async () => {
    const { result } = renderHook(() => useCouplingCaveats("telegram"));
    // Give the fetch a chance to resolve, then confirm no caveat matches.
    await waitFor(() => expect(couplingCaveats).toHaveBeenCalled());
    expect(result.current).toEqual([]);
  });

  it("returns [] for a null network (native Matrix room)", async () => {
    const { result } = renderHook(() => useCouplingCaveats(null));
    await waitFor(() => expect(couplingCaveats).toHaveBeenCalled());
    expect(result.current).toEqual([]);
  });

  it("returns [] when the catalog fetch rejects (best-effort)", async () => {
    couplingCaveats.mockRejectedValue({
      code: "internal",
      message: "boom",
      accountId: null,
      retriable: false,
    });
    const { result } = renderHook(() => useCouplingCaveats("whatsapp"));
    await waitFor(() => expect(couplingCaveats).toHaveBeenCalled());
    expect(result.current).toEqual([]);
  });
});

import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CapabilitiesVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

// The shell without a Tauri backend: what `capabilities` answers now, and the
// `keeper://capabilities-changed` handler the hook registers.
const PHONE: CapabilitiesVm = { ...DEFAULT_CAPABILITIES, sync: true, notes: true };
let served: CapabilitiesVm = PHONE;
let fireChanged: (() => void) | undefined;
// When set, the subscription is not in place until the test releases it.
let holdListen = false;
let releaseListen: (() => void) | undefined;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string) =>
    command === "capabilities"
      ? Promise.resolve(served)
      : Promise.reject(new Error(`unexpected ${command}`)),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: () => void) => {
    if (event === "keeper://capabilities-changed") {
      fireChanged = handler;
    }
    if (holdListen) {
      return new Promise<() => void>((resolve) => {
        releaseListen = () => resolve(() => {});
      });
    }
    return Promise.resolve(() => {});
  },
}));

import { useCapabilitiesHydrate } from "@/hooks/use-capabilities-hydrate";

afterEach(() => {
  served = PHONE;
  fireChanged = undefined;
  holdListen = false;
  releaseListen = undefined;
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useCapabilitiesHydrate", () => {
  it("asks again when Rust says a capability changed: a phone's first sessions zone", async () => {
    renderHook(() => useCapabilitiesHydrate());
    await waitFor(() => expect(capabilitiesStore.getState().hydrated).toBe(true));
    expect(capabilitiesStore.getState().capabilities.sessions).toBe(false);
    await waitFor(() => expect(fireChanged).toBeTypeOf("function"));

    served = { ...PHONE, sessions: true };
    fireChanged?.();

    await waitFor(() => expect(capabilitiesStore.getState().capabilities.sessions).toBe(true));
  });

  it("misses no change that lands while it is still subscribing", async () => {
    holdListen = true;
    renderHook(() => useCapabilitiesHydrate());
    await waitFor(() => expect(capabilitiesStore.getState().hydrated).toBe(true));
    expect(capabilitiesStore.getState().capabilities.sessions).toBe(false);

    // The first zone lands before the subscription is in place: Rust's one
    // event for it reaches nobody.
    served = { ...PHONE, sessions: true };
    await waitFor(() => expect(releaseListen).toBeTypeOf("function"));
    releaseListen?.();

    await waitFor(() => expect(capabilitiesStore.getState().capabilities.sessions).toBe(true));
  });
});

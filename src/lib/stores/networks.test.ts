import { afterEach, describe, expect, it } from "vitest";
import type { NetworkVm } from "@/lib/ipc/client";
import { networksStore } from "@/lib/stores/networks";

function network(name: string): NetworkVm {
  return { name };
}

afterEach(() => {
  networksStore.getState().clear();
});

describe("networksStore", () => {
  it("applySnapshot replaces the list wholesale", () => {
    networksStore.getState().applySnapshot({ networks: [network("Signal"), network("Telegram")] });
    expect(networksStore.getState().networks.map((n) => n.name)).toEqual(["Signal", "Telegram"]);

    // A second snapshot replaces the list (no diff/merge).
    networksStore.getState().applySnapshot({ networks: [network("WhatsApp")] });
    expect(networksStore.getState().networks.map((n) => n.name)).toEqual(["WhatsApp"]);
  });

  it("applySnapshot does not touch the active selection", () => {
    networksStore.getState().setActiveNetwork("Telegram");
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    expect(networksStore.getState().activeNetwork).toBe("Telegram");
  });

  it("setActiveNetwork records and clears the selection", () => {
    networksStore.getState().setActiveNetwork("Signal");
    expect(networksStore.getState().activeNetwork).toBe("Signal");

    networksStore.getState().setActiveNetwork(null);
    expect(networksStore.getState().activeNetwork).toBeNull();
  });

  it("clear resets both the list and the selection", () => {
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    networksStore.getState().setActiveNetwork("Telegram");

    networksStore.getState().clear();
    expect(networksStore.getState().networks).toEqual([]);
    expect(networksStore.getState().activeNetwork).toBeNull();
  });
});

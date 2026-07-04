import { afterEach, describe, expect, it } from "vitest";
import { connectionStore } from "@/lib/stores/connection";

afterEach(() => {
  connectionStore.getState().reset();
});

describe("connectionStore", () => {
  it("defaults to online (no false-offline flash before the first snapshot)", () => {
    expect(connectionStore.getState().status).toBe("online");
  });

  it("applyBatch records the streamed status", () => {
    connectionStore.getState().applyBatch({ status: "offline" });
    expect(connectionStore.getState().status).toBe("offline");
    connectionStore.getState().applyBatch({ status: "online" });
    expect(connectionStore.getState().status).toBe("online");
  });

  it("applyBatch is idempotent for a repeated snapshot", () => {
    connectionStore.getState().applyBatch({ status: "offline" });
    connectionStore.getState().applyBatch({ status: "offline" });
    expect(connectionStore.getState().status).toBe("offline");
  });

  it("reset returns to the online default", () => {
    connectionStore.getState().applyBatch({ status: "offline" });
    connectionStore.getState().reset();
    expect(connectionStore.getState().status).toBe("online");
  });
});

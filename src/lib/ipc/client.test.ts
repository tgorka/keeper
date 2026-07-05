import { afterEach, describe, expect, it, vi } from "vitest";
import type { DemoBatch, IpcError, SpacesSnapshot } from "./client";
import { invoke, setSpaceFilter, subscribe, subscribeInbox } from "./client";

// `vi.mock` is hoisted above imports, so the mock's dependencies must be created
// with `vi.hoisted` to be available when the factory runs. `Channel` is mocked
// as a class whose `onmessage` handler the backend would drive; captured
// instances let the test push batches through it.
const { invokeMock, channelInstances, MockChannel } = vi.hoisted(() => {
  class MockChannelImpl<T> {
    onmessage: ((message: T) => void) | null = null;
    constructor() {
      instances.push(this as MockChannelImpl<unknown>);
    }
  }
  const instances: MockChannelImpl<unknown>[] = [];
  return {
    invokeMock: vi.fn(),
    channelInstances: instances,
    MockChannel: MockChannelImpl,
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
  Channel: MockChannel,
}));

afterEach(() => {
  invokeMock.mockReset();
  channelInstances.length = 0;
});

describe("invoke", () => {
  it("resolves with the command's view model", async () => {
    invokeMock.mockResolvedValueOnce({ message: "pong", ts: 42 });
    await expect(invoke("app_ping")).resolves.toEqual({ message: "pong", ts: 42 });
    expect(invokeMock).toHaveBeenCalledWith("app_ping", undefined);
  });

  it("rejects with the IpcError envelope on failure", async () => {
    const envelope: IpcError = {
      code: "unsupported",
      message: "nope",
      accountId: null,
      retriable: false,
    };
    invokeMock.mockRejectedValueOnce(envelope);
    await expect(invoke("app_ping")).rejects.toEqual(envelope);
  });

  it("wraps a non-envelope rejection as an internal IpcError", async () => {
    invokeMock.mockRejectedValueOnce("raw string boom");
    await expect(invoke("app_ping")).rejects.toMatchObject({
      code: "internal",
      message: "raw string boom",
      retriable: false,
    });
  });
});

describe("subscribe", () => {
  it("forwards batches in order (snapshot before diff) and returns the id", async () => {
    invokeMock.mockResolvedValueOnce(7);
    const received: DemoBatch[] = [];

    const id = await subscribe<DemoBatch>("demo_subscribe", (batch) => {
      received.push(batch);
    });
    expect(id).toBe(7);

    // The mocked backend drives the channel created inside subscribe().
    const channel = channelInstances[0] as {
      onmessage: ((message: DemoBatch) => void) | null;
    };
    expect(channel).toBeDefined();
    channel.onmessage?.({ kind: "snapshot", items: [{ id: "1", label: "Alpha" }] });
    channel.onmessage?.({ kind: "diff", added: [], removed: ["1"] });

    expect(received.map((b) => b.kind)).toEqual(["snapshot", "diff"]);
  });
});

describe("subscribeInbox", () => {
  it("opens five channels and forwards the spaces snapshot to onSpaces", async () => {
    invokeMock.mockResolvedValueOnce(3);
    const spaces: SpacesSnapshot[] = [];

    const id = await subscribeInbox(
      () => {},
      () => {},
      () => {},
      () => {},
      (s) => {
        spaces.push(s);
      },
    );
    expect(id).toBe(3);

    // Five channels created; the command receives all five (inbox/archive/pins/
    // favourites/spaces).
    expect(channelInstances).toHaveLength(5);
    const [, args] = invokeMock.mock.calls[0];
    expect(args).toHaveProperty("spaces");

    // The fifth channel drives the spaces snapshot into onSpaces.
    const spacesChannel = channelInstances[4] as {
      onmessage: ((message: SpacesSnapshot) => void) | null;
    };
    spacesChannel.onmessage?.({
      spaces: [{ accountId: "acctA", spaceId: "!s", name: "Design", avatarUrl: null }],
    });
    expect(spaces).toHaveLength(1);
    expect(spaces[0].spaces[0].name).toBe("Design");
  });
});

describe("setSpaceFilter", () => {
  it("invokes set_space_filter with the selection", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await setSpaceFilter("acctA", "!s");
    expect(invokeMock).toHaveBeenCalledWith("set_space_filter", {
      accountId: "acctA",
      spaceId: "!s",
    });
  });

  it("invokes set_space_filter with nulls to clear", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await setSpaceFilter(null, null);
    expect(invokeMock).toHaveBeenCalledWith("set_space_filter", {
      accountId: null,
      spaceId: null,
    });
  });
});

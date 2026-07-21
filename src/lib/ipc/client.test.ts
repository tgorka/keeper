import { afterEach, describe, expect, it, vi } from "vitest";
import type { DemoBatch, IpcError, NetworksSnapshot, SpacesSnapshot } from "./client";
import {
  chatNotifyModeGet,
  chatNotifyModeSet,
  dndGetGlobal,
  dndSetGlobal,
  invoke,
  networkMuteGet,
  networkMuteSet,
  notifyGetPreviewEnabled,
  notifySetPreviewEnabled,
  recordingStart,
  requestCameraPermission,
  requestMicrophonePermission,
  setNetworkFilter,
  setSpaceFilter,
  subscribe,
  subscribeInbox,
} from "./client";

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
  it("opens six channels and forwards the spaces + networks snapshots", async () => {
    invokeMock.mockResolvedValueOnce(3);
    const spaces: SpacesSnapshot[] = [];
    const networks: NetworksSnapshot[] = [];

    const id = await subscribeInbox(
      () => {},
      () => {},
      () => {},
      () => {},
      (s) => {
        spaces.push(s);
      },
      (n) => {
        networks.push(n);
      },
    );
    expect(id).toBe(3);

    // Six channels created; the command receives all six (inbox/archive/pins/
    // favourites/spaces/networks).
    expect(channelInstances).toHaveLength(6);
    const [, args] = invokeMock.mock.calls[0];
    expect(args).toHaveProperty("spaces");
    expect(args).toHaveProperty("networks");

    // The fifth channel drives the spaces snapshot into onSpaces.
    const spacesChannel = channelInstances[4] as {
      onmessage: ((message: SpacesSnapshot) => void) | null;
    };
    spacesChannel.onmessage?.({
      spaces: [{ accountId: "acctA", spaceId: "!s", name: "Design", avatarUrl: null }],
    });
    expect(spaces).toHaveLength(1);
    expect(spaces[0].spaces[0].name).toBe("Design");

    // The sixth channel (Story 4.6) drives the networks snapshot into onNetworks.
    const networksChannel = channelInstances[5] as {
      onmessage: ((message: NetworksSnapshot) => void) | null;
    };
    networksChannel.onmessage?.({ networks: [{ name: "Telegram" }] });
    expect(networks).toHaveLength(1);
    expect(networks[0].networks[0].name).toBe("Telegram");
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

describe("setNetworkFilter", () => {
  it("invokes set_network_filter with the selected Network name", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await setNetworkFilter("Telegram");
    expect(invokeMock).toHaveBeenCalledWith("set_network_filter", { network: "Telegram" });
  });

  it("invokes set_network_filter with null to clear", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await setNetworkFilter(null);
    expect(invokeMock).toHaveBeenCalledWith("set_network_filter", { network: null });
  });
});

describe("notifyGetPreviewEnabled", () => {
  it("invokes notify_get_preview_enabled and resolves with the boolean", async () => {
    invokeMock.mockResolvedValueOnce(true);
    await expect(notifyGetPreviewEnabled()).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("notify_get_preview_enabled", undefined);
  });
});

describe("notifySetPreviewEnabled", () => {
  it("invokes notify_set_preview_enabled with the enabled flag", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await notifySetPreviewEnabled(false);
    expect(invokeMock).toHaveBeenCalledWith("notify_set_preview_enabled", { enabled: false });
  });
});

describe("dndGetGlobal / dndSetGlobal (Story 10.2)", () => {
  it("dndGetGlobal invokes dnd_get_global and resolves with the boolean", async () => {
    invokeMock.mockResolvedValueOnce(true);
    await expect(dndGetGlobal()).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("dnd_get_global", undefined);
  });

  it("dndSetGlobal invokes dnd_set_global with the enabled flag", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await dndSetGlobal(true);
    expect(invokeMock).toHaveBeenCalledWith("dnd_set_global", { enabled: true });
  });
});

describe("networkMuteGet / networkMuteSet (Story 10.2)", () => {
  it("networkMuteGet invokes network_mute_get with the networkId", async () => {
    invokeMock.mockResolvedValueOnce(false);
    await expect(networkMuteGet("Telegram")).resolves.toBe(false);
    expect(invokeMock).toHaveBeenCalledWith("network_mute_get", { networkId: "Telegram" });
  });

  it("networkMuteSet invokes network_mute_set with networkId + muted", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await networkMuteSet("Telegram", true);
    expect(invokeMock).toHaveBeenCalledWith("network_mute_set", {
      networkId: "Telegram",
      muted: true,
    });
  });
});

describe("chatNotifyModeGet / chatNotifyModeSet (Story 10.2)", () => {
  it("chatNotifyModeGet invokes chat_notify_mode_get with ids and resolves the mode", async () => {
    invokeMock.mockResolvedValueOnce("mention_only");
    await expect(chatNotifyModeGet("acctA", "!r:example.org")).resolves.toBe("mention_only");
    expect(invokeMock).toHaveBeenCalledWith("chat_notify_mode_get", {
      accountId: "acctA",
      roomId: "!r:example.org",
    });
  });

  it("chatNotifyModeSet invokes chat_notify_mode_set with ids + mode", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await chatNotifyModeSet("acctA", "!r:example.org", "mute");
    expect(invokeMock).toHaveBeenCalledWith("chat_notify_mode_set", {
      accountId: "acctA",
      roomId: "!r:example.org",
      mode: "mute",
    });
  });
});

describe("recordingStart / requestMicrophonePermission (Story 19.3)", () => {
  it("recordingStart with every arg omitted sends the honest nulls (defaults in Rust)", async () => {
    invokeMock.mockResolvedValueOnce({ state: "preflight" });
    await recordingStart();
    expect(invokeMock).toHaveBeenCalledWith("recording_start", {
      target: null,
      systemAudio: null,
      microphoneEnabled: null,
      microphoneDeviceId: null,
      cameraEnabled: null,
      cameraDeviceId: null,
      metaTitle: null,
      metaParticipants: null,
      metaNote: null,
    });
  });

  it("recordingStart threads the mic selection through as microphoneEnabled/DeviceId", async () => {
    invokeMock.mockResolvedValueOnce({ state: "preflight" });
    await recordingStart({ kind: "display", displayId: null }, false, true, "X");
    expect(invokeMock).toHaveBeenCalledWith("recording_start", {
      target: { kind: "display", displayId: null },
      systemAudio: false,
      microphoneEnabled: true,
      microphoneDeviceId: "X",
      cameraEnabled: null,
      cameraDeviceId: null,
      metaTitle: null,
      metaParticipants: null,
      metaNote: null,
    });
  });

  it("recordingStart maps a null device id (system default input) verbatim", async () => {
    invokeMock.mockResolvedValueOnce({ state: "preflight" });
    await recordingStart(undefined, true, true, null);
    expect(invokeMock).toHaveBeenCalledWith("recording_start", {
      target: null,
      systemAudio: true,
      microphoneEnabled: true,
      microphoneDeviceId: null,
      cameraEnabled: null,
      cameraDeviceId: null,
      metaTitle: null,
      metaParticipants: null,
      metaNote: null,
    });
  });

  it("requestMicrophonePermission resolves the sidecar-reported tri-state", async () => {
    invokeMock.mockResolvedValueOnce("denied");
    await expect(requestMicrophonePermission()).resolves.toBe("denied");
    expect(invokeMock).toHaveBeenCalledWith("request_microphone_permission", undefined);
  });
});

describe("recordingStart camera args / requestCameraPermission (Story 20.1)", () => {
  it("recordingStart threads the camera selection through as cameraEnabled/DeviceId", async () => {
    invokeMock.mockResolvedValueOnce({ state: "preflight" });
    await recordingStart(undefined, true, false, null, true, "CAM");
    expect(invokeMock).toHaveBeenCalledWith("recording_start", {
      target: null,
      systemAudio: true,
      microphoneEnabled: false,
      microphoneDeviceId: null,
      cameraEnabled: true,
      cameraDeviceId: "CAM",
      metaTitle: null,
      metaParticipants: null,
      metaNote: null,
    });
  });

  it("recordingStart maps a null camera id (system default camera) verbatim", async () => {
    invokeMock.mockResolvedValueOnce({ state: "preflight" });
    await recordingStart(undefined, true, false, null, true, null);
    expect(invokeMock).toHaveBeenCalledWith("recording_start", {
      target: null,
      systemAudio: true,
      microphoneEnabled: false,
      microphoneDeviceId: null,
      cameraEnabled: true,
      cameraDeviceId: null,
      metaTitle: null,
      metaParticipants: null,
      metaNote: null,
    });
  });

  it("requestCameraPermission resolves the sidecar-reported tri-state", async () => {
    invokeMock.mockResolvedValueOnce("denied");
    await expect(requestCameraPermission()).resolves.toBe("denied");
    expect(invokeMock).toHaveBeenCalledWith("request_camera_permission", undefined);
  });
});

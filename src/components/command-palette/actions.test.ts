import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stub the IPC client and the shared recording-control module so dispatch is
// assertable without a live backend (the palette component test covers the
// UI-side dispatch path; this covers the handler wiring itself).
const recordingRevealFolder = vi.fn();
const startRecordingWithCurrentSelections = vi.fn();
const stopRecording = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  archiveRoom: vi.fn(),
  chatNotifyModeSet: vi.fn(),
  favoriteRoom: vi.fn(),
  incognitoGet: vi.fn(),
  incognitoGetGlobal: vi.fn(),
  incognitoSetChat: vi.fn(),
  incognitoSetGlobal: vi.fn(),
  markRoomRead: vi.fn(),
  markRoomUnread: vi.fn(),
  pinRoom: vi.fn(),
  recordingRevealFolder: () => recordingRevealFolder(),
  syncNow: vi.fn(),
  unarchiveRoom: vi.fn(),
  unfavoriteRoom: vi.fn(),
  unpinRoom: vi.fn(),
}));

vi.mock("@/lib/recording-control", () => ({
  startRecordingWithCurrentSelections: () => startRecordingWithCurrentSelections(),
  stopRecording: () => stopRecording(),
}));

import { dispatchPaletteAction, paletteActionHandlers } from "@/components/command-palette/actions";
import { primaryViewStore } from "@/lib/stores/primary-view";

beforeEach(() => {
  recordingRevealFolder.mockReset().mockResolvedValue(undefined);
  startRecordingWithCurrentSelections.mockReset().mockResolvedValue(undefined);
  stopRecording.mockReset().mockResolvedValue(undefined);
  primaryViewStore.setState({ view: "inbox" });
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  vi.clearAllMocks();
});

describe("recording palette handlers (Story 20.4)", () => {
  it("registers a handler for each recording action id in the Rust registry", () => {
    for (const id of ["recording-start", "recording-stop", "recording-open-folder"]) {
      expect(paletteActionHandlers[id], `handler for ${id}`).toBeTypeOf("function");
    }
  });

  it("recording-start switches to the Recording view and starts with current selections", async () => {
    await dispatchPaletteAction("recording-start", null);
    expect(primaryViewStore.getState().view).toBe("recording");
    expect(startRecordingWithCurrentSelections).toHaveBeenCalledTimes(1);
  });

  it("recording-stop routes through the shared stopRecording", async () => {
    await dispatchPaletteAction("recording-stop", null);
    expect(stopRecording).toHaveBeenCalledTimes(1);
    expect(primaryViewStore.getState().view).toBe("inbox");
  });

  it("recording-open-folder reveals the effective destination folder", async () => {
    await dispatchPaletteAction("recording-open-folder", null);
    expect(recordingRevealFolder).toHaveBeenCalledTimes(1);
  });

  it("recording-open-folder swallows a reveal failure (never crashes the palette)", async () => {
    recordingRevealFolder.mockRejectedValue(new Error("no finder"));
    await expect(dispatchPaletteAction("recording-open-folder", null)).resolves.toBeUndefined();
  });
});

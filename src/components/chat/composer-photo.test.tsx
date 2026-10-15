import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Composer } from "@/components/chat/composer";
import { logFailure } from "@/lib/failure";
import type * as IpcClient from "@/lib/ipc/client";
import { attachmentsStore } from "@/lib/stores/attachments";
import { draftsStore } from "@/lib/stores/drafts";

const layout = vi.hoisted(() => ({ phone: false }));
vi.mock("@/hooks/use-shell-layout", () => ({ useShellLayout: () => layout }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));
vi.mock("@/lib/failure", () => ({ logFailure: vi.fn(() => vi.fn()), sayFailure: vi.fn() }));
vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  loadDraft: vi.fn(async () => null),
  saveDraft: vi.fn(async () => {}),
  clearDraft: vi.fn(async () => {}),
  loadRemoteDraft: vi.fn(async () => null),
  mirrorDraft: vi.fn(async () => {}),
  clearDraftMirror: vi.fn(async () => {}),
  openCameraSettings: vi.fn(async () => {}),
}));

const originalMediaDevices = Object.getOwnPropertyDescriptor(navigator, "mediaDevices");
const getUserMedia = vi.fn();
const report = vi.fn();

beforeEach(() => {
  layout.phone = false;
  attachmentsStore.getState().clear();
  draftsStore.getState().clear();
  vi.clearAllMocks();
  vi.mocked(logFailure).mockReturnValue(report);
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia, enumerateDevices: vi.fn(async () => []) },
  });
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
});

afterEach(() => {
  vi.restoreAllMocks();
  if (originalMediaDevices) {
    Object.defineProperty(navigator, "mediaDevices", originalMediaDevices);
  } else {
    Reflect.deleteProperty(navigator, "mediaDevices");
  }
  attachmentsStore.getState().clear();
  draftsStore.getState().clear();
});

function mountComposer() {
  const send = vi.fn(async () => {});
  render(
    <Composer
      accountId="a"
      roomId="!photos:example.org"
      onSend={vi.fn()}
      onSendAttachments={send}
    />,
  );
  return send;
}

describe("Composer photos", () => {
  it("explains a denied camera in the sheet and reports the refusal", async () => {
    const refusal = new DOMException("Denied", "NotAllowedError");
    getUserMedia.mockRejectedValue(refusal);
    mountComposer();
    fireEvent.click(screen.getByRole("button", { name: "Take photo" }));
    const dialog = screen.getByRole("dialog", { name: "Take photo" });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(/allow it.*camera/i);
    expect(within(dialog).getByRole("button", { name: "Open Camera settings" })).toBeEnabled();
    expect(within(dialog).getByRole("button", { name: "Capture" })).toBeDisabled();
    expect(logFailure).toHaveBeenCalledWith("composer.camera");
    expect(report).toHaveBeenCalledWith(refusal);
  });

  it("captures one JPEG into the tray, stops the camera and sends it with the caption", async () => {
    const stop = vi.fn();
    getUserMedia.mockResolvedValue({ getTracks: () => [{ stop, addEventListener: vi.fn() }] });
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      drawImage: vi.fn(),
    } as unknown as CanvasRenderingContext2D);
    const bytes = new Uint8Array([255, 216, 255, 217]).buffer;
    const blob = new Blob([bytes], { type: "image/jpeg" });
    Object.defineProperty(blob, "arrayBuffer", { value: async () => bytes });
    vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((done) => {
      done(blob);
    });
    const send = mountComposer();
    fireEvent.change(screen.getByLabelText("Message"), { target: { value: "  My view  " } });
    fireEvent.click(screen.getByRole("button", { name: "Take photo" }));
    const video = screen.getByLabelText("Camera preview");
    Object.defineProperties(video, { videoWidth: { value: 640 }, videoHeight: { value: 480 } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Capture" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Capture" }));
    const tray = await screen.findByRole("list", { name: "Pending attachments" });
    expect(within(tray).getByText(/^photo-\d{8}-\d{6}\.jpg$/)).toBeInTheDocument();
    expect(attachmentsStore.getState().pending).toEqual([
      expect.objectContaining({ kind: "bytes", bytes, mime: "image/jpeg", size: 4 }),
    ]);
    expect(stop).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(send).toHaveBeenCalledWith(
        [expect.objectContaining({ kind: "bytes", bytes, mime: "image/jpeg" })],
        "My view",
      ),
    );
  });

  it("omits desktop camera capture when the browser has no camera API", () => {
    Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: undefined });
    mountComposer();
    expect(screen.queryByRole("button", { name: "Take photo" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose photo" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Attach file" })).toBeEnabled();
  });

  it("adds a phone capture as bytes and allows the same photo to be selected again", async () => {
    layout.phone = true;
    Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: undefined });
    const send = mountComposer();
    const input = screen.getByLabelText<HTMLInputElement>("Take a photo file");
    const bytes = new Uint8Array([10, 20, 30]).buffer;
    const file = new File([bytes], "garden.jpg", { type: "image/jpeg" });
    Object.defineProperty(file, "arrayBuffer", { value: async () => bytes });
    // jsdom disallows setting a native file value. Model its selection/reset state.
    Object.defineProperty(input, "value", { writable: true, value: "garden.jpg" });
    fireEvent.change(input, { target: { files: [file] } });
    await screen.findByRole("list", { name: "Pending attachments" });
    expect(attachmentsStore.getState().pending).toEqual([
      expect.objectContaining({ kind: "bytes", bytes, filename: "garden.jpg", mime: "image/jpeg" }),
    ]);
    expect(input.value).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(send).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Pending attachments" })).toBeNull(),
    );
    fireEvent.change(input, { target: { files: [file] } });
    expect(await screen.findByText("garden.jpg")).toBeInTheDocument();
    expect(getUserMedia).not.toHaveBeenCalled();
  });

  it("sends a HEIC this webview cannot decode as it is, for the host to explain", async () => {
    layout.phone = true;
    Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: undefined });
    mountComposer();
    const input = screen.getByLabelText<HTMLInputElement>("Choose photo files");
    const bytes = new Uint8Array([0, 0, 0, 24]).buffer;
    const file = new File([bytes], "IMG_0042.HEIC", { type: "image/heic" });
    Object.defineProperty(file, "arrayBuffer", { value: async () => bytes });
    fireEvent.change(input, { target: { files: [file] } });
    await screen.findByRole("list", { name: "Pending attachments" });
    expect(attachmentsStore.getState().pending).toEqual([
      expect.objectContaining({
        kind: "bytes",
        bytes,
        filename: "IMG_0042.HEIC",
        mime: "image/heic",
      }),
    ]);
  });
});

it("stops the old camera on device change and releases a replacement arriving after close", async () => {
  const firstStop = vi.fn();
  const lateStop = vi.fn();
  let release: (stream: MediaStream) => void = () => {};
  const replacement = new Promise<MediaStream>((resolve) => {
    release = resolve;
  });
  getUserMedia
    .mockResolvedValueOnce({ getTracks: () => [{ stop: firstStop, addEventListener: vi.fn() }] })
    .mockReturnValueOnce(replacement);
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: {
      getUserMedia,
      enumerateDevices: async () => [
        { kind: "videoinput", deviceId: "rear", label: "Rear camera" },
      ],
    },
  });
  mountComposer();
  fireEvent.click(screen.getByRole("button", { name: "Take photo" }));
  fireEvent.change(await screen.findByRole("combobox", { name: "Camera" }), {
    target: { value: "rear" },
  });
  expect(firstStop).toHaveBeenCalledTimes(1);
  expect(getUserMedia).toHaveBeenLastCalledWith({
    video: { deviceId: { exact: "rear" } },
    audio: false,
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await act(async () => {
    release({ getTracks: () => [{ stop: lateStop }] } as unknown as MediaStream);
    await replacement;
  });
  expect(lateStop).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument();
});

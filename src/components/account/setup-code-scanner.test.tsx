import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("zxing-wasm/reader", () => ({
  prepareZXingModule: vi.fn(),
  readBarcodes: vi.fn(),
}));

vi.mock("@/lib/ipc/client", () => ({
  accountSetupResolve: vi.fn(() => new Promise(() => {})),
  accountSetupConfirm: vi.fn(),
  accountCancelSignIn: vi.fn(),
  openCameraSettings: vi.fn(),
}));

import { prepareZXingModule, readBarcodes } from "zxing-wasm/reader";
import {
  AccountSetupSheet,
  SCAN_DENIED,
  SCAN_FAILED,
  SCAN_HINT,
  SCAN_OPEN_CAMERA_SETTINGS_LABEL,
  SCAN_SETUP_CODE_LABEL,
  SETUP_CANCEL_LABEL,
  SetupLinkField,
} from "@/components/account/account-setup-sheet";
import { KeeperAccountDialog } from "@/components/account/keeper-account-dialog";
// Imported here as well as lazily by the field, so the decoder's one-time
// configuration can be read before any test clears the mocks.
import "@/components/account/setup-code-scanner";
import { accountSetupResolve, openCameraSettings } from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";

const zxingSetup = vi.mocked(prepareZXingModule).mock.calls[0]?.[0];
const mockRead = vi.mocked(readBarcodes);

/** A decoded code is a link like any pasted one; the scanner parses none of it. */
const CODE = "keeper://setup?descriptor=https%3A%2F%2Fid.acme.dev%2Fk.json";

interface FakeCamera {
  getUserMedia: ReturnType<typeof vi.fn>;
  stops: ReturnType<typeof vi.fn>[];
  tracks: EventTarget[];
  /** Answer a `"deferred"` camera's request now. */
  grant: () => void;
}

/**
 * A camera whose stream carries two tracks, each recording `stop()` and able
 * to fire `ended`. `"deferred"` holds the answer until {@link FakeCamera.grant}.
 */
function stubCamera(outcome: "grant" | "deferred" | DOMException = "grant"): FakeCamera {
  const stops = [vi.fn(), vi.fn()];
  const tracks = stops.map((stop) => Object.assign(new EventTarget(), { stop }));
  const stream = { getTracks: () => tracks };
  // The project's lib predates `Promise.withResolvers`, so the resolver is taken by hand.
  let resolve: (value: typeof stream) => void = () => {};
  const answer = new Promise<typeof stream>((settle) => {
    resolve = settle;
  });
  if (outcome === "grant") {
    resolve(stream);
  }
  const getUserMedia = vi.fn(() =>
    outcome instanceof DOMException ? Promise.reject(outcome) : answer,
  );
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia },
  });
  return { getUserMedia, stops, tracks, grant: () => resolve(stream) };
}

/** Start a scan from the field and wait until the preview is live. */
async function startScanning(): Promise<FakeCamera> {
  const camera = stubCamera();
  // Nothing in view: the scanner keeps looking until something stops it.
  mockRead.mockResolvedValue([]);
  renderField();
  fireEvent.click(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL }));
  expect(await screen.findByText(SCAN_HINT)).toBeInTheDocument();
  return camera;
}

function expectCameraOff(camera: FakeCamera) {
  for (const stop of camera.stops) {
    expect(stop).toHaveBeenCalled();
  }
}

function renderField() {
  render(
    <>
      <SetupLinkField id="scan-test-link" />
      <AccountSetupSheet />
    </>,
  );
}

beforeEach(() => {
  accountStore.setState({ vm: NO_ACCOUNT, setupLink: null, entryOpen: false });
  vi.spyOn(HTMLVideoElement.prototype, "play").mockResolvedValue(undefined);
  vi.spyOn(HTMLVideoElement.prototype, "videoWidth", "get").mockReturnValue(1920);
  vi.spyOn(HTMLVideoElement.prototype, "videoHeight", "get").mockReturnValue(1080);
  const frame = { data: new Uint8ClampedArray(4), width: 1, height: 1, colorSpace: "srgb" };
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    () => ({ drawImage: vi.fn(), getImageData: () => frame }) as never,
  );
});

afterEach(() => {
  Reflect.deleteProperty(navigator, "mediaDevices");
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

describe("SetupLinkField's scan", () => {
  it("is offered only where the webview can reach a camera", () => {
    // jsdom, like WKWebView without a camera usage string, has no mediaDevices.
    const { unmount } = render(<SetupLinkField id="scan-test-link" />);
    expect(screen.queryByRole("button", { name: SCAN_SETUP_CODE_LABEL })).not.toBeInTheDocument();
    unmount();

    stubCamera();
    render(<SetupLinkField id="scan-test-link" />);
    expect(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL })).toBeInTheDocument();
  });

  it("opens the setup sheet on exactly the decoded link, with the camera off", async () => {
    const camera = stubCamera();
    mockRead.mockResolvedValue([{ text: CODE } as never]);
    renderField();

    fireEvent.click(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL }));

    // The sheet resolves what was scanned the way it resolves a paste.
    await waitFor(() => expect(accountSetupResolve).toHaveBeenCalledWith(CODE));
    expect(accountStore.getState().setupLink).toBe(CODE);
    expect(camera.getUserMedia).toHaveBeenCalledWith(
      expect.objectContaining({
        audio: false,
        video: expect.objectContaining({ facingMode: "environment" }),
      }),
    );
    expectCameraOff(camera);
    // And the scanner is gone behind the sheet, not left running under it.
    expect(screen.queryByText(SCAN_HINT)).not.toBeInTheDocument();
  });

  it("serves the decoder's wasm from the app bundle, never from the CDN", () => {
    // zxing's own default fetches it from jsDelivr: an undisclosed egress (AD-53).
    const locateFile = zxingSetup?.overrides?.locateFile;
    expect(locateFile).toBeTypeOf("function");
    const url = locateFile?.(
      "zxing_reader.wasm",
      "https://fastly.jsdelivr.net/npm/zxing-wasm/dist/reader/",
    );
    expect(url).not.toContain("jsdelivr");
    expect(url).toMatch(/zxing_reader.*\.wasm/);
  });

  it("says how to allow a denied camera, and opens Camera settings", async () => {
    stubCamera(new DOMException("Permission denied", "NotAllowedError"));
    // A deep link that fails is swallowed, not thrown at the person.
    vi.mocked(openCameraSettings).mockRejectedValue(new Error("no System Settings"));
    renderField();

    fireEvent.click(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL }));

    expect(await screen.findByRole("alert")).toHaveTextContent(SCAN_DENIED);
    fireEvent.click(screen.getByRole("button", { name: SCAN_OPEN_CAMERA_SETTINGS_LABEL }));
    expect(openCameraSettings).toHaveBeenCalledTimes(1);
  });

  it("Cancel turns the camera off and gives the idle field back", async () => {
    const camera = await startScanning();
    // The paste field is still usable while the camera runs.
    expect(screen.getByLabelText("Paste a setup link")).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: SETUP_CANCEL_LABEL }));

    expectCameraOff(camera);
    expect(screen.queryByText(SCAN_HINT)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL })).toBeInTheDocument();
    expect(accountStore.getState().setupLink).toBeNull();
  });

  it("turns off a camera that answers only after Cancel", async () => {
    const camera = stubCamera("deferred");
    renderField();
    fireEvent.click(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CANCEL_LABEL }));
    expect(camera.getUserMedia).toHaveBeenCalledTimes(1);

    // The stream lands on a scanner that is already gone: it belongs to nobody.
    await act(async () => camera.grant());
    await waitFor(() => expectCameraOff(camera));
  });

  // Closing keeper's window hides it; nothing unmounts, so only the page says
  // the scanner's surface is gone.
  it.each([
    [
      "hidden",
      () => {
        vi.spyOn(Document.prototype, "visibilityState", "get").mockReturnValue("hidden");
        document.dispatchEvent(new Event("visibilitychange"));
      },
    ],
    ["unloaded", () => window.dispatchEvent(new Event("pagehide"))],
  ])("turns the camera off and puts the scan away when the page is %s", async (_, leave) => {
    const camera = await startScanning();

    act(leave);

    expectCameraOff(camera);
    expect(screen.queryByText(SCAN_HINT)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL })).toBeInTheDocument();
  });

  it("says the camera failed when it stops mid-scan", async () => {
    const camera = await startScanning();

    // Unplugged, or taken by another app.
    act(() => {
      camera.tracks[0]?.dispatchEvent(new Event("ended"));
    });

    expect(await screen.findByRole("alert")).toHaveTextContent(SCAN_FAILED);
    expect(screen.queryByText(SCAN_HINT)).not.toBeInTheDocument();
    expectCameraOff(camera);
  });

  it("does not blame the camera grant for a failure after the camera started", async () => {
    const camera = stubCamera();
    // Playback refused by name — but the grant is fine, so System Settings is the wrong advice.
    vi.mocked(HTMLVideoElement.prototype.play).mockRejectedValue(
      new DOMException("Autoplay refused", "NotAllowedError"),
    );
    renderField();
    fireEvent.click(screen.getByRole("button", { name: SCAN_SETUP_CODE_LABEL }));

    expect(await screen.findByRole("alert")).toHaveTextContent(SCAN_FAILED);
    expect(
      screen.queryByRole("button", { name: SCAN_OPEN_CAMERA_SETTINGS_LABEL }),
    ).not.toBeInTheDocument();
    expectCameraOff(camera);
  });

  it("turns the camera off when the surface it runs in closes", async () => {
    const camera = stubCamera();
    mockRead.mockResolvedValue([]);
    render(<KeeperAccountDialog />);
    act(() => accountStore.getState().openEntry());

    fireEvent.click(await screen.findByRole("button", { name: SCAN_SETUP_CODE_LABEL }));
    expect(await screen.findByText(SCAN_HINT)).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expectCameraOff(camera);
  });
});

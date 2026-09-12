import type { CaptureResult, PostHogConfig } from "posthog-js";
import { buildNetworkRequestOptions } from "posthog-js/lib/src/extensions/replay/external/config";
import { defaultConfig, PostHog } from "posthog-js/lib/src/posthog-core";
import { beforeEach, expect, it, vi } from "vitest";
import { createStudyRecorder, studyEvent } from "./recorder";

const sdk = vi.hoisted(() => ({
  init: vi.fn(),
  startSessionRecording: vi.fn(),
  sessionRecordingStarted: vi.fn(),
  capture: vi.fn(),
  set_config: vi.fn(),
  stopSessionRecording: vi.fn(),
  shutdown: vi.fn(),
}));
vi.mock("posthog-js/dist/module.no-external", () => ({ default: sdk }));
vi.mock("posthog-js/dist/posthog-recorder", () => ({}));
const CONFIG = { host: "https://us.i.posthog.com", projectToken: "phc_synthetic_test" };
let options: Partial<PostHogConfig>;
beforeEach(() => {
  vi.resetAllMocks();
  sdk.init.mockImplementation((_token: string, config: Partial<PostHogConfig>) => {
    options = config;
  });
});

it("drops unintended SDK events rather than capturing exception or arbitrary event payloads", () => {
  const event: CaptureResult = {
    uuid: "synthetic",
    event: "$exception",
    properties: { message: "PRIVATE_SENTINEL" },
  };
  expect(studyEvent(event, CONFIG.projectToken)).toBeNull();
  expect(studyEvent({ ...event, event: "arbitrary-name" }, CONFIG.projectToken)).toBeNull();
});

it("preserves heatmap geometry without source URLs, unexpected point fields or person properties", () => {
  const event: CaptureResult = {
    uuid: "synthetic",
    event: "$$heatmap",
    $set: { email: "PRIVATE_SENTINEL" },
    $set_once: { path: "PRIVATE_SENTINEL" },
    properties: {
      distinct_id: "random-study-id",
      $session_id: "random-session",
      $window_id: "random-window",
      token: "PRIVATE_SENTINEL",
      $current_url: "https://local.test/?PRIVATE_SENTINEL",
      $referrer: "PRIVATE_SENTINEL",
      $viewport_width: 100,
      $viewport_height: 200,
      $heatmap_data: {
        "https://local.test/?PRIVATE_SENTINEL": [
          { x: 21, y: 34, type: "click", target_fixed: false, private: "PRIVATE_SENTINEL" },
          { x: Number.NaN, y: 34, type: "click", target_fixed: false },
          { x: 12, y: 34, type: "PRIVATE_SENTINEL", target_fixed: false },
        ],
      },
    },
  };
  const result = studyEvent(event, CONFIG.projectToken);
  expect(result?.properties.$heatmap_data).toEqual({
    "https://keeper.invalid/synthetic-study": [
      { x: 21, y: 34, type: "click", target_fixed: false },
    ],
  });
  const properties = result?.properties;
  const point = properties?.$heatmap_data["https://keeper.invalid/synthetic-study"][0];
  // Ingestion needs the viewport geometry; coordinates alone are silently discarded.
  expect([point?.x / properties?.$viewport_width, point?.y / properties?.$viewport_height]).toEqual(
    [0.21, 0.17],
  );
  expect(result?.properties.token).toBe(CONFIG.projectToken);
  expect(JSON.stringify(result)).not.toContain("PRIVATE_SENTINEL");
  expect(result).not.toHaveProperty("$set");
  expect(result).not.toHaveProperty("$set_once");
});

it.each([
  "$snapshot",
  "$pageview",
])("injects only the configured public ingestion token for %s", (event) => {
  const result = studyEvent(
    { uuid: "synthetic", event, properties: { token: "PRIVATE_SENTINEL" } },
    CONFIG.projectToken,
  );
  expect(result?.properties.token).toBe(CONFIG.projectToken);
  expect(JSON.stringify(result)).not.toContain("PRIVATE_SENTINEL");
});

it("routes the SDK's regional config lookup to the sole disclosed destination", () => {
  createStudyRecorder(CONFIG, vi.fn()).start();
  const instance = new PostHog();
  instance.set_config({
    api_host: options.api_host,
    rewriteRequestPath: options.rewriteRequestPath,
  });
  expect(instance.requestRouter.endpointFor("assets", `/array/${CONFIG.projectToken}/config`)).toBe(
    `${CONFIG.host}/array/${CONFIG.projectToken}/config`,
  );
});

it("vetoes network content even when remote headers, bodies and performance are enabled", () => {
  createStudyRecorder(CONFIG, vi.fn()).start();
  const merged = buildNetworkRequestOptions(
    {
      ...defaultConfig(),
      api_host: CONFIG.host,
      capture_performance: options.capture_performance,
      session_recording: options.session_recording ?? {},
    },
    {
      recordHeaders: true,
      recordBody: true,
      recordPerformance: true,
    },
  );
  expect(merged.recordInitialRequests).toBe(false);
  expect(
    merged.maskRequestFn?.({
      name: "https://private.example/?PRIVATE_SENTINEL",
      duration: 10,
      startTime: 0,
      entryType: "resource",
      requestHeaders: { "x-private": "PRIVATE_SENTINEL" },
      requestBody: "PRIVATE_SENTINEL",
      responseBody: "PRIVATE_SENTINEL",
    }),
  ).toBeUndefined();
});

it.each([
  "event size",
  "session bytes",
  "session events",
])("ends capture rather than silently dropping replay on the %s budget", (budget) => {
  const failure = vi.fn();
  createStudyRecorder(CONFIG, failure).start();
  const send = options.before_send;
  if (typeof send !== "function") throw new Error("capture boundary unavailable");
  const size = budget === "event size" ? 180_000 : budget === "session bytes" ? 100_000 : 0;
  const event: CaptureResult = {
    uuid: "synthetic",
    event: "$snapshot",
    properties: { $snapshot_data: "x".repeat(size) },
  };
  for (let count = 0; count < 2_001 && failure.mock.calls.length === 0; count++) send(event);
  expect(failure).toHaveBeenCalledTimes(1);
  expect(send({ uuid: "late", event: "$pageview", properties: {} })).toBeNull();
});

it("stops on ingestion errors and never emits the pageview before replay readiness", () => {
  const failure = vi.fn();
  const recorder = createStudyRecorder(CONFIG, failure);
  recorder.start();
  expect(sdk.capture).not.toHaveBeenCalled();
  recorder.ready();
  expect(sdk.capture).toHaveBeenCalledWith("$pageview");
  options.on_request_error?.({ statusCode: 401, text: "PRIVATE_SENTINEL" });
  expect(failure).toHaveBeenCalledTimes(1);
});

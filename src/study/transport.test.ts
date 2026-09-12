import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installStudyTransport, type StudyTransport } from "./transport";

const fetchBefore = window.fetch;
const openBefore = window.XMLHttpRequest.prototype.open;
const sendBefore = window.XMLHttpRequest.prototype.send;
const beaconBefore = Object.getOwnPropertyDescriptor(window.navigator, "sendBeacon");
const network = vi.fn<typeof fetch>();
let transport: StudyTransport | undefined;
beforeEach(() => {
  network.mockReset();
  network.mockResolvedValue(new Response("{}", { status: 200 }));
  window.fetch = network;
});
afterEach(() => {
  transport?.stop();
  window.fetch = fetchBefore;
  window.XMLHttpRequest.prototype.open = openBefore;
  window.XMLHttpRequest.prototype.send = sendBefore;
  if (beaconBefore) Object.defineProperty(window.navigator, "sendBeacon", beaconBefore);
  else Reflect.deleteProperty(window.navigator, "sendBeacon");
});

it("aborts in-flight sends and refuses late fetch, beacon and XHR after Stop", async () => {
  let signal: AbortSignal | null | undefined;
  network.mockImplementation((_input, init) => {
    signal = init?.signal;
    return new Promise((_resolve, reject) =>
      signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")), {
        once: true,
      }),
    );
  });
  transport = installStudyTransport("https://us.i.posthog.com", vi.fn());
  const inFlight = window.fetch("https://us.i.posthog.com/e/", {
    method: "POST",
    body: "synthetic",
  });
  const rejected = expect(inFlight).rejects.toHaveProperty("name", "AbortError");
  transport.stop();
  await rejected;
  expect(signal?.aborted).toBe(true);
  await expect(
    window.fetch("https://us.i.posthog.com/e/", { method: "POST" }),
  ).rejects.toHaveProperty("name", "AbortError");
  navigator.sendBeacon("https://us.i.posthog.com/e/", "synthetic");
  const xhr = new XMLHttpRequest();
  xhr.open("POST", "https://us.i.posthog.com/e/");
  expect(() => xhr.send("synthetic")).toThrow();
  expect(network).toHaveBeenCalledTimes(1);
});

it("cannot redirect, send cookies or referrers, load scripts, or follow another destination", async () => {
  transport = installStudyTransport("https://us.i.posthog.com", vi.fn());
  await window.fetch("https://us.i.posthog.com/array/phc_test/config");
  expect(network).toHaveBeenCalledWith(
    "https://us.i.posthog.com/array/phc_test/config",
    expect.objectContaining({
      credentials: "omit",
      referrerPolicy: "no-referrer",
      redirect: "error",
      keepalive: false,
    }),
  );
  // The live public config selects this ingestion endpoint instead of legacy /e/.
  await window.fetch("https://us.i.posthog.com/i/v0/e/", { method: "POST" });
  await expect(window.fetch("https://us.i.posthog.com/static/recorder.js")).rejects.toThrow();
  await expect(window.fetch("https://other.example/e/")).rejects.toThrow();
  await expect(window.fetch("https://us.i.posthog.com/flags/")).rejects.toThrow();
  expect(network).toHaveBeenCalledTimes(2);
});

it("keeps local Tauri stop IPC working behind a closed transport fence", async () => {
  transport = installStudyTransport("https://us.i.posthog.com", vi.fn());
  transport.stop();
  await window.fetch("http://ipc.localhost/telemetry_study_stop", { method: "POST" });
  expect(network).toHaveBeenCalledTimes(1);
});

it.each([
  401, 500,
])("ends the session on HTTP %s without reading a response body", async (status) => {
  const failure = vi.fn();
  const response = new Response("PRIVATE_SENTINEL", { status });
  const readBody = vi.spyOn(response, "text");
  network.mockResolvedValue(response);
  transport = installStudyTransport("https://us.i.posthog.com", failure);
  await expect(window.fetch("https://us.i.posthog.com/e/")).rejects.toThrow(
    "Study request refused",
  );
  await expect(window.fetch("https://us.i.posthog.com/e/")).rejects.toThrow();
  expect(failure).toHaveBeenCalledTimes(1);
  expect(network).toHaveBeenCalledTimes(1);
  expect(readBody).not.toHaveBeenCalled();
});

it("a concurrency refusal aborts the outstanding burst and fences every retry", async () => {
  const failure = vi.fn();
  const signals: AbortSignal[] = [];
  network.mockImplementation(
    (_input, init) =>
      new Promise((_resolve, reject) => {
        const signal = init?.signal;
        if (!signal) throw new Error("missing cancellation");
        signals.push(signal);
        signal.addEventListener("abort", () => reject(new Error("PRIVATE_SENTINEL")));
      }),
  );
  transport = installStudyTransport("https://us.i.posthog.com", failure);
  const burst = Array.from({ length: 5 }, () => window.fetch("https://us.i.posthog.com/s/"));
  const result = await Promise.allSettled(burst);
  expect(result.every((item) => item.status === "rejected")).toBe(true);
  expect(signals.every((signal) => signal.aborted)).toBe(true);
  expect(failure).toHaveBeenCalledTimes(1);
  await expect(window.fetch("https://us.i.posthog.com/e/")).rejects.toThrow();
  expect(network).toHaveBeenCalledTimes(4);
});

it("the cumulative request budget ends the session instead of silently retrying", async () => {
  const failure = vi.fn();
  transport = installStudyTransport("https://us.i.posthog.com", failure);
  for (let count = 0; count < 600; count++) await window.fetch("https://us.i.posthog.com/e/");
  await expect(window.fetch("https://us.i.posthog.com/e/")).rejects.toThrow();
  await expect(window.fetch("https://us.i.posthog.com/e/")).rejects.toThrow();
  expect(network).toHaveBeenCalledTimes(600);
  expect(failure).toHaveBeenCalledTimes(1);
});

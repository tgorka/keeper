import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { startStudyCapture } from "./session";

const ports = vi.hoisted(() => ({
  config: vi.fn(),
  stop: vi.fn(),
  transport: vi.fn(),
  recorder: vi.fn(),
}));
vi.mock("./commands", () => ({
  telemetryStudyConfig: ports.config,
  telemetryStudyStop: ports.stop,
}));
vi.mock("./transport", () => ({ installStudyTransport: ports.transport }));
vi.mock("./recorder", () => ({ createStudyRecorder: ports.recorder }));
const CONFIG = { host: "https://us.i.posthog.com", projectToken: "phc_synthetic_test" };
let sending = false;
let recording = false;
let order: string[];
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  sending = false;
  recording = false;
  order = [];
  ports.config.mockResolvedValue(CONFIG);
  ports.transport.mockImplementation(() => {
    sending = true;
    return {
      stop: () => {
        sending = false;
        order.push("network closed");
      },
    };
  });
  ports.recorder.mockImplementation(() => {
    return {
      start: () => {
        recording = true;
      },
      recording: () => recording,
      ready: vi.fn(),
      stop: async () => {
        expect(sending).toBe(false);
        recording = false;
        order.push("recorder stopped");
      },
    };
  });
  ports.stop.mockImplementation(async () => {
    expect(sending).toBe(false);
    expect(recording).toBe(false);
    order.push("disclosure cleared");
  });
});
afterEach(() => {
  vi.useRealTimers();
});

it("a stop while local config is pending cannot initialize the SDK later", async () => {
  // The application targets ES2020, which has no Promise.withResolvers.
  let resolveConfig: (config: typeof CONFIG) => void = () => {
    throw new Error("not pending");
  };
  ports.config.mockReturnValue(
    new Promise((resolve) => {
      resolveConfig = resolve;
    }),
  );
  const abort = new AbortController();
  const pending = startStudyCapture(abort.signal, vi.fn());
  const refused = expect(pending).rejects.toThrow();
  abort.abort();
  resolveConfig(CONFIG);
  await refused;
  expect(ports.transport).not.toHaveBeenCalled();
  expect(ports.recorder).not.toHaveBeenCalled();
  expect(order).toEqual(["disclosure cleared"]);
});

it("revocation synchronously closes network before SDK teardown and backend disclosure", async () => {
  const abort = new AbortController();
  const pending = startStudyCapture(abort.signal, vi.fn());
  await vi.waitFor(() => expect(recording).toBe(true));
  await vi.advanceTimersByTimeAsync(100);
  const session = await pending;
  abort.abort();
  expect(sending).toBe(false);
  await session.stop();
  expect(order).toEqual(["network closed", "recorder stopped", "disclosure cleared"]);
  await session.stop();
  expect(ports.stop).toHaveBeenCalledTimes(1);
});

it("unavailable replay times out and clears capture instead of reporting an active study", async () => {
  ports.recorder.mockImplementation(() => ({
    start: () => {},
    recording: () => false,
    stop: async () => {
      order.push("recorder stopped");
    },
  }));
  const pending = startStudyCapture(new AbortController().signal, vi.fn());
  const refused = expect(pending).rejects.toThrow();
  await vi.waitFor(() => expect(ports.recorder).toHaveBeenCalled());
  await vi.advanceTimersByTimeAsync(10_000);
  await refused;
  expect(sending).toBe(false);
  expect(order).toEqual(["network closed", "recorder stopped", "disclosure cleared"]);
});

it("an active request failure fences capture before cleanup and informs the caller", async () => {
  const failure = vi.fn();
  const pending = startStudyCapture(new AbortController().signal, failure);
  await vi.waitFor(() => expect(recording).toBe(true));
  await vi.advanceTimersByTimeAsync(100);
  const session = await pending;
  ports.transport.mock.calls[0][1]();
  expect(sending).toBe(false);
  expect(failure).toHaveBeenCalledTimes(1);
  await session.stop();
  expect(order).toEqual(["network closed", "recorder stopped", "disclosure cleared"]);
});

it.each([
  "config",
  "SDK cleanup",
  "IPC cleanup",
])("redacts start failure even when %s rejects", async (edge) => {
  const privateError = new Error("PRIVATE_SENTINEL");
  if (edge === "config") ports.config.mockRejectedValue(privateError);
  else {
    ports.recorder.mockImplementation(() => ({
      start: () => {
        throw privateError;
      },
      stop: edge === "SDK cleanup" ? () => Promise.reject(privateError) : async () => {},
    }));
    if (edge === "IPC cleanup") ports.stop.mockRejectedValue(privateError);
  }
  await expect(startStudyCapture(new AbortController().signal, vi.fn())).rejects.toThrow(
    "Study unavailable or cancelled",
  );
  expect(sending).toBe(false);
  if (edge === "SDK cleanup") expect(ports.stop).not.toHaveBeenCalled();
});

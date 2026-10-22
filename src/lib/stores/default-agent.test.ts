import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DefaultAgentVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsDefaultGet: vi.fn(),
  agentsDefaultSet: vi.fn(),
  frontendErrorReport: vi.fn(() => Promise.resolve()),
}));

import { agentsDefaultGet, agentsDefaultSet, frontendErrorReport } from "@/lib/ipc/client";
import { defaultAgentStore, readFailedSentence } from "@/lib/stores/default-agent";

const NIXI = "nixie";
const TALA = "tala";

function answer(defaultAgent: string | null): DefaultAgentVm {
  return {
    defaultAgent,
    reason: null,
    choices: [
      { agent: NIXI, name: "Nixi" },
      { agent: TALA, name: "Tala" },
    ],
    chooser: false,
    fileSet: null,
  };
}

/** An answer the test settles when it chooses. The executor form, not
 *  `Promise.withResolvers`: the project compiles against `lib: ES2020`. */
function pending<T = DefaultAgentVm>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
} {
  let resolve: (value: T) => void = () => {};
  let reject: (error: unknown) => void = () => {};
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

/** Let every promise callback the store queued run, whatever it chose to do
 *  with a read asked for during a save (issue it, or hold it): there is no
 *  signal for "nothing more happens", so one zero-length macrotask, not a
 *  timed wait. */
function settle(): Promise<void> {
  return new Promise((done) => setTimeout(done, 0));
}

function reset(): void {
  vi.mocked(agentsDefaultGet).mockReset();
  vi.mocked(agentsDefaultSet).mockReset();
  vi.mocked(frontendErrorReport).mockClear();
  defaultAgentStore.setState({
    vm: null,
    readError: null,
    saveError: null,
    saving: false,
    navRestore: "pending",
  });
}

beforeEach(reset);
afterEach(reset);

describe("defaultAgentStore", () => {
  it("keeps a saved choice when a read issued before the save answers after it", async () => {
    const read = pending();
    vi.mocked(agentsDefaultGet).mockReturnValue(read.promise);
    vi.mocked(agentsDefaultSet).mockResolvedValue(answer(TALA));
    const loading = defaultAgentStore.getState().load();
    await defaultAgentStore.getState().choose(TALA);
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(TALA);
    read.resolve(answer(NIXI));
    await loading;
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(TALA);
  });

  it("shows a read issued after the save", async () => {
    vi.mocked(agentsDefaultSet).mockResolvedValue(answer(TALA));
    vi.mocked(agentsDefaultGet).mockResolvedValue(answer(null));
    await defaultAgentStore.getState().choose(TALA);
    await defaultAgentStore.getState().load();
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBeNull();
  });

  it.each([
    ["before", true],
    ["after", false],
  ])("keeps a saved choice when a refresh asked for during the save answers %s it", async (_, readFirst) => {
    // Rust as it behaves: the save commits only when `commit` resolves, and
    // a read answers with whatever is stored when it is issued.
    let stored: string | null = NIXI;
    const commit = pending<void>();
    const release = pending<void>();
    vi.mocked(agentsDefaultSet).mockImplementation(async (agent) => {
      await commit.promise;
      stored = agent;
      return answer(stored);
    });
    vi.mocked(agentsDefaultGet).mockImplementation(() => {
      const seen = answer(stored);
      return readFirst ? Promise.resolve(seen) : release.promise.then(() => seen);
    });
    const saving = defaultAgentStore.getState().choose(TALA);
    // A room change refreshes while the save is still out.
    const loading = defaultAgentStore.getState().load();
    await settle();
    commit.resolve();
    expect((await saving)?.defaultAgent).toBe(TALA);
    release.resolve();
    await loading;
    expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(TALA);
  });

  it("keeps a newer read's answer when an older read fails after it, and still reports", async () => {
    const older = pending();
    vi.mocked(agentsDefaultGet)
      .mockReturnValueOnce(older.promise)
      .mockResolvedValueOnce(answer(NIXI));
    const first = defaultAgentStore.getState().load();
    await defaultAgentStore.getState().load();
    older.reject(new Error("the old read timed out"));
    await first;
    const state = defaultAgentStore.getState();
    expect(state.readError).toBeNull();
    expect(state.vm?.choices).toHaveLength(2);
    expect(vi.mocked(frontendErrorReport).mock.calls.map((call) => call[0])).toEqual([
      "agents.default.read",
    ]);
  });

  it("reports a failed save and a failed read, each with its own sentence", async () => {
    // Errors whose message the crash funnel has not seen in this file.
    vi.mocked(agentsDefaultSet).mockRejectedValue(new Error("the bridge is gone"));
    vi.mocked(agentsDefaultGet).mockRejectedValue(new Error("no data directory"));
    expect(await defaultAgentStore.getState().choose(NIXI)).toBeNull();
    await defaultAgentStore.getState().load();
    const state = defaultAgentStore.getState();
    expect(state.saveError).toBe("the bridge is gone");
    expect(state.readError).toBe(readFailedSentence(new Error("no data directory")));
    expect(state.saving).toBe(false);
    expect(vi.mocked(frontendErrorReport).mock.calls.map((call) => call[0])).toEqual([
      "agents.default.save",
      "agents.default.read",
    ]);
  });
});

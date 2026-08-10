/**
 * The mirror of the open capture windows (Story 45.15, FR-191, FR-192).
 *
 * Every action here is one IPC call plus a re-read, so **each test asserts the
 * call and the resulting state**, never one without the other. A mock resolves
 * whatever it was told to resolve regardless of its arguments, so a test that
 * acts and then asserts on the mirrored list is checking the SHAPE of the
 * payload and never its VALUE — and it reads exactly like a test that checks
 * both. Two windows in every fixture, for the same reason: a mutation that
 * keeps only the first element passes every single-item test.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureWindowVm } from "@/lib/ipc/client";

const notesCaptureWindows = vi.fn<() => Promise<CaptureWindowVm[]>>();
const notesCaptureOpen = vi.fn<(target: unknown) => Promise<void>>();
const notesCaptureClose = vi.fn<(key: string) => Promise<void>>();
const notesCaptureSetLocked = vi.fn<(key: string, locked: boolean) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesCaptureWindows: (...args: []) => notesCaptureWindows(...args),
  notesCaptureOpen: (target: unknown) => notesCaptureOpen(target),
  notesCaptureClose: (key: string) => notesCaptureClose(key),
  notesCaptureSetLocked: (key: string, locked: boolean) => notesCaptureSetLocked(key, locked),
}));

// After `vi.mock`, following `use-encryption-statuses.test.ts`: the mock is
// hoisted above every import, so this is a static import that reads in the
// order it takes effect.
import {
  captureWindowFor,
  captureWindowsStore,
  closeCaptureWindow,
  hydrateCaptureWindows,
  openCaptureWindow,
  resetCaptureWindowsStoreForTest,
  setCaptureWindowLocked,
} from "@/lib/stores/capture-windows";

const DRAFT: CaptureWindowVm = {
  key: "draft",
  target: { kind: "draft" },
  locked: true,
  visible: true,
};

const FIRST_NOTE: CaptureWindowVm = {
  key: "note:v1/n1",
  target: { kind: "note", vaultId: "v1", noteId: "n1" },
  locked: false,
  visible: true,
};

const SECOND_NOTE: CaptureWindowVm = {
  key: "note:v1/n2",
  target: { kind: "note", vaultId: "v1", noteId: "n2" },
  locked: true,
  visible: true,
};

beforeEach(() => {
  vi.clearAllMocks();
  resetCaptureWindowsStoreForTest();
  notesCaptureWindows.mockResolvedValue([DRAFT, FIRST_NOTE, SECOND_NOTE]);
  notesCaptureOpen.mockResolvedValue(undefined);
  notesCaptureClose.mockResolvedValue(undefined);
  notesCaptureSetLocked.mockResolvedValue(undefined);
});

describe("hydrateCaptureWindows", () => {
  it("holds several windows at once, each with its own target and lock", async () => {
    // The story's headline: several capture windows, each holding its own note.
    // Asserted on all three rows rather than on a count, because a mirror that
    // keeps the first row and drops the rest has the same count as one that
    // keeps the right ones after the first re-read.
    await hydrateCaptureWindows();
    expect(captureWindowsStore.getState().windows).toEqual([DRAFT, FIRST_NOTE, SECOND_NOTE]);
    const first = captureWindowFor(captureWindowsStore.getState(), "note:v1/n1");
    const second = captureWindowFor(captureWindowsStore.getState(), "note:v1/n2");
    expect(first?.target).toEqual({ kind: "note", vaultId: "v1", noteId: "n1" });
    expect(second?.target).toEqual({ kind: "note", vaultId: "v1", noteId: "n2" });
    // Two windows, two DIFFERENT notes, two different lock states — the pair a
    // single-item fixture could never distinguish from a mirror that answers
    // the same row for every key.
    expect(first?.locked).toBe(false);
    expect(second?.locked).toBe(true);
  });

  it("keeps the last known list when a read fails, rather than claiming none", async () => {
    await hydrateCaptureWindows();
    notesCaptureWindows.mockRejectedValueOnce(new Error("no"));
    await hydrateCaptureWindows();
    // A stale list is wrong about one window that closed. An empty one is wrong
    // about every window that is open, and would make the main window offer a
    // second capture for a note that already has one.
    expect(captureWindowsStore.getState().windows).toEqual([DRAFT, FIRST_NOTE, SECOND_NOTE]);
  });

  it("starts unread rather than empty", () => {
    // `null` is "keeper has not looked". An empty array here would let a
    // surface conclude "no window holds this note" before anything had asked.
    expect(captureWindowsStore.getState().windows).toBeNull();
    expect(captureWindowFor(captureWindowsStore.getState(), "note:v1/n1")).toBeNull();
  });
});

describe("captureWindowFor", () => {
  it("finds the row for a key and no other", async () => {
    await hydrateCaptureWindows();
    const state = captureWindowsStore.getState();
    expect(captureWindowFor(state, "draft")?.key).toBe("draft");
    expect(captureWindowFor(state, "note:v1/n2")?.key).toBe("note:v1/n2");
    // A key nothing holds is null, not the first row — the difference between
    // "this note has no window" and "this note's window is the draft one".
    expect(captureWindowFor(state, "note:v1/n9")).toBeNull();
  });
});

describe("the actions", () => {
  it("opens the window for the target it was given, then re-reads", async () => {
    await openCaptureWindow({ kind: "note", vaultId: "v1", noteId: "n2" });
    // The call, not only the result: a mock resolves the same list whatever
    // target it is handed, so without this the wrong-note mutation is invisible.
    expect(notesCaptureOpen).toHaveBeenCalledWith({
      kind: "note",
      vaultId: "v1",
      noteId: "n2",
    });
    expect(notesCaptureWindows).toHaveBeenCalled();
    expect(captureWindowsStore.getState().windows).toEqual([DRAFT, FIRST_NOTE, SECOND_NOTE]);
  });

  it("closes the window it was asked to close and not its neighbour", async () => {
    await hydrateCaptureWindows();
    notesCaptureWindows.mockResolvedValue([DRAFT, FIRST_NOTE]);
    await closeCaptureWindow("note:v1/n2");
    expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/n2");
    // Asserted on the surviving rows: "one fewer" is also true of closing the
    // wrong one.
    expect(captureWindowsStore.getState().windows).toEqual([DRAFT, FIRST_NOTE]);
  });

  it("locks the window it was asked to lock, with the value it was asked for", async () => {
    await setCaptureWindowLocked("note:v1/n1", true);
    // Both arguments. A mutation that passes the key and drops the boolean, or
    // passes the current value instead of the next one, leaves a lock button
    // that renders, presses, and does nothing.
    expect(notesCaptureSetLocked).toHaveBeenCalledWith("note:v1/n1", true);
    await setCaptureWindowLocked("note:v1/n2", false);
    expect(notesCaptureSetLocked).toHaveBeenLastCalledWith("note:v1/n2", false);
    expect(notesCaptureSetLocked).toHaveBeenCalledTimes(2);
  });

  it("lets a rejection reach the caller rather than swallowing it", async () => {
    // Deliberately unlike `hydrateCaptureWindows`, which runs on an event with
    // nobody to tell. These three run because a person pressed something, and a
    // press that did nothing must not also be silent.
    notesCaptureClose.mockRejectedValueOnce(new Error("window is gone"));
    await expect(closeCaptureWindow("note:v1/n1")).rejects.toThrow("window is gone");
  });
});

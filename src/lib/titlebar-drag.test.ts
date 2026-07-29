import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const startWindowDragging = vi.fn();
const titlebarDragReport = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  startWindowDragging: () => startWindowDragging(),
  titlebarDragReport: (...args: unknown[]) => titlebarDragReport(...args),
}));

import { beginTitleBarDrag } from "@/lib/titlebar-drag";

/** Every stage this module reported, in order. */
function reportedStages(): unknown[] {
  return titlebarDragReport.mock.calls.map(([stage]) => stage);
}

beforeEach(() => {
  startWindowDragging.mockReset().mockResolvedValue(undefined);
  titlebarDragReport.mockReset().mockResolvedValue(undefined);
  vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("beginTitleBarDrag", () => {
  it("issues the drag synchronously, before the first report and before any await", async () => {
    // The whole defect is that AppKit only honours the drag for the mouse-down it
    // is processing, so deferring the call by even one microtask is a regression.
    const pending = beginTitleBarDrag();

    expect(startWindowDragging).toHaveBeenCalledTimes(1);
    expect(reportedStages()).toEqual(["issued"]);
    expect(startWindowDragging.mock.invocationCallOrder[0]).toBeLessThan(
      titlebarDragReport.mock.invocationCallOrder[0],
    );

    await pending;
  });

  it("records the accepted outcome once the window layer answers", async () => {
    await beginTitleBarDrag();

    expect(reportedStages()).toEqual(["issued", "accepted"]);
  });

  it("records an ACL refusal verbatim — it names the missing permission", async () => {
    // Tauri denies a command by rejecting with a bare string; that string is the
    // one thing that says which permission is missing, so it must survive intact.
    const denial =
      "window.start_dragging not allowed. Permissions associated with this command: core:window:allow-start-dragging";
    startWindowDragging.mockRejectedValue(denial);

    await beginTitleBarDrag();

    expect(titlebarDragReport).toHaveBeenLastCalledWith("refused", denial);
  });

  it("records the message of a structured rejection", async () => {
    startWindowDragging.mockRejectedValue({
      code: "internal",
      message: "no window with label main",
      retriable: false,
    });

    await beginTitleBarDrag();

    expect(titlebarDragReport).toHaveBeenLastCalledWith("refused", "no window with label main");
  });

  it("never rejects, even when the report itself cannot be delivered", async () => {
    // No Tauri host, or the diagnostic command absent from an older build: the
    // drag must not become a failed promise nobody handles.
    startWindowDragging.mockRejectedValue(new Error("no ipc"));
    titlebarDragReport.mockRejectedValue(new Error("command not found"));

    await expect(beginTitleBarDrag()).resolves.toBeUndefined();
    expect(reportedStages()).toEqual(["issued", "refused"]);
  });
});

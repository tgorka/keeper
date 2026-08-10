/**
 * The loader and the save rule (Story 45.6, FR-179, AD-89).
 *
 * `renderHook` is used here, and DW-172 is the reason it needs a word of
 * justification: mounting a hook proves nothing about whether any surface
 * mounts it. What makes it the right tool *here* is that this hook has no
 * rendering to be wrong about — its whole contract is a state machine over two
 * IPC calls — and the mounting question is answered on the other side of the
 * seam, by 45.4's suite, which mounts the component that calls it.
 *
 * What is deliberately NOT mocked is the decision itself. Every "keeper
 * declines to act" branch below asserts the `console.info` line as well as the
 * absence of a write, because a save that quietly does nothing is the failure
 * DW-162 names and an assertion on `syncWriteEntry` alone cannot tell "declined
 * for a reason" from "forgot to call it".
 */
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TextFileVm } from "@/lib/ipc/client";

const syncReadText = vi.fn<(id: string, subpath: string) => Promise<TextFileVm>>();
const syncWriteEntry = vi.fn<(id: string, subpath: string, content: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  syncReadText: (id: string, subpath: string) => syncReadText(id, subpath),
  syncWriteEntry: (id: string, subpath: string, content: string) =>
    syncWriteEntry(id, subpath, content),
}));

import { useTextFile } from "./use-text-file";

/** An ordinary, editable text file. */
function opened(text: string, overrides: Partial<TextFileVm> = {}): TextFileVm {
  return {
    text,
    sizeBytes: text.length,
    sizeLabel: `${text.length} bytes`,
    oversize: false,
    binary: false,
    detail: null,
    ...overrides,
  };
}

/** Everything written to `console.info` during the current test, in order. */
let logged: string[] = [];

beforeEach(() => {
  vi.clearAllMocks();
  syncWriteEntry.mockResolvedValue();
  logged = [];
  vi.spyOn(console, "info").mockImplementation((...args: unknown[]) => {
    logged.push(args.map(String).join(" "));
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** The reasons keeper gave for not saving, in order. */
function declines(): string[] {
  return logged.filter((line) => line.includes("not saving"));
}

async function mounted(profileId: string | null = "p1", subpath = "notes/a.md") {
  const hook = renderHook(() => useTextFile({ profileId, subpath }));
  await waitFor(() => expect(hook.result.current.loading).toBe(false));
  return hook;
}

describe("useTextFile, opening", () => {
  it("hands back the file's text byte for byte", async () => {
    // Every shape a careless loader normalises: CRLF, a hard tab, and no
    // trailing newline.
    const body = "a\tb\r\nsecond\r\nno trailing newline";
    syncReadText.mockResolvedValue(opened(body));

    const { result } = await mounted();

    expect(result.current.content).toBe(body);
    expect(result.current.dirty).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("reads through the profile id and the listing's own subpath, never a path", async () => {
    syncReadText.mockResolvedValue(opened("x"));

    await mounted("p7", "deep/nested/file.toml");

    expect(syncReadText).toHaveBeenCalledWith("p7", "deep/nested/file.toml");
  });

  it("says a file outside every profile cannot be opened here, rather than hanging", async () => {
    const { result } = await mounted(null, "elsewhere.txt");

    expect(syncReadText).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toContain("not inside a synced folder");
  });

  it("surfaces Rust's own sentence when the read is refused", async () => {
    syncReadText.mockRejectedValue({
      code: "internal",
      message: "merope is not attached.",
      retriable: false,
    });

    const { result } = await mounted();

    expect(result.current.error).toBe("merope is not attached.");
    expect(result.current.content).toBe("");
  });

  it("treats an oversize file as opened, not as an error", async () => {
    // It opened fine. It simply cannot be edited, and the banner reads `detail`
    // off the VM. Reporting it as an error would put a failure message over a
    // file the user can perfectly well read.
    syncReadText.mockResolvedValue(
      opened("prefix", { oversize: true, sizeLabel: "4.0 MB", detail: "This file is 4.0 MB…" }),
    );

    const { result } = await mounted();

    expect(result.current.error).toBeNull();
    expect(result.current.vm?.oversize).toBe(true);
    expect(result.current.content).toBe("prefix");
  });

  it("reports a binary file as an error and holds no text for it", async () => {
    syncReadText.mockResolvedValue({
      text: null,
      sizeBytes: 8,
      sizeLabel: "8 bytes",
      oversize: false,
      binary: true,
      detail: "This file is not text — it contains bytes no editor can show. It is 8 bytes.",
    });

    const { result } = await mounted();

    expect(result.current.content).toBe("");
    expect(result.current.error).toContain("not text");
  });
});

describe("useTextFile, saving", () => {
  it("writes exactly what was typed, including a trailing newline", async () => {
    syncReadText.mockResolvedValue(opened("one\n"));
    const { result } = await mounted();

    act(() => result.current.setContent("one\ntwo\n"));
    await act(async () => {
      await result.current.save();
    });

    expect(syncWriteEntry).toHaveBeenCalledWith("p1", "notes/a.md", "one\ntwo\n");
  });

  it("writes exactly what was typed when the trailing newline was removed", async () => {
    // The other side of the same coin, and the one a `.trim()` anywhere in the
    // path would break silently.
    syncReadText.mockResolvedValue(opened("one\n"));
    const { result } = await mounted();

    act(() => result.current.setContent("one"));
    await act(async () => {
      await result.current.save();
    });

    expect(syncWriteEntry).toHaveBeenCalledWith("p1", "notes/a.md", "one");
  });

  it("is clean again after a save, and dirty again after the next edit", async () => {
    syncReadText.mockResolvedValue(opened("a"));
    const { result } = await mounted();

    act(() => result.current.setContent("ab"));
    expect(result.current.dirty).toBe(true);
    await act(async () => {
      await result.current.save();
    });
    expect(result.current.dirty).toBe(false);

    act(() => result.current.setContent("abc"));

    expect(result.current.dirty).toBe(true);
  });

  it("is clean when an edit is typed and typed back", async () => {
    syncReadText.mockResolvedValue(opened("a"));
    const { result } = await mounted();

    act(() => result.current.setContent("ab"));
    act(() => result.current.setContent("a"));

    expect(result.current.dirty).toBe(false);
  });

  it("keeps the buffer and stays dirty when the write is refused", async () => {
    syncReadText.mockResolvedValue(opened("a"));
    syncWriteEntry.mockRejectedValue({
      code: "internal",
      message: "keeper could not write notes/a.md: the volume is read-only.",
      retriable: false,
    });
    const { result } = await mounted();

    act(() => result.current.setContent("typed and precious"));
    await act(async () => {
      await result.current.save();
    });

    // Never rolled back. Losing what someone typed is worse than showing text
    // the disk does not have yet.
    expect(result.current.content).toBe("typed and precious");
    expect(result.current.dirty).toBe(true);
    expect(result.current.error).toContain("the volume is read-only");
  });
});

describe("useTextFile, declining to save", () => {
  it("declines, out loud, when nothing changed", async () => {
    syncReadText.mockResolvedValue(opened("a"));
    const { result } = await mounted();

    await act(async () => {
      await result.current.save();
    });

    expect(syncWriteEntry).not.toHaveBeenCalled();
    expect(declines()).toHaveLength(1);
    expect(declines()[0]).toContain("nothing changed");
  });

  it("declines, out loud, for an oversize file, and names its size", async () => {
    // The one that would lose data: the buffer is a one-megabyte prefix, and
    // writing it would delete everything past it.
    syncReadText.mockResolvedValue(
      opened("prefix", { oversize: true, sizeLabel: "4.0 MB", detail: "…" }),
    );
    const { result } = await mounted();

    act(() => result.current.setContent("prefix edited"));
    await act(async () => {
      await result.current.save();
    });

    expect(syncWriteEntry).not.toHaveBeenCalled();
    expect(declines()[0]).toContain("4.0 MB");
    expect(declines()[0]).toContain("truncate");
  });

  it("declines, out loud, for a binary file", async () => {
    syncReadText.mockResolvedValue({
      text: null,
      sizeBytes: 4,
      sizeLabel: "4 bytes",
      oversize: false,
      binary: true,
      detail: "not text",
    });
    const { result } = await mounted();

    act(() => result.current.setContent("hello"));
    await act(async () => {
      await result.current.save();
    });

    expect(syncWriteEntry).not.toHaveBeenCalled();
    expect(declines()[0]).toContain("not text");
  });

  it("declines, out loud, with no profile to write into", async () => {
    const { result } = await mounted(null, "elsewhere.txt");

    act(() => result.current.setContent("hello"));
    await act(async () => {
      await result.current.save();
    });

    expect(syncWriteEntry).not.toHaveBeenCalled();
    expect(declines()[0]).toContain("not inside a synced folder");
  });

  it("names the file in every decline, so a log line identifies its subject", async () => {
    syncReadText.mockResolvedValue(opened("a"));
    const { result } = await mounted("p1", "deep/config.toml");

    await act(async () => {
      await result.current.save();
    });

    expect(declines()[0]).toContain("deep/config.toml");
  });
});

describe("useTextFile, reloading", () => {
  it("replaces the buffer with what is now on disk", async () => {
    // 45.4's CSV rendered view writes a cell straight to disk through
    // `notes_csv_set_cell`, so this hook's buffer is stale afterwards and there
    // is no new text to hand back — only a reason to re-read.
    syncReadText.mockResolvedValueOnce(opened("a,b\n1,2\n"));
    const { result } = await mounted("p1", "t.csv");
    syncReadText.mockResolvedValueOnce(opened("a,b\n1,9\n"));

    await act(async () => {
      await result.current.reload();
    });

    expect(result.current.content).toBe("a,b\n1,9\n");
    expect(result.current.dirty).toBe(false);
  });

  it("discards a local edit rather than merging it", async () => {
    // Deliberate and worth pinning: `reload` is called when something else has
    // changed the file, and a three-way merge here would be a second, silent
    // conflict resolver beside the one the notes path already has.
    syncReadText.mockResolvedValueOnce(opened("original"));
    const { result } = await mounted();
    act(() => result.current.setContent("mine"));
    syncReadText.mockResolvedValueOnce(opened("theirs"));

    await act(async () => {
      await result.current.reload();
    });

    expect(result.current.content).toBe("theirs");
  });

  it("lets a late read for the previous file lose to the current one", async () => {
    // Two reads in flight, the first slower. Without a generation check the
    // stale answer lands last and the pane shows the wrong file's text.
    let releaseFirst: (vm: TextFileVm) => void = () => {};
    syncReadText.mockImplementationOnce(
      async () =>
        await new Promise<TextFileVm>((resolve) => {
          releaseFirst = resolve;
        }),
    );
    const hook = renderHook(({ subpath }) => useTextFile({ profileId: "p1", subpath }), {
      initialProps: { subpath: "first.txt" },
    });

    syncReadText.mockResolvedValueOnce(opened("second file"));
    hook.rerender({ subpath: "second.txt" });
    await waitFor(() => expect(hook.result.current.content).toBe("second file"));
    await act(async () => {
      releaseFirst(opened("first file"));
      await Promise.resolve();
    });

    expect(hook.result.current.content).toBe("second file");
  });
});

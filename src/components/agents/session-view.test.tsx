import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionView } from "@/components/agents/session-view";
import { FILE_READ_TIMEOUT_MS } from "@/components/chat/composer";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentSessionViewVm } from "@/lib/ipc/client";

const mock = vi.hoisted(() => ({
  view: null as ((vm: AgentSessionViewVm) => void) | null,
  drop: null as ((event: { payload: { type: string; paths: string[] } }) => void) | null,
  release: vi.fn(),
  path: vi.fn(),
  bytes: vi.fn(),
  commit: vi.fn(),
  releaseHandles: vi.fn(),
  send: vi.fn(),
  picker: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mock.picker }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(async (callback) => {
      mock.drop = callback;
      return mock.release;
    }),
  }),
}));
vi.mock("@/lib/failure", () => ({ logFailure: () => () => {}, sayFailure: () => () => {} }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionOpen: vi.fn(async (_target, callback) => {
    mock.view = callback;
    return "subscription";
  }),
  agentSessionClose: vi.fn().mockResolvedValue(undefined),
  agentAttachPath: mock.path,
  agentAttachBytes: mock.bytes,
  agentAttachCommit: mock.commit,
  agentAttachRelease: mock.releaseHandles,
  agentSend: mock.send,
}));
const vm: AgentSessionViewVm = {
  agentMark: "A",
  database: "beta",
  sessionId: "same.1",
  title: "Research",
  agentActor: "assistant",
  agentName: "Assistant",
  readOnly: false,
  archived: false,
  hasEarlier: false,
  loadingEarlier: false,
  complete: true,
  readsTrace: true,
  untrusted: false,
  live: [],
  tools: [],
  messages: [],
  approvals: [],
  prefs: { showReasoning: false, showDetails: false },
};
async function draw() {
  const result = render(
    <TooltipProvider>
      <SessionView target={{ database: "beta", sessionId: "same.1" }} />
    </TooltipProvider>,
  );
  await waitFor(() => expect(mock.view).not.toBeNull());
  act(() => mock.view?.(vm));
  await waitFor(() => expect(mock.drop).not.toBeNull());
  return result;
}
function paste(name: string, mime = "text/csv") {
  const bytes = new TextEncoder().encode("item,price\ntea,3").buffer;
  fireEvent.paste(screen.getByRole("textbox", { name: "Message" }), {
    clipboardData: {
      items: [
        {
          kind: "file",
          type: mime,
          getAsFile: () => ({
            name,
            type: mime,
            size: bytes.byteLength,
            arrayBuffer: async () => bytes,
          }),
        },
      ],
    },
  });
}
let staged = 0;
beforeEach(() => {
  vi.clearAllMocks();
  mock.view = null;
  mock.drop = null;
  staged = 0;
  // Rust stages each file under a handle; the mock names them in staging order.
  mock.path
    .mockReset()
    .mockImplementation(async (_target, paths: string[]) => paths.map(() => `h${++staged}`));
  mock.bytes.mockReset().mockImplementation(async () => `h${++staged}`);
  mock.commit.mockReset().mockResolvedValue(undefined);
  mock.releaseHandles.mockReset().mockResolvedValue(undefined);
  mock.send.mockReset().mockResolvedValue(undefined);
  mock.picker.mockReset().mockResolvedValue(["/tmp/plan.pdf", "/tmp/prices.csv"]);
});

describe("session composer attachments", () => {
  it("sends a picked batch with one caption into its own database, without a second text message", async () => {
    await draw();
    fireEvent.click(screen.getByRole("button", { name: "Attach file" }));
    await screen.findByRole("button", { name: "Remove plan.pdf" });
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Read both files" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(mock.commit).toHaveBeenCalledWith(
        { database: "beta", sessionId: "same.1" },
        ["h1", "h2"],
        "Read both files",
      ),
    );
    expect(mock.path).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, [
      "/tmp/plan.pdf",
      "/tmp/prices.csv",
    ]);
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("");
    expect(mock.send).not.toHaveBeenCalled();
  });

  it("keeps the oversize refusal and caption, and lets the person remove the refused file", async () => {
    await draw();
    const sentence =
      "huge.zip is 25.1 MB and an agent takes at most 25.0 MB from one message, so it did not get the file.";
    mock.bytes.mockRejectedValue({ message: sentence, retriable: false });
    paste("huge.zip", "application/zip");
    await screen.findByRole("button", { name: "Remove huge.zip" });
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Keep my caption" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(sentence);
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Keep my caption");
    expect(mock.commit).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove huge.zip" }));
    expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument();
    expect(mock.send).not.toHaveBeenCalled();
  });

  it("sends a mixed selection of a dropped path and two pastes as one message with its caption", async () => {
    await draw();
    act(() => mock.drop?.({ payload: { type: "drop", paths: ["/tmp/notes.md"] } }));
    paste("first.csv");
    await screen.findByRole("button", { name: "Remove first.csv" });
    paste("second.csv");
    await screen.findByRole("button", { name: "Remove second.csv" });
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Compare these two files" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument(),
    );
    // Staging carries no caption; the one commit carries every handle and the caption once.
    expect(mock.path).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, [
      "/tmp/notes.md",
    ]);
    expect(mock.bytes.mock.calls.map((call) => call.slice(2))).toEqual([
      ["first.csv", "text/csv"],
      ["second.csv", "text/csv"],
    ]);
    expect(mock.commit).toHaveBeenCalledOnce();
    expect(mock.commit).toHaveBeenCalledWith(
      { database: "beta", sessionId: "same.1" },
      ["h1", "h2", "h3"],
      "Compare these two files",
    );
    expect(mock.send).not.toHaveBeenCalled();
    expect(mock.releaseHandles).not.toHaveBeenCalled();
  });

  it("commits nothing when a later file is refused at staging, releases what was staged, and retries whole", async () => {
    await draw();
    act(() => mock.drop?.({ payload: { type: "drop", paths: ["/tmp/notes.md"] } }));
    paste("prices.csv");
    await screen.findByRole("button", { name: "Remove prices.csv" });
    mock.bytes.mockRejectedValueOnce({ message: "The pasted file could not be staged." });
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Review these" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The pasted file could not be staged.",
    );
    expect(mock.commit).not.toHaveBeenCalled();
    expect(mock.releaseHandles).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, [
      "h1",
    ]);
    // Nothing was sent, so the whole selection and the caption stay.
    expect(screen.getByRole("button", { name: "Remove notes.md" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove prices.csv" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Review these");
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument(),
    );
    expect(mock.commit).toHaveBeenCalledOnce();
    expect(mock.commit).toHaveBeenCalledWith(
      { database: "beta", sessionId: "same.1" },
      ["h2", "h3"],
      "Review these",
    );
    expect(mock.send).not.toHaveBeenCalled();
  });

  /** A paste of `names` with `text`, each file's read held until `finish(i)` (R1071C-04). */
  function pasteHeld(names: string[], text = "") {
    const bytes = new TextEncoder().encode("item,price\ntea,3").buffer;
    // The executor form: the project's ES2020 lib has no Promise.withResolvers.
    const finishers: Array<(read: ArrayBuffer) => void> = [];
    const reads = names.map(
      () =>
        new Promise<ArrayBuffer>((resolve) => {
          finishers.push(resolve);
        }),
    );
    const files = names.map((name, i) => ({
      kind: "file",
      type: "text/csv",
      getAsFile: () => ({
        name,
        type: "text/csv",
        size: bytes.byteLength,
        arrayBuffer: () => reads[i],
      }),
    }));
    const plain =
      text === "" ? [] : [{ kind: "string", type: "text/plain", getAsFile: () => null }];
    fireEvent.paste(screen.getByRole("textbox", { name: "Message" }), {
      clipboardData: {
        items: [...files, ...plain],
        getData: (type: string) => (type === "text/plain" ? text : ""),
      },
    });
    return { finish: (i: number) => act(async () => finishers[i](bytes)) };
  }

  it("holds Send while a pasted file is still being read, then sends its caption with it as one message", async () => {
    await draw();
    const held = pasteHeld(["delayed.csv"], "Explain these prices");
    const field = screen.getByRole("textbox", { name: "Message" });
    expect(field).toHaveValue("Explain these prices");
    expect(screen.getByRole("status")).toHaveTextContent("Reading the file to send…");
    // Neither the button nor Enter sends the caption without its file.
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    fireEvent.keyDown(field, { key: "Enter" });
    await act(async () => {});
    expect(mock.send).not.toHaveBeenCalled();
    expect(mock.commit).not.toHaveBeenCalled();
    expect(field).toHaveValue("Explain these prices");

    await held.finish(0);
    await screen.findByRole("button", { name: "Remove delayed.csv" });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(mock.commit).toHaveBeenCalledOnce());
    expect(mock.commit).toHaveBeenCalledWith(
      { database: "beta", sessionId: "same.1" },
      ["h1"],
      "Explain these prices",
    );
    expect(mock.send).not.toHaveBeenCalled();
  });

  it("shows no part of a multi-file paste while a later file is still being read", async () => {
    await draw();
    const held = pasteHeld(["first.csv", "second.csv"]);
    await held.finish(0);
    // The first file is read; the second is not: no partial selection, nothing sendable.
    expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    await held.finish(1);
    await screen.findByRole("button", { name: "Remove second.csv" });
    expect(screen.getByRole("button", { name: "Remove first.csv" })).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(mock.commit).toHaveBeenCalledOnce());
    expect(mock.commit).toHaveBeenCalledWith(
      { database: "beta", sessionId: "same.1" },
      ["h1", "h2"],
      undefined,
    );
  });

  /** After a cancelled or failed batch: its late read adds nothing, and the text sends alone. */
  async function lateReadAddsNothingAndTextSends(finish: () => Promise<void>, text: string) {
    await finish();
    expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Files being read" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(mock.send).toHaveBeenCalledOnce());
    expect(mock.send).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, text);
    expect(mock.bytes).not.toHaveBeenCalled();
    expect(mock.commit).not.toHaveBeenCalled();
  }

  it("drops a stuck pasted file from its reading chip; its late read adds nothing (R1071C-05)", async () => {
    await draw();
    const held = pasteHeld(["held-never.csv"], "Original caption");
    const reading = screen.getByRole("list", { name: "Files being read" });
    expect(within(reading).getByText("held-never.csv")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel reading held-never.csv" }));
    expect(screen.queryByRole("list", { name: "Files being read" })).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Original caption");
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
    await lateReadAddsNothingAndTextSends(() => held.finish(0), "Original caption");
  });

  it("drops every stuck read on Escape and keeps the caption (R1071C-05)", async () => {
    await draw();
    const held = pasteHeld(["held-never.csv"], "Escaped caption");
    const field = screen.getByRole("textbox", { name: "Message" });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("list", { name: "Files being read" })).not.toBeInTheDocument();
    expect(field).toHaveValue("Escaped caption");
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
    await lateReadAddsNothingAndTextSends(() => held.finish(0), "Escaped caption");
  });

  it("cancelling one stuck read leaves another still holding Send (R1071C-05)", async () => {
    await draw();
    const abandoned = pasteHeld(["abandoned.csv"], "Only the kept file");
    const kept = pasteHeld(["kept.csv"]);
    fireEvent.click(screen.getByRole("button", { name: "Cancel reading abandoned.csv" }));
    // kept.csv is still being read: the caption must not leave without it (R1071C-04).
    expect(screen.getByRole("button", { name: "Cancel reading kept.csv" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message" }), { key: "Enter" });
    await act(async () => {});
    expect(mock.send).not.toHaveBeenCalled();
    await kept.finish(0);
    await abandoned.finish(0);
    const tray = screen.getByRole("list", { name: "Pending attachments" });
    expect(within(tray).getAllByRole("listitem")).toHaveLength(1);
    expect(within(tray).getByText("kept.csv")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(mock.commit).toHaveBeenCalledOnce());
    expect(mock.bytes).toHaveBeenCalledOnce();
    expect(mock.commit).toHaveBeenCalledWith(
      { database: "beta", sessionId: "same.1" },
      ["h1"],
      "Only the kept file",
    );
    expect(mock.send).not.toHaveBeenCalled();
  });

  it("fails a read still pending after the timeout with the read-error sentence (R1071C-05)", async () => {
    await draw();
    vi.useFakeTimers();
    const held = pasteHeld(["slow.csv"], "Timed caption");
    try {
      await act(async () => vi.advanceTimersByTimeAsync(FILE_READ_TIMEOUT_MS - 1));
      expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
      await act(async () => vi.advanceTimersByTimeAsync(1));
    } finally {
      vi.useRealTimers();
    }
    expect(screen.getByRole("alert")).toHaveTextContent(
      "keeper couldn't read the pasted file. Paste it again to retry.",
    );
    expect(screen.queryByRole("list", { name: "Files being read" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Timed caption");
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
    await lateReadAddsNothingAndTextSends(() => held.finish(0), "Timed caption");
    // A later paste that reads retires the timed-out batch's sentence.
    paste("next.csv");
    await screen.findByRole("button", { name: "Remove next.csv" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("retains a new drop that arrives during a send and releases the native listener on close", async () => {
    const { unmount } = await draw();
    let complete: (() => void) | undefined;
    mock.commit.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          complete = resolve;
        }),
    );
    act(() => mock.drop?.({ payload: { type: "drop", paths: ["/tmp/first.md"] } }));
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    act(() => mock.drop?.({ payload: { type: "drop", paths: ["/tmp/next.md"] } }));
    await waitFor(() => expect(complete).toBeDefined());
    await act(async () => complete?.());
    const tray = screen.getByRole("list", { name: "Pending attachments" });
    expect(within(tray).getByRole("button", { name: "Remove next.md" })).toBeInTheDocument();
    expect(within(tray).queryByRole("button", { name: "Remove first.md" })).not.toBeInTheDocument();
    unmount();
    expect(mock.release).toHaveBeenCalledOnce();
  });
});

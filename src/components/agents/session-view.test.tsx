import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionView } from "@/components/agents/session-view";
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

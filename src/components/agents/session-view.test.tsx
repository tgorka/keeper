import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionView } from "@/components/agents/session-view";
import { FILE_READ_TIMEOUT_MS } from "@/components/chat/composer";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentChildSessionVm, AgentSessionViewVm } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

const mock = vi.hoisted(() => ({
  view: null as ((vm: AgentSessionViewVm) => void) | null,
  drop: null as ((event: { payload: { type: string; paths: string[] } }) => void) | null,
  release: vi.fn(),
  path: vi.fn(),
  bytes: vi.fn(),
  commit: vi.fn(),
  releaseHandles: vi.fn(),
  send: vi.fn(),
  decide: vi.fn(),
  picker: vi.fn(),
  preview: vi.fn(),
  command: vi.fn(),
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
  agentSessionApprovalDecide: mock.decide,
  agentCommandPreview: mock.preview,
  agentCommand: mock.command,
  // The composer's line goes out through Rust's mention-aware send (109.24),
  // declining what the banner showed as `agentSend` does.
  agentSendLine: mock.send,
  agentMentionPreview: vi.fn(async (_target: unknown, before: string, after: string) => ({
    before,
    after,
    token: null,
    rows: [],
    note: null,
  })),
}));
const vm: AgentSessionViewVm = {
  agentMark: "A",
  database: "beta",
  sessionId: "same.1",
  title: "Research",
  displayName: "assistant · Research",
  handle: "jolly-pigeon",
  agentActor: "assistant",
  agentName: "Assistant",
  readOnly: false,
  archived: false,
  hasEarlier: false,
  loadingEarlier: false,
  complete: true,
  readsTrace: true,
  live: [],
  tools: [],
  messages: [],
  items: [],
  folds: [],
  approvals: [],
  prefs: { viewMode: "normal" },
  // Rust's answers before any turn: unknown context, the agent's defaults as read-only chips.
  context: {
    used: null,
    window: null,
    label: "—",
    percent: null,
    meterPercent: null,
    ringBackground: "conic-gradient(currentColor 0deg, var(--muted) 0deg)",
    summary: "Context unknown: no answer has measured it yet.",
    parts: [],
  },
  footer: {
    model: "beta-small",
    effort: "Default",
    shortModel: "beta-small",
    effortCode: "D",
    choosesModel: false,
    choosesEffort: false,
  },
};
async function draw(
  shown: AgentSessionViewVm = vm,
  onSession?: (session: { database: string; sessionId: string }) => void,
) {
  const result = render(
    <TooltipProvider>
      <SessionView target={{ database: "beta", sessionId: "same.1" }} onSession={onSession} />
    </TooltipProvider>,
  );
  await waitFor(() => expect(mock.view).not.toBeNull());
  act(() => mock.view?.(shown));
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
  mock.preview.mockReset().mockImplementation(async (_target, draft: string) => ({
    draft,
    verdict: { kind: "command", name: "context", args: null },
    rows: [],
    escapeHint: "",
  }));
  mock.command.mockReset().mockResolvedValue({ session: null, surface: "context", message: "" });
});

it("says Rust's quiet line under a message sent again without the note (109.28 § 9.1a)", async () => {
  const note = "This chat's readers can't all read tgdrive, so the note you're in wasn't sent.";
  await draw({
    ...vm,
    items: [{ kind: "message", messageId: "m1" }],
    messages: [
      {
        id: "m1",
        role: "person",
        authorActor: "ana",
        mine: true,
        authorName: "Ana",
        body: "what is this note?",
        createdAtMs: 1780000000000,
        state: "sent",
        note,
      },
    ],
  });
  const message = screen.getByText("what is this note?").closest("li");
  expect(message).not.toBeNull();
  expect(within(message as HTMLElement).getByText(note)).toBeInTheDocument();
});

it("draws the Notes Assistant's unseen tick in the Agents window too (round 6 ruling)", async () => {
  // A dock answer nobody saw left tgdrive ticked; the Agents window opens that chat.
  await draw({
    ...vm,
    drives: {
      drives: [{ name: "tgdrive", ticked: true }],
      warning: "Once you tick a drive, the agent may read it.",
      pending: false,
      unseen: [{ drive: "tgdrive", tick: "01JTICK" }],
    },
  });
  const note = screen.getByRole("note", { name: "What ticking tgdrive allows" });
  expect(note).toHaveTextContent("Once you tick a drive, the agent may read it.");
  expect(within(note).getByRole("button", { name: "Untick tgdrive" })).toBeInTheDocument();
  expect(within(note).getByRole("button", { name: "OK" })).toBeInTheDocument();
  expect(
    within(screen.getByRole("list", { name: "Drives ticked" })).getByText("tgdrive"),
  ).toBeInTheDocument();
});

describe("session context details", () => {
  const context: AgentSessionViewVm["context"] = {
    used: 48000,
    window: 200000,
    label: "48k / 200k",
    percent: "24%",
    meterPercent: 24,
    ringBackground: "conic-gradient(currentColor 86deg, var(--muted) 0deg)",
    summary: "24% · used ≈ 48000 / 200000 tokens",
    parts: [
      {
        label: "History",
        description: "Conversation history and the current question.",
        tokens: "≈ 48000 tokens",
        share: "≈ 100% of used context",
      },
    ],
  };
  async function runContextCommand() {
    const field = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(field, { target: { value: "/context" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() =>
      expect(mock.command).toHaveBeenCalledWith(
        { database: "beta", sessionId: "same.1" },
        "/context",
        null,
        [],
      ),
    );
    return await screen.findByRole("dialog", { name: "Details" });
  }

  it("/context opens the same Details dialog the bar's reading opens, in the Normal view", async () => {
    // Owner ruling 2026-10-27: the reading is in the bar whatever the view mode.
    await draw({ ...vm, context });
    const ring = screen.getByRole("button", { name: `Context details: ${context.summary}` });
    expect(ring).toHaveTextContent("48k / 200k");
    expect(within(ring).getByRole("meter", { name: "Context window occupancy" })).toHaveValue(24);
    fireEvent.click(ring);
    const viaRing = await screen.findByRole("dialog", { name: "Details" });
    expect(within(viaRing).getByText("History")).toBeInTheDocument();
    fireEvent.keyDown(viaRing, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    const viaCommand = await runContextCommand();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    // One Sheet root: the ring controls this very dialog, whichever way it was opened.
    expect(viaCommand.id).toBe(viaRing.id);
    expect(ring).toHaveAttribute("aria-controls", viaCommand.id);
    expect(ring).toHaveAttribute("aria-expanded", "true");
    expect(within(viaCommand).getByText(context.summary)).toBeInTheDocument();
    expect(within(viaCommand).getByText("History")).toBeInTheDocument();
    expect(screen.queryByText(/not reported by this runtime/)).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Conversation context" })).not.toBeInTheDocument();
  });

  it("reads unknown before any measured answer: an empty ring, no meter, and Rust's why", async () => {
    await draw();
    const ring = screen.getByRole("button", { name: `Context details: ${vm.context.summary}` });
    expect(ring).toHaveTextContent("—");
    expect(within(ring).queryByRole("meter")).toBeNull();
    const details = await runContextCommand();
    expect(details).toHaveTextContent(vm.context.summary);
    expect(details).not.toHaveTextContent(/usage/i);
  });
});

describe("the composer's bar (owner ruling 2026-10-27)", () => {
  it("names the model and effort as read-only chips when the agent offers no alternative", async () => {
    await draw();
    const bar = screen.getByRole("group", { name: "Model and effort" });
    expect(within(bar).getByText("beta-small")).toBeInTheDocument();
    expect(within(bar).getByText("Default")).toBeInTheDocument();
    expect(within(bar).queryByRole("button")).toBeNull();
  });

  it("opens the pickers from the same chips where Rust says there is a choice", async () => {
    await draw({
      ...vm,
      footer: {
        ...vm.footer,
        effort: "Medium",
        effortCode: "M",
        choosesModel: true,
        choosesEffort: true,
      },
      settings: {
        models: [
          { id: "alpha-large", provider: "Other", current: false, default: false },
          { id: "beta-small", provider: "Other", current: true, default: true },
        ],
        efforts: [
          { effort: "medium", label: "Medium", current: true, default: true },
          { effort: "high", label: "High", current: false, default: false },
        ],
        modelLabel: "beta-small",
        effortLabel: "Medium",
        resettable: false,
        write: { state: "confirmed" },
      },
    });
    expect(screen.getByRole("button", { name: "Effort: Medium" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Model: beta-small" }));
    expect(await screen.findByRole("dialog", { name: "Choose a model" })).toBeInTheDocument();
  });

  it("keeps the reading on an archived session, and its untrusted chip offers no fresh chat (R598-03)", async () => {
    const why =
      "This chat read outside content, such as a web page, so its commands need your approval.";
    render(
      <TooltipProvider>
        <SessionView target={{ database: "beta", sessionId: "same.1" }} />
      </TooltipProvider>,
    );
    await waitFor(() => expect(mock.view).not.toBeNull());
    // An archived session takes no drop, so `draw`'s wait for one does not apply.
    act(() =>
      mock.view?.({
        ...vm,
        archived: true,
        readOnly: true,
        // Rust leaves the way out off where `/clean` refuses.
        untrusted: { sentence: why },
        context: {
          ...vm.context,
          label: "5.3k / 1M",
          summary: "1% · used ≈ 5347 / 1048576 tokens",
        },
      }),
    );
    expect(screen.queryByRole("textbox", { name: "Message" })).toBeNull();
    const bar = screen.getByRole("group", { name: "Model and effort" });
    expect(within(bar).getByText("beta-small")).toBeInTheDocument();
    expect(within(bar).queryByRole("button", { name: /^Model:/ })).toBeNull();
    fireEvent.click(
      within(screen.getByRole("banner")).getByRole("button", { name: `Untrusted: ${why}` }),
    );
    const why_ = await screen.findByRole("dialog", { name: "Why commands ask" });
    expect(why_).toHaveTextContent(why);
    expect(within(why_).queryByRole("button")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Context details: 1% · used ≈ 5347 / 1048576 tokens" }),
    ).toHaveTextContent("5.3k / 1M");
  });

  it("marks an untrusted session in the header, says why in the banner, and offers a fresh chat", async () => {
    const why =
      "This chat read outside content, such as a web page, so its commands need your approval.";
    const fresh = "Start a fresh chat: starts trusted, keeps the drives you can still tick";
    const next = { database: "beta", sessionId: "same.2" };
    mock.command.mockResolvedValue({ session: next, surface: null, message: "" });
    const onSession = vi.fn();
    await draw(
      {
        ...vm,
        untrusted: { sentence: why, freshChat: fresh },
        banner: {
          sentence: "Sending a message declines this request, even a message that says yes.",
          approvals: [
            {
              id: "a6",
              database: "beta",
              sessionId: "same.1",
              toolName: "run",
              tier: "T3",
              summary: "Run git log",
              action: '{"argv":["git","log"]}',
              bindingDigest: "d6",
              expiresAtMs: 1800000000000,
              createdAtMs: 1780000000000,
              state: "pending",
              canDecide: true,
              sentence: "Waiting for you.",
              actions: ["allowOnce", "deny"],
              declinedBySend: true,
            },
          ],
        },
      },
      onSession,
    );
    // A session-state chip on the header's meta line, not a control in the composer's bar.
    const chip = within(screen.getByRole("banner")).getByRole("button", {
      name: `Untrusted: ${why}`,
    });
    expect(
      within(screen.getByRole("group", { name: "Model and effort" })).queryByRole("button", {
        name: /Untrusted/,
      }),
    ).toBeNull();
    expect(
      screen.getByRole("complementary", { name: "Waiting for your decision" }),
    ).toHaveTextContent(why);
    // The Agents window shows another session meanwhile.
    const shownInWindow = { database: "alpha", sessionId: "other.1" };
    agentSessionsStore.getState().select(shownInWindow);
    fireEvent.click(chip);
    const popover = await screen.findByRole("dialog", { name: "Why commands ask" });
    expect(popover).toHaveTextContent(why);
    fireEvent.click(within(popover).getByRole("button", { name: fresh }));
    // `/clean` through the composer's command path; it declines nothing the banner shows.
    await waitFor(() =>
      expect(mock.command).toHaveBeenCalledWith(
        { database: "beta", sessionId: "same.1" },
        "/clean",
        null,
        [],
      ),
    );
    // The host that holds its own choice (the Notes Assistant dock) moves to the fresh chat, and
    // the Agents window's selection stays where it was (R601-04).
    await waitFor(() => expect(onSession).toHaveBeenCalledWith(next));
    expect(agentSessionsStore.getState().selected).toEqual(shownInWindow);
  });

  it("moves the Agents window to the fresh chat where no host holds its own choice", async () => {
    const next = { database: "beta", sessionId: "same.2" };
    mock.command.mockResolvedValue({ session: next, surface: null, message: "" });
    agentSessionsStore.getState().select({ database: "beta", sessionId: "same.1" });
    await draw({
      ...vm,
      untrusted: {
        sentence: "This chat read outside content.",
        freshChat: "Start a fresh chat: starts trusted, keeps the drives you can still tick",
      },
    });
    fireEvent.click(
      within(screen.getByRole("banner")).getByRole("button", { name: /^Untrusted:/ }),
    );
    const popover = await screen.findByRole("dialog", { name: "Why commands ask" });
    fireEvent.click(within(popover).getByRole("button", { name: /^Start a fresh chat/ }));
    await waitFor(() => expect(agentSessionsStore.getState().selected).toEqual(next));
  });

  it("says why a fresh chat could not start, in the chip's popover", async () => {
    const why =
      "This chat read outside content, such as a web page, so its commands need your approval.";
    const fresh = "Start a fresh chat: starts trusted, keeps the drives you can still tick";
    mock.command.mockRejectedValue({
      code: "internal",
      message: "Unarchive this conversation first.",
      accountId: null,
    });
    await draw({ ...vm, untrusted: { sentence: why, freshChat: fresh } });
    fireEvent.click(
      within(screen.getByRole("banner")).getByRole("button", { name: `Untrusted: ${why}` }),
    );
    const popover = await screen.findByRole("dialog", { name: "Why commands ask" });
    fireEvent.click(within(popover).getByRole("button", { name: fresh }));
    expect(await within(popover).findByRole("alert")).toHaveTextContent(
      "Unarchive this conversation first.",
    );
  });
});

describe("session composer attachments", () => {
  it("sends a picked batch with one caption into its own database, without a second text message", async () => {
    await draw();
    // Attach, camera and photo share the bar's one `+` menu (FooterOneRow).
    fireEvent.pointerDown(screen.getByRole("button", { name: "Attach" }), {
      button: 0,
      ctrlKey: false,
    });
    expect(
      within(await screen.findByRole("menu"))
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Attach file", "Choose photo"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Attach file" }));
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
        [],
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
      [],
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
      [],
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
      [],
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
      [],
    );
  });

  /** After a cancelled or failed batch: its late read adds nothing, and the text sends alone. */
  async function lateReadAddsNothingAndTextSends(finish: () => Promise<void>, text: string) {
    await finish();
    expect(screen.queryByRole("list", { name: "Pending attachments" })).not.toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Files being read" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(mock.send).toHaveBeenCalledOnce());
    expect(mock.send).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, text, []);
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
      [],
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
    // The composer drops a paste while a send is in flight, and the send is in flight until
    // its field is spent — `agentSendLine` being called is not the send being over.
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(""));
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

describe("session header drives", () => {
  it("draws the drive control only where Rust sends the selection (a person member)", async () => {
    await draw();
    // The base view carries no `drives`: anyone but a person member sees no control.
    expect(screen.queryByRole("button", { name: "Drives" })).toBeNull();
    act(() =>
      mock.view?.({
        ...vm,
        drives: {
          drives: [{ name: "beta", ticked: true }],
          warning: "Once you tick a drive…",
          pending: false,
        },
      }),
    );
    expect(screen.getByRole("button", { name: "Drives" })).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Drives ticked" })).getByText("beta"));
  });

  const withDrives: AgentSessionViewVm = {
    ...vm,
    drives: { drives: [{ name: "beta", ticked: false }], warning: "Once…", pending: false },
  };

  it("opens its Drives menu when the list's right-click menu asked for it", async () => {
    agentSessionsStore.getState().openDrives({ database: "beta", sessionId: "same.1" });
    await draw(withDrives);
    expect(await screen.findByRole("menu", { name: "Drives" })).toBeInTheDocument();
    expect(agentSessionsStore.getState().drivesFor).toBeNull();
  });

  it("drops a Drives request its first view answers without drives (R10923-09)", async () => {
    agentSessionsStore.getState().openDrives({ database: "beta", sessionId: "same.1" });
    await draw();
    expect(agentSessionsStore.getState().drivesFor).toBeNull();
    // The person's role comes back later: the menu does not open on its own.
    act(() => mock.view?.(withDrives));
    expect(screen.getByRole("button", { name: "Drives" })).toBeInTheDocument();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("ignores a Drives request for another session", async () => {
    agentSessionsStore.getState().openDrives({ database: "alpha", sessionId: "same.1" });
    await draw(withDrives);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(agentSessionsStore.getState().drivesFor).toEqual({
      database: "alpha",
      sessionId: "same.1",
    });
    agentSessionsStore.getState().drivesShown();
  });
});

describe("Model… from the list (A109-05)", () => {
  const withModels: AgentSessionViewVm = {
    ...vm,
    footer: { ...vm.footer, choosesModel: true },
    settings: {
      models: [
        { id: "alpha-large", provider: "Other", current: false, default: false },
        { id: "beta-small", provider: "Other", current: true, default: true },
      ],
      efforts: [],
      modelLabel: "beta-small",
      resettable: false,
      write: { state: "confirmed" },
    },
  };

  it("opens the composer's model picker when the list's right-click menu asked for it", async () => {
    agentSessionsStore.getState().openModel({ database: "beta", sessionId: "same.1" });
    await draw(withModels);
    const picker = await screen.findByRole("dialog", { name: "Choose a model" });
    expect(within(picker).getByRole("option", { name: "beta-small, in use, default" }));
    expect(agentSessionsStore.getState().modelFor).toBeNull();
  });

  it("drops a Model request its first view answers without a picker", async () => {
    agentSessionsStore.getState().openModel({ database: "beta", sessionId: "same.1" });
    await draw();
    expect(agentSessionsStore.getState().modelFor).toBeNull();
    act(() => mock.view?.(withModels));
    expect(screen.getByRole("button", { name: "Model: beta-small" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("the approval banner", () => {
  const waiting = (id: string, digest: string) => ({
    id,
    database: "beta",
    sessionId: "same.1",
    toolName: "run",
    tier: "T2",
    summary: `Run ${id}`,
    action: '{"argv":["ls"]}',
    bindingDigest: digest,
    expiresAtMs: 1800000000000,
    createdAtMs: 1780000000000,
    state: "pending" as const,
    canDecide: true,
    sentence: "Waiting for you.",
    actions: ["allowOnce" as const, "deny" as const],
    declinedBySend: true,
  });
  const banner = {
    approvals: [waiting("a1", "d1"), waiting("a2", "d2")],
    sentence: "Sending a message declines these 2 requests, even a message that says yes.",
  };

  it("sends the requests it showed, at their digests, with the message", async () => {
    await draw();
    act(() => mock.view?.({ ...vm, banner }));
    expect(
      screen.getByRole("complementary", { name: "Waiting for your decision" }),
    ).toHaveTextContent("declines these 2 requests");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "yes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(mock.send).toHaveBeenCalledWith({ database: "beta", sessionId: "same.1" }, "yes", [
        { id: "a1", bindingDigest: "d1" },
        { id: "a2", bindingDigest: "d2" },
      ]),
    );
  });

  it("says what waits for another device, keeps it at the turn that asked, and a message then declines nothing (DW-E109-27)", async () => {
    const elsewhere =
      "1 request needs a decision from another device; a message here leaves it waiting.";
    const t4 = {
      ...waiting("t4", "d4"),
      toolName: "delegate",
      tier: "T4",
      summary: "Run t4",
      canDecide: false,
      sentence: "Needs a decision from another device.",
      actions: [],
      declinedBySend: false,
    };
    const asked: Ipc.AgentTurnFoldVm = {
      turnId: "t-asked",
      leadInIds: [],
      reasoning: null,
      steps: [{ id: "s1", name: "delegate", status: "parked", statusLabel: "Parked" }],
      failures: [],
      status: "waiting",
      statusLabel: "Waiting for you",
      summary: "Waiting for you · called delegate 1 time",
      durationMs: null,
      approvals: [],
      waiting: [t4],
    };
    await draw();
    act(() =>
      mock.view?.({
        ...vm,
        items: [{ kind: "fold", turnId: "t-asked" }],
        folds: [asked],
        banner: { approvals: [], sentence: elsewhere },
      }),
    );
    const region = screen.getByRole("complementary", { name: "Waiting for your decision" });
    expect(within(region).getByRole("note")).toHaveTextContent(elsewhere);
    expect(within(region).queryByRole("article")).toBeNull();
    // In sight at its turn, the fold closed: a pending request is never folded away.
    const card = screen.getByRole("article", { name: "Approval: Run t4" });
    expect(card.closest('[data-slot="turn-fold"]')).toHaveAttribute("data-turn-id", "t-asked");
    expect(card.closest('[data-slot="turn-fold-body"]')).toBeNull();
    expect(screen.queryByRole("region", { name: "Session approvals" })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "go on" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(mock.send).toHaveBeenCalledWith(
        { database: "beta", sessionId: "same.1" },
        "go on",
        [],
      ),
    );
  });

  it("moves focus to the fold a decided card folded into, else to the next request", async () => {
    mock.decide.mockResolvedValue({ state: "sent", sentence: "Approved." });
    await draw();
    act(() => mock.view?.({ ...vm, banner }));
    const region = () => screen.getByRole("complementary", { name: "Waiting for your decision" });
    const a1 = within(region()).getByRole("article", { name: "Approval: Run a1" });
    fireEvent.click(within(a1).getByRole("button", { name: "Allow once" }));
    await waitFor(() => expect(mock.decide).toHaveBeenCalledTimes(1));
    // Rust's next view moves a1, approved, into the fold of the turn that asked.
    const decided = {
      ...waiting("a1", "d1"),
      state: "approved" as const,
      canDecide: false,
      sentence: "Approved by Ana.",
      actions: [],
      declinedBySend: false,
    };
    act(() =>
      mock.view?.({
        ...vm,
        items: [{ kind: "fold", turnId: "t-asked" }],
        folds: [
          {
            turnId: "t-asked",
            leadInIds: [],
            reasoning: null,
            steps: [],
            failures: [],
            status: "waiting",
            statusLabel: "Waiting for you",
            summary: "Waiting for you",
            durationMs: null,
            approvals: [decided],
            waiting: [],
          },
        ],
        banner: { ...banner, approvals: [waiting("a2", "d2")] },
      }),
    );
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "Waiting for you" })),
    );
    expect(screen.queryByRole("article", { name: "Approval: Run a1" })).toBeNull();
    // a2 decided, and Rust's view has it nowhere: focus goes to the next request, else the
    // composer — never the body.
    const a2 = within(region()).getByRole("article", { name: "Approval: Run a2" });
    fireEvent.click(within(a2).getByRole("button", { name: "Deny" }));
    await waitFor(() => expect(mock.decide).toHaveBeenCalledTimes(2));
    act(() => mock.view?.({ ...vm, banner: undefined }));
    await waitFor(() => expect(document.activeElement).not.toBe(document.body));
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Message" }));
  });

  it("drops the refused-send alert once the banner's requests change", async () => {
    const said =
      "keeper could not decline “Run a1” on this device, so your message was not sent; the request still waits for you (disk full).";
    mock.send.mockRejectedValueOnce({ message: said });
    await draw();
    act(() => mock.view?.({ ...vm, banner }));
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "yes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    const region = () => screen.getByRole("complementary", { name: "Waiting for your decision" });
    await waitFor(() => expect(within(region()).getByRole("alert")).toHaveTextContent(said));
    // The same banner drawn again keeps it: nothing it names has changed.
    act(() => mock.view?.({ ...vm, banner: { ...banner } }));
    expect(within(region()).getByRole("alert")).toHaveTextContent(said);
    // a1 is decided elsewhere: the banner holds a2 alone, and the sentence about a1 goes.
    act(() => mock.view?.({ ...vm, banner: { ...banner, approvals: [waiting("a2", "d2")] } }));
    expect(within(region()).queryByRole("alert")).toBeNull();
  });

  it("says in the banner that a refused send left its requests waiting, and keeps the draft", async () => {
    const said =
      "keeper could not decline “Run a1” on this device, so your message was not sent; the request still waits for you (disk full).";
    mock.send.mockRejectedValueOnce({ message: said });
    await draw();
    act(() => mock.view?.({ ...vm, banner }));
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "yes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    const region = screen.getByRole("complementary", { name: "Waiting for your decision" });
    await waitFor(() => expect(within(region).getByRole("alert")).toHaveTextContent(said));
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("yes");
    // Announced once: the composer shows the sentence beside the draft, hidden from readers.
    expect(screen.getAllByRole("alert").filter((alert) => alert.textContent === said)).toHaveLength(
      1,
    );
    const copies = screen.getAllByText(said);
    expect(copies).toHaveLength(2);
    expect(copies.filter((copy) => copy.getAttribute("aria-hidden") === "true")).toHaveLength(1);
  });
});

describe("the reading column (109.14)", () => {
  const person: Ipc.AgentMessageVm = {
    id: "p1",
    role: "person",
    authorActor: "ana",
    mine: true,
    authorName: "Ana",
    body: "What will the weather be tomorrow?",
    createdAtMs: 1780000000000,
  };
  const lead: Ipc.AgentMessageVm = {
    id: "lead",
    role: "agent",
    authorActor: "assistant",
    mine: false,
    authorName: "Assistant",
    body: "Checking — one minute.",
    createdAtMs: 1780000001000,
  };
  const answer: Ipc.AgentMessageVm = {
    id: "answer",
    role: "agent",
    authorActor: "assistant",
    mine: false,
    authorName: "Assistant",
    body: "Cloudy, then sunny.",
    createdAtMs: 1780000015000,
  };
  const turn: Ipc.AgentTurnFoldVm = {
    turnId: "t1",
    leadInIds: ["lead"],
    reasoning: null,
    steps: [{ id: "s1", name: "web_search", status: "done", statusLabel: "Finished" }],
    failures: [{ stepId: "s2", sentence: "web_search · Refused: no more searches" }],
    status: "done",
    statusLabel: "Finished",
    summary: "Worked for 14 s · searched the web 2 times",
    durationMs: 14000,
    approvals: [],
    waiting: [],
  };
  const screenshot: AgentSessionViewVm = {
    ...vm,
    messages: [person, lead, answer],
    items: [
      { kind: "message", messageId: "p1" },
      { kind: "fold", turnId: "t1" },
      { kind: "message", messageId: "answer" },
    ],
    folds: [turn],
  };

  it("draws Rust's column in its order: the fold line above the answer, the answer last and outside it", async () => {
    await draw();
    act(() => mock.view?.(screenshot));
    const column = screen.getByRole("list", { name: "Messages" });
    const rows = Array.from(column.children);
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent(person.body);
    expect(rows[1]).toHaveAttribute("data-slot", "turn-fold");
    expect(rows[2]).toHaveTextContent(answer.body);
    // The answer is its own row, never inside the fold, opened or not.
    fireEvent.click(within(rows[1] as HTMLElement).getByRole("button"));
    expect(within(rows[1] as HTMLElement).queryByText(answer.body)).not.toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText(lead.body)).toBeInTheDocument();
  });

  it("never draws a message the column does not name: a lead-in lives in its fold only", async () => {
    await draw();
    act(() => mock.view?.(screenshot));
    // Folded, the lead-in is nowhere: a renderer that walked `messages`
    // itself would draw it as a row of its own.
    expect(screen.queryByText(lead.body)).not.toBeInTheDocument();
    expect(screen.getAllByText(answer.body)).toHaveLength(1);
  });

  it("folds a decided request with its turn and keeps a pending one in sight; nothing after the column", async () => {
    const card = (id: string, state: Ipc.AgentApprovalVm["state"], sentence: string) => ({
      id,
      database: "beta",
      sessionId: "same.1",
      toolName: "run",
      tier: "T2",
      summary: `Run ${id}`,
      bindingDigest: `digest-${id}`,
      action: '{"argv":["git","status"]}',
      expiresAtMs: 1800000000000,
      createdAtMs: 1780000002000,
      state,
      canDecide: false,
      sentence,
      actions: [],
      declinedBySend: false,
    });
    const asked: AgentSessionViewVm = {
      ...screenshot,
      folds: [
        {
          ...turn,
          approvals: [card("decided", "approved", "Approved by Ana.")],
          waiting: [card("bens", "pending", "Waiting for Ben.")],
        },
      ],
    };
    await draw();
    act(() => mock.view?.(asked));
    const column = screen.getByRole("list", { name: "Messages" });
    expect(column.nextElementSibling).toBeNull();
    expect(column.lastElementChild).toHaveTextContent(answer.body);
    // Normal: the decided card is folded with the work; the pending one is outside the body.
    expect(screen.queryByRole("article", { name: "Approval: Run decided" })).toBeNull();
    const bens = screen.getByRole("article", { name: "Approval: Run bens" });
    expect(bens.closest('[data-slot="turn-fold"]')).toHaveAttribute("data-turn-id", "t1");
    expect(bens.closest('[data-slot="turn-fold-body"]')).toBeNull();
    // Verbose: the decided card shows, inside its turn's opened fold.
    act(() => mock.view?.({ ...asked, prefs: { viewMode: "verbose" } }));
    const decided = screen.getByRole("article", { name: "Approval: Run decided" });
    expect(decided.closest('[data-slot="turn-fold-body"]')).not.toBeNull();
    expect(decided.closest('[data-slot="turn-fold"]')).toHaveAttribute("data-turn-id", "t1");
    expect(column.nextElementSibling).toBeNull();
  });

  it("opens every fold in the verbose mode", async () => {
    await draw();
    act(() => mock.view?.({ ...screenshot, prefs: { viewMode: "verbose" } }));
    expect(screen.getByRole("button", { name: turn.summary })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByText(lead.body)).toBeInTheDocument();
  });
});

/** A thread started here (109.24) whose newest turn ran tools: as if Rust had sent steps. */
const RUNNING_CHILD: AgentChildSessionVm = {
  threadId: "line-1",
  sessionId: "same.7",
  agentActor: "tola-grey",
  agentName: "Tola Grey",
  agentMark: "T",
  createdAtMs: 1,
  archived: false,
  truncated: false,
  prompt: {
    id: "line-1",
    role: "person",
    authorActor: "ana",
    mine: true,
    authorName: "Ana",
    body: "@tola-grey check the notes",
    createdAtMs: 1,
    state: "taken",
  },
  items: [
    { kind: "fold", turnId: "line-1" },
    { kind: "liveAnswer", turnId: "line-1" },
  ],
  folds: [
    {
      turnId: "line-1",
      leadInIds: [],
      reasoning: null,
      steps: [
        {
          id: "s1",
          name: "web_search",
          status: "started",
          statusLabel: "Started",
          startedAtMs: 2,
        },
      ],
      failures: [],
      status: "running",
      statusLabel: "Working",
      summary: "Searching the web… · 1 step",
      durationMs: null,
      approvals: [],
      waiting: [],
    },
  ],
  messages: [],
  live: [{ turnId: "line-1", text: "The notes say ship on Friday.", stopped: false }],
};

describe("a thread started here, inline (109.24)", () => {
  it("never opens its fold, even in Verbose: the answer is drawn and nothing expands", async () => {
    await draw({
      ...vm,
      prefs: { viewMode: "verbose" },
      children: [RUNNING_CHILD],
      items: [{ kind: "child", threadId: "line-1" }],
    });
    const thread = screen.getByRole("region", { name: "Thread with Tola Grey" });
    expect(within(thread).getByText("The notes say ship on Friday.")).toBeInTheDocument();
    expect(within(thread).getByText("Searching the web… · 1 step")).toBeInTheDocument();
    expect(within(thread).queryByRole("button", { expanded: true })).toBeNull();
    expect(within(thread).queryByRole("button", { expanded: false })).toBeNull();
    expect(within(thread).queryByText("web_search")).toBeNull();
  });

  it("leaves the parent one live region while a thread runs: its own answer's", async () => {
    await draw({
      ...vm,
      children: [RUNNING_CHILD],
      live: [{ turnId: "own", text: "Working on it", stopped: false }],
      items: [
        { kind: "child", threadId: "line-1" },
        { kind: "liveAnswer", turnId: "own" },
      ],
    });
    const column = screen.getByRole("list", { name: "Messages" });
    const live = column.querySelectorAll('[role="status"], [role="alert"], [aria-live]');
    expect(live).toHaveLength(1);
    expect(
      within(screen.getByRole("region", { name: "Thread with Tola Grey" })).queryByRole("status"),
    ).toBeNull();
    // Whose work the inline line is, in its accessible text.
    expect(
      within(screen.getByRole("region", { name: "Thread with Tola Grey" })).getByText(
        (_, node) =>
          node?.getAttribute("data-slot") === "inline-fold" &&
          node.textContent === "Tola Grey: Searching the web… · 1 step",
      ),
    ).toBeInTheDocument();
  });
});

/**
 * The Bots surface: gated by absence, streamed progressively, stoppable, and
 * honest about an answer that never finished (Epic 61, Story 61.4, FR-378,
 * FR-372).
 *
 * Four things are asserted here that nothing else in the tree asserts:
 *
 * 1. **Absence, not disabling** — with `CapabilitiesVm.bots` off there is no
 *    sidebar row and no pane, even with a stale `bots` primary-view. This is
 *    the mutation-sensitive one: flip either gate to always-true and these two
 *    tests fail while every other test in the file still passes.
 * 2. **The flag is `bots` and not `sessions`** — a machine with full folder
 *    sync and `bots` off must still show nothing, which is what catches a gate
 *    wired to the flag ⌘8 uses.
 * 3. **A streamed reply renders progressively** — the row exists before the
 *    first delta, because Rust persisted it before the request went out, and it
 *    grows as deltas land.
 * 4. **A dead stream leaves a partial row with a caption** — the terminal
 *    `closed` event carries `partial: true` and a reason, and what arrived is
 *    still on screen with a sentence saying it stopped.
 * 5. **The transcript is the pane's flexible box** (Story 61.14) — structurally:
 *    jsdom performs no layout, so the pixels were measured on Chrome through
 *    `dev/mock-shell.ts` and the block at the end guards the decisions those
 *    pixels were measured over.
 * 6. **The voice block folds to one line by default** (Story 64.1, AD-184) —
 *    where voice is available the pane shows the line and no control, one
 *    click unfolds the block, and the fold survives a remount through the
 *    pane's own cookie — the mount point's `hydrateBotsPaneFold`, which a
 *    store-level test cannot see (DW-172).
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { APPROVAL_DENY_LABEL, APPROVAL_ONCE_LABEL } from "@/components/bots/bot-approval-dialog";
import {
  BOT_COMPOSER_LABEL,
  BOT_COMPOSER_SEND_LABEL,
  BOT_COMPOSER_STOP_LABEL,
} from "@/components/bots/bot-composer";
import { BOT_CONTEXT_TITLE } from "@/components/bots/bot-context-note";
import { BOTS_EMPTY_COPY } from "@/components/bots/bot-empty-state";
import { GRANT_ADD_LABEL, GRANT_NONE_HELD } from "@/components/bots/bot-grant-bar";
import { BOT_PARTIAL_CAPTION, BOT_RETRY_LABEL } from "@/components/bots/bot-message";
import { BOT_SESSION_NEW_LABEL } from "@/components/bots/bot-session-list";
import { voiceFoldedLine, WAKE_SWITCH_LABEL } from "@/components/bots/bot-voice-wake";
import {
  BOTS_PANE_TITLE,
  BOTS_RAIL_LIST_LABEL,
  BOTS_TRANSCRIPT_LEVEL_SLOT,
  BotsPane,
} from "@/components/bots/bots-pane";
import { AppShell } from "@/components/layout/app-shell";
import { SidebarPane } from "@/components/layout/sidebar-pane";
import {
  COLUMN_COLLAPSE_PREFIX,
  COLUMN_RAIL_CONTROL_SLOT,
} from "@/components/layout/surface-column";
import { columnMinWidth, SURFACE_COLUMNS } from "@/lib/column-widths";
import type {
  BotMessageVm,
  BotModelVm,
  BotProviderVm,
  BotSessionVm,
  BotStreamEvent,
  BotVm,
  VoiceUnavailableVm,
  VoiceWakeVm,
} from "@/lib/ipc/client";
import { botsStore } from "@/lib/stores/bots";
import { BOTS_PANE_FOLD_COOKIE, resetBotsPaneFoldForTest } from "@/lib/stores/bots-pane-fold";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { resetColumnFoldForTest } from "@/lib/stores/column-fold";
import { primaryViewStore } from "@/lib/stores/primary-view";

const botsChatStop = vi.fn();
const botsApprovalAnswer = vi.fn();
const botsGrantsList = vi.fn();
/** The two voice facts (Story 64.1). Unanswered — rejected — by default, so
 *  the block is absent in every test that is not about it. */
const voiceAvailability = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceWakeGet = vi.fn<() => Promise<VoiceWakeVm>>();
/** The event sink the pane handed to `botsChatSend`, so the test can drive the
 *  stream exactly as Rust would. */
let sink: ((event: BotStreamEvent) => void) | null = null;

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsProvidersList: () => Promise.resolve([PROVIDER]),
    botsBotsList: () => Promise.resolve([BOT]),
    botsSessionsList: () => Promise.resolve([] as BotSessionVm[]),
    // Story 61.6's list queries its own bounded page. Answered here so the
    // pane's own tests exercise the real component with no conversations
    // rather than its read-failed sentence.
    botsSessionsSearch: () => Promise.resolve({ rows: [], total: 0 }),
    botsModelsList: () => Promise.resolve([MODEL]),
    botsSessionOpen: () => Promise.reject(new Error("not used")),
    botsChatSend: (_req: unknown, onEvent: (event: BotStreamEvent) => void) => {
      sink = onEvent;
      return Promise.resolve(SUBSCRIPTION_ID);
    },
    botsMessageRetry: (_req: unknown, onEvent: (event: BotStreamEvent) => void) => {
      sink = onEvent;
      return Promise.resolve(SUBSCRIPTION_ID);
    },
    botsChatStop: (id: string) => {
      botsChatStop(id);
      return Promise.resolve();
    },
    botsApprovalAnswer: (requestId: string, approved: boolean) => {
      botsApprovalAnswer(requestId, approved);
      return Promise.resolve();
    },
    // The grant bar's read (Story 61.10). Mocked so the `botTools` cases below
    // exercise the real bar over an empty table rather than its read-failed
    // sentence; the cases without `botTools` must never reach it at all.
    botsGrantsList: () => {
      botsGrantsList();
      return Promise.resolve({ grants: [], unknown: [] });
    },
    voiceAvailability: () => voiceAvailability(),
    voiceWakeGet: () => voiceWakeGet(),
    voiceWatch: () => Promise.reject(new Error("not used")),
    voiceUnwatch: () => Promise.resolve(),
  };
});

const SUBSCRIPTION_ID = "sub-1";

const PROVIDER: BotProviderVm = {
  id: "prov-1",
  kind: "ollama",
  name: "Ollama here",
  baseUrl: "http://localhost:11434",
  host: "localhost",
  isPrivate: true,
  createdMs: 1,
  health: "reachable",
  healthCheckedMs: 2,
  healthDetail: null,
  readTimeoutMs: null,
  hasToken: false,
};

const BOT: BotVm = {
  id: "bot-1",
  providerId: "prov-1",
  target: "llama4:8b",
  name: "Llama",
  pinOrder: 0,
  shape: null,
  colour: null,
  mark: null,
  createdMs: 1,
};

const MODEL: BotModelVm = {
  id: "llama4:8b",
  family: "llama4",
  parameterSize: "8.0B",
  quantization: null,
  sizeBytes: null,
  contextWindow: null,
  maxOutputTokens: null,
  vision: false,
  tools: true,
  reasoning: false,
  capabilities: ["completion", "tools"],
};

const SESSION: BotSessionVm = {
  id: "sess-1",
  botId: "bot-1",
  providerId: "prov-1",
  title: "What changed?",
  createdMs: 1,
  updatedMs: 1,
  archived: false,
  remoteSessionId: null,
  // Epic 63's two gateway facts: absent on a row no gateway described.
  remoteLastActiveMs: null,
  remoteSource: null,
};

function message(overrides: Partial<BotMessageVm> & { id: string; role: string }): BotMessageVm {
  return {
    sessionId: "sess-1",
    seq: 0,
    content: "",
    model: null,
    providerId: null,
    promptTokens: null,
    completionTokens: null,
    totalTokens: null,
    ttftMs: null,
    durationMs: null,
    finishReason: null,
    requestId: null,
    toolCallCount: 0,
    partial: false,
    createdMs: 1,
    ...overrides,
  };
}

const QUESTION = message({ id: "msg-user", role: "user", seq: 0, content: "What changed?" });
const ANSWER = message({ id: "msg-answer", role: "assistant", seq: 1, partial: true });

/** A build that can hold a conversation and cannot reach the drive: a desktop
 *  with no folder sync, or a phone (Epic 62). `botTools` off. */
const WITH_BOTS = { ...DEFAULT_CAPABILITIES, bots: true };

/** A desktop that can also reach the drive: both halves on. */
const WITH_BOT_TOOLS = { ...DEFAULT_CAPABILITIES, bots: true, botTools: true, sync: true };

/** Full folder sync, `bots` off — the state a gate on the wrong flag misreads. */
const SYNC_WITHOUT_BOTS = {
  ...DEFAULT_CAPABILITIES,
  sync: true,
  notes: true,
  sessions: true,
  bots: false,
};

beforeEach(() => {
  sink = null;
  botsChatStop.mockClear();
  botsApprovalAnswer.mockClear();
  botsGrantsList.mockClear();
  voiceAvailability.mockReset();
  voiceWakeGet.mockReset();
  voiceAvailability.mockRejectedValue(new Error("not used"));
  voiceWakeGet.mockRejectedValue(new Error("not used"));
  botsStore.getState().reset();
  capabilitiesStore.getState().applySnapshot(WITH_BOTS);
});

afterEach(() => {
  primaryViewStore.getState().setView("inbox");
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  botsStore.getState().reset();
  resetColumnFoldForTest();
  resetBotsPaneFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing the fold this suite wrote
  document.cookie = `${BOTS_PANE_FOLD_COOKIE}=; path=/; max-age=0`;
});

describe("the Bots surface's capability gate", () => {
  it("offers the sidebar row when the capability is on", () => {
    render(<SidebarPane collapsed={false} onToggleFold={null} />);
    expect(screen.getByRole("button", { name: "Bots" })).toBeInTheDocument();
  });

  it("has NO sidebar row when the capability is off", () => {
    // Absent, not disabled: a greyed row answering "unsupported on this
    // platform" is a worse answer than no row.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    render(<SidebarPane collapsed={false} onToggleFold={null} />);
    expect(screen.queryByRole("button", { name: "Bots" })).not.toBeInTheDocument();
  });

  it("has no sidebar row on a full sync machine whose bots flag is off", () => {
    // Wire the row to `capabilities.sessions` — the flag Tasks uses — and the
    // test above still passes while this one fails.
    capabilitiesStore.getState().applySnapshot(SYNC_WITHOUT_BOTS);
    render(<SidebarPane collapsed={false} onToggleFold={null} />);
    expect(screen.queryByRole("button", { name: "Bots" })).not.toBeInTheDocument();
    // And the surfaces that DO ride the sync gate are present, so the fixture
    // is proving a distinction rather than an empty sidebar.
    expect(screen.getByRole("button", { name: "Tasks" })).toBeInTheDocument();
  });

  it("renders the pane for the bots view when the capability is on", async () => {
    primaryViewStore.getState().setView("bots");
    render(<AppShell />);
    expect(screen.getByRole("region", { name: BOTS_PANE_TITLE })).toBeInTheDocument();
    // And it replaces the chat cluster rather than sitting beside it.
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
    await waitFor(() => expect(botsStore.getState().bots).not.toBeNull());
  });

  it("does not render the pane when the capability is off", () => {
    // A stale "bots" primary-view must never show the pane on a build with no
    // bots surface.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    primaryViewStore.getState().setView("bots");
    render(<AppShell />);
    expect(screen.queryByRole("region", { name: BOTS_PANE_TITLE })).not.toBeInTheDocument();
  });
});

describe("the drive half's capability gate (Epic 62, FR-396)", () => {
  /** Send one question and stream one answer, the way Rust would. */
  async function askAndAnswer() {
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));
    fireEvent.change(screen.getByLabelText(BOT_COMPOSER_LABEL), {
      target: { value: "What changed?" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_SEND_LABEL }));
    await waitFor(() => expect(sink).not.toBeNull());
    act(() => {
      sink?.({
        kind: "opened",
        subscriptionId: SUBSCRIPTION_ID,
        session: SESSION,
        user: QUESTION,
        assistant: ANSWER,
      });
      sink?.({ kind: "delta", text: "Nothing on your drive." });
      sink?.({
        kind: "closed",
        message: message({
          id: ANSWER.id,
          role: "assistant",
          seq: 1,
          content: "Nothing on your drive.",
          finishReason: "stop",
        }),
        reason: null,
      });
    });
  }

  it("offers the grant affordance where this build can reach the drive", async () => {
    capabilitiesStore.getState().applySnapshot(WITH_BOT_TOOLS);
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));
    expect(await screen.findByText(GRANT_NONE_HELD)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: GRANT_ADD_LABEL })).toBeInTheDocument();
    expect(botsGrantsList).toHaveBeenCalled();
  });

  it("has NO grant affordance where it cannot, and the conversation still works", async () => {
    // The phone's shape (Epic 62): `bots` on, `botTools` off. Absent, not
    // disabled — no grant sentence, no control, and no read of a grant table
    // the build could not act on — while a question is still asked and
    // answered. Flip `botTools` to always-true and this fails while the case
    // above still passes.
    capabilitiesStore.getState().applySnapshot(WITH_BOTS);
    render(<BotsPane />);
    await askAndAnswer();

    expect(screen.getByText("What changed?")).toBeInTheDocument();
    expect(screen.getByText("Nothing on your drive.")).toBeInTheDocument();
    expect(botsStore.getState().streamingId).toBeNull();

    expect(screen.queryByText(GRANT_NONE_HELD)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: GRANT_ADD_LABEL })).not.toBeInTheDocument();
    expect(botsGrantsList).not.toHaveBeenCalled();
  });
});

describe("BotsPane", () => {
  it("says what to do first when nothing has been asked", async () => {
    render(<BotsPane />);
    await waitFor(() =>
      expect(screen.getByText(BOTS_EMPTY_COPY["no-conversation"].message)).toBeInTheDocument(),
    );
  });

  it("renders a streamed reply progressively", async () => {
    render(<BotsPane />);
    // The picker resolves the model list and chooses the first, which is what
    // makes the composer live.
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));

    fireEvent.change(screen.getByLabelText(BOT_COMPOSER_LABEL), {
      target: { value: "What changed?" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_SEND_LABEL }));
    await waitFor(() => expect(sink).not.toBeNull());

    // Rust persisted both rows BEFORE the request went out, so the answer row
    // exists — empty and partial — on the first event.
    act(() => {
      sink?.({
        kind: "opened",
        subscriptionId: SUBSCRIPTION_ID,
        session: SESSION,
        user: QUESTION,
        assistant: ANSWER,
      });
    });
    expect(screen.getByText("What changed?")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Conversation" })).toBeInTheDocument();

    act(() => {
      sink?.({ kind: "firstToken", afterMs: 240 });
      sink?.({ kind: "delta", text: "Nothing " });
    });
    expect(screen.getByText("Nothing")).toBeInTheDocument();

    act(() => {
      sink?.({ kind: "delta", text: "on your drive." });
    });
    expect(screen.getByText("Nothing on your drive.")).toBeInTheDocument();

    // Closing replaces the row with what Rust stored — the pane never decides
    // an answer is finished.
    act(() => {
      sink?.({
        kind: "closed",
        message: message({
          id: ANSWER.id,
          role: "assistant",
          seq: 1,
          content: "Nothing on your drive.",
          finishReason: "stop",
        }),
        reason: null,
      });
    });
    expect(botsStore.getState().streamingId).toBeNull();
    expect(screen.getByText("Nothing on your drive.")).toBeInTheDocument();
  });

  it("stops a streaming answer by its subscription id", async () => {
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));
    fireEvent.change(screen.getByLabelText(BOT_COMPOSER_LABEL), {
      target: { value: "What changed?" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_SEND_LABEL }));
    await waitFor(() => expect(sink).not.toBeNull());
    act(() => {
      sink?.({
        kind: "opened",
        subscriptionId: SUBSCRIPTION_ID,
        session: SESSION,
        user: QUESTION,
        assistant: ANSWER,
      });
    });

    // Send is gone while an answer is arriving, and Stop is what is offered.
    expect(screen.queryByRole("button", { name: BOT_COMPOSER_SEND_LABEL })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_STOP_LABEL }));
    expect(botsChatStop).toHaveBeenCalledWith(SUBSCRIPTION_ID);
  });

  it("leaves a partial row with an honest caption when the stream dies", async () => {
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));
    fireEvent.change(screen.getByLabelText(BOT_COMPOSER_LABEL), {
      target: { value: "What changed?" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_SEND_LABEL }));
    await waitFor(() => expect(sink).not.toBeNull());

    act(() => {
      sink?.({
        kind: "opened",
        subscriptionId: SUBSCRIPTION_ID,
        session: SESSION,
        user: QUESTION,
        assistant: ANSWER,
      });
      sink?.({ kind: "delta", text: "Half a sen" });
    });
    // While it is arriving the row must NOT read as failed — the two states
    // look identical in the data and must not look identical on screen.
    expect(screen.queryByText(BOT_PARTIAL_CAPTION)).not.toBeInTheDocument();

    act(() => {
      sink?.({
        kind: "closed",
        message: message({
          id: ANSWER.id,
          role: "assistant",
          seq: 1,
          content: "Half a sen",
          finishReason: "failed",
          partial: true,
        }),
        reason: "The answer stopped before it finished.",
      });
    });

    // What arrived is kept, the caption says so, and Retry is offered on it.
    expect(screen.getByText("Half a sen")).toBeInTheDocument();
    expect(screen.getByText(BOT_PARTIAL_CAPTION)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: BOT_RETRY_LABEL })).toBeInTheDocument();
    // And the row on screen is the one Rust stored, still marked partial.
    const rows = botsStore.getState().conversation?.messages ?? [];
    const stored = rows[rows.length - 1];
    expect(stored?.partial).toBe(true);
    expect(stored?.content).toBe("Half a sen");
  });

  /** Open a stream with the standard question and answer rows. */
  async function openStream() {
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));
    fireEvent.change(screen.getByLabelText(BOT_COMPOSER_LABEL), {
      target: { value: "What changed?" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_SEND_LABEL }));
    await waitFor(() => expect(sink).not.toBeNull());
    act(() => {
      sink?.({
        kind: "opened",
        subscriptionId: SUBSCRIPTION_ID,
        session: SESSION,
        user: QUESTION,
        assistant: ANSWER,
      });
    });
  }

  it("renders a tool row under the arriving answer as the turn reports it", async () => {
    await openStream();
    act(() => {
      sink?.({
        kind: "toolResult",
        call: {
          id: "call-1",
          requestedName: "drive_read",
          name: "read",
          displayPath: "work/notes/a.md",
          refusal: null,
          grantDenied: false,
          arguments: '{"path":"work/notes/a.md"}',
          result: "# Notes",
          outcome: "text",
          bytes: 7,
          truncatedAtBytes: null,
          ofBytes: null,
          entries: null,
          truncatedAtEntries: null,
          ofEntries: null,
          okf: null,
        },
      });
    });
    // The row is on screen before the turn closes — a crash mid-loop has
    // already shown the call that ran.
    expect(screen.getByRole("button", { name: /read work\/notes\/a\.md/ })).toBeInTheDocument();
    expect(botsStore.getState().toolRows[ANSWER.id]).toHaveLength(1);
  });

  it("discloses what the model was told about the drive", async () => {
    await openStream();
    act(() => {
      sink?.({
        kind: "context",
        bundle: {
          preamble: "The blocks below are files from the user's own drive.",
          files: [{ subpath: "work/AGENTS.md", bytes: 120, ofBytes: 120, truncated: false }],
          skipped: [],
          totalBytes: 120,
        },
      });
    });
    expect(screen.getByText(BOT_CONTEXT_TITLE)).toBeInTheDocument();
    expect(botsStore.getState().contexts[ANSWER.id]?.files[0]?.subpath).toBe("work/AGENTS.md");
  });

  it("opens the approval sheet on an ask and answers it over IPC", async () => {
    await openStream();
    act(() => {
      sink?.({
        kind: "approvalAsked",
        request: {
          requestId: "ask-1",
          providerId: "prov-1",
          botId: "bot-1",
          tool: "drive_write",
          path: "work/notes/plan.md",
          profileId: "work",
          subpath: "notes/plan.md",
          effect: "write",
          reason: "This bot may write here, and keeper asks before every write.",
        },
      });
    });
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(botsStore.getState().pendingApproval?.request.requestId).toBe("ask-1");

    fireEvent.click(screen.getByRole("button", { name: APPROVAL_ONCE_LABEL }));
    await waitFor(() => expect(botsApprovalAnswer).toHaveBeenCalledWith("ask-1", true));
    expect(botsStore.getState().pendingApproval).toBeNull();
  });

  it("answers a dismissed ask with a refusal", async () => {
    await openStream();
    act(() => {
      sink?.({
        kind: "approvalAsked",
        request: {
          requestId: "ask-2",
          providerId: "prov-1",
          botId: "bot-1",
          tool: "drive_write",
          path: "work/notes/plan.md",
          profileId: "work",
          subpath: "notes/plan.md",
          effect: "write",
          reason: "This bot may write here, and keeper asks before every write.",
        },
      });
    });
    fireEvent.click(screen.getByRole("button", { name: APPROVAL_DENY_LABEL }));
    await waitFor(() => expect(botsApprovalAnswer).toHaveBeenCalledWith("ask-2", false));
  });
});

/**
 * What the layout depends on, as facts a test can hold (Story 61.14).
 *
 * Measured on Chrome at 1440×1050 before this story: the chrome above and
 * below the transcript was 670px of a 1022px pane and the transcript 353px,
 * with nothing in the pane a scroller. jsdom performs no layout, so nothing
 * here is a height and nothing pretends to be: `dev/mock-shell.ts` under a
 * real engine measured the pixels, and this block guards the structure those
 * pixels were measured over. Each of these fails if the corresponding decision
 * is reverted, and none of them can tell you the transcript is readable.
 */
describe("the Bots pane's layout contract", () => {
  it("makes the transcript the one flexible box and bounds every band around it", async () => {
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));

    // The level that holds the transcript is the flexible half of the row, and
    // it may shrink: without `min-h-0` a flex column's content is its floor
    // and the composer is laid out under the window edge.
    const level = document.querySelector(`[data-slot="${BOTS_TRANSCRIPT_LEVEL_SLOT}"]`);
    expect(level).not.toBeNull();
    expect(level).toHaveClass("flex", "flex-col", "flex-1", "min-h-0");

    // Inside it, exactly one child is flexible — the transcript's box, which
    // here is the empty state standing where the transcript would — and every
    // other band is `shrink-0`: bounded chrome, not a claimant.
    const children = [...(level as HTMLElement).children];
    const flexible = children.filter((child) => child.classList.contains("flex-1"));
    expect(flexible).toHaveLength(1);
    expect(flexible[0]).toHaveClass("min-h-0");
    expect(flexible[0]).toHaveTextContent(BOTS_EMPTY_COPY["no-conversation"].message);
    for (const child of children) {
      if (child !== flexible[0]) {
        expect(child).toHaveClass("shrink-0");
      }
    }
  });

  it("draws the conversation list as a column beside the transcript, not a band above it", async () => {
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().sessions).not.toBeNull());

    // A surface column: named from its own title band, floored at the
    // registry's number, and able to give width rather than `shrink-0`.
    const column = screen.getByRole("region", { name: SURFACE_COLUMNS["bots-list"].title });
    expect(column.id).toBe("column-bots-list");
    expect(column).not.toHaveClass("shrink-0");
    expect(column.style.minWidth).toBe(`${columnMinWidth("bots-list")}px`);
    // The list lives INSIDE the column, and the column and the transcript
    // level are siblings in one row — the shape a list above the transcript
    // cannot have.
    const level = document.querySelector(`[data-slot="${BOTS_TRANSCRIPT_LEVEL_SLOT}"]`);
    expect(column.parentElement).toBe(level?.parentElement);
    expect(column.parentElement).toHaveClass("flex", "min-h-0", "flex-1");
    expect(within(column).getByRole("button", { name: BOT_SESSION_NEW_LABEL })).toBeInTheDocument();
  });

  it("keeps the count and New reachable from the folded rail", async () => {
    render(<BotsPane />);
    await waitFor(() => expect(botsStore.getState().sessions).not.toBeNull());

    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["bots-list"].label}`,
      }),
    );
    // Folded, the body is gone — that is the height and the subscriptions the
    // fold reclaims — and the rail says what it holds and gives it back.
    expect(screen.queryByLabelText("Search conversations")).toBeNull();
    const rail = [...document.querySelectorAll(`[data-slot="${COLUMN_RAIL_CONTROL_SLOT}"]`)].map(
      (control) => control.getAttribute("aria-label"),
    );
    expect(rail[0]).toMatch(new RegExp(`^${BOTS_RAIL_LIST_LABEL}, .*conversations`));
    expect(rail).toContain(BOT_SESSION_NEW_LABEL);
  });
});

/**
 * The voice block, folded (Story 64.1, FR-427, FR-428, AD-184).
 *
 * Measured with `dev/measure-bots.ts` on Chrome at 1440×900 over the dev
 * shell: before, the transcript was 259px of the 872px pane (29.7%) under a
 * 223px block; folded by default it is 444px (50.9%) under a 39px line. What
 * jsdom can hold is the structure: the band is `shrink-0` either way, folded
 * it is a line and no control, and the mount point restores the fold from
 * the cookie.
 */
describe("the Bots pane's voice block folds (Story 64.1)", () => {
  const WAKE: VoiceWakeVm = {
    enabled: false,
    phrase: "nixie",
    limits: "Listening uses the microphone.",
    locale: "en-US",
    localeChosen: null,
    onDeviceLocales: ["en-US"],
  };
  const NOT_AUTHORIZED: VoiceUnavailableVm = {
    kind: "notAuthorized",
    message:
      "keeper is not allowed to use the microphone or speech recognition on this Mac — allow both under System Settings > Privacy & Security",
  };

  /** The band the pane draws for the voice block: the disclosure's section. */
  function band(): HTMLElement {
    const disclosure = screen.getByRole("button", { name: /^(Expand|Collapse) Listening/ });
    const section = disclosure.closest("section");
    if (section === null) {
      throw new Error("the disclosure is not inside its band");
    }
    return section;
  }

  beforeEach(() => {
    voiceAvailability.mockResolvedValue(NOT_AUTHORIZED);
    voiceWakeGet.mockResolvedValue(WAKE);
  });

  it("is one line by default, saying the setting and the refusal, with no control", async () => {
    render(<BotsPane />);
    const disclosure = await screen.findByRole("button", {
      name: `Expand ${voiceFoldedLine(WAKE, NOT_AUTHORIZED)}`,
    });
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(disclosure).toHaveTextContent(
      "Listening off · en-US · keeper is not allowed to use the microphone or speech recognition on this Mac",
    );
    expect(screen.queryByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeNull();

    // The 61.14 contract with the band present: it sits in the transcript
    // level, bounded, and the transcript is still the one flexible box.
    const level = document.querySelector(`[data-slot="${BOTS_TRANSCRIPT_LEVEL_SLOT}"]`);
    expect(band().parentElement).toBe(level);
    expect(band()).toHaveClass("shrink-0");
    const flexible = [...(level as HTMLElement).children].filter((child) =>
      child.classList.contains("flex-1"),
    );
    expect(flexible).toHaveLength(1);
    expect(flexible[0]).toHaveTextContent(BOTS_EMPTY_COPY["no-conversation"].message);
  });

  it("unfolds on one click to the whole block, still bounded", async () => {
    render(<BotsPane />);
    fireEvent.click(await screen.findByRole("button", { name: /^Expand Listening/ }));
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
    // Scoped: the composer's own status line is another `status` in the pane.
    expect(within(band()).getByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message);
    expect(band()).toHaveClass("shrink-0");
  });

  it("remembers the unfold across a remount, through the pane's own cookie", async () => {
    const first = render(<BotsPane />);
    fireEvent.click(await screen.findByRole("button", { name: /^Expand Listening/ }));
    expect(document.cookie).toContain(`${BOTS_PANE_FOLD_COOKIE}=${encodeURIComponent("voice:0")}`);
    first.unmount();

    // A fresh document's store: unhydrated, at its default. Only the pane's
    // own `hydrateBotsPaneFold` can bring the cookie back, and a pane that
    // forgot to call it would start folded here.
    resetBotsPaneFoldForTest();
    render(<BotsPane />);
    expect(await screen.findByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Collapse Listening/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });
});

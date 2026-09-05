/**
 * Following the conversation from the other device (Epic 63, Story 63.7,
 * FR-425, FR-426, AD-177).
 *
 * What is asserted here and nowhere else:
 *
 * 1. **The caption says following, not streaming** — while Rust reports a
 *    turn open on the other device, the transcript carries one sentence that
 *    names the difference, and the merged rows Rust composed replace what was
 *    on screen.
 * 2. **It is absent while this device is the one streaming** — an `opened`
 *    event clears it, and no further read is scheduled under the stream.
 * 3. **The timer is Rust's decision** — a read that says `nextPollMs: null`
 *    is the last read; a local transcript is never read at all.
 * 4. **A read landing under a stream is dropped whole** — the store rule,
 *    asserted directly.
 *
 * `botsSessionFollow` is mocked at the IPC boundary and nothing below it;
 * the intervals it answers are short so the tests run on real timers.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BOT_FOLLOWING_CAPTION, BotConversation } from "@/components/bots/bot-conversation";
import { BOT_STREAMING_CAPTION } from "@/components/bots/bot-message";
import type { BotConversationVm, BotFollowVm, BotMessageVm, BotSessionVm } from "@/lib/ipc/client";
import { botsStore, useBotsStore } from "@/lib/stores/bots";

const botsSessionFollow = vi.fn<(sessionId: string) => Promise<BotFollowVm>>();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsSessionFollow: (sessionId: string) => botsSessionFollow(sessionId),
  };
});

const SESSION: BotSessionVm = {
  id: "sess-1",
  botId: "bot-1",
  providerId: "prov-1",
  title: "What changed?",
  createdMs: 1,
  updatedMs: 1,
  archived: false,
  remoteSessionId: "hermes-1",
  remoteLastActiveMs: 1,
  remoteSource: "api",
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

const QUESTION = message({ id: "1", role: "user", content: "What changed?" });
const ANSWER = message({ id: "2", role: "assistant", seq: 1, content: "Three notes." });
/** The other device's next question, as Hermes flushes it at turn start. */
const THEIR_QUESTION = message({ id: "3", role: "user", seq: 2, content: "And the drive?" });

function conversation(transcript: BotConversationVm["transcript"]): BotConversationVm {
  return { session: SESSION, messages: [QUESTION, ANSWER], transcript };
}

/** A stable empty list, so the selector below never hands React a fresh array. */
const NO_MESSAGES: BotMessageVm[] = [];

/** Render the list over the store's conversation, the way both panes do. */
function Surface() {
  const messages = useBotsStore((s) => s.conversation?.messages ?? NO_MESSAGES);
  const streamingMessageId = useBotsStore((s) => s.streamingMessageId);
  return (
    <BotConversation
      messages={messages}
      streamingMessageId={streamingMessageId}
      retryableId={null}
      onRetry={() => {}}
    />
  );
}

/** Start an answer here, the way Rust's `opened` does. */
function startStreamingHere() {
  act(() => {
    botsStore.getState().applyStreamEvent({
      kind: "opened",
      subscriptionId: "sub-1",
      session: SESSION,
      user: message({ id: "L1", role: "user", seq: 3, content: "Mine" }),
      assistant: message({ id: "L2", role: "assistant", seq: 4, partial: true }),
    });
  });
}

/** Long enough for a 5 ms follow interval to have fired several times.
 *  Executor form: the project's TS lib predates `Promise.withResolvers`. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 40));
}

beforeEach(() => {
  botsStore.getState().reset();
  botsSessionFollow.mockReset();
});

afterEach(() => {
  botsStore.getState().reset();
});

describe("BotConversation — following the other device", () => {
  it("says the steps land as they complete, and shows the merged transcript", async () => {
    botsSessionFollow.mockResolvedValue({
      messages: [QUESTION, ANSWER, THEIR_QUESTION],
      live: true,
      nextPollMs: null,
    });
    botsStore.getState().openConversation(conversation("remote"));
    render(<Surface />);

    const caption = await screen.findByRole("status");
    expect(caption).toHaveTextContent(BOT_FOLLOWING_CAPTION);
    expect(caption).not.toHaveTextContent(BOT_STREAMING_CAPTION);
    expect(screen.getByText("And the drive?")).toBeInTheDocument();
    expect(botsSessionFollow).toHaveBeenCalledWith(SESSION.id);
    expect(botsStore.getState().follow).toEqual({ live: true, nextPollMs: null });
  });

  it("carries no caption while the turn is closed over there", async () => {
    botsSessionFollow.mockResolvedValue({ messages: null, live: false, nextPollMs: null });
    botsStore.getState().openConversation(conversation("remote"));
    render(<Surface />);
    await waitFor(() =>
      expect(botsStore.getState().follow).toEqual({ live: false, nextPollMs: null }),
    );
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    // `messages: null` left the transcript as it was.
    expect(botsStore.getState().conversation?.messages).toEqual([QUESTION, ANSWER]);
  });

  it("is absent while this device is the one streaming, and reads nothing under the stream", async () => {
    botsSessionFollow.mockResolvedValue({ messages: null, live: true, nextPollMs: 5 });
    botsStore.getState().openConversation(conversation("remote"));
    render(<Surface />);
    await screen.findByText(BOT_FOLLOWING_CAPTION);
    // The timer is real: a second read followed the first.
    await waitFor(() => expect(botsSessionFollow.mock.calls.length).toBeGreaterThanOrEqual(2));

    startStreamingHere();
    const reads = botsSessionFollow.mock.calls.length;
    expect(screen.queryByText(BOT_FOLLOWING_CAPTION)).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(BOT_STREAMING_CAPTION);
    expect(botsStore.getState().follow).toBeNull();
    await settle();
    expect(botsSessionFollow.mock.calls.length).toBe(reads);
  });

  it("stops when Rust says the session went cold", async () => {
    botsSessionFollow.mockResolvedValue({ messages: null, live: false, nextPollMs: null });
    botsStore.getState().openConversation(conversation("remote"));
    render(<Surface />);
    await waitFor(() => expect(botsSessionFollow).toHaveBeenCalledTimes(1));
    await settle();
    expect(botsSessionFollow).toHaveBeenCalledTimes(1);
  });

  it("never reads a transcript that is keeper's own", async () => {
    botsStore.getState().openConversation(conversation("local"));
    render(<Surface />);
    await settle();
    expect(botsSessionFollow).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("stops reading when the list leaves the screen", async () => {
    botsSessionFollow.mockResolvedValue({ messages: null, live: true, nextPollMs: 5 });
    botsStore.getState().openConversation(conversation("remote"));
    const { unmount } = render(<Surface />);
    await waitFor(() => expect(botsSessionFollow.mock.calls.length).toBeGreaterThanOrEqual(2));
    unmount();
    const reads = botsSessionFollow.mock.calls.length;
    await settle();
    expect(botsSessionFollow.mock.calls.length).toBe(reads);
    expect(botsStore.getState().follow).toBeNull();
  });
});

describe("the store's follow rules", () => {
  it("drops a read whole while an answer is streaming here", () => {
    botsStore.getState().openConversation(conversation("remote"));
    startStreamingHere();
    const before = botsStore.getState().conversation;
    const taken = botsStore.getState().applyFollow({
      messages: [QUESTION, ANSWER, THEIR_QUESTION],
      live: true,
      nextPollMs: 2000,
    });
    expect(taken).toBe(false);
    expect(botsStore.getState().conversation).toBe(before);
    expect(botsStore.getState().follow).toBeNull();
  });

  it("takes a read at rest, and opening another conversation forgets it", () => {
    botsStore.getState().openConversation(conversation("remote"));
    const taken = botsStore.getState().applyFollow({
      messages: [QUESTION, ANSWER, THEIR_QUESTION],
      live: true,
      nextPollMs: 2000,
    });
    expect(taken).toBe(true);
    expect(botsStore.getState().conversation?.messages).toHaveLength(3);
    expect(botsStore.getState().follow).toEqual({ live: true, nextPollMs: 2000 });
    botsStore.getState().openConversation(null);
    expect(botsStore.getState().follow).toBeNull();
  });
});

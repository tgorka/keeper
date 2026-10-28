/**
 * A mention's thread drawn inline in its parent (109.24): the agent's answer
 * under the person's line, "Open thread" opening the child, and a refused
 * line shown with Rust's sentence and nothing to open.
 */
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CHILD_CONTINUES, SessionChildThread } from "@/components/agents/session-mention";
import type { AgentChildSessionVm, AgentMessageVm } from "@/lib/ipc/client";

const PROMPT: AgentMessageVm = {
  id: "m-1",
  role: "person",
  authorActor: "ana",
  mine: true,
  authorName: "Ana",
  body: "@tola-grey what do the notes say?",
  createdAtMs: 1_000,
  state: "taken",
};

const ANSWERED: AgentChildSessionVm = {
  threadId: "m-1",
  sessionId: "dev1.7",
  archived: false,
  truncated: false,
  agentActor: "tola-grey",
  agentName: "Tola Grey",
  agentMark: "T",
  createdAtMs: 1_000,
  prompt: PROMPT,
  items: [
    { kind: "fold", turnId: "m-1" },
    { kind: "message", messageId: "a-1" },
  ],
  folds: [
    {
      turnId: "m-1",
      leadInIds: [],
      reasoning: null,
      steps: [],
      failures: [],
      status: "done",
      statusLabel: "Finished",
      summary: "Answered in 2 s",
      durationMs: 2000,
      approvals: [],
      waiting: [],
    },
  ],
  messages: [
    {
      id: "a-1",
      role: "agent",
      authorActor: "tola-grey",
      mine: false,
      authorName: "Tola Grey",
      body: "Ship on Friday.",
      createdAtMs: 2_000,
    },
  ],
  live: [],
};

describe("SessionChildThread", () => {
  it("draws the agent's latest answer under the line and opens the child", () => {
    const onOpen = vi.fn();
    render(<SessionChildThread database="alpha" child={ANSWERED} onOpen={onOpen} />);
    const thread = screen.getByRole("region", { name: "Thread with Tola Grey" });
    expect(within(thread).getByText(PROMPT.body)).toBeInTheDocument();
    const latest = within(thread).getByRole("list", { name: "Latest from Tola Grey" });
    // The fold is Rust's summary line, naming whose work it is.
    expect(within(latest).getByText("Answered in 2 s")).toBeInTheDocument();
    expect(within(latest).getByText("Ship on Friday.")).toBeInTheDocument();
    fireEvent.click(within(thread).getByRole("button", { name: "Open thread with Tola Grey" }));
    expect(onOpen).toHaveBeenCalledWith({ database: "alpha", sessionId: "dev1.7" });
  });

  it("draws the person's own line as their bubble, and another person's line plain (R-HQ-86)", () => {
    const { unmount } = render(
      <SessionChildThread database="alpha" child={ANSWERED} onOpen={vi.fn()} />,
    );
    const own = screen.getByText(PROMPT.body);
    expect(own).toHaveAttribute("data-mine");
    expect(own).toHaveClass("bg-primary");
    unmount();
    const ben = { ...PROMPT, authorActor: "ben", authorName: "Ben", mine: false };
    render(
      <SessionChildThread database="alpha" child={{ ...ANSWERED, prompt: ben }} onOpen={vi.fn()} />,
    );
    expect(screen.getByText(PROMPT.body)).not.toHaveAttribute("data-mine");
    expect(screen.getByText(PROMPT.body)).not.toHaveClass("bg-primary");
  });

  it("shows a refused line with Rust's sentence and nothing to open", () => {
    const sentence =
      "The agents server refused to bring Mason in: Mason is not an agent of alpha. Nothing was sent to Mason.";
    render(
      <SessionChildThread
        database="alpha"
        child={{
          ...ANSWERED,
          agentName: "Mason",
          prompt: { ...PROMPT, body: "@mason check", state: "refused" },
          refusal: sentence,
          items: [],
          folds: [],
          messages: [],
        }}
        onOpen={vi.fn()}
      />,
    );
    const thread = screen.getByRole("region", { name: "Thread with Mason" });
    expect(within(thread).getByRole("alert")).toHaveTextContent(sentence);
    expect(within(thread).getByText("@mason check")).toBeInTheDocument();
    expect(within(thread).queryByRole("button")).toBeNull();
  });
});

describe("SessionChildThread, archived or long", () => {
  it("draws an archived thread as one line that still opens it", () => {
    const onOpen = vi.fn();
    render(
      <SessionChildThread
        database="alpha"
        child={{ ...ANSWERED, archived: true, prompt: undefined, items: [], messages: [] }}
        onOpen={onOpen}
      />,
    );
    const thread = screen.getByRole("region", { name: "Thread with Tola Grey" });
    expect(within(thread).getByText("Thread archived")).toBeInTheDocument();
    expect(within(thread).queryByRole("list")).toBeNull();
    fireEvent.click(within(thread).getByRole("button", { name: "Open thread with Tola Grey" }));
    expect(onOpen).toHaveBeenCalledWith({ database: "alpha", sessionId: "dev1.7" });
  });

  it("says a cut answer continues in the thread", () => {
    render(
      <SessionChildThread
        database="alpha"
        child={{ ...ANSWERED, truncated: true }}
        onOpen={vi.fn()}
      />,
    );
    expect(screen.getByText(CHILD_CONTINUES)).toBeInTheDocument();
  });
});

describe("SessionChildThread, clipped by its height cap", () => {
  it("says the rest is in the thread when the box overflows an answer Rust did not cut", () => {
    const heights = vi
      .spyOn(HTMLElement.prototype, "scrollHeight", "get")
      .mockImplementation(function (this: HTMLElement) {
        return this.tagName === "OL" ? 512 : 0;
      });
    const client = vi
      .spyOn(HTMLElement.prototype, "clientHeight", "get")
      .mockImplementation(function (this: HTMLElement) {
        return this.tagName === "OL" ? 320 : 0;
      });
    try {
      render(<SessionChildThread database="alpha" child={ANSWERED} onOpen={vi.fn()} />);
      expect(screen.getByText(CHILD_CONTINUES)).toBeInTheDocument();
    } finally {
      heights.mockRestore();
      client.mockRestore();
    }
  });

  it("says nothing when the answer fits", () => {
    render(<SessionChildThread database="alpha" child={ANSWERED} onOpen={vi.fn()} />);
    expect(screen.queryByText(CHILD_CONTINUES)).toBeNull();
  });
});

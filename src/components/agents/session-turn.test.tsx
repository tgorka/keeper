import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TurnFold, TurnSteps } from "@/components/agents/session-turn";
import type { AgentMessageVm, AgentToolStepVm, AgentTurnFoldVm } from "@/lib/ipc/client";

const lead: AgentMessageVm = {
  id: "lead",
  role: "agent",
  authorActor: "assistant",
  mine: false,
  authorName: "Assistant",
  body: "Checking — one minute.",
  createdAtMs: 1780000000000,
};

function fold(patch: Partial<AgentTurnFoldVm> = {}): AgentTurnFoldVm {
  return {
    turnId: "t1",
    leadInIds: ["lead"],
    reasoning: null,
    steps: [
      { id: "s1", name: "run", status: "done", statusLabel: "Finished", result: "ok" },
      { id: "s2", name: "web_search", status: "refused", statusLabel: "Refused", result: "No." },
    ],
    failures: [{ stepId: "s2", sentence: "web_search · Refused: No." }],
    status: "done",
    statusLabel: "Finished",
    summary: "Worked for 14 s · ran 1 command, searched the web 1 time",
    durationMs: 14000,
    ...patch,
  };
}

function draw(vm: AgentTurnFoldVm, verbose = false) {
  return render(
    <ol>
      <TurnFold fold={vm} leadIns={[lead]} agentName="Assistant" verbose={verbose} />
    </ol>,
  );
}

describe("TurnFold", () => {
  it("is one line until opened, with each refusal a line of its own and never a card", () => {
    draw(fold());
    const line = screen.getByRole("button", {
      name: "Worked for 14 s · ran 1 command, searched the web 1 time",
    });
    expect(line).toHaveAttribute("aria-expanded", "false");
    // Folded: the lead-in and the calls are not drawn at all.
    expect(screen.queryByText("Checking — one minute.")).not.toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Tool calls" })).not.toBeInTheDocument();
    // The refusal stays visible, as one plain line: no disclosure, no card.
    const refusal = screen.getByText("web_search · Refused: No.");
    expect(refusal).toBeVisible();
    const row = refusal.closest("li");
    expect(row).toHaveAttribute("data-slot", "turn-failure");
    expect(refusal.closest("details")).toBeNull();
    expect(row?.className).not.toMatch(/\bborder\b|\brounded/);

    fireEvent.click(line);
    expect(line).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Checking — one minute.")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Tool calls" })).toBeInTheDocument();
  });

  it("opens by default in the verbose mode, and a person's own toggle wins until the mode changes", () => {
    const { rerender } = draw(fold(), true);
    const line = screen.getByRole("button", { expanded: true });
    fireEvent.click(line);
    expect(line).toHaveAttribute("aria-expanded", "false");
    rerender(
      <ol>
        <TurnFold fold={fold()} leadIns={[lead]} agentName="Assistant" verbose={false} />
      </ol>,
    );
    expect(screen.getByRole("button", { expanded: false })).toBeInTheDocument();
    rerender(
      <ol>
        <TurnFold fold={fold()} leadIns={[lead]} agentName="Assistant" verbose={true} />
      </ol>,
    );
    expect(screen.getByRole("button", { expanded: true })).toBeInTheDocument();
  });

  it("draws the thinking Rust sent, and none when it sent none", () => {
    const { rerender } = draw(fold({ reasoning: "Compare two sources." }));
    expect(screen.getByRole("region", { name: "Thinking" })).toHaveTextContent(
      "Compare two sources.",
    );
    rerender(
      <ol>
        <TurnFold fold={fold()} leadIns={[lead]} agentName="Assistant" verbose={false} />
      </ol>,
    );
    expect(screen.queryByRole("region", { name: "Thinking" })).not.toBeInTheDocument();
  });

  // R10914-01: Chromium names a button holding a live region "" — so the
  // name is given outright, and the status sits beside the button, once.
  // Asserted on the DOM, not through jsdom's name computation, which names
  // that button where Chromium does not.
  it("names a running fold's button outright and announces its line from beside it", () => {
    const summary = "Searching the web… · 3 steps";
    const { container, rerender } = draw(
      fold({ status: "running", statusLabel: "Working", summary, failures: [] }),
    );
    const button = container.querySelector('[data-slot="turn-fold"] > button');
    expect(button).toHaveAttribute("aria-label", summary);
    expect(button?.querySelector('[role="status"]')).toBeNull();
    const statuses = container.querySelectorAll('[role="status"]');
    expect(statuses).toHaveLength(1);
    expect(statuses[0]).toHaveTextContent(summary);
    expect(button?.contains(statuses[0])).toBe(false);

    // Ended: the same stable name, Rust's new sentence, and no live region left.
    rerender(
      <ol>
        <TurnFold
          fold={fold({ summary: "Worked for 14 s · searched the web 3 times" })}
          leadIns={[lead]}
          agentName="Assistant"
          verbose={false}
        />
      </ol>,
    );
    expect(container.querySelector('[data-slot="turn-fold"] > button')).toHaveAttribute(
      "aria-label",
      "Worked for 14 s · searched the web 3 times",
    );
    expect(container.querySelector('[role="status"]')).toBeNull();
  });

  // R10914-08: a running turn with nothing to open yet still announces its line.
  it("announces a running fold's line while there is nothing to open yet", () => {
    const { container } = render(
      <ol>
        <TurnFold
          fold={fold({
            status: "running",
            statusLabel: "Working",
            summary: "Working…",
            leadInIds: [],
            steps: [],
            failures: [],
          })}
          leadIns={[]}
          agentName="Assistant"
          verbose={false}
        />
      </ol>,
    );
    const statuses = container.querySelectorAll('[role="status"]');
    expect(statuses).toHaveLength(1);
    expect(statuses[0]).toHaveTextContent("Working…");
    expect(container.querySelector("button")).toBeNull();
  });

  // R10914-07: the first step updates the region a screen reader already
  // watches; a region replaced at that moment is one it never announces.
  it("keeps one status element across a running turn's first step, updating its text", () => {
    const running = {
      status: "running" as const,
      statusLabel: "Working",
      failures: [],
      leadInIds: [],
    };
    const { container, rerender } = render(
      <ol>
        <TurnFold
          fold={fold({ ...running, summary: "Working…", steps: [] })}
          leadIns={[]}
          agentName="Assistant"
          verbose={false}
        />
      </ol>,
    );
    const before = container.querySelector('[role="status"]');
    expect(before).toHaveTextContent("Working…");
    rerender(
      <ol>
        <TurnFold
          fold={fold({
            ...running,
            summary: "Searching the web… · 1 step",
            steps: [{ id: "s1", name: "web_search", status: "started", statusLabel: "Started" }],
          })}
          leadIns={[]}
          agentName="Assistant"
          verbose={false}
        />
      </ol>,
    );
    const after = container.querySelectorAll('[role="status"]');
    expect(after).toHaveLength(1);
    expect(after[0]).toBe(before);
    expect(after[0]).toHaveTextContent("Searching the web… · 1 step");
    expect(container.querySelector("button")).toHaveAttribute(
      "aria-label",
      "Searching the web… · 1 step",
    );
  });

  it("is a plain line, not a control, when there is nothing to open", () => {
    render(
      <ol>
        <TurnFold
          fold={fold({ leadInIds: [], steps: [], failures: [], summary: "Worked for 14 s" })}
          leadIns={[]}
          agentName="Assistant"
          verbose={true}
        />
      </ol>,
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("Worked for 14 s")).toBeInTheDocument();
  });
});

describe("TurnSteps", () => {
  // Rust names every step status (keeper-core `live.rs`); the renderer maps none.
  it("shows each step's status in Rust's own words, never a local guess", () => {
    const steps: AgentToolStepVm[] = [
      { id: "c1", name: "web_search", status: "interrupted", statusLabel: "Interrupted" },
      { id: "c2", name: "web_search", status: "parked", statusLabel: "Paused" },
      { id: "c3", name: "run", status: "failed", statusLabel: "Failed" },
      { id: "c4", name: "run", status: "unknown", statusLabel: 'Unknown status "frobbed"' },
      { id: "c5", name: "read_file", status: "refused", statusLabel: "Refused" },
    ];
    render(<TurnSteps steps={steps} />);
    expect(screen.getByText("web_search · Interrupted")).toBeInTheDocument();
    expect(screen.getByText("web_search · Paused")).toBeInTheDocument();
    expect(screen.getByText("run · Failed")).toBeInTheDocument();
    expect(screen.getByText('run · Unknown status "frobbed"')).toBeInTheDocument();
    expect(screen.getAllByText(/· Refused$/)).toHaveLength(1);
  });

  // 109.13's review note: the end caption said "Finished" for every ending.
  it("captions how a call ended in Rust's word, never Finished for an interruption", () => {
    render(
      <TurnSteps
        steps={[
          {
            id: "c1",
            name: "web_search",
            status: "interrupted",
            statusLabel: "Interrupted",
            startedAtMs: 1780000000000,
            endedAtMs: 1780000001000,
          },
        ]}
      />,
    );
    const call = screen.getByRole("listitem");
    expect(within(call).getByText(/^Interrupted \d/)).toBeInTheDocument();
    expect(within(call).queryByText(/^Finished/)).not.toBeInTheDocument();
  });
});

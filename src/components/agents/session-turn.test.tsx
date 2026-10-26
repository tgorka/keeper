import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SessionTrace } from "@/components/agents/session-turn";
import type { AgentToolStepVm } from "@/lib/ipc/client";

describe("SessionTrace", () => {
  // Rust names every step status (keeper-core `live.rs`); the renderer maps none.
  it("shows each step's status in Rust's own words, never a local guess", () => {
    const steps: AgentToolStepVm[] = [
      { id: "c1", name: "web_search", status: "interrupted", statusLabel: "Interrupted" },
      { id: "c2", name: "web_search", status: "parked", statusLabel: "Paused" },
      { id: "c3", name: "run", status: "failed", statusLabel: "Failed" },
      { id: "c4", name: "run", status: "unknown", statusLabel: 'Unknown status "frobbed"' },
      { id: "c5", name: "read_file", status: "refused", statusLabel: "Refused" },
    ];
    render(<SessionTrace steps={steps} />);
    expect(screen.getByText("web_search · Interrupted")).toBeInTheDocument();
    expect(screen.getByText("web_search · Paused")).toBeInTheDocument();
    expect(screen.getByText("run · Failed")).toBeInTheDocument();
    expect(screen.getByText('run · Unknown status "frobbed"')).toBeInTheDocument();
    expect(screen.getAllByText(/· Refused$/)).toHaveLength(1);
  });
});

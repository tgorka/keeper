import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentRoomHeader, GATE_ANYONE } from "@/components/chat/agent-room-header";
import type { AgentRoomHeaderVm, AgentStatusVm } from "@/lib/ipc/client";

function status(overrides: Partial<AgentStatusVm> = {}): AgentStatusVm {
  return {
    agent: "@nixi:example.org",
    agentName: "Nixi",
    handle: "nixi@electra",
    host: "electra",
    title: "Nixi",
    kind: "main",
    run: "running",
    waiting: null,
    detail: "2 notes read",
    unreadable: null,
    ...overrides,
  };
}

function header(overrides: Partial<AgentRoomHeaderVm> = {}): AgentRoomHeaderVm {
  return {
    status: status(),
    scope: [
      { id: "tgdrive", title: "tgdrive" },
      { id: "neura", title: "Neura" },
    ],
    label: {
      readers: ["harness", "Marta"],
      anyone: false,
      integrity: "owner",
      localOnly: false,
      sentence: "What you read here may be shown only to: harness, Marta.",
    },
    scopeUnreadable: null,
    caretKey: null,
    gate: null,
    ...overrides,
  };
}

function region() {
  return screen.getByRole("region", { name: "Agent status" });
}

describe("AgentRoomHeader", () => {
  it("draws the handle, the run and the detail as sent", () => {
    render(<AgentRoomHeader header={header()} />);
    expect(within(region()).getByText("nixi@electra")).toBeInTheDocument();
    expect(screen.getByTestId("agent-run")).toHaveTextContent(/^run: running$/);
    expect(within(region()).getByText("2 notes read")).toBeInTheDocument();
  });

  it("names the host a waiting session waits for, before its need", () => {
    render(
      <AgentRoomHeader
        header={header({
          status: status({ run: "waiting", waiting: "hesperia", detail: "a reader to sign in" }),
        })}
      />,
    );
    expect(screen.getByTestId("agent-run")).toHaveTextContent(/^run: waiting$/);
    expect(
      within(region()).getByText("waiting: hesperia — a reader to sign in"),
    ).toBeInTheDocument();
  });

  it("shows an unreadable status as unreadable, with Rust's sentence", () => {
    const sentence = "This status is from a newer keeper. Update keeper to read it.";
    render(
      <AgentRoomHeader
        header={header({
          status: status({ run: "unreadable", detail: null, unreadable: sentence }),
        })}
      />,
    );
    expect(screen.getByTestId("agent-run")).toHaveTextContent(/^run: unreadable$/);
    expect(within(region()).getByText(sentence)).toBeInTheDocument();
  });

  it("lists the drives in scope in the scope's own order", () => {
    render(
      <AgentRoomHeader
        header={header({
          scope: [
            { id: "neura", title: "Neura" },
            { id: "tgdrive", title: "tgdrive" },
          ],
        })}
      />,
    );
    const drives = within(screen.getByRole("list", { name: "Drives in scope" })).getAllByRole(
      "listitem",
    );
    expect(drives.map((d) => d.textContent)).toEqual(["Neura", "tgdrive"]);
    expect(screen.queryByText("no scope yet")).not.toBeInTheDocument();
  });

  it("says there is no scope yet before one was read, and draws no label chip without a label", () => {
    render(<AgentRoomHeader header={header({ scope: null, label: null })} />);
    expect(within(region()).getByText("no scope yet")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Drives in scope" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("agent-label")).not.toBeInTheDocument();
  });

  it("tells an empty scope from no scope", () => {
    render(<AgentRoomHeader header={header({ scope: [] })} />);
    expect(within(region()).getByText("no drives in scope")).toBeInTheDocument();
    expect(screen.queryByText("no scope yet")).not.toBeInTheDocument();
  });

  it("says why a scope cannot be read, never 'no scope yet'", () => {
    const sentence = "This scope is from a newer keeper. Update keeper to read it.";
    render(
      <AgentRoomHeader header={header({ scope: null, label: null, scopeUnreadable: sentence })} />,
    );
    expect(within(region()).getByText(sentence)).toBeInTheDocument();
    expect(screen.queryByText("no scope yet")).not.toBeInTheDocument();
  });

  it("draws the label chip as readers by name and integrity as a word, with the sentence", () => {
    render(<AgentRoomHeader header={header()} />);
    expect(screen.getByTestId("agent-label")).toHaveTextContent(
      "harness, Marta · owner. What you read here may be shown only to: harness, Marta.",
    );
  });

  it("marks the agent with one glyph from its handle in the identity cell", () => {
    const { container } = render(
      <AgentRoomHeader header={header({ status: status({ handle: "tola@electra" }) })} />,
    );
    const cell = container.querySelector('[data-slot="bot-identity"]');
    expect(cell).toHaveAttribute("data-shape", "hollow");
    expect(cell).toHaveTextContent(/^T$/);
  });

  it("prefers the soul's icon to the handle's first letter where Rust sends one", () => {
    const { container } = render(
      <AgentRoomHeader
        header={header({ status: status({ handle: "tola@electra", icon: "🜂" }) })}
      />,
    );
    const cell = container.querySelector('[data-slot="bot-identity"]');
    expect(cell).toHaveTextContent(/^🜂$/);
  });

  it("names the agent's model and level where its home is on this device, and nothing elsewhere", () => {
    const { rerender } = render(
      <AgentRoomHeader
        header={header({ status: status({ model: { bot: "Opus", effort: "xhigh" } }) })}
      />,
    );
    expect(screen.getByTestId("agent-model")).toHaveTextContent(/^model: Opus · xhigh$/);
    rerender(
      <AgentRoomHeader
        header={header({ status: status({ model: { bot: "GPT Astra", effort: null } }) })}
      />,
    );
    expect(screen.getByTestId("agent-model")).toHaveTextContent(/^model: GPT Astra$/);
    // The phone: no home, no line.
    rerender(<AgentRoomHeader header={header()} />);
    expect(screen.queryByTestId("agent-model")).not.toBeInTheDocument();
  });

  it("says no status has arrived when the agent has sent none", () => {
    render(<AgentRoomHeader header={header({ status: null })} />);
    expect(within(region()).getByText("No status from the agent yet.")).toBeInTheDocument();
    expect(screen.queryByTestId("agent-run")).not.toBeInTheDocument();
  });
});

describe("AgentRoomHeader — a gate room (UX-DR147)", () => {
  const gateHeader = (peer: boolean) =>
    header({
      status: status({
        agent: "@forge:example.org",
        handle: "forge@electra",
        title: "github",
        kind: "gate",
        run: "idle",
        detail: null,
      }),
      scope: [],
      label: {
        readers: [],
        anyone: true,
        integrity: "untrusted",
        localOnly: false,
        sentence: "What you read here may be shown to anyone.",
      },
      gate: {
        system: "github",
        peer: peer ? "@github:example.org" : null,
        peerName: peer ? '"GitHub"' : null,
      },
    });

  it("names the system and its account, and reads anyone as whoever the system shares with", () => {
    render(<AgentRoomHeader header={gateHeader(true)} />);
    const gate = within(region()).getByTestId("agent-gate");
    for (const shown of ["github", '"GitHub"', "@github:example.org"]) {
      expect(gate).toHaveTextContent(shown);
    }
    expect(within(region()).getByTestId("agent-label")).toHaveTextContent(
      `${GATE_ANYONE} · untrusted`,
    );
  });

  it("names the system alone before the gate has named its account", () => {
    render(<AgentRoomHeader header={gateHeader(false)} />);
    const gate = within(region()).getByTestId("agent-gate");
    expect(gate).toHaveTextContent("github");
    expect(gate).not.toHaveTextContent("@github:example.org");
    expect(gate).not.toHaveTextContent('"GitHub"');
  });

  it("keeps anyone as anyone outside a gate room", () => {
    render(
      <AgentRoomHeader
        header={header({ label: { ...gateHeader(true).label!, integrity: "owner" }, gate: null })}
      />,
    );
    expect(within(region()).getByTestId("agent-label")).toHaveTextContent("anyone · owner");
    expect(within(region()).queryByTestId("agent-gate")).toBeNull();
  });
});

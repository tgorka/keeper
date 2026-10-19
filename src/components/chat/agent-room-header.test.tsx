import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AgentCopyVm,
  AgentRoomHeaderVm,
  AgentStatusVm,
  AgentsLaunchVm,
  InboxRoomVm,
} from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsDefaultGet: vi.fn(() => new Promise(() => {})),
  agentsDefaultSet: vi.fn(),
  agentsCopies: vi.fn(() => new Promise(() => {})),
  agentsWebSearchSet: vi.fn(),
  agentsConsoleSet: vi.fn(),
  frontendErrorReport: vi.fn(() => Promise.resolve()),
}));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { toast } from "sonner";
import {
  AgentRoomHeader,
  CLEAR_DEFAULT_LABEL,
  GATE_ANYONE,
  MAKE_DEFAULT_LABEL,
} from "@/components/chat/agent-room-header";
import {
  agentsConsoleSet,
  agentsCopies,
  agentsDefaultGet,
  agentsDefaultSet,
  agentsWebSearchSet,
} from "@/lib/ipc/client";
import { agentRoomsStore } from "@/lib/stores/agent-rooms";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { defaultAgentStore, readFailedSentence } from "@/lib/stores/default-agent";

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

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
};

/** An answer the test settles when it chooses. The executor form, not
 *  `Promise.withResolvers`: the project compiles against `lib: ES2020`. */
function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<T>((settle, refuse) => {
    resolve = settle;
    reject = refuse;
  });
  return { promise, resolve, reject };
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

  // R457: the marker while the agent searches the web without asking, with
  // Rust's sentence for anyone reading it; none at any other level.
  it("marks an agent that searches without asking with Rust's sentence, and no other", () => {
    const sentence = "Searches the web without asking: its queries go out unreviewed.";
    const { rerender } = render(
      <AgentRoomHeader header={header({ status: status({ searchesUnasked: sentence }) })} />,
    );
    const mark = screen.getByTestId("agent-searches-unasked");
    expect(mark).toHaveTextContent(sentence);
    expect(mark).toHaveAttribute("title", sentence);
    rerender(<AgentRoomHeader header={header({ status: status({}) })} />);
    expect(screen.queryByTestId("agent-searches-unasked")).not.toBeInTheDocument();
  });

  // Story 101.5 (R420): an agent whose console runs at full access wears a
  // standing marker carrying Rust's sentence; any other console wears none.
  it("marks an agent at full access with Rust's sentence, and no other", () => {
    const sentence = "Full access: Nixi's commands run on its host without a sandbox.";
    const { rerender } = render(
      <AgentRoomHeader header={header({ status: status({ fullAccess: sentence }) })} />,
    );
    const marker = screen.getByTestId("agent-full-access");
    expect(marker).toHaveAttribute("title", sentence);
    expect(marker).toHaveTextContent(sentence);
    rerender(<AgentRoomHeader header={header()} />);
    expect(screen.queryByTestId("agent-full-access")).not.toBeInTheDocument();
  });

  // Story 100.4 #2/#4: the agent's picture in the identity cell, an `<img>`
  // (never inlined markup), with the letter back when it does not load.
  it("draws the agent's picture in the cell and the letter when it does not load", () => {
    const avatar = "keeper-file://p1/80-agents/tola/avatar.svg?v=0123456789ab";
    const { container } = render(
      <AgentRoomHeader header={header({ status: status({ handle: "tola@electra", avatar }) })} />,
    );
    const cell = container.querySelector('[data-slot="bot-identity"]');
    const picture = cell?.querySelector("img");
    expect(picture).toHaveAttribute("src", avatar);
    expect(cell?.querySelector("svg image, foreignObject")).toBeNull();
    expect(cell).not.toHaveTextContent("T");
    fireEvent.error(picture as HTMLImageElement);
    expect(cell?.querySelector("img")).toBeNull();
    expect(cell).toHaveTextContent(/^T$/);
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

describe("AgentRoomHeader's default-agent menu", () => {
  const NIXI = "@nixi:example.org";
  function launch(overrides: Partial<AgentsLaunchVm> = {}): AgentsLaunchVm {
    return {
      view: "agents",
      room: null,
      defaultAgent: null,
      reason: null,
      announce: false,
      choices: [{ agent: NIXI, name: "Nixi" }],
      chooser: false,
      fileSet: null,
      ask: null,
      askable: [],
      ...overrides,
    };
  }
  function agentRoom(roomId: string): InboxRoomVm {
    return {
      accountId: "acctA",
      hueIndex: 0,
      roomId,
      displayName: roomId,
      lastMessage: null,
      timestamp: null,
      avatarUrl: null,
      isUnread: false,
      mentionCount: 0,
      isArchived: false,
      isPinned: false,
      isFavourite: false,
      network: null,
      networkId: null,
      muteState: "none",
    };
  }
  async function openMenu() {
    const trigger = screen.getByRole("button", { name: "Agent menu" });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    return await screen.findByRole("menu");
  }

  beforeEach(() => {
    vi.mocked(agentsDefaultSet).mockReset();
    // A read that never answers unless a test says otherwise.
    vi.mocked(agentsDefaultGet).mockReset();
    vi.mocked(agentsDefaultGet).mockReturnValue(new Promise(() => {}));
    vi.mocked(toast).mockClear();
    vi.mocked(toast.error).mockClear();
    agentRoomsStore.getState().clear();
    defaultAgentStore.setState({
      vm: null,
      readError: null,
      saveError: null,
      saving: false,
      navRestore: "pending",
    });
  });

  it("offers the action once Rust's choices gain the agent, when the Agents window's rooms change", async () => {
    // The launch's answer came before Nixi's conversation was readable.
    defaultAgentStore.setState({ vm: launch({ choices: [] }) });
    vi.mocked(agentsDefaultGet)
      .mockResolvedValueOnce(launch({ choices: [] }))
      .mockResolvedValueOnce(launch());
    render(<AgentRoomHeader header={header()} />);
    await waitFor(() => expect(agentsDefaultGet).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("button", { name: "Agent menu" })).not.toBeInTheDocument();
    act(() => {
      agentRoomsStore
        .getState()
        .applyBatch({ ops: [{ op: "reset", rooms: [agentRoom("!nixi-dm:x")] }], total: 1 });
    });
    expect(await screen.findByRole("button", { name: "Agent menu" })).toBeInTheDocument();
    expect(agentsDefaultGet).toHaveBeenCalledTimes(2);
  });

  it("says a file decides the default and never announces an agent that will not open", async () => {
    const fileSet =
      "/etc/keeper.toml (user) sets agents.default to @tala:example.org, so @tala:example.org opens at launch whatever you choose here. Your choice is kept and takes effect once that file stops setting it.";
    const shadowed = launch({ defaultAgent: "@tala:example.org", fileSet });
    defaultAgentStore.setState({ vm: shadowed });
    vi.mocked(agentsDefaultSet).mockResolvedValue(shadowed);
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    expect(within(menu).getByText(fileSet)).toBeInTheDocument();
    fireEvent.click(within(menu).getByRole("menuitem", { name: MAKE_DEFAULT_LABEL }));
    await waitFor(() => expect(toast).toHaveBeenCalledWith(fileSet));
    expect(toast).not.toHaveBeenCalledWith("Nixi opens when keeper starts.");
  });

  it("says where the menu would be that the default agent could not be read", async () => {
    vi.mocked(agentsDefaultGet).mockReset();
    vi.mocked(agentsDefaultGet).mockRejectedValue(new Error("no data directory here"));
    render(<AgentRoomHeader header={header()} />);
    expect(await within(region()).findByRole("alert")).toHaveTextContent(
      readFailedSentence(new Error("no data directory here")),
    );
    expect(screen.queryByRole("button", { name: "Agent menu" })).not.toBeInTheDocument();
  });

  it("makes an agent Rust offers the default", async () => {
    defaultAgentStore.setState({ vm: launch() });
    vi.mocked(agentsDefaultSet).mockResolvedValue(launch({ defaultAgent: NIXI }));
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: MAKE_DEFAULT_LABEL }));
    await waitFor(() => expect(agentsDefaultSet).toHaveBeenCalledWith(NIXI));
    await waitFor(() => expect(defaultAgentStore.getState().vm?.defaultAgent).toBe(NIXI));
  });

  it("stops the default agent being one", async () => {
    defaultAgentStore.setState({ vm: launch({ defaultAgent: NIXI }) });
    vi.mocked(agentsDefaultSet).mockResolvedValue(launch());
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: CLEAR_DEFAULT_LABEL }));
    await waitFor(() => expect(agentsDefaultSet).toHaveBeenCalledWith(null));
  });

  it("is not offered for an agent Rust does not list as a choice", () => {
    defaultAgentStore.setState({ vm: launch({ choices: [] }) });
    render(<AgentRoomHeader header={header()} />);
    expect(screen.queryByRole("button", { name: "Agent menu" })).not.toBeInTheDocument();
  });

  it("says why a save failed", async () => {
    defaultAgentStore.setState({ vm: launch() });
    vi.mocked(agentsDefaultSet).mockRejectedValue({
      message: "keeper could not save the default agent: database is locked",
    });
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: MAKE_DEFAULT_LABEL }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "keeper could not save the default agent: database is locked",
      ),
    );
  });
});

// Story 102.3 (R469): where this device writes the agent's agent.toml, the
// header's menu switches its web search and its console through the commands
// Settings › Agents saves with, against what the menu showed.
describe("AgentRoomHeader's agent menu: web search and console", () => {
  const NIXI = "@nixi:example.org";
  function row(overrides: Partial<AgentCopyVm> = {}): AgentCopyVm {
    return {
      profileId: "p1",
      drive: "tgdrive",
      agent: "nixi",
      name: "Nixi",
      matrixUser: NIXI,
      device: "MAC",
      host: "hesperia",
      signedIn: true,
      pin: null,
      problem: null,
      model: null,
      webSearch: {
        level: "ask",
        locked: null,
        unavailable: null,
        warning: null,
        host: "Nixi asks for web search, but electra names no search engine, so it searches none there.",
      },
      console: {
        mode: "sandboxed",
        paths: ["tgdrive/projects"],
        modes: [
          { mode: "off", label: "Off", description: "No console.", refused: null },
          { mode: "supervised", label: "Supervised", description: "Asks.", refused: null },
          { mode: "sandboxed", label: "Sandboxed", description: "Sandbox.", refused: null },
          {
            mode: "full",
            label: "Full access",
            description: "No sandbox.",
            refused: "Nixi cannot have this mode: a gate names it in its delegates.",
          },
        ],
        drives: ["tgdrive"],
        warning: "Full access: Nixi's commands run without a sandbox.",
        unrestricted: "Unrestricted.",
        refused: [],
        locked: null,
      },
      avatarProblem: null,
      ...overrides,
    };
  }
  async function openMenu() {
    const trigger = await screen.findByRole("button", { name: "Agent menu" });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    return await screen.findByRole("menu");
  }

  beforeEach(() => {
    capabilitiesStore.setState({
      capabilities: { ...DEFAULT_CAPABILITIES, botTools: true },
      hydrated: true,
    });
    vi.mocked(agentsDefaultGet).mockReset();
    vi.mocked(agentsDefaultGet).mockReturnValue(new Promise(() => {}));
    defaultAgentStore.setState({
      vm: null,
      readError: null,
      saveError: null,
      saving: false,
      navRestore: "pending",
    });
    vi.mocked(agentsCopies).mockReset();
    vi.mocked(agentsWebSearchSet).mockReset();
    vi.mocked(agentsConsoleSet).mockReset();
    vi.mocked(toast).mockClear();
    vi.mocked(toast.error).mockClear();
    return () => capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES });
  });

  it("switches web search against the level it showed and shows what Rust reads after the save", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([row()]);
    const saved = row({
      webSearch: {
        level: "withoutAsking",
        locked: null,
        unavailable: null,
        warning: "Queries go out.",
        host: null,
      },
    });
    vi.mocked(agentsWebSearchSet).mockResolvedValue([saved]);
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    const group = within(menu).getByTestId("agent-menu-web-search");
    expect(within(group).getByRole("menuitemradio", { name: "Ask" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    // What the serving host makes of the level is said in the menu.
    expect(within(group).getByTestId("agent-menu-web-search-host")).toHaveTextContent(
      "electra names no search engine",
    );
    fireEvent.click(within(group).getByRole("menuitemradio", { name: "Without asking" }));
    await waitFor(() =>
      expect(agentsWebSearchSet).toHaveBeenCalledWith({
        profileId: "p1",
        agent: "nixi",
        level: "withoutAsking",
        shownLevel: "ask",
      }),
    );
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(
        "Nixi's web search: Without asking. Saved; it takes effect once the host serving Nixi has it.",
      ),
    );
    // The rows the save handed back are what the menu shows, even before
    // its next read answers.
    vi.mocked(agentsCopies).mockReturnValue(new Promise(() => {}));
    const again = await openMenu();
    expect(within(again).getByRole("menuitemradio", { name: "Without asking" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("switches the console's mode keeping its folders, and offers no mode Rust refuses", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([row()]);
    vi.mocked(agentsConsoleSet).mockResolvedValue([row()]);
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    const group = within(menu).getByTestId("agent-menu-console");
    expect(within(group).getByRole("menuitemradio", { name: "Full access" })).toHaveAttribute(
      "data-disabled",
    );
    fireEvent.click(within(group).getByRole("menuitemradio", { name: "Supervised" }));
    await waitFor(() =>
      expect(agentsConsoleSet).toHaveBeenCalledWith({
        profileId: "p1",
        agent: "nixi",
        mode: "supervised",
        paths: ["tgdrive/projects"],
        shownMode: "sandboxed",
        shownPaths: ["tgdrive/projects"],
      }),
    );
  });

  // Review R102T1-05: Full access is said before it is saved.
  it("says what full access gives before saving it from a narrower mode", async () => {
    const open = row();
    const modes = open.console?.modes.map((one) => ({ ...one, refused: null })) ?? [];
    const sandboxed = row({ console: open.console && { ...open.console, modes } });
    vi.mocked(agentsCopies).mockResolvedValue([sandboxed]);
    vi.mocked(agentsConsoleSet).mockResolvedValue([sandboxed]);
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    const group = within(menu).getByTestId("agent-menu-console");
    fireEvent.click(within(group).getByRole("menuitemradio", { name: "Full access" }));
    const said = await within(menu).findByTestId("agent-menu-full-access");
    expect(said).toHaveTextContent("Full access: Nixi's commands run without a sandbox.");
    expect(said).toHaveTextContent("Unrestricted.");
    expect(said).toHaveTextContent("No sandbox.");
    expect(agentsConsoleSet).not.toHaveBeenCalled();
    fireEvent.click(within(said).getByRole("menuitem", { name: "Give full access" }));
    await waitFor(() =>
      expect(agentsConsoleSet).toHaveBeenCalledWith({
        profileId: "p1",
        agent: "nixi",
        mode: "full",
        paths: ["tgdrive/projects"],
        shownMode: "sandboxed",
        shownPaths: ["tgdrive/projects"],
      }),
    );
  });

  // Review R102T1-04: a read issued before a save never replaces what the
  // save handed back, whether it answers or fails after it.
  it.each([
    ["answers", (late: Deferred<AgentCopyVm[]>) => late.resolve([row()])],
    ["fails", (late: Deferred<AgentCopyVm[]>) => late.reject({ message: "read failed" })],
  ])("keeps the saved level when a read issued before the save %s after it", async (_, end) => {
    vi.mocked(agentsCopies).mockResolvedValueOnce([row()]);
    const late = deferred<AgentCopyVm[]>();
    vi.mocked(agentsCopies).mockReturnValueOnce(late.promise);
    const saved = row({
      webSearch: {
        level: "withoutAsking",
        locked: null,
        unavailable: null,
        warning: "Queries go out.",
        host: null,
      },
    });
    vi.mocked(agentsWebSearchSet).mockResolvedValue([saved]);
    render(<AgentRoomHeader header={header()} />);
    // Opening reads again; that read is still out when the save answers.
    const menu = await openMenu();
    await waitFor(() => expect(agentsCopies).toHaveBeenCalledTimes(2));
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Without asking" }));
    await waitFor(() => expect(toast).toHaveBeenCalled());
    await act(async () => {
      end(late);
      await late.promise.catch(() => {});
    });
    vi.mocked(agentsCopies).mockReturnValue(new Promise(() => {}));
    const again = await openMenu();
    expect(within(again).getByRole("menuitemradio", { name: "Without asking" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(within(again).queryByRole("alert")).not.toBeInTheDocument();
  });

  // Review R102T1-04: Rust may still read the old level until the save
  // commits, so a read asked while the save is out is issued after it.
  it("issues a read asked during a save only once the save has answered", async () => {
    const saved = row({
      webSearch: {
        level: "withoutAsking",
        locked: null,
        unavailable: null,
        warning: "Queries go out.",
        host: null,
      },
    });
    let committed = false;
    vi.mocked(agentsCopies).mockImplementation(async () => [committed ? saved : row()]);
    const save = deferred<AgentCopyVm[]>();
    vi.mocked(agentsWebSearchSet).mockReturnValue(save.promise);
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Without asking" }));
    await waitFor(() => expect(agentsWebSearchSet).toHaveBeenCalled());
    const reads = vi.mocked(agentsCopies).mock.calls.length;
    await openMenu();
    expect(agentsCopies).toHaveBeenCalledTimes(reads);
    await act(async () => {
      committed = true;
      save.resolve([saved]);
      await save.promise;
    });
    await waitFor(() => expect(vi.mocked(agentsCopies).mock.calls.length).toBeGreaterThan(reads));
    const shown = await screen.findByRole("menu");
    await waitFor(() =>
      expect(within(shown).getByRole("menuitemradio", { name: "Without asking" })).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
  });

  it("says Rust's refusal of a switch and reads the row again", async () => {
    const sentence =
      "electra runs keeper 0.9.0, which does not read [web_search] (keeper 0.10.4 does): saving it would stop electra serving Nixi. Update keeper on electra first; nothing was saved.";
    vi.mocked(agentsCopies).mockResolvedValue([row()]);
    vi.mocked(agentsWebSearchSet).mockRejectedValue({ message: sentence });
    render(<AgentRoomHeader header={header()} />);
    const menu = await openMenu();
    const reads = vi.mocked(agentsCopies).mock.calls.length;
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Without asking" }));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith(sentence));
    await waitFor(() => expect(vi.mocked(agentsCopies).mock.calls.length).toBeGreaterThan(reads));
    expect(toast).not.toHaveBeenCalled();
  });

  it("offers no switch for another agent's row, nor where this device does not write agent.toml", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([row({ matrixUser: "@tola:example.org" })]);
    const { unmount } = render(<AgentRoomHeader header={header()} />);
    await waitFor(() => expect(agentsCopies).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Agent menu" })).not.toBeInTheDocument();
    unmount();
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES });
    vi.mocked(agentsCopies).mockClear();
    vi.mocked(agentsCopies).mockResolvedValue([row()]);
    render(<AgentRoomHeader header={header()} />);
    expect(screen.queryByRole("button", { name: "Agent menu" })).not.toBeInTheDocument();
    expect(agentsCopies).not.toHaveBeenCalled();
  });
});

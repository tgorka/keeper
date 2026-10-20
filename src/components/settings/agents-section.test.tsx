import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CLAIMS_LEASE_LABEL } from "@/components/settings/agents-claims-lease-row";
import {
  AGENTS_RELOAD_MS,
  AgentsSection,
  CONSOLE_ADD_FOLDER_LABEL,
  CONSOLE_SAVE_LABEL,
  MODEL_SAVE_LABEL,
  PIN_LABEL,
  PROVIDER_DEFAULT_LABEL,
  REVIEW_READERS_LABEL,
  SEED_NO_BOTS,
  SEED_PREVIEW_LABEL,
  SEED_WRITE_LABEL,
  SET_UP_AGENTS_LABEL,
  SIGN_IN_LABEL,
  WEB_SEARCH_SWITCH_NOTE,
} from "@/components/settings/agents-section";
import type {
  AgentConsoleVm,
  AgentCopyVm,
  AgentModelVm,
  AgentPersonVm,
  AgentPinVm,
  AgentSeedFolderVm,
  AgentSeedOfferVm,
  AgentWebSearchVm,
  Effort,
} from "@/lib/ipc/client";
import {
  agentsConsoleSet,
  agentsCopies,
  agentsCopySignIn,
  agentsDriveRepin,
  agentsMcpList,
  agentsMcpRemove,
  agentsModelSet,
  agentsSeedApply,
  agentsSeedOffer,
  agentsSeedPlan,
  agentsWebSearchSet,
} from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsCopies: vi.fn(),
  agentsCopySignIn: vi.fn(),
  agentsDriveRepin: vi.fn(),
  agentsSeedOffer: vi.fn(),
  agentsSeedPlan: vi.fn(),
  agentsSeedApply: vi.fn(),
  agentsMcpList: vi.fn(),
  agentsMcpRemove: vi.fn(),
  agentsModelSet: vi.fn(),
  agentsConsoleSet: vi.fn(),
  agentsMcpDraft: vi.fn(() =>
    Promise.resolve({ floor: null, fixed: null, tools: [], conflicts: [], drop: null }),
  ),
  agentsClaimsLeaseGet: vi.fn(() => Promise.resolve("short")),
  agentsClaimsLeaseSet: vi.fn(),
  agentsSandboxGet: vi.fn(() =>
    Promise.resolve({ readExec: [], env: [], status: "", refusal: null }),
  ),
  agentsWebSearchGet: vi.fn(() => Promise.resolve({ url: null, refusal: null })),
  agentsWebSearchSave: vi.fn(),
  agentsWebSearchSet: vi.fn(),
}));

const OWNER: AgentPersonVm = { matrixId: "@tgorka:tgorka.org", displayName: "Tomasz Gorka" };
const MARTA: AgentPersonVm = { matrixId: "@marta:tgorka.org", displayName: null };
const EVE: AgentPersonVm = { matrixId: "@eve:tgorka.org", displayName: null };

const unpinned: AgentPinVm = {
  state: "unpinned",
  owner: OWNER,
  readers: [OWNER, MARTA],
  localOnly: true,
  pinnedOwner: null,
  pinnedReaders: [],
  pinnedLocalOnly: null,
  differences: [],
};

const differs: AgentPinVm = {
  state: "differs",
  owner: OWNER,
  readers: [OWNER, MARTA, EVE],
  localOnly: false,
  pinnedOwner: OWNER,
  pinnedReaders: [OWNER, MARTA],
  pinnedLocalOnly: false,
  differences: [
    "_drive.toml names the readers @eve:tgorka.org, @marta:tgorka.org, @tgorka:tgorka.org; this host pinned @marta:tgorka.org, @tgorka:tgorka.org",
  ],
};

/** The same owner and readers, but `_drive.toml` turned local-only off. */
const lowered: AgentPinVm = {
  state: "differs",
  owner: OWNER,
  readers: [OWNER, MARTA],
  localOnly: false,
  pinnedOwner: OWNER,
  pinnedReaders: [OWNER, MARTA],
  pinnedLocalOnly: true,
  differences: ["_drive.toml says local_only = false; this host pinned local_only = true"],
};

function nixi(pin: AgentPinVm, signedIn = false): AgentCopyVm {
  return {
    profileId: "p1",
    drive: "tgdrive",
    agent: "nixi",
    name: "Nixi",
    matrixUser: "@nixi:tgorka.org",
    device: signedIn ? "DEVICE" : null,
    host: "hesperia",
    signedIn,
    pin,
    problem: null,
    model: null,
    webSearch: null,
    console: null,
    avatarProblem: null,
  };
}

const NO_FOLDERS: AgentSeedOfferVm = {
  folders: [],
  catalogue: [],
  bots: [],
  localBots: [],
  botsUnread: null,
  accounts: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(agentsSeedOffer).mockResolvedValue(NO_FOLDERS);
  vi.mocked(agentsMcpList).mockResolvedValue({ servers: [], tiers: [] });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Settings › Agents", () => {
  it("is absent where no flagged folder holds an agent", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    const { container } = render(<AgentsSection open />);
    await waitFor(() => expect(agentsCopies).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("stays reachable with no flagged folder while this Mac keeps an MCP server, which can be removed", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    const kept = {
      name: "notes",
      url: "https://notes.example.org/mcp",
      command: [],
      role: null,
      fingerprint: null,
      readers: [],
      anyone: true,
      trustAnnotations: false,
      token: true,
      rows: [],
      floor: null,
      fixed: null,
      answers: false,
      answer: "does not answer",
      started: null,
      refusal: null,
    };
    vi.mocked(agentsMcpList).mockResolvedValue({ servers: [kept], tiers: [] });
    vi.mocked(agentsMcpRemove).mockResolvedValue({ servers: [], tiers: [] });
    render(<AgentsSection open />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove notes" }));
    await waitFor(() => expect(agentsMcpRemove).toHaveBeenCalledWith("notes"));
    await waitFor(() => expect(screen.queryByRole("listitem", { name: "notes" })).toBeNull());
  });

  it("shows the owner, readers and local-only setting the first sign-in pins, pins exactly those, and then says who is signed in", async () => {
    vi.mocked(agentsCopies)
      .mockResolvedValueOnce([nixi(unpinned)])
      .mockResolvedValue([nixi({ ...unpinned, state: "pinned" }, true)]);
    vi.mocked(agentsCopySignIn).mockResolvedValue(nixi({ ...unpinned, state: "pinned" }, true));
    render(<AgentsSection open />);

    expect(await screen.findByText("Owner: Tomasz Gorka (@tgorka:tgorka.org)")).toBeInTheDocument();
    expect(screen.getByText("Reader: @marta:tgorka.org")).toBeInTheDocument();
    expect(screen.getByText("Local models only: yes")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Password for @nixi:tgorka.org"), {
      target: { value: "secret" },
    });
    fireEvent.click(screen.getByRole("button", { name: SIGN_IN_LABEL }));

    expect(await screen.findByText("Signed in as nixi@hesperia")).toBeInTheDocument();
    expect(agentsCopySignIn).toHaveBeenCalledWith("p1", "nixi", "secret", {
      owner: "@tgorka:tgorka.org",
      readers: ["@tgorka:tgorka.org", "@marta:tgorka.org"],
      localOnly: true,
    });
  });

  it("shows Rust's sentence for a wrong password and stays signed out", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([nixi({ ...unpinned, state: "pinned" })]);
    vi.mocked(agentsCopySignIn).mockRejectedValue({
      code: "internal",
      message: "The homeserver did not accept that password for @nixi:tgorka.org.",
      accountId: null,
      retriable: false,
    });
    render(<AgentsSection open />);
    fireEvent.change(await screen.findByLabelText("Password for @nixi:tgorka.org"), {
      target: { value: "wrong" },
    });
    fireEvent.click(screen.getByRole("button", { name: SIGN_IN_LABEL }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The homeserver did not accept that password for @nixi:tgorka.org.",
    );
    expect(screen.queryByText(/Signed in as/)).not.toBeInTheDocument();
    // A drive already pinned sends no pin: only the first sign-in pins.
    expect(agentsCopySignIn).toHaveBeenCalledWith("p1", "nixi", "wrong", null);
  });

  it("names a changed drive's difference and re-pins only on the tap, with what it showed", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([nixi(differs)]);
    vi.mocked(agentsDriveRepin).mockResolvedValue([
      nixi({ ...differs, state: "pinned", differences: [] }),
    ]);
    render(<AgentsSection open />);

    expect(await screen.findByText(differs.differences[0] as string)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: REVIEW_READERS_LABEL }));
    expect(screen.getByText("Pinned on this Mac")).toBeInTheDocument();
    expect(screen.getByText("Now in _drive.toml")).toBeInTheDocument();
    expect(screen.getByText("Reader: @eve:tgorka.org")).toBeInTheDocument();
    expect(agentsDriveRepin).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: PIN_LABEL }));
    await waitFor(() =>
      expect(agentsDriveRepin).toHaveBeenCalledWith("p1", {
        owner: "@tgorka:tgorka.org",
        readers: ["@tgorka:tgorka.org", "@marta:tgorka.org", "@eve:tgorka.org"],
        localOnly: false,
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: REVIEW_READERS_LABEL })).not.toBeInTheDocument(),
    );
  });

  it("shows a lowered local-only setting in both columns before a re-pin may lower it", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([nixi(lowered)]);
    vi.mocked(agentsDriveRepin).mockResolvedValue([
      nixi({ ...lowered, state: "pinned", pinnedLocalOnly: false, differences: [] }),
    ]);
    render(<AgentsSection open />);

    fireEvent.click(await screen.findByRole("button", { name: REVIEW_READERS_LABEL }));
    const pinned = screen.getByText("Pinned on this Mac").parentElement as HTMLElement;
    const now = screen.getByText("Now in _drive.toml").parentElement as HTMLElement;
    expect(within(pinned).getByText("Local models only: yes")).toBeInTheDocument();
    expect(within(now).getByText("Local models only: no")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: PIN_LABEL }));
    await waitFor(() =>
      expect(agentsDriveRepin).toHaveBeenCalledWith("p1", {
        owner: "@tgorka:tgorka.org",
        readers: ["@tgorka:tgorka.org", "@marta:tgorka.org"],
        localOnly: false,
      }),
    );
  });

  it("shows why a flagged folder whose _drive.toml does not read hosts nothing, with no sign-in", async () => {
    const sentence = "`local_only` in _drive.toml must be true or false, not text.";
    vi.mocked(agentsCopies).mockResolvedValue([
      {
        profileId: "p2",
        drive: "marta-notes",
        agent: "",
        name: "marta-notes",
        matrixUser: "",
        device: null,
        host: "hesperia",
        signedIn: false,
        pin: null,
        problem: sentence,
        model: null,
        webSearch: null,
        console: null,
        avatarProblem: null,
      },
    ]);
    render(<AgentsSection open />);

    expect(await screen.findByText(sentence)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "marta-notes" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SIGN_IN_LABEL })).not.toBeInTheDocument();
  });

  it("reads the rows again while open, so a sentence the host cleared goes away", async () => {
    vi.useFakeTimers();
    const waiting = "This Mac has not found your agents' control room yet.";
    vi.mocked(agentsCopies)
      .mockResolvedValueOnce([
        { ...nixi({ ...unpinned, state: "pinned" }, true), problem: waiting },
      ])
      .mockResolvedValue([nixi({ ...unpinned, state: "pinned" }, true)]);
    render(<AgentsSection open />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText(waiting)).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(AGENTS_RELOAD_MS);
    });
    expect(agentsCopies).toHaveBeenCalledTimes(2);
    expect(screen.queryByText(waiting)).not.toBeInTheDocument();
  });

  // Story 100.4 #5: why the picture is not used, beside a signed-in copy's
  // row, and gone when Rust says nothing is wrong.
  it("says beside the agent's row why its picture is not used", async () => {
    vi.useFakeTimers();
    const tooLarge =
      "Nixi's avatar.svg is 301 KiB; keeper draws a picture of at most 256 KiB, so it is not used.";
    vi.mocked(agentsCopies)
      .mockResolvedValueOnce([
        { ...nixi({ ...unpinned, state: "pinned" }, true), avatarProblem: tooLarge },
      ])
      .mockResolvedValue([nixi({ ...unpinned, state: "pinned" }, true)]);
    render(<AgentsSection open />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByTestId("agent-avatar-problem")).toHaveTextContent(tooLarge);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AGENTS_RELOAD_MS);
    });
    expect(screen.queryByTestId("agent-avatar-problem")).not.toBeInTheDocument();
  });

  it("names no host for a signed-in copy when the account has none", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([
      { ...nixi({ ...unpinned, state: "pinned" }, true), host: null },
    ]);
    render(<AgentsSection open />);

    expect(await screen.findByRole("status")).toHaveTextContent(/^Signed in as nixi$/);
  });
});

/** tgdrive with its zone declared in `_drive.toml`, local models only. */
const declared: AgentSeedFolderVm = {
  profileId: "p1",
  name: "tgdrive",
  drive: "tgdrive",
  owner: "@tgorka:tgorka.org",
  readers: ["@marta:tgorka.org", "@tgorka:tgorka.org"],
  localOnly: true,
  declared: true,
  preselected: ["nixi", "tola-grey"],
  problem: null,
};

/** neuradrive, flagged for agents, with no `_drive.toml` yet. */
const fresh: AgentSeedFolderVm = {
  profileId: "p3",
  name: "neuradrive",
  drive: "neuradrive",
  owner: "@tgorka:tgorka.org",
  readers: ["@tgorka:tgorka.org"],
  localOnly: false,
  declared: false,
  // Rust ticks the agents whose souls were written for neuradrive.
  preselected: ["lucyna-novak"],
  problem: null,
};

const WORK_BOT = "bot:openai:https://provider.example:8452/v1#gpt-5";
const QWEN = "bot:ollama:http://electra.example:11434#qwen3:32b";
const QWEN_BOT = {
  reference: QWEN,
  name: "Qwen on electra",
  provider: "electra",
  efforts: ["low", "medium", "high"] as Effort[],
};

function offerOf(...folders: AgentSeedFolderVm[]): AgentSeedOfferVm {
  return {
    folders,
    catalogue: [
      { id: "nixi", name: "Nixi", kind: "proxy", homeDrive: "tgdrive" },
      { id: "tola-grey", name: "Dr Tola Grey", kind: "steward", homeDrive: "tgdrive" },
      { id: "lucyna-novak", name: "Dr Lucyna Novak", kind: "steward", homeDrive: "neuradrive" },
    ],
    bots: [
      {
        reference: WORK_BOT,
        name: "Work model",
        provider: "Provider",
        efforts: ["low", "medium", "high", "xhigh"],
      },
      QWEN_BOT,
    ],
    // Rust's `seed::local_choices` of the two.
    localBots: [QWEN_BOT],
    botsUnread: null,
    accounts: ["@tgorka:tgorka.org"],
  };
}

async function openSetUp() {
  render(<AgentsSection open />);
  fireEvent.click(await screen.findByRole("button", { name: SET_UP_AGENTS_LABEL }));
}

describe("Settings › Agents › Set up agents", () => {
  it("picks no bot for the person and asks Rust with none until they pick one", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf({ ...declared, localOnly: false }));
    vi.mocked(agentsSeedPlan).mockResolvedValue({ write: ["README.md"], left: [] });
    await openSetUp();

    const bots = screen.getAllByRole("radio");
    expect(bots).toHaveLength(2);
    for (const bot of bots) {
      expect(bot).not.toBeChecked();
    }
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(expect.objectContaining({ bot: null })),
    );

    fireEvent.click(screen.getByRole("radio", { name: /Work model/ }));
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(expect.objectContaining({ bot: WORK_BOT })),
    );
  });

  it("shows Rust's refusal as given, with no files to write", async () => {
    const sentence = "Name the bot the seeded agents run on: keeper never picks one for them.";
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf(declared));
    vi.mocked(agentsSeedPlan).mockRejectedValue({
      code: "internal",
      message: sentence,
      accountId: null,
      retriable: false,
    });
    await openSetUp();
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));

    expect((await screen.findByRole("alert")).textContent).toBe(sentence);
    expect(screen.queryByRole("list", { name: /^Files to write/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SEED_WRITE_LABEL })).not.toBeInTheDocument();
  });

  it("asks with a declared zone's own drive, owner and readers, and lists the files to write apart from the files left", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf(declared));
    vi.mocked(agentsSeedPlan).mockResolvedValue({
      write: ["tola-grey/agent.toml", "tola-grey/SOUL.md"],
      left: ["_drive.toml", "nixi/agent.toml"],
    });
    await openSetUp();

    // The file decides these: nothing here edits them.
    expect(screen.queryByLabelText("Drive id")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/^Readers/)).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /^Nixi/ })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /^Dr Lucyna Novak/ })).not.toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: /Qwen on electra/ }));
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));

    const toWrite = await screen.findByRole("list", { name: /^Files to write/ });
    const left = screen.getByRole("list", { name: /^Files left as they are/ });
    expect(
      within(toWrite)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["tola-grey/agent.toml", "tola-grey/SOUL.md"]);
    expect(
      within(left)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["_drive.toml", "nixi/agent.toml"]);
    expect(agentsSeedPlan).toHaveBeenCalledWith({
      profileId: "p1",
      drive: "tgdrive",
      owner: "@tgorka:tgorka.org",
      readers: ["@marta:tgorka.org", "@tgorka:tgorka.org"],
      localOnly: true,
      bot: QWEN,
      with: ["nixi", "tola-grey"],
      effort: null,
      models: [],
    });
  });

  it("drops a preview the person changed the form after, so what is written is what was shown", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf(fresh));
    vi.mocked(agentsSeedPlan).mockResolvedValue({ write: ["README.md"], left: [] });
    await openSetUp();
    fireEvent.click(screen.getByRole("radio", { name: /Work model/ }));
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    expect(await screen.findByRole("button", { name: SEED_WRITE_LABEL })).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(/^Readers/), {
      target: { value: "@tgorka:tgorka.org\n @marta:tgorka.org \n" },
    });
    expect(screen.queryByRole("button", { name: SEED_WRITE_LABEL })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(
        expect.objectContaining({
          drive: "neuradrive",
          readers: ["@tgorka:tgorka.org", "@marta:tgorka.org"],
          with: ["lucyna-novak"],
        }),
      ),
    );
  });

  it("ticks only the agents Rust preselected, and asks local models only when the person ticks it", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(
      offerOf({ ...fresh, drive: "tgdrive", preselected: [] }),
    );
    vi.mocked(agentsSeedPlan).mockResolvedValue({ write: ["README.md"], left: [] });
    await openSetUp();

    for (const agent of [/^Nixi/, /^Dr Tola Grey/, /^Dr Lucyna Novak/]) {
      expect(screen.getByRole("checkbox", { name: agent })).not.toBeChecked();
    }
    const local = screen.getByRole("checkbox", { name: /^Local models only/ });
    expect(local).not.toBeChecked();
    fireEvent.click(local);
    fireEvent.click(screen.getByRole("radio", { name: /Qwen on electra/ }));
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(
        expect.objectContaining({ localOnly: true, with: [] }),
      ),
    );
  });

  it("writes, lists what it wrote, and takes the person to each seeded agent's sign-in row", async () => {
    const lucyna: AgentCopyVm = {
      ...nixi(unpinned),
      profileId: "p3",
      drive: "neuradrive",
      agent: "lucyna-novak",
      name: "Dr Lucyna Novak",
      matrixUser: "@lucyna-novak:tgorka.org",
    };
    vi.mocked(agentsCopies).mockResolvedValueOnce([]).mockResolvedValue([lucyna]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf(fresh));
    vi.mocked(agentsSeedPlan).mockResolvedValue({
      write: ["README.md", "lucyna-novak/agent.toml"],
      left: [],
    });
    vi.mocked(agentsSeedApply).mockResolvedValue({
      profileId: "p3",
      written: ["README.md", "lucyna-novak/agent.toml"],
      left: [],
      agents: ["lucyna-novak"],
    });
    await openSetUp();
    expect(
      screen.queryByLabelText("Password for @lucyna-novak:tgorka.org"),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Work model/ }));
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: SEED_WRITE_LABEL }));

    const written = await screen.findByRole("list", { name: /^Written/ });
    expect(
      within(written)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["README.md", "lucyna-novak/agent.toml"]);
    expect(agentsSeedApply).toHaveBeenCalledWith(expect.objectContaining({ bot: WORK_BOT }));
    expect(agentsCopySignIn).not.toHaveBeenCalled();

    // The row is read again by the write itself, not by the next poll.
    fireEvent.click(
      await screen.findByRole(
        "button",
        { name: "Sign Dr Lucyna Novak in" },
        { timeout: AGENTS_RELOAD_MS / 5 },
      ),
    );
    expect(screen.getByLabelText("Password for @lucyna-novak:tgorka.org")).toHaveFocus();
  });

  it("shows why a folder cannot be seeded from this Mac, and no form", async () => {
    const sentence = "`local_only` in _drive.toml must be true or false, not text.";
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(
      offerOf({ ...fresh, profileId: "p2", name: "marta-notes", problem: sentence }),
    );
    render(<AgentsSection open />);

    expect(await screen.findByText(sentence)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "marta-notes" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SET_UP_AGENTS_LABEL })).not.toBeInTheDocument();
    // No seed form's checkboxes: the one left is this Mac's claims lease.
    expect(screen.getAllByRole("checkbox")).toEqual([
      screen.getByRole("checkbox", { name: CLAIMS_LEASE_LABEL }),
    ]);
  });
});

const OPUS = "bot:openai:https://cliproxy.example:8452#claude-opus-5-5";

/** Nixi on Opus at xhigh; Rust offers Opus (four levels) and Qwen (three). */
const OPUS_XHIGH: AgentModelVm = {
  bot: OPUS,
  effort: "xhigh",
  bots: [
    {
      reference: OPUS,
      name: "Opus",
      provider: "CLIProxyAPI",
      efforts: ["low", "medium", "high", "xhigh"],
    },
    {
      reference: QWEN,
      name: "Qwen on electra",
      provider: "electra",
      efforts: ["low", "medium", "high"],
    },
  ],
  locked: null,
  unread: null,
};

function withModel(model: AgentModelVm): AgentCopyVm {
  return { ...nixi({ ...unpinned, localOnly: false, state: "pinned" }, true), model };
}

async function choose(name: string, option: string) {
  const control = screen.getByRole("combobox", { name });
  fireEvent.keyDown(control, { key: "Enter" });
  fireEvent.click(await screen.findByRole("option", { name: option }));
  await waitFor(() => expect(control).toHaveTextContent(option));
}

function withWebSearch(webSearch: AgentWebSearchVm): AgentCopyVm {
  return { ...nixi({ ...unpinned, localOnly: false, state: "pinned" }, true), webSearch };
}

const SEARCH_OFF: AgentWebSearchVm = {
  level: "off",
  locked: null,
  unavailable: null,
  warning: null,
  host: null,
};

const WARNING =
  "Queries go to search.example.org and its upstream search engines without asking — they may contain words from your files. A host other than this Mac sends them to its own engine.";

function level(name: string) {
  return screen.getByRole("radio", { name });
}

describe("Settings › Agents › an agent's web search (101.4, R457)", () => {
  it("chooses a level against what the row showed and draws the rows Rust answers", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([withWebSearch(SEARCH_OFF)]);
    vi.mocked(agentsWebSearchSet).mockResolvedValue([
      withWebSearch({ ...SEARCH_OFF, level: "ask" }),
    ]);
    render(<AgentsSection open />);

    const group = await screen.findByRole("radiogroup", { name: "Web search" });
    expect(group).toHaveAccessibleDescription(WEB_SEARCH_SWITCH_NOTE);
    expect(level("Off")).toBeChecked();
    fireEvent.click(level("Ask"));

    await waitFor(() =>
      expect(agentsWebSearchSet).toHaveBeenCalledWith({
        profileId: "p1",
        agent: "nixi",
        level: "ask",
        shownLevel: "off",
      }),
    );
    await waitFor(() => expect(level("Ask")).toBeChecked());
  });

  it("stands Rust's warning beside without asking while it is the level, and takes it away after", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([withWebSearch({ ...SEARCH_OFF, level: "ask" })]);
    vi.mocked(agentsWebSearchSet).mockResolvedValueOnce([
      withWebSearch({ ...SEARCH_OFF, level: "withoutAsking", warning: WARNING }),
    ]);
    render(<AgentsSection open />);
    await screen.findByRole("radiogroup", { name: "Web search" });
    expect(screen.queryByText(WARNING)).not.toBeInTheDocument();

    fireEvent.click(level("Without asking"));
    await waitFor(() =>
      expect(agentsWebSearchSet).toHaveBeenLastCalledWith({
        profileId: "p1",
        agent: "nixi",
        level: "withoutAsking",
        shownLevel: "ask",
      }),
    );
    await waitFor(() => expect(level("Without asking")).toBeChecked());
    expect(screen.getByText(WARNING)).toBeInTheDocument();
    expect(level("Without asking")).toHaveAccessibleDescription(WARNING);

    vi.mocked(agentsWebSearchSet).mockResolvedValueOnce([
      withWebSearch({ ...SEARCH_OFF, level: "ask" }),
    ]);
    fireEvent.click(level("Ask"));
    await waitFor(() =>
      expect(agentsWebSearchSet).toHaveBeenLastCalledWith({
        profileId: "p1",
        agent: "nixi",
        level: "ask",
        shownLevel: "withoutAsking",
      }),
    );
    await waitFor(() => expect(level("Ask")).toBeChecked());
    expect(screen.queryByText(WARNING)).not.toBeInTheDocument();
  });

  it("says Rust's refusal beside the choice and leaves it as the file says", async () => {
    const sentence =
      "This agent's agent.toml changed on the drive meanwhile, so web search was not switched. Look at it again and choose.";
    vi.mocked(agentsCopies).mockResolvedValue([withWebSearch(SEARCH_OFF)]);
    vi.mocked(agentsWebSearchSet).mockRejectedValue({
      code: "internal",
      message: sentence,
      accountId: null,
      retriable: false,
    });
    render(<AgentsSection open />);
    await screen.findByRole("radiogroup", { name: "Web search" });
    fireEvent.click(level("Ask"));

    expect((await screen.findByRole("alert")).textContent).toBe(sentence);
    expect(level("Off")).toBeChecked();
  });

  it("offers only off for an agent Rust offers no search, so one that is on can be switched off", async () => {
    const unavailable =
      "Nixi uses only models that run on its readers' own machines, and a web search sends its query out, so it is never offered one.";
    vi.mocked(agentsCopies).mockResolvedValue([
      withWebSearch({ ...SEARCH_OFF, level: "withoutAsking", unavailable }),
    ]);
    render(<AgentsSection open />);
    const group = await screen.findByRole("radiogroup", { name: "Web search" });
    expect(group).toHaveAccessibleDescription(unavailable);
    expect(level("Ask")).toBeDisabled();
    expect(level("Without asking")).toBeDisabled();
    expect(level("Off")).toBeEnabled();
  });

  it("changes nothing on a Mac that only pulls the drive, and says why", async () => {
    const locked =
      "This Mac only pulls tgdrive, so a change made here would never reach the drive. Switch web search where the drive is written.";
    vi.mocked(agentsCopies).mockResolvedValue([
      withWebSearch({ ...SEARCH_OFF, level: "ask", locked }),
    ]);
    render(<AgentsSection open />);
    const group = await screen.findByRole("radiogroup", { name: "Web search" });
    expect(group).toHaveAccessibleDescription(locked);
    for (const name of ["Off", "Ask", "Without asking"]) expect(level(name)).toBeDisabled();
  });
});

describe("Settings › Agents › an agent's model (100.2)", () => {
  it("offers the bots Rust lists with each one's levels, and saves the choice against what it showed", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([withModel(OPUS_XHIGH)]);
    const saved: AgentModelVm = { ...OPUS_XHIGH, bot: QWEN, effort: "high" };
    vi.mocked(agentsModelSet).mockResolvedValue([withModel(saved)]);
    render(<AgentsSection open />);

    const model = await screen.findByRole("combobox", { name: "Model" });
    expect(model).toHaveTextContent("Opus — CLIProxyAPI");
    expect(screen.getByRole("combobox", { name: "Reasoning" })).toHaveTextContent("xhigh");
    // Nothing changed: nothing to save.
    expect(screen.getByRole("button", { name: MODEL_SAVE_LABEL })).toBeDisabled();

    // Qwen is sent no xhigh, so the level goes back to the provider's default.
    await choose("Model", "Qwen on electra — electra");
    expect(screen.getByRole("combobox", { name: "Reasoning" })).toHaveTextContent(
      PROVIDER_DEFAULT_LABEL,
    );
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Reasoning" }), { key: "Enter" });
    const levels = await screen.findAllByRole("option");
    expect(levels.map((level) => level.textContent)).toEqual([
      PROVIDER_DEFAULT_LABEL,
      "low",
      "medium",
      "high",
    ]);
    fireEvent.click(levels[3]);
    fireEvent.click(screen.getByRole("button", { name: MODEL_SAVE_LABEL }));

    await waitFor(() =>
      expect(agentsModelSet).toHaveBeenCalledWith({
        profileId: "p1",
        agent: "nixi",
        bot: QWEN,
        effort: "high",
        shownBot: OPUS,
        shownEffort: "xhigh",
      }),
    );
    expect(
      await screen.findByText("Saved to Nixi's agent.toml. Its next turn runs on it."),
    ).toHaveAttribute("role", "status");
  });

  it("says Rust's refusal beside the control and keeps the choice", async () => {
    const sentence =
      "This agent's agent.toml names another model than the one shown here: it changed on the drive meanwhile. Look at it again and choose.";
    vi.mocked(agentsCopies).mockResolvedValue([withModel(OPUS_XHIGH)]);
    vi.mocked(agentsModelSet).mockRejectedValue({
      code: "internal",
      message: sentence,
      accountId: null,
      retriable: false,
    });
    render(<AgentsSection open />);
    await screen.findByRole("combobox", { name: "Model" });
    await choose("Reasoning", "low");
    fireEvent.click(screen.getByRole("button", { name: MODEL_SAVE_LABEL }));

    expect((await screen.findByRole("alert")).textContent).toBe(sentence);
    expect(screen.getByRole("combobox", { name: "Reasoning" })).toHaveTextContent("low");
  });

  it("offers nothing to change on a Mac that cannot write the drive, and says why", async () => {
    const locked =
      "This Mac only pulls tgdrive, so a change made here would never reach the drive. Change the model where the drive is written.";
    vi.mocked(agentsCopies).mockResolvedValue([withModel({ ...OPUS_XHIGH, locked })]);
    render(<AgentsSection open />);

    expect(await screen.findByText(locked)).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Model" })).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Reasoning" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: MODEL_SAVE_LABEL })).not.toBeInTheDocument();
  });

  it("lets each seeded agent take its own bot and level, the rest the folder's", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf({ ...declared, localOnly: false }));
    vi.mocked(agentsSeedPlan).mockResolvedValue({ write: ["README.md"], left: [] });
    await openSetUp();

    fireEvent.click(screen.getByRole("radio", { name: /Work model/ }));
    await choose("Their reasoning level", "xhigh");
    await choose("Dr Tola Grey", "Qwen on electra");
    await choose("Dr Tola Grey's level", "low");
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));

    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(
        expect.objectContaining({
          bot: WORK_BOT,
          effort: "xhigh",
          with: ["nixi", "tola-grey"],
          models: [{ agent: "tola-grey", bot: QWEN, effort: "low" }],
        }),
      ),
    );
  });

  it("offers a local-only folder only the bots Rust lists as local, for the folder and each agent", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf(declared));
    await openSetUp();

    const radios = screen.getAllByRole("radio");
    expect(radios.map((radio) => radio.getAttribute("value"))).toEqual([QWEN]);
    fireEvent.click(screen.getByRole("radio", { name: /Qwen on electra/ }));
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Dr Tola Grey" }), { key: "Enter" });
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "The agents' bot and level",
      "Qwen on electra",
    ]);
  });

  it("takes back a choice the folder stops offering when its agents become local-only", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([]);
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf({ ...fresh, localOnly: false }));
    vi.mocked(agentsSeedPlan).mockResolvedValue({ write: ["README.md"], left: [] });
    await openSetUp();

    fireEvent.click(screen.getByRole("radio", { name: /Work model/ }));
    await choose("Dr Lucyna Novak", "Work model");
    fireEvent.click(screen.getByRole("checkbox", { name: /^Local models only/ }));

    expect(screen.queryByRole("radio", { name: /Work model/ })).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Qwen on electra/ })).not.toBeChecked();
    // Nothing the folder no longer offers is asked for.
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(
        expect.objectContaining({ localOnly: true, bot: null, models: [] }),
      ),
    );
    fireEvent.click(screen.getByRole("radio", { name: /Qwen on electra/ }));
    expect(screen.getByRole("combobox", { name: "Dr Lucyna Novak" })).toHaveTextContent(
      "The agents' bot and level",
    );
    fireEvent.click(screen.getByRole("button", { name: SEED_PREVIEW_LABEL }));
    await waitFor(() =>
      expect(agentsSeedPlan).toHaveBeenLastCalledWith(
        expect.objectContaining({ localOnly: true, bot: QWEN, models: [] }),
      ),
    );
  });

  it("says Rust's sentence where the bots would be when they could not be read", async () => {
    const sentence =
      "Your bots could not be read on this Mac, so no other bot can be offered here. The reason is in keeper's log; look at Settings › Bots, then open this again.";
    vi.mocked(agentsCopies).mockResolvedValue([
      withModel({ ...OPUS_XHIGH, bots: [OPUS_XHIGH.bots[0]], unread: sentence }),
    ]);
    vi.mocked(agentsSeedOffer).mockResolvedValue({
      ...offerOf({ ...declared, localOnly: false }),
      bots: [],
      localBots: [],
      botsUnread: sentence,
    });
    await openSetUp();

    const alerts = await screen.findAllByRole("alert");
    expect(alerts.map((alert) => alert.textContent)).toEqual([sentence, sentence]);
    expect(screen.queryByText(SEED_NO_BOTS)).not.toBeInTheDocument();
    expect(screen.queryAllByRole("radio")).toHaveLength(0);
    expect(screen.getByRole("combobox", { name: "Model" })).toHaveTextContent("Opus");
  });
});

/**
 * Nixi's console as Rust shows it (101.5): sandboxed with one folder this Mac
 * does not give her commands (R440), every mode offered, full access refused
 * for her with Rust's reason.
 */
const SITE_REFUSED = "Not given to its commands on this Mac: it is not on this host.";
const UNRESTRICTED = "Unrestricted — folders do not constrain full access.";
const SANDBOXED: AgentConsoleVm = {
  mode: "sandboxed",
  paths: ["tgdrive/projects/site"],
  modes: [
    { mode: "off", label: "Off", description: "No console.", refused: null },
    { mode: "supervised", label: "Supervised", description: "Every command asks.", refused: null },
    { mode: "sandboxed", label: "Sandboxed", description: "Runs in the sandbox.", refused: null },
    {
      mode: "full",
      label: "Full access",
      description: "No sandbox.",
      refused: "Nixi cannot have this mode: the gate forge names this one in its delegates.",
    },
  ],
  drives: ["tgdrive"],
  warning: "Full access: Nixi's commands run on its host without a sandbox and without asking.",
  unrestricted: UNRESTRICTED,
  refused: [{ path: "tgdrive/projects/site", refused: SITE_REFUSED }],
  locked: null,
};

function withConsole(shown: AgentConsoleVm): AgentCopyVm {
  return { ...nixi({ ...unpinned, localOnly: false, state: "pinned" }, true), console: shown };
}

describe("Settings › Agents › an agent's console (101.5)", () => {
  it("saves the chosen mode and folders against the console it showed", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([withConsole(SANDBOXED)]);
    vi.mocked(agentsConsoleSet).mockResolvedValue([
      withConsole({ ...SANDBOXED, mode: "supervised", paths: ["/Users/tgorka/prj/keeper"] }),
    ]);
    render(<AgentsSection open />);

    const mode = await screen.findByRole("combobox", { name: "Console" });
    expect(mode).toHaveTextContent("Sandboxed");
    // R440, R381: the folder this Mac does not give says why, beside it.
    expect(screen.getByText(SITE_REFUSED)).toBeInTheDocument();
    // Nothing changed: nothing to save.
    expect(screen.getByRole("button", { name: CONSOLE_SAVE_LABEL })).toBeDisabled();

    await choose("Console", "Supervised");
    fireEvent.click(screen.getByRole("button", { name: "Remove tgdrive/projects/site" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Folder" }), {
      target: { value: " /Users/tgorka/prj/keeper " },
    });
    fireEvent.click(screen.getByRole("button", { name: CONSOLE_ADD_FOLDER_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: CONSOLE_SAVE_LABEL }));

    await waitFor(() =>
      expect(agentsConsoleSet).toHaveBeenCalledWith({
        profileId: "p1",
        agent: "nixi",
        mode: "supervised",
        paths: ["/Users/tgorka/prj/keeper"],
        shownMode: "sandboxed",
        shownPaths: ["tgdrive/projects/site"],
      }),
    );
    expect(await screen.findByText(/^Saved to Nixi's agent\.toml\./)).toHaveAttribute(
      "role",
      "status",
    );
  });

  it("does not offer a mode Rust refuses this agent, and says Rust's warning and no folders when full access is chosen", async () => {
    vi.mocked(agentsCopies).mockResolvedValue([
      withConsole(SANDBOXED),
      {
        ...withConsole({
          ...SANDBOXED,
          modes: SANDBOXED.modes.map((mode) => ({ ...mode, refused: null })),
          warning: "Full access: Tara's commands run without a sandbox.",
        }),
        agent: "tara",
        name: "Tara",
      },
    ]);
    render(<AgentsSection open />);
    const [nixiMode, taraMode] = await screen.findAllByRole("combobox", { name: "Console" });

    fireEvent.keyDown(nixiMode, { key: "Enter" });
    expect(await screen.findByRole("option", { name: "Full access" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    fireEvent.keyDown(await screen.findByRole("listbox"), { key: "Escape" });
    expect(screen.queryByTestId("agent-console-warning-nixi")).not.toBeInTheDocument();

    fireEvent.keyDown(taraMode, { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: "Full access" }));
    expect((await screen.findByTestId("agent-console-warning-tara")).textContent).toBe(
      "Full access: Tara's commands run without a sandbox.",
    );
    // Full access is held to no folder: Rust's sentence where the folders
    // were, never "only the session's own workspace".
    const tara = screen.getByTestId("agent-console-tara");
    expect(screen.getByTestId("agent-console-unrestricted-tara").textContent).toBe(UNRESTRICTED);
    expect(within(tara).queryByRole("textbox", { name: "Folder" })).not.toBeInTheDocument();
    expect(within(tara).queryByText(/only the session's own workspace/)).not.toBeInTheDocument();
  });

  it("says Rust's refusal beside the control and keeps the choice", async () => {
    const sentence = 'Nixi\'s console was not changed: "~/x" starts with ~.';
    vi.mocked(agentsCopies).mockResolvedValue([withConsole(SANDBOXED)]);
    vi.mocked(agentsConsoleSet).mockRejectedValue({
      code: "internal",
      message: sentence,
      accountId: null,
      retriable: false,
    });
    render(<AgentsSection open />);
    await screen.findByRole("combobox", { name: "Console" });
    fireEvent.change(screen.getByRole("textbox", { name: "Folder" }), {
      target: { value: "~/x" },
    });
    fireEvent.click(screen.getByRole("button", { name: CONSOLE_ADD_FOLDER_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: CONSOLE_SAVE_LABEL }));

    expect((await screen.findByRole("alert")).textContent).toBe(sentence);
    expect(screen.getByText("~/x")).toBeInTheDocument();
  });
});

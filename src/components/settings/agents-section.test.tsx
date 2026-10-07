import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AGENTS_RELOAD_MS,
  AgentsSection,
  PIN_LABEL,
  REVIEW_READERS_LABEL,
  SEED_PREVIEW_LABEL,
  SEED_WRITE_LABEL,
  SET_UP_AGENTS_LABEL,
  SIGN_IN_LABEL,
} from "@/components/settings/agents-section";
import type {
  AgentCopyVm,
  AgentPersonVm,
  AgentPinVm,
  AgentSeedFolderVm,
  AgentSeedOfferVm,
} from "@/lib/ipc/client";
import {
  agentsCopies,
  agentsCopySignIn,
  agentsDriveRepin,
  agentsMcpList,
  agentsMcpRemove,
  agentsSeedApply,
  agentsSeedOffer,
  agentsSeedPlan,
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
  agentsMcpDraft: vi.fn(() =>
    Promise.resolve({ floor: null, fixed: null, tools: [], conflicts: [], drop: null }),
  ),
  agentsSandboxGet: vi.fn(() =>
    Promise.resolve({ readExec: [], env: [], status: "", refusal: null }),
  ),
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
  };
}

const NO_FOLDERS: AgentSeedOfferVm = { folders: [], catalogue: [], bots: [], accounts: [] };

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

function offerOf(...folders: AgentSeedFolderVm[]): AgentSeedOfferVm {
  return {
    folders,
    catalogue: [
      { id: "nixi", name: "Nixi", kind: "proxy", homeDrive: "tgdrive" },
      { id: "tola-grey", name: "Dr Tola Grey", kind: "steward", homeDrive: "tgdrive" },
      { id: "lucyna-novak", name: "Dr Lucyna Novak", kind: "steward", homeDrive: "neuradrive" },
    ],
    bots: [
      { reference: WORK_BOT, name: "Work model", provider: "Provider" },
      {
        reference: "bot:ollama:http://electra.example:11434#qwen3:32b",
        name: "Qwen on electra",
        provider: "electra",
      },
    ],
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
    vi.mocked(agentsSeedOffer).mockResolvedValue(offerOf(declared));
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
    fireEvent.click(screen.getByRole("radio", { name: /Work model/ }));
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
      bot: WORK_BOT,
      with: ["nixi", "tola-grey"],
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
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });
});

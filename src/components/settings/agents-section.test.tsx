import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AGENTS_RELOAD_MS,
  AgentsSection,
  PIN_LABEL,
  REVIEW_READERS_LABEL,
  SIGN_IN_LABEL,
} from "@/components/settings/agents-section";
import type { AgentCopyVm, AgentPersonVm, AgentPinVm } from "@/lib/ipc/client";
import { agentsCopies, agentsCopySignIn, agentsDriveRepin } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsCopies: vi.fn(),
  agentsCopySignIn: vi.fn(),
  agentsDriveRepin: vi.fn(),
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

beforeEach(() => {
  vi.clearAllMocks();
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

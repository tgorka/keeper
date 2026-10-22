import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DefaultAgentVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsDefaultGet: vi.fn(),
  agentsDefaultSet: vi.fn(),
  frontendErrorReport: vi.fn(() => Promise.resolve()),
}));
vi.mock("@/components/settings/config-source-section", () => ({ FileControlled: () => null }));

import {
  DEFAULT_AGENT_NONE,
  DEFAULT_AGENT_TITLE,
  DefaultAgentSection,
} from "@/components/settings/default-agent-section";
import { agentsDefaultGet, agentsDefaultSet } from "@/lib/ipc/client";
import { defaultAgentStore, readFailedSentence } from "@/lib/stores/default-agent";

const NIXI = "nixie";
const TALA = "tala";

function vm(overrides: Partial<DefaultAgentVm>): DefaultAgentVm {
  return {
    defaultAgent: null,
    reason: null,
    choices: [{ agent: NIXI, name: "Nixi" }],
    chooser: false,
    fileSet: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.mocked(agentsDefaultGet).mockReset();
  vi.mocked(agentsDefaultSet).mockReset();
  defaultAgentStore.setState({
    vm: null,
    readError: null,
    saveError: null,
    saving: false,
    navRestore: "pending",
  });
});

describe("DefaultAgentSection", () => {
  it("shows the stored default selected and saves a new choice", async () => {
    vi.mocked(agentsDefaultGet).mockResolvedValue(vm({ defaultAgent: NIXI }));
    vi.mocked(agentsDefaultSet).mockResolvedValue(vm({ defaultAgent: null }));
    render(<DefaultAgentSection open />);
    const nixi = await screen.findByRole("radio", { name: /Nixi/ });
    expect(nixi).toBeChecked();

    fireEvent.click(screen.getByRole("radio", { name: DEFAULT_AGENT_NONE }));
    await waitFor(() => expect(agentsDefaultSet).toHaveBeenCalledWith(null));
    await waitFor(() =>
      expect(screen.getByRole("radio", { name: DEFAULT_AGENT_NONE })).toBeChecked(),
    );
  });

  it("keeps a stored agent no database here names selected, beside Rust's reason", async () => {
    const reason = `Your default agent ${TALA} is in none of the agents' databases this device holds.`;
    vi.mocked(agentsDefaultGet).mockResolvedValue(vm({ defaultAgent: TALA, reason }));
    render(<DefaultAgentSection open />);
    expect(await screen.findByRole("radio", { name: TALA })).toBeChecked();
    expect(screen.getByText(reason)).toBeInTheDocument();
  });

  it("says why a save failed beside the control and keeps what was stored", async () => {
    vi.mocked(agentsDefaultGet).mockResolvedValue(vm({}));
    vi.mocked(agentsDefaultSet).mockRejectedValue({
      message: "keeper could not save the default agent: database is locked",
    });
    render(<DefaultAgentSection open />);
    fireEvent.click(await screen.findByRole("radio", { name: /Nixi/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
    expect(screen.getByRole("radio", { name: DEFAULT_AGENT_NONE })).toBeChecked();
  });

  it("is absent with no agent to choose and none stored", async () => {
    vi.mocked(agentsDefaultGet).mockResolvedValue(vm({ choices: [] }));
    const { container } = render(<DefaultAgentSection open />);
    await waitFor(() => expect(agentsDefaultGet).toHaveBeenCalled());
    await waitFor(() => expect(defaultAgentStore.getState().vm).not.toBeNull());
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByText(DEFAULT_AGENT_TITLE)).not.toBeInTheDocument();
  });

  it("says a failed read even when what it holds has nothing to choose", async () => {
    defaultAgentStore.setState({ vm: vm({ choices: [] }) });
    vi.mocked(agentsDefaultGet).mockRejectedValue(new Error("the settings table is locked"));
    render(<DefaultAgentSection open />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      readFailedSentence(new Error("the settings table is locked")),
    );
  });

  it("names the file that decides the default, and shows the agent it opens", async () => {
    const fileSet =
      "/etc/keeper.toml (user) sets agents.default to nixie, so nixie opens at launch whatever you choose here. Your choice is kept and takes effect once that file stops setting it.";
    vi.mocked(agentsDefaultGet).mockResolvedValue(vm({ defaultAgent: NIXI, fileSet }));
    vi.mocked(agentsDefaultSet).mockResolvedValue(vm({ defaultAgent: NIXI, fileSet }));
    render(<DefaultAgentSection open />);
    expect(await screen.findByText(fileSet)).toBeInTheDocument();
    // Choosing None is stored, but the file's agent is still what opens.
    fireEvent.click(screen.getByRole("radio", { name: DEFAULT_AGENT_NONE }));
    await waitFor(() => expect(agentsDefaultSet).toHaveBeenCalledWith(null));
    await waitFor(() => expect(screen.getByRole("radio", { name: /Nixi/ })).toBeChecked());
  });
});

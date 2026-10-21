import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentInstancesSection } from "@/components/settings/agent-instances-section";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentInstancesVm, AgentInstanceVm } from "@/lib/ipc/client";

const mock = vi.hoisted(() => ({
  push: null as ((agents: AgentInstancesVm[]) => void) | null,
  subscribe: vi.fn(),
  unsubscribe: vi.fn().mockResolvedValue(undefined),
  set: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentInstancesSubscribe: mock.subscribe,
  agentInstancesUnsubscribe: mock.unsubscribe,
  agentInstancesSet: mock.set,
}));

function instance(patch: Partial<AgentInstanceVm> & Pick<AgentInstanceVm, "clientId">) {
  return {
    label: patch.clientId.replace(".", "@"),
    databases: ["tgdrive", "neuradrive"],
    presence: "online" as const,
    definition: "3f9c2a71b0de",
    ...patch,
  };
}

function nixie(patch: Partial<AgentInstancesVm> = {}): AgentInstancesVm {
  return {
    agentActor: "nixie",
    agentName: "Nixie",
    agentMark: "N",
    databases: ["tgdrive", "neuradrive"],
    instances: [
      instance({ clientId: "nixie.electra", rank: 0 }),
      instance({ clientId: "nixie.hesperia", rank: 1, presence: "offline" }),
      instance({ clientId: "nixie.laptop", rank: 2, presence: "unknown" }),
    ],
    defaultClient: "nixie.electra",
    fallback: ["nixie.hesperia", "nixie.laptop"],
    sentence: "New sessions go to nixie@electra; while it is offline, to nixie@hesperia.",
    ...patch,
  };
}

async function drawWith(agents: AgentInstancesVm[]) {
  render(<AgentInstancesSection open />);
  await waitFor(() => expect(mock.push).not.toBeNull());
  act(() => mock.push?.(agents));
}

const row = (client: string) =>
  document.querySelector(`[data-instance="${client}"]`) as HTMLElement;

beforeEach(() => {
  vi.clearAllMocks();
  mock.push = null;
  mock.subscribe.mockImplementation(async (callback) => {
    mock.push = callback;
    return "instances-1";
  });
});

describe("Settings › Answers from", () => {
  it("is absent while Rust lists no agent with an instance", async () => {
    const { container } = render(<AgentInstancesSection open />);
    await waitFor(() => expect(mock.push).not.toBeNull());
    act(() => mock.push?.([]));
    expect(container).toBeEmptyDOMElement();
  });

  it("draws Rust's order, who is online, and the marks it sends", async () => {
    await drawWith([
      nixie({
        disagreement:
          "Your databases differ: tgdrive asks nixie@electra; neuradrive asks nixie@hesperia.",
        instances: [
          instance({ clientId: "nixie.electra", rank: 0 }),
          instance({
            clientId: "nixie.hesperia",
            rank: 1,
            presence: "offline",
            definition: "a01b44c9e2f7",
            definitionDiffers: "Runs a different definition from nixie@electra.",
          }),
          instance({ clientId: "nixie.laptop", rank: 2, presence: "unknown" }),
        ],
      }),
    ]);
    const group = screen.getByRole("radiogroup", { name: "Answers from: Nixie" });
    expect(within(group).getByRole("radio", { name: /nixie@electra/ })).toBeChecked();
    expect(within(row("nixie.electra")).getByText("Online")).toBeInTheDocument();
    expect(within(row("nixie.electra")).getByText("Answers first")).toBeInTheDocument();
    expect(within(row("nixie.hesperia")).getByText("Offline")).toBeInTheDocument();
    expect(within(row("nixie.hesperia")).getByText("First fallback")).toBeInTheDocument();
    expect(within(row("nixie.laptop")).getByText("Not known here")).toBeInTheDocument();
    expect(
      within(row("nixie.hesperia")).getByText(/Runs a different definition from nixie@electra/),
    ).toBeInTheDocument();
    expect(within(row("nixie.electra")).queryByText(/different definition/i)).toBeNull();
    expect(screen.getByRole("note")).toHaveTextContent("Your databases differ");
    expect(
      screen.getByText("New sessions go to nixie@electra; while it is offline, to nixie@hesperia."),
    ).toBeInTheDocument();
  });

  it("writes the chosen instance first, then the others as shown, and waits for Rust's next list", async () => {
    await drawWith([nixie()]);
    fireEvent.click(screen.getByRole("radio", { name: /nixie@hesperia/ }));
    await waitFor(() =>
      expect(mock.set).toHaveBeenCalledWith("nixie", [
        "nixie.hesperia",
        "nixie.electra",
        "nixie.laptop",
      ]),
    );
    // Nothing moved here: the radio follows the row Rust reads back.
    expect(screen.getByRole("radio", { name: /nixie@electra/ })).toBeChecked();
    act(() =>
      mock.push?.([
        nixie({
          defaultClient: "nixie.hesperia",
          fallback: ["nixie.electra", "nixie.laptop"],
          instances: [
            instance({ clientId: "nixie.hesperia", rank: 0 }),
            instance({ clientId: "nixie.electra", rank: 1 }),
            instance({ clientId: "nixie.laptop", rank: 2 }),
          ],
        }),
      ]),
    );
    expect(screen.getByRole("radio", { name: /nixie@hesperia/ })).toBeChecked();
  });

  it("moves a fallback within the fallback, never above the instance that answers first", async () => {
    await drawWith([nixie()]);
    expect(screen.queryByRole("button", { name: "Ask nixie@electra earlier" })).toBeNull();
    expect(screen.getByRole("button", { name: "Ask nixie@hesperia earlier" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Ask nixie@laptop earlier" }));
    await waitFor(() =>
      expect(mock.set).toHaveBeenCalledWith("nixie", [
        "nixie.electra",
        "nixie.laptop",
        "nixie.hesperia",
      ]),
    );
    expect(screen.getByRole("button", { name: "Ask nixie@laptop later" })).toBeDisabled();
  });

  it("says a refused write, and the server's refusal Rust sends back", async () => {
    mock.set.mockRejectedValueOnce({
      code: "internal",
      message: "Choose the instance that answers first.",
    });
    await drawWith([nixie()]);
    fireEvent.click(screen.getByRole("radio", { name: /nixie@laptop/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Choose the instance that answers first.",
    );
    act(() =>
      mock.push?.([nixie({ notice: "The server refused your choice in neuradrive: hq.v9.id" })]),
    );
    expect(
      screen.getByText("The server refused your choice in neuradrive: hq.v9.id"),
    ).toBeInTheDocument();
  });
});

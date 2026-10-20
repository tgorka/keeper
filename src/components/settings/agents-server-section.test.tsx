import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AGENTS_SERVER_LABEL,
  AGENTS_SERVER_SAVE,
  AgentsServerSection,
  FORGET_SERVER,
  PAIR_CODE_LABEL,
  PAIR_REDEEM,
} from "@/components/settings/agents-server-section";
import {
  type HqStatusVm,
  hqDeviceRevoke,
  hqDevicesList,
  hqPairRedeem,
  hqServerSet,
  hqStatusSubscribe,
} from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  hqStatusSubscribe: vi.fn(),
  hqStatusUnsubscribe: vi.fn(() => Promise.resolve()),
  hqServerSet: vi.fn(),
  hqServerForget: vi.fn(),
  hqPairStart: vi.fn(),
  hqPairRedeem: vi.fn(),
  hqDevicesList: vi.fn(() => Promise.resolve([])),
  hqDeviceRevoke: vi.fn(),
}));
vi.mock("@/components/settings/config-source-section", () => ({
  FileControlled: () => null,
}));

const NO_SERVER: HqStatusVm = {
  server: null,
  phase: "noServer",
  sentence: "No server is set. What you write to your agents waits on this device.",
  device: null,
  databases: [],
  waitingHere: 0,
};

let push: (status: HqStatusVm) => void = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(hqStatusSubscribe).mockImplementation((onStatus) => {
    push = onStatus;
    onStatus(NO_SERVER);
    return Promise.resolve("sub-1");
  });
});

describe("AgentsServerSection (UX-DR-E106-1)", () => {
  it("shows Rust's no-server line and its refusal of an address, keeping the typed text", async () => {
    const refusal =
      "A server address starts with https:// or http://, as in https://agents.example.org.";
    vi.mocked(hqServerSet).mockRejectedValue({ code: "internal", message: refusal });
    render(<AgentsServerSection open />);
    expect(await screen.findByRole("status")).toHaveTextContent(NO_SERVER.sentence);
    fireEvent.change(screen.getByLabelText(AGENTS_SERVER_LABEL), {
      target: { value: "agents.example.org" },
    });
    fireEvent.click(screen.getByRole("button", { name: AGENTS_SERVER_SAVE }));
    expect(await screen.findByRole("alert")).toHaveTextContent(refusal);
    expect(hqServerSet).toHaveBeenCalledWith("agents.example.org");
    expect(screen.getByLabelText(AGENTS_SERVER_LABEL)).toHaveValue("agents.example.org");
  });

  it("offers the code field only while not paired, and redeems what was typed", async () => {
    render(<AgentsServerSection open />);
    await screen.findByRole("status");
    expect(screen.queryByLabelText(PAIR_CODE_LABEL)).toBeNull();
    const notPaired: HqStatusVm = {
      ...NO_SERVER,
      server: "https://agents.example.org",
      phase: "notPaired",
      sentence: "This device is not paired with agents.example.org.",
    };
    act(() => push(notPaired));
    vi.mocked(hqPairRedeem).mockResolvedValue({ ...notPaired, phase: "paired" });
    fireEvent.change(await screen.findByLabelText(PAIR_CODE_LABEL), {
      target: { value: "7K2M-Q9XD-4HTR-8BWN-3CVE-6PJA" },
    });
    fireEvent.click(screen.getByRole("button", { name: PAIR_REDEEM }));
    await waitFor(() =>
      expect(hqPairRedeem).toHaveBeenCalledWith("7K2M-Q9XD-4HTR-8BWN-3CVE-6PJA"),
    );
    await waitFor(() => expect(screen.queryByLabelText(PAIR_CODE_LABEL)).toBeNull());
  });

  it("draws each database's line and removes another device", async () => {
    const paired: HqStatusVm = {
      server: "https://agents.example.org",
      phase: "paired",
      sentence: "Paired with agents.example.org.",
      device: { clientId: "t.1", name: "keeper on hesperia" },
      databases: [
        { name: "tgdrive", state: "connected", waiting: 0, sentence: "Connected." },
        {
          name: "neuradrive",
          state: "notOpen",
          waiting: 0,
          sentence: "Not open to this device any more; what it holds stays readable here.",
        },
      ],
      waitingHere: 0,
    };
    vi.mocked(hqStatusSubscribe).mockImplementation((onStatus) => {
      onStatus(paired);
      return Promise.resolve("sub-1");
    });
    const kalypso = {
      clientId: "t.2",
      name: "keeper on kalypso",
      addedAtMs: null,
      thisDevice: false,
      revoked: false,
    };
    vi.mocked(hqDevicesList).mockResolvedValue([
      { clientId: "t.1", name: "keeper on hesperia", addedAtMs: null, thisDevice: true, revoked: false },
      kalypso,
    ]);
    vi.mocked(hqDeviceRevoke).mockResolvedValue([{ ...kalypso, revoked: true }]);
    render(<AgentsServerSection open />);
    const databases = await screen.findByRole("list", { name: "Databases" });
    expect(within(databases).getByRole("listitem", { name: "neuradrive" })).toHaveTextContent(
      "Not open to this device any more",
    );
    fireEvent.click(await screen.findByRole("button", { name: "Remove keeper on kalypso" }));
    await waitFor(() => expect(hqDeviceRevoke).toHaveBeenCalledWith("t.2"));
    expect(await screen.findByText("Removed")).toBeInTheDocument();
  });

  it("offers to remove a removed device's copies", async () => {
    vi.mocked(hqStatusSubscribe).mockImplementation((onStatus) => {
      onStatus({ ...NO_SERVER, server: "https://agents.example.org", phase: "removed" });
      return Promise.resolve("sub-1");
    });
    render(<AgentsServerSection open />);
    expect(await screen.findByRole("button", { name: FORGET_SERVER })).toBeInTheDocument();
  });
});

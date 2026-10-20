import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AGENTS_SERVER_LABEL,
  AGENTS_SERVER_SAVE,
  AgentsServerSection,
  FORGET_SERVER,
  OFFER_PAIR,
  PAIR_CODE_LABEL,
  PAIR_REDEEM,
} from "@/components/settings/agents-server-section";
import {
  type HqStatusVm,
  hqDeviceRevoke,
  hqPairOfferAccept,
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
  hqPairOfferAccept: vi.fn(),
  hqPairOfferDismiss: vi.fn(() => Promise.resolve()),
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
  devices: [],
  offer: null,
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

  it("offers the code field until paired, and redeems what was typed", async () => {
    render(<AgentsServerSection open />);
    await screen.findByRole("status");
    // With no server yet, the field takes a pairing link, which names its server.
    expect(screen.getByLabelText(PAIR_CODE_LABEL)).toBeInTheDocument();
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
    await waitFor(() => expect(hqPairRedeem).toHaveBeenCalledWith("7K2M-Q9XD-4HTR-8BWN-3CVE-6PJA"));
    await waitFor(() => expect(screen.queryByLabelText(PAIR_CODE_LABEL)).toBeNull());
  });

  it("draws each database's line and the devices the status carries, and follows them", async () => {
    const kalypso = {
      clientId: "ana.2",
      name: "keeper on phone",
      addedAtMs: null,
      thisDevice: false,
      revoked: false,
    };
    const paired: HqStatusVm = {
      server: "https://agents.example.org",
      phase: "paired",
      sentence: "Paired with agents.example.org.",
      device: { clientId: "ana.1", name: "keeper on laptop" },
      databases: [
        { name: "alpha", state: "connected", waiting: 0, sentence: "Connected." },
        {
          name: "beta",
          state: "notOpen",
          waiting: 0,
          sentence: "Not open to this device any more; what it holds stays readable here.",
        },
      ],
      waitingHere: 0,
      devices: [
        {
          clientId: "ana.1",
          name: "keeper on laptop",
          addedAtMs: null,
          thisDevice: true,
          revoked: false,
        },
        kalypso,
      ],
      offer: null,
    };
    vi.mocked(hqStatusSubscribe).mockImplementation((onStatus) => {
      push = onStatus;
      onStatus(paired);
      return Promise.resolve("sub-1");
    });
    vi.mocked(hqDeviceRevoke).mockResolvedValue(paired);
    render(<AgentsServerSection open />);
    const databases = await screen.findByRole("list", { name: "Databases" });
    expect(within(databases).getByRole("listitem", { name: "beta" })).toHaveTextContent(
      "Not open to this device any more",
    );
    fireEvent.click(await screen.findByRole("button", { name: "Remove keeper on phone" }));
    await waitFor(() => expect(hqDeviceRevoke).toHaveBeenCalledWith("ana.2"));
    // The directory replicates: Rust's next status marks it removed.
    act(() => push({ ...paired, devices: [paired.devices[0], { ...kalypso, revoked: true }] }));
    expect(await screen.findByText("Removed")).toBeInTheDocument();
    // A device paired elsewhere appears without reopening Settings.
    act(() =>
      push({
        ...paired,
        devices: [...paired.devices, { ...kalypso, clientId: "ana.3", name: "keeper on tablet" }],
      }),
    );
    expect(await screen.findByRole("listitem", { name: "keeper on tablet" })).toBeInTheDocument();
  });

  it("asks before pairing with a scanned link's server", async () => {
    vi.mocked(hqStatusSubscribe).mockImplementation((onStatus) => {
      onStatus({ ...NO_SERVER, offer: "agents.example.org" });
      return Promise.resolve("sub-1");
    });
    vi.mocked(hqPairOfferAccept).mockResolvedValue({ ...NO_SERVER, phase: "paired" });
    render(<AgentsServerSection open />);
    const offer = await screen.findByRole("group", { name: OFFER_PAIR });
    expect(offer).toHaveTextContent("agents.example.org");
    fireEvent.click(within(offer).getByRole("button", { name: OFFER_PAIR }));
    await waitFor(() => expect(hqPairOfferAccept).toHaveBeenCalledTimes(1));
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

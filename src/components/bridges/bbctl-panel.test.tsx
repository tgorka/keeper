import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BbctlPanel } from "@/components/bridges/bbctl-panel";
import type { BbctlAvailabilityVm } from "@/lib/ipc/client";
import { bbctlStore } from "@/lib/stores/bbctl";

// Stub the run Sheet so the panel test stays focused on the branch logic (available
// picker vs. unavailable guided install) without radix portal / stream machinery.
vi.mock("@/components/bridges/bbctl-run-sheet", () => ({
  BbctlRunSheet: ({ networkName }: { networkName: string }) => (
    <div data-testid="mock-run-sheet">run-sheet:{networkName}</div>
  ),
}));

const bbctlAvailabilityMock = vi.fn<() => Promise<BbctlAvailabilityVm>>();
vi.mock("@/lib/ipc/client", () => ({
  bbctlAvailability: () => bbctlAvailabilityMock(),
}));

const ACCOUNT_ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

function availableVm(): BbctlAvailabilityVm {
  return {
    available: true,
    install: { steps: ["install bbctl"], docsUrl: "https://example.org/docs" },
    networks: [
      { networkId: "signal", name: "Signal", bbctlName: "sh-signal" },
      { networkId: "whatsapp", name: "WhatsApp", bbctlName: "sh-whatsapp" },
    ],
  };
}

function unavailableVm(): BbctlAvailabilityVm {
  return {
    available: false,
    install: {
      // Deliberately repeated prose to prove index-keying (no duplicate-key warning).
      steps: ["Install bbctl", "Install bbctl", "Run bbctl login"],
      docsUrl: "https://example.org/self-host",
    },
    networks: [],
  };
}

beforeEach(() => {
  bbctlAvailabilityMock.mockReset();
  bbctlStore.getState().close();
});

afterEach(() => {
  bbctlStore.getState().close();
});

describe("BbctlPanel", () => {
  it("shows a checking state before availability resolves", () => {
    bbctlAvailabilityMock.mockReturnValue(new Promise(() => {}));
    render(<BbctlPanel accountId={ACCOUNT_ID} onBridgeAdded={vi.fn()} />);
    expect(screen.getByText("Checking for bbctl…")).toBeInTheDocument();
  });

  it("available: renders the network picker and a Run button", async () => {
    bbctlAvailabilityMock.mockResolvedValue(availableVm());
    render(<BbctlPanel accountId={ACCOUNT_ID} onBridgeAdded={vi.fn()} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeInTheDocument());
    // The trigger surfaces the selected network name.
    expect(screen.getByLabelText("Network to run")).toBeInTheDocument();
  });

  it("Run opens the run sheet for the selected network", async () => {
    bbctlAvailabilityMock.mockResolvedValue(availableVm());
    render(<BbctlPanel accountId={ACCOUNT_ID} onBridgeAdded={vi.fn()} />);
    const run = await screen.findByRole("button", { name: "Run" });
    run.click();
    await waitFor(() => expect(screen.getByTestId("mock-run-sheet")).toBeInTheDocument());
    // Defaulted to the first network.
    expect(screen.getByText("run-sheet:Signal")).toBeInTheDocument();
    const state = bbctlStore.getState();
    expect(state.isOpen).toBe(true);
    expect(state.accountId).toBe(ACCOUNT_ID);
    expect(state.selectedNetworkId).toBe("signal");
  });

  it("unavailable: renders the guided-install steps (index-keyed) and docs link", async () => {
    bbctlAvailabilityMock.mockResolvedValue(unavailableVm());
    render(<BbctlPanel accountId={ACCOUNT_ID} onBridgeAdded={vi.fn()} />);
    await waitFor(() => expect(screen.getByText(/bbctl isn't installed/)).toBeInTheDocument());
    // The repeated step renders twice without a React key collision.
    expect(screen.getAllByText("Install bbctl")).toHaveLength(2);
    const docs = screen.getByRole("link", { name: "Beeper self-host docs" });
    expect(docs).toHaveAttribute("href", "https://example.org/self-host");
  });

  it("closes the store when the open selected network is absent from availability", async () => {
    // The store is opened for a network the availability set does NOT contain.
    bbctlStore.getState().open(ACCOUNT_ID, "telegram");
    bbctlAvailabilityMock.mockResolvedValue(availableVm());
    render(<BbctlPanel accountId={ACCOUNT_ID} onBridgeAdded={vi.fn()} />);
    await waitFor(() => expect(bbctlStore.getState().isOpen).toBe(false));
  });

  it("surfaces an honest error when availability rejects", async () => {
    bbctlAvailabilityMock.mockRejectedValue({ message: "internal boom" });
    render(<BbctlPanel accountId={ACCOUNT_ID} onBridgeAdded={vi.fn()} />);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("internal boom"));
  });
});

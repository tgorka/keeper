import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NetworkVm } from "@/lib/ipc/client";

// The group pokes the Rust filter via the typed IPC wrapper; mock it so tests
// assert the command without a live backend.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    setNetworkFilter: vi.fn(async () => {}),
    // Per-Network mute (Story 10.2): default not-muted, capture the set call.
    networkMuteGet: vi.fn(async () => false),
    networkMuteSet: vi.fn(async () => {}),
  };
});

import { NetworksGroup } from "@/components/layout/networks-group";
import { networkMuteGet, networkMuteSet, setNetworkFilter } from "@/lib/ipc/client";
import { networksStore } from "@/lib/stores/networks";

function network(name: string): NetworkVm {
  return { name };
}

afterEach(() => {
  vi.clearAllMocks();
  networksStore.getState().clear();
});

describe("NetworksGroup", () => {
  it("renders nothing when there are no networks", () => {
    const { container } = render(<NetworksGroup />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("region", { name: "Networks" })).not.toBeInTheDocument();
  });

  it("renders a labeled row per network", () => {
    networksStore.getState().applySnapshot({ networks: [network("Telegram"), network("Signal")] });
    render(<NetworksGroup />);
    expect(screen.getByText("Networks")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Telegram/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Signal/ })).toBeInTheDocument();
  });

  it("selecting a network records the selection and pokes the Rust filter", async () => {
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    render(<NetworksGroup />);

    fireEvent.click(screen.getByRole("button", { name: /Telegram/ }));

    expect(networksStore.getState().activeNetwork).toBe("Telegram");
    await waitFor(() => {
      expect(setNetworkFilter).toHaveBeenCalledWith("Telegram");
    });
    expect(screen.getByRole("button", { name: /Telegram/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
  });

  it("clicking the active network again clears the filter (toggle)", async () => {
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    networksStore.getState().setActiveNetwork("Telegram");
    render(<NetworksGroup />);

    fireEvent.click(screen.getByRole("button", { name: /Telegram/ }));

    expect(networksStore.getState().activeNetwork).toBeNull();
    await waitFor(() => {
      expect(setNetworkFilter).toHaveBeenCalledWith(null);
    });
  });

  // ── Per-Network mute (Story 10.2) ──────────────────────────────────────────
  it("reads the muted state for each network on render", async () => {
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    render(<NetworksGroup />);
    await waitFor(() => {
      expect(networkMuteGet).toHaveBeenCalledWith("Telegram");
    });
  });

  it("the context menu mutes an unmuted network via networkMuteSet", async () => {
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    render(<NetworksGroup />);
    // Default not-muted → the menu offers "Mute Network".
    fireEvent.contextMenu(screen.getByRole("button", { name: /Telegram/ }));
    fireEvent.click(await screen.findByText("Mute Network"));
    await waitFor(() => {
      expect(networkMuteSet).toHaveBeenCalledWith("Telegram", true);
    });
  });

  it("shows the mute glyph and offers Unmute when a network is muted", async () => {
    (networkMuteGet as ReturnType<typeof vi.fn>).mockResolvedValueOnce(true);
    networksStore.getState().applySnapshot({ networks: [network("Telegram")] });
    render(<NetworksGroup />);
    await waitFor(() => {
      expect(screen.getByTestId("network-mute-glyph")).toBeInTheDocument();
    });
    fireEvent.contextMenu(screen.getByRole("button", { name: /Telegram/ }));
    fireEvent.click(await screen.findByText("Unmute Network"));
    await waitFor(() => {
      expect(networkMuteSet).toHaveBeenCalledWith("Telegram", false);
    });
  });
});

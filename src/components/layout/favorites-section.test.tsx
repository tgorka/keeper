import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { InboxRoomVm } from "@/lib/ipc/client";

// The section round-trips unfavorite + collapse persistence through the typed IPC
// client wrappers; mock them so tests assert the command without a live backend.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    unfavoriteRoom: vi.fn(async () => {}),
    setFavoritesCollapsed: vi.fn(async () => {}),
  };
});

import { FavoritesSection } from "@/components/layout/favorites-section";
import { setFavoritesCollapsed, unfavoriteRoom } from "@/lib/ipc/client";
import { favoritesUiStore } from "@/lib/stores/favorites-ui";

function room(id: string, overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: "acctA",
    hueIndex: 0,
    roomId: id,
    displayName: id,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: true,
    network: null,
    networkId: null,
    ...overrides,
  };
}

afterEach(() => {
  vi.clearAllMocks();
  favoritesUiStore.getState().setCollapsed(false);
});

describe("FavoritesSection", () => {
  it("renders nothing when there are no favourites", () => {
    const { container } = render(<FavoritesSection favorites={[]} />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("region", { name: "Favorites" })).not.toBeInTheDocument();
  });

  it("renders favourited rooms in the given (stream) order", () => {
    render(
      <FavoritesSection
        favorites={[room("!b", { displayName: "Bravo" }), room("!a", { displayName: "Alpha" })]}
      />,
    );
    const rows = screen.getAllByRole("button", { name: /Favorite conversation with/ });
    // Order is exactly the array order (Rust-authoritative recency) — no re-sort.
    expect(rows[0]).toHaveAccessibleName("Favorite conversation with Bravo");
    expect(rows[1]).toHaveAccessibleName("Favorite conversation with Alpha");
    // The uppercase label renders.
    expect(screen.getByText("Favorites")).toBeInTheDocument();
  });

  it("selects the room on click", () => {
    const onSelect = vi.fn();
    render(
      <FavoritesSection favorites={[room("!a", { displayName: "Alpha" })]} onSelect={onSelect} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Favorite conversation with Alpha" }));
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctA", roomId: "!a" });
  });

  it("collapse hides the list and persists the collapse state", async () => {
    render(<FavoritesSection favorites={[room("!a", { displayName: "Alpha" })]} />);
    // Expanded by default: the row and its list are present.
    expect(screen.getByLabelText("Favorite conversations")).toBeInTheDocument();
    const toggle = screen.getByRole("button", { name: "Favorites" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(toggle);

    // The list is hidden; the header + toggle remain.
    expect(screen.queryByLabelText("Favorite conversations")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Favorites" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(setFavoritesCollapsed).toHaveBeenCalledWith(true);
  });

  it("invokes unfavoriteRoom from the per-row context menu", async () => {
    render(<FavoritesSection favorites={[room("!a", { displayName: "Alpha" })]} />);
    fireEvent.contextMenu(screen.getByRole("button", { name: "Favorite conversation with Alpha" }));
    const unfavorite = await screen.findByText("Unfavorite");
    fireEvent.click(unfavorite);
    expect(unfavoriteRoom).toHaveBeenCalledWith("acctA", "!a");
  });
});

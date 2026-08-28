import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PhoneInboxHeader } from "@/components/layout/phone-inbox-header";
import type { AccountVm, BridgeHealthSnapshot } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { bridgeHealthStore } from "@/lib/stores/bridge-health";
import { draftsStore } from "@/lib/stores/drafts";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { newChatStore } from "@/lib/stores/new-chat";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { searchSurfaceStore } from "@/lib/stores/search-surface";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 3,
  provider: "password",
};

/** A one-session health snapshot at the given health, so the worst-state roll-up matches. */
function healthSnapshot(health: "healthy" | "degraded" | "disconnected"): BridgeHealthSnapshot {
  return {
    sessions: [
      {
        accountId: account.accountId,
        networkId: "whatsapp",
        networkName: "WhatsApp",
        health,
        lastCheckedMs: 0,
        detail: null,
      },
    ],
  };
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  bridgeHealthStore.getState().reset();
  draftsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  leadingDrawerStore.getState().close();
  searchSurfaceStore.setState({ isOpen: false, scope: "chats", chatLock: null });
  newChatStore.setState({ isOpen: false });
});

afterEach(() => {
  accountsStore.getState().clear();
  bridgeHealthStore.getState().reset();
  draftsStore.getState().clear();
});

describe("PhoneInboxHeader", () => {
  it("renders exactly one 52px header with the leading drawer trigger and trailing cluster", () => {
    render(<PhoneInboxHeader />);
    expect(screen.getAllByRole("banner")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Open navigation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New chat" })).toBeInTheDocument();
    // No bottom tab bar element and no navigation role in the header.
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
  });

  it("pads the bar by the safe-area top inset above the 52px content (Story 13.5)", () => {
    render(<PhoneInboxHeader />);
    // jsdom cannot evaluate env(), so assert the class plumbing: the notch band
    // pads above the content row and the bar's height grows by the same inset.
    const header = screen.getByRole("banner");
    expect(header.className).toContain("pt-[var(--safe-top)]");
    expect(header.className).toContain("h-[calc(var(--safe-top)+var(--phone-header))]");
  });

  it("opens the leading drawer when the avatar button is tapped", () => {
    render(<PhoneInboxHeader />);
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
  });

  it("shows a bridge-health lamp only when the worst state is unhealthy", () => {
    // healthy → quiet (no lamp).
    bridgeHealthStore.getState().applySnapshot(healthSnapshot("healthy"));
    const { rerender } = render(<PhoneInboxHeader />);
    expect(document.querySelector('[data-slot="bridge-health-dot"]')).toBeNull();

    // The two unhealthy states must differ by SHAPE, not only by amber-versus-
    // red, and the word must reach the accessibility tree. It reaches it
    // through the button's name rather than the lamp's own text, because an
    // explicit `aria-label` on the button replaces whatever is inside it — the
    // trap the old `AvatarBadge` fell into by putting `aria-label` on a
    // role-less span, where most screen readers never announced it at all.
    bridgeHealthStore.getState().applySnapshot(healthSnapshot("degraded"));
    rerender(<PhoneInboxHeader />);
    expect(document.querySelector('[data-slot="bridge-health-dot"]')).toHaveAttribute(
      "data-state",
      "working",
    );
    expect(
      screen.getByRole("button", { name: "Open navigation, Action needed" }),
    ).toBeInTheDocument();

    bridgeHealthStore.getState().applySnapshot(healthSnapshot("disconnected"));
    rerender(<PhoneInboxHeader />);
    expect(document.querySelector('[data-slot="bridge-health-dot"]')).toHaveAttribute(
      "data-state",
      "fault",
    );
    expect(
      screen.getByRole("button", { name: "Open navigation, Disconnected" }),
    ).toBeInTheDocument();
  });

  it("keeps the status cluster quiet when nothing is monitored", () => {
    render(<PhoneInboxHeader />);
    expect(document.querySelector('[data-slot="bridge-health-dot"]')).toBeNull();
  });

  it("shows the amber Approval chip only when the pending count > 0 and deep-links on tap", () => {
    const { rerender } = render(<PhoneInboxHeader />);
    // Count 0 → no chip.
    expect(screen.queryByRole("button", { name: /Approvals/ })).not.toBeInTheDocument();

    draftsStore.getState().mark(account.accountId, "!r1", true);
    draftsStore.getState().mark(account.accountId, "!r2", true);
    rerender(<PhoneInboxHeader />);
    const chip = screen.getByRole("button", { name: "Approvals, 2 pending" });
    expect(chip).toHaveTextContent("2");

    fireEvent.click(chip);
    expect(primaryViewStore.getState().view).toBe("approval");
  });

  it("renders the filtered account's avatar cue when a filter is active, else a neutral avatar", () => {
    accountsStore.getState().addAccount(account);
    // Unfiltered: the neutral all-accounts avatar (no account initial).
    const { rerender } = render(<PhoneInboxHeader />);
    expect(screen.queryByText("A")).not.toBeInTheDocument();

    // Filtered: the account's hue-initials avatar cue.
    accountsStore.setState({ filterAccountId: account.accountId });
    rerender(<PhoneInboxHeader />);
    expect(screen.getByText("A")).toBeInTheDocument();
  });

  it("opens the search surface from the magnifier and the new-chat store from compose", () => {
    render(<PhoneInboxHeader />);
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(searchSurfaceStore.getState().isOpen).toBe(true);
    // The magnifier opens the merged surface in its default Chats scope — not the
    // desktop command palette (Story 13.4 repoint).
    expect(searchSurfaceStore.getState().scope).toBe("chats");

    fireEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(newChatStore.getState().isOpen).toBe(true);
  });
});

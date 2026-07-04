import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, Provider } from "@/lib/ipc/client";

// Mock the sign-out hook so the footer never touches Tauri; the handler is a spy
// that records the account id it was called with.
const signOutHandler = vi.fn();
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => signOutHandler,
}));

// The Settings dialog loads the encryption posture on open; stub just that
// wrapper so opening it from the row menu never reaches Tauri.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
  };
});

import { AccountFooter } from "@/components/layout/account-footer";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { addAccountStore } from "@/lib/stores/add-account";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { settingsUiStore } from "@/lib/stores/settings-ui";

function account(id: string, userId: string, hue = 0, provider: Provider = "password"): AccountVm {
  return {
    accountId: id,
    userId,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: hue,
    provider,
  };
}

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV", "@alice:example.org", 0);
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ", "@bob:example.org", 1);

const beeper: AccountVm = {
  accountId: "01CX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@carol:beeper.com",
  homeserverUrl: "https://matrix.beeper.com/",
  hueIndex: 2,
  provider: "beeper",
};

function renderFooter(collapsed = false) {
  return render(
    <TooltipProvider>
      <AccountFooter collapsed={collapsed} />
    </TooltipProvider>,
  );
}

/** Open the per-account dropdown menu and return the menu element. Radix opens
 * its menu on pointer-down (not `click` in jsdom); keyboard activation is the
 * reliable path under Testing Library. */
async function openRowMenu(userId: string) {
  const trigger = screen.getByRole("button", { name: `Account menu for ${userId}` });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  accountStatusStore.getState().reset();
  encryptionStatusStore.getState().reset();
  settingsUiStore.getState().setSettingsOpen(false);
  addAccountStore.getState().closeAddAccount();
  signOutHandler.mockReset();
  signOutHandler.mockResolvedValue(undefined);
});

afterEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  accountStatusStore.getState().reset();
  addAccountStore.getState().closeAddAccount();
});

describe("AccountFooter", () => {
  it("shows only the Add Account button when there are no accounts", () => {
    renderFooter();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
    expect(screen.queryByText(alice.userId)).not.toBeInTheDocument();
  });

  it("lists every signed-in account with a switcher row, homeserver and menu", () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();
    expect(screen.getByText(alice.userId)).toBeInTheDocument();
    expect(screen.getByText(bob.userId)).toBeInTheDocument();
    // The homeserver host is rendered on each row.
    expect(screen.getAllByText("matrix.example.org")).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: `Account menu for ${alice.userId}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Account menu for ${bob.userId}` }),
    ).toBeInTheDocument();
  });

  it("the Add Account button opens the add-account overlay and is never count-gated", () => {
    // No accounts at all: Add Account is still present.
    renderFooter();
    expect(addAccountStore.getState().open).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Add account" }));
    expect(addAccountStore.getState().open).toBe(true);
  });

  it("shows a syncing spinner when no status batch has arrived yet", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    expect(screen.getByLabelText("Syncing")).toBeInTheDocument();
  });

  it("shows the synced glyph when the account is online", () => {
    accountsStore.getState().hydrateAll([alice]);
    accountStatusStore.getState().setStatus(alice.accountId, "online");
    renderFooter();
    expect(screen.getByLabelText("Synced")).toBeInTheDocument();
  });

  it("shows the offline glyph when the account is offline", () => {
    accountsStore.getState().hydrateAll([alice]);
    accountStatusStore.getState().setStatus(alice.accountId, "offline");
    renderFooter();
    expect(screen.getByLabelText("Offline")).toBeInTheDocument();
  });

  it("clicking an account row filters the inbox to it; clicking again clears it", () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();
    const row = screen.getByRole("button", { name: `Filter inbox to ${alice.userId}` });
    fireEvent.click(row);
    expect(accountsStore.getState().filterAccountId).toBe(alice.accountId);
    // The active row now offers to clear the filter.
    fireEvent.click(screen.getByRole("button", { name: `Clear filter for ${alice.userId}` }));
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });

  it("the row menu opens the keep-archive sign-out dialog and confirming signs out", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();

    const menu = await openRowMenu(bob.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out…" }));

    const dialog = await screen.findByRole("alertdialog");
    // The dialog title frames the keep-local-archive default (UX-DR20).
    expect(
      within(dialog).getByRole("heading", { name: "Sign out, keep local archive" }),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out, keep local archive" }));

    await waitFor(() => {
      expect(signOutHandler).toHaveBeenCalledWith(bob.accountId);
    });
  });

  it("cancelling the sign-out dialog does not sign out and closes it", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const menu = await openRowMenu(alice.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out…" }));

    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(signOutHandler).not.toHaveBeenCalled();
  });

  it("the row menu opens the Settings dialog", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const menu = await openRowMenu(alice.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Settings" }));

    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Archive & Storage")).toBeInTheDocument();
  });

  it("offers a Beeper coverage menu item that opens the disclosure for a Beeper account", async () => {
    accountsStore.getState().hydrateAll([beeper]);
    renderFooter();

    const menu = await openRowMenu(beeper.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Beeper coverage" }));

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(
        "WhatsApp connected in the official Beeper app will not appear here.",
      ),
    ).toBeInTheDocument();
  });

  it("does not offer a Beeper coverage item for a non-Beeper account", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const menu = await openRowMenu(alice.userId);
    expect(
      within(menu).queryByRole("menuitem", { name: "Beeper coverage" }),
    ).not.toBeInTheDocument();
  });

  it("renders avatar-only rows with a menu when collapsed", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter(true);
    expect(screen.queryByText(alice.userId)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Filter inbox to ${alice.userId}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Account menu for ${alice.userId}` }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
  });
});

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mock the sign-out hook so the footer never touches Tauri; the handler is a spy
// that records the account id it was called with.
const signOutHandler = vi.fn();
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => signOutHandler,
}));

import { AccountFooter } from "@/components/layout/account-footer";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountsStore } from "@/lib/stores/accounts";
import { addAccountStore } from "@/lib/stores/add-account";

function account(id: string, userId: string, hue = 0): AccountVm {
  return { accountId: id, userId, homeserverUrl: "https://matrix.example.org/", hueIndex: hue };
}

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV", "@alice:example.org", 0);
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ", "@bob:example.org", 1);

/** A Beeper account is identified solely by its `matrix.beeper.com` homeserver. */
const beeper: AccountVm = {
  accountId: "01CX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@carol:beeper.com",
  homeserverUrl: "https://matrix.beeper.com/",
  hueIndex: 2,
};

function renderFooter(collapsed = false) {
  return render(
    <TooltipProvider>
      <AccountFooter collapsed={collapsed} />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  accountsStore.getState().clear();
  addAccountStore.getState().closeAddAccount();
  signOutHandler.mockReset();
  signOutHandler.mockResolvedValue(undefined);
});

afterEach(() => {
  accountsStore.getState().clear();
  addAccountStore.getState().closeAddAccount();
});

describe("AccountFooter", () => {
  it("shows only the Add Account button when there are no accounts", () => {
    renderFooter();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
    expect(screen.queryByText(alice.userId)).not.toBeInTheDocument();
  });

  it("lists every signed-in account with its own sign-out control", () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();
    expect(screen.getByText(alice.userId)).toBeInTheDocument();
    expect(screen.getByText(bob.userId)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Sign out ${alice.userId}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Sign out ${bob.userId}` })).toBeInTheDocument();
  });

  it("the Add Account button opens the add-account overlay", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    expect(addAccountStore.getState().open).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Add account" }));
    expect(addAccountStore.getState().open).toBe(true);
  });

  it("confirming the dialog signs out exactly that account", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();

    fireEvent.click(screen.getByRole("button", { name: `Sign out ${bob.userId}` }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out" }));

    await waitFor(() => {
      expect(signOutHandler).toHaveBeenCalledWith(bob.accountId);
    });
  });

  it("cancelling the dialog does not sign out and closes it", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    fireEvent.click(screen.getByRole("button", { name: `Sign out ${alice.userId}` }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(signOutHandler).not.toHaveBeenCalled();
  });

  it("renders icon-only affordances when collapsed", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter(true);
    expect(screen.queryByText(alice.userId)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Sign out ${alice.userId}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
  });

  it("exposes a Beeper coverage control that opens the disclosure for a Beeper account", async () => {
    accountsStore.getState().hydrateAll([beeper]);
    renderFooter();
    const control = screen.getByRole("button", { name: `Beeper coverage for ${beeper.userId}` });
    fireEvent.click(control);
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(
        "WhatsApp connected in the official Beeper app will not appear here.",
      ),
    ).toBeInTheDocument();
  });

  it("does not render a coverage control for a non-Beeper account", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    expect(
      screen.queryByRole("button", { name: `Beeper coverage for ${alice.userId}` }),
    ).not.toBeInTheDocument();
  });
});

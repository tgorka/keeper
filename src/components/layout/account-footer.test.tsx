import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mock the sign-out hook so the footer never touches Tauri; the handler is a spy.
const signOutHandler = vi.fn();
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => signOutHandler,
}));

import { AccountFooter } from "@/components/layout/account-footer";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountsStore } from "@/lib/stores/accounts";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
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
  accountsStore.getState().setCurrentAccount(account);
  signOutHandler.mockReset();
  signOutHandler.mockResolvedValue(undefined);
});

afterEach(() => {
  accountsStore.getState().clear();
});

describe("AccountFooter", () => {
  it("renders nothing when signed out", () => {
    accountsStore.getState().clear();
    const { container } = renderFooter();
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the signed-in user id and a sign-out control", () => {
    renderFooter();
    expect(screen.getByText(account.userId)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Sign out ${account.userId}` })).toBeInTheDocument();
  });

  it("confirming the dialog invokes the sign-out handler", async () => {
    renderFooter();

    fireEvent.click(screen.getByRole("button", { name: `Sign out ${account.userId}` }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out" }));

    await waitFor(() => {
      expect(signOutHandler).toHaveBeenCalledTimes(1);
    });
  });

  it("cancelling the dialog does not invoke the handler and closes it", async () => {
    renderFooter();

    fireEvent.click(screen.getByRole("button", { name: `Sign out ${account.userId}` }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(signOutHandler).not.toHaveBeenCalled();
  });

  it("renders an icon-only sign-out affordance when collapsed", () => {
    renderFooter(true);
    // No visible user id text in the collapsed rail.
    expect(screen.queryByText(account.userId)).not.toBeInTheDocument();
    // The labelled icon control is still present and opens the same dialog.
    expect(screen.getByRole("button", { name: `Sign out ${account.userId}` })).toBeInTheDocument();
  });
});

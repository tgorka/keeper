import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// The account footer renders `useSignOut`, which imports the IPC client; mock the
// hook so mounting the sidebar never reaches Tauri.
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

// The Settings dialog loads the encryption posture on open; stub just that
// wrapper so mounting the sidebar never reaches Tauri.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
  };
});

import { SidebarPane } from "@/components/layout/sidebar-pane";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { primaryViewStore } from "@/lib/stores/primary-view";

const OFFLINE_TEXT = "Offline — showing your local archive. Messages queue until you're back.";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

const other: AccountVm = {
  accountId: "01BX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@bob:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 1,
  provider: "password",
};

function renderSidebar(collapsed = false) {
  return render(
    <TooltipProvider>
      <SidebarPane collapsed={collapsed} />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  accountStatusStore.getState().reset();
  accountsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
});

afterEach(() => {
  accountStatusStore.getState().reset();
  accountsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
});

describe("SidebarPane offline pill", () => {
  it("hides the pill while online (the default)", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "online");
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("hides the pill while an account is pending (no false flash)", () => {
    accountsStore.getState().addAccount(account);
    // No status batch yet → pending, must not show the pill.
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("shows the persistent pill with the exact text when every account is offline", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    renderSidebar();
    const pill = screen.getByRole("status");
    expect(pill).toBeInTheDocument();
    expect(screen.getByText(OFFLINE_TEXT)).toBeInTheDocument();
    // Amber `held` tokens.
    expect(pill).toHaveClass("text-held");
    // Rendered in the footer region (the wrapper carries `mt-auto`; the pill
    // itself keeps the `border-t` divider).
    expect(pill).toHaveClass("border-t");
    expect(pill.parentElement).toHaveClass("mt-auto");
  });

  it("hides the pill when one account is offline and another is online (mixed)", () => {
    accountsStore.getState().hydrateAll([account, other]);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    accountStatusStore.getState().setStatus(other.accountId, "online");
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("hides again when connectivity returns", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    const { rerender } = renderSidebar();
    expect(screen.getByRole("status")).toBeInTheDocument();

    accountStatusStore.getState().setStatus(account.accountId, "online");
    rerender(
      <TooltipProvider>
        <SidebarPane collapsed={false} />
      </TooltipProvider>,
    );
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("announces the offline status via an accessible label when collapsed", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    renderSidebar(true);
    expect(screen.getByRole("status", { name: OFFLINE_TEXT })).toBeInTheDocument();
  });
});

describe("SidebarPane account footer", () => {
  it("shows the account switcher row with the signed-in user id when signed in", () => {
    accountsStore.getState().addAccount(account);
    renderSidebar();
    expect(screen.getByText(account.userId)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Account menu for ${account.userId}` }),
    ).toBeInTheDocument();
  });

  it("shows no account row when signed out", () => {
    renderSidebar();
    expect(screen.queryByText(account.userId)).not.toBeInTheDocument();
  });
});

describe("SidebarPane primary view", () => {
  it("switches the primary view to archive when Archive is clicked", () => {
    renderSidebar();
    expect(primaryViewStore.getState().view).toBe("inbox");

    fireEvent.click(screen.getByRole("button", { name: "Archive" }));

    expect(primaryViewStore.getState().view).toBe("archive");
    // The Archive entry reflects the active view.
    expect(screen.getByRole("button", { name: "Archive" })).toHaveAttribute("aria-current", "page");
  });

  it("switches back to the inbox when Chats is clicked", () => {
    primaryViewStore.getState().setView("archive");
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Chats" }));

    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(screen.getByRole("button", { name: "Chats" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Archive" })).not.toHaveAttribute("aria-current");
  });
});

describe("SidebarPane settings", () => {
  it("opens the Settings dialog when the Settings button is clicked", async () => {
    renderSidebar();
    // The dialog is closed initially.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeInTheDocument();
    expect(screen.getByText("Archive & Storage")).toBeInTheDocument();
  });
});

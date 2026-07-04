import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// The account footer renders `useSignOut`, which imports the IPC client; mock the
// hook so mounting the sidebar never reaches Tauri.
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

import { SidebarPane } from "@/components/layout/sidebar-pane";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountsStore } from "@/lib/stores/accounts";
import { connectionStore } from "@/lib/stores/connection";

const OFFLINE_TEXT = "Offline — showing your local archive. Messages queue until you're back.";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
};

function renderSidebar(collapsed = false) {
  return render(
    <TooltipProvider>
      <SidebarPane collapsed={collapsed} />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  connectionStore.getState().reset();
  accountsStore.getState().clear();
});

afterEach(() => {
  connectionStore.getState().reset();
  accountsStore.getState().clear();
});

describe("SidebarPane offline pill", () => {
  it("hides the pill while online (the default)", () => {
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows the persistent pill with the exact text while offline", () => {
    connectionStore.getState().applyBatch({ status: "offline" });
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

  it("hides again when connectivity returns", () => {
    connectionStore.getState().applyBatch({ status: "offline" });
    const { rerender } = renderSidebar();
    expect(screen.getByRole("status")).toBeInTheDocument();

    connectionStore.getState().applyBatch({ status: "online" });
    rerender(
      <TooltipProvider>
        <SidebarPane collapsed={false} />
      </TooltipProvider>,
    );
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("announces the offline status via an accessible label when collapsed", () => {
    connectionStore.getState().applyBatch({ status: "offline" });
    renderSidebar(true);
    expect(screen.getByRole("status", { name: OFFLINE_TEXT })).toBeInTheDocument();
  });
});

describe("SidebarPane account footer", () => {
  it("shows the account row with the signed-in user id when signed in", () => {
    accountsStore.getState().addAccount(account);
    renderSidebar();
    expect(screen.getByText(account.userId)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Sign out ${account.userId}` })).toBeInTheDocument();
  });

  it("shows no account row when signed out", () => {
    renderSidebar();
    expect(screen.queryByText(account.userId)).not.toBeInTheDocument();
  });
});

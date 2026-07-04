import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { SidebarPane } from "@/components/layout/sidebar-pane";
import { TooltipProvider } from "@/components/ui/tooltip";
import { connectionStore } from "@/lib/stores/connection";

const OFFLINE_TEXT = "Offline — showing your local archive. Messages queue until you're back.";

function renderSidebar(collapsed = false) {
  return render(
    <TooltipProvider>
      <SidebarPane collapsed={collapsed} />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  connectionStore.getState().reset();
});

afterEach(() => {
  connectionStore.getState().reset();
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
    // Rendered in the footer (mt-auto + border-t).
    expect(pill).toHaveClass("mt-auto");
    expect(pill).toHaveClass("border-t");
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

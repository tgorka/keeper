import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SpaceVm } from "@/lib/ipc/client";

// The group pokes the Rust filter via the typed IPC wrapper; mock it so tests
// assert the command without a live backend.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    setSpaceFilter: vi.fn(async () => {}),
  };
});

import { SpacesGroup } from "@/components/layout/spaces-group";
import { setSpaceFilter } from "@/lib/ipc/client";
import {
  hydrateSidebarFold,
  resetSidebarFoldForTest,
  SIDEBAR_FOLD_COOKIE,
} from "@/lib/stores/sidebar-fold";
import { spacesStore } from "@/lib/stores/spaces";

function space(spaceId: string, name: string, accountId = "acctA"): SpaceVm {
  return { accountId, spaceId, name, avatarUrl: null };
}

afterEach(() => {
  vi.clearAllMocks();
  spacesStore.getState().clear();
  resetSidebarFoldForTest();
});

describe("SpacesGroup", () => {
  it("renders nothing when there are no spaces", () => {
    const { container } = render(<SpacesGroup />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("region", { name: "Spaces" })).not.toBeInTheDocument();
  });

  it("renders a labeled row per space", () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a", "Design"), space("!b", "Ops")] });
    render(<SpacesGroup />);
    expect(screen.getByText("Spaces")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Design/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Ops/ })).toBeInTheDocument();
  });

  it("selecting a space records the selection and pokes the Rust filter", async () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a", "Design")] });
    render(<SpacesGroup />);

    fireEvent.click(screen.getByRole("button", { name: /Design/ }));

    expect(spacesStore.getState().activeSpace).toEqual({ accountId: "acctA", spaceId: "!a" });
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith("acctA", "!a");
    });
    expect(screen.getByRole("button", { name: /Design/ })).toHaveAttribute("aria-current", "true");
  });

  it("clicking the active space again clears the filter (toggle)", async () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a", "Design")] });
    spacesStore.getState().setActiveSpace({ accountId: "acctA", spaceId: "!a" });
    render(<SpacesGroup />);

    fireEvent.click(screen.getByRole("button", { name: /Design/ }));

    expect(spacesStore.getState().activeSpace).toBeNull();
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(null, null);
    });
  });

  it("keeps each row's name and its filter on the folded rail", () => {
    // Two spaces, not one: a rail that rendered only the first row would pass
    // every single-space fixture, and the reported failure was the group
    // vanishing wholesale.
    spacesStore.getState().applySnapshot({ spaces: [space("!a", "Design"), space("!b", "Ops")] });
    render(<SpacesGroup collapsed />);

    // Named, but not by visible text — the rail has none. That is exactly the
    // difference between a folded menu and a strip of unlabelled glyphs.
    expect(screen.getByRole("button", { name: "Design" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ops" })).toBeInTheDocument();
    expect(screen.queryByText("Design")).not.toBeInTheDocument();

    // And pressing one still filters: a folded row is the same control, drawn
    // narrower, not a decoration.
    fireEvent.click(screen.getByRole("button", { name: "Ops" }));
    expect(spacesStore.getState().activeSpace).toEqual({ accountId: "acctA", spaceId: "!b" });
    expect(setSpaceFilter).toHaveBeenCalledWith("acctA", "!b");
  });

  it("folds its rows away and keeps the control that brings them back", () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a", "Design"), space("!b", "Ops")] });
    render(<SpacesGroup />);

    const fold = screen.getByRole("button", { name: "Collapse Spaces" });
    expect(fold).toHaveAttribute("aria-expanded", "true");
    expect(fold).toHaveAttribute("aria-controls", "sidebar-group-spaces");
    // …and the thing it names EXISTS. `aria-controls` pointing at nothing
    // renders exactly the same DOM as one pointing at the list — the attribute
    // is present either way and every `getByRole` query still passes — so
    // dropping the `id` off the `<ul>` would break the announced relationship
    // silently. The only witness is the target (W3Recording's shape).
    expect(document.getElementById("sidebar-group-spaces")).toContainElement(
      screen.getByRole("button", { name: "Design" }),
    );

    fireEvent.click(fold);

    expect(screen.queryByRole("button", { name: "Design" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ops" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand Spaces" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    // The section itself stays, or there is no way back into it.
    expect(screen.getByRole("region", { name: "Spaces" })).toBeInTheDocument();
  });

  it("opens folded when the last run left it folded", () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a", "Design"), space("!b", "Ops")] });
    hydrateSidebarFold(`${SIDEBAR_FOLD_COOKIE}=menu%3A0%7Cspaces%3A1%7Cnetworks%3A0`);

    render(<SpacesGroup />);

    expect(screen.getByRole("button", { name: "Expand Spaces" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Design" })).not.toBeInTheDocument();
  });
});

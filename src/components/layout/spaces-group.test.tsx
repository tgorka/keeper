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
import { spacesStore } from "@/lib/stores/spaces";

function space(spaceId: string, name: string, accountId = "acctA"): SpaceVm {
  return { accountId, spaceId, name, avatarUrl: null };
}

afterEach(() => {
  vi.clearAllMocks();
  spacesStore.getState().clear();
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
});

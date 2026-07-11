import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// The account footer renders `useSignOut`, which imports the IPC client; mock the
// hook so mounting the drawer's SidebarPane never reaches Tauri.
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

// The Settings dialog loads the encryption posture on open; stub just that
// wrapper so mounting the drawer never reaches Tauri.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
  };
});

import { LeadingDrawer } from "@/components/layout/leading-drawer";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountsStore } from "@/lib/stores/accounts";
import { bridgeHealthStore } from "@/lib/stores/bridge-health";
import { draftsStore } from "@/lib/stores/drafts";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { networksStore } from "@/lib/stores/networks";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { settingsUiStore } from "@/lib/stores/settings-ui";
import { spacesStore } from "@/lib/stores/spaces";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

function renderDrawer() {
  return render(
    <TooltipProvider>
      <LeadingDrawer />
    </TooltipProvider>,
  );
}

/** Open the drawer and wait for its content (the reused SidebarPane nav) to mount. */
async function openDrawer() {
  leadingDrawerStore.getState().open();
  await screen.findByRole("navigation", { name: "Views" });
}

/** Mock every rect at the given width so the close-swipe reads a real drag range. */
function mockRectWidth(width: number) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width,
    height: 700,
    top: 0,
    left: 0,
    right: width,
    bottom: 700,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  bridgeHealthStore.getState().reset();
  draftsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  spacesStore.getState().clear();
  networksStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  settingsUiStore.getState().setSettingsOpen(false);
  leadingDrawerStore.getState().close();
  accountsStore.getState().addAccount(account);
});

afterEach(() => {
  leadingDrawerStore.getState().close();
  vi.restoreAllMocks();
});

describe("LeadingDrawer", () => {
  it("renders nothing while closed", () => {
    renderDrawer();
    expect(screen.queryByRole("navigation", { name: "Views" })).not.toBeInTheDocument();
  });

  it("renders the reused SidebarPane verbatim (sections + account footer) when open", async () => {
    renderDrawer();
    await openDrawer();
    // The primary views, rendered from the exact SidebarPane, are present.
    expect(screen.getByRole("button", { name: /^Chats/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Approvals/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Bridges/ })).toBeInTheDocument();
    // The account footer's account switcher row is present (settings gear lives
    // in its per-row menu).
    expect(
      screen.getByRole("button", { name: `Filter inbox to ${account.userId}` }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
  });

  it("renders as a modal dialog (radix focus-trapping Sheet)", async () => {
    renderDrawer();
    await openDrawer();
    // The reused SidebarPane nav lives inside a radix Dialog (the modal Sheet).
    const dialog = screen.getByRole("dialog");
    expect(dialog).toBeInTheDocument();
    expect(dialog).toContainElement(screen.getByRole("navigation", { name: "Views" }));
  });

  it("closes when a primary view is selected", async () => {
    renderDrawer();
    await openDrawer();
    fireEvent.click(screen.getByRole("button", { name: /^Approvals/ }));
    expect(primaryViewStore.getState().view).toBe("approval");
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("closes when the account filter changes while open", async () => {
    renderDrawer();
    await openDrawer();
    fireEvent.click(screen.getByRole("button", { name: `Filter inbox to ${account.userId}` }));
    expect(accountsStore.getState().filterAccountId).toBe(account.accountId);
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("closes when the selected room changes while open", async () => {
    renderDrawer();
    await openDrawer();
    act(() => {
      roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!a:example.org" });
    });
    // The close-on-select effect runs on the store change.
    await vi.waitFor(() => {
      expect(leadingDrawerStore.getState().isOpen).toBe(false);
    });
  });

  it("does not close on the initial open (no spurious close from a pre-open change)", async () => {
    // A view change happened *before* opening: it must not close the freshly
    // opened drawer.
    primaryViewStore.getState().setView("archive");
    renderDrawer();
    await openDrawer();
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
  });

  it("closes on Escape (radix dismissal)", async () => {
    renderDrawer();
    await openDrawer();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("closes on a trailing→leading swipe past the threshold and stays open below it", async () => {
    mockRectWidth(260);
    renderDrawer();
    await openDrawer();
    const content = screen.getByTestId("leading-drawer-content");

    // Below half the width and below the flick minimum: stays open.
    fireEvent.pointerDown(content, { pointerId: 1, clientX: 200 });
    fireEvent.pointerUp(content, { pointerId: 1, clientX: 190 });
    expect(leadingDrawerStore.getState().isOpen).toBe(true);

    // A leftward drag past half the width closes.
    fireEvent.pointerDown(content, { pointerId: 2, clientX: 200 });
    fireEvent.pointerUp(content, { pointerId: 2, clientX: 20 });
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("applies the reduced-motion cut class to the drawer content", async () => {
    renderDrawer();
    await openDrawer();
    const content = screen.getByTestId("leading-drawer-content");
    expect(content.className).toContain("motion-reduce:animate-none");
    expect(content.className).toContain("motion-reduce:transition-none");
  });
});

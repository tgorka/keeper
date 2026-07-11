import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MenuSectionVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the overlay never touches Tauri. `cheatSheetSections`
// is the only backend call the overlay makes; the rest of the client is kept real via
// `importOriginal` so the AppShell gating test below can mount the full shell.
const cheatSheetSections = vi.fn();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    cheatSheetSections: () => cheatSheetSections(),
  };
});

import { CheatSheetOverlay } from "@/components/cheat-sheet/cheat-sheet-overlay";
import { AppShell } from "@/components/layout/app-shell";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { cheatSheetStore } from "@/lib/stores/cheat-sheet";

/** All seven capabilities present = the desktop tier (cheat sheet mounts). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
};

const SECTIONS: MenuSectionVm[] = [
  {
    category: "Navigation",
    items: [
      {
        id: "open-inbox",
        title: "Open Inbox",
        shortcut: "⌘1",
        toggleGroup: null,
        requiresOpenChat: false,
      },
      {
        id: "open-archive",
        title: "Open Archive",
        shortcut: "⌘2",
        toggleGroup: null,
        requiresOpenChat: false,
      },
    ],
  },
  {
    category: "Chat",
    items: [
      {
        id: "archive-chat",
        title: "Archive / Unarchive Chat",
        shortcut: "E",
        toggleGroup: "archive",
        requiresOpenChat: true,
      },
      {
        id: "mark-read",
        title: "Mark as Read / Unread",
        shortcut: "U",
        toggleGroup: "read",
        requiresOpenChat: true,
      },
    ],
  },
];

beforeEach(() => {
  cheatSheetSections.mockResolvedValue(SECTIONS);
  cheatSheetStore.setState({ isOpen: false });
});

afterEach(() => {
  cheatSheetStore.setState({ isOpen: false });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  vi.clearAllMocks();
});

async function openOverlay() {
  render(<CheatSheetOverlay />);
  act(() => {
    cheatSheetStore.setState({ isOpen: true });
  });
  await waitFor(() => expect(cheatSheetSections).toHaveBeenCalled());
}

describe("CheatSheetOverlay", () => {
  it("does not fetch or render rows while closed", () => {
    render(<CheatSheetOverlay />);
    expect(cheatSheetSections).not.toHaveBeenCalled();
    expect(screen.queryByText("Open Inbox")).not.toBeInTheDocument();
  });

  it("renders sections grouped by category with shortcut chips on open", async () => {
    await openOverlay();
    await screen.findByText("Open Inbox");
    expect(screen.getByText("Navigation")).toBeInTheDocument();
    expect(screen.getByText("Chat")).toBeInTheDocument();
    expect(screen.getByText("Open Inbox")).toBeInTheDocument();
    expect(screen.getByText("⌘1")).toBeInTheDocument();
  });

  it("shows each toggle pair as ONE collapsed row", async () => {
    await openOverlay();
    // Combined single row for the archive pair, and no separate unarchive row.
    expect(await screen.findByText("Archive / Unarchive Chat")).toBeInTheDocument();
    expect(screen.getByText("Mark as Read / Unread")).toBeInTheDocument();
    expect(screen.queryByText("Unarchive Chat")).not.toBeInTheDocument();
    expect(screen.getByText("E")).toBeInTheDocument();
  });

  it("filters rows by substring search over title/category/shortcut", async () => {
    await openOverlay();
    await screen.findByText("Open Inbox");
    const input = screen.getByPlaceholderText("Search shortcuts…");
    act(() => {
      fireEvent.change(input, { target: { value: "arch" } });
    });
    await waitFor(() => {
      // "arch" matches Open Archive (title) and Archive / Unarchive Chat (title).
      expect(screen.queryByText("Open Inbox")).not.toBeInTheDocument();
    });
    expect(screen.getByText("Open Archive")).toBeInTheDocument();
    expect(screen.getByText("Archive / Unarchive Chat")).toBeInTheDocument();
  });
});

// The cheat-sheet overlay is mounted by AppShell gated on the `nativeMenuBar`
// capability (Story 13.7). These tests assert the mount/unmount at that gate: an
// unmounted overlay simply cannot render, even when the store is opened.
describe("CheatSheetOverlay capability gating in AppShell", () => {
  it("desktop: mounts the overlay so opening the store renders it", async () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    render(<AppShell />);
    act(() => {
      cheatSheetStore.setState({ isOpen: true });
    });
    // The mounted overlay resolves its sections and renders its search box.
    expect(await screen.findByPlaceholderText("Search shortcuts…")).toBeInTheDocument();
  });

  it("iOS: the overlay is unmounted, so opening the store renders nothing", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AppShell />);
    act(() => {
      cheatSheetStore.setState({ isOpen: true });
    });
    // Unmounted by the gate: no fetch, no search box, even with the store open.
    expect(cheatSheetSections).not.toHaveBeenCalled();
    expect(screen.queryByPlaceholderText("Search shortcuts…")).not.toBeInTheDocument();
  });
});

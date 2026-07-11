import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/layout/app-shell";
import { detailStore } from "@/lib/stores/detail-ui";
import { roomsStore } from "@/lib/stores/rooms";

/**
 * Mock matchMedia so that any query with a `max-width: <bp>` matches when the
 * simulated viewport width is below that breakpoint (mirrors the
 * use-shell-layout suite). Restored after each test so the remaining tests keep
 * the desktop default from the global setup (every query `matches: false`).
 */
const originalMatchMedia = window.matchMedia;
function mockViewportWidth(width: number) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    return {
      matches: width <= maxWidth,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    };
  });
}

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  // Detail-open now lives in the shared detail store (Story 13.1); reset it so
  // one test's open panel never leaks into the next.
  detailStore.setState({ open: false });
  roomsStore.getState().selectRoom(null);
});

describe("AppShell", () => {
  it("renders the semantic landmarks", () => {
    render(<AppShell />);
    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    // With no account set, the chat list pane sits in its loading state.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
  });

  it("renders the placeholder copy without any Matrix data", () => {
    render(<AppShell />);
    // No account → the chat list is in its loading state (not the empty state).
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.getByText("Select a conversation to start reading.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Chats" })).toBeInTheDocument();
  });

  it("opens and closes the detail panel via the toggle control", () => {
    render(<AppShell />);

    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();

    const toggle = screen.getByRole("button", { name: "Toggle detail panel" });
    fireEvent.click(toggle);
    expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("drives detail-open through the lifted detail store", () => {
    render(<AppShell />);

    // The toggle mutates the shared store, not shell-local state (Story 13.1)…
    fireEvent.click(screen.getByRole("button", { name: "Toggle detail panel" }));
    expect(detailStore.getState().open).toBe(true);
    expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();

    // …and a programmatic store close reflects back into the shell.
    act(() => {
      detailStore.getState().closeDetail();
    });
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("renders the phone stack below 768 instead of the desktop frame", () => {
    mockViewportWidth(600);
    render(<AppShell />);

    // The sidebar and the desktop panes row are replaced by the single-pane
    // stack: no Views navigation, no always-mounted conversation pane…
    expect(screen.queryByRole("navigation", { name: "Views" })).not.toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
    // …just level 0 of the stack (the chat list in its loading state).
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("keeps the desktop three-pane frame at exactly 768", () => {
    mockViewportWidth(768);
    render(<AppShell />);

    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
  });
});

import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/layout/app-shell";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { detailStore } from "@/lib/stores/detail-ui";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

const beginTitleBarDrag = vi.fn();

// The band's drag path is asserted here as "the app asked for a drag"; what the
// window layer answers is `titlebar-drag`'s own suite.
vi.mock("@/lib/titlebar-drag", () => ({
  beginTitleBarDrag: () => beginTitleBarDrag(),
}));

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
  primaryViewStore.getState().setView("inbox");
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  beginTitleBarDrag.mockClear();
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

  // ── Recording view (Story 16.3) ────────────────────────────────────────────
  it("renders the Recording pane for the recording view when the capability is on", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
    primaryViewStore.getState().setView("recording");
    render(<AppShell />);

    // The Recording section shell replaces the chat-list + conversation cluster.
    expect(screen.getByRole("region", { name: "Recording" })).toBeInTheDocument();
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
  });

  it("does not render the Recording pane when the recording capability is off", () => {
    // A stale "recording" primary-view must never show the pane where recording is
    // unavailable (the flag is off) — no dead surface.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    primaryViewStore.getState().setView("recording");
    render(<AppShell />);

    expect(screen.queryByRole("region", { name: "Recording" })).not.toBeInTheDocument();
  });

  // ── Recordings browser (Story 42.3) ────────────────────────────────────────
  it("renders the Recordings browser for the recordings view when the capability is on", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
    primaryViewStore.getState().setView("recordings");
    render(<AppShell />);

    expect(screen.getByRole("region", { name: "Recordings" })).toBeInTheDocument();
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
    // The browser is a sibling of the capture surface, not a tab inside it.
    expect(screen.queryByRole("region", { name: "Recording" })).not.toBeInTheDocument();
  });

  it("does not render the Recordings browser when the recording capability is off", () => {
    // A browser over recordings this build cannot make is a puzzle: the surface
    // is ABSENT from the DOM, not empty and not disabled.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    primaryViewStore.getState().setView("recordings");
    render(<AppShell />);

    expect(screen.queryByRole("region", { name: "Recordings" })).not.toBeInTheDocument();
  });

  // ── Sync view (Story 32.5) ─────────────────────────────────────────────────
  it("renders the Sync pane for the sync view when the capability is on", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, sync: true });
    primaryViewStore.getState().setView("sync");
    render(<AppShell />);

    expect(screen.getByRole("region", { name: "Sync" })).toBeInTheDocument();
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
  });

  it("does not render the Sync pane when the sync capability is off", () => {
    // A stale "sync" primary-view must never show the pane on a machine with no
    // usable `git`, where every command behind it rejects as unsupported.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    primaryViewStore.getState().setView("sync");
    render(<AppShell />);

    expect(screen.queryByRole("region", { name: "Sync" })).not.toBeInTheDocument();
  });

  // ── Overlay-titlebar drag band (Story 34.2) ────────────────────────────────
  it("renders no drag band where the platform draws its own title bar", () => {
    // Off macOS the window controls live in a real title bar above the webview, so
    // a band there is empty space under chrome the OS already owns (AD-34-2).
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    render(<AppShell />);

    expect(document.querySelectorAll("[data-tauri-drag-region]")).toHaveLength(0);
  });

  it("paints the drag band per column so each column matches the pane beneath it", () => {
    // AD-34-3: a single full-width `bg-background` strip above a `bg-sidebar` drawer
    // is a seam in light mode and a black bar in dark — the reported "black strip".
    // Both columns carry the drag region, so the whole band moves the window.
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      overlayTitleBar: true,
    });
    render(<AppShell />);

    const columns = document.querySelectorAll("[data-tauri-drag-region]");
    expect(columns).toHaveLength(2);
    // The drawer column is exactly the drawer's width, then the rest of the row.
    expect(columns[0]).toHaveClass("bg-sidebar", "w-[260px]");
    expect(columns[1]).toHaveClass("bg-background");
  });

  it("leaves the band as the only element reserving the window-control inset", () => {
    // AD-34-2: the sidebar's second `pt-3 pl-[78px]` inset is gone, so the top of
    // the window is never paid for twice.
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      overlayTitleBar: true,
    });
    render(<AppShell />);

    expect(document.querySelectorAll(".pl-\\[78px\\]")).toHaveLength(0);
  });

  it("drops the drawer column from the band on the phone tier", () => {
    // Below 768 there is no drawer, so a `bg-sidebar` column would paint a strip
    // above content that is `bg-background` — the same seam, mirrored.
    mockViewportWidth(600);
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      overlayTitleBar: true,
    });
    render(<AppShell />);

    const columns = document.querySelectorAll("[data-tauri-drag-region]");
    expect(columns).toHaveLength(1);
    expect(columns[0]).toHaveClass("bg-background");
    expect(columns[0]).not.toHaveClass("bg-sidebar");
  });

  // ── App-driven window drag (Story 34.3) ────────────────────────────────────
  it("asks for a window drag on a primary-button mousedown on either column", () => {
    // Tauri's `data-tauri-drag-region` shim invokes `start_dragging` and drops the
    // promise, so a refused drag is silent. The app issues the call itself.
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      overlayTitleBar: true,
    });
    render(<AppShell />);
    const columns = document.querySelectorAll("[data-tauri-drag-region]");

    fireEvent.mouseDown(columns[0], { button: 0, detail: 1 });
    fireEvent.mouseDown(columns[1], { button: 0, detail: 1 });

    expect(beginTitleBarDrag).toHaveBeenCalledTimes(2);
  });

  it("leaves a mousedown that is aimed at something else alone", () => {
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      overlayTitleBar: true,
    });
    render(<AppShell />);
    const band = document.querySelectorAll("[data-tauri-drag-region]")[1];

    // A secondary or middle button is not a drag, and the second mousedown of a
    // double click belongs to macOS double-click-to-zoom, which Tauri's shim
    // completes on the following mouseup — a drag there would eat that gesture.
    fireEvent.mouseDown(band, { button: 2, detail: 1 });
    fireEvent.mouseDown(band, { button: 1, detail: 1 });
    fireEvent.mouseDown(band, { button: 0, detail: 2 });

    expect(beginTitleBarDrag).not.toHaveBeenCalled();
  });

  it("takes the gesture over from Tauri's document-level drag-region shim", () => {
    // The shim is a bubble-phase listener on `document` that invokes the same
    // command. One `start_dragging` per mousedown keeps the recorded outcome
    // attributable, and the `preventDefault` the shim spent the event on — which
    // suppresses the text cursor — happens here instead.
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      overlayTitleBar: true,
    });
    render(<AppShell />);
    const band = document.querySelectorAll("[data-tauri-drag-region]")[1];
    const shim = vi.fn();
    document.addEventListener("mousedown", shim);

    try {
      // First prove the listener is reachable at all, so "the shim never fired"
      // below cannot pass vacuously: a mousedown the band ignores still bubbles
      // to `document` exactly where the shim binds.
      fireEvent.mouseDown(band, { button: 2, detail: 1 });
      expect(shim).toHaveBeenCalledTimes(1);

      const event = new MouseEvent("mousedown", {
        button: 0,
        detail: 1,
        bubbles: true,
        cancelable: true,
      });
      fireEvent(band, event);

      expect(beginTitleBarDrag).toHaveBeenCalledTimes(1);
      expect(shim).toHaveBeenCalledTimes(1);
      expect(event.defaultPrevented).toBe(true);
    } finally {
      document.removeEventListener("mousedown", shim);
    }
  });
});

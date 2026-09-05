import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/layout/app-shell";
import { SIDEBAR_WIDTH_CLASS } from "@/components/layout/sidebar-pane";
import { COLUMN_COLLAPSE_PREFIX, COLUMN_EXPAND_PREFIX } from "@/components/layout/surface-column";
import { SURFACE_COLUMNS } from "@/lib/column-widths";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import {
  COLUMN_FOLD_COOKIE,
  columnFoldCookie,
  columnFoldStore,
  columnsUnfolded,
  resetColumnFoldForTest,
} from "@/lib/stores/column-fold";
import { detailStore } from "@/lib/stores/detail-ui";
import {
  FILES_TREE_COOKIE,
  filesTreeCookie,
  filesTreeStore,
  nodeKey,
  resetFilesTreeForTest,
} from "@/lib/stores/files-tree";
import {
  PANELS_COOKIE,
  panelsCookie,
  panelsStore,
  resetPanelsStoreForTest,
} from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import {
  readSidebarFold,
  resetSidebarFoldForTest,
  SIDEBAR_FOLD_COOKIE,
  sidebarFoldCookie,
  sidebarFoldStore,
  unfolded,
} from "@/lib/stores/sidebar-fold";

const beginTitleBarDrag = vi.fn();

/**
 * A desktop with every gated view off. One desktop-only flag is on so
 * `isReducedCapabilityPlatform` reads it as a desktop: since Epic 65 (AD-189)
 * the all-`false` hydrated snapshot IS the phone, and the tier it renders is
 * the stack — so a test of "the pane is absent when its flag is off" would
 * otherwise pass for the wrong reason.
 */
const DESKTOP_WITHOUT_VIEWS = { ...DEFAULT_CAPABILITIES, trayIcon: true };

/**
 * A desktop that can sync. `sync` alone stopped telling the tiers apart in
 * Epic 66 (a phone can sync too), so the fixture carries the same kind of
 * desktop-only flag as above or the shell renders the phone stack.
 */
const DESKTOP_WITH_SYNC = { ...DESKTOP_WITHOUT_VIEWS, sync: true };

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
  // Unmount BEFORE the stores are reset. The capabilities reset below flips a
  // still-mounted phone-tier shell back to the desktop frame, whose restore
  // effects then fire late — after the cookies are cleared and the latches
  // reset — and latch every store on an empty document. The next test's
  // restore then finds the latch set and reads nothing.
  cleanup();
  window.matchMedia = originalMatchMedia;
  detailStore.setState({ open: false });
  roomsStore.getState().selectRoom(null);
  primaryViewStore.getState().setView("inbox");
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  beginTitleBarDrag.mockClear();
  // The panel list is remembered in a cookie, so one test's arrangement would
  // otherwise be the next test's restore.
  resetPanelsStoreForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${PANELS_COOKIE}=; path=/; max-age=0`;
  // The fold is remembered in a cookie too, so one test's fold would otherwise
  // be the next test's restore — and `hydrateSidebarFold` runs once per module,
  // so the reset has to clear both halves.
  resetSidebarFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this test's subject
  document.cookie = `${SIDEBAR_FOLD_COOKIE}=; path=/; max-age=0`;
  // And so is the Files tree's expansion (Story 46.3), for the same reason and
  // with the same two halves.
  resetFilesTreeForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this test's subject
  document.cookie = `${FILES_TREE_COOKIE}=; path=/; max-age=0`;
  // Story 48.1's surface-column fold, for the same reason and with the same two
  // halves: a module-level store and a cookie.
  resetColumnFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this test's subject
  document.cookie = `${COLUMN_FOLD_COOKIE}=; path=/; max-age=0`;
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

  // ── The tier is the platform's (Epic 65, AD-189, FR-441…FR-443) ─────────────
  it("renders the phone stack on a reduced-capability platform at a landscape width", () => {
    // The owner's report: an iPhone 14 Pro Max rotated is 932px wide, and the
    // width rule rendered the desktop frame there at 430px tall.
    mockViewportWidth(932);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AppShell />);

    expect(screen.queryByRole("navigation", { name: "Views" })).not.toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("keeps the desktop frame on a desktop at 1440", () => {
    mockViewportWidth(1440);
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
    render(<AppShell />);

    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
  });

  it("sizes the root by the dynamic viewport, not the largest one", () => {
    // `100vh` on a phone is the viewport with the chrome retracted, which is
    // taller than the screen; `100dvh` is what is on it now and follows a
    // rotation. The two agree on the desktop.
    render(<AppShell />);

    const root = screen.getByRole("navigation", { name: "Views" }).closest(".h-dvh");
    expect(root).not.toBeNull();
    expect(document.querySelector(".h-screen")).toBeNull();
  });

  it("reads none of the desktop cookies on the phone tier", async () => {
    // Every one of the four restores is desktop state the stack never mounts a
    // control for. Left over from an older build's landscape session (or from
    // a desktop profile the harness shares), they must stay unread: a phone
    // that inherits a folded drawer or a folded chat list has no way to unfold
    // either.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = sidebarFoldCookie({ menu: true, groups: { spaces: true, networks: true } });
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = columnFoldCookie({ ...columnsUnfolded(), "chat-list": true });
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = filesTreeCookie(new Set([nodeKey("p1", "docs")]));
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = panelsCookie(
      [
        {
          id: "p",
          target: { kind: "file", profileId: "p1", relativePath: "docs/report.pdf" },
          replaced: null,
          folded: false,
        },
      ],
      "p",
    );
    mockViewportWidth(932);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(sidebarFoldStore.getState().menu).toBe(false);
    expect(sidebarFoldStore.getState().groups).toEqual(unfolded().groups);
    expect(columnFoldStore.getState().columns["chat-list"]).toBe(false);
    expect(filesTreeStore.getState().expanded.size).toBe(0);
    expect(panelsStore.getState().panels.every((panel) => panel.target === null)).toBe(true);
    // And the stack is what rendered: the chat list at level 0, unfolded.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("restores the desktop cookies once a harness window widens into the desktop frame", async () => {
    // The one place the tier still changes: a non-reduced platform crossing
    // 768px. The restore that was held while the stack was up runs on the
    // first desktop frame, so the fold comes back exactly as the width rule
    // alone used to restore it at mount.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = sidebarFoldCookie({ menu: true, groups: { spaces: false, networks: false } });
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
    const listeners: Array<() => void> = [];
    let width = 600;
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
      return {
        get matches() {
          return width <= maxWidth;
        },
        media: query,
        onchange: null,
        addEventListener: (_: string, listener: () => void) => listeners.push(listener),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      };
    });

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(sidebarFoldStore.getState().menu).toBe(false);

    width = 1440;
    act(() => {
      for (const listener of listeners) {
        listener();
      }
    });

    expect(sidebarFoldStore.getState().menu).toBe(true);
    expect(screen.getByRole("button", { name: "Expand menu" })).toBeInTheDocument();
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
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
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
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
    primaryViewStore.getState().setView("recordings");
    render(<AppShell />);

    expect(screen.queryByRole("region", { name: "Recordings" })).not.toBeInTheDocument();
  });

  // ── Sync view (Story 32.5) ─────────────────────────────────────────────────
  it("renders the Sync pane for the sync view when the capability is on", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITH_SYNC);
    primaryViewStore.getState().setView("sync");
    render(<AppShell />);

    expect(screen.getByRole("region", { name: "Sync" })).toBeInTheDocument();
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
  });

  it("does not render the Sync pane when the sync capability is off", () => {
    // A stale "sync" primary-view must never show the pane on a machine with no
    // usable `git`, where every command behind it rejects as unsupported.
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
    primaryViewStore.getState().setView("sync");
    render(<AppShell />);

    expect(screen.queryByRole("region", { name: "Sync" })).not.toBeInTheDocument();
  });

  // ── Files view (Story 43.8) ────────────────────────────────────────────────
  it("renders the Files pane for the files view when the sync capability is on", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITH_SYNC);
    primaryViewStore.getState().setView("files");
    render(<AppShell />);

    expect(screen.getByRole("region", { name: "Files" })).toBeInTheDocument();
    // The browser replaces the chat cluster rather than sitting beside it.
    expect(screen.queryByText("Select a conversation to start reading.")).not.toBeInTheDocument();
    // Sibling surfaces, not the same one.
    expect(screen.queryByRole("region", { name: "Sync" })).not.toBeInTheDocument();
  });

  it("does not render the Files pane when the sync capability is off", () => {
    // A stale "files" primary-view must never show a browser over folders this
    // build cannot sync — the same rule the Sync pane above is gated by.
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
    primaryViewStore.getState().setView("files");
    render(<AppShell />);

    expect(screen.queryByRole("region", { name: "Files" })).not.toBeInTheDocument();
  });

  // ── Tasks view (Story 59.12, corrected by 59.13) ───────────────────────────
  it("keeps an empty panel strip out of the Tasks view, so the pane owns the surface", () => {
    // The defect Story 59.13 measured. The strip is a claimant — `grow shrink
    // basis-[280px] min-w-[280px]` — and mounted unconditionally it took 628 of
    // a 1024px window to render one sentence, leaving the pane's detail region
    // 28px and the add form inside it 0px wide. Right for Files and Notes, where
    // the strip IS the document area; wrong here, where Story 59.1's detail
    // region already is.
    //
    // A `queryByLabelText` and not a width assertion, deliberately: jsdom lays
    // nothing out, so the only thing a component test can own is whether the
    // claimant is in the tree at all. The widths are measured with `dev/probe`.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, sessions: true });
    primaryViewStore.getState().setView("tasks");
    render(<AppShell />);

    expect(screen.getByRole("region", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Open panels")).not.toBeInTheDocument();
  });

  it("puts the panel strip beside the Tasks pane once a panel is holding something", () => {
    // The reachability half of Story 59.12, and it belongs here because nothing
    // else can assert it: the pane writes a `task` target into `panelsStore` and
    // the strip renders it, but the two are only ever siblings HERE. Untested, a
    // pane that composed no strip would ship a gesture with nothing on the other
    // end of it — which is the shape of the report that started 59.12.
    //
    // A FILE target on purpose: the condition is "some panel holds something",
    // not "some panel holds a task". A file left open in the Files surface is
    // still a document somebody asked to keep, and it must not vanish because
    // the reader pressed ⌘8.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, sessions: true });
    primaryViewStore.getState().setView("tasks");
    act(() => {
      panelsStore.getState().openPanel({ kind: "file", profileId: "p1", relativePath: "notes.md" });
    });
    render(<AppShell />);

    expect(screen.getByRole("region", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.getByLabelText("Open panels")).toBeInTheDocument();
  });

  // ── Overlay-titlebar drag band (Story 34.2) ────────────────────────────────
  it("renders no drag band where the platform draws its own title bar", () => {
    // Off macOS the window controls live in a real title bar above the webview, so
    // a band there is empty space under chrome the OS already owns (AD-34-2).
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITHOUT_VIEWS);
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
    expect(columns[0]).toHaveClass("bg-sidebar", SIDEBAR_WIDTH_CLASS.expanded);
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

  /**
   * Story 45.1, and DW-172's lesson made into an assertion.
   *
   * Three tray listeners shipped in epic 44 declared and never mounted, because
   * `renderHook` mounts the hook itself and can therefore never see that `App`
   * does not. The panel restore is the same shape of thing: a `useEffect` that
   * is correct in isolation and worthless if the shell does not call it. So it
   * is asserted HERE, against the real shell, by writing a cookie the way the
   * last run would have and looking for the panel on screen.
   */
  it("restores the panels the last run left open", async () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
    document.cookie = panelsCookie(
      [
        {
          id: "p",
          target: { kind: "file", profileId: "p1", relativePath: "docs/report.pdf" },
          replaced: null,
          folded: false,
        },
      ],
      "p",
    );
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITH_SYNC);
    primaryViewStore.getState().setView("files");

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    // The panel is named after the file it holds, so finding the frame proves
    // the cookie was read, the store was hydrated and the strip was mounted.
    // What it RESOLVES to is the strip's own suite: with no Tauri host the
    // listing call fails and the panel renders that reason, which is the
    // correct behaviour and not what this test is about.
    expect(await screen.findByLabelText("report.pdf")).toBeInTheDocument();
    expect(panelsStore.getState().panels).toHaveLength(1);
  });

  /**
   * The Files tree comes back open (Story 46.3), asserted at the SHELL for the
   * third time and the third reason it is the only place that can fail.
   *
   * `FilesPane`'s own suite calls `hydrateFilesTree` itself, so it would pass
   * unchanged on a build where `AppShell` never called it — which is precisely
   * DW-172 again, and precisely the shape of the defect this story fixed: a
   * restore that lives in a component the shell unmounts.
   */
  it("restores the folders the Files tree last had open", async () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = filesTreeCookie(new Set([nodeKey("p1", ""), nodeKey("p1", "docs")]));
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITH_SYNC);
    primaryViewStore.getState().setView("files");

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    // The store, not the tree on screen: with no Tauri host the profile list
    // never arrives, so there is nothing to render the expansion against. What
    // this asserts is the one thing only the shell can get wrong.
    expect(filesTreeStore.getState().expanded).toEqual(
      new Set([nodeKey("p1", ""), nodeKey("p1", "docs")]),
    );
  });

  /**
   * The fold survives a restart (Story 45.20, FR-198).
   *
   * At the SHELL, deliberately, because the shell is the only place that can
   * fail: `hydrateSidebarFold` is mounted here, and a hook-level test of the
   * store can never see that `AppShell` does not call it (DW-172). That is the
   * exact defect epic 44 shipped with three tray listeners.
   */
  it("comes back folded when the last run left it folded", async () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = sidebarFoldCookie({ menu: true, groups: { spaces: false, networks: false } });

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    // The rail, and a control that says how to get out of it. The nav is still
    // there and still navigable — a fold is not a disappearance.
    expect(screen.getByRole("button", { name: "Expand menu" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Views" })).toHaveClass("w-12");
    expect(screen.getByRole("button", { name: "Chats" })).toBeInTheDocument();
  });

  it("comes back unfolded when the last run left it unfolded", async () => {
    // The inverse, written down, because "folded" is the state a restore that
    // did nothing could not produce and "unfolded" is the state it could. Both
    // arms or the test only proves the cookie can say one thing.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = sidebarFoldCookie(unfolded());

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByRole("button", { name: "Collapse menu" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Views" })).toHaveClass(
      SIDEBAR_WIDTH_CLASS.expanded,
    );
  });

  it("folds on the press and writes it out, so the next run reads it back", async () => {
    // The act, not the offer. This is the whole loop the previous two tests
    // only read one end of: press, persist, and re-read through the same
    // parser a restart would use.
    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    fireEvent.click(screen.getByRole("button", { name: "Collapse menu" }));

    expect(screen.getByRole("navigation", { name: "Views" })).toHaveClass("w-12");
    expect(readSidebarFold(document.cookie).menu).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Expand menu" }));
    expect(readSidebarFold(document.cookie).menu).toBe(false);
  });

  it("withdraws the fold control below the collapse breakpoint and folds anyway", async () => {
    // Under 1080px the viewport has already decided; the user's remembered
    // choice cannot unfold into a width that is not there, and the control is
    // absent rather than present and inert.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = sidebarFoldCookie(unfolded());
    mockViewportWidth(1000);

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByRole("navigation", { name: "Views" })).toHaveClass("w-12");
    expect(screen.queryByRole("button", { name: "Expand menu" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Collapse menu" })).not.toBeInTheDocument();
    // Still navigable, still named.
    expect(screen.getByRole("button", { name: "Chats" })).toBeInTheDocument();
  });

  /**
   * The surface columns come back folded (Story 48.1), asserted at the SHELL for
   * the fourth time and the fourth instance of the same reason.
   *
   * `hydrateColumnFold` is mounted here and nowhere else, because the four
   * columns live on three primary views that are each unmounted whenever
   * another is showing. `column-fold.test.ts` calls the hydrate itself and
   * would pass unchanged on a build where `AppShell` never did (DW-172) — which
   * is exactly the defect Story 45.15 shipped one epic ago, a whole chain built
   * and never mounted.
   *
   * The chat list is the column this test can reach: it is what the default
   * "inbox" view renders.
   */
  it("brings the chat list back folded when the last run left it folded", async () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = columnFoldCookie({ ...columnsUnfolded(), "chat-list": true });

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    const label = SURFACE_COLUMNS["chat-list"].label;
    expect(
      screen.getByRole("button", { name: `${COLUMN_EXPAND_PREFIX} ${label}` }),
    ).toBeInTheDocument();
    // The list itself is gone, and the conversation pane beside it is not.
    expect(screen.queryByLabelText("Loading conversations")).not.toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
  });

  it("brings it back showing when the last run left it showing", async () => {
    // The other arm: "folded" is the state a restore that did nothing could not
    // produce, and "showing" is the state it could.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging cookie state is this test's subject
    document.cookie = columnFoldCookie(columnsUnfolded());

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["chat-list"].label}`,
      }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("keeps offering the column folds below the sidebar's collapse breakpoint", async () => {
    // The sidebar's own fold is WITHDRAWN under 1080px because the viewport has
    // already forced it. A surface column is the opposite case: it is exactly
    // where room is short that putting one away is worth offering, and nothing
    // has decided it for the user.
    mockViewportWidth(1000);

    render(<AppShell />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.queryByRole("button", { name: "Collapse menu" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["chat-list"].label}`,
      }),
    ).toBeInTheDocument();
  });
});

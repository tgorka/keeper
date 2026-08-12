import { act, fireEvent, render, screen } from "@testing-library/react";
import { Layers, Star } from "lucide-react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { SpaceVm } from "@/lib/ipc/client";

// The sidebar's footer reaches for the sign-out hook, and its Settings dialog
// reads the encryption posture. Neither is what this suite measures.
vi.mock("@/hooks/use-sign-out", () => ({ useSignOut: () => vi.fn() }));
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcClient>();
  return { ...actual, encryptionPosture: vi.fn(() => Promise.resolve(false)) };
});

import {
  FOLD_STRIP,
  FOLD_STRIP_DIVIDER_SLOT,
  FOLD_STRIP_TITLE_SLOT,
} from "@/components/layout/fold-strip";
import { PANEL_TESTID, PanelStrip } from "@/components/layout/panel-strip";
import { FoldSection } from "@/components/layout/sidebar-group";
import { SIDEBAR_TITLE, SidebarPane } from "@/components/layout/sidebar-pane";
import {
  COLUMN_COLLAPSE_PREFIX,
  COLUMN_EXPAND_PREFIX,
  type SurfaceRail,
  useSurfaceColumn,
} from "@/components/layout/surface-column";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  COLUMN_WIDTH_COOKIE,
  SURFACE_COLUMN_IDS,
  SURFACE_COLUMNS,
  type SurfaceColumnId,
} from "@/lib/column-widths";
import {
  COLUMN_FOLD_COOKIE,
  columnFoldStore,
  resetColumnFoldForTest,
} from "@/lib/stores/column-fold";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { resetSidebarFoldForTest } from "@/lib/stores/sidebar-fold";
import { spacesStore } from "@/lib/stores/spaces";

/**
 * What the four folding mechanisms owe each other.
 *
 * The owner looked at four folded strips side by side and reported that the
 * gaps were not even and were not present in every menu. They were not: the
 * sidebar, a surface column, a panel and a foldable section carried four
 * different sets of numbers, two of them literals in separate files whose
 * comments each claimed to match the other. **Nothing anywhere asserted a
 * single one of those numbers**, which is exactly why they drifted — a comment
 * saying "the sidebar's collapsed rail, which is `w-12`" does not fail when
 * somebody writes `w-auto` in the fourth file.
 *
 * So this suite measures the strips against each other rather than against
 * numbers typed here. Everything it compares comes from {@link FOLD_STRIP}, and
 * the first `describe` pins that module's Tailwind classes to its own pixel
 * figures, so the two notations cannot come apart either. A test that hard-coded
 * `"w-12"` would pass forever while the strips diverged again around it.
 */

/** `DESIGN.md` → `spacing.unit`. What one Tailwind step is worth, in px. */
const UNIT = 4;

/** Every foldable surface's display name, so a strip can be asked for its own. */
const SURFACE_TITLES = [
  ...SURFACE_COLUMN_IDS.map((id) => SURFACE_COLUMNS[id].title),
  SIDEBAR_TITLE,
];

function classesOf(el: Element): string[] {
  return el.className.split(/\s+/).filter((c) => c !== "");
}

/**
 * The rhythm contract, applied to whatever strip is on screen.
 *
 * Two kinds of container live on a strip and the DOM says which it is. One
 * `inset` container owns the strip's 8px edge; a `nested` one sits inside it and
 * must spell no padding at all — that second rule is the actual defect, because
 * the sidebar's SPACES and NETWORKS sections were nested containers that brought
 * their own `px-1`, so the strip's inset changed halfway down.
 *
 * Both kinds owe the same gap, whichever they are.
 */
function expectStripRhythm(root: ParentNode): void {
  const containers = [...root.querySelectorAll("[data-fold-strip-items]")];
  expect(containers.length).toBeGreaterThan(0);
  for (const el of containers) {
    const classes = classesOf(el);
    expect(classes).toContain(FOLD_STRIP.gapClass);
    const padding = classes.filter((c) => /^p[xytblr]?-/.test(c));
    if (el.getAttribute("data-fold-strip-items") === "inset") {
      // `p-2` or `px-2` — the module's, either way. `p-2 pb-0` and `p-2 pt-1`
      // both contain `p-2`, which is the point of spelling them that way.
      expect(padding.some((c) => c === FOLD_STRIP.padClass || c === FOLD_STRIP.padXClass)).toBe(
        true,
      );
    } else {
      expect(el.getAttribute("data-fold-strip-items")).toBe("nested");
      expect(padding).toEqual([]);
    }
  }
}

/** A surface column, rendered by the same hook the four real surfaces use. */
function Column({ id }: { id: SurfaceColumnId }) {
  const column = useSurfaceColumn(id, {
    rail: [
      { id: "one", icon: Star, label: "Something", onSelect: () => {} },
    ] as unknown as SurfaceRail,
  });
  return (
    <div className="flex">
      <div {...column.rootProps} data-testid="column">
        {column.chrome}
        {column.folded ? null : <p>a body</p>}
      </div>
      {column.seam}
    </div>
  );
}

function renderSidebar(collapsed: boolean) {
  return render(
    <TooltipProvider>
      <SidebarPane collapsed={collapsed} onToggleFold={() => {}} />
    </TooltipProvider>,
  );
}

/** The chat sidebar's foldable section, on the rail, where its metrics matter. */
function renderSection(collapsed: boolean) {
  return render(
    <TooltipProvider>
      <FoldSection
        label="Spaces"
        icon={Layers}
        folded={false}
        onToggle={() => {}}
        id="a-section"
        collapsed={collapsed}
        as="ul"
        bodyClassName={collapsed ? `flex flex-col items-center ${FOLD_STRIP.gapClass}` : "gap-0.5"}
      >
        <li>a row</li>
      </FoldSection>
    </TooltipProvider>,
  );
}

/** The strip a folded panel leaves. The default panel has no target, so nothing
 *  below the header ever reaches the IPC edge. */
function renderFoldedPanel() {
  const result = render(<PanelStrip />);
  const id = panelsStore.getState().panels[0]?.id ?? "";
  act(() => {
    panelsStore.getState().toggleFold(id);
  });
  return { ...result, frame: screen.getByTestId(`${PANEL_TESTID}-${id}`) };
}

beforeEach(() => {
  resetColumnFoldForTest();
  resetPanelsStoreForTest();
  resetSidebarFoldForTest();
});

afterEach(() => {
  for (const name of [COLUMN_FOLD_COOKIE, COLUMN_WIDTH_COOKIE]) {
    // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this suite's arrangement
    document.cookie = `${name}=; path=/; max-age=0`;
  }
});

describe("the folded-strip metrics", () => {
  it("spells every metric once, in two notations that cannot disagree", () => {
    // The class and the number are the same fact. Before this module they were
    // two facts in two files, and that is the whole defect.
    expect(FOLD_STRIP.widthClass).toBe(`w-${FOLD_STRIP.widthPx / UNIT}`);
    expect(FOLD_STRIP.padClass).toBe(`p-${FOLD_STRIP.padPx / UNIT}`);
    expect(FOLD_STRIP.padXClass).toBe(`px-${FOLD_STRIP.padPx / UNIT}`);
    expect(FOLD_STRIP.gapClass).toBe(`gap-${FOLD_STRIP.gapPx / UNIT}`);
    // A head ends flush and the body under it opens with exactly one gap, so
    // the rhythm does not change at the seam between two scroll containers.
    expect(FOLD_STRIP.headPadClass).toBe(`${FOLD_STRIP.padClass} pb-0`);
    expect(FOLD_STRIP.bodyPadClass).toBe(`${FOLD_STRIP.padClass} pt-${FOLD_STRIP.gapPx / UNIT}`);
  });

  it("keeps DESIGN.md's load-bearing 48px strip", () => {
    // Named in DESIGN.md → Layout & Spacing as a dimension the design may not
    // move. Asserted here because it is now spent from one place, so one edit
    // could move all four strips at once.
    expect(FOLD_STRIP.widthPx).toBe(48);
  });

  it("sizes a strip item and a pane-header item at the sizes it claims", () => {
    // The two `size` values are handed to `Button`, so the pixels only hold if
    // the variant still spends them. Rendered rather than asserted from a table.
    render(
      <>
        <Button size={FOLD_STRIP.controlSize} data-testid="rail-item" />
        <Button size={FOLD_STRIP.headControlSize} data-testid="head-item" />
      </>,
    );
    expect(classesOf(screen.getByTestId("rail-item"))).toContain(
      `size-${FOLD_STRIP.controlPx / UNIT}`,
    );
    expect(classesOf(screen.getByTestId("head-item"))).toContain(
      `size-${FOLD_STRIP.headControlPx / UNIT}`,
    );
    // The exception is smaller, not larger: it exists to fit a 40px pane-header
    // row, and it still clears DESIGN.md's 32px control floor exactly.
    expect(FOLD_STRIP.headControlPx).toBe(32);
    expect(FOLD_STRIP.controlPx).toBeGreaterThan(FOLD_STRIP.headControlPx);
  });
});

describe("every folded strip is the same strip", () => {
  it("gives the sidebar the shared width", () => {
    renderSidebar(true);
    expect(classesOf(screen.getByRole("navigation", { name: "Views" }))).toContain(
      FOLD_STRIP.widthClass,
    );
  });

  it.each(SURFACE_COLUMN_IDS)("gives the %s column the shared width", (id) => {
    render(<Column id={id} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
      }),
    );
    // A number rather than a class, because a column's width is an inline style
    // the rest of the time. Same number.
    expect(screen.getByTestId("column")).toHaveStyle({ width: `${FOLD_STRIP.widthPx}px` });
  });

  it("gives a folded panel the shared width instead of fitting its one button", () => {
    // `w-auto` was the fourth strip's answer, and nothing measured it: its width
    // was whatever its control happened to be, so the four strips standing side
    // by side were not the same width and no test could have said so.
    const { frame } = renderFoldedPanel();
    expect(classesOf(frame)).toContain(FOLD_STRIP.widthClass);
    expect(classesOf(frame)).not.toContain("w-auto");
  });

  it("gives the sidebar's strip one inset and one rhythm from top to bottom", () => {
    // The SPACES/NETWORKS boundary: the views list and the groups are siblings,
    // so before this the strip's inset changed halfway down it.
    const { container } = renderSidebar(true);
    expectStripRhythm(container);
  });

  it.each(SURFACE_COLUMN_IDS)("gives the %s column's strip that rhythm", (id) => {
    const { container } = render(<Column id={id} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
      }),
    );
    expectStripRhythm(container);
  });

  it("gives a folded panel that rhythm", () => {
    const { container } = renderFoldedPanel();
    expectStripRhythm(container);
  });

  it("gives a section folded onto the rail that rhythm, and no inset of its own", () => {
    const { container } = renderSection(true);
    expectStripRhythm(container);
  });

  it("sizes every way back on a rail at the strip's item size", () => {
    const item = `size-${FOLD_STRIP.controlPx / UNIT}`;

    const sidebar = renderSidebar(true);
    expect(
      classesOf(sidebar.container.querySelector("[data-slot=sidebar-fold]") as Element),
    ).toContain(item);
    sidebar.unmount();

    const column = render(<Column id="notes-rail" />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );
    expect(
      classesOf(column.container.querySelector("[data-slot=column-fold]") as Element),
    ).toContain(item);
    column.unmount();

    const section = renderSection(true);
    // ~22px `h-auto` before this, sitting under 36px nav buttons on one strip.
    expect(
      classesOf(section.container.querySelector("[data-slot=sidebar-group-fold]") as Element),
    ).toContain(item);
  });

  it("sizes every ITEM on the sidebar's strip alike, nav rows and group rows", () => {
    // The rows the strip actually holds, not just the controls that fold it.
    // A Space row reached 36px by putting `p-1.5` around a 24px avatar and a
    // Network row copied the sum; either could have drifted off the column
    // without a single test noticing, which is how a strip gets ragged edges.
    spacesStore.setState({
      spaces: [
        {
          accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
          spaceId: "!space:example.org",
          name: "Field",
          avatarUrl: null,
          roomIds: [],
        } as unknown as SpaceVm,
      ],
    });
    const { container } = renderSidebar(true);

    const rows = [...container.querySelectorAll("[data-fold-strip-items] button")];
    expect(rows.length).toBeGreaterThan(1);
    for (const row of rows) {
      expect(classesOf(row)).toContain(FOLD_STRIP.controlClass);
    }
  });

  it("puts a divider under the way back on every strip that has more below it", () => {
    // The sidebar had none and a surface column had one, for no reason either
    // file could state. Now the rule is the reason: a divider separates the way
    // out from what is still reachable, so a strip with nothing else has none.
    const sidebar = renderSidebar(true);
    expect(
      sidebar.container.querySelectorAll(`[data-slot=${FOLD_STRIP_DIVIDER_SLOT}]`),
    ).toHaveLength(1);
    sidebar.unmount();

    const column = render(<Column id="notes-rail" />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );
    expect(
      column.container.querySelectorAll(`[data-slot=${FOLD_STRIP_DIVIDER_SLOT}]`),
    ).toHaveLength(1);
    column.unmount();

    // A folded panel is the way back and nothing else, so there is nothing to
    // separate it from.
    const panel = renderFoldedPanel();
    expect(panel.container.querySelectorAll(`[data-slot=${FOLD_STRIP_DIVIDER_SLOT}]`)).toHaveLength(
      0,
    );
  });

  it("draws no divider while a surface is open", () => {
    const sidebar = renderSidebar(false);
    expect(
      sidebar.container.querySelectorAll(`[data-slot=${FOLD_STRIP_DIVIDER_SLOT}]`),
    ).toHaveLength(0);
    sidebar.unmount();

    const column = render(<Column id="notes-list" />);
    expect(
      column.container.querySelectorAll(`[data-slot=${FOLD_STRIP_DIVIDER_SLOT}]`),
    ).toHaveLength(0);
  });
});

describe("every foldable surface says which one it is", () => {
  it("gives each one a distinct display name", () => {
    // "Notes" over both the rail and the list would answer nothing: they sit
    // side by side on one screen, which is the case the names exist for.
    expect(new Set(SURFACE_TITLES).size).toBe(SURFACE_TITLES.length);
  });

  it("puts the visible name inside the fold control's spoken name", () => {
    // WCAG 2.5.3, ignoring case: the control is operated by people saying the
    // word they can see, and the two used to have no relationship at all.
    for (const id of SURFACE_COLUMN_IDS) {
      const spec = SURFACE_COLUMNS[id];
      expect(spec.label.toLowerCase()).toContain(spec.title.toLowerCase());
    }
  });

  it.each(SURFACE_COLUMN_IDS)("shows the %s column's name at the very top, open", (id) => {
    render(<Column id={id} />);

    const title = screen.getByRole("heading", { name: SURFACE_COLUMNS[id].title });
    expect(title).toHaveAttribute("data-slot", FOLD_STRIP_TITLE_SLOT);
    // And the fold control kept its place in that row: a strip that gained a
    // title and lost its way back would be the worse defect.
    expect(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
      }),
    ).toBeInTheDocument();
  });

  it("shows the drawer's name at the very top, open, and only then", () => {
    const open = renderSidebar(false);
    expect(screen.getByRole("heading", { name: SIDEBAR_TITLE })).toBeInTheDocument();
    open.unmount();

    // 48px has no room for the word — see `fold-strip.tsx` for the measurement
    // — so the folded strip carries it on the control instead.
    renderSidebar(true);
    expect(screen.queryByRole("heading", { name: SIDEBAR_TITLE })).toBeNull();
    expect(
      screen.getByRole("button", { name: `Expand ${SIDEBAR_TITLE.toLowerCase()}` }),
    ).toBeInTheDocument();
  });

  it.each(SURFACE_COLUMN_IDS)("names the %s region from its own heading, open", (id) => {
    // One source for the name. A region labelled "Files" wrapping a heading
    // reading "Files" announces the word twice, and the two can drift.
    render(<Column id={id} />);
    const root = screen.getByTestId("column");
    const heading = screen.getByRole("heading", { name: SURFACE_COLUMNS[id].title });
    expect(root).toHaveAttribute("aria-labelledby", heading.id);
    expect(root).not.toHaveAttribute("aria-label");
  });

  it.each(SURFACE_COLUMN_IDS)("still names the %s region once folded", (id) => {
    render(<Column id={id} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
      }),
    );

    // No heading to point at, so the words are spelled out rather than dropped.
    const root = screen.getByTestId("column");
    expect(root).toHaveAttribute("aria-label", SURFACE_COLUMNS[id].title);
    expect(root).not.toHaveAttribute("aria-labelledby");
  });

  it.each(SURFACE_COLUMN_IDS)("hangs a tooltip on the %s strip's way back", (id) => {
    // The strip's only answer to "which menu is this". Asserted structurally:
    // the control is a tooltip trigger while folded and a bare button while
    // open, where the title beside it has already said the word.
    render(<Column id={id} />);
    const open = screen.getByRole("button", {
      name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
    });
    expect(open).not.toHaveAttribute("data-state");

    fireEvent.click(open);
    const folded = screen.getByRole("button", {
      name: `${COLUMN_EXPAND_PREFIX} ${SURFACE_COLUMNS[id].label}`,
    });
    expect(folded).toHaveAttribute("data-state");
  });

  it("hangs one on the drawer's way back too, and on a section folded to a glyph", () => {
    const sidebar = renderSidebar(true);
    expect(sidebar.container.querySelector("[data-slot=sidebar-fold]")).toHaveAttribute(
      "data-state",
    );
    sidebar.unmount();

    const section = renderSection(true);
    expect(section.container.querySelector("[data-slot=sidebar-group-fold]")).toHaveAttribute(
      "data-state",
    );
  });
});

describe("folding one strip is still just folding one strip", () => {
  it("leaves the other columns where they were", () => {
    // The metrics are shared; the state is not. Worth pinning now that one
    // module is read by all four.
    render(
      <div className="flex">
        <Column id="notes-rail" />
        <Column id="notes-list" />
      </div>,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );
    expect(columnFoldStore.getState().columns["notes-rail"]).toBe(true);
    expect(columnFoldStore.getState().columns["notes-list"]).toBe(false);
  });
});

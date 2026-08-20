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
  FOLD_STRIP_HEAD_SLOT,
  FOLD_STRIP_NAME_SLOT,
  FOLD_STRIP_SLOT,
  FOLD_STRIP_TITLE_SLOT,
} from "@/components/layout/fold-strip";
import { PaneHeader } from "@/components/layout/pane-header";
import {
  PANEL_FOLD_LABEL,
  PANEL_TESTID,
  PANEL_UNFOLD_LABEL,
  PanelStrip,
} from "@/components/layout/panel-strip";
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
 *
 * The second round of this was the head. Owning the width, the inset, the gap
 * and the item size still left three strips ragged, because the glyph on the
 * way back and the HEIGHT of the row holding it were not here: a panel folded
 * under `ChevronsRightLeft` in a 40px band while the drawer and the columns
 * folded under `PanelLeftClose` in a 44px one, so the divider under the panel
 * sat 8px above the divider under everything beside it. That is why this suite
 * now compares the mechanisms' HEADS glyph for glyph and class for class, and
 * why it holds the band against {@link PaneHeader} itself: the band is
 * `DESIGN.md`'s `pane-header`, and a strip's head lining up with the pane
 * headers beside it is the whole of what the owner asked for.
 *
 * The third is the name down the spine. A strip that says which one it is in
 * rotated text is one CSS property away from a strip whose name pushes its
 * controls off the top or eats their clicks, so the properties that make it
 * safe are asserted rather than trusted: it comes last, it is `aria-hidden`, it
 * is transparent to the pointer, and it carries a cap. What this suite CANNOT
 * see is the pixels — jsdom does no layout — so the boxes were measured in
 * Chromium against the running app, and what is left here is the contract that
 * measurement depended on.
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
    // The body under the head's edge opens with exactly one gap, so the rhythm
    // does not change at the seam between the band and the scrolling part.
    expect(FOLD_STRIP.bodyPadClass).toBe(`${FOLD_STRIP.padClass} pt-${FOLD_STRIP.gapPx / UNIT}`);
    expect(FOLD_STRIP.headHeightClass).toBe(`h-${FOLD_STRIP.headPx / UNIT}`);
    expect(FOLD_STRIP.nameMaxClass).toBe(`max-h-${FOLD_STRIP.namePx / UNIT}`);
  });

  it("keeps DESIGN.md's load-bearing 48px strip", () => {
    // Named in DESIGN.md → Layout & Spacing as a dimension the design may not
    // move. Asserted here because it is now spent from one place, so one edit
    // could move all four strips at once.
    expect(FOLD_STRIP.widthPx).toBe(48);
  });

  it("draws its head to DESIGN.md's pane-header, the same one PaneHeader draws", () => {
    // The head height is not this module's to choose: a folded strip stands in
    // a row with open panes, and their header band is 40px. Asserted against
    // the COMPONENT rather than against the number, because a `pane-header`
    // that moved and a `FOLD_STRIP` that did not is the same class of drift as
    // the four widths this module was written to end.
    expect(FOLD_STRIP.headPx).toBe(40);
    render(<PaneHeader identity={<span>a pane</span>} actions={null} />);
    const band = classesOf(screen.getByRole("banner"));
    expect(band).toContain(FOLD_STRIP.headHeightClass);
    // And it ends in an edge, which is the rule a strip's head puts its divider
    // at. One band, one height, one line across the row.
    expect(band).toContain("border-b");
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
    // The head control is SMALLER than a strip item, and that is the shape and
    // not an accident: a control in a header band is a header control, at the
    // size every other pane header in this app spends, and it still clears
    // DESIGN.md's 32px floor exactly. A strip item below the band has no label
    // beside it to widen its target, so it stays above the floor.
    expect(FOLD_STRIP.headControlPx).toBe(32);
    expect(FOLD_STRIP.controlPx).toBeGreaterThan(FOLD_STRIP.headControlPx);
    // Both fit the band they are in — 36px in a 40px row still leaves the rule
    // where it belongs, but only one of them is what a header row spends.
    expect(FOLD_STRIP.headControlPx).toBeLessThanOrEqual(FOLD_STRIP.headPx);
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
    expect(screen.getByTestId("column")).toHaveStyle({ flexBasis: `${FOLD_STRIP.widthPx}px` });
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

  it("puts every way back in a head band, at the head control's size", () => {
    // Before this the drawer and the columns spent a 36px strip item here and
    // the panel spent a 32px header one, so three strips in a row had their
    // glyphs at two different heights with their dividers 8px apart. The head
    // is a pane-header band now, and a control in a header band is a header
    // control.
    const head = `size-${FOLD_STRIP.headControlPx / UNIT}`;

    const sidebar = renderSidebar(true);
    const sidebarFold = sidebar.container.querySelector("[data-slot=sidebar-fold]") as Element;
    expect(classesOf(sidebarFold)).toContain(head);
    expect(sidebarFold.closest(`[data-slot=${FOLD_STRIP_HEAD_SLOT}]`)).not.toBeNull();
    sidebar.unmount();

    const column = render(<Column id="notes-rail" />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );
    const columnFold = column.container.querySelector("[data-slot=column-fold]") as Element;
    expect(classesOf(columnFold)).toContain(head);
    expect(columnFold.closest(`[data-slot=${FOLD_STRIP_HEAD_SLOT}]`)).not.toBeNull();
    column.unmount();

    const panel = renderFoldedPanel();
    const panelFold = panel.container.querySelector(
      `[aria-label^="${PANEL_UNFOLD_LABEL}"]`,
    ) as Element;
    expect(classesOf(panelFold)).toContain(head);
    expect(panelFold.closest(`[data-slot=${FOLD_STRIP_HEAD_SLOT}]`)).not.toBeNull();
  });

  it("keeps a section folded onto the rail at the strip's ITEM size", () => {
    // Not a head: a `FoldSection` is a row ON a strip, under somebody else's
    // band, so it is sized by the list it is in and not by a header row. ~22px
    // `h-auto` before this module, sitting under 36px nav buttons.
    const section = renderSection(true);
    expect(
      classesOf(section.container.querySelector("[data-slot=sidebar-group-fold]") as Element),
    ).toContain(`size-${FOLD_STRIP.controlPx / UNIT}`);
  });

  it("folds every mechanism under one glyph, and unfolds under its pair", () => {
    // The panel wore `ChevronsRightLeft` beside three `PanelLeftClose`s. The
    // glyphs are compared to EACH OTHER rather than to a name lucide could
    // rename: what must hold is that no mechanism picks its own.
    const glyphOf = (control: Element): string => {
      const svg = control.querySelector("svg");
      expect(svg).not.toBeNull();
      return (svg as SVGElement).getAttribute("class") ?? "";
    };

    const openSidebar = renderSidebar(false);
    const openGlyph = glyphOf(
      openSidebar.container.querySelector("[data-slot=sidebar-fold]") as Element,
    );
    const openColumn = render(<Column id="notes-rail" />);
    expect(glyphOf(openColumn.container.querySelector("[data-slot=column-fold]") as Element)).toBe(
      openGlyph,
    );
    const openPanel = render(<PanelStrip />);
    expect(glyphOf(screen.getByRole("button", { name: PANEL_FOLD_LABEL }))).toBe(openGlyph);
    openSidebar.unmount();
    openColumn.unmount();
    openPanel.unmount();

    const sidebar = renderSidebar(true);
    const foldedGlyph = glyphOf(
      sidebar.container.querySelector("[data-slot=sidebar-fold]") as Element,
    );
    // The two states are two glyphs, not one: the control says which way it
    // goes, which is the half a strip has no other way to state.
    expect(foldedGlyph).not.toBe(openGlyph);
    sidebar.unmount();

    const column = render(<Column id="notes-rail" />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );
    expect(glyphOf(column.container.querySelector("[data-slot=column-fold]") as Element)).toBe(
      foldedGlyph,
    );
    column.unmount();

    const panel = renderFoldedPanel();
    expect(
      glyphOf(panel.container.querySelector(`[aria-label^="${PANEL_UNFOLD_LABEL}"]`) as Element),
    ).toBe(foldedGlyph);
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

    // Below the band only: the head holds the way back, which is a header
    // control and deliberately a size smaller. Two groups, one rule between.
    const rows = [
      ...container.querySelectorAll(
        `[data-fold-strip-items]:not([data-slot=${FOLD_STRIP_HEAD_SLOT}]) button`,
      ),
    ];
    expect(rows.length).toBeGreaterThan(1);
    for (const row of rows) {
      expect(classesOf(row)).toContain(FOLD_STRIP.controlClass);
    }
  });

  it("gives every strip exactly one head, and the head is what carries the rule", () => {
    // The divider used to be a 24px hairline the drawer did not draw, the
    // column did, and the panel replaced with its own full-bleed edge 8px
    // higher. There is one rule now and it is the bottom of the band, so every
    // strip's divider is at the same y as every other strip's by construction
    // — and at the same y as the pane headers of the open panes beside them.
    const expectOneRuledHead = (root: ParentNode): void => {
      // Every strip marks its own root, so this suite finds them by ONE
      // selector rather than by a list of per-mechanism ones it has to be told
      // about — the fifth foldable thing is held to this without an edit here.
      const strips = [...root.querySelectorAll(`[data-fold-strip=${FOLD_STRIP_SLOT}]`)];
      expect(strips).toHaveLength(1);
      const heads = [...root.querySelectorAll(`[data-slot=${FOLD_STRIP_HEAD_SLOT}]`)];
      expect(heads).toHaveLength(1);
      const classes = classesOf(heads[0] as Element);
      expect(classes).toContain(FOLD_STRIP.headHeightClass);
      expect(classes).toContain("border-b");
      // And it is not a landmark. Drafted as a `<header>`, the band was
      // measured in Chromium announcing a second and a third `banner` — one
      // for each column whose root is a plain `<div>` and so does not scope it
      // away. There is one banner on a screen, and it is not a fold row.
      expect((heads[0] as Element).tagName).toBe("DIV");
      expect(heads[0] as Element).not.toHaveAttribute("role");
    };

    const sidebar = renderSidebar(true);
    expectOneRuledHead(sidebar.container);
    sidebar.unmount();

    for (const id of SURFACE_COLUMN_IDS) {
      const column = render(<Column id={id} />);
      fireEvent.click(
        screen.getByRole("button", {
          name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
        }),
      );
      expectOneRuledHead(column.container);
      column.unmount();
    }

    // The panel was the one that had this right. It still has it, from the
    // shared component rather than from a `py-1` of its own.
    expectOneRuledHead(renderFoldedPanel().container);
  });

  it("gives an OPEN surface the same band, so folding moves nothing", () => {
    // The head used to be 44px open and 44px folded, which was self-consistent
    // and 4px off every pane header in the shell. Now the fold control is at
    // the same y in both states and in every mechanism.
    const sidebar = renderSidebar(false);
    const openHead = sidebar.container.querySelector(
      `[data-slot=${FOLD_STRIP_HEAD_SLOT}]`,
    ) as Element;
    expect(classesOf(openHead)).toContain(FOLD_STRIP.headHeightClass);
    sidebar.unmount();

    const column = render(<Column id="notes-list" />);
    expect(
      classesOf(column.container.querySelector(`[data-slot=${FOLD_STRIP_HEAD_SLOT}]`) as Element),
    ).toContain(FOLD_STRIP.headHeightClass);
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

    // Folded, the heading is gone and the name is down the spine instead — the
    // strip still says it, in an element no heading list has to carry.
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

  it.each(SURFACE_COLUMN_IDS)("writes the %s column's name down the folded spine", (id) => {
    // The owner looked at four tooltipped strips and asked for the words. This
    // is the words: the surface's own title, the last thing on the strip.
    render(<Column id={id} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS[id].label}`,
      }),
    );
    const strip = screen.getByTestId("column");
    const spine = strip.querySelector(`[data-slot=${FOLD_STRIP_NAME_SLOT}]`) as Element;
    expect(spine.textContent).toBe(SURFACE_COLUMNS[id].title);
    // LAST. Everything above it took its height first; the spine is what is
    // left over, which is the only reason a rotated name at 48px is safe.
    expect(strip.lastElementChild).toBe(spine);
  });

  it("writes the drawer's name down its spine, and nothing while it is open", () => {
    const sidebar = renderSidebar(true);
    expect(
      sidebar.container.querySelector(`[data-slot=${FOLD_STRIP_NAME_SLOT}]`)?.textContent,
    ).toBe(SIDEBAR_TITLE);
    sidebar.unmount();

    const open = renderSidebar(false);
    expect(open.container.querySelector(`[data-slot=${FOLD_STRIP_NAME_SLOT}]`)).toBeNull();
  });

  it("writes a folded panel's name down its spine", () => {
    const panel = renderFoldedPanel();
    const spine = panel.frame.querySelector(`[data-slot=${FOLD_STRIP_NAME_SLOT}]`) as Element;
    // The default panel has no target, so this is what a panel with nothing in
    // it is called. The point is that the strip is not blank below its glyph.
    expect(spine.textContent).toBe("Panel");
    expect(panel.frame.lastElementChild).toBe(spine);
  });

  it("keeps the spine out of the way of the strip it is on", () => {
    // The four properties the rotated name is only safe because of. jsdom does
    // no layout, so these are the contract the Chromium measurement rested on:
    // it cannot be read aloud a third time, it cannot take a click meant for a
    // control, it cannot grow past its cap, and it gives its space back before
    // anything else does.
    const sidebar = renderSidebar(true);
    const spine = sidebar.container.querySelector(
      `[data-slot=${FOLD_STRIP_NAME_SLOT}]`,
    ) as HTMLElement;
    expect(spine).toHaveAttribute("aria-hidden", "true");
    expect(classesOf(spine)).toContain("pointer-events-none");
    expect(classesOf(spine)).toEqual(expect.arrayContaining(["min-h-0", "flex-1"]));

    const line = spine.firstElementChild as Element;
    expect(classesOf(line)).toContain(FOLD_STRIP.nameMaxClass);
    // Turned on its side, so the cap above is a LINE LENGTH and `truncate`
    // ellipsises against it — a note title is user input and unbounded.
    expect(classesOf(line)).toContain("[writing-mode:vertical-rl]");
    expect(classesOf(line)).toContain("rotate-180");
    expect(classesOf(line)).toContain("truncate");
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

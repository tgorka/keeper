/**
 * Show what fits, menu what does not (Story 48.5).
 *
 * **What this file can and cannot prove, up front, because the gap is the whole
 * difficulty of the story.** jsdom performs no layout: every element reports a
 * zero rect, `src/test/setup.ts`'s shim answers one viewport for the zero-sized
 * ones, and it deliberately stops at CodeMirror's edge because an unscoped shim
 * once told the editor every line was a screen tall. So a reflow cannot be
 * observed here, and a test that watched the fourth control move into the menu
 * as a real window narrowed would be a test of the shim.
 *
 * That is why the decision is a pure function. {@link planPriorityActions} and
 * {@link paneHeaderActionsBudget} take numbers and return numbers, so every
 * boundary in the policy — the width at which the first item moves, the width
 * at which nothing is left but the menu — is provable to the pixel, below,
 * without a browser.
 *
 * The second half of the file drives the real components with a geometry the
 * test *arranges* rather than one it measures: a `ResizeObserver` the test
 * fires by hand, and a `getBoundingClientRect` that answers a declared width
 * for the elements this mechanism measures. That proves the plumbing — the
 * observer reaches the plan, the plan reaches the DOM, the menu keeps what the
 * row dropped — with the honest caveat that the NUMBERS are the test's and not
 * a browser's. What is left for a human on the Mac is only whether the real
 * font in the real 560px window produces the widths this file invents.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { withActionWidths, withHandFiredResize } from "@/test/layout";
import { PaneHeader, paneHeaderActionsBudget } from "./pane-header";
import {
  PRIORITY_ACTION_ATTR,
  type PriorityAction,
  PriorityActions,
  planPriorityActions,
} from "./priority-actions";

describe("planPriorityActions", () => {
  /** The four candidates, in the order the note header declares them. */
  const WIDTHS = [120, 110, 90, 100];
  /** A leading control (100) and the menu trigger (80), with one gap between. */
  const RESERVED = 188;
  const GAP = 8;

  it("promotes everything when the row can hold everything", () => {
    // Every cost paid: 188 + 128 + 118 + 98 + 108.
    expect(
      planPriorityActions({ available: 640, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(4);
    expect(
      planPriorityActions({ available: 4000, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(4);
  });

  it("promotes nothing, and still has a menu, when the row can hold nothing", () => {
    // The trigger is inside `reserved`, so the menu is never something the
    // budget can decline to buy: at any width at all the answer is "no
    // controls", never "no menu".
    expect(
      planPriorityActions({ available: 188, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(0);
    expect(
      planPriorityActions({ available: 0, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(0);
    expect(
      planPriorityActions({ available: -400, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(0);
  });

  it("moves the first item at exactly one width and not one pixel earlier", () => {
    // 188 reserved + 120 wide + 8 of gap. The gap is charged to the item
    // because there is always something to its right — the trigger, at least.
    expect(
      planPriorityActions({ available: 316, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(1);
    expect(
      planPriorityActions({ available: 315, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(0);
  });

  it("moves each later item at exactly one width too", () => {
    expect(
      planPriorityActions({ available: 434, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(2);
    expect(
      planPriorityActions({ available: 433, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(1);
    expect(
      planPriorityActions({ available: 532, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(3);
    expect(
      planPriorityActions({ available: 531, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(2);
    expect(
      planPriorityActions({ available: 639, reserved: RESERVED, widths: WIDTHS, gap: GAP }),
    ).toBe(3);
  });

  it("charges the gap once per promoted item", () => {
    // Two items of 100 in a group with 200 free do NOT both fit: each has a
    // neighbour on its right. A plan that forgot the gap would answer 2, and
    // the row it described would be 16px wider than the row that exists.
    expect(planPriorityActions({ available: 200, reserved: 0, widths: [100, 100], gap: 8 })).toBe(
      1,
    );
    expect(planPriorityActions({ available: 216, reserved: 0, widths: [100, 100], gap: 8 })).toBe(
      2,
    );
  });

  it("stops at the first item that does not fit rather than packing by width", () => {
    // The second item is enormous and the third would fit in what is left.
    // Priority means priority: an item is out here only if everything above it
    // is, because a toolbar whose controls change places as a window is dragged
    // cannot be learned.
    expect(
      planPriorityActions({ available: 500, reserved: 0, widths: [100, 900, 20], gap: 8 }),
    ).toBe(1);
  });

  it("treats a width that was never measured as a stop, not as zero", () => {
    // Number.NaN is what an unmeasured candidate contributes. Read as zero it
    // would be free, and a group with a huge budget would promote a control
    // whose width nobody knows — straight past the edge of the window.
    expect(
      planPriorityActions({ available: 4000, reserved: 0, widths: [100, Number.NaN, 20], gap: 8 }),
    ).toBe(1);
  });

  it("promotes nothing when the row itself has not been measured", () => {
    expect(planPriorityActions({ available: Number.NaN, reserved: 0, widths: [10], gap: 8 })).toBe(
      0,
    );
    expect(
      planPriorityActions({ available: 100, reserved: Number.NaN, widths: [10], gap: 8 }),
    ).toBe(0);
  });

  it("promotes nothing in a world with no layout at all", () => {
    // Every width zero is what an unshimmed jsdom reports, and it is also the
    // first commit in a real browser. Charging the gap is what keeps the answer
    // honest: without it, four zero-width items would all "fit" in a zero-width
    // row and this suite would be asserting a header that cannot exist.
    expect(planPriorityActions({ available: 0, reserved: 0, widths: [0, 0, 0, 0], gap: 8 })).toBe(
      0,
    );
  });
});

describe("paneHeaderActionsBudget", () => {
  it("keeps the identity's floor, the status's box and both gaps out of it", () => {
    // 1400 - 160 identity - 8 - 70 status - 8.
    expect(paneHeaderActionsBudget({ header: 1400, status: 70 })).toBe(1154);
  });

  it("charges one gap, not two, for a row with no status group", () => {
    // A header with nothing to report renders two groups, so there is one seam
    // in it and not two — see the component's own doc.
    expect(paneHeaderActionsBudget({ header: 1400, status: null })).toBe(1232);
  });

  it("never reports negative room", () => {
    expect(paneHeaderActionsBudget({ header: 100, status: 70 })).toBe(0);
    expect(paneHeaderActionsBudget({ header: 0, status: null })).toBe(0);
  });

  it("reports no room at all when the row has not been measured", () => {
    expect(paneHeaderActionsBudget({ header: Number.NaN, status: 70 })).toBe(0);
  });

  it("ignores a status box that measured to nothing but still keeps its gap", () => {
    // The slot is in the DOM, so the seam beside it is too.
    expect(paneHeaderActionsBudget({ header: 1000, status: 0 })).toBe(824);
  });
});

/**
 * The declared widths this file's geometry hands back, keyed the way the
 * mechanism asks for them. Chosen so the boundaries are memorable rather than
 * realistic: reserved is 188, and the four items cost 128, 118, 98 and 108.
 */
const WIDTH: Record<string, number> = {
  attachments: 120,
  properties: 110,
  history: 90,
  files: 100,
  leading: 100,
  menu: 80,
  status: 70,
};

const picked: string[] = [];

/** The note header's four candidates, in its declared priority order. */
const ITEMS: readonly PriorityAction[] = [
  { id: "attachments", label: "Attachments", onSelect: () => picked.push("attachments") },
  { id: "properties", label: "Properties", onSelect: () => picked.push("properties") },
  { id: "history", label: "History", onSelect: () => picked.push("history") },
  { id: "files", label: "Show in Files", onSelect: () => picked.push("files") },
];

/** A verb that never leaves the menu, whatever the row can afford. */
const DELETE_LABEL = "Delete note";

/** A menu item this mechanism has never heard of, interleaved among the
 *  candidates the way the note header interleaves its capture item. */
const CAPTURE_LABEL = "Open in a capture window";

/**
 * The one place a width can be declared for the elements this mechanism
 * measures, and the hand-fired observation that reaches the plan. Both live in
 * `src/test/layout.ts`, beside the rest of this repository's crude layout
 * engine, so the note editor's own header suite arranges the same geometry the
 * same way.
 */
function mount(): (width: number) => void {
  restoreWidths = withActionWidths(WIDTH);
  observer = withHandFiredResize();
  render(<Harness />);
  const { resize } = observer;
  return (width) => {
    act(() => resize(width));
  };
}

let restoreWidths: (() => void) | null = null;
let observer: { resize: (width: number) => void; undo: () => void } | null = null;

function Harness(): React.ReactElement {
  return (
    <PaneHeader
      identity={<h1>a note</h1>}
      status={{ sizers: ["Saved"], caption: "Saved" }}
      actions={(budget) => (
        <PriorityActions
          budget={budget}
          leading={
            <Button size="sm" variant="ghost">
              Attach a file
            </Button>
          }
          items={ITEMS}
          menu={(inMenu) => (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button size="sm" variant="ghost">
                  Actions
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {ITEMS.filter((item) => inMenu(item.id)).map((item) => (
                  <DropdownMenuItem key={item.id} onSelect={item.onSelect}>
                    {item.label}
                  </DropdownMenuItem>
                ))}
                <DropdownMenuItem>{CAPTURE_LABEL}</DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive">{DELETE_LABEL}</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        />
      )}
    />
  );
}

/** Every label that is a control in the row right now, in the row's order. */
function promoted(): string[] {
  return Array.from(document.querySelectorAll(`[${PRIORITY_ACTION_ATTR}]`)).map(
    (control) => control.textContent ?? "",
  );
}

/** Open the menu and hand back what is in it, in its own order. */
function openMenu(): string[] {
  const trigger = screen.getByRole("button", { name: "Actions" });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(trigger);
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

/** Shut it again, so the next assertion in the same test starts level. */
function closeMenu(): void {
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
}

describe("the header row at several widths", () => {
  afterEach(() => {
    restoreWidths?.();
    restoreWidths = null;
    observer?.undo();
    observer = null;
    picked.length = 0;
  });

  it("shows every verb as a word when the row is wide", () => {
    const resize = mount();
    resize(1400);

    expect(promoted()).toEqual(["Attachments", "Properties", "History", "Show in Files"]);
    // The menu is still there, and it still holds what a menu should.
    expect(openMenu()).toEqual([CAPTURE_LABEL, DELETE_LABEL]);
  });

  it("degrades one control at a time as the row narrows", () => {
    const resize = mount();

    // 1400 - 246 owed = 1154: everything.
    resize(1400);
    expect(promoted()).toEqual(["Attachments", "Properties", "History", "Show in Files"]);

    // 800 - 246 = 554, which buys three of the four costs 128/118/98/108.
    resize(800);
    expect(promoted()).toEqual(["Attachments", "Properties", "History"]);

    // 700 - 246 = 454.
    resize(700);
    expect(promoted()).toEqual(["Attachments", "Properties"]);

    // 620 - 246 = 374, the shape a 560px capture window is near.
    resize(620);
    expect(promoted()).toEqual(["Attachments"]);

    // 400 - 246 = 154, which buys nothing.
    resize(400);
    expect(promoted()).toEqual([]);
  });

  it("moves the first control into the menu at exactly one width", () => {
    const resize = mount();

    // Budget 316 is the cost of the leading control, the trigger and the first
    // item to the pixel.
    resize(562);
    expect(promoted()).toEqual(["Attachments"]);
    resize(561);
    expect(promoted()).toEqual([]);
  });

  it("keeps the destructive verb in the menu at every width", () => {
    const resize = mount();

    for (const width of [1400, 800, 700, 620, 400, 0]) {
      resize(width);
      expect(promoted()).not.toContain(DELETE_LABEL);
      expect(openMenu()).toContain(DELETE_LABEL);
      closeMenu();
    }
  });

  it("renders no verb twice at any width", () => {
    const resize = mount();

    for (const width of [1400, 800, 700, 620, 400]) {
      resize(width);
      openMenu();
      for (const item of ITEMS) {
        // Once as a control or once as a menu item — the row and the menu
        // partition the list, they do not each get a copy.
        expect(screen.getAllByText(item.label)).toHaveLength(1);
      }
      expect(screen.getAllByText(DELETE_LABEL)).toHaveLength(1);
      expect(screen.getAllByText(CAPTURE_LABEL)).toHaveLength(1);
      closeMenu();
    }
  });

  it("keeps the menu's own order around the items the row gave back", () => {
    const resize = mount();

    resize(620);
    // Properties, History and Show in Files came back to the menu and landed
    // above the item this mechanism has never heard of, because the ORDER is
    // the caller's and not the group's.
    expect(openMenu()).toEqual([
      "Properties",
      "History",
      "Show in Files",
      CAPTURE_LABEL,
      DELETE_LABEL,
    ]);
  });

  it("gives a promoted control and its menu item the same handler", () => {
    const resize = mount();

    resize(1400);
    fireEvent.click(screen.getByRole("button", { name: "History" }));
    expect(picked).toEqual(["history"]);

    resize(400);
    const items = screen.getByRole("button", { name: "Actions" });
    fireEvent.pointerDown(items, { button: 0, ctrlKey: false });
    fireEvent.click(items);
    fireEvent.click(screen.getByRole("menuitem", { name: "History" }));
    expect(picked).toEqual(["history", "history"]);
  });

  it("leaves the row at 46.5's shape when nothing ever observes it", () => {
    // No hand-fired observer at all: this is what `src/test/setup.ts` gives
    // every other suite in the repository, and it is why none of them saw a
    // control appear underneath them. The budget stays zero, so the group is
    // the one control and the menu that 46.5 shipped.
    restoreWidths = withActionWidths(WIDTH);
    render(<Harness />);

    expect(promoted()).toEqual([]);
    expect(openMenu()).toEqual([
      "Attachments",
      "Properties",
      "History",
      "Show in Files",
      CAPTURE_LABEL,
      DELETE_LABEL,
    ]);
  });
});

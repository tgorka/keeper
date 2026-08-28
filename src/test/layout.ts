/**
 * The crudest possible layout engine, for the assertions jsdom cannot carry
 * (Story 44.12).
 *
 * jsdom performs no layout, so `scrollWidth` and `clientWidth` are hard-coded
 * zero on every element. A component that asks "is this text wider than its
 * box" therefore always hears "no" — which means an untreated jsdom cannot
 * distinguish a truncation affordance that works from one that never appears,
 * and epic 43 shipped exactly that class of defect twice.
 *
 * This does not stub the component or its hook. It answers the two DOM
 * properties the real code reads, so the real effect runs, the real
 * `scrollWidth > clientWidth` comparison runs, the real conditional render runs
 * and the real popover mounts. What is modelled is the browser, not keeper.
 *
 * The model applies ONLY to elements carrying Tailwind's `truncate` — which is
 * precisely the set of elements that can produce an ellipsis, so nothing else
 * in the tree (Radix's positioning, a virtualiser's viewport) has its geometry
 * quietly redefined underneath it.
 *
 * What this cannot prove is named in each caller: whether the CSS that produces
 * the ellipsis is applied at all, and whether the real font makes a real value
 * overflow a real pane. Those are browser facts and this is not a browser.
 */
import { PANE_HEADER_FRAME_SLOT, PANE_HEADER_STATUS_SLOT } from "@/components/layout/pane-header";
import { PRIORITY_ACTION_ATTR, PRIORITY_ACTIONS_SLOT } from "@/components/layout/priority-actions";
import { WINDOW_ROW_ATTR, WINDOW_VIEWPORT_ATTR } from "@/components/ui/window-list";

/** Every glyph is this wide. A monospace font, in a world with one font. */
export const TEST_CHAR_PX = 8;

/** One line's height, in the same world where every glyph is the same width. */
export const TEST_LINE_PX = 16;

/**
 * Make `truncate` elements report a width derived from their own text and a
 * fixed available width. Returns the undo, which the caller MUST run —
 * `Element.prototype` is shared with every other test in the file.
 */
export function withTextLayout(availablePx: number): () => void {
  const descriptors = {
    scrollWidth: Object.getOwnPropertyDescriptor(Element.prototype, "scrollWidth"),
    clientWidth: Object.getOwnPropertyDescriptor(Element.prototype, "clientWidth"),
  };

  Object.defineProperty(Element.prototype, "scrollWidth", {
    configurable: true,
    get(this: Element) {
      return this.classList.contains("truncate")
        ? (this.textContent ?? "").length * TEST_CHAR_PX
        : 0;
    },
  });
  Object.defineProperty(Element.prototype, "clientWidth", {
    configurable: true,
    get(this: Element) {
      return this.classList.contains("truncate") ? availablePx : 0;
    },
  });

  return () => {
    for (const [name, descriptor] of Object.entries(descriptors)) {
      if (descriptor === undefined) {
        Reflect.deleteProperty(Element.prototype, name);
      } else {
        Object.defineProperty(Element.prototype, name, descriptor);
      }
    }
  };
}

/**
 * Pin one element's box, for the pointer arithmetic a drag does.
 *
 * The suite-wide shim in `setup.ts` answers every zero-sized rect with one full
 * viewport at the origin, which is right for a virtualiser and useless for a
 * seam whose whole job is to sit at x = 160. Overriding the instance rather
 * than the prototype keeps that shim intact for everything else on screen.
 */
export function withRect(element: Element, left: number, width = 0): void {
  Object.defineProperty(element, "getBoundingClientRect", {
    configurable: true,
    value: () =>
      ({
        x: left,
        y: 0,
        left,
        right: left + width,
        top: 0,
        bottom: 0,
        width,
        height: 0,
        toJSON: () => ({}),
      }) as DOMRect,
  });
}

/**
 * Give `Range` the client rects jsdom does not implement, for a real
 * `EditorView` (Story 45.4).
 *
 * CodeMirror measures its default character width and line height by putting a
 * `Range` over a probe and calling `getClientRects()`. jsdom has no such method
 * at all, so the measure pass — which runs on an animation frame, ANY animation
 * frame that happens to elapse during a test — throws
 * `textRange(...).getClientRects is not a function`. The throw lands outside
 * every `try` a test can write, is reported as an unhandled error, and takes
 * the run's exit code with it whether or not a single assertion failed. Worse,
 * whether it happens at all depends on how many milliseconds the test spent, so
 * a suite is green until it is slow.
 *
 * The rects are a monospace fiction: one glyph {@link TEST_CHAR_PX} wide and
 * one line {@link TEST_LINE_PX} tall. What is modelled is the browser, not
 * keeper — nothing here is asserted on, and no test may claim a pixel it read
 * back out of this.
 *
 * Returns the undo, which the caller MUST run: `Range.prototype` is shared with
 * every other test in the file.
 */
export function withRangeRects(): () => void {
  const proto = Range.prototype as Range & {
    getClientRects?: () => DOMRectList;
    getBoundingClientRect?: () => DOMRect;
  };
  const hadRects = proto.getClientRects;
  const hadBox = proto.getBoundingClientRect;
  const rect = {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: TEST_CHAR_PX,
    bottom: TEST_LINE_PX,
    width: TEST_CHAR_PX,
    height: TEST_LINE_PX,
    toJSON: () => ({}),
  } as DOMRect;
  const list = Object.assign([rect], {
    item: (index: number) => (index === 0 ? rect : null),
  }) as unknown as DOMRectList;

  proto.getClientRects = () => list;
  proto.getBoundingClientRect = () => rect;
  // The undo restores the PREVIOUS implementation when there was one, and
  // otherwise leaves this one in place rather than deleting the property.
  //
  // Deleting it is what a symmetric teardown would do, and it is wrong here.
  // CodeMirror measures in an animation frame; a frame still in flight when
  // `afterAll` runs finds `undefined` and throws out of
  // `DocView.measureTextSize`, where no `try` in the test can catch it — so it
  // takes the run's exit code while the summary still prints passes. Whether a
  // frame is in flight is decided by how busy the box is, which is why it
  // presented as a suite that was green until eight agents ran at once.
  //
  // Leaving the stub costs nothing: vitest isolates per test file (measured
  // twice with a two-file probe, one of them inverted to confirm the probe was
  // sensitive), so `Range.prototype` is clean at every file's start and nothing
  // can inherit this. The undo exists for a file that had a real implementation
  // to put back, which is the only case where leaving ours would be a lie.
  return () => {
    if (hadRects !== undefined) {
      proto.getClientRects = hadRects;
    }
    if (hadBox !== undefined) {
      proto.getBoundingClientRect = hadBox;
    }
  };
}

/** The installed scrolling box: a way to scroll it, and the undo the caller
 * MUST run — `Element.prototype` is shared with every other test in the file. */
export interface ListGeometry {
  scrollTo: (element: Element, top: number) => void;
  undo: () => void;
}

/**
 * A scrolling box with real numbers in it, for the assertions a windowed list
 * needs (Story 44.10).
 *
 * jsdom is worse here than merely unlaid-out. `clientHeight` is hard-coded
 * zero, `scrollTop`'s setter is a no-op that always reads back zero, and no
 * scroll event is ever dispatched. A list that renders the window under the
 * scroll position therefore renders its FIRST window forever — so "scrolling
 * reaches the last row" passes for the wrong reason, having never scrolled, and
 * "only a bounded number of rows mount" passes on a list that would mount all
 * ten thousand in a browser. Both are assertions about jsdom, not about keeper.
 *
 * This answers the three properties the window reads, and only for the two
 * kinds of element the window marks — the viewport and a mounted row — so
 * nothing else on screen has its geometry quietly redefined. What is modelled
 * is the browser, not keeper: the real scroll handler runs, the real binary
 * search runs, the real window is what mounts.
 *
 * What this cannot prove is named in each caller: whether a row's real height
 * at a real font is what the estimate says, and whether the browser's own
 * scroll anchoring moves the list when a measurement changes the total height.
 */
export function withListGeometry(sizes: { viewport: number; row: number }): ListGeometry {
  const descriptors = {
    clientHeight: Object.getOwnPropertyDescriptor(Element.prototype, "clientHeight"),
    scrollTop: Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop"),
  };
  const tops = new WeakMap<Element, number>();

  Object.defineProperty(Element.prototype, "clientHeight", {
    configurable: true,
    get(this: Element) {
      if (this.hasAttribute(WINDOW_VIEWPORT_ATTR)) {
        return sizes.viewport;
      }
      return this.hasAttribute(WINDOW_ROW_ATTR) ? sizes.row : 0;
    },
  });
  Object.defineProperty(Element.prototype, "scrollTop", {
    configurable: true,
    get(this: Element) {
      return tops.get(this) ?? 0;
    },
    set(this: Element, value: number) {
      tops.set(this, value);
    },
  });

  return {
    scrollTo: (element, top) => {
      element.scrollTop = top;
      element.dispatchEvent(new Event("scroll"));
    },
    undo: () => {
      for (const [name, descriptor] of Object.entries(descriptors)) {
        if (descriptor === undefined) {
          Reflect.deleteProperty(Element.prototype, name);
        } else {
          Object.defineProperty(Element.prototype, name, descriptor);
        }
      }
    },
  };
}

/**
 * A width for every element a priority-overflow header measures, and nothing
 * else (Story 48.5).
 *
 * The header decides how many of its controls fit by measuring three kinds of
 * thing: each candidate control, the two wrappers that hold what never moves,
 * and the reserved status slot. jsdom measures none of them — `src/test/
 * setup.ts` answers one whole viewport for every zero-sized element, so an
 * unaided suite sees a 1024px trigger beside a 1024px button in a 1024px row
 * and the arithmetic is meaningless. This answers a width the caller DECLARED
 * for exactly those elements and leaves every other element to the suite's own
 * shim, so a header test states its geometry instead of inheriting one.
 *
 * Keys are {@link PRIORITY_ACTION_ATTR} values, plus `leading`, `menu`,
 * `status` and `frame`. The two wrappers are found by their position in the
 * group rather than by an attribute added to the product for this file's
 * benefit; the two reserved slots are found by the `data-slot` the product
 * already carries.
 *
 * Returns the undo, which the caller MUST run — `Element.prototype` is shared
 * with every other test in the file.
 *
 * What it cannot prove is the only thing it invents: whether the real font in
 * the real 560px window produces widths anything like these.
 */
export function withActionWidths(widths: Record<string, number>): () => void {
  const real = Element.prototype.getBoundingClientRect;
  const box = (width: number): DOMRect =>
    ({
      width,
      height: TEST_LINE_PX * 2,
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: width,
      bottom: TEST_LINE_PX * 2,
      toJSON: () => ({}),
    }) as DOMRect;

  Element.prototype.getBoundingClientRect = function declared(this: Element): DOMRect {
    const action = this.getAttribute(PRIORITY_ACTION_ATTR);
    if (action !== null && widths[action] !== undefined) {
      return box(widths[action]);
    }
    if (this.getAttribute("data-slot") === PANE_HEADER_STATUS_SLOT && widths.status !== undefined) {
      return box(widths.status);
    }
    if (this.getAttribute("data-slot") === PANE_HEADER_FRAME_SLOT && widths.frame !== undefined) {
      return box(widths.frame);
    }
    const group = this.parentElement;
    if (group?.getAttribute("data-slot") === PRIORITY_ACTIONS_SLOT) {
      if (this === group.lastElementChild && widths.menu !== undefined) {
        return box(widths.menu);
      }
      if (this === group.firstElementChild && widths.leading !== undefined) {
        return box(widths.leading);
      }
    }
    return real.call(this);
  };
  return () => {
    Element.prototype.getBoundingClientRect = real;
  };
}

/**
 * A `ResizeObserver` that fires when the test says so and never otherwise
 * (Story 48.5).
 *
 * `src/test/setup.ts` installs one that records nothing and delivers nothing,
 * because Radix's popper only needs the constructor to exist. That is exactly
 * why an unaided jsdom leaves a self-sizing header at its narrowest shape — and
 * why a suite that wants to see the header at 1400px has to deliver the
 * observation itself.
 *
 * `resize` reaches only the observations taken on a `<header>`: Radix observes
 * elements too, and handing its callback an invented entry would be a test of
 * Radix. Callers wrap the call in `act`.
 */
export function withHandFiredResize(): {
  resize: (width: number) => void;
  undo: () => void;
} {
  const seen: { target: Element; callback: ResizeObserverCallback }[] = [];
  const previous = globalThis.ResizeObserver;
  globalThis.ResizeObserver = class implements ResizeObserver {
    constructor(private readonly callback: ResizeObserverCallback) {}
    observe(target: Element): void {
      seen.push({ target, callback: this.callback });
    }
    unobserve(): void {}
    disconnect(): void {}
  };
  return {
    resize: (width: number) => {
      for (const { target, callback } of seen) {
        if (target.tagName !== "HEADER") {
          continue;
        }
        callback(
          [
            {
              target,
              contentRect: { width, height: TEST_LINE_PX * 2 } as DOMRectReadOnly,
            } as ResizeObserverEntry,
          ],
          {} as ResizeObserver,
        );
      }
    },
    undo: () => {
      globalThis.ResizeObserver = previous;
    },
  };
}

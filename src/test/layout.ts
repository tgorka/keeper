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

/** Every glyph is this wide. A monospace font, in a world with one font. */
export const TEST_CHAR_PX = 8;

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

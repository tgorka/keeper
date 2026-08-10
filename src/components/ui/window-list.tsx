/**
 * One window over a long list, hand-rolled (Story 44.10, FR-165, AD-84).
 *
 * A vault of ten thousand notes and a folder of a thousand photos are ordinary
 * here, and a surface that mounts every row is one that stops responding on the
 * machine with the most to show. This hook answers exactly one question — which
 * indices are on screen — and leaves every other decision (what a row looks
 * like, what the arrow keys mean, which row is selected) with the list that
 * owns it. Three lists sharing one window is one place for this to be wrong
 * instead of three.
 *
 * **Rows are measured, not assumed, and that is not gold-plating.** A fixed row
 * height is far simpler and it is a lie the moment a row wraps. Two of the three
 * callers wrap: a recordings row wraps its tag badges onto a third line and
 * grows a monospace path line wherever the platform has no Finder, and the Files
 * tree interleaves prose rows — "this folder is empty", the absent-drive
 * sentence composed in Rust — which wrap at any narrow pane width. Only the
 * notes list is genuinely uniform. So `rowHeight` is the height a row is
 * ASSUMED to be until it has been mounted once; the measurement then replaces
 * it, and is remembered by KEY rather than by index, so a re-sorted or
 * re-filtered list carries each row's geometry with the row instead of leaving
 * the previous occupant's height behind at that position.
 *
 * **Two rows outside the viewport stay mounted on purpose**, and each prevents a
 * specific way virtualising a keyboard-navigable list destroys it in silence:
 *
 *   - `pinnedIndex` is the roving tab stop. Exactly one row carries
 *     `tabIndex=0`; unmount it and the list has NO row in the tab order, so Tab
 *     skips the entire surface and there is no way in from the keyboard at all.
 *   - the row `reveal` last moved to. Focus lives on a DOM node. Unmount a
 *     focused row and focus falls to `<body>`, where the list's key handler
 *     never sees the next arrow press.
 *
 * **`reveal` does not wait a frame.** "Scroll, `requestAnimationFrame`, hope the
 * row mounted" is a race that passes on a fast machine and loses on a loaded
 * one. Here the target index is forced into the render outright, and the
 * caller's `onReveal` runs in the effect after that commit — the row is mounted
 * by construction, not by timing.
 *
 * jsdom lays nothing out: `clientHeight` and `scrollTop` are hard-coded zero and
 * a scroll event never fires. A measurement of zero therefore means "this
 * environment did not lay anything out", the estimate stands, and the list
 * behaves as a fixed-height window. `withListGeometry` in `src/test/layout.ts`
 * is what gives a test a scrolling box with real numbers in it.
 */
import {
  type CSSProperties,
  type RefCallback,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

/** Marks the scroll container. The test geometry model answers its height. */
export const WINDOW_VIEWPORT_ATTR = "data-window-viewport";

/** Marks one mounted row, and carries its index. Counting these IS the AD-84
 * assertion: a list that renders the vault has as many as the vault has rows. */
export const WINDOW_ROW_ATTR = "data-window-row";

/** Rows rendered beyond each edge, so a flick does not expose blank space
 * before the next render lands. */
const DEFAULT_OVERSCAN = 6;

/** The viewport height assumed until the scroll element reports its own — about
 * one screen. Layout replaces it immediately in a browser; jsdom never does, so
 * a test that arranges no geometry still gets a window with rows in it rather
 * than an empty list. */
const ASSUMED_VIEWPORT_HEIGHT = 640;

/** One row inside the window: where it is, and what it is. */
export interface WindowedRow<K> {
  /** Its position in the caller's data, not in the DOM. */
  index: number;
  key: K;
  /** Pixels from the top of the whole list. */
  start: number;
}

/** Goes on the element that wraps one row. */
export interface WindowedRowProps {
  ref: RefCallback<HTMLElement>;
  style: CSSProperties;
  [WINDOW_ROW_ATTR]: number;
}

/** Goes on the scroll container. */
export interface WindowedViewportProps {
  ref: RefCallback<HTMLElement>;
  [WINDOW_VIEWPORT_ATTR]: true;
}

export interface WindowedRowsOptions<K> {
  count: number;
  /**
   * A row's identity, stable across a re-order. Measurements hang off this, so
   * a key that is really an index throws every measurement away on every sort.
   * Keep it referentially stable (`useCallback`): its identity is what tells
   * the geometry the rows changed.
   */
  getKey: (index: number) => K;
  /** The height a row is assumed to be until it has been mounted once. */
  rowHeight: number;
  /** Space between rows, folded into each row's box as bottom padding — flex
   * `gap` does not apply to absolutely positioned children. */
  gap?: number;
  overscan?: number;
  /** The roving tab stop, kept mounted however far away it is scrolled. */
  pinnedIndex?: number;
  /** Runs after {@link WindowedRows.reveal}'s row is mounted. Where the caller
   * moves focus — focus policy stays with the list that owns the tab order. */
  onReveal?: (index: number) => void;
}

export interface WindowedRows<K> {
  viewportProps: WindowedViewportProps;
  /** The height the whole list would be. Goes on the positioned container. */
  totalSize: number;
  /** Ascending, and the only rows that may be rendered. Includes the pinned and
   * revealed rows, which may be nowhere near the viewport. */
  rows: WindowedRow<K>[];
  /**
   * The last index the viewport actually shows, ignoring overscan and the rows
   * kept mounted for focus. A caller paging in more rows must ask this and not
   * the tail of {@link rows}: a tab stop pinned near the end of the list would
   * otherwise read as "the viewport has reached the end" from the very top.
   */
  lastVisible: number;
  rowProps: (row: WindowedRow<K>) => WindowedRowProps;
  /** Scroll `index` into view, mount it, then run `onReveal` on it. */
  reveal: (index: number) => void;
}

/** The last index whose top is at or above `position`. */
function indexAt(offsets: Float64Array, count: number, position: number): number {
  let low = 0;
  let high = count - 1;
  let best = 0;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (offsets[mid] <= position) {
      best = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return best;
}

/**
 * Where every index starts, and — at `[count]` — where the list ends.
 *
 * Split out of the hook, with {@link windowSlice}, because Story 44.15's
 * gallery is a CodeMirror widget: imperative DOM in the editor's React-free
 * lazy chunk, which cannot call a hook and must not mount a React root to get
 * one. Two bindings over one piece of arithmetic keeps the promise 44.10 made
 * — that "which indices are on screen" is answered in exactly one place — while
 * letting the surfaces that ask differ. The React hook adds measurement, a
 * ResizeObserver and a scroll subscription on top; a grid of uniform tiles
 * needs none of those and supplies a constant `heightAt`.
 */
export function rowOffsets(count: number, heightAt: (index: number) => number): Float64Array {
  const out = new Float64Array(count + 1);
  let running = 0;
  for (let index = 0; index < count; index += 1) {
    out[index] = running;
    running += heightAt(index);
  }
  out[count] = running;
  return out;
}

/** Which indices a window over `offsets` may render, and where its viewport
 *  really ends. */
export interface WindowSlice {
  /** Ascending, and the only indices that may be rendered. Includes anything
   *  `forced` named, however far off screen it is. */
  indices: number[];
  /** The last index the viewport actually shows, ignoring overscan and the
   *  forced rows. `-1` for an empty list. */
  lastVisible: number;
}

/**
 * The window over a laid-out list: what is on screen, plus the overscan, plus
 * whatever the caller insists stays mounted.
 *
 * `forced` carries indices that must be rendered wherever they are, and
 * `undefined` entries are ignored so a caller can pass optional slots without
 * filtering first. Each one prevents a specific way virtualising a list
 * destroys it in silence — the roving tab stop, and the row that holds focus —
 * and they are inserted in index order so the result stays ascending, which is
 * what a positioned container needs to paint in one pass.
 */
export function windowSlice(
  offsets: Float64Array,
  count: number,
  top: number,
  height: number,
  overscan: number,
  forced: readonly (number | undefined)[] = [],
): WindowSlice {
  if (count === 0) {
    return { indices: [], lastVisible: -1 };
  }
  const first = indexAt(offsets, count, top);
  let last = first;
  while (last + 1 < count && offsets[last + 1] < top + height) {
    last += 1;
  }
  const from = Math.max(0, first - overscan);
  const to = Math.min(count - 1, last + overscan);

  const indices: number[] = [];
  for (const extra of forced) {
    if (extra !== undefined && extra >= 0 && extra < from && !indices.includes(extra)) {
      indices.push(extra);
    }
  }
  indices.sort((a, b) => a - b);
  for (let index = from; index <= to; index += 1) {
    indices.push(index);
  }
  for (const extra of forced) {
    if (extra !== undefined && extra > to && extra < count && !indices.includes(extra)) {
      indices.push(extra);
    }
  }
  return { indices, lastVisible: last };
}

export function useWindowedRows<K>({
  count,
  getKey,
  rowHeight,
  gap = 0,
  overscan = DEFAULT_OVERSCAN,
  pinnedIndex,
  onReveal,
}: WindowedRowsOptions<K>): WindowedRows<K> {
  const viewport = useRef<HTMLElement | null>(null);
  const measured = useRef(new Map<K, number>());
  const keyOfElement = useRef(new WeakMap<Element, K>());
  const rowSizes = useRef<ResizeObserver | null>(null);
  const [revision, setRevision] = useState(0);
  const [view, setView] = useState({ top: 0, height: ASSUMED_VIEWPORT_HEIGHT });
  const [anchor, setAnchor] = useState<{ index: number; token: number } | null>(null);
  const token = useRef(0);
  const reveals = useRef(onReveal);
  const latest = useRef({ offsets: new Float64Array(1), count: 0, view });

  useEffect(() => {
    reveals.current = onReveal;
  }, [onReveal]);

  /**
   * Record what a row actually measured.
   *
   * Zero is not a measurement — it is what every element reports where nothing
   * has been laid out — so it leaves the estimate in place rather than
   * collapsing the whole list to nothing.
   */
  const measure = useCallback((key: K, element: HTMLElement) => {
    const height = element.clientHeight;
    if (height <= 0 || measured.current.get(key) === height) {
      return;
    }
    measured.current.set(key, height);
    setRevision((value) => value + 1);
  }, []);

  // The row observer is built lazily by the first row that mounts, and is the
  // only one: a per-row observer on a ten-thousand-row list is ten thousand
  // observers.
  useEffect(
    () => () => {
      rowSizes.current?.disconnect();
      rowSizes.current = null;
    },
    [],
  );

  const attachViewport = useCallback<RefCallback<HTMLElement>>((element) => {
    viewport.current = element;
    if (element === null) {
      return;
    }
    const sync = () => {
      const height = element.clientHeight;
      const next = {
        top: element.scrollTop,
        height: height > 0 ? height : ASSUMED_VIEWPORT_HEIGHT,
      };
      setView((prev) => (prev.top === next.top && prev.height === next.height ? prev : next));
    };
    sync();
    element.addEventListener("scroll", sync, { passive: true });
    const resizes = new ResizeObserver(sync);
    resizes.observe(element);
    return () => {
      element.removeEventListener("scroll", sync);
      resizes.disconnect();
      viewport.current = null;
    };
  }, []);

  const box = rowHeight + gap;

  // `revision` in the dependency list is a re-run trigger, not a read: a new
  // measurement mutates the store in place and bumps it, and that bump is what
  // makes these offsets a function of everything measured so far.
  const offsets = useMemo(
    () => rowOffsets(count, (index) => measured.current.get(getKey(index)) ?? box),
    [count, getKey, box, revision],
  );

  const totalSize = offsets[count];

  useEffect(() => {
    latest.current = { offsets, count, view };
  });

  const reveal = useCallback((index: number) => {
    const { offsets: current, count: total, view: seen } = latest.current;
    if (index < 0 || index >= total) {
      return;
    }
    const top = current[index];
    const bottom = current[index + 1];
    let next = seen.top;
    if (top < seen.top) {
      next = top;
    } else if (bottom > seen.top + seen.height) {
      next = bottom - seen.height;
    }
    // Clamped here rather than trusted back from the element: jsdom's
    // `scrollTop` setter is a no-op and the browser silently clamps, so reading
    // the value back would give two different answers to the same request.
    next = Math.min(Math.max(next, 0), Math.max(0, current[total] - seen.height));
    if (next !== seen.top) {
      setView((prev) => ({ ...prev, top: next }));
      if (viewport.current !== null) {
        viewport.current.scrollTop = next;
      }
    }
    token.current += 1;
    setAnchor({ index, token: token.current });
  }, []);

  // The anchor index is deliberately never cleared: it is the row that has
  // focus, and it stays mounted until focus moves somewhere else.
  useEffect(() => {
    if (anchor !== null) {
      reveals.current?.(anchor.index);
    }
  }, [anchor]);

  const { rows, lastVisible } = useMemo(() => {
    const slice = windowSlice(offsets, count, view.top, view.height, overscan, [
      pinnedIndex,
      anchor?.index,
    ]);
    return {
      rows: slice.indices.map((index) => ({ index, key: getKey(index), start: offsets[index] })),
      lastVisible: slice.lastVisible,
    };
  }, [anchor, count, getKey, offsets, overscan, pinnedIndex, view]);

  const rowProps = useCallback(
    (row: WindowedRow<K>): WindowedRowProps => ({
      ref: (element) => {
        if (element === null) {
          return;
        }
        keyOfElement.current.set(element, row.key);
        measure(row.key, element);
        rowSizes.current ??= new ResizeObserver((entries) => {
          for (const entry of entries) {
            const changed = keyOfElement.current.get(entry.target);
            if (changed !== undefined) {
              measure(changed, entry.target as HTMLElement);
            }
          }
        });
        const observer = rowSizes.current;
        observer.observe(element);
        return () => {
          observer.unobserve(element);
          keyOfElement.current.delete(element);
        };
      },
      style: {
        position: "absolute",
        top: 0,
        left: 0,
        width: "100%",
        transform: `translateY(${row.start}px)`,
        paddingBottom: gap === 0 ? undefined : gap,
      },
      [WINDOW_ROW_ATTR]: row.index,
    }),
    [gap, measure],
  );

  return {
    viewportProps: { ref: attachViewport, [WINDOW_VIEWPORT_ATTR]: true },
    totalSize,
    rows,
    lastVisible,
    rowProps,
    reveal,
  };
}

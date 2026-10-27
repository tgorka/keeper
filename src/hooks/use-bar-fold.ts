import { type RefObject, useLayoutEffect } from "react";

/**
 * What an agent session's composer bar gives up, in this order, when its one row cannot hold
 * everything (R601-01): the context reading becomes its ring (`ring`), Send becomes an icon
 * (`send`), the chips tighten (`tight`), the effort chip says its level's short code (`effort`),
 * and the row's gaps, the model's chevron and the effort's brain go (`snug`). Only after all of
 * them does the model's name truncate. Reset is never in the bar; it lives in the popovers. A
 * control reads a step with `group-data-[fold~=<step>]/bar:` from the bar (`BAR_CLASS`).
 *
 * Width alone cannot order this: the same 335px bar holds `glm-5.3-flash` beside "Medium" and
 * cuts it beside "Extra high", so the bar measures its own row instead.
 */
export const BAR_FOLDS = ["ring", "send", "tight", "effort", "snug"] as const;

/** Marks text the bar keeps whole for as long as any step is left: the model's name. */
export const BAR_WHOLE = { "data-bar-whole": "" } as const;

/**
 * The bar's own classes: the group its controls read the steps from, and no transition while it
 * measures (`data-fitting`). The chips animate their padding (`transition-all`), so a step
 * measured mid-transition reads the old padding: on the 360px panel that folded "Medium" to `M`
 * where the tightened chips held it whole.
 */
export const BAR_CLASS = "group/bar data-fitting:**:transition-none";

/**
 * Whether `bar` holds its row: nothing overflows it and no `BAR_WHOLE` text is cut. A cut is
 * read at sub-pixel precision, text against its box: the integer `scrollWidth` and
 * `clientWidth` both read 80 for `glm-5.3-flash`'s 80.45px in an 80.42px box, which the
 * browser still ends with an ellipsis.
 */
function holds(bar: HTMLElement): boolean {
  if (bar.scrollWidth > bar.clientWidth) return false;
  const text = document.createRange();
  for (const box of bar.querySelectorAll<HTMLElement>("[data-bar-whole]")) {
    text.selectNodeContents(box);
    if (text.getBoundingClientRect().width > box.getBoundingClientRect().width) return false;
  }
  return true;
}

/**
 * Fold `bar` by the fewest steps that let it hold its row, from none each time, so a wider bar
 * unfolds again; refold when it resizes or what it shows changes. The steps are written straight
 * to `data-fold` before paint, never through React state: a re-render per step would flicker,
 * and a fold measured from a folded row could keep itself folded.
 */
export function useBarFold(bar: RefObject<HTMLElement | null>, enabled: boolean): void {
  useLayoutEffect(() => {
    const element = bar.current;
    if (!enabled || element === null) return;
    const refold = () => {
      element.dataset.fitting = "";
      for (let steps = 0; steps <= BAR_FOLDS.length; steps++) {
        const fold = BAR_FOLDS.slice(0, steps).join(" ");
        if (element.dataset.fold !== fold) element.dataset.fold = fold;
        if (holds(element)) break;
      }
      delete element.dataset.fitting;
    };
    refold();
    const resized = new ResizeObserver(refold);
    resized.observe(element);
    // A label, a Reset, a reading, or a control's classes or state that change later change what
    // the row needs. The refold's own writes are not such a change.
    const changed = new MutationObserver((records) => {
      const foreign = ({ attributeName: name }: MutationRecord) =>
        name !== "data-fold" && name !== "data-fitting";
      if (records.some(foreign)) refold();
    });
    changed.observe(element, {
      childList: true,
      subtree: true,
      characterData: true,
      attributes: true,
    });
    // A web font that lands after the first refold changes every word's width without a mutation
    // or a resize.
    const fonts: FontFaceSet | undefined = document.fonts; // jsdom has none
    fonts?.addEventListener("loadingdone", refold);
    return () => {
      resized.disconnect();
      changed.disconnect();
      fonts?.removeEventListener("loadingdone", refold);
      delete element.dataset.fold;
    };
  }, [bar, enabled]);
}

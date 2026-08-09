/**
 * The third step of AD-83: what still does not fit truncates, and the whole
 * value is one click away (Story 44.12, FR-168).
 *
 * The failure this replaces is `title={value}`. A native tooltip is hover-only,
 * so it does not exist for a keyboard, for a touch screen, or for anyone using
 * the app with a trackpad they are not currently resting a finger on; it
 * appears after a delay nobody chose, it cannot be scrolled, and the platform
 * silently clips a long one. The panel where the owner met this is full of them.
 *
 * The affordance here is a real button that opens a real popover holding the
 * COMPLETE value in a region that scrolls — a `files:` list of forty paths has
 * to be readable, not merely present.
 *
 * **It renders only when the value is actually cut.** That order matters: an
 * affordance on every value is a tab stop on every value, and a Properties panel
 * where Tab visits twenty buttons to reach the one control you wanted is a worse
 * surface than the one with the tooltips. The measurement is `scrollWidth >
 * clientWidth` on the rendered element, which is the browser reporting on the
 * text it actually painted — and it is also the thing jsdom cannot do, so read
 * the note in `properties-panel.test.tsx` about how that is exercised.
 */
import { Expand } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

/** The accessible name of a standalone overflow trigger, suffixed with the
 * value's name — otherwise a pane of them is the same control many times. */
export const OVERFLOW_TRIGGER_LABEL = "Show all of";

/** The heading inside the popover, so the panel says what it is showing. */
export const OVERFLOW_PANEL_LABEL = "Full value";

/**
 * Whether the attached element is painting less text than it holds.
 *
 * Re-read after every render rather than on a dependency list: the answer
 * changes when the value changes, when a column is dragged, when the window
 * resizes, and when a font finishes loading — and a stale `false` here is a
 * value with no way to read it, which is the bug. The read is two property
 * accesses and the state only changes when the boolean flips, so re-running it
 * costs nothing worth naming.
 *
 * A callback ref, not a `RefObject`, and for a type reason worth stating: a
 * mutable ref is invariant, so a `RefObject<HTMLElement | null>` cannot be
 * handed to a `<span>` or a `<button>` without a cast at every call site. A
 * callback accepting the wider element assigns to both.
 */
export function useOverflowing(): {
  ref: (element: HTMLElement | null) => void;
  overflowing: boolean;
} {
  const element = useRef<HTMLElement | null>(null);
  const [overflowing, setOverflowing] = useState(false);

  useLayoutEffect(() => {
    const measured = element.current;
    if (measured !== null) {
      setOverflowing(measured.scrollWidth > measured.clientWidth);
    }
  });

  // Keyed on `overflowing` because crossing the threshold swaps the element
  // itself — the inert span becomes the trigger — and an observer still
  // watching the detached one reports on nothing for the rest of the session.
  useEffect(() => {
    const measured = element.current;
    if (measured === null || typeof ResizeObserver === "undefined") {
      return;
    }
    // A pane resize moves no React state, so nothing would re-render and the
    // layout effect above would never re-run. This is the door for that case.
    const observer = new ResizeObserver(() => {
      setOverflowing(measured.scrollWidth > measured.clientWidth);
    });
    observer.observe(measured);
    return () => observer.disconnect();
  }, [overflowing]);

  return {
    ref: useCallback((next: HTMLElement | null) => {
      element.current = next;
    }, []),
    overflowing,
  };
}

interface FullValuePanelProps {
  /** What the value is, for the panel's heading and the trigger's name. */
  name: string;
  /** The complete value. Never a truncation — that is the whole point. */
  value: string;
  /** Render the value in the monospace face the surface shows it in. */
  monospace?: boolean;
}

/**
 * The popover body: the complete value, wrapped and scrollable.
 *
 * `whitespace-pre-wrap` and `break-words` together, because the two values that
 * reach here are a long path with no spaces (which needs breaking mid-token or
 * it overflows again) and a multi-line block (which needs its newlines kept).
 *
 * The scroll region carries its own `tabIndex` so a keyboard user can page
 * through a value taller than the panel. Radix moves focus into the content on
 * open, but focus on a non-focusable wrapper does not scroll with the arrow
 * keys, and a full value you cannot reach the bottom of is not a full value.
 */
function FullValuePanel({ name, value, monospace }: FullValuePanelProps) {
  return (
    <>
      <p className="font-medium text-muted-foreground text-xs">{name}</p>
      <div
        tabIndex={0}
        data-slot="overflow-full-value"
        aria-label={`${OVERFLOW_PANEL_LABEL}: ${name}`}
        className={cn(
          "max-h-64 overflow-y-auto whitespace-pre-wrap break-words text-xs",
          "outline-none focus-visible:ring-2 focus-visible:ring-ring",
          monospace === true && "font-mono",
        )}
      >
        {value}
      </div>
    </>
  );
}

/**
 * A standalone trigger that opens `value` in full.
 *
 * For the surfaces where the value's own click is already spoken for — a file
 * tree row, where clicking the name expands the folder — so the affordance
 * cannot be the text itself. Render it beside the truncating element, driven by
 * that element's {@link useOverflowing}.
 */
export function FullValueButton({
  name,
  value,
  monospace,
  tabIndex,
}: FullValuePanelProps & {
  /** Set to -1 where the surface runs a roving tab order, so the affordance
   * joins the tab sequence on the focused row only — a tree with a stop per
   * overflowing name is a tree Tab cannot get out of. */
  tabIndex?: number;
}) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          tabIndex={tabIndex}
          data-slot="overflow-trigger"
          aria-label={`${OVERFLOW_TRIGGER_LABEL} ${name}`}
          className="shrink-0 rounded-sm p-0.5 text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Expand aria-hidden="true" className="size-3" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 gap-1">
        <FullValuePanel name={name} value={value} monospace={monospace} />
      </PopoverContent>
    </Popover>
  );
}

interface OverflowValueProps extends FullValuePanelProps {
  /** Extra classes for the visible, truncating text. */
  className?: string;
}

/**
 * One line of text that fits, truncates, and offers the rest — in that order.
 *
 * While the text fits it is an inert `span`, with no tab stop and no glyph.
 * Once it does not, the same text becomes the button that opens it in full: the
 * affordance is the value, so there is nothing extra to find and the CSS
 * ellipsis the browser painted is the cue.
 *
 * The button keeps the value as its accessible name rather than taking a
 * "Show all of…" label. A screen reader walking the panel has to hear the
 * values, not a column of identical verbs — the popup is announced by
 * `aria-haspopup`, which Radix sets.
 */
export function OverflowValue({ name, value, monospace, className }: OverflowValueProps) {
  const { ref, overflowing } = useOverflowing();
  const text = cn("block min-w-0 truncate", monospace === true && "font-mono", className);

  if (!overflowing) {
    return (
      <span ref={ref} data-slot="overflow-value" className={text}>
        {value}
      </span>
    );
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          ref={ref}
          data-slot="overflow-value"
          data-overflowing="true"
          className={cn(
            text,
            "cursor-pointer text-left outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring",
          )}
        >
          {value}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 gap-1">
        <FullValuePanel name={name} value={value} monospace={monospace} />
      </PopoverContent>
    </Popover>
  );
}

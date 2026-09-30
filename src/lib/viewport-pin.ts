import { reportFrontendError } from "@/lib/crash-report";

/** How many of the boxes reaching below the window a report names. */
const MAX_NAMED = 8;

/** Why the viewport scrolled: the boxes that make the document taller than the window. */
function overhang(): string {
  const bottom = window.innerHeight;
  const named: string[] = [];
  for (const element of document.body.querySelectorAll("*")) {
    const box = element.getBoundingClientRect();
    const parent = element.parentElement?.getBoundingClientRect();
    // The outermost box that reaches below the window; its children say nothing new.
    if (box.height > 0 && box.bottom > bottom + 1 && (!parent || parent.bottom <= bottom + 1)) {
      const style = getComputedStyle(element);
      named.push(
        `${element.tagName.toLowerCase()}.${String(element.className).slice(0, 80)} bottom=${Math.round(box.bottom)} position=${style.position}`,
      );
      if (named.length === MAX_NAMED) {
        break;
      }
    }
  }
  return named.length === 0 ? "nothing reaches below the window" : named.join("; ");
}

/**
 * Keep the main window's document from ever scrolling.
 *
 * The app is one fixed-height shell whose panes scroll themselves; the document
 * behind it has nothing to scroll. On 2026-09-30 it scrolled anyway — the owner
 * saw every pane shifted up with an empty band below and a window-sized
 * scrollbar — and nothing in the mock shell reproduced it. So the document gets
 * no overflow, a scroll that still happens is put back at once, and the boxes
 * that made it possible go to the app log (`frontend error: … source=layout`),
 * so the next time is a log line rather than a guess.
 *
 * Main window only: the print and capture windows share `index.css` and a
 * printed note must be allowed to flow.
 */
export function pinViewport(): void {
  document.documentElement.style.overflow = "hidden";
  document.documentElement.style.height = "100%";
  document.body.style.overflow = "hidden";
  document.body.style.height = "100%";
  window.addEventListener(
    "scroll",
    () => {
      if (window.scrollX === 0 && window.scrollY === 0) {
        return;
      }
      reportFrontendError(
        "layout",
        new Error(`the window scrolled to ${window.scrollY}px: ${overhang()}`),
        null,
      );
      window.scrollTo(0, 0);
    },
    { passive: true },
  );
}

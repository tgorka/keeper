/**
 * The minimal replacement that turns one string into another.
 *
 * **Lifted out of `live-preview.ts` to break a real import cycle (Story
 * 45.18).** `live-preview` imports `tableLayer` from `markdown-table`, and
 * `markdown-table` imported `spliceBetween` back out of `live-preview` — so
 * whichever of the two the bundler evaluated second found the first one's
 * bindings still in the temporal dead zone. Under Vite that surfaced as
 * `Cannot access '__vite_ssr_import_N__' before initialization`, thrown out of
 * `livePreview` at `mermaidLayer()` — which is simply the next binding in the
 * returned array and had nothing to do with mermaid at all.
 *
 * It was intermittent because which module loads first depends on which surface
 * the process reached first, and Story 45.18 made it common by giving the
 * markdown preview a second host: a `.md` file opened from the Files pane now
 * mounts the preview where it previously refused.
 *
 * **This module imports nothing**, which is what makes it a safe shared bottom
 * and what stops the cycle coming back.
 */

export interface TextSplice {
  /** Start of the replaced span in the old text. */
  from: number;
  /** End of the replaced span in the old text. */
  to: number;
  /** What replaces it. */
  insert: string;
}

/**
 * The single minimal replacement that turns `before` into `after`, or null when
 * they are identical.
 *
 * Minimal matters twice over: CodeMirror maps the caret and the selection
 * through the change, so replacing only what actually moved is what keeps the
 * caret still when an agent appends a section somewhere else in the file; and
 * the same span is what gets the fading highlight, so the user sees where the
 * change landed rather than a whole-document flash.
 */
export function spliceBetween(before: string, after: string): TextSplice | null {
  if (before === after) {
    return null;
  }
  const shortest = Math.min(before.length, after.length);
  let start = 0;
  while (start < shortest && before[start] === after[start]) {
    start += 1;
  }
  let endBefore = before.length;
  let endAfter = after.length;
  while (endBefore > start && endAfter > start && before[endBefore - 1] === after[endAfter - 1]) {
    endBefore -= 1;
    endAfter -= 1;
  }
  return { from: start, to: endBefore, insert: after.slice(start, endAfter) };
}

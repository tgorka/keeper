/**
 * A `/` in the middle of a message — the mechanics, and only the mechanics
 * (Story 101.3, R418).
 *
 * Which token the caret is at and what it offers are Rust's
 * (`keeper_core::chat::inline`, through `chatInlinePreview`): mentions of
 * other agents and drive files in an agent room, `/shrug` everywhere. This
 * module answers the two questions a textarea answers locally:
 *
 * 1. **Is it worth asking?** {@link inlineCandidate} — the caret right after a
 *    `/` that follows whitespace, with nothing glued after the caret. A bound
 *    that keeps a keystroke of prose from becoming a round trip, never the
 *    rule: Rust finds the token again and answers `null` when there is none.
 * 2. **What does the draft become?** {@link inserted} — the token Rust named
 *    replaced by the row's insert, and nothing else of the draft touched.
 *
 * A draft that is a whole-line command (`isCommandLine`) belongs to the
 * whole-line menu; the composer asks this one only otherwise.
 */

/** The longest query after the slash worth asking about (Rust's bound too). */
export const MAX_INLINE_QUERY = 64;

/** What a composer says when it could not ask Rust what a mid-text `/` offers (R381). */
export const INLINE_PREVIEW_FAILED = "keeper couldn't look up what / offers here.";

/** The menu's accessible name. */
export const INLINE_MENU_LABEL = "Insert";

/**
 * Whether the caret, between `before` and `after`, could be at a mid-text
 * token: a `/` after whitespace (never the draft's first character), then no
 * whitespace and no other slash up to the caret, and the caret at a word's end.
 */
export function inlineCandidate(before: string, after: string): boolean {
  if (after.length > 0 && !/^\s/.test(after)) {
    return false;
  }
  const slash = before.lastIndexOf("/");
  if (slash < 1 || !/\s/.test(before[slash - 1])) {
    return false;
  }
  const query = before.slice(slash + 1);
  return query.length <= MAX_INLINE_QUERY && !/\s/.test(query);
}

/**
 * The draft after a row is chosen: `token` — the end of `before` — replaced by
 * `insert`, followed by a space when the caret was at the draft's end so the
 * next word can be typed, and where the caret lands. `null` when `before` no
 * longer ends with the token (the field moved on).
 */
export function inserted(
  before: string,
  after: string,
  token: string,
  insert: string,
): { draft: string; caret: number } | null {
  if (!before.endsWith(token)) {
    return null;
  }
  const head = `${before.slice(0, before.length - token.length)}${insert}${after === "" ? " " : ""}`;
  return { draft: `${head}${after}`, caret: head.length };
}

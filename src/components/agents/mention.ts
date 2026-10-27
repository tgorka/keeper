/**
 * A leading `@` in an agents session's composer (109.24, Q109-21 a) — the
 * mechanics only. Which agents it offers, and what a sent `@` line does, are
 * Rust's (`keeper_core::hq::mention`, through `agentMentionPreview` and
 * `agentSendLine`); this answers only whether a keystroke is worth asking about.
 */

/** The longest handle query worth asking about (Rust's bound too). */
export const MAX_MENTION_QUERY = 64;

/** The `@` menu's accessible name. */
export const MENTION_MENU_LABEL = "Agents";

/** What the composer says when it could not ask Rust what `@` offers (R381). */
export const MENTION_PREVIEW_FAILED = "keeper couldn't look up which agents @ can bring in here.";

/**
 * Whether the caret, between `before` and `after`, is in the draft's leading `@`
 * word: `@` first (after any spaces), no whitespace up to the caret, and the caret
 * at the word's end. Rust finds the token again and answers `null` when there is
 * none; this bound only keeps prose from becoming a round trip per keystroke.
 */
export function mentionCandidate(before: string, after: string): boolean {
  if (after.length > 0 && !/^\s/.test(after)) {
    return false;
  }
  const token = before.trimStart();
  if (!token.startsWith("@")) {
    return false;
  }
  const query = token.slice(1);
  return query.length <= MAX_MENTION_QUERY && !/\s/.test(query);
}

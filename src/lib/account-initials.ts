/**
 * Shared account-initial derivation (Story 2.5, Story 4.6).
 *
 * One source for the single-letter avatar initial derived from a Matrix user id,
 * reused by the account-footer switcher and the conversation header's
 * account-initial chip so both surfaces stay consistent.
 */

/**
 * The first character of the user id (without the leading `@`), uppercased, as the
 * avatar initials fallback. Empty ids fall back to `?`.
 */
export function initials(userId: string): string {
  const stripped = userId.startsWith("@") ? userId.slice(1) : userId;
  const first = stripped.trim().charAt(0);
  return first ? first.toUpperCase() : "?";
}

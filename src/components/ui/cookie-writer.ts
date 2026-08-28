// SPDX-License-Identifier: Apache-2.0
/**
 * The one place `document.cookie` is assigned.
 *
 * **Why a module for one statement.** This codebase remembers small pieces of
 * UI state in cookies rather than `localStorage` — see `src/lib/column-widths.ts`
 * for the argument. Biome's `suspicious/noDocumentCookie` disagrees, and it is
 * switched off for `src/components/ui/**` and nowhere else. Before this file,
 * the convention that kept the two facts compatible was implicit: the pure
 * *builder* lives in `src/lib` where it can be unit-tested against a string,
 * and the impure *assignment* lives here. Story 45.1 and 45.4 each added a
 * store that assigned directly from `src/lib`, and each would have needed its
 * own suppression comment — which is how a rule ends up disabled in six files
 * and enforced in none.
 *
 * So: builders stay pure and testable, this is the only statement that writes,
 * and the suppression is written down once with its reason.
 */

/**
 * Write a fully-formed cookie assignment string.
 *
 * The caller composes the whole `name=value; path=/; max-age=...` string,
 * because the escaping and the expiry policy are the caller's decision and are
 * covered by the caller's own tests. This function exists to perform the
 * assignment, not to have an opinion about it.
 */
export function writeCookie(assignment: string): void {
  // biome-ignore lint/suspicious/noDocumentCookie: cookies are this codebase's
  // deliberate choice for small UI state (see the module doc and
  // src/lib/column-widths.ts); this is the single assignment site that choice
  // is allowed, so the rule stays on everywhere else.
  document.cookie = assignment;
}

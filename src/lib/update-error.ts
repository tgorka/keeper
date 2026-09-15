/**
 * The one place a thrown updater failure becomes a sentence.
 *
 * Shared by the two drivers of the update flow — the About section's manual
 * two-step control and the background loop in `use-auto-update` — so a failed
 * check reads the same whoever asked for it, and neither path can regress into
 * rendering `[object Object]`, `undefined`, or a dangling colon.
 */

/**
 * Extract a human-readable message from an unknown thrown value (never throws).
 * Falls back to a generic line for a non-string / empty / object-valued
 * `message`.
 */
export function updateErrorMessage(raw: unknown): string {
  if (typeof raw === "string" && raw.trim() !== "") {
    return raw;
  }
  if (typeof raw === "object" && raw !== null && "message" in raw) {
    const { message } = raw;
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return "Something went wrong.";
}

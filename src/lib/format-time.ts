/**
 * Chat-row timestamp formatting.
 *
 * Renders a room's latest-event timestamp (ms since the Unix epoch, UTC) for
 * the chat list: a same-day event shows the local clock time (`HH:MM`); an
 * older event shows a short local date. Uses the runtime locale via
 * `Intl.DateTimeFormat`.
 */

/**
 * Format a room timestamp (ms since the Unix epoch) for a chat row.
 *
 * - Today → localized `HH:MM` (e.g. `"14:03"`).
 * - Any other day → localized short date (e.g. `"Jul 2"` / `"02/07/2024"`).
 *
 * @param ms - Milliseconds since the Unix epoch (UTC).
 * @param now - Reference "now" in ms; defaults to `Date.now()` (injectable for tests).
 */
/**
 * The largest absolute time value a JavaScript `Date` can represent (±8.64e15
 * ms). A finite `origin_server_ts` from an untrusted homeserver can exceed this
 * (ruma's `UInt` reaches ~9.007e15), and `new Date(ms)` past this range makes
 * `Intl.DateTimeFormat.format` / `Date.toISOString` throw `RangeError` — which,
 * unguarded, would crash the render. Values beyond it format as "" (no time).
 */
const MAX_DATE_MS = 8.64e15;

export function formatRoomTimestamp(ms: number, now: number = Date.now()): string {
  if (!Number.isFinite(ms) || ms <= 0 || ms > MAX_DATE_MS) {
    return "";
  }
  const date = new Date(ms);
  const today = new Date(now);

  const sameDay =
    date.getFullYear() === today.getFullYear() &&
    date.getMonth() === today.getMonth() &&
    date.getDate() === today.getDate();

  if (sameDay) {
    return new Intl.DateTimeFormat(undefined, {
      hour: "2-digit",
      minute: "2-digit",
    }).format(date);
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(date);
}

/**
 * Format a message timestamp (ms since the Unix epoch) as a localized clock time
 * (`HH:MM`) for a timeline bubble. An invalid or non-positive input yields `""`.
 *
 * @param ms - Milliseconds since the Unix epoch (UTC).
 */
export function formatMessageTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0 || ms > MAX_DATE_MS) {
    return "";
  }
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

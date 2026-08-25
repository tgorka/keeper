/**
 * Timestamp and duration formatting for rows that show a time.
 *
 * Renders a room's latest-event timestamp (ms since the Unix epoch, UTC) for
 * the chat list: a same-day event shows the local clock time (`HH:MM`); an
 * older event shows a short local date. Uses the runtime locale via
 * `Intl.DateTimeFormat`.
 *
 * It also holds the Files pane's release countdown (Story 56.9, FR-343), which
 * is a duration and not a timestamp. A duration is rendered here rather than
 * composed in Rust because a countdown is stale the instant it is serialized:
 * the Files tree lists on demand and does not poll at all, so a figure shipped
 * from the backend would still read `"23 hr"` the next time the owner opened
 * the folder. Rust ships the moment a row becomes releasable; the frontend
 * renders the time left against its own clock.
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

/**
 * Format a draft's age (its `updatedTs`, ms since the Unix epoch) as a short,
 * localized relative string for the approval pane (Story 7.3).
 *
 * - < 1 min → `"just now"`.
 * - < 1 h → whole minutes (e.g. `"5 min ago"`) via `Intl.RelativeTimeFormat`.
 * - < 24 h → whole hours (e.g. `"2 hr ago"`).
 * - Older → a localized short date (`formatRoomTimestamp`'s date branch).
 *
 * A future or clock-skewed timestamp (ms > now) clamps to `"just now"`. An
 * invalid / non-positive / out-of-range input yields `""`.
 *
 * @param ms - The draft's `updatedTs` in ms since the Unix epoch (UTC).
 * @param now - Reference "now" in ms; defaults to `Date.now()` (injectable for tests).
 */
export function formatDraftAge(ms: number, now: number = Date.now()): string {
  if (!Number.isFinite(ms) || ms <= 0 || ms > MAX_DATE_MS) {
    return "";
  }
  const elapsedMs = now - ms;
  // Future / clock-skewed timestamps clamp to "just now" rather than "in 5 min".
  if (elapsedMs < 60_000) {
    return "just now";
  }
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "always", style: "short" });
  const minutes = Math.floor(elapsedMs / 60_000);
  if (minutes < 60) {
    return rtf.format(-minutes, "minute");
  }
  const hours = Math.floor(elapsedMs / 3_600_000);
  if (hours < 24) {
    return rtf.format(-hours, "hour");
  }
  // Older than a day → a short absolute date, reusing the chat-row date branch.
  return formatRoomTimestamp(ms, now);
}

/**
 * Format the time left before a release deadline, short enough for a Files
 * table cell (Story 56.9, FR-343).
 *
 * - Past due, or exactly now → `"due"`.
 * - < 1 min → whole seconds, floored (e.g. `"45s"`), never `"60s"`.
 * - < 1 h → whole minutes (e.g. `"12 min"`).
 * - < 24 h → whole hours (e.g. `"23 hr"`).
 * - Longer → whole days (`"1 day"` / `"6 days"`).
 *
 * The ladder is `formatSyncWaited`'s coarse house vocabulary (`sync-pane.tsx`)
 * inverted — time remaining, not elapsed — with seconds added in the last
 * minute because the pane's 1 s tick needs something to move. A deadline in
 * the past, or a skewed clock, clamps to `"due"` rather than rendering a
 * negative. An invalid / non-positive / out-of-range deadline yields `""`, as
 * does a non-finite `now`, and the caller then draws nothing at all.
 *
 * `"due"` means *eligible*, never *gone*: keeper's release sweep runs on the
 * first successful sync after its own hourly gate, a bounded number of objects
 * per pass, so a row can read `due` for a long while with its content still on
 * disk. This function therefore never claims the content has been released —
 * the sentence shown beside the figure is Rust's own (`FilesReleaseVm.detail`)
 * and is the only thing that describes what actually happens.
 *
 * @param deadlineMs - When the row becomes releasable, ms since the Unix epoch (UTC).
 * @param now - Reference "now" in ms; defaults to `Date.now()` (injectable for tests).
 */
export function formatReleaseIn(deadlineMs: number, now: number = Date.now()): string {
  if (!Number.isFinite(deadlineMs) || deadlineMs <= 0 || deadlineMs > MAX_DATE_MS) {
    return "";
  }
  // `now` is guarded for a sharper reason than the deadline: an unrenderable
  // deadline fails this function's own test, but a `NaN` / `±Infinity` `now`
  // makes every rung comparison below false, so control would fall through to
  // the days branch and the cell would paint "NaN days". The answer is the
  // module's usual one for anything it cannot render honestly — nothing.
  if (!Number.isFinite(now)) {
    return "";
  }
  const remainingMs = deadlineMs - now;
  // The clamp: an expired deadline — or a clock that ran ahead of the backend's
  // — reads "due" instead of a negative figure with a minus sign in it.
  if (remainingMs <= 0) {
    return "due";
  }
  if (remainingMs < 60_000) {
    // Floored, like the minutes, hours and days rungs below, so the whole
    // ladder is one rule and the figure reads "at least this many seconds
    // left". A `ceil` here rendered 59 999 ms as "60s", and under the pane's
    // 1 s tick the last minute then counted *up* in unit terms: 1 min → 60s →
    // 59s, the figure growing as the time shrank. Flooring makes the sequence
    // monotonically non-increasing, which is the one property a countdown owes
    // the person watching it. `max(1, …)` keeps the final millisecond from
    // announcing "0s" for a deadline that has not arrived.
    return `${Math.max(1, Math.floor(remainingMs / 1000))}s`;
  }
  if (remainingMs < 3_600_000) {
    return `${Math.floor(remainingMs / 60_000)} min`;
  }
  if (remainingMs < 86_400_000) {
    return `${Math.floor(remainingMs / 3_600_000)} hr`;
  }
  const days = Math.floor(remainingMs / 86_400_000);
  return days === 1 ? "1 day" : `${days} days`;
}

/**
 * The same fact as a phrase a screen reader can read on its own.
 *
 * `formatReleaseIn`'s figure is a fragment — `"23 hr"` read alone says nothing
 * about what happens in 23 hours — so the spoken form carries the verb. Empty
 * exactly when the figure is empty, so a row that draws nothing announces
 * nothing either.
 *
 * Both of `formatReleaseIn`'s guards — the deadline's and `now`'s — are
 * inherited through that single call and are deliberately not repeated here:
 * one place decides what is renderable, so the cell and its screen-reader
 * phrase cannot disagree about whether this row has a countdown at all.
 *
 * @param deadlineMs - When the row becomes releasable, ms since the Unix epoch (UTC).
 * @param now - Reference "now" in ms; defaults to `Date.now()` (injectable for tests).
 */
export function formatReleaseSpoken(deadlineMs: number, now: number = Date.now()): string {
  const short = formatReleaseIn(deadlineMs, now);
  if (short === "") {
    return "";
  }
  return short === "due" ? "Release is due" : `Releases in ${short}`;
}

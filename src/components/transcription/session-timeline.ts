import type { Utterance } from "@/lib/ipc/gen/Utterance";

/** A part's place on the transcript's one timeline, in seconds. */
export interface TimelinePart {
  offset: number;
  duration: number;
}

/** Where the session ends: the last part's end, however the parts are spaced. */
export function sessionLength(parts: readonly TimelinePart[]): number {
  return parts.reduce((end, part) => Math.max(end, part.offset + part.duration), 0);
}

/**
 * The part a global time falls in, and the time inside that part's file.
 *
 * A time in a gap between parts, or before the first, lands at the start of the
 * next part; a time past the end lands at the end of the last. `null` for a
 * session with no parts.
 */
export function locate(
  parts: readonly TimelinePart[],
  seconds: number,
): { index: number; local: number } | null {
  if (parts.length === 0) return null;
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    if (seconds < part.offset + part.duration)
      return { index, local: Math.max(0, seconds - part.offset) };
  }
  const index = parts.length - 1;
  return { index, local: parts[index].duration };
}

/**
 * The line being spoken at `seconds`: the latest-starting line whose span holds
 * it, so of two overlapping voices the one who spoke last is the one shown.
 * `-1` in silence.
 */
export function currentUtterance(utterances: readonly Utterance[], seconds: number): number {
  let found = -1;
  for (let index = 0; index < utterances.length; index += 1) {
    const utterance = utterances[index];
    if (utterance.start > seconds) break;
    if (seconds < utterance.end && (found < 0 || utterance.start >= utterances[found].start))
      found = index;
  }
  return found;
}

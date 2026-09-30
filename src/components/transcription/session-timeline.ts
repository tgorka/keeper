import type { Utterance } from "@/lib/ipc/gen/Utterance";

/** What a line needs to sit on the timeline; an `Utterance` and a block's line both are one. */
export type TimedLine = Pick<Utterance, "speaker" | "start" | "end">;

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
export function currentUtterance(utterances: readonly TimedLine[], seconds: number): number {
  let found = -1;
  for (let index = 0; index < utterances.length; index += 1) {
    const utterance = utterances[index];
    if (utterance.start > seconds) break;
    if (seconds < utterance.end && (found < 0 || utterance.start >= utterances[found].start))
      found = index;
  }
  return found;
}

/**
 * The line of `speaker` nearest `seconds`: one whose span holds it, else the
 * one whose start or end is closest. Of two equally near, the later one — the
 * reader asked where the voice goes next. `-1` when the speaker has no lines.
 */
export function nearestLine(lines: readonly TimedLine[], speaker: string, seconds: number): number {
  let best = -1;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.speaker !== speaker) continue;
    const distance =
      seconds < line.start ? line.start - seconds : seconds >= line.end ? seconds - line.end : 0;
    if (distance <= bestDistance) {
      best = index;
      bestDistance = distance;
    }
  }
  return best;
}

/**
 * The line of `speaker` that starts next after `seconds`; past their last line,
 * their first again. `-1` when the speaker has no lines.
 */
export function nextLine(lines: readonly TimedLine[], speaker: string, seconds: number): number {
  let first = -1;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.speaker !== speaker) continue;
    if (line.start > seconds) return index;
    if (first < 0) first = index;
  }
  return first;
}

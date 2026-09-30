import { describe, expect, it } from "vitest";
import {
  currentUtterance,
  locate,
  nearestLine,
  sessionLength,
} from "@/components/transcription/session-timeline";
import { SESSION_TRANSCRIPT_FIXTURE } from "../../../dev/transcription-fixture";

// Two parts with a two-second gap between them, as a session paused and resumed.
const parts = [
  { offset: 0, duration: 20 },
  { offset: 22, duration: 10 },
];

describe("One timeline over a session's parts", () => {
  it("maps a global time to its part and the time inside that part's file", () => {
    expect(locate(parts, 5)).toEqual({ index: 0, local: 5 });
    // A part's end is the next part's business, not a frame past this one.
    expect(locate(parts, 20)).toEqual({ index: 1, local: 0 });
    expect(locate(parts, 21)).toEqual({ index: 1, local: 0 });
    expect(locate(parts, 25.5)).toEqual({ index: 1, local: 3.5 });
    expect(locate(parts, 99)).toEqual({ index: 1, local: 10 });
    expect(locate([], 5)).toBeNull();
    expect(sessionLength(parts)).toBe(32);
  });

  it("names the line being spoken, and none in silence", () => {
    const lines = SESSION_TRANSCRIPT_FIXTURE.transcript.utterances;
    const at = (seconds: number) => lines[currentUtterance(lines, seconds)]?.id ?? null;
    expect(at(0)).toBe("u1");
    expect(at(5)).toBeNull();
    expect(at(6)).toBe("u2");
    expect(at(22)).toBe("u4");
    expect(at(41)).toBeNull();
  });

  it("shows the later of two overlapping voices", () => {
    const lines = structuredClone(SESSION_TRANSCRIPT_FIXTURE.transcript.utterances.slice(0, 2));
    lines[0].end = 10;
    expect(lines[currentUtterance(lines, 8)].id).toBe("u2");
    expect(lines[currentUtterance(lines, 4)].id).toBe("u1");
  });

  it("finds a speaker's nearest line, the one being said first and the later on a tie", () => {
    const lines = [
      { id: "a", speaker: "S1", start: 0, end: 4 },
      { id: "b", speaker: "S2", start: 4, end: 8 },
      { id: "c", speaker: "S1", start: 12, end: 16 },
    ];
    const near = (speaker: string, seconds: number) =>
      lines[nearestLine(lines, speaker, seconds)]?.id ?? null;
    expect(near("S1", 2)).toBe("a");
    expect(near("S1", 5)).toBe("a");
    // Four seconds from a's end and from c's start: the next one.
    expect(near("S1", 8)).toBe("c");
    expect(near("S1", 11)).toBe("c");
    expect(near("S1", 99)).toBe("c");
    expect(near("S2", 0)).toBe("b");
    expect(near("S3", 0)).toBeNull();
  });
});

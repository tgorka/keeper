/**
 * The mid-text `/` mechanics (Story 101.3): when a keystroke is worth a round
 * trip, and what a chosen row does to the draft. Which token it is and what it
 * offers are Rust's (`keeper_core::chat::inline`).
 */
import { describe, expect, it } from "vitest";
import { inlineCandidate, inserted } from "@/components/slash-menu/inline-slash";

describe("inlineCandidate", () => {
  it("asks after whitespace only, never at the draft's start or glued to text", () => {
    expect(inlineCandidate("ask /to", "")).toBe(true);
    expect(inlineCandidate("ask /", " later")).toBe(true);
    expect(inlineCandidate("line\n/sh", "")).toBe(true);
    expect(inlineCandidate("/to", "")).toBe(false);
    expect(inlineCandidate("and/or", "")).toBe(false);
    expect(inlineCandidate("a //x", "")).toBe(false);
  });

  it("asks only at a word's end and within the bound", () => {
    expect(inlineCandidate("ask /to", "la")).toBe(false);
    expect(inlineCandidate("ask /to la", "")).toBe(false);
    expect(inlineCandidate(`ask /${"x".repeat(65)}`, "")).toBe(false);
    expect(inlineCandidate(`ask /${"x".repeat(64)}`, "")).toBe(true);
  });
});

describe("inserted", () => {
  it("replaces only the token, keeping what follows the caret", () => {
    expect(inserted("ask /to", " about it", "/to", "@tola-grey")).toEqual({
      draft: "ask @tola-grey about it",
      caret: "ask @tola-grey".length,
    });
  });

  it("adds a space at the draft's end so the next word can follow", () => {
    expect(inserted("read /pl", "", "/pl", "notes/plan.md")).toEqual({
      draft: "read notes/plan.md ",
      caret: "read notes/plan.md ".length,
    });
  });

  it("changes nothing when the field no longer ends with the token", () => {
    expect(inserted("ask /tol", "", "/to", "@tola-grey")).toBeNull();
  });
});

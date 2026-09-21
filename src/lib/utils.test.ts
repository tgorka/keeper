import { describe, expect, it } from "vitest";
import { cn } from "@/lib/utils";

describe("cn", () => {
  it("keeps a custom font size beside a text colour", () => {
    // tailwind-merge classifies an unknown `text-*` utility as a colour, so
    // before the scale was registered these two were treated as one property
    // and the size lost. Every badge that also carried a colour then rendered
    // at the inherited 16px — larger than the 13px prompt it annotates.
    expect(cn("text-meta", "text-foreground")).toBe("text-meta text-foreground");
    expect(cn("text-title", "text-destructive")).toBe("text-title text-destructive");
  });

  it("still lets one font size replace another", () => {
    expect(cn("text-meta", "text-title")).toBe("text-title");
    expect(cn("text-sm", "text-meta")).toBe("text-meta");
  });

  it("still lets one text colour replace another", () => {
    expect(cn("text-foreground", "text-destructive")).toBe("text-destructive");
  });
});

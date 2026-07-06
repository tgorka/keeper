import { describe, expect, it } from "vitest";
import { acceleratorFromEvent, formatAccelerator } from "@/lib/hotkey";

describe("formatAccelerator", () => {
  it("renders the default as ⌃⌥Space", () => {
    expect(formatAccelerator("Control+Alt+Space")).toBe("⌃⌥Space");
  });

  it("renders modifiers in canonical ⌃⌥⇧⌘ order regardless of input order", () => {
    expect(formatAccelerator("Super+Shift+Control+K")).toBe("⌃⇧⌘K");
  });

  it("maps every modifier spelling to its glyph", () => {
    expect(formatAccelerator("Command+Space")).toBe("⌘Space");
    expect(formatAccelerator("Cmd+Space")).toBe("⌘Space");
    expect(formatAccelerator("Meta+Space")).toBe("⌘Space");
    expect(formatAccelerator("Option+F4")).toBe("⌥F4");
    expect(formatAccelerator("Ctrl+Up")).toBe("⌃Up");
  });

  it("title-cases a single-letter key and shows multi-char keys verbatim", () => {
    expect(formatAccelerator("Control+k")).toBe("⌃K");
    expect(formatAccelerator("Control+F12")).toBe("⌃F12");
  });

  it("returns unknown/empty input unchanged", () => {
    expect(formatAccelerator("")).toBe("");
  });
});

/** Build a minimal KeyboardEvent-like object for the pure capture helper. */
function keyEvent(init: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    code: "",
    key: "",
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...init,
  } as KeyboardEvent;
}

describe("acceleratorFromEvent", () => {
  it("captures ⌃⌥Space as Control+Alt+Space", () => {
    const event = keyEvent({ code: "Space", key: " ", ctrlKey: true, altKey: true });
    expect(acceleratorFromEvent(event)).toBe("Control+Alt+Space");
  });

  it("spells the ⌘/Command key as Super", () => {
    const event = keyEvent({ code: "KeyK", key: "k", metaKey: true, shiftKey: true });
    expect(acceleratorFromEvent(event)).toBe("Shift+Super+K");
  });

  it("returns null for a modifier-only press", () => {
    expect(
      acceleratorFromEvent(keyEvent({ code: "ShiftLeft", key: "Shift", shiftKey: true })),
    ).toBe(null);
    expect(
      acceleratorFromEvent(keyEvent({ code: "ControlLeft", key: "Control", ctrlKey: true })),
    ).toBe(null);
    expect(acceleratorFromEvent(keyEvent({ code: "MetaLeft", key: "Meta", metaKey: true }))).toBe(
      null,
    );
  });

  it("returns null for a modifier-less key so a bare key cannot become a global hotkey", () => {
    // A fat-fingered bare press (letter, Space, or Tab/Enter while capturing) must not
    // produce an accelerator — a modifier-less global hotkey would hijack that key OS-wide.
    expect(acceleratorFromEvent(keyEvent({ code: "KeyK", key: "k" }))).toBe(null);
    expect(acceleratorFromEvent(keyEvent({ code: "Space", key: " " }))).toBe(null);
    expect(acceleratorFromEvent(keyEvent({ code: "Tab", key: "Tab" }))).toBe(null);
    expect(acceleratorFromEvent(keyEvent({ code: "Enter", key: "Enter" }))).toBe(null);
    expect(acceleratorFromEvent(keyEvent({ code: "F4", key: "F4" }))).toBe(null);
    // A single modifier is enough to make it a valid chord.
    expect(acceleratorFromEvent(keyEvent({ code: "KeyK", key: "k", shiftKey: true }))).toBe(
      "Shift+K",
    );
  });

  it("maps physical codes to layout-independent key tokens", () => {
    expect(acceleratorFromEvent(keyEvent({ code: "Digit1", key: "1", metaKey: true }))).toBe(
      "Super+1",
    );
    expect(acceleratorFromEvent(keyEvent({ code: "ArrowUp", key: "ArrowUp", ctrlKey: true }))).toBe(
      "Control+Up",
    );
    expect(acceleratorFromEvent(keyEvent({ code: "F4", key: "F4", altKey: true }))).toBe("Alt+F4");
  });
});

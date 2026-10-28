import { act, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { BAR_FOLDS, BAR_WHOLE, useBarFold } from "@/hooks/use-bar-fold";

/**
 * jsdom lays nothing out, so the model's name is given the width its text needs (80.45px, a
 * real sub-pixel width) and its box the room each fold step frees (10px per step): the
 * browser's part, modelled. The hook's measuring loop, its order and its refolding are the real
 * ones; which control each step hides is CSS, measured in a real browser
 * (report-FooterOneRow.md, Round 2).
 */
const NEEDS_PX = 80.45;
const STEP_PX = 10;
let barPx = 0;
let resize: (() => void) | null = null;
const realObserver = globalThis.ResizeObserver;
const realRangeBox = Range.prototype.getBoundingClientRect;

beforeEach(() => {
  globalThis.ResizeObserver = class {
    constructor(callback: ResizeObserverCallback) {
      resize = () => callback([], this as unknown as ResizeObserver);
    }
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  Range.prototype.getBoundingClientRect = () => DOMRect.fromRect({ width: NEEDS_PX, height: 16 });
});
afterEach(() => {
  globalThis.ResizeObserver = realObserver;
  Range.prototype.getBoundingClientRect = realRangeBox;
  resize = null;
});

function Bar({ model = "glm-5.3-flash" }: { model?: string }) {
  const bar = useRef<HTMLDivElement>(null);
  useBarFold(bar, true);
  return (
    <div ref={bar} data-testid="bar">
      <span {...BAR_WHOLE} ref={laidOut}>
        {model}
      </span>
    </div>
  );
}

/** The name's box: as wide as the bar's width plus what the steps taken so far freed. */
function laidOut(name: HTMLSpanElement | null) {
  if (name === null) return;
  name.getBoundingClientRect = () => {
    const fold = name.closest<HTMLElement>("[data-testid=bar]")?.dataset.fold ?? "";
    const steps = fold === "" ? 0 : fold.split(" ").length;
    return DOMRect.fromRect({ width: barPx + STEP_PX * steps, height: 16 });
  };
}

const fold = () => screen.getByTestId("bar").dataset.fold;

it("gives up the fewest steps, in order, that keep the model's name whole", () => {
  barPx = 55;
  render(<Bar />);
  // 55 + 3 × 10 holds 80.45: the context's label, Send's word and the chips' padding go; the
  // effort keeps its words.
  expect(fold()).toBe("ring send tight");
});

it("folds once more for a fraction of a pixel, which the browser still ellipsizes", () => {
  // 50.42 + 3 × 10 = 80.42px of box for 80.45px of text: integer widths read both as 80.
  barPx = 50.42;
  render(<Bar />);
  expect(fold()).toBe("ring send tight effort");
});

it("unfolds when the bar widens and folds again when it narrows", () => {
  barPx = 55;
  render(<Bar />);
  barPx = NEEDS_PX;
  act(() => resize?.());
  expect(fold()).toBe("");
  barPx = 41;
  act(() => resize?.());
  expect(fold()).toBe("ring send tight effort");
});

it("takes every step, then cuts the name, where no step is enough", () => {
  barPx = 0;
  render(<Bar />);
  expect(fold()).toBe(BAR_FOLDS.join(" "));
});

it("refolds when what the bar shows changes", async () => {
  barPx = NEEDS_PX;
  const { rerender } = render(<Bar />);
  expect(fold()).toBe("");
  barPx = 65;
  rerender(<Bar model="claude-opus-5-5" />);
  // The text change reaches the hook's MutationObserver, a microtask later.
  await act(async () => {});
  expect(fold()).toBe("ring send");
});

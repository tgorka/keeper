import { afterEach, describe, expect, it } from "vitest";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";

afterEach(() => {
  leadingDrawerStore.getState().close();
});

describe("leadingDrawerStore", () => {
  it("starts closed", () => {
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("open() sets isOpen true", () => {
    leadingDrawerStore.getState().open();
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
  });

  it("close() sets isOpen false", () => {
    leadingDrawerStore.getState().open();
    leadingDrawerStore.getState().close();
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("toggle() flips isOpen in both directions", () => {
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
    leadingDrawerStore.getState().toggle();
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
    leadingDrawerStore.getState().toggle();
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });
});

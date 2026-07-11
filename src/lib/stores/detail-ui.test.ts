import { afterEach, describe, expect, it } from "vitest";
import { detailStore } from "@/lib/stores/detail-ui";

afterEach(() => {
  detailStore.setState({ open: false });
});

describe("detailStore", () => {
  it("starts closed", () => {
    expect(detailStore.getState().open).toBe(false);
  });

  it("opens and closes", () => {
    detailStore.getState().openDetail();
    expect(detailStore.getState().open).toBe(true);
    detailStore.getState().closeDetail();
    expect(detailStore.getState().open).toBe(false);
  });

  it("open and close are idempotent", () => {
    detailStore.getState().openDetail();
    detailStore.getState().openDetail();
    expect(detailStore.getState().open).toBe(true);
    detailStore.getState().closeDetail();
    detailStore.getState().closeDetail();
    expect(detailStore.getState().open).toBe(false);
  });

  it("toggles from either state", () => {
    detailStore.getState().toggleDetail();
    expect(detailStore.getState().open).toBe(true);
    detailStore.getState().toggleDetail();
    expect(detailStore.getState().open).toBe(false);
  });
});

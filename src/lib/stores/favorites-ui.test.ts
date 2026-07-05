import { afterEach, describe, expect, it } from "vitest";
import { favoritesUiStore } from "@/lib/stores/favorites-ui";

afterEach(() => {
  favoritesUiStore.getState().setCollapsed(false);
});

describe("favoritesUiStore", () => {
  it("defaults to expanded", () => {
    expect(favoritesUiStore.getState().isCollapsed).toBe(false);
  });

  it("toggles the collapse state", () => {
    favoritesUiStore.getState().setCollapsed(true);
    expect(favoritesUiStore.getState().isCollapsed).toBe(true);
    favoritesUiStore.getState().setCollapsed(false);
    expect(favoritesUiStore.getState().isCollapsed).toBe(false);
  });
});

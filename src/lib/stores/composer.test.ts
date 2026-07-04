import { afterEach, describe, expect, it } from "vitest";
import { composerStore } from "@/lib/stores/composer";

afterEach(() => {
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
});

describe("composerStore", () => {
  it("startReply sets a reply pending and leaves the draft untouched (no stash)", () => {
    composerStore.getState().startReply({
      targetKey: "k1",
      sender: "Bob",
      bodyPreview: "hi there",
    });
    const { pending, stashedDraft } = composerStore.getState();
    expect(pending).toEqual({
      mode: "reply",
      targetKey: "k1",
      sender: "Bob",
      bodyPreview: "hi there",
    });
    expect(stashedDraft).toBeNull();
  });

  it("startEdit stashes the current draft and returns the body to prefill", () => {
    const body = composerStore
      .getState()
      .startEdit({ targetKey: "k2", body: "original" }, "typed so far");
    expect(body).toBe("original");
    const { pending, stashedDraft } = composerStore.getState();
    expect(pending).toEqual({ mode: "edit", targetKey: "k2" });
    expect(stashedDraft).toBe("typed so far");
  });

  it("cancel restores the stashed draft for an edit and clears pending", () => {
    composerStore.getState().startEdit({ targetKey: "k2", body: "original" }, "my draft");
    const restored = composerStore.getState().cancel();
    expect(restored).toBe("my draft");
    expect(composerStore.getState().pending).toBeNull();
    expect(composerStore.getState().stashedDraft).toBeNull();
  });

  it("cancel returns null for a reply (draft is kept by the composer)", () => {
    composerStore.getState().startReply({ targetKey: "k1", sender: "Bob", bodyPreview: "hi" });
    const restored = composerStore.getState().cancel();
    expect(restored).toBeNull();
    expect(composerStore.getState().pending).toBeNull();
  });

  it("clear drops the pending context and stash", () => {
    composerStore.getState().startEdit({ targetKey: "k2", body: "x" }, "d");
    composerStore.getState().clear();
    expect(composerStore.getState().pending).toBeNull();
    expect(composerStore.getState().stashedDraft).toBeNull();
  });

  it("select / clearSelection manage the keyboard-selected key", () => {
    composerStore.getState().select("k9");
    expect(composerStore.getState().selectedKey).toBe("k9");
    composerStore.getState().clearSelection();
    expect(composerStore.getState().selectedKey).toBeNull();
  });

  it("switching from edit to reply clears the stash", () => {
    composerStore.getState().startEdit({ targetKey: "k2", body: "x" }, "d");
    composerStore.getState().startReply({ targetKey: "k1", sender: "Bob", bodyPreview: "hi" });
    expect(composerStore.getState().stashedDraft).toBeNull();
  });
});

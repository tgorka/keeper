/**
 * Story 55.4 — one element per kind, and the ordering that makes a failure
 * reportable.
 */
import { describe, expect, it, vi } from "vitest";
import { mediaElementFor } from "./media-element";

describe("the element a file becomes", () => {
  it("draws an image with the file name as its alt text", () => {
    const node = mediaElementFor(
      { kind: "image", name: "holiday.png", url: "keeper-note://v1/holiday.png" },
      vi.fn(),
    );

    expect(node.tagName).toBe("IMG");
    const image = node as HTMLImageElement;
    expect(image.alt).toBe("holiday.png");
    // An off-screen embed in a long note costs nothing until it is scrolled to.
    expect(image.loading).toBe("lazy");
    expect(image.getAttribute("src")).toBe("keeper-note://v1/holiday.png");
  });

  it("draws a video and an audio player, both metadata-only", () => {
    for (const [kind, tag] of [
      ["video", "VIDEO"],
      ["audio", "AUDIO"],
    ] as const) {
      const node = mediaElementFor({ kind, name: `clip.${kind}`, url: "u" }, vi.fn());

      expect(node.tagName).toBe(tag);
      const player = node as HTMLMediaElement;
      expect(player.controls).toBe(true);
      // A note with ten videos must not fetch ten videos.
      expect(player.preload).toBe("metadata");
      expect(player.getAttribute("aria-label")).toBe(`clip.${kind}`);
    }
  });

  it("draws a PDF as the webview's own renderer", () => {
    const node = mediaElementFor({ kind: "pdf", name: "report.pdf", url: "u" }, vi.fn());

    expect(node.tagName).toBe("EMBED");
    expect(node.getAttribute("type")).toBe("application/pdf");
    expect(node.getAttribute("aria-label")).toBe("report.pdf");
  });

  it("registers the failure handler before the load starts", () => {
    // The ordering, not the handler: assigning `src` is what begins the fetch,
    // so a handler attached afterwards can miss the error of a URL that fails
    // immediately — and the whole degrade path hangs off that one event.
    const order: string[] = [];
    const element = document.createElement("img");
    const realAdd = element.addEventListener.bind(element);
    vi.spyOn(element, "addEventListener").mockImplementation((type, listener, opts) => {
      order.push(`listen:${type}`);
      realAdd(type, listener as EventListener, opts);
    });
    vi.spyOn(document, "createElement").mockReturnValueOnce(element);
    Object.defineProperty(element, "src", {
      set() {
        order.push("src");
      },
      get: () => "",
      configurable: true,
    });

    mediaElementFor({ kind: "image", name: "a.png", url: "u" }, vi.fn());

    expect(order).toEqual(["listen:error", "src"]);
    vi.restoreAllMocks();
  });

  it("puts the link back when the bytes do not arrive", () => {
    const onFailed = vi.fn();
    const node = mediaElementFor({ kind: "image", name: "gone.png", url: "u" }, onFailed);

    node.dispatchEvent(new Event("error"));

    expect(onFailed).toHaveBeenCalledTimes(1);
  });
});

import { describe, expect, it, vi } from "vitest";
import { asShownPhoto } from "@/lib/shown-photo";

/** A picked file whose bytes jsdom hands back. */
function picked(bytes: number[], name: string, type: string): File {
  const buffer = new Uint8Array(bytes).buffer;
  const file = new File([buffer], name, { type });
  Object.defineProperty(file, "arrayBuffer", { value: async () => buffer });
  return file;
}

describe("asShownPhoto (R444/05)", () => {
  it("sends a picture a model is shown as it is", async () => {
    const encode = vi.fn();
    const photo = await asShownPhoto(picked([1, 2, 3], "cat.png", "image/png"), "cat.png", encode);
    expect(photo).toEqual({
      bytes: new Uint8Array([1, 2, 3]).buffer,
      filename: "cat.png",
      mime: "image/png",
    });
    expect(encode).not.toHaveBeenCalled();
  });

  it("re-encodes a HEIC the webview can decode as a JPEG of the same name", async () => {
    const jpeg = new Uint8Array([255, 216, 255, 217]).buffer;
    const photo = await asShownPhoto(
      picked([0, 0, 0, 24], "IMG_0042.HEIC", "image/heic"),
      "IMG_0042.HEIC",
      async () => jpeg,
    );
    expect(photo).toEqual({ bytes: jpeg, filename: "IMG_0042.jpg", mime: "image/jpeg" });
  });

  it("sends the original HEIC, typed by its name, when it cannot be decoded", async () => {
    const photo = await asShownPhoto(
      picked([0, 0, 0, 24], "IMG_0042.heic", ""),
      "IMG_0042.heic",
      async () => {
        throw new Error("no HEIC decoder");
      },
    );
    expect(photo).toEqual({
      bytes: new Uint8Array([0, 0, 0, 24]).buffer,
      filename: "IMG_0042.heic",
      mime: "image/heic",
    });
  });
});

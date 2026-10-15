/**
 * A photo a person picks for an agent, in a format a model is shown (story
 * 101.2, R444/05). A model is shown PNG, JPEG, WebP and GIF
 * (`keeper_core::bots::deliverable::IMAGE_MIMES`); a phone's HEIC or HEIF, or
 * any other picture, is re-encoded as JPEG here when this webview can decode
 * it. When it cannot, the original goes as it is, and the agent's host says in
 * the room that its format can't be shown to the model — never silence.
 */

/** The picture types a model is shown as they are. */
const SHOWN = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/** Picture extensions a picker may hand over with no type. */
const PICTURE_NAME = /\.(heic|heif|tiff?|bmp|avif)$/i;

/** A photo ready for the tray: its bytes, name and type. */
export interface ShownPhoto {
  bytes: ArrayBuffer;
  filename: string;
  mime: string;
}

/** Decodes a picture and encodes it as JPEG; throws when it cannot. */
export type JpegEncoder = (picture: Blob) => Promise<ArrayBuffer>;

/** Whether a picture of `mime` named `filename` must become JPEG to be shown. */
export function needsJpeg(mime: string, filename: string): boolean {
  const type = mime.toLowerCase();
  if (SHOWN.includes(type)) {
    return false;
  }
  return type.startsWith("image/") || (type === "" && PICTURE_NAME.test(filename));
}

/** `filename` with its extension replaced by `.jpg`. */
function jpegName(filename: string): string {
  const stem = filename.replace(/\.[^./]+$/, "");
  return `${stem || "photo"}.jpg`;
}

/**
 * `picture` as the tray takes it: as it is when a model is shown its type,
 * else re-encoded as JPEG by `encode` — the original when that fails.
 */
export async function asShownPhoto(
  picture: Blob,
  filename: string,
  encode: JpegEncoder = encodeJpeg,
): Promise<ShownPhoto> {
  const original = async (): Promise<ShownPhoto> => ({
    bytes: await picture.arrayBuffer(),
    filename,
    mime: picture.type || "image/jpeg",
  });
  if (!needsJpeg(picture.type, filename)) {
    return original();
  }
  try {
    return { bytes: await encode(picture), filename: jpegName(filename), mime: "image/jpeg" };
  } catch {
    // This webview cannot decode it: the original goes, typed by its name
    // when the picker gave no type, and the host says so in the room.
    const named = PICTURE_NAME.exec(filename)?.[1].toLowerCase().replace(/^tif$/, "tiff");
    return {
      ...(await original()),
      mime: picture.type || (named ? `image/${named}` : "application/octet-stream"),
    };
  }
}

/** The webview's own decoder: an image bitmap drawn on a canvas, as JPEG. */
async function encodeJpeg(picture: Blob): Promise<ArrayBuffer> {
  const bitmap = await createImageBitmap(picture);
  try {
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const context = canvas.getContext("2d");
    if (!context) {
      throw new Error("no canvas to draw the photo on");
    }
    context.drawImage(bitmap, 0, 0);
    const jpeg = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/jpeg", 0.9));
    if (!jpeg) {
      throw new Error("the photo could not be encoded as JPEG");
    }
    return await jpeg.arrayBuffer();
  } finally {
    bitmap.close();
  }
}

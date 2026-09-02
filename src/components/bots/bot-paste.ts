/**
 * What the composer does with a paste (Epic 61, Story 61.12, FR-392, AD-160).
 *
 * # The gesture, and the one byte-path it is allowed to take
 *
 * A clipboard image has no OS path, so the only route into keeper is the one
 * the Matrix composer already proved: `clipboardData` → `File` → `ArrayBuffer`
 * → a **raw binary IPC body**, never base64 inside a JSON payload (AD-58, and
 * `notes_vault.rs:1851-1853` in the house's own words). This module makes the
 * decision; {@link botsImagePaste} carries the bytes; nothing in between ever
 * holds a base64 string.
 *
 * What was extended to get there: `notes_attachment_paste` still refuses with
 * `Unsupported` because reading the system clipboard from Rust needs a
 * clipboard backend this build does not link — that refusal is untouched and
 * still correct. What this story extends is the **webview-paste** route
 * (`chat/composer.tsx:696-721` + `client.ts` `sendAttachmentBytes`), which is
 * the only proven clipboard-image path in the tree, by giving the bots surface
 * its own raw-body command rather than widening a Matrix one with two null
 * arguments.
 *
 * # Why the capability check is here and also in Rust
 *
 * The person needs the refusal in the same frame as the gesture — a paste that
 * silently does nothing for a second and then explains itself is the dead
 * affordance AD-27 forbids. So the tri-state is evaluated here, and
 * `keeper_core::bots::deliverable::accept_image` evaluates it again at the
 * door, because a check that only exists in the webview is not a check.
 *
 * The two copies of the copy are held together by a test rather than by hope:
 * `bot-paste.test.ts` reads `bots/deliverable.rs` and fails if the numbers or
 * the sentences drift apart. That is the same shape as
 * `src/test/file-scheme-registration.test.ts`, which reads the shell's own
 * source to prove a registration nobody else can observe.
 *
 * # A capability keeper could not read is `unknown`, never `false`
 *
 * `vision: null` means the endpoint did not say. The paste is **offered**, with
 * a sentence saying keeper could not tell; only an endpoint that said "no"
 * refuses, and that refusal names the model, because switching model is the
 * act that would change the answer.
 */

/**
 * The largest single image keeper attaches, in bytes.
 *
 * 8 MiB — `note_protocol::MAX_RANGE_CHUNK`, the ceiling the tree already puts
 * on one read of one asset into the webview, rather than a number invented
 * here. Its base64 expansion is ~10.7 MB, which is then the largest request
 * body keeper ever builds. Mirrors `deliverable::MAX_IMAGE_BYTES`.
 */
export const BOT_IMAGE_MAX_BYTES = 8 * 1024 * 1024;

/**
 * The most images one message may carry.
 *
 * Four, and the reason is the context window rather than the bytes: Ollama's
 * `/v1` layer cannot set `num_ctx`, so keeper cannot widen the window it is
 * about to fill. Mirrors `deliverable::MAX_IMAGES_PER_MESSAGE`.
 */
export const BOT_IMAGE_MAX_COUNT = 4;

/**
 * The image types keeper attaches — the vision guide's four, minus SVG, which
 * Ollama has errored on since v0.4.6 and which is a document that can carry
 * script. Mirrors `deliverable::IMAGE_MIMES`.
 */
export const BOT_IMAGE_MIMES = ["image/png", "image/jpeg", "image/webp", "image/gif"] as const;

/** Why an image was not attached to a model that cannot see. */
export function botRefuseVision(model: string): string {
  return `${model} does not accept images, so the paste was not attached. Choose a model that can see, and paste again.`;
}

/** What an unknown vision capability says while still offering the paste. */
export function botWarnVisionUnknown(model: string): string {
  return `keeper could not read whether ${model} accepts images. The image is attached, and the model may answer that it cannot see it.`;
}

/** Why an oversized paste was refused, with both numbers in it. */
export function botRefuseOversize(byteLength: number): string {
  return `That image is ${botHumanBytes(byteLength)} and keeper attaches at most ${botHumanBytes(
    BOT_IMAGE_MAX_BYTES,
  )} per image, so it was not attached. Save it, shrink it, and paste it again.`;
}

/** Why one image too many was refused, with the count in it. */
export function botRefuseTooMany(): string {
  return `This message already carries ${BOT_IMAGE_MAX_COUNT} images, which is as many as keeper attaches at once. Send these, then paste the next one.`;
}

/** Why a clipboard image in a format keeper does not attach was refused. */
export function botRefuseMime(mime: string): string {
  return `keeper does not attach ${mime} images, so the paste was not attached. PNG, JPEG, WEBP and GIF are attached.`;
}

/**
 * Render a byte count the way a refusal should read it: whole tenths of a MB
 * above a megabyte, whole kB below. Mirrors `deliverable::human_bytes`, so a
 * refusal written by Rust and one written here read identically.
 */
export function botHumanBytes(bytes: number): string {
  const KB = 1024;
  const MB = 1024 * 1024;
  if (bytes >= MB) {
    const tenths = Math.ceil((bytes * 10) / MB);
    return `${Math.floor(tenths / 10)}.${tenths % 10} MB`;
  }
  return `${Math.ceil(bytes / KB)} kB`;
}

/** What the composer knows about the bot it is pasting into. */
export interface BotPasteContext {
  /**
   * The model's own vision answer, read from the endpoint by Story 61.3:
   * `true` where it said yes, `false` where it said no, `null` where keeper
   * could not read it. `null` offers the paste with a warning.
   */
  vision: boolean | null;
  /** The model's name, for the refusal that has to name it. */
  model: string;
  /** How many images this message already carries. */
  attached: number;
}

/** What the composer should do with one paste event. */
export type BotPasteDecision =
  /** Leave it to the browser — the text flavour lands in the textarea. */
  | { kind: "passthrough" }
  /** keeper takes the image. The bytes have not been read yet. */
  | {
      kind: "attachImage";
      /** The clipboard's own file handle — the only thing that holds bytes. */
      file: File;
      /** What the attachment is called, invented when the clipboard has no name. */
      filename: string;
      /** The clipboard's MIME. */
      mime: string;
      /** Its size in bytes. */
      size: number;
      /** A sentence to show beside it, where there is one to show. */
      warning: string | null;
    }
  /** keeper will not take it, and this sentence says why. */
  | { kind: "refuse"; reason: string };

/**
 * Decide what to do with a paste.
 *
 * Takes the `DataTransfer` rather than the event, so the decision is testable
 * without a DOM event and so the composer keeps `preventDefault` — a component
 * that hands out its event is a component whose caller can cancel a gesture it
 * does not own.
 *
 * `context` is `null` when there is no bot to paste into. Then every paste is
 * a passthrough: with nothing to attach to, claiming the gesture would swallow
 * it and say nothing.
 *
 * The order of the refusals is capability, then format, then size, then count —
 * `deliverable::accept_image`'s order, and for its reason: a person pasting
 * into a bot that cannot see needs to hear that, not that their screenshot is
 * 9 MB, which would be a true sentence about the wrong problem.
 */
export function botPasteDecision(
  data: DataTransfer | null,
  context: BotPasteContext | null = null,
): BotPasteDecision {
  if (data === null || context === null) {
    return { kind: "passthrough" };
  }
  const item = Array.from(data.items).find((entry) => entry.type.startsWith("image/"));
  if (item === undefined) {
    // Not an image. The browser's own text paste is the right behaviour and is
    // left entirely alone.
    return { kind: "passthrough" };
  }
  const file = item.getAsFile();
  if (file === null) {
    // An `image/*` entry the browser will not materialise. Nothing to attach
    // and nothing to say, so the default paste proceeds.
    return { kind: "passthrough" };
  }
  if (context.vision === false) {
    return { kind: "refuse", reason: botRefuseVision(context.model) };
  }
  const mime = file.type;
  if (!(BOT_IMAGE_MIMES as readonly string[]).includes(mime)) {
    return { kind: "refuse", reason: botRefuseMime(mime) };
  }
  if (file.size > BOT_IMAGE_MAX_BYTES) {
    return { kind: "refuse", reason: botRefuseOversize(file.size) };
  }
  if (context.attached >= BOT_IMAGE_MAX_COUNT) {
    return { kind: "refuse", reason: botRefuseTooMany() };
  }
  const extension = mime.split("/")[1] ?? "png";
  return {
    kind: "attachImage",
    file,
    filename: file.name !== "" ? file.name : `pasted-image.${extension}`,
    mime,
    size: file.size,
    warning: context.vision === null ? botWarnVisionUnknown(context.model) : null,
  };
}

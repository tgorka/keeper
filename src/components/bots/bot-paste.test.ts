/**
 * What the composer does with a paste (Epic 61, Story 61.12, FR-392, AD-58).
 *
 * Four things are asserted here that nothing else in the tree asserts:
 *
 * 1. **The vision tri-state survives the trip to a component prop.** `true`
 *    attaches silently, `null` attaches with a warning that names the model,
 *    `false` refuses by name. A boolean anywhere in that chain collapses the
 *    middle case into the wrong one, which is the AD-27 failure the epic names
 *    three times.
 * 2. **A non-image paste is not touched.** The decision is `passthrough`, so
 *    the composer never calls `preventDefault` and the browser's own text paste
 *    lands — the behaviour `chat/composer.test.tsx:358-362` already pins for
 *    the Matrix composer.
 * 3. **The caps refuse with their numbers in the sentence.** A cap whose
 *    refusal omits the number is a cap the reader cannot act on.
 * 4. **The copy and the numbers match `keeper-core`.** This file reads
 *    `bots/deliverable.rs` and fails if the two drift, which is the same shape
 *    `src/test/file-scheme-registration.test.ts` uses to pin a Rust fact a
 *    TypeScript test could not otherwise see.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  BOT_IMAGE_MAX_BYTES,
  BOT_IMAGE_MAX_COUNT,
  BOT_IMAGE_MIMES,
  type BotPasteContext,
  botHumanBytes,
  botPasteDecision,
  botRefuseMime,
  botRefuseOversize,
  botRefuseTooMany,
  botRefuseVision,
  botWarnVisionUnknown,
} from "@/components/bots/bot-paste";

/** A clipboard holding one item of `type`, whose file is `size` bytes. */
function clipboard(type: string, size = 1024, name = ""): DataTransfer {
  const file = new File([new Uint8Array(0)], name, { type });
  // `File` computes its own size from the parts, and a test that allocated a
  // real 8 MiB buffer to exercise the cap would be a slow test proving nothing
  // about the cap. The size is the input to the decision, so it is the thing
  // stubbed.
  Object.defineProperty(file, "size", { value: size });
  return {
    items: [{ type, getAsFile: () => file }],
  } as unknown as DataTransfer;
}

/** A clipboard whose only item refuses to materialise a file. */
function emptyImageClipboard(): DataTransfer {
  return {
    items: [{ type: "image/png", getAsFile: () => null }],
  } as unknown as DataTransfer;
}

function context(overrides: Partial<BotPasteContext> = {}): BotPasteContext {
  return { vision: true, model: "llava:13b", attached: 0, ...overrides };
}

describe("botPasteDecision", () => {
  it("leaves a text paste entirely to the browser", () => {
    expect(botPasteDecision(clipboard("text/plain"), context())).toEqual({ kind: "passthrough" });
  });

  it("leaves a paste alone when there is no bot to attach to", () => {
    expect(botPasteDecision(clipboard("image/png"), null)).toEqual({ kind: "passthrough" });
    expect(botPasteDecision(null, context())).toEqual({ kind: "passthrough" });
  });

  it("leaves an image entry the browser will not materialise alone", () => {
    expect(botPasteDecision(emptyImageClipboard(), context())).toEqual({ kind: "passthrough" });
  });

  it("attaches an image for a model that can see, with nothing to disclose", () => {
    const decision = botPasteDecision(clipboard("image/png", 4096), context({ vision: true }));
    expect(decision.kind).toBe("attachImage");
    if (decision.kind !== "attachImage") {
      return;
    }
    expect(decision.mime).toBe("image/png");
    expect(decision.size).toBe(4096);
    expect(decision.filename).toBe("pasted-image.png");
    expect(decision.warning).toBeNull();
  });

  it("keeps the clipboard's own file name when it has one", () => {
    const decision = botPasteDecision(clipboard("image/png", 10, "shot.png"), context());
    expect(decision.kind === "attachImage" && decision.filename).toBe("shot.png");
  });

  it("attaches with a warning when the endpoint did not say whether the model can see", () => {
    const decision = botPasteDecision(
      clipboard("image/png"),
      context({ vision: null, model: "mystery:7b" }),
    );
    expect(decision.kind).toBe("attachImage");
    if (decision.kind !== "attachImage") {
      return;
    }
    expect(decision.warning).toBe(botWarnVisionUnknown("mystery:7b"));
    expect(decision.warning).toContain("mystery:7b");
    expect(decision.warning).toContain("could not read");
  });

  it("refuses an image for a model that cannot see, naming the model", () => {
    const decision = botPasteDecision(
      clipboard("image/png"),
      context({ vision: false, model: "llama4:8b" }),
    );
    expect(decision).toEqual({ kind: "refuse", reason: botRefuseVision("llama4:8b") });
    expect(decision.kind === "refuse" && decision.reason).toContain("llama4:8b");
  });

  it("refuses on the capability before it refuses on the size", () => {
    // A person pasting into a bot that cannot see must hear that, not that
    // their screenshot is large — a true sentence about the wrong problem.
    const decision = botPasteDecision(
      clipboard("image/png", BOT_IMAGE_MAX_BYTES + 1),
      context({ vision: false, model: "llama4:8b" }),
    );
    expect(decision.kind === "refuse" && decision.reason).toBe(botRefuseVision("llama4:8b"));
  });

  it("refuses an oversize image with both numbers in the sentence", () => {
    const decision = botPasteDecision(clipboard("image/png", BOT_IMAGE_MAX_BYTES + 1), context());
    expect(decision.kind).toBe("refuse");
    if (decision.kind !== "refuse") {
      return;
    }
    expect(decision.reason).toBe(botRefuseOversize(BOT_IMAGE_MAX_BYTES + 1));
    expect(decision.reason).toContain("8.1 MB");
    expect(decision.reason).toContain("8.0 MB");
  });

  it("accepts an image of exactly the cap", () => {
    const decision = botPasteDecision(clipboard("image/png", BOT_IMAGE_MAX_BYTES), context());
    expect(decision.kind).toBe("attachImage");
  });

  it("refuses one image past the count cap, with the count in the sentence", () => {
    const decision = botPasteDecision(
      clipboard("image/png"),
      context({ attached: BOT_IMAGE_MAX_COUNT }),
    );
    expect(decision).toEqual({ kind: "refuse", reason: botRefuseTooMany() });
    expect(decision.kind === "refuse" && decision.reason).toContain(String(BOT_IMAGE_MAX_COUNT));
    expect(
      botPasteDecision(clipboard("image/png"), context({ attached: BOT_IMAGE_MAX_COUNT - 1 })).kind,
    ).toBe("attachImage");
  });

  it("refuses an SVG rather than attaching one", () => {
    const decision = botPasteDecision(clipboard("image/svg+xml"), context());
    expect(decision).toEqual({ kind: "refuse", reason: botRefuseMime("image/svg+xml") });
    expect(BOT_IMAGE_MIMES).not.toContain("image/svg+xml");
  });

  it("never puts the bytes anywhere but the File it was handed", () => {
    // The decision carries a `File` handle and a length, and nothing else.
    // The bytes leave over a raw binary IPC body; a base64 string anywhere in
    // this object would be the AD-58 violation the whole design avoids.
    const decision = botPasteDecision(clipboard("image/png", 4096), context());
    const serialized = JSON.stringify(decision, (_key, value) =>
      value instanceof File ? "[File]" : value,
    );
    expect(serialized).not.toContain("base64");
    expect(serialized).not.toContain("data:image");
    expect(Object.keys(decision).sort()).toEqual([
      "file",
      "filename",
      "kind",
      "mime",
      "size",
      "warning",
    ]);
  });
});

describe("botHumanBytes", () => {
  it("reads a cap and a file with the same precision", () => {
    expect(botHumanBytes(8 * 1024 * 1024)).toBe("8.0 MB");
    expect(botHumanBytes(8 * 1024 * 1024 + 1)).toBe("8.1 MB");
    expect(botHumanBytes(1024)).toBe("1 kB");
    expect(botHumanBytes(1)).toBe("1 kB");
  });
});

describe("the copy and the caps mirror keeper-core", () => {
  // The gate exists on both sides of the IPC boundary, so the two wordings and
  // the two numbers must be one wording and one number. This reads the Rust
  // source rather than trusting a comment.
  const RUST = readFileSync("src-tauri/crates/keeper-core/src/bots/deliverable.rs", "utf8");

  it("agrees on the byte cap and the count cap", () => {
    expect(RUST).toContain("pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;");
    expect(BOT_IMAGE_MAX_BYTES).toBe(8 * 1024 * 1024);
    expect(RUST).toContain(`pub const MAX_IMAGES_PER_MESSAGE: usize = ${BOT_IMAGE_MAX_COUNT};`);
  });

  it("agrees on the attachable formats", () => {
    for (const mime of BOT_IMAGE_MIMES) {
      expect(RUST).toContain(`"${mime}"`);
    }
    expect(RUST).not.toContain('"image/svg+xml",');
  });

  it("agrees on every refusal sentence", () => {
    // Each sentence is asserted through the fragment Rust spells literally —
    // the interpolated model name and byte counts are the only difference
    // between the two implementations.
    expect(botRefuseVision("M")).toContain(
      "does not accept images, so the paste was not attached.",
    );
    expect(RUST).toContain("does not accept images, so the paste was not attached.");
    expect(botWarnVisionUnknown("M")).toContain("accepts images.");
    expect(RUST).toContain("keeper could not read whether {model} accepts images.");
    expect(botRefuseTooMany()).toContain("as many as keeper attaches at once.");
    expect(RUST).toContain("which is as many as \\");
    expect(botRefuseMime("image/x")).toContain("PNG, JPEG, WEBP and GIF are attached.");
    expect(RUST).toContain("PNG, JPEG, WEBP and GIF are attached.");
    expect(botRefuseOversize(1)).toContain("Save it, shrink it, and paste it again.");
    expect(RUST).toContain("Save it, shrink it, and paste it again.");
  });
});

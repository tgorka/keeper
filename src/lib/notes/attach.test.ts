import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  type AttachmentPlan,
  attachmentEmbed,
  attachmentName,
  bodyWithAttachments,
  embeddedAttachmentNames,
  planAttachments,
} from "@/lib/notes/attach";

/**
 * The one insertion path (Story 45.13, FR-188, FR-189).
 *
 * Two things are under test here and they are different in kind. The first is
 * the mirror: this module re-implements
 * `keeper_core::notes::attach::embedded_attachment_names` because the open
 * editor's buffer never reaches Rust, and the shared vector table is what stops
 * the two drifting into offering a note that the panel then refuses. The second
 * is the plan itself — what gets written, what gets refused, and in what order
 * — which is the contract all three entry points are built on.
 */

/**
 * The vector table `keeper_core::notes::attach` is tested against.
 *
 * Read from the Rust tree rather than copied into `src/`, the same direction
 * `src/lib/file-size.test.ts` reads its own: the fixture lives beside the
 * canonical implementation and the mirror reaches for it. A copy would be a
 * third thing to keep in step, which is the problem this file exists to solve.
 */
const FIXTURE = resolve(
  import.meta.dirname,
  "../../../src-tauri/crates/keeper-core/src/notes/attach-vectors.json",
);

interface AttachVectors {
  vectors: { body: string; embedded: string[]; why: string }[];
}

const shared = JSON.parse(readFileSync(FIXTURE, "utf8")) as AttachVectors;

describe("embeddedAttachmentNames, against keeper-core's own vectors", () => {
  /**
   * The whole point of the mirror. If this fails, either this module or
   * `keeper-core/src/notes/attach.rs` has changed and the other has not — and
   * the Files-pane chooser is about to offer a note the attachments panel will
   * refuse to write into.
   */
  it("matches keeper-core on every shared vector", () => {
    expect(shared.vectors.length).toBeGreaterThanOrEqual(18);
    for (const vector of shared.vectors) {
      expect(
        [...embeddedAttachmentNames(vector.body)].sort(),
        `${vector.body}: ${vector.why}`,
      ).toEqual(vector.embedded);
    }
  });
});

describe("the one embed spelling", () => {
  it("writes Obsidian's embed and nothing keeper-specific", () => {
    expect(attachmentEmbed("attachments/photo.png")).toBe("![[attachments/photo.png]]");
  });

  /**
   * The regression that ends the two-inserter era. `attachment_markdown` used
   * to produce `![photo.png](attachments/photo.png)` for the same file, which
   * `live-preview.ts` renders as nothing at all.
   */
  it("never writes the CommonMark spelling the deleted Rust inserter produced", () => {
    expect(attachmentEmbed("attachments/photo.png")).not.toContain("](");
  });
});

describe("attachmentName", () => {
  it("is the last segment, and a path with no slash is its own name", () => {
    expect(attachmentName("a/b/c.png")).toBe("c.png");
    expect(attachmentName("c.png")).toBe("c.png");
  });
});

describe("planAttachments", () => {
  it("writes one embed per file, in the order offered, newline-separated", () => {
    const plan = planAttachments("intro\n", ["b/second.png", "a/first.png"]);

    expect(plan.text).toBe("![[b/second.png]]\n![[a/first.png]]");
    expect(plan.inserted).toEqual(["b/second.png", "a/first.png"]);
    expect(plan.refusal).toBeNull();
  });

  it("refuses a file the note already holds, and says which and why", () => {
    const plan = planAttachments("![[attachments/photo.png]]\n", ["attachments/photo.png"]);

    expect(plan.text).toBe("");
    expect(plan.inserted).toEqual([]);
    expect(plan.alreadyThere).toEqual(["attachments/photo.png"]);
    // A sentence, not a silence: doing nothing without saying so is the
    // failure this story exists to end.
    expect(plan.refusal).toBe("photo.png is already in this note, so keeper left it out.");
  });

  it("refuses only the duplicates in a mixed selection and writes the rest", () => {
    const plan = planAttachments("![[old/screen.mov]]\n", [
      "new/screen.mov",
      "attachments/map.pdf",
      "attachments/photo.png",
    ]);

    // Matched by name across a Story 40.4 folder rename, which is the whole
    // reason the key is the name.
    expect(plan.alreadyThere).toEqual(["new/screen.mov"]);
    expect(plan.text).toBe("![[attachments/map.pdf]]\n![[attachments/photo.png]]");
    expect(plan.refusal).toBe("screen.mov is already in this note, so keeper left it out.");
  });

  it("lists several refusals the way a person would say them", () => {
    const plan = planAttachments("![[a.png]] ![[b.png]] ![[c.png]]\n", ["a.png", "b.png", "c.png"]);

    expect(plan.refusal).toBe(
      "a.png, b.png and c.png are already in this note, so keeper left them out.",
    );
  });

  it("writes one embed when the same file is offered twice in one gesture", () => {
    const plan = planAttachments("", ["attachments/photo.png", "attachments/photo.png"]);

    expect(plan.text).toBe("![[attachments/photo.png]]");
    expect(plan.alreadyThere).toEqual(["attachments/photo.png"]);
  });

  it("counts the CommonMark embed spelling as already holding the file", () => {
    const plan = planAttachments("![A photo](attachments/photo.png)\n", ["attachments/photo.png"]);

    expect(plan.inserted).toEqual([]);
    expect(plan.refusal).toContain("already in this note");
  });

  it("does not count a mention, because a link is not a copy of the picture", () => {
    const plan = planAttachments("[[attachments/photo.png]]\n", ["attachments/photo.png"]);

    expect(plan.text).toBe("![[attachments/photo.png]]");
    expect(plan.refusal).toBeNull();
  });

  it("refuses a name no wikilink can spell, naming the characters", () => {
    const plan = planAttachments("", ["attachments/why#not.png"]);

    expect(plan.text).toBe("");
    expect(plan.unnameable).toEqual(["attachments/why#not.png"]);
    expect(plan.refusal).toContain("an embed cannot spell");
    expect(plan.refusal).toContain("Renaming the file is the fix.");
  });

  it("says both things when one file is a duplicate and another cannot be spelled", () => {
    const plan = planAttachments("![[a.png]]\n", ["a.png", "b|c.png", "d.png"]);

    expect(plan.text).toBe("![[d.png]]");
    expect(plan.refusal).toContain("a.png is already in this note");
    expect(plan.refusal).toContain("b|c.png has a name an embed cannot spell");
  });

  /**
   * The negative property every refusal shares, asserted as a class rather
   * than sentence by sentence.
   *
   * The property: **a file keeper actually wrote is never named in the sentence
   * about what it did not write**, and its converse. Telling somebody a file
   * was skipped when it went in is the mirror of this story's original bug —
   * silently doing nothing — and is just as wrong.
   *
   * Three stories in this wave hit one hole from three directions: a mutation
   * that changes what the user READS survives any assertion that only checks
   * the SHAPE of what they read (`toContain`, a distinct-strings count, "some
   * refusal appeared"). W2Media's remedy is the cheap one and it is the shape
   * used here — pin the negative property the class shares rather than N exact
   * sentences, so it fails on the class of mutation and not on one instance.
   *
   * **Measured honestly: this catches nothing the tests above miss today.**
   * Two cross-bucket mutations were run against it — a duplicate written as
   * well as refused, and the refusal built from every offered path — and both
   * were caught here AND by five to seven of the instance tests, which pin
   * `inserted`, `alreadyThere` and `unnameable` directly. It is never the only
   * failure. It is kept because it is the one assertion a future refusal
   * bucket inherits for free: a fifth reason to decline a file gets the
   * invariant without anyone remembering to write its instance test. Anyone
   * trimming this suite should know it is redundant today and why it is here.
   */
  it("never names a file it wrote in the sentence about what it did not write", () => {
    const cases: [string, string[]][] = [
      ["", ["a.png", "b.png"]],
      ["![[a.png]]\n", ["a.png", "b.png", "c.png"]],
      ["![[a.png]] ![alt](deep/b.png)\n", ["a.png", "b.png", "d|e.png", "f.png"]],
      ["![[x.png]]\n", ["deep/X.PNG", "why#not.png", "ok.png"]],
    ];

    for (const [body, paths] of cases) {
      const plan = planAttachments(body, paths);
      for (const written of plan.inserted) {
        // The embed for it is in the text, and its name is nowhere in the
        // sentence about what was left out.
        expect(plan.text, `${body} + ${paths}`).toContain(attachmentEmbed(written));
        expect(plan.refusal ?? "", `${written} was written, so must not be refused`).not.toContain(
          attachmentName(written),
        );
      }
      // And the converse, which is the same invariant read the other way: a
      // file that was refused is not in the text.
      for (const skipped of [...plan.alreadyThere, ...plan.unnameable]) {
        expect(plan.text, `${skipped} was refused, so must not be written`).not.toContain(
          attachmentEmbed(skipped),
        );
      }
    }
  });
});

describe("bodyWithAttachments", () => {
  const plan = (): AttachmentPlan => planAttachments("", ["attachments/photo.png"]);

  it("adds a separator to a body that has none, and never a terminator", () => {
    expect(bodyWithAttachments("intro", plan())).toBe("intro\n![[attachments/photo.png]]");
    expect(bodyWithAttachments("intro\n", plan())).toBe("intro\n![[attachments/photo.png]]");
    expect(bodyWithAttachments("", plan())).toBe("![[attachments/photo.png]]");
  });

  /**
   * A plan that writes nothing must leave the body identical, not
   * whitespace-different: the Files-pane path saves whatever comes back, and a
   * body that gained a newline would be a save, a sync and a commit for a
   * gesture that was refused.
   */
  it("returns the body unchanged when the plan writes nothing", () => {
    const refused = planAttachments("![[a.png]]", ["a.png"]);
    expect(bodyWithAttachments("![[a.png]]", refused)).toBe("![[a.png]]");
  });
});

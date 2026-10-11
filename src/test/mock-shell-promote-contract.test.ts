/**
 * The dev harness's promote commands serve Rust's contract, pinned by
 * `promote-vectors.json`, which `keeper_core::sessions::offer`'s and
 * `keeper_agent::promote`'s tests load as well: the row, checklist and copy
 * revisions as Rust frames them, the text test's verdicts on exact bytes,
 * the decisions over a person's intent and the reader bounds. The digests
 * in the table were computed independently of both implementations, and the
 * plain-TypeScript SHA-256 a dev server over plain http falls back to is
 * held to them with WebCrypto absent.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PanelIntentVm } from "@/lib/ipc/client";
import {
  PROMOTE_MAX_NOTE_BYTES,
  PROMOTE_MAX_REVIEWED_BYTES,
  promoteCopyRevision,
  promoteDecisions,
  promoteIsText,
  promoteRowRevision,
  promoteSnapshotRevision,
  sha256Hex,
} from "../../dev/mock-shell";

const VECTORS = JSON.parse(
  readFileSync(
    resolve(
      import.meta.dirname,
      "../../src-tauri/crates/keeper-core/src/sessions/promote-vectors.json",
    ),
    "utf8",
  ),
) as {
  maxNoteBytes: number;
  maxReviewedBytes: number;
  sha256: { text: string; repeat: number; digest: string }[];
  rowRevision: {
    source: string;
    target: string;
    note: string;
    stamp: string | null;
    targetFact: string | null;
    revision: string;
  }[];
  snapshotRevision: {
    readme: string;
    stamps: Record<string, string>;
    targets: Record<string, string>;
    revision: string;
  }[];
  copyRevision: { target: string; text: string; revision: string }[];
  isText: { hex: string; text: boolean }[];
  decisions: {
    items: { revision: string; refused: boolean }[];
    notes: { path: string; revision: string | null; copy: string | null }[];
    intent: PanelIntentVm;
    expected: unknown;
  }[];
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the dev harness's promote contract", () => {
  it("hashes as Rust does, with WebCrypto and without it", async () => {
    for (const subtle of [true, false]) {
      if (!subtle) vi.stubGlobal("crypto", {});
      for (const vector of VECTORS.sha256) {
        expect(await sha256Hex(vector.text.repeat(vector.repeat))).toBe(vector.digest);
      }
      for (const row of VECTORS.rowRevision) {
        expect(
          await promoteRowRevision(row.source, row.target, row.note, row.stamp, row.targetFact),
        ).toBe(row.revision);
      }
      for (const snapshot of VECTORS.snapshotRevision) {
        expect(
          await promoteSnapshotRevision(snapshot.readme, snapshot.stamps, snapshot.targets),
        ).toBe(snapshot.revision);
      }
      for (const copy of VECTORS.copyRevision) {
        expect(await promoteCopyRevision(copy.target, copy.text)).toBe(copy.revision);
      }
    }
  });

  it("tells text by its bytes as Rust does: invalid UTF-8 is not, a NUL is", () => {
    expect(
      VECTORS.isText.map((vector) =>
        promoteIsText(
          new Uint8Array((vector.hex.match(/../g) ?? []).map((pair) => Number.parseInt(pair, 16))),
        ),
      ),
    ).toEqual(VECTORS.isText.map((vector) => vector.text));
  });

  it("decides a person's intent as Rust does, and reads within Rust's bounds", () => {
    for (const vector of VECTORS.decisions) {
      expect(promoteDecisions(vector.items, vector.notes, vector.intent)).toEqual(vector.expected);
    }
    expect([PROMOTE_MAX_NOTE_BYTES, PROMOTE_MAX_REVIEWED_BYTES]).toEqual([
      VECTORS.maxNoteBytes,
      VECTORS.maxReviewedBytes,
    ]);
  });
});

/**
 * Zero-egress source-scan audit over the recording UI (Story 20.4, FR-76,
 * epic-20 exit invariant).
 *
 * The recording surfaces must carry NO upload, share-link, transcription, or
 * cloud affordance and no direct network call — recording is fully local. This
 * test reads the recording production sources off disk (node `fs`, never the
 * bundler) and fails loudly — naming the offending file and token — if such an
 * affordance ever appears. It mirrors the Rust `dependency_firewall_holds`
 * pattern: every forbidden token is built by string concatenation so this scan
 * file never matches itself.
 *
 * Token design: functional network tokens are matched case-insensitively; the
 * affordance words are matched as capitalized whole words ("Upload", "Share",
 * "Cloud") because a UI affordance surfaces as a Title-case label/identifier,
 * while the honest lowercase disclosures the copy is REQUIRED to carry
 * ("Nothing uploads.", "no … share, upload, or cloud affordance" in doc
 * comments) must not trip the audit. The per-surface rendered-DOM tests (e.g.
 * the destination card's local-only assertion) cover the lowercase rendered
 * side.
 */
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/** This test file's directory: `src/components/recording`. */
const HERE = dirname(fileURLToPath(import.meta.url));
/** The frontend source root (`src/`). */
const SRC = resolve(HERE, "../../");

/** Production (non-test) sources in `dir` whose basename matches `predicate`. */
function scanDir(dir: string, predicate: (name: string) => boolean): string[] {
  return readdirSync(dir)
    .filter((name) => predicate(name) && !name.endsWith(".test.ts") && !name.endsWith(".test.tsx"))
    .map((name) => join(dir, name));
}

/** Every recording production source. The recording feature is namespaced by
 * prefix, so this globs by pattern rather than a hand-maintained list — a new
 * recording store/hook/lib file is scanned automatically, closing the gap where
 * an egress affordance could land in an un-enumerated file and pass vacuously:
 *   - `src/components/recording/*.tsx` (this dir),
 *   - the two layout surfaces + the palette handlers,
 *   - `src/lib/recording-*.ts` and `src/lib/stores/recording-*.ts`,
 *   - `src/hooks/use-record*.ts` (use-recording-*, use-recorded-*). */
function recordingSources(): string[] {
  return [
    ...scanDir(HERE, (name) => name.endsWith(".tsx")),
    join(SRC, "components/layout/recording-pane.tsx"),
    join(SRC, "components/layout/recording-summary-card.tsx"),
    join(SRC, "components/command-palette/actions.ts"),
    ...scanDir(join(SRC, "lib"), (name) => name.startsWith("recording-") && name.endsWith(".ts")),
    ...scanDir(
      join(SRC, "lib/stores"),
      (name) => name.startsWith("recording-") && name.endsWith(".ts"),
    ),
    ...scanDir(join(SRC, "hooks"), (name) => name.startsWith("use-record") && name.endsWith(".ts")),
  ];
}

describe("recording UI zero-egress audit (Story 20.4, FR-76)", () => {
  it("carries no upload/share/transcription/cloud affordance and no network call", () => {
    // Functional network tokens — case-insensitive (a call is a leak in any
    // casing). Built by concatenation: never self-matching.
    const functionalTokens: RegExp[] = [
      new RegExp(`XMLHttp${"Request"}`, "i"),
      new RegExp(`\\bfet${"ch"}\\s*\\(`, "i"),
      new RegExp(`WebSoc${"ket"}`, "i"),
      new RegExp(`sendBea${"con"}`, "i"),
      new RegExp(`EventSou${"rce"}`, "i"),
      new RegExp(`ht${"tps?"}://`, "i"),
      new RegExp(`\\baxi${"os"}\\b`, "i"),
    ];
    // Affordance tokens — capitalized whole words (a shipped affordance is a
    // Title-case label or identifier); `transcri` in any case (no honest copy
    // needs the word at all).
    const affordanceTokens: RegExp[] = [
      new RegExp(`\\bUpl${"oad"}`),
      new RegExp(`\\bSha${"re"}\\b`),
      new RegExp(`\\bClo${"ud"}\\b`),
      new RegExp(`transc${"ri"}`, "i"),
    ];
    const forbidden = [...functionalTokens, ...affordanceTokens];

    const files = recordingSources();
    expect(files.length, "the audit must never pass vacuously").toBeGreaterThan(5);

    for (const file of files) {
      // A missing file throws here — loud, never a vacuous pass.
      const source = readFileSync(file, "utf8");
      for (const token of forbidden) {
        expect(
          token.test(source),
          `zero-egress violation: ${file} matches forbidden token ${token} — ` +
            "the recording UI must stay fully local with no egress affordance (FR-76)",
        ).toBe(false);
      }
    }
  });
});

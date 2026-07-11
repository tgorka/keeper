/**
 * Repo-wide convention test (Story 12.2): the frontend must never derive
 * platform facts from the browser or the build — platform variance reaches the
 * UI exclusively through the Rust-served capability handshake
 * (`capabilities()` → `useCapabilitiesStore`).
 *
 * Scans every `src/**` TypeScript source as raw text and fails on:
 * - user-agent / platform sniffing via the `navigator` object,
 * - the Tauri OS plugin (its `platform()`/`type()`/`arch()` exist only to sniff),
 * - build-time env feature gating (Vite `import` `.meta.env`).
 *
 * Deliberately a raw-text scan (comments included): the forbidden names must
 * not appear at all, so a violation can never hide behind a lint suppression.
 * Excluded: this file itself (it names the patterns) and the generated
 * shadcn `src/components/ui/**` tree.
 */
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const SRC_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SELF = fileURLToPath(import.meta.url);
const SHADCN_UI_DIR = path.join(SRC_DIR, "components", "ui") + path.sep;

/** Forbidden pattern → why it is banned. */
const FORBIDDEN: ReadonlyArray<{ pattern: RegExp; reason: string }> = [
  {
    pattern: /navigator\s*\.\s*userAgent/,
    reason: "user-agent sniffing — read the Rust capabilities mirror instead",
  },
  {
    // `navigator.platform` only; benign navigator members (e.g. `.clipboard`)
    // must not match.
    pattern: /navigator\s*\.\s*platform\b/,
    reason: "platform sniffing — read the Rust capabilities mirror instead",
  },
  {
    pattern: /@tauri-apps\/plugin-os/,
    reason:
      "the Tauri OS plugin (platform()/type()/arch()) — read the Rust capabilities mirror instead",
  },
  {
    pattern: /import\s*\.\s*meta\s*\.\s*env/,
    reason: "build-flag feature gating — read the Rust capabilities mirror instead",
  },
];

/** Recursively collect every `.ts`/`.tsx` file under `dir`. */
function collectSources(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectSources(full));
    } else if (/\.tsx?$/.test(entry.name)) {
      files.push(full);
    }
  }
  return files;
}

function isExcluded(file: string): boolean {
  return file === SELF || file.startsWith(SHADCN_UI_DIR);
}

describe("no user-agent / build-flag feature gating", () => {
  it("no src/ file consults the user agent, the OS plugin, or build env flags", () => {
    const violations: string[] = [];
    for (const file of collectSources(SRC_DIR)) {
      if (isExcluded(file)) {
        continue;
      }
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, index) => {
        for (const { pattern, reason } of FORBIDDEN) {
          if (pattern.test(line)) {
            violations.push(
              `${path.relative(SRC_DIR, file)}:${index + 1} — ${reason}\n    ${line.trim()}`,
            );
          }
        }
      });
    }
    expect(
      violations,
      `platform gating must come from the Rust capabilities handshake, never the browser or build flags:\n${violations.join("\n")}`,
    ).toEqual([]);
  });

  it("the scanner actually sees the tree (sanity guard against an empty scan)", () => {
    // If the walk ever silently scanned nothing (moved dir, glob typo), the
    // convention test above would pass vacuously — fail loudly instead.
    const sources = collectSources(SRC_DIR).filter((file) => !isExcluded(file));
    expect(sources.length).toBeGreaterThan(100);
  });
});

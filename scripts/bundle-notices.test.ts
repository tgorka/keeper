import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

// The third-party notices (NOTICE, and the licence texts under licenses/) must
// travel inside every app that links the code they name. The built bundles are
// checked by scripts/lib/bundle-guard.sh on the install paths; this pins the
// two configurations that put them there, so a dropped line fails on Linux
// before any Mac builds anything.

const ROOT = process.cwd();
const KEEPER = join(ROOT, "src-tauri/crates/keeper");
const APPLE = join(KEEPER, "gen/apple");

/** NOTICE and every file under licenses/, repository-relative. */
function notices(): string[] {
  const files = ["NOTICE"];
  const walk = (dir: string) => {
    for (const name of readdirSync(join(ROOT, dir)).sort()) {
      const rel = `${dir}/${name}`;
      if (statSync(join(ROOT, rel)).isDirectory()) {
        walk(rel);
      } else {
        files.push(rel);
      }
    }
  };
  walk("licenses");
  return files;
}

/**
 * The `- path:` entries of `keeper_iOS`'s `sources`, each with its keys, read
 * structurally from the target's own block rather than found anywhere in the
 * file.
 */
function iosSources(): Array<Record<string, string>> {
  const lines = readFileSync(join(APPLE, "project.yml"), "utf8").split("\n");
  const target = lines.indexOf("  keeper_iOS:");
  const sources = lines.indexOf("    sources:", target);
  expect(target).toBeGreaterThan(-1);
  expect(sources).toBeGreaterThan(target);
  const entries: Array<Record<string, string>> = [];
  for (const line of lines.slice(sources + 1)) {
    if (/^ {0,5}\S/.test(line)) {
      break;
    }
    const item = line.match(/^ {6}- (\w+): (.+)$/);
    const key = line.match(/^ {8}(\w+): (.+)$/);
    if (item) {
      entries.push({ [item[1]]: item[2] });
    } else if (key && entries.length > 0) {
      entries[entries.length - 1][key[1]] = key[2];
    }
  }
  return entries;
}

describe("third-party notices in the app bundles", () => {
  it("has a licence text for ONNX Runtime and its ThirdPartyNotices", () => {
    expect(notices()).toEqual(
      expect.arrayContaining([
        "NOTICE",
        "licenses/onnxruntime-1.28.0/LICENSE",
        "licenses/onnxruntime-1.28.0/ThirdPartyNotices.txt",
      ]),
    );
  });

  it("records each licence text's sha256 in NOTICE, so a changed copy is caught", () => {
    const notice = readFileSync(join(ROOT, "NOTICE"), "utf8");
    for (const rel of notices().filter((file) => file.startsWith("licenses/onnxruntime"))) {
      const sha = createHash("sha256")
        .update(readFileSync(join(ROOT, rel)))
        .digest("hex");
      expect(notice.replace(/\s+/g, " "), rel).toContain(sha);
    }
  });

  it("ships every one in the macOS app's Contents/Resources", () => {
    const conf = JSON.parse(readFileSync(join(KEEPER, "tauri.conf.json"), "utf8"));
    const files: Record<string, string> = conf.bundle.macOS.files ?? {};
    for (const rel of notices()) {
      const source = files[`Resources/${rel}`];
      expect(source, rel).toBeDefined();
      expect(relative(ROOT, resolve(KEEPER, source)), rel).toBe(rel);
    }
  });

  it("ships NOTICE and the whole licenses/ folder at the top of the iOS app", () => {
    const sources = iosSources();
    for (const rel of ["NOTICE", "licenses"]) {
      const entry = sources.find(
        (source) => source.path && relative(ROOT, resolve(APPLE, source.path)) === rel,
      );
      expect(entry, rel).toBeDefined();
      expect(entry?.buildPhase, rel).toBe("resources");
      expect(existsSync(resolve(APPLE, entry?.path ?? "")), rel).toBe(true);
    }
    const folder = sources.find((source) => source.path?.endsWith("/licenses"));
    expect(folder?.type).toBe("folder");
  });
});

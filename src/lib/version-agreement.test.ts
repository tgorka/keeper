import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * One version, spelled in four files, and nothing but this test connects them.
 *
 * `scripts/release-macos.sh` checks the tag against `tauri.conf.json` and
 * nothing else, which is exactly as far as it can see: it is handed a tag and a
 * tree. So the other three spellings are unchecked, and the failure is not
 * hypothetical — v0.8.21 shipped with `package.json` still saying `0.8.20` and
 * needed the follow-up commit b1e196c ("fix(release): carry package.json to
 * 0.8.21") to correct it. That is the whole reason this file exists.
 *
 * Why it matters rather than being tidy: `package.json`'s version is what the
 * webview build stamps into the About surface and what a bug report quotes, so a
 * stale one means a user tells you they are on a version nobody released. The
 * Cargo workspace version is what the binary reports. `tauri.conf.json` is what
 * the updater compares against `latest.json` to decide whether an update is
 * even offered — the one place a disagreement stops being cosmetic and starts
 * meaning "no update for anyone".
 *
 * `Cargo.lock` is included because a bump that edits `Cargo.toml` and forgets to
 * refresh the lock leaves a tree that builds a DIFFERENT version than it claims,
 * and the release build is where that surfaces — after ten minutes of signing.
 */
const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../..");

/** The workspace version, from the one `[workspace.package]` table. */
function cargoWorkspaceVersion(): string {
  const toml = readFileSync(resolve(ROOT, "src-tauri/Cargo.toml"), "utf8");
  // The first `version = "…"` after the `[workspace.package]` header. Parsed
  // narrowly on purpose: a TOML dependency would be a build-time dependency of
  // the test suite for one line of string matching.
  const table = toml.slice(toml.indexOf("[workspace.package]"));
  const match = /^version\s*=\s*"([^"]+)"/m.exec(table);
  if (match === null) {
    throw new Error("src-tauri/Cargo.toml has no [workspace.package] version");
  }
  return match[1];
}

/** Every workspace crate's version as the lockfile records it. */
function lockedWorkspaceVersions(): Record<string, string> {
  const lock = readFileSync(resolve(ROOT, "src-tauri/Cargo.lock"), "utf8");
  const wanted = ["keeper", "keeper-core", "keeper-sync", "keeper-syncd"];
  const found: Record<string, string> = {};
  for (const name of wanted) {
    // `name = "x"` then the very next `version = "…"`; a locked crate's two
    // fields are adjacent in every lockfile cargo writes.
    const pattern = new RegExp(`name = "${name}"\\nversion = "([^"]+)"`);
    const match = pattern.exec(lock);
    if (match === null) {
      throw new Error(`src-tauri/Cargo.lock has no entry for ${name}`);
    }
    found[name] = match[1];
  }
  return found;
}

describe("the version is one number", () => {
  it("agrees across package.json, the Cargo workspace and tauri.conf.json", () => {
    const pkg = JSON.parse(readFileSync(resolve(ROOT, "package.json"), "utf8")) as {
      version: string;
    };
    const tauri = JSON.parse(
      readFileSync(resolve(ROOT, "src-tauri/crates/keeper/tauri.conf.json"), "utf8"),
    ) as { version: string };

    // Asserted as one object so a failure names every spelling at once: told
    // only that two strings differ, the next person has to go and look up which
    // of the four files is the stale one.
    expect({
      packageJson: pkg.version,
      cargoWorkspace: cargoWorkspaceVersion(),
      tauriConf: tauri.version,
    }).toEqual({
      packageJson: pkg.version,
      cargoWorkspace: pkg.version,
      tauriConf: pkg.version,
    });
  });

  it("is the version the lockfile actually builds", () => {
    const expected = cargoWorkspaceVersion();
    expect(lockedWorkspaceVersions()).toEqual({
      keeper: expected,
      "keeper-core": expected,
      "keeper-sync": expected,
      "keeper-syncd": expected,
    });
  });

  it("is a plain three-part version, so a tag can be derived from it", () => {
    // The release script computes `VERSION="${TAG#v}"` and compares. A version
    // carrying a pre-release suffix would still pass that comparison and then
    // name assets — `keeper_0.8.22-rc1_aarch64.dmg` — that the updater's own
    // manifest builder spells differently. Nobody has needed one yet, so the
    // shape is pinned rather than the handling invented.
    const pkg = JSON.parse(readFileSync(resolve(ROOT, "package.json"), "utf8")) as {
      version: string;
    };
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+$/);
  });
});

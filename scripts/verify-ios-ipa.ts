#!/usr/bin/env bun
/**
 * iOS compile-seam verifier (FR-56).
 *
 * Proves that the desktop-only Tauri plugins never leak into the iOS build, both
 * against the dependency graph (authoritative) and, when present, against the
 * shipped IPA's Mach-O executable (best-effort artifact confirmation).
 *
 * The forbidden set is the crates declared in
 * `src-tauri/crates/keeper/Cargo.toml`'s
 * `[target.'cfg(not(any(target_os = "ios", target_os = "android")))'.dependencies]`
 * block. They are structurally excluded from the iOS target and already enforced
 * every CI run by `cargo check --target aarch64-apple-ios`. `tauri-plugin-deep-link`
 * is cross-platform and is deliberately NOT in this set.
 *
 * Two layers:
 *   1. Dependency-graph seam check (always runs, load-bearing): the forbidden crates
 *      must be ABSENT from the `aarch64-apple-ios` tree and PRESENT in the
 *      `aarch64-apple-darwin` tree (a differential control — if they are also absent
 *      on desktop the tree is mis-scoped and we refuse to false-pass).
 *   2. IPA symbol scan (optional, informational): if an IPA exists, dump its Mach-O
 *      symbols and assert none of the forbidden plugin namespaces appear. Release iOS
 *      binaries are usually stripped, so a NON-match cannot prove absence — only the
 *      graph assertion above is authoritative; a match is a strong signal worth chasing.
 *
 * Read-only, idempotent, fail-closed. Self-contained (no external deps). Runs under Bun.
 */

import { spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Forbidden desktop-only crates, keyed to the `cfg(not(any(ios,android)))` block in
 * `src-tauri/crates/keeper/Cargo.toml`. Keep this list in sync with that manifest.
 *   - `crate`  matches a `<crate> vX.Y.Z` line in `cargo tree` output.
 *   - `symbol` is the Rust symbol-namespace prefix that would appear in a Mach-O
 *     built with the crate linked in (underscored crate name). Rust mangles symbols as
 *     length-prefixed segments (e.g. `_ZN27tauri_plugin_global_shortcut...`), so a
 *     plain substring test — not a word-boundary regex — is the correct matcher.
 */
export const FORBIDDEN = [
  { crate: "tray-icon", symbol: "tray_icon" },
  { crate: "tauri-plugin-global-shortcut", symbol: "tauri_plugin_global_shortcut" },
  { crate: "tauri-plugin-autostart", symbol: "tauri_plugin_autostart" },
  { crate: "tauri-plugin-updater", symbol: "tauri_plugin_updater" },
  { crate: "tauri-plugin-process", symbol: "tauri_plugin_process" },
] as const;

type Forbidden = readonly { readonly crate: string; readonly symbol: string }[];

const IOS_TARGET = "aarch64-apple-ios";
const DESKTOP_TARGET = "aarch64-apple-darwin";

/** Repo root, resolved relative to this script (`<root>/scripts/verify-ios-ipa.ts`). */
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SRC_TAURI = join(REPO_ROOT, "src-tauri");
/** Tauri writes the iOS IPA under this (gitignored) tree; the exact filename/subdir is
 *  product- and export-derived, so we search it rather than hard-code one name. */
const IPA_BUILD_DIR = join(SRC_TAURI, "crates/keeper/gen/apple/build");

/** Mach-O / universal-binary magic numbers (first 4 bytes, either endianness). */
const MACHO_MAGICS = new Set([
  0xfeedface, 0xcefaedfe, 0xfeedfacf, 0xcffaedfe, 0xcafebabe, 0xbebafeca,
]);

/** Run a command, capturing stdout. Throws (fail-closed) on spawn error or non-zero exit. */
function run(cmd: string, args: string[], cwd?: string): string {
  const result = spawnSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (result.error) {
    const code = (result.error as NodeJS.ErrnoException).code;
    if (code === "ENOENT") {
      throw new Error(`required tool not found on PATH: \`${cmd}\``);
    }
    throw new Error(`failed to run \`${cmd}\`: ${result.error.message}`);
  }
  if (result.status !== 0) {
    const stderr = (result.stderr ?? "").trim();
    throw new Error(
      `\`${cmd} ${args.join(" ")}\` exited ${result.status}${stderr ? `:\n${stderr}` : ""}`,
    );
  }
  return result.stdout ?? "";
}

/** Best-effort symbol dump. Returns text, or flags a truncated (maxBuffer) / missing tool
 *  without throwing, so the informational IPA scan degrades instead of failing closed. */
function tryDump(cmd: string, args: string[]): { text: string; truncated: boolean } {
  const result = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  if (result.error) {
    const code = (result.error as NodeJS.ErrnoException).code;
    const truncated = code === "ENOBUFS" || /maxBuffer/i.test(result.error.message);
    return { text: result.stdout ?? "", truncated };
  }
  return { text: result.stdout ?? "", truncated: false };
}

/**
 * PURE. Return the set of forbidden crate names present in `cargo tree` output text.
 * Extracted from the cargo invocation so the parsing/fail-closed logic is unit-testable.
 */
export function parseForbiddenCrates(treeText: string, forbidden: Forbidden): Set<string> {
  const present = new Set<string>();
  for (const { crate } of forbidden) {
    // `cargo tree` prints tree-drawing glyphs, then `<crate> vX.Y.Z`.
    const pattern = new RegExp(`(^|[\\s│├└─])${escapeRegExp(crate)} v\\d`, "m");
    if (pattern.test(treeText)) {
      present.add(crate);
    }
  }
  return present;
}

/** PURE. Return the forbidden symbols found in a symbol/strings dump (substring match). */
export function findForbiddenSymbols(dump: string, forbidden: Forbidden): string[] {
  return forbidden.filter(({ symbol }) => dump.includes(symbol)).map(({ symbol }) => symbol);
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function forbiddenCratesInTree(target: string): Set<string> {
  const tree = run(
    "cargo",
    ["tree", "-p", "keeper", "--target", target, "-e", "normal"],
    SRC_TAURI,
  );
  return parseForbiddenCrates(tree, FORBIDDEN);
}

/**
 * Authoritative seam check. Returns true on pass, false on any seam violation
 * (leaked crate on iOS, or the differential control failing on desktop).
 */
function checkDependencyGraph(): boolean {
  console.log("Dependency-graph seam check (authoritative):");

  const iosPresent = forbiddenCratesInTree(IOS_TARGET);
  const desktopPresent = forbiddenCratesInTree(DESKTOP_TARGET);

  let ok = true;

  console.log(`  iOS target (${IOS_TARGET}) — forbidden crates must be ABSENT:`);
  for (const { crate } of FORBIDDEN) {
    if (iosPresent.has(crate)) {
      console.log(`    FAIL  ${crate}: LEAKED into the iOS build closure`);
      ok = false;
    } else {
      console.log(`    OK    ${crate}: absent`);
    }
  }

  console.log(`  Desktop control (${DESKTOP_TARGET}) — same crates must be PRESENT:`);
  for (const { crate } of FORBIDDEN) {
    if (desktopPresent.has(crate)) {
      console.log(`    OK    ${crate}: present`);
    } else {
      console.log(`    FAIL  ${crate}: missing from desktop tree (control failing)`);
      ok = false;
    }
  }

  if (desktopPresent.size === 0) {
    console.error(
      "\n  REFUSING TO PASS: the desktop control shows zero forbidden crates. The tree is\n" +
        "  mis-scoped (wrong package or target), so the iOS 'absent' result is not meaningful.",
    );
    ok = false;
  }

  return ok;
}

/** True if `file` starts with a Mach-O / universal-binary magic number. */
function isMachO(file: string): boolean {
  let fd: number | undefined;
  try {
    fd = openSync(file, "r");
    const buf = Buffer.alloc(4);
    if (readSync(fd, buf, 0, 4, 0) < 4) {
      return false;
    }
    return MACHO_MAGICS.has(buf.readUInt32BE(0)) || MACHO_MAGICS.has(buf.readUInt32LE(0));
  } catch {
    return false;
  } finally {
    if (fd !== undefined) {
      closeSync(fd);
    }
  }
}

/** Find the app's main Mach-O executable inside `Payload/<app>.app`, or null if none. */
function findMachOExecutable(appPath: string, appName: string): string | null {
  // The main executable is normally named after the bundle (CFBundleExecutable).
  const named = join(appPath, appName);
  if (existsSync(named) && statSync(named).isFile() && isMachO(named)) {
    return named;
  }
  // Fallback: the largest top-level file that is actually a Mach-O (skips Assets.car,
  // resource blobs, etc. that a naive largest-file pick would wrongly scan).
  const machoFiles = readdirSync(appPath, { withFileTypes: true })
    .filter((e) => e.isFile())
    .map((e) => join(appPath, e.name))
    .filter(isMachO)
    .sort((a, b) => statSync(b).size - statSync(a).size);
  return machoFiles[0] ?? null;
}

/** Recursively find the first `*.ipa` under `dir`, or null. */
function findIpa(dir: string): string | null {
  if (!existsSync(dir)) {
    return null;
  }
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      const nested = findIpa(full);
      if (nested) {
        return nested;
      }
    } else if (entry.isFile() && entry.name.endsWith(".ipa")) {
      return full;
    }
  }
  return null;
}

/**
 * Optional IPA symbol scan. Returns true on pass (or inconclusive skip). Returns false
 * only when a forbidden symbol is found in the shipped binary. Throws (fail-closed) only
 * on a structural IPA problem (bad archive, no Payload, no Mach-O).
 */
function scanIpa(ipaPath: string | null): boolean {
  console.log("\nIPA artifact scan (best-effort confirmation):");

  if (ipaPath === null) {
    console.log("  SKIP  no IPA found (looked under");
    console.log(`          ${IPA_BUILD_DIR}).`);
    console.log(
      "        Build one with `bun run tauri ios build --export-method debugging`,\n" +
        "        then re-run to scan the shipped artifact. The graph check above is the\n" +
        "        load-bearing proof, so this skip does not affect the result.",
    );
    return true;
  }

  const tempDir = mkdtempSync(join(tmpdir(), "keeper-ipa-"));
  try {
    run("unzip", ["-q", "-o", ipaPath, "-d", tempDir]);

    const payload = join(tempDir, "Payload");
    if (!existsSync(payload)) {
      throw new Error(`IPA has no Payload/ directory: ${ipaPath}`);
    }
    const appDirs = readdirSync(payload).filter((name) => name.endsWith(".app"));
    if (appDirs.length === 0) {
      throw new Error(`IPA Payload/ contains no .app bundle: ${ipaPath}`);
    }
    // App extensions live under <app>.app/PlugIns, not directly under Payload/, so a
    // second top-level .app is unusual; scan the first and note it if there are more.
    if (appDirs.length > 1) {
      console.log(
        `  note: ${appDirs.length} .app bundles at Payload/ root; scanning ${appDirs[0]}`,
      );
    }
    const appDir = appDirs[0];
    const appName = appDir.replace(/\.app$/, "");
    const appPath = join(payload, appDir);

    const executable = findMachOExecutable(appPath, appName);
    if (executable === null) {
      console.log(`  SKIP  could not locate a Mach-O executable in Payload/${appDir};`);
      console.log("        artifact scan inconclusive (the graph check remains authoritative).");
      return true;
    }
    console.log(`  Scanning: Payload/${appDir}/${executable.slice(appPath.length + 1)}`);

    // `nm -gU` lists external defined symbols (`-U` = defined-only is macOS-`nm`
    // semantics; iOS artifacts only exist on macOS). Union with `strings` because a
    // binary can carry symbols one tool surfaces and the other misses. Either dump
    // exceeding its buffer degrades the scan to inconclusive rather than failing closed.
    const dumps: string[] = [];
    let inconclusive = false;
    for (const [cmd, args] of [
      ["nm", ["-gU", executable]],
      ["strings", ["-a", executable]],
    ] as const) {
      const { text, truncated } = tryDump(cmd, args);
      if (text.length > 0) {
        dumps.push(text);
      }
      if (truncated) {
        inconclusive = true;
      }
    }

    if (dumps.length === 0) {
      console.log("  SKIP  symbol dump empty/unavailable; artifact scan inconclusive");
      console.log("        (the graph check remains authoritative).");
      return true;
    }

    const found = findForbiddenSymbols(dumps.join("\n"), FORBIDDEN);
    for (const { crate, symbol } of FORBIDDEN) {
      if (found.includes(symbol)) {
        console.log(`    FAIL  ${crate}: symbol \`${symbol}\` present in ${executable}`);
      } else {
        console.log(`    OK    ${crate}: no \`${symbol}\` symbols`);
      }
    }
    if (found.length > 0) {
      return false;
    }
    console.log(
      `        No forbidden symbols found${inconclusive ? " in the (truncated) dump" : ""}.\n` +
        "        A non-match cannot prove absence in a stripped release binary — the graph\n" +
        "        check is the authoritative proof. (For symbol-level certainty, scan the\n" +
        "        unstripped Rust staticlib under gen/apple/Externals/ after a build.)",
    );
    return true;
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
}

function main(): void {
  const args = process.argv.slice(2);
  if (args.includes("-h") || args.includes("--help")) {
    console.log("usage: bun run verify:ios-ipa [path-to-ipa]");
    console.log("  Verifies the desktop/iOS compile seam (FR-56). With no argument it runs");
    console.log("  the authoritative dependency-graph check and scans a built IPA if one");
    console.log(`  exists under ${IPA_BUILD_DIR}.`);
    return;
  }
  if (args.length > 1) {
    throw new Error("too many arguments; usage: bun run verify:ios-ipa [path-to-ipa]");
  }

  const ipaPath = args[0] ? resolve(process.cwd(), args[0]) : findIpa(IPA_BUILD_DIR);

  const graphOk = checkDependencyGraph();
  const ipaOk = scanIpa(ipaPath);

  console.log("");
  if (graphOk && ipaOk) {
    console.log("iOS compile-seam verification PASSED.");
    return;
  }
  console.error("iOS compile-seam verification FAILED.");
  process.exit(1);
}

if (import.meta.main) {
  try {
    main();
  } catch (error) {
    console.error(
      "iOS compile-seam verification ERROR:",
      error instanceof Error ? error.message : String(error),
    );
    process.exit(1);
  }
}

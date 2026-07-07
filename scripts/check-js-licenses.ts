#!/usr/bin/env bun
/**
 * JS license firewall gate.
 *
 * Enumerates installed npm dependencies (top-level, scoped, and nested/non-hoisted)
 * from `node_modules` and classifies each package's declared SPDX license.
 *
 * This is a DENYLIST gate: it fails the build on copyleft licenses (GPL/AGPL/LGPL
 * family + SSPL) — the AD-5 firewall's named threat. Recognized-permissive licenses
 * are reported as `allow`; anything else (proprietary, source-available, custom, or
 * missing) is reported as `unknown` and is NON-FATAL. This intentionally differs from
 * the Rust half (`cargo deny check licenses`), which is an allowlist that fails closed
 * on anything unrecognized. `ALLOW_TOKENS` below only suppresses noise for known
 * permissive ids; it does not gate the build.
 *
 * Self-contained: no external license-checker dependency. Runs under Bun.
 */

/** Classification buckets for a single SPDX license expression. */
export type LicenseVerdict = "allow" | "deny" | "unknown";

/** Copyleft family matcher: GPL/AGPL/LGPL family + SSPL (case-insensitive, anchored). */
const DENY_PATTERN = /^(a?gpl|lgpl|sspl)/i;

/** Known-permissive SPDX identifiers (normalized to lowercase) — noise suppression only. */
const ALLOW_TOKENS = new Set<string>([
  "apache-2.0",
  "mit",
  "mit-0",
  "bsd-2-clause",
  "bsd-3-clause",
  "0bsd",
  "isc",
  "zlib",
  "bsl-1.0",
  "cc0-1.0",
  "mpl-2.0",
  "unicode-3.0",
  "unicode-dfs-2016",
  "openssl",
  "cdla-permissive-2.0",
  "python-2.0",
  "wtfpl",
  "unlicense",
  "blueoak-1.0.0",
]);

/** Lowercase a token and drop a trailing SPDX `+` (or-later), e.g. `Apache-2.0+`. */
function normalizeToken(token: string): string {
  return token.toLowerCase().replace(/\+$/, "");
}

/**
 * Split an SPDX expression into bare license tokens. Drops `WITH <exception>`
 * clauses (keeping the base license), strips parentheses, then splits on OR/AND.
 */
function tokenize(spdx: string): string[] {
  return spdx
    .replace(/\s+WITH\s+\S+/gi, "")
    .replace(/[()]/g, " ")
    .split(/\s+(?:OR|AND)\s+/i)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

/**
 * Classify an SPDX license expression.
 *
 * Deny wins: if any token in the expression is copyleft the whole expression is
 * denied (safe direction — a dual-licensed `MIT OR GPL` is treated as deny). An
 * expression whose tokens are all recognized-permissive is allowed. Otherwise
 * (empty, or any token unrecognized and none denied) it is unknown.
 */
export function classifyLicense(spdx: string): LicenseVerdict {
  const tokens = tokenize(spdx);
  if (tokens.length === 0) {
    return "unknown";
  }

  let allAllowed = true;
  for (const token of tokens) {
    if (DENY_PATTERN.test(token)) {
      return "deny";
    }
    if (!ALLOW_TOKENS.has(normalizeToken(token))) {
      allAllowed = false;
    }
  }

  return allAllowed ? "allow" : "unknown";
}

/** Minimal shape of the fields we read from an installed `package.json`. */
interface PackageManifest {
  name?: string;
  version?: string;
  license?: string | { type?: string };
  licenses?: Array<{ type?: string }>;
}

/**
 * Resolve the SPDX string declared by a package manifest, if any. Handles the
 * modern string form, the deprecated object form (`{ type, url }`), and the
 * deprecated `licenses` array form.
 */
export function readLicenseField(manifest: PackageManifest): string | null {
  const license = manifest.license;
  if (typeof license === "string" && license.trim().length > 0) {
    return license;
  }
  if (
    license !== null &&
    typeof license === "object" &&
    typeof license.type === "string" &&
    license.type.trim().length > 0
  ) {
    return license.type;
  }
  if (Array.isArray(manifest.licenses)) {
    const types = manifest.licenses
      .map((entry) => entry?.type)
      .filter((type): type is string => typeof type === "string" && type.trim().length > 0);
    if (types.length > 0) {
      return types.join(" OR ");
    }
  }
  return null;
}

interface ScannedPackage {
  name: string;
  version: string;
  license: string | null;
}

interface ScanResult {
  packages: ScannedPackage[];
  unreadable: number;
}

/** Enumerate every installed package manifest under `node_modules` (incl. nested). */
async function scanInstalledPackages(root: string): Promise<ScanResult> {
  const modulesDir = `${root}/node_modules`;
  // Top-level (`*/package.json`), scoped (`@*/*/package.json`), and nested/non-hoisted
  // packages (`**/node_modules/...`) so a copyleft dep cannot hide behind hoisting.
  const globs = [
    new Bun.Glob("*/package.json"),
    new Bun.Glob("@*/*/package.json"),
    new Bun.Glob("**/node_modules/*/package.json"),
    new Bun.Glob("**/node_modules/@*/*/package.json"),
  ];
  const packages: ScannedPackage[] = [];
  const seen = new Set<string>();
  let unreadable = 0;

  for (const glob of globs) {
    for await (const relativePath of glob.scan({ cwd: modulesDir, onlyFiles: true })) {
      const fullPath = `${modulesDir}/${relativePath}`;
      let manifest: PackageManifest;
      try {
        manifest = (await Bun.file(fullPath).json()) as PackageManifest;
      } catch {
        unreadable += 1;
        continue;
      }
      const name = manifest.name ?? relativePath.replace(/\/package\.json$/, "");
      const version = manifest.version ?? "0.0.0";
      const key = `${name}@${version}`;
      if (seen.has(key)) {
        continue;
      }
      seen.add(key);
      packages.push({ name, version, license: readLicenseField(manifest) });
    }
  }

  return { packages, unreadable };
}

async function main(): Promise<void> {
  const { packages, unreadable } = await scanInstalledPackages(process.cwd());

  if (packages.length === 0) {
    console.error(
      "JS license firewall ERROR: scanned 0 packages — is node_modules installed? Run `bun install`.",
    );
    process.exit(1);
  }

  const denied: string[] = [];
  const unknown: string[] = [];

  for (const pkg of packages) {
    const spdx = pkg.license;
    if (spdx === null) {
      unknown.push(`${pkg.name}@${pkg.version}: <no license field>`);
      continue;
    }
    const verdict = classifyLicense(spdx);
    if (verdict === "deny") {
      denied.push(`${pkg.name}@${pkg.version}: ${spdx}`);
    } else if (verdict === "unknown") {
      unknown.push(`${pkg.name}@${pkg.version}: ${spdx}`);
    }
  }

  if (unreadable > 0) {
    console.error(
      `Warning: ${unreadable} package manifest(s) could not be parsed and were skipped.`,
    );
  }
  if (unknown.length > 0) {
    console.error(`Unknown/unrecognized licenses (${unknown.length}, not fatal):`);
    for (const line of unknown) {
      console.error(`  ${line}`);
    }
  }

  if (denied.length > 0) {
    console.error(`\nDenied (copyleft) licenses (${denied.length}):`);
    for (const line of denied) {
      console.error(`  ${line}`);
    }
    console.error("\nJS license firewall FAILED: copyleft dependency detected.");
    process.exit(1);
  }

  console.log(`JS license firewall passed: scanned ${packages.length} packages, 0 denied.`);
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(
      "JS license firewall ERROR:",
      error instanceof Error ? error.message : String(error),
    );
    process.exit(1);
  }
}

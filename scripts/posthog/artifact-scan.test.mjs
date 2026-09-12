import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scanner = fileURLToPath(new URL("../check-client-secrets.ts", import.meta.url));
function scan(paths) {
  return spawnSync("bun", [scanner, ...paths], { encoding: "utf8" });
}

test("a clean root cannot conceal missing coverage in a second empty artifact", () => {
  const root = mkdtempSync(join(tmpdir(), "keeper-scan-"));
  try {
    const clean = join(root, "client.bin");
    const empty = join(root, "empty-app");
    writeFileSync(clean, `public ingestion: ${"phc_"}${"x".repeat(30)}`);
    mkdirSync(empty);
    assert.equal(scan([clean]).status, 0);
    assert.equal(scan([clean, empty]).status, 2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a private token spanning a stream chunk is rejected without printing it", () => {
  const root = mkdtempSync(join(tmpdir(), "keeper-scan-"));
  try {
    const credential = `phx_${"X".repeat(30)}`;
    const artifact = join(root, "client.bin");
    writeFileSync(artifact, `${"x".repeat(64 * 1024 - 2)}${credential}`);
    const result = scan([artifact]);
    assert.equal(result.status, 1);
    assert.ok(!`${result.stdout}${result.stderr}`.includes(credential));
    assert.ok(!result.stdout.includes("passed"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("ordinary ops identifiers remain buildable but 1Password service capsules are refused", () => {
  const root = mkdtempSync(join(tmpdir(), "keeper-scan-"));
  try {
    const artifact = join(root, "client.bin");
    writeFileSync(artifact, "ops_ordinary_compiler_generated_identifier");
    assert.equal(scan([artifact]).status, 0);
    const credential = `ops_eyJ${"X".repeat(270)}`;
    writeFileSync(artifact, `${"x".repeat(64 * 1024 - 4)}${credential}`);
    const result = scan([artifact]);
    assert.equal(result.status, 1);
    assert.ok(!`${result.stdout}${result.stderr}`.includes(credential));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

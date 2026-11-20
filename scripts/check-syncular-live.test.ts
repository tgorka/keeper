import { spawnSync } from "node:child_process";
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

// The live check's "none across units" line, shown to fail on a contract built
// to fail it: the ledger tables declared under `kg_graph` as well as
// `kg_history`, so the graph's state unit covers them and the server hands it
// ledger rows. The check runs from a copy of the script beside the mutated,
// regenerated contract, against a fresh schema of its own:
//
//   KEEPER_SYNC_TEST_PG_MUTANT_URL="postgres://role_m:…@host/db" \
//     bunx vitest run scripts/check-syncular-live.test.ts
//
// Without that URL the test is skipped: it needs a real Postgres, as the check
// itself does.

const MUTANT_URL = process.env.KEEPER_SYNC_TEST_PG_MUTANT_URL;
const LEDGER = ["kg_events", "kg_event_ops", "kg_head", "kg_reviews"];

describe.skipIf(!MUTANT_URL)("check:syncular:live on a ledger the state unit covers", () => {
  let root = "";

  beforeAll(() => {
    root = mkdtempSync(join(tmpdir(), "check-syncular-live-"));
    cpSync("syncular", join(root, "syncular"), { recursive: true });
    mkdirSync(join(root, "scripts"));
    cpSync("scripts/check-syncular-live.ts", join(root, "scripts/check-syncular-live.ts"));
    symlinkSync(join(process.cwd(), "node_modules"), join(root, "node_modules"));
    const manifest = join(root, "syncular/syncular.json");
    const contract = JSON.parse(readFileSync(manifest, "utf8"));
    for (const table of contract.tables) {
      if (LEDGER.includes(table.name)) {
        table.scopes = [
          { pattern: "kg_graph:{kg_graph}", column: "graph" },
          { pattern: "kg_history:{kg_history}", column: "graph" },
        ];
      }
    }
    writeFileSync(manifest, JSON.stringify(contract, null, 2));
    const generated = spawnSync(
      join(process.cwd(), "node_modules/.bin/syncular"),
      ["generate", "--manifest-dir", join(root, "syncular")],
      { encoding: "utf8" },
    );
    expect(generated.status, generated.stderr).toBe(0);
  });

  afterAll(() => {
    if (root) rmSync(root, { recursive: true, force: true });
  });

  it("fails on the line that says no unit receives the other's rows", () => {
    const run = spawnSync("bun", ["run", join(root, "scripts/check-syncular-live.ts")], {
      encoding: "utf8",
      env: { ...process.env, KEEPER_SYNC_TEST_PG_URL: MUTANT_URL },
      timeout: 120_000,
    });
    expect(run.status).not.toBe(0);
    expect(run.stderr).toMatch(/kg_\w+ reached the other graph unit/);
    expect(run.stdout).not.toContain("none across units");
  });
});

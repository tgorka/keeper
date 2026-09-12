import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { maintainerEnvironmentReady } from "./environment.mjs";

const environment = {
  name: "posthog-maintainers",
  can_admins_bypass: false,
  protection_rules: [
    { type: "required_reviewers", reviewers: [{ type: "User", reviewer: { id: 1956779 } }] },
  ],
  deployment_branch_policy: { protected_branches: false, custom_branch_policies: true },
};
const policies = { total_count: 1, branch_policies: [{ name: "main", type: "branch" }] };

test("reviewed owner and explicit main branch permit the protected job", () => {
  assert.equal(maintainerEnvironmentReady(environment, policies), true);
});

test("administrator bypass cannot release credentials without reviewer approval", () => {
  assert.equal(
    maintainerEnvironmentReady({ ...environment, can_admins_bypass: true }, policies),
    false,
  );
});

test("auto-created environment without reviewer protection cannot release credentials", () => {
  assert.ok(!maintainerEnvironmentReady({ ...environment, protection_rules: [] }, policies));
});

test("a different reviewer cannot satisfy the owner's approval contract", () => {
  const changed = structuredClone(environment);
  changed.protection_rules[0].reviewers[0].reviewer.id = 1;
  assert.equal(maintainerEnvironmentReady(changed, policies), false);
});

test("general protected branches are not equivalent to reviewed main-only deployment", () => {
  assert.equal(
    maintainerEnvironmentReady(
      {
        ...environment,
        deployment_branch_policy: { protected_branches: true, custom_branch_policies: false },
      },
      policies,
    ),
    false,
  );
});

test("a same-named tag cannot satisfy the main branch restriction", () => {
  assert.equal(
    maintainerEnvironmentReady(environment, {
      total_count: 1,
      branch_policies: [{ name: "main", type: "tag" }],
    }),
    false,
  );
});

test("a truncated policy page cannot hide an additional allowed branch", () => {
  assert.equal(maintainerEnvironmentReady(environment, { ...policies, total_count: 2 }), false);
});

test("the preflight job can read the protection it is required to verify", () => {
  const workflow = readFileSync(
    fileURLToPath(new URL("../../.github/workflows/posthog.yml", import.meta.url)),
    "utf8",
  );
  const protection = workflow.slice(
    workflow.indexOf("\n  protection:"),
    workflow.indexOf("\n  maintainer:"),
  );
  assert.ok(protection.length > 0, "the protection job must exist");
  // GitHub serves environment protection rules under `actions=read`. Without it the
  // lookup answers 403, the fail-closed preflight refuses, and the reviewed
  // maintainer job can never run however the environment is configured.
  assert.match(protection, /^ {6}actions: read$/m);
  assert.match(protection, /^ {6}contents: read$/m);
});

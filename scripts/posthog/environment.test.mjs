import assert from "node:assert/strict";
import test from "node:test";
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

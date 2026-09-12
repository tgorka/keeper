import { fileURLToPath } from "node:url";

/** A named but unprotected GitHub environment is not an approval boundary. */
export function maintainerEnvironmentReady(environment, policies) {
  const reviewers = environment?.protection_rules?.find(
    (rule) => rule.type === "required_reviewers",
  )?.reviewers;
  return (
    environment?.name === "posthog-maintainers" &&
    environment.can_admins_bypass === false &&
    reviewers?.length === 1 &&
    reviewers[0].type === "User" &&
    reviewers[0].reviewer?.id === 1956779 &&
    environment.deployment_branch_policy?.protected_branches === false &&
    environment.deployment_branch_policy?.custom_branch_policies === true &&
    policies?.total_count === 1 &&
    policies.branch_policies?.length === 1 &&
    policies.branch_policies[0].name === "main" &&
    policies.branch_policies[0].type === "branch"
  );
}

async function main() {
  const base = "https://api.github.com/repos/tgorka/keeper/environments/posthog-maintainers";
  const read = async (url) => {
    const response = await fetch(url, {
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${process.env.GH_TOKEN}`,
        "X-GitHub-Api-Version": "2022-11-28",
      },
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
    });
    if (!response.ok) throw new Error("Environment lookup failed");
    return response.json();
  };
  if (!process.env.GH_TOKEN) throw new Error("GitHub read credential missing");
  const [environment, policies] = await Promise.all([
    read(base),
    read(`${base}/deployment-branch-policies?per_page=100`),
  ]);
  if (!maintainerEnvironmentReady(environment, policies)) {
    throw new Error("Required approval protections or main-only policy missing");
  }
  console.log("Required tgorka approval, disabled admin bypass, and main-only policy verified.");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch(() => {
    console.error(
      "PostHog environment preflight failed. Configure posthog-maintainers with required tgorka approval, administrator bypass disabled, and a single main branch policy. The PostHog management-key job remains blocked.",
    );
    process.exitCode = 1;
  });
}

#!/usr/bin/env bun
/**
 * DCO gate: every non-merge commit of a pull request carries a Developer
 * Certificate of Origin sign-off (`DCO`, CONTRIBUTING.md) by its own author.
 *
 * A commit passes when its message has a `Signed-off-by: Name <email>` line
 * whose email is the commit's author email (case-insensitive), or when a
 * remediation commit in the same range — authored by the same email and itself
 * signed off — certifies it after the fact, one line per commit:
 *
 *   I, Name <email>, hereby add my Signed-off-by to this commit: <sha>
 *
 * Remediation exists so a contributor who forgot `-s` can fix the pull request
 * without rewriting and force-pushing it.
 *
 * Which pull requests are checked is the workflow's decision
 * (`.github/workflows/dco.yml` skips the repository owner and bots); this
 * script checks whatever range it is given.
 *
 * Usage: bun run scripts/check-dco.ts <base> <head>
 */
import { execFileSync } from "node:child_process";

export interface Commit {
  sha: string;
  authorName: string;
  authorEmail: string;
  message: string;
}

const SIGN_OFF = /^Signed-off-by:[^\S\n]*.*?<([^<>\s]+)>[^\S\n]*$/gim;
const REMEDIATION =
  /^I,[^\S\n]*.*?<([^<>\s]+)>,[^\S\n]*hereby add my Signed-off-by to this commit:[^\S\n]*([0-9a-f]{7,40})[^\S\n]*$/gim;

function signedOffBy(commit: Commit): boolean {
  const author = commit.authorEmail.toLowerCase();
  for (const match of commit.message.matchAll(SIGN_OFF)) {
    if (match[1]?.toLowerCase() === author) return true;
  }
  return false;
}

/** The commits that neither carry their author's sign-off nor were remediated by that author. */
export function unsignedCommits(commits: readonly Commit[]): Commit[] {
  const remediated = new Map<string, string[]>();
  for (const commit of commits) {
    if (!signedOffBy(commit)) continue;
    const author = commit.authorEmail.toLowerCase();
    for (const match of commit.message.matchAll(REMEDIATION)) {
      const email = match[1]?.toLowerCase();
      const sha = match[2]?.toLowerCase();
      if (email !== author || sha === undefined) continue;
      remediated.set(author, [...(remediated.get(author) ?? []), sha]);
    }
  }
  return commits.filter((commit) => {
    if (signedOffBy(commit)) return false;
    const prefixes = remediated.get(commit.authorEmail.toLowerCase()) ?? [];
    return !prefixes.some((prefix) => commit.sha.toLowerCase().startsWith(prefix));
  });
}

/** Non-merge commits reachable from `head` and not from `base`, oldest first. */
export function readCommits(base: string, head: string, cwd?: string): Commit[] {
  const out = execFileSync(
    "git",
    ["log", "--reverse", "--no-merges", "--format=%H%x00%an%x00%ae%x00%B%x1e", `${base}..${head}`],
    { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  return out
    .split("\x1e")
    .map((record) => record.replace(/^\n/, ""))
    .filter((record) => record.length > 0)
    .map((record) => {
      const [sha = "", authorName = "", authorEmail = "", message = ""] = record.split("\0");
      return { sha, authorName, authorEmail, message };
    });
}

/** The failure report: which commits, and both ways to fix them. */
export function report(unsigned: readonly Commit[], base: string): string {
  const lines = [
    `DCO: ${unsigned.length} commit(s) are not signed off by their author (see CONTRIBUTING.md):`,
  ];
  for (const commit of unsigned) {
    const subject = commit.message.split("\n", 1)[0] ?? "";
    lines.push(
      `  ${commit.sha.slice(0, 12)}  ${commit.authorName} <${commit.authorEmail}>  ${subject}`,
    );
  }
  lines.push(
    "",
    "Fix by rewriting the branch:",
    `  git rebase --signoff ${base.slice(0, 12)} && git push --force-with-lease`,
    "",
    "or, without rewriting history, push one remediation commit per author:",
  );
  const byAuthor = new Map<string, Commit[]>();
  for (const commit of unsigned) {
    const key = `${commit.authorName} <${commit.authorEmail}>`;
    byAuthor.set(key, [...(byAuthor.get(key) ?? []), commit]);
  }
  for (const [author, commits] of byAuthor) {
    const certs = commits
      .map((c) => ` -m "I, ${author}, hereby add my Signed-off-by to this commit: ${c.sha}"`)
      .join("");
    lines.push(`  git commit --allow-empty -s -m "DCO remediation"${certs}`);
  }
  return lines.join("\n");
}

if (import.meta.main) {
  const [base, head] = process.argv.slice(2);
  if (base === undefined || head === undefined) {
    console.error("usage: bun run scripts/check-dco.ts <base> <head>");
    process.exit(2);
  }
  try {
    const commits = readCommits(base, head);
    const unsigned = unsignedCommits(commits);
    if (unsigned.length > 0) {
      console.error(report(unsigned, base));
      if (process.env.GITHUB_ACTIONS === "true") {
        console.error(
          `::error title=DCO::${unsigned.length} commit(s) lack a Signed-off-by from their author`,
        );
      }
      process.exit(1);
    }
    console.log(`DCO passed: ${commits.length} commit(s) signed off by their authors.`);
  } catch (error) {
    console.error("DCO check ERROR:", error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

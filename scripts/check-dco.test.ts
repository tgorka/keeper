import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { readCommits, report, unsignedCommits } from "./check-dco";

interface TestRepo {
  dir: string;
  base: string;
  commit: (message: string[], opts?: { signOff?: boolean; author?: string }) => string;
  cleanup: () => void;
}

/** A throwaway repository whose commits are made by real `git commit`, sign-off and all. */
function makeRepo(): TestRepo {
  const dir = mkdtempSync(path.join(tmpdir(), "check-dco-"));
  const git = (args: string[], author = "Ada Lovelace <ada@example.org>") => {
    const [, name = "", email = ""] = /^(.*) <(.*)>$/.exec(author) ?? [];
    return execFileSync(
      "git",
      ["-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", ...args],
      {
        cwd: dir,
        encoding: "utf8",
        env: {
          ...process.env,
          GIT_AUTHOR_NAME: name,
          GIT_AUTHOR_EMAIL: email,
          GIT_COMMITTER_NAME: name,
          GIT_COMMITTER_EMAIL: email,
        },
      },
    ).trim();
  };
  git(["init", "-q", "-b", "main"]);
  git(["commit", "-q", "--allow-empty", "-m", "base"]);
  const base = git(["rev-parse", "HEAD"]);
  const commit = (message: string[], opts: { signOff?: boolean; author?: string } = {}) => {
    git(
      [
        "commit",
        "-q",
        "--allow-empty",
        ...(opts.signOff ? ["-s"] : []),
        ...message.flatMap((m) => ["-m", m]),
      ],
      opts.author,
    );
    return git(["rev-parse", "HEAD"]);
  };
  return { dir, base, commit, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

describe("check-dco on a real repository", () => {
  let repo: TestRepo;
  beforeEach(() => {
    repo = makeRepo();
  });
  afterEach(() => repo.cleanup());

  it("passes commits made with `git commit -s`", () => {
    repo.commit(["feat: one"], { signOff: true });
    repo.commit(["fix: two"], { signOff: true });
    const commits = readCommits(repo.base, "HEAD", repo.dir);
    expect(commits).toHaveLength(2);
    expect(unsignedCommits(commits)).toEqual([]);
  });

  it("flags only the commit that forgot the sign-off", () => {
    repo.commit(["feat: signed"], { signOff: true });
    const forgot = repo.commit(["fix: forgot"]);
    const unsigned = unsignedCommits(readCommits(repo.base, "HEAD", repo.dir));
    expect(unsigned.map((c) => c.sha)).toEqual([forgot]);
  });

  it("rejects a sign-off by someone other than the author", () => {
    const sha = repo.commit(["feat: x", "Signed-off-by: Someone Else <else@example.org>"]);
    expect(unsignedCommits(readCommits(repo.base, "HEAD", repo.dir)).map((c) => c.sha)).toEqual([
      sha,
    ]);
  });

  it("accepts the author's sign-off whatever the email's case", () => {
    repo.commit(["feat: x", "Signed-off-by: Ada Lovelace <ADA@Example.org>"]);
    expect(unsignedCommits(readCommits(repo.base, "HEAD", repo.dir))).toEqual([]);
  });

  it("accepts a remediation commit by the same author, which the failure report prints", () => {
    repo.commit(["fix: forgot"]);
    const before = readCommits(repo.base, "HEAD", repo.dir);
    const unsigned = unsignedCommits(before);
    const command = report(unsigned, repo.base);
    const lines = [...command.matchAll(/-m "(I, [^"]+)"/g)].map((m) => m[1] ?? "");
    expect(lines).toHaveLength(1);
    repo.commit(["DCO remediation", ...lines], { signOff: true });
    expect(unsignedCommits(readCommits(repo.base, "HEAD", repo.dir))).toEqual([]);
  });

  it("does not let one author remediate another author's commit", () => {
    const sha = repo.commit(["fix: by grace"], { author: "Grace Hopper <grace@example.org>" });
    repo.commit(
      [
        "DCO remediation",
        `I, Grace Hopper <grace@example.org>, hereby add my Signed-off-by to this commit: ${sha}`,
      ],
      { signOff: true },
    );
    expect(unsignedCommits(readCommits(repo.base, "HEAD", repo.dir)).map((c) => c.sha)).toEqual([
      sha,
    ]);
  });

  it("ignores a remediation commit that is not itself signed off", () => {
    const sha = repo.commit(["fix: forgot"]);
    repo.commit([
      "DCO remediation",
      `I, Ada Lovelace <ada@example.org>, hereby add my Signed-off-by to this commit: ${sha}`,
    ]);
    expect(unsignedCommits(readCommits(repo.base, "HEAD", repo.dir))).toHaveLength(2);
  });
});

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

// The stamp is exercised on real repositories laid out on disk, because the
// defect it guards against is a property of `git`, not of the script: asked
// in a directory that is not a repository, `git` walks up and answers for the
// nearest one. On hesperia that was the dotfiles checkout at `~/.git`, and a
// keeper build logged its head (the header of scripts/lib/build-sha.sh keeps
// the account). Every case here runs the library exactly as its callers do —
// sourced under `set -euo pipefail` — so a return path that failed the caller
// would fail the test.

const LIB = join(process.cwd(), "scripts/lib/build-sha.sh");
const STAMP = "src-tauri/crates/keeper/build-sha.txt";

/** `git` without the developer's own config: no signing, no hooks, no templates. */
const GIT_ENV: Record<string, string> = {
  ...(process.env as Record<string, string>),
  GIT_CONFIG_GLOBAL: "/dev/null",
  GIT_CONFIG_SYSTEM: "/dev/null",
  GIT_CONFIG_NOSYSTEM: "1",
  GIT_AUTHOR_NAME: "keeper test",
  GIT_AUTHOR_EMAIL: "test@example.invalid",
  GIT_COMMITTER_NAME: "keeper test",
  GIT_COMMITTER_EMAIL: "test@example.invalid",
};

const hasGit = spawnSync("git", ["--version"]).status === 0;

let root: string;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "keeper-build-sha-"));
});
afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

function git(cwd: string, ...args: string[]): string {
  const r = spawnSync("git", args, { cwd, encoding: "utf8", env: GIT_ENV });
  expect(r.status, `git ${args.join(" ")}: ${r.stderr}`).toBe(0);
  return r.stdout.trim();
}

/** A repository with one commit, laid out so the stamp has somewhere to go. */
function repo(name: string): string {
  const dir = join(root, name);
  mkdirSync(join(dir, "src-tauri/crates/keeper"), { recursive: true });
  git(dir, "init", "-q", "-b", "main");
  writeFileSync(join(dir, "tracked.txt"), "one\n");
  writeFileSync(join(dir, "src-tauri/crates/keeper/Cargo.toml"), "[package]\n");
  git(dir, "add", ".");
  git(dir, "commit", "-q", "-m", "one");
  return dir;
}

/** The library as its callers run it: sourced under `set -euo pipefail`. */
function stamp(dir: string) {
  const r = spawnSync(
    "bash",
    ["-c", 'set -euo pipefail; . "$1"; keeper_stamp_build_sha "$2"', "_", LIB, dir],
    { encoding: "utf8", env: GIT_ENV },
  );
  return { status: r.status, out: r.stdout, err: r.stderr };
}

function stampFile(dir: string): string | undefined {
  const path = join(dir, STAMP);
  return existsSync(path) ? readFileSync(path, "utf8") : undefined;
}

describe.skipIf(!hasGit)("keeper_stamp_build_sha", () => {
  it("stamps a clean checkout with its 12-hex head, and prints it", () => {
    const dir = repo("clean");
    const head = git(dir, "rev-parse", "--short=12", "HEAD");
    const r = stamp(dir);
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
    expect(r.out).toBe(`${head}\n`);
    expect(head).toMatch(/^[0-9a-f]{12}$/);
    expect(stampFile(dir)).toBe(`${head}\n`);
  });

  it("marks a checkout with a modified tracked file -dirty", () => {
    const dir = repo("dirty");
    const head = git(dir, "rev-parse", "--short=12", "HEAD");
    writeFileSync(join(dir, "tracked.txt"), "two\n");
    const r = stamp(dir);
    expect(r.status).toBe(0);
    expect(r.out).toBe(`${head}-dirty\n`);
    expect(stampFile(dir)).toBe(`${head}-dirty\n`);
  });

  // The stated trade-off of `--untracked-files=no`: a file that was never
  // `git add`ed does not make the tree dirty. Asserted so that the trade-off
  // stays a decision and not an accident.
  it("reads a never-added file as clean, as the library's comment says it does", () => {
    const dir = repo("untracked");
    const head = git(dir, "rev-parse", "--short=12", "HEAD");
    writeFileSync(join(dir, "scratch.txt"), "never added\n");
    expect(stamp(dir).out).toBe(`${head}\n`);
  });

  it("removes a stale stamp and prints nothing where there is no .git", () => {
    const dir = join(root, "no-git");
    mkdirSync(join(dir, "src-tauri/crates/keeper"), { recursive: true });
    writeFileSync(join(dir, STAMP), "0f96b59d1ba5\n");
    const r = stamp(dir);
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
    expect(r.out).toBe("");
    expect(stampFile(dir)).toBeUndefined();
  });

  // The hesperia shape: a `.git`-less tree inside somebody else's repository.
  // `git` itself would answer for the outer one — the first assertion proves
  // the trap is real on this host — and the library must not pass that on.
  it("inherits nothing from a repository the tree merely sits inside", () => {
    const outer = repo("outer");
    const inner = join(outer, "inner");
    mkdirSync(join(inner, "src-tauri/crates/keeper"), { recursive: true });
    writeFileSync(join(inner, STAMP), "0f96b59d1ba5\n");
    expect(git(inner, "rev-parse", "--short=12", "HEAD")).toBe(
      git(outer, "rev-parse", "--short=12", "HEAD"),
    );
    const r = stamp(inner);
    expect(r.status).toBe(0);
    expect(r.out).toBe("");
    expect(stampFile(inner)).toBeUndefined();
  });

  // In a worktree `.git` is a FILE naming the real repository, which is why
  // the library tests `-e` and not `-d`.
  it("stamps a git worktree, where .git is a file", () => {
    const main = repo("main-for-worktree");
    const wt = join(root, "worktree");
    git(main, "worktree", "add", "-q", "-b", "branch", wt);
    git(wt, "commit", "-q", "--allow-empty", "-m", "two");
    const head = git(wt, "rev-parse", "--short=12", "HEAD");
    expect(head).not.toBe(git(main, "rev-parse", "--short=12", "HEAD"));
    const r = stamp(wt);
    expect(r.status).toBe(0);
    expect(r.out).toBe(`${head}\n`);
    expect(stampFile(wt)).toBe(`${head}\n`);
  });

  it("never fails the caller when the stamp cannot be written, and says so once", () => {
    const dir = repo("unwritable");
    rmSync(join(dir, "src-tauri"), { recursive: true, force: true });
    const r = stamp(dir);
    expect(r.status).toBe(0);
    expect(r.out).toBe("");
    expect(r.err.trim().split("\n")).toHaveLength(1);
    expect(r.err).toContain("could not write");
  });
});

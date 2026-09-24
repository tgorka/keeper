import { describe, expect, it } from "vitest";
import {
  BATCH_NAME_EMPTY,
  batchConflicts,
  batchFolderTakenSentence,
  batchNameTakenSentence,
  DEFAULT_REPO_FILTERS,
  filterRepos,
  groupRepos,
} from "@/lib/forge-repos";
import type { ForgeOwnerVm, ForgeRepoVm } from "@/lib/ipc/client";

function repo(fullName: string, patch: Partial<ForgeRepoVm> = {}): ForgeRepoVm {
  const [owner, name] = fullName.split("/");
  return {
    fullName,
    owner,
    name,
    description: null,
    private: false,
    fork: false,
    archived: false,
    template: false,
    mirror: false,
    defaultBranch: "main",
    cloneUrl: `https://github.com/${fullName}.git`,
    webUrl: `https://github.com/${fullName}`,
    updatedMs: null,
    sizeKb: null,
    canPush: true,
    addedAs: [],
    elsewhere: [],
    pullOnly: false,
    pullOnlySentence: null,
    ...patch,
  };
}

const names = (repos: readonly ForgeRepoVm[]) => repos.map((each) => each.fullName);

describe("filterRepos", () => {
  const repos = [
    repo("tgorka/keeper", { description: "Matrix client that keeps folders in sync" }),
    repo("tgorka/tauri", { fork: true }),
    repo("tgorka/old-blog", { archived: true }),
    repo("makistack/handbook", { description: "How we work" }),
  ];

  it("hides forks and archived repositories until their toggle is on", () => {
    expect(names(filterRepos(repos, DEFAULT_REPO_FILTERS))).toEqual([
      "tgorka/keeper",
      "makistack/handbook",
    ]);
    expect(names(filterRepos(repos, { ...DEFAULT_REPO_FILTERS, forks: true }))).toContain(
      "tgorka/tauri",
    );
    expect(names(filterRepos(repos, { ...DEFAULT_REPO_FILTERS, archived: true }))).toContain(
      "tgorka/old-blog",
    );
  });

  it("searches the name, the owner and the description, every word, ignoring case", () => {
    const search = (query: string) => names(filterRepos(repos, { ...DEFAULT_REPO_FILTERS, query }));
    expect(search("HANDBOOK")).toEqual(["makistack/handbook"]);
    expect(search("makistack")).toEqual(["makistack/handbook"]);
    expect(search("folders")).toEqual(["tgorka/keeper"]);
    // Every word narrows: both are in keeper's row, only one is in handbook's.
    expect(search("matrix sync")).toEqual(["tgorka/keeper"]);
    expect(search("matrix work")).toEqual([]);
  });

  it("shows one owner's repositories under the owner filter", () => {
    expect(names(filterRepos(repos, { ...DEFAULT_REPO_FILTERS, owner: "makistack" }))).toEqual([
      "makistack/handbook",
    ]);
  });
});

describe("groupRepos", () => {
  const owners: ForgeOwnerVm[] = [
    { login: "tgorka", isYou: true, count: 2 },
    { login: "hesperia-labs", isYou: false, count: 1 },
    { login: "makistack", isYou: false, count: 1 },
  ];

  it("keeps Rust's owner order, drops empty owners and sorts inside each group", () => {
    const visible = [
      repo("tgorka/b-old", { updatedMs: 1 }),
      repo("makistack/infra", { updatedMs: 5 }),
      repo("tgorka/a-new", { updatedMs: 9 }),
      repo("tgorka/c-undated"),
    ];
    const byUpdate = groupRepos(visible, owners, "updated");
    expect(byUpdate.map((group) => [group.login, group.isYou])).toEqual([
      ["tgorka", true],
      ["makistack", false],
    ]);
    // Newest first; a repository with no date goes last, not first.
    expect(names(byUpdate[0].repos)).toEqual(["tgorka/a-new", "tgorka/b-old", "tgorka/c-undated"]);
    expect(names(groupRepos(visible, owners, "name")[0].repos)).toEqual([
      "tgorka/a-new",
      "tgorka/b-old",
      "tgorka/c-undated",
    ]);
  });
});

describe("batchConflicts", () => {
  const drives = [{ name: "notes", localPath: "/Users/t/Drives/notes/" }];

  it("blocks only the second of two rows with one name, whatever the case", () => {
    const conflicts = batchConflicts(
      [
        { fullName: "tgorka/notes-a", driveName: "Journal" },
        { fullName: "makistack/journal", driveName: " journal " },
        { fullName: "tgorka/keeper", driveName: "keeper" },
      ],
      "/Users/t/Drives",
      [],
    );
    expect([...conflicts]).toEqual([["makistack/journal", batchNameTakenSentence("journal")]]);
  });

  it("blocks a row whose folder another drive already has, and an empty name", () => {
    const conflicts = batchConflicts(
      [
        { fullName: "tgorka/notes", driveName: "notes" },
        { fullName: "tgorka/keeper", driveName: "  " },
        { fullName: "tgorka/other", driveName: "other" },
      ],
      "/Users/t/Drives/",
      drives,
    );
    expect(conflicts.get("tgorka/notes")).toBe(
      batchFolderTakenSentence("/Users/t/Drives/notes", "notes"),
    );
    expect(conflicts.get("tgorka/keeper")).toBe(BATCH_NAME_EMPTY);
    expect(conflicts.has("tgorka/other")).toBe(false);
  });

  it("does not compare folders where Rust picks them (no base folder)", () => {
    expect(
      batchConflicts([{ fullName: "tgorka/notes", driveName: "notes" }], null, drives).size,
    ).toBe(0);
  });
});

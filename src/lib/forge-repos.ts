/**
 * The Browse repositories sheet's list logic (Epic 86, UX-DR120), pure.
 *
 * Rust fetches, marks and orders the repositories (AD-335); what is left for
 * the webview is the person's LENS on that list — what they searched for,
 * which owner they are looking at, whether forks and archived repositories
 * show — and the batch step's preview, which has to answer "can this row be
 * added?" before anything is sent. Both are here, apart from the sheet, so a
 * test can hold them to the list the person reads rather than to markup.
 */
import type { ForgeOwnerVm, ForgeRepoVm } from "@/lib/ipc/client";

export type RepoSort = "updated" | "name";

/** What the toolbar holds. `owner: null` is the All view. */
export interface RepoFilters {
  query: string;
  owner: string | null;
  forks: boolean;
  archived: boolean;
  sort: RepoSort;
}

/** Forks and archived repositories are off by default: most people add neither. */
export const DEFAULT_REPO_FILTERS: RepoFilters = {
  query: "",
  owner: null,
  forks: false,
  archived: false,
  sort: "updated",
};

/**
 * The repositories the toolbar lets through, in Rust's order.
 *
 * Search is over the name, the owner and the description, case-insensitive,
 * and every word typed has to be found somewhere in them — so `keeper notes`
 * narrows rather than widens. It looks only through what was fetched: past
 * the cap, Rust's truncated notice says so.
 */
export function filterRepos(repos: readonly ForgeRepoVm[], filters: RepoFilters): ForgeRepoVm[] {
  const terms = filters.query.toLowerCase().split(/\s+/).filter(Boolean);
  return repos.filter((repo) => {
    if ((repo.fork && !filters.forks) || (repo.archived && !filters.archived)) {
      return false;
    }
    if (filters.owner !== null && repo.owner !== filters.owner) {
      return false;
    }
    const haystack = `${repo.name}\n${repo.owner}\n${repo.description ?? ""}`.toLowerCase();
    return terms.every((term) => haystack.includes(term));
  });
}

/** One owner's rows, under a header that counts what is visible of them. */
export interface RepoGroup {
  login: string;
  isYou: boolean;
  repos: ForgeRepoVm[];
}

/**
 * The visible repositories under their owners, in Rust's owner order ("you"
 * first, then the others A→Z) and sorted inside each group. An owner with
 * nothing visible has no group: a header over nothing is noise.
 */
export function groupRepos(
  visible: readonly ForgeRepoVm[],
  owners: readonly ForgeOwnerVm[],
  sort: RepoSort,
): RepoGroup[] {
  const byOwner = new Map<string, ForgeRepoVm[]>();
  for (const repo of visible) {
    const rows = byOwner.get(repo.owner);
    if (rows === undefined) {
      byOwner.set(repo.owner, [repo]);
    } else {
      rows.push(repo);
    }
  }
  const byName = (a: ForgeRepoVm, b: ForgeRepoVm) => a.name.localeCompare(b.name);
  // Recently updated first; a repository whose date Rust could not read goes
  // last rather than pretending to be the oldest or the newest.
  const byUpdated = (a: ForgeRepoVm, b: ForgeRepoVm) =>
    (b.updatedMs ?? Number.NEGATIVE_INFINITY) - (a.updatedMs ?? Number.NEGATIVE_INFINITY) ||
    byName(a, b);
  // An owner Rust's list does not name still gets its rows shown, after the
  // named ones, rather than losing them.
  const known = new Set(owners.map((owner) => owner.login));
  const order = [
    ...owners.map((owner) => ({ login: owner.login, isYou: owner.isYou })),
    ...[...byOwner.keys()]
      .filter((login) => !known.has(login))
      .sort((a, b) => a.localeCompare(b))
      .map((login) => ({ login, isYou: false })),
  ];
  return order.flatMap(({ login, isYou }) => {
    const rows = byOwner.get(login);
    if (rows === undefined) {
      return [];
    }
    return [{ login, isYou, repos: rows.sort(sort === "name" ? byName : byUpdated) }];
  });
}

/**
 * The repositories a batch would add: what the person ticked, still in the
 * list Rust last answered, and not already synced here. A tick outlives the
 * list it was made on — a refresh can drop the repository, and a drive added
 * meanwhile (a restore, the other surface) marks it `addedAs` — so the count
 * on the footer and the rows of the batch step are both read from this, never
 * from the raw ticks, and "Add 5 drives…" always opens a batch of five.
 */
export function pickedRepos(
  repos: readonly ForgeRepoVm[],
  selected: readonly string[],
): ForgeRepoVm[] {
  const ticked = new Set(selected);
  return repos.filter((repo) => ticked.has(repo.fullName) && repo.addedAs.length === 0);
}

/**
 * Whether a drive with `remoteUrl` can sign with a source whose host is
 * `host` (`ForgeSourceVm.host`, which Rust gives without a port) — the rule
 * `drive_credential` holds a `forge:<id>` drive to: an https remote on that
 * source's own site, so the source's token never goes anywhere else. Anything
 * else (another host, ssh, `git@host:…`, text that is not a URL yet) is not on
 * the source. Rust compares the full origin again when the choice is saved.
 */
export function remoteOnSourceHost(remoteUrl: string, host: string): boolean {
  try {
    const remote = new URL(remoteUrl.trim());
    return remote.protocol === "https:" && remote.hostname.toLowerCase() === host.toLowerCase();
  } catch {
    return false;
  }
}

/** Where a batch row's drive would live: `<base>/<name>`, or nowhere to say (iOS). */
export function batchFolder(base: string | null, driveName: string): string | null {
  return base === null ? null : `${base.replace(/\/+$/, "")}/${driveName.trim()}`;
}

/** One row of the batch step, as the person has named it. */
export interface BatchEntry {
  fullName: string;
  driveName: string;
}

export const BATCH_NAME_EMPTY = "Give this drive a name.";
export const BATCH_NAME_INVALID = "A folder name can't contain a slash or be . or ..";

export function batchNameTakenSentence(name: string): string {
  return `Another drive in this list is also called ${name}.`;
}

export function batchFolderTakenSentence(path: string, drive: string): string {
  return `${path} is already the folder of ${drive}.`;
}

/**
 * The rows the batch step cannot send, each with the sentence it shows.
 *
 * Only the offending row is blocked: of two rows with one name, the SECOND is,
 * so renaming either one clears it and the first can go as it is. Names are
 * compared without case, because the folders they become are on a filesystem
 * that ignores it (macOS by default). A folder that already holds another
 * drive is known here from the drive list; one that holds other files is only
 * known once Rust looks, and comes back as that row's result sentence.
 */
export function batchConflicts(
  entries: readonly BatchEntry[],
  base: string | null,
  drives: readonly { name: string; localPath: string }[],
): Map<string, string> {
  const conflicts = new Map<string, string>();
  const seen = new Set<string>();
  const folders = new Map(drives.map((drive) => [drive.localPath.replace(/\/+$/, ""), drive.name]));
  for (const entry of entries) {
    const name = entry.driveName.trim();
    if (name === "") {
      conflicts.set(entry.fullName, BATCH_NAME_EMPTY);
      continue;
    }
    if (/[/\\]/.test(name) || name === "." || name === "..") {
      conflicts.set(entry.fullName, BATCH_NAME_INVALID);
      continue;
    }
    const key = name.toLowerCase();
    if (seen.has(key)) {
      conflicts.set(entry.fullName, batchNameTakenSentence(name));
      continue;
    }
    seen.add(key);
    const folder = batchFolder(base, name);
    const holder = folder === null ? undefined : folders.get(folder);
    if (folder !== null && holder !== undefined) {
      conflicts.set(entry.fullName, batchFolderTakenSentence(folder, holder));
    }
  }
  return conflicts;
}

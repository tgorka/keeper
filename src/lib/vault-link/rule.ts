/**
 * Whether a synced file is a note, and where a note's file sits in its profile
 * (Story 45.18, FR-196, AD-65, UX-DR79).
 *
 * **This is a mirror, not a decision.** The rule is authored in
 * `keeper_core::vault_link` and the two are pinned to each other by
 * `src-tauri/crates/keeper-core/src/vault-link-vectors.json`, which both test
 * suites load — the treatment `keeper_core::size`, `keeper_core::file_asset`
 * and `keeper_core::notes::attach` already have. A rule that has to run in two
 * languages drifts unless one table fails the commit that separates them.
 *
 * **Why it runs here at all, given AD-65.** These functions decide whether an
 * action EXISTS, not what it does: the Files surface offers "Open in Notes"
 * only for a file that has a note, and a markdown file outside every vault must
 * offer nothing rather than an action that fails. An IPC round trip per file
 * would make every such affordance appear a frame late and would make a CSV
 * panel flash "not in a notes vault" before replacing it with a table.
 *
 * What AD-65 forbids is a frontend joining a **root** and a subpath. Nothing
 * here has a root. Both inputs and both outputs are relative paths, no absolute
 * path is formed or read (FR-145), and every real read still goes through
 * `keeper_sync::browse`, which re-resolves and re-contains whatever this
 * produced (AD-59). The refusals below are the outer of two gates.
 *
 * It resolves no note **id**: turning a vault-relative path into a note is a
 * lookup in the index, which is `notes_tree`'s job and stays there.
 */

/**
 * One vault as this rule needs to see it.
 *
 * Field names are `NoteVaultVm`'s, so a vault straight out of the mirror store
 * is assignable with no adapter — and deliberately narrower than that VM, which
 * also carries `root`, an absolute path. A function that accepted `root` would
 * be one edit away from composing with it.
 */
export interface VaultLocation {
  /** The notes vault id, which every notes command is addressed by. */
  readonly id: string;
  /** The sync profile the vault is a flag on. */
  readonly profileId: string;
  /** Where the vault sits inside the profile, exactly as stored. */
  readonly subfolder: string;
}

/** Where a profile-relative file lives inside a notes vault. */
export interface VaultFilePath {
  /** The vault that holds it. */
  readonly vaultId: string;
  /** The file's path relative to the vault root, `/`-joined. */
  readonly vaultPath: string;
  /** The directory holding it, vault-relative; empty for the vault root. What
   *  `notes_tree` takes, carried rather than re-derived by every caller. */
  readonly vaultDir: string;
}

/** Where a vault-relative note lives inside its sync profile. */
export interface ProfileFilePath {
  /** The sync profile that holds it. */
  readonly profileId: string;
  /** The path relative to the profile root — the exact shape `FilesEntryVm`
   *  carries, so it can become a `file` panel target unchanged. */
  readonly relativePath: string;
}

/**
 * ASCII-only lowercase, matching Rust's `str::to_ascii_lowercase`.
 *
 * Not `String.prototype.toLowerCase`, which is Unicode-aware: it maps `İ`
 * (U+0130) to `i̇` and `K` (U+212A) to `k`, while Rust's ASCII form leaves both
 * alone. A vault called `KELVIN` and a folder called `KELVIN` must be the same
 * folder in both languages or the shared vector table is the only place the
 * disagreement is visible — which is exactly what it is for.
 */
function asciiLower(value: string): string {
  return value.replace(/[A-Z]/g, (character) => String.fromCharCode(character.charCodeAt(0) + 32));
}

/**
 * Split a **configured subfolder** into its lowercased components.
 *
 * Both separators, because this string is whatever the user typed into the
 * settings form — `Notes/`, `\Notes`, `notes//daily` all reach the stored
 * profile intact, since `NotesConfig::validate` refuses rather than corrects.
 * Case-insensitive because the stored spelling and the dirent's spelling differ
 * in case for the same folder as a matter of course on APFS and HFS+.
 *
 * Empty means "no vault here": the profile root is not a vault, and an
 * unflagged profile is projected with an empty subfolder, so treating empty as
 * a match at depth zero would make every file in every synced folder a note.
 */
function subfolderComponents(subfolder: string): string[] {
  return subfolder
    .split(/[/\\]/)
    .filter((part) => part !== "")
    .map(asciiLower);
}

/**
 * Split a **path that names a real file** into its components, or `null` when
 * it is not a plain relative descendant.
 *
 * Split on `/` only, and that asymmetry with {@link subfolderComponents} is
 * deliberate. A subfolder is configuration a human typed on some platform; a
 * path here came from a dirent or the note index and is `/`-joined on every
 * platform by contract. A backslash inside it is a *character in a file name*,
 * which is legal on Linux, so splitting on it would name a file that does not
 * exist.
 *
 * Refused, in the shape `panels.ts`'s `isRestorableTarget` refuses them and for
 * the same reason — a path from outside the app must be proven relative before
 * it is used as one: absolute in all four spellings (leading `/`, leading `\`,
 * a UNC `\\`, a Windows drive letter), any `.` or `..` component, and empty,
 * which names a directory rather than a file.
 */
function fileComponents(path: string): string[] | null {
  if (path.startsWith("/") || path.startsWith("\\") || /^[a-zA-Z]:[\\/]/.test(path)) {
    return null;
  }
  const parts = path.split("/").filter((part) => part !== "");
  if (parts.length === 0 || parts.some((part) => part === "." || part === "..")) {
    return null;
  }
  return parts;
}

/**
 * Which vault holds this profile-relative file, and what it is called there.
 *
 * `null` is the honest and common answer: a synced folder that is not a vault,
 * a file beside the vault rather than inside it, or the vault directory itself.
 * Story 45.18 turns that `null` into an **absent** action rather than a present
 * one that fails.
 *
 * **The longest matching subfolder wins.** A profile carrying vaults at `notes`
 * and `notes/journal` is unusual but expressible, and a first-match rule would
 * answer `journal/x.md` in the outer vault — where its note id does not exist,
 * so the surface would say a file it is showing has no note. Most specific is
 * the only answer right for both configurations, and it does not depend on the
 * order the vault list happens to arrive in.
 *
 * A vault whose `profileId` differs is skipped before any path work: a path is
 * only relative to the profile that produced it.
 */
export function notePathForFile(
  vaults: readonly VaultLocation[],
  profileId: string,
  relativePath: string,
): VaultFilePath | null {
  if (profileId === "") {
    return null;
  }
  const parts = fileComponents(relativePath);
  if (parts === null) {
    return null;
  }
  let best: VaultFilePath | null = null;
  let bestDepth = 0;
  for (const vault of vaults) {
    if (vault.id === "" || vault.profileId !== profileId) {
      continue;
    }
    const prefix = subfolderComponents(vault.subfolder);
    // `>=` and not `>`: a path exactly as long as the subfolder IS the vault
    // directory, which is a folder and never a note.
    if (prefix.length === 0 || prefix.length >= parts.length || prefix.length <= bestDepth) {
      continue;
    }
    if (!prefix.every((configured, at) => configured === asciiLower(parts[at] ?? ""))) {
      continue;
    }
    // The remainder in the case the filesystem actually reported. Only the
    // COMPARISON ignores case; lowercasing the answer would hand `notes_tree` a
    // path that does not exist on a case-sensitive volume.
    const rest = parts.slice(prefix.length);
    bestDepth = prefix.length;
    best = {
      vaultId: vault.id,
      vaultPath: rest.join("/"),
      vaultDir: rest.slice(0, -1).join("/"),
    };
  }
  return best;
}

/**
 * Where this vault-relative note sits inside its sync profile.
 *
 * The inverse of {@link notePathForFile}, and the direction that lets "from a
 * note, open its file" exist without the note editor knowing what a profile
 * root is. `null` when the vault holds no subfolder — which is what a
 * `NoteVaultVm` projected from an unflagged profile carries — or when the note
 * path is not a plain relative descendant.
 *
 * **The stored subfolder's own case is preserved**, matching every other
 * composition of it in the repo (`notes_vault.rs` writes
 * `format!("{subfolder}/{rel}")` into its `git` arguments). Case is ignored in
 * the comparison direction; this direction has one spelling available and uses
 * it.
 */
export function filePathForNote(vault: VaultLocation, notePath: string): ProfileFilePath | null {
  if (vault.profileId === "") {
    return null;
  }
  const prefix = vault.subfolder.split(/[/\\]/).filter((part) => part !== "");
  if (prefix.length === 0) {
    return null;
  }
  const parts = fileComponents(notePath);
  if (parts === null) {
    return null;
  }
  return {
    profileId: vault.profileId,
    relativePath: `${prefix.join("/")}/${parts.join("/")}`,
  };
}

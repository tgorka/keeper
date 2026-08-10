//! The one rule that says whether a synced file is a note, and where a note's
//! file sits in its profile (Story 45.18, FR-196, AD-65, AD-90, UX-DR79).
//!
//! # The question, and why it has to be answered here
//!
//! Two identifiers name overlapping bytes. A Files panel holds a **sync profile
//! id** plus a profile-relative path; every notes command holds a **notes vault
//! id** plus a vault-relative path. A vault is a synced folder plus a flag
//! (FR-94), so the two coordinate systems differ by exactly one thing — the
//! vault's `subfolder` inside the profile — and converting between them is
//! stripping or restoring that prefix.
//!
//! Doing that in the webview is the path arithmetic AD-65 forbids: it would be
//! the frontend deciding which folders are vaults, which is the decision that
//! has to have one owner or two surfaces will disagree about whether
//! `Notes/daily/x.md` is a note. Story 45.4 hit this and deliberately declined
//! to guess, shipping a CSV panel that said so instead of drawing a table.
//! This module is the answer it was waiting for.
//!
//! # Why a mirror in TypeScript rather than a command
//!
//! The consumers need this **synchronously**, because they use it to decide
//! whether an action exists at all — the Files pane offers "Open in Notes" only
//! for a file that has a note, and a markdown file outside every vault must
//! offer nothing rather than an action that fails. An IPC round trip per file
//! would make the affordance appear a frame late, and the panel that resolves
//! a CSV's coordinates would flash its "not in a vault" sentence before
//! replacing it with a table.
//!
//! So the rule is authored here, mirrored in `src/lib/vault-link/rule.ts`, and
//! the two are pinned to each other by `vault-link-vectors.json`, which both
//! test suites load. That is the same treatment `keeper_core::size`,
//! `keeper_core::file_asset` and `keeper_core::notes::attach` already have, and
//! for the same reason: a rule that must run in both languages is drift waiting
//! to happen unless one table fails the commit that introduces it.
//!
//! # What it does NOT do
//!
//! It touches no disk, resolves no note **id**, and never composes an absolute
//! path. `vault_path` is a path *inside* a vault and `relative_path` is a path
//! *inside* a profile; neither is joined to a root here or anywhere in the
//! webview (AD-65, FR-145). Turning a vault path into a note id is a lookup in
//! the note index, which is `notes_tree`'s job, and it stays there.
//!
//! Containment is still Rust's on every real read: `keeper_sync::browse`
//! re-resolves and re-contains whatever path this produced (AD-59). The
//! refusals below are the outer of two gates, kept here so a `..` reaches a log
//! as a refusal with a name rather than as a path that already collapsed.

/// One vault as this rule needs to see it — a projection of
/// [`crate::notes::vm::NoteVaultVm`], borrowed so resolving one file against
/// twenty vaults allocates nothing.
///
/// Deliberately **not** `NoteVaultVm` itself. That VM carries `root`, an
/// absolute path, and a function that accepted it would be one edit away from
/// composing with it. The three fields here are the whole of the question, and
/// the absent fourth is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultLocation<'a> {
    /// The notes vault id, which every notes command is addressed by.
    pub vault_id: &'a str,
    /// The sync profile the vault is a flag on, which every files command is
    /// addressed by.
    pub profile_id: &'a str,
    /// Where the vault sits inside the profile, exactly as stored — whatever
    /// the user typed into the settings form. Normalised here and only here.
    pub subfolder: &'a str,
}

/// Where a profile-relative file lives inside a notes vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultFilePath {
    /// The vault that holds it.
    pub vault_id: String,
    /// The file's path relative to the vault root, `/`-joined.
    pub vault_path: String,
    /// The directory holding it, vault-relative; empty for the vault root.
    ///
    /// Carried rather than left to the caller to derive, because the caller
    /// that needs it is `notes_tree`'s, in TypeScript, and "take everything
    /// before the last slash" is a fifth spelling of a path operation this
    /// module already performed. One function, one set of tests.
    pub vault_dir: String,
}

/// Where a vault-relative note lives inside its sync profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFilePath {
    /// The sync profile that holds it.
    pub profile_id: String,
    /// The file's path relative to the profile root, `/`-joined — the exact
    /// shape [`crate::vm::FilesEntryVm::relative_path`] carries, so the result
    /// can be handed to a `file` panel target unchanged.
    pub relative_path: String,
}

/// Split a **configured subfolder** into its lowercased components.
///
/// Both separators, because this string is whatever the user typed into the
/// settings form — `Notes/`, `\Notes`, `notes//daily` all reach the stored
/// profile intact, since `NotesConfig::validate` refuses rather than corrects.
/// Lowercased because the stored spelling and the dirent's spelling differ in
/// case for the same folder as a matter of course on APFS and HFS+, and a
/// case-sensitive compare would silently decide that a user who typed a capital
/// has no notes — the same invisible failure
/// [`crate::vm::FilesFolderRoles::role_of`] documents and avoids.
///
/// Empty means "no vault here": the profile root is not a vault (`validate`
/// refuses an empty subfolder), and treating it as one would make every file in
/// every synced folder a note.
fn subfolder_components(subfolder: &str) -> Vec<String> {
    subfolder
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Split a **path that names a real file** into its components, or `None` when
/// it is not a plain relative descendant.
///
/// Split on `/` only, and that asymmetry with [`subfolder_components`] is
/// deliberate. A subfolder is configuration a human typed on some platform; a
/// path here came from a dirent or the note index and is `/`-joined on every
/// platform by contract. A backslash inside it is therefore a *character in a
/// file name*, which is legal on Linux, and splitting on it would resolve
/// `a\b.md` to a file that does not exist.
///
/// Refused, in the shape `panels.ts`'s `isRestorableTarget` refuses them and
/// for the same reason — a path that arrived from outside the app has to be a
/// relative one before it is used as one:
///
/// - absolute, in all four spellings: a leading `/`, a leading `\` (UNC), a
///   Windows drive letter, or a leading `\\`;
/// - any `.` or `..` component, so nothing climbs out of a profile or names a
///   vault by a route through its parent;
/// - empty, which names a directory rather than a file.
fn file_components(path: &str) -> Option<Vec<&str>> {
    if path.starts_with('/') || path.starts_with('\\') {
        return None;
    }
    let mut characters = path.chars();
    if let (Some(letter), Some(':'), Some(separator)) =
        (characters.next(), characters.next(), characters.next())
    {
        if letter.is_ascii_alphabetic() && (separator == '/' || separator == '\\') {
            return None;
        }
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|part| *part == "." || *part == "..") {
        return None;
    }
    Some(parts)
}

/// Which vault holds this profile-relative file, and what it is called there.
///
/// `None` is the honest and common answer: a synced folder that is not a vault,
/// a file beside the vault rather than inside it, or the vault directory
/// itself. Story 45.18 turns that `None` into an **absent** action rather than
/// a present one that fails.
///
/// **The longest matching subfolder wins.** A profile carrying vaults at
/// `notes` and `notes/journal` is unusual but expressible, and a first-match
/// rule would answer `journal/2026-01-01.md` in the outer vault — a note id
/// looked up there would not be found, and the surface would say the file has
/// no note while showing it. Most specific is the only answer that is right for
/// both configurations, and it does not depend on the order the vault list
/// happens to arrive in.
///
/// A vault whose `profile_id` does not match is skipped before any path work: a
/// path is only relative to the profile that produced it, and comparing it
/// against another profile's subfolder is how a file in one synced folder
/// resolves into another's vault.
pub fn note_path_for_file(
    vaults: &[VaultLocation<'_>],
    profile_id: &str,
    relative_path: &str,
) -> Option<VaultFilePath> {
    if profile_id.is_empty() {
        return None;
    }
    let parts = file_components(relative_path)?;
    let mut best: Option<VaultFilePath> = None;
    let mut best_depth = 0usize;
    for vault in vaults {
        if vault.vault_id.is_empty() || vault.profile_id != profile_id {
            continue;
        }
        let prefix = subfolder_components(vault.subfolder);
        if prefix.is_empty() || prefix.len() >= parts.len() {
            // `>=` and not `>`: a path exactly as long as the subfolder IS the
            // vault directory, which is a folder and never a note.
            continue;
        }
        let matched = prefix
            .iter()
            .zip(parts.iter())
            .all(|(configured, actual)| *configured == actual.to_ascii_lowercase());
        if !matched || prefix.len() <= best_depth {
            continue;
        }
        // The remainder, in the case the filesystem actually reported. Only the
        // COMPARISON is case-insensitive; lowercasing the answer would hand
        // `notes_tree` a path that does not exist on a case-sensitive volume.
        let rest = &parts[prefix.len()..];
        best_depth = prefix.len();
        best = Some(VaultFilePath {
            vault_id: vault.vault_id.to_owned(),
            vault_path: rest.join("/"),
            vault_dir: rest[..rest.len() - 1].join("/"),
        });
    }
    best
}

/// Where this vault-relative note sits inside its sync profile.
///
/// The inverse of [`note_path_for_file`], and the direction that makes "from a
/// note, open its file" possible without the note editor knowing what a profile
/// root is. `None` when the vault holds no subfolder — which is what a
/// `NoteVaultVm` projected from an unflagged profile carries — or when the note
/// path is not a plain relative descendant.
///
/// **The stored subfolder's own case is preserved**, matching every other
/// composition of it in this repo (`notes_vault.rs` writes
/// `format!("{subfolder}/{rel}")` into `git` arguments). The comparison
/// direction is where case is ignored; the composition direction has only one
/// spelling available and uses it.
pub fn file_path_for_note(vault: &VaultLocation<'_>, note_path: &str) -> Option<ProfileFilePath> {
    if vault.profile_id.is_empty() {
        return None;
    }
    let prefix: Vec<&str> = vault
        .subfolder
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if prefix.is_empty() {
        return None;
    }
    let parts = file_components(note_path)?;
    let mut relative_path = prefix.join("/");
    relative_path.push('/');
    relative_path.push_str(&parts.join("/"));
    Some(ProfileFilePath {
        profile_id: vault.profile_id.to_owned(),
        relative_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pinning table. `src/lib/vault-link/rule.test.ts` loads THIS file and
    /// runs its mirror over every row; the tests below run this implementation
    /// over the same rows. Two languages deciding "is this file a note" and
    /// never meeting at runtime is how a vault called `Notes` gets an action in
    /// one surface and not in the other.
    const VECTORS_JSON: &str = include_str!("vault-link-vectors.json");

    #[derive(serde::Deserialize)]
    struct VaultRow {
        vault_id: String,
        profile_id: String,
        subfolder: String,
    }

    #[derive(serde::Deserialize)]
    struct ToNote {
        profile_id: String,
        relative_path: String,
        #[serde(default)]
        vault_id: Option<String>,
        #[serde(default)]
        vault_path: Option<String>,
        #[serde(default)]
        vault_dir: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct ToFile {
        vault_id: String,
        note_path: String,
        #[serde(default)]
        relative_path: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        vaults: Vec<VaultRow>,
        to_note: Vec<ToNote>,
        to_file: Vec<ToFile>,
    }

    fn vectors() -> Vectors {
        serde_json::from_str(VECTORS_JSON).expect("the shared vector table parses")
    }

    fn locations(rows: &[VaultRow]) -> Vec<VaultLocation<'_>> {
        rows.iter()
            .map(|row| VaultLocation {
                vault_id: &row.vault_id,
                profile_id: &row.profile_id,
                subfolder: &row.subfolder,
            })
            .collect()
    }

    #[test]
    fn every_shared_vector_resolves_a_file_to_the_note_path_the_mirror_expects() {
        let table = vectors();
        assert!(
            table.vaults.len() >= 2 && table.to_note.len() >= 12,
            "a one-vault table cannot tell a per-profile filter from an unconditional \
             match, and a short one cannot tell a prefix rule from a substring one"
        );
        let vaults = locations(&table.vaults);
        for vector in &table.to_note {
            let actual = note_path_for_file(&vaults, &vector.profile_id, &vector.relative_path);
            let expected = vector.vault_id.as_ref().map(|vault_id| VaultFilePath {
                vault_id: vault_id.clone(),
                vault_path: vector
                    .vault_path
                    .clone()
                    .expect("a resolved vector names its vault path"),
                vault_dir: vector
                    .vault_dir
                    .clone()
                    .expect("a resolved vector names its vault dir"),
            });
            assert_eq!(
                actual, expected,
                "{}/{} resolved wrongly",
                vector.profile_id, vector.relative_path
            );
        }
    }

    #[test]
    fn every_shared_vector_resolves_a_note_to_the_profile_path_the_mirror_expects() {
        let table = vectors();
        assert!(
            table.to_file.len() >= 8,
            "the reverse table is load-bearing"
        );
        let vaults = locations(&table.vaults);
        for vector in &table.to_file {
            let vault = vaults
                .iter()
                .find(|candidate| candidate.vault_id == vector.vault_id)
                .expect("every to_file vector names a vault in the table");
            let actual = file_path_for_note(vault, &vector.note_path);
            let expected = vector
                .relative_path
                .as_ref()
                .map(|relative_path| ProfileFilePath {
                    profile_id: vault.profile_id.to_owned(),
                    relative_path: relative_path.clone(),
                });
            assert_eq!(
                actual, expected,
                "{}:{} resolved wrongly",
                vector.vault_id, vector.note_path
            );
        }
    }

    /// The two directions have to compose, or one of them is wrong in a way no
    /// single-direction table can see: a note opened in Files and then opened
    /// back in Notes must be the same note.
    #[test]
    fn a_file_resolved_to_a_note_resolves_back_to_the_same_file() {
        let table = vectors();
        let vaults = locations(&table.vaults);
        let mut checked = 0;
        for vector in table.to_note.iter().filter(|row| row.vault_id.is_some()) {
            let forward = note_path_for_file(&vaults, &vector.profile_id, &vector.relative_path)
                .expect("this vector resolves");
            let vault = vaults
                .iter()
                .find(|candidate| candidate.vault_id == forward.vault_id)
                .expect("the answer names a vault in the table");
            let back = file_path_for_note(vault, &forward.vault_path).expect("and back again");
            assert_eq!(back.profile_id, vector.profile_id);
            assert_eq!(
                back.relative_path.to_ascii_lowercase(),
                vector.relative_path.to_ascii_lowercase(),
                "{} did not round trip",
                vector.relative_path
            );
            checked += 1;
        }
        assert!(checked >= 6, "the round trip covered {checked} vectors");
    }

    /// A profile carrying two vaults, one inside the other. First-match would
    /// answer the outer one for a file in the inner, and the note id looked up
    /// there would not exist — the surface would say a file it is showing has
    /// no note.
    #[test]
    fn the_innermost_vault_holding_a_file_is_the_one_that_answers() {
        let outer = VaultLocation {
            vault_id: "v-outer",
            profile_id: "p1",
            subfolder: "notes",
        };
        let inner = VaultLocation {
            vault_id: "v-inner",
            profile_id: "p1",
            subfolder: "notes/journal",
        };
        for order in [[outer, inner], [inner, outer]] {
            let resolved =
                note_path_for_file(&order, "p1", "notes/journal/2026-01-01.md").expect("resolves");
            assert_eq!(resolved.vault_id, "v-inner");
            assert_eq!(resolved.vault_path, "2026-01-01.md");
            assert_eq!(resolved.vault_dir, "");
        }
        // And a file in the outer vault only is still the outer vault's.
        let resolved =
            note_path_for_file(&[outer, inner], "p1", "notes/inbox/idea.md").expect("resolves");
        assert_eq!(resolved.vault_id, "v-outer");
        assert_eq!(resolved.vault_path, "inbox/idea.md");
        assert_eq!(resolved.vault_dir, "inbox");
    }

    /// The subfolder is matched component by component, never as a string
    /// prefix. `notesy/x.md` shares five characters with `notes` and is not in
    /// it, and a `starts_with` would put it in the vault and then fail to find
    /// its note.
    #[test]
    fn a_sibling_folder_whose_name_starts_with_the_subfolder_is_not_in_the_vault() {
        let vault = VaultLocation {
            vault_id: "v1",
            profile_id: "p1",
            subfolder: "notes",
        };
        assert_eq!(note_path_for_file(&[vault], "p1", "notesy/x.md"), None);
        assert_eq!(note_path_for_file(&[vault], "p1", "not/notes/x.md"), None);
    }

    /// A path relative to profile A means nothing in profile B. Without the
    /// profile filter, every synced folder would inherit every other one's
    /// vault layout.
    #[test]
    fn a_path_is_only_resolved_against_its_own_profiles_vaults() {
        let vaults = [
            VaultLocation {
                vault_id: "v1",
                profile_id: "p1",
                subfolder: "notes",
            },
            VaultLocation {
                vault_id: "v2",
                profile_id: "p2",
                subfolder: "notes",
            },
        ];
        assert_eq!(
            note_path_for_file(&vaults, "p2", "notes/x.md")
                .expect("resolves in its own profile")
                .vault_id,
            "v2"
        );
        assert_eq!(note_path_for_file(&vaults, "p3", "notes/x.md"), None);
    }

    /// The vault directory itself is a folder, and a folder has no note. The
    /// `>=` in the length guard is the whole of this rule and an off-by-one
    /// there would offer "Open in Notes" on the vault folder.
    #[test]
    fn the_vault_directory_itself_is_not_a_note() {
        let vault = VaultLocation {
            vault_id: "v1",
            profile_id: "p1",
            subfolder: "notes/inner",
        };
        assert_eq!(note_path_for_file(&[vault], "p1", "notes/inner"), None);
        assert_eq!(note_path_for_file(&[vault], "p1", "notes/inner/"), None);
        assert_eq!(note_path_for_file(&[vault], "p1", "notes"), None);
        assert!(note_path_for_file(&[vault], "p1", "notes/inner/a.md").is_some());
    }

    /// A profile with no vault is a `NoteVaultVm` carrying an empty subfolder
    /// (`notes_ipc.rs` builds exactly that for an unflagged folder). Treating
    /// the empty string as "matches at depth zero" would make every file in
    /// every synced folder a note in a vault that cannot open it.
    #[test]
    fn an_empty_subfolder_is_no_vault_in_either_direction() {
        let vault = VaultLocation {
            vault_id: "v1",
            profile_id: "p1",
            subfolder: "",
        };
        assert_eq!(note_path_for_file(&[vault], "p1", "anything.md"), None);
        assert_eq!(file_path_for_note(&vault, "anything.md"), None);
        let slashes = VaultLocation {
            subfolder: "//",
            ..vault
        };
        assert_eq!(note_path_for_file(&[slashes], "p1", "anything.md"), None);
        assert_eq!(file_path_for_note(&slashes, "anything.md"), None);
    }

    /// Both directions refuse a path that is not a plain relative descendant,
    /// so a `..` typed into a note's frontmatter or arriving in a restored
    /// cookie cannot address a file beside the profile.
    #[test]
    fn neither_direction_composes_a_path_that_climbs_or_is_absolute() {
        let vault = VaultLocation {
            vault_id: "v1",
            profile_id: "p1",
            subfolder: "notes",
        };
        for hostile in [
            "/etc/passwd",
            "\\\\server\\share\\x.md",
            "\\x.md",
            "C:/secrets.md",
            "c:\\secrets.md",
            "../x.md",
            "notes/../../x.md",
            "./x.md",
            "",
            "/",
        ] {
            assert_eq!(
                file_path_for_note(&vault, hostile),
                None,
                "file_path_for_note accepted {hostile}"
            );
        }
        for hostile in [
            "/notes/x.md",
            "\\notes\\x.md",
            "C:/notes/x.md",
            "notes/../x.md",
            "notes/./x.md",
            "",
        ] {
            assert_eq!(
                note_path_for_file(&[vault], "p1", hostile),
                None,
                "note_path_for_file accepted {hostile}"
            );
        }
    }

    /// The stored spelling and the dirent's spelling differ in case for the
    /// same folder on APFS as a matter of course, and the file's own name must
    /// come back exactly as it was reported — a lowercased answer is a path
    /// that does not exist on a case-sensitive volume.
    #[test]
    fn the_subfolder_matches_case_insensitively_and_the_remainder_keeps_its_case() {
        let vault = VaultLocation {
            vault_id: "v1",
            profile_id: "p1",
            subfolder: "Notes\\Daily/",
        };
        let resolved =
            note_path_for_file(&[vault], "p1", "notes/daily/Sub Folder/Meeting Notes.MD")
                .expect("resolves despite the case and the separators");
        assert_eq!(resolved.vault_path, "Sub Folder/Meeting Notes.MD");
        assert_eq!(resolved.vault_dir, "Sub Folder");
        // The composed direction keeps the CONFIGURED spelling, normalised only
        // in its separators — the one spelling this side has available.
        assert_eq!(
            file_path_for_note(&vault, "Sub Folder/Meeting Notes.MD")
                .expect("composes")
                .relative_path,
            "Notes/Daily/Sub Folder/Meeting Notes.MD"
        );
    }

    /// A backslash is a legal character in a Linux file name, and this rule is
    /// asked about real files. Splitting a dirent path on it would resolve a
    /// file that does not exist; splitting the CONFIGURED subfolder on it is
    /// required, because that string is whatever a human typed.
    #[test]
    fn a_backslash_in_a_file_name_is_a_character_and_not_a_separator() {
        let vault = VaultLocation {
            vault_id: "v1",
            profile_id: "p1",
            subfolder: "notes",
        };
        let resolved = note_path_for_file(&[vault], "p1", "notes/a\\b.md").expect("resolves");
        assert_eq!(resolved.vault_path, "a\\b.md");
        assert_eq!(resolved.vault_dir, "");
        assert_eq!(
            file_path_for_note(&vault, "a\\b.md")
                .expect("composes")
                .relative_path,
            "notes/a\\b.md"
        );
    }

    /// A vault with no id cannot be addressed by any notes command, so
    /// answering with one would produce an action that rejects on click.
    #[test]
    fn a_vault_with_no_id_and_a_file_with_no_profile_resolve_to_nothing() {
        let anonymous = VaultLocation {
            vault_id: "",
            profile_id: "p1",
            subfolder: "notes",
        };
        assert_eq!(note_path_for_file(&[anonymous], "p1", "notes/x.md"), None);
        let rootless = VaultLocation {
            vault_id: "v1",
            profile_id: "",
            subfolder: "notes",
        };
        assert_eq!(note_path_for_file(&[rootless], "", "notes/x.md"), None);
        assert_eq!(file_path_for_note(&rootless, "x.md"), None);
    }
}

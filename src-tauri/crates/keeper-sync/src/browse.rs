//! Read-only browsing of a synced folder, one directory at a time (Story 43.8,
//! FR-153, AD-74, AD-75).
//!
//! # Why this lives here and not in the shell
//!
//! The Files tab asks a question only this crate can answer without guessing:
//! *what is in this folder right now, minus the noise sync already knows to
//! skip, and is the drive even attached?* Both halves of that already exist
//! here — [`crate::exclude::ExcludeSet`] and [`crate::volume::scan`] — and
//! putting the listing beside them means the containment rule, the exclusion
//! rule and the absent-media answer are all asserted over a temp directory on
//! any machine, which is the only way a security rule is ever actually tested.
//! A copy of the rule in the Tauri shell would be a copy that compiles on one
//! platform and is proved on none.
//!
//! # Read-only twice over
//!
//! AD-75 says a browser must never move a file the user did not ask it to move,
//! and that is the obvious half. The half that is easy to get wrong is that a
//! browser must also never move the *engine*: nothing in this module touches
//! [`crate::engine::Engine`], the stability gate, `file_state`, the journal, the
//! scan clock or the watcher. Looking is not an event. Two bugs in this
//! codebase already had the shape "a caller that only meant to LOOK also made
//! the engine forget", and a listing that reached for a convenient engine
//! helper would be the third. Everything here is `std::fs` reads plus one
//! marker read, and it takes a `&SyncProfile` rather than an `&Engine`
//! precisely so it *cannot* make that mistake.
//!
//! # A listing is not a licence to serve bytes
//!
//! Nothing here opens a file. The Files tab reveals, copies a path, or hands a
//! file to the system handler; the `keeper-recording://` protocol stays rooted
//! at the recordings destination and is not widened to reach a synced folder
//! (AD-74). This module hands out names, and the actions on those names are the
//! ones the shell already had.

use std::path::{Component, Path, PathBuf};

use crate::exclude::ExcludeSet;
use crate::profile::SyncProfile;
use crate::volume::{self, VolumeStatus};

/// The most entries one listing will return.
///
/// A synced folder can hold a hundred thousand files, and lazy expansion only
/// bounds the *depth* of what the surface asks for — one flat directory with
/// fifty thousand entries in it is still one call, and serialising it through
/// the IPC boundary would stall the webview for seconds to render a list nobody
/// can read. The cap is reported rather than hidden ([`BrowseDirectory::truncated`]):
/// a browser that silently drops entries is worse than one that says it did.
pub const LISTING_CAP: usize = 1000;

/// What one entry in a browsed directory is, in terms this crate can produce.
///
/// Deliberately not a view model: `keeper-sync` is `keeper-core`-free (AD-40),
/// so the attachment vocabulary — which is `keeper-core`'s
/// `RecordingNoteTargetKind` and belongs to exactly one place (AD-73) — is
/// applied by the shell when it projects these into VMs. What this crate knows
/// is the one thing a `read_dir` knows and an extension table cannot: whether
/// the entry is a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseEntry {
    /// The entry's own file name, with no path in it.
    pub name: String,
    /// The entry's path relative to the profile root, `/`-joined on every
    /// platform — the same frame [`ExcludeSet::is_excluded`] matches in, and the
    /// only path shape that ever crosses to the frontend (FR-145). Feeding it
    /// straight back as the `subpath` of the next call is what makes expansion
    /// lazy without the caller ever composing a path (AD-65).
    pub relative_path: String,
    /// The same entry as an absolute path, joined here onto the resolved
    /// directory and nowhere else. AD-65 asks that a root and a subpath meet in
    /// exactly one place; this is that place for the Files tab, and the shell
    /// above only stringifies what it is handed. Only ever an action's
    /// argument — never shown, never written into a note (FR-145).
    pub absolute_path: PathBuf,
    /// Whether the entry is a directory, following symlinks — a symlink to a
    /// folder expands like the folder it points at, and [`resolve`] refuses it
    /// on the next call if it points outside the root.
    pub is_dir: bool,
}

/// One directory's worth of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseDirectory {
    /// Directories first, then files, each group by name — a stable order that
    /// does not depend on what `read_dir` happens to hand back, so the same
    /// folder reads the same way twice.
    pub entries: Vec<BrowseEntry>,
    /// Whether [`LISTING_CAP`] cut the list short. The surface says so; it does
    /// not pretend the folder ended there.
    pub truncated: bool,
}

/// What a browse call found, when it was not refused outright.
///
/// The three non-listing answers are separate variants rather than an empty
/// list because they are separate facts, and the whole of a user's trust in a
/// file browser is that it does not report "nothing here" when the truth is
/// "I could not look". An unplugged pendrive and an empty folder look identical
/// to `read_dir` — the folder is simply gone — and collapsing them is how a
/// browser tells someone their recordings were deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseListing {
    /// The directory was read. `entries` may legitimately be empty: that is the
    /// one place "nothing here" is the truth.
    Listed(BrowseDirectory),
    /// This profile is on removable media (AD-48) and the media is not
    /// attached. A pause, not a fault, and nothing on disk is missing.
    MediaAbsent,
    /// This profile is on removable media and something else is mounted where
    /// its volume lives — a second stick at the first one's mountpoint. Never
    /// listed: those files belong to a stranger's disk, and showing them under
    /// this profile's name would be a lie about whose folder it is.
    MediaUnexpected {
        /// The foreign volume's id, for a sentence that says what is wrong
        /// rather than only that something is.
        found_id: String,
    },
    /// The directory is not on disk, on media that is attached (or on a fixed
    /// disk, where there is no media question). Moved, renamed or deleted
    /// outside keeper — a different next step from absent media, so a different
    /// answer.
    Missing,
}

/// Why a browse call was refused before anything was read.
///
/// Refusals are separate from [`BrowseListing`] because they are not states of
/// the folder — they are states of the *request*, and a caller that produces
/// one has asked for something it must never be able to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseRefusal {
    /// The subpath is not a plain descendant of the profile root: it was
    /// absolute, or it contained `..`, `.`, an empty component, or (on Windows)
    /// a drive prefix or a backslash separator.
    Escapes {
        /// The offending subpath, verbatim, so the log names what was asked
        /// for.
        subpath: String,
    },
    /// The subpath resolved to a plain descendant lexically and then, once
    /// symlinks were followed, landed outside the profile root. A planted
    /// symlink is the one escape no string test can catch (AD-59).
    EscapesAfterResolution {
        /// The offending subpath, verbatim.
        subpath: String,
    },
    /// The directory is there and could not be read — a permissions failure,
    /// most often. Distinct from [`BrowseListing::Missing`]: the folder exists,
    /// so telling someone it is gone would send them looking for a backup.
    Unreadable {
        /// The OS's own words, which are the most useful thing to show.
        reason: String,
    },
}

impl std::fmt::Display for BrowseRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Escapes { subpath } => write!(
                f,
                "\"{subpath}\" is not a path inside this folder, so it will not be listed"
            ),
            Self::EscapesAfterResolution { subpath } => write!(
                f,
                "\"{subpath}\" resolves outside this folder, so it will not be listed"
            ),
            Self::Unreadable { reason } => write!(f, "this folder could not be read: {reason}"),
        }
    }
}

/// Turn a profile-relative subpath into an absolute path under `root`, or
/// refuse it.
///
/// **This is the whole of AD-65 for the Files tab.** The frontend never joins a
/// root and a subpath; it hands back a `relative_path` this module produced and
/// this function does the join, once, in Rust — and gets to say no.
///
/// The lexical half runs first and runs unconditionally, before the disk is
/// consulted at all, so `..` is refused identically whether the drive is in,
/// out, or replaced by a stranger's. Each component must be exactly one
/// `Component::Normal`, which is a single test that covers every shape an
/// escape takes and covers it *per platform*: `..` is `ParentDir`, `.` is
/// `CurDir`, a leading `/` is `RootDir`, `C:` is `Prefix`, and on Windows —
/// where `\` is a separator and on Linux it is an ordinary filename character —
/// `a\b` yields two components there and one here, which is exactly the
/// difference that matters.
///
/// The canonicalizing half catches what no string test can: a symlink inside
/// the folder pointing somewhere else (AD-59, the same two-halves idiom
/// `recording_open_path` uses). Canonicalizing the root as well is what keeps a
/// folder reached through a symlink — a relocated home directory — from
/// refusing every path under it.
///
/// `Ok(None)` means the lexical test passed and the path is simply not on disk;
/// the caller decides whether that is absent media or a missing folder, because
/// only the caller knows whether the profile is removable.
pub fn resolve(root: &Path, subpath: &str) -> Result<Option<PathBuf>, BrowseRefusal> {
    let escapes = || BrowseRefusal::Escapes {
        subpath: subpath.to_owned(),
    };
    let mut target = root.to_path_buf();
    for segment in subpath.split('/') {
        if segment.is_empty() {
            // A leading, trailing or doubled `/`. Tolerating it would mean
            // tolerating `""`, `"/"` and `"a//b"` as three spellings of paths
            // that are not the same request, and the root is spelled `""` —
            // which never enters this loop because `"".split('/')` yields one
            // empty segment and we would have to special-case it anyway.
            if subpath.is_empty() {
                break;
            }
            return Err(escapes());
        }
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => target.push(name),
            _ => return Err(escapes()),
        }
    }

    let (Ok(canonical_root), Ok(canonical_target)) = (root.canonicalize(), target.canonicalize())
    else {
        // Either the profile folder is not there (absent media, or moved), or
        // the entry under it is not. Not a refusal: nothing was asked for that
        // may not be asked for.
        return Ok(None);
    };
    if !canonical_target.starts_with(&canonical_root) {
        return Err(BrowseRefusal::EscapesAfterResolution {
            subpath: subpath.to_owned(),
        });
    }
    Ok(Some(canonical_target))
}

/// List one directory of one profile, lazily and without touching the engine.
///
/// `subpath` is `""` for the profile root and otherwise a `/`-joined path this
/// module previously handed out. `excludes` is compiled by the caller so a
/// surface expanding a tree pays for the glob compilation once rather than per
/// click.
///
/// The order of the checks is the contract:
///
/// 1. The lexical containment test, always — an escape is refused even with the
///    drive unplugged, so the refusal cannot be probed for by pulling the media.
/// 2. The volume, for a removable profile only. Every ordinary folder pays one
///    boolean for this feature.
/// 3. The disk.
pub fn browse(
    profile: &SyncProfile,
    subpath: &str,
    excludes: &ExcludeSet,
) -> Result<BrowseListing, BrowseRefusal> {
    let resolved = resolve(&profile.local_path, subpath)?;

    if profile.removable {
        // Read-only: `volume::scan` walks up to the nearest marker and reads it.
        // It never mints, adopts or rewrites one — that is `VolumeMarker::ensure`,
        // and browsing must not be the thing that binds a profile to a disk.
        match volume::scan(&profile.local_path, profile.volume_id.as_deref()) {
            Ok(VolumeStatus::Present { .. }) => {}
            Ok(VolumeStatus::Absent) => return Ok(BrowseListing::MediaAbsent),
            Ok(VolumeStatus::Foreign { found_id }) => {
                return Ok(BrowseListing::MediaUnexpected { found_id })
            }
            // An unreadable marker is "something is there and it is not provably
            // yours", which takes the same refusal as a foreign one and a
            // different sentence. Reported, never treated as attached.
            Err(error) => {
                return Ok(BrowseListing::MediaUnexpected {
                    found_id: error.to_string(),
                })
            }
        }
    }

    let Some(dir) = resolved else {
        return Ok(BrowseListing::Missing);
    };
    if !dir.is_dir() {
        // A file was asked to be expanded, or the directory turned into one
        // between the resolve and now. Either way there are no children.
        return Ok(BrowseListing::Missing);
    }

    let prefix = subpath.trim_end_matches('/');
    let read = std::fs::read_dir(&dir).map_err(|error| BrowseRefusal::Unreadable {
        reason: error.to_string(),
    })?;

    let mut entries: Vec<BrowseEntry> = Vec::new();
    let mut truncated = false;
    for entry in read {
        let Ok(entry) = entry else {
            // One unreadable dirent must not fail the whole listing: a folder
            // with one broken entry is still a folder worth browsing.
            continue;
        };
        // Lossy rather than skipped. On macOS — the only platform this surface
        // renders on — the filesystem APIs guarantee UTF-8, so the substitution
        // branch is unreachable there; anywhere else a mangled name is still
        // better than a hole in a browser, and every action on it is re-resolved
        // in Rust, where it will honestly refuse rather than open the wrong file.
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        // `metadata` follows symlinks; `file_type` would report the link. A
        // symlink to a folder should expand like a folder, and `resolve` is what
        // stops it if it points out of the tree. A broken symlink has no
        // metadata and reads as a file, which is what it looks like on disk.
        let absolute_path = entry.path();
        let is_dir = std::fs::metadata(&absolute_path).is_ok_and(|meta| meta.is_dir());
        let match_path = Path::new(&relative_path);
        let excluded = if is_dir {
            excludes.is_excluded_directory(match_path)
        } else {
            excludes.is_excluded(match_path)
        };
        if excluded {
            continue;
        }
        if entries.len() == LISTING_CAP {
            truncated = true;
            break;
        }
        entries.push(BrowseEntry {
            name,
            relative_path,
            absolute_path,
            is_dir,
        });
    }

    // Folders first, then files, then by name — case-insensitively, because a
    // list where `Zebra` sorts before `apple` is a list people re-read twice.
    // The case-sensitive comparison breaks the tie so the order is total.
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        })
    });

    Ok(BrowseListing::Listed(BrowseDirectory {
        entries,
        truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(local_path: &Path) -> SyncProfile {
        SyncProfile::new(
            "01PROFILE",
            "Field",
            local_path,
            "https://example.invalid/r.git",
        )
    }

    fn no_excludes() -> ExcludeSet {
        ExcludeSet::new(&[]).expect("builtin corpus compiles")
    }

    fn names(listing: &BrowseListing) -> Vec<String> {
        match listing {
            BrowseListing::Listed(dir) => dir.entries.iter().map(|e| e.name.clone()).collect(),
            other => panic!("expected a listing, got {other:?}"),
        }
    }

    // --- The escape rule ---------------------------------------------------
    //
    // The reason this module is in `keeper-sync` and not in the Tauri shell:
    // these five run on any machine.

    #[test]
    fn parent_traversal_is_refused_at_every_position() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join("inner")).expect("inner");
        for subpath in ["..", "../..", "inner/..", "inner/../..", "../inner"] {
            assert_eq!(
                resolve(root.path(), subpath),
                Err(BrowseRefusal::Escapes {
                    subpath: subpath.to_owned()
                }),
                "{subpath} must not resolve"
            );
        }
    }

    #[test]
    fn absolute_and_degenerate_subpaths_are_refused() {
        let root = tempfile::tempdir().expect("temp");
        for subpath in ["/etc", "/", "inner/", "/inner", "inner//child", ".", "./x"] {
            assert!(
                matches!(
                    resolve(root.path(), subpath),
                    Err(BrowseRefusal::Escapes { .. })
                ),
                "{subpath} must not resolve"
            );
        }
    }

    #[test]
    fn the_root_itself_is_the_empty_subpath() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            resolve(root.path(), "").expect("root resolves"),
            Some(root.path().canonicalize().expect("canonical root"))
        );
    }

    #[test]
    fn a_plain_descendant_resolves_under_the_root() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("a/b")).expect("tree");
        let resolved = resolve(root.path(), "a/b")
            .expect("resolves")
            .expect("exists");
        assert!(resolved.starts_with(root.path().canonicalize().expect("canonical")));
        assert!(resolved.ends_with("a/b"));
    }

    /// The half a lexical test cannot catch. A `..` never appears in the
    /// subpath; the escape is planted on disk.
    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_refused_after_resolution() {
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secrets.txt"), "x").expect("secret");
        let root = tempfile::tempdir().expect("root");
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).expect("symlink");

        assert_eq!(
            resolve(root.path(), "escape"),
            Err(BrowseRefusal::EscapesAfterResolution {
                subpath: "escape".to_owned()
            })
        );
        // And the whole browse call refuses, not merely the resolver.
        assert_eq!(
            browse(&profile(root.path()), "escape", &no_excludes()),
            Err(BrowseRefusal::EscapesAfterResolution {
                subpath: "escape".to_owned()
            })
        );
    }

    /// An escape must be refused on its own terms, not because the disk
    /// happened to be unavailable — otherwise the refusal could be probed for
    /// by pulling the drive.
    #[test]
    fn traversal_is_refused_even_when_the_media_is_absent() {
        let root = tempfile::tempdir().expect("temp");
        let mut removable = profile(&root.path().join("gone"));
        removable.removable = true;
        assert_eq!(
            browse(&removable, "../..", &no_excludes()),
            Err(BrowseRefusal::Escapes {
                subpath: "../..".to_owned()
            })
        );
    }

    // --- Absent media is not an empty folder -------------------------------

    #[test]
    fn a_removable_profile_with_no_marker_reports_absent_media() {
        let root = tempfile::tempdir().expect("temp");
        let mut removable = profile(root.path());
        removable.removable = true;
        removable.volume_id = Some("01VOLUME".to_owned());
        assert_eq!(
            browse(&removable, "", &no_excludes()).expect("no refusal"),
            BrowseListing::MediaAbsent
        );
    }

    #[test]
    fn an_empty_folder_on_a_fixed_disk_is_an_empty_listing_not_absent_media() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            browse(&profile(root.path()), "", &no_excludes()).expect("no refusal"),
            BrowseListing::Listed(BrowseDirectory {
                entries: Vec::new(),
                truncated: false,
            })
        );
    }

    #[test]
    fn a_foreign_marker_is_neither_listed_nor_reported_as_absent() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("theirs.txt"), "x").expect("file");
        volume::VolumeMarker::write(
            root.path(),
            &volume::VolumeMarker::new("Someone Else", 1_700_000_000_000),
        )
        .expect("marker");

        let mut removable = profile(root.path());
        removable.removable = true;
        removable.volume_id = Some("01NOTTHEIRS".to_owned());
        assert!(matches!(
            browse(&removable, "", &no_excludes()).expect("no refusal"),
            BrowseListing::MediaUnexpected { .. }
        ));
    }

    #[test]
    fn a_removable_profile_whose_volume_is_attached_lists_normally() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("clip.mov"), "x").expect("file");
        let marker = volume::VolumeMarker::new("merope", 1_700_000_000_000);
        volume::VolumeMarker::write(root.path(), &marker).expect("marker");

        let mut removable = profile(root.path());
        removable.removable = true;
        removable.volume_id = Some(marker.volume_id.clone());
        assert_eq!(
            names(&browse(&removable, "", &no_excludes()).expect("no refusal")),
            vec!["clip.mov"]
        );
    }

    #[test]
    fn a_fixed_folder_that_is_gone_is_missing_not_absent_media() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            browse(
                &profile(&root.path().join("moved-away")),
                "",
                &no_excludes()
            )
            .expect("no refusal"),
            BrowseListing::Missing
        );
    }

    // --- Listing shape ------------------------------------------------------

    #[test]
    fn tier_zero_noise_is_filtered_by_the_one_exclude_set() {
        let root = tempfile::tempdir().expect("temp");
        for dir in ["node_modules", ".keeper", ".git", ".keeper-sync", "Notes"] {
            std::fs::create_dir(root.path().join(dir)).expect("dir");
        }
        for file in [".DS_Store", "clip.mov.partial", "notes.md"] {
            std::fs::write(root.path().join(file), "x").expect("file");
        }
        assert_eq!(
            names(&browse(&profile(root.path()), "", &no_excludes()).expect("no refusal")),
            vec!["Notes", "notes.md"]
        );
    }

    #[test]
    fn a_profile_pattern_excludes_on_top_of_the_builtin_corpus() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("keep.md"), "x").expect("file");
        std::fs::write(root.path().join("drop.tmp"), "x").expect("file");
        let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("compiles");
        assert_eq!(
            names(&browse(&profile(root.path()), "", &excludes).expect("no refusal")),
            vec!["keep.md"]
        );
    }

    #[test]
    fn folders_sort_before_files_and_names_sort_case_insensitively() {
        let root = tempfile::tempdir().expect("temp");
        for dir in ["zeta", "Alpha"] {
            std::fs::create_dir(root.path().join(dir)).expect("dir");
        }
        for file in ["b.md", "A.md"] {
            std::fs::write(root.path().join(file), "x").expect("file");
        }
        assert_eq!(
            names(&browse(&profile(root.path()), "", &no_excludes()).expect("no refusal")),
            vec!["Alpha", "zeta", "A.md", "b.md"]
        );
    }

    #[test]
    fn a_child_listing_carries_the_parent_prefix_and_the_composed_absolute_path() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("2026/Standup")).expect("tree");
        std::fs::write(root.path().join("2026/Standup/manifest.json"), "{}").expect("file");

        let listing =
            browse(&profile(root.path()), "2026/Standup", &no_excludes()).expect("no refusal");
        let BrowseListing::Listed(dir) = listing else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), 1);
        let entry = &dir.entries[0];
        assert_eq!(entry.relative_path, "2026/Standup/manifest.json");
        assert!(!entry.is_dir);
        // The join happens here and only here: the shell stringifies this, and
        // no TypeScript ever puts a root and a subpath together (AD-65).
        assert_eq!(
            entry.absolute_path,
            root.path()
                .canonicalize()
                .expect("canonical root")
                .join("2026/Standup/manifest.json")
        );
        // …and the relative path never leaks the machine's own directory names
        // into a string the surface renders (FR-145).
        assert!(!entry
            .relative_path
            .contains(&*root.path().to_string_lossy()));
    }

    /// Lazy by construction: asking for the root reads the root's dirents and
    /// nothing below them, so the surface cannot accidentally walk a
    /// hundred-thousand-file tree by rendering a folder.
    #[test]
    fn listing_a_directory_does_not_descend_into_its_children() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("deep/deeper")).expect("tree");
        std::fs::write(root.path().join("deep/deeper/buried.md"), "x").expect("file");

        let listing = browse(&profile(root.path()), "", &no_excludes()).expect("no refusal");
        let BrowseListing::Listed(dir) = listing else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), 1);
        assert_eq!(dir.entries[0].name, "deep");
        assert!(dir.entries[0].is_dir);
    }

    #[test]
    fn a_directory_beyond_the_cap_reports_that_it_was_cut_short() {
        let root = tempfile::tempdir().expect("temp");
        for index in 0..(LISTING_CAP + 5) {
            std::fs::write(root.path().join(format!("f{index:06}.md")), "x").expect("file");
        }
        let BrowseListing::Listed(dir) =
            browse(&profile(root.path()), "", &no_excludes()).expect("no refusal")
        else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), LISTING_CAP);
        assert!(dir.truncated);
    }

    #[test]
    fn expanding_a_file_reports_missing_rather_than_an_empty_folder() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("notes.md"), "x").expect("file");
        assert_eq!(
            browse(&profile(root.path()), "notes.md", &no_excludes()).expect("no refusal"),
            BrowseListing::Missing
        );
    }
}

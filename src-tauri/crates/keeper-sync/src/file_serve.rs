//! Resolving one profile-scoped path for the `keeper-file://` scheme
//! (Story 45.7, FR-180, AD-59, AD-65, AD-74).
//!
//! # Why this exists, and why here
//!
//! Story 45.7 opens images, audio and video in a panel. The bytes have to reach
//! the webview, and neither existing scheme's coordinates fit a Files-pane file:
//! `keeper-recording://` is rooted at the recordings destination and keyed by a
//! SESSION id (AD-74 says in as many words that the Files tab must not reach for
//! it), and `keeper-note://` is rooted at `vault.root` — `local_path/subfolder`
//! — which is narrower than the root the Files pane browses. So a fourth scheme
//! with a fourth fixed root: the sync profile's own `local_path`.
//!
//! A protocol handler is a hole in the sandbox unless its root is fixed and
//! every request is checked against it (AD-59). That check is not written here.
//! It is [`crate::browse::resolve`] — the same function `sync_browse`,
//! `sync_open_entry`, `sync_read_text` and `files_write::resolve_existing`
//! already call. This module is the second caller of a rule, not a second copy
//! of one.
//!
//! **And that is exactly why the function lives in this crate rather than in the
//! Tauri shell beside the handler.** The shell does not build on Linux (AD-55,
//! AD-56), so a resolution written there is a security decision compiled on one
//! platform and proved on none — 43.8's own argument for putting `browse` here,
//! applied to the story that adds a second caller. What is left in the shell is
//! four lines with no path logic in them: parse the URL, ask this, ask for a
//! `Content-Type`, answer.
//!
//! # The order is part of the rule
//!
//! An unknown profile id is refused **before any path work happens**. Not
//! "resolves to nothing later" — refused, with no join, no `canonicalize` and no
//! `stat`. A URL can arrive from anywhere in the webview, including from text
//! that came over sync from another machine, so the id is the first gate and an
//! unregistered id costs an attacker a string comparison and nothing else.
//!
//! # What this module does NOT decide
//!
//! Whether the FORMAT may be served at all is `keeper_core::file_asset`'s
//! answer, and it cannot be asked from here: AD-40 makes this crate deliberately
//! `keeper-core`-free. The shell asks it, before this, on the file's name alone
//! — a string test that touches no path and no disk, so it does not disturb the
//! ordering above. Range, the slice cap and the 200/206/416/404 shapes are
//! `note_protocol`'s and are called rather than copied.

use std::path::{Path, PathBuf};

use crate::browse::{self, BrowseRefusal};
use crate::profile::SyncProfile;

/// Why one `keeper-file://` request will not be served.
///
/// Distinct variants although the handler collapses every one of them to the
/// same 404 — a probe must not be able to tell "outside the folder" from "not
/// there". They exist for the two readers who are not the attacker: the log
/// line, which says which refusal fired without echoing the path, and the test,
/// which asserts that an unknown profile is refused *as an unknown profile*
/// rather than incidentally by a later check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeRefusal {
    /// No profile carries this id. The first gate, and the cheapest.
    UnknownProfile,
    /// The subpath is not a path inside the profile root — lexically, or after
    /// resolution. Carries [`browse::resolve`]'s own refusal so the reason a log
    /// records is the reason the containment rule gave.
    Escapes(BrowseRefusal),
    /// The lexical test passed and nothing is at that path: deleted, renamed, or
    /// on a volume that is not attached.
    Missing,
    /// Something is there and it is not a regular file. A fifo would block the
    /// read forever and a device would lie about its length.
    NotAFile,
}

impl std::fmt::Display for ServeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProfile => write!(f, "no profile carries that id"),
            Self::Escapes(refusal) => write!(f, "{refusal}"),
            Self::Missing => write!(f, "nothing is at that path"),
            Self::NotAFile => write!(f, "that path is not a regular file"),
        }
    }
}

/// The absolute path a `keeper-file://` request may read, or the refusal that
/// ends it.
///
/// Takes the profile LIST rather than a root, so that "is this a profile keeper
/// knows" is answered here and cannot be skipped by a caller that already has a
/// `PathBuf` in hand. A handler that could be handed a root would eventually be
/// handed one from the URL.
///
/// Returns a canonical path: [`browse::resolve`] canonicalizes both sides, so
/// what comes back is the file that will actually be read rather than a name
/// that resolves to it.
pub fn resolve_served_path(
    profiles: &[SyncProfile],
    profile_id: &str,
    subpath: &str,
) -> Result<PathBuf, ServeRefusal> {
    // First, and before the path is looked at at all.
    let profile = profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or(ServeRefusal::UnknownProfile)?;

    let resolved = browse::resolve(&profile.local_path, subpath).map_err(ServeRefusal::Escapes)?;
    let resolved = resolved.ok_or(ServeRefusal::Missing)?;

    // `resolve` has already followed every link, so this describes the file that
    // would actually be opened rather than the name that points at it.
    let meta = std::fs::symlink_metadata(&resolved).map_err(|_| ServeRefusal::Missing)?;
    if !meta.is_file() {
        return Err(ServeRefusal::NotAFile);
    }
    Ok(resolved)
}

/// The last `/`-separated component of a subpath — the file's own name, which
/// is all the format allow-list needs.
///
/// Here rather than in the shell because the shell is where an off-by-one in a
/// string split would be uncompilable on this machine, and because `rsplit`
/// silently returning the whole subpath for a path with no separator is the
/// behaviour that makes a root-level file work.
pub fn served_file_name(subpath: &str) -> &str {
    subpath.rsplit('/').next().unwrap_or(subpath)
}

/// Whether `path` is inside `root` after both are canonicalized.
///
/// Not used by [`resolve_served_path`] — `browse::resolve` already does this —
/// and exported for the test that proves the second caller inherits it. Keeping
/// the assertion honest requires an independent way to state the property, and
/// re-calling `browse::resolve` to check `browse::resolve` would prove nothing.
pub fn is_inside(root: &Path, path: &Path) -> bool {
    match (root.canonicalize(), path.canonicalize()) {
        (Ok(root), Ok(path)) => path.starts_with(&root),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn profile(id: &str, local_path: &Path) -> SyncProfile {
        SyncProfile::new(id, "Field", local_path, "https://example.invalid/r.git")
    }

    /// A profile root with one servable file in it and one nested.
    fn tree() -> (tempfile::TempDir, Vec<SyncProfile>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("folder");
        fs::create_dir_all(root.join("2026/08")).expect("mkdir");
        fs::write(root.join("clip.mov"), b"video-bytes").expect("write");
        fs::write(root.join("2026/08/screen-0000.mov"), b"more").expect("write");
        let profiles = vec![profile("01PROFILE", &root)];
        (dir, profiles)
    }

    #[test]
    fn resolves_a_file_at_the_root_and_a_file_below_it() {
        let (dir, profiles) = tree();
        let root = dir.path().join("folder");

        let at_root = resolve_served_path(&profiles, "01PROFILE", "clip.mov").expect("resolved");
        assert!(is_inside(&root, &at_root));
        assert_eq!(fs::read(&at_root).expect("read"), b"video-bytes");

        let nested = resolve_served_path(&profiles, "01PROFILE", "2026/08/screen-0000.mov")
            .expect("resolved");
        assert!(is_inside(&root, &nested));
    }

    #[test]
    fn an_unregistered_profile_id_is_refused_as_an_unknown_profile() {
        let (_dir, profiles) = tree();
        // Named as the FIRST gate, not as an incidental miss: the path below is
        // a real file of a real profile, so anything other than
        // `UnknownProfile` would mean the id was not the thing that refused it.
        assert_eq!(
            resolve_served_path(&profiles, "01SOMEONE-ELSE", "clip.mov"),
            Err(ServeRefusal::UnknownProfile)
        );
        assert_eq!(
            resolve_served_path(&[], "01PROFILE", "clip.mov"),
            Err(ServeRefusal::UnknownProfile)
        );
        assert_eq!(
            resolve_served_path(&profiles, "", "clip.mov"),
            Err(ServeRefusal::UnknownProfile)
        );
    }

    #[test]
    fn an_unregistered_profile_id_is_refused_before_any_path_work() {
        // The proof that the id gate runs first rather than merely runs: a
        // subpath that would itself be refused, under an id that does not
        // exist, must come back as the ID's refusal. If the path were consulted
        // first this would be `Escapes`, and a handler could be induced to
        // canonicalize an attacker's string before deciding whether it had any
        // business doing so at all.
        let (_dir, profiles) = tree();
        assert_eq!(
            resolve_served_path(&profiles, "01NOPE", "../../../etc/passwd"),
            Err(ServeRefusal::UnknownProfile)
        );
        // And with no profiles configured at all, where there is not even a root
        // to join onto.
        assert_eq!(
            resolve_served_path(&[], "01NOPE", "../../../etc/passwd"),
            Err(ServeRefusal::UnknownProfile)
        );
    }

    #[test]
    fn a_parent_segment_is_refused_lexically() {
        let (_dir, profiles) = tree();
        for subpath in [
            "../clip.mov",
            "2026/../../clip.mov",
            "..",
            "a/../../b.mov",
            "./clip.mov",
        ] {
            assert!(
                matches!(
                    resolve_served_path(&profiles, "01PROFILE", subpath),
                    Err(ServeRefusal::Escapes(_))
                ),
                "{subpath} was not refused"
            );
        }
    }

    #[test]
    fn an_absolute_path_where_a_relative_one_belongs_is_refused() {
        let (_dir, profiles) = tree();
        // The shape that is not obviously an escape: no `..` anywhere, and on a
        // naive `root.join(subpath)` an absolute path REPLACES the root
        // entirely, which is how a containment check that only looked for `..`
        // hands out `/etc/passwd`.
        for subpath in ["/etc/passwd", "/clip.mov", "//etc/passwd"] {
            assert!(
                matches!(
                    resolve_served_path(&profiles, "01PROFILE", subpath),
                    Err(ServeRefusal::Escapes(_))
                ),
                "{subpath} was not refused"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_out_of_the_profile_root_is_refused_after_resolution() {
        let (dir, profiles) = tree();
        let root = dir.path().join("folder");
        let outside = dir.path().join("outside.mov");
        fs::write(&outside, b"not yours").expect("write");
        std::os::unix::fs::symlink(&outside, root.join("escape.mov")).expect("symlink");

        // Every string test this passes: one component, a normal name, no dots.
        // Only canonicalisation catches it, which is the half a lexical rule
        // cannot have and the reason `browse::resolve` does both.
        assert_eq!(
            resolve_served_path(&profiles, "01PROFILE", "escape.mov"),
            Err(ServeRefusal::Escapes(
                BrowseRefusal::EscapesAfterResolution {
                    subpath: "escape.mov".to_owned(),
                }
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_stays_inside_the_profile_root_is_served() {
        // The other half, so the rule above is containment and not a blanket
        // refusal of links: a synced folder full of symlinked media would
        // otherwise be unopenable, and the check would look correct while being
        // useless.
        let (dir, profiles) = tree();
        let root = dir.path().join("folder");
        std::os::unix::fs::symlink(root.join("clip.mov"), root.join("alias.mov")).expect("symlink");

        let resolved = resolve_served_path(&profiles, "01PROFILE", "alias.mov").expect("resolved");
        assert!(is_inside(&root, &resolved));
        assert_eq!(fs::read(&resolved).expect("read"), b"video-bytes");
    }

    #[test]
    fn a_path_that_is_not_on_disk_is_missing_rather_than_an_escape() {
        let (_dir, profiles) = tree();
        // Two different facts and two different log lines: "the drive is out"
        // is not "somebody probed us".
        assert_eq!(
            resolve_served_path(&profiles, "01PROFILE", "gone.mov"),
            Err(ServeRefusal::Missing)
        );
        assert_eq!(
            resolve_served_path(&profiles, "01PROFILE", "2026/08/gone.mov"),
            Err(ServeRefusal::Missing)
        );
    }

    #[test]
    fn a_directory_is_not_a_file_to_serve() {
        let (_dir, profiles) = tree();
        assert_eq!(
            resolve_served_path(&profiles, "01PROFILE", "2026"),
            Err(ServeRefusal::NotAFile)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_rather_than_read_until_the_end_of_time() {
        let (dir, profiles) = tree();
        let root = dir.path().join("folder");
        let fifo = root.join("pipe.mov");
        // `mkfifo` through the shell rather than a crate: keeper-sync carries no
        // libc dependency and this test may skip where the tool is absent.
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            return;
        }
        assert_eq!(
            resolve_served_path(&profiles, "01PROFILE", "pipe.mov"),
            Err(ServeRefusal::NotAFile)
        );
    }

    #[test]
    fn one_profile_cannot_be_reached_through_another_profiles_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mine = dir.path().join("mine");
        let yours = dir.path().join("yours");
        fs::create_dir_all(&mine).expect("mkdir");
        fs::create_dir_all(&yours).expect("mkdir");
        fs::write(yours.join("secret.mov"), b"yours").expect("write");
        let profiles = vec![profile("01MINE", &mine), profile("01YOURS", &yours)];

        // The file exists, and under the other profile it resolves — so this is
        // the id doing the containment rather than the filesystem.
        assert!(resolve_served_path(&profiles, "01YOURS", "secret.mov").is_ok());
        assert_eq!(
            resolve_served_path(&profiles, "01MINE", "secret.mov"),
            Err(ServeRefusal::Missing)
        );
        assert!(matches!(
            resolve_served_path(&profiles, "01MINE", "../yours/secret.mov"),
            Err(ServeRefusal::Escapes(_))
        ));
    }

    #[test]
    fn the_file_name_is_the_last_segment_and_a_rootless_path_is_its_own_name() {
        assert_eq!(served_file_name("clip.mov"), "clip.mov");
        assert_eq!(
            served_file_name("2026/08/screen-0000.mov"),
            "screen-0000.mov"
        );
        assert_eq!(served_file_name(""), "");
        assert_eq!(served_file_name("a/"), "");
    }
}

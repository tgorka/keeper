//! Where a note the person is looking at lives, as its agent names it (story
//! 91.2; shared with 91.3's surface tools).
//!
//! A notes vault is a synced folder plus a flag; a drive is a synced folder
//! whose agents zone declares it in `_drive.toml`. The docked notes view
//! names a note by its vault and its vault-relative path, and the agent by
//! its drive id and drive-relative path. The two are joined here once, by
//! `keeper_sync::browse`'s segment rules (AD-65), never by string arithmetic
//! in a caller.

use keeper_core::agents::drive::{self, DriveDecl};
use keeper_sync::browse;
use keeper_sync::SyncProfile;

use crate::zone::read_text;

/// A note as its drive names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRef {
    /// The drive's id, from its `_drive.toml`.
    pub drive: String,
    /// The note's path from the drive's root, `/`-joined.
    pub path: String,
}

/// The `_drive.toml` of the synced folder `profile`, read from its agents
/// zone; the sentence when it has none or it does not read.
pub fn declared(profile: &SyncProfile) -> Result<DriveDecl, String> {
    let zone = profile
        .agents_root()
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    let text = read_text(&zone, drive::FILE_NAME)?
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    drive::parse(&text).map_err(|refusal| refusal.sentence())
}

/// The note at `note_path` in the vault kept in `profile`'s `subfolder`, as
/// its drive names it; `None` when the folder declares no drive, or the path
/// does not name a file inside it.
pub fn drive_of(profile: &SyncProfile, subfolder: &str, note_path: &str) -> Option<DriveRef> {
    let decl = declared(profile).ok()?;
    let mut segments = browse::plain_segments(subfolder.trim_matches('/')).ok()?;
    segments.extend(browse::plain_segments(note_path).ok()?);
    if segments.is_empty() {
        return None;
    }
    let parts: Option<Vec<&str>> = segments.iter().map(|segment| segment.to_str()).collect();
    let path = parts?.join("/");
    match browse::resolve(&profile.local_path, &path) {
        Ok(Some(found)) if found.is_file() => Some(DriveRef {
            drive: decl.id,
            path,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(declares: bool) -> (tempfile::TempDir, SyncProfile) {
        let root = tempfile::tempdir().expect("tempdir");
        let write = |rel: &str, text: &str| {
            let path = root.path().join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        };
        if declares {
            write(
                "80-agents/_drive.toml",
                "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\n",
            );
        }
        write("notes/plans/q3.md", "# Q3\n");
        let mut profile = SyncProfile::new("p1", "tgdrive", root.path(), "unused");
        profile.agents = Some(Default::default());
        (root, profile)
    }

    #[test]
    fn a_vault_note_is_named_by_its_drive() {
        let (_root, profile) = folder(true);
        assert_eq!(
            drive_of(&profile, "notes", "plans/q3.md"),
            Some(DriveRef {
                drive: "tgdrive".to_owned(),
                path: "notes/plans/q3.md".to_owned(),
            })
        );
        // A vault at the folder's root.
        assert_eq!(
            drive_of(&profile, "", "notes/plans/q3.md").map(|found| found.path),
            Some("notes/plans/q3.md".to_owned())
        );
        // No such note, an escape, a folder with no `_drive.toml`.
        assert_eq!(drive_of(&profile, "notes", "plans/q4.md"), None);
        assert_eq!(drive_of(&profile, "notes", "../notes/plans/q3.md"), None);
        let (_other, undeclared) = folder(false);
        assert_eq!(drive_of(&undeclared, "notes", "plans/q3.md"), None);
    }
}

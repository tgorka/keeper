//! An agent's drive and the note its person is looking at (stories 91.2,
//! 91.3; AD-382, AD-383).
//!
//! A notes vault is a synced folder plus a flag; a drive is a synced folder
//! whose agents zone declares it in `_drive.toml`. The docked notes view
//! names a note by its vault and its vault-relative path, and the agent by
//! its drive id and drive-relative path. The two are joined here, both ways
//! ([`drive_of`], [`locate`]), by `keeper_sync::browse`'s segment rules
//! (AD-65), never by string arithmetic in a caller.
//!
//! The surface tools — a request into the person's focused device over the
//! agent's Matrix room — left keeper in 108.2 (DW-E106-1).

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

/// Where on this device a surface request's drive and path are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// A note of the vault kept in the profile `vault_id`, at `note_path`
    /// from the vault's folder: opened in the editor.
    Note { vault_id: String, note_path: String },
    /// Any other file of the synced folder `profile_id`: opened in the Files
    /// preview, never the editor.
    File {
        profile_id: String,
        relative_path: String,
    },
}

/// The file `path` of the drive `drive` among this device's synced folders,
/// each with its notes vault's subfolder when it keeps one. A path the
/// segment rules refuse, or one that resolves outside its folder, is
/// refused with `browse`'s own sentence (AD-65).
pub fn locate(
    folders: &[(SyncProfile, Option<String>)],
    drive: &str,
    path: &str,
) -> Result<Located, String> {
    let (profile, vault) = folders
        .iter()
        .find(|(profile, _)| declared(profile).is_ok_and(|decl| decl.id == drive))
        .ok_or_else(|| format!("This device does not keep the drive {drive}."))?;
    let segments = browse::plain_segments(path).map_err(|refusal| refusal.to_string())?;
    match browse::resolve(&profile.local_path, path) {
        Ok(Some(found)) if found.is_file() => {}
        Ok(_) => return Err(format!("{path} is not a file on {drive}.")),
        Err(refusal) => return Err(refusal.to_string()),
    }
    let parts: Option<Vec<&str>> = segments.iter().map(|segment| segment.to_str()).collect();
    let parts = parts.ok_or_else(|| format!("{path} is not a file on {drive}."))?;
    if let Some(subfolder) = vault {
        let folder = browse::plain_segments(subfolder.trim_matches('/'))
            .map_err(|refusal| refusal.to_string())?;
        let folder: Option<Vec<&str>> = folder.iter().map(|segment| segment.to_str()).collect();
        if let Some(folder) = folder {
            if parts.len() > folder.len() && parts[..folder.len()] == folder[..] {
                return Ok(Located::Note {
                    vault_id: profile.id.clone(),
                    note_path: parts[folder.len()..].join("/"),
                });
            }
        }
    }
    Ok(Located::File {
        profile_id: profile.id.clone(),
        relative_path: parts.join("/"),
    })
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
        write("media/talk.txt", "a talk\n");
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

    #[test]
    fn a_surface_path_is_resolved_by_browse_resolve() {
        let (_root, profile) = folder(true);
        let folders = [(profile.clone(), Some("notes".to_owned()))];
        // A note of the vault opens in the editor, named by the vault.
        assert_eq!(
            locate(&folders, "tgdrive", "notes/plans/q3.md"),
            Ok(Located::Note {
                vault_id: "p1".to_owned(),
                note_path: "plans/q3.md".to_owned(),
            })
        );
        // A file outside every vault opens in the Files preview.
        assert_eq!(
            locate(&folders, "tgdrive", "media/talk.txt"),
            Ok(Located::File {
                profile_id: "p1".to_owned(),
                relative_path: "media/talk.txt".to_owned(),
            })
        );
        // A path escaping the drive is refused with browse's own sentence.
        let escape = locate(&folders, "tgdrive", "../outside.md").expect_err("refused");
        let browse_says = browse::plain_segments("../outside.md")
            .expect_err("browse refuses it")
            .to_string();
        assert_eq!(escape, browse_says);
        // A drive this device does not keep, a file that is not there.
        assert!(locate(&folders, "neuradrive", "notes/plans/q3.md").is_err());
        assert!(locate(&folders, "tgdrive", "notes/plans/q4.md").is_err());
        // With no vault, every file is a file.
        assert!(matches!(
            locate(&[(profile, None)], "tgdrive", "notes/plans/q3.md"),
            Ok(Located::File { .. })
        ));
    }
}

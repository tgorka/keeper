//! Removing a recording: finding the one folder a session lives in, measuring
//! it, and deleting it — never anything outside a recordings root keeper
//! follows, and never a root itself.
//!
//! The folder comes from the recordings index, as every other surface that
//! acts on a session finds it: the row names its root and its root-relative
//! path, and the root must be one the archive follows right now. The shell
//! decides when a removal may run (nothing records into the folder, no
//! transcription reads it) and what happens to the notes; this module decides
//! which directory it is and removes it.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::archive::recordings::fallback_session_id;
use crate::archive::recordings_fts::recordings_indexed;
use crate::archive::KnownRoot;
use crate::recording::SessionManifest;

/// What a session folder holds: every regular file under it and their bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderSize {
    pub bytes: u64,
    pub files: u64,
}

/// Why a recording cannot be removed, as the sentence a person reads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemovalRefusal {
    #[error("keeper does not know where this recording is, so there is nothing it can remove.")]
    Unknown,
    #[error("This recording is in a recordings folder keeper does not follow right now, so keeper will not delete it.")]
    RootNotFollowed,
    #[error("This recording's folder is not on this Mac, so there is nothing here to remove.")]
    Missing,
    #[error("This recording's folder is not inside a recordings folder keeper follows, so keeper will not delete it.")]
    OutsideRoots,
    #[error("This recording's folder does not hold the recording keeper was asked to remove, so keeper will not delete it.")]
    NotThisSession,
    #[error("The recordings index could not be read: {0}")]
    Index(String),
    #[error("keeper could not delete {folder}: {reason}")]
    Failed { folder: String, reason: String },
}

/// A session's folder, resolved and checked: inside `root`, and not `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFolder {
    /// The folder with every symlink resolved: what is measured and deleted.
    pub folder: PathBuf,
    /// The recordings root it is in, as the archive follows it.
    pub root: KnownRoot,
    /// The folder relative to `root`, `/`-joined, as the index stores it.
    pub relative_path: String,
    /// The folder as `root` spells it, every symlink left in: the path the
    /// drive's sync knows it by.
    pub path: PathBuf,
    /// How far its bytes have travelled, as the index says: `local`,
    /// `committed`, `pushed` or `verified`.
    pub durability: String,
}

/// The folder of `session_id`, under the root its row names among `roots`.
///
/// Refused when no row knows the session, when its root is not one of
/// `roots`, when the stored path has an empty, `.` or `..` component, when
/// the folder is not on disk, when — every symlink resolved on both sides —
/// it is not strictly inside that root (a symlinked folder may point
/// anywhere), and when its `manifest.json` does not name this session: a
/// stale row must never aim a deletion at whatever now sits at its path.
pub fn session_folder(
    conn: &Connection,
    session_id: &str,
    roots: &[KnownRoot],
) -> Result<SessionFolder, RemovalRefusal> {
    let index = |error: rusqlite::Error| RemovalRefusal::Index(error.to_string());
    if !recordings_indexed(conn).map_err(|error| RemovalRefusal::Index(error.to_string()))? {
        return Err(RemovalRefusal::Unknown);
    }
    let (relative_path, root_kind, profile_id, durability): (
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT relative_path, root_kind, profile_id, durability FROM recordings \
             WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(index)?
        .ok_or(RemovalRefusal::Unknown)?;
    let root = roots
        .iter()
        .find(|root| root.root_kind == root_kind && root.profile_id == profile_id)
        .ok_or(RemovalRefusal::RootNotFollowed)?;
    let mut path = root.root.clone();
    for component in relative_path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(RemovalRefusal::OutsideRoots);
        }
        path.push(component);
    }
    let folder = path.canonicalize().map_err(|_| RemovalRefusal::Missing)?;
    let canonical_root = root
        .root
        .canonicalize()
        .map_err(|_| RemovalRefusal::Missing)?;
    if folder == canonical_root || !folder.starts_with(&canonical_root) || !folder.is_dir() {
        return Err(RemovalRefusal::OutsideRoots);
    }
    let named = SessionManifest::load(&folder)
        .map_err(|_| RemovalRefusal::NotThisSession)?
        .meta
        .and_then(|meta| meta.session_id)
        .unwrap_or_else(|| fallback_session_id(&relative_path));
    if named != session_id {
        return Err(RemovalRefusal::NotThisSession);
    }
    Ok(SessionFolder {
        folder,
        root: root.clone(),
        relative_path,
        path,
        durability,
    })
}

/// Every regular file under `folder` and their bytes. Symlinks are counted
/// as nothing and never followed: removing the folder removes the link, not
/// what it points at.
pub fn measure(folder: &Path) -> FolderSize {
    let mut size = FolderSize::default();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                size.files += 1;
                size.bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            }
        }
    }
    size
}

/// Delete a resolved session folder and everything in it; answers what it
/// held.
pub fn remove(session: &SessionFolder) -> Result<FolderSize, RemovalRefusal> {
    let size = measure(&session.folder);
    std::fs::remove_dir_all(&session.folder).map_err(|error| RemovalRefusal::Failed {
        folder: session.relative_path.clone(),
        reason: error.to_string(),
    })?;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::recordings::{ensure_recordings_schema, upsert_recording};
    use crate::archive::RecordingRow;
    use crate::recording::{CaptureTarget, SessionDevices, SessionMeta};

    fn scratch(tag: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("keeper-removal-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn index(rows: &[(&str, &str, Option<&str>)]) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory archive");
        ensure_recordings_schema(&conn).expect("schema");
        crate::archive::recordings_fts::ensure_recordings_fts(&conn).expect("fts");
        for (session_id, relative_path, profile_id) in rows {
            let row = RecordingRow {
                session_id: (*session_id).to_owned(),
                device_id: None,
                relative_path: (*relative_path).to_owned(),
                root_kind: if profile_id.is_some() {
                    "profile"
                } else {
                    "folder"
                }
                .to_owned(),
                profile_id: profile_id.map(str::to_owned),
                started_ts: None,
                ended_ts: None,
                title: None,
                participants_json: None,
                note: None,
                tags_json: None,
                custom_json: None,
                codec: None,
                width: None,
                height: None,
                fps: None,
                durability: "pushed".to_owned(),
                manifest_version: 1,
            };
            upsert_recording(&conn, &row).expect("row");
        }
        conn
    }

    fn drive(root: &Path) -> KnownRoot {
        KnownRoot {
            root: root.to_path_buf(),
            root_kind: "profile".to_owned(),
            profile_id: Some("01DRIVE".to_owned()),
        }
    }

    /// A finished session on disk: its manifest naming `session_id`, a
    /// segment, a transcript and a nested log.
    fn session(root: &Path, relative: &str, session_id: &str) -> PathBuf {
        let folder = root.join(relative);
        std::fs::create_dir_all(folder.parent().expect("parent")).expect("parents");
        SessionManifest::create_with_meta(
            folder.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(SessionMeta {
                session_id: Some(session_id.to_owned()),
                ..SessionMeta::default()
            }),
            None,
        )
        .expect("manifest");
        std::fs::create_dir_all(folder.join("nested")).expect("nested");
        std::fs::write(folder.join("screen-0000.mov"), b"12345").expect("segment");
        std::fs::write(folder.join("transcript.json"), b"{}").expect("transcript");
        std::fs::write(folder.join("nested/events.log"), b"abc").expect("log");
        folder
    }

    #[test]
    fn a_session_folder_is_measured_deleted_and_nothing_beside_it_goes() {
        let base = scratch("deletes");
        let root = base.join("recordings");
        let folder = session(&root, "2026/standup", "S1");
        let beside = session(&root, "2026/retro", "S2");
        let manifest_bytes = std::fs::metadata(folder.join("manifest.json"))
            .expect("manifest")
            .len();
        let conn = index(&[("S1", "2026/standup", Some("01DRIVE"))]);

        let found = session_folder(&conn, "S1", &[drive(&root)]).expect("resolved");
        assert_eq!(found.relative_path, "2026/standup");
        assert_eq!(found.path, root.join("2026").join("standup"));
        assert_eq!(found.durability, "pushed");
        let size = remove(&found).expect("removed");

        assert_eq!(
            size,
            FolderSize {
                bytes: 10 + manifest_bytes,
                files: 4
            }
        );
        assert!(!root.join("2026/standup").exists());
        assert!(
            beside.join("screen-0000.mov").is_file(),
            "the other session stays"
        );
        assert!(root.join("2026").is_dir(), "and so does the year folder");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_folder_outside_every_followed_root_is_refused() {
        let base = scratch("outside");
        let root = base.join("recordings");
        std::fs::create_dir_all(root.join("2026")).expect("root");
        let elsewhere = session(&base, "precious", "LINKED");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, root.join("2026/linked")).expect("symlink");
        session(&root, "2026/inside", "DOT");
        let conn = index(&[
            ("ROOT", "", Some("01DRIVE")),
            ("LINKED", "2026/linked", Some("01DRIVE")),
            ("CLIMB", "2026/../../precious", Some("01DRIVE")),
            ("DOT", "2026/./inside", Some("01DRIVE")),
            ("OTHER", "2026/x", Some("01OTHER")),
        ]);
        let roots = [drive(&root)];

        assert_eq!(
            session_folder(&conn, "ROOT", &roots),
            Err(RemovalRefusal::OutsideRoots),
            "the root itself is never a session"
        );
        #[cfg(unix)]
        assert_eq!(
            session_folder(&conn, "LINKED", &roots),
            Err(RemovalRefusal::OutsideRoots),
            "a symlink out of the root is resolved before it is judged"
        );
        assert_eq!(
            session_folder(&conn, "CLIMB", &roots),
            Err(RemovalRefusal::OutsideRoots),
            "a `..` in the stored path is refused, never dropped or walked"
        );
        assert_eq!(
            session_folder(&conn, "DOT", &roots),
            Err(RemovalRefusal::OutsideRoots),
            "and so is a `.`"
        );
        assert_eq!(
            session_folder(&conn, "OTHER", &roots),
            Err(RemovalRefusal::RootNotFollowed)
        );
        assert_eq!(
            session_folder(&conn, "NOBODY", &roots),
            Err(RemovalRefusal::Unknown)
        );
        assert!(elsewhere.join("screen-0000.mov").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_folder_whose_manifest_names_another_session_or_none_is_refused() {
        let base = scratch("manifest");
        let root = base.join("recordings");
        session(&root, "2026/standup", "SOMEONE-ELSE");
        std::fs::create_dir_all(root.join("2026/bare")).expect("no manifest");
        let conn = index(&[
            ("S1", "2026/standup", Some("01DRIVE")),
            ("S2", "2026/bare", Some("01DRIVE")),
        ]);
        let roots = [drive(&root)];

        assert_eq!(
            session_folder(&conn, "S1", &roots),
            Err(RemovalRefusal::NotThisSession),
            "a stale row aims at a folder that is another recording now"
        );
        assert_eq!(
            session_folder(&conn, "S2", &roots),
            Err(RemovalRefusal::NotThisSession),
            "a folder with no manifest is not a session"
        );
        assert!(root.join("2026/standup/screen-0000.mov").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }
}

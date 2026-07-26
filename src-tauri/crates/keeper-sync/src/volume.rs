//! Removable-media identity: absence is never deletion (Stories 27.3 / 27.4,
//! AD-48).
//!
//! This module exists to prevent exactly one catastrophe. A pendrive is
//! unplugged, its mount point reverts to an empty directory, and the engine
//! reads that as "the user deleted 40 GB" — then stages it, commits it and
//! pushes it. A path cannot distinguish "empty because unmounted" from "empty
//! because deleted", so identity is bound to the **volume** instead: a
//! `.keeper-sync/volume.json` marker written at the mount root. Marker present
//! ⇒ the media is attached and a missing file is a real deletion. Marker gone
//! ⇒ the media is gone, the profile pauses, and nothing is staged.
//!
//! Binding to the marker rather than to the mountpoint is also what survives a
//! re-mount: the same stick appearing at `/media/dev/KEEPER1` instead of
//! `/run/media/dev/KEEPER` is still the same volume, because
//! [`find_mount_root`] walks up to the marker rather than comparing paths.
//!
//! Since the marker is the only thing standing between a user and that
//! catastrophe, every operation here is biased toward *not* destroying it:
//!
//! * writes are staged into a sibling temp file and published by an atomic
//!   rename (the `SessionManifest::write` precedent), so a torn write can never
//!   leave half a marker behind;
//! * a marker carrying a **newer** schema version is left untouched, never
//!   rewritten by an older build;
//! * a marker that will not parse is an **error**, never an overwrite;
//! * a marker naming a different volume is [`VolumeStatus::Foreign`], never a
//!   silent adoption.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, SyncError};

/// Directory holding the marker at a volume's mount root.
pub const MARKER_DIR: &str = ".keeper-sync";

/// The marker file inside [`MARKER_DIR`].
pub const MARKER_FILE: &str = "volume.json";

/// `volume.json` schema version.
///
/// Bumped only for a change an older build cannot tolerate. Purely additive
/// fields do **not** bump it — they carry `#[serde(default)]` instead, so a
/// marker written by any build still parses in any other. This is the
/// `MANIFEST_VERSION` discipline from the recording manifest, for the same
/// reason: the file outlives the binary that wrote it, and on removable media
/// it is routinely read by a *different* machine running a different version.
pub const MARKER_VERSION: u32 = 1;

/// The `.keeper-sync/volume.json` path under `mount_root`.
pub fn marker_path(mount_root: &Path) -> PathBuf {
    mount_root.join(MARKER_DIR).join(MARKER_FILE)
}

/// The identity of one removable volume, as persisted at its mount root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeMarker {
    /// Schema version of this marker ([`MARKER_VERSION`]).
    ///
    /// Required, not defaulted: it is the anchor the compatibility rule reads,
    /// and a file without it is not a marker this code may reason about.
    pub schema_version: u32,
    /// Stable ULID minted once, when the volume is first adopted. This — not
    /// the mountpoint, not the filesystem UUID, not the label — is the volume's
    /// identity, because it is the only one the user cannot silently change.
    ///
    /// Required for the same reason as `schema_version`: defaulting it to an
    /// empty string would make every unidentifiable volume look like the same
    /// volume, which is precisely the confusion AD-48 exists to prevent.
    pub volume_id: String,
    /// Human label for the volume, shown in the tray and used in provenance
    /// (`Keeper-Origin`). Cosmetic — never matched on.
    #[serde(default)]
    pub label: String,
    /// Profiles that have adopted this volume. Bookkeeping only: it lets the
    /// UI say which profiles a re-attached stick just woke up.
    #[serde(default)]
    pub profile_ids: Vec<String>,
    /// Wall-clock ms when the marker was minted.
    #[serde(default)]
    pub created_ms: i64,
}

/// What a scan found at a profile's mount root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeStatus {
    /// The expected volume is attached.
    Present { marker: VolumeMarker },
    /// No marker: the media is not attached. This maps to
    /// [`crate::profile::ProfileState::MediaAbsent`] — a first-class state, not
    /// an error — and **no deletion may be staged, committed or pushed while
    /// it holds** (AD-48).
    Absent,
    /// A marker is here, but it belongs to a *different* volume than the
    /// profile expects: a second stick mounted at the path the first one used.
    ///
    /// Never adopted silently. Treating this as `Present` would sync one
    /// volume's contents into another volume's history; treating it as `Absent`
    /// would be equally wrong, because something genuinely is mounted here and
    /// its files are not the profile's deletions.
    Foreign { found_id: String },
}

impl VolumeMarker {
    /// Mint a marker for a volume that has none yet.
    ///
    /// `now_ms` is injected rather than read from the clock so callers stay on
    /// the [`crate::platform::SyncPlatform::now_ms`] port, matching every other
    /// timestamped API in this crate (`db::enqueue`, `backoff::not_before_ms`).
    pub fn new(label: impl Into<String>, now_ms: i64) -> Self {
        Self {
            schema_version: MARKER_VERSION,
            volume_id: ulid::Ulid::new().to_string(),
            label: label.into(),
            profile_ids: Vec::new(),
            created_ms: now_ms,
        }
    }

    /// Whether a marker file exists at `mount_root`.
    ///
    /// Deliberately a file-existence test and **not** a parse: the question
    /// this answers is "is the media attached", and a marker that is corrupt,
    /// unreadable or written by a future build still proves something is
    /// mounted here. Reporting `false` for any of those would hand the engine
    /// the one answer that licenses deleting the user's files.
    pub fn is_present(mount_root: &Path) -> bool {
        marker_path(mount_root).is_file()
    }

    /// Read the marker at `mount_root`.
    ///
    /// `Ok(None)` means the media is not attached (or was never adopted), which
    /// is an ordinary state. A marker that exists but cannot be read or parsed
    /// is an **error**: silently treating it as absent would let the next write
    /// clobber a file we failed to understand.
    pub fn read(mount_root: &Path) -> Result<Option<Self>> {
        let path = marker_path(mount_root);
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(SyncError::io("read volume marker", &path, err)),
        };
        let marker: Self = serde_json::from_slice(&raw).map_err(|err| {
            SyncError::Config(format!(
                "volume marker {} is corrupt ({err}); \
                 inspect it and delete it to let keeper re-adopt this volume",
                path.display()
            ))
        })?;
        Ok(Some(marker))
    }

    /// Publish `marker` at `mount_root` atomically.
    ///
    /// Staged into a unique sibling temp file in the marker directory, flushed,
    /// then renamed over `volume.json` — a same-directory rename, so a reader
    /// (including one on another machine) sees the old marker or the new one,
    /// never a torn one, and a failure anywhere leaves the previous marker
    /// intact. The temp name is unique rather than fixed because the desktop
    /// app and `keeper-syncd` may both be running against the same volume
    /// (AD-52), and a shared fixed name would let one publish the other's
    /// half-written bytes.
    ///
    /// Refuses to overwrite a marker this build does not understand — a newer
    /// schema version, or one that will not parse. Downgrading a volume by
    /// running an older keeper against it is not a recoverable mistake.
    pub fn write(mount_root: &Path, marker: &Self) -> Result<()> {
        // `read` propagates a parse failure, so a corrupt marker is refused
        // here too rather than being quietly replaced.
        if let Some(existing) = Self::read(mount_root)? {
            if existing.schema_version > MARKER_VERSION {
                return Err(SyncError::Config(format!(
                    "volume marker {} has schema version {} but this build understands {}; \
                     refusing to rewrite it — upgrade keeper instead",
                    marker_path(mount_root).display(),
                    existing.schema_version,
                    MARKER_VERSION
                )));
            }
        }

        let dir = mount_root.join(MARKER_DIR);
        std::fs::create_dir_all(&dir)
            .map_err(|err| SyncError::io("create volume marker directory", &dir, err))?;

        let json = serde_json::to_vec_pretty(marker).map_err(|err| {
            SyncError::Config(format!("volume marker is not serializable: {err}"))
        })?;

        let mut tmp = tempfile::NamedTempFile::new_in(&dir)
            .map_err(|err| SyncError::io("create volume marker temp file", &dir, err))?;
        tmp.write_all(&json)
            .map_err(|err| SyncError::io("write volume marker temp file", &dir, err))?;
        // A removable volume is by definition the device most likely to be
        // yanked between the write and the rename; get the bytes down before
        // publishing them.
        tmp.as_file()
            .sync_all()
            .map_err(|err| SyncError::io("flush volume marker temp file", &dir, err))?;
        relax_marker_permissions(tmp.as_file())
            .map_err(|err| SyncError::io("set volume marker permissions", &dir, err))?;

        let path = marker_path(mount_root);
        // `NamedTempFile` deletes the staged file when the `PersistError` it
        // hands back is dropped, so a failed rename litters nothing.
        tmp.persist(&path)
            .map_err(|err| SyncError::io("publish volume marker", &path, err.error))?;
        Ok(())
    }

    /// Adopt the volume at `mount_root` for `profile_id`, creating the marker
    /// if there is none.
    ///
    /// Adopt-and-extend, never replace: an existing marker keeps its
    /// `volume_id`, its `created_ms` and any other profile already registered
    /// on it, because a second profile pointing at the same stick must not
    /// re-identify it out from under the first.
    ///
    /// A marker written by a newer build is returned **unchanged and unwritten**
    /// (with a warning). The volume is still perfectly usable — its identity is
    /// all the presence check needs — and losing a field we do not understand
    /// costs more than skipping one bookkeeping entry. This mirrors the
    /// recording recovery pass, which likewise skips a newer manifest rather
    /// than rewriting it.
    pub fn ensure(mount_root: &Path, label: &str, profile_id: &str, now_ms: i64) -> Result<Self> {
        let Some(mut marker) = Self::read(mount_root)? else {
            let mut minted = Self::new(label, now_ms);
            minted.profile_ids.push(profile_id.to_owned());
            Self::write(mount_root, &minted)?;
            return Ok(minted);
        };

        if marker.schema_version > MARKER_VERSION {
            tracing::warn!(
                volume_id = %marker.volume_id,
                schema_version = u64::from(marker.schema_version),
                understood = u64::from(MARKER_VERSION),
                "volume marker was written by a newer keeper; adopting it read-only"
            );
            return Ok(marker);
        }

        let mut dirty = false;
        if !marker.profile_ids.iter().any(|id| id == profile_id) {
            marker.profile_ids.push(profile_id.to_owned());
            dirty = true;
        }
        // Fill in a missing label, but never rename a volume another profile
        // already named — the label is the volume's, not the profile's.
        if marker.label.is_empty() && !label.is_empty() {
            marker.label = label.to_owned();
            dirty = true;
        }
        if dirty {
            Self::write(mount_root, &marker)?;
        }
        Ok(marker)
    }
}

/// Walk up from `path` to the nearest ancestor carrying a volume marker.
///
/// Upward rather than "the path the profile was configured with", because a
/// stick re-mounted at a different mountpoint is still the same volume: the
/// marker travels with the filesystem, the mountpoint does not. Returns `None`
/// when nothing on the way to the root is marked, which for a `removable`
/// profile means the media is not attached.
pub fn find_mount_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        // `Path::ancestors` on a relative path ends at `""`, whose marker probe
        // would resolve against the process CWD — an unrelated volume.
        .filter(|candidate| !candidate.as_os_str().is_empty())
        .find(|candidate| VolumeMarker::is_present(candidate))
        .map(Path::to_path_buf)
}

/// Classify the volume at `mount_root` against the id a profile expects.
///
/// `expected_volume_id` is `None` for a profile that has not been bound to a
/// volume yet, in which case any marker found is [`VolumeStatus::Present`].
pub fn status(mount_root: &Path, expected_volume_id: Option<&str>) -> Result<VolumeStatus> {
    let Some(marker) = VolumeMarker::read(mount_root)? else {
        return Ok(VolumeStatus::Absent);
    };
    match expected_volume_id {
        Some(expected) if expected != marker.volume_id => Ok(VolumeStatus::Foreign {
            found_id: marker.volume_id,
        }),
        _ => Ok(VolumeStatus::Present { marker }),
    }
}

/// The per-scan question AD-48 asks: given a profile's local path, is its
/// volume attached?
///
/// Locates the mount root by marker rather than by path ([`find_mount_root`]),
/// so a re-mount elsewhere still resolves. No marker anywhere above the path is
/// [`VolumeStatus::Absent`] — the media is gone, not the files.
pub fn scan(local_path: &Path, expected_volume_id: Option<&str>) -> Result<VolumeStatus> {
    match find_mount_root(local_path) {
        Some(root) => status(&root, expected_volume_id),
        None => Ok(VolumeStatus::Absent),
    }
}

/// Make a freshly staged marker world-readable.
///
/// `tempfile` creates at `0600`. A pendrive formatted ext4 and carried between
/// machines is routinely read under a *different* uid (the research notes call
/// this out as the normal case for removable media), and a `0600` marker would
/// make every one of those reads fail — turning a healthy volume into a stream
/// of errors. The marker holds no secret: a ULID, a label and profile ids.
#[cfg(unix)]
fn relax_marker_permissions(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))
}

#[cfg(not(unix))]
fn relax_marker_permissions(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn temp_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    /// Write raw bytes as the marker, bypassing every guard in `write`.
    fn plant(mount_root: &Path, bytes: &str) {
        let dir = mount_root.join(MARKER_DIR);
        std::fs::create_dir_all(&dir).expect("marker dir");
        std::fs::write(dir.join(MARKER_FILE), bytes).expect("plant marker");
    }

    #[test]
    fn write_then_read_round_trips_the_marker() {
        let root = temp_root();
        let mut marker = VolumeMarker::new("Field Drive", NOW);
        marker.profile_ids.push("profile-a".to_owned());

        VolumeMarker::write(root.path(), &marker).expect("write");
        let read = VolumeMarker::read(root.path())
            .expect("read")
            .expect("marker must exist after a write");

        assert_eq!(read, marker);
        assert!(VolumeMarker::is_present(root.path()));
    }

    #[test]
    fn an_unmarked_root_is_absent_rather_than_an_error() {
        let root = temp_root();
        assert!(!VolumeMarker::is_present(root.path()));
        assert_eq!(VolumeMarker::read(root.path()).expect("read"), None);
        assert_eq!(
            status(root.path(), Some("01VOLUME")).expect("status"),
            VolumeStatus::Absent
        );
    }

    #[test]
    fn a_newer_schema_version_is_preserved_verbatim_rather_than_rewritten() {
        let root = temp_root();
        let planted = format!(
            r#"{{"schemaVersion":{},"volumeId":"01FUTURE","label":"Tomorrow",
                "profileIds":["old-profile"],"createdMs":7,"unknownFutureField":true}}"#,
            MARKER_VERSION + 1
        );
        plant(root.path(), &planted);
        let before = std::fs::read(marker_path(root.path())).expect("before");

        let adopted =
            VolumeMarker::ensure(root.path(), "Renamed", "new-profile", NOW).expect("ensure");

        assert_eq!(adopted.schema_version, MARKER_VERSION + 1);
        assert_eq!(adopted.volume_id, "01FUTURE");
        // The whole point: the newer marker's bytes — including the field this
        // build cannot even name — survive untouched.
        assert_eq!(
            std::fs::read(marker_path(root.path())).expect("after"),
            before
        );
        assert!(!adopted.profile_ids.iter().any(|id| id == "new-profile"));
    }

    #[test]
    fn write_refuses_to_downgrade_a_newer_marker() {
        let root = temp_root();
        plant(
            root.path(),
            &format!(
                r#"{{"schemaVersion":{},"volumeId":"01FUTURE"}}"#,
                MARKER_VERSION + 1
            ),
        );
        let before = std::fs::read(marker_path(root.path())).expect("before");

        let err = VolumeMarker::write(root.path(), &VolumeMarker::new("Mine", NOW))
            .expect_err("an older build must not overwrite a newer marker");

        assert_eq!(err.code(), "config");
        assert_eq!(
            std::fs::read(marker_path(root.path())).expect("after"),
            before
        );
    }

    #[test]
    fn ensure_adopts_an_existing_marker_and_appends_each_profile_once() {
        let root = temp_root();

        let first = VolumeMarker::ensure(root.path(), "Field Drive", "profile-a", NOW)
            .expect("first ensure");
        let again = VolumeMarker::ensure(root.path(), "Field Drive", "profile-a", NOW + 1)
            .expect("idempotent ensure");
        let second = VolumeMarker::ensure(root.path(), "Renamed", "profile-b", NOW + 2)
            .expect("second profile");

        // Identity is minted once and never re-minted by a later adopter.
        assert_eq!(again.volume_id, first.volume_id);
        assert_eq!(second.volume_id, first.volume_id);
        assert_eq!(second.created_ms, first.created_ms);
        // A second adopter must not rename the volume.
        assert_eq!(second.label, "Field Drive");
        assert_eq!(again.profile_ids, vec!["profile-a".to_owned()]);
        assert_eq!(
            second.profile_ids,
            vec!["profile-a".to_owned(), "profile-b".to_owned()]
        );
    }

    #[test]
    fn find_mount_root_walks_up_from_a_deep_subdirectory() {
        let root = temp_root();
        VolumeMarker::write(root.path(), &VolumeMarker::new("Field Drive", NOW)).expect("write");
        let deep = root.path().join("projects/keeper/assets/raw");
        std::fs::create_dir_all(&deep).expect("deep dirs");

        assert_eq!(find_mount_root(&deep).as_deref(), Some(root.path()));
        // The nearest marked ancestor wins, so a marked subtree inside a marked
        // volume resolves to the inner one.
        assert_eq!(find_mount_root(root.path()).as_deref(), Some(root.path()));
    }

    #[test]
    fn find_mount_root_is_none_when_nothing_above_is_marked() {
        let root = temp_root();
        let deep = root.path().join("a/b/c");
        std::fs::create_dir_all(&deep).expect("deep dirs");

        assert_eq!(find_mount_root(&deep), None);
        // And that is reported as absent media, never as an error.
        assert_eq!(
            scan(&deep, Some("01VOLUME")).expect("scan"),
            VolumeStatus::Absent
        );
    }

    #[test]
    fn scan_recognizes_the_same_volume_under_a_different_mountpoint() {
        let first_mount = temp_root();
        let marker = VolumeMarker::ensure(first_mount.path(), "Field Drive", "profile-a", NOW)
            .expect("adopt");
        let data = first_mount.path().join("data");
        std::fs::create_dir_all(&data).expect("data dir");

        // Simulate a re-mount: the same marker directory, a different path.
        let second_mount = temp_root();
        std::fs::create_dir_all(second_mount.path().join(MARKER_DIR)).expect("marker dir");
        std::fs::copy(
            marker_path(first_mount.path()),
            marker_path(second_mount.path()),
        )
        .expect("carry the marker across");
        let moved_data = second_mount.path().join("data");
        std::fs::create_dir_all(&moved_data).expect("data dir");

        let expected_id = marker.volume_id.clone();
        assert_eq!(
            scan(&moved_data, Some(&expected_id)).expect("scan"),
            VolumeStatus::Present { marker }
        );
        // The original mountpoint still resolves too: nothing about the
        // identity was tied to where it happened to be mounted.
        assert_eq!(
            scan(&data, Some(&expected_id)).expect("scan"),
            VolumeStatus::Present {
                marker: VolumeMarker::read(first_mount.path())
                    .expect("read")
                    .expect("present")
            }
        );
    }

    #[test]
    fn a_different_volume_at_the_same_path_is_foreign_not_present() {
        let root = temp_root();
        let marker = VolumeMarker::ensure(root.path(), "Someone Else's Stick", "profile-a", NOW)
            .expect("adopt");

        let verdict = scan(root.path(), Some("01EXPECTEDVOLUME")).expect("scan");

        assert_eq!(
            verdict,
            VolumeStatus::Foreign {
                found_id: marker.volume_id
            }
        );
    }

    #[test]
    fn an_unbound_profile_accepts_whatever_volume_is_mounted() {
        let root = temp_root();
        VolumeMarker::ensure(root.path(), "Fresh Stick", "profile-a", NOW).expect("adopt");

        assert!(matches!(
            scan(root.path(), None).expect("scan"),
            VolumeStatus::Present { .. }
        ));
    }

    #[test]
    fn a_corrupt_marker_is_an_error_and_is_never_overwritten() {
        let root = temp_root();
        plant(root.path(), "{ this is not json");
        let before = std::fs::read(marker_path(root.path())).expect("before");

        let read_err = VolumeMarker::read(root.path()).expect_err("corrupt marker must not parse");
        assert_eq!(read_err.code(), "config");

        let ensure_err = VolumeMarker::ensure(root.path(), "Field Drive", "profile-a", NOW)
            .expect_err("ensure must not adopt a corrupt marker");
        assert_eq!(ensure_err.code(), "config");

        assert_eq!(
            std::fs::read(marker_path(root.path())).expect("after"),
            before,
            "a corrupt marker must survive for a human to inspect"
        );
        // Corrupt still means *something is mounted here*, so presence — the
        // signal that gates deletion — must stay true.
        assert!(VolumeMarker::is_present(root.path()));
    }

    #[test]
    fn an_older_marker_missing_added_fields_still_parses() {
        let root = temp_root();
        plant(root.path(), r#"{"schemaVersion":1,"volumeId":"01LEGACY"}"#);

        let marker = VolumeMarker::read(root.path())
            .expect("an older marker must parse")
            .expect("present");

        assert_eq!(marker.volume_id, "01LEGACY");
        assert_eq!(marker.label, "");
        assert!(marker.profile_ids.is_empty());
        assert_eq!(marker.created_ms, 0);
    }

    #[test]
    fn a_marker_without_a_volume_id_is_rejected_rather_than_defaulted() {
        let root = temp_root();
        // Defaulting a missing id to "" would make every unidentifiable volume
        // compare equal to every other one.
        plant(root.path(), r#"{"schemaVersion":1,"label":"No Id"}"#);

        assert!(VolumeMarker::read(root.path()).is_err());
    }
}

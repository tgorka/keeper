//! The log file both hosts write, and the filter both hosts start from
//! (Epic 70, Story 70.7, AD-234).
//!
//! Two facts from hesperia's `keeper.log` on 2026-09-08 decided this module's
//! shape. **1.9 M of its 4.35 M lines were two gitoxide targets** —
//! `gix_attributes::search::attributes` warning once per path per lookup about
//! one mis-stored `.gitattributes` pointer (1 314 669 lines) and
//! `gix_worktree_state::checkout::chunk` erroring once per path about one
//! checkout into a non-empty directory (588 408 lines) — and the file had
//! **no rotation at all**: 895 MB live beside a 1.33 GB sibling, every line
//! written whether or not the owner had opted into debug logging, with one
//! `open(2)`/`close(2)` pair per line.
//!
//! Here rather than in either host because both hosts write a log and neither
//! can compile on the other's machine: the shell crate is macOS-only, the
//! daemon is Linux, and a rotation written twice is a rotation tested once.
//! This crate is `tauri`-free and `keeper-core`-free (AD-40), so both can
//! link it, and a `tempdir` test on the Linux box proves the byte boundary
//! for the Mac.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Bytes a log file may reach before [`RotatingFile`] moves it aside.
///
/// 64 MiB. Keeper's own lines are one `status walk finished` per walk and one
/// anomaly per hour per profile — a few MB a month — so this bound is reached
/// only by a defect class of the kind above, and 64 MiB of it is more than
/// any bug report needs and less than any laptop minds. With two generations
/// kept the disk cost tops out at 192 MiB, against the 2.2 GB measured.
pub const LOG_ROTATE_BYTES: u64 = 64 * 1024 * 1024;

/// How many rotated generations survive beside the live file.
///
/// Two: `keeper.log.1` is the one that just filled and `keeper.log.2` the one
/// before it, so a problem reported a day after it happened is still on disk.
/// A third is deleted at the rotation that would create it.
pub const LOG_GENERATIONS: u32 = 2;

/// The per-target directives every default filter carries.
///
/// The two targets that wrote 1.9 M lines are held to `error` (their `WARN`
/// is per path per lookup and says the same thing every time; an `ERROR` is
/// still one line a person needs), and `gix_dir`, the directory walker, to
/// `warn` — it emits an `INFO` per pruned directory on a walk that visits
/// 155 626 entries. Keeper's own targets are untouched, and `RUST_LOG`, when
/// set, replaces the whole string: a person debugging gix wants gix.
pub const LOG_TARGET_DIRECTIVES: &str =
    "gix_attributes=error,gix_worktree_state=error,gix_dir=warn";

/// The default `EnvFilter` directive for a host whose base level is `level`.
///
/// `RUST_LOG` is still consulted first by every caller and wins whole; this
/// is only what a host starts from when nobody said otherwise. Keeper's own
/// crates read at `level`; the gitoxide targets above are held down.
pub fn default_filter(level: &str) -> String {
    format!("{level},{LOG_TARGET_DIRECTIVES}")
}

/// One open handle on a log file that rotates itself past
/// [`LOG_ROTATE_BYTES`].
///
/// Held open across writes — `O_APPEND`, one `write_all` per event — where
/// the shell used to open and close the file per line. The size is tracked
/// in memory from the position the file was opened at plus what this handle
/// has written, so a write costs no `fstat`; a second writer on the same
/// file (the app and the daemon share nothing here, but a person's `tee`
/// might) merely rotates a little late.
///
/// # Rotation is delete, rename, rename — in that order
///
/// `keeper.log.2` is removed, `.1` becomes `.2`, the live file becomes `.1`
/// and a fresh live file is opened. The delete comes first because a rename
/// onto an existing name replaces it on Unix and fails on Windows; deleting
/// makes the two agree. Each step is best-effort and the next runs
/// regardless: a rename that fails leaves a bigger file, never a lost one,
/// and a log that could not rotate is still a log. A rename within one
/// directory is atomic on every filesystem keeper ships to, so no reader
/// ever sees a half-moved file.
///
/// # Failure is silence, never a panic
///
/// This sits under `tracing`, which is under everything. A full disk, a
/// vanished directory or a permissions change must not take the process
/// with it, so every error here is swallowed and the next write tries again.
pub struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    rotate_at: u64,
}

impl RotatingFile {
    /// Open (creating if needed) the log at `path`, rotating past
    /// [`LOG_ROTATE_BYTES`].
    pub fn open(path: PathBuf) -> Self {
        Self::open_rotating_at(path, LOG_ROTATE_BYTES)
    }

    /// [`Self::open`] with the boundary chosen by the caller.
    ///
    /// Public so a host can test rotation without writing 64 MiB; production
    /// callers use [`Self::open`].
    pub fn open_rotating_at(path: PathBuf, rotate_at: u64) -> Self {
        let mut this = Self {
            path,
            file: None,
            written: 0,
            rotate_at,
        };
        this.reopen();
        this
    }

    /// Where the live file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `n`th rotated generation beside the live file: `keeper.log.1`.
    pub fn generation(path: &Path, n: u32) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    fn reopen(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = self
            .file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map_or(0, |meta| meta.len());
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = std::fs::remove_file(Self::generation(&self.path, LOG_GENERATIONS));
        for n in (1..LOG_GENERATIONS).rev() {
            let _ = std::fs::rename(
                Self::generation(&self.path, n),
                Self::generation(&self.path, n + 1),
            );
        }
        let _ = std::fs::rename(&self.path, Self::generation(&self.path, 1));
        self.reopen();
    }
}

impl Write for RotatingFile {
    /// Append `buf` whole, rotating first if the file would cross the
    /// boundary. `Ok(buf.len())` always, whatever the disk said — see the
    /// type's doc.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.file.is_none() {
            self.reopen();
        }
        if self.written > 0 && self.written.saturating_add(buf.len() as u64) > self.rotate_at {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut() {
            if file.write_all(buf).is_ok() {
                self.written = self.written.saturating_add(buf.len() as u64);
            } else {
                // Reopen on the next write: the descriptor may be gone with
                // its directory, and a fresh open is the only repair.
                self.file = None;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Past the boundary the live file is moved to `.1`, `.1` to `.2`, and
    /// the third generation is deleted — proven at a 1 000-byte boundary
    /// with 100-byte lines, so each generation is exactly ten lines and the
    /// arithmetic is the same as 64 MiB with less waiting.
    #[test]
    fn a_log_past_the_boundary_rotates_and_the_third_generation_is_deleted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("logs").join("keeper.log");
        let boundary = 1000u64;
        let mut log = RotatingFile::open_rotating_at(path.clone(), boundary);
        // Ten lines fill a generation exactly; the eleventh rotates. Each
        // batch is lettered so its journey through the generations can be
        // followed.
        let mut fill = |letter: u8| {
            for _ in 0..10 {
                log.write_all(&[letter; 100]).expect("infallible");
            }
        };

        fill(b'a');
        assert_eq!(std::fs::metadata(&path).expect("live").len(), 1000);
        assert!(
            !RotatingFile::generation(&path, 1).exists(),
            "nothing rotated yet"
        );

        fill(b'b');
        assert_eq!(
            std::fs::read(&path).expect("live"),
            vec![b'b'; 1000],
            "the first `b` crossed the boundary and opened a fresh file"
        );
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 1)).expect(".1"),
            vec![b'a'; 1000],
            "the full file moved aside whole"
        );

        fill(b'c');
        assert_eq!(std::fs::read(&path).expect("live"), vec![b'c'; 1000]);
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 1)).expect(".1"),
            vec![b'b'; 1000]
        );
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 2)).expect(".2"),
            vec![b'a'; 1000],
            "the oldest generation is the highest number"
        );

        fill(b'd');
        assert_eq!(std::fs::read(&path).expect("live"), vec![b'd'; 1000]);
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 1)).expect(".1"),
            vec![b'c'; 1000]
        );
        assert_eq!(
            std::fs::read(RotatingFile::generation(&path, 2)).expect(".2"),
            vec![b'b'; 1000]
        );
        assert!(
            !RotatingFile::generation(&path, 3).exists(),
            "the `a` generation was deleted rather than becoming `.3`"
        );
    }

    /// Reopening an existing file counts its bytes, so a restart does not
    /// reset the boundary and let the file grow to twice the bound.
    #[test]
    fn an_existing_file_is_measured_on_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("keeper.log");
        std::fs::write(&path, vec![b'x'; 900]).expect("seed");
        let mut log = RotatingFile::open_rotating_at(path.clone(), 1000);
        log.write_all(&[b'y'; 200]).expect("infallible");
        assert_eq!(
            std::fs::read(&path).expect("live"),
            vec![b'y'; 200],
            "the write that would cross the bound rotated the seeded bytes out"
        );
        assert_eq!(
            std::fs::metadata(RotatingFile::generation(&path, 1))
                .expect("rotated")
                .len(),
            900
        );
    }

    /// A vanished directory is not a panic and not a lost log: once the
    /// descriptor is dropped, the next write recreates the directory and the
    /// file. (Unix keeps an unlinked file's descriptor writable, so the drop
    /// is what a failed write does in production — see `Write::write`.)
    #[test]
    fn a_removed_directory_is_recreated_on_the_next_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("gone").join("keeper.log");
        let mut log = RotatingFile::open(path.clone());
        log.write_all(b"one\n").expect("infallible");
        std::fs::remove_dir_all(dir.path().join("gone")).expect("remove");
        log.file = None;
        log.write_all(b"two\n").expect("infallible");
        assert_eq!(std::fs::read(&path).expect("the log came back"), b"two\n");
    }

    /// The daemon's and the app's default filter holds the two chattering
    /// gitoxide targets down while keeper's own `INFO` still passes
    /// (Epic 70, F-db-8).
    #[test]
    fn the_default_filter_silences_gix_attributes_warnings_and_keeps_keepers_info() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let filter: tracing_subscriber::EnvFilter = default_filter("info")
            .parse()
            .expect("the default filter is a valid directive set");
        let registry = tracing_subscriber::registry().with(filter);

        fn metadata(target: &'static str, level: tracing::Level) -> tracing::Metadata<'static> {
            tracing::Metadata::new(
                "event",
                target,
                level,
                None,
                None,
                None,
                tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&CALLSITE)),
                tracing::metadata::Kind::EVENT,
            )
        }
        struct Callsite;
        impl tracing::callsite::Callsite for Callsite {
            fn set_interest(&self, _: tracing::subscriber::Interest) {}
            fn metadata(&self) -> &tracing::Metadata<'_> {
                unreachable!("only the identifier is used")
            }
        }
        static CALLSITE: Callsite = Callsite;

        let enabled = |target, level| {
            let meta = metadata(target, level);
            tracing::Subscriber::enabled(&registry, &meta)
        };
        assert!(
            !enabled("gix_attributes::search::attributes", tracing::Level::WARN),
            "the 1 314 669-line target is held to error"
        );
        assert!(enabled(
            "gix_attributes::search::attributes",
            tracing::Level::ERROR
        ));
        assert!(!enabled(
            "gix_worktree_state::checkout::chunk",
            tracing::Level::WARN
        ));
        assert!(!enabled("gix_dir::walk", tracing::Level::INFO));
        assert!(enabled("gix_dir::walk", tracing::Level::WARN));
        assert!(
            enabled("keeper_sync::engine", tracing::Level::INFO),
            "keeper's own lines are untouched"
        );
        assert!(!enabled("keeper_sync::engine", tracing::Level::DEBUG));
    }
}

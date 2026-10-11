//! The sessions runtime (AD-368, FR-778): the journaled executor, the zone
//! lock, the scan, the verbs and `session_write`, shared by the app and an
//! agent host.

pub mod exec;
pub mod lock;
pub mod scan;
pub mod verbs;
pub mod write;

use std::path::{Path, PathBuf};

/// Finish every interrupted plan in these zones, each under its lock — what a
/// host does at start, before its first verb. Returns the zones whose plan
/// could not be finished, with why; the rest are clean.
pub fn resume_all<'a>(
    zones: impl IntoIterator<Item = &'a Path>,
) -> Vec<(PathBuf, exec::ExecError)> {
    zones
        .into_iter()
        .filter_map(|zone| {
            exec::resume(zone)
                .err()
                .map(|error| (zone.to_path_buf(), error))
        })
        .collect()
}

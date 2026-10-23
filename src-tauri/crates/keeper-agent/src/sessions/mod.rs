//! The sessions runtime (AD-368, FR-778): the journaled executor, the zone
//! lock, the scan, the verbs and `session_write`, shared by the app and an
//! agent host.

pub mod board;
pub mod exec;
pub mod lock;
pub mod scan;
pub mod verbs;
pub mod write;

use std::path::{Path, PathBuf};

/// A fresh opaque id, in the shape every other keeper record uses.
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

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

/// Whether a registry going from `before` zones to `after` changes what a
/// phone may show (98.2 #6, R346): on a phone the sessions surface exists
/// while at least one zone is registered, so only the first zone arriving and
/// the last one leaving flip it — a second zone, or one of two leaving, does
/// not, and announcing it would make the webview ask for nothing.
pub fn zones_flip_the_surface(before: usize, after: usize) -> bool {
    (before == 0) != (after == 0)
}

#[cfg(test)]
mod tests {
    use super::zones_flip_the_surface;

    #[test]
    fn only_the_first_zone_and_the_last_one_flip_the_surface() {
        assert!(zones_flip_the_surface(0, 1), "the first zone arrives");
        assert!(
            zones_flip_the_surface(0, 3),
            "a restore lands several at once"
        );
        assert!(zones_flip_the_surface(1, 0), "the last zone leaves");
        assert!(zones_flip_the_surface(2, 0), "a removal takes both");
        assert!(
            !zones_flip_the_surface(1, 2),
            "a second zone changes nothing"
        );
        assert!(
            !zones_flip_the_surface(2, 1),
            "one of two leaving changes nothing"
        );
        assert!(!zones_flip_the_surface(0, 0));
        assert!(
            !zones_flip_the_surface(1, 1),
            "a refresh that kept its zone"
        );
    }
}

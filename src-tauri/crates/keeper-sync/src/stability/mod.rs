//! The completeness gate, moved to `keeper_folder::stability` (story 119.1).
//!
//! Re-exported under its old path until 121.4 removes this crate. What stays
//! here is the one constructor that reads a [`SyncProfile`], which the leaf
//! crate may not name (DF/AD-2).

pub use keeper_folder::stability::*;

use crate::error::Result;
use crate::exclude::ExcludeSet;
use crate::profile::SyncProfile;

/// The gate a sync profile implies.
pub trait GateForProfile: Sized {
    /// Build the gate a profile implies: its root, its excludes, and its
    /// effective window (which already accounts for removable media).
    fn for_profile(profile: &SyncProfile) -> Result<Self>;
}

impl GateForProfile for StabilityGate {
    fn for_profile(profile: &SyncProfile) -> Result<Self> {
        Ok(Self::new(
            profile.local_path.clone(),
            ExcludeSet::new(&profile.excludes)?,
            profile.effective_settle_ms(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn a_gate_built_from_a_profile_inherits_its_window_and_excludes() {
        let mut profile =
            SyncProfile::new("01J", "docs", "/tmp/docs", "https://example.test/r.git");
        profile.excludes = vec!["*.scratch".to_owned()];
        profile.removable = true;
        let mut g = StabilityGate::for_profile(&profile).expect("profile compiles");
        // `removable` widens the default window; taking `settle_ms` raw would
        // silently drop that.
        assert_eq!(g.settle_ms(), profile.effective_settle_ms());
        assert_eq!(
            g.is_stable(Path::new("/tmp/docs/notes.scratch"), 0),
            StabilityVerdict::Excluded
        );
    }
}

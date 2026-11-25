//! A state directory of keeper-cli's own (story 123.3, DR-4, XR-11): the
//! app and keeper-cli never share a data directory, so the lock never stands
//! between them. The marker `<state>/.keeper-cli` is written the first time
//! keeper-cli opens a directory that holds only 113.5's layout; one that
//! holds the app's layout (`keeper.db` or `hq/`) and no marker is refused
//! before anything there is opened.

use std::path::Path;

/// The marker's file name.
pub const MARKER: &str = ".keeper-cli";

/// The sentence a directory that is not keeper-cli's own is refused with.
pub const NOT_OURS: &str =
    "This directory was not written by keeper-cli. Use a state directory of keeper-cli's own.";

/// Claim `state` for keeper-cli: create it (mode 0700) when absent, accept
/// it when it carries the marker, refuse it when it holds the app's layout
/// without one, and otherwise write the marker.
pub fn claim(state: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt as _;
    if !state.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(state)
            .map_err(|e| format!("{}: {e}", state.display()))?;
    }
    let marker = state.join(MARKER);
    if marker.exists() {
        return Ok(());
    }
    if state.join("keeper.db").exists() || state.join("hq").exists() {
        return Err(NOT_OURS.to_owned());
    }
    std::fs::write(&marker, "keeper-cli state directory\n")
        .map_err(|e| format!("{}: {e}", marker.display()))
}

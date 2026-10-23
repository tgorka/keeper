//! A file read inside a drive's zone, through `keeper_sync::browse`, so a
//! link that points out of the drive is a refusal, never a read (AD-65).
//!
//! The zone scans, avatars and skills that also lived here served the agents'
//! drive host, which left keeper with the agents' runtime (108.2, 108.3c).

use std::path::Path;

use keeper_sync::browse;

/// A file's text under `root`, reached through `browse::resolve_known`;
/// `None` only when the disk says it is not there. A link out of `root`, a
/// dangling link, a folder on the way that cannot be searched, or anything
/// but a regular file (a pipe would never end a read), is refused: what is
/// there is not known, which is not the same as nothing.
pub fn read_text(root: &Path, rel: &str) -> Result<Option<String>, String> {
    match browse::resolve_known(root, rel).map(browse::Known::landed) {
        Ok(Some(landing)) if !landing.path().is_file() => Err(format!("{rel} is not a file")),
        Ok(Some(landing)) => match std::fs::read_to_string(landing.path()) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{rel} could not be read: {error}")),
        },
        Ok(None) => Ok(None),
        Err(refusal) => Err(format!("{rel} is refused: {refusal}")),
    }
}

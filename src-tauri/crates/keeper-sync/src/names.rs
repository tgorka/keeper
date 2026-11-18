/// Create a file whose name is not valid UTF-8, or `None` when the filesystem
/// refuses the bytes.
///
/// **macOS will not let you make one.** APFS validates filename bytes and
/// returns `EILSEQ` ("Illegal byte sequence") for anything that is not valid
/// UTF-8, so every fixture in this crate that needs such a file on disk is
/// unbuildable on the machine keeper ships from. That is a fact about the
/// filesystem, not about keeper, and it does not make the defect theoretical:
/// a Mac cannot CREATE such a name but can very easily RECEIVE one, because
/// git carries the raw bytes and a Linux peer can commit it. The sync that
/// delivers it is exactly the path this crate is.
///
/// So the tests that need the file skip with a reason where the filesystem
/// refuses, and the rules those tests are about — which are pure — are
/// asserted separately and run everywhere.
#[cfg(all(test, unix))]
pub(crate) fn create_unspellable(
    dir: &std::path::Path,
    bytes: &[u8],
) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let path = dir.join(std::ffi::OsString::from_vec(bytes.to_vec()));
    match std::fs::write(&path, "x") {
        Ok(()) => Some(path),
        // EILSEQ. Any other error is a real failure and must not be swallowed
        // into a silent skip — a test that skips on a permissions bug reports
        // success for a run that proved nothing.
        Err(error) if error.raw_os_error() == Some(92) => None,
        Err(error) => panic!("could not create the fixture: {error}"),
    }
}

/// What a test prints when it steps aside, so a green run that skipped is
/// distinguishable from a green run that checked.
#[cfg(all(test, unix))]
pub(crate) const UNSPELLABLE_UNAVAILABLE: &str =
    "skipped: this filesystem refuses a non-UTF-8 filename (macOS/APFS EILSEQ)";

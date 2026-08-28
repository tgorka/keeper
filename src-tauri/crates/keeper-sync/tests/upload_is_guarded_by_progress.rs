//! An upload is guarded by progress, not by silence.
//!
//! While a client sends a body the server says nothing, because that is
//! correct. A ceiling on silence is therefore, for an upload, a ceiling on
//! DURATION — and an object that needs longer than the ceiling can never
//! finish, however good the connection is.
//!
//! Observed on a real folder before this: eight objects, 3.8 GB, failing every
//! 61 seconds for eighteen hours against a 60-second read timeout. The median
//! gap between failures was 60.2s and the minimum was 60s — the size of the
//! object never entered into it, and neither did the network.

use std::time::Duration;

/// The two ceilings say different things, and the transfer one has to be far
/// enough above a real upload that it never decides anything.
///
/// This is a unit assertion rather than a live transfer because the failure it
/// guards is a CONSTANT being wrong, and a test that uploaded a gigabyte to
/// find that out would take as long as the bug did.
#[test]
fn a_transfer_may_take_much_longer_than_a_request() {
    let request = keeper_sync::http::READ_TIMEOUT;
    let transfer = keeper_sync::http::TRANSFER_READ_TIMEOUT;

    assert!(
        transfer >= request * 10,
        "a transfer ceiling that is close to the request ceiling is the bug: {transfer:?} vs {request:?}"
    );
    // Twenty-eight minutes at the floor speed that made this fail — 3.8 GB over
    // eight objects, on a link that could not do 475 MB in sixty seconds.
    assert!(transfer >= Duration::from_secs(15 * 60));
}

/// And the guard that replaces it has to be long enough that a slow disk is not
/// mistaken for a dead connection.
///
/// A file on a sleeping external drive can genuinely take seconds to yield its
/// next chunk. Turning that into a failure would trade a rare hang for a retry
/// storm on exactly the folders — large files, external volumes — where this
/// matters most.
#[test]
fn the_stall_window_is_longer_than_a_slow_disk_takes_to_answer() {
    let window = keeper_sync::lfs::basic::UPLOAD_STALL_WINDOW;
    assert!(window >= Duration::from_secs(60), "{window:?}");
    // ...and short enough that a genuinely dead upload is noticed in a couple
    // of minutes rather than half an hour.
    assert!(window <= Duration::from_secs(5 * 60), "{window:?}");
}

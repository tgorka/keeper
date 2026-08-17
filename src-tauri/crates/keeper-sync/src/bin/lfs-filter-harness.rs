//! A binary whose only job is to *be* keeper's LFS filter, for tests (DW-140).
//!
//! Registering `filter.lfs.process` makes gitoxide launch a subprocess and speak
//! a protocol to it, and DW-206 is what that costs when the launch fails: gix's
//! `maybe_launch_process` errors before any driver leniency is consulted, so
//! `status` fails hard whatever `filter.lfs.required` says, no commit happens,
//! the index is never rewritten, and the same entry is re-read forever. That
//! shape cannot be tested by calling a function — it needs a real process on the
//! other end of a real pipe.
//!
//! The app and the daemon both serve this invocation from their own `main`; a
//! test cannot use either (one is a GUI shell that will not build headless, the
//! other is a different crate, so `CARGO_BIN_EXE_` cannot name it). Hence a
//! third, minimal one, which exists purely so `tests/lfs_filter_process.rs` can
//! point a git config at something real.

fn main() {
    let Some(invocation) = keeper_sync::lfs::filter::parse_args(std::env::args().skip(1)) else {
        eprintln!("not an lfs filter invocation");
        std::process::exit(2);
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let served = match invocation {
        keeper_sync::lfs::filter::Invocation::Single { direction, repo } => {
            keeper_sync::lfs::filter::run(&repo, direction, &mut stdin.lock(), &mut stdout.lock())
        }
        keeper_sync::lfs::filter::Invocation::Process { repo } => {
            keeper_sync::lfs::filter::run_process(&repo, &mut stdin.lock(), &mut stdout.lock())
        }
        keeper_sync::lfs::filter::Invocation::Unsupported => {
            eprintln!("unsupported lfs filter invocation");
            std::process::exit(1);
        }
    };
    if let Err(err) = served {
        // stderr only: git reads stdout as content.
        eprintln!("lfs filter failed: {err}");
        std::process::exit(1);
    }
}

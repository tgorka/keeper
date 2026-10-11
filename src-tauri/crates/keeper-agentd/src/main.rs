//! `keeper-agentd` — keeper's agents on a Linux server (AD-375; story 90.5).
//!
//! One process per principal, as that principal's own OS user. It answers in
//! its agents' rooms, streams each answer as edits, and writes each session's
//! log into the drive, which its own sync engine commits and pushes.
//!
//! **`main`'s order is the security of the process (S-07):**
//! 1. git's LFS filter invocations are answered first: the engine registers
//!    this binary as the filter, and git runs it with no configuration.
//! 2. The command line is parsed.
//! 3. Every secret is read, the secret variables leave the environment and
//!    the process makes itself non-dumpable — while it still has one thread.
//! 4. Only then is the multi-thread runtime built; `#[tokio::main]` would
//!    have started its threads before the body ran.
//!
//! Logging is `tracing-subscriber`'s fmt layer to stderr, which journald
//! keeps, and nothing else: agentd registers no observability sink (S-19).
//! Installing a subscriber starts no thread, so it is set up right after the
//! parse and the hardening can be logged.

// matrix-sdk's sync future is deep enough to need it, as in keeper-core.
#![recursion_limit = "256"]

mod cli;
mod host_lock;
mod init;
mod login;
mod report;
mod seed;

use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    if served_as_lfs_filter() {
        return ExitCode::SUCCESS;
    }
    let cli = cli::Cli::parse();
    init_logging(cli.verbose);
    ExitCode::from(cli::run(cli))
}

/// Serve git's `lfs clean|smudge|filter-process` invocation when that is what
/// this run is, exactly as the app and syncd do: `Engine::open` registers the
/// running binary as the filter. Nothing but object bytes goes to stdout.
fn served_as_lfs_filter() -> bool {
    use keeper_sync::lfs::filter;

    let Some(invocation) = filter::parse_args(std::env::args().skip(1)) else {
        return false;
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let served = match invocation {
        filter::Invocation::Single { direction, repo } => {
            filter::run(&repo, direction, &mut stdin.lock(), &mut stdout.lock())
        }
        filter::Invocation::Process { repo } => {
            filter::run_process(&repo, &mut stdin.lock(), &mut stdout.lock())
        }
        // A newer config reached an older binary: fail loudly and write
        // nothing, so git does not record an empty success (DW-140).
        filter::Invocation::Unsupported => {
            eprintln!("keeper-agentd: this build cannot serve that lfs filter invocation");
            std::process::exit(1);
        }
    };
    if let Err(err) = served {
        // stderr only: stdout is content to git.
        eprintln!("keeper-agentd: lfs filter failed: {err}");
        std::process::exit(1);
    }
    true
}

/// stderr only, no ANSI: journald keeps it, and a log file would be a second
/// copy of what the journal already holds. `RUST_LOG` beats `--verbose`.
fn init_logging(verbose: u8) {
    use tracing_subscriber::filter::EnvFilter;

    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(keeper_sync::logfile::default_filter(level)));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
}

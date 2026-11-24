//! `keeper-cli`: keeper without a window (story 113.5, DF/AD-2). One command
//! line, `keeper-cli <module> <verb> [args] [--json]`, for a person or an
//! agent who drives keeper on a server, from a script or from a test. Its
//! first module is `graph` (`keeper-cli graph <verb>`): a person's device's
//! graph against a real keeper-hq. Since story 123.2 the binary composes
//! keeper's runtime with the `app` and `graph` modules ([`compose`]) and
//! every graph verb reaches its operation through the runtime's dispatcher.
//!
//! # Output
//!
//! Every verb has a human rendering, a sentence, and, with `--json`, one JSON
//! document on one line. Both come out of `output::Printer`, the only writer
//! of stdout: a command's output *is* stdout. Diagnostics (a usage error, an
//! unsupported platform) go to stderr.
//!
//! # Exit codes
//!
//! keeper-syncd's five, followed without depending on it: `0` success · `1`
//! operational failure · `2` configuration error · `3` a prerequisite is
//! missing · `4` the work did not run, and that is not a failure. `2` and `3`
//! will never succeed on retry, `1` might, `4` has nothing to alert about.
//! Each module says which of its answers earns which code.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod accounts;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod app_start;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod compose;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod debug;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod files;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod graph;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod output;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod serve;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod state;

/// Everything went as asked.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
const EXIT_OK: u8 = 0;
/// Something failed that might succeed later: the server, the network, storage.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
const EXIT_FAILURE: u8 = 1;
/// The invocation or its inputs are wrong. Retrying changes nothing.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
const EXIT_CONFIG: u8 = 2;
/// A hard prerequisite is absent: the platform, or an admitted graph worker.
const EXIT_PREREQUISITE: u8 = 3;
/// The work did not run, and that is not a failure (it waits for an approval).
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
const EXIT_DEFERRED: u8 = 4;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod cli {
    use clap::{Parser, Subcommand};

    #[derive(Parser)]
    #[command(
        name = "keeper-cli",
        about = "keeper without a window: keeper-cli <module> <verb> [args] [--json]"
    )]
    pub struct Cli {
        /// Print one JSON document instead of a sentence.
        #[arg(long, global = true)]
        pub json: bool,
        #[command(subcommand)]
        pub module: Module,
    }

    #[derive(Subcommand)]
    pub enum Module {
        /// A person's device's graph against a real keeper-hq.
        Graph(super::graph::Args),
        /// A domain's drives: list, fetch on demand, put.
        Files(super::files::Args),
        /// The person's domains, as a device last showed them.
        #[command(subcommand)]
        Accounts(super::accounts::Args),
        /// Serve keeper's local socket on this state until Ctrl-C.
        Serve(super::serve::Args),
        /// Test-only verbs; not part of keeper-cli's surface.
        #[command(subcommand, hide = true)]
        Debug(super::debug::Args),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> std::process::ExitCode {
    use clap::Parser as _;
    // clap prints its own diagnostics to stderr and exits `2` on a usage
    // error (`0` for `--help`).
    let cli = cli::Cli::parse();
    let printer = output::Printer::new(cli.json);
    let code = match cli.module {
        cli::Module::Graph(args) => graph::main(args, &printer),
        cli::Module::Files(args) => files::main(args, &printer),
        cli::Module::Accounts(args) => accounts::main(args, &printer),
        cli::Module::Serve(args) => serve::main(args, &printer),
        cli::Module::Debug(args) => debug::main(args, &printer),
    };
    std::process::ExitCode::from(code)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() -> std::process::ExitCode {
    // A diagnostic, on stderr: there is no answer to print and no logging here.
    eprintln!("keeper-cli runs on Linux and macOS only");
    std::process::ExitCode::from(EXIT_PREREQUISITE)
}

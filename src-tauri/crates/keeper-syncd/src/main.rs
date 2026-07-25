//! `keeper-syncd` — folder synchronization with no app attached (Epic 30,
//! AD-52).
//!
//! The same `keeper-sync` engine the desktop app runs, driven from a CLI and a
//! TOML config instead of a webview and a tray. There is no forked policy here:
//! every verb delegates to `keeper_sync::engine::Engine`, and this binary
//! supplies only the three things a headless box needs and the app already has
//! elsewhere — an XDG-shaped [`SyncPlatform`](keeper_sync::SyncPlatform), a
//! configuration format, and a process lifecycle.
//!
//! **Linux-first, unix-only.** Secret files are enforced by mode bits and
//! `doctor` reads `/proc`, so this deliberately does not pretend to build for a
//! platform that cannot express either.
//!
//! Startup order matters and is not arbitrary: the configuration is read
//! *before* logging is initialised, because the configured level is one of the
//! inputs to the logger. A configuration error is therefore reported through
//! the command layer rather than logged — which is also why `init`, `doctor`
//! and `logs` all work on a box whose config is missing or broken.

mod commands;
mod config;
mod platform;
mod update;

use std::path::Path;
use std::process::ExitCode;

use clap::Parser as _;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use crate::commands::{Cli, Printer, EXIT_CONFIG, EXIT_OK};
use crate::platform::LinuxPlatform;

#[tokio::main]
async fn main() -> ExitCode {
    // clap prints its own diagnostics and exits on a parse error or `--help`.
    let cli = Cli::parse();
    let json = cli.json;
    let verbose = cli.verbose;

    let platform = match LinuxPlatform::new() {
        Ok(platform) => platform,
        Err(err) => {
            // The one place `eprintln!` is legitimate: logging does not exist
            // yet, and it cannot — resolving the state directory is what just
            // failed. Everything after this point goes through `tracing`.
            eprintln!("keeper-syncd: {err}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };

    let config_path = cli.config.clone().unwrap_or_else(|| platform.config_path());
    let config = config::load(&config_path);

    let configured_level = config
        .as_ref()
        .ok()
        .map(|config| config.daemon.log_level.clone());
    init_logging(&platform.log_path(), verbose, configured_level.as_deref());

    let code = match commands::run(cli, platform, config_path, config).await {
        Ok(()) => EXIT_OK,
        Err(err) => {
            let code = err.exit_code();
            tracing::error!(error = %err, code = err.code(), exit = u64::from(code), "keeper-syncd failed");
            // A `--json` consumer must be able to read the failure off stdout
            // rather than scraping stderr.
            Printer::new(json).json(&serde_json::json!({
                "ok": false,
                "error": err.to_string(),
                "code": err.code(),
                "exit": code,
            }));
            code
        }
    };
    ExitCode::from(code)
}

/// Send events to stderr and to the state-directory log file.
///
/// Both sinks, always: stderr is what systemd captures into the journal, and
/// the file is what survives a journal rotation and what `keeper-syncd logs`
/// reads. ANSI is off on both — this process normally has no terminal, and
/// escape codes in a journal or a log file are noise, not colour.
///
/// Best-effort by design. A state directory that cannot be written is worth a
/// warning, not a refusal to sync: losing the log file is strictly better than
/// losing the daemon.
fn init_logging(log_path: &Path, verbose: u8, configured_level: Option<&str>) {
    let (filter, rust_log_problem) = build_filter(verbose, configured_level);

    let stderr_layer = fmt::layer().with_ansi(false).with_writer(std::io::stderr);
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer);

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path);
    let (installed, file_problem) = match file {
        // A bare `File` is itself a `MakeWriter`; each event is one `write_all`
        // on an `O_APPEND` descriptor, so nothing sits in a buffer waiting to be
        // lost if the process is killed.
        Ok(file) => (
            registry
                .with(fmt::layer().with_ansi(false).with_writer(file))
                .try_init(),
            None,
        ),
        Err(err) => (
            registry.try_init(),
            Some(format!("{}: {err}", log_path.display())),
        ),
    };

    if installed.is_err() {
        // Only reachable if something already installed a global subscriber,
        // which in a binary means a test harness. Nothing to report to.
        return;
    }
    if let Some(problem) = file_problem {
        tracing::warn!(problem = %problem, "cannot open the daemon log file; logging to stderr only");
    }
    if let Some(problem) = rust_log_problem {
        tracing::warn!(problem = %problem, "RUST_LOG could not be parsed and was ignored");
    }
}

/// Resolve the log filter: `RUST_LOG` beats `--verbose` beats the config.
///
/// Returns the filter plus a complaint when `RUST_LOG` was set but unusable —
/// a silently ignored `RUST_LOG` is a genuinely wasted afternoon, and it cannot
/// be reported until the subscriber it produces is installed.
fn build_filter(verbose: u8, configured_level: Option<&str>) -> (EnvFilter, Option<String>) {
    match EnvFilter::try_from_default_env() {
        Ok(filter) => (filter, None),
        Err(err) => {
            // `try_from_default_env` fails both when RUST_LOG is unset (the
            // ordinary case) and when it is set but unparseable (the case worth
            // saying something about), so the variable itself decides which.
            let complaint = std::env::var_os(EnvFilter::DEFAULT_ENV)
                .filter(|value| !value.is_empty())
                .map(|_| err.to_string());
            // `EnvFilter::new` parses leniently, so a malformed directive would
            // degrade silently. Every string reaching it is either a literal
            // from `default_level` or a level `config::parse` already checked
            // against its allow-list.
            (
                EnvFilter::new(default_level(verbose, configured_level)),
                complaint,
            )
        }
    }
}

/// The level to log at when `RUST_LOG` has not spoken.
fn default_level(verbose: u8, configured_level: Option<&str>) -> &str {
    match verbose {
        0 => configured_level.unwrap_or("info"),
        1 => "debug",
        _ => "trace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_escalates_the_default_level() {
        // A verbosity flag that quietly does nothing is worse than none.
        assert_eq!(default_level(0, None), "info");
        assert_eq!(default_level(1, None), "debug");
        assert_eq!(default_level(2, None), "trace");
        // More -v than levels must saturate, not wrap or panic.
        assert_eq!(default_level(u8::MAX, None), "trace");
    }

    #[test]
    fn the_configured_level_is_the_default_and_verbosity_overrides_it() {
        assert_eq!(default_level(0, Some("warn")), "warn");
        // An explicit -v on the command line must beat the config file.
        assert_eq!(default_level(1, Some("warn")), "debug");
        assert_eq!(default_level(2, Some("error")), "trace");
    }

    #[test]
    fn every_configurable_log_level_survives_the_round_trip() {
        // `config::parse` validates against this exact set, so anything it
        // accepts must come back out of the level resolver unchanged.
        for level in ["trace", "debug", "info", "warn", "error"] {
            assert_eq!(default_level(0, Some(level)), level);
        }
    }
}

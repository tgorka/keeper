//! The line every log starts with: which binary wrote it.
//!
//! # Why this exists
//!
//! A log that says `keeper 0.8.20` cannot tell two builds of 0.8.20 apart, and
//! this project ships those routinely — a release, then a test build off a
//! branch installed over it. When a log arrives from a machine nobody can
//! reach, "which binary was this?" is the first question, and until now nothing
//! in the file answered it.
//!
//! # What it reports, and why each part earns its place
//!
//! - **version** — the release it claims to be.
//! - **commit** — the source it was built from, `-dirty` when the tree had
//!   uncommitted changes, because a build from a modified tree is not the commit
//!   it names and a log claiming otherwise sends the reader to the wrong diff.
//!   `unknown` is expected and honest: `scripts/release-macos.sh` builds from an
//!   rsync'd copy with no `.git`.
//! - **built** — when, so two builds of one commit are still distinguishable.
//! - **profile and target** — a debug build behaves differently enough that
//!   reading its log as a release build's wastes an afternoon.
//! - **signature** — on macOS, the team, the authority and the **cdhash**. The
//!   cdhash is the exact bytes of the running executable: two builds that agree
//!   on everything above and differ here are different binaries, full stop. It
//!   is also what says whether a machine is running a properly signed build or
//!   an ad-hoc one, which decides whether its TCC grants and keychain items
//!   survive an update at all.
//!
//! # Why the signature is read in the background
//!
//! It costs a `codesign` process. That is tens of milliseconds and it is not
//! worth one of them on the path to the first window, so it lands a moment
//! after the rest. A log line is not less useful for arriving second.

use std::process::Command;

/// What the build says about itself, without asking the system anything.
pub fn banner() -> String {
    format!(
        "keeper {} commit={} built={} profile={} target={}",
        env!("CARGO_PKG_VERSION"),
        env!("KEEPER_BUILD_SHA"),
        built_at(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        std::env::consts::ARCH,
    )
}

/// The build timestamp as a readable UTC instant rather than an epoch second,
/// because the reader of a log is a person comparing it against other lines in
/// the same file.
fn built_at() -> String {
    let secs: i64 = env!("KEEPER_BUILD_TIME").parse().unwrap_or(0);
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Log the banner, then the signature once it has been read.
pub fn announce() {
    tracing::info!("{}", banner());
    #[cfg(target_os = "macos")]
    std::thread::spawn(|| {
        if let Some(signature) = signature() {
            tracing::info!("{signature}");
        } else {
            // Absence is itself a finding: an unsigned or unreadable signature
            // is exactly the state in which keychain items and TCC grants stop
            // surviving updates, and a log that stayed silent about it would
            // hide the cause of the next "it forgot my accounts".
            tracing::warn!("signature: could not be read; this build may be unsigned or ad-hoc");
        }
    });
}

/// The running bundle's signing identity, as one line.
#[cfg(target_os = "macos")]
fn signature() -> Option<String> {
    // The executable, not a guessed bundle path: an app run from a build
    // directory, a copy, or `/Applications` must all report the thing that is
    // actually running.
    let exe = std::env::current_exe().ok()?;
    let out = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=2", "--"])
        .arg(&exe)
        .output()
        .ok()?;
    // `codesign -d` writes its report to stderr, which is not an error here.
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .map(|value| value.trim().to_owned())
    };
    let authority = text
        .lines()
        .find_map(|line| line.strip_prefix("Authority="))
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| "none".to_owned());
    Some(format!(
        "signature: team={} cdhash={} authority=\"{}\" identifier={}",
        field("TeamIdentifier=").unwrap_or_else(|| "none".to_owned()),
        field("CDHash=").unwrap_or_else(|| "none".to_owned()),
        authority,
        field("Identifier=").unwrap_or_else(|| "none".to_owned()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The banner has to carry every field a reader needs to identify a binary.
    /// Asserted by name rather than by shape: a banner that lost `commit` in a
    /// refactor would still look like a banner.
    #[test]
    fn the_banner_names_the_build() {
        let line = banner();
        for key in ["keeper ", "commit=", "built=", "profile=", "target="] {
            assert!(line.contains(key), "{key} missing from {line}");
        }
    }

    /// A version that is not the crate's would make every log line about a
    /// different program than the one that wrote it.
    #[test]
    fn the_banner_reports_this_crate_s_version() {
        assert!(banner().contains(env!("CARGO_PKG_VERSION")));
    }

    /// `unknown` is a legitimate answer — the release script builds without a
    /// `.git` — and the banner must be well-formed either way.
    #[test]
    fn a_build_with_no_commit_still_produces_a_line() {
        let line = banner();
        let commit = line
            .split("commit=")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .expect("a commit field");
        assert!(!commit.is_empty(), "even 'unknown' must be spelled out");
    }
}

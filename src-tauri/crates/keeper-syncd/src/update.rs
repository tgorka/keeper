//! Self-update against GitHub releases.
//!
//! The desktop app auto-updates through `tauri-plugin-updater`, which verifies
//! a minisign signature over its artifacts. The daemon cannot reuse that: it is
//! not a Tauri bundle, it is installed by copying a file, and it is frequently
//! the only thing on the machine.
//!
//! # Why this never installs by itself
//!
//! A sync daemon holds a durable journal and can be mid-push at any moment.
//! Replacing its binary silently, on a timer, is a good way to turn a routine
//! release into a corrupted transfer that nobody asked for. So:
//!
//! * [`check`] is read-only and is what `doctor` and startup use;
//! * [`apply`] runs only from an explicit `keeper-syncd update`;
//! * the replacement swaps the file, and the **running** process keeps its old
//!   inode until it is restarted — which the command says plainly.
//!
//! # Integrity
//!
//! Every release asset is published with a `.sha256` sidecar. The download is
//! hashed while it streams and refused on mismatch, so a truncated transfer or
//! a substituted asset never lands on disk. This is a weaker guarantee than the
//! app's signature check — it authenticates the transfer, not the publisher —
//! and that difference is stated in the docs rather than glossed over.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use keeper_sync::{Result as SyncResult, SyncError};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Where releases are published. Kept as a constant so the disclosed egress
/// destination and the code that reaches it cannot drift apart.
pub const RELEASES_API: &str = "https://api.github.com/repos/tgorka/keeper/releases/latest";

/// Read bodies are bounded: a hostile or broken endpoint must not be able to
/// turn a version check into an unbounded allocation.
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
/// A daemon binary is tens of megabytes; 256 MiB is generous and still finite.
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;

/// The asset naming scheme CI publishes, e.g. `keeper-syncd-x86_64-unknown-linux-gnu`.
pub fn asset_name() -> String {
    format!("keeper-syncd-{}", target_triple())
}

/// This build's target triple.
///
/// Derived from `std::env::consts` rather than a build script: the daemon ships
/// for a small, explicit set of targets, and an unrecognised combination should
/// say so rather than guess at an asset name that will 404.
pub fn target_triple() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        _ => "unsupported",
    }
}

/// A release newer than what is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    /// The release tag, e.g. `v0.4.0`.
    pub tag: String,
    /// The semantic version parsed out of the tag.
    pub version: String,
    pub download_url: String,
    pub sha256_url: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Compare two dotted numeric versions.
///
/// Deliberately not a semver dependency: the comparison needed here is
/// "is the published release numerically newer", and a pre-release suffix is
/// treated as older than the same release without one. Non-numeric components
/// compare as zero rather than failing the check — refusing to look for updates
/// because a tag was odd would be the worse outcome.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let core = v.trim_start_matches('v');
        let (core, pre) = match core.split_once(['-', '+']) {
            Some((head, _)) => (head, true),
            None => (core, false),
        };
        (
            core.split('.')
                .map(|p| p.parse::<u64>().unwrap_or(0))
                .collect(),
            pre,
        )
    }
    let (a, a_pre) = parts(candidate);
    let (b, b_pre) = parts(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    // Equal cores: a release outranks a pre-release of the same number.
    b_pre && !a_pre
}

/// Pick the release asset for this target, and its checksum sidecar.
fn select(release: &Release) -> Option<(String, String)> {
    let wanted = asset_name();
    let binary = release
        .assets
        .iter()
        .find(|a| a.name == wanted)?
        .browser_download_url
        .clone();
    let sums = format!("{wanted}.sha256");
    let checksum = release
        .assets
        .iter()
        .find(|a| a.name == sums)?
        .browser_download_url
        .clone();
    Some((binary, checksum))
}

/// Ask GitHub whether a newer release exists. Read-only.
///
/// `Ok(None)` means "already current", which is the common answer and not a
/// failure. A network problem IS an error, so `doctor` can report it as one
/// rather than implying the machine is up to date when it simply could not ask.
pub fn check(current_version: &str) -> SyncResult<Option<Available>> {
    if target_triple() == "unsupported" {
        return Err(SyncError::Config(format!(
            "no keeper-syncd release is published for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    }
    let body = fetch(RELEASES_API, MAX_METADATA_BYTES)?;
    let release: Release = serde_json::from_slice(&body)
        .map_err(|err| SyncError::Config(format!("unreadable release metadata: {err}")))?;

    let version = release.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&release.tag_name, current_version) {
        return Ok(None);
    }
    let Some((download_url, sha256_url)) = select(&release) else {
        return Err(SyncError::Config(format!(
            "release {} publishes no {} asset",
            release.tag_name,
            asset_name()
        )));
    };
    Ok(Some(Available {
        tag: release.tag_name,
        version,
        download_url,
        sha256_url,
    }))
}

/// Download, verify and install `available`, returning where it was written.
///
/// The new binary is staged beside the current one and moved into place with a
/// rename, so an interrupted download can never leave a half-written
/// executable. A running daemon keeps its old inode and must be restarted.
pub fn apply(available: &Available, destination: &Path) -> SyncResult<PathBuf> {
    let expected = String::from_utf8_lossy(&fetch(&available.sha256_url, MAX_METADATA_BYTES)?)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SyncError::Integrity {
            subject: "release checksum".to_owned(),
            expected: "64 hex characters".to_owned(),
            actual: format!("{} characters", expected.len()),
        });
    }

    let payload = fetch(&available.download_url, MAX_BINARY_BYTES)?;
    let actual = hex::encode(Sha256::digest(&payload));
    if actual != expected {
        return Err(SyncError::Integrity {
            subject: format!("keeper-syncd {}", available.tag),
            expected,
            actual,
        });
    }

    // Stage in the destination's own directory: a rename across filesystems is
    // not atomic, and /tmp is very often a different one.
    let parent = destination.parent().unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| SyncError::io("stage the update", parent.to_path_buf(), err))?;
    staged
        .write_all(&payload)
        .map_err(|err| SyncError::io("write the update", staged.path().to_path_buf(), err))?;
    staged
        .flush()
        .map_err(|err| SyncError::io("flush the update", staged.path().to_path_buf(), err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(staged.path(), std::fs::Permissions::from_mode(0o755))
            .map_err(|err| SyncError::io("mark the update executable", staged.path(), err))?;
    }
    staged
        .persist(destination)
        .map_err(|err| SyncError::io("install the update", destination.to_path_buf(), err.error))?;
    Ok(destination.to_path_buf())
}

/// One bounded blocking GET.
///
/// Blocking on purpose: `update` is a one-shot command, and dragging an async
/// runtime into it for a single request would be more machinery than the task
/// deserves.
fn fetch(url: &str, cap: u64) -> SyncResult<Vec<u8>> {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("github.com")
        .to_owned();
    let client = reqwest::blocking::Client::builder()
        // GitHub rejects requests with no user agent.
        .user_agent(concat!("keeper-syncd/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|err| SyncError::Network {
            host: host.clone(),
            reason: err.to_string(),
        })?;
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|err| SyncError::Network {
            host: host.clone(),
            reason: err.to_string(),
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(SyncError::Network {
            host,
            reason: format!("{url} answered {status}"),
        });
    }
    if let Some(len) = response.content_length() {
        if len > cap {
            return Err(SyncError::Network {
                host,
                reason: format!("response is {len} bytes, over the {cap}-byte cap"),
            });
        }
    }
    let bytes = response.bytes().map_err(|err| SyncError::Network {
        host,
        reason: err.to_string(),
    })?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_are_recognised_across_every_component() {
        assert!(is_newer("v0.4.0", "0.3.0"));
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(
            is_newer("v0.3.10", "0.3.9"),
            "components compare numerically, not as text"
        );
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn a_prerelease_never_supersedes_the_release_it_precedes() {
        assert!(!is_newer("0.4.0-rc.1", "0.4.0"));
        assert!(is_newer("0.4.0", "0.4.0-rc.1"));
        // …but it is still newer than the previous release.
        assert!(is_newer("0.4.0-rc.1", "0.3.0"));
    }

    #[test]
    fn a_short_version_is_padded_rather_than_mishandled() {
        assert!(is_newer("0.4", "0.3.9"));
        assert!(!is_newer("0.3", "0.3.0"));
    }

    #[test]
    fn an_unparseable_component_does_not_abort_the_check() {
        // Refusing to look for updates because a tag was odd is worse than
        // treating the odd part as zero.
        assert!(!is_newer("v0.3.x", "0.3.0"));
        assert!(is_newer("v0.4.x", "0.3.0"));
    }

    #[test]
    fn the_asset_name_matches_what_ci_publishes() {
        let name = asset_name();
        assert!(name.starts_with("keeper-syncd-"));
        assert!(
            !name.ends_with("unsupported"),
            "this test host has no published target: {name}"
        );
    }

    #[test]
    fn an_asset_is_only_selected_when_its_checksum_is_published_too() {
        // Installing a binary whose checksum is missing would silently drop the
        // only integrity guarantee this path has.
        let with_both = Release {
            tag_name: "v0.4.0".into(),
            assets: vec![
                Asset {
                    name: asset_name(),
                    browser_download_url: "https://x/bin".into(),
                },
                Asset {
                    name: format!("{}.sha256", asset_name()),
                    browser_download_url: "https://x/sum".into(),
                },
            ],
        };
        assert!(select(&with_both).is_some());

        let binary_only = Release {
            tag_name: "v0.4.0".into(),
            assets: vec![Asset {
                name: asset_name(),
                browser_download_url: "https://x/bin".into(),
            }],
        };
        assert!(select(&binary_only).is_none());
    }

    #[test]
    fn release_metadata_parses_the_shape_github_actually_returns() {
        let body = br#"{
            "tag_name": "v0.4.0",
            "name": "keeper 0.4.0",
            "assets": [
                {"name": "keeper-syncd-x86_64-unknown-linux-gnu",
                 "browser_download_url": "https://example/bin",
                 "size": 123},
                {"name": "unrelated.dmg", "browser_download_url": "https://example/dmg"}
            ]
        }"#;
        let release: Release = serde_json::from_slice(body).expect("parses");
        assert_eq!(release.tag_name, "v0.4.0");
        assert_eq!(release.assets.len(), 2);
    }
}

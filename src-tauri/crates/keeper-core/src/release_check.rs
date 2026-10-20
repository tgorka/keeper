//! Whether a newer keeper is out, for a build that cannot install one itself
//! (story 102.9 #2, R464).
//!
//! The phone tier is dev-signed and sideloaded (`scripts/install-ios.sh`,
//! `scripts/install-android.sh`): no TestFlight, no store, no
//! `itms-services` manifest, and the desktop updater's `latest.json` carries
//! macOS artifacts only. So the phone never downloads or installs anything —
//! it asks GitHub for the latest release's tag once, when the person presses
//! *Check for updates*, compares it with the version it was built as, and
//! says how to install the new one from the Mac.
//!
//! One unauthenticated `GET` of [`EGRESS_RELEASE_CHECK_ENDPOINT`] — the
//! destination Settings › About lists on these builds instead of the signed
//! updater's — bounded by [`TIMEOUT`] and [`MAX_BODY_BYTES`]. Every way it can
//! fail is a sentence where the person is looking and one WARN line (R381);
//! nothing here retries or runs on a cadence.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::disclosure::Device;
pub use crate::egress::EGRESS_RELEASE_CHECK_ENDPOINT;

/// How long the one request may take, connection included.
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// The largest answer read. GitHub's release object is a few kilobytes plus
/// its asset list; anything past this is not the answer asked for.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// What a check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReleaseCheckOutcome {
    /// A release newer than this build is published.
    Newer,
    /// This build is the latest release, or newer than it.
    Current,
    /// keeper could not tell; the sentence says why.
    Failed,
}

/// The answer to *Check for updates* on a build without an in-app updater:
/// what was found, and Rust's sentence for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReleaseCheckVm {
    pub outcome: ReleaseCheckOutcome,
    /// The version this build was made as.
    pub installed: String,
    /// The latest release's version, once it was read.
    pub latest: Option<String>,
    pub sentence: String,
}

/// A release version as keeper's tags are written (`v0.10.5`,
/// `v0.11.0-rc.1`): strict SemVer 2.0 behind an optional `v`.
#[derive(Debug, Clone)]
pub struct Version(semver::Version);

/// Equal exactly when neither is newer: `0.10.5+a` is `0.10.5+b`.
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Version {}

impl Version {
    /// Read `raw`, a tag or a version: an optional `v`, then the whole of a
    /// SemVer 2.0 version — `MAJOR.MINOR.PATCH`, an optional `-pre` of
    /// non-empty dot-separated identifiers (numeric ones without leading
    /// zeros), an optional `+build` of the same shape. The grammar is
    /// checked in full, build metadata included, before anything is
    /// ignored. A shortened version (`0.10`) is refused: every keeper tag
    /// and every bundle version (Tauri requires SemVer) has three numbers,
    /// so a two-number tag is not one keeper published. `None` for anything
    /// else — a tag keeper cannot read is never guessed into a number.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let raw = raw.strip_prefix('v').unwrap_or(raw);
        semver::Version::parse(raw).ok().map(Self)
    }

    /// The version as a person reads it: the numbers, then the pre-release
    /// (the build metadata orders nothing, so it is not shown).
    pub fn display(&self) -> String {
        let v = &self.0;
        if v.pre.is_empty() {
            format!("{}.{}.{}", v.major, v.minor, v.patch)
        } else {
            format!("{}.{}.{}-{}", v.major, v.minor, v.patch, v.pre)
        }
    }
}

impl Ord for Version {
    /// SemVer precedence: the numbers numerically, a release above any
    /// pre-release of it, two pre-releases identifier by identifier (numeric
    /// ones numerically at any length, numeric below text), build metadata
    /// ignored.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp_precedence(&other.0)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The command a person runs on their Mac to put a new build on `device`.
fn install_command(device: Device) -> &'static str {
    match device {
        Device::Iphone => "bun run install:ios",
        Device::AndroidTablet | Device::AndroidPhone => "bun run install:android",
    }
}

/// What `installed` against the latest release's `tag` means for `device`.
/// An installed version or a tag keeper cannot read is a failure with its
/// sentence, never a guess either way.
pub fn verdict(installed: &str, tag: &str, device: Device) -> ReleaseCheckVm {
    let failed = |sentence: String| ReleaseCheckVm {
        outcome: ReleaseCheckOutcome::Failed,
        installed: installed.to_owned(),
        latest: None,
        sentence,
    };
    let Some(mine) = Version::parse(installed) else {
        tracing::warn!(
            installed,
            "release check: this build's version is unreadable"
        );
        return failed(format!(
            "keeper could not read its own version ({installed}), so it cannot tell whether a newer one is out."
        ));
    };
    let Some(latest) = Version::parse(tag) else {
        tracing::warn!(tag, "release check: the latest release's tag is unreadable");
        return failed(format!(
            "keeper could not read the latest release's version ({tag}), so it cannot tell whether it is newer."
        ));
    };
    let shown = latest.display();
    if latest > mine {
        ReleaseCheckVm {
            outcome: ReleaseCheckOutcome::Newer,
            installed: installed.to_owned(),
            sentence: format!(
                "keeper {shown} is available — install it from your Mac with `{}`.",
                install_command(device)
            ),
            latest: Some(shown),
        }
    } else {
        ReleaseCheckVm {
            outcome: ReleaseCheckOutcome::Current,
            installed: installed.to_owned(),
            sentence: "keeper is up to date.".to_owned(),
            latest: Some(shown),
        }
    }
}

/// The one field of GitHub's release object this reads.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// Why a request did not produce a tag: the sentence's reason, never the
/// response body.
fn latest_tag_error(reason: &str) -> String {
    format!("keeper could not check GitHub for a newer version ({reason}). Try again later.")
}

/// GET `url` and read `tag_name` out of GitHub's release object.
async fn latest_tag(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let mut response = http
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header(
            reqwest::header::USER_AGENT,
            concat!("keeper/", env!("CARGO_PKG_VERSION")),
        )
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "release check: the request failed");
            if error.is_timeout() {
                "it did not answer in time".to_owned()
            } else {
                "it could not be reached".to_owned()
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(status = status.as_u16(), "release check: GitHub refused");
        return Err(format!("it answered {}", status.as_u16()));
    }
    let too_large = || {
        tracing::warn!("release check: the answer is over the size cap");
        "its answer was too large".to_owned()
    };
    if response
        .content_length()
        .is_some_and(|len| len > MAX_BODY_BYTES as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        tracing::warn!(error = %error, "release check: the answer broke off");
        "its answer broke off".to_owned()
    })? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice::<Release>(&body)
        .map(|release| release.tag_name)
        .map_err(|error| {
            tracing::warn!(error = %error, "release check: the answer is not a release");
            "its answer was not a release".to_owned()
        })
}

/// Read the latest release from `url` and compare it with `installed`.
async fn check_at(
    http: &reqwest::Client,
    url: &str,
    installed: &str,
    device: Device,
) -> ReleaseCheckVm {
    match latest_tag(http, url).await {
        Ok(tag) => verdict(installed, &tag, device),
        Err(reason) => ReleaseCheckVm {
            outcome: ReleaseCheckOutcome::Failed,
            installed: installed.to_owned(),
            latest: None,
            sentence: latest_tag_error(&reason),
        },
    }
}

/// Ask GitHub for keeper's latest release — [`EGRESS_RELEASE_CHECK_ENDPOINT`],
/// the destination About discloses on these builds — and compare it with
/// `installed`, this build's version. Downloads and installs nothing.
pub async fn check(http: &reqwest::Client, installed: &str, device: Device) -> ReleaseCheckVm {
    check_at(http, EGRESS_RELEASE_CHECK_ENDPOINT, installed, device).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn v(raw: &str) -> Version {
        Version::parse(raw).expect("a readable version")
    }

    #[test]
    fn components_compare_as_numbers_not_text() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("v0.10.5") > v("0.10.4"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.10.10") > v("0.10.9"));
        assert_eq!(v("v0.10.5").cmp(&v("0.10.5")), Ordering::Equal);
        assert!(v("0.10.4") < v("0.10.5"));
    }

    #[test]
    fn a_shortened_version_is_not_one_keeper_publishes() {
        for raw in ["0.11", "v0.11", "1", "0.10.5.1"] {
            assert_eq!(Version::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_release_outranks_its_own_pre_releases_and_not_the_one_before() {
        assert!(v("0.11.0") > v("0.11.0-rc.1"));
        assert!(v("0.11.0-rc.1") > v("0.10.5"));
        assert!(v("0.11.0-rc.2") > v("0.11.0-rc.1"));
        assert!(v("0.11.0-rc.10") > v("0.11.0-rc.9"));
        assert!(v("0.11.0-beta") > v("0.11.0-alpha"));
        assert!(v("0.11.0-rc.1.1") > v("0.11.0-rc.1"));
        assert!(v("0.11.0-1") < v("0.11.0-rc"));
        assert_eq!(v("0.11.0+build.7").cmp(&v("0.11.0")), Ordering::Equal);
        assert_eq!(v("0.11.0+build.7"), v("0.11.0+build.8"));
    }

    #[test]
    fn numeric_pre_release_identifiers_compare_as_numbers_past_u64() {
        // 18446744073709551616 is u64::MAX + 1: still a number, so it ranks
        // by value against numbers of any length and below any text.
        let past = v("0.11.0-rc.18446744073709551616");
        assert!(past > v("0.11.0-rc.18446744073709551615"));
        assert!(past > v("0.11.0-rc.7"));
        assert!(past < v("0.11.0-rc.99999999999999999999"));
        assert!(v("0.11.0-rc.100000000000000000000") > v("0.11.0-rc.99999999999999999999"));
        assert!(v("0.11.0-99999999999999999999") < v("0.11.0-a"));
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_unreadable() {
        for raw in [
            "",
            "v",
            "latest",
            "0.10.x",
            "0..1",
            "0.10.5-",
            "v-1.0",
            "1.-2",
            // Build metadata and pre-release are checked in full before
            // either is ignored or compared.
            "v0.10.5+",
            "v0.10.5+a..b",
            "v0.10.5+a b",
            "v0.10.5-..",
            "v0.10.5-rc..1",
            "v0.10.5-rc.",
            "v0.10.5-.rc",
            "v0.10.5-rc.01",
            "v0.10.5-rc_1",
            "v00.10.5",
        ] {
            assert_eq!(Version::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_malformed_tag_for_this_very_version_is_never_up_to_date() {
        for tag in ["v0.10.5+", "v0.10.5-..", "v0.10.5-rc..1", "v0.10.5+a..b"] {
            let vm = verdict("0.10.5", tag, Device::Iphone);
            assert_eq!(vm.outcome, ReleaseCheckOutcome::Failed, "{tag}");
            assert_eq!(vm.latest, None, "{tag}");
        }
    }

    #[test]
    fn a_newer_release_says_which_and_how_to_install_it_on_this_device() {
        let iphone = verdict("0.10.5", "v0.11.0", Device::Iphone);
        assert_eq!(iphone.outcome, ReleaseCheckOutcome::Newer);
        assert_eq!(iphone.latest.as_deref(), Some("0.11.0"));
        assert_eq!(
            iphone.sentence,
            "keeper 0.11.0 is available — install it from your Mac with `bun run install:ios`."
        );
        let tablet = verdict("0.10.5", "v0.11.0", Device::AndroidTablet);
        assert!(tablet.sentence.ends_with("`bun run install:android`."));
    }

    #[test]
    fn the_same_or_an_older_release_is_up_to_date() {
        for tag in ["v0.10.5", "v0.10.4", "v0.10.5-rc.1"] {
            let vm = verdict("0.10.5", tag, Device::Iphone);
            assert_eq!(vm.outcome, ReleaseCheckOutcome::Current, "{tag}");
            assert_eq!(vm.sentence, "keeper is up to date.");
        }
    }

    #[test]
    fn an_unreadable_version_on_either_side_is_a_failure_not_a_guess() {
        let tag = verdict("0.10.5", "nightly", Device::Iphone);
        assert_eq!(tag.outcome, ReleaseCheckOutcome::Failed);
        assert_eq!(tag.latest, None);
        assert!(tag.sentence.contains("(nightly)"), "{}", tag.sentence);
        let mine = verdict("dev", "v0.11.0", Device::Iphone);
        assert_eq!(mine.outcome, ReleaseCheckOutcome::Failed);
        assert!(
            mine.sentence.contains("its own version (dev)"),
            "{}",
            mine.sentence
        );
    }

    /// A one-shot HTTP server answering `status` with `body`; returns its URL.
    async fn serve(status: &'static str, body: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
        });
        format!("http://{addr}/repos/tgorka/keeper/releases/latest")
    }

    #[tokio::test]
    async fn the_latest_release_is_read_from_github_s_release_object() {
        let url = serve(
            "200 OK",
            br#"{"tag_name":"v0.11.0","name":"keeper 0.11.0","assets":[{"name":"x"}]}"#.to_vec(),
        )
        .await;
        let vm = check_at(&reqwest::Client::new(), &url, "0.10.5", Device::Iphone).await;
        assert_eq!(vm.outcome, ReleaseCheckOutcome::Newer);
        assert_eq!(vm.latest.as_deref(), Some("0.11.0"));
    }

    #[tokio::test]
    async fn a_refused_request_is_said_with_its_status() {
        let url = serve("403 Forbidden", br#"{"message":"rate limited"}"#.to_vec()).await;
        let vm = check_at(&reqwest::Client::new(), &url, "0.10.5", Device::Iphone).await;
        assert_eq!(vm.outcome, ReleaseCheckOutcome::Failed);
        assert_eq!(
            vm.sentence,
            "keeper could not check GitHub for a newer version (it answered 403). Try again later."
        );
    }

    #[tokio::test]
    async fn an_answer_over_the_cap_is_not_read() {
        let mut body = br#"{"tag_name":"v0.11.0","pad":""#.to_vec();
        body.resize(MAX_BODY_BYTES + 16, b'x');
        body.extend_from_slice(br#""}"#);
        let url = serve("200 OK", body).await;
        let vm = check_at(&reqwest::Client::new(), &url, "0.10.5", Device::Iphone).await;
        assert_eq!(vm.outcome, ReleaseCheckOutcome::Failed);
        assert!(vm.sentence.contains("too large"), "{}", vm.sentence);
    }

    #[tokio::test]
    async fn an_answer_that_is_not_a_release_is_a_failure() {
        let url = serve("200 OK", br#"{"message":"Not Found"}"#.to_vec()).await;
        let vm = check_at(&reqwest::Client::new(), &url, "0.10.5", Device::Iphone).await;
        assert_eq!(vm.outcome, ReleaseCheckOutcome::Failed);
        assert!(vm.sentence.contains("not a release"), "{}", vm.sentence);
    }
}

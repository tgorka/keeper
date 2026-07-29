//! `git-lfs-authenticate` over SSH — the credential an `ssh://` remote has and
//! HTTPS does not (Story 34.17, AD-53).
//!
//! # Why this has to exist
//!
//! An LFS server is always HTTP, even when git is not. [`super::endpoint`]
//! therefore maps `ssh://host/owner/repo.git` onto
//! `https://host/owner/repo.git/info/lfs`, and until this module existed that
//! was the whole story — which left a hole big enough to lose files through:
//!
//! * `git push` over ssh authenticates with the user's **ssh key**, through the
//!   agent, and needs no token at all.
//! * The LFS batch API over https authenticates with a **token**, and keeper
//!   keeps exactly one per profile.
//!
//! So a folder synced over ssh pushes perfectly with an empty keychain slot and
//! then fails every LFS transfer with a 401 — the batch POST goes out bare,
//! Forgejo answers `401` with `WWW-Authenticate: Basic realm="gitea-lfs"`, the
//! refresher re-reads the same empty slot, and the unit parks. The commit is
//! already published by then, so the remote holds a pointer to content only the
//! machine that made it can supply. That is the failure this module closes, and
//! [`crate::credential`] has named the remedy from the start: Forgejo's LFS
//! `Bearer` is a JWT its own server signs, "obtainable only through
//! `git-lfs-authenticate` over SSH".
//!
//! # The protocol
//!
//! Verified against `git-lfs` v3.7.0 (`ssh/ssh.go`, `lfshttp/ssh.go`,
//! `lfshttp/client.go`) and against the server side in Forgejo/Gitea
//! `cmd/serv.go`, by observing a real `git-lfs` binary's argv:
//!
//! ```text
//! ssh [-o …] [-p <port>] [--] <user@host> "git-lfs-authenticate <path> <operation>"
//! ```
//!
//! Five details are load-bearing, and each is easy to get wrong:
//!
//! 1. **The remote command is ONE argv element.** OpenSSH joins trailing argv
//!    with single spaces and quotes nothing (`ssh.c`), so one pre-joined string
//!    and three separate arguments put identical bytes in `SSH_ORIGINAL_COMMAND`.
//!    Pre-joining is the form git-lfs uses.
//! 2. **`--` before the host, but only when the host starts with `-`.** Without
//!    it a remote URL of `ssh://-oProxyCommand=…@host/r.git` is arbitrary
//!    command execution. git-lfs guards this with the same rule.
//! 3. **The leading `/` survives for `ssh://` and is stripped for scp-style.**
//!    `.git` is never added or removed. Both forges `TrimPrefix("/")` and
//!    `TrimSuffix(".git")`, so either spelling works — but this is the wire form
//!    git-lfs sends, and matching it means never debugging a path difference.
//! 4. **`href` is the API root VERBATIM.** `/info/lfs` is *not* appended to it;
//!    the server already put it there (`cmd/serv.go` builds
//!    `<AppURL><owner>/<repo>.git/info/lfs`). Appending again is a plausible-
//!    looking 404.
//! 5. **The response must be exactly one JSON value** with no banner either
//!    side. A login banner is therefore a configuration failure and is reported
//!    as one, rather than retried six times the way git-lfs does.
//!
//! # Expiry, and the trap in it
//!
//! The wire format has `expires_in`/`expires_at`, and **Forgejo and Gitea send
//! neither** — their `LFSTokenResponse` carries only `header` and `href`. Their
//! JWT nonetheless expires, on `LFS_HTTP_AUTH_EXPIRY`, default **24 hours**.
//! git-lfs reads an absent expiry as "never stale", which for a process that
//! lives for days is simply wrong; upstream's own escape hatch,
//! `lfs.defaulttokenttl`, exists for exactly these servers. So a silent server
//! gets [`DEFAULT_TTL_MS`] here rather than eternity.
//!
//! # Non-interactivity
//!
//! git-lfs sets **no** ssh options at all, so stock git-lfs can block forever on
//! a passphrase or an unknown-host prompt. In a background sync daemon that is a
//! hang, not an error, so [`authenticate`] sets `BatchMode=yes` and friends. The
//! user's `~/.ssh/config` is deliberately still honoured — the same
//! configuration their `git push` already works through — with our options
//! appended, which win because OpenSSH takes the first value it is given.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use url::Url;

use crate::error::{Result, SyncError};

/// How long a credential is trusted when the server does not say.
///
/// Ten minutes, against a server-side default of 24 hours. The cost of being
/// wrong in one direction is a wasted ssh round trip; in the other it is a 401
/// mid-transfer on a multi-gigabyte object, so the asymmetry decides it. One
/// handshake per ten minutes per profile is not a cost worth optimizing.
pub const DEFAULT_TTL_MS: i64 = 10 * 60 * 1_000;

/// Treat a credential as stale slightly before it nominally dies, so a request
/// cannot lose a race against the server's clock. git-lfs uses the same 5 s.
const EXPIRY_SLACK_MS: i64 = 5_000;

/// Wall-clock ceiling on the whole handshake.
///
/// `ConnectTimeout` covers the TCP connect and key exchange only, so a server
/// that accepts the connection and then wedges would hang forever without this.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);

/// `ConnectTimeout` handed to ssh itself.
const CONNECT_TIMEOUT_SECS: u32 = 10;

/// Bound on captured stderr, which is surfaced in an error message and a log.
///
/// A misconfigured server can emit a megabyte-long banner, and an error string
/// is not a place to put it.
const MAX_STDERR_BYTES: usize = 2 * 1024;

/// Which half of the protocol a credential is for.
///
/// Not a bool: the two spellings go on the wire verbatim, and the forges reject
/// anything that is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operation {
    Upload,
    Download,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
        }
    }
}

/// An ssh remote, decomposed into the three pieces the handshake needs.
///
/// Deliberately *not* [`super::endpoint`]'s `RemoteParts`: that one strips
/// userinfo and maps the scheme onto HTTP, which is right for building an
/// endpoint and useless here — `git@` is exactly the part ssh needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshRemote {
    /// `host` or `user@host`, passed to ssh as one argument.
    pub user_host: String,
    /// The explicit port, if the URL carried one. Never a default.
    pub port: Option<String>,
    /// The repository operand, in the form git-lfs sends: leading `/` kept for
    /// `ssh://`, stripped for scp-style.
    pub path: String,
}

impl SshRemote {
    /// Decompose an ssh remote, or `None` when this is not one.
    ///
    /// `None` is the ordinary answer for an `https://` remote and is not an
    /// error: it means "there is no ssh handshake to attempt here", and the
    /// caller falls back to the derived endpoint.
    pub fn parse(remote_url: &str) -> Option<Self> {
        let trimmed = remote_url.trim();
        let (user_host, port, path) = match trimmed.split_once("://") {
            Some(("ssh", rest)) => {
                // No path at all: there is no repository to name.
                let idx = rest.find('/')?;
                let (authority, path) = (&rest[..idx], &rest[idx..]);
                let (host, port) = split_port(authority);
                // The leading slash is kept, which is what git-lfs sends for a
                // `ssh://` URL.
                (host.to_owned(), port, path.to_owned())
            }
            // Any other explicit scheme is not ssh.
            Some(_) => return None,
            // scp-style `[user@]host:path`. The two guards are
            // `super::endpoint`'s, for the same reasons: a `/` before the colon
            // means this is a path, and a one-character authority is a Windows
            // drive letter.
            None => match trimmed.split_once(':') {
                Some((authority, path)) if !authority.contains('/') && authority.len() > 1 => {
                    // git-lfs strips the leading slash here and only here.
                    (
                        authority.to_owned(),
                        None,
                        path.trim_start_matches('/').to_owned(),
                    )
                }
                _ => return None,
            },
        };

        if user_host.is_empty() || path.is_empty() || path == "/" {
            return None;
        }
        Some(Self {
            user_host,
            port,
            path,
        })
    }

    /// The cache key for one credential.
    ///
    /// The same four components git-lfs keys on, minus its use of the HTTP
    /// method in place of the operation — the operation is what the server
    /// actually scopes the token to, so it is the honest key.
    pub fn cache_key(&self, operation: Operation) -> String {
        format!(
            "{}//{}//{}//{}",
            self.user_host,
            self.port.as_deref().unwrap_or(""),
            self.path,
            operation.as_str()
        )
    }

    /// Is the path representable in this protocol at all?
    ///
    /// The server shell-splits `SSH_ORIGINAL_COMMAND` and git-lfs quotes
    /// nothing, so a path containing whitespace cannot be transmitted: the
    /// server would read `git-lfs-authenticate /own er/r.git upload` as four
    /// words and take `er/r.git` for the operation. Refusing up front beats a
    /// baffling server-side rejection. The control characters are refused for
    /// the obvious reason.
    fn is_transmittable(&self) -> bool {
        !self
            .path
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    }
}

/// Split `host[:port]`, tolerating a bracketed IPv6 literal.
fn split_port(authority: &str) -> (&str, Option<String>) {
    // `[::1]:22` — the port is only the colon AFTER the bracket.
    if let Some(end) = authority.rfind(']') {
        return match authority[end..].split_once(':') {
            Some((_, port)) if !port.is_empty() => (&authority[..end + 1], Some(port.to_owned())),
            _ => (authority, None),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port.to_owned()))
        }
        _ => (authority, None),
    }
}

/// What the server said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A credential, and optionally the endpoint to spend it at.
    Granted(Credential),
    /// This server does not serve LFS over ssh at all: `git-lfs-authenticate`
    /// is not a command it knows. Not a failure — the caller falls back to the
    /// derived HTTPS endpoint and whatever token is stored.
    NoSshLfs,
}

/// One minted credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// The LFS API root, **verbatim**. `None` when the server did not name one,
    /// in which case the derived endpoint stands.
    pub href: Option<Url>,
    /// The `Authorization` header value the server wants back, if it sent one.
    pub authorization: Option<String>,
    /// Seconds this credential is good for, when the server said. `None` means
    /// the server was silent and [`DEFAULT_TTL_MS`] applies.
    pub expires_in_secs: Option<i64>,
}

/// The response body, exactly as `git-lfs`'s `sshAuthResponse` reads it.
///
/// Every field is optional, and unknown fields are ignored — both are the
/// documented contract, and the forges send only two of the four.
#[derive(Debug, Deserialize)]
struct AuthResponse {
    #[serde(default)]
    href: Option<String>,
    /// `null` is legal and means absent, which is why this is an `Option` around
    /// the map rather than a `#[serde(default)]` map.
    #[serde(default)]
    header: Option<BTreeMap<String, String>>,
    /// A whole number of **seconds**, never a string, and possibly negative.
    #[serde(default)]
    expires_in: Option<i64>,
}

/// Run `git-lfs-authenticate` and read what comes back.
///
/// `ssh` is taken from `PATH`, which is where gitoxide's own ssh transport finds
/// it for the push leg — so a remote whose push works has an `ssh` this can use.
pub async fn authenticate(remote: &SshRemote, operation: Operation) -> Result<Answer> {
    authenticate_with(remote, operation, std::path::Path::new("ssh")).await
}

/// As [`authenticate`], with the program to run named explicitly.
///
/// Exists for the integration test, and it is a seam rather than a setting: the
/// workspace forbids `unsafe_code`, `std::env::set_var` is `unsafe`, and so a
/// test cannot put a stand-in `ssh` on `PATH` to observe the argv this builds.
/// Observing it is not optional — every mistake in this wire format comes back
/// as a server saying "Invalid repository path" with no indication of which half
/// was wrong.
pub async fn authenticate_with(
    remote: &SshRemote,
    operation: Operation,
    program: &std::path::Path,
) -> Result<Answer> {
    if !remote.is_transmittable() {
        return Err(SyncError::Config(format!(
            "the repository path `{}` cannot be sent over ssh: git-lfs-authenticate \
             takes it as one shell word, so a path containing whitespace has no \
             representation. Set `lfs.url` in the repository's .lfsconfig to name \
             the LFS server directly.",
            remote.path
        )));
    }

    let mut command = Command::new(program);
    // Every one of these is a prompt git-lfs leaves open. A sync daemon has no
    // terminal to answer one on, so a prompt is an indefinite hang.
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("NumberOfPasswordPrompts=0")
        .arg("-o")
        .arg(format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"));
    if let Some(port) = &remote.port {
        // Before the host, as ssh requires.
        command.arg("-p").arg(port);
    }
    // Guard against a remote URL smuggling ssh options through the host field.
    if remote.user_host.starts_with('-') {
        command.arg("--");
    }
    command.arg(&remote.user_host);
    // ONE argument: ssh joins trailing argv with spaces and quotes nothing, so
    // splitting this would change nothing on the wire and only invite someone to
    // "fix" it by adding quotes the server would then have to strip.
    command.arg(format!(
        "git-lfs-authenticate {} {}",
        remote.path,
        operation.as_str()
    ));
    // The GUI askpass helper is the one prompt `BatchMode` does not close.
    command
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env_remove("SSH_ASKPASS")
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // So the wall-clock kill below takes the whole process, not a shell that
        // outlives its child.
        .kill_on_drop(true);

    let output = match tokio::time::timeout(COMMAND_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            // No `ssh` binary. Not "the server has no LFS": the handshake could
            // not be attempted, and saying otherwise would send the caller down
            // an unauthenticated path that fails less legibly.
            return Err(SyncError::Config(
                "`ssh` is not on PATH, so keeper cannot ask this remote for an LFS \
                 credential. Install an ssh client, or set `lfs.url` in the \
                 repository's .lfsconfig to an LFS server keeper can reach over https."
                    .to_owned(),
            ));
        }
        Ok(Err(err)) => {
            return Err(SyncError::io(
                "run git-lfs-authenticate",
                std::path::PathBuf::from("ssh"),
                err,
            ))
        }
        Err(_elapsed) => {
            return Err(SyncError::Network {
                host: remote.user_host.clone(),
                reason: format!(
                    "git-lfs-authenticate did not answer within {}s",
                    COMMAND_TIMEOUT.as_secs()
                ),
            })
        }
    };

    let stderr = truncated(&output.stderr);
    if !output.status.success() {
        if is_command_absent(output.status.code(), &stderr) {
            tracing::debug!(
                host = remote.user_host,
                "the remote has no git-lfs-authenticate, so LFS falls back to https"
            );
            return Ok(Answer::NoSshLfs);
        }
        // Everything else is a real refusal, and the stderr is the only
        // diagnostic the forge gives: Forgejo says "Unknown git command" when
        // LFS is switched off, Gitea says "LFS Server is not enabled", and
        // neither is distinguishable any other way.
        //
        // `Config` rather than `Auth`, deliberately. `Auth` means "the token was
        // rejected" and its message says to replace the token, which is the
        // wrong instruction for every one of these: the server either does not
        // serve LFS at all, or does not serve it to this key. Both are
        // Permanent, so the unit parks either way and the message reaches the
        // file row that needs it — but only this one tells the truth about what
        // to change.
        return Err(SyncError::Config(format!(
            "{} refused to issue an LFS credential over ssh{}. It said: {}",
            remote.user_host,
            output
                .status
                .code()
                .map(|code| format!(" (exit {code})"))
                .unwrap_or_default(),
            if stderr.is_empty() {
                "nothing. Check that the remote is a forge with LFS enabled"
            } else {
                stderr.as_str()
            }
        )));
    }

    parse_response(&output.stdout, remote, &stderr).map(Answer::Granted)
}

/// Did the *server* not know the command, as opposed to refusing it?
///
/// Two independent signals, because no forge provides a machine-readable one. A
/// login shell reports a missing command with exit **127**, which is the
/// reliable half. The substrings are the same list git-lfs added upstream, for
/// the servers that answer 1 with prose — Gerrit among them.
fn is_command_absent(code: Option<i32>, stderr: &str) -> bool {
    if code == Some(127) {
        return true;
    }
    let lower = stderr.to_ascii_lowercase();
    [
        "git-lfs-authenticate: not found",
        "git-lfs-authenticate: command not found",
        "git-lfs-authenticate: no such file",
        "command not found: git-lfs-authenticate",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Read the JSON body.
///
/// Strict on purpose, matching git-lfs: exactly one value, nothing but
/// whitespace around it. A login banner therefore fails here — and it *should*,
/// because a banner means every LFS operation on this remote is broken until
/// someone silences it, which is a thing to be told once rather than retried.
fn parse_response(stdout: &[u8], remote: &SshRemote, stderr: &str) -> Result<Credential> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| malformed(remote, "the answer was not UTF-8", stderr))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(malformed(remote, "the answer was empty", stderr));
    }
    let parsed: AuthResponse = serde_json::from_str(trimmed).map_err(|err| {
        malformed(
            remote,
            &format!("the answer was not the expected JSON ({err})"),
            stderr,
        )
    })?;

    let href = match parsed.href {
        Some(raw) if !raw.is_empty() => {
            let url = Url::parse(&raw)
                .map_err(|_| malformed(remote, "the `href` it named is not a URL", stderr))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(malformed(
                    remote,
                    "the `href` it named is not an http(s) URL",
                    stderr,
                ));
            }
            // Verbatim. `/info/lfs` belongs to the DERIVATION, and the server
            // has already included it.
            Some(url)
        }
        _ => None,
    };

    Ok(Credential {
        // Case-insensitively `Authorization`, because a header name is.
        authorization: parsed.header.as_ref().and_then(|header| {
            header
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.clone())
        }),
        href,
        expires_in_secs: parsed.expires_in,
    })
}

/// A well-formed refusal beats a parse error nobody can act on, so the message
/// names the remote and carries whatever the server said on stderr.
fn malformed(remote: &SshRemote, what: &str, stderr: &str) -> SyncError {
    let mut reason = format!("asked {} for an LFS credential and {what}", remote.path);
    if !stderr.is_empty() {
        // A banner is the overwhelmingly common cause, and it lands on stderr.
        reason.push_str(&format!(". The remote also said: {stderr}"));
    }
    SyncError::Config(reason)
}

/// Collapse captured stderr into one bounded line.
fn truncated(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    if joined.len() <= MAX_STDERR_BYTES {
        return joined;
    }
    // On a char boundary, or `String::truncate` panics.
    let mut end = MAX_STDERR_BYTES;
    while end > 0 && !joined.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &joined[..end])
}

impl Credential {
    /// When this credential must be re-derived, as an absolute millisecond.
    ///
    /// The server's own figure when it gave one, [`DEFAULT_TTL_MS`] when it did
    /// not — never "forever", which is what git-lfs assumes and what makes a
    /// long-lived process quietly start failing 24 hours in. The slack is
    /// subtracted here so every reader gets it.
    pub fn expires_ms(&self, now_ms: i64) -> i64 {
        let ttl_ms = match self.expires_in_secs {
            // A server that hands out an already-expired credential is telling
            // us to re-derive every time. Honour it rather than clamping to the
            // default, which would cache a credential it disowned.
            Some(secs) => secs.saturating_mul(1_000),
            None => DEFAULT_TTL_MS,
        };
        now_ms
            .saturating_add(ttl_ms)
            .saturating_sub(EXPIRY_SLACK_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_operand_form_matches_what_git_lfs_sends() {
        // The table observed out of a real `git-lfs` 3.7.0 argv. The leading
        // slash survives for `ssh://` and is stripped for scp-style, and `.git`
        // is never touched — get either wrong and the forge answers "Invalid
        // repository path" with no hint which half was wrong.
        let cases = [
            (
                "ssh://git@host.example/owner/repo.git",
                "git@host.example",
                None,
                "/owner/repo.git",
            ),
            (
                "ssh://git@host.example:2222/owner/repo.git",
                "git@host.example",
                Some("2222"),
                "/owner/repo.git",
            ),
            (
                "git@host.example:owner/repo.git",
                "git@host.example",
                None,
                "owner/repo.git",
            ),
            (
                "ssh://git@host.example/owner/repo",
                "git@host.example",
                None,
                "/owner/repo",
            ),
            (
                "ssh://host.example/owner/repo.git",
                "host.example",
                None,
                "/owner/repo.git",
            ),
        ];
        for (url, user_host, port, path) in cases {
            let parsed = SshRemote::parse(url).unwrap_or_else(|| panic!("{url} is an ssh remote"));
            assert_eq!(parsed.user_host, user_host, "{url}");
            assert_eq!(parsed.port.as_deref(), port, "{url}");
            assert_eq!(parsed.path, path, "{url}");
        }
    }

    #[test]
    fn a_remote_that_is_not_ssh_has_no_handshake_to_attempt() {
        // `None` is the ordinary answer here, not a failure: these remotes reach
        // their LFS server over https with the stored token, exactly as before.
        for url in [
            "https://git.example/owner/repo.git",
            "http://git.example/owner/repo.git",
            "git://git.example/owner/repo.git",
            // A local path, and a Windows drive letter that must not be read as
            // `scp`-style `c:` — the same two guards `endpoint` applies.
            "/srv/git/repo.git",
            "C:/repos/thing.git",
            // An ssh URL naming no repository.
            "ssh://git@host.example",
        ] {
            assert!(SshRemote::parse(url).is_none(), "{url}");
        }
    }

    #[test]
    fn an_ipv6_literal_keeps_its_brackets_and_finds_its_port() {
        let plain = SshRemote::parse("ssh://git@[fe80::1]/o/r.git").expect("ipv6");
        assert_eq!(plain.user_host, "git@[fe80::1]");
        assert_eq!(plain.port, None);

        let ported = SshRemote::parse("ssh://git@[fe80::1]:2222/o/r.git").expect("ipv6 with port");
        assert_eq!(ported.user_host, "git@[fe80::1]");
        assert_eq!(ported.port.as_deref(), Some("2222"));
    }

    #[test]
    fn a_path_with_whitespace_is_refused_rather_than_mangled() {
        // The server shell-splits the command and git-lfs quotes nothing, so
        // this path is genuinely unrepresentable. Failing here names the cause;
        // sending it produces "Invalid repository path: er/r.git".
        let remote = SshRemote::parse("ssh://git@host.example/own er/r.git").expect("parses");
        assert!(!remote.is_transmittable());
        assert!(SshRemote::parse("ssh://git@host.example/owner/r.git")
            .expect("parses")
            .is_transmittable());
    }

    #[test]
    fn the_cache_key_separates_upload_from_download_and_host_from_port() {
        let a = SshRemote::parse("ssh://git@host.example/o/r.git").expect("parse");
        let b = SshRemote::parse("ssh://git@host.example:2222/o/r.git").expect("parse");
        assert_ne!(
            a.cache_key(Operation::Upload),
            a.cache_key(Operation::Download),
            "a download token is not a write token"
        );
        assert_ne!(
            a.cache_key(Operation::Upload),
            b.cache_key(Operation::Upload)
        );
    }

    /// The one detail here with a security consequence, and it went untested
    /// until a spec review said so out loud.
    ///
    /// A remote URL is user data. Without the `--`, a host of
    /// `-oProxyCommand=…` is handed to ssh as an OPTION rather than a
    /// destination, and `ProxyCommand` runs a shell command — so a crafted
    /// remote would be arbitrary code execution the moment a large file was
    /// staged. git-lfs guards this with the same rule, and the guard is worth
    /// nothing if it is only asserted by reading it.
    #[test]
    fn a_host_that_looks_like_an_ssh_option_is_separated_from_the_options() {
        let hostile =
            SshRemote::parse("ssh://-oProxyCommand=touch${IFS}pwned@host.example/o/r.git")
                .expect("it parses — refusing it is not the defence, `--` is");
        // Userinfo is NOT stripped here, unlike the endpoint derivation: ssh
        // needs `user@host`, so the hostile string survives into the argument
        // and the separator is the only thing standing between it and ssh's
        // option parser.
        assert!(
            hostile.user_host.starts_with('-'),
            "the guard's trigger condition must actually hold for this input, got: {}",
            hostile.user_host
        );

        // An ordinary remote must NOT gain the separator: `--` before a normal
        // destination is harmless but it would mean the condition was inverted,
        // and the inverse mistake is the dangerous one.
        let ordinary = SshRemote::parse("ssh://git@host.example/o/r.git").expect("parse");
        assert!(!ordinary.user_host.starts_with('-'));
    }

    #[test]
    fn the_forge_response_shape_round_trips() {
        // Byte-for-byte what Forgejo's `LFSTokenResponse` serializes: two fields
        // and no expiry at all.
        let remote = SshRemote::parse("ssh://git@host.example/o/r.git").expect("parse");
        let body = br#"{"header":{"Authorization":"Bearer eyJhbGc"},"href":"https://host.example/o/r.git/info/lfs"}"#;
        let credential = parse_response(body, &remote, "").expect("parse");
        assert_eq!(
            credential.authorization.as_deref(),
            Some("Bearer eyJhbGc"),
            "the Bearer JWT is the whole point: it is the one scheme keeper \
             cannot mint itself"
        );
        assert_eq!(
            credential.href.as_ref().map(Url::as_str),
            Some("https://host.example/o/r.git/info/lfs"),
            "verbatim — `/info/lfs` is already in it and must not be appended again"
        );
        assert_eq!(credential.expires_in_secs, None);
    }

    #[test]
    fn a_silent_server_gets_a_conservative_ttl_not_eternity() {
        // Forgejo and Gitea send no expiry and expire the JWT in 24 h anyway.
        // Reading silence as "never stale", which git-lfs does, is how a daemon
        // starts failing a day after it last restarted.
        let silent = Credential {
            href: None,
            authorization: Some("Bearer x".to_owned()),
            expires_in_secs: None,
        };
        assert_eq!(
            silent.expires_ms(1_000),
            1_000 + DEFAULT_TTL_MS - EXPIRY_SLACK_MS
        );

        // A server that does speak is believed, in both directions.
        let short = Credential {
            expires_in_secs: Some(60),
            ..silent.clone()
        };
        assert_eq!(short.expires_ms(1_000), 1_000 + 60_000 - EXPIRY_SLACK_MS);

        let already_dead = Credential {
            expires_in_secs: Some(-5),
            ..silent
        };
        assert!(
            already_dead.expires_ms(1_000) < 1_000,
            "a credential the server disowns must not be cached"
        );
    }

    #[test]
    fn optional_fields_are_optional_and_extra_ones_are_ignored() {
        let remote = SshRemote::parse("ssh://git@host.example/o/r.git").expect("parse");
        let empty = parse_response(b"{}", &remote, "").expect("an empty object is legal");
        assert_eq!(empty.authorization, None);
        assert_eq!(empty.href, None);

        // `header: null` is legal and means absent.
        let null_header =
            parse_response(br#"{"header":null,"expires_in":600}"#, &remote, "").expect("null");
        assert_eq!(null_header.authorization, None);
        assert_eq!(null_header.expires_in_secs, Some(600));

        // A newer server's extra fields must not break an older client.
        let extra = parse_response(
            br#"{"href":"https://h/x","tomorrow":{"nested":true}}"#,
            &remote,
            "",
        )
        .expect("unknown fields are ignored");
        assert_eq!(extra.href.as_ref().map(Url::as_str), Some("https://h/x"));

        // The header name is a header name, so its case does not matter.
        let lowercased =
            parse_response(br#"{"header":{"authorization":"Bearer y"}}"#, &remote, "").expect("ok");
        assert_eq!(lowercased.authorization.as_deref(), Some("Bearer y"));
    }

    #[test]
    fn a_login_banner_is_a_configuration_error_not_a_retry() {
        // git-lfs retries this six times and then reports a JSON parse error. A
        // banner will still be there on the sixth attempt, so it is reported
        // once, with the banner itself included — because the banner IS the fix.
        let remote = SshRemote::parse("ssh://git@host.example/o/r.git").expect("parse");
        let err = parse_response(b"Welcome to host!\n{\"href\":\"https://h/x\"}", &remote, "")
            .expect_err("a banner before the JSON is fatal");
        assert!(matches!(err, SyncError::Config(_)), "got {err:?}");
        let message = err.to_string();
        assert!(message.contains("/o/r.git"), "got: {message}");

        for body in [
            &b""[..],
            // Two documents.
            br#"{"href":"https://h/x"}{"href":"https://h/y"}"#,
            // `expires_in` must be a number.
            br#"{"expires_in":"600"}"#,
        ] {
            assert!(parse_response(body, &remote, "").is_err(), "{body:?}");
        }
    }

    #[test]
    fn an_href_that_is_not_an_http_url_is_refused() {
        let remote = SshRemote::parse("ssh://git@host.example/o/r.git").expect("parse");
        for body in [
            &br#"{"href":"host.example/o/r.git/info/lfs"}"#[..],
            br#"{"href":"file:///etc/passwd"}"#,
            br#"{"href":"ssh://host.example/o/r.git"}"#,
        ] {
            assert!(
                parse_response(body, &remote, "").is_err(),
                "a non-http href must not become an endpoint: {body:?}"
            );
        }
    }

    #[test]
    fn only_a_missing_command_is_read_as_no_ssh_lfs() {
        // Exit 127 is the reliable signal; the substrings cover the servers that
        // answer 1 with prose. Everything else is a refusal, and reading a
        // refusal as "no LFS here" would silently downgrade to an
        // unauthenticated https attempt that fails less legibly.
        assert!(is_command_absent(Some(127), ""));
        assert!(is_command_absent(
            Some(1),
            "fatal: Gerrit Code Review: git-lfs-authenticate: not found"
        ));
        assert!(is_command_absent(
            Some(1),
            "bash: git-lfs-authenticate: command not found"
        ));

        // Forgejo with LFS switched off, and Gitea with the same.
        assert!(!is_command_absent(Some(1), "Forgejo: Unknown git command"));
        assert!(!is_command_absent(Some(1), "LFS Server is not enabled"));
        // A permission refusal is emphatically not an absent command.
        assert!(!is_command_absent(
            Some(1),
            "Gitea: User 2 has no \"write\" permission for owner/repo"
        ));
        assert!(!is_command_absent(None, ""));
    }

    #[test]
    fn captured_stderr_is_bounded() {
        let long = vec![b'x'; MAX_STDERR_BYTES * 2];
        let out = truncated(&long);
        assert!(out.len() <= MAX_STDERR_BYTES + 4, "got {} bytes", out.len());
        assert!(out.ends_with('…'));
        // Multi-line output collapses rather than smuggling newlines into a log.
        assert_eq!(truncated(b"one\n\ntwo\n"), "one; two");
    }
}

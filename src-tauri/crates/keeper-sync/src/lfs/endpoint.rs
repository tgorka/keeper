//! LFS server discovery (Story 25.2, AD-46, AD-53).
//!
//! The endpoint is *derived* from the git remote unless the repository
//! overrides it. Both upstream git-lfs (`lfsapi/endpoint.go`) and Forgejo
//! (`modules/lfs/endpoint.go`) implement the identical algorithm, and the
//! research pass confirmed them line for line — so this module reproduces it
//! rather than inventing anything:
//!
//! 1. trim the trailing `/`;
//! 2. append `/info/lfs` when the path already ends in `.git`, otherwise
//!    `.git/info/lfs`;
//! 3. `git://` becomes `https://`; `ssh://` and scp-style `user@host:path`
//!    become `https://` with the userinfo stripped.
//!
//! Overrides, in decreasing precedence: `remote.<name>.lfsurl`, then `lfs.url`,
//! then the derivation. An override is the LFS server root **verbatim** — the
//! `.git/info/lfs` suffix is a property of the *derivation*, not of endpoints.
//! Forgejo repositories in the wild set these keys, so this is not optional.
//!
//! # Credentials
//!
//! Userinfo is stripped from every scheme, including `https://`, which is a
//! deliberate deviation from upstream. AD-53 makes `SyncPlatform::secret_*` the
//! only credential source, and it means the returned [`Url`] is always safe to
//! put in a log line or an error message.

use url::Url;

use crate::error::{Result, SyncError};

/// Derive the LFS endpoint from a git remote URL.
pub fn derive(remote_url: &str) -> Result<Url> {
    let parts = RemoteParts::split(remote_url)?;
    // Go's `path.Ext(p) == ".git"`, spelled the way Rust reads.
    let path = if parts.path.ends_with(".git") {
        format!("{}/info/lfs", parts.path)
    } else {
        format!("{}.git/info/lfs", parts.path)
    };
    parts.build(&path)
}

/// The endpoint the repository's own `.lfsconfig` names, if it names one.
///
/// Split out from [`resolve`] because an override is not merely a different
/// answer, it is a different *authority*: a repository that names its LFS server
/// explicitly has settled the question, so the ssh handshake in
/// [`super::ssh`] must not run and second-guess it. A caller that only wants an
/// endpoint still wants [`resolve`].
///
/// `lfsconfig` is the *contents* of the repository's root `.lfsconfig`, which
/// git-lfs reads as an additional git-config file. `remote_name` selects the
/// `remote.<name>.lfsurl` key — normally `"origin"`.
pub fn override_url(lfsconfig: Option<&str>, remote_name: &str) -> Result<Option<Url>> {
    let Some(text) = lfsconfig else {
        return Ok(None);
    };
    let config = GitConfig::parse(text);
    // Most specific first: a key naming this one remote outranks the
    // repository-wide one.
    if let Some(raw) = config.get(&format!("remote.{remote_name}.lfsurl")) {
        return parse_override(raw).map(Some);
    }
    if let Some(raw) = config.get("lfs.url") {
        return parse_override(raw).map(Some);
    }
    Ok(None)
}

/// Resolve the endpoint, honouring `.lfsconfig` overrides.
pub fn resolve(remote_url: &str, lfsconfig: Option<&str>, remote_name: &str) -> Result<Url> {
    match override_url(lfsconfig, remote_name)? {
        Some(url) => Ok(url),
        None => derive(remote_url),
    }
}

/// Append a path *under* an LFS endpoint.
///
/// Deliberately not [`Url::join`]: that resolves an RFC 3986 relative
/// reference, which **replaces** the last path segment. `…/info/lfs` joined
/// with `objects/batch` yields `…/info/objects/batch` — a 404 that looks
/// plausible enough in a log to cost an afternoon.
pub fn sub_path(endpoint: &Url, path: &str) -> Result<Url> {
    let mut out = endpoint.clone();
    {
        let mut segments = out.path_segments_mut().map_err(|()| {
            // Host omitted here on purpose: the caller already knows it, and a
            // cannot-be-a-base URL is a configuration bug, not a host problem.
            SyncError::Config("LFS endpoint cannot have a path appended".to_owned())
        })?;
        // A trailing empty segment (from a `…/lfs/` endpoint) would otherwise
        // produce a double slash.
        segments.pop_if_empty();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            segments.push(part);
        }
    }
    Ok(out)
}

/// An explicit `lfsurl`/`lfs.url` is the server root as written; only the
/// scheme mapping and userinfo stripping still apply.
fn parse_override(raw: &str) -> Result<Url> {
    let parts = RemoteParts::split(raw)?;
    let path = parts.path.clone();
    parts.build(&path)
}

/// A remote URL decomposed into the three pieces the derivation needs.
struct RemoteParts {
    /// Already mapped onto an HTTP scheme.
    scheme: &'static str,
    /// `host` or `host:port`, never userinfo.
    authority: String,
    /// Always starts with `/`, never ends with one, never carries a query.
    path: String,
}

impl RemoteParts {
    fn split(raw: &str) -> Result<Self> {
        // Upstream strips exactly one trailing slash. Stripping a run of them
        // is a strict superset: the single-slash case — the only one that
        // occurs in practice — is identical, and a typo'd `repo//` becomes a
        // working endpoint instead of a guaranteed 404.
        let trimmed = raw.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(SyncError::Config("remote URL is empty".to_owned()));
        }

        let (scheme, authority, path) = match trimmed.split_once("://") {
            Some((scheme, rest)) => {
                let (authority, path) = split_authority(rest);
                (scheme, authority, path)
            }
            // scp-style `[user@]host:path`. Git accepts no port here, so
            // everything after the first colon is the repository path.
            // `authority.len() > 1` rejects a Windows drive letter (`C:/repos`),
            // which is a local path and would otherwise become `https://c/…`.
            None => match trimmed.split_once(':') {
                Some((authority, path)) if !authority.contains('/') && authority.len() > 1 => {
                    ("ssh", authority, path)
                }
                // A bare filesystem path. There is no LFS server behind one,
                // and pretending otherwise produces a confusing 404 later.
                _ => {
                    return Err(SyncError::Config(
                        "remote is a local path and has no LFS server".to_owned(),
                    ))
                }
            },
        };

        let scheme = match scheme {
            // Plain HTTP is preserved rather than upgraded: an intranet Forgejo
            // on http is a legitimate deployment, and silently switching to
            // https would fail with a TLS error nobody can explain.
            "http" => "http",
            "https" | "git" | "ssh" => "https",
            other => {
                // Scheme only — a remote URL may embed a token in its userinfo
                // and error messages are logged (AD-53).
                return Err(SyncError::Config(format!(
                    "remote scheme `{other}` has no LFS transport"
                )));
            }
        };

        // Strip userinfo. For `ssh://` this is upstream's own rule; for
        // `https://` it is ours (AD-53).
        let authority = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        if authority.is_empty() {
            return Err(SyncError::Config("remote URL has no host".to_owned()));
        }

        // A query or fragment is not part of a repository path; leaving one in
        // would embed it in the middle of the endpoint path.
        let path = path
            .split(['?', '#'])
            .next()
            .unwrap_or(path)
            .trim_end_matches('/');
        let path = if path.is_empty() {
            String::new()
        } else if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };

        Ok(Self {
            scheme,
            authority: authority.to_owned(),
            path,
        })
    }

    fn build(&self, path: &str) -> Result<Url> {
        let raw = format!("{}://{}{}", self.scheme, self.authority, path);
        Url::parse(&raw).map_err(|_| {
            SyncError::Config(format!(
                "cannot build an LFS endpoint for host `{}`",
                self.authority
            ))
        })
    }
}

/// Split `host[:port]/path…` into authority and path.
fn split_authority(rest: &str) -> (&str, &str) {
    match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    }
}

/// A minimal git-config reader, sufficient for `.lfsconfig`.
///
/// `.lfsconfig` is git-config syntax: `[section]` / `[section "subsection"]`
/// headers, `key = value` bodies, `#`/`;` comments. Only the fragment this
/// module needs is implemented — no includes, no multi-valued keys, no
/// `--type` coercion — because the alternative is dragging in a full config
/// parser to read at most two string keys.
#[derive(Debug, Default, Clone)]
pub struct GitConfig {
    /// Normalized `section[.subsection].key` → value, in file order.
    entries: Vec<(String, String)>,
}

impl GitConfig {
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        let mut section = String::new();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = parse_section_header(header);
                continue;
            }
            // A valueless key means boolean true in git-config. No LFS key we
            // read is boolean, so skipping is both correct and cheaper than
            // representing it.
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            if key.is_empty() || section.is_empty() {
                continue;
            }
            entries.push((format!("{section}.{key}"), clean_value(value)));
        }

        Self { entries }
    }

    /// Look up `section[.subsection].key`. The last definition wins, as git does.
    pub fn get(&self, key: &str) -> Option<&str> {
        let key = normalize_key(key);
        self.entries
            .iter()
            .rev()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, value)| value.as_str())
    }
}

/// `remote "origin"` → `remote.origin`; `lfs` → `lfs`.
fn parse_section_header(header: &str) -> String {
    let header = header.trim();
    match header.split_once(char::is_whitespace) {
        Some((section, subsection)) => format!(
            "{}.{}",
            section.trim().to_ascii_lowercase(),
            // git-config(1): section and key names fold case, **subsection
            // names do not**. A remote called `Origin` is not `origin`.
            subsection.trim().trim_matches('"')
        ),
        None => header.to_ascii_lowercase(),
    }
}

/// Apply the same case folding to a lookup key that [`GitConfig::parse`]
/// applied when storing it.
fn normalize_key(key: &str) -> String {
    let (Some(first), Some(last)) = (key.find('.'), key.rfind('.')) else {
        return key.to_ascii_lowercase();
    };
    if first == last {
        return key.to_ascii_lowercase();
    }
    format!(
        "{}.{}.{}",
        key[..first].to_ascii_lowercase(),
        &key[first + 1..last],
        key[last + 1..].to_ascii_lowercase()
    )
}

fn clean_value(raw: &str) -> String {
    let mut value = raw.trim();
    // `#`/`;` only start a comment outside quotes; a URL never contains either
    // in a position where this matters.
    if !value.starts_with('"') {
        if let Some(idx) = value.find(['#', ';']) {
            value = value[..idx].trim_end();
        }
    }
    value.trim_matches('"').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_table_matches_upstream_and_forgejo() {
        // Every row of the research §5.2 table, plus the scheme and
        // trailing-slash cases the table implies.
        let cases = [
            ("https://host/foo/bar", "https://host/foo/bar.git/info/lfs"),
            (
                "https://host/foo/bar.git",
                "https://host/foo/bar.git/info/lfs",
            ),
            ("git@host:foo/bar.git", "https://host/foo/bar.git/info/lfs"),
            (
                "ssh://user@host/foo/bar.git",
                "https://host/foo/bar.git/info/lfs",
            ),
            ("git://host/foo/bar", "https://host/foo/bar.git/info/lfs"),
            // Trailing slashes are trimmed before the `.git` test, or every
            // one of these would gain a `/.git/info/lfs` segment.
            ("https://host/foo/bar/", "https://host/foo/bar.git/info/lfs"),
            (
                "https://host/foo/bar.git///",
                "https://host/foo/bar.git/info/lfs",
            ),
            // scp-style without a user.
            ("host:foo/bar.git", "https://host/foo/bar.git/info/lfs"),
            // Plain HTTP stays plain HTTP.
            ("http://host/foo/bar", "http://host/foo/bar.git/info/lfs"),
            // A non-standard SSH port survives; git-lfs keeps it too, and for
            // SSH remotes this URL is only the fallback anyway.
            (
                "ssh://git@host:2222/foo/bar.git",
                "https://host:2222/foo/bar.git/info/lfs",
            ),
            // Deep paths and dots in the owner name.
            (
                "https://host/an.org/sub/bar",
                "https://host/an.org/sub/bar.git/info/lfs",
            ),
        ];

        for (remote, expected) in cases {
            let derived = derive(remote).expect(remote);
            assert_eq!(derived.as_str(), expected, "deriving {remote}");
        }
    }

    #[test]
    fn userinfo_never_survives_into_the_endpoint() {
        // AD-53: the endpoint ends up in logs and error strings, so a token
        // embedded in the remote must not ride along.
        for remote in [
            "https://someone:s3cr3t@host/foo/bar.git",
            "ssh://someone@host/foo/bar.git",
            "someone@host:foo/bar.git",
        ] {
            let derived = derive(remote).expect(remote);
            assert_eq!(derived.username(), "");
            assert_eq!(derived.password(), None);
            assert!(!derived.as_str().contains("s3cr3t"), "{derived}");
            assert!(!derived.as_str().contains("someone"), "{derived}");
        }
    }

    #[test]
    fn unsupported_remotes_are_configuration_errors_that_leak_nothing() {
        let err = derive("/srv/git/bar.git").expect_err("local path has no LFS server");
        assert_eq!(err.code(), "config");

        let err = derive("rsync://token@host/foo").expect_err("unknown scheme");
        assert_eq!(err.code(), "config");
        assert!(!err.to_string().contains("token"), "{err}");

        assert_eq!(derive("   ").expect_err("empty").code(), "config");
    }

    #[test]
    fn sub_path_appends_instead_of_replacing_the_last_segment() {
        let endpoint = derive("https://host/foo/bar").expect("derive");
        let batch = sub_path(&endpoint, "objects/batch").expect("sub path");

        assert_eq!(
            batch.as_str(),
            "https://host/foo/bar.git/info/lfs/objects/batch"
        );
        // The bug this guards against: `Url::join` drops `lfs`.
        assert_ne!(
            batch.as_str(),
            endpoint.join("objects/batch").expect("join").as_str()
        );
    }

    #[test]
    fn remote_specific_lfsurl_outranks_lfs_url_which_outranks_derivation() {
        let config = concat!(
            "[lfs]\n",
            "\turl = https://cdn.example/repo-wide\n",
            "[remote \"origin\"]\n",
            "\tlfsurl = https://cdn.example/for-origin\n",
        );

        // Most specific wins.
        assert_eq!(
            resolve("https://host/foo/bar", Some(config), "origin")
                .expect("resolve")
                .as_str(),
            "https://cdn.example/for-origin"
        );
        // The remote-specific key belongs to `origin` only; another remote
        // falls through to the repository-wide key.
        assert_eq!(
            resolve("https://host/foo/bar", Some(config), "upstream")
                .expect("resolve")
                .as_str(),
            "https://cdn.example/repo-wide"
        );
        // No override at all: derive.
        assert_eq!(
            resolve("https://host/foo/bar", None, "origin")
                .expect("resolve")
                .as_str(),
            "https://host/foo/bar.git/info/lfs"
        );
        // A config with neither key also derives.
        assert_eq!(
            resolve(
                "https://host/foo/bar",
                Some("[lfs]\n\tlocking = true\n"),
                "origin"
            )
            .expect("resolve")
            .as_str(),
            "https://host/foo/bar.git/info/lfs"
        );
    }

    #[test]
    fn an_override_is_used_verbatim_without_the_info_lfs_suffix() {
        // The `.git/info/lfs` suffix is a property of the derivation. Appending
        // it to an explicit endpoint would break every self-hosted LFS server
        // that is not a git host.
        let config = "[lfs]\n\turl = https://cdn.example/lfs\n";
        let resolved = resolve("https://host/foo/bar", Some(config), "origin").expect("resolve");
        assert_eq!(resolved.as_str(), "https://cdn.example/lfs");

        // A trailing slash on the override is trimmed so `sub_path` cannot
        // produce a double slash.
        let config = "[lfs]\n\turl = https://cdn.example/lfs/\n";
        let resolved = resolve("https://host/foo/bar", Some(config), "origin").expect("resolve");
        assert_eq!(resolved.as_str(), "https://cdn.example/lfs");
        assert_eq!(
            sub_path(&resolved, "objects/batch")
                .expect("sub path")
                .as_str(),
            "https://cdn.example/lfs/objects/batch"
        );
    }

    #[test]
    fn an_ssh_override_is_mapped_to_https_with_userinfo_stripped() {
        let config = "[lfs]\n\turl = ssh://git@cdn.example/lfs\n";
        let resolved = resolve("https://host/foo/bar", Some(config), "origin").expect("resolve");
        assert_eq!(resolved.as_str(), "https://cdn.example/lfs");
    }

    #[test]
    fn git_config_reader_handles_the_syntax_lfsconfig_actually_uses() {
        let config = GitConfig::parse(concat!(
            "# a comment\n",
            "; another\n",
            "[LFS]\n",
            "  URL = https://one.example/lfs  ; trailing comment\n",
            "[remote \"Origin\"]\n",
            "  lfsurl = \"https://two.example/lfs\"\n",
            "[lfs]\n",
            "  url = https://three.example/lfs\n",
            "  locking\n",
        ));

        // Section and key names fold case; the last definition wins.
        assert_eq!(config.get("lfs.url"), Some("https://three.example/lfs"));
        assert_eq!(config.get("LFS.URL"), Some("https://three.example/lfs"));
        // Subsection names are case-sensitive.
        assert_eq!(
            config.get("remote.Origin.lfsurl"),
            Some("https://two.example/lfs")
        );
        assert_eq!(config.get("remote.origin.lfsurl"), None);
        // Unknown keys are absent rather than empty.
        assert_eq!(config.get("lfs.missing"), None);
        // A key outside any section is ignored, not attributed to the file.
        assert_eq!(GitConfig::parse("url = https://x/\n").get("url"), None);
    }

    #[test]
    fn quoted_values_keep_characters_a_comment_strip_would_eat() {
        let config = GitConfig::parse("[lfs]\n  url = \"https://x.example/a#b\"\n");
        assert_eq!(config.get("lfs.url"), Some("https://x.example/a#b"));
    }
}

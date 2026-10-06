//! Where a Markdown link in a drive points, as the drive's tools resolve it
//! (the OKF digest: bundle-relative `/x.md` recommended, relative paths
//! allowed, a URL is a URL, a broken link is no error). The rule for each
//! shape is the drive's answer over `tests/fixtures/okf/links.jsonl`.

/// What a link names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Only a fragment of the document it is in.
    Fragment,
    /// A URL, or anything else with a scheme.
    External(String),
    /// A drive-relative path, normalised; it may climb out with `..`, which
    /// the caller refuses.
    Path(String),
}

/// Whether `target` starts with a URI scheme: a letter, then letters,
/// digits, `+`, `.` or `-`, then `:`.
fn has_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Resolve `target`, written in the folder `base` (drive-relative).
pub fn resolve(base: &str, target: &str) -> Link {
    if target.starts_with('#') {
        return Link::Fragment;
    }
    if has_scheme(target) {
        return Link::External(target.to_owned());
    }
    let target = target.split('#').next().unwrap_or("");
    let target = target.split('?').next().unwrap_or("");
    if target.is_empty() {
        return Link::Path(String::new());
    }
    let target = target.replace("%20", " ");
    let joined = if target.starts_with('/') || base.is_empty() {
        target
    } else {
        format!("{base}/{target}")
    };
    Link::Path(normpath(&joined).trim_start_matches('/').to_owned())
}

/// POSIX `normpath`: `.` and empty segments dropped, `..` taking the one
/// before it, and the leading slashes kept as POSIX keeps them.
fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let leading = path.len() - path.trim_start_matches('/').len();
    let root = match leading {
        0 => "",
        2 => "//",
        _ => "/",
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if root.is_empty() {
                    parts.push("..");
                }
            }
            _ => parts.push(part),
        }
    }
    let joined = format!("{root}{}", parts.join("/"));
    if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

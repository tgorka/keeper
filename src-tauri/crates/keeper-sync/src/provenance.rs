//! Provenance as git trailers (Story 23.4 / 28.1, AD-44, FR-86).
//!
//! "Where did this change come from, and who made it?" must be answerable from
//! a clone alone, offline, with no keeper installed. That rules out a sidecar
//! metadata file — it would drift from history the first time someone used
//! plain `git`. So provenance rides git's own commit message trailers, which
//! every git tool already preserves through merges, rebases and format-patch.
//!
//! The block is fixed-shape and machine-parseable, and `parse` is deliberately
//! tolerant: a commit made by a human with plain `git` has no trailers at all,
//! and that is not an error.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// What caused a commit to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncSource {
    /// The filesystem watcher observed a settled change.
    Watch,
    /// A user asked for a sync explicitly.
    Manual,
    /// `keeper-syncd` on a server or a cron.
    Cli,
    /// An autonomous agent writing into a worktree lane (AD-50).
    Bot,
}

impl SyncSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Manual => "manual",
            Self::Cli => "cli",
            Self::Bot => "bot",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "watch" => Some(Self::Watch),
            "manual" => Some(Self::Manual),
            "cli" => Some(Self::Cli),
            "bot" => Some(Self::Bot),
            _ => None,
        }
    }
}

/// Trailer keys. Named constants because they are a wire contract: a peer
/// running an older keeper parses commits written by a newer one.
const K_PROFILE: &str = "Keeper-Profile";
const K_DEVICE: &str = "Keeper-Device";
const K_ORIGIN: &str = "Keeper-Origin";
const K_SOURCE: &str = "Keeper-Source";
const K_AGENT: &str = "Keeper-Agent";
const K_TAG: &str = "Keeper-Tag";

/// The provenance stamped on (or read from) one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub profile: String,
    /// Human device label.
    pub device_label: String,
    /// Device ULID — the stable identity; the label may be edited.
    pub device_id: String,
    /// Hostname, or a volume label for a removable profile.
    pub origin: String,
    pub source: SyncSource,
    /// `keeper-sync/<version>`.
    pub agent: String,
    pub tags: Vec<String>,
}

impl Provenance {
    pub fn new(
        profile: impl Into<String>,
        device_label: impl Into<String>,
        device_id: impl Into<String>,
        origin: impl Into<String>,
        source: SyncSource,
    ) -> Self {
        Self {
            profile: profile.into(),
            device_label: device_label.into(),
            device_id: device_id.into(),
            origin: origin.into(),
            source,
            agent: format!("keeper-sync/{}", env!("CARGO_PKG_VERSION")),
            tags: Vec::new(),
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Render the trailer block, without a leading blank line.
    ///
    /// Values are sanitized to a single line: a newline inside one would end
    /// the trailer block early and let a crafted profile name inject arbitrary
    /// trailers into every commit the engine makes.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(256);
        let _ = writeln!(out, "{K_PROFILE}: {}", sanitize(&self.profile));
        let _ = writeln!(
            out,
            "{K_DEVICE}: {} ({})",
            sanitize(&self.device_label),
            sanitize(&self.device_id)
        );
        let _ = writeln!(out, "{K_ORIGIN}: {}", sanitize(&self.origin));
        let _ = writeln!(out, "{K_SOURCE}: {}", self.source.as_str());
        let _ = writeln!(out, "{K_AGENT}: {}", sanitize(&self.agent));
        for tag in &self.tags {
            let tag = sanitize(tag);
            if !tag.is_empty() {
                let _ = writeln!(out, "{K_TAG}: {tag}");
            }
        }
        out
    }

    /// Read provenance back out of a commit message.
    ///
    /// Returns `None` for a message with no keeper trailers — an ordinary
    /// human commit, which is not an error. A partial block (some keys
    /// missing) still parses, with empty strings for what is absent, because a
    /// truncated block is more useful than nothing when diagnosing a field
    /// report.
    pub fn parse(message: &str) -> Option<Self> {
        let mut found = false;
        let mut profile = String::new();
        let mut device_label = String::new();
        let mut device_id = String::new();
        let mut origin = String::new();
        let mut source = SyncSource::Manual;
        let mut agent = String::new();
        let mut tags = Vec::new();

        for line in message.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key {
                K_PROFILE => {
                    profile = value.to_owned();
                    found = true;
                }
                K_DEVICE => {
                    let (label, id) = split_device(value);
                    device_label = label;
                    device_id = id;
                    found = true;
                }
                K_ORIGIN => {
                    origin = value.to_owned();
                    found = true;
                }
                K_SOURCE => {
                    if let Some(parsed) = SyncSource::parse(value) {
                        source = parsed;
                        found = true;
                    }
                }
                K_AGENT => {
                    agent = value.to_owned();
                    found = true;
                }
                K_TAG if !value.is_empty() => {
                    tags.push(value.to_owned());
                    found = true;
                }
                _ => {}
            }
        }

        found.then_some(Self {
            profile,
            device_label,
            device_id,
            origin,
            source,
            agent,
            tags,
        })
    }
}

/// Split `label (id)` back into its parts, tolerating a label that itself
/// contains parentheses by anchoring on the LAST `(`.
fn split_device(value: &str) -> (String, String) {
    match (value.rfind('('), value.strip_suffix(')')) {
        (Some(open), Some(_)) => (
            value[..open].trim().to_owned(),
            value[open + 1..value.len() - 1].trim().to_owned(),
        ),
        _ => (value.to_owned(), String::new()),
    }
}

/// Collapse a value to a single trimmed line.
///
/// Trailer injection guard: a profile named `"x\nKeeper-Source: bot"` must not
/// be able to forge a trailer.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Build the full commit message: a generated subject, an optional body, then
/// a blank line and the trailer block.
///
/// The blank line is what makes git recognize the block as trailers rather
/// than prose, so it is not cosmetic.
pub fn commit_message(subject: &str, body: &str, provenance: &Provenance) -> String {
    let mut out = String::with_capacity(subject.len() + body.len() + 256);
    out.push_str(sanitize(subject).as_str());
    out.push('\n');
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(body.trim_end());
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&provenance.render());
    out
}

/// Generated commit subject: `sync(<profile>): 3 added, 1 modified, 1 deleted`.
///
/// Stable and mechanical on purpose — a human reading `git log` should be able
/// to tell at a glance which commits the engine made.
pub fn change_subject(profile: &str, added: usize, modified: usize, deleted: usize) -> String {
    let mut parts = Vec::with_capacity(3);
    if added > 0 {
        parts.push(format!("{added} added"));
    }
    if modified > 0 {
        parts.push(format!("{modified} modified"));
    }
    if deleted > 0 {
        parts.push(format!("{deleted} deleted"));
    }
    if parts.is_empty() {
        parts.push("no file changes".to_owned());
    }
    format!("sync({}): {}", sanitize(profile), parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Provenance {
        Provenance::new(
            "tgdrive",
            "Dev Laptop",
            "01JDEVICE",
            "delectra",
            SyncSource::Watch,
        )
        .with_tags(vec!["drive".to_owned(), "personal".to_owned()])
    }

    #[test]
    fn provenance_round_trips_through_a_commit_message() {
        let p = sample();
        let msg = commit_message("sync(tgdrive): 1 added", "  a.txt\n", &p);
        let back = Provenance::parse(&msg).expect("must parse");
        assert_eq!(back, p);
    }

    #[test]
    fn a_plain_human_commit_has_no_provenance_and_that_is_fine() {
        assert!(Provenance::parse("fix the thing\n\nSigned-off-by: X <x@y>").is_none());
    }

    #[test]
    fn a_newline_in_a_value_cannot_forge_a_trailer() {
        // A profile named to look like a trailer must not be able to claim a
        // different source on every commit.
        let p = Provenance::new(
            "evil\nKeeper-Source: bot",
            "Laptop",
            "01J",
            "host",
            SyncSource::Watch,
        );
        let rendered = p.render();
        // The property that matters is that no injected LINE appears. A value
        // may legitimately contain the text "Keeper-Source:" after newline
        // folding; what must never happen is a second line starting with it,
        // because that is what a trailer parser keys on.
        let forged = rendered
            .lines()
            .filter(|line| line.starts_with("Keeper-Source:"))
            .count();
        assert_eq!(forged, 1, "exactly one source trailer line");
        let injected_line = rendered
            .lines()
            .any(|line| line.starts_with("Keeper-Source: bot"));
        assert!(
            !injected_line,
            "the injected value must not become its own trailer line"
        );
        let back = Provenance::parse(&rendered).expect("parse");
        assert_eq!(back.source, SyncSource::Watch);
    }

    #[test]
    fn device_label_containing_parentheses_still_splits_on_the_id() {
        let p = Provenance::new("p", "Mac (work)", "01JID", "host", SyncSource::Cli);
        let back = Provenance::parse(&p.render()).expect("parse");
        assert_eq!(back.device_label, "Mac (work)");
        assert_eq!(back.device_id, "01JID");
    }

    #[test]
    fn the_trailer_block_is_separated_by_a_blank_line() {
        // Without it git treats the block as prose, not trailers.
        let msg = commit_message("subject", "", &sample());
        let lines: Vec<&str> = msg.lines().collect();
        assert_eq!(lines[0], "subject");
        assert_eq!(lines[1], "");
        assert!(lines[2].starts_with("Keeper-Profile:"));
    }

    #[test]
    fn subjects_describe_only_what_actually_changed() {
        assert_eq!(
            change_subject("p", 3, 1, 1),
            "sync(p): 3 added, 1 modified, 1 deleted"
        );
        assert_eq!(change_subject("p", 0, 2, 0), "sync(p): 2 modified");
        assert_eq!(change_subject("p", 0, 0, 0), "sync(p): no file changes");
    }

    #[test]
    fn empty_tags_are_dropped_rather_than_emitted_blank() {
        let p = Provenance::new("p", "d", "i", "o", SyncSource::Bot).with_tags(vec![
            String::new(),
            "  ".to_owned(),
            "real".to_owned(),
        ]);
        assert_eq!(p.render().matches("Keeper-Tag:").count(), 1);
        assert_eq!(
            Provenance::parse(&p.render()).expect("parse").tags,
            vec!["real"]
        );
    }
}

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

/// The placeholders a commit-subject template may name, without their braces.
///
/// Closed and small on purpose. A template is written once and then rides every
/// commit that folder will ever make, so the set has to be short enough to
/// document in one line of help text and stable enough that a profile written
/// today still renders in a year. Only the SUBJECT is templatable — the trailer
/// block is provenance, and a repository has to be able to trust its shape.
pub const SUBJECT_PLACEHOLDERS: [&str; 6] = [
    "profile", "device", "added", "modified", "deleted", "changed",
];

/// The `@device` qualifier for a commit subject, or `None` when this machine
/// still answers to its hostname.
///
/// The subject names the device only once the user has *renamed* it. A label
/// that is still the hostname carries nothing a reader wants: `Keeper-Device`
/// already records it on every commit, and repeating it in the one line
/// `git log --oneline` shows would spend the scarcest space in history on the
/// least surprising fact. A deliberate name is the opposite — someone chose it
/// so they could tell two machines apart at a glance, which is exactly what a
/// subject is for.
///
/// Compared case-insensitively after trimming: macOS reports the same host as
/// `Hesperia` and `hesperia` depending on who is asking, and a qualifier that
/// appeared or vanished with the casing of a hostname would be a mystery rather
/// than a feature.
pub fn device_qualifier<'a>(label: &'a str, host_label: &str) -> Option<&'a str> {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(host_label.trim()) {
        return None;
    }
    Some(trimmed)
}

/// One piece of a parsed subject template.
enum Piece<'a> {
    /// Literal text, emitted as it stands.
    Text(&'a str),
    /// A `{name}` reference. Whether `name` is one keeper knows is the caller's
    /// business: the renderer leaves an unknown one alone, the validator refuses
    /// it, and both have to agree on what counts as a reference at all.
    Placeholder(&'a str),
}

/// Split a template into literal text and `{name}` references.
///
/// The grammar is deliberately tiny and has NO escape sequence: `{`, one or more
/// ASCII letters or underscores, `}` is a reference, and every other `{` is
/// ordinary text. So a literal `{profile}` cannot be written — an acceptable
/// trade in a one-line commit subject, and the reason the placeholder set is
/// closed rather than open. One scanner, two consumers, so a template can never
/// pass validation and then render as something else.
fn pieces(template: &str) -> Pieces<'_> {
    Pieces { rest: template }
}

struct Pieces<'a> {
    rest: &'a str,
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        match self.rest.find('{') {
            // No brace left: all that remains is literal.
            None => Some(Piece::Text(std::mem::take(&mut self.rest))),
            // A brace at the front: a reference, or a brace that is just a
            // brace. Either way one piece is consumed, so this terminates.
            Some(0) => {
                let after = &self.rest[1..];
                match placeholder_name(after) {
                    Some(name) => {
                        // `name.len()` is the offset of `}`, so +1 clears it.
                        self.rest = &after[name.len() + 1..];
                        Some(Piece::Placeholder(name))
                    }
                    None => {
                        self.rest = after;
                        Some(Piece::Text("{"))
                    }
                }
            }
            // Literal text up to the next brace; the brace is the next piece,
            // whatever it turns out to be.
            Some(open) => {
                let (text, rest) = self.rest.split_at(open);
                self.rest = rest;
                Some(Piece::Text(text))
            }
        }
    }
}

/// The placeholder name `after` opens with, when it opens with
/// `<letters-or-underscores>}`.
///
/// The shape check is what separates a typo from a stray brace: `{Profile}` is
/// shaped like a reference and is reported as an unknown one, while `{a{b}` is
/// not shaped like one and stays literal text.
fn placeholder_name(after: &str) -> Option<&str> {
    let end = after.find('}')?;
    let name = &after[..end];
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_')).then_some(name)
}

/// The first `{placeholder}` a template names that keeper does not know.
///
/// [`crate::profile::SyncProfile::validate`] calls this, so a typo is refused
/// where the user can still see the field rather than riding into every commit
/// the folder makes from then on. It returns the name rather than a bool so the
/// message can say which one.
pub fn unknown_subject_placeholder(template: &str) -> Option<&str> {
    pieces(template).find_map(|piece| match piece {
        Piece::Placeholder(name) if !SUBJECT_PLACEHOLDERS.contains(&name) => Some(name),
        _ => None,
    })
}

/// The generated commit subject: a profile's template, or the mechanical
/// `sync(<profile>): 3 added, 1 modified, 1 deleted` when it has none.
///
/// An empty (or all-whitespace) template is the documented default and produces
/// the mechanical subject byte for byte — that string is what a human scanning
/// `git log` recognizes as a keeper commit, and it has been in every repository
/// keeper has ever written to.
///
/// Two things a template cannot do, because a commit subject is exactly one
/// line and must not be blank:
///
/// * **Span lines.** Newlines collapse to spaces, the same `sanitize` every
///   trailer value goes through — a subject that ran on would turn the rest of
///   itself into an unasked-for commit body.
/// * **Render to nothing.** `{deleted}` alone on a commit that deleted nothing
///   is legitimately empty, and an empty subject makes `git log --oneline`
///   unreadable, so the mechanical subject stands in.
///
/// An unknown placeholder is left verbatim rather than dropped. `validate`
/// refuses one on save and on load, so this is only reachable by a row that got
/// in some other way — and a visible `{oops}` in `git log` is a bug report,
/// while a silently deleted one is a mystery. A commit still happens either way:
/// refusing to sync over the wording of a subject would be absurd.
pub fn change_subject(
    template: &str,
    profile: &str,
    device: Option<&str>,
    added: usize,
    modified: usize,
    deleted: usize,
) -> String {
    if template.trim().is_empty() {
        return mechanical_subject(profile, device, added, modified, deleted);
    }
    let mut out = String::with_capacity(template.len() + 32);
    for piece in pieces(template) {
        match piece {
            Piece::Text(text) => out.push_str(text),
            // The profile name goes in raw: the whole subject is sanitized
            // below, which is where a newline in any part of it is collapsed.
            Piece::Placeholder("profile") => out.push_str(profile),
            // Bare, with no `@`: a template author writes their own separator,
            // and `{profile}@{device}` on an un-renamed machine would otherwise
            // leave a trailing `@` with nothing after it.
            Piece::Placeholder("device") => out.push_str(device.unwrap_or_default()),
            Piece::Placeholder("added") => {
                let _ = write!(out, "{added}");
            }
            Piece::Placeholder("modified") => {
                let _ = write!(out, "{modified}");
            }
            Piece::Placeholder("deleted") => {
                let _ = write!(out, "{deleted}");
            }
            Piece::Placeholder("changed") => {
                let _ = write!(out, "{}", added + modified + deleted);
            }
            Piece::Placeholder(unknown) => {
                let _ = write!(out, "{{{unknown}}}");
            }
        }
    }
    let subject = sanitize(&out);
    if subject.is_empty() {
        return mechanical_subject(profile, device, added, modified, deleted);
    }
    subject
}

/// `sync(<profile>): 3 added, 1 modified, 1 deleted`, with the device appended as
/// `sync(<profile>@<device>)` once this machine has been renamed.
///
/// Stable and mechanical on purpose — a human reading `git log` should be able
/// to tell at a glance which commits the engine made. This is what an empty
/// template means, so its bytes are a compatibility surface, not a detail: the
/// un-renamed form is unchanged to the byte, and only a machine the user has
/// deliberately named gains the qualifier. See [`device_qualifier`] for why that
/// is the condition.
fn mechanical_subject(
    profile: &str,
    device: Option<&str>,
    added: usize,
    modified: usize,
    deleted: usize,
) -> String {
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
    let scope = match device {
        Some(device) => format!("{}@{}", sanitize(profile), sanitize(device)),
        None => sanitize(profile),
    };
    format!("sync({scope}): {}", parts.join(", "))
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
            change_subject("", "p", None, 3, 1, 1),
            "sync(p): 3 added, 1 modified, 1 deleted"
        );
        assert_eq!(
            change_subject("", "p", None, 0, 2, 0),
            "sync(p): 2 modified"
        );
        assert_eq!(
            change_subject("", "p", None, 0, 0, 0),
            "sync(p): no file changes"
        );
    }

    /// The regression that would go unnoticed: every repository keeper has ever
    /// written to carries these bytes, and no profile has a template yet, so a
    /// templating bug in the empty case reworks all of history's future.
    #[test]
    fn an_empty_template_reproduces_the_mechanical_subject_byte_for_byte() {
        for (added, modified, deleted) in [(3, 1, 1), (0, 2, 0), (1, 0, 0), (0, 0, 0)] {
            let mechanical = mechanical_subject("tgdrive", None, added, modified, deleted);
            assert_eq!(
                change_subject("", "tgdrive", None, added, modified, deleted),
                mechanical
            );
            // Whitespace is not a template either: a field the user tabbed
            // through must not reword every commit.
            assert_eq!(
                change_subject("  \t ", "tgdrive", None, added, modified, deleted),
                mechanical
            );
        }
    }

    #[test]
    fn a_renamed_device_qualifies_the_subject_and_an_un_renamed_one_does_not() {
        // The requested shape, verbatim.
        assert_eq!(
            change_subject("", "tgdrive", Some("hesperia"), 2, 0, 0),
            "sync(tgdrive@hesperia): 2 added"
        );
        // Untouched machines keep the string every existing repository holds.
        assert_eq!(
            change_subject("", "tgdrive", None, 2, 0, 0),
            "sync(tgdrive): 2 added"
        );
    }

    #[test]
    fn a_device_qualifier_exists_only_once_the_label_stops_being_the_hostname() {
        assert_eq!(device_qualifier("hesperia", "electra"), Some("hesperia"));
        // Still the default: `Keeper-Device` already records it, so the subject
        // would only be repeating itself.
        assert_eq!(device_qualifier("electra", "electra"), None);
        // macOS reports one host under either casing, and the qualifier must not
        // blink in and out with it.
        assert_eq!(device_qualifier("Electra", "electra"), None);
        assert_eq!(device_qualifier("  electra  ", "electra"), None);
        // An empty label cannot qualify anything, and `sync(p@)` would be worse
        // than saying nothing.
        assert_eq!(device_qualifier("   ", "electra"), None);
    }

    #[test]
    fn the_qualifier_reads_the_hostname_out_of_the_provenance_origin() {
        // `git::commit` derives the qualifier from `Provenance`'s own fields
        // rather than taking it as an argument, so the subject and the trailers
        // can never disagree about which machine made a commit. That relies on
        // `origin` being this host — which every production `Provenance::new`
        // passes. If `origin` ever becomes the volume label its doc comment
        // mentions, this is the test that says so, and the qualifier needs a
        // parameter of its own.
        let p = Provenance::new("tgdrive", "hesperia", "01J", "electra", SyncSource::Watch);
        assert_eq!(
            device_qualifier(&p.device_label, &p.origin),
            Some("hesperia")
        );

        let plain = Provenance::new("tgdrive", "electra", "01J", "electra", SyncSource::Watch);
        assert_eq!(device_qualifier(&plain.device_label, &plain.origin), None);
    }

    #[test]
    fn a_template_can_place_the_device_itself_and_gets_nothing_when_unset() {
        assert_eq!(
            change_subject(
                "{profile}@{device}: {changed}",
                "p",
                Some("hesperia"),
                1,
                0,
                0
            ),
            "p@hesperia: 1"
        );
        // Bare substitution, so the template author owns the separator — and an
        // un-renamed machine leaves a dangling `@` they chose to write.
        assert_eq!(
            change_subject("{profile}@{device}: {changed}", "p", None, 1, 0, 0),
            "p@: 1"
        );
        // Which is why `{device}` is a known placeholder: `validate` must accept
        // it rather than refusing the template outright.
        assert!(SUBJECT_PLACEHOLDERS.contains(&"device"));
        assert_eq!(unknown_subject_placeholder("{profile}@{device}"), None);
    }

    #[test]
    fn a_template_substitutes_the_profile_name_and_the_counts() {
        assert_eq!(
            change_subject(
                "{profile}: {changed} files ({added}/{modified}/{deleted})",
                "tgdrive",
                None,
                3,
                1,
                1
            ),
            "tgdrive: 5 files (3/1/1)"
        );
    }

    /// Every documented placeholder must actually be wired into the renderer.
    /// The set and the match arms are two lists that have to agree, and this is
    /// what makes a name added to one but not the other fail.
    #[test]
    fn every_documented_placeholder_is_wired_up() {
        for name in SUBJECT_PLACEHOLDERS {
            // Padded so the render can never come out empty and fall back to
            // the mechanical subject, which would mask an unwired name.
            let template = format!("x{{{name}}}y");
            let rendered = change_subject(&template, "p", None, 1, 2, 3);
            assert_ne!(
                rendered, template,
                "{{{name}}} is documented but renders as itself"
            );
            assert!(
                unknown_subject_placeholder(&template).is_none(),
                "{{{name}}} is documented but the validator refuses it"
            );
        }
    }

    #[test]
    fn an_unknown_placeholder_is_named_by_the_validator_and_left_alone_by_the_renderer() {
        // Shaped like a reference, so a typo is caught rather than silently
        // treated as decoration.
        assert_eq!(
            unknown_subject_placeholder("{Profile} moved"),
            Some("Profile")
        );
        assert_eq!(unknown_subject_placeholder("{oops}"), Some("oops"));
        // Reachable only by a row that skipped validation; a visible marker is
        // a bug report, a dropped one is a mystery.
        assert_eq!(
            change_subject("a {oops} b", "p", None, 0, 0, 0),
            "a {oops} b"
        );
    }

    #[test]
    fn a_brace_that_is_not_a_reference_is_ordinary_text() {
        for template in ["{", "a{b", "{a b}", "{}", "{1}"] {
            assert_eq!(
                unknown_subject_placeholder(template),
                None,
                "{template} must not read as a placeholder"
            );
            assert_eq!(change_subject(template, "p", None, 0, 0, 0), template);
        }
        // The inner reference still resolves; only the stray brace is literal.
        assert_eq!(change_subject("{a{profile}", "p", None, 0, 0, 0), "{ap");
    }

    #[test]
    fn a_template_cannot_produce_a_second_line() {
        // A subject that ran on would turn its own tail into a commit body, and
        // a value containing a trailer key would land inside the trailer block.
        let subject = change_subject("one\ntwo\rKeeper-Source: bot", "p", None, 1, 0, 0);
        assert_eq!(subject.lines().count(), 1);
        assert_eq!(subject, "one two Keeper-Source: bot");
    }

    #[test]
    fn a_count_placeholder_always_renders_a_number_even_when_it_is_zero() {
        // "{deleted} removed" must read "0 removed", not " removed". A
        // placeholder named after a count is that count, always — anything else
        // would have a template silently reshape itself around the commit.
        assert_eq!(
            change_subject("{deleted} removed", "p", None, 1, 0, 0),
            "0 removed"
        );
    }

    #[test]
    fn a_template_that_renders_to_nothing_falls_back_to_the_mechanical_subject() {
        // `git log --oneline` on a blank subject is unreadable, so a render that
        // comes out empty is refused rather than committed. Counts cannot cause
        // this (see above); a profile whose name is all whitespace can.
        assert_eq!(
            change_subject("{profile}", "  ", None, 1, 0, 0),
            "sync(): 1 added"
        );
        assert_eq!(
            change_subject("   ", "p", None, 1, 0, 0),
            "sync(p): 1 added"
        );
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

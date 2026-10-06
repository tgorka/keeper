//! A staged change to an agent's memory or skills: `proposals/<ulid>.md`
//! (AD-400, story 95.1; *Data formats*). Written once by the agent's own
//! host and never edited; the consolidator reads it at night.
//!
//! Pure: the grammar — [`Proposal::render`] and the strict
//! [`Proposal::parse`] — and the `origin` a session's kind gives a
//! proposal ([`origin_of`], R127). The label is a block map: flow maps are
//! outside keeper's frontmatter subset.

use std::collections::BTreeSet;

use chrono::{DateTime, SecondsFormat, Utc};
use matrix_sdk::ruma::OwnedUserId;
use ulid::Ulid;

use crate::agents::label::{Integrity, Label, Readers};
use crate::agents::memory::MemoryTarget;
use crate::agents::session::SessionKind;
use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// The folder of a home that holds proposals.
pub const DIR: &str = "proposals";

/// Every key a proposal's frontmatter may hold, in the order it is written.
const KEYS: [&str; 11] = [
    "type",
    "id",
    "agent",
    "target",
    "op",
    "match",
    "session",
    "host",
    "origin",
    "label",
    "created_at",
];

/// The label's keys.
const LABEL_KEYS: [&str; 3] = ["readers", "integrity", "local_only"];

/// What a proposal changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Memory(MemoryTarget),
    /// A skill under `_skills/`, by name.
    Skill(String),
}

impl Target {
    /// The word the frontmatter holds: `user`, `memory` or `skill:<name>`.
    pub fn as_word(&self) -> String {
        match self {
            Target::Memory(target) => target.as_word().to_owned(),
            Target::Skill(name) => format!("skill:{name}"),
        }
    }

    fn from_word(word: &str) -> Option<Target> {
        match word.strip_prefix("skill:") {
            Some(name) if !name.trim().is_empty() => Some(Target::Skill(name.to_owned())),
            Some(_) => None,
            None => MemoryTarget::from_word(word).map(Target::Memory),
        }
    }
}

/// The change a proposal makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Replace,
    Remove,
    Create,
    Patch,
    Archive,
}

impl Op {
    /// Every op, in the grammar's order.
    pub const ALL: [Op; 6] = [
        Op::Add,
        Op::Replace,
        Op::Remove,
        Op::Create,
        Op::Patch,
        Op::Archive,
    ];

    pub fn as_word(self) -> &'static str {
        match self {
            Op::Add => "add",
            Op::Replace => "replace",
            Op::Remove => "remove",
            Op::Create => "create",
            Op::Patch => "patch",
            Op::Archive => "archive",
        }
    }

    pub fn from_word(word: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|op| op.as_word() == word)
    }

    /// Whether the op is one of `target`'s: add/replace/remove for memory,
    /// create/patch/archive for a skill.
    fn fits(self, target: &Target) -> bool {
        match target {
            Target::Memory(_) => matches!(self, Op::Add | Op::Replace | Op::Remove),
            Target::Skill(_) => matches!(self, Op::Create | Op::Patch | Op::Archive),
        }
    }

    /// Whether the op is pinned by `match`: the exact entry a memory
    /// replace/remove was resolved to, or the SHA-256 of the `SKILL.md` a
    /// skill patch/archive was proposed against (R132).
    pub fn is_pinned(self) -> bool {
        matches!(self, Op::Replace | Op::Remove | Op::Patch | Op::Archive)
    }
}

/// How the session a proposal came from was started (AD-401's gates read
/// it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A person's own conversation with their proxy.
    Foreground,
    /// A nudge's review pass in such a conversation.
    Review,
    Scheduled,
    Delegated,
    Gate,
}

impl Origin {
    pub const ALL: [Origin; 5] = [
        Origin::Foreground,
        Origin::Review,
        Origin::Scheduled,
        Origin::Delegated,
        Origin::Gate,
    ];

    pub fn as_word(self) -> &'static str {
        match self {
            Origin::Foreground => "foreground",
            Origin::Review => "review",
            Origin::Scheduled => "scheduled",
            Origin::Delegated => "delegated",
            Origin::Gate => "gate",
        }
    }

    pub fn from_word(word: &str) -> Option<Origin> {
        Origin::ALL
            .into_iter()
            .find(|origin| origin.as_word() == word)
    }
}

/// The origin of a proposal written in a session of `kind` (R127): `main`
/// and `conversation` are `foreground`, or `review` in a nudge's review
/// pass; a `workflow` is `scheduled` when the session that started it is
/// (`parent`, its kind when it could be read), else `delegated`.
pub fn origin_of(kind: SessionKind, parent: Option<SessionKind>, review: bool) -> Origin {
    match kind {
        SessionKind::Main | SessionKind::Conversation if review => Origin::Review,
        SessionKind::Main | SessionKind::Conversation => Origin::Foreground,
        SessionKind::Scheduled => Origin::Scheduled,
        SessionKind::Delegated => Origin::Delegated,
        SessionKind::Gate => Origin::Gate,
        SessionKind::Workflow if parent == Some(SessionKind::Scheduled) => Origin::Scheduled,
        SessionKind::Workflow => Origin::Delegated,
    }
}

/// One proposal file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Equals the file's stem.
    pub id: Ulid,
    /// The agent's id.
    pub agent: String,
    pub target: Target,
    pub op: Op,
    /// The pin: an entry's exact text, or a `SKILL.md`'s SHA-256 (R132).
    pub matched: Option<String>,
    /// The session's folder, drive-relative.
    pub session: String,
    /// The writing host's slug.
    pub host: String,
    pub origin: Origin,
    /// The session's label when it was written.
    pub label: Label,
    pub created_at: DateTime<Utc>,
    /// The proposed entry text, or the proposed `SKILL.md`; empty for a
    /// remove or an archive.
    pub body: String,
}

impl Proposal {
    /// The file name: `<ulid>.md`.
    pub fn file_name(&self) -> String {
        format!("{}.md", self.id)
    }

    /// The memory change as Hermes' batch op, pinned to its `match`;
    /// `None` for a skill's.
    pub fn hermes_op(&self) -> Option<keeper_ported::hermes::memory::Op> {
        use keeper_ported::hermes::memory::Op as HermesOp;
        let matched = self.matched.clone();
        let pin = matched.clone().unwrap_or_default();
        Some(match self.op {
            Op::Add => HermesOp::Add {
                content: self.body.clone(),
            },
            Op::Replace => HermesOp::Replace {
                old_text: pin,
                content: self.body.clone(),
                matched_entry: matched,
            },
            Op::Remove => HermesOp::Remove {
                old_text: pin,
                matched_entry: matched,
            },
            Op::Create | Op::Patch | Op::Archive => return None,
        })
    }

    /// The file's bytes: frontmatter in [`KEYS`]' order, then the body.
    pub fn render(&self) -> String {
        let text = |value: &str| FieldValue::Str(value.to_owned());
        let readers = match &self.label.readers {
            Readers::Anyone => text("*"),
            Readers::Only(set) => {
                FieldValue::List(set.iter().map(|user| text(user.as_str())).collect())
            }
        };
        let mut label = vec![
            ("readers".to_owned(), readers),
            ("integrity".to_owned(), text(self.label.integrity.as_word())),
        ];
        if self.label.local_only {
            label.push(("local_only".to_owned(), FieldValue::Bool(true)));
        }
        let mut pairs = vec![
            ("type".to_owned(), text("proposal")),
            ("id".to_owned(), text(&self.id.to_string())),
            ("agent".to_owned(), text(&self.agent)),
            ("target".to_owned(), text(&self.target.as_word())),
            ("op".to_owned(), text(self.op.as_word())),
        ];
        if let Some(matched) = &self.matched {
            pairs.push(("match".to_owned(), text(matched)));
        }
        pairs.extend([
            ("session".to_owned(), text(&self.session)),
            ("host".to_owned(), text(&self.host)),
            ("origin".to_owned(), text(self.origin.as_word())),
            ("label".to_owned(), FieldValue::Map(label)),
            (
                "created_at".to_owned(),
                text(&self.created_at.to_rfc3339_opts(SecondsFormat::Secs, true)),
            ),
        ]);
        let mut out = Frontmatter::serialise_new(&pairs);
        out.push_str(&self.body);
        if !self.body.is_empty() && !self.body.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    /// Read the proposal file `stem`.md whose text is `text`. Strict: an
    /// unknown key, a missing one, a value of the wrong shape, an op that
    /// is not the target's, a pin where none belongs or none where one
    /// does, or an id that is not the stem is refused with its sentence.
    pub fn parse(stem: &str, text: &str) -> Result<Proposal, String> {
        let (frontmatter, body_offset) = Frontmatter::parse(text);
        if !frontmatter.has_block() {
            return Err("A proposal starts with its frontmatter.".to_owned());
        }
        if let Some(unparsed) = frontmatter.unparsed() {
            return Err(format!(
                "A proposal's frontmatter does not read at line {}: {}",
                unparsed.line, unparsed.reason
            ));
        }
        if let Some(other) = frontmatter.keys().find(|key| !KEYS.contains(key)) {
            return Err(format!(
                "A proposal has `{other}`, which is not one of its keys."
            ));
        }
        let word = |key: &str| -> Result<&str, String> {
            frontmatter
                .as_string(key)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| format!("A proposal needs `{key}`, a word."))
        };
        if word("type")? != "proposal" {
            return Err("A proposal's `type` is `proposal`.".to_owned());
        }
        let id: Ulid = word("id")?
            .parse()
            .map_err(|_| "A proposal's `id` is a ULID.".to_owned())?;
        if id.to_string() != stem {
            return Err(format!(
                "A proposal's `id` is its file's name: {stem}, not {id}."
            ));
        }
        let target = Target::from_word(word("target")?).ok_or_else(|| {
            "A proposal's `target` is `user`, `memory` or `skill:<name>`.".to_owned()
        })?;
        let op = Op::from_word(word("op")?)
            .filter(|op| op.fits(&target))
            .ok_or_else(|| {
                format!(
                    "A proposal's `op` for `{}` is one of {}.",
                    target.as_word(),
                    match target {
                        Target::Memory(_) => "add, replace or remove",
                        Target::Skill(_) => "create, patch or archive",
                    }
                )
            })?;
        let matched = frontmatter.as_string("match").map(str::to_owned);
        match (op.is_pinned(), &matched) {
            (true, None) => {
                return Err(format!(
                    "A proposal's `{}` needs `match`, what it is pinned to.",
                    op.as_word()
                ))
            }
            (false, Some(_)) => {
                return Err(format!(
                    "A proposal's `{}` is pinned to nothing; it has no `match`.",
                    op.as_word()
                ))
            }
            _ => {}
        }
        let origin = Origin::from_word(word("origin")?).ok_or_else(|| {
            "A proposal's `origin` is foreground, review, scheduled, delegated or gate.".to_owned()
        })?;
        let created_at = DateTime::parse_from_rfc3339(word("created_at")?)
            .map_err(|_| "A proposal's `created_at` is an RFC 3339 time.".to_owned())?
            .with_timezone(&Utc);
        Ok(Proposal {
            id,
            agent: word("agent")?.to_owned(),
            target,
            op,
            matched,
            session: word("session")?.to_owned(),
            host: word("host")?.to_owned(),
            origin,
            label: label_of(frontmatter.get("label"))?,
            created_at,
            body: text[body_offset..].to_owned(),
        })
    }
}

/// The `label` block map.
fn label_of(value: Option<&FieldValue>) -> Result<Label, String> {
    let shape = || "A proposal's `label` is a map of readers and integrity.".to_owned();
    let Some(FieldValue::Map(pairs)) = value else {
        return Err(shape());
    };
    if let Some((other, _)) = pairs
        .iter()
        .find(|(key, _)| !LABEL_KEYS.contains(&key.as_str()))
    {
        return Err(format!(
            "A proposal's `label` has `{other}`, which is not one of its keys."
        ));
    }
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let readers = match get("readers") {
        Some(FieldValue::Str(star)) if star == "*" => Readers::Anyone,
        Some(FieldValue::List(items)) => {
            let mut set = BTreeSet::new();
            for item in items {
                let FieldValue::Str(user) = item else {
                    return Err(shape());
                };
                let user = OwnedUserId::try_from(user.as_str()).map_err(|_| {
                    format!("A proposal's `label` names {user}, which is not a Matrix user.")
                })?;
                set.insert(user);
            }
            Readers::Only(set)
        }
        _ => return Err(shape()),
    };
    let integrity = match get("integrity") {
        Some(FieldValue::Str(word)) => [
            Integrity::Untrusted,
            Integrity::Agent,
            Integrity::Peer,
            Integrity::Owner,
        ]
        .into_iter()
        .find(|integrity| integrity.as_word() == word)
        .ok_or_else(shape)?,
        _ => return Err(shape()),
    };
    let local_only = match get("local_only") {
        None => false,
        Some(FieldValue::Bool(flag)) => *flag,
        Some(_) => return Err(shape()),
    };
    Ok(Label {
        readers,
        integrity,
        local_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user id")
    }

    fn proposal() -> Proposal {
        Proposal {
            id: "01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z".parse().expect("ulid"),
            agent: "nixi".to_owned(),
            target: Target::Memory(MemoryTarget::Memory),
            op: Op::Replace,
            matched: Some("first line\nsecond line with \"quotes\": and § and \\".to_owned()),
            session: "60-sessions/active/2026-10-06-inbox".to_owned(),
            host: "electra".to_owned(),
            origin: Origin::Foreground,
            label: Label {
                readers: Readers::Only(
                    [user("@tgorka:example.org"), user("@marta:example.org")]
                        .into_iter()
                        .collect(),
                ),
                integrity: Integrity::Owner,
                local_only: false,
            },
            created_at: DateTime::parse_from_rfc3339("2026-10-06T09:15:00Z")
                .expect("time")
                .with_timezone(&Utc),
            body: "The replacement entry, whole.\n".to_owned(),
        }
    }

    /// 95.1 acceptance 9: every key of *Data formats* written and read back
    /// as written — a multi-line pin with quotes, a block-map label, every
    /// origin, a skill target with its SHA-256 pin, a local-only label and
    /// anyone as readers.
    #[test]
    fn proposal_frontmatter_round_trips() {
        let mut written = proposal();
        let text = written.render();
        assert!(text.starts_with("---\ntype: proposal\nid: "), "{text}");
        assert!(
            text.contains("label:\n  readers: [\"@marta:example.org\", \"@tgorka:example.org\"]\n  integrity: owner\n"),
            "{text}"
        );
        assert_eq!(
            Proposal::parse(&written.id.to_string(), &text),
            Ok(written.clone())
        );
        for origin in Origin::ALL {
            written.origin = origin;
            let text = written.render();
            assert_eq!(
                Proposal::parse(&written.id.to_string(), &text).map(|p| p.origin),
                Ok(origin)
            );
        }
        let skill = Proposal {
            target: Target::Skill("weekly-review".to_owned()),
            op: Op::Patch,
            matched: Some("a".repeat(64)),
            label: Label {
                readers: Readers::Anyone,
                integrity: Integrity::Agent,
                local_only: true,
            },
            body: "---\nname: weekly-review\ndescription: d\n---\nSteps.\n".to_owned(),
            ..proposal()
        };
        let text = skill.render();
        assert_eq!(Proposal::parse(&skill.id.to_string(), &text), Ok(skill));
        let add = Proposal {
            op: Op::Add,
            matched: None,
            ..proposal()
        };
        assert_eq!(Proposal::parse(&add.id.to_string(), &add.render()), Ok(add));
    }

    /// The grammar is closed: each of these is refused with its sentence.
    #[test]
    fn a_proposal_outside_the_grammar_is_refused() {
        let good = proposal();
        let stem = good.id.to_string();
        let text = good.render();
        let refused = |text: &str| Proposal::parse(&stem, text).expect_err(text);
        assert!(
            refused(&text.replace("host: electra\n", "host: electra\nextra: 1\n"))
                .contains("`extra`")
        );
        assert!(refused(&text.replace("host: electra\n", "")).contains("`host`"));
        assert!(
            refused(&text.replace("op: replace", "op: create")).contains("add, replace or remove")
        );
        assert!(refused(&text.replace("origin: foreground", "origin: night")).contains("origin"));
        assert!(refused(&text.replace("type: proposal", "type: note")).contains("`type`"));
        assert!(refused(&text.replace("integrity: owner", "integrity: boss")).contains("label"));
        let unpinned = Proposal {
            matched: None,
            ..proposal()
        }
        .render();
        assert!(refused(&unpinned).contains("needs `match`"));
        let pinned_add = text.replace("op: replace", "op: add");
        assert!(refused(&pinned_add).contains("no `match`"));
        let other = Ulid::new().to_string();
        assert!(Proposal::parse(&other, &text)
            .expect_err("another stem")
            .contains("file's name"));
        assert!(refused("no frontmatter").contains("frontmatter"));
    }

    /// R127: the origin by the session's kind, a review pass, and the kind
    /// of the session that started a workflow.
    #[test]
    fn the_origin_comes_from_the_session() {
        use SessionKind::*;
        for (kind, parent, review, origin) in [
            (Main, None, false, Origin::Foreground),
            (Conversation, None, false, Origin::Foreground),
            (Main, None, true, Origin::Review),
            (Conversation, None, true, Origin::Review),
            (Scheduled, None, false, Origin::Scheduled),
            (Delegated, None, false, Origin::Delegated),
            (Gate, None, false, Origin::Gate),
            (Workflow, Some(Scheduled), false, Origin::Scheduled),
            (Workflow, Some(Main), false, Origin::Delegated),
            (Workflow, None, false, Origin::Delegated),
        ] {
            assert_eq!(
                origin_of(kind, parent, review),
                origin,
                "{kind:?} {parent:?}"
            );
        }
    }
}

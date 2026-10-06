//! The drive's `_skills/`: agentskills validation and the offered index (AD-362, story 89.3).
//!
//! Each `_skills/<name>/SKILL.md` is read with keeper's frontmatter subset and
//! validated by `keeper_ported::agentskills`, so a refused skill carries the
//! reference validator's own sentences. A refused skill is listed with its
//! reasons and never offered. The prompt carries name and description only;
//! the body loads through `skill_view` (§9.1).
//!
//! Pure: the host walks `_skills/` through `browse::resolve` and hands each
//! directory name with its `SKILL.md` text; a dotted folder (`.archive/`)
//! is never a skill. A skill an agent proposed carries
//! `metadata.keeper_proposal` and waits for a person: it is never offered
//! until a person adopts it by deleting the key (R28 S-12, AD-402).

use keeper_ported::agentskills::{validate_metadata, MetaValue};

use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// A `SKILL.md` larger than this is refused with its size.
pub const MAX_SKILL_BYTES: usize = 256 * 1024;
/// A body longer than this is warned about, not refused (agentskills.io).
pub const BODY_WARN_LINES: usize = 500;

/// `[tools].skills`: every valid skill, or the ones named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillFilter {
    All,
    Named(Vec<String>),
}

impl SkillFilter {
    /// `["*"]` (or any list holding `"*"`) is all of them.
    pub fn from_list(names: &[String]) -> Self {
        if names.iter().any(|name| name == "*") {
            Self::All
        } else {
            Self::Named(names.to_vec())
        }
    }
}

/// One offered skill, as the prompt shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
}

/// The drive's skills for one agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillsIndex {
    /// Valid and wanted, by name.
    pub offered: Vec<SkillEntry>,
    /// `(directory, reasons)`, by directory.
    pub refused: Vec<(String, Vec<String>)>,
    /// Valid skills an agent proposed that no person adopted yet, by
    /// directory: listed, never offered.
    pub waiting: Vec<String>,
    pub warnings: Vec<String>,
}

/// Why a directory under `_skills/` yields no `SKILL.md`, in upstream's words
/// (`skills_ref.validator.validate`), for the host's walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillDirProblem {
    Missing,
    NotADirectory,
    NoSkillMd,
}

impl SkillDirProblem {
    /// The sentence agentskills gives for `path`.
    pub fn sentence(self, path: &str) -> String {
        match self {
            Self::Missing => format!("Path does not exist: {path}"),
            Self::NotADirectory => format!("Not a directory: {path}"),
            Self::NoSkillMd => "Missing required file: SKILL.md".to_owned(),
        }
    }
}

/// The `metadata` key a skill an agent proposed carries until a person
/// adopts it.
pub const PROPOSAL_KEY: &str = "keeper_proposal";

/// What a skill that waits for a person is said to be, after its name.
pub const WAITING: &str = "waits for a person: an agent proposed it, and it is offered once a person adopts it by deleting metadata.keeper_proposal.";

/// Why a skill whose `metadata` keeper cannot read is not offered.
pub const UNREADABLE_METADATA: &str = "metadata is not one block map of distinct plain keys, so keeper cannot tell whether an agent proposed this skill and a person adopted it; write metadata as a block map of `key: value` lines.";

/// What a skill's `metadata` says of its adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adoption {
    /// No proposal key: a person's skill, or one a person adopted.
    Adopted,
    /// [`PROPOSAL_KEY`] is there: it waits for a person.
    Proposed,
    /// Keeper cannot model it, or it says a key twice: whether the
    /// proposal key is there is unknown, so it is not offered (R204).
    Unreadable,
}

/// A `SKILL.md`'s `metadata` pairs — none when it has no `metadata` — or
/// `None` when keeper cannot read it as one block map of distinct plain
/// keys: a construct the parser does not model, a key said twice,
/// `metadata` itself said twice. What such metadata says is unknown, so
/// no occurrence of a key is taken for its value (R204).
pub fn metadata_of(frontmatter: &Frontmatter) -> Option<&[(String, FieldValue)]> {
    match frontmatter.count("metadata") {
        // A construct the parser does not model can hide the key itself.
        0 if frontmatter.unparsed().is_some() && frontmatter.raw_block().contains(PROPOSAL_KEY) => {
            None
        }
        0 => Some(&[]),
        1 => match frontmatter.get("metadata") {
            Some(FieldValue::Map(pairs)) => {
                let repeated = pairs
                    .iter()
                    .enumerate()
                    .any(|(at, (key, _))| pairs[..at].iter().any(|(earlier, _)| earlier == key));
                (!repeated).then_some(pairs.as_slice())
            }
            _ => None,
        },
        _ => None,
    }
}

fn adoption(frontmatter: &Frontmatter) -> Adoption {
    match metadata_of(frontmatter) {
        None => Adoption::Unreadable,
        Some(pairs) if pairs.iter().any(|(key, _)| key == PROPOSAL_KEY) => Adoption::Proposed,
        Some(_) => Adoption::Adopted,
    }
}

/// The string `metadata.<key>` of a `SKILL.md`'s text, when it holds one
/// and its `metadata` is readable ([`metadata_of`]).
pub fn metadata_value(text: &str, key: &str) -> Option<String> {
    metadata_of(&Frontmatter::parse(text).0)?
        .iter()
        .find_map(|(name, value)| match value {
            FieldValue::Str(value) if name == key => Some(value.clone()),
            _ => None,
        })
}

/// `text` with `metadata.<key>` set to `value`, or removed when `value` is
/// `None`: the `metadata` block re-rendered — its other keys in their order,
/// a set key last — and every other byte kept. A `metadata` left empty goes.
pub fn set_metadata(text: &str, key: &str, value: Option<&str>) -> String {
    let (frontmatter, _) = Frontmatter::parse(text);
    let mut pairs: Vec<(String, FieldValue)> = match frontmatter.get("metadata") {
        Some(FieldValue::Map(pairs)) => pairs.clone(),
        _ => Vec::new(),
    };
    pairs.retain(|(name, _)| name != key);
    if let Some(value) = value {
        pairs.push((key.to_owned(), FieldValue::Str(value.to_owned())));
    }
    if pairs.is_empty() {
        if frontmatter.get("metadata").is_some() {
            return Frontmatter::remove_in(text, "metadata");
        }
        return text.to_owned();
    }
    Frontmatter::set_in(text, "metadata", FieldValue::Map(pairs))
}

/// Validate every found skill and offer the wanted, valid ones.
pub fn index(found: &[(String, String)], wanted: &SkillFilter) -> SkillsIndex {
    let mut sorted: Vec<&(String, String)> = found.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = SkillsIndex::default();
    for (dir, text) in sorted.into_iter().filter(|(dir, _)| !dir.starts_with('.')) {
        if text.len() > MAX_SKILL_BYTES {
            out.refused.push((
                dir.clone(),
                vec![format!(
                    "SKILL.md is {} bytes, and keeper reads at most {MAX_SKILL_BYTES}",
                    text.len()
                )],
            ));
            continue;
        }
        let (frontmatter, body_offset) = Frontmatter::parse(text);
        let fields: Vec<(&str, MetaValue<'_>)> = frontmatter
            .keys()
            .map(|key| {
                let value = match frontmatter.get(key) {
                    Some(FieldValue::Str(s)) => MetaValue::Str(s),
                    _ => MetaValue::Other,
                };
                (key, value)
            })
            .collect();
        let errors = validate_metadata(&fields, Some(dir));
        if !errors.is_empty() {
            out.refused.push((dir.clone(), errors));
            continue;
        }
        let lines = text[body_offset..].lines().count();
        if lines > BODY_WARN_LINES {
            out.warnings.push(format!(
                "_skills/{dir}/SKILL.md's body is {lines} lines; agentskills recommends fewer than \
                 {BODY_WARN_LINES}."
            ));
        }
        match adoption(&frontmatter) {
            Adoption::Adopted => {}
            Adoption::Proposed => {
                out.waiting.push(dir.clone());
                continue;
            }
            Adoption::Unreadable => {
                out.refused
                    .push((dir.clone(), vec![UNREADABLE_METADATA.to_owned()]));
                continue;
            }
        }
        let wanted_here = match wanted {
            SkillFilter::All => true,
            SkillFilter::Named(names) => names.iter().any(|name| name == dir),
        };
        if wanted_here {
            out.offered.push(SkillEntry {
                name: frontmatter
                    .as_string("name")
                    .unwrap_or(dir)
                    .trim()
                    .to_owned(),
                description: frontmatter
                    .as_string("description")
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
    }
    if let SkillFilter::Named(names) = wanted {
        for name in names {
            if !found.iter().any(|(dir, _)| dir == name) {
                out.warnings
                    .push(format!("{name} is named in agent.toml, not in _skills/."));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture `_skills/`, as the host's walk hands it over.
    fn found() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agents/zone-ok/_skills");
        let mut found: Vec<(String, String)> = std::fs::read_dir(&root)
            .expect("_skills is readable")
            .map(|entry| {
                let entry = entry.expect("dirent");
                let name = entry.file_name().to_string_lossy().into_owned();
                let text =
                    std::fs::read_to_string(entry.path().join("SKILL.md")).expect("SKILL.md");
                (name, text)
            })
            .collect();
        found.sort();
        found
    }

    fn names(index: &SkillsIndex) -> Vec<&str> {
        index.offered.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn the_fixture_offers_three_and_refuses_two_with_agentskills_reasons() {
        let index = index(&found(), &SkillFilter::from_list(&["*".to_owned()]));
        assert_eq!(names(&index), ["inbox-triage", "okf-note", "weekly-review"]);
        assert_eq!(
            index.offered[0].description,
            "Read what arrived in 00-inbox/ today and propose one card per item, each with an owner and a next step."
        );
        let refused: Vec<&str> = index.refused.iter().map(|(dir, _)| dir.as_str()).collect();
        assert_eq!(refused, ["My_Skill", "mismatch"]);
        let reasons = |dir: &str| {
            index
                .refused
                .iter()
                .find(|(d, _)| d == dir)
                .map(|(_, r)| r.join(" "))
                .unwrap_or_default()
        };
        assert!(reasons("My_Skill").contains("must be lowercase"));
        assert!(reasons("My_Skill").contains("invalid characters"));
        assert!(reasons("mismatch").contains("must match skill name"));
        assert_eq!(index.warnings, Vec::<String>::new());
    }

    #[test]
    fn a_skill_without_frontmatter_is_refused() {
        let index = index(
            &[("bare".to_owned(), "# Just a body\n".to_owned())],
            &SkillFilter::All,
        );
        assert!(index.offered.is_empty());
        assert_eq!(
            index.refused[0].1[0],
            "Missing required field in frontmatter: name"
        );
    }

    #[test]
    fn a_long_body_is_warned_and_still_offered() {
        let long = format!(
            "---\nname: long\ndescription: d\n---\n{}",
            "line\n".repeat(501)
        );
        let index = index(&[("long".to_owned(), long)], &SkillFilter::All);
        assert_eq!(names(&index), ["long"]);
        assert_eq!(
            index.warnings,
            ["_skills/long/SKILL.md's body is 501 lines; agentskills recommends fewer than 500."]
        );
        let edge = format!(
            "---\nname: edge\ndescription: d\n---\n{}",
            "line\n".repeat(500)
        );
        assert!(
            super::index(&[("edge".to_owned(), edge)], &SkillFilter::All)
                .warnings
                .is_empty()
        );
    }

    #[test]
    fn named_skills_offer_only_those_and_name_the_absent() {
        let wanted = SkillFilter::from_list(&["okf-note".to_owned(), "web".to_owned()]);
        let index = index(&found(), &wanted);
        assert_eq!(names(&index), ["okf-note"]);
        assert_eq!(
            index.warnings,
            ["web is named in agent.toml, not in _skills/."]
        );
        assert_eq!(
            index.refused.len(),
            2,
            "refusals are listed whatever is wanted"
        );
    }

    #[test]
    fn an_oversized_skill_is_refused_with_its_size() {
        let big = format!(
            "---\nname: big\ndescription: d\n---\n{}",
            "x".repeat(MAX_SKILL_BYTES)
        );
        let size = big.len();
        let index = index(&[("big".to_owned(), big)], &SkillFilter::All);
        assert!(index.offered.is_empty());
        assert_eq!(
            index.refused[0].1,
            [format!(
                "SKILL.md is {size} bytes, and keeper reads at most 262144"
            )]
        );
        let fits = "---\nname: fits\ndescription: d\n---\n".to_owned();
        let fits = format!("{fits}{}", "x".repeat(MAX_SKILL_BYTES - fits.len()));
        assert_eq!(
            names(&super::index(
                &[("fits".to_owned(), fits)],
                &SkillFilter::All
            )),
            ["fits"]
        );
    }

    /// R28 S-12: a skill carrying `metadata.keeper_proposal` is listed as
    /// waiting for a person and offered to no one, by name or by `*`; the
    /// same skill without the key — adopted — is offered; a dotted folder
    /// is never read as a skill.
    #[test]
    fn a_proposed_skill_waits_for_a_person() {
        let proposed = "---\nname: tidy\ndescription: Tidy the inbox.\nmetadata:\n  keeper_proposal: 01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z\n  author: nixi\n---\nSteps.\n";
        let adopted = "---\nname: tidy\ndescription: Tidy the inbox.\nmetadata:\n  author: nixi\n---\nSteps.\n";
        let archived = "---\nname: old\ndescription: d\n---\n";
        for wanted in [
            SkillFilter::All,
            SkillFilter::from_list(&["tidy".to_owned()]),
        ] {
            let found = [
                ("tidy".to_owned(), proposed.to_owned()),
                (".archive".to_owned(), archived.to_owned()),
            ];
            let waiting = index(&found, &wanted);
            assert!(waiting.offered.is_empty(), "{wanted:?}");
            assert_eq!(waiting.waiting, ["tidy"]);
            assert!(waiting.refused.is_empty());
            let adopted = index(&[("tidy".to_owned(), adopted.to_owned())], &wanted);
            assert_eq!(names(&adopted), ["tidy"]);
            assert!(adopted.waiting.is_empty());
        }
    }

    /// R204: metadata keeper cannot read as one block map of distinct keys
    /// — a flow map, a nested value that makes the map opaque, a key said
    /// twice, `metadata` itself said twice — is not offered: refused with
    /// the reason, never taken for adopted.
    #[test]
    fn unreadable_adoption_metadata_is_not_offered() {
        let head = "---\nname: tidy\ndescription: Tidy the inbox.\n";
        for metadata in [
            "metadata: {keeper_proposal: pending}\n",
            "metadata:\n  keeper_proposal: pending\n  extra:\n    deep: x\n",
            "metadata:\n  keeper_proposal: |\n    pending\n",
            "metadata:\n  author: nixi\n  author: tgorka\n",
            "metadata:\n  author: nixi\nmetadata:\n  keeper_proposal: pending\n",
        ] {
            let text = format!("{head}{metadata}---\nSteps.\n");
            let index = index(&[("tidy".to_owned(), text)], &SkillFilter::All);
            assert!(index.offered.is_empty(), "{metadata:?}");
            assert_eq!(
                index.refused,
                [("tidy".to_owned(), vec![UNREADABLE_METADATA.to_owned()])],
                "{metadata:?}"
            );
        }
    }
}

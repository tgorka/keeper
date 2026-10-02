//! The drive's `_skills/`: agentskills validation and the offered index (AD-362, story 89.3).
//!
//! Each `_skills/<name>/SKILL.md` is read with keeper's frontmatter subset and
//! validated by `keeper_ported::agentskills`, so a refused skill carries the
//! reference validator's own sentences. A refused skill is listed with its
//! reasons and never offered. The prompt carries name and description only;
//! the body loads through `skill_view` (§9.1).
//!
//! Pure: the host walks `_skills/` (skipping `.archive/`) through
//! `browse::resolve` and hands each directory name with its `SKILL.md` text.

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

/// Validate every found skill and offer the wanted, valid ones.
pub fn index(found: &[(String, String)], wanted: &SkillFilter) -> SkillsIndex {
    let mut sorted: Vec<&(String, String)> = found.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = SkillsIndex::default();
    for (dir, text) in sorted {
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
}

// Ported from agentskills/agentskills, licensed under the Apache License,
// Version 2.0 (`UPSTREAM.md` beside this file names the licence and commit).
//
// Apache-2.0 §4(b) notice: this file is MODIFIED from
// `skills-ref/src/skills_ref/validator.py` at agentskills/agentskills@69ef37e9.
// It was rewritten from Python into Rust; it validates frontmatter fields the
// caller already parsed (`validate_metadata`) and leaves out upstream's
// directory and YAML reading (`validate`, `parser.py`). Lengths count Unicode
// scalar values (Python's `len`), case is `to_lowercase` (Python's `lower`),
// NFKC comes from `unicode-normalization`, and `isalnum` is
// `char::is_alphanumeric`. Every error sentence is upstream's, verbatim.

//! The agentskills.io `SKILL.md` metadata validator (AD-396).
//!
//! keeper parses `SKILL.md` with its own frontmatter subset and hands the
//! fields here; the sentences that come back are agentskills' own, so a skill
//! refused in keeper is refused for the reason the reference tool gives.

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;

pub const MAX_SKILL_NAME_LENGTH: usize = 64;
pub const MAX_DESCRIPTION_LENGTH: usize = 1024;
pub const MAX_COMPATIBILITY_LENGTH: usize = 500;

/// The frontmatter fields the Agent Skills spec allows, sorted as upstream's
/// `sorted(ALLOWED_FIELDS)` prints them.
pub const ALLOWED_FIELDS: [&str; 6] = [
    "allowed-tools",
    "compatibility",
    "description",
    "license",
    "metadata",
    "name",
];

/// One frontmatter value, as much of it as the validator looks at: a string,
/// or anything else (a list, a map, a number), which upstream's `isinstance(…,
/// str)` checks refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaValue<'a> {
    Str(&'a str),
    Other,
}

impl<'a> MetaValue<'a> {
    fn as_str(self) -> Option<&'a str> {
        match self {
            Self::Str(s) => Some(s),
            Self::Other => None,
        }
    }
}

/// Validate parsed skill metadata; an empty list means valid.
///
/// `fields` are the frontmatter's top-level keys in source order; a key given
/// twice reads as its last value, as a YAML mapping loaded into a dict does.
/// `dir_name` is the skill's directory name, when the name-directory match is
/// to be checked.
pub fn validate_metadata(fields: &[(&str, MetaValue<'_>)], dir_name: Option<&str>) -> Vec<String> {
    let mut errors = validate_metadata_fields(fields);
    let field = |key: &str| {
        fields
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    };

    match field("name") {
        None => errors.push("Missing required field in frontmatter: name".to_owned()),
        Some(name) => errors.extend(validate_name(name, dir_name)),
    }
    match field("description") {
        None => errors.push("Missing required field in frontmatter: description".to_owned()),
        Some(description) => errors.extend(validate_description(description)),
    }
    if let Some(compatibility) = field("compatibility") {
        errors.extend(validate_compatibility(compatibility));
    }
    errors
}

fn validate_name(name: MetaValue<'_>, dir_name: Option<&str>) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(raw) = name.as_str().filter(|s| !s.trim().is_empty()) else {
        errors.push("Field 'name' must be a non-empty string".to_owned());
        return errors;
    };

    let name: String = raw.trim().nfkc().collect();
    let length = name.chars().count();

    if length > MAX_SKILL_NAME_LENGTH {
        errors.push(format!(
            "Skill name '{name}' exceeds {MAX_SKILL_NAME_LENGTH} character limit ({length} chars)"
        ));
    }
    if name != name.to_lowercase() {
        errors.push(format!("Skill name '{name}' must be lowercase"));
    }
    if name.starts_with('-') || name.ends_with('-') {
        errors.push("Skill name cannot start or end with a hyphen".to_owned());
    }
    if name.contains("--") {
        errors.push("Skill name cannot contain consecutive hyphens".to_owned());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        errors.push(format!(
            "Skill name '{name}' contains invalid characters. Only letters, digits, and hyphens \
             are allowed."
        ));
    }
    if let Some(dir) = dir_name {
        let normalised: String = dir.nfkc().collect();
        if normalised != name {
            errors.push(format!(
                "Directory name '{dir}' must match skill name '{name}'"
            ));
        }
    }
    errors
}

fn validate_description(description: MetaValue<'_>) -> Vec<String> {
    let Some(description) = description.as_str().filter(|s| !s.trim().is_empty()) else {
        return vec!["Field 'description' must be a non-empty string".to_owned()];
    };
    let length = description.chars().count();
    if length > MAX_DESCRIPTION_LENGTH {
        return vec![format!(
            "Description exceeds {MAX_DESCRIPTION_LENGTH} character limit ({length} chars)"
        )];
    }
    Vec::new()
}

fn validate_compatibility(compatibility: MetaValue<'_>) -> Vec<String> {
    let Some(compatibility) = compatibility.as_str() else {
        return vec!["Field 'compatibility' must be a string".to_owned()];
    };
    let length = compatibility.chars().count();
    if length > MAX_COMPATIBILITY_LENGTH {
        return vec![format!(
            "Compatibility exceeds {MAX_COMPATIBILITY_LENGTH} character limit ({length} chars)"
        )];
    }
    Vec::new()
}

fn validate_metadata_fields(fields: &[(&str, MetaValue<'_>)]) -> Vec<String> {
    let extra: BTreeSet<&str> = fields
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| !ALLOWED_FIELDS.contains(key))
        .collect();
    if extra.is_empty() {
        return Vec::new();
    }
    let extra: Vec<&str> = extra.into_iter().collect();
    // Upstream interpolates `sorted(ALLOWED_FIELDS)`, a Python list repr.
    let allowed: Vec<String> = ALLOWED_FIELDS.iter().map(|f| format!("'{f}'")).collect();
    vec![format!(
        "Unexpected fields in frontmatter: {}. Only [{}] are allowed.",
        extra.join(", "),
        allowed.join(", ")
    )]
}

#[cfg(test)]
mod tests {
    //! Upstream's metadata cases (`skills-ref/tests/test_validator.py` at
    //! `69ef37e9`), each asserting upstream's substring. The directory cases
    //! (`nonexistent_path`, `not_a_directory`, `missing_skill_md`) belong to
    //! the reader keeper wrote instead of `validate`.
    use super::*;
    use MetaValue::{Other, Str};

    fn validate(dir: &str, fields: &[(&str, MetaValue<'_>)]) -> Vec<String> {
        validate_metadata(fields, Some(dir))
    }

    fn any(errors: &[String], needles: &[&str]) -> bool {
        errors
            .iter()
            .any(|e| needles.iter().all(|needle| e.contains(needle)))
    }

    #[test]
    fn valid_skill() {
        let errors = validate(
            "my-skill",
            &[
                ("name", Str("my-skill")),
                ("description", Str("A test skill")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn invalid_name_uppercase() {
        let errors = validate(
            "MySkill",
            &[
                ("name", Str("MySkill")),
                ("description", Str("A test skill")),
            ],
        );
        assert!(any(&errors, &["lowercase"]), "{errors:?}");
    }

    #[test]
    fn name_too_long() {
        let long = "a".repeat(70);
        let errors = validate(
            &long,
            &[("name", Str(&long)), ("description", Str("A test skill"))],
        );
        assert!(any(&errors, &["exceeds", "character limit"]), "{errors:?}");
        assert!(any(&errors, &["(70 chars)"]), "{errors:?}");
        let edge = "a".repeat(64);
        let errors = validate(
            &edge,
            &[("name", Str(&edge)), ("description", Str("A test skill"))],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn name_leading_hyphen() {
        let errors = validate(
            "-my-skill",
            &[
                ("name", Str("-my-skill")),
                ("description", Str("A test skill")),
            ],
        );
        assert!(
            any(&errors, &["cannot start or end with a hyphen"]),
            "{errors:?}"
        );
    }

    #[test]
    fn name_consecutive_hyphens() {
        let errors = validate(
            "my--skill",
            &[
                ("name", Str("my--skill")),
                ("description", Str("A test skill")),
            ],
        );
        assert!(any(&errors, &["consecutive hyphens"]), "{errors:?}");
    }

    #[test]
    fn name_invalid_characters() {
        let errors = validate(
            "my_skill",
            &[
                ("name", Str("my_skill")),
                ("description", Str("A test skill")),
            ],
        );
        assert!(any(&errors, &["invalid characters"]), "{errors:?}");
    }

    #[test]
    fn name_directory_mismatch() {
        let errors = validate(
            "wrong-name",
            &[
                ("name", Str("correct-name")),
                ("description", Str("A test skill")),
            ],
        );
        assert!(any(&errors, &["must match skill name"]), "{errors:?}");
        let errors = validate_metadata(
            &[
                ("name", Str("correct-name")),
                ("description", Str("A test skill")),
            ],
            None,
        );
        assert_eq!(errors, Vec::<String>::new(), "no directory, no match check");
    }

    #[test]
    fn unexpected_fields() {
        let errors = validate(
            "my-skill",
            &[
                ("name", Str("my-skill")),
                ("description", Str("A test skill")),
                ("unknown_field", Str("should not be here")),
            ],
        );
        assert_eq!(
            errors,
            vec![
                "Unexpected fields in frontmatter: unknown_field. Only ['allowed-tools', \
                 'compatibility', 'description', 'license', 'metadata', 'name'] are allowed."
                    .to_owned()
            ]
        );
    }

    #[test]
    fn valid_with_all_fields() {
        let errors = validate(
            "my-skill",
            &[
                ("name", Str("my-skill")),
                ("description", Str("A test skill")),
                ("license", Str("MIT")),
                ("metadata", Other),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn allowed_tools_accepted() {
        let errors = validate(
            "my-skill",
            &[
                ("name", Str("my-skill")),
                ("description", Str("A test skill")),
                ("allowed-tools", Str("Bash(jq:*) Bash(git:*)")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn i18n_chinese_name() {
        let errors = validate(
            "技能",
            &[
                ("name", Str("技能")),
                ("description", Str("A skill with Chinese name")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn i18n_russian_name_with_hyphens() {
        let errors = validate(
            "мой-навык",
            &[
                ("name", Str("мой-навык")),
                ("description", Str("A skill with Russian name")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn i18n_russian_lowercase_valid() {
        let errors = validate(
            "навык",
            &[
                ("name", Str("навык")),
                ("description", Str("A skill with Russian lowercase name")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn i18n_russian_uppercase_rejected() {
        let errors = validate(
            "НАВЫК",
            &[
                ("name", Str("НАВЫК")),
                ("description", Str("A skill with Russian uppercase name")),
            ],
        );
        assert!(any(&errors, &["lowercase"]), "{errors:?}");
    }

    #[test]
    fn description_too_long() {
        let long = "x".repeat(1100);
        let errors = validate(
            "my-skill",
            &[("name", Str("my-skill")), ("description", Str(&long))],
        );
        assert!(any(&errors, &["exceeds", "1024"]), "{errors:?}");
        let edge = "x".repeat(1024);
        let errors = validate(
            "my-skill",
            &[("name", Str("my-skill")), ("description", Str(&edge))],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn valid_compatibility() {
        let errors = validate(
            "my-skill",
            &[
                ("name", Str("my-skill")),
                ("description", Str("A test skill")),
                ("compatibility", Str("Requires Python 3.11+")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn compatibility_too_long() {
        let long = "x".repeat(550);
        let errors = validate(
            "my-skill",
            &[
                ("name", Str("my-skill")),
                ("description", Str("A test skill")),
                ("compatibility", Str(&long)),
            ],
        );
        assert!(any(&errors, &["exceeds", "500"]), "{errors:?}");
    }

    #[test]
    fn nfkc_normalization() {
        let errors = validate(
            "café",
            &[
                ("name", Str("cafe\u{301}")),
                ("description", Str("A test skill")),
            ],
        );
        assert_eq!(errors, Vec::<String>::new());
        let errors = validate(
            "cafe\u{301}",
            &[("name", Str("café")), ("description", Str("A test skill"))],
        );
        assert_eq!(
            errors,
            Vec::<String>::new(),
            "the directory is normalised too"
        );
    }

    #[test]
    fn missing_and_non_string_fields_use_upstream_sentences() {
        let errors = validate_metadata(&[("license", Str("MIT"))], None);
        assert_eq!(
            errors,
            vec![
                "Missing required field in frontmatter: name".to_owned(),
                "Missing required field in frontmatter: description".to_owned(),
            ]
        );
        let errors = validate_metadata(
            &[
                ("name", Other),
                ("description", Str("  ")),
                ("compatibility", Other),
            ],
            None,
        );
        assert_eq!(
            errors,
            vec![
                "Field 'name' must be a non-empty string".to_owned(),
                "Field 'description' must be a non-empty string".to_owned(),
                "Field 'compatibility' must be a string".to_owned(),
            ]
        );
    }
}

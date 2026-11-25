//! `_drive.toml`: a drive's id, principal, owner and readers (AD-361, story 89.2).
//!
//! The file is a contract: an unknown key is refused with its name (the D-30
//! precedent), a newer `version` is refused rather than half-read, and every
//! refusal is one sentence a person can act on. Parsing is pure — the host
//! reads the file through `keeper_paths::resolve` and hands the text here.
//!
//! The grammar, `local_only`, and `[integrity]` are read by
//! `keeper_files_api::drive_file`; `parse` keeps the Matrix-typed owner and readers.

use std::collections::BTreeSet;

use keeper_files_api::drive_file::{self, DriveFileRefusal};
use matrix_sdk::ruma::OwnedUserId;

/// The file's name inside an agents zone.
pub const FILE_NAME: &str = "_drive.toml";

const TITLE_MAX: usize = 64;

/// A drive's declaration, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveDecl {
    /// `[a-z0-9][a-z0-9-]{0,31}`.
    pub id: String,
    /// At most 64 characters; the id when the file has none.
    pub title: String,
    /// `[a-z0-9-]{1,32}`: the process that homes this drive's agents (AD-377).
    pub principal: String,
    /// One of `readers`.
    pub owner: OwnedUserId,
    /// The humans who may read this drive; never empty.
    pub readers: BTreeSet<OwnedUserId>,
    /// Every agent homed here must pin a local model (AD-377).
    pub local_only: bool,
    /// Drive-relative glob patterns whose files are `untrusted`, in the
    /// file's order; [`keeper_files_api::drive_file::DEFAULT_UNTRUSTED`] when
    /// there is no `[integrity]`.
    pub untrusted: Vec<String>,
}

/// Why a `_drive.toml` cannot be read. Each is the sentence the zone shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriveDeclRefusal {
    #[error(transparent)]
    File(#[from] DriveFileRefusal),
    #[error(
        "`id` \"{id}\" does not fit [a-z0-9][a-z0-9-]{{0,31}}: lowercase letters, digits and dashes, starting with a letter or digit, at most 32."
    )]
    BadId { id: String },
    #[error("`title` is {chars} characters; a drive's title is at most {TITLE_MAX}.")]
    TitleTooLong { chars: usize },
    #[error(
        "`principal` \"{principal}\" does not fit [a-z0-9-]{{1,32}}: lowercase letters, digits and dashes, at most 32."
    )]
    BadPrincipal { principal: String },
    #[error("`{key}` holds \"{value}\", which is not a Matrix user id (@name:server).")]
    NotAMatrixId { key: &'static str, value: String },
    #[error("`readers` names nobody; a drive needs at least one reader.")]
    NoReaders,
    #[error("`readers` names {reader} twice.")]
    DuplicateReader { reader: String },
    #[error("The owner {owner} is not among `readers`; the owner must be a reader.")]
    OwnerNotReader { owner: String },
}

impl DriveDeclRefusal {
    /// The refusal as the sentence a person reads.
    pub fn sentence(&self) -> String {
        self.to_string()
    }
}

/// Read a `_drive.toml`.
pub fn parse(text: &str) -> Result<DriveDecl, DriveDeclRefusal> {
    let table = drive_file::table(text)?;
    drive_file::version(&table)?;

    let id = text_of(&table, "id")?;
    if !fits_id(&id) {
        return Err(DriveDeclRefusal::BadId { id });
    }
    let title = match table.get("title") {
        None => id.clone(),
        Some(_) => text_of(&table, "title")?,
    };
    let chars = title.chars().count();
    if chars > TITLE_MAX {
        return Err(DriveDeclRefusal::TitleTooLong { chars });
    }
    let principal = text_of(&table, "principal")?;
    if !fits_principal(&principal) {
        return Err(DriveDeclRefusal::BadPrincipal { principal });
    }

    let owner = user_id("owner", &text_of(&table, "owner")?)?;
    let readers = audience(&owner, &reader_texts(&table)?)?;

    let local_only = drive_file::local_only(&table)?;

    Ok(DriveDecl {
        id,
        title,
        principal,
        owner,
        readers,
        local_only,
        untrusted: drive_file::untrusted(&table)?,
    })
}

fn text_of(table: &toml::Table, key: &'static str) -> Result<String, DriveDeclRefusal> {
    match drive_file::required(table, key)? {
        toml::Value::String(value) => Ok(value.clone()),
        other => Err(drive_file::wrong(key, "text in quotes", other).into()),
    }
}

fn reader_texts(table: &toml::Table) -> Result<Vec<String>, DriveDeclRefusal> {
    let items = match drive_file::required(table, "readers")? {
        toml::Value::Array(items) => items,
        other => {
            return Err(drive_file::wrong("readers", "a list of Matrix user ids", other).into());
        }
    };
    items
        .iter()
        .map(|item| match item {
            toml::Value::String(raw) => Ok(raw.clone()),
            other => Err(drive_file::wrong("readers", "a list of Matrix user ids", other).into()),
        })
        .collect()
}

/// A drive's audience as `_drive.toml` and a host's pin both state it: Matrix
/// ids, at least one, none twice, and the owner among them (S-15).
pub fn audience(
    owner: &OwnedUserId,
    readers: &[String],
) -> Result<BTreeSet<OwnedUserId>, DriveDeclRefusal> {
    if readers.is_empty() {
        return Err(DriveDeclRefusal::NoReaders);
    }
    let mut set = BTreeSet::new();
    for raw in readers {
        let reader = user_id("readers", raw)?;
        if set.contains(&reader) {
            return Err(DriveDeclRefusal::DuplicateReader {
                reader: reader.to_string(),
            });
        }
        set.insert(reader);
    }
    if !set.contains(owner) {
        return Err(DriveDeclRefusal::OwnerNotReader {
            owner: owner.to_string(),
        });
    }
    Ok(set)
}

fn user_id(key: &'static str, raw: &str) -> Result<OwnedUserId, DriveDeclRefusal> {
    OwnedUserId::try_from(raw).map_err(|_| DriveDeclRefusal::NotAMatrixId {
        key,
        value: raw.to_owned(),
    })
}

fn lower_digit_or_dash(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

/// `[a-z0-9][a-z0-9-]{0,31}`.
fn fits_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.len() <= 32
        && chars.all(lower_digit_or_dash)
}

/// `[a-z0-9-]{1,32}`.
fn fits_principal(principal: &str) -> bool {
    (1..=32).contains(&principal.len()) && principal.chars().all(lower_digit_or_dash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_files_api::drive_file::DEFAULT_UNTRUSTED;

    /// The architecture's example, its `<homeserver>` filled in.
    const NEURADRIVE: &str = r#"
version   = 1
id        = "neuradrive"
title     = "neuradrive"
principal = "neuraffica"
owner     = "@tgorka:example.org"
readers   = ["@marta:example.org", "@tgorka:example.org"]
"#;

    fn uid(raw: &str) -> OwnedUserId {
        OwnedUserId::try_from(raw).expect("a test id")
    }

    fn refusal(text: &str) -> DriveDeclRefusal {
        parse(text).expect_err("refused")
    }

    fn with(line: &str) -> String {
        format!("{NEURADRIVE}{line}\n")
    }

    fn replacing(from: &str, to: &str) -> String {
        assert!(NEURADRIVE.contains(from), "{from}");
        NEURADRIVE.replace(from, to)
    }

    #[test]
    fn the_architectures_neuradrive_example_parses() {
        let drive = parse(NEURADRIVE).expect("parses");
        assert_eq!(drive.id, "neuradrive");
        assert_eq!(drive.title, "neuradrive");
        assert_eq!(drive.principal, "neuraffica");
        assert_eq!(drive.owner, uid("@tgorka:example.org"));
        assert_eq!(
            drive
                .readers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["@marta:example.org", "@tgorka:example.org"]
        );
        assert!(!drive.local_only);
        assert_eq!(drive.untrusted, DEFAULT_UNTRUSTED);
    }

    #[test]
    fn a_title_defaults_to_the_id_and_is_at_most_64_characters() {
        let untitled = replacing("title     = \"neuradrive\"\n", "");
        assert_eq!(parse(&untitled).expect("parses").title, "neuradrive");
        let at_cap = replacing(
            "\"neuradrive\"\nprincipal",
            &format!("\"{}\"\nprincipal", "é".repeat(64)),
        );
        assert_eq!(
            parse(&at_cap).expect("64 is allowed").title.chars().count(),
            64
        );
        let over = replacing(
            "\"neuradrive\"\nprincipal",
            &format!("\"{}\"\nprincipal", "é".repeat(65)),
        );
        assert_eq!(
            refusal(&over).sentence(),
            "`title` is 65 characters; a drive's title is at most 64."
        );
    }

    #[test]
    fn a_typo_key_is_refused_naming_it() {
        let typo = replacing("readers   =", "reader =");
        assert_eq!(
            refusal(&typo).sentence(),
            "_drive.toml has `reader`, which is not one of its keys."
        );
    }

    #[test]
    fn a_newer_version_is_refused_as_written_by_a_newer_keeper() {
        let newer = replacing("version   = 1", "version = 2");
        let sentence = refusal(&newer).sentence();
        assert!(sentence.contains("written by a newer keeper"), "{sentence}");
        assert_eq!(
            refusal(&replacing("version   = 1", "version = 0")),
            DriveDeclRefusal::File(DriveFileRefusal::BadVersion { version: 0 })
        );
        assert_eq!(
            refusal(&replacing("version   = 1\n", "")),
            DriveDeclRefusal::File(DriveFileRefusal::Missing { key: "version" })
        );
    }

    #[test]
    fn an_id_outside_the_pattern_is_refused_with_the_pattern() {
        let sentence = refusal(&replacing(
            "id        = \"neuradrive\"",
            "id = \"TG_Drive\"",
        ))
        .sentence();
        assert_eq!(
            sentence,
            "`id` \"TG_Drive\" does not fit [a-z0-9][a-z0-9-]{0,31}: lowercase letters, digits and dashes, starting with a letter or digit, at most 32."
        );
        for bad in ["-tg", "", &"a".repeat(33)] {
            let text = replacing("id        = \"neuradrive\"", &format!("id = \"{bad}\""));
            assert!(
                matches!(refusal(&text), DriveDeclRefusal::BadId { .. }),
                "{bad}"
            );
        }
        let longest = replacing(
            "id        = \"neuradrive\"",
            &format!("id = \"{}\"", "a".repeat(32)),
        );
        assert!(parse(&longest).is_ok());
        let digit_first = replacing("id        = \"neuradrive\"", "id = \"9-lives\"");
        assert!(parse(&digit_first).is_ok());
    }

    #[test]
    fn a_principal_outside_the_pattern_is_refused() {
        for bad in ["", "Neuraffica", &"p".repeat(33)] {
            let text = replacing("\"neuraffica\"", &format!("\"{bad}\""));
            assert!(
                matches!(refusal(&text), DriveDeclRefusal::BadPrincipal { .. }),
                "{bad}"
            );
        }
        assert!(parse(&replacing("\"neuraffica\"", "\"-\"")).is_ok());
    }

    #[test]
    fn an_owner_who_is_not_a_reader_is_refused() {
        let text = replacing(
            "owner     = \"@tgorka:example.org\"",
            "owner = \"@nobody:example.org\"",
        );
        assert_eq!(
            refusal(&text).sentence(),
            "The owner @nobody:example.org is not among `readers`; the owner must be a reader."
        );
    }

    #[test]
    fn empty_duplicate_and_non_matrix_readers_are_refused() {
        let readers = "readers   = [\"@marta:example.org\", \"@tgorka:example.org\"]";
        assert_eq!(
            refusal(&replacing(readers, "readers = []")),
            DriveDeclRefusal::NoReaders
        );
        assert_eq!(
            refusal(&replacing(
                readers,
                "readers = [\"@tgorka:example.org\", \"@tgorka:example.org\"]"
            ))
            .sentence(),
            "`readers` names @tgorka:example.org twice."
        );
        assert_eq!(
            refusal(&replacing(
                readers,
                "readers = [\"marta\", \"@tgorka:example.org\"]"
            ))
            .sentence(),
            "`readers` holds \"marta\", which is not a Matrix user id (@name:server)."
        );
        assert_eq!(
            refusal(&replacing(
                "owner     = \"@tgorka:example.org\"",
                "owner = \"tgorka\""
            )),
            DriveDeclRefusal::NotAMatrixId {
                key: "owner",
                value: "tgorka".to_owned()
            }
        );
    }

    #[test]
    fn local_only_of_the_wrong_type_is_refused_naming_its_type() {
        assert_eq!(
            refusal(&with("local_only = \"yes\"")).sentence(),
            "`local_only` in _drive.toml must be true or false, not text."
        );
        assert!(
            parse(&with("local_only = true"))
                .expect("parses")
                .local_only
        );
    }

    #[test]
    fn unsorted_readers_are_accepted_and_reported_sorted() {
        let text = replacing(
            "readers   = [\"@marta:example.org\", \"@tgorka:example.org\"]",
            "readers = [\"@zed:example.org\", \"@tgorka:example.org\", \"@marta:example.org\"]",
        );
        let drive = parse(&text).expect("parses");
        assert_eq!(
            drive
                .readers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            [
                "@marta:example.org",
                "@tgorka:example.org",
                "@zed:example.org"
            ]
        );
    }

    #[test]
    fn an_integrity_table_replaces_the_default_untrusted_globs() {
        let own = parse(&with(
            "[integrity]\nuntrusted = [\"99-temp/**\", \"00-inbox/**\"]",
        ))
        .expect("parses");
        assert_eq!(own.untrusted, ["99-temp/**", "00-inbox/**"]);
        let none = parse(&with("[integrity]\nuntrusted = []")).expect("parses");
        assert!(none.untrusted.is_empty(), "an empty list means none");
        let bare = parse(&with("[integrity]")).expect("parses");
        assert!(
            bare.untrusted.is_empty(),
            "a present table replaces the default"
        );
    }

    #[test]
    fn integrity_refuses_unknown_keys_and_bad_globs() {
        assert_eq!(
            refusal(&with("[integrity]\ntrusted = []")).sentence(),
            "_drive.toml's [integrity] has `trusted`, which is not one of its keys."
        );
        assert!(matches!(
            refusal(&with("[integrity]\nuntrusted = [\"a/[b\"]")),
            DriveDeclRefusal::File(DriveFileRefusal::BadGlob { pattern, .. }) if pattern == "a/[b"
        ));
        assert_eq!(
            refusal(&with("[integrity]\nuntrusted = \"00-inbox/**\"")).sentence(),
            "`untrusted` in _drive.toml must be a list of glob patterns, not text."
        );
        assert_eq!(
            refusal(&with("integrity = 1")).sentence(),
            "`integrity` in _drive.toml must be a table, [integrity], not a whole number."
        );
    }

    #[test]
    fn text_that_is_not_toml_is_refused_with_the_parsers_line() {
        assert!(matches!(
            refusal("id = "),
            DriveDeclRefusal::File(DriveFileRefusal::Syntax(_))
        ));
        assert_eq!(
            refusal(&replacing("id        = \"neuradrive\"\n", "")),
            DriveDeclRefusal::File(DriveFileRefusal::Missing { key: "id" })
        );
    }
}

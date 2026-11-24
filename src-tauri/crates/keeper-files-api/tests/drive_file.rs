use keeper_files_api::drive_file::{read, DriveFileRefusal, DriveRules, DEFAULT_UNTRUSTED};

const BASE: &str = r#"version = 1
id = "tgdrive"
principal = "tg"
owner = "@t:example.org"
readers = ["@t:example.org"]
"#;

fn with(extra: &str) -> String {
    format!("{BASE}{extra}\n")
}

fn replacing(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "{from}");
    BASE.replace(from, to)
}

#[test]
fn the_untrusted_globs_union_holds_on_an_absent_table() {
    let rules = read(BASE).expect("reads");
    assert_eq!(rules.untrusted, DEFAULT_UNTRUSTED);
    assert_eq!(rules.untrusted_globs(), DEFAULT_UNTRUSTED);
}

#[test]
fn the_untrusted_globs_union_holds_on_an_empty_list() {
    let rules = read(&with("[integrity]\nuntrusted = []")).expect("reads");
    assert!(rules.untrusted.is_empty());
    assert_eq!(rules.untrusted_globs(), DEFAULT_UNTRUSTED);
}

#[test]
fn the_untrusted_globs_union_holds_on_a_table_without_the_key() {
    let rules = read(&with("[integrity]")).expect("reads");
    assert!(rules.untrusted.is_empty());
    assert_eq!(rules.untrusted_globs(), DEFAULT_UNTRUSTED);
}

#[test]
fn the_untrusted_globs_union_restores_an_omitted_default() {
    let rules = read(&with(
        "[integrity]\nuntrusted = [\"00-inbox/**\", \"recordings/**\"]",
    ))
    .expect("reads");
    assert_eq!(
        rules.untrusted_globs(),
        ["00-inbox/**", "recordings/**", "70-comms/**"]
    );
}

#[test]
fn the_untrusted_globs_union_keeps_an_added_glob() {
    let rules = read(&with("[integrity]\nuntrusted = [\"90-archive/mail/**\"]")).expect("reads");
    assert_eq!(
        rules.untrusted_globs(),
        [
            "90-archive/mail/**",
            "00-inbox/**",
            "70-comms/**",
            "recordings/**",
        ]
    );
}

#[test]
fn the_reader_refuses_as_the_grammar_says() {
    let unknown = read(&with("colour = \"blue\"")).expect_err("unknown key");
    assert_eq!(
        unknown.to_string(),
        "_drive.toml has `colour`, which is not one of its keys."
    );

    let two_unknown = read(&with("zeta = 1\nalpha = 2")).expect_err("two unknown keys");
    assert_eq!(
        two_unknown,
        DriveFileRefusal::UnknownKey {
            key: "alpha".to_owned()
        }
    );

    assert_eq!(
        read(&replacing("version = 1", "version = 2")).expect_err("newer version"),
        DriveFileRefusal::NewerVersion { version: 2 }
    );
    assert_eq!(
        read(&replacing("version = 1", "version = 0")).expect_err("version 0"),
        DriveFileRefusal::BadVersion { version: 0 }
    );
    assert_eq!(
        read(&replacing("version = 1\n", "")).expect_err("missing version"),
        DriveFileRefusal::Missing { key: "version" }
    );

    let wrong_local_only = read(&with("local_only = \"yes\"")).expect_err("wrong local_only");
    assert_eq!(
        wrong_local_only.to_string(),
        "`local_only` in _drive.toml must be true or false, not text."
    );

    assert_eq!(
        read(&with("[integrity]\ntrusted = []")).expect_err("unknown integrity key"),
        DriveFileRefusal::UnknownIntegrityKey {
            key: "trusted".to_owned()
        }
    );
    assert!(matches!(
        read(&with("[integrity]\nuntrusted = [\"a/[b\"]")).expect_err("bad glob"),
        DriveFileRefusal::BadGlob { pattern, .. } if pattern == "a/[b"
    ));
    assert!(matches!(
        read("id = ").expect_err("syntax error"),
        DriveFileRefusal::Syntax(_)
    ));
}

#[test]
fn the_reader_reads_local_only_and_the_list() {
    let text =
        with("local_only = true\n[integrity]\nuntrusted = [\"90-archive/**\", \"00-inbox/**\"]");
    let rules = read(&text).expect("reads");
    assert_eq!(
        rules,
        DriveRules {
            local_only: true,
            untrusted: vec!["90-archive/**".to_owned(), "00-inbox/**".to_owned()],
        }
    );

    let non_matrix_owner = replacing("owner = \"@t:example.org\"", "owner = \"not-a-matrix-id\"");
    let rules = read(&non_matrix_owner).expect("ignores Matrix ids");
    assert!(!rules.local_only);
    assert_eq!(rules.untrusted, DEFAULT_UNTRUSTED);
}

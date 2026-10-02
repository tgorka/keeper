//! Every ported module carries its provenance record (AD-396, NFR-119).

use std::path::{Path, PathBuf};

const KEYS: [&str; 9] = [
    "repository:",
    "commit:",
    "licence:",
    "copyright:",
    "files read:",
    "ported:",
    "not ported:",
    "changed:",
    "revisit:",
];

const WRITTEN_FROM_DOCS: &str = "written from documentation; no upstream code is copied";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `deny.toml`'s `[licenses] allow` list, read from the file so the record
/// test and the firewall cannot disagree.
fn allowed_licences() -> Vec<String> {
    let path = crate_dir().join("../../deny.toml");
    let text = std::fs::read_to_string(&path).expect("deny.toml is readable");
    let deny: toml::Table = toml::from_str(&text).expect("deny.toml parses");
    deny["licenses"]["allow"]
        .as_array()
        .expect("[licenses] allow is an array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("an allowed licence is a string")
                .to_owned()
        })
        .collect()
}

/// The problems with one module's record, empty when it is sound.
fn record_problems(module: &Path, allowed: &[String]) -> Vec<String> {
    let name = module
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Ok(text) = std::fs::read_to_string(module.join("UPSTREAM.md")) else {
        return vec![format!("{name} has no UPSTREAM.md")];
    };
    let head: Vec<&str> = text
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .collect();
    let mut problems = Vec::new();
    for (at, key) in KEYS.iter().enumerate() {
        match head.get(at) {
            Some(line) if line.starts_with(key) && !line[key.len()..].trim().is_empty() => {}
            _ => problems.push(format!(
                "{name}/UPSTREAM.md line {} must be `{key} …`",
                at + 1
            )),
        }
    }
    if let Some(licence) = head.get(2).and_then(|line| line.strip_prefix("licence:")) {
        let licence = licence.trim();
        if licence != WRITTEN_FROM_DOCS && !allowed.iter().any(|a| a == licence) {
            problems.push(format!(
                "{name}/UPSTREAM.md names licence `{licence}`, which is not on deny.toml's allow list"
            ));
        }
    }
    problems
}

fn modules() -> Vec<PathBuf> {
    let mut modules: Vec<PathBuf> = std::fs::read_dir(crate_dir().join("src"))
        .expect("src is readable")
        .map(|entry| entry.expect("dirent").path())
        .filter(|path| path.is_dir())
        .collect();
    modules.sort();
    modules
}

#[test]
fn every_module_has_an_upstream_record_whose_licence_is_allowed() {
    let allowed = allowed_licences();
    let modules = modules();
    assert!(!modules.is_empty(), "keeper-ported has modules to check");
    let problems: Vec<String> = modules
        .iter()
        .flat_map(|module| record_problems(module, &allowed))
        .collect();
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn a_record_is_refused_for_a_licence_off_the_allow_list_or_a_missing_key() {
    let allowed = allowed_licences();
    let dir = tempfile::tempdir().expect("tempdir");
    let module = dir.path().join("gpl");
    std::fs::create_dir(&module).expect("mkdir");
    let record = |licence: &str| {
        KEYS.iter()
            .map(|key| {
                if *key == "licence:" {
                    format!("{key} {licence}")
                } else {
                    format!("{key} x")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    std::fs::write(module.join("UPSTREAM.md"), record("GPL-3.0")).expect("write");
    assert_eq!(
        record_problems(&module, &allowed),
        vec!["gpl/UPSTREAM.md names licence `GPL-3.0`, which is not on deny.toml's allow list"]
    );

    std::fs::write(module.join("UPSTREAM.md"), record(WRITTEN_FROM_DOCS)).expect("write");
    assert_eq!(record_problems(&module, &allowed), Vec::<String>::new());

    std::fs::write(
        module.join("UPSTREAM.md"),
        record("MIT").replace("commit: x\n", ""),
    )
    .expect("write");
    assert!(
        !record_problems(&module, &allowed).is_empty(),
        "a missing key is refused"
    );

    std::fs::remove_file(module.join("UPSTREAM.md")).expect("rm");
    assert_eq!(
        record_problems(&module, &allowed),
        vec!["gpl has no UPSTREAM.md"]
    );
}

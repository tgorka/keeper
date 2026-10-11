//! An OKF drive's own rules, as the drive's `.okf/bin/` tools apply them
//! (AD-396, AD-403): its YAML, its `config.yaml`, which paths a bundle holds
//! and which it excludes, a document's title and description, the listings
//! `okf index` writes, and where a link points.
//!
//! Written from the drive's `OKF-0.2-digest.md` and its config's comments;
//! the drive's scripts carry no licence, so none of their code was read.
//! Every rule the documentation leaves open is the answer the scripts gave
//! when run over the fixtures in `tests/fixtures/okf/` (`UPSTREAM.md`).

pub mod config;
pub mod doc;
pub mod index;
pub mod links;
pub mod matcher;
pub mod yaml;

pub use config::{load_config, Bundle, Config, ConfigError, Listed};
pub use doc::Doc;
pub use links::{resolve, Link};
pub use matcher::{bundle_for, is_excluded};

#[cfg(test)]
mod tests {
    use serde_json::{json, Value as Json};

    use super::yaml::Value;
    use super::*;

    fn to_json(value: &Value) -> Json {
        match value {
            Value::Null => Json::Null,
            Value::Bool(b) => json!(b),
            Value::Int(n) => json!(n),
            Value::Float(f) => json!(f),
            Value::Str(s) => json!(s),
            Value::List(items) => Json::Array(items.iter().map(to_json).collect()),
            Value::Map(entries) => Json::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), to_json(v)))
                    .collect(),
            ),
        }
    }

    fn config_json(config: &Config) -> Json {
        json!({
            "okf_version": config.okf_version,
            "bundles": config.bundles.iter().map(|b| json!({
                "path": b.path, "name": b.name, "title": b.title,
                "description": b.description, "entry": b.entry,
                "index": b.index, "log": b.log,
            })).collect::<Vec<_>>(),
            "exclude": config.exclude,
            "guides": config.guides,
            "no_index": config.no_index,
            "listed": config.listed.iter().map(|l| json!({
                "path": l.path, "title": l.title,
                "description": l.description, "note": l.note,
            })).collect::<Vec<_>>(),
        })
    }

    fn text<'r>(row: &'r Json, key: &str) -> &'r str {
        row[key].as_str().expect(key)
    }

    fn flag(row: &Json, key: &str) -> bool {
        row[key].as_bool().expect(key)
    }

    fn rows(fixture: &str) -> Vec<Json> {
        fixture
            .lines()
            .map(|line| serde_json::from_str(line).expect("a fixture line"))
            .collect()
    }

    fn config() -> Config {
        load_config(include_str!("../../tests/fixtures/okf/config.yaml"))
            .expect("the fixture config")
    }

    /// `load_config` over the fixture config is what the drive's own
    /// `load_config` answered for it: every key, the defaults of a bundle's
    /// `index` and `log`, a single-quoted `''`, a trailing comment and a
    /// folded `note: >-` — the last two where the drive's fallback parser
    /// and PyYAML disagree, PyYAML's being the drive's answer.
    #[test]
    fn okf_config_reads_as_the_drives_load_config() {
        let expected: Json =
            serde_json::from_str(include_str!("../../tests/fixtures/okf/config.json"))
                .expect("config.json");
        assert_eq!(config_json(&config()), expected);
    }

    /// Every (pattern, path) of the probe table is what the drive's `_match`
    /// answered: the literal prefix of `/` and `/**`, exact paths, `*`
    /// across `/` in a pattern with a `/`, the file name for one without,
    /// brackets as text.
    #[test]
    fn okf_patterns_match_as_the_drives_match() {
        let mut checked = 0;
        for row in rows(include_str!("../../tests/fixtures/okf/match.jsonl")) {
            let (pattern, path) = (text(&row, "pattern"), text(&row, "path"));
            assert_eq!(
                matcher::matches(pattern, path),
                flag(&row, "match"),
                "{pattern:?} on {path:?}"
            );
            checked += 1;
        }
        assert!(checked > 4000, "{checked}");
    }

    /// `is_excluded` and `bundle_for` over the fixture config give the
    /// drive's answers for every probe path: guides win over exclusions,
    /// the innermost bundle holds a path, a bundle's own folder is its
    /// parent's.
    #[test]
    fn okf_paths_are_placed_as_the_drives_tools_place_them() {
        let config = config();
        for row in rows(include_str!("../../tests/fixtures/okf/paths.jsonl")) {
            let path = text(&row, "path");
            assert_eq!(
                is_excluded(&config, path),
                flag(&row, "excluded"),
                "excluded {path:?}"
            );
            assert_eq!(
                bundle_for(&config, path).map(|b| b.name.as_str()),
                row["bundle"].as_str(),
                "bundle of {path:?}"
            );
        }
    }

    /// Acceptance 1 (95.4, R137 b, R95S-12): over the drive's own
    /// `config.yaml` — read from `$KEEPER_TGDRIVE`, since the owner has not
    /// said whether it may be committed (OA-95-2) — `load_config` is the
    /// drive's own `load_config`, every key of it, as `generate.py
    /// --drive-config` runs the drive's tool now (nothing of it kept here);
    /// and every path of the 40-path table is placed as the drive's tools
    /// placed it: `00-inbox/x.md` excluded while `00-inbox/README.md` is a
    /// guide, client folders, conflict copies at any depth, vault
    /// `.keeper/`, library content, the sessions template in its bundle,
    /// `80-agents/` in the root bundle, and a real session's workspace NOT
    /// excluded (Q9).
    #[test]
    #[ignore = "reads the drive's own config from $KEEPER_TGDRIVE and runs its tools (OA-95-2)"]
    fn okf_matching_matches_the_drives_own_tools() {
        let drive = std::env::var("KEEPER_TGDRIVE").expect("KEEPER_TGDRIVE names the drive");
        let source = std::fs::read_to_string(format!("{drive}/.okf/config.yaml"))
            .expect("the drive's config");
        let config = load_config(&source).expect("the drive's config reads");
        let ran = std::process::Command::new("python3")
            .arg("-B")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/okf/generate.py"
            ))
            .arg("--drive-config")
            .env("KEEPER_TGDRIVE", &drive)
            .output()
            .expect("python3 runs the drive's load_config");
        assert!(
            ran.status.success(),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        let expected: Json = serde_json::from_slice(&ran.stdout).expect("its answer is JSON");
        assert_eq!(config_json(&config), expected);
        let table = rows(include_str!("../../tests/fixtures/okf/drive-paths.jsonl"));
        assert!(table.len() >= 40);
        for row in table {
            let path = text(&row, "path");
            assert_eq!(
                is_excluded(&config, path),
                flag(&row, "excluded"),
                "excluded {path:?}"
            );
            assert_eq!(
                bundle_for(&config, path).map(|b| b.name.as_str()),
                row["bundle"].as_str(),
                "bundle of {path:?}"
            );
        }
    }

    /// A document's frontmatter, title and description are what the
    /// drive's `read_doc` read from the same bytes.
    #[test]
    fn okf_documents_read_as_the_drives_read_doc() {
        for row in rows(include_str!("../../tests/fixtures/okf/docs.jsonl")) {
            let name = text(&row, "name");
            let doc = Doc::read(&format!("docs/{name}"), text(&row, "text"));
            assert_eq!(to_json(&doc.meta), row["meta"], "meta of {name}");
            assert_eq!(doc.title(), text(&row, "title"), "title of {name}");
            assert_eq!(
                doc.description(),
                text(&row, "description"),
                "description of {name}"
            );
            assert_eq!(doc.reserved(), flag(&row, "reserved"), "{name}");
            assert_eq!(doc.error.is_some(), flag(&row, "error"), "{name}");
        }
    }

    /// Every link of the probe table resolves as the drive's
    /// `okf_links.resolve` resolved it.
    #[test]
    fn okf_links_resolve_as_the_drives_resolve() {
        for row in rows(include_str!("../../tests/fixtures/okf/links.jsonl")) {
            let (base, target) = (text(&row, "base"), text(&row, "target"));
            let expected = match (row["kind"].as_str(), row["value"].as_str()) {
                (None, None) => Link::Fragment,
                (Some("external"), Some(value)) => Link::External(value.to_owned()),
                (Some("path"), Some(value)) => Link::Path(value.to_owned()),
                other => panic!("an unknown answer {other:?}"),
            };
            assert_eq!(resolve(base, target), expected, "{base:?} + {target:?}");
        }
    }

    /// Every listing `okf index` wrote for the fixture drive reads back as
    /// the documents it was written from — each `## Documents` line's link
    /// resolves to the document, with its title and description — and a
    /// bundle root's frontmatter names its bundle.
    #[test]
    fn okf_listings_read_back_what_the_drives_index_wrote() {
        let config = config();
        let mut documents_read = 0;
        for row in rows(include_str!("../../tests/fixtures/okf/index.jsonl")) {
            let dir = text(&row, "dir");
            let listing = index::parse(text(&row, "text"));
            let bundle = config
                .bundles
                .iter()
                .find(|b| b.path == if dir.is_empty() { "." } else { dir });
            assert_eq!(
                listing.bundle.as_ref().map(|b| b.name.as_str()),
                bundle.map(|b| b.name.as_str()),
                "{dir:?}"
            );
            if listing.section("Documents").next().is_none() {
                continue;
            }
            let read: Vec<Json> = listing
                .section("Documents")
                .map(|entry| {
                    let Link::Path(path) = resolve(dir, &entry.link) else {
                        panic!("{entry:?} is not a path");
                    };
                    json!({"path": path, "title": entry.title, "description": entry.description})
                })
                .collect();
            documents_read += read.len();
            assert_eq!(Json::Array(read), row["documents"], "{dir:?}");
        }
        assert!(documents_read >= 8, "{documents_read}");
    }
}

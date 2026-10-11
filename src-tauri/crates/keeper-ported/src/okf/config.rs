//! A drive's `.okf/config.yaml`: which folders are OKF bundles, what is
//! excluded from every bundle, the guides that stay in the catalogue though
//! their zone is excluded, the staging zones that still get a listing, and
//! the folders that must not carry an `index.md`.
//!
//! The keys and their meaning are the config's own comments and the OKF
//! digest's; the defaults for a key left out are what the drive's
//! `load_config` answered (`tests/fixtures/okf/`).

use super::yaml::{self, Value};

/// One bundle: a folder whose Markdown is one OKF bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Drive-relative; `.` is the drive's root.
    pub path: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub entry: String,
    /// Whether the bundle keeps a generated `index.md`.
    pub index: bool,
    /// Whether it keeps a `log.md`.
    pub log: bool,
}

impl Bundle {
    /// Whether this is the drive's root bundle.
    pub fn is_root(&self) -> bool {
        self.path == "."
    }
}

/// A staging zone that is listed though it is no bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub path: String,
    pub title: String,
    pub description: String,
    pub note: String,
}

/// A drive's OKF configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub okf_version: String,
    /// In the config's order.
    pub bundles: Vec<Bundle>,
    pub exclude: Vec<String>,
    pub guides: Vec<String>,
    pub no_index: Vec<String>,
    pub listed: Vec<Listed>,
}

/// Why a config is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Yaml(yaml::YamlError),
    /// The document is not a mapping of the config's keys.
    NotAMapping,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Yaml(error) => write!(f, "the OKF config is not YAML this reads: {error}"),
            ConfigError::NotAMapping => write!(f, "the OKF config is not a mapping of its keys"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// A key's text: a string as it is, any other scalar as Python's `str()`
/// writes it, `default` when absent, null or a collection.
fn text(map: &Value, key: &str, default: &str) -> String {
    match map.get(key) {
        Some(Value::Null) | None => default.to_owned(),
        Some(value) => value.scalar_text().unwrap_or_else(|| default.to_owned()),
    }
}

/// A list of strings; anything else in it, and a missing list, is nothing.
fn strings(map: &Value, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(Value::as_list)
        .map(|items| items.iter().filter_map(Value::scalar_text).collect())
        .unwrap_or_default()
}

/// The mappings of a list.
fn maps<'a>(map: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    map.get(key)
        .and_then(Value::as_list)
        .unwrap_or_default()
        .iter()
        .filter(|item| matches!(item, Value::Map(_)))
}

/// Read a config's text.
pub fn load_config(src: &str) -> Result<Config, ConfigError> {
    let root = yaml::parse(src).map_err(ConfigError::Yaml)?;
    if !matches!(root, Value::Map(_)) {
        return Err(ConfigError::NotAMapping);
    }
    let flag = |map: &Value, key: &str| map.get(key).is_none_or(Value::truthy);
    Ok(Config {
        okf_version: text(&root, "okf_version", "0.2"),
        bundles: maps(&root, "bundles")
            .map(|bundle| Bundle {
                path: text(bundle, "path", "."),
                name: text(bundle, "name", ""),
                title: text(bundle, "title", ""),
                description: text(bundle, "description", ""),
                entry: text(bundle, "entry", ""),
                index: flag(bundle, "index"),
                log: flag(bundle, "log"),
            })
            .collect(),
        exclude: strings(&root, "exclude"),
        guides: strings(&root, "guides"),
        no_index: strings(&root, "no_index"),
        listed: maps(&root, "listed")
            .map(|listed| {
                let path = text(listed, "path", "");
                Listed {
                    title: text(listed, "title", &path),
                    description: text(listed, "description", ""),
                    note: text(listed, "note", ""),
                    path,
                }
            })
            .collect(),
    })
}

//! `_drive.toml`'s rules that hold without Matrix: grammar version, closed key
//! sets, `local_only`, and `[integrity]`. Owner and readers stay in
//! `keeper-core` (`agents::drive::parse`), which calls these functions.
//!
//! A glob of the form `<folder>/**` matches what lies inside the folder, never
//! the folder itself: `70-comms/**` is read as the prefix `70-comms/`, as
//! `keeper-ported::okf::matcher` reads it, so the zone's own folder is an
//! ordinary area (`comms`) while every entry under it is outside content
//! (story 117.1, Ambiguity 9).

/// The grammar this build reads.
pub const GRAMMAR_VERSION: i64 = 1;

/// Drive-relative globs whose files are `untrusted` whoever wrote them, when
/// the file has no `[integrity]` table: what arrives from outside the drive's
/// readers (an inbox, messages, recordings of other people).
pub const DEFAULT_UNTRUSTED: [&str; 3] = ["00-inbox/**", "70-comms/**", "recordings/**"];

/// The closed set of top-level keys in `_drive.toml`.
pub const ROOT_KEYS: [&str; 8] = [
    "version",
    "id",
    "title",
    "principal",
    "owner",
    "readers",
    "local_only",
    "integrity",
];

/// The closed set of keys in `_drive.toml`'s `[integrity]` table.
pub const INTEGRITY_KEYS: [&str; 1] = ["untrusted"];

/// Why a `_drive.toml` cannot be read. Each is the sentence the zone shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriveFileRefusal {
    #[error("_drive.toml is not valid TOML: {0}")]
    Syntax(String),
    #[error("_drive.toml has `{key}`, which is not one of its keys.")]
    UnknownKey { key: String },
    #[error("_drive.toml's [integrity] has `{key}`, which is not one of its keys.")]
    UnknownIntegrityKey { key: String },
    #[error("_drive.toml needs `{key}`.")]
    Missing { key: &'static str },
    #[error("`{key}` in _drive.toml must be {expected}, not {found}.")]
    WrongType {
        key: &'static str,
        expected: &'static str,
        found: &'static str,
    },
    #[error(
        "This _drive.toml was written by a newer keeper (version {version}); this keeper reads version {GRAMMAR_VERSION}."
    )]
    NewerVersion { version: i64 },
    #[error("`version` in _drive.toml is {version}; the first version is 1.")]
    BadVersion { version: i64 },
    #[error("`untrusted` holds \"{pattern}\", which is not a glob pattern: {reason}")]
    BadGlob { pattern: String, reason: String },
}

/// The file's two rules, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRules {
    pub local_only: bool,
    pub untrusted: Vec<String>,
}

impl DriveRules {
    /// The drive's untrusted globs: configured patterns union [`DEFAULT_UNTRUSTED`].
    pub fn untrusted_globs(&self) -> Vec<String> {
        untrusted_globs(&self.untrusted)
    }
}

/// Parse `_drive.toml` in key order and refuse any unknown top-level key.
pub fn table(text: &str) -> Result<toml::Table, DriveFileRefusal> {
    let table: toml::Table = key_order::from_str(text)
        .map_err(|error| DriveFileRefusal::Syntax(first_line(error.message())))?;
    if let Some(key) = table.keys().find(|key| !ROOT_KEYS.contains(&key.as_str())) {
        return Err(DriveFileRefusal::UnknownKey { key: key.clone() });
    }
    Ok(table)
}

/// Read and validate `version`.
pub fn version(table: &toml::Table) -> Result<i64, DriveFileRefusal> {
    match required(table, "version")? {
        toml::Value::Integer(version) if *version > GRAMMAR_VERSION => {
            Err(DriveFileRefusal::NewerVersion { version: *version })
        }
        toml::Value::Integer(version) if *version < 1 => {
            Err(DriveFileRefusal::BadVersion { version: *version })
        }
        toml::Value::Integer(version) => Ok(*version),
        other => Err(wrong("version", "a whole number", other)),
    }
}

/// Read `local_only`, defaulting to `false` when absent.
pub fn local_only(table: &toml::Table) -> Result<bool, DriveFileRefusal> {
    match table.get("local_only") {
        None => Ok(false),
        Some(toml::Value::Boolean(on)) => Ok(*on),
        Some(other) => Err(wrong("local_only", "true or false", other)),
    }
}

/// Read `[integrity].untrusted`, defaulting to [`DEFAULT_UNTRUSTED`] when
/// `[integrity]` is absent and to an empty list when `[integrity]` is present
/// without `untrusted`.
pub fn untrusted(table: &toml::Table) -> Result<Vec<String>, DriveFileRefusal> {
    let integrity = match table.get("integrity") {
        None => {
            return Ok(DEFAULT_UNTRUSTED
                .iter()
                .map(|glob| (*glob).to_owned())
                .collect());
        }
        Some(toml::Value::Table(integrity)) => integrity,
        Some(other) => return Err(wrong("integrity", "a table, [integrity]", other)),
    };
    if let Some(key) = integrity
        .keys()
        .find(|key| !INTEGRITY_KEYS.contains(&key.as_str()))
    {
        return Err(DriveFileRefusal::UnknownIntegrityKey { key: key.clone() });
    }
    let items = match integrity.get("untrusted") {
        None => return Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items,
        Some(other) => return Err(wrong("untrusted", "a list of glob patterns", other)),
    };
    items
        .iter()
        .map(|item| {
            let toml::Value::String(pattern) = item else {
                return Err(wrong("untrusted", "a list of glob patterns", item));
            };
            globset::Glob::new(pattern).map_err(|error| DriveFileRefusal::BadGlob {
                pattern: pattern.clone(),
                reason: error.kind().to_string(),
            })?;
            Ok(pattern.clone())
        })
        .collect()
}

/// Look up a required key in a `_drive.toml` table.
pub fn required<'t>(
    table: &'t toml::Table,
    key: &'static str,
) -> Result<&'t toml::Value, DriveFileRefusal> {
    table.get(key).ok_or(DriveFileRefusal::Missing { key })
}

/// Build a [`DriveFileRefusal::WrongType`] naming `found`'s TOML type.
pub fn wrong(key: &'static str, expected: &'static str, found: &toml::Value) -> DriveFileRefusal {
    DriveFileRefusal::WrongType {
        key,
        expected,
        found: describe(found),
    }
}

/// Read `_drive.toml`'s Matrix-free rules: grammar, `local_only`, and `[integrity]`.
pub fn read(text: &str) -> Result<DriveRules, DriveFileRefusal> {
    let table = table(text)?;
    version(&table)?;
    let local_only = local_only(&table)?;
    let untrusted = untrusted(&table)?;
    Ok(DriveRules {
        local_only,
        untrusted,
    })
}

/// The union of the drive's configured list and [`DEFAULT_UNTRUSTED`],
/// configured first in its order, then each default not already in it. Used as
/// the trust rule by the import; `untrusted()` alone is never the trust rule,
/// because a present `[integrity]` with no `untrusted` key or an empty list
/// reads as no globs, while the agents' runtime treats the defaults as outside
/// content.
pub fn untrusted_globs(configured: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(configured.len() + DEFAULT_UNTRUSTED.len());
    for glob in configured {
        if !out.iter().any(|seen| seen == glob) {
            out.push(glob.clone());
        }
    }
    for default in DEFAULT_UNTRUSTED {
        if !out.iter().any(|seen| seen == default) {
            out.push(default.to_owned());
        }
    }
    out
}

fn describe(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "text",
        toml::Value::Integer(_) => "a whole number",
        toml::Value::Float(_) => "a decimal number",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a date",
        toml::Value::Array(_) => "a list",
        toml::Value::Table(_) => "a table",
    }
}

fn first_line(message: &str) -> String {
    message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(message)
        .trim()
        .to_owned()
}

/// The same key-order reading as keeper-core's `toml_order` (R170), so the
/// first refusal is the same.
mod key_order {
    use serde::Deserialize;
    use toml::de::{DeTable, DeValue, Deserializer};

    pub fn from_str<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, toml::de::Error> {
        let mut root = DeTable::parse(text)?;
        sort_table(root.get_mut());
        T::deserialize(Deserializer::from(root)).map_err(|mut error| {
            error.set_input(Some(text));
            error
        })
    }

    fn sort_table(table: &mut DeTable<'_>) {
        let mut entries: Vec<_> = std::mem::take(table).into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, value) in &mut entries {
            sort_value(value.get_mut());
        }
        *table = entries.into_iter().collect();
    }

    fn sort_value(value: &mut DeValue<'_>) {
        match value {
            DeValue::Table(table) => sort_table(table),
            DeValue::Array(items) => {
                for item in items.iter_mut() {
                    sort_value(item.get_mut());
                }
            }
            _ => {}
        }
    }
}

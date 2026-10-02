// Ported from BMAD-METHOD `src/scripts/config_utils.py` at tag v6.12.0
// (05bfbd46d00766ec88eb9b42e76be2c575d64d7b). MIT, Copyright (c) 2025 BMad
// Code, LLC; see `UPSTREAM.md`. Python dicts and lists became `toml::Value`,
// and errors became `ConfigError` carrying upstream's messages.

//! Strict TOML layers and BMAD's structural merge.
//!
//! Tables merge recursively and the override's scalar wins. Arrays append,
//! except an array whose every item is a table carrying a `code` (else an `id`)
//! string: there an override item replaces the base item with the same
//! identifier in place, and a new identifier appends.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use toml::{Table, Value};

/// A present configuration layer that cannot be used safely. `Display` is
/// upstream's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    message: String,
}

impl ConfigError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ConfigError {}

/// The identifiers a table array may be merged by, in the order tried.
const KEYED_MERGE_FIELDS: [&str; 2] = ["code", "id"];

/// Load a TOML table, allowing absence only for an optional layer.
pub fn load_toml(path: &Path, required: bool) -> Result<Table, ConfigError> {
    if !path.exists() {
        if required {
            return Err(ConfigError::new(format!(
                "required TOML file not found: {}",
                path.display()
            )));
        }
        return Ok(Table::new());
    }
    if !path.is_file() {
        return Err(ConfigError::new(format!(
            "TOML layer is not a file: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| ConfigError::new(format!("failed to read {}: {error}", path.display())))?;
    let parse_failed = |error: &dyn fmt::Display| {
        ConfigError::new(format!("failed to parse {}: {error}", path.display()))
    };
    let text = std::str::from_utf8(&bytes).map_err(|error| parse_failed(&error))?;
    toml::from_str::<Table>(text).map_err(|error| parse_failed(&error))
}

/// Upstream's `type(value).__name__` for the value tomllib would have built.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "str",
        Value::Integer(_) => "int",
        Value::Float(_) => "float",
        Value::Boolean(_) => "bool",
        Value::Datetime(datetime) => match (datetime.date, datetime.time) {
            (Some(_), None) => "date",
            (None, Some(_)) => "time",
            _ => "datetime",
        },
        Value::Array(_) => "list",
        Value::Table(_) => "dict",
    }
}

fn detect_keyed_merge_field<'a, I>(items: I) -> Result<Option<&'static str>, ConfigError>
where
    I: Iterator<Item = &'a Value> + Clone,
{
    let mut tables = items.clone().peekable();
    if tables.peek().is_none() || !items.clone().all(Value::is_table) {
        return Ok(None);
    }
    for candidate in KEYED_MERGE_FIELDS {
        let identifier = |item: &'a Value| item.as_table().and_then(|t| t.get(candidate));
        if !items.clone().all(|item| identifier(item).is_some()) {
            continue;
        }
        for value in items.clone().filter_map(identifier) {
            match value {
                Value::String(s) if s.is_empty() => {
                    return Err(ConfigError::new(format!(
                        "keyed array identifier `{candidate}` must not be empty"
                    )))
                }
                Value::String(_) => {}
                other => {
                    return Err(ConfigError::new(format!(
                        "keyed array identifier `{candidate}` must be a string, got {}",
                        python_type_name(other)
                    )))
                }
            }
        }
        return Ok(Some(candidate));
    }
    Ok(None)
}

fn merge_arrays(base: Vec<Value>, over: Vec<Value>) -> Result<Vec<Value>, ConfigError> {
    let Some(field) = detect_keyed_merge_field(base.iter().chain(over.iter()))? else {
        let mut result = base;
        result.extend(over);
        return Ok(result);
    };
    let key_of = |item: &Value| -> String {
        item.get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let mut result: Vec<Value> = Vec::with_capacity(base.len() + over.len());
    let mut index_by_key: HashMap<String, usize> = HashMap::new();
    for item in base {
        index_by_key.insert(key_of(&item), result.len());
        result.push(item);
    }
    for item in over {
        let key = key_of(&item);
        match index_by_key.get(&key) {
            Some(&at) => result[at] = item,
            None => {
                index_by_key.insert(key, result.len());
                result.push(item);
            }
        }
    }
    Ok(result)
}

/// Merge tables recursively, keyed table arrays by identity, and append other
/// arrays.
pub fn structural_merge(base: Value, over: Value) -> Result<Value, ConfigError> {
    match (base, over) {
        (Value::Table(base), Value::Table(over)) => merge_tables(base, over).map(Value::Table),
        (Value::Array(base), Value::Array(over)) => merge_arrays(base, over).map(Value::Array),
        (_, over) => Ok(over),
    }
}

fn merge_tables(mut result: Table, over: Table) -> Result<Table, ConfigError> {
    // In place, so a key the base already had keeps its position, as a
    // Python dict update keeps it.
    for (key, value) in over {
        match result.get_mut(&key) {
            Some(existing) => {
                let base = std::mem::replace(existing, Value::Boolean(false));
                *existing = structural_merge(base, value)?;
            }
            None => {
                result.insert(key, value);
            }
        }
    }
    Ok(result)
}

/// Fold layers left to right through [`structural_merge`].
pub fn merge_layers(layers: impl IntoIterator<Item = Table>) -> Result<Table, ConfigError> {
    let mut merged = Table::new();
    for layer in layers {
        merged = merge_tables(merged, layer)?;
    }
    Ok(merged)
}

/// A skill's three customization layers merged in order: the skill's own
/// `customize.toml` (required), then `_bmad/custom/<skill>.toml` and
/// `_bmad/custom/<skill>.user.toml` under the project root (optional).
pub fn load_customization(
    project_root: Option<&Path>,
    skill_dir: &Path,
) -> Result<Table, ConfigError> {
    let skill_name = skill_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let defaults = load_toml(&skill_dir.join("customize.toml"), true)?;
    let (team, user) = match project_root {
        Some(root) => {
            let custom = root.join("_bmad").join("custom");
            (
                load_toml(&custom.join(format!("{skill_name}.toml")), false)?,
                load_toml(&custom.join(format!("{skill_name}.user.toml")), false)?,
            )
        }
        None => (Table::new(), Table::new()),
    };
    merge_layers([defaults, team, user])
}

#[cfg(test)]
mod tests {
    //! Upstream's own cases (`src/scripts/tests/test_config_utils.py` at
    //! v6.12.0), then the keyed-merge order and the Winston golden.
    use super::*;

    fn table(text: &str) -> Table {
        toml::from_str(text).expect("test TOML parses")
    }

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bmad")
    }

    #[test]
    fn structural_merge_recurses_appends_and_replaces_keyed_tables() {
        let base = table(
            r#"
            nested = { keep = true, replace = "old" }
            plain = ["base"]
            items = [{ id = "one", value = "old" }]
            "#,
        );
        let over = table(
            r#"
            nested = { replace = "new" }
            plain = ["override"]
            items = [{ id = "one", value = "new" }, { id = "two", value = "added" }]
            "#,
        );
        let merged = structural_merge(Value::Table(base), Value::Table(over)).expect("merges");
        let expected = table(
            r#"
            nested = { keep = true, replace = "new" }
            plain = ["base", "override"]
            items = [{ id = "one", value = "new" }, { id = "two", value = "added" }]
            "#,
        );
        assert_eq!(merged, Value::Table(expected));
    }

    #[test]
    fn non_string_keyed_identifier_is_rejected() {
        let base = Value::Array(vec![Value::Table(table(r#"id = "valid""#))]);
        let over = Value::Array(vec![Value::Table(table("id = 42"))]);
        let error = structural_merge(base, over).expect_err("refused");
        assert_eq!(
            error.to_string(),
            "keyed array identifier `id` must be a string, got int"
        );
    }

    #[test]
    fn present_malformed_optional_layer_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("optional.toml");
        std::fs::write(&path, "[broken\n").expect("write");
        let error = load_toml(&path, false).expect_err("refused");
        assert!(error.to_string().starts_with("failed to parse "), "{error}");
        assert!(error.to_string().contains("optional.toml"), "{error}");
    }

    #[test]
    fn missing_optional_layer_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("optional.toml");
        assert_eq!(load_toml(&path, false), Ok(Table::new()));
        let error = load_toml(&path, true).expect_err("a required layer must exist");
        assert!(
            error
                .to_string()
                .starts_with("required TOML file not found: "),
            "{error}"
        );
    }

    #[test]
    fn customization_layer_precedence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let custom = root.join("_bmad/custom");
        let skill = root.join("_bmad/bmm/sample-skill");
        std::fs::create_dir_all(&custom).expect("mkdir");
        std::fs::create_dir_all(&skill).expect("mkdir");
        let order = |layers: &Table| {
            layers["value"]["order"]
                .as_str()
                .map(str::to_owned)
                .expect("order is a string")
        };

        std::fs::write(
            skill.join("customize.toml"),
            "[value]\norder = \"default\"\n",
        )
        .expect("write");
        assert_eq!(
            order(&load_customization(Some(root), &skill).expect("loads")),
            "default"
        );

        std::fs::write(
            custom.join("sample-skill.toml"),
            "[value]\norder = \"team\"\n",
        )
        .expect("write");
        assert_eq!(
            order(&load_customization(Some(root), &skill).expect("loads")),
            "team"
        );

        std::fs::write(
            custom.join("sample-skill.user.toml"),
            "[value]\norder = \"user\"\n",
        )
        .expect("write");
        assert_eq!(
            order(&load_customization(Some(root), &skill).expect("loads")),
            "user"
        );

        assert_eq!(
            order(&load_customization(None, &skill).expect("loads")),
            "default"
        );
    }

    #[test]
    fn keyed_merge_prefers_code_then_id_and_appends_when_any_item_lacks_it() {
        let both = structural_merge(
            Value::Array(vec![Value::Table(table(
                r#"code = "A"
id = "x"
v = 1"#,
            ))]),
            Value::Array(vec![Value::Table(table(
                r#"code = "A"
id = "y"
v = 2"#,
            ))]),
        )
        .expect("merges");
        assert_eq!(
            both,
            Value::Array(vec![Value::Table(table(
                r#"code = "A"
id = "y"
v = 2"#
            ))]),
            "both keys present: merged by code, so differing ids do not split it"
        );

        let partial = structural_merge(
            Value::Array(vec![Value::Table(table(r#"code = "A""#))]),
            Value::Array(vec![Value::Table(table(r#"name = "A""#))]),
        )
        .expect("merges");
        assert_eq!(
            partial.as_array().map(Vec::len),
            Some(2),
            "an item without the key appends"
        );

        let mixed = structural_merge(
            Value::Array(vec![Value::Table(table(r#"code = "A""#))]),
            Value::Array(vec![Value::String("plain".to_owned())]),
        )
        .expect("merges");
        assert_eq!(
            mixed.as_array().map(Vec::len),
            Some(2),
            "a non-table item appends"
        );

        let error = structural_merge(
            Value::Array(vec![Value::Table(table(r#"code = "A""#))]),
            Value::Array(vec![Value::Table(table(r#"code = """#))]),
        )
        .expect_err("refused");
        assert_eq!(
            error.to_string(),
            "keyed array identifier `code` must not be empty"
        );
    }

    #[test]
    fn merges_winston_like_resolve_customization() {
        let root = fixtures();
        let merged = load_customization(Some(&root), &root.join("bmad-agent-architect"))
            .expect("Winston merges");
        let golden: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("winston-resolved.json")).expect("golden"),
        )
        .expect("golden is JSON");
        let ours = serde_json::to_value(&merged["agent"]).expect("toml to json");
        assert_eq!(ours, golden["agent"]);
    }
}

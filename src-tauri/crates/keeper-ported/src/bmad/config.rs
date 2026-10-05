// Ported from BMAD-METHOD `src/scripts/config_utils.py`, `resolve_config.py`
// and `resolve_customization.py` at tag v6.12.0
// (05bfbd46d00766ec88eb9b42e76be2c575d64d7b). MIT, Copyright (c) 2025 BMad
// Code, LLC; see `UPSTREAM.md`. Python dicts and lists became `toml::Value`,
// and errors became `ConfigError` carrying upstream's messages.

//! Strict TOML layers and BMAD's structural merge.
//!
//! Tables merge recursively and the override's scalar wins. Arrays append,
//! except an array whose every item is a table carrying a `code` (else an `id`)
//! string: there an override item replaces the base item with the same
//! identifier in place, and a new identifier appends.
//!
//! A table keeps its keys in document order (`toml`'s `preserve_order`), as a
//! Python dict does: a key the base already had keeps its place, a new key
//! goes last, and [`to_json`] prints them in that order.
//!
//! A layer arrives as a [`Layer`]: the name its sentences print and what the
//! caller found there. [`Layer::read`] reads one from the filesystem as
//! upstream does; a caller that must read through its own containment (a
//! drive, where `_bmad/custom/` is read only if the drive holds it) builds the
//! [`Source`] itself and names the layer by its drive-relative path.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use toml::{Table, Value};

use super::py;

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

/// What a reader found at one layer's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Nothing there: an empty table for an optional layer.
    Absent,
    /// Something other than a file, such as a directory.
    NotAFile,
    /// A file that could not be read; the detail is the reader's.
    Unreadable(String),
    Bytes(Vec<u8>),
}

/// One configuration layer, named by the path its sentences print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub source: Source,
}

impl Layer {
    /// Read `path` as upstream does, following symlinks, and name it by its
    /// path.
    pub fn read(path: &Path) -> Self {
        let source = if !path.exists() {
            Source::Absent
        } else if !path.is_file() {
            Source::NotAFile
        } else {
            match std::fs::read(path) {
                Ok(bytes) => Source::Bytes(bytes),
                Err(error) => Source::Unreadable(error.to_string()),
            }
        };
        Self {
            name: path.display().to_string(),
            source,
        }
    }

    /// Parse the layer, allowing absence only when it is optional.
    pub fn parse(self, required: bool) -> Result<Table, ConfigError> {
        let name = self.name;
        let bytes = match self.source {
            Source::Absent if required => {
                return Err(ConfigError::new(format!(
                    "required TOML file not found: {name}"
                )))
            }
            Source::Absent => return Ok(Table::new()),
            Source::NotAFile => {
                return Err(ConfigError::new(format!(
                    "TOML layer is not a file: {name}"
                )))
            }
            Source::Unreadable(error) => {
                return Err(ConfigError::new(format!("failed to read {name}: {error}")))
            }
            Source::Bytes(bytes) => bytes,
        };
        let parse_failed =
            |error: &dyn fmt::Display| ConfigError::new(format!("failed to parse {name}: {error}"));
        let text = std::str::from_utf8(&bytes).map_err(|error| parse_failed(&error))?;
        toml::from_str::<Table>(text).map_err(|error| parse_failed(&error))
    }
}

/// Load a TOML table, allowing absence only for an optional layer.
pub fn load_toml(path: &Path, required: bool) -> Result<Table, ConfigError> {
    Layer::read(path).parse(required)
}

/// The four central layers, relative to the project root, in merge order;
/// only the first is required.
pub const CENTRAL_LAYERS: [&str; 4] = [
    "_bmad/config.toml",
    "_bmad/config.user.toml",
    "_bmad/custom/config.toml",
    "_bmad/custom/config.user.toml",
];

/// BMAD's central configuration: [`CENTRAL_LAYERS`] merged in order. `read`
/// is given each project-relative path and returns that layer; every layer is
/// parsed before any is merged, so the first broken one is the one named.
pub fn load_central_config(mut read: impl FnMut(&str) -> Layer) -> Result<Table, ConfigError> {
    let mut layers = Vec::with_capacity(CENTRAL_LAYERS.len());
    for (at, path) in CENTRAL_LAYERS.into_iter().enumerate() {
        layers.push(read(path).parse(at == 0)?);
    }
    merge_layers(layers)
}

/// The value at a dotted path, each part a key of a table (`resolve_config.py`'s
/// `extract_key`).
pub fn extract_key<'a>(data: &'a Table, dotted_key: &str) -> Option<&'a Value> {
    let mut parts = dotted_key.split('.');
    let first = parts.next()?;
    let mut current = data.get(first)?;
    for part in parts {
        current = current.as_table()?.get(part)?;
    }
    Some(current)
}

/// What the resolvers print for `--key`: each found key under its dotted
/// name, in the order asked; a key that is not there is left out.
pub fn extract_keys<'k>(data: &Table, keys: impl IntoIterator<Item = &'k str>) -> Table {
    let mut found = Table::new();
    for key in keys {
        if let Some(value) = extract_key(data, key) {
            found.insert(key.to_owned(), value.clone());
        }
    }
    found
}

/// The resolvers' output, as their `json.dumps(output, indent=2,
/// ensure_ascii=False)` writes it, then a newline: keys in table order,
/// non-ASCII as itself, a float as Python's `repr` (`NaN`, `Infinity` and
/// `-Infinity` included). A TOML date or time is refused with the `TypeError`
/// the script dies of.
pub fn to_json(table: &Table) -> Result<String, ConfigError> {
    let mut out = String::new();
    write_table(&mut out, table, 0)?;
    out.push('\n');
    Ok(out)
}

fn indent(out: &mut String, depth: usize) {
    out.push('\n');
    out.push_str(&"  ".repeat(depth));
}

fn write_table(out: &mut String, table: &Table, depth: usize) -> Result<(), ConfigError> {
    if table.is_empty() {
        out.push_str("{}");
        return Ok(());
    }
    out.push('{');
    for (at, (key, item)) in table.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        indent(out, depth + 1);
        out.push_str(&py::json_string(key, false));
        out.push_str(": ");
        write_json(out, item, depth + 1)?;
    }
    indent(out, depth);
    out.push('}');
    Ok(())
}

fn write_json(out: &mut String, value: &Value, depth: usize) -> Result<(), ConfigError> {
    match value {
        Value::String(text) => out.push_str(&py::json_string(text, false)),
        Value::Integer(number) => out.push_str(&number.to_string()),
        Value::Float(number) => out.push_str(&py::json_float(*number)),
        Value::Boolean(on) => out.push_str(if *on { "true" } else { "false" }),
        Value::Datetime(_) => {
            return Err(ConfigError::new(format!(
                "Object of type {} is not JSON serializable",
                python_type_name(value)
            )))
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                indent(out, depth + 1);
                write_json(out, item, depth + 1)?;
            }
            indent(out, depth);
            out.push(']');
        }
        Value::Table(table) => write_table(out, table, depth)?,
    }
    Ok(())
}

/// Upstream's `type(value).__name__` for the value tomllib would have built.
pub(crate) fn python_type_name(value: &Value) -> &'static str {
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

/// A skill's two overlay layers, relative to the project root:
/// `_bmad/custom/<skill>.toml`, then `_bmad/custom/<skill>.user.toml`.
pub fn customization_overlays(skill_name: &str) -> [String; 2] {
    [
        format!("_bmad/custom/{skill_name}.toml"),
        format!("_bmad/custom/{skill_name}.user.toml"),
    ]
}

/// A skill's customization: its own `customize.toml` (required), then the two
/// [`customization_overlays`] (optional), each parsed before any is merged.
/// `None` overlays is upstream's "no project root": the defaults alone.
pub fn merge_customization(
    defaults: Layer,
    overlays: Option<[Layer; 2]>,
) -> Result<Table, ConfigError> {
    let defaults = defaults.parse(true)?;
    let (team, user) = match overlays {
        Some([team, user]) => (team.parse(false)?, user.parse(false)?),
        None => (Table::new(), Table::new()),
    };
    merge_layers([defaults, team, user])
}

/// [`merge_customization`] over the filesystem: the skill directory's
/// `customize.toml` and, under `project_root`, the overlays named after the
/// skill directory.
pub fn load_customization(
    project_root: Option<&Path>,
    skill_dir: &Path,
) -> Result<Table, ConfigError> {
    let skill_name = skill_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let overlays = project_root
        .map(|root| customization_overlays(&skill_name).map(|path| Layer::read(&root.join(path))));
    merge_customization(Layer::read(&skill_dir.join("customize.toml")), overlays)
}

#[cfg(test)]
mod tests {
    //! Upstream's own cases (`src/scripts/tests/test_config_utils.py` at
    //! v6.12.0), then the keyed-merge order and the resolvers' goldens; the
    //! CLI cases of `test_resolve_config.py` and `test_resolve_customization.py`
    //! follow in their own modules, as the functions the CLIs call.
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
    fn filesystem_layer_precedence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let custom = root.join("_bmad/custom");
        let skill = root.join("_bmad/bmm/sample-skill");
        std::fs::create_dir_all(&custom).expect("mkdir");
        std::fs::create_dir_all(&skill).expect("mkdir");
        for (path, order) in [
            ("_bmad/config.toml", "base-team"),
            ("_bmad/config.user.toml", "base-user"),
            ("_bmad/custom/config.toml", "custom-team"),
            ("_bmad/custom/config.user.toml", "custom-user"),
        ] {
            std::fs::write(root.join(path), format!("[value]\norder = \"{order}\"\n"))
                .expect("write");
        }
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
        assert_eq!(
            order(&load_central_config(|path| Layer::read(&root.join(path))).expect("loads")),
            "custom-user"
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

    fn expected(name: &str) -> String {
        std::fs::read_to_string(fixtures().join("expected").join(name)).expect("golden")
    }

    fn central() -> Table {
        let root = fixtures();
        load_central_config(|path| Layer::read(&root.join(path))).expect("central")
    }

    /// This repo's two installer layers and makistack's team layer (tgdrive's
    /// `_bmad/custom/config.toml`), merged, byte for byte against
    /// `resolve_config.py`'s dump and its `--key agents`: values, and the key
    /// order a Python dict keeps (R98).
    #[test]
    fn central_config_matches_resolve_config() {
        let merged = central();
        assert_eq!(to_json(&merged).expect("JSON"), expected("central.out"));
        assert_eq!(
            to_json(&extract_keys(&merged, ["agents"])).expect("JSON"),
            expected("central-agents.out")
        );
        assert_eq!(
            merged["modules"]["bmm"]["planning_artifacts"].as_str(),
            Some("{project-root}/_bmad-output/planning-artifacts"),
            "values keep a literal {{project-root}}"
        );
        assert_eq!(
            merged["core"]
                .as_table()
                .map(|core| core.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec![
                "project_name",
                "document_output_language",
                "output_folder",
                "user_name",
                "communication_language"
            ]),
            "a base key keeps its place and a later layer's new key goes last"
        );
    }

    #[test]
    fn keyed_array_identifier_errors() {
        let keyed = |base: &str, over: &str| {
            let layer = |text: &str| table(&format!("items = [{text}]"));
            merge_layers([layer(base), layer(over)]).map_err(|error| error.to_string())
        };
        assert_eq!(
            keyed(r#"{ code = "CA" }"#, r#"{ code = 7 }"#),
            Err("keyed array identifier `code` must be a string, got int".to_owned())
        );
        assert_eq!(
            keyed(r#"{ id = "a" }"#, r#"{ id = "" }"#),
            Err("keyed array identifier `id` must not be empty".to_owned())
        );
        assert_eq!(
            keyed(r#"{ id = "a" }"#, r#"{ id = [1] }"#),
            Err("keyed array identifier `id` must be a string, got list".to_owned())
        );
    }

    /// Winston with a team overlay that replaces menu `CA` by code: what
    /// `resolve_customization.py --key agent` printed, byte for byte.
    #[test]
    fn agent_customization_matches_the_resolver() {
        let root = fixtures();
        let merged = load_customization(Some(&root), &root.join("bmad-agent-architect"))
            .expect("Winston merges");
        assert_eq!(
            to_json(&extract_keys(&merged, ["agent"])).expect("JSON"),
            expected("architect-agent.out")
        );
    }

    /// `bmad-architecture` with makistack's real overlay (tgdrive's
    /// `_bmad/custom/bmad-architecture.toml`), `--key workflow`.
    #[test]
    fn workflow_customization_with_a_real_overlay() {
        let root = fixtures();
        let merged =
            load_customization(Some(&root), &root.join("bmad-architecture")).expect("merges");
        assert_eq!(
            to_json(&extract_keys(&merged, ["workflow"])).expect("JSON"),
            expected("architecture-workflow.out")
        );
    }

    /// keeper reads layers through its own reader: a sentence names the path
    /// the caller gave, an absent overlay is empty, and anything else present
    /// is refused as upstream refuses it.
    #[test]
    fn a_layer_read_by_the_caller_is_named_by_the_callers_path() {
        let layer = |name: &str, source: Source| Layer {
            name: name.to_owned(),
            source,
        };
        let refused = |result: Result<Table, ConfigError>| result.expect_err("refused").to_string();
        assert_eq!(
            refused(load_central_config(|path| layer(path, Source::Absent))),
            "required TOML file not found: _bmad/config.toml"
        );
        assert_eq!(
            refused(layer("_bmad/custom/x.toml", Source::NotAFile).parse(false)),
            "TOML layer is not a file: _bmad/custom/x.toml"
        );
        assert_eq!(
            refused(
                layer(
                    "_bmad/custom/x.toml",
                    Source::Unreadable("denied".to_owned())
                )
                .parse(false)
            ),
            "failed to read _bmad/custom/x.toml: denied"
        );
        let defaults = layer(
            "_workflows/w/customize.toml",
            Source::Bytes(b"[w]\nv = 1\n".to_vec()),
        );
        let absent = [
            layer("_bmad/custom/w.toml", Source::Absent),
            layer("_bmad/custom/w.user.toml", Source::Absent),
        ];
        assert_eq!(
            merge_customization(defaults, Some(absent)).map(|t| t["w"]["v"].as_integer()),
            Ok(Some(1))
        );
        assert_eq!(
            customization_overlays("w"),
            ["_bmad/custom/w.toml", "_bmad/custom/w.user.toml"]
        );
    }

    /// What `json.dumps(tomllib.loads(text), indent=2, ensure_ascii=False)`
    /// printed for the same TOML, and the `TypeError` it raised for each kind
    /// of TOML date and time.
    #[test]
    fn the_dump_writes_every_toml_value_as_the_resolver_does() {
        let merged = table(
            "x = nan\ny = inf\nz = -inf\ne = 1e100\nf = 3.0\ns = 1e-7\n[t]\n\
             k = \"ü\\u007f\\u001f\"\nempty = {}\nlist = []\nnested = [[1, 2.5], {a = true}]\n",
        );
        let python =
            "{\n  \"x\": NaN,\n  \"y\": Infinity,\n  \"z\": -Infinity,\n  \"e\": 1e+100,\n  \
            \"f\": 3.0,\n  \"s\": 1e-07,\n  \"t\": {\n    \"k\": \"ü\u{7f}\\u001f\",\n    \
            \"empty\": {},\n    \"list\": [],\n    \"nested\": [\n      [\n        1,\n        \
            2.5\n      ],\n      {\n        \"a\": true\n      }\n    ]\n  }\n}\n";
        assert_eq!(to_json(&merged), Ok(python.to_owned()));
        for (text, kind) in [
            ("d = 2026-10-05", "date"),
            ("d = 12:30:00", "time"),
            ("d = 2026-10-05T12:30:00", "datetime"),
            ("d = 2026-10-05T12:30:00Z", "datetime"),
        ] {
            assert_eq!(
                to_json(&table(text)).map_err(|error| error.to_string()),
                Err(format!("Object of type {kind} is not JSON serializable"))
            );
        }
    }

    mod resolve_config {
        //! `src/scripts/tests/test_resolve_config.py` at v6.12.0, as the
        //! functions the CLI calls.
        use super::super::*;

        fn root_with(layers: &[(&str, &str)]) -> tempfile::TempDir {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::create_dir_all(dir.path().join("_bmad/custom")).expect("mkdir");
            for (path, text) in layers {
                std::fs::write(dir.path().join(path), text).expect("write");
            }
            dir
        }

        fn load(dir: &tempfile::TempDir) -> Result<Table, ConfigError> {
            load_central_config(|path| Layer::read(&dir.path().join(path)))
        }

        #[test]
        fn full_and_repeated_key_output_follow_layer_precedence() {
            let dir = root_with(&[
                (
                    "_bmad/config.toml",
                    "[core]\nname = \"base\"\nkeep = \"yes\"\n",
                ),
                ("_bmad/config.user.toml", "[core]\nname = \"base-user\"\n"),
                ("_bmad/custom/config.toml", "[core]\nname = \"team\"\n"),
                ("_bmad/custom/config.user.toml", "[core]\nname = \"user\"\n"),
            ]);
            let full = load(&dir).expect("resolves");
            let json: serde_json::Value =
                serde_json::from_str(&to_json(&full).expect("JSON")).expect("parses");
            assert_eq!(
                json["core"],
                serde_json::json!({"name": "user", "keep": "yes"})
            );

            let keyed = extract_keys(&full, ["core.name", "missing"]);
            let json: serde_json::Value =
                serde_json::from_str(&to_json(&keyed).expect("JSON")).expect("parses");
            assert_eq!(json, serde_json::json!({"core.name": "user"}));
        }

        #[test]
        fn malformed_present_layer_fails() {
            let dir = root_with(&[
                ("_bmad/config.toml", "[core]\nvalid = true\n"),
                ("_bmad/custom/config.toml", "[broken\n"),
            ]);
            let error = load(&dir).expect_err("refused").to_string();
            assert!(error.contains("failed to parse"), "{error}");
        }

        #[test]
        fn writes_emoji_json_when_stdout_encoding_is_cp1252() {
            let dir = root_with(&[(
                "_bmad/config.toml",
                "[agents]\nname = \"Analyst\"\nicon = \"📊\"\n",
            )]);
            let output = to_json(&load(&dir).expect("resolves")).expect("JSON");
            assert!(output.contains("📊"), "{output}");
            let resolved: serde_json::Value = serde_json::from_str(&output).expect("parses");
            assert_eq!(resolved["agents"]["icon"], "📊");
        }
    }

    mod resolve_customization {
        //! `src/scripts/tests/test_resolve_customization.py` at v6.12.0, as the
        //! function the CLI calls with an explicit project root.
        use super::super::*;

        fn write(path: &Path, body: &str) {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
            std::fs::write(path, body).expect("write");
        }

        fn facts(entries: &[&str]) -> String {
            let listed: Vec<String> = entries.iter().map(|entry| format!("\"{entry}\"")).collect();
            format!("[workflow]\npersistent_facts = [{}]\n", listed.join(", "))
        }

        #[test]
        fn writes_emoji_json_when_stdout_encoding_is_cp1252() {
            let dir = tempfile::tempdir().expect("tempdir");
            let skill = dir.path().join("emoji-agent");
            write(
                &skill.join("customize.toml"),
                "[agent]\nname = \"Emoji Agent\"\nicon = \"🧭\"\n",
            );
            let merged = load_customization(None, &skill).expect("resolves");
            let output = to_json(&extract_keys(&merged, ["agent"])).expect("JSON");
            assert!(output.contains("🧭"), "{output}");
            let resolved: serde_json::Value = serde_json::from_str(&output).expect("parses");
            assert_eq!(resolved["agent"]["icon"], "🧭");
        }

        #[test]
        fn explicit_project_root_wins_and_stays_quiet() {
            let dir = tempfile::tempdir().expect("tempdir");
            let home = dir.path().join("home");
            let project = dir.path().join("project");
            let skill = home.join(".claude/skills/demo-skill");
            write(&skill.join("customize.toml"), &facts(&["shipped default"]));
            write(
                &home.join("_bmad/custom/demo-skill.toml"),
                &facts(&["home override"]),
            );
            std::fs::create_dir_all(project.join("_bmad/custom")).expect("mkdir");

            let merged = load_customization(Some(&home), &skill).expect("resolves");
            let resolved = extract_key(&merged, "workflow.persistent_facts");
            assert_eq!(
                resolved
                    .and_then(Value::as_array)
                    .map(|items| { items.iter().filter_map(Value::as_str).collect::<Vec<_>>() }),
                Some(vec!["shipped default", "home override"])
            );
        }
    }
}

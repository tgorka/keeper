// Ported from BMAD-METHOD `src/scripts/render_skill.py` at tag v6.12.0
// (05bfbd46d00766ec88eb9b42e76be2c575d64d7b). MIT, Copyright (c) 2025 BMad
// Code, LLC; see `UPSTREAM.md`. The four token regexes became hand-written
// scanners with the same matches; reading the sources, the configuration and
// publishing the generation are the caller's.

//! BMAD's format-B render: a skill's Markdown sources with their config,
//! customization and snapshot tokens resolved, as one immutable generation
//! named by the hash of everything that went into it.
//!
//! Tokens, as `render_skill.py` reads them:
//! - `{{config.a.b}}` — a value at a dotted path of the central configuration;
//! - `{{.key}}` — the one leaf named `key` anywhere in it (two are a refusal,
//!   [`RenderError::Ambiguous`], never a guess);
//! - `{workflow.x}` — a customization value, rendered by its default's type;
//! - `[[bmad-snapshot:file.md]]` — the path of another source in the generation.
//!
//! A config value's `{project-root}` is bound by [`ProjectRoot`]: BMAD's one
//! absolute directory, or keeper's session (R96), where `{project-root}/_bmad`
//! is the drive's install, read where it is, and every other path is the
//! session's write location.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::LazyLock;

use serde_json::{json, Map, Value as Json};
use sha2::{Digest, Sha256};
use toml::{Table, Value};

use super::config::python_type_name;
use super::py;

/// Why a render was refused. `Display` is upstream's sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// `{{.key}}` names more than one leaf; `paths` in merged-table order.
    Ambiguous {
        key: String,
        paths: Vec<String>,
    },
    Refused(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ambiguous { key, paths } => write!(
                f,
                "ambiguous config value `{key}` found at: {}",
                paths.join(", ")
            ),
            Self::Refused(sentence) => f.write_str(sentence),
        }
    }
}

impl std::error::Error for RenderError {}

fn refused(sentence: String) -> RenderError {
    RenderError::Refused(sentence)
}

/// What `{project-root}` stands for in a config value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRoot {
    /// BMAD's reading: one absolute directory, and a value must resolve to an
    /// absolute path.
    Absolute(String),
    /// keeper's (R96), drive-relative: `{project-root}/_bmad/**` is the drive's
    /// install, read in place; every other `{project-root}` is this write
    /// location inside the session (its `artifacts/`). A value must resolve
    /// inside one or the other.
    Session(String),
}

impl ProjectRoot {
    /// The root as the manifest records it.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Absolute(root) | Self::Session(root) => root,
        }
    }
}

const PROJECT_ROOT: &str = "{project-root}";

/// Where a `{project-root}` path value is read and where it is written in a
/// session (R96): the read location relative to the drive root, the write
/// location under `session`. The install, `{project-root}/_bmad/**`, is read
/// and never written: both are the drive's.
///
/// `None` — refused — unless the value is `{project-root}` alone or followed
/// by `/` and plain relative segments, and `session` is plain relative
/// segments too: no empty, `.` or `..` segment, no backslash, no drive letter,
/// no second `{project-root}` (a trailing `/` is allowed). This is the
/// grammar check only; the caller still resolves both locations through the
/// drive's own containment before reading or writing.
pub fn session_locations(value: &str, session: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix(PROJECT_ROOT)?;
    let relative = match rest.strip_prefix('/') {
        Some(relative) => relative.strip_suffix('/').unwrap_or(relative),
        None if rest.is_empty() => "",
        None => return None,
    };
    if !(relative.is_empty() || is_plain(relative)) || !is_plain(session) {
        return None;
    }
    let write = if is_install(relative) {
        relative.to_owned()
    } else if relative.is_empty() {
        session.to_owned()
    } else {
        format!("{session}/{relative}")
    };
    Some((relative.to_owned(), write))
}

/// Whether `path` is one or more `/`-separated segments that name a place
/// under the directory it is joined to, on every platform keeper runs on.
fn is_plain(path: &str) -> bool {
    path.split('/').all(|segment| {
        let drive_letter = segment.len() >= 2
            && segment.as_bytes()[0].is_ascii_alphabetic()
            && segment.as_bytes()[1] == b':';
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.contains('\\')
            && !segment.contains(PROJECT_ROOT)
            && !drive_letter
    })
}

/// Whether a path relative to the project root names the BMAD install.
fn is_install(relative: &str) -> bool {
    relative == "_bmad" || relative.starts_with("_bmad/")
}

fn require_string<'v>(
    value: Option<&'v Value>,
    label: &str,
    allow_empty: bool,
) -> Result<&'v str, RenderError> {
    let Some(Value::String(text)) = value else {
        return Err(refused(format!(
            "{label} must be a string, got {}",
            value.map_or("NoneType", python_type_name)
        )));
    };
    if !allow_empty && py::strip(text).is_empty() {
        return Err(refused(format!("{label} must not be empty")));
    }
    Ok(text)
}

fn require_string_list(value: &Value, label: &str) -> Result<Vec<String>, RenderError> {
    let Value::Array(items) = value else {
        return Err(refused(format!(
            "{label} must be a list, got {}",
            python_type_name(value)
        )));
    };
    items
        .iter()
        .enumerate()
        .map(|(at, item)| {
            require_string(Some(item), &format!("{label}[{at}]"), false).map(str::to_owned)
        })
        .collect()
}

/// One review layer as the render keeps it.
struct ReviewLayer {
    id: String,
    name: String,
    instruction: String,
    when: Option<String>,
}

impl ReviewLayer {
    fn to_json(&self) -> Json {
        let mut layer = Map::new();
        layer.insert("id".to_owned(), Json::String(self.id.clone()));
        layer.insert("name".to_owned(), Json::String(self.name.clone()));
        layer.insert(
            "instruction".to_owned(),
            Json::String(self.instruction.clone()),
        );
        if let Some(when) = &self.when {
            layer.insert("when".to_owned(), Json::String(when.clone()));
        }
        Json::Object(layer)
    }
}

fn require_review_layers(value: &Value, label: &str) -> Result<Vec<ReviewLayer>, RenderError> {
    let Value::Array(items) = value else {
        return Err(refused(format!("{label} must be a list of tables")));
    };
    let mut layers: Vec<ReviewLayer> = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let item_label = format!("{label}[{at}]");
        let Value::Table(item) = item else {
            return Err(refused(format!("{item_label} must be a table")));
        };
        let id = require_string(item.get("id"), &format!("{item_label}.id"), false)?;
        if layers.iter().any(|layer| layer.id == id) {
            return Err(refused(format!("duplicate review layer id `{id}`")));
        }
        let name = match item.get("name") {
            Some(name) => require_string(Some(name), &format!("{item_label}.name"), false)?,
            None => id,
        };
        let instruction = require_string(
            item.get("instruction"),
            &format!("{item_label}.instruction"),
            true,
        )?;
        let when = item
            .get("when")
            .map(|when| require_string(Some(when), &format!("{item_label}.when"), false))
            .transpose()?;
        layers.push(ReviewLayer {
            id: id.to_owned(),
            name: name.to_owned(),
            instruction: instruction.to_owned(),
            when: when.map(str::to_owned),
        });
    }
    Ok(layers)
}

fn format_markdown_list(items: &[String]) -> String {
    if items.is_empty() {
        return "_None._".to_owned();
    }
    let mut rendered = Vec::new();
    for item in items {
        let lines = py::splitlines(item);
        rendered.push(format!("- {}", lines.first().copied().unwrap_or_default()));
        rendered.extend(lines.iter().skip(1).map(|line| format!("  {line}")));
    }
    rendered.join("\n")
}

fn format_review_layers(layers: &[ReviewLayer]) -> String {
    let active: Vec<&ReviewLayer> = layers
        .iter()
        .filter(|layer| !py::strip(&layer.instruction).is_empty())
        .collect();
    if active.is_empty() {
        return "No active review layers. HALT with blocking condition `no active review layers`."
            .to_owned();
    }
    let sections: Vec<String> = active
        .into_iter()
        .map(|layer| {
            let mut section = vec![format!("#### {} (`{}`)", layer.name, layer.id)];
            if let Some(when) = layer.when.as_deref().filter(|when| !when.is_empty()) {
                section.extend([String::new(), format!("Run only when: {when}")]);
            }
            section.extend([String::new(), py::strip(&layer.instruction).to_owned()]);
            section.join("\n")
        })
        .collect();
    sections.join("\n\n")
}

/// A customization value checked against its default's type: what the
/// manifest records and what the token renders as.
fn resolve_customization_value(
    value: &Value,
    default: &Value,
    label: &str,
) -> Result<(Json, String), RenderError> {
    match default {
        Value::String(default) => {
            let allow_empty =
                py::strip(default).is_empty() || label == "customization.workflow.open_spec";
            let resolved = require_string(Some(value), label, allow_empty)?;
            Ok((Json::String(resolved.to_owned()), resolved.to_owned()))
        }
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_table) => {
            let layers = require_review_layers(value, label)?;
            let resolved = Json::Array(layers.iter().map(ReviewLayer::to_json).collect());
            Ok((resolved, format_review_layers(&layers)))
        }
        Value::Array(_) => {
            let items = require_string_list(value, label)?;
            let rendered = format_markdown_list(&items);
            Ok((json!(items), rendered))
        }
        other => Err(refused(format!(
            "{label} has unsupported default type {}",
            python_type_name(other)
        ))),
    }
}

fn lookup<'t>(data: &'t Table, dotted_path: &str, label: &str) -> Result<&'t Value, RenderError> {
    let missing = || refused(format!("missing {label} `{dotted_path}`"));
    let mut parts = dotted_path.split('.');
    let first = parts.next().ok_or_else(missing)?;
    let mut current = data.get(first).ok_or_else(missing)?;
    for part in parts {
        current = current
            .as_table()
            .and_then(|table| table.get(part))
            .ok_or_else(missing)?;
    }
    Ok(current)
}

/// Bind a config value's `{project-root}`.
fn resolve_config_value(
    value: &Value,
    label: &str,
    root: &ProjectRoot,
) -> Result<String, RenderError> {
    let text = require_string(Some(value), label, false)?;
    if !text.contains(PROJECT_ROOT) {
        return Ok(text.to_owned());
    }
    match root {
        ProjectRoot::Absolute(root) => {
            let resolved = text.replace(PROJECT_ROOT, root);
            if !resolved.starts_with('/') {
                return Err(refused(format!(
                    "{label} must resolve to an absolute path: {resolved}"
                )));
            }
            Ok(resolved)
        }
        ProjectRoot::Session(session) => session_locations(text, session)
            .map(|(_, write)| write)
            .ok_or_else(|| {
                refused(format!(
                    "`{label}` must resolve inside this session: {text}"
                ))
            }),
    }
}

/// Every leaf (a value that is neither a table nor a list) named `key`, with
/// its dotted path, in table order.
fn find_config_values<'t>(
    data: &'t Table,
    key: &str,
    prefix: &str,
    into: &mut Vec<(String, &'t Value)>,
) {
    for (name, value) in data {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            Value::Table(inner) => find_config_values(inner, key, &path, into),
            Value::Array(_) => {}
            leaf if name == key => into.push((path, leaf)),
            _ => {}
        }
    }
}

/// `{{.key}}`: the one leaf named `key`, its path and its bound value.
pub fn resolve_short(
    central: &Table,
    key: &str,
    root: &ProjectRoot,
) -> Result<(String, String), RenderError> {
    let mut matches = Vec::new();
    find_config_values(central, key, "", &mut matches);
    match matches.as_slice() {
        [] => Err(refused(format!("missing config value `{key}`"))),
        [(path, value)] => Ok((
            path.clone(),
            resolve_config_value(value, &format!("config.{path}"), root)?,
        )),
        _ => Err(RenderError::Ambiguous {
            key: key.to_owned(),
            paths: matches.into_iter().map(|(path, _)| path).collect(),
        }),
    }
}

/// One token pattern: a prefix, one or more characters of a class, a suffix.
struct Pattern {
    prefix: &'static str,
    class: fn(u8) -> bool,
    suffix: &'static str,
    /// The captured run must end with this (the snapshot's `.md`).
    ends_with: &'static str,
}

fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn dotted(byte: u8) -> bool {
    word(byte) || byte == b'.' || byte == b'-'
}

fn pathlike(byte: u8) -> bool {
    dotted(byte) || byte == b'/'
}

const CONFIG_TOKEN: Pattern = Pattern {
    prefix: "{{config.",
    class: dotted,
    suffix: "}}",
    ends_with: "",
};
const SHORT_CONFIG_TOKEN: Pattern = Pattern {
    prefix: "{{.",
    class: word,
    suffix: "}}",
    ends_with: "",
};
const CUSTOM_TOKEN: Pattern = Pattern {
    prefix: "{workflow.",
    class: dotted,
    suffix: "}",
    ends_with: "",
};
const SNAPSHOT_TOKEN: Pattern = Pattern {
    prefix: "[[bmad-snapshot:",
    class: pathlike,
    suffix: "]]",
    ends_with: ".md",
};

impl Pattern {
    /// The match starting at `at`: its length and its capture.
    fn match_at<'t>(&self, text: &'t str, at: usize) -> Option<(usize, &'t str)> {
        let start = at + self.prefix.len();
        if !text[at..].starts_with(self.prefix) {
            return None;
        }
        let run = text.as_bytes()[start..]
            .iter()
            .take_while(|byte| (self.class)(**byte))
            .count();
        let capture = &text[start..start + run];
        let fits = capture.len() > self.ends_with.len() && capture.ends_with(self.ends_with);
        (fits && text[start + run..].starts_with(self.suffix))
            .then(|| (self.prefix.len() + run + self.suffix.len(), capture))
    }

    /// Every match, left to right, not overlapping (`re.finditer`).
    fn find_iter<'t>(&'t self, text: &'t str) -> impl Iterator<Item = (&'t str, &'t str)> + 't {
        let mut from = 0;
        std::iter::from_fn(move || {
            while let Some(found) = text[from..].find(self.prefix) {
                let at = from + found;
                match self.match_at(text, at) {
                    Some((len, capture)) => {
                        from = at + len;
                        return Some((&text[at..at + len], capture));
                    }
                    None => from = at + 1,
                }
            }
            None
        })
    }
}

/// A skill's render sources: every `*.md` but `SKILL.md`, by relative path,
/// in `pathlib`'s order (path parts compared one by one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    files: Vec<(String, String)>,
}

impl Sources {
    /// `files` are `(path relative to the skill directory, text)`; reading
    /// them, and refusing one that escapes the directory or is not UTF-8, is
    /// the caller's. `skill_dir` names the directory in the missing-entry
    /// sentence.
    pub fn new(
        skill_dir: &str,
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, RenderError> {
        let mut files: Vec<(String, String)> = files
            .into_iter()
            .filter(|(name, _)| {
                let base = name.rsplit('/').next().unwrap_or(name);
                base.ends_with(".md") && base != "SKILL.md"
            })
            .collect();
        files.sort_by(|(a, _), (b, _)| a.split('/').cmp(b.split('/')));
        if !files.iter().any(|(name, _)| name == "workflow.md") {
            return Err(refused(format!(
                "render entry is missing: {}/workflow.md",
                skill_dir.trim_end_matches('/')
            )));
        }
        Ok(Self { files })
    }

    /// The sources in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
    }

    /// Whether any source holds a `{workflow.*}` token, so the render needs the
    /// skill's customization.
    pub fn uses_customization(&self) -> bool {
        self.iter()
            .any(|(_, text)| CUSTOM_TOKEN.find_iter(text).next().is_some())
    }
}

/// The skill's customization, when its sources use it: the skill's own
/// `customize.toml` (the defaults that type each value) and the merged layers.
#[derive(Debug, Clone, Copy)]
pub struct Customization<'a> {
    pub defaults: &'a Table,
    pub merged: &'a Table,
}

/// Each token's replacement, in first-seen order, and each input's resolved
/// value by its source (`config.a.b`, `customization.workflow.x`).
#[derive(Debug, Clone, PartialEq)]
pub struct Replacements {
    pub tokens: Vec<(String, String)>,
    pub values: BTreeMap<String, Json>,
}

impl Replacements {
    fn set(&mut self, token: &str, value: String) {
        match self.tokens.iter_mut().find(|(known, _)| known == token) {
            Some((_, existing)) => *existing = value,
            None => self.tokens.push((token.to_owned(), value)),
        }
    }
}

/// Every token of every source resolved, source by source: short config
/// tokens, then config paths, then customization values.
pub fn resolve_replacements(
    sources: &Sources,
    central: &Table,
    customization: Option<Customization<'_>>,
    root: &ProjectRoot,
) -> Result<Replacements, RenderError> {
    let mut out = Replacements {
        tokens: Vec::new(),
        values: BTreeMap::new(),
    };
    for (_, content) in sources.iter() {
        for (token, key) in SHORT_CONFIG_TOKEN.find_iter(content) {
            let (path, resolved) = resolve_short(central, key, root)?;
            out.values
                .insert(format!("config.{path}"), Json::String(resolved.clone()));
            out.set(token, resolved);
        }
        for (token, path) in CONFIG_TOKEN.find_iter(content) {
            let source = format!("config.{path}");
            let resolved =
                resolve_config_value(lookup(central, path, "config value")?, &source, root)?;
            out.values.insert(source, Json::String(resolved.clone()));
            out.set(token, resolved);
        }
        for (token, relative) in CUSTOM_TOKEN.find_iter(content) {
            let Some(customization) = customization else {
                return Err(refused(
                    "customization tokens require customize.toml".to_owned(),
                ));
            };
            let path = format!("workflow.{relative}");
            let source = format!("customization.{path}");
            let (resolved, rendered) = resolve_customization_value(
                lookup(customization.merged, &path, "customization value")?,
                lookup(customization.defaults, &path, "customization default")?,
                &source,
            )?;
            out.values.insert(source, resolved);
            out.set(token, rendered);
        }
    }
    Ok(out)
}

/// Every source with its tokens replaced in one pass: the longest token
/// first, then snapshot references, which become paths inside `destination`.
/// A customization value's `{skill-root}` is bound to `destination` first;
/// inserted text is never scanned again.
pub fn render_sources(
    sources: &Sources,
    replacements: &Replacements,
    destination: &str,
) -> Result<Vec<(String, String)>, RenderError> {
    let mut tokens: Vec<(&str, String)> = replacements
        .tokens
        .iter()
        .map(|(token, value)| {
            let value = if token.starts_with("{workflow.") {
                value.replace("{skill-root}", destination)
            } else {
                value.clone()
            };
            (token.as_str(), value)
        })
        .collect();
    tokens.sort_by_key(|(token, _)| std::cmp::Reverse(token.len()));
    let names: Vec<&str> = sources.iter().map(|(name, _)| name).collect();

    let mut rendered = Vec::with_capacity(names.len());
    for (name, content) in sources.iter() {
        let mut out = String::with_capacity(content.len());
        let mut copied = 0;
        let mut at = 0;
        while let Some(found) = content[at..].find(['{', '[']) {
            at += found;
            let replaced = tokens
                .iter()
                .find(|(token, _)| content[at..].starts_with(token))
                .map(|(token, value)| (token.len(), value.clone()));
            let replaced = match replaced {
                Some(replaced) => Some(replaced),
                None => match SNAPSHOT_TOKEN.match_at(content, at) {
                    Some((len, target)) if names.contains(&target) => {
                        Some((len, format!("{destination}/{target}")))
                    }
                    Some((_, target)) => {
                        return Err(refused(format!(
                            "snapshot reference targets undeclared source: {target}"
                        )))
                    }
                    None => None,
                },
            };
            match replaced {
                Some((len, value)) => {
                    out.push_str(&content[copied..at]);
                    out.push_str(&value);
                    at += len;
                    copied = at;
                }
                None => at += 1,
            }
        }
        out.push_str(&content[copied..]);
        rendered.push((name.to_owned(), out));
    }
    Ok(rendered)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `json.dumps(..., sort_keys=True)`'s key order, whatever map `serde_json`
/// was built with.
fn sorted(value: Json) -> Json {
    match value {
        Json::Object(map) => {
            let entries: BTreeMap<String, Json> = map
                .into_iter()
                .map(|(key, value)| (key, sorted(value)))
                .collect();
            Json::Object(entries.into_iter().collect())
        }
        Json::Array(items) => Json::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The identity hash's input: compact, sorted keys, non-ASCII as itself.
fn canonical_json(value: &Json) -> Vec<u8> {
    serde_json::to_vec(&sorted(value.clone())).unwrap_or_default()
}

/// The renderer's own identity, as upstream hashes `render_skill.py`: the
/// SHA-256 of this file, so any change to the renderer is a new generation.
pub fn renderer_sha256() -> &'static str {
    static HASH: LazyLock<String> = LazyLock::new(|| sha256_hex(include_bytes!("render.rs")));
    &HASH
}

/// A generation's name parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// The first 20 hex digits of the identity's hash.
    pub hash: String,
    /// The root's last segment, lower-cased to `[a-z0-9-]`.
    pub slug: String,
    /// The first 12 hex digits of the root's hash.
    pub root_hash: String,
}

/// A rendered generation, ready for the caller to publish.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub generation: Generation,
    pub destination: String,
    /// The rendered sources, in source order.
    pub outputs: Vec<(String, String)>,
    pub manifest: Json,
    /// `manifest.json` as upstream writes it: two-space indented, sorted keys,
    /// non-ASCII as itself, then a newline.
    pub manifest_bytes: Vec<u8>,
}

fn slug_of(root: &str) -> String {
    let last = root
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let mut slug = String::new();
    for c in last.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "project" } else { slug };
    let cut: String = slug.chars().take(80).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() { "project" } else { cut }.to_owned()
}

/// Render a skill (`render_skill.render` without its file I/O): resolve every
/// token, name the generation, bind it to `destination(&generation)` and
/// build the manifest. `renderer` is the renderer's identity
/// ([`renderer_sha256`] for this port).
pub fn render(
    skill: &str,
    sources: &Sources,
    central: &Table,
    customization: Option<Customization<'_>>,
    root: &ProjectRoot,
    renderer: &str,
    destination: impl FnOnce(&Generation) -> String,
) -> Result<Rendered, RenderError> {
    let replacements = resolve_replacements(sources, central, customization, root)?;
    let source_sha256: Map<String, Json> = sources
        .iter()
        .map(|(name, text)| (name.to_owned(), Json::String(sha256_hex(text.as_bytes()))))
        .collect();
    let identity = json!({
        "project_root": root.as_str(),
        "renderer_sha256": renderer,
        "resolved_values": replacements.values,
        "source_sha256": source_sha256,
    });
    let full_root_hash = sha256_hex(root.as_str().as_bytes());
    let generation = Generation {
        hash: sha256_hex(&canonical_json(&identity))[..20].to_owned(),
        slug: slug_of(root.as_str()),
        root_hash: full_root_hash[..12].to_owned(),
    };
    let destination = destination(&generation);
    let outputs = render_sources(sources, &replacements, &destination)?;
    let output_sha256: Map<String, Json> = outputs
        .iter()
        .map(|(name, text)| (name.clone(), Json::String(sha256_hex(text.as_bytes()))))
        .collect();
    let manifest = sorted(json!({
        "schema_version": 1,
        "skill": skill,
        "project_root": root.as_str(),
        "project_slug": generation.slug,
        "root_hash": generation.root_hash,
        "generation_hash": generation.hash,
        "inputs": identity,
        "outputs": output_sha256,
    }));
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| refused(format!("manifest cannot be written: {error}")))?;
    manifest_bytes.push(b'\n');
    Ok(Rendered {
        generation,
        destination,
        outputs,
        manifest,
        manifest_bytes,
    })
}

/// Check a generation that already exists at `destination` before reusing it:
/// its `manifest.json` (`existing`) must equal `manifest` as parsed JSON, its
/// files (`files`, every file under it by relative path, `manifest.json`
/// included) must be exactly the outputs and the manifest, and each output
/// must hash as recorded.
pub fn verify_existing(
    destination: &str,
    manifest: &Json,
    existing: &[u8],
    files: &HashMap<String, Vec<u8>>,
) -> Result<(), RenderError> {
    let parsed: Json = std::str::from_utf8(existing)
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str(text).map_err(|error| error.to_string()))
        .map_err(|error| {
            refused(format!(
                "corrupt existing generation {destination}: {error}"
            ))
        })?;
    if &parsed != manifest {
        return Err(refused(format!(
            "generation collision or corruption at {destination}"
        )));
    }
    let outputs = manifest
        .get("outputs")
        .and_then(Json::as_object)
        .ok_or_else(|| {
            refused(format!(
                "generation collision or corruption at {destination}"
            ))
        })?;
    let expected = outputs.len() + 1;
    if files.len() != expected
        || !files.contains_key("manifest.json")
        || !outputs.keys().all(|name| files.contains_key(name))
    {
        return Err(refused(format!(
            "generation contains unexpected or missing files: {destination}"
        )));
    }
    for (name, hash) in outputs {
        if files.get(name).map(|bytes| sha256_hex(bytes)).as_deref() != hash.as_str() {
            return Err(refused(format!(
                "generation output hash mismatch: {destination}/{name}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Goldens `render_skill.py` printed in process (fixture README), the
    //! refusals it raised, then keeper's session root and this repo's own
    //! `_bmad/config.toml`, whose duplicated module keys BMAD refuses.
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::bmad::config::{load_central_config, load_customization, load_toml, Layer};

    /// `sha256(_bmad/scripts/render_skill.py)` at v6.12.0, the identity the
    /// goldens were printed with.
    const PYTHON_RENDERER: &str =
        "8496d0d8b449d64c21b42a9aab3b13fc8a813a430c0695ffd84ad75bb1da7942";
    const ROOT: &str = "/fixture-root";

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bmad")
    }

    fn files_under(dir: &Path, prefix: &str, into: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("a directory") {
            let path = entry.expect("an entry").path();
            let name = path
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            let relative = format!("{prefix}{name}");
            if path.is_dir() {
                files_under(&path, &format!("{relative}/"), into);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                into.push((relative, text));
            }
        }
    }

    fn sources_of(dir: &Path) -> Sources {
        let mut files = Vec::new();
        files_under(dir, "", &mut files);
        Sources::new(&dir.display().to_string(), files).expect("sources")
    }

    fn central_at(root: &Path) -> Table {
        load_central_config(|path| Layer::read(&root.join(path))).expect("central")
    }

    fn table(text: &str) -> Table {
        toml::from_str(text).expect("test TOML parses")
    }

    /// Render a fixture skill as `render_skill.render` would under
    /// `/fixture-root`, its central configuration being `render/_bmad/`.
    fn render_fixture(skill: &str) -> Rendered {
        let dir = fixtures().join(skill);
        let sources = sources_of(&dir);
        let central = central_at(&fixtures().join("render"));
        let defaults = load_toml(&dir.join("customize.toml"), true).expect("defaults");
        let merged = load_customization(Some(&fixtures().join("render")), &dir).expect("merged");
        let customization = sources.uses_customization().then_some(Customization {
            defaults: &defaults,
            merged: &merged,
        });
        render(
            skill,
            &sources,
            &central,
            customization,
            &ProjectRoot::Absolute(ROOT.to_owned()),
            PYTHON_RENDERER,
            |generation| {
                format!(
                    "{ROOT}/_bmad/render/{skill}/{}-{}/{}",
                    generation.slug, generation.root_hash, generation.hash
                )
            },
        )
        .expect("renders")
    }

    fn expected(path: &str) -> String {
        std::fs::read_to_string(fixtures().join("expected").join(path)).expect("golden")
    }

    #[test]
    fn render_tokens_match_render_skill() {
        for skill in ["bmad-build", "all-tokens"] {
            let dir = fixtures().join(skill);
            let sources = sources_of(&dir);
            let central = central_at(&fixtures().join("render"));
            let defaults = load_toml(&dir.join("customize.toml"), true).expect("defaults");
            let merged =
                load_customization(Some(&fixtures().join("render")), &dir).expect("merged");
            let replacements = resolve_replacements(
                &sources,
                &central,
                Some(Customization {
                    defaults: &defaults,
                    merged: &merged,
                }),
                &ProjectRoot::Absolute(ROOT.to_owned()),
            )
            .expect("resolves");
            let ordered: Table = replacements
                .tokens
                .iter()
                .map(|(token, value)| (token.clone(), Value::String(value.clone())))
                .collect();
            assert_eq!(
                crate::bmad::config::to_json(&ordered).expect("JSON"),
                expected(&format!("render-{skill}/replacements.out")),
                "{skill}: replacements, in first-seen order"
            );

            let rendered = render_fixture(skill);
            for (name, text) in &rendered.outputs {
                assert_eq!(
                    *text,
                    expected(&format!("render-{skill}/outputs/{name}")),
                    "{skill}/{name}"
                );
            }
            assert_eq!(
                String::from_utf8(rendered.manifest_bytes.clone()).expect("UTF-8"),
                expected(&format!("render-{skill}/manifest.out")),
                "{skill}: manifest.json"
            );
        }
    }

    #[test]
    fn render_refusals_match_render_skill() {
        let golden: Json = serde_json::from_str(&expected("render-refusals.out")).expect("JSON");
        let central = table("core = { name = \"x\" }\na = { key = \"1\" }\nb = { key = \"2\" }");
        let root = ProjectRoot::Absolute(ROOT.to_owned());
        let refusal = |source: &str, central: &Table, custom: Option<(&str, &str)>| {
            let sources = Sources::new("s", [("workflow.md".to_owned(), source.to_owned())])
                .expect("sources");
            let tables = custom.map(|(merged, defaults)| (table(merged), table(defaults)));
            let customization = tables
                .as_ref()
                .map(|(merged, defaults)| Customization { defaults, merged });
            resolve_replacements(&sources, central, customization, &root)
                .and_then(|replacements| {
                    render_sources(&sources, &replacements, "/fixture-root/out")
                })
                .expect_err("refused")
                .to_string()
        };
        let layers = |items: &str| format!("[workflow]\nlayers = [{items}]");
        let cases = [
            (
                "missing config path",
                refusal("{{config.a.b}}", &central, None),
            ),
            ("missing short key", refusal("{{.nokey}}", &central, None)),
            ("ambiguous short key", refusal("{{.key}}", &central, None)),
            (
                "undeclared snapshot",
                refusal("[[bmad-snapshot:nope.md]]", &central, None),
            ),
            (
                "unsupported default type",
                refusal(
                    "{workflow.flag}",
                    &central,
                    Some(("workflow.flag = true", "workflow.flag = true")),
                ),
            ),
            (
                "review layer without instruction",
                refusal(
                    "{workflow.layers}",
                    &central,
                    Some((
                        &layers(r#"{ id = "a" }"#),
                        &layers(r#"{ id = "a", instruction = "x" }"#),
                    )),
                ),
            ),
            (
                "duplicate review layer id",
                refusal(
                    "{workflow.layers}",
                    &central,
                    Some((
                        &layers(
                            r#"{ id = "a", instruction = "x" }, { id = "a", instruction = "y" }"#,
                        ),
                        &layers(r#"{ id = "a", instruction = "x" }"#),
                    )),
                ),
            ),
            (
                "string expected",
                refusal(
                    "{workflow.word}",
                    &central,
                    Some(("workflow.word = 7", "workflow.word = \"default\"")),
                ),
            ),
            (
                "customization without customize.toml",
                refusal("{workflow.word}", &central, None),
            ),
            (
                "relative project root",
                refusal(
                    "{{config.core.name}}",
                    &table("core = { name = \"x{project-root}\" }"),
                    None,
                ),
            ),
        ];
        for (case, sentence) in cases {
            assert_eq!(Some(sentence.as_str()), golden[case].as_str(), "{case}");
        }
        assert_eq!(
            golden.as_object().map(Map::len),
            Some(10),
            "every golden case is run"
        );
    }

    #[test]
    fn render_in_a_session_stays_inside_it() {
        let session = ProjectRoot::Session("60-sessions/active/s/artifacts".to_owned());
        let central = table(
            r#"
            [modules.bmm]
            planning_artifacts = "{project-root}/_bmad-output/planning-artifacts"
            out = "{project-root}/../../escaped"
            glued = "{project-root}x"
            twice = "{project-root}/a/{project-root}"
            install = "{project-root}/_bmad/bmm/config.yaml"
            words = "Polish"
            "#,
        );
        let resolve = |key: &str| resolve_short(&central, key, &session).map(|(_, value)| value);
        assert_eq!(
            resolve("planning_artifacts").as_deref(),
            Ok("60-sessions/active/s/artifacts/_bmad-output/planning-artifacts")
        );
        assert_eq!(
            resolve("install").as_deref(),
            Ok("_bmad/bmm/config.yaml"),
            "the install is read where the drive holds it"
        );
        assert_eq!(resolve("words").as_deref(), Ok("Polish"));
        for (key, value) in [
            ("out", "{project-root}/../../escaped"),
            ("glued", "{project-root}x"),
            ("twice", "{project-root}/a/{project-root}"),
        ] {
            assert_eq!(
                resolve(key).map_err(|error| error.to_string()),
                Err(format!(
                    "`config.modules.bmm.{key}` must resolve inside this session: {value}"
                ))
            );
        }
        assert_eq!(
            session_locations(
                "{project-root}/_bmad-output/planning-artifacts",
                "60-sessions/active/s/artifacts"
            ),
            Some((
                "_bmad-output/planning-artifacts".to_owned(),
                "60-sessions/active/s/artifacts/_bmad-output/planning-artifacts".to_owned()
            ))
        );
        assert_eq!(
            session_locations("{project-root}/docs/", "s/artifacts"),
            Some(("docs".to_owned(), "s/artifacts/docs".to_owned())),
            "a trailing slash is allowed"
        );
        assert_eq!(
            session_locations("{project-root}", "s/artifacts"),
            Some((String::new(), "s/artifacts".to_owned()))
        );
        let artifacts = "60-sessions/active/s/artifacts";
        for value in [
            "{project-root}/../../outside",
            "{project-root}/..\\..\\..\\..\\outside.md",
            "{project-root}/a\\b",
            "{project-root}//etc",
            "{project-root}/./x",
            "{project-root}/x/./y",
            "{project-root}/C:/x",
            "{project-root}/c:x",
            "{project-root}x",
            "{project-root}/a/{project-root}",
        ] {
            assert_eq!(session_locations(value, artifacts), None, "{value}");
        }
        for root in [
            "/abs/artifacts",
            "C:/s",
            "s\\artifacts",
            "s/../x",
            "",
            "s//a",
            "./s",
        ] {
            assert_eq!(
                session_locations("{project-root}/docs", root),
                None,
                "{root:?}"
            );
        }
    }

    fn this_repos_bmad() -> Table {
        central_at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
    }

    /// This repo's installer-written `_bmad/config.toml` carries three keys in
    /// both `[modules.bmm]` and `[modules.gds]`; BMAD refuses each short token
    /// with these sentences (the epic's in-process run), and so does the port,
    /// for the real file and for the fixture copy of the same layers.
    #[test]
    fn duplicate_module_keys_are_refused_as_render_skill_refuses() {
        let root = ProjectRoot::Absolute(ROOT.to_owned());
        for central in [this_repos_bmad(), central_at(&fixtures())] {
            for key in [
                "implementation_artifacts",
                "planning_artifacts",
                "project_knowledge",
            ] {
                assert_eq!(
                    resolve_short(&central, key, &root),
                    Err(RenderError::Ambiguous {
                        key: key.to_owned(),
                        paths: vec![format!("modules.bmm.{key}"), format!("modules.gds.{key}")],
                    })
                );
            }
            let error =
                resolve_short(&central, "implementation_artifacts", &root).expect_err("refused");
            assert_eq!(
                error.to_string(),
                "ambiguous config value `implementation_artifacts` found at: \
                 modules.bmm.implementation_artifacts, modules.gds.implementation_artifacts"
            );

            let dir = fixtures().join("bmad-build");
            let defaults = load_toml(&dir.join("customize.toml"), true).expect("defaults");
            let mut written = false;
            let refused = render(
                "bmad-build",
                &sources_of(&dir),
                &central,
                Some(Customization {
                    defaults: &defaults,
                    merged: &defaults,
                }),
                &root,
                renderer_sha256(),
                |_| {
                    written = true;
                    String::new()
                },
            )
            .expect_err("bmad-build is refused");
            assert_eq!(refused.to_string(), error.to_string());
            assert!(!written, "refused before a generation is named");
        }
    }

    #[test]
    fn module_scoped_paths_still_resolve() {
        let central = this_repos_bmad();
        let sources = Sources::new(
            "s",
            [(
                "workflow.md".to_owned(),
                "{{config.modules.bmm.implementation_artifacts}}".to_owned(),
            )],
        )
        .expect("sources");
        let replacements = resolve_replacements(
            &sources,
            &central,
            None,
            &ProjectRoot::Absolute(ROOT.to_owned()),
        )
        .expect("an explicit path is never ambiguous");
        assert_eq!(
            replacements.tokens,
            [(
                "{{config.modules.bmm.implementation_artifacts}}".to_owned(),
                "/fixture-root/_bmad-output/implementation-artifacts".to_owned()
            )]
        );
    }

    #[test]
    fn an_existing_generation_is_reused_only_when_it_is_exactly_this_one() {
        let rendered = render_fixture("all-tokens");
        let at = &rendered.destination;
        let mut files: HashMap<String, Vec<u8>> = rendered
            .outputs
            .iter()
            .map(|(name, text)| (name.clone(), text.clone().into_bytes()))
            .collect();
        files.insert("manifest.json".to_owned(), rendered.manifest_bytes.clone());
        let verify = |manifest: &[u8], files: &HashMap<String, Vec<u8>>| {
            verify_existing(at, &rendered.manifest, manifest, files).map_err(|e| e.to_string())
        };
        assert_eq!(verify(&rendered.manifest_bytes, &files), Ok(()));

        let compact = serde_json::to_vec(&rendered.manifest).expect("JSON");
        assert_eq!(verify(&compact, &files), Ok(()), "compared as parsed JSON");
        assert_eq!(
            verify(b"{\"schema_version\": 2}", &files),
            Err(format!("generation collision or corruption at {at}"))
        );
        assert!(verify(b"{", &files)
            .expect_err("corrupt")
            .starts_with(&format!("corrupt existing generation {at}: ")));

        let mut extra = files.clone();
        extra.insert("notes.md".to_owned(), Vec::new());
        assert_eq!(
            verify(&rendered.manifest_bytes, &extra),
            Err(format!(
                "generation contains unexpected or missing files: {at}"
            ))
        );
        let mut edited = files.clone();
        edited.insert("workflow.md".to_owned(), b"edited".to_vec());
        assert_eq!(
            verify(&rendered.manifest_bytes, &edited),
            Err(format!("generation output hash mismatch: {at}/workflow.md"))
        );
    }

    #[test]
    fn sources_follow_pathlib_order_and_need_an_entry() {
        let names = |files: &[&str]| {
            Sources::new(
                "skill",
                files.iter().map(|name| ((*name).to_owned(), String::new())),
            )
            .map(|sources| {
                sources
                    .iter()
                    .map(|(name, _)| name.to_owned())
                    .collect::<Vec<_>>()
            })
            .map_err(|error| error.to_string())
        };
        assert_eq!(
            names(&[
                "workflow.md",
                "steps-notes.md",
                "steps/a.md",
                "SKILL.md",
                "x/SKILL.md",
                "a.toml"
            ]),
            Ok(vec![
                "steps/a.md".to_owned(),
                "steps-notes.md".to_owned(),
                "workflow.md".to_owned()
            ])
        );
        assert_eq!(
            names(&["steps/workflow.md"]),
            Err("render entry is missing: skill/workflow.md".to_owned())
        );
    }
}

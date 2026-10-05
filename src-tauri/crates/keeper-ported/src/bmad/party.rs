// Ported from BMAD-METHOD `skills/bmad-party-mode/scripts/resolve_party.py`
// at tag v6.12.0 (05bfbd46d00766ec88eb9b42e76be2c575d64d7b). MIT, Copyright
// (c) 2025 BMad Code, LLC; see `UPSTREAM.md`. The script's two resolver
// subprocesses became parameters: the merged `[workflow]` table and the
// central configuration (`None` when it did not resolve).

//! Party mode's roster: the installed BMAD agents and the custom
//! `party_members` merged into one collective, then projected three ways —
//! the room to load on entry ([`roster`]), the menu of rooms ([`groups`]) and
//! one room in full ([`group`]).
//!
//! Every projection is a table in the script's key order; [`to_json`]
//! (`config::to_json`) prints it as the script does.

use std::collections::HashMap;
use std::fmt;

use toml::{Table, Value};

use super::config::python_type_name;
use super::py;

pub use super::config::to_json;

/// A roster value the script would have crashed on. `Display` names the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyError {
    message: String,
}

impl PartyError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for PartyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PartyError {}

/// The short alias of an installed agent's code: `bmad-agent-analyst` →
/// `analyst`, `bmad-foo` → `foo`.
pub fn alias(code: &str) -> &str {
    ["bmad-agent-", "bmad-"]
        .into_iter()
        .find_map(|prefix| code.strip_prefix(prefix))
        .unwrap_or(code)
}

/// Every member, the tokens that name each, and the default room.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collective {
    /// Every member, installed and custom, by canonical code.
    pub members: Table,
    /// Code, lower-cased code, lower-cased alias and lower-cased name →
    /// canonical code.
    pub index: HashMap<String, String>,
    /// The installed agents' codes in order: the default room. A custom member
    /// that overrides one keeps its slot; a new custom member is not in it.
    pub installed: Vec<String>,
}

impl Collective {
    fn register(&mut self, code: &str, entry: Table, field: &str) -> Result<(), PartyError> {
        let name = match entry.get("name") {
            Some(Value::String(name)) if !name.is_empty() => Some(name.to_lowercase()),
            Some(value) if py::truthy(value) => {
                return Err(PartyError::new(format!(
                    "{field} `{code}`'s name must be text, got {}",
                    python_type_name(value)
                )))
            }
            _ => None,
        };
        self.members.insert(code.to_owned(), Value::Table(entry));
        for token in [
            code.to_owned(),
            code.to_lowercase(),
            alias(code).to_lowercase(),
        ]
        .into_iter()
        .chain(name)
        {
            self.index.insert(token, code.to_owned());
        }
        Ok(())
    }

    /// The canonical code a token names: as written, then lower-cased.
    fn lookup(&self, token: &str) -> Option<&String> {
        self.index
            .get(token)
            .or_else(|| self.index.get(&token.to_lowercase()))
    }
}

/// One pool keyed by code. `agents` is the central `[agents]` table; a custom
/// member whose code, alias or name matches an installed agent overrides it
/// in place and keeps the installed fields it does not set.
pub fn build_collective(
    agents: &Table,
    party_members: Option<&Value>,
) -> Result<Collective, PartyError> {
    let mut collective = Collective::default();
    for (code, info) in agents {
        let Value::Table(info) = info else {
            return Err(PartyError::new(format!(
                "installed agent `{code}` must be a table, got {}",
                python_type_name(info)
            )));
        };
        let text = |key: &str| {
            info.get(key)
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()))
        };
        let mut entry = Table::new();
        entry.insert("code".to_owned(), Value::String(code.clone()));
        entry.insert(
            "name".to_owned(),
            info.get("name")
                .cloned()
                .unwrap_or_else(|| Value::String(code.clone())),
        );
        for key in ["icon", "title", "description", "module", "team"] {
            entry.insert(key.to_owned(), text(key));
        }
        entry.insert("source".to_owned(), Value::String("installed".to_owned()));
        collective.register(code, entry, "installed agent")?;
        collective.installed.push(code.clone());
    }

    let customs = match party_members {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    for member in customs {
        let Value::Table(member) = member else {
            continue;
        };
        let code = match member.get("code") {
            Some(Value::String(code)) if !code.is_empty() => code,
            Some(value) if py::truthy(value) => {
                return Err(PartyError::new(format!(
                    "party member code must be text, got {}",
                    python_type_name(value)
                )))
            }
            _ => continue,
        };
        let canonical = collective
            .lookup(code)
            .cloned()
            .unwrap_or_else(|| code.clone());
        let mut entry = match collective.members.get(&canonical) {
            Some(Value::Table(installed)) => installed.clone(),
            _ => Table::new(),
        };
        entry.insert("code".to_owned(), Value::String(canonical.clone()));
        entry.insert("source".to_owned(), Value::String("custom".to_owned()));
        for field in ["name", "icon", "title", "persona", "capabilities", "model"] {
            if let Some(value) = member.get(field) {
                entry.insert(field.to_owned(), value.clone());
            }
        }
        if !entry.contains_key("name") {
            entry.insert("name".to_owned(), Value::String(canonical.clone()));
        }
        collective.register(&canonical, entry, "party member")?;
    }
    Ok(collective)
}

/// Python's iteration over a config value: a list's items, a string's
/// characters, a table's keys; nothing for a falsy value.
fn items(value: &Value, field: &str) -> Result<Vec<Value>, PartyError> {
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::String(text) => Ok(text.chars().map(|c| Value::String(c.to_string())).collect()),
        Value::Table(table) => Ok(table.keys().cloned().map(Value::String).collect()),
        value if !py::truthy(value) => Ok(Vec::new()),
        value => Err(PartyError::new(format!(
            "{field} must be a list, got {}",
            python_type_name(value)
        ))),
    }
}

/// `g.get(key, default) or default` for a group's list-like field.
fn listed(table: &Table, key: &str) -> Result<Vec<Value>, PartyError> {
    match table.get(key) {
        Some(value) => items(value, key),
        None => Ok(Vec::new()),
    }
}

/// The members `tokens` name, in listed order, and the tokens that name none
/// (a token that is not text is never looked up).
pub fn resolve_members(tokens: &[Value], collective: &Collective) -> (Vec<Value>, Vec<Value>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for token in tokens {
        let member = token
            .as_str()
            .and_then(|token| collective.lookup(token))
            .and_then(|code| collective.members.get(code));
        match member {
            Some(member) => resolved.push(member.clone()),
            None => unresolved.push(token.clone()),
        }
    }
    (resolved, unresolved)
}

/// The tables of `groups` that carry a truthy `id`.
fn named_groups(groups: &[Value]) -> impl Iterator<Item = &Table> {
    groups
        .iter()
        .filter_map(Value::as_table)
        .filter(|group| group.get("id").is_some_and(py::truthy))
}

/// The cheap menu: each group's id, name and member count; a group without
/// members is flagged open-cast.
pub fn group_menu(groups: &[Value]) -> Result<Vec<Value>, PartyError> {
    let mut menu = Vec::new();
    for group in named_groups(groups) {
        let members = listed(group, "members")?;
        let mut entry = named(group, "id")?;
        entry.insert(
            "member_count".to_owned(),
            Value::Integer(i64::try_from(members.len()).unwrap_or(i64::MAX)),
        );
        if members.is_empty() {
            entry.insert("open_cast".to_owned(), Value::Boolean(true));
        }
        menu.push(Value::Table(entry));
    }
    Ok(menu)
}

/// A group's `id`, under `key`, then its `name`, which defaults to the id.
fn named(group: &Table, key: &str) -> Result<Table, PartyError> {
    let id = group
        .get("id")
        .ok_or_else(|| PartyError::new("a party group has no `id`".to_owned()))?;
    let mut out = Table::new();
    out.insert(key.to_owned(), id.clone());
    out.insert("name".to_owned(), group.get("name").unwrap_or(id).clone());
    Ok(out)
}

/// The first group whose `id` equals `id` (Python's `==`).
pub fn find_group<'g>(groups: &'g [Value], id: &Value) -> Option<&'g Table> {
    groups
        .iter()
        .filter_map(Value::as_table)
        .find(|group| group.get("id").is_some_and(|own| py::equal(own, id)))
}

/// One group in full: its resolved members, the tokens that resolved to no
/// one, its memory flag (off unless set), its scene when it has one, and the
/// open-cast flag when it lists nobody.
pub fn group_detail(group: &Table, collective: &Collective) -> Result<Table, PartyError> {
    let raw = listed(group, "members")?;
    let (members, unresolved) = resolve_members(&raw, collective);
    let mut out = named(group, "active")?;
    out.insert("members".to_owned(), Value::Array(members));
    out.insert("unresolved".to_owned(), Value::Array(unresolved));
    out.insert(
        "memory_enabled".to_owned(),
        Value::Boolean(group.get("memory").is_some_and(py::truthy)),
    );
    if let Some(scene) = group.get("scene").filter(|scene| py::truthy(scene)) {
        out.insert("scene".to_owned(), scene.clone());
    }
    if raw.is_empty() {
        out.insert("open_cast".to_owned(), Value::Boolean(true));
    }
    Ok(out)
}

/// The party settings read from the merged `[workflow]` table.
struct Settings {
    groups: Vec<Value>,
    default_party: Value,
    party_mode: Value,
    party_memory: bool,
}

impl Settings {
    fn of(workflow: &Table) -> Result<Self, PartyError> {
        let or = |key: &str, default: &str| match workflow.get(key) {
            Some(value) if py::truthy(value) => value.clone(),
            _ => Value::String(default.to_owned()),
        };
        Ok(Self {
            groups: listed(workflow, "party_groups")?,
            default_party: or("default_party", ""),
            party_mode: or("party_mode", "session"),
            party_memory: workflow.get("party_memory").is_none_or(py::truthy),
        })
    }
}

/// The installed agents: `[agents]` of the central configuration, empty when
/// it has none.
fn installed_agents(central: &Table) -> Result<Table, PartyError> {
    match central.get("agents") {
        Some(Value::Table(agents)) => Ok(agents.clone()),
        Some(value) if py::truthy(value) => Err(PartyError::new(format!(
            "`agents` must be a table, got {}",
            python_type_name(value)
        ))),
        _ => Ok(Table::new()),
    }
}

fn collective_of(workflow: &Table, central: Option<&Table>) -> Result<Collective, PartyError> {
    let agents = central
        .map(installed_agents)
        .transpose()?
        .unwrap_or_default();
    build_collective(&agents, workflow.get("party_members"))
}

/// `--list-groups`: the menu of rooms, with no member detail.
pub fn groups(workflow: &Table) -> Result<Table, PartyError> {
    let settings = Settings::of(workflow)?;
    let mut out = Table::new();
    out.insert("party_mode".to_owned(), settings.party_mode);
    out.insert("default_party".to_owned(), settings.default_party);
    out.insert(
        "groups".to_owned(),
        Value::Array(group_menu(&settings.groups)?),
    );
    Ok(out)
}

/// `--party <id>`: one room in full, or `unknown_group` with the menu.
pub fn group(workflow: &Table, central: Option<&Table>, id: &str) -> Result<Table, PartyError> {
    let settings = Settings::of(workflow)?;
    let collective = collective_of(workflow, central)?;
    let mut out = Table::new();
    match find_group(&settings.groups, &Value::String(id.to_owned())) {
        None => {
            out.insert(
                "error".to_owned(),
                Value::String("unknown_group".to_owned()),
            );
            out.insert("requested".to_owned(), Value::String(id.to_owned()));
            out.insert(
                "available".to_owned(),
                Value::Array(group_menu(&settings.groups)?),
            );
        }
        Some(found) => {
            out = group_detail(found, &collective)?;
            out.insert("party_mode".to_owned(), settings.party_mode);
        }
    }
    Ok(out)
}

/// The default projection, the room to load on entry: the `default_party`
/// group when one is set and found, else the installed agents with the global
/// memory flag. `central` is `None` when the configuration did not resolve,
/// which the result reports as `installed_agents_resolved: false`.
pub fn roster(workflow: &Table, central: Option<&Table>) -> Result<Table, PartyError> {
    let settings = Settings::of(workflow)?;
    let collective = collective_of(workflow, central)?;
    let mut out = Table::new();
    out.insert("party_mode".to_owned(), settings.party_mode);
    out.insert(
        "groups".to_owned(),
        Value::Array(group_menu(&settings.groups)?),
    );
    out.insert(
        "installed_agents_resolved".to_owned(),
        Value::Boolean(central.is_some()),
    );
    let chosen = if py::truthy(&settings.default_party) {
        find_group(&settings.groups, &settings.default_party)
    } else {
        None
    };
    match chosen {
        Some(found) => out.extend(group_detail(found, &collective)?),
        None => {
            out.insert("active".to_owned(), Value::String("installed".to_owned()));
            let members = collective
                .installed
                .iter()
                .filter_map(|code| collective.members.get(code).cloned())
                .collect();
            out.insert("members".to_owned(), Value::Array(members));
            out.insert(
                "memory_enabled".to_owned(),
                Value::Boolean(settings.party_memory),
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Upstream's cases (`skills/bmad-party-mode/scripts/tests/
    //! test_resolve_party.py` at v6.12.0), then the three projections against
    //! what the script printed for this repo's agents.
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::bmad::config::{load_central_config, load_customization, Layer};

    fn table(text: &str) -> Table {
        toml::from_str(text).expect("test TOML parses")
    }

    fn agents() -> Table {
        table(
            r#"
            [bmad-agent-analyst]
            name = "Mary"
            icon = "📊"
            title = "Analyst"
            [bmad-agent-pm]
            name = "John"
            icon = "📋"
            title = "PM"
            "#,
        )
    }

    fn members(text: &str) -> Value {
        table(&format!("m = [{text}]"))
            .remove("m")
            .expect("the list")
    }

    fn collective(customs: &str) -> Collective {
        build_collective(&agents(), Some(&members(customs))).expect("builds")
    }

    fn codes(members: &[Value]) -> Vec<&str> {
        members
            .iter()
            .map(|member| member["code"].as_str().expect("a code"))
            .collect()
    }

    fn strings(items: &[&str]) -> Vec<Value> {
        items
            .iter()
            .map(|item| Value::String((*item).to_owned()))
            .collect()
    }

    fn group_of(text: &str) -> Table {
        table(text)
    }

    #[test]
    fn strips_known_prefixes() {
        assert_eq!(alias("bmad-agent-analyst"), "analyst");
        assert_eq!(alias("bmad-foo"), "foo");
    }

    #[test]
    fn passes_through_unprefixed() {
        assert_eq!(alias("morpheus"), "morpheus");
    }

    #[test]
    fn installed_agents_indexed_by_code_alias_and_name() {
        let col = collective("");
        assert_eq!(
            col.members.keys().collect::<Vec<_>>(),
            ["bmad-agent-analyst", "bmad-agent-pm"]
        );
        assert_eq!(col.index["analyst"], "bmad-agent-analyst");
        assert_eq!(col.index["mary"], "bmad-agent-analyst");
        assert_eq!(col.index["bmad-agent-pm"], "bmad-agent-pm");
        assert_eq!(
            col.members["bmad-agent-analyst"]["source"].as_str(),
            Some("installed")
        );
    }

    #[test]
    fn custom_member_appends() {
        let col = collective(r#"{ code = "morpheus", name = "Morpheus", persona = "riddles" }"#);
        assert_eq!(col.members["morpheus"]["source"].as_str(), Some("custom"));
        assert_eq!(col.members["morpheus"]["persona"].as_str(), Some("riddles"));
    }

    #[test]
    fn custom_overrides_installed_by_alias() {
        let col = collective(r#"{ code = "analyst", name = "Mary-Custom", persona = "p" }"#);
        assert!(!col.members.contains_key("analyst"));
        assert_eq!(
            col.members["bmad-agent-analyst"]["source"].as_str(),
            Some("custom")
        );
        assert_eq!(
            col.members["bmad-agent-analyst"]["name"].as_str(),
            Some("Mary-Custom")
        );
    }

    #[test]
    fn member_without_code_skipped() {
        let col = collective(r#"{ name = "Nameless" }"#);
        assert_eq!(
            col.members.keys().collect::<Vec<_>>(),
            ["bmad-agent-analyst", "bmad-agent-pm"]
        );
    }

    #[test]
    fn resolves_in_listed_order_and_flags_unknowns() {
        let col = collective(r#"{ code = "morpheus", name = "Morpheus" }"#);
        let (resolved, unresolved) =
            resolve_members(&strings(&["morpheus", "analyst", "ghost"]), &col);
        assert_eq!(codes(&resolved), ["morpheus", "bmad-agent-analyst"]);
        assert_eq!(unresolved, strings(&["ghost"]));
    }

    #[test]
    fn empty() {
        let col = collective(r#"{ code = "morpheus", name = "Morpheus" }"#);
        assert_eq!(resolve_members(&[], &col), (Vec::new(), Vec::new()));
    }

    fn groups_fixture() -> Vec<Value> {
        let Value::Array(groups) = members(
            r#"{ id = "wr", name = "Writers", members = ["analyst", "morpheus"] },
            { id = "bad" },
            { name = "no-id" }"#,
        ) else {
            unreachable!("a list")
        };
        groups
    }

    #[test]
    fn menu_is_names_only_with_counts_and_open_cast_flag() {
        let menu = group_menu(&groups_fixture()).expect("a menu");
        assert_eq!(
            Value::Array(menu),
            members(
                r#"{ id = "wr", name = "Writers", member_count = 2 },
                { id = "bad", name = "bad", member_count = 0, open_cast = true }"#
            )
        );
    }

    #[test]
    fn find_group() {
        let groups = groups_fixture();
        let found = super::find_group(&groups, &Value::String("wr".to_owned()));
        assert_eq!(found.and_then(|g| g["name"].as_str()), Some("Writers"));
        assert!(super::find_group(&groups, &Value::String("missing".to_owned())).is_none());
    }

    fn detail(text: &str) -> Table {
        let col = collective(r#"{ code = "morpheus", name = "Morpheus" }"#);
        group_detail(&group_of(text), &col).expect("a detail")
    }

    #[test]
    fn scene_passes_through_when_present() {
        let d = detail(
            r#"id = "tos-10-forward"
            name = "Ten Forward"
            members = ["morpheus"]
            scene = "Late evening, a few rounds in.""#,
        );
        assert_eq!(d["scene"].as_str(), Some("Late evening, a few rounds in."));
        assert_eq!(
            codes(d["members"].as_array().expect("members")),
            ["morpheus"]
        );
    }

    #[test]
    fn scene_omitted_when_absent_or_empty() {
        for text in [
            r#"id = "g"
            members = ["morpheus"]"#,
            r#"id = "g"
            members = ["morpheus"]
            scene = """#,
        ] {
            assert!(!detail(text).contains_key("scene"), "{text}");
        }
    }

    #[test]
    fn anchored_group_is_not_open_cast() {
        assert!(!detail("id = \"g\"\nmembers = [\"morpheus\"]").contains_key("open_cast"));
    }

    #[test]
    fn open_cast_group_flagged_with_empty_members() {
        let d = detail(
            r#"id = "rebels"
            name = "Star Wars Rebels"
            scene = "Figures from the Rebels universe drop in as the topic calls for them.""#,
        );
        assert_eq!(d["open_cast"].as_bool(), Some(true));
        assert_eq!(d["members"].as_array().map(Vec::len), Some(0));
        assert!(d["scene"].as_str().expect("a scene").starts_with("Figures"));
    }

    #[test]
    fn memory_enabled_follows_group_flag_and_defaults_off() {
        let flag = |extra: &str| {
            detail(&format!("id = \"g\"\nmembers = [\"morpheus\"]\n{extra}"))["memory_enabled"]
                .as_bool()
        };
        assert_eq!(flag("memory = true"), Some(true));
        assert_eq!(flag("memory = false"), Some(false));
        assert_eq!(flag(""), Some(false));
    }

    #[test]
    fn pure_custom_excluded_override_kept_in_default_room() {
        let col = collective(
            r#"{ code = "morpheus", name = "Morpheus" },
            { code = "analyst", name = "Mary-Custom", persona = "p" },
            { code = "sec-hawk", name = "Vex" }"#,
        );
        assert!(col.members.contains_key("morpheus"));
        assert!(col.members.contains_key("sec-hawk"));
        assert_eq!(col.installed, ["bmad-agent-analyst", "bmad-agent-pm"]);
        assert_eq!(
            col.members["bmad-agent-analyst"]["name"].as_str(),
            Some("Mary-Custom")
        );
    }

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bmad")
    }

    fn expected(name: &str) -> String {
        std::fs::read_to_string(fixtures().join("expected").join(name)).expect("golden")
    }

    /// This repo's `[agents.*]` (through the four central layers) and the
    /// installed party skill, projected three ways, byte for byte against
    /// `resolve_party.py`'s output: key order and member order included.
    #[test]
    fn party_roster_matches_resolve_party() {
        let root = fixtures();
        let central = load_central_config(|path| Layer::read(&root.join(path))).expect("central");
        let merged =
            load_customization(Some(&root), &root.join("bmad-party-mode")).expect("merges");
        let workflow = merged["workflow"].as_table().expect("[workflow]");
        let json = |table: Table| to_json(&table).expect("JSON");

        assert_eq!(
            json(roster(workflow, Some(&central)).expect("roster")),
            expected("party-default.out")
        );
        assert_eq!(
            json(groups(workflow).expect("menu")),
            expected("party-groups.out")
        );
        assert_eq!(
            json(group(workflow, Some(&central), "code-review-crew").expect("a room")),
            expected("party-code-review-crew.out")
        );
        assert_eq!(
            json(group(workflow, Some(&central), "no-such-room").expect("unknown")),
            expected("party-unknown.out")
        );

        let unresolved = roster(workflow, None).expect("roster");
        assert_eq!(
            unresolved["installed_agents_resolved"].as_bool(),
            Some(false)
        );
        assert_eq!(unresolved["members"].as_array().map(Vec::len), Some(0));

        // A custom member named like an installed agent overrides it in place
        // and keeps the installed fields it does not set.
        let mut custom = workflow.clone();
        custom.insert(
            "party_members".to_owned(),
            members(r#"{ code = "winston", persona = "draws boxes" }"#),
        );
        let room = roster(&custom, Some(&central)).expect("roster");
        let winston = room["members"]
            .as_array()
            .and_then(|members| {
                members
                    .iter()
                    .find(|m| m["code"].as_str() == Some("bmad-agent-architect"))
            })
            .expect("Winston keeps his slot");
        assert_eq!(winston["source"].as_str(), Some("custom"));
        assert_eq!(winston["persona"].as_str(), Some("draws boxes"));
        assert_eq!(winston["title"].as_str(), Some("System Architect"));
        assert_eq!(
            room["members"].as_array().map(Vec::len),
            Some(central["agents"].as_table().map_or(0, Table::len))
        );
    }

    /// A numeric `default_party` matches a group id only at its exact value,
    /// as Python's `==` does: 2**53 + 1 is not the float 2**53.
    #[test]
    fn a_numeric_default_party_matches_only_its_exact_group() {
        let room = |default: &str| {
            let workflow = table(&format!(
                "default_party = {default}\n[[party_groups]]\nid = 9007199254740992.0\nmembers = []\n"
            ));
            roster(&workflow, Some(&Table::new())).expect("roster")["active"].clone()
        };
        assert_eq!(
            room("9007199254740993"),
            Value::String("installed".to_owned())
        );
        assert_eq!(
            room("9007199254740992"),
            Value::Float(9_007_199_254_740_992.0)
        );
    }

    #[test]
    fn a_value_the_script_would_crash_on_is_refused_with_its_field() {
        let refused = build_collective(&agents(), Some(&members("{ code = 7 }")))
            .expect_err("a number is not a code");
        assert_eq!(
            refused.to_string(),
            "party member code must be text, got int"
        );
        let workflow = table("party_groups = 3");
        assert_eq!(
            groups(&workflow).expect_err("refused").to_string(),
            "party_groups must be a list, got int"
        );
    }
}

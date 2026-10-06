//! An agent's BMAD and skill tools (story 94.2, AD-397): `bmad_config` and
//! `bmad_party` over `keeper_ported::bmad`, `skills_list` and `skill_view`
//! over the zone's `_skills/` ([`crate::skills`]). Each is a read, T0 (R105),
//! served by the agent's own host through `ToolHost::run_named` — never a
//! ⌘9 bot's (R38) — and nothing here runs Python.
//!
//! The home drive's root is BMAD's project root (R96): the four central
//! layers and the overlays are read from its `_bmad/`, a skill's or a
//! workflow's `customize.toml` from the agents zone, and every
//! `{project-root}` path is stated with where it is read (the drive) and
//! where it is written (the session's `artifacts/`). Every file is reached
//! through `browse::resolve`: an `_bmad/custom/` that is absent, or that
//! leads out of the drive (a host-local symlink), holds no layer, and the
//! result says so (R97). Sentences name drive-relative paths.
//!
//! Each file read is kept as a [`FileRead`]: where it landed and the label
//! facts of the bytes returned, taken at the read, so the turn's reporter
//! labels what the model was given and never reopens the file (R195).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use keeper_core::agents::card::marked_untrusted;
use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::label::{
    label_drive_read, okf_label_facts, Author, Integrity, Label, OkfLabelFacts, ReadFacts,
};
use keeper_core::agents::skills::SkillsIndex;
use keeper_core::agents::workflow::{
    self, ConfigCall, ConfigScope, PartyCall, BMAD_CONFIG, BMAD_PARTY, SKILLS_LIST, SKILL_VIEW,
};
use keeper_core::bots::chat::ToolCall as WireToolCall;
use keeper_core::bots::tools::ToolOutcome;
use keeper_ported::bmad::config::{self, ConfigError, Layer, Source};
use keeper_ported::bmad::{party, render};
use keeper_sync::browse;
use serde_json::Value as Json;
use toml::{Table, Value};

/// The tools this module serves.
pub const TOOLS: [&str; 4] = [SKILLS_LIST, SKILL_VIEW, BMAD_CONFIG, BMAD_PARTY];

/// Whether `name` is one of [`TOOLS`].
pub fn serves(name: &str) -> bool {
    TOOLS.contains(&name)
}

/// The party skill, by its BMAD name: its customization is the roster's.
const PARTY_SKILL: &str = "bmad-party-mode";

/// The overlays' folder, drive-relative.
const CUSTOM_DIR: &str = "_bmad/custom";

/// Where `_bmad/custom/` stands in this drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Custom {
    /// A folder of the drive: its layers are read.
    Here,
    /// Not in the drive.
    Absent,
    /// A link that leads out of the drive, never followed (AD-65).
    LeadsOut,
}

impl Custom {
    fn of(root: &Path) -> Custom {
        match browse::resolve(root, CUSTOM_DIR) {
            Ok(Some(path)) if path.is_dir() => Custom::Here,
            Ok(_) => Custom::Absent,
            Err(_) => Custom::LeadsOut,
        }
    }

    /// What the result says when no overlay is read.
    fn sentence(self) -> Option<&'static str> {
        match self {
            Custom::Here => None,
            Custom::Absent => Some(
                "`_bmad/custom/` is not in this drive, so no team or personal overlay applies.",
            ),
            Custom::LeadsOut => Some(
                "`_bmad/custom/` leads out of this drive, so keeper does not read it; no team or personal overlay applies.",
            ),
        }
    }
}

/// One file a BMAD or skill tool read: the path the call named, where it
/// landed, and the label facts of the bytes it returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRead {
    /// The path the call named, drive-relative.
    pub requested: String,
    /// Where it landed, drive-relative through every link; `None` where
    /// that could not be established.
    pub landed: Option<String>,
    /// The OKF facts of the bytes returned.
    pub okf: OkfLabelFacts,
    /// Whether the bytes returned are a card marked `integrity: untrusted`.
    pub card_untrusted: bool,
}

impl FileRead {
    /// The read of `requested` in the drive at `root`, which `browse::resolve`
    /// landed at `path`, returning `bytes`.
    pub fn of(root: &Path, requested: String, path: &Path, bytes: &[u8]) -> FileRead {
        let landed = root.canonicalize().ok().and_then(|root| {
            let rel = path.strip_prefix(root).ok()?;
            let parts: Option<Vec<&str>> = rel
                .components()
                .map(|part| part.as_os_str().to_str())
                .collect();
            Some(parts?.join("/"))
        });
        let text = String::from_utf8_lossy(bytes);
        FileRead {
            requested,
            landed,
            okf: okf_label_facts(&text),
            card_untrusted: marked_untrusted(&text),
        }
    }

    /// Where the read came from, drive-relative: its landing where known.
    pub fn path(&self) -> &str {
        self.landed.as_deref().unwrap_or(&self.requested)
    }

    /// The read's label under the drive's declaration: the stricter of the
    /// requested path's and the landing's, `untrusted` where the landing is
    /// not known.
    pub fn label(&self, drive: &DriveDecl) -> Label {
        let at = |path: &str| {
            label_drive_read(
                drive,
                &ReadFacts {
                    path: path.to_owned(),
                    last_author: Author::Unknown,
                    okf_human_reviewed: self.okf.human_reviewed,
                    okf_external_source: self.okf.external_source,
                    card_untrusted: self.card_untrusted,
                },
            )
        };
        let requested = at(&self.requested);
        match &self.landed {
            Some(landed) => requested.join(&at(landed)),
            None => Label {
                integrity: Integrity::Untrusted,
                ..requested
            },
        }
    }
}

/// One turn's BMAD and skill tools, and the drive files they read.
pub struct BmadTools {
    /// The home drive's id.
    pub drive: String,
    /// The home drive's root on this host: BMAD's project root.
    pub root: PathBuf,
    /// The agents zone, drive-relative (`_skills/` and `_workflows/` live
    /// there).
    pub zone: String,
    /// The session's `artifacts/`, drive-relative: where `{project-root}`
    /// is written (R96).
    pub artifacts: String,
    /// The workflow the session runs, by its folder under `_workflows/`.
    pub workflow: Option<String>,
    /// The skills offered to the agent, and the ones refused (89.3).
    pub skills: SkillsIndex,
    /// The files this turn's calls read, until the turn's reporter takes
    /// them.
    reads: Mutex<Vec<FileRead>>,
}

fn refused(reason: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Refused {
        reason: reason.into(),
    }
}

/// A text result: what the drive's files said, printed as BMAD's script
/// prints it.
fn text(body: String) -> ToolOutcome {
    ToolOutcome::Text {
        body,
        truncated_at: None,
        of_bytes: None,
        okf: None,
    }
}

impl BmadTools {
    pub fn new(
        drive: String,
        root: PathBuf,
        zone: String,
        artifacts: String,
        workflow: Option<String>,
        skills: SkillsIndex,
    ) -> BmadTools {
        BmadTools {
            drive,
            root,
            zone,
            artifacts,
            workflow,
            skills,
            reads: Mutex::new(Vec::new()),
        }
    }

    /// The files read since the last take.
    pub fn take_reads(&self) -> Vec<FileRead> {
        std::mem::take(&mut *self.reads.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn read(&self, files: Vec<FileRead>) {
        self.reads
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(files);
    }

    /// Where a call reads, drive-relative, as its audit row names it.
    pub fn at(&self, wire: &WireToolCall) -> String {
        let args = wire.arguments.as_ref().unwrap_or(&Json::Null);
        let skills = format!("{}/_skills", self.zone);
        match wire.name.as_str() {
            SKILLS_LIST => skills,
            SKILL_VIEW => format!(
                "{skills}/{}/{}",
                args["name"].as_str().unwrap_or(""),
                args["path"].as_str().unwrap_or("SKILL.md")
            ),
            _ => "_bmad".to_owned(),
        }
    }

    /// Run `wire`, one of [`TOOLS`], keeping the files it read.
    pub fn run(&self, wire: &WireToolCall) -> ToolOutcome {
        let args = wire.arguments.as_ref().unwrap_or(&Json::Null);
        let mut files = Vec::new();
        let outcome = match wire.name.as_str() {
            SKILLS_LIST => match workflow::parse_list(args) {
                Ok(()) => crate::skills::list(&self.skills),
                Err(sentence) => refused(sentence),
            },
            SKILL_VIEW => match workflow::parse_view(args) {
                Ok(call) => {
                    crate::skills::view(&self.skills, &self.root, &self.zone, &call, &mut files)
                }
                Err(sentence) => refused(sentence),
            },
            BMAD_CONFIG => match workflow::parse_config(args) {
                Ok(call) => self.config(&call, &mut files),
                Err(sentence) => refused(sentence),
            },
            BMAD_PARTY => match workflow::parse_party(args) {
                Ok(call) => self.party(&call, &mut files),
                Err(sentence) => refused(sentence),
            },
            other => refused(format!("{other} is not one of this agent's tools.")),
        };
        self.read(files);
        outcome
    }

    /// One layer at `rel` of the drive, read through `browse::resolve`; a
    /// layer under an `_bmad/custom/` the drive does not hold is absent.
    fn layer(&self, rel: &str, custom: Custom, files: &mut Vec<FileRead>) -> Layer {
        let overlay = rel.starts_with(CUSTOM_DIR) && rel[CUSTOM_DIR.len()..].starts_with('/');
        let source = if overlay && custom != Custom::Here {
            Source::Absent
        } else {
            match browse::resolve(&self.root, rel) {
                Ok(None) => Source::Absent,
                Ok(Some(path)) if !path.is_file() => Source::NotAFile,
                Ok(Some(path)) => match std::fs::read(&path) {
                    Ok(bytes) => {
                        files.push(FileRead::of(&self.root, rel.to_owned(), &path, &bytes));
                        Source::Bytes(bytes)
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Source::Absent,
                    Err(error) => Source::Unreadable(error.to_string()),
                },
                Err(refusal) => Source::Unreadable(refusal.to_string()),
            }
        };
        Layer {
            name: rel.to_owned(),
            source,
        }
    }

    fn central(&self, custom: Custom, files: &mut Vec<FileRead>) -> Result<Table, ConfigError> {
        config::load_central_config(|rel| self.layer(rel, custom, files))
    }

    /// The folder whose `customize.toml` a skill's customization starts
    /// from, drive-relative, and the name its overlays are keyed by (the
    /// folder's, as `resolve_customization.py` keys them): the offered
    /// skill `skill` under `_skills/`, else the workflow the session runs.
    fn folder(&self, skill: Option<&str>) -> Result<(String, String), String> {
        match (skill, &self.workflow) {
            (Some(skill), _) => {
                if !self.skills.offered.iter().any(|entry| entry.name == skill) {
                    return Err(crate::skills::not_offered(&self.skills, skill));
                }
                Ok((format!("{}/_skills/{skill}", self.zone), skill.to_owned()))
            }
            (None, Some(workflow)) => Ok((
                format!("{}/_workflows/{workflow}", self.zone),
                workflow.clone(),
            )),
            (None, None) => Err(
                "This session runs no workflow; name an offered skill under _skills/ as skill."
                    .to_owned(),
            ),
        }
    }

    /// A skill's three customization layers: its `customize.toml`, then
    /// the drive's two overlays named after it.
    fn customization(
        &self,
        folder: &str,
        name: &str,
        custom: Custom,
        files: &mut Vec<FileRead>,
    ) -> (Layer, [Layer; 2]) {
        let defaults = self.layer(&format!("{folder}/customize.toml"), custom, files);
        let overlays =
            config::customization_overlays(name).map(|rel| self.layer(&rel, custom, files));
        (defaults, overlays)
    }

    /// `bmad_config`: what `resolve_config.py` or
    /// `resolve_customization.py` prints, under `config`, with the
    /// central configuration's roots (R96) and the overlays' absence (R97).
    fn config(&self, call: &ConfigCall, files: &mut Vec<FileRead>) -> ToolOutcome {
        let custom = Custom::of(&self.root);
        let merged = match &call.scope {
            ConfigScope::Central => self.central(custom, files),
            ConfigScope::Customization { skill } => match self.folder(skill.as_deref()) {
                Ok((folder, name)) => {
                    let (defaults, overlays) = self.customization(&folder, &name, custom, files);
                    config::merge_customization(defaults, Some(overlays))
                }
                Err(sentence) => return refused(sentence),
            },
        };
        let merged = match merged {
            Ok(merged) => merged,
            Err(error) => return refused(error.to_string()),
        };
        let shown = match &call.keys {
            Some(keys) => config::extract_keys(&merged, keys.iter().map(String::as_str)),
            None => merged,
        };
        let roots = (call.scope == ConfigScope::Central).then(|| self.roots(&shown));
        let mut result = Table::new();
        result.insert("config".to_owned(), Value::Table(shown));
        if let Some(roots) = roots {
            result.insert("roots".to_owned(), Value::Table(roots));
        }
        if let Some(sentence) = custom.sentence() {
            result.insert("overlays".to_owned(), Value::String(sentence.to_owned()));
        }
        match config::to_json(&result) {
            Ok(json) => text(json),
            Err(error) => refused(error.to_string()),
        }
    }

    /// The roots of `config` (R96): the drive, its install, the session's
    /// output, and each `{project-root}` path key's read and write
    /// location, by dotted key.
    fn roots(&self, config: &Table) -> Table {
        let mut paths = Table::new();
        path_keys(config, "", &self.artifacts, &mut paths);
        let mut roots = Table::new();
        roots.insert("drive".to_owned(), Value::String(self.drive.clone()));
        roots.insert("install".to_owned(), Value::String("_bmad/".to_owned()));
        roots.insert(
            "output".to_owned(),
            Value::String(format!("{}/", self.artifacts)),
        );
        roots.insert("paths".to_owned(), Value::Table(paths));
        roots
    }

    /// `bmad_party`: what `resolve_party.py` prints for the drive's install,
    /// under `party`, with the overlays' absence (R97) as `bmad_config`
    /// states it. The party's customization is the running workflow's when
    /// the session runs `bmad-party-mode`, else the offered skill's; a
    /// merge that fails falls back to the skill's own `customize.toml`, and
    /// the installed agents to none (`installed_agents_resolved: false`),
    /// as the script falls back.
    fn party(&self, call: &PartyCall, files: &mut Vec<FileRead>) -> ToolOutcome {
        let skill = (self.workflow.as_deref() != Some(PARTY_SKILL)).then_some(PARTY_SKILL);
        let (folder, name) = match self.folder(skill) {
            Ok(found) => found,
            Err(sentence) => return refused(sentence),
        };
        let custom = Custom::of(&self.root);
        let (defaults, overlays) = self.customization(&folder, &name, custom, files);
        let workflow = config::merge_customization(defaults.clone(), Some(overlays))
            .ok()
            .and_then(|merged| match merged.get("workflow") {
                Some(Value::Table(workflow)) => Some(workflow.clone()),
                _ => None,
            })
            .or_else(|| match defaults.parse(true).ok()?.remove("workflow") {
                Some(Value::Table(workflow)) => Some(workflow),
                _ => None,
            })
            .unwrap_or_default();
        let projected = match call {
            PartyCall::Groups => party::groups(&workflow),
            PartyCall::Group(id) => {
                let central = self.central(custom, files).ok();
                party::group(&workflow, central.as_ref(), id)
            }
            PartyCall::Roster => {
                let central = self.central(custom, files).ok();
                party::roster(&workflow, central.as_ref())
            }
        };
        let projected = match projected {
            Ok(table) => table,
            Err(error) => return refused(error.to_string()),
        };
        let mut result = Table::new();
        result.insert("party".to_owned(), Value::Table(projected));
        if let Some(sentence) = custom.sentence() {
            result.insert("overlays".to_owned(), Value::String(sentence.to_owned()));
        }
        match config::to_json(&result) {
            Ok(json) => text(json),
            Err(error) => refused(error.to_string()),
        }
    }
}

/// Each `{project-root}` string under `table` at its dotted key: where it
/// is read and where it is written in a session whose `artifacts/` is
/// `artifacts` (`render::session_locations`), or the sentence saying it
/// resolves nowhere in the drive.
fn path_keys(table: &Table, prefix: &str, artifacts: &str, out: &mut Table) {
    for (key, value) in table {
        let dotted = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Table(inner) => path_keys(inner, &dotted, artifacts, out),
            Value::String(path) if path.contains("{project-root}") => {
                let located = match render::session_locations(path, artifacts) {
                    Some((read, write)) => {
                        let mut at = Table::new();
                        at.insert("read".to_owned(), Value::String(read));
                        at.insert("write".to_owned(), Value::String(write));
                        Value::Table(at)
                    }
                    None => Value::String(format!("{path} does not resolve inside this drive")),
                };
                out.insert(dotted, located);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! Each tool over a temp drive holding 94.1's fixtures: what BMAD's own
    //! scripts printed for them, byte for byte.
    use super::*;
    use keeper_core::agents::skills::{index, SkillFilter};

    const ZONE: &str = "80-agents";
    const ARTIFACTS: &str = "60-sessions/active/2026-10-06-arch/artifacts";

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../keeper-ported/tests/fixtures/bmad")
    }

    fn expected(name: &str) -> String {
        std::fs::read_to_string(fixtures().join("expected").join(name)).expect("golden")
    }

    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("mkdir");
        for entry in std::fs::read_dir(from).expect("read_dir").flatten() {
            let target = to.join(entry.file_name());
            if entry.file_type().expect("type").is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).expect("copy");
            }
        }
    }

    /// A drive holding the fixtures' `_bmad/` and, under the zone's
    /// `_skills/`, the skills `skills` (fixture folders), each with a
    /// `SKILL.md` that agentskills accepts.
    fn drive(skills: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tempdir");
        copy(&fixtures().join("_bmad"), &root.path().join("_bmad"));
        for skill in skills {
            let dir = root.path().join(ZONE).join("_skills").join(skill);
            copy(&fixtures().join(skill), &dir);
            std::fs::write(
                dir.join("SKILL.md"),
                format!(
                    "---\nname: {skill}\ndescription: The {skill} fixture.\n---\n\nFollow it.\n"
                ),
            )
            .expect("SKILL.md");
        }
        root
    }

    fn tools_over(root: &Path, workflow: Option<&str>) -> BmadTools {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root.join(ZONE).join("_skills")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let text =
                    std::fs::read_to_string(entry.path().join("SKILL.md")).expect("SKILL.md");
                found.push((name, text));
            }
        }
        BmadTools::new(
            "tgdrive".to_owned(),
            root.to_owned(),
            ZONE.to_owned(),
            ARTIFACTS.to_owned(),
            workflow.map(str::to_owned),
            index(&found, &SkillFilter::All),
        )
    }

    fn call(tools: &BmadTools, name: &str, args: Json) -> ToolOutcome {
        tools.run(&WireToolCall {
            id: "call-1".to_owned(),
            name: name.to_owned(),
            arguments_raw: args.to_string(),
            arguments: Some(args),
        })
    }

    fn body(outcome: ToolOutcome) -> String {
        match outcome {
            ToolOutcome::Text { body, .. } => body,
            other => panic!("not text: {other:?}"),
        }
    }

    fn reason(outcome: ToolOutcome) -> String {
        match outcome {
            ToolOutcome::Refused { reason } => reason,
            other => panic!("not refused: {other:?}"),
        }
    }

    /// The dump under `config`, re-indented as it sits one level down.
    fn nested(golden: &str) -> String {
        golden.trim_end().replace('\n', "\n  ")
    }

    /// `resolve_party.py`'s output `golden` as `bmad_party` prints it for
    /// a drive that holds `_bmad/custom/`: under `party`.
    fn party_of(golden: &str) -> String {
        format!("{{\n  \"party\": {}\n}}\n", nested(golden))
    }

    /// Where each file `tools` read since the last take came from.
    fn paths(tools: &BmadTools) -> Vec<String> {
        let mut read: Vec<String> = tools
            .take_reads()
            .iter()
            .map(|read| read.path().to_owned())
            .collect();
        read.sort();
        read
    }

    fn decl() -> DriveDecl {
        keeper_core::agents::drive::parse(
            "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\n",
        )
        .expect("a declaration")
    }

    /// 94.2 acceptance 2: `bmad_config` prints what the resolvers print for
    /// the drive's install, states where each `{project-root}` path is read
    /// and written (R96), reads a workflow's overlays by its folder's name,
    /// and names a missing required layer by its drive-relative path.
    #[test]
    fn bmad_config_is_the_resolvers() {
        let root = drive(&["bmad-architecture"]);
        let tools = tools_over(root.path(), None);

        let central = body(call(
            &tools,
            BMAD_CONFIG,
            serde_json::json!({"scope": "central"}),
        ));
        assert!(
            central.contains(&format!("\"config\": {}", nested(&expected("central.out")))),
            "{central}"
        );
        let parsed: Json = serde_json::from_str(&central).expect("JSON");
        assert_eq!(
            parsed["roots"]["paths"]["modules.bmm.planning_artifacts"],
            serde_json::json!({
                "read": "_bmad-output/planning-artifacts",
                "write": format!("{ARTIFACTS}/_bmad-output/planning-artifacts"),
            })
        );
        assert_eq!(parsed["roots"]["install"], "_bmad/");
        assert_eq!(parsed["roots"]["output"], format!("{ARTIFACTS}/"));
        assert_eq!(
            parsed.get("overlays"),
            None,
            "the drive holds _bmad/custom/"
        );
        assert_eq!(
            paths(&tools),
            [
                "_bmad/config.toml",
                "_bmad/config.user.toml",
                "_bmad/custom/config.toml"
            ]
        );

        let agents = body(call(
            &tools,
            BMAD_CONFIG,
            serde_json::json!({"scope": "central", "keys": ["agents", "no.such.key"]}),
        ));
        assert!(
            agents.contains(&format!(
                "\"config\": {}",
                nested(&expected("central-agents.out"))
            )),
            "{agents}"
        );

        // A skill's customization with the drive's overlay, keyed by the
        // skill's folder; and the same folder as the session's workflow.
        let skill = body(call(
            &tools,
            BMAD_CONFIG,
            serde_json::json!({"scope": "customization", "skill": "bmad-architecture", "keys": ["workflow"]}),
        ));
        let golden = format!(
            "{{\n  \"config\": {}\n}}\n",
            nested(&expected("architecture-workflow.out"))
        );
        assert_eq!(skill, golden);
        std::fs::create_dir_all(root.path().join(ZONE).join("_workflows")).expect("mkdir");
        std::fs::rename(
            root.path().join(ZONE).join("_skills/bmad-architecture"),
            root.path().join(ZONE).join("_workflows/bmad-architecture"),
        )
        .expect("moved");
        let running = BmadTools {
            workflow: Some("bmad-architecture".to_owned()),
            ..tools
        };
        assert_eq!(
            body(call(
                &running,
                BMAD_CONFIG,
                serde_json::json!({"scope": "customization", "keys": ["workflow"]}),
            )),
            golden
        );
        reason(call(
            &tools_over(root.path(), None),
            BMAD_CONFIG,
            serde_json::json!({"scope": "customization"}),
        ));

        std::fs::remove_file(root.path().join("_bmad/config.toml")).expect("rm");
        assert_eq!(
            reason(call(
                &running,
                BMAD_CONFIG,
                serde_json::json!({"scope": "central"})
            )),
            "required TOML file not found: _bmad/config.toml"
        );
    }

    /// R97: keeper reads only what the drive holds. An `_bmad/custom/` that
    /// is not there, or that is a link out of the drive (tgdrive's host-local
    /// symlink into makistack), applies no overlay, and the result says
    /// which; the link is never followed.
    #[cfg(unix)]
    #[test]
    fn overlays_are_read_only_from_the_drive() {
        let root = drive(&[]);
        let outside = tempfile::tempdir().expect("outside");
        std::fs::rename(
            root.path().join("_bmad/custom"),
            outside.path().join("custom"),
        )
        .expect("move");
        let tools = tools_over(root.path(), None);
        let without = body(call(
            &tools,
            BMAD_CONFIG,
            serde_json::json!({"scope": "central"}),
        ));
        let parsed: Json = serde_json::from_str(&without).expect("JSON");
        assert_eq!(
            parsed["overlays"],
            Custom::Absent.sentence().expect("a sentence")
        );
        // Without the team layer, the merge is the installer's two alone.
        assert_ne!(
            parsed["config"],
            serde_json::from_str::<Json>(&expected("central.out")).expect("golden")
        );

        std::os::unix::fs::symlink(
            outside.path().join("custom"),
            root.path().join("_bmad/custom"),
        )
        .expect("link");
        let linked = body(call(
            &tools,
            BMAD_CONFIG,
            serde_json::json!({"scope": "central"}),
        ));
        let parsed: Json = serde_json::from_str(&linked).expect("JSON");
        assert_eq!(
            parsed["overlays"],
            Custom::LeadsOut.sentence().expect("a sentence")
        );
        assert_eq!(
            parsed["config"],
            serde_json::from_str::<Json>(&without).expect("JSON")["config"]
        );
        assert!(
            tools
                .take_reads()
                .iter()
                .all(|file| !file.requested.starts_with("_bmad/custom/")),
            "nothing behind the link was read"
        );
    }

    /// 94.2 acceptance 5: `bmad_party` prints `resolve_party.py`'s three
    /// projections for the drive's install, from the offered party skill.
    #[test]
    fn bmad_party_is_resolve_party() {
        let root = drive(&["bmad-party-mode"]);
        let tools = tools_over(root.path(), None);
        assert_eq!(
            body(call(&tools, BMAD_PARTY, serde_json::json!({}))),
            party_of(&expected("party-default.out"))
        );
        assert_eq!(
            body(call(
                &tools,
                BMAD_PARTY,
                serde_json::json!({"list_groups": true})
            )),
            party_of(&expected("party-groups.out"))
        );
        assert_eq!(
            body(call(
                &tools,
                BMAD_PARTY,
                serde_json::json!({"party": "code-review-crew"})
            )),
            party_of(&expected("party-code-review-crew.out"))
        );
        assert_eq!(
            body(call(
                &tools,
                BMAD_PARTY,
                serde_json::json!({"party": "no-such-room"})
            )),
            party_of(&expected("party-unknown.out"))
        );

        let bare = drive(&[]);
        reason(call(
            &tools_over(bare.path(), None),
            BMAD_PARTY,
            serde_json::json!({}),
        ));
    }

    /// R97 for `bmad_party` (R195): each projection says when no team or
    /// personal overlay was read — `_bmad/custom/` absent, or a link out of
    /// the drive that is never followed.
    #[cfg(unix)]
    #[test]
    fn bmad_party_says_which_overlays_it_did_not_read() {
        let root = drive(&["bmad-party-mode"]);
        let tools = tools_over(root.path(), None);
        let calls = [
            serde_json::json!({}),
            serde_json::json!({"list_groups": true}),
            serde_json::json!({"party": "code-review-crew"}),
        ];
        let results = |tools: &BmadTools| -> Vec<Json> {
            calls
                .iter()
                .map(|args| {
                    serde_json::from_str(&body(call(tools, BMAD_PARTY, args.clone())))
                        .expect("JSON")
                })
                .collect()
        };
        for here in results(&tools) {
            assert_eq!(here.get("overlays"), None, "{here}");
        }
        tools.take_reads();

        let outside = tempfile::tempdir().expect("outside");
        std::fs::rename(
            root.path().join("_bmad/custom"),
            outside.path().join("custom"),
        )
        .expect("move");
        let absent = results(&tools);
        for result in &absent {
            assert_eq!(
                result["overlays"],
                Custom::Absent.sentence().expect("a sentence"),
                "{result}"
            );
        }

        std::os::unix::fs::symlink(
            outside.path().join("custom"),
            root.path().join("_bmad/custom"),
        )
        .expect("link");
        tools.take_reads();
        for (linked, absent) in results(&tools).iter().zip(&absent) {
            assert_eq!(
                linked["overlays"],
                Custom::LeadsOut.sentence().expect("a sentence"),
                "{linked}"
            );
            assert_eq!(linked["party"], absent["party"]);
        }
        assert!(
            tools
                .take_reads()
                .iter()
                .all(|file| !file.requested.starts_with("_bmad/custom/")),
            "nothing behind the link was read"
        );
    }

    /// R195: a read is labelled by where it landed as well as the path it
    /// named, the stricter winning, and by the bytes it returned — what the
    /// file holds afterwards, or whether it is still there, changes nothing;
    /// a read whose landing is unknown is `untrusted`.
    #[cfg(unix)]
    #[test]
    fn a_read_is_labelled_by_its_landing_and_its_bytes() {
        let root = drive(&["bmad-architecture"]);
        let decl = decl();
        let tools = tools_over(root.path(), None);

        // An installer layer linked to a file in the inbox, an untrusted zone.
        std::fs::create_dir_all(root.path().join("00-inbox")).expect("mkdir");
        std::fs::rename(
            root.path().join("_bmad/config.user.toml"),
            root.path().join("00-inbox/overlay.toml"),
        )
        .expect("move");
        std::os::unix::fs::symlink(
            root.path().join("00-inbox/overlay.toml"),
            root.path().join("_bmad/config.user.toml"),
        )
        .expect("link");
        body(call(
            &tools,
            BMAD_CONFIG,
            serde_json::json!({"scope": "central"}),
        ));
        let reads = tools.take_reads();
        let of = |requested: &str| {
            reads
                .iter()
                .find(|read| read.requested == requested)
                .expect("read")
        };
        let linked = of("_bmad/config.user.toml");
        assert_eq!(linked.path(), "00-inbox/overlay.toml");
        assert_eq!(linked.label(&decl).integrity, Integrity::Untrusted);
        assert_ne!(
            of("_bmad/config.toml").label(&decl).integrity,
            Integrity::Untrusted
        );

        // A skill file marked untrusted, then replaced by a clean one or
        // removed; and a clean one, then replaced by a marked one.
        let skill = root.path().join(ZONE).join("_skills/bmad-architecture");
        let marked = "---\nintegrity: untrusted\n---\n\nPasted from a web page.\n";
        let clean = "---\nname: notes\n---\n\nWritten here.\n";
        let view = |rel: &str| {
            body(call(
                &tools,
                SKILL_VIEW,
                serde_json::json!({"name": "bmad-architecture", "path": rel}),
            ));
            let mut reads = tools.take_reads();
            assert_eq!(reads.len(), 1);
            reads.remove(0)
        };
        std::fs::write(skill.join("replaced.md"), marked).expect("write");
        std::fs::write(skill.join("removed.md"), marked).expect("write");
        std::fs::write(skill.join("cleaned.md"), clean).expect("write");
        let replaced = view("replaced.md");
        let removed = view("removed.md");
        let cleaned = view("cleaned.md");
        std::fs::write(skill.join("replaced.md"), clean).expect("write");
        std::fs::remove_file(skill.join("removed.md")).expect("rm");
        std::fs::write(skill.join("cleaned.md"), marked).expect("write");
        assert_eq!(replaced.label(&decl).integrity, Integrity::Untrusted);
        assert_eq!(removed.label(&decl).integrity, Integrity::Untrusted);
        assert_ne!(cleaned.label(&decl).integrity, Integrity::Untrusted);

        let unknown = FileRead {
            landed: None,
            ..cleaned
        };
        assert_eq!(unknown.label(&decl).integrity, Integrity::Untrusted);
    }
}

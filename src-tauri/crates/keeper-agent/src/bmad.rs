//! An agent's BMAD and skill tools (story 94.2, AD-397): `bmad_config`,
//! `bmad_render`, `bmad_memlog` and `bmad_party` over `keeper_ported::bmad`,
//! `skills_list` and `skill_view` over the zone's `_skills/`
//! ([`crate::skills`]). The four reads are T0, the two writes T1 (R105);
//! each is served by the agent's own host through `ToolHost::run_named` —
//! never a ⌘9 bot's (R38) — and nothing here runs Python.
//!
//! The home drive's root is BMAD's project root (R96): the four central
//! layers and the overlays are read from its `_bmad/`, a skill's or a
//! workflow's `customize.toml` and sources from the agents zone, and every
//! `{project-root}` path is stated with where it is read (the drive) and
//! where it is written (the session's `artifacts/`). Every file is reached
//! through `browse::resolve`: an `_bmad/custom/` that is absent, or that
//! leads out of the drive (a host-local symlink), holds no layer, and the
//! result says so (R97). Sentences name drive-relative paths.
//!
//! Each file read is kept as a [`FileRead`]: where it landed and the label
//! facts of the bytes returned, taken at the read, so the turn's reporter
//! labels what the model was given and never reopens the file (R195).
//!
//! The writes go through the session runtime's own doors, never a drive
//! write (R51): a render is published once into the session's
//! `workspace/bmad-render/` ([`publish_generation`]), a memlog is the one
//! dotted file of `artifacts/` ([`memlog_write`]), each a journaled,
//! atomic and durable plan made only under the session's claim (R120).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use keeper_core::agents::card::marked_untrusted;
use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::label::{
    label_drive_read, okf_label_facts, Author, Integrity, Label, OkfLabelFacts, ReadFacts,
};
use keeper_core::agents::skills::SkillsIndex;
use keeper_core::agents::workflow::{
    self, ConfigCall, ConfigScope, MemlogCall, MemlogCommand, MemlogTarget, PartyCall, RenderCall,
    BMAD_CONFIG, BMAD_MEMLOG, BMAD_PARTY, BMAD_RENDER, SKILLS_LIST, SKILL_VIEW,
};
use keeper_core::bots::chat::ToolCall as WireToolCall;
use keeper_core::bots::tools::ToolOutcome;
use keeper_ported::bmad::config::{self, ConfigError, Layer, Source};
use keeper_ported::bmad::{memlog, party, render};
use keeper_sync::browse;
use serde_json::Value as Json;
use toml::{Table, Value};

use crate::sessions::write::{in_session, memlog_write, publish_generation};

/// The tools this module serves.
pub const TOOLS: [&str; 6] = [
    SKILLS_LIST,
    SKILL_VIEW,
    BMAD_CONFIG,
    BMAD_RENDER,
    BMAD_MEMLOG,
    BMAD_PARTY,
];

/// What a render answers before its generation's `workflow.md`, as
/// `render_skill.py` prints it.
pub const READ_AND_FOLLOW: &str = "read and follow ";

/// Whether `name` is one of [`TOOLS`].
pub fn serves(name: &str) -> bool {
    TOOLS.contains(&name)
}

/// Whether `name` is one of the two of [`TOOLS`] that write into the
/// session.
pub fn writes(name: &str) -> bool {
    name == BMAD_RENDER || name == BMAD_MEMLOG
}

/// Where a render's generations are published, session-relative.
const RENDER_DIR: &str = "workspace/bmad-render";

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

/// The session a turn's BMAD tools write in.
#[derive(Debug, Clone)]
pub struct SessionFolder {
    /// The home drive's sessions zone on this host.
    pub zone: PathBuf,
    /// The session, zone-relative (`active/<name>`).
    pub path: String,
    /// The session's folder, drive-relative.
    pub dir: String,
}

/// A call of one of the two writes, ready for its admission
/// ([`BmadTools::prepare`]).
pub struct Write {
    /// Where it writes, drive-relative, as its audit row names it: a
    /// prepared render's generation folder.
    pub at: String,
    /// The canonical bytes its declassification binds: a prepared render's
    /// drive, generation folder and manifest SHA-256; else the call's
    /// arguments as the model sent them.
    pub effect: Vec<u8>,
    call: WireToolCall,
    /// A render's generation, or the sentence it was refused with.
    render: Option<Result<render::Rendered, String>>,
    /// The files preparing it read.
    files: Vec<FileRead>,
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
    /// The session the turn runs in: where the two writes land.
    pub session: SessionFolder,
    /// The session's `artifacts/`, drive-relative: where `{project-root}`
    /// is written (R96).
    pub artifacts: String,
    /// The workflow the session runs, by its folder under `_workflows/`.
    pub workflow: Option<String>,
    /// The skills offered to the agent, and the ones refused (89.3).
    pub skills: SkillsIndex,
    /// The memlog's clock: local time to the minute, `memlog.py`'s `now()`.
    now: fn() -> String,
    /// The files this turn's calls read, until the turn's reporter takes
    /// them.
    reads: Mutex<Vec<FileRead>>,
}

/// `memlog.py`'s `now()`: local `%Y-%m-%dT%H:%M`.
fn local_minute() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M").to_string()
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

/// What a write said it did.
fn answered(text: String) -> ToolOutcome {
    ToolOutcome::Answered { text }
}

impl BmadTools {
    pub fn new(
        drive: String,
        root: PathBuf,
        zone: String,
        session: SessionFolder,
        workflow: Option<String>,
        skills: SkillsIndex,
    ) -> BmadTools {
        BmadTools {
            drive,
            root,
            zone,
            artifacts: format!("{}/artifacts", session.dir),
            session,
            workflow,
            skills,
            now: local_minute,
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

    /// Where a call reads or writes, drive-relative, as its audit row names
    /// it.
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
            BMAD_RENDER => format!(
                "{}/{RENDER_DIR}/{}",
                self.session.dir,
                args["skill"]
                    .as_str()
                    .or(self.workflow.as_deref())
                    .unwrap_or("")
            ),
            BMAD_MEMLOG => {
                let file = match (args["path"].as_str(), args["workspace"].as_str()) {
                    (Some(file), _) => memlog::Target::Path(file).file(),
                    (None, run) => memlog::Target::Workspace(run.unwrap_or("")).file(),
                };
                format!(
                    "{}/{}",
                    self.session.dir,
                    in_session(&self.session.dir, &file)
                )
            }
            _ => "_bmad".to_owned(),
        }
    }

    /// Run `wire`, one of [`TOOLS`], keeping the files it read; a write
    /// asks `may_write` — the session's claim — right before its effect.
    pub fn run(&self, wire: &WireToolCall, may_write: &dyn Fn() -> bool) -> ToolOutcome {
        if writes(&wire.name) {
            return self.write(self.prepare(wire), may_write);
        }
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

    /// `wire`, `bmad_render` or `bmad_memlog`, made ready for its
    /// admission: where it writes and the bytes a declassification of it
    /// binds (R191). A render is rendered here, before anything is asked,
    /// so the generation admitted — its folder and its manifest's SHA-256
    /// — is the one [`BmadTools::write`] publishes, and a source or a
    /// customization changed while an approval waits is another effect.
    pub fn prepare(&self, wire: &WireToolCall) -> Write {
        let args = wire.arguments.as_ref().unwrap_or(&Json::Null);
        let mut files = Vec::new();
        let render = (wire.name == BMAD_RENDER).then(|| match workflow::parse_render(args) {
            Ok(call) => self
                .render(&call, &mut files)
                .map_err(|sentence| format!("HALT: {sentence}")),
            Err(sentence) => Err(sentence),
        });
        let (at, effect) = match &render {
            Some(Ok(rendered)) => (
                rendered.destination.clone(),
                serde_json::json!({
                    "drive": self.drive,
                    "path": rendered.destination,
                    "manifest_sha256": keeper_core::agents::approval::sha256_hex(&rendered.manifest_bytes),
                })
                .to_string()
                .into_bytes(),
            ),
            _ => (self.at(wire), wire.arguments_raw.clone().into_bytes()),
        };
        Write {
            at,
            effect,
            call: wire.clone(),
            render,
            files,
        }
    }

    /// Make the write `prepared` admitted, keeping the files it read: a
    /// render publishes the generation it prepared, a memlog runs its
    /// command; each asks `may_write` right before its effect.
    pub fn write(&self, prepared: Write, may_write: &dyn Fn() -> bool) -> ToolOutcome {
        let args = prepared.call.arguments.as_ref().unwrap_or(&Json::Null);
        let outcome = match prepared.render {
            Some(Ok(rendered)) => match self.publish(&rendered, may_write) {
                Ok(entry) => answered(format!("{READ_AND_FOLLOW}{entry}")),
                Err(sentence) => refused(format!("HALT: {sentence}")),
            },
            Some(Err(sentence)) => refused(sentence),
            None => match workflow::parse_memlog(args) {
                Ok(call) => self.memlog(&call, may_write),
                Err(sentence) => refused(sentence),
            },
        };
        self.read(prepared.files);
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

    /// The merged customization of the offered skill `skill`, else of the
    /// workflow the session runs, as `resolve_customization.py` merges it,
    /// the files it read kept in `files`: where a helper's lens is looked
    /// up (94.4).
    pub fn customization_of(
        &self,
        skill: Option<&str>,
        files: &mut Vec<FileRead>,
    ) -> Result<Table, String> {
        let (folder, name) = self.folder(skill)?;
        let (defaults, overlays) =
            self.customization(&folder, &name, Custom::of(&self.root), files);
        config::merge_customization(defaults, Some(overlays)).map_err(|error| error.to_string())
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

    /// `bmad_render`'s render: `render_skill.py`'s of the offered skill
    /// `call.skill`, or of the workflow the session runs, with the drive's
    /// configuration and the session's write location as `{project-root}`
    /// (R96), bound to `workspace/bmad-render/<skill>/<generation>/` of the
    /// session — or the sentence the script halts with. Nothing is written.
    fn render(
        &self,
        call: &RenderCall,
        files: &mut Vec<FileRead>,
    ) -> Result<render::Rendered, String> {
        let (folder, name) = self.folder(call.skill.as_deref())?;
        let sources = self.sources(&folder, files)?;
        let custom = Custom::of(&self.root);
        let central = self.central(custom, files).map_err(|e| e.to_string())?;
        let customization = if sources.uses_customization() {
            let (defaults, overlays) = self.customization(&folder, &name, custom, files);
            let parsed = defaults.clone().parse(true).map_err(|e| e.to_string())?;
            let merged =
                config::merge_customization(defaults, Some(overlays)).map_err(|e| e.to_string())?;
            Some((parsed, merged))
        } else {
            None
        };
        render::render(
            &name,
            &sources,
            &central,
            customization
                .as_ref()
                .map(|(defaults, merged)| render::Customization { defaults, merged }),
            &render::ProjectRoot::Session(self.artifacts.clone()),
            render::renderer_sha256(),
            |generation| {
                format!(
                    "{}/{RENDER_DIR}/{name}/{}",
                    self.session.dir, generation.hash
                )
            },
        )
        .map_err(|e| e.to_string())
    }

    /// Publish `rendered` once into the session (`render_skill.py`'s
    /// `_publish`): the drive-relative entry to follow, or the sentence
    /// the script halts with. A generation already there is used only
    /// when it is exactly this one.
    fn publish(
        &self,
        rendered: &render::Rendered,
        may_write: &dyn Fn() -> bool,
    ) -> Result<String, String> {
        let mut outputs = rendered.outputs.clone();
        outputs.push((
            "manifest.json".to_owned(),
            String::from_utf8_lossy(&rendered.manifest_bytes).into_owned(),
        ));
        publish_generation(
            &self.session.zone,
            &self.session.path,
            in_session(&self.session.dir, &rendered.destination),
            &outputs,
            |found| {
                let found = found.map_err(|reason| {
                    format!(
                        "corrupt existing generation {}: {reason}",
                        rendered.destination
                    )
                })?;
                let existing = found.get("manifest.json").map_or(&[][..], Vec::as_slice);
                render::verify_existing(&rendered.destination, &rendered.manifest, existing, found)
                    .map_err(|e| e.to_string())
            },
            may_write,
        )
        .map_err(|e| e.to_string())?;
        Ok(format!("{}/workflow.md", rendered.destination))
    }

    /// A skill's render sources, as `_load_sources` reads them: every
    /// `*.md` but `SKILL.md` under `folder` (drive-relative), by its path
    /// inside it. A source that leads out of the folder is refused, never
    /// read.
    fn sources(&self, folder: &str, files: &mut Vec<FileRead>) -> Result<render::Sources, String> {
        let dir = match browse::resolve(&self.root, folder) {
            Ok(Some(dir)) if dir.is_dir() => dir,
            Ok(_) => return Err(format!("render entry is missing: {folder}/workflow.md")),
            Err(refusal) => return Err(format!("{folder} is refused: {refusal}")),
        };
        let mut names = Vec::new();
        markdown_under(&dir, folder, "", &mut names)?;
        let mut read = Vec::with_capacity(names.len());
        for name in names {
            let path = match browse::resolve(&dir, &name) {
                Ok(Some(path)) if path.is_file() => path,
                Ok(_) => {
                    return Err(format!(
                        "render source is missing or not a file: {folder}/{name}"
                    ))
                }
                Err(_) => return Err(format!("render source escapes skill directory: {name}")),
            };
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("failed to read render source {folder}/{name}: {e}"))?;
            files.push(FileRead::of(
                &self.root,
                format!("{folder}/{name}"),
                &path,
                text.as_bytes(),
            ));
            read.push((name, text));
        }
        render::Sources::new(folder, read).map_err(|e| e.to_string())
    }

    /// `bmad_memlog`: `memlog.py`'s command on a memlog of the session's
    /// `artifacts/`, through the memlog door: its one-line ack, or its
    /// refusal. The ack names the memlog as the call did.
    fn memlog(&self, call: &MemlogCall, may_write: &dyn Fn() -> bool) -> ToolOutcome {
        let file = match &call.target {
            MemlogTarget::Workspace(run) => memlog::Target::Workspace(run).file(),
            MemlogTarget::Path(file) => memlog::Target::Path(file).file(),
        };
        let now = (self.now)();
        let mut body = String::new();
        let written = memlog_write(
            &self.session.zone,
            &self.session.path,
            in_session(&self.session.dir, &file),
            may_write,
            |old| {
                let written = match (&call.command, old) {
                    (MemlogCommand::Init { fields }, old) => memlog::init(
                        &file,
                        old.is_some(),
                        fields.iter().map(String::as_str),
                        &now,
                    ),
                    (_, None) => {
                        return Err(format!(
                            "{file} is not there yet; bmad_memlog's init starts it."
                        ))
                    }
                    (
                        MemlogCommand::Append {
                            text,
                            entry_type,
                            by,
                        },
                        Some(old),
                    ) => memlog::append(old, text, entry_type.as_deref(), by.as_deref(), &now),
                    (MemlogCommand::Set { key, value }, Some(old)) => {
                        memlog::set(old, key, value, &now)
                    }
                }
                .map_err(|error| error.to_string())?;
                body = written.body;
                Ok(written.text)
            },
        );
        match written {
            Ok(()) => answered(memlog::ack(&file, &body)),
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

/// The names of every `*.md` file but `SKILL.md` under `dir` — the skill
/// folder `folder` (drive-relative) — by `/`-joined path below it, or the
/// sentence naming the folder whose listing failed: a render never
/// publishes from a part of its sources. A folder link is not walked; a
/// file link is named, so its reader refuses it when it leads out.
fn markdown_under(
    dir: &Path,
    folder: &str,
    prefix: &str,
    into: &mut Vec<String>,
) -> Result<(), String> {
    let unread = |error: std::io::Error| {
        format!("render source folder {folder}/{prefix} could not be read: {error}")
    };
    for entry in std::fs::read_dir(dir).map_err(unread)? {
        let entry = entry.map_err(unread)?;
        let base = entry.file_name().to_string_lossy().into_owned();
        let name = format!("{prefix}{base}");
        let kind = entry
            .file_type()
            .map_err(|error| format!("render source {folder}/{name} could not be read: {error}"))?;
        if kind.is_dir() {
            markdown_under(&entry.path(), folder, &format!("{name}/"), into)?;
        } else if base.ends_with(".md") && base != "SKILL.md" {
            into.push(name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Each tool over a temp drive holding 94.1's fixtures: what BMAD's own
    //! scripts printed for them, byte for byte.
    use super::*;
    use keeper_core::agents::skills::{index, SkillFilter};

    const ZONE: &str = "80-agents";
    /// The sessions zone, drive-relative, and the session in it.
    const SESSIONS: &str = "60-sessions";
    const SESSION: &str = "active/2026-10-06-arch";
    const SESSION_DIR: &str = "60-sessions/active/2026-10-06-arch";
    const ARTIFACTS: &str = "60-sessions/active/2026-10-06-arch/artifacts";
    const NOW: &str = "2026-10-06T09:30";

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
        drive_with("_bmad", skills)
    }

    /// [`drive`] with the fixtures' `bmad` folder as the drive's `_bmad/`,
    /// and the session the tools write in.
    fn drive_with(bmad: &str, skills: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tempdir");
        copy(&fixtures().join(bmad), &root.path().join("_bmad"));
        std::fs::create_dir_all(root.path().join(SESSION_DIR).join("artifacts")).expect("mkdir");
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
        BmadTools {
            now: || NOW.to_owned(),
            ..BmadTools::new(
                "tgdrive".to_owned(),
                root.to_owned(),
                ZONE.to_owned(),
                SessionFolder {
                    zone: root.join(SESSIONS),
                    path: SESSION.to_owned(),
                    dir: SESSION_DIR.to_owned(),
                },
                workflow.map(str::to_owned),
                index(&found, &SkillFilter::All),
            )
        }
    }

    fn call(tools: &BmadTools, name: &str, args: Json) -> ToolOutcome {
        call_claimed(tools, name, args, true)
    }

    /// [`call`] on a host whose claim on the session is `held`.
    fn call_claimed(tools: &BmadTools, name: &str, args: Json, held: bool) -> ToolOutcome {
        tools.run(
            &WireToolCall {
                id: "call-1".to_owned(),
                name: name.to_owned(),
                arguments_raw: args.to_string(),
                arguments: Some(args),
            },
            &|| held,
        )
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

    fn said(outcome: ToolOutcome) -> String {
        match outcome {
            ToolOutcome::Answered { text } => text,
            other => panic!("not answered: {other:?}"),
        }
    }

    /// Every file under `dir` with its modification time, by path below it.
    fn stamps(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_owned()];
        while let Some(at) = stack.pop() {
            for entry in std::fs::read_dir(&at).expect("read_dir").flatten() {
                let meta = entry.metadata().expect("meta");
                if meta.is_dir() {
                    stack.push(entry.path());
                }
                out.push((entry.path(), meta.modified().expect("mtime")));
            }
        }
        out.sort();
        out
    }

    /// 94.2 acceptance 3: on a drive whose configuration has no duplicated
    /// keys, `bmad_render` publishes one generation of `bmad-build` into the
    /// session's `workspace/bmad-render/` and says where to read it; a
    /// second call names the same generation and writes nothing; a
    /// generation whose file was edited is refused as the script refuses
    /// it.
    #[test]
    fn bmad_render_publishes_one_generation() {
        let root = drive_with("render/_bmad", &["bmad-build"]);
        let tools = tools_over(root.path(), None);
        let first = said(call(
            &tools,
            BMAD_RENDER,
            serde_json::json!({"skill": "bmad-build"}),
        ));
        let entry = first
            .strip_prefix("read and follow ")
            .expect("the script's line");
        let generation = entry
            .strip_prefix(&format!("{SESSION_DIR}/workspace/bmad-render/bmad-build/"))
            .and_then(|rest| rest.strip_suffix("/workflow.md"))
            .expect("published in the session's workspace");
        assert!(
            generation.len() == 20 && generation.bytes().all(|b| b.is_ascii_hexdigit()),
            "{generation}"
        );
        let published = root.path().join(entry).parent().expect("folder").to_owned();
        let manifest: Json = serde_json::from_slice(
            &std::fs::read(published.join("manifest.json")).expect("manifest"),
        )
        .expect("JSON");
        assert_eq!(manifest["generation_hash"], generation);
        assert_eq!(manifest["project_root"], ARTIFACTS);
        let outputs = manifest["outputs"].as_object().expect("outputs");
        assert_eq!(outputs.len(), 14, "every source but SKILL.md");
        for name in outputs.keys() {
            assert!(published.join(name).is_file(), "{name}");
        }
        // `{project-root}` is the session's write location (R96), and a
        // snapshot reference the generation's own path.
        let step = std::fs::read_to_string(published.join("step-01-clarify-and-route.md"))
            .expect("step 1");
        assert!(
            step.contains(&format!(
                "`{ARTIFACTS}/_bmad-output/implementation-artifacts`"
            )),
            "{step}"
        );
        assert!(step.contains(&format!(
            "`{SESSION_DIR}/workspace/bmad-render/bmad-build/{generation}/compile-epic-context.md`"
        )));
        let read = paths(&tools);
        assert!(read.contains(&"_bmad/config.toml".to_owned()), "{read:?}");
        assert!(
            read.contains(&format!("{ZONE}/_skills/bmad-build/workflow.md")),
            "{read:?}"
        );
        assert!(read.contains(&format!("{ZONE}/_skills/bmad-build/customize.toml")));

        let session = root.path().join(SESSION_DIR);
        let before = stamps(&session);
        assert_eq!(
            said(call(
                &tools,
                BMAD_RENDER,
                serde_json::json!({"skill": "bmad-build"}),
            )),
            first
        );
        assert_eq!(stamps(&session), before, "the second call writes nothing");
        assert!(!root
            .path()
            .join(SESSIONS)
            .join(".keeper/sessions-journal.json")
            .exists());

        std::fs::write(published.join("step-02-plan.md"), "edited\n").expect("plant");
        assert_eq!(
            reason(call(
                &tools,
                BMAD_RENDER,
                serde_json::json!({"skill": "bmad-build"}),
            )),
            format!(
                "HALT: generation output hash mismatch: {SESSION_DIR}/workspace/bmad-render/bmad-build/{generation}/step-02-plan.md"
            )
        );
    }

    /// 94.2 acceptance 3 on the defective configuration (94.1 #5): the
    /// render halts with BMAD's sentence for the duplicated key, and
    /// nothing is written; without the session's claim, nothing either.
    #[test]
    fn bmad_render_halts_on_the_duplicate_keys() {
        let root = drive(&["bmad-build"]);
        let tools = tools_over(root.path(), None);
        let refused = reason(call(
            &tools,
            BMAD_RENDER,
            serde_json::json!({"skill": "bmad-build"}),
        ));
        assert!(
            refused.starts_with(
                "HALT: ambiguous config value `implementation_artifacts` found at: modules.bmm.implementation_artifacts, modules.gds.implementation_artifacts"
            ),
            "{refused}"
        );
        let workspace = root.path().join(SESSION_DIR).join("workspace");
        assert!(!workspace.exists(), "nothing under workspace/");

        let fixed = drive_with("render/_bmad", &["bmad-build"]);
        assert_eq!(
            reason(call_claimed(
                &tools_over(fixed.path(), None),
                BMAD_RENDER,
                serde_json::json!({"skill": "bmad-build"}),
                false,
            )),
            format!("HALT: {}", crate::sessions::write::NO_CLAIM)
        );
        assert!(!fixed.path().join(SESSION_DIR).join("workspace").exists());
    }

    /// `_load_sources`' escape check: a source that is a link out of the
    /// skill's folder halts the render, unread.
    #[cfg(unix)]
    #[test]
    fn a_render_source_never_leads_out_of_its_skill() {
        let root = drive_with("render/_bmad", &["bmad-build"]);
        let outside = root.path().join("10-notes/secret.md");
        std::fs::create_dir_all(outside.parent().expect("parent")).expect("mkdir");
        std::fs::write(&outside, "secret\n").expect("write");
        std::os::unix::fs::symlink(
            &outside,
            root.path()
                .join(ZONE)
                .join("_skills/bmad-build/references/leak.md"),
        )
        .expect("link");
        let tools = tools_over(root.path(), None);
        assert_eq!(
            reason(call(
                &tools,
                BMAD_RENDER,
                serde_json::json!({"skill": "bmad-build"}),
            )),
            "HALT: render source escapes skill directory: references/leak.md"
        );
        assert!(paths(&tools)
            .iter()
            .all(|file| !file.contains("leak") && !file.starts_with("10-notes")));
        assert!(!root.path().join(SESSION_DIR).join("workspace").exists());
    }

    fn render_call() -> WireToolCall {
        let args = serde_json::json!({"skill": "bmad-build"});
        WireToolCall {
            id: "call-1".to_owned(),
            name: BMAD_RENDER.to_owned(),
            arguments_raw: args.to_string(),
            arguments: Some(args),
        }
    }

    /// R94R-01: whatever stands where a staging folder was once named —
    /// a link into another session, a stale folder holding a file of its
    /// own — is never written through or published: the render stages in
    /// a folder of its own and publishes exactly its outputs.
    #[cfg(unix)]
    #[test]
    fn a_render_stages_only_in_a_folder_of_its_own() {
        let root = drive_with("render/_bmad", &["bmad-build"]);
        let tools = tools_over(root.path(), None);
        let destination = tools.prepare(&render_call()).at;
        let generation = destination
            .rsplit('/')
            .next()
            .expect("a generation")
            .to_owned();
        let skill_dir = root
            .path()
            .join(SESSION_DIR)
            .join("workspace/bmad-render/bmad-build");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        let other = root.path().join(SESSIONS).join("active/2026-10-05-other");
        std::fs::create_dir_all(&other).expect("mkdir");
        std::os::unix::fs::symlink(&other, skill_dir.join(format!(".staging-{generation}")))
            .expect("link");
        let stale = skill_dir.join(format!(".staging-{generation}-old"));
        std::fs::create_dir_all(&stale).expect("mkdir");
        std::fs::write(stale.join("extra.md"), "not this render's\n").expect("plant");

        let said = said(call(
            &tools,
            BMAD_RENDER,
            serde_json::json!({"skill": "bmad-build"}),
        ));
        assert_eq!(said, format!("read and follow {destination}/workflow.md"));
        assert_eq!(
            std::fs::read_dir(&other).expect("other").count(),
            0,
            "nothing reached the other session"
        );
        let published = root.path().join(&destination);
        let found = crate::sessions::exec::regular_files(&published).expect("a real tree");
        let manifest: Json = serde_json::from_slice(&found["manifest.json"]).expect("manifest");
        let outputs = manifest["outputs"].as_object().expect("outputs");
        assert_eq!(found.len(), outputs.len() + 1, "exactly the outputs");
        assert!(!found.contains_key("extra.md"));
    }

    /// R94R-02: an output that was empty, replaced by a link to other
    /// bytes, is not a generation keeper verifies: the render halts, and
    /// the link is never followed.
    #[cfg(unix)]
    #[test]
    fn a_link_in_a_generation_is_never_verified() {
        let root = drive_with("render/_bmad", &["bmad-build"]);
        std::fs::write(
            root.path()
                .join(ZONE)
                .join("_skills/bmad-build/references/empty.md"),
            "",
        )
        .expect("an empty source");
        let tools = tools_over(root.path(), None);
        let first = said(call(
            &tools,
            BMAD_RENDER,
            serde_json::json!({"skill": "bmad-build"}),
        ));
        let destination = first
            .strip_prefix("read and follow ")
            .and_then(|entry| entry.strip_suffix("/workflow.md"))
            .expect("published")
            .to_owned();
        let empty = root.path().join(&destination).join("references/empty.md");
        assert_eq!(std::fs::read(&empty).expect("empty"), b"");
        let elsewhere = root.path().join("10-notes/secret.md");
        std::fs::create_dir_all(elsewhere.parent().expect("parent")).expect("mkdir");
        std::fs::write(&elsewhere, "secret\n").expect("write");
        std::fs::remove_file(&empty).expect("rm");
        std::os::unix::fs::symlink(&elsewhere, &empty).expect("link");
        assert_eq!(
            reason(call(
                &tools,
                BMAD_RENDER,
                serde_json::json!({"skill": "bmad-build"})
            )),
            format!(
                "HALT: corrupt existing generation {destination}: references/empty.md is a link"
            )
        );
    }

    /// R94R-06: a source folder that cannot be listed halts the render,
    /// naming it; nothing is published from the sources that could be.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_source_folder_halts_the_render() {
        use std::os::unix::fs::PermissionsExt;
        let root = drive_with("render/_bmad", &["bmad-build"]);
        let references = root.path().join(ZONE).join("_skills/bmad-build/references");
        std::fs::set_permissions(&references, std::fs::Permissions::from_mode(0o000))
            .expect("chmod");
        let refused = reason(call(
            &tools_over(root.path(), None),
            BMAD_RENDER,
            serde_json::json!({"skill": "bmad-build"}),
        ));
        std::fs::set_permissions(&references, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        assert!(
            refused.starts_with(&format!(
                "HALT: render source folder {ZONE}/_skills/bmad-build/references/ could not be read: "
            )),
            "{refused}"
        );
        assert!(!root.path().join(SESSION_DIR).join("workspace").exists());
    }

    /// R94R-07: what a render's admission binds is its generation — the
    /// folder and the manifest's SHA-256 it publishes — so a source changed
    /// between two preparations is another effect, and an unchanged one the
    /// same.
    #[test]
    fn a_renders_effect_is_its_generation() {
        let root = drive_with("render/_bmad", &["bmad-build"]);
        let tools = tools_over(root.path(), None);
        let first = tools.prepare(&render_call());
        assert_eq!(first.effect, tools.prepare(&render_call()).effect);
        let effect: Json = serde_json::from_slice(&first.effect).expect("JSON");
        assert_eq!(effect["path"], first.at);
        assert_eq!(effect["drive"], "tgdrive");
        std::fs::write(
            root.path()
                .join(ZONE)
                .join("_skills/bmad-build/references/claims-check.md"),
            "changed while it waited\n",
        )
        .expect("edit");
        let again = tools.prepare(&render_call());
        assert_ne!(again.effect, first.effect);
        assert_ne!(again.at, first.at);
    }

    /// The memlog of `run` in this session, on the disk.
    fn memlog_at(root: &Path, run: &str) -> PathBuf {
        root.join(SESSION_DIR).join(run).join(".memlog.md")
    }

    /// R94R-04: a memlog that is there but cannot be read — bytes that are
    /// not UTF-8, a file this host may not read — is never taken for
    /// absent: `init` refuses and the bytes stay.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_memlog_is_never_started_over() {
        use std::os::unix::fs::PermissionsExt;
        let root = drive(&[]);
        let tools = tools_over(root.path(), None);
        let memlog = memlog_at(root.path(), "artifacts/run");
        std::fs::create_dir_all(memlog.parent().expect("parent")).expect("mkdir");
        let init = || {
            reason(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "init", "workspace": "artifacts/run"}),
            ))
        };
        std::fs::write(&memlog, b"---\ntopic: \xff\n---\n").expect("plant");
        assert!(
            init().starts_with("artifacts/run/.memlog.md is there but could not be read"),
            "{}",
            init()
        );
        assert_eq!(
            std::fs::read(&memlog).expect("kept"),
            b"---\ntopic: \xff\n---\n"
        );

        std::fs::write(&memlog, "---\ntopic: T\n---\n").expect("plant");
        std::fs::set_permissions(&memlog, std::fs::Permissions::from_mode(0o200)).expect("chmod");
        let refused = init();
        std::fs::set_permissions(&memlog, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(
            refused.contains("is there but could not be read"),
            "{refused}"
        );
        assert_eq!(
            std::fs::read_to_string(&memlog).expect("kept"),
            "---\ntopic: T\n---\n"
        );
    }

    /// 94.2 acceptance 4: `bmad_memlog` is `memlog.py` inside the session —
    /// its three commands on a run's `.memlog.md` under `artifacts/`, its
    /// ack — and writes nothing else: another dotted name, or a memlog
    /// anywhere but `artifacts/`, is the Hidden refusal; a link out of the
    /// session, or a host without the session's claim, writes nothing.
    #[test]
    fn bmad_memlog_writes_only_the_memlog() {
        let root = drive(&[]);
        let tools = tools_over(root.path(), None);
        let memlog = memlog_at(root.path(), "artifacts/run-1");
        assert_eq!(
            said(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "init", "workspace": "artifacts/run-1", "fields": ["topic=Lunchbox", "goal = pitch"]}),
            )),
            "{\"ok\": true, \"memlog\": \"artifacts/run-1/.memlog.md\", \"entries\": 0}"
        );
        assert_eq!(
            std::fs::read_to_string(&memlog).expect("memlog"),
            format!("---\ntopic: Lunchbox\ngoal: pitch\nupdated: {NOW}\n---\n\n\n")
        );
        // Drive-relative, as bmad_config's write locations name it.
        assert_eq!(
            said(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "append", "path": format!("{SESSION_DIR}/artifacts/run-1/.memlog.md"), "text": "a  bento\nbox", "type": "idea", "by": "user"}),
            )),
            format!("{{\"ok\": true, \"memlog\": \"{SESSION_DIR}/artifacts/run-1/.memlog.md\", \"entries\": 1}}")
        );
        assert_eq!(
            said(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "set", "workspace": "artifacts/run-1", "key": "topic", "value": "Bento"}),
            )),
            "{\"ok\": true, \"memlog\": \"artifacts/run-1/.memlog.md\", \"entries\": 1}"
        );
        let written = std::fs::read_to_string(&memlog).expect("memlog");
        assert_eq!(
            written,
            format!("---\ntopic: Bento\ngoal: pitch\nupdated: {NOW}\n---\n\n- (idea by user) a bento box\n")
        );
        assert_eq!(
            reason(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "init", "workspace": "artifacts/run-1"}),
            )),
            "error: artifacts/run-1/.memlog.md already exists; use append/set to update it"
        );
        assert_eq!(
            reason(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "append", "workspace": "artifacts/run-2", "text": "x"}),
            )),
            "artifacts/run-2/.memlog.md is not there yet; bmad_memlog's init starts it."
        );

        let session = root.path().join(SESSION_DIR);
        let before = stamps(&session);
        for (target, rel) in [
            ("path", "artifacts/run-1/.other.md"),
            ("path", "notes/.memlog.md"),
            ("workspace", "notes"),
            ("workspace", "."),
            ("workspace", "workspace/run"),
        ] {
            let refused = reason(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "init", target: rel}),
            ));
            assert!(
                refused.contains("is dotted, and keeper's markdown scan never reads a dotted name"),
                "{rel}: {refused}"
            );
        }
        assert!(reason(call(
            &tools,
            BMAD_MEMLOG,
            serde_json::json!({"command": "init", "path": "artifacts/run-1/notes.md"}),
        ))
        .contains("is not a memlog"));
        assert!(reason(call(
            &tools,
            BMAD_MEMLOG,
            serde_json::json!({"command": "init", "workspace": "artifacts/../../other"}),
        ))
        .contains("is not a path inside this session"));
        assert_eq!(
            reason(call_claimed(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "append", "workspace": "artifacts/run-1", "text": "lost"}),
                false,
            )),
            crate::sessions::write::NO_CLAIM
        );
        assert_eq!(stamps(&session), before, "no refusal wrote anything");
        assert_eq!(std::fs::read_to_string(&memlog).expect("memlog"), written);

        // A run folder that is a link to another session's is not this
        // session's.
        #[cfg(unix)]
        {
            let other = root
                .path()
                .join(SESSIONS)
                .join("active/2026-10-05-other/artifacts");
            std::fs::create_dir_all(&other).expect("mkdir");
            std::os::unix::fs::symlink(&other, session.join("artifacts/elsewhere")).expect("link");
            assert!(reason(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "init", "workspace": "artifacts/elsewhere"}),
            ))
            .contains("does not stay inside this session"));
            assert!(!other.join(".memlog.md").exists());
        }
    }

    /// 94.2 acceptance 4, the crash: a host that dies between the temp
    /// file and the rename leaves the old memlog whole, and the next host
    /// to hold the zone finishes the journaled write — the new memlog,
    /// whole, no temp file left. One that dies after the rename finishes
    /// with the same file. No kill harness exists: the journal the write
    /// left is planted as the executor writes it (codemap §4).
    #[test]
    fn bmad_memlog_survives_a_crash_mid_write() {
        let root = drive(&[]);
        let tools = tools_over(root.path(), None);
        said(call(
            &tools,
            BMAD_MEMLOG,
            serde_json::json!({"command": "init", "workspace": "artifacts/run", "fields": ["topic=T"]}),
        ));
        let zone = root.path().join(SESSIONS);
        let memlog = memlog_at(root.path(), "artifacts/run");
        let old = std::fs::read_to_string(&memlog).expect("memlog");
        let new = memlog::append(&old, "after the crash", None, None, NOW)
            .expect("append")
            .text;
        let plan = keeper_core::sessions::files::compile_memlog(
            SESSION,
            "artifacts/run/.memlog.md",
            Some(&old),
            &new,
        )
        .expect("plan");
        let journal = zone.join(".keeper/sessions-journal.json");
        let crash = |done: usize| {
            std::fs::create_dir_all(journal.parent().expect("parent")).expect("mkdir");
            std::fs::write(
                &journal,
                serde_json::json!({"plan": plan, "done": done}).to_string(),
            )
            .expect("journal");
        };
        let torn = memlog.with_file_name("..memlog.md.keeper-tmp");

        // Killed after the temp file's first bytes, before the rename.
        crash(1);
        std::fs::write(&torn, &new[..new.len() / 2]).expect("torn");
        assert_eq!(std::fs::read_to_string(&memlog).expect("old"), old);
        crate::sessions::exec::resume(&zone).expect("resumed");
        assert_eq!(std::fs::read_to_string(&memlog).expect("new"), new);
        assert!(!torn.exists(), "no temp file left");
        assert!(!journal.exists(), "the journal is cleared");

        // Killed after the rename, before the journal said so.
        crash(1);
        crate::sessions::exec::resume(&zone).expect("resumed");
        assert_eq!(std::fs::read_to_string(&memlog).expect("new"), new);
        assert!(!journal.exists());

        // The memlog reads whole, and the tool goes on from it.
        assert_eq!(
            said(call(
                &tools,
                BMAD_MEMLOG,
                serde_json::json!({"command": "append", "workspace": "artifacts/run", "text": "next"}),
            )),
            "{\"ok\": true, \"memlog\": \"artifacts/run/.memlog.md\", \"entries\": 2}"
        );
    }
}

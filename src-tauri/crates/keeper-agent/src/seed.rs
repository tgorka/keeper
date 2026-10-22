//! The agents zone written to disk and a new agent from the template (story
//! 91.5; AD-361, AD-362). A proxy's DM and a gate's room — sessions made
//! with an agent's Matrix room — left keeper in 108.2.
//!
//! `keeper_core::agents::seed` decides what the files are; this writes them
//! with create-new semantics, so nothing is ever overwritten: a file that is
//! there when the plan is made is left, and so is one that appears between
//! the plan and the write. A folder on the way that is a link, or not a
//! folder, refuses the write rather than following it out of the drive.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use keeper_core::agents::agentd::DrivePin;
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::seed::{
    self, AgentSeedFolderVm, AgentSeedOfferVm, AgentSeedPlanVm, AgentSeedReq, AgentSeedResultVm,
    SeedChoices, SeedFile, SeedPlan, TEMPLATE_DIR,
};
use keeper_sync::{browse, SyncProfile};

use crate::zone::read_text;

/// What a seed wrote and what it left, zone-relative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub written: Vec<String>,
    pub left: Vec<String>,
}

/// Whether anything — a file, a folder, a link — is at `rel` under `zone`.
pub fn exists_under(zone: &Path) -> impl Fn(&str) -> bool + '_ {
    move |rel| std::fs::symlink_metadata(zone.join(rel)).is_ok()
}

/// `choices` against the zone at `zone` as it is now.
pub fn plan_at(choices: &SeedChoices, zone: &Path) -> SeedPlan {
    seed::plan(choices, exists_under(zone))
}

/// `dir` as a real folder: made when it is not there, refused when it is a
/// link or a file.
fn real_dir(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "{} is not a folder keeper may write into, so nothing was written there.",
            dir.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                // Made meanwhile by someone else: fine when it is a folder.
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => real_dir(dir),
                Err(error) => Err(format!("{} could not be made: {error}", dir.display())),
            }
        }
        Err(error) => Err(format!("{} cannot be read: {error}", dir.display())),
    }
}

/// Write `plan` under `zone`, each file only if nothing is there. `zone`'s
/// parent must exist; the zone folder is made when it is not there.
pub fn apply(plan: SeedPlan, zone: &Path) -> Result<Applied, String> {
    real_dir(zone)?;
    let mut applied = Applied {
        written: Vec::new(),
        left: plan.left,
    };
    for SeedFile { path, text } in plan.write {
        write_new(zone, path, text.as_bytes(), &mut applied)?;
    }
    Ok(applied)
}

/// The real folders on the way to zone-relative `rel`, made as needed, and
/// the path they lead to.
fn real_dirs(zone: &Path, dirs: &[&str]) -> Result<PathBuf, String> {
    let mut at: PathBuf = zone.to_owned();
    for dir in dirs {
        at.push(dir);
        real_dir(&at)?;
    }
    Ok(at)
}

/// One file at zone-relative `path`, only if nothing is there: named in
/// `applied` as written or left.
fn write_new(zone: &Path, path: String, bytes: &[u8], applied: &mut Applied) -> Result<(), String> {
    let parts: Vec<&str> = path.split('/').collect();
    let Some((name, dirs)) = parts.split_last() else {
        return Ok(());
    };
    let at = real_dirs(zone, dirs)?.join(name);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&at)
    {
        Ok(mut file) => {
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("{path} could not be written: {error}"))?;
            applied.written.push(path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            applied.left.push(path);
        }
        Err(error) => return Err(format!("{path} could not be written: {error}")),
    }
    Ok(())
}

/// The zone's `_drive.toml`, when there is one: `None` when the file is not
/// there, the sentence when it does not read.
pub fn declared(zone: &Path) -> Option<Result<DriveDecl, String>> {
    match read_text(zone, drive::FILE_NAME) {
        Ok(None) => None,
        Ok(Some(text)) => Some(drive::parse(&text).map_err(|refusal| refusal.sentence())),
        Err(sentence) => Some(Err(sentence)),
    }
}

/// The declaration the zone will host under: its `_drive.toml` when it has
/// one, else the flags'. A seed never leaves a `_drive.toml` that says
/// something else than what the seeded agents were checked against, so the
/// flags must state the file's id, owner, readers and `local_only`.
pub fn check_declared(choices: &SeedChoices, zone: &Path) -> Result<DriveDecl, String> {
    match declared(zone) {
        None => Ok(choices.decl.clone()),
        Some(Err(sentence)) => Err(format!(
            "This zone's _drive.toml does not read, and a seed never replaces it: {sentence}"
        )),
        Some(Ok(decl)) => {
            let ours = &choices.decl;
            if decl.id == ours.id
                && decl.owner == ours.owner
                && decl.readers == ours.readers
                && decl.local_only == ours.local_only
            {
                Ok(decl)
            } else {
                Err(format!(
                    "This zone's _drive.toml already names {} with the owner {}, the readers {} and local_only = {}; a seed leaves it, so name the same.",
                    decl.id,
                    decl.owner,
                    decl.readers
                        .iter()
                        .map(|r| r.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    decl.local_only
                ))
            }
        }
    }
}

/// `_template/` as `agents new` copies it, relative to it: text files whose
/// tokens are expanded, other files copied byte for byte, and every folder,
/// so an empty one is made too. A link is refused, never followed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Template {
    pub text: Vec<(String, String)>,
    pub bytes: Vec<(String, Vec<u8>)>,
    pub folders: Vec<String>,
}

/// `_template/`, read through `browse::resolve`.
pub fn read_template(zone: &Path) -> Result<Template, String> {
    fn walk(zone: &Path, rel: &str, found: &mut Template) -> Result<(), String> {
        let dir = zone.join(rel);
        let mut names: Vec<(String, std::fs::FileType)> = std::fs::read_dir(&dir)
            .map_err(|error| format!("{rel} cannot be read: {error}"))?
            .map(|entry| {
                let entry = entry.map_err(|error| format!("{rel} cannot be read: {error}"))?;
                let kind = entry
                    .file_type()
                    .map_err(|error| format!("{rel} cannot be read: {error}"))?;
                Ok((entry.file_name().to_string_lossy().into_owned(), kind))
            })
            .collect::<Result<_, String>>()?;
        names.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind) in names {
            let child = format!("{rel}/{name}");
            let inner = child
                .strip_prefix(&format!("{TEMPLATE_DIR}/"))
                .unwrap_or(&child)
                .to_owned();
            if kind.is_symlink() {
                return Err(format!(
                    "{child} is a link, and agents new copies files and folders only: put what it points at there instead."
                ));
            }
            if kind.is_dir() {
                found.folders.push(inner);
                walk(zone, &child, found)?;
                continue;
            }
            let path = match browse::resolve(zone, &child) {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(refusal) => return Err(format!("{child} is refused: {refusal}")),
            };
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("{child} could not be read: {error}"))?;
            match String::from_utf8(bytes) {
                Ok(text) => found.text.push((inner, text)),
                Err(not_text) => found.bytes.push((inner, not_text.into_bytes())),
            }
        }
        Ok(())
    }
    match std::fs::symlink_metadata(zone.join(TEMPLATE_DIR)) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "{TEMPLATE_DIR} is not a folder, and agents new copies a folder: put the template there."
            ))
        }
        Err(_) => {
            return Err(format!(
                "This zone has no {TEMPLATE_DIR}/ to copy: run `keeper-agentd agents init` first."
            ))
        }
    }
    let mut found = Template::default();
    walk(zone, TEMPLATE_DIR, &mut found)?;
    Ok(found)
}

/// A new agent `id` from the zone's template; refused when its folder is
/// already there, whatever it holds.
pub fn new_agent(
    zone: &Path,
    id: &str,
    name: &str,
    date: &str,
    soul: Option<&str>,
) -> Result<Applied, String> {
    if keeper_core::agents::zone::is_zone_own(id) || !is_agent_id(id) {
        return Err(format!(
            "\"{id}\" is not an agent id: a lowercase letter, then up to 31 lowercase letters, digits or hyphens."
        ));
    }
    seed::check_name(name)?;
    if std::fs::symlink_metadata(zone.join(id)).is_ok() {
        return Err(format!(
            "{id}/ is already in the zone; a new agent never takes an existing folder."
        ));
    }
    let template = read_template(zone)?;
    let mut applied = apply(
        SeedPlan {
            write: seed::from_template(&template.text, id, name, date, soul),
            left: Vec::new(),
        },
        zone,
    )?;
    for (rel, bytes) in &template.bytes {
        write_new(zone, format!("{id}/{rel}"), bytes, &mut applied)?;
    }
    for rel in &template.folders {
        let path = format!("{id}/{rel}");
        let made = !zone.join(&path).exists();
        real_dirs(zone, &path.split('/').collect::<Vec<_>>())?;
        if made {
            applied.written.push(format!("{path}/"));
        }
    }
    Ok(applied)
}

fn is_agent_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && id.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ---------------------------------------------------------------------------
// Settings › Agents › Set up agents
// ---------------------------------------------------------------------------

/// What *Set up agents* refuses on a Mac with no organisation login and a
/// zone with no `_drive.toml` to take the principal from.
pub const NO_PRINCIPAL: &str = "A new zone's principal is your organisation account's login. Sign in under Account, then set the agents up.";

/// What *Set up agents* says for a zone with no `_drive.toml` when no
/// Matrix account is signed in: there is no one to name as owner.
pub const NO_ACCOUNT: &str =
    "Sign in to a Matrix account first: the owner and readers are Matrix ids.";

/// The folders that keep agents, with what the form starts from: the
/// zone's `_drive.toml` when it has one, else the first signed-in account
/// and this Mac's pin for the folder (`pins`, by profile id). `bots` is the
/// person's bots, or the sentence a person reads where they would be when
/// they could not be read; those that run locally are offered apart, for a
/// `local_only` folder (R405).
pub fn offer(
    profiles: &[SyncProfile],
    pins: &BTreeMap<String, DrivePin>,
    accounts: &[String],
    login: Option<&str>,
    bots: Result<Vec<keeper_core::agents::seed::AgentSeedBotVm>, String>,
) -> AgentSeedOfferVm {
    let first = accounts.first().cloned().unwrap_or_default();
    let folders = profiles
        .iter()
        .filter(|profile| profile.agents.is_some())
        .filter_map(|profile| {
            let zone = profile.agents_root()?;
            let drive = drive_id_of(&profile.name);
            let base = AgentSeedFolderVm {
                profile_id: profile.id.clone(),
                name: profile.name.clone(),
                preselected: seed::preselected(&drive),
                drive,
                owner: first.clone(),
                readers: if first.is_empty() {
                    Vec::new()
                } else {
                    vec![first.clone()]
                },
                local_only: pins.get(&profile.id).is_some_and(|pin| pin.local_only),
                declared: false,
                problem: None,
            };
            Some(match declared(&zone) {
                Some(Ok(decl)) => AgentSeedFolderVm {
                    preselected: seed::preselected(&decl.id),
                    drive: decl.id.clone(),
                    owner: decl.owner.to_string(),
                    readers: decl.readers.iter().map(|r| r.to_string()).collect(),
                    local_only: decl.local_only,
                    declared: true,
                    ..base
                },
                Some(Err(sentence)) => AgentSeedFolderVm {
                    problem: Some(sentence),
                    ..base
                },
                None if login.is_none() => AgentSeedFolderVm {
                    problem: Some(NO_PRINCIPAL.to_owned()),
                    ..base
                },
                None if accounts.is_empty() => AgentSeedFolderVm {
                    problem: Some(NO_ACCOUNT.to_owned()),
                    ..base
                },
                None => base,
            })
        })
        .collect();
    let (bots, bots_unread) = match bots {
        Ok(bots) => (bots, None),
        Err(sentence) => (Vec::new(), Some(sentence)),
    };
    AgentSeedOfferVm {
        folders,
        catalogue: seed::catalogue_vm(),
        local_bots: seed::local_choices(&bots),
        bots,
        bots_unread,
        accounts: accounts.to_vec(),
    }
}

/// A folder's name as a drive id when it is one, else empty for the person
/// to fill in.
fn drive_id_of(name: &str) -> String {
    let id = name.trim().to_ascii_lowercase();
    let fits = id
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if fits {
        id
    } else {
        String::new()
    }
}

/// One *Set up agents* request, checked against the folder, its
/// `_drive.toml` and this Mac's pin: the choices and the zone they go to.
pub fn desktop_choices(
    profiles: &[SyncProfile],
    pin: Option<&DrivePin>,
    login: Option<&str>,
    req: &AgentSeedReq,
) -> Result<(SeedChoices, PathBuf), String> {
    let profile = profiles
        .iter()
        .find(|p| p.id == req.profile_id && p.agents.is_some())
        .ok_or_else(|| "That folder does not keep agents.".to_owned())?;
    let zone = profile
        .agents_root()
        .ok_or_else(|| "That folder does not keep agents.".to_owned())?;
    let principal = match declared(&zone) {
        Some(Ok(decl)) => decl.principal,
        Some(Err(sentence)) => return Err(sentence),
        None => login.ok_or_else(|| NO_PRINCIPAL.to_owned())?.to_owned(),
    };
    let choices = SeedChoices::new(
        &req.drive,
        &principal,
        &req.owner,
        &req.readers,
        req.local_only,
        req.bot.as_deref(),
        &req.with,
    )?
    .with_models(req.effort, &req.models)?;
    let hosting = check_declared(&choices, &zone)?;
    choices.check_hosting(&hosting, pin)?;
    Ok((choices, zone))
}

/// The plan as the form shows it.
pub fn plan_vm(plan: &SeedPlan) -> AgentSeedPlanVm {
    AgentSeedPlanVm {
        write: plan.write.iter().map(|file| file.path.clone()).collect(),
        left: plan.left.clone(),
    }
}

/// What a seed did, and the agents whose `agent.toml` it wrote.
pub fn result_vm(profile_id: &str, choices: &SeedChoices, applied: Applied) -> AgentSeedResultVm {
    let agents = choices
        .with
        .iter()
        .filter(|agent| {
            applied
                .written
                .iter()
                .any(|path| *path == format!("{}/agent.toml", agent.id))
        })
        .map(|agent| agent.id.to_owned())
        .collect();
    AgentSeedResultVm {
        profile_id: profile_id.to_owned(),
        written: applied.written,
        left: applied.left,
        agents,
    }
}

#[cfg(test)]
mod tests;

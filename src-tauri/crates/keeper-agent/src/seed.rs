//! The agents zone written to disk, a new agent from the template, and a
//! proxy's DM (story 91.5; AD-361, AD-362, AD-372).
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
/// and this Mac's pin for the folder (`pins`, by profile id).
pub fn offer(
    profiles: &[SyncProfile],
    pins: &BTreeMap<String, DrivePin>,
    accounts: &[String],
    login: Option<&str>,
    bots: Vec<keeper_core::agents::seed::AgentSeedBotVm>,
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
    AgentSeedOfferVm {
        folders,
        catalogue: seed::catalogue_vm(),
        bots,
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
    )?;
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

// ---------------------------------------------------------------------------
// The proxy's DM
// ---------------------------------------------------------------------------

#[cfg(unix)]
pub use dm::{main_dm, main_session, Made, MainDm};

#[cfg(unix)]
mod dm {
    use std::collections::BTreeSet;
    use std::path::Path;

    use keeper_core::agents::drive::DriveDecl;
    use keeper_core::agents::events::{
        RunState, StatusContent, CONTENT_VERSION, SESSION_ROOM_TYPE, STATUS,
    };
    use keeper_core::agents::home::AgentConfig;
    use keeper_core::agents::label::{Integrity, Label, Readers};
    use keeper_core::agents::matrix::{AgentClient, RoomKind};
    use keeper_core::agents::seed::main_session_id;
    use keeper_core::agents::session::{self, SessionAgent, SessionKind};
    use matrix_sdk::room::MessagesOptions;
    use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UInt, UserId};
    use matrix_sdk::{Room, RoomMemberships};
    use serde_json::Value;

    use crate::runtime::{BACKLOG_PAGE, BACKLOG_PAGES};
    use crate::sessions::verbs::{self, CreateOutcome};

    /// The title of a proxy's `main` session.
    pub const MAIN_TITLE: &str = "main";

    /// What one call of [`main_dm`] made.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Made {
        /// The room and the `main` session folder naming it.
        RoomAndFolder,
        /// The folder only: the copy was already in the person's DM.
        Folder,
        /// Nothing: the folder was there.
        Nothing,
    }

    /// A proxy's DM and its `main` session.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MainDm {
        pub room: OwnedRoomId,
        /// Zone-relative: `active/<folder>`.
        pub path: String,
        pub made: Made,
    }

    /// The `main` session of `proxy` in `room`. Its label's readers are the
    /// person alone, not the drive's: the room holds the proxy and its
    /// person, so what is said there is theirs (AD-390 lets a label narrow
    /// at its opening).
    pub fn main_session(
        proxy: &AgentConfig,
        decl: &DriveDecl,
        human: &UserId,
        room: &RoomId,
        now: chrono::DateTime<chrono::Local>,
    ) -> SessionAgent {
        let mut label = Label::opening(decl, Integrity::Owner);
        label.readers =
            Readers::Only([human.to_owned()].into_iter().collect()).meet(&label.readers);
        SessionAgent {
            id: main_session_id(&proxy.drive, &proxy.id),
            agent: proxy.id.clone(),
            drive: proxy.drive.clone(),
            kind: SessionKind::Main,
            title: MAIN_TITLE.to_owned(),
            requested_by: human.to_owned(),
            parent: None,
            room: room.to_owned(),
            drives: vec![proxy.drive.clone()],
            label,
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: vec![human.to_owned(), proxy.matrix_user.clone()],
            limits: None,
            workflow: None,
            created_at: now.with_timezone(&chrono::Utc),
        }
    }

    /// Make `proxy`'s DM with its `human` once (AD-372): a `main` session
    /// room (`is_direct`, typed `dev.keeper.agent.session`, the person
    /// invited and allowed to talk), the `main` session folder naming it
    /// under the id [`main_session_id`] derives, and its status anchor saying
    /// `kind: "main"`.
    ///
    /// Once means once on both sides. The folder is looked for first; with
    /// none, a room the copy is already in that is this DM (see
    /// [`is_main_dm`]) is adopted rather than a second one made — a checkout
    /// made again before `run` pushed the folder, or a run stopped between
    /// the room and the folder. A folder that appears while the room is
    /// being made wins: its room is the DM, and the room just made is left
    /// and forgotten, the person's invite revoked, as it is when the folder
    /// cannot be written. The anchor goes only into the room the folder
    /// names, and only when it has none.
    ///
    /// No claim is taken: a session with no claim event is acquirable
    /// (AD-378), so placement chooses the holder once a host serves it.
    pub async fn main_dm(
        client: &AgentClient,
        proxy: &AgentConfig,
        decl: &DriveDecl,
        sessions: &Path,
        sessions_subfolder: &str,
        host: &str,
        now: chrono::DateTime<chrono::Local>,
    ) -> Result<MainDm, String> {
        let human = proxy
            .human
            .clone()
            .ok_or_else(|| format!("{} is not a proxy: it has no human.", proxy.id))?;
        let me = client
            .user_id()
            .ok_or_else(|| "the copy is not signed in".to_owned())?
            .to_owned();
        let id = main_session_id(&proxy.drive, &proxy.id);
        client
            .sync_once()
            .await
            .map_err(|error| error.to_string())?;
        if let Some((path, room)) = existing(sessions, &id.to_string())? {
            ensure_anchor(client, &me, proxy, &room, sessions_subfolder, &path, host).await?;
            return Ok(MainDm {
                room,
                path,
                made: Made::Nothing,
            });
        }

        let (room, made_room) = match find_main_dm(client, &me, &human).await {
            Some(room) => (room, false),
            None => (
                client
                    .create_room(
                        RoomKind::Session(SessionKind::Main),
                        &proxy.name,
                        vec![human.clone()],
                        &[],
                    )
                    .await
                    .map_err(|error| error.to_string())?,
                true,
            ),
        };
        let agent = main_session(proxy, decl, &human, &room, now);
        let folder = sessions.to_owned();
        let settled =
            tokio::task::spawn_blocking(move || make_folder(&folder, &agent, made_room, now))
                .await
                .map_err(|error| error.to_string())
                .and_then(|settled| settled);
        let settled = match settled {
            Ok(settled) => settled,
            Err(sentence) => {
                if made_room {
                    discard(client, &room, &human).await;
                }
                return Err(sentence);
            }
        };
        if let Some(orphan) = &settled.discard {
            discard(client, orphan, &human).await;
        }
        let made = match (made_room, settled.folder_made) {
            (true, true) => Made::RoomAndFolder,
            (false, true) => Made::Folder,
            (_, false) => Made::Nothing,
        };
        let Settled { room, path, .. } = settled;
        ensure_anchor(client, &me, proxy, &room, sessions_subfolder, &path, host).await?;
        Ok(MainDm { room, path, made })
    }

    /// Where [`make_folder`] leaves the DM.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Settled {
        /// The room the folder names: the DM.
        pub room: OwnedRoomId,
        pub path: String,
        /// Whether this call wrote the folder.
        pub folder_made: bool,
        /// A room this call made that no folder names, to leave.
        pub discard: Option<OwnedRoomId>,
    }

    /// The `main` session folder for `agent`, made under the zone's lock
    /// unless a session with its id is there by then; `made_room` says
    /// whether `agent.room` was made by this call (and so is discarded when
    /// another folder won).
    pub(crate) fn make_folder(
        sessions: &Path,
        agent: &SessionAgent,
        made_room: bool,
        now: chrono::DateTime<chrono::Local>,
    ) -> Result<Settled, String> {
        std::fs::create_dir_all(sessions)
            .map_err(|error| format!("{} could not be made: {error}", sessions.display()))?;
        match verbs::create_agent_session(sessions, agent, now).map_err(|e| e.to_string())? {
            CreateOutcome::Created { path, .. } => Ok(Settled {
                room: agent.room.clone(),
                path,
                folder_made: true,
                discard: None,
            }),
            CreateOutcome::Existed { .. } => {
                let (path, room) = existing(sessions, &agent.id.to_string())?.ok_or_else(|| {
                    format!("{}'s main session vanished while it was read.", agent.agent)
                })?;
                let discard = (made_room && room != agent.room).then(|| agent.room.clone());
                Ok(Settled {
                    room,
                    path,
                    folder_made: false,
                    discard,
                })
            }
        }
    }

    /// Whether a room the copy `me` is in is its `main` DM with `human`: a
    /// session room whose members (joined or invited) are the two of them,
    /// whose newest status from `me` says `kind: "main"` — or, with no
    /// status yet, that `me` marked direct with `human`, which only a `main`
    /// room is made.
    pub(crate) fn is_main_dm(
        room_type: Option<&str>,
        members: &BTreeSet<OwnedUserId>,
        me: &UserId,
        human: &UserId,
        direct_to_human: bool,
        status_kind: Option<&str>,
    ) -> bool {
        room_type == Some(SESSION_ROOM_TYPE)
            && members.len() == 2
            && members.iter().any(|member| member == me)
            && members.iter().any(|member| member == human)
            && match status_kind {
                Some(kind) => kind == SessionKind::Main.to_string(),
                None => direct_to_human,
            }
    }

    /// The copy's `main` DM with `human` among the rooms it is in, the
    /// first by room id when there is more than one.
    async fn find_main_dm(
        client: &AgentClient,
        me: &UserId,
        human: &UserId,
    ) -> Option<OwnedRoomId> {
        let mut found = Vec::new();
        for room in client.client().joined_rooms() {
            let room_type = room.room_type().map(|kind| kind.to_string());
            if room_type.as_deref() != Some(SESSION_ROOM_TYPE) {
                continue;
            }
            let Ok(members) = room
                .members(RoomMemberships::JOIN | RoomMemberships::INVITE)
                .await
            else {
                continue;
            };
            let members: BTreeSet<OwnedUserId> =
                members.iter().map(|m| m.user_id().to_owned()).collect();
            let direct = room.is_direct().await.unwrap_or(false)
                && room
                    .direct_targets()
                    .iter()
                    .any(|target| target.as_user_id() == Some(human));
            let kind = status_kind(&room, me).await;
            if is_main_dm(
                room_type.as_deref(),
                &members,
                me,
                human,
                direct,
                kind.as_deref(),
            ) {
                found.push(room.room_id().to_owned());
            }
        }
        found.sort();
        found.into_iter().next()
    }

    /// The `kind` of the newest status `me` sent in `room`.
    async fn status_kind(room: &Room, me: &UserId) -> Option<String> {
        let mut from: Option<String> = None;
        for _ in 0..BACKLOG_PAGES {
            let mut options = MessagesOptions::backward().from(from.as_deref());
            options.limit = UInt::from(BACKLOG_PAGE);
            let page = room.messages(options).await.ok()?;
            let newest = page.chunk.iter().find_map(|event| {
                let value: Value = event.raw().deserialize_as().ok()?;
                (value["type"] == STATUS && value["sender"] == me.as_str())
                    .then(|| value["content"]["kind"].as_str().map(str::to_owned))
                    .flatten()
            });
            if newest.is_some() {
                return newest;
            }
            match page.end {
                Some(end) if !page.chunk.is_empty() => from = Some(end),
                _ => return None,
            }
        }
        None
    }

    /// Leave and forget a room no folder names, first revoking the person's
    /// invite so they are not left one to a room nobody serves. Each step is
    /// tried and logged; none of them undoes the DM.
    async fn discard(client: &AgentClient, room: &RoomId, human: &UserId) {
        let Some(joined) = client.client().get_room(room) else {
            return;
        };
        if let Err(error) = joined
            .kick_user(
                human,
                Some("This room was made twice; the DM is the other one."),
            )
            .await
        {
            tracing::warn!(%room, %error, "seed: the invite to a discarded DM could not be revoked");
        }
        if let Err(error) = joined.leave().await {
            tracing::warn!(%room, %error, "seed: a discarded DM could not be left");
            return;
        }
        if let Err(error) = joined.forget().await {
            tracing::warn!(%room, %error, "seed: a discarded DM could not be forgotten");
        }
    }

    /// The `main` session with `id` and the room its `agent.toml` names.
    fn existing(sessions: &Path, id: &str) -> Result<Option<(String, OwnedRoomId)>, String> {
        if !sessions.is_dir() {
            return Ok(None);
        }
        let Some(row) = verbs::find(sessions, id) else {
            return Ok(None);
        };
        let rel = format!("{}/{}", row.path, session::FILE_NAME);
        let text = crate::zone::read_text(sessions, &rel)?.ok_or_else(|| {
            format!(
                "{} is the main session, but it has no {}: restore it from the drive's history.",
                row.path,
                session::FILE_NAME
            )
        })?;
        let agent = session::parse_session_agent_toml(&text).map_err(|r| r.sentence())?;
        Ok(Some((row.path, agent.room)))
    }

    /// The status anchor in `room`, sent only when the room has none.
    async fn ensure_anchor(
        client: &AgentClient,
        me: &UserId,
        proxy: &AgentConfig,
        room: &OwnedRoomId,
        sessions_subfolder: &str,
        path: &str,
        host: &str,
    ) -> Result<(), String> {
        if let Some(joined) = client.client().get_room(room) {
            if crate::runtime::latest_status(&joined, me).await.is_some() {
                return Ok(());
            }
        }
        let status = StatusContent {
            v: CONTENT_VERSION,
            session: format!("{sessions_subfolder}/{path}"),
            kind: SessionKind::Main,
            title: MAIN_TITLE.to_owned(),
            agent: proxy.matrix_user.clone(),
            host: host.to_owned(),
            epoch: 0,
            run: RunState::Idle,
            detail: None,
            waiting: None,
            anchor: None,
        };
        let content = serde_json::to_value(&status).map_err(|error| error.to_string())?;
        client
            .send(room, STATUS, content, None)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(all(unix, test))]
use dm::{is_main_dm, make_folder};

#[cfg(test)]
mod tests;

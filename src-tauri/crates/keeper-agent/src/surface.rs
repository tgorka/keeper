//! An agent and the note its person is looking at (stories 91.2, 91.3;
//! AD-382, AD-383).
//!
//! A notes vault is a synced folder plus a flag; a drive is a synced folder
//! whose agents zone declares it in `_drive.toml`. The docked notes view
//! names a note by its vault and its vault-relative path, and the agent by
//! its drive id and drive-relative path. The two are joined here, both ways
//! ([`drive_of`], [`locate`]), by `keeper_sync::browse`'s segment rules
//! (AD-65), never by string arithmetic in a caller.
//!
//! The five surface tools (`surface_open`, `surface_highlight`,
//! `surface_point`, `surface_scroll`, `surface_propose_edit`) are an
//! agent's alone: a ⌘9 bot's tool vocabulary is the drive's seven verbs and
//! stays so (R38). They reach the loop through [`ToolHost::run_named`] of
//! the agent's own host ([`SurfaceTools`]), offered only to an agent whose
//! audience is exactly its person ([`person`], Q10). A call reads the note
//! on this host's checkout to name body lines (R40), sends one
//! `dev.keeper.agent.surface.request` to the person's live, focused device
//! ([`surface_target`]) and waits for its `surface.result` — which the
//! copy's timeline handler hands to [`deliver`] before any routing, so it
//! never queues behind the turn waiting for it (R39) — at most `SURFACE_WAIT`.
//!
//! [`ToolHost::run_named`]: keeper_core::bots::tools::ToolHost::run_named

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{RecvTimeoutError, SyncSender};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use keeper_core::agents::claim::rfc3339;
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::events::{
    LineSpan, SurfaceArgs, SurfaceOutcome, SurfaceRequestContent, SurfaceResultContent,
    SurfaceTool, CONTENT_VERSION,
};
use keeper_core::agents::home::AgentConfig;
use keeper_core::agents::log::SurfaceBody;
use keeper_core::agents::presence::{surface_target, Published};
use keeper_core::bots::chat::{CancelSignal, ToolCall as WireToolCall, ToolSpec};
use keeper_core::bots::tools::ToolOutcome;
use keeper_core::notes::outline::{body_lines, find_heading};
use keeper_sync::browse;
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use serde_json::{json, Value};

use crate::matrix_sink::SendFuture;
use crate::sinks::Blocked;
use crate::zone::read_text;

/// How often a waiting call looks at its turn's cancel signal.
const POLL: Duration = Duration::from_millis(250);
/// The longest device detail the model is told.
const DETAIL_MAX: usize = 200;

/// A note as its drive names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRef {
    /// The drive's id, from its `_drive.toml`.
    pub drive: String,
    /// The note's path from the drive's root, `/`-joined.
    pub path: String,
}

/// The `_drive.toml` of the synced folder `profile`, read from its agents
/// zone; the sentence when it has none or it does not read.
pub fn declared(profile: &SyncProfile) -> Result<DriveDecl, String> {
    let zone = profile
        .agents_root()
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    let text = read_text(&zone, drive::FILE_NAME)?
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    drive::parse(&text).map_err(|refusal| refusal.sentence())
}

/// The note at `note_path` in the vault kept in `profile`'s `subfolder`, as
/// its drive names it; `None` when the folder declares no drive, or the path
/// does not name a file inside it.
pub fn drive_of(profile: &SyncProfile, subfolder: &str, note_path: &str) -> Option<DriveRef> {
    let decl = declared(profile).ok()?;
    let mut segments = browse::plain_segments(subfolder.trim_matches('/')).ok()?;
    segments.extend(browse::plain_segments(note_path).ok()?);
    if segments.is_empty() {
        return None;
    }
    let parts: Option<Vec<&str>> = segments.iter().map(|segment| segment.to_str()).collect();
    let path = parts?.join("/");
    match browse::resolve(&profile.local_path, &path) {
        Ok(Some(found)) if found.is_file() => Some(DriveRef {
            drive: decl.id,
            path,
        }),
        _ => None,
    }
}

/// Where on this device a surface request's drive and path are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// A note of the vault kept in the profile `vault_id`, at `note_path`
    /// from the vault's folder: opened in the editor.
    Note { vault_id: String, note_path: String },
    /// Any other file of the synced folder `profile_id`: opened in the Files
    /// preview, never the editor.
    File {
        profile_id: String,
        relative_path: String,
    },
}

/// The file `path` of the drive `drive` among this device's synced folders,
/// each with its notes vault's subfolder when it keeps one. A path the
/// segment rules refuse, or one that resolves outside its folder, is
/// refused with `browse`'s own sentence (AD-65).
pub fn locate(
    folders: &[(SyncProfile, Option<String>)],
    drive: &str,
    path: &str,
) -> Result<Located, String> {
    let (profile, vault) = folders
        .iter()
        .find(|(profile, _)| declared(profile).is_ok_and(|decl| decl.id == drive))
        .ok_or_else(|| format!("This device does not keep the drive {drive}."))?;
    let segments = browse::plain_segments(path).map_err(|refusal| refusal.to_string())?;
    match browse::resolve(&profile.local_path, path) {
        Ok(Some(found)) if found.is_file() => {}
        Ok(_) => return Err(format!("{path} is not a file on {drive}.")),
        Err(refusal) => return Err(refusal.to_string()),
    }
    let parts: Option<Vec<&str>> = segments.iter().map(|segment| segment.to_str()).collect();
    let parts = parts.ok_or_else(|| format!("{path} is not a file on {drive}."))?;
    if let Some(subfolder) = vault {
        let folder = browse::plain_segments(subfolder.trim_matches('/'))
            .map_err(|refusal| refusal.to_string())?;
        let folder: Option<Vec<&str>> = folder.iter().map(|segment| segment.to_str()).collect();
        if let Some(folder) = folder {
            if parts.len() > folder.len() && parts[..folder.len()] == folder[..] {
                return Ok(Located::Note {
                    vault_id: profile.id.clone(),
                    note_path: parts[folder.len()..].join("/"),
                });
            }
        }
    }
    Ok(Located::File {
        profile_id: profile.id.clone(),
        relative_path: parts.join("/"),
    })
}

// ---------------------------------------------------------------------------
// The offer
// ---------------------------------------------------------------------------

/// The person an agent's surface calls go to: the one reader of its home
/// drive — and, for a proxy, its `human` — or `None` when its audience is
/// anyone more (Q10, AD-383).
pub fn person(config: &AgentConfig) -> Option<&OwnedUserId> {
    let mut audience = config.audience.iter();
    let (Some(only), None) = (audience.next(), audience.next()) else {
        return None;
    };
    match &config.human {
        Some(human) if human != only => None,
        _ => Some(only),
    }
}

/// The surface tools `config`'s turns are offered: those its `[tools].allow`
/// names, and none when its audience is not exactly its person.
pub fn offered(config: &AgentConfig) -> Vec<SurfaceTool> {
    if person(config).is_none() {
        return Vec::new();
    }
    SurfaceTool::ALL
        .into_iter()
        .filter(|tool| config.allow.iter().any(|allowed| allowed == tool.wire()))
        .collect()
}

/// Whether `name` is one of the five surface tools.
pub fn is_surface(name: &str) -> bool {
    SurfaceTool::from_wire(name).is_some()
}

/// The specs of `tools`, as the model is offered them.
pub fn specs(tools: &[SurfaceTool]) -> Vec<ToolSpec> {
    let place = json!({
        "drive": {"type": "string", "description": "The drive's id, as the frame names the drives in scope."},
        "path": {"type": "string", "description": "The note's path from the drive's root, as drive_read takes it."},
    });
    let range = json!({
        "type": "object",
        "description": "Lines as drive_read numbers them: 1-based, inclusive, from ≤ to.",
        "properties": {"from": {"type": "integer", "minimum": 1}, "to": {"type": "integer", "minimum": 1}},
        "required": ["from", "to"],
        "additionalProperties": false,
    });
    let heading =
        json!({"type": "string", "description": "A heading's text, or its trail (Plans › Q3)."});
    tools
        .iter()
        .map(|tool| {
            let mut properties = place.clone();
            let (description, required): (&str, &[&str]) = match tool {
                SurfaceTool::Open => {
                    properties["heading"] = heading.clone();
                    ("Open a note on the screen your person is using now, at a heading if you name one. Answers done, expired or unavailable.", &["drive", "path"])
                }
                SurfaceTool::Highlight => {
                    properties["range"] = range.clone();
                    ("Highlight lines of a note on your person's screen until they dismiss it.", &["drive", "path", "range"])
                }
                SurfaceTool::Point => {
                    properties["range"] = range.clone();
                    ("Point at lines of a note on your person's screen with a short pulse.", &["drive", "path", "range"])
                }
                SurfaceTool::Scroll => {
                    properties["heading"] = heading.clone();
                    properties["range"] = range.clone();
                    ("Scroll your person's open note to a heading or to lines (one of the two).", &["drive", "path"])
                }
                SurfaceTool::ProposeEdit => {
                    properties["range"] = range.clone();
                    properties["text"] = json!({"type": "string", "description": "The lines that replace the range."});
                    ("Propose replacing lines of a note: your person sees the change and applies or declines it; you never write the note. Read the lines first: the proposal applies only while the note still holds what you read. Answers done (applied or not), declined, expired or unavailable.", &["drive", "path", "range", "text"])
                }
            };
            ToolSpec {
                name: tool.wire().to_owned(),
                description: description.to_owned(),
                parameters: json!({
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// A call
// ---------------------------------------------------------------------------

/// One surface call, its arguments read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceCall {
    pub tool: SurfaceTool,
    pub drive: String,
    pub path: String,
    pub heading: Option<String>,
    /// File lines, as `drive_read` numbers them.
    pub range: Option<(u32, u32)>,
    pub text: Option<String>,
}

impl SurfaceCall {
    /// `wire`'s arguments for `tool`, or the sentence the model is told
    /// instead. Nothing is sent for a call that does not read.
    pub fn parse(tool: SurfaceTool, wire: &WireToolCall) -> Result<SurfaceCall, String> {
        let name = tool.wire();
        let Some(Value::Object(args)) = &wire.arguments else {
            return Err(format!("The arguments for {name} must be a JSON object."));
        };
        let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_owned);
        let required =
            |key: &str| text(key).ok_or_else(|| format!("{name} needs a \"{key}\" argument."));
        let range = match args.get("range") {
            None | Some(Value::Null) => None,
            Some(range) => {
                let line = |key: &str| {
                    range[key]
                        .as_u64()
                        .and_then(|line| u32::try_from(line).ok())
                        .filter(|line| *line >= 1)
                };
                let (Some(from), Some(to)) = (line("from"), line("to")) else {
                    return Err(format!(
                        "{name}'s \"range\" is {{\"from\": n, \"to\": m}}, 1-based lines."
                    ));
                };
                if from > to {
                    return Err(format!(
                        "{name}'s range starts after it ends ({from} > {to}): lines are 1-based and inclusive, from ≤ to."
                    ));
                }
                Some((from, to))
            }
        };
        let call = SurfaceCall {
            tool,
            drive: required("drive")?,
            path: required("path")?,
            heading: text("heading").filter(|heading| !heading.trim().is_empty()),
            range,
            text: text("text"),
        };
        match tool {
            SurfaceTool::Highlight | SurfaceTool::Point if call.range.is_none() => {
                Err(format!("{name} needs a \"range\" argument."))
            }
            SurfaceTool::ProposeEdit if call.range.is_none() => {
                Err(format!("{name} needs a \"range\" argument."))
            }
            SurfaceTool::ProposeEdit if call.text.is_none() => {
                Err(format!("{name} needs a \"text\" argument."))
            }
            SurfaceTool::Scroll if call.range.is_none() && call.heading.is_none() => {
                Err(format!("{name} needs a \"heading\" or a \"range\"."))
            }
            _ => Ok(call),
        }
    }
}

/// A boxed future of the presences a control room holds.
pub type PresenceFuture<'a> = Pin<Box<dyn Future<Output = Vec<Published>> + Send + 'a>>;

/// The session room's side of a surface call.
pub trait SurfacePort: Send + Sync {
    /// The session room a request goes into.
    fn room(&self) -> &RoomId;
    /// Every presence in the principal's control room, as the last sync left
    /// them.
    fn presences(&self) -> PresenceFuture<'_>;
    /// Send a `surface.request` with `content` into the session room.
    fn request(&self, content: Value) -> SendFuture<'_>;
}

/// One call waiting for its device.
struct Waiter {
    room: OwnedRoomId,
    person: OwnedUserId,
    device: String,
    answer: SyncSender<SurfaceResultContent>,
}

fn waiting() -> MutexGuard<'static, HashMap<String, Waiter>> {
    static WAITING: LazyLock<Mutex<HashMap<String, Waiter>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    // Senders and ids only: nothing torn by a panic mid-update.
    WAITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A `surface.result` arrived in `room` from `sender`: hand it to the call
/// waiting on its request, when it comes from that call's person, from a
/// device their identity signed (`signed`), and names the device the request
/// went to. Anyone else's result changes nothing. Whether it was taken.
pub fn deliver(room: &RoomId, sender: &UserId, signed: bool, content: &Value) -> bool {
    let Ok(result) = serde_json::from_value::<SurfaceResultContent>(content.clone()) else {
        return false;
    };
    if result.v != CONTENT_VERSION || !signed {
        return false;
    }
    let mut waiting = waiting();
    let matches = waiting.get(&result.request).is_some_and(|waiter| {
        waiter.room == room && waiter.person == sender && waiter.device == result.device
    });
    if !matches {
        return false;
    }
    match waiting.remove(&result.request) {
        // A receiver that is gone was a call that stopped waiting.
        Some(waiter) => waiter.answer.send(result).is_ok(),
        None => false,
    }
}

/// The surface tools of one turn: what its agent is offered, the drives in
/// scope, and where requests go.
pub struct SurfaceTools {
    /// `None` on a host with no room to send from: every call is
    /// `unavailable`.
    pub port: Option<Arc<dyn SurfacePort>>,
    pub person: OwnedUserId,
    pub offered: Vec<SurfaceTool>,
    /// The mounted drives, each profile's id its drive id.
    pub profiles: Vec<SyncProfile>,
    /// The session's drives in scope.
    pub scope: Vec<String>,
    pub stop: CancelSignal,
    pub wait: Duration,
    /// The `surface` lines of calls that sent a request, for the turn's log.
    pub lines: Mutex<Vec<SurfaceBody>>,
    /// Whether a request — its whole content, the replaced text a proposed
    /// edit carries included — may go into the room it is sent into, as
    /// the room is at the send (R168); `None` sends unchecked.
    pub admit: Option<SurfaceAdmit>,
}

/// [`SurfaceTools::admit`]: the room's verdict on the tool's name and the
/// request's bytes, the block not yet audited.
pub type SurfaceAdmit = Arc<dyn Fn(&str, &[u8]) -> Result<(), Blocked> + Send + Sync>;

/// What the call's audit makes of the room's verdict on its request to the
/// note at `(drive, path)`: nothing to say, its row written before the
/// request is sent, or the sentence refusing it (R90).
pub type Admission<'a> = &'a dyn Fn(Result<(), Blocked>, (&str, &str)) -> Result<(), String>;

fn refused(reason: String) -> Option<ToolOutcome> {
    Some(ToolOutcome::Refused { reason })
}

fn answered(text: String) -> Option<ToolOutcome> {
    Some(ToolOutcome::Answered { text })
}

/// What the model is told of an answer.
fn told(result: &SurfaceResultContent, host_detail: Option<&str>) -> String {
    let mut text = result.outcome.word().to_owned();
    if result.outcome == SurfaceOutcome::Done {
        match result.applied {
            Some(true) => text.push_str(" (applied)"),
            Some(false) => text.push_str(" (not applied)"),
            None => {}
        }
    }
    if let Some(detail) = host_detail {
        text.push_str(": ");
        text.push_str(detail);
    }
    if let Some(detail) = result.detail.as_deref().filter(|d| !d.trim().is_empty()) {
        let detail: String = detail.trim().chars().take(DETAIL_MAX).collect();
        text.push_str(&format!(" — the device says: {detail}"));
    }
    text
}

/// Run `fut` to its end from inside a synchronous tool call, handing this
/// worker's slot to another thread meanwhile.
fn block_on<F: Future>(fut: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
}

fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

impl SurfaceTools {
    /// The `surface` lines written since the last take.
    pub fn take_lines(&self) -> Vec<SurfaceBody> {
        std::mem::take(&mut *self.lines.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Run `wire` when it names a surface tool; `None` for any other name.
    /// `admission` decides, once the request is made, whether it is sent.
    pub fn run(&self, wire: &WireToolCall, admission: Admission<'_>) -> Option<ToolOutcome> {
        let tool = SurfaceTool::from_wire(&wire.name)?;
        let name = tool.wire();
        if !self.offered.contains(&tool) {
            return refused(format!("{name} is not one of this agent's tools."));
        }
        let call = match SurfaceCall::parse(tool, wire) {
            Ok(call) => call,
            Err(sentence) => return refused(sentence),
        };
        if !self.scope.contains(&call.drive) {
            return refused(format!(
                "{} is not among this session's drives in scope.",
                call.drive
            ));
        }
        let Some(profile) = self.profiles.iter().find(|p| p.id == call.drive) else {
            return refused(format!("unavailable: {} is not on this host.", call.drive));
        };
        let file = match browse::resolve(&profile.local_path, &call.path) {
            Ok(Some(file)) if file.is_file() => file,
            Ok(_) => {
                return refused(format!(
                    "unavailable: {} is not a file on {}.",
                    call.path, call.drive
                ))
            }
            Err(refusal) => return refused(format!("unavailable: {refusal}")),
        };
        let Ok(note) = std::fs::read_to_string(&file) else {
            return refused(format!("unavailable: {} is not a text file.", call.path));
        };
        let mut args = SurfaceArgs {
            drive: call.drive.clone(),
            path: call.path.clone(),
            heading: call.heading.clone(),
            range: None,
            text: None,
            expected: None,
        };
        let mut host_detail = None;
        match (call.range, &call.heading) {
            (Some((from, to)), _) => match body_lines(&note, from, to) {
                Ok((first, last, text)) => {
                    args.range = Some(LineSpan {
                        from: first,
                        to: last,
                    });
                    if tool == SurfaceTool::ProposeEdit {
                        args.text = call.text.clone();
                        args.expected = Some(text);
                    }
                }
                Err(refusal) => return refused(refusal.to_string()),
            },
            (None, Some(heading)) => match find_heading(&note, heading) {
                Some((from, to)) => args.range = Some(LineSpan { from, to }),
                // Open goes to the top; scroll has nowhere to go.
                None if tool == SurfaceTool::Open => host_detail = Some("no such heading"),
                None => return refused(format!("{} has no heading \"{heading}\".", call.path)),
            },
            (None, None) => {}
        }

        let Some(port) = &self.port else {
            return answered(SurfaceOutcome::Unavailable.word().to_owned());
        };
        let presences = block_on(port.presences());
        let Some(device) = surface_target(&presences, &self.person, now_ms()) else {
            // No live, focused device: the model is told so and nothing is
            // sent.
            return answered(SurfaceOutcome::Unavailable.word().to_owned());
        };
        let id = ulid::Ulid::new().to_string();
        let wait_ms = u64::try_from(self.wait.as_millis()).unwrap_or(u64::MAX);
        let request = SurfaceRequestContent {
            v: CONTENT_VERSION,
            id: id.clone(),
            device: device.clone(),
            tool,
            args,
            expires_at: rfc3339(now_ms().saturating_add(wait_ms)),
        };
        let Ok(content) = serde_json::to_value(&request) else {
            return refused("unavailable: the request could not be written".to_owned());
        };
        // The request goes into the session's room, not to the device: the
        // room's audience is its audience.
        let verdict = match &self.admit {
            Some(admit) => admit(name, content.to_string().as_bytes()),
            None => Ok(()),
        };
        if let Err(sentence) = admission(verdict, (&call.drive, &call.path)) {
            return refused(sentence);
        }
        let (answer, answers) = std::sync::mpsc::sync_channel::<SurfaceResultContent>(1);
        waiting().insert(
            id.clone(),
            Waiter {
                room: port.room().to_owned(),
                person: self.person.clone(),
                device: device.clone(),
                answer,
            },
        );
        if let Err(error) = block_on(port.request(content)) {
            waiting().remove(&id);
            return refused(format!(
                "unavailable: the request could not be sent ({error})"
            ));
        }
        let deadline = Instant::now() + self.wait;
        let result = tokio::task::block_in_place(|| loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || self.stop.is_cancelled() {
                break None;
            }
            match answers.recv_timeout(left.min(POLL)) {
                Ok(result) => break Some(result),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break None,
            }
        });
        waiting().remove(&id);
        let result = result.unwrap_or(SurfaceResultContent {
            v: CONTENT_VERSION,
            request: id.clone(),
            device: device.clone(),
            outcome: SurfaceOutcome::Expired,
            applied: None,
            detail: None,
        });
        self.lines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(SurfaceBody {
                id,
                tool: name.to_owned(),
                device,
                outcome: Some(result.outcome.word().to_owned()),
            });
        answered(told(&result, host_detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(declares: bool) -> (tempfile::TempDir, SyncProfile) {
        let root = tempfile::tempdir().expect("tempdir");
        let write = |rel: &str, text: &str| {
            let path = root.path().join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        };
        if declares {
            write(
                "80-agents/_drive.toml",
                "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\n",
            );
        }
        write("notes/plans/q3.md", "# Q3\n");
        write("media/talk.txt", "a talk\n");
        let mut profile = SyncProfile::new("p1", "tgdrive", root.path(), "unused");
        profile.agents = Some(Default::default());
        (root, profile)
    }

    #[test]
    fn a_vault_note_is_named_by_its_drive() {
        let (_root, profile) = folder(true);
        assert_eq!(
            drive_of(&profile, "notes", "plans/q3.md"),
            Some(DriveRef {
                drive: "tgdrive".to_owned(),
                path: "notes/plans/q3.md".to_owned(),
            })
        );
        // A vault at the folder's root.
        assert_eq!(
            drive_of(&profile, "", "notes/plans/q3.md").map(|found| found.path),
            Some("notes/plans/q3.md".to_owned())
        );
        // No such note, an escape, a folder with no `_drive.toml`.
        assert_eq!(drive_of(&profile, "notes", "plans/q4.md"), None);
        assert_eq!(drive_of(&profile, "notes", "../notes/plans/q3.md"), None);
        let (_other, undeclared) = folder(false);
        assert_eq!(drive_of(&undeclared, "notes", "plans/q3.md"), None);
    }

    #[test]
    fn a_surface_path_is_resolved_by_browse_resolve() {
        let (_root, profile) = folder(true);
        let folders = [(profile.clone(), Some("notes".to_owned()))];
        // A note of the vault opens in the editor, named by the vault.
        assert_eq!(
            locate(&folders, "tgdrive", "notes/plans/q3.md"),
            Ok(Located::Note {
                vault_id: "p1".to_owned(),
                note_path: "plans/q3.md".to_owned(),
            })
        );
        // A file outside every vault opens in the Files preview.
        assert_eq!(
            locate(&folders, "tgdrive", "media/talk.txt"),
            Ok(Located::File {
                profile_id: "p1".to_owned(),
                relative_path: "media/talk.txt".to_owned(),
            })
        );
        // A path escaping the drive is refused with browse's own sentence.
        let escape = locate(&folders, "tgdrive", "../outside.md").expect_err("refused");
        let browse_says = browse::plain_segments("../outside.md")
            .expect_err("browse refuses it")
            .to_string();
        assert_eq!(escape, browse_says);
        // A drive this device does not keep, a file that is not there.
        assert!(locate(&folders, "neuradrive", "notes/plans/q3.md").is_err());
        assert!(locate(&folders, "tgdrive", "notes/plans/q4.md").is_err());
        // With no vault, every file is a file.
        assert!(matches!(
            locate(&[(profile, None)], "tgdrive", "notes/plans/q3.md"),
            Ok(Located::File { .. })
        ));
    }

    // -----------------------------------------------------------------------
    // A call, against a room that records what it is sent
    // -----------------------------------------------------------------------

    use keeper_core::agents::events::{PresencePlatform, SURFACE_WAIT};
    use keeper_core::agents::presence::{presence_content, DevicePresence};
    use keeper_core::bots::chat::{cancellation, CancelHandle};

    const TGORKA: &str = "@tgorka:example.org";
    const NOTE: &str = "---\ntitle: Plan\n---\n# Plan\nfirst\nsecond\nthird\n## Q3\nnumbers\n";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    /// What the device does when a request reaches it.
    #[derive(Clone)]
    enum Device {
        /// Nothing: the call waits out its time.
        Silent,
        /// These results, in order, as `(sender, signed, content)`.
        Answers(Vec<(String, bool, Value)>),
    }

    struct Room {
        id: OwnedRoomId,
        presences: Vec<Published>,
        sent: Mutex<Vec<Value>>,
        device: Device,
        taken: Arc<Mutex<Vec<bool>>>,
    }

    impl SurfacePort for Room {
        fn room(&self) -> &RoomId {
            &self.id
        }

        fn presences(&self) -> PresenceFuture<'_> {
            let presences = self.presences.clone();
            Box::pin(async move { presences })
        }

        fn request(&self, content: Value) -> SendFuture<'_> {
            self.sent.lock().expect("lock").push(content.clone());
            if let Device::Answers(answers) = self.device.clone() {
                let room = self.id.clone();
                let taken = Arc::clone(&self.taken);
                std::thread::spawn(move || {
                    for (sender, signed, mut result) in answers {
                        std::thread::sleep(Duration::from_millis(50));
                        if result["request"] == "<id>" {
                            result["request"] = content["id"].clone();
                        }
                        let took = deliver(&room, &user(&sender), signed, &result);
                        taken.lock().expect("lock").push(took);
                    }
                });
            }
            Box::pin(async {
                Ok(
                    matrix_sdk::ruma::OwnedEventId::try_from("$request:example.org")
                        .expect("event id"),
                )
            })
        }
    }

    fn presence(device: &str, focused: bool) -> Published {
        let state = DevicePresence {
            platform: PresencePlatform::Ios,
            focused,
            view: "notes".to_owned(),
        };
        Published {
            state_key: device.to_owned(),
            sender: user(TGORKA),
            content: serde_json::to_value(presence_content(
                &user(TGORKA),
                device,
                &state,
                now_ms(),
            ))
            .expect("presence"),
        }
    }

    fn result(sender: &str, signed: bool, device: &str, outcome: &str) -> (String, bool, Value) {
        (
            sender.to_owned(),
            signed,
            json!({"v": 1, "request": "<id>", "device": device, "outcome": outcome}),
        )
    }

    struct Bench {
        _root: tempfile::TempDir,
        room: Arc<Room>,
        tools: SurfaceTools,
        _stop: CancelHandle,
    }

    fn bench(presences: Vec<Published>, device: Device, wait: Duration) -> Bench {
        let (root, mut profile) = folder(true);
        std::fs::write(root.path().join("notes/plan.md"), NOTE).expect("note");
        profile.id = "tgdrive".to_owned();
        let room = Arc::new(Room {
            id: OwnedRoomId::try_from("!dm:example.org").expect("room"),
            presences,
            sent: Mutex::new(Vec::new()),
            device,
            taken: Arc::new(Mutex::new(Vec::new())),
        });
        let (stop, signal) = cancellation();
        let port: Arc<dyn SurfacePort> = room.clone();
        Bench {
            _root: root,
            room,
            tools: SurfaceTools {
                port: Some(port),
                person: user(TGORKA),
                offered: SurfaceTool::ALL.to_vec(),
                profiles: vec![profile],
                scope: vec!["tgdrive".to_owned()],
                stop: signal,
                wait,
                lines: Mutex::new(Vec::new()),
                admit: None,
            },
            _stop: stop,
        }
    }

    fn wire(name: &str, arguments: Value) -> WireToolCall {
        WireToolCall {
            id: "call_0".to_owned(),
            name: name.to_owned(),
            arguments_raw: arguments.to_string(),
            arguments: Some(arguments),
        }
    }

    /// The room's verdict as the call's answer, with no audit row.
    fn unaudited(verdict: Result<(), Blocked>, _: (&str, &str)) -> Result<(), String> {
        verdict.map_err(|blocked| blocked.sentence)
    }

    fn said(outcome: Option<ToolOutcome>) -> String {
        match outcome.expect("a surface tool") {
            ToolOutcome::Answered { text } => text,
            ToolOutcome::Refused { reason } => format!("Refused: {reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_proposal_names_body_lines_and_waits_for_the_persons_device() {
        let bench = bench(
            vec![presence("KALYPSO", true), presence("HESPERIA", false)],
            Device::Answers(vec![
                // Marta, an unsigned device, another device: none counts.
                result("@marta:example.org", true, "KALYPSO", "declined"),
                result(TGORKA, false, "KALYPSO", "declined"),
                result(TGORKA, true, "HESPERIA", "declined"),
                {
                    let (sender, signed, mut done) = result(TGORKA, true, "KALYPSO", "done");
                    done["applied"] = json!(true);
                    (sender, signed, done)
                },
            ]),
            SURFACE_WAIT,
        );
        let text = said(bench.tools.run(&wire(
            "surface_propose_edit",
            json!({"drive": "tgdrive", "path": "notes/plan.md", "range": {"from": 5, "to": 6}, "text": "1st\n2nd"}),
        ), &unaudited));
        assert_eq!(text, "done (applied)");
        assert_eq!(
            *bench.room.taken.lock().expect("lock"),
            [false, false, false, true]
        );
        let sent = bench.room.sent.lock().expect("lock").clone();
        assert_eq!(sent.len(), 1, "one request");
        let request: SurfaceRequestContent =
            serde_json::from_value(sent[0].clone()).expect("a request");
        assert_eq!(request.device, "KALYPSO", "the focused device");
        // File lines 5–6 are body lines 2–3 (the frontmatter is three), and
        // the request carries what the agent read there.
        assert_eq!(request.args.range, Some(LineSpan { from: 2, to: 3 }));
        assert_eq!(request.args.expected.as_deref(), Some("first\nsecond"));
        assert_eq!(request.args.text.as_deref(), Some("1st\n2nd"));
        let lines = bench.tools.take_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].id, request.id);
        assert_eq!(lines[0].device, "KALYPSO");
        assert_eq!(lines[0].outcome.as_deref(), Some("done"));
        // Rust never writes the note.
        let on_disk = std::fs::read_to_string(bench._root.path().join("notes/plan.md"));
        assert_eq!(on_disk.expect("note"), NOTE);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_call_sends_nothing_unless_it_reads_and_a_device_is_in_front() {
        let bench = bench(
            vec![presence("KALYPSO", true)],
            Device::Silent,
            SURFACE_WAIT,
        );
        let call = |name: &str, args: Value| said(bench.tools.run(&wire(name, args), &unaudited));
        // Backwards, in the frontmatter, past the end, out of scope.
        assert!(call(
            "surface_highlight",
            json!({"drive": "tgdrive", "path": "notes/plan.md", "range": {"from": 6, "to": 5}})
        )
        .contains("starts after it ends"));
        assert!(call(
            "surface_point",
            json!({"drive": "tgdrive", "path": "notes/plan.md", "range": {"from": 2, "to": 4}})
        )
        .contains("frontmatter"));
        assert!(call(
            "surface_point",
            json!({"drive": "tgdrive", "path": "notes/plan.md", "range": {"from": 9, "to": 12}})
        )
        .contains("has 9 lines"));
        assert!(call(
            "surface_open",
            json!({"drive": "neuradrive", "path": "notes/plan.md"})
        )
        .contains("not among this session's drives"));
        // A path escaping the drive: unavailable, in browse's words.
        let escape = call(
            "surface_open",
            json!({"drive": "tgdrive", "path": "../plan.md"}),
        );
        let browse_says = browse::resolve(&bench.tools.profiles[0].local_path, "../plan.md")
            .expect_err("browse refuses it")
            .to_string();
        assert_eq!(escape, format!("Refused: unavailable: {browse_says}"));
        // A scroll to a heading the note does not have.
        assert!(call(
            "surface_scroll",
            json!({"drive": "tgdrive", "path": "notes/plan.md", "heading": "Q4"})
        )
        .contains("no heading"));
        assert!(
            bench.room.sent.lock().expect("lock").is_empty(),
            "nothing was sent"
        );
        // A tool this agent is not offered.
        let mut narrow = bench;
        narrow.tools.offered = vec![SurfaceTool::Open];
        assert!(said(
            narrow
                .tools
                .run(&wire("surface_point", json!({})), &unaudited)
        )
        .contains("not one of this agent's tools"));
        // Not a surface tool at all: the drive verbs' business.
        assert_eq!(
            narrow.tools.run(&wire("drive_read", json!({})), &unaudited),
            None
        );

        // No live, focused device: `unavailable`, and no event.
        let away = bench_away();
        assert_eq!(
            said(away.tools.run(
                &wire(
                    "surface_open",
                    json!({"drive": "tgdrive", "path": "notes/plan.md"})
                ),
                &unaudited
            )),
            "unavailable"
        );
        assert!(away.room.sent.lock().expect("lock").is_empty());
    }

    fn bench_away() -> Bench {
        bench(
            vec![presence("HESPERIA", false)],
            Device::Silent,
            SURFACE_WAIT,
        )
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_unanswered_call_expires_and_a_missing_heading_opens_the_top() {
        let bench = bench(
            vec![presence("KALYPSO", true)],
            Device::Silent,
            Duration::from_millis(400),
        );
        let started = Instant::now();
        let text = said(bench.tools.run(
            &wire(
                "surface_open",
                json!({"drive": "tgdrive", "path": "notes/plan.md", "heading": "q3"}),
            ),
            &unaudited,
        ));
        assert_eq!(text, "expired");
        assert!(started.elapsed() >= Duration::from_millis(400));
        let sent = bench.room.sent.lock().expect("lock").clone();
        // The heading's section, case-insensitively, in body lines.
        assert_eq!(sent[0]["args"]["range"], json!({"from": 5, "to": 6}));
        assert_eq!(
            bench.tools.take_lines()[0].outcome.as_deref(),
            Some("expired")
        );

        let open = bench_answering(vec![result(TGORKA, true, "KALYPSO", "done")]);
        let text = said(open.tools.run(
            &wire(
                "surface_open",
                json!({"drive": "tgdrive", "path": "notes/plan.md", "heading": "Budget"}),
            ),
            &unaudited,
        ));
        assert_eq!(text, "done: no such heading");
        let sent = open.room.sent.lock().expect("lock").clone();
        assert!(sent[0]["args"].get("range").is_none(), "opened at the top");
    }

    fn bench_answering(answers: Vec<(String, bool, Value)>) -> Bench {
        bench(
            vec![presence("KALYPSO", true)],
            Device::Answers(answers),
            SURFACE_WAIT,
        )
    }
}

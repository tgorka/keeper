//! An agent's console, and the guardrails a person picks for it (story
//! 101.5, R420).
//!
//! `agent.toml` `[run]` says how the agent's `run` tool works: `mode` is one
//! of [`RunMode`]'s four words and `paths` the folders its commands may read
//! and write beside the session's `workspace/` — each a folder of a drive in
//! the agent's scope (`tgdrive/projects/site`) or an absolute path on a host
//! (`/Users/tgorka/prj/keeper`). A home without `[run]` keeps the behaviour
//! it had: `supervised` when `[tools].allow` names `run`, else `off`. What a
//! mode allows, which mode an agent may have, what Settings › Agents shows
//! and what a save is refused with are decided here; the host, the shell and
//! the front decide none of it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::home::{AgentConfig, AgentKind};

/// How an agent's console works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RunMode {
    /// No console: `run` is not offered.
    #[default]
    Off,
    /// Every command waits for a person's approval and runs in the host's
    /// sandbox — the console as epic 96 shipped it.
    Supervised,
    /// Commands run in the host's sandbox, limited to the session's
    /// workspace and `paths`, without asking; one that reaches the network
    /// or mounts a drive asks.
    Sandboxed,
    /// No sandbox and no approval ("yolo"): never for a gate or an agent a
    /// gate hands outside work to.
    Full,
}

impl RunMode {
    /// Every mode, in the order Settings offers them.
    pub const ALL: [RunMode; 4] = [
        RunMode::Off,
        RunMode::Supervised,
        RunMode::Sandboxed,
        RunMode::Full,
    ];

    /// The word `agent.toml` spells it with.
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Off => "off",
            RunMode::Supervised => "supervised",
            RunMode::Sandboxed => "sandboxed",
            RunMode::Full => "full",
        }
    }

    pub fn parse(word: &str) -> Option<RunMode> {
        RunMode::ALL.into_iter().find(|mode| mode.as_str() == word)
    }

    /// Whether `run` is offered at all.
    pub fn offers_run(self) -> bool {
        self != RunMode::Off
    }

    /// Whether a run is started inside the host's sandbox.
    pub fn sandboxes(self) -> bool {
        matches!(self, RunMode::Supervised | RunMode::Sandboxed)
    }

    /// Whether the sandbox alone guards a run of this mode with `network`,
    /// drives to `read` and folders asked for beyond the console's
    /// (`widens`, R440 08): a `sandboxed` run that reaches nobody and whose
    /// writes the kernel confines to the folders the person granted
    /// read-write and its workspace — whatever its session read, untrusted
    /// content can at worst change files there (R458). Such a run is
    /// neither asked about nor raised by its session's integrity, held code
    /// included (R459); a row's own T5 is still refused.
    pub fn guards_itself(self, network: bool, reads_drives: bool, widens: bool) -> bool {
        self == RunMode::Sandboxed && !network && !reads_drives && !widens
    }

    /// Whether a run of this mode with `network`, drives to `read` and
    /// folders asked for (`widens`) goes without a person's approval, where
    /// `integrity_allows` is the integrity rule's verdict on it in its
    /// session (R82: an untrusted session asks from T3): in `sandboxed`
    /// mode a run its sandbox guards ([`Self::guards_itself`]); in `full`
    /// mode every run the integrity rule lets through. T5 is refused before
    /// this is asked.
    pub fn runs_unasked(
        self,
        network: bool,
        reads_drives: bool,
        widens: bool,
        integrity_allows: bool,
    ) -> bool {
        match self {
            RunMode::Off | RunMode::Supervised => false,
            RunMode::Sandboxed => self.guards_itself(network, reads_drives, widens),
            RunMode::Full => integrity_allows,
        }
    }
}

/// One folder of `[run].paths`, as the file names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPath {
    /// A folder of a drive in the agent's scope: its id and the path inside
    /// it, resolved on each host through the drive's own resolver.
    Drive { drive: String, path: String },
    /// An absolute path on whichever host can express it.
    Host(String),
}

impl RunPath {
    /// The path as `agent.toml` spells it.
    pub fn written(&self) -> String {
        match self {
            RunPath::Drive { drive, path } => format!("{drive}/{path}"),
            RunPath::Host(path) => path.clone(),
        }
    }
}

/// `[run]` as read: the mode, its folders, and whether the file has the
/// table at all (a home without it keeps the mode its `[tools].allow`
/// implies).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunConfig {
    pub mode: RunMode,
    pub paths: Vec<RunPath>,
    pub said: bool,
}

/// At most this many folders in `[run].paths`.
pub const PATHS_MAX: usize = 16;
/// At most this many bytes in one of them.
pub const PATH_MAX_BYTES: usize = 1024;

/// Folders keeper keeps to itself inside a drive: a console is never given
/// one.
const KEEPER_FOLDERS: [&str; 2] = [".git", ".keeper"];

/// `text`, one entry of `[run].paths`, read against the drives in the
/// agent's scope (`[tools].drives` with the home drive first). The error is
/// the clause of the refusal that says why. Only the spelling is read here;
/// where the folder lands is each host's resolver's (AD-65, AD-159).
pub fn parse_path(text: &str, drives: &[String]) -> Result<RunPath, String> {
    if text.trim().is_empty() {
        return Err("an empty path names no folder".to_owned());
    }
    if text.len() > PATH_MAX_BYTES {
        return Err(format!("\"{text}\" is longer than {PATH_MAX_BYTES} bytes"));
    }
    if text.starts_with('~') {
        return Err(format!(
            "\"{text}\" starts with ~, which keeper does not expand: write the folder's absolute path"
        ));
    }
    let plain = |segments: &[&str]| {
        segments
            .iter()
            .all(|part| !part.is_empty() && *part != "." && *part != ".." && !part.contains('\0'))
    };
    if let Some(rest) = text.strip_prefix('/') {
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.is_empty() {
            return Err("/ is the whole file system".to_owned());
        }
        let segments: Vec<&str> = rest.split('/').collect();
        if !plain(&segments) {
            return Err(format!(
                "\"{text}\" is not a plain absolute path: no empty, . or .. parts"
            ));
        }
        return Ok(RunPath::Host(format!("/{rest}")));
    }
    let trimmed = text.strip_suffix('/').unwrap_or(text);
    let segments: Vec<&str> = trimmed.split('/').collect();
    let Some((drive, inside)) = segments.split_first() else {
        return Err("an empty path names no folder".to_owned());
    };
    if !drives.iter().any(|scope| scope == drive) {
        return Err(format!(
            "\"{text}\" is neither an absolute path nor a folder of a drive in this agent's scope ({})",
            drives.join(", ")
        ));
    }
    if inside.is_empty() {
        return Err(format!(
            "\"{text}\" is a whole drive, and a console is given a folder inside one"
        ));
    }
    if !plain(inside) {
        return Err(format!(
            "\"{text}\" is not a plain folder path: no empty, . or .. parts"
        ));
    }
    // Case-folded: on a case-insensitive volume `.Keeper` is `.keeper`.
    if let Some(kept) = inside.iter().find(|part| {
        KEEPER_FOLDERS
            .iter()
            .any(|kept| part.eq_ignore_ascii_case(kept))
    }) {
        return Err(format!(
            "\"{text}\" reaches {kept}, which keeper keeps to itself"
        ));
    }
    Ok(RunPath::Drive {
        drive: (*drive).to_owned(),
        path: inside.join("/"),
    })
}

/// Why `kind` may not have `mode`, as the clause of `run.mode`'s refusal:
/// a gate has no console at all, and never full access.
pub fn mode_refusal(kind: AgentKind, mode: RunMode) -> Option<String> {
    match (kind, mode) {
        (AgentKind::Gate, RunMode::Full) => Some(
            "full access is never given to a gate: it answers an outside system, whose words would run commands on this host unchecked".to_owned(),
        ),
        (AgentKind::Gate, RunMode::Supervised | RunMode::Sandboxed) => Some(
            "a gate has no console: its tools are at most reply, delegate and journal_append".to_owned(),
        ),
        _ => None,
    }
}

/// Why an agent a gate hands work to may not have full access: the gate
/// `gate` names it in its `delegates`.
pub fn delegate_refusal(gate: &str) -> String {
    format!(
        "full access is never given to an agent a gate hands outside work to, and the gate {gate} names this one in its delegates"
    )
}

/// The standing warning for an agent at full access — the same sentence in
/// Settings › Agents and in its room's header.
pub fn full_access_warning(name: &str) -> String {
    format!(
        "Full access: {}'s commands run on its host without a sandbox and without asking. They can read, change and delete anything that host's user can, and reach the network.",
        capitalised(name)
    )
}

/// What Settings › Agents says where the folders would be, for an agent at
/// full access (R440): its commands are not held to any folder.
pub const UNRESTRICTED: &str =
    "Unrestricted — folders do not constrain full access: its commands reach anything its host's user can.";

/// What a room's header says of an agent whose host did not say how its
/// commands run — a host older than the console (R440 09): never read as
/// a safe console.
pub const CONSOLE_UNKNOWN: &str =
    "Console: unknown. This agent's host does not say how its commands run.";

/// What a room's header says when `count` of an agent's console folders
/// are not given to its commands on `host` (R440, R381): the Settings ›
/// Agents row and `keeper.log` on that host say which and why.
pub fn folders_not_given(count: u32, host: &str) -> String {
    if count == 1 {
        format!("1 of its console's folders is not given to its commands on {host}.")
    } else {
        format!("{count} of its console's folders are not given to its commands on {host}.")
    }
}

/// What a room's header says when the agent's home asks for a console and
/// `host`'s sandbox did not pass its probe, so it runs no command there
/// (story 102.3): off by failure, not by choice. That host's log says why.
pub fn sandbox_failed(name: &str, host: &str) -> String {
    format!(
        "Console off on {host}: {name}'s console is set, but {host}'s sandbox did not pass its check, so {name} runs no commands there. The reason is in that host's log."
    )
}

/// Why `drive`'s agents' consoles cannot be changed on a Mac that only
/// pulls it: a change made here would never reach the drive.
pub fn pull_only(drive: &str) -> String {
    format!("This Mac only pulls {drive}, so a change made here would never reach the drive. Change the console where the drive is written.")
}

/// What a save is refused with when the file's console is no longer the
/// one the row showed.
pub const CHANGED_UNDERNEATH: &str = "This agent's agent.toml names another console than the one shown here: it changed on the drive meanwhile. Look at it again and choose.";

/// One mode as Settings › Agents offers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleModeVm {
    pub mode: RunMode,
    pub label: String,
    pub description: String,
    /// Why this agent may not have it, in one sentence; `None` when it may.
    pub refused: Option<String>,
}

/// What a Settings › Agents row says under a folder of an agent's console
/// this Mac does not give its commands, `why` the clause (R440, R381).
pub fn not_given_here(why: &str) -> String {
    format!("Not given to its commands on this Mac: {why}.")
}

/// A folder of an agent's console that this Mac does not give its commands,
/// and why (R440, R381).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleFolderVm {
    /// As `[run].paths` spells it.
    pub path: String,
    /// The sentence its row says ([`not_given_here`]).
    pub refused: String,
}

/// An agent's console on its Settings › Agents row (101.5 #3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentConsoleVm {
    /// The mode in effect: `[run].mode`, or the one `[tools].allow` implies.
    pub mode: RunMode,
    /// `[run].paths` as the file spells them.
    pub paths: Vec<String>,
    /// Every mode, each with why it is refused for this agent.
    pub modes: Vec<ConsoleModeVm>,
    /// The drives a folder may be a folder of: the agent's scope.
    pub drives: Vec<String>,
    /// The full-access warning naming this agent: said beside the mode
    /// whenever `full` is chosen, saved or not.
    pub warning: String,
    /// What is said where the folders are while `full` is chosen
    /// ([`UNRESTRICTED`]): full access is held to no folder.
    pub unrestricted: String,
    /// The folders of `paths` this Mac does not give the agent's commands,
    /// each with why (R440, R381).
    pub refused: Vec<ConsoleFolderVm>,
    /// Why the console cannot be changed from this Mac, in one sentence.
    pub locked: Option<String>,
}

/// A save of one agent's console: the choice, and what the row showed, so a
/// file changed meanwhile is refused rather than overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentConsoleReq {
    pub profile_id: String,
    pub agent: String,
    pub mode: RunMode,
    pub paths: Vec<String>,
    pub shown_mode: RunMode,
    pub shown_paths: Vec<String>,
}

fn label(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Off => "Off",
        RunMode::Supervised => "Supervised",
        RunMode::Sandboxed => "Sandboxed",
        RunMode::Full => "Full access",
    }
}

fn description(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Off => "No console: the agent runs no commands.",
        RunMode::Supervised => "Every command waits for your approval, and runs in its host's sandbox.",
        RunMode::Sandboxed => "Commands run in its host's sandbox without asking, writing only the session's workspace and the folders below. A command that needs the network, reads a drive or asks for more folders asks.",
        RunMode::Full => "Commands run without a sandbox and without asking.",
    }
}

/// Why `config`'s agent may not have `mode`: its kind's refusal, or a
/// gate's delegation (`delegated_by`, the gate naming it) for `full`.
fn refusal(config: &AgentConfig, mode: RunMode, delegated_by: Option<&str>) -> Option<String> {
    mode_refusal(config.kind, mode)
        .or_else(|| {
            delegated_by
                .filter(|_| mode == RunMode::Full)
                .map(delegate_refusal)
        })
        .map(|clause| {
            format!(
                "{} cannot have this mode: {clause}.",
                capitalised(&config.name)
            )
        })
}

/// A save refused before anything was written, `clause` saying why.
fn not_changed(config: &AgentConfig, clause: &str) -> String {
    format!(
        "{}'s console was not changed: {clause}.",
        capitalised(&config.name)
    )
}

/// The row's console: what `[run]` says (or `[tools].allow` implies), each
/// mode with why it is refused, the folders this Mac does not give it
/// (`refused`), and `locked` when this Mac cannot write the file.
pub fn console_vm(
    config: &AgentConfig,
    delegated_by: Option<&str>,
    locked: Option<String>,
    refused: Vec<ConsoleFolderVm>,
) -> AgentConsoleVm {
    AgentConsoleVm {
        mode: config.run.mode,
        paths: config.run.paths.iter().map(RunPath::written).collect(),
        modes: RunMode::ALL
            .into_iter()
            .map(|mode| ConsoleModeVm {
                mode,
                label: label(mode).to_owned(),
                description: description(mode).to_owned(),
                refused: refusal(config, mode, delegated_by),
            })
            .collect(),
        drives: config.drives.clone(),
        warning: full_access_warning(&config.name),
        unrestricted: UNRESTRICTED.to_owned(),
        refused,
        locked,
    }
}

/// `req` against the home as it reads now: the row showed this very
/// console, the mode is one this agent may have and each folder reads. The
/// file is parsed again — and its zone checked again — before it is
/// written, so this is the sentence a person reads first, not the only
/// guard.
pub fn check(
    req: &AgentConsoleReq,
    config: &AgentConfig,
    delegated_by: Option<&str>,
) -> Result<Vec<RunPath>, String> {
    let shown: Vec<String> = config.run.paths.iter().map(RunPath::written).collect();
    if req.shown_mode != config.run.mode || req.shown_paths != shown {
        return Err(CHANGED_UNDERNEATH.to_owned());
    }
    if let Some(sentence) = refusal(config, req.mode, delegated_by) {
        return Err(sentence);
    }
    if req.paths.len() > PATHS_MAX {
        return Err(not_changed(
            config,
            &format!("a console is given at most {PATHS_MAX} folders"),
        ));
    }
    let mut paths: Vec<RunPath> = Vec::with_capacity(req.paths.len());
    for text in &req.paths {
        let path =
            parse_path(text, &config.drives).map_err(|reason| not_changed(config, &reason))?;
        if paths.contains(&path) {
            return Err(not_changed(config, &format!("\"{text}\" is listed twice")));
        }
        paths.push(path);
    }
    Ok(paths)
}

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{drive, home};

    fn amelia(kind: &str, run: &str) -> AgentConfig {
        let decl = drive::parse(
            "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\n\
             owner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\n",
        )
        .expect("the drive parses");
        home::parse_agent_toml(
            &format!(
                "version = 1\nid = \"amelia\"\nname = \"amelia\"\nkind = \"{kind}\"\n\
                 matrix_user = \"@amelia:example.org\"\n{run}\n\
                 [model]\nbot = \"bot:ollama:http://localhost:11434#qwen3\"\n"
            ),
            "amelia",
            &decl,
        )
        .expect("amelia parses")
    }

    fn req(config: &AgentConfig, mode: RunMode, paths: &[&str]) -> AgentConsoleReq {
        AgentConsoleReq {
            profile_id: "p1".to_owned(),
            agent: config.id.clone(),
            mode,
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
            shown_mode: config.run.mode,
            shown_paths: config.run.paths.iter().map(RunPath::written).collect(),
        }
    }

    /// 101.5 #2, R440 08, R458: what goes without a person, by mode:
    /// nothing while supervised; in the sandbox only a run that needs
    /// neither the network, nor a drive, nor a folder beyond the console's
    /// — and that one even where the integrity rule would ask; at full
    /// access every run the integrity rule lets through.
    #[test]
    fn a_run_goes_unasked_only_inside_its_sandbox_or_at_full_access() {
        for (mode, network, reads, widens, integrity, unasked) in [
            (RunMode::Off, false, false, false, true, false),
            (RunMode::Supervised, false, false, false, true, false),
            (RunMode::Sandboxed, false, false, false, true, true),
            (RunMode::Sandboxed, false, false, false, false, true),
            (RunMode::Sandboxed, true, false, false, true, false),
            (RunMode::Sandboxed, true, false, false, false, false),
            (RunMode::Sandboxed, false, true, false, false, false),
            (RunMode::Sandboxed, false, false, true, false, false),
            (RunMode::Full, true, true, false, true, true),
            (RunMode::Full, false, false, false, true, true),
            (RunMode::Full, false, false, false, false, false),
        ] {
            assert_eq!(
                mode.runs_unasked(network, reads, widens, integrity),
                unasked,
                "{mode:?} network={network} reads={reads} widens={widens} integrity={integrity}"
            );
        }
        assert!(RunMode::Supervised.sandboxes() && RunMode::Sandboxed.sandboxes());
        assert!(!RunMode::Full.sandboxes() && !RunMode::Off.offers_run());
    }

    /// 101.5 #3: a save is checked against the file as it reads now — the
    /// console the row showed, a mode this agent may have (never full for a
    /// gate's delegate), folders that read, each once — and comes back as
    /// the folders the file will say.
    #[test]
    fn a_console_save_is_checked_against_the_file_as_it_reads_now() {
        let config = amelia(
            "specialist",
            "[run]\nmode = \"supervised\"\npaths = [\"/a\"]",
        );
        let saved = check(
            &req(&config, RunMode::Sandboxed, &["/a/", "tgdrive/site"]),
            &config,
            None,
        )
        .expect("checked");
        assert_eq!(
            saved.iter().map(RunPath::written).collect::<Vec<_>>(),
            ["/a", "tgdrive/site"]
        );
        let mut stale = req(&config, RunMode::Full, &[]);
        stale.shown_paths.clear();
        assert_eq!(
            check(&stale, &config, None),
            Err(CHANGED_UNDERNEATH.to_owned())
        );
        let mut stale = req(&config, RunMode::Full, &[]);
        stale.shown_mode = RunMode::Off;
        assert_eq!(
            check(&stale, &config, None),
            Err(CHANGED_UNDERNEATH.to_owned())
        );
        assert!(
            check(&req(&config, RunMode::Full, &[]), &config, Some("forge"))
                .expect_err("a delegate never gets full access")
                .contains("forge")
        );
        assert!(check(
            &req(&config, RunMode::Sandboxed, &[]),
            &config,
            Some("forge")
        )
        .is_ok());
        assert!(check(&req(&config, RunMode::Full, &[]), &config, None).is_ok());
        for bad in [&["~/x"][..], &["other/x"], &["/a", "/a/"]] {
            assert!(
                check(&req(&config, RunMode::Sandboxed, bad), &config, None).is_err(),
                "{bad:?}"
            );
        }
        let many: Vec<String> = (0..=PATHS_MAX).map(|n| format!("/f{n}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(check(&req(&config, RunMode::Sandboxed, &many), &config, None).is_err());
        assert!(check(
            &req(&config, RunMode::Sandboxed, &many[..PATHS_MAX]),
            &config,
            None
        )
        .is_ok());
    }

    /// 101.5 #3: the row offers every mode, and says why one is refused:
    /// full access for a gate's delegate; everything but off for a gate.
    #[test]
    fn the_row_offers_every_mode_and_says_which_this_agent_may_not_have() {
        let refused = |vm: &AgentConsoleVm| -> Vec<RunMode> {
            vm.modes
                .iter()
                .filter(|mode| mode.refused.is_some())
                .map(|mode| mode.mode)
                .collect()
        };
        let specialist = amelia(
            "specialist",
            "[run]\nmode = \"sandboxed\"\npaths = [\"/a\"]",
        );
        let vm = console_vm(&specialist, None, None, Vec::new());
        assert_eq!(vm.mode, RunMode::Sandboxed);
        assert_eq!(vm.paths, ["/a"]);
        assert_eq!(vm.modes.len(), 4);
        assert!(refused(&vm).is_empty());
        assert_eq!(
            refused(&console_vm(&specialist, Some("forge"), None, Vec::new())),
            [RunMode::Full]
        );
        let gate = amelia("gate", "");
        assert_eq!(
            refused(&console_vm(&gate, None, None, Vec::new())),
            [RunMode::Supervised, RunMode::Sandboxed, RunMode::Full]
        );
    }
}

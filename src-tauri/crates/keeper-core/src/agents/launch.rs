//! What keeper opens at launch, and the one default agent (Epic 100,
//! story 100.3, R383, R397).
//!
//! `agents.default` names one agent by its Matrix user id, or nothing. At
//! launch the webview asks once, after the accounts are hydrated, and applies
//! the answer once. Rust answers only when it knows ([`resolve`]): once every
//! signed-in account's room list has finished its first sync since launch
//! ([`LaunchReadiness`]), or as soon as the default agent's `main` room is
//! here, and never later than [`LAUNCH_READY_MS`]:
//!
//! 1. No agent session room on any live account: the Inbox, as before.
//! 2. Otherwise the Agents window. When `agents.default` names an agent whose
//!    `main` room — the person's DM with their proxy, [`proxy_rooms`]'
//!    `kind: main` — is on a live account, that room is opened and focused.
//! 3. When it names one with no such room here (another account's agent, an
//!    agent not yet spoken to, a value that is not a user id), the Agents
//!    window opens with nothing selected and [`AgentsLaunchVm::reason`] says
//!    why — beside the choice in Settings › Agents, in the launch's notice,
//!    and at WARN in `keeper.log` ([`warn_launch`]). A room still missing at
//!    the bound is one that "has not arrived yet" ([`not_arrived_sentence`]),
//!    not one that does not exist.
//!
//! Which room is Rust's alone ([`choose`]); the webview only applies it. The
//! choices a person may make the default are the agents with a `main` room
//! here, one per agent ([`AgentsLaunchVm::choices`]). When a settings file
//! decides the key, [`AgentsLaunchVm::file_set`] says so and names the file.
//!
//! [`proxy_rooms`]: crate::agents::proxy::proxy_rooms

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use matrix_sdk::ruma::UserId;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use ts_rs::TS;

use crate::agents::proxy::ProxyRoomVm;
use crate::agents::session::SessionKind;
use crate::error::CoreError;
use crate::registry;

/// The longest the launch waits for the accounts' first sync and the default
/// agent's conversation before answering from what is there (R397).
pub const LAUNCH_READY_MS: u64 = 15_000;

/// While the launch waits, how often it looks again without being told that
/// an account finished syncing: an agent's status can become readable (a
/// decryption, a late proxy list) without any account's readiness changing.
pub const LAUNCH_RECHECK_MS: u64 = 1_000;

/// The window keeper opens at launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LaunchView {
    /// The Agents window.
    Agents,
    /// The Inbox: launch unchanged.
    Inbox,
}

/// The room keeper opens at launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaunchRoomVm {
    pub account_id: String,
    pub room_id: String,
}

/// An agent a person may make the default: one with a `main` room here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DefaultAgentChoiceVm {
    /// The agent's Matrix user id — the value `agents.default` stores.
    pub agent: String,
    /// The `main` room's name (the DM, named for the agent).
    pub name: String,
}

/// What keeper opens at launch, and the default agent as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentsLaunchVm {
    pub view: LaunchView,
    /// The default agent's `main` room, opened and focused; `None` opens
    /// the window with nothing selected.
    pub room: Option<LaunchRoomVm>,
    /// `agents.default` as keeper reads it, trimmed — a settings file's value
    /// when one sets the key; `None` when no agent is chosen.
    pub default_agent: Option<String>,
    /// Why the default agent's room does not open, as a sentence; `None`
    /// when it does or when no agent is chosen.
    pub reason: Option<String>,
    /// Whether [`Self::reason`] is said where the person is looking when
    /// keeper opens (R398, R100L3-01): the Agents window opened without the
    /// default agent's room, or the launch stopped waiting before it knew
    /// whether this device has its conversation. A finished look that found
    /// no agent room opens the Inbox as before, quietly: that reason is
    /// Settings' alone.
    pub announce: bool,
    /// The agents a person may choose, by name.
    pub choices: Vec<DefaultAgentChoiceVm>,
    /// When a settings file decides `agents.default`: the sentence that says
    /// so and names the file ([`file_set_sentence`]). A choice made here is
    /// still stored, and takes effect once the file stops setting the key.
    pub file_set: Option<String>,
}

/// What the launch rule reads besides the key, gathered by
/// `AccountManager::agents_launch_facts`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchFacts {
    /// The agent session rooms on every live account: the Agents window's.
    pub agent_rooms: usize,
    /// Every live account's proxy conversations, by account id, accounts in
    /// a stable order and each account's `main` rooms first.
    pub proxies: Vec<(String, ProxyRoomVm)>,
}

/// The sentence when the default agent's `main` room is not on any
/// signed-in account here.
pub fn no_main_room_sentence(agent: &str) -> String {
    format!(
        "Your default agent {agent} has no conversation with you on this device's accounts, \
         so Agents opens with nothing selected."
    )
}

/// The sentence when `agents.default` holds something that is not a user id
/// (a row edited by hand, a file value that rotted).
pub fn not_a_user_id_sentence(stored: &str) -> String {
    format!(
        "The default agent is set to \"{stored}\", which is not a Matrix user id, so Agents \
         opens with nothing selected. Choose the default agent again."
    )
}

/// The sentence when a default is chosen but this device has no agent room.
pub const NO_AGENT_ROOMS_SENTENCE: &str =
    "You have no agent rooms on this device yet, so keeper opens the Inbox.";

/// The sentence when the launch stopped waiting at [`LAUNCH_READY_MS`] and
/// the default agent's `main` room was still not here: the accounts may not
/// have finished their first sync, or the agent's status was not yet
/// readable — not the same as there being no conversation.
pub fn not_arrived_sentence(agent: &str, view: LaunchView) -> String {
    let opens = match view {
        LaunchView::Agents => "Agents opens with nothing selected",
        LaunchView::Inbox => "keeper opens the Inbox",
    };
    format!(
        "Your conversation with your default agent {agent} has not arrived yet, so {opens}. \
         It is in Agents once it arrives."
    )
}

/// The sentence when a settings file decides `agents.default`, naming the
/// file and what it sets; `None` when no file sets the key. Shown beside the
/// choice in Settings and by the room header's "Make default" (R397): a
/// choice made there is stored, but is not what opens at launch.
pub fn file_set_sentence() -> Option<String> {
    let over = crate::config::setting_override(registry::AGENTS_DEFAULT_KEY)?;
    let value = over.value.trim();
    let (sets, opens) = if value.is_empty() {
        ("no agent".to_owned(), "no default agent opens".to_owned())
    } else {
        (value.to_owned(), format!("{value} opens"))
    };
    Some(format!(
        "{} ({}) sets {} to {sets}, so {opens} at launch whatever you choose here. Your \
         choice is kept and takes effect once that file stops setting it.",
        over.source.path.display(),
        over.source.tier.label(),
        registry::AGENTS_DEFAULT_KEY,
    ))
}

/// Decide what keeper opens at launch from `chosen` (`agents.default` as
/// stored) and `facts`.
pub fn choose(chosen: Option<&str>, facts: &LaunchFacts) -> AgentsLaunchVm {
    let default_agent = chosen
        .map(str::trim)
        .filter(|agent| !agent.is_empty())
        .map(str::to_owned);
    let choices = choices(&facts.proxies);
    let mut vm = AgentsLaunchVm {
        view: LaunchView::Inbox,
        room: None,
        default_agent: default_agent.clone(),
        reason: None,
        announce: false,
        choices,
        file_set: None,
    };
    let Some(agent) = default_agent else {
        if facts.agent_rooms > 0 {
            vm.view = LaunchView::Agents;
        }
        return vm;
    };
    if facts.agent_rooms == 0 {
        vm.reason = Some(NO_AGENT_ROOMS_SENTENCE.to_owned());
        return vm;
    }
    vm.view = LaunchView::Agents;
    if UserId::parse(agent.as_str()).is_err() {
        vm.reason = Some(not_a_user_id_sentence(&agent));
        vm.announce = true;
        return vm;
    }
    vm.room = facts
        .proxies
        .iter()
        .find(|(_, room)| room.kind == SessionKind::Main && room.agent == agent)
        .map(|(account_id, room)| LaunchRoomVm {
            account_id: account_id.clone(),
            room_id: room.room_id.clone(),
        });
    if vm.room.is_none() {
        vm.reason = Some(no_main_room_sentence(&agent));
        vm.announce = true;
    }
    vm
}

/// One choice per agent with a `main` room, by name and then id.
fn choices(proxies: &[(String, ProxyRoomVm)]) -> Vec<DefaultAgentChoiceVm> {
    let mut choices: Vec<DefaultAgentChoiceVm> = Vec::new();
    for (_, room) in proxies {
        if room.kind != SessionKind::Main || choices.iter().any(|c| c.agent == room.agent) {
            continue;
        }
        choices.push(DefaultAgentChoiceVm {
            agent: room.agent.clone(),
            name: room.name.clone(),
        });
    }
    choices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.agent.cmp(&b.agent)));
    choices
}

/// Write the launch's reason to `keeper.log` at WARN (R381): the person sees
/// it in the launch's notice and beside the choice in Settings.
pub fn warn_launch(vm: &AgentsLaunchVm) {
    if let Some(reason) = &vm.reason {
        tracing::warn!(
            view = ?vm.view,
            reason = reason.as_str(),
            "agents: the default agent's room does not open at launch"
        );
    }
}

/// The default agent as Settings › Agents and the room header show it, from
/// what is here now: [`choose`] over the key as keeper reads it, with the
/// file that decides it when one does. Never waits.
pub fn read(data_dir: &Path, facts: &LaunchFacts) -> Result<AgentsLaunchVm, CoreError> {
    let chosen = registry::get_agents_default(data_dir)?;
    let mut vm = choose(chosen.as_deref(), facts);
    vm.file_set = file_set_sentence();
    Ok(vm)
}

/// Whether `vm`'s default agent is a user id whose `main` room is not here:
/// at the bound, a room that "has not arrived yet".
fn awaits_room(vm: &AgentsLaunchVm) -> bool {
    vm.room.is_none()
        && vm
            .default_agent
            .as_deref()
            .is_some_and(|agent| UserId::parse(agent).is_ok())
}

/// Which signed-in accounts' room lists have finished their first sync since
/// launch (R397): the merged inbox subscription names the accounts it starts
/// ([`Self::expect`]), each account's room-list producer says when its
/// list's loading state first reads loaded ([`Self::loaded`]), and an account
/// that could not start or was signed out stops being waited for
/// ([`Self::gone`]). Ready once a subscription started and nothing it
/// started is still pending. Every change counts a generation, so the launch
/// can tell whether readiness held for the whole of a look at the rooms
/// (R409).
#[derive(Debug, Clone)]
pub struct LaunchReadiness(Arc<watch::Sender<ReadinessState>>);

/// The accounts [`LaunchReadiness`] is waiting for and has seen loaded.
#[derive(Debug, Default)]
pub struct ReadinessState {
    started: bool,
    pending: BTreeSet<String>,
    loaded: BTreeSet<String>,
    generation: u64,
}

impl Default for LaunchReadiness {
    fn default() -> Self {
        Self(Arc::new(watch::channel(ReadinessState::default()).0))
    }
}

impl LaunchReadiness {
    /// A subscription is starting `accounts`: each one not yet loaded is
    /// waited for; an account no longer among them is not.
    pub fn expect(&self, accounts: impl IntoIterator<Item = String>) {
        self.0.send_modify(|state| {
            state.started = true;
            state.pending = accounts
                .into_iter()
                .filter(|account| !state.loaded.contains(account))
                .collect();
            state.generation += 1;
        });
    }

    /// `account`'s room list has finished its first sync.
    pub fn loaded(&self, account: &str) {
        self.0.send_if_modified(|state| {
            let was_pending = state.pending.remove(account);
            let changed = state.loaded.insert(account.to_owned()) || was_pending;
            state.generation += u64::from(changed);
            changed
        });
    }

    /// `account` could not start or was signed out: not waited for.
    pub fn gone(&self, account: &str) {
        self.0.send_if_modified(|state| {
            let was_loaded = state.loaded.remove(account);
            let changed = state.pending.remove(account) || was_loaded;
            state.generation += u64::from(changed);
            changed
        });
    }

    /// `Some(generation)` once every account the subscription started has
    /// finished its first sync; the generation changes with every change in
    /// readiness, so two equal answers mean nothing changed between them.
    pub fn ready(&self) -> Option<u64> {
        let state = self.0.borrow();
        (state.started && state.pending.is_empty()).then_some(state.generation)
    }

    /// Told of every change in readiness.
    pub fn subscribe(&self) -> watch::Receiver<ReadinessState> {
        self.0.subscribe()
    }
}

/// What [`resolve`] reads while the launch waits: the key, the readiness,
/// the rooms here, and a wake-up when any may have changed.
pub trait LaunchSource {
    /// `agents.default` as keeper reads it now.
    fn chosen(&self) -> Result<Option<String>, CoreError>;
    /// [`LaunchReadiness::ready`]: `Some(generation)` once every signed-in
    /// account's room list has finished its first sync.
    fn ready(&self) -> Option<u64>;
    /// The rooms here now. May be slow (a proxy's status read from the
    /// server); [`resolve`]'s bound covers it.
    fn facts(&self) -> impl Future<Output = LaunchFacts> + Send;
    /// Resolves when something [`resolve`] reads may have changed.
    fn changed(&mut self) -> impl Future<Output = ()> + Send;
}

/// What keeper opens at launch, answered only when Rust knows (R397, R409):
/// as soon as the default agent's `main` room is here, or once a look at the
/// rooms began and ended with every signed-in account's room list synced
/// and unchanged — then [`choose`] over that look is the answer, and a
/// default with no room here is one with no conversation on this device. A
/// look that began before the last account was ready certifies nothing: the
/// rooms are looked at again. `bound` covers everything, the looks included:
/// at the bound, [`choose`] runs over the last look that finished (nothing,
/// when none did) without starting another, and a still-missing room's
/// reason is [`not_arrived_sentence`]. The key is read again at each look, so
/// a choice saved while the launch waits is the one it answers with.
pub async fn resolve<S: LaunchSource>(
    source: &mut S,
    bound: Duration,
) -> Result<AgentsLaunchVm, CoreError> {
    let mut seen = LaunchFacts::default();
    if let Ok(settled) = tokio::time::timeout(bound, settle(source, &mut seen)).await {
        return settled;
    }
    let chosen = source.chosen()?;
    let mut vm = choose(chosen.as_deref(), &seen);
    if awaits_room(&vm) {
        if let Some(agent) = vm.default_agent.as_deref() {
            vm.reason = Some(not_arrived_sentence(agent, vm.view));
            // Not knowing is not knowing there is none: said on either
            // window, the Inbox included.
            vm.announce = true;
        }
    }
    Ok(vm)
}

/// [`resolve`]'s looks, unbounded; each finished look is left in `seen` for
/// the answer at the bound.
async fn settle<S: LaunchSource>(
    source: &mut S,
    seen: &mut LaunchFacts,
) -> Result<AgentsLaunchVm, CoreError> {
    loop {
        let ready_before = source.ready();
        let facts = source.facts().await;
        let chosen = source.chosen()?;
        let vm = choose(chosen.as_deref(), &facts);
        *seen = facts;
        let ready_after = source.ready();
        if vm.room.is_some() || (ready_before.is_some() && ready_before == ready_after) {
            return Ok(vm);
        }
        if ready_before == ready_after {
            source.changed().await;
        }
    }
}

/// Why the default agent was not stored, or not read back once it was.
#[derive(Debug)]
pub enum SetDefaultError {
    /// The value is not a Matrix user id.
    NotAUserId(String),
    /// The settings table refused the write.
    Store(CoreError),
    /// The value was stored, but the answer could not be read again.
    ReadBack(CoreError),
}

impl fmt::Display for SetDefaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetDefaultError::NotAUserId(value) => write!(
                f,
                "\"{value}\" is not a Matrix user id, so it cannot be the default agent. An \
                 agent is named like @nixi:example.org."
            ),
            SetDefaultError::Store(error) => {
                write!(f, "keeper could not save the default agent: {error}")
            }
            SetDefaultError::ReadBack(error) => write!(
                f,
                "keeper saved the default agent but could not read it back: {error}. Open \
                 Settings again to see it."
            ),
        }
    }
}

impl std::error::Error for SetDefaultError {}

/// Store `agent` as the default agent, or clear it with `None` (or blank),
/// and answer with [`read`] over `facts`. A value that is not a Matrix user
/// id is refused before anything is written; every failure — the refusal,
/// the write, the read after it — is logged at WARN (R381) and its sentence
/// is the caller's to show beside the control.
pub fn set_default(
    data_dir: &Path,
    agent: Option<&str>,
    facts: &LaunchFacts,
) -> Result<AgentsLaunchVm, SetDefaultError> {
    let agent = agent.map(str::trim).filter(|agent| !agent.is_empty());
    let result = match agent {
        Some(id) if UserId::parse(id).is_err() => Err(SetDefaultError::NotAUserId(id.to_owned())),
        _ => registry::set_agents_default(data_dir, agent).map_err(SetDefaultError::Store),
    }
    .and_then(|()| read(data_dir, facts).map_err(SetDefaultError::ReadBack));
    if let Err(error) = &result {
        tracing::warn!(%error, "agents: the default agent was not saved, or not read back");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(room_id: &str, agent: &str, kind: SessionKind, name: &str) -> ProxyRoomVm {
        ProxyRoomVm {
            room_id: room_id.to_owned(),
            name: name.to_owned(),
            kind,
            agent: agent.to_owned(),
            allowed: None,
        }
    }

    fn facts(agent_rooms: usize, proxies: &[(&str, ProxyRoomVm)]) -> LaunchFacts {
        LaunchFacts {
            agent_rooms,
            proxies: proxies
                .iter()
                .map(|(account, room)| ((*account).to_owned(), room.clone()))
                .collect(),
        }
    }

    const NIXI: &str = "@nixi:example.org";
    const TALA: &str = "@tala:example.org";

    #[test]
    fn the_default_agents_main_room_opens_and_not_its_conversations() {
        let facts = facts(
            4,
            &[
                ("a1", room("!tala:x", TALA, SessionKind::Main, "Tala")),
                (
                    "a1",
                    room("!talk:x", NIXI, SessionKind::Conversation, "Plans"),
                ),
                ("a2", room("!dm:x", NIXI, SessionKind::Main, "Nixi")),
            ],
        );
        let vm = choose(Some(NIXI), &facts);
        assert_eq!(vm.view, LaunchView::Agents);
        assert_eq!(
            vm.room,
            Some(LaunchRoomVm {
                account_id: "a2".into(),
                room_id: "!dm:x".into()
            })
        );
        assert_eq!(vm.reason, None);
        assert_eq!(vm.default_agent.as_deref(), Some(NIXI));
    }

    #[test]
    fn an_agent_with_only_conversations_here_opens_agents_with_nothing_selected_and_says_why() {
        let facts = facts(
            2,
            &[
                ("a1", room("!tala:x", TALA, SessionKind::Main, "Tala")),
                (
                    "a1",
                    room("!talk:x", NIXI, SessionKind::Conversation, "Plans"),
                ),
            ],
        );
        let vm = choose(Some(NIXI), &facts);
        assert_eq!(vm.view, LaunchView::Agents);
        assert_eq!(vm.room, None);
        assert_eq!(vm.reason, Some(no_main_room_sentence(NIXI)));
        assert!(vm.announce);
    }

    #[test]
    fn no_agent_rooms_leaves_launch_on_the_inbox_whatever_the_default() {
        for chosen in [None, Some(NIXI)] {
            let vm = choose(chosen, &facts(0, &[]));
            assert_eq!(vm.view, LaunchView::Inbox, "{chosen:?}");
            assert_eq!(vm.room, None);
            // A finished look that found no agent room is the Inbox as
            // before, quietly (R398); the reason is Settings' alone.
            assert!(!vm.announce, "{chosen:?}");
        }
        assert_eq!(choose(None, &facts(0, &[])).reason, None);
        assert_eq!(
            choose(Some(NIXI), &facts(0, &[])).reason.as_deref(),
            Some(NO_AGENT_ROOMS_SENTENCE)
        );
    }

    #[test]
    fn agent_rooms_without_a_default_open_agents_with_nothing_selected_and_no_reason() {
        // An agent room that is not a proxy conversation (a gate, another
        // person's agent) still counts: the Agents window lists it.
        for chosen in [None, Some(""), Some("   ")] {
            let vm = choose(chosen, &facts(1, &[]));
            assert_eq!(vm.view, LaunchView::Agents, "{chosen:?}");
            assert_eq!(vm.room, None);
            assert_eq!(vm.reason, None);
            assert_eq!(vm.default_agent, None);
        }
    }

    #[test]
    fn a_stored_value_that_is_not_a_user_id_selects_nothing_and_says_so() {
        let facts = facts(1, &[("a1", room("!dm:x", NIXI, SessionKind::Main, "Nixi"))]);
        let vm = choose(Some("nixi"), &facts);
        assert_eq!(vm.view, LaunchView::Agents);
        assert_eq!(vm.room, None);
        assert_eq!(vm.reason, Some(not_a_user_id_sentence("nixi")));
    }

    #[test]
    fn the_choices_are_one_per_agent_with_a_main_room_by_name() {
        let facts = facts(
            4,
            &[
                ("a1", room("!n1:x", NIXI, SessionKind::Main, "Nixi")),
                ("a2", room("!n2:x", NIXI, SessionKind::Main, "Nixi")),
                (
                    "a1",
                    room(
                        "!c:x",
                        "@lumi:example.org",
                        SessionKind::Conversation,
                        "Lumi",
                    ),
                ),
                ("a1", room("!t:x", TALA, SessionKind::Main, "Atala")),
            ],
        );
        let vm = choose(None, &facts);
        assert_eq!(
            vm.choices,
            vec![
                DefaultAgentChoiceVm {
                    agent: TALA.into(),
                    name: "Atala".into()
                },
                DefaultAgentChoiceVm {
                    agent: NIXI.into(),
                    name: "Nixi".into()
                },
            ]
        );
    }

    #[test]
    fn set_default_refuses_what_is_not_a_user_id_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let none = LaunchFacts::default();
        set_default(dir.path(), Some(NIXI), &none).expect("a user id is stored");
        let error = set_default(dir.path(), Some("nixi@example.org"), &none).expect_err("refused");
        assert!(matches!(error, SetDefaultError::NotAUserId(_)), "{error}");
        assert!(error.to_string().contains("nixi@example.org"), "{error}");
        assert_eq!(
            registry::get_agents_default(dir.path()).expect("read"),
            Some(NIXI.to_owned())
        );
        set_default(dir.path(), Some("  "), &none).expect("blank clears");
        assert_eq!(
            registry::get_agents_default(dir.path()).expect("read"),
            None
        );
    }

    #[test]
    fn a_file_that_sets_the_default_is_what_the_answer_shows_and_names() {
        use crate::config::{install_for_test, layers_from, LayerTier};
        let dir = tempfile::tempdir().expect("tempdir");
        let here = facts(
            2,
            &[
                ("a1", room("!n:x", NIXI, SessionKind::Main, "Nixi")),
                ("a1", room("!t:x", TALA, SessionKind::Main, "Tala")),
            ],
        );
        let saved = set_default(dir.path(), Some(NIXI), &here).expect("stored");
        assert_eq!(saved.file_set, None);
        assert_eq!(saved.default_agent.as_deref(), Some(NIXI));

        let _file = install_for_test(layers_from(&[(
            "agents.default",
            TALA,
            LayerTier::UserGlobal,
        )]));
        // Making Nixi the default stores her, but the file's Tala is what
        // opens — and the answer says so, naming the file.
        let vm = set_default(dir.path(), Some(NIXI), &here).expect("stored");
        assert_eq!(vm.default_agent.as_deref(), Some(TALA));
        assert_eq!(
            vm.room.as_ref().map(|room| room.room_id.as_str()),
            Some("!t:x")
        );
        let sentence = vm.file_set.expect("a file decides the key");
        assert!(sentence.contains("/test/user.toml"), "{sentence}");
        assert!(sentence.contains(TALA), "{sentence}");
        // Clearing it is stored too, and still not what opens.
        let cleared = set_default(dir.path(), None, &here).expect("cleared");
        assert_eq!(cleared.default_agent.as_deref(), Some(TALA));
        assert!(cleared.file_set.is_some());
    }

    #[test]
    fn readiness_waits_for_every_started_account_and_not_for_one_that_could_not_start() {
        let readiness = LaunchReadiness::default();
        assert_eq!(readiness.ready(), None, "nothing started yet");
        readiness.loaded("a1");
        readiness.expect(["a1".to_owned(), "a2".to_owned(), "a3".to_owned()]);
        assert_eq!(readiness.ready(), None, "a2 and a3 are still syncing");
        readiness.loaded("a2");
        assert_eq!(readiness.ready(), None, "a3 is still syncing");
        readiness.gone("a3");
        let first = readiness.ready().expect("ready");
        // A re-subscribe waits only for an account it has not seen loaded,
        // and being ready again is not the readiness a look began under.
        readiness.expect(["a1".to_owned(), "a2".to_owned(), "a4".to_owned()]);
        assert_eq!(readiness.ready(), None, "a4 is new");
        readiness.loaded("a4");
        let again = readiness.ready().expect("ready again");
        assert_ne!(first, again);
    }

    /// A day on the paused clock: past any bound, so a look that takes it is
    /// one that does not come back.
    const STUCK: Duration = Duration::from_secs(86_400);

    /// A launch source on a script, on the paused clock from `start`: a look
    /// sees the last `facts` entry due when it begins and takes `scan` (one
    /// begun at or after `stuck_from` does not come back), the accounts are
    /// ready from `ready_at`, and each `changed` is one second.
    struct Script {
        chosen: Option<&'static str>,
        start: tokio::time::Instant,
        ready_at: Duration,
        facts: Vec<(Duration, LaunchFacts)>,
        scan: Duration,
        stuck_from: Duration,
    }

    impl Script {
        fn new(
            chosen: Option<&'static str>,
            ready_at: u64,
            facts: Vec<(u64, LaunchFacts)>,
        ) -> Self {
            Self {
                chosen,
                start: tokio::time::Instant::now(),
                ready_at: Duration::from_secs(ready_at),
                facts: facts
                    .into_iter()
                    .map(|(at, facts)| (Duration::from_secs(at), facts))
                    .collect(),
                scan: Duration::ZERO,
                stuck_from: Duration::MAX,
            }
        }
    }

    impl LaunchSource for Script {
        fn chosen(&self) -> Result<Option<String>, CoreError> {
            Ok(self.chosen.map(str::to_owned))
        }

        fn ready(&self) -> Option<u64> {
            (self.start.elapsed() >= self.ready_at).then_some(1)
        }

        fn facts(&self) -> impl Future<Output = LaunchFacts> + Send {
            let now = self.start.elapsed();
            let seen = self
                .facts
                .iter()
                .rev()
                .find(|(at, _)| *at <= now)
                .map(|(_, facts)| facts.clone())
                .unwrap_or_default();
            let takes = if now >= self.stuck_from {
                STUCK
            } else {
                self.scan
            };
            async move {
                if !takes.is_zero() {
                    tokio::time::sleep(takes).await;
                }
                seen
            }
        }

        async fn changed(&mut self) {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    const BOUND: Duration = Duration::from_millis(LAUNCH_READY_MS);

    fn nixi_room() -> Option<LaunchRoomVm> {
        Some(LaunchRoomVm {
            account_id: "a2".into(),
            room_id: "!n:x".into(),
        })
    }

    #[tokio::test(start_paused = true)]
    async fn the_default_agents_room_arriving_late_within_the_bound_opens_it() {
        let tala = ("a1", room("!t:x", TALA, SessionKind::Main, "Tala"));
        let nixi = ("a2", room("!n:x", NIXI, SessionKind::Main, "Nixi"));
        let mut source = Script::new(
            Some(NIXI),
            10,
            vec![
                (0, facts(1, std::slice::from_ref(&tala))),
                (3, facts(2, &[tala.clone(), nixi])),
            ],
        );
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), Duration::from_secs(3));
        assert_eq!(vm.room, nixi_room());
        assert_eq!(vm.reason, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_room_that_never_arrives_answers_at_the_bound_saying_it_has_not_arrived() {
        let mut source = Script::new(
            Some(NIXI),
            u64::MAX,
            vec![(
                0,
                facts(1, &[("a1", room("!t:x", TALA, SessionKind::Main, "Tala"))]),
            )],
        );
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), BOUND);
        assert_eq!(vm.view, LaunchView::Agents);
        assert_eq!(vm.room, None);
        assert_eq!(
            vm.reason,
            Some(not_arrived_sentence(NIXI, LaunchView::Agents))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_first_look_that_never_comes_back_is_answered_at_the_bound() {
        let mut source = Script::new(
            Some(NIXI),
            0,
            vec![(
                0,
                facts(1, &[("a2", room("!n:x", NIXI, SessionKind::Main, "Nixi"))]),
            )],
        );
        source.stuck_from = Duration::ZERO;
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), BOUND);
        // Nothing was seen: no agent room, so the Inbox, and the default's
        // conversation has not arrived — not "there is none".
        assert_eq!(vm.view, LaunchView::Inbox);
        assert_eq!(vm.room, None);
        assert_eq!(
            vm.reason,
            Some(not_arrived_sentence(NIXI, LaunchView::Inbox))
        );
        assert!(vm.announce, "a launch that did not finish looking says so");
    }

    #[tokio::test(start_paused = true)]
    async fn a_later_look_that_never_comes_back_is_answered_at_the_bound_from_the_last_that_did() {
        let tala = ("a1", room("!t:x", TALA, SessionKind::Main, "Tala"));
        let nixi = ("a2", room("!n:x", NIXI, SessionKind::Main, "Nixi"));
        let mut source = Script::new(
            Some(NIXI),
            u64::MAX,
            vec![
                (0, facts(1, std::slice::from_ref(&tala))),
                (5, facts(2, &[tala.clone(), nixi])),
            ],
        );
        // The looks at 0 s and 1 s come back; the one begun at 2 s never
        // does, and no look begins at the bound.
        source.stuck_from = Duration::from_secs(2);
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), BOUND);
        assert_eq!(vm.view, LaunchView::Agents);
        assert_eq!(vm.room, None);
        assert_eq!(
            vm.reason,
            Some(not_arrived_sentence(NIXI, LaunchView::Agents))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_account_ready_while_an_older_look_ran_is_looked_at_again() {
        // Account a2 brings Nixi's DM at 1 s and is ready at 2 s, while the
        // look begun at 0 s — which saw only a1 — is still out. That look
        // certifies nothing; the next one finds the DM.
        let tala = ("a1", room("!t:x", TALA, SessionKind::Main, "Tala"));
        let nixi = ("a2", room("!n:x", NIXI, SessionKind::Main, "Nixi"));
        let mut source = Script::new(
            Some(NIXI),
            2,
            vec![
                (0, facts(1, std::slice::from_ref(&tala))),
                (1, facts(2, &[tala.clone(), nixi])),
            ],
        );
        source.scan = Duration::from_secs(3);
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), Duration::from_secs(6));
        assert_eq!(vm.room, nixi_room());
        assert_eq!(vm.reason, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_account_slow_to_sync_is_waited_for() {
        // The first account is synced and has no agent room; the second
        // brings its agent rooms when it finishes, four seconds in.
        let mut source = Script::new(
            None,
            4,
            vec![
                (0, facts(0, &[])),
                (
                    4,
                    facts(2, &[("a2", room("!n:x", NIXI, SessionKind::Main, "Nixi"))]),
                ),
            ],
        );
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), Duration::from_secs(4));
        assert_eq!(vm.view, LaunchView::Agents);
    }

    #[tokio::test(start_paused = true)]
    async fn with_every_account_synced_a_default_without_a_room_here_has_no_conversation() {
        let mut source = Script::new(
            Some(NIXI),
            2,
            vec![(
                0,
                facts(1, &[("a1", room("!t:x", TALA, SessionKind::Main, "Tala"))]),
            )],
        );
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        assert_eq!(vm.view, LaunchView::Agents);
        assert_eq!(vm.reason, Some(no_main_room_sentence(NIXI)));
    }

    #[tokio::test(start_paused = true)]
    async fn with_every_account_synced_and_no_default_the_launch_answers_at_once() {
        let mut source = Script::new(None, 0, vec![(0, facts(0, &[]))]);
        let started = tokio::time::Instant::now();
        let vm = resolve(&mut source, BOUND).await.expect("answered");
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(vm.view, LaunchView::Inbox);
    }
}

//! The one default agent (story 100.3, R383, R397, R463, R488), read from the
//! agents' databases (108.2).
//!
//! `agents.default` names one agent by its actor id in the databases this
//! device holds — or nothing. Before 108.2 it named an agent by its Matrix
//! user id; such a value still reads as the agent its localpart names
//! (`@nixi:example.org` is `nixi`), as [`sessions::default_session`] reads it
//! for launch and the docks. What it opens is the agent's main conversation
//! ([`crate::hq::Hq::default_session`]); this module is what Settings ›
//! Agents and the launch's chooser show of it: the choice, the agents one may
//! choose ([`DefaultAgentVm::choices`] — every agent of every held database),
//! why the chosen one opens nothing, a settings file that decides the key,
//! and whether to ask which agent keeper opens when it starts.
//!
//! [`DefaultAgentSeen`] and [`layers_observer`] tell the webview when the
//! account's settings files moved the default, since no settings row moves
//! then (R488).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::hq::sessions::{self, AgentChoiceVm};
use crate::hq::{Hq, HqError};
use crate::registry;

/// keeper's own version: a chooser put away stays away until keeper is
/// updated (R463).
pub const KEEPER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An agent a person may make the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DefaultAgentChoiceVm {
    /// The agent's actor id — the value `agents.default` stores.
    pub agent: String,
    /// The agent's name.
    pub name: String,
}

/// The default agent as Settings › Agents and the launch's chooser show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DefaultAgentVm {
    /// The agent `agents.default` names, as an actor id — a settings file's
    /// value when one sets the key; `None` when no agent is chosen.
    pub default_agent: Option<String>,
    /// Why the default agent opens nothing, as a sentence; `None` when it is
    /// one of [`Self::choices`] or when none is chosen.
    pub reason: Option<String>,
    /// The agents a person may choose, by name: every agent of every
    /// database this device holds.
    pub choices: Vec<DefaultAgentChoiceVm>,
    /// Whether keeper asks, where the person is looking, which agent it
    /// opens when it starts (R463): no default is chosen, there is an agent
    /// to choose, no settings file decides the key, and the person did not
    /// put the chooser away in this version of keeper.
    pub chooser: bool,
    /// When a settings file decides `agents.default`: the sentence that says
    /// so and names the file ([`file_set_sentence`]). A choice made here is
    /// still stored, and takes effect once the file stops setting the key.
    pub file_set: Option<String>,
}

/// The agent a stored `agents.default` names: its actor id, or — for a value
/// stored before 108.2 — the localpart of the Matrix user id it holds.
pub fn actor_of(stored: &str) -> &str {
    let stored = stored.trim();
    stored
        .strip_prefix('@')
        .and_then(|rest| rest.split(':').next())
        .unwrap_or(stored)
}

/// The sentence when the default agent is none of the agents this device's
/// databases hold.
pub fn not_held_sentence(agent: &str) -> String {
    format!(
        "Your default agent {agent} is in none of the agents' databases this device holds, so \
         Agents opens with nothing selected."
    )
}

/// The sentence when a settings file decides `agents.default`, naming the
/// file and what it sets; `None` when no file sets the key. A choice made
/// here is stored, but is not what opens at launch.
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

/// What Settings shows of `chosen` (`agents.default` as keeper reads it) over
/// `choices`, in keeper `version`, with `put_away` the version in which the
/// person put the chooser away.
pub fn decide(
    chosen: Option<&str>,
    choices: &[AgentChoiceVm],
    file_set: Option<String>,
    put_away: Option<&str>,
    version: &str,
) -> DefaultAgentVm {
    let default_agent = chosen
        .map(actor_of)
        .filter(|agent| !agent.is_empty())
        .map(str::to_owned);
    let choices: Vec<DefaultAgentChoiceVm> = choices
        .iter()
        .map(|choice| DefaultAgentChoiceVm {
            agent: choice.agent_actor.clone(),
            name: choice.agent_name.clone(),
        })
        .collect();
    let reason = default_agent
        .as_deref()
        .filter(|agent| !choices.iter().any(|choice| choice.agent == *agent))
        .map(not_held_sentence);
    let chooser = default_agent.is_none()
        && !choices.is_empty()
        && file_set.is_none()
        && put_away != Some(version);
    DefaultAgentVm {
        default_agent,
        reason,
        choices,
        chooser,
        file_set,
    }
}

/// The person put the launch's chooser away (R463): it is not shown again
/// until keeper is updated. A failed write is a WARN line (R381); its
/// sentence is the caller's to show.
pub fn dismiss_chooser(data_dir: &Path) -> Result<(), CoreError> {
    let result = registry::set_default_agent_chooser_dismissed(data_dir, KEEPER_VERSION);
    if let Err(error) = &result {
        tracing::warn!(%error, "agents: the default agent's chooser was not put away");
    }
    result
}

/// Why the default agent was not stored, or not read back once it was.
#[derive(Debug)]
pub enum SetDefaultError {
    /// The value is not one agent's id.
    NotAnAgent(String),
    /// The settings table refused the write.
    Store(CoreError),
    /// The value was stored, but the answer could not be read again.
    ReadBack(HqError),
}

impl fmt::Display for SetDefaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetDefaultError::NotAnAgent(value) => write!(
                f,
                "\"{value}\" is not an agent's name, so it cannot be the default agent. Choose \
                 one of the agents listed."
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

/// Store `agent` (an actor id) as the default agent, or clear it with `None`
/// (or blank). A value with whitespace in it or a Matrix-style `@`/`:` — no
/// actor id — is refused before anything is written. Every failure is logged
/// at WARN (R381) and its sentence is the caller's to show.
pub fn store(data_dir: &Path, agent: Option<&str>) -> Result<(), SetDefaultError> {
    let agent = agent.map(str::trim).filter(|agent| !agent.is_empty());
    let result = match agent {
        Some(id) if id.contains(|c: char| c.is_whitespace() || c == '@' || c == ':') => {
            Err(SetDefaultError::NotAnAgent(id.to_owned()))
        }
        _ => registry::set_agents_default(data_dir, agent).map_err(SetDefaultError::Store),
    };
    if let Err(error) = &result {
        tracing::warn!(%error, "agents: the default agent was not saved");
    }
    result
}

impl Hq {
    /// The default agent as Settings › Agents and the launch's chooser show
    /// it, from what the replicas hold now (never waits).
    pub fn default_agent(&self) -> Result<DefaultAgentVm, HqError> {
        let data_dir = self.data_dir()?;
        let chosen = registry::get_agents_default(&data_dir)?;
        let choices = sessions::choices(&self.sources(), &sessions::last_choices(&data_dir));
        let put_away = registry::get_default_agent_chooser_dismissed(&data_dir)?;
        Ok(decide(
            chosen.as_deref(),
            &choices,
            file_set_sentence(),
            put_away.as_deref(),
            KEEPER_VERSION,
        ))
    }

    /// The person put the launch's chooser away ([`dismiss_chooser`]).
    pub fn dismiss_default_chooser(&self) -> Result<(), HqError> {
        dismiss_chooser(&self.data_dir()?).map_err(HqError::from)
    }

    /// Make `agent` the default, or clear it with `None`, and answer with
    /// the default agent as it now reads.
    pub fn set_default_agent(
        &self,
        agent: Option<&str>,
    ) -> Result<DefaultAgentVm, SetDefaultError> {
        let data_dir = self
            .data_dir()
            .map_err(|error| SetDefaultError::Store(CoreError::Internal(error.to_string())))?;
        store(&data_dir, agent)?;
        self.default_agent().map_err(|error| {
            tracing::warn!(%error, "agents: the default agent was saved but not read back");
            SetDefaultError::ReadBack(error)
        })
    }
}

/// The default agent as keeper reads it at one moment — a settings file
/// that sets `agents.default` first (the account's `keeper.toml` and
/// `keeper.<device>.toml` included), then this device's table — taken
/// before something that may move it: the account's layers installed or
/// cleared, or a synced settings row written (R488). [`Self::moved`] answers
/// whether that something moved it, which is when the webview is told
/// (`keeper://agents-default-changed`) — never for a row written beneath a
/// file that keeps deciding the key, and never for layers reinstalled
/// unchanged.
#[derive(Debug)]
pub struct DefaultAgentSeen(Result<Option<String>, CoreError>);

impl DefaultAgentSeen {
    /// The default agent as it reads now.
    pub fn now(data_dir: &Path) -> Self {
        Self(registry::get_agents_default(data_dir))
    }

    /// Whether the default agent reads differently now than when this was
    /// taken. A read that fails, then or now, is a WARN line (R381) and
    /// counts as moved: the webview reads it again and says why it cannot.
    pub fn moved(&self, data_dir: &Path) -> bool {
        match (&self.0, registry::get_agents_default(data_dir)) {
            (Ok(before), Ok(after)) => *before != after,
            (before, after) => {
                if let Err(error) = before {
                    tracing::warn!(%error, "agents: the default agent did not read before a change");
                }
                if let Err(error) = after {
                    tracing::warn!(%error, "agents: the default agent did not read after a change");
                }
                true
            }
        }
    }
}

/// The one account-layer transition keeper installs
/// ([`crate::config::set_account_layers_observer`]; R488 × R495 05). Every
/// install or clear of the account's layers, once the new layers answer
/// every read, runs in this order and each once: `consent` — diagnostics
/// consent re-read — then `default_moved` when the default agent reads
/// differently than before. `default_dir` is where the table is, `None`
/// before the app is up (nothing listens then, so nothing is compared).
pub fn layers_observer(
    consent: impl Fn() + Send + Sync + 'static,
    default_dir: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
    default_moved: impl Fn() + Send + Sync + 'static,
) -> crate::config::AccountLayersObserver {
    let effects = Arc::new((consent, default_moved));
    Box::new(move || {
        let seen = default_dir().map(|dir| {
            let seen = DefaultAgentSeen::now(&dir);
            (dir, seen)
        });
        let effects = Arc::clone(&effects);
        Box::new(move || {
            let (consent, default_moved) = &*effects;
            consent();
            if let Some((dir, seen)) = seen {
                if seen.moved(&dir) {
                    default_moved();
                }
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIXIE: &str = "nixie";
    const TALA: &str = "tala";

    fn choice(agent: &str, name: &str) -> AgentChoiceVm {
        AgentChoiceVm {
            agent_actor: agent.to_owned(),
            agent_name: name.to_owned(),
            agent_mark: name.chars().next().map(String::from).unwrap_or_default(),
            databases: vec!["tgdrive".to_owned()],
            database: Some("tgdrive".to_owned()),
            ask: false,
        }
    }

    /// 108.2: the choices are the agents of the held databases; a stored
    /// actor id is the default, and a Matrix user id stored before 108.2
    /// reads as the agent its localpart names.
    #[test]
    fn the_default_is_an_agent_of_the_databases_and_an_old_user_id_reads_as_its_localpart() {
        let held = [choice(NIXIE, "Nixie"), choice(TALA, "Tala")];
        let vm = decide(Some(NIXIE), &held, None, None, "1.0.0");
        assert_eq!(vm.default_agent.as_deref(), Some(NIXIE));
        assert_eq!(vm.reason, None);
        assert!(!vm.chooser);
        assert_eq!(
            vm.choices,
            [
                DefaultAgentChoiceVm {
                    agent: NIXIE.to_owned(),
                    name: "Nixie".to_owned()
                },
                DefaultAgentChoiceVm {
                    agent: TALA.to_owned(),
                    name: "Tala".to_owned()
                },
            ]
        );
        let old = decide(Some("@nixie:example.org"), &held, None, None, "1.0.0");
        assert_eq!(old.default_agent.as_deref(), Some(NIXIE));
        assert_eq!(old.reason, None);
    }

    /// An agent none of the held databases names opens nothing, and says
    /// why; with no default and agents to choose, keeper asks which.
    #[test]
    fn a_default_not_held_says_why_and_no_default_asks_which() {
        let held = [choice(NIXIE, "Nixie")];
        let gone = decide(Some("dixie"), &held, None, None, "1.0.0");
        assert_eq!(gone.reason, Some(not_held_sentence("dixie")));
        let none = decide(None, &held, None, None, "1.0.0");
        assert!(none.chooser);
        assert_eq!(none.reason, None);
        assert!(
            !decide(None, &[], None, None, "1.0.0").chooser,
            "nothing to choose"
        );
    }

    /// R463: a chooser put away stays away in this version of keeper and is
    /// asked again after an update; a settings file that decides the key
    /// leaves nothing to choose.
    #[test]
    fn a_chooser_put_away_stays_away_until_keeper_is_updated() {
        let held = [choice(NIXIE, "Nixie")];
        assert!(!decide(None, &held, None, Some("1.0.0"), "1.0.0").chooser);
        assert!(decide(None, &held, None, Some("1.0.0"), "1.1.0").chooser);
        assert!(!decide(None, &held, Some("a file".to_owned()), None, "1.0.0").chooser);

        let dir = tempfile::tempdir().expect("tempdir");
        dismiss_chooser(dir.path()).expect("put away");
        assert_eq!(
            registry::get_default_agent_chooser_dismissed(dir.path()).expect("read"),
            Some(KEEPER_VERSION.to_owned())
        );
    }

    #[test]
    fn store_refuses_what_is_no_actor_id_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        store(dir.path(), Some(NIXIE)).expect("an actor id is stored");
        for refused in ["@nixie:example.org", "nix ie", "nixie:x"] {
            let error = store(dir.path(), Some(refused)).expect_err("refused");
            assert!(matches!(error, SetDefaultError::NotAnAgent(_)), "{error}");
            assert!(error.to_string().contains(refused), "{error}");
        }
        assert_eq!(
            registry::get_agents_default(dir.path()).expect("read"),
            Some(NIXIE.to_owned())
        );
        store(dir.path(), Some("  ")).expect("blank clears");
        assert_eq!(
            registry::get_agents_default(dir.path()).expect("read"),
            None
        );
    }

    #[test]
    fn a_file_that_sets_the_default_is_named() {
        use crate::config::{install_for_test, layers_from, LayerTier};
        assert_eq!(file_set_sentence(), None);
        let _file = install_for_test(layers_from(&[(
            "agents.default",
            TALA,
            LayerTier::UserGlobal,
        )]));
        let sentence = file_set_sentence().expect("a file decides the key");
        assert!(sentence.contains("/test/user.toml"), "{sentence}");
        assert!(sentence.contains(TALA), "{sentence}");
    }

    /// Clears this thread's account layers when dropped, so a failing
    /// assertion cannot leave the account installed for the next test.
    struct AccountCleared;

    impl Drop for AccountCleared {
        fn drop(&mut self) {
            crate::config::install_account_layers(None);
        }
    }

    /// What a sync's `apply_clone` does: install the account's layers from
    /// `account/keeper.toml`, which sets `agents.default` to `agent` — or,
    /// with `None`, clear the account's layers.
    fn account_sets_default(account: &Path, agent: Option<&str>) {
        use crate::config::{install_account_layers, load_account_layers, AccountLayerSource};
        let Some(agent) = agent else {
            install_account_layers(None);
            return;
        };
        std::fs::write(
            account.join("keeper.toml"),
            format!("[settings]\n\"agents.default\" = {agent:?}\n"),
        )
        .expect("write the account's keeper.toml");
        install_account_layers(Some(load_account_layers(&AccountLayerSource {
            dir: account.to_owned(),
            device: "laptop".to_owned(),
            account_name: "Acme".to_owned(),
            repo_host: "git.acme.dev".to_owned(),
        })));
    }

    fn stored_row(data_dir: &Path) -> Option<String> {
        registry::stored_settings(data_dir, &[registry::AGENTS_DEFAULT_KEY])
            .expect("read the table")
            .remove(registry::AGENTS_DEFAULT_KEY)
    }

    /// R488: the account's `keeper.toml` moving the default from Nixie to
    /// Tala is a change although no settings row moved; the same file
    /// installed again, as every sync does, is not.
    #[test]
    fn the_account_file_moving_the_default_is_a_change_though_no_row_moved() {
        let (dir, account) = (
            tempfile::tempdir().expect("tempdir"),
            tempfile::tempdir().expect("tempdir"),
        );
        let _cleared = AccountCleared;
        account_sets_default(account.path(), Some(NIXIE));
        let seen = DefaultAgentSeen::now(dir.path());

        account_sets_default(account.path(), Some(NIXIE));
        assert!(!seen.moved(dir.path()), "the same file again moves nothing");

        account_sets_default(account.path(), Some(TALA));
        assert!(seen.moved(dir.path()));
        assert_eq!(
            registry::get_agents_default(dir.path()).expect("read"),
            Some(TALA.to_owned())
        );
        assert_eq!(stored_row(dir.path()), None, "no settings row was written");
    }

    /// R488: the account's file going away reveals the default stored here,
    /// which is a change; when the file said what the table says, it is not.
    #[test]
    fn the_account_file_going_away_reveals_the_stored_default() {
        let (dir, account) = (
            tempfile::tempdir().expect("tempdir"),
            tempfile::tempdir().expect("tempdir"),
        );
        let _cleared = AccountCleared;
        registry::set_agents_default(dir.path(), Some(NIXIE)).expect("store Nixie");
        account_sets_default(account.path(), Some(TALA));
        let seen = DefaultAgentSeen::now(dir.path());
        account_sets_default(account.path(), None);
        assert!(seen.moved(dir.path()));
        assert_eq!(
            registry::get_agents_default(dir.path()).expect("read"),
            Some(NIXIE.to_owned())
        );

        account_sets_default(account.path(), Some(NIXIE));
        let same = DefaultAgentSeen::now(dir.path());
        account_sets_default(account.path(), None);
        assert!(!same.moved(dir.path()), "the table says what the file said");
    }

    /// R488: a synced row written beneath a file that keeps deciding the key
    /// moves nothing a person sees; the same row with no file above it does.
    #[test]
    fn a_row_written_beneath_an_unchanged_file_is_no_change() {
        let (dir, account) = (
            tempfile::tempdir().expect("tempdir"),
            tempfile::tempdir().expect("tempdir"),
        );
        let _cleared = AccountCleared;
        account_sets_default(account.path(), Some(TALA));
        let seen = DefaultAgentSeen::now(dir.path());
        registry::set_agents_default(dir.path(), Some(NIXIE)).expect("store Nixie");
        assert!(!seen.moved(dir.path()), "Tala still decides it");
        assert_eq!(stored_row(dir.path()), Some(NIXIE.to_owned()));

        account_sets_default(account.path(), None);
        let seen = DefaultAgentSeen::now(dir.path());
        registry::set_agents_default(dir.path(), Some(TALA)).expect("store Tala");
        assert!(
            seen.moved(dir.path()),
            "with no file, the row is the default"
        );
    }

    /// R381/R488: a default that does not read counts as moved, so the
    /// webview reads it again and says why it cannot.
    #[test]
    fn a_default_that_does_not_read_counts_as_moved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let not_a_dir = dir.path().join("a-file");
        std::fs::write(&not_a_dir, "").expect("write a file where the data dir goes");
        let seen = DefaultAgentSeen::now(&not_a_dir);
        assert!(seen.0.is_err(), "the table does not open in a file");
        assert!(seen.moved(&not_a_dir));
    }

    /// R488 × R495 05: one account-layer transition. Every install or clear
    /// re-reads consent and then tells a moved default, each once and both
    /// after the new layers answer every read; layers installed again
    /// unchanged re-read consent and tell no move; before the app is up no
    /// default is compared.
    #[test]
    fn every_layer_transition_rereads_consent_then_tells_a_moved_default_once() {
        use std::sync::Mutex;
        let (dir, account) = (
            tempfile::tempdir().expect("tempdir"),
            tempfile::tempdir().expect("tempdir"),
        );
        let _cleared = AccountCleared;
        registry::set_agents_default(dir.path(), Some(NIXIE)).expect("store Nixie");
        let told: Arc<Mutex<Vec<String>>> = Arc::default();
        let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (consent_told, moved_told, data_dir, listening) = (
            Arc::clone(&told),
            Arc::clone(&told),
            dir.path().to_owned(),
            Arc::clone(&up),
        );
        let consent_dir = data_dir.clone();
        crate::config::set_account_layers_observer(layers_observer(
            move || {
                let now = registry::get_agents_default(&consent_dir).expect("read");
                consent_told
                    .lock()
                    .expect("told")
                    .push(format!("consent {now:?}"));
            },
            move || {
                listening
                    .load(std::sync::atomic::Ordering::SeqCst)
                    .then(|| data_dir.clone())
            },
            move || moved_told.lock().expect("told").push("moved".to_owned()),
        ));
        let said = || std::mem::take(&mut *told.lock().expect("told"));
        let consent = |agent: &str| format!("consent {:?}", Some(agent));

        account_sets_default(account.path(), Some(TALA));
        assert_eq!(said(), [consent(TALA), "moved".to_owned()]);
        account_sets_default(account.path(), Some(TALA));
        assert_eq!(
            said(),
            [consent(TALA)],
            "the same layers again move nothing"
        );
        account_sets_default(account.path(), None);
        assert_eq!(said(), [consent(NIXIE), "moved".to_owned()]);

        up.store(false, std::sync::atomic::Ordering::SeqCst);
        account_sets_default(account.path(), Some(TALA));
        assert_eq!(
            said(),
            [consent(TALA)],
            "nothing listens before the app is up"
        );
    }
}

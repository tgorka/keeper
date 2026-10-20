//! The Agents window's service on [`Hq`]: the list, opened sessions, their
//! windows, writing, archiving and deciding — every read from the replicas,
//! every write into a session's own database (AD-433). The shell streams
//! what this answers and decides nothing.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::watch;

use crate::hq::approvals::{self, AgentApprovalVm, AgentDecisionOutcomeVm};
use crate::hq::send;
use crate::hq::sessions::{
    self, AgentChoiceVm, AgentDefaultSessionVm, AgentSessionRef, AgentSessionRowVm, Source,
};
use crate::hq::timeline::{self, AgentSessionViewVm};
use crate::hq::{lock, sentence, Hq, HqError};

impl Hq {
    /// Every database this device holds, and who it is in each. Empty until
    /// the device is paired.
    pub fn sources(&self) -> Vec<Source> {
        let inner = lock(&self.inner);
        let Some(pairing) = &inner.pairing else {
            return Vec::new();
        };
        inner
            .dbs
            .iter()
            .map(|db| Source {
                database: db.name.clone(),
                replica: Arc::clone(&db.replica),
                actor: pairing.actor_id.clone(),
                client: pairing.client_id.clone(),
            })
            .collect()
    }

    fn source(&self, database: &str) -> Result<Source, HqError> {
        self.sources()
            .into_iter()
            .find(|source| source.database == database)
            .ok_or_else(|| HqError::Sentence(format!("This device holds no copy of {database}.")))
    }

    /// Ticks whenever a replica's rows, a replica or an opened session
    /// change: what every view of this module reads again on.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    pub(crate) fn touch(&self) {
        self.changes
            .send_modify(|tick| *tick = tick.wrapping_add(1));
    }

    /// The Agents window's rows, every held database merged.
    pub fn sessions(&self) -> Vec<AgentSessionRowVm> {
        sessions::list(&self.sources(), sessions::now_ms())
    }

    /// Keep every replica's windows on what its rows and the opened sessions
    /// ask for. Blocking: a window change waits for the replica's owner.
    pub fn reconcile_windows(&self) {
        let now = sessions::now_ms();
        for source in self.sources() {
            let widen: HashMap<String, u32> = lock(&self.views)
                .iter()
                .filter(|(key, _)| key.database == source.database)
                .map(|(key, opened)| (key.session_id.clone(), opened.widen))
                .collect();
            let plan = sessions::plan(&sessions::held(&source), &widen, now);
            for (table, units) in plan {
                let key = (source.database.clone(), table);
                if lock(&self.windows).get(&key) == Some(&units) {
                    continue;
                }
                match source.replica.set_window(&sessions::window(table), &units) {
                    Ok(()) => {
                        lock(&self.windows).insert(key, units);
                    }
                    Err(error) => {
                        tracing::debug!(database = %source.database, table, %error, "an agents window could not be set");
                    }
                }
            }
        }
    }

    /// Start following the replicas with their windows, once.
    pub(crate) fn follow_windows(&self) {
        let mut task = lock(&self.window_task);
        if task.is_some() {
            return;
        }
        let mut changes = self.changes();
        let me = self.me.clone();
        *task = Some(tokio::spawn(async move {
            loop {
                let Some(hq) = me.upgrade() else { return };
                let done = tokio::task::spawn_blocking(move || hq.reconcile_windows()).await;
                if let Err(error) = done {
                    tracing::warn!(%error, "the agents windows could not be reconciled");
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        }));
    }

    /// An opened session as it stands, marking it read up to its newest
    /// message.
    pub fn session_view(&self, key: &AgentSessionRef) -> Result<AgentSessionViewVm, HqError> {
        let source = self.source(&key.database)?;
        let opened = lock(&self.views).get(key).cloned().unwrap_or_default();
        let (view, newest) = timeline::view(&source, &key.session_id, &opened, sessions::now_ms())
            .map_err(HqError::Sentence)?;
        if let (Some(newest), false) = (newest, view.archived) {
            if let Err(error) = sessions::mark_read(&source, &key.session_id, newest) {
                tracing::debug!(%error, "an agents session's read point could not be written");
            }
        }
        Ok(view)
    }

    /// A stream begins showing `key`: its window holds it (an archived one
    /// starts reading through the server).
    pub fn open_session(&self, key: &AgentSessionRef) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let session = timeline::session_row(&source, &key.session_id).ok_or_else(|| {
            HqError::Sentence("This conversation is not on this device.".to_owned())
        })?;
        let fetch = {
            let mut views = lock(&self.views);
            let opened = views.entry(key.clone()).or_default();
            opened.streams += 1;
            session.archived && !opened.remote.fetched && !opened.remote.loading
        };
        if fetch {
            self.fetch_remote(key.clone(), source, session);
        }
        self.touch();
        Ok(())
    }

    /// A stream stops showing `key`; with none left its widened months go.
    pub fn close_session(&self, key: &AgentSessionRef) {
        {
            let mut views = lock(&self.views);
            let gone = views.get_mut(key).is_some_and(|opened| {
                opened.streams = opened.streams.saturating_sub(1);
                opened.streams == 0
            });
            if gone {
                views.remove(key);
            }
        }
        self.touch();
    }

    /// Read the next page of an archived session through the server, off
    /// the async workers.
    fn fetch_remote(&self, key: AgentSessionRef, source: Source, session: timeline::SessionRow) {
        let pages = {
            let mut views = lock(&self.views);
            let Some(opened) = views.get_mut(&key) else {
                return;
            };
            if opened.remote.loading || opened.remote.exhausted {
                return;
            }
            opened.remote.loading = true;
            opened.remote.clone()
        };
        let me = self.me.clone();
        tokio::task::spawn_blocking(move || {
            let fetched = timeline::fetch_page(&source, &key.session_id, &session, &pages);
            let Some(hq) = me.upgrade() else { return };
            {
                let mut views = lock(&hq.views);
                if let Some(opened) = views.get_mut(&key) {
                    let remote = &mut opened.remote;
                    remote.loading = false;
                    remote.fetched = true;
                    match fetched {
                        Ok(page) => {
                            remote.rows.extend(page.rows);
                            remote.month = page.month;
                            remote.cursor = page.cursor;
                            remote.exhausted = page.exhausted;
                            remote.error = None;
                        }
                        Err(error) => {
                            tracing::info!(database = %source.database, %error, "an archived agents session could not be fetched");
                            remote.error = Some(timeline::fetch_sentence(&source.database, &error));
                        }
                    }
                }
            }
            hq.touch();
        });
    }

    /// Scrolled back to the top: widen an active session by a month, or
    /// read an archived one's next page.
    pub fn page_back(&self, key: &AgentSessionRef) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let session = timeline::session_row(&source, &key.session_id).ok_or_else(|| {
            HqError::Sentence("This conversation is not on this device.".to_owned())
        })?;
        if session.archived {
            self.fetch_remote(key.clone(), source, session);
            return Ok(());
        }
        if let Some(opened) = lock(&self.views).get_mut(key) {
            opened.widen = opened.widen.saturating_add(1);
        }
        self.touch();
        Ok(())
    }

    /// Write a person's message into a session.
    pub fn send(&self, key: &AgentSessionRef, body: &str) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        send::send(&source, &key.session_id, body).map_err(sentence)
    }

    /// Open a session with `agent` in `database` and write its first
    /// message; the database is remembered as the person's choice.
    pub fn new_session(
        &self,
        database: &str,
        agent: &str,
        body: &str,
    ) -> Result<AgentSessionRef, HqError> {
        if body.trim().is_empty() {
            return Err(HqError::Sentence("Write something first.".to_owned()));
        }
        let sources = self.sources();
        let offered = sessions::choices(&sources, &HashMap::new())
            .into_iter()
            .any(|choice| {
                choice.agent_actor == agent && choice.databases.iter().any(|d| d == database)
            });
        if !offered {
            return Err(HqError::Sentence(format!(
                "{database}'s directory does not name {agent}; a session goes to a database the agent shares."
            )));
        }
        let source = self.source(database)?;
        let session_id = sessions::create(&source, agent, body).map_err(sentence)?;
        sessions::remember_choice(&self.data_dir()?, agent, database);
        self.touch();
        Ok(AgentSessionRef {
            database: database.to_owned(),
            session_id,
        })
    }

    /// Archive or unarchive a session; its window follows.
    pub fn set_archived(&self, key: &AgentSessionRef, archived: bool) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        sessions::set_archived(&source, &key.session_id, archived).map_err(sentence)?;
        if let Some(opened) = lock(&self.views).get_mut(key) {
            opened.remote = timeline::RemotePages::default();
        }
        if archived {
            // An archived session opened now reads through the server.
            if let Some(session) = timeline::session_row(&source, &key.session_id) {
                if lock(&self.views).contains_key(key) {
                    self.fetch_remote(key.clone(), source, session);
                }
            }
        }
        self.touch();
        Ok(())
    }

    /// The agents a new session can be opened with, and where it would go.
    pub fn choices(&self) -> Result<Vec<AgentChoiceVm>, HqError> {
        Ok(sessions::choices(
            &self.sources(),
            &sessions::last_choices(&self.data_dir()?),
        ))
    }

    /// The default agent's main conversation, for launch and the docks.
    pub fn default_session(&self) -> Result<Option<AgentDefaultSessionVm>, HqError> {
        let data_dir = self.data_dir()?;
        let chosen = crate::registry::get_agents_default(&data_dir)?;
        Ok(sessions::default_session(
            &self.sources(),
            chosen.as_deref(),
            &sessions::last_choices(&data_dir),
            sessions::now_ms(),
        ))
    }

    /// Every approval this person may decide now.
    pub fn approvals_waiting(&self) -> Vec<AgentApprovalVm> {
        approvals::waiting(&self.sources(), sessions::now_ms())
    }

    /// Decide an approval. Blocking: it waits briefly for the server.
    pub fn decide(
        &self,
        key: &AgentSessionRef,
        approval_id: &str,
        binding_digest: &str,
        approve: bool,
    ) -> Result<AgentDecisionOutcomeVm, HqError> {
        let source = self.source(&key.database)?;
        approvals::decide(
            &source,
            &key.session_id,
            approval_id,
            binding_digest,
            approve,
            sessions::now_ms(),
        )
        .map_err(sentence)
    }
}

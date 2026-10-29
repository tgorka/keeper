//! The Agents window's service on [`Hq`]: the list, opened sessions, their
//! windows, writing, archiving and deciding — every read from the replicas,
//! every write into a session's own database (AD-433). The shell streams
//! what this answers and decides nothing.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use tokio::sync::watch;

use crate::hq::approvals::{
    self, AgentApprovalVm, AgentDecisionOutcomeVm, Decisions, ShownApprovalVm,
};
use crate::hq::attachments::{self, OpenedBlob};
use crate::hq::drives;
use crate::hq::live::{self, AgentViewPrefsPatch, AgentViewPrefsVm};
use crate::hq::send;
use crate::hq::sessions::{
    self, AgentChoiceVm, AgentDefaultSessionVm, AgentSessionRef, AgentSessionRowVm, Source,
};
use crate::hq::timeline::{self, AgentSessionViewVm, Opened};
use crate::hq::vm::{phase_sentence, HqPhase};
use crate::hq::{lock, sentence, Hq, HqError, LocalOpen};

/// How often the grants units the server revoked are asked for again.
pub const GRANTS_REASK_EVERY: Duration = Duration::from_secs(3600);

/// The shortest wait between two asks [`Hq::recheck_grants`] causes.
pub const GRANTS_RECHECK_AFTER: Duration = Duration::from_secs(60);

/// Every archive fetch's token, so an answer for a view that has moved on
/// is told apart from the one it waits for.
static FETCHES: AtomicU64 = AtomicU64::new(0);

/// The read points being written, so one slow write is not queued twice.
static MARKING: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Mutex::default);

/// The view of an opened session from the replica, then — on its own
/// thread, so the view never waits behind the replica's owner and a round
/// it may be in — the person's read point moved up to its newest message.
pub(crate) fn view_now(
    source: &Source,
    session_id: &str,
    opened: &Opened,
    refusals: &HashMap<(String, String), String>,
    now_ms: i64,
) -> Result<AgentSessionViewVm, String> {
    let (view, newest) = timeline::view(source, session_id, opened, refusals, now_ms)?;
    if let (Some(newest), false) = (newest, view.archived) {
        let key = (source.database.clone(), session_id.to_owned());
        if lock(&MARKING).insert(key.clone()) {
            let (writer, marked) = (source.clone(), key.clone());
            let spawned = std::thread::Builder::new()
                .name("hq-read-mark".to_owned())
                .spawn(move || {
                    if let Err(error) = sessions::mark_read(&writer, &marked.1, newest) {
                        tracing::debug!(%error, "an agents session's read point could not be written");
                    }
                    lock(&MARKING).remove(&marked);
                });
            if let Err(error) = spawned {
                tracing::debug!(%error, "an agents session's read point was not written");
                lock(&MARKING).remove(&key);
            }
        }
    }
    Ok(view)
}

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

    pub(crate) fn source(&self, database: &str) -> Result<Source, HqError> {
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

    /// Ticks whenever only a presence document moved — a tail of an answer
    /// being written: what an open session reads again on beside
    /// [`Hq::changes`]. A presence move does not reach [`Hq::changes`] or
    /// the status, so anything else that reads presence (who is online, by
    /// `dir:`) must follow this too; the sessions list does not re-read on it.
    pub fn live_changes(&self) -> watch::Receiver<u64> {
        self.live.subscribe()
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
            let plan = sessions::plan(
                &sessions::held(&source),
                &widen,
                self.view_prefs().view_mode.shows_thinking(),
                now,
            )
            .into_iter()
            .chain(std::iter::once((
                crate::hq::grants::TABLE,
                crate::hq::grants::units(&source),
            )));
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
            self.reask_grants_if_due(&source);
        }
    }

    /// Ask the server again for `source`'s revoked grants units
    /// ([`crate::hq::grants::reask_revoked`]) once after the replicas
    /// (re)start, then at most every [`GRANTS_REASK_EVERY`], or sooner when
    /// [`Hq::recheck_grants`] asks: whether an agent serves the person is the
    /// server's registry, which no synced row reflects, so only the server's
    /// own answer can turn it true again.
    fn reask_grants_if_due(&self, source: &Source) {
        let due = {
            let mut asked = lock(&self.grants_asked);
            let due = asked
                .get(&source.database)
                .is_none_or(|at| at.elapsed() >= GRANTS_REASK_EVERY);
            if due {
                asked.insert(source.database.clone(), std::time::Instant::now());
            }
            due
        };
        if !due {
            return;
        }
        if let Err(error) = crate::hq::grants::reask_revoked(source) {
            tracing::debug!(database = %source.database, %error, "the revoked grants units could not be asked for again");
        }
    }

    /// Ask the server again soon for the grants units it revoked, as the
    /// person looks at what grants decide: Settings › Always allowed, an
    /// opened session. Not more than once a [`GRANTS_RECHECK_AFTER`] per
    /// database.
    pub fn recheck_grants(&self) {
        lock(&self.grants_asked).retain(|_, at| at.elapsed() < GRANTS_RECHECK_AFTER);
        self.touch();
    }

    /// Start following the replicas with their windows, once; and wake
    /// every view when the next pending approval or grant expires, since
    /// nothing in the replica changes then.
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
                let done = tokio::task::spawn_blocking(move || {
                    hq.reconcile_windows();
                    // A drive write decided here whose answer the server
                    // has now given — or that a stop left — goes on (109.27).
                    hq.carry_on_writes();
                    let (sources, now) = (hq.sources(), sessions::now_ms());
                    // A request or a grant expiring changes what is shown
                    // with no row changing.
                    [
                        approvals::next_expiry(&sources, now),
                        crate::hq::grants::next_expiry(&sources, now),
                    ]
                    .into_iter()
                    .flatten()
                    .min()
                })
                .await;
                let next = match done {
                    Ok(next) => next,
                    Err(error) => {
                        tracing::warn!(%error, "the agents windows could not be reconciled");
                        None
                    }
                };
                let changed = match next {
                    Some(at) => {
                        let wait = u64::try_from(at - sessions::now_ms()).unwrap_or(0) + 1;
                        tokio::time::timeout(Duration::from_millis(wait), changes.changed()).await
                    }
                    None => Ok(changes.changed().await),
                };
                match changed {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => return,
                    Err(_expired) => {
                        if let Some(hq) = me.upgrade() {
                            hq.touch();
                        }
                    }
                }
            }
        }));
    }

    /// An opened session as it stands. Its read point is written after,
    /// never before: the view is the replica's and waits on nothing. The
    /// tails present now are kept first, so one that leaves later is still
    /// shown until its chunk arrives. They are read and kept under the
    /// views' lock: two streams of one session (a remount opens the next
    /// before the last has closed) each add to the one kept set, so neither
    /// writes back a set missing the other's tail. What a view settles
    /// after is only what its own snapshot held: an older view never
    /// forgets a turn a newer stream observed meanwhile.
    pub fn session_view(&self, key: &AgentSessionRef) -> Result<AgentSessionViewVm, HqError> {
        let source = self.source(&key.database)?;
        let prefs = self.view_prefs();
        let now_ms = sessions::now_ms();
        let session = timeline::session_row(&source, &key.session_id);
        // Read once for this pass: a member who does not read the trace now
        // reads no trace presence, and the reasoning tails kept from before
        // the flag was withdrawn (or the member removed) are dropped.
        let reads_trace = sessions::reads_trace(&source, &key.session_id);
        let observe = |live: &mut live::Live| {
            let Some(session) = &session else { return };
            let presence = |scope: &str| {
                source.replica.presence(scope).unwrap_or_else(|error| {
                    tracing::debug!(database = %source.database, %error, "an agents presence could not be read");
                    Vec::new()
                })
            };
            let on_session = presence(&format!("session:{}", key.session_id));
            let on_trace: Vec<_> = if reads_trace {
                sessions::newest_months(session.created_ms, session.last_activity_ms, now_ms)
                    .iter()
                    .flat_map(|m| {
                        presence(&format!("trace:{}", sessions::page_key(&key.session_id, m)))
                    })
                    .collect()
            } else {
                live.forget_trace();
                Vec::new()
            };
            live.observe(
                &on_session,
                &on_trace,
                live::claim(&source, &key.session_id),
                std::time::Instant::now(),
                now_ms,
            );
        };
        let opened = {
            let mut views = lock(&self.views);
            match views.get_mut(key) {
                Some(held) => {
                    held.prefs = prefs;
                    observe(&mut held.live);
                    held.clone()
                }
                None => {
                    let mut opened = Opened {
                        prefs,
                        ..Opened::default()
                    };
                    observe(&mut opened.live);
                    opened
                }
            }
        };
        let refusals = self.refusals();
        let mut view = view_now(&source, &key.session_id, &opened, &refusals, now_ms)
            .map_err(HqError::Sentence)?;
        // Said under a message sent again without the note (§ 9.1a).
        if let Ok(data_dir) = self.data_dir() {
            let withheld = send::withheld(&crate::hq::databases::root(&data_dir), &key.database);
            for message in &mut view.messages {
                if let Some(sentence) = withheld.get(&message.id) {
                    message.note = Some(sentence.clone());
                }
            }
        }
        if let Some(session) = timeline::session_row(&source, &key.session_id) {
            view.drives = drives::view(&source, &key.session_id, &session, &self.sources());
        }
        self.writes
            .draw(&self.sources(), &source, &mut view.approvals_mut());
        // Ticked drives this person has not been shown the warning for on
        // this device, read beside the chips from the same selection.
        if let Some(drives) = view.drives.as_mut() {
            let ticked: Vec<String> = drives
                .drives
                .iter()
                .filter(|drive| drive.ticked)
                .map(|drive| drive.name.clone())
                .collect();
            drives.unseen = self.unseen_ticks(key, &ticked);
        }
        if let Some(held) = lock(&self.views).get_mut(key) {
            held.live.settle(&opened.live, &view.live);
        }
        Ok(view)
    }

    /// When an open session's stream reads it again with nothing changing:
    /// soon while an answer is still arriving (one whose instance stopped
    /// says so), and just after its claim's lease would lapse, so a header
    /// naming a holder that stopped renewing says so with no new row
    /// (R1065-05). `None`: only a change wakes it.
    pub fn session_recheck(
        &self,
        key: &AgentSessionRef,
        view: Option<&AgentSessionViewVm>,
    ) -> Option<Duration> {
        if view.is_some_and(|view| view.live.iter().any(|turn| !turn.stopped)) {
            return Some(live::RECHECK);
        }
        let views = lock(&self.views);
        let lapse = views
            .get(key)?
            .live
            .lapses_in(std::time::Instant::now(), sessions::now_ms())?;
        Some(lapse + Duration::from_millis(50))
    }

    /// What this viewer sees, as the settings resolve it (a settings file
    /// wins), read once and kept until it is written here or
    /// [`Hq::reconcile`] finds it moved.
    pub fn view_prefs(&self) -> AgentViewPrefsVm {
        let mut held = lock(&self.prefs);
        if let Some(prefs) = *held {
            return prefs;
        }
        match self.read_view_prefs() {
            Ok(prefs) => *held.insert(prefs),
            Err(error) => {
                tracing::debug!(%error, "the agents view preferences could not be read");
                AgentViewPrefsVm::default()
            }
        }
    }

    fn read_view_prefs(&self) -> Result<AgentViewPrefsVm, HqError> {
        let data_dir = self.data_dir()?;
        Ok(AgentViewPrefsVm {
            view_mode: crate::registry::get_agents_view_mode(&data_dir)?,
        })
    }

    /// The settings may have moved under this process (a settings file, or
    /// a change synced from another device): read them again, and when
    /// they differ from what is kept, the windows and open views follow.
    /// Read and kept under the one lock a write holds, so a read from
    /// before a write is never kept after it.
    pub(crate) fn reread_view_prefs(&self) {
        let mut held = lock(&self.prefs);
        let Ok(read) = self.read_view_prefs() else {
            return;
        };
        let moved = held.replace(read) != Some(read);
        drop(held);
        if moved {
            self.touch();
        }
    }

    /// Write what is present in `patch` and answer what holds after — the
    /// settings read again, so a settings file that sets a preference keeps
    /// it, and the answer says so. Written, read and kept under one lock,
    /// so a re-read racing it never keeps an older value. The windows and
    /// open views follow when it moved: the thinking is subscribed only
    /// while it is shown.
    pub fn set_view_prefs(&self, patch: AgentViewPrefsPatch) -> Result<AgentViewPrefsVm, HqError> {
        let data_dir = self.data_dir()?;
        let mut held = lock(&self.prefs);
        if let Some(mode) = patch.view_mode {
            crate::registry::set_agents_view_mode(&data_dir, mode)?;
        }
        let prefs = self.read_view_prefs()?;
        let moved = held.replace(prefs) != Some(prefs);
        drop(held);
        if moved {
            self.touch();
        }
        Ok(prefs)
    }

    /// Switch an offered tool off or back on for one session.
    pub fn set_session_tool(
        &self,
        key: &AgentSessionRef,
        name: &str,
        on: bool,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let session = timeline::session_row(&source, &key.session_id).ok_or_else(|| {
            HqError::Sentence("This conversation is not on this device.".to_owned())
        })?;
        live::set_tool(
            &source,
            &key.session_id,
            &session.agent_actor,
            name,
            on,
            sessions::now_ms(),
        )
        .map_err(sentence)?;
        self.touch();
        Ok(())
    }

    /// A stream begins showing `key`: its window holds it (an archived one
    /// starts reading through the server). A session this device does not
    /// hold is [`HqError::Arriving`] while a round may still bring it — the
    /// replicas are opening, or its database syncs — and a sentence once
    /// nothing will: no server, a removed device, a database not open to it.
    pub fn open_session(&self, key: &AgentSessionRef) -> Result<(), HqError> {
        // Its cards may offer a grant: whether the agent serves the person
        // now is asked of the server again (R10911-10).
        self.recheck_grants();
        let held = self.source(&key.database).ok().and_then(|source| {
            timeline::session_row(&source, &key.session_id).map(|session| (source, session))
        });
        let Some((source, session)) = held else {
            return Err(self.not_here(&key.database));
        };
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

    /// Why a session of `database` this device does not hold cannot open:
    /// still arriving — the replicas on disk are still opening, or a round
    /// may bring it — or not coming. Reads the configuration only.
    fn not_here(&self, database: &str) -> HqError {
        if let Some(refusal) = self.not_open() {
            return refusal;
        }
        let inner = lock(&self.inner);
        let (Some(server), Some(_)) = (&inner.server, &inner.pairing) else {
            // Nothing syncs, so what is not here does not come.
            return HqError::Sentence("This conversation is not on this device.".to_owned());
        };
        if inner.authorization.removed {
            return HqError::Sentence(phase_sentence(HqPhase::Removed, Some(server.host()), 0));
        }
        if inner.discovering || inner.authorization.open.iter().any(|open| open == database) {
            return HqError::Arriving(format!(
                "This conversation has not reached this device yet; it opens here when {database} \
                 brings it."
            ));
        }
        HqError::Sentence(format!(
            "{database} is not open to this device, so this conversation cannot come here."
        ))
    }

    /// Why nothing held can be read yet, while the replicas on disk are not
    /// open: still opening is [`HqError::Arriving`] — not open yet is not
    /// absent, whatever the configuration says — and a failed reopen its
    /// sentence. `None` once they are open.
    pub(crate) fn not_open(&self) -> Option<HqError> {
        match &lock(&self.inner).local_open {
            LocalOpen::Opening => Some(HqError::Arriving(
                "keeper is still opening the conversations on this device.".to_owned(),
            )),
            LocalOpen::Failed(why) => Some(HqError::Sentence(format!(
                "keeper could not open the conversations on this device: {why}"
            ))),
            LocalOpen::Open => None,
        }
    }

    /// A stream stops showing `key`; with none left its widened months go,
    /// and so does every file its composer staged and did not send.
    pub fn close_session(&self, key: &AgentSessionRef) {
        let gone = {
            let mut views = lock(&self.views);
            let gone = views.get_mut(key).is_some_and(|opened| {
                opened.streams = opened.streams.saturating_sub(1);
                opened.streams == 0
            });
            if gone {
                views.remove(key);
            }
            gone
        };
        if gone {
            self.staging.release_session(key);
        }
        self.touch();
    }

    /// Read the next page of an archived session through the server, off
    /// the async workers.
    fn fetch_remote(&self, key: AgentSessionRef, source: Source, session: timeline::SessionRow) {
        let token = FETCHES.fetch_add(1, Ordering::Relaxed) + 1;
        let pages = {
            let mut views = lock(&self.views);
            let Some(opened) = views.get_mut(&key) else {
                return;
            };
            if opened.remote.loading || opened.remote.exhausted {
                return;
            }
            opened.remote.begin(token)
        };
        let me = self.me.clone();
        tokio::task::spawn_blocking(move || {
            let fetched = timeline::fetch_page(&source, &key.session_id, &session, &pages)
                .map_err(|error| {
                    tracing::info!(database = %source.database, %error, "an archived agents session could not be fetched");
                    timeline::fetch_sentence(&source.database, &error)
                });
            let Some(hq) = me.upgrade() else { return };
            // A view closed and opened again, or archived again, waits for
            // another fetch: this answer is dropped.
            let landed = lock(&hq.views)
                .get_mut(&key)
                .is_some_and(|opened| opened.remote.land(token, fetched));
            if landed {
                hq.touch();
            }
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

    /// The messages of `database` the server refused for their focus's
    /// audience, written again without it (contract § 9.1a,
    /// [`send::resend_without_focus`]).
    pub(crate) fn resend_withheld(&self, database: &str) {
        let (Ok(source), Ok(data_dir)) = (self.source(database), self.data_dir()) else {
            return;
        };
        match send::resend_without_focus(&source, &crate::hq::databases::root(&data_dir)) {
            Ok(0) => {}
            Ok(_) => self.touch(),
            Err(error) => {
                tracing::warn!(%database, %error, "a message refused for its note could not be sent again")
            }
        }
    }

    /// Write a person's message into a session; it declines the requests its
    /// banner showed (`shown`) that still wait for them (Q109-12 a). `focus`
    /// is the note open in front of them in the Notes Assistant (109.28).
    pub fn send(
        &self,
        key: &AgentSessionRef,
        body: &str,
        shown: &[ShownApprovalVm],
        focus: Option<&send::AgentFocus>,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let focus = send::AgentFocus::for_destination(focus, &source, &self.sources());
        let sent = send::send(
            &source,
            self.decisions()?,
            &key.session_id,
            body,
            shown,
            focus,
        );
        self.touch();
        sent.map_err(sentence)
    }

    /// Stage the files at `paths` for a message of a session: every file
    /// read and checked, so one over the bound or unreadable refuses them
    /// all, with its sentence, and none is staged. A handle for each, in
    /// order. Nothing reaches the replica until [`Hq::commit_attachments`].
    pub fn stage_paths(
        &self,
        key: &AgentSessionRef,
        paths: &[std::path::PathBuf],
    ) -> Result<Vec<String>, HqError> {
        self.source(&key.database)?;
        let files = paths
            .iter()
            .map(|path| attachments::stage_path(path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(sentence)?;
        Ok(files
            .into_iter()
            .map(|file| self.staging.stage(key, file))
            .collect())
    }

    /// Stage pasted `bytes` named `name` for a message of a session; its
    /// handle.
    pub fn stage_bytes(
        &self,
        key: &AgentSessionRef,
        name: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> Result<String, HqError> {
        self.source(&key.database)?;
        let file = attachments::stage_bytes(name, mime, bytes).map_err(sentence)?;
        Ok(self.staging.stage(key, file))
    }

    /// The composer's one Send: every file `handles` name, with `caption`
    /// as its text, committed as one new message of the session — or
    /// nothing, with the sentence. The handles are spent either way.
    /// `focus` is the note open in front of the person (109.28).
    pub fn commit_attachments(
        &self,
        key: &AgentSessionRef,
        handles: &[String],
        caption: &str,
        shown: &[ShownApprovalVm],
        focus: Option<&send::AgentFocus>,
    ) -> Result<(), HqError> {
        let files = self.staging.take(key, handles).map_err(sentence)?;
        // 109.24: a mention starts its own thread and carries no files; the
        // caption is never sent to this session's agent in its place.
        if crate::hq::mention::parse(caption).is_some() {
            return Err(HqError::Sentence(crate::hq::mention::WITH_FILES.to_owned()));
        }
        let source = self.source(&key.database)?;
        let focus = send::AgentFocus::for_destination(focus, &source, &self.sources());
        let attached = attachments::attach(
            &source,
            self.decisions()?,
            &key.session_id,
            caption,
            files,
            shown,
            focus,
        );
        self.touch();
        attached.map_err(sentence)?;
        self.touch();
        Ok(())
    }

    /// The composer let go of `handles` without sending them.
    pub fn release_attachments(&self, key: &AgentSessionRef, handles: &[String]) {
        self.staging.release(key, handles);
    }

    /// A chip opened: `blob_id` of `database`, from this device's cache or
    /// downloaded now. Blocking: a download waits for the replica's owner.
    pub fn open_blob(&self, database: &str, blob_id: &str) -> Result<OpenedBlob, HqError> {
        let source = self.source(database)?;
        attachments::open_blob(&source, blob_id).map_err(sentence)
    }

    /// Resolve a device-only thumbnail request; no network fetch is reachable.
    pub fn attachment_thumbnail(&self, uri: &str) -> Result<Vec<u8>, HqError> {
        if let Some((database, blob)) = attachments::parse_blob_url(uri) {
            super::thumbnails::blob(&self.source(&database)?, &blob).map_err(sentence)
        } else {
            super::thumbnails::staged(&self.staging, uri).map_err(sentence)
        }
    }

    /// The bytes of blobs `database`'s replica caches on this device.
    pub fn blob_cache_bytes(&self, database: &str) -> Result<u64, HqError> {
        self.source(database)?
            .replica
            .blob_cache_bytes()
            .map_err(sentence)
    }

    /// Open a session with `agent` in `database` and write its first
    /// message; the database is remembered as the person's choice. `pick`
    /// is this device's last model and effort for `agent`: what it still
    /// offers starts the session's settings (Q109-16 b). `focus` is the note
    /// in front of the person when the Notes Assistant starts the session
    /// (109.28), carried by the first message where its audience allows.
    pub fn new_session(
        &self,
        database: &str,
        agent: &str,
        body: &str,
        pick: Option<&crate::hq::settings::AgentSettingsPick>,
        focus: Option<&send::AgentFocus>,
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
        let focus = send::AgentFocus::for_destination(focus, &source, &sources);
        let session_id = sessions::create(&source, agent, body, pick, focus).map_err(sentence)?;
        sessions::remember_choice(&self.data_dir()?, agent, database);
        self.touch();
        Ok(AgentSessionRef {
            database: database.to_owned(),
            session_id,
        })
    }

    pub fn rename_session(&self, key: &AgentSessionRef, title: &str) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        sessions::rename(&source, &key.session_id, title).map_err(sentence)?;
        self.touch();
        Ok(())
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

    /// The decisions written here whose outcome is still owed, kept with
    /// the replicas so a refusal that arrives after a relaunch is still
    /// shown.
    pub(crate) fn decisions(&self) -> Result<&Decisions, HqError> {
        if let Some(decisions) = self.decisions.get() {
            return Ok(decisions);
        }
        let path = crate::hq::databases::root(&self.data_dir()?).join("decisions.json");
        Ok(self.decisions.get_or_init(|| Decisions::load(path)))
    }

    /// The server's refusals of decisions written here, by database and
    /// approval; settled decisions are forgotten as they are read.
    fn refusals(&self) -> HashMap<(String, String), String> {
        match self.decisions() {
            Ok(decisions) => decisions.refusals(&self.sources()),
            Err(error) => {
                tracing::debug!(%error, "the agents decisions could not be read");
                HashMap::new()
            }
        }
    }

    /// Every approval this person may decide now.
    pub fn approvals_waiting(&self) -> Vec<AgentApprovalVm> {
        let sources = self.sources();
        let mut waiting = approvals::waiting(&sources, &self.refusals(), sessions::now_ms());
        for source in &sources {
            let mut here: Vec<&mut AgentApprovalVm> = waiting
                .iter_mut()
                .filter(|approval| approval.database == source.database)
                .collect();
            self.writes.draw(&sources, source, &mut here);
        }
        waiting
    }

    /// Set this device's drive writer (109.27): the shell's, over its synced
    /// folders. Until it is, a drive write is never offered *Allow* here.
    pub fn set_drive_writes(&self, port: Arc<dyn crate::hq::port::DriveWrites>) {
        self.writes.set_port(port);
        self.touch();
    }

    /// Carry on, on a thread of its own, the drive writes this device
    /// decided and has not said the outcome of
    /// ([`approvals::write::Writes::carry_on`]); one run at a time. A wake
    /// that arrives while a pass runs is never lost (R606-02): it leaves
    /// `carry_again` set, and the worker passes once more before it goes
    /// idle. The flag is cleared only at the top of a pass, and the worker
    /// looks at it only after it has let go of `carrying`, so a wake landing
    /// between the two either sees `carrying` free and starts its own run,
    /// or is seen by this one. A pass is driven by wakes only, so a drive
    /// that stays unavailable is not retried in a loop.
    pub(crate) fn carry_on_writes(&self) {
        self.carry_again.store(true, Ordering::SeqCst);
        if self.carrying.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.me.clone();
        let (carrying, again) = (Arc::clone(&self.carrying), Arc::clone(&self.carry_again));
        let spawned = std::thread::Builder::new()
            .name("hq-drive-writes".to_owned())
            .spawn(move || loop {
                again.store(false, Ordering::SeqCst);
                if let Some(hq) = me.upgrade() {
                    hq.writes.carry_on(&hq.sources());
                }
                carrying.store(false, Ordering::SeqCst);
                // A wake during the pass: pass again, unless that wake has
                // started a run of its own since `carrying` was let go.
                if !again.load(Ordering::SeqCst) || carrying.swap(true, Ordering::SeqCst) {
                    return;
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "the drive writes could not be carried on");
            self.carrying.store(false, Ordering::SeqCst);
        }
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
        let decisions = self.decisions()?;
        let now = sessions::now_ms();
        let writes = approvals::write::Writes::is_write(&source, approval_id);
        let outcome = match (writes, approve) {
            (true, true) => self.writes.allow(
                &source,
                decisions,
                &key.session_id,
                approval_id,
                binding_digest,
                now,
            ),
            (true, false) => self.writes.deny(
                &source,
                decisions,
                &key.session_id,
                approval_id,
                binding_digest,
                now,
            ),
            (false, _) => approvals::decide(
                &source,
                decisions,
                &key.session_id,
                approval_id,
                binding_digest,
                approve,
                now,
            ),
        }
        .map_err(sentence);
        self.touch();
        outcome
    }
}

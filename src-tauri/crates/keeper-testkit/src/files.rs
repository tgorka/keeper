//! The files half of a keeper-hq for tests, judged by
//! [`keeper_files_api::rules`], together with a [`Replica`] whose
//! [`Replica::query`] runs real SQL over an in-memory SQLite holding the
//! replicated `files_*` tables.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use keeper_accounts_api::Role;
use keeper_files_api::caller::Caller;
use keeper_files_api::refusal::{Code, Refusal};
use keeper_files_api::rows::{Entry, Event, Head, Op, Proposal, ProposalReviewer, Review};
use keeper_files_api::rules::{self, evaluate, Accepted, Manifest, Server};
use keeper_ports::{
    BlobBody, CommandResult, Covered, Delivery, LinkState, Peer, RefusedCommit, Replica,
    ReplicaError, ReplicaState, RowOp, RowWrites, Source, Unit, UploadRefusal, WindowBase,
    WindowState,
};
use rusqlite::types::ValueRef;
use serde_json::{json, Map, Value};
use tokio::sync::watch;

const INITIAL_RECORDED_AT_MS: i64 = 1_767_225_600_000;
const RECORDED_AT_STEP_MS: i64 = 1_000;

const CREATE_FILES_ENTRIES: &str = "
CREATE TABLE files_entries (
    id TEXT PRIMARY KEY,
    domain TEXT,
    drive_key TEXT,
    parent_id TEXT,
    name TEXT,
    name_fold TEXT,
    kind TEXT,
    content_id TEXT,
    size INTEGER,
    media_type TEXT,
    mtime_ms INTEGER,
    mode TEXT,
    revision TEXT,
    lifecycle TEXT,
    created_event_id TEXT,
    updated_event_id TEXT,
    updated_seq TEXT,
    author_actor TEXT,
    author_client TEXT
)";

const CREATE_FILES_HEAD: &str = "
CREATE TABLE files_head (
    id TEXT PRIMARY KEY,
    domain TEXT,
    seq TEXT,
    event_id TEXT,
    digest TEXT,
    drives TEXT,
    author_actor TEXT,
    author_client TEXT
)";

const CREATE_FILES_RECEIPTS: &str = "
CREATE TABLE files_receipts (
    id TEXT PRIMARY KEY,
    client_scope TEXT,
    descriptor TEXT,
    request_id TEXT,
    request_digest TEXT,
    outcome TEXT,
    reason TEXT,
    event_id TEXT,
    seq TEXT,
    detail TEXT,
    recorded_at_ms INTEGER,
    author_actor TEXT,
    author_client TEXT
)";

const CREATE_FILES_PROPOSALS: &str = "
CREATE TABLE files_proposals (
    id TEXT PRIMARY KEY,
    domain TEXT,
    proposal_id TEXT,
    revision TEXT,
    status TEXT,
    reason TEXT,
    digest TEXT,
    change TEXT,
    reviewers TEXT,
    expires_at_ms INTEGER,
    proposer_actor TEXT,
    proposer_client TEXT,
    proposer_context TEXT,
    origin_actor TEXT,
    origin_chain TEXT,
    source_labels TEXT,
    created_at_ms INTEGER,
    decided_at_ms INTEGER,
    decided_by TEXT,
    decided_event_id TEXT,
    change_set_id TEXT,
    job_id TEXT,
    job_ordinal INTEGER,
    job_attempt_id TEXT,
    author_actor TEXT,
    author_client TEXT
)";

const CREATE_FILES_PROPOSAL_REVIEWERS: &str = "
CREATE TABLE files_proposal_reviewers (
    id TEXT PRIMARY KEY,
    domain TEXT,
    reviewer_actor TEXT,
    proposal_id TEXT,
    revision TEXT,
    status TEXT,
    proposer_actor TEXT,
    change_set_id TEXT,
    expires_at_ms INTEGER,
    op_count INTEGER,
    author_actor TEXT,
    author_client TEXT
)";

type ObjectChecker = Box<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
struct ReceiptRow {
    id: String,
    client_scope: String,
    descriptor: String,
    request_id: String,
    request_digest: String,
    outcome: String,
    reason: Option<String>,
    event_id: Option<String>,
    seq: Option<String>,
    detail: String,
    recorded_at_ms: i64,
    author_actor: String,
    author_client: String,
}

struct ServerState {
    manifest: Manifest,
    receipts: BTreeMap<String, ReceiptRow>,
    object_check: Option<ObjectChecker>,
    next_time_ms: i64,
    commands: usize,
}

struct Snapshot {
    entries: Vec<(Entry, String, String)>,
    head: Option<(Head, String, String)>,
    receipts: Vec<ReceiptRow>,
    proposals: Vec<Proposal>,
    proposal_reviewers: Vec<(ProposalReviewer, String)>,
}

/// `<a>:<actor><c>:<client>`, where `a` and `c` are the UTF-8 byte lengths.
#[must_use]
pub fn client_scope(actor: &str, client: &str) -> String {
    format!("{}:{actor}{}:{client}", actor.len(), client.len())
}

fn value_to_text(value: Value) -> String {
    match value {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

fn enum_word<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value).map_or_else(|_| String::new(), value_to_text)
}

fn collect_content_ids(params: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let Some(payload) = params.get("payload") else {
        return ids;
    };
    if let Some(cid) = payload
        .get("operation")
        .and_then(|op| op.get("content_id"))
        .and_then(Value::as_str)
    {
        ids.push(cid.to_owned());
    }
    if let Some(ops) = payload.get("operations").and_then(Value::as_array) {
        for op in ops {
            if let Some(cid) = op.get("content_id").and_then(Value::as_str) {
                ids.push(cid.to_owned());
            }
        }
    }
    if let Some(ops) = payload
        .get("change")
        .and_then(|c| c.get("operations"))
        .and_then(Value::as_array)
    {
        for op in ops {
            if let Some(cid) = op.get("content_id").and_then(Value::as_str) {
                ids.push(cid.to_owned());
            }
        }
    }
    ids
}

/// The files half of a keeper-hq: one domain's manifest judged by the
/// contract's reference rules.
pub struct FilesServer {
    state: Mutex<ServerState>,
    next_id: AtomicU64,
}

impl FilesServer {
    /// Creates an empty files server for `domain`.
    #[must_use]
    pub fn new(domain: &str) -> Arc<FilesServer> {
        let manifest = Manifest {
            domain: domain.to_owned(),
            ..Manifest::default()
        };
        Arc::new(Self {
            state: Mutex::new(ServerState {
                manifest,
                receipts: BTreeMap::new(),
                object_check: None,
                next_time_ms: INITIAL_RECORDED_AT_MS,
                commands: 0,
            }),
            next_id: AtomicU64::new(1),
        })
    }

    fn lock(&self) -> MutexGuard<'_, ServerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn next_id(&self) -> String {
        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        format!("01J{n:023}")
    }

    /// Registers what the object store holds: consulted for every content id
    /// a command names (`content_id` in `payload.operation` or
    /// `payload.operations[*]`) before evaluating; ids it answers `true` for
    /// are inserted into `manifest.objects`.
    pub fn set_objects(&self, has: impl Fn(&str) -> bool + Send + Sync + 'static) {
        self.lock().object_check = Some(Box::new(has));
    }

    /// Marks a content id present in the store.
    pub fn add_object(&self, content_id: &str) {
        self.lock().manifest.objects.insert(content_id.to_owned());
    }

    /// Sets `actor`'s domain role in the manifest.
    pub fn set_role(&self, actor: &str, role: Role) {
        self.lock().manifest.roles.insert(actor.to_owned(), role);
    }

    fn make_server(&self, recorded_at_ms: i64, client_id: &str) -> Server {
        Server {
            event_id: self.next_id(),
            recorded_at_ms,
            client_id: client_id.to_owned(),
            review_id: Some(self.next_id()),
            event_ids: (0..127).map(|_| self.next_id()).collect(),
            review_ids: (0..127).map(|_| self.next_id()).collect(),
        }
    }

    fn prepare_caller_and_objects(
        state: &mut ServerState,
        caller: &Caller,
        params: &Value,
    ) -> Caller {
        let content_ids = collect_content_ids(params);
        if let Some(has) = &state.object_check {
            for cid in &content_ids {
                if has(cid) {
                    state.manifest.objects.insert(cid.clone());
                }
            }
        }
        let mut effective = caller.clone();
        match &mut effective {
            Caller::Agent {
                verified_uploads, ..
            }
            | Caller::Tool {
                verified_uploads, ..
            }
            | Caller::Job {
                verified_uploads, ..
            } => {
                for cid in content_ids {
                    if state.manifest.objects.contains(&cid) && !verified_uploads.contains(&cid) {
                        verified_uploads.push(cid);
                    }
                }
            }
            Caller::Person { .. } | Caller::Import { .. } => {}
        }
        effective
    }

    #[allow(clippy::too_many_arguments)]
    fn record_receipt(
        state: &mut ServerState,
        caller: &Caller,
        client_id: &str,
        descriptor: &str,
        params: &Value,
        recorded_at_ms: i64,
        outcome: String,
        reason: Option<String>,
        event_id: Option<String>,
        seq: Option<String>,
        detail: String,
    ) {
        let request_id = params
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let scope = client_scope(caller.actor_id(), client_id);
        let receipt_id = format!("{descriptor}/{request_id}/{scope}");
        state.receipts.insert(
            receipt_id.clone(),
            ReceiptRow {
                id: receipt_id,
                client_scope: scope,
                descriptor: descriptor.to_owned(),
                request_id,
                request_digest: String::new(),
                outcome,
                reason,
                event_id,
                seq,
                detail,
                recorded_at_ms,
                author_actor: caller.actor_id().to_owned(),
                author_client: client_id.to_owned(),
            },
        );
    }

    fn refusal_receipt_fields(refusal: &Refusal) -> (String, Option<String>, String) {
        let detail = refusal.detail.as_ref().map_or_else(
            || "{}".to_owned(),
            |d| serde_json::to_string(d).unwrap_or_else(|_| "{}".to_owned()),
        );
        if refusal.code == Code::Conflict {
            ("conflict".to_owned(), None, detail)
        } else {
            ("rejected".to_owned(), Some(enum_word(refusal.code)), detail)
        }
    }

    /// Runs one command (`files.propose`, `files.decide`, or any `evaluate`
    /// command) as `caller` from device `client_id` and records its receipt.
    /// Returns `Ok(Some(seq))` when an event was accepted, or `Ok(None)` when
    /// no single event sequence was assigned (e.g. `proposed` or a list).
    pub fn command(
        &self,
        caller: &Caller,
        client_id: &str,
        descriptor: &str,
        params: Value,
    ) -> Result<Option<String>, Refusal> {
        match descriptor {
            "files.propose" => {
                let mut state = self.lock();
                state.commands += 1;
                let effective = Self::prepare_caller_and_objects(&mut state, caller, &params);
                let recorded_at_ms = state.next_time_ms;
                state.next_time_ms += RECORDED_AT_STEP_MS;
                let server = self.make_server(recorded_at_ms, client_id);
                let result = rules::propose(&mut state.manifest, &effective, &params, &server);
                let (outcome, reason, detail) = match &result {
                    Ok(detail) => (
                        "proposed".to_owned(),
                        None,
                        serde_json::to_string(detail).unwrap_or_else(|_| "{}".to_owned()),
                    ),
                    Err(refusal) => Self::refusal_receipt_fields(refusal),
                };
                Self::record_receipt(
                    &mut state,
                    caller,
                    client_id,
                    descriptor,
                    &params,
                    recorded_at_ms,
                    outcome,
                    reason,
                    None,
                    None,
                    detail,
                );
                result.map(|_| None)
            }
            "files.decide" => {
                let mut state = self.lock();
                state.commands += 1;
                let effective = Self::prepare_caller_and_objects(&mut state, caller, &params);
                let recorded_at_ms = state.next_time_ms;
                state.next_time_ms += RECORDED_AT_STEP_MS;
                let server = self.make_server(recorded_at_ms, client_id);
                let result = rules::decide(&mut state.manifest, &effective, &params, &server);
                let (outcome, reason, event_id, seq, detail) = match &result {
                    Ok(receipt) => (
                        enum_word(receipt.outcome),
                        receipt.code.map(enum_word),
                        receipt.event_id.clone(),
                        receipt.seq.clone(),
                        serde_json::to_string(&receipt.detail).unwrap_or_else(|_| "[]".to_owned()),
                    ),
                    Err(refusal) => {
                        let (outcome, reason, detail) = Self::refusal_receipt_fields(refusal);
                        (outcome, reason, None, None, detail)
                    }
                };
                Self::record_receipt(
                    &mut state,
                    caller,
                    client_id,
                    descriptor,
                    &params,
                    recorded_at_ms,
                    outcome,
                    reason,
                    event_id,
                    seq,
                    detail,
                );
                result.map(|receipt| receipt.seq)
            }
            _ => self
                .apply(caller, client_id, descriptor, params)
                .map(|accepted| Some(accepted.event.seq)),
        }
    }

    /// Evaluates one read query (`files.history`, `files.preview_reversal`,
    /// `files.proposal`, `files.proposals`, `files.unreviewed`) as `caller`.
    pub fn query(
        &self,
        caller: &Caller,
        operation: &str,
        params: &Value,
    ) -> Result<Value, Refusal> {
        let state = self.lock();
        match operation {
            "files.history" => {
                let page = rules::history(&state.manifest, params)?;
                serde_json::to_value(page).map_err(|_| Refusal::new(Code::InvalidChange))
            }
            "files.preview_reversal" => {
                let preview = rules::preview_reversal(&state.manifest, caller, params)?;
                serde_json::to_value(preview).map_err(|_| Refusal::new(Code::InvalidChange))
            }
            "files.proposal" => {
                let proposals = rules::proposal(&state.manifest, caller, params)?;
                let value = serde_json::to_value(proposals)
                    .map_err(|_| Refusal::new(Code::InvalidChange))?;
                Ok(json!({ "proposals": value }))
            }
            "files.proposals" => {
                let page = rules::proposals(&state.manifest, caller, params, state.next_time_ms)?;
                serde_json::to_value(page).map_err(|_| Refusal::new(Code::InvalidChange))
            }
            "files.unreviewed" => {
                let page = rules::unreviewed(&state.manifest, caller, params)?;
                serde_json::to_value(page).map_err(|_| Refusal::new(Code::InvalidChange))
            }
            _ => Err(Refusal::new(Code::InvalidChange)),
        }
    }

    /// Runs one command as `caller` from device `client_id` against the
    /// contract's reference rules and records its receipt row.
    pub fn apply(
        &self,
        caller: &Caller,
        client_id: &str,
        descriptor: &str,
        params: Value,
    ) -> Result<Accepted, Refusal> {
        let mut state = self.lock();
        state.commands += 1;

        let effective = Self::prepare_caller_and_objects(&mut state, caller, &params);

        let recorded_at_ms = state.next_time_ms;
        state.next_time_ms += RECORDED_AT_STEP_MS;
        let server = self.make_server(recorded_at_ms, client_id);

        let result = evaluate(
            &mut state.manifest,
            &effective,
            descriptor,
            &params,
            &server,
        );

        let (outcome, reason, event_id, seq, detail) = match &result {
            Ok(accepted) => (
                "accepted".to_owned(),
                None,
                Some(accepted.event.id.clone()),
                Some(accepted.event.seq.clone()),
                "{}".to_owned(),
            ),
            Err(refusal) => {
                let (outcome, reason, detail) = Self::refusal_receipt_fields(refusal);
                (outcome, reason, None, None, detail)
            }
        };

        Self::record_receipt(
            &mut state,
            caller,
            client_id,
            descriptor,
            &params,
            recorded_at_ms,
            outcome,
            reason,
            event_id,
            seq,
            detail,
        );

        result
    }

    /// Seeds a drive named `name` as `owner` (Owner role) and returns its
    /// root entry id.
    pub fn create_drive(&self, owner: &str, name: &str) -> String {
        let id = self.next_id();
        let request_id = self.next_id();
        let params = json!({
            "version": 1,
            "request_id": request_id,
            "context_id": null,
            "payload": {
                "operations": [{
                    "op": "create",
                    "id": id,
                    "kind": "dir",
                    "parent_id": null,
                    "name": name,
                }]
            }
        });
        let caller = Caller::Person {
            actor_id: owner.to_owned(),
            role: Role::Owner,
        };
        self.apply(&caller, "seed", "files.change", params)
            .expect("create_drive refused");
        id
    }

    /// Seeds a directory named `name` under `parent_id` as `actor` (Editor
    /// role) and returns its entry id.
    pub fn mkdir(&self, actor: &str, parent_id: &str, name: &str) -> String {
        let id = self.next_id();
        let request_id = self.next_id();
        let params = json!({
            "version": 1,
            "request_id": request_id,
            "context_id": null,
            "payload": {
                "operations": [{
                    "op": "create",
                    "id": id,
                    "kind": "dir",
                    "parent_id": parent_id,
                    "name": name,
                }]
            }
        });
        let caller = Caller::Person {
            actor_id: actor.to_owned(),
            role: Role::Editor,
        };
        self.apply(&caller, "seed", "files.change", params)
            .expect("mkdir refused");
        id
    }

    /// Seeds a file with `content_id` and `size` under `parent_id` as `actor`
    /// (Editor role), marking the object present first, and returns its id.
    pub fn create_file(
        &self,
        actor: &str,
        parent_id: &str,
        name: &str,
        content_id: &str,
        size: u64,
    ) -> String {
        self.add_object(content_id);
        let id = self.next_id();
        let request_id = self.next_id();
        let params = json!({
            "version": 1,
            "request_id": request_id,
            "context_id": null,
            "payload": {
                "operation": {
                    "op": "create",
                    "id": id,
                    "kind": "file",
                    "parent_id": parent_id,
                    "name": name,
                    "content_id": content_id,
                    "size": size,
                }
            }
        });
        let caller = Caller::Person {
            actor_id: actor.to_owned(),
            role: Role::Editor,
        };
        self.apply(&caller, "seed", "files.put", params)
            .expect("create_file refused");
        id
    }

    /// Moves `id` to (`parent_id`, `name`) at its current revision as `actor`
    /// (Editor role).
    pub fn move_entry(&self, actor: &str, id: &str, parent_id: &str, name: &str) {
        let expected_revision = self
            .entry(id)
            .expect("move_entry: entry not found")
            .revision;
        let request_id = self.next_id();
        let params = json!({
            "version": 1,
            "request_id": request_id,
            "context_id": null,
            "payload": {
                "operation": {
                    "op": "move",
                    "id": id,
                    "expected_revision": expected_revision,
                    "parent_id": parent_id,
                    "name": name,
                }
            }
        });
        let caller = Caller::Person {
            actor_id: actor.to_owned(),
            role: Role::Editor,
        };
        self.apply(&caller, "seed", "files.move", params)
            .expect("move_entry refused");
    }

    /// Returns the entry with `id`, if present.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<Entry> {
        self.lock().manifest.entries.get(id).cloned()
    }

    /// Returns every entry currently in the manifest.
    #[must_use]
    pub fn entries(&self) -> Vec<Entry> {
        self.lock().manifest.entries.values().cloned().collect()
    }

    /// Returns the number of commands applied so far (seed helpers included).
    #[must_use]
    pub fn commands(&self) -> usize {
        self.lock().commands
    }

    /// Returns every proposal revision currently in the manifest, ascending by id.
    #[must_use]
    pub fn proposals(&self) -> Vec<Proposal> {
        self.lock()
            .manifest
            .proposals
            .values()
            .map(|state| state.row.clone())
            .collect()
    }

    /// Returns every proposal-reviewer inbox row currently in the manifest.
    #[must_use]
    pub fn inbox(&self) -> Vec<ProposalReviewer> {
        self.lock().manifest.inbox.values().cloned().collect()
    }

    /// Returns every review row currently in the manifest.
    #[must_use]
    pub fn reviews(&self) -> Vec<Review> {
        self.lock().manifest.reviews.values().cloned().collect()
    }

    /// Returns every accepted event with its operations, ascending by numeric sequence.
    #[must_use]
    pub fn events(&self) -> Vec<(Event, Vec<Op>)> {
        let mut out: Vec<(Event, Vec<Op>)> =
            self.lock().manifest.events.values().cloned().collect();
        out.sort_by(|(a, _), (b, _)| {
            a.seq
                .len()
                .cmp(&b.seq.len())
                .then_with(|| a.seq.cmp(&b.seq))
        });
        out
    }

    /// Returns the domain head row, if any event has been accepted.
    #[must_use]
    pub fn head(&self) -> Option<Head> {
        self.lock().manifest.head.clone()
    }

    fn snapshot(
        &self,
        drives: &BTreeSet<String>,
        domains: &BTreeSet<String>,
        clients: &BTreeSet<String>,
        proposers: &BTreeSet<String>,
        reviewers: &BTreeSet<String>,
    ) -> Snapshot {
        let state = self.lock();
        let entries = state
            .manifest
            .entries
            .values()
            .filter(|entry| drives.contains(&entry.drive_key))
            .map(|entry| {
                let (actor, client) = state
                    .manifest
                    .events
                    .get(&entry.updated_event_id)
                    .map_or_else(
                        || (String::new(), String::new()),
                        |(ev, _)| (ev.actor_id.clone(), ev.client_id.clone()),
                    );
                (entry.clone(), actor, client)
            })
            .collect();
        let head = state
            .manifest
            .head
            .as_ref()
            .filter(|head| domains.contains(&head.domain))
            .map(|head| {
                let (actor, client) = state.manifest.events.get(&head.event_id).map_or_else(
                    || (String::new(), String::new()),
                    |(ev, _)| (ev.actor_id.clone(), ev.client_id.clone()),
                );
                (head.clone(), actor, client)
            });
        let receipts = state
            .receipts
            .values()
            .filter(|receipt| clients.contains(&receipt.client_scope))
            .cloned()
            .collect();
        let proposals = state
            .manifest
            .proposals
            .values()
            .map(|p| &p.row)
            .filter(|row| proposers.contains(&row.proposer_actor))
            .cloned()
            .collect();
        let proposal_reviewers = state
            .manifest
            .inbox
            .values()
            .filter(|row| reviewers.contains(&row.reviewer_actor))
            .map(|row| {
                let key = format!("{}/{}", row.proposal_id, row.revision);
                let client = state
                    .manifest
                    .proposals
                    .get(&key)
                    .map(|p| p.row.proposer_client.clone())
                    .unwrap_or_default();
                (row.clone(), client)
            })
            .collect();
        Snapshot {
            entries,
            head,
            receipts,
            proposals,
            proposal_reviewers,
        }
    }
}

/// A device's local replica of a [`FilesServer`]'s files tables.
pub struct FilesReplica {
    server: Arc<FilesServer>,
    caller: Caller,
    client: String,
    online: AtomicBool,
    requests: AtomicUsize,
    units: Mutex<Vec<Unit>>,
    db: Mutex<rusqlite::Connection>,
    state: watch::Sender<ReplicaState>,
}

impl FilesReplica {
    /// Opens a new connected replica for (`actor`, `client`, `role`) backed
    /// by an in-memory SQLite database holding the files tables.
    #[must_use]
    pub fn new(
        server: Arc<FilesServer>,
        actor: &str,
        client: &str,
        role: Role,
    ) -> Arc<FilesReplica> {
        Self::with_caller(
            server,
            Caller::Person {
                actor_id: actor.to_owned(),
                role,
            },
            client,
        )
    }

    /// Opens a new connected replica for `caller` and device `client` backed
    /// by an in-memory SQLite database holding the files tables.
    #[must_use]
    pub fn with_caller(server: Arc<FilesServer>, caller: Caller, client: &str) -> Arc<Self> {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory sqlite");
        conn.execute_batch(&format!(
            "{CREATE_FILES_ENTRIES};{CREATE_FILES_HEAD};{CREATE_FILES_RECEIPTS};{CREATE_FILES_PROPOSALS};{CREATE_FILES_PROPOSAL_REVIEWERS};"
        ))
        .expect("create files tables");
        let (state, _) = watch::channel(ReplicaState {
            link: LinkState::Connected,
            outbox: 0,
            directory: 0,
            rows: 0,
            presence: 0,
            presence_known: false,
        });
        Arc::new(Self {
            server,
            caller,
            client: client.to_owned(),
            online: AtomicBool::new(true),
            requests: AtomicUsize::new(0),
            units: Mutex::new(Vec::new()),
            db: Mutex::new(conn),
            state,
        })
    }

    /// Sets whether this replica can reach the server and publishes
    /// [`LinkState::Connected`] or [`LinkState::Offline`].
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::SeqCst);
        let link = if online {
            LinkState::Connected
        } else {
            LinkState::Offline
        };
        self.state.send_modify(|state| state.link = link);
    }

    /// Returns the total number of `remote_command` and `remote_query` calls
    /// attempted, whether or not they reached the server.
    #[must_use]
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// Builds a [`Source`] for `database` over this replica.
    #[must_use]
    pub fn source(self: &Arc<Self>, database: &str) -> Source {
        Source {
            database: database.to_owned(),
            replica: Arc::clone(self) as Arc<dyn Replica>,
            actor: self.caller.actor_id().to_owned(),
            client: self.client.clone(),
        }
    }

    /// Pulls subscribed rows from the server now if online.
    pub fn pull(&self) {
        if !self.online.load(Ordering::SeqCst) {
            return;
        }

        let mut drives = BTreeSet::new();
        let mut domains = BTreeSet::new();
        let mut clients = BTreeSet::new();
        let mut proposers = BTreeSet::new();
        let mut reviewers = BTreeSet::new();
        {
            let units = self.units.lock().unwrap_or_else(PoisonError::into_inner);
            for unit in units.iter() {
                if let Some(drive) = unit.scopes.get("files_drive") {
                    drives.insert(drive.clone());
                }
                if let Some(domain) = unit.scopes.get("files_domain") {
                    domains.insert(domain.clone());
                }
                if let Some(client) = unit.scopes.get("files_client") {
                    clients.insert(client.clone());
                }
                if let Some(proposer) = unit.scopes.get("files_proposal") {
                    proposers.insert(proposer.clone());
                }
                if let Some(reviewer) = unit.scopes.get("files_reviewer") {
                    reviewers.insert(reviewer.clone());
                }
            }
        }

        let snapshot = self
            .server
            .snapshot(&drives, &domains, &clients, &proposers, &reviewers);
        let db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
        db.execute_batch(
            "DELETE FROM files_entries; DELETE FROM files_head; DELETE FROM files_receipts; DELETE FROM files_proposals; DELETE FROM files_proposal_reviewers;",
        )
        .expect("clear files tables");

        {
            let mut stmt = db
                .prepare(
                    "INSERT INTO files_entries (
                        id, domain, drive_key, parent_id, name, name_fold, kind,
                        content_id, size, media_type, mtime_ms, mode, revision,
                        lifecycle, created_event_id, updated_event_id, updated_seq,
                        author_actor, author_client
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                )
                .expect("prepare insert files_entries");
            for (entry, author_actor, author_client) in &snapshot.entries {
                let kind =
                    value_to_text(serde_json::to_value(entry.kind).expect("serialize entry kind"));
                let mode =
                    value_to_text(serde_json::to_value(entry.mode).expect("serialize entry mode"));
                let lifecycle = value_to_text(
                    serde_json::to_value(entry.lifecycle).expect("serialize entry lifecycle"),
                );
                let size = entry
                    .size
                    .map(|s| i64::try_from(s).expect("entry size fits i64"));
                stmt.execute(rusqlite::params![
                    entry.id,
                    entry.domain,
                    entry.drive_key,
                    entry.parent_id,
                    entry.name,
                    entry.name_fold,
                    kind,
                    entry.content_id,
                    size,
                    entry.media_type,
                    entry.mtime_ms,
                    mode,
                    entry.revision,
                    lifecycle,
                    entry.created_event_id,
                    entry.updated_event_id,
                    entry.updated_seq,
                    author_actor,
                    author_client,
                ])
                .expect("insert files_entries row");
            }
        }

        if let Some((head, author_actor, author_client)) = &snapshot.head {
            let drives_json = serde_json::to_string(&head.drives).expect("serialize head drives");
            db.execute(
                "INSERT INTO files_head (
                    id, domain, seq, event_id, digest, drives, author_actor, author_client
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    head.id,
                    head.domain,
                    head.seq,
                    head.event_id,
                    head.digest,
                    drives_json,
                    author_actor,
                    author_client,
                ],
            )
            .expect("insert files_head row");
        }

        {
            let mut stmt = db
                .prepare(
                    "INSERT INTO files_receipts (
                        id, client_scope, descriptor, request_id, request_digest,
                        outcome, reason, event_id, seq, detail, recorded_at_ms,
                        author_actor, author_client
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                )
                .expect("prepare insert files_receipts");
            for receipt in &snapshot.receipts {
                stmt.execute(rusqlite::params![
                    receipt.id,
                    receipt.client_scope,
                    receipt.descriptor,
                    receipt.request_id,
                    receipt.request_digest,
                    receipt.outcome,
                    receipt.reason,
                    receipt.event_id,
                    receipt.seq,
                    receipt.detail,
                    receipt.recorded_at_ms,
                    receipt.author_actor,
                    receipt.author_client,
                ])
                .expect("insert files_receipts row");
            }
        }

        insert_proposals(&db, &snapshot.proposals);
        insert_proposal_reviewers(&db, &snapshot.proposal_reviewers);

        drop(db);
        self.state.send_modify(|state| state.rows += 1);
    }
}

fn insert_proposals(db: &rusqlite::Connection, proposals: &[Proposal]) {
    let Ok(mut stmt) = db.prepare(
        "INSERT INTO files_proposals (
            id, domain, proposal_id, revision, status, reason, digest,
            change, reviewers, expires_at_ms, proposer_actor, proposer_client,
            proposer_context, origin_actor, origin_chain, source_labels,
            created_at_ms, decided_at_ms, decided_by, decided_event_id,
            change_set_id, job_id, job_ordinal, job_attempt_id,
            author_actor, author_client
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
        )",
    ) else {
        return;
    };
    for proposal in proposals {
        let status = enum_word(proposal.status);
        let change = serde_json::to_string(&proposal.change).unwrap_or_else(|_| "{}".to_owned());
        let reviewers =
            serde_json::to_string(&proposal.reviewers).unwrap_or_else(|_| "[]".to_owned());
        let origin_chain = proposal
            .origin_chain
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        let source_labels =
            serde_json::to_string(&proposal.source_labels).unwrap_or_else(|_| "[]".to_owned());
        let _ = stmt.execute(rusqlite::params![
            proposal.id,
            proposal.domain,
            proposal.proposal_id,
            proposal.revision,
            status,
            proposal.reason,
            proposal.digest,
            change,
            reviewers,
            proposal.expires_at_ms,
            proposal.proposer_actor,
            proposal.proposer_client,
            proposal.proposer_context,
            proposal.origin_actor,
            origin_chain,
            source_labels,
            proposal.created_at_ms,
            proposal.decided_at_ms,
            proposal.decided_by,
            proposal.decided_event_id,
            proposal.change_set_id,
            proposal.job_id,
            proposal.job_ordinal,
            proposal.job_attempt_id,
            proposal.proposer_actor,
            proposal.proposer_client,
        ]);
    }
}

fn insert_proposal_reviewers(
    db: &rusqlite::Connection,
    proposal_reviewers: &[(ProposalReviewer, String)],
) {
    let Ok(mut stmt) = db.prepare(
        "INSERT INTO files_proposal_reviewers (
            id, domain, reviewer_actor, proposal_id, revision, status,
            proposer_actor, change_set_id, expires_at_ms, op_count,
            author_actor, author_client
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    ) else {
        return;
    };
    for (row, author_client) in proposal_reviewers {
        let status = enum_word(row.status);
        let op_count = i64::from(row.op_count);
        let _ = stmt.execute(rusqlite::params![
            row.id,
            row.domain,
            row.reviewer_actor,
            row.proposal_id,
            row.revision,
            status,
            row.proposer_actor,
            row.change_set_id,
            row.expires_at_ms,
            op_count,
            row.proposer_actor,
            author_client,
        ]);
    }
}

fn json_to_sql(value: &Value) -> Result<rusqlite::types::Value, ReplicaError> {
    match value {
        Value::Null => Ok(rusqlite::types::Value::Null),
        Value::Bool(b) => Ok(rusqlite::types::Value::Integer(i64::from(*b))),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(rusqlite::types::Value::Integer(i))
            } else if let Some(u) = n.as_u64() {
                let i = i64::try_from(u)
                    .map_err(|_| ReplicaError::Client(format!("integer out of range: {u}")))?;
                Ok(rusqlite::types::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(rusqlite::types::Value::Real(f))
            } else {
                Ok(rusqlite::types::Value::Null)
            }
        }
        Value::String(s) => Ok(rusqlite::types::Value::Text(s.clone())),
        Value::Array(_) | Value::Object(_) => Ok(rusqlite::types::Value::Text(value.to_string())),
    }
}

fn sql_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Value::from(f),
        ValueRef::Text(bytes) => Value::String(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Value::String(hex::encode(bytes)),
    }
}

fn offline_error() -> ReplicaError {
    ReplicaError::Unheard {
        code: "transport.failed".to_owned(),
        message: "offline".to_owned(),
        retryable: true,
    }
}

impl Replica for FilesReplica {
    fn mutate(&self, _: Vec<RowOp>) -> Result<String, ReplicaError> {
        Err(ReplicaError::Client(
            "files rows are written by commands".to_owned(),
        ))
    }

    fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        let sql_params: Vec<rusqlite::types::Value> =
            params.iter().map(json_to_sql).collect::<Result<_, _>>()?;
        let db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stmt = db
            .prepare(sql)
            .map_err(|err| ReplicaError::Client(err.to_string()))?;
        let cols: Vec<String> = stmt.column_names().map(str::to_owned).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(sql_params), |row| {
                let mut map = Map::with_capacity(cols.len());
                for (idx, col) in cols.iter().enumerate() {
                    let val = sql_to_json(row.get_ref(idx)?);
                    map.insert(col.clone(), val);
                }
                Ok(map)
            })
            .map_err(|err| ReplicaError::Client(err.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| ReplicaError::Client(err.to_string()))?;
        Ok(rows)
    }

    fn query_covered(
        &self,
        sql: &str,
        params: &[Value],
        _: &WindowBase,
        _: &[String],
    ) -> Result<Covered, ReplicaError> {
        Ok((self.query(sql, params)?, Vec::new()))
    }

    fn subscribe(&self, unit: Unit) -> Result<(), ReplicaError> {
        let mut units = self.units.lock().unwrap_or_else(PoisonError::into_inner);
        if !units.contains(&unit) {
            units.push(unit);
        }
        Ok(())
    }

    fn set_window(&self, _: &WindowBase, _: &[String]) -> Result<(), ReplicaError> {
        Ok(())
    }

    fn window_state(&self, _: &WindowBase) -> Result<WindowState, ReplicaError> {
        Ok(WindowState::default())
    }

    fn set_presence(&self, _: &str, _: Option<Value>) -> Result<(), ReplicaError> {
        Ok(())
    }

    fn presence(&self, _: &str) -> Result<Vec<Peer>, ReplicaError> {
        Ok(Vec::new())
    }

    fn commit_with_blobs(&self, _: Vec<BlobBody>, _: Vec<RowOp>) -> Result<String, ReplicaError> {
        Err(ReplicaError::Client(
            "files rows are written by commands".to_owned(),
        ))
    }

    fn fetch_blob(&self, _: &str) -> Result<Vec<u8>, ReplicaError> {
        Err(ReplicaError::Client(
            "files replica does not serve syncular blobs".to_owned(),
        ))
    }

    fn cached_blob(&self, _: &str) -> Result<Vec<u8>, ReplicaError> {
        Err(ReplicaError::Client(
            "files replica does not serve syncular blobs".to_owned(),
        ))
    }

    fn delivery(&self, _: &str) -> Result<Delivery, ReplicaError> {
        Ok(Delivery::Unknown)
    }

    fn row_writes(&self, _: &str, _: &str, _: Option<&str>) -> Result<RowWrites, ReplicaError> {
        Ok(RowWrites::default())
    }

    fn refused_commits(&self, _: &str) -> Result<Vec<RefusedCommit>, ReplicaError> {
        Ok(Vec::new())
    }

    fn granted(&self, _: &str, _: &str) -> Result<Vec<String>, ReplicaError> {
        Ok(Vec::new())
    }

    fn revoked_units(&self, _: &WindowBase) -> Result<Vec<String>, ReplicaError> {
        Ok(Vec::new())
    }

    fn upload_refusals(&self) -> Vec<UploadRefusal> {
        Vec::new()
    }

    fn blob_cache_bytes(&self) -> Result<u64, ReplicaError> {
        Ok(0)
    }

    fn mutate_superseding(&self, _: Vec<RowOp>, _: Vec<String>) -> Result<String, ReplicaError> {
        Err(ReplicaError::Client(
            "files rows are written by commands".to_owned(),
        ))
    }

    fn remote_query(
        &self,
        operation_id: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if !self.online.load(Ordering::SeqCst) {
            return Err(offline_error());
        }
        match self.server.query(&self.caller, operation_id, &params) {
            Ok(Value::Object(map)) => Ok(vec![map]),
            Ok(_) => Ok(Vec::new()),
            Err(refusal) => Err(ReplicaError::Remote {
                code: enum_word(refusal.code),
                message: String::new(),
                retryable: false,
            }),
        }
    }

    fn remote_command(
        &self,
        operation_id: &str,
        request_id: &str,
        params: Value,
    ) -> Result<CommandResult, ReplicaError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if !self.online.load(Ordering::SeqCst) {
            return Err(offline_error());
        }
        match self
            .server
            .command(&self.caller, &self.client, operation_id, params)
        {
            Ok(seq) => Ok(CommandResult {
                request_id: request_id.to_owned(),
                commit_seq: seq.and_then(|s| s.parse().ok()),
                cached: false,
            }),
            Err(_) => Ok(CommandResult {
                request_id: request_id.to_owned(),
                commit_seq: None,
                cached: false,
            }),
        }
    }

    fn sync_now(&self) {
        self.pull();
    }

    fn state(&self) -> ReplicaState {
        *self.state.borrow()
    }

    fn watch(&self) -> watch::Receiver<ReplicaState> {
        self.state.subscribe()
    }

    fn close(&self) {
        self.set_online(false);
    }

    fn join(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_replicates_entries_and_offline_command_is_unheard() {
        let server = FilesServer::new("alpha");
        let root = server.create_drive("owner-1", "notes");
        let content_id = crate::blob_id(b"hello");
        server.add_object(&content_id);
        let _file = server.create_file("editor-1", &root, "readme.md", &content_id, 5);

        let replica = FilesReplica::new(Arc::clone(&server), "editor-1", "dev-1", Role::Editor);
        replica
            .subscribe(Unit::of("files_drive", "alpha/notes"))
            .expect("subscribe drive");
        replica
            .subscribe(Unit::of("files_domain", "alpha"))
            .expect("subscribe domain");
        replica.pull();

        let rows = replica
            .query(
                "SELECT name FROM files_entries WHERE parent_id = ? AND id <> parent_id",
                &[Value::String(root)],
            )
            .expect("query entries");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get("name").and_then(Value::as_str),
            Some("readme.md")
        );

        replica.set_online(false);
        let err = replica
            .remote_command("files.put", "01J00000000000000000000099", json!({}))
            .expect_err("offline command must fail");
        assert!(matches!(
            err,
            ReplicaError::Unheard {
                retryable: true,
                ..
            }
        ));
        assert_eq!(replica.requests(), 1);
    }

    #[test]
    fn propose_replicates_to_proposer_and_reviewer_and_lists_via_query() {
        let server = FilesServer::new("alpha");
        let root = server.create_drive("owner-1", "notes");
        server.set_role("r", Role::Reviewer);
        let content_id = crate::blob_id(b"draft");
        server.add_object(&content_id);

        let proposer = FilesReplica::new(Arc::clone(&server), "editor-1", "dev-1", Role::Editor);
        proposer
            .subscribe(Unit::of("files_proposal", "editor-1"))
            .expect("subscribe proposer");

        let reviewer = FilesReplica::new(Arc::clone(&server), "r", "dev-2", Role::Reviewer);
        reviewer
            .subscribe(Unit::of("files_reviewer", "r"))
            .expect("subscribe reviewer");

        let request_id = "01J00000000000000000000100";
        let proposal_id = "01J00000000000000000000101";
        let entry_id = "01J00000000000000000000102";
        let params = json!({
            "version": 1,
            "request_id": request_id,
            "context_id": null,
            "payload": {
                "proposal_id": proposal_id,
                "base_revision": null,
                "change": {
                    "operations": [
                        {
                            "op": "create",
                            "id": entry_id,
                            "kind": "file",
                            "parent_id": root,
                            "name": "draft.md",
                            "content_id": content_id,
                            "size": 5
                        }
                    ]
                },
                "reviewers": ["r"],
                "expires_at_ms": null
            }
        });

        let cmd = proposer
            .remote_command("files.propose", request_id, params)
            .expect("propose command");
        assert_eq!(cmd.commit_seq, None);
        assert_eq!(server.proposals().len(), 1);
        assert_eq!(server.inbox().len(), 1);

        proposer.pull();
        reviewer.pull();

        let proposer_rows = proposer
            .query("SELECT proposal_id, status FROM files_proposals", &[])
            .expect("query proposer files_proposals");
        assert_eq!(proposer_rows.len(), 1);
        assert_eq!(
            proposer_rows[0].get("proposal_id").and_then(Value::as_str),
            Some(proposal_id)
        );
        assert_eq!(
            proposer_rows[0].get("status").and_then(Value::as_str),
            Some("open")
        );

        let reviewer_inbox = reviewer
            .query(
                "SELECT reviewer_actor, proposal_id, status FROM files_proposal_reviewers",
                &[],
            )
            .expect("query reviewer files_proposal_reviewers");
        assert_eq!(reviewer_inbox.len(), 1);
        assert_eq!(
            reviewer_inbox[0]
                .get("reviewer_actor")
                .and_then(Value::as_str),
            Some("r")
        );
        assert_eq!(
            reviewer_inbox[0].get("proposal_id").and_then(Value::as_str),
            Some(proposal_id)
        );

        let reviewer_proposals = reviewer
            .query("SELECT id FROM files_proposals", &[])
            .expect("query reviewer files_proposals");
        assert!(reviewer_proposals.is_empty());

        let pages = reviewer
            .remote_query("files.proposals", json!({ "version": 1, "limit": 10 }))
            .expect("query files.proposals");
        assert_eq!(pages.len(), 1);
        let listed = pages[0]
            .get("proposals")
            .and_then(Value::as_array)
            .expect("proposals array");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].get("proposal_id").and_then(Value::as_str),
            Some(proposal_id)
        );
    }
}

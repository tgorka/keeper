//! The Agents window over real on-disk replicas, seeded through `mutate`
//! with no server: the list of two databases, the attention fold, a
//! message's turn state, the windows an archive shrinks (an outbox-pinned
//! row surviving it), new sessions and their database, and decisions.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::hq::approvals::{self, AgentApprovalStateVm, AgentDecisionStateVm};
use crate::hq::port::{ReplicaSpec, RowOp};
use crate::hq::sessions::AgentAttentionVm;
use crate::hq::sessions::{self, object, Source};
use crate::hq::syncular::SyncularReplica;
use crate::hq::timeline::{self, AgentMessageStateVm, Opened};

const ANA: &str = "ana";
const BEN: &str = "ben";
const NIXIE: &str = "nixie";
const STEWARD: &str = "steward";

struct World {
    _dir: tempfile::TempDir,
    alpha: Source,
    beta: Source,
}

fn open(dir: &std::path::Path, database: &str) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: dir.join(format!("{database}.sqlite")),
        client_id: Some("dev1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor: ANA.to_owned(),
        client: "dev1".to_owned(),
    }
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

fn actor(id: &str, kind: &str, name: &str, dir: &str) -> RowOp {
    upsert(
        "actors",
        json!({"id": id, "kind": kind, "display_name": name, "family": null, "dir": dir,
               "created_at_ms": 1, "author_actor": "server", "author_client": "server"}),
    )
}

/// A session of `agent` with ana as its owner, active at `at`.
fn session(id: &str, agent: &str, at: i64, archived: bool, read_until: Option<i64>) -> Vec<RowOp> {
    vec![
        upsert(
            "sessions",
            json!({"id": id, "title": format!("about {id}"), "status": "open",
                   "agent_actor": agent, "created_by_actor": ANA, "created_by_client": "dev1",
                   "preferred_client": null, "parent_session": null, "invite": "[]",
                   "archived": archived, "heard_until": null, "created_at_ms": at,
                   "last_activity_ms": at, "author_actor": ANA, "author_client": "dev1"}),
        ),
        upsert(
            "session_members",
            json!({"id": format!("{id}/{ANA}"), "session_id": id, "actor_id": ANA,
                   "role": "owner", "reads_trace": true, "added_by_actor": ANA,
                   "added_at_ms": at, "read_until": read_until,
                   "author_actor": ANA, "author_client": "dev1"}),
        ),
    ]
}

fn message(
    id: &str,
    session: &str,
    role: &str,
    at: i64,
    turn: Option<&str>,
    metadata: Value,
) -> RowOp {
    let author = if role == "agent" { NIXIE } else { ANA };
    let client = if role == "agent" { "nixie.box" } else { "dev1" };
    upsert(
        "messages",
        json!({"id": id, "session_id": session,
               "page_key": sessions::page_key(session, &sessions::month(at)),
               "turn_id": turn, "role": role, "body": format!("body of {id}"),
               "body_blob": null,
               "metadata": if metadata.is_null() { Value::Null } else { Value::from(metadata.to_string()) },
               "created_at_ms": at, "author_actor": author, "author_client": client}),
    )
}

fn approval(id: &str, session: &str, at: i64, approvers: &[&str], with_call: bool) -> Vec<RowOp> {
    let page = sessions::page_key(session, &sessions::month(at));
    let mut ops = Vec::new();
    if with_call {
        ops.push(upsert(
            "tool_calls",
            json!({"id": format!("call-{id}"), "session_id": session, "page_key": page,
                   "turn_id": "t", "name": "run", "args": "{\"argv\":[\"ls\"]}",
                   "status": "parked", "result": null, "result_blob": null,
                   "started_at_ms": at, "ended_at_ms": null, "created_at_ms": at,
                   "author_actor": NIXIE, "author_client": "nixie.box"}),
        ));
    }
    ops.push(upsert(
        "approvals",
        json!({"id": id, "session_id": session, "page_key": page,
               "tool_call_id": format!("call-{id}"), "tier": "T2",
               "binding_digest": format!("digest-{id}"), "summary": "list files",
               "approvers": serde_json::to_string(approvers).expect("json"),
               "state": "pending", "decided_by_actor": null, "decided_by_client": null,
               "consumed_by_client": null, "expires_at_ms": at + 3_600_000,
               "created_at_ms": at, "author_actor": NIXIE, "author_client": "nixie.box"}),
    ));
    ops
}

fn world() -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    let beta = open(dir.path(), "beta");
    alpha
        .replica
        .mutate(vec![
            actor(NIXIE, "agent", "Nixie", "alpha"),
            actor(STEWARD, "agent", "Steward", "alpha"),
            actor(ANA, "person", "Ana", "alpha"),
        ])
        .expect("alpha directory");
    beta.replica
        .mutate(vec![
            actor(NIXIE, "agent", "Nixie", "beta"),
            actor(ANA, "person", "Ana", "beta"),
        ])
        .expect("beta directory");
    World {
        _dir: dir,
        alpha,
        beta,
    }
}

fn now() -> i64 {
    sessions::now_ms()
}

#[test]
fn both_databases_list_together_each_row_labelled_and_attention_is_rusts() {
    let w = world();
    let t = now() - 60_000;
    let mut alpha = Vec::new();
    // dev1.1: an approval naming ana waits.
    alpha.extend(session("dev1.1", NIXIE, t, false, None));
    alpha.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    // dev1.2: a turn is open.
    alpha.extend(session("dev1.2", STEWARD, t + 1, false, None));
    alpha.push(upsert(
        "turn_chunks",
        json!({"id": "k1", "session_id": "dev1.2", "turn_id": "m1", "seq": 0, "text": "",
               "created_at_ms": t, "author_actor": STEWARD, "author_client": "s.box"}),
    ));
    // dev1.3: answered after ana last read it.
    alpha.extend(session("dev1.3", NIXIE, t + 2, false, Some(t)));
    alpha.push(message(
        "m3",
        "dev1.3",
        "agent",
        t + 2,
        Some("x"),
        Value::Null,
    ));
    // dev1.4: the agent asks; dev1.5: ana answered its ask.
    alpha.extend(session("dev1.4", NIXIE, t + 3, false, Some(t + 10)));
    alpha.push(message(
        "m4",
        "dev1.4",
        "agent",
        t + 3,
        None,
        json!({"kind": "ask", "options": ["yes", "no"]}),
    ));
    alpha.extend(session("dev1.5", NIXIE, t + 4, false, Some(t + 10)));
    alpha.push(message(
        "m5a",
        "dev1.5",
        "agent",
        t + 4,
        None,
        json!({"kind": "ask"}),
    ));
    alpha.push(message("m5b", "dev1.5", "person", t + 5, None, Value::Null));
    w.alpha.replica.mutate(alpha).expect("alpha sessions");
    // The same id in beta is another session.
    w.beta
        .replica
        .mutate(session("dev1.1", NIXIE, t + 6, false, None))
        .expect("beta session");

    let list = sessions::list(&[w.alpha.clone(), w.beta.clone()], now());
    let keys: Vec<(&str, &str)> = list
        .iter()
        .map(|r| (r.database.as_str(), r.session_id.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("beta", "dev1.1"),
            ("alpha", "dev1.5"),
            ("alpha", "dev1.4"),
            ("alpha", "dev1.3"),
            ("alpha", "dev1.2"),
            ("alpha", "dev1.1"),
        ]
    );
    let attention: HashMap<(&str, &str), Option<AgentAttentionVm>> = list
        .iter()
        .map(|r| ((r.database.as_str(), r.session_id.as_str()), r.attention))
        .collect();
    assert_eq!(
        attention[&("alpha", "dev1.1")],
        Some(AgentAttentionVm::NeedsYou)
    );
    assert_eq!(
        attention[&("alpha", "dev1.2")],
        Some(AgentAttentionVm::Working)
    );
    assert_eq!(
        attention[&("alpha", "dev1.3")],
        Some(AgentAttentionVm::DoneUnread)
    );
    assert_eq!(
        attention[&("alpha", "dev1.4")],
        Some(AgentAttentionVm::NeedsYou)
    );
    assert_eq!(attention[&("alpha", "dev1.5")], None);
    assert_eq!(attention[&("beta", "dev1.1")], None);
    let steward = list.iter().find(|r| r.session_id == "dev1.2").expect("row");
    assert_eq!(
        (steward.agent_name.as_str(), steward.agent_mark.as_str()),
        ("Steward", "S")
    );
}

#[test]
fn a_message_is_waiting_until_the_server_has_it_and_an_ask_closes_on_an_answer() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.push(message(
        "m1",
        "dev1.1",
        "agent",
        t,
        None,
        json!({"kind": "ask", "options": ["go", "stop"]}),
    ));
    w.alpha.replica.mutate(ops).expect("seed");
    let (view, _) = timeline::view(
        &w.alpha,
        "dev1.1",
        &Opened::default(),
        &HashMap::new(),
        now(),
    )
    .expect("view");
    let ask = view.messages[0].ask.clone().expect("an ask");
    assert_eq!(
        (ask.options, ask.open),
        (vec!["go".to_owned(), "stop".to_owned()], true)
    );

    crate::hq::send::send(&w.alpha, "dev1.1", "go").expect("sent");
    let (view, newest) = timeline::view(
        &w.alpha,
        "dev1.1",
        &Opened::default(),
        &HashMap::new(),
        now(),
    )
    .expect("view");
    let mine = view.messages.last().expect("the person's message");
    assert_eq!(
        (mine.body.as_str(), mine.state),
        ("go", Some(AgentMessageStateVm::Waiting))
    );
    assert_eq!(view.messages[0].ask.as_ref().map(|a| a.open), Some(false));
    assert_eq!(
        view.messages[0].state, None,
        "an agent's message has no turn state"
    );
    assert_eq!(newest, Some(mine.created_at_ms));
    assert!(!view.read_only && !view.has_earlier);
}

#[test]
fn an_archived_session_takes_no_message_and_is_read_through_the_server() {
    let w = world();
    let t = now() - 60_000;
    w.alpha
        .replica
        .mutate(session("dev1.1", NIXIE, t, true, None))
        .expect("seed");
    assert_eq!(
        crate::hq::send::send(&w.alpha, "dev1.1", "hi").map_err(|e| e.to_string()),
        Err("This conversation is archived; unarchive it to write in it.".to_owned())
    );
    // No server: the remote read is refused with the database's sentence.
    let row = timeline::session_row(&w.alpha, "dev1.1").expect("row");
    let error =
        timeline::fetch_page(&w.alpha, "dev1.1", &row, &Default::default()).expect_err("no server");
    assert_eq!(
        timeline::fetch_sentence("alpha", &error),
        "This conversation is archived on the server; keeper will fetch it when alpha is reachable."
    );
}

#[test]
fn archiving_shrinks_the_windows_and_an_outbox_pinned_row_survives() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(session("dev1.2", NIXIE, t, false, None));
    ops.push(message("m1", "dev1.1", "person", t, None, Value::Null));
    w.alpha.replica.mutate(ops).expect("seed");
    let apply = |source: &Source| {
        for (table, units) in sessions::plan(&sessions::held(source), &HashMap::new(), false, now())
        {
            source
                .replica
                .set_window(&sessions::window(table), &units)
                .expect("window");
        }
    };
    apply(&w.alpha);
    let month = sessions::month(t);
    let messages = sessions::window("messages");
    let held = w
        .alpha
        .replica
        .window_state(&messages)
        .expect("state")
        .units;
    assert!(
        held.contains(&sessions::page_key("dev1.1", &month)),
        "{held:?}"
    );

    sessions::set_archived(&w.alpha, "dev1.1", true).expect("archived");
    apply(&w.alpha);
    let held = w
        .alpha
        .replica
        .window_state(&messages)
        .expect("state")
        .units;
    assert!(!held.iter().any(|u| u.starts_with("dev1.1/")), "{held:?}");
    assert!(
        held.contains(&sessions::page_key("dev1.2", &month)),
        "{held:?}"
    );
    let sessions_held = w
        .alpha
        .replica
        .window_state(&sessions::window("sessions"))
        .expect("state")
        .units;
    assert_eq!(
        sessions_held,
        vec!["dev1.1", "dev1.2"],
        "the list stays local"
    );
    // m1 was never sent: the outbox pins it through the eviction.
    let count = w
        .alpha
        .replica
        .query(
            "SELECT count(*) AS n FROM messages WHERE session_id = 'dev1.1'",
            &[],
        )
        .expect("count");
    assert_eq!(count[0]["n"], json!(1));
    let row = sessions::list(std::slice::from_ref(&w.alpha), now())
        .into_iter()
        .find(|r| r.session_id == "dev1.1")
        .expect("still listed");
    assert!(row.archived);
}

#[test]
fn a_new_session_takes_the_next_id_and_its_database_is_the_only_one_or_the_last_chosen() {
    let w = world();
    let sources = [w.alpha.clone(), w.beta.clone()];
    let first = sessions::create(&w.alpha, NIXIE, "plan the week\nthen more").expect("first");
    let second = sessions::create(&w.alpha, NIXIE, "again").expect("second");
    assert_eq!((first.as_str(), second.as_str()), ("dev1.1", "dev1.2"));
    let row = timeline::session_row(&w.alpha, &first).expect("row");
    assert_eq!(
        (row.title.as_str(), row.agent_actor.as_str()),
        ("plan the week", NIXIE)
    );
    let (view, _) =
        timeline::view(&w.alpha, &first, &Opened::default(), &HashMap::new(), now()).expect("view");
    assert_eq!(view.messages.len(), 1);

    let choices = sessions::choices(&sources, &HashMap::new());
    let nixie = choices
        .iter()
        .find(|c| c.agent_actor == NIXIE)
        .expect("nixie");
    assert_eq!(nixie.databases, vec!["alpha", "beta"]);
    assert!(nixie.ask);
    assert_eq!(nixie.database, None);
    let steward = choices
        .iter()
        .find(|c| c.agent_actor == STEWARD)
        .expect("steward");
    assert_eq!(
        (steward.ask, steward.database.as_deref()),
        (false, Some("alpha"))
    );
    let last = HashMap::from([(NIXIE.to_owned(), "beta".to_owned())]);
    let chosen = sessions::choices(&sources, &last);
    let nixie = chosen
        .iter()
        .find(|c| c.agent_actor == NIXIE)
        .expect("nixie");
    assert_eq!(nixie.database.as_deref(), Some("beta"));

    // The default agent opens its newest active session.
    let default = sessions::default_session(&sources, Some("@nixie:example.org"), &last, now())
        .expect("a default");
    assert_eq!(
        (default.database.as_deref(), default.session_id.as_deref()),
        (Some("alpha"), Some("dev1.2"))
    );
    let fresh = sessions::default_session(&sources, Some("@steward:example.org"), &last, now())
        .expect("a default");
    assert_eq!(
        (fresh.database.as_deref(), fresh.session_id),
        (Some("alpha"), None)
    );
    assert_eq!(
        sessions::default_session(&sources, Some("@nobody:x"), &last, now()),
        None
    );
}

#[test]
fn a_decision_is_bound_to_the_digest_shown_and_to_the_approvers() {
    let w = world();
    let decisions = approvals::Decisions::load(w._dir.path().join("decisions.json"));
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    ops.extend(approval("ap2", "dev1.1", t + 1, &[BEN], true));
    ops.extend(approval("ap3", "dev1.1", t + 2, &[ANA], false));
    w.alpha.replica.mutate(ops).expect("seed");

    let waiting = approvals::waiting(std::slice::from_ref(&w.alpha), &HashMap::new(), now());
    assert_eq!(
        waiting.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec!["ap1"]
    );
    assert!(waiting[0]
        .action
        .as_deref()
        .is_some_and(|a| a.contains("\"ls\"")));
    let all = approvals::of_session(
        &w.alpha,
        "dev1.1",
        &sessions::directory(&w.alpha),
        &HashMap::new(),
        now(),
    );
    assert_eq!(
        all[2].sentence,
        "keeper does not hold this request's action yet, so it cannot be approved here."
    );
    assert!(!all[1].can_decide && all[1].sentence.starts_with("Waiting for"));

    let refused = |id: &str, digest: &str| {
        approvals::decide(&w.alpha, &decisions, "dev1.1", id, digest, true, now())
            .expect_err("refused")
            .to_string()
    };
    assert_eq!(
        refused("ap1", "stale"),
        "This request changed since it was shown; look at it again before deciding."
    );
    assert_eq!(
        refused("ap2", "digest-ap2"),
        "This request is not yours to decide."
    );
    assert!(refused("ap3", "digest-ap3").contains("cannot be approved here"));

    let outcome = approvals::decide(
        &w.alpha,
        &decisions,
        "dev1.1",
        "ap1",
        "digest-ap1",
        true,
        now(),
    )
    .expect("decided");
    assert_eq!(
        outcome.state,
        AgentDecisionStateVm::Waiting,
        "no server: it waits here"
    );
    let decided = approvals::of_session(
        &w.alpha,
        "dev1.1",
        &sessions::directory(&w.alpha),
        &HashMap::new(),
        now(),
    );
    assert_eq!(decided[0].state, AgentApprovalStateVm::Approved);
    assert_eq!(decided[0].sentence, "Approved by Ana.");
    assert_eq!(
        refused("ap1", "digest-ap1"),
        "This request was already decided."
    );
}

/// A replica that answers as the real one does, except that its writes can
/// be held (as behind a stalled round), its commits can be refused (as the
/// server would), the version it reports for every row can be set, and its
/// server commands can be answered by a stand-in server. The People sheet's
/// tests refuse a leave and lose a command's answer through it too.
pub(super) struct Staged {
    inner: Arc<dyn crate::hq::port::Replica>,
    held: std::sync::Mutex<bool>,
    released: std::sync::Condvar,
    pub(super) refused: std::sync::Mutex<HashMap<String, crate::hq::port::Rejection>>,
    version: std::sync::Mutex<Option<i64>>,
    written: std::sync::Mutex<Vec<Vec<RowOp>>>,
    /// Every commit written through it, in order.
    pub(super) commits: std::sync::Mutex<Vec<String>>,
    /// Answers `remote_command` (operation, request id, params) in place
    /// of a server.
    pub(super) server: std::sync::Mutex<Option<ServerCommand>>,
}

/// A stand-in server's answer to one command.
pub(super) type ServerCommand =
    Box<dyn FnMut(&str, &str, &Value) -> Result<(), crate::hq::port::ReplicaError> + Send>;

impl Staged {
    pub(super) fn over(source: &Source, version: Option<i64>) -> (Arc<Staged>, Source) {
        let staged = Arc::new(Staged {
            inner: Arc::clone(&source.replica),
            held: std::sync::Mutex::new(false),
            released: std::sync::Condvar::new(),
            refused: std::sync::Mutex::new(HashMap::new()),
            version: std::sync::Mutex::new(version),
            written: std::sync::Mutex::new(Vec::new()),
            commits: std::sync::Mutex::new(Vec::new()),
            server: std::sync::Mutex::new(None),
        });
        let wrapped = Source {
            replica: Arc::clone(&staged) as Arc<dyn crate::hq::port::Replica>,
            ..source.clone()
        };
        (staged, wrapped)
    }
    /// Report `version` for every row from now on, as a pull that wrote
    /// the rows again would.
    pub(super) fn set_version(&self, version: Option<i64>) {
        *self.version.lock().expect("version") = version;
    }
    fn hold(&self, held: bool) {
        *self.held.lock().expect("held") = held;
        self.released.notify_all();
    }
}

impl crate::hq::port::Replica for Staged {
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, crate::hq::port::ReplicaError> {
        let mut held = self.held.lock().expect("held");
        while *held {
            held = self.released.wait(held).expect("held");
        }
        drop(held);
        self.written.lock().expect("written").push(ops.clone());
        let commit = self.inner.mutate(ops)?;
        self.commits.lock().expect("commits").push(commit.clone());
        Ok(commit)
    }
    fn query(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<Vec<serde_json::Map<String, Value>>, crate::hq::port::ReplicaError> {
        let mut rows = self.inner.query(sql, params)?;
        if let Some(version) = *self.version.lock().expect("version") {
            for row in &mut rows {
                if row.contains_key("version") {
                    row.insert("version".to_owned(), json!(version));
                }
            }
        }
        Ok(rows)
    }
    fn query_covered(
        &self,
        sql: &str,
        params: &[Value],
        base: &crate::hq::port::WindowBase,
        units: &[String],
    ) -> Result<crate::hq::port::Covered, crate::hq::port::ReplicaError> {
        self.inner.query_covered(sql, params, base, units)
    }
    fn subscribe(
        &self,
        s: crate::hq::port::Subscription,
    ) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.subscribe(s)
    }
    fn set_window(
        &self,
        base: &crate::hq::port::WindowBase,
        units: &[String],
    ) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.set_window(base, units)
    }
    fn window_state(
        &self,
        base: &crate::hq::port::WindowBase,
    ) -> Result<crate::hq::port::WindowState, crate::hq::port::ReplicaError> {
        self.inner.window_state(base)
    }
    fn set_presence(
        &self,
        key: &str,
        doc: Option<Value>,
    ) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.set_presence(key, doc)
    }
    fn presence(
        &self,
        key: &str,
    ) -> Result<Vec<crate::hq::port::Peer>, crate::hq::port::ReplicaError> {
        self.inner.presence(key)
    }
    fn commit_with_blobs(
        &self,
        b: Vec<crate::hq::port::BlobBody>,
        ops: Vec<RowOp>,
    ) -> Result<String, crate::hq::port::ReplicaError> {
        self.inner.commit_with_blobs(b, ops)
    }
    fn fetch_blob(&self, b: &str) -> Result<Vec<u8>, crate::hq::port::ReplicaError> {
        self.inner.fetch_blob(b)
    }
    fn cached_blob(&self, b: &str) -> Result<Vec<u8>, crate::hq::port::ReplicaError> {
        self.inner.cached_blob(b)
    }
    fn delivery(
        &self,
        commit: &str,
    ) -> Result<crate::hq::port::Delivery, crate::hq::port::ReplicaError> {
        if let Some(rejection) = self.refused.lock().expect("refused").get(commit) {
            return Ok(crate::hq::port::Delivery::Refused(rejection.clone()));
        }
        self.inner.delivery(commit)
    }
    fn row_writes(
        &self,
        table: &str,
        row_id: &str,
    ) -> Result<crate::hq::port::RowWrites, crate::hq::port::ReplicaError> {
        self.inner.row_writes(table, row_id)
    }
    fn refused_commits(
        &self,
        table: &str,
    ) -> Result<Vec<crate::hq::port::RefusedCommit>, crate::hq::port::ReplicaError> {
        self.inner.refused_commits(table)
    }
    fn upload_refusals(&self) -> Vec<crate::hq::port::UploadRefusal> {
        self.inner.upload_refusals()
    }
    fn blob_cache_bytes(&self) -> Result<u64, crate::hq::port::ReplicaError> {
        self.inner.blob_cache_bytes()
    }
    fn supersede(
        &self,
        commits: Vec<String>,
        replacement: &str,
    ) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.supersede(commits, replacement)
    }
    fn remote_query(
        &self,
        id: &str,
        params: Value,
    ) -> Result<Vec<serde_json::Map<String, Value>>, crate::hq::port::ReplicaError> {
        self.inner.remote_query(id, params)
    }
    fn remote_command(
        &self,
        id: &str,
        request: &str,
        params: Value,
    ) -> Result<(), crate::hq::port::ReplicaError> {
        if let Some(server) = self.server.lock().expect("server").as_mut() {
            return server(id, request, &params);
        }
        self.inner.remote_command(id, request, params)
    }
    fn sync_now(&self) {
        self.inner.sync_now();
    }
    fn state(&self) -> crate::hq::port::ReplicaState {
        self.inner.state()
    }
    fn watch(&self) -> tokio::sync::watch::Receiver<crate::hq::port::ReplicaState> {
        self.inner.watch()
    }
    fn close(&self) {
        self.inner.close();
    }
    fn join(&self) {
        self.inner.join();
    }
}

#[test]
fn sessions_created_at_once_never_share_an_id() {
    let w = world();
    let created: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let alpha = w.alpha.clone();
                scope.spawn(move || {
                    sessions::create(&alpha, NIXIE, &format!("hello {n}")).expect("created")
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().expect("joins"))
            .collect()
    });
    let distinct: std::collections::HashSet<&String> = created.iter().collect();
    assert_eq!(distinct.len(), 8, "{created:?}");
    let firsts = w
        .alpha
        .replica
        .query(
            "SELECT count(*) AS n FROM messages WHERE role = 'person'",
            &[],
        )
        .expect("count");
    assert_eq!(firsts[0]["n"], json!(8), "every first message kept");
}

#[test]
fn a_stale_archive_page_is_dropped_and_a_page_never_lands_twice() {
    let page = |ids: &[&str]| timeline::Fetched {
        rows: ids.iter().map(|id| object(json!({"id": id}))).collect(),
        month: Some("2026-09".to_owned()),
        cursor: Some((1, ids[ids.len() - 1].to_owned())),
        exhausted: false,
    };
    let mut pages = timeline::RemotePages::default();
    pages.begin(1);
    // The view reopened: a new fetch waits.
    pages.begin(2);
    assert!(
        !pages.land(1, Ok(page(&["a", "b"]))),
        "the first fetch's answer is stale"
    );
    assert!(pages.rows.is_empty() && pages.loading);
    assert!(pages.land(2, Ok(page(&["a", "b"]))));
    pages.begin(3);
    assert!(pages.land(3, Ok(page(&["b", "c"]))));
    let ids: Vec<&str> = pages.rows.iter().map(|r| sessions::text(r, "id")).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
    assert_eq!(pages.cursor, Some((1, "c".to_owned())));
    assert!(!pages.land(3, Ok(page(&["z"]))), "an answer lands once");
}

#[test]
fn an_open_session_shows_at_once_while_its_read_point_waits_behind_the_owner() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.push(message("m1", "dev1.1", "agent", t, Some("x"), Value::Null));
    w.alpha.replica.mutate(ops).expect("seed");
    let (staged, source) = Staged::over(&w.alpha, None);
    staged.hold(true);
    let began = std::time::Instant::now();
    let view = crate::hq::window::view_now(
        &source,
        "dev1.1",
        &Opened::default(),
        &HashMap::new(),
        now(),
    )
    .expect("view");
    assert!(
        began.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(view.messages.len(), 1);
    let read_until = || {
        w.alpha
            .replica
            .query(
                "SELECT read_until FROM session_members WHERE id = 'dev1.1/ana'",
                &[],
            )
            .expect("read")[0]["read_until"]
            .clone()
    };
    assert_eq!(read_until(), Value::Null, "the write is still held");
    staged.hold(false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while read_until() != json!(t) {
        assert!(
            std::time::Instant::now() < deadline,
            "the read point never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_refusal_after_the_wait_reaches_the_request_and_survives_a_relaunch() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    w.alpha.replica.mutate(ops).expect("seed");
    let (staged, source) = Staged::over(&w.alpha, None);
    let path = w._dir.path().join("decisions.json");
    let decisions = approvals::Decisions::load(path.clone());
    let outcome = approvals::decide(
        &source,
        &decisions,
        "dev1.1",
        "ap1",
        "digest-ap1",
        true,
        now(),
    )
    .expect("written");
    assert_eq!(outcome.state, AgentDecisionStateVm::Waiting);
    // The server refuses the commit after the wait, and the device restarts.
    let written = staged.written.lock().expect("written").len();
    assert_eq!(written, 1);
    let pending_commit = {
        let raw = std::fs::read_to_string(&path).expect("kept on disk");
        let owed: HashMap<String, Value> = serde_json::from_str(&raw).expect("json");
        owed.keys().next().expect("one owed").clone()
    };
    staged.refused.lock().expect("refused").insert(
        pending_commit,
        crate::hq::port::Rejection::new(
            "hq.v8.tier4_host",
            "a tier-4 decision must come from another host",
        ),
    );
    let relaunched = approvals::Decisions::load(path);
    let refusals = relaunched.refusals(std::slice::from_ref(&source));
    assert_eq!(
        refusals
            .get(&("alpha".to_owned(), "ap1".to_owned()))
            .map(String::as_str),
        Some("a tier-4 decision must come from another host")
    );
    let shown = approvals::of_session(&source, "dev1.1", &HashMap::new(), &refusals, now());
    assert_eq!(shown[0].state, AgentApprovalStateVm::Pending);
    assert!(shown[0].can_decide, "the request can be decided again");
    assert_eq!(
        shown[0].refused.as_deref(),
        Some("a tier-4 decision must come from another host")
    );
    assert_eq!(shown[0].decided_by, None);
    let again = approvals::decide(
        &source,
        &relaunched,
        "dev1.1",
        "ap1",
        "digest-ap1",
        false,
        now(),
    );
    assert!(
        again.is_ok(),
        "a refused decision may be made again: {again:?}"
    );
}

#[test]
fn an_ask_followed_by_more_agent_words_still_needs_you_and_a_failed_turn_is_failed() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, Some(t + 100));
    ops.push(message(
        "m1",
        "dev1.1",
        "agent",
        t + 1,
        None,
        json!({"kind": "ask"}),
    ));
    ops.push(message("m2", "dev1.1", "agent", t + 2, None, Value::Null));
    ops.extend(session("dev1.2", NIXIE, t, false, None));
    ops.push(message("m3", "dev1.2", "person", t + 1, None, Value::Null));
    ops.push(message(
        "m4",
        "dev1.2",
        "agent",
        t + 2,
        Some("m3"),
        json!({"finish": "cut", "error": "the model refused"}),
    ));
    ops.extend(session("dev1.3", NIXIE, t, false, None));
    ops.push(message(
        "m5",
        "dev1.3",
        "agent",
        t + 2,
        None,
        json!({"error": "boom"}),
    ));
    ops.push(upsert(
        "turn_chunks",
        json!({"id": "k5", "session_id": "dev1.3", "turn_id": "m6", "seq": 0, "text": "",
               "created_at_ms": t + 3, "author_actor": NIXIE, "author_client": "nixie.box"}),
    ));
    w.alpha.replica.mutate(ops).expect("seed");
    let attention: HashMap<String, Option<AgentAttentionVm>> =
        sessions::list(std::slice::from_ref(&w.alpha), now())
            .into_iter()
            .map(|r| (r.session_id, r.attention))
            .collect();
    assert_eq!(attention["dev1.1"], Some(AgentAttentionVm::NeedsYou));
    assert_eq!(attention["dev1.2"], Some(AgentAttentionVm::Failed));
    assert_eq!(
        attention["dev1.3"],
        Some(AgentAttentionVm::Working),
        "a new turn supersedes a failed one"
    );
}

/// R-HQ-68: the person's heard row is a mark for the claim holder, not a
/// message — the session draws none, and it answers no ask.
#[test]
fn a_heard_row_is_not_drawn_and_answers_no_ask() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, Some(t + 100));
    ops.push(message("q1", "dev1.1", "person", t + 1, None, Value::Null));
    ops.push(message(
        "a1",
        "dev1.1",
        "agent",
        t + 2,
        Some("q1"),
        json!({"kind": "ask", "options": ["yes", "no"]}),
    ));
    ops.push(message(
        "h1",
        "dev1.1",
        "person",
        t + 3,
        None,
        json!({"kind": "heard", "message": "q1", "heard_until": 12,
               "sentence": 1, "reason": "stop"}),
    ));
    w.alpha.replica.mutate(ops).expect("seed");
    let (view, _) = timeline::view(
        &w.alpha,
        "dev1.1",
        &Opened::default(),
        &HashMap::new(),
        now(),
    )
    .expect("view");
    let ids: Vec<&str> = view.messages.iter().map(|m| m.body.as_str()).collect();
    assert_eq!(ids, ["body of q1", "body of a1"]);
    assert_eq!(view.messages[1].ask.as_ref().map(|a| a.open), Some(true));
    let row = sessions::list(std::slice::from_ref(&w.alpha), now())
        .into_iter()
        .find(|r| r.session_id == "dev1.1")
        .expect("listed");
    assert_eq!(row.attention, Some(AgentAttentionVm::NeedsYou));
}

#[test]
fn the_next_expiry_is_the_soonest_pending_one_still_ahead() {
    let w = world();
    let t = now();
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    // Expiring at t + 1 h, an hour ago, and in half an hour.
    ops.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    ops.extend(approval("ap2", "dev1.1", t - 7_200_000, &[ANA], true));
    ops.extend(approval("ap3", "dev1.1", t - 1_800_000, &[ANA], true));
    w.alpha.replica.mutate(ops).expect("seed");
    let sources = [w.alpha.clone()];
    assert_eq!(approvals::next_expiry(&sources, t), Some(t + 1_800_000));
    assert_eq!(
        approvals::next_expiry(&sources, t + 1_800_000),
        Some(t + 3_600_000)
    );
    assert_eq!(approvals::next_expiry(&sources, t + 3_600_000), None);
    // At the expiry the request stops being actionable, with no row changed.
    let list = |at| sessions::list(&sources, at)[0].attention;
    let before = approvals::of_session(
        &w.alpha,
        "dev1.1",
        &HashMap::new(),
        &HashMap::new(),
        t + 3_599_999,
    );
    let after = approvals::of_session(
        &w.alpha,
        "dev1.1",
        &HashMap::new(),
        &HashMap::new(),
        t + 3_600_000,
    );
    assert!(before[2].can_decide && !after[2].can_decide);
    assert_eq!(after[2].state, AgentApprovalStateVm::Expired);
    assert_eq!(list(t + 3_599_999), Some(AgentAttentionVm::NeedsYou));
    assert_ne!(list(t + 3_600_000), Some(AgentAttentionVm::NeedsYou));
}

#[test]
fn a_decision_names_this_person_and_device_on_the_version_it_read() {
    let w = world();
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(approval("ap1", "dev1.1", t, &[ANA], true));
    w.alpha.replica.mutate(ops).expect("seed");
    // As if the server had sent the approval at version 7.
    let (staged, source) = Staged::over(&w.alpha, Some(7));
    let decisions = approvals::Decisions::load(w._dir.path().join("decisions.json"));
    approvals::decide(
        &source,
        &decisions,
        "dev1.1",
        "ap1",
        "digest-ap1",
        false,
        now(),
    )
    .expect("written");
    let written = staged.written.lock().expect("written").clone();
    let [ops] = &written[..] else {
        panic!("one commit: {written:?}")
    };
    let [RowOp::Patch {
        table,
        values,
        base_version,
    }] = &ops[..]
    else {
        panic!("one patch: {ops:?}")
    };
    assert_eq!(table, "approvals");
    assert_eq!(
        *base_version,
        Some(7),
        "the version read, so a racing decision conflicts"
    );
    for (column, want) in [
        ("id", "ap1"),
        ("state", "denied"),
        ("decided_by_actor", ANA),
        ("decided_by_client", "dev1"),
        ("author_actor", ANA),
        ("author_client", "dev1"),
    ] {
        assert_eq!(values.get(column), Some(&json!(want)), "{column}");
    }
    assert_eq!(
        approvals::decision_op(&w.alpha, "ap1", true, Some(-1)),
        RowOp::Patch {
            table: "approvals".to_owned(),
            values: object(
                json!({"id": "ap1", "state": "approved", "decided_by_actor": ANA,
                                  "decided_by_client": "dev1", "author_actor": ANA, "author_client": "dev1"})
            ),
            base_version: None,
        },
        "an optimistic row has no server version to send"
    );
}

fn pushed_decision(id: &str, approve: bool) -> crate::hq::push::HqActionDecision {
    crate::hq::push::HqActionDecision {
        session: sessions::AgentSessionRef {
            database: "alpha".to_owned(),
            session_id: "dev1.1".to_owned(),
        },
        approval_id: id.to_owned(),
        binding_digest: format!("digest-{id}"),
        approve,
    }
}

/// 106.6: *Approve once* from the lock screen writes the decision row
/// through the session's replica — waiting while a background launch's
/// replica fills — bound to the digest the notification showed, as this
/// person and this device.
#[test]
fn a_lock_screen_decision_waits_for_the_request_then_writes_the_row() {
    let w = world();
    let decisions = approvals::Decisions::load(w._dir.path().join("decisions.json"));
    let t = now() - 60_000;
    let alpha = w.alpha.clone();
    // The request arrives while the action waits, as a first sync would.
    let arriving = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut ops = session("dev1.1", NIXIE, t, false, None);
        ops.extend(approval("ap1", "dev1.1", t, &[ANA], true));
        alpha.replica.mutate(ops).expect("seed");
    });
    let refusal = approvals::decide_pushed(
        || Some(w.alpha.clone()),
        &decisions,
        &pushed_decision("ap1", true),
        std::time::Instant::now() + std::time::Duration::from_secs(5),
    )
    .expect_err("no server: not sent within the action");
    arriving.join().expect("seeded");
    assert_eq!(
        refusal.to_string(),
        "Decided here; it reaches alpha when this device does."
    );
    let decided = approvals::of_session(
        &w.alpha,
        "dev1.1",
        &sessions::directory(&w.alpha),
        &HashMap::new(),
        now(),
    );
    assert_eq!(decided[0].state, AgentApprovalStateVm::Approved);
    let row = &w
        .alpha
        .replica
        .query(
            "SELECT decided_by_actor, decided_by_client FROM approvals WHERE id = 'ap1'",
            &[],
        )
        .expect("reads")[0];
    assert_eq!(row["decided_by_actor"], ANA);
    assert_eq!(row["decided_by_client"], "dev1");
}

/// The replica's own tier decides: *Approve once* on a T4 request is
/// refused whatever the notification carried, *Deny* is written; a request
/// that never arrives is refused at the deadline with why; a digest that
/// changed is refused.
#[test]
fn a_lock_screen_decision_never_approves_t4_and_says_why_it_was_not_sent() {
    let w = world();
    let decisions = approvals::Decisions::load(w._dir.path().join("decisions.json"));
    let t = now() - 60_000;
    let mut ops = session("dev1.1", NIXIE, t, false, None);
    ops.extend(approval("ap4", "dev1.1", t, &[ANA], true));
    ops.push(upsert(
        "approvals",
        serde_json::json!({"id": "ap4", "session_id": "dev1.1",
            "page_key": sessions::page_key("dev1.1", &sessions::month(t)),
            "tool_call_id": "call-ap4", "tier": "T4", "binding_digest": "digest-ap4",
            "summary": "delete the backups", "approvers": "[\"ana\"]", "state": "pending",
            "decided_by_actor": null, "decided_by_client": null, "consumed_by_client": null,
            "expires_at_ms": t + 3_600_000, "created_at_ms": t,
            "author_actor": NIXIE, "author_client": "nixie.box"}),
    ));
    w.alpha.replica.mutate(ops).expect("seed");
    let soon = || std::time::Instant::now() + std::time::Duration::from_millis(200);
    let decide = |decision| {
        approvals::decide_pushed(|| Some(w.alpha.clone()), &decisions, &decision, soon())
            .map_err(|refusal| refusal.to_string())
    };
    assert_eq!(
        decide(pushed_decision("ap4", true)),
        Err(approvals::NOT_FROM_A_NOTIFICATION.to_owned())
    );
    assert_eq!(
        decide(pushed_decision("ap4", false)),
        Err("Decided here; it reaches alpha when this device does.".to_owned()),
        "Deny is written"
    );
    assert_eq!(
        decide(pushed_decision("gone", true)),
        Err("This request is not on this device any more.".to_owned())
    );
    let stale = crate::hq::push::HqActionDecision {
        binding_digest: "other".to_owned(),
        ..pushed_decision("ap4", false)
    };
    assert!(decide(stale).is_err());
    assert_eq!(
        approvals::decide_pushed(|| None, &decisions, &pushed_decision("ap4", false), soon())
            .map_err(|r| r.to_string()),
        Err("This device does not hold alpha yet.".to_owned())
    );
}

#[test]
fn agent_commands_keep_history_and_create_no_phantom_prompt() {
    use crate::hq::commands::{run, AgentCommandSurface};
    let w = world();
    w.alpha
        .replica
        .mutate(session("old", NIXIE, now(), false, None))
        .expect("session");
    w.alpha
        .replica
        .mutate(vec![message(
            "m1",
            "old",
            "person",
            now(),
            None,
            Value::Null,
        )])
        .expect("history");
    for draft in ["/clean", "/new", "/new A fresh title"] {
        let next = run(&w.alpha, "old", draft)
            .expect("fresh session")
            .session
            .expect("navigation");
        assert_eq!(next.database, "alpha");
        assert_ne!(next.session_id, "old");
        let row = timeline::session_row(&w.alpha, &next.session_id).expect("new row");
        assert_eq!(row.agent_actor, NIXIE);
        if draft.contains("A fresh") {
            assert_eq!(row.title, "A fresh title");
        }
        assert!(
            sessions::read(
                &w.alpha,
                "SELECT id FROM messages WHERE session_id = ?1",
                &[next.session_id.into()]
            )
            .is_empty(),
            "fresh context must not dispatch a fake message"
        );
        assert!(
            !timeline::session_row(&w.alpha, "old")
                .expect("old preserved")
                .archived
        );
        assert_eq!(
            sessions::read(&w.alpha, "SELECT body FROM messages WHERE id = 'm1'", &[])[0]["body"],
            "body of m1"
        );
    }
    assert!(run(&w.alpha, "old", "/rename").is_err());
    run(&w.alpha, "old", "/rename New title").expect("rename");
    assert_eq!(
        timeline::session_row(&w.alpha, "old").expect("row").title,
        "New title"
    );
    for (draft, surface) in [
        ("/people", AgentCommandSurface::People),
        ("/context", AgentCommandSurface::Context),
        ("/help", AgentCommandSurface::Help),
    ] {
        assert_eq!(
            run(&w.alpha, "old", draft).expect("open").surface,
            Some(surface)
        );
    }
    assert!(
        run(&w.alpha, "old", "/stop").is_err(),
        "no turn means no control row"
    );
    run(&w.alpha, "old", "/archive").expect("archive");
    assert!(
        timeline::session_row(&w.alpha, "old")
            .expect("preserved")
            .archived
    );
    assert!(run(&w.alpha, "old", "/rename archived").is_err());
}

#[test]
fn agent_commands_filter_capabilities_and_never_send_unknown_commands_as_prose() {
    use crate::hq::commands::preview;
    use crate::vm::ChatCommandVerdictVm;
    let names = |tools| {
        preview("/", tools)
            .rows
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(false),
        ["clean", "new", "rename", "stop", "context", "people", "archive", "help"]
    );
    assert!(names(true).contains(&"tools".to_owned()));
    for draft in [
        "/unknown",
        "/model some-model",
        "/effort high",
        "/drives",
        "/tools",
    ] {
        assert!(
            matches!(
                preview(draft, false).verdict,
                ChatCommandVerdictVm::Refusal { .. }
            ),
            "{draft}"
        );
    }
    // These verdicts also govern attachment captions: only an explicit escape
    // turns a command-looking caption into text, including unknown commands.
    for caption in ["/stop", "/unknown", "/model unavailable"] {
        let escaped = format!("/{caption}");
        assert!(
            matches!(preview(&escaped, false).verdict, ChatCommandVerdictVm::Text { text } if text == caption),
            "escaped caption {escaped} must retain exactly one leading slash"
        );
    }
    assert_eq!(preview("/ren", false).rows[0].name, "rename");
    assert!(
        matches!(preview("/stop", false).verdict, ChatCommandVerdictVm::Command { name, args: None } if name == "stop")
    );
}

#[test]
fn stop_control_names_only_unfinished_turns_and_tools_open_the_existing_surface() {
    use crate::hq::commands::{run, AgentCommandSurface};
    let w = world();
    let t = now();
    w.alpha
        .replica
        .mutate(session("s", NIXIE, t, false, None))
        .expect("session");
    let chunk = |id: &str| {
        upsert(
            "turn_chunks",
            json!({"id":format!("chunk-{id}"), "session_id":"s", "turn_id":id, "seq":0, "text":"", "created_at_ms":t, "author_actor":NIXIE,"author_client":"nixie.box"}),
        )
    };
    w.alpha
        .replica
        .mutate(vec![
            chunk("live"),
            chunk("done"),
            message("answer", "s", "agent", t, Some("done"), Value::Null),
        ])
        .expect("turns");
    run(&w.alpha, "s", "/stop").expect("stop");
    let controls = sessions::read(
        &w.alpha,
        "SELECT metadata, role FROM messages WHERE session_id = 's' AND role = 'person'",
        &[],
    );
    assert_eq!(controls.len(), 1);
    let metadata: Value =
        serde_json::from_str(controls[0]["metadata"].as_str().expect("json string")).expect("json");
    assert_eq!(metadata, json!({"kind":"stop","turn":"live"}));
    w.alpha.replica.mutate(vec![upsert("agent_profiles", json!({"id":"nixie.box", "actor_id":NIXIE, "name":"Nixie", "description":"", "avatar_blob":null, "definition_hash":"h", "image_digest":"d", "started_at_ms":t, "dir":"alpha", "tools":"[\"read\"]", "author_actor":NIXIE, "author_client":"nixie.box"}))]).expect("tools");
    assert_eq!(
        run(&w.alpha, "s", "/tools").expect("tools").surface,
        Some(AgentCommandSurface::Tools)
    );
}

//! 109.24 over real on-disk replicas: a mention's one commit and its V3
//! shape, what is refused before anything is written, the completion, the
//! child's answer inline in its parent, and a server refusal on the
//! person's line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use super::{parse, preview, refusal_sentence, start, MentionError};
use crate::hq::port::{Rejection, ReplicaSpec, RowOp};
use crate::hq::sessions::{self, object, Source};
use crate::hq::syncular::SyncularReplica;
use crate::hq::timeline::{self, AgentMessageStateVm, AgentTimelineItemVm as Item, Opened};

const ANA: &str = "ana";
const NIXIE: &str = "nixie";
const TOLA: &str = "tola-grey";
const SKIFF: &str = "skiff";
const PARENT: &str = "dev1.1";

fn replica_path(dir: &Path, database: &str) -> PathBuf {
    dir.join(format!("{database}.sqlite"))
}

fn open(dir: &Path, database: &str) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: replica_path(dir, database),
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

struct World {
    dir: tempfile::TempDir,
    alpha: Source,
    beta: Source,
    at: i64,
}

impl World {
    fn sources(&self) -> Vec<Source> {
        vec![self.alpha.clone(), self.beta.clone()]
    }

    fn view(&self) -> timeline::AgentSessionViewVm {
        timeline::view(
            &self.alpha,
            PARENT,
            &Opened::default(),
            &HashMap::new(),
            sessions::now_ms(),
        )
        .expect("the parent's view")
        .0
    }

    /// The outbox's commits, oldest first: each id and its operations.
    fn outbox(&self) -> Vec<(String, Vec<Value>)> {
        let conn =
            rusqlite::Connection::open(replica_path(self.dir.path(), "alpha")).expect("file");
        let mut statement = conn
            .prepare("SELECT commit_id, ops_json FROM _syncular_outbox ORDER BY seq")
            .expect("prepares");
        let rows: Vec<(String, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("reads")
            .map(|row| row.expect("a row"))
            .collect();
        rows.into_iter()
            .map(|(id, ops)| {
                let ops: Value = serde_json::from_str(&ops).expect("json");
                (id, ops.as_array().cloned().unwrap_or_default())
            })
            .collect()
    }
}

/// alpha: Nixie (the parent's agent), Tola Grey and Ana; beta: Skiff. Ana's
/// session with Nixie in alpha, a minute old.
fn world() -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    let beta = open(dir.path(), "beta");
    let at = sessions::now_ms() - 60_000;
    alpha
        .replica
        .mutate(vec![
            actor(NIXIE, "agent", "Nixie", "alpha"),
            actor(TOLA, "agent", "Tola Grey", "alpha"),
            actor(ANA, "person", "Ana", "alpha"),
            upsert(
                "sessions",
                json!({"id": PARENT, "title": "plans", "status": "open", "agent_actor": NIXIE,
                       "created_by_actor": ANA, "created_by_client": "dev1",
                       "preferred_client": null, "parent_session": null, "invite": "[]",
                       "archived": false, "heard_until": null, "created_at_ms": at,
                       "last_activity_ms": at, "author_actor": ANA, "author_client": "dev1"}),
            ),
            upsert(
                "session_members",
                json!({"id": format!("{PARENT}/{ANA}"), "session_id": PARENT, "actor_id": ANA,
                       "role": "owner", "reads_trace": true, "added_by_actor": ANA,
                       "added_at_ms": at, "read_until": null,
                       "author_actor": ANA, "author_client": "dev1"}),
            ),
        ])
        .expect("alpha");
    beta.replica
        .mutate(vec![
            actor(SKIFF, "agent", "Skiff", "beta"),
            actor(ANA, "person", "Ana", "beta"),
        ])
        .expect("beta");
    World {
        dir,
        alpha,
        beta,
        at,
    }
}

fn op<'a>(ops: &'a [Value], table: &str) -> &'a Map<String, Value> {
    let found: Vec<&Value> = ops.iter().filter(|op| op["table"] == table).collect();
    assert_eq!(found.len(), 1, "one {table} row in {ops:?}");
    found[0]["values"].as_object().expect("values")
}

fn mention(w: &World, line: &str) -> (String, String) {
    start(
        &w.sources(),
        &w.alpha,
        PARENT,
        parse(line).expect("a mention"),
        line,
        None,
        || Ok(false),
    )
    .expect("started")
}

#[test]
fn a_mention_is_one_commit_of_the_child_its_owner_and_the_line_in_v3s_shape() {
    let w = world();
    let line = "@tola-grey: what do the notes say about the release?";
    let before = w.outbox().len();
    let (child, commit) = mention(&w, line);

    let outbox = w.outbox();
    assert_eq!(outbox.len(), before + 1, "exactly one new commit");
    let (id, ops) = outbox.last().expect("the mention's commit");
    assert_eq!(id, &commit);
    assert_eq!(ops.len(), 3, "the child, its owner and the line: {ops:?}");
    // V3: a person member of the parent inserts a child of another agent of
    // this database, created by this actor and client, its id this client's.
    let session = op(ops, "sessions");
    assert!(child.starts_with("dev1.") && child != PARENT);
    assert_eq!(session["id"], child.as_str());
    assert_eq!(session["parent_session"], PARENT);
    assert_eq!(session["agent_actor"], TOLA);
    assert_eq!(session["created_by_actor"], ANA);
    assert_eq!(session["created_by_client"], "dev1");
    assert_eq!(session["invite"], "[]");
    assert_eq!(session["archived"], false);
    assert_eq!(
        session["title"], "what do the notes say about the release?",
        "titled by what follows the handle"
    );
    let member = op(ops, "session_members");
    assert_eq!(member["session_id"], child.as_str());
    assert_eq!(member["actor_id"], ANA);
    assert_eq!(member["role"], "owner");
    let message = op(ops, "messages");
    assert_eq!(message["session_id"], child.as_str());
    assert_eq!(message["role"], "person");
    assert_eq!(message["body"], line, "the person's line, as written");
    assert_eq!(
        message["created_at_ms"], session["created_at_ms"],
        "written together"
    );
    // Nothing is written into the parent: its agent is not asked.
    assert!(ops
        .iter()
        .all(|op| op["values"]["id"] != PARENT && op["values"]["session_id"] != PARENT));
}

#[test]
fn the_sessions_own_agent_another_databases_and_an_unknown_handle_are_refused_writing_nothing() {
    let w = world();
    let before = w.outbox().len();
    let refuse = |line: &str| {
        start(
            &w.sources(),
            &w.alpha,
            PARENT,
            parse(line).expect("a mention"),
            line,
            None,
            || panic!("a mention refused here declines nothing"),
        )
        .expect_err("refused")
    };
    let own = refuse("@nixie look again");
    assert_eq!(
        own,
        MentionError::Own {
            name: "Nixie".to_owned(),
            handle: NIXIE.to_owned()
        }
    );
    assert_eq!(
        own.to_string(),
        "Nixie already answers this conversation; write without @nixie."
    );
    assert_eq!(
        refuse("@Skiff ahoy").to_string(),
        "Skiff is an agent of beta, not alpha: a conversation brings in only agents of its own database."
    );
    assert_eq!(
        refuse("@nobody hello").to_string(),
        "No agent of alpha is called @nobody."
    );
    assert_eq!(
        refuse("@ana hello").to_string(),
        "No agent of alpha is called @ana.",
        "a person is no agent"
    );
    assert_eq!(
        refuse("@tola-grey   ").to_string(),
        "Write something for Tola Grey after @tola-grey."
    );
    assert_eq!(w.outbox().len(), before, "nothing written");
    // A line that does not start with a mention is the session agent's.
    assert_eq!(parse("ask @tola-grey later"), None);
    assert_eq!(parse("@ hello"), None);
    assert_eq!(
        parse("  @TOLA-GREY, hi").map(|m| (m.handle, m.rest)),
        Some(("TOLA-GREY", "hi"))
    );
}

#[test]
fn the_completion_offers_the_databases_other_agents_by_handle_and_name() {
    let w = world();
    let inserts = |before: &str, after: &str| {
        let answer = preview(&w.alpha, PARENT, before, after);
        (
            answer.token,
            answer
                .rows
                .into_iter()
                .map(|row| row.insert)
                .collect::<Vec<_>>(),
        )
    };
    // Never the session's own agent; never beta's.
    assert_eq!(
        inserts("@", ""),
        (Some("@".to_owned()), vec!["@tola-grey".to_owned()])
    );
    assert_eq!(
        inserts("@gr", " hi"),
        (Some("@gr".to_owned()), vec!["@tola-grey".to_owned()]),
        "a word of the name"
    );
    assert_eq!(inserts("@ni", "").1, Vec::<String>::new());
    assert_eq!(inserts("@sk", "").1, Vec::<String>::new());
    // Only a leading @ word, the caret at its end.
    assert_eq!(inserts("hi @to", "").0, None);
    assert_eq!(inserts("@to", "la").0, None);
    assert_eq!(inserts("@tola-grey hi", "").0, None);
}

#[test]
fn the_childs_latest_answer_is_drawn_inline_where_it_was_started() {
    let w = world();
    let line = "@tola-grey summarise the notes";
    let (child, _) = mention(&w, line);
    let started = w.view();
    let shown = &started.children[0];
    assert_eq!(
        (shown.session_id.as_str(), shown.agent_name.as_str()),
        (child.as_str(), "Tola Grey")
    );
    let prompt = shown.prompt.clone().expect("the line");
    assert_eq!(
        (prompt.body.as_str(), prompt.state),
        (line, Some(AgentMessageStateVm::Waiting))
    );
    assert!(
        shown.items.is_empty() && shown.refusal.is_none(),
        "not answered yet"
    );

    // Tola's instance answers in the child (as a claim holder writes it);
    // later, Ana writes to Nixie in the parent.
    let now = sessions::now_ms();
    let page = |session: &str| sessions::page_key(session, &sessions::month(now));
    w.alpha
        .replica
        .mutate(vec![
            upsert(
                "messages",
                json!({"id": "a-tola", "session_id": child, "page_key": page(&child),
                       "turn_id": prompt.id, "role": "agent",
                       "body": "The notes say: ship on Friday.", "body_blob": null,
                       "metadata": json!({"finish": "stop", "duration_ms": 900}).to_string(),
                       "created_at_ms": now + 1, "author_actor": TOLA,
                       "author_client": "tola-grey.box"}),
            ),
            upsert(
                "messages",
                json!({"id": "m-later", "session_id": PARENT, "page_key": page(PARENT),
                       "turn_id": null, "role": "person", "body": "and you, Nixie?",
                       "body_blob": null, "metadata": null, "created_at_ms": now + 2,
                       "author_actor": ANA, "author_client": "dev1"}),
            ),
        ])
        .expect("answered");

    let view = w.view();
    assert_eq!(view.children.len(), 1);
    let shown = &view.children[0];
    assert_eq!(
        shown.items,
        vec![
            Item::Fold {
                turn_id: prompt.id.clone()
            },
            Item::Message {
                message_id: "a-tola".to_owned()
            },
        ],
        "the latest turn, folded as 109.13 folds it"
    );
    assert_eq!(shown.folds.len(), 1);
    assert_eq!(shown.folds[0].status_label, "Finished");
    assert_eq!(
        shown
            .messages
            .iter()
            .map(|m| (m.body.as_str(), m.author_name.as_str()))
            .collect::<Vec<_>>(),
        vec![("The notes say: ship on Friday.", "Tola Grey")],
        "labelled with the agent"
    );
    // In the parent's column where it was started, before the later line;
    // the parent's own messages hold nothing of the child.
    assert_eq!(
        view.items,
        vec![
            Item::Child {
                thread_id: prompt.id.clone()
            },
            Item::Message {
                message_id: "m-later".to_owned()
            },
        ]
    );
    assert!(view
        .messages
        .iter()
        .all(|m| m.id != "a-tola" && m.body != line));
    // The child's own view carries no children of its own.
    let (own, _) = timeline::view(
        &w.alpha,
        &child,
        &Opened::default(),
        &HashMap::new(),
        sessions::now_ms(),
    )
    .expect("the child's view");
    assert!(own.children.is_empty());
}

/// What a round does with a refused commit, on the file: the server's
/// refusal journalled with the operations the commit wrote, and the commit
/// out of the outbox — so its rows, purely optimistic, are gone on reopen.
fn refuse_on_the_server(w: World, commit: &str, code: &str) -> World {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let World {
        dir,
        alpha,
        beta,
        at,
    } = w;
    for source in [&alpha, &beta] {
        source.replica.close();
        source.replica.join();
    }
    drop((alpha, beta));
    let conn = rusqlite::Connection::open(replica_path(dir.path(), "alpha")).expect("file");
    let ops: String = conn
        .query_row(
            "SELECT ops_json FROM _syncular_outbox WHERE commit_id = ?1",
            [commit],
            |row| row.get(0),
        )
        .expect("the commit waits");
    // The outbox's operations, as the outcome journal records a commit's.
    let ops: Vec<Value> = serde_json::from_str(&ops).expect("json");
    let operations: Vec<CommitOperation> = ops
        .iter()
        .map(|op| CommitOperation {
            table: op["table"].as_str().expect("a table").to_owned(),
            row_id: op["rowId"].as_str().expect("a row id").to_owned(),
            op: op["op"].as_str().unwrap_or("upsert").to_owned(),
            base_version: None,
            values: op["values"].as_object().cloned(),
        })
        .collect();
    let results = vec![CommitOperationOutcome::Error {
        rejection: RejectionRecord {
            client_commit_id: commit.to_owned(),
            op_index: 0,
            code: code.to_owned(),
            message: code.to_owned(),
            retryable: false,
            details: None,
            operation: None,
        },
    }];
    conn.execute(
        "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
         results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
        rusqlite::params![
            commit,
            serde_json::to_string(&results).expect("results"),
            serde_json::to_string(&operations).expect("operations")
        ],
    )
    .expect("journals");
    conn.execute(
        "DELETE FROM _syncular_outbox WHERE commit_id = ?1",
        [commit],
    )
    .expect("drops");
    drop(conn);
    World {
        alpha: open(dir.path(), "alpha"),
        beta: open(dir.path(), "beta"),
        dir,
        at,
    }
}

#[test]
fn a_server_refusal_stays_on_the_persons_line_in_plain_words() {
    let w = world();
    let line = "@tola-grey check the notes";
    let (child, commit) = mention(&w, line);
    let w = refuse_on_the_server(w, &commit, "hq.v3.parent_not_member");
    assert!(
        timeline::session_row(&w.alpha, &child).is_none(),
        "the child never existed"
    );
    let view = w.view();
    assert_eq!(view.children.len(), 1, "the line is not lost");
    let shown = &view.children[0];
    assert_eq!(
        (shown.session_id.as_str(), shown.agent_name.as_str()),
        (child.as_str(), "Tola Grey")
    );
    assert_eq!(
        shown.refusal.as_deref(),
        Some(
            "The agents server refused to bring Tola Grey in: you are no longer a member of this \
             conversation. Nothing was sent to Tola Grey."
        )
    );
    let prompt = shown.prompt.as_ref().expect("the person's line");
    assert_eq!(
        (prompt.body.as_str(), prompt.state, prompt.mine),
        (line, Some(AgentMessageStateVm::Refused), true),
        "a refused line is this replica's own, so it stays the person's bubble"
    );
    assert!(shown.created_at_ms >= w.at);
    assert_eq!(
        view.items,
        vec![Item::Child {
            thread_id: prompt.id.clone()
        }]
    );
    // V11's words, and any other code in the server's own.
    assert_eq!(
        refusal_sentence(
            &Rejection::new("hq.v11.agent_outside_directory", ""),
            "Tola Grey",
            "alpha"
        ),
        "The agents server refused to bring Tola Grey in: Tola Grey is not an agent of alpha. \
         Nothing was sent to Tola Grey."
    );
    assert_eq!(
        refusal_sentence(
            &Rejection::new("hq.v3.starter", "not yours"),
            "Tola Grey",
            "alpha"
        ),
        "The agents server refused to bring Tola Grey in: hq.v3.starter: not yours. Nothing was \
         sent to Tola Grey."
    );
}

/// R10924K-02: 200 threads started from one parent, each answered with 20 KB:
/// the parent's view carries a bounded number of them, each bounded in size.
#[test]
fn many_long_threads_keep_the_parents_view_bounded() {
    let w = world();
    let answer = "The notes say ship it. ".repeat(900);
    assert!(answer.len() > 20_000);
    let now = sessions::now_ms();
    let mut ops = Vec::new();
    for n in 0..200 {
        let line = format!("@tola-grey question {n}");
        let (child, _) = mention(&w, &line);
        let first = w
            .alpha
            .replica
            .query(
                "SELECT id FROM messages WHERE session_id = ?1",
                &[Value::from(child.as_str())],
            )
            .expect("reads")[0]["id"]
            .as_str()
            .expect("an id")
            .to_owned();
        ops.push(upsert(
            "messages",
            json!({"id": format!("a-{n}"), "session_id": child,
                   "page_key": sessions::page_key(&child, &sessions::month(now)),
                   "turn_id": first, "role": "agent", "body": answer, "body_blob": null,
                   "metadata": json!({"finish": "stop"}).to_string(),
                   "created_at_ms": now + 1, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        ));
    }
    w.alpha.replica.mutate(ops).expect("answered");
    let started = std::time::Instant::now();
    let view = w.view();
    let elapsed = started.elapsed();
    let bytes = serde_json::to_string(&view).expect("json").len();
    eprintln!(
        "MEASURE children={} items={} view={:?} vm_bytes={bytes}",
        view.children.len(),
        view.items.len(),
        elapsed
    );
    assert_eq!(
        view.children.len(),
        super::MAX_INLINE_CHILDREN,
        "the newest only"
    );
    assert_eq!(
        view.children_note.as_deref(),
        Some("170 earlier threads started here are not shown; open them from the sessions list.")
    );
    assert_eq!(
        view.children
            .last()
            .and_then(|child| child.prompt.as_ref())
            .map(|prompt| prompt.body.as_str()),
        Some("@tola-grey question 199")
    );
    for child in &view.children {
        assert!(child.truncated);
        let answer = &child.messages[0].body;
        assert!(answer.chars().count() <= super::MAX_INLINE_ANSWER + 1 && answer.ends_with('…'));
    }
    assert!(bytes < 120_000, "the view is bounded: {bytes} bytes");
}

/// R10924K-01: a refused child's session id is minted again by the next
/// mention. Two refusals stay two lines; an accepted mention under the same
/// id hides neither.
#[test]
fn refusals_are_told_apart_by_their_line_not_their_session_id() {
    let mut w = world();
    let mut refused = Vec::new();
    for line in ["@tola-grey one", "@tola-grey two"] {
        let (child, commit) = mention(&w, line);
        refused.push(child);
        w = refuse_on_the_server(w, &commit, "hq.v3.parent_not_member");
    }
    let (third, _) = mention(&w, "@tola-grey three");
    assert_eq!(
        (refused[0].as_str(), refused[1].as_str()),
        (third.as_str(), third.as_str()),
        "the same id, minted three times"
    );
    let view = w.view();
    let lines: Vec<(&str, Option<AgentMessageStateVm>)> = view
        .children
        .iter()
        .map(|child| {
            let prompt = child.prompt.as_ref().expect("a line");
            (prompt.body.as_str(), prompt.state)
        })
        .collect();
    assert_eq!(
        lines,
        vec![
            ("@tola-grey one", Some(AgentMessageStateVm::Refused)),
            ("@tola-grey two", Some(AgentMessageStateVm::Refused)),
            ("@tola-grey three", Some(AgentMessageStateVm::Waiting)),
        ]
    );
    let threads: Vec<&str> = view
        .children
        .iter()
        .map(|child| child.thread_id.as_str())
        .collect();
    let mut distinct = threads.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "three threads: {threads:?}");
    assert_eq!(
        view.items,
        threads
            .iter()
            .map(|thread| Item::Child {
                thread_id: (*thread).to_owned()
            })
            .collect::<Vec<_>>()
    );
}

/// R10924K-03: the local checks run before anything is declined; a mention
/// that passes them declines before its commit, and a decline that fails
/// writes nothing.
#[test]
fn a_mention_declines_after_its_checks_and_before_its_commit() {
    let w = world();
    let before = w.outbox().len();
    let declined = std::cell::Cell::new(0);
    for line in ["@nobody hi", "@nixie hi", "@skiff hi", "@tola-grey"] {
        start(
            &w.sources(),
            &w.alpha,
            PARENT,
            parse(line).expect("a mention"),
            line,
            None,
            || {
                declined.set(declined.get() + 1);
                Ok(true)
            },
        )
        .expect_err("refused here");
    }
    assert_eq!(declined.get(), 0, "a mention refused here declines nothing");
    let failed = start(
        &w.sources(),
        &w.alpha,
        PARENT,
        parse("@tola-grey hi").expect("a mention"),
        "@tola-grey hi",
        None,
        || Err("This device cannot decline that request.".to_owned()),
    )
    .expect_err("the decline failed");
    assert_eq!(
        failed.to_string(),
        "This device cannot decline that request."
    );
    assert_eq!(w.outbox().len(), before, "nothing written");
    let line = "@tola-grey hi";
    start(
        &w.sources(),
        &w.alpha,
        PARENT,
        parse(line).expect("a mention"),
        line,
        None,
        || {
            assert_eq!(
                w.outbox().len(),
                before,
                "declined before the child's commit"
            );
            Ok(true)
        },
    )
    .expect("started");
    assert_eq!(w.outbox().len(), before + 1);
}

/// R10924K-04: an archived thread's rows are not held; inline it is only
/// archived, with nothing else drawn.
#[test]
fn an_archived_thread_is_drawn_as_archived() {
    let w = world();
    let (child, _) = mention(&w, "@tola-grey keep this");
    sessions::set_archived(&w.alpha, &child, true).expect("archived");
    let view = w.view();
    let shown = &view.children[0];
    assert_eq!(shown.session_id, child);
    assert!(shown.archived);
    assert!(shown.prompt.is_none() && shown.items.is_empty() && shown.refusal.is_none());
}

/// R10924K-02: only threads started within the months the parent shows.
#[test]
fn a_thread_older_than_the_parents_months_is_not_drawn() {
    let w = world();
    let old = sessions::now_ms() - 120 * 86_400_000;
    w.alpha
        .replica
        .mutate(vec![upsert(
            "sessions",
            json!({"id": "dev1.40", "title": "old", "status": "open", "agent_actor": TOLA,
                   "created_by_actor": ANA, "created_by_client": "dev1",
                   "preferred_client": null, "parent_session": PARENT, "invite": "[]",
                   "archived": false, "heard_until": null, "created_at_ms": old,
                   "last_activity_ms": old, "author_actor": ANA, "author_client": "dev1"}),
        )])
        .expect("an old thread");
    let (recent, _) = mention(&w, "@tola-grey now");
    let view = w.view();
    let shown: Vec<&str> = view
        .children
        .iter()
        .map(|child| child.session_id.as_str())
        .collect();
    assert_eq!(shown, vec![recent.as_str()]);
}

/// Counts the rows every read of the replica returns: what a view costs,
/// independent of the host's speed.
struct Counting {
    inner: Arc<dyn crate::hq::port::Replica>,
    rows: std::sync::atomic::AtomicUsize,
}

impl crate::hq::port::Replica for Counting {
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, crate::hq::port::ReplicaError> {
        self.inner.mutate(ops)
    }
    fn query(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<Vec<Map<String, Value>>, crate::hq::port::ReplicaError> {
        let rows = self.inner.query(sql, params)?;
        self.rows
            .fetch_add(rows.len(), std::sync::atomic::Ordering::SeqCst);
        Ok(rows)
    }
    fn query_covered(
        &self,
        sql: &str,
        params: &[Value],
        base: &crate::hq::port::WindowBase,
        units: &[String],
    ) -> Result<crate::hq::port::Covered, crate::hq::port::ReplicaError> {
        let covered = self.inner.query_covered(sql, params, base, units)?;
        self.rows
            .fetch_add(covered.0.len(), std::sync::atomic::Ordering::SeqCst);
        Ok(covered)
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
        self.inner.delivery(commit)
    }
    fn row_writes(
        &self,
        table: &str,
        row_id: &str,
        field: Option<&str>,
    ) -> Result<crate::hq::port::RowWrites, crate::hq::port::ReplicaError> {
        self.inner.row_writes(table, row_id, field)
    }
    fn refused_commits(
        &self,
        table: &str,
    ) -> Result<Vec<crate::hq::port::RefusedCommit>, crate::hq::port::ReplicaError> {
        self.inner.refused_commits(table)
    }
    fn granted(
        &self,
        table: &str,
        variable: &str,
    ) -> Result<Vec<String>, crate::hq::port::ReplicaError> {
        self.inner.granted(table, variable)
    }
    fn revoked_units(&self, table: &str) -> Result<Vec<String>, crate::hq::port::ReplicaError> {
        self.inner.revoked_units(table)
    }
    fn upload_refusals(&self) -> Vec<crate::hq::port::UploadRefusal> {
        self.inner.upload_refusals()
    }
    fn blob_cache_bytes(&self) -> Result<u64, crate::hq::port::ReplicaError> {
        self.inner.blob_cache_bytes()
    }
    fn mutate_superseding(
        &self,
        ops: Vec<RowOp>,
        earlier: Vec<String>,
    ) -> Result<String, crate::hq::port::ReplicaError> {
        self.inner.mutate_superseding(ops, earlier)
    }
    fn remote_query(
        &self,
        id: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, crate::hq::port::ReplicaError> {
        self.inner.remote_query(id, params)
    }
    fn remote_command(
        &self,
        id: &str,
        request: &str,
        params: Value,
    ) -> Result<(), crate::hq::port::ReplicaError> {
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

/// `turns` earlier turns of `child`, each with a 4 KB tool call, thinking,
/// a streamed chunk and an answer — history the parent never draws.
fn history(w: &World, child: &str, from: usize, turns: usize, at: i64) {
    let page = sessions::page_key(child, &sessions::month(at));
    let result = "x".repeat(4_000);
    let mut ops = Vec::new();
    for n in from..from + turns {
        let turn = format!("old-{n}");
        let at = at + i64::try_from(n).expect("small");
        ops.push(upsert(
            "tool_calls",
            json!({"id": format!("call-{n}"), "session_id": child, "page_key": page,
                   "turn_id": turn, "name": "web_search", "args": "{}", "status": "done",
                   "result": result, "result_blob": null, "started_at_ms": at,
                   "ended_at_ms": at, "created_at_ms": at, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        ));
        ops.push(upsert(
            "reasoning",
            json!({"id": format!("r-{n}"), "session_id": child, "page_key": page,
                   "turn_id": turn, "seq": 1, "compacted": true, "text": "thinking",
                   "created_at_ms": at, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        ));
        ops.push(upsert(
            "turn_chunks",
            json!({"id": format!("k-{n}"), "session_id": child, "turn_id": turn, "seq": 0,
                   "text": "partial", "created_at_ms": at, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        ));
        ops.push(upsert(
            "messages",
            json!({"id": format!("a-old-{n}"), "session_id": child, "page_key": page,
                   "turn_id": turn, "role": "agent", "body": "an old answer",
                   "body_blob": null, "metadata": json!({"finish": "stop"}).to_string(),
                   "created_at_ms": at, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        ));
    }
    w.alpha.replica.mutate(ops).expect("history");
}

/// R10924K-05: what the parent reads of a thread does not grow with the
/// thread's history — only its first line and its newest turn are read.
#[test]
fn a_threads_history_is_not_read_by_its_parent() {
    let w = world();
    let (child, _) = mention(&w, "@tola-grey keep researching");
    let at = w.at + 1_000;
    history(&w, &child, 0, 50, at);
    let newest = at + 10_000;
    let page = sessions::page_key(&child, &sessions::month(newest));
    w.alpha
        .replica
        .mutate(vec![upsert(
            "messages",
            json!({"id": "a-newest", "session_id": child, "page_key": page,
                   "turn_id": "newest", "role": "agent", "body": "the newest answer",
                   "body_blob": null, "metadata": json!({"finish": "stop"}).to_string(),
                   "created_at_ms": newest, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        )])
        .expect("the newest turn");
    let counting = Arc::new(Counting {
        inner: Arc::clone(&w.alpha.replica),
        rows: std::sync::atomic::AtomicUsize::new(0),
    });
    let source = Source {
        replica: Arc::clone(&counting) as Arc<dyn crate::hq::port::Replica>,
        ..w.alpha.clone()
    };
    let read = || {
        counting.rows.store(0, std::sync::atomic::Ordering::SeqCst);
        let view = timeline::view(
            &source,
            PARENT,
            &Opened::default(),
            &HashMap::new(),
            sessions::now_ms(),
        )
        .expect("the parent's view")
        .0;
        (
            counting.rows.load(std::sync::atomic::Ordering::SeqCst),
            view,
        )
    };
    let (before, first) = read();
    assert_eq!(
        first.children[0]
            .messages
            .iter()
            .map(|m| m.body.as_str())
            .collect::<Vec<_>>(),
        vec!["the newest answer"]
    );
    // Ten times the history, all older than the newest turn.
    history(&w, &child, 50, 450, at);
    let (after, again) = read();
    assert_eq!(again.children, first.children, "the same thread drawn");
    assert_eq!(
        after, before,
        "rows read for the parent: {before} before, {after} after 450 more old turns"
    );
}

/// R10924K-07: the person's own line is cut as the answer is.
#[test]
fn a_long_line_is_cut_in_the_parent() {
    let w = world();
    let line = format!("@tola-grey {}", "pasted text ".repeat(2_000));
    mention(&w, &line);
    let view = w.view();
    let shown = &view.children[0];
    let body = &shown.prompt.as_ref().expect("the line").body;
    assert!(body.chars().count() <= super::MAX_INLINE_ANSWER + 1 && body.ends_with('…'));
    assert!(shown.truncated, "the rest is in the thread");
}

/// R10924K-09: a refusal whose thread the replica holds again — the same
/// line — is that thread, not refused.
#[test]
fn a_refusal_whose_thread_is_held_again_is_not_shown_refused() {
    let w = world();
    let (child, commit) = mention(&w, "@tola-grey once more");
    let ops = w
        .outbox()
        .into_iter()
        .find(|(id, _)| id == &commit)
        .expect("the commit")
        .1;
    let w = refuse_on_the_server(w, &commit, "keeper.conflict");
    let refused = w.view().children[0].clone();
    assert!(refused.refusal.is_some());
    // The server holds the same rows after all (a conflict resolved its
    // way): they arrive again.
    w.alpha
        .replica
        .mutate(
            ops.iter()
                .map(|op| upsert(op["table"].as_str().expect("a table"), op["values"].clone()))
                .collect(),
        )
        .expect("held again");
    let view = w.view();
    assert_eq!(view.children.len(), 1, "one thread: {:?}", view.children);
    let shown = &view.children[0];
    assert_eq!(
        (shown.thread_id.as_str(), shown.session_id.as_str()),
        (refused.thread_id.as_str(), child.as_str())
    );
    assert_eq!(shown.refusal, None);
}

/// R10924K-11: at the bound, a refusal whose thread came back is counted
/// once — as the held thread — and nothing is said to be left out.
#[test]
fn a_refusal_held_again_is_counted_once_at_the_bound() {
    let w = world();
    let (_, commit) = mention(&w, "@tola-grey the oldest");
    let ops = w
        .outbox()
        .into_iter()
        .find(|(id, _)| id == &commit)
        .expect("the commit")
        .1;
    let w = refuse_on_the_server(w, &commit, "keeper.conflict");
    w.alpha
        .replica
        .mutate(
            ops.iter()
                .map(|op| upsert(op["table"].as_str().expect("a table"), op["values"].clone()))
                .collect(),
        )
        .expect("held again");
    for n in 0..super::MAX_INLINE_CHILDREN - 1 {
        mention(&w, &format!("@tola-grey newer {n}"));
    }
    let view = w.view();
    assert_eq!(view.children.len(), super::MAX_INLINE_CHILDREN);
    assert_eq!(view.children_note, None, "no thread is left out");
    assert!(
        view.children.iter().all(|child| child.refusal.is_none()),
        "none drawn refused"
    );
    assert_eq!(
        view.children[0]
            .prompt
            .as_ref()
            .map(|prompt| prompt.body.as_str()),
        Some("@tola-grey the oldest")
    );
}

/// R10924K-13: 30 threads whose newest turn ran 1,000 tool calls each — half
/// of them failed — keep the parent's view under the same bound: the inline
/// fold is its summary line, never its steps or their failures.
#[test]
fn threads_with_long_turns_keep_the_parents_view_bounded() {
    let w = world();
    let answer = "The notes say ship it. ".repeat(900);
    let now = sessions::now_ms();
    for n in 0..super::MAX_INLINE_CHILDREN {
        let (child, _) = mention(&w, &format!("@tola-grey long turn {n}"));
        let first = w
            .alpha
            .replica
            .query(
                "SELECT id FROM messages WHERE session_id = ?1",
                &[Value::from(child.as_str())],
            )
            .expect("reads")[0]["id"]
            .as_str()
            .expect("an id")
            .to_owned();
        let page = sessions::page_key(&child, &sessions::month(now));
        let mut ops: Vec<RowOp> = (0..1_000)
            .map(|k| {
                upsert(
                    "tool_calls",
                    json!({"id": format!("call-{n}-{k}"), "session_id": child,
                           "page_key": page, "turn_id": first, "name": "web_search",
                           "args": "{}", "status": if k % 2 == 0 { "done" } else { "failed" },
                           "result": "no result", "result_blob": null, "started_at_ms": now,
                           "ended_at_ms": now, "created_at_ms": now, "author_actor": TOLA,
                           "author_client": "tola-grey.box"}),
                )
            })
            .collect();
        ops.push(upsert(
            "messages",
            json!({"id": format!("a-{n}"), "session_id": child, "page_key": page,
                   "turn_id": first, "role": "agent", "body": answer, "body_blob": null,
                   "metadata": json!({"finish": "stop"}).to_string(),
                   "created_at_ms": now + 1, "author_actor": TOLA,
                   "author_client": "tola-grey.box"}),
        ));
        w.alpha.replica.mutate(ops).expect("a long turn");
    }
    let view = w.view();
    let bytes = serde_json::to_string(&view).expect("json").len();
    eprintln!(
        "MEASURE long-turns children={} vm_bytes={bytes}",
        view.children.len()
    );
    assert_eq!(view.children.len(), super::MAX_INLINE_CHILDREN);
    for child in &view.children {
        assert_eq!(child.folds.len(), 1, "the turn's one fold");
        assert!(
            child.folds[0].steps.is_empty() && child.folds[0].failures.is_empty(),
            "the summary line only"
        );
        assert!(!child.folds[0].summary.is_empty());
    }
    assert!(bytes < 120_000, "the view is bounded: {bytes} bytes");
}

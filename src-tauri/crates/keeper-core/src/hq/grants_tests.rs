//! Always allow (109.11) over real on-disk replicas with no server: which
//! cards offer a grant, what a grant writes (the person-agent key the
//! server computes, the scope derived from the request's own call), a
//! message declining what waits for its sender, and the grants list — a
//! refused write and a revocation read back from the replica's outcome
//! record, settled at the file as a round would settle them.

use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::hq::approvals::{AgentApprovalActionVm as Action, AgentApprovalStateVm, Decisions};
use crate::hq::port::ReplicaSpec;
use crate::hq::sessions::{self, object, Source};
use crate::hq::syncular::SyncularReplica;

const NIXI: &str = "nixi";

struct World {
    dir: tempfile::TempDir,
    alpha: Source,
    decisions: Decisions,
}

/// One replica of `alpha` whose person is `person`, with Nixi, Ben and
/// them in its directory.
fn world(person: &str) -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let w = open_world(dir, person);
    w.alpha
        .replica
        .mutate(vec![
            actor(NIXI, "agent", "Nixi"),
            actor("tola", "agent", "Tola"),
            actor(person, "person", "Zoë"),
            actor("ben", "person", "Ben"),
        ])
        .expect("directory");
    serve(&w, NIXI);
    w
}

/// `dir`'s replica of `alpha` opened for `person`.
fn open_world(dir: tempfile::TempDir, person: &str) -> World {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "alpha".to_owned(),
        path: dir.path().join("alpha.sqlite"),
        client_id: Some("dev1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    let alpha = Source {
        database: "alpha".to_owned(),
        replica: Arc::new(replica),
        actor: person.to_owned(),
        client: "dev1".to_owned(),
    };
    let decisions = Decisions::load(dir.path().join("decisions.json"));
    World {
        dir,
        alpha,
        decisions,
    }
}

/// The server granted the grants window `agent`'s person-agent key for this
/// person — the agent serves them in this database — as a pull records it in
/// the replica file.
fn serve(w: &World, agent: &str) {
    let key = crate::hq::vocabulary::person_agent_key(&w.alpha.actor, agent);
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("timeout");
    let state = json!({"requested": {"person_agent": [key]}, "params": null, "cursor": 3,
                       "bootstrapState": null, "status": "active", "reasonCode": null,
                       "effectiveScopes": {"person_agent": [key]}, "syncedOnce": true});
    conn.execute(
        "INSERT INTO _syncular_subscriptions (id, tbl, state_json) VALUES (?1, ?2, ?3)",
        rusqlite::params![format!("grants-{agent}"), TABLE, state.to_string()],
    )
    .expect("a granted unit");
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

fn actor(id: &str, kind: &str, name: &str) -> RowOp {
    upsert(
        "actors",
        json!({"id": id, "kind": kind, "display_name": name, "family": null, "dir": "alpha",
               "created_at_ms": 1, "author_actor": "server", "author_client": "server"}),
    )
}

fn now() -> i64 {
    sessions::now_ms()
}

/// A session of Nixi's that `w`'s person owns and reads the trace of.
fn session(w: &World, id: &str, trust: Option<&str>, archived: bool) {
    session_with(w, id, NIXI, "owner", trust, archived);
}

/// A session of `agent`'s in which `w`'s person is a `role` member reading
/// the trace.
fn session_with(w: &World, id: &str, agent: &str, role: &str, trust: Option<&str>, archived: bool) {
    let at = now() - 60_000;
    let me = w.alpha.actor.as_str();
    w.alpha
        .replica
        .mutate(vec![
            upsert(
                "sessions",
                json!({"id": id, "title": format!("about {id}"), "status": "open",
                       "agent_actor": agent, "created_by_actor": me, "created_by_client": "dev1",
                       "preferred_client": null, "parent_session": null, "invite": "[]",
                       "archived": archived, "heard_until": null, "trust": trust,
                       "created_at_ms": at, "last_activity_ms": at,
                       "author_actor": me, "author_client": "dev1"}),
            ),
            upsert(
                "session_members",
                json!({"id": format!("{id}/{me}"), "session_id": id, "actor_id": me,
                       "role": role, "reads_trace": true, "added_by_actor": me,
                       "added_at_ms": at, "read_until": null,
                       "author_actor": me, "author_client": "dev1"}),
            ),
        ])
        .expect("session");
}

/// A pending request of `session` for `tool` at `tier`, its call's `args`,
/// decided by `approvers`.
fn request(
    w: &World,
    id: &str,
    session: &str,
    tool: &str,
    tier: &str,
    args: Value,
    approvers: &[&str],
) {
    let at = now() - 30_000;
    let page = sessions::page_key(session, &sessions::month(at));
    w.alpha
        .replica
        .mutate(vec![
            upsert(
                "tool_calls",
                json!({"id": format!("call-{id}"), "session_id": session, "page_key": page,
                       "turn_id": "t", "name": tool, "args": args.to_string(),
                       "status": "parked", "result": null, "result_blob": null,
                       "started_at_ms": at, "ended_at_ms": null, "created_at_ms": at,
                       "author_actor": NIXI, "author_client": "nixi.box"}),
            ),
            upsert(
                "approvals",
                json!({"id": id, "session_id": session, "page_key": page,
                       "tool_call_id": format!("call-{id}"), "tier": tier,
                       "binding_digest": format!("digest-{id}"), "summary": "a request",
                       "approvers": serde_json::to_string(approvers).expect("json"),
                       "state": "pending", "decided_by_actor": null, "decided_by_client": null,
                       "consumed_by_client": null, "grant_id": null,
                       "expires_at_ms": at + 3_600_000,
                       "created_at_ms": at, "author_actor": NIXI, "author_client": "nixi.box"}),
            ),
        ])
        .expect("request");
}

/// This device (`dev1`) on `mine`, and Nixi's instance `nixi.box` holding
/// `session`'s claim on `holder`, as the directory records their hosts
/// (`None`: no host recorded).
fn hosts(w: &World, session: &str, mine: Option<&str>, holder: Option<&str>) {
    let me = w.alpha.actor.as_str();
    let client = |id: &str, actor: &str, kind: &str, host: Option<&str>| {
        upsert(
            "clients",
            json!({"id": id, "actor_id": actor, "kind": kind, "name": id, "host": host,
                   "created_at_ms": 1, "revoked_at_ms": null, "dir": "alpha",
                   "author_actor": "server", "author_client": "server"}),
        )
    };
    w.alpha
        .replica
        .mutate(vec![
            client("dev1", me, "device", mine),
            client("nixi.box", NIXI, "instance", holder),
            upsert(
                "claims",
                json!({"session_id": session, "holder_actor": NIXI,
                       "holder_client": "nixi.box", "epoch": 1, "renewed_at_ms": now(),
                       "released": false, "author_actor": NIXI, "author_client": "nixi.box"}),
            ),
        ])
        .expect("hosts");
}

fn git_status() -> Value {
    json!({"argv": ["git", "status", "--porcelain"]})
}

fn card(w: &World, session: &str, id: &str) -> crate::hq::approvals::AgentApprovalVm {
    crate::hq::approvals::of_session(
        &w.alpha,
        session,
        &sessions::directory(&w.alpha),
        &w.decisions.refusals(std::slice::from_ref(&w.alpha)),
        now(),
    )
    .into_iter()
    .find(|a| a.id == id)
    .expect("the card")
}

fn grant_rows(w: &World) -> Vec<serde_json::Map<String, Value>> {
    w.alpha
        .replica
        .query(
            "SELECT * FROM standing_grants ORDER BY created_at_ms, id",
            &[],
        )
        .expect("grants")
}

fn ask(session: &str, id: &str, scope: &str, session_only: bool) -> AlwaysAllow {
    AlwaysAllow {
        session_id: session.to_owned(),
        approval_id: id.to_owned(),
        binding_digest: format!("digest-{id}"),
        scope: scope.to_owned(),
        session_only,
        expires_in_days: None,
    }
}

#[test]
fn a_trusted_t2_run_offers_always_allow_narrowest_scope_first() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let card = card(&w, "dev1.1", "ap1");
    assert_eq!(
        card.actions,
        vec![
            Action::AllowOnce,
            Action::AlwaysAllow,
            Action::SessionOnly,
            Action::Deny
        ]
    );
    let offer = card.grant.expect("an offer");
    assert_eq!(
        offer
            .scopes
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["programArg", "program"]
    );
    // The breadth is said: whatever follows the scope is covered too.
    assert_eq!(
        offer.scopes[0].standing,
        "Nixi may run “git status …” in every session you share with it in alpha, without \
         asking — whatever follows “status”."
    );
    assert!(offer.scopes[1]
        .standing
        .contains("whatever arguments follow"));
    assert!(offer.scopes[0].session.contains("in this session"));
    assert_eq!(offer.revoke_note, REVOKE_NOTE);
    assert_eq!(offer.tier, "T2");
}

/// Q109-09 b, Q109-10 a, Q109-22 a: *Always allow …* is left out — not
/// disabled — at T4, in an untrusted session, for `delegate`, for a drive
/// tool, for any tool off the allow-list and in an archived session;
/// *Allow once* and *Deny* stay.
#[test]
fn always_allow_is_left_out_where_no_grant_could_apply() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    session(&w, "dev1.2", Some("untrusted"), false);
    // On another host than the agent's, so the T4 request is decidable here.
    hosts(&w, "dev1.1", Some("laptop"), Some("electra"));
    let paseo = json!({"title": "x"});
    request(
        &w,
        "t4",
        "dev1.1",
        "mcp__paseo__create_agent",
        "T4",
        paseo.clone(),
        &["zoë"],
    );
    request(&w, "dirty", "dev1.2", "run", "T3", git_status(), &["zoë"]);
    request(
        &w,
        "deleg",
        "dev1.1",
        "delegate",
        "T3",
        json!({"to": "tola"}),
        &["zoë"],
    );
    request(
        &w,
        "drive",
        "dev1.1",
        "drive_write",
        "T2",
        json!({"path": "a"}),
        &["zoë"],
    );
    request(
        &w,
        "fetch",
        "dev1.1",
        "web_fetch",
        "T2",
        json!({"url": "x"}),
        &["zoë"],
    );
    request(&w, "spelt", "dev1.1", "Run", "T2", git_status(), &["zoë"]);
    for (session, id) in [
        ("dev1.1", "t4"),
        ("dev1.2", "dirty"),
        ("dev1.1", "deleg"),
        ("dev1.1", "drive"),
        ("dev1.1", "fetch"),
        ("dev1.1", "spelt"),
    ] {
        let card = card(&w, session, id);
        assert_eq!(card.actions, vec![Action::AllowOnce, Action::Deny], "{id}");
        assert_eq!(card.grant, None, "{id}");
        // Asked for anyway, keeper refuses before writing anything.
        assert!(
            always_allow(
                &w.alpha,
                &w.decisions,
                &ask(session, id, "any", false),
                now()
            )
            .is_err(),
            "{id}"
        );
    }
    assert!(grant_rows(&w).is_empty());
    // The Paseo verb at T3 in a trusted session is offered, any arguments.
    request(
        &w,
        "start",
        "dev1.1",
        "mcp__paseo__create_agent",
        "T3",
        paseo,
        &["zoë"],
    );
    let offer = card(&w, "dev1.1", "start").grant.expect("offered");
    assert_eq!(offer.scopes.len(), 1);
    assert!(offer.scopes[0].standing.contains("whatever its arguments"));
}

/// The grant is filed under the server's `personAgentKey` — the person's
/// UTF-8 byte length, not their character count — in the decision's own
/// commit, its scope derived from the request's call.
#[test]
fn always_allow_writes_the_grant_under_the_servers_key_with_the_approval() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let outbox_before = outbox(&w);
    let outcome = always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap1", "programArg", false),
        now(),
    )
    .expect("written");
    assert_eq!(
        outcome.state,
        crate::hq::approvals::AgentDecisionStateVm::Waiting
    );
    assert_eq!(
        outbox(&w),
        outbox_before + 1,
        "grant and decision are one commit"
    );
    let rows = grant_rows(&w);
    assert_eq!(rows.len(), 1);
    let grant = &rows[0];
    // The contract's own example (`vocabulary.json`): "zoë" is 4 bytes.
    assert_eq!(grant["person_agent_key"], json!("4:zoë/nixi"));
    assert_eq!(grant["person_actor"], json!("zoë"));
    assert_eq!(grant["agent_actor"], json!(NIXI));
    assert_eq!(grant["session_id"], Value::Null);
    assert_eq!(grant["tool"], json!("run"));
    assert_eq!(
        sessions::json(grant, "args_match"),
        json!({"program": "git", "arg": "status"})
    );
    assert_eq!(grant["max_tier"], json!("T2"));
    assert_eq!(grant["granted_by_client"], json!("dev1"));
    assert_eq!(grant["revoked_at_ms"], Value::Null);
    assert_eq!(
        card(&w, "dev1.1", "ap1").state,
        AgentApprovalStateVm::Approved
    );
    // The window holds the same key the row is filed under.
    assert!(units(&w.alpha).contains(&"4:zoë/nixi".to_owned()));

    // This session only binds the session; the program scope drops the arg.
    request(&w, "ap2", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let mut bound = ask("dev1.1", "ap2", "program", true);
    bound.expires_in_days = Some(7);
    always_allow(&w.alpha, &w.decisions, &bound, now()).expect("written");
    let rows = grant_rows(&w);
    let second = rows
        .iter()
        .find(|r| r["session_id"] == json!("dev1.1"))
        .expect("bound");
    assert_eq!(
        sessions::json(second, "args_match"),
        json!({"program": "git"})
    );
    assert!(second["expires_at_ms"]
        .as_i64()
        .is_some_and(|at| at > now()));
    // A scope the request does not offer, or an expiry not offered, writes nothing.
    request(&w, "ap3", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    assert!(always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap3", "any", false),
        now()
    )
    .is_err());
    let mut odd = ask("dev1.1", "ap3", "program", false);
    odd.expires_in_days = Some(2);
    assert!(always_allow(&w.alpha, &w.decisions, &odd, now()).is_err());
    assert_eq!(grant_rows(&w).len(), 2);
}

fn outbox(w: &World) -> i64 {
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    conn.query_row("SELECT count(*) FROM _syncular_outbox", [], |row| {
        row.get(0)
    })
    .expect("counts")
}

/// Q109-12 a: a message declines every request of its session waiting for
/// its sender — each its own denial, before the message — and nothing
/// waiting for someone else; the banner holds exactly what it would decline.
#[test]
fn sending_a_message_declines_what_waits_for_the_sender() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    // This device is on another host than the agent's: its T4 decisions go.
    hosts(&w, "dev1.1", Some("laptop"), Some("electra"));
    request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    request(
        &w,
        "also",
        "dev1.1",
        "mcp__paseo__create_agent",
        "T4",
        json!({}),
        &["zoë", "ben"],
    );
    request(&w, "bens", "dev1.1", "run", "T2", git_status(), &["ben"]);
    let opened = crate::hq::timeline::Opened::default();
    let view = |w: &World| {
        crate::hq::timeline::view(
            &w.alpha,
            "dev1.1",
            &opened,
            &w.decisions.refusals(std::slice::from_ref(&w.alpha)),
            now(),
        )
        .expect("view")
        .0
    };
    let before = view(&w);
    let banner = before.banner.expect("a banner");
    assert_eq!(
        banner
            .approvals
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["mine", "also"]
    );
    assert!(banner
        .sentence
        .contains("declines these 2 requests, even a message that says yes"));
    assert_eq!(
        before
            .approvals
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["bens"],
        "out of the reading column"
    );

    let shown = shown(&w, "dev1.1");
    crate::hq::send::send(&w.alpha, &w.decisions, "dev1.1", "yes, do it", &shown).expect("sent");
    for id in ["mine", "also"] {
        let declined = card(&w, "dev1.1", id);
        assert_eq!(declined.state, AgentApprovalStateVm::Denied, "{id}");
        assert_eq!(declined.decided_by.as_deref(), Some("Zoë"));
    }
    assert_eq!(
        card(&w, "dev1.1", "bens").state,
        AgentApprovalStateVm::Pending
    );
    let after = view(&w);
    assert_eq!(after.banner, None);
    assert!(after.messages.iter().any(|m| m.body == "yes, do it"));
    // Each denial is its own commit, and the message its own after them.
    assert_eq!(outbox_ops(&w), vec!["approvals", "approvals", "messages"]);
}

/// DW-E109-27, R109T-02, R109T-04: V8 refuses a T4 decision — a denial
/// too — from the host the agent runs on, or when a host is unknown. Such a
/// card offers no decision and says why; a message leaves it waiting, out of
/// the banner; and the banner says how many wait, and for what.
#[test]
fn a_t4_request_this_device_cannot_decide_offers_no_decision_and_is_not_declined() {
    let another = "Needs a decision from another device.";
    let unknown = "Can't be decided until the agent's host is known.";
    let another_line =
        "1 request needs a decision from another device; a message here leaves it waiting.";
    let unknown_line = "1 request can't be decided until the agent's host is known; a message \
                        here leaves it waiting.";
    for (mine, holder, card_says, banner_says) in [
        (Some("electra"), Some("electra"), another, another_line),
        (None, Some("electra"), another, another_line),
        (Some("laptop"), None, unknown, unknown_line),
        (None, None, unknown, unknown_line),
    ] {
        let case = format!("{mine:?}/{holder:?}");
        let w = world("zoë");
        session(&w, "dev1.1", None, false);
        hosts(&w, "dev1.1", mine, holder);
        request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
        request(&w, "t4", "dev1.1", "delegate", "T4", json!({}), &["zoë"]);
        let view = || {
            crate::hq::timeline::view(
                &w.alpha,
                "dev1.1",
                &crate::hq::timeline::Opened::default(),
                &w.decisions.refusals(std::slice::from_ref(&w.alpha)),
                now(),
            )
            .expect("view")
            .0
        };
        let before = view();
        let banner = before.banner.expect("a banner");
        assert_eq!(
            banner
                .approvals
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["mine"],
            "{case}"
        );
        assert_eq!(
            banner.sentence,
            format!(
                "Sending a message declines this request, even a message that says yes. Use \
                 Allow to let it run. {banner_says}"
            ),
            "{case}"
        );
        let [t4] = before.approvals.as_slice() else {
            panic!("{case}: the T4 request alone in the reading column");
        };
        assert_eq!(t4.id, "t4");
        assert_eq!(t4.sentence, card_says, "{case}");
        assert!(!t4.can_decide, "{case}");
        assert!(t4.actions.is_empty(), "{case}: no Allow once, no Deny");
        assert!(
            !crate::hq::approvals::waiting(
                std::slice::from_ref(&w.alpha),
                &Default::default(),
                now()
            )
            .iter()
            .any(|a| a.id == "t4"),
            "{case}: not in the decisions panel"
        );

        // Even shown, a message declines only what this device may decide.
        let mut shown = shown(&w, "dev1.1");
        shown.push(crate::hq::approvals::ShownApprovalVm {
            id: "t4".to_owned(),
            binding_digest: "digest-t4".to_owned(),
        });
        crate::hq::send::send(&w.alpha, &w.decisions, "dev1.1", "go on", &shown).expect("sent");
        assert_eq!(
            card(&w, "dev1.1", "mine").state,
            AgentApprovalStateVm::Denied
        );
        assert_eq!(
            card(&w, "dev1.1", "t4").state,
            AgentApprovalStateVm::Pending
        );
        assert_eq!(outbox_ops(&w), vec!["approvals", "messages"]);
        let after = view().banner.expect("still said");
        assert!(after.approvals.is_empty());
        assert_eq!(after.sentence, banner_says, "{case}");
    }

    // On another host than the agent's, the same request is this device's
    // to decide.
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    hosts(&w, "dev1.1", Some("laptop"), Some("electra"));
    request(&w, "t4", "dev1.1", "delegate", "T4", json!({}), &["zoë"]);
    let t4 = card(&w, "dev1.1", "t4");
    assert_eq!(t4.sentence, "Waiting for you.");
    assert!(t4.can_decide && t4.declined_by_send);
    assert_eq!(t4.actions, [Action::AllowOnce, Action::Deny]);
}

/// What the session's banner shows now, as the webview sends it back with a
/// message.
fn shown(w: &World, session: &str) -> Vec<crate::hq::approvals::ShownApprovalVm> {
    let view = crate::hq::timeline::view(
        &w.alpha,
        session,
        &crate::hq::timeline::Opened::default(),
        &w.decisions.refusals(std::slice::from_ref(&w.alpha)),
        now(),
    )
    .expect("view")
    .0;
    view.banner
        .map(|banner| banner.approvals)
        .unwrap_or_default()
        .into_iter()
        .map(|approval| crate::hq::approvals::ShownApprovalVm {
            id: approval.id,
            binding_digest: approval.binding_digest,
        })
        .collect()
}

/// R10911-01: a message declines only what its banner showed. A request
/// that arrived after the banner was drawn, or one shown at another digest,
/// still waits after the send.
#[test]
fn a_request_that_arrived_after_the_banner_was_drawn_stays_pending() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "seen", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    request(&w, "moved", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let mut banner = shown(&w, "dev1.1");
    assert_eq!(banner.len(), 2);
    // The banner drew `moved` at a digest it no longer has.
    if let Some(moved) = banner.iter_mut().find(|shown| shown.id == "moved") {
        moved.binding_digest = "digest-before".to_owned();
    }
    request(&w, "late", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    crate::hq::send::send(&w.alpha, &w.decisions, "dev1.1", "yes", &banner).expect("sent");
    assert_eq!(
        card(&w, "dev1.1", "seen").state,
        AgentApprovalStateVm::Denied
    );
    for id in ["moved", "late"] {
        assert_eq!(
            card(&w, "dev1.1", id).state,
            AgentApprovalStateVm::Pending,
            "{id}"
        );
    }
    // A message sent with no banner shown declines nothing.
    crate::hq::send::send(&w.alpha, &w.decisions, "dev1.1", "and again", &[]).expect("sent");
    assert_eq!(
        card(&w, "dev1.1", "late").state,
        AgentApprovalStateVm::Pending
    );
}

/// R10911-06: a denial that cannot be written on this device stops the
/// message, and the person is told which request still waits.
#[test]
fn a_denial_that_cannot_be_written_stops_the_message() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let banner = shown(&w, "dev1.1");
    let (staged, source) = crate::hq::window_tests::Staged::over(&w.alpha, None);
    *staged.failing.lock().expect("failing") = Some("approvals".to_owned());
    let refused = crate::hq::send::send(&source, &w.decisions, "dev1.1", "yes", &banner)
        .expect_err("not sent");
    assert_eq!(
        refused.to_string(),
        "keeper could not decline “a request” on this device, so your message was not sent; \
         the request still waits for you (database or disk is full)."
    );
    assert_eq!(
        card(&w, "dev1.1", "mine").state,
        AgentApprovalStateVm::Pending
    );
    let messages = w
        .alpha
        .replica
        .query("SELECT id FROM messages WHERE body = 'yes'", &[])
        .expect("messages");
    assert!(messages.is_empty(), "no message was written");
}

/// R10911-07: `/stop` declines what the banner showed, before the stop; any
/// other command declines nothing.
#[test]
fn stop_declines_what_the_banner_showed_and_other_commands_do_not() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let banner = shown(&w, "dev1.1");
    request(&w, "late", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    crate::hq::commands::run(
        &w.alpha,
        &[],
        &w.decisions,
        "dev1.1",
        "/rename Plans",
        None,
        &banner,
    )
    .expect("renamed");
    assert_eq!(
        card(&w, "dev1.1", "mine").state,
        AgentApprovalStateVm::Pending
    );
    let at = now();
    w.alpha
        .replica
        .mutate(vec![upsert(
            "turn_chunks",
            json!({"id": "chunk-t", "session_id": "dev1.1", "turn_id": "t", "seq": 0,
                   "text": "", "created_at_ms": at, "author_actor": NIXI,
                   "author_client": "nixi.box"}),
        )])
        .expect("a running turn");
    crate::hq::commands::run(
        &w.alpha,
        &[],
        &w.decisions,
        "dev1.1",
        "/stop",
        None,
        &banner,
    )
    .expect("stop");
    assert_eq!(
        card(&w, "dev1.1", "mine").state,
        AgentApprovalStateVm::Denied
    );
    assert_eq!(
        card(&w, "dev1.1", "late").state,
        AgentApprovalStateVm::Pending
    );
}

/// R10911-02: a person the session's agent does not serve here — invited
/// into someone else's session — is offered no grant: the server refuses
/// one about any agent that does not serve them (`hq.v15.agent`).
#[test]
fn a_member_the_agent_does_not_serve_is_offered_no_grant() {
    let w = world("zoë");
    session_with(&w, "ana.1", "tola", "person", None, false);
    request(
        &w,
        "ap1",
        "ana.1",
        "run",
        "T2",
        git_status(),
        &["zoë", "ana"],
    );
    let card = card(&w, "ana.1", "ap1");
    assert_eq!(card.actions, vec![Action::AllowOnce, Action::Deny]);
    assert_eq!(card.grant, None);
    assert!(always_allow(
        &w.alpha,
        &w.decisions,
        &ask("ana.1", "ap1", "program", false),
        now()
    )
    .is_err());
    assert!(grant_rows(&w).is_empty());
    // Once the server grants Tola's key, the same card offers one.
    serve(&w, "tola");
    assert!(crate::hq::approvals::of_session(
        &w.alpha,
        "ana.1",
        &sessions::directory(&w.alpha),
        &std::collections::HashMap::new(),
        now(),
    )[0]
    .grant
    .is_some());
}

/// R10911-11: which scopes say "any code", for both the program-and-first-
/// argument scope and the program-alone scope a call offers. A row's
/// `narrow` is the first scope's answer, `wide` the program-alone scope's.
#[test]
fn any_code_is_said_for_every_form_that_runs_given_code() {
    let cases: &[(&[&str], bool, bool)] = &[
        // Shells: an eval flag alone or in a cluster, any flag first, or bare.
        (&["bash", "-c", "ls"], true, true),
        (&["bash", "-lc", "ls"], true, true),
        (&["zsh", "-lc", "ls"], true, true),
        (&["sh", "-ec", "ls"], true, true),
        (&["/bin/bash", "-euxc", "ls"], true, true),
        (&["bash", "-l"], true, true),
        (&["bash", "script.sh"], false, true),
        (&["fish", "--command", "ls"], true, true),
        // Interpreters, versioned or not.
        (&["python3", "-c", "print(1)"], true, true),
        (&["/usr/bin/python3.12", "-c", "x"], true, true),
        (&["python3", "-u", "-c", "x"], true, true),
        (&["python3", "tool.py"], false, true),
        (&["node", "-e", "1"], true, true),
        (&["deno", "eval", "1"], true, true),
        (&["deno", "run", "https://x/y.ts"], true, true),
        (&["perl", "-e", "1"], true, true),
        (&["php", "-r", "1"], true, true),
        // Program text first.
        (&["awk", "BEGIN{system(\"ls\")}"], true, true),
        (&["gawk", "{print}"], true, true),
        // Wrappers run whatever program follows them.
        (&["env", "python3"], true, true),
        (&["env", "FOO=1", "ls"], true, true),
        (&["sudo", "sh"], true, true),
        (&["xargs", "sh"], true, true),
        (&["ssh", "host"], true, true),
        (&["nice", "-n", "5", "make"], true, true),
        (&["timeout", "5", "ls"], true, true),
        // Ordinary programs.
        (&["git", "status"], false, false),
        (&["ls", "-la"], false, false),
        (&["make", "test"], false, false),
        (&["envsubst", "x"], false, false),
    ];
    for (argv, narrow, wide) in cases {
        let found = scopes("run", &json!({ "argv": argv }));
        let [first, .., last] = found.as_slice() else {
            panic!("{argv:?} offers no scope");
        };
        assert_eq!(first.any_code().is_some(), *narrow, "{argv:?}: {first:?}");
        assert_eq!(last.any_code().is_some(), *wide, "{argv:?}: {last:?}");
    }
}

/// R10911-08: a scope that lets an interpreter run whatever code it is
/// given says so plainly.
#[test]
fn an_interpreter_scope_says_it_covers_any_code() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(
        &w,
        "py",
        "dev1.1",
        "run",
        "T2",
        json!({"argv": ["python3", "-c", "print(1)"]}),
        &["zoë"],
    );
    let offer = card(&w, "dev1.1", "py").grant.expect("offered");
    for scope in &offer.scopes {
        assert!(
            scope
                .standing
                .ends_with("That is any code: python3 runs whatever program text it is given."),
            "{}",
            scope.standing
        );
    }
    let git = card_of_git(&w);
    assert!(!git.contains("any code"), "{git}");
}

fn card_of_git(w: &World) -> String {
    request(w, "git", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    card(w, "dev1.1", "git").grant.expect("offered").scopes[0]
        .standing
        .clone()
}

/// The first table each queued commit writes, in order.
fn outbox_ops(w: &World) -> Vec<String> {
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(ops_json, '$[0].table') FROM _syncular_outbox \
             WHERE json_extract(ops_json, '$[0].table') IN ('approvals', 'messages') ORDER BY seq",
        )
        .expect("outbox");
    stmt.query_map([], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("tables")
}

#[test]
fn a_message_with_files_declines_too_and_an_archived_session_holds_no_banner() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let files = vec![
        crate::hq::attachments::stage_bytes("a.txt", "text/plain", b"tea".to_vec())
            .expect("staged"),
    ];
    let banner = shown(&w, "dev1.1");
    crate::hq::attachments::attach(&w.alpha, &w.decisions, "dev1.1", "", files, &banner)
        .expect("attached");
    assert_eq!(
        card(&w, "dev1.1", "mine").state,
        AgentApprovalStateVm::Denied
    );

    session(&w, "dev1.2", None, true);
    request(&w, "old", "dev1.2", "run", "T2", git_status(), &["zoë"]);
    let old = card(&w, "dev1.2", "old");
    assert!(!old.declined_by_send);
    assert_eq!(old.grant, None, "a closed session offers no grant");
}

/// Settle every queued commit as a round would, at the replica file: a
/// refused one leaves the outbox with `code` journalled; an accepted one
/// also lands in the server's copy, so its row stays.
fn settle(w: &World, refused: Option<(&str, &str)>) {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let mut conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("timeout");
    let queued: Vec<(String, String)> = conn
        .prepare("SELECT commit_id, ops_json FROM _syncular_outbox ORDER BY seq")
        .expect("outbox")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("queued")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert!(!queued.is_empty(), "a real local commit to settle");
    let tx = conn.transaction().expect("transaction");
    for (commit, operations) in queued {
        let ops: Vec<CommitOperation> = serde_json::from_str(&operations).expect("operations");
        let results: Vec<_> = (0..ops.len())
            .map(|index| {
                let op_index = i32::try_from(index).expect("small");
                match refused {
                    Some((code, message)) => CommitOperationOutcome::Error {
                        rejection: RejectionRecord {
                            client_commit_id: commit.clone(),
                            op_index,
                            code: code.to_owned(),
                            message: message.to_owned(),
                            retryable: false,
                            details: None,
                            operation: None,
                        },
                    },
                    None => CommitOperationOutcome::Applied { op_index },
                }
            })
            .collect();
        tx.execute(
            "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
             results_json, operations_json) VALUES (?1, ?2, 1, ?3, ?4)",
            rusqlite::params![
                commit,
                if refused.is_some() {
                    "rejected"
                } else {
                    "applied"
                },
                serde_json::to_string(&results).expect("results"),
                serde_json::to_string(&ops).expect("operations")
            ],
        )
        .expect("outcome");
        for op in &ops {
            let (base, visible) = (format!("_syncular_base_{}", op.table), &op.table);
            if refused.is_some() {
                // The server never had it: the optimistic row goes back to
                // the server's copy, as a refusal's arrival leaves it.
                tx.execute(
                    &format!("DELETE FROM {visible} WHERE id = ?1"),
                    [&op.row_id],
                )
                .expect("refused row leaves");
                tx.execute(
                    &format!("INSERT INTO {visible} SELECT * FROM {base} WHERE id = ?1"),
                    [&op.row_id],
                )
                .expect("server copy back");
                continue;
            }
            tx.execute(
                &format!(
                    "INSERT INTO {base} SELECT * FROM {visible} WHERE id = ?1 \
                     AND NOT EXISTS (SELECT 1 FROM {base} WHERE id = ?1)"
                ),
                [&op.row_id],
            )
            .expect("server copy");
            for column in op.values.iter().flat_map(|values| values.keys()) {
                tx.execute(
                    &format!(
                        "UPDATE {base} SET \"{column}\" = (SELECT \"{column}\" FROM {visible} \
                         WHERE id = ?1) WHERE id = ?1"
                    ),
                    [&op.row_id],
                )
                .expect("server column");
            }
        }
        tx.execute(
            "DELETE FROM _syncular_outbox WHERE commit_id = ?1",
            [&commit],
        )
        .expect("settles");
    }
    tx.commit().expect("durable");
}

/// A grant the server refuses later is read back from the outcome record:
/// listed as refused in plain words, the card's request waiting again with
/// the same words; a new grant of the same scope sets the refusal aside.
#[test]
fn a_later_refused_grant_is_listed_and_its_request_waits_again() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    settle(&w, None);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    settle(&w, None);
    always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap1", "programArg", false),
        now(),
    )
    .expect("written");
    let listed = list(std::slice::from_ref(&w.alpha), now());
    assert_eq!(listed.grants[0].state, AgentGrantStateVm::Waiting);
    settle(&w, Some(("hq.v15.agent", "hq.v15.agent")));

    let listed = list(std::slice::from_ref(&w.alpha), now());
    assert_eq!(listed.grants.len(), 1);
    let refused = &listed.grants[0];
    assert_eq!(refused.state, AgentGrantStateVm::Refused);
    assert_eq!(
        refused.sentence,
        "The server refused this grant: that agent does not serve you in this database."
    );
    assert!(!refused.can_revoke);
    assert_eq!(
        refused.scope,
        "Run “git status …”, whatever follows “status”"
    );
    let card = card(&w, "dev1.1", "ap1");
    assert_eq!(card.state, AgentApprovalStateVm::Pending);
    assert_eq!(
        card.refused.as_deref(),
        Some("that agent does not serve you in this database.")
    );
    assert!(
        card.actions.contains(&Action::AlwaysAllow),
        "it may be asked again"
    );

    always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap1", "programArg", false),
        now(),
    )
    .expect("asked again");
    let listed = list(std::slice::from_ref(&w.alpha), now());
    assert_eq!(
        listed.grants.iter().map(|g| g.state).collect::<Vec<_>>(),
        vec![AgentGrantStateVm::Waiting],
        "the new grant set the refused one aside"
    );
}

/// Revoking is the one change: it waits, then reads revoked; a refused
/// revocation leaves the grant active with the server's words. The list
/// says what revoking does.
#[test]
fn revoking_is_listed_and_a_refused_revocation_is_said() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap1", "program", false),
        now(),
    )
    .expect("written");
    settle(&w, None);
    let listed = list(std::slice::from_ref(&w.alpha), now());
    assert_eq!(listed.note, REVOKE_NOTE);
    let grant = listed.grants[0].clone();
    assert_eq!(grant.state, AgentGrantStateVm::Active);
    assert_eq!(
        grant.sentence,
        "Always allowed in every session with Nixi in alpha."
    );
    assert!(grant.can_revoke);

    revoke(&w.alpha, &grant.id, now()).expect("revoked");
    settle(&w, Some(("hq.v15.revoked_once", "hq.v15.revoked_once")));
    let refused = list(std::slice::from_ref(&w.alpha), now()).grants[0].clone();
    assert_eq!(refused.state, AgentGrantStateVm::Active);
    assert_eq!(
        refused.refused.as_deref(),
        Some("The server refused revoking it: it was already revoked.")
    );

    revoke(&w.alpha, &grant.id, now()).expect("revoked again");
    let waiting = list(std::slice::from_ref(&w.alpha), now()).grants[0].clone();
    assert_eq!(waiting.state, AgentGrantStateVm::Revoked);
    assert_eq!(waiting.sentence, "Revoking; it stops once alpha has it.");
    assert_eq!(waiting.refused, None, "the retry set the refusal aside");
    settle(&w, None);
    let done = list(std::slice::from_ref(&w.alpha), now()).grants[0].clone();
    assert_eq!(
        (done.state, done.sentence.as_str()),
        (AgentGrantStateVm::Revoked, "Revoked.")
    );
    assert!(!done.can_revoke);
    assert!(revoke(&w.alpha, &grant.id, now()).is_err(), "once");
}

#[test]
fn an_expired_grant_reads_expired_and_offers_no_revoke() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let mut day = ask("dev1.1", "ap1", "program", true);
    day.expires_in_days = Some(1);
    always_allow(&w.alpha, &w.decisions, &day, now()).expect("written");
    settle(&w, None);
    let later = now() + 2 * 86_400_000;
    let grant = list(std::slice::from_ref(&w.alpha), later).grants[0].clone();
    assert_eq!(grant.state, AgentGrantStateVm::Expired);
    assert_eq!(grant.session.as_deref(), Some("about dev1.1"));
    assert!(!grant.can_revoke);
}

/// RG-08: a grant made on a device signed out since decides nothing
/// (`hq.v8.grant_credential`), so it reads Inactive — never Active — and
/// stays revocable; one from a device still signed in stays Active.
#[test]
fn a_grant_made_on_a_signed_out_device_reads_inactive_and_stays_revocable() {
    let w = world("zoë");
    let key = crate::hq::vocabulary::person_agent_key("zoë", NIXI);
    let grant = |id: &str, device: &str| {
        upsert(
            TABLE,
            json!({"id": id, "person_agent_key": key, "person_actor": "zoë",
                   "agent_actor": NIXI, "session_id": null, "tool": "run",
                   "args_match": "{\"program\":\"make\"}", "max_tier": "T2",
                   "expires_at_ms": null, "revoked_at_ms": null, "granted_by_client": device,
                   "created_at_ms": now() - 1_000, "author_actor": "zoë",
                   "author_client": device}),
        )
    };
    let device = |id: &str, name: &str, revoked: Option<i64>| {
        upsert(
            "clients",
            json!({"id": id, "actor_id": "zoë", "kind": "device", "name": name, "host": null,
                   "created_at_ms": 1, "revoked_at_ms": revoked, "dir": "alpha",
                   "author_actor": "server", "author_client": "server"}),
        )
    };
    w.alpha
        .replica
        .mutate(vec![
            device("dev2", "laptop", Some(now() - 500)),
            device("dev3", "phone", None),
            grant("from-laptop", "dev2"),
            grant("from-phone", "dev3"),
        ])
        .expect("written");
    settle(&w, None);
    let listed = list(std::slice::from_ref(&w.alpha), now());
    let of = |id: &str| {
        listed
            .grants
            .iter()
            .find(|grant| grant.id == id)
            .expect(id)
            .clone()
    };
    let laptop = of("from-laptop");
    assert_eq!(
        (laptop.state, laptop.sentence.as_str()),
        (
            AgentGrantStateVm::Inactive,
            "Inactive: made on “laptop”, a device that was signed out, so it decides nothing. \
             Revoke it to clear it."
        )
    );
    assert!(laptop.can_revoke, "the person can still clear it");
    assert_eq!(of("from-phone").state, AgentGrantStateVm::Active);

    revoke(&w.alpha, "from-laptop", now()).expect("revocable");
    let after = list(std::slice::from_ref(&w.alpha), now());
    let laptop = after
        .grants
        .iter()
        .find(|grant| grant.id == "from-laptop")
        .expect("listed");
    assert_eq!(laptop.state, AgentGrantStateVm::Revoked);
    assert!(!laptop.can_revoke);
}

/// A grant bound to a session ends with the session's archive, before the
/// server's revocation (R3) reaches this device: it never reads live, and
/// offers no revoke.
#[test]
fn a_session_bound_grant_reads_ended_once_its_session_is_archived() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap1", "program", true),
        now(),
    )
    .expect("written");
    settle(&w, None);
    let shown = || list(std::slice::from_ref(&w.alpha), now()).grants[0].clone();
    assert_eq!(shown().state, AgentGrantStateVm::Active);
    sessions::set_archived(&w.alpha, "dev1.1", true).expect("archived");
    let ended = shown();
    assert_eq!(ended.state, AgentGrantStateVm::Ended);
    assert!(!ended.can_revoke);
    assert!(ended.sentence.starts_with("Ended with its session"));
}

/// R10911-03: a session merely absent from this device — the person left,
/// or it has not arrived yet — does not end its grant: it keeps its state
/// and its Revoke.
#[test]
fn a_grant_whose_session_is_not_on_this_device_is_not_ended() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    always_allow(
        &w.alpha,
        &w.decisions,
        &ask("dev1.1", "ap1", "program", true),
        now(),
    )
    .expect("written");
    settle(&w, None);
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("timeout");
    for table in ["sessions", "_syncular_base_sessions"] {
        conn.execute(&format!("DELETE FROM {table} WHERE id = 'dev1.1'"), [])
            .expect("the session leaves this device");
    }
    let grant = list(std::slice::from_ref(&w.alpha), now()).grants[0].clone();
    assert_eq!(grant.state, AgentGrantStateVm::Active);
    assert!(grant.can_revoke);
    assert_eq!(
        grant.sentence,
        "Allowed in one session only; that session is not on this device."
    );
}

/// R10911-05: the next grant expiry is what wakes the views, so a grant reads
/// Expired once it passes with no row changing.
#[test]
fn the_soonest_live_grant_expiry_is_the_next_wake() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "ap1", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    request(&w, "ap2", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let t = now();
    let mut day = ask("dev1.1", "ap1", "program", false);
    day.expires_in_days = Some(1);
    always_allow(&w.alpha, &w.decisions, &day, t).expect("written");
    let mut week = ask("dev1.1", "ap2", "programArg", false);
    week.expires_in_days = Some(7);
    always_allow(&w.alpha, &w.decisions, &week, t).expect("written");
    let sources = std::slice::from_ref(&w.alpha);
    assert_eq!(next_expiry(sources, t), Some(t + 86_400_000));
    assert_eq!(
        next_expiry(sources, t + 86_400_000),
        Some(t + 7 * 86_400_000)
    );
    assert_eq!(next_expiry(sources, t + 7 * 86_400_000), None);
}

/// R10911-10: a grants unit the server revoked — the agent did not serve
/// the person when the device first asked — is never pulled again by
/// syncular-client while it stays in the window: setting the same window
/// again leaves it revoked. keeper asks again (`reask_revoked`), and once the
/// agent serves the person the next pull grants the unit: the card offers a
/// grant, and a grant the person made on another device lists here, with
/// Revoke.
#[test]
fn a_grants_unit_revoked_once_is_asked_again_so_service_that_began_shows() {
    let w = world("zoë");
    session_with(&w, "ana.1", "tola", "person", None, false);
    request(
        &w,
        "ap1",
        "ana.1",
        "run",
        "T2",
        git_status(),
        &["zoë", "ana"],
    );
    let key = crate::hq::vocabulary::person_agent_key("zoë", "tola");
    let base = sessions::window(TABLE);
    w.alpha
        .replica
        .set_window(&base, std::slice::from_ref(&key))
        .expect("window");
    // The server revokes the unit at the first pull — Tola does not serve
    // Zoë — and syncular-client records it so; the device relaunches.
    w.alpha.replica.close();
    w.alpha.replica.join();
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    let revoked = conn
        .execute(
            "UPDATE _syncular_subscriptions SET state_json = json_set(state_json, \
             '$.status', 'revoked', '$.reasonCode', 'sync.scope_revoked', '$.cursor', 0) \
             WHERE id IN (SELECT sub_id FROM _syncular_windows WHERE unit = ?1)",
            [&key],
        )
        .expect("revoked");
    assert_eq!(revoked, 1, "the window's own subscription");
    drop(conn);
    let World { dir, alpha, .. } = w;
    drop(alpha);
    let w = open_world(dir, "zoë");
    // What every window pass does — set the same units — leaves it revoked.
    w.alpha
        .replica
        .set_window(&base, std::slice::from_ref(&key))
        .expect("window");
    assert_eq!(
        w.alpha.replica.revoked_units(TABLE).expect("revoked"),
        vec![key.clone()]
    );
    assert_eq!(card(&w, "ana.1", "ap1").grant, None);

    // Asked again: a fresh subscription the next pull sends to the server.
    assert_eq!(reask_revoked(&w.alpha).expect("asked"), vec![key.clone()]);
    assert!(w
        .alpha
        .replica
        .revoked_units(TABLE)
        .expect("revoked")
        .is_empty());
    assert_eq!(
        w.alpha.replica.window_state(&base).expect("state").pending,
        vec![key.clone()],
        "waiting for its first pull"
    );

    // Tola serves Zoë now; the pull grants the unit and brings the grant Zoë
    // made about Tola on another device.
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("timeout");
    conn.execute(
        "UPDATE _syncular_subscriptions SET state_json = json_set(state_json, \
         '$.status', 'active', '$.cursor', 4, '$.effectiveScopes', json(?2)) \
         WHERE id IN (SELECT sub_id FROM _syncular_windows WHERE unit = ?1)",
        rusqlite::params![key, json!({ "person_agent": [key] }).to_string()],
    )
    .expect("granted");
    w.alpha
        .replica
        .mutate(vec![upsert(
            TABLE,
            json!({"id": "grant-from-laptop", "person_agent_key": key, "person_actor": "zoë",
                   "agent_actor": "tola", "session_id": null, "tool": "run",
                   "args_match": "{\"program\":\"make\"}", "max_tier": "T2",
                   "expires_at_ms": null, "revoked_at_ms": null, "granted_by_client": "dev2",
                   "created_at_ms": now() - 1_000, "author_actor": "zoë",
                   "author_client": "dev2"}),
        )])
        .expect("the other device's grant");
    settle(&w, None);
    assert!(
        card(&w, "ana.1", "ap1").grant.is_some(),
        "the offer appears"
    );
    let listed = list(std::slice::from_ref(&w.alpha), now());
    let laptop = listed
        .grants
        .iter()
        .find(|grant| grant.id == "grant-from-laptop")
        .expect("the other device's grant lists");
    assert_eq!(laptop.agent_name, "Tola");
    assert!(laptop.can_revoke);
}

/// 109.24 (R10924K-10, Q109-12 a): a mention declines what the banner showed,
/// as any message does — after its local checks, so a mention refused here
/// declines nothing — and the denials are written before the thread.
#[test]
fn a_mention_declines_what_the_banner_showed_before_its_thread() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let shown = shown(&w, "dev1.1");
    assert_eq!(shown.len(), 1, "the banner shows the request");
    let sources = [w.alpha.clone()];
    let send = |line: &str| {
        crate::hq::mention::send_line(&sources, &w.alpha, &w.decisions, "dev1.1", line, &shown)
    };
    // Refused here: nothing declined, nothing written.
    for line in ["@nobody hi", "@nixi hi", "@tola"] {
        send(line).expect_err("refused here");
    }
    assert_eq!(
        card(&w, "dev1.1", "mine").state,
        AgentApprovalStateVm::Pending
    );
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    let last: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM _syncular_outbox",
            [],
            |row| row.get(0),
        )
        .expect("seq");
    send("@tola look at the logs").expect("started");
    let declined = card(&w, "dev1.1", "mine");
    assert_eq!(declined.state, AgentApprovalStateVm::Denied);
    assert_eq!(declined.decided_by.as_deref(), Some("Zoë"));
    // The denial is its own commit, before the thread's one commit.
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(ops_json, '$[0].table') FROM _syncular_outbox \
             WHERE seq > ?1 ORDER BY seq",
        )
        .expect("outbox");
    let tables: Vec<String> = stmt
        .query_map([last], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("tables");
    assert_eq!(tables, vec!["approvals", "sessions"]);
}

/// 109.24 (R10924K-12, Q109-12 a): the composer's every line goes through
/// `mention::send_line`; a plain one declines what the banner showed, as
/// 109.11's send does — the denial, then the message.
#[test]
fn a_plain_line_from_the_composer_declines_what_the_banner_showed() {
    let w = world("zoë");
    session(&w, "dev1.1", None, false);
    request(&w, "mine", "dev1.1", "run", "T2", git_status(), &["zoë"]);
    let shown = shown(&w, "dev1.1");
    assert_eq!(shown.len(), 1, "the banner shows the request");
    let conn = rusqlite::Connection::open(w.dir.path().join("alpha.sqlite")).expect("opens");
    let last: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM _syncular_outbox",
            [],
            |row| row.get(0),
        )
        .expect("seq");
    crate::hq::mention::send_line(
        std::slice::from_ref(&w.alpha),
        &w.alpha,
        &w.decisions,
        "dev1.1",
        "yes, do it",
        &shown,
    )
    .expect("sent");
    let declined = card(&w, "dev1.1", "mine");
    assert_eq!(declined.state, AgentApprovalStateVm::Denied);
    assert_eq!(declined.decided_by.as_deref(), Some("Zoë"));
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(ops_json, '$[0].table') FROM _syncular_outbox \
             WHERE seq > ?1 ORDER BY seq",
        )
        .expect("outbox");
    let tables: Vec<String> = stmt
        .query_map([last], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("tables");
    assert_eq!(tables, vec!["approvals", "messages"]);
}

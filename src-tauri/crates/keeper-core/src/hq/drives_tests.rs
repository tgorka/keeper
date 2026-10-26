//! The drive selection over real on-disk replicas, seeded through `mutate`
//! with no server: nothing ticked until a person ticks it, an untick that
//! writes `[]`, the control only for a person member, the agent's ceiling,
//! the audience rule from the directories this device holds, and a server
//! refusal reaching the drive it would have changed.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::*;
use crate::hq::port::ReplicaSpec;
use crate::hq::syncular::SyncularReplica;

const SESSION: &str = "dev1.1";
const AGENT: &str = "nixie";

fn open(dir: &Path, database: &str) -> Source {
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
        actor: "ana".to_owned(),
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

fn put(source: &Source, ops: Vec<RowOp>) {
    source.replica.mutate(ops).expect("written");
}

fn actor(id: &str, kind: &str, name: &str, dir: &str) -> RowOp {
    upsert(
        "actors",
        json!({"id": id, "kind": kind, "display_name": name, "family": null, "dir": dir,
               "created_at_ms": 1, "author_actor": "server", "author_client": "server"}),
    )
}

fn profile(id: &str, agent: &str, drives: Option<&[&str]>) -> RowOp {
    upsert(
        "agent_profiles",
        json!({"id": id, "actor_id": agent, "name": agent, "description": "",
               "avatar_blob": null, "definition_hash": "h", "image_digest": "d",
               "started_at_ms": 1, "dir": "alpha",
               "drives": drives.map(|d| Value::from(d.to_vec()).to_string()),
               "author_actor": agent, "author_client": id}),
    )
}

fn member(actor: &str, role: &str) -> RowOp {
    upsert(
        "session_members",
        json!({"id": format!("{SESSION}/{actor}"), "session_id": SESSION, "actor_id": actor,
               "role": role, "reads_trace": true, "added_by_actor": "ana", "added_at_ms": 1,
               "read_until": null, "author_actor": "ana", "author_client": "dev1"}),
    )
}

/// `ana` (role `role`, or no membership) in session `dev1.1` of `source`'s
/// database with agent `nixie`, archived or not.
fn seed(source: &Source, role: Option<&str>, archived: bool) {
    let db = source.database.as_str();
    let mut ops = vec![
        actor("ana", "person", "Ana", db),
        actor(AGENT, "agent", "Nixie", db),
        upsert(
            "sessions",
            json!({"id": SESSION, "title": "t", "status": "open", "agent_actor": AGENT,
                   "created_by_actor": "ana", "created_by_client": "dev1",
                   "preferred_client": null, "parent_session": null, "invite": "[]",
                   "archived": archived, "heard_until": null, "created_at_ms": 1,
                   "last_activity_ms": 1, "trust": null, "author_actor": "ana",
                   "author_client": "dev1"}),
        ),
    ];
    if let Some(role) = role {
        ops.push(member("ana", role));
    }
    put(source, ops);
}

fn row(source: &Source) -> SessionRow {
    timeline::session_row(source, SESSION).expect("the session")
}

fn drives_vm(source: &Source, sources: &[Source]) -> Option<AgentSessionDrivesVm> {
    view(source, SESSION, &row(source), sources)
}

/// Each drive of the menu: (name, ticked, may be ticked).
fn menu(vm: &AgentSessionDrivesVm) -> Vec<(String, bool, bool)> {
    vm.drives
        .iter()
        .map(|d| (d.name.clone(), d.ticked, d.reason.is_none()))
        .collect()
}

/// The stored row's drives, or `None` when there is no row.
fn stored(source: &Source) -> Option<String> {
    read(
        source,
        "SELECT drives FROM session_drives WHERE session_id = ?1",
        &[Value::from(SESSION)],
    )
    .first()
    .map(|row| text(row, "drives").to_owned())
}

#[test]
fn nothing_is_ticked_until_a_person_ticks_a_drive_and_a_tick_writes_only_that_drive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("owner"), false);
    put(
        &alpha,
        vec![profile("nixie.box", AGENT, Some(&["alpha", "gamma"]))],
    );
    let sources = [alpha.clone()];

    let vm = drives_vm(&alpha, &sources).expect("an owner has the control");
    assert_eq!(
        menu(&vm),
        [
            ("alpha".to_owned(), false, true),
            ("gamma".to_owned(), false, true)
        ],
        "no row is no drive: not even the session's own database is ticked"
    );
    assert_eq!(stored(&alpha), None, "showing writes nothing");
    assert_eq!(vm.warning, WARNING);
    assert!(!vm.pending && vm.refused.is_none());

    set(&alpha, SESSION, "gamma", true, &sources, 5).expect("ticks");
    assert_eq!(stored(&alpha).as_deref(), Some(r#"["gamma"]"#));
    let vm = drives_vm(&alpha, &sources).expect("control");
    assert_eq!(
        menu(&vm),
        [
            ("alpha".to_owned(), false, true),
            ("gamma".to_owned(), true, true)
        ]
    );
    assert!(vm.pending, "the tick waits in the outbox for the server");

    set(&alpha, SESSION, "gamma", true, &sources, 6).expect("ticked twice is once");
    set(&alpha, SESSION, "alpha", true, &sources, 7).expect("a second drive");
    assert_eq!(stored(&alpha).as_deref(), Some(r#"["gamma","alpha"]"#));
}

#[test]
fn unticking_the_last_drive_writes_an_empty_array_never_a_delete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("person"), false);
    put(&alpha, vec![profile("nixie.box", AGENT, Some(&["alpha"]))]);
    let sources = [alpha.clone()];

    set(&alpha, SESSION, "alpha", true, &sources, 5).expect("ticks");
    set(&alpha, SESSION, "alpha", false, &sources, 6).expect("unticks");
    assert_eq!(
        stored(&alpha).as_deref(),
        Some("[]"),
        "the row stays, naming no drive: a deleted id could never be written again"
    );
    let vm = drives_vm(&alpha, &sources).expect("control");
    assert_eq!(menu(&vm), [("alpha".to_owned(), false, true)]);

    set(&alpha, SESSION, "alpha", false, &sources, 7).expect("unticked twice is once");
    set(&alpha, SESSION, "alpha", true, &sources, 8).expect("ticks again");
    assert_eq!(stored(&alpha).as_deref(), Some(r#"["alpha"]"#));
}

#[test]
fn only_a_person_member_of_a_live_session_has_the_control() {
    let sources = |s: &Source| [s.clone()];
    let cases: [(Option<&str>, bool, bool); 5] = [
        (Some("owner"), false, true),
        (Some("person"), false, true),
        (Some("agent"), false, false),
        (None, false, false),
        (Some("owner"), true, false),
    ];
    for (role, archived, has) in cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let alpha = open(dir.path(), "alpha");
        seed(&alpha, role, archived);
        put(&alpha, vec![profile("nixie.box", AGENT, Some(&["alpha"]))]);
        let shown = drives_vm(&alpha, &sources(&alpha));
        assert_eq!(
            shown.is_some(),
            has,
            "role {role:?}, archived {archived}: control {shown:?}"
        );
        assert_eq!(available(&alpha, SESSION, &row(&alpha)), has);
        // The list's right-click menu offers Drives… on the same rule.
        let listed = sessions::rows(&alpha, sessions::now_ms())
            .into_iter()
            .find(|r| r.session_id == SESSION)
            .is_some_and(|r| r.chooses_drives);
        assert_eq!(listed, has, "row menu: role {role:?}, archived {archived}");
        let written = set(&alpha, SESSION, "alpha", true, &sources(&alpha), 5);
        assert_eq!(written.is_ok(), has, "role {role:?}, archived {archived}");
        if !has {
            assert_eq!(stored(&alpha), None, "nothing written for {role:?}");
        }
    }
}

#[test]
fn an_agent_that_has_not_said_what_it_reads_gets_no_control() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("owner"), false);
    let sources = [alpha.clone()];
    assert_eq!(drives_vm(&alpha, &sources), None, "no instance has said");
    put(&alpha, vec![profile("nixie.old", AGENT, None)]);
    assert_eq!(
        drives_vm(&alpha, &sources),
        None,
        "an instance older than the column"
    );
    let in_list = |a: &Source| {
        sessions::rows(a, sessions::now_ms())
            .into_iter()
            .any(|r| r.session_id == SESSION && r.chooses_drives)
    };
    assert!(!in_list(&alpha), "no Drives… in the row menu either");
    put(&alpha, vec![profile("nixie.new", AGENT, Some(&[]))]);
    let vm = drives_vm(&alpha, &sources).expect("an empty list is said");
    assert!(
        in_list(&alpha),
        "said, even empty: the row menu offers Drives…"
    );
    assert_eq!(
        menu(&vm),
        [("alpha".to_owned(), false, false)],
        "the database's own drive, not offered"
    );
}

#[test]
fn a_drive_the_agent_does_not_offer_is_shown_with_its_reason_and_cannot_be_ticked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("owner"), false);
    put(
        &alpha,
        vec![
            actor("tola", "agent", "Tola", "alpha"),
            profile("nixie.one", AGENT, Some(&["alpha"])),
            profile("nixie.two", AGENT, Some(&["delta"])),
            profile("tola.box", "tola", Some(&["omega"])),
        ],
    );
    let sources = [alpha.clone()];
    let vm = drives_vm(&alpha, &sources).expect("control");
    assert_eq!(
        menu(&vm),
        [
            ("alpha".to_owned(), false, true),
            ("delta".to_owned(), false, true),
            ("omega".to_owned(), false, false),
        ],
        "the agent's ceiling is every instance's, as the server reads it; another agent's \
         drive is listed with why not"
    );
    let omega = vm.drives.iter().find(|d| d.name == "omega").expect("omega");
    assert_eq!(omega.reason.as_deref(), Some("Nixie does not offer omega."));

    let refused = set(&alpha, SESSION, "omega", true, &sources, 5).expect_err("not offered");
    assert_eq!(refused.0, "Nixie does not offer omega.");
    let refused = set(&alpha, SESSION, "zeta", true, &sources, 5).expect_err("unknown");
    assert!(refused.0.contains("not a drive"), "{refused}");
    assert_eq!(stored(&alpha), None, "nothing written");
}

#[test]
fn the_audience_rule_is_read_from_the_directories_this_device_holds() {
    let dir = tempfile::tempdir().expect("tempdir");
    // alpha is read by Ana and Ben; beta by Ana alone.
    let alpha = open(dir.path(), "alpha");
    let beta = open(dir.path(), "beta");
    seed(&alpha, Some("owner"), false);
    put(&alpha, vec![actor("ben", "person", "Ben", "alpha")]);
    seed(&beta, Some("owner"), false);
    let offers = Some(&["alpha", "beta", "gamma"][..]);
    put(&alpha, vec![profile("nixie.box", AGENT, offers)]);
    put(&beta, vec![profile("nixie.box", AGENT, offers)]);
    let sources = [alpha.clone(), beta.clone()];

    let vm = drives_vm(&alpha, &sources).expect("control");
    assert_eq!(
        menu(&vm),
        [
            ("alpha".to_owned(), false, true),
            ("gamma".to_owned(), false, true),
            ("beta".to_owned(), false, false),
        ],
        "an alpha session may not read beta: Ben reads alpha and not beta; who reads gamma \
         this device cannot know, so the server decides"
    );
    let beta_row = vm.drives.iter().find(|d| d.name == "beta").expect("beta");
    assert_eq!(
        beta_row.reason.as_deref(),
        Some("Ben reads alpha but cannot read beta, so it cannot be ticked here.")
    );
    let refused = set(&alpha, SESSION, "beta", true, &sources, 5).expect_err("audience");
    assert!(refused.0.contains("Ben reads alpha"), "{refused}");
    assert_eq!(stored(&alpha), None);

    let vm = drives_vm(&beta, &sources).expect("control");
    assert!(
        vm.drives.iter().all(|d| d.reason.is_none()),
        "a beta session may read alpha: everyone who reads beta reads alpha: {vm:?}"
    );
    set(&beta, SESSION, "alpha", true, &sources, 5).expect("ticks");

    // Holding only alpha, this device cannot know who reads beta.
    let vm = drives_vm(&alpha, &sources[..1]).expect("control");
    assert!(
        vm.drives
            .iter()
            .find(|d| d.name == "beta")
            .expect("beta")
            .reason
            .is_none(),
        "unknown audience: offered, the server decides"
    );
}

/// Record, as a round would, the server refusing commit `commit`, which
/// wrote the session's drives as `drives`.
fn journal_refusal(path: &Path, commit: &str, drives: &[&str], code: &str) {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let operations = vec![CommitOperation {
        table: TABLE.to_owned(),
        row_id: SESSION.to_owned(),
        op: "upsert".to_owned(),
        base_version: None,
        values: json!({"session_id": SESSION, "drives": Value::from(drives.to_vec()).to_string(),
                       "updated_at_ms": 5})
        .as_object()
        .cloned(),
    }];
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
    let conn = rusqlite::Connection::open(path).expect("opens");
    conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
    conn.execute(
        "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
         results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
        rusqlite::params![
            commit,
            serde_json::to_string(&results).expect("results"),
            serde_json::to_string(&operations).expect("operations")
        ],
    )
    .expect("records");
}

#[test]
fn a_server_refusal_reaches_the_drive_it_would_have_ticked_until_the_next_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("owner"), false);
    put(
        &alpha,
        vec![profile("nixie.box", AGENT, Some(&["alpha", "gamma"]))],
    );
    let sources = [alpha.clone()];

    journal_refusal(
        &dir.path().join("alpha.sqlite"),
        "c-gamma",
        &["gamma"],
        "hq.v17.audience",
    );
    let vm = drives_vm(&alpha, &sources).expect("control");
    let gamma = vm.drives.iter().find(|d| d.name == "gamma").expect("gamma");
    assert!(!gamma.ticked, "refused: the server kept no tick");
    assert_eq!(
        gamma.refused.as_deref(),
        Some("The server refused gamma: not everyone who reads alpha may read it.")
    );
    let other = vm.drives.iter().find(|d| d.name == "alpha").expect("alpha");
    assert_eq!(
        other.refused, None,
        "only the drive the write would have changed"
    );
    assert_eq!(vm.refused, gamma.refused);

    set(&alpha, SESSION, "alpha", true, &sources, 6).expect("a new selection");
    let vm = drives_vm(&alpha, &sources).expect("control");
    assert!(
        vm.refused.is_none() && vm.drives.iter().all(|d| d.refused.is_none()),
        "a selection written since sets the refusal aside: {vm:?}"
    );
}

#[test]
fn a_refusal_without_a_drive_of_its_own_is_said_for_the_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("person"), false);
    put(&alpha, vec![profile("nixie.box", AGENT, Some(&["alpha"]))]);
    journal_refusal(
        &dir.path().join("alpha.sqlite"),
        "c-member",
        &[],
        "hq.v17.person_member",
    );
    let vm = drives_vm(&alpha, std::slice::from_ref(&alpha)).expect("control");
    assert_eq!(
        vm.refused.as_deref(),
        Some("The server refused this change: only a person in this chat may choose its drives.")
    );
    assert!(vm.drives.iter().all(|d| d.refused.is_none()));
}

/// R10923-01: V17 checks every drive of the selection, so a drive ticked
/// before the agent stopped offering it would make every other change a
/// refused write — and the refusal used to be hung on the drive clicked.
#[test]
fn a_ticked_drive_that_stopped_being_offered_holds_the_others_back_until_it_is_unticked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("owner"), false);
    put(
        &alpha,
        vec![profile("nixie.box", AGENT, Some(&["alpha", "gamma"]))],
    );
    let sources = [alpha.clone()];
    set(&alpha, SESSION, "alpha", true, &sources, 5).expect("ticks alpha");
    set(&alpha, SESSION, "gamma", true, &sources, 6).expect("ticks gamma");
    // The instance stops publishing gamma.
    put(&alpha, vec![profile("nixie.box", AGENT, Some(&["alpha"]))]);

    let vm = drives_vm(&alpha, &sources).expect("control");
    let gamma = &vm.drives[0];
    assert_eq!(
        gamma.name, "gamma",
        "the drive holding the others back comes first"
    );
    assert!(gamma.ticked);
    assert_eq!(
        gamma.reason.as_deref(),
        Some("Untick gamma to change the other drives: Nixie no longer offers it.")
    );

    let refused = set(&alpha, SESSION, "alpha", false, &sources, 7).expect_err("held back");
    assert_eq!(refused.0, "Untick gamma first: Nixie no longer offers it.");
    assert_eq!(
        stored(&alpha).as_deref(),
        Some(r#"["alpha","gamma"]"#),
        "nothing V17 must refuse is written"
    );

    set(&alpha, SESSION, "gamma", false, &sources, 8).expect("unticking it is always allowed");
    assert_eq!(stored(&alpha).as_deref(), Some(r#"["alpha"]"#));
    set(&alpha, SESSION, "alpha", false, &sources, 9).expect("then the others change");
    assert_eq!(stored(&alpha).as_deref(), Some("[]"));
}

#[test]
fn a_ticked_drive_the_audience_rule_now_refuses_is_named_in_the_refusal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    let beta = open(dir.path(), "beta");
    seed(&alpha, Some("owner"), false);
    seed(&beta, Some("owner"), false);
    let offers = Some(&["alpha", "beta"][..]);
    put(&alpha, vec![profile("nixie.box", AGENT, offers)]);
    put(&beta, vec![profile("nixie.box", AGENT, offers)]);
    let sources = [alpha.clone(), beta.clone()];
    set(&beta, SESSION, "alpha", true, &sources, 5).expect("ticks alpha");
    // Cy joins beta and not alpha.
    put(&beta, vec![actor("cy", "person", "Cy", "beta")]);

    let refused = set(&beta, SESSION, "beta", true, &sources, 6).expect_err("held back");
    assert_eq!(
        refused.0,
        "Untick alpha first: Cy reads beta but cannot read it."
    );
    assert_eq!(stored(&beta).as_deref(), Some(r#"["alpha"]"#));
    set(&beta, SESSION, "alpha", false, &sources, 7).expect("the way out");
    assert_eq!(stored(&beta).as_deref(), Some("[]"));
}

/// R10923-01's other half: the server names no drive, so its refusal is hung
/// on the drive keeper knows fails, never on the one the person clicked.
#[test]
fn a_server_refusal_is_hung_on_the_drive_that_fails_not_the_one_clicked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path(), "alpha");
    seed(&alpha, Some("owner"), false);
    put(
        &alpha,
        vec![profile("nixie.box", AGENT, Some(&["alpha", "gamma"]))],
    );
    let sources = [alpha.clone()];
    set(&alpha, SESSION, "alpha", true, &sources, 5).expect("ticks alpha");
    set(&alpha, SESSION, "gamma", true, &sources, 6).expect("ticks gamma");
    put(&alpha, vec![profile("nixie.box", AGENT, Some(&["alpha"]))]);
    // Another device unticked alpha while gamma was still ticked; refused.
    journal_refusal(
        &dir.path().join("alpha.sqlite"),
        "c-other",
        &["gamma"],
        "hq.v17.not_offered",
    );
    let vm = drives_vm(&alpha, &sources).expect("control");
    let by = |name: &str| vm.drives.iter().find(|d| d.name == name).expect(name);
    assert_eq!(
        by("gamma").refused.as_deref(),
        Some("The server refused gamma: Nixie does not offer it.")
    );
    assert_eq!(
        by("alpha").refused,
        None,
        "alpha was clicked; it is not what failed"
    );
    assert_eq!(vm.refused, by("gamma").refused);
}

/// R10923-06: two ticked drives that stopped being offered. Unticking one
/// alone would write a selection V17 still refuses for the other, so each
/// untick was refused for the other and nothing could change: unticking
/// either unticks both, in one write.
#[test]
fn unticking_either_of_two_drives_no_longer_offered_unticks_both() {
    for clicked in ["gamma", "delta"] {
        let dir = tempfile::tempdir().expect("tempdir");
        let alpha = open(dir.path(), "alpha");
        seed(&alpha, Some("owner"), false);
        put(
            &alpha,
            vec![profile(
                "nixie.box",
                AGENT,
                Some(&["alpha", "gamma", "delta"]),
            )],
        );
        let sources = [alpha.clone()];
        for drive in ["alpha", "gamma", "delta"] {
            set(&alpha, SESSION, drive, true, &sources, 5).expect("ticks");
        }
        put(&alpha, vec![profile("nixie.box", AGENT, Some(&["alpha"]))]);

        let vm = drives_vm(&alpha, &sources).expect("control");
        let said = "Untick delta and gamma to change the other drives: Nixie no longer offers \
                    them. Unticking either unticks both.";
        for name in ["gamma", "delta"] {
            let drive = vm.drives.iter().find(|d| d.name == name).expect(name);
            assert_eq!(
                drive.reason.as_deref(),
                Some(said),
                "{name} says so before acting"
            );
        }
        let refused = set(&alpha, SESSION, "alpha", false, &sources, 6).expect_err("held back");
        assert_eq!(
            refused.0,
            "Untick delta and gamma first: Nixie no longer offers them."
        );
        assert_eq!(
            stored(&alpha).as_deref(),
            Some(r#"["alpha","gamma","delta"]"#)
        );

        set(&alpha, SESSION, clicked, false, &sources, 7).expect("the way out");
        assert_eq!(
            stored(&alpha).as_deref(),
            Some(r#"["alpha"]"#),
            "unticking {clicked} unticks both in one write V17 takes"
        );
        set(&alpha, SESSION, "alpha", false, &sources, 8).expect("then a healthy change");
        assert_eq!(stored(&alpha).as_deref(), Some("[]"));
    }
}

/// R10923-06 with two causes: one drive no longer offered, one the audience
/// rule now refuses.
#[test]
fn unticking_either_of_two_drives_failing_for_different_reasons_unticks_both() {
    for clicked in ["beta", "gamma"] {
        let dir = tempfile::tempdir().expect("tempdir");
        let alpha = open(dir.path(), "alpha");
        let beta = open(dir.path(), "beta");
        seed(&alpha, Some("owner"), false);
        seed(&beta, Some("owner"), false);
        let offers = Some(&["alpha", "beta", "gamma"][..]);
        put(&alpha, vec![profile("nixie.box", AGENT, offers)]);
        put(&beta, vec![profile("nixie.box", AGENT, offers)]);
        let sources = [alpha.clone(), beta.clone()];
        for drive in ["alpha", "beta", "gamma"] {
            set(&alpha, SESSION, drive, true, &sources, 5).expect("ticks");
        }
        // Ben joins alpha and not beta; the instance stops publishing gamma.
        put(
            &alpha,
            vec![
                actor("ben", "person", "Ben", "alpha"),
                profile("nixie.box", AGENT, Some(&["alpha", "beta"])),
            ],
        );

        let vm = drives_vm(&alpha, &sources).expect("control");
        let said = "Untick beta and gamma to change the other drives: Nixie no longer offers \
                    gamma; Ben reads alpha but cannot read beta. Unticking either unticks both.";
        assert_eq!(vm.drives[0].reason.as_deref(), Some(said));
        assert_eq!(vm.drives[1].reason.as_deref(), Some(said));
        let refused = set(&alpha, SESSION, "alpha", false, &sources, 6).expect_err("held back");
        assert_eq!(
            refused.0,
            "Untick beta and gamma first: Nixie no longer offers gamma; Ben reads alpha but \
             cannot read beta."
        );

        set(&alpha, SESSION, clicked, false, &sources, 7).expect("the way out");
        assert_eq!(stored(&alpha).as_deref(), Some(r#"["alpha"]"#), "{clicked}");
        set(&alpha, SESSION, "alpha", false, &sources, 8).expect("then a healthy change");
        assert_eq!(stored(&alpha).as_deref(), Some("[]"));
    }
}

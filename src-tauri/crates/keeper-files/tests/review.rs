//! Integration tests for proposals, decisions, acknowledgements, reversals,
//! and history in `keeper-files` running under `keeper-testkit` with no live
//! server.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use keeper_accounts_api::grant::{FileItem, GrantMode};
use keeper_accounts_api::Role;
use keeper_files::{Change, Files, FilesError, Layout, PutOutcome, Target};
use keeper_files_api::caller::{Caller, GrantRef};
use keeper_files_api::history::{HistoryFilter, HistoryParams};
use keeper_files_api::refusal::Code;
use keeper_files_api::reversal::PreviewStatus;
use keeper_files_api::rows::{
    ApprovalBasis, EntityType, OriginClass, Outcome, ProposalStatus, ReviewKind,
};
use keeper_files_api::wire::{Decision, ReviewerRole, Reviewers, RoleReviewers, Verdict};
use keeper_ports::transfer::ScopedFileTransfer;
use keeper_ports::{Replica, StoreSlot};
use keeper_replica::transfer::{announced_store, HttpGrants, StoreTransfer, TransferConfig};
use keeper_testkit::files::{FilesReplica, FilesServer};
use keeper_testkit::store::StandIn;
use tempfile::TempDir;

const SERVER_URL: &str = "http://stand-in";
const DOMAIN: &str = "alpha";
const DRIVE: &str = "notes";
const PART_BYTES: u64 = 65_536;

struct World {
    server: Arc<FilesServer>,
    store: Arc<StandIn>,
    folder_a: String,
}

impl World {
    fn new() -> Self {
        let store = Arc::new(StandIn::start(DOMAIN));
        let server = FilesServer::new(DOMAIN);
        let store_check = Arc::clone(&store);
        server.set_objects(move |cid| store_check.object(cid).is_some());
        let root = server.create_drive("owner", DRIVE);
        let folder_a = server.mkdir("owner", &root, "A");
        Self {
            server,
            store,
            folder_a,
        }
    }
}

struct Device {
    replica: Arc<FilesReplica>,
    files: Files,
    dir: TempDir,
}

fn attach(world: &World, replica: Arc<FilesReplica>, actor: &str, client: &str) -> Device {
    let dir = TempDir::new().expect("tempdir");
    for unit in keeper_files::units(DOMAIN, actor, client) {
        replica.subscribe(unit).expect("subscribe unit");
    }
    replica.pull();

    let slot = StoreSlot::default();
    slot.set(
        announced_store(
            &world.store.server_url(),
            DOMAIN,
            world.store.token(),
            Duration::from_secs(5),
        )
        .expect("announced store"),
    );
    let grants = Arc::new(
        HttpGrants::new(
            &world.store.base_url(),
            world.store.token(),
            Duration::from_secs(5),
        )
        .expect("http grants"),
    );
    let mut config = TransferConfig::contract(dir.path().join("plans"));
    config.threshold = PART_BYTES;
    config.part_bytes = PART_BYTES;
    let transfer: Arc<dyn ScopedFileTransfer> = Arc::new(
        StoreTransfer::new(grants, &world.store.base_url(), slot, config).expect("store transfer"),
    );

    let files = Files::new(
        SERVER_URL,
        replica.source(DOMAIN),
        Some(transfer),
        &Layout::under(dir.path()),
    )
    .expect("open files")
    .with_receipt_wait(Duration::from_millis(500));
    files.subscribe_drives().expect("subscribe drives");
    replica.pull();

    Device {
        replica,
        files,
        dir,
    }
}

fn person_device(world: &World, actor: &str, client: &str, role: Role) -> Device {
    world.server.set_role(actor, role);
    let replica = FilesReplica::new(Arc::clone(&world.server), actor, client, role);
    attach(world, replica, actor, client)
}

fn tool_device(world: &World, mode: &str) -> Device {
    let caller = Caller::Tool {
        actor_id: "tool-1".to_owned(),
        grant: GrantRef {
            grant_id: "g1".to_owned(),
            grant_revision: "1".to_owned(),
        },
        files: vec![FileItem {
            roots: vec![world.folder_a.clone()],
            classes: vec![
                "read".to_owned(),
                "write".to_owned(),
                "reverse_own".to_owned(),
            ],
            mode: mode.to_owned(),
        }],
        verified_uploads: Vec::new(),
    };
    let actor = caller.actor_id().to_owned();
    let client = "tool-1.1";
    let replica = FilesReplica::with_caller(Arc::clone(&world.server), caller, client);
    attach(world, replica, &actor, client)
}

fn path_target(path: &str) -> Target {
    Target::Path {
        drive: DRIVE.to_owned(),
        path: path.to_owned(),
    }
}

fn write_local(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write local file");
    path
}

#[test]
fn editor_put_create_and_replace_records_direct_human_events_unlisted_by_unreviewed() {
    let world = World::new();
    let ed = person_device(&world, "ed", "ed.1", Role::Editor);
    let reviewer = person_device(&world, "r", "r.1", Role::Reviewer);
    let events_before = world.server.events().len();

    let f1 = write_local(ed.dir.path(), "d1.txt", b"first version");
    let create = ed
        .files
        .put(&f1, &path_target("A/d.txt"), &mut |_| {})
        .expect("editor create put");
    assert_eq!(create.outcome, PutOutcome::Accepted);
    ed.replica.pull();

    let f2 = write_local(ed.dir.path(), "d2.txt", b"second version");
    let replace = ed
        .files
        .put(&f2, &path_target("A/d.txt"), &mut |_| {})
        .expect("editor replace put");
    assert_eq!(replace.outcome, PutOutcome::Accepted);
    assert_eq!(replace.entry_id, create.entry_id);

    let events = world.server.events();
    assert_eq!(events.len(), events_before + 2);
    let added = &events[events_before..];
    for (event, _) in added {
        assert_eq!(event.actor_id, "ed");
        assert_eq!(event.origin_class, OriginClass::Human);
        assert_eq!(event.approval_basis, ApprovalBasis::Direct);
    }

    let history = ed
        .files
        .history(&HistoryParams {
            version: 1,
            after: "0".to_owned(),
            limit: 100,
            filter: HistoryFilter {
                entity_id: Some(create.entry_id),
                ..HistoryFilter::default()
            },
        })
        .expect("entity history");
    assert_eq!(history.events.len(), 2);
    assert_eq!(history.events[0].event.id, added[0].0.id);
    assert_eq!(history.events[1].event.id, added[1].0.id);

    let unreviewed = reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("reviewer unreviewed");
    assert!(unreviewed.events.is_empty());
}

#[test]
fn tool_propose_put_open_approve_and_reject_lifecycle() {
    let world = World::new();
    let tool = tool_device(&world, "propose");
    let reviewer = person_device(&world, "r", "r.1", Role::Reviewer);
    let events_before = world.server.events().len();
    let head_before = world.server.head();

    let put_bytes = b"proposed payload from tool-1";
    let local_p = write_local(tool.dir.path(), "p.txt", put_bytes);
    let proposed = tool
        .files
        .propose(
            &Change::Put {
                file: local_p,
                target: path_target("A/p.txt"),
            },
            &Reviewers::Actors(vec!["r".to_owned()]),
            None,
            None,
        )
        .expect("tool propose put");

    assert_eq!(
        world.store.last_grant_body().expect("proposal upload")["target"],
        serde_json::json!({"parent_id": world.folder_a, "name": "p.txt"})
    );
    let server_proposals = world.server.proposals();
    assert_eq!(server_proposals.len(), 1);
    assert_eq!(server_proposals[0].proposal_id, proposed.detail.proposal_id);
    assert_eq!(
        server_proposals[0].revision,
        proposed.detail.proposal_revision
    );
    assert_eq!(server_proposals[0].digest, proposed.detail.proposal_digest);
    assert_eq!(server_proposals[0].status, ProposalStatus::Open);
    assert_eq!(world.server.events().len(), events_before);
    assert_eq!(world.server.head(), head_before);

    tool.replica.pull();
    reviewer.replica.pull();
    let tool_own = tool.files.own_proposals().expect("tool own proposals");
    assert_eq!(tool_own.len(), 1);
    assert_eq!(tool_own[0].proposal_id, proposed.detail.proposal_id);
    assert_eq!(tool_own[0].status, ProposalStatus::Open);
    assert!(reviewer
        .files
        .own_proposals()
        .expect("reviewer own proposals")
        .is_empty());

    let inbox = world.server.inbox();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].reviewer_actor, "r");
    assert_eq!(inbox[0].proposal_id, proposed.detail.proposal_id);
    assert_eq!(inbox[0].status, ProposalStatus::Open);

    let listed = reviewer.files.proposals(None).expect("reviewer proposals");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].proposal_id, proposed.detail.proposal_id);

    let revisions = reviewer
        .files
        .proposal(&proposed.detail.proposal_id)
        .expect("reviewer proposal");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].digest, proposed.detail.proposal_digest);

    let opened = reviewer
        .files
        .open_revision(&revisions[0])
        .expect("open revision");
    assert_eq!(opened.len(), 1);
    assert_eq!(fs::read(&opened[0].file).expect("read opened"), put_bytes);

    assert_eq!(
        world.store.last_grant_body(),
        Some(serde_json::json!({
            "proposal_id": proposed.detail.proposal_id,
            "proposal_revision": proposed.detail.proposal_revision,
            "content_id": opened[0].content_id,
        }))
    );
    let approved = reviewer
        .files
        .decide(vec![Decision {
            proposal_id: proposed.detail.proposal_id.clone(),
            proposal_revision: proposed.detail.proposal_revision.clone(),
            proposal_digest: proposed.detail.proposal_digest.clone(),
            decision: Verdict::Approve,
        }])
        .expect("approve proposal");
    assert_eq!(approved.outcome, Outcome::Accepted);
    let decided_event_id = approved.event_id.clone().expect("decided event_id");

    let events_after = world.server.events();
    assert_eq!(events_after.len(), events_before + 1);
    let (event, _) = events_after.last().expect("approved event");
    assert_eq!(event.id, decided_event_id);
    assert_eq!(event.actor_id, "r");
    assert_eq!(event.origin_class, OriginClass::Human);
    match &event.approval_basis {
        ApprovalBasis::Proposal {
            proposal_id,
            proposal_revision,
            proposal_digest,
            proposer_actor,
            ..
        } => {
            assert_eq!(proposal_id, &proposed.detail.proposal_id);
            assert_eq!(proposal_revision, &proposed.detail.proposal_revision);
            assert_eq!(proposal_digest, &proposed.detail.proposal_digest);
            assert_eq!(proposer_actor, "tool-1");
        }
        other => panic!("expected Proposal approval basis, got {other:?}"),
    }

    let proposals_after = world.server.proposals();
    assert_eq!(proposals_after.len(), 1);
    assert_eq!(proposals_after[0].status, ProposalStatus::Approved);
    assert_eq!(
        proposals_after[0].decided_event_id.as_deref(),
        Some(decided_event_id.as_str())
    );
    let inbox_after = world.server.inbox();
    assert_eq!(inbox_after.len(), 1);
    assert_eq!(inbox_after[0].status, ProposalStatus::Approved);

    let local_q = write_local(tool.dir.path(), "q.txt", b"rejected payload");
    let second = tool
        .files
        .propose(
            &Change::Put {
                file: local_q,
                target: path_target("A/q.txt"),
            },
            &Reviewers::Actors(vec!["r".to_owned()]),
            None,
            None,
        )
        .expect("second propose");
    let events_before_reject = world.server.events().len();

    let rejected = reviewer
        .files
        .decide(vec![Decision {
            proposal_id: second.detail.proposal_id.clone(),
            proposal_revision: second.detail.proposal_revision.clone(),
            proposal_digest: second.detail.proposal_digest.clone(),
            decision: Verdict::Reject,
        }])
        .expect("reject proposal");
    assert_eq!(rejected.detail.len(), 1);
    assert_eq!(rejected.detail[0].status, Some(ProposalStatus::Rejected));

    let second_row = world
        .server
        .proposals()
        .into_iter()
        .find(|row| row.proposal_id == second.detail.proposal_id)
        .expect("second proposal row");
    assert_eq!(second_row.status, ProposalStatus::Rejected);
    assert_eq!(world.server.events().len(), events_before_reject);
}

#[test]
fn editor_proposes_role_addressed_move_and_delete_approved_by_reviewer() {
    let world = World::new();
    let body = b"entry to move then delete";
    let cid = world.store.seed(body);
    let size = u64::try_from(body.len()).expect("size fits u64");
    let entry_id = world
        .server
        .create_file("owner", &world.folder_a, "orig.txt", &cid, size);

    let ed = person_device(&world, "ed", "ed.1", Role::Editor);
    let reviewer = person_device(&world, "r", "r.1", Role::Reviewer);
    let role_reviewers = Reviewers::Role(RoleReviewers {
        role: ReviewerRole::Reviewer,
    });

    let move_prop = ed
        .files
        .propose(
            &Change::Move {
                target: path_target("A/orig.txt"),
                to: path_target("A/renamed.txt"),
            },
            &role_reviewers,
            None,
            None,
        )
        .expect("propose move");
    let move_decision = reviewer
        .files
        .decide(vec![Decision {
            proposal_id: move_prop.detail.proposal_id,
            proposal_revision: move_prop.detail.proposal_revision,
            proposal_digest: move_prop.detail.proposal_digest,
            decision: Verdict::Approve,
        }])
        .expect("approve move");
    assert_eq!(move_decision.outcome, Outcome::Accepted);

    ed.replica.pull();
    let after_move = ed.files.list(&path_target("A")).expect("list after move");
    assert_eq!(after_move.entries.len(), 1);
    assert_eq!(after_move.entries[0].entry.id, entry_id);
    assert_eq!(after_move.entries[0].entry.name, "renamed.txt");

    let delete_prop = ed
        .files
        .propose(
            &Change::Delete {
                target: path_target("A/renamed.txt"),
            },
            &role_reviewers,
            None,
            None,
        )
        .expect("propose delete");
    let delete_decision = reviewer
        .files
        .decide(vec![Decision {
            proposal_id: delete_prop.detail.proposal_id,
            proposal_revision: delete_prop.detail.proposal_revision,
            proposal_digest: delete_prop.detail.proposal_digest,
            decision: Verdict::Approve,
        }])
        .expect("approve delete");
    assert_eq!(delete_decision.outcome, Outcome::Accepted);

    ed.replica.pull();
    let after_delete = ed.files.list(&path_target("A")).expect("list after delete");
    assert!(after_delete.entries.is_empty());
}

#[test]
fn tool_apply_put_acknowledged_once_and_second_change_reversed() {
    let world = World::new();
    let tool = tool_device(&world, "apply");
    let reviewer = person_device(&world, "r", "r.1", Role::Reviewer);

    let f1 = write_local(tool.dir.path(), "a.txt", b"applied by tool");
    let put1 = tool
        .files
        .put(&f1, &path_target("A/a.txt"), &mut |_| {})
        .expect("tool apply put");
    assert_eq!(put1.outcome, PutOutcome::Accepted);

    let (target_event_before, target_ops_before) =
        world.server.events().pop().expect("applied tool event");
    assert_eq!(target_event_before.actor_id, "tool-1");
    assert_eq!(target_event_before.origin_class, OriginClass::Tool);
    match &target_event_before.approval_basis {
        ApprovalBasis::Grant { mode, .. } => assert_eq!(*mode, GrantMode::Apply),
        other => panic!("expected Grant(Apply), got {other:?}"),
    }

    let unreviewed = reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("unreviewed lists applied put");
    assert_eq!(unreviewed.events.len(), 1);
    assert_eq!(unreviewed.events[0].event.id, target_event_before.id);

    let events_before_ack = world.server.events().len();
    let ack = reviewer
        .files
        .acknowledge(std::slice::from_ref(&target_event_before.id))
        .expect("acknowledge applied event");
    let ack_event_id = ack.event_id.expect("ack event_id");

    let events_after_ack = world.server.events();
    assert_eq!(events_after_ack.len(), events_before_ack + 1);
    let (ack_event, ack_ops) = events_after_ack.last().expect("ack event");
    assert_eq!(ack_event.id, ack_event_id);
    assert_eq!(ack_ops.len(), 1);
    assert_eq!(ack_ops[0].entity_type, EntityType::Review);

    let reviews = world.server.reviews();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].kind, ReviewKind::Acknowledgement);
    assert_eq!(reviews[0].target_event_id, target_event_before.id);

    assert!(reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("unreviewed after ack")
        .events
        .is_empty());

    let (target_event_after, target_ops_after) = events_after_ack
        .iter()
        .find(|(ev, _)| ev.id == target_event_before.id)
        .expect("target event remains");
    assert_eq!(target_event_after, &target_event_before);
    assert_eq!(target_ops_after, &target_ops_before);

    let events_before_dup = world.server.events().len();
    let dup_err = reviewer
        .files
        .acknowledge(&[target_event_before.id])
        .expect_err("second acknowledge refused");
    assert!(
        matches!(
            dup_err,
            FilesError::Refused {
                code: Code::InvalidChange,
                ..
            }
        ),
        "expected Refused(InvalidChange), got {dup_err:?}"
    );
    assert_eq!(world.server.events().len(), events_before_dup);

    let f2 = write_local(tool.dir.path(), "b.txt", b"second applied file");
    let put2 = tool
        .files
        .put(&f2, &path_target("A/b.txt"), &mut |_| {})
        .expect("second apply put");
    assert_eq!(put2.outcome, PutOutcome::Accepted);
    let second_event_id = world
        .server
        .events()
        .last()
        .expect("second applied event")
        .0
        .id
        .clone();

    let preview = reviewer
        .files
        .preview_reversal(&second_event_id)
        .expect("preview reversal");
    assert_eq!(preview.target_event_id, second_event_id);
    assert_eq!(preview.status, PreviewStatus::Ready);
    assert!(!preview.plan.is_empty());

    let reversed = reviewer
        .files
        .reverse(&second_event_id)
        .expect("reverse applied event");
    let rev_event_id = reversed.event_id.expect("reversal event_id");
    let (rev_event, _) = world.server.events().pop().expect("reversal event");
    assert_eq!(rev_event.id, rev_event_id);
    assert_eq!(
        rev_event.reverses_event_id.as_deref(),
        Some(second_event_id.as_str())
    );

    let reversal_review = world
        .server
        .reviews()
        .into_iter()
        .find(|row| row.target_event_id == second_event_id)
        .expect("reversal review row");
    assert_eq!(reversal_review.kind, ReviewKind::Reversal);

    assert!(reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("unreviewed after reverse")
        .events
        .is_empty());
}

#[test]
fn tool_in_propose_mode_plain_put_is_refused_propose_required() {
    let world = World::new();
    let tool = tool_device(&world, "propose");
    let events_before = world.server.events().len();

    let local = write_local(tool.dir.path(), "direct.txt", b"must propose instead");
    let err = tool
        .files
        .put(&local, &path_target("A/direct.txt"), &mut |_| {})
        .expect_err("plain put in propose mode must be refused");
    assert!(
        matches!(
            err,
            FilesError::Refused {
                code: Code::ProposeRequired,
                ..
            }
        ),
        "expected Refused(ProposeRequired), got {err:?}"
    );
    assert_eq!(world.server.events().len(), events_before);
}

#[test]
fn change_set_decide_accepts_three_rejects_stale_fourth_and_lists_change_set_history() {
    let world = World::new();
    let orig = b"original e";
    let cid = world.store.seed(orig);
    let size = u64::try_from(orig.len()).expect("size fits u64");
    world
        .server
        .create_file("owner", &world.folder_a, "e.txt", &cid, size);

    let tool = tool_device(&world, "propose");
    let ed = person_device(&world, "ed", "ed.1", Role::Editor);
    let reviewer = person_device(&world, "r", "r.1", Role::Reviewer);

    let cs = "01K00000000000000000000CS1";
    let reviewers = Reviewers::Actors(vec!["r".to_owned()]);

    for name in ["n1.txt", "n2.txt", "n3.txt"] {
        let local = write_local(tool.dir.path(), name, format!("body of {name}").as_bytes());
        tool.files
            .propose(
                &Change::Put {
                    file: local,
                    target: path_target(&format!("A/{name}")),
                },
                &reviewers,
                None,
                Some(cs),
            )
            .expect("propose new file in change set");
    }
    let stale_prop = tool
        .files
        .propose(
            &Change::Put {
                file: write_local(tool.dir.path(), "e_tool.txt", b"tool replace e"),
                target: path_target("A/e.txt"),
            },
            &reviewers,
            None,
            Some(cs),
        )
        .expect("propose replace e in change set");

    let ed_replace = ed
        .files
        .put(
            &write_local(ed.dir.path(), "e_ed.txt", b"editor replace e"),
            &path_target("A/e.txt"),
            &mut |_| {},
        )
        .expect("editor replace e");
    assert_eq!(ed_replace.outcome, PutOutcome::Accepted);

    let listed = reviewer
        .files
        .proposals(Some(cs))
        .expect("proposals for change set");
    assert_eq!(listed.len(), 4);

    let decisions = listed
        .iter()
        .map(|row| Decision {
            proposal_id: row.proposal_id.clone(),
            proposal_revision: row.revision.clone(),
            proposal_digest: row.digest.clone(),
            decision: Verdict::Approve,
        })
        .collect();
    let receipt = reviewer.files.decide(decisions).expect("decide change set");
    assert_eq!(receipt.outcome, Outcome::Accepted);
    assert_eq!(receipt.detail.len(), 4);

    let mut accepted_ids = BTreeSet::new();
    let mut stale_count = 0usize;
    for item in &receipt.detail {
        if item.proposal_id == stale_prop.detail.proposal_id {
            assert_eq!(item.outcome, Outcome::Rejected);
            assert_eq!(item.code, Some(Code::ProposalStale));
            stale_count += 1;
        } else {
            assert_eq!(item.outcome, Outcome::Accepted);
            let event_id = item.event_id.clone().expect("accepted item event_id");
            accepted_ids.insert(event_id);
        }
    }
    assert_eq!(accepted_ids.len(), 3);
    assert_eq!(stale_count, 1);

    let server_events = world.server.events();
    for event_id in &accepted_ids {
        let (event, _) = server_events
            .iter()
            .find(|(ev, _)| &ev.id == event_id)
            .expect("accepted event in server");
        assert_eq!(event.change_set_id.as_deref(), Some(cs));
    }

    let history = reviewer
        .files
        .change_set_history("0", &HistoryFilter::default(), cs)
        .expect("change set history");
    assert_eq!(history.events.len(), 3);
    let history_ids: BTreeSet<String> = history
        .events
        .into_iter()
        .map(|item| item.event.id)
        .collect();
    assert_eq!(history_ids, accepted_ids);
}

#[test]
fn batch_acknowledge_fifty_files_and_reject_mixed_batch_atomically() {
    let world = World::new();
    let tool = tool_device(&world, "apply");
    let reviewer = person_device(&world, "r", "r.1", Role::Reviewer);

    for i in 0..50usize {
        let name = format!("f{i:02}.txt");
        let local = write_local(tool.dir.path(), &name, format!("file {i}").as_bytes());
        let report = tool
            .files
            .put(&local, &path_target(&format!("A/{name}")), &mut |_| {})
            .expect("apply put in batch");
        assert_eq!(report.outcome, PutOutcome::Accepted);
    }

    let unreviewed_50 = reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("unreviewed 50");
    assert_eq!(unreviewed_50.events.len(), 50);
    let ids_50: Vec<String> = unreviewed_50
        .events
        .iter()
        .map(|item| item.event.id.clone())
        .collect();

    let events_before_ack = world.server.events().len();
    let ack = reviewer
        .files
        .acknowledge(&ids_50)
        .expect("acknowledge all 50");
    let ack_event_id = ack.event_id.expect("batch ack event_id");

    let events_after_ack = world.server.events();
    assert_eq!(events_after_ack.len(), events_before_ack + 1);
    let (ack_event, ack_ops) = events_after_ack.last().expect("batch ack event");
    assert_eq!(ack_event.id, ack_event_id);
    assert_eq!(ack_ops.len(), 50);
    assert!(ack_ops
        .iter()
        .all(|op| op.entity_type == EntityType::Review));

    let reviews = world.server.reviews();
    assert_eq!(reviews.len(), 50);
    assert!(reviews
        .iter()
        .all(|row| row.kind == ReviewKind::Acknowledgement));
    assert!(reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("unreviewed after batch ack")
        .events
        .is_empty());

    for i in 50..52usize {
        let name = format!("f{i:02}.txt");
        let local = write_local(tool.dir.path(), &name, format!("file {i}").as_bytes());
        let report = tool
            .files
            .put(&local, &path_target(&format!("A/{name}")), &mut |_| {})
            .expect("extra apply put");
        assert_eq!(report.outcome, PutOutcome::Accepted);
    }

    let unreviewed_2 = reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("unreviewed 2");
    assert_eq!(unreviewed_2.events.len(), 2);
    let mut mixed: Vec<String> = unreviewed_2
        .events
        .iter()
        .map(|item| item.event.id.clone())
        .collect();
    mixed.push(ids_50[0].clone());

    let events_before_refusal = world.server.events().len();
    let err = reviewer
        .files
        .acknowledge(&mixed)
        .expect_err("mixed acknowledge must be refused");
    assert!(
        matches!(
            err,
            FilesError::Refused {
                code: Code::InvalidChange,
                ..
            }
        ),
        "expected Refused(InvalidChange), got {err:?}"
    );
    assert_eq!(world.server.events().len(), events_before_refusal);

    let still_unreviewed = reviewer
        .files
        .unreviewed("0", 100, None)
        .expect("still unreviewed 2");
    assert_eq!(still_unreviewed.events.len(), 2);
}

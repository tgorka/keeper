use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use keeper_accounts_api::grant::FileItem;
use keeper_accounts_api::Role;
use keeper_files::{Files, Layout};
use keeper_files_api::caller::{Caller, GrantRef};
use keeper_ports::transfer::ScopedFileTransfer;
use keeper_ports::{Replica, StoreSlot};
use keeper_replica::transfer::{announced_store, HttpGrants, StoreTransfer, TransferConfig};
use keeper_testkit::files::{FilesReplica, FilesServer};
use keeper_testkit::store::StandIn;
use serde_json::{json, Value};
use tempfile::TempDir;

use super::*;

const SERVER_URL: &str = "http://stand-in";
const DOMAIN: &str = "alpha";
const DRIVE: &str = "notes";
const PART_BYTES: u64 = 65_536;

#[derive(Parser)]
struct T {
    #[command(flatten)]
    args: Args,
}

fn command(verb: &[&str]) -> Command {
    let mut argv = vec![
        "t",
        "--state",
        "/x",
        "--url",
        "http://x",
        "--database",
        DOMAIN,
        "--actor",
        "a",
        "--client",
        "a.1",
    ];
    argv.extend_from_slice(verb);
    T::try_parse_from(argv).expect("parse cli").args.command
}

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
        server.set_role("ed", Role::Editor);
        server.set_role("r", Role::Reviewer);
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

fn open_device(world: &World, replica: Arc<FilesReplica>, actor: &str, client: &str) -> Device {
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

fn person(world: &World, actor: &str, client: &str, role: Role) -> Device {
    let replica = FilesReplica::new(Arc::clone(&world.server), actor, client, role);
    open_device(world, replica, actor, client)
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
    let replica = FilesReplica::with_caller(Arc::clone(&world.server), caller, "tool-1.1");
    open_device(world, replica, "tool-1", "tool-1.1")
}

fn write_local(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write local file");
    path
}

fn run_ok(files: &Files, verb: &[&str]) -> Value {
    let cmd = command(verb);
    let mut ans = answer(files, &cmd).expect("expected ok answer");
    ans["ok"] = json!(true);
    assert_eq!(exit_code(&ans), 0);
    ans
}

fn run_err(files: &Files, verb: &[&str]) -> Value {
    let cmd = command(verb);
    answer(files, &cmd).expect_err("expected error answer")
}

#[test]
fn editor_puts_twice_and_history_records_two_human_events() {
    let world = World::new();
    let ed = person(&world, "ed", "ed.1", Role::Editor);

    let f1 = write_local(ed.dir.path(), "d1.txt", b"first draft\n");
    let put1 = run_ok(
        &ed.files,
        &[
            "put",
            "--file",
            f1.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/d.txt",
        ],
    );
    assert_eq!(put1["outcome"], "accepted");
    let entry_id = put1["entry_id"].as_str().expect("entry_id").to_owned();
    ed.replica.pull();

    let f2 = write_local(ed.dir.path(), "d2.txt", b"second draft\n");
    let put2 = run_ok(
        &ed.files,
        &[
            "put",
            "--file",
            f2.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/d.txt",
        ],
    );
    assert_eq!(put2["outcome"], "accepted");
    assert_eq!(put2["entry_id"], entry_id);

    let hist = run_ok(&ed.files, &["history", "--entity", &entry_id]);
    let events = hist["events"].as_array().expect("events array");
    assert_eq!(events.len(), 2);
    for item in events {
        assert_eq!(item["event"]["origin_class"], "human");
    }
}

#[test]
fn tool_proposes_reviewer_opens_and_approves_and_missing_reviewer_is_usage_error() {
    let world = World::new();
    let tool = tool_device(&world, "propose");
    let reviewer = person(&world, "r", "r.1", Role::Reviewer);

    let body = b"proposed file contents\n";
    let f = write_local(tool.dir.path(), "p.txt", body);
    let f_str = f.to_str().expect("utf8");

    let proposed = run_ok(
        &tool.files,
        &[
            "put",
            "--file",
            f_str,
            "--drive",
            DRIVE,
            "A/p.txt",
            "--propose",
            "--reviewer",
            "r",
        ],
    );
    assert_eq!(proposed["outcome"], "proposed");
    let proposal_id = proposed["proposal_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .expect("proposal_id")
        .to_owned();

    let listed = run_ok(&reviewer.files, &["proposals"]);
    let proposals = listed["proposals"].as_array().expect("proposals array");
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0]["proposal_id"], proposal_id);

    let opened = run_ok(&reviewer.files, &["proposal", &proposal_id, "--open"]);
    assert_eq!(opened["proposal_id"], proposal_id);
    let opened_files = opened["opened"].as_array().expect("opened array");
    assert_eq!(opened_files.len(), 1);
    let opened_path = opened_files[0]["file"].as_str().expect("opened file path");
    assert_eq!(fs::read(opened_path).expect("read opened bytes"), body);

    let named = format!(
        "{proposal_id}/{}",
        proposed["proposal_revision"].as_str().expect("rev")
    );
    let decided = run_ok(&reviewer.files, &["decide", &named, "--approve"]);
    assert_eq!(decided["outcome"], "accepted");
    let items = decided["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert!(items[0]["event_id"].as_str().is_some_and(|s| !s.is_empty()));

    let after = run_ok(&reviewer.files, &["proposals"]);
    assert!(after["proposals"]
        .as_array()
        .expect("proposals array")
        .is_empty());

    let err = run_err(
        &tool.files,
        &[
            "put",
            "--file",
            f_str,
            "--drive",
            DRIVE,
            "A/p2.txt",
            "--propose",
        ],
    );
    assert_eq!(err["code"], "usage");
    assert_eq!(exit_code(&err), 2);
}

/// Sends revision 2 of `proposal_id` on `base` (revision 1) from `device`,
/// the way a proposer revises a proposal the reviewer already opened.
fn revise(device: &Device, base: &Value) {
    let request_id = "01JREV00000000000000000000";
    let params = json!({
        "version": 1,
        "request_id": request_id,
        "context_id": null,
        "payload": {
            "proposal_id": base["proposal_id"],
            "base_revision": base["revision"],
            "change": base["change"],
            "reviewers": base["reviewers"],
            "expires_at_ms": null,
        },
    });
    device
        .replica
        .remote_command("files.propose", request_id, params)
        .expect("revise proposal");
}

fn propose_put(device: &Device, name: &str, extra: &[&str]) -> Value {
    let f = write_local(device.dir.path(), name, name.as_bytes());
    let path = format!("A/{name}");
    let mut verb = vec![
        "put",
        "--file",
        f.to_str().expect("utf8"),
        "--drive",
        DRIVE,
        path.as_str(),
        "--propose",
        "--reviewer",
        "r",
    ];
    verb.extend_from_slice(extra);
    let proposed = run_ok(&device.files, &verb);
    assert_eq!(proposed["outcome"], "proposed");
    proposed
}

fn has_entry(world: &World, name: &str) -> bool {
    world.server.entries().iter().any(|e| e.name == name)
}

#[test]
fn decide_approves_only_the_named_revision_and_refuses_a_superseded_one() {
    let world = World::new();
    let tool = tool_device(&world, "propose");
    let reviewer = person(&world, "r", "r.1", Role::Reviewer);

    let proposed = propose_put(&tool, "rev.txt", &[]);
    let id = proposed["proposal_id"]
        .as_str()
        .expect("proposal_id")
        .to_owned();
    let opened = run_ok(&reviewer.files, &["proposal", &id, "--open"]);
    let reviewed = opened["revisions"][0].clone();
    assert_eq!(reviewed["revision"], "1");

    // The proposer sends revision 2 after the reviewer opened revision 1.
    revise(&tool, &reviewed);

    // An id without a revision is a usage error: the CLI never picks one.
    let bare = run_err(&reviewer.files, &["decide", &id, "--approve"]);
    assert_eq!(bare["code"], "usage");
    assert_eq!(exit_code(&bare), 2);

    // Approving the reviewed, now superseded, revision is refused whole.
    let stale = run_err(
        &reviewer.files,
        &["decide", &format!("{id}/1"), "--approve"],
    );
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["code"], "proposal_stale");
    assert_eq!(exit_code(&stale), 2);
    assert_eq!(stale["items"][0]["proposal_revision"], "1");
    assert!(!has_entry(&world, "rev.txt"), "nothing applied");

    // The revision the reviewer names is the one approved.
    let decided = run_ok(
        &reviewer.files,
        &["decide", &format!("{id}/2"), "--approve"],
    );
    assert_eq!(decided["outcome"], "accepted");
    assert_eq!(decided["items"][0]["proposal_revision"], "2");
    assert!(has_entry(&world, "rev.txt"));
    let states: Vec<(String, String)> = world
        .server
        .proposals()
        .iter()
        .filter(|p| p.proposal_id == id)
        .map(|p| {
            (
                p.revision.clone(),
                json!(p.status).as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(states.contains(&("1".to_owned(), "superseded".to_owned())));
    assert!(states.contains(&("2".to_owned(), "approved".to_owned())));
}

#[test]
fn expired_proposal_is_not_listed_and_its_decide_is_refused_stale() {
    let world = World::new();
    let tool = tool_device(&world, "propose");
    let reviewer = person(&world, "r", "r.1", Role::Reviewer);

    // The testkit's clock moves one step per command; learn it from a first proposal.
    let first = propose_put(&tool, "first.txt", &[]);
    let created = world
        .server
        .proposals()
        .iter()
        .find(|p| p.proposal_id == first["proposal_id"].as_str().unwrap_or_default())
        .map(|p| p.created_at_ms)
        .expect("first proposal row");
    // Open when proposed (created + 1 step) and when listed next (created + 2 steps),
    // expired after one more command.
    let expires = (created + 2_500).to_string();
    let expiring = propose_put(&tool, "expiring.txt", &["--expires-at-ms", &expires]);
    let expiring_id = expiring["proposal_id"]
        .as_str()
        .expect("proposal_id")
        .to_owned();

    let listed_ids = |answer: &Value| -> Vec<String> {
        answer["proposals"]
            .as_array()
            .expect("proposals")
            .iter()
            .map(|p| p["proposal_id"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    let before = run_ok(&reviewer.files, &["proposals"]);
    assert!(
        listed_ids(&before).contains(&expiring_id),
        "listed while unexpired"
    );

    let _ = propose_put(&tool, "later.txt", &[]);
    let after = run_ok(&reviewer.files, &["proposals"]);
    let after_ids = listed_ids(&after);
    assert!(
        !after_ids.contains(&expiring_id),
        "an expired inbox row is not listed"
    );
    assert_eq!(after_ids.len(), 2);

    let stale = run_err(
        &reviewer.files,
        &["decide", &format!("{expiring_id}/1"), "--approve"],
    );
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["code"], "proposal_stale");
    assert_eq!(exit_code(&stale), 2);
    assert_eq!(stale["items"][0]["status"], "expired");
    assert!(!has_entry(&world, "expiring.txt"));
}

#[test]
fn tool_apply_unreviewed_acknowledge_preview_and_reverse() {
    let world = World::new();
    let tool = tool_device(&world, "apply");
    let reviewer = person(&world, "r", "r.1", Role::Reviewer);

    let f1 = write_local(tool.dir.path(), "one.txt", b"one\n");
    let f2 = write_local(tool.dir.path(), "two.txt", b"two\n");
    let put1 = run_ok(
        &tool.files,
        &[
            "put",
            "--file",
            f1.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/one.txt",
        ],
    );
    assert_eq!(put1["outcome"], "accepted");
    let put2 = run_ok(
        &tool.files,
        &[
            "put",
            "--file",
            f2.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/two.txt",
        ],
    );
    assert_eq!(put2["outcome"], "accepted");

    let unrev = run_ok(&reviewer.files, &["unreviewed"]);
    let events = unrev["events"].as_array().expect("unreviewed events");
    assert_eq!(events.len(), 2);
    let first = events[0]["event"]["id"]
        .as_str()
        .expect("first id")
        .to_owned();
    let second = events[1]["event"]["id"]
        .as_str()
        .expect("second id")
        .to_owned();

    let ack = run_ok(&reviewer.files, &["acknowledge", &first]);
    assert_eq!(ack["outcome"], "accepted");
    assert_eq!(ack["targets"], json!([first]));

    let preview = run_ok(&reviewer.files, &["preview-reversal", &second]);
    assert_eq!(preview["target_event_id"], second);
    assert_eq!(preview["status"], "ready");

    let reversed = run_ok(&reviewer.files, &["reverse", &second]);
    assert_eq!(reversed["outcome"], "accepted");
    assert!(reversed["event_id"].as_str().is_some_and(|s| !s.is_empty()));

    let unrev_after = run_ok(&reviewer.files, &["unreviewed"]);
    assert!(unrev_after["events"]
        .as_array()
        .expect("unreviewed events")
        .is_empty());

    let dup_err = run_err(&reviewer.files, &["acknowledge", &first]);
    assert_eq!(dup_err["code"], "invalid_change");
    assert_eq!(exit_code(&dup_err), 2);
}

#[test]
fn change_set_decide_accepts_three_and_rejects_stale_replace() {
    let world = World::new();
    let ed = person(&world, "ed", "ed.1", Role::Editor);
    let tool = tool_device(&world, "propose");
    let reviewer = person(&world, "r", "r.1", Role::Reviewer);
    let change_set = "01J00000000000000000000001";

    let existing_v1 = write_local(ed.dir.path(), "existing-v1.txt", b"v1\n");
    let seed = run_ok(
        &ed.files,
        &[
            "put",
            "--file",
            existing_v1.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/existing.txt",
        ],
    );
    assert_eq!(seed["outcome"], "accepted");
    tool.replica.pull();
    ed.replica.pull();

    for (local_name, remote_path, body) in [
        ("c1.txt", "A/c1.txt", b"c1\n".as_slice()),
        ("c2.txt", "A/c2.txt", b"c2\n".as_slice()),
        ("c3.txt", "A/c3.txt", b"c3\n".as_slice()),
    ] {
        let f = write_local(tool.dir.path(), local_name, body);
        let res = run_ok(
            &tool.files,
            &[
                "put",
                "--file",
                f.to_str().expect("utf8"),
                "--drive",
                DRIVE,
                remote_path,
                "--propose",
                "--reviewer",
                "r",
                "--change-set",
                change_set,
            ],
        );
        assert_eq!(res["outcome"], "proposed");
    }

    let tool_replace = write_local(tool.dir.path(), "existing-tool.txt", b"tool v2\n");
    let stale_prop = run_ok(
        &tool.files,
        &[
            "put",
            "--file",
            tool_replace.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/existing.txt",
            "--propose",
            "--reviewer",
            "r",
            "--change-set",
            change_set,
        ],
    );
    assert_eq!(stale_prop["outcome"], "proposed");

    let ed_replace = write_local(ed.dir.path(), "existing-ed.txt", b"editor v2\n");
    let direct = run_ok(
        &ed.files,
        &[
            "put",
            "--file",
            ed_replace.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/existing.txt",
        ],
    );
    assert_eq!(direct["outcome"], "accepted");

    // A proposal outside the change set stays open through the set's decision.
    let outside = write_local(tool.dir.path(), "outside.txt", b"outside\n");
    let outside_prop = run_ok(
        &tool.files,
        &[
            "put",
            "--file",
            outside.to_str().expect("utf8"),
            "--drive",
            DRIVE,
            "A/outside.txt",
            "--propose",
            "--reviewer",
            "r",
        ],
    );
    assert_eq!(outside_prop["outcome"], "proposed");

    let listed = run_ok(&reviewer.files, &["proposals", "--change-set", change_set]);
    assert_eq!(listed["proposals"].as_array().expect("proposals").len(), 4);

    let decided = run_ok(
        &reviewer.files,
        &["decide", "--change-set", change_set, "--approve"],
    );
    assert_eq!(decided["outcome"], "accepted");
    let items = decided["items"].as_array().expect("items array");
    assert_eq!(items.len(), 4);
    let accepted_count = items
        .iter()
        .filter(|item| item["outcome"] == "accepted")
        .count();
    let stale_count = items
        .iter()
        .filter(|item| item["outcome"] == "rejected" && item["code"] == "proposal_stale")
        .count();
    assert_eq!(accepted_count, 3);
    assert_eq!(stale_count, 1);

    let hist = run_ok(&reviewer.files, &["history", "--change-set", change_set]);
    assert_eq!(hist["events"].as_array().expect("events array").len(), 3);
    let still_open = run_ok(&reviewer.files, &["proposals"]);
    let open = still_open["proposals"].as_array().expect("proposals");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["proposal_id"], outside_prop["proposal_id"]);
}

#[test]
fn batch_acknowledge_records_three_reviews_in_one_event() {
    let world = World::new();
    let tool = tool_device(&world, "apply");
    let reviewer = person(&world, "r", "r.1", Role::Reviewer);

    for (local_name, remote_path, body) in [
        ("b1.txt", "A/b1.txt", b"b1\n".as_slice()),
        ("b2.txt", "A/b2.txt", b"b2\n".as_slice()),
        ("b3.txt", "A/b3.txt", b"b3\n".as_slice()),
    ] {
        let f = write_local(tool.dir.path(), local_name, body);
        let put = run_ok(
            &tool.files,
            &[
                "put",
                "--file",
                f.to_str().expect("utf8"),
                "--drive",
                DRIVE,
                remote_path,
            ],
        );
        assert_eq!(put["outcome"], "accepted");
    }

    let unrev = run_ok(&reviewer.files, &["unreviewed"]);
    let events = unrev["events"].as_array().expect("unreviewed events");
    assert_eq!(events.len(), 3);
    let id1 = events[0]["event"]["id"].as_str().expect("id1").to_owned();
    let id2 = events[1]["event"]["id"].as_str().expect("id2").to_owned();
    let id3 = events[2]["event"]["id"].as_str().expect("id3").to_owned();

    let ack = run_ok(&reviewer.files, &["acknowledge", &id1, &id2, &id3]);
    assert_eq!(ack["outcome"], "accepted");
    let targets = ack["targets"].as_array().expect("targets array");
    assert_eq!(targets.len(), 3);
    assert_eq!(ack["targets"], json!([id1, id2, id3]));
    let ack_event_id = ack["event_id"].as_str().expect("ack event_id");

    let reviews = world.server.reviews();
    assert_eq!(reviews.len(), 3);
    for review in &reviews {
        assert_eq!(review.created_event_id, ack_event_id);
    }
}

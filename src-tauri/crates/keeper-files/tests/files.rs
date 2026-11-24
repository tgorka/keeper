//! Integration tests for `keeper-files` running under `keeper-testkit`
//! with no live server.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use keeper_accounts_api::Role;
use keeper_files::{Dropped, Files, FilesError, Layout, Listing, PutOutcome, Target};
use keeper_ports::transfer::FileTransfer;
use keeper_ports::{Replica, StoreSlot};
use keeper_replica::transfer::{announced_store, HttpGrants, StoreTransfer, TransferConfig};
use keeper_testkit::files::{FilesReplica, FilesServer};
use keeper_testkit::store::StandIn;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const SERVER_URL: &str = "http://stand-in";
const DOMAIN: &str = "alpha";
const DRIVE: &str = "notes";
const PART_BYTES: u64 = 65_536;

struct World {
    server: Arc<FilesServer>,
    store: Arc<StandIn>,
    root: String,
    docs: String,
}

impl World {
    fn new() -> Self {
        let store = Arc::new(StandIn::start(DOMAIN));
        let server = FilesServer::new(DOMAIN);
        let store_check = Arc::clone(&store);
        server.set_objects(move |cid| store_check.object(cid).is_some());
        let root = server.create_drive("owner", DRIVE);
        let docs = server.mkdir("owner", &root, "Docs");
        Self {
            server,
            store,
            root,
            docs,
        }
    }
}

struct Device {
    replica: Arc<FilesReplica>,
    files: Files,
    dir: TempDir,
}

fn device(world: &World, actor: &str, client: &str) -> Device {
    let dir = TempDir::new().expect("tempdir");
    let replica = FilesReplica::new(Arc::clone(&world.server), actor, client, Role::Editor);
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
    let transfer: Arc<dyn FileTransfer> = Arc::new(
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

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    let mut state = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA076_1D64_78BD_642F;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        out.push(state.to_le_bytes()[7]);
    }
    out
}

fn sha(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
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
fn ls_lists_a_folder_from_the_replica_offline_as_online() {
    let world = World::new();
    let body = pattern(128, 1);
    let cid = world.store.seed(&body);
    let size = u64::try_from(body.len()).expect("size fits u64");
    let plan_id = world
        .server
        .create_file("owner", &world.docs, "plan.md", &cid, size);

    let dev = device(&world, "ana", "ana.1");
    let drives = dev.files.drives().expect("drives");
    assert_eq!(drives.len(), 1);
    assert_eq!(drives[0].name, DRIVE);
    assert_eq!(drives[0].root_id, world.root);

    let reqs_before = dev.replica.requests();
    let online = dev.files.list(&path_target("Docs")).expect("list online");
    assert_eq!(online.entries.len(), 1);
    let item = &online.entries[0];
    assert_eq!(item.entry.id, plan_id);
    assert_eq!(item.entry.name, "plan.md");
    assert_eq!(item.entry.size, Some(size));
    assert_eq!(item.entry.revision, "1");
    assert_eq!(
        item.entry.media_type.as_deref(),
        Some(keeper_files_api::names::media_type("plan.md"))
    );
    assert_eq!(item.path, "Docs/plan.md");

    dev.replica.set_online(false);
    let offline = dev.files.list(&path_target("Docs")).expect("list offline");
    assert_eq!(offline, online);
    assert_eq!(dev.replica.requests(), reqs_before);
}

#[test]
fn the_listing_json_shape_is_pinned() {
    let world = World::new();
    let body = pattern(64, 2);
    let cid = world.store.seed(&body);
    let size = u64::try_from(body.len()).expect("size fits u64");
    world
        .server
        .create_file("owner", &world.docs, "plan.md", &cid, size);

    let dev = device(&world, "ana", "ana.1");
    let listing = dev.files.list(&path_target("Docs")).expect("list");
    let value = serde_json::to_value(&listing).expect("serialize listing");

    let top = value.as_object().expect("listing object");
    let mut top_keys: Vec<&str> = top.keys().map(String::as_str).collect();
    top_keys.sort_unstable();
    assert_eq!(top_keys, vec!["at", "entries", "folder"]);
    assert_eq!(
        value["at"],
        json!({ "server": SERVER_URL, "domain": DOMAIN })
    );

    let entry_obj = value["entries"][0].as_object().expect("entry object");
    let mut entry_keys: Vec<&str> = entry_obj.keys().map(String::as_str).collect();
    entry_keys.sort_unstable();
    assert_eq!(
        entry_keys,
        vec![
            "at",
            "content_id",
            "drive",
            "id",
            "kind",
            "media_type",
            "mode",
            "mtime_ms",
            "name",
            "parent_id",
            "path",
            "revision",
            "size",
        ]
    );
    assert!(!entry_obj.contains_key("facts"));
}

#[test]
fn get_fetches_once_then_serves_the_cache_and_refetches_a_changed_copy() {
    let world = World::new();
    let body = pattern(200_000, 3);
    let cid = world.store.seed(&body);
    let size = u64::try_from(body.len()).expect("size fits u64");
    world
        .server
        .create_file("owner", &world.docs, "report.bin", &cid, size);

    let dev = device(&world, "ana", "ana.1");
    let target = path_target("Docs/report.bin");

    let first = dev.files.get(&target, &mut |_| {}).expect("first get");
    assert!(first.fetched);
    let cached_bytes = fs::read(&first.file).expect("read cached");
    assert_eq!(sha(&cached_bytes), cid);
    assert_eq!(world.store.store_gets(), 1);
    let grants_after_first = world.store.grant_requests();
    assert!(
        grants_after_first
            .iter()
            .any(|req| req.ends_with("download-grant")),
        "expected download-grant in {grants_after_first:?}"
    );

    let second = dev.files.get(&target, &mut |_| {}).expect("second get");
    assert!(!second.fetched);
    assert_eq!(world.store.store_gets(), 1);
    assert_eq!(world.store.grant_requests().len(), grants_after_first.len());

    let mut corrupted = cached_bytes;
    corrupted[0] ^= 0xFF;
    fs::write(&first.file, &corrupted).expect("overwrite cached byte");

    let third = dev.files.get(&target, &mut |_| {}).expect("third get");
    assert!(third.fetched);
    assert_eq!(sha(&fs::read(&third.file).expect("read refetched")), cid);
}

#[test]
fn put_uploads_then_commits_and_one_content_is_one_blob() {
    let world = World::new();
    let dev = device(&world, "ana", "ana.1");
    let body = pattern(150_000, 4);
    let expected_cid = sha(&body);
    let local = write_local(dev.dir.path(), "source.bin", &body);

    let first = dev
        .files
        .put(&local, &path_target("Docs/new.bin"), &mut |_| {})
        .expect("put new.bin");
    assert_eq!(first.outcome, PutOutcome::Accepted);
    assert_eq!(first.content_id, expected_cid);
    assert!(world.store.object(&first.content_id).is_some());

    dev.replica.pull();
    let listing = dev
        .files
        .list(&path_target("Docs"))
        .expect("list after put");
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].entry.name, "new.bin");
    assert_eq!(
        listing.entries[0].entry.content_id.as_deref(),
        Some(expected_cid.as_str())
    );

    let second = dev
        .files
        .put(&local, &path_target("Docs/copy.bin"), &mut |_| {})
        .expect("put copy.bin");
    assert_eq!(second.outcome, PutOutcome::Accepted);
    assert_eq!(second.bytes_sent, 0);
    assert_eq!(second.content_id, first.content_id);

    dev.replica.pull();
    let listing = dev
        .files
        .list(&path_target("Docs"))
        .expect("list after second put");
    assert_eq!(listing.entries.len(), 2);
    for item in &listing.entries {
        assert_eq!(
            item.entry.content_id.as_deref(),
            Some(expected_cid.as_str())
        );
    }
}

#[test]
fn an_offline_put_waits_and_commits_on_reconnect() {
    let world = World::new();
    let dev = device(&world, "ana", "ana.1");
    let body = pattern(2_048, 5);
    let expected_cid = sha(&body);
    let local = write_local(dev.dir.path(), "offline.txt", &body);

    dev.replica.set_online(false);
    let cmds_before = world.server.commands();

    let report = dev
        .files
        .put(&local, &path_target("Docs/offline.txt"), &mut |_| {})
        .expect("offline put");
    assert_eq!(report.outcome, PutOutcome::Queued);
    assert_eq!(dev.files.pending().expect("pending").len(), 1);
    assert_eq!(world.server.commands(), cmds_before);

    dev.replica.set_online(true);
    let flushed = dev.files.flush().expect("flush");
    assert_eq!(flushed.len(), 1);
    assert_eq!(flushed[0].outcome, PutOutcome::Accepted);
    assert!(dev.files.pending().expect("pending after flush").is_empty());

    dev.replica.pull();
    let listing = dev.files.list(&path_target("Docs")).expect("list");
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].entry.name, "offline.txt");
    assert_eq!(
        listing.entries[0].entry.content_id.as_deref(),
        Some(expected_cid.as_str())
    );
}

#[test]
fn a_changed_or_deleted_queued_source_is_dropped_and_the_puts_behind_it_commit() {
    let world = World::new();
    let dev = device(&world, "ana", "ana.1");

    let one_bytes = pattern(1_024, 11);
    let two_original = pattern(1_024, 12);
    let two_changed = pattern(1_024, 99);
    let three_bytes = pattern(1_024, 13);
    let four_bytes = pattern(1_024, 14);

    let one_cid = sha(&one_bytes);
    let two_changed_cid = sha(&two_changed);
    let four_cid = sha(&four_bytes);

    let one_local = write_local(dev.dir.path(), "one.txt", &one_bytes);
    let two_local = write_local(dev.dir.path(), "two.txt", &two_original);
    let three_local = write_local(dev.dir.path(), "three.txt", &three_bytes);
    let four_local = write_local(dev.dir.path(), "four.txt", &four_bytes);

    dev.replica.set_online(false);
    let offline = Files::new(
        SERVER_URL,
        dev.replica.source(DOMAIN),
        None,
        &Layout::under(dev.dir.path()),
    )
    .expect("open offline files");
    for (local, target) in [
        (&one_local, "Docs/one.txt"),
        (&two_local, "Docs/two.txt"),
        (&three_local, "Docs/three.txt"),
        (&four_local, "Docs/four.txt"),
    ] {
        let report = offline
            .put(local, &path_target(target), &mut |_| {})
            .expect("queue offline put");
        assert_eq!(report.outcome, PutOutcome::Queued);
    }
    drop(offline);

    let pending = dev.files.pending().expect("pending");
    assert_eq!(pending.len(), 4);
    assert!(pending.iter().all(|item| item.body.is_some()));

    fs::write(&two_local, &two_changed).expect("overwrite two.txt");
    fs::remove_file(&three_local).expect("remove three.txt");

    dev.replica.set_online(true);
    let flushed = dev.files.flush().expect("flush");
    let outcomes: Vec<PutOutcome> = flushed.into_iter().map(|r| r.outcome).collect();
    assert_eq!(
        outcomes,
        vec![
            PutOutcome::Accepted,
            PutOutcome::Dropped(Dropped::SourceChanged),
            PutOutcome::Dropped(Dropped::SourceGone),
            PutOutcome::Accepted,
        ]
    );
    assert!(dev.files.pending().expect("pending after flush").is_empty());

    dev.replica.pull();
    let listing = dev.files.list(&path_target("Docs")).expect("list Docs");
    let entries: BTreeMap<String, Option<String>> = listing
        .entries
        .into_iter()
        .map(|row| (row.entry.name, row.entry.content_id))
        .collect();
    assert_eq!(
        entries,
        BTreeMap::from([
            ("one.txt".to_owned(), Some(one_cid)),
            ("four.txt".to_owned(), Some(four_cid)),
        ])
    );
    assert!(world.store.object(&two_changed_cid).is_none());
}

#[test]
fn two_devices_offline_end_with_one_path_and_one_conflict_copy() {
    let world = World::new();
    let base = pattern(512, 60);
    let base_cid = world.store.seed(&base);
    let base_size = u64::try_from(base.len()).expect("size fits u64");
    let plan_id = world
        .server
        .create_file("owner", &world.docs, "plan.md", &base_cid, base_size);

    let dev_a = device(&world, "ana", "ana.1");
    let dev_b = device(&world, "bo", "bo.1");
    let plan_target = path_target("Docs/plan.md");
    dev_a.files.get(&plan_target, &mut |_| {}).expect("A get");
    dev_b.files.get(&plan_target, &mut |_| {}).expect("B get");

    dev_a.replica.set_online(false);
    dev_b.replica.set_online(false);

    let bytes_x = pattern(1_024, 61);
    let bytes_y = pattern(1_024, 62);
    let file_a = write_local(dev_a.dir.path(), "x.md", &bytes_x);
    let file_b = write_local(dev_b.dir.path(), "y.md", &bytes_y);

    let put_a = dev_a
        .files
        .put(&file_a, &plan_target, &mut |_| {})
        .expect("A put");
    let put_b = dev_b
        .files
        .put(&file_b, &plan_target, &mut |_| {})
        .expect("B put");
    assert_eq!(put_a.outcome, PutOutcome::Queued);
    assert_eq!(put_b.outcome, PutOutcome::Queued);

    dev_a.replica.set_online(true);
    let flushed_a = dev_a.files.flush().expect("A flush");
    assert_eq!(flushed_a.len(), 1);
    assert_eq!(flushed_a[0].outcome, PutOutcome::Accepted);

    dev_b.replica.set_online(true);
    let flushed_b = dev_b.files.flush().expect("B flush");
    assert_eq!(flushed_b.len(), 1);
    let conflict_name = match &flushed_b[0].outcome {
        PutOutcome::ConflictCopy { of, name, .. } => {
            assert_eq!(of, &plan_id);
            assert!(
                name.starts_with("plan.sync-conflict-"),
                "unexpected conflict copy prefix: {name}"
            );
            assert!(
                name.ends_with(".md"),
                "unexpected conflict copy suffix: {name}"
            );
            name.clone()
        }
        other => panic!("expected ConflictCopy, got {other:?}"),
    };

    dev_a.replica.pull();
    dev_b.replica.pull();

    let pairs = |listing: &Listing| -> Vec<(String, Option<String>)> {
        let mut out: Vec<_> = listing
            .entries
            .iter()
            .map(|item| (item.entry.name.clone(), item.entry.content_id.clone()))
            .collect();
        out.sort();
        out
    };
    let mut expected = vec![
        ("plan.md".to_owned(), Some(sha(&bytes_x))),
        (conflict_name, Some(sha(&bytes_y))),
    ];
    expected.sort();

    let list_a = dev_a.files.list(&path_target("Docs")).expect("A list");
    let list_b = dev_b.files.list(&path_target("Docs")).expect("B list");
    assert_eq!(pairs(&list_a), expected);
    assert_eq!(pairs(&list_b), expected);
}

#[test]
fn a_refused_path_makes_no_request() {
    let world = World::new();
    let dev = device(&world, "ana", "ana.1");
    let local = write_local(dev.dir.path(), "payload.txt", b"hello");

    let reqs_before = dev.replica.requests();
    let grants_before = world.store.grant_requests().len();
    let recv_before = world.store.store_bytes_received();
    let cmds_before = world.server.commands();

    for bad in [
        "../x",
        "Docs/../x",
        "/etc/passwd",
        "Docs//x",
        "Docs/\u{FFFD}.md",
    ] {
        let target = path_target(bad);
        assert!(
            matches!(
                dev.files.get(&target, &mut |_| {}),
                Err(FilesError::PathRefused(_))
            ),
            "get should refuse {bad:?}"
        );
        assert!(
            matches!(dev.files.list(&target), Err(FilesError::PathRefused(_))),
            "list should refuse {bad:?}"
        );
        assert!(
            matches!(
                dev.files.put(&local, &target, &mut |_| {}),
                Err(FilesError::PathRefused(_))
            ),
            "put should refuse {bad:?}"
        );
    }

    assert_eq!(dev.replica.requests(), reqs_before);
    assert_eq!(world.store.grant_requests().len(), grants_before);
    assert_eq!(world.store.store_bytes_received(), recv_before);
    assert_eq!(world.server.commands(), cmds_before);
}

#[test]
fn a_get_in_parts_cut_midway_continues_from_its_bytes() {
    let world = World::new();
    let len = 10 * 65_536 + 7;
    let body = pattern(len, 8);
    let cid = world.store.seed(&body);
    let size = u64::try_from(body.len()).expect("size fits u64");
    world
        .server
        .create_file("owner", &world.docs, "big.bin", &cid, size);

    let dev = device(&world, "ana", "ana.1");
    let target = path_target("Docs/big.bin");

    // The transfer may continue past the cut within the same call (it asks
    // again by `Range`) or fail it; either way the next get continues from
    // the bytes on disk, and the store never serves the file twice.
    world.store.cut_next(3 * PART_BYTES);
    let fetched = match dev.files.get(&target, &mut |_| {}) {
        Ok(fetched) => fetched,
        Err(_) => dev.files.get(&target, &mut |_| {}).expect("resumed get"),
    };
    assert!(fetched.fetched);
    assert!(
        world.store.store_bytes_served() <= size + PART_BYTES,
        "served {} bytes for a {size}-byte file",
        world.store.store_bytes_served()
    );
    assert_eq!(sha(&fs::read(&fetched.file).expect("read big")), cid);
}

#[test]
fn a_put_in_parts_cut_midway_continues_resending_at_most_one_part() {
    let world = World::new();
    let dev = device(&world, "ana", "ana.1");
    let body = pattern(10 * 65_536 + 7, 9);
    let local = write_local(dev.dir.path(), "upload.bin", &body);
    let target = path_target("Docs/upload.bin");

    // Cut the route once three parts are through.
    let store = Arc::clone(&world.store);
    let mut armed = false;
    let first = dev
        .files
        .put(&local, &target, &mut |progress| {
            if !armed && progress.done >= 3 * PART_BYTES {
                armed = true;
                store.cut_next(100);
            }
        })
        .expect("cut put");
    // A cut the transfer could not continue past leaves the put queued with
    // its plan file; the same put again continues from the parts the store
    // holds. A cut it continued past within the call is accepted at once.
    let done = if first.outcome == PutOutcome::Queued {
        assert_eq!(dev.files.pending().expect("pending after cut").len(), 1);
        let second = dev
            .files
            .put(&local, &target, &mut |_| {})
            .expect("resumed put");
        assert!(
            second.parts_skipped >= 3,
            "expected at least 3 skipped parts, got {}",
            second.parts_skipped
        );
        second
    } else {
        first
    };
    assert_eq!(done.outcome, PutOutcome::Accepted);
    assert!(dev
        .files
        .pending()
        .expect("pending after resume")
        .is_empty());
    let overhead = world
        .store
        .store_bytes_received()
        .saturating_sub(body.len() as u64);
    assert!(
        overhead <= PART_BYTES + 100,
        "re-sent {overhead} bytes, more than one part"
    );

    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for part in world.store.parts_accepted() {
        *counts.entry(part).or_insert(0) += 1;
    }
    let resent = counts.values().filter(|&&count| count > 1).count();
    assert!(resent <= 1, "more than one part resent: {counts:?}");
    assert_eq!(world.store.object(&done.content_id), Some(body));
}

#[test]
fn folders_are_by_id_and_paths_are_derived_after_a_rename_and_move() {
    let world = World::new();
    let body = pattern(4_096, 10);
    let cid = world.store.seed(&body);
    let size = u64::try_from(body.len()).expect("size fits u64");
    world
        .server
        .create_file("owner", &world.docs, "plan.md", &cid, size);
    let archive_id = world.server.mkdir("owner", &world.root, "Archive");

    let dev = device(&world, "ana", "ana.1");
    let first_get = dev
        .files
        .get(&path_target("Docs/plan.md"), &mut |_| {})
        .expect("initial get");
    assert!(first_get.fetched);
    let gets_before = world.store.store_gets();

    let l1 = dev
        .files
        .list(&Target::Entry(world.docs.clone()))
        .expect("list before move");

    world
        .server
        .move_entry("bo", &world.docs, &archive_id, "Papers");
    dev.replica.pull();

    let l2 = dev
        .files
        .list(&Target::Entry(world.docs.clone()))
        .expect("list after move");

    let ids_names = |listing: &Listing| -> Vec<(String, String)> {
        listing
            .entries
            .iter()
            .map(|item| (item.entry.id.clone(), item.entry.name.clone()))
            .collect()
    };
    assert_eq!(ids_names(&l2), ids_names(&l1));
    assert!(!l2.entries.is_empty());
    for item in &l2.entries {
        assert!(
            item.path.starts_with("Archive/Papers/"),
            "unexpected derived path: {}",
            item.path
        );
    }

    let cached = dev
        .files
        .get(&path_target("archive/papers/PLAN.MD"), &mut |_| {})
        .expect("get by folded moved path");
    assert!(!cached.fetched);
    assert_eq!(world.store.store_gets(), gets_before);

    let reqs_before = dev.replica.requests();
    let grants_before = world.store.grant_requests().len();
    assert!(matches!(
        dev.files.resolve(&path_target("Archive/Nope.md")),
        Err(FilesError::NotFound(_))
    ));
    assert_eq!(dev.replica.requests(), reqs_before);
    assert_eq!(world.store.grant_requests().len(), grants_before);
}

#[test]
fn no_replica_row_holds_file_bytes() {
    let world = World::new();
    let dev = device(&world, "ana", "ana.1");
    let body = pattern(200_000, 11);
    let local = write_local(dev.dir.path(), "payload.bin", &body);

    let report = dev
        .files
        .put(&local, &path_target("Docs/payload.bin"), &mut |_| {})
        .expect("put");
    assert_eq!(report.outcome, PutOutcome::Accepted);
    dev.replica.pull();

    let fetched = dev
        .files
        .get(&path_target("Docs/payload.bin"), &mut |_| {})
        .expect("get");

    let rows = dev
        .replica
        .query("SELECT * FROM files_entries", &[])
        .expect("query files_entries");
    assert!(!rows.is_empty());
    for row in &rows {
        let serialized = serde_json::to_string(row).expect("serialize row");
        assert!(
            serialized.len() < 2_000,
            "replica row too large ({} bytes)",
            serialized.len()
        );
    }
    assert!(
        fetched.file.starts_with(dev.dir.path().join("cache")),
        "cache file outside cache dir: {}",
        fetched.file.display()
    );
}

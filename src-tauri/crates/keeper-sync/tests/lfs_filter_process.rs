//! Regression: gitoxide MUST be able to launch and drive keeper's own
//! long-running filter (DW-140, guarding DW-206).
//!
//! Registering `filter.lfs.process` is what stops a globally-installed git-lfs
//! from answering for keeper's repositories. It also hands gitoxide a
//! subprocess to launch on every content re-read, and DW-206 is the record of
//! what that costs when the launch fails: `gix_filter::driver::State::
//! maybe_launch_process` errors *before* any driver leniency is consulted, so
//! `status` fails hard whatever `filter.lfs.required` says. Nothing commits, the
//! index is never rewritten, the same entry is re-read next pass — 90 430
//! identical log lines in one user's field measurement, unclearable by restart.
//!
//! So the key this branch installs turns "keeper's filter process starts and
//! speaks protocol version 2 correctly" into a precondition of syncing at all.
//! This test is that precondition, exercised the only way it can be: a real
//! process on the other end of a real pipe.

use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;

/// The filter binary a test may point a git config at. Neither shipping binary
/// can be used here: the app is a GUI shell, and the daemon lives in another
/// crate, which `CARGO_BIN_EXE_` cannot name.
const HARNESS: &str = env!("CARGO_BIN_EXE_lfs-filter-harness");

/// The promise is only made to a binary that keeps it.
///
/// `Engine::open` registers `std::env::current_exe()`, and "whatever executable
/// linked the engine" has been wrong before — DW-121 is months of the app binary
/// being registered for two verbs it did not implement. Under a `process` driver
/// that mistake stops being silent and starts being fatal, so the probe is what
/// decides whether the key is written at all.
#[test]
fn the_probe_tells_a_real_filter_from_a_binary_that_is_not_one() {
    assert!(
        keeper_sync::lfs::filter::serves_process(std::path::Path::new(HARNESS)),
        "a binary that serves the protocol must be recognised"
    );

    // The libtest harness running this very test: an executable that links the
    // engine and answers a filter request with its own argument-parser error.
    // Exactly the shape the probe exists to catch.
    let not_a_filter = std::env::current_exe().expect("locate the test binary");
    assert!(
        !keeper_sync::lfs::filter::serves_process(&not_a_filter),
        "a binary that cannot serve it must never be registered as one"
    );

    // And something that is not a program at all does not panic or hang.
    assert!(!keeper_sync::lfs::filter::serves_process(
        std::path::Path::new("/nonexistent/keeper")
    ));
}

/// A repository whose one tracked path is an LFS pointer whose worktree stat no
/// longer matches, with keeper's filter registered in all three forms.
///
/// The stat mismatch is the load-bearing part: it is what sends gix down the
/// *content* comparison and therefore through the filter, rather than letting
/// the stat shortcut answer. Without it both tests below would pass with no
/// filter configured at all.
fn pointer_fixture(root: &std::path::Path) -> gix::Repository {
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init");
    std::fs::write(
        root.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");

    // Exactly what `enforce_local_config_with_filter` registers, pointed at a
    // binary that really does serve it.
    for (key, value) in [
        (
            "filter.lfs.process",
            format!(
                "\"{HARNESS}\" lfs filter-process --repo \"{}\"",
                root.display()
            ),
        ),
        (
            "filter.lfs.clean",
            format!("\"{HARNESS}\" lfs clean --repo \"{}\" %f", root.display()),
        ),
        (
            "filter.lfs.smudge",
            format!("\"{HARNESS}\" lfs smudge --repo \"{}\" %f", root.display()),
        ),
        ("filter.lfs.required", "false".to_owned()),
    ] {
        std::process::Command::new("git")
            .args(["config", key, &value])
            .current_dir(root)
            .status()
            .expect("git config");
    }

    // A real object in the store, and the worktree file it stands for.
    let payload = vec![4u8; 300_000];
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(payload.as_slice())
        .expect("insert the object");
    let big = root.join("video.bin");
    std::fs::write(&big, &payload).expect("write the worktree file");

    // The index entry LFS actually produces: the POINTER's blob, the WORKTREE
    // file's stat (AD-46).
    let repo = git::repo::open(root, false).expect("open");
    let pointer = Pointer::new(oid, size);
    let blob = repo
        .write_blob(pointer.render().as_bytes())
        .expect("write blob")
        .detach();
    let metadata = gix::index::fs::Metadata::from_path_no_follow(&big).expect("stat");
    let stat = Stat::from_fs(&metadata).expect("stat convert");
    let mut index = State::new(repo.object_hash());
    index.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, "video.bin".into());
    index.sort_entries();
    let mut file = gix::index::File::from_state(index, repo.index_path());
    file.write(gix::index::write::Options::default())
        .expect("write index");

    // Commit, so HEAD exists and the entry reads as tracked rather than added.
    // `.gitattributes` joins the index here rather than in the hand-built state
    // above: `git add` merges into the index just written, where rebuilding the
    // state by hand would drop the pointer entry that is the point of the test.
    for args in [
        &["add", ".gitattributes"][..],
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "pointer",
        ][..],
    ] {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git");
    }

    // Touch the file so its stat no longer matches the entry.
    std::fs::write(&big, &payload).expect("re-write to move the stat");

    git::repo::open(root, false).expect("reopen")
}

#[test]
fn gitoxide_drives_keepers_filter_process_without_failing_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let repo = pointer_fixture(root);

    let status = git::repo::status_paths(&repo)
        .expect("status must not fail: a filter gix cannot drive is DW-206 all over again");
    println!("status with keeper's process filter registered -> {status:?}");

    // The content comparison ran the clean filter, which turned the worktree's
    // 300 KB back into the very pointer the index holds — so the path is clean.
    // A filter that failed, or one that answered with anything else, reports it
    // as modified and the folder never stops re-reading it.
    assert!(
        status.modified.is_empty(),
        "the re-read went through keeper's filter and matched: {status:?}"
    );

    // And the git binary agrees, driving the same registered process.
    let out = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(root)
        .output()
        .expect("git status");
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "git's own view: [{}]",
        String::from_utf8_lossy(&out.stdout).trim()
    );
}

/// Every child gitoxide launched for the walk is gone when the walk returns.
///
/// This is the guard for the fork pin in `src-tauri/Cargo.toml`. Upstream
/// `gix_filter::driver::State` has no `Drop` and its `shutdown` — the only code
/// in the gitoxide workspace that waits a filter child — has no production
/// caller, so each pipeline terminated its helper by closing the pipes and left
/// the corpse in the process table. `index_as_worktree` clones the state per
/// thread, so a single `status()` leaked on the order of `thread_limit + 2`.
/// Field measurement on the shipped bundle: 274 zombies plus 32 live helpers
/// after 10h29m, against a `kern.maxprocperuid` of 2666.
///
/// Nothing about that is observable from keeper's own code — `Repository::
/// status` takes no pipeline and hands none back — which is why this test looks
/// at the process table instead. It is not a sampling loop: the harness records
/// its own pid on launch, so the driver's presence is a fact on disk rather
/// than something to catch in the act.
///
/// **The corpse is the shell, not the harness.** gitoxide runs a filter command
/// through `sh -c`, so the direct child is a shell and the harness is a
/// grandchild. An unreaped filter therefore shows up as `[sh] <defunct>`
/// parented to this process, while the harness itself is re-parented away —
/// measured against the crates.io release, which leaves exactly that and passes
/// a check that only looks for the recorded pid among our own children.
///
/// So both arms are asserted: no zombie child of ours, and no recorded pid
/// still in the table. With the pin reverted the first arm fails.
#[cfg(unix)]
#[test]
fn a_finished_status_walk_reaps_the_filter_children_it_launched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let repo = pointer_fixture(root);

    let status = git::repo::status_paths(&repo).expect("status");
    assert!(
        status.modified.is_empty(),
        "the fixture must go through the filter, or there is no child to reap: {status:?}"
    );

    // An empty record would mean gitoxide answered from the single-shot pair
    // (or from no filter at all) and this test proved nothing.
    let launched = launched_filter_pids(root);
    assert!(
        !launched.is_empty(),
        "no `process` filter was launched, so the reaping claim is vacuous"
    );

    let ours = std::process::id();
    let table = process_table();
    let zombies: Vec<String> = table
        .iter()
        .filter(|row| row.ppid == ours && row.state.starts_with('Z'))
        .map(|row| format!("{} [{}] {}", row.pid, row.state, row.command))
        .collect();
    assert!(
        zombies.is_empty(),
        "the walk returned and left {} unreaped child(ren) of its own: {zombies:?}",
        zombies.len()
    );

    let survivors: Vec<String> = table
        .iter()
        .filter(|row| launched.contains(&row.pid) && row.command.contains("lfs filter-process"))
        .map(|row| format!("{} [{}] {}", row.pid, row.state, row.command))
        .collect();
    assert!(
        survivors.is_empty(),
        "{} of the {} launched filters are still in the table: {survivors:?}",
        survivors.len(),
        launched.len()
    );
}

/// A helper is a helper keeper can name: while it serves, its marker under
/// `.git/lfs/helpers/<pid>` is held under an exclusive lock and touched after
/// every request; once its git drops it, the marker is unlocked or gone.
///
/// Two arms, because the two halves of the contract are observable at
/// different moments. The **serving** half needs a helper that is provably
/// alive at the instant the marker is inspected, and a gix status walk is
/// synchronous — by the time it returns its helpers are gone — so that arm
/// drives the very command the config names by hand over the real pipe, the
/// way `filter.rs`'s own tests script `run_process`, but against the built
/// binary and a real pid. The **dropped** half is the walk itself: every
/// helper gitoxide launched has left nothing locked behind when `status`
/// returns.
#[cfg(unix)]
#[test]
fn a_serving_helper_holds_a_locked_marker_and_leaves_none_behind() {
    use fs4::fs_std::FileExt as _;
    use keeper_sync::lfs::pktline;
    use std::io::Write as _;

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let repo = pointer_fixture(root);
    let helpers = root.join(".git").join("lfs").join("helpers");

    // ---- serving: the marker is there, locked, and moves with each request.
    let mut child = std::process::Command::new(HARNESS)
        .args(["lfs", "filter-process", "--repo"])
        .arg(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("start the helper");
    let mut to_helper = child.stdin.take().expect("stdin");
    let mut from_helper = child.stdout.take().expect("stdout");

    // The handshake, as git opens it.
    let mut hello = Vec::new();
    pktline::write_line(&mut hello, "git-filter-client").expect("client");
    pktline::write_line(&mut hello, "version=2").expect("version");
    pktline::write_flush(&mut hello).expect("flush");
    for capability in ["capability=clean", "capability=smudge", "capability=delay"] {
        pktline::write_line(&mut hello, capability).expect("capability");
    }
    pktline::write_flush(&mut hello).expect("flush");
    to_helper.write_all(&hello).expect("send hello");
    to_helper.flush().expect("flush hello");
    assert_eq!(
        pktline::read_text_list(&mut from_helper).expect("server hello"),
        Some(vec!["git-filter-server".to_owned(), "version=2".to_owned()])
    );
    assert_eq!(
        pktline::read_text_list(&mut from_helper).expect("capabilities"),
        Some(vec![
            "capability=clean".to_owned(),
            "capability=smudge".to_owned()
        ])
    );

    // Handshake answered: the helper is registered, under its own pid.
    let marker = helpers.join(child.id().to_string());
    assert!(
        marker.is_file(),
        "a serving helper has a marker: {}",
        marker.display()
    );
    let probe = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&marker)
        .expect("open the marker");
    assert!(
        !probe.try_lock_exclusive().expect("try the lock"),
        "the marker is locked while the helper serves"
    );

    // Back-date it, serve one request, and the touch has moved it forward
    // again — the busy-for-forty-minutes helper is never stuck.
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
    probe.set_modified(long_ago).expect("back-date the marker");
    let payload = std::fs::read(root.join("video.bin")).expect("read the fixture");
    let mut request = Vec::new();
    pktline::write_line(&mut request, "command=clean").expect("command");
    pktline::write_line(&mut request, "pathname=video.bin").expect("pathname");
    pktline::write_flush(&mut request).expect("flush");
    for chunk in payload.chunks(pktline::MAX_DATA) {
        pktline::write_data(&mut request, chunk).expect("content");
    }
    pktline::write_flush(&mut request).expect("flush");
    to_helper.write_all(&request).expect("send the request");
    to_helper.flush().expect("flush the request");
    assert_eq!(
        pktline::read_text_list(&mut from_helper).expect("status"),
        Some(vec!["status=success".to_owned()])
    );
    let mut answer = Vec::new();
    loop {
        match pktline::read(&mut from_helper).expect("body packet") {
            pktline::Packet::Data(bytes) => answer.extend(bytes),
            pktline::Packet::Flush => break,
            pktline::Packet::Eof => panic!("the helper hung up mid-answer"),
        }
    }
    assert!(
        pktline::read_text_list(&mut from_helper)
            .expect("trailer")
            .is_some_and(|trailer| trailer.is_empty()),
        "an empty trailing status list ends a successful answer"
    );
    assert!(
        Pointer::parse(&answer).is_some(),
        "the clean of a stored object is its pointer: {}",
        String::from_utf8_lossy(&answer)
    );
    let touched = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .expect("the marker's mtime");
    assert!(
        touched > long_ago + std::time::Duration::from_secs(3600),
        "every request touches the marker; it reads {touched:?} against {long_ago:?}"
    );
    assert!(
        !probe.try_lock_exclusive().expect("try the lock again"),
        "and it stays locked between requests"
    );
    drop(probe);

    // git hangs up: the helper returns, and its last act unlinks the marker.
    drop(to_helper);
    let exit = child.wait().expect("wait for the helper");
    assert!(exit.success(), "the helper exits cleanly on EOF: {exit}");
    assert!(
        !marker.exists(),
        "a helper that returns leaves no marker: {}",
        marker.display()
    );

    // ---- dropped: the helpers a real gix walk started are gone with it.
    let by_hand = child.id();
    let status = git::repo::status_paths(&repo).expect("status");
    assert!(
        status.modified.is_empty(),
        "the fixture must go through the filter, or no helper was started: {status:?}"
    );
    let launched: Vec<u32> = launched_filter_pids(root)
        .into_iter()
        .filter(|pid| *pid != by_hand)
        .collect();
    assert!(
        !launched.is_empty(),
        "no `process` filter was launched by gix, so the second arm is vacuous"
    );
    for pid in launched {
        let marker = helpers.join(pid.to_string());
        if !marker.exists() {
            continue;
        }
        let probe = std::fs::File::open(&marker).expect("open a leftover");
        assert!(
            probe.try_lock_exclusive().expect("try a leftover's lock"),
            "helper {pid} is gone, so its marker must be unlocked or gone: {}",
            marker.display()
        );
    }
}

/// The pids the harness recorded for itself, one per `process` launch.
#[cfg(unix)]
fn launched_filter_pids(root: &std::path::Path) -> Vec<u32> {
    std::fs::read_to_string(root.join(".git").join("filter-process-pids"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

#[cfg(unix)]
struct ProcessRow {
    pid: u32,
    ppid: u32,
    state: String,
    command: String,
}

/// Every process on the machine, with its parent, state and command.
///
/// `ps` rather than `libc`: a zombie is exactly what is being looked for, and
/// `kill(pid, 0)` succeeds for one — it is reachable, it just cannot run.
/// `/proc` would be Linux-only, and the machine this defect was measured on is
/// the Mac.
#[cfg(unix)]
fn process_table() -> Vec<ProcessRow> {
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,state=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let state = fields.next()?.to_owned();
            Some(ProcessRow {
                pid,
                ppid,
                state,
                command: fields.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

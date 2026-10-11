//! `Engine::commit_paths` over a real repository and a bare remote (95.2
//! acceptance 5 and 6, R207): the guards, the lane, the trailers, the one
//! publication and the roll forward.

use super::*;
use crate::browse::Staging;
use crate::provenance::MemoryTrailer;

/// `git <args>` in `dir`, its standard output.
fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).expect("utf-8")
}

/// A profile publishing to a bare remote whose tip holds `MEMORY.md` =
/// `memory`, pulled: `(engine, platform, profile)`.
async fn with_memory(
    dir: &Path,
    remote_dir: &Path,
    memory: &str,
) -> Option<(Arc<Engine>, Arc<TestPlatform>, SyncProfile)> {
    let platform = Arc::new(TestPlatform::new(dir));
    let engine = Arc::new(Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?);
    let p = publishing_fixture(&engine, &platform, dir, remote_dir).await?;
    advance_remote_with(
        remote_dir,
        &[("MEMORY.md", memory), ("root.md", "root")],
        "memory",
    );
    engine
        .sync_once(&p.id, SyncSource::Manual)
        .await
        .expect("pull the memory");
    Some((engine, platform, p))
}

fn yes() -> CommitFence {
    Arc::new(|| true)
}

fn request(memory: &str, guard: Option<String>) -> CommitRequest {
    CommitRequest {
        writes: vec![("MEMORY.md".to_owned(), Some(memory.as_bytes().to_vec()))],
        moves: Vec::new(),
        guards: vec![("MEMORY.md".to_owned(), guard)],
        subject: "memory: nixi — 1 promoted, 0 rejected".to_owned(),
        trailers: vec![
            MemoryTrailer::MemoryOrigin("consolidator@delectra".to_owned()),
            MemoryTrailer::SourceSession("60-sessions/active/a".to_owned()),
        ],
    }
}

/// A request settling one proposal: `MEMORY.md` written, `proposals/01P.md`
/// moved to `proposals/done/`, its verdict written; with the proposal
/// committed first.
fn settling(engine: &Engine, platform: &TestPlatform, p: &SyncProfile) -> CommitRequest {
    let root = &p.local_path;
    std::fs::create_dir_all(root.join("proposals")).expect("proposals");
    std::fs::write(root.join("proposals/01P.md"), "proposal").expect("proposal");
    commit_after_settling(engine, platform, p);
    let mut ask = request("a\n§\nb", Some(blob_id(b"a")));
    ask.guards
        .push(("proposals/01P.md".to_owned(), Some(blob_id(b"proposal"))));
    ask.moves = vec![(
        "proposals/01P.md".to_owned(),
        "proposals/done/01P.md".to_owned(),
    )];
    ask.writes.push((
        "proposals/done/01P.verdict.toml".to_owned(),
        Some(b"verdict = \"promoted\"\n".to_vec()),
    ));
    ask.guards
        .push(("proposals/done/01P.verdict.toml".to_owned(), None));
    ask
}

/// The index of `MEMORY.md` in [`settling`]'s changes: the move's two, then
/// the writes.
const MEMORY_CHANGE: usize = 2;

/// What `git status --porcelain` says, sorted lines.
fn status(root: &Path) -> Vec<String> {
    let mut lines: Vec<String> = git_out(root, &["status", "--porcelain"])
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).expect("read")
}

fn record(root: &Path) -> PathBuf {
    root.join(".git/keeper-commit-paths.json")
}

/// The folder as [`settling`]'s request found it: nothing of it anywhere.
fn untouched(root: &Path, head: &str, context: &str) {
    assert_eq!(
        git_out(root, &["rev-parse", "HEAD"]),
        head,
        "{context}: HEAD"
    );
    assert_eq!(read(root, "MEMORY.md"), "a", "{context}");
    assert!(root.join("proposals/01P.md").exists(), "{context}");
    assert!(!root.join("proposals/done").exists(), "{context}");
    assert!(status(root).is_empty(), "{context}: {:?}", status(root));
    assert!(!record(root).exists(), "{context}: the record is gone");
}

/// The folder once [`settling`]'s commit is published and followed.
fn finished(root: &Path, context: &str) {
    assert_eq!(read(root, "MEMORY.md"), "a\n§\nb", "{context}");
    assert!(!root.join("proposals/01P.md").exists(), "{context}");
    assert_eq!(read(root, "proposals/done/01P.md"), "proposal", "{context}");
    assert!(
        root.join("proposals/done/01P.verdict.toml").exists(),
        "{context}"
    );
    assert!(status(root).is_empty(), "{context}: {:?}", status(root));
    assert!(!record(root).exists(), "{context}: the record is gone");
}

/// Acceptance 6: one commit holds the writes and the moves, under the
/// request's subject, keeper's provenance block and then the trailers; a
/// pass holding the lane delays it and commits none of its paths; the push
/// is queued and the next pass publishes it.
#[tokio::test(flavor = "multi_thread")]
async fn consolidation_commits_carry_the_trailers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);

    // A pass is running: the request waits for its lane and writes nothing
    // until it ends, so that pass can commit none of its paths.
    let lane = engine.reserve(&p.id).expect("the lane is free");
    let (done, ()) = tokio::join!(engine.commit_paths(&p.id, &ask, yes()), async {
        tokio::time::sleep(COMMIT_LANE_POLL * 3).await;
        assert_eq!(read(&root, "MEMORY.md"), "a");
        assert!(root.join("proposals/01P.md").exists());
        drop(lane);
    });
    let CommitPaths::Committed { commit } = done.expect("committed") else {
        panic!("a commit was made");
    };
    finished(&root, "committed");
    // A watcher pass forced right after has nothing of its own to commit.
    platform.advance_ms(60_000);
    engine.tick_profile(&p).await.expect("a tick never raises");

    let log = git_out(
        &root,
        &["log", "--format=%H", &format!("{}..HEAD", head.trim())],
    );
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        [commit.as_str()],
        "one commit"
    );
    let message = git_out(&root, &["log", "-1", "--format=%B", &commit]);
    let lines: Vec<&str> = message.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines[0], "memory: nixi — 1 promoted, 0 rejected");
    let block = lines
        .iter()
        .position(|line| line.starts_with("Keeper-Profile:"))
        .expect("keeper's provenance block");
    assert_eq!(
        &lines[lines.len() - 2..],
        [
            "Memory-Origin: consolidator@delectra",
            "Source-Session: 60-sessions/active/a"
        ]
    );
    assert!(block < lines.len() - 2);
    let files = git_out(
        &root,
        &[
            "show",
            "--no-renames",
            "--name-status",
            "--format=",
            &commit,
        ],
    );
    let mut files: Vec<&str> = files.lines().collect();
    files.sort_unstable();
    assert_eq!(
        files,
        [
            "A\tproposals/done/01P.md",
            "A\tproposals/done/01P.verdict.toml",
            "D\tproposals/01P.md",
            "M\tMEMORY.md"
        ]
    );

    engine
        .sync_once(&p.id, SyncSource::Manual)
        .await
        .expect("the queued push");
    assert_eq!(remote_main(remote_dir.path()), commit);
}

/// Acceptance 5, the third half: an edit pushed from another device before
/// the night arrives through the pull and is planned over; one pushed after
/// the plan's read moves the blob at the fetched head, and the night writes
/// nothing for that file. The guard is the blob id at `HEAD` as well as on
/// the disk: a person's commit the disk does not show, of a file only
/// guarded, refuses it too.
#[tokio::test(flavor = "multi_thread")]
async fn the_guard_compares_blob_ids_at_the_fetched_head() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let head_blob = || {
        let bytes = git::history::blob_at(&root, "HEAD", "MEMORY.md")
            .expect("read")
            .expect("tracked");
        blob_id(&bytes)
    };

    // Before the night: the person's edit is in the plan through the pull.
    advance_remote_with(
        remote_dir.path(),
        &[("MEMORY.md", "person"), ("root.md", "root")],
        "edit",
    );
    engine
        .sync_once(&p.id, SyncSource::Manual)
        .await
        .expect("pull");
    let planned = head_blob();
    assert_eq!(planned, blob_id(b"person"));
    let done = engine
        .commit_paths(&p.id, &request("person\n§\nb", Some(planned)), yes())
        .await
        .expect("commit");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");

    // After the plan's read: the pull moves the blob at the head, and the
    // guard writes nothing.
    let planned = head_blob();
    engine
        .sync_once(&p.id, SyncSource::Manual)
        .await
        .expect("publish");
    advance_remote_with(
        remote_dir.path(),
        &[("MEMORY.md", "later"), ("root.md", "root")],
        "later",
    );
    engine
        .sync_once(&p.id, SyncSource::Manual)
        .await
        .expect("pull");
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let done = engine
        .commit_paths(&p.id, &request("person\n§\nb\n§\nc", Some(planned)), yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "MEMORY.md".to_owned()
        }
    );
    assert_eq!(read(&root, "MEMORY.md"), "later");
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head, "no commit");

    // A person's commit the disk does not show, of a file the request only
    // guards — committed from the index, the read bytes back on the disk:
    // the disk matches the guard, the head does not, and nothing is
    // published on that commit.
    std::fs::write(root.join("root.md"), "mine").expect("a person's save");
    git_out(&root, &["add", "root.md"]);
    commit_as_person(&root, "mine");
    std::fs::write(root.join("root.md"), "root").expect("the read bytes back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    let mut ask = request("later\n§\nc", Some(blob_id(b"later")));
    ask.guards
        .push(("root.md".to_owned(), Some(blob_id(b"root"))));
    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "root.md".to_owned()
        }
    );
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs, "no commit");
    assert_eq!(git_out(&root, &["show", "HEAD:root.md"]), "mine");
    assert_eq!(read(&root, "MEMORY.md"), "later");
}

/// Acceptance 5, the second half, and R95CR-02: a file changed on the disk
/// between the plan's read and the request — its blob at the head
/// unchanged — refuses the request, and nothing of it is written: not the
/// other file, and not over a person's untracked file that already holds
/// the requested bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_concurrent_edit_skips_the_night() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::write(root.join("MEMORY.md"), "typed by a person").expect("edit");
    let mut ask = request("a\n§\nb", Some(blob_id(b"a")));
    ask.writes
        .push(("other.md".to_owned(), Some(b"other".to_vec())));
    ask.guards.push(("other.md".to_owned(), None));
    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "MEMORY.md".to_owned()
        }
    );
    assert_eq!(read(&root, "MEMORY.md"), "typed by a person");
    assert!(!root.join("other.md").exists(), "nothing of the request");

    std::fs::write(root.join("MEMORY.md"), "a").expect("undo");
    std::fs::write(root.join("other.md"), "other").expect("the person's own");
    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "other.md".to_owned()
        }
    );
    assert_eq!(read(&root, "other.md"), "other", "the person's file stays");
    assert!(!record(&root).exists());
}

/// A path that would land elsewhere through a link, or inside `.git`, is
/// refused before anything is written; so is a write that does not say
/// what it is written over.
#[tokio::test(flavor = "multi_thread")]
async fn a_path_through_a_link_or_unguarded_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    std::os::unix::fs::symlink(outside.path(), p.local_path.join("away")).expect("link");
    for path in ["away/MEMORY.md", ".git/config", "../MEMORY.md"] {
        let ask = CommitRequest {
            writes: vec![(path.to_owned(), Some(b"x".to_vec()))],
            guards: vec![(path.to_owned(), None)],
            ..CommitRequest::default()
        };
        assert!(
            engine.commit_paths(&p.id, &ask, yes()).await.is_err(),
            "{path}"
        );
    }
    assert!(!outside.path().join("MEMORY.md").exists());
    let unguarded = CommitRequest {
        writes: vec![("new.md".to_owned(), Some(b"x".to_vec()))],
        ..CommitRequest::default()
    };
    assert!(engine.commit_paths(&p.id, &unguarded, yes()).await.is_err());
    assert!(!p.local_path.join("new.md").exists());
}

/// R95CR-02 and R95CR-04, before the publication: a request killed once its
/// record is written changed nothing — and a person who then commits
/// something else of their own does not make it look published. The next
/// pass drops the record, and nothing of the request is anywhere.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_cut_off_before_its_publication_changed_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Recorded);
    assert!(stopped.is_err());
    assert!(record(&root).exists(), "the kill left its record");
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(status(&root).is_empty(), "{:?}", status(&root));

    std::fs::write(root.join("unrelated.md"), "mine").expect("a person's file");
    git_out(&root, &["add", "unrelated.md"]);
    git_out(
        &root,
        &[
            "-c",
            "user.name=p",
            "-c",
            "user.email=p@e",
            "commit",
            "-qm",
            "mine",
        ],
    );
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    untouched(&root, &theirs, "a person's commit after the kill");
    assert_eq!(
        git_out(&root, &["log", "-1", "--format=%s"]).trim(),
        "mine",
        "nothing of the request was committed"
    );
}

/// R95CR-02 and R95CR-04: a request cut off before its publication is
/// dropped, never rolled forward — even once a person committed the very
/// text it would have written and then took it back on the disk: the
/// person's commit is not the request's, and their file stays as they left
/// it.
#[tokio::test(flavor = "multi_thread")]
async fn an_unpublished_request_never_writes_over_a_persons_own_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Recorded);
    assert!(stopped.is_err());
    assert!(record(&root).exists(), "the kill left its record");

    std::fs::write(root.join("MEMORY.md"), "a\n§\nb").expect("a person's edit");
    git_out(&root, &["add", "MEMORY.md"]);
    git_out(
        &root,
        &[
            "-c",
            "user.name=p",
            "-c",
            "user.email=p@e",
            "commit",
            "-qm",
            "mine",
        ],
    );
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("MEMORY.md"), "a").expect("taken back");

    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert!(!record(&root).exists(), "the record is gone");
    assert_eq!(read(&root, "MEMORY.md"), "a", "the person's file stays");
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    assert!(root.join("proposals/01P.md").exists());
    assert!(!root.join("proposals/done").exists());
}

/// R95C-10, R95CR-04 and R95CR-05: a request killed right after its branch
/// moved is finished by the next pass, never undone — its proposal stays
/// moved and its verdict written — while what a person did meanwhile is
/// theirs: an edit staged and saved to a file it changes stays staged and
/// on the disk.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_cut_off_after_its_publication_is_finished_never_undone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Published);
    assert!(stopped.is_err());
    let commit = git_out(&root, &["rev-parse", "HEAD"]);
    assert!(
        git_out(&root, &["log", "-1", "--format=%s"]).starts_with("memory: nixi"),
        "published"
    );
    assert_eq!(read(&root, "MEMORY.md"), "a", "nothing followed it yet");

    std::fs::write(root.join("MEMORY.md"), "typed by a person").expect("edit");
    git_out(&root, &["add", "MEMORY.md"]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert!(!record(&root).exists(), "settled");
    assert_eq!(
        git_out(&root, &["diff", "--cached", "--name-only"]),
        "MEMORY.md\n",
        "the person's staging stays staged"
    );
    assert!(!root.join("proposals/01P.md").exists(), "still moved");
    assert_eq!(read(&root, "proposals/done/01P.md"), "proposal");
    assert!(root.join("proposals/done/01P.verdict.toml").exists());
    assert_eq!(read(&root, "MEMORY.md"), "typed by a person");
    assert!(
        git_out(&root, &["log", "--format=%H"]).contains(commit.trim()),
        "the commit stands"
    );
    for path in ["proposals/done/01P.md", "proposals/done/01P.verdict.toml"] {
        git_out(&root, &["cat-file", "-e", &format!("HEAD:{path}")]);
    }
    assert!(
        git_out(&root, &["ls-tree", "HEAD", "proposals/01P.md"]).is_empty(),
        "never put back"
    );
}

/// R207: the branch moved by someone else after the commit was built is
/// never overwritten — the compare-and-swap refuses, the record goes, and
/// nothing of the request is anywhere.
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_moved_before_the_publication_is_never_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let theirs = |at: Cut| {
        if at == Cut::Recorded {
            std::fs::write(root.join("unrelated.md"), "mine").expect("a person's file");
            git_out(&root, &["add", "unrelated.md"]);
            git_out(
                &root,
                &[
                    "-c",
                    "user.name=p",
                    "-c",
                    "user.email=p@e",
                    "commit",
                    "-qm",
                    "mine",
                ],
            );
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &theirs)
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "HEAD".to_owned()
        }
    );
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    assert_eq!(git_out(&root, &["log", "-1", "--format=%s"]).trim(), "mine");
    untouched(&root, &head, "the branch moved");
}

/// R95CR-07: a person who saves a file in the instant between its old
/// version being moved aside and the new one placed keeps their save: the
/// new one is not placed over it, the old one goes, and the commit stands.
#[tokio::test(flavor = "multi_thread")]
async fn a_save_at_the_replacement_is_never_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let memory = root.join("MEMORY.md");
    let save = |at: Cut| {
        if at == Cut::Displaced(MEMORY_CHANGE) {
            std::fs::write(&memory, "saved at that instant").expect("save");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &save)
        .expect("committed");
    let CommitPaths::Committed { commit } = done else {
        panic!("{done:?}");
    };
    assert_eq!(
        git_out(&root, &["show", &format!("{commit}:MEMORY.md")]),
        "a\n§\nb"
    );
    assert_eq!(read(&root, "MEMORY.md"), "saved at that instant");
    let leftovers: Vec<String> = std::fs::read_dir(&root)
        .expect("list")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(".keeper-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(status(&root), [" M MEMORY.md"]);
}

/// R95CR-03 and R95CR-05: a published commit whose files could not all be
/// written is reported committed — it was — and its record stays: the card
/// says so, and the next pass finishes it, the file moved aside by the cut
/// included. A record that does not read holds every commit of the folder,
/// said on its card, until it is gone.
#[tokio::test(flavor = "multi_thread")]
async fn an_unfinished_commit_holds_the_folder_until_it_is_finished() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &|at| {
            at != Cut::Displaced(MEMORY_CHANGE)
        })
        .expect("published");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert!(record(&root).exists(), "unfinished, so kept");
    assert!(!root.join("MEMORY.md").exists(), "moved aside by the cut");
    assert!(engine
        .status(&p.id)
        .expect("status")
        .warning
        .is_some_and(|warning| warning.contains("commits nothing else")));
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    finished(&root, "finished by the next pass");

    std::fs::write(record(&root), "{ not a record").expect("a broken record");
    std::fs::write(root.join("new.md"), "new").expect("a person's file");
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    platform.advance_ms(60_000);
    let _ = engine.tick_profile(&p).await;
    let _ = engine.sync_once(&p.id, SyncSource::Manual).await;
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head, "held");
    assert!(engine
        .status(&p.id)
        .expect("status")
        .warning
        .is_some_and(|warning| warning.contains("does not read")));
    std::fs::remove_file(record(&root)).expect("checked and removed");
    assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
}

/// R95CR-08 and R95C3-09: the folder's root replaced by a link once the
/// request held it redirects nothing — nothing lands outside it, and
/// nothing of the repository is read or written through the link: the
/// record stays, and the next pass in the folder checked finishes it. A
/// folder on the way swapped for a link after the publication refuses that
/// path, keeps the record, and lands nothing outside either.
#[tokio::test(flavor = "multi_thread")]
async fn a_root_or_folder_swapped_for_a_link_redirects_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let moved = dir.path().join("the-real-folder");
    let swap = |at: Cut| {
        if at == Cut::Published {
            std::fs::rename(&root, &moved).expect("move the root");
            std::os::unix::fs::symlink(outside.path(), &root).expect("link");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &swap)
        .expect("published");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert_eq!(std::fs::read_dir(outside.path()).expect("list").count(), 0);
    assert!(record(&moved).exists(), "held in the folder checked");
    std::fs::remove_file(&root).expect("unlink");
    std::fs::rename(&moved, &root).expect("back");
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    finished(&root, "finished once the folder was back");

    let ask = {
        std::fs::create_dir_all(root.join("proposals")).expect("proposals");
        std::fs::write(root.join("proposals/02P.md"), "second").expect("proposal");
        commit_after_settling(&engine, &platform, &p);
        let mut ask = request("a\n§\nb\n§\nc", Some(blob_id("a\n§\nb".as_bytes())));
        ask.guards
            .push(("proposals/02P.md".to_owned(), Some(blob_id(b"second"))));
        ask.moves = vec![(
            "proposals/02P.md".to_owned(),
            "proposals/done/02P.md".to_owned(),
        )];
        ask
    };
    let swap = |at: Cut| {
        if at == Cut::Published {
            std::fs::rename(root.join("proposals"), root.join("elsewhere")).expect("move");
            std::os::unix::fs::symlink(outside.path(), root.join("proposals")).expect("link");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &swap)
        .expect("published");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert_eq!(std::fs::read_dir(outside.path()).expect("list").count(), 0);
    assert!(record(&root).exists(), "refused there, kept");
}

/// R95C-13: what the index stages beside the request — an addition, a
/// modification, a deletion — is not in its commit, and is still staged
/// after it.
#[tokio::test(flavor = "multi_thread")]
async fn an_authored_commit_holds_only_its_own_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::write(root.join("extra.md"), "extra").expect("extra");
    commit_after_settling(&engine, &platform, &p);
    std::fs::write(root.join("staged.md"), "staged").expect("staged");
    std::fs::write(root.join("root.md"), "root, staged").expect("root");
    git_out(&root, &["add", "staged.md", "root.md"]);
    git_out(&root, &["rm", "-q", "extra.md"]);
    let staged = git_out(&root, &["diff", "--cached", "--name-status"]);
    let done = engine
        .commit_paths(&p.id, &request("a\n§\nb", Some(blob_id(b"a"))), yes())
        .await
        .expect("committed");
    let CommitPaths::Committed { commit } = done else {
        panic!("{done:?}");
    };
    assert_eq!(
        git_out(&root, &["show", "--name-status", "--format=", &commit]),
        "M\tMEMORY.md\n"
    );
    assert_eq!(
        git_out(&root, &["diff", "--cached", "--name-status"]),
        staged
    );
}

/// R95C-18's last safeguard: a request whose paths overlap — a skill's
/// file written and its folder moved — is refused before any effect.
#[tokio::test(flavor = "multi_thread")]
async fn overlapping_paths_are_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::create_dir_all(root.join("_skills/x")).expect("skill");
    std::fs::write(root.join("_skills/x/SKILL.md"), "skill").expect("skill");
    commit_after_settling(&engine, &platform, &p);
    let ask = CommitRequest {
        writes: vec![("_skills/x/SKILL.md".to_owned(), Some(b"patched".to_vec()))],
        moves: vec![("_skills/x".to_owned(), "_skills/.archive/x".to_owned())],
        guards: vec![("_skills/x/SKILL.md".to_owned(), Some(blob_id(b"skill")))],
        ..CommitRequest::default()
    };
    assert!(engine.commit_paths(&p.id, &ask, yes()).await.is_err());
    assert_eq!(read(&root, "_skills/x/SKILL.md"), "skill");
    assert!(!root.join("_skills/.archive").exists());
}

/// R208 (R95U-07): a folder moves only whole as the request read it —
/// a file under it the request names no guard for (one added since the
/// plan) moves nothing; guarded file by file, it moves, its destination
/// guarded absent.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_takes_only_the_files_it_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::create_dir_all(root.join("_skills/x/scripts")).expect("skill");
    std::fs::write(root.join("_skills/x/SKILL.md"), "skill").expect("skill");
    std::fs::write(root.join("_skills/x/scripts/run.sh"), "run").expect("script");
    commit_after_settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let mut ask = CommitRequest {
        moves: vec![("_skills/x".to_owned(), "_skills/.archive/x".to_owned())],
        guards: vec![
            ("_skills/x/SKILL.md".to_owned(), Some(blob_id(b"skill"))),
            ("_skills/.archive/x".to_owned(), None),
        ],
        subject: "skills".to_owned(),
        ..CommitRequest::default()
    };
    assert_eq!(
        engine.commit_paths(&p.id, &ask, yes()).await.expect("asks"),
        CommitPaths::Guarded {
            path: "_skills/x/scripts/run.sh".to_owned()
        }
    );
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(read(&root, "_skills/x/scripts/run.sh"), "run");
    ask.guards
        .push(("_skills/x/scripts/run.sh".to_owned(), Some(blob_id(b"run"))));
    assert!(matches!(
        engine.commit_paths(&p.id, &ask, yes()).await.expect("asks"),
        CommitPaths::Committed { .. }
    ));
    assert_eq!(read(&root, "_skills/.archive/x/scripts/run.sh"), "run");
    assert!(!root.join("_skills/x").exists());
}

/// R95U2-03: a moved folder must hold on the disk exactly what the commit
/// holds under it — a file put there and never committed, before the
/// guards are read or after, right before the publication, holds the
/// move: nothing is published, nothing is moved, the new file stays, and
/// no record is left to settle. One there before is refused with the
/// guards, so a process killed once a record would be written leaves none
/// holding the folder's commits.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_holds_for_a_file_on_the_disk_it_never_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::create_dir_all(root.join("_skills/x")).expect("skill");
    std::fs::write(root.join("_skills/x/SKILL.md"), "skill").expect("skill");
    commit_after_settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let ask = CommitRequest {
        moves: vec![("_skills/x".to_owned(), "_skills/.archive/x".to_owned())],
        guards: vec![
            ("_skills/x/SKILL.md".to_owned(), Some(blob_id(b"skill"))),
            ("_skills/.archive/x".to_owned(), None),
        ],
        subject: "skills".to_owned(),
        ..CommitRequest::default()
    };
    let new = root.join("_skills/x/references/new.md");
    let stays = |context: &str| {
        assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head, "{context}");
        assert_eq!(
            read(&root, "_skills/x/references/new.md"),
            "mine",
            "{context}"
        );
        assert_eq!(read(&root, "_skills/x/SKILL.md"), "skill", "{context}");
        assert!(!root.join("_skills/.archive").exists(), "{context}");
        std::fs::remove_dir_all(root.join("_skills/x/references")).expect("rm");
    };
    let guarded = CommitPaths::Guarded {
        path: "_skills/x/references/new.md".to_owned(),
    };

    std::fs::create_dir_all(new.parent().expect("parent")).expect("mkdir");
    std::fs::write(&new, "mine").expect("new");
    assert_eq!(
        engine.commit_paths(&p.id, &ask, yes()).await.expect("asks"),
        guarded
    );
    assert!(!record(&root).exists(), "there before: no record");
    stays("there before");

    std::fs::create_dir_all(new.parent().expect("parent")).expect("mkdir");
    std::fs::write(&new, "mine").expect("new");
    let killed = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Recorded);
    assert!(
        !engine.unsettled_commit(&p.id).expect("unsettled"),
        "there before, killed at the record: nothing to settle"
    );
    assert_eq!(killed.expect("refused with the guards"), guarded);
    stays("there before, killed at the record");

    let add = |at: Cut| {
        if at == Cut::Prepared {
            std::fs::create_dir_all(new.parent().expect("parent")).expect("mkdir");
            std::fs::write(&new, "mine").expect("new");
        }
        true
    };
    assert_eq!(
        engine
            .commit_paths_held(&p, &ask, &|| true, &add)
            .expect("asks"),
        guarded
    );
    stays("put there right before the publication");

    assert!(matches!(
        engine.commit_paths(&p.id, &ask, yes()).await.expect("asks"),
        CommitPaths::Committed { .. }
    ));
    assert_eq!(read(&root, "_skills/.archive/x/SKILL.md"), "skill");
}

/// R95C-04, R95CR-06 and R95C3-01 at the publication: a fence that says no
/// once the lane is held changes nothing; nor does a lease lost while the
/// request's bytes are read and its guards read again — the fence is asked
/// after all of that, right before the branch moves: no commit, no file,
/// no index entry, no record.
#[tokio::test(flavor = "multi_thread")]
async fn a_lease_lost_before_the_branch_moves_moves_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let done = engine
        .commit_paths(&p.id, &ask, Arc::new(|| false))
        .await
        .expect("fenced");
    assert_eq!(done, CommitPaths::Fenced);
    untouched(&root, &head, "fenced at once");

    let lost = AtomicBool::new(false);
    let done = engine
        .commit_paths_held(&p, &ask, &|| !lost.load(Ordering::SeqCst), &|at| {
            if at == Cut::Prepared {
                lost.store(true, Ordering::SeqCst);
            }
            true
        })
        .expect("fenced");
    assert_eq!(done, CommitPaths::Fenced);
    untouched(&root, &head, "the lease lost while the bytes were read");
}

/// R95CR-11 at the engine: a guard-only path — a declaration the plan read
/// — changed on the disk after the guards were checked and before the
/// publication holds the commit: nothing changes.
#[tokio::test(flavor = "multi_thread")]
async fn a_declaration_changed_before_the_publication_holds_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::write(root.join("_drive.toml"), "readers = [\"a\"]\n").expect("declaration");
    let mut ask = settling(&engine, &platform, &p);
    ask.guards.push((
        "_drive.toml".to_owned(),
        Some(blob_id(b"readers = [\"a\"]\n")),
    ));
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let widen = |at: Cut| {
        if at == Cut::Recorded {
            std::fs::write(root.join("_drive.toml"), "readers = [\"a\", \"b\"]\n")
                .expect("widened");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &widen)
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "_drive.toml".to_owned()
        }
    );
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(root.join("proposals/01P.md").exists());
    assert!(!record(&root).exists());
}

/// R95CR-06: the request's git work runs off the async executor, so what
/// runs beside it on the caller's task — a lease's renewal — goes on while
/// it works: a fence that waits for its sibling to run is answered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_commit_leaves_its_callers_task_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let renewed = Arc::new(AtomicBool::new(false));
    let fence: CommitFence = {
        let renewed = Arc::clone(&renewed);
        Arc::new(move || {
            let waited = std::time::Instant::now();
            while !renewed.load(Ordering::SeqCst) {
                if waited.elapsed() > std::time::Duration::from_secs(10) {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            true
        })
    };
    let ask = request("a\n§\nb", Some(blob_id(b"a")));
    let (done, ()) = tokio::join!(engine.commit_paths(&p.id, &ask, fence), async {
        renewed.store(true, Ordering::SeqCst);
    });
    assert!(
        matches!(done.expect("ran"), CommitPaths::Committed { .. }),
        "the sibling ran while the commit worked"
    );
}

/// R95CR-09: a write LFS routes is committed as its pointer, with the rule
/// it needs added to `HEAD`'s attributes in the same commit, and the disk
/// gets the bytes and the rule; a person's unsaved edit to the attributes
/// holds such a request instead of riding it; a request that routes nothing
/// leaves the attributes alone.
#[tokio::test(flavor = "multi_thread")]
async fn routed_writes_carry_their_rule_and_nothing_else() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, mut p)) = with_memory(dir.path(), remote_dir.path(), "a").await
    else {
        return;
    };
    p.lfs_threshold_bytes = 1024;
    engine.upsert_profile(&p).expect("upsert");
    let root = p.local_path.clone();
    let big = vec![b'x'; 4096];
    let ask = CommitRequest {
        writes: vec![("clip.bin".to_owned(), Some(big.clone()))],
        guards: vec![("clip.bin".to_owned(), None)],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };

    std::fs::write(root.join(".gitattributes"), "# a person's note\n").expect("unsaved");
    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: ".gitattributes".to_owned()
        }
    );
    assert!(!root.join("clip.bin").exists());
    std::fs::remove_file(root.join(".gitattributes")).expect("undo");

    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("committed");
    let CommitPaths::Committed { commit } = done else {
        panic!("{done:?}");
    };
    let pointer = git_out(&root, &["show", &format!("{commit}:clip.bin")]);
    assert!(pointer.starts_with("version https://git-lfs"), "{pointer}");
    let rule = git_out(&root, &["show", &format!("{commit}:.gitattributes")]);
    assert!(rule.contains("*.bin filter=lfs"), "{rule}");
    assert_eq!(std::fs::read(root.join("clip.bin")).expect("bytes"), big);
    assert_eq!(read(&root, ".gitattributes"), rule);

    let done = engine
        .commit_paths(&p.id, &request("a\n§\nb", Some(blob_id(b"a"))), yes())
        .await
        .expect("committed");
    let CommitPaths::Committed { commit } = done else {
        panic!("{done:?}");
    };
    assert_eq!(
        git_out(&root, &["show", "--name-status", "--format=", &commit]),
        "M\tMEMORY.md\n",
        "nothing routed: the attributes are not part of it"
    );
}

/// `git <args>` in `dir` as a person whose clock says `date`: its output.
fn git_dated(dir: &Path, date: &str, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=p", "-c", "user.email=p@e"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).expect("utf-8")
}

/// A person's commit of what the index holds.
fn commit_as_person(root: &Path, message: &str) {
    git_out(
        root,
        &[
            "-c",
            "user.name=p",
            "-c",
            "user.email=p@e",
            "commit",
            "-qm",
            message,
        ],
    );
}

/// Whether a person's `git <args>` in `dir` went through.
fn git_ok(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs")
        .status
        .success()
}

/// Every file under `root` whose name says it is the engine's own staging
/// or moved-aside file, `.git` aside.
fn engine_leftovers(root: &Path) -> Vec<String> {
    fn walk(at: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(at).expect("list") {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
            if entry.file_type().expect("type").is_dir() {
                walk(&entry.path(), out);
            } else if name.starts_with(".keeper") {
                out.push(entry.path().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// A folder a settling holds: its record stays, it is said to be
/// unsettled, and a watcher pass over it commits nothing — `HEAD` stays
/// `head`.
fn held(engine: &Engine, platform: &TestPlatform, p: &SyncProfile, head: &str) {
    let root = &p.local_path;
    assert!(engine.settle_commit_paths(p).is_err(), "the settling holds");
    assert!(record(root).exists(), "the record stays");
    assert!(engine.unsettled_commit(&p.id).expect("asked"));
    let first = engine.commit_local(p, SyncSource::Watch, None);
    platform.advance_ms(p.effective_settle_ms() as i64 + 1);
    let second = engine.commit_local(p, SyncSource::Watch, None);
    assert!(
        !matches!(first, Ok(n) if n > 0) && second.is_err(),
        "no pass commits, and the one that would is refused: {first:?} {second:?}"
    );
    assert_eq!(git_out(root, &["rev-parse", "HEAD"]), head);
    assert!(record(root).exists(), "still held");
}

/// The staging name a kill at [`Staging::Linked`] left, removed — as a
/// kill right after the placement leaves it.
fn drop_staging_name(root: &Path) {
    let staged: Vec<String> = engine_leftovers(root)
        .into_iter()
        .filter(|path| path.ends_with(".tmp"))
        .collect();
    assert_eq!(staged.len(), 1, "{staged:?}");
    std::fs::remove_file(&staged[0]).expect("the staging name gone");
}

/// `text` committed by a person at `rel` through the index alone, as
/// `mode`: the disk is not touched.
fn commit_through_the_index(root: &Path, scratch: &Path, rel: &str, mode: &str, text: &str) {
    let file = scratch.join("version");
    std::fs::write(&file, text).expect("a version");
    let blob = git_out(root, &["hash-object", "-w", &file.to_string_lossy()]);
    let entry = format!("{mode},{},{rel}", blob.trim());
    assert!(git_ok(
        root,
        &["update-index", "--add", "--cacheinfo", &entry]
    ));
    commit_as_person(root, "a version through the index");
}

/// R95C3-03: the request is checked against one commit, read once. A
/// person's commit made after the checks and before the commit is built —
/// a declaration widened in the index and committed, its old bytes back on
/// the disk — is never taken for the base: nothing is published over it.
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_made_after_the_checks_is_never_built_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let declaration = "readers = [\"a\"]\n";
    let widened = "readers = [\"a\", \"b\"]\n";
    std::fs::write(root.join("_drive.toml"), declaration).expect("declaration");
    let mut ask = settling(&engine, &platform, &p);
    ask.guards.push((
        "_drive.toml".to_owned(),
        Some(blob_id(declaration.as_bytes())),
    ));
    let theirs = |at: Cut| {
        if at == Cut::Checked {
            std::fs::write(root.join("_drive.toml"), widened).expect("widened");
            git_out(&root, &["add", "_drive.toml"]);
            commit_as_person(&root, "widened");
            std::fs::write(root.join("_drive.toml"), declaration).expect("old bytes back");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &theirs)
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "HEAD".to_owned()
        }
    );
    assert_eq!(
        git_out(&root, &["log", "-1", "--format=%s"]).trim(),
        "widened"
    );
    assert_eq!(git_out(&root, &["show", "HEAD:_drive.toml"]), widened);
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(root.join("proposals/01P.md").exists());
    assert!(!root.join("proposals/done").exists());
    assert!(!record(&root).exists());
}

/// R95C3-04: a person who commits right after the publication — the index
/// and the disk still the parent's, so their commit takes all of it back —
/// is never written over: neither the request going on nor its settling
/// after a kill writes the approved bytes again, and a watcher pass after
/// either commits nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_commit_right_after_the_publication_is_never_written_over() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let take_back = |at: Cut| {
        if at == Cut::Published {
            commit_as_person(&root, "taken back");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &take_back)
        .expect("published");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    untouched(&root, &theirs, "taken back as the request went on");
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);

    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Published);
    assert!(stopped.is_err());
    commit_as_person(&root, "taken back again");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    untouched(&root, &theirs, "taken back before the settling");
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
}

/// R95C3-05: whether a cut-off commit was published is read from the
/// commit graph, never from commit dates — a person's child dated years
/// before it, or a merge dated so whose second parent it is, still makes it
/// published, and it is finished; an intended commit that cannot be read
/// is not known to be either, and its record stays and holds the folder.
#[tokio::test(flavor = "multi_thread")]
async fn a_published_commit_is_known_by_its_ancestry_not_its_date() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let long_ago = "2001-01-01T00:00:00Z";
    let ask = settling(&engine, &platform, &p);
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Published);
    assert!(stopped.is_err());
    // A child of the published commit holding its tree, its clock years
    // behind.
    let child = git_dated(
        &root,
        long_ago,
        &[
            "commit-tree",
            "HEAD^{tree}",
            "-p",
            "HEAD",
            "-m",
            "dated long ago",
        ],
    );
    git_out(&root, &["update-ref", "HEAD", child.trim()]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    finished(&root, "under a child dated before it");

    std::fs::write(root.join("proposals/02P.md"), "second").expect("proposal");
    commit_after_settling(&engine, &platform, &p);
    let mut ask = request("a\n§\nb\n§\nc", Some(blob_id("a\n§\nb".as_bytes())));
    ask.guards
        .push(("proposals/02P.md".to_owned(), Some(blob_id(b"second"))));
    ask.moves = vec![(
        "proposals/02P.md".to_owned(),
        "proposals/done/02P.md".to_owned(),
    )];
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Published);
    assert!(stopped.is_err());
    let side = git_dated(
        &root,
        long_ago,
        &["commit-tree", "HEAD~1^{tree}", "-p", "HEAD~1", "-m", "side"],
    );
    let merge = git_dated(
        &root,
        long_ago,
        &[
            "commit-tree",
            "HEAD^{tree}",
            "-p",
            side.trim(),
            "-p",
            "HEAD",
            "-m",
            "merged long ago",
        ],
    );
    git_out(&root, &["update-ref", "HEAD", merge.trim()]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb\n§\nc");
    assert_eq!(read(&root, "proposals/done/02P.md"), "second");
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert!(!record(&root).exists());

    let ask = request(
        "a\n§\nb\n§\nc\n§\nd",
        Some(blob_id("a\n§\nb\n§\nc".as_bytes())),
    );
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Recorded);
    assert!(stopped.is_err());
    let intent: serde_json::Value =
        serde_json::from_str(&read(&root, ".git/keeper-commit-paths.json")).expect("record");
    let commit = intent["commit"].as_str().expect("commit");
    std::fs::remove_file(root.join(format!(".git/objects/{}/{}", &commit[..2], &commit[2..])))
        .expect("the commit made unreadable");
    assert!(engine.settle_commit_paths(&p).is_err(), "not known");
    assert!(record(&root).exists(), "kept");
    assert!(engine
        .status(&p.id)
        .expect("status")
        .warning
        .is_some_and(|warning| warning.contains("not known")));
}

/// R95C3-09: the folder swapped for another checkout of the same commit
/// before the request opens its repository publishes nothing: not in the
/// checkout swapped in — its branch and its `.git` untouched — and not in
/// the folder held.
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_swapped_before_its_repository_opens_publishes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let other = dir.path().join("another-checkout");
    git_out(
        dir.path(),
        &[
            "clone",
            "-q",
            &root.to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    let held = dir.path().join("the-folder-held");
    let swap = |at: Cut| {
        if at == Cut::Held {
            std::fs::rename(&root, &held).expect("move the folder");
            std::fs::rename(&other, &root).expect("another checkout in its place");
        }
        true
    };
    assert!(engine.commit_paths_held(&p, &ask, &|| true, &swap).is_err());
    for (folder, context) in [(&root, "swapped in"), (&held, "held")] {
        assert_eq!(
            git_out(folder, &["rev-parse", "HEAD"]),
            head,
            "{context}: HEAD"
        );
        assert!(!record(folder).exists(), "{context}: no record");
        assert_eq!(read(folder, "MEMORY.md"), "a", "{context}");
        assert!(status(folder).is_empty(), "{context}: {:?}", status(folder));
    }
}

/// R95C4-01: the folder held keeps its own `.git`. Another `.git` put in
/// its place inside the same folder — a copy holding the same commit and
/// every object the request needs — after the request read its base, or
/// after it recorded its commit, gets no record and no branch move, and
/// the `.git` the request began with moves no branch either.
#[tokio::test(flavor = "multi_thread")]
async fn a_git_folder_replaced_inside_the_folder_publishes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    for (n, cut) in [Cut::Checked, Cut::Prepared].into_iter().enumerate() {
        let held = dir.path().join(format!("the-git-held-{n}"));
        let swap = |at: Cut| {
            if at == cut {
                std::fs::rename(root.join(".git"), &held).expect("move .git");
                let copied = std::process::Command::new("cp")
                    .args(["-a"])
                    .arg(&held)
                    .arg(root.join(".git"))
                    .status()
                    .expect("cp runs");
                assert!(copied.success(), "another .git in its place");
            }
            true
        };
        assert!(
            engine.commit_paths_held(&p, &ask, &|| true, &swap).is_err(),
            "{cut:?}"
        );
        let held_head = git_out(
            dir.path(),
            &["--git-dir", &held.to_string_lossy(), "rev-parse", "HEAD"],
        );
        assert_eq!(held_head, head, "{cut:?}: the .git held");
        assert_eq!(
            git_out(&root, &["rev-parse", "HEAD"]),
            head,
            "{cut:?}: the .git put in its place"
        );
        if cut == Cut::Checked {
            assert!(!record(&root).exists(), "{cut:?}: no record");
        }
        assert_eq!(read(&root, "MEMORY.md"), "a", "{cut:?}");
        assert!(root.join("proposals/01P.md").exists(), "{cut:?}");
    }
}

/// R95C5-01: a refused request drops its record only from the `.git` it
/// began with. Another `.git` put in its place right before the last
/// checks, holding another request's record, while a guarded file is
/// saved too: the guard refuses, the record in the `.git` put in place is
/// left as it was, the request's own stays in the `.git` it began with,
/// and neither moves a branch.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_never_drops_another_gits_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = settling(&engine, &platform, &p);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    let held = dir.path().join("the-git-held");
    let another = b"another request's record";
    let swap = |at: Cut| {
        if at == Cut::Prepared {
            std::fs::rename(root.join(".git"), &held).expect("move .git");
            let copied = std::process::Command::new("cp")
                .args(["-a"])
                .arg(&held)
                .arg(root.join(".git"))
                .status()
                .expect("cp runs");
            assert!(copied.success(), "another .git in its place");
            std::fs::write(record(&root), another).expect("another record");
            std::fs::write(root.join("MEMORY.md"), "mine").expect("a person's save");
        }
        true
    };
    assert!(engine.commit_paths_held(&p, &ask, &|| true, &swap).is_err());
    assert_eq!(
        std::fs::read(record(&root)).expect("the record put in place"),
        another
    );
    assert!(
        held.join("keeper-commit-paths.json").exists(),
        "the request's own record stays where it began"
    );
    let held_head = git_out(
        dir.path(),
        &["--git-dir", &held.to_string_lossy(), "rev-parse", "HEAD"],
    );
    assert_eq!(held_head, head, "the .git held");
    assert_eq!(
        git_out(&root, &["rev-parse", "HEAD"]),
        head,
        "the .git put in place"
    );
    assert_eq!(read(&root, "MEMORY.md"), "mine");
    assert!(root.join("proposals/01P.md").exists());
}

/// R95C3-10: what a person stages while the index follows a commit is
/// never lost — the index is held under its own lock from its read to its
/// write, so a `git add` meanwhile is refused rather than written over,
/// whether it stages a path of the commit or another — and staged again,
/// it stays.
#[tokio::test(flavor = "multi_thread")]
async fn staging_while_the_index_follows_is_never_lost() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    std::fs::write(root.join("unrelated.md"), "theirs").expect("a person's file");
    let paths = ["unrelated.md", "MEMORY.md"];
    let added = std::sync::Mutex::new(Vec::new());
    let stage = |at: Cut| {
        if at == Cut::Indexed {
            std::fs::write(root.join("MEMORY.md"), "typed meanwhile").expect("typed");
            for path in paths {
                let went = git_ok(&root, &["add", path]);
                added.lock().expect("lock").push((path, went));
            }
        }
        true
    };
    let done = engine
        .commit_paths_held(
            &p,
            &request("a\n§\nb", Some(blob_id(b"a"))),
            &|| true,
            &stage,
        )
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    let added = added.into_inner().expect("lock");
    assert_eq!(added.len(), 2, "the person staged at that instant");
    let staged = git_out(&root, &["diff", "--cached", "--name-only"]);
    for (path, went) in added {
        assert!(
            !went || staged.lines().any(|line| line == path),
            "{path} was staged and stays: {staged}"
        );
    }
    for path in paths {
        git_out(&root, &["add", path]);
    }
    let staged = git_out(&root, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged, "MEMORY.md\nunrelated.md\n");
}

/// R95C3-11: a request rewriting several files the index holds, an early
/// one first, has every one of them follow in the index: each is decided
/// against the index as it was read, sorted, before any entry moves.
#[tokio::test(flavor = "multi_thread")]
async fn every_rewritten_file_follows_in_the_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    for name in ["a.md", "b.md", "c.md", "d.md", "z.md"] {
        std::fs::write(root.join(name), format!("old {name}")).expect("file");
    }
    commit_after_settling(&engine, &platform, &p);
    let mut ask = CommitRequest {
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    for name in ["a.md", "z.md", "c.md"] {
        ask.writes
            .push((name.to_owned(), Some(format!("new {name}").into_bytes())));
        ask.guards.push((
            name.to_owned(),
            Some(blob_id(format!("old {name}").as_bytes())),
        ));
    }
    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(git_out(&root, &["diff", "--cached", "--name-only"]), "");
}

/// R95C3-12: the attributes a routed write needs are one of the commit's
/// own paths: a request moving a file into `.gitattributes`, or moving
/// `.gitattributes` away, beside a write LFS routes is refused before any
/// effect — no commit, no record, nothing on the disk or in the index.
#[tokio::test(flavor = "multi_thread")]
async fn the_attributes_a_routed_write_needs_are_no_moved_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, mut p)) = with_memory(dir.path(), remote_dir.path(), "a").await
    else {
        return;
    };
    p.lfs_threshold_bytes = 1024;
    engine.upsert_profile(&p).expect("upsert");
    let root = p.local_path.clone();
    std::fs::write(root.join("notes.txt"), "notes").expect("notes");
    git_out(&root, &["add", "notes.txt"]);
    commit_as_person(&root, "notes");
    // Every moved file guarded, as a move must be: what is refused is the
    // attributes' place, not an unread file.
    let routed = |moves: Vec<(String, String)>| CommitRequest {
        writes: vec![("clip.bin".to_owned(), Some(vec![b'x'; 4096]))],
        guards: std::iter::once(("clip.bin".to_owned(), None))
            .chain(moves.iter().map(|(from, _)| {
                let bytes = std::fs::read(root.join(from)).expect("moved file");
                (from.clone(), Some(blob_id(&bytes)))
            }))
            .collect(),
        moves,
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    let refused = |ask: CommitRequest, context: &str| {
        let head = git_out(&root, &["rev-parse", "HEAD"]);
        let done = engine.commit_paths_held(&p, &ask, &|| true, &|_| true);
        assert!(done.is_err(), "{context}: {done:?}");
        assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head, "{context}");
        assert!(!record(&root).exists(), "{context}");
        assert!(!root.join("clip.bin").exists(), "{context}");
        assert!(status(&root).is_empty(), "{context}: {:?}", status(&root));
    };
    refused(
        routed(vec![("notes.txt".to_owned(), ".gitattributes".to_owned())]),
        "moved into the attributes",
    );
    assert_eq!(read(&root, "notes.txt"), "notes");
    assert!(!root.join(".gitattributes").exists());

    std::fs::write(root.join(".gitattributes"), "# rules\n").expect("attributes");
    git_out(&root, &["add", ".gitattributes"]);
    commit_as_person(&root, "rules");
    refused(
        routed(vec![(
            ".gitattributes".to_owned(),
            "attributes.bak".to_owned(),
        )]),
        "the attributes moved away",
    );
    assert_eq!(read(&root, ".gitattributes"), "# rules\n");
    assert!(!root.join("attributes.bak").exists());
}

/// R95C3-13: a file the commit records executable is placed executable —
/// moved with its skill or rewritten, by the request or by its settling
/// after a kill — so the folder says what the commit says, and a watcher
/// pass after it commits no permission change.
#[tokio::test(flavor = "multi_thread")]
async fn executable_files_stay_executable_on_the_disk() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let executable = |rel: &str| {
        std::fs::metadata(root.join(rel))
            .expect("file")
            .permissions()
            .mode()
            & 0o111
            != 0
    };
    let committed_executable =
        |rel: &str| git_out(&root, &["ls-tree", "HEAD", rel]).starts_with("100755 ");
    std::fs::create_dir_all(root.join("_skills/x")).expect("skill");
    for (rel, text) in [
        ("_skills/x/SKILL.md", "skill"),
        ("_skills/x/run.sh", "echo run"),
        ("tool.sh", "echo old"),
    ] {
        std::fs::write(root.join(rel), text).expect("file");
    }
    for rel in ["_skills/x/run.sh", "tool.sh"] {
        std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(0o755))
            .expect("executable");
    }
    git_out(&root, &["add", "-A"]);
    commit_as_person(&root, "a skill and a tool");
    assert!(committed_executable("tool.sh"));

    let ask = CommitRequest {
        writes: vec![("tool.sh".to_owned(), Some(b"echo new".to_vec()))],
        moves: vec![("_skills/x".to_owned(), "_skills/archive/x".to_owned())],
        guards: vec![
            ("tool.sh".to_owned(), Some(blob_id(b"echo old"))),
            ("_skills/x/SKILL.md".to_owned(), Some(blob_id(b"skill"))),
            ("_skills/x/run.sh".to_owned(), Some(blob_id(b"echo run"))),
        ],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    for rel in ["_skills/archive/x/run.sh", "tool.sh"] {
        assert!(committed_executable(rel), "{rel} at HEAD");
        assert!(executable(rel), "{rel} on the disk");
    }
    assert!(!executable("_skills/archive/x/SKILL.md"));
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);

    let ask = CommitRequest {
        writes: vec![("tool.sh".to_owned(), Some(b"echo newer".to_vec()))],
        guards: vec![("tool.sh".to_owned(), Some(blob_id(b"echo new")))],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    let stopped = engine.commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Published);
    assert!(stopped.is_err());
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert_eq!(read(&root, "tool.sh"), "echo newer");
    assert!(executable("tool.sh"), "settled after a kill");
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
}

/// R95C4-07: a person's own executable bit — the bytes left as committed —
/// is never undone: set or cleared before the request, or right after its
/// publication, the rewritten file takes it, by the request or by its
/// settling after a kill; a file a move takes away stays where it is with
/// it. A watcher pass after each commits the person's change and nothing
/// else.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_mode_change_is_never_undone() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let executable = |rel: &str| {
        std::fs::metadata(root.join(rel))
            .expect("file")
            .permissions()
            .mode()
            & 0o111
            != 0
    };
    let chmod = |rel: &str, mode: u32| {
        std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(mode))
            .expect("chmod");
    };
    let mode_at_head = |rel: &str| git_out(&root, &["ls-tree", "HEAD", rel])[..6].to_owned();
    let rewrite = |from: &str, to: &str| CommitRequest {
        writes: vec![("tool.sh".to_owned(), Some(to.as_bytes().to_vec()))],
        guards: vec![("tool.sh".to_owned(), Some(blob_id(from.as_bytes())))],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    std::fs::create_dir_all(root.join("_skills/x")).expect("skill");
    for (rel, text) in [
        ("_skills/x/SKILL.md", "skill"),
        ("_skills/x/run.sh", "echo run"),
        ("tool.sh", "echo 0"),
    ] {
        std::fs::write(root.join(rel), text).expect("file");
    }
    chmod("_skills/x/run.sh", 0o755);
    chmod("tool.sh", 0o755);
    git_out(&root, &["add", "-A"]);
    commit_as_person(&root, "a skill and a tool");

    // Cleared before the request.
    chmod("tool.sh", 0o644);
    let done = engine
        .commit_paths(&p.id, &rewrite("echo 0", "echo 1"), yes())
        .await
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert_eq!(read(&root, "tool.sh"), "echo 1");
    assert!(!executable("tool.sh"), "cleared before the request");
    commit_after_settling(&engine, &platform, &p);
    assert_eq!(mode_at_head("tool.sh"), "100644");
    assert_eq!(git_out(&root, &["show", "HEAD:tool.sh"]), "echo 1");
    assert!(status(&root).is_empty(), "{:?}", status(&root));

    // Set right after the publication.
    let set = |at: Cut| {
        if at == Cut::Published {
            chmod("tool.sh", 0o755);
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &rewrite("echo 1", "echo 2"), &|| true, &set)
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert_eq!(read(&root, "tool.sh"), "echo 2");
    assert!(executable("tool.sh"), "set after the publication");
    commit_after_settling(&engine, &platform, &p);
    assert_eq!(mode_at_head("tool.sh"), "100755");
    assert!(status(&root).is_empty(), "{:?}", status(&root));

    // Cleared before a request a kill stopped as it replaced the file.
    chmod("tool.sh", 0o644);
    let stopped = engine
        .commit_paths_held(&p, &rewrite("echo 2", "echo 3"), &|| true, &|at| {
            at != Cut::Displaced(0)
        })
        .expect("published");
    assert!(
        matches!(stopped, CommitPaths::Committed { .. }),
        "{stopped:?}"
    );
    assert!(record(&root).exists(), "cut off, kept");
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert_eq!(read(&root, "tool.sh"), "echo 3");
    assert!(!executable("tool.sh"), "settled after a kill");
    commit_after_settling(&engine, &platform, &p);
    assert_eq!(mode_at_head("tool.sh"), "100644");
    assert!(status(&root).is_empty(), "{:?}", status(&root));

    // Cleared right after a move's publication: that file stays.
    let cleared = |at: Cut| {
        if at == Cut::Published {
            chmod("_skills/x/run.sh", 0o644);
        }
        true
    };
    let ask = CommitRequest {
        moves: vec![("_skills/x".to_owned(), "_skills/archive/x".to_owned())],
        guards: vec![
            ("_skills/x/SKILL.md".to_owned(), Some(blob_id(b"skill"))),
            ("_skills/x/run.sh".to_owned(), Some(blob_id(b"echo run"))),
        ],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &cleared)
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert_eq!(read(&root, "_skills/x/run.sh"), "echo run");
    assert!(!executable("_skills/x/run.sh"), "the person's file stays");
    assert!(executable("_skills/archive/x/run.sh"));
    assert!(!root.join("_skills/x/SKILL.md").exists());
    commit_after_settling(&engine, &platform, &p);
    assert_eq!(mode_at_head("_skills/x/run.sh"), "100644");
    assert!(status(&root).is_empty(), "{:?}", status(&root));
}

/// R95C4-08: a commit a person took back after a kill stopped it with a
/// file moved aside gets that file back: the settling puts the old file in
/// its place, leaves nothing of its own, and a watcher pass after it
/// commits nothing. Where the person saved a file there meanwhile, theirs
/// stays and the old one — the committed bytes — goes.
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_taken_back_after_a_kill_gets_its_old_file_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = request("a\n§\nb", Some(blob_id(b"a")));
    let stopped = engine
        .commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Displaced(0))
        .expect("published");
    assert!(
        matches!(stopped, CommitPaths::Committed { .. }),
        "{stopped:?}"
    );
    assert!(!root.join("MEMORY.md").exists(), "moved aside");
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(
        engine_leftovers(&root).is_empty(),
        "{:?}",
        engine_leftovers(&root)
    );
    assert!(!record(&root).exists());
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    assert_eq!(git_out(&root, &["show", "HEAD:MEMORY.md"]), "a");

    let stopped = engine
        .commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Displaced(0))
        .expect("published");
    assert!(
        matches!(stopped, CommitPaths::Committed { .. }),
        "{stopped:?}"
    );
    std::fs::write(root.join("MEMORY.md"), "mine").expect("a person's save");
    commit_as_person(&root, "taken back again");
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert_eq!(read(&root, "MEMORY.md"), "mine");
    assert!(
        engine_leftovers(&root).is_empty(),
        "{:?}",
        engine_leftovers(&root)
    );
    assert!(!record(&root).exists());
    assert_eq!(status(&root), [" M MEMORY.md"]);
}

/// R95C5-02: a settling follows what the person committed since a kill
/// stopped the commit's files. Stopped once the new file was linked under
/// the path and its staging name both, then taken back: the old file comes
/// back and the commit's own new file goes. Stopped there again, the
/// person editing that file in place before taking the commit back: their
/// edit stays and the old file goes. Stopped with the old file moved
/// aside, then the path's deletion committed: the old file never comes
/// back. Each time the record goes, nothing of the engine's is left, the
/// index holds what `HEAD` does, and a watcher pass after it commits
/// nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_settling_follows_what_the_person_committed_since() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = request("a\n§\nb", Some(blob_id(b"a")));
    let settled = |context: &str| {
        assert!(
            engine.settle_commit_paths(&p).expect("settled"),
            "{context}"
        );
        assert!(
            engine_leftovers(&root).is_empty(),
            "{context}: {:?}",
            engine_leftovers(&root)
        );
        assert!(!record(&root).exists(), "{context}");
    };
    let indexed = || git_out(&root, &["ls-files", "--stage", "MEMORY.md"]);

    let stopped = engine
        .commit_paths_held(&p, &ask, &|| true, &|at| {
            at != Cut::Staged(0, Staging::Linked)
        })
        .expect("published");
    assert!(
        matches!(stopped, CommitPaths::Committed { .. }),
        "{stopped:?}"
    );
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb", "the commit's own file");
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    settled("taken back");
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(indexed().contains(&blob_id(b"a")), "{}", indexed());
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    assert_eq!(git_out(&root, &["show", "HEAD:MEMORY.md"]), "a");

    engine
        .commit_paths_held(&p, &ask, &|| true, &|at| {
            at != Cut::Staged(0, Staging::Linked)
        })
        .expect("published");
    std::fs::OpenOptions::new()
        .append(true)
        .open(root.join("MEMORY.md"))
        .and_then(|mut file| file.write_all(b"\nmine"))
        .expect("an edit in place");
    commit_as_person(&root, "taken back after an edit");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    settled("edited in place");
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb\nmine");
    assert!(indexed().contains(&blob_id(b"a")), "{}", indexed());
    assert_eq!(status(&root), [" M MEMORY.md"]);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    std::fs::write(root.join("MEMORY.md"), "a").expect("the edit undone");

    engine
        .commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Displaced(0))
        .expect("published");
    assert!(!root.join("MEMORY.md").exists(), "moved aside");
    assert!(git_ok(&root, &["rm", "-q", "--cached", "MEMORY.md"]));
    commit_as_person(&root, "deleted");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    settled("deleted");
    assert!(!root.join("MEMORY.md").exists(), "never brought back");
    assert_eq!(indexed(), "");
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    assert!(!git_ok(&root, &["cat-file", "-e", "HEAD:MEMORY.md"]));
}

/// R95C5-02: where a settling cannot tell whose a file is, it keeps the
/// record. Stopped once the new file was placed and its staging name gone
/// — as a kill right after the placement leaves it — with the old file
/// still moved aside, then taken back: the path holds the commit's bytes
/// with nothing to say they are the commit's, so both files stay, the
/// record stays and the folder's commits are held. Stopped with the old
/// file moved aside, then a third version committed: the old file is
/// neither what `HEAD` holds nor gone from it, so it stays beside the
/// empty path and the record stays.
#[tokio::test(flavor = "multi_thread")]
async fn a_settling_that_cannot_tell_whose_a_file_is_keeps_the_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let ask = request("a\n§\nb", Some(blob_id(b"a")));
    engine
        .commit_paths_held(&p, &ask, &|| true, &|at| {
            at != Cut::Staged(0, Staging::Linked)
        })
        .expect("published");
    let staged: Vec<String> = engine_leftovers(&root)
        .into_iter()
        .filter(|path| path.ends_with(".tmp"))
        .collect();
    assert_eq!(staged.len(), 1, "{staged:?}");
    std::fs::remove_file(&staged[0]).expect("the staging name gone");
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    assert!(engine.settle_commit_paths(&p).is_err());
    assert!(record(&root).exists(), "the record stays");
    assert!(engine.unsettled_commit(&p.id).expect("asked"));
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb");
    let kept = engine_leftovers(&root);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(std::fs::read_to_string(&kept[0]).expect("kept"), "a");
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);

    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    engine
        .commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Displaced(0))
        .expect("published");
    let third = dir.path().join("third");
    std::fs::write(&third, "third").expect("a third version");
    let blob = git_out(&root, &["hash-object", "-w", &third.to_string_lossy()]);
    let entry = format!("100644,{},MEMORY.md", blob.trim());
    assert!(git_ok(&root, &["update-index", "--cacheinfo", &entry]));
    commit_as_person(&root, "a third version");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    assert!(engine.settle_commit_paths(&p).is_err());
    assert!(record(&root).exists(), "the record stays");
    assert!(!root.join("MEMORY.md").exists(), "nothing brought back");
    let kept = engine_leftovers(&root);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(std::fs::read_to_string(&kept[0]).expect("kept"), "a");
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
}

/// R250: a reversal needs the old file back. Stopped once the new file was
/// linked, the old file moved aside then removed, and the commit taken
/// back: `HEAD` holds bytes nothing at the path can give back, so the
/// commit's own file stays at the path rather than leave it empty for a
/// watcher to commit as a deletion, the record stays and a watcher pass
/// commits nothing. Once the person checks the path out, the settling
/// completes.
#[tokio::test(flavor = "multi_thread")]
async fn a_reversal_whose_old_file_is_gone_keeps_the_commits_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    engine
        .commit_paths_held(
            &p,
            &request("a\n§\nb", Some(blob_id(b"a"))),
            &|| true,
            &|at| at != Cut::Staged(0, Staging::Linked),
        )
        .expect("published");
    let aside: Vec<String> = engine_leftovers(&root)
        .into_iter()
        .filter(|path| !path.ends_with(".tmp"))
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    std::fs::remove_file(&aside[0]).expect("the old file gone");
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb", "never removed");
    assert_eq!(git_out(&root, &["show", "HEAD:MEMORY.md"]), "a");

    git_out(&root, &["checkout", "--", "MEMORY.md"]);
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert!(
        engine_leftovers(&root).is_empty(),
        "{:?}",
        engine_leftovers(&root)
    );
    assert!(!record(&root).exists());
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
}

/// R95C6-01: stopped once the new file was linked, its staging name then
/// gone, and the path's deletion committed: the path holds the commit's
/// bytes with nothing to say they are the commit's, so the settling holds
/// — that file, the old one beside it and the record stay, and a watcher
/// pass commits nothing. Once the person removes the file, the settling
/// drops the old one and the deletion stays one.
#[tokio::test(flavor = "multi_thread")]
async fn a_deletion_committed_over_a_file_nothing_attributes_holds_the_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    engine
        .commit_paths_held(
            &p,
            &request("a\n§\nb", Some(blob_id(b"a"))),
            &|| true,
            &|at| at != Cut::Staged(0, Staging::Linked),
        )
        .expect("published");
    drop_staging_name(&root);
    assert!(git_ok(
        &root,
        &["update-index", "--force-remove", "MEMORY.md"]
    ));
    commit_as_person(&root, "deleted");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb", "never removed");
    let kept = engine_leftovers(&root);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(std::fs::read_to_string(&kept[0]).expect("kept"), "a");
    assert!(!git_ok(&root, &["cat-file", "-e", "HEAD:MEMORY.md"]));

    std::fs::remove_file(root.join("MEMORY.md")).expect("settled by hand");
    assert!(engine.settle_commit_paths(&p).expect("settled"));
    assert!(!root.join("MEMORY.md").exists(), "never brought back");
    assert!(
        engine_leftovers(&root).is_empty(),
        "{:?}",
        engine_leftovers(&root)
    );
    assert!(!record(&root).exists());
    assert!(status(&root).is_empty(), "{:?}", status(&root));
    assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
    assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    assert!(!git_ok(&root, &["cat-file", "-e", "HEAD:MEMORY.md"]));
}

/// R95C6-02: a third version committed over the commit's own file, still
/// linked under its staging name, holds the settling whether or not an
/// old file was moved aside: a new file the commit made, and a file it
/// rewrote, stay at the path as they are, nothing is dropped, and a
/// watcher pass commits nothing. Once the person checks the third version
/// out — and removes the old file kept beside it — the settling completes
/// and the third version stays.
#[tokio::test(flavor = "multi_thread")]
async fn a_third_version_over_the_commits_own_file_holds_the_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let linked = |at: Cut| at != Cut::Staged(0, Staging::Linked);
    let settled = |context: &str, theirs: &str| {
        assert!(
            engine.settle_commit_paths(&p).expect("settled"),
            "{context}"
        );
        assert!(
            engine_leftovers(&root).is_empty(),
            "{context}: {:?}",
            engine_leftovers(&root)
        );
        assert!(!record(&root).exists(), "{context}");
        assert!(status(&root).is_empty(), "{context}: {:?}", status(&root));
        assert_eq!(commit_after_settling(&engine, &platform, &p), 0);
        assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
    };

    let create = CommitRequest {
        writes: vec![("NEW.md".to_owned(), Some(b"new".to_vec()))],
        guards: vec![("NEW.md".to_owned(), None)],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    engine
        .commit_paths_held(&p, &create, &|| true, &linked)
        .expect("published");
    commit_through_the_index(&root, dir.path(), "NEW.md", "100644", "third");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "NEW.md"), "new", "never removed");
    assert_eq!(git_out(&root, &["show", "HEAD:NEW.md"]), "third");
    git_out(&root, &["checkout", "--", "NEW.md"]);
    settled("a new file", &theirs);
    assert_eq!(read(&root, "NEW.md"), "third");

    engine
        .commit_paths_held(
            &p,
            &request("a\n§\nb", Some(blob_id(b"a"))),
            &|| true,
            &linked,
        )
        .expect("published");
    commit_through_the_index(&root, dir.path(), "MEMORY.md", "100644", "third");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "a\n§\nb", "never removed");
    let aside: Vec<String> = engine_leftovers(&root)
        .into_iter()
        .filter(|path| !path.ends_with(".tmp"))
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(std::fs::read_to_string(&aside[0]).expect("kept"), "a");
    git_out(&root, &["checkout", "--", "MEMORY.md"]);
    std::fs::remove_file(&aside[0]).expect("settled by hand");
    settled("a rewritten file", &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "third");
}

/// R264: a settling never moves an executable bit from one file to
/// another. The commit's own file, still linked under its staging name,
/// its bit set or cleared since — or the old file moved aside, its bit
/// set or cleared since — holds the settling once the commit is taken
/// back: both files stay with their bits, the record stays and a watcher
/// pass commits nothing. Settled again, or after a kill left the
/// commit's file moved off the path, it still holds and the bit is still
/// there. Once the person settles the path — the bit put back, or the
/// path checked out — the settling completes, and a bit the person kept
/// on the old file is theirs to commit. A new file whose bit the person
/// changed holds too. A reversal committed with a mode the old file does
/// not have holds — the old file stays beside the empty path — until the
/// person checks it out.
#[tokio::test(flavor = "multi_thread")]
async fn a_mode_a_settling_cannot_attribute_holds_the_folder() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let executable = |rel: &str| {
        std::fs::metadata(root.join(rel))
            .expect("file")
            .permissions()
            .mode()
            & 0o111
            != 0
    };
    let chmod = |rel: &str, mode: u32| {
        std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(mode))
            .expect("chmod");
    };
    let at_head = |rel: &str| git_out(&root, &["ls-tree", "HEAD", rel])[..6].to_owned();
    let linked = |at: Cut| at != Cut::Staged(0, Staging::Linked);
    let aside = || {
        let aside: Vec<String> = engine_leftovers(&root)
            .into_iter()
            .filter(|path| !path.ends_with(".tmp"))
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        aside[0].clone()
    };
    let settled = |context: &str, theirs: &str, commits: u64| {
        assert!(
            engine.settle_commit_paths(&p).expect("settled"),
            "{context}"
        );
        assert!(
            engine_leftovers(&root).is_empty(),
            "{context}: {:?}",
            engine_leftovers(&root)
        );
        assert!(!record(&root).exists(), "{context}");
        if commits == 0 {
            assert!(status(&root).is_empty(), "{context}: {:?}", status(&root));
            assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
        }
        assert_eq!(
            commit_after_settling(&engine, &platform, &p),
            commits,
            "{context}"
        );
    };
    let rewrite = |rel: &str, from: &str, to: &str| CommitRequest {
        writes: vec![(rel.to_owned(), Some(to.as_bytes().to_vec()))],
        guards: vec![(rel.to_owned(), Some(blob_id(from.as_bytes())))],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };

    // Set on the commit's own file.
    engine
        .commit_paths_held(&p, &rewrite("MEMORY.md", "a", "b"), &|| true, &linked)
        .expect("published");
    chmod("MEMORY.md", 0o755);
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "b", "never removed");
    assert!(executable("MEMORY.md"), "the person's bit stays");
    let old = aside();
    assert_eq!(std::fs::read_to_string(&old).expect("kept"), "a");
    assert!(!executable(&old), "the old file as it was");
    // As a kill right after the commit's file was moved off the path
    // leaves it.
    std::fs::rename(
        root.join("MEMORY.md"),
        old.replace(".keeper-displaced-", ".keeper-taken-"),
    )
    .expect("moved off");
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "b", "back at the path");
    assert!(executable("MEMORY.md"), "the person's bit is still there");
    assert!(!executable(&aside()), "the old file as it was");
    chmod("MEMORY.md", 0o644);
    settled("set, then put back", &theirs, 0);
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(!executable("MEMORY.md"));

    // Cleared on the commit's own file.
    std::fs::write(root.join("tool.sh"), "echo 0").expect("a tool");
    chmod("tool.sh", 0o755);
    git_out(&root, &["add", "tool.sh"]);
    commit_as_person(&root, "a tool");
    engine
        .commit_paths_held(
            &p,
            &rewrite("tool.sh", "echo 0", "echo 1"),
            &|| true,
            &linked,
        )
        .expect("published");
    assert!(executable("tool.sh"), "the commit's own file");
    chmod("tool.sh", 0o644);
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "tool.sh"), "echo 1", "never removed");
    assert!(!executable("tool.sh"), "the person's bit stays");
    let old = aside();
    assert_eq!(std::fs::read_to_string(&old).expect("kept"), "echo 0");
    assert!(executable(&old), "the old file as it was");
    git_out(&root, &["checkout", "--", "tool.sh"]);
    std::fs::remove_file(&old).expect("settled by hand");
    settled("cleared, then checked out", &theirs, 0);
    assert_eq!(read(&root, "tool.sh"), "echo 0");
    assert!(executable("tool.sh"));

    // Set on the old file once the commit's own was linked.
    engine
        .commit_paths_held(&p, &rewrite("MEMORY.md", "a", "b"), &|| true, &linked)
        .expect("published");
    chmod(&aside(), 0o755);
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "MEMORY.md"), "b", "never removed");
    assert!(!executable("MEMORY.md"), "the commit's own file as it was");
    assert!(
        executable(&aside()),
        "the person's bit on the old file stays"
    );
    chmod(&aside(), 0o644);
    settled("the old file's set, then put back", &theirs, 0);
    assert_eq!(read(&root, "MEMORY.md"), "a");
    assert!(!executable("MEMORY.md"));

    // Cleared on the old file once the commit's own was linked.
    engine
        .commit_paths_held(
            &p,
            &rewrite("tool.sh", "echo 0", "echo 1"),
            &|| true,
            &linked,
        )
        .expect("published");
    chmod(&aside(), 0o644);
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "tool.sh"), "echo 1", "never removed");
    assert!(executable("tool.sh"), "the commit's own file as it was");
    assert!(
        !executable(&aside()),
        "the person's bit on the old file stays"
    );
    std::fs::remove_file(root.join("tool.sh")).expect("settled by hand");
    settled("the old file's cleared, then kept", &theirs, 1);
    assert_eq!(read(&root, "tool.sh"), "echo 0");
    assert!(!executable("tool.sh"), "the person's bit is kept");
    assert_eq!(at_head("tool.sh"), "100644");
    assert!(status(&root).is_empty(), "{:?}", status(&root));

    engine
        .commit_paths_held(
            &p,
            &request("a\n§\nb", Some(blob_id(b"a"))),
            &|| true,
            &|at| at != Cut::Displaced(0),
        )
        .expect("published");
    // `HEAD` holds the old bytes as non-executable: the reversal sets it.
    commit_through_the_index(&root, dir.path(), "MEMORY.md", "100755", "a");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert!(!root.join("MEMORY.md").exists(), "nothing brought back");
    aside();
    git_out(&root, &["checkout", "--", "MEMORY.md"]);
    settled("a reversal with another mode", &theirs, 0);
    assert_eq!(read(&root, "MEMORY.md"), "a");

    let create = CommitRequest {
        writes: vec![("NEW.md".to_owned(), Some(b"new".to_vec()))],
        guards: vec![("NEW.md".to_owned(), None)],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    engine
        .commit_paths_held(&p, &create, &|| true, &linked)
        .expect("published");
    chmod("NEW.md", 0o755);
    commit_as_person(&root, "taken back");
    let theirs = git_out(&root, &["rev-parse", "HEAD"]);
    held(&engine, &platform, &p, &theirs);
    assert_eq!(read(&root, "NEW.md"), "new", "never removed");
    assert!(executable("NEW.md"), "the person's bit stays");
    assert!(!git_ok(&root, &["cat-file", "-e", "HEAD:NEW.md"]));
    std::fs::remove_file(root.join("NEW.md")).expect("settled by hand");
    settled("a new file's bit", &theirs, 0);
    assert!(!root.join("NEW.md").exists(), "never brought back");
}

/// R95C8-01: the old file a commit moved aside goes only while it holds
/// the committed bytes with the bit they were committed with, read right
/// before it goes. Its bit set or cleared since — beside a deletion
/// committed since, beside a person's own file saved at a path whose
/// commit was taken back, or after a kill right after the settling
/// dropped the commit's own file — holds the settling: the old file stays
/// with the person's bit, a file saved at the path with it, the record
/// stays and a watcher pass commits nothing, settled again or not. Once
/// the person puts the bit back the settling drops it; once the person
/// removes it the settling completes without it.
#[tokio::test(flavor = "multi_thread")]
async fn an_old_file_whose_bit_a_person_changed_is_never_dropped() {
    use std::os::unix::fs::PermissionsExt as _;
    let executable =
        |path: &Path| std::fs::metadata(path).expect("file").permissions().mode() & 0o111 != 0;
    let chmod = |path: &Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    };
    for set in [true, false] {
        for case in ["deleted", "saved over", "killed once dropped"] {
            let context = format!("{case}, the bit {}", if set { "set" } else { "cleared" });
            let dir = tempfile::tempdir().expect("tempdir");
            let remote_dir = tempfile::tempdir().expect("tempdir");
            let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await
            else {
                return;
            };
            let root = p.local_path.clone();
            // Set on a file committed without it, cleared on one with it.
            let (rel, committed, changed) = if set {
                ("MEMORY.md", "100644", 0o755)
            } else {
                std::fs::write(root.join("tool.sh"), "a").expect("a tool");
                chmod(&root.join("tool.sh"), 0o755);
                git_out(&root, &["add", "tool.sh"]);
                commit_as_person(&root, "a tool");
                ("tool.sh", "100755", 0o644)
            };
            let ask = CommitRequest {
                writes: vec![(rel.to_owned(), Some(b"b".to_vec()))],
                guards: vec![(rel.to_owned(), Some(blob_id(b"a")))],
                subject: "memory: nixi".to_owned(),
                ..CommitRequest::default()
            };
            let stop = if case == "deleted" {
                Cut::Displaced(0)
            } else {
                Cut::Staged(0, Staging::Linked)
            };
            engine
                .commit_paths_held(&p, &ask, &|| true, &|at| at != stop)
                .expect("published");
            match case {
                "saved over" => {
                    std::fs::write(root.join(rel), "mine").expect("a person's save");
                    commit_through_the_index(&root, dir.path(), rel, committed, "a");
                }
                _ => {
                    assert!(git_ok(&root, &["update-index", "--force-remove", rel]));
                    commit_as_person(&root, "deleted");
                }
            }
            if case == "killed once dropped" {
                assert!(engine
                    .settle_commit_paths_held(&p, &|at| at != Cut::Dropped(0))
                    .is_err());
                assert!(!root.join(rel).exists(), "{context}: dropped");
            }
            let old: Vec<String> = engine_leftovers(&root)
                .into_iter()
                .filter(|path| !path.ends_with(".tmp"))
                .collect();
            assert_eq!(old.len(), 1, "{context}: {old:?}");
            let old = PathBuf::from(&old[0]);
            chmod(&old, changed);
            let theirs = git_out(&root, &["rev-parse", "HEAD"]);
            held(&engine, &platform, &p, &theirs);
            held(&engine, &platform, &p, &theirs);
            assert_eq!(
                std::fs::read_to_string(&old).expect("kept"),
                "a",
                "{context}"
            );
            assert_eq!(executable(&old), set, "{context}: the person's bit stays");
            if case == "saved over" {
                assert_eq!(read(&root, rel), "mine", "{context}: never removed");
            } else {
                assert!(!root.join(rel).exists(), "{context}: never brought back");
            }

            if set {
                chmod(&old, 0o644);
            } else {
                std::fs::remove_file(&old).expect("settled by hand");
            }
            assert!(
                engine.settle_commit_paths(&p).expect("settled"),
                "{context}"
            );
            assert!(
                engine_leftovers(&root).is_empty(),
                "{context}: {:?}",
                engine_leftovers(&root)
            );
            assert!(!record(&root).exists(), "{context}");
            if case == "saved over" {
                assert_eq!(read(&root, rel), "mine", "{context}");
                assert_eq!(
                    commit_after_settling(&engine, &platform, &p),
                    1,
                    "{context}"
                );
            } else {
                assert!(!root.join(rel).exists(), "{context}");
                assert!(status(&root).is_empty(), "{context}: {:?}", status(&root));
                assert_eq!(
                    commit_after_settling(&engine, &platform, &p),
                    0,
                    "{context}"
                );
                assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
            }
        }
    }
}

/// R95C8-02: the old file is read for whose it is right before it goes,
/// not only before the commit's own file is moved off the path. Replaced
/// by a person's own file, or its bit set, once the settling dropped the
/// commit's file beside a deletion committed since: it stays as the
/// person left it and the record stays. The person's own file then goes
/// to the path as anything else of theirs moved aside does, for a watcher
/// pass to commit; the bit holds the folder — a watcher pass commits
/// nothing — until the person removes the old file.
#[tokio::test(flavor = "multi_thread")]
async fn an_old_file_changed_while_its_settling_runs_stays() {
    use std::os::unix::fs::PermissionsExt as _;
    for case in ["replaced", "the bit set"] {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote_dir = tempfile::tempdir().expect("tempdir");
        let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await
        else {
            return;
        };
        let root = p.local_path.clone();
        engine
            .commit_paths_held(
                &p,
                &request("a\n§\nb", Some(blob_id(b"a"))),
                &|| true,
                &|at| at != Cut::Staged(0, Staging::Linked),
            )
            .expect("published");
        assert!(git_ok(
            &root,
            &["update-index", "--force-remove", "MEMORY.md"]
        ));
        commit_as_person(&root, "deleted");
        let theirs = git_out(&root, &["rev-parse", "HEAD"]);
        let old: Vec<String> = engine_leftovers(&root)
            .into_iter()
            .filter(|path| !path.ends_with(".tmp"))
            .collect();
        assert_eq!(old.len(), 1, "{case}: {old:?}");
        let old = PathBuf::from(&old[0]);
        let saved = dir.path().join("saved");
        let person = |at: Cut| {
            if at == Cut::Dropped(0) {
                if case == "replaced" {
                    std::fs::write(&saved, "a, mine").expect("a person's file");
                    std::fs::rename(&saved, &old).expect("saved over the old file");
                } else {
                    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755))
                        .expect("chmod");
                }
            }
            true
        };
        assert!(
            engine.settle_commit_paths_held(&p, &person).is_err(),
            "{case}"
        );
        assert!(record(&root).exists(), "{case}: the record stays");
        assert!(!root.join("MEMORY.md").exists(), "{case}");
        let mode = std::fs::metadata(&old).expect("kept").permissions().mode();
        if case == "replaced" {
            assert_eq!(std::fs::read_to_string(&old).expect("kept"), "a, mine");
            assert!(engine.settle_commit_paths(&p).expect("settled"), "{case}");
            assert_eq!(read(&root, "MEMORY.md"), "a, mine", "the person's file");
        } else {
            assert_eq!(std::fs::read_to_string(&old).expect("kept"), "a");
            assert_ne!(mode & 0o111, 0, "the person's bit stays");
            held(&engine, &platform, &p, &theirs);
            let mode = std::fs::metadata(&old).expect("kept").permissions().mode();
            assert_ne!(mode & 0o111, 0, "the person's bit is still there");
            std::fs::remove_file(&old).expect("settled by hand");
            assert!(engine.settle_commit_paths(&p).expect("settled"), "{case}");
            assert!(!root.join("MEMORY.md").exists(), "never brought back");
        }
        assert!(
            engine_leftovers(&root).is_empty(),
            "{case}: {:?}",
            engine_leftovers(&root)
        );
        assert!(!record(&root).exists(), "{case}");
        assert_eq!(
            commit_after_settling(&engine, &platform, &p),
            u64::from(case == "replaced"),
            "{case}"
        );
        if case != "replaced" {
            assert!(status(&root).is_empty(), "{case}: {:?}", status(&root));
            assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), theirs);
        }
    }
}

/// R95C3-14: a kill inside a file's staging — once the staging file is
/// made, once its bytes are synced, or once it is linked under the path's
/// name and still under its own — leaves nothing a watcher commits: the
/// settling clears it, and a watcher pass after it commits nothing of it.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_inside_a_files_staging_leaves_nothing_to_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, platform, p)) = with_memory(dir.path(), remote_dir.path(), "a").await else {
        return;
    };
    let root = p.local_path.clone();
    let mut memory = "a".to_owned();
    for (n, step) in [Staging::Created, Staging::Synced, Staging::Linked]
        .into_iter()
        .enumerate()
    {
        let next = format!("{memory}\n§\n{n}");
        let ask = request(&next, Some(blob_id(memory.as_bytes())));
        let done = engine
            .commit_paths_held(&p, &ask, &|| true, &|at| at != Cut::Staged(0, step))
            .expect("published");
        assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
        assert!(record(&root).exists(), "{step:?}: cut off, kept");
        assert!(engine.settle_commit_paths(&p).expect("settled"));
        assert_eq!(read(&root, "MEMORY.md"), next, "{step:?}");
        assert!(
            engine_leftovers(&root).is_empty(),
            "{step:?}: {:?}",
            engine_leftovers(&root)
        );
        commit_after_settling(&engine, &platform, &p);
        assert!(status(&root).is_empty(), "{step:?}: {:?}", status(&root));
        memory = next;
    }
    let history = git_out(&root, &["log", "--all", "--name-only", "--format="]);
    assert!(!history.contains(".keeper"), "{history}");
}

/// R95C3-15: a routed write whose publication is refused — the lease lost
/// at the compare-and-swap, or the branch moved before it — owes the
/// remote nothing: no upload is queued for the journal to drain. One that
/// is published owes its object.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_routed_write_owes_no_upload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let Some((engine, _platform, mut p)) = with_memory(dir.path(), remote_dir.path(), "a").await
    else {
        return;
    };
    p.lfs_threshold_bytes = 1024;
    engine.upsert_profile(&p).expect("upsert");
    let root = p.local_path.clone();
    let ask = CommitRequest {
        writes: vec![("clip.bin".to_owned(), Some(vec![b'x'; 4096]))],
        guards: vec![("clip.bin".to_owned(), None)],
        subject: "memory: nixi".to_owned(),
        ..CommitRequest::default()
    };
    let owed = || {
        engine
            .with_db(|conn| db::outstanding_count(conn, &p.id, WorkKind::LFS_UPLOAD))
            .expect("count")
    };
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let done = engine
        .commit_paths_held(
            &p,
            &ask,
            &|| asked.fetch_add(1, Ordering::SeqCst) < 1,
            &|_| true,
        )
        .expect("fenced");
    assert_eq!(done, CommitPaths::Fenced);
    assert_eq!(owed(), 0, "the lease lost at the compare-and-swap");

    let theirs = |at: Cut| {
        if at == Cut::Prepared {
            std::fs::write(root.join("unrelated.md"), "mine").expect("a person's file");
            git_out(&root, &["add", "unrelated.md"]);
            commit_as_person(&root, "mine");
        }
        true
    };
    let done = engine
        .commit_paths_held(&p, &ask, &|| true, &theirs)
        .expect("checked");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "HEAD".to_owned()
        }
    );
    assert_eq!(owed(), 0, "the branch moved before the compare-and-swap");

    let done = engine
        .commit_paths(&p.id, &ask, yes())
        .await
        .expect("committed");
    assert!(matches!(done, CommitPaths::Committed { .. }), "{done:?}");
    assert_eq!(owed(), 1, "published, so owed");
}

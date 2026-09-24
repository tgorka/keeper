//! `config_repo` against a real smart-HTTP git server (AD-312).
//!
//! A loopback listener hands each request to `git upload-pack` /
//! `git receive-pack --stateless-rpc` on a bare repository, the way a forge's
//! smart-HTTP backend does, and answers 401 to any request whose
//! `Authorization` is not exactly the one configured. So a green fetch proves
//! the header reached gix's transport, and `git -C <bare>` is the witness for
//! what landed.
//!
//! Skipped, not failed, on a machine with no `git`.

use std::{
    io::{BufRead, BufReader, Read, Write as _},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use keeper_sync::config_repo::{
    clone_or_fetch, commit_and_push, last_change_secs, move_and_push, Author, PushResult, RepoAuth,
    RepoSpec, Write,
};
use keeper_sync::error::SyncError;

const BRANCH: &str = "main";

fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
    command
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_command(dir).args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn have_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// A bare repository behind a loopback smart-HTTP server.
struct Harness {
    bare: PathBuf,
    url: String,
    posts: Arc<AtomicUsize>,
}

/// Run with the bare repository's path.
type Hook = Box<dyn Fn(&Path) + Send>;

/// What the server does besides serving git.
#[derive(Default)]
struct Behaviour {
    /// Answer 401 to any request whose `Authorization` is not exactly this.
    require_auth: Option<String>,
    /// Answer 403 to the fetch advertisement: the credential is accepted and
    /// may not read the repository.
    forbid_fetch: bool,
    /// Run with the bare repository just before `receive-pack` handles a push:
    /// another device landing between our advertisement and our `POST`.
    before_push: Option<Hook>,
}

impl Harness {
    fn start(root: &Path, require_auth: Option<String>) -> Self {
        Self::serve(
            root,
            Behaviour {
                require_auth,
                ..Behaviour::default()
            },
        )
    }

    fn serve(root: &Path, behaviour: Behaviour) -> Self {
        let bare = root.join("repo.git");
        std::fs::create_dir_all(&bare).expect("mkdir");
        git(&bare, &["init", "-q", "--bare", "-b", BRANCH]);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let posts = Arc::new(AtomicUsize::new(0));
        let served = Served {
            bare: bare.clone(),
            posts: Arc::clone(&posts),
            behaviour,
        };
        std::thread::spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                served.answer(stream);
            }
        });
        Self {
            bare,
            url: format!("http://127.0.0.1:{port}/repo.git"),
            posts,
        }
    }

    /// Pushes received (`POST git-receive-pack`).
    fn pushes(&self) -> usize {
        self.posts.load(Ordering::SeqCst)
    }

    fn tip(&self) -> Option<String> {
        let output = git_command(&self.bare)
            .args([
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{BRANCH}"),
            ])
            .output()
            .expect("git");
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn show(&self, path: &str) -> String {
        git(&self.bare, &["show", &format!("{BRANCH}:{path}")])
    }

    fn files(&self) -> Vec<String> {
        let listed = git(&self.bare, &["ls-tree", "-r", "--name-only", BRANCH]);
        listed.lines().map(str::to_owned).collect()
    }

    /// Commit `files` from somewhere else, straight into the bare repository.
    fn commit_elsewhere(&self, scratch: &Path, files: &[(&str, &str)]) {
        commit_into(&self.bare, scratch, files);
    }
}

/// Commit `files` into `bare` through the clone at `scratch` (made on first
/// use), over the file system rather than the served URL.
fn commit_into(bare: &Path, scratch: &Path, files: &[(&str, &str)]) {
    if !scratch.join(".git").exists() {
        git(
            scratch.parent().expect("parent"),
            &[
                "clone",
                "-q",
                bare.to_str().expect("utf-8"),
                scratch.to_str().expect("utf-8"),
            ],
        );
        git(scratch, &["checkout", "-q", "-B", BRANCH]);
    } else {
        git(scratch, &["pull", "-q", "origin", BRANCH]);
    }
    for (path, text) in files {
        let full = scratch.join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(full, text).expect("write");
        git(scratch, &["add", "--", path]);
    }
    git(scratch, &["commit", "-q", "-m", "elsewhere"]);
    git(scratch, &["push", "-q", "origin", BRANCH]);
}

struct Served {
    bare: PathBuf,
    posts: Arc<AtomicUsize>,
    behaviour: Behaviour,
}

impl Served {
    fn answer(&self, stream: std::net::TcpStream) {
        let mut writer = stream.try_clone().expect("clone stream");
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut parts = request_line.split(' ');
        let method = parts.next().unwrap_or_default().to_owned();
        let target = parts.next().unwrap_or_default().to_owned();
        let mut content_length = 0usize;
        let mut chunked = false;
        let mut authorization = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            match name.to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.trim().parse().unwrap_or(0),
                "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
                "authorization" => authorization = Some(value.trim().to_owned()),
                _ => {}
            }
        }
        let mut body = Vec::new();
        if chunked {
            loop {
                let mut size = String::new();
                if reader.read_line(&mut size).is_err() {
                    return;
                }
                let Ok(size) = usize::from_str_radix(size.trim(), 16) else {
                    return;
                };
                if size == 0 {
                    let mut end = String::new();
                    let _ = reader.read_line(&mut end);
                    break;
                }
                let mut chunk = vec![0u8; size + 2];
                if reader.read_exact(&mut chunk).is_err() {
                    return;
                }
                body.extend_from_slice(&chunk[..size]);
            }
        } else {
            body.resize(content_length, 0);
            if reader.read_exact(&mut body).is_err() {
                return;
            }
        }

        if let Some(expected) = &self.behaviour.require_auth {
            if authorization.as_deref() != Some(expected.as_str()) {
                let _ = writer.write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\n\
                      Content-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
        }

        let run = |service: &str, advertise: bool, input: &[u8]| -> Vec<u8> {
            let mut command = git_command(&self.bare);
            command.arg(service).arg("--stateless-rpc");
            if advertise {
                command.arg("--advertise-refs");
            }
            let mut child = command
                .arg(".")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("git service");
            child
                .stdin
                .take()
                .expect("piped")
                .write_all(input)
                .expect("feed the service");
            let output = child.wait_with_output().expect("service exits");
            output.stdout
        };
        let advertisement = |service: &str| -> Vec<u8> {
            let line = format!("# service=git-{service}\n");
            let mut payload = format!("{:04x}{line}0000", line.len() + 4).into_bytes();
            payload.extend(run(service, true, &[]));
            payload
        };

        if self.behaviour.forbid_fetch && target.ends_with("?service=git-upload-pack") {
            let _ = writer.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }

        let (status, content_type, payload) = match (method.as_str(), target.as_str()) {
            ("GET", "/repo.git/info/refs?service=git-upload-pack") => (
                "200 OK",
                "application/x-git-upload-pack-advertisement".to_owned(),
                advertisement("upload-pack"),
            ),
            ("GET", "/repo.git/info/refs?service=git-receive-pack") => (
                "200 OK",
                "application/x-git-receive-pack-advertisement".to_owned(),
                advertisement("receive-pack"),
            ),
            ("POST", "/repo.git/git-upload-pack") => (
                "200 OK",
                "application/x-git-upload-pack-result".to_owned(),
                run("upload-pack", false, &body),
            ),
            ("POST", "/repo.git/git-receive-pack") => {
                self.posts.fetch_add(1, Ordering::SeqCst);
                if let Some(before_push) = &self.behaviour.before_push {
                    before_push(&self.bare);
                }
                (
                    "200 OK",
                    "application/x-git-receive-pack-result".to_owned(),
                    run("receive-pack", false, &body),
                )
            }
            _ => ("404 Not Found", "text/plain".to_owned(), Vec::new()),
        };
        let _ = writer.write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        let _ = writer.write_all(&payload);
        let _ = writer.flush();
    }
}

fn client() -> reqwest::Client {
    keeper_sync::http::client("keeper-sync-test").expect("client")
}

fn spec(harness: &Harness, dir: &Path) -> RepoSpec {
    RepoSpec {
        url: harness.url.clone(),
        branch: BRANCH.to_owned(),
        dir: dir.to_path_buf(),
    }
}

fn author() -> Author {
    Author {
        name: "Alice".to_owned(),
        email: "alice@example.invalid".to_owned(),
    }
}

/// A create-only write.
fn write(rel: &str, text: &str) -> Write {
    Write {
        rel: PathBuf::from(rel),
        bytes: text.as_bytes().to_vec(),
        replace: false,
    }
}

/// A write that may overwrite a regular file.
fn replacing(rel: &str, text: &str) -> Write {
    Write {
        replace: true,
        ..write(rel, text)
    }
}

/// What the account layout's planner does: a write only for what is absent.
fn create_only(dir: &Path, wanted: &[(&str, &str)]) -> Vec<Write> {
    wanted
        .iter()
        .filter(|(rel, _)| !dir.join(rel).exists())
        .map(|(rel, text)| write(rel, text))
        .collect()
}

/// Bearer on every leg: an empty remote is adopted, the first publish creates
/// the branch, and a second device's copy sees it. The server refuses any
/// request without the exact header, so the fetch succeeding is the proof the
/// in-memory `http.extraHeader` reached gix's transport.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_bearer_adopts_an_empty_remote_publishes_and_a_second_copy_follows() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), Some("Bearer eyJ.tok".to_owned()));
    let auth = RepoAuth::Bearer("eyJ.tok".to_owned());
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));

    let outcome = clone_or_fetch(&mac, &auth, &interrupt).expect("an empty remote is adopted");
    assert!(outcome.empty_remote);
    assert_eq!(outcome.head, None);

    let wrong = RepoAuth::Bearer("stale".to_owned());
    let err =
        clone_or_fetch(&mac, &wrong, &interrupt).expect_err("the header is what authenticates");
    assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");
    assert!(!err.to_string().contains("stale"), "{err}");
    let err = clone_or_fetch(&mac, &RepoAuth::None, &interrupt).expect_err("no credential at all");
    assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");

    let pushed = commit_and_push(
        &client(),
        &mac,
        &auth,
        &author(),
        "Add alice",
        |dir| create_only(dir, &[("alice/user.toml", "login = \"alice\"\n")]),
        &interrupt,
    )
    .await
    .expect("published");
    let PushResult::Pushed { head } = pushed else {
        panic!("expected a push, got {pushed:?}");
    };
    assert_eq!(harness.tip().as_deref(), Some(head.as_str()));
    assert_eq!(harness.show("alice/user.toml"), "login = \"alice\"");
    assert_eq!(
        git(
            &harness.bare,
            &["log", "-1", "--format=%an <%ae>|%s", BRANCH]
        ),
        "Alice <alice@example.invalid>|Add alice"
    );

    let phone = spec(&harness, &root.path().join("phone"));
    let outcome = clone_or_fetch(&phone, &auth, &interrupt).expect("a second copy");
    assert!(!outcome.empty_remote);
    assert!(outcome.changed);
    assert_eq!(outcome.head.as_deref(), Some(head.as_str()));
    assert_eq!(
        std::fs::read_to_string(phone.dir.join("alice/user.toml")).expect("checked out"),
        "login = \"alice\"\n"
    );

    let again = commit_and_push(
        &client(),
        &phone,
        &auth,
        &author(),
        "Add alice",
        |dir| create_only(dir, &[("alice/user.toml", "login = \"someone else\"\n")]),
        &interrupt,
    )
    .await
    .expect("nothing to do is not an error");
    assert_eq!(again, PushResult::NothingToDo);
    assert_eq!(harness.pushes(), 1);
}

/// Another device publishes between our refresh and our push. The push is
/// refused as not a fast-forward, the next attempt plans against the new tip,
/// and the file the other device created is kept — never rewritten.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_a_raced_push_is_replanned_on_the_new_tip_and_keeps_their_file() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(&scratch, &[("alice/user.toml", "login = \"alice\"\n")]);
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));

    let calls = AtomicUsize::new(0);
    let wanted = [
        ("alice/devices/mac.toml", "name = \"ours\"\n"),
        ("alice/keeper.mac.toml", "# ours\n"),
    ];
    let pushed = commit_and_push(
        &client(),
        &mac,
        &RepoAuth::None,
        &author(),
        "Register mac",
        |dir| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                harness.commit_elsewhere(
                    &scratch,
                    &[("alice/devices/mac.toml", "name = \"theirs\"\n")],
                );
            }
            create_only(dir, &wanted)
        },
        &interrupt,
    )
    .await
    .expect("the second attempt lands");

    assert!(matches!(pushed, PushResult::Pushed { .. }), "{pushed:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one refusal, one replan");
    assert_eq!(
        harness.pushes(),
        1,
        "the first attempt was refused before any POST"
    );
    assert_eq!(harness.show("alice/devices/mac.toml"), "name = \"theirs\"");
    assert_eq!(harness.show("alice/keeper.mac.toml"), "# ours");
    assert_eq!(
        std::fs::read_to_string(mac.dir.join("alice/devices/mac.toml")).expect("worktree"),
        "name = \"theirs\"\n",
        "the local copy is the remote's"
    );
}

/// A write that escapes the repository is refused whether or not it may
/// replace, and a create-only write naming a file the tip already holds is
/// refused — before anything is committed or sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_refuses_escaping_paths_and_rewrites() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(&scratch, &[("alice/user.toml", "login = \"alice\"\n")]);
    let tip = harness.tip();
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));

    let escaping = [
        "../escaped.toml",
        "alice/../../escaped.toml",
        "/tmp/escaped.toml",
    ];
    let bad = escaping
        .iter()
        .flat_map(|rel| [write(rel, "evil\n"), replacing(rel, "evil\n")])
        .chain([write("alice/user.toml", "evil\n")]);
    for bad in bad {
        let err = commit_and_push(
            &client(),
            &mac,
            &RepoAuth::None,
            &author(),
            "bad",
            |_| vec![bad.clone(), write("alice/keeper.toml", "ok\n")],
            &interrupt,
        )
        .await
        .expect_err("refused");
        let SyncError::InvalidPathForRemote { reason, .. } = &err else {
            panic!("{bad:?}: {err:?}");
        };
        if !escaping.iter().any(|rel| bad.rel == Path::new(rel)) {
            assert_eq!(
                reason, "already exists in the repository; files there are never rewritten",
                "{bad:?}"
            );
        }
    }
    assert!(!root.path().join("escaped.toml").exists());
    assert_eq!(harness.pushes(), 0);
    assert_eq!(harness.tip(), tip);
    assert_eq!(harness.show("alice/user.toml"), "login = \"alice\"");
}

/// A replacing write overwrites the regular file the tip holds, creates one
/// that is absent, and both land in one pushed commit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_a_replacing_write_overwrites_a_file_and_pushes() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(
        &scratch,
        &[
            ("alice/user.toml", "login = \"alice\"\n"),
            ("alice/settings.toml", "[settings]\n\"a\" = 1\n"),
        ],
    );
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));

    let pushed = commit_and_push(
        &client(),
        &mac,
        &RepoAuth::None,
        &author(),
        "alice: settings from mac",
        |_| {
            vec![
                replacing("alice/settings.toml", "[settings]\n\"a\" = 2\n"),
                replacing("alice/drives.toml", "# drives\n"),
            ]
        },
        &interrupt,
    )
    .await
    .expect("replaced");
    let PushResult::Pushed { head } = pushed else {
        panic!("expected a push, got {pushed:?}");
    };
    assert_eq!(harness.tip().as_deref(), Some(head.as_str()));
    assert_eq!(harness.pushes(), 1);
    assert_eq!(harness.show("alice/settings.toml"), "[settings]\n\"a\" = 2");
    assert_eq!(harness.show("alice/drives.toml"), "# drives");
    assert_eq!(harness.show("alice/user.toml"), "login = \"alice\"");
    assert_eq!(
        git(&harness.bare, &["log", "-1", "--format=%s", BRANCH]),
        "alice: settings from mac"
    );
    assert_eq!(
        std::fs::read_to_string(mac.dir.join("alice/settings.toml")).expect("worktree"),
        "[settings]\n\"a\" = 2\n",
        "the local copy is what was pushed"
    );
}

/// Commit `files` in `scratch` (already a clone of the served repository) at
/// committer time `secs`, and push.
fn commit_at(scratch: &Path, files: &[(&str, &str)], secs: i64) {
    for (path, text) in files {
        let full = scratch.join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(full, text).expect("write");
        git(scratch, &["add", "--", path]);
    }
    let output = git_command(scratch)
        .env("GIT_COMMITTER_DATE", format!("@{secs} +0000"))
        .args(["commit", "-q", "-m", "at"])
        .output()
        .expect("git runs");
    assert!(output.status.success(), "{output:?}");
    git(scratch, &["push", "-q", "origin", BRANCH]);
}

/// A file's last change is the newest commit that touched it — not the tip
/// when a later commit touched something else, not the commit that created
/// it — read from the refreshed copy; never touched is `None`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_last_change_is_the_newest_commit_touching_the_file() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(&scratch, &[("alice/user.toml", "login = \"alice\"\n")]);
    commit_at(
        &scratch,
        &[("alice/settings.mac.toml", "# 1\n")],
        1_700_000_000,
    );
    commit_at(
        &scratch,
        &[("alice/settings.mac.toml", "# 2\n")],
        1_700_000_100,
    );
    commit_at(
        &scratch,
        &[("alice/settings.phone.toml", "# 1\n")],
        1_700_000_200,
    );
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));
    clone_or_fetch(&mac, &RepoAuth::None, &interrupt).expect("refreshed");

    let last = |rel| last_change_secs(&mac.dir, rel).expect("read");
    assert_eq!(last("alice/settings.mac.toml"), Some(1_700_000_100));
    assert_eq!(last("alice/settings.phone.toml"), Some(1_700_000_200));
    assert_eq!(last("alice/settings.tablet.toml"), None);
}

/// The copy is a cache: a local edit, a stray unpushed commit and a file the
/// remote deleted all give way to `origin/<branch>`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_refresh_is_a_hard_reset_to_the_remote() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(
        &scratch,
        &[
            ("alice/user.toml", "login = \"alice\"\n"),
            ("alice/old.toml", "old\n"),
        ],
    );
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));
    clone_or_fetch(&mac, &RepoAuth::None, &interrupt).expect("clone");

    std::fs::write(mac.dir.join("alice/user.toml"), "tampered\n").expect("edit");
    std::fs::write(mac.dir.join("stray.toml"), "stray\n").expect("stray");
    git(&mac.dir, &["add", "stray.toml"]);
    git(&mac.dir, &["commit", "-q", "-m", "unpushed"]);
    git(&scratch, &["rm", "-q", "alice/old.toml"]);
    git(&scratch, &["commit", "-q", "-m", "drop old"]);
    git(&scratch, &["push", "-q", "origin", BRANCH]);

    let outcome = clone_or_fetch(&mac, &RepoAuth::None, &interrupt).expect("refresh");
    assert!(outcome.changed);
    assert_eq!(outcome.head, harness.tip());
    assert_eq!(
        git(&mac.dir, &["rev-parse", BRANCH]),
        harness.tip().expect("tip")
    );
    assert_eq!(
        std::fs::read_to_string(mac.dir.join("alice/user.toml")).expect("restored"),
        "login = \"alice\"\n"
    );
    assert!(!mac.dir.join("alice/old.toml").exists());
    assert!(
        !mac.dir.join("stray.toml").exists(),
        "tracked by the discarded commit only"
    );
    assert_eq!(git(&mac.dir, &["status", "--porcelain"]), "");
}

/// A device rename moves its files; repeating it finds the move already made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_move_renames_once_and_refuses_to_overwrite() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(
        &scratch,
        &[
            ("alice/devices/mac.toml", "name = \"mac\"\n"),
            ("alice/keeper.mac.toml", "# mac\n"),
            ("alice/devices/taken.toml", "name = \"taken\"\n"),
        ],
    );
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));
    let moves = [
        (
            PathBuf::from("alice/devices/mac.toml"),
            PathBuf::from("alice/devices/studio.toml"),
        ),
        (
            PathBuf::from("alice/keeper.mac.toml"),
            PathBuf::from("alice/keeper.studio.toml"),
        ),
    ];

    let pushed = move_and_push(
        &client(),
        &mac,
        &RepoAuth::None,
        &author(),
        "Rename",
        &moves,
        &interrupt,
    )
    .await
    .expect("renamed");
    assert!(matches!(pushed, PushResult::Pushed { .. }));
    assert_eq!(
        harness.files(),
        vec![
            "alice/devices/studio.toml",
            "alice/devices/taken.toml",
            "alice/keeper.studio.toml"
        ]
    );
    assert_eq!(harness.show("alice/devices/studio.toml"), "name = \"mac\"");
    assert!(!mac.dir.join("alice/devices/mac.toml").exists());

    let again = move_and_push(
        &client(),
        &mac,
        &RepoAuth::None,
        &author(),
        "Rename",
        &moves,
        &interrupt,
    )
    .await
    .expect("already done");
    assert_eq!(again, PushResult::NothingToDo);

    let onto_taken = [(
        PathBuf::from("alice/devices/studio.toml"),
        PathBuf::from("alice/devices/taken.toml"),
    )];
    let err = move_and_push(
        &client(),
        &mac,
        &RepoAuth::None,
        &author(),
        "Rename",
        &onto_taken,
        &interrupt,
    )
    .await
    .expect_err("never overwrites");
    assert!(
        matches!(err, SyncError::InvalidPathForRemote { .. }),
        "{err:?}"
    );
    assert_eq!(harness.show("alice/devices/taken.toml"), "name = \"taken\"");
    assert_eq!(harness.pushes(), 1);
}

/// Another device lands in the window between our advertisement and our
/// `POST`, so the refusal comes from the server's `receive-pack` rather than
/// the client-side fast-forward guard — both on an empty remote (two devices
/// racing to create the branch) and on one with history. The remote moved, so
/// the next attempt re-plans on its tip instead of reporting a failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_a_push_the_server_refuses_because_the_remote_moved_is_retried() {
    if !have_git() {
        return;
    }
    for seeded in [false, true] {
        let root = tempfile::tempdir().expect("tempdir");
        let scratch = root.path().join("elsewhere");
        let raced = scratch.clone();
        let fired = AtomicBool::new(false);
        let harness = Harness::serve(
            root.path(),
            Behaviour {
                before_push: Some(Box::new(move |bare: &Path| {
                    if !fired.swap(true, Ordering::SeqCst) {
                        commit_into(
                            bare,
                            &raced,
                            &[("alice/devices/phone.toml", "name = \"phone\"\n")],
                        );
                    }
                })),
                ..Behaviour::default()
            },
        );
        if seeded {
            harness.commit_elsewhere(&scratch, &[("alice/user.toml", "login = \"alice\"\n")]);
        }
        let interrupt = AtomicBool::new(false);
        let mac = spec(&harness, &root.path().join("mac"));

        let pushed = commit_and_push(
            &client(),
            &mac,
            &RepoAuth::None,
            &author(),
            "Register mac",
            |dir| create_only(dir, &[("alice/devices/mac.toml", "name = \"mac\"\n")]),
            &interrupt,
        )
        .await
        .unwrap_or_else(|err| panic!("seeded={seeded}: the second attempt lands: {err:?}"));

        let PushResult::Pushed { head } = pushed else {
            panic!("seeded={seeded}: expected a push, got {pushed:?}");
        };
        assert_eq!(
            harness.pushes(),
            2,
            "seeded={seeded}: one refusal, one retry"
        );
        assert_eq!(harness.tip().as_deref(), Some(head.as_str()));
        assert_eq!(harness.show("alice/devices/phone.toml"), "name = \"phone\"");
        assert_eq!(harness.show("alice/devices/mac.toml"), "name = \"mac\"");
    }
}

/// A 403 on the fetch is the host refusing to let this account read the
/// repository: a credential problem, not the push-side refusal to write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_a_fetch_the_host_forbids_is_an_auth_failure() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::serve(
        root.path(),
        Behaviour {
            forbid_fetch: true,
            ..Behaviour::default()
        },
    );
    let auth = RepoAuth::Bearer("eyJ.tok".to_owned());
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));

    let err = clone_or_fetch(&mac, &auth, &interrupt).expect_err("read denied");
    assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");

    let err = commit_and_push(
        &client(),
        &mac,
        &auth,
        &author(),
        "m",
        |_| vec![write("alice/user.toml", "login = \"alice\"\n")],
        &interrupt,
    )
    .await
    .expect_err("nothing is published without a read");
    assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");
    assert_eq!(harness.pushes(), 0);
}

/// Commit a symbolic link at `rel` pointing at `target` through `scratch`.
#[cfg(unix)]
fn commit_link(scratch: &Path, rel: &str, target: &Path) {
    let full = scratch.join(rel);
    std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
    std::os::unix::fs::symlink(target, &full).expect("symlink");
    git(scratch, &["add", "--", rel]);
    git(scratch, &["commit", "-q", "-m", "link"]);
    git(scratch, &["push", "-q", "origin", BRANCH]);
}

/// A path the tip holds as a symbolic link — the file itself, or a directory
/// on the way to it — is refused by name, and nothing is committed through the
/// link or into what it points at; nor is a file replaced by a directory, or a
/// directory by a file. A write that may replace is refused all the same.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_refuses_to_write_through_a_symlink_in_the_tree() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("mkdir");
    std::fs::write(outside.join("keeper.toml"), "# outside\n").expect("write");
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(&scratch, &[("alice/user.toml", "login = \"alice\"\n")]);
    commit_link(&scratch, "alice/devices", &outside);
    commit_link(&scratch, "alice/keeper.toml", &outside.join("keeper.toml"));
    let tip = harness.tip();
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));

    // (path written, path refused, why a create is refused, why a replace is)
    for (rel, refused, why_create, why_replace) in [
        (
            "alice/devices/mac.toml",
            "alice/devices",
            "symbolic link",
            "symbolic link",
        ),
        (
            "alice/keeper.toml",
            "alice/keeper.toml",
            "symbolic link",
            "symbolic link",
        ),
        (
            "alice/user.toml/x.toml",
            "alice/user.toml",
            "is a file",
            "is a file",
        ),
        ("alice", "alice", "already exists", "is a directory"),
    ] {
        for (replace, why) in [(false, why_create), (true, why_replace)] {
            let bad = Write {
                replace,
                ..write(rel, "evil\n")
            };
            let err = commit_and_push(
                &client(),
                &mac,
                &RepoAuth::None,
                &author(),
                "m",
                |_| vec![bad.clone()],
                &interrupt,
            )
            .await
            .expect_err("refused");
            let SyncError::InvalidPathForRemote { path, reason } = &err else {
                panic!("{rel} (replace {replace}): {err:?}");
            };
            assert_eq!(path, Path::new(refused), "{rel} (replace {replace}): {err}");
            assert!(reason.contains(why), "{rel} (replace {replace}): {err}");
        }
    }
    let err = move_and_push(
        &client(),
        &mac,
        &RepoAuth::None,
        &author(),
        "Rename",
        &[(
            PathBuf::from("alice/user.toml"),
            PathBuf::from("alice/devices/mac.toml"),
        )],
        &interrupt,
    )
    .await
    .expect_err("a move is not written through a link either");
    assert!(
        matches!(&err, SyncError::InvalidPathForRemote { path, .. } if path == Path::new("alice/devices")),
        "{err:?}"
    );
    assert_eq!(harness.pushes(), 0);
    assert_eq!(harness.tip(), tip);
    assert!(!outside.join("mac.toml").exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("keeper.toml")).expect("outside"),
        "# outside\n"
    );
}

/// The refresh's removal of files the remote dropped never goes through a
/// symbolic link. The index still names `alice/d/f` while the working tree
/// already holds the new tip's `alice/d` as a link to a directory outside the
/// copy — what a checkout interrupted after writing its (delayed) links, or a
/// killed process, leaves. The link is replaced; what it points at is kept.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_repo_refresh_never_removes_through_a_symlinked_directory() {
    if !have_git() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let harness = Harness::start(root.path(), None);
    let victim = root.path().join("victim");
    std::fs::create_dir_all(&victim).expect("mkdir");
    std::fs::write(victim.join("f"), "precious").expect("write");
    let scratch = root.path().join("elsewhere");
    harness.commit_elsewhere(&scratch, &[("alice/d/f", "tracked\n")]);
    let interrupt = AtomicBool::new(false);
    let mac = spec(&harness, &root.path().join("mac"));
    clone_or_fetch(&mac, &RepoAuth::None, &interrupt).expect("first tip");

    git(&scratch, &["rm", "-q", "alice/d/f"]);
    commit_link(&scratch, "alice/d", &victim);
    std::fs::remove_dir_all(mac.dir.join("alice/d")).expect("rm");
    std::os::unix::fs::symlink(&victim, mac.dir.join("alice/d")).expect("symlink");

    clone_or_fetch(&mac, &RepoAuth::None, &interrupt).expect("second tip");
    assert_eq!(
        std::fs::read_to_string(victim.join("f")).expect("kept"),
        "precious"
    );
    assert!(std::fs::symlink_metadata(mac.dir.join("alice/d"))
        .expect("checked out")
        .file_type()
        .is_symlink());
    assert_eq!(git(&mac.dir, &["status", "--porcelain"]), "");
}

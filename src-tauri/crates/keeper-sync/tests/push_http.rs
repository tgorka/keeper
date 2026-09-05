//! `git::push_http` against a real `git receive-pack` (Story 66.5, AD-202).
//!
//! The phone's push is a pack keeper writes itself and one `POST` to a smart
//! HTTP server. Nothing here mocks the server side: a loopback HTTP listener
//! hands each request to `git receive-pack --stateless-rpc` on a bare
//! repository — exactly what a forge's smart-HTTP backend does — so an `ok`
//! read back here is git's own verdict on the pack, the command line and the
//! side-band framing. `git -C <bare> log` is the second witness.
//!
//! Skipped, not failed, on a machine with no `git`: there is no receiving side
//! to speak to and nothing is falsifiable. The dev host and CI both have one.

use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use keeper_sync::error::SyncError;
use keeper_sync::git::{fetch::Credential, push_http};

/// A `git` that reads no configuration but the repository's own.
///
/// The developer's global config is not part of the fixture: a
/// `core.hooksPath` there would silently replace the bare repository's
/// `pre-receive` hook, and a `credential.helper` would answer for the push.
/// Fixed identities and a fixed clock make commit ids deterministic, which is
/// why every root commit below is seeded with distinct content.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000");
    command
}

/// Run `git` in `dir` and return its stdout; `None` when there is no `git`.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = git_command(dir).args(args).output().ok()?;
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A working repository on branch `lane` whose root commit holds `seed`, or
/// `None` without `git`.
fn working_repo(dir: &Path, seed: &[u8]) -> Option<()> {
    git_command(dir)
        .args(["init", "-q", "-b", "lane"])
        .status()
        .ok()
        .filter(|status| status.success())?;
    commit_file(dir, "note.md", seed);
    Some(())
}

fn commit_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
    std::fs::write(dir.join(name), bytes).expect("write");
    git(dir, &["add", "--", name]).expect("git");
    git(dir, &["commit", "-q", "-m", name]).expect("git");
    git(dir, &["rev-parse", "lane"]).expect("git")
}

/// The receiving side: a bare repository behind a loopback smart-HTTP server.
struct Harness {
    bare: PathBuf,
    /// `http://127.0.0.1:<port>/repo.git`
    url: String,
    gets: Arc<AtomicUsize>,
    posts: Arc<AtomicUsize>,
}

impl Harness {
    /// `require_auth` is the exact `Authorization` value a request must carry
    /// to be served; anything else is a 401.
    fn start(root: &Path, require_auth: Option<String>) -> Option<Self> {
        let bare = root.join("repo.git");
        std::fs::create_dir_all(&bare).expect("mkdir");
        git_command(&bare)
            .args(["init", "-q", "--bare", "-b", "lane"])
            .status()
            .ok()
            .filter(|status| status.success())?;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let gets = Arc::new(AtomicUsize::new(0));
        let posts = Arc::new(AtomicUsize::new(0));
        let served = Served {
            bare: bare.clone(),
            gets: Arc::clone(&gets),
            posts: Arc::clone(&posts),
            require_auth,
        };
        std::thread::spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                served.answer(stream);
            }
        });
        Some(Self {
            bare,
            url: format!("http://127.0.0.1:{port}/repo.git"),
            gets,
            posts,
        })
    }

    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }

    fn posts(&self) -> usize {
        self.posts.load(Ordering::SeqCst)
    }

    fn tip(&self) -> Option<String> {
        let output = git_command(&self.bare)
            .args(["rev-parse", "--verify", "-q", "refs/heads/lane"])
            .output()
            .expect("git");
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

struct Served {
    bare: PathBuf,
    gets: Arc<AtomicUsize>,
    posts: Arc<AtomicUsize>,
    require_auth: Option<String>,
}

impl Served {
    /// One HTTP/1.1 request per connection; `Connection: close` keeps the
    /// client from reusing it, which keeps this reader trivial.
    fn answer(&self, mut stream: std::net::TcpStream) {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => return,
            }
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split(' ');
        let method = parts.next().unwrap_or_default().to_owned();
        let target = parts.next().unwrap_or_default().to_owned();
        let mut content_length = 0usize;
        let mut authorization = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            match name.to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.trim().parse().unwrap_or(0),
                "authorization" => authorization = Some(value.trim().to_owned()),
                _ => {}
            }
        }
        let mut body = vec![0u8; content_length];
        if stream.read_exact(&mut body).is_err() {
            return;
        }

        if let Some(expected) = &self.require_auth {
            if authorization.as_deref() != Some(expected.as_str()) {
                let _ = stream.write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\n\
                      Content-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
        }

        let (status, content_type, payload) = match (method.as_str(), target.as_str()) {
            ("GET", "/repo.git/info/refs?service=git-receive-pack") => {
                self.gets.fetch_add(1, Ordering::SeqCst);
                let advertised = git_command(&self.bare)
                    .args(["receive-pack", "--stateless-rpc", "--advertise-refs", "."])
                    .output()
                    .expect("git receive-pack --advertise-refs");
                assert!(advertised.status.success());
                let service = "# service=git-receive-pack\n";
                let mut payload = format!("{:04x}{service}0000", service.len() + 4).into_bytes();
                payload.extend_from_slice(&advertised.stdout);
                (
                    "200 OK",
                    "application/x-git-receive-pack-advertisement",
                    payload,
                )
            }
            ("POST", "/repo.git/git-receive-pack") => {
                self.posts.fetch_add(1, Ordering::SeqCst);
                let mut child = git_command(&self.bare)
                    .args(["receive-pack", "--stateless-rpc", "."])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .expect("git receive-pack");
                child
                    .stdin
                    .take()
                    .expect("piped")
                    .write_all(&body)
                    .expect("feed receive-pack");
                let output = child.wait_with_output().expect("receive-pack exits");
                (
                    "200 OK",
                    "application/x-git-receive-pack-result",
                    output.stdout,
                )
            }
            _ => ("404 Not Found", "text/plain", Vec::new()),
        };
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        let _ = stream.write_all(&payload);
        let _ = stream.flush();
    }
}

fn client() -> reqwest::Client {
    keeper_sync::http::client("keeper-sync-test").expect("client")
}

/// (a) the first push of one commit lands on an empty bare repository, and
/// (b) a second commit on top lands as a fast-forward; a push with nothing
/// new makes no `POST` at all.
#[tokio::test]
async fn one_commit_then_another_land_on_the_bare_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("mkdir");
    let (Some(()), Some(harness)) = (
        working_repo(&work, b"# first\n"),
        Harness::start(dir.path(), None),
    ) else {
        return;
    };
    let first = git(&work, &["rev-parse", "lane"]).expect("git");

    let report = push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect("the first push is accepted");
    assert!(report.updated);
    assert_eq!(report.remote_old, None);
    assert_eq!(report.new.to_string(), first);
    // A root commit with one file: the commit, its tree, the blob.
    assert_eq!(report.objects, 3);
    assert!(
        report
            .server_lines
            .iter()
            .any(|line| line == "ok refs/heads/lane"),
        "{:?}",
        report.server_lines
    );
    assert_eq!(harness.tip().as_deref(), Some(first.as_str()));
    assert_eq!((harness.gets(), harness.posts()), (1, 1));

    let second = commit_file(&work, "note.md", b"# first\n\nand more\n");
    let report = push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect("a fast-forward is accepted");
    assert!(report.updated);
    assert_eq!(
        report.remote_old.map(|id| id.to_string()).as_deref(),
        Some(first.as_str())
    );
    assert_eq!(report.new.to_string(), second);
    // One changed file on top of a known tree: commit, tree, blob — and not
    // the parent commit or its tree, which the remote already holds.
    assert_eq!(report.objects, 3);
    assert_eq!(harness.tip().as_deref(), Some(second.as_str()));
    assert_eq!(
        git(&harness.bare, &["log", "--format=%H", "lane"]).expect("git"),
        format!("{second}\n{first}")
    );
    assert_eq!((harness.gets(), harness.posts()), (2, 2));

    let report = push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect("nothing to push is not an error");
    assert!(!report.updated);
    assert_eq!(report.objects, 0);
    assert_eq!(
        (harness.gets(), harness.posts()),
        (3, 2),
        "an up-to-date lane costs the advertisement and nothing else"
    );
}

/// (c) a tip that does not descend from the remote's is refused **here**,
/// before any request carries a pack — whether the remote's tip is known
/// locally (a rewritten history) or has never been fetched at all.
#[tokio::test]
async fn a_non_fast_forward_is_refused_before_a_pack_is_sent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("mkdir");
    let (Some(()), Some(harness)) = (
        working_repo(&work, b"# first\n"),
        Harness::start(dir.path(), None),
    ) else {
        return;
    };
    let first = git(&work, &["rev-parse", "lane"]).expect("git");
    push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect("the first push is accepted");
    assert_eq!(harness.posts(), 1);

    // The remote's tip is known here and the local branch was rewritten.
    git(&work, &["commit", "-q", "--amend", "-m", "rewritten"]).expect("git");
    let rewritten = git(&work, &["rev-parse", "lane"]).expect("git");
    assert_ne!(rewritten, first);
    let err = push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect_err("a rewritten lane is refused");
    let SyncError::Diverged { profile, reason } = &err else {
        panic!("expected Diverged, got {err:?}");
    };
    assert_eq!(profile, "work");
    assert!(reason.contains(&first[..7]), "{reason}");
    assert!(reason.contains(&rewritten[..7]), "{reason}");
    assert!(reason.contains("never force-pushes"), "{reason}");
    assert_eq!(
        harness.posts(),
        1,
        "the refusal is client-side: no second POST reached the server"
    );
    assert_eq!(harness.tip().as_deref(), Some(first.as_str()));

    // The remote's tip was never fetched into this copy.
    let stranger = dir.path().join("stranger");
    std::fs::create_dir_all(&stranger).expect("mkdir");
    working_repo(&stranger, b"# a different root\n").expect("git was found above");
    let err = push_http::push(&client(), &stranger, &harness.url, "lane", None)
        .await
        .expect_err("an unrelated history is refused");
    let SyncError::Diverged { reason, .. } = &err else {
        panic!("expected Diverged, got {err:?}");
    };
    assert!(reason.contains("never fetched"), "{reason}");
    assert!(reason.contains("fetch first"), "{reason}");
    assert_eq!(harness.posts(), 1);
    assert_eq!(harness.tip().as_deref(), Some(first.as_str()));
}

/// (d) an LFS pointer is a blob like any other: it arrives byte-identical,
/// and nothing here needs an LFS endpoint to exist.
#[tokio::test]
async fn a_pointer_blob_arrives_byte_identical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("mkdir");
    let (Some(()), Some(harness)) = (
        working_repo(&work, b"# first\n"),
        Harness::start(dir.path(), None),
    ) else {
        return;
    };
    let pointer = b"version https://git-lfs.github.com/spec/v1\n\
                    oid sha256:4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393\n\
                    size 12345\n";
    std::fs::write(
        work.join(".gitattributes"),
        "*.mp4 filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write");
    git(&work, &["add", "--", ".gitattributes"]).expect("git");
    let tip = commit_file(&work, "clip.mp4", pointer);
    let blob = git(&work, &["rev-parse", "lane:clip.mp4"]).expect("git");

    let report = push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect("accepted");
    assert!(report.updated);
    // Root commit, tree, note.md, then commit, tree, .gitattributes, clip.mp4.
    assert_eq!(report.objects, 7);
    assert_eq!(harness.tip().as_deref(), Some(tip.as_str()));

    let received = git_command(&harness.bare)
        .args(["cat-file", "blob", &blob])
        .output()
        .expect("git cat-file");
    assert!(received.status.success());
    assert_eq!(received.stdout, pointer);
    assert!(
        git(&harness.bare, &["fsck", "--strict"]).is_some(),
        "the received pack is a whole, valid object set"
    );
}

/// (e) a server `ng` reaches the caller as the server wrote it, and (f) the
/// hook's own text and the remote URL are scrubbed of userinfo on the way.
#[tokio::test]
async fn a_server_refusal_reaches_the_caller_scrubbed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("mkdir");
    let (Some(()), Some(harness)) = (
        working_repo(&work, b"# first\n"),
        Harness::start(dir.path(), None),
    ) else {
        return;
    };
    let hook = harness.bare.join("hooks").join("pre-receive");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'policy: see http://bob:hunter2@forge.invalid/rules' >&2\nexit 1\n",
    )
    .expect("write hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let with_userinfo = harness.url.replacen("http://", "http://alice:s3cret@", 1);
    let err = push_http::push(&client(), &work, &with_userinfo, "lane", None)
        .await
        .expect_err("the hook declines");
    let text = err.to_string();
    assert!(
        text.contains("ng refs/heads/lane pre-receive hook declined"),
        "{text}"
    );
    assert!(
        text.contains("policy: see http://***@forge.invalid/rules"),
        "{text}"
    );
    assert!(!text.contains("hunter2"), "{text}");
    assert!(!text.contains("s3cret"), "{text}");
    assert!(!text.contains("alice"), "{text}");
    assert_eq!(harness.posts(), 1, "the server did receive the request");
    assert_eq!(harness.tip(), None, "and declined it");
}

/// The credential goes out as HTTP Basic and never into a diagnostic.
#[tokio::test]
async fn the_credential_is_sent_as_basic_and_never_shown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("mkdir");
    let token = keeper_sync::credential::AccessToken::new("tkn-s3cret");
    let (Some(()), Some(harness)) = (
        working_repo(&work, b"# first\n"),
        Harness::start(dir.path(), Some(token.lfs_basic())),
    ) else {
        return;
    };

    let err = push_http::push(&client(), &work, &harness.url, "lane", None)
        .await
        .expect_err("no credential is a 401");
    assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");
    assert!(!err.to_string().contains("s3cret"));
    assert_eq!(harness.posts(), 0);

    let wrong = Credential {
        username: "tkn-wrong".into(),
        secret: String::new(),
    };
    let err = push_http::push(&client(), &work, &harness.url, "lane", Some(&wrong))
        .await
        .expect_err("a wrong credential is a 401");
    assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");
    assert!(!err.to_string().contains("tkn-wrong"));

    let report = push_http::push(&client(), &work, &harness.url, "lane", Some(&token.git()))
        .await
        .expect("the token as the Basic username is what the server wants");
    assert!(report.updated);
    assert_eq!(harness.posts(), 1);
}

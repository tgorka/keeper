//! A note saved on the phone becomes a commit and a push (Story 66.4, AD-202).
//!
//! The whole path a save takes on a phone, driven end to end against a real
//! `git`: a phone-shaped engine (`GitEngine::Gix` — nothing here spawns on the
//! phone's side) clones a remote over smart HTTP, a note is written into the
//! container the way `notes_vault::write_note` writes one, the phone declares
//! its own container settled (`Engine::prime_worktree_changes`, the seam the
//! notes cadence calls on a phone), and one sync pass commits the note and
//! pushes it through `push_http`. The bare repository behind the loopback
//! server is the witness: `git log` there names the commit, and `git show`
//! reads the note's bytes back.
//!
//! The server is `push_http.rs`'s harness grown by the fetch half — `git
//! upload-pack --stateless-rpc` behind `info/refs?service=git-upload-pack` and
//! `POST /git-upload-pack`, with the client's `Git-Protocol` header handed to
//! git as `GIT_PROTOCOL`, which is what a forge's backend does. So the clone
//! and the fetch before the push go over HTTP too, as they do on the phone.
//!
//! Skipped, not failed, on a machine with no `git`: there is no server side to
//! speak to and nothing is falsifiable. The dev host and CI both have one.

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

use keeper_sync::{
    engine::Engine,
    git::{cli::GitEngine, history},
    platform::TestPlatform,
    provenance::SyncSource,
    SyncPlatform, SyncProfile,
};

/// A `git` that reads no configuration but the repository's own.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "mac")
        .env("GIT_AUTHOR_EMAIL", "mac@example.invalid")
        .env("GIT_COMMITTER_NAME", "mac")
        .env("GIT_COMMITTER_EMAIL", "mac@example.invalid");
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

/// The remote: a bare repository behind a loopback smart-HTTP server that
/// answers both services, and a "Mac" clone that seeds it.
struct Harness {
    bare: PathBuf,
    mac: PathBuf,
    /// `http://127.0.0.1:<port>/repo.git`
    url: String,
    receive_posts: Arc<AtomicUsize>,
}

impl Harness {
    fn start(root: &Path) -> Option<Self> {
        let bare = root.join("repo.git");
        std::fs::create_dir_all(&bare).expect("mkdir");
        git_command(&bare)
            .args(["init", "-q", "--bare", "-b", "main"])
            .status()
            .ok()
            .filter(|status| status.success())?;
        let mac = root.join("mac");
        std::fs::create_dir_all(&mac).expect("mkdir");
        git(&mac, &["init", "-q", "-b", "main"])?;
        git(&mac, &["remote", "add", "origin", &bare.to_string_lossy()])?;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let receive_posts = Arc::new(AtomicUsize::new(0));
        let served = Served {
            bare: bare.clone(),
            receive_posts: Arc::clone(&receive_posts),
        };
        std::thread::spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                served.answer(stream);
            }
        });
        Some(Self {
            bare,
            mac,
            url: format!("http://127.0.0.1:{port}/repo.git"),
            receive_posts,
        })
    }

    /// The Mac writes `rel` and publishes it.
    fn mac_publishes(&self, rel: &str, bytes: &[u8]) -> String {
        let path = self.mac.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, bytes).expect("write");
        git(&self.mac, &["add", "-A"]).expect("git");
        git(&self.mac, &["commit", "-q", "-m", rel]).expect("git");
        git(&self.mac, &["push", "-q", "origin", "main"]).expect("git");
        git(&self.mac, &["rev-parse", "main"]).expect("git")
    }

    fn tip(&self) -> String {
        git(
            &self.bare,
            &["rev-parse", "--verify", "-q", "refs/heads/main"],
        )
        .expect("git")
    }
}

struct Served {
    bare: PathBuf,
    receive_posts: Arc<AtomicUsize>,
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
        let mut content_length = None;
        let mut chunked = false;
        let mut git_protocol = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            match name.to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.trim().parse().ok(),
                "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
                "git-protocol" => git_protocol = Some(value.trim().to_owned()),
                _ => {}
            }
        }
        let body = if chunked {
            read_chunked(&mut stream)
        } else {
            let mut body = vec![0u8; content_length.unwrap_or(0)];
            if stream.read_exact(&mut body).is_err() {
                return;
            }
            body
        };

        let service = |name: &str| {
            let mut command = git_command(&self.bare);
            if let Some(protocol) = &git_protocol {
                command.env("GIT_PROTOCOL", protocol);
            }
            command.arg(name);
            command
        };
        let advertise = |name: &str| {
            let advertised = service(name)
                .args(["--stateless-rpc", "--advertise-refs", "."])
                .output()
                .expect("git --advertise-refs");
            assert!(advertised.status.success());
            let banner = format!("# service=git-{name}\n");
            let mut payload = format!("{:04x}{banner}0000", banner.len() + 4).into_bytes();
            payload.extend_from_slice(&advertised.stdout);
            payload
        };
        let serve = |name: &str| {
            let mut child = service(name)
                .args(["--stateless-rpc", "."])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("git --stateless-rpc");
            child
                .stdin
                .take()
                .expect("piped")
                .write_all(&body)
                .expect("feed the service");
            child.wait_with_output().expect("the service exits").stdout
        };
        let (status, content_type, payload) = match (method.as_str(), target.as_str()) {
            ("GET", "/repo.git/info/refs?service=git-upload-pack") => (
                "200 OK",
                "application/x-git-upload-pack-advertisement",
                advertise("upload-pack"),
            ),
            ("POST", "/repo.git/git-upload-pack") => (
                "200 OK",
                "application/x-git-upload-pack-result",
                serve("upload-pack"),
            ),
            ("GET", "/repo.git/info/refs?service=git-receive-pack") => (
                "200 OK",
                "application/x-git-receive-pack-advertisement",
                advertise("receive-pack"),
            ),
            ("POST", "/repo.git/git-receive-pack") => {
                self.receive_posts.fetch_add(1, Ordering::SeqCst);
                (
                    "200 OK",
                    "application/x-git-receive-pack-result",
                    serve("receive-pack"),
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

/// A `Transfer-Encoding: chunked` body, for a client that streams its request.
fn read_chunked(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => line.push(byte[0]),
                _ => return body,
            }
        }
        let size_text = String::from_utf8_lossy(&line);
        let size = usize::from_str_radix(size_text.trim().split(';').next().unwrap_or(""), 16)
            .unwrap_or(0);
        if size == 0 {
            // The trailing CRLF after the last chunk, if the client sent one.
            let mut trailer = [0u8; 2];
            let _ = stream.read_exact(&mut trailer);
            return body;
        }
        let mut chunk = vec![0u8; size + 2];
        if stream.read_exact(&mut chunk).is_err() {
            return body;
        }
        chunk.truncate(size);
        body.extend_from_slice(&chunk);
    }
}

/// A phone-shaped engine and a profile pointing at the harness, not yet
/// cloned — the state the phone's profile sheet leaves behind.
fn phone(dir: &Path, remote_url: &str) -> (Arc<TestPlatform>, Engine, SyncProfile) {
    let platform = Arc::new(TestPlatform::new(dir).without_git());
    let engine = Engine::open_with_engine(
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        GitEngine::Gix,
    )
    .expect("a phone opens with no git binary at all");
    let mut profile = SyncProfile::new(
        "01JPHONENOTES",
        "phone",
        dir.join("phone"),
        remote_url.to_owned(),
    );
    profile.make_fully_virtual();
    engine.upsert_profile(&profile).expect("upsert");
    (platform, engine, profile)
}

/// The whole path: clone over HTTP, write a note the way the editor's save
/// does, declare it settled, one pass commits it and pushes it, and the
/// remote's own `git` reads it back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_saved_on_the_phone_is_a_commit_on_the_remote_within_one_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(harness) = Harness::start(dir.path()) else {
        return;
    };
    let first = harness.mac_publishes("notes/one.md", b"one\n");

    let (_platform, engine, profile) = phone(dir.path(), &harness.url);
    let outcome = engine
        .sync_once_recording(&profile.id, SyncSource::Manual)
        .await
        .expect("the first pass clones over HTTP");
    assert!(outcome.pulled);
    assert_eq!(
        std::fs::read(profile.local_path.join("notes/one.md")).expect("checked out"),
        b"one\n"
    );
    assert_eq!(
        harness.receive_posts.load(Ordering::SeqCst),
        0,
        "nothing to push yet"
    );

    // The save: `notes_vault::write_note` is a temp-and-rename of the whole
    // document, frontmatter included, into the vault subfolder.
    let note = profile.local_path.join("notes/captured.md");
    let text = b"---\nid: 01JCAPTUREDNOTE\n---\n\nring the dentist\n";
    std::fs::write(profile.local_path.join("notes/.keeper.tmp"), text).expect("temp");
    std::fs::rename(profile.local_path.join("notes/.keeper.tmp"), &note).expect("rename");

    // The phone's cadence: declare, then sync once.
    assert_eq!(
        engine
            .prime_worktree_changes(&profile.id)
            .expect("a phone declares its own container settled"),
        1
    );
    let outcome = engine
        .sync_once_recording(&profile.id, SyncSource::Watch)
        .await
        .expect("the pass commits and pushes");
    assert_eq!(outcome.files_changed, 1, "the one note was committed");
    assert_eq!(outcome.committed.as_deref(), Some("main"));
    assert!(outcome.pushed, "the push leg ran");
    assert_eq!(
        harness.receive_posts.load(Ordering::SeqCst),
        1,
        "exactly one pack reached git receive-pack"
    );

    // The remote's own witness.
    let tip = harness.tip();
    assert_ne!(tip, first);
    assert_eq!(
        git(&harness.bare, &["log", "--format=%H", "main"]).expect("git"),
        format!("{tip}\n{first}"),
        "one commit on top of the Mac's, and nothing rewritten"
    );
    assert_eq!(
        git(
            &harness.bare,
            &["show", &format!("{tip}:notes/captured.md")]
        )
        .expect("git"),
        String::from_utf8_lossy(text).trim(),
        "the note's bytes are on the remote"
    );
    let trailers = git(&harness.bare, &["log", "-1", "--format=%B", "main"]).expect("git");
    assert!(
        trailers.contains("Keeper-Device: test-host"),
        "the phone's commit carries keeper's provenance trailers:\n{trailers}"
    );

    // And the phone's own history, read in-process, names the same commit.
    let phone_log = history::file_log(&profile.local_path, "notes/captured.md", 5)
        .expect("the phone reads its history without a binary");
    assert_eq!(phone_log.len(), 1);
    assert_eq!(phone_log[0].id, tip);

    // The Mac's next fetch sees it: the round trip the acceptance describes.
    git(&harness.mac, &["pull", "-q", "--ff-only", "origin", "main"]).expect("git");
    assert_eq!(
        std::fs::read(harness.mac.join("notes/captured.md")).expect("on the mac"),
        text
    );

    // Nothing new: a second pass pushes nothing.
    engine
        .sync_once_recording(&profile.id, SyncSource::Manual)
        .await
        .expect("an up-to-date phone syncs cleanly");
    assert_eq!(harness.receive_posts.load(Ordering::SeqCst), 1);
}

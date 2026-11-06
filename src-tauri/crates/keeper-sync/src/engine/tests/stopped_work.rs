//! Stopped work that ends when its cause does (Story 102.2, R467): the
//! credential's return re-arms what it stopped, a held push says why, a merge
//! owes the uploads its pointers need, and a download the server cannot answer
//! is healed from history or waits, once, for its upload to land — the
//! owner's tgdrive and neuradrive of 2026-10-19, rebuilt over a fixture LFS
//! server.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;

use sha2::{Digest as _, Sha256};

use super::*;
use crate::platform::TestPlatform;

/// The sentence the owner's journal held beside upload 704972.
const SIGNED_OUT: &str = "the account credential is unavailable: Reconnect the repository.";

/// A [`TestPlatform`] whose account can be signed out: while `out`, every
/// credential read fails the way the shell's account leg does.
struct Account {
    inner: Arc<TestPlatform>,
    out: AtomicBool,
}

impl SyncPlatform for Account {
    fn data_dir(&self) -> Result<PathBuf> {
        self.inner.data_dir()
    }
    fn secret_get(&self, key: &str) -> Result<Option<String>> {
        if self.out.load(Ordering::SeqCst) {
            return Err(SyncError::Credential(SIGNED_OUT.to_owned()));
        }
        self.inner.secret_get(key)
    }
    fn secret_set(&self, key: &str, value: &str) -> Result<()> {
        self.inner.secret_set(key, value)
    }
    fn secret_delete(&self, key: &str) -> Result<()> {
        self.inner.secret_delete(key)
    }
    fn notify(&self, title: &str, body: &str) {
        self.inner.notify(title, body);
    }
    fn now_ms(&self) -> i64 {
        self.inner.now_ms()
    }
    fn utc_offset_minutes(&self) -> i32 {
        self.inner.utc_offset_minutes()
    }
    fn open_file_state(&self, path: &Path) -> crate::OpenFileState {
        self.inner.open_file_state(path)
    }
    fn free_space(&self, path: &Path) -> Option<u64> {
        self.inner.free_space(path)
    }
    fn git_program(&self) -> Result<PathBuf> {
        self.inner.git_program()
    }
    fn host_label(&self) -> String {
        self.inner.host_label()
    }
}

/// A batch endpoint that holds the objects in its store and nothing else: an
/// upload of a held object is answered with no action (already there), of
/// any other with an upload action to this server, whose `PUT` stores it; a
/// download of a held object with a download action, of any other with a
/// per-object 404. Every batch is logged as `"<operation> <oid>"`.
struct LfsServer {
    url: String,
    batches: Arc<Mutex<Vec<String>>>,
    objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    barrier: Barrier,
}

/// Run once while the next object download is in flight, before its bytes
/// are sent: the moment its unit is `running`.
type Barrier = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

impl LfsServer {
    /// A server already holding `held` (their bytes never asked for).
    fn start(held: Vec<String>) -> Self {
        Self::start_with(held.into_iter().map(|oid| (oid, Vec::new())).collect())
    }

    /// A server holding these objects' bytes.
    fn start_with(objects: Vec<(String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let base = format!("http://127.0.0.1:{port}");
        let batches = Arc::new(Mutex::new(Vec::new()));
        let objects = Arc::new(Mutex::new(objects.into_iter().collect::<HashMap<_, _>>()));
        let barrier: Barrier = Arc::new(Mutex::new(None));
        let log = Arc::clone(&batches);
        let held = Arc::clone(&objects);
        let waiting = Arc::clone(&barrier);
        let href = base.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                answer(stream, &href, &held, &log, &waiting);
            }
        });
        Self {
            url: format!("{base}/o/r.git/info/lfs"),
            batches,
            objects,
            barrier,
        }
    }

    fn batches(&self) -> Vec<String> {
        Engine::lock(&self.batches).clone()
    }

    /// The bytes the server holds for `oid`, if it holds them.
    fn holds(&self, oid: &str) -> Option<Vec<u8>> {
        Engine::lock(&self.objects).get(oid).cloned()
    }

    /// Another device's upload of `bytes` lands.
    fn receive(&self, oid: &str, bytes: &[u8]) {
        Engine::lock(&self.objects).insert(oid.to_owned(), bytes.to_vec());
    }

    /// Run `hook` during the next object download ([`Barrier`]).
    fn during_next_download(&self, hook: impl FnOnce() + Send + 'static) {
        *Engine::lock(&self.barrier) = Some(Box::new(hook));
    }
}

fn answer(
    mut stream: TcpStream,
    base: &str,
    held: &Mutex<HashMap<String, Vec<u8>>>,
    log: &Mutex<Vec<String>>,
    barrier: &Barrier,
) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut request = head.lines().next().unwrap_or_default().split_whitespace();
    let method = request.next().unwrap_or_default().to_owned();
    let target = request.next().unwrap_or_default().to_owned();
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if stream.read_exact(&mut body).is_err() {
        return;
    }
    if let Some(oid) = target.strip_prefix("/transfer/") {
        let oid = oid.to_owned();
        let payload = if method == "PUT" {
            Engine::lock(held).insert(oid, body);
            Vec::new()
        } else {
            let hook = Engine::lock(barrier).take();
            if let Some(hook) = hook {
                hook();
            }
            Engine::lock(held).get(&oid).cloned().unwrap_or_default()
        };
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        let _ = stream.write_all(&payload);
        return;
    }
    let asked: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let operation = asked["operation"].as_str().unwrap_or_default().to_owned();
    let objects: Vec<serde_json::Value> = asked["objects"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|object| {
            let oid = object["oid"].as_str().unwrap_or_default().to_owned();
            Engine::lock(log).push(format!("{operation} {oid}"));
            let here = Engine::lock(held).contains_key(&oid);
            let href = format!("{base}/transfer/{oid}");
            match (operation.as_str(), here) {
                ("upload", true) => serde_json::json!({ "oid": oid, "size": object["size"] }),
                ("upload", false) => serde_json::json!({
                    "oid": oid,
                    "size": object["size"],
                    "actions": { "upload": { "href": href } }
                }),
                (_, true) => serde_json::json!({
                    "oid": oid,
                    "size": object["size"],
                    "actions": { "download": { "href": href } }
                }),
                (_, false) => serde_json::json!({
                    "oid": oid,
                    "size": object["size"],
                    "error": { "code": 404, "message": "Object does not exist" }
                }),
            }
        })
        .collect();
    let payload = serde_json::to_vec(&serde_json::json!({ "objects": objects })).expect("json");
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.git-lfs+json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
        .as_bytes(),
    );
    let _ = stream.write_all(&payload);
}

fn oid_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// `git <args>` in `dir`, failing the test on a non-zero exit.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Marta",
            "-c",
            "user.email=marta@keeper.invalid",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// An engine over `platform`, with a profile whose LFS server is `server`.
fn engine_with_server(
    dir: &Path,
    platform: Arc<dyn SyncPlatform>,
    server: &LfsServer,
) -> Option<(Engine, SyncProfile)> {
    let engine = Engine::open(platform).ok()?;
    let p = adoptable(dir);
    std::fs::write(
        p.local_path.join(".lfsconfig"),
        format!("[lfs]\n\turl = {}\n", server.url),
    )
    .expect("lfsconfig");
    engine.upsert_profile(&p).expect("upsert");
    Some((engine, p))
}

fn journal_rows(engine: &Engine, id: &str) -> Vec<(String, String, Option<String>, String)> {
    engine
        .with_db(|conn| {
            let mut stmt = conn.prepare(
                "SELECT kind, state, label, payload FROM journal WHERE profile_id = ?1 ORDER BY id",
            )?;
            let rows =
                stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .expect("journal")
}

/// The owner's tgdrive: an upload the signed-out account stopped, of an
/// object the server already holds, and a push held behind it.
async fn stopped_upload(
    dir: &Path,
) -> Option<(Arc<Account>, Engine, SyncProfile, LfsServer, String, i64)> {
    let bytes = br#"{"recording":"2026-10-16 14.07 test"}"#;
    let oid = oid_of(bytes);
    let server = LfsServer::start(vec![oid.clone()]);
    let account = Arc::new(Account {
        inner: Arc::new(TestPlatform::new(dir)),
        out: AtomicBool::new(true),
    });
    let (engine, p) =
        engine_with_server(dir, Arc::clone(&account) as Arc<dyn SyncPlatform>, &server)?;
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    store
        .insert_verified(&oid, &bytes[..], bytes.len() as u64)
        .expect("the object is in this folder's store");
    let now = account.now_ms();
    let unit = WorkKind::LfsUpload {
        oid: oid.clone(),
        size: bytes.len() as u64,
    };
    engine
        .with_db(|conn| {
            let id = db::enqueue(conn, &p.id, &unit, now, now)?;
            db::label_unit(conn, id, "40-media/recordings/2026/test/manifest.json")
        })
        .expect("queue the upload");
    let push = engine
        .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, now, now))
        .expect("queue the push");
    engine
        .reschedule_after(&p, push, 1, &SyncError::LfsUploadPending { objects: 1 })
        .expect("hold the push");

    engine
        .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("the drain itself succeeds");
    assert_eq!(
        engine
            .with_db(|conn| db::list_parked(conn, &p.id))
            .expect("parked")
            .len(),
        1,
        "the signed-out account stops the upload on its first attempt"
    );
    assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 1);
    assert!(server.batches().is_empty(), "nothing reached the server");
    Some((account, engine, p, server, oid, push))
}

/// After the stopped upload is re-armed, the next drains run it and release
/// the push — and the server is asked once.
async fn the_push_follows(engine: &Engine, p: &SyncProfile, server: &LfsServer, oid: &str) {
    engine
        .drain_kind(p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("drain the re-armed upload");
    assert_eq!(engine.lfs_uploads_outstanding(p).expect("count"), 0);
    assert_eq!(server.batches(), vec![format!("upload {oid}")]);
    // The next drain's first act releases the held push.
    engine
        .drain_kind(p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("release");
    let held = engine
        .with_db(|conn| db::has_unit_in(conn, &p.id, WorkKind::PUSH, "deferred"))
        .expect("read");
    assert!(!held, "the push is released once nothing is outstanding");
}

#[tokio::test]
async fn a_sign_in_rearms_the_upload_the_signed_out_account_stopped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some((account, engine, p, server, oid, _)) = stopped_upload(dir.path()).await else {
        return;
    };
    // A second folder of the same account and a stopped unit of another
    // cause: neither is touched by this folder's credential.
    engine
        .with_db(|conn| {
            let id = db::enqueue(conn, &p.id, &WorkKind::Verify, 0, 0)?;
            db::reschedule_failed(
                conn,
                id,
                WorkState::Parked,
                0,
                &SyncError::Forbidden {
                    host: "electra".to_owned(),
                },
            )
        })
        .expect("a 403 stopped verify");

    account.out.store(false, Ordering::SeqCst);
    assert_eq!(
        engine
            .rearm_after_credential(&|id: &str| id == p.id)
            .expect("rearm"),
        1
    );
    let parked = engine
        .with_db(|conn| db::list_parked(conn, &p.id))
        .expect("parked");
    assert_eq!(parked.len(), 1, "a 403 is not the credential: {parked:?}");
    assert_eq!(parked[0].kind, WorkKind::Verify.tag());

    the_push_follows(&engine, &p, &server, &oid).await;
}

/// R503's scope: a credential that came back re-arms the folders on THAT
/// credential. Two folders, both stopped by their credential; the source
/// of folder A recovers, and folder B's stopped rows — and the automatic
/// re-arms they have left — are exactly as they were.
#[tokio::test]
async fn a_recovered_credential_rearms_only_the_folders_that_use_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Ok(engine) = Engine::open(platform as Arc<dyn SyncPlatform>) else {
        return;
    };
    let a = adoptable(&dir.path().join("a"));
    let mut b = adoptable(&dir.path().join("b"));
    b.id = "01JOTHERSOURCE".to_owned();
    b.name = "on-another-source".to_owned();
    engine.upsert_profile(&a).expect("upsert a");
    engine.upsert_profile(&b).expect("upsert b");
    let stop = |p: &SyncProfile| {
        engine
            .with_db(|conn| {
                let id = db::enqueue(conn, &p.id, &upload_of(&p.id), 0, 0)?;
                db::reschedule_failed(
                    conn,
                    id,
                    WorkState::Parked,
                    0,
                    &SyncError::Credential(SIGNED_OUT.to_owned()),
                )?;
                Ok(id)
            })
            .expect("a credential-stopped upload")
    };
    let (in_a, in_b) = (stop(&a), stop(&b));
    let row = |id: i64| -> (String, Option<i64>) {
        engine
            .with_db(|conn| {
                Ok(conn.query_row(
                    "SELECT state, rearms FROM journal WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .expect("row")
    };

    assert_eq!(
        engine
            .rearm_after_credential(&|id: &str| id == a.id)
            .expect("rearm"),
        1
    );
    assert_eq!(row(in_a), ("pending".to_owned(), Some(1)));
    assert_eq!(
        row(in_b),
        ("parked".to_owned(), None),
        "the other source has not come back: its row and its allowance stand"
    );
}

/// One upload of an object named after `seed`.
fn upload_of(seed: &str) -> WorkKind {
    WorkKind::LfsUpload {
        oid: oid_of(seed.as_bytes()),
        size: seed.len() as u64,
    }
}

#[tokio::test]
async fn a_round_trip_the_remote_accepted_rearms_the_stopped_upload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some((account, engine, p, server, oid, _)) = stopped_upload(dir.path()).await else {
        return;
    };
    account.out.store(false, Ordering::SeqCst);
    // A pull that worked — the owner's 02:25Z — is a unit that reached the
    // remote; nothing else is asked of it here.
    engine.note_unit_succeeded(&p, RemoteContact::Reached);
    the_push_follows(&engine, &p, &server, &oid).await;
}

/// R503: a push held while its uploads still moved explains itself when the
/// last of them stops — nothing has to fail the push a second time — and a
/// fresh process restores the explanation from the journal.
#[tokio::test]
async fn a_push_held_while_its_upload_moved_says_why_once_the_upload_stops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some((account, engine, p, _server, _oid, _push)) = stopped_upload(dir.path()).await else {
        return;
    };
    // `stopped_upload` held the push while the upload was still queued, then
    // drained the upload, which parked. Nothing else ran.
    let status = engine.status(&p.id).expect("status");
    assert_eq!(status.state, ProfileState::NeedsAttention);
    let error = status.error.expect("a held push says why");
    assert!(is_held_push(&error), "{error}");
    assert!(
        error.contains("40-media/recordings/2026/test/manifest.json") && error.contains(SIGNED_OUT),
        "names the file and the reason it stopped: {error}"
    );

    // A pull that skipped the remote does not publish the folder: the
    // sentence stands.
    engine.note_unit_succeeded(&p, RemoteContact::Skipped);
    assert_eq!(
        engine.status(&p.id).expect("status").error.as_deref(),
        Some(error.as_str())
    );

    // An older run left the upload's own sentence on the profile; a fresh
    // process reads the hold off the journal instead.
    engine
        .with_db(|conn| db::set_profile_error(conn, &p.id, Some(SIGNED_OUT), 0))
        .expect("an older run's sentence");
    drop(engine);
    let Ok(engine) = Engine::open(Arc::clone(&account) as Arc<dyn SyncPlatform>) else {
        return;
    };
    let restored = engine
        .status(&p.id)
        .expect("status")
        .error
        .expect("restored");
    assert!(is_held_push(&restored), "{restored}");

    assert_eq!(engine.retry_all_parked(&p.id).await.expect("retry all"), 1);
    assert!(engine.status(&p.id).expect("status").error.is_none());
    assert!(engine
        .with_db(|conn| db::list_parked(conn, &p.id))
        .expect("parked")
        .is_empty());
}

/// Marta's merge on neuradrive: a conflict copy whose pointer names an
/// object only this machine holds. The merge owes its upload, and the push
/// waits for it.
#[tokio::test]
async fn a_merge_owes_the_upload_of_its_conflict_copys_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    let Some(p) = publishing_fixture(&engine, &platform, dir.path(), remote_dir.path()).await
    else {
        return;
    };
    advance_remote_with(
        remote_dir.path(),
        &[("manifest.json", "{\"theirs\":1}\n"), ("root.md", "root")],
        "theirs",
    );
    // This machine's own revision of the same file: a pointer whose object is
    // in its store and nowhere else, committed by hand so no upload is owed
    // for the commit itself.
    let bytes = b"{\"ours\":\"maczek\"}\n";
    let oid = oid_of(bytes);
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    store
        .insert_verified(&oid, &bytes[..], bytes.len() as u64)
        .expect("store");
    let pointer = lfs::pointer::Pointer::new(oid.clone(), bytes.len() as u64).render();
    std::fs::write(p.local_path.join("manifest.json"), pointer).expect("write");
    git(&p.local_path, &["add", "manifest.json"]);
    git(&p.local_path, &["commit", "-qm", "ours"]);
    assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);

    engine
        .do_pull(&p, SyncSource::Manual, TreeState::Unknown)
        .await
        .expect("converge");

    let owed: Vec<_> = journal_rows(&engine, &p.id)
        .into_iter()
        .filter(|(kind, _, _, payload)| kind == WorkKind::LFS_UPLOAD && payload.contains(&oid))
        .collect();
    assert_eq!(owed.len(), 1, "{owed:?}");
    let label = owed[0].2.clone().expect("labelled");
    assert!(
        label.starts_with("manifest.sync-conflict-"),
        "the upload is the conflict copy's: {label}"
    );
    assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 1);
}

/// The meeting manifest neuradrive's conflict copy points at.
const MEETING: &[u8] = b"{\"meeting\":\"coin-fund-jackson-contact\"}\n";

/// neuradrive's pointer: a conflict copy whose object the server never got,
/// in a folder whose history holds the bytes as the plain file it was copied
/// from (when `plain` is true).
fn missing_object_folder(p: &SyncProfile, plain: bool) -> (String, u64, String) {
    let bytes = MEETING;
    let oid = oid_of(bytes);
    let dir = "70-comms/meetings/2026/2026-10-06 1300 coin-fund-jackson-contact";
    let copy = format!("{dir}/manifest.sync-conflict-20261010-205450-maczek.json");
    std::fs::create_dir_all(p.local_path.join(dir)).expect("dir");
    git(&p.local_path, &["init", "-q", "-b", "main"]);
    if plain {
        std::fs::write(p.local_path.join(dir).join("manifest.json"), bytes).expect("write");
        git(&p.local_path, &["add", "."]);
        git(&p.local_path, &["commit", "-qm", "maczek's plain manifest"]);
    }
    let pointer = lfs::pointer::Pointer::new(oid.clone(), bytes.len() as u64).render();
    std::fs::write(p.local_path.join(&copy), pointer).expect("pointer");
    std::fs::write(p.local_path.join(dir).join("manifest.json"), b"{}\n").expect("canonical");
    git(&p.local_path, &["add", "."]);
    git(
        &p.local_path,
        &[
            "commit",
            "-qm",
            "sync(neuradrive): merge remote changes\n\nKeeper-Device: martas-air (01MACZEK)\nKeeper-Agent: keeper-sync/0.9.0",
        ],
    );
    (oid, bytes.len() as u64, copy)
}

fn queue_download(engine: &Engine, p: &SyncProfile, oid: &str, size: u64, label: &str) {
    let unit = WorkKind::LfsDownload {
        oid: oid.to_owned(),
        size,
    };
    engine
        .with_db(|conn| {
            let id = db::enqueue(conn, &p.id, &unit, 0, 0)?;
            db::label_unit(conn, id, label)
        })
        .expect("queue");
}

#[tokio::test]
async fn a_missing_object_is_restored_from_history_and_its_upload_queued() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    let (oid, size, copy) = missing_object_folder(&p, true);
    queue_download(&engine, &p, &oid, size, &copy);

    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");

    assert_eq!(server.batches(), vec![format!("download {oid}")]);
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    assert!(store.contains(&oid, size), "the bytes are in the store");
    let rows = journal_rows(&engine, &p.id);
    assert_eq!(
        rows.iter()
            .map(|(kind, state, label, _)| (kind.as_str(), state.as_str(), label.as_deref()))
            .collect::<Vec<_>>(),
        vec![(WorkKind::LFS_UPLOAD, "pending", Some(copy.as_str()))],
        "the download completed and the server is owed the bytes"
    );
    let activity = engine
        .with_db(|conn| db::list_activity(conn, &p.id, 10))
        .expect("activity");
    assert_eq!(activity[0].kind, db::ActivityKind::Restored);
    assert_eq!(activity[0].path, copy);
}

#[tokio::test]
async fn a_missing_object_with_no_history_names_the_file_and_waits_once_for_its_upload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(dir.path(), platform.clone(), &server) else {
        return;
    };
    let (oid, size, copy) = missing_object_folder(&p, false);
    queue_download(&engine, &p, &oid, size, &copy);
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");

    // Not stopped: the download waits for the bytes, and its row says whose.
    assert!(engine
        .with_db(|conn| db::list_parked(conn, &p.id))
        .expect("parked")
        .is_empty());
    assert_eq!(journal_rows(&engine, &p.id).len(), 1);
    let error = waiting_download_error(&engine, &p).expect("a sentence");
    assert!(
        error.contains(&copy) && error.contains("martas-air") && error.starts_with("127.0.0.1 "),
        "names the server, the file and the device that has the bytes: {error}"
    );
    assert_ne!(
        engine.status(&p.id).expect("status").state,
        ProfileState::NeedsAttention,
        "one late object is not a folder that needs a person"
    );

    // Every scan pass runs the sweep; the waiting row covers the work, and
    // its backoff keeps the server from being asked again within the minute.
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    for _ in 0..3 {
        engine.materialize_pending(&p, &store).expect("sweep");
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
    }
    assert_eq!(journal_rows(&engine, &p.id).len(), 1);
    assert_eq!(server.batches(), vec![format!("download {oid}")]);

    // maczek's upload lands; the next due attempt finishes with nobody's Retry.
    server.receive(&oid, MEETING);
    platform.advance_ms(MISSING_OBJECT_BACKOFF.max.as_millis() as i64);
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    assert!(store.contains(&oid, size), "the bytes arrived");
    assert!(journal_rows(&engine, &p.id).is_empty());
    assert_eq!(server.batches().len(), 2);
}

/// Run `sql` on the engine's own connection — where a `TEMP` trigger lives,
/// so a fault injected here is seen by every statement the engine runs.
fn inject(engine: &Engine, sql: &str) {
    engine
        .with_db(|conn| Ok(conn.execute_batch(sql)?))
        .expect("inject");
}

fn parked_ids(engine: &Engine, p: &SyncProfile) -> Vec<(i64, String)> {
    engine
        .with_db(|conn| db::list_parked(conn, &p.id))
        .expect("parked")
        .into_iter()
        .map(|row| (row.id, row.kind))
        .collect()
}

/// R503 (review R102S1-01): the upload a heal owes is journaled BEFORE its
/// bytes are published. Here the journal refuses that upload: nothing is
/// published — a published object would let the next attempt finish the
/// download and owe the server nothing — the download stops, and the
/// person's Retry heals it and the server gets the bytes.
#[tokio::test]
async fn a_heal_whose_upload_the_journal_refused_publishes_nothing_until_retry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    let (oid, size, copy) = missing_object_folder(&p, true);
    queue_download(&engine, &p, &oid, size, &copy);
    inject(
        &engine,
        "CREATE TEMP TRIGGER refuse_the_debt BEFORE INSERT ON journal
           WHEN NEW.state = 'repairing'
         BEGIN SELECT RAISE(ABORT, 'the journal refused the upload'); END;",
    );

    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    assert!(
        !store.contains(&oid, size),
        "no bytes are published while their upload is not owed"
    );
    let parked = parked_ids(&engine, &p);
    assert_eq!(
        parked
            .iter()
            .map(|(_, kind)| kind.as_str())
            .collect::<Vec<_>>(),
        vec![WorkKind::LFS_DOWNLOAD]
    );

    inject(&engine, "DROP TRIGGER refuse_the_debt;");
    engine
        .retry_parked(&p.id, parked[0].0)
        .await
        .expect("retry");
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain the retried download");
    assert!(store.contains(&oid, size));
    engine
        .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("drain the upload");
    let held = server.holds(&oid).expect("the server has the object");
    assert_eq!(oid_of(&held), oid, "byte-exact");
    assert!(
        journal_rows(&engine, &p.id).is_empty(),
        "nothing is owed any more"
    );
}

/// R503 (review R102S1-01): the process dies after the healed bytes were
/// published and before the upload they owe was released. The obligation
/// was already on disk; the first drain of the next process re-verifies the
/// stored bytes, releases the upload, and the server gets them.
#[tokio::test]
async fn a_heal_interrupted_after_its_bytes_were_stored_still_gives_the_server_its_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    let (oid, size, copy) = missing_object_folder(&p, true);
    queue_download(&engine, &p, &oid, size, &copy);
    inject(
        &engine,
        "CREATE TEMP TRIGGER die_before_the_release BEFORE UPDATE OF state ON journal
           WHEN OLD.state = 'repairing'
         BEGIN SELECT RAISE(ABORT, 'the process died here'); END;",
    );
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    assert!(store.contains(&oid, size), "the bytes were published");
    assert!(
        journal_rows(&engine, &p.id)
            .iter()
            .any(|(kind, state, _, _)| kind == WorkKind::LFS_UPLOAD && state == db::REPAIRING),
        "and the upload they owe was journaled before them"
    );
    assert_eq!(
        engine.lfs_uploads_outstanding(&p).expect("count"),
        1,
        "the push waits for it"
    );

    // The process dies; its TEMP trigger dies with its connection.
    drop(engine);
    let Ok(engine) = Engine::open(platform as Arc<dyn SyncPlatform>) else {
        return;
    };
    engine
        .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("the first drain replays the repair and runs its upload");
    let held = server.holds(&oid).expect("the server has the object");
    assert_eq!(oid_of(&held), oid, "byte-exact");
    assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);
    let activity = engine
        .with_db(|conn| db::list_activity(conn, &p.id, 10))
        .expect("activity");
    assert_eq!(activity[0].kind, db::ActivityKind::Restored);
    assert_eq!(activity[0].path, copy);
}

/// The bytes, committed plain as `drafts/standup.json`, then moved and
/// published as a pointer at a name history never held them under (R503,
/// review R102S1-04). With `impostor`, history holds a different blob of
/// exactly the same length under the draft's name instead.
fn renamed_folder(p: &SyncProfile, impostor: bool) -> (String, u64, String) {
    let bytes = b"{\"recording\":\"2026-09-30 standup\"}\n";
    let oid = oid_of(bytes);
    let moved = "40-media/recordings/2026/2026-09-30 standup/manifest.json";
    std::fs::create_dir_all(p.local_path.join("drafts")).expect("dir");
    git(&p.local_path, &["init", "-q", "-b", "main"]);
    let mut draft = bytes.to_vec();
    if impostor {
        draft[2] = b'R';
    }
    std::fs::write(p.local_path.join("drafts/standup.json"), &draft).expect("draft");
    git(&p.local_path, &["add", "."]);
    git(&p.local_path, &["commit", "-qm", "draft"]);
    git(&p.local_path, &["rm", "-q", "drafts/standup.json"]);
    std::fs::create_dir_all(p.local_path.join(moved).parent().expect("parent")).expect("dir");
    let pointer = lfs::pointer::Pointer::new(oid.clone(), bytes.len() as u64).render();
    std::fs::write(p.local_path.join(moved), pointer).expect("pointer");
    git(&p.local_path, &["add", "."]);
    git(&p.local_path, &["commit", "-qm", "moved"]);
    (oid, bytes.len() as u64, moved.to_owned())
}

#[tokio::test]
async fn a_missing_object_committed_under_an_earlier_name_is_restored_from_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    let (oid, size, moved) = renamed_folder(&p, false);
    queue_download(&engine, &p, &oid, size, &moved);
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    assert!(store.contains(&oid, size), "found under its earlier name");
    assert_eq!(
        journal_rows(&engine, &p.id)
            .iter()
            .map(|(kind, state, label, _)| (kind.as_str(), state.as_str(), label.as_deref()))
            .collect::<Vec<_>>(),
        vec![(WorkKind::LFS_UPLOAD, "pending", Some(moved.as_str()))]
    );
}

#[tokio::test]
async fn a_blob_of_the_same_length_with_another_hash_is_not_taken_for_the_missing_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    let (oid, size, moved) = renamed_folder(&p, true);
    queue_download(&engine, &p, &oid, size, &moved);
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    assert!(!store.contains(&oid, size));
    let error = waiting_download_error(&engine, &p).expect("a sentence");
    assert!(
        error.contains(&moved) && error.contains("history does not hold it"),
        "{error}"
    );
}

/// The sentence on the one download still waiting for the server's object.
fn waiting_download_error(engine: &Engine, p: &SyncProfile) -> Option<String> {
    engine
        .with_db(|conn| {
            Ok(conn.query_row(
                "SELECT last_error FROM journal
                  WHERE profile_id = ?1 AND kind = ?2 AND state = 'pending'",
                (&p.id, WorkKind::LFS_DOWNLOAD),
                |r| r.get(0),
            )?)
        })
        .expect("one waiting download")
}

/// [`git::history::HISTORY_SEARCH_OBJECTS`]'s resumption: with a bound of
/// one object per attempt, each attempt starts where the last stopped — the
/// download is retried, not stopped — until the bytes are found.
#[tokio::test]
async fn a_bounded_history_search_resumes_where_it_stopped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((mut engine, p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    engine.history_search_objects = 1;
    let (oid, size, moved) = renamed_folder(&p, false);
    queue_download(&engine, &p, &oid, size, &moved);
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    // MAX over the download's row: one row whatever is queued, NULL once the
    // row is gone or carries no place.
    let cursor = |engine: &Engine| -> Option<String> {
        engine
            .with_db(|conn| {
                Ok(conn.query_row(
                    "SELECT MAX(heal_after) FROM journal WHERE kind = ?1",
                    [WorkKind::LFS_DOWNLOAD],
                    |r| r.get::<_, Option<String>>(0),
                )?)
            })
            .expect("cursor")
    };
    let mut attempts = 0;
    let mut last: Option<String> = None;
    while !store.contains(&oid, size) {
        attempts += 1;
        assert!(attempts < 200, "the search never reached the bytes");
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
        assert!(
            parked_ids(&engine, &p).is_empty(),
            "a stopped search is retried, never stopped"
        );
        if let Some(now) = cursor(&engine) {
            assert!(
                last.as_ref().is_none_or(|before| now > *before),
                "resumes forward"
            );
            last = Some(now);
        }
        inject(&engine, "UPDATE journal SET not_before_ms = 0;");
    }
    assert!(attempts > 1, "the bound stopped at least one attempt");
}

/// R503 (review R102S1-05): Retry all folds a person's request into the
/// background download of the same object, and the request survives the
/// fold — under pointer-only, the file is actually delivered.
#[tokio::test]
async fn retry_all_keeps_the_request_a_folded_download_carried() {
    let bytes = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let oid = oid_of(bytes);
    let size = bytes.len() as u64;
    let server = LfsServer::start_with(vec![(oid.clone(), bytes.to_vec())]);
    let dir = tempfile::tempdir().expect("tempdir");
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, mut p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    p.lfs_mode = LfsMode::PointerOnly;
    engine.upsert_profile(&p).expect("pointer-only");
    git(&p.local_path, &["init", "-q", "-b", "main"]);
    let pointer = lfs::pointer::Pointer::new(oid.clone(), size).render();
    std::fs::write(p.local_path.join("clip.json"), &pointer).expect("pointer");
    git(&p.local_path, &["add", "."]);
    git(&p.local_path, &["commit", "-qm", "clip"]);
    let unit = WorkKind::LfsDownload {
        oid: oid.clone(),
        size,
    };
    // The background download stopped first; the person's request queued a
    // second row of the same object (a stopped row is no cover for a
    // request), and it stopped too.
    engine
        .with_db(|conn| {
            for urgency in [None, Some(db::Urgency::Requested.level())] {
                let id = db::enqueue(conn, &p.id, &unit, 0, 0)?;
                db::label_unit(conn, id, "clip.json")?;
                conn.execute(
                    "UPDATE journal SET urgency = ?2 WHERE id = ?1",
                    (id, urgency),
                )?;
                db::reschedule_failed(
                    conn,
                    id,
                    WorkState::Parked,
                    0,
                    &SyncError::Credential(SIGNED_OUT.to_owned()),
                )?;
            }
            Ok(())
        })
        .expect("two stopped rows of one download");

    assert_eq!(engine.retry_all_parked(&p.id).await.expect("retry all"), 2);
    assert_eq!(journal_rows(&engine, &p.id).len(), 1, "folded into one");
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    assert_eq!(
        std::fs::read(p.local_path.join("clip.json")).expect("read"),
        bytes.to_vec(),
        "the requested file is delivered, not left a pointer"
    );
}

/// R503 (review R102S1-02): the merge's upload inventory is what it stages,
/// not what it hands `add`. The other side added a pointer whose object this
/// machine holds and the server may not; git staged it on its own, without
/// any conflict. The merge owes its upload, its push waits for it, and the
/// merge is published once the upload has landed.
#[tokio::test]
async fn a_merge_owes_the_upload_of_a_pointer_git_staged_on_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    let Some(p) = publishing_fixture(&engine, &platform, dir.path(), remote_dir.path()).await
    else {
        return;
    };
    let bytes = b"{\"photo\":\"kalypso 2026-10-01\"}\n";
    let oid = oid_of(bytes);
    let size = bytes.len() as u64;
    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    store
        .insert_verified(&oid, &bytes[..], size)
        .expect("store");
    let pointer = lfs::pointer::Pointer::new(oid.clone(), size).render();
    advance_remote_with(
        remote_dir.path(),
        &[("photo.json", pointer.as_str()), ("root.md", "root")],
        "theirs",
    );
    std::fs::write(p.local_path.join("notes.md"), b"ours\n").expect("write");
    git(&p.local_path, &["add", "notes.md"]);
    git(&p.local_path, &["commit", "-qm", "ours"]);

    engine
        .do_pull(&p, SyncSource::Manual, TreeState::Unknown)
        .await
        .expect("converge");
    let owed: Vec<_> = journal_rows(&engine, &p.id)
        .into_iter()
        .filter(|(kind, _, _, payload)| kind == WorkKind::LFS_UPLOAD && payload.contains(&oid))
        .collect();
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!(owed[0].2.as_deref(), Some("photo.json"));
    push_waits_then_publishes(&engine, &p, remote_dir.path(), &[(&oid, size)]).await;
}

/// The held push, the upload landing in the filesystem remote's store, and
/// the merge published after it.
async fn push_waits_then_publishes(
    engine: &Engine,
    p: &SyncProfile,
    remote_dir: &Path,
    objects: &[(&str, u64)],
) {
    let before = remote_main(remote_dir);
    let held = engine
        .do_push(p, SyncSource::Manual, None, TreeState::Committed)
        .await;
    assert!(
        matches!(held, Err(SyncError::LfsUploadPending { .. })),
        "the push waits for the merge's uploads: {held:?}"
    );
    assert_eq!(remote_main(remote_dir), before, "nothing was published");
    engine
        .drain_kind(p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("upload");
    let remote_store = lfs::local::remote_store(&p.remote_url).expect("a filesystem remote");
    for (oid, size) in objects {
        assert!(
            remote_store.contains(oid, *size),
            "{oid} reached the remote"
        );
    }
    engine
        .do_push(p, SyncSource::Manual, None, TreeState::Committed)
        .await
        .expect("publish");
    let local_head = git_out(&p.local_path, &["rev-parse", "HEAD"]);
    assert_eq!(
        remote_main(remote_dir),
        local_head.trim(),
        "the merge is published"
    );
}

/// `git <args>` in `dir`, its stdout.
fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// keeper's real filter: the `lfs-filter-harness` binary `cargo test` builds
/// beside this test binary (`tests/lfs_filter_process.rs` uses the same one).
fn filter_harness() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let target = exe
        .parent()
        .and_then(Path::parent)
        .expect("the target directory");
    let harness = target.join(format!(
        "lfs-filter-harness{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        harness.is_file(),
        "{} is built by `cargo test -p keeper-sync`",
        harness.display()
    );
    harness
}

/// R503 (review R102S1-02), Marta's merge as it happened: the folder routes
/// JSON through LFS, this machine's `manifest.json` is a pointer whose bytes
/// never left it, and the other side changed the same file. The conflict
/// copy is written as the plain JSON on disk, the merge's `add` turns it into
/// a pointer through keeper's real clean filter, the merge owes its upload,
/// and the push publishes the merge only after the bytes have landed.
#[tokio::test]
async fn a_conflict_copy_the_clean_filter_makes_a_pointer_is_uploaded_before_the_merge_is_published(
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = tempfile::tempdir().expect("tempdir");
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Ok(mut engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    engine.filter_program = Some(filter_harness());
    engine.filter_serves_process = true;
    let Some(p) = publishing_fixture(&engine, &platform, dir.path(), remote_dir.path()).await
    else {
        return;
    };
    advance_remote_with(
        remote_dir.path(),
        &[("manifest.json", "{\"theirs\":1}\n"), ("root.md", "root")],
        "theirs",
    );
    std::fs::write(
        p.local_path.join(".gitattributes"),
        "*.json filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("rule");
    git(&p.local_path, &["add", ".gitattributes"]);
    git(&p.local_path, &["commit", "-qm", "route json through lfs"]);
    let ours = b"{\"ours\":\"maczek\"}\n";
    std::fs::write(p.local_path.join("manifest.json"), ours).expect("write");
    git(&p.local_path, &["add", "manifest.json"]);
    git(&p.local_path, &["commit", "-qm", "ours"]);
    let committed = git_out(&p.local_path, &["show", "HEAD:manifest.json"]);
    assert!(
        committed.starts_with("version https://git-lfs"),
        "the real filter made this machine's manifest a pointer: {committed}"
    );
    assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);

    engine
        .do_pull(&p, SyncSource::Manual, TreeState::Unknown)
        .await
        .expect("converge");
    let copy = journal_rows(&engine, &p.id)
        .into_iter()
        .filter(|(kind, _, _, _)| kind == WorkKind::LFS_UPLOAD)
        .find_map(|(_, _, label, _)| label.filter(|l| l.starts_with("manifest.sync-conflict-")))
        .expect("the conflict copy's upload is owed");
    let indexed = git_out(&p.local_path, &["show", &format!(":{copy}")]);
    let pointer = lfs::pointer::Pointer::parse(indexed.as_bytes())
        .expect("the copy went into the merge as a pointer, through the filter");
    assert_eq!(pointer.oid, oid_of(ours), "of the plain JSON on disk");
    push_waits_then_publishes(
        &engine,
        &p,
        remote_dir.path(),
        &[(&pointer.oid, pointer.size)],
    )
    .await;
}

/// A second connection to the engine's `sync.db`: what a fault, a barrier
/// or a witness reads and writes from outside the engine's own.
fn second_connection(platform: &TestPlatform) -> rusqlite::Connection {
    db::open(&platform.data_dir().expect("data dir")).expect("a second connection")
}

/// Every sync a durable publication makes, with the state of the folder's
/// upload row at that moment, read from outside the engine (R514).
struct Witness {
    journal: Mutex<rusqlite::Connection>,
    calls: Mutex<Vec<(String, String)>>,
}

impl Witness {
    fn note(&self, what: String) {
        let state: String = Engine::lock(&self.journal)
            .query_row(
                "SELECT COALESCE(MAX(state), 'none') FROM journal WHERE kind = ?1",
                [WorkKind::LFS_UPLOAD],
                |r| r.get(0),
            )
            .expect("the upload's state");
        Engine::lock(&self.calls).push((what, state));
    }
}

impl lfs::store::Durability for Witness {
    fn sync_file(&self, file: &std::fs::File, path: &Path) -> std::io::Result<()> {
        self.note(format!("file {}", path.display()));
        file.sync_all()
    }
    fn sync_dir(&self, dir: &Path) -> std::io::Result<()> {
        self.note(format!("dir {}", dir.display()));
        lfs::store::OsDurability.sync_dir(dir)
    }
}

/// R514 (review R102S2-01): the healed object is on disk — the bytes synced
/// before the rename, the directories naming them after it — while its
/// repair intent still stands, and only then is the intent retired.
#[tokio::test]
async fn a_healed_object_is_on_disk_before_its_repair_is_released() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((mut engine, p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    let witness = Arc::new(Witness {
        journal: Mutex::new(second_connection(&platform)),
        calls: Mutex::new(Vec::new()),
    });
    engine.durability = Arc::clone(&witness) as Arc<dyn lfs::store::Durability>;
    let (oid, size, copy) = missing_object_folder(&p, true);
    queue_download(&engine, &p, &oid, size, &copy);
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");

    let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
    let object = store.object_path(&oid);
    let calls = Engine::lock(&witness.calls).clone();
    let tmp = p.local_path.join(".git/lfs/tmp");
    assert!(
        calls
            .first()
            .is_some_and(|(what, _)| what.starts_with(&format!("file {}", tmp.display()))),
        "the staged bytes are synced before they are renamed into place: {calls:?}"
    );
    let dirs: Vec<&str> = calls
        .iter()
        .filter_map(|(what, _)| what.strip_prefix("dir "))
        .collect();
    let objects = p.local_path.join(".git/lfs/objects");
    assert_eq!(
        dirs.first().copied(),
        object.parent().map(|d| d.display().to_string()).as_deref(),
        "the rename's directory is synced: {calls:?}"
    );
    assert_eq!(
        dirs.last().copied(),
        Some(objects.display().to_string().as_str()),
        "and every shard up to the store's objects: {calls:?}"
    );
    assert!(
        calls.iter().all(|(_, state)| state == db::REPAIRING),
        "every sync happens while the repair is still owed: {calls:?}"
    );
    assert_eq!(
        journal_rows(&engine, &p.id)
            .iter()
            .map(|(kind, state, _, _)| (kind.as_str(), state.as_str()))
            .collect::<Vec<_>>(),
        vec![(WorkKind::LFS_UPLOAD, "pending")],
        "and then it is released"
    );
}

/// R514 (review R102S2-01): after the repair was released, the healed
/// object is gone from the store — absent, or cut short — and the process
/// restarts. The upload rebuilds it from history and the server gets it.
#[tokio::test]
async fn a_released_heal_whose_object_went_missing_is_rebuilt_from_history_after_a_restart() {
    for truncated in [false, true] {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = LfsServer::start(Vec::new());
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Some((engine, p)) = engine_with_server(
            dir.path(),
            Arc::clone(&platform) as Arc<dyn SyncPlatform>,
            &server,
        ) else {
            return;
        };
        let (oid, size, copy) = missing_object_folder(&p, true);
        queue_download(&engine, &p, &oid, size, &copy);
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
        assert_eq!(
            journal_rows(&engine, &p.id)
                .iter()
                .map(|(kind, state, _, _)| (kind.as_str(), state.as_str()))
                .collect::<Vec<_>>(),
            vec![(WorkKind::LFS_UPLOAD, "pending")],
            "the repair was released"
        );
        let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
        let object = store.object_path(&oid);
        if truncated {
            std::fs::write(&object, &MEETING[..MEETING.len() / 2]).expect("cut short");
        } else {
            std::fs::remove_file(&object).expect("gone");
        }

        drop(engine);
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        engine
            .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
            .await
            .expect("drain the upload");
        let held = server
            .holds(&oid)
            .unwrap_or_else(|| panic!("the server has the object (truncated: {truncated})"));
        assert_eq!(held, MEETING, "byte-exact (truncated: {truncated})");
        assert_eq!(store.verify(&oid, size), lfs::store::Stored::Verified);
        assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);
    }
}

/// R514 (review R102S2-02): a healed object whose upload is still owed
/// cannot be read. It is kept — the same file, untouched — whether history
/// could supply the bytes again or not; the upload stops with the read's
/// own sentence; once the file reads, a Retry finishes the repair and the
/// server gets the bytes.
#[tokio::test]
async fn an_unreadable_healed_object_is_kept_and_its_upload_waits_until_it_reads() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    for history in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = LfsServer::start(Vec::new());
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Some((engine, p)) = engine_with_server(
            dir.path(),
            Arc::clone(&platform) as Arc<dyn SyncPlatform>,
            &server,
        ) else {
            return;
        };
        let (oid, size, copy) = missing_object_folder(&p, history);
        // A heal as an interruption leaves it: the upload owed, the bytes in
        // the store.
        let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
        store.insert_verified(&oid, MEETING, size).expect("stored");
        let upload = WorkKind::LfsUpload {
            oid: oid.clone(),
            size,
        };
        engine
            .with_db(|conn| db::owe_repair(conn, &p.id, &upload, &copy, 0).map(|_| ()))
            .expect("owed");
        let object = store.object_path(&oid);
        let inode = std::fs::metadata(&object).expect("stat").ino();
        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o000))
            .expect("unreadable");
        if std::fs::File::open(&object).is_ok() {
            // Root reads anything: there is no unreadable file to keep here.
            return;
        }

        engine
            .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
            .await
            .expect("drain");
        let kept = std::fs::metadata(&object).expect("the object is still there");
        assert_eq!(
            kept.ino(),
            inode,
            "the same file is kept (history: {history})"
        );
        assert_eq!(
            kept.permissions().mode() & 0o777,
            0,
            "untouched (history: {history})"
        );
        let parked = engine
            .with_db(|conn| db::list_parked(conn, &p.id))
            .expect("parked");
        assert_eq!(parked.len(), 1, "the upload stops (history: {history})");
        let error = parked[0].last_error.clone().expect("a sentence");
        assert!(
            error.contains("denied") && !error.contains("history"),
            "says it could not read the file, not that it is gone: {error}"
        );
        assert!(server.holds(&oid).is_none(), "nothing unverified was sent");

        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o600))
            .expect("readable again");
        assert_eq!(engine.retry_all_parked(&p.id).await.expect("retry all"), 1);
        engine
            .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
            .await
            .expect("drain the retried repair");
        assert_eq!(
            server.holds(&oid).as_deref(),
            Some(MEETING),
            "byte-exact (history: {history})"
        );
        assert_eq!(std::fs::metadata(&object).expect("stat").ino(), inode);
    }
}

/// R514 (review R102S2-03): after the last upload parked and the held
/// push said why, a download and then a pull fail — and no unit succeeds
/// after them. The held push's sentence, its state and its stopped upload
/// (the card's "Retry all stopped") stay. The push itself is not failed
/// again.
#[tokio::test]
async fn failures_after_the_last_upload_parked_keep_the_held_push_and_its_retry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some((_account, engine, p, _server, _oid, _push)) = stopped_upload(dir.path()).await else {
        return;
    };
    let held = engine.status(&p.id).expect("status").error.expect("held");
    assert!(is_held_push(&held), "{held}");

    let other = WorkKind::LfsDownload {
        oid: oid_of(b"another object"),
        size: 14,
    };
    engine
        .with_db(|conn| db::enqueue(conn, &p.id, &other, 0, 0).map(|_| ()))
        .expect("queue a download");
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    engine
        .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Pull, 0, 0).map(|_| ()))
        .expect("queue a pull");
    engine
        .drain_kind(&p, WorkKind::PULL, SyncSource::Watch)
        .await
        .expect("drain");

    let rows = journal_rows(&engine, &p.id);
    for kind in [WorkKind::LFS_DOWNLOAD, WorkKind::PULL] {
        assert!(
            rows.iter()
                .any(|(k, state, _, _)| k == kind && state != "running"),
            "the {kind} failed and stands: {rows:?}"
        );
    }
    let status = engine.status(&p.id).expect("status");
    assert_eq!(status.state, ProfileState::NeedsAttention);
    let error = status.error.expect("a held push says why");
    assert!(
        is_held_push(&error),
        "the held push outranks the failures: {error}"
    );
    assert!(
        error.contains("40-media/recordings/2026/test/manifest.json"),
        "{error}"
    );
    let parked = engine
        .with_db(|conn| db::list_parked(conn, &p.id))
        .expect("parked");
    assert_eq!(
        parked[0].kind,
        WorkKind::LFS_UPLOAD,
        "Retry all stopped has its upload"
    );
}

/// R514 (review R102S2-03): the push is already held; the only upload left
/// is a heal's repair, and its replay stops it — nothing in the store,
/// nothing in history. The drain claims nothing after that, and the held
/// push says why all the same, naming the file. The push is not failed
/// again.
#[tokio::test]
async fn a_repair_that_stops_behind_a_held_push_says_why() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = LfsServer::start(Vec::new());
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, p)) = engine_with_server(dir.path(), platform, &server) else {
        return;
    };
    let (oid, size, copy) = missing_object_folder(&p, false);
    let upload = WorkKind::LfsUpload { oid, size };
    engine
        .with_db(|conn| {
            db::owe_repair(conn, &p.id, &upload, &copy, 0)?;
            let push = db::enqueue(conn, &p.id, &WorkKind::Push, 0, 0)?;
            db::reschedule(conn, push, WorkState::Deferred, 0, None)
        })
        .expect("a repair owed and a push held behind it");
    assert!(engine.status(&p.id).expect("status").error.is_none());

    engine
        .drain_kind(&p, WorkKind::LFS_UPLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    let status = engine.status(&p.id).expect("status");
    assert_eq!(status.state, ProfileState::NeedsAttention);
    let error = status.error.expect("the held push says why");
    assert!(is_held_push(&error) && error.contains(&copy), "{error}");
    assert!(
        engine
            .with_db(|conn| db::has_unit_in(conn, &p.id, WorkKind::PUSH, "deferred"))
            .expect("read"),
        "the push still waits"
    );
}

/// A pointer-only (or virtual-pattern) folder holding one object at two
/// paths, with a background download of it for `a.json` queued and a
/// person's request for `b.json` stopped.
fn two_paths_one_object(
    engine: &Engine,
    p: &SyncProfile,
    a_running: bool,
) -> (String, u64, &'static [u8]) {
    let bytes: &'static [u8] = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let oid = oid_of(bytes);
    let size = bytes.len() as u64;
    git(&p.local_path, &["init", "-q", "-b", "main"]);
    let pointer = lfs::pointer::Pointer::new(oid.clone(), size).render();
    for name in ["a.json", "b.json"] {
        std::fs::write(p.local_path.join(name), &pointer).expect("pointer");
    }
    git(&p.local_path, &["add", "."]);
    git(&p.local_path, &["commit", "-qm", "clip twice"]);
    let unit = WorkKind::LfsDownload {
        oid: oid.clone(),
        size,
    };
    engine
        .with_db(|conn| {
            let a = db::enqueue(conn, &p.id, &unit, 0, 0)?;
            db::label_unit(conn, a, "a.json")?;
            if a_running {
                conn.execute("UPDATE journal SET state = 'running' WHERE id = ?1", [a])?;
            }
            conn.execute(
                "INSERT INTO journal (profile_id, kind, payload, state, not_before_ms, created_ms,
                                      label, urgency)
                 SELECT profile_id, kind, payload, ?2, 0, 0, 'b.json', ?3 FROM journal WHERE id = ?1",
                (
                    a,
                    if a_running { "pending" } else { "parked" },
                    db::Urgency::Requested.level(),
                ),
            )?;
            Ok(())
        })
        .expect("a background download and a request for the other path");
    (oid, size, bytes)
}

/// R514 (review R102S2-04): Retry all runs while the background download
/// for `a.json` is in flight. `b.json` is virtual under the folder's
/// policy, so only the request exempts it; the running unit carries the
/// label it claimed. The request stays its own work and `b.json` is
/// delivered.
#[tokio::test]
async fn retry_all_during_a_running_download_keeps_the_requested_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes: &[u8] = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let server = LfsServer::start_with(vec![(oid_of(bytes), bytes.to_vec())]);
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, mut p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    p.virtual_patterns = vec!["b.json".to_owned()];
    engine.upsert_profile(&p).expect("b.json is virtual");
    let (_, _, bytes) = two_paths_one_object(&engine, &p, false);
    let journal = second_connection(&platform);
    let profile_id = p.id.clone();
    server.during_next_download(move || {
        let done = db::unpark_all(&journal, &profile_id, 0).expect("retry all");
        assert_eq!(done.total(), 1);
    });

    for _ in 0..2 {
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
    }
    assert_eq!(
        std::fs::read(p.local_path.join("b.json")).expect("read"),
        bytes,
        "the requested path is delivered, virtual or not"
    );
    assert!(journal_rows(&engine, &p.id).is_empty());
}

/// R514 (review R102S2-04): pointer-only. Retry all lands after the running
/// download's delivery read its row and before the row is completed. The
/// request stays its own work and `b.json` is delivered.
#[tokio::test]
async fn retry_all_between_delivery_and_completion_keeps_the_request() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes: &[u8] = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let server = LfsServer::start_with(vec![(oid_of(bytes), bytes.to_vec())]);
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, mut p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    p.lfs_mode = LfsMode::PointerOnly;
    engine.upsert_profile(&p).expect("pointer-only");
    let (_, _, bytes) = two_paths_one_object(&engine, &p, false);
    let journal = second_connection(&platform);
    let profile_id = p.id.clone();
    super::super::before_complete::install(&p.local_path, move || {
        let done = db::unpark_all(&journal, &profile_id, 0).expect("retry all");
        assert_eq!(done.total(), 1);
    });

    for _ in 0..2 {
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
    }
    assert_eq!(
        std::fs::read(p.local_path.join("b.json")).expect("read"),
        bytes,
        "the requested file is delivered, not left a pointer"
    );
}

/// R514: a requested row requeued beside its running cover survives a
/// restart. The cover comes back `pending` and older; collapsing the pair
/// keeps the request on the row that stays.
#[tokio::test]
async fn a_request_beside_a_running_cover_survives_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes: &[u8] = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let server = LfsServer::start_with(vec![(oid_of(bytes), bytes.to_vec())]);
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, mut p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    p.lfs_mode = LfsMode::PointerOnly;
    engine.upsert_profile(&p).expect("pointer-only");
    let (_, _, bytes) = two_paths_one_object(&engine, &p, true);

    drop(engine);
    let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    assert_eq!(journal_rows(&engine, &p.id).len(), 1, "collapsed into one");
    engine
        .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
        .await
        .expect("drain");
    assert_eq!(
        std::fs::read(p.local_path.join("b.json")).expect("read"),
        bytes,
        "the request survived the restart"
    );
}

/// [`two_paths_one_object`] with `a.json` a person's request too, in
/// `a_state`: two requests for two paths that share one object.
fn two_requests_one_object(
    engine: &Engine,
    p: &SyncProfile,
    a_running: bool,
    a_state: &str,
) -> &'static [u8] {
    let (_, _, bytes) = two_paths_one_object(engine, p, a_running);
    engine
        .with_db(|conn| {
            conn.execute(
                "UPDATE journal SET urgency = ?1, state = ?2 WHERE label = 'a.json'",
                (db::Urgency::Requested.level(), a_state),
            )?;
            Ok(())
        })
        .expect("a.json requested too");
    bytes
}

/// R517 (review R102S3-03): both requests stopped, then Retry all. Both
/// paths are virtual, so each is delivered only on its own request; the
/// two stay two rows, and the second is published from the store the first
/// filled.
#[tokio::test]
async fn retry_all_keeps_two_requests_for_one_object_apart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes: &[u8] = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let server = LfsServer::start_with(vec![(oid_of(bytes), bytes.to_vec())]);
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, mut p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    p.virtual_patterns = vec!["a.json".to_owned(), "b.json".to_owned()];
    engine.upsert_profile(&p).expect("both paths are virtual");
    let bytes = two_requests_one_object(&engine, &p, false, "parked");

    engine.retry_all_parked(&p.id).await.expect("retry all");
    for _ in 0..2 {
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
    }
    for name in ["a.json", "b.json"] {
        assert_eq!(
            std::fs::read(p.local_path.join(name)).expect("read"),
            bytes,
            "{name} is delivered on its own request"
        );
    }
    assert!(journal_rows(&engine, &p.id).is_empty());
}

/// R517: the restart variant. `a.json`'s request was running when the
/// process died and `b.json`'s waited beside it; startup's collapse keeps
/// both, and both are delivered.
#[tokio::test]
async fn two_requests_for_one_object_both_survive_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes: &[u8] = b"{\"clip\":\"2026-10-12 walk\"}\n";
    let server = LfsServer::start_with(vec![(oid_of(bytes), bytes.to_vec())]);
    let platform = Arc::new(TestPlatform::new(dir.path()));
    let Some((engine, mut p)) = engine_with_server(
        dir.path(),
        Arc::clone(&platform) as Arc<dyn SyncPlatform>,
        &server,
    ) else {
        return;
    };
    p.virtual_patterns = vec!["a.json".to_owned(), "b.json".to_owned()];
    engine.upsert_profile(&p).expect("both paths are virtual");
    let bytes = two_requests_one_object(&engine, &p, true, "running");

    drop(engine);
    let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    for _ in 0..2 {
        engine
            .drain_kind(&p, WorkKind::LFS_DOWNLOAD, SyncSource::Watch)
            .await
            .expect("drain");
    }
    for name in ["a.json", "b.json"] {
        assert_eq!(
            std::fs::read(p.local_path.join(name)).expect("read"),
            bytes,
            "{name}'s request survived the restart"
        );
    }
}

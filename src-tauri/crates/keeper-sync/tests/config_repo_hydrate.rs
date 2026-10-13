//! `config_repo::hydrate_lfs_dir` against a loopback LFS server (AD-341).
//!
//! The "clone" is a plain directory holding pointer files, which is exactly
//! what a gix copy of an LFS-tracked tree is. The server speaks the batch API
//! and the `basic` download, answers 401 to any request whose `Authorization`
//! is not the configured bearer, and counts what it was asked, so the tests can
//! say not only what landed but what crossed the wire to get it there.

use std::{
    collections::HashMap,
    io::{Read, Write as _},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use keeper_sync::config_repo::{
    completion_digest, hydrate_complete_file, hydrate_lfs_dir, hydrate_state_file,
    hydration_is_current, Folders, HydrateGroup, HydrateReport, Manifest, RepoAuth,
};
use keeper_sync::error::SyncError;
use keeper_sync::lfs::{pointer::Pointer, store::LfsStore};

const TOKEN: &str = "eyJ.config";

/// The whole directory, as one group.
const ALL: HydrateGroup<'static> = HydrateGroup {
    name: "all",
    folders: Folders::AllBut(&[]),
    manifest: None,
};

/// The lines of `models.toml`'s `wanted` sections, as keeper-core's
/// fingerprints read it: what one group depends on.
fn sections(raw: &[u8], wanted: &[&str]) -> Option<String> {
    let raw = std::str::from_utf8(raw).ok()?;
    let mut taking = false;
    let mut kept = String::new();
    for line in raw.lines() {
        if let Some(section) = line.strip_prefix('[') {
            taking = wanted.iter().any(|name| section == format!("{name}]"));
        }
        if taking {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    Some(kept)
}

fn transcription_manifest(raw: &[u8]) -> Option<String> {
    sections(raw, &["asr", "diarizer", "embedding"])
}

fn turn_manifest(raw: &[u8]) -> Option<String> {
    sections(raw, &["vad", "smart_turn"])
}

/// The transcription and turn groups over `turn_dirs`, each reading its own
/// sections of `models.toml`.
fn role_groups(turn_dirs: &[String]) -> (HydrateGroup<'_>, HydrateGroup<'_>) {
    (
        HydrateGroup {
            name: "transcription",
            folders: Folders::AllBut(turn_dirs),
            manifest: Some(Manifest {
                file: "models.toml",
                fingerprint: transcription_manifest,
            }),
        },
        HydrateGroup {
            name: "turn",
            folders: Folders::Only(turn_dirs),
            manifest: Some(Manifest {
                file: "models.toml",
                fingerprint: turn_manifest,
            }),
        },
    )
}

fn oid_of(bytes: &[u8]) -> String {
    LfsStore::digest_of(bytes).expect("hash").0
}

fn pointer_text(bytes: &[u8]) -> String {
    Pointer::new(oid_of(bytes), bytes.len() as u64).render()
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

/// A loopback LFS server holding `objects` (oid → the bytes it serves).
struct Server {
    remote_url: String,
    /// Objects named by each batch request, in arrival order.
    batches: Arc<Mutex<Vec<usize>>>,
    downloads: Arc<AtomicUsize>,
}

impl Server {
    fn start(objects: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let batches = Arc::new(Mutex::new(Vec::new()));
        let downloads = Arc::new(AtomicUsize::new(0));
        let objects = Arc::new(objects);
        {
            let batches = Arc::clone(&batches);
            let downloads = Arc::clone(&downloads);
            std::thread::spawn(move || {
                while let Ok((stream, _)) = listener.accept() {
                    let batches = Arc::clone(&batches);
                    let downloads = Arc::clone(&downloads);
                    let objects = Arc::clone(&objects);
                    std::thread::spawn(move || {
                        answer(stream, port, &objects, &batches, &downloads);
                    });
                }
            });
        }
        Self {
            remote_url: format!("http://127.0.0.1:{port}/keeper/config.git"),
            batches,
            downloads,
        }
    }

    fn batches(&self) -> Vec<usize> {
        self.batches.lock().expect("lock").clone()
    }

    fn downloads(&self) -> usize {
        self.downloads.load(Ordering::SeqCst)
    }
}

fn answer(
    mut stream: TcpStream,
    port: u16,
    objects: &HashMap<String, Vec<u8>>,
    batches: &Mutex<Vec<usize>>,
    downloads: &AtomicUsize,
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
    let mut lines = head.lines();
    let request = lines.next().unwrap_or_default().to_owned();
    let mut length = 0usize;
    let mut authorization = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse().unwrap_or(0),
            "authorization" => authorization = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    let mut body = vec![0u8; length];
    if stream.read_exact(&mut body).is_err() {
        return;
    }

    let respond = |stream: &mut TcpStream, status: &str, kind: &str, payload: &[u8]| {
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
                 WWW-Authenticate: Basic realm=\"lfs\"\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        let _ = stream.write_all(payload);
        let _ = stream.flush();
    };

    if authorization.as_deref() != Some(&format!("Bearer {TOKEN}")) {
        respond(
            &mut stream,
            "401 Unauthorized",
            "application/vnd.git-lfs+json",
            br#"{"message":"credentials needed"}"#,
        );
        return;
    }

    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method == "POST" && target == "/keeper/config.git/info/lfs/objects/batch" {
        let asked: serde_json::Value = serde_json::from_slice(&body).expect("batch json");
        let asked = asked["objects"].as_array().cloned().unwrap_or_default();
        batches.lock().expect("lock").push(asked.len());
        let answered: Vec<serde_json::Value> = asked
            .iter()
            .map(|object| {
                let oid = object["oid"].as_str().unwrap_or_default();
                serde_json::json!({
                    "oid": oid,
                    "size": object["size"],
                    "actions": { "download": {
                        "href": format!("http://127.0.0.1:{port}/objects/{oid}")
                    } }
                })
            })
            .collect();
        let payload =
            serde_json::to_vec(&serde_json::json!({ "objects": answered })).expect("json");
        respond(
            &mut stream,
            "200 OK",
            "application/vnd.git-lfs+json",
            &payload,
        );
    } else if let Some(oid) = target.strip_prefix("/objects/").filter(|_| method == "GET") {
        downloads.fetch_add(1, Ordering::SeqCst);
        match objects.get(oid) {
            Some(bytes) => respond(&mut stream, "200 OK", "application/octet-stream", bytes),
            None => respond(&mut stream, "404 Not Found", "text/plain", b"gone"),
        }
    } else {
        respond(&mut stream, "404 Not Found", "text/plain", b"no route");
    }
}

fn bearer() -> RepoAuth {
    RepoAuth::Bearer(TOKEN.to_owned())
}

async fn hydrate(
    server: &Server,
    clone: &Path,
    auth: &RepoAuth,
    dest: &Path,
) -> keeper_sync::error::Result<HydrateReport> {
    hydrate_group(server, clone, ALL, auth, dest).await
}

async fn hydrate_group(
    server: &Server,
    clone: &Path,
    group: HydrateGroup<'_>,
    auth: &RepoAuth,
    dest: &Path,
) -> keeper_sync::error::Result<HydrateReport> {
    hydrate_lfs_dir(
        &reqwest::Client::new(),
        clone,
        "_models",
        group,
        &server.remote_url,
        auth,
        dest,
        &AtomicBool::new(false),
    )
    .await
}

fn encoder() -> Vec<u8> {
    (0..200_000u32).map(|i| (i % 251) as u8).collect()
}

const VOCAB: &[u8] = br#"{"0":"<unk>","1":"hello"}"#;
const MODELS_TOML: &[u8] = b"[asr]\ndir = \"parakeet\"\n";

/// A tree of pointers and plain files: pointers become their objects, plain
/// files are copied, one object backing two paths is asked for once, nothing
/// outside `_models` is read, a symbolic link is not followed — and a second
/// run finds everything in place without a single request.
#[tokio::test]
async fn a_tree_is_hydrated_once_and_then_found_in_place() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    let encoder = encoder();
    write(
        &clone.join("_models/parakeet/Encoder.mlmodelc/weights.bin"),
        pointer_text(&encoder).as_bytes(),
    );
    write(
        &clone.join("_models/parakeet/vocab.json"),
        pointer_text(VOCAB).as_bytes(),
    );
    write(
        &clone.join("_models/backup/weights.bin"),
        pointer_text(&encoder).as_bytes(),
    );
    write(&clone.join("_models/models.toml"), MODELS_TOML);
    write(&clone.join("_models/empty"), b"");
    // Outside the directory asked for: never requested.
    write(
        &clone.join("tgorka/huge.bin"),
        pointer_text(b"somebody else's").as_bytes(),
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(clone.join("tgorka"), clone.join("_models/escape"))
        .expect("symlink");

    let server = Server::start(HashMap::from([
        (oid_of(&encoder), encoder.clone()),
        (oid_of(VOCAB), VOCAB.to_vec()),
    ]));

    let first = hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("hydrates");
    assert_eq!(
        first,
        HydrateReport {
            downloaded: 3,
            copied: 2,
            skipped: 0,
            downloaded_bytes: 2 * encoder.len() as u64 + VOCAB.len() as u64,
            copied_bytes: MODELS_TOML.len() as u64,
        }
    );
    assert_eq!(
        server.batches(),
        vec![2],
        "one batch, naming each distinct object once"
    );
    assert_eq!(server.downloads(), 2);
    let read = |rel: &str| std::fs::read(dest.join(rel)).expect(rel);
    assert_eq!(read("parakeet/Encoder.mlmodelc/weights.bin"), encoder);
    assert_eq!(read("backup/weights.bin"), encoder);
    assert_eq!(read("parakeet/vocab.json"), VOCAB);
    assert_eq!(read("models.toml"), MODELS_TOML);
    assert_eq!(read("empty"), b"");
    assert!(
        std::fs::symlink_metadata(dest.join("escape")).is_err(),
        "a symbolic link is neither followed nor recreated"
    );
    assert!(dest.join(hydrate_state_file(ALL.name)).is_file());

    let second = hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("again");
    assert_eq!(
        second,
        HydrateReport {
            skipped: 5,
            ..HydrateReport::default()
        }
    );
    assert_eq!(server.batches(), vec![2], "nothing asked the second time");
    assert_eq!(server.downloads(), 2);

    // A file that is no longer what the state says — a different length — is
    // fetched again, and only it.
    std::fs::write(dest.join("parakeet/vocab.json"), b"{}").expect("truncate");
    let third = hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("repair");
    assert_eq!((third.downloaded, third.skipped), (1, 4));
    assert_eq!(server.batches(), vec![2, 1]);
    assert_eq!(read("parakeet/vocab.json"), VOCAB);
}

/// The server hands back bytes that are not the object: refused, and nothing
/// is written at that path — while the object that did verify is placed.
#[tokio::test]
async fn content_that_does_not_match_its_pointer_is_refused_and_not_written() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    let encoder = encoder();
    write(
        &clone.join("_models/weights.bin"),
        pointer_text(&encoder).as_bytes(),
    );
    write(
        &clone.join("_models/vocab.json"),
        pointer_text(VOCAB).as_bytes(),
    );
    // Same length, other bytes: only the digest can tell.
    let mut forged = VOCAB.to_vec();
    forged[3] ^= 0x20;
    let server = Server::start(HashMap::from([
        (oid_of(&encoder), encoder.clone()),
        (oid_of(VOCAB), forged),
    ]));

    let refused = hydrate(&server, &clone, &bearer(), &dest).await;
    assert!(
        matches!(refused, Err(SyncError::Integrity { .. })),
        "{refused:?}"
    );
    assert!(
        std::fs::symlink_metadata(dest.join("vocab.json")).is_err(),
        "nothing is written for content that failed its check"
    );
    assert_eq!(
        std::fs::read(dest.join("weights.bin")).expect("placed"),
        encoder
    );
    let state = std::fs::read_to_string(dest.join(hydrate_state_file(ALL.name))).expect("state");
    assert!(
        state.contains("weights.bin") && !state.contains("vocab.json"),
        "{state}"
    );
}

/// A file already holding the right bytes is adopted by one hash, with no
/// state file to vouch for it and no request.
#[tokio::test]
async fn a_file_already_in_place_is_adopted_without_a_download() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    write(
        &clone.join("_models/vocab.json"),
        pointer_text(VOCAB).as_bytes(),
    );
    write(&dest.join("vocab.json"), VOCAB);
    let server = Server::start(HashMap::new());

    let report = hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("hydrates");
    assert_eq!(
        report,
        HydrateReport {
            skipped: 1,
            ..HydrateReport::default()
        }
    );
    assert!(server.batches().is_empty());
}

/// The config repository's credential is what reaches the LFS server; a wrong
/// one is an authentication failure, not an empty result.
#[tokio::test]
async fn a_rejected_credential_is_an_auth_failure_and_writes_nothing() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    write(
        &clone.join("_models/vocab.json"),
        pointer_text(VOCAB).as_bytes(),
    );
    let server = Server::start(HashMap::from([(oid_of(VOCAB), VOCAB.to_vec())]));

    let refused = hydrate(
        &server,
        &clone,
        &RepoAuth::Bearer("stale".to_owned()),
        &dest,
    )
    .await;
    assert!(
        matches!(refused, Err(SyncError::Auth { .. })),
        "{refused:?}"
    );
    assert!(std::fs::symlink_metadata(dest.join("vocab.json")).is_err());
}

/// Hydration reads only below the copy: a directory that climbs out, or one
/// reached through a symbolic link, is refused before anything is read.
#[tokio::test]
async fn a_directory_outside_the_copy_is_refused() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    write(&work.path().join("secret/key.pem"), b"private");
    std::fs::create_dir_all(&clone).expect("clone");
    let server = Server::start(HashMap::new());
    let run = |rel: &'static str| {
        let clone = clone.clone();
        let dest = dest.clone();
        let url = server.remote_url.clone();
        async move {
            hydrate_lfs_dir(
                &reqwest::Client::new(),
                &clone,
                rel,
                ALL,
                &url,
                &bearer(),
                &dest,
                &AtomicBool::new(false),
            )
            .await
        }
    };

    for rel in ["../secret", "/etc", ""] {
        let refused = run(rel).await;
        assert!(
            matches!(refused, Err(SyncError::InvalidPathForRemote { .. })),
            "{rel:?}: {refused:?}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(work.path().join("secret"), clone.join("_models"))
            .expect("symlink");
        let refused = run("_models").await;
        assert!(
            matches!(refused, Err(SyncError::InvalidPathForRemote { .. })),
            "{refused:?}"
        );
    }
    assert!(!dest.join("key.pem").exists());
}

/// A raised interrupt stops the run before it asks the server for anything.
#[tokio::test]
async fn an_interrupt_cancels_the_run() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    write(
        &clone.join("_models/vocab.json"),
        pointer_text(VOCAB).as_bytes(),
    );
    let server = Server::start(HashMap::from([(oid_of(VOCAB), VOCAB.to_vec())]));

    let cancelled = hydrate_lfs_dir(
        &reqwest::Client::new(),
        &clone,
        "_models",
        ALL,
        &server.remote_url,
        &bearer(),
        &dest,
        &AtomicBool::new(true),
    )
    .await;
    assert!(
        matches!(cancelled, Err(SyncError::Cancelled)),
        "{cancelled:?}"
    );
    assert!(server.batches().is_empty());
}

/// A `.lfsconfig` in the config repository names another host: it is not
/// asked, and so never sees the repository credential — the objects come from
/// the remote's own LFS endpoint.
#[tokio::test]
async fn a_repository_lfsconfig_never_redirects_the_credential() {
    let thief = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let thief_port = thief.local_addr().expect("addr").port();
    let visits = Arc::new(AtomicUsize::new(0));
    {
        let visits = Arc::clone(&visits);
        std::thread::spawn(move || {
            while let Ok((stream, _)) = thief.accept() {
                visits.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
    }
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    write(
        &clone.join(".lfsconfig"),
        format!("[lfs]\n\turl = http://127.0.0.1:{thief_port}/steal.git/info/lfs\n").as_bytes(),
    );
    write(
        &clone.join("_models/vocab.json"),
        pointer_text(VOCAB).as_bytes(),
    );
    let server = Server::start(HashMap::from([(oid_of(VOCAB), VOCAB.to_vec())]));

    let report = hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("hydrates from the remote's own endpoint");
    assert_eq!(report.downloaded, 1);
    assert_eq!(
        std::fs::read(dest.join("vocab.json")).expect("placed"),
        VOCAB
    );
    assert_eq!(
        visits.load(Ordering::SeqCst),
        0,
        "the other host was never contacted"
    );
}

/// A set is ready only when complete and current: an in-place model update
/// whose new object fails leaves the old file beside nothing missing, and the
/// set must still not count as ready until the whole new set is in place.
#[tokio::test]
async fn a_half_updated_set_is_never_current() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    let encoder = encoder();
    write(
        &clone.join("_models/Encoder/weights.bin"),
        pointer_text(&encoder).as_bytes(),
    );
    write(
        &clone.join("_models/Decoder/weights.bin"),
        pointer_text(VOCAB).as_bytes(),
    );
    write(&clone.join("_models/models.toml"), MODELS_TOML);
    let current = || hydration_is_current(&clone, "_models", ALL, &dest);
    assert!(!current(), "nothing hydrated yet");

    let server = Server::start(HashMap::from([
        (oid_of(&encoder), encoder.clone()),
        (oid_of(VOCAB), VOCAB.to_vec()),
    ]));
    hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("hydrates");
    assert!(current());
    let first = completion_digest(&dest, ALL.name).expect("marked complete");
    hydrate(&server, &clone, &bearer(), &dest)
        .await
        .expect("nothing to do");
    assert!(current(), "a run that changes nothing keeps the set ready");
    assert_eq!(
        completion_digest(&dest, ALL.name).as_deref(),
        Some(first.as_str())
    );

    // The config repository moves the decoder to new weights.
    let decoder = b"new decoder weights".to_vec();
    write(
        &clone.join("_models/Decoder/weights.bin"),
        pointer_text(&decoder).as_bytes(),
    );
    assert!(!current(), "the copy names a set dest does not hold");
    let failed = hydrate(&server, &clone, &bearer(), &dest).await;
    assert!(failed.is_err(), "the server does not have the new decoder");
    assert!(
        dest.join("Decoder/weights.bin").is_file(),
        "the old decoder is still on disk, so nothing looks missing"
    );
    assert!(!dest.join(hydrate_complete_file(ALL.name)).exists());
    assert!(!current(), "…and yet the set is not ready");

    let updated = Server::start(HashMap::from([(oid_of(&decoder), decoder.clone())]));
    hydrate(&updated, &clone, &bearer(), &dest)
        .await
        .expect("the rest of the set arrives");
    assert!(current());
    assert_ne!(completion_digest(&dest, ALL.name), Some(first));

    let cancelled = hydrate_lfs_dir(
        &reqwest::Client::new(),
        &clone,
        "_models",
        ALL,
        &updated.remote_url,
        &bearer(),
        &dest,
        &AtomicBool::new(true),
    )
    .await;
    assert!(matches!(cancelled, Err(SyncError::Cancelled)));
    assert!(!current(), "a cancelled run leaves the set unvouched for");
}

/// Two groups into one destination, the way keeper fetches its models: the
/// turn group asks the server for its two folders' objects and nothing else
/// (the transcription model is neither requested nor written), both groups
/// copy the plain top-level files, and each keeps its own marker and state —
/// so a new turn model makes only the turn group stale, and bringing it up to
/// date leaves the transcription group's marker as it was.
#[tokio::test]
async fn hydrate_lfs_dir_takes_only_the_named_roles() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    let asr = encoder();
    let vad = b"silero weights".to_vec();
    let turn = b"smart turn weights".to_vec();
    write(
        &clone.join("_models/parakeet/Encoder.mlmodelc/weights.bin"),
        pointer_text(&asr).as_bytes(),
    );
    write(
        &clone.join("_models/silero-vad/model.onnx"),
        pointer_text(&vad).as_bytes(),
    );
    write(
        &clone.join("_models/smart-turn-v3/model.onnx"),
        pointer_text(&turn).as_bytes(),
    );
    write(&clone.join("_models/silero-vad/LICENSE"), b"MIT");
    write(&clone.join("_models/models.toml"), MODELS_TOML);
    let server = Server::start(HashMap::from([
        (oid_of(&asr), asr.clone()),
        (oid_of(&vad), vad.clone()),
        (oid_of(&turn), turn.clone()),
    ]));
    let turn_dirs = ["silero-vad".to_owned(), "smart-turn-v3".to_owned()];
    let (transcription, turn_group) = role_groups(&turn_dirs);

    let report = hydrate_group(&server, &clone, turn_group, &bearer(), &dest)
        .await
        .expect("the turn models");
    assert_eq!((report.downloaded, report.copied), (2, 2));
    assert_eq!(server.batches(), vec![2], "only the two turn objects asked");
    assert_eq!(server.downloads(), 2);
    assert_eq!(
        std::fs::read(dest.join("silero-vad/model.onnx")).expect("vad"),
        vad
    );
    assert_eq!(
        std::fs::read(dest.join("smart-turn-v3/model.onnx")).expect("turn"),
        turn
    );
    assert!(dest.join("models.toml").is_file());
    assert!(!dest.join("parakeet").exists(), "nothing else is written");
    assert!(hydration_is_current(&clone, "_models", turn_group, &dest));
    assert!(!hydration_is_current(
        &clone,
        "_models",
        transcription,
        &dest
    ));

    let report = hydrate_group(&server, &clone, transcription, &bearer(), &dest)
        .await
        .expect("the transcription models");
    assert_eq!(
        (report.downloaded, report.copied, report.skipped),
        (1, 0, 1),
        "the transcription model fetched; models.toml already in place"
    );
    assert_eq!(server.batches(), vec![2, 1]);
    let transcription_state =
        std::fs::read_to_string(dest.join(hydrate_state_file("transcription"))).expect("state");
    assert!(
        transcription_state.contains("parakeet") && !transcription_state.contains("silero"),
        "{transcription_state}"
    );
    let transcription_digest =
        completion_digest(&dest, "transcription").expect("transcription marked");
    let turn_digest = completion_digest(&dest, "turn").expect("turn marked");
    assert_ne!(transcription_digest, turn_digest);

    // The organisation publishes a new Smart Turn.
    let turn_next = b"smart turn weights, retrained".to_vec();
    write(
        &clone.join("_models/smart-turn-v3/model.onnx"),
        pointer_text(&turn_next).as_bytes(),
    );
    assert!(!hydration_is_current(&clone, "_models", turn_group, &dest));
    assert!(
        hydration_is_current(&clone, "_models", transcription, &dest),
        "a turn model change never makes transcription look half-updated"
    );
    let updated = Server::start(HashMap::from([(oid_of(&turn_next), turn_next.clone())]));
    let report = hydrate_group(&updated, &clone, turn_group, &bearer(), &dest)
        .await
        .expect("the new turn model");
    assert_eq!(report.downloaded, 1);
    assert_eq!(updated.batches(), vec![1]);
    assert!(hydration_is_current(&clone, "_models", turn_group, &dest));
    assert_eq!(
        completion_digest(&dest, "transcription").as_deref(),
        Some(transcription_digest.as_str())
    );
    assert_ne!(completion_digest(&dest, "turn"), Some(turn_digest));
}

/// A group's name becomes two file names in the destination, so it is
/// letters only: anything else is refused before anything is read.
#[tokio::test]
async fn a_group_name_that_is_not_letters_is_refused() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    write(&clone.join("_models/models.toml"), MODELS_TOML);
    let server = Server::start(HashMap::new());
    for name in ["", "../turn", "Turn"] {
        let group = HydrateGroup {
            name,
            folders: Folders::AllBut(&[]),
            manifest: None,
        };
        let refused = hydrate_group(&server, &clone, group, &bearer(), &dest).await;
        assert!(matches!(refused, Err(SyncError::Config(_))), "{name:?}");
    }
    assert!(!dest.exists());
}

const ROLES_TOML: &[u8] = b"[asr]\ndir = \"parakeet\"\n\n[vad]\ndir = \"silero-vad\"\n\n\
    [smart_turn]\ndir = \"smart-turn-v3\"\n";

/// A clone with a transcription model and two turn models, all served.
fn roles_clone(clone: &Path) -> HashMap<String, Vec<u8>> {
    let asr = encoder();
    let vad = b"silero weights".to_vec();
    let turn = b"smart turn weights".to_vec();
    for (rel, bytes) in [
        ("parakeet/Encoder.mlmodelc/weights.bin", &asr),
        ("silero-vad/model.onnx", &vad),
        ("smart-turn-v3/model.onnx", &turn),
    ] {
        write(
            &clone.join("_models").join(rel),
            pointer_text(bytes).as_bytes(),
        );
    }
    write(&clone.join("_models/models.toml"), ROLES_TOML);
    HashMap::from([
        (oid_of(&asr), asr),
        (oid_of(&vad), vad),
        (oid_of(&turn), turn),
    ])
}

/// A turn model picked in settings whose object the server does not have:
/// the turn group fails, and transcription — which leaves the picked folder
/// out — hydrates, stays current and keeps its marker. Had transcription
/// taken the picked folder, its own hydration would have failed on it.
#[tokio::test]
async fn an_unavailable_picked_turn_model_leaves_transcription_usable() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    let server = Server::start(roles_clone(&clone));
    write(
        &clone.join("_models/silero-vad-v7/model.onnx"),
        pointer_text(b"silero v7, never uploaded").as_bytes(),
    );
    let picked = [
        "silero-vad".to_owned(),
        "silero-vad-v7".to_owned(),
        "smart-turn-v3".to_owned(),
    ];
    let (transcription, turn) = role_groups(&picked);

    hydrate_group(&server, &clone, transcription, &bearer(), &dest)
        .await
        .expect("transcription never asks for the picked folder");
    let failed = hydrate_group(&server, &clone, turn, &bearer(), &dest).await;
    assert!(failed.is_err(), "the picked object is not on the server");
    assert!(!hydration_is_current(&clone, "_models", turn, &dest));
    assert!(hydration_is_current(
        &clone,
        "_models",
        transcription,
        &dest
    ));
    assert!(completion_digest(&dest, "transcription").is_some());
    assert!(dest.join("parakeet/Encoder.mlmodelc/weights.bin").is_file());

    // The overlap the groups must not have: a transcription group taking the
    // picked folder fails on its object.
    let repo_only = ["silero-vad".to_owned(), "smart-turn-v3".to_owned()];
    let (overlapping, _) = role_groups(&repo_only);
    let overlapping = HydrateGroup {
        name: "overlap",
        ..overlapping
    };
    assert!(
        hydrate_group(&server, &clone, overlapping, &bearer(), &dest)
            .await
            .is_err()
    );
}

/// The documented upgrade: `[smart_turn] dir` moves to a new folder, the old
/// one kept. Transcription — its sections of `models.toml` and its folders
/// unchanged — is current before the turn group is fetched and while that
/// fetch fails, and copying the new `models.toml` changes nothing for it.
#[tokio::test]
async fn a_turn_model_version_switch_keeps_transcription_current() {
    let work = tempfile::tempdir().expect("tempdir");
    let clone = work.path().join("repo");
    let dest = work.path().join("models");
    let server = Server::start(roles_clone(&clone));
    let before = ["silero-vad".to_owned(), "smart-turn-v3".to_owned()];
    let (transcription, turn) = role_groups(&before);
    hydrate_group(&server, &clone, transcription, &bearer(), &dest)
        .await
        .expect("transcription");
    hydrate_group(&server, &clone, turn, &bearer(), &dest)
        .await
        .expect("turn");
    let marked = completion_digest(&dest, "transcription").expect("marked");

    write(
        &clone.join("_models/smart-turn-v4/model.onnx"),
        pointer_text(b"smart turn v4, not served yet").as_bytes(),
    );
    let switched = String::from_utf8(ROLES_TOML.to_vec())
        .expect("utf-8")
        .replace("smart-turn-v3", "smart-turn-v4");
    write(&clone.join("_models/models.toml"), switched.as_bytes());
    // keeper-core leaves every folder holding a turn model out of
    // transcription, the old version's too.
    let turn_folders = [
        "silero-vad".to_owned(),
        "smart-turn-v3".to_owned(),
        "smart-turn-v4".to_owned(),
    ];
    let (transcription, _) = role_groups(&turn_folders);
    let after = ["silero-vad".to_owned(), "smart-turn-v4".to_owned()];
    let (_, turn) = role_groups(&after);
    let current = || hydration_is_current(&clone, "_models", transcription, &dest);

    assert!(current(), "before the turn group is fetched");
    assert!(!hydration_is_current(&clone, "_models", turn, &dest));
    let report = hydrate_group(&server, &clone, transcription, &bearer(), &dest)
        .await
        .expect("transcription copies the new models.toml");
    assert_eq!(report.copied, 1);
    assert_eq!(
        completion_digest(&dest, "transcription").as_deref(),
        Some(marked.as_str())
    );
    let failed = hydrate_group(&server, &clone, turn, &bearer(), &dest).await;
    assert!(failed.is_err(), "the new turn model is not served yet");
    assert!(current(), "while the turn fetch fails");
    assert_eq!(
        completion_digest(&dest, "transcription").as_deref(),
        Some(marked.as_str())
    );
}

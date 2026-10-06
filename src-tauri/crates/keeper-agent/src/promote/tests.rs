use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keeper_core::agents::agentd::DrivePin;
use keeper_core::agents::label::{
    Integrity, Label, LabelBody, LabelCause, LabelCauseKind, Readers,
};
use keeper_core::agents::log::{HostSlug, LineBody, LogLine, LINE_VERSION};
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use keeper_core::sessions::promote::PromoteState;
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use super::*;

const SESSION: &str = "active/2026-10-06-harvest";
const README_TEXT: &str = "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\n---\n# Harvest\n\n## Promote\n\n<!-- promotion notes -->\n\n| workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n\n## After\n\nkept\n";
const SETTLE_MS: u64 = 5_000;
const TG: &str = "@tgorka:h";
const MARTA: &str = "@marta:h";
const ME: &str = "human:tgorka";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(0))
}

/// A drive holding a sessions zone with one session made from the template.
fn drive() -> (tempfile::TempDir, PathBuf) {
    let drive = tempfile::tempdir().expect("drive");
    let zone = drive.path().join("60-sessions");
    let dir = zone.join(SESSION);
    std::fs::create_dir_all(dir.join("workspace")).expect("workspace");
    std::fs::create_dir_all(dir.join("artifacts")).expect("artifacts");
    std::fs::write(dir.join("README.md"), README_TEXT).expect("readme");
    (drive, zone)
}

fn put(zone: &Path, rel: &str, bytes: &[u8], age: Duration) {
    let path = zone.join(SESSION).join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(&path, bytes).expect("write");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(SystemTime::now() - age))
        .expect("mtime");
}

fn read(zone: &Path, rel: &str) -> Option<Vec<u8>> {
    std::fs::read(zone.join(SESSION).join(rel)).ok()
}

fn readme(zone: &Path) -> String {
    std::fs::read_to_string(zone.join(SESSION).join("README.md")).expect("readme")
}

const AN_HOUR: Duration = Duration::from_secs(3600);

/// A notes vault, `10-notes/` unless moved, written as the shell's writer
/// writes it: a write refused unless the vault is still where it was
/// checked, one that can be made to fail before it lands or after it
/// landed and before it is durable, and an amend guarded on the text it
/// read, with an editor's save that can be made to land between its read
/// and its write.
#[derive(Default)]
struct Vault {
    root: PathBuf,
    subfolder: Mutex<String>,
    failing: AtomicBool,
    unflushed: AtomicBool,
    meanwhile: Mutex<Option<String>>,
}

impl Vault {
    fn at(root: &Path) -> Vault {
        Vault {
            root: root.to_owned(),
            subfolder: Mutex::new("10-notes".to_owned()),
            ..Vault::default()
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root
            .join(&*self.subfolder.lock().expect("subfolder"))
            .join(rel)
    }
}

impl VaultWriter for Vault {
    fn subfolder(&self, _: &str) -> Option<String> {
        Some(self.subfolder.lock().expect("subfolder").clone())
    }

    fn write(&self, _: &str, subfolder: &str, rel: &str, text: &str) -> Result<(), String> {
        if *self.subfolder.lock().expect("subfolder") != subfolder {
            return Err("the vault moved".to_owned());
        }
        if self.failing.load(Ordering::SeqCst) {
            return Err("the disk is full".to_owned());
        }
        // Through a temp file renamed over the target, as the shell's
        // writer does: what is at the target is replaced, never opened.
        let path = self.path(rel);
        let parent = path.parent().ok_or("no parent")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temp = parent.join(".keeper.test.tmp");
        std::fs::write(&temp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
        if self.unflushed.load(Ordering::SeqCst) {
            return Err("the copy could not be synced to the disk".to_owned());
        }
        Ok(())
    }

    fn amend(
        &self,
        _: &str,
        rel: &str,
        amend: &dyn Fn(&str) -> Option<String>,
    ) -> Result<bool, String> {
        let path = self.path(rel);
        for _ in 0..3 {
            let disk = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let Some(next) = amend(&disk) else {
                return Ok(false);
            };
            if let Some(saved) = self.meanwhile.lock().expect("lock").take() {
                std::fs::write(&path, saved).map_err(|e| e.to_string())?;
            }
            if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != disk {
                continue;
            }
            std::fs::write(&path, next).map_err(|e| e.to_string())?;
            return Ok(true);
        }
        Err(format!("{rel} kept changing"))
    }
}

fn user(id: &str) -> OwnedUserId {
    OwnedUserId::try_from(id).expect("user")
}

fn readers(ids: &[&str]) -> Readers {
    Readers::Only(ids.iter().map(|id| user(id)).collect::<BTreeSet<_>>())
}

fn label(ids: &[&str]) -> Label {
    Label {
        readers: readers(ids),
        integrity: Integrity::Agent,
        local_only: false,
    }
}

fn profile(root: &Path) -> SyncProfile {
    let mut profile = SyncProfile::new(
        "P1".to_owned(),
        "tgdrive".to_owned(),
        root.to_owned(),
        "https://forge.example/tgdrive.git".to_owned(),
    );
    profile.sessions = Some(Default::default());
    profile.agents = Some(Default::default());
    profile
}

/// The drive's `_drive.toml`, owned by tgorka and read by `ids`.
fn declare(profile: &SyncProfile, ids: &[&str]) {
    let root = profile.agents_root().expect("agents zone");
    std::fs::create_dir_all(&root).expect("agents zone");
    let readers: Vec<String> = ids.iter().map(|id| format!("\"{id}\"")).collect();
    std::fs::write(
        root.join(keeper_core::agents::drive::FILE_NAME),
        format!(
            "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{TG}\"\nreaders = [{}]\nlocal_only = false\n",
            readers.join(", ")
        ),
    )
    .expect("_drive.toml");
}

/// This device's pin of the drive: tgorka's, read by `ids`.
fn pin(ids: &[&str]) -> DrivePin {
    DrivePin {
        id: "tgdrive".to_owned(),
        remote: "https://forge.example/tgdrive.git".to_owned(),
        credential: None,
        owner: user(TG),
        readers: ids.iter().map(|id| user(id)).collect(),
        local_only: false,
    }
}

/// Make the session an agent's, opened with `opening`.
fn agent_session(zone: &Path, opening: &[&str]) {
    let toml = compose_session_agent_toml(&SessionAgent {
        id: ulid::Ulid::new(),
        agent: "tola-grey".to_owned(),
        drive: "tgdrive".to_owned(),
        kind: SessionKind::Conversation,
        title: "harvest".to_owned(),
        requested_by: user(TG),
        parent: None,
        room: OwnedRoomId::try_from("!harvest:h").expect("room"),
        drives: vec!["tgdrive".to_owned()],
        label: label(opening),
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: Vec::new(),
        limits: None,
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
        created_at: chrono::Utc::now(),
    });
    std::fs::write(zone.join(SESSION).join("agent.toml"), toml).expect("agent.toml");
}

const CHUNK: &str = "log/2026-10-06.electra.1.jsonl";

/// One `label` line, the `n`th of the log, narrowing to `ids`.
fn label_line(ids: &[&str], n: i64) -> String {
    let at = chrono::DateTime::parse_from_rfc3339("2026-10-06T09:00:00Z")
        .expect("ts")
        .with_timezone(&chrono::Utc)
        + chrono::Duration::milliseconds(n);
    let line = LogLine {
        v: LINE_VERSION,
        id: ulid::Ulid::new(),
        parent: None,
        ts: at,
        host: HostSlug::new("electra").expect("slug"),
        epoch: 0,
        claim: None,
        matrix_event: None,
        body: LineBody::Label(LabelBody::new(
            &label(ids),
            LabelCause {
                kind: LabelCauseKind::DriveRead,
                reference: "tgdrive/10-notes/x.md".to_owned(),
            },
        )),
    };
    format!("{}\n", line.to_json().expect("json"))
}

fn append_log(zone: &Path, text: &str) {
    use std::io::Write as _;
    let path = zone.join(SESSION).join(CHUNK);
    std::fs::create_dir_all(path.parent().expect("log")).expect("log");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .expect("append");
}

fn out<'a>(profile: &'a SyncProfile, vault: &'a Vault, pin: Option<&'a DrivePin>) -> OutOf<'a> {
    OutOf {
        profile,
        vault,
        pin: Ok(pin),
    }
}

/// 95.5 acceptance 5 (FR-243, real files and mtimes): a settled
/// `workspace/draft.md` promoted to `artifacts/report.md` is copied byte for
/// byte and recorded as one row, every other byte of the README kept;
/// promoted again after it changed, the target is replaced and the row
/// stays one; a source written within the stability window is refused with
/// the sentence and nothing changes. The panel then reads the row `ok`, and
/// the draft is no longer unlisted.
#[test]
fn promote_copies_and_records_one_row() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    let draft: &[u8] = b"# Report\n\x00\xffbinary tail\n";
    put(&zone, "workspace/draft.md", draft, AN_HOUR);
    put(&zone, "workspace/other.csv", b"a,b\n", AN_HOUR);
    let promote = |zone: &Path| {
        promote_in(
            zone,
            SESSION,
            "workspace/draft.md",
            "artifacts/report.md",
            "weekly",
            SETTLE_MS,
            now_ms(),
        )
    };

    promote(&zone).expect("promoted");
    assert_eq!(read(&zone, "artifacts/report.md").as_deref(), Some(draft));
    let row = "| workspace/draft.md | artifacts/report.md | weekly |\n";
    assert_eq!(
        readme(&zone),
        README_TEXT.replace("| ---- |\n", &format!("| ---- |\n{row}")),
        "one row, every other byte kept"
    );

    put(&zone, "workspace/draft.md", b"# Report v2\n", AN_HOUR);
    promote(&zone).expect("promoted again");
    assert_eq!(
        read(&zone, "artifacts/report.md").as_deref(),
        Some(&b"# Report v2\n"[..])
    );
    assert_eq!(readme(&zone).matches("workspace/draft.md").count(), 1);

    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.rows.len(), 1);
    assert_eq!(vm.rows[0].state, PromoteState::Ok);
    assert_eq!(vm.unlisted, ["workspace/other.csv"]);

    // Later than the copy by more than the mtime resolution.
    std::thread::sleep(Duration::from_millis(20));
    put(
        &zone,
        "workspace/draft.md",
        b"# Report v3, mid-write\n",
        Duration::ZERO,
    );
    let refused = promote(&zone);
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.ends_with(&format!("{STILL_WRITING}."))),
        "{refused:?}"
    );
    assert_eq!(
        read(&zone, "artifacts/report.md").as_deref(),
        Some(&b"# Report v2\n"[..])
    );
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.rows[0].state, PromoteState::Stale);
}

/// R95K-05: the copy a promotion journals is of the bytes that were
/// checked, by their digest: a source that changed since is refused with
/// the target as it was and no stage left beside it; the checked bytes are
/// copied; and a resume finding its own copy there is done.
#[test]
fn a_checked_copy_copies_only_what_was_read() {
    let (_drive, zone) = drive();
    put(&zone, "workspace/a.bin", b"checked\n", AN_HOUR);
    put(&zone, "artifacts/a.bin", b"before\n", AN_HOUR);
    let copy = |sha256: String| {
        exec::run(
            &zone,
            Plan {
                verb: "promote".to_owned(),
                session: SESSION.to_owned(),
                steps: vec![PlanStep::CopyChecked {
                    from: format!("{SESSION}/workspace/a.bin"),
                    to: format!("{SESSION}/artifacts/a.bin"),
                    sha256,
                }],
            },
        )
    };
    let refused = copy(sha256_hex("what was read before a writer started\n"));
    assert!(matches!(refused, Err(ExecError::Refused(_))), "{refused:?}");
    assert_eq!(
        read(&zone, "artifacts/a.bin").as_deref(),
        Some(&b"before\n"[..])
    );
    let leftovers: Vec<_> = std::fs::read_dir(zone.join(SESSION).join("artifacts"))
        .expect("artifacts")
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(leftovers, ["a.bin"]);

    copy(sha256_hex("checked\n")).expect("copied");
    assert_eq!(
        read(&zone, "artifacts/a.bin").as_deref(),
        Some(&b"checked\n"[..])
    );
    std::fs::remove_file(zone.join(SESSION).join("workspace/a.bin")).expect("gone");
    copy(sha256_hex("checked\n")).expect("a resume finds its own copy");
}

/// R95K-07, into the session: a promotion whose copy fails after its row
/// is recorded leaves that row, its target not there, and promoting again
/// finishes it with one row. Its target runs through a file, which the
/// one resolver never takes for an absence (only `NotFound` is): the row
/// is `unknown` with why, not a missing target.
#[test]
fn a_failed_copy_leaves_a_row_promoting_again_finishes() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/draft.md", b"draft\n", AN_HOUR);
    // A file where the target's folder belongs: the copy cannot land.
    put(&zone, "artifacts/sub", b"in the way\n", AN_HOUR);
    let promote = || {
        promote_in(
            &zone,
            SESSION,
            "workspace/draft.md",
            "artifacts/sub/report.md",
            "",
            SETTLE_MS,
            now_ms(),
        )
    };
    assert!(promote().is_err());
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.rows.len(), 1);
    assert_eq!(vm.rows[0].state, PromoteState::Unknown);
    assert!(vm.rows[0].problem.is_some());

    std::fs::remove_file(zone.join(SESSION).join("artifacts/sub")).expect("cleared");
    promote().expect("finished");
    assert_eq!(
        read(&zone, "artifacts/sub/report.md").as_deref(),
        Some(&b"draft\n"[..])
    );
    assert_eq!(readme(&zone).matches("workspace/draft.md").count(), 1);
}

const NOTE_REL: &str = "artifacts/knowledge/2026-10-05-taxes/short.md";
const NOTE: &str = "---\ntype: Note\ntitle: Taxes, in short\ngenerated:\n  by: agent:tola-grey@electra\n  at: 2026-10-06T09:00:00Z\nhuman_reviewed: false\nstatus: draft\n---\n\nThree papers.\n";
const TARGET: &str = "10-notes/knowledge/short.md";
const AT: &str = "2026-10-06T10:00:00Z";

fn tgorka() -> Reviewer<'static> {
    Reviewer {
        person: "tgorka",
        at: AT,
    }
}

fn request<'a>(target: &'a str, expected: Option<&'a str>) -> Request<'a> {
    Request {
        source: NOTE_REL,
        target,
        note: "knowledge",
        expected,
    }
}

/// 95.5 acceptance 6 and 7 (FR-808, R138, R139, R212): a harvested note
/// promoted into `10-notes/knowledge/` of the same drive — the version the
/// person read — is copied with its frontmatter and the person's canonical
/// review, recorded as `| artifacts/knowledge/<…>.md |
/// 10-notes/knowledge/<note>.md | knowledge |`; the candidate stays as the
/// agent's host wrote it. A target outside the vault and one in another
/// drive are refused, nothing written. The panel names the reviewer; the
/// untick takes the review out of the copy and the tick puts it back.
#[test]
fn promote_out_into_the_vault() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read_as = sha256_hex(NOTE);

    for elsewhere in ["30-work/short.md", "../neuradrive/10-notes/short.md"] {
        let refused = promote_out(
            &zone,
            SESSION,
            &request(elsewhere, Some(&read_as)),
            Some(&tgorka()),
            &out,
        );
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{elsewhere}: {refused:?}"
        );
    }
    assert_eq!(readme(&zone), README_TEXT);
    assert!(!drive.path().join("30-work").exists());

    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(vm.knowledge[0].revision.as_deref(), Some(read_as.as_str()));
    assert_eq!(vm.out_refused, None);
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&read_as)),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    let reviewed = knowledge::review(NOTE, "tgorka", AT, true);
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(reviewed.clone())
    );
    assert!(readme(&zone).contains(&published_row(&reviewed)));
    assert_eq!(
        read(&zone, NOTE_REL).as_deref(),
        Some(NOTE.as_bytes()),
        "the candidate stays"
    );
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(vm.knowledge[0].reviewed_by.as_deref(), Some(ME));
    assert!(vm.knowledge[0].reviewed_by_me);
    assert_eq!(vm.knowledge[0].state, Some(PromoteState::Ok));

    review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out).expect("unticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET))
            .ok()
            .as_deref(),
        Some(NOTE)
    );
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert!(!vm.knowledge[0].reviewed_by_me);
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, true, &out).expect("ticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(reviewed)
    );

    // A second harvested note, not promoted: nothing to review yet.
    put(
        &zone,
        "artifacts/knowledge/2026-10-05-taxes/other.md",
        NOTE.as_bytes(),
        AN_HOUR,
    );
    let early = review(
        &zone,
        SESSION,
        "artifacts/knowledge/2026-10-05-taxes/other.md",
        "tgorka",
        "x",
        true,
        &out,
    );
    assert!(
        matches!(&early, Err(VerbError::Refused(sentence)) if sentence == NOT_PROMOTED),
        "{early:?}"
    );
}

/// R95K-06: promoting a harvested note is the person's review of the
/// version they read, owned by Rust: one that names no version, one whose
/// candidate changed since the version named, and one with no person to
/// record are refused — no vault copy, no row.
#[test]
fn a_harvested_note_is_promoted_only_as_it_was_read() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read_as = sha256_hex(NOTE);
    let stale = sha256_hex("an earlier version\n");
    // The drift case has a version and a person, so drift is the only
    // refusal left to it.
    for (expected, reviewer, why) in [
        (None, Some(tgorka()), Some(UNREVIEWED)),
        (Some(stale.as_str()), Some(tgorka()), None),
        (Some(read_as.as_str()), None, Some(NO_REVIEWER)),
    ] {
        let refused = promote_out(
            &zone,
            SESSION,
            &request(TARGET, expected),
            reviewer.as_ref(),
            &out,
        );
        assert!(
            matches!(&refused, Err(VerbError::Refused(sentence)) if why.is_none_or(|why| sentence == why)),
            "{refused:?}"
        );
    }
    assert!(!drive.path().join(TARGET).exists());
    assert_eq!(readme(&zone), README_TEXT);
}

/// R95K-07, out of the session: a publication that fails after the row is
/// recorded leaves the row and a missing target, said; promoting again
/// finishes it — the row names that target — with one row.
#[test]
fn a_failed_publication_leaves_a_row_promoting_again_finishes() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read_as = sha256_hex(NOTE);
    vault.failing.store(true, Ordering::SeqCst);
    let failed = promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&read_as)),
        Some(&tgorka()),
        &out,
    );
    assert!(matches!(failed, Err(VerbError::Refused(_))), "{failed:?}");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.rows.len(), 1);
    assert_eq!(vm.rows[0].state, PromoteState::MissingTarget);

    vault.failing.store(false, Ordering::SeqCst);
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&read_as)),
        Some(&tgorka()),
        &out,
    )
    .expect("finished");
    assert!(drive.path().join(TARGET).is_file());
    assert_eq!(readme(&zone).matches(NOTE_REL).count(), 1);
}

/// R95K-14: a note the table cannot hold as one row is refused before
/// anything is copied, the README and the vault as they were.
#[test]
fn a_row_the_table_cannot_hold_is_refused_before_any_copy() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    put(&zone, "workspace/draft.md", b"draft\n", AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read_as = sha256_hex(NOTE);
    for note in ["weekly | final", "weekly\n## Heading"] {
        let refused = promote_out(
            &zone,
            SESSION,
            &Request {
                note,
                ..request(TARGET, Some(&read_as))
            },
            Some(&tgorka()),
            &out,
        );
        assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
        let refused = promote_in(
            &zone,
            SESSION,
            "workspace/draft.md",
            "artifacts/draft.md",
            note,
            SETTLE_MS,
            now_ms(),
        );
        assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    }
    assert!(!drive.path().join(TARGET).exists());
    assert_eq!(read(&zone, "artifacts/draft.md"), None);
    assert_eq!(readme(&zone), README_TEXT);
}

/// R95K-08: a tick composes from the vault copy as it is at the write, so
/// another person's review that landed after the copy was read is kept
/// beside it, never written over. R244: an editor's save that changed what
/// the copy says, landing in the same window, is kept too, and the review
/// refused — that file is no longer the copy the note published (DW-960).
#[test]
fn a_tick_keeps_an_edit_that_landed_meanwhile() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out).expect("unticked");
    let by_marta = knowledge::review(NOTE, "marta", AT, true);
    *vault.meanwhile.lock().expect("lock") = Some(by_marta.clone());
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, true, &out).expect("ticked");
    let copy = std::fs::read_to_string(drive.path().join(TARGET)).expect("copy");
    assert_eq!(copy, knowledge::review(&by_marta, "tgorka", AT, true));

    let edited = copy.replace("Three papers.", "Three papers, and the ID.");
    *vault.meanwhile.lock().expect("lock") = Some(edited.clone());
    let refused = review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out);
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(edited)
    );
}

/// 95.5 acceptance 6, a wider audience (AD-391, NFR-115): an agent's
/// session whose label, narrowed by what it read, is `{tgorka}` cannot
/// promote into the vault of a drive read by `{tgorka, marta}` — refused
/// naming marta, nothing written into the vault, no row recorded — and the
/// panel shows the narrowed chip and the same refusal. Without that line
/// it may.
#[test]
fn promoting_out_respects_the_label() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG, MARTA]);
    let pinned = pin(&[TG, MARTA]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    agent_session(&zone, &[TG, MARTA]);
    append_log(&zone, &label_line(&[TG], 0));
    let read_as = sha256_hex(NOTE);
    let promote = || {
        promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&read_as)),
            Some(&tgorka()),
            &out,
        )
    };
    let refused = promote();
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains(MARTA)),
        "{refused:?}"
    );
    assert!(!drive.path().join(TARGET).exists());
    assert_eq!(readme(&zone), README_TEXT);
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert!(vm
        .out_refused
        .as_deref()
        .is_some_and(|why| why.contains(MARTA)));
    assert_eq!(vm.label.map(|chip| chip.readers), Some(vec![TG.to_owned()]));

    std::fs::remove_file(zone.join(SESSION).join(CHUNK)).expect("no narrowing");
    promote().expect("a label the drive's readers fit is promoted");
}

type Arrange = dyn Fn(&Path, &SyncProfile) -> Option<DrivePin>;

/// R95K-01, R95K-02: a promotion out is refused — nothing written, the
/// panel saying so — whenever who reads the session or the drive cannot be
/// established: an `agent.toml` that does not read, a `_drive.toml` that
/// does not parse, a drive not pinned on this device, a declaration edited
/// away from the pin (narrowed to the session's own `{tgorka}`), and an
/// agent's log with a line it cannot read.
#[test]
fn an_audience_that_cannot_be_established_refuses() {
    let read_as = sha256_hex(NOTE);
    let cases: [(&str, &Arrange); 5] = [
        ("agent.toml unreadable", &|zone, profile| {
            declare(profile, &[TG, MARTA]);
            std::fs::create_dir(zone.join(SESSION).join("agent.toml")).expect("dir");
            Some(pin(&[TG, MARTA]))
        }),
        ("_drive.toml malformed", &|_, profile| {
            declare(profile, &[TG, MARTA]);
            let root = profile.agents_root().expect("zone");
            std::fs::write(root.join("_drive.toml"), "readers = [").expect("broken");
            Some(pin(&[TG, MARTA]))
        }),
        ("not pinned", &|_, profile| {
            declare(profile, &[TG]);
            None
        }),
        ("declaration differs from the pin", &|zone, profile| {
            declare(profile, &[TG]);
            agent_session(zone, &[TG]);
            Some(pin(&[TG, MARTA]))
        }),
        ("a log line that does not read", &|zone, profile| {
            declare(profile, &[TG]);
            agent_session(zone, &[TG]);
            append_log(zone, "{\"v\": 1, \"half\n");
            Some(pin(&[TG]))
        }),
    ];
    for (what, arrange) in cases {
        let (drive, zone) = drive();
        put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
        let profile = profile(drive.path());
        let pinned = arrange(&zone, &profile);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, pinned.as_ref());
        let refused = promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&read_as)),
            Some(&tgorka()),
            &out,
        );
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{what}: {refused:?}"
        );
        assert!(!drive.path().join(TARGET).exists(), "{what}");
        assert_eq!(readme(&zone), README_TEXT, "{what}");
        let vm = panel(&zone, SESSION, &out, None).expect("panel");
        assert!(vm.out_refused.is_some(), "{what}");
        assert_eq!(vm.label, None, "{what}");
    }
}

/// R95K-03: the label a promotion is checked against is read from the
/// whole log to its frontier, not from the agents index — which, a refresh
/// behind, would still hold the wider label while a narrowing line sits
/// past what one refresh reads.
#[test]
fn a_label_past_the_index_s_reach_still_binds() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG, MARTA]);
    let pinned = pin(&[TG, MARTA]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    agent_session(&zone, &[TG, MARTA]);
    append_log(&zone, &label_line(&[TG, MARTA], 0));
    let mut index = keeper_core::agents::index::Index::open(&zone).expect("index");
    index.refresh_session(SESSION, None).expect("indexed");
    let wide = label_line(&[TG, MARTA], 1);
    let filler = (keeper_core::agents::index::REFRESH_BYTES as usize / wide.len()) + 16;
    let grown: String = (1..=filler as i64)
        .map(|n| label_line(&[TG, MARTA], n))
        .chain([label_line(&[TG], filler as i64 + 1)])
        .collect();
    append_log(&zone, &grown);
    let refused = promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out,
    );
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains(MARTA)),
        "{refused:?}"
    );
    assert!(!drive.path().join(TARGET).exists());
}

/// R95K-04: a vault folder that is a link — out of the drive, or into an
/// agent's home — never carries a promotion or a tick: refused, nothing
/// written there.
#[cfg(unix)]
#[test]
fn a_link_in_the_vault_never_carries_a_promotion() {
    let outside = tempfile::tempdir().expect("outside");
    for into_home in [false, true] {
        let (drive, zone) = drive();
        put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
        let profile = profile(drive.path());
        declare(&profile, &[TG]);
        let pinned = pin(&[TG]);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        let landing_dir = if into_home {
            let home = profile.agents_root().expect("agents").join("tola-grey");
            std::fs::create_dir_all(&home).expect("home");
            home
        } else {
            outside.path().to_owned()
        };
        std::fs::create_dir_all(drive.path().join("10-notes")).expect("vault");
        std::os::unix::fs::symlink(&landing_dir, drive.path().join("10-notes/knowledge"))
            .expect("link");
        let refused = promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out,
        );
        assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
        assert!(!landing_dir.join("short.md").exists());
        assert_eq!(readme(&zone), README_TEXT);
    }
}

/// A promotion out never writes where the drive's maintenance commits do —
/// the curator's `_skills/`, a home's memory files and proposals — nor a
/// session's scratch, even with the vault at the drive's root: those paths
/// change only through their own writers (`commit_paths`, guarded; the
/// session's agent), never by a person's promotion beside them. Nor does
/// finishing an interrupted one whose record names such a target. However
/// the profile spells its zones — `./80-agents`, ` 80-agents ` — they are
/// the folders its readers open, and the targets here are named as the disk
/// names them.
#[test]
fn a_promotion_never_lands_where_maintenance_commits() {
    for (agents, sessions) in [
        ("80-agents", "60-sessions"),
        ("./80-agents", "./60-sessions"),
        (" 80-agents ", " 60-sessions "),
        ("./80-agents/", " ./60-sessions"),
    ] {
        let (drive, zone) = drive();
        put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
        let mut profile = profile(drive.path());
        profile.agents.as_mut().expect("agents").subfolder = agents.to_owned();
        profile.sessions.as_mut().expect("sessions").subfolder = sessions.to_owned();
        declare(&profile, &[TG]);
        let pinned = pin(&[TG]);
        let vault = Vault::at(drive.path());
        *vault.subfolder.lock().expect("subfolder") = String::new();
        let out = out(&profile, &vault, Some(&pinned));
        let targets = [
            "80-agents/_skills/harvested/SKILL.md".to_owned(),
            "80-agents/tola-grey/MEMORY.md".to_owned(),
            "80-agents/tola-grey/proposals/short.md".to_owned(),
            format!("60-sessions/{SESSION}/workspace/short.md"),
        ];
        for target in &targets {
            let refused = promote_out(
                &zone,
                SESSION,
                &request(target, Some(&sha256_hex(NOTE))),
                Some(&tgorka()),
                &out,
            );
            assert!(
                matches!(refused, Err(VerbError::Refused(_))),
                "{agents:?}/{sessions:?} {target}: {refused:?}"
            );
            assert!(!drive.path().join(target).exists(), "{agents:?} {target}");
        }
        assert_eq!(readme(&zone), README_TEXT);

        put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
        let other = Request {
            source: "artifacts/other.md",
            target: "10-notes/other.md",
            note: "",
            expected: None,
        };
        for target in &targets {
            let mut kept = pending(NOTE);
            kept.target.clone_from(target);
            kept.vault_relative.clone_from(target);
            keep_pending(&zone, &kept).expect("kept");
            let refused = promote_out(&zone, SESSION, &other, None, &out);
            assert!(
                matches!(refused, Err(VerbError::Refused(_))),
                "{agents:?}/{sessions:?} {target}: {refused:?}"
            );
            assert!(!drive.path().join(target).exists(), "{agents:?} {target}");
            assert!(!zone.join(PENDING_REL).exists(), "{agents:?} {target}");
            assert!(!drive.path().join(other.target).exists());
        }
        assert_eq!(readme(&zone), README_TEXT);
    }
}

/// A vault configured `notes\.` on Unix is the folder of that name — the
/// one its writer writes into — and `notes/` beside it is not the vault: a
/// promotion to `notes/keep.md` is refused, and the person's
/// `notes\./keep.md`, which it never checked, keeps its bytes.
#[cfg(unix)]
#[test]
fn a_promotion_lands_only_where_it_was_checked() {
    promotes_beside_a_backslash_vault(false);
}

/// The same, for finishing a promotion interrupted on its way to
/// `notes/keep.md`: refused, its record cleared, nothing written.
#[cfg(unix)]
#[test]
fn a_recovered_promotion_lands_only_where_it_was_checked() {
    promotes_beside_a_backslash_vault(true);
}

/// Promote `NOTE` to `notes/keep.md` — afresh, or by finishing an
/// interrupted promotion's record — with the vault at `notes\.` and both
/// folders on the disk.
#[cfg(unix)]
fn promotes_beside_a_backslash_vault(recovery: bool) {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    *vault.subfolder.lock().expect("subfolder") = "notes\\.".to_owned();
    let out = out(&profile, &vault, Some(&pinned));
    let target = "notes/keep.md";
    std::fs::create_dir_all(drive.path().join("notes")).expect("notes");
    let theirs = drive.path().join("notes\\.").join("keep.md");
    std::fs::create_dir_all(theirs.parent().expect("parent")).expect("notes\\.");
    std::fs::write(&theirs, "theirs\n").expect("theirs");

    let refused = if recovery {
        let mut kept = pending(NOTE);
        kept.target = target.to_owned();
        kept.vault_relative = "keep.md".to_owned();
        keep_pending(&zone, &kept).expect("kept");
        put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
        promote_out(&zone, SESSION, &OTHER, None, &out)
    } else {
        promote_out(
            &zone,
            SESSION,
            &request(target, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out,
        )
    };
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    assert_eq!(std::fs::read(&theirs).expect("theirs"), b"theirs\n");
    assert!(!drive.path().join(target).exists());
    assert_eq!(readme(&zone), README_TEXT);
    assert!(!zone.join(PENDING_REL).exists());
}

/// R95K-12: what the panel cannot read it says — a row whose target is a
/// link out of the drive is `unknown` with why, a harvested note that is
/// not UTF-8 or larger than a note holds is listed with its problem and no
/// revision, a folder that will not list and a listing past its cap are
/// told — rather than reading as missing or leaving things out.
#[cfg(unix)]
#[test]
fn what_the_panel_cannot_read_it_says() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("x.md"), "theirs\n").expect("outside");
    put(&zone, "workspace/a.md", b"a\n", AN_HOUR);
    std::os::unix::fs::symlink(
        outside.path().join("x.md"),
        zone.join(SESSION).join("artifacts/a.md"),
    )
    .expect("link");
    std::fs::write(
        zone.join(SESSION).join("README.md"),
        README_TEXT.replace(
            "| ---- |\n",
            "| ---- |\n| workspace/a.md | artifacts/a.md | |\n",
        ),
    )
    .expect("readme");
    put(
        &zone,
        "artifacts/knowledge/t/bytes.md",
        b"\xff\xfe\n",
        AN_HOUR,
    );
    let big = "x".repeat(knowledge::MAX_NOTE_BYTES + 1);
    put(
        &zone,
        "artifacts/knowledge/t/big.md",
        big.as_bytes(),
        AN_HOUR,
    );
    let denied = zone.join(SESSION).join("workspace/denied");
    std::fs::create_dir_all(&denied).expect("denied");
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert_eq!(vm.rows[0].state, PromoteState::Unknown);
    assert!(vm.rows[0].problem.is_some());
    let notes: Vec<(&str, bool, bool)> = vm
        .knowledge
        .iter()
        .map(|note| {
            (
                note.path.as_str(),
                note.problem.is_some(),
                note.revision.is_some(),
            )
        })
        .collect();
    assert_eq!(
        notes,
        [
            ("artifacts/knowledge/t/big.md", true, false),
            ("artifacts/knowledge/t/bytes.md", true, false)
        ]
    );
    assert_eq!(vm.knowledge[0].bytes, big.len() as u64);
    assert!(
        vm.problems
            .iter()
            .any(|problem| problem.contains("workspace/denied")),
        "{:?}",
        vm.problems
    );

    for n in 0..=4096 {
        std::fs::write(zone.join(SESSION).join(format!("workspace/f{n}")), "").expect("file");
    }
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert!(
        vm.problems.iter().any(|problem| problem.contains("4096")),
        "{:?}",
        vm.problems
    );
}

/// The drive at `root` as a git repository, committing `paths` at the
/// epoch second `at`.
fn git(root: &Path, args: &[&str], at: i64) {
    let date = format!("{at} +0000");
    let status = std::process::Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@h",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn set_mtime(path: &Path, age: Duration) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(SystemTime::now() - age))
        .expect("mtime");
}

/// R95K-11: "newer" is told by what changed, not by a file's mtime. A
/// checkout that leaves the vault copy's mtime newer than the candidate's
/// does not hide a candidate edited and committed after promotion; and
/// after the candidate is edited here, a review-only write to the vault
/// copy — committed or not — does not make the copy the newer one.
#[test]
fn staleness_is_by_what_changed_not_by_mtime() {
    let (drive, zone) = drive();
    let root = drive.path();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(root);
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(root);
    let out = out(&profile, &vault, Some(&pinned));
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    let t0 = 1_790_000_000;
    git(root, &["init", "-q"], t0);
    git(root, &["add", "-A"], t0);
    git(root, &["commit", "-q", "-m", "promoted"], t0);
    let candidate = zone.join(SESSION).join(NOTE_REL);
    std::fs::write(&candidate, NOTE.replace("Three", "Four")).expect("edited");
    git(root, &["commit", "-q", "-am", "edited"], t0 + 60);
    // A checkout: the copy's mtime now, the candidate's an hour ago.
    set_mtime(&candidate, AN_HOUR);
    set_mtime(&root.join(TARGET), Duration::ZERO);
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.knowledge[0].state, Some(PromoteState::Stale));

    // Edited again here, then the copy unticked and ticked: review only.
    std::fs::write(&candidate, NOTE.replace("Three", "Five")).expect("edited");
    set_mtime(&candidate, AN_HOUR);
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out).expect("unticked");
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, true, &out).expect("ticked");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.knowledge[0].state, Some(PromoteState::Stale));
    git(root, &["commit", "-q", "-am", "ticked"], t0 + 120);
    set_mtime(&root.join(TARGET), Duration::ZERO);
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.knowledge[0].state, Some(PromoteState::Stale));
}

/// R95K2-06: review-only commits never date what a vault copy says, at
/// the history's edge either: sixteen tick/untick commits after the
/// candidate changed leave no commit read that says when the copy's text
/// last changed, so the row is `unknown` — never `ok` by the time of a
/// tick.
#[test]
fn review_commits_past_the_history_read_never_date_the_copy() {
    let (drive, zone) = drive();
    let root = drive.path();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(root);
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(root);
    let out = out(&profile, &vault, Some(&pinned));
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    let t0 = 1_790_000_000;
    git(root, &["init", "-q"], t0);
    git(root, &["add", "-A"], t0);
    git(root, &["commit", "-q", "-m", "promoted"], t0);
    std::fs::write(
        zone.join(SESSION).join(NOTE_REL),
        NOTE.replace("Three", "Four"),
    )
    .expect("edited");
    git(root, &["commit", "-q", "-am", "edited"], t0 + 60);
    for n in 0..16 {
        review(&zone, SESSION, NOTE_REL, "tgorka", AT, n % 2 == 1, &out).expect("review");
        git(root, &["commit", "-q", "-am", "review"], t0 + 120 + n);
    }
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.knowledge[0].state, Some(PromoteState::Unknown));
}

/// R95K2-06: before a vault copy has any commit, a tick moves its mtime
/// and nothing it says, so a candidate edited after the promotion is not
/// read as older than the copy: the row is `unknown`, never `ok`.
#[test]
fn a_tick_before_the_first_commit_never_dates_the_copy() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    let candidate = zone.join(SESSION).join(NOTE_REL);
    std::fs::write(&candidate, NOTE.replace("Three", "Four")).expect("edited");
    set_mtime(&candidate, Duration::from_secs(60));
    set_mtime(&drive.path().join(TARGET), AN_HOUR);
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out).expect("unticked");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.knowledge[0].state, Some(PromoteState::Unknown));
}

/// R95K2-07: a row's file behind a folder that may not be searched is
/// there as far as anyone knows — `unknown`, with why — for a source, a
/// target in the session and a target in the drive alike; only a file the
/// disk says is not there is missing.
#[cfg(unix)]
#[test]
fn a_row_file_behind_a_closed_folder_is_unknown_not_missing() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/closed/a.md", b"a\n", AN_HOUR);
    put(&zone, "artifacts/a.md", b"a\n", AN_HOUR);
    put(&zone, "workspace/b.md", b"b\n", AN_HOUR);
    put(&zone, "artifacts/closed/b.md", b"b\n", AN_HOUR);
    put(&zone, "artifacts/c.md", b"c\n", AN_HOUR);
    std::fs::create_dir_all(drive.path().join("10-notes/closed")).expect("vault");
    std::fs::write(drive.path().join("10-notes/closed/c.md"), "c\n").expect("copy");
    put(&zone, "workspace/d.md", b"d\n", AN_HOUR);
    std::fs::write(
        zone.join(SESSION).join("README.md"),
        README_TEXT.replace(
            "| ---- |\n",
            "| ---- |\n| workspace/closed/a.md | artifacts/a.md | |\n| workspace/b.md | artifacts/closed/b.md | |\n| artifacts/c.md | 10-notes/closed/c.md | |\n| workspace/d.md | artifacts/gone/d.md | |\n",
        ),
    )
    .expect("readme");
    let closed = [
        zone.join(SESSION).join("workspace/closed"),
        zone.join(SESSION).join("artifacts/closed"),
        drive.path().join("10-notes/closed"),
    ];
    for dir in &closed {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    }
    let vm = panel(&zone, SESSION, &out, None);
    for dir in &closed {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let vm = vm.expect("panel");
    let states: Vec<(PromoteState, bool)> = vm
        .rows
        .iter()
        .map(|row| (row.state, row.problem.is_some()))
        .collect();
    assert_eq!(
        states,
        [
            (PromoteState::Unknown, true),
            (PromoteState::Unknown, true),
            (PromoteState::Unknown, true),
            (PromoteState::MissingTarget, false),
        ]
    );
}

/// R95K2-01: a person's session on a drive whose agents folder may not be
/// searched has an audience nobody can establish — its `_drive.toml` may
/// be there — so a promotion out is refused and the panel says so; once
/// the disk says there is no declaration, and no pin, the drive is
/// anyone's and it is promoted.
#[cfg(unix)]
#[test]
fn a_declaration_that_cannot_be_looked_at_is_not_an_absence() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone) = drive();
    put(&zone, "artifacts/plan.md", b"# Plan\n", AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let agents = profile.agents_root().expect("agents zone");
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    let plan = Request {
        source: "artifacts/plan.md",
        target: "10-notes/plan.md",
        note: "",
        expected: None,
    };
    std::fs::set_permissions(&agents, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let refused = promote_out(&zone, SESSION, &plan, None, &out);
    let vm = panel(&zone, SESSION, &out, None);
    std::fs::set_permissions(&agents, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    assert!(vm.expect("panel").out_refused.is_some());
    assert!(!drive.path().join("10-notes/plan.md").exists());
    assert_eq!(readme(&zone), README_TEXT);

    std::fs::remove_dir_all(&agents).expect("no zone");
    promote_out(&zone, SESSION, &plan, None, &out).expect("anyone's drive");
    assert!(drive.path().join("10-notes/plan.md").is_file());
}

/// R95K2-01: the frontier a promotion compares the label's log against is
/// read whole or not at all — a `log/` that will not list is no empty log.
#[cfg(unix)]
#[test]
fn a_log_that_will_not_list_has_no_frontier() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_drive, zone) = drive();
    assert_eq!(frontier(&zone, SESSION), Ok(Vec::new()));
    append_log(&zone, &label_line(&[TG], 0));
    let log = zone.join(SESSION).join("log");
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let seen = frontier(&zone, SESSION);
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert!(seen.is_err(), "{seen:?}");
}

/// R95K2-02: a staging name planted before a promotion — a link to a file
/// outside, or a second name of one — is never written through: the copy
/// and the README's row land as new regular files with the checked bytes,
/// and the outside file keeps its own.
#[cfg(unix)]
#[test]
fn a_planted_stage_is_never_written_through() {
    let outside = tempfile::tempdir().expect("outside");
    for hard in [false, true] {
        let (_drive, zone) = drive();
        put(&zone, "workspace/a.md", b"checked\n", AN_HOUR);
        put(&zone, "artifacts/a.md", b"before\n", AN_HOUR);
        let mut theirs = Vec::new();
        for (n, stage) in ["artifacts/.a.md.keeper-tmp", ".README.md.keeper-tmp"]
            .into_iter()
            .enumerate()
        {
            let file = outside.path().join(format!("{hard}-{n}"));
            std::fs::write(&file, "theirs\n").expect("theirs");
            let planted = zone.join(SESSION).join(stage);
            if hard {
                std::fs::hard_link(&file, &planted).expect("hard link");
            } else {
                std::os::unix::fs::symlink(&file, &planted).expect("link");
            }
            theirs.push(file);
        }
        promote_in(
            &zone,
            SESSION,
            "workspace/a.md",
            "artifacts/a.md",
            "",
            SETTLE_MS,
            now_ms(),
        )
        .expect("promoted");
        for file in &theirs {
            assert_eq!(
                std::fs::read_to_string(file).ok().as_deref(),
                Some("theirs\n"),
                "hard: {hard}"
            );
        }
        for written in ["artifacts/a.md", "README.md"] {
            let meta = std::fs::symlink_metadata(zone.join(SESSION).join(written)).expect("there");
            assert!(meta.file_type().is_file(), "{written}, hard: {hard}");
        }
        assert_eq!(
            read(&zone, "artifacts/a.md").as_deref(),
            Some(&b"checked\n"[..])
        );
        assert!(readme(&zone).contains("| workspace/a.md | artifacts/a.md |  |\n"));
    }
}

/// What a promotion out of `NOTE_REL` to `TARGET` keeps while it runs.
fn pending(text: &str) -> Pending {
    Pending {
        session: SESSION.to_owned(),
        session_id: "01J5AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
        source: NOTE_REL.to_owned(),
        target: TARGET.to_owned(),
        note: "knowledge".to_owned(),
        profile: "P1".to_owned(),
        vault_relative: "knowledge/short.md".to_owned(),
        text: text.to_owned(),
    }
}

/// The row of `NOTE_REL` → `TARGET` once `copy` is published there: its
/// fourth cell the copy's digest.
fn published_row(copy: &str) -> String {
    format!(
        "| {NOTE_REL} | {TARGET} | knowledge | {} |\n",
        promote::copy_digest(NOTE_REL, copy.as_bytes())
    )
}

/// R95K2-03: a promotion out interrupted anywhere between its admission
/// and its end — before its row, after its row and before its copy (a
/// write that failed), after its copy and before its record cleared — is
/// finished by the next promotion out of the zone with exactly the
/// version that was reviewed, whatever the candidate became or whether it
/// is there at all; and refused, said and cleared, where that version may
/// no longer go — another file at a target it was not to replace.
#[test]
fn an_interrupted_promotion_out_publishes_the_version_it_reviewed() {
    let reviewed = knowledge::review(NOTE, "tgorka", AT, true);
    let setup = || {
        let (drive, zone) = drive();
        put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
        put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
        (drive, zone)
    };
    let other = Request {
        source: "artifacts/other.md",
        target: "10-notes/other.md",
        note: "",
        expected: None,
    };
    let row = format!("| {NOTE_REL} | {TARGET} | knowledge |\n");
    let pinned = pin(&[TG]);

    // After the row, the copy's write failed; the candidate then changed.
    {
        let (drive, zone) = setup();
        let profile = profile(drive.path());
        declare(&profile, &[TG]);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        vault.failing.store(true, Ordering::SeqCst);
        let failed = promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out,
        );
        assert!(matches!(failed, Err(VerbError::Refused(_))), "{failed:?}");
        vault.failing.store(false, Ordering::SeqCst);
        put(
            &zone,
            NOTE_REL,
            NOTE.replace("Three", "Four").as_bytes(),
            AN_HOUR,
        );
        promote_out(&zone, SESSION, &other, None, &out).expect("the next promotion");
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET)).ok(),
            Some(reviewed.clone())
        );
        assert!(drive.path().join("10-notes/other.md").is_file());
        assert!(!zone.join(PENDING_REL).exists());
    }

    // Before the row, and the candidate gone since.
    {
        let (drive, zone) = setup();
        let profile = profile(drive.path());
        declare(&profile, &[TG]);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        keep_pending(&zone, &pending(&reviewed)).expect("kept");
        std::fs::remove_file(zone.join(SESSION).join(NOTE_REL)).expect("gone");
        promote_out(&zone, SESSION, &other, None, &out).expect("the next promotion");
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET)).ok(),
            Some(reviewed.clone())
        );
        assert_eq!(readme(&zone).matches(&published_row(&reviewed)).count(), 1);
        assert!(!zone.join(PENDING_REL).exists());
    }

    // After the copy, before the record was cleared.
    {
        let (drive, zone) = setup();
        let profile = profile(drive.path());
        declare(&profile, &[TG]);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        keep_pending(&zone, &pending(&reviewed)).expect("kept");
        let readme_path = zone.join(SESSION).join("README.md");
        std::fs::write(
            &readme_path,
            README_TEXT.replace("| ---- |\n", &format!("| ---- |\n{row}")),
        )
        .expect("row");
        vault
            .write("P1", "10-notes", "knowledge/short.md", &reviewed)
            .expect("copy");
        promote_out(&zone, SESSION, &other, None, &out).expect("the next promotion");
        assert_eq!(readme(&zone).matches(NOTE_REL).count(), 1);
        assert!(readme(&zone).contains(&published_row(&reviewed)));
        assert!(!zone.join(PENDING_REL).exists());
    }

    // Someone else's file at a target it was not to replace.
    {
        let (drive, zone) = setup();
        let profile = profile(drive.path());
        declare(&profile, &[TG]);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        keep_pending(&zone, &pending(&reviewed)).expect("kept");
        vault
            .write("P1", "10-notes", "knowledge/short.md", "theirs\n")
            .expect("theirs");
        let refused = promote_out(&zone, SESSION, &other, None, &out);
        assert!(
            matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains(NOTE_REL)),
            "{refused:?}"
        );
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET))
                .ok()
                .as_deref(),
            Some("theirs\n")
        );
        assert!(!zone.join(PENDING_REL).exists());
        promote_out(&zone, SESSION, &other, None, &out).expect("refused once, then cleared");

        // R95K3-04, R244: the row the refused recovery recorded names no
        // copy, so promoting the same source to the same target again does
        // not take the file it refused for one.
        let again = promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out,
        );
        assert!(matches!(again, Err(VerbError::Refused(_))), "{again:?}");
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET))
                .ok()
                .as_deref(),
            Some("theirs\n")
        );

        // Once the file is gone, the same promotion publishes, and its row
        // then records the copy it made: promoting again replaces it.
        std::fs::remove_file(drive.path().join(TARGET)).expect("theirs gone");
        promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out,
        )
        .expect("published");
        put(
            &zone,
            NOTE_REL,
            NOTE.replace("Three", "Four").as_bytes(),
            AN_HOUR,
        );
        let four = NOTE.replace("Three", "Four");
        promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(&four))),
            Some(&tgorka()),
            &out,
        )
        .expect("re-promoted");
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET)).ok(),
            Some(knowledge::review(&four, "tgorka", AT, true))
        );
    }
}

/// A drive with `NOTE_REL` in its session, declared and pinned as tgorka's,
/// whose first promotion of it out to `TARGET` failed in the vault's write:
/// the row recorded, the reviewed version kept in the zone.
fn interrupted() -> (tempfile::TempDir, PathBuf, SyncProfile, DrivePin) {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    vault.failing.store(true, Ordering::SeqCst);
    let failed = promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out(&profile, &vault, Some(&pinned)),
    );
    assert!(matches!(failed, Err(VerbError::Refused(_))), "{failed:?}");
    assert!(zone.join(PENDING_REL).is_file());
    (drive, zone, profile, pinned)
}

const OTHER: Request<'static> = Request {
    source: "artifacts/other.md",
    target: "10-notes/other.md",
    note: "",
    expected: None,
};

/// R95K3-01: a person's session on a drive with no pin is anyone's only
/// where the disk says its agents folder or its `_drive.toml` is not
/// there; an agents folder that is a link to nothing, or a file where the
/// folder should be, is not known and refuses the promotion. R244: so too
/// when the profile spells its agents subfolder `80-agents/` or
/// `./80-agents`.
#[cfg(unix)]
#[test]
fn an_agents_folder_the_disk_cannot_vouch_for_is_no_absence() {
    let plan = Request {
        source: "artifacts/plan.md",
        target: "10-notes/plan.md",
        note: "",
        expected: None,
    };
    for spelling in ["{}", "{}/", "./{}"] {
        for (shape, admitted) in [
            ("no declaration", true),
            ("no agents folder", true),
            ("a link to nothing", false),
            ("a file", false),
        ] {
            let (drive, zone) = drive();
            put(&zone, "artifacts/plan.md", b"# Plan\n", AN_HOUR);
            let mut profile = profile(drive.path());
            let agents = profile.agents_root().expect("agents zone");
            let config = profile.agents.as_mut().expect("agents");
            config.subfolder = spelling.replace("{}", &config.subfolder);
            match shape {
                "no declaration" => std::fs::create_dir_all(&agents).expect("agents"),
                "a link to nothing" => {
                    std::os::unix::fs::symlink(drive.path().join("gone"), &agents).expect("link");
                }
                "a file" => std::fs::write(&agents, "x").expect("file"),
                _ => {}
            }
            let vault = Vault::at(drive.path());
            let promoted = promote_out(&zone, SESSION, &plan, None, &out(&profile, &vault, None));
            assert_eq!(
                promoted.is_ok(),
                admitted,
                "{spelling} {shape}: {promoted:?}"
            );
            assert_eq!(
                drive.path().join("10-notes/plan.md").is_file(),
                admitted,
                "{spelling} {shape}"
            );
        }
    }
}

/// R95K3-02: an interrupted promotion is finished only into the vault it
/// was admitted to. The drive's vault moved to `20-notes/` since: the
/// recovery is refused — nothing written at the old target or at the new
/// place, another's file already there kept — and its record cleared.
#[test]
fn a_moved_vault_is_never_written_by_an_interrupted_promotion() {
    let (drive, zone, profile, pinned) = interrupted();
    let vault = Vault::at(drive.path());
    *vault.subfolder.lock().expect("subfolder") = "20-notes".to_owned();
    let moved_to = drive.path().join("20-notes/knowledge/short.md");
    std::fs::create_dir_all(moved_to.parent().expect("parent")).expect("dir");
    std::fs::write(&moved_to, "theirs\n").expect("theirs");
    let out = out(&profile, &vault, Some(&pinned));

    let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains(NOTE_REL)),
        "{refused:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&moved_to).ok().as_deref(),
        Some("theirs\n")
    );
    assert!(!drive.path().join(TARGET).exists());
    assert!(!zone.join(PENDING_REL).exists());
}

/// R95K3-03: what is at the target of an interrupted promotion and cannot
/// be read is not an absence: the recovery is refused, the file and the
/// rest of the folder as they were, never written over.
#[cfg(unix)]
#[test]
fn an_unreadable_target_is_never_written_by_an_interrupted_promotion() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone, profile, pinned) = interrupted();
    let vault = Vault::at(drive.path());
    let target = drive.path().join(TARGET);
    std::fs::create_dir_all(target.parent().expect("parent")).expect("dir");
    std::fs::write(&target, "theirs\n").expect("theirs");
    std::fs::write(target.with_file_name("beside.md"), "beside\n").expect("beside");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let refused = promote_out(
        &zone,
        SESSION,
        &OTHER,
        None,
        &out(&profile, &vault, Some(&pinned)),
    );
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    assert_eq!(
        std::fs::read_to_string(&target).ok().as_deref(),
        Some("theirs\n")
    );
    assert_eq!(
        std::fs::read_to_string(target.with_file_name("beside.md"))
            .ok()
            .as_deref(),
        Some("beside\n")
    );
}

/// R95K3-05: a person who archives or deletes the session whose promotion
/// out was interrupted does not block the zone: the next promotion out of
/// another of its sessions is refused once, saying that session is gone,
/// the record cleared; then it, and a review, go through.
#[test]
fn a_session_moved_away_never_blocks_the_zone() {
    const SECOND: &str = "active/2026-10-07-second";
    for gone in ["archive", "delete"] {
        let (drive, zone, profile, pinned) = interrupted();
        let second = zone.join(SECOND);
        std::fs::create_dir_all(second.join("artifacts")).expect("second");
        std::fs::write(
            second.join("README.md"),
            README_TEXT.replace("01J5AAAAAAAAAAAAAAAAAAAAAA", "01J5BBBBBBBBBBBBBBBBBBBBBB"),
        )
        .expect("readme");
        std::fs::write(second.join("artifacts/other.md"), "other\n").expect("other");
        match gone {
            "archive" => crate::sessions::verbs::archive(
                &zone,
                "01J5AAAAAAAAAAAAAAAAAAAAAA",
                Vec::new(),
                false,
                2026,
            ),
            _ => crate::sessions::verbs::delete(&zone, "01J5AAAAAAAAAAAAAAAAAAAAAA"),
        }
        .expect(gone);
        assert!(!zone.join(SESSION).exists(), "{gone}");
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));

        let refused = promote_out(&zone, SECOND, &OTHER, None, &out);
        assert!(
            matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains(SESSION)),
            "{gone}: {refused:?}"
        );
        assert!(!zone.join(PENDING_REL).exists(), "{gone}");
        promote_out(&zone, SECOND, &OTHER, None, &out).expect("the zone is usable");
        assert!(drive.path().join("10-notes/other.md").is_file(), "{gone}");
        let review = review(&zone, SECOND, NOTE_REL, "tgorka", AT, true, &out);
        assert!(
            matches!(&review, Err(VerbError::Refused(sentence)) if sentence == NOT_PROMOTED),
            "{gone}: {review:?}"
        );
    }
}

/// R95K3-06: a promotion record that does not read is refused with why,
/// never passed over: kept, byte for byte, where the refusal says, and the
/// next promotion goes through. One that cannot be set aside stays where
/// it is, and every promotion is refused — none writes over it — until it
/// can be.
#[cfg(unix)]
#[test]
fn an_unreadable_promotion_record_is_refused_and_kept() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone) = drive();
    put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    drop(exec::hold(&zone).expect("the zone's .keeper/"));
    let garbled = "{\"session\": \"active/2026-10-06-harvest\", \"sour";
    std::fs::write(zone.join(PENDING_REL), garbled).expect("garbled");
    let keeper = zone.join(".keeper");

    std::fs::set_permissions(&keeper, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let stuck = [
        promote_out(&zone, SESSION, &OTHER, None, &out),
        promote_out(&zone, SESSION, &OTHER, None, &out),
    ];
    std::fs::set_permissions(&keeper, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    for refused in &stuck {
        assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    }
    assert_eq!(
        std::fs::read_to_string(zone.join(PENDING_REL))
            .ok()
            .as_deref(),
        Some(garbled)
    );
    assert!(!drive.path().join("10-notes/other.md").exists());

    let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
    let Err(VerbError::Refused(sentence)) = &refused else {
        panic!("{refused:?}");
    };
    let kept = std::fs::read_dir(&keeper)
        .expect("listed")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| name.ends_with(".unreadable.json"))
        .expect("kept");
    assert!(sentence.contains(&kept), "{sentence}");
    assert_eq!(
        std::fs::read_to_string(keeper.join(&kept)).ok().as_deref(),
        Some(garbled)
    );
    assert!(!drive.path().join("10-notes/other.md").exists());
    promote_out(&zone, SESSION, &OTHER, None, &out).expect("the next promotion");
    assert!(drive.path().join("10-notes/other.md").is_file());
}

/// R95K3-07: an interrupted promotion's record goes only once the vault
/// says its copy is durable. A copy that landed and was not synced keeps
/// the record — whether it was written now or was there already — and
/// the next promotion finishes it.
#[test]
fn a_copy_not_yet_durable_keeps_its_promotion() {
    let reviewed = knowledge::review(NOTE, "tgorka", AT, true);
    for there_already in [false, true] {
        let (drive, zone, profile, pinned) = interrupted();
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        if there_already {
            vault
                .write("P1", "10-notes", "knowledge/short.md", &reviewed)
                .expect("copy");
        }
        vault.unflushed.store(true, Ordering::SeqCst);
        let failed = promote_out(&zone, SESSION, &OTHER, None, &out);
        assert!(matches!(failed, Err(VerbError::Refused(_))), "{failed:?}");
        let kept: Pending =
            serde_json::from_str(&std::fs::read_to_string(zone.join(PENDING_REL)).expect("kept"))
                .expect("a record");
        assert_eq!(
            (kept.source.as_str(), kept.target.as_str()),
            (NOTE_REL, TARGET),
            "there already: {there_already}"
        );

        vault.unflushed.store(false, Ordering::SeqCst);
        promote_out(&zone, SESSION, &OTHER, None, &out).expect("finished");
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET)).ok(),
            Some(reviewed.clone())
        );
        assert!(!zone.join(PENDING_REL).exists());
    }
}

/// `from`'s drive as another Mac has it once it synced: every file but
/// the `.keeper/` folders, which never sync.
fn synced_copy(from: &Path) -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        for entry in std::fs::read_dir(from).expect("listed") {
            let entry = entry.expect("entry");
            let name = entry.file_name();
            if name == ".keeper" {
                continue;
            }
            let kind = entry.file_type().expect("kind");
            if kind.is_dir() {
                std::fs::create_dir(to.join(&name)).expect("folder");
                copy(&entry.path(), &to.join(&name));
            } else if kind.is_file() {
                std::fs::copy(entry.path(), to.join(&name)).expect("file");
            }
        }
    }
    let to = tempfile::tempdir().expect("the other Mac");
    copy(from, to.path());
    to
}

/// R244 (R95K4-02): the authority to replace a vault copy travels with the
/// session, never in this Mac's `.keeper/`. A first publication failed
/// after its row, and another's file then took its target — refused here,
/// or not yet tried here. On another Mac that synced the session and the
/// vault, `.keeper/` not, promoting the same source to the same target is
/// refused and the other's bytes kept.
#[test]
fn a_row_no_copy_stands_behind_grants_nothing_on_another_mac() {
    for refused_here in [true, false] {
        let (drive, zone, profile, pinned) = interrupted();
        let vault = Vault::at(drive.path());
        vault
            .write("P1", "10-notes", "knowledge/short.md", "theirs\n")
            .expect("theirs");
        if refused_here {
            let refused = promote_out(
                &zone,
                SESSION,
                &OTHER,
                None,
                &out(&profile, &vault, Some(&pinned)),
            );
            assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
        }
        assert!(readme(&zone).contains(&format!("| {NOTE_REL} | {TARGET} | knowledge |\n")));

        let mac = synced_copy(drive.path());
        let (zone, profile) = (mac.path().join("60-sessions"), self::profile(mac.path()));
        assert!(!zone.join(PENDING_REL).exists());
        let vault = Vault::at(mac.path());
        let again = promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out(&profile, &vault, Some(&pinned)),
        );
        assert!(
            matches!(again, Err(VerbError::Refused(_))),
            "refused here: {refused_here}: {again:?}"
        );
        assert_eq!(
            std::fs::read_to_string(mac.path().join(TARGET))
                .ok()
                .as_deref(),
            Some("theirs\n"),
            "refused here: {refused_here}"
        );
    }
}

/// R244 (R95K4-03): a record set aside leaves the row its publication wrote
/// with no authority. A first publication failed after its row; its record
/// then does not decode — garbled, or one that names no session id — and
/// is refused and set aside; with another's file at the target, promoting
/// the same source to the same target is refused, the bytes kept.
#[test]
fn a_set_aside_record_leaves_its_row_without_authority() {
    for record in ["garbled", "no session id"] {
        let (drive, zone, profile, pinned) = interrupted();
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        let text = if record == "garbled" {
            "{\"session\": \"active/".to_owned()
        } else {
            let mut kept: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(zone.join(PENDING_REL)).expect("kept"),
            )
            .expect("a record");
            kept.as_object_mut()
                .expect("an object")
                .remove("session_id")
                .expect("its session id");
            kept.to_string()
        };
        std::fs::write(zone.join(PENDING_REL), text).expect("record");
        vault
            .write("P1", "10-notes", "knowledge/short.md", "theirs\n")
            .expect("theirs");
        let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
        let kept = std::fs::read_dir(zone.join(".keeper"))
            .expect("listed")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name.ends_with(".unreadable.json"))
            .expect("set aside");
        assert!(
            matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains(&kept)),
            "{record}: {refused:?}"
        );
        let again = promote_out(
            &zone,
            SESSION,
            &request(TARGET, Some(&sha256_hex(NOTE))),
            Some(&tgorka()),
            &out,
        );
        assert!(
            matches!(again, Err(VerbError::Refused(_))),
            "{record}: {again:?}"
        );
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET))
                .ok()
                .as_deref(),
            Some("theirs\n"),
            "{record}"
        );
    }
}

/// R244 (R95K4-04): a review is written only into the copy its row records
/// as the note's. Another's file — carrying marta's review — at the target
/// of a row whose publication was refused: a tick and an untick are both
/// refused, the file's bytes kept, and the panel shows no review of the
/// note from it. Once the note is published there, its review goes out
/// and in.
#[test]
fn a_review_never_writes_into_a_file_the_note_did_not_publish() {
    let (drive, zone, profile, pinned) = interrupted();
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let theirs = knowledge::review("---\ntitle: Theirs\n---\n\nTheirs.\n", "marta", AT, true);
    vault
        .write("P1", "10-notes", "knowledge/short.md", &theirs)
        .expect("theirs");
    let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    let vm = panel(&zone, SESSION, &out, Some("human:marta")).expect("panel");
    assert_eq!(vm.knowledge[0].reviewed_by, None);
    assert!(!vm.knowledge[0].reviewed_by_me);
    for reviewed in [true, false] {
        let refused = review(&zone, SESSION, NOTE_REL, "tgorka", AT, reviewed, &out);
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "reviewed {reviewed}: {refused:?}"
        );
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET)).ok(),
            Some(theirs.clone()),
            "reviewed {reviewed}"
        );
    }

    std::fs::remove_file(drive.path().join(TARGET)).expect("theirs gone");
    promote_out(
        &zone,
        SESSION,
        &request(TARGET, Some(&sha256_hex(NOTE))),
        Some(&tgorka()),
        &out,
    )
    .expect("published");
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out).expect("unticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET))
            .ok()
            .as_deref(),
        Some(NOTE)
    );
    review(&zone, SESSION, NOTE_REL, "tgorka", AT, true, &out).expect("ticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(knowledge::review(NOTE, "tgorka", AT, true))
    );
}

/// R244 (R95K4-05): an interrupted promotion is finished only into the
/// session it was admitted from. That session archived or deleted, and
/// another made at the path it was at: the next promotion out is refused
/// once, naming the session there now, its record cleared, and nothing is
/// written into the new session — its README as it was, no copy at the
/// target or another's file there kept — then the zone goes on.
#[test]
fn a_session_at_a_reused_path_is_never_finished_into() {
    for gone in ["archive", "delete"] {
        for occupied in [false, true] {
            let (drive, zone, profile, pinned) = interrupted();
            match gone {
                "archive" => crate::sessions::verbs::archive(
                    &zone,
                    "01J5AAAAAAAAAAAAAAAAAAAAAA",
                    Vec::new(),
                    false,
                    2026,
                ),
                _ => crate::sessions::verbs::delete(&zone, "01J5AAAAAAAAAAAAAAAAAAAAAA"),
            }
            .expect(gone);
            let fresh =
                README_TEXT.replace("01J5AAAAAAAAAAAAAAAAAAAAAA", "01J5CCCCCCCCCCCCCCCCCCCCCC");
            std::fs::create_dir_all(zone.join(SESSION).join("artifacts")).expect("another session");
            std::fs::write(zone.join(SESSION).join("README.md"), &fresh).expect("readme");
            put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
            let vault = Vault::at(drive.path());
            if occupied {
                vault
                    .write("P1", "10-notes", "knowledge/short.md", "theirs\n")
                    .expect("theirs");
            }
            let out = out(&profile, &vault, Some(&pinned));

            let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
            assert!(
                matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains("01J5CCCCCCCCCCCCCCCCCCCCCC")),
                "{gone} {occupied}: {refused:?}"
            );
            assert_eq!(readme(&zone), fresh, "{gone} {occupied}");
            assert_eq!(
                std::fs::read_to_string(drive.path().join(TARGET))
                    .ok()
                    .as_deref(),
                occupied.then_some("theirs\n"),
                "{gone} {occupied}"
            );
            assert!(!zone.join(PENDING_REL).exists(), "{gone} {occupied}");
            promote_out(&zone, SESSION, &OTHER, None, &out).expect("the zone goes on");
        }
    }
}

/// R244 (R95K4-07): a record that does not read is set aside only once its
/// copy is on the disk. With `.keeper/` writable but its entry list
/// unsyncable, the copy can be made and not synced: the record stays where
/// it was, byte for byte, no copy beside it, and every promotion and review
/// is refused — each a fresh start, only the disk carrying anything — until
/// it can be set aside; then the next goes through.
#[cfg(unix)]
#[test]
fn a_record_is_set_aside_only_once_its_copy_is_durable() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    drop(exec::hold(&zone).expect("the zone's .keeper/"));
    let garbled = "{\"session\": \"active/2026-10-06-harvest\", \"sour";
    std::fs::write(zone.join(PENDING_REL), garbled).expect("garbled");
    let keeper = zone.join(".keeper");
    let set_aside = || {
        std::fs::read_dir(&keeper)
            .expect("listed")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".unreadable.json"))
            .collect::<Vec<_>>()
    };

    std::fs::set_permissions(&keeper, std::fs::Permissions::from_mode(0o300)).expect("chmod");
    let stuck = [
        promote_out(&zone, SESSION, &OTHER, None, &out),
        review(&zone, SESSION, NOTE_REL, "tgorka", AT, true, &out),
        promote_out(&zone, SESSION, &OTHER, None, &out),
    ];
    std::fs::set_permissions(&keeper, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    for refused in &stuck {
        assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    }
    assert_eq!(
        std::fs::read_to_string(zone.join(PENDING_REL))
            .ok()
            .as_deref(),
        Some(garbled)
    );
    assert_eq!(set_aside(), Vec::<String>::new());
    assert!(!drive.path().join("10-notes/other.md").exists());

    let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
    let kept = set_aside();
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if kept.len() == 1 && sentence.contains(&kept[0])),
        "{refused:?} {kept:?}"
    );
    assert_eq!(
        std::fs::read_to_string(keeper.join(&kept[0]))
            .ok()
            .as_deref(),
        Some(garbled)
    );
    promote_out(&zone, SESSION, &OTHER, None, &out).expect("the next promotion");
    assert!(drive.path().join("10-notes/other.md").is_file());
}

/// R244 (R95K4-05): which session is at an interrupted promotion's path is
/// read, never stood in for: a session record that cannot be read refuses
/// the next promotion out and keeps the record; once it reads, the
/// promotion is finished.
#[cfg(unix)]
#[test]
fn an_unreadable_session_record_keeps_its_promotion() {
    use std::os::unix::fs::PermissionsExt as _;
    let (drive, zone, profile, pinned) = interrupted();
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let record = zone.join(SESSION).join("README.md");
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
    assert!(zone.join(PENDING_REL).is_file());
    assert!(!drive.path().join(TARGET).exists());
    promote_out(&zone, SESSION, &OTHER, None, &out).expect("finished");
    assert!(drive.path().join(TARGET).is_file());
}

/// A drive with `NOTE_REL` and `artifacts/other.md` in its session,
/// declared and pinned as tgorka's.
fn harvest_drive() -> (tempfile::TempDir, PathBuf, SyncProfile, DrivePin) {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    put(&zone, "artifacts/other.md", b"other\n", AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    (drive, zone, profile, pin(&[TG]))
}

/// The sentence a refused verb says; `None` for anything else.
fn refusal(result: Result<(), VerbError>) -> Option<String> {
    match result {
        Err(VerbError::Refused(sentence)) => Some(sentence),
        _ => None,
    }
}

/// Promote the session's harvested note, read as `text`, to `target` as
/// tgorka.
fn promote_note(zone: &Path, target: &str, text: &str, out: &OutOf) -> Result<(), VerbError> {
    promote_out(
        zone,
        SESSION,
        &request(target, Some(&sha256_hex(text))),
        Some(&tgorka()),
        out,
    )
}

/// R253 (R95K5-03, R95K5-06): identical bytes never give a promotion a
/// file it did not publish. With the reviewed copy's exact bytes already at
/// the target — and no row; a row of three cells; a row whose publication
/// was interrupted and its record set aside; or another session's published
/// copy — promoting the note there is refused, the file and the README
/// kept, and so is promoting it again once the candidate changed. Only the
/// interrupted operation itself finishes onto its own bytes
/// (`an_interrupted_promotion_out_publishes_the_version_it_reviewed`).
#[test]
fn identical_bytes_never_adopt_a_file() {
    const SECOND: &str = "active/2026-10-07-second";
    let reviewed = knowledge::review(NOTE, "tgorka", AT, true);
    let four = NOTE.replace("Three", "Four");
    for case in ["no row", "three cells", "set aside", "another session's"] {
        let (drive, zone, profile, pinned) = harvest_drive();
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        let land = || {
            vault
                .write("P1", "10-notes", "knowledge/short.md", &reviewed)
                .expect("the same bytes");
        };
        match case {
            "no row" => land(),
            "three cells" => {
                std::fs::write(
                    zone.join(SESSION).join("README.md"),
                    README_TEXT.replace(
                        "| ---- |\n",
                        &format!("| ---- |\n| {NOTE_REL} | {TARGET} | knowledge |\n"),
                    ),
                )
                .expect("row");
                land();
            }
            "set aside" => {
                vault.failing.store(true, Ordering::SeqCst);
                let failed = promote_note(&zone, TARGET, NOTE, &out);
                assert!(matches!(failed, Err(VerbError::Refused(_))), "{failed:?}");
                vault.failing.store(false, Ordering::SeqCst);
                land();
                std::fs::write(zone.join(PENDING_REL), "{\"session\": \"active/").expect("garbled");
                let refused = promote_out(&zone, SESSION, &OTHER, None, &out);
                assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");
                assert!(!zone.join(PENDING_REL).exists());
            }
            _ => {
                let second = zone.join(SECOND);
                std::fs::create_dir_all(second.join(NOTE_REL).parent().expect("parent"))
                    .expect("second");
                std::fs::write(
                    second.join("README.md"),
                    README_TEXT.replace("01J5AAAAAAAAAAAAAAAAAAAAAA", "01J5BBBBBBBBBBBBBBBBBBBBBB"),
                )
                .expect("readme");
                std::fs::write(second.join(NOTE_REL), NOTE).expect("note");
                set_mtime(&second.join(NOTE_REL), AN_HOUR);
                promote_out(
                    &zone,
                    SECOND,
                    &request(TARGET, Some(&sha256_hex(NOTE))),
                    Some(&tgorka()),
                    &out,
                )
                .expect("the other session's copy");
            }
        }
        let before = readme(&zone);
        let refused = promote_note(&zone, TARGET, NOTE, &out);
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{case}: {refused:?}"
        );
        put(&zone, NOTE_REL, four.as_bytes(), AN_HOUR);
        let again = promote_note(&zone, TARGET, &four, &out);
        assert!(
            matches!(again, Err(VerbError::Refused(_))),
            "{case}: {again:?}"
        );
        assert_eq!(
            std::fs::read_to_string(drive.path().join(TARGET)).ok(),
            Some(reviewed.clone()),
            "{case}"
        );
        assert_eq!(readme(&zone), before, "{case}");
    }
}

/// R252, R253 (R95K5-05): a copy that lost the note's authority says why,
/// the same through a re-promotion, a review and the panel. The person
/// edits the published copy: promoting the note again, and a tick, are
/// refused as a copy changed since; a row of three cells over the copy is
/// refused as one that records no publication. The file stays as it is.
#[test]
fn a_copy_that_lost_its_authority_says_why() {
    let (drive, zone, profile, pinned) = harvest_drive();
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    promote_note(&zone, TARGET, NOTE, &out).expect("published");
    let reviewed = knowledge::review(NOTE, "tgorka", AT, true);
    let edited = reviewed.replace("Three papers.", "Three papers, and my notes.");
    std::fs::write(drive.path().join(TARGET), &edited).expect("the person's edit");
    let changed = promote::CopyLoss::Changed.explain(NOTE_REL, TARGET);
    let refusals = [
        promote_note(&zone, TARGET, NOTE, &out),
        review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out),
    ];
    for refused in refusals {
        assert_eq!(refusal(refused), Some(changed.clone()));
    }
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(vm.knowledge[0].foreign_copy.as_ref(), Some(&changed));
    assert_eq!(vm.knowledge[0].reviewed_by, None);
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(edited)
    );

    std::fs::write(drive.path().join(TARGET), &reviewed).expect("as published");
    let path = zone.join(SESSION).join("README.md");
    let three = readme(&zone).replace(
        &published_row(&reviewed),
        &format!("| {NOTE_REL} | {TARGET} | knowledge |\n"),
    );
    std::fs::write(&path, three).expect("a row of three cells");
    let unrecorded = promote::CopyLoss::Unrecorded.explain(NOTE_REL, TARGET);
    assert_eq!(
        refusal(promote_note(&zone, TARGET, NOTE, &out)),
        Some(unrecorded.clone())
    );
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(vm.knowledge[0].foreign_copy, Some(unrecorded));
    assert_eq!(vm.knowledge[0].reviewed_by, None);
}

/// R253 (R95K5-01): where a copy's frontmatter ends is part of what it is.
/// The published copy is edited so the note's metadata lies in its body,
/// behind a block that holds only a review — byte for byte what the copy
/// said once review keys are out, read as one stream: promoting the note
/// again and a review are both refused, the edit kept.
#[test]
fn a_copy_whose_frontmatter_moved_is_not_the_notes() {
    let (drive, zone, profile, pinned) = harvest_drive();
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    promote_note(&zone, TARGET, NOTE, &out).expect("published");
    let moved = format!(
        "---\nhuman_reviewed: true\n---\n{}",
        NOTE.replace("human_reviewed: false\n", "")
    );
    std::fs::write(drive.path().join(TARGET), &moved).expect("the edit");
    let four = NOTE.replace("Three", "Four");
    put(&zone, NOTE_REL, four.as_bytes(), AN_HOUR);
    let replaced = promote_note(&zone, TARGET, &four, &out);
    assert!(
        matches!(replaced, Err(VerbError::Refused(_))),
        "{replaced:?}"
    );
    let reviewed = review(&zone, SESSION, NOTE_REL, "tgorka", AT, false, &out);
    assert!(
        matches!(reviewed, Err(VerbError::Refused(_))),
        "{reviewed:?}"
    );
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(moved)
    );
}

/// R253 (R95K5-02), R263 (R95K6-01): a review never unseats the copy it is
/// written into, and never moves where its frontmatter ends. A note promoted
/// to a target not named `.md`, one whose frontmatter the publication's
/// review carries past the head a digest of differences reads, and a block
/// of reviews alone or an empty one in front of a body that opens with a
/// block of its own: untick, tick, the panel showing each, the copy's body
/// the note's byte for byte, then the changed candidate promoted over that
/// copy.
#[test]
fn a_review_never_unseats_its_own_copy() {
    let pad = promote::HEAD_BYTES as usize - NOTE.len() - 20;
    let near_the_head = NOTE.replace(
        "status: draft\n",
        &format!("status: draft\nnotes: {}\n", "x".repeat(pad)),
    );
    let body = |text: &str| {
        let (_, offset) = keeper_core::notes::frontmatter::Frontmatter::parse(text);
        text[offset..].to_owned()
    };
    for (target, note) in [
        ("10-notes/knowledge/short.txt", NOTE.to_owned()),
        (TARGET, near_the_head),
        (
            TARGET,
            "---\nhuman_reviewed: true\n---\n---\ntitle: T\n---\nThree papers.\n".to_owned(),
        ),
        (
            TARGET,
            "---\n---\n---\ntitle: T\n---\nThree papers.\n".to_owned(),
        ),
    ] {
        let (drive, zone, profile, pinned) = harvest_drive();
        put(&zone, NOTE_REL, note.as_bytes(), AN_HOUR);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        promote_note(&zone, target, &note, &out).expect("published");
        for reviewed in [false, true] {
            review(&zone, SESSION, NOTE_REL, "tgorka", AT, reviewed, &out)
                .unwrap_or_else(|error| panic!("{target} {reviewed}: {error:?}"));
            let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
            assert_eq!(vm.knowledge[0].reviewed_by_me, reviewed, "{note:?}");
            assert_eq!(vm.knowledge[0].foreign_copy, None, "{note:?}");
            let copy = std::fs::read_to_string(drive.path().join(target)).expect("copy");
            assert_eq!(body(&copy), body(&note), "{note:?} → {copy:?}");
        }
        let changed = note.replace("Three", "Four");
        put(&zone, NOTE_REL, changed.as_bytes(), AN_HOUR);
        promote_note(&zone, target, &changed, &out)
            .unwrap_or_else(|error| panic!("{note:?}: {error:?}"));
        assert_eq!(
            std::fs::read_to_string(drive.path().join(target)).ok(),
            Some(knowledge::review(&changed, "tgorka", AT, true)),
            "{note:?}"
        );
    }
}

/// R253 (R95K5-07): a note's row is its first; a second row naming it
/// lends that row nothing. A first row to `a.md` records no publication; a
/// second, to `b.md`, records the copy the note published there. `a.md`
/// says what `b.md` says, with marta's review: the panel names `a.md`,
/// shows no review and says why, and marta's tick is refused for the same
/// reason — both files kept.
#[test]
fn a_second_row_lends_the_first_nothing() {
    const A: &str = "10-notes/knowledge/a.md";
    const B: &str = "10-notes/knowledge/b.md";
    let (drive, zone, profile, pinned) = harvest_drive();
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    promote_note(&zone, B, NOTE, &out).expect("published to b.md");
    let by_marta = knowledge::review(NOTE, "marta", AT, true);
    std::fs::write(drive.path().join(A), &by_marta).expect("a.md");
    let path = zone.join(SESSION).join("README.md");
    let first = format!("| {NOTE_REL} | {A} | knowledge |\n");
    std::fs::write(
        &path,
        readme(&zone).replace(
            &format!("| {NOTE_REL} | {B}"),
            &format!("{first}| {NOTE_REL} | {B}"),
        ),
    )
    .expect("two rows");
    let b = std::fs::read_to_string(drive.path().join(B)).expect("b.md");

    let why = promote::CopyLoss::Unrecorded.explain(NOTE_REL, A);
    let vm = panel(&zone, SESSION, &out, Some("human:marta")).expect("panel");
    assert_eq!(vm.knowledge[0].promoted_to.as_deref(), Some(A));
    assert_eq!(vm.knowledge[0].reviewed_by, None);
    assert!(!vm.knowledge[0].reviewed_by_me);
    assert_eq!(vm.knowledge[0].foreign_copy.as_ref(), Some(&why));
    assert_eq!(
        refusal(review(&zone, SESSION, NOTE_REL, "marta", AT, true, &out)),
        Some(why)
    );
    assert_eq!(
        std::fs::read_to_string(drive.path().join(A)).ok(),
        Some(by_marta)
    );
    assert_eq!(std::fs::read_to_string(drive.path().join(B)).ok(), Some(b));
}

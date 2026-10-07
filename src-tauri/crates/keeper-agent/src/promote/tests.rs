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
use keeper_core::sessions::offer::{ChoiceVm, NoteIntentVm, PanelIntentVm};
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
        reply: None,
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
    assert_eq!(
        vm.unlisted
            .iter()
            .map(|file| file.source.as_str())
            .collect::<Vec<_>>(),
        ["workspace/other.csv"]
    );

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

/// `person`'s tick (`reviewed`) or untick of [`NOTE_REL`]'s vault copy as
/// read just before, the way the panel reads it first.
fn tick(zone: &Path, person: &str, reviewed: bool, out: &OutOf) -> Result<(), VerbError> {
    let read = offer::read_note(zone, SESSION, NOTE_REL, true, out)?;
    review(
        zone,
        SESSION,
        NOTE_REL,
        person,
        AT,
        reviewed,
        &read.revision,
        out,
    )
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

    tick(&zone, "tgorka", false, &out).expect("unticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET))
            .ok()
            .as_deref(),
        Some(NOTE)
    );
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert!(!vm.knowledge[0].reviewed_by_me);
    tick(&zone, "tgorka", true, &out).expect("ticked");
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
        "",
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

/// R234 (R95P2-01) with R95K-08 and R244: a review lands only on the vault
/// copy as the person read it, checked inside the guarded amend in the held
/// zone, and only on the copy the note's row records it published. Another
/// person's review that lands between the read and the write refuses with
/// `COPY_CHANGED`, theirs kept, never written over; read again, the tick
/// lands beside it. An editor's save landing in that window is kept and the
/// review refused as no longer the note's copy (DW-960). A promotion out
/// left pending is published first and refuses a review of the copy before
/// it; a row retargeted to another file holding the same bytes refuses a
/// review of the copy that was read. Read again, it lands.
#[test]
fn a_review_lands_only_on_the_copy_as_read() {
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
    let copy = || std::fs::read_to_string(drive.path().join(TARGET)).expect("copy");
    let changed = |result: Result<(), VerbError>| {
        assert!(
            matches!(&result, Err(VerbError::Refused(sentence)) if sentence == COPY_CHANGED),
            "{result:?}"
        );
    };

    tick(&zone, "tgorka", false, &out).expect("unticked");
    let read = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect("read");
    let by_marta = knowledge::review(NOTE, "marta", AT, true);
    *vault.meanwhile.lock().expect("lock") = Some(by_marta.clone());
    changed(review(
        &zone,
        SESSION,
        NOTE_REL,
        "tgorka",
        AT,
        true,
        &read.revision,
        &out,
    ));
    assert_eq!(copy(), by_marta, "their review kept, never written over");
    tick(&zone, "tgorka", true, &out).expect("read again, it lands beside theirs");
    assert_eq!(copy(), knowledge::review(&by_marta, "tgorka", AT, true));

    let read = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect("read");
    let before = copy();
    let edited = before.replace("Three papers.", "Three papers, and the ID.");
    *vault.meanwhile.lock().expect("lock") = Some(edited.clone());
    let refused = review(
        &zone,
        SESSION,
        NOTE_REL,
        "tgorka",
        AT,
        false,
        &read.revision,
        &out,
    );
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence))
            if *sentence == promote::CopyLoss::Changed.explain(NOTE_REL, TARGET)),
        "{refused:?}"
    );
    assert_eq!(copy(), edited, "the edit kept, no review composed on it");
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(
        vm.knowledge[0].foreign_copy,
        Some(promote::CopyLoss::Changed.explain(NOTE_REL, TARGET)),
        "the panel says the edited file is no longer the note's copy"
    );
    assert_eq!(vm.knowledge[0].destination, None);
    std::fs::write(drive.path().join(TARGET), &before).expect("the edit undone");

    let read = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect("read");
    let newer = knowledge::review(
        &NOTE.replace("Three papers.", "Four papers."),
        "tgorka",
        AT,
        true,
    );
    keep_pending(&zone, &pending(&newer)).expect("pending");
    changed(review(
        &zone,
        SESSION,
        NOTE_REL,
        "marta",
        AT,
        true,
        &read.revision,
        &out,
    ));
    assert_eq!(copy(), newer, "published as admitted, not reviewed unread");

    let read = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect("read");
    let moved = "10-notes/knowledge/moved.md";
    std::fs::write(drive.path().join(moved), copy()).expect("the same bytes elsewhere");
    std::fs::write(
        zone.join(SESSION).join("README.md"),
        readme(&zone).replace(TARGET, moved),
    )
    .expect("retargeted");
    changed(review(
        &zone,
        SESSION,
        NOTE_REL,
        "marta",
        AT,
        true,
        &read.revision,
        &out,
    ));
    assert_eq!(
        std::fs::read_to_string(drive.path().join(moved)).ok(),
        Some(newer.clone())
    );

    tick(&zone, "marta", true, &out).expect("read again, it lands");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(moved)).ok(),
        Some(knowledge::review(&newer, "marta", AT, true))
    );
}

/// R234 (R95P2-02): a candidate at a knowledge note's 64 KiB cap is read,
/// promoted with the person's review — its vault copy then larger than
/// the cap — and that copy is read whole, unticked and ticked again; a
/// copy at the reviewed bound is still read, one byte past it is not.
#[test]
fn a_note_at_the_cap_is_read_and_reviewed_after_promotion() {
    let (drive, zone) = drive();
    let tail = "The last line.\n";
    let fill = knowledge::MAX_NOTE_BYTES - NOTE.len() - tail.len();
    let candidate = format!("{NOTE}{}{tail}", "x".repeat(fill));
    assert_eq!(candidate.len(), knowledge::MAX_NOTE_BYTES);
    put(&zone, NOTE_REL, candidate.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read = offer::read_note(&zone, SESSION, NOTE_REL, false, &out).expect("the candidate");
    assert_eq!(read.text, candidate);
    offer::promote_to(
        &zone,
        SESSION,
        NOTE_REL,
        "10-notes/knowledge",
        "short.md",
        Some(&read.revision),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    let reviewed = knowledge::review(&candidate, "tgorka", AT, true);
    assert!(reviewed.len() > knowledge::MAX_NOTE_BYTES);

    let copy = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect("the copy whole");
    assert_eq!(copy.text, reviewed);
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(
        vm.knowledge[0]
            .copy
            .as_ref()
            .and_then(|copy| copy.revision.clone()),
        Some(copy.revision.clone())
    );
    tick(&zone, "tgorka", false, &out).expect("unticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(candidate.clone())
    );
    tick(&zone, "tgorka", true, &out).expect("ticked again");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(reviewed.clone())
    );

    let at_bound = format!(
        "{reviewed}{}",
        "y".repeat(knowledge::MAX_REVIEWED_BYTES - reviewed.len())
    );
    std::fs::write(drive.path().join(TARGET), &at_bound).expect("at the bound");
    assert!(offer::read_note(&zone, SESSION, NOTE_REL, true, &out).is_ok());
    std::fs::write(drive.path().join(TARGET), format!("{at_bound}y")).expect("past it");
    assert!(offer::read_note(&zone, SESSION, NOTE_REL, true, &out).is_err());
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
    tick(&zone, "tgorka", false, &out).expect("unticked");
    tick(&zone, "tgorka", true, &out).expect("ticked");
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
        tick(&zone, "tgorka", n % 2 == 1, &out).expect("review");
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
    tick(&zone, "tgorka", false, &out).expect("unticked");
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
            "archive" => {
                crate::sessions::verbs::archive(&zone, "01J5AAAAAAAAAAAAAAAAAAAAAA", false, 2026)
            }
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
        let review = review(&zone, SECOND, NOTE_REL, "tgorka", AT, true, "", &out);
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
    // R298: what the panel offers agrees with what promoting would do —
    // nothing is offered at a file that is not the note's copy, and the
    // note says why.
    assert!(vm.knowledge[0].foreign_copy.is_some());
    assert_eq!(vm.knowledge[0].destination, None);
    assert_eq!(vm.knowledge[0].unavailable, vm.knowledge[0].foreign_copy);
    for reviewed in [true, false] {
        let refused = tick(&zone, "tgorka", reviewed, &out);
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
    tick(&zone, "tgorka", false, &out).expect("unticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET))
            .ok()
            .as_deref(),
        Some(NOTE)
    );
    tick(&zone, "tgorka", true, &out).expect("ticked");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(knowledge::review(NOTE, "tgorka", AT, true))
    );
}

/// R298: the panel reads a note's vault copy from its one row
/// ([`promote::entry_of`]), as review and promotion do: a first row naming
/// the note into the session is its row, so a later row out lends the panel
/// no copy to read or review.
#[test]
fn the_panel_takes_a_notes_copy_from_its_one_row() {
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
    assert!(panel(&zone, SESSION, &out, Some(ME))
        .expect("panel")
        .knowledge[0]
        .copy
        .is_some());
    let first = format!("| {NOTE_REL} | artifacts/kept.md | kept |\n");
    let readme = readme(&zone);
    let at = readme.find(&format!("| {NOTE_REL} |")).expect("its row");
    std::fs::write(
        zone.join(SESSION).join("README.md"),
        format!("{}{first}{}", &readme[..at], &readme[at..]),
    )
    .expect("a first row into the session");
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(vm.knowledge[0].copy, None);
    let read = offer::read_note(&zone, SESSION, NOTE_REL, true, &out);
    assert!(
        matches!(&read, Err(VerbError::Refused(sentence)) if sentence == NOT_PROMOTED),
        "{read:?}"
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
        tick(&zone, "tgorka", true, &out),
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
        tick(&zone, "tgorka", false, &out),
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
    let reviewed = tick(&zone, "tgorka", false, &out);
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
            tick(&zone, "tgorka", reviewed, &out)
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
    assert_eq!(refusal(tick(&zone, "marta", true, &out)), Some(why));
    assert_eq!(
        std::fs::read_to_string(drive.path().join(A)).ok(),
        Some(by_marta)
    );
    assert_eq!(std::fs::read_to_string(drive.path().join(B)).ok(), Some(b));
}

// ---- What the panel offers and how an archive promotes (R216, R95P) ----

/// Where the archived fixture session lands.
const ARCHIVED: &str = "archive/2026/2026-10-06-harvest";

/// Every item of the checklist `vm` decided: each of `promotes`' sources
/// promoted to its target, every other row and file skipped.
fn decide_all(vm: &SessionPromoteVm, promotes: &[(&str, &str)]) -> Vec<ChoiceVm> {
    vm.rows
        .iter()
        .map(|row| (&row.revision, &row.source))
        .chain(
            vm.unlisted
                .iter()
                .map(|file| (&file.revision, &file.source)),
        )
        .map(|(revision, source)| {
            let to = promotes.iter().find(|(from, _)| from == source);
            ChoiceVm {
                revision: revision.clone(),
                promote: to.is_some(),
                target: to.map(|(_, to)| (*to).to_owned()).unwrap_or_default(),
            }
        })
        .collect()
}

fn archive_with(zone: &Path, choices: &[ChoiceVm], revision: &str) -> Result<(), VerbError> {
    offer::archive(
        zone,
        "01J5AAAAAAAAAAAAAAAAAAAAAA",
        &offer::Archive {
            choices,
            revision,
            empty_workspace: true,
            year: 2026,
            root: zone.parent().expect("the drive"),
        },
        SETTLE_MS,
        now_ms(),
    )
}

/// R95P-01: an archive's promotion is the panel's promotion — admitted as
/// a settled `workspace/` file into `artifacts/`, its row recorded where
/// the table reads it back (a row already there retargeted, never a second
/// one), its verified bytes copied — in the archive's one plan before the
/// workspace is emptied and the folder moved.
#[test]
fn an_archive_promotes_as_the_panel_does() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    let draft: &[u8] = b"# Report\n\x00\xffbinary tail\n";
    put(&zone, "workspace/draft.md", draft, AN_HOUR);
    put(&zone, "workspace/skipped.md", b"scratch\n", AN_HOUR);
    promote_in(
        &zone,
        SESSION,
        "workspace/draft.md",
        "artifacts/old.md",
        "weekly",
        SETTLE_MS,
        now_ms(),
    )
    .expect("promoted");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");

    archive_with(
        &zone,
        &decide_all(&vm, &[("workspace/draft.md", "artifacts/report.md")]),
        &vm.revision,
    )
    .expect("archived");
    let archived = zone.join(ARCHIVED);
    assert!(!zone.join(SESSION).exists());
    assert_eq!(
        std::fs::read(archived.join("artifacts/report.md"))
            .ok()
            .as_deref(),
        Some(draft)
    );
    let readme = std::fs::read_to_string(archived.join("README.md")).expect("readme");
    assert_eq!(
        promote::parse(&readme).expect("table").rows,
        [promote::PromoteRow::Entry {
            source: "workspace/draft.md".to_owned(),
            target: "artifacts/report.md".to_owned(),
            note: "weekly".to_owned(),
            published: None,
        }]
    );
    assert!(!archived.join("workspace/draft.md").exists());
    assert!(!archived.join("workspace/skipped.md").exists());
}

/// R95P-01: what a promotion refuses an archive refuses, with nothing
/// changed — a target in `workspace/` (copied, then emptied), the README,
/// outside the session, and a source still being written.
#[test]
fn an_archive_refuses_what_a_promotion_refuses() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/draft.md", b"draft\n", AN_HOUR);
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    for target in [
        "workspace/keep.md",
        "README.md",
        "../elsewhere.md",
        "artifacts",
    ] {
        let refused = archive_with(
            &zone,
            &decide_all(&vm, &[("workspace/draft.md", target)]),
            &vm.revision,
        );
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{target}: {refused:?}"
        );
        assert_eq!(readme(&zone), README_TEXT, "{target}");
        assert_eq!(
            read(&zone, "workspace/draft.md").as_deref(),
            Some(&b"draft\n"[..])
        );
    }

    put(
        &zone,
        "workspace/draft.md",
        b"draft, mid-write\n",
        Duration::ZERO,
    );
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    let still = archive_with(
        &zone,
        &decide_all(&vm, &[("workspace/draft.md", "artifacts/draft.md")]),
        &vm.revision,
    );
    assert!(
        matches!(&still, Err(VerbError::Refused(sentence)) if sentence.ends_with(&format!("{STILL_WRITING}."))),
        "{still:?}"
    );
    assert!(zone.join(SESSION).join("workspace/draft.md").exists());
    assert!(!zone.join(SESSION).join("artifacts/draft.md").exists());
}

/// R95P-05/06: the archive is bound to the checklist the person read: a
/// workspace file that arrived or was written since, or a row changed, is
/// refused with nothing archived; an unchanged reread keeps the revisions,
/// and only the row that changed gets a new one.
#[test]
fn an_archive_refuses_a_checklist_that_changed() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/a.md", b"a\n", AN_HOUR);
    put(&zone, "workspace/b.md", b"b\n", AN_HOUR);
    let read_at = panel(&zone, SESSION, &out, None).expect("panel");
    let again = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(again.revision, read_at.revision);
    assert_eq!(again.unlisted, read_at.unlisted);

    put(&zone, "workspace/arrived.md", b"new\n", AN_HOUR);
    let refused = archive_with(&zone, &[], &read_at.revision);
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence == keeper_core::sessions::offer::SNAPSHOT_CHANGED),
        "{refused:?}"
    );
    assert!(zone.join(SESSION).join("workspace/a.md").exists());

    let before = panel(&zone, SESSION, &out, None).expect("panel");
    put(&zone, "workspace/b.md", b"b, longer\n", AN_HOUR);
    let after = panel(&zone, SESSION, &out, None).expect("panel");
    let revision = |vm: &SessionPromoteVm, source: &str| {
        vm.unlisted
            .iter()
            .find(|file| file.source == source)
            .map(|file| file.revision.clone())
    };
    assert_eq!(
        revision(&after, "workspace/a.md"),
        revision(&before, "workspace/a.md")
    );
    assert_ne!(
        revision(&after, "workspace/b.md"),
        revision(&before, "workspace/b.md")
    );
    assert!(matches!(
        archive_with(&zone, &[], &before.revision),
        Err(VerbError::Refused(_))
    ));
    archive_with(&zone, &decide_all(&after, &[]), &after.revision).expect("archived as read");
    assert!(zone.join(ARCHIVED).is_dir());
}

/// R95P-02/03: the candidate and the vault copy are read separately, each
/// with the revision of what was read; a review names the copy's version
/// and is refused for any other; a missing copy offers its row's target
/// fixed, and promoting there restores it reviewed without a review of an
/// absent file.
#[test]
fn a_note_is_read_reviewed_and_restored_as_the_version_read() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    let offered = vm.knowledge[0].destination.clone().expect("a destination");
    assert_eq!(
        (
            offered.folder.as_str(),
            offered.name.as_str(),
            offered.fixed
        ),
        ("10-notes", "short.md", false)
    );
    offer::promote_to(
        &zone,
        SESSION,
        NOTE_REL,
        "10-notes/knowledge",
        "short.md",
        Some(&sha256_hex(NOTE)),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");

    // Another person's review in notes: the copy differs from the candidate
    // and is still the one the note published (R252 refuses an edit).
    let edited = knowledge::review(
        &knowledge::review(NOTE, "tgorka", AT, true),
        "marta",
        AT,
        true,
    );
    std::fs::write(drive.path().join(TARGET), &edited).expect("marta reviews the copy");
    let candidate = offer::read_note(&zone, SESSION, NOTE_REL, false, &out).expect("candidate");
    let copy = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect("copy");
    assert_eq!(
        (candidate.text.as_str(), candidate.revision.as_str()),
        (NOTE, sha256_hex(NOTE).as_str())
    );
    assert_eq!(copy.text, edited);
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert_eq!(
        vm.knowledge[0]
            .copy
            .as_ref()
            .and_then(|copy| copy.revision.clone()),
        Some(copy.revision.clone())
    );
    let wrong = review(
        &zone,
        SESSION,
        NOTE_REL,
        "tgorka",
        AT,
        false,
        &candidate.revision,
        &out,
    );
    assert!(
        matches!(&wrong, Err(VerbError::Refused(sentence)) if sentence == COPY_CHANGED),
        "{wrong:?}"
    );
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(edited.clone())
    );
    review(
        &zone,
        SESSION,
        NOTE_REL,
        "tgorka",
        AT,
        false,
        &copy.revision,
        &out,
    )
    .expect("unticked as read");
    assert!(
        !panel(&zone, SESSION, &out, Some(ME))
            .expect("panel")
            .knowledge[0]
            .reviewed_by_me
    );

    std::fs::remove_file(drive.path().join(TARGET)).expect("lose the copy");
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    let note = &vm.knowledge[0];
    assert_eq!(note.state, Some(PromoteState::MissingTarget));
    assert!(!note.copy.as_ref().expect("its copy").there);
    let repair = note.destination.clone().expect("a repair");
    assert_eq!(
        (repair.folder.as_str(), repair.name.as_str(), repair.fixed),
        ("10-notes/knowledge", "short.md", true)
    );
    offer::promote_to(
        &zone,
        SESSION,
        NOTE_REL,
        &repair.folder,
        &repair.name,
        Some(&candidate.revision),
        Some(&tgorka()),
        &out,
    )
    .expect("restored");
    assert_eq!(
        std::fs::read_to_string(drive.path().join(TARGET)).ok(),
        Some(knowledge::review(NOTE, "tgorka", AT, true))
    );
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert!(vm.knowledge[0].reviewed_by_me);
    assert_eq!(vm.knowledge[0].destination, None);
}

/// R95P-07/08: the panel offers out only what the promotion takes — a
/// text artifact under its own name in the vault, not a binary one, nothing
/// without a table to record it — and the target is composed in Rust from
/// the folder and filename, inside the vault only.
#[test]
fn the_panel_offers_only_what_the_promotion_takes() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    put(
        &zone,
        "artifacts/report.md",
        "# Report — é\n".as_bytes(),
        AN_HOUR,
    );
    put(&zone, "artifacts/data.bin", b"\x00\xff\xfe binary", AN_HOUR);
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    let offers: Vec<_> = vm
        .artifacts
        .iter()
        .map(|offer| {
            (
                offer.path.as_str(),
                offer
                    .destination
                    .as_ref()
                    .map(|to| (to.folder.as_str(), to.name.as_str())),
                offer.unavailable.is_some(),
            )
        })
        .collect();
    assert_eq!(
        offers,
        [
            ("artifacts/data.bin", None, true),
            (
                "artifacts/report.md",
                Some(("10-notes", "report.md")),
                false
            ),
        ]
    );
    for (folder, name) in [
        ("30-work", "report.md"),
        ("10-notes", "../report.md"),
        ("10-notes/../30-work", "report.md"),
    ] {
        let refused = offer::promote_to(
            &zone,
            SESSION,
            "artifacts/report.md",
            folder,
            name,
            None,
            None,
            &out,
        );
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{folder} + {name}: {refused:?}"
        );
    }
    assert!(!drive.path().join("30-work").exists());
    offer::promote_to(
        &zone,
        SESSION,
        "artifacts/report.md",
        "10-notes",
        "report.md",
        None,
        None,
        &out,
    )
    .expect("promoted out");
    assert!(drive.path().join("10-notes/report.md").is_file());

    std::fs::write(
        zone.join(SESSION).join("README.md"),
        "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\n---\n# Harvest\n",
    )
    .expect("no table");
    put(&zone, "workspace/draft.md", b"d\n", AN_HOUR);
    let vm = panel(&zone, SESSION, &out, Some(ME)).expect("panel");
    assert!(vm.unlisted.iter().all(|file| file.refused.is_some()));
    assert!(vm.artifacts.iter().all(|offer| offer.destination.is_none()));
    assert!(vm.knowledge[0].destination.is_none() && vm.knowledge[0].unavailable.is_some());
}

/// R234 (R95P2-03): the archive checklist holds everything the emptying
/// removes. From an empty checklist, a hidden file and a file in a hidden
/// folder arriving refuse the archive decided before them, nothing
/// removed; read again, each is an item that needs a choice, and an
/// archive without one is refused; a link is an item that may only be
/// skipped. Decided, the archive removes them.
#[test]
fn the_archive_checklist_holds_everything_the_emptying_removes() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    let empty = panel(&zone, SESSION, &out, None).expect("panel");
    assert!(empty.unlisted.is_empty() && empty.complete);

    put(&zone, "workspace/.draft.md", b"hidden work\n", AN_HOUR);
    put(&zone, "workspace/.staging/output.md", b"staged\n", AN_HOUR);
    let refused = archive_with(&zone, &[], &empty.revision);
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence == keeper_core::sessions::offer::SNAPSHOT_CHANGED),
        "{refused:?}"
    );
    assert!(zone.join(SESSION).join("workspace/.draft.md").is_file());

    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/hostname", zone.join(SESSION).join("workspace/link"))
        .expect("link");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    let unlisted: Vec<_> = vm
        .unlisted
        .iter()
        .map(|file| (file.source.as_str(), file.refused.is_some()))
        .collect();
    let mut expected = vec![
        ("workspace/.draft.md", false),
        ("workspace/.staging/output.md", false),
    ];
    if cfg!(unix) {
        expected.push(("workspace/link", true));
    }
    assert_eq!(unlisted, expected);
    let refused = archive_with(&zone, &[], &vm.revision);
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence.contains("has no choice")),
        "{refused:?}"
    );
    assert!(zone
        .join(SESSION)
        .join("workspace/.staging/output.md")
        .is_file());
    archive_with(&zone, &decide_all(&vm, &[]), &vm.revision).expect("archived as decided");
    assert!(!zone.join(ARCHIVED).join("workspace/.draft.md").exists());
    assert!(!zone.join(ARCHIVED).join("workspace/.staging").exists());
}

/// R234 (R95P2-05): what a row says of its target is part of its choice.
/// A target deleted after a Skip takes that row's choice with it — the
/// other choices stay and the checklist is no longer complete — and a
/// target replaced between the checklist and the archive refuses the
/// archive, the source kept.
#[test]
fn losing_a_target_takes_the_choice_with_it() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/report.md", b"report\n", AN_HOUR);
    put(&zone, "workspace/other.md", b"other\n", AN_HOUR);
    promote_in(
        &zone,
        SESSION,
        "workspace/report.md",
        "artifacts/report.md",
        "",
        SETTLE_MS,
        now_ms(),
    )
    .expect("promoted");
    let shown = panel(&zone, SESSION, &out, None).expect("panel");
    let skipped = PanelIntentVm {
        choices: decide_all(&shown, &[]),
        notes: Vec::new(),
    };
    let decided = offer::panel_for(&zone, SESSION, &out, None, &skipped).expect("panel");
    assert!(decided.complete);

    std::fs::remove_file(zone.join(SESSION).join("artifacts/report.md")).expect("lost");
    let after = offer::panel_for(&zone, SESSION, &out, None, &decided.intent).expect("panel");
    assert_eq!(after.rows[0].state, PromoteState::MissingTarget);
    assert_eq!(after.rows[0].choice, None, "the Skip went with the target");
    assert!(after.unlisted[0].choice.is_some(), "the other choice stays");
    assert!(!after.complete);
    let refused = archive_with(&zone, &decided.intent.choices, &after.revision);
    assert!(matches!(refused, Err(VerbError::Refused(_))), "{refused:?}");

    put(&zone, "artifacts/report.md", b"report\n", AN_HOUR);
    let shown = panel(&zone, SESSION, &out, None).expect("panel");
    let choices = decide_all(&shown, &[]);
    put(&zone, "artifacts/report.md", b"someone else's\n", AN_HOUR);
    let refused = archive_with(&zone, &choices, &shown.revision);
    assert!(
        matches!(&refused, Err(VerbError::Refused(sentence)) if sentence == keeper_core::sessions::offer::SNAPSHOT_CHANGED),
        "{refused:?}"
    );
    assert!(zone.join(SESSION).join("workspace/report.md").is_file());
}

/// R234 (R95P2-06): the archive is told an explicitly decided checklist
/// from one with no decisions: no choices, a choice missing, a choice
/// about a row the checklist does not hold or two about one row are each
/// refused, nothing archived; a promotion of a row that offers none is
/// refused with that row's reason; every row explicitly skipped archives.
#[test]
fn an_archive_needs_one_choice_for_every_row() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/a.md", b"a\n", AN_HOUR);
    put(&zone, "workspace/b.md", b"b\n", AN_HOUR);
    std::fs::write(
        zone.join(SESSION).join("README.md"),
        README_TEXT.replace(
            "| --------- | ----------- | ---- |\n",
            "| --------- | ----------- | ---- |\n| workspace/gone.md | artifacts/gone.md | |\n",
        ),
    )
    .expect("a row whose source is gone");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    let all = decide_all(&vm, &[]);
    let stranger = ChoiceVm {
        revision: "not a row".to_owned(),
        promote: false,
        target: String::new(),
    };
    let mut gone = all.clone();
    gone[0].promote = true;
    gone[0].target = "artifacts/gone.md".to_owned();
    for (name, choices) in [
        ("none", Vec::new()),
        ("one missing", all[1..].to_vec()),
        ("a stranger", [all.clone(), vec![stranger]].concat()),
        ("twice", [all.clone(), all[..1].to_vec()].concat()),
        ("a refused promotion", gone),
    ] {
        let refused = archive_with(&zone, &choices, &vm.revision);
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{name}: {refused:?}"
        );
        assert!(
            zone.join(SESSION).join("workspace/a.md").is_file(),
            "{name}"
        );
    }
    archive_with(&zone, &all, &vm.revision).expect("every row skipped");
    assert!(zone.join(ARCHIVED).is_dir());
}

/// R234 (R95P2-06): the panel decides the person's reads and consent:
/// a candidate read and consented to is current and consented; a newer
/// candidate makes the read stale and drops the consent from the intent
/// the panel keeps.
#[test]
fn the_panel_decides_reads_and_consent() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read = offer::read_note(&zone, SESSION, NOTE_REL, false, &out).expect("read");
    let intent = PanelIntentVm {
        choices: Vec::new(),
        notes: vec![NoteIntentVm {
            path: NOTE_REL.to_owned(),
            read: Some(read.revision.clone()),
            copy: None,
            consent: Some(read.revision.clone()),
        }],
    };
    let vm = offer::panel_for(&zone, SESSION, &out, Some(ME), &intent).expect("panel");
    assert!(vm.knowledge[0].consented);
    assert_eq!(
        vm.knowledge[0].candidate_read,
        keeper_core::sessions::offer::ReadState::Current
    );
    put(
        &zone,
        NOTE_REL,
        NOTE.replace("Three", "Four").as_bytes(),
        AN_HOUR,
    );
    let vm = offer::panel_for(&zone, SESSION, &out, Some(ME), &vm.intent).expect("panel");
    assert!(!vm.knowledge[0].consented);
    assert_eq!(
        vm.knowledge[0].candidate_read,
        keeper_core::sessions::offer::ReadState::Stale
    );
    assert_eq!(vm.intent.notes[0].consent, None);
}

/// R234 (R95P2-07): the panel's text test agrees with every verdict of
/// `promote-vectors.json`, which the mock shell's test loads as well:
/// invalid UTF-8 is not text, valid UTF-8 holding a NUL is.
#[test]
fn every_text_vector_matches() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../keeper-core/src/sessions/promote-vectors.json"
    ))
    .expect("vectors");
    let dir = tempfile::tempdir().expect("dir");
    for case in vectors["isText"].as_array().expect("cases") {
        let path = dir.path().join("file");
        let hex = case["hex"].as_str().expect("hex");
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect();
        std::fs::write(&path, bytes).expect("write");
        assert_eq!(offer::is_text(&path).ok(), case["text"].as_bool(), "{case}");
    }
}

/// The archive of the checklist `revision` with `choices`, run up to its
/// emptying's last check and stopped there as a crash stops it: its journal
/// kept, every step before the emptying done.
fn crash_at_the_emptying(zone: &Path, choices: &[ChoiceVm], revision: &str) {
    crate::sessions::exec::seam::after_check(|| panic!("the process is gone"));
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        archive_with(zone, choices, revision)
    }));
    assert!(crashed.is_err(), "the archive stopped at its emptying");
    assert!(zone.join(".keeper/sessions-journal.json").is_file());
}

/// R249 (R95P3-03): what a target said when a person's choice let the
/// emptying remove its source travels with the archive's journal. Every
/// row skipped, or a row promoted by the plan's own checked copy, the
/// archive crashes before its emptying; a target deleted or replaced
/// before the resume refuses it — the source kept, the session not moved,
/// the old choice no longer making the checklist complete. Nothing
/// changed, the plan's own copy is no drift: the resume archives.
#[test]
fn a_resumed_archive_empties_only_while_its_targets_say_what_they_said() {
    for (name, promote, change) in [
        ("skipped, the target deleted", false, Some(None)),
        (
            "skipped, the target replaced",
            false,
            Some(Some(&b"another\n"[..])),
        ),
        ("promoted, the copy deleted", true, Some(None)),
        (
            "promoted, the copy replaced",
            true,
            Some(Some(&b"another\n"[..])),
        ),
        ("promoted, nothing changed", true, None),
    ] {
        let (drive, zone) = drive();
        let profile = profile(drive.path());
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, None);
        put(&zone, "workspace/report.md", b"report\n", AN_HOUR);
        promote_in(
            &zone,
            SESSION,
            "workspace/report.md",
            "artifacts/report.md",
            "",
            SETTLE_MS,
            now_ms(),
        )
        .expect("promoted");
        if promote {
            put(&zone, "workspace/report.md", b"report, v2\n", AN_HOUR);
        }
        let shown = panel(&zone, SESSION, &out, None).expect("panel");
        let promotes: &[(&str, &str)] = if promote {
            &[("workspace/report.md", "artifacts/report.md")]
        } else {
            &[]
        };
        let intent = PanelIntentVm {
            choices: decide_all(&shown, promotes),
            notes: Vec::new(),
        };
        crash_at_the_emptying(&zone, &intent.choices, &shown.revision);
        let target = zone.join(SESSION).join("artifacts/report.md");
        match change {
            Some(None) => std::fs::remove_file(&target).expect("deleted"),
            Some(Some(bytes)) => std::fs::write(&target, bytes).expect("replaced"),
            None => {}
        }
        let resumed = crate::sessions::exec::resume(&zone);
        if change.is_none() {
            resumed.expect("resumed");
            assert_eq!(
                std::fs::read(zone.join(ARCHIVED).join("artifacts/report.md")).ok(),
                Some(b"report, v2\n".to_vec()),
                "{name}"
            );
            continue;
        }
        assert!(
            matches!(resumed, Err(crate::sessions::exec::ExecError::Refused(_))),
            "{name}: {resumed:?}"
        );
        assert!(
            read(&zone, "workspace/report.md").is_some(),
            "{name}: the source kept"
        );
        assert!(!zone.join(ARCHIVED).exists(), "{name}");
        let again = offer::panel_for(&zone, SESSION, &out, None, &intent).expect("panel");
        assert!(!again.complete, "{name}: decided again");
    }
}

/// R249 (R95P3-04): a session whose `workspace/` is a link — to another
/// session's workspace in the zone, or to a folder outside it — is not
/// listed through it: the panel says so, an archive that empties is
/// refused, and the folder it leads to keeps every byte.
#[cfg(unix)]
#[test]
fn a_linked_workspace_is_never_listed_or_emptied() {
    let outside = tempfile::tempdir().expect("outside");
    for name in ["in the zone", "out of it"] {
        let (drive, zone) = drive();
        let profile = profile(drive.path());
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, None);
        let other = if name == "in the zone" {
            zone.join("active/2026-10-01-other/workspace")
        } else {
            outside.path().join("workspace")
        };
        std::fs::create_dir_all(&other).expect("other");
        std::fs::write(other.join("theirs.md"), "their work\n").expect("theirs");
        let workspace = zone.join(SESSION).join("workspace");
        std::fs::remove_dir(&workspace).expect("empty workspace");
        std::os::unix::fs::symlink(&other, &workspace).expect("link");

        let vm = panel(&zone, SESSION, &out, None).expect("panel");
        assert!(vm.unlisted.is_empty(), "{name}: {:?}", vm.unlisted);
        assert!(
            vm.problems.iter().any(|problem| problem.contains("link")),
            "{name}: {:?}",
            vm.problems
        );
        let refused = archive_with(&zone, &decide_all(&vm, &[]), &vm.revision);
        assert!(
            matches!(refused, Err(VerbError::Refused(_))),
            "{name}: {refused:?}"
        );
        assert_eq!(
            std::fs::read_to_string(other.join("theirs.md"))
                .ok()
                .as_deref(),
            Some("their work\n"),
            "{name}"
        );
        assert!(zone.join(SESSION).exists(), "{name}");
        std::fs::remove_dir_all(&other).expect("cleaned");
    }
}

/// R249 (R95P3-05): a line of the `## Promote` table written twice is two
/// items of the checklist, each with its own revision and choice: one
/// skipped leaves the checklist incomplete; the repeat offers no promotion
/// of its own; both decided, the intent the panel returns as complete is
/// one the archive takes — the first promoted, the repeat skipped.
#[test]
fn a_row_written_twice_is_two_choices_the_archive_takes() {
    let (drive, zone) = drive();
    let profile = profile(drive.path());
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, None);
    put(&zone, "workspace/a.md", b"a\n", AN_HOUR);
    let row = "| workspace/a.md | artifacts/a.md | |\n";
    std::fs::write(
        zone.join(SESSION).join("README.md"),
        README_TEXT.replace("| ---- |\n", &format!("| ---- |\n{row}{row}")),
    )
    .expect("a row written twice");
    let vm = panel(&zone, SESSION, &out, None).expect("panel");
    assert_eq!(vm.rows.len(), 2);
    assert_ne!(vm.rows[0].revision, vm.rows[1].revision);
    assert_eq!(vm.rows[0].refused, None);
    assert!(vm.rows[1].refused.is_some(), "the repeat promotes nothing");

    let skip = |at: usize| ChoiceVm {
        revision: vm.rows[at].revision.clone(),
        promote: false,
        target: String::new(),
    };
    let one = offer::panel_for(
        &zone,
        SESSION,
        &out,
        None,
        &PanelIntentVm {
            choices: vec![skip(0)],
            notes: Vec::new(),
        },
    )
    .expect("panel");
    assert!(!one.complete, "the repeat still needs its choice");
    assert_eq!(
        one.rows
            .iter()
            .map(|row| row.choice.is_some())
            .collect::<Vec<_>>(),
        [true, false]
    );

    let promote_first = ChoiceVm {
        promote: true,
        target: "artifacts/a.md".to_owned(),
        ..skip(0)
    };
    let both = offer::panel_for(
        &zone,
        SESSION,
        &out,
        None,
        &PanelIntentVm {
            choices: vec![promote_first, skip(1)],
            notes: Vec::new(),
        },
    )
    .expect("panel");
    assert!(both.complete);
    archive_with(&zone, &both.intent.choices, &both.revision).expect("archived as decided");
    assert_eq!(
        std::fs::read(zone.join(ARCHIVED).join("artifacts/a.md")).ok(),
        Some(b"a\n".to_vec())
    );
}

/// R249 (R95P3-07): a reviewed copy stays one keeper reads whole. Read at
/// a length where another person's review lands exactly at the bound, the
/// review lands and the copy is read whole; one byte longer, that review is
/// refused and the copy keeps every byte.
#[test]
fn a_review_never_grows_the_copy_past_what_keeper_reads() {
    let (drive, zone) = drive();
    put(&zone, NOTE_REL, NOTE.as_bytes(), AN_HOUR);
    let profile = profile(drive.path());
    declare(&profile, &[TG]);
    let pinned = pin(&[TG]);
    let vault = Vault::at(drive.path());
    let out = out(&profile, &vault, Some(&pinned));
    let read = offer::read_note(&zone, SESSION, NOTE_REL, false, &out).expect("the candidate");
    offer::promote_to(
        &zone,
        SESSION,
        NOTE_REL,
        "10-notes/knowledge",
        "short.md",
        Some(&read.revision),
        Some(&tgorka()),
        &out,
    )
    .expect("promoted");
    let reviewed = std::fs::read_to_string(drive.path().join(TARGET)).expect("the copy");
    let growth = knowledge::review(&reviewed, "marta", AT, true).len() - reviewed.len();
    // The copy is filled with a long-named reviewer's entry, not an edit: it
    // stays the copy the note published (R252), only its review keys grow.
    let one = knowledge::review(&reviewed, "p", AT, true).len();
    for (name, over) in [("at the bound", 0), ("one byte past it", 1)] {
        let length = knowledge::MAX_REVIEWED_BYTES - growth + over;
        let copy = knowledge::review(&reviewed, &"p".repeat(1 + length - one), AT, true);
        assert_eq!(copy.len(), length, "{name}");
        std::fs::write(drive.path().join(TARGET), &copy).expect("the copy");
        let landed = tick(&zone, "marta", true, &out);
        let now = std::fs::read_to_string(drive.path().join(TARGET)).expect("the copy");
        if over == 0 {
            landed.expect(name);
            assert_eq!(now.len(), knowledge::MAX_REVIEWED_BYTES, "{name}");
            let whole = offer::read_note(&zone, SESSION, NOTE_REL, true, &out).expect(name);
            assert_eq!(whole.text, now, "{name}");
        } else {
            assert!(
                matches!(&landed, Err(VerbError::Refused(why)) if why.contains("would hold more than")),
                "{name}: {landed:?}"
            );
            assert_eq!(now, copy, "{name}: every byte kept");
        }
    }
}

/// The command vectors: Unix only, for the closed files their scenarios
/// make (`Permissions::from_mode`).
#[cfg(unix)]
mod command_vectors {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    /// R249 (R95P3-06): the outcomes of the panel's commands over real files —
    /// every panel, read, promotion out, review and archive of each scenario of
    /// `command-vectors.json` — are the ones recorded there, which the dev
    /// harness's test replays through its own handlers. With
    /// `KEEPER_WRITE_VECTORS` set, this records them instead.
    #[test]
    fn every_command_vector_holds() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/promote/command-vectors.json");
        let mut vectors: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("vectors")).expect("json");
        let record = std::env::var_os("KEEPER_WRITE_VECTORS").is_some();
        for scenario in vectors["scenarios"].as_array_mut().expect("scenarios") {
            let outcomes = command_outcomes(scenario);
            let name = scenario["name"].clone();
            for (step, outcome) in scenario["steps"]
                .as_array_mut()
                .expect("steps")
                .iter_mut()
                .zip(outcomes)
            {
                if record {
                    step["expect"] = outcome;
                } else {
                    let asked = step.get("expect").cloned().unwrap_or_default();
                    assert_eq!(asked, outcome, "{name}: {step}");
                }
            }
        }
        if record {
            let text = serde_json::to_string_pretty(&vectors).expect("json");
            std::fs::write(&path, format!("{text}\n")).expect("recorded");
        }
    }

    /// The bytes a vector's file stands for: its `text` or `hex`, padded with
    /// `pad.with` to `pad.to` bytes.
    fn vector_bytes(file: &serde_json::Value) -> Vec<u8> {
        let mut bytes = match file["hex"].as_str() {
            Some(hex) => (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
                .collect(),
            None => file["text"].as_str().expect("text").as_bytes().to_vec(),
        };
        if let Some(to) = file["pad"]["to"].as_u64() {
            let with = file["pad"]["with"].as_str().expect("with").as_bytes()[0];
            bytes.resize(usize::try_from(to).expect("length"), with);
        }
        bytes
    }

    /// Write `bytes` at `path`, last changed `changed` ms after the epoch.
    fn put_at(path: &Path, bytes: &[u8], changed: &serde_json::Value) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, bytes).expect("write");
        let at = UNIX_EPOCH + Duration::from_millis(changed.as_u64().expect("changed"));
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(at))
            .expect("mtime");
    }

    /// A command's outcome as a vector records it: `ok`, or the refusal.
    fn outcome_of<T>(result: Result<T, VerbError>) -> Result<T, serde_json::Value> {
        result.map_err(|error| {
            let sentence = match error {
                VerbError::Refused(sentence) => sentence,
                other => other.to_string(),
            };
            serde_json::json!({ "refused": sentence })
        })
    }

    /// The choices a step names: `"intent"`, the last panel's kept intent;
    /// otherwise each by the `item` of the last panel (its rows, then its
    /// unlisted files) or by a `revision` of its own.
    fn vector_choices(asked: &serde_json::Value, shown: &SessionPromoteVm) -> Vec<ChoiceVm> {
        if asked.as_str() == Some("intent") {
            return shown.intent.choices.clone();
        }
        let items: Vec<&String> = shown
            .rows
            .iter()
            .map(|row| &row.revision)
            .chain(shown.unlisted.iter().map(|file| &file.revision))
            .collect();
        asked
            .as_array()
            .expect("choices")
            .iter()
            .map(|choice| ChoiceVm {
                revision: match choice["item"].as_u64() {
                    Some(item) => items[usize::try_from(item).expect("item")].clone(),
                    None => choice["revision"].as_str().expect("revision").to_owned(),
                },
                promote: choice["promote"].as_bool().unwrap_or(false),
                target: choice["target"].as_str().unwrap_or_default().to_owned(),
            })
            .collect()
    }

    /// Every step's outcome of the vector `scenario`, run over real files.
    fn command_outcomes(scenario: &serde_json::Value) -> Vec<serde_json::Value> {
        let (drive, zone) = drive();
        let profile = profile(drive.path());
        declare(&profile, &[TG]);
        let pinned = pin(&[TG]);
        let vault = Vault::at(drive.path());
        let out = out(&profile, &vault, Some(&pinned));
        let session = zone.join(SESSION);
        // A vault copy's text as written: its bytes, each review in.
        let copy_text = |copy: &serde_json::Value| {
            let mut text = String::from_utf8(vector_bytes(copy)).expect("text");
            for by in copy["reviewers"].as_array().expect("reviewers") {
                let person = by.as_str().and_then(|by| by.strip_prefix("human:"));
                text = knowledge::review(&text, person.expect("a person"), AT, true);
            }
            text
        };
        let rows: String = scenario["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| {
                let (source, target) = (
                    row["source"].as_str().expect("source"),
                    row["target"].as_str().expect("target"),
                );
                // `"published": true`: the row records the scenario's copy at
                // its target as the one its source published.
                let published = (row["published"].as_bool() == Some(true)).then(|| {
                    let copy = scenario["vault"]
                        .as_array()
                        .expect("vault")
                        .iter()
                        .find(|copy| copy["path"] == target)
                        .expect("the copy it published");
                    promote::copy_digest(source, copy_text(copy).as_bytes())
                });
                promote::render_row(
                    source,
                    target,
                    row["note"].as_str().expect("note"),
                    published.as_deref(),
                )
            })
            .collect();
        std::fs::write(
            session.join("README.md"),
            format!("---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\n---\n# Session\n\n## Promote\n\n| workspace | → artifacts | note |\n| --- | --- | --- |\n{rows}"),
        )
        .expect("readme");
        for dir in scenario["dirs"].as_array().into_iter().flatten() {
            std::fs::create_dir_all(session.join(dir.as_str().expect("dir"))).expect("dir");
        }
        for file in scenario["files"].as_array().expect("files") {
            let rel = file["path"].as_str().expect("path");
            put_at(&session.join(rel), &vector_bytes(file), &file["changed"]);
        }
        for copy in scenario["vault"].as_array().expect("vault") {
            let text = copy_text(copy);
            let rel = copy["path"].as_str().expect("path");
            put_at(&drive.path().join(rel), text.as_bytes(), &copy["changed"]);
        }
        let at = |rel: &str| {
            if rel.starts_with("10-notes/") {
                drive.path().join(rel)
            } else {
                session.join(rel)
            }
        };
        let unreadable: Vec<PathBuf> = scenario["unreadable"]
            .as_array()
            .expect("unreadable")
            .iter()
            .map(|rel| at(rel.as_str().expect("path")))
            .collect();
        for path in &unreadable {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("closed");
        }

        let mut shown = None::<SessionPromoteVm>;
        let mut candidates = std::collections::HashMap::<String, String>::new();
        let mut copies = std::collections::HashMap::<String, String>::new();
        let mut outcomes = Vec::new();
        for step in scenario["steps"].as_array().expect("steps") {
            let ok = serde_json::json!({ "ok": true });
            let outcome = if let Some(asked) = step.get("panel") {
                let intent = match (&asked["intent"], &shown) {
                    (serde_json::Value::Null, _) => PanelIntentVm::default(),
                    (choices, Some(shown)) => PanelIntentVm {
                        choices: vector_choices(choices, shown),
                        notes: Vec::new(),
                    },
                    (_, None) => panic!("an intent needs a panel first"),
                };
                match outcome_of(offer::panel_for(&zone, SESSION, &out, Some(ME), &intent)) {
                    Ok(vm) => {
                        let value = serde_json::to_value(&vm).expect("vm");
                        shown = Some(vm);
                        value
                    }
                    Err(refused) => refused,
                }
            } else if let Some(asked) = step.get("read") {
                let path = asked["path"].as_str().expect("path");
                let copy = asked["copy"].as_bool().expect("copy");
                match outcome_of(offer::read_note(&zone, SESSION, path, copy, &out)) {
                    Ok(read) => {
                        let reads = if copy { &mut copies } else { &mut candidates };
                        reads.insert(path.to_owned(), read.revision.clone());
                        let mut value = serde_json::json!({
                            "revision": read.revision,
                            "bytes": read.text.len(),
                        });
                        if read.text.len() <= 4096 {
                            value["text"] = read.text.into();
                        }
                        value
                    }
                    Err(refused) => refused,
                }
            } else if let Some(asked) = step.get("promoteTo") {
                let source = asked["source"].as_str().expect("source");
                let expected = asked["expected"]
                    .as_bool()
                    .filter(|expected| *expected)
                    .and_then(|_| candidates.get(source));
                outcome_of(offer::promote_to(
                    &zone,
                    SESSION,
                    source,
                    asked["folder"].as_str().expect("folder"),
                    asked["name"].as_str().expect("name"),
                    expected.map(String::as_str),
                    Some(&tgorka()),
                    &out,
                ))
                .map_or_else(|refused| refused, |()| ok)
            } else if let Some(asked) = step.get("review") {
                let path = asked["path"].as_str().expect("path");
                outcome_of(review(
                    &zone,
                    SESSION,
                    path,
                    "tgorka",
                    AT,
                    asked["reviewed"].as_bool().expect("reviewed"),
                    copies.get(path).map_or("", String::as_str),
                    &out,
                ))
                .map_or_else(|refused| refused, |()| ok)
            } else if let Some(asked) = step.get("archive") {
                let shown = shown.as_ref().expect("an archive needs a panel first");
                let choices = vector_choices(&asked["choices"], shown);
                outcome_of(archive_with(&zone, &choices, &shown.revision))
                    .map_or_else(|refused| refused, |()| ok)
            } else if let Some(asked) = step.get("write") {
                let rel = asked["path"].as_str().expect("path");
                put_at(&session.join(rel), &vector_bytes(asked), &asked["changed"]);
                serde_json::Value::Null
            } else {
                panic!("a step this test does not know: {step}");
            };
            outcomes.push(outcome);
        }
        for path in &unreadable {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
        }
        outcomes
    }
}

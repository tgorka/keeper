//! The nightly consolidation over agentd's own engine, a real checkout and a
//! bare remote (story 95.2): one night per agent through
//! `consolidate::run_home`, and a person's decision carried out through
//! `consolidate::settle_decided`.
//!
//! Its own test binary: `open_engine` arms a process-global tier, as in
//! `headless_engine.rs`. Skipped, not failed, on a machine with no `git`.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use keeper_agent::consolidate::{self, waiting, Home, Outcome};
use keeper_agent::headless::{
    open_engine, AgentdEngine, HeadlessError, HeadlessSyncPlatform, SecretMap, SECRET_ENV_PREFIX,
};
use keeper_agent::zone::read_zone;
use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::agents::approval::{DecidedBy, Decision, DecisionRecord, WrittenBy};
use keeper_core::agents::consolidate::{Verdict, VerdictFile};
use keeper_core::agents::doorbell::rings;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
use keeper_core::agents::log::{
    ApprovalBody, ApprovalState, HostSlug, LineBody, LogLine, LINE_VERSION,
};
use keeper_core::agents::memory::MemoryTarget;
use keeper_core::agents::proposal::{Op, Origin, Proposal, Target};
use keeper_core::agents::session::{
    compose_session_agent_toml, parse_session_agent_toml, SessionAgent, SessionKind,
};
use keeper_core::agents::skills::{index, SkillFilter};
use keeper_sync::provenance::SyncSource;
use keeper_sync::xdg::SecretStore;
use keeper_sync::SyncError;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use ulid::Ulid;

const TGORKA: &str = "@tgorka:example.org";
const MARTA: &str = "@marta:example.org";
const MEMORY: &str = "80-agents/nixi/MEMORY.md";

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "seed")
        .env("GIT_AUTHOR_EMAIL", "seed@example.invalid")
        .env("GIT_COMMITTER_NAME", "seed")
        .env("GIT_COMMITTER_EMAIL", "seed@example.invalid")
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn user(id: &str) -> OwnedUserId {
    OwnedUserId::try_from(id).expect("user")
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

/// The night's window: today's 03:00 UTC.
fn window() -> DateTime<Utc> {
    let at = now();
    at.date_naive()
        .and_hms_opt(3, 0, 0)
        .expect("03:00")
        .and_utc()
}

fn memory_of(entries: &[&str]) -> String {
    format!("---\ntype: memory\n---\n{}\n", entries.join("\n§\n"))
}

/// A pending proposal's path and text, made `days_ago` days ago.
#[allow(clippy::too_many_arguments)]
fn proposal(
    seq: u128,
    target: Target,
    op: Op,
    body: &str,
    matched: Option<&str>,
    session: &str,
    integrity: Integrity,
    days_ago: i64,
) -> (String, String) {
    let created = now() - Duration::days(days_ago) - Duration::minutes(1);
    let id = Ulid::from_parts(
        u64::try_from(created.timestamp_millis()).expect("after 1970"),
        seq,
    );
    let text = Proposal {
        id,
        agent: "nixi".to_owned(),
        target,
        op,
        matched: matched.map(str::to_owned),
        session: format!("60-sessions/active/{session}"),
        host: "electra".to_owned(),
        origin: Origin::Foreground,
        label: Label {
            readers: Readers::Only(BTreeSet::from([user(TGORKA), user(MARTA)])),
            integrity,
            local_only: false,
        },
        created_at: created,
        body: body.to_owned(),
    }
    .render();
    (format!("80-agents/nixi/proposals/{id}.md"), text)
}

/// The same fact proposed in three sessions on three days, the first at
/// `first`'s integrity.
fn thrice(text: &str, target: MemoryTarget, first: Integrity) -> Vec<(String, String)> {
    (0..3)
        .map(|n| {
            let integrity = if n == 0 { first } else { Integrity::Agent };
            proposal(
                n as u128 + 1,
                Target::Memory(target),
                Op::Add,
                text,
                None,
                &format!("s{n}"),
                integrity,
                n,
            )
        })
        .collect()
}

/// A session folder's `agent.toml`, asked for by `requester`.
fn session(name: &str, requester: &str) -> (String, String) {
    let decl = keeper_core::agents::drive::parse(&drive_toml(&[TGORKA])).expect("decl");
    let agent = SessionAgent {
        id: Ulid::new(),
        agent: "nixi".to_owned(),
        drive: "tgdrive".to_owned(),
        kind: SessionKind::Main,
        title: name.to_owned(),
        requested_by: user(requester),
        parent: None,
        reply: None,
        room: OwnedRoomId::try_from(format!("!{name}:example.org")).expect("room"),
        drives: vec!["tgdrive".to_owned()],
        label: Label::opening(&decl, Integrity::Owner),
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: vec![user(requester)],
        limits: None,
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
        created_at: now(),
    };
    (
        format!("60-sessions/active/{name}/agent.toml"),
        compose_session_agent_toml(&agent),
    )
}

fn drive_toml(readers: &[&str]) -> String {
    let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
    format!(
        "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [{}]\n",
        readers.join(", ")
    )
}

fn yes() -> consolidate::Fence {
    Arc::new(|| true)
}

const AGENT_TOML: &str = "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"@nixi:example.org\"\nhuman = \"@tgorka:example.org\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[tools]\nallow = [\"memory_propose\"]\ndrives = [\"tgdrive\"]\n";

/// A drive checked out by agentd's engine, seeded with `files` beside the
/// zone, nixi's home and sessions `s0`–`s2` (`m…` sessions are Marta's).
struct World {
    _root: tempfile::TempDir,
    agentd: AgentdEngine,
    home: Home,
    /// Every home of the drive, nixi's first.
    homes: Vec<Home>,
}

/// `AGENT_TOML` for the agent `id`.
fn agent_toml(id: &str) -> String {
    AGENT_TOML
        .replace("id = \"nixi\"", &format!("id = \"{id}\""))
        .replace("Nixi", id)
        .replace("@nixi:", &format!("@{id}:"))
}

impl World {
    async fn new(readers: &[&str], files: &[(String, String)]) -> Option<World> {
        World::of(&["nixi"], readers, files).await
    }

    /// The drive with a home for each of `agents`, nixi first.
    async fn of(agents: &[&str], readers: &[&str], files: &[(String, String)]) -> Option<World> {
        let root = tempfile::tempdir().expect("tempdir");
        let bare = root.path().join("tgdrive.git");
        std::fs::create_dir_all(&bare).ok()?;
        git(&bare, &["init", "-q", "--bare", "-b", "main"])?;
        let seed = root.path().join("seed");
        std::fs::create_dir_all(&seed).expect("seed");
        git(&seed, &["init", "-q", "-b", "main"])?;
        let mut all: Vec<(String, String)> = vec![
            ("80-agents/_drive.toml".to_owned(), drive_toml(readers)),
            (
                "60-sessions/README.md".to_owned(),
                "# sessions\n".to_owned(),
            ),
        ];
        for agent in agents {
            all.push((format!("80-agents/{agent}/agent.toml"), agent_toml(agent)));
            all.push((format!("80-agents/{agent}/proposals/.keep"), String::new()));
        }
        for name in ["s0", "s1", "s2"] {
            all.push(session(name, TGORKA));
        }
        all.push(session("m0", MARTA));
        all.extend(files.iter().cloned());
        for (rel, text) in &all {
            let path = seed.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        }
        git(&seed, &["add", "-A"]).expect("add");
        git(&seed, &["commit", "-q", "-m", "seed"]).expect("commit");
        git(&seed, &["push", "-q", &bare.to_string_lossy(), "main"]).expect("push");

        let data = root.path().join("data");
        let mut text = String::from(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\nalways_on = true\n\n[homeserver]\nurl = \"https://matrix.example.org\"\n",
        );
        let quoted: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
        let ids: Vec<String> = agents.iter().map(|a| format!("\"{a}\"")).collect();
        text.push_str(&format!(
            "\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [{}]\n\n[[agents]]\ndrive = \"tgdrive\"\nids = [{}]\n",
            bare.display(),
            quoted.join(", "),
            ids.join(", ")
        ));
        let config = AgentdConfig::parse(&text).expect("config");
        let store = SecretStore::new(SECRET_ENV_PREFIX, data.join("secrets"));
        let platform = Arc::new(HeadlessSyncPlatform::new(
            &data,
            "electra",
            Arc::new(SecretMap::new(store)),
        ));
        let agentd = match open_engine(&config, platform) {
            Ok(agentd) => agentd,
            Err(HeadlessError::Sync(SyncError::GitMissing { .. })) => return None,
            Err(error) => panic!("open_engine: {error}"),
        };
        let profile_id = agentd.drives[0].profile_id.clone();
        agentd
            .engine
            .sync_once(&profile_id, SyncSource::Manual)
            .await
            .expect("the first checkout");
        let profile = agentd
            .engine
            .list_profiles()
            .expect("profiles")
            .into_iter()
            .find(|p| p.id == profile_id)
            .expect("profile");
        let decl = keeper_core::agents::drive::parse(&drive_toml(readers)).expect("decl");
        let zone = read_zone("tgdrive", &profile, Some(&decl));
        let homes: Vec<Home> = agents
            .iter()
            .map(|agent| {
                let (_, read) = zone
                    .homes
                    .iter()
                    .find(|(name, _)| name == agent)
                    .expect("the agent's home");
                let read = read.as_ref().expect("the home reads");
                Home::of(&profile, read, "electra").expect("home")
            })
            .collect();
        Some(World {
            _root: root,
            agentd,
            home: homes[0].clone(),
            homes,
        })
    }

    fn root(&self) -> &Path {
        &self.home.root
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root().join(rel)).ok()
    }

    async fn night(&self) -> Outcome {
        self.night_in("!review:example.org").await
    }

    /// The night, a new review session made in `room`.
    async fn night_in(&self, room: &str) -> Outcome {
        self.night_of(&self.home, room).await
    }

    /// `home`'s night, a new review session made in `room`.
    async fn night_of(&self, home: &Home, room: &str) -> Outcome {
        let room = OwnedRoomId::try_from(room).expect("room");
        consolidate::run_home(
            &self.agentd.engine,
            home,
            &self.homes,
            window(),
            now(),
            || async move { Ok(room) },
            &yes(),
        )
        .await
        .expect("the night runs")
    }

    /// The decisions people took, carried out.
    async fn settle(&self) {
        consolidate::settle_decided(&self.agentd.engine, &self.home, now(), &yes())
            .await
            .expect("carried out");
    }

    fn plan(&self) -> consolidate::Planned {
        consolidate::plan_home(&self.home, &self.homes, now()).expect("plan")
    }

    /// `body`, a line of the review session's log as its worker writes it.
    fn log(&self, body: ApprovalBody) {
        let host = HostSlug::new("electra").expect("slug");
        let mut writer = ChunkWriter::open(
            &self.review_dir(),
            &host,
            rotate_at(10 * 1024 * 1024),
            now().date_naive(),
        )
        .expect("the log");
        writer
            .append(&LogLine {
                v: LINE_VERSION,
                id: Ulid::new(),
                parent: None,
                ts: now(),
                host,
                epoch: 1,
                claim: None,
                matrix_event: None,
                body: LineBody::Approval(body),
            })
            .expect("a line");
        writer.sync().expect("synced");
    }

    /// The worker's line for the agent's one waiting review: `state`, with
    /// `decision` when it is a decision.
    fn mark(&self, state: ApprovalState, decision: Option<&str>) {
        let id = waiting(&self.home)
            .first()
            .expect("a review waits")
            .record
            .id
            .clone();
        self.log(ApprovalBody {
            id,
            state,
            decision: decision.map(str::to_owned),
            by: Some("electra".to_owned()),
            result: None,
            reason: None,
            scope: None,
        });
    }

    /// The worker won the consume-once of the agent's one waiting review.
    fn consume(&self) {
        self.mark(ApprovalState::Consumed, None);
    }

    fn head_message(&self) -> String {
        git(self.root(), &["log", "-1", "--format=%B"]).expect("log")
    }

    fn verdict(&self, proposal: &str) -> VerdictFile {
        let done = proposal
            .replace("/proposals/", "/proposals/done/")
            .replace(".md", ".verdict.toml");
        VerdictFile::parse(&self.read(&done).expect("verdict")).expect("parses")
    }

    fn review_dir(&self) -> PathBuf {
        self.root().join(format!(
            "60-sessions/active/{}",
            keeper_core::agents::consolidate::review_session_name("nixi", window())
        ))
    }

    /// The person `by` decides the agent's one waiting review.
    fn decide(&self, by: &str, decision: Decision) {
        let all = waiting(&self.home);
        let waiting = all.first().expect("a review waits");
        let record = &waiting.record;
        let decided = DecisionRecord {
            v: 1,
            id: record.id.clone(),
            decision,
            scope: record.scopes[0],
            note: None,
            binding_digest: record.binding_digest.clone(),
            decided_by: DecidedBy {
                user: by.to_owned(),
                device: "PHONE".to_owned(),
                verified: true,
            },
            decided_at: keeper_core::agents::approval::stamp(now()),
            matrix_event: None,
            written_by: WrittenBy {
                host: "electra".to_owned(),
                epoch: 1,
            },
        };
        std::fs::write(
            self.review_dir()
                .join("approvals")
                .join(format!("{}.decision.json", record.id)),
            serde_json::to_string_pretty(&decided).expect("json"),
        )
        .expect("the worker's decision");
        self.mark(
            ApprovalState::Decided,
            Some(match decision {
                Decision::Approve => "approve",
                Decision::Deny => "deny",
            }),
        );
    }
}

/// 95.2 acceptance 6 and 12 at the agent: a promoted candidate on a private
/// drive is one commit carrying its subject, the provenance block, the
/// consolidator's origin and one `Source-Session` per contributing session,
/// with the moved proposals and their verdicts; its paths ring `memory`.
#[tokio::test(flavor = "multi_thread")]
async fn consolidation_commits_carry_the_trailers() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Peer,
    );
    let mut files = staged.clone();
    files.push((MEMORY.to_owned(), memory_of(&["tgorka uses vim"])));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    let before = git(w.root(), &["rev-parse", "HEAD"]).expect("head");
    let Outcome::Committed(Some(commit)) = w.night().await else {
        panic!("a commit");
    };
    assert_eq!(
        w.read(MEMORY).expect("memory"),
        memory_of(&["tgorka uses vim", "tgorka reviews on Fridays"])
    );
    let message = w.head_message();
    let lines: Vec<&str> = message.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines[0], "memory: nixi — 3 promoted, 0 rejected");
    assert!(lines.iter().any(|l| l.starts_with("Keeper-Profile:")));
    assert_eq!(
        &lines[lines.len() - 4..],
        [
            "Memory-Origin: consolidator@electra",
            "Source-Session: 60-sessions/active/s0",
            "Source-Session: 60-sessions/active/s1",
            "Source-Session: 60-sessions/active/s2",
        ]
    );
    for (path, _) in &staged {
        assert!(w.read(path).is_none(), "{path} moved");
        let verdict = w.verdict(path);
        assert_eq!(verdict.verdict, Verdict::Promoted);
        assert_eq!(verdict.decided_by, "consolidator@electra");
        assert_eq!(verdict.commit, before.trim());
    }
    let paths = w
        .agentd
        .engine
        .changed_paths(&w.home.profile_id, Some(before.trim()), &commit)
        .expect("range");
    let rung = rings(&paths, Some("60-sessions"), Some("80-agents"));
    assert!(rung.memory && rung.sessions.is_empty(), "{rung:?}");
}

/// 95.2 acceptance 13: three `agent`-integrity sessions on three days pass
/// the score and are not applied; the review session, its card and a T2
/// record for tgorka are written instead, and tgorka's approval applies
/// exactly the previewed change. One `owner` proposal is applied by the
/// night (`consolidation_commits_carry_the_trailers` holds a `peer` one).
#[tokio::test(flavor = "multi_thread")]
async fn an_agents_own_repetition_needs_a_person() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let mut files = staged.clone();
    let memory = memory_of(&["tgorka uses vim"]);
    files.push((MEMORY.to_owned(), memory.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    assert!(matches!(w.night().await, Outcome::Committed(Some(_))));
    assert_eq!(w.read(MEMORY).expect("memory"), memory, "nothing applied");
    for (path, _) in &staged {
        assert!(w.read(path).is_some(), "{path} still pending");
    }
    let review = w.review_dir();
    let agent = parse_session_agent_toml(
        &std::fs::read_to_string(review.join("agent.toml")).expect("agent.toml"),
    )
    .expect("a session");
    assert_eq!(agent.kind, SessionKind::Scheduled);
    assert_eq!(agent.agent, "nixi");
    let card = std::fs::read_to_string(review.join(consolidate::REVIEW_CARD)).expect("card");
    let all = waiting(&w.home);
    assert_eq!(all.len(), 1);
    let record = &all[0].record;
    assert_eq!(
        all[0].args.preview,
        format!("artifacts/memory-review-{}.md", record.id),
        "the preview is named by its record"
    );
    assert!(card.contains(&all[0].args.preview), "{card}");
    assert_eq!(record.action.tool, "memory_apply");
    assert_eq!(record.risk.tier, 2);
    assert_eq!(all[0].args.approvers, [TGORKA]);
    let preview = std::fs::read_to_string(review.join(&all[0].args.preview)).expect("preview");
    assert_eq!(
        keeper_core::agents::consolidate::sha256_hex(preview.as_bytes()),
        all[0].args.preview_sha256
    );
    let keeper_core::agents::consolidate::After::Text(after) = all[0].args.change.after.clone()
    else {
        panic!("a text change");
    };

    // A second night leaves what waits alone.
    w.night().await;
    assert_eq!(waiting(&w.home).len(), 1);

    w.decide(TGORKA, Decision::Approve);
    w.settle().await;
    assert_eq!(
        w.read(MEMORY).expect("memory"),
        memory,
        "decided, not consumed"
    );
    w.consume();
    w.settle().await;
    assert_eq!(
        w.read(MEMORY).expect("memory"),
        after,
        "exactly the preview"
    );
    for (path, _) in &staged {
        let verdict = w.verdict(path);
        assert_eq!(verdict.verdict, Verdict::Promoted);
        assert_eq!(verdict.decided_by, TGORKA);
    }
    assert!(w
        .head_message()
        .contains("Source-Session: 60-sessions/active/s2"));

    // The same candidate with one proposal at `owner` integrity is applied
    // by the night.
    let staged = thrice(
        "tgorka plans on Mondays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    w.night().await;
    assert!(w
        .read(MEMORY)
        .is_some_and(|text| text.contains("tgorka plans on Mondays")));
    assert!(waiting(&w.home).is_empty());
}

/// 95.2 acceptance 10: on a drive two people read nothing of the home
/// changes by night; `MEMORY.md` waits for the drive's owner and `USER.md`
/// for the source session's requester; a denial rejects the proposals in
/// that person's name.
#[tokio::test(flavor = "multi_thread")]
async fn on_a_shared_drive_nothing_changes_without_its_person() {
    let mut staged: Vec<(String, String)> = (0..3)
        .map(|n| {
            proposal(
                10 + n as u128,
                Target::Memory(MemoryTarget::User),
                Op::Add,
                "marta prefers mornings",
                None,
                if n == 0 { "m0" } else { "s1" },
                Integrity::Owner,
                n,
            )
        })
        .collect();
    staged.push(proposal(
        20,
        Target::Memory(MemoryTarget::User),
        Op::Add,
        "marta prefers mornings",
        None,
        "s2",
        Integrity::Owner,
        2,
    ));
    let Some(w) = World::new(&[TGORKA, MARTA], &staged).await else {
        return;
    };
    let tree = git(w.root(), &["ls-tree", "-r", "HEAD", "80-agents/nixi"]).expect("tree");
    w.night().await;
    let after = git(w.root(), &["ls-tree", "-r", "HEAD", "80-agents/nixi"]).expect("tree");
    assert_eq!(tree, after, "no file in the home changed");
    let all = waiting(&w.home);
    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].args.approvers,
        [MARTA, TGORKA],
        "USER.md waits for the people whose sessions it came from"
    );

    w.decide(MARTA, Decision::Deny);
    w.settle().await;
    assert!(w.read("80-agents/nixi/USER.md").is_none());
    for (path, _) in &staged {
        let verdict = w.verdict(path);
        assert_eq!(verdict.verdict, Verdict::Rejected);
        assert_eq!(verdict.decided_by, MARTA);
    }
}

/// 95.2 acceptance 5, the first half: a `MEMORY.md` a person left in a
/// shape keeper would not write back makes the night write nothing of the
/// agent's memory and settle none of its proposals; the review card quotes
/// Hermes' drift sentence.
#[tokio::test(flavor = "multi_thread")]
async fn a_hand_edited_memory_file_is_never_clobbered() {
    let mut files = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let edited = "---\ntype: memory\nnote: {typed: by hand}\n---\ntgorka uses vim\n".to_owned();
    files.push((MEMORY.to_owned(), edited.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    assert_eq!(w.read(MEMORY).expect("memory"), edited);
    for (path, _) in &files[..3] {
        assert!(w.read(path).is_some(), "{path} still pending");
    }
    let card =
        std::fs::read_to_string(w.review_dir().join(consolidate::REVIEW_CARD)).expect("card");
    assert!(
        card.contains(&keeper_ported::hermes::memory::drift_message("MEMORY.md")),
        "{card}"
    );
}

fn skill(name: &str, key: Option<&str>) -> String {
    let metadata = key.map_or(String::new(), |key| {
        format!("metadata:\n  keeper_proposal: \"{key}\"\n")
    });
    format!("---\nname: {name}\ndescription: Does {name}.\n{metadata}---\n\nSteps.\n")
}

/// 95.2 acceptance 11: on a private drive an agent's valid `create` lands
/// stamped and is not offered — the index names it waiting — until a person
/// deletes the key; its patch of a person's skill waits for them; a skill
/// agentskills refuses is rejected with the validator's reason.
#[tokio::test(flavor = "multi_thread")]
async fn skill_proposals_land_validated_stamped_and_unoffered() {
    let person = skill("triage", None);
    let create = proposal(
        30,
        Target::Skill("weekly".to_owned()),
        Op::Create,
        &skill("weekly", None),
        None,
        "s0",
        Integrity::Agent,
        0,
    );
    let patch = proposal(
        31,
        Target::Skill("triage".to_owned()),
        Op::Patch,
        &skill("triage", None).replace("Steps.", "Better steps."),
        Some(&keeper_core::agents::consolidate::sha256_hex(
            person.as_bytes(),
        )),
        "s1",
        Integrity::Agent,
        0,
    );
    let broken = proposal(
        32,
        Target::Skill("broken".to_owned()),
        Op::Create,
        "---\nname: other\n---\nno description\n",
        None,
        "s2",
        Integrity::Agent,
        0,
    );
    let files = vec![
        create.clone(),
        patch.clone(),
        broken.clone(),
        (
            "80-agents/_skills/triage/SKILL.md".to_owned(),
            person.clone(),
        ),
    ];
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    let landed = w
        .read("80-agents/_skills/weekly/SKILL.md")
        .expect("created");
    let id = create
        .0
        .trim_start_matches("80-agents/nixi/proposals/")
        .trim_end_matches(".md")
        .to_owned();
    assert!(
        landed.contains(&format!("keeper_proposal: {id}\n")),
        "{landed}"
    );
    let offered = index(&[("weekly".to_owned(), landed.clone())], &SkillFilter::All);
    assert!(
        offered.offered.is_empty() && offered.waiting.len() == 1,
        "{offered:?}"
    );
    let adopted = landed.replace(&format!("metadata:\n  keeper_proposal: {id}\n"), "");
    let offered = index(&[("weekly".to_owned(), adopted)], &SkillFilter::All);
    assert_eq!(
        offered.offered.len(),
        1,
        "a person deleted the key: offered"
    );

    assert_eq!(
        w.read("80-agents/_skills/triage/SKILL.md")
            .expect("person's"),
        person,
        "a person's skill waits for them"
    );
    assert!(w.read(&patch.0).is_some(), "the patch is pending review");
    let all = waiting(&w.home);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].record.action.tool, "skill_apply");

    let verdict = w.verdict(&broken.0);
    assert_eq!(verdict.verdict, Verdict::Rejected);
    assert!(
        verdict.reason.starts_with("agentskills: "),
        "{}",
        verdict.reason
    );
}

/// 95.2 acceptance 5, the second half at the agent: a file edited on the
/// disk after the night's read is not written over and nothing of the
/// agent's night is committed.
#[tokio::test(flavor = "multi_thread")]
async fn a_concurrent_edit_skips_the_night() {
    let mut files = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    files.push((MEMORY.to_owned(), memory_of(&["tgorka uses vim"])));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    let planned = w.plan();
    std::fs::write(
        w.root().join(MEMORY),
        "typed by a person while the night ran\n",
    )
    .expect("edit");
    let head = git(w.root(), &["rev-parse", "HEAD"]);
    let done = w
        .agentd
        .engine
        .commit_paths(&w.home.profile_id, &planned.request, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        keeper_sync::CommitPaths::Guarded {
            path: MEMORY.to_owned()
        }
    );
    assert_eq!(git(w.root(), &["rev-parse", "HEAD"]), head);
    for (path, _) in &files[..3] {
        assert!(w.read(path).is_some(), "{path} still pending");
    }
}

/// `staged` with its label narrowed to `readers`.
fn narrowed((path, text): (String, String), readers: &[&str]) -> (String, String) {
    let stem = path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".md"))
        .expect("a proposal");
    let mut proposal = Proposal::parse(stem, &text).expect("parses");
    proposal.label.readers = Readers::Only(readers.iter().map(|r| user(r)).collect());
    (path, proposal.render())
}

impl World {
    /// Commit `files` and `moves` in the checkout, as a pull would bring
    /// them.
    fn commit(&self, files: &[(String, String)], moves: &[(&str, &str)]) {
        for (rel, text) in files {
            let path = self.root().join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        }
        for (from, to) in moves {
            let to = self.root().join(to);
            std::fs::create_dir_all(to.parent().expect("parent")).expect("mkdir");
            std::fs::rename(self.root().join(from), to).expect("move");
        }
        git(self.root(), &["add", "-A"]).expect("add");
        git(self.root(), &["commit", "-q", "-m", "elsewhere"]).expect("commit");
    }

    fn head(&self) -> String {
        git(self.root(), &["rev-parse", "HEAD"]).expect("head")
    }
}

/// R95C-01: an approval is carried out only once the worker's consume-once
/// authorization of it succeeded: a decision alone, or one whose record
/// then ended refused, changes nothing — and an ended record no longer
/// holds its proposals, so a later night reviews them again.
#[tokio::test(flavor = "multi_thread")]
async fn a_decision_is_carried_out_only_once_consumed() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let mut files = staged.clone();
    let memory = memory_of(&["tgorka uses vim"]);
    files.push((MEMORY.to_owned(), memory.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    w.decide(TGORKA, Decision::Approve);
    let head = w.head();
    w.settle().await;
    assert_eq!(w.head(), head, "decided, not consumed: nothing");
    w.mark(ApprovalState::Refused, None);
    w.settle().await;
    assert_eq!(w.head(), head, "refused: nothing");
    assert_eq!(w.read(MEMORY).expect("memory"), memory);
    for (path, _) in &staged {
        assert!(w.read(path).is_some(), "{path} still pending");
    }
    w.night().await;
    let standings: Vec<consolidate::Standing> =
        waiting(&w.home).iter().map(|one| one.standing).collect();
    assert_eq!(standings.len(), 2, "{standings:?}");
    assert!(
        standings.contains(&consolidate::Standing::Ended)
            && standings.contains(&consolidate::Standing::Open),
        "the ended review's proposals wait on a new one: {standings:?}"
    );
}

/// R95C-02: an owner-only review decided by another reader of the drive —
/// consumed even — changes nothing: the approvers are the arguments', not
/// whoever reads the session.
#[tokio::test(flavor = "multi_thread")]
async fn a_decision_by_someone_it_does_not_name_changes_nothing() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let Some(w) = World::new(&[TGORKA, MARTA], &staged).await else {
        return;
    };
    w.night().await;
    assert_eq!(waiting(&w.home)[0].args.approvers, [TGORKA]);
    w.decide(MARTA, Decision::Approve);
    w.consume();
    let head = w.head();
    w.settle().await;
    assert_eq!(w.head(), head);
    assert!(w.read(MEMORY).is_none());
}

/// R95C-03 at the agent: on a drive two people read, proposals only one of
/// them may read are rejected — no review session, no preview, no record
/// shows them to the other.
#[tokio::test(flavor = "multi_thread")]
async fn a_narrow_proposal_is_never_published_on_a_shared_drive() {
    let staged: Vec<(String, String)> = thrice(
        "tgorka's private note",
        MemoryTarget::Memory,
        Integrity::Owner,
    )
    .into_iter()
    .map(|one| narrowed(one, &[TGORKA]))
    .collect();
    let Some(w) = World::new(&[TGORKA, MARTA], &staged).await else {
        return;
    };
    w.night().await;
    assert!(!w.review_dir().exists(), "nothing published");
    for (path, _) in &staged {
        assert_eq!(w.verdict(path).verdict, Verdict::Rejected);
    }
}

/// R95C-04 and the owed lease test at the agent: a night whose fence says
/// the lease is gone makes no room and writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_night_without_its_lease_writes_nothing() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    let head = w.head();
    let outcome = consolidate::run_home(
        &w.agentd.engine,
        &w.home,
        &w.homes,
        window(),
        now(),
        || async { panic!("no room is made without the lease") },
        &(Arc::new(|| false) as consolidate::Fence),
    )
    .await
    .expect("runs");
    assert_eq!(outcome, Outcome::Fenced);
    assert_eq!(w.head(), head);
    assert!(!w.review_dir().exists());
}

/// R95C-09: the same night run again keeps its review session — its room,
/// its `agent.toml`, its earlier previews and records — and adds to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_night_run_again_keeps_its_review_session() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    w.night_in("!first:example.org").await;
    let first = waiting(&w.home);
    let preview = w.review_dir().join(&first[0].args.preview);
    let shown = std::fs::read_to_string(&preview).expect("preview");
    let later: Vec<(String, String)> = (0..3)
        .map(|n| {
            proposal(
                100 + n as u128,
                Target::Memory(MemoryTarget::User),
                Op::Add,
                "tgorka is in Warsaw",
                None,
                &format!("s{n}"),
                Integrity::Agent,
                n,
            )
        })
        .collect();
    w.commit(&later, &[]);
    w.night_in("!second:example.org").await;
    let agent = parse_session_agent_toml(
        &std::fs::read_to_string(w.review_dir().join("agent.toml")).expect("agent.toml"),
    )
    .expect("a session");
    assert_eq!(agent.room.as_str(), "!first:example.org");
    assert_eq!(std::fs::read_to_string(&preview).expect("kept"), shown);
    assert_eq!(waiting(&w.home).len(), 2);
}

/// R95C-14: the night reads the agent's `agent.toml` at the head it pulled:
/// `promote = false` committed elsewhere before the night sends the change
/// to a person though the host admitted the agent with it on; a drive
/// declaration that changed since the host admitted it holds the night.
#[tokio::test(flavor = "multi_thread")]
async fn the_night_reads_its_declarations_at_the_head() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    assert!(w.home.config.memory.promote);
    w.commit(
        &[(
            "80-agents/nixi/agent.toml".to_owned(),
            format!("{AGENT_TOML}\n[memory]\npromote = false\n"),
        )],
        &[],
    );
    w.night().await;
    assert!(w.read(MEMORY).is_none(), "nothing applied");
    assert_eq!(waiting(&w.home).len(), 1, "it waits for a person");

    w.commit(
        &[(
            "80-agents/_drive.toml".to_owned(),
            drive_toml(&[TGORKA, MARTA]),
        )],
        &[],
    );
    assert!(consolidate::plan_home(&w.home, &w.homes, now()).is_err());
}

/// R95C-15: a proposal whose session was archived since is still that
/// session's: it promotes, and its `Source-Session` names it as proposed.
#[tokio::test(flavor = "multi_thread")]
async fn a_proposal_of_an_archived_session_is_still_its_sessions() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    w.commit(
        &[],
        &[("60-sessions/active/s1", "60-sessions/archive/2026/s1")],
    );
    w.night().await;
    for (path, _) in &staged {
        assert_eq!(w.verdict(path).verdict, Verdict::Promoted, "{path}");
    }
}

/// R95C-22: the consolidator reads the approval store as the worker does:
/// a store that is a link is not read, and a record under another id than
/// its own is not this review's.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_consolidator_reads_only_protected_records() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    w.night().await;
    let id = waiting(&w.home)[0].record.id.clone();
    let store = w.review_dir().join("approvals");
    std::fs::copy(
        store.join(format!("{id}.json")),
        store.join(format!("{}.json", Ulid::new())),
    )
    .expect("a copy");
    assert_eq!(waiting(&w.home).len(), 1, "the copy names another id");

    let real = w.root().join("elsewhere");
    std::fs::rename(&store, &real).expect("move");
    std::os::unix::fs::symlink(&real, &store).expect("link");
    assert!(waiting(&w.home).is_empty(), "a linked store is not read");
}

/// `(path, text)` of a proposal of nixi's, made otto's.
fn ottos((path, text): (String, String)) -> (String, String) {
    let stem = path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".md"))
        .expect("a proposal");
    let mut proposal = Proposal::parse(stem, &text).expect("parses");
    proposal.agent = "otto".to_owned();
    (path.replace("/nixi/", "/otto/"), proposal.render())
}

/// R95CR-10, R95CR-01 and R95C-17: an approved review is applied once, by
/// a commit naming its record; a person who reverts that commit is not
/// overruled by the next tick — the history, not the pending proposals,
/// says it was carried out — and its file is free again: what is pending
/// for it is reviewed anew.
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_is_applied_once_and_frees_its_file() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let mut files = staged.clone();
    let memory = memory_of(&["tgorka uses vim"]);
    files.push((MEMORY.to_owned(), memory.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    let id = waiting(&w.home)[0].record.id.clone();
    w.decide(TGORKA, Decision::Approve);
    w.consume();
    w.settle().await;
    assert!(
        w.head_message().contains(&format!("Approval-Record: {id}")),
        "{}",
        w.head_message()
    );
    assert_ne!(w.read(MEMORY).expect("memory"), memory, "applied");

    git(w.root(), &["revert", "--no-edit", "HEAD"]).expect("a person reverts it");
    assert_eq!(w.read(MEMORY).expect("memory"), memory);
    let head = w.head();
    assert!(!consolidate::any_decided(&w.homes), "carried out already");
    w.settle().await;
    assert_eq!(w.head(), head, "never applied again");
    assert_eq!(w.read(MEMORY).expect("memory"), memory);

    w.night().await;
    let all = waiting(&w.home);
    assert_eq!(all.len(), 2, "the file is reviewed again");
    assert!(all
        .iter()
        .any(|one| one.record.id != id && one.standing == consolidate::Standing::Open));
}

/// R95CR-01 on a denial: once the rejection is committed, naming the
/// record, the denied review no longer holds its file, and a new proposal
/// for it is reviewed — read from the drive alone, as a restarted host
/// reads it.
#[tokio::test(flavor = "multi_thread")]
async fn a_denied_review_frees_its_file_once_rejected() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    w.night().await;
    let id = waiting(&w.home)[0].record.id.clone();
    w.decide(TGORKA, Decision::Deny);
    w.settle().await;
    assert!(w.head_message().contains(&format!("Approval-Record: {id}")));
    for (path, _) in &staged {
        assert_eq!(w.verdict(path).verdict, Verdict::Rejected);
    }
    let later: Vec<(String, String)> = (0..3)
        .map(|n| {
            proposal(
                60 + n as u128,
                Target::Memory(MemoryTarget::Memory),
                Op::Add,
                "tgorka plans on Mondays",
                None,
                &format!("s{n}"),
                Integrity::Agent,
                n,
            )
        })
        .collect();
    w.commit(&later, &[]);
    w.night().await;
    let all = waiting(&w.home);
    assert_eq!(all.len(), 2, "a new review of the same file");
    let new = all.iter().find(|one| one.record.id != id).expect("new");
    assert_eq!(new.standing, consolidate::Standing::Open);
}

/// R95CR-11 and R95C-03 at the room: the drive's declaration changed on
/// the disk after the night read it at `HEAD` makes no room — whose
/// invitations would follow it — and writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_declaration_changed_since_the_plan_makes_no_room() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    std::fs::write(
        w.root().join("80-agents/_drive.toml"),
        drive_toml(&[TGORKA, MARTA]),
    )
    .expect("widened by hand");
    let head = w.head();
    let outcome = consolidate::run_home(
        &w.agentd.engine,
        &w.home,
        &w.homes,
        window(),
        now(),
        || async { panic!("no room is made for an audience the night did not read") },
        &yes(),
    )
    .await
    .expect("runs");
    assert_eq!(
        outcome,
        Outcome::Skipped {
            path: "80-agents/_drive.toml".to_owned()
        }
    );
    assert_eq!(w.head(), head);
    assert!(!w.review_dir().exists());
}

/// R95CR-12 and R95C-18: a skill is the drive's. nixi's open review of a
/// person's skill holds it for otto too: otto's patch of the same skill
/// makes no second review over the same base, and waits.
#[tokio::test(flavor = "multi_thread")]
async fn one_review_per_skill_across_the_drive() {
    let person = skill("triage", None);
    let patch = |seq: u128, session: &str| {
        proposal(
            seq,
            Target::Skill("triage".to_owned()),
            Op::Patch,
            &skill("triage", None).replace("Steps.", &format!("Steps of {session}.")),
            Some(&keeper_core::agents::consolidate::sha256_hex(
                person.as_bytes(),
            )),
            session,
            Integrity::Agent,
            0,
        )
    };
    let otto = ottos(patch(71, "s1"));
    let files = vec![
        patch(70, "s0"),
        otto.clone(),
        (
            "80-agents/_skills/triage/SKILL.md".to_owned(),
            person.clone(),
        ),
    ];
    let Some(w) = World::of(&["nixi", "otto"], &[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    assert_eq!(waiting(&w.homes[0]).len(), 1, "nixi's review");
    w.night_of(&w.homes[1], "!otto:example.org").await;
    assert!(waiting(&w.homes[1]).is_empty(), "no second review");
    assert!(w.read(&otto.0).is_some(), "otto's patch waits");
    assert_eq!(
        w.read("80-agents/_skills/triage/SKILL.md").expect("skill"),
        person
    );
}

/// `git <args>` in `dir` as a person whose clock says `date`.
fn git_dated(dir: &Path, date: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "p")
        .env("GIT_AUTHOR_EMAIL", "p@example.invalid")
        .env("GIT_COMMITTER_NAME", "p")
        .env("GIT_COMMITTER_EMAIL", "p@example.invalid")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Three more proposals for nixi's MEMORY.md, staged later.
fn later_proposals() -> Vec<(String, String)> {
    (0..3)
        .map(|n| {
            proposal(
                60 + n as u128,
                Target::Memory(MemoryTarget::Memory),
                Op::Add,
                "tgorka plans on Mondays",
                None,
                &format!("s{n}"),
                Integrity::Agent,
                n,
            )
        })
        .collect()
}

/// R95C3-05: whether an approval was carried out is read from every
/// commit the drive's history reaches, whatever their dates: a person's
/// revert dated years before the record is not taken for the end of the
/// history, and the approval is never applied again.
#[tokio::test(flavor = "multi_thread")]
async fn an_applied_approval_is_found_whatever_the_dates_say() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let mut files = staged.clone();
    let memory = memory_of(&["tgorka uses vim"]);
    files.push((MEMORY.to_owned(), memory.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    w.decide(TGORKA, Decision::Approve);
    w.consume();
    w.settle().await;
    assert_ne!(w.read(MEMORY).expect("memory"), memory, "applied");
    git_dated(
        w.root(),
        "2001-01-01T00:00:00Z",
        &["revert", "--no-edit", "HEAD"],
    )
    .expect("a person reverts it, their clock years behind");
    let head = w.head();
    assert!(!consolidate::any_decided(&w.homes), "carried out already");
    w.settle().await;
    assert_eq!(w.head(), head, "never applied again");
    assert_eq!(w.read(MEMORY).expect("memory"), memory);
}

/// R95C3-06: a history that cannot be read says nothing of whether an
/// approval was carried out — it is not applied on it, and it keeps its
/// file: a competing proposal for the file gets no second review, not
/// while the history is unreadable and not once it reads again; then the
/// approval is applied, once.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_history_applies_nothing_and_frees_nothing() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let mut files = staged.clone();
    let memory = memory_of(&["tgorka uses vim"]);
    files.push((MEMORY.to_owned(), memory.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    let id = waiting(&w.home)[0].record.id.clone();
    w.decide(TGORKA, Decision::Approve);
    w.consume();
    w.commit(&later_proposals(), &[]);
    let good = w.head();
    let tree = git(w.root(), &["rev-parse", "HEAD^{tree}"]).expect("tree");
    let broken = format!(
        "tree {}\nparent {}\nauthor p <p@e> 1700000000 +0000\ncommitter p <p@e> 1700000000 +0000\n\nits parent is not here\n",
        tree.trim(),
        "1".repeat(40)
    );
    let unreadable = w.root().join("broken-commit");
    std::fs::write(&unreadable, broken).expect("commit text");
    let bad = git(
        w.root(),
        &[
            "hash-object",
            "-t",
            "commit",
            "-w",
            "--literally",
            &unreadable.to_string_lossy(),
        ],
    )
    .expect("a commit whose history does not read");
    std::fs::remove_file(&unreadable).expect("clean");
    git(w.root(), &["update-ref", "refs/heads/main", bad.trim()]).expect("HEAD on it");

    assert!(w.plan().plan.reviews.is_empty(), "the file stays reserved");
    assert!(consolidate::any_decided(&w.homes), "not known to be done");
    let head = w.head();
    w.settle().await;
    assert_eq!(w.head(), head, "nothing applied on an unknown");
    assert_eq!(w.read(MEMORY).expect("memory"), memory);

    git(w.root(), &["update-ref", "refs/heads/main", good.trim()]).expect("readable again");
    assert!(w.plan().plan.reviews.is_empty(), "still reserved");
    w.settle().await;
    assert!(
        w.head_message().contains(&format!("Approval-Record: {id}")),
        "{}",
        w.head_message()
    );
    assert_eq!(waiting(&w.home).len(), 1, "no second review");
}

/// R95C3-07: a night is complete only once everything it began is
/// finished — a commit published whose files could not all be written
/// leaves it unfinished, until the commit is settled.
#[tokio::test(flavor = "multi_thread")]
async fn a_night_whose_commit_did_not_finish_is_not_complete() {
    use std::os::unix::fs::PermissionsExt as _;
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Peer,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    let done = w.root().join("80-agents/nixi/proposals/done");
    std::fs::create_dir_all(&done).expect("done");
    std::fs::set_permissions(&done, std::fs::Permissions::from_mode(0o555)).expect("read-only");
    let outcome = w.night().await;
    std::fs::set_permissions(&done, std::fs::Permissions::from_mode(0o755)).expect("writable");
    assert!(
        matches!(outcome, Outcome::Committed(Some(_))),
        "{outcome:?}"
    );
    assert_eq!(
        consolidate::settled(&w.agentd.engine, &w.homes).await,
        Ok(false),
        "its files did not all follow"
    );
    w.agentd
        .engine
        .sync_once(&w.home.profile_id, SyncSource::Manual)
        .await
        .expect("the next pass finishes it");
    assert_eq!(
        consolidate::settled(&w.agentd.engine, &w.homes).await,
        Ok(true)
    );
}

/// R95C3-07: a decision whose rejection could not be committed — the
/// approved file and a proposal both changed since — leaves the night
/// unfinished, until the rejection is made.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_rejection_leaves_the_night_unfinished() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let mut files = staged.clone();
    files.push((MEMORY.to_owned(), memory_of(&["tgorka uses vim"])));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    w.night().await;
    w.decide(TGORKA, Decision::Approve);
    w.consume();
    std::fs::write(w.root().join(MEMORY), "a person's edit").expect("edited");
    let proposal = w.root().join(&staged[0].0);
    std::fs::write(&proposal, "a person's edit too").expect("edited");
    w.settle().await;
    assert_eq!(
        consolidate::settled(&w.agentd.engine, &w.homes).await,
        Ok(false),
        "the decision is not carried out"
    );
    std::fs::write(&proposal, &staged[0].1).expect("put back");
    w.settle().await;
    assert_eq!(
        consolidate::settled(&w.agentd.engine, &w.homes).await,
        Ok(true)
    );
}

/// The night reads proposals as the memory tools publish them: only
/// `<ulid>.md`. The dotted part file a host that died mid-publication left
/// beside them — committed since, holding a whole proposal's bytes — is
/// never read as a proposal, never settled and never moved.
#[tokio::test(flavor = "multi_thread")]
async fn a_proposals_part_file_is_never_a_proposal() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let (first, text) = staged[0].clone();
    let (dir, name) = first.rsplit_once('/').expect("a proposal's path");
    let part = format!("{dir}/.{name}.{}.part", Ulid::new());
    let mut files = staged.clone();
    files.push((part.clone(), text.clone()));
    files.push((MEMORY.to_owned(), memory_of(&["tgorka uses vim"])));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    let Outcome::Committed(Some(_)) = w.night().await else {
        panic!("a commit");
    };
    assert_eq!(
        w.head_message().lines().next(),
        Some("memory: nixi — 3 promoted, 0 rejected")
    );
    assert_eq!(
        w.read(MEMORY).expect("memory"),
        memory_of(&["tgorka uses vim", "tgorka reviews on Fridays"])
    );
    assert_eq!(w.read(&part).as_deref(), Some(text.as_str()));
    assert_eq!(
        git(w.root(), &["ls-files", "--", &part]).map(|out| out.trim().to_owned()),
        Some(part.clone())
    );
}

/// A proposal the memory tools took back — its folder could not be synced
/// after it was linked, so the call was refused and the file removed — is
/// never applied, even when the drive committed it and the night planned
/// over it: the night's commit is guarded on every settled proposal as it
/// is on the disk right before the publication, under the lease, so that
/// agent's night writes nothing; and a night planned after the take-back
/// does not count it.
#[tokio::test(flavor = "multi_thread")]
async fn a_proposal_taken_back_is_never_applied() {
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Owner,
    );
    let mut files = staged.clone();
    files.push((MEMORY.to_owned(), memory_of(&["tgorka uses vim"])));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    let planned = w.plan();
    assert_eq!(planned.plan.settled.len(), 3);
    std::fs::remove_file(w.root().join(&staged[0].0)).expect("taken back");
    let head = w.head();
    let done = w
        .agentd
        .engine
        .commit_paths(&w.home.profile_id, &planned.request, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        keeper_sync::CommitPaths::Guarded {
            path: staged[0].0.clone()
        }
    );
    assert_eq!(w.head(), head);
    assert_eq!(
        w.read(MEMORY).expect("memory"),
        memory_of(&["tgorka uses vim"])
    );
    for (path, _) in &staged[1..] {
        assert!(w.read(path).is_some(), "{path} still pending");
    }
    let after = w.plan();
    assert!(after.plan.settled.is_empty(), "{:?}", after.plan.settled);
    assert!(after.plan.writes.is_empty());
}

/// The proposal id `path` names.
fn id_of(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".md"))
        .expect("a proposal")
        .to_owned()
}

/// R95C4-04: a review's change is made from every proposal its preview
/// shows. One of them taken back before the review is published publishes
/// no review; one taken back — and that removal committed — after it was
/// published leaves the approved change unapplied, whatever was decided:
/// nothing is written, nothing is settled, no decision stays outstanding,
/// and the next night previews what is still pending again, without it.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_that_lost_a_proposal_is_never_applied() {
    let first = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let second = later_proposals();
    let mut files = first.clone();
    files.extend(second.iter().cloned());
    let memory = memory_of(&["tgorka uses vim"]);
    files.push((MEMORY.to_owned(), memory.clone()));
    let Some(w) = World::new(&[TGORKA], &files).await else {
        return;
    };
    let taken = id_of(&first[0].0);

    let mut planned = w.plan();
    assert_eq!(planned.plan.reviews.len(), 1, "{:?}", planned.plan.reviews);
    assert_eq!(planned.plan.reviews[0].proposals.len(), 6);
    let room = OwnedRoomId::try_from("!review:example.org").expect("room");
    consolidate::add_review(&w.home, &mut planned, window(), &room, now()).expect("review");
    std::fs::remove_file(w.root().join(&first[0].0)).expect("taken back");
    let head = w.head();
    let done = w
        .agentd
        .engine
        .commit_paths(&w.home.profile_id, &planned.request, yes())
        .await
        .expect("checked");
    assert_eq!(
        done,
        keeper_sync::CommitPaths::Guarded {
            path: first[0].0.clone()
        }
    );
    assert_eq!(w.head(), head);
    assert!(!w.review_dir().exists(), "no review published");
    std::fs::write(w.root().join(&first[0].0), &first[0].1).expect("back");

    w.night().await;
    let all = waiting(&w.home);
    assert_eq!(all.len(), 1);
    assert!(all[0].args.proposals.contains(&taken));
    git(w.root(), &["rm", "-q", &first[0].0]).expect("taken back");
    git(w.root(), &["commit", "-q", "-m", "taken back"]).expect("and synced");
    w.decide(TGORKA, Decision::Approve);
    w.consume();
    let head = w.head();
    w.settle().await;
    assert_eq!(w.head(), head, "nothing applied");
    assert_eq!(w.read(MEMORY).expect("memory"), memory);
    for (path, _) in first[1..].iter().chain(&second) {
        assert!(w.read(path).is_some(), "{path} still pending");
    }
    assert!(!consolidate::any_decided(&w.homes), "nothing outstanding");

    w.night().await;
    assert_eq!(w.read(MEMORY).expect("memory"), memory);
    let all = waiting(&w.home);
    assert_eq!(all.len(), 2, "previewed again");
    let again = all
        .iter()
        .find(|one| one.standing == consolidate::Standing::Open)
        .expect("a new review");
    assert!(
        !again.args.proposals.contains(&taken),
        "{:?}",
        again.args.proposals
    );
}

/// R95C4-06: the claim lost while the night checks who the agent answers
/// to — the check stuck reading the drive's declaration — makes no room
/// once the check comes back, and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lease_lost_while_the_room_is_checked_makes_no_room() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let staged = thrice(
        "tgorka reviews on Fridays",
        MemoryTarget::Memory,
        Integrity::Agent,
    );
    let Some(w) = World::new(&[TGORKA], &staged).await else {
        return;
    };
    let decl = w.root().join("80-agents/_drive.toml");
    let text = std::fs::read(&decl).expect("declaration");
    std::fs::remove_file(&decl).expect("gone");
    assert!(Command::new("mkfifo")
        .arg(&decl)
        .status()
        .expect("mkfifo")
        .success());
    let lease = Arc::new(AtomicBool::new(true));
    let fence: consolidate::Fence = {
        let lease = Arc::clone(&lease);
        Arc::new(move || lease.load(Ordering::SeqCst))
    };
    // A writer opens only once the check reads the declaration: then the
    // claim is lost, then the check gets the declaration's bytes.
    let lost = {
        let (decl, lease) = (decl.clone(), Arc::clone(&lease));
        tokio::task::spawn_blocking(move || {
            use rustix::fs::{Mode, OFlags};
            let fd = loop {
                match rustix::fs::open(
                    &decl,
                    OFlags::WRONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                ) {
                    Ok(fd) => break fd,
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            };
            lease.store(false, Ordering::SeqCst);
            std::io::Write::write_all(&mut std::fs::File::from(fd), &text)
                .expect("the declaration's bytes");
        })
    };
    let made = Arc::new(AtomicBool::new(false));
    let head = w.head();
    let outcome = consolidate::run_home(
        &w.agentd.engine,
        &w.home,
        &w.homes,
        window(),
        now(),
        || {
            let made = Arc::clone(&made);
            async move {
                made.store(true, Ordering::SeqCst);
                OwnedRoomId::try_from("!review:example.org").map_err(|error| error.to_string())
            }
        },
        &fence,
    )
    .await
    .expect("runs");
    lost.await.expect("the check got its bytes");
    assert!(
        !made.load(Ordering::SeqCst),
        "no room after the claim was lost"
    );
    assert_eq!(outcome, Outcome::Fenced);
    assert_eq!(w.head(), head);
    assert!(!w.review_dir().exists());
}

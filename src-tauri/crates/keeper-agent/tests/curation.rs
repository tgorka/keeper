//! The weekly curator over agentd's own engine, a real checkout and a bare
//! remote (story 95.3): one sweep per drive through `curate::run_sweep`
//! and `curate::sweep_drive`, over a history whose author and committer
//! dates are set, every fixture read at one fixed instant.
//!
//! Its own test binary: `open_engine` arms a process-global tier, as in
//! `headless_engine.rs`. Skipped, not failed, on a machine with no `git`.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use keeper_agent::claims::ServerClock;
use keeper_agent::consolidate::{Fence, Home, Outcome};
use keeper_agent::curate::{plan_sweep, run_sweep, sweep_drive};
use keeper_agent::headless::{
    open_engine, AgentdEngine, HeadlessError, HeadlessSyncPlatform, SecretMap, SECRET_ENV_PREFIX,
};
use keeper_agent::zone::{read_zone, skills_of, AgentHome};
use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::agents::consolidate::{Verdict, VerdictFile};
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::memory::MemoryTarget;
use keeper_core::agents::proposal::{Op, Origin, Proposal, Target};
use keeper_core::bots::tools::ToolOutcome;
use keeper_sync::provenance::{authored_message, Provenance, SyncSource};
use keeper_sync::xdg::SecretStore;
use keeper_sync::{CommitPaths, MemoryTrailer, SyncError};
use matrix_sdk::ruma::OwnedUserId;
use ulid::Ulid;

const TGORKA: &str = "@tgorka:example.org";
const MEMORY: &str = "80-agents/nixi/MEMORY.md";

/// The instant every fixture is seeded relative to and swept at: one fixed
/// clock, never read again.
fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-04T05:00:00Z")
        .expect("time")
        .with_timezone(&Utc)
}

fn yes() -> Fence {
    Arc::new(|| true)
}

/// A server clock on a wall clock fixed at `wall`, the server `hours`
/// ahead of it (behind, negative): no real clock is read.
fn at_server(wall: DateTime<Utc>, hours: i64) -> ServerClock {
    let wall = u64::try_from(wall.timestamp_millis()).expect("after 1970");
    let clock = ServerClock::on_wall(Arc::new(std::sync::atomic::AtomicU64::new(wall)));
    clock.observe(wall.saturating_add_signed(hours * 3_600_000), wall, wall);
    clock
}

/// `git` in `dir`, its commits' author and committer dates `dates` (git's
/// internal format) when given.
fn git_at(dir: &Path, dates: Option<(&str, &str)>, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "seed")
        .env("GIT_AUTHOR_EMAIL", "seed@example.invalid")
        .env("GIT_COMMITTER_NAME", "seed")
        .env("GIT_COMMITTER_EMAIL", "seed@example.invalid");
    if let Some((author, committer)) = dates {
        command
            .env("GIT_AUTHOR_DATE", author)
            .env("GIT_COMMITTER_DATE", committer);
    }
    let out = command.args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    git_at(dir, None, args)
}

fn user(id: &str) -> OwnedUserId {
    OwnedUserId::try_from(id).expect("user")
}

/// A `SKILL.md` for `name`, its `metadata` map holding `meta`.
fn skill(name: &str, meta: &[(&str, &str)]) -> String {
    let mut metadata = String::new();
    if !meta.is_empty() {
        metadata.push_str("metadata:\n");
        for (key, value) in meta {
            metadata.push_str(&format!("  {key}: {value}\n"));
        }
    }
    format!("---\nname: {name}\ndescription: Does {name}.\n{metadata}---\n\nThe {name} steps.\n")
}

/// An agent's own skill, as the night lands it.
fn ours(name: &str) -> String {
    skill(name, &[("keeper_proposal", "01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z")])
}

/// An agent's skill the curator marked stale.
fn marked(name: &str) -> String {
    skill(
        name,
        &[
            ("keeper_proposal", "01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z"),
            ("keeper_stale", "2026-09-30"),
        ],
    )
}

fn skill_md(name: &str) -> String {
    format!("80-agents/_skills/{name}/SKILL.md")
}

/// A commit message as the curator's `commit_paths` writes it: keeper's
/// provenance block, then `Memory-Origin: curator@electra`.
fn curators(subject: &str) -> String {
    authored_message(
        subject,
        "",
        &Provenance::new(
            "tgdrive",
            "electra",
            "01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z",
            "electra",
            SyncSource::Bot,
        ),
        &[MemoryTrailer::MemoryOrigin("curator@electra".to_owned())],
    )
}

/// A pending proposal of nixi's from a session of `origin`, made at
/// `created`: its path and text.
fn proposal(seq: u128, origin: Origin, created: DateTime<Utc>) -> (String, String) {
    let id = Ulid::from_parts(
        u64::try_from(created.timestamp_millis()).expect("after 1970"),
        seq,
    );
    let text = Proposal {
        id,
        agent: "nixi".to_owned(),
        target: Target::Memory(MemoryTarget::Memory),
        op: Op::Add,
        matched: None,
        session: "60-sessions/active/s0".to_owned(),
        host: "electra".to_owned(),
        origin,
        label: Label {
            readers: Readers::Only(BTreeSet::from([user(TGORKA)])),
            integrity: Integrity::Owner,
            local_only: false,
        },
        created_at: created,
        body: "tgorka reviews on Fridays".to_owned(),
    }
    .render();
    (format!("80-agents/nixi/proposals/{id}.md"), text)
}

fn drive_toml() -> String {
    format!(
        "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\n"
    )
}

const AGENT_TOML: &str = "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"@nixi:example.org\"\nhuman = \"@tgorka:example.org\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[tools]\nallow = [\"skill_view\"]\ndrives = [\"tgdrive\"]\nskills = [\"listed\"]\n";

type Files = Vec<(String, Option<String>)>;

/// One commit of the seeded history: committed `days_ago` before the
/// fixture's instant (authored `authored` days before it when given), its
/// message, and each path's new text (`None` deletes it).
struct Seeded {
    days_ago: i64,
    authored: Option<i64>,
    message: String,
    files: Files,
}

fn at(days_ago: i64, message: &str, files: Files) -> Seeded {
    Seeded {
        days_ago,
        authored: None,
        message: message.to_owned(),
        files,
    }
}

/// The seed checkout the bare remote is pushed from, dated from `now`.
struct Seed<'a> {
    dir: &'a Path,
    now: DateTime<Utc>,
}

impl Seed<'_> {
    fn date(&self, days_ago: i64) -> String {
        format!(
            "{} +0000",
            (self.now - Duration::days(days_ago)).timestamp()
        )
    }

    /// `git <args>` dated `days_ago`.
    fn git(&self, days_ago: i64, args: &[&str]) -> String {
        let date = self.date(days_ago);
        git_at(self.dir, Some((&date, &date)), args).expect("git")
    }

    fn commit(&self, seeded: &Seeded) {
        for (rel, text) in &seeded.files {
            let path = self.dir.join(rel);
            match text {
                Some(text) => {
                    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                    std::fs::write(path, text).expect("write");
                }
                None => std::fs::remove_file(path).expect("delete"),
            }
        }
        let (author, committer) = (
            self.date(seeded.authored.unwrap_or(seeded.days_ago)),
            self.date(seeded.days_ago),
        );
        git(self.dir, &["add", "-A"]).expect("add");
        git_at(
            self.dir,
            Some((&author, &committer)),
            &["commit", "-q", "--allow-empty", "-m", &seeded.message],
        )
        .expect("commit");
    }
}

/// A drive checked out by agentd's engine, its history the base (100 days
/// before `now`: the zone, nixi's home, `MEMORY.md`) followed by what
/// `seed` commits.
struct World {
    _root: tempfile::TempDir,
    agentd: AgentdEngine,
    home: Home,
    agent: AgentHome,
    now: DateTime<Utc>,
}

impl World {
    async fn new(history: Vec<Seeded>) -> Option<World> {
        World::build(t0(), |seed| {
            for one in &history {
                seed.commit(one);
            }
        })
        .await
    }

    async fn build(now: DateTime<Utc>, seed: impl FnOnce(&Seed<'_>)) -> Option<World> {
        let root = tempfile::tempdir().expect("tempdir");
        let bare = root.path().join("tgdrive.git");
        std::fs::create_dir_all(&bare).ok()?;
        git(&bare, &["init", "-q", "--bare", "-b", "main"])?;
        let dir = root.path().join("seed");
        std::fs::create_dir_all(&dir).expect("seed");
        git(&dir, &["init", "-q", "-b", "main"])?;
        let seeding = Seed { dir: &dir, now };
        seeding.commit(&at(
            100,
            "seed",
            vec![
                ("80-agents/_drive.toml".to_owned(), Some(drive_toml())),
                (
                    "80-agents/nixi/agent.toml".to_owned(),
                    Some(AGENT_TOML.to_owned()),
                ),
                (
                    "80-agents/nixi/proposals/.keep".to_owned(),
                    Some(String::new()),
                ),
                (
                    MEMORY.to_owned(),
                    Some("---\ntype: memory\n---\nThe drive is tgdrive.\n".to_owned()),
                ),
                (
                    "60-sessions/README.md".to_owned(),
                    Some("# sessions\n".to_owned()),
                ),
            ],
        ));
        seed(&seeding);
        git(&dir, &["push", "-q", &bare.to_string_lossy(), "main"]).expect("push");

        let data = root.path().join("data");
        let text = format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\nalways_on = true\n\n[homeserver]\nurl = \"https://matrix.example.org\"\n\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\n\n[[agents]]\ndrive = \"tgdrive\"\nids = [\"nixi\"]\n",
            bare.display(),
        );
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
        let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
        let zone = read_zone("tgdrive", &profile, Some(&decl));
        let (_, read) = zone
            .homes
            .into_iter()
            .find(|(folder, _)| folder == "nixi")
            .expect("nixi's home");
        let agent = read.expect("nixi reads");
        let home = Home::of(&profile, &agent, "electra").expect("home");
        Some(World {
            _root: root,
            agentd,
            home,
            agent,
            now,
        })
    }

    fn root(&self) -> &Path {
        &self.home.root
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root().join(rel)).ok()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    fn git(&self, args: &[&str]) -> String {
        git(self.root(), args).expect("git")
    }

    /// A person's commit in the checkout, at the fixture's instant.
    fn commit_here(&self, message: &str) {
        let date = format!("{} +0000", self.now.timestamp());
        self.git(&["add", "-A"]);
        git_at(
            self.root(),
            Some((&date, &date)),
            &["commit", "-q", "-m", message],
        )
        .expect("commit");
    }

    fn commits(&self) -> usize {
        self.git(&["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .expect("n")
    }

    async fn sweep(&self) -> Outcome {
        run_sweep(
            &self.agentd.engine,
            std::slice::from_ref(&self.home),
            self.now,
            &yes(),
        )
        .await
        .expect("the sweep runs")
    }

    fn plan(&self) -> keeper_agent::curate::Swept {
        plan_sweep(std::slice::from_ref(&self.home), self.now)
            .expect("plans")
            .expect("a sweep")
    }

    fn archived(&self, name: &str) -> bool {
        self.root()
            .join(format!("80-agents/_skills/.archive/{name}/SKILL.md"))
            .is_file()
            && !self
                .root()
                .join(format!("80-agents/_skills/{name}"))
                .exists()
    }
}

fn add(rel: String, text: String) -> (String, Option<String>) {
    (rel, Some(text))
}

/// 95.3 acceptance 2: of seven skills unchanged for a month or more, the
/// curator archives only the agents' own unadopted, unpinned, unnamed one
/// — a `_workflows/` file naming `tidy-up` does not name `tidy` — and
/// leaves a person's, an adopted, a pinned, a listed and a workflow-named
/// one byte for byte.
#[tokio::test(flavor = "multi_thread")]
async fn the_curator_touches_only_what_it_made() {
    let pinned = skill(
        "pinned",
        &[
            ("keeper_proposal", "01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z"),
            ("keeper_pinned", "\"true\""),
        ],
    );
    let Some(w) = World::new(vec![
        at(
            90,
            "skills",
            vec![
                add(skill_md("persons"), skill("persons", &[])),
                add(skill_md("adopted"), ours("adopted")),
                add(skill_md("pinned"), pinned.clone()),
                add(skill_md("listed"), ours("listed")),
                add(skill_md("flowed"), ours("flowed")),
                add(
                    "80-agents/_workflows/weekly/workflow.md".to_owned(),
                    "# Weekly\n\nLoad flowed, then run tidy-up.\n".to_owned(),
                ),
            ],
        ),
        at(
            90,
            "a person adopts it",
            vec![add(skill_md("adopted"), skill("adopted", &[]))],
        ),
        at(31, "the night", vec![add(skill_md("tidy"), ours("tidy"))]),
    ])
    .await
    else {
        return;
    };
    let before: Vec<(&str, String)> = ["persons", "adopted", "pinned", "listed", "flowed"]
        .into_iter()
        .map(|name| (name, w.read(&skill_md(name)).expect("skill")))
        .collect();
    assert!(matches!(w.sweep().await, Outcome::Committed(Some(_))));
    assert!(w.archived("tidy"));
    for (name, text) in before {
        assert_eq!(
            w.read(&skill_md(name)).as_deref(),
            Some(text.as_str()),
            "{name}"
        );
    }
}

/// R95U3-01: a month-old agent's skill whose ownership metadata keeper
/// cannot read — `keeper_pinned` said twice, the last saying `"true"`, or
/// the pin in a second `metadata` block — stays byte for byte and is not
/// archived by a real sweep; the readable one beside them is.
#[tokio::test(flavor = "multi_thread")]
async fn ambiguous_ownership_metadata_is_never_archived() {
    let pinned_twice = skill(
        "pinned-twice",
        &[
            ("keeper_proposal", "01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z"),
            ("keeper_pinned", "\"false\""),
            ("keeper_pinned", "\"true\""),
        ],
    );
    let two_blocks = "---\nname: two-blocks\ndescription: Does two-blocks.\nmetadata:\n  keeper_proposal: 01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z\nmetadata:\n  keeper_pinned: \"true\"\n---\n\nThe two-blocks steps.\n".to_owned();
    let Some(w) = World::new(vec![at(
        31,
        "the night",
        vec![
            add(skill_md("pinned-twice"), pinned_twice.clone()),
            add(skill_md("two-blocks"), two_blocks.clone()),
            add(skill_md("tidy"), ours("tidy")),
        ],
    )])
    .await
    else {
        return;
    };
    assert!(matches!(w.sweep().await, Outcome::Committed(Some(_))));
    assert!(w.archived("tidy"));
    for (name, text) in [("pinned-twice", pinned_twice), ("two-blocks", two_blocks)] {
        assert_eq!(w.read(&skill_md(name)), Some(text), "{name}");
        assert!(
            !w.root()
                .join(format!("80-agents/_skills/.archive/{name}"))
                .exists(),
            "{name}"
        );
    }
}

/// 95.3 acceptance 3 and 1's last row: the clock is git's alone. An
/// agent's skill applied 31 days ago is archived, one patched 3 days ago
/// stays as it is, one a person adopted 40 days ago is untouched; one the
/// curator marked stale 6 days ago is not made active by that commit, and
/// one patched since its mark — the patch kept the mark — is active again,
/// its mark cleared. A `.keeper/agents.db` changes nothing of the plan.
#[tokio::test(flavor = "multi_thread")]
async fn an_agents_skill_ages_from_its_last_change() {
    let Some(w) = World::new(vec![
        at(
            50,
            "the nights",
            vec![
                add(skill_md("patched"), ours("patched")),
                add(skill_md("adopted"), ours("adopted")),
            ],
        ),
        at(
            40,
            "a person adopts it",
            vec![add(skill_md("adopted"), skill("adopted", &[]))],
        ),
        at(
            31,
            "the night",
            vec![add(skill_md("applied"), ours("applied"))],
        ),
        at(
            20,
            "the nights",
            vec![
                add(skill_md("resting"), ours("resting")),
                add(skill_md("revived"), ours("revived")),
            ],
        ),
        at(
            6,
            &curators("skills: tgdrive — 2 stale"),
            vec![
                add(skill_md("resting"), marked("resting")),
                add(skill_md("revived"), marked("revived")),
            ],
        ),
        at(
            3,
            "memory: nixi — 1 promoted",
            vec![add(
                skill_md("patched"),
                ours("patched").replace("steps.", "better steps."),
            )],
        ),
        at(
            2,
            "memory: nixi — 1 promoted",
            vec![add(
                skill_md("revived"),
                marked("revived").replace("steps.", "better steps."),
            )],
        ),
    ])
    .await
    else {
        return;
    };
    let plan = w.plan();
    let db = w.root().join(".keeper/agents.db");
    std::fs::create_dir_all(db.parent().expect("parent")).expect("mkdir");
    std::fs::write(&db, b"not a database").expect("db");
    let with_db = w.plan();
    std::fs::remove_file(&db).expect("delete");
    assert_eq!(plan.plan, with_db.plan, "no log, no index: git alone");

    let archived: Vec<&str> = plan.plan.archives.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(archived, ["_skills/applied"]);
    assert_eq!(plan.plan.reactivated, ["revived"]);
    assert!(plan.plan.stale.is_empty(), "{:?}", plan.plan);

    let patched = w.read(&skill_md("patched")).expect("patched");
    let adopted = w.read(&skill_md("adopted")).expect("adopted");
    let resting = w.read(&skill_md("resting")).expect("resting");
    assert!(matches!(w.sweep().await, Outcome::Committed(Some(_))));
    assert!(w.archived("applied"));
    assert_eq!(w.read(&skill_md("patched")), Some(patched));
    assert_eq!(w.read(&skill_md("adopted")), Some(adopted));
    assert_eq!(w.read(&skill_md("resting")), Some(resting));
    assert_eq!(
        w.read(&skill_md("revived")),
        Some(ours("revived").replace("steps.", "better steps.")),
        "the mark cleared, every other byte kept"
    );
}

/// R95U-09: the curator's own commits are known by keeper's own trailer
/// block, not by what a message says, wherever they were merged from:
/// - a person's patch 3 days ago whose body quotes the curator's line is a
///   change, so its skill stays;
/// - a curator's stale mark merged in 4 days ago is no change, so its skill
///   is as old as its creation, 40 days, and is archived;
/// - a skill whose 32 newest changes are all the curator's has no age the
///   walk can establish, and stays;
/// - the age is the committer's date: authored 40 days ago, committed 3
///   days ago (rebased), the skill stays.
#[tokio::test(flavor = "multi_thread")]
async fn the_curators_own_commits_are_known_by_their_trailer() {
    let Some(w) = World::build(t0(), |seed| {
        seed.commit(&at(
            90,
            "the nights",
            vec![
                add(skill_md("quoted"), ours("quoted")),
                add(skill_md("busy"), ours("busy")),
            ],
        ));
        seed.commit(&at(
            40,
            "the nights",
            vec![add(skill_md("merged"), ours("merged"))],
        ));
        seed.commit(&Seeded {
            days_ago: 3,
            authored: Some(40),
            message: "the night, rebased".to_owned(),
            files: vec![add(skill_md("rebased"), ours("rebased"))],
        });
        seed.commit(&at(
            3,
            "fix quoted\n\nThe curator wrote\nMemory-Origin: curator@electra\nbefore; this is mine.",
            vec![add(
                skill_md("quoted"),
                ours("quoted").replace("steps.", "my steps."),
            )],
        ));
        for n in 0..32 {
            let text = if n % 2 == 0 { marked("busy") } else { ours("busy") };
            seed.commit(&at(
                30 - n / 4,
                &curators("skills: tgdrive"),
                vec![add(skill_md("busy"), text)],
            ));
        }
        seed.git(5, &["checkout", "-q", "-b", "side"]);
        seed.commit(&at(
            5,
            &curators("skills: tgdrive — 1 stale"),
            vec![add(skill_md("merged"), marked("merged"))],
        ));
        seed.git(4, &["checkout", "-q", "main"]);
        seed.git(4, &["merge", "-q", "--no-ff", "-m", "Merge branch 'side'", "side"]);
    })
    .await
    else {
        return;
    };
    let plan = w.plan();
    let archived: Vec<&str> = plan.plan.archives.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(archived, ["_skills/merged"], "{:?}", plan.plan);
    assert!(plan.plan.marks.is_empty(), "{:?}", plan.plan);
}

/// R95U2-01, through the real age calculation: a person's patch 3 days
/// ago whose last paragraph only looks like the curator's — keeper's lines
/// mixed with prose, a partial block, the block said twice, an origin that
/// names no host — is a change, so its skill, 40 days old otherwise,
/// stays; the curator's own stale mark 3 days ago is not, so its skill is
/// archived.
#[tokio::test(flavor = "multi_thread")]
async fn only_the_curators_whole_block_is_the_curators() {
    let genuine = curators("skills: tgdrive — 1 stale");
    let block = genuine.rsplit_once("\n\n").expect("a block").1.to_owned();
    let lookalikes = [
        (
            "prose",
            "fix prose\n\nQuoted example:\nKeeper-Profile: tgdrive\nMemory-Origin: curator@electra\nThis is my patch.\n".to_owned(),
        ),
        (
            "partial",
            "fix partial\n\nKeeper-Profile: tgdrive\nMemory-Origin: curator@electra\n".to_owned(),
        ),
        ("twice", format!("fix twice\n\n{block}{block}")),
        (
            "nohost",
            genuine.replace("curator@electra", "curator@not a host"),
        ),
    ];
    let Some(w) = World::build(t0(), |seed| {
        let mut created = vec![add(skill_md("genuine"), ours("genuine"))];
        created.extend(
            lookalikes
                .iter()
                .map(|(name, _)| add(skill_md(name), ours(name))),
        );
        seed.commit(&at(40, "the nights", created));
        seed.commit(&at(
            3,
            &genuine,
            vec![add(skill_md("genuine"), marked("genuine"))],
        ));
        for (name, message) in &lookalikes {
            seed.commit(&at(
                3,
                message,
                vec![add(
                    skill_md(name),
                    ours(name).replace("steps.", "my steps."),
                )],
            ));
        }
    })
    .await
    else {
        return;
    };
    let plan = w.plan();
    let archived: Vec<&str> = plan.plan.archives.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(archived, ["_skills/genuine"], "{:?}", plan.plan);
    assert!(plan.plan.marks.is_empty(), "{:?}", plan.plan);
}

/// R95U2-02, through a sweep: a patch made 3 days ago on a side branch
/// whose merge 2 days ago kept the skill as it was is no change of the
/// skill's — 40 days old, it is archived as it was kept, the discarded
/// patch nowhere.
#[tokio::test(flavor = "multi_thread")]
async fn a_discarded_patch_does_not_keep_a_skill() {
    let Some(w) = World::build(t0(), |seed| {
        seed.commit(&at(
            40,
            "the night",
            vec![add(skill_md("tidy"), ours("tidy"))],
        ));
        seed.git(3, &["checkout", "-q", "-b", "side"]);
        seed.commit(&at(
            3,
            "fix tidy",
            vec![add(
                skill_md("tidy"),
                ours("tidy").replace("steps.", "my steps."),
            )],
        ));
        seed.git(2, &["checkout", "-q", "main"]);
        seed.git(
            2,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-s",
                "ours",
                "-m",
                "Merge branch 'side'",
                "side",
            ],
        );
    })
    .await
    else {
        return;
    };
    assert!(matches!(w.sweep().await, Outcome::Committed(Some(_))));
    assert!(w.archived("tidy"));
    assert_eq!(
        w.read("80-agents/_skills/.archive/tidy/SKILL.md"),
        Some(ours("tidy"))
    );
}

/// 95.3 acceptance 4: an archived skill's every file — `scripts/` and
/// `references/` included, a binary and an executable among them — is
/// under `_skills/.archive/<name>/` with its mode, and git reads the
/// sweep's commit as one 100 % rename per file, nothing deleted.
#[tokio::test(flavor = "multi_thread")]
async fn archiving_keeps_every_byte() {
    let files = [
        ("SKILL.md", ours("tidy")),
        ("scripts/run.sh", "#!/bin/sh\necho tidy\n".to_owned()),
        ("references/inbox.md", "# The inbox\n\nRules.\n".to_owned()),
    ];
    let binary: Vec<u8> = (0u8..=255).chain([0, 0, 0xff]).collect();
    let Some(w) = World::build(t0(), |seed| {
        for (rel, text) in &files {
            let path = seed.dir.join(format!("80-agents/_skills/tidy/{rel}"));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        }
        std::fs::write(seed.dir.join("80-agents/_skills/tidy/icon.bin"), &binary).expect("bin");
        git(seed.dir, &["add", "-A"]).expect("add");
        git(
            seed.dir,
            &[
                "update-index",
                "--chmod=+x",
                "80-agents/_skills/tidy/scripts/run.sh",
            ],
        )
        .expect("chmod");
        seed.git(31, &["commit", "-q", "-m", "the night"]);
    })
    .await
    else {
        return;
    };
    assert!(matches!(w.sweep().await, Outcome::Committed(Some(_))));
    for (rel, text) in &files {
        assert_eq!(
            w.read(&format!("80-agents/_skills/.archive/tidy/{rel}"))
                .as_deref(),
            Some(text.as_str()),
            "{rel}"
        );
    }
    assert_eq!(
        std::fs::read(w.root().join("80-agents/_skills/.archive/tidy/icon.bin")).expect("bin"),
        binary
    );
    assert!(w
        .git(&[
            "ls-tree",
            "HEAD",
            "80-agents/_skills/.archive/tidy/scripts/run.sh"
        ])
        .starts_with("100755"));
    let mut renames: Vec<String> = w
        .git(&["diff", "--find-renames", "--name-status", "HEAD~1", "HEAD"])
        .lines()
        .map(str::to_owned)
        .collect();
    renames.sort();
    let mut want: Vec<String> = files
        .iter()
        .map(|(rel, _)| rel.to_string())
        .chain(["icon.bin".to_owned()])
        .map(|rel| {
            format!("R100\t80-agents/_skills/tidy/{rel}\t80-agents/_skills/.archive/tidy/{rel}")
        })
        .collect();
    want.sort();
    assert_eq!(renames, want);
}

/// R95U-07: a skill folder moves only whole as the sweep read it. A support
/// file edited, added or removed on the disk and not committed keeps the
/// skill where it is; one committed after the plan, or anything at the
/// destination on the disk, makes the commit write nothing; a destination
/// already committed leaves the skill where it is.
#[tokio::test(flavor = "multi_thread")]
async fn archiving_guards_the_whole_tree() {
    let Some(w) = World::new(vec![at(
        31,
        "the night",
        vec![
            add(skill_md("tidy"), ours("tidy")),
            add(
                "80-agents/_skills/tidy/scripts/run.sh".to_owned(),
                "echo tidy\n".to_owned(),
            ),
        ],
    )])
    .await
    else {
        return;
    };
    let script = "80-agents/_skills/tidy/scripts/run.sh";
    let stays = |w: &World, context: &str| {
        assert!(!w.archived("tidy"), "{context}");
        assert!(w.read(&skill_md("tidy")).is_some(), "{context}");
    };
    let head = w.git(&["rev-parse", "HEAD"]);

    w.write(script, "echo mine\n");
    assert_eq!(w.sweep().await, Outcome::Committed(None));
    stays(&w, "an edited support file");
    w.git(&["checkout", "--", script]);

    w.write("80-agents/_skills/tidy/references/new.md", "mine\n");
    assert_eq!(w.sweep().await, Outcome::Committed(None));
    stays(&w, "a new support file");
    std::fs::remove_dir_all(w.root().join("80-agents/_skills/tidy/references")).expect("rm");

    std::fs::remove_file(w.root().join(script)).expect("rm");
    assert_eq!(w.sweep().await, Outcome::Committed(None));
    stays(&w, "a removed support file");
    w.git(&["checkout", "--", script]);

    w.write("80-agents/_skills/.archive/tidy/mine.md", "mine\n");
    assert!(matches!(w.sweep().await, Outcome::Skipped { .. }));
    stays(&w, "an occupied destination");
    assert_eq!(
        w.read("80-agents/_skills/.archive/tidy/mine.md").as_deref(),
        Some("mine\n")
    );
    std::fs::remove_dir_all(w.root().join("80-agents/_skills/.archive")).expect("rm");
    assert_eq!(w.git(&["rev-parse", "HEAD"]), head, "nothing committed");

    w.write("80-agents/_skills/.archive/tidy/SKILL.md", &ours("tidy"));
    w.commit_here("an archived copy already");
    let swept = w.plan();
    assert!(swept.plan.archives.is_empty(), "{:?}", swept.plan);
    w.git(&["rm", "-q", "-r", "80-agents/_skills/.archive"]);
    w.commit_here("the archived copy goes");

    let swept = w.plan();
    assert_eq!(swept.plan.archives.len(), 1);

    // R95U2-03: a file put on the disk after the plan, never committed,
    // holds the move: nothing is published, the folder stays whole with
    // the person's new file in it.
    let before = w.git(&["rev-parse", "HEAD"]);
    let new = "80-agents/_skills/tidy/references/new.md";
    w.write(new, "mine\n");
    let done = w
        .agentd
        .engine
        .commit_paths(&w.home.profile_id, &swept.request, yes())
        .await
        .expect("asks");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: new.to_owned()
        }
    );
    assert_eq!(w.git(&["rev-parse", "HEAD"]), before, "nothing published");
    assert_eq!(w.read(new).as_deref(), Some("mine\n"));
    assert_eq!(w.read(script).as_deref(), Some("echo tidy\n"));
    stays(&w, "a file put on the disk after the plan");
    std::fs::remove_dir_all(w.root().join("80-agents/_skills/tidy/references")).expect("rm");

    w.write("80-agents/_skills/tidy/references/later.md", "later\n");
    w.commit_here("a reference added after the plan");
    let after = w.git(&["rev-parse", "HEAD"]);
    let done = w
        .agentd
        .engine
        .commit_paths(&w.home.profile_id, &swept.request, yes())
        .await
        .expect("asks");
    assert_eq!(
        done,
        CommitPaths::Guarded {
            path: "80-agents/_skills/tidy/references/later.md".to_owned()
        }
    );
    assert_eq!(w.git(&["rev-parse", "HEAD"]), after);
    stays(&w, "a file committed after the plan");
}

/// The skill `tidy`, 31 days old and named nowhere the sweep can read,
/// stays where it is after a sweep over `extra` (`case` labels it).
async fn tidy_stays(extra: impl FnOnce(&Seed<'_>), case: &str) {
    let Some(w) = World::build(t0(), |seed| {
        seed.commit(&at(
            31,
            "the night",
            vec![add(skill_md("tidy"), ours("tidy"))],
        ));
        extra(seed);
    })
    .await
    else {
        return;
    };
    let swept = w.plan();
    assert!(swept.plan.archives.is_empty(), "{case}: {:?}", swept.plan);
    assert_eq!(w.sweep().await, Outcome::Committed(None), "{case}");
    assert!(!w.archived("tidy"), "{case}");
}

/// R95U-04 and R95U-06: what names a skill is read from git's objects of
/// one commit, and a read that cannot be whole archives nothing — more
/// workflow files than the cap, a workflow larger than its limit (this one
/// names `tidy` past it), a workflow that is a link out of the drive, an
/// agent's `agent.toml` that does not parse.
#[tokio::test(flavor = "multi_thread")]
async fn an_incomplete_protection_read_archives_nothing() {
    tidy_stays(
        |seed| {
            for n in 0..=2_000 {
                let path = seed.dir.join(format!("80-agents/_workflows/many/{n}.md"));
                std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                std::fs::write(path, "x\n").expect("write");
            }
            seed.git(20, &["add", "-A"]);
            seed.git(20, &["commit", "-q", "-m", "many"]);
        },
        "more than 2000 files",
    )
    .await;
    tidy_stays(
        |seed| {
            let mut big = "padding\n".repeat(40_000);
            big.push_str("then run tidy\n");
            seed.commit(&at(
                20,
                "big",
                vec![add("80-agents/_workflows/big.md".to_owned(), big)],
            ));
        },
        "larger than",
    )
    .await;
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("flow.md"), "nothing here\n").expect("outside");
    let target = outside.path().to_path_buf();
    tidy_stays(
        move |seed| {
            std::fs::create_dir_all(seed.dir.join("80-agents")).expect("zone");
            std::os::unix::fs::symlink(&target, seed.dir.join("80-agents/_workflows"))
                .expect("link");
            seed.git(20, &["add", "-A"]);
            seed.git(20, &["commit", "-q", "-m", "a linked folder"]);
        },
        "not a plain file",
    )
    .await;
    tidy_stays(
        |seed| {
            seed.commit(&at(
                20,
                "otto",
                vec![add(
                    "80-agents/otto/agent.toml".to_owned(),
                    "not toml at all [".to_owned(),
                )],
            ));
        },
        "does not parse",
    )
    .await;
}

/// R95U-05 and R95U-06: what protects a skill is read at the one commit
/// and guarded through the publication. A reference removed on the disk,
/// not committed, still protects (and a `_workflows` swapped on the disk
/// for a link out of the drive is such a change: nothing out there is
/// read); one committed after the plan — in a workflow or in any agent's
/// skills, one this host serves or not — makes the commit write nothing; a drive declaration changed
/// since this host admitted it holds the sweep.
#[tokio::test(flavor = "multi_thread")]
async fn a_reference_the_sweep_cannot_see_protects() {
    let flow = "80-agents/_workflows/weekly.md";
    let otto = "80-agents/otto/agent.toml";
    let otto_toml = AGENT_TOML
        .replace("\"nixi\"", "\"otto\"")
        .replace("Nixi", "Otto")
        .replace("@nixi:", "@otto:");
    let Some(w) = World::new(vec![at(
        31,
        "the night",
        vec![
            add(skill_md("tidy"), ours("tidy")),
            add(flow.to_owned(), "# Weekly\n\nRun tidy.\n".to_owned()),
            add(otto.to_owned(), otto_toml.clone()),
        ],
    )])
    .await
    else {
        return;
    };
    w.write(flow, "# Weekly\n\nNothing.\n");
    let swept = w.plan();
    assert!(swept.plan.archives.is_empty(), "{:?}", swept.plan);
    w.git(&["checkout", "--", flow]);

    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("weekly.md"), "Nothing.\n").expect("outside");
    let flows = w.root().join("80-agents/_workflows");
    std::fs::rename(&flows, w.root().join("flows-aside")).expect("aside");
    std::os::unix::fs::symlink(outside.path(), &flows).expect("link");
    assert!(w.plan().plan.archives.is_empty());
    std::fs::remove_file(&flows).expect("unlink");
    std::fs::rename(w.root().join("flows-aside"), &flows).expect("back");

    // Not named at the plan's commit; named by a commit after it.
    w.write(flow, "# Weekly\n\nNothing.\n");
    w.commit_here("tidy is not used");
    let swept = w.plan();
    assert_eq!(swept.plan.archives.len(), 1);
    w.write(flow, "# Weekly\n\nRun tidy again.\n");
    w.commit_here("tidy is used again");
    assert_eq!(
        w.agentd
            .engine
            .commit_paths(&w.home.profile_id, &swept.request, yes())
            .await
            .expect("asks"),
        CommitPaths::Guarded {
            path: flow.to_owned()
        }
    );
    assert!(!w.archived("tidy"));

    w.write(flow, "# Weekly\n\nNothing.\n");
    w.commit_here("tidy is not used");
    let swept = w.plan();
    assert_eq!(swept.plan.archives.len(), 1);
    w.write(
        otto,
        &otto_toml.replace("skills = [\"listed\"]", "skills = [\"listed\", \"tidy\"]"),
    );
    w.commit_here("otto uses tidy");
    assert_eq!(
        w.agentd
            .engine
            .commit_paths(&w.home.profile_id, &swept.request, yes())
            .await
            .expect("asks"),
        CommitPaths::Guarded {
            path: otto.to_owned()
        }
    );
    assert!(!w.archived("tidy"));

    w.write(
        "80-agents/_drive.toml",
        &drive_toml().replace(
            &format!("readers = [\"{TGORKA}\"]"),
            &format!("readers = [\"{TGORKA}\", \"@marta:example.org\"]"),
        ),
    );
    w.commit_here("marta reads the drive");
    let held = plan_sweep(std::slice::from_ref(&w.home), w.now).expect_err("held");
    assert!(held.contains("_drive.toml"), "{held}");
}

/// R95U-08: a gate proposal's expiry never writes over a person's file. A
/// verdict already committed where the expiry would put it keeps the
/// proposal pending; a file on the disk at its `done/` place makes the
/// commit write nothing, its bytes kept.
#[tokio::test(flavor = "multi_thread")]
async fn an_expiry_leaves_a_persons_files_alone() {
    let (due, due_text) = proposal(2, Origin::Gate, t0() - Duration::days(30));
    let done = due.replace("/proposals/", "/proposals/done/");
    let verdict = done.replace(".md", ".verdict.toml");
    let Some(w) = World::new(vec![at(20, "the night", vec![add(due.clone(), due_text)])]).await
    else {
        return;
    };
    w.write(&done, "mine\n");
    assert!(matches!(w.sweep().await, Outcome::Skipped { .. }));
    assert_eq!(w.read(&done).as_deref(), Some("mine\n"));
    assert!(w.read(&due).is_some(), "still pending");
    assert!(w.read(&verdict).is_none());

    std::fs::remove_file(w.root().join(&done)).expect("rm");
    w.write(&verdict, "verdict = \"mine\"\n");
    w.commit_here("my verdict");
    assert_eq!(w.sweep().await, Outcome::Committed(None));
    assert_eq!(w.read(&verdict).as_deref(), Some("verdict = \"mine\"\n"));
    assert!(w.read(&due).is_some(), "still pending");
}

/// 95.3 acceptance 6 and 8 at the agent: one sweep marks a skill stale,
/// archives another and expires a gate proposal exactly 30 days old in ONE
/// commit carrying `Memory-Origin: curator@electra` — its verdict
/// `expired` beside it in `proposals/done/`, the one a second younger
/// still pending, MEMORY.md untouched — and the next sweep finds nothing
/// to do.
#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_is_one_commit() {
    let due_at = t0() - Duration::days(30);
    let (young, young_text) = proposal(1, Origin::Gate, due_at + Duration::seconds(1));
    let (due, due_text) = proposal(2, Origin::Gate, due_at);
    let Some(w) = World::new(vec![
        at(31, "the night", vec![add(skill_md("tidy"), ours("tidy"))]),
        at(
            20,
            "the night",
            vec![
                add(skill_md("sort"), ours("sort")),
                add(young.clone(), young_text),
                add(due.clone(), due_text),
            ],
        ),
    ])
    .await
    else {
        return;
    };
    let memory = w.read(MEMORY);
    let before = w.commits();
    assert!(matches!(w.sweep().await, Outcome::Committed(Some(_))));
    assert_eq!(w.commits(), before + 1, "one commit");
    let message = w.git(&["log", "-1", "--format=%B"]);
    assert_eq!(
        MemoryTrailer::of_message(&message),
        [MemoryTrailer::MemoryOrigin("curator@electra".to_owned())]
    );
    let changed = w.git(&["show", "--name-status", "--format=", "HEAD"]);
    assert!(w.archived("tidy"));
    assert!(
        changed.contains("80-agents/_skills/sort/SKILL.md"),
        "{changed}"
    );
    assert_eq!(
        keeper_core::agents::skills::metadata_value(
            &w.read(&skill_md("sort")).expect("sort"),
            "keeper_stale"
        ),
        Some("2026-10-04".to_owned())
    );
    let done = due.replace("/proposals/", "/proposals/done/");
    assert!(changed.contains(&done), "{changed}");
    let verdict = VerdictFile::parse(
        &w.read(&done.replace(".md", ".verdict.toml"))
            .expect("verdict"),
    )
    .expect("parses");
    assert_eq!(verdict.verdict, Verdict::Expired);
    assert_eq!(verdict.decided_by, "curator@electra");
    assert!(w.read(&due).is_none());
    assert!(w.read(&young).is_some(), "a second short: still pending");
    assert_eq!(w.read(MEMORY), memory, "never read into memory");

    assert_eq!(w.sweep().await, Outcome::Committed(None));
    assert_eq!(w.commits(), before + 1);
}

/// A fence that says yes `allowed` times, then no.
fn fence_after(allowed: usize) -> Fence {
    let asked = std::sync::atomic::AtomicUsize::new(0);
    Arc::new(move || asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < allowed)
}

/// R95U-01: a sweep whose claim is lost writes nothing, whenever it is
/// lost — after the pull, once the commit's lane is held, or right before
/// its publication — and says so. The drive lives seventy years ahead of
/// any machine this runs on (git dates stop at 2099), so its skill is due
/// — and the fence asked after the pull — only at the clock the sweep is
/// handed.
#[tokio::test(flavor = "multi_thread")]
async fn a_lost_claim_writes_nothing_at_any_point() {
    let ahead = t0() + Duration::days(70 * 365);
    let Some(w) = World::build(ahead, |seed| {
        seed.commit(&at(
            31,
            "the night",
            vec![add(skill_md("tidy"), ours("tidy"))],
        ));
    })
    .await
    else {
        return;
    };
    let before = w.commits();
    let clock = at_server(w.now, 0);
    assert_eq!(
        sweep_drive(
            &w.agentd.engine,
            std::slice::from_ref(&w.home),
            &clock,
            &fence_after(0)
        )
        .await
        .expect("runs"),
        Outcome::Fenced,
        "after the pull"
    );
    for allowed in [0, 1] {
        let outcome = run_sweep(
            &w.agentd.engine,
            std::slice::from_ref(&w.home),
            w.now,
            &fence_after(allowed),
        )
        .await
        .expect("runs");
        assert_eq!(outcome, Outcome::Fenced, "after {allowed} yes");
        assert_eq!(w.commits(), before);
        assert_eq!(w.git(&["status", "--porcelain"]), "");
        assert!(!w.archived("tidy"));
    }
    assert!(matches!(
        run_sweep(
            &w.agentd.engine,
            std::slice::from_ref(&w.home),
            w.now,
            &fence_after(2)
        )
        .await
        .expect("runs"),
        Outcome::Committed(Some(_))
    ));
    assert!(w.archived("tidy"));
}

/// R95U-10: the sweep runs at the server's time, not this machine's: a
/// gate proposal 30 days old by this machine's clock but not by the
/// server's, two hours behind, waits; one 30 days old only by the server's
/// clock, two hours ahead, expires. Both clocks are driven by hand.
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_runs_at_the_servers_time() {
    let wall = t0();
    let (ahead, ahead_text) = proposal(
        1,
        Origin::Gate,
        wall - Duration::days(30) + Duration::hours(1),
    );
    let (behind, behind_text) = proposal(
        2,
        Origin::Gate,
        wall - Duration::days(30) - Duration::hours(1),
    );
    let Some(w) = World::build(wall, |seed| {
        seed.commit(&at(
            20,
            "the night",
            vec![
                add(ahead.clone(), ahead_text),
                add(behind.clone(), behind_text),
            ],
        ));
    })
    .await
    else {
        return;
    };
    let skewed = |hours: i64| at_server(wall, hours);
    let homes = std::slice::from_ref(&w.home);
    assert_eq!(
        sweep_drive(&w.agentd.engine, homes, &skewed(-2), &yes())
            .await
            .expect("runs"),
        Outcome::Committed(None),
        "two hours behind: neither is 30 days old"
    );
    assert!(w.read(&behind).is_some() && w.read(&ahead).is_some());
    assert!(matches!(
        sweep_drive(&w.agentd.engine, homes, &skewed(2), &yes())
            .await
            .expect("runs"),
        Outcome::Committed(Some(_))
    ));
    assert!(w.read(&behind).is_none() && w.read(&ahead).is_none());
}

/// 95.3 acceptance 7: a skill under `_skills/.archive/` — one a person
/// wrote, which would be offered anywhere else — is neither listed nor
/// offered, and `skill_view` refuses it, reading no file.
#[tokio::test(flavor = "multi_thread")]
async fn archived_skills_are_not_offered() {
    let Some(w) = World::new(vec![at(
        31,
        "skills",
        vec![
            add(skill_md("kept"), skill("kept", &[])),
            add(skill_md("listed"), skill("listed", &[])),
            // The archive folder itself is never read as a skill, whatever
            // it holds.
            add(
                "80-agents/_skills/.archive/SKILL.md".to_owned(),
                skill("archive", &[]),
            ),
            add(
                "80-agents/_skills/.archive/listed-old/SKILL.md".to_owned(),
                skill("listed-old", &[]),
            ),
            add(
                "80-agents/_skills/.archive/listed/SKILL.md".to_owned(),
                skill("listed", &[]),
            ),
        ],
    )])
    .await
    else {
        return;
    };
    let mut agent = w.agent.clone();
    agent.config.skills = vec!["*".to_owned()];
    let index = skills_of(&agent);
    let offered: Vec<&str> = index.offered.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(offered, ["kept", "listed"]);
    assert!(
        index.refused.is_empty() && index.waiting.is_empty(),
        "{index:?}"
    );
    for name in [".archive", "listed-old"] {
        let call = keeper_core::agents::workflow::parse_view(&serde_json::json!({"name": name}))
            .expect("call");
        let mut files = Vec::new();
        let outcome = keeper_agent::skills::view(&index, w.root(), "80-agents", &call, &mut files);
        assert!(
            matches!(&outcome, ToolOutcome::Refused { .. }),
            "{name}: {outcome:?}"
        );
        assert!(files.is_empty());
    }
}

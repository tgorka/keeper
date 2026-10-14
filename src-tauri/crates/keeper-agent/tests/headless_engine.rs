//! agentd's own engine over real bare repositories (story 90.3, acceptance 4–6).
//!
//! Its own test binary: `open_engine` arms the folder tier, a process-global
//! `RwLock`, and `cargo test` runs every test of a binary in one process.
//! Skipped, not failed, on a machine with no `git` — and only then: any other
//! error from `open_engine` fails the test.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use keeper_agent::headless::{
    open_engine, zone_verdicts, AgentdEngine, HeadlessError, HeadlessSyncPlatform, SecretMap,
    SECRET_ENV_PREFIX,
};
use keeper_core::agents::agentd::AgentdConfig;
use keeper_sync::lfs::virtual_policy::{VirtualPolicy, Virtualization};
use keeper_sync::provenance::SyncSource;
use keeper_sync::xdg::SecretStore;
use keeper_sync::SyncError;

const TGORKA: &str = "@tgorka:example.org";
const MARTA: &str = "@marta:example.org";

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "seed")
        .env("GIT_AUTHOR_EMAIL", "seed@example.invalid")
        .env("GIT_COMMITTER_NAME", "seed")
        .env("GIT_COMMITTER_EMAIL", "seed@example.invalid")
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

/// A bare drive seeded with `files`; `None` when there is no `git`.
fn bare_drive(root: &Path, name: &str, files: &[(&str, &str)]) -> Option<PathBuf> {
    let bare = root.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).ok()?;
    if !git(&bare, &["init", "-q", "--bare", "-b", "main"]) {
        return None;
    }
    let seed = root.join(format!("{name}-seed"));
    std::fs::create_dir_all(&seed).ok()?;
    assert!(git(&seed, &["init", "-q", "-b", "main"]));
    for (rel, text) in files {
        let path = seed.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }
    assert!(git(&seed, &["add", "-A"]));
    assert!(git(&seed, &["commit", "-q", "-m", "seed"]));
    assert!(git(&seed, &["push", "-q", &bare.to_string_lossy(), "main"]));
    Some(bare)
}

fn drive_toml(id: &str, owner: &str, readers: &[&str]) -> String {
    let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
    format!(
        "version = 1\nid = \"{id}\"\nprincipal = \"tgorka\"\nowner = \"{owner}\"\nreaders = [{}]\n",
        readers.join(", ")
    )
}

struct Drive<'a> {
    id: &'a str,
    remote: &'a Path,
    readers: &'a [&'a str],
}

fn config(drives: &[Drive], homes: &[&str]) -> AgentdConfig {
    let mut text = String::from(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\n\n[homeserver]\nurl = \"https://matrix.example.org\"\n",
    );
    for drive in drives {
        let readers: Vec<String> = drive.readers.iter().map(|r| format!("\"{r}\"")).collect();
        text.push_str(&format!(
            "\n[[drives]]\nid = \"{}\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [{}]\n",
            drive.id,
            drive.remote.display(),
            readers.join(", ")
        ));
    }
    for home in homes {
        text.push_str(&format!(
            "\n[[agents]]\ndrive = \"{home}\"\nids = [\"nixi\"]\n"
        ));
    }
    AgentdConfig::parse(&text).expect("the test config parses")
}

fn platform(data: &Path) -> Arc<HeadlessSyncPlatform> {
    let store = SecretStore::new(SECRET_ENV_PREFIX, data.join("secrets"));
    Arc::new(HeadlessSyncPlatform::new(
        data,
        "electra",
        Arc::new(SecretMap::new(store)),
    ))
}

/// `open_engine`, or `None` when this box has no usable `git`; every other
/// error is a failure, never a skip.
fn open_or_skip(config: &AgentdConfig, data: &Path) -> Option<AgentdEngine> {
    match open_engine(config, platform(data)) {
        Ok(agentd) => Some(agentd),
        Err(HeadlessError::Sync(SyncError::GitMissing { .. })) => None,
        Err(error) => panic!("open_engine failed: {error}"),
    }
}

#[test]
fn agentd_never_opens_a_sync_db_it_did_not_create() {
    let root = tempfile::tempdir().expect("tempdir");
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).expect("mkdir");
    let planted = data.join("sync.db");
    std::fs::write(&planted, b"syncd's").expect("plant");
    let remote = root.path().join("none.git");

    let refusal = open_engine(
        &config(
            &[Drive {
                id: "tgdrive",
                remote: &remote,
                readers: &[TGORKA],
            }],
            &["tgdrive"],
        ),
        platform(&data),
    )
    .err()
    .expect("a foreign sync.db is refused");

    assert!(matches!(refusal, HeadlessError::ForeignDatabase { .. }));
    assert_eq!(refusal.exit_code(), 2);
    assert!(
        refusal.to_string().contains(&planted.display().to_string()),
        "{refusal}"
    );
    assert_eq!(std::fs::read(&planted).expect("untouched"), b"syncd's");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_per_drive_with_agents_and_sessions_armed() {
    let root = tempfile::tempdir().expect("tempdir");
    let Some(bare) = bare_drive(
        root.path(),
        "tgdrive",
        &[
            (
                "80-agents/_drive.toml",
                &drive_toml("tgdrive", TGORKA, &[TGORKA]),
            ),
            ("80-agents/nixi/agent.toml", "version = 1\n"),
            ("60-sessions/README.md", "# sessions\n"),
        ],
    ) else {
        return;
    };
    let data = root.path().join("data");
    let config = config(
        &[Drive {
            id: "tgdrive",
            remote: &bare,
            readers: &[TGORKA],
        }],
        &["tgdrive"],
    );
    let Some(agentd) = open_or_skip(&config, &data) else {
        return;
    };
    let mounted = agentd.drives[0].clone();
    agentd
        .engine
        .sync_once(&mounted.profile_id, SyncSource::Manual)
        .await
        .expect("the first checkout");

    let profile = agentd
        .engine
        .list_profiles()
        .expect("profiles")
        .into_iter()
        .find(|row| row.id == mounted.profile_id)
        .expect("the drive's profile");
    assert!(profile.agents.is_some() && profile.sessions.is_some());
    assert_eq!(mounted.local_path, data.join("drives").join("tgdrive"));
    assert_eq!(
        std::fs::read_to_string(mounted.local_path.join("80-agents/_drive.toml")).expect("zone"),
        drive_toml("tgdrive", TGORKA, &[TGORKA])
    );
    assert!(mounted.local_path.join("60-sessions/README.md").is_file());

    let verdicts = zone_verdicts(&config, &agentd).expect("verdicts");
    assert!(verdicts[0].hosts.is_ok(), "{:?}", verdicts[0].hosts);

    // A second open finds the same profile by its drive id.
    drop(agentd);
    let again = open_engine(&config, platform(&data)).expect("reopen");
    assert_eq!(again.drives[0].profile_id, mounted.profile_id);
    assert_eq!(again.engine.list_profiles().expect("profiles").len(), 1);
}

#[test]
fn a_drive_removed_from_agentd_toml_is_no_longer_mounted() {
    let root = tempfile::tempdir().expect("tempdir");
    let data = root.path().join("data");
    let (tgdrive, neuradrive) = (root.path().join("tg.git"), root.path().join("neura.git"));
    let both = config(
        &[
            Drive {
                id: "tgdrive",
                remote: &tgdrive,
                readers: &[TGORKA],
            },
            Drive {
                id: "neuradrive",
                remote: &neuradrive,
                readers: &[TGORKA, MARTA],
            },
        ],
        &["tgdrive"],
    );
    let Some(first) = open_or_skip(&both, &data) else {
        return;
    };
    let tg_profile = first.drives[0].profile_id.clone();
    assert_eq!(first.engine.list_profiles().expect("profiles").len(), 2);
    drop(first);

    let one = config(
        &[Drive {
            id: "tgdrive",
            remote: &tgdrive,
            readers: &[TGORKA],
        }],
        &["tgdrive"],
    );
    let again = open_engine(&one, platform(&data)).expect("reopen");
    let profiles = again.engine.list_profiles().expect("profiles");
    assert_eq!(
        profiles
            .iter()
            .map(|row| (row.id.as_str(), row.name.as_str()))
            .collect::<Vec<_>>(),
        [(tg_profile.as_str(), "tgdrive")],
        "neuradrive's profile is gone before anything syncs"
    );
}

#[test]
fn a_changed_remote_keeps_one_profile() {
    let root = tempfile::tempdir().expect("tempdir");
    let data = root.path().join("data");
    let (old, moved) = (root.path().join("old.git"), root.path().join("moved.git"));
    let at = |remote: &Path| {
        config(
            &[Drive {
                id: "tgdrive",
                remote,
                readers: &[TGORKA],
            }],
            &["tgdrive"],
        )
    };
    let Some(first) = open_or_skip(&at(&old), &data) else {
        return;
    };
    let profile_id = first.drives[0].profile_id.clone();
    drop(first);

    let again = open_engine(&at(&moved), platform(&data)).expect("reopen");
    let profiles = again.engine.list_profiles().expect("profiles");
    assert_eq!(profiles.len(), 1, "one checkout, one profile");
    assert_eq!(profiles[0].id, profile_id);
    assert_eq!(profiles[0].remote_url, moved.display().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_virtual_pattern_over_the_agents_zone_is_refused_naming_it() {
    let root = tempfile::tempdir().expect("tempdir");
    let Some(bare) = bare_drive(
        root.path(),
        "tgdrive",
        &[
            (
                "80-agents/_drive.toml",
                &drive_toml("tgdrive", TGORKA, &[TGORKA]),
            ),
            (
                ".keeper/keeper.toml",
                "[folder]\nvirtualPatterns = [\"80-agents/**\"]\n",
            ),
        ],
    ) else {
        return;
    };
    let data = root.path().join("data");
    let config = config(
        &[Drive {
            id: "tgdrive",
            remote: &bare,
            readers: &[TGORKA],
        }],
        &["tgdrive"],
    );
    let Some(agentd) = open_or_skip(&config, &data) else {
        return;
    };
    agentd
        .engine
        .sync_once(&agentd.drives[0].profile_id, SyncSource::Manual)
        .await
        .expect("the checkout succeeds; only the verdict refuses");

    let verdicts = zone_verdicts(&config, &agentd).expect("verdicts");
    let sentence = verdicts[0].hosts.as_ref().expect_err("refused");
    assert!(sentence.contains("\"80-agents/**\""), "{sentence}");
    assert!(sentence.contains("agents zone"), "{sentence}");
}

/// `materialize = "zones"`: every LFS path may stay on the server except the
/// agents and sessions zones', which stay whole — so the zone still hosts,
/// and a drive moved back to `all` loses the list.
#[tokio::test(flavor = "multi_thread")]
async fn a_drive_materialized_by_zones_keeps_only_its_zones_and_still_hosts() {
    let root = tempfile::tempdir().expect("tempdir");
    let Some(bare) = bare_drive(
        root.path(),
        "tgdrive",
        &[
            (
                "80-agents/_drive.toml",
                &drive_toml("tgdrive", TGORKA, &[TGORKA]),
            ),
            ("40-media/README.md", "# media\n"),
        ],
    ) else {
        return;
    };
    let data = root.path().join("data");
    let drives = [Drive {
        id: "tgdrive",
        remote: &bare,
        readers: &[TGORKA],
    }];
    let zones = AgentdConfig::parse(&format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\n\n[homeserver]\nurl = \"https://matrix.example.org\"\n\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\nmaterialize = \"zones\"\n\n[[agents]]\ndrive = \"tgdrive\"\nids = [\"nixi\"]\n",
        bare.display()
    ))
    .expect("zones parses");
    let Some(agentd) = open_or_skip(&zones, &data) else {
        return;
    };
    agentd
        .engine
        .sync_once(&agentd.drives[0].profile_id, SyncSource::Manual)
        .await
        .expect("checkout");

    let profile = agentd
        .engine
        .list_profiles()
        .expect("profiles")
        .into_iter()
        .find(|row| row.id == agentd.drives[0].profile_id)
        .expect("the drive's profile");
    assert_eq!(
        profile.virtual_patterns,
        ["**", "!/80-agents/", "!/60-sessions/"]
    );

    // The real compiler, not a reading of the list: what a checkout asks.
    let policy = VirtualPolicy::compile(&profile).expect("the policy compiles");
    let big = 10 * 1024 * 1024;
    for away in ["40-media/clip.mp4", "top.bin", "10-notes/deep/a/scan.pdf"] {
        assert_eq!(
            policy.resolve(Path::new(away), big),
            Virtualization::Virtual,
            "{away} may stay on the server"
        );
    }
    for kept in [
        "80-agents/nixi/avatar.png",
        "60-sessions/01ABC/log/blobs/feed.json",
    ] {
        assert_eq!(
            policy.resolve(Path::new(kept), big),
            Virtualization::Materialize,
            "{kept} is in a zone and stays on this disk"
        );
    }

    let verdicts = zone_verdicts(&zones, &agentd).expect("verdicts");
    assert!(verdicts[0].hosts.is_ok(), "{:?}", verdicts[0].hosts);
    drop(agentd);

    let again = open_engine(&config(&drives, &["tgdrive"]), platform(&data)).expect("reopen");
    let profiles = again.engine.list_profiles().expect("profiles");
    assert!(
        profiles[0].virtual_patterns.is_empty(),
        "`all` takes the list `zones` wrote back off: {:?}",
        profiles[0].virtual_patterns
    );
}

/// The outcome `zones` exists for, over real LFS objects on a filesystem
/// remote: the zone's object is fetched, the media's is never queued, so its
/// pointer is all that reaches this disk. Under `all` both are fetched — the
/// control that proves the first half would see a download.
#[tokio::test(flavor = "multi_thread")]
async fn zones_fetches_the_zones_lfs_objects_and_never_queues_the_rest() {
    use keeper_sync::lfs::pointer::Pointer;
    use keeper_sync::lfs::store::LfsStore;

    for (materialize, media_fetched) in [("zones", false), ("all", true)] {
        let root = tempfile::tempdir().expect("tempdir");
        // The objects first, so their pointers can be committed as text.
        let remote = root.path().join("tgdrive.git");
        std::fs::create_dir_all(&remote).expect("mkdir");
        let store = LfsStore::in_git_dir(&remote);
        let put = |byte: u8| {
            let (oid, size) = store
                .insert_streaming(std::io::Cursor::new(vec![byte; 300 * 1024]))
                .expect("insert the object");
            Pointer::new(oid, size).render()
        };
        let (avatar, clip) = (put(1), put(2));
        let Some(bare) = bare_drive(
            root.path(),
            "tgdrive",
            &[
                (
                    "80-agents/_drive.toml",
                    &drive_toml("tgdrive", TGORKA, &[TGORKA]),
                ),
                ("80-agents/nixi/avatar.png", &avatar),
                ("40-media/clip.mp4", &clip),
            ],
        ) else {
            return;
        };
        assert_eq!(bare, remote, "the store sits in the remote it serves");

        let data = root.path().join("data");
        let config = AgentdConfig::parse(&format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\n\n[homeserver]\nurl = \"https://matrix.example.org\"\n\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\nmaterialize = \"{materialize}\"\n\n[[agents]]\ndrive = \"tgdrive\"\nids = [\"nixi\"]\n",
            bare.display()
        ))
        .expect("config parses");
        let Some(agentd) = open_or_skip(&config, &data) else {
            return;
        };
        let profile_id = agentd.drives[0].profile_id.clone();
        // Twice: the first pass checks out and queues, the second drains.
        for _ in 0..2 {
            agentd
                .engine
                .sync_once(&profile_id, SyncSource::Manual)
                .await
                .expect("sync");
        }

        let worktree = &agentd.drives[0].local_path;
        let read = |rel: &str| std::fs::read(worktree.join(rel)).expect("read the worktree");
        assert_eq!(
            read("80-agents/nixi/avatar.png"),
            vec![1u8; 300 * 1024],
            "{materialize}: the agents zone's object is on this disk"
        );
        let clip_on_disk = read("40-media/clip.mp4");
        if media_fetched {
            assert_eq!(clip_on_disk, vec![2u8; 300 * 1024], "all: the media too");
        } else {
            assert_eq!(
                clip_on_disk,
                clip.as_bytes(),
                "zones: only the pointer reaches this disk"
            );
            let local = LfsStore::in_git_dir(worktree.join(".git"));
            let oid = Pointer::parse(clip.as_bytes()).expect("pointer").oid;
            assert!(
                !local.contains(&oid, 300 * 1024),
                "zones: the media's object was never downloaded"
            );
        }
    }
}

/// A pattern beside a protection over the zone is tried under it: the zone
/// stays whole, so the drive hosts. Without the protection, it is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_pattern_is_judged_under_the_protections_beside_it() {
    for (patterns, hosts) in [
        (
            "[\"40-media/**\", \"**\", \"!/80-agents/\", \"!/60-sessions/\"]",
            true,
        ),
        ("[\"**\", \"!/60-sessions/\"]", false),
    ] {
        let root = tempfile::tempdir().expect("tempdir");
        let Some(bare) = bare_drive(
            root.path(),
            "tgdrive",
            &[
                (
                    "80-agents/_drive.toml",
                    &drive_toml("tgdrive", TGORKA, &[TGORKA]),
                ),
                (
                    ".keeper/keeper.toml",
                    &format!("[folder]\nvirtualPatterns = {patterns}\n"),
                ),
            ],
        ) else {
            return;
        };
        let data = root.path().join("data");
        let config = config(
            &[Drive {
                id: "tgdrive",
                remote: &bare,
                readers: &[TGORKA],
            }],
            &["tgdrive"],
        );
        let Some(agentd) = open_or_skip(&config, &data) else {
            return;
        };
        agentd
            .engine
            .sync_once(&agentd.drives[0].profile_id, SyncSource::Manual)
            .await
            .expect("checkout");

        let verdicts = zone_verdicts(&config, &agentd).expect("verdicts");
        match (&verdicts[0].hosts, hosts) {
            (Ok(_), true) => {}
            (Err(sentence), false) => {
                assert!(sentence.contains("\"**\""), "{sentence}");
                assert!(sentence.contains("agents zone"), "{sentence}");
            }
            (verdict, _) => panic!("{patterns}: {verdict:?}"),
        }
    }
}

/// The size floor is judged at the largest log chunk, not at an unbounded
/// size: a floor above a chunk leaves the zone whole, one below it does not.
#[tokio::test(flavor = "multi_thread")]
async fn a_size_floor_refuses_the_zone_only_below_a_chunk() {
    for (floor, hosts) in [(1024 * 1024, true), (64 * 1024, false)] {
        let root = tempfile::tempdir().expect("tempdir");
        let Some(bare) = bare_drive(
            root.path(),
            "tgdrive",
            &[
                (
                    "80-agents/_drive.toml",
                    &drive_toml("tgdrive", TGORKA, &[TGORKA]),
                ),
                (
                    ".keeper/keeper.toml",
                    &format!("[folder]\nvirtualOverBytes = {floor}\n"),
                ),
            ],
        ) else {
            return;
        };
        let data = root.path().join("data");
        let config = config(
            &[Drive {
                id: "tgdrive",
                remote: &bare,
                readers: &[TGORKA],
            }],
            &["tgdrive"],
        );
        let Some(agentd) = open_or_skip(&config, &data) else {
            return;
        };
        agentd
            .engine
            .sync_once(&agentd.drives[0].profile_id, SyncSource::Manual)
            .await
            .expect("checkout");

        let verdicts = zone_verdicts(&config, &agentd).expect("verdicts");
        match (&verdicts[0].hosts, hosts) {
            (Ok(_), true) => {}
            (Err(sentence), false) => assert!(sentence.contains("size floor"), "{sentence}"),
            (verdict, _) => panic!("a floor of {floor} bytes: {verdict:?}"),
        }
    }
}

#[test]
fn a_drive_failing_the_mount_rule_is_never_fetched() {
    let root = tempfile::tempdir().expect("tempdir");
    let Some(neuradrive) = bare_drive(root.path(), "neuradrive", &[("a.md", "a\n")]) else {
        return;
    };
    let Some(tgdrive) = bare_drive(root.path(), "tgdrive", &[("b.md", "b\n")]) else {
        return;
    };
    let data = root.path().join("data");
    let config = config(
        &[
            Drive {
                id: "neuradrive",
                remote: &neuradrive,
                readers: &[TGORKA, MARTA],
            },
            Drive {
                id: "tgdrive",
                remote: &tgdrive,
                readers: &[TGORKA],
            },
        ],
        &["neuradrive"],
    );

    let refusal = open_engine(&config, platform(&data))
        .err()
        .expect("the mount rule refuses");

    assert_eq!(refusal.exit_code(), 2);
    assert!(refusal.to_string().contains(MARTA), "{refusal}");
    assert!(
        !data.join("drives").exists(),
        "nothing is checked out, not even the drive that passes"
    );
    assert!(
        !data.join("sync.db").exists(),
        "no engine was opened, so no profile exists"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declaration_that_differs_from_its_pin_hosts_nothing_on_disk() {
    let root = tempfile::tempdir().expect("tempdir");
    let wider = drive_toml("tgdrive", TGORKA, &[TGORKA, "@x:example.org"]);
    let Some(bare) = bare_drive(root.path(), "tgdrive", &[("80-agents/_drive.toml", &wider)])
    else {
        return;
    };
    let data = root.path().join("data");
    let config = config(
        &[Drive {
            id: "tgdrive",
            remote: &bare,
            readers: &[TGORKA],
        }],
        &["tgdrive"],
    );
    let Some(agentd) = open_or_skip(&config, &data) else {
        return;
    };
    agentd
        .engine
        .sync_once(&agentd.drives[0].profile_id, SyncSource::Manual)
        .await
        .expect("checkout");

    let verdicts = zone_verdicts(&config, &agentd).expect("verdicts");
    let sentence = verdicts[0].hosts.as_ref().expect_err("hosts nothing");
    assert!(
        sentence.contains("_drive.toml names the readers @tgorka:example.org, @x:example.org; this host pinned @tgorka:example.org"),
        "{sentence}"
    );
}

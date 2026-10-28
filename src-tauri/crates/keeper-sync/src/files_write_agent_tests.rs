//! [`agent_write_check`] over a real git checkout (109.27): every path an
//! agent's write may never reach, and the ones it may.

use super::*;
use crate::exclude::ExcludeSet;

/// `git <args>` in `dir`, with no user or system configuration in the room.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

const POINTER: &str = "version https://git-lfs.github.com/spec/v1\n\
     oid sha256:4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393\n\
     size 12345\n";

/// A drive: tracked notes, a `.gitignore` (`*.log`), a `.gitattributes`
/// routing `*.bin` through LFS, a tracked pointer, an untracked note, a
/// force-added ignored file, a tracked link to a file and one to a folder,
/// and a tracked folder no longer on the disk.
fn drive() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    write(root, "notes/a.md", "hello\n");
    write(root, "notes/deep/b.md", "b\n");
    write(root, ".gitignore", "*.log\n");
    write(
        root,
        ".gitattributes",
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    write(root, "media/clip.bin", "not text \u{1}\n");
    write(root, "notes/pointer.md", POINTER);
    write(root, ".keeper/keeper.toml", "x = 1\n");
    write(root, "gone/x.md", "x\n");
    std::os::unix::fs::symlink("a.md", root.join("notes/link.md")).expect("file link");
    std::os::unix::fs::symlink("notes", root.join("linked")).expect("folder link");
    git(root, &["add", "-A"]);
    write(root, "debug.log", "log\n");
    git(root, &["add", "-f", "debug.log"]);
    std::fs::remove_dir_all(root.join("gone")).expect("rm gone");
    write(root, "notes/untracked.md", "mine\n");
    dir
}

fn check(root: &Path, subpath: &str, target: AgentTarget) -> Result<String, WriteRefusal> {
    let scope = WriteScope::new("tgdrive", None);
    let excludes = ExcludeSet::new(&[]).expect("excludes");
    agent_write_check(root, &scope, &excludes, subpath, target)
}

fn subpath(refusal: Result<String, WriteRefusal>) -> (WriteRefusal, String) {
    let refusal = refusal.expect_err("refused");
    let sentence = refusal.to_string();
    (refusal, sentence)
}

#[test]
fn a_tracked_text_file_and_a_new_path_beside_it_are_allowed() {
    let dir = drive();
    let root = dir.path();
    assert_eq!(
        check(root, "notes/a.md", AgentTarget::Existing),
        Ok("notes/a.md".to_owned())
    );
    assert_eq!(
        check(root, "notes/deep/b.md", AgentTarget::Existing),
        Ok("notes/deep/b.md".to_owned())
    );
    assert_eq!(
        check(root, "notes/new.md", AgentTarget::New),
        Ok("notes/new.md".to_owned())
    );
    // A new folder is made by the file in it.
    assert_eq!(
        check(root, "new/dir/c.md", AgentTarget::New),
        Ok("new/dir/c.md".to_owned())
    );
}

#[test]
fn git_and_keeper_own_files_are_refused_at_any_depth_and_in_any_case() {
    let dir = drive();
    let root = dir.path();
    for path in [
        ".git/config",
        ".GIT/config",
        ".git",
        ".keeper/keeper.toml",
        "notes/.Keeper/x.md",
        ".keeper-sync/state.json",
        ".obsidian/app.json",
        ".gitattributes",
        "notes/.gitattributes",
        ".gitignore",
        "notes/.GitIgnore",
        ".gitmodules",
        ".lfsconfig",
        ".keepervirtual",
    ] {
        for target in [AgentTarget::Existing, AgentTarget::New] {
            assert_eq!(
                check(root, path, target),
                Err(WriteRefusal::Machinery {
                    subpath: path.to_owned()
                }),
                "{path} {target:?}"
            );
        }
    }
    // A name that only starts like one is a note.
    assert_eq!(
        check(root, "notes/.gitignore.md", AgentTarget::New),
        Ok("notes/.gitignore.md".to_owned())
    );
}

#[test]
fn an_ignored_path_is_refused_whether_git_or_the_profile_ignores_it() {
    let dir = drive();
    let root = dir.path();
    let ignored = |path: &str| WriteRefusal::Ignored {
        subpath: path.to_owned(),
    };
    assert_eq!(
        check(root, "notes/today.log", AgentTarget::New),
        Err(ignored("notes/today.log"))
    );
    // Tracked, force-added past `.gitignore`: still not an agent's to write.
    assert_eq!(
        check(root, "debug.log", AgentTarget::Existing),
        Err(ignored("debug.log"))
    );
    // keeper's own exclusions, built in and the profile's.
    assert_eq!(
        check(root, "notes/.DS_Store", AgentTarget::New),
        Err(ignored("notes/.DS_Store"))
    );
    let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("excludes");
    assert_eq!(
        agent_write_check(
            root,
            &WriteScope::new("tgdrive", None),
            &excludes,
            "notes/x.tmp",
            AgentTarget::New
        ),
        Err(ignored("notes/x.tmp"))
    );
}

#[test]
fn an_lfs_path_is_refused_by_its_attributes_and_by_its_pointer() {
    let dir = drive();
    let root = dir.path();
    let lfs = |path: &str| WriteRefusal::LfsRouted {
        subpath: path.to_owned(),
    };
    assert_eq!(
        check(root, "media/new.bin", AgentTarget::New),
        Err(lfs("media/new.bin"))
    );
    assert_eq!(
        check(root, "media/clip.bin", AgentTarget::Existing),
        Err(lfs("media/clip.bin"))
    );
    // `*.md` is not routed: the pointer itself says so.
    assert_eq!(
        check(root, "notes/pointer.md", AgentTarget::Existing),
        Err(lfs("notes/pointer.md"))
    );
}

#[test]
fn a_pointer_only_in_the_index_is_refused_too() {
    let dir = drive();
    let root = dir.path();
    // Committed as a pointer, its content materialized on this disk.
    write(root, "notes/pointer.md", "the real text\n");
    assert_eq!(
        check(root, "notes/pointer.md", AgentTarget::Existing),
        Err(WriteRefusal::LfsRouted {
            subpath: "notes/pointer.md".to_owned()
        })
    );
}

#[test]
fn a_pointer_only_on_the_disk_is_refused_too() {
    let dir = drive();
    let root = dir.path();
    write(root, "notes/a.md", POINTER);
    assert_eq!(
        check(root, "notes/a.md", AgentTarget::Existing),
        Err(WriteRefusal::LfsRouted {
            subpath: "notes/a.md".to_owned()
        })
    );
}

/// An untracked file is not the drive's: changing it answers as a file that
/// is not there does, and creating over it is refused as over any file.
#[test]
fn an_untracked_file_is_not_the_drives_and_cannot_be_created_over() {
    let dir = drive();
    let root = dir.path();
    assert_eq!(
        check(root, "notes/untracked.md", AgentTarget::Existing),
        Err(WriteRefusal::Missing {
            subpath: "notes/untracked.md".to_owned()
        })
    );
    assert_eq!(
        check(root, "notes/untracked.md", AgentTarget::New),
        Err(WriteRefusal::Exists {
            subpath: "notes/untracked.md".to_owned()
        })
    );
}

/// R603 (review of #603): no refusal tells an agent what a person keeps
/// beside the drive. The same path, asked before and after an untracked or
/// ignored file appears there, answers the same — except a new path over an
/// untracked file, which must be refused (allowing it would write over the
/// file), and is refused exactly as over a tracked one.
#[test]
fn an_untracked_or_ignored_file_is_answered_as_if_it_were_not_there() {
    let dir = drive();
    let root = dir.path();
    let cases = [
        ("private.md", AgentTarget::Existing, "file"),
        ("private/inner.md", AgentTarget::Existing, "file"),
        ("private-folder", AgentTarget::Existing, "folder"),
        ("notes/secret.log", AgentTarget::Existing, "file"),
        ("notes/secret.log", AgentTarget::New, "file"),
        ("junk.log/inner.md", AgentTarget::New, "file"),
        ("notes/.DS_Store", AgentTarget::Existing, "file"),
        ("notes/.DS_Store", AgentTarget::New, "file"),
    ];
    for (path, target, kind) in cases {
        let before = check(root, path, target);
        assert!(before.is_err(), "{path} {target:?}: {before:?}");
        match kind {
            "folder" => std::fs::create_dir_all(root.join(path).join("x")).expect("mkdir"),
            _ => write(root, path, "a person's own\n"),
        }
        assert_eq!(check(root, path, target), before, "{path} {target:?}");
    }
    // Something untracked in the way of a new path: the file at it, or a
    // file where its folder would be.
    write(root, "mine.md", "a person's own\n");
    assert_eq!(
        check(root, "mine.md", AgentTarget::New),
        Err(WriteRefusal::Exists {
            subpath: "mine.md".to_owned()
        })
    );
}

/// R603-1, R603-2: a tracked file or link gone from this disk still stands
/// where a folder of a new path would be — committing under it would turn
/// the drive's file into a folder — in any spelling the Mac takes for it.
#[test]
fn a_tracked_file_or_link_gone_from_the_disk_still_stands_in_a_new_paths_way() {
    let dir = drive();
    let root = dir.path();
    std::fs::remove_file(root.join("notes/a.md")).expect("rm file");
    std::fs::remove_file(root.join("linked")).expect("rm link");
    let taken = |name: &str| {
        Err(WriteRefusal::NameTaken {
            name: name.to_owned(),
        })
    };
    assert_eq!(
        check(root, "notes/a.md/new.md", AgentTarget::New),
        taken("a.md")
    );
    assert_eq!(
        check(root, "notes/A.md/deeper/new.md", AgentTarget::New),
        taken("A.md")
    );
    assert_eq!(
        check(root, "notes/a.md/new.md", AgentTarget::Existing),
        taken("a.md")
    );
    for path in ["linked/new.md", "Linked/new.md"] {
        assert_eq!(
            check(root, path, AgentTarget::New),
            Err(WriteRefusal::Link {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
}

/// R603-3: a new path the Mac's volume would take for a tracked one — in
/// case or in Unicode normalization — is refused, whether or not the tracked
/// file is on this disk. Always, not only where `core.ignorecase` is set: the
/// drive syncs to every device its person keeps, and the Mac's APFS folds
/// names whatever this checkout's configuration says.
#[test]
fn a_new_path_the_mac_would_take_for_a_tracked_one_is_refused() {
    let dir = drive();
    let root = dir.path();
    write(root, "notes/caf\u{e9}.md", "composed\n");
    git(root, &["add", "notes/caf\u{e9}.md"]);
    for gone in ["notes/a.md", "notes/caf\u{e9}.md"] {
        std::fs::remove_file(root.join(gone)).expect("rm");
    }
    for path in [
        "notes/A.md",
        "NOTES/a.md",
        "notes/cafe\u{301}.md",
        "notes/CAF\u{c9}.md",
    ] {
        assert_eq!(
            check(root, path, AgentTarget::New),
            Err(WriteRefusal::Exists {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
    // A tracked folder spelled otherwise: on the Mac the file lands in it,
    // under a key no other device has.
    assert_eq!(
        check(root, "Notes/new.md", AgentTarget::New),
        Err(WriteRefusal::NameTaken {
            name: "Notes".to_owned()
        })
    );
    assert_eq!(
        check(root, "notes/new.md", AgentTarget::New),
        Ok("notes/new.md".to_owned())
    );
    // On this disk, untracked: the same file on the Mac in another
    // normalization.
    write(root, "notes/na\u{ef}ve.md", "a person's own\n");
    assert_eq!(
        check(root, "notes/nai\u{308}ve.md", AgentTarget::New),
        Err(WriteRefusal::Exists {
            subpath: "notes/nai\u{308}ve.md".to_owned()
        })
    );
}

/// R603-4: every fence compares names as the Mac's volume does — the agents'
/// home, the sessions zone and its workspace, and git's and keeper's own
/// names — so a decomposed accent, a capital or a Kelvin sign (`\u{212A}`,
/// which decomposes to `K`) is not a way past one.
#[test]
fn the_fences_hold_for_any_spelling_the_mac_takes_for_them() {
    let dir = drive();
    let root = dir.path();
    let excludes = ExcludeSet::new(&[]).expect("excludes");
    for (zone, spelled) in [
        ("caf\u{e9}", "cafe\u{301}"),
        ("cafe\u{301}", "caf\u{e9}"),
        ("\u{c1}gents", "\u{e1}gents"),
        ("80-agents", "80-AGENTS"),
    ] {
        let path = format!("{spelled}/nixi/SOUL.md");
        write(root, &path, "a person's own\n");
        git(root, &["add", &path]);
        let scope = WriteScope::new("tgdrive", None).with_agents(Some(zone));
        assert_eq!(
            agent_write_check(root, &scope, &excludes, &path, AgentTarget::Existing),
            Err(WriteRefusal::AgentHome {
                subpath: path.clone()
            }),
            "{zone:?} {path:?}"
        );
    }
    let sessions = WriteScope::new("tgdrive", None)
        .with_sessions(Some("60-s\u{e9}ances"))
        .with_sessions_closed(true);
    assert_eq!(
        agent_write_check(
            root,
            &sessions,
            &excludes,
            "60-SE\u{301}ANCES/active/s/a.md",
            AgentTarget::New
        ),
        Err(WriteRefusal::SessionsZone {
            subpath: "60-SE\u{301}ANCES/active/s/a.md".to_owned()
        })
    );
    // A person's scope keeps the zone open but its workspace fenced.
    let person = WriteScope::new("tgdrive", None).with_sessions(Some("60-s\u{e9}ances"));
    for path in [
        "60-se\u{301}ances/Active/s/Workspace/x.md",
        "60-S\u{c9}ANCES/ARCHIVE/2026/s/workspace/x.md",
    ] {
        assert_eq!(
            person.fenced(path),
            Err(WriteRefusal::SessionWorkspace {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
    for path in [".\u{212A}eeper/keeper.toml", "notes/.\u{212A}EEPER-sync/x"] {
        assert_eq!(
            check(root, path, AgentTarget::New),
            Err(WriteRefusal::Machinery {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
}

/// R603-08: every fence an agent's write meets folds as APFS folds, which is
/// full case folding, not lowercasing. Measured on hesperia: `.obſidian`
/// (U+017F long s) is the `.obsidian` folder, `ſessions` is `sessions`, and
/// `ß` is `ss` — and lowercasing keeps each of those apart, so each was a way
/// past the fence it names.
#[test]
fn the_fences_hold_for_a_full_case_fold() {
    let dir = drive();
    let root = dir.path();
    let excludes = ExcludeSet::new(&[]).expect("excludes");

    // Machinery: git's, keeper's, the sync engine's and Obsidian's own.
    for path in [
        ".ob\u{17f}idian/app.json",
        "notes/.OB\u{17f}IDIAN/workspace.json",
        "notes/.keeper-\u{17f}ync/state.json",
        "notes/.gitattribute\u{17f}",
    ] {
        for target in [AgentTarget::Existing, AgentTarget::New] {
            assert_eq!(
                check(root, path, target),
                Err(WriteRefusal::Machinery {
                    subpath: path.to_owned()
                }),
                "{path} {target:?}"
            );
        }
    }

    // The sessions zone, closed in an agent host's scope, and its workspace,
    // fenced in a person's.
    let host = WriteScope::new("tgdrive", None)
        .with_sessions(Some("sessions"))
        .with_sessions_closed(true);
    for path in ["\u{17f}essions/active/s/a.md", "SE\u{17f}SIONS/AGENTS.md"] {
        assert_eq!(
            agent_write_check(root, &host, &excludes, path, AgentTarget::New),
            Err(WriteRefusal::SessionsZone {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
    let person = WriteScope::new("tgdrive", None).with_sessions(Some("sessions"));
    for path in [
        "\u{17f}essions/active/s/workspace/x.md",
        "sessions/active/s/work\u{17f}pace/x.md",
    ] {
        assert_eq!(
            agent_write_check(root, &person, &excludes, path, AgentTarget::New),
            Err(WriteRefusal::SessionWorkspace {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }

    // An agent's home, its zone spelled with `ß` one side and `ss` the other.
    for (zone, spelled) in [
        ("80-stra\u{df}e", "80-STRASSE"),
        ("80-strasse", "80-Stra\u{df}e"),
        ("80-agents", "80-agent\u{17f}"),
    ] {
        let path = format!("{spelled}/nixi/SOUL.md");
        write(root, &path, "a person's own\n");
        git(root, &["add", &path]);
        let scope = WriteScope::new("tgdrive", None).with_agents(Some(zone));
        for target in [AgentTarget::Existing, AgentTarget::New] {
            assert_eq!(
                agent_write_check(root, &scope, &excludes, &path, target),
                Err(WriteRefusal::AgentHome {
                    subpath: path.clone()
                }),
                "{zone:?} {path:?} {target:?}"
            );
        }
    }
}

/// R603-08: a new path collides, in the index and on the disk, with every
/// spelling APFS takes for one already there — `ß`/`ss`, `ſ`/`s`, `ς`/`Σ`.
#[test]
fn a_new_path_collides_under_a_full_case_fold() {
    let dir = drive();
    let root = dir.path();
    write(root, "notes/stra\u{df}e.md", "tracked\n");
    write(root, "sessions/log.md", "tracked\n");
    git(root, &["add", "notes/stra\u{df}e.md", "sessions/log.md"]);
    // Gone from this disk, still the drive's.
    std::fs::remove_file(root.join("notes/stra\u{df}e.md")).expect("rm");
    for path in ["notes/STRASSE.md", "notes/stra\u{17f}\u{17f}e.md"] {
        assert_eq!(
            check(root, path, AgentTarget::New),
            Err(WriteRefusal::Exists {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
    // A tracked folder the Mac takes this one for.
    assert_eq!(
        check(root, "\u{17f}essions/new.md", AgentTarget::New),
        Err(WriteRefusal::NameTaken {
            name: "\u{17f}essions".to_owned()
        })
    );
    // On this disk, untracked: the same file on the Mac.
    write(root, "notes/\u{3a3}\u{3a3}.md", "a person's own\n");
    write(root, "notes/ss.md", "a person's own\n");
    for path in [
        "notes/\u{3c3}\u{3c2}.md",
        "notes/\u{df}.md",
        "notes/\u{1e9e}.md",
    ] {
        assert_eq!(
            check(root, path, AgentTarget::New),
            Err(WriteRefusal::Exists {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
    // What APFS keeps apart is still a new path.
    assert_eq!(
        check(root, "notes/s\u{200d}s.md", AgentTarget::New),
        Ok("notes/s\u{200d}s.md".to_owned())
    );
}

/// R603-5: an indexed blob that cannot be read may be a pointer, so whether
/// the path is stored through LFS cannot be told: refused, never allowed.
#[test]
fn an_indexed_file_whose_blob_is_gone_is_undecidable() {
    let dir = drive();
    let root = dir.path();
    write(root, "notes/a.md", POINTER);
    git(root, &["add", "notes/a.md"]);
    let out = std::process::Command::new("git")
        .args(["rev-parse", ":notes/a.md"])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    let oid = String::from_utf8(out.stdout).expect("utf-8");
    let oid = oid.trim();
    write(root, "notes/a.md", "the real text\n");
    std::fs::remove_file(root.join(".git/objects").join(&oid[..2]).join(&oid[2..]))
        .expect("rm object");
    let (refusal, _) = subpath(check(root, "notes/a.md", AgentTarget::Existing));
    assert!(
        matches!(&refusal, WriteRefusal::Undecidable { subpath, .. } if subpath == "notes/a.md"),
        "{refusal:?}"
    );
}

/// R603-6: the device that carries a write out decides it again over its own
/// checkout right before it commits — LFS routing included, since a
/// `.gitattributes` may route the path by then — and refuses bytes that are
/// themselves an LFS pointer.
#[test]
fn the_apply_check_decides_again_and_refuses_a_pointer_as_bytes() {
    let dir = drive();
    let root = dir.path();
    let scope = WriteScope::new("tgdrive", None);
    let excludes = ExcludeSet::new(&[]).expect("excludes");
    let apply = |path: &str, target: AgentTarget, bytes: Option<&[u8]>| {
        agent_apply_check(root, &scope, &excludes, path, target, bytes)
    };
    assert_eq!(
        apply("notes/a.md", AgentTarget::Existing, Some(b"new text\n")),
        Ok("notes/a.md".to_owned())
    );
    assert_eq!(
        apply("notes/new.md", AgentTarget::New, Some(POINTER.as_bytes())),
        Err(WriteRefusal::LfsRouted {
            subpath: "notes/new.md".to_owned()
        })
    );
    assert_eq!(
        apply("notes/new.md", AgentTarget::New, Some(b"")),
        Ok("notes/new.md".to_owned())
    );
    write(root, "notes/.gitattributes", "*.md filter=lfs\n");
    for (path, target) in [
        ("notes/a.md", AgentTarget::Existing),
        ("notes/new.md", AgentTarget::New),
    ] {
        assert_eq!(
            apply(path, target, Some(b"new text\n")),
            Err(WriteRefusal::LfsRouted {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
}

#[test]
fn a_new_path_is_refused_where_anything_already_is() {
    let dir = drive();
    let root = dir.path();
    let exists = |path: &str| {
        Err(WriteRefusal::Exists {
            subpath: path.to_owned(),
        })
    };
    assert_eq!(
        check(root, "notes/a.md", AgentTarget::New),
        exists("notes/a.md")
    );
    // A name differing only in case is the same file on the Mac.
    assert_eq!(
        check(root, "notes/A.md", AgentTarget::New),
        exists("notes/A.md")
    );
    // In the index, gone from the disk.
    std::fs::remove_file(root.join("notes/deep/b.md")).expect("rm");
    assert_eq!(
        check(root, "notes/deep/b.md", AgentTarget::New),
        exists("notes/deep/b.md")
    );
    // A file where a folder of the path would be.
    assert_eq!(
        check(root, "notes/a.md/c.md", AgentTarget::New),
        Err(WriteRefusal::NameTaken {
            name: "a.md".to_owned()
        })
    );
}

#[test]
fn an_existing_target_must_be_there() {
    let dir = drive();
    assert_eq!(
        check(dir.path(), "notes/nothing.md", AgentTarget::Existing),
        Err(WriteRefusal::Missing {
            subpath: "notes/nothing.md".to_owned()
        })
    );
}

#[test]
fn a_folder_is_refused_on_the_disk_and_in_the_index() {
    let dir = drive();
    let root = dir.path();
    for target in [AgentTarget::Existing, AgentTarget::New] {
        assert_eq!(
            check(root, "notes/deep", target),
            Err(WriteRefusal::IsDirectory {
                name: "deep".to_owned()
            })
        );
    }
    // Tracked files under it, the folder gone from the disk.
    assert_eq!(
        check(root, "gone", AgentTarget::New),
        Err(WriteRefusal::IsDirectory {
            name: "gone".to_owned()
        })
    );
}

#[test]
fn a_link_or_a_path_through_one_is_refused() {
    let dir = drive();
    let root = dir.path();
    let link = |path: &str| {
        Err(WriteRefusal::Link {
            subpath: path.to_owned(),
        })
    };
    assert_eq!(
        check(root, "notes/link.md", AgentTarget::Existing),
        link("notes/link.md")
    );
    assert_eq!(
        check(root, "linked/a.md", AgentTarget::Existing),
        link("linked/a.md")
    );
    assert_eq!(
        check(root, "linked/new.md", AgentTarget::New),
        link("linked/new.md")
    );
    // Tracked as a link, a regular file on this disk: the index decides.
    std::fs::remove_file(root.join("notes/link.md")).expect("rm link");
    write(root, "notes/link.md", "hello\n");
    assert_eq!(
        check(root, "notes/link.md", AgentTarget::Existing),
        link("notes/link.md")
    );
}

#[test]
fn a_path_that_is_not_a_plain_descendant_is_refused() {
    let dir = drive();
    let root = dir.path();
    for path in [
        "../x.md",
        "notes/../a.md",
        "/etc/passwd",
        "./notes/a.md",
        "notes//a.md",
        "",
    ] {
        assert_eq!(
            check(root, path, AgentTarget::New),
            Err(WriteRefusal::Escapes {
                subpath: path.to_owned()
            }),
            "{path}"
        );
    }
    let long = "n".repeat(MAX_NAME_BYTES + 1);
    assert_eq!(
        check(root, &format!("notes/{long}"), AgentTarget::New),
        Err(WriteRefusal::NameTooLong {
            name: long,
            bytes: MAX_NAME_BYTES + 1
        })
    );
}

#[test]
fn the_scopes_fences_hold_for_an_agent() {
    let dir = drive();
    let root = dir.path();
    let scope = WriteScope::new("tgdrive", None)
        .with_sessions(Some("60-sessions"))
        .with_sessions_closed(true)
        .with_agents(Some("80-agents"));
    let excludes = ExcludeSet::new(&[]).expect("excludes");
    assert_eq!(
        agent_write_check(
            root,
            &scope,
            &excludes,
            "80-agents/nixi/SOUL.md",
            AgentTarget::New
        ),
        Err(WriteRefusal::AgentHome {
            subpath: "80-agents/nixi/SOUL.md".to_owned()
        })
    );
    assert_eq!(
        agent_write_check(
            root,
            &scope,
            &excludes,
            "60-sessions/active/s/a.md",
            AgentTarget::New
        ),
        Err(WriteRefusal::SessionsZone {
            subpath: "60-sessions/active/s/a.md".to_owned()
        })
    );
}

#[test]
fn a_checkout_whose_index_cannot_be_read_refuses_rather_than_allows() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "a.md", "a\n");
    let (refusal, _) = subpath(check(dir.path(), "a.md", AgentTarget::Existing));
    assert!(
        matches!(refusal, WriteRefusal::Undecidable { ref subpath, .. } if subpath == "a.md"),
        "{refusal:?}"
    );
}

#[test]
fn the_new_refusals_say_what_an_agent_may_not_do() {
    let at = || "notes/x.md".to_owned();
    let cases = [
        (
            WriteRefusal::Machinery { subpath: at() },
            "\"notes/x.md\" is git's or keeper's own file, which an agent never writes.",
        ),
        (
            WriteRefusal::Ignored { subpath: at() },
            "\"notes/x.md\" is ignored by this drive, so it would never be part of it.",
        ),
        (
            WriteRefusal::LfsRouted { subpath: at() },
            "\"notes/x.md\" is kept in the drive's large-file store (LFS), which an agent never \
             writes.",
        ),
        (
            WriteRefusal::Untracked { subpath: at() },
            "\"notes/x.md\" is not a file this drive carries.",
        ),
        (
            WriteRefusal::Link { subpath: at() },
            "\"notes/x.md\" is or passes through a link; an agent writes only the file at its \
             own path.",
        ),
        (
            WriteRefusal::Exists { subpath: at() },
            "\"notes/x.md\" already exists.",
        ),
        (
            WriteRefusal::Undecidable {
                subpath: at(),
                reason: "no index".to_owned(),
            },
            "keeper could not tell whether \"notes/x.md\" may be written (no index), so an \
             agent may not write it.",
        ),
    ];
    for (refusal, sentence) in cases {
        assert_eq!(refusal.to_string(), sentence);
    }
}

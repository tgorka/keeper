//! [`ShellDriveWrites`] over a real checkout and keeper-sync's own engine
//! (109.27, contract § 5.2): each op is one authored commit carrying its
//! approval's trailer, found again by the trailer search; git's, keeper's
//! and LFS-routed paths are refused by keeper-sync's path rule.

use std::path::Path;
use std::sync::Arc;

use keeper_core::agents::drive_write::Refusal;
use keeper_core::hq::port::{DriveCommit, DriveCommitted, DriveTarget, DriveWrites};
use keeper_sync::engine::{blob_id, Engine};
use keeper_sync::platform::TestPlatform;
use keeper_sync::{SyncPlatform, SyncProfile};

use super::ShellDriveWrites;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Ana",
            "-c",
            "user.email=ana@example.org",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).expect("utf-8")
}

/// A drive `tgdrive` cloned from a bare remote: a note, a large-file rule,
/// its declaration; and the writer over an engine that keeps it.
struct Drive {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    writes: ShellDriveWrites,
}

fn drive() -> Option<Drive> {
    drive_with(|_| {})
}

/// [`drive`], its profile changed by `tune` before the engine keeps it.
fn drive_with(tune: impl FnOnce(&mut SyncProfile)) -> Option<Drive> {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote = dir.path().join("remote.git");
    let seed = dir.path().join("seed");
    let root = dir.path().join("work");
    std::fs::create_dir_all(&remote).expect("remote");
    git(&remote, &["init", "--bare", "-q"]);
    std::fs::create_dir_all(seed.join("80-agents")).expect("seed");
    std::fs::create_dir_all(seed.join("10-notes")).expect("seed");
    std::fs::write(
        seed.join("80-agents/_drive.toml"),
        "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"ana\"\n\
         owner = \"@ana:example.org\"\nreaders = [\"@ana:example.org\"]\n",
    )
    .expect("declare");
    std::fs::write(seed.join("10-notes/inbox.md"), "- [ ] milk\n").expect("note");
    std::fs::write(
        seed.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("attributes");
    git(&seed, &["init", "-q"]);
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "seed"]);
    git(&seed, &["push", "-q", remote.to_str()?, "HEAD:main"]);
    git(
        dir.path(),
        &["clone", "-q", remote.to_str()?, root.to_str()?],
    );

    let platform = Arc::new(TestPlatform::new(dir.path().join("data")));
    let engine = Arc::new(Engine::open(platform as Arc<dyn SyncPlatform>).ok()?);
    let mut profile = SyncProfile::new(
        "01JDRIVEPROFILE",
        "tgdrive",
        root.clone(),
        remote.to_string_lossy().into_owned(),
    );
    profile.agents = Some(Default::default());
    profile.sessions = Some(Default::default());
    tune(&mut profile);
    engine.upsert_profile(&profile).expect("upsert");
    let writes = ShellDriveWrites {
        engine: Box::new(move || Some(Arc::clone(&engine))),
        vault: Box::new(|_| None),
    };
    Some(Drive {
        _dir: dir,
        root,
        writes,
    })
}

impl Drive {
    /// The commit of one request, as keeper-core builds it (§ 5.2): the
    /// keys the path check answered, guarded by what they hold here.
    fn commit(
        &self,
        approval: &str,
        path: &str,
        writes: Vec<(String, Option<Vec<u8>>)>,
        moves: Vec<(String, String)>,
        target: DriveTarget,
    ) -> DriveCommitted {
        let at = self
            .writes
            .target("tgdrive", path, target)
            .expect("the path may be written");
        let guard = at.disk.map(|(blob, _)| blob);
        assert_eq!(at.head, guard, "the file at HEAD is the one on the disk");
        self.writes
            .commit(
                "tgdrive",
                DriveCommit {
                    writes,
                    moves,
                    guards: vec![(at.key, guard)],
                    subject: format!("Nixi: {approval}"),
                    approval: approval.to_owned(),
                },
            )
            .expect("committed")
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(rel)).ok()
    }

    /// The commit `commit` is `HEAD`, is one commit on top of `before`, and
    /// carries `approval`'s trailer; the trailer search finds it.
    fn one_commit(&self, before: &str, commit: &DriveCommitted, approval: &str) {
        let DriveCommitted::Committed(commit) = commit else {
            panic!("{approval}: a commit, not {commit:?}");
        };
        assert_eq!(git(&self.root, &["rev-parse", "HEAD"]).trim(), commit);
        let log = git(
            &self.root,
            &["log", "--format=%H", &format!("{before}..HEAD")],
        );
        assert_eq!(log.lines().count(), 1, "{approval}: one commit");
        let message = git(&self.root, &["log", "-1", "--format=%B", commit]);
        assert_eq!(
            message.trim_end().lines().last(),
            Some(format!("Approval-Record: {approval}").as_str()),
            "{approval}: {message}"
        );
        assert_eq!(
            self.writes.carried("tgdrive", approval),
            Ok(Some(commit.clone()))
        );
    }

    fn head(&self) -> String {
        git(&self.root, &["rev-parse", "HEAD"]).trim().to_owned()
    }
}

#[test]
fn each_op_is_one_commit_carrying_its_approval() {
    let Some(d) = drive() else { return };
    assert_eq!(d.writes.keeps("tgdrive"), Ok(()));
    let inbox = "10-notes/inbox.md".to_owned();

    let before = d.head();
    let made = d.commit(
        "01JCREATE",
        "10-notes/new.md",
        vec![("10-notes/new.md".to_owned(), Some(b"hi\n".to_vec()))],
        Vec::new(),
        DriveTarget::New,
    );
    d.one_commit(&before, &made, "01JCREATE");
    assert_eq!(d.read("10-notes/new.md").as_deref(), Some("hi\n"));

    for (approval, text) in [
        ("01JREPLACE", "- [ ] eggs\n"),
        ("01JEDIT", "- [x] eggs\n"),
        ("01JAPPEND", "- [x] eggs\n- [ ] tea"),
    ] {
        let before = d.head();
        let made = d.commit(
            approval,
            &inbox,
            vec![(inbox.clone(), Some(text.as_bytes().to_vec()))],
            Vec::new(),
            DriveTarget::Existing,
        );
        d.one_commit(&before, &made, approval);
        assert_eq!(d.read(&inbox).as_deref(), Some(text), "{approval}");
    }

    let before = d.head();
    let made = d.commit(
        "01JMOVE",
        "10-notes/new.md",
        Vec::new(),
        vec![("10-notes/new.md".to_owned(), "20-done/new.md".to_owned())],
        DriveTarget::Existing,
    );
    d.one_commit(&before, &made, "01JMOVE");
    assert_eq!(d.read("10-notes/new.md"), None);
    assert_eq!(d.read("20-done/new.md").as_deref(), Some("hi\n"));

    let before = d.head();
    let made = d.commit(
        "01JDELETE",
        &inbox,
        vec![(inbox.clone(), None)],
        Vec::new(),
        DriveTarget::Existing,
    );
    d.one_commit(&before, &made, "01JDELETE");
    assert_eq!(d.read(&inbox), None);
    assert_eq!(
        git(&d.root, &["status", "--porcelain"]),
        "",
        "nothing left on the disk"
    );
    assert_eq!(d.writes.carried("tgdrive", "01JNEVER"), Ok(None));
}

/// A file changed since it was read is not written over.
#[test]
fn a_file_changed_since_it_was_read_is_guarded() {
    let Some(d) = drive() else { return };
    let inbox = "10-notes/inbox.md";
    let read = d
        .writes
        .target("tgdrive", inbox, DriveTarget::Existing)
        .expect("readable");
    assert_eq!(
        read.disk.as_ref().map(|(blob, _)| blob.clone()),
        Some(blob_id(b"- [ ] milk\n"))
    );
    std::fs::write(d.root.join(inbox), "- [ ] milk\n- [ ] bread\n").expect("a person's save");
    let before = d.head();
    let done = d
        .writes
        .commit(
            "tgdrive",
            DriveCommit {
                writes: vec![(inbox.to_owned(), Some(b"- [x] milk\n".to_vec()))],
                moves: Vec::new(),
                guards: vec![(inbox.to_owned(), read.disk.map(|(blob, _)| blob))],
                subject: "Nixi: edit".to_owned(),
                approval: "01JSTALE".to_owned(),
            },
        )
        .expect("answered");
    assert_eq!(done, DriveCommitted::Guarded(inbox.to_owned()));
    assert_eq!(d.head(), before);
    assert_eq!(d.read(inbox).as_deref(), Some("- [ ] milk\n- [ ] bread\n"));
}

/// keeper-sync's rule, verbatim: git's and keeper's own files, LFS-routed
/// paths and folders are refused; a missing file is absent, a taken one
/// exists; a drive this device does not keep is not kept.
#[test]
fn machinery_lfs_and_folders_are_refused() {
    let Some(d) = drive() else { return };
    let refused =
        |path: &str, target: DriveTarget| d.writes.target("tgdrive", path, target).expect_err(path);
    for path in [
        ".keeper/x.md",
        ".git/config",
        "10-notes/.gitattributes",
        ".gitignore",
    ] {
        let Refusal::Path { sentence } = refused(path, DriveTarget::New) else {
            panic!("{path}: a path refusal");
        };
        assert_eq!(
            sentence,
            format!("\"{path}\" is git's or keeper's own file, which an agent never writes."),
        );
    }
    let Refusal::Path { sentence } = refused("media/talk.bin", DriveTarget::New) else {
        panic!("an LFS refusal");
    };
    assert_eq!(
        sentence,
        "\"media/talk.bin\" is kept in the drive's large-file store (LFS), which an agent never \
         writes."
    );
    assert!(matches!(
        refused("10-notes", DriveTarget::Existing),
        Refusal::Folder { .. }
    ));
    assert!(matches!(
        refused("10-notes/none.md", DriveTarget::Existing),
        Refusal::Absent { .. }
    ));
    assert!(matches!(
        refused("10-notes/inbox.md", DriveTarget::New),
        Refusal::Exists { .. }
    ));
    assert_eq!(
        d.writes.keeps("neuradrive"),
        Err(keeper_core::hq::port::DriveAbsent::NotKept)
    );
}

/// The edit a card showed for `10-notes/inbox.md`, as keeper-core builds it.
fn inbox_edit(d: &Drive, approval: &str) -> DriveCommit {
    let read = d
        .writes
        .target("tgdrive", "10-notes/inbox.md", DriveTarget::Existing)
        .expect("the card's check passes");
    DriveCommit {
        writes: vec![(read.key.clone(), Some(b"- [x] milk\n".to_vec()))],
        moves: Vec::new(),
        guards: vec![(read.key, read.disk.map(|(blob, _)| blob))],
        subject: "Nixi: edit".to_owned(),
        approval: approval.to_owned(),
    }
}

/// Review-603 item 6: a `.gitattributes` that starts routing `*.md` through
/// LFS between showing the card and pressing Allow (a person's commit, or
/// a pull) is read again right before the commit: nothing is written, no
/// LFS object made, and the sentence is keeper-sync's.
#[test]
fn a_gitattributes_change_after_the_card_refuses_the_commit() {
    let Some(d) = drive() else { return };
    let commit = inbox_edit(&d, "01JATTRS");
    std::fs::write(
        d.root.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n*.md filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("attributes");
    git(&d.root, &["commit", "-q", "-am", "route notes through lfs"]);
    let before = d.head();
    assert_eq!(
        d.writes.commit("tgdrive", commit),
        Err(
            "\"10-notes/inbox.md\" is kept in the drive's large-file store (LFS), which an agent \
             never writes."
                .to_owned()
        )
    );
    assert_eq!(d.head(), before, "no commit");
    assert_eq!(d.read("10-notes/inbox.md").as_deref(), Some("- [ ] milk\n"));
    assert!(
        !d.root.join(".git/lfs/objects").exists()
            || walk_files(&d.root.join(".git/lfs/objects")) == 0,
        "no LFS object"
    );
}

/// What `commit_paths` would route by size — the profile's LFS threshold
/// over the new text — is refused too: no agent write makes an LFS object.
#[test]
fn a_post_image_over_the_lfs_threshold_is_refused() {
    let Some(d) = drive_with(|profile| profile.lfs_threshold_bytes = 8) else {
        return;
    };
    let commit = inbox_edit(&d, "01JSIZE");
    let before = d.head();
    assert_eq!(
        d.writes.commit("tgdrive", commit),
        Err(
            "\"10-notes/inbox.md\" is kept in the drive's large-file store (LFS), which an agent \
             never writes."
                .to_owned()
        )
    );
    assert_eq!(d.head(), before);
}

fn walk_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    walk_files(&path)
                } else {
                    1
                }
            })
            .sum()
    })
}

/// The apply-time check (§ 5.1 as amended): new text that is itself an LFS
/// pointer is refused, whatever the path's attributes say.
#[test]
fn a_post_image_that_is_an_lfs_pointer_is_refused() {
    let Some(d) = drive() else { return };
    let mut commit = inbox_edit(&d, "01JPOINTER");
    commit.writes[0].1 = Some(
        b"version https://git-lfs.github.com/spec/v1\noid sha256:\
          4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393\nsize 12345\n"
            .to_vec(),
    );
    let before = d.head();
    assert_eq!(
        d.writes.commit("tgdrive", commit),
        Err(
            "\"10-notes/inbox.md\" is kept in the drive's large-file store (LFS), which an agent \
             never writes."
                .to_owned()
        )
    );
    assert_eq!(d.head(), before);
}

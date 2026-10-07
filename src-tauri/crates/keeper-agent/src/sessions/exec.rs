//! The lifecycle executor: plans run with a journal beside them (AD-111,
//! NFR-38, AD-368).
//!
//! `keeper_core::sessions::plan` compiles; this runs. One plan at a time per
//! zone ([`ZoneLock`]: a process-wide mutex keyed by the zone's canonical root
//! and a file lock a second process waits on), each step idempotent, and the
//! journal row in `<zone>/.keeper/sessions-journal.json` written BEFORE the
//! first step and cleared AFTER the last — so a crash leaves a resumable
//! record naming the verb, the plan and the completed prefix. Whoever takes
//! the zone next finishes that record first ([`hold`]), before it reads or
//! plans anything, and an incomplete journal resumes by re-running the
//! remaining steps; idempotency is what makes "re-run" the whole recovery
//! story. At start a host also calls [`super::resume_all`], so a crash is
//! finished without waiting for the next verb.
//!
//! Every step's path is zone-relative twice over: lexically, and on the disk —
//! the deepest part of it that exists already must resolve inside the zone, so
//! a symlinked folder cannot carry a write out of it. A step that moves or
//! trashes a path acts on the link itself, so only the folder holding it has
//! to be inside.
//!
//! Nothing here decides. A plan arrives compiled; refusals (`GuardedWrite`
//! mismatch, a missing source) surface as errors the caller sentences.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use keeper_core::sessions::plan::{Emptying, Plan, PlanStep};

use super::lock::ZoneLock;

/// The journal file, zone-relative. Inside `.keeper/` so it never syncs.
const JOURNAL_REL: &str = ".keeper/sessions-journal.json";

/// One persisted run: the plan and how far it got.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JournalRow {
    plan: Plan,
    /// Steps completed, a prefix of `plan.steps`.
    done: usize,
}

/// Everything the executor can refuse or fail with. `Refused` is a decision
/// (the caller re-plans); `Failed` is the disk saying no.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("{0}")]
    Refused(String),
    #[error("step {step} of {verb} failed: {reason}")]
    Failed {
        verb: String,
        step: usize,
        reason: String,
    },
}

/// Run a plan against a zone root, journaled, holding the zone. Synchronous —
/// lifecycle verbs are single-digit file counts, and the callers run on
/// blocking tasks.
pub fn run(zone: &Path, plan: Plan) -> Result<(), ExecError> {
    let held = hold(zone)?;
    run_held(plan, &held)
}

/// Hold a zone for one plan, waiting for whoever holds it now, and finish
/// any plan a crash left in it — so a verb that holds the zone reads it as
/// every finished plan left it, not halfway through one.
pub fn hold(zone: &Path) -> Result<ZoneLock, ExecError> {
    let held = ZoneLock::acquire(zone).map_err(|error| ExecError::Failed {
        verb: "lock".to_owned(),
        step: 0,
        reason: format!("the sessions zone could not be locked: {error}"),
    })?;
    resume_held(held.zone())?;
    Ok(held)
}

/// [`run`] for a caller that already holds the zone through [`hold`] — a verb
/// whose own reads must see the zone as the plan will find it. The plan runs
/// in the zone the lock holds.
pub fn run_held(plan: Plan, held: &ZoneLock) -> Result<(), ExecError> {
    let zone = held.zone();
    let journal = zone.join(JOURNAL_REL);
    // `hold` finished any earlier run; a lock taken with `ZoneLock::acquire`
    // directly has not, and a journal written over is a crash never finished.
    resume_held(zone)?;
    write_journal(
        &journal,
        &JournalRow {
            plan: plan.clone(),
            done: 0,
        },
    )?;
    run_from(zone, &journal, plan, 0)
}

/// Resume the zone's journaled run, if one is pending, holding the zone.
/// A journal that cannot be read is renamed aside rather than deleted —
/// evidence, not litter. A zone with no journal is only looked at: no lock
/// file is made and nothing waits.
pub fn resume(zone: &Path) -> Result<(), ExecError> {
    if !zone.join(JOURNAL_REL).exists() {
        return Ok(());
    }
    hold(zone).map(drop)
}

fn resume_held(zone: &Path) -> Result<(), ExecError> {
    let journal = zone.join(JOURNAL_REL);
    if !journal.exists() {
        return Ok(());
    }
    let row: JournalRow = match std::fs::read_to_string(&journal)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(row) => row,
        None => {
            let aside = journal.with_extension("json.unreadable");
            let _ = std::fs::rename(&journal, &aside);
            tracing::warn!(path = %aside.display(), "sessions: unreadable journal set aside");
            return Ok(());
        }
    };
    tracing::info!(
        verb = %row.plan.verb,
        session = %row.plan.session,
        done = row.done,
        total = row.plan.steps.len(),
        "sessions: resuming a journaled run"
    );
    let done = row.done;
    run_from(zone, &journal, row.plan, done)
}

fn run_from(zone: &Path, journal: &Path, plan: Plan, from: usize) -> Result<(), ExecError> {
    for (index, step) in plan.steps.iter().enumerate().skip(from) {
        run_step(zone, step).map_err(|error| match error {
            ExecError::Refused(_) => {
                // A refusal abandons the plan: the journal clears, because a
                // re-run would refuse identically and the caller re-plans.
                let _ = std::fs::remove_file(journal);
                error
            }
            other => other,
        })?;
        write_journal(
            journal,
            &JournalRow {
                plan: plan.clone(),
                done: index + 1,
            },
        )?;
    }
    let cleared = |error: std::io::Error| ExecError::Failed {
        verb: plan.verb.clone(),
        step: plan.steps.len(),
        reason: format!("could not clear the journal: {error}"),
    };
    std::fs::remove_file(journal).map_err(cleared)?;
    // The removal on the disk too, or a power cut brings back a journal
    // whose remaining steps run over what came after.
    sync_dir(journal.parent().unwrap_or(zone)).map_err(cleared)
}

/// The most entries [`inventory`] lists before it says it stopped.
pub const INVENTORY_CAP: usize = 4096;

/// Every entry under `top` of the folder `dir` — `top`-relative paths
/// `/`-joined from `top`, hidden ones, links and special files too, with
/// `top`'s own `.gitkeep` (which an emptying keeps) aside — each with its
/// stamp ([`Folder::stamp`]). What an emptying of `top` removes, and what a
/// choice about it is bound to. `top` itself must be a real folder: a link
/// there is told, never followed, and so is anything else that is not a
/// folder. What could not be listed or looked at, and a listing cut at
/// [`INVENTORY_CAP`], is told in `problems`, never left out silently; `top`
/// absent is no entries.
pub fn inventory(dir: &Path, top: &str, problems: &mut Vec<String>) -> BTreeMap<String, String> {
    match std::fs::symlink_metadata(dir.join(top)) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
        Ok(meta) if meta.file_type().is_symlink() => {
            problems.push(format!(
                "{top}/ is a link, and keeper does not follow it to list what it holds"
            ));
            return BTreeMap::new();
        }
        Ok(_) => {
            problems.push(format!("{top} is not a folder"));
            return BTreeMap::new();
        }
        Err(error) => {
            problems.push(format!("{top}/ could not be looked at: {error}"));
            return BTreeMap::new();
        }
    }
    match Folder::open(dir).and_then(|dir| dir.child(top)) {
        Ok(root) => walk(&root, top, problems, &mut BTreeMap::new()),
        Err(error) => {
            problems.push(format!("{top}/ could not be looked at: {error}"));
            BTreeMap::new()
        }
    }
}

/// [`inventory`] of the held folder `root`, named `top`. Every folder below
/// it is reached from `root` one real folder at a time ([`down`]), never
/// through a link, and recorded in `folders` with its identity; one found
/// again as another folder is told, not listed.
fn walk(
    root: &Folder,
    top: &str,
    problems: &mut Vec<String>,
    folders: &mut BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut todo = vec![top.to_owned()];
    while let Some(rel) = todo.pop() {
        let folder = match down(root, &rel, folders) {
            Ok(Some(folder)) => folder,
            Ok(None) => {
                problems.push(format!("{rel}/ changed while it was being looked at"));
                continue;
            }
            Err(error) => {
                problems.push(format!("{rel}/ could not be listed: {error}"));
                continue;
            }
        };
        if let Some(identity) = folder.identity() {
            folders.insert(rel.clone(), identity);
        }
        let entries = match folder.names() {
            Ok(entries) => entries,
            Err(error) => {
                problems.push(format!("{rel}/ could not be listed: {error}"));
                continue;
            }
        };
        for name in entries {
            let name = match name {
                Ok(name) => name,
                Err(error) => {
                    problems.push(format!("an entry of {rel}/ could not be read: {error}"));
                    continue;
                }
            };
            let Ok(name) = name.into_string() else {
                problems.push(format!("{rel}/ holds a name that is not UTF-8"));
                continue;
            };
            if rel == top && name == ".gitkeep" {
                continue;
            }
            if out.len() >= INVENTORY_CAP {
                problems.push(format!(
                    "{top}/ holds more than {INVENTORY_CAP} entries; only {INVENTORY_CAP} were looked at."
                ));
                return out;
            }
            let child = format!("{rel}/{name}");
            match folder.stamp(&name) {
                Ok(stamp) => {
                    if stamp == "dir" {
                        todo.push(child.clone());
                    }
                    out.insert(child, stamp);
                }
                Err(error) => problems.push(format!("{child} could not be looked at: {error}")),
            }
        }
    }
    out
}

/// The folder `rel` — `/`-joined from `root`'s own name — reached from
/// `root` one real folder at a time, a link on the way refused, never
/// followed; `Ok(None)` when a folder on the way is not the one `folders`
/// recorded under its name.
fn down(
    root: &Folder,
    rel: &str,
    folders: &BTreeMap<String, String>,
) -> std::io::Result<Option<Folder>> {
    let mut parts = rel.split('/');
    let mut so_far = parts.next().unwrap_or_default().to_owned();
    let mut at = root.dup()?;
    for part in parts {
        at = at.child(part)?;
        so_far.push('/');
        so_far.push_str(part);
        if folders
            .get(&so_far)
            .is_some_and(|was| at.identity().as_ref() != Some(was))
        {
            return Ok(None);
        }
    }
    Ok(Some(at))
}

/// A regular file's stamp, `file:<length>:<mtime ns>`; `since_epoch` is
/// `None` for a file changed before 1970.
fn file_stamp(len: u64, since_epoch: Option<std::time::Duration>) -> std::io::Result<String> {
    let at = since_epoch
        .ok_or_else(|| std::io::Error::other("it was changed before 1970"))?
        .as_nanos();
    Ok(format!("file:{len}:{at}"))
}

/// A folder an emptying or an inventory holds. On Unix an open descriptor:
/// every look, listing and removal below it is relative to the folder that
/// was opened, each folder further down opened without following a link,
/// so a folder renamed or linked into its place afterwards is never
/// reached. Elsewhere its path (DW-1002).
#[cfg(unix)]
struct Folder(std::os::fd::OwnedFd);

#[cfg(unix)]
impl Folder {
    const DIR: rustix::fs::OFlags = rustix::fs::OFlags::RDONLY
        .union(rustix::fs::OFlags::DIRECTORY)
        .union(rustix::fs::OFlags::CLOEXEC);

    /// The folder at `path`, links on the way followed: for a place keeper
    /// itself names (the zone, a session folder it holds).
    fn open(path: &Path) -> std::io::Result<Folder> {
        Ok(Folder(rustix::fs::open(
            path,
            Self::DIR,
            rustix::fs::Mode::empty(),
        )?))
    }

    /// The folder `name` in this one, refused when it is a link or not a
    /// folder.
    fn child(&self, name: &str) -> std::io::Result<Folder> {
        let flags = Self::DIR | rustix::fs::OFlags::NOFOLLOW;
        Ok(Folder(rustix::fs::openat(
            &self.0,
            name,
            flags,
            rustix::fs::Mode::empty(),
        )?))
    }

    fn dup(&self) -> std::io::Result<Folder> {
        Ok(Folder(self.0.try_clone()?))
    }

    /// [`identity`] of the folder held.
    fn identity(&self) -> Option<String> {
        rustix::fs::fstat(&self.0)
            .ok()
            .map(|stat| identity_of(&stat))
    }

    /// The stamp of the entry `name` here, never following a link: a regular
    /// file's `file:<length>:<mtime ns>`, a link's `link:<where it points>`,
    /// a folder's `dir`, anything else `other`.
    #[allow(clippy::unnecessary_cast)] // `stat`'s field types differ between targets
    fn stamp(&self, name: &str) -> std::io::Result<String> {
        use rustix::fs::{AtFlags, FileType, RawMode};
        let stat = rustix::fs::statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW)?;
        Ok(match FileType::from_raw_mode(stat.st_mode as RawMode) {
            FileType::Directory => "dir".to_owned(),
            FileType::Symlink => {
                let to = rustix::fs::readlinkat(&self.0, name, Vec::new())?;
                format!("link:{}", to.to_string_lossy())
            }
            FileType::RegularFile => {
                let since_epoch = u64::try_from(stat.st_mtime as i64)
                    .ok()
                    .map(|secs| std::time::Duration::new(secs, stat.st_mtime_nsec as u32));
                return file_stamp(stat.st_size as u64, since_epoch);
            }
            _ => "other".to_owned(),
        })
    }

    /// The names this folder holds, `.` and `..` aside.
    fn names(&self) -> std::io::Result<impl Iterator<Item = std::io::Result<std::ffi::OsString>>> {
        use std::os::unix::ffi::OsStrExt as _;
        let mut dir = rustix::fs::Dir::read_from(&self.0)?;
        Ok(
            std::iter::from_fn(move || dir.read()).filter_map(|entry| match entry {
                Ok(entry) => match entry.file_name().to_bytes() {
                    b"." | b".." => None,
                    name => Some(Ok(std::ffi::OsStr::from_bytes(name).to_owned())),
                },
                Err(error) => Some(Err(error.into())),
            }),
        )
    }

    /// Unlink `name` here — a folder only when `folder`, and only empty.
    fn remove(&self, name: &str, folder: bool) -> std::io::Result<()> {
        let flags = if folder {
            rustix::fs::AtFlags::REMOVEDIR
        } else {
            rustix::fs::AtFlags::empty()
        };
        Ok(rustix::fs::unlinkat(&self.0, name, flags)?)
    }

    /// This folder's entry list made durable.
    fn sync(&self) -> std::io::Result<()> {
        Ok(rustix::fs::fsync(&self.0)?)
    }

    /// An empty file `name` here unless something by that name is there —
    /// never written through a link.
    fn keep(&self, name: &str) -> std::io::Result<()> {
        use rustix::fs::{Mode, OFlags};
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mode = Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH;
        match rustix::fs::openat(&self.0, name, flags, mode) {
            Ok(_) | Err(rustix::io::Errno::EXIST) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(not(unix))]
struct Folder(PathBuf);

#[cfg(not(unix))]
impl Folder {
    fn open(path: &Path) -> std::io::Result<Folder> {
        if std::fs::metadata(path)?.is_dir() {
            Ok(Folder(path.to_owned()))
        } else {
            Err(std::io::Error::other("not a folder"))
        }
    }

    fn child(&self, name: &str) -> std::io::Result<Folder> {
        let path = self.0.join(name);
        if std::fs::symlink_metadata(&path)?.file_type().is_dir() {
            Ok(Folder(path))
        } else {
            Err(std::io::Error::other(format!(
                "{name} is a link or not a folder"
            )))
        }
    }

    fn dup(&self) -> std::io::Result<Folder> {
        Ok(Folder(self.0.clone()))
    }

    fn identity(&self) -> Option<String> {
        identity(&self.0)
    }

    fn stamp(&self, name: &str) -> std::io::Result<String> {
        let path = self.0.join(name);
        let meta = std::fs::symlink_metadata(&path)?;
        let kind = meta.file_type();
        Ok(if kind.is_dir() {
            "dir".to_owned()
        } else if kind.is_symlink() {
            format!("link:{}", std::fs::read_link(&path)?.to_string_lossy())
        } else if kind.is_file() {
            let since_epoch = meta.modified()?.duration_since(std::time::UNIX_EPOCH).ok();
            return file_stamp(meta.len(), since_epoch);
        } else {
            "other".to_owned()
        })
    }

    fn names(&self) -> std::io::Result<impl Iterator<Item = std::io::Result<std::ffi::OsString>>> {
        Ok(std::fs::read_dir(&self.0)?.map(|entry| entry.map(|entry| entry.file_name())))
    }

    fn remove(&self, name: &str, folder: bool) -> std::io::Result<()> {
        let path = self.0.join(name);
        if folder {
            std::fs::remove_dir(path)
        } else {
            std::fs::remove_file(path)
        }
    }

    fn sync(&self) -> std::io::Result<()> {
        Ok(())
    }

    fn keep(&self, name: &str) -> std::io::Result<()> {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.0.join(name))
        {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Folder {
    /// The folder at the zone-relative `path` of `zone`, reached from the
    /// zone one real folder at a time — a link on the way refused.
    fn reach(zone: &Path, path: &str) -> std::io::Result<Folder> {
        path.split('/')
            .try_fold(Folder::open(zone)?, |at, part| at.child(part))
    }
}

/// What the folder at `path` is on the disk, for an emptying to be bound
/// to ([`keeper_core::sessions::plan::Emptying::root`]): a real folder's
/// identity — `<device>:<inode>` where the platform tells one — or `None`
/// when no real folder is there (absent, a link, a file).
pub fn identity(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use rustix::fs::{FileType, RawMode};
        let stat = rustix::fs::lstat(path).ok()?;
        #[allow(clippy::unnecessary_cast)] // `stat`'s field types differ between targets
        let kind = FileType::from_raw_mode(stat.st_mode as RawMode);
        (kind == FileType::Directory).then(|| identity_of(&stat))
    }
    #[cfg(not(unix))]
    {
        let meta = std::fs::symlink_metadata(path).ok()?;
        meta.file_type().is_dir().then(|| "dir".to_owned())
    }
}

/// `<device>:<inode>` of what `stat` describes, as [`identity`] tells it.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // `stat`'s field types differ between targets
fn identity_of(stat: &rustix::fs::Stat) -> String {
    format!("{}:{}", stat.st_dev as u64, stat.st_ino as u64)
}

/// One idempotent step. The idempotency table is the resume contract:
/// re-running a completed step is a no-op, never an error.
fn run_step(zone: &Path, step: &PlanStep) -> Result<(), ExecError> {
    let failed = |reason: String| ExecError::Refused(reason);
    match step {
        PlanStep::MkDir { path } => {
            make_dirs(&rel(zone, path)?).map_err(|e| failed(format!("mkdir {path}: {e}")))
        }
        PlanStep::MkDirNew { path } => {
            let dir = rel_link(zone, path)?;
            match std::fs::create_dir(&dir) {
                Ok(()) => {}
                // A resume finds the folder its own run made: a real one.
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists
                        && dir
                            .symlink_metadata()
                            .is_ok_and(|meta| meta.file_type().is_dir()) => {}
                Err(e) => return Err(failed(format!("mkdir {path}: {e}"))),
            }
            sync_parent(&dir).map_err(|e| failed(format!("mkdir {path}: {e}")))
        }
        PlanStep::CopyFile { from, to } => {
            let source = rel(zone, from)?;
            let target = rel(zone, to)?;
            if !source.exists() && target.exists() {
                // The copy already happened and the source has since gone
                // (an archive resume after its own EmptyDirKeep): complete.
                return Ok(());
            }
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::copy(&source, &target)
                .map(|_| ())
                .map_err(|e| failed(format!("copy {from} → {to}: {e}")))
        }
        PlanStep::CopyChecked { from, to, sha256 } => {
            let source = rel(zone, from)?;
            let target = rel(zone, to)?;
            if hash_file(&target).is_ok_and(|held| held == *sha256) {
                return sync_parent(&target)
                    .map_err(|e| failed(format!("copy {from} → {to}: {e}")));
            }
            copy_checked(&source, &target, sha256)
                .map_err(|e| failed(format!("copy {from} → {to}: {e}")))?
                .then_some(())
                .ok_or_else(|| {
                    ExecError::Refused(format!(
                        "{from} changed after it was checked; nothing was copied — try again"
                    ))
                })
        }
        PlanStep::WriteFile { path, content } => write_durable(&rel(zone, path)?, content)
            .map_err(|e| failed(format!("write {path}: {e}"))),
        PlanStep::CreateFile { path, content } => {
            let target = rel(zone, path)?;
            match target.symlink_metadata() {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(failed(format!("write {path}: {e}"))),
                // A resume re-running a completed create sees its own bytes.
                Ok(_) if std::fs::read(&target).is_ok_and(|bytes| bytes == content.as_bytes()) => {
                    return sync_parent(&target).map_err(|e| failed(format!("write {path}: {e}")))
                }
                Ok(_) => {
                    return Err(ExecError::Refused(format!(
                    "{path} appeared while this was being planned; nothing was written — try again"
                )))
                }
            }
            write_durable(&target, content).map_err(|e| failed(format!("write {path}: {e}")))
        }
        PlanStep::GuardedWrite {
            path,
            expect_len,
            expect_sha256,
            content,
        } => {
            let target = rel(zone, path)?;
            let current = std::fs::read_to_string(&target)
                .map_err(|e| failed(format!("read {path}: {e}")))?;
            let changed = current.len() != *expect_len
                || expect_sha256
                    .as_ref()
                    .is_some_and(|sha| *sha != keeper_core::sessions::plan::sha256_hex(&current));
            if changed {
                // Idempotency first: a resume re-running a completed guarded
                // write sees its own output. Then the real guard.
                if current == *content {
                    return Ok(());
                }
                return Err(ExecError::Refused(format!(
                    "{path} changed while this was being planned; nothing was written — try again"
                )));
            }
            write_durable(&target, content).map_err(|e| failed(format!("write {path}: {e}")))
        }
        PlanStep::MoveDir { from, to } => {
            let source = rel_link(zone, from)?;
            let target = rel(zone, to)?;
            if !source.exists() && target.exists() {
                return Ok(()); // already moved
            }
            // A target that exists AND is a different directory is the refusal
            // this step is here to make. A target that exists and IS the source
            // is not: on APFS and NTFS `_template/interview` exists the moment
            // `_template/Interview` does, so a case-only rename — the one that
            // normalises a hand-made name — would be refused by its own source.
            if target.exists() && !same_directory(&target, &source) {
                return Err(ExecError::Refused(format!(
                    "{to} already exists; nothing was moved"
                )));
            }
            std::fs::rename(&source, &target)
                .and_then(|()| sync_moved(&source, &target))
                .map_err(|e| failed(format!("move {from} → {to}: {e}")))
        }
        PlanStep::PublishDir { from, to, files } => {
            let source = rel_link(zone, from)?;
            let target = rel_link(zone, to)?;
            let there = |path: &Path| path.symlink_metadata().is_ok();
            let exactly = |dir: &Path| {
                regular_files(dir).map(|found| {
                    found.len() == files.len()
                        && found.iter().all(|(name, bytes)| {
                            files.get(name).map(String::as_str)
                                == Some(keeper_core::agents::approval::sha256_hex(bytes).as_str())
                        })
                })
            };
            if !there(&source) && there(&target) {
                // Already moved — and only its own generation is.
                return match exactly(&target) {
                    Ok(true) => sync_moved(&source, &target)
                        .map_err(|e| failed(format!("publish {from} → {to}: {e}"))),
                    Ok(false) => Err(ExecError::Refused(format!(
                        "{to} is not what this plan published; nothing was moved"
                    ))),
                    Err(reason) => Err(ExecError::Refused(format!(
                        "{to}: {reason}; nothing was moved"
                    ))),
                };
            }
            if there(&target) {
                return Err(ExecError::Refused(format!(
                    "{to} already exists; nothing was moved"
                )));
            }
            match exactly(&source) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(ExecError::Refused(format!(
                        "{from} does not hold exactly what was staged; nothing was published"
                    )))
                }
                Err(reason) => {
                    return Err(ExecError::Refused(format!(
                        "{from}: {reason}; nothing was published"
                    )))
                }
            }
            std::fs::rename(&source, &target)
                .and_then(|()| sync_moved(&source, &target))
                .map_err(|e| failed(format!("publish {from} → {to}: {e}")))
        }
        PlanStep::MoveFile { from, to } => {
            let source = rel_link(zone, from)?;
            let target = rel(zone, to)?;
            // **No already-moved short-circuit here, unlike `MoveDir` above.**
            // That one infers "this plan already ran" from a gone source and a
            // present target, and the inference holds where it lives: a resumed
            // journal's only writer produced that exact pair. `MoveFile` has no
            // crash-resume caller — its one caller is
            // `sessions_template_rename_entry`, which stats the source through
            // `entry_kind` and runs the plan straight away — so the same test
            // proves nothing about the target: if the source disappears in that
            // window (a sync pull, an agent, a move in Finder) and the typed
            // destination happens to name an existing neighbour, the step would
            // answer Ok, clear the journal, and hand the room the subpath of a
            // file it never touched. A missing source is a stale list, and the
            // rename error is what says so.
            // `MoveDir`'s guard above, verbatim in its reasoning and sharing its
            // predicate: a target that exists AND is a different file is a
            // neighbour a rename must not eat, while a target that exists and IS
            // the source is the case-only rename that normalises a hand-made
            // name — `_template/x/About.md` → `about.md` — and on APFS the
            // destination of that one exists because it is the file being
            // renamed. `exists()` alone reads them as one thing.
            if target.exists() && !same_directory(&target, &source) {
                return Err(ExecError::Refused(format!(
                    "{to} already exists; nothing was moved"
                )));
            }
            // No `create_dir_all` for the target's parent, unlike `CopyFile`:
            // a rename moves a file inside a directory that is already there,
            // and inventing a parent here would turn a typo in a plan into a
            // new directory on somebody's drive.
            std::fs::rename(&source, &target)
                .map_err(|e| failed(format!("move {from} → {to}: {e}")))
        }
        PlanStep::TrashDir { path, trash_key } => {
            let source = rel_link(zone, path)?;
            let trash = zone.join(".keeper/trash").join(trash_key);
            if !source.exists() && trash.exists() {
                return Ok(()); // already trashed
            }
            if let Some(parent) = trash.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::rename(&source, &trash).map_err(|e| failed(format!("trash {path}: {e}")))
        }
        PlanStep::TrashFile { path, trash_key } => {
            let source = rel_link(zone, path)?;
            // The basename rides along, so what lands in the trash is
            // `.keeper/trash/<key>/tasks.md` — recoverable by looking at it.
            let name = source
                .file_name()
                .ok_or_else(|| failed(format!("trash {path}: not a file name")))?
                .to_owned();
            let dir = zone.join(".keeper/trash").join(trash_key);
            let target = dir.join(&name);
            if !source.exists() && target.exists() {
                return Ok(()); // already trashed
            }
            std::fs::create_dir_all(&dir).map_err(|e| failed(format!("trash {path}: {e}")))?;
            std::fs::rename(&source, &target).map_err(|e| failed(format!("trash {path}: {e}")))
        }
        PlanStep::EmptyDirKeep { path, decided } => empty_as_decided(zone, path, decided.as_ref()),
    }
}

/// What a refused emptying adds: nothing more happened, and what to do.
const NOT_EMPTIED: &str = "so the workspace was not emptied and the session not moved; archive it again to decide about it.";

/// [`PlanStep::EmptyDirKeep`]: remove from the folder at `path` exactly
/// the entries `decided` names, after checking that the folder is the one
/// they were listed in — the same real folder, reached from the zone
/// without following a link — that every target the choices lean on still
/// says what it said, and that the folder holds nothing else. From that
/// check on the folder is held ([`Folder`]): every look and removal is
/// relative to it and to the folders below it as the check found them, so
/// a folder linked, renamed or moved into the place of it or of one inside
/// it later is never reached — the step refuses, and a held folder taken
/// from its place refuses once emptied. Each entry is looked at again as
/// it is removed, deepest first, and a folder is removed only once empty,
/// so work arriving in the held folders is never removed: it refuses the
/// step and is kept. Entries already gone — a crash inside an earlier run
/// of this step — are passed over. Without `decided` nothing is removed.
fn empty_as_decided(zone: &Path, path: &str, decided: Option<&Emptying>) -> Result<(), ExecError> {
    let refuse = |why: String| ExecError::Refused(why);
    let Some(decided) = decided else {
        return Err(refuse(format!(
            "this archive was decided by an earlier keeper that did not record what {path} held, {NOT_EMPTIED}"
        )));
    };
    let dir = rel(zone, path)?;
    let Some(top) = dir.file_name().and_then(|n| n.to_str()) else {
        return Err(refuse(format!("empty {path}: not a folder's path")));
    };
    let not_it = || {
        refuse(format!(
            "{path} is not the folder this archive was decided on — it was replaced, removed or reached through a link since — {NOT_EMPTIED}"
        ))
    };
    let held = match Folder::reach(zone, path) {
        Ok(held) => Some(held),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(not_it()),
    };
    if let Some(was) = &decided.root {
        if held.as_ref().and_then(Folder::identity).as_ref() != Some(was) {
            return Err(not_it());
        }
    }
    if let Some(target) = moved_target(zone, decided) {
        return Err(refuse(format!(
            "{target}, which a choice about the workspace leans on, changed or went after this archive was decided, {NOT_EMPTIED}"
        )));
    }
    let mut problems = Vec::new();
    let mut folders = BTreeMap::new();
    let found = held
        .as_ref()
        .map(|held| walk(held, top, &mut problems, &mut folders))
        .unwrap_or_default();
    if !problems.is_empty() {
        return Err(refuse(format!(
            "{}; the workspace cannot be checked whole, so it was not emptied.",
            problems.join("; ")
        )));
    }
    if let Some((entry, _)) = found
        .iter()
        .find(|(entry, stamp)| decided.entries.get(*entry) != Some(stamp))
    {
        return Err(refuse(format!(
            "{entry} arrived or changed after this archive was decided, {NOT_EMPTIED}"
        )));
    }
    #[cfg(test)]
    seam::reached();
    let held = match held {
        Some(held) => held,
        None => {
            std::fs::create_dir_all(&dir).map_err(|e| refuse(format!("mkdir {path}: {e}")))?;
            let made = Folder::reach(zone, path).map_err(|_| not_it())?;
            if let Some(identity) = made.identity() {
                folders.insert(top.to_owned(), identity);
            }
            made
        }
    };
    let kept = |entry: &str| {
        refuse(format!(
            "{entry} arrived or changed while the workspace was being emptied; it was kept, and the session not moved; archive it again to decide about it."
        ))
    };
    let mut deepest_first: Vec<_> = decided.entries.iter().collect();
    deepest_first.sort_by_key(|(entry, _)| std::cmp::Reverse(entry.matches('/').count()));
    for (entry, stamp) in deepest_first {
        let mut parts = entry.split('/');
        let inside =
            parts.next() == Some(top) && parts.all(|part| !matches!(part, "" | "." | ".."));
        let Some((holder, name)) = entry.rsplit_once('/').filter(|_| inside) else {
            return Err(refuse(format!(
                "{entry} is not an entry of {path}, {NOT_EMPTIED}"
            )));
        };
        // A folder the check did not find held nothing then: whatever is
        // in it now arrived since.
        let found_then = folders.contains_key(holder);
        let removed = match down(&held, holder, &folders) {
            Ok(Some(_)) if !found_then => return Err(kept(entry)),
            Ok(Some(holder)) if stamp == "dir" => holder.remove(name, true),
            Ok(Some(holder)) => match holder.stamp(name) {
                Ok(now) if now == *stamp => holder.remove(name, false),
                Ok(_) => return Err(kept(entry)),
                Err(error) => Err(error),
            },
            Ok(None) => return Err(kept(entry)),
            Err(error) => Err(error),
        };
        match removed {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(refuse(format!(
                    "{entry} could not be removed ({error}); what the workspace still holds was kept, and the session not moved; archive it again to decide about it."
                )))
            }
        }
    }
    let unread = |e: std::io::Error| refuse(format!("read {path}: {e}"));
    for name in held.names().map_err(unread)? {
        let name = name.map_err(unread)?;
        if name != ".gitkeep" {
            return Err(kept(&format!("{top}/{}", name.to_string_lossy())));
        }
    }
    // What was emptied must still be the folder at its place: one put
    // there since was never looked at, and is left as it is.
    let there = Folder::reach(zone, path)
        .ok()
        .and_then(|now| now.identity());
    if there.is_none() || there != held.identity() {
        return Err(refuse(format!(
            "{path} was replaced, moved or reached through a link while it was being emptied; what was decided was removed from the folder it was decided on, whatever is there now was not touched, and the session was not moved; archive it again to decide about it."
        )));
    }
    held.sync()
        .map_err(|e| refuse(format!("empty {path}: {e}")))?;
    held.keep(".gitkeep")
        .map_err(|e| refuse(format!("gitkeep {path}: {e}")))
}

/// The first target `decided` names that no longer says what the choices
/// leaning on it were made on, or `None`.
fn moved_target<'d>(zone: &Path, decided: &'d Emptying) -> Option<&'d str> {
    let drive = decided
        .zone_in_drive
        .as_deref()
        .and_then(|inside| drive_of(zone, inside));
    decided
        .targets
        .iter()
        .map(|(rel, said)| (Some(zone), rel, said))
        .chain(
            decided
                .drive_targets
                .iter()
                .map(|(rel, said)| (drive.as_deref(), rel, said)),
        )
        .find(|(base, rel, said)| !base.is_some_and(|base| still_says(base, rel, said)))
        .map(|(_, rel, _)| rel.as_str())
}

/// The drive holding `zone`, whose path in it is `inside`.
fn drive_of(zone: &Path, inside: &str) -> Option<PathBuf> {
    let mut drive = zone.to_path_buf();
    for part in inside.split('/').filter(|part| !part.is_empty()).rev() {
        if drive.file_name()? != part {
            return None;
        }
        drive.pop();
    }
    Some(drive)
}

/// Whether the file `rel` of `base` says `said`: `sha256:<hex>`, its bytes
/// hash to that; anything else, it is that target fact
/// ([`keeper_core::sessions::offer::target_fact`]) as the panel reads it.
fn still_says(base: &Path, rel: &str, said: &str) -> bool {
    let found = keeper_sync::browse::resolve_known(base, rel)
        .map(|known| known.landed().map(keeper_sync::browse::Landing::into_path));
    if let Some(sha256) = said.strip_prefix("sha256:") {
        return matches!(found, Ok(Some(path)) if hash_file(&path).is_ok_and(|held| held == sha256));
    }
    let fact = match found {
        Ok(None) => None,
        Err(refusal) => Some(Err(refusal.to_string())),
        Ok(Some(path)) => Some(target_fact_at(rel, &path)),
    };
    keeper_core::sessions::offer::target_fact(fact.as_ref()) == said
}

/// What the file `rel` at `path` says, as the panel's row facts read it.
fn target_fact_at(
    rel: &str,
    path: &Path,
) -> Result<keeper_core::sessions::promote::FileFact, String> {
    let unread = |error: std::io::Error| format!("{rel} could not be read: {error}");
    if !std::fs::metadata(path).map_err(unread)?.is_file() {
        return Err(format!("{rel} is not a file"));
    }
    let file = std::fs::File::open(path).map_err(unread)?;
    keeper_core::sessions::promote::fact_of(rel, file, None).map_err(unread)
}

/// A test's hold on the moment between an emptying's last check and its
/// first removal.
#[cfg(test)]
pub(crate) mod seam {
    use std::cell::RefCell;

    thread_local! {
        static AFTER_CHECK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    /// Run `then` on this thread once the next emptying has checked
    /// everything and before it removes anything.
    pub(crate) fn after_check(then: impl FnOnce() + 'static) {
        AFTER_CHECK.with(|hold| *hold.borrow_mut() = Some(Box::new(then)));
    }

    /// The emptying has checked everything: run what a test holds there.
    pub(super) fn reached() {
        if let Some(then) = AFTER_CHECK.with(|hold| hold.borrow_mut().take()) {
            then();
        }
    }
}

/// Whether two paths are the **same** thing on the disk, rather than two
/// spellings of it that only look different. Named for the directory move it
/// was extracted from, and asked by [`PlanStep::MoveFile`] too:
/// `canonicalize` does not care whether the path names a file, and a case-only
/// rename of `About.md` is the same trap as one of `Interview/`.
///
/// Asked wherever a move has to tell "the destination is taken" from "the
/// destination IS the source". On APFS and NTFS `_template/interview` exists the
/// moment `_template/Interview` does, so a case-only rename would be refused by
/// the very directory it is renaming; `exists()` alone cannot tell those apart.
///
/// `canonicalize` is the filesystem's own answer, which is why it is the one
/// asked: a lowercased path comparison would invent case-insensitivity on ext4,
/// where two such names are two directories and the refusal is correct. A path
/// that is not there canonicalises to nothing and is never "the same", so an
/// absent destination is not a collision either way.
///
/// Shared with the app's sessions commands, which make the same distinction one
/// layer up so the operator gets a sentence instead of an executor refusal.
/// Two copies of this would be two chances for the two layers to disagree about
/// which moves a zone accepts.
pub fn same_directory(left: &Path, right: &Path) -> bool {
    std::fs::canonicalize(left)
        .ok()
        .zip(std::fs::canonicalize(right).ok())
        .is_some_and(|(left, right)| left == right)
}

/// A zone-relative plan path joined onto the zone, refused if it escapes —
/// the executor's own containment, independent of who compiled the plan — for
/// a step that reads or writes **through** the path.
///
/// Lexically first (no `..`, no empty part, not absolute), then on the disk:
/// the deepest part of the joined path that exists already (the whole path
/// when it is there; an ancestor, for a file about to be written) must
/// canonicalise inside the canonical zone. A target that is not there yet
/// cannot be canonicalised, so its nearest existing ancestor is what a
/// symlinked folder would redirect it through.
fn rel(zone: &Path, path: &str) -> Result<PathBuf, ExecError> {
    contained(zone, path, Reach::Through)
}

/// [`rel`] for a step that acts on the path **itself** — moves it, or moves it
/// into the trash — so a link there is moved as a link and never followed.
/// Only the folder holding it must resolve inside the zone: a session whose
/// `workspace/` is a link to another disk can be trashed, and a dangling link
/// can be too.
fn rel_link(zone: &Path, path: &str) -> Result<PathBuf, ExecError> {
    contained(zone, path, Reach::Link)
}

/// Whether a step follows its path or acts on the entry the path names.
enum Reach {
    Through,
    Link,
}

fn contained(zone: &Path, path: &str, reach: Reach) -> Result<PathBuf, ExecError> {
    if path.is_empty()
        || Path::new(path).is_absolute()
        || path.split('/').any(|part| part == ".." || part.is_empty())
    {
        return Err(ExecError::Refused(format!(
            "plan path {path} is not zone-relative"
        )));
    }
    let target = zone.join(path);
    let escapes = || {
        ExecError::Refused(format!(
            "plan path {path} leaves the zone through a link; nothing was done"
        ))
    };
    let canonical_zone = zone.canonicalize().map_err(|error| {
        ExecError::Refused(format!(
            "the sessions zone is not there any more ({error}); nothing was done"
        ))
    })?;
    let from = match reach {
        Reach::Through => target.as_path(),
        Reach::Link => target.parent().unwrap_or(zone),
    };
    // The zone itself is an ancestor and exists, so this finds something; if
    // it does not, nothing on the way can be vouched for.
    let Some(existing) = from
        .ancestors()
        .find(|ancestor| ancestor.symlink_metadata().is_ok())
    else {
        return Err(escapes());
    };
    // There, yet not resolvable — a dangling link — or resolving outside the
    // zone: either way a step through it would land somewhere this zone does
    // not own.
    match existing.canonicalize() {
        Ok(resolved) if resolved.starts_with(&canonical_zone) => Ok(target),
        _ => Err(escapes()),
    }
}

/// Write bytes atomically and durably: a fresh stage beside the target
/// ([`stage_beside`]), synced, renamed over it, then the folder synced, so
/// a crash or a power cut leaves the old file or the new one whole and the
/// rename on the disk (`memlog.py`'s `write_atomic`, NFR-117). A folder it
/// has to make is made as [`make_dirs`] makes it.
pub(crate) fn write_durable(target: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    if parent.symlink_metadata().is_err() {
        make_dirs(parent)?;
    }
    let (tmp, mut file) = stage_beside(target)?;
    let written = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| publish_stage(&tmp, &file, target));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    sync_dir(parent)
}

/// Remove the file at `path` durably — the removal synced in its folder —
/// and succeed when it is not there.
pub(crate) fn remove_durable(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// A stage for `target`, beside it, that is this write's own: whatever
/// is at the staging name already — a crash's leftover, or a link or a
/// second name of another file planted there — is removed as an entry,
/// never followed or written through, and the stage is created
/// exclusively, so its handle is a new regular file nothing else names.
fn stage_beside(target: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(
        ".{}.keeper-tmp",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned())
    ));
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    Ok((tmp, file))
}

/// Rename the stage `tmp` over `target` once the name still holds the
/// file `held` is a handle of: what is published is that regular file,
/// never an entry put in its place meanwhile.
fn publish_stage(tmp: &Path, held: &std::fs::File, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let named = std::fs::symlink_metadata(tmp)?;
        let ours = held.metadata()?;
        if !named.file_type().is_file() || (named.dev(), named.ino()) != (ours.dev(), ours.ino()) {
            return Err(std::io::Error::other(
                "the staged file was replaced before it was published",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = held;
    std::fs::rename(tmp, target)
}

/// The lowercase hex SHA-256 of the file at `path`, read as one version.
fn hash_file(path: &Path) -> std::io::Result<String> {
    keeper_sync::stability::verify_while_reading(path)
        .map(|(sha256, _)| sha256)
        .map_err(std::io::Error::other)
}

/// Copy `source` over `target` as [`write_durable`] writes, once the
/// staged bytes hash to `sha256`: copied through the stage's own handle
/// ([`stage_beside`]), synced, read back through that handle and hashed,
/// and that file renamed into place, the folder synced. `false`, the stage
/// removed and the target as it was, when they do not: the source changed
/// after it was checked.
fn copy_checked(source: &Path, target: &Path, sha256: &str) -> std::io::Result<bool> {
    use std::io::Seek as _;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    if parent.symlink_metadata().is_err() {
        make_dirs(parent)?;
    }
    let (tmp, mut staged) = stage_beside(target)?;
    let copied = std::fs::File::open(source)
        .and_then(|mut from| std::io::copy(&mut from, &mut staged))
        .and_then(|_| staged.sync_all())
        .and_then(|()| staged.rewind())
        .and_then(|()| keeper_core::sessions::plan::sha256_hex_of(&staged))
        .and_then(|held| {
            if held != sha256 {
                return Ok(false);
            }
            publish_stage(&tmp, &staged, target).map(|()| true)
        });
    match copied {
        Ok(true) => {
            sync_dir(parent)?;
            Ok(true)
        }
        other => {
            let _ = std::fs::remove_file(&tmp);
            other
        }
    }
}

/// Make `dir` and every folder above it that is not there, durably: each
/// new folder's entry synced in its parent, so a folder the journal says
/// was made survives a power cut with the journal. A folder already there
/// has its own entry synced again — a resume after a crash between the
/// make and the sync.
fn make_dirs(dir: &Path) -> std::io::Result<()> {
    let missing: Vec<&Path> = dir
        .ancestors()
        .take_while(|ancestor| ancestor.symlink_metadata().is_err())
        .collect();
    std::fs::create_dir_all(dir)?;
    if missing.is_empty() {
        return sync_parent(dir);
    }
    for made in missing.iter().rev() {
        sync_parent(made)?;
    }
    Ok(())
}

/// Make `dir`, a folder of the notes vault rooted at `vault` in the drive
/// rooted at `drive`, and every folder on the way that is not there,
/// durably: then every entry from the drive's child down to `dir` synced
/// in its parent, whether this call made it, an earlier one made it and
/// failed before its sync, or the vault's registration made it — the vault
/// root and every configured folder above it included. So `Ok` says every
/// folder a write under `dir` is reached through is on the disk, and a
/// retry after a partial failure syncs what the failed one did not. A vault
/// that does not lie in the drive (a link the person made) is synced from
/// its own root down. For a writer outside the zone that says a write is
/// durable: the notes vault's (R244, R253). A `dir` under neither is
/// refused.
pub fn make_dirs_within(drive: &Path, vault: &Path, dir: &Path) -> std::io::Result<()> {
    let root = if vault.starts_with(drive) {
        drive
    } else {
        vault
    };
    let below = dir.strip_prefix(root).map_err(|_| {
        std::io::Error::other(format!("{} is not under {}", dir.display(), root.display()))
    })?;
    std::fs::create_dir_all(dir)?;
    let mut at = root.to_path_buf();
    for part in below.components() {
        at.push(part);
        sync_parent(&at)?;
    }
    Ok(())
}

/// [`sync_dir`] of the folder holding `path`.
pub(crate) fn sync_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) => sync_dir(parent),
        None => Ok(()),
    }
}

/// Make a move from `source` to `target` durable: both folders' entries.
fn sync_moved(source: &Path, target: &Path) -> std::io::Result<()> {
    sync_parent(target)?;
    if source.parent() != target.parent() {
        sync_parent(source)?;
    }
    Ok(())
}

/// Every file of the real folder `dir`, by `/`-joined path below it, with
/// its bytes — or the sentence saying why its tree is not only real
/// folders and regular files that read: a link (never followed), another
/// kind of entry, a name that is not UTF-8, or a read that failed.
pub(crate) fn regular_files(dir: &Path) -> Result<HashMap<String, Vec<u8>>, String> {
    fn walk(dir: &Path, prefix: &str, into: &mut HashMap<String, Vec<u8>>) -> Result<(), String> {
        let shown = |name: &str| {
            if name.is_empty() {
                ".".to_owned()
            } else {
                name.to_owned()
            }
        };
        let entries = std::fs::read_dir(dir).map_err(|e| {
            format!(
                "{} could not be listed: {e}",
                shown(prefix.trim_end_matches('/'))
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                format!(
                    "{} could not be listed: {e}",
                    shown(prefix.trim_end_matches('/'))
                )
            })?;
            let base = entry.file_name();
            let base = base
                .to_str()
                .ok_or_else(|| format!("{prefix}{} is not a UTF-8 name", base.to_string_lossy()))?;
            let name = format!("{prefix}{base}");
            let kind = entry
                .file_type()
                .map_err(|e| format!("{name} could not be read: {e}"))?;
            if kind.is_symlink() {
                return Err(format!("{name} is a link"));
            } else if kind.is_dir() {
                walk(&entry.path(), &format!("{name}/"), into)?;
            } else if kind.is_file() {
                let bytes = std::fs::read(entry.path())
                    .map_err(|e| format!("{name} could not be read: {e}"))?;
                into.insert(name, bytes);
            } else {
                return Err(format!("{name} is neither a file nor a folder"));
            }
        }
        Ok(())
    }
    match dir.symlink_metadata() {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(meta) if meta.file_type().is_symlink() => return Err("it is a link".to_owned()),
        Ok(_) => return Err("it is not a folder".to_owned()),
        Err(e) => return Err(format!("it could not be read: {e}")),
    }
    let mut found = HashMap::new();
    walk(dir, "", &mut found)?;
    Ok(found)
}

/// Make a rename inside `dir` durable: the folder's own entry list synced.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Windows commits a rename with the file; a folder cannot be opened to sync.
#[cfg(not(unix))]
fn sync_dir(_: &Path) -> std::io::Result<()> {
    Ok(())
}

fn write_journal(journal: &Path, row: &JournalRow) -> Result<(), ExecError> {
    let text = serde_json::to_string_pretty(row).map_err(|e| ExecError::Failed {
        verb: row.plan.verb.clone(),
        step: row.done,
        reason: format!("could not encode the journal: {e}"),
    })?;
    write_durable(journal, &text).map_err(|e| ExecError::Failed {
        verb: row.plan.verb.clone(),
        step: row.done,
        reason: format!("could not write the journal: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::sessions::files::compile_dir_new;
    use keeper_core::sessions::plan::{
        compile_archive, compile_create, compile_delete, ArchiveDecision,
    };

    fn zone() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for sub in [
            "_template/workspace",
            "_template/artifacts",
            "active",
            "archive",
        ] {
            std::fs::create_dir_all(dir.path().join(sub)).expect("mkdir");
        }
        std::fs::write(dir.path().join("_template/README.md"), "# template\n").expect("write");
        dir
    }

    /// A create runs end to end: template copied, README stamped, journal
    /// cleared. The plan compiled in core; the executor only obeys.
    #[test]
    fn a_create_plan_lands_a_session_and_clears_the_journal() {
        let zone = zone();
        let plan = compile_create(
            "2026-08-12-research",
            "_template",
            &[
                ("README.md".to_owned(), false),
                ("workspace".to_owned(), true),
                ("artifacts".to_owned(), true),
            ],
            "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\n---\n# research\n",
        );
        run(zone.path(), plan).expect("runs");
        let readme =
            std::fs::read_to_string(zone.path().join("active/2026-08-12-research/README.md"))
                .expect("readme");
        assert!(readme.contains("# research"));
        assert!(zone
            .path()
            .join("active/2026-08-12-research/workspace")
            .is_dir());
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// The archive's crash story (NFR-38): kill the run before the move, and
    /// a resume completes it — promotes idempotent, move-last honoured.
    #[test]
    fn a_journaled_archive_resumes_after_a_crash_before_the_move() {
        let zone = zone();
        let session = zone.path().join("active/2026-08-10-keeper");
        std::fs::create_dir_all(session.join("workspace")).expect("mkdir");
        std::fs::create_dir_all(session.join("artifacts")).expect("mkdir");
        std::fs::write(session.join("README.md"), "# keeper\n").expect("write");
        std::fs::write(session.join("workspace/draft.md"), "the draft").expect("write");

        let plan = compile_archive(
            "active/2026-08-10-keeper",
            ArchiveDecision {
                before: vec![PlanStep::CopyFile {
                    from: "active/2026-08-10-keeper/workspace/draft.md".to_owned(),
                    to: "active/2026-08-10-keeper/artifacts/report.md".to_owned(),
                }],
                empty_workspace: true,
                emptying: Emptying {
                    entries: BTreeMap::from([(
                        "workspace/draft.md".to_owned(),
                        stamp(&session.join("workspace/draft.md")),
                    )]),
                    root: identity(&session.join("workspace")),
                    ..Emptying::default()
                },
                year: 2026,
            },
        );
        // Simulate the crash: journal written, first step done, process gone.
        let journal = zone.path().join(JOURNAL_REL);
        write_journal(
            &journal,
            &JournalRow {
                plan: plan.clone(),
                done: 0,
            },
        )
        .expect("journal");
        run_step(zone.path(), &plan.steps[0]).expect("the promote copy");
        write_journal(
            &journal,
            &JournalRow {
                plan: plan.clone(),
                done: 1,
            },
        )
        .expect("journal");

        // Relaunch: resume finishes the remaining steps.
        resume(zone.path()).expect("resumes");
        let moved = zone.path().join("archive/2026/2026-08-10-keeper");
        assert!(
            moved.join("artifacts/report.md").exists(),
            "the promote landed"
        );
        assert!(
            moved.join("workspace/.gitkeep").exists(),
            "workspace emptied"
        );
        assert!(!moved.join("workspace/draft.md").exists());
        assert!(!zone.path().join("active/2026-08-10-keeper").exists());
        assert!(!journal.exists(), "journal cleared after resume");
    }

    /// An inventory stamp of the file at `path`, as [`inventory`] writes it.
    fn stamp(path: &Path) -> String {
        let mut problems = Vec::new();
        let parent = path.parent().expect("parent");
        let top = parent.file_name().and_then(|n| n.to_str()).expect("top");
        let name = path.file_name().and_then(|n| n.to_str()).expect("name");
        inventory(parent.parent().expect("dir"), top, &mut problems)
            .remove(&format!("{top}/{name}"))
            .expect("stamped")
    }

    /// An archive of `active/2026-08-10-keeper` decided on its workspace as
    /// it is now — `draft.md` promoted by a checked copy, then the guarded
    /// emptying, then the move — journaled with its first `done` steps run,
    /// as a crash leaves it.
    fn crashed_archive(zone: &Path, done: usize) -> PathBuf {
        let session = zone.join("active/2026-08-10-keeper");
        let draft = keeper_core::sessions::plan::sha256_hex("the draft");
        let mut problems = Vec::new();
        let emptying = Emptying {
            entries: inventory(&session, "workspace", &mut problems),
            root: identity(&session.join("workspace")),
            targets: BTreeMap::from([(
                "active/2026-08-10-keeper/artifacts/report.md".to_owned(),
                format!("sha256:{draft}"),
            )]),
            ..Emptying::default()
        };
        assert!(problems.is_empty(), "{problems:?}");
        let plan = compile_archive(
            "active/2026-08-10-keeper",
            ArchiveDecision {
                before: vec![PlanStep::CopyChecked {
                    from: "active/2026-08-10-keeper/workspace/draft.md".to_owned(),
                    to: "active/2026-08-10-keeper/artifacts/report.md".to_owned(),
                    sha256: draft,
                }],
                empty_workspace: true,
                emptying,
                year: 2026,
            },
        );
        let journal = zone.join(JOURNAL_REL);
        for step in &plan.steps[..done] {
            run_step(zone, step).expect("a step before the crash");
        }
        write_journal(&journal, &JournalRow { plan, done }).expect("journal");
        session
    }

    fn keeper_session(zone: &Path) -> PathBuf {
        let session = zone.join("active/2026-08-10-keeper");
        std::fs::create_dir_all(session.join("workspace/sub")).expect("mkdir");
        std::fs::create_dir_all(session.join("artifacts")).expect("mkdir");
        std::fs::write(session.join("README.md"), "# keeper\n").expect("write");
        std::fs::write(session.join("workspace/draft.md"), "the draft").expect("write");
        std::fs::write(session.join("workspace/sub/x.md"), "x").expect("write");
        session
    }

    /// R234 (R95P2-04): the archive's emptying is guarded by the workspace
    /// it was decided on, at the step and on a resume. A file arriving
    /// after a completed checked copy, or a decided file written before the
    /// emptying, refuses the resumed run: that work is kept, the session
    /// not moved, the journal cleared, the promotion's copy kept. A resume
    /// finding part of the decided workspace already removed — a crash
    /// inside the emptying — empties the rest and moves the session.
    #[test]
    fn an_archive_resumed_after_a_crash_keeps_work_that_arrived_since() {
        for (name, done, change) in [
            (
                "a file arrived after the checked copy",
                1,
                &(|session: &Path| {
                    std::fs::write(session.join("workspace/.late.md"), "new work").expect("late");
                }) as &dyn Fn(&Path),
            ),
            (
                "a decided file written before the emptying",
                1,
                &|session: &Path| {
                    std::fs::write(session.join("workspace/sub/x.md"), "x, and more")
                        .expect("edit");
                },
            ),
            (
                "a folder arrived before anything ran",
                0,
                &|session: &Path| {
                    std::fs::create_dir(session.join("workspace/.staging")).expect("dir");
                },
            ),
        ] {
            let zone = zone();
            let session = keeper_session(zone.path());
            crashed_archive(zone.path(), done);
            change(&session);
            let refused = resume(zone.path());
            assert!(
                matches!(refused, Err(ExecError::Refused(_))),
                "{name}: {refused:?}"
            );
            assert!(session.join("workspace/draft.md").is_file(), "{name}");
            assert!(session.join("workspace/sub/x.md").is_file(), "{name}");
            // The checked copy of unchanged bytes runs (or ran) before the
            // guarded emptying refuses: the promotion is kept either way.
            assert_eq!(
                std::fs::read_to_string(session.join("artifacts/report.md")).ok(),
                Some("the draft".to_owned()),
                "{name}"
            );
            assert!(
                !zone.path().join("archive/2026/2026-08-10-keeper").exists(),
                "{name}"
            );
            assert!(!zone.path().join(JOURNAL_REL).exists(), "{name}");
        }

        for (name, half) in [
            ("a folder of it removed", "workspace/sub"),
            ("a file of it removed", "workspace/draft.md"),
        ] {
            let zone = zone();
            let session = keeper_session(zone.path());
            crashed_archive(zone.path(), 1);
            let half = session.join(half);
            if half.is_dir() {
                std::fs::remove_dir_all(half).expect("half emptied");
            } else {
                std::fs::remove_file(half).expect("half emptied");
            }
            resume(zone.path()).expect("the rest emptied");
            let moved = zone.path().join("archive/2026/2026-08-10-keeper");
            assert!(moved.join("artifacts/report.md").is_file(), "{name}");
            assert_eq!(
                std::fs::read_dir(moved.join("workspace"))
                    .expect("workspace")
                    .map(|entry| entry.expect("entry").file_name())
                    .collect::<Vec<_>>(),
                [".gitkeep"],
                "{name}"
            );
        }
    }

    /// R249 (R95P3-01): a journal an earlier keeper wrote — its emptying
    /// recorded without what it was decided on (the build before R234), or
    /// with only the inventory (R234's) — resumes without any authority to
    /// remove: its emptying refuses, every byte of the workspace and what
    /// arrived since is kept, the session is not moved and the journal
    /// clears, so the archive is decided again.
    #[test]
    fn an_earlier_keepers_archive_journal_removes_nothing() {
        let draft = "active/2026-08-10-keeper/workspace/draft.md";
        let steps = |emptying: &str| {
            format!(
                r#"{{"plan":{{"verb":"archive","session":"active/2026-08-10-keeper","steps":[{{"op":"copyFile","from":"{draft}","to":"active/2026-08-10-keeper/artifacts/report.md"}},{{"op":"emptyDirKeep","path":"active/2026-08-10-keeper/workspace"{emptying}}},{{"op":"mkDir","path":"archive/2026"}},{{"op":"moveDir","from":"active/2026-08-10-keeper","to":"archive/2026/2026-08-10-keeper"}}]}},"done":1}}"#
            )
        };
        for (name, emptying) in [
            ("no record", String::new()),
            (
                "an inventory only",
                r#","accepted":{"workspace/draft.md":"file:9:1","workspace/sub":"dir","workspace/sub/x.md":"file:1:1"}"#.to_owned(),
            ),
        ] {
            let zone = zone();
            let session = keeper_session(zone.path());
            let journal = zone.path().join(JOURNAL_REL);
            std::fs::create_dir_all(journal.parent().expect(".keeper")).expect(".keeper");
            std::fs::write(&journal, steps(&emptying)).expect("journal");
            std::fs::write(session.join("workspace/late.md"), "new work").expect("late");
            let refused = resume(zone.path());
            assert!(
                matches!(&refused, Err(ExecError::Refused(why)) if why.contains("earlier keeper")),
                "{name}: {refused:?}"
            );
            for (rel, bytes) in [
                ("workspace/draft.md", "the draft"),
                ("workspace/sub/x.md", "x"),
                ("workspace/late.md", "new work"),
            ] {
                assert_eq!(
                    std::fs::read_to_string(session.join(rel)).ok().as_deref(),
                    Some(bytes),
                    "{name}: {rel}"
                );
            }
            assert!(
                !zone.path().join("archive/2026/2026-08-10-keeper").exists(),
                "{name}"
            );
            assert!(!zone.path().join(JOURNAL_REL).exists(), "{name}");
        }
    }

    /// R249 (R95P3-02): the emptying removes only what it was decided on.
    /// Work arriving after its last check — a file at the top of the
    /// workspace, one inside a decided folder — or a decided file written
    /// then, is never removed: the step refuses, that work is kept with its
    /// bytes, the session is not moved.
    #[test]
    fn work_arriving_while_the_workspace_is_emptied_is_kept() {
        for (name, rel, bytes) in [
            ("a file at the top", "workspace/late.md", "new work"),
            (
                "a file in a decided folder",
                "workspace/sub/late.md",
                "new work",
            ),
            (
                "a decided file written",
                "workspace/sub/x.md",
                "x, and more",
            ),
        ] {
            let zone = zone();
            let session = keeper_session(zone.path());
            crashed_archive(zone.path(), 1);
            let arrival = session.join(rel);
            let written = arrival.clone();
            seam::after_check(move || std::fs::write(written, bytes).expect("arrives"));
            let refused = resume(zone.path());
            assert!(
                matches!(refused, Err(ExecError::Refused(_))),
                "{name}: {refused:?}"
            );
            assert_eq!(
                std::fs::read_to_string(&arrival).ok().as_deref(),
                Some(bytes),
                "{name}"
            );
            assert!(
                !zone.path().join("archive/2026/2026-08-10-keeper").exists(),
                "{name}"
            );
        }
    }

    /// Copies of `rels` of the folder `of` into `into`, each with the same
    /// length and mtime — the same stamps.
    #[cfg(unix)]
    fn twin(of: &Path, into: &Path, rels: &[&str]) {
        for rel in rels {
            let to = into.join(rel);
            std::fs::create_dir_all(to.parent().expect("parent")).expect("twin");
            std::fs::copy(of.join(rel), &to).expect("copy");
            let at = std::fs::metadata(of.join(rel))
                .and_then(|meta| meta.modified())
                .expect("mtime");
            std::fs::File::options()
                .write(true)
                .open(&to)
                .and_then(|file| file.set_modified(at))
                .expect("same stamp");
        }
    }

    /// R249 (R95P3-04): the emptying is bound to the folder it was decided
    /// on, at its own place. Replaced before a resume by another folder —
    /// even one holding the same names with the same stamps — by a link to
    /// another session's workspace in the zone or to a folder outside it,
    /// or reached through a link on the way, it refuses and the other
    /// folder keeps every byte.
    #[cfg(unix)]
    #[test]
    fn an_emptying_is_bound_to_the_folder_it_was_decided_on() {
        let outside = tempfile::tempdir().expect("outside");
        for name in [
            "replaced",
            "a link in the zone",
            "a link out of it",
            "a link on the way",
        ] {
            let zone = zone();
            let session = keeper_session(zone.path());
            crashed_archive(zone.path(), 1);
            let workspace = session.join("workspace");
            let other = match name {
                "a link out of it" => outside.path().join(name),
                "a link on the way" => zone.path().join("active/elsewhere"),
                _ => zone.path().join("active/other/workspace"),
            };
            if name == "a link on the way" {
                std::fs::rename(&session, &other).expect("moved away");
                std::os::unix::fs::symlink(&other, &session).expect("link");
            } else {
                twin(&workspace, &other, &["draft.md", "sub/x.md"]);
                std::fs::rename(&workspace, session.join("decided")).expect("aside");
                if name == "replaced" {
                    std::fs::rename(&other, &workspace).expect("replaced");
                } else {
                    std::os::unix::fs::symlink(&other, &workspace).expect("link");
                }
            }
            let refused = resume(zone.path());
            assert!(
                matches!(refused, Err(ExecError::Refused(_))),
                "{name}: {refused:?}"
            );
            let kept = if name == "replaced" {
                workspace
            } else if name == "a link on the way" {
                other.join("workspace")
            } else {
                other
            };
            for (rel, bytes) in [("draft.md", "the draft"), ("sub/x.md", "x")] {
                assert_eq!(
                    std::fs::read_to_string(kept.join(rel)).ok().as_deref(),
                    Some(bytes),
                    "{name}: {rel}"
                );
            }
        }
    }

    /// R265 (R95P4-01): the same replacements after the emptying's last
    /// check (the seam), before it removes anything — the workspace or a
    /// folder in it swapped for a link to a twin holding the same names
    /// with the same stamps, in the zone or out of it, or swapped for such a
    /// twin itself — never carry a removal into the
    /// twin: the step refuses, every byte of the twin is kept, the session
    /// is not moved.
    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_in_after_the_check_is_never_emptied() {
        let outside = tempfile::tempdir().expect("outside");
        for (name, swapped, rels, linked, out) in [
            (
                "the workspace, a link in the zone",
                "workspace",
                &["draft.md", "sub/x.md"][..],
                true,
                false,
            ),
            (
                "the workspace, a link out of it",
                "workspace",
                &["draft.md", "sub/x.md"][..],
                true,
                true,
            ),
            (
                "the workspace, a twin folder",
                "workspace",
                &["draft.md", "sub/x.md"][..],
                false,
                false,
            ),
            (
                "a folder in it, a link in the zone",
                "workspace/sub",
                &["x.md"][..],
                true,
                false,
            ),
            (
                "a folder in it, a link out of it",
                "workspace/sub",
                &["x.md"][..],
                true,
                true,
            ),
            (
                "a folder in it, a twin folder",
                "workspace/sub",
                &["x.md"][..],
                false,
                false,
            ),
        ] {
            let zone = zone();
            let session = keeper_session(zone.path());
            crashed_archive(zone.path(), 1);
            let at = session.join(swapped);
            let other = if out {
                outside.path().join(name)
            } else {
                zone.path().join("active/other").join(name)
            };
            twin(&at, &other, rels);
            let (place, aside, twin_dir) = (at.clone(), session.join("decided"), other.clone());
            seam::after_check(move || {
                std::fs::rename(&place, aside).expect("aside");
                if linked {
                    std::os::unix::fs::symlink(&twin_dir, &place).expect("link");
                } else {
                    std::fs::rename(&twin_dir, &place).expect("twin in its place");
                }
            });
            let refused = resume(zone.path());
            assert!(
                matches!(refused, Err(ExecError::Refused(_))),
                "{name}: {refused:?}"
            );
            let kept = if linked { &other } else { &at };
            for rel in rels {
                let want = if *rel == "draft.md" { "the draft" } else { "x" };
                assert_eq!(
                    std::fs::read_to_string(kept.join(rel)).ok().as_deref(),
                    Some(want),
                    "{name}: {rel}"
                );
            }
            assert!(
                !zone.path().join("archive/2026/2026-08-10-keeper").exists(),
                "{name}"
            );
        }
    }

    /// A delete is a recoverable trash move — the folder, workspace and all,
    /// sits under .keeper/trash keyed by id.
    #[test]
    fn a_delete_lands_in_the_zone_trash_recoverable() {
        let zone = zone();
        let session = zone.path().join("active/x");
        std::fs::create_dir_all(session.join("workspace")).expect("mkdir");
        std::fs::write(session.join("workspace/scratch.md"), "s").expect("write");
        run(
            zone.path(),
            compile_delete("active/x", "01J5AAAAAAAAAAAAAAAAAAAAAA"),
        )
        .expect("runs");
        assert!(!session.exists());
        assert!(zone
            .path()
            .join(".keeper/trash/01J5AAAAAAAAAAAAAAAAAAAAAA/workspace/scratch.md")
            .exists());
    }

    /// A guarded write refuses when the file moved under it — and the refusal
    /// clears the journal so the next attempt re-plans instead of resuming a
    /// stale plan.
    #[test]
    fn a_guarded_write_against_a_moved_file_refuses_and_clears_the_journal() {
        let zone = zone();
        std::fs::create_dir_all(zone.path().join("active/s")).expect("mkdir");
        std::fs::write(zone.path().join("active/s/README.md"), "original").expect("write");
        let plan = Plan {
            verb: "log-today".to_owned(),
            session: "active/s".to_owned(),
            steps: vec![PlanStep::GuardedWrite {
                path: "active/s/README.md".to_owned(),
                expect_len: "stale-length-that-is-wrong".len(),
                expect_sha256: None,
                content: "clobber".to_owned(),
            }],
        };
        let error = run(zone.path(), plan).expect_err("refuses");
        assert!(matches!(error, ExecError::Refused(_)));
        assert_eq!(
            std::fs::read_to_string(zone.path().join("active/s/README.md")).expect("read"),
            "original",
            "nothing was written"
        );
        assert!(
            !zone.path().join(JOURNAL_REL).exists(),
            "refusal clears the journal"
        );
    }

    /// R120 (R4-09): a guard on the bytes read refuses an edit of the same
    /// length — `todo` → `done` — that a length alone lets through.
    #[test]
    fn a_guarded_write_refuses_a_same_length_edit() {
        let zone = zone();
        std::fs::create_dir_all(zone.path().join("active/s")).expect("mkdir");
        let card = zone.path().join("active/s/card.md");
        let read = "---\nstatus: todo\n---\n";
        std::fs::write(&card, read.replace("todo", "done")).expect("a person's edit");
        let plan = |read: &str| Plan {
            verb: "card-run".to_owned(),
            session: "active/s".to_owned(),
            steps: vec![PlanStep::guarded(
                "active/s/card.md".to_owned(),
                read,
                "---\nstatus: todo\nrun: running\n---\n".to_owned(),
            )],
        };
        assert!(matches!(
            run(zone.path(), plan(read)),
            Err(ExecError::Refused(_))
        ));
        assert_eq!(
            std::fs::read_to_string(&card).expect("read"),
            "---\nstatus: done\n---\n",
            "the person's edit stands"
        );
        run(zone.path(), plan("---\nstatus: done\n---\n")).expect("the bytes it read");
    }

    /// The executor's own containment: a plan path that escapes refuses, no
    /// matter who compiled it.
    #[test]
    fn an_escaping_plan_path_is_refused_by_the_executor_itself() {
        let zone = zone();
        let plan = Plan {
            verb: "create".to_owned(),
            session: "active/x".to_owned(),
            steps: vec![PlanStep::WriteFile {
                path: "../outside.md".to_owned(),
                content: "no".to_owned(),
            }],
        };
        assert!(matches!(run(zone.path(), plan), Err(ExecError::Refused(_))));
    }

    /// The refusal the `MoveDir` guard exists for, unchanged by the
    /// source-identity carve-out beside it: a target that is a *different*
    /// directory is a neighbour, and a move must not eat one. Asserted on the
    /// sentence, because the IPC layer shows it to the operator.
    #[test]
    fn a_move_onto_a_different_directory_is_still_refused() {
        let zone = zone();
        for name in ["_template/interview", "_template/kick-off"] {
            std::fs::create_dir_all(zone.path().join(name)).expect("mkdir");
        }
        std::fs::write(zone.path().join("_template/kick-off/about.md"), "theirs").expect("write");
        let plan = Plan {
            verb: "template-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveDir {
                from: "_template/interview".to_owned(),
                to: "_template/kick-off".to_owned(),
            }],
        };
        let error = run(zone.path(), plan).expect_err("refuses");
        assert!(matches!(
            &error,
            ExecError::Refused(said)
                if said == "_template/kick-off already exists; nothing was moved"
        ));
        assert!(zone.path().join("_template/interview").is_dir());
        assert_eq!(
            std::fs::read_to_string(zone.path().join("_template/kick-off/about.md")).expect("read"),
            "theirs",
            "the neighbour was not touched"
        );
    }

    /// The carve-out: a target that *resolves to the source* is not a
    /// collision, so the move runs instead of being refused by the directory it
    /// is renaming.
    ///
    /// The case that motivates it — `_template/Interview/` → `interview` on
    /// APFS — cannot be reproduced on a case-sensitive volume, so what is
    /// asserted here is the property the carve-out rests on, spelled a way any
    /// filesystem can produce: two paths for one directory. On macOS the two
    /// paths differ in case instead, and the branch taken is this one.
    #[test]
    fn a_move_whose_target_resolves_to_the_source_is_not_a_collision() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("about.md"), "mine").expect("write");
        let plan = Plan {
            verb: "template-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveDir {
                from: "_template/interview".to_owned(),
                to: "_template/./interview".to_owned(),
            }],
        };
        run(zone.path(), plan).expect("the source is not its own collision");
        assert_eq!(
            std::fs::read_to_string(dir.join("about.md")).expect("read"),
            "mine",
            "renaming a directory onto itself keeps it"
        );
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// Row 1 of the matrix: a file rename runs end to end — at the new path,
    /// gone from the old, journal cleared. The bytes are asserted rather than
    /// only the existence, because a copy-then-delete would also satisfy
    /// "present at `to`, absent at `from`" and this step is a move.
    #[test]
    fn a_move_file_lands_the_file_at_its_new_name_and_clears_the_journal() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("about.md"), "the record").expect("write");
        let plan = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/record.md".to_owned(),
            }],
        };
        run(zone.path(), plan).expect("runs");
        assert_eq!(
            std::fs::read_to_string(dir.join("record.md")).expect("read"),
            "the record"
        );
        assert!(!dir.join("about.md").exists(), "the old name is gone");
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// Row 2: the refusal, and the carve-out beside it — `MoveDir`'s pair of
    /// tests asked of a file, because the two arms share `same_directory` and a
    /// file rename is where the case-only case actually bites (`About.md` is a
    /// name people capitalise; `Interview/` is one they rarely do).
    ///
    /// Both halves in one test because they are one rule: a destination that
    /// exists is a collision exactly when it is a *different* file.
    #[test]
    fn a_move_file_refuses_a_different_file_and_allows_its_own_source() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("about.md"), "mine").expect("write");
        std::fs::write(dir.join("questions.md"), "theirs").expect("write");

        let onto_a_neighbour = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/questions.md".to_owned(),
            }],
        };
        let error = run(zone.path(), onto_a_neighbour).expect_err("refuses");
        assert!(matches!(
            &error,
            ExecError::Refused(said)
                if said == "_template/interview/questions.md already exists; nothing was moved"
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join("questions.md")).expect("read"),
            "theirs",
            "the neighbour was not written over"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("about.md")).expect("read"),
            "mine",
            "and the source stayed where it was"
        );

        // The carve-out. The motivating case — `About.md` → `about.md` on APFS —
        // cannot be reproduced on a case-sensitive volume, so this asserts the
        // property it rests on in a spelling every filesystem produces: two paths
        // for one file. On macOS the two differ in case instead, and the branch
        // taken is this one.
        let onto_itself = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/./about.md".to_owned(),
            }],
        };
        run(zone.path(), onto_itself).expect("the source is not its own collision");
        assert_eq!(
            std::fs::read_to_string(dir.join("about.md")).expect("read"),
            "mine",
            "renaming a file onto itself keeps it"
        );
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// A `MoveFile` whose source vanished between the shell's stat and the plan
    /// running answers with the failure, never with somebody else's file.
    ///
    /// `MoveDir`'s already-moved short-circuit reads "source gone, target there"
    /// as "this plan already ran", which is sound for a resumed journal whose
    /// only writer produced that pair. Copied onto `MoveFile` it was unsound: the
    /// rename's source is stated by whoever typed the name, so a neighbour at the
    /// destination satisfies the same test, and the command would have cleared
    /// the journal and answered the room with the subpath of a file it never
    /// touched. Both spellings of the vanished source are asserted, because they
    /// answer differently and both answers must be about THIS plan: a neighbour
    /// is the collision refusal, and nothing at all is the rename error.
    #[test]
    fn a_move_file_whose_source_vanished_never_reports_a_neighbour_as_moved() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // No `about.md`: the source the plan names is already gone.
        std::fs::write(dir.join("questions.md"), "theirs").expect("write");

        let onto_a_neighbour = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/questions.md".to_owned(),
            }],
        };
        let error = run(zone.path(), onto_a_neighbour).expect_err("must not answer Ok");
        assert!(
            matches!(
                &error,
                ExecError::Refused(said)
                    if said == "_template/interview/questions.md already exists; nothing was moved"
            ),
            "a neighbour is a collision, not a completed move: {error}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("questions.md")).expect("read"),
            "theirs",
            "and the neighbour is untouched"
        );

        let onto_nothing = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/record.md".to_owned(),
            }],
        };
        let error = run(zone.path(), onto_nothing).expect_err("a missing source is a failure");
        // `run_step` reports a failed rename through `Refused` too (its own
        // `failed` closure), so the variant is not what distinguishes this from a
        // collision — the sentence is, and it names the move that did not happen.
        assert!(
            matches!(
                &error,
                ExecError::Refused(said)
                    if said.starts_with(
                        "move _template/interview/about.md → _template/interview/record.md"
                    )
            ),
            "the rename error is what says the list was stale: {error}"
        );
        assert!(!dir.join("record.md").exists(), "and nothing was created");
    }

    /// `MkDir`'s idempotence, executed against a filesystem instead of asserted
    /// about a plan (Story 51.2, FR-287).
    ///
    /// `files.rs` compares two pure `compile_dir_new` calls, which says nothing
    /// about the disk: swap `create_dir_all` for `create_dir` and that assertion
    /// stays green while both halves of the paragraph break. Both halves are
    /// here — parents that are not there yet, and a directory that already is —
    /// because they fail differently (`NotFound` and `AlreadyExists`) and a test
    /// holding only one of them would let the other rot.
    #[test]
    fn a_dir_new_plan_makes_missing_parents_and_absorbs_a_second_run() {
        let zone = zone();
        std::fs::create_dir_all(zone.path().join("active/s")).expect("mkdir");
        let deep = zone.path().join("active/s/a/b/c");

        let plan = compile_dir_new("active/s", "a/b/c").expect("a session may hold a folder");
        run(zone.path(), plan).expect("one step makes the whole path");
        assert!(
            deep.is_dir(),
            "MkDir made the parents, so the plan is one step"
        );
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");

        // A file inside is what makes the second run's claim testable: a
        // directory that was re-made would be an empty one, and "changes
        // nothing" is the promise, not merely "does not error".
        std::fs::write(deep.join("note.md"), "kept").expect("write");
        let again = compile_dir_new("active/s", "a/b/c").expect("the same request twice");
        run(zone.path(), again).expect("a folder already there is not a failure");
        assert_eq!(
            std::fs::read_to_string(deep.join("note.md")).expect("read"),
            "kept",
            "the second press left the folder and its contents alone"
        );
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");

        // The shallow case too: one new segment under a session that exists is
        // the ordinary press, and it must not be the only one the suite runs.
        let shallow = compile_dir_new("active/s", "log").expect("a session may hold a log/");
        run(zone.path(), shallow).expect("runs");
        assert!(zone.path().join("active/s/log").is_dir());
    }

    /// A folder verb pointed at a path that is already a FILE refuses, and the
    /// file is still the operator's file afterwards.
    ///
    /// `sessions_dir_new` deliberately runs no pre-flight `is_file`, on the
    /// stated grounds that "the executor's `create_dir_all` fails on it and says
    /// so". Nothing executed that, so the sentence was a claim about a code path
    /// no test had ever taken. `create_dir_all` answers `Ok` for an existing
    /// DIRECTORY and an error for an existing file, and the difference between
    /// those two is the whole reason the verb may skip the pre-flight.
    #[test]
    fn a_mkdir_onto_an_existing_file_refuses_and_leaves_the_file_alone() {
        let zone = zone();
        std::fs::create_dir_all(zone.path().join("active/s")).expect("mkdir");
        let taken = zone.path().join("active/s/log");
        std::fs::write(&taken, "somebody's file").expect("write");

        let plan = compile_dir_new("active/s", "log").expect("the name itself is a legal folder");
        let error = run(zone.path(), plan).expect_err("a file is in the way");
        assert!(
            matches!(&error, ExecError::Refused(said) if said.starts_with("mkdir active/s/log")),
            "the refusal names the step that did not happen: {error}"
        );
        assert_eq!(
            std::fs::read_to_string(&taken).expect("read"),
            "somebody's file",
            "a folder verb never writes over a file"
        );
        assert!(taken.is_file(), "and never turns one into a directory");
        assert!(
            !zone.path().join(JOURNAL_REL).exists(),
            "the refusal clears the journal, so the next press re-plans"
        );
    }

    fn publish_plan(files: &[(&str, &str)]) -> Plan {
        Plan {
            verb: "render-publish".to_owned(),
            session: "active/s".to_owned(),
            steps: vec![PlanStep::PublishDir {
                from: "active/s/workspace/.staging-g".to_owned(),
                to: "active/s/workspace/g".to_owned(),
                files: files
                    .iter()
                    .map(|(name, text)| {
                        (
                            (*name).to_owned(),
                            keeper_core::agents::approval::sha256_hex(text.as_bytes()),
                        )
                    })
                    .collect(),
            }],
        }
    }

    /// R94R-01: a generation is published only when its staged tree is
    /// exactly what the plan wrote — a file left there by someone else, a
    /// link inside it, a staging folder that is itself a link, or a target
    /// already there refuses the move; a resume recognises a publication
    /// as its own only when the published tree is exactly its files.
    #[cfg(unix)]
    #[test]
    fn a_generation_is_published_only_as_exactly_what_was_staged() {
        let zone = zone();
        let workspace = zone.path().join("active/s/workspace");
        let staging = workspace.join(".staging-g");
        let target = workspace.join("g");
        let stage = || {
            std::fs::create_dir_all(staging.join("refs")).expect("mkdir");
            std::fs::write(staging.join("workflow.md"), "w").expect("write");
            std::fs::write(staging.join("refs/a.md"), "").expect("write");
        };
        let plan = || publish_plan(&[("workflow.md", "w"), ("refs/a.md", "")]);

        stage();
        std::fs::write(staging.join("stale.md"), "not ours").expect("plant");
        let refused = run(zone.path(), plan()).expect_err("an extra file");
        assert!(
            refused.to_string().contains("does not hold exactly"),
            "{refused}"
        );
        assert!(!target.exists());
        std::fs::remove_file(staging.join("stale.md")).expect("rm");

        // A staged file missing.
        std::fs::remove_file(staging.join("refs/a.md")).expect("rm");
        let refused = run(zone.path(), plan()).expect_err("a missing file");
        assert!(
            refused.to_string().contains("does not hold exactly"),
            "{refused}"
        );
        assert!(!target.exists());
        std::fs::write(staging.join("refs/a.md"), "").expect("write");

        // An empty output replaced by a link to bytes elsewhere.
        let elsewhere = zone.path().join("active/other.md");
        std::fs::write(&elsewhere, "").expect("write");
        std::fs::remove_file(staging.join("refs/a.md")).expect("rm");
        std::os::unix::fs::symlink(&elsewhere, staging.join("refs/a.md")).expect("link");
        let refused = run(zone.path(), plan()).expect_err("a link");
        assert!(
            refused.to_string().contains("refs/a.md is a link"),
            "{refused}"
        );
        assert!(!target.exists());
        std::fs::remove_dir_all(&staging).expect("rm");

        // A staging folder that is a link to another session's folder.
        let other = zone.path().join("active/other");
        std::fs::create_dir_all(other.join("refs")).expect("mkdir");
        std::fs::write(other.join("workflow.md"), "w").expect("write");
        std::fs::write(other.join("refs/a.md"), "").expect("write");
        std::os::unix::fs::symlink(&other, &staging).expect("link");
        let refused = run(zone.path(), plan()).expect_err("a linked staging");
        assert!(refused.to_string().contains("it is a link"), "{refused}");
        assert!(!target.exists());
        std::fs::remove_file(&staging).expect("rm");

        stage();
        run(zone.path(), plan()).expect("published");
        assert_eq!(
            std::fs::read_to_string(target.join("workflow.md")).expect("w"),
            "w"
        );
        assert!(!staging.exists());
        // A resume after the move: its own generation, and only that.
        run(zone.path(), plan()).expect("already published");
        std::fs::write(target.join("refs/a.md"), "edited").expect("edit");
        let refused = run(zone.path(), plan()).expect_err("not ours");
        assert!(
            refused
                .to_string()
                .contains("is not what this plan published"),
            "{refused}"
        );

        stage();
        let refused = run(zone.path(), plan()).expect_err("the target is there");
        assert!(refused.to_string().contains("already exists"), "{refused}");
    }

    /// R94R-01: a folder made new is never a link; a resume takes it only
    /// as the real folder its own run made.
    #[cfg(unix)]
    #[test]
    fn a_new_folder_is_never_a_link() {
        let zone = zone();
        std::fs::create_dir_all(zone.path().join("active/s/workspace")).expect("mkdir");
        std::fs::create_dir_all(zone.path().join("active/other")).expect("mkdir");
        let step = PlanStep::MkDirNew {
            path: "active/s/workspace/.staging-g".to_owned(),
        };
        std::os::unix::fs::symlink(
            zone.path().join("active/other"),
            zone.path().join("active/s/workspace/.staging-g"),
        )
        .expect("link");
        let refused = run_step(zone.path(), &step).expect_err("a link");
        assert!(refused.to_string().starts_with("mkdir "), "{refused}");
        std::fs::remove_file(zone.path().join("active/s/workspace/.staging-g")).expect("rm");
        run_step(zone.path(), &step).expect("made");
        run_step(zone.path(), &step).expect("a resume takes its own folder");
    }

    /// R94R-04: a new file is created only where none appeared since the
    /// plan; a resume finding its own bytes completes.
    #[test]
    fn a_created_file_never_replaces_one_that_appeared() {
        let zone = zone();
        let dir = zone.path().join("active/s/artifacts/run");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let step = PlanStep::CreateFile {
            path: "active/s/artifacts/run/.memlog.md".to_owned(),
            content: "new".to_owned(),
        };
        std::fs::write(dir.join(".memlog.md"), "somebody's").expect("plant");
        let refused = run_step(zone.path(), &step).expect_err("it appeared");
        assert!(refused.to_string().contains("appeared"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".memlog.md")).expect("kept"),
            "somebody's"
        );
        std::fs::remove_file(dir.join(".memlog.md")).expect("rm");
        run_step(zone.path(), &step).expect("created");
        run_step(zone.path(), &step).expect("a resume sees its own bytes");
        assert_eq!(
            std::fs::read_to_string(dir.join(".memlog.md")).expect("new"),
            "new"
        );
    }

    /// `dir`'s mode set to `mode` until the guard drops.
    #[cfg(unix)]
    struct Mode<'p>(&'p Path);

    #[cfg(unix)]
    impl<'p> Mode<'p> {
        fn set(dir: &'p Path, mode: u32) -> Mode<'p> {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).expect("chmod");
            Mode(dir)
        }
    }

    #[cfg(unix)]
    impl Drop for Mode<'_> {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    /// R94R-03, at the boundary a test can plant: a folder whose entry
    /// list cannot be synced (writable and searchable, not readable). A
    /// folder made in it — a new ancestor of the one asked for, or a
    /// generation moved into it — is a step that did not finish, never
    /// one the journal moves past: the step's effect is on the disk before
    /// the cursor advances.
    #[cfg(unix)]
    #[test]
    fn a_folder_the_journal_counts_is_synced_where_it_was_made() {
        let zone = zone();
        let session = zone.path().join("active/s");
        std::fs::create_dir_all(session.join("workspace")).expect("mkdir");
        {
            let _unsynced = Mode::set(&session, 0o300);
            let made = run(
                zone.path(),
                Plan {
                    verb: "memlog".to_owned(),
                    session: "active/s".to_owned(),
                    steps: vec![PlanStep::MkDir {
                        path: "active/s/artifacts/run".to_owned(),
                    }],
                },
            );
            assert!(
                made.as_ref().is_err_and(|error| error
                    .to_string()
                    .starts_with("mkdir active/s/artifacts/run")),
                "{made:?}"
            );
        }
        let workspace = session.join("workspace");
        std::fs::create_dir_all(workspace.join(".staging-g")).expect("mkdir");
        std::fs::write(workspace.join(".staging-g/workflow.md"), "w").expect("write");
        let _unsynced = Mode::set(&workspace, 0o300);
        let published = run(zone.path(), publish_plan(&[("workflow.md", "w")]));
        assert!(
            published
                .as_ref()
                .is_err_and(|error| error.to_string().starts_with("publish ")),
            "{published:?}"
        );
    }

    /// R244, R253 (R95K4-06, R95K5-04): the folders a durable vault write
    /// reaches its file through are on the disk before it is said done, from
    /// the drive's root down — a vault root not there yet, under a nested
    /// configured folder, included. With the drive's entry list unsyncable,
    /// the write is an error, and still one on a retry that finds the vault
    /// made; so too with the configured folder's, the vault root's or a
    /// folder under it; once every one syncs, done. A vault that lies
    /// outside the drive is synced from its own root.
    #[cfg(unix)]
    #[test]
    fn folders_a_durable_write_makes_are_synced_from_the_drive_down() {
        let drive = tempfile::tempdir().expect("drive");
        let vault = drive.path().join("10-notes/team");
        let deep = vault.join("knowledge/topic");
        {
            let _unsynced = Mode::set(drive.path(), 0o300);
            let made = make_dirs_within(drive.path(), &vault, &deep);
            assert!(made.is_err(), "{made:?}");
            assert!(deep.is_dir());
            let retried = make_dirs_within(drive.path(), &vault, &deep);
            assert!(retried.is_err(), "{retried:?}");
        }
        for unsyncable in [
            drive.path().join("10-notes"),
            vault.clone(),
            vault.join("knowledge"),
        ] {
            let _unsynced = Mode::set(&unsyncable, 0o300);
            let retried = make_dirs_within(drive.path(), &vault, &deep);
            assert!(retried.is_err(), "{}: {retried:?}", unsyncable.display());
        }
        make_dirs_within(drive.path(), &vault, &deep).expect("synced");

        let elsewhere = tempfile::tempdir().expect("elsewhere");
        let _unsynced = Mode::set(drive.path(), 0o300);
        make_dirs_within(drive.path(), elsewhere.path(), &elsewhere.path().join("k"))
            .expect("synced from its own root");
    }
}

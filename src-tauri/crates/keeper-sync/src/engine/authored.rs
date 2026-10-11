//! [`Engine::commit_paths`]: one commit of exactly a caller's request,
//! published by one compare-and-swap of the branch and rolled forward from
//! there — never undone.
//!
//! Nothing on the disk or in the index changes before the publication. The
//! commit is built from the tree of the one commit the branch named when
//! the request began and the requested bytes alone, and a durable record
//! under `.git` names it before the branch moves from exactly that commit.
//! After the branch moved, the index and the disk follow it path by path —
//! each only where `HEAD` still holds the commit's version and the path
//! still holds what was there before: anything else is a person's, made
//! meanwhile, and stays theirs. A process killed on the way leaves the
//! record; the next commit of the folder — a watcher pass's included —
//! finishes it first, or drops it when the commit graph says it was never
//! published, and a record it cannot settle holds the folder's commits with
//! the reason on its card.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gix::index::entry::Mode;
use serde::{Deserialize, Serialize};

use super::{blob_id, CommitFence, CommitPaths, CommitRequest, Engine, HashMap};
use crate::browse::{landing, Contained, Displaced, Root, Staging};
use crate::db::{self, WorkKind};
use crate::error::{Result, SyncError};
use crate::git;
use crate::lfs;
use crate::profile::SyncProfile;
use crate::provenance::{Provenance, SyncSource};

/// How long [`Engine::commit_paths`] waits for the profile's lane before it
/// says the folder is busy, and how often it looks.
pub(super) const COMMIT_LANE_WAIT: std::time::Duration = std::time::Duration::from_secs(120);
pub(super) const COMMIT_LANE_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// Where [`Engine::commit_paths`] — or the settling of one a kill stopped
/// — is: what a test stops at, as a killed process would, or acts at, as a
/// person would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cut {
    /// The folder is held and the request checked; its repository is not
    /// opened yet.
    Held,
    /// Checked against the commit the branch named when the request began;
    /// the commit is not built.
    Checked,
    /// The record is written; the branch has not moved.
    Recorded,
    /// Every byte the disk will get is read; the last checks, the fence
    /// and the compare-and-swap are next.
    Prepared,
    /// The branch moved; neither the index nor the disk followed yet.
    Published,
    /// The `n`th path's old file is moved aside; its new one is not placed.
    Displaced(usize),
    /// The `n`th path's new file is at that step of its staging.
    Staged(usize, Staging),
    /// A settling dropped the `n`th path's new file the commit had placed;
    /// the old file it moved aside is not settled yet.
    Dropped(usize),
    /// The index is read under its lock and not written yet.
    Indexed,
}

/// What a run stopped at a [`Cut`] returns.
const INTERRUPTED: &str = "the request was stopped before it finished";

/// One path of an authored commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: String,
    /// The entry `HEAD` held before — blob id and mode — which the disk
    /// held too; `None`: absent.
    before: Option<(String, u32)>,
    /// The entry the commit holds; `None`: the path goes.
    after: Option<(String, u32)>,
    /// The blob id of the bytes the disk gets — an LFS path's content, not
    /// its pointer; `None`: the file goes.
    disk: Option<String>,
}

impl Change {
    fn before_blob(&self) -> Option<&str> {
        self.before.as_ref().map(|(id, _)| id.as_str())
    }
}

/// An LFS object a commit points at, owed to the remote once that commit
/// is published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    path: String,
    oid: String,
    size: u64,
}

/// `.git/keeper-commit-paths.json`: the commit an [`Engine::commit_paths`]
/// is about to publish, written durably before the branch moves and removed
/// once the index and the disk followed it — or once it is known it never
/// was published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    v: u32,
    /// The request's own id: names what it moves aside and stages.
    request: String,
    /// `HEAD` before it, hex; `None` on an unborn branch.
    parent: Option<String>,
    tree: String,
    commit: String,
    changes: Vec<Change>,
    /// The uploads its commit owes: queued once it is published, never
    /// before, so a request refused on the way owes the remote nothing.
    uploads: Vec<Upload>,
}

impl Intent {
    const VERSION: u32 = 3;
    const FILE: &'static str = "keeper-commit-paths.json";

    fn path(repo: &gix::Repository) -> PathBuf {
        repo.git_dir().join(Intent::FILE)
    }

    /// The name the `n`th path's old file is moved aside under.
    fn aside(&self, n: usize) -> String {
        format!(".keeper-displaced-{}-{n}", self.request)
    }

    /// The name the `n`th path's new file is staged under beside it: the
    /// engine's own staging name, which no commit takes in, and this
    /// request's, so settling it clears what a kill left.
    fn staging(&self, n: usize) -> String {
        format!(".keeper.{}-{n}.tmp", self.request)
    }

    /// The name the `n`th path's new file is moved to when a settling
    /// takes it back off the path: checked there to be this request's own
    /// before it goes.
    fn taken(&self, n: usize) -> String {
        format!(".keeper-taken-{}-{n}", self.request)
    }

    /// Written whole and synced, the folder with it.
    fn write(&self, repo: &gix::Repository) -> Result<()> {
        use std::io::Write as _;
        let path = Intent::path(repo);
        let failed = |error: std::io::Error| SyncError::io("record a commit request", &path, error);
        let bytes =
            serde_json::to_vec(self).map_err(|error| SyncError::Config(error.to_string()))?;
        let mut staging = tempfile::NamedTempFile::new_in(repo.git_dir()).map_err(failed)?;
        staging
            .write_all(&bytes)
            .and_then(|()| staging.as_file().sync_all())
            .map_err(failed)?;
        staging
            .persist(&path)
            .map_err(|error| failed(error.error))?;
        Intent::sync_folder(repo)
    }

    fn read(repo: &gix::Repository) -> Result<Option<Intent>> {
        let path = Intent::path(repo);
        match std::fs::read(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SyncError::io("read a commit request", &path, error)),
            Ok(bytes) => serde_json::from_slice::<Intent>(&bytes)
                .ok()
                .filter(|intent| intent.v == Intent::VERSION)
                .map(Some)
                .ok_or_else(|| {
                    SyncError::Config(format!(
                        "{} does not read; remove it once the folder is checked",
                        path.display()
                    ))
                }),
        }
    }

    /// Removed only from the `.git` the folder `root` holds — the one the
    /// request began with — so a `.git` put in its place keeps its own
    /// record, whatever this request comes to.
    fn remove(repo: &gix::Repository, root: &Root) -> Result<()> {
        Engine::bound(repo, root)?;
        let path = Intent::path(repo);
        match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(SyncError::io("remove a commit request", &path, error))
            }
            _ => Intent::sync_folder(repo),
        }
    }

    fn sync_folder(repo: &gix::Repository) -> Result<()> {
        std::fs::File::open(repo.git_dir())
            .and_then(|folder| folder.sync_all())
            .map_err(|error| SyncError::io("sync .git", repo.git_dir(), error))
    }
}

fn hex(id: gix::hash::ObjectId) -> String {
    id.to_hex().to_string()
}

fn entry_of(entry: &(String, u32)) -> Result<(gix::hash::ObjectId, Mode)> {
    let id = gix::hash::ObjectId::from_hex(entry.0.as_bytes()).map_err(|error| {
        SyncError::Config(format!("a commit request names {}: {error}", entry.0))
    })?;
    let mode = Mode::from_bits(entry.1)
        .ok_or_else(|| SyncError::Config(format!("a commit request names mode {:o}", entry.1)))?;
    Ok((id, mode))
}

impl Engine {
    /// Commit exactly `request` as one commit under its subject and trailers,
    /// then queue the push (R130, R207).
    ///
    /// Under the profile's lane — the reservation every pass takes, waited
    /// for at most [`COMMIT_LANE_WAIT`] — and off the async executor, so a
    /// caller's lease renewal runs on while it works:
    ///
    /// 1. Every path must land on itself under the folder's root
    ///    ([`landing`]), held open from then on ([`Root`]) with the `.git`
    ///    inside it — a repository opened by the folder's name must be that
    ///    one — and no written or moved path, nor the attributes the commit
    ///    generates, may be or contain another; every written path is
    ///    guarded.
    /// 2. The branch `HEAD` names and its commit are read once
    ///    ([`git::commit::Base`]): every guard, every moved file and the
    ///    attributes the commit needs must be in that commit and on the disk
    ///    what the request read — anything else is [`CommitPaths::Guarded`]
    ///    and changes nothing.
    /// 3. The commit is built from that commit's tree and the requested
    ///    bytes alone — never what the disk or the index holds — its LFS
    ///    objects stored, and a durable record under `.git` names it and the
    ///    uploads it will owe.
    /// 4. Every byte the disk will get is read; then every guarded path is
    ///    read on the disk again, and `fence` is asked right before the
    ///    branch moves from that commit to the new one in one
    ///    compare-and-swap: the single publication. A guard that moved by
    ///    that re-read, a fence that says no or a branch that moved drops
    ///    the record and changes nothing — no upload is owed either. The
    ///    re-read is right before the publication, not one step with it: a
    ///    guarded file changed in the instant between them does not stop it.
    /// 5. The uploads are queued, and the index and the disk follow the
    ///    commit for exactly the paths `HEAD` holds as it does — read once,
    ///    right before the files follow — each only where it still holds
    ///    what was there before — a file is moved aside, checked, and the
    ///    new one placed, with its mode, or with a person's own executable
    ///    bit on the old file, where nothing took its place, so a save
    ///    meanwhile is never replaced, and a person's commit before that
    ///    read is never written over. Then the record goes, and the
    ///    activity rows and the push follow.
    ///
    /// What a kill leaves is settled by the next commit of the folder
    /// ([`Self::settle_commit_paths`]): never put back, only finished.
    pub async fn commit_paths(
        self: &Arc<Self>,
        profile_id: &str,
        request: &CommitRequest,
        fence: CommitFence,
    ) -> Result<CommitPaths> {
        let engine = Arc::clone(self);
        let (profile_id, request) = (profile_id.to_owned(), request.clone());
        tokio::task::spawn_blocking(move || {
            engine.commit_paths_blocking(&profile_id, &request, fence.as_ref())
        })
        .await
        .map_err(|error| SyncError::Config(format!("a commit request was cut off: {error}")))?
    }

    fn commit_paths_blocking(
        &self,
        profile_id: &str,
        request: &CommitRequest,
        fence: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<CommitPaths> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            return Err(SyncError::Config(format!(
                "no such sync profile: {profile_id}"
            )));
        };
        if !profile.direction.pushes() {
            return Err(SyncError::Config(format!(
                "{} never commits: it only pulls",
                profile.name
            )));
        }
        if !self.volume_ready(&profile)? {
            return Err(SyncError::MediaAbsent);
        }
        let waited = std::time::Instant::now();
        let _reservation = loop {
            if let Some(reservation) = self.reserve(&profile.id) {
                break reservation;
            }
            if waited.elapsed() >= COMMIT_LANE_WAIT {
                return Err(SyncError::Busy(profile.name.clone()));
            }
            std::thread::sleep(COMMIT_LANE_POLL);
        };
        self.commit_paths_held(&profile, request, fence, &|_| true)
    }

    /// [`Self::commit_paths`] once the lane is held. `at` is told each
    /// [`Cut`] as it is passed; `false` stops there as a killed process
    /// would, leaving what is done as it is.
    pub(crate) fn commit_paths_held(
        &self,
        profile: &SyncProfile,
        request: &CommitRequest,
        fence: &dyn Fn() -> bool,
        at: &dyn Fn(Cut) -> bool,
    ) -> Result<CommitPaths> {
        let root_path = profile.local_path.as_path();
        self.settle_commit_paths(profile)?;
        let root =
            Root::open(root_path).map_err(|error| SyncError::io("open", root_path, error))?;
        Self::checked(root_path, request)?;
        if !root.is_at(root_path) {
            return Err(SyncError::Config(format!(
                "{} was replaced while the request was checked; nothing is committed",
                root_path.display()
            )));
        }
        if !fence() {
            return Ok(CommitPaths::Fenced);
        }
        self.clear_stale_merge(profile)?;
        if !at(Cut::Held) {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }
        Self::bound_at(&root, root_path, &root_path.join(".git"))?;
        let repo = self.open_repo(profile)?;
        Self::bound(&repo, &root)?;
        let base = git::commit::Base::of(&repo)?;

        for (path, planned) in &request.guards {
            if base.blob(path) != *planned || Self::disk_blob(&root, path)? != *planned {
                tracing::info!(
                    profile = profile.name,
                    path,
                    "a guarded path changed since it was read; nothing is written"
                );
                return Ok(CommitPaths::Guarded { path: path.clone() });
            }
        }

        let attributes = Path::new(".gitattributes");
        let mut prefixes: Vec<&Path> = request
            .moves
            .iter()
            .map(|(from, _)| Path::new(from))
            .collect();
        prefixes.extend(request.moves.iter().map(|(_, to)| Path::new(to)));
        prefixes.extend(request.writes.iter().map(|(path, _)| Path::new(path)));
        prefixes.push(attributes);
        let mut at_base = base.files_under(&prefixes)?.into_iter();
        let moved_from: Vec<_> = at_base.by_ref().take(request.moves.len()).collect();
        let moved_to: Vec<_> = at_base.by_ref().take(request.moves.len()).collect();
        let written: Vec<_> = at_base.by_ref().take(request.writes.len()).collect();
        let base_attributes = at_base.next().unwrap_or_default();
        let exactly = |files: &[(PathBuf, gix::hash::ObjectId, Mode)], path: &str| {
            files
                .iter()
                .find(|(file, _, _)| file == Path::new(path))
                .map(|(_, id, mode)| (hex(*id), mode.bits()))
        };

        let mut changes: Vec<Change> = Vec::new();
        for (((from, to), files), there) in request.moves.iter().zip(&moved_from).zip(&moved_to) {
            if files.is_empty() {
                return Err(SyncError::Config(format!(
                    "{from} is not in the commit at HEAD; nothing is moved"
                )));
            }
            if !there.is_empty() || Self::disk_blob(&root, to)?.is_some() {
                return Err(SyncError::Config(format!(
                    "{to} exists already; {from} is not moved there"
                )));
            }
            for (file, id, mode) in files {
                let file = file.to_string_lossy().into_owned();
                // A move takes only what the request read: a file it names
                // no guard for came since — an addition the plan never saw.
                if !request.guards.iter().any(|(guarded, _)| *guarded == file)
                    || Self::disk_blob(&root, &file)? != Some(hex(*id))
                {
                    return Ok(CommitPaths::Guarded { path: file });
                }
                let rest = &file[from.len()..];
                let entry = Some((hex(*id), mode.bits()));
                changes.push(Change {
                    path: file.clone(),
                    before: entry.clone(),
                    after: None,
                    disk: None,
                });
                changes.push(Change {
                    path: format!("{to}{rest}"),
                    before: None,
                    after: entry,
                    disk: Some(hex(*id)),
                });
            }
            if let Some(path) = Self::stray(&root, from, files)? {
                return Ok(CommitPaths::Guarded { path });
            }
        }

        let store = lfs::store::LfsStore::in_git_dir(root_path.join(".git"));
        let routed: Vec<(&Path, &[u8])> = request
            .writes
            .iter()
            .filter_map(|(path, bytes)| Some((Path::new(path.as_str()), bytes.as_deref()?)))
            .collect();
        let base_attributes_text = match base_attributes
            .iter()
            .find(|(file, _, _)| file == attributes)
        {
            Some((_, id, _)) => String::from_utf8_lossy(
                &repo
                    .find_object(*id)
                    .map_err(|err| SyncError::Git(format!("could not read .gitattributes: {err}")))?
                    .detach()
                    .data,
            )
            .into_owned(),
            None => String::new(),
        };
        let staging =
            lfs::stage::prepare_authored(&repo, profile, &store, &routed, &base_attributes_text)?;
        if staging.attributes.is_some() {
            Self::apart_from_generated(request, ".gitattributes")?;
        }
        let write_blob = |bytes: &[u8], path: &str| -> Result<gix::hash::ObjectId> {
            repo.write_blob(bytes)
                .map(|id| id.detach())
                .map_err(|err| SyncError::Git(format!("could not write {path}: {err}")))
        };
        for ((path, bytes), files) in request.writes.iter().zip(&written) {
            if files.iter().any(|(file, _, _)| file != Path::new(path)) {
                return Err(SyncError::Config(format!(
                    "{path} is a folder at HEAD; nothing is written over it"
                )));
            }
            let before = exactly(files, path);
            let (after, disk) = match bytes {
                Some(bytes) => {
                    let committed = staging
                        .substitutions
                        .get(Path::new(path))
                        .map_or(bytes.as_slice(), Vec::as_slice);
                    let mode = match &before {
                        Some((_, mode)) if *mode == Mode::FILE_EXECUTABLE.bits() => *mode,
                        _ => Mode::FILE.bits(),
                    };
                    (
                        Some((hex(write_blob(committed, path)?), mode)),
                        Some(blob_id(bytes)),
                    )
                }
                None => (None, None),
            };
            changes.push(Change {
                path: path.clone(),
                before,
                after,
                disk,
            });
        }
        if let Some(text) = &staging.attributes {
            let before = exactly(&base_attributes, ".gitattributes");
            if Self::disk_blob(&root, ".gitattributes")?
                != before.as_ref().map(|(id, _)| id.clone())
            {
                return Ok(CommitPaths::Guarded {
                    path: ".gitattributes".to_owned(),
                });
            }
            changes.push(Change {
                path: ".gitattributes".to_owned(),
                before,
                after: Some((
                    hex(write_blob(text.as_bytes(), ".gitattributes")?),
                    Mode::FILE.bits(),
                )),
                disk: Some(blob_id(text.as_bytes())),
            });
        }

        let mut staged = git::commit::StagedChange::default();
        let mut entries: Vec<(PathBuf, Option<(gix::hash::ObjectId, Mode)>)> = Vec::new();
        for change in &changes {
            let path = PathBuf::from(&change.path);
            match (&change.before, &change.after) {
                (Some(_), Some(_)) => staged.modified.push(path.clone()),
                (None, Some(_)) => staged.added.push(path.clone()),
                (Some(_), None) => staged.deleted.push(path.clone()),
                (None, None) => continue,
            }
            entries.push((path, change.after.as_ref().map(entry_of).transpose()?));
        }
        if !at(Cut::Checked) {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }
        let device = self.device();
        let (name, email) = git::commit::author_for(profile, &device);
        let author = gix::actor::Signature {
            name: name.into(),
            email: email.into(),
            time: gix::date::Time::new(self.platform.now_ms() / 1_000, 0),
        };
        let provenance = Provenance::new(
            &profile.name,
            &device.label,
            &device.id,
            self.platform.host_label(),
            Self::commit_source(profile, SyncSource::Bot),
        )
        .with_tags(profile.tags.clone());
        let Some(built) = git::commit::build_authored(
            &repo,
            &base,
            &staged,
            &provenance,
            &author,
            git::commit::Authored {
                subject: &request.subject,
                trailers: &request.trailers,
                entries: &entries,
            },
        )?
        else {
            return Ok(CommitPaths::Unchanged);
        };

        let intent = Intent {
            v: Intent::VERSION,
            request: ulid::Ulid::new().to_string(),
            parent: built.parent.map(hex),
            tree: hex(built.tree),
            commit: hex(built.commit),
            changes,
            uploads: staging
                .uploads
                .iter()
                .map(|object| Upload {
                    path: lfs::stage::index_key(&object.path),
                    oid: object.oid.clone(),
                    size: object.size,
                })
                .collect(),
        };
        Self::bound(&repo, &root)?;
        intent.write(&repo)?;
        if !at(Cut::Recorded) {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }
        let abandon = |outcome: CommitPaths| -> Result<CommitPaths> {
            Intent::remove(&repo, &root)?;
            Ok(outcome)
        };
        // Every byte the disk will get, read before the last checks: after
        // the publication the folder is reached only through the root held
        // open, and nothing long runs between the fence and the branch.
        let ready = intent
            .changes
            .iter()
            .map(|change| match (&change.after, &change.disk) {
                (Some(after), Some(disk)) => {
                    Self::disk_bytes(&repo, &store, &after.0, disk).map(Some)
                }
                _ => Ok(None),
            })
            .collect::<Result<Vec<_>>>()?;
        if !at(Cut::Prepared) {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }
        // Read on the disk again right before the publication: a declaration
        // a person changed since the guards were checked holds it.
        for (path, planned) in &request.guards {
            if Self::disk_blob(&root, path)? != *planned {
                return abandon(CommitPaths::Guarded { path: path.clone() });
            }
        }
        for ((from, _), files) in request.moves.iter().zip(&moved_from) {
            if let Some(path) = Self::stray(&root, from, files)? {
                return abandon(CommitPaths::Guarded { path });
            }
        }
        for change in &intent.changes {
            if Self::disk_blob(&root, &change.path)?.as_deref() != change.before_blob() {
                return abandon(CommitPaths::Guarded {
                    path: change.path.clone(),
                });
            }
        }
        let last = || -> Result<bool> {
            Self::bound(&repo, &root)?;
            Ok(fence())
        };
        match git::commit::publish(&repo, &base, &built, &last)? {
            git::commit::Publication::Published => {}
            git::commit::Publication::Refused => return abandon(CommitPaths::Fenced),
            git::commit::Publication::Moved => {
                tracing::info!(
                    profile = profile.name,
                    "the branch moved while a request was committed; nothing is written"
                );
                return abandon(CommitPaths::Guarded {
                    path: "HEAD".to_owned(),
                });
            }
        }
        if !at(Cut::Published) {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }

        // Published: from here everything rolls forward, and nothing that
        // fails says the commit was not made.
        let mut lfs_units = HashMap::new();
        let followed = (|| -> Result<()> {
            lfs_units = self.owe_uploads(profile, &intent)?;
            self.roll_forward(&repo, &root, &store, &intent, Some(&ready), at)
        })();
        match followed {
            Ok(()) => Intent::remove(&repo, &root)?,
            Err(error) => {
                tracing::warn!(profile = profile.name, %error, "a published commit's files could not all follow it yet; the next commit of the folder finishes them");
                self.warn(
                    &profile.id,
                    &profile.name,
                    format!("A commit's files could not all be written yet ({error}); this folder commits nothing else until they are."),
                );
            }
        }
        self.bump_counters(&profile.id, |counters| counters.commits += 1);
        if staging.attributes.is_some() {
            self.bump_counters(&profile.id, |counters| counters.attribute_writes += 1);
        }
        let now = self.platform.now_ms();
        let booked = (|| -> Result<()> {
            for object in &staging.uploads {
                let key = lfs::stage::index_key(&object.path);
                self.with_db(|conn| {
                    db::note_local_authorship(
                        conn,
                        &profile.id,
                        &key,
                        now,
                        &object.oid,
                        object.size,
                    )
                })?;
            }
            self.record_commit_activity(profile, &staged, &lfs_units, None)?;
            self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &WorkKind::Push, now, now))
                .map(drop)
        })();
        if let Err(error) = booked {
            tracing::warn!(profile = profile.name, %error, "a published commit's activity or push could not be recorded; the next pass pushes it");
        }
        self.refresh_pending(&profile.id);
        Ok(CommitPaths::Committed {
            commit: intent.commit,
        })
    }

    /// Queue the upload of every LFS object `intent`'s commit points at:
    /// asked once it is published, never before, so a request refused on
    /// the way owes the remote nothing, and again by its settling, so a kill
    /// right after the publication loses no obligation. A unit queued
    /// already is that unit.
    fn owe_uploads(&self, profile: &SyncProfile, intent: &Intent) -> Result<HashMap<PathBuf, i64>> {
        let now = self.platform.now_ms();
        let mut units = HashMap::with_capacity(intent.uploads.len());
        for upload in intent
            .uploads
            .iter()
            .filter(|upload| Self::upload_is_needed(upload.size))
        {
            let unit = WorkKind::LfsUpload {
                oid: upload.oid.clone(),
                size: upload.size,
            };
            let id = self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &unit, now, now))?;
            self.with_db(|conn| db::label_unit(conn, id, &upload.path))?;
            units.insert(PathBuf::from(&upload.path), id);
        }
        Ok(units)
    }

    /// Refuse to go on unless `repo` is the repository of the folder `root`
    /// holds — its work tree that folder, its `.git` the one inside it — so
    /// a folder swapped in under the profile's name, another checkout of
    /// the same history even, never gets a record, a commit or a branch
    /// move of this request's.
    fn bound(repo: &gix::Repository, root: &Root) -> Result<()> {
        match repo.workdir() {
            Some(folder) => Self::bound_at(root, folder, repo.git_dir()),
            None => Self::bound_at(root, Path::new(""), repo.git_dir()),
        }
    }

    /// [`Self::bound`] by the paths a repository is opened by: asked before
    /// it is opened too, so a swapped-in checkout is not even opened.
    fn bound_at(root: &Root, folder: &Path, git_dir: &Path) -> Result<()> {
        if root.is_at(folder) && root.holds_git_dir(git_dir) {
            return Ok(());
        }
        Err(SyncError::Config(
            "the folder was replaced while its commit was written; nothing more of it is written"
                .to_owned(),
        ))
    }

    /// Settle what a [`Self::commit_paths`] killed on the way left, before
    /// anything else commits the folder: a record whose commit `HEAD`
    /// reaches is rolled forward; one whose commit it does not reach was
    /// never published and is dropped, nothing of it being anywhere. Never
    /// put back. Whether it was published is read from the commit graph
    /// alone, never from commit times; a record whose commit or history
    /// cannot be read is not known to be either, and like any record that
    /// cannot be settled it stays, says why on the folder's card and holds
    /// the folder's commits. Whether there was a record.
    pub(crate) fn settle_commit_paths(&self, profile: &SyncProfile) -> Result<bool> {
        self.settle_commit_paths_held(profile, &|_| true)
    }

    /// [`Self::settle_commit_paths`], `at` told each [`Cut`] it passes;
    /// `false` stops there as a killed process would.
    pub(crate) fn settle_commit_paths_held(
        &self,
        profile: &SyncProfile,
        at: &dyn Fn(Cut) -> bool,
    ) -> Result<bool> {
        // Asked before the repository is opened: a folder never cloned has
        // nothing to settle, and every pass asks.
        let record = profile.local_path.join(".git").join(Intent::FILE);
        if std::fs::symlink_metadata(&record)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(false);
        }
        let settled = (|| -> Result<bool> {
            let repo = self.open_repo(profile)?;
            let Some(intent) = Intent::read(&repo)? else {
                return Ok(false);
            };
            let root_path = profile.local_path.as_path();
            let root =
                Root::open(root_path).map_err(|error| SyncError::io("open", root_path, error))?;
            Self::bound(&repo, &root)?;
            let published = git::history::reaches(root_path, &intent.commit).map_err(|error| {
                SyncError::Config(format!(
                    "whether a cut-off commit was published is not known: {error}"
                ))
            })?;
            if !published {
                tracing::warn!(
                    profile = profile.name,
                    "a request to commit paths was cut off before its commit was published; nothing of it is anywhere"
                );
                Intent::remove(&repo, &root)?;
                return Ok(true);
            }
            let store = lfs::store::LfsStore::in_git_dir(root_path.join(".git"));
            self.owe_uploads(profile, &intent)?;
            self.roll_forward(&repo, &root, &store, &intent, None, at)?;
            Intent::remove(&repo, &root)?;
            tracing::info!(
                profile = profile.name,
                "a commit cut off after its publication was finished"
            );
            Ok(true)
        })();
        if let Err(error) = &settled {
            self.warn(
                &profile.id,
                &profile.name,
                format!("A commit's files could not all be written yet ({error}); this folder commits nothing else until they are."),
            );
        }
        settled
    }

    /// Whether a commit of the profile `profile_id` is published and its
    /// files have not all followed it yet — or its record does not read:
    /// either holds the folder's commits until it is settled.
    pub fn unsettled_commit(&self, profile_id: &str) -> Result<bool> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            return Err(SyncError::Config(format!(
                "no such sync profile: {profile_id}"
            )));
        };
        let record = profile.local_path.join(".git").join(Intent::FILE);
        match std::fs::symlink_metadata(&record) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(SyncError::io("read a commit request", &record, error)),
        }
    }

    /// Have the index and the disk follow `intent`'s published commit for
    /// each of its paths `HEAD` holds as the commit does — read once, now:
    /// a path a later commit changed again, a person's own made right after
    /// the publication included, is that commit's, and its file and index
    /// entry stay as they are, and what a kill left of this commit at it is
    /// settled against what `HEAD` holds there ([`Self::put_back`]). What a
    /// kill staged is cleared for every path.
    fn roll_forward(
        &self,
        repo: &gix::Repository,
        root: &Root,
        store: &lfs::store::LfsStore,
        intent: &Intent,
        ready: Option<&[Option<Vec<u8>>]>,
        at: &dyn Fn(Cut) -> bool,
    ) -> Result<()> {
        let paths: Vec<&Path> = intent
            .changes
            .iter()
            .map(|change| Path::new(&change.path))
            .collect();
        // `HEAD` is read by the folder's name: only while it is the folder
        // held.
        Self::bound(repo, root)?;
        let at_head = git::commit::head_files_under(repo, &paths)?;
        let mut followed: Vec<git::commit::Followed> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        for (n, change) in intent.changes.iter().enumerate() {
            let now = at_head[n]
                .iter()
                .find(|(file, _, _)| file == Path::new(&change.path))
                .map(|(_, id, mode)| (hex(*id), mode.bits()));
            if now != change.after {
                Self::put_back(
                    root,
                    intent,
                    n,
                    now.as_ref().map(|(id, mode)| (id.as_str(), *mode)),
                    at,
                )?;
                continue;
            }
            current.push(n);
            followed.push((
                PathBuf::from(&change.path),
                change.before.as_ref().map(entry_of).transpose()?,
                change.after.as_ref().map(entry_of).transpose()?,
            ));
        }
        // The disk first, through the folder held open: the index is reached
        // by its path, and a root swapped meanwhile must stop it there.
        for n in current {
            let change = &intent.changes[n];
            let wanted = match (&change.after, &change.disk) {
                (Some(after), Some(disk)) => {
                    let bytes = match ready.and_then(|ready| ready[n].clone()) {
                        Some(bytes) => bytes,
                        None => Self::disk_bytes(repo, store, &after.0, disk)?,
                    };
                    Some((
                        bytes,
                        disk.as_str(),
                        after.1 == Mode::FILE_EXECUTABLE.bits(),
                    ))
                }
                _ => None,
            };
            Self::materialize(
                root,
                &change.path,
                change
                    .before
                    .as_ref()
                    .map(|(id, mode)| (id.as_str(), *mode == Mode::FILE_EXECUTABLE.bits())),
                wanted
                    .as_ref()
                    .map(|(bytes, id, executable)| (bytes.as_slice(), *id, *executable)),
                &intent.aside(n),
                &intent.staging(n),
                &|step| at(Cut::Staged(n, step)),
                &|| at(Cut::Displaced(n)),
            )?;
        }
        for (n, change) in intent.changes.iter().enumerate() {
            if let Ok(here) = Contained::open(root, &change.path, false) {
                here.clear(&intent.staging(n))
                    .map_err(|error| SyncError::io("clear a staged file", &change.path, error))?;
            }
        }
        Self::bound(repo, root)?;
        git::commit::index_follow(repo, &followed, &|| {
            at(Cut::Indexed);
        })?;
        // A folder a move or a deletion emptied goes with it.
        for change in intent
            .changes
            .iter()
            .filter(|change| change.after.is_none())
        {
            let mut folder = Path::new(&change.path).parent();
            while let Some(path) = folder.filter(|path| !path.as_os_str().is_empty()) {
                let path_text = path.to_string_lossy();
                if let Ok(here) = Contained::open(root, &path_text, false) {
                    let _ = here.remove_if_empty();
                }
                folder = path.parent();
            }
        }
        Ok(())
    }

    /// Settle what a kill left at the `n`th path of `intent` once `HEAD` —
    /// read once, `head` the entry it holds there (blob id and mode),
    /// `None`: no file — no longer holds it as the commit does: a later
    /// commit changed it, a person's taking the commit back or deleting the
    /// path included.
    ///
    /// It completes only where whose everything there is can be told. The
    /// commit's own new file is told by being still linked under its
    /// staging name, holding the commit's bytes, with the executable bit the
    /// commit gave it; it is moved off the path and dropped where `HEAD`
    /// holds the old file again — the old file it moved aside going back —
    /// or no file. The old file moved aside — the committed bytes, `before`
    /// — goes back where `HEAD` holds it again, mode included, and the path
    /// is free, and goes where `HEAD` holds no file, so a deletion committed
    /// since stays one; a person's own file at the path stays and the old
    /// one goes — only while it holds the committed bytes with the
    /// executable bit they were committed with, read right before it goes.
    /// Anything else moved aside is a person's and goes back. No mode is
    /// ever moved from one file to another. Where whose a file is cannot be
    /// told — the path holds the commit's bytes with nothing to say they
    /// are the commit's, or the commit's own file has another executable
    /// bit than the commit gave it, or the old file other bytes or another
    /// bit than it was committed with, or `HEAD` holds the old file again
    /// with the old file gone, or a third version with anything of the
    /// commit's still there — nothing is moved or dropped: the files stay as
    /// they are, and the record with them, until a person settles the path.
    /// `at` is told [`Cut::Dropped`]; `false` stops there.
    fn put_back(
        root: &Root,
        intent: &Intent,
        n: usize,
        head: Option<(&str, u32)>,
        at: &dyn Fn(Cut) -> bool,
    ) -> Result<()> {
        let change = &intent.changes[n];
        let path = change.path.as_str();
        let failed = |what: &'static str| {
            let path = path.to_owned();
            move |error: std::io::Error| SyncError::io(what, path, error)
        };
        let kept_beside = |kept: &str| {
            SyncError::Config(format!(
                "{path} changed while a commit taken back since was written; the file it \
                 replaced is kept beside it as {kept}"
            ))
        };
        let saved_there = |kept: &str| {
            SyncError::Config(format!(
                "{path} changed while a commit taken back since was settled; a file saved \
                 there is kept beside it as {kept}"
            ))
        };
        let left_there = || {
            SyncError::Config(format!(
                "{path} changed while a commit taken back since was written; whose the file \
                 there is cannot be told, so it stays as it is until it is settled by hand"
            ))
        };
        let here = match Contained::open(root, path, false) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            opened => opened.map_err(failed("open a committed path"))?,
        };
        let (aside, staging, taken) = (intent.aside(n), intent.staging(n), intent.taken(n));
        let wanted = change.disk.as_deref();
        let before = change
            .before
            .as_ref()
            .map(|(id, mode)| (id.as_str(), *mode));
        let before_blob = change.before_blob();
        let executable = |mode: u32| mode == Mode::FILE_EXECUTABLE.bits();
        // The old file moved aside is the commit's only while it holds the
        // committed bytes with the bit they were committed with: read last,
        // right before it goes or the commit's file goes for it. Anything
        // else — a person's save, a bit set or cleared, a file that cannot
        // be read — is a person's change, and it stays, the record with it.
        let committed = |old: &Displaced<'_>| -> Result<bool> {
            match before {
                Some((blob, mode)) => old
                    .holds(blob, executable(mode))
                    .map_err(failed("read a moved-aside file")),
                None => Ok(false),
            }
        };
        let drop_old = |old: Displaced<'_>| -> Result<()> {
            if !committed(&old)? {
                return Err(kept_beside(&aside));
            }
            old.discard().map_err(failed("drop a moved-aside file"))
        };
        // Moved off the path by a settling a kill stopped: back, and told
        // again with the rest.
        if let Some(off) = here
            .displaced(&taken)
            .map_err(failed("read a moved-aside file"))?
        {
            if let Some(kept) = off.restore().map_err(failed("put a file back"))? {
                return Err(saved_there(&kept));
            }
        }
        let earlier = match here
            .displaced(&aside)
            .map_err(failed("read a moved-aside file"))?
        {
            Some(old)
                if old
                    .blob()
                    .map_err(failed("read a moved-aside file"))?
                    .as_deref()
                    != before_blob =>
            {
                // Saved between the read and the move: the person's, put
                // back.
                return match old.restore().map_err(failed("put a file back"))? {
                    Some(kept) => Err(saved_there(&kept)),
                    None => Ok(()),
                };
            }
            earlier => earlier,
        };
        let occupant = here.blob().map_err(failed("read a committed path"))?;
        if wanted.is_none() || occupant.as_deref() != wanted {
            // Nothing of the commit's at the path: a person's file, or none.
            let Some(old) = earlier else {
                return Ok(());
            };
            return match head {
                None => drop_old(old),
                Some((id, _)) if occupant.is_some() && Some(id) == before_blob => {
                    match old.restore().map_err(failed("put a file back"))? {
                        Some(kept) => match here
                            .displaced(&kept)
                            .map_err(failed("read a moved-aside file"))?
                        {
                            Some(left) => drop_old(left),
                            None => Ok(()),
                        },
                        None => Ok(()),
                    }
                }
                Some(entry) if Some(entry) == before => {
                    match old.restore().map_err(failed("put a file back"))? {
                        Some(kept) => Err(kept_beside(&kept)),
                        None => Ok(()),
                    }
                }
                Some(_) => Err(kept_beside(&aside)),
            };
        }
        // The commit's bytes: its own file only while still linked under
        // its staging name — and a third version at `HEAD` leaves no choice
        // that cannot lose a person's file. Where `HEAD` holds the old file
        // again, the old file must be there to go back.
        let reversed = head == before;
        if !(reversed || head.is_none())
            || (reversed && before.is_some() && earlier.is_none())
            || !here
                .is_linked_as(&staging)
                .map_err(failed("read a staged file"))?
        {
            return Err(left_there());
        }
        // Modes are never moved from one file to another: the commit's
        // file with another bit than the commit gave it, or the old file
        // with another than it was committed with, is a person's change
        // nothing here can carry without risking it, so both stay.
        let given = change.after.as_ref().map(|(_, mode)| executable(*mode));
        let bit = here.executable().map_err(failed("read a committed path"))?;
        if given != Some(bit) {
            return Err(left_there());
        }
        if let Some(old) = &earlier {
            if !committed(old)? {
                return Err(left_there());
            }
        }
        let Some(off) = here
            .displace(&taken)
            .map_err(failed("move a staged file aside"))?
        else {
            return Err(left_there());
        };
        let still = off
            .is_linked_as(&staging)
            .map_err(failed("read a staged file"))?
            && off
                .blob()
                .map_err(failed("read a moved-aside file"))?
                .as_deref()
                == wanted
            && off
                .executable()
                .map_err(failed("read a moved-aside file"))?
                == bit;
        if !still {
            return match off.restore().map_err(failed("put a file back"))? {
                Some(kept) => Err(saved_there(&kept)),
                None => Err(left_there()),
            };
        }
        off.discard().map_err(failed("drop a staged file"))?;
        if !at(Cut::Dropped(n)) {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }
        let Some(old) = earlier else {
            return Ok(());
        };
        if head.is_none() {
            return drop_old(old);
        }
        match old.restore().map_err(failed("put a file back"))? {
            Some(kept) => Err(kept_beside(&kept)),
            None => Ok(()),
        }
    }

    /// The bytes whose blob id is `disk`, for the commit's blob `after`:
    /// the blob itself, or — an LFS path — the object its pointer names.
    fn disk_bytes(
        repo: &gix::Repository,
        store: &lfs::store::LfsStore,
        after: &str,
        disk: &str,
    ) -> Result<Vec<u8>> {
        let id = gix::hash::ObjectId::from_hex(after.as_bytes()).map_err(|error| {
            SyncError::Config(format!("a commit request names {after}: {error}"))
        })?;
        let blob = repo
            .find_object(id)
            .map_err(|err| SyncError::Git(format!("could not read {after}: {err}")))?
            .detach()
            .data;
        if blob_id(&blob) == disk {
            return Ok(blob);
        }
        let object = lfs::pointer::Pointer::parse(&blob)
            .map(|pointer| store.object_path(&pointer.oid))
            .ok_or_else(|| {
                SyncError::Config(format!("{after} is neither the file nor its pointer"))
            })?;
        let bytes = std::fs::read(&object)
            .map_err(|error| SyncError::io("read an LFS object", &object, error))?;
        if blob_id(&bytes) != disk {
            return Err(SyncError::Integrity {
                subject: object.to_string_lossy().into_owned(),
                expected: disk.to_owned(),
                actual: blob_id(&bytes),
            });
        }
        Ok(bytes)
    }

    /// Have `path` hold `wanted` (bytes, their blob id, and whether the
    /// file is executable; `None`: gone) if it still holds `before` (its
    /// blob id, and whether it was committed executable): the old file
    /// moved aside as `aside`, checked, and dropped; the new one staged as
    /// `staging` and placed only where nothing took its place. Anything
    /// else at `path` is a person's and stays, and what was moved aside and
    /// turned out to be theirs goes back. A person's own executable bit on
    /// the old file — its bytes the committed ones — is theirs too: the new
    /// bytes take it, and a file the commit removes stays. `staged` and
    /// `displaced` are told those steps, as [`Cut::Staged`] and
    /// [`Cut::Displaced`].
    #[allow(clippy::too_many_arguments)]
    fn materialize(
        root: &Root,
        path: &str,
        before: Option<(&str, bool)>,
        wanted: Option<(&[u8], &str, bool)>,
        aside: &str,
        staging: &str,
        staged: &dyn Fn(Staging) -> bool,
        displaced: &dyn Fn() -> bool,
    ) -> Result<()> {
        let failed = |what: &'static str| {
            let path = path.to_owned();
            move |error: std::io::Error| SyncError::io(what, path, error)
        };
        let before_blob = before.map(|(id, _)| id);
        let here = match Contained::open(root, path, wanted.is_some()) {
            Err(error) if wanted.is_none() && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(());
            }
            opened => opened.map_err(failed("open a committed path"))?,
        };
        let now = here.blob().map_err(failed("read a committed path"))?;
        let left = || {
            tracing::info!(
                path,
                "a file changed since its commit was made; it is left as it is"
            );
        };
        let place = |(bytes, _, executable): (&[u8], &str, bool)| -> Result<()> {
            if !here
                .create(bytes, staging, executable, staged)
                .map_err(failed("write a committed path"))?
            {
                left();
            }
            Ok(())
        };
        // The old file moved aside, its bytes the committed ones: settled
        // with the executable bit it has — a person's, when it is not the
        // committed one.
        let settle = |old: Displaced<'_>, put: bool| -> Result<()> {
            let executable = old
                .executable()
                .map_err(failed("read a moved-aside file"))?;
            let theirs = before.is_some_and(|(_, committed)| committed != executable);
            match wanted {
                None if theirs => {
                    left();
                    if let Some(kept) = old.restore().map_err(failed("put a file back"))? {
                        tracing::warn!(path, kept, "a person's file was kept beside its path");
                    }
                    return Ok(());
                }
                Some((bytes, id, committed)) if put => {
                    place((bytes, id, if theirs { executable } else { committed }))?;
                }
                _ => {}
            }
            old.discard().map_err(failed("drop a moved-aside file"))
        };
        // What a kill between moving the old file aside and placing the new
        // one left: the old file goes once the new one is placed; anything
        // else moved aside goes back.
        if let Some(earlier) = here
            .displaced(aside)
            .map_err(failed("read a moved-aside file"))?
        {
            if earlier
                .blob()
                .map_err(failed("read a moved-aside file"))?
                .as_deref()
                != before_blob
            {
                if let Some(kept) = earlier.restore().map_err(failed("put a file back"))? {
                    tracing::warn!(path, kept, "a person's file was kept beside its path");
                }
                return Ok(());
            }
            return settle(earlier, now.is_none());
        }
        if now.as_deref() == wanted.map(|(_, id, _)| id) {
            return Ok(());
        }
        if now.as_deref() != before_blob {
            left();
            return Ok(());
        }
        if before.is_none() {
            wanted.map(place).transpose()?;
            return Ok(());
        }
        let Some(old) = here.displace(aside).map_err(failed("move a file aside"))? else {
            left();
            return Ok(());
        };
        if !displaced() {
            return Err(SyncError::Config(INTERRUPTED.to_owned()));
        }
        if old
            .blob()
            .map_err(failed("read a moved-aside file"))?
            .as_deref()
            != before_blob
        {
            // Saved between the read and the move: the person's, put back.
            if let Some(kept) = old.restore().map_err(failed("put a file back"))? {
                tracing::warn!(path, kept, "a person's file was kept beside its path");
            }
            return Ok(());
        }
        settle(old, true)
    }

    /// Refuse `request` unless every path lands on itself under `root`, no
    /// written or moved path is or contains another, and every written path
    /// is guarded.
    fn checked(root: &Path, request: &CommitRequest) -> Result<()> {
        let named: Vec<&String> = Self::named(request).collect();
        for path in named
            .iter()
            .copied()
            .chain(request.guards.iter().map(|(path, _)| path))
        {
            Self::fenced(root, path)?;
        }
        for (at, one) in named.iter().enumerate() {
            for other in &named[at + 1..] {
                if within(one, other) || within(other, one) {
                    return Err(SyncError::Config(format!(
                        "{one} and {other} are one path or one inside the other; nothing is \
                         committed"
                    )));
                }
            }
        }
        if let Some((path, _)) = request
            .writes
            .iter()
            .find(|(path, _)| !request.guards.iter().any(|(guarded, _)| guarded == path))
        {
            return Err(SyncError::Config(format!(
                "{path} is written without saying what it holds now; nothing is committed"
            )));
        }
        Ok(())
    }

    /// Every path `request` writes or moves, from and to.
    fn named(request: &CommitRequest) -> impl Iterator<Item = &String> {
        request
            .writes
            .iter()
            .map(|(path, _)| path)
            .chain(request.moves.iter().flat_map(|(from, to)| [from, to]))
    }

    /// Refuse `request` when `generated` — a path the commit writes of its
    /// own, the attributes routing a file through LFS needs — is one of its
    /// written or moved paths, or inside or around one: one path would be
    /// changed twice.
    fn apart_from_generated(request: &CommitRequest, generated: &str) -> Result<()> {
        match Self::named(request).find(|path| within(path, generated) || within(generated, path)) {
            Some(path) => Err(SyncError::Config(format!(
                "the request names {path} and routes a file through LFS, whose rule in \
                 {generated} the commit writes; nothing is committed"
            ))),
            None => Ok(()),
        }
    }

    /// The blob id of what is at `path` on the disk, read through no link:
    /// `None` when nothing is, an empty id for what is not a file.
    fn disk_blob(root: &Root, path: &str) -> Result<Option<String>> {
        match Contained::open(root, path, false).and_then(|here| here.blob()) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SyncError::io("read a guarded file", path, error)),
            Ok(blob) => Ok(blob),
        }
    }

    /// The first thing on the disk under the moved `from` that `files` —
    /// what the commit holds there — does not name: an addition, committed
    /// or not, the request never read. A folder moves whole or not at all,
    /// so it holds the move and stays where it is with the folder.
    fn stray(
        root: &Root,
        from: &str,
        files: &[(PathBuf, gix::hash::ObjectId, Mode)],
    ) -> Result<Option<String>> {
        let on_disk = match crate::browse::members(root, from) {
            Ok(found) => found,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(SyncError::io("list a moved folder", from, error)),
        };
        Ok(on_disk
            .into_iter()
            .find(|path| !files.iter().any(|(file, _, _)| file == Path::new(path))))
    }

    /// Refuse `path` unless it lands on itself under `root`: plain names,
    /// no link on the way, and nothing of `.git`.
    fn fenced(root: &Path, path: &str) -> Result<()> {
        let refused = |why: String| SyncError::Config(format!("{path} is not committed: {why}"));
        let landed = landing(root, path).map_err(|refusal| refused(refusal.to_string()))?;
        if landed.first().is_some_and(|name| name == ".git") {
            return Err(refused("it is inside .git".to_owned()));
        }
        if landed.join("/") != path {
            return Err(refused("a link on the way leads elsewhere".to_owned()));
        }
        Ok(())
    }
}

/// Whether `a` is `b` or lies inside it.
fn within(a: &str, b: &str) -> bool {
    a == b || a.strip_prefix(b).is_some_and(|rest| rest.starts_with('/'))
}

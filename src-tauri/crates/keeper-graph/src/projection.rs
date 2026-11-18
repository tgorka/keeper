//! A domain's query projection on disk (contract §5; AD-11/AD-12): the
//! verified export of its accepted prefix and the immutable generations
//! built from it, under one root no other process or [`Store`] uses.
//!
//! **Export.** [`Exporter::extend`] reads the domain's history through
//! [`GraphSource`] in bounded batches, takes events 1, 2, … while each is
//! whole (exactly `op_count` operations), checks every one (ordinals,
//! delta digest, every before-image against the digest of the image the
//! events before it replayed to) and appends its after-image frame to the
//! export file. It keeps only a digest per entity, never the images. A
//! verified prefix is immutable and extends from its last position; a
//! present event that does not verify discards the export (the next one
//! starts from genesis). Current `kg_nodes`/`kg_edges` rows are never read.
//!
//! **Generations.** A build writes `<ulid>.staging`, which the builder
//! worker fills; publication checks the file's header and sidecars, syncs
//! it, renames it to `<ulid>.db`, syncs the directory, then writes the
//! selection `current` (`current.tmp`, synced, renamed, directory synced):
//! the generation's name, its manifest and its last event. Recovery at open
//! keeps exactly the generation `current` names (when it exists and its
//! manifest is of this build's versions) and removes every other owned
//! file: a crash before the builder committed, before the rename, before
//! or during the selection's write leaves the previous selection (or none),
//! never a pointer to incomplete output. Purge removes the whole projection.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::prefix::{corrupt, group, PrefixError, BATCH_EVENTS};
use super::protocol::value::canonical;
use super::protocol::vocabulary::EntityType;
use super::protocol::wire::Authority;
use super::protocol::worker::{
    chain, EventFrame, Manifest, ENGINE, GENESIS, MAX_ENTITIES, MAX_EVENTS, MAX_EXPORT_BYTES,
    PROJECTION_VERSION,
};
use super::protocol::SCHEMA_VERSION;
use super::query::QueryError;
use super::source::GraphSource;

const LOCK: &str = ".projection.lock";
const CURRENT: &str = "current";
const CURRENT_TMP: &str = "current.tmp";
const EXPORT: &str = "prefix.export";

/// The last event of a generation's prefix, for the freshness check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Last {
    pub(crate) graph_seq: u64,
    pub(crate) event_id: String,
    pub(crate) delta_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    generation: String,
    manifest: Manifest,
    last: Option<Last>,
}

/// A published generation; holding it pins its file.
#[derive(Clone, Debug)]
pub(crate) struct Generation {
    file: Arc<PathBuf>,
    pub(crate) manifest: Manifest,
    pub(crate) last: Option<Last>,
}

impl Generation {
    pub(crate) fn path(&self) -> &Path {
        &self.file
    }
}

/// Published generations a projection root keeps alive at once (the
/// selection included). A build is refused, never queued, while this many
/// are referenced, so the disk one root holds is at most
/// (`MAX_RETAINED_GENERATIONS` + one staging) generations.
pub const MAX_RETAINED_GENERATIONS: usize = 2;

/// The selection and every published generation still referenced, each
/// registered once, under one lock with the reclaim that reads them.
#[derive(Default)]
struct State {
    current: Option<Generation>,
    registry: Vec<Weak<PathBuf>>,
}

impl State {
    fn live(&mut self) -> BTreeSet<PathBuf> {
        self.registry.retain(|g| g.strong_count() > 0);
        self.registry
            .iter()
            .filter_map(Weak::upgrade)
            .map(|p| (*p).clone())
            .collect()
    }
}

/// One domain's projection root.
pub(crate) struct Store {
    root: PathBuf,
    _lock: std::fs::File,
    state: Mutex<State>,
}

fn storage(what: &'static str) -> impl Fn(std::io::Error) -> QueryError {
    move |_| QueryError::Storage(what)
}

fn owned(name: &str) -> bool {
    let base = ["-journal", "-wal", "-shm"]
        .iter()
        .find_map(|s| name.strip_suffix(s))
        .unwrap_or(name);
    name == CURRENT_TMP
        || name == EXPORT
        || base
            .strip_suffix(".db")
            .or_else(|| base.strip_suffix(".staging"))
            .is_some_and(|id| ulid::Ulid::from_string(id).is_ok())
}

fn sync_dir(root: &Path) -> Result<(), QueryError> {
    std::fs::File::open(root)
        .and_then(|d| d.sync_all())
        .map_err(storage("sync directory"))
}

impl Store {
    /// Open (creating) `root`, exclusively, and recover it.
    pub(crate) fn open(root: &Path) -> Result<Self, QueryError> {
        std::fs::create_dir_all(root).map_err(storage("projection root"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
                .map_err(storage("projection root"))?;
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join(LOCK))
            .map_err(storage("projection lock"))?;
        lock.try_lock()
            .map_err(|_| QueryError::Unavailable("projection.locked"))?;
        let store = Self {
            root: root.to_owned(),
            _lock: lock,
            state: Mutex::new(State::default()),
        };
        store.recover()?;
        Ok(store)
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, State>, QueryError> {
        self.state
            .lock()
            .map_err(|_| QueryError::Storage("projection state"))
    }

    fn selection(&self) -> Option<Generation> {
        let text = std::fs::read(self.root.join(CURRENT)).ok()?;
        let selection: Selection = serde_json::from_slice(&text).ok()?;
        if ulid::Ulid::from_string(&selection.generation).is_err()
            || selection.manifest.validate().is_err()
            || selection.last.as_ref().map(|l| l.graph_seq)
                != Some(selection.manifest.accepted_head).filter(|n| *n > 0)
        {
            return None;
        }
        let file = self.root.join(format!("{}.db", selection.generation));
        header_ok(&file).ok()?;
        Some(Generation {
            file: Arc::new(file),
            manifest: selection.manifest,
            last: selection.last,
        })
    }

    /// Keep what `current` names, when it is complete and of this build's
    /// versions; remove every other owned file.
    fn recover(&self) -> Result<(), QueryError> {
        let selected = self.selection();
        if selected.is_none() {
            let _ = std::fs::remove_file(self.root.join(CURRENT));
        }
        let mut state = self.state()?;
        if let Some(g) = &selected {
            state.registry.push(Arc::downgrade(&g.file));
        }
        state.current = selected;
        self.reclaim(&mut state, true)
    }

    /// Remove every owned file nothing references (not a live generation,
    /// not the selection, and not the export unless `export`), under the
    /// state lock, so no handle is taken between the decision and the
    /// removal.
    fn reclaim(&self, state: &mut State, export: bool) -> Result<(), QueryError> {
        let mut keep = state.live();
        if let Some(g) = &state.current {
            keep.insert(g.path().to_owned());
        }
        for entry in std::fs::read_dir(&self.root).map_err(storage("projection root"))? {
            let path = entry.map_err(storage("projection root"))?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !owned(name) || (name == EXPORT && !export) || keep.contains(&path) {
                continue;
            }
            std::fs::remove_file(&path).map_err(storage("reclaim"))?;
        }
        Ok(())
    }

    /// The selected generation; holding the handle pins its file. Taking
    /// one registers nothing: each generation is registered once, when it
    /// is published or recovered.
    pub(crate) fn current(&self) -> Option<Generation> {
        self.state().ok()?.current.clone()
    }

    /// How many registry entries the store holds (live or not yet pruned).
    #[cfg(test)]
    pub(crate) fn registry_len(&self) -> usize {
        self.state.lock().map(|s| s.registry.len()).unwrap_or(0)
    }

    /// Unselect `generation` when a worker refused it as unreadable: the
    /// next refresh rebuilds from the accepted history. Its file goes once
    /// nothing references it.
    pub(crate) fn discard(&self, generation: &Generation) -> Result<(), QueryError> {
        let mut state = self.state()?;
        if state
            .current
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(&c.file, &generation.file))
        {
            state.current = None;
            std::fs::remove_file(self.root.join(CURRENT))
                .or_else(|e| match e.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(e),
                })
                .map_err(storage("selection"))?;
            sync_dir(&self.root)?;
        }
        self.reclaim(&mut state, false)
    }

    pub(crate) fn export_path(&self) -> PathBuf {
        self.root.join(EXPORT)
    }

    /// A fresh staging file for one build, after reclaiming what nothing
    /// references; refused (`query_limit`) while
    /// [`MAX_RETAINED_GENERATIONS`] generations are alive. Removed on drop
    /// unless published.
    pub(crate) fn stage(&self) -> Result<Staged, QueryError> {
        let mut state = self.state()?;
        self.reclaim(&mut state, false)?;
        let mut alive = state.live();
        if let Some(g) = &state.current {
            alive.insert(g.path().to_owned());
        }
        if alive.len() >= MAX_RETAINED_GENERATIONS {
            return Err(QueryError::Limit);
        }
        let id = ulid::Ulid::new().to_string();
        let path = self.root.join(format!("{id}.staging"));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(storage("staging"))?;
        Ok(Staged {
            id,
            path: Some(path),
        })
    }

    /// Publish `staged`, built for `manifest`, and select it.
    pub(crate) fn publish(
        &self,
        mut staged: Staged,
        manifest: Manifest,
        last: Option<Last>,
    ) -> Result<Generation, QueryError> {
        let staging = staged.path.clone().ok_or(QueryError::Storage("staging"))?;
        header_ok(&staging)?;
        std::fs::File::open(&staging)
            .and_then(|f| f.sync_all())
            .map_err(storage("staging sync"))?;
        let published = self.root.join(format!("{}.db", staged.id));
        std::fs::rename(&staging, &published).map_err(storage("publish"))?;
        staged.path = Some(published.clone());
        sync_dir(&self.root)?;
        let selection = Selection {
            generation: staged.id.clone(),
            manifest: manifest.clone(),
            last: last.clone(),
        };
        let tmp = self.root.join(CURRENT_TMP);
        let mut file = std::fs::File::create(&tmp).map_err(storage("selection"))?;
        file.write_all(
            &serde_json::to_vec(&selection).map_err(|_| QueryError::Storage("selection"))?,
        )
        .and_then(|()| file.sync_all())
        .map_err(storage("selection"))?;
        std::fs::rename(&tmp, self.root.join(CURRENT)).map_err(storage("selection"))?;
        sync_dir(&self.root)?;
        staged.path = None;
        let generation = Generation {
            file: Arc::new(published),
            manifest,
            last,
        };
        let mut state = self.state()?;
        state.registry.push(Arc::downgrade(&generation.file));
        state.current = Some(generation.clone());
        self.reclaim(&mut state, false)?;
        Ok(generation)
    }

    /// Remove the whole projection: the selection, every generation and
    /// the export. Callers cancel and reap their workers first.
    pub(crate) fn purge(&self) -> Result<(), QueryError> {
        let mut state = self.state()?;
        state.current = None;
        let _ = std::fs::remove_file(self.root.join(CURRENT));
        for entry in std::fs::read_dir(&self.root).map_err(storage("projection root"))? {
            let path = entry.map_err(storage("projection root"))?.path();
            if path.file_name().and_then(|n| n.to_str()).is_some_and(owned) {
                std::fs::remove_file(&path).map_err(storage("purge"))?;
            }
        }
        sync_dir(&self.root)
    }
}

/// A staging file that is removed unless published.
pub(crate) struct Staged {
    id: String,
    path: Option<PathBuf>,
}

impl Staged {
    pub(crate) fn path(&self) -> Result<&Path, QueryError> {
        self.path.as_deref().ok_or(QueryError::Storage("staging"))
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A complete SQLite file in rollback-journal mode with no sidecar.
fn header_ok(path: &Path) -> Result<(), QueryError> {
    let mut header = [0u8; 20];
    std::io::Read::read_exact(
        &mut std::fs::File::open(path).map_err(storage("generation"))?,
        &mut header,
    )
    .map_err(storage("generation"))?;
    if &header[..16] != b"SQLite format 3\0"
        || header[18..20] != [1, 1]
        || ["-journal", "-wal", "-shm"]
            .iter()
            .any(|suffix| PathBuf::from(format!("{}{suffix}", path.display())).exists())
    {
        return Err(QueryError::Unavailable("generation.incomplete"));
    }
    Ok(())
}

/// A compact key for an entity type (the protocol's enum is not `Hash`).
fn tag(kind: EntityType) -> u8 {
    match kind {
        EntityType::Node => 0,
        EntityType::Edge => 1,
        EntityType::Tag => 2,
        EntityType::NodeTag => 3,
        EntityType::Content => 4,
        EntityType::Review => 5,
    }
}

fn digest_of(image: &Value) -> Result<[u8; 32], ()> {
    let bytes = canonical(image).map_err(|_| ())?;
    Ok(Sha256::digest(bytes).into())
}

/// A domain's verified, exported prefix.
pub(crate) struct Exporter {
    domain: String,
    path: PathBuf,
    file: std::fs::File,
    pub(crate) n: u64,
    pub(crate) digest: String,
    pub(crate) last: Option<Last>,
    pub(crate) bytes: u64,
    images: HashMap<(u8, String), [u8; 32]>,
}

/// How far [`Exporter::extend`] got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Extended {
    /// The head the source held when the read started.
    pub(crate) head: u64,
    /// Whether the prefix reaches that head.
    pub(crate) complete: bool,
}

impl Exporter {
    /// An empty prefix over a new export file at `path`.
    pub(crate) fn genesis(domain: &str, path: &Path) -> Result<Self, QueryError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map_err(storage("export"))?;
        Ok(Self {
            domain: domain.to_owned(),
            path: path.to_owned(),
            file,
            n: 0,
            digest: GENESIS.to_owned(),
            last: None,
            bytes: 0,
            images: HashMap::new(),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Verify and export the events `source` holds after the prefix.
    pub(crate) fn extend(&mut self, source: &dyn GraphSource) -> Result<Extended, PrefixError> {
        let first = source.prefix_batch(self.n, BATCH_EVENTS)?;
        let head = first.head.unwrap_or(0);
        if head < self.n {
            return Err(corrupt(head + 1, "head behind the verified prefix"));
        }
        let mut batch = Some(first);
        while self.n < head {
            if head > MAX_EVENTS {
                return Err(corrupt(
                    self.n + 1,
                    "prefix longer than one generation holds",
                ));
            }
            let current = match batch.take() {
                Some(batch) => batch,
                None => source.prefix_batch(self.n, BATCH_EVENTS)?,
            };
            let upto = self.n.saturating_add(BATCH_EVENTS as u64).min(head);
            let mut events = group(self.n, &current.events, &current.ops)?;
            while self.n < upto {
                let at = self.n + 1;
                let Some(Some(event)) = events.remove(&at) else {
                    return Ok(Extended {
                        head,
                        complete: false,
                    });
                };
                event.check().map_err(|_| corrupt(at, "digest or count"))?;
                for op in &event.ops {
                    let key = (tag(op.entity_type), op.entity_id.clone());
                    let before = op
                        .before_value
                        .as_ref()
                        .map(digest_of)
                        .transpose()
                        .map_err(|()| corrupt(at, "before-image"))?;
                    if self.images.get(&key).copied() != before {
                        return Err(corrupt(at, "before-image"));
                    }
                    match &op.after_value {
                        None => {
                            self.images.remove(&key);
                        }
                        Some(after) => {
                            let d = digest_of(after).map_err(|()| corrupt(at, "after-image"))?;
                            self.images.insert(key, d);
                        }
                    }
                }
                if self.images.len() as u64 > MAX_ENTITIES {
                    return Err(corrupt(at, "more entities than one generation holds"));
                }
                let frame =
                    EventFrame::new(at, &event.event.id, &event.event.delta_digest, &event.ops);
                frame.check(at).map_err(|_| corrupt(at, "frame"))?;
                let line = frame.encode().map_err(|_| corrupt(at, "frame size"))?;
                self.bytes += line.len() as u64;
                if self.bytes > MAX_EXPORT_BYTES {
                    return Err(corrupt(at, "export larger than one generation holds"));
                }
                self.file
                    .write_all(&line)
                    .map_err(|_| corrupt(at, "export write"))?;
                self.digest = chain(&self.digest, at, &event.event.delta_digest);
                self.last = Some(Last {
                    graph_seq: at,
                    event_id: event.event.id.clone(),
                    delta_digest: event.event.delta_digest.clone(),
                });
                self.n = at;
            }
        }
        self.file
            .flush()
            .map_err(|_| corrupt(self.n, "export write"))?;
        Ok(Extended {
            head,
            complete: true,
        })
    }

    /// The manifest of a generation of this prefix under `authority` at
    /// Syncular `checkpoint`.
    pub(crate) fn manifest(&self, authority: &Authority, checkpoint: i64) -> Manifest {
        Manifest {
            domain: self.domain.clone(),
            authority_epoch: authority.epoch.clone(),
            authority_digest: authority.digest.clone(),
            accepted_head: self.n,
            prefix_digest: self.digest.clone(),
            sync_checkpoint: checkpoint.max(0).to_string(),
            contract_schema: SCHEMA_VERSION,
            projection_version: PROJECTION_VERSION,
            engine: ENGINE.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ulid_named_artifacts_and_the_projection_files_are_owned() {
        for (name, own) in [
            ("01J0000000000000000000000A.db", true),
            ("01J0000000000000000000000A.staging", true),
            ("01J0000000000000000000000A.db-journal", true),
            ("current.tmp", true),
            ("prefix.export", true),
            ("current", false),
            (".projection.lock", false),
            ("canonical.db", false),
            ("01J0000000000000000000000A.sqlite", false),
        ] {
            assert_eq!(owned(name), own, "{name}");
        }
    }

    #[test]
    fn a_second_store_on_one_root_is_refused() {
        let root = tempfile::tempdir().expect("root");
        let _first = Store::open(root.path()).expect("first");
        assert!(matches!(
            Store::open(root.path()),
            Err(QueryError::Unavailable("projection.locked"))
        ));
    }

    fn manifest(n: u64) -> Manifest {
        Manifest {
            domain: "work".into(),
            authority_epoch: "1".into(),
            authority_digest: "ab".repeat(32),
            accepted_head: n,
            prefix_digest: GENESIS.into(),
            sync_checkpoint: "0".into(),
            contract_schema: SCHEMA_VERSION,
            projection_version: PROJECTION_VERSION,
            engine: ENGINE.into(),
        }
    }

    /// A staged file with a complete rollback-journal SQLite header, as a
    /// builder leaves it (the Store checks no more than that).
    fn built(store: &Store) -> Result<Staged, QueryError> {
        let staged = store.stage()?;
        let mut bytes = b"SQLite format 3\0".to_vec();
        bytes.extend_from_slice(&[16, 0, 1, 1]);
        bytes.resize(4096, 0);
        std::fs::write(staged.path()?, bytes).expect("header");
        Ok(staged)
    }

    /// B3: at most two generations are alive at once; a build is refused,
    /// not queued, while two are referenced, and admitted again once one
    /// is released. No referenced file is deleted to make room.
    #[test]
    fn at_most_two_generations_are_retained() {
        let root = tempfile::tempdir().expect("root");
        let store = Store::open(root.path()).expect("store");
        let first = store
            .publish(built(&store).expect("stage 1"), manifest(0), None)
            .expect("g1");
        let held = store.current().expect("pinned by a query");
        let second = store
            .publish(built(&store).expect("stage 2"), manifest(0), None)
            .expect("g2");
        assert!(matches!(built(&store).map(drop), Err(QueryError::Limit)));
        assert!(first.path().exists() && second.path().exists());
        drop((first, held));
        let third = store
            .publish(built(&store).expect("released"), manifest(0), None)
            .expect("g3");
        assert!(second.path().exists(), "a referenced generation is kept");
        assert!(third.path().exists());
    }

    /// B4: taking the selected generation registers nothing; a million
    /// reads leave the registry at its one entry.
    #[test]
    fn reads_do_not_grow_the_registry() {
        let root = tempfile::tempdir().expect("root");
        let store = Store::open(root.path()).expect("store");
        let _g = store
            .publish(built(&store).expect("stage"), manifest(0), None)
            .expect("published");
        for _ in 0..1_000_000 {
            drop(store.current());
        }
        assert_eq!(store.registry_len(), 1);
    }

    /// B2: a discarded selection is no longer served or recovered, and its
    /// file goes once nothing holds it.
    #[test]
    fn a_discarded_generation_is_unselected() {
        let root = tempfile::tempdir().expect("root");
        let store = Store::open(root.path()).expect("store");
        let g = store
            .publish(built(&store).expect("stage"), manifest(0), None)
            .expect("published");
        store.discard(&g).expect("discard");
        assert!(store.current().is_none());
        assert!(!root.path().join("current").exists());
        let path = g.path().to_owned();
        drop(g);
        drop(store.stage().expect("reclaims"));
        assert!(!path.exists());
    }
}

//! Safe single-use graph worker supervisor. No engine is linked into this crate.
use keeper_graph_protocol::wire::{Manifest, QueryRequest, Response, MAX_EXPORT_BYTES};
use keeper_graph_protocol::{admit, MAX_RESULT_BYTES, PROFILE};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{watch, Semaphore};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("query_unsupported")]
    Unsupported,
    #[error("query_limit")]
    Limit,
    #[error("query_cancelled")]
    Cancelled,
    #[error("projection_unavailable")]
    Unavailable,
    #[error("storage_unavailable")]
    Storage(#[from] std::io::Error),
}
type Result<T, E = Error> = std::result::Result<T, E>;
#[derive(Clone)]
pub struct Cancellation(watch::Sender<bool>);
impl Default for Cancellation {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}
impl Cancellation {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
}
/// Concurrent builder workers per supervisor. A second build is refused, never queued.
pub const MAX_BUILDERS: usize = 1;
/// Published generations callers may keep referenced. A build is refused while this
/// many are alive; unreferenced owned files are reclaimed before each build reserves
/// its staging file. Disk held by one supervisor root is therefore at most
/// (MAX_RETAINED_GENERATIONS + MAX_BUILDERS + 1 unreclaimable failed leftover)
/// x MAX_DATABASE_BYTES; a leftover that cannot be removed refuses the next build.
pub const MAX_RETAINED_GENERATIONS: usize = 2;
#[derive(Clone)]
pub struct Generation {
    file: Arc<PathBuf>,
    /// Keeps the root's exclusive lock and live-generation registry alive while any
    /// generation is held, so no new supervisor can take over the root and reclaim it.
    owner: Arc<Owner>,
    pub manifest: Manifest,
    /// Builder worker charged-allocation high-water mark and residual at exit, and its
    /// peak resident set. Measurements only; the budget is enforced inside the worker.
    pub build_peak_allocated_bytes: usize,
    pub build_final_allocated_bytes: usize,
    pub build_peak_rss_bytes: u64,
}
impl Generation {
    fn path(&self) -> &Path {
        &self.file
    }
}
/// Exclusive owner of one supervisor root: the advisory lock and the registry of live
/// generations. Shared by the supervisor and every generation it published; the lock is
/// released only when all of them are gone. Holds generations weakly (no cycle).
struct Owner {
    root: PathBuf,
    generations: Mutex<Vec<Weak<PathBuf>>>,
    _lock: std::fs::File,
}
#[derive(Clone)]
pub struct Supervisor(Arc<Inner>);
struct Inner {
    worker: PathBuf,
    owner: Arc<Owner>,
    global: Arc<Semaphore>,
    queued: Semaphore,
    builds: Semaphore,
    identities: Mutex<Identities>,
    last_reaped: AtomicU32,
    live: Mutex<BTreeSet<u32>>,
    peak: AtomicUsize,
}
#[derive(Default)]
struct Identities {
    agents: BTreeMap<String, Arc<Semaphore>>,
    contexts: BTreeMap<(String, String, String), Arc<Semaphore>>,
}
#[derive(Clone)]
pub struct Session {
    supervisor: Supervisor,
    domain: String,
    agent: Arc<Semaphore>,
    context: Arc<Semaphore>,
}

pub fn available() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}
impl Supervisor {
    pub fn new(worker: &Path, root: &Path, headless: bool) -> Result<Self> {
        if !available() {
            return Err(Error::Unsupported);
        }
        std::fs::create_dir_all(root)?;
        let worker = worker.canonicalize()?;
        let root = root.canonicalize()?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join(".supervisor.lock"))?;
        // Refused while another supervisor, or any generation it published, is alive.
        lock.try_lock().map_err(|_| Error::Unavailable)?;
        Ok(Self(Arc::new(Inner {
            worker,
            owner: Arc::new(Owner {
                root,
                generations: Mutex::new(Vec::new()),
                _lock: lock,
            }),
            global: Arc::new(Semaphore::new(if headless { 2 } else { 4 })),
            queued: Semaphore::new(16),
            builds: Semaphore::new(MAX_BUILDERS),
            identities: Mutex::new(Identities::default()),
            last_reaped: AtomicU32::new(0),
            live: Mutex::new(BTreeSet::new()),
            peak: AtomicUsize::new(0),
        })))
    }
    /// PIDs of currently running workers; proof drivers use them for crash injection.
    pub fn live_workers(&self) -> Vec<u32> {
        self.0
            .live
            .lock()
            .map(|l| l.iter().copied().collect())
            .unwrap_or_default()
    }
    /// Owned generation/staging artifacts currently on disk (bounded-storage proof).
    pub fn artifacts(&self) -> Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&self.0.owner.root)? {
            let path = entry?.path();
            if owned_artifact(&path) {
                found.push(path);
            }
        }
        found.sort();
        Ok(found)
    }
    /// PID of the most recently waited-for worker; proof drivers verify its absence.
    pub fn last_reaped(&self) -> Option<u32> {
        Some(self.0.last_reaped.load(Ordering::SeqCst)).filter(|pid| *pid != 0)
    }
    /// Highest number of simultaneously live worker processes observed.
    pub fn peak_workers(&self) -> usize {
        self.0.peak.load(Ordering::SeqCst)
    }
    pub fn session(&self, agent: &str, context: &str, domain: &str) -> Result<Session> {
        if [agent, context, domain]
            .iter()
            .any(|s| s.is_empty() || s.len() > 128)
        {
            return Err(Error::Unsupported);
        }
        let mut identities = self.0.identities.lock().map_err(|_| Error::Unavailable)?;
        let key = (agent.to_owned(), context.to_owned(), domain.to_owned());
        if !identities.contexts.contains_key(&key) && identities.contexts.len() >= 64 {
            return Err(Error::Limit);
        }
        let agent = identities
            .agents
            .entry(agent.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(2)))
            .clone();
        let context = identities
            .contexts
            .entry(key)
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .clone();
        Ok(Session {
            supervisor: self.clone(),
            domain: domain.to_owned(),
            agent,
            context,
        })
    }
    /// Host-only accepted export input, never exposed as a model/tool path argument.
    pub async fn build_export(
        &self,
        export: &Path,
        expected: &Manifest,
        cancel: &Cancellation,
    ) -> Result<Generation> {
        expected.validate().map_err(|_| Error::Unsupported)?;
        // P6c first: a refused launch opens no input, reclaims no artifact and creates
        // no staging. `run` repeats the check immediately before the spawn.
        #[cfg(target_os = "linux")]
        launch_prerequisites()?;
        // Admission and storage reservation happen before any file is created.
        let _builder = self.0.builds.try_acquire().map_err(|_| Error::Limit)?;
        if *cancel.0.borrow() {
            return Err(Error::Cancelled);
        }
        let input = tokio::fs::File::open(export).await?;
        if input.metadata().await?.len() > MAX_EXPORT_BYTES {
            return Err(Error::Limit);
        }
        self.reserve_build()?;
        let id = ulid::Ulid::new().to_string();
        let mut artifact = Artifact(Some(self.0.owner.root.join(format!("{id}.staging"))));
        let staging = artifact.path()?.to_owned();
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)?;
        let response = self
            .run("build", &staging, input, Duration::from_secs(180), cancel)
            .await
            .map_err(|e| match e {
                Error::Cancelled => Error::Cancelled,
                Error::Limit => Error::Limit,
                _ => Error::Unavailable,
            })?;
        if &response.manifest != expected || !response.rows.is_empty() {
            return Err(Error::Unavailable);
        }
        let published = Arc::new(self.0.owner.root.join(format!("{id}.db")));
        publish(&Filesystem, &mut artifact, &published, &self.0.owner.root)?;
        self.0
            .owner
            .generations
            .lock()
            .map_err(|_| Error::Unavailable)?
            .push(Arc::downgrade(&published));
        Ok(Generation {
            file: published,
            owner: self.0.owner.clone(),
            manifest: response.manifest,
            build_peak_allocated_bytes: response.peak_allocated_bytes,
            build_final_allocated_bytes: response.allocated_bytes,
            build_peak_rss_bytes: response.peak_rss_bytes,
        })
    }
    /// Refuses while MAX_RETAINED_GENERATIONS are referenced, then removes every owned
    /// artifact no live Generation references (earlier failures, dropped generations).
    fn reserve_build(&self) -> Result<()> {
        let mut generations = self
            .0
            .owner
            .generations
            .lock()
            .map_err(|_| Error::Unavailable)?;
        generations.retain(|g| g.strong_count() > 0);
        if generations.len() >= MAX_RETAINED_GENERATIONS {
            return Err(Error::Limit);
        }
        let referenced: BTreeSet<PathBuf> = generations
            .iter()
            .filter_map(Weak::upgrade)
            .map(|p| (*p).clone())
            .collect();
        for path in self.artifacts()? {
            let base = PathBuf::from(
                path.to_string_lossy()
                    .trim_end_matches("-journal")
                    .trim_end_matches("-wal")
                    .trim_end_matches("-shm"),
            );
            if !referenced.contains(&base) {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
    async fn run<R: tokio::io::AsyncRead + Unpin>(
        &self,
        role: &str,
        path: &Path,
        input: R,
        deadline: Duration,
        cancel: &Cancellation,
    ) -> Result<Response> {
        // The deadline covers launcher/exec, setup, transfer, native work and exit.
        let deadline = tokio::time::Instant::now() + deadline;
        let mut cancellation = cancel.0.subscribe();
        if *cancellation.borrow() {
            return Err(Error::Cancelled);
        }
        // P6c: the launch prerequisites hold before any child exists.
        #[cfg(target_os = "linux")]
        launch_prerequisites()?;
        let mut command = worker_command(&self.0.worker, path, role)?;
        command.env_clear().env("GQL_STMT_CACHE", "0");
        #[cfg(target_os = "linux")]
        command.env(
            "GLIBC_TUNABLES",
            keeper_graph_protocol::wire::LINUX_WORKER_GLIBC_TUNABLES,
        );
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let Some(pid) = child.id() else {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(Error::Unavailable);
        };
        {
            let mut live = self.0.live.lock().map_err(|_| Error::Unavailable)?;
            live.insert(pid);
            self.0.peak.fetch_max(live.len(), Ordering::SeqCst);
        }
        let result = self
            .exchange(&mut child, pid, input, deadline, &mut cancellation)
            .await;
        if result.is_err() {
            let _ = child.start_kill();
        }
        // Every path reaps the child before reporting, including success.
        let waited = child.wait().await;
        if let Ok(mut live) = self.0.live.lock() {
            live.remove(&pid);
        }
        self.0.last_reaped.store(pid, Ordering::SeqCst);
        waited?;
        result
    }
    async fn exchange<R: tokio::io::AsyncRead + Unpin>(
        &self,
        child: &mut tokio::process::Child,
        pid: u32,
        input: R,
        deadline: tokio::time::Instant,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<Response> {
        let mut stdin = child.stdin.take().ok_or(Error::Unavailable)?;
        let stdout = child.stdout.take().ok_or(Error::Unavailable)?;
        let execute = async {
            // A worker that exits early closes its stdin; its exit status, not the
            // broken pipe, decides the outcome.
            let write = async {
                let _ = tokio::io::copy(&mut input.take(MAX_EXPORT_BYTES + 1), &mut stdin).await;
                let _ = stdin.shutdown().await;
                drop(stdin);
            };
            let read = async {
                let mut out = Vec::new();
                stdout
                    .take(MAX_RESULT_BYTES as u64 + 1)
                    .read_to_end(&mut out)
                    .await
                    .map(|_| out)
            };
            let ((), bytes) = tokio::join!(write, read);
            let bytes = bytes.map_err(|_| Error::Unavailable)?;
            if bytes.len() > MAX_RESULT_BYTES {
                return Err(Error::Unavailable);
            }
            let status = child.wait().await?;
            if !status.success() {
                return Err(if matches!(status.code(), Some(72 | 75)) {
                    Error::Limit
                } else {
                    Error::Unavailable
                });
            }
            let response: Response =
                serde_json::from_slice(&bytes).map_err(|_| Error::Unavailable)?;
            if response.pid != pid {
                return Err(Error::Unavailable);
            }
            Ok(response)
        };
        tokio::select! {
            result = tokio::time::timeout_at(deadline, execute) => result.unwrap_or(Err(Error::Limit)),
            _ = cancellation.changed() => Err(Error::Cancelled),
        }
    }
}
/// Removes its file on drop unless disarmed; owns staging, then the renamed file
/// until the directory entry is durable.
struct Artifact(Option<PathBuf>);
impl Artifact {
    fn path(&self) -> Result<&Path> {
        self.0.as_deref().ok_or(Error::Unavailable)
    }
}
impl Drop for Artifact {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}
fn owned_artifact(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = ["-journal", "-wal", "-shm"]
        .iter()
        .find_map(|s| name.strip_suffix(s))
        .unwrap_or(name);
    name.strip_suffix(".db")
        .or_else(|| name.strip_suffix(".staging"))
        .is_some_and(|id| ulid::Ulid::from_string(id).is_ok())
}
trait PublishOps {
    fn sync_file(&self, path: &Path) -> std::io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn sync_dir(&self, path: &Path) -> std::io::Result<()>;
}
struct Filesystem;
impl PublishOps for Filesystem {
    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
    fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }
}
/// Validates the completed staging file, syncs, renames and syncs the directory.
/// Any failure leaves neither staging nor a new generation behind (best effort if
/// the filesystem refuses removal; reserve_build then reclaims or refuses).
fn publish(
    ops: &impl PublishOps,
    artifact: &mut Artifact,
    published: &Path,
    root: &Path,
) -> Result<()> {
    let staging = artifact.path()?.to_owned();
    // N2: header bytes and absent sidefiles, not a journal_mode PRAGMA, prove layout.
    let mut header = [0u8; 20];
    std::io::Read::read_exact(&mut std::fs::File::open(&staging)?, &mut header)?;
    if &header[..16] != b"SQLite format 3\0"
        || header[18..20] != [1, 1]
        || ["-journal", "-wal", "-shm"]
            .iter()
            .any(|suffix| PathBuf::from(format!("{}{suffix}", staging.display())).exists())
    {
        return Err(Error::Unavailable);
    }
    ops.sync_file(&staging)?;
    ops.rename(&staging, published)?;
    artifact.0 = Some(published.to_owned());
    ops.sync_dir(root)?;
    artifact.0 = None;
    Ok(())
}
impl Session {
    pub async fn query(
        &self,
        generation: &Generation,
        source: &str,
        parameters: &[u8],
        cancel: &Cancellation,
    ) -> Result<Response> {
        let admitted = admit(PROFILE, source, parameters).map_err(|e| match e {
            keeper_graph_protocol::Refusal::QueryLimit => Error::Limit,
            _ => Error::Unsupported,
        })?;
        // A refusal here is an encoder/bound defect, never a caller-visible limit.
        let bytes = QueryRequest::encode(&admitted, source).map_err(|_| Error::Unavailable)?;
        let response = self.request(generation, "query", bytes, cancel).await?;
        if response.rows.len() > admitted.limit() {
            return Err(Error::Unavailable);
        }
        Ok(response)
    }
    pub async fn inspect(
        &self,
        generation: &Generation,
        id: &str,
        include_archived: bool,
        cancel: &Cancellation,
    ) -> Result<Response> {
        if !keeper_graph_protocol::wire::ulid(id) {
            return Err(Error::Unsupported);
        }
        let request = keeper_graph_protocol::wire::InspectRequest {
            id: id.to_owned(),
            include_archived,
        };
        let mut bytes = serde_json::to_vec(&request).map_err(|_| Error::Unsupported)?;
        bytes.push(b'\n');
        let response = self.request(generation, "inspect", bytes, cancel).await?;
        if response.rows.len() > 1 {
            return Err(Error::Unavailable);
        }
        Ok(response)
    }
    async fn request(
        &self,
        generation: &Generation,
        role: &str,
        bytes: Vec<u8>,
        cancel: &Cancellation,
    ) -> Result<Response> {
        // A generation is only served by the supervisor that owns its root.
        if generation.manifest.domain != self.domain
            || !Arc::ptr_eq(&generation.owner, &self.supervisor.0.owner)
        {
            return Err(Error::Unavailable);
        }
        let _queue = self
            .supervisor
            .0
            .queued
            .try_acquire()
            .map_err(|_| Error::Limit)?;
        let mut cancellation = cancel.0.subscribe();
        if *cancellation.borrow() {
            return Err(Error::Cancelled);
        }
        let admission = async {
            let a = self
                .context
                .acquire()
                .await
                .map_err(|_| Error::Unavailable)?;
            let b = self.agent.acquire().await.map_err(|_| Error::Unavailable)?;
            let c = self
                .supervisor
                .0
                .global
                .acquire()
                .await
                .map_err(|_| Error::Unavailable)?;
            Ok::<_, Error>((a, b, c))
        };
        let _permits = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(3), admission) => result.map_err(|_| Error::Limit)??,
            _ = cancellation.changed() => return Err(Error::Cancelled),
        };
        let response = self
            .supervisor
            .run(
                role,
                generation.path(),
                bytes.as_slice(),
                Duration::from_secs(3),
                cancel,
            )
            .await?;
        if response.manifest != generation.manifest {
            return Err(Error::Unavailable);
        }
        Ok(response)
    }
}
/// Largest inherited soft `RLIMIT_STACK` the worker's startup accounting admits.
#[cfg(any(target_os = "linux", test))]
pub const MAX_INHERITED_STACK: u64 = 8 * 1024 * 1024;
/// P6c (story 110.1, R1 design v7): checked before every Linux worker spawn. The child
/// inherits this process's soft stack limit and runs ld.so with this host's loader
/// configuration; both are startup premises of the worker's allocation accounting.
/// A prerequisite check, not an atomic setter: nothing in the supervisor changes its
/// rlimits or the loader configuration between this check and exec.
#[cfg(target_os = "linux")]
fn launch_prerequisites() -> Result<()> {
    let stack = rustix::process::getrlimit(rustix::process::Resource::Stack).current;
    launch_decision(
        stack,
        std::fs::symlink_metadata("/etc/ld.so.preload").map(|_| ()),
    )
}
/// The decision alone: a finite soft stack limit of at most 8 MiB (`None` is
/// infinite), and `/etc/ld.so.preload` positively absent. Any other lookup outcome,
/// including permission errors and a dangling symlink (which `symlink_metadata`
/// reports as present), refuses.
#[cfg(any(target_os = "linux", test))]
fn launch_decision(stack_soft: Option<u64>, preload: std::io::Result<()>) -> Result<()> {
    match stack_soft {
        Some(limit) if limit <= MAX_INHERITED_STACK => {}
        _ => return Err(Error::Unavailable),
    }
    match preload {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::Unavailable),
    }
}
fn worker_command(worker: &Path, path: &Path, role: &str) -> Result<tokio::process::Command> {
    #[cfg(target_os = "macos")]
    {
        // dyld's libignition opens exactly "/" as an openat root. SQLite also
        // resolves ancestor metadata. Neither rule permits descendant file data.
        const READ: &str = r#"(version 1)(deny default)(allow process-exec (literal (param "WORKER")))(allow file-read* (subpath "/System/Library") (subpath "/usr/lib") (literal (param "WORKER")) (literal (param "DB")) (literal "/dev/fd") (literal "/"))(allow file-read-metadata file-test-existence (path-ancestors (param "DB")) (path-ancestors (param "WORKER")))(allow sysctl-read)(deny process-fork)"#;
        const BUILD: &str = r#"(version 1)(deny default)(allow process-exec (literal (param "WORKER")))(allow file-read* (subpath "/System/Library") (subpath "/usr/lib") (literal (param "WORKER")) (literal (param "DB")) (literal "/dev/fd") (literal "/"))(allow file-read-metadata file-test-existence (path-ancestors (param "DB")) (path-ancestors (param "WORKER")))(allow file-write* (literal (param "DB")))(allow sysctl-read)(deny process-fork)"#;
        let path = path.canonicalize()?;
        let mut command = tokio::process::Command::new("/usr/bin/sandbox-exec");
        command
            .arg("-p")
            .arg(if role == "build" { BUILD } else { READ })
            .arg("-D")
            .arg(format!("WORKER={}", worker.display()))
            .arg("-D")
            .arg(format!("DB={}", path.display()))
            .arg(worker)
            .arg(role)
            .arg(path);
        Ok(command)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut command = tokio::process::Command::new(worker);
        command.arg(role).arg(path);
        Ok(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn launch_is_refused_unless_the_stack_limit_is_finite_and_at_most_8_mib_and_no_preload_exists()
    {
        use std::io::{Error as IoError, ErrorKind};
        let absent = || Err(IoError::from(ErrorKind::NotFound));
        assert!(launch_decision(Some(MAX_INHERITED_STACK), absent()).is_ok());
        assert!(launch_decision(Some(4096), absent()).is_ok());
        for stack in [Some(MAX_INHERITED_STACK + 1), Some(u64::MAX), None] {
            assert!(matches!(
                launch_decision(stack, absent()),
                Err(Error::Unavailable)
            ));
        }
        // Present (including a dangling symlink), unreadable or otherwise unknown.
        for preload in [
            Ok(()),
            Err(IoError::from(ErrorKind::PermissionDenied)),
            Err(IoError::other("lookup failed")),
        ] {
            assert!(matches!(
                launch_decision(Some(MAX_INHERITED_STACK), preload),
                Err(Error::Unavailable)
            ));
        }
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Step {
        SyncFile,
        Rename,
        SyncDir,
    }
    struct Faulty(Option<Step>, Cell<u32>);
    impl PublishOps for Faulty {
        fn sync_file(&self, path: &Path) -> std::io::Result<()> {
            self.fail(Step::SyncFile)?;
            Filesystem.sync_file(path)
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            self.fail(Step::Rename)?;
            Filesystem.rename(from, to)
        }
        fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
            self.fail(Step::SyncDir)?;
            Filesystem.sync_dir(path)
        }
    }
    impl Faulty {
        fn fail(&self, step: Step) -> std::io::Result<()> {
            self.1.set(self.1.get() + 1);
            if self.0 == Some(step) {
                return Err(std::io::Error::other("injected"));
            }
            Ok(())
        }
    }
    fn staged(root: &Path, header_ok: bool) -> (Artifact, PathBuf) {
        let staging = root.join("01J0000000000000000000000A.staging");
        let mut bytes = b"SQLite format 3\0".to_vec();
        bytes.extend_from_slice(&[16, 0, if header_ok { 1 } else { 2 }, 1]);
        bytes.resize(4096, 0);
        std::fs::write(&staging, bytes).expect("staging fixture");
        (
            Artifact(Some(staging)),
            root.join("01J0000000000000000000000A.db"),
        )
    }

    #[test]
    fn every_publication_failure_leaves_no_staging_and_no_generation() {
        for step in [Step::SyncFile, Step::Rename, Step::SyncDir] {
            let root = tempfile::tempdir().expect("root");
            let (mut artifact, published) = staged(root.path(), true);
            let ops = Faulty(Some(step), Cell::new(0));
            assert!(publish(&ops, &mut artifact, &published, root.path()).is_err());
            drop(artifact);
            let left: Vec<_> = std::fs::read_dir(root.path())
                .expect("list")
                .map(|e| e.expect("entry").file_name())
                .collect();
            assert!(left.is_empty(), "{step:?} left {left:?}");
        }
    }

    #[test]
    fn wal_header_or_sidecar_refuses_before_any_publication_step() {
        let root = tempfile::tempdir().expect("root");
        let (mut artifact, published) = staged(root.path(), false);
        let ops = Faulty(None, Cell::new(0));
        assert!(publish(&ops, &mut artifact, &published, root.path()).is_err());
        assert_eq!(ops.1.get(), 0);
        drop(artifact);
        let (mut artifact, published) = staged(root.path(), true);
        let journal = root
            .path()
            .join("01J0000000000000000000000A.staging-journal");
        std::fs::write(&journal, b"hot").expect("journal fixture");
        assert!(publish(&ops, &mut artifact, &published, root.path()).is_err());
        assert_eq!(ops.1.get(), 0);
        drop(artifact);
        assert!(!published.exists());
    }

    #[test]
    fn successful_publication_disarms_cleanup() {
        let root = tempfile::tempdir().expect("root");
        let (mut artifact, published) = staged(root.path(), true);
        publish(&Filesystem, &mut artifact, &published, root.path()).expect("publish");
        drop(artifact);
        assert!(published.exists());
        assert!(!root
            .path()
            .join("01J0000000000000000000000A.staging")
            .exists());
    }

    #[test]
    fn only_ulid_named_artifacts_are_owned() {
        for (name, owned) in [
            ("01J0000000000000000000000A.db", true),
            ("01J0000000000000000000000A.staging", true),
            ("01J0000000000000000000000A.db-journal", true),
            (".supervisor.lock", false),
            ("canonical.db", false),
            ("01J0000000000000000000000A.sqlite", false),
        ] {
            assert_eq!(owned_artifact(Path::new(name)), owned, "{name}");
        }
    }
}

//! Safe single-use graph worker supervisor. No engine is linked into this crate.
use keeper_graph_protocol::wire::{Manifest, QueryRequest, Response, MAX_EXPORT_BYTES};
use keeper_graph_protocol::{admit, MAX_RESULT_BYTES, PROFILE};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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
#[derive(Clone)]
pub struct Generation {
    path: PathBuf,
    pub manifest: Manifest,
}
#[derive(Clone)]
pub struct Supervisor(Arc<Inner>);
struct Inner {
    worker: PathBuf,
    root: PathBuf,
    global: Arc<Semaphore>,
    queued: Semaphore,
    identities: Mutex<Identities>,
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
        Ok(Self(Arc::new(Inner {
            worker,
            root,
            global: Arc::new(Semaphore::new(if headless { 2 } else { 4 })),
            queued: Semaphore::new(16),
            identities: Mutex::new(Identities::default()),
        })))
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
    pub async fn build_export(&self, export: &Path, expected: &Manifest) -> Result<Generation> {
        expected.validate().map_err(|_| Error::Unsupported)?;
        let id = ulid::Ulid::new().to_string();
        let staging = self.0.root.join(format!("{id}.staging"));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)?;
        drop(file);
        let input = tokio::fs::File::open(export).await?;
        if input.metadata().await?.len() > MAX_EXPORT_BYTES {
            std::fs::remove_file(staging)?;
            return Err(Error::Limit);
        }
        let result = self
            .run(
                "build",
                &staging,
                input,
                Duration::from_secs(180),
                &Cancellation::default(),
            )
            .await;
        let response = match result {
            Ok(r) if &r.manifest == expected && r.rows.is_empty() => r,
            _ => {
                let _ = std::fs::remove_file(&staging);
                return Err(Error::Unavailable);
            }
        };
        // N2: header bytes and absent sidefiles, not a journal_mode PRAGMA, prove layout.
        let mut file = std::fs::File::open(&staging)?;
        let mut header = [0u8; 20];
        std::io::Read::read_exact(&mut file, &mut header)?;
        if &header[..16] != b"SQLite format 3\0"
            || header[18..20] != [1, 1]
            || ["-journal", "-wal", "-shm"]
                .iter()
                .any(|suffix| PathBuf::from(format!("{}{suffix}", staging.display())).exists())
        {
            let _ = std::fs::remove_file(&staging);
            return Err(Error::Unavailable);
        }
        file.sync_all()?;
        let published = self.0.root.join(format!("{id}.db"));
        std::fs::rename(&staging, &published)?;
        std::fs::File::open(&self.0.root)?.sync_all()?;
        Ok(Generation {
            path: published,
            manifest: response.manifest,
        })
    }
    async fn run<R: tokio::io::AsyncRead + Unpin>(
        &self,
        role: &str,
        path: &Path,
        input: R,
        deadline: Duration,
        cancel: &Cancellation,
    ) -> Result<Response> {
        let mut cancellation = cancel.0.subscribe();
        if *cancellation.borrow() {
            return Err(Error::Cancelled);
        }
        let mut command = worker_command(&self.0.worker, path, role)?;
        command
            .env_clear()
            .env("GQL_STMT_CACHE", "0")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let pid = child.id().ok_or(Error::Unavailable)?;
        let mut stdin = child.stdin.take().ok_or(Error::Unavailable)?;
        let stdout = child.stdout.take().ok_or(Error::Unavailable)?;
        let execute = async {
            let write = async {
                tokio::io::copy(&mut input.take(MAX_EXPORT_BYTES + 1), &mut stdin).await?;
                stdin.shutdown().await?;
                drop(stdin);
                Ok::<(), std::io::Error>(())
            };
            let read = async {
                let mut out = Vec::new();
                stdout
                    .take(MAX_RESULT_BYTES as u64 + 1)
                    .read_to_end(&mut out)
                    .await?;
                if out.len() > MAX_RESULT_BYTES {
                    return Err(std::io::Error::other("protocol overrun"));
                }
                Ok::<_, std::io::Error>(out)
            };
            let (_, bytes) = tokio::try_join!(write, read)?;
            let status = child.wait().await?;
            if !status.success() {
                return Err(if status.code() == Some(72) {
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
        let result = tokio::select! {
            result=tokio::time::timeout(deadline,execute)=>result.unwrap_or(Err(Error::Limit)),
            _=cancellation.changed()=>Err(Error::Cancelled),
        };
        if result.is_err() {
            let _ = child.start_kill();
            child.wait().await?;
        }
        result
    }
}
impl Session {
    pub async fn query(&self, generation: &Generation, source: &str, parameters: &[u8], cancel: &Cancellation) -> Result<Response> {
        let admitted = admit(PROFILE, source, parameters).map_err(|e| match e {
            keeper_graph_protocol::Refusal::QueryLimit => Error::Limit,
            _ => Error::Unsupported,
        })?;
        let request = QueryRequest { source: source.to_owned(), parameters: admitted.parameters_json().map_err(|_| Error::Unsupported)? };
        let bytes = serde_json::to_vec(&request).map_err(|_| Error::Unsupported)?;
        let response = self.request(generation, "query", bytes, cancel).await?;
        if response.rows.len() > admitted.limit() { return Err(Error::Unavailable); }
        Ok(response)
    }
    pub async fn inspect(&self, generation: &Generation, id: &str, include_archived: bool, cancel: &Cancellation) -> Result<Response> {
        if !keeper_graph_protocol::wire::ulid(id) { return Err(Error::Unsupported); }
        let request = keeper_graph_protocol::wire::InspectRequest { id: id.to_owned(), include_archived };
        let bytes = serde_json::to_vec(&request).map_err(|_| Error::Unsupported)?;
        let response = self.request(generation, "inspect", bytes, cancel).await?;
        if response.rows.len() > 1 { return Err(Error::Unavailable); }
        Ok(response)
    }
    async fn request(&self, generation: &Generation, role: &str, mut bytes: Vec<u8>, cancel: &Cancellation) -> Result<Response> {
        if generation.manifest.domain != self.domain { return Err(Error::Unavailable); }
        let _queue = self.supervisor.0.queued.try_acquire().map_err(|_| Error::Limit)?;
        let mut cancellation = cancel.0.subscribe();
        if *cancellation.borrow() { return Err(Error::Cancelled); }
        let admission = async {
            let a = self.context.acquire().await.map_err(|_| Error::Unavailable)?;
            let b = self.agent.acquire().await.map_err(|_| Error::Unavailable)?;
            let c = self.supervisor.0.global.acquire().await.map_err(|_| Error::Unavailable)?;
            Ok::<_, Error>((a,b,c))
        };
        let _permits = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(3), admission) => result.map_err(|_| Error::Limit)??,
            _ = cancellation.changed() => return Err(Error::Cancelled),
        };
        bytes.push(b'\n');
        let response = self.supervisor.run(role, &generation.path, bytes.as_slice(), Duration::from_secs(3), cancel).await?;
        if response.manifest != generation.manifest { return Err(Error::Unavailable); }
        Ok(response)
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

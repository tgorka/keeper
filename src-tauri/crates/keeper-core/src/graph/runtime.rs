//! Where and how a graph worker may run: the verified worker executable and
//! the runtime class admitted for it (OD-A Q6, OD-B, OD-C).
//!
//! R1 admits exactly one Linux class, `agent-image/linux-amd64/trixie/r1`:
//! a Linux worker is spawned only when this binary was built for
//! `linux`/`x86_64`, a runtime ledger was embedded at build time, the
//! worker's digest is the ledger's, the tree and the process verify against
//! it ([`super::ledger`]) and the launch prerequisites (P6c) hold.
//! Otherwise — any other Linux tuple (an arm64 image has no worker; a
//! host-native build is not the measured class) — graph is
//! `projection_unavailable` before any child exists. macOS is evidence-only
//! (OD-C): nothing shipped enables it, and only an explicit
//! [`Runtime::macos_evidence`] runs a worker there, under the proof's
//! `sandbox-exec` profile. Every other target has no graph.
//!
//! Every spawn rechecks the class first ([`Runtime::before_spawn`]) and runs
//! the spawning-thread check as the last statement before `spawn`, with
//! nothing awaited between ([`Runtime::spawn`]). A failed recheck
//! invalidates the runtime for good: the consumer opens a new one, which
//! verifies everything again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use super::query::QueryError;

/// A worker executable whose bytes are the expected build: an absolute
/// path and the SHA-256 of its file, fixed when it was opened. There is no
/// search path and no fallback.
#[derive(Clone, Debug)]
pub struct VerifiedWorker {
    path: PathBuf,
    sha256: String,
}

impl VerifiedWorker {
    /// Open the worker at `path`, whose digest must be `expected` (the
    /// digest embedded at build time; `None` when none was, which refuses).
    pub fn open(path: &Path, expected: Option<&str>) -> Result<Self, QueryError> {
        let expected = expected.ok_or(QueryError::Unavailable("worker.digest"))?;
        if !path.is_absolute()
            || path.components().any(|c| {
                !matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
        {
            return Err(QueryError::Unavailable("worker.path"));
        }
        let meta =
            std::fs::symlink_metadata(path).map_err(|_| QueryError::Unavailable("worker.path"))?;
        if !meta.is_file() {
            return Err(QueryError::Unavailable("worker.path"));
        }
        let mut file =
            std::fs::File::open(path).map_err(|_| QueryError::Unavailable("worker.path"))?;
        let mut hash = Sha256::new();
        std::io::copy(&mut file, &mut hash).map_err(|_| QueryError::Unavailable("worker.path"))?;
        let sha256: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if sha256 != expected {
            return Err(QueryError::Unavailable("worker.digest"));
        }
        Ok(Self {
            path: path.to_owned(),
            sha256,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

enum Class {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    Ledger(super::ledger::Verified),
    #[cfg(target_os = "macos")]
    MacosEvidence,
    /// Unit tests of the supervisor run the real worker without the ledger
    /// (a dev host is never the measured class).
    #[cfg(test)]
    Test,
    /// Keeps the enum inhabited-checkable on targets with no class.
    #[allow(dead_code)]
    Never(std::convert::Infallible),
}

/// A worker and the runtime class it was admitted in.
pub struct Runtime {
    worker: VerifiedWorker,
    class: Class,
    invalidated: AtomicBool,
}

/// Q6 rule 1–2 as a decision: which target may admit a Linux worker at all.
pub(crate) fn class_decision(os: &str, arch: &str, ledger: bool) -> Result<(), &'static str> {
    match (os, arch) {
        ("linux", "x86_64") if ledger => Ok(()),
        ("linux", "x86_64") => Err("ledger.absent"),
        ("linux", _) => Err("platform.target"),
        ("macos", _) => Err("macos.evidence_only"),
        _ => Err("platform.unsupported"),
    }
}

impl Runtime {
    /// Admit `worker` in R1's Linux class under the embedded `ledger`
    /// (`None` when none was embedded): the full verification of §4.4
    /// steps 1–7 and P6c.
    pub fn admit(worker: VerifiedWorker, ledger: Option<&str>) -> Result<Self, QueryError> {
        class_decision(
            std::env::consts::OS,
            std::env::consts::ARCH,
            ledger.is_some(),
        )
        .map_err(QueryError::Unavailable)?;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let ledger = super::ledger::Ledger::parse(ledger.unwrap_or_default())
                .map_err(QueryError::Unavailable)?;
            let (path, sha) = ledger.worker();
            if Path::new(path) != worker.path() || sha != worker.sha256() {
                return Err(QueryError::Unavailable("worker.ledger"));
            }
            let verified = ledger
                .verify_rootfs(Path::new("/"))
                .map_err(QueryError::Unavailable)?;
            ledger.verify_process().map_err(QueryError::Unavailable)?;
            launch_prerequisites()?;
            Ok(Self {
                worker,
                class: Class::Ledger(verified),
                invalidated: AtomicBool::new(false),
            })
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            let _ = worker;
            Err(QueryError::Unavailable("platform.target"))
        }
    }

    /// macOS only, evidence only (OD-C): run `worker` under the constant
    /// `sandbox-exec` profile. Nothing shipped calls this; the headless
    /// driver does when asked explicitly.
    #[cfg(target_os = "macos")]
    pub fn macos_evidence(worker: VerifiedWorker) -> Result<Self, QueryError> {
        Ok(Self {
            worker,
            class: Class::MacosEvidence,
            invalidated: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_tests(worker: VerifiedWorker) -> Self {
        Self {
            worker,
            class: Class::Test,
            invalidated: AtomicBool::new(false),
        }
    }

    #[must_use]
    pub fn worker(&self) -> &VerifiedWorker {
        &self.worker
    }

    /// Whether a failed recheck invalidated this runtime.
    #[must_use]
    pub fn is_invalidated(&self) -> bool {
        self.invalidated.load(Ordering::SeqCst)
    }

    /// The per-spawn recheck (§4.5) and P6c, before any child, staging file
    /// or output exists. A failure invalidates the runtime.
    pub(crate) fn before_spawn(&self) -> Result<(), QueryError> {
        if self.is_invalidated() {
            return Err(QueryError::Unavailable("runtime.invalidated"));
        }
        let result = match &self.class {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            Class::Ledger(verified) => verified
                .recheck()
                .map_err(QueryError::Unavailable)
                .and_then(|()| launch_prerequisites()),
            #[cfg(target_os = "macos")]
            Class::MacosEvidence => Ok(()),
            #[cfg(test)]
            Class::Test => Ok(()),
            Class::Never(never) => match *never {},
        };
        if result.is_err() {
            self.invalidated.store(true, Ordering::SeqCst);
        }
        result
    }

    /// The worker's command for `role` over `path`: the fixed environment
    /// only, stdin and stdout piped, stderr `/dev/null`, no namespace flag.
    pub(crate) fn command(
        &self,
        role: &str,
        path: &Path,
    ) -> Result<tokio::process::Command, QueryError> {
        let mut command = worker_command(self.worker.path(), path, role)?;
        command.env_clear().env("GQL_STMT_CACHE", "0");
        #[cfg(target_os = "linux")]
        command.env(
            "GLIBC_TUNABLES",
            super::protocol::worker::LINUX_WORKER_GLIBC_TUNABLES,
        );
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        Ok(command)
    }

    /// Step 5 on this thread, then the spawn, synchronously.
    pub(crate) fn spawn(
        &self,
        command: &mut tokio::process::Command,
    ) -> Result<tokio::process::Child, QueryError> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Class::Ledger(_) = &self.class {
            if let Err(label) = super::ledger::spawning_thread() {
                self.invalidated.store(true, Ordering::SeqCst);
                return Err(QueryError::Unavailable(label));
            }
        }
        command
            .spawn()
            .map_err(|_| QueryError::Unavailable("worker.spawn"))
    }
}

/// The largest inherited soft `RLIMIT_STACK` the worker's startup
/// accounting admits.
pub const MAX_INHERITED_STACK: u64 = 8 * 1024 * 1024;

/// P6c: checked before every Linux worker spawn. The child inherits this
/// process's soft stack limit and runs ld.so with this host's loader
/// configuration; both are startup premises of the worker's accounting.
#[cfg(target_os = "linux")]
fn launch_prerequisites() -> Result<(), QueryError> {
    let stack = rustix::process::getrlimit(rustix::process::Resource::Stack).current;
    launch_decision(
        stack,
        std::fs::symlink_metadata("/etc/ld.so.preload").map(|_| ()),
    )
}

/// The decision alone: a finite soft stack limit of at most 8 MiB (`None`
/// is infinite), and `/etc/ld.so.preload` positively absent. Any other
/// lookup outcome, a dangling symlink included, refuses.
#[cfg(any(target_os = "linux", test))]
fn launch_decision(
    stack_soft: Option<u64>,
    preload: std::io::Result<()>,
) -> Result<(), QueryError> {
    match stack_soft {
        Some(limit) if limit <= MAX_INHERITED_STACK => {}
        _ => return Err(QueryError::Unavailable("launch.stack")),
    }
    match preload {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(QueryError::Unavailable("launch.preload")),
    }
}

fn worker_command(
    worker: &Path,
    path: &Path,
    role: &str,
) -> Result<tokio::process::Command, QueryError> {
    #[cfg(target_os = "macos")]
    {
        // dyld's libignition opens exactly "/" as an openat root. SQLite also
        // resolves ancestor metadata. Neither rule permits descendant file data.
        const READ: &str = r#"(version 1)(deny default)(allow process-exec (literal (param "WORKER")))(allow file-read* (subpath "/System/Library") (subpath "/usr/lib") (literal (param "WORKER")) (literal (param "DB")) (literal "/dev/fd") (literal "/"))(allow file-read-metadata file-test-existence (path-ancestors (param "DB")) (path-ancestors (param "WORKER")))(allow sysctl-read)(deny process-fork)"#;
        const BUILD: &str = r#"(version 1)(deny default)(allow process-exec (literal (param "WORKER")))(allow file-read* (subpath "/System/Library") (subpath "/usr/lib") (literal (param "WORKER")) (literal (param "DB")) (literal "/dev/fd") (literal "/"))(allow file-read-metadata file-test-existence (path-ancestors (param "DB")) (path-ancestors (param "WORKER")))(allow file-write* (literal (param "DB")))(allow sysctl-read)(deny process-fork)"#;
        let path = path
            .canonicalize()
            .map_err(|_| QueryError::Storage("generation path"))?;
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

    /// Q6 / OD-B / OD-C guard: only linux/x86_64 with an embedded ledger
    /// may admit a worker; arm64 Linux, macOS without the explicit evidence
    /// path and every other target are unavailable.
    #[test]
    fn only_the_agent_image_class_is_admitted() {
        assert_eq!(class_decision("linux", "x86_64", true), Ok(()));
        assert_eq!(
            class_decision("linux", "x86_64", false),
            Err("ledger.absent")
        );
        assert_eq!(
            class_decision("linux", "aarch64", true),
            Err("platform.target")
        );
        assert_eq!(
            class_decision("macos", "aarch64", true),
            Err("macos.evidence_only")
        );
        assert_eq!(
            class_decision("ios", "aarch64", true),
            Err("platform.unsupported")
        );
        assert_eq!(
            class_decision("android", "aarch64", true),
            Err("platform.unsupported")
        );
    }

    /// `linux_without_ledger_unavailable`, `dev_host_not_admitted`: this
    /// host's real worker digest, without a ledger or with one that does not
    /// name this tree, never admits.
    #[test]
    fn a_dev_host_without_or_with_a_foreign_ledger_is_not_admitted() {
        let exe = std::env::current_exe().expect("exe");
        let mut hash = Sha256::new();
        std::io::copy(&mut std::fs::File::open(&exe).expect("open"), &mut hash).expect("hash");
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let worker = || VerifiedWorker::open(&exe, Some(&digest)).expect("verified");
        assert!(matches!(
            Runtime::admit(worker(), None),
            Err(QueryError::Unavailable(_))
        ));
        assert!(matches!(
            Runtime::admit(worker(), Some("{}")),
            Err(QueryError::Unavailable(_))
        ));
    }

    #[test]
    fn a_wrong_digest_relative_path_or_absent_digest_refuses() {
        let exe = std::env::current_exe().expect("exe");
        assert!(matches!(
            VerifiedWorker::open(&exe, Some(&"0".repeat(64))),
            Err(QueryError::Unavailable("worker.digest"))
        ));
        assert!(matches!(
            VerifiedWorker::open(&exe, None),
            Err(QueryError::Unavailable("worker.digest"))
        ));
        assert!(matches!(
            VerifiedWorker::open(
                Path::new("target/release/keeper-graph-query"),
                Some(&"0".repeat(64))
            ),
            Err(QueryError::Unavailable("worker.path"))
        ));
        let dotted = exe.parent().expect("dir").join("..").join("x");
        assert!(matches!(
            VerifiedWorker::open(&dotted, Some(&"0".repeat(64))),
            Err(QueryError::Unavailable("worker.path"))
        ));
    }

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
                Err(QueryError::Unavailable("launch.stack"))
            ));
        }
        for preload in [
            Ok(()),
            Err(IoError::from(ErrorKind::PermissionDenied)),
            Err(IoError::other("lookup failed")),
        ] {
            assert!(matches!(
                launch_decision(Some(MAX_INHERITED_STACK), preload),
                Err(QueryError::Unavailable("launch.preload"))
            ));
        }
    }
}

//! An agent's `run` on this host (AD-405, D-33, FR-809): what keeper-core's
//! [`keeper_core::agents::run`] decides, done on the disk.
//!
//! [`SandboxHost::prepare`] reads what a call relies on — the session's
//! `workspace/`, reached from the sessions zone through real folders only
//! (a link in its place or an ancestor's is refused, never followed), where
//! `cwd` lands (keeper-sync's [`resolve`], never path arithmetic), where its
//! program and a wrapper's program resolve and their SHA-256, `workspace/`'s
//! files and theirs — and composes what an approval binds;
//! [`SandboxHost::execute`] checks every grant against what the host may
//! never grant, makes the run's own `HOME` and `TMPDIR`, mounts the plan,
//! checks once more that what runs is what was checked ([`verify`]) and
//! starts the program in its own process group through the host's sandbox,
//! bounds its time, its output and the draining of its pipes, kills the
//! whole group and removes both folders. There is no path through here that
//! starts a program without the sandbox: a host whose probe failed has no
//! [`SandboxHost`], and `run` is not offered there.
//!
//! [`resolve`]: keeper_sync::browse::resolve

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::agents::approval::sha256_hex;
use keeper_core::agents::run::{
    self as core_run, Access, Captured, Exit, Expected, Operand, Reaped, RunFacts, RunRequest,
    SandboxPlan, SandboxTable, HOME_CREDENTIALS, NETWORK_WORKSPACE_MAX_FILES, NEVER_MOUNTED,
    STREAM_CAP,
};
use keeper_core::agents::tier::CallFacts;
use serde_json::Value;

/// The argv agentd answers as the trampoline, before anything else runs.
pub const TRAMPOLINE_ARG: &str = "__keeper-run-sandbox";
/// The trampoline's exit when the sandbox could not be applied.
pub const NOT_SANDBOXED: u8 = 125;
/// What the trampoline writes on both streams once the sandbox holds and
/// right before the program starts: everything before it is not the
/// program's, and a run whose stream never carries it never ran sandboxed.
const SENTINEL_HEAD: &[u8] = b"\0keeper-sandbox:landlock-abi=";
/// How long a run's pipes are drained once its group is killed: a process
/// that kept one open past this is not waited for (R213).
const DRAIN: Duration = Duration::from_secs(2);

/// [`SENTINEL_HEAD`], the kernel's landlock ABI, and its end.
pub fn sentinel(abi: i32) -> String {
    format!("\0keeper-sandbox:landlock-abi={abi}\0\n")
}

/// How this host applies a sandbox.
#[derive(Debug, Clone)]
pub enum Kind {
    /// Linux: a program that answers [`TRAMPOLINE_ARG`] — agentd itself —
    /// started with `args` and the plan file's path after them.
    Trampoline {
        program: PathBuf,
        args: Vec<OsString>,
    },
    /// macOS: `/usr/bin/sandbox-exec`.
    SandboxExec,
}

/// A host whose probe passed: how it sandboxes, its name, its checked
/// `[sandbox]` table, what it may never grant, and what the probe found.
#[derive(Debug, Clone)]
pub struct SandboxHost {
    kind: Kind,
    host: String,
    table: SandboxTable,
    forbidden: Forbidden,
    /// `landlock ABI 7, seccomp ok`, or `sandbox-exec ok`.
    pub status: String,
}

/// What a host is told it may not grant: its drives, its own folders and
/// secrets, its user's home — canonical once the probe has read them.
#[derive(Debug, Clone, Default)]
pub struct Forbidden {
    pub drives: Vec<(String, PathBuf)>,
    pub secrets: Vec<PathBuf>,
    pub home: Option<PathBuf>,
}

impl Forbidden {
    fn canonical(&self) -> Forbidden {
        Forbidden {
            drives: self
                .drives
                .iter()
                .map(|(id, root)| (id.clone(), canonical(root)))
                .collect(),
            secrets: self.secrets.iter().map(|dir| canonical(dir)).collect(),
            home: self.home.as_deref().map(canonical),
        }
    }

    /// Where each credential under the home leads now (R231, R247): the
    /// credential as it resolves and every link below it as it resolves,
    /// a link to a folder followed into that folder as a run would read
    /// through it — each folder once, by its device and inode, so a cycle
    /// ends. Read at each check, so a link made or retargeted since the
    /// probe counts. `Err` when an existing credential, or a folder or an
    /// entry below one, cannot be read: where it leads is not known, so
    /// nothing may be granted.
    fn credentials(&self) -> Result<Vec<PathBuf>, String> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let Some(home) = &self.home else {
            return Ok(out);
        };
        for credential in HOME_CREDENTIALS
            .iter()
            .map(|credential| home.join(credential))
        {
            match std::fs::symlink_metadata(&credential) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(unknown(&credential, &error)),
                Ok(_) => leads(&credential, &mut out, &mut seen)?,
            }
        }
        Ok(out)
    }

    /// Why `path`, as it resolves now, may not be granted, `credentials`
    /// being [`Self::credentials`].
    fn refusal(&self, path: &Path, credentials: &[PathBuf]) -> Option<String> {
        core_run::grant_refusal(
            &canonical(path),
            &self.drives,
            &self.secrets,
            self.home.as_deref(),
            credentials,
        )
    }
}

/// Why nothing is granted when `path`, a credential or what is below one,
/// cannot be read.
fn unknown(path: &Path, error: &std::io::Error) -> String {
    format!(
        "anything while {} cannot be read ({error}): where a credential under the home folder leads is not known",
        path.display()
    )
}

/// Push where `path` leads onto `out` and, when it is a folder not in
/// `seen`, where each link below it leads, its folders and the folders
/// its links lead to read the same way. A link that leads nowhere leads
/// to nothing to protect; anything that cannot be read is `Err`.
fn leads(
    path: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<(u64, u64)>,
) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let target = match path.canonicalize() {
        Ok(target) => target,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_symlink()) =>
        {
            return Ok(());
        }
        Err(error) => return Err(unknown(path, &error)),
    };
    let meta = std::fs::metadata(&target).map_err(|error| unknown(path, &error))?;
    out.push(target.clone());
    if !meta.is_dir() || !seen.insert((meta.dev(), meta.ino())) {
        return Ok(());
    }
    for entry in std::fs::read_dir(&target).map_err(|error| unknown(&target, &error))? {
        let entry = entry.map_err(|error| unknown(&target, &error))?;
        let kind = entry
            .file_type()
            .map_err(|error| unknown(&entry.path(), &error))?;
        if kind.is_symlink() || kind.is_dir() {
            leads(&entry.path(), out, seen)?;
        }
    }
    Ok(())
}

/// Where a session's runs work (R231): its drive's checkout, the sessions
/// zone's folder in it as the drive's profile names it, and the session's
/// folder in that zone.
#[derive(Debug, Clone, Copy)]
pub struct Session<'a> {
    pub drive: &'a Path,
    pub zone: &'a str,
    pub path: &'a str,
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A folder's identity: its device and inode.
fn identity(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).map(|meta| (meta.dev(), meta.ino()))
}

/// The session's `workspace/` and its identity (R213, R231, R247): from
/// the drive's checkout down — each folder of the sessions zone, the
/// session's, the workspace — every folder a real one, made when absent,
/// a link or a file in any place refused. Each folder is opened from the
/// one held before it, never by its path, its last component never
/// followed, and made there when absent: a folder already checked that
/// is swapped for a link before the next step redirects neither a check
/// nor a folder made, and a path that no longer names the folder held at
/// the end is refused. The identity is the held folder's, which the plan
/// binds. The checkout itself is where the person put the drive, taken as
/// it resolves.
fn real_workspace(session: Session<'_>) -> Result<(PathBuf, (u64, u64)), String> {
    walk_workspace(session, &mut |_| {})
}

/// [`real_workspace`], `held` told the path of each folder once it is
/// held.
fn walk_workspace(
    session: Session<'_>,
    held: &mut dyn FnMut(&Path),
) -> Result<(PathBuf, (u64, u64)), String> {
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::MetadataExt;
    let refused = |sentence: String| format!("This run was refused: {sentence}");
    let not_real = |at: &Path, error: &dyn std::fmt::Display| {
        refused(format!(
            "its workspace is not a real folder ({}: {error}); keeper does not follow a link there.",
            at.display()
        ))
    };
    let folder = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut at = session
        .drive
        .canonicalize()
        .map_err(|error| refused(format!("its drive could not be read: {error}")))?;
    let mut dir = rustix::fs::open(&at, folder, Mode::empty())
        .map_err(|error| refused(format!("its drive could not be read: {error}")))?;
    held(&at);
    for part in session
        .zone
        .trim()
        .split('/')
        .chain(session.path.split('/'))
        .filter(|part| !part.is_empty())
        .chain([keeper_core::sessions::model::WORKSPACE_DIR])
    {
        if part == "." || part == ".." {
            return Err(refused(format!(
                "{} is not a session folder.",
                session.path
            )));
        }
        at = at.join(part);
        match rustix::fs::mkdirat(&dir, part, Mode::from_raw_mode(0o777)) {
            Ok(()) => {}
            Err(error) if error == rustix::io::Errno::EXIST => {}
            Err(error) => return Err(not_real(&at, &error)),
        }
        dir = rustix::fs::openat(&dir, part, folder, Mode::empty())
            .map_err(|error| not_real(&at, &error))?;
        held(&at);
    }
    let meta = std::fs::File::from(dir)
        .metadata()
        .map_err(|error| not_real(&at, &error))?;
    let id = (meta.dev(), meta.ino());
    if identity(&at).ok() != Some(id) {
        return Err(not_real(
            &at,
            &"a link was put on its way while keeper walked to it",
        ));
    }
    Ok((at, id))
}

impl SandboxHost {
    /// Probe `kind` on the host named `host` (96.1 #11): every `read_exec`
    /// folder and `env` path checked against `forbidden` (R148), then
    /// `/usr/bin/true` run through the whole sandbox — on Linux the
    /// trampoline must apply landlock at ABI ≥ 6 and the seccomp filter, on
    /// macOS `sandbox-exec` must run it — and exit 0. `Err` is the reason
    /// the host offers no `sandbox`.
    pub fn probe(
        kind: Kind,
        host: &str,
        table: &SandboxTable,
        forbidden: &Forbidden,
    ) -> Result<SandboxHost, String> {
        let forbidden = forbidden.canonical();
        let credentials = forbidden
            .credentials()
            .map_err(|reason| format!("[sandbox] cannot grant it {reason}"))?;
        for path in table
            .read_exec
            .iter()
            .chain(table.env.iter().map(|(_, path)| path))
        {
            if let Some(reason) = forbidden.refusal(path, &credentials) {
                return Err(format!("[sandbox] cannot grant it: {reason}"));
            }
        }
        match &kind {
            #[cfg(target_os = "macos")]
            Kind::SandboxExec if !macos::present() => {
                return Err(format!("{} is not on this Mac", macos::SANDBOX_EXEC));
            }
            Kind::SandboxExec if !cfg!(target_os = "macos") => {
                return Err("sandbox-exec exists only on macOS".to_owned());
            }
            Kind::Trampoline { .. } if !cfg!(target_os = "linux") => {
                return Err("landlock and seccomp exist only on Linux".to_owned());
            }
            _ => {}
        }
        let mut sandbox = SandboxHost {
            kind,
            host: host.to_owned(),
            table: table.clone(),
            forbidden,
            status: String::new(),
        };
        let zone = tempfile_dir("probe")?;
        let probed = sandbox.prepare(
            &serde_json::json!({"argv": ["true"]}),
            Session {
                drive: &zone,
                zone: "",
                path: "probe",
            },
            &[],
        );
        let ran = probed.and_then(|prepared| sandbox.execute(&prepared));
        let _ = std::fs::remove_dir_all(&zone);
        let ran = ran?;
        if ran.exit != Exit::Code(0) {
            return Err(format!(
                "a probe of /usr/bin/true did not exit 0: {}",
                ran.stderr.text().trim()
            ));
        }
        sandbox.status = match (&sandbox.kind, ran.abi) {
            (Kind::Trampoline { .. }, Some(abi)) => format!("landlock ABI {abi}, seccomp ok"),
            (Kind::Trampoline { .. }, None) => {
                return Err("the trampoline never said the sandbox held".to_owned())
            }
            (Kind::SandboxExec, _) => {
                "sandbox-exec ok; no [sandbox] table is configurable on this Mac yet".to_owned()
            }
        };
        Ok(sandbox)
    }

    /// Read what the call `args` relies on, in `session`, among `drives` —
    /// the drives in its scope this host mounts and the agent's grants let
    /// it read, by id and checkout — and compose what its approval binds.
    pub fn prepare(
        &self,
        args: &Value,
        session: Session<'_>,
        drives: &[(String, PathBuf)],
    ) -> Result<Prepared, String> {
        let request = core_run::parse_request(args)?;
        let refused = |sentence: String| format!("This run was refused: {sentence}");
        let (root, workspace_id) = real_workspace(session)?;
        let cwd = match keeper_sync::browse::resolve(&root, &request.cwd) {
            Ok(Some(cwd)) if cwd.is_dir() => cwd,
            Ok(_) => {
                return Err(refused(format!(
                    "`workspace/{}` is not a folder.",
                    request.cwd
                )))
            }
            Err(_) => {
                return Err(refused(format!(
                    "`workspace/{}` leads outside the workspace.",
                    request.cwd
                )))
            }
        };
        let unread =
            |error: std::io::Error| refused(format!("its workspace could not be read: {error}"));
        let ids = (workspace_id, identity(&cwd).map_err(unread)?);
        let mounted: Vec<(String, PathBuf)> = request
            .read
            .iter()
            .map(|id| {
                drives
                    .iter()
                    .find(|(drive, _)| drive == id)
                    .map(|(drive, root)| (drive.clone(), canonical(root)))
                    .ok_or_else(|| {
                        refused(format!(
                            "{id} is not a drive this session may read on this host."
                        ))
                    })
            })
            .collect::<Result<_, _>>()?;
        let hashed = |path: &Path| {
            std::fs::read(path)
                .map(|bytes| sha256_hex(&bytes))
                .map_err(|error| refused(format!("its program could not be read: {error}")))
        };
        let not_found = |program: &str| {
            refused(format!(
                "`{program}` is not a program on this host's PATH or in the workspace."
            ))
        };
        let exe = self
            .resolve(&request.argv[0], &cwd)
            .ok_or_else(|| not_found(&request.argv[0]))?;
        let exe_sha256 = hashed(&exe)?;
        // Each program a wrapper starts, resolved as the wrapper resolves
        // it: on the run's PATH, or from `cwd` (R247).
        let mut chain = Vec::new();
        for at in core_run::started(&request.argv) {
            let path = self
                .resolve(&request.argv[at], &cwd)
                .ok_or_else(|| not_found(&request.argv[at]))?;
            let sha256 = hashed(&path)?;
            chain.push((at, path, sha256));
        }
        let shown = |(_, path, sha256): &(usize, PathBuf, String)| {
            (path.display().to_string(), sha256.clone())
        };
        let (listing, workspace_bytes) = list_workspace(&root).map_err(refused)?;
        let inside = |path: &Path| {
            path.strip_prefix(&root).ok().and_then(|rel| {
                let names: Option<Vec<&str>> = rel
                    .components()
                    .map(|part| part.as_os_str().to_str())
                    .collect();
                names.map(|names| names.join("/"))
            })
        };
        let in_workspace = std::iter::once((exe.clone(), exe_sha256.clone()))
            .chain(
                chain
                    .iter()
                    .map(|(_, path, sha256)| (path.clone(), sha256.clone())),
            )
            .filter_map(|(path, sha256)| inside(&path).map(|rel| (rel, sha256)))
            .collect();
        let script = core_run::script_operand(&request.argv)
            .and_then(|operand| cwd.join(operand).canonicalize().ok())
            .and_then(|path| inside(&path))
            .and_then(|rel| listing.iter().find(|(path, _)| *path == rel).cloned());
        let names: Vec<&str> = listing.iter().map(|(path, _)| path.as_str()).collect();
        let held_configs = held_configs(&root, &names).map_err(refused)?;
        for (path, sha256) in &held_configs {
            if !listing.contains(&(path.clone(), sha256.clone())) {
                return Err(refused(format!("{path} changed while keeper read it.")));
            }
        }
        if identity(&root).ok() != Some(workspace_id) {
            return Err(refused(
                "its workspace changed while keeper read it.".to_owned(),
            ));
        }
        let facts = RunFacts {
            host: self.host.clone(),
            exe: exe.display().to_string(),
            exe_sha256,
            program: chain.last().map(shown),
            wrappers: chain
                .iter()
                .take(chain.len().saturating_sub(1))
                .map(shown)
                .collect(),
            in_workspace,
            cwd: inside(&cwd).unwrap_or_default(),
            folders: [ids.0, ids.1],
            script,
            listing,
            workspace_bytes,
            held_configs,
        };
        let operands = facts.operands(&request);
        let call_facts = core_run::call_facts(&request, &operands);
        let env = core_run::environment(&self.table.read_exec, &self.table.env, "", "");
        let exec_binding = core_run::exec_binding(&request, &env, &facts, &operands);
        let workspace_set = if request.network {
            if facts.listing.len() > NETWORK_WORKSPACE_MAX_FILES {
                return Err(refused(format!(
                    "a run with network releases its whole workspace, which a person must see first, and this one holds {} files — more than {NETWORK_WORKSPACE_MAX_FILES}. Remove what it does not need, then ask again.",
                    facts.listing.len()
                )));
            }
            Some(core_run::workspace_set(
                &facts.listing,
                facts.workspace_bytes,
            ))
        } else {
            None
        };
        let expect = Expected {
            workspace: root.clone(),
            workspace_id: ids.0,
            cwd_id: ids.1,
            exe_sha256: facts.exe_sha256.clone(),
            programs: chain,
            files: operands
                .iter()
                .filter_map(|operand| Some((operand.path.clone()?, operand.sha256.clone())))
                .collect(),
            workspace_sha256: workspace_set
                .as_ref()
                .and_then(|set| set["sha256"].as_str().map(str::to_owned)),
        };
        Ok(Prepared {
            request,
            facts,
            operands,
            call_facts,
            exec_binding,
            workspace_set,
            workspace: root,
            cwd,
            mounted,
            expect,
        })
    }

    /// The program `program` names from `cwd`: a path when it holds a `/`,
    /// else the first executable file of that name on the run's `PATH`.
    fn resolve(&self, program: &str, cwd: &Path) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let executable = |path: &Path| {
            std::fs::metadata(path)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        };
        let found = if program.contains('/') {
            Some(cwd.join(program)).filter(|path| executable(path))
        } else {
            core_run::SYSTEM_PATH
                .split(':')
                .map(PathBuf::from)
                .chain(self.table.read_exec.iter().cloned())
                .map(|dir| dir.join(program))
                .find(|path| executable(path))
        };
        found.and_then(|path| path.canonicalize().ok())
    }

    /// The plan for `prepared`, with its own `home` and `tmp`. Every grant
    /// beyond the run's own folders is checked as it resolves now against
    /// what the host may never grant (R213): a drive under a system folder,
    /// `/proc`, a credential under the home or where one leads, a
    /// `read_exec` link retargeted since the probe — the run is refused
    /// rather than given it, as it is when where a credential leads cannot
    /// be read (R247).
    fn plan(&self, prepared: &Prepared, home: &Path, tmp: &Path) -> Result<SandboxPlan, String> {
        let request = &prepared.request;
        let refused =
            |reason: String| format!("This run was refused: keeper cannot grant it {reason}.");
        let credentials = self.forbidden.credentials().map_err(refused)?;
        let mut grants: Vec<(PathBuf, Access)> = vec![
            (prepared.workspace.clone(), Access::ReadWrite),
            (home.to_path_buf(), Access::ReadWrite),
            (tmp.to_path_buf(), Access::ReadWrite),
            (PathBuf::from("/dev/null"), Access::ReadWrite),
        ];
        let macos = matches!(self.kind, Kind::SandboxExec);
        for path in core_run::system_read_exec(macos)
            .into_iter()
            .chain(self.table.read_exec.iter().cloned())
            .chain(self.table.env.iter().map(|(_, path)| path.clone()))
            .filter(|path| path.exists())
        {
            if let Some(reason) = self.forbidden.refusal(&path, &credentials) {
                return Err(refused(reason));
            }
            grants.push((canonical(&path), Access::ReadExec));
        }
        for device in ["/dev/zero", "/dev/urandom", "/dev/random"] {
            grants.push((PathBuf::from(device), Access::Read));
        }
        let mut drives = Vec::new();
        for (id, root) in &prepared.mounted {
            if let Some(reason) = core_run::grant_refusal(
                &canonical(root),
                &[],
                &self.forbidden.secrets,
                self.forbidden.home.as_deref(),
                &credentials,
            ) {
                return Err(refused(reason));
            }
            let (entries, excluded) = mount(root).map_err(|error| {
                format!("This run was refused: {id} could not be read: {error}")
            })?;
            if excluded {
                grants.extend(entries.into_iter().map(|path| (path, Access::Read)));
            } else {
                grants.push((root.clone(), Access::Read));
            }
            drives.push(root.clone());
        }
        let env = core_run::environment(
            &self.table.read_exec,
            &self.table.env,
            &home.display().to_string(),
            &tmp.display().to_string(),
        );
        Ok(SandboxPlan {
            exe: PathBuf::from(&prepared.facts.exe),
            argv: core_run::as_run(&request.argv),
            cwd: prepared.cwd.clone(),
            env,
            network: request.network,
            grants,
            drives,
            expect: prepared.expect.clone(),
            timeout_s: request.timeout_s,
        })
    }

    /// Run `prepared` sandboxed (96.1 #4–#8): its own `HOME` and `TMPDIR`,
    /// made empty for it outside the workspace and the drives and removed
    /// after it; what it was checked against checked again ([`verify`]);
    /// its own process group, which keeper's filter lets nothing leave,
    /// killed whole when it ends or its time is up; each stream kept to
    /// [`STREAM_CAP`] bytes and counted, and drained for at most [`DRAIN`]
    /// after.
    pub fn execute(&self, prepared: &Prepared) -> Result<Ran, String> {
        let dir = tempfile_dir("run")?;
        let ran = self.execute_in(prepared, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        ran
    }

    fn execute_in(&self, prepared: &Prepared, dir: &Path) -> Result<Ran, String> {
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        use std::process::{Command, Stdio};

        let failed = |error: std::io::Error| format!("keeper could not start this run: {error}");
        let (home, tmp) = (dir.join("home"), dir.join("tmp"));
        for made in [&home, &tmp] {
            std::fs::create_dir(made).map_err(failed)?;
        }
        let plan = self.plan(prepared, &home, &tmp)?;
        verify(&plan).map_err(|reason| format!("This run did not start: {reason}"))?;
        let mut command = match &self.kind {
            Kind::Trampoline { program, args } => {
                let plan_path = dir.join("plan.json");
                let text = serde_json::to_vec(&plan)
                    .map_err(|error| failed(std::io::Error::other(error)))?;
                std::fs::write(&plan_path, text).map_err(failed)?;
                let mut command = Command::new(program);
                command.args(args).arg(&plan_path).env_clear();
                command
            }
            #[cfg(target_os = "macos")]
            Kind::SandboxExec => macos::command(&plan),
            #[cfg(not(target_os = "macos"))]
            Kind::SandboxExec => return Err("sandbox-exec exists only on macOS".to_owned()),
        };
        let marked = matches!(self.kind, Kind::Trampoline { .. });
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(failed)?;
        let group = rustix::process::Pid::from_raw(child.id() as i32);
        let kill_group = || {
            if let Some(group) = group {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
        };
        let readers = [
            child
                .stdout
                .take()
                .map(|out| Box::new(out) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|err| Box::new(err) as Box<dyn Read + Send>),
        ]
        .map(|stream| {
            let kept = Arc::new(Mutex::new(Stream::default()));
            let (done, ended) = std::sync::mpsc::channel();
            if let Some(stream) = stream {
                let into = Arc::clone(&kept);
                std::thread::spawn(move || {
                    capture(stream, marked, &into);
                    let _ = done.send(());
                });
            }
            (kept, ended)
        });
        let deadline = Instant::now() + Duration::from_secs(prepared.request.timeout_s);
        let status = loop {
            match child.try_wait().map_err(failed)? {
                Some(status) => break Some(status),
                None if Instant::now() >= deadline => break None,
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        // Whatever it left running dies with it: nothing of a run outlives
        // its result.
        kill_group();
        let status = match status {
            Some(status) => Some(status),
            None => {
                let _ = child.wait();
                None
            }
        };
        // On the Mac, what left the group is found by its TMPDIR, and the
        // result says only what was found (DW-757, R231).
        #[cfg(target_os = "macos")]
        let reaped = match self.kind {
            Kind::SandboxExec => Reaped::Swept {
                found: macos::sweep(&tmp),
            },
            Kind::Trampoline { .. } => Reaped::Group,
        };
        #[cfg(not(target_os = "macos"))]
        let reaped = Reaped::Group;
        let drained = Instant::now() + DRAIN;
        let [out, err] = readers.map(|(kept, ended)| {
            let _ = ended.recv_timeout(drained.saturating_duration_since(Instant::now()));
            std::mem::take(&mut *kept.lock().unwrap_or_else(|p| p.into_inner()))
        });
        if marked && err.abi.is_none() {
            return Err(format!(
                "This run did not start: the sandbox could not be applied. {}",
                String::from_utf8_lossy(&err.before).trim()
            ));
        }
        let exit = match status {
            None => Exit::TimedOut(prepared.request.timeout_s),
            Some(status) => match (status.code(), status.signal()) {
                (Some(code), _) => Exit::Code(code),
                (None, Some(signal)) => Exit::Signal(signal),
                (None, None) => Exit::Code(-1),
            },
        };
        Ok(Ran {
            exit,
            reaped,
            abi: err.abi,
            stdout: out.captured(),
            stderr: err.captured(),
        })
    }
}

/// Whether what `plan` runs is still what was checked (R144, R213): the
/// workspace and `cwd` the same folders, by device and inode; the program
/// started and each program a wrapper starts the same bytes; each file of
/// the code the session holds the same bytes, and no repository
/// configuration that runs a program, nor a repository keeper cannot read
/// as git does, there that was not (R247); and, with network, the same
/// workspace set. The host's last step before the program starts — on
/// Linux the trampoline, sandboxed already, through [`verify_program`] —
/// checks it, so a change made while the approval
/// was consumed, or after, is refused, not run.
pub fn verify(plan: &SandboxPlan) -> Result<(), String> {
    let mut program = std::fs::File::open(&plan.exe)
        .map_err(|_| "its program changed after keeper checked it".to_owned())?;
    let mut behind = plan
        .expect
        .programs
        .iter()
        .map(|(_, path, _)| {
            std::fs::File::open(path)
                .map_err(|_| "the program it runs changed after keeper checked it".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    verify_program(plan, &mut program, &mut behind)
}

/// [`verify`] with the program started already open as `program`, and
/// each program a wrapper starts as `behind`, in order: their bytes
/// checked as read through those handles (R231, R247) — the handles the
/// trampoline then starts them by, never their paths again, so what is put
/// at a path after this check is not what runs.
pub fn verify_program(
    plan: &SandboxPlan,
    program: &mut std::fs::File,
    behind: &mut [std::fs::File],
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};
    let expect = &plan.expect;
    let moved = |what: &str| format!("{what} changed after keeper checked it");
    if identity(&expect.workspace).ok() != Some(expect.workspace_id) {
        return Err(moved("its workspace"));
    }
    if identity(&plan.cwd).ok() != Some(expect.cwd_id) {
        return Err(moved("its folder"));
    }
    let hash = |path: &Path| std::fs::read(path).map(|bytes| sha256_hex(&bytes)).ok();
    let read = |file: &mut std::fs::File| {
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.read_to_end(&mut bytes))
            .ok()
            .map(|_| sha256_hex(&bytes))
    };
    if read(program).as_ref() != Some(&expect.exe_sha256) {
        return Err(moved("its program"));
    }
    if behind.len() != expect.programs.len() {
        return Err(moved("the program it runs"));
    }
    for ((_, _, sha256), file) in expect.programs.iter().zip(behind) {
        if read(file).as_ref() != Some(sha256) {
            return Err(moved("the program it runs"));
        }
    }
    for (path, sha256) in &expect.files {
        let held = keeper_sync::browse::resolve(&expect.workspace, path)
            .ok()
            .flatten()
            .and_then(|path| hash(&path));
        if held.as_ref() != Some(sha256) {
            return Err(moved(&format!("workspace/{path}")));
        }
    }
    let names = workspace_names(&expect.workspace)?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    for (path, _) in held_configs(&expect.workspace, &names)? {
        if !expect.files.iter().any(|(held, _)| *held == path) {
            return Err(moved(&format!("workspace/{path}")));
        }
    }
    if let Some(sha256) = &expect.workspace_sha256 {
        let now = list_workspace(&expect.workspace)
            .ok()
            .map(|(listing, bytes)| core_run::workspace_set(&listing, bytes));
        if now.as_ref().and_then(|set| set["sha256"].as_str()) != Some(sha256.as_str()) {
            return Err(moved("the workspace it releases"));
        }
    }
    Ok(())
}

/// What a call relies on, read from the disk.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub request: RunRequest,
    pub facts: RunFacts,
    pub operands: Vec<Operand>,
    pub call_facts: CallFacts,
    /// What its approval binds (AD-393).
    pub exec_binding: Value,
    /// A networked run's `preconditions.workspace` (S-03).
    pub workspace_set: Option<Value>,
    /// `workspace/`, canonical.
    pub workspace: PathBuf,
    pub cwd: PathBuf,
    /// The drives it reads, by id and canonical checkout.
    pub mounted: Vec<(String, PathBuf)>,
    /// What the host checks once more before the program starts.
    pub expect: Expected,
}

/// What a run came to.
#[derive(Debug, Clone)]
pub struct Ran {
    pub exit: Exit,
    /// What keeper stopped of it once it ended.
    pub reaped: Reaped,
    /// The landlock ABI the trampoline reported; `None` on macOS.
    pub abi: Option<i32>,
    pub stdout: Captured,
    pub stderr: Captured,
}

impl Ran {
    /// What the model reads.
    pub fn render(&self) -> String {
        core_run::render(self.exit, self.reaped, &self.stdout, &self.stderr)
    }
}

/// One stream as read: what came before the trampoline's sentinel, the ABI
/// it named, the program's first [`STREAM_CAP`] bytes and their total.
#[derive(Debug, Default)]
struct Stream {
    before: Vec<u8>,
    abi: Option<i32>,
    seen: bool,
    kept: Vec<u8>,
    total: u64,
}

impl Stream {
    fn captured(&self) -> Captured {
        Captured {
            bytes: self.kept.clone(),
            total: self.total,
        }
    }
}

/// Read `stream` to its end into `into`; with `marked`, nothing counts as
/// the program's until the sentinel has passed.
fn capture(mut stream: Box<dyn Read + Send>, marked: bool, into: &Mutex<Stream>) {
    into.lock().unwrap_or_else(|p| p.into_inner()).seen = !marked;
    let mut chunk = [0u8; 8192];
    loop {
        let read = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let bytes = &chunk[..read];
        let mut out = into.lock().unwrap_or_else(|p| p.into_inner());
        if !out.seen {
            out.before.extend_from_slice(bytes);
            let Some(at) = find(&out.before, SENTINEL_HEAD) else {
                // Whatever precedes it is bounded: a trampoline's error.
                if out.before.len() > STREAM_CAP {
                    let excess = out.before.len() - STREAM_CAP;
                    out.before.drain(..excess);
                }
                continue;
            };
            let tail = &out.before[at + SENTINEL_HEAD.len()..];
            let Some(end) = find(tail, b"\0\n") else {
                continue;
            };
            let abi = std::str::from_utf8(&tail[..end])
                .ok()
                .and_then(|abi| abi.parse().ok());
            let rest = tail[end + 2..].to_vec();
            out.abi = abi;
            out.before.truncate(at);
            out.seen = true;
            keep(&mut out, &rest);
            continue;
        }
        keep(&mut out, bytes);
    }
}

fn keep(out: &mut Stream, bytes: &[u8]) {
    let room = STREAM_CAP.saturating_sub(out.kept.len());
    out.kept.extend_from_slice(&bytes[..room.min(bytes.len())]);
    out.total += bytes.len() as u64;
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A new folder of this run's own, `0700`, under the system's temporary
/// folder — outside every workspace and drive (systemd's `PrivateTmp` for
/// agentd).
fn tempfile_dir(what: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("keeper-{what}-{}", ulid::Ulid::new()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|error| format!("keeper could not make a folder for this run: {error}"))?;
    Ok(canonical(&dir))
}

/// Each entry below `root` but its folders, by `/`-joined name relative
/// to it, and its type: links not followed, folders descended. `Err` on a
/// name that is not UTF-8, which keeper cannot show or bind (R213), or on
/// what `each` refuses.
fn walk(
    root: &Path,
    each: &mut dyn FnMut(String, &std::fs::DirEntry, std::fs::FileType) -> Result<(), String>,
) -> Result<(), String> {
    let unread = |error: std::io::Error| format!("its workspace could not be read: {error}");
    let mut pending: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(unread)? {
            let entry = entry.map_err(unread)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                return Err(format!(
                    "workspace/{rel} holds a name that is not UTF-8 ({}), which keeper cannot show or bind. Rename or remove it, then ask again.",
                    entry.file_name().to_string_lossy()
                ));
            };
            let named = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let kind = entry.file_type().map_err(unread)?;
            if kind.is_dir() {
                pending.push((entry.path(), named));
            } else {
                each(named, &entry, kind)?;
            }
        }
    }
    Ok(())
}

/// Every file of `root` and its SHA-256, by `/`-joined names relative to
/// it, links not followed — a link is hashed as its target's spelling —
/// and their bytes summed. `Err` names an entry keeper cannot bind (R213):
/// a name that is not UTF-8, or a FIFO, socket or device, whose bytes a
/// program could read without their being in any set.
fn list_workspace(root: &Path) -> Result<(Vec<(String, String)>, u64), String> {
    let unread = |error: std::io::Error| format!("its workspace could not be read: {error}");
    let mut out = Vec::new();
    let mut bytes = 0u64;
    walk(root, &mut |named, entry, kind| {
        if kind.is_symlink() {
            let target = std::fs::read_link(entry.path()).map_err(unread)?;
            let spelled = format!("symlink:{}", target.display());
            out.push((named, sha256_hex(spelled.as_bytes())));
        } else if kind.is_file() {
            let content = std::fs::read(entry.path()).map_err(unread)?;
            bytes += content.len() as u64;
            out.push((named, sha256_hex(&content)));
        } else {
            return Err(format!(
                "workspace/{named} is not a file, a folder or a link (a pipe, a socket or a device), which keeper cannot bind. Remove it, then ask again."
            ));
        }
        Ok(())
    })?;
    out.sort();
    Ok((out, bytes))
}

/// The name of every file and link of `root`, as [`list_workspace`] names
/// them, no file read.
fn workspace_names(root: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    walk(root, &mut |named, _, _| {
        out.push(named);
        Ok(())
    })?;
    Ok(out)
}

/// Each repository configuration of `workspace/` (canonical `root`, the
/// `names` of its files) that starts a program or reads more
/// ([`core_run::git_config_runs_code`]), and its SHA-256 as read now.
fn held_configs(root: &Path, names: &[&str]) -> Result<Vec<(String, String)>, String> {
    let mut held = Vec::new();
    for path in git_configs(root, names)? {
        let text = std::fs::read(root.join(&path))
            .map_err(|error| format!("workspace/{path} could not be read: {error}"))?;
        if core_run::git_config_runs_code(&String::from_utf8_lossy(&text)) {
            held.push((path, sha256_hex(&text)));
        }
    }
    Ok(held)
}

/// Every file of `workspace/` (canonical `root`, the `names` of its
/// files) that git would read as a repository's own configuration (R231),
/// found the ways git finds a repository: a `config` or `config.worktree`
/// in any `.git/`; those of every folder git takes for a repository's own
/// — one whose `HEAD` reads as git's ([`headref`]) beside an `objects` and
/// a `refs` git can enter, as a bare repository or a separate git folder
/// is; and those of the folder a `.git` file (`gitdir: …`) names, or a
/// `commondir` file of a repository's own folders — one beside a `HEAD`,
/// or inside a `.git/` (R247); a file of that name anywhere else is a
/// project's and names nothing. Every repository of the workspace counts,
/// not only the one a run's folder finds, so none is missed. `Err` when
/// such a folder is not one keeper can find inside `workspace/`, and when
/// git would follow a link keeper does not read as git does — a `.git`,
/// an `objects` or `refs` beside a `HEAD` that reads as git's, a pointer
/// or a configuration that is a link (R247, R260): keeper cannot tell what
/// git would read there, so it refuses the run rather than taking it for
/// no repository. A folder git cannot take for one refuses nothing.
fn git_configs(root: &Path, names: &[&str]) -> Result<BTreeSet<String>, String> {
    let split = |path: &str| -> (String, String) {
        match path.rsplit_once('/') {
            Some((dir, leaf)) => (dir.to_owned(), leaf.to_owned()),
            None => (String::new(), path.to_owned()),
        }
    };
    let joined = |dir: &str, leaf: &str| {
        if dir.is_empty() {
            leaf.to_owned()
        } else {
            format!("{dir}/{leaf}")
        }
    };
    let unknown = |path: &str| {
        format!(
            "workspace/{path} names a repository's folder outside the workspace, or none, so keeper cannot tell what git would run there. Move the repository into the workspace, then ask again."
        )
    };
    let linked = |path: &str| {
        format!(
            "workspace/{path} is a link where git keeps a repository's own folders or files, which git follows and keeper does not, so keeper cannot tell what git would run there. Make it a real folder or file, then ask again."
        )
    };
    let link =
        |path: &str| std::fs::symlink_metadata(root.join(path)).is_ok_and(|meta| meta.is_symlink());
    // The folder `named` leads to from `workspace/<dir>`, inside it.
    let inside = |dir: &str, named: &Path, path: &str| -> Result<String, String> {
        let at = root.join(dir).join(named).canonicalize();
        at.ok()
            .and_then(|at| {
                at.strip_prefix(root).ok().and_then(|rel| {
                    rel.components()
                        .map(|part| part.as_os_str().to_str())
                        .collect::<Option<Vec<_>>>()
                        .map(|names| names.join("/"))
                })
            })
            .ok_or_else(|| unknown(path))
    };
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path))
            .map_err(|error| format!("workspace/{path} could not be read: {error}"))
    };
    let listed: BTreeSet<&str> = names.iter().copied().collect();
    let mut repositories = BTreeSet::new();
    let mut configs: BTreeSet<String> = names
        .iter()
        .filter(|path| core_run::is_git_config(path))
        .map(|path| (*path).to_owned())
        .collect();
    for path in names {
        let (dir, leaf) = split(path);
        match leaf.as_str() {
            // Git takes the folder for a repository when its `HEAD` reads as
            // one and it can enter both `objects` and `refs`, links followed
            // (`is_git_directory`): a folder whose `HEAD` cannot is no
            // repository, whatever else it holds, and neither is one whose
            // `objects` or `refs` it cannot enter — a project's own file of
            // that name. Where `HEAD` reads as one, a link there is refused.
            "HEAD" if headref(&root.join(path)) => {
                let mut entered = 0;
                for name in ["objects", "refs"] {
                    let at = joined(&dir, name);
                    match std::fs::symlink_metadata(root.join(&at)) {
                        Ok(meta) if meta.is_symlink() => return Err(linked(&at)),
                        Ok(_) => {
                            let access = rustix::fs::Access::EXEC_OK;
                            if rustix::fs::access(root.join(&at), access).is_ok() {
                                entered += 1;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(format!("workspace/{at} could not be read: {error}"))
                        }
                    }
                }
                if entered == 2 {
                    repositories.insert(dir);
                }
            }
            ".git" => {
                if link(path) {
                    return Err(linked(path));
                }
                // Not a pointer: git reads no repository from it.
                if let Some(pointer) = read(path)?.strip_prefix("gitdir:") {
                    repositories.insert(inside(&dir, Path::new(pointer.trim()), path)?);
                }
            }
            "commondir"
                if listed.contains(joined(&dir, "HEAD").as_str())
                    || dir.split('/').any(|part| part == ".git") =>
            {
                if link(path) {
                    return Err(linked(path));
                }
                repositories.insert(inside(&dir, Path::new(read(path)?.trim()), path)?);
            }
            _ => {}
        }
    }
    for repository in repositories {
        for leaf in ["config", "config.worktree"] {
            let path = joined(&repository, leaf);
            if listed.contains(path.as_str()) {
                configs.insert(path);
            }
        }
    }
    if let Some(config) = configs.iter().find(|config| link(config)) {
        return Err(linked(config));
    }
    Ok(configs)
}

/// Whether `head` reads as a repository's `HEAD` the way git reads one
/// (`validate_headref`): a link whose target starts `refs/`; or a file
/// starting `ref:` and then, after any space, `refs/`; or one starting with
/// an object id's 40 hex digits. One git cannot read is none, as for git.
fn headref(head: &Path) -> bool {
    if let Ok(target) = std::fs::read_link(head) {
        return target.as_os_str().as_encoded_bytes().starts_with(b"refs/");
    }
    let Ok(bytes) = std::fs::read(head) else {
        return false;
    };
    if let Some(named) = bytes.strip_prefix(b"ref:") {
        let named = named.trim_ascii_start();
        return named.starts_with(b"refs/");
    }
    bytes.len() >= 40 && bytes[..40].iter().all(u8::is_ascii_hexdigit)
}

/// A drive's checkout as a run reads it (R142): its entries, every
/// [`NEVER_MOUNTED`] folder left out at any depth — granted whole where
/// nothing below needs leaving out, entry by entry where something does —
/// and whether anything was left out. A link is never granted.
fn mount(dir: &Path) -> std::io::Result<(Vec<PathBuf>, bool)> {
    let mut grants = Vec::new();
    let mut excluded = false;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let never = entry
            .file_name()
            .to_str()
            .is_some_and(|name| NEVER_MOUNTED.contains(&name));
        if never || kind.is_symlink() {
            excluded = true;
            continue;
        }
        if kind.is_dir() {
            let (below, left_out) = mount(&entry.path())?;
            if left_out {
                excluded = true;
                grants.extend(below);
                continue;
            }
        }
        grants.push(entry.path());
    }
    Ok((grants, excluded))
}

/// The readers and `local_only` of each drive a run mounted, for its label.
pub fn mounted_labels(
    mounted: &[(String, PathBuf)],
    drives: &std::collections::BTreeMap<String, keeper_core::agents::drive::DriveDecl>,
) -> Vec<(BTreeSet<matrix_sdk::ruma::OwnedUserId>, bool)> {
    mounted
        .iter()
        .map(|(id, _)| {
            drives.get(id).map_or_else(
                || (BTreeSet::new(), false),
                |decl| (decl.readers.clone(), decl.local_only),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn host() -> SandboxHost {
        SandboxHost {
            kind: Kind::SandboxExec,
            host: "electra".to_owned(),
            table: SandboxTable::default(),
            forbidden: Forbidden::default(),
            status: String::new(),
        }
    }

    /// A sessions zone and one session in it: `(tempdir, zone, workspace)`.
    fn zone() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let zone = root
            .path()
            .canonicalize()
            .expect("canonical")
            .join("tgdrive/60-sessions");
        let workspace = zone.join("active/s/workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        (root, zone, workspace)
    }

    /// The session `active/s` of the drive holding `zone`, its sessions
    /// zone `60-sessions`.
    fn session(zone: &Path) -> Session<'_> {
        Session {
            drive: zone.parent().expect("the drive"),
            zone: "60-sessions",
            path: "active/s",
        }
    }

    /// R96R2-01, R231: the walk to `workspace/` starts at the drive and
    /// never follows the sessions zone's own path — the zone, or a folder
    /// of a nested zone above it, replaced by a link to another tree whose
    /// `active/s/workspace` is real is refused — while the real zone
    /// prepares.
    #[test]
    fn a_substituted_sessions_zone_is_refused() {
        let (root, zone, _) = zone();
        let drive = zone.parent().expect("drive").to_path_buf();
        let args = serde_json::json!({"argv": ["ls"]});
        let elsewhere = root.path().canonicalize().expect("c").join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("active/s/workspace")).expect("other");
        std::fs::create_dir_all(elsewhere.join("60-sessions/active/s/workspace")).expect("o");
        assert!(host().prepare(&args, session(&zone), &[]).is_ok());
        std::fs::rename(&zone, root.path().join("moved")).expect("move the zone");
        symlink(&elsewhere, &zone).expect("link");
        let refused = host()
            .prepare(&args, session(&zone), &[])
            .expect_err("a linked zone");
        assert!(refused.contains("not a real folder"), "{refused}");
        let nested = Session {
            drive: &drive,
            zone: "agents/60-sessions",
            path: "active/s",
        };
        std::fs::create_dir_all(drive.join("agents/60-sessions/active/s/workspace")).expect("n");
        assert!(host().prepare(&args, nested, &[]).is_ok());
        std::fs::rename(drive.join("agents"), root.path().join("agents")).expect("move");
        symlink(&elsewhere, drive.join("agents")).expect("link");
        let refused = host()
            .prepare(&args, nested, &[])
            .expect_err("a linked folder above the zone");
        assert!(refused.contains("not a real folder"), "{refused}");
    }

    /// R96R2-02, R231: a credential under the home that is a link is
    /// refused where it leads — a `read_exec` of its target, or of a
    /// folder holding that, refuses the run — while a folder beside it is
    /// granted.
    #[test]
    fn a_credential_link_is_refused_where_it_leads() {
        let (root, zone, workspace) = zone();
        let base = root.path().canonicalize().expect("c");
        let toolchain = base.join("opt/toolchain");
        std::fs::create_dir_all(toolchain.join("keys")).expect("keys");
        std::fs::write(toolchain.join("keys/id_ed25519"), "PRIVATE KEY").expect("key");
        std::fs::create_dir_all(base.join("opt/bin")).expect("bin");
        let home = base.join("home");
        std::fs::create_dir_all(&home).expect("home");
        symlink(toolchain.join("keys"), home.join(".ssh")).expect("link");
        let with = |read_exec: PathBuf| SandboxHost {
            table: SandboxTable {
                read_exec: vec![read_exec],
                env: Vec::new(),
            },
            forbidden: Forbidden {
                home: Some(home.clone()),
                ..Forbidden::default()
            }
            .canonical(),
            ..host()
        };
        let prepared = host()
            .prepare(&serde_json::json!({"argv": ["ls"]}), session(&zone), &[])
            .expect("prepared");
        for granted in [toolchain.clone(), toolchain.join("keys")] {
            let refused = with(granted.clone())
                .plan(&prepared, &workspace, &workspace)
                .expect_err("refused");
            assert!(refused.contains("keeper cannot grant it"), "{refused}");
        }
        assert!(with(base.join("opt/bin"))
            .plan(&prepared, &workspace, &workspace)
            .is_ok());
    }

    /// R96R3-05, R247: where a credential leads is read through the links
    /// a run would read through — `~/.ssh/keys` a link to a folder whose
    /// key is itself a link elsewhere: granting that last place refuses
    /// the run, a link cycle below a credential ends, and a credential
    /// folder keeper cannot list refuses every run — while a folder beside
    /// them all is granted.
    #[test]
    fn a_credential_behind_a_linked_folder_is_refused_and_one_unread_refuses_all() {
        let (root, zone, workspace) = zone();
        let base = root.path().canonicalize().expect("c");
        let store = base.join("opt/key-store");
        let private = base.join("usr/local/share");
        for dir in [&store, &private, &base.join("opt/bin")] {
            std::fs::create_dir_all(dir).expect("dir");
        }
        std::fs::write(private.join("private-key"), "PRIVATE KEY").expect("key");
        symlink(private.join("private-key"), store.join("id_ed25519")).expect("key link");
        let home = base.join("home");
        std::fs::create_dir_all(home.join(".ssh")).expect(".ssh");
        symlink(&store, home.join(".ssh/keys")).expect("folder link");
        symlink(home.join(".ssh"), home.join(".ssh/loop")).expect("cycle");
        let with = |read_exec: PathBuf| SandboxHost {
            table: SandboxTable {
                read_exec: vec![read_exec],
                env: Vec::new(),
            },
            forbidden: Forbidden {
                home: Some(home.clone()),
                ..Forbidden::default()
            }
            .canonical(),
            ..host()
        };
        let prepared = host()
            .prepare(&serde_json::json!({"argv": ["ls"]}), session(&zone), &[])
            .expect("prepared");
        let plan = |granted: PathBuf| with(granted).plan(&prepared, &workspace, &workspace);
        let refused = plan(private.clone()).expect_err("the key's own folder");
        assert!(refused.contains("private-key"), "{refused}");
        assert!(plan(base.join("opt/bin")).is_ok());
        let locked = home.join(".gnupg/private-keys-v1.d");
        std::fs::create_dir_all(&locked).expect("gnupg");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("lock");
        let refused = plan(base.join("opt/bin")).expect_err("an unread credential");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).expect("open");
        assert!(refused.contains("cannot be read"), "{refused}");
    }

    /// R96R3-07, R247: the walk to `workspace/` goes on from the folder it
    /// holds, never from a path: the sessions zone swapped for a link to
    /// another tree right after the walk checked it — between two of its
    /// steps — refuses the run and makes nothing in that tree, whether the
    /// session's folders there are absent or real; what the walk made is
    /// in the folder it held.
    #[test]
    fn a_zone_swapped_between_two_steps_is_refused_and_nothing_is_made_there() {
        for real_there in [false, true] {
            let root = tempfile::tempdir().expect("tempdir");
            let base = root.path().canonicalize().expect("c");
            let zone = base.join("tgdrive/60-sessions");
            std::fs::create_dir_all(&zone).expect("zone");
            let elsewhere = base.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).expect("elsewhere");
            if real_there {
                std::fs::create_dir_all(elsewhere.join("active/s/workspace")).expect("real");
            }
            let before = workspace_names(&elsewhere).expect("names");
            let mut swapped = false;
            let refused = walk_workspace(session(&zone), &mut |at| {
                if at == zone && !swapped {
                    std::fs::rename(&zone, base.join("moved")).expect("move the zone");
                    symlink(&elsewhere, &zone).expect("link");
                    swapped = true;
                }
            })
            .expect_err("refused");
            assert!(swapped);
            assert!(refused.contains("not a real folder"), "{refused}");
            let made = |dir: &Path| {
                let mut found = Vec::new();
                walk(dir, &mut |name, _, _| {
                    found.push(name);
                    Ok(())
                })
                .expect("walk");
                (found, dir.join("active/s/workspace").is_dir())
            };
            assert_eq!(made(&elsewhere), (before, real_there), "{real_there}");
            assert!(base.join("moved/active/s/workspace").is_dir());
        }
    }

    /// R96R2-07, R231: an approval binds `workspace/` and the folder `cwd`
    /// resolves to by device and inode — the same folder's name holding
    /// another folder is another binding.
    #[test]
    fn a_folder_replaced_at_its_path_is_another_binding() {
        let (root, zone, workspace) = zone();
        std::fs::create_dir_all(workspace.join("repo")).expect("repo");
        let args = serde_json::json!({"argv": ["ls"], "cwd": "repo"});
        let before = host().prepare(&args, session(&zone), &[]).expect("a");
        std::fs::rename(workspace.join("repo"), root.path().join("old")).expect("move");
        std::fs::create_dir(workspace.join("repo")).expect("again");
        let after = host().prepare(&args, session(&zone), &[]).expect("b");
        assert_eq!(before.exec_binding["cwd"], after.exec_binding["cwd"]);
        assert_ne!(before.exec_binding, after.exec_binding);
        assert_ne!(
            core_run::allowance_key(&before.exec_binding),
            core_run::allowance_key(&after.exec_binding)
        );
    }

    /// R96R2-05, R231: `ruby -C sub job.rb` reads `sub/job.rb`, not the
    /// `job.rb` its operand names from `cwd`: refused, as are its other
    /// spellings and perl's, while a script read from `cwd` is code the
    /// session holds.
    #[test]
    fn a_script_read_from_another_folder_is_refused() {
        let (_root, zone, workspace) = zone();
        std::fs::create_dir_all(workspace.join("sub")).expect("sub");
        for (name, text) in [
            ("job.rb", "puts 1\n"),
            ("sub/job.rb", "puts 2\n"),
            ("job.pl", "print 1;\n"),
            ("sub/job.pl", "print 2;\n"),
        ] {
            std::fs::write(workspace.join(name), text).expect("script");
        }
        let prepare = |argv: &[&str]| {
            host().prepare(&serde_json::json!({ "argv": argv }), session(&zone), &[])
        };
        for argv in [
            &["ruby", "-C", "sub", "job.rb"][..],
            &["ruby", "-Csub", "job.rb"],
            &["env", "-Csub", "ruby", "job.rb"],
            &["perl", "-xsub", "job.pl"],
            &["ruby", "-X", "sub", "job.rb"],
            &["ruby", "-Xsub", "job.rb"],
        ] {
            let refused = prepare(argv).expect_err("refused");
            assert!(
                refused.contains("keeper sets a run's"),
                "{argv:?}: {refused}"
            );
        }
        let held = prepare(&["perl", "-x", "job.pl"]).expect("prepared");
        assert_eq!(held.operands[0].path.as_deref(), Some("job.pl"));
        assert_eq!(held.operands[0].sha256, sha256_hex(b"print 1;\n"));
    }

    /// R96R2-06, R231: a repository's configuration is found where git
    /// finds it, whatever its folder is called — a separate git folder a
    /// `.git` file points at, and a bare repository — and one that runs a
    /// program is code the session holds; a `.git` pointing outside the
    /// workspace refuses the run.
    #[test]
    fn a_repository_found_as_git_finds_it_holds_its_configuration() {
        let (root, zone, workspace) = zone();
        for meta in ["meta", "bare.git"] {
            for dir in ["objects", "refs"] {
                std::fs::create_dir_all(workspace.join(meta).join(dir)).expect("dir");
            }
            std::fs::write(workspace.join(meta).join("HEAD"), "ref: refs/heads/main\n")
                .expect("HEAD");
            std::fs::write(
                workspace.join(meta).join("config"),
                "[core]\n\tbare = false\n",
            )
            .expect("config");
        }
        std::fs::create_dir_all(workspace.join("repo")).expect("repo");
        std::fs::write(workspace.join("repo/.git"), "gitdir: ../meta\n").expect("pointer");
        let prepare = || host().prepare(&serde_json::json!({"argv": ["ls"]}), session(&zone), &[]);
        assert!(!prepare().expect("plain").call_facts.held_code);
        for meta in ["meta", "bare.git"] {
            let config = workspace.join(meta).join("config");
            std::fs::write(&config, "[alias]\n\tx = !touch marker\n").expect("runs");
            let held = prepare().expect("held");
            let path = format!("{meta}/config");
            assert!(
                held.operands
                    .iter()
                    .any(|operand| operand.path.as_deref() == Some(path.as_str())),
                "{:?}",
                held.operands
            );
            std::fs::write(&config, "[core]\n\tbare = false\n").expect("plain");
        }
        // The bare repository loses `objects/`: no longer one git reads.
        std::fs::remove_dir(workspace.join("bare.git/objects")).expect("rm");
        std::fs::write(workspace.join("bare.git/config"), "[alias]\n\tx = !y\n").expect("c");
        assert!(!prepare().expect("not a repository").call_facts.held_code);
        let outside = root.path().canonicalize().expect("c").join("outside");
        std::fs::create_dir_all(&outside).expect("outside");
        std::fs::write(
            workspace.join("repo/.git"),
            format!("gitdir: {}\n", outside.display()),
        )
        .expect("pointer");
        let refused = prepare().expect_err("outside");
        assert!(refused.contains("outside the workspace"), "{refused}");
    }

    /// 96.1 #3: `cwd` lands where keeper-sync resolves it — a real symlink
    /// in `workspace/` to the drive's `.git` is refused, not followed.
    #[test]
    fn run_cwd_symlink_escape_is_refused() {
        let (root, zone, workspace) = zone();
        let drive = root.path().join("tgdrive");
        std::fs::create_dir_all(drive.join(".git")).expect(".git");
        std::fs::create_dir_all(workspace.join("src")).expect("src");
        symlink(drive.join(".git"), workspace.join("escape")).expect("link");
        symlink(workspace.join("src"), workspace.join("inside")).expect("link");
        let prepare = |cwd: &str| {
            host().prepare(
                &serde_json::json!({"argv": ["ls"], "cwd": cwd}),
                session(&zone),
                &[],
            )
        };
        let refused = prepare("escape").expect_err("refused");
        assert!(refused.contains("leads outside the workspace"), "{refused}");
        let refused = prepare("missing").expect_err("refused");
        assert!(refused.contains("is not a folder"), "{refused}");
        assert_eq!(prepare("src").expect("src").cwd, workspace.join("src"));
        assert_eq!(
            prepare("inside").expect("inside").cwd,
            workspace.join("src")
        );
        assert_eq!(prepare("").expect("root").cwd, workspace);
    }

    /// R96R-01: a link planted as `workspace/` itself, or as one of the
    /// session's folders above it, is refused — never followed to another
    /// place keeper would then grant read-write — while the real folders
    /// prepare.
    #[test]
    fn a_linked_workspace_or_session_folder_is_refused() {
        let (root, zone, workspace) = zone();
        let elsewhere = root.path().join("tgdrive/.git");
        std::fs::create_dir_all(&elsewhere).expect("elsewhere");
        let args = serde_json::json!({"argv": ["ls"]});
        assert!(host().prepare(&args, session(&zone), &[]).is_ok());
        std::fs::remove_dir(&workspace).expect("rm");
        symlink(&elsewhere, &workspace).expect("link");
        let refused = host()
            .prepare(&args, session(&zone), &[])
            .expect_err("refused");
        assert!(refused.contains("not a real folder"), "{refused}");
        std::fs::remove_file(&workspace).expect("rm link");
        let s = zone.join("active/s");
        std::fs::remove_dir(&s).expect("rm s");
        std::fs::create_dir_all(elsewhere.join("workspace")).expect("w");
        symlink(&elsewhere, &s).expect("link");
        let refused = host()
            .prepare(&args, session(&zone), &[])
            .expect_err("refused");
        assert!(refused.contains("not a real folder"), "{refused}");
    }

    /// R96R-10: an approval binds where `cwd` resolved, not its spelling:
    /// `selected` retargeted from one folder of the workspace to another is
    /// another binding.
    #[test]
    fn the_binding_names_the_folder_cwd_resolves_to() {
        let (_root, zone, workspace) = zone();
        for repo in ["repo-a", "repo-b"] {
            std::fs::create_dir_all(workspace.join(repo)).expect("repo");
        }
        symlink(workspace.join("repo-a"), workspace.join("selected")).expect("link");
        let args = serde_json::json!({"argv": ["ls"], "cwd": "selected"});
        let before = host().prepare(&args, session(&zone), &[]).expect("a");
        assert_eq!(before.exec_binding["cwd"], "repo-a");
        std::fs::remove_file(workspace.join("selected")).expect("rm");
        symlink(workspace.join("repo-b"), workspace.join("selected")).expect("link");
        let after = host().prepare(&args, session(&zone), &[]).expect("b");
        assert_eq!(after.exec_binding["cwd"], "repo-b");
        assert_ne!(before.exec_binding, after.exec_binding);
        // And the last check before the program starts sees the swap too.
        let mut plan = host().plan(&before, &workspace, &workspace).expect("plan");
        plan.cwd = workspace.join("repo-b");
        assert!(verify(&plan).is_err());
    }

    /// R96R-13: a name that is not UTF-8 or a FIFO in the workspace refuses
    /// the run instead of being left out of what it binds. (APFS refuses a
    /// name that is not UTF-8 itself, so that half is Linux's.)
    #[test]
    fn an_unbindable_workspace_entry_refuses() {
        use std::os::unix::ffi::OsStrExt;
        let (_root, zone, workspace) = zone();
        let args = serde_json::json!({"argv": ["ls"]});
        assert!(host().prepare(&args, session(&zone), &[]).is_ok());
        if cfg!(target_os = "linux") {
            let raw = workspace.join(std::ffi::OsStr::from_bytes(b"bad\xff"));
            std::fs::write(&raw, "x").expect("raw");
            let refused = host().prepare(&args, session(&zone), &[]).expect_err("raw");
            assert!(refused.contains("not UTF-8"), "{refused}");
            std::fs::remove_file(&raw).expect("rm");
        }
        let made = std::process::Command::new("mkfifo")
            .arg(workspace.join("pipe"))
            .status()
            .expect("mkfifo");
        assert!(made.success());
        let refused = host()
            .prepare(&args, session(&zone), &[])
            .expect_err("fifo");
        assert!(
            refused.contains("a pipe, a socket or a device"),
            "{refused}"
        );
    }

    /// 96.1 #2 on the disk: a program and a script in `workspace/`, its root
    /// dotfiles and a runnable hook are code the session holds, hashed; and
    /// (R96R-08, R96R-09) so is a program a wrapper runs from it, a script
    /// found from the folder `cwd` names, and a nested repository's
    /// configuration that runs a program — a plain one is not.
    #[test]
    fn code_the_session_holds_is_found_on_the_disk() {
        let (_root, zone, workspace) = zone();
        std::fs::create_dir_all(workspace.join("repo/.git/hooks")).expect("hooks");
        std::fs::create_dir_all(workspace.join("sub")).expect("sub");
        std::fs::write(workspace.join("tool"), "#!/bin/true\n").expect("tool");
        std::fs::set_permissions(
            workspace.join("tool"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("mode");
        std::fs::write(workspace.join("job.py"), "print(1)\n").expect("script");
        std::fs::write(workspace.join("sub/job.py"), "print(2)\n").expect("script");
        std::fs::write(workspace.join("repo/.git/hooks/pre-commit.sample"), "x").expect("sample");
        std::fs::write(
            workspace.join("repo/.git/config"),
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
        )
        .expect("config");
        let prepare = |argv: &[&str], cwd: &str| {
            host()
                .prepare(
                    &serde_json::json!({ "argv": argv, "cwd": cwd }),
                    session(&zone),
                    &[],
                )
                .expect("prepared")
        };
        let plain = prepare(&["ls"], "");
        assert!(plain.operands.is_empty(), "{:?}", plain.operands);
        assert!(!plain.call_facts.held_code);
        let tool = prepare(&["./tool"], "");
        assert_eq!(tool.facts.in_workspace[0].0, "tool");
        assert!(tool.call_facts.held_code);
        let wrapped = prepare(&["env", "./tool"], "");
        assert!(wrapped.call_facts.held_code, "{:?}", wrapped.operands);
        assert_eq!(
            wrapped.exec_binding["program"]["path"],
            workspace.join("tool").display().to_string()
        );
        let script = prepare(&["python3", "job.py"], "");
        assert_eq!(script.operands[0].path.as_deref(), Some("job.py"));
        assert_eq!(script.operands[0].sha256, sha256_hex(b"print(1)\n"));
        let nested = prepare(&["python3", "job.py"], "sub");
        assert_eq!(nested.operands[0].path.as_deref(), Some("sub/job.py"));
        std::fs::write(
            workspace.join("repo/.git/config"),
            "[core]\n\tfsmonitor = ./watch\n",
        )
        .expect("config");
        let configured = prepare(&["ls"], "");
        assert_eq!(
            configured.operands[0].path.as_deref(),
            Some("repo/.git/config"),
            "{:?}",
            configured.operands
        );
        std::fs::write(
            workspace.join("repo/.git/config"),
            "[core]\n\tbare = false\n",
        )
        .expect("c");
        std::fs::write(workspace.join("repo/.git/hooks/pre-commit"), "x").expect("hook");
        assert!(prepare(&["ls"], "").call_facts.held_code);
    }

    /// R96R3-03, R247: a `commondir` names a repository only among a
    /// repository's own files — beside a `HEAD`, or in a `.git/` — so a
    /// project's own file of that name, pointing at a repository's folder
    /// or holding any text, names nothing and refuses nothing, while a
    /// worktree's `commondir` makes the configuration it names code the
    /// session holds.
    #[test]
    fn only_a_repositorys_own_commondir_names_one() {
        let (_root, zone, workspace) = zone();
        for dir in ["store/objects", "store/refs", "notes", "admin"] {
            std::fs::create_dir_all(workspace.join(dir)).expect("dir");
        }
        std::fs::write(workspace.join("store/config"), "[alias]\n\tx = !touch m\n").expect("c");
        let prepare = || host().prepare(&serde_json::json!({"argv": ["ls"]}), session(&zone), &[]);
        for text in ["../store\n", "a note about common directories\n"] {
            std::fs::write(workspace.join("notes/commondir"), text).expect("note");
            let plain = prepare().expect("a project's file");
            assert!(
                !plain.call_facts.held_code,
                "{text:?}: {:?}",
                plain.operands
            );
        }
        std::fs::write(workspace.join("admin/HEAD"), "ref: refs/heads/w\n").expect("HEAD");
        std::fs::write(workspace.join("admin/commondir"), "../store\n").expect("pointer");
        let held = prepare().expect("a worktree's");
        assert!(
            held.operands
                .iter()
                .any(|operand| operand.path.as_deref() == Some("store/config")),
            "{:?}",
            held.operands
        );
    }

    /// R96R4-04, R260: a folder git cannot take for a repository — its
    /// `HEAD` not text git reads as one, or its `objects` and `refs`
    /// ordinary files git cannot enter — refuses nothing and holds no code,
    /// even beside a link; one whose `HEAD` git reads, with a link for its
    /// `objects` or `refs`, is refused; and one whose `objects` and `refs`
    /// git can enter, executable files though they are, is a repository
    /// whose configuration is held.
    #[test]
    fn a_folder_git_cannot_take_for_a_repository_refuses_nothing() {
        let (_root, zone, workspace) = zone();
        let fixture = workspace.join("fixture");
        std::fs::create_dir_all(&fixture).expect("fixture");
        std::fs::write(fixture.join("config"), "[alias]\n\tx = !touch m\n").expect("config");
        let prepare = || host().prepare(&serde_json::json!({"argv": ["ls"]}), session(&zone), &[]);
        let shape = |head: &str, mode: u32| {
            std::fs::write(fixture.join("HEAD"), head).expect("HEAD");
            for name in ["objects", "refs"] {
                let at = fixture.join(name);
                let _ = std::fs::remove_file(&at);
                std::fs::write(&at, "a project's own file\n").expect(name);
                std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode)).expect("mode");
            }
        };
        let held = |prepared: &Prepared| {
            prepared
                .operands
                .iter()
                .any(|operand| operand.path.as_deref() == Some("fixture/config"))
        };
        for (head, mode) in [
            ("the head of this fixture\n", 0o644),
            ("the head of this fixture\n", 0o755),
            ("ref: refs/heads/main\n", 0o644),
        ] {
            shape(head, mode);
            let plain = prepare().expect("no repository");
            assert!(!held(&plain), "{head:?} {mode:o}: {:?}", plain.operands);
        }
        shape("ref: refs/heads/main\n", 0o755);
        assert!(held(&prepare().expect("a repository")), "entered");
        std::fs::remove_file(fixture.join("objects")).expect("rm");
        std::fs::create_dir(workspace.join("store")).expect("store");
        symlink("../store", fixture.join("objects")).expect("link");
        let refused = prepare().expect_err("a link where git keeps objects");
        assert!(refused.contains("is a link where git keeps"), "{refused}");
        std::fs::write(fixture.join("HEAD"), "the head of this fixture\n").expect("HEAD");
        let plain = prepare().expect("no repository beside a link");
        assert!(!held(&plain), "{:?}", plain.operands);
    }

    /// R96R3-08, R247: a run too large to ride inline — T2 without network,
    /// T3 with — prepared on the disk, parked as keeper parks it, its
    /// payload moved out of the record and the request made as keeper
    /// makes it, is never approved unseen; once its file is checked it is
    /// shown as the run it binds, and approved; a file whose SHA-256 the
    /// request names but whose binding or workspace set is not the one
    /// digested is never shown, so never approved.
    #[test]
    fn a_large_run_is_decided_only_on_what_it_binds() {
        use keeper_core::agents::approval::{
            canonical as canonical_json, ApprovalRecord, CallRef, Checkpoint, Decision, Parking,
            Preconditions, Scope,
        };
        use keeper_core::agents::approval_card::{
            attached_payload, ApprovalDecideReq, ApprovalFold, Senders, Viewer, PAYLOAD_REFUSED,
            UNSEEN,
        };
        use keeper_core::agents::events::APPROVAL_REQUEST;
        use keeper_core::agents::label::{Integrity, Label, Readers};
        use keeper_core::agents::session::SessionKind;
        use keeper_core::agents::tier::{classify, AgentTool, Context, Tier};
        use matrix_sdk::ruma::{OwnedUserId, UserId};
        const PERSON: &str = "@tgorka:example.org";
        const AGENT: &str = "@nixi:example.org";
        let person = OwnedUserId::try_from(PERSON).expect("user");
        let agent = |user: &UserId| user.as_str() == AGENT;
        let senders = Senders {
            own: &person,
            agent: &agent,
            owner: &agent,
        };
        let viewer = Viewer {
            own: person.clone(),
            device_cross_signed: true,
            hosted: BTreeSet::new(),
        };
        let name = |user: &UserId| user.to_string();
        let label = Label {
            readers: Readers::Only([person.clone()].into()),
            ..Label::top()
        };
        let file = serde_json::json!({
            "url": "mxc://example.org/x",
            "key": {"kty": "oct", "key_ops": ["encrypt", "decrypt"], "alg": "A256CTR",
                "k": "aWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWk", "ext": true},
            "iv": "aWlpaWlpaWlpaWlpaWlpaQ",
            "hashes": {"sha256": "aWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWk"},
            "v": "v2",
        });
        let (_root, zone, workspace) = zone();
        std::fs::write(workspace.join("a.txt"), "released\n").expect("a file");
        let long = "d".repeat(20_000);
        for (network, tier) in [(false, Tier::T2), (true, Tier::T3)] {
            let args = serde_json::json!({"argv": ["cat", long], "network": network});
            let prepared = host()
                .prepare(&args, session(&zone), &[])
                .expect("prepared");
            let classification = classify(
                AgentTool::Run,
                &prepared.call_facts,
                &Context {
                    delegated: false,
                    unattended: false,
                    integrity: Integrity::Owner,
                    via_kvm: false,
                    grant: None,
                },
            );
            assert_eq!(classification.tier, tier);
            let id = format!("01JRUN{}", tier.as_u8());
            let mut record = ApprovalRecord::new(Parking {
                id: &id,
                created_at: chrono::Utc::now(),
                session: "60-sessions/active/s",
                session_kind: SessionKind::Conversation,
                agent: "nixi",
                drive: "tgdrive",
                host: "electra",
                epoch: 1,
                call: CallRef {
                    line: "01JLINE".to_owned(),
                    call_id: "r1".to_owned(),
                },
                dispatch_chain: vec![PERSON.to_owned()],
                checkpoint: Checkpoint {
                    chunk: "log/c.jsonl".to_owned(),
                    through: "01JLINE".to_owned(),
                    sha256: "c".repeat(64),
                },
                args: &args,
                exec_binding: prepared.exec_binding.clone(),
                classification: &classification,
                label: &label,
                preconditions: Preconditions {
                    workspace: prepared.workspace_set.clone(),
                    ..Preconditions::default()
                },
            })
            .expect("record");
            let (_, bytes) = record.externalise_args().expect("too large to ride inline");
            let request = crate::approvals::request_content(
                &record,
                "!session:example.org",
                vec![PERSON.to_owned()],
                Some(file.clone()),
            );
            let req = ApprovalDecideReq {
                id: id.clone(),
                binding_digest: request.binding_digest.clone(),
                decision: Decision::Approve,
                scope: Scope::Once,
                note: None,
            };
            let fold_of = |request: &keeper_core::agents::events::ApprovalRequestContent| {
                let mut fold = ApprovalFold::default();
                let event = serde_json::json!({"type": APPROVAL_REQUEST, "event_id": "$r",
                    "sender": AGENT, "origin_server_ts": 1,
                    "content": serde_json::to_value(request).expect("json")});
                assert!(fold.apply(&event, true, &senders));
                fold
            };
            let now = chrono::Utc::now();
            let fold = fold_of(&request);
            let unseen = fold.decide(&viewer, &req, now, &name, &|_| false);
            assert_eq!(unseen.err().as_deref(), Some(UNSEEN), "{tier:?}");
            let card = fold.record(&id).expect("its card");
            let run = attached_payload(card, bytes.as_bytes())
                .expect("shown")
                .run
                .expect("a run view");
            assert_eq!(run.argv, ["cat", long.as_str()]);
            assert_eq!(run.network.is_some(), network);
            let shown = |digest: &str| digest == request.binding_digest;
            assert!(
                fold.decide(&viewer, &req, now, &name, &shown).is_ok(),
                "{tier:?}"
            );
            let payload: Value = serde_json::from_str(&bytes).expect("payload");
            let mut tampered = payload.clone();
            tampered["exec_binding"]["exe_sha256"] = serde_json::json!("f".repeat(64));
            let mut forgeries = vec![tampered];
            if network {
                let mut wider = payload.clone();
                wider["workspace"]["files"]
                    .as_array_mut()
                    .expect("files")
                    .push(serde_json::json!({"path": "b.txt", "sha256": "b".repeat(64)}));
                forgeries.push(wider);
            }
            for forged in forgeries {
                let forged = canonical_json(&forged).expect("canonical");
                let mut request = request.clone();
                request.file_sha256 = Some(sha256_hex(forged.as_bytes()));
                let fold = fold_of(&request);
                let card = fold.record(&id).expect("its card");
                assert_eq!(
                    attached_payload(card, forged.as_bytes()).err().as_deref(),
                    Some(PAYLOAD_REFUSED),
                    "{tier:?}"
                );
                let refused = fold.decide(&viewer, &req, now, &name, &|_| false);
                assert_eq!(refused.err().as_deref(), Some(UNSEEN), "{tier:?}");
            }
        }
    }
}

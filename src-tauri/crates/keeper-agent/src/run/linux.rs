//! The Linux sandbox (D-33, R141): the trampoline's body. agentd re-executes
//! itself with [`super::TRAMPOLINE_ARG`] and a plan file before anything
//! else in `main` runs — one thread, no runtime — and this applies a
//! landlock ruleset and a seccomp filter to that process, then `exec`s the
//! program. Nothing here is `unsafe`, and nothing runs between `fork` and
//! `exec` in a multi-threaded parent.
//!
//! **Descriptors** (R213): only standard input, output and error reach the
//! program from what the trampoline inherited. Every other descriptor — one
//! agentd, a library it links or whatever started agentd left without
//! close-on-exec: a secret file, a connected socket, a lock — is closed
//! first (nix's safe `close`), and the run is refused if one is still open
//! after. The one other kind a program gets is the read-only handle of
//! each program a wrapper starts, below.
//!
//! **Landlock** (ABI ≥ 6, a hard requirement): every file-system right is
//! handled, and only the plan's grants are given — nothing under `/proc`,
//! no drive's `.git/` or `.keeper/`, no home, no secrets. `workspace/` and
//! `cwd` are opened as the folders keeper checked, by device and inode, and
//! the program starts in that `cwd`, so a link swapped in after the check
//! is not followed.
//!
//! **The program** (R231): it is opened once, its bytes are checked as read
//! through that handle inside the sandbox, and it is started by that handle
//! (nix's safe `execveat` of `/proc/self/fd/<n>`, through a link of the
//! program's own name beside the plan, outside every grant), so a program
//! put at its path after the last check is not the one that runs. A script (`#!`) is started by its
//! path: its interpreter opens it by name, which the descriptor cannot
//! give it under landlock — a script replaced in that window is DW-759.
//! Each program a wrapper starts (`env ./tool`, `env nice tool`: R247) is
//! linked the same way, and the wrapper is given that link in the
//! program's place in its argv: the wrapper still does all it does (sets
//! the environment, the priority, the time limit) and then starts the
//! program keeper checked, whatever is at its path by then. That program
//! is opened only once the sandbox holds (R260), so only a file the run's
//! own grants let it read is ever opened — one outside them refuses the
//! run — and its descriptor stays open across `exec` so the wrapper (and a
//! script's interpreter) can open it through the link: the program
//! inherits a read-only descriptor of each such program, a file its grants
//! let it read anyway.
//! Without network TCP bind and connect are handled and
//! never granted. Abstract Unix sockets and signals are scoped to the run's
//! own domain (R147), so it can neither reach a socket of agentd's nor
//! signal it.
//!
//! **Seccomp** ([`refused`], R147, R213): `socket(2)` is refused for every
//! family without network and for every family but `AF_INET`/`AF_INET6`
//! with it — a pathname Unix socket (D-Bus, an ssh agent, `docker.sock`) is
//! unreachable either way, `socketpair` still works. In every run the calls
//! that reach into another process (`ptrace`, `process_vm_*`, `kcmp`,
//! `pidfd_getfd`, `perf_event_open`, io_uring), the keyrings, System V IPC
//! and POSIX message queues, `bpf`, `personality`, namespaces (`unshare`,
//! `setns`, a `clone` with any `CLONE_NEW*` flag), leaving the process group
//! (`setsid`, `setpgid`), and every change to a file's metadata (the
//! `chmod`, `chown`, `*xattr` and `utime` families, `fchmod` on any
//! descriptor — landlock does not mediate them) fail with `EPERM`;
//! `clone3`, whose flags a filter cannot read, fails with `ENOSYS`, so the C
//! library falls back to `clone`. The x32 numbers of each are refused too.
//! The cost of the metadata rule: a program cannot set a mode or a time
//! even inside `workspace/` (`chmod +x`, `tar x`, `cp -p`).

use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::FileExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

use keeper_core::agents::run::{Access, SandboxPlan};
use landlock::{
    make_bitflags, Access as _, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible,
    LandlockStatus, PathBeneath, PathFd, RestrictionStatus, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, Scope, ABI,
};
use rustix::fs::{Mode, OFlags};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

/// The least landlock ABI a run is sandboxed under: 6 scopes abstract Unix
/// sockets and signals (R147).
pub const REQUIRED_ABI: ABI = ABI::V6;

/// `setxattrat` and `removexattrat` (Linux 6.13), newer than the C library's
/// headers: the same number on every architecture.
const SYS_SETXATTRAT: libc::c_long = 463;
const SYS_REMOVEXATTRAT: libc::c_long = 466;

/// The `CLONE_NEW*` flags: a `clone` holding any makes a namespace.
const CLONE_NAMESPACES: [u64; 8] = [
    libc::CLONE_NEWNS as u64,
    libc::CLONE_NEWCGROUP as u64,
    libc::CLONE_NEWUTS as u64,
    libc::CLONE_NEWIPC as u64,
    libc::CLONE_NEWUSER as u64,
    libc::CLONE_NEWPID as u64,
    libc::CLONE_NEWNET as u64,
    0x80, // CLONE_NEWTIME
];

/// Every call refused with `EPERM` in every run, whatever its arguments.
pub fn refused() -> Vec<libc::c_long> {
    let mut calls = vec![
        // Into another process.
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kcmp,
        libc::SYS_pidfd_getfd,
        libc::SYS_perf_event_open,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        // The keyrings, System V IPC, POSIX message queues.
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_msgget,
        libc::SYS_msgsnd,
        libc::SYS_msgrcv,
        libc::SYS_msgctl,
        libc::SYS_semget,
        libc::SYS_semop,
        libc::SYS_semtimedop,
        libc::SYS_semctl,
        libc::SYS_shmget,
        libc::SYS_shmat,
        libc::SYS_shmdt,
        libc::SYS_shmctl,
        libc::SYS_mq_open,
        libc::SYS_mq_unlink,
        libc::SYS_mq_timedsend,
        libc::SYS_mq_timedreceive,
        libc::SYS_mq_notify,
        libc::SYS_mq_getsetattr,
        // The kernel's own programs and personalities, namespaces.
        libc::SYS_bpf,
        libc::SYS_personality,
        libc::SYS_unshare,
        libc::SYS_setns,
        // Leaving the process group keeper kills whole.
        libc::SYS_setsid,
        libc::SYS_setpgid,
        // Metadata, which landlock does not mediate.
        libc::SYS_fchmod,
        libc::SYS_fchmodat,
        libc::SYS_fchmodat2,
        libc::SYS_fchown,
        libc::SYS_fchownat,
        libc::SYS_setxattr,
        libc::SYS_lsetxattr,
        libc::SYS_fsetxattr,
        SYS_SETXATTRAT,
        libc::SYS_removexattr,
        libc::SYS_lremovexattr,
        libc::SYS_fremovexattr,
        SYS_REMOVEXATTRAT,
        libc::SYS_utimensat,
    ];
    // The older spellings x86_64 still has.
    #[cfg(target_arch = "x86_64")]
    calls.extend([
        libc::SYS_chmod,
        libc::SYS_chown,
        libc::SYS_lchown,
        libc::SYS_utime,
        libc::SYS_utimes,
        libc::SYS_futimesat,
    ]);
    calls
}

/// The descriptors open in this process other than 0, 1 and 2, read from
/// `/proc/self/fd` (the listing's own, closed after it, is not counted).
fn inherited() -> Result<Vec<i32>, String> {
    let names: Vec<i32> = std::fs::read_dir("/proc/self/fd")
        .map_err(|error| format!("its descriptors could not be listed: {error}"))?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect();
    Ok(names
        .into_iter()
        .filter(|fd| *fd > 2 && std::fs::read_link(format!("/proc/self/fd/{fd}")).is_ok())
        .collect())
}

/// Close every descriptor but 0, 1 and 2 (R213) — one agentd or what started
/// it left without close-on-exec, a secret file or a connected socket, never
/// reaches the program — then look again: `Err` when one is still open. It
/// runs before landlock, which grants nothing of `/proc`; what the
/// trampoline opens after — the checked folders, the ruleset — is
/// close-on-exec. The numbers are listed first and the listing closed, so
/// its own handle is not among them.
fn close_inherited() -> Result<(), String> {
    for fd in inherited()? {
        // `EBADF` for a number already closed changes nothing; the look
        // below is what decides.
        let _ = nix::unistd::close(fd);
    }
    match inherited()?.as_slice() {
        [] => Ok(()),
        open => Err(format!(
            "descriptors {open:?} stayed open, and a run gets only its standard streams"
        )),
    }
}

/// The trampoline: read the plan at `plan_path` (then remove it), close
/// every inherited descriptor ([`close_inherited`]), open the program and
/// name its handle ([`program_link`]), keep and name a descriptor for each
/// program a wrapper starts ([`reserve`]), sandbox this process, open each
/// of those programs onto its descriptor inside the sandbox ([`hand`]),
/// check once more that what runs is what was checked, every program
/// through its handle ([`super::verify_program`]), say so on both streams
/// ([`super::sentinel`]) and become the program in its folder ([`exec`]).
/// It returns only when something failed, and the program then never ran.
pub fn trampoline(plan_path: &Path) -> ExitCode {
    let plan = std::fs::read(plan_path)
        .map_err(|error| format!("the plan could not be read: {error}"))
        .and_then(|bytes| {
            let _ = std::fs::remove_file(plan_path);
            serde_json::from_slice::<SandboxPlan>(&bytes)
                .map_err(|error| format!("the plan could not be read: {error}"))
        });
    let applied = plan.and_then(|mut plan| {
        close_inherited()?;
        let mut program = std::fs::File::open(&plan.exe)
            .map_err(|error| format!("its program could not be opened: {error}"))?;
        let link = program_link(plan_path, "program", plan.argv.first(), &program)?;
        let link = CString::new(link.into_os_string().into_vec())
            .map_err(|_| "its folder holds a NUL byte".to_owned())?;
        let reserved = reserve(plan_path, &mut plan)?;
        let (abi, cwd) = restrict(&plan)?;
        let mut behind = hand(&plan, reserved)?;
        apply_filter(plan.network)?;
        super::verify_program(&plan, &mut program, &mut behind)?;
        Ok((plan, abi, cwd, program, link, behind))
    });
    let (plan, abi, cwd, program, link, _behind) = match applied {
        Ok(applied) => applied,
        Err(reason) => {
            eprintln!("keeper: the sandbox could not be applied: {reason}");
            return ExitCode::from(super::NOT_SANDBOXED);
        }
    };
    if let Err(error) = rustix::process::fchdir(&cwd) {
        eprintln!("keeper: the sandbox could not be applied: its folder: {error}");
        return ExitCode::from(super::NOT_SANDBOXED);
    }
    drop(cwd);
    let said = super::sentinel(abi);
    for mut stream in [
        Box::new(std::io::stdout()) as Box<dyn Write>,
        Box::new(std::io::stderr()),
    ] {
        if stream
            .write_all(said.as_bytes())
            .and_then(|()| stream.flush())
            .is_err()
        {
            return ExitCode::from(super::NOT_SANDBOXED);
        }
    }
    let Some(name) = plan.argv.first() else {
        return ExitCode::from(super::NOT_SANDBOXED);
    };
    let error = exec(&plan, &program, &link);
    eprintln!("keeper: {name} could not be started: {error}");
    ExitCode::from(127)
}

/// A descriptor kept for each program a wrapper of `plan` starts
/// ([`Expected::programs`], R247) — `/dev/null`, until [`hand`] puts the
/// program there — a link to it made ([`program_link`]) and put in its
/// place in `plan`'s argv: the wrapper starts it through that link, so the
/// kernel opens what the descriptor holds. The links are made now, before
/// the sandbox, as nothing of the run's own folder is granted after.
///
/// [`Expected::programs`]: keeper_core::agents::run::Expected::programs
fn reserve(plan_path: &Path, plan: &mut SandboxPlan) -> Result<Vec<OwnedFd>, String> {
    let mut out = Vec::new();
    for (at, _, _) in plan.expect.programs.clone() {
        let kept = OwnedFd::from(
            std::fs::File::open("/dev/null")
                .map_err(|error| format!("a descriptor could not be kept: {error}"))?,
        );
        let link = program_link(
            plan_path,
            &format!("program-{at}"),
            plan.argv.get(at),
            &kept,
        )?
        .into_os_string()
        .into_string()
        .map_err(|_| "its folder's name is not UTF-8".to_owned())?;
        *plan
            .argv
            .get_mut(at)
            .ok_or_else(|| "its argv does not hold the program a wrapper runs".to_owned())? = link;
        out.push(kept);
    }
    Ok(out)
}

/// Each program a wrapper of `plan` starts, opened inside the sandbox and
/// put on the descriptor [`reserve`] kept for it, open across `exec`
/// (R260): what is opened is what the run's own grants let it read, at
/// that moment — a program outside every grant refuses the run, and is
/// never handed on, since a descriptor opened before the sandbox would
/// carry a file landlock never checks. The handles, in order, which
/// [`super::verify_program`] checks and which must stay open until the
/// program starts.
fn hand(plan: &SandboxPlan, reserved: Vec<OwnedFd>) -> Result<Vec<std::fs::File>, String> {
    plan.expect
        .programs
        .iter()
        .zip(reserved)
        .map(|((_, path, _), mut kept)| {
            let file = std::fs::File::open(path).map_err(|error| {
                format!(
                    "the program {} a wrapper starts is not one this run may read ({error}): keeper hands a wrapper no program outside the run's own grants",
                    path.display()
                )
            })?;
            // `dup2` leaves the kept number without close-on-exec.
            rustix::io::dup2(&file, &mut kept)
                .map_err(|error| format!("the program it runs could not be handed on: {error}"))?;
            Ok(std::fs::File::from(kept))
        })
        .collect()
}

/// A link named as `program` names itself to `file`'s descriptor
/// (`/proc/self/fd/<n>`), made before the sandbox in a folder `folder` of
/// the run's own beside its plan — outside every grant, so the program
/// never sees it and its `TMPDIR` stays empty. Started through it, the
/// kernel opens what the descriptor holds, never what is at the program's
/// path now, and a multi-call program that reads its name from how it was
/// started (uutils' coreutils, busybox) still finds it, which
/// `AT_EMPTY_PATH` — named `/dev/fd/<n>` — would hide.
fn program_link(
    plan_path: &Path,
    folder: &str,
    program: Option<&String>,
    file: &impl AsRawFd,
) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let name = program
        .map(|program| keeper_core::agents::run::base_name(program))
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("program");
    let named = plan_path
        .parent()
        .ok_or_else(|| "its plan has no folder".to_owned())?
        .join(folder);
    let link = named.join(name);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&named)
        .and_then(|()| {
            std::os::unix::fs::symlink(format!("/proc/self/fd/{}", file.as_raw_fd()), &link)
        })
        .map_err(|error| format!("its program's handle could not be named: {error}"))?;
    Ok(link)
}

/// Become `plan`'s program, `program` the handle its bytes were checked
/// through (R231): `execveat` of `link`, its [`program_link`]. A script
/// (`#!`) is started by its path: its interpreter opens it by name once the
/// descriptor is closed (DW-759). Returns only why it failed.
fn exec(plan: &SandboxPlan, program: &std::fs::File, link: &CString) -> String {
    let mut head = [0u8; 2];
    let script = program.read_exact_at(&mut head, 0).is_ok() && head == *b"#!";
    if script {
        let Some((name, args)) = plan.argv.split_first() else {
            return "it has no argv".to_owned();
        };
        return Command::new(&plan.exe)
            .arg0(name)
            .args(args)
            .env_clear()
            .envs(plan.env.iter().map(|(name, value)| (name, value)))
            .exec()
            .to_string();
    }
    let strings = |items: Vec<String>| {
        items
            .into_iter()
            .map(CString::new)
            .collect::<Result<Vec<_>, _>>()
    };
    let env = plan
        .env
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    let (Ok(args), Ok(env)) = (strings(plan.argv.clone()), strings(env)) else {
        return "an argument or a variable holds a NUL byte".to_owned();
    };
    match nix::unistd::execveat(
        nix::fcntl::AT_FDCWD,
        link,
        &args,
        &env,
        nix::fcntl::AtFlags::empty(),
    ) {
        Ok(never) => match never {},
        Err(error) => error.to_string(),
    }
}

/// The rights a grant gives, cut to what a file can carry when it is one.
fn rights(path: &Path, access: &Access) -> BitFlags<AccessFs> {
    let wanted = match access {
        Access::ReadWrite => AccessFs::from_all(REQUIRED_ABI),
        Access::ReadExec => AccessFs::from_read(REQUIRED_ABI),
        Access::Read => make_bitflags!(AccessFs::{ReadFile | ReadDir}),
    };
    if path.is_dir() {
        wanted
    } else {
        wanted & AccessFs::from_file(REQUIRED_ABI)
    }
}

/// `path` opened as the folder whose device and inode are `id`, its last
/// component never a link.
fn checked_dir(path: &Path, id: (u64, u64), what: &str) -> Result<OwnedFd, String> {
    let fd = rustix::fs::open(
        path,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("{what} could not be opened: {error}"))?;
    let stat = rustix::fs::fstat(&fd).map_err(|error| format!("{what}: {error}"))?;
    if (stat.st_dev, stat.st_ino) != id {
        return Err(format!("{what} changed after keeper checked it"));
    }
    Ok(fd)
}

/// Restrict this process to `plan`'s grants — `workspace/` the folder keeper
/// checked — and open its `cwd` the same way: the kernel's landlock ABI and
/// the folder the program starts in.
fn restrict(plan: &SandboxPlan) -> Result<(i32, OwnedFd), String> {
    let failed = |error: landlock::RulesetError| format!("landlock: {error}");
    let workspace = checked_dir(
        &plan.expect.workspace,
        plan.expect.workspace_id,
        "its workspace",
    )?;
    let cwd = checked_dir(&plan.cwd, plan.expect.cwd_id, "its folder")?;
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(REQUIRED_ABI))
        .map_err(failed)?
        .scope(Scope::from_all(REQUIRED_ABI))
        .map_err(failed)?;
    if !plan.network {
        ruleset = ruleset
            .handle_access(AccessNet::from_all(REQUIRED_ABI))
            .map_err(failed)?;
    }
    let mut created = ruleset.create().map_err(failed)?;
    let mut workspace = Some(workspace);
    for (path, access) in &plan.grants {
        let rule = match workspace.take_if(|_| *path == plan.expect.workspace) {
            Some(fd) => PathBeneath::new(fd, rights(path, access)),
            None => {
                let fd = PathFd::new(path)
                    .map_err(|error| format!("{} could not be opened: {error}", path.display()))?;
                created = created
                    .add_rule(PathBeneath::new(fd, rights(path, access)))
                    .map_err(failed)?;
                continue;
            }
        };
        created = created.add_rule(rule).map_err(failed)?;
    }
    let status: RestrictionStatus = created.restrict_self().map_err(failed)?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err("landlock could not enforce every rule".to_owned());
    }
    match status.landlock {
        LandlockStatus::Available {
            effective_abi,
            kernel_abi,
        } => Ok((kernel_abi.unwrap_or(effective_abi as i32), cwd)),
        _ => Err("landlock is not available".to_owned()),
    }
}

/// The filters for a run with or without `network`, in the order they are
/// installed: the first refuses with `EPERM` (sockets, [`refused`], a
/// namespace-making `clone`), the second `clone3` with `ENOSYS`.
pub fn filters(network: bool) -> Result<[BpfProgram; 2], String> {
    let failed = |error: seccompiler::BackendError| format!("seccomp: {error}");
    let arch = TargetArch::try_from(std::env::consts::ARCH)
        .map_err(|error| format!("seccomp: {error}"))?;
    let socket_rules = if network {
        vec![SeccompRule::new(vec![
            SeccompCondition::new(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::Ne,
                libc::AF_INET as u64,
            )
            .map_err(failed)?,
            SeccompCondition::new(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::Ne,
                libc::AF_INET6 as u64,
            )
            .map_err(failed)?,
        ])
        .map_err(failed)?]
    } else {
        Vec::new()
    };
    let namespaces = CLONE_NAMESPACES
        .iter()
        .map(|flag| {
            SeccompCondition::new(
                0,
                SeccompCmpArgLen::Qword,
                SeccompCmpOp::MaskedEq(*flag),
                *flag,
            )
            .and_then(|condition| SeccompRule::new(vec![condition]))
            .map_err(failed)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    rules.insert(libc::SYS_socket, socket_rules);
    rules.insert(libc::SYS_clone, namespaces);
    for call in refused() {
        rules.insert(call, Vec::new());
    }
    let mut clone3: BTreeMap<i64, Vec<SeccompRule>> =
        BTreeMap::from([(libc::SYS_clone3, Vec::new())]);
    if arch == TargetArch::x86_64 {
        // x32's numbers: the same call with bit 30 set, and its own
        // numbers for the calls whose arguments differ there — `ptrace`
        // 521, `mq_notify` 527, `process_vm_readv`/`writev` 539 and 540.
        const X32: i64 = 0x4000_0000;
        for table in [&mut rules, &mut clone3] {
            let numbered: Vec<(i64, Vec<SeccompRule>)> = table
                .iter()
                .map(|(call, rules)| (call | X32, rules.clone()))
                .collect();
            table.extend(numbered);
        }
        for call in [521, 527, 539, 540] {
            rules.insert(X32 | call, Vec::new());
        }
    }
    let program = |rules, errno: i32| {
        SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(errno as u32),
            arch,
        )
        .and_then(BpfProgram::try_from)
        .map_err(failed)
    };
    Ok([program(rules, libc::EPERM)?, program(clone3, libc::ENOSYS)?])
}

fn apply_filter(network: bool) -> Result<(), String> {
    for program in filters(network)? {
        seccompiler::apply_filter(&program).map_err(|error| format!("seccomp: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AUDIT_ARCH_X86_64` and `AUDIT_ARCH_I386`, `AUDIT_ARCH_AARCH64`.
    const X86_64: u32 = 0xc000_003e;
    const I386: u32 = 0x4000_0003;
    const AARCH64: u32 = 0xc000_00b7;

    fn native() -> u32 {
        if cfg!(target_arch = "x86_64") {
            X86_64
        } else {
            AARCH64
        }
    }

    /// What the kernel does with `program` for a call `nr` on `arch` with
    /// `args`: classic BPF over `seccomp_data`, the instructions seccompiler
    /// emits — any other fails the test.
    fn verdict(program: &BpfProgram, arch: u32, nr: i64, args: [u64; 6]) -> u32 {
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(&(nr as u32).to_le_bytes());
        data[4..8].copy_from_slice(&arch.to_le_bytes());
        for (i, arg) in args.iter().enumerate() {
            data[16 + 8 * i..24 + 8 * i].copy_from_slice(&arg.to_le_bytes());
        }
        let word = |at: u32| {
            let at = at as usize;
            u32::from_le_bytes(data[at..at + 4].try_into().expect("a word"))
        };
        let (mut pc, mut acc) = (0usize, 0u32);
        loop {
            let op = &program[pc];
            pc += 1;
            let jump = |taken: bool| if taken { op.jt } else { op.jf } as usize;
            match op.code {
                0x20 => acc = word(op.k),        // LD | W | ABS
                0x54 => acc &= op.k,             // ALU | AND | K
                0x05 => pc += op.k as usize,     // JMP | JA
                0x15 => pc += jump(acc == op.k), // JMP | JEQ | K
                0x25 => pc += jump(acc > op.k),  // JMP | JGT | K
                0x35 => pc += jump(acc >= op.k), // JMP | JGE | K
                0x06 => return op.k,             // RET | K
                code => panic!("an instruction this test does not read: {code:#x}"),
            }
        }
    }

    const ALLOW: u32 = 0x7fff_0000;
    const KILL_PROCESS: u32 = 0x8000_0000;
    const ERRNO: u32 = 0x0005_0000;

    /// Every run's first filter, then its second, as the kernel combines
    /// them: the first verdict that is not allow.
    fn combined(network: bool, arch: u32, nr: i64, args: [u64; 6]) -> u32 {
        let [first, second] = filters(network).expect("the filters");
        match verdict(&first, arch, nr, args) {
            ALLOW => verdict(&second, arch, nr, args),
            other => other,
        }
    }

    /// What every run must refuse with `EPERM` (R147, R213, R231), named
    /// and numbered here — never read from [`refused`] — so a rule removed
    /// from the filter fails this test: x86_64's numbers written out, and
    /// x32's for the calls whose x32 number is its own.
    #[cfg(target_arch = "x86_64")]
    const REQUIRED: &[(&str, i64)] = &[
        ("ptrace", 101),
        ("process_vm_readv", 310),
        ("process_vm_writev", 311),
        ("kcmp", 312),
        ("pidfd_getfd", 438),
        ("perf_event_open", 298),
        ("io_uring_setup", 425),
        ("io_uring_enter", 426),
        ("io_uring_register", 427),
        ("keyctl", 250),
        ("add_key", 248),
        ("request_key", 249),
        ("msgget", 68),
        ("msgsnd", 69),
        ("msgrcv", 70),
        ("msgctl", 71),
        ("semget", 64),
        ("semop", 65),
        ("semtimedop", 220),
        ("semctl", 66),
        ("shmget", 29),
        ("shmat", 30),
        ("shmdt", 67),
        ("shmctl", 31),
        ("mq_open", 240),
        ("mq_unlink", 241),
        ("mq_timedsend", 242),
        ("mq_timedreceive", 243),
        ("mq_notify", 244),
        ("mq_getsetattr", 245),
        ("bpf", 321),
        ("personality", 135),
        ("unshare", 272),
        ("setns", 308),
        ("setsid", 112),
        ("setpgid", 109),
        ("fchmod", 91),
        ("fchmodat", 268),
        ("fchmodat2", 452),
        ("fchown", 93),
        ("fchownat", 260),
        ("setxattr", 188),
        ("lsetxattr", 189),
        ("fsetxattr", 190),
        ("setxattrat", 463),
        ("removexattr", 197),
        ("lremovexattr", 198),
        ("fremovexattr", 199),
        ("removexattrat", 466),
        ("utimensat", 280),
        ("chmod", 90),
        ("chown", 92),
        ("lchown", 94),
        ("utime", 132),
        ("utimes", 235),
        ("futimesat", 261),
    ];
    /// x32's own numbers of the calls above whose x32 number differs.
    #[cfg(target_arch = "x86_64")]
    const X32_OWN: &[(&str, i64)] = &[
        ("ptrace", 521),
        ("mq_notify", 527),
        ("process_vm_readv", 539),
        ("process_vm_writev", 540),
    ];
    #[cfg(target_arch = "aarch64")]
    const REQUIRED: &[(&str, i64)] = &[
        ("ptrace", libc::SYS_ptrace),
        ("process_vm_readv", libc::SYS_process_vm_readv),
        ("process_vm_writev", libc::SYS_process_vm_writev),
        ("kcmp", libc::SYS_kcmp),
        ("pidfd_getfd", libc::SYS_pidfd_getfd),
        ("perf_event_open", libc::SYS_perf_event_open),
        ("io_uring_setup", libc::SYS_io_uring_setup),
        ("io_uring_enter", libc::SYS_io_uring_enter),
        ("io_uring_register", libc::SYS_io_uring_register),
        ("keyctl", libc::SYS_keyctl),
        ("add_key", libc::SYS_add_key),
        ("request_key", libc::SYS_request_key),
        ("msgget", libc::SYS_msgget),
        ("msgsnd", libc::SYS_msgsnd),
        ("msgrcv", libc::SYS_msgrcv),
        ("msgctl", libc::SYS_msgctl),
        ("semget", libc::SYS_semget),
        ("semop", libc::SYS_semop),
        ("semtimedop", libc::SYS_semtimedop),
        ("semctl", libc::SYS_semctl),
        ("shmget", libc::SYS_shmget),
        ("shmat", libc::SYS_shmat),
        ("shmdt", libc::SYS_shmdt),
        ("shmctl", libc::SYS_shmctl),
        ("mq_open", libc::SYS_mq_open),
        ("mq_unlink", libc::SYS_mq_unlink),
        ("mq_timedsend", libc::SYS_mq_timedsend),
        ("mq_timedreceive", libc::SYS_mq_timedreceive),
        ("mq_notify", libc::SYS_mq_notify),
        ("mq_getsetattr", libc::SYS_mq_getsetattr),
        ("bpf", libc::SYS_bpf),
        ("personality", libc::SYS_personality),
        ("unshare", libc::SYS_unshare),
        ("setns", libc::SYS_setns),
        ("setsid", libc::SYS_setsid),
        ("setpgid", libc::SYS_setpgid),
        ("fchmod", libc::SYS_fchmod),
        ("fchmodat", libc::SYS_fchmodat),
        ("fchmodat2", libc::SYS_fchmodat2),
        ("fchown", libc::SYS_fchown),
        ("fchownat", libc::SYS_fchownat),
        ("setxattr", libc::SYS_setxattr),
        ("lsetxattr", libc::SYS_lsetxattr),
        ("fsetxattr", libc::SYS_fsetxattr),
        ("setxattrat", 463),
        ("removexattr", libc::SYS_removexattr),
        ("lremovexattr", libc::SYS_lremovexattr),
        ("fremovexattr", libc::SYS_fremovexattr),
        ("removexattrat", 466),
        ("utimensat", libc::SYS_utimensat),
    ];

    /// R147, R213, R96R-22, R96R2-12: the generated filters themselves,
    /// call by call, against [`REQUIRED`] — a list of its own, so a rule
    /// removed from the filter is a test that fails here, whatever the
    /// host's own filter already refuses. Every required call gets `EPERM`
    /// with and without network, on x86_64 its x32 number too (bit 30, or
    /// x32's own); `clone3` gets `ENOSYS`; a `clone` refuses each namespace
    /// flag and lets a thread or a process be made; `socket` keeps IP only
    /// with network; a call of another architecture kills the process.
    #[test]
    fn the_filter_refuses_each_rule_and_nothing_else() {
        let eperm = ERRNO | libc::EPERM as u32;
        for network in [false, true] {
            for (name, call) in REQUIRED {
                assert_eq!(
                    combined(network, native(), *call, [0; 6]),
                    eperm,
                    "{name} ({call})"
                );
                #[cfg(target_arch = "x86_64")]
                {
                    let own = X32_OWN.iter().find(|(own, _)| own == name);
                    let x32 = 0x4000_0000 | own.map_or(*call, |(_, number)| *number);
                    assert_eq!(
                        combined(network, X86_64, x32, [0; 6]),
                        eperm,
                        "x32 {name} ({x32:#x})"
                    );
                }
            }
            assert_eq!(
                combined(network, native(), libc::SYS_clone3, [0; 6]),
                ERRNO | libc::ENOSYS as u32
            );
            // CLONE_NEWNS, NEWCGROUP, NEWUTS, NEWIPC, NEWUSER, NEWPID,
            // NEWNET and NEWTIME, written out.
            for flag in [
                0x0002_0000u64,
                0x0200_0000,
                0x0400_0000,
                0x0800_0000,
                0x1000_0000,
                0x2000_0000,
                0x4000_0000,
                0x80,
            ] {
                assert_eq!(
                    combined(
                        network,
                        native(),
                        libc::SYS_clone,
                        [flag | 0x11, 0, 0, 0, 0, 0]
                    ),
                    eperm,
                    "clone {flag:#x}"
                );
            }
            let thread = (libc::CLONE_VM
                | libc::CLONE_FS
                | libc::CLONE_FILES
                | libc::CLONE_SIGHAND
                | libc::CLONE_THREAD) as u64;
            assert_eq!(
                combined(network, native(), libc::SYS_clone, [thread, 0, 0, 0, 0, 0]),
                ALLOW
            );
            assert_eq!(
                combined(network, native(), libc::SYS_clone, [0x11, 0, 0, 0, 0, 0]),
                ALLOW
            );
            for allowed in [
                libc::SYS_read,
                libc::SYS_write,
                libc::SYS_openat,
                libc::SYS_execve,
                libc::SYS_socketpair,
                libc::SYS_kill,
                libc::SYS_getpgid,
                libc::SYS_setuid,
            ] {
                assert_eq!(
                    combined(network, native(), allowed, [0; 6]),
                    ALLOW,
                    "call {allowed}"
                );
            }
            let socket = |family: i32| {
                combined(
                    network,
                    native(),
                    libc::SYS_socket,
                    [family as u64, 1, 0, 0, 0, 0],
                )
            };
            assert_eq!(socket(libc::AF_UNIX), eperm);
            assert_eq!(socket(libc::AF_NETLINK), eperm);
            for family in [libc::AF_INET, libc::AF_INET6] {
                assert_eq!(socket(family), if network { ALLOW } else { eperm });
            }
        }
        if cfg!(target_arch = "x86_64") {
            assert_eq!(
                combined(false, I386, libc::SYS_read, [0; 6]),
                KILL_PROCESS,
                "a foreign architecture's call"
            );
        }
    }
}

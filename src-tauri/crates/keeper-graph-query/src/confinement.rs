use rustix::process::{Resource, Rlimit};
#[cfg(target_os = "linux")]
use std::path::Path;
type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

pub fn close_inherited() -> Result<()> {
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    #[cfg(target_os = "macos")]
    let directory = "/dev/fd";
    match std::fs::read_dir(directory) {
        Ok(entries) => {
            let mut descriptors = Vec::new();
            for entry in entries {
                let name = entry?.file_name();
                if let Some(fd) = name.to_str().and_then(|s| s.parse::<i32>().ok()) {
                    if descriptors.len() >= 4096 {
                        return Err("descriptor enumeration limit".into());
                    }
                    if fd >= 3 {
                        descriptors.push(fd);
                    }
                }
            }
            // Iterator and directory handle are gone before any raw close.
            for fd in descriptors {
                close(fd)?;
            }
        }
        Err(error) => {
            #[cfg(target_os = "linux")]
            return Err(error.into());
            #[cfg(target_os = "macos")]
            {
                let _ = error;
                let ceiling = rustix::process::getrlimit(Resource::Nofile)
                    .maximum
                    .ok_or("unbounded descriptor limit")?;
                if ceiling > 1_048_576 {
                    return Err("descriptor limit too large".into());
                }
                for fd in 3..ceiling {
                    close(i32::try_from(fd)?)?;
                }
            }
        }
    }
    Ok(())
}
fn close(fd: i32) -> Result<()> {
    match nix::unistd::close(fd) {
        Ok(()) | Err(nix::errno::Errno::EBADF) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn limit_resources() -> Result<()> {
    rustix::process::setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )?;
    #[cfg(target_os = "linux")]
    {
        rustix::process::setrlimit(
            Resource::As,
            Rlimit {
                current: Some(512 * 1024 * 1024),
                maximum: Some(512 * 1024 * 1024),
            },
        )?;
        rustix::process::setrlimit(
            Resource::Stack,
            Rlimit {
                current: Some(8 * 1024 * 1024),
                maximum: Some(8 * 1024 * 1024),
            },
        )?;
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub fn restrict(path: &Path, writable: bool) -> Result<()> {
    use landlock::{
        Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, ABI,
    };
    use nix::libc;
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};
    use std::collections::BTreeMap;
    let abi = ABI::V6;
    let mut access: landlock::BitFlags<AccessFs> = AccessFs::ReadFile.into();
    if writable {
        access |= AccessFs::WriteFile | AccessFs::Truncate;
    }
    let status = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))?
        .handle_access(AccessNet::from_all(abi))?
        .scope(Scope::from_all(abi))?
        .create()?
        .add_rule(PathBeneath::new(PathFd::new(path)?, access))?
        .restrict_self()?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err("confinement not fully enforced".into());
    }
    let kill: BTreeMap<_, _> = [
        libc::SYS_clone,
        libc::SYS_clone3,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_fork,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_vfork,
    ]
    .into_iter()
    .map(|n| (n, Vec::new()))
    .collect();
    let filter: BpfProgram = SeccompFilter::new(
        kill,
        SeccompAction::Allow,
        SeccompAction::KillProcess,
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()?;
    seccompiler::apply_filter(&filter)?;
    let allowed = [
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_close,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_lseek,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_openat,
        libc::SYS_fcntl,
        libc::SYS_fsync,
        libc::SYS_fdatasync,
        libc::SYS_ftruncate,
        libc::SYS_fchmod,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mremap,
        libc::SYS_mprotect,
        libc::SYS_brk,
        libc::SYS_madvise,
        libc::SYS_clock_gettime,
        libc::SYS_gettimeofday,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getrandom,
        libc::SYS_getrusage,
        libc::SYS_futex,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_sched_yield,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_readlinkat,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_readlink,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_stat,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_lstat,
    ]
    .into_iter()
    .map(|n| (n, Vec::new()))
    .collect();
    let filter: BpfProgram = SeccompFilter::new(
        allowed,
        SeccompAction::Errno(libc::EPERM as u32),
        SeccompAction::Allow,
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()?;
    seccompiler::apply_filter(&filter)?;
    Ok(())
}

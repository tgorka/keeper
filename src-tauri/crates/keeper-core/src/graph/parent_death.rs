//! The one `unsafe` item keeper-core carries (owner amendment 2026-11-13;
//! inventory row K01 in docs/constraints-and-limitations.md; this file's
//! bytes are pinned by `scripts/check-graph-boundary.py`): a graph worker
//! dies with the process that spawned it.

use std::io;

use rustix::io::Errno;
use rustix::process::{getpid, getppid, set_parent_process_death_signal, Signal};

/// Make `command`'s child receive SIGKILL when its parent dies, so a worker
/// never outlives a driver or agentd ended by a signal it cannot take
/// (SIGKILL). The kernel's parent is the spawning *thread*, so callers spawn
/// from the process-lifetime spawner thread (`runtime::spawner`).
///
/// A child whose parent already died between the fork and the `prctl` (the
/// signal would then never come) does not exec: its spawn fails and it
/// exits. `exec` keeps the setting: the worker is neither set-id nor
/// file-capable (the ledger's `rootfs.setid` and capability checks).
#[allow(unsafe_code)]
pub(crate) fn die_with_parent(command: &mut tokio::process::Command) {
    let parent = getpid();
    // SAFETY: `CommandExt::pre_exec`'s contract: the closure runs in the
    // forked child before `exec`, where only async-signal-safe work is
    // sound, and must not use state the parent's other threads may have
    // held at the fork. It makes two system calls through rustix
    // (`prctl(PR_SET_PDEATHSIG, SIGKILL)` and `getppid`, both
    // async-signal-safe) and may build an `io::Error` from a raw errno,
    // which allocates nothing. It takes no lock, logs nothing and captures
    // only a `Copy` pid. On `Err`, std hands the errno to the parent and the
    // child `_exit`s without `exec`.
    unsafe {
        command.pre_exec(move || {
            set_parent_process_death_signal(Some(Signal::KILL))?;
            if getppid() != Some(parent) {
                return Err(io::Error::from(Errno::SRCH));
            }
            Ok(())
        });
    }
}

//! The macOS sandbox (D-33): `/usr/bin/sandbox-exec -p <profile> -- <exe>
//! <args>`, the profile [`keeper_core::agents::run::sbpl`] wrote for the
//! plan. `sandbox-exec` applies it before it `exec`s the program, so nothing
//! of the program runs outside it; if Apple removes the tool, the probe
//! fails and the host stops offering `sandbox` — a run never falls back to
//! running unsandboxed.
//!
//! **Descendants** (R213, R231): a profile cannot refuse `setsid`, so a
//! process of a run can leave its group, and the Mac gives keeper no
//! descendant identity a process cannot shed without privileges keeper
//! does not hold. The Mac's bound is two sweeps. Before the group is
//! killed, [`strays`] kills every process that still descends from the
//! run's first process by parent but left its group; once the group is
//! killed, [`sweep`] kills every process whose environment still names
//! the run's own `TMPDIR` — which macOS 27 no longer shows for every
//! program (a system binary's, such as `/bin/sleep`'s, reads empty to
//! `ps -E` and `kern.procargs2`) — and the run's pipes are drained for a
//! bounded time either way. A process whose parent had already ended and
//! whose environment the Mac does not show, or that also rewrote its
//! environment, escapes both and stays inside the same profile, keeping
//! every grant the run had — the workspace read and write, and for a run
//! without network the drives it asked to read, read-only; no secret, no
//! other drive, no network the run did not have (DW-757, accepted by
//! R231; R247) — and the run's result says what was swept
//! ([`keeper_core::agents::run::Reaped::Swept`]), never that every
//! process ended.

use std::path::Path;
use std::process::Command;

use keeper_core::agents::run::{sbpl, SandboxPlan};

/// Where the tool is; never looked up on a `PATH`.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The command that runs `plan` sandboxed.
pub fn command(plan: &SandboxPlan) -> Command {
    let mut command = Command::new(SANDBOX_EXEC);
    command
        .arg("-p")
        .arg(sbpl(plan))
        .arg("--")
        .arg(&plan.exe)
        .args(plan.argv.iter().skip(1))
        .env_clear()
        .envs(plan.env.iter().map(|(name, value)| (name, value)))
        .current_dir(&plan.cwd);
    command
}

/// Whether the tool is there to probe.
pub fn present() -> bool {
    Path::new(SANDBOX_EXEC).is_file()
}

/// Kill every process that descends from `root` — the run's first process,
/// its group's leader — by parent, as `ps` lists them now, but is no
/// longer in its group: what left the group while its parent still ran.
/// The pids killed.
pub fn strays(root: i32) -> Vec<i32> {
    let Ok(listed) = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "pgid="])
        .output()
    else {
        return Vec::new();
    };
    outside_the_group(&String::from_utf8_lossy(&listed.stdout), root)
        .into_iter()
        .filter(|&pid| {
            rustix::process::Pid::from_raw(pid).is_some_and(|pid| {
                rustix::process::kill_process(pid, rustix::process::Signal::KILL).is_ok()
            })
        })
        .collect()
}

/// Of `listing`'s `pid ppid pgid` rows, every descendant of `root` whose
/// group is not `root`'s.
fn outside_the_group(listing: &str, root: i32) -> Vec<i32> {
    let rows: Vec<[i32; 3]> = listing
        .lines()
        .filter_map(|line| {
            let mut cells = line.split_whitespace().map(|cell| cell.parse::<i32>().ok());
            Some([cells.next()??, cells.next()??, cells.next()??])
        })
        .collect();
    let mut found = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for &[pid, ppid, pgid] in &rows {
            if ppid == parent && pid != root && !found.contains(&pid) {
                parents.push(pid);
                if pgid != root {
                    found.push(pid);
                }
            }
        }
    }
    found
}

/// Kill every process of this user whose environment names `tmp`, the
/// run's own `TMPDIR` — what of the run left its process group — but the
/// `killed` already, and say how many were found.
pub fn sweep(tmp: &Path, killed: &[i32]) -> usize {
    let marker = format!("TMPDIR={}", tmp.display());
    let Ok(listed) = Command::new("/bin/ps")
        .args(["-A", "-E", "-ww", "-o", "pid=", "-o", "command="])
        .output()
    else {
        return 0;
    };
    let mut found = 0;
    for line in String::from_utf8_lossy(&listed.stdout).lines() {
        let line = line.trim_start();
        if !line.contains(&marker) {
            continue;
        }
        let pid = line
            .split_whitespace()
            .next()
            .and_then(|pid| pid.parse::<i32>().ok())
            .filter(|pid| !killed.contains(pid))
            .and_then(rustix::process::Pid::from_raw);
        if let Some(pid) = pid {
            if rustix::process::kill_process(pid, rustix::process::Signal::KILL).is_ok() {
                found += 1;
            }
        }
    }
    found
}

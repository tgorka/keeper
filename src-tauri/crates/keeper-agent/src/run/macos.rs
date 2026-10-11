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
//! does not hold. The Mac's bound is [`sweep`]: once the group is killed,
//! every process whose environment still names the run's own `TMPDIR` is
//! killed too, and the run's pipes are drained for a bounded time either
//! way. A process that also rewrote its environment escapes the sweep and
//! stays inside the same profile, keeping every grant the run had — the
//! workspace read and write, and for a run without network the drives it
//! asked to read, read-only; no secret, no other drive, no network the
//! run did not have (DW-757, accepted by R231; R247) — and the run's
//! result says what was swept
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

/// Kill every process of this user whose environment names `tmp`, the
/// run's own `TMPDIR` — what of the run left its process group — and say
/// how many were found.
pub fn sweep(tmp: &Path) -> usize {
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
            .and_then(rustix::process::Pid::from_raw);
        if let Some(pid) = pid {
            if rustix::process::kill_process(pid, rustix::process::Signal::KILL).is_ok() {
                found += 1;
            }
        }
    }
    found
}

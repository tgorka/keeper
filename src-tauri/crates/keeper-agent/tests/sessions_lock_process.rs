//! A second process waits on the zone lock (90.2 acceptance 2).
//!
//! The test re-executes its own binary with an environment flag, the way
//! `keeper-sync`'s durability matrix does, so the holder is a real second
//! process and the lock under test is the file lock, not the in-process one.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keeper_agent::sessions::exec;
use keeper_agent::sessions::lock::ZoneLock;
use keeper_core::sessions::plan::{Plan, PlanStep};

const HOLD_ZONE: &str = "KEEPER_AGENT_TEST_HOLD_ZONE";
const HOLD: Duration = Duration::from_millis(500);

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos()
}

/// The child's half: take the lock, say so, hold it, and record when it let go.
///
/// A harness, not a test: run as itself, without the flag, it does nothing and
/// passes. `a_second_process_waits_on_the_lock_file` runs it in a child.
#[test]
fn hold_the_zone_when_asked() {
    let Some(zone) = std::env::var_os(HOLD_ZONE) else {
        return;
    };
    let zone = Path::new(&zone);
    let held = ZoneLock::acquire(zone).expect("the child takes the lock");
    std::fs::write(zone.join("child-holds"), "").expect("signal");
    std::thread::sleep(HOLD);
    let released = now_ns();
    drop(held);
    std::fs::write(zone.join("child-released"), released.to_string()).expect("stamp");
}

/// Kills and reaps the child however the parent's test ends, so a failed
/// assertion never leaves a process holding the zone.
struct Reaped(std::process::Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_second_process_waits_on_the_lock_file() {
    let zone = tempfile::tempdir().expect("zone");
    std::fs::create_dir_all(zone.path().join("active")).expect("active");
    let mut child = Reaped(
        std::process::Command::new(std::env::current_exe().expect("this binary"))
            .args(["--exact", "hold_the_zone_when_asked", "--nocapture"])
            .env(HOLD_ZONE, zone.path())
            .spawn()
            .expect("spawn the holder"),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !zone.path().join("child-holds").exists() {
        if let Some(status) = child.0.try_wait().expect("the child's status") {
            panic!("the child ended before it took the lock: {status}");
        }
        assert!(Instant::now() < deadline, "the child never took the lock");
        std::thread::sleep(Duration::from_millis(5));
    }
    exec::run(
        zone.path(),
        Plan {
            verb: "test".to_owned(),
            session: "active/s".to_owned(),
            steps: vec![PlanStep::MkDir {
                path: "active/s".to_owned(),
            }],
        },
    )
    .expect("the plan runs once the child lets go");
    let finished = now_ns();
    assert!(child.0.wait().expect("child").success());
    let released: u128 = std::fs::read_to_string(zone.path().join("child-released"))
        .expect("the child stamped its release")
        .parse()
        .expect("a number");
    assert!(
        finished >= released,
        "the parent's plan finished before the child released the zone"
    );
    assert!(zone.path().join("active/s").is_dir());
}

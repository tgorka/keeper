//! `harden_process` leaves no secret variable and no core dump (story 90.3,
//! acceptance 2; S-07).
//!
//! The test re-executes its own binary with an environment flag, so the
//! process that hardens itself is a fresh one whose environment and dump flag
//! nothing else in the suite shares.
#![cfg(unix)]

use std::process::Command;

use keeper_agent::headless::{harden_process, HardenError, SECRET_ENV_PREFIX};
use keeper_sync::xdg::SecretStore;

const CHILD: &str = "KEEPER_AGENT_TEST_HARDEN_CHILD";

/// The child's half: harden, then report what is left.
#[test]
fn harden_when_asked() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let mut store = SecretStore::new(SECRET_ENV_PREFIX, root.path().join("secrets"));

    harden_process(&mut store, &["x"]).expect("harden");

    assert_eq!(store.get("x").expect("held"), Some("v".to_owned()));
    assert_eq!(std::env::var_os("KEEPER_AGENTD_SECRET_X"), None);
    let env = Command::new("env").output().expect("spawn env");
    let printed = String::from_utf8_lossy(&env.stdout);
    assert!(
        !printed.contains("KEEPER_AGENTD_SECRET_"),
        "a child inherited a secret variable"
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        rustix::process::dumpable_behavior().expect("PR_GET_DUMPABLE"),
        rustix::process::DumpableBehavior::NotDumpable
    );
}

#[test]
fn harden_leaves_no_secret_variable_and_no_dump() {
    let output = Command::new(std::env::current_exe().expect("this binary"))
        .args(["--exact", "harden_when_asked", "--nocapture"])
        .env(CHILD, "1")
        .env("KEEPER_AGENTD_SECRET_X", "v")
        .output()
        .expect("spawn the child");
    assert!(
        output.status.success(),
        "the hardened child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "the child's test did not run"
    );
}

/// A needed secret in none of the three places stops the start, naming each
/// place, before the environment or the dump flag is touched.
#[test]
fn a_needed_secret_that_is_absent_is_refused_naming_where_it_goes() {
    let root = tempfile::tempdir().expect("tempdir");
    let secrets = root.path().join("secrets");
    let mut store = SecretStore::new(SECRET_ENV_PREFIX, &secrets);

    let refusal = harden_process(&mut store, &["harden-test-absent"]).expect_err("refused");

    assert!(matches!(refusal, HardenError::Missing { .. }), "{refusal}");
    let sentence = refusal.to_string();
    for place in [
        "LoadCredential=harden_test_absent:",
        "KEEPER_AGENTD_SECRET_HARDEN_TEST_ABSENT",
        &secrets.join("harden_test_absent").display().to_string(),
    ] {
        assert!(sentence.contains(place), "{place} missing from: {sentence}");
    }
}

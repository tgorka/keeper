//! The worker executable's startup refusals, launched as a process (OD-A
//! delta NC-4 and NC-14 shapes): an environment other than the fixed one,
//! and an exec without `no_new_privs`, exit 74 before anything of a
//! generation is opened. The generation path names a file that does not
//! exist, so any later step would fail differently.

#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};

const WORKER: &str = env!("CARGO_BIN_EXE_keeper-graph-query");
const TUNABLES: &str = "glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0:glibc.malloc.trim_threshold=536870912:glibc.malloc.arena_max=1";

fn worker(no_new_privs: bool, extra: Option<(&str, &str)>) -> (Option<i32>, String) {
    let missing = std::env::temp_dir().join("kg1-startup-missing-generation.db");
    let mut command = if no_new_privs {
        let mut c = Command::new("setpriv");
        c.arg("--no-new-privs").arg(WORKER);
        c
    } else {
        Command::new(WORKER)
    };
    command
        .arg("query")
        .arg(&missing)
        .env_clear()
        .env("GQL_STMT_CACHE", "0")
        .env("GLIBC_TUNABLES", TUNABLES)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some((key, value)) = extra {
        command.env(key, value);
    }
    let out = command.output().expect("the worker runs");
    (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn an_exec_without_no_new_privs_is_refused_with_its_label() {
    let (code, out) = worker(false, None);
    assert_eq!(code, Some(74));
    assert_eq!(out, "execution class refused: no_new_privs\n");
}

#[test]
fn an_extra_variable_is_refused_before_the_class_check() {
    for extra in [("LC_ALL", "C.UTF-8"), ("TZ", "UTC")] {
        let (code, out) = worker(true, Some(extra));
        assert_eq!(code, Some(74), "{extra:?}");
        assert_eq!(out, "", "the environment check runs first: {extra:?}");
    }
}

#[test]
fn an_admitted_exec_passes_startup_and_fails_only_at_the_missing_generation() {
    let (code, out) = worker(true, None);
    assert_eq!(code, Some(74));
    assert_eq!(out, "", "no class or platform label: the class was admitted");
}

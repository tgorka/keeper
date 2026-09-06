//! Bake the build's identity in, so a log line can name the binary that wrote it.
//!
//! A log that says only "keeper 0.8.20" cannot tell two builds of 0.8.20 apart,
//! and this project ships builds of the same version routinely — a release, and
//! then a test build off a branch, installed over it. When a log arrives from a
//! machine nobody can reach, "which binary was this?" is the first question and
//! there has been no way to answer it.
//!
//! `KEEPER_BUILD_SHA` is best effort by necessity: `scripts/release-macos.sh`
//! builds from an rsync'd copy of the tree that has no `.git`, which is stated
//! in that script's own header. An honest `"unknown"` beats a lie, and the
//! timestamp below identifies the build even when the commit cannot.
//!
//! **The caller may state it, and then it is believed.** A build directory on
//! another machine can hold a `.git` from an older rsync — hesperia's did, and
//! on 2026-09-05 a freshly installed build of `4cce07c19538` logged
//! `220f8bde27d2-dirty`, which is worse than `unknown`: it sends the next
//! reader to a diff that is not the one running. The install scripts export
//! `KEEPER_BUILD_SHA` from the repository they rsynced FROM, and this script
//! prefers it over any local `git` answer.

use std::process::Command;

fn main() {
    // A FILE beside this script, not only an environment variable: the iOS
    // build reaches `cargo` through an Xcode build phase dispatched into a GUI
    // login session, and an export that survives every hop of that is not
    // something this script can verify. Measured on hesperia 2026-09-06: with
    // `KEEPER_BUILD_SHA` exported into the payload, the macOS build took it and
    // the iOS bundle still carried the stale probe. A file in the rsynced tree
    // is immune to env plumbing, and `rerun-if-changed` below makes a new sha
    // rebuild this script's output.
    println!("cargo:rerun-if-changed=build-sha.txt");
    println!("cargo:rerun-if-env-changed=KEEPER_BUILD_SHA");
    let stated_sha = std::fs::read_to_string("build-sha.txt")
        .ok()
        .or_else(|| std::env::var("KEEPER_BUILD_SHA").ok());
    let sha = stated_sha
        .clone()
        .map(|stated| stated.trim().to_owned())
        .filter(|stated| !stated.is_empty())
        .unwrap_or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
                .filter(|sha| !sha.is_empty())
                .unwrap_or_else(|| "unknown".to_owned())
        });
    // A stated sha carries its own `-dirty` (the scripts append it), so the
    // local probe below must not add a second one.
    let stated = stated_sha.is_some_and(|value| !value.trim().is_empty());
    // Dirty is worth knowing: a build from a modified tree is not the commit it
    // names, and a log that claimed it was would send the next person to the
    // wrong diff.
    let dirty = !stated
        && Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=no"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| !out.stdout.is_empty())
            .unwrap_or(false);
    println!(
        "cargo:rustc-env=KEEPER_BUILD_SHA={sha}{}",
        if dirty { "-dirty" } else { "" }
    );
    println!(
        "cargo:rustc-env=KEEPER_BUILD_TIME={}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    // On iOS the crate is linked into the app as a staticlib, and the four
    // `keeper_island_*` symbols `voice_island.rs` calls are defined by the
    // app target's Swift (`gen/apple/Sources/keeper/KeeperIsland.swift`), so
    // they resolve at the app's link and nowhere earlier. Cargo still builds
    // the `cdylib` crate type on iOS - nothing loads it there - and ld refuses
    // a dylib with undefined symbols, so that one link is told to look them up
    // at load time. The staticlib and the app's link are untouched. Measured
    // on hesperia 2026-09-05: without this, `tauri ios build` dies at
    // "Undefined symbols for architecture arm64: _keeper_island_end" while
    // linking `libkeeper_lib.dylib`, an artefact the bundle never carries.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        println!("cargo:rustc-link-arg-cdylib=-Wl,-undefined,dynamic_lookup");
    }
    // Without this the sha is frozen at whatever the first build saw.
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
    tauri_build::build()
}

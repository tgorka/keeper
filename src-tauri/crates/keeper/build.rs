//! Bake the build's identity in, so a log line can name the binary that wrote it.
//!
//! A log that says only "keeper 0.8.20" cannot tell two builds of 0.8.20 apart,
//! and this project ships builds of the same version routinely — a release, and
//! then a test build off a branch, installed over it. When a log arrives from a
//! machine nobody can reach, "which binary was this?" is the first question and
//! there has been no way to answer it.
//!
//! **The file is the primary channel.** `src-tauri/crates/keeper/build-sha.txt`
//! is written by `scripts/lib/build-sha.sh` — the one writer — from the checkout
//! a script runs in, before it rsyncs or builds, and this script reads it first.
//! It has to be a statement rather than a probe: the tree that gets built on
//! the Mac is an rsynced copy with no `.git`, and a `git` probe there does not
//! say "not a repository", it walks up and answers for the nearest one — the
//! dotfiles checkout at `~/.git`, which is the `220f8bde27d2-dirty` a freshly
//! installed build logged on 2026-09-05 (the library's header keeps the
//! verified account). Second comes `KEEPER_BUILD_SHA` in the environment, which
//! the install scripts export as well but which does not survive every hop of
//! the iOS build; third a `git` probe of THIS repository only; and `unknown`
//! when all three are silent.
//!
//! **A believed statement can be stale**, and a stale one is worse than
//! `unknown`: on 2026-09-09 a bare rebuild on hesperia read a stamp two days
//! old and the `commit=` line sent the next reader to the wrong diff. Two
//! things guard against that here. The binary carries WHERE its sha came from —
//! `KEEPER_BUILD_SHA_SOURCE`, one of `file`, `env`, `git`, `none` — and the
//! build prints the same as a cargo warning, so a stamp is at least visibly a
//! stamp. And on a real checkout, where `../../../.git` exists and can be
//! asked, `git` is asked too: a stamp left behind by a script run outranks a
//! `.git` that has since moved on unless the two disagree, in which case the
//! moving one is right and the build says so.

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
    let (sha, source) = build_sha();
    // The binary says WHERE its sha came from, and the build says so too. A
    // stamp file is believed over `git` by design, which means a stale stamp
    // is believed just as readily: on 2026-09-09 hesperia's 0.8.26 build
    // logged a commit two days older than the code it ran, because the file
    // had last been written by an install and nothing since had rewritten it.
    // The sha alone cannot reveal that; `sha_source=file` at least tells the
    // reader it is a stamp, and the warning puts the same fact in the build log.
    println!("cargo:warning=keeper build sha {sha} from {source}");
    println!("cargo:rustc-env=KEEPER_BUILD_SHA={sha}");
    println!("cargo:rustc-env=KEEPER_BUILD_SHA_SOURCE={source}");
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
    // Without these the sha is frozen at whatever the first build saw. `HEAD`
    // changes on a checkout or a branch switch; a plain commit on the SAME
    // branch leaves it untouched and moves only the ref it points at, which
    // is why the reflog is watched too — every commit appends a line there.
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../../.git/logs/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
    tauri_build::build()
}

/// Whether a stated value has the shape of a sha this build could have come
/// from: 7 to 40 lowercase hex digits, optionally `-dirty`. A stamp holds what
/// a script wrote, an env var holds what a caller typed, and either can be a
/// stray word; a `commit=` field that reads as text but names nothing is worse
/// than `unknown`, because it looks like an answer.
fn looks_like_sha(value: &str) -> bool {
    let hex = value.strip_suffix("-dirty").unwrap_or(value);
    (7..=40).contains(&hex.len())
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The commit this build is of, and which of the four sources answered.
///
/// Precedence: the `build-sha.txt` stamp beside this script (`file`), then
/// `KEEPER_BUILD_SHA` in the environment (`env`), then a `git` probe of the
/// tree (`git`), and `unknown` from `none` when all three are silent. The two
/// stated forms carry their own `-dirty` — the scripts append it — so only the
/// probed one asks `git status`; a stated sha must never grow a second suffix.
///
/// One exception to the precedence: where this repository's own `.git` exists,
/// `git` is asked as well, and a stated sha that disagrees with it loses. A
/// stamp is written by a script run and then sits there while the developer
/// keeps committing; without this rule a `cargo build` a week later would
/// still name the commit of that run, labelled `file`, and be believed.
fn build_sha() -> (String, &'static str) {
    let non_empty = |value: String| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    };
    // A stated value that is not a sha is treated as absent, with a warning
    // rather than silently: the reader of the build log should learn that a
    // stamp or an export said something that was ignored.
    let validated = |value: String, source: &'static str| {
        if looks_like_sha(&value) {
            Some((value, source))
        } else {
            println!(
                "cargo:warning=keeper build sha: ignoring {source} value {value:?}; not a sha"
            );
            None
        }
    };
    let stated = std::fs::read_to_string("build-sha.txt")
        .ok()
        .and_then(non_empty)
        .and_then(|value| validated(value, "file"))
        .or_else(|| {
            std::env::var("KEEPER_BUILD_SHA")
                .ok()
                .and_then(non_empty)
                .and_then(|value| validated(value, "env"))
        });
    // Only THIS repository may answer, the one the rerun triggers in `main`
    // already assume at `../../../.git`. Without the check `git` walks up the
    // parents and answers for whatever repository the tree happens to sit
    // inside: a home directory that is itself a dotfiles checkout makes an
    // rsynced copy with no `.git` of its own report the dotfiles commit,
    // `-dirty`, as if it were keeper's — which is exactly the
    // `220f8bde27d2-dirty` line hesperia logged. `-e`, not a directory test:
    // in a worktree `.git` is a file.
    let probed = if std::path::Path::new("../../../.git").exists() {
        probe_git()
    } else {
        None
    };
    match (stated, probed) {
        (Some((stamp, _)), Some(git)) if stamp != git => {
            println!(
                "cargo:warning=keeper build sha: stamp {stamp} disagrees with git {git}; using git"
            );
            (git, "git")
        }
        (Some((stamp, source)), _) => (stamp, source),
        (None, Some(git)) => (git, "git"),
        (None, None) => ("unknown".to_owned(), "none"),
    }
}

/// What `git` says about the tree this script runs in: the short head, with
/// `-dirty` when tracked files differ from it. `None` when it cannot say.
fn probe_git() -> Option<String> {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let sha = git(&["rev-parse", "--short=12", "HEAD"]).filter(|sha| !sha.is_empty())?;
    // Dirty is worth knowing: a build from a modified tree is not the commit it
    // names, and a log that claimed it was would send the next person to the
    // wrong diff. `--untracked-files=no` for the same reason, and with the same
    // trade-off, as the shell library: a never-added file reads clean.
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|out| !out.is_empty());
    Some(if dirty { format!("{sha}-dirty") } else { sha })
}

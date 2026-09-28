//! NFR-105 (D-29, AD-339, AD-341, AD-350): transcription happens on this Mac,
//! and the claim is enforced, not asserted.
//!
//! `docs/egress.md` names every destination keeper contacts, and no
//! transcription server, model hub or cloud API is on it. That stays true only
//! while no transcription module reaches for a network API and the vendored
//! FluidAudio bridge never calls one of FluidAudio's download helpers. This
//! test reads the sources off disk as text, so it runs on the dev host even
//! though the engine only compiles for macOS:
//!
//! - `keeper-core/src/transcription/**` — the pure pipeline and the bank;
//! - every `transcribe*.rs` directly in the shell crate's `src/` — the engine
//!   port and the job commands, picked up by prefix;
//! - the vendored bridge `tools/fluidaudio-rs/`: its Swift under `swift/**`
//!   and its Rust under `src/**`, which must also never name a FluidAudio
//!   download entry point (FluidAudio itself carries them; the bridge loads
//!   models only from a directory it is given).
//!
//! The ONE sanctioned fetch of transcription models is the Sync lane's
//! hydration of the account config repo's `_models/` (keeper-sync
//! `config_repo`, driven from the shell's account code), which reaches only
//! the config repo host `docs/egress.md` already lists. It lives outside the
//! scanned set on purpose: the engine and the job never fetch, they find a
//! complete set on disk or refuse.
//!
//! Only production code is scanned: a file's trailing `#[cfg(test)]` module
//! (fixtures carry LFS pointer text, which names its spec URL) is cut off, and
//! the cut is refused unless that module really is the file's tail.
//!
//! Shape follows `voice_on_device.rs`: tokens are built by concatenation so
//! this file never matches itself, file lists are derived rather than
//! hand-maintained, paths are anchored on `CARGO_MANIFEST_DIR`, and every
//! scanned set has a floor, so an empty or moved tree is a failure, not a pass.

use std::path::{Path, PathBuf};

/// Every file under `dir`, recursively, whose extension is `ext`.
fn collect_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("transcription scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .expect("transcription scan: cannot read a directory entry")
            .path();
        if path.is_dir() {
            collect_ext(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

/// Every `transcribe*.rs` directly under `dir` — the shell crate keeps its
/// transcription modules flat, so a new one is scanned by prefix.
fn collect_transcribe_prefixed(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("transcription scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .expect("transcription scan: cannot read a directory entry")
            .path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_file() && name.starts_with("transcribe") && name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// Collects with `collect` from `dir` and fails unless at least `floor`
/// files were found — a scan over nothing guards nothing.
fn scanned(
    dir: &Path,
    floor: usize,
    what: &str,
    collect: impl Fn(&Path, &mut Vec<PathBuf>),
) -> Vec<PathBuf> {
    assert!(
        dir.is_dir(),
        "transcription scan: {what} not found at {} — the guard must never pass vacuously",
        dir.display()
    );
    let mut files = Vec::new();
    collect(dir, &mut files);
    assert!(
        files.len() >= floor,
        "transcription scan: {} {what} file(s) under {}, expected at least {floor}",
        files.len(),
        dir.display()
    );
    files
}

/// The file's lowercased production text. A Rust file's `#[cfg(test)]`
/// module is dropped only when it is the file's tail: after it nothing may
/// start at column 0 except its own closing brace.
fn production_text(file: &Path) -> String {
    let source = std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("transcription scan: cannot read {}: {e}", file.display()));
    let marker = "\n#[cfg(test)]\n";
    let production = match source.find(marker) {
        Some(at) if file.extension().is_some_and(|e| e == "rs") => {
            let tail = &source[at + marker.len()..];
            let mut lines = tail.lines();
            let opener = lines.next().unwrap_or_default();
            assert!(
                opener.starts_with("mod ") || opener.starts_with("pub(crate) mod "),
                "transcription scan: {} has a column-0 #[cfg(test)] item that is not the \
                 test module; move it into the module so the scan can tell tests from code",
                file.display()
            );
            let closers = lines
                .filter(|l| !l.is_empty() && !l.starts_with(char::is_whitespace))
                .collect::<Vec<_>>();
            assert!(
                closers == ["}"],
                "transcription scan: {} has code after its #[cfg(test)] module; the scan \
                 would skip it, so keep the test module last",
                file.display()
            );
            &source[..at]
        }
        _ => source.as_str(),
    };
    production.to_lowercase()
}

fn assert_free_of(files: &[PathBuf], forbidden: &[&str], why: &str) {
    for file in files {
        let text = production_text(file);
        for token in forbidden {
            assert!(
                !text.contains(token),
                "NFR-105 violation: {} contains {token:?} — {why}",
                file.display()
            );
        }
    }
}

/// No transcription source, in keeper or in the vendored bridge, names a
/// network API; the bridge additionally names none of FluidAudio's model
/// download entry points.
#[test]
fn transcription_sources_carry_no_network_path() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bridge = manifest.join("../../../tools/fluidaudio-rs");

    let core = scanned(
        &manifest.join("src/transcription"),
        8,
        "keeper-core transcription",
        |d, o| collect_ext(d, "rs", o),
    );
    let shell = scanned(
        &manifest.join("../keeper/src"),
        1,
        "shell transcribe*.rs",
        collect_transcribe_prefixed,
    );
    let bridge_swift = scanned(&bridge.join("swift"), 1, "bridge Swift", |d, o| {
        collect_ext(d, "swift", o)
    });
    let bridge_rust = scanned(&bridge.join("src"), 1, "bridge Rust", |d, o| {
        collect_ext(d, "rs", o)
    });

    let url_session = format!("urlses{}", "sion");
    let url_request = format!("urlreq{}", "uest");
    let ns_url = format!("ns{}", "url");
    let reqwest = format!("req{}", "west");
    let nw_connection = format!("nwconnec{}", "tion");
    let http = format!("ht{}", "tp");
    let network = [
        url_session.as_str(),
        url_request.as_str(),
        ns_url.as_str(),
        reqwest.as_str(),
        nw_connection.as_str(),
        http.as_str(),
    ];
    let why = "transcription is on this Mac or nothing, and docs/egress.md names no \
               transcription server (D-29)";
    assert_free_of(&core, &network, why);
    assert_free_of(&shell, &network, why);

    // FluidAudio's own loaders fall back to fetching from Hugging Face; the
    // bridge must reach models only through `loadLocal` / `MLModel(contentsOf:)`.
    let download_and_load = format!("downloadand{}", "load");
    let download_models = format!("downloadmod{}", "els");
    let download_if_needed = format!("downloadifne{}", "eded");
    let download_utils = format!("downloadut{}", "ils");
    let dot_download = format!(".downl{}", "oad(");
    let prepare_models = format!(".preparemod{}", "els(");
    let hugging_face = format!("hugging{}", "face");
    let bridge_forbidden = [
        network.as_slice(),
        &[
            download_and_load.as_str(),
            download_models.as_str(),
            download_if_needed.as_str(),
            download_utils.as_str(),
            dot_download.as_str(),
            prepare_models.as_str(),
            hugging_face.as_str(),
        ],
    ]
    .concat();
    let why = "the bridge loads models only from the directory keeper hydrated from the \
               config repo, never from a model hub (AD-341)";
    assert_free_of(&bridge_swift, &bridge_forbidden, why);
    assert_free_of(&bridge_rust, &bridge_forbidden, why);
}

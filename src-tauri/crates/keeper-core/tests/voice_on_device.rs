//! Story 62.7 (NFR-50, FR-402, AD-166): voice adds no network destination,
//! and the claim is enforced, not asserted.
//!
//! `docs/egress.md` names every destination keeper contacts, and Apple's
//! speech servers are not on it. That stays true only while every recognition
//! request the iOS port builds sets `requiresOnDeviceRecognition = true` and
//! no voice module reaches for a network API. This test reads the voice
//! sources off disk as text — `keeper-core/src/voice/**` and every
//! `voice*.rs` in the sibling shell crate — so it runs on the dev host even
//! though the port itself only compiles for iOS, and it fails loudly, naming
//! the file and the token, if a server path ever appears.
//!
//! Shape follows `keeper_rec_sidecar_sources_are_network_free` in the `keeper`
//! crate and `presence_is_withheld_everywhere` in this one: tokens are built
//! by concatenation so this file never matches itself, the file list is
//! derived rather than hand-maintained, and paths are anchored on
//! `CARGO_MANIFEST_DIR`, never the process CWD. An empty file list is a
//! failure, not a pass.

use std::path::{Path, PathBuf};

/// Every `.rs` under `dir`, recursively.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("voice on-device scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .expect("voice on-device scan: cannot read a directory entry")
            .path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Every `voice*.rs` directly under `dir` — the shell crate keeps its voice
/// modules flat (`voice_ios.rs`, `voice_ipc.rs`), so a new one is picked up
/// by prefix rather than by editing this list.
fn collect_voice_prefixed(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("voice on-device scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .expect("voice on-device scan: cannot read a directory entry")
            .path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_file() && name.starts_with("voice") && name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// The voice sources across both crates: this crate's `src/voice/` tree and
/// the shell crate's `voice*.rs` files. Both halves must resolve — the port
/// is the file the guard exists for, so its absence is a failure.
fn voice_sources() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core_voice = manifest.join("src/voice");
    let shell_src = manifest.join("../keeper/src");
    assert!(
        core_voice.is_dir(),
        "voice on-device scan: keeper-core voice module not found at {} — the guard must never pass vacuously",
        core_voice.display()
    );
    assert!(
        shell_src.is_dir(),
        "voice on-device scan: sibling shell crate src not found at {} — the guard must never pass vacuously",
        shell_src.display()
    );
    let mut files = Vec::new();
    collect_rs(&core_voice, &mut files);
    let before_shell = files.len();
    collect_voice_prefixed(&shell_src, &mut files);
    assert!(
        before_shell > 0,
        "voice on-device scan: no sources under {}",
        core_voice.display()
    );
    assert!(
        files.len() > before_shell,
        "voice on-device scan: no voice*.rs under {} — the iOS port is the file this guard exists for",
        shell_src.display()
    );
    files
}

/// Every byte offset at which `needle` occurs in `haystack`.
fn offsets(haystack: &str, needle: &str) -> Vec<usize> {
    haystack.match_indices(needle).map(|(i, _)| i).collect()
}

/// No voice module contains a token that would mean a server round trip: the
/// on-device flag set to `false`, or any network API at all. The scan is over
/// the lowercased source so a novel casing cannot slip past it.
#[test]
fn voice_sources_carry_no_server_path() {
    let flag_false_setter = format!("setrequiresondevicerecognition({})", "false");
    let flag_false_assign = format!("requiresondevicerecognition = {}", "false");
    let url_session = format!("urlses{}", "sion");
    let url_request = format!("urlreq{}", "uest");
    let ns_url = format!("ns{}", "url");
    let reqwest = format!("req{}", "west");
    let nw_connection = format!("nwconnec{}", "tion");
    let http = format!("ht{}", "tp");
    let forbidden = [
        flag_false_setter.as_str(),
        flag_false_assign.as_str(),
        url_session.as_str(),
        url_request.as_str(),
        ns_url.as_str(),
        reqwest.as_str(),
        nw_connection.as_str(),
        http.as_str(),
    ];

    for file in voice_sources() {
        let source = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("voice on-device scan: cannot read {}: {e}", file.display()))
            .to_lowercase();
        for token in forbidden {
            assert!(
                !source.contains(token),
                "NFR-50 violation: {} contains {token:?} — speech recognition is on-device or \
                 nothing, and docs/egress.md names no speech server (FR-402)",
                file.display()
            );
        }
    }
}

/// Where a recognition request is built, the on-device flag is set to `true`
/// before a task is started on it. Every `…RecognitionRequest::new()` or
/// `…RecognitionRequest::alloc(` in the iOS port must be followed by
/// `setRequiresOnDeviceRecognition(true)` before the next request is built or
/// a task is started, and every task start must have a request built before
/// it. At least one request site must exist, or the port has stopped
/// recognising anything and this guard would be guarding nothing.
#[test]
fn every_recognition_request_is_on_device_before_its_task_starts() {
    let build_new = format!("recognitionrequest::{}()", "new");
    let build_alloc = format!("recognitionrequest::{}(", "alloc");
    let flag_true = format!("setrequiresondevicerecognition({})", "true");
    let task_start = format!("recognitiontaskwithrequest{}", "_resulthandler(");
    let supports = format!("supportsondevice{}()", "recognition");

    let mut request_sites = 0usize;
    for file in voice_sources() {
        let source = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("voice on-device scan: cannot read {}: {e}", file.display()))
            .to_lowercase();
        let mut builds = offsets(&source, &build_new);
        builds.extend(offsets(&source, &build_alloc));
        builds.sort_unstable();
        let flags = offsets(&source, &flag_true);
        let starts = offsets(&source, &task_start);

        if builds.is_empty() && starts.is_empty() {
            continue;
        }
        assert!(
            source.contains(&supports),
            "NFR-50: {} builds a recognition request without checking `supportsOnDeviceRecognition` \
             first, which is the precondition Apple names for the flag to be honoured",
            file.display()
        );

        for (i, &build) in builds.iter().enumerate() {
            let next_build = builds.get(i + 1).copied().unwrap_or(usize::MAX);
            let next_start = starts
                .iter()
                .copied()
                .find(|&s| s > build)
                .unwrap_or(usize::MAX);
            let window_end = next_build.min(next_start);
            assert!(
                flags.iter().any(|&f| f > build && f < window_end),
                "NFR-50 violation: {} builds a recognition request at byte {build} without \
                 `setRequiresOnDeviceRecognition(true)` before the task starts — that request \
                 could reach Apple's servers, a destination docs/egress.md does not name",
                file.display()
            );
        }
        for &start in &starts {
            assert!(
                builds.iter().any(|&b| b < start),
                "NFR-50 violation: {} starts a recognition task at byte {start} on a request \
                 this guard did not see built — build it with `…RecognitionRequest::new()` \
                 and set the on-device flag",
                file.display()
            );
        }
        request_sites += builds.len();
    }
    assert!(
        request_sites > 0,
        "voice on-device scan: no recognition request is built anywhere in the voice sources — \
         the guard would be enforcing nothing"
    );
}

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
//!
//! Android (Epic 98, story 98.4 #4) is read the same way: the port's Kotlin
//! half (`Voice*.kt` in `gen/android`) joins the network scan, and every
//! Kotlin file of the app is held to the on-device recogniser — the one
//! that may use a server is never made, and the scan's floor names
//! `voice_android.rs` and `VoicePlugin.kt`.

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

/// Every `.kt` under `dir`, recursively.
fn collect_kt(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("voice on-device scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .expect("voice on-device scan: cannot read a directory entry")
            .path();
        if path.is_dir() {
            collect_kt(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "kt") {
            out.push(path);
        }
    }
}

/// The Android app's Kotlin sources (Epic 98, story 98.4 #4): every `.kt`
/// under `gen/android/app/src/main/java`. Non-empty, and holding the voice
/// port's Kotlin half, or the guard would pass by looking nowhere.
fn android_kotlin() -> Vec<PathBuf> {
    let java =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../keeper/gen/android/app/src/main/java");
    assert!(
        java.is_dir(),
        "voice on-device scan: the Android app's sources are not at {} — the guard must never pass vacuously",
        java.display()
    );
    let mut files = Vec::new();
    collect_kt(&java, &mut files);
    let port = java.join("dev/tgorka/keeper/VoicePlugin.kt");
    assert!(
        files.contains(&port),
        "voice on-device scan: {} is not scanned — the guard must cover the Android port",
        port.display()
    );
    files
}

/// The voice sources across both crates: this crate's `src/voice/` tree and
/// the shell crate's `voice*.rs` files, and the Android port's Kotlin half —
/// every `Voice*.kt` of the app, picked up by prefix like the shell's. Every
/// half must resolve — the ports are the files the guard exists for, so
/// their absence is a failure.
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
    // The floor: the files that hold the turn models (D-36) and the
    // synthesiser the person talks over (AD-411) are read by name, so
    // renaming one out of the scan fails here instead of passing quietly.
    for required in [
        core_voice.join("turn_models.rs"),
        core_voice.join("backchannel.rs"),
        shell_src.join("voice_turn_models.rs"),
        shell_src.join("voice_synthesizer.rs"),
        shell_src.join("voice_ios.rs"),
        shell_src.join("voice_macos.rs"),
        shell_src.join("voice_android.rs"),
    ] {
        assert!(
            files.contains(&required),
            "voice on-device scan: {} is not scanned — the guard must cover it",
            required.display()
        );
    }
    files.extend(android_kotlin().into_iter().filter(|file| {
        file.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("Voice"))
    }));
    files
}

/// `source` with its comments taken out — `//` to the end of the line and
/// `/* … */` — so an ordering check reads code, not a sentence about it: a
/// comment naming the on-device check must not count as the check.
fn code_only(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    loop {
        // The first comment opener, and whether it is a line comment.
        let opener = match (rest.find("//"), rest.find("/*")) {
            (Some(line), Some(block)) if block < line => Some((block, false)),
            (Some(line), _) => Some((line, true)),
            (None, Some(block)) => Some((block, false)),
            (None, None) => None,
        };
        let Some((at, line)) = opener else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..at]);
        rest = if line {
            rest[at..].find('\n').map_or("", |end| &rest[at + end..])
        } else {
            rest[at + 2..]
                .find("*/")
                .map_or("", |end| &rest[at + 2 + end + 2..])
        };
    }
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
    // ONNX Runtime's own model download (`SessionBuilder::commit_from_url`):
    // turn models load from the hydrated `_models/` by path, never by address.
    let model_download = format!("commit_from_{}", "url");
    // The Android port's Kotlin half (98.4 #4): Java's network package and
    // raw sockets, and the one recogniser factory that may use a server.
    let java_net = format!("java.{}", "net");
    let socket = format!("sock{}", "et");
    let any_recogniser = format!("createspeech{}(", "recognizer");
    // The system's own recogniser screen, which an intent launches as an
    // activity (`RecognizerIntent.ACTION_RECOGNIZE_SPEECH` and the voice
    // search actions): it needs no factory, and it may use a server.
    let launched = format!("startactiv{}", "ity");
    let launched_for_result = format!("registerforactivity{}", "result");
    let voice_search = format!("action_voice_search_hands_{}", "free");
    let web_search = format!("action_web_{}", "search");
    let forbidden = [
        flag_false_setter.as_str(),
        flag_false_assign.as_str(),
        url_session.as_str(),
        url_request.as_str(),
        ns_url.as_str(),
        reqwest.as_str(),
        nw_connection.as_str(),
        http.as_str(),
        model_download.as_str(),
        java_net.as_str(),
        socket.as_str(),
        any_recogniser.as_str(),
        launched.as_str(),
        launched_for_result.as_str(),
        voice_search.as_str(),
        web_search.as_str(),
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
        let source = code_only(
            &std::fs::read_to_string(&file)
                .unwrap_or_else(|e| {
                    panic!("voice on-device scan: cannot read {}: {e}", file.display())
                })
                .to_lowercase(),
        );
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

/// Android (Epic 98, story 98.4 #4): every recogniser in the app is the
/// on-device one. No Kotlin file anywhere in the app makes the recogniser
/// that may send audio to a server; every on-device one is made after the
/// file has asked whether on-device recognition exists here — Android's
/// precondition, as `supportsOnDeviceRecognition` is Apple's — and nothing
/// tells a recogniser it may prefer a server. The order is read in code
/// with its comments taken out ([`code_only`]), so a comment naming the
/// check is not the check. At least one is made, or the port has stopped
/// recognising and the guard would guard nothing.
#[test]
fn every_android_recogniser_is_on_device() {
    let server_capable = format!("speechrecognizer.createspeech{}(", "recognizer");
    let on_device = format!("createondevicespeech{}(", "recognizer");
    let available = format!("isondevicerecognition{}(", "available");
    let online_preferred = format!("extra_prefer_offline, {}", "false");

    let mut made = 0usize;
    for file in android_kotlin() {
        let source = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("voice on-device scan: cannot read {}: {e}", file.display()))
            .to_lowercase();
        assert!(
            !source.contains(&server_capable),
            "NFR-50 violation: {} makes a recogniser that may send audio to a server — \
             keeper on Android uses `createOnDeviceSpeechRecognizer` only",
            file.display()
        );
        assert!(
            !source.contains(&online_preferred),
            "NFR-50 violation: {} lets a recogniser prefer a server",
            file.display()
        );
        let code = code_only(&source);
        let sites = offsets(&code, &on_device);
        if sites.is_empty() {
            continue;
        }
        let first_check = code.find(&available).unwrap_or(usize::MAX);
        for site in &sites {
            assert!(
                first_check < *site,
                "NFR-50: {} makes an on-device recogniser at byte {site} without first asking \
                 `SpeechRecognizer.isOnDeviceRecognitionAvailable`",
                file.display()
            );
        }
        made += sites.len();
    }
    assert!(
        made > 0,
        "voice on-device scan: no on-device recogniser is made anywhere in the Android app — \
         the guard would be enforcing nothing"
    );
}

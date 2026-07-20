//! Zero-egress source-scan audit over the `keeper-rec` sidecar (Story 20.4,
//! FR-76, epic-20 exit invariant).
//!
//! The Swift sidecar is the only recording-specific process; the record → stop
//! → recover cycle is provably local only if it contains **no network API at
//! all** (ScreenCaptureKit + AVAssetWriter + stdio NDJSON only). This module is
//! `#[cfg(test)]`-only (declared so in `lib.rs`): it ships no code, it only
//! scans the checked-in Swift sources at test time and fails loudly — naming
//! the offending file and token — if a network affordance ever appears.
//!
//! Mirrors `keeper_core::recording::tests::dependency_firewall_holds` (AD-33):
//! forbidden tokens are built by string concatenation so this scan file never
//! self-matches, and the path is anchored on `CARGO_MANIFEST_DIR` (never the
//! process CWD, which varies across nextest/IDE runners).

use std::path::{Path, PathBuf};

/// Recursively collect every `.swift` file under `dir`. `expect` is fine here —
/// this is test-only code, and an unreadable tree must fail the audit loudly.
fn collect_swift_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("zero-egress scan: cannot read {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("zero-egress scan: cannot read a directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_swift_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "swift") {
            out.push(path);
        }
    }
}

/// The `keeper-rec` sidecar sources contain no network API token: no
/// URLSession/URLRequest, no Network.framework, no raw sockets, no http URL.
/// The scan is case-insensitive over the lowercased sources so a novel casing
/// can never slip past it; each token is built by concatenation so this test
/// file never matches itself.
#[test]
fn keeper_rec_sidecar_sources_are_network_free() {
    let url_session = format!("urlses{}", "sion");
    let url_request = format!("urlreq{}", "uest");
    let nw_connection = format!("nwconnec{}", "tion");
    let nw_listener = format!("nwlist{}", "ener");
    let import_network = format!("import net{}", "work");
    let http = format!("ht{}", "tp");
    let socket = format!("soc{}", "ket");
    let cf_stream = format!("cfstr{}", "eam");
    let forbidden = [
        url_session.as_str(),
        url_request.as_str(),
        nw_connection.as_str(),
        nw_listener.as_str(),
        import_network.as_str(),
        http.as_str(),
        socket.as_str(),
        cf_stream.as_str(),
    ];

    // `CARGO_MANIFEST_DIR` = src-tauri/crates/keeper; three levels up is the
    // repository root hosting the SwiftPM package.
    let sources_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/keeper-rec/Sources");
    assert!(
        sources_dir.is_dir(),
        "zero-egress scan: sidecar sources dir not found at {} — the audit must never pass vacuously",
        sources_dir.display()
    );
    let mut files = Vec::new();
    collect_swift_sources(&sources_dir, &mut files);
    assert!(
        !files.is_empty(),
        "zero-egress scan: no Swift sources found under {} — the audit must never pass vacuously",
        sources_dir.display()
    );

    for file in files {
        let source = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("zero-egress scan: cannot read {}: {e}", file.display()))
            .to_lowercase();
        for token in forbidden {
            assert!(
                !source.contains(token),
                "zero-egress violation: {} contains the forbidden network token {token:?} — \
                 the keeper-rec sidecar must stay fully local (FR-76)",
                file.display()
            );
        }
    }
}

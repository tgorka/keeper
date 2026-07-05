//! The single receipt/typing SDK emit/subscribe gate (AD-14).
//!
//! This module is the *sole* caller of the SDK receipt/typing emit/subscribe
//! surface: `Timeline::mark_as_read` / `Timeline::send_single_receipt` (read
//! receipts), `Room::typing_notice` (set typing), and
//! `Room::subscribe_to_typing_notifications` (observe typing). A crate-wide
//! source-scan guard test asserts those method names appear in no other
//! `keeper-core/src/*.rs` file — a stronger invariant than the send-gate's
//! single-file guard, by design (AD-14).
//!
//! Reading *already-populated* per-item receipts for rendering
//! (`EventTimelineItem::read_receipts`) is render data, not an emission, so it
//! lives in `timeline.rs`, not here. Back-pagination reads history and is not a
//! signal, so it stays in `timeline.rs`/`account.rs`.
//!
//! Receipts and typing are emitted as PUBLIC (`m.read` / normal typing) — this is
//! normal, non-Incognito operation (Incognito/private-receipt policy is Epic 8).
//! Best-effort: an emit failure is a non-fatal [`SignalError::Dispatch`] the
//! caller swallows for the UI. Secret containment (NFR-9): no token, event id, or
//! plaintext ever crosses here — only opaque user ids flow out of the typing
//! subscription.

use matrix_sdk::event_handler::EventHandlerDropGuard;
use matrix_sdk::ruma::api::client::receipt::create_receipt::v3::ReceiptType;
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::Room;
use matrix_sdk_ui::timeline::Timeline;
use tokio::sync::broadcast;

use crate::error::SignalError;

/// Mark the room's timeline read up to its latest event, dispatching a PUBLIC
/// `m.read` receipt other Matrix clients observe (AD-14).
///
/// The sole call site of `Timeline::mark_as_read`. Delegates to
/// `mark_as_read(ReceiptType::Read)`, which internally sends a single receipt on
/// the latest event. Returns `true` when a receipt was actually dispatched, or
/// `false` when there was nothing to mark (an empty timeline / the receipt is
/// already at the latest event) — a benign no-op, not an error. A dispatch
/// failure is best-effort: it surfaces as [`SignalError::Dispatch`] for the caller
/// to log and swallow (no UI error).
pub async fn mark_read(timeline: &Timeline) -> Result<bool, SignalError> {
    // SOLE-RECEIPT-GATE: the one and only `.mark_as_read(` call site (AD-14 guard).
    timeline
        .mark_as_read(ReceiptType::Read)
        .await
        .map_err(|e| SignalError::Dispatch(e.to_string()))
}

/// Set (or clear) the account's typing notice in the room, emitted as a normal
/// (non-private) typing notification (AD-14).
///
/// The sole call site of `Room::typing_notice`. `true` announces typing (the SDK
/// throttles the on-wire re-sends and auto-expires it); `false` stops it. A
/// failure is best-effort — it surfaces as [`SignalError::Dispatch`] for the
/// caller to log and swallow.
pub async fn set_typing(room: &Room, typing: bool) -> Result<(), SignalError> {
    // SOLE-TYPING-EMIT-GATE: the one and only `.typing_notice(` call site (AD-14).
    room.typing_notice(typing)
        .await
        .map_err(|e| SignalError::Dispatch(e.to_string()))
}

/// Subscribe to the room's typing notifications (AD-14).
///
/// The sole call site of `Room::subscribe_to_typing_notifications`. Returns the
/// SDK's [`EventHandlerDropGuard`] (kept alive by the caller's producer so the
/// underlying event handler is unregistered on drop) and a broadcast receiver of
/// the currently-typing user ids. The SDK already filters the account's own user
/// id out of every emitted vector, so the caller renders exactly the *other*
/// members typing. Only opaque user ids flow out — no presence, no avatars, no
/// crypto material (NFR-9, AD-1).
pub fn subscribe_typing(
    room: &Room,
) -> (EventHandlerDropGuard, broadcast::Receiver<Vec<OwnedUserId>>) {
    // SOLE-TYPING-SUBSCRIBE-GATE: the one and only
    // `.subscribe_to_typing_notifications(` call site (AD-14).
    room.subscribe_to_typing_notifications()
}

#[cfg(test)]
mod tests {
    /// AD-14 sole-gate guard (crate-wide): the four SDK receipt/typing
    /// emit/subscribe methods appear in **no** `keeper-core/src/*.rs` file other
    /// than `signals.rs`. This is a stronger invariant than the send-gate's
    /// single-file guard, written crate-wide from the start because AD-14 names
    /// exactly this containment.
    ///
    /// Walks every `.rs` source under `keeper-core/src/` at test time (recursing
    /// into any submodule directory) and asserts each of `.mark_as_read(`,
    /// `.send_single_receipt(`, `.typing_notice(`, and
    /// `.subscribe_to_typing_notifications(` is absent from all of them except
    /// `signals.rs`. Reading the directory (rather than a hand-maintained
    /// `include_str!` list) makes the guard **fail closed**: a newly added module
    /// that calls one of these SDK methods is caught automatically, with no human
    /// step to keep a file list in sync (AD-14 containment).
    ///
    /// Reading already-populated receipts for rendering
    /// (`EventTimelineItem::read_receipts`) is a *different* token
    /// (`.read_receipts(`), so the timeline mapper's render read is not matched.
    #[test]
    fn signals_is_the_sole_receipt_typing_gate() {
        use std::path::{Path, PathBuf};

        // The SDK receipt/typing emit/subscribe call forms guarded by AD-14. Only
        // `signals.rs` may invoke these; every other module must reach the surface
        // through this seam.
        let forbidden = [
            ".mark_as_read(",
            ".send_single_receipt(",
            ".typing_notice(",
            ".subscribe_to_typing_notifications(",
        ];

        // Collect every `.rs` file under the crate's `src/`, recursing into any
        // submodule subdirectory. `signals.rs` is the permitted home and skipped.
        fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read keeper-core src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    collect_rs(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }

        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sources = Vec::new();
        collect_rs(&src_dir, &mut sources);
        assert!(
            !sources.is_empty(),
            "guard scanned no sources — src dir resolution failed"
        );

        for path in sources {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            if name == "signals.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            for pattern in forbidden {
                assert!(
                    !source.contains(pattern),
                    "`{pattern}` must appear only in signals.rs (AD-14 seam), but was found in {name}"
                );
            }
        }
    }
}

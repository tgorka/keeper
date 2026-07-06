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
//! Read receipts are emitted PUBLIC (`m.read`) or PRIVATE (`m.read.private`)
//! depending on the effective Incognito policy resolved *here* at emission time
//! ([`resolve_incognito`], Story 8.1): the pure Chat > Account > Global resolver and
//! the receipt-type branch both live in this seam, so privacy is decided at the one
//! gate — never faked by suppressing the receipt. Typing is emitted normally (typing
//! suppression is Story 8.2).
//! Best-effort: an emit failure is a non-fatal [`SignalError::Dispatch`] the
//! caller swallows for the UI. Secret containment (NFR-9): no token, event id, or
//! plaintext ever crosses here — only opaque user ids flow out of the typing
//! subscription.

use matrix_sdk::event_handler::EventHandlerDropGuard;
use matrix_sdk::ruma::api::client::receipt::create_receipt::v3::ReceiptType;
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::Room;
use matrix_sdk_ui::timeline::Timeline;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use ts_rs::TS;

use crate::error::SignalError;

/// The scope that decided the effective Incognito policy (Story 8.1).
///
/// Serializes camelCase (`"global" | "account" | "chat"`) and is part of the
/// frontend contract via [`IncognitoVm`](crate::vm::IncognitoVm) — it names *which*
/// scope won the Chat > Account > Global precedence, so the header chip can label
/// "this chat overrides account" even when the underlying value equals the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IncognitoScope {
    /// The app-wide default (`settings` key `incognito.global`) decided the policy.
    Global,
    /// A per-Account override (`accounts.incognito`) decided the policy.
    Account,
    /// A per-Chat override (`chat_incognito`) decided the policy.
    Chat,
}

/// The resolved Incognito policy at emission time (Story 8.1).
///
/// `enabled` drives whether [`mark_read`] dispatches a private (`m.read.private`)
/// or public (`m.read`) receipt; `source` is the scope that decided it (for the
/// header chip label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePolicy {
    /// Whether Incognito is effective — `true` ⇒ private receipt, `false` ⇒ public.
    pub enabled: bool,
    /// The scope that decided the outcome under Chat > Account > Global precedence.
    pub source: IncognitoScope,
}

/// Resolve the effective Incognito policy from the three scope values (Story 8.1).
///
/// Pure and deterministic — the whole scope contract lives here. Per-Chat and
/// per-Account are tri-state (`None` = inherit the next-broader scope); Global is a
/// plain bool (off by default). Precedence is Chat > Account > Global: the first set
/// (`Some`) scope, from narrowest to broadest, decides both `enabled` and `source`.
/// All eight combinations are covered by unit tests (the I/O matrix).
pub fn resolve_incognito(
    chat: Option<bool>,
    account: Option<bool>,
    global: bool,
) -> EffectivePolicy {
    if let Some(c) = chat {
        return EffectivePolicy {
            enabled: c,
            source: IncognitoScope::Chat,
        };
    }
    if let Some(a) = account {
        return EffectivePolicy {
            enabled: a,
            source: IncognitoScope::Account,
        };
    }
    EffectivePolicy {
        enabled: global,
        source: IncognitoScope::Global,
    }
}

/// Mark the room's timeline read up to its latest event, dispatching either a
/// PRIVATE `m.read.private` receipt (when the resolved Incognito `policy` is
/// enabled) or a PUBLIC `m.read` receipt (when it is not) (AD-14, Story 8.1).
///
/// The sole call site of `Timeline::mark_as_read`. Branches the [`ReceiptType`] on
/// the effective policy — [`ReceiptType::ReadPrivate`] when Incognito applies (the
/// user's own read position still syncs across their devices, but the remote party's
/// client keeps showing the message unread), else [`ReceiptType::Read`] (unchanged,
/// public behavior). Both go through the *same* `mark_as_read` call, which internally
/// sends a single receipt on the latest event. Returns `true` when a receipt was
/// actually dispatched, or `false` when there was nothing to mark (an empty timeline /
/// the receipt is already at the latest event) — a benign no-op, not an error. A
/// dispatch failure is best-effort: it surfaces as [`SignalError::Dispatch`] for the
/// caller to log and swallow (no UI error).
pub async fn mark_read(timeline: &Timeline, policy: EffectivePolicy) -> Result<bool, SignalError> {
    // SOLE-RECEIPT-GATE: the one and only `.mark_as_read(` call site (AD-14 guard).
    timeline
        .mark_as_read(receipt_type_for(&policy))
        .await
        .map_err(|e| SignalError::Dispatch(e.to_string()))
}

/// Select the receipt type the resolved policy dispatches: the PRIVATE
/// `m.read.private` variant when Incognito is effective, otherwise the PUBLIC
/// `m.read` variant. Pure and side-effect-free so the privacy-critical branch is
/// unit-testable without a live `Timeline` (the actual `mark_as_read` call stays
/// inside [`mark_read`], preserving the AD-14 sole gate).
fn receipt_type_for(policy: &EffectivePolicy) -> ReceiptType {
    if policy.enabled {
        ReceiptType::ReadPrivate
    } else {
        ReceiptType::Read
    }
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
    use super::{
        receipt_type_for, resolve_incognito, EffectivePolicy, IncognitoScope, ReceiptType,
    };

    /// The privacy-critical branch: an enabled policy dispatches the PRIVATE
    /// `m.read.private` receipt; a disabled policy dispatches the PUBLIC `m.read`.
    /// This is the one behavior the whole feature exists for, so it is pinned
    /// directly (the pure resolver feeding it is covered separately above).
    #[test]
    fn receipt_type_follows_effective_policy() {
        let private = receipt_type_for(&EffectivePolicy {
            enabled: true,
            source: IncognitoScope::Chat,
        });
        assert!(
            matches!(private, ReceiptType::ReadPrivate),
            "enabled policy must dispatch m.read.private, got {private:?}"
        );
        let public = receipt_type_for(&EffectivePolicy {
            enabled: false,
            source: IncognitoScope::Global,
        });
        assert!(
            matches!(public, ReceiptType::Read),
            "disabled policy must dispatch public m.read, got {public:?}"
        );
    }

    /// The eight deterministic resolver rows from the spec I/O matrix (2 global ×
    /// 2 account × 2 chat, each broader scope either inherited or overridden). The
    /// resolver is the whole scope contract, so every combination is pinned.
    /// One resolver row: `(chat, account, global) -> (enabled, source)`.
    type ResolverCase = (Option<bool>, Option<bool>, bool, bool, IncognitoScope);

    #[test]
    fn resolve_incognito_covers_all_eight_matrix_rows() {
        let cases: [ResolverCase; 8] = [
            // Global off, all inherit -> (false, Global)
            (None, None, false, false, IncognitoScope::Global),
            // Global on, all inherit -> (true, Global)
            (None, None, true, true, IncognitoScope::Global),
            // Account enables over global-off -> (true, Account)
            (None, Some(true), false, true, IncognitoScope::Account),
            // Account disables over global-on -> (false, Account)
            (None, Some(false), true, false, IncognitoScope::Account),
            // Chat enables over global-off -> (true, Chat)
            (Some(true), None, false, true, IncognitoScope::Chat),
            // Chat disables over global-on -> (false, Chat)
            (Some(false), None, true, false, IncognitoScope::Chat),
            // Chat overrides account (off wins) -> (false, Chat)
            (Some(false), Some(true), false, false, IncognitoScope::Chat),
            // Chat overrides account (on wins) -> (true, Chat)
            (Some(true), Some(false), true, true, IncognitoScope::Chat),
        ];
        for (chat, account, global, want_enabled, want_source) in cases {
            let got = resolve_incognito(chat, account, global);
            assert_eq!(
                got,
                EffectivePolicy {
                    enabled: want_enabled,
                    source: want_source,
                },
                "resolve_incognito(chat={chat:?}, account={account:?}, global={global}) mismatch"
            );
        }
    }

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

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
//! gate — never faked by suppressing the receipt. Typing is gated on the SAME policy
//! (Story 8.2): while Incognito applies, [`set_typing`] emits nothing at all — neither
//! a start nor a stop — so zero `m.typing` events leave the machine.
//! [`release_receipt`] is the explicit user-triggered exception: it dispatches exactly
//! one PUBLIC `m.read` on demand, regardless of policy, reusing the already-gated
//! `mark_as_read(ReceiptType::Read)` call (so it adds no new SDK surface). Presence is
//! never emitted anywhere — keeper has no `set_presence` path (a guardrail test pins
//! that it stays withheld across keeper-core and the keeper IPC crate).
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

/// Whether a typing notice may be emitted under the resolved Incognito `policy`
/// (Story 8.2). Pure and side-effect-free so the suppression gate is unit-testable
/// without a live `Room`: typing leaves the machine only when Incognito is *off*
/// (`!policy.enabled`). When Incognito applies, both start and stop are suppressed —
/// a stop is itself an `m.typing` event on the wire, so honoring "zero typing events"
/// means dropping both (any indicator shown before Incognito was enabled expires via
/// the server's typing timeout).
pub fn should_emit_typing(policy: &EffectivePolicy) -> bool {
    !policy.enabled
}

/// Set (or clear) the account's typing notice in the room, gated on the resolved
/// Incognito `policy` (AD-14, Story 8.2).
///
/// The sole call site of `Room::typing_notice`. When Incognito is effective the
/// notice is suppressed entirely — neither `true` (start) nor `false` (stop) is
/// emitted, so no `m.typing` event leaves the machine (see [`should_emit_typing`]).
/// Otherwise `true` announces typing (the SDK throttles the on-wire re-sends and
/// auto-expires it) and `false` stops it. A failure is best-effort — it surfaces as
/// [`SignalError::Dispatch`] for the caller to log and swallow.
pub async fn set_typing(
    room: &Room,
    typing: bool,
    policy: EffectivePolicy,
) -> Result<(), SignalError> {
    // Incognito effective ⇒ emit nothing (suppress start AND stop), so zero typing
    // events leave the machine.
    if !should_emit_typing(&policy) {
        return Ok(());
    }
    // SOLE-TYPING-EMIT-GATE: the one and only `.typing_notice(` call site (AD-14).
    room.typing_notice(typing)
        .await
        .map_err(|e| SignalError::Dispatch(e.to_string()))
}

/// Dispatch exactly one PUBLIC `m.read` receipt on the timeline's latest event —
/// the explicit, user-triggered read release (AD-14, Story 8.2, FR-45).
///
/// The deliberate exception to the private-by-default path: it *always* emits a
/// public `ReceiptType::Read` regardless of the effective Incognito policy, because
/// the user chose to acknowledge. It reuses the already-gated `mark_as_read`
/// (`ReceiptType::Read`) call — the same PUBLIC branch [`mark_read`] uses when
/// Incognito is off — so it adds no new SDK surface to the sole gate. Returns `true`
/// when a receipt was actually dispatched, or `false` for a benign no-op (empty
/// timeline / receipt already at the latest event). A dispatch failure is best-effort:
/// it surfaces as [`SignalError::Dispatch`] for the caller to log and swallow.
pub async fn release_receipt(timeline: &Timeline) -> Result<bool, SignalError> {
    // SOLE-RECEIPT-GATE: reuses the one and only `.mark_as_read(` call site (AD-14),
    // forcing the PUBLIC receipt type — the user's explicit release is never private.
    timeline
        .mark_as_read(ReceiptType::Read)
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
        receipt_type_for, resolve_incognito, should_emit_typing, EffectivePolicy, IncognitoScope,
        ReceiptType,
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

    /// The pure typing-suppression gate (Story 8.2). `should_emit_typing` takes no
    /// `typing` argument — it is `typing`-independent *by construction*, a function of
    /// the effective policy alone: it returns `false` when Incognito is enabled and
    /// `true` when it is disabled. The four typing I/O-matrix rows (suppress both start
    /// and stop while on; emit both while off) follow from this single decision plus
    /// [`set_typing`] early-returning whenever this gate is `false` — so both the
    /// `true` (start) and `false` (stop) calls are dropped without the gate ever seeing
    /// the `typing` bool. Pinning the enabled-vs-disabled outcome here is the whole
    /// contract; no duplicated identical assertion is needed to "cover" a bool the gate
    /// does not take.
    #[test]
    fn should_emit_typing_follows_effective_policy() {
        // Incognito effective ⇒ no typing leaves the machine (start AND stop suppressed,
        // since the gate is checked before either is emitted). Spot-checked once per
        // source variant.
        assert!(
            !should_emit_typing(&EffectivePolicy {
                enabled: true,
                source: IncognitoScope::Chat,
            }),
            "Incognito effective (chat) must gate typing off"
        );
        assert!(
            !should_emit_typing(&EffectivePolicy {
                enabled: true,
                source: IncognitoScope::Account,
            }),
            "Incognito effective (account) must gate typing off"
        );
        // Incognito off ⇒ typing may be emitted (unchanged behavior — both start and
        // stop pass the gate).
        assert!(
            should_emit_typing(&EffectivePolicy {
                enabled: false,
                source: IncognitoScope::Global,
            }),
            "Incognito off must allow typing to be emitted"
        );
    }

    /// Presence-withheld guardrail (Story 8.2, FR-43): keeper emits NO presence
    /// anywhere. Matrix presence is a per-user (account-global) signal that per-Chat
    /// Incognito cannot scope, so "withheld where the protocol allows" is satisfied by
    /// having no presence-emit path at all. This walks every `.rs` source under BOTH
    /// `keeper-core/src/` (this crate) AND the sibling `keeper/src/` (the IPC crate,
    /// where a `set_presence` Tauri command would otherwise slip past a keeper-core-only
    /// scan) and asserts neither the presence emit-call form nor the presence enum type
    /// name appears in any of them — so a future change that silently adds a presence
    /// emission in either crate is caught automatically, fail-closed. The two guarded
    /// tokens are spelled split (built by concatenation below) so this file does not trip
    /// its own scan. The sibling `keeper/src` directory must resolve at runtime — a
    /// missing dir fails the scan loudly rather than silently skipping the IPC crate.
    #[test]
    fn presence_is_withheld_everywhere() {
        use std::path::{Path, PathBuf};

        // Build the forbidden tokens by concatenation so the literals do NOT appear as
        // contiguous substrings *in this very file* — the scan below covers signals.rs
        // too, and spelling them out here would false-positive against itself.
        let set_presence = format!(".set_{}(", "presence");
        let presence_state = format!("Presence{}", "State");
        let forbidden = [set_presence.as_str(), presence_state.as_str()];

        fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read crate src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    collect_rs(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }

        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        // This crate (keeper-core/src) AND the sibling IPC crate (keeper/src). Both must
        // stay presence-clean; the crate-wide guarantee spans keeper-core and keeper.
        let core_src = manifest.join("src");
        let ipc_src = manifest.join("../keeper/src");
        assert!(
            ipc_src.is_dir(),
            "sibling IPC crate src dir did not resolve at {} — the presence guard must scan it, not silently skip it",
            ipc_src.display()
        );
        let mut sources = Vec::new();
        collect_rs(&core_src, &mut sources);
        collect_rs(&ipc_src, &mut sources);
        assert!(
            !sources.is_empty(),
            "guard scanned no sources — src dir resolution failed"
        );

        for path in &sources {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            for pattern in forbidden {
                assert!(
                    !source.contains(pattern),
                    "`{pattern}` must appear nowhere under keeper-core/src or keeper/src (presence stays withheld), but was found in {name}"
                );
            }
        }
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

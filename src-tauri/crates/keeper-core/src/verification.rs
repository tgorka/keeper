//! Interactive device self-verification — emoji/SAS + QR display (Story 3.2,
//! FR-14, AD-1, NFR-9).
//!
//! This module owns **every** verification SDK call, flow id, SAS/QR crypto, and
//! key material for a self-verification flow. The webview receives only a rendered
//! [`VerificationFlowVm`] (emoji symbols + names, a QR SVG string, a phase, and a
//! cancel reason) — never a `Verification`/`SasVerification`/`QrVerification`
//! object, SAS key, decimal, or plaintext (AD-1).
//!
//! Two directions are supported, as FR-14 requires:
//! * **keeper starts** — [`start`] resolves the own-user identity and calls
//!   `request_verification()`, then feeds the new flow id into the producer.
//! * **the other session starts** — [`run_producer`] registers a to-device event
//!   handler for `m.key.verification.request`; an incoming self-verification
//!   request's flow id is fed into the same producer, which surfaces it as a
//!   `Requested` `VerificationFlowVm` so the UI can auto-open and Accept.
//!
//! The producer drives exactly one active flow at a time: it drains a channel of
//! flow ids, resolves the `VerificationRequest`, drives its `changes()` stream,
//! transitions into the `SasVerification`'s `changes()` stream when the request
//! becomes `Verification::SasV1`, generates a QR SVG when `Ready`, maps each state
//! into a [`VerificationFlowVm`], and emits it through the sink. A terminal
//! `Done`/`Cancelled`/`Failed` ends the active flow; the producer then waits for
//! the next incoming request.
//!
//! QR is **display only** (the peer scans keeper's QR). Camera-based live QR
//! scanning is deliberately out of scope for the desktop MVP (see the story's
//! Never); SAS is the guaranteed path in both directions.

use futures_util::StreamExt;
use matrix_sdk::encryption::verification::{
    CancelInfo, QrVerification, SasState, SasVerification, Verification, VerificationRequest,
    VerificationRequestState,
};
use matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent;
use matrix_sdk::Client;
use tokio::sync::mpsc;

use crate::error::{CoreError, VerificationError};
use crate::vm::{SasEmojiVm, VerificationFlowVm, VerificationPhase};

/// Sink that receives each produced [`VerificationFlowVm`]. The shell wraps a
/// Tauri `Channel::send`; tests capture into a vector. Returns `true` if the
/// snapshot was delivered, `false` if the channel is closed (the producer stops).
pub type VerificationSink = Box<dyn Fn(VerificationFlowVm) -> bool + Send + Sync>;

/// Resolve the own-user identity and request an interactive self-verification,
/// returning the new flow id (Story 3.2, FR-14). The caller feeds this flow id
/// into the producer so the stream picks it up and drives it.
///
/// Errors: no crypto identity yet / a request-verification failure map to
/// [`VerificationError`].
pub async fn start(client: &Client) -> Result<String, CoreError> {
    let own_user_id = client.user_id().ok_or_else(|| {
        VerificationError::Unavailable("no signed-in user id on the client".to_owned())
    })?;
    let identity = client
        .encryption()
        .get_user_identity(own_user_id)
        .await
        .map_err(|e| VerificationError::Unavailable(e.to_string()))?
        .ok_or_else(|| {
            VerificationError::Unavailable(
                "no cross-signing identity for this account yet".to_owned(),
            )
        })?;
    let request = identity
        .request_verification()
        .await
        .map_err(|e| VerificationError::Action(e.to_string()))?;
    let flow_id = request.flow_id().to_owned();
    tracing::info!(%flow_id, "self-verification requested from keeper");
    Ok(flow_id)
}

/// Accept an incoming verification request by `flow_id` (the peer started it).
pub async fn accept(client: &Client, flow_id: &str) -> Result<(), CoreError> {
    let request = resolve_request(client, flow_id).await?;
    request
        .accept()
        .await
        .map_err(|e| VerificationError::Action(e.to_string()))?;
    Ok(())
}

/// Start the emoji/SAS sub-flow on the ready request `flow_id`.
pub async fn start_sas(client: &Client, flow_id: &str) -> Result<(), CoreError> {
    let request = resolve_request(client, flow_id).await?;
    // Returns `Ok(None)` if SAS is not available; that is not an error — the flow
    // simply stays on QR. The transition (if any) arrives via `changes()`.
    request
        .start_sas()
        .await
        .map_err(|e| VerificationError::Action(e.to_string()))?;
    Ok(())
}

/// Confirm that the SAS emoji match on `flow_id` (our side).
pub async fn confirm(client: &Client, flow_id: &str) -> Result<(), CoreError> {
    let sas = resolve_sas(client, flow_id).await?;
    sas.confirm()
        .await
        .map_err(|e| VerificationError::Action(e.to_string()))?;
    Ok(())
}

/// Signal that the SAS emoji do **not** match on `flow_id` — cancels the flow
/// with the SDK's mismatch code (surfaces as `Failed`).
pub async fn mismatch(client: &Client, flow_id: &str) -> Result<(), CoreError> {
    let sas = resolve_sas(client, flow_id).await?;
    sas.mismatch()
        .await
        .map_err(|e| VerificationError::Action(e.to_string()))?;
    Ok(())
}

/// Cancel the flow `flow_id` (user closed the modal / pressed Esc). Cancels the
/// active SAS if one exists, else the request. A missing flow is a no-op — the
/// flow may already be terminal.
pub async fn cancel(client: &Client, flow_id: &str) -> Result<(), CoreError> {
    let own_user_id = client.user_id().ok_or_else(|| {
        VerificationError::Unavailable("no signed-in user id on the client".to_owned())
    })?;
    if let Some(Verification::SasV1(sas)) = client
        .encryption()
        .get_verification(own_user_id, flow_id)
        .await
    {
        sas.cancel()
            .await
            .map_err(|e| VerificationError::Action(e.to_string()))?;
        return Ok(());
    }
    if let Some(request) = client
        .encryption()
        .get_verification_request(own_user_id, flow_id)
        .await
    {
        request
            .cancel()
            .await
            .map_err(|e| VerificationError::Action(e.to_string()))?;
    }
    Ok(())
}

/// Resolve a live `VerificationRequest` by flow id, or a typed error.
async fn resolve_request(client: &Client, flow_id: &str) -> Result<VerificationRequest, CoreError> {
    let own_user_id = client.user_id().ok_or_else(|| {
        VerificationError::Unavailable("no signed-in user id on the client".to_owned())
    })?;
    client
        .encryption()
        .get_verification_request(own_user_id, flow_id)
        .await
        .ok_or_else(|| VerificationError::FlowNotFound.into())
}

/// Resolve a live `SasVerification` by flow id, or a typed error.
async fn resolve_sas(client: &Client, flow_id: &str) -> Result<SasVerification, CoreError> {
    let own_user_id = client.user_id().ok_or_else(|| {
        VerificationError::Unavailable("no signed-in user id on the client".to_owned())
    })?;
    match client
        .encryption()
        .get_verification(own_user_id, flow_id)
        .await
    {
        Some(Verification::SasV1(sas)) => Ok(sas),
        _ => Err(VerificationError::FlowNotFound.into()),
    }
}

/// Per-account verification producer (Story 3.2). Observes both incoming
/// self-verification requests (via a to-device event handler) and flow ids fed in
/// through `flow_rx` (a request keeper started), and drives whichever flow is
/// active into a stream of [`VerificationFlowVm`] snapshots through `sink`.
///
/// Runs until the sink reports the channel is closed (the frontend unsubscribed),
/// at which point the event handler is removed and the task ends. There is exactly
/// one active flow at a time.
pub async fn run_producer(
    client: Client,
    sink: VerificationSink,
    mut flow_rx: mpsc::UnboundedReceiver<String>,
    account_id: &str,
) {
    // Register a to-device handler for incoming `m.key.verification.request`
    // events (the other session started verification). It only forwards the flow
    // id; the producer resolves and drives the request. Self-verifications carry
    // our own user id as the sender.
    let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel::<String>();
    let own_user_id = client.user_id().map(|u| u.to_owned());
    let handler_tx = incoming_tx.clone();
    let handler = client.add_event_handler(move |event: ToDeviceKeyVerificationRequestEvent| {
        let tx = handler_tx.clone();
        let own = own_user_id.clone();
        async move {
            // Only surface self-verification requests (sender is us).
            if own.as_deref() == Some(event.sender.as_ref()) {
                let flow_id = event.content.transaction_id.to_string();
                tracing::info!(%flow_id, "incoming self-verification request observed");
                let _ = tx.send(flow_id);
            }
        }
    });

    loop {
        // Wait for the next flow id from either source (keeper-started or
        // incoming). If both channels close, the producer ends.
        let flow_id = tokio::select! {
            id = flow_rx.recv() => id,
            id = incoming_rx.recv() => id,
        };
        let Some(flow_id) = flow_id else {
            break;
        };

        if !drive_flow(&client, &sink, &flow_id, account_id).await {
            // The sink is closed — the frontend unsubscribed; stop the producer.
            break;
        }
    }

    client.remove_event_handler(handler);
    tracing::info!(account_id = %account_id, "verification producer ended");
}

/// Drive one flow (identified by `flow_id`) to a terminal state, emitting a
/// [`VerificationFlowVm`] snapshot on every transition. Transitions into the SAS
/// `changes()` stream when the request becomes `Verification::SasV1`. Returns
/// `false` if the sink closed (the caller then stops the producer), `true` when
/// the flow reached a terminal state normally.
async fn drive_flow(
    client: &Client,
    sink: &VerificationSink,
    flow_id: &str,
    account_id: &str,
) -> bool {
    let Some(request) = resolve_request_opt(client, flow_id).await else {
        tracing::warn!(%flow_id, account_id = %account_id, "flow id had no live request; skipping");
        return true;
    };

    // Emit the current request state first, then follow changes.
    if !emit_request_state(sink, flow_id, &request.state(), &request).await {
        return false;
    }

    let mut changes = request.changes();
    while let Some(state) = changes.next().await {
        match &state {
            VerificationRequestState::Transitioned { verification } => match verification {
                Verification::SasV1(sas) => {
                    // Hand off to the SAS stream for the rest of the flow.
                    return drive_sas(sink, flow_id, sas).await;
                }
                Verification::QrV1(_) => {
                    // A QR verification the *peer* scanned/confirmed drives itself
                    // to done; surface it as confirmed-then-done via the request
                    // terminal states below. Keep following request changes.
                    if !emit_request_state(sink, flow_id, &state, &request).await {
                        return false;
                    }
                }
                // `Verification` is `#[non_exhaustive]`; surface any future flow
                // variant via the request state rather than dropping the change.
                _ => {
                    if !emit_request_state(sink, flow_id, &state, &request).await {
                        return false;
                    }
                }
            },
            _ => {
                if !emit_request_state(sink, flow_id, &state, &request).await {
                    return false;
                }
                if matches!(
                    state,
                    VerificationRequestState::Done | VerificationRequestState::Cancelled(_)
                ) {
                    // Terminal request state (no SAS sub-flow was entered).
                    return true;
                }
            }
        }
    }
    // The request stream ended without a terminal state or a SAS hand-off (e.g. the
    // SDK dropped the request) — surface an honest failure so the modal never hangs
    // on "waiting".
    emit_stream_ended(sink, flow_id)
}

/// Resolve a request by flow id, returning `None` (not an error) if absent.
async fn resolve_request_opt(client: &Client, flow_id: &str) -> Option<VerificationRequest> {
    let own_user_id = client.user_id()?;
    client
        .encryption()
        .get_verification_request(own_user_id, flow_id)
        .await
}

/// Map a request state into a [`VerificationFlowVm`] and emit it, generating a QR
/// SVG when the request is `Ready`. Returns `false` if the sink closed.
async fn emit_request_state(
    sink: &VerificationSink,
    flow_id: &str,
    state: &VerificationRequestState,
    request: &VerificationRequest,
) -> bool {
    let phase = map_request_state(state);
    let (reason, qr_code_svg) = match state {
        VerificationRequestState::Cancelled(info) => (map_cancel_reason(info).1, None),
        VerificationRequestState::Ready { .. } => {
            // Best-effort keeper QR for the peer to scan; SAS still offered if none.
            let svg = match request.generate_qr_code().await {
                Ok(Some(qr)) => qr_to_svg(&qr),
                Ok(None) => None,
                Err(e) => {
                    tracing::debug!(%flow_id, error = %e, "QR code generation failed; SAS only");
                    None
                }
            };
            (None, svg)
        }
        _ => (None, None),
    };
    (sink)(VerificationFlowVm {
        flow_id: flow_id.to_owned(),
        phase,
        emojis: None,
        qr_code_svg,
        reason,
    })
}

/// Drive the SAS sub-flow to a terminal state, emitting a snapshot on every
/// change (with the 7 emoji when keys are exchanged). Returns `false` if the sink
/// closed, `true` when the SAS reached a terminal state.
async fn drive_sas(sink: &VerificationSink, flow_id: &str, sas: &SasVerification) -> bool {
    if !emit_sas_state(sink, flow_id, &sas.state(), sas) {
        return false;
    }
    let mut changes = sas.changes();
    while let Some(state) = changes.next().await {
        if !emit_sas_state(sink, flow_id, &state, sas) {
            return false;
        }
        if matches!(state, SasState::Done { .. } | SasState::Cancelled(_)) {
            return true;
        }
    }
    // The SAS stream ended without a terminal state — surface an honest failure so
    // the comparing UI never hangs with dead match / no-match buttons.
    emit_stream_ended(sink, flow_id)
}

/// Emit an honest terminal `Failed` when a verification stream ends without ever
/// reaching a Done/Cancelled state (the SDK dropped the flow). Returns `false` if
/// the sink closed.
fn emit_stream_ended(sink: &VerificationSink, flow_id: &str) -> bool {
    (sink)(VerificationFlowVm {
        flow_id: flow_id.to_owned(),
        phase: VerificationPhase::Failed,
        emojis: None,
        qr_code_svg: None,
        reason: Some("Verification ended unexpectedly.".to_owned()),
    })
}

/// Map a SAS state into a [`VerificationFlowVm`] and emit it. The emoji list is
/// read from `sas.emoji()` (the SDK's rendered short-auth string) in the
/// `KeysExchanged` phase; no SAS key or decimal ever crosses IPC. Returns `false`
/// if the sink closed.
fn emit_sas_state(
    sink: &VerificationSink,
    flow_id: &str,
    state: &SasState,
    sas: &SasVerification,
) -> bool {
    let (phase, want_emojis) = map_sas_state(state);
    let emojis = if want_emojis {
        sas.emoji().map(emojis_to_vms)
    } else {
        None
    };
    let reason = match state {
        SasState::Cancelled(info) => map_cancel_reason(info).1,
        _ => None,
    };
    (sink)(VerificationFlowVm {
        flow_id: flow_id.to_owned(),
        phase,
        emojis,
        qr_code_svg: None,
        reason,
    })
}

/// Convert the SDK's `[Emoji; 7]` into the rendered [`SasEmojiVm`] list (symbol +
/// name). Pure; the emoji symbol/description are non-secret display strings.
fn emojis_to_vms(emojis: [matrix_sdk::encryption::verification::Emoji; 7]) -> Vec<SasEmojiVm> {
    emojis
        .iter()
        .map(|e| SasEmojiVm {
            symbol: e.symbol.to_owned(),
            name: e.description.to_owned(),
        })
        .collect()
}

/// Pure mapping of a [`VerificationRequestState`] to a [`VerificationPhase`].
/// `Created`/`Requested` are pre-ready waiting states; `Ready` offers QR + SAS;
/// `Transitioned` means a concrete flow (SAS) is underway (comparing);
/// `Done`/`Cancelled` are terminal. A `Cancelled` maps to `Cancelled`/`Failed`
/// via [`map_cancel_reason`].
pub fn map_request_state(state: &VerificationRequestState) -> VerificationPhase {
    match state {
        VerificationRequestState::Created { .. } | VerificationRequestState::Requested { .. } => {
            VerificationPhase::Requested
        }
        VerificationRequestState::Ready { .. } => VerificationPhase::Ready,
        VerificationRequestState::Transitioned { .. } => VerificationPhase::Comparing,
        VerificationRequestState::Done => VerificationPhase::Done,
        VerificationRequestState::Cancelled(info) => map_cancel_reason(info).0,
    }
}

/// Pure mapping of a [`SasState`] to a [`VerificationPhase`] plus whether the
/// emoji list should be attached (only in `KeysExchanged`). `Created`/`Started`/
/// `Accepted` are pre-emoji waiting states (comparing UI still shows "waiting");
/// `KeysExchanged` is the emoji-compare phase; `Confirmed` means we confirmed and
/// await the peer; `Done`/`Cancelled` are terminal.
pub fn map_sas_state(state: &SasState) -> (VerificationPhase, bool) {
    match state {
        SasState::Created { .. } | SasState::Started { .. } | SasState::Accepted { .. } => {
            (VerificationPhase::Comparing, false)
        }
        SasState::KeysExchanged { .. } => (VerificationPhase::Comparing, true),
        SasState::Confirmed => (VerificationPhase::Confirmed, false),
        SasState::Done { .. } => (VerificationPhase::Done, false),
        // A terminal cancel/failure carries no emoji.
        SasState::Cancelled(info) => (map_cancel_reason(info).0, false),
    }
}

/// Pure mapping of a terminal [`CancelInfo`] to a phase + optional reason. **Only**
/// the `m.user` code (a clean user/peer dismissal — our Cancel/Esc or the peer's)
/// maps to `Cancelled` with no reason; every other terminal code (mismatch, key
/// mismatch, timeout, …) maps to `Failed` carrying the SDK's human-readable reason.
/// Note this deliberately does **not** key on `cancelled_by_us()`: a SAS emoji
/// mismatch is `cancelled_by_us()` yet must surface as `Failed` (see below).
pub fn map_cancel_reason(info: &CancelInfo) -> (VerificationPhase, Option<String>) {
    // Only a clean user-initiated dismissal (our Cancel/Esc or the peer's — both
    // carry the `m.user` code) is a benign `Cancelled`. Every other terminal
    // code is a `Failed` with a reason so the UI can tell "cancelled" from "went
    // wrong". Crucially this includes a SAS emoji **mismatch** (`m.mismatched_sas`,
    // which `sas.mismatch()` sends and is therefore `cancelled_by_us()`): a
    // detected mismatch is a security-relevant failure, not a soft cancel, so we
    // must not let `cancelled_by_us()` short-circuit it into `Cancelled`.
    if info.cancel_code().as_str() == "m.user" {
        (VerificationPhase::Cancelled, None)
    } else {
        (VerificationPhase::Failed, Some(info.reason().to_owned()))
    }
}

/// Render a [`QrVerification`] to a self-contained SVG string via the `qrcode`
/// crate, or `None` if the QR could not be built. QR *display* only — the webview
/// renders the returned SVG as an image and never decodes QR crypto (Story 3.2).
pub fn qr_to_svg(qr: &QrVerification) -> Option<String> {
    match qr.to_qr_code() {
        Ok(code) => Some(
            code.render::<qrcode::render::svg::Color>()
                .min_dimensions(200, 200)
                .quiet_zone(true)
                .build(),
        ),
        Err(e) => {
            tracing::debug!(error = %e, "could not build QR code from verification");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::VerificationPhase;

    #[test]
    fn map_request_state_maps_done_to_done() {
        // Only the payload-free `Done` variant is unit-assertable: `Requested`/
        // `Ready`/`Transitioned` carry non-constructible payloads (DeviceData /
        // Verification) and `Cancelled` needs a `CancelInfo` (private fields, no
        // public ctor). The Cancelled→Cancelled/Failed split in `map_cancel_reason`
        // and the Ready/Transitioned mappings are therefore covered by the manual
        // second-session check documented in the spec, not by this unit test.
        assert_eq!(
            map_request_state(&VerificationRequestState::Done),
            VerificationPhase::Done
        );
    }

    #[test]
    fn map_sas_state_confirmed_is_confirmed_without_emojis() {
        let (phase, want_emojis) = map_sas_state(&SasState::Confirmed);
        assert_eq!(phase, VerificationPhase::Confirmed);
        assert!(!want_emojis, "confirmed carries no emoji");
    }
}

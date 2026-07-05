//! The generic native-bridge-login driver (Story 6.3, FR-26, AD-16).
//!
//! [`drive_login`] runs one login's state loop over any [`BridgeTransport`],
//! statically dispatched (`drive_login<T: BridgeTransport>`) — no trait object, so
//! Story 6.4's `BotDriver` slots in behind the same driver with zero changes. It
//! translates each transport [`LoginStep`] into a [`BridgeLoginVm`] via the pure
//! [`step_to_vm`], emits it through the [`BridgeLoginSink`], and pumps user input
//! (a flow choice or field values) in from an `mpsc` channel:
//!
//! - more than one flow → emit `ChoosingMethod`, wait for a
//!   [`BridgeLoginInput::ChooseFlow`]; exactly one → auto-start it.
//! - `display_and_wait` → emit `Qr`/`Waiting`, then long-poll the same step; a
//!   *fresh* `display_and_wait` before `complete` is a QR rotation (`qr_refreshed`).
//! - `user_input` → emit `CodeEntry`, wait for a [`BridgeLoginInput::Fields`],
//!   submit it, follow the next step.
//! - `complete` → emit `Success` (terminal).
//! - `cookies` / `webauthn` / unknown → emit the distinct **unsupported-method**
//!   failure (no webview, no fake success) and stop.
//! - any transport error → emit `Failure` with the bridge's verbatim message.
//!
//! The QR is rendered to an SVG string in Rust (the `qrcode` crate, `svg` feature —
//! exactly Story 3.2's verification QR pattern); the frontend renders it as an
//! `<img src="data:image/svg+xml,…">`, never a JS QR lib.

use std::collections::BTreeMap;

use tokio::sync::mpsc;

use crate::bridges::transport::{BridgeTransport, DisplayData, LoginStep};
use crate::vm::{BridgeLoginInput, BridgeLoginPhase, BridgeLoginVm, LoginFieldVm, LoginFlowVm};

/// Sink that receives each produced [`BridgeLoginVm`] snapshot. The shell wraps a
/// Tauri `Channel::send`; tests capture into a vector. Returns `true` if delivered,
/// `false` if the channel is closed (the driver then stops).
pub type BridgeLoginSink = Box<dyn Fn(BridgeLoginVm) -> bool + Send + Sync>;

/// Render a QR payload to a self-contained SVG string via the `qrcode` crate,
/// mirroring the verification QR pattern (Story 3.2). Returns `None` if the payload
/// can't be encoded (the caller then surfaces an honest failure rather than a blank
/// panel). The white card + quiet zone are the frontend's responsibility; the SVG
/// carries a quiet zone and a minimum size for scannability.
pub fn qr_svg(data: &str) -> Option<String> {
    match qrcode::QrCode::new(data.as_bytes()) {
        Ok(code) => Some(
            code.render::<qrcode::render::svg::Color>()
                .min_dimensions(240, 240)
                .quiet_zone(true)
                .build(),
        ),
        Err(e) => {
            tracing::debug!(error = %e, "could not build QR code for bridge login");
            None
        }
    }
}

/// Pure translation of one transport [`LoginStep`] into a [`BridgeLoginVm`] for
/// `network_id`. `refreshed` sets [`BridgeLoginVm::qr_refreshed`] (a fresh QR during
/// an already-active QR phase). A `cookies`/`webauthn`/unknown step maps to the
/// distinct unsupported-method `Failure`; a `complete` step maps to `Success`.
///
/// The QR payload → SVG rendering happens here (via [`qr_svg`]); a QR whose payload
/// can't be encoded becomes an honest `Failure` rather than a blank QR panel.
pub fn step_to_vm(network_id: &str, step: &LoginStep, refreshed: bool) -> BridgeLoginVm {
    let base = |phase: BridgeLoginPhase| BridgeLoginVm {
        network_id: network_id.to_owned(),
        phase,
        instruction: None,
        qr_svg: None,
        qr_refreshed: false,
        fields: Vec::new(),
        flows: Vec::new(),
        error: None,
    };

    match step {
        LoginStep::DisplayAndWait {
            display_and_wait, ..
        } => match display_and_wait {
            DisplayData::Qr { data, .. } => match data.as_deref().and_then(qr_svg) {
                Some(svg) => BridgeLoginVm {
                    phase: BridgeLoginPhase::Qr,
                    instruction: Some(format!(
                        "Scan this QR code with {} on your phone.",
                        display_network_name(network_id)
                    )),
                    qr_svg: Some(svg),
                    qr_refreshed: refreshed,
                    ..base(BridgeLoginPhase::Qr)
                },
                None => BridgeLoginVm {
                    error: Some(
                        "The bridge sent a login QR keeper couldn't render. Try again.".to_owned(),
                    ),
                    ..base(BridgeLoginPhase::Failure)
                },
            },
            DisplayData::Code { data } => BridgeLoginVm {
                instruction: Some(match data {
                    Some(code) => format!("Enter this code on your other device: {code}"),
                    None => "Waiting for the bridge…".to_owned(),
                }),
                ..base(BridgeLoginPhase::Waiting)
            },
            DisplayData::Emoji { .. } | DisplayData::Nothing => BridgeLoginVm {
                instruction: Some("Waiting for the bridge to confirm…".to_owned()),
                ..base(BridgeLoginPhase::Waiting)
            },
        },
        LoginStep::UserInput { fields, .. } => BridgeLoginVm {
            instruction: Some("Enter the requested details to continue.".to_owned()),
            fields: fields
                .iter()
                .map(|f| LoginFieldVm {
                    id: f.id.clone(),
                    field_type: f.field_type.clone(),
                    name: f.name.clone(),
                    description: f.description.clone(),
                    pattern: f.pattern.clone(),
                    default_value: f.default_value.clone(),
                })
                .collect(),
            ..base(BridgeLoginPhase::CodeEntry)
        },
        LoginStep::Complete { .. } => BridgeLoginVm {
            instruction: Some("Linked ✓".to_owned()),
            ..base(BridgeLoginPhase::Success)
        },
        LoginStep::Cookies { .. } => unsupported_vm(network_id, "browser sign-in"),
        LoginStep::Webauthn { .. } => unsupported_vm(network_id, "a passkey / security key"),
        LoginStep::Unknown => unsupported_vm(network_id, "an unsupported login method"),
    }
}

/// Build the distinct unsupported-method failure VM — honest copy that names the
/// Bridge Bot chat as the manual path (Story 6.4 wires the actual navigation; here
/// we only name it), never a half-built webview or a fake success.
fn unsupported_vm(network_id: &str, method: &str) -> BridgeLoginVm {
    BridgeLoginVm {
        network_id: network_id.to_owned(),
        phase: BridgeLoginPhase::Failure,
        instruction: None,
        qr_svg: None,
        qr_refreshed: false,
        fields: Vec::new(),
        flows: Vec::new(),
        error: Some(format!(
            "This network needs {method}, which keeper can't do natively yet. \
             You can still log in from the Bridge Bot chat."
        )),
    }
}

/// The display name for the network id, for the QR instruction line. Resolves the
/// canonical name from the 6.1 catalog (which carries the exact display name, e.g.
/// "WhatsApp"); falls back to a simple title-case of the id only when the id isn't
/// in the catalog or the catalog fails to load. `catalog()` reads embedded,
/// already-cached data, so the lookup is cheap.
fn display_network_name(network_id: &str) -> String {
    if let Ok(networks) = crate::bridges::catalog() {
        if let Some(vm) = networks.iter().find(|n| n.network_id == network_id) {
            return vm.name.clone();
        }
    }
    let mut chars = network_id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => network_id.to_owned(),
    }
}

/// Emit a `Waiting` VM (used before the first step is fetched).
fn waiting_vm(network_id: &str, instruction: &str) -> BridgeLoginVm {
    BridgeLoginVm {
        network_id: network_id.to_owned(),
        phase: BridgeLoginPhase::Waiting,
        instruction: Some(instruction.to_owned()),
        qr_svg: None,
        qr_refreshed: false,
        fields: Vec::new(),
        flows: Vec::new(),
        error: None,
    }
}

/// Emit a `Failure` VM carrying the bridge's verbatim message.
fn failure_vm(network_id: &str, error: String) -> BridgeLoginVm {
    BridgeLoginVm {
        network_id: network_id.to_owned(),
        phase: BridgeLoginPhase::Failure,
        instruction: None,
        qr_svg: None,
        qr_refreshed: false,
        fields: Vec::new(),
        flows: Vec::new(),
        error: Some(error),
    }
}

/// Drive one bridge login over `transport` to a terminal state, emitting a
/// [`BridgeLoginVm`] on every transition and pumping user input from `input_rx`.
///
/// Generic and statically dispatched (no trait object). Runs until a terminal state
/// (`Success` / `Failure`), the sink closes (the frontend unsubscribed), or the
/// input channel closes (the session was cancelled). On any exit the driver simply
/// returns — it does **not** post `/login/cancel`. The `login_id_slot` is populated
/// with the login id once `login_start` succeeds so that an *explicit* cancel
/// ([`crate::account::AccountManager::cancel_bridge_login`]) can best-effort POST
/// `/login/cancel/{login_id}` before aborting this task.
pub async fn drive_login<T: BridgeTransport>(
    transport: T,
    network_id: &str,
    sink: BridgeLoginSink,
    mut input_rx: mpsc::UnboundedReceiver<BridgeLoginInput>,
    login_id_slot: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    // Emit an initial waiting state so the Sheet opens on "Connecting…".
    if !(sink)(waiting_vm(network_id, "Connecting…")) {
        return;
    }

    // Resolve the flow to start: >1 flow requires a user choice.
    let flows = match transport.login_flows().await {
        Ok(flows) => flows,
        Err(e) => {
            (sink)(failure_vm(network_id, e.to_string()));
            return;
        }
    };

    let flow_id = if flows.len() > 1 {
        // Offer the choice and wait for it.
        let vm = BridgeLoginVm {
            network_id: network_id.to_owned(),
            phase: BridgeLoginPhase::ChoosingMethod,
            instruction: Some("Choose how to sign in.".to_owned()),
            qr_svg: None,
            qr_refreshed: false,
            fields: Vec::new(),
            flows: flows
                .iter()
                .map(|f| LoginFlowVm {
                    id: f.id.clone(),
                    name: f.name.clone(),
                    description: f.description.clone(),
                })
                .collect(),
            error: None,
        };
        if !(sink)(vm) {
            return;
        }
        loop {
            match input_rx.recv().await {
                Some(BridgeLoginInput::ChooseFlow { flow_id }) => break flow_id,
                // A stray field submit while choosing is ignored (wait for a choice).
                Some(BridgeLoginInput::Fields { .. }) => continue,
                // The session was cancelled before a choice.
                None => return,
            }
        }
    } else if let Some(only) = flows.into_iter().next() {
        only.id
    } else {
        (sink)(failure_vm(
            network_id,
            "This bridge offers no login methods.".to_owned(),
        ));
        return;
    };

    // Start the chosen flow.
    let mut current = match transport.login_start(&flow_id).await {
        Ok(resp) => resp,
        Err(e) => {
            (sink)(failure_vm(network_id, e.to_string()));
            return;
        }
    };

    // Record the login id (stable for the whole flow) so an explicit cancel can
    // best-effort POST `/login/cancel/{login_id}`. A poisoned lock here is
    // ignored (best-effort) rather than panicking on this internal write path.
    if let Ok(mut slot) = login_id_slot.lock() {
        *slot = Some(current.login_id.clone());
    }

    // Track the previous phase so a repeated QR reads as a refresh.
    let mut was_qr = false;

    loop {
        let refreshed = was_qr && step_is_qr(&current.step);
        let vm = step_to_vm(network_id, &current.step, refreshed);
        let phase = vm.phase;
        if !(sink)(vm) {
            return;
        }
        was_qr = matches!(phase, BridgeLoginPhase::Qr);

        match &current.step {
            // Terminal states — stop driving.
            LoginStep::Complete { .. }
            | LoginStep::Cookies { .. }
            | LoginStep::Webauthn { .. }
            | LoginStep::Unknown => return,

            // A QR/code/emoji/nothing wait: long-poll the same step for the next
            // transition (a fresh display = rotation; a `complete` = success).
            LoginStep::DisplayAndWait { step_id, .. } => {
                let step_id = step_id.clone();
                let login_id = current.login_id.clone();
                let empty = BTreeMap::new();
                match transport
                    .login_step(&login_id, &step_id, "display_and_wait", &empty)
                    .await
                {
                    Ok(next) => current = next,
                    Err(e) => {
                        (sink)(failure_vm(network_id, e.to_string()));
                        return;
                    }
                }
            }

            // A typed-input step: wait for the user's values, then submit them.
            LoginStep::UserInput { step_id, .. } => {
                let step_id = step_id.clone();
                let login_id = current.login_id.clone();
                let values = loop {
                    match input_rx.recv().await {
                        Some(BridgeLoginInput::Fields { values }) => break values,
                        // A stray flow-choice while entering fields is ignored.
                        Some(BridgeLoginInput::ChooseFlow { .. }) => continue,
                        None => return,
                    }
                };
                match transport
                    .login_step(&login_id, &step_id, "user_input", &values)
                    .await
                {
                    Ok(next) => current = next,
                    Err(e) => {
                        (sink)(failure_vm(network_id, e.to_string()));
                        return;
                    }
                }
            }
        }
    }
}

/// Whether a step is a QR `display_and_wait` (for the refresh-flag heuristic).
fn step_is_qr(step: &LoginStep) -> bool {
    matches!(
        step,
        LoginStep::DisplayAndWait {
            display_and_wait: DisplayData::Qr { .. },
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridges::transport::{LoginField, LoginStepResponse};

    fn qr_step(data: &str) -> LoginStep {
        LoginStep::DisplayAndWait {
            step_id: "qr".to_owned(),
            display_and_wait: DisplayData::Qr {
                data: Some(data.to_owned()),
                image_url: None,
            },
        }
    }

    #[test]
    fn qr_svg_renders_a_scannable_svg() {
        let svg = qr_svg("2@abc123def").expect("qr renders");
        assert!(svg.contains("<svg"), "expected an SVG document: {svg}");
        assert!(svg.contains("</svg>"), "expected a closed SVG: {svg}");
    }

    #[test]
    fn step_to_vm_qr_carries_svg_and_instruction_and_refresh_flag() {
        let vm = step_to_vm("whatsapp", &qr_step("payload"), false);
        assert_eq!(vm.phase, BridgeLoginPhase::Qr);
        assert!(vm.qr_svg.is_some(), "qr phase must carry an SVG");
        assert!(!vm.qr_refreshed, "not a refresh");
        assert!(
            vm.instruction
                .as_deref()
                .unwrap_or_default()
                .contains("WhatsApp"),
            "instruction names the network from the catalog: {:?}",
            vm.instruction
        );

        let refreshed = step_to_vm("whatsapp", &qr_step("payload2"), true);
        assert!(refreshed.qr_refreshed, "refresh flag must propagate");
    }

    #[test]
    fn step_to_vm_user_input_maps_fields_to_code_entry() {
        let step = LoginStep::UserInput {
            step_id: "phone".to_owned(),
            fields: vec![LoginField {
                id: "phone".to_owned(),
                field_type: "phone_number".to_owned(),
                name: "Phone number".to_owned(),
                description: None,
                pattern: Some("^\\+".to_owned()),
                default_value: None,
            }],
        };
        let vm = step_to_vm("signal", &step, false);
        assert_eq!(vm.phase, BridgeLoginPhase::CodeEntry);
        assert_eq!(vm.fields.len(), 1);
        assert_eq!(vm.fields[0].field_type, "phone_number");
        assert_eq!(vm.fields[0].pattern.as_deref(), Some("^\\+"));
    }

    #[test]
    fn step_to_vm_complete_is_success() {
        let vm = step_to_vm(
            "telegram",
            &LoginStep::Complete {
                user_login_id: Some("telegram_1".to_owned()),
            },
            false,
        );
        assert_eq!(vm.phase, BridgeLoginPhase::Success);
    }

    #[test]
    fn step_to_vm_cookies_and_webauthn_are_unsupported_failures() {
        let cookies = step_to_vm("instagram", &LoginStep::Cookies { step_id: None }, false);
        assert_eq!(cookies.phase, BridgeLoginPhase::Failure);
        let err = cookies.error.expect("unsupported failure carries copy");
        assert!(
            err.contains("browser"),
            "names the unsupported method: {err}"
        );
        assert!(
            err.contains("Bridge Bot chat"),
            "names the manual path honestly: {err}"
        );

        let webauthn = step_to_vm("linkedin", &LoginStep::Webauthn { step_id: None }, false);
        assert_eq!(webauthn.phase, BridgeLoginPhase::Failure);
        assert!(
            webauthn.error.unwrap_or_default().contains("passkey"),
            "webauthn names passkeys"
        );
    }

    #[test]
    fn step_to_vm_unknown_step_is_unsupported_failure() {
        let vm = step_to_vm("xchat", &LoginStep::Unknown, false);
        assert_eq!(vm.phase, BridgeLoginPhase::Failure);
        assert!(vm.error.is_some());
    }

    #[test]
    fn step_to_vm_qr_with_unencodable_payload_is_honest_failure() {
        // A payload too large for any QR version can't encode; the VM must be an
        // honest failure, never a blank QR panel.
        let huge = "x".repeat(10_000);
        let vm = step_to_vm(
            "whatsapp",
            &LoginStep::DisplayAndWait {
                step_id: "qr".to_owned(),
                display_and_wait: DisplayData::Qr {
                    data: Some(huge),
                    image_url: None,
                },
            },
            false,
        );
        assert_eq!(vm.phase, BridgeLoginPhase::Failure);
        assert!(vm.error.is_some());
    }

    // --- Driver loop tests over a scripted fake transport ---------------------

    struct FakeTransport {
        flows: Vec<crate::bridges::transport::LoginFlow>,
        /// The scripted step responses returned in order by `login_start` then each
        /// `login_step`. The first is the `start` result.
        script: std::sync::Mutex<
            std::collections::VecDeque<Result<LoginStepResponse, crate::error::BridgeError>>,
        >,
    }

    impl BridgeTransport for FakeTransport {
        fn login_flows(
            &self,
        ) -> impl std::future::Future<
            Output = Result<Vec<crate::bridges::transport::LoginFlow>, crate::error::BridgeError>,
        > + Send {
            let flows = self.flows.clone();
            async move { Ok(flows) }
        }

        fn login_start(
            &self,
            _flow_id: &str,
        ) -> impl std::future::Future<Output = Result<LoginStepResponse, crate::error::BridgeError>> + Send
        {
            let next = self.script.lock().expect("lock").pop_front();
            async move { next.expect("script exhausted at start") }
        }

        fn login_step(
            &self,
            _login_id: &str,
            _step_id: &str,
            _step_type: &str,
            _body: &BTreeMap<String, String>,
        ) -> impl std::future::Future<Output = Result<LoginStepResponse, crate::error::BridgeError>> + Send
        {
            let next = self.script.lock().expect("lock").pop_front();
            async move { next.expect("script exhausted at step") }
        }

        async fn login_cancel(&self, _login_id: &str) {}
    }

    fn flow(id: &str) -> crate::bridges::transport::LoginFlow {
        crate::bridges::transport::LoginFlow {
            id: id.to_owned(),
            name: id.to_owned(),
            description: None,
        }
    }

    fn resp(login_id: &str, step: LoginStep) -> LoginStepResponse {
        LoginStepResponse {
            login_id: login_id.to_owned(),
            step,
        }
    }

    #[tokio::test]
    async fn drives_qr_then_complete_to_success() {
        let transport = FakeTransport {
            flows: vec![flow("qr")],
            script: std::sync::Mutex::new(
                [
                    Ok(resp("l1", qr_step("payload"))),
                    Ok(resp(
                        "l1",
                        LoginStep::Complete {
                            user_login_id: Some("wa_1".to_owned()),
                        },
                    )),
                ]
                .into(),
            ),
        };
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<BridgeLoginVm>::new()));
        let sink_captured = captured.clone();
        let sink: BridgeLoginSink = Box::new(move |vm| {
            sink_captured.lock().expect("lock").push(vm);
            true
        });
        let (_tx, rx) = mpsc::unbounded_channel();
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        drive_login(transport, "whatsapp", sink, rx, slot.clone()).await;

        // Once `login_start` succeeds the login id is recorded in the slot so an
        // explicit cancel has an id to POST `/login/cancel/{id}` with.
        assert_eq!(
            slot.lock().expect("slot").as_deref(),
            Some("l1"),
            "the login id must be populated after start"
        );

        let vms = captured.lock().expect("lock");
        let phases: Vec<_> = vms.iter().map(|v| v.phase).collect();
        // waiting → qr → success (single flow auto-starts, no ChoosingMethod).
        assert_eq!(
            phases,
            vec![
                BridgeLoginPhase::Waiting,
                BridgeLoginPhase::Qr,
                BridgeLoginPhase::Success,
            ]
        );
    }

    #[tokio::test]
    async fn multiple_flows_wait_for_a_choice_before_starting() {
        let transport = FakeTransport {
            flows: vec![flow("qr"), flow("phone")],
            script: std::sync::Mutex::new(
                [Ok(resp(
                    "l1",
                    LoginStep::Complete {
                        user_login_id: Some("x".to_owned()),
                    },
                ))]
                .into(),
            ),
        };
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<BridgeLoginVm>::new()));
        let sink_captured = captured.clone();
        let sink: BridgeLoginSink = Box::new(move |vm| {
            sink_captured.lock().expect("lock").push(vm);
            true
        });
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(BridgeLoginInput::ChooseFlow {
            flow_id: "qr".to_owned(),
        })
        .expect("send choice");
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        drive_login(transport, "whatsapp", sink, rx, slot).await;

        let vms = captured.lock().expect("lock");
        assert_eq!(vms[0].phase, BridgeLoginPhase::Waiting);
        assert_eq!(vms[1].phase, BridgeLoginPhase::ChoosingMethod);
        assert_eq!(vms[1].flows.len(), 2);
        assert_eq!(vms.last().expect("last").phase, BridgeLoginPhase::Success);
    }

    #[tokio::test]
    async fn a_step_error_surfaces_verbatim_failure() {
        let transport = FakeTransport {
            flows: vec![flow("qr")],
            script: std::sync::Mutex::new(
                [Err(crate::error::BridgeError::Provisioning(
                    "M_FORBIDDEN: already logged in".to_owned(),
                ))]
                .into(),
            ),
        };
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<BridgeLoginVm>::new()));
        let sink_captured = captured.clone();
        let sink: BridgeLoginSink = Box::new(move |vm| {
            sink_captured.lock().expect("lock").push(vm);
            true
        });
        let (_tx, rx) = mpsc::unbounded_channel();
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        drive_login(transport, "whatsapp", sink, rx, slot).await;

        let vms = captured.lock().expect("lock");
        let last = vms.last().expect("a failure vm");
        assert_eq!(last.phase, BridgeLoginPhase::Failure);
        assert_eq!(
            last.error.as_deref(),
            Some("bridge login failed: M_FORBIDDEN: already logged in")
        );
    }
}

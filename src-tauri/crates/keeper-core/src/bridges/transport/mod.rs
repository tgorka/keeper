//! The bridge-login transport seam (Story 6.3, FR-26, AD-16).
//!
//! A [`BridgeTransport`] drives one bridge login to a terminal state. Its first
//! (and, in 6.3, only) impl is [`provisioning::Provisioning`], which speaks the
//! mautrix **bridgev2 HTTP+JSON provisioning API**; Story 6.4 adds a `BotDriver`
//! impl behind the same trait so the two paths are behaviorally interchangeable —
//! the generic driver ([`crate::bridges::login::drive_login`]) and the emitted
//! [`crate::vm::BridgeLoginVm`] states never know which transport powered a login.
//!
//! **No `async-trait`.** Following [`crate::auth::AuthProvider`], the trait uses
//! native `async fn`s returning `impl Future<Output = …> + Send`, dispatched
//! **statically** via a generic driver (`drive_login<T: BridgeTransport>`). There
//! is no trait object, so no `async_fn_in_trait` clippy warning and no dynamic
//! dispatch.
//!
//! The wire types ([`LoginFlow`], [`LoginStep`], [`LoginField`]) are the serde
//! shapes of the bridgev2 provisioning responses. A [`LoginStep`] is `type`-tagged
//! (`user_input` | `cookies` | `display_and_wait` | `webauthn` | `complete`) with
//! the `login_id` flattened alongside; `cookies`/`webauthn` are out of scope (a
//! webview / passkey ceremony) and surface as a distinct unsupported-method state,
//! never a half-built flow.

use std::collections::BTreeMap;
use std::future::Future;

use serde::Deserialize;

use crate::error::BridgeError;

pub mod bot;
pub mod provisioning;

/// One login method the bridge offers (a bridgev2 login flow descriptor).
///
/// `name`/`description` are display copy; `id` is the stable handle passed to
/// [`BridgeTransport::login_start`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LoginFlow {
    /// The stable flow id used to start this method.
    pub id: String,
    /// The flow's human-readable name.
    pub name: String,
    /// An optional longer description of the method.
    #[serde(default)]
    pub description: Option<String>,
}

/// One input field a `user_input` step asks for (a bridgev2 field descriptor).
///
/// The submit body is keyed by [`LoginField::id`]. `field_type` (e.g.
/// `phone_number`, `2fa_code`, `password`, `token`) drives the input treatment;
/// `pattern` is an optional client-side validation regex.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LoginField {
    /// The field id the submit body is keyed by.
    pub id: String,
    /// The provisioning field type (drives the input treatment).
    #[serde(rename = "type")]
    pub field_type: String,
    /// The human-readable field label.
    pub name: String,
    /// An optional longer description / helper text.
    #[serde(default)]
    pub description: Option<String>,
    /// An optional regex the entered value must match before submit.
    #[serde(default)]
    pub pattern: Option<String>,
    /// An optional prefilled default value.
    #[serde(default)]
    pub default_value: Option<String>,
}

/// The `type`-tagged payload of a bridgev2 `display_and_wait` step: a QR image, a
/// code to type on the peer, an emoji SAS, or nothing (a plain wait).
///
/// This is a server long-poll; a *fresh* `DisplayAndWait` returned before
/// `Complete` is a QR (or code) rotation. Only `Qr`/`Code` render something the
/// user acts on in 6.3; `Emoji`/`Nothing` render as a plain waiting state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DisplayData {
    /// A QR code the user scans; `data` is the QR payload string.
    Qr {
        /// The QR payload to render (encoded into an SVG by the driver).
        #[serde(default)]
        data: Option<String>,
        /// An optional server-rendered image URL (unused — keeper renders its own).
        #[serde(default)]
        image_url: Option<String>,
    },
    /// A short code the user types on the peer device; `data` is the code.
    Code {
        /// The code to display.
        #[serde(default)]
        data: Option<String>,
    },
    /// An emoji SAS to compare; `data` is the emoji string. Rendered as a wait +
    /// instruction in 6.3 (no compare UI — that is verification's domain).
    Emoji {
        /// The emoji string to display.
        #[serde(default)]
        data: Option<String>,
    },
    /// Nothing to show — a plain long-poll wait.
    Nothing,
}

/// One step of a bridgev2 login state machine, with the `login_id` flattened
/// alongside the `type`-tagged step body.
///
/// `#[serde(other)]` on [`LoginStep::Unknown`] keeps a future step type from
/// failing deserialization — it is treated as an unsupported method rather than a
/// hard error.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LoginStepResponse {
    /// The opaque login id for subsequent `step`/`cancel` calls, flattened by the
    /// server beside the step body.
    pub login_id: String,
    /// The step body.
    #[serde(flatten)]
    pub step: LoginStep,
}

/// The `type`-tagged body of a login step (bridgev2 `bridgev2/login.go`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginStep {
    /// The bridge wants typed input: `step_id` identifies the step; `fields` are
    /// the descriptors to render; submit posts a body keyed by each field `id`.
    UserInput {
        /// The step id, the middle path segment of the submit URL.
        step_id: String,
        /// The fields to render and collect.
        #[serde(default)]
        fields: Vec<LoginField>,
    },
    /// The bridge is showing something and long-polling for the peer to act
    /// (QR / code / emoji / nothing).
    DisplayAndWait {
        /// The step id, the middle path segment of the poll URL.
        step_id: String,
        /// What to display while waiting. The bridgev2 wire nests the display data
        /// under a `display_and_wait` object (its own `type`-tagged shape), so it is
        /// a named field rather than flattened — a flatten would collide with the
        /// outer step's own `type` tag.
        display_and_wait: DisplayData,
    },
    /// A browser-cookie harvest step — OUT OF SCOPE (no webview in 6.3).
    Cookies {
        /// The step id (unused in 6.3 — surfaced as unsupported).
        #[serde(default)]
        step_id: Option<String>,
    },
    /// A passkey (WebAuthn) ceremony — OUT OF SCOPE (no passkey engine in 6.3).
    Webauthn {
        /// The step id (unused in 6.3 — surfaced as unsupported).
        #[serde(default)]
        step_id: Option<String>,
    },
    /// The login completed; `user_login_id` is the linked login (terminal).
    Complete {
        /// The linked bridge login id.
        #[serde(default)]
        user_login_id: Option<String>,
    },
    /// Any future / unrecognized step type — treated as unsupported, never a hard
    /// deserialize error.
    #[serde(other)]
    Unknown,
}

/// Drives one bridge login to a terminal state (Story 6.3, AD-16).
///
/// Native `async fn`s dispatched statically via
/// [`crate::bridges::login::drive_login`] — no `async-trait`, no trait object. The
/// `-> impl Future + Send` return signatures keep the `async_fn_in_trait` clippy
/// lint quiet while the trait stays object-safe-free (static dispatch only).
///
/// Later stories (6.5/6.6) add `list_logins`/`logout`/`set_relay` when they are
/// exercised — defining unused ones now would be dead code.
pub trait BridgeTransport {
    /// List the login methods (flows) the bridge exposes.
    fn login_flows(&self) -> impl Future<Output = Result<Vec<LoginFlow>, BridgeError>> + Send;

    /// Start login `flow_id`, returning the first step (with its `login_id`).
    fn login_start(
        &self,
        flow_id: &str,
    ) -> impl Future<Output = Result<LoginStepResponse, BridgeError>> + Send;

    /// Advance the login `login_id` at `step_id`/`step_type` with a submit `body`
    /// (a map of field id → value; empty for a bare poll advance).
    fn login_step(
        &self,
        login_id: &str,
        step_id: &str,
        step_type: &str,
        body: &BTreeMap<String, String>,
    ) -> impl Future<Output = Result<LoginStepResponse, BridgeError>> + Send;

    /// Best-effort cancel of login `login_id` (the user closed the flow). A failure
    /// here is logged and swallowed — cancel must never surface an error.
    fn login_cancel(&self, login_id: &str) -> impl Future<Output = ()> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_display_and_wait_qr_with_flattened_login_id() {
        let json = r#"{
            "login_id": "login-abc",
            "type": "display_and_wait",
            "step_id": "qr",
            "display_and_wait": { "type": "qr", "data": "2@abc123" }
        }"#;
        let resp: LoginStepResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(resp.login_id, "login-abc");
        match resp.step {
            LoginStep::DisplayAndWait {
                step_id,
                display_and_wait,
            } => {
                assert_eq!(step_id, "qr");
                assert_eq!(
                    display_and_wait,
                    DisplayData::Qr {
                        data: Some("2@abc123".to_owned()),
                        image_url: None,
                    }
                );
            }
            other => panic!("expected display_and_wait, got {other:?}"),
        }
    }

    #[test]
    fn deserializes_user_input_fields() {
        let json = r#"{
            "login_id": "l1",
            "type": "user_input",
            "step_id": "phone",
            "fields": [
                {"type": "phone_number", "id": "phone", "name": "Phone number", "pattern": "^\\+"}
            ]
        }"#;
        let resp: LoginStepResponse = serde_json::from_str(json).expect("parse");
        match resp.step {
            LoginStep::UserInput { step_id, fields } => {
                assert_eq!(step_id, "phone");
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].id, "phone");
                assert_eq!(fields[0].field_type, "phone_number");
                assert_eq!(fields[0].pattern.as_deref(), Some("^\\+"));
            }
            other => panic!("expected user_input, got {other:?}"),
        }
    }

    #[test]
    fn deserializes_complete_and_cookies_and_unknown() {
        let complete: LoginStepResponse = serde_json::from_str(
            r#"{"login_id":"l","type":"complete","user_login_id":"whatsapp_123"}"#,
        )
        .expect("parse complete");
        assert!(matches!(
            complete.step,
            LoginStep::Complete {
                user_login_id: Some(_)
            }
        ));

        let cookies: LoginStepResponse =
            serde_json::from_str(r#"{"login_id":"l","type":"cookies","step_id":"c"}"#)
                .expect("parse cookies");
        assert!(matches!(cookies.step, LoginStep::Cookies { .. }));

        let webauthn: LoginStepResponse =
            serde_json::from_str(r#"{"login_id":"l","type":"webauthn"}"#).expect("parse webauthn");
        assert!(matches!(webauthn.step, LoginStep::Webauthn { .. }));

        // A future step type must not fail deserialization — it maps to Unknown.
        let unknown: LoginStepResponse =
            serde_json::from_str(r#"{"login_id":"l","type":"some_future_type"}"#)
                .expect("parse unknown");
        assert_eq!(unknown.step, LoginStep::Unknown);
    }
}

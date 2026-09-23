//! The browser leg of an account sign-in (Epic 82, AD-311).
//!
//! `keeper_core::org_account::oidc` builds the authorization URL, registers
//! its `state` in the [`OAuthFlowRegistry`] and asks the platform to show the
//! URL through [`Platform::start_web_auth`]. On macOS and iOS that is
//! `ASWebAuthenticationSession` (`web_auth_apple.rs`): a sheet that shares
//! Safari's cookies, offers passkeys and password managers, and hands the
//! callback URL straight back — no deep link has to make it through Launch
//! Services, which on a phone is the difference between a sign-in that
//! finishes and one that waits five minutes for nothing. Everywhere else the
//! port's default opens the system browser and the `keeper://` deep link (or
//! the loopback listener core binds itself) delivers the callback.
//!
//! This file is the part of that relay that is not FFI: where the sheet's
//! outcome goes. It compiles and is tested on every target; only
//! [`start`] and [`cancel_all`] reach the Apple half.
//!
//! [`Platform::start_web_auth`]: keeper_core::platform::Platform::start_web_auth

// Off Apple nothing presents a sheet, so the routing below is reached only by
// its tests there; it stays compiled so those tests run on every host.
#![cfg_attr(not(any(target_os = "macos", target_os = "ios")), allow(dead_code))]

use std::sync::{Arc, OnceLock};

use keeper_core::oauth::OAuthFlowRegistry;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

/// Why a flow ends when its sheet hands back a callback that is not its own.
const FOREIGN_CALLBACK: &str = "the sign-in page answered without this sign-in's state";

/// The registry the sheet's outcome is delivered to, and the handle that
/// reaches the main thread, set once from `lib.rs`'s setup. The platform
/// structs are unit structs that reach process state through write-once
/// globals (`NOTIFY_APP`, `BADGE_APP`); this is the same shape.
static HOST: OnceLock<(tauri::AppHandle, Arc<OAuthFlowRegistry>)> = OnceLock::new();

/// Record the app handle and the registry. Idempotent: a second call is
/// ignored, as the handle is write-once.
pub fn install(app: &tauri::AppHandle, flows: Arc<OAuthFlowRegistry>) {
    let _ = HOST.set((app.clone(), flows));
}

/// The `state` an authorization URL carries — the key its flow waits under in
/// the registry, and so the only way to end that one flow when the sheet
/// closes without a callback.
///
/// A forge's `signin_url` wraps the authorization request in its own query
/// (`…/user/login?redirect_to=%2Flogin%2Foauth%2Fauthorize%3F…%26state%3D…`),
/// so when the top level carries no `state` the one inside `redirect_to` is
/// the flow's.
pub fn state_of(url: &str) -> Option<String> {
    query_param(url, "state").or_else(|| query_param(&query_param(url, "redirect_to")?, "state"))
}

/// One form-decoded query parameter of `url` (absolute or a bare path),
/// ignoring any fragment.
fn query_param(url: &str, key: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    let query = query.split('#').next().unwrap_or_default();
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

/// Whether the platform sheet can hand this callback back itself.
///
/// `ASWebAuthenticationSession` matches a custom scheme; an `http(s)`
/// redirect is either a loopback listener (which core serves, and which
/// needs the default browser) or a universal link keeper has no associated
/// domain for. Core only asks for the sheet with a custom scheme, so this is
/// the second lock on that door, not the first.
pub fn sheet_can_deliver(scheme: &str) -> bool {
    !scheme.is_empty()
        && !scheme.eq_ignore_ascii_case("http")
        && !scheme.eq_ignore_ascii_case("https")
}

/// How a sheet ended, before it is told to the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The provider redirected to the callback scheme with this URL.
    Callback(String),
    /// The person closed the sheet or declined the "wants to sign in" alert.
    Cancelled,
    /// The sheet could not be shown or ended with a system error; the
    /// sentence says why.
    Failed(String),
}

/// Deliver one sheet's ending to the flow waiting under `state`.
///
/// A callback carrying this flow's `state` goes through
/// [`OAuthFlowRegistry::resolve`] exactly as a deep link would. A callback
/// the sheet hands back without it — an identity provider's error page that
/// drops `state`, or some other `keeper://` address — still ends this sheet's
/// sign-in: it ends it as an error at once (the provider's own `error` when
/// it sent one) rather than leaving it to wait out its five-minute timeout,
/// and it is never offered to another flow. A cancel ends only that flow, so
/// a sign-in waiting beside it keeps waiting. A failure is delivered as the
/// provider would deliver a refusal — an `error=` callback for the same
/// `state` — so the waiting flow ends at once with the reason.
pub fn finish(flows: &OAuthFlowRegistry, state: &str, ending: Ending) {
    match ending {
        Ending::Callback(url) => {
            if query_param(&url, "state").as_deref() == Some(state) && flows.resolve(&url) {
                return;
            }
            tracing::debug!("web auth: the callback did not carry this sign-in's state");
            let reason = query_param(&url, "error").unwrap_or_else(|| FOREIGN_CALLBACK.to_owned());
            if !flows.resolve(&failure_url(state, &reason)) {
                tracing::debug!("web auth: the sign-in had already ended");
            }
        }
        Ending::Cancelled => flows.cancel(state),
        Ending::Failed(reason) => {
            tracing::warn!(%reason, "web auth: the sign-in sheet failed");
            if !flows.resolve(&failure_url(state, &reason)) {
                tracing::debug!("web auth: the failure matched no waiting sign-in");
            }
        }
    }
}

/// A callback URL that carries `reason` as the OAuth `error` for `state`.
fn failure_url(state: &str, reason: &str) -> String {
    format!(
        "keeper://oauth/web-auth?state={}&error={}",
        utf8_percent_encode(state, NON_ALPHANUMERIC),
        utf8_percent_encode(reason, NON_ALPHANUMERIC)
    )
}

/// The window the sheet is presented over: keeper's main window, which is
/// where every sign-in is started from.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const MAIN_WINDOW: &str = "main";

/// Present `url` in `ASWebAuthenticationSession` over the main window.
///
/// Returns once the presentation is queued. Whatever happens after — a
/// callback, a cancel, a sheet that could not start — reaches the registry
/// through [`finish`], so the caller waits on its flow and nothing else.
/// `with_webview` is the way in because its closure runs on the main thread
/// (its whole contract) and is handed the native window the sheet anchors to.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn start(url: &str, callback_scheme: &str) -> Result<(), keeper_core::error::CoreError> {
    use keeper_core::error::CoreError;
    use tauri::Manager;

    let (app, flows) = HOST.get().ok_or_else(|| {
        CoreError::Unsupported(
            "the sign-in sheet is not available before the app starts".to_owned(),
        )
    })?;
    let state = state_of(url)
        .ok_or_else(|| CoreError::Internal("the authorization URL carries no state".to_owned()))?;
    let window = app.get_webview_window(MAIN_WINDOW).ok_or_else(|| {
        CoreError::Internal(
            "keeper's main window is gone, so the sign-in sheet has nothing to open over"
                .to_owned(),
        )
    })?;
    let url = url.to_owned();
    let scheme = callback_scheme.to_owned();
    let flows = Arc::clone(flows);
    let app = app.clone();
    window
        .with_webview(move |webview| {
            if let Err(reason) =
                crate::web_auth_apple::present(&app, &webview, &url, &scheme, &state, &flows)
            {
                finish(&flows, &state, Ending::Failed(reason));
            }
        })
        .map_err(|error| CoreError::Internal(format!("could not reach the main window: {error}")))
}

/// Close every sign-in sheet this process has open. Each one's completion
/// then reports a cancel, which ends its flow through [`finish`].
pub fn cancel_all() {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if let Some((app, _)) = HOST.get() {
        if let Err(error) = app.run_on_main_thread(crate::web_auth_apple::cancel_all) {
            tracing::warn!(%error, "web auth: could not reach the main thread to cancel");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::oauth::OAuthCallback;

    #[test]
    fn the_state_is_read_from_the_query_and_decoded() {
        assert_eq!(
            state_of("https://id.acme.dev/authorize?client_id=k&state=a%2Bb&nonce=n").as_deref(),
            Some("a+b")
        );
        assert_eq!(state_of("https://id.acme.dev/authorize?client_id=k"), None);
        assert_eq!(state_of("https://id.acme.dev/authorize#state=x"), None);
    }

    /// A forge `signin_url` carries the authorization request, state and
    /// all, query-encoded inside `redirect_to` — encoded the way core's
    /// `fill_signin_url` encodes it.
    #[test]
    fn the_state_is_found_inside_a_signin_url_redirect_to() {
        let authorize = "/login/oauth/authorize?client_id=k&state=abc123&code_challenge=x";
        let wrapped: String = url::form_urlencoded::byte_serialize(authorize.as_bytes()).collect();
        let signin = format!("https://git.acme.dev/user/login?redirect_to={wrapped}");
        assert_eq!(state_of(&signin).as_deref(), Some("abc123"));
        // The top level wins when both carry one.
        assert_eq!(
            state_of(&format!("{signin}&state=outer")).as_deref(),
            Some("outer")
        );
        assert_eq!(
            state_of("https://git.acme.dev/user/login?redirect_to=%2Fhome"),
            None
        );
    }

    /// A sheet that comes back without its own state ends its own flow at
    /// once, as an error carrying the provider's reason, and is never
    /// offered to another waiting flow. `try_recv`, not `await`: a flow left
    /// waiting must fail the test, not hang it.
    #[test]
    fn a_callback_without_this_state_ends_the_original_flow_as_an_error() {
        let flows = OAuthFlowRegistry::new();
        let mut mine = flows.register("mine".to_owned());
        let _other = flows.register("other".to_owned());
        finish(
            &flows,
            "mine",
            Ending::Callback("keeper://oauth/acme/callback?code=c&state=other".to_owned()),
        );
        match mine.try_recv() {
            Ok(OAuthCallback::Error(reason)) => assert_eq!(reason, FOREIGN_CALLBACK),
            other => panic!("expected an error callback, got {other:?}"),
        }
        // The other flow was not resolved by the foreign callback.
        assert!(flows.resolve("keeper://oauth/acme/callback?state=other&code=c"));

        let mut dropped = flows.register("s2".to_owned());
        finish(
            &flows,
            "s2",
            Ending::Callback("keeper://oauth/acme/callback?error=access_denied".to_owned()),
        );
        assert!(matches!(
            dropped.try_recv(),
            Ok(OAuthCallback::Error(reason)) if reason == "access_denied"
        ));
    }

    #[test]
    fn only_a_custom_scheme_is_handed_to_the_sheet() {
        assert!(sheet_can_deliver("keeper"));
        assert!(sheet_can_deliver("dev.tgorka.keeper"));
        assert!(!sheet_can_deliver("http"));
        assert!(!sheet_can_deliver("HTTPS"));
        assert!(!sheet_can_deliver(""));
    }

    /// A cancelled sheet ends its own flow and no other: a Matrix sign-in
    /// waiting beside it must not be torn down by the person closing the
    /// account's sheet.
    #[tokio::test]
    async fn a_cancel_ends_only_its_own_flow() {
        let flows = OAuthFlowRegistry::new();
        let mine = flows.register("mine".to_owned());
        let _other = flows.register("other".to_owned());
        finish(&flows, "mine", Ending::Cancelled);
        assert!(matches!(mine.await, Ok(OAuthCallback::Cancelled)));
        assert!(flows.resolve("keeper://oauth/callback?state=other&code=c"));
    }

    /// A sheet that could not be shown ends the flow with its reason at once,
    /// instead of leaving it to time out.
    #[tokio::test]
    async fn a_failure_reaches_the_flow_as_an_error_with_its_reason() {
        let flows = OAuthFlowRegistry::new();
        let waiting = flows.register("s&1".to_owned());
        finish(
            &flows,
            "s&1",
            Ending::Failed("the window is not in a foreground scene".to_owned()),
        );
        match waiting.await {
            Ok(OAuthCallback::Error(reason)) => {
                assert_eq!(reason, "the window is not in a foreground scene");
            }
            other => panic!("expected an error callback, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_callback_is_resolved_like_a_deep_link() {
        let flows = OAuthFlowRegistry::new();
        let waiting = flows.register("abc".to_owned());
        let url = "keeper://oauth/acme/callback?code=c&state=abc".to_owned();
        finish(&flows, "abc", Ending::Callback(url.clone()));
        assert!(matches!(waiting.await, Ok(OAuthCallback::Redirect(got)) if got == url));
    }
}

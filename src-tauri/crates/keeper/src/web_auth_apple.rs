//! `ASWebAuthenticationSession`, macOS and iOS (Epic 82, AD-311).
//!
//! One module for both Apple targets: the class, its callback matcher and its
//! presentation-anchor protocol are the same on each, and only the anchor
//! differs — the main window's `NSWindow` on a Mac, its `UIWindow` on a
//! phone. Everything here runs on the main thread: [`present`] is reached
//! from `with_webview`'s closure and [`cancel_all`] from `run_on_main_thread`,
//! and the table of live sheets is a main-thread `thread_local` for that
//! reason.
//!
//! # What must stay alive, and for how long
//!
//! Apple's session is started and then left to run; the app must hold it
//! until its completion handler fires, or it is deallocated mid-sign-in and
//! the sheet vanishes. Its `presentationContextProvider` is a *weak* property,
//! so the anchor object must be held too. The completion block is copied by
//! the session, but ours is kept beside it so the whole triple is released at
//! one moment: on the main thread's next turn after the completion ran, never
//! from inside the completion itself, which the session is still executing.
//!
//! # The sheet is not ephemeral
//!
//! `prefersEphemeralWebBrowserSession = false`: the sheet shares Safari's
//! cookies, so a person already signed in to their identity provider — and
//! the forge's own login that `signin_url` rides through — is not asked for a
//! password again, and 1Password and iCloud passkeys are offered in it.
//!
//! Every objc2 call below that the bindings mark `unsafe` sits in one of this
//! file's function-level `#[allow(unsafe_code)]` items — the `anchor` module
//! (the class definition), [`window_of`] (one per target), [`present`],
//! [`is_session_error`] and [`cancel_all`] — each with a `// SAFETY:`
//! comment, and all are listed in the audit inventory in
//! `docs/constraints-and-limitations.md`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use block2::RcBlock;
use keeper_core::oauth::OAuthFlowRegistry;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{available, AnyThread, MainThreadMarker};
use objc2_authentication_services::{
    ASWebAuthenticationSession, ASWebAuthenticationSessionCallback,
    ASWebAuthenticationSessionErrorCode, ASWebAuthenticationSessionErrorDomain,
};
use objc2_foundation::{NSError, NSString, NSURL};

use crate::web_auth::{finish, Ending};

/// The object the session asks where to present: it answers with the window
/// it was made with.
///
/// A module of its own so the one `#[allow(unsafe_code)]` covers exactly the
/// class definition — `define_class!` is `unsafe impl`s by construction.
#[allow(unsafe_code)]
mod anchor {
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_authentication_services::{
        ASPresentationAnchor, ASWebAuthenticationPresentationContextProviding,
        ASWebAuthenticationSession,
    };

    define_class!(
        // SAFETY: `NSObject` has no subclassing requirements, and `Anchor`
        // does not implement `Drop`. Main-thread-only because the protocol
        // requires it and because the anchor is a window.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = Retained<NSObject>]
        pub struct Anchor;

        unsafe impl NSObjectProtocol for Anchor {}

        // SAFETY: the protocol's one required method is implemented with the
        // signature AuthenticationServices declares —
        // `- (ASPresentationAnchor)presentationAnchorForWebAuthenticationSession:
        // (ASWebAuthenticationSession *)session` — returning a retained window
        // that the ivar keeps alive for as long as this object lives.
        unsafe impl ASWebAuthenticationPresentationContextProviding for Anchor {
            #[unsafe(method_id(presentationAnchorForWebAuthenticationSession:))]
            fn presentation_anchor(
                &self,
                _session: &ASWebAuthenticationSession,
            ) -> Retained<ASPresentationAnchor> {
                self.ivars().clone()
            }
        }
    );

    impl Anchor {
        pub fn new(main: MainThreadMarker, window: Retained<NSObject>) -> Retained<Self> {
            let this = Self::alloc(main).set_ivars(window);
            // SAFETY: `init` is `NSObject`'s designated initializer, and the
            // ivars were set on the allocation above, as `define_class!`
            // requires before it is sent.
            unsafe { msg_send![super(this), init] }
        }
    }
}

/// One sheet in flight, held until its completion has run.
struct Live {
    session: Retained<ASWebAuthenticationSession>,
    _anchor: Retained<anchor::Anchor>,
    _completion: RcBlock<dyn Fn(*mut NSURL, *mut NSError)>,
}

thread_local! {
    /// The sheets this process has open, by the `state` of their flow. Only
    /// the main thread touches it — see the module doc.
    static LIVE: RefCell<HashMap<String, Live>> = RefCell::new(HashMap::new());
}

/// The main window's native window, as the anchor type the protocol returns.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn window_of(webview: &tauri::webview::PlatformWebview) -> Result<Retained<NSObject>, String> {
    use objc2_app_kit::NSWindow;

    let window = webview.ns_window();
    // SAFETY: on macOS `ns_window()` is the `NSWindow` tao created for this
    // webview's window, alive while the window is — and the caller is inside
    // `with_webview` on that window. `retain` takes our own strong reference
    // (and answers `None` for a null pointer), so the sheet's anchor outlives
    // the borrow.
    let window: Retained<NSWindow> = unsafe { Retained::retain(window.cast::<NSWindow>()) }
        .ok_or("the main window has no native window to open the sign-in sheet over")?;
    Ok(Retained::into_super(Retained::into_super(window)))
}

/// The main window's native window, as the anchor type the protocol returns.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
fn window_of(webview: &tauri::webview::PlatformWebview) -> Result<Retained<NSObject>, String> {
    use objc2_ui_kit::UIViewController;

    let controller = webview.view_controller();
    if controller.is_null() {
        return Err(
            "the main window has no view controller to open the sign-in sheet over".to_owned(),
        );
    }
    // SAFETY: on iOS `view_controller()` is the `UIViewController` tao created
    // for this window, alive for as long as the window is — and the caller is
    // inside `with_webview` on that window. Null-checked above; only borrowed
    // for the two reads below, which hand back retained objects.
    let controller: &UIViewController = unsafe { &*controller.cast::<UIViewController>() };
    let window = controller
        .view()
        .and_then(|view| view.window())
        .ok_or("the main window is not on screen, so the sign-in sheet cannot open")?;
    Ok(Retained::into_super(Retained::into_super(
        Retained::into_super(window),
    )))
}

/// Start one sheet for `url`, whose callback comes back on `scheme`, and
/// report its ending for `state` through [`finish`].
///
/// `Err` is a sheet that never started, worded for the person; the caller
/// delivers it to the flow so it ends at once.
#[allow(unsafe_code)]
pub fn present(
    app: &tauri::AppHandle,
    webview: &tauri::webview::PlatformWebview,
    url: &str,
    scheme: &str,
    state: &str,
    flows: &Arc<OAuthFlowRegistry>,
) -> Result<(), String> {
    let Some(main) = MainThreadMarker::new() else {
        return Err("the sign-in sheet was asked for off the main thread".to_owned());
    };
    let window = window_of(webview)?;
    let auth_url = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or("the identity provider's sign-in address is not a URL")?;

    let completion: RcBlock<dyn Fn(*mut NSURL, *mut NSError)> = {
        let flows = Arc::clone(flows);
        let state = state.to_owned();
        let app = app.clone();
        RcBlock::new(move |callback: *mut NSURL, error: *mut NSError| {
            // SAFETY: the completion handler's contract is that exactly one
            // of the two is non-null, each valid for the duration of the
            // call; both are only borrowed here, and nothing outlives it but
            // owned Rust strings.
            let callback = unsafe { callback.as_ref() };
            // SAFETY: as above.
            let error = unsafe { error.as_ref() };
            finish(&flows, &state, ending(callback, error));
            release(&app, state.clone());
        })
    };

    let callback_scheme = NSString::from_str(scheme);
    // The `else` arm names the only initializer before macOS 14.4 / iOS 17.4,
    // deprecated in favour of the one above and never withdrawn.
    #[allow(deprecated)]
    let session = if available!(macos = 14.4, ios = 17.4) {
        // SAFETY: `callbackWithCustomScheme:` takes any scheme string and
        // returns an autoreleased matcher, retained by the binding.
        let callback = unsafe {
            ASWebAuthenticationSessionCallback::callbackWithCustomScheme(&callback_scheme)
        };
        // SAFETY: `initWithURL:callback:completionHandler:` consumes the
        // fresh allocation; the URL is http(s) (core builds it from the
        // provider's https authorization endpoint), and the handler pointer
        // is a live heap block that the session copies and that `Live` also
        // keeps until after it has run.
        unsafe {
            ASWebAuthenticationSession::initWithURL_callback_completionHandler(
                ASWebAuthenticationSession::alloc(),
                &auth_url,
                &callback,
                RcBlock::as_ptr(&completion),
            )
        }
    } else {
        // SAFETY: as for the branch above; the scheme is a plain string the
        // session matches the callback URL's scheme against.
        unsafe {
            ASWebAuthenticationSession::initWithURL_callbackURLScheme_completionHandler(
                ASWebAuthenticationSession::alloc(),
                &auth_url,
                Some(&callback_scheme),
                RcBlock::as_ptr(&completion),
            )
        }
    };

    let anchor = anchor::Anchor::new(main, window);
    // SAFETY: both setters are documented to be called before `start`, on
    // the main thread, which the marker above proves. The provider is weak,
    // so `anchor` is kept in `Live` below for the session's whole life.
    unsafe {
        session.setPresentationContextProvider(Some(ProtocolObject::from_ref(&*anchor)));
        session.setPrefersEphemeralWebBrowserSession(false);
    }

    // Held before `start`, so a completion that arrives on the very next turn
    // finds the entry it releases.
    LIVE.with(|live| {
        live.borrow_mut().insert(
            state.to_owned(),
            Live {
                session: session.clone(),
                _anchor: anchor,
                _completion: completion,
            },
        );
    });
    // SAFETY: `start` is called once, on the main thread, after the provider
    // is set — the preconditions Apple lists for it.
    let started = unsafe { session.start() };
    if !started {
        LIVE.with(|live| live.borrow_mut().remove(state));
        return Err("the sign-in sheet could not start; try again with keeper in front".to_owned());
    }
    Ok(())
}

/// What the completion handler's two arguments mean for the waiting flow.
fn ending(callback: Option<&NSURL>, error: Option<&NSError>) -> Ending {
    if let Some(url) = callback.and_then(NSURL::absoluteString) {
        return Ending::Callback(url.to_string());
    }
    let Some(error) = error else {
        return Ending::Failed("the sign-in sheet closed without an answer".to_owned());
    };
    if error.code() == ASWebAuthenticationSessionErrorCode::CanceledLogin.0
        && is_session_error(error)
    {
        return Ending::Cancelled;
    }
    Ending::Failed(format!(
        "the sign-in sheet could not finish: {}",
        error.localizedDescription()
    ))
}

/// Whether `error` is the session's own (its codes mean nothing in any other
/// domain).
#[allow(unsafe_code)]
fn is_session_error(error: &NSError) -> bool {
    // SAFETY: `ASWebAuthenticationSessionErrorDomain` is an immutable,
    // process-lifetime `NSErrorDomain` extern static exported by the
    // framework; reading it carries no other obligation.
    let domain: &NSString = unsafe { ASWebAuthenticationSessionErrorDomain };
    *error.domain() == *domain
}

/// Drop one sheet's session, anchor and block on the main thread's next turn.
fn release(app: &tauri::AppHandle, state: String) {
    let queued = app.run_on_main_thread(move || {
        LIVE.with(|live| live.borrow_mut().remove(&state));
    });
    if let Err(error) = queued {
        tracing::warn!(%error, "web auth: could not release a finished sign-in sheet");
    }
}

/// Cancel every open sheet. Each one's completion then reports
/// `CanceledLogin`, which [`ending`] turns into a cancel of its own flow.
#[allow(unsafe_code)]
pub fn cancel_all() {
    let sessions: Vec<Retained<ASWebAuthenticationSession>> = LIVE.with(|live| {
        live.borrow()
            .values()
            .map(|live| live.session.clone())
            .collect()
    });
    for session in sessions {
        // SAFETY: `cancel` is documented as safe to call at any time on a
        // started session, and a no-op on one already finished; it runs on
        // the main thread (this is reached through `run_on_main_thread`) and
        // outside the table's borrow, since it may run the completion — which
        // only queues its release — before it returns.
        unsafe { session.cancel() };
    }
}

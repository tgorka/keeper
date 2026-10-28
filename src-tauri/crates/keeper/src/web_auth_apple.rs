//! `ASWebAuthenticationSession`, iOS (Epic 82, AD-311).
//!
//! The sheet the account and Matrix sign-ins open over the main window's
//! `UIWindow`. macOS used it too until the Mac moved to the default browser:
//! there it was a separate Safari window whatever the person's browser was.
//! Everything here runs on the main thread: [`present`] and
//! [`previous_sheet`] are reached from `with_webview`'s closure, [`cancel`]
//! and [`cancel_for`] from `run_on_main_thread`, and the table of sheets is
//! a main-thread `thread_local` for that reason.
//!
//! # When a sheet ends, and what must stay alive until when
//!
//! The table is `keeper_core::sign_in_sheet::Sheets`, and two questions are
//! kept apart in it. A sheet stops holding back the next one when it
//! *retires*, which this file does explicitly on every way a sheet ends: its
//! completion ran, keeper cancelled it ([`cancel`], [`cancel_for`]), Apple
//! refused to start it, or its sign-in's deadline passed
//! ([`previous_sheet`]). keeper never waits for a completion after its own
//! `cancel()` — Apple documents one for the person's cancel, not the app's
//! (R608-01) — so a cancel retires the sheet and tells its flow itself, and
//! a completion that comes later finds its sheet retired and does nothing.
//!
//! The native objects live longer. Apple's session must be held until its
//! dismissal has finished, or it crashes when the transition completes
//! (FB12132525); its `presentationContextProvider` is a *weak* property, so
//! the anchor is held too; and the completion block beside them. All three
//! are released together `sign_in_sheet::RETENTION` after the retirement,
//! from a task a background thread queues ([`release_later`]). That is a
//! later turn of the main thread by construction: tauri-runtime-wry's
//! `run_on_main_thread` runs a task *inline* when called on the main thread
//! (2.11.4, `send_user_message`), and queues it only from another thread.
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
//! (the class definition), [`window_of`], [`anchor_state`], [`present`],
//! [`is_session_error`] and [`dismiss`] — each with a `// SAFETY:`
//! comment, and all are listed in the audit inventory in
//! `docs/constraints-and-limitations.md`.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use keeper_core::oauth::OAuthFlowRegistry;
use keeper_core::sign_in_sheet::{Activation, PreviousSheet, Sheet, SheetId, Sheets, RETENTION};
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

/// What one sheet holds alive, from before its `start` until
/// `RETENTION` after it retired.
struct Live {
    session: Retained<ASWebAuthenticationSession>,
    /// The registry the sheet reports to: a cancel of one registry's
    /// sign-ins leaves the other's sheets open.
    flows: Arc<OAuthFlowRegistry>,
    _anchor: Retained<anchor::Anchor>,
    _completion: RcBlock<dyn Fn(*mut NSURL, *mut NSError)>,
}

thread_local! {
    /// The sheets this process started, open and retired. Only the main
    /// thread touches it — see the module doc. Never borrowed across a call
    /// into the session (`start`, `cancel`, a release), any of which may run
    /// a completion that borrows it again.
    static SHEETS: RefCell<Sheets<Live>> = RefCell::new(Sheets::new());
}

/// The main window's native window, as the anchor type the protocol returns.
#[allow(unsafe_code)]
fn window_of(
    webview: &tauri::webview::PlatformWebview,
) -> Result<Retained<objc2_ui_kit::UIWindow>, String> {
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
    controller
        .view()
        .and_then(|view| view.window())
        .ok_or_else(|| {
            "the main window is not on screen, so the sign-in sheet cannot open".to_owned()
        })
}

/// What Apple's `start` checks the anchor against, read the same way: does
/// the window have a scene, what is that scene's `activationState`, is the
/// window key. Logged when a sheet is refused, so a refusal names its cause.
#[derive(Clone, Copy)]
struct AnchorState {
    scene: bool,
    activation: isize,
    key: bool,
}

#[allow(unsafe_code)]
fn anchor_state(window: &objc2_ui_kit::UIWindow) -> AnchorState {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    // SAFETY: on the main thread (the callers are `activation` and `present`,
    // both inside `with_webview`'s closure), `windowScene`
    // answers a `UIWindowScene` or nil, `activationState` is an `NSInteger`
    // enum on that scene, and `isKeyWindow` a `BOOL` on the window; none
    // retains or keeps anything beyond the call.
    unsafe {
        let scene: *mut AnyObject = msg_send![window, windowScene];
        let activation: isize = if scene.is_null() {
            -1
        } else {
            msg_send![scene, activationState]
        };
        AnchorState {
            scene: !scene.is_null(),
            activation,
            key: msg_send![window, isKeyWindow],
        }
    }
}

/// Why [`present`] did not start a sheet.
pub enum Refusal {
    /// Apple's `start` refused the window it was given; another may be
    /// handed later (`sign_in_sheet::SheetWait::refused`).
    Refused,
    /// It will not start; the sentence is for the person.
    Failed(String),
}

/// keeper's window as Apple's `start` will judge it: its scene's
/// activation, read after the keyboard of a just-pasted setup link is
/// dismissed (it holds the window's attention, and a sheet over it is
/// refused; dismissing it is what a tap elsewhere does). `Err` is a window
/// that is not there to look at, worded for the person.
pub fn activation(webview: &tauri::webview::PlatformWebview) -> Result<Activation, String> {
    let window = window_of(webview)?;
    let _ = window.endEditing(true);
    let anchor = anchor_state(&window);
    Ok(Activation::of_scene(anchor.scene, anchor.activation))
}

/// The sheet shown before the one being asked for: on screen while any
/// sheet keeper started has not retired, otherwise how long ago the last one
/// retired. A sheet past its sign-in's deadline is cancelled and retired
/// first (`Sheets::overdue`), so nothing holds a new sheet back for longer
/// than that whatever became of its completion. Main thread only, like the
/// table it reads.
pub fn previous_sheet(app: &tauri::AppHandle) -> PreviousSheet {
    let now = Instant::now();
    let overdue = SHEETS.with(|sheets| dismissed(sheets.borrow_mut().overdue(now)));
    if !overdue.is_empty() {
        tracing::warn!(
            sheets = overdue.len(),
            "web auth: closed a sign-in sheet still open past its sign-in's deadline"
        );
    }
    dismiss(app, overdue);
    SHEETS.with(|sheets| sheets.borrow().previous(now))
}

/// Start one sheet for `url`, whose callback comes back on `scheme`, and
/// report its ending for `state` through [`finish`]. The caller has already
/// judged the window ready ([`activation`], `sign_in_sheet::SheetWait`).
///
/// `Err` is a sheet that never started: [`Refusal::Refused`] when Apple
/// refused the window, [`Refusal::Failed`] worded for the person otherwise.
#[allow(unsafe_code)]
pub fn present(
    app: &tauri::AppHandle,
    webview: &tauri::webview::PlatformWebview,
    url: &str,
    scheme: &str,
    state: &str,
    flows: &Arc<OAuthFlowRegistry>,
) -> Result<(), Refusal> {
    let Some(main) = MainThreadMarker::new() else {
        return Err(Refusal::Failed(
            "the sign-in sheet was asked for off the main thread".to_owned(),
        ));
    };
    let window = window_of(webview).map_err(Refusal::Failed)?;
    let anchor_was = anchor_state(&window);
    let window: Retained<NSObject> =
        Retained::into_super(Retained::into_super(Retained::into_super(window)));
    let auth_url = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| {
        Refusal::Failed("the identity provider's sign-in address is not a URL".to_owned())
    })?;

    let id = SheetId::next();
    let completion: RcBlock<dyn Fn(*mut NSURL, *mut NSError)> = {
        let flows = Arc::clone(flows);
        let state = state.to_owned();
        let app = app.clone();
        RcBlock::new(move |callback: *mut NSURL, error: *mut NSError| {
            // Retired already — keeper cancelled it, or its deadline passed —
            // and its flow was told then: a late answer is nobody's, and must
            // not end a newer sheet or flow under the same `state`.
            if !SHEETS.with(|sheets| sheets.borrow_mut().retire(id, Instant::now())) {
                tracing::debug!("web auth: a sign-in sheet answered after it was retired");
                return;
            }
            // SAFETY: the completion handler's contract is that exactly one
            // of the two is non-null, each valid for the duration of the
            // call; both are only borrowed here, and nothing outlives it but
            // owned Rust strings.
            let callback = unsafe { callback.as_ref() };
            // SAFETY: as above.
            let error = unsafe { error.as_ref() };
            finish(&flows, &state, ending(callback, error));
            release_later(&app);
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
        // keeps until `RETENTION` after the sheet retired.
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

    // Held before `start`, so a completion that runs inside it, or on the
    // very next turn, finds its sheet open.
    SHEETS.with(|sheets| {
        sheets.borrow_mut().open(
            id,
            state,
            Live {
                session: session.clone(),
                flows: Arc::clone(flows),
                _anchor: anchor,
                _completion: completion,
            },
            Instant::now(),
        );
    });
    // SAFETY: `start` is called once, on the main thread, after the provider
    // is set — the preconditions Apple lists for it.
    let started = unsafe { session.start() };
    if !started {
        SHEETS.with(|sheets| sheets.borrow_mut().retire(id, Instant::now()));
        release_later(app);
        tracing::warn!(
            scene = anchor_was.scene,
            activation = anchor_was.activation,
            key = anchor_was.key,
            "web auth: Apple refused the sign-in sheet's window"
        );
        return Err(Refusal::Refused);
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
    // The error's domain and code, never its description: the sign-in sheet
    // is the opener of an authorization page, and an opener's words may
    // quote it (R522). `web_auth::finish` logs this reason.
    Ending::Failed(format!(
        "the sign-in sheet could not finish ({} error {})",
        error.domain(),
        error.code()
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

/// Release, on a later turn of the main thread, what every sheet retired at
/// least `RETENTION` ago held: its session, anchor and block.
///
/// The task is queued from a thread of its own, after `RETENTION`. From the
/// main thread `run_on_main_thread` would run it inline (tauri-runtime-wry
/// 2.11.4 `send_user_message`), inside the completion that retired the
/// sheet; from any other thread it goes through the event loop, so it runs
/// once the main thread is free. The table judges what is due, so a task
/// that runs early releases nothing too soon, and each retirement queues
/// its own task, so none is held past the next one due.
fn release_later(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(RETENTION);
        let queued = app.run_on_main_thread(|| {
            let due = SHEETS.with(|sheets| sheets.borrow_mut().release_due(Instant::now()));
            // Dropped outside the table's borrow: a session may run its
            // completion as it goes, and that borrows the table.
            drop(due);
        });
        if let Err(error) = queued {
            tracing::warn!(%error, "web auth: could not release a finished sign-in sheet");
        }
    });
}

/// A sheet keeper retired itself, taken out of the table's borrow to be
/// dismissed.
struct Dismissed {
    state: String,
    flows: Arc<OAuthFlowRegistry>,
    session: Retained<ASWebAuthenticationSession>,
}

fn dismissed(sheets: Vec<&Sheet<Live>>) -> Vec<Dismissed> {
    sheets
        .into_iter()
        .map(|sheet| Dismissed {
            state: sheet.state().to_owned(),
            flows: Arc::clone(&sheet.held().flows),
            session: sheet.held().session.clone(),
        })
        .collect()
}

/// Dismiss sheets keeper has already retired, and tell each one's flow it
/// was cancelled — here, not in a completion Apple does not promise after
/// an app's `cancel()`. A completion that does come finds its sheet retired
/// and does nothing.
#[allow(unsafe_code)]
fn dismiss(app: &tauri::AppHandle, sheets: Vec<Dismissed>) {
    if sheets.is_empty() {
        return;
    }
    for sheet in sheets {
        // SAFETY: `cancel` is documented as safe to call at any time on a
        // started session, and a no-op on one already finished; it runs on
        // the main thread (every caller is reached through `with_webview` or
        // `run_on_main_thread`) and outside the table's borrow, since it may
        // run the completion before it returns.
        unsafe { sheet.session.cancel() };
        finish(&sheet.flows, &sheet.state, Ending::Cancelled);
    }
    release_later(app);
}

/// Cancel every open sheet that reports to `flows`, and the flow each one
/// serves.
pub fn cancel_for(app: &tauri::AppHandle, flows: &Arc<OAuthFlowRegistry>) {
    let now = Instant::now();
    let sheets = SHEETS.with(|sheets| {
        dismissed(
            sheets
                .borrow_mut()
                .cancel(now, |sheet| Arc::ptr_eq(&sheet.held().flows, flows)),
        )
    });
    dismiss(app, sheets);
}

/// Cancel the sheet open for `state`, if there still is one.
pub fn cancel(app: &tauri::AppHandle, state: &str) {
    let now = Instant::now();
    let sheets = SHEETS.with(|sheets| {
        dismissed(
            sheets
                .borrow_mut()
                .cancel(now, |sheet| sheet.state() == state),
        )
    });
    dismiss(app, sheets);
}

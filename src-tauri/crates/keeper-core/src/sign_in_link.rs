//! The authorization page a waiting sign-in is shown to the person, so a
//! browser that did not come forward can be given it by hand (story 102.9 #1,
//! R470, R480).
//!
//! Both OIDC sign-ins — Matrix single sign-on ([`crate::auth`]) and the
//! organisation account's ([`crate::org_account::oidc`]) — build the
//! authorization URL, register its `state` with
//! [`OAuthFlowRegistry::register_with_link`], and ask the platform to open
//! it. [`offer`] is that last step for both: the link goes to the person
//! first ([`Platform::show_sign_in_link`]), then the browser is asked for it.
//!
//! A page that could not be shown never ends the sign-in, whenever that is
//! learnt: an open that fails at once, or a sign-in sheet that was queued and
//! refused later ([`OAuthFlowRegistry::not_shown`], iOS and Android). The
//! link stays on screen with [`NOT_OPENED`], and the sign-in waits for its
//! callback until its own deadline or the person's Cancel — the deep link (or
//! the loopback listener) delivers it whichever browser the person pasted the
//! link into. A cancel, the provider's refusal and a callback that is not
//! this sign-in's stay what they were: the end of it.
//!
//! The link carries a public client's request — `client_id`, `redirect_uri`,
//! `scope`, `state`, `nonce` and a PKCE `code_challenge`, never a secret or a
//! verifier — so showing it is safe. It lives exactly as long as its sign-in:
//! [`Shown`] takes it away when the sign-in ends, however it ends (a callback,
//! a cancel, a refusal, the deadline, or the waiting future being dropped),
//! and [`SignInLinkSlot`] keeps a surface from drawing a finished sign-in's
//! link again. A pasted link of an ended sign-in would answer nobody — its
//! `state` and verifier are gone — so the person is offered a fresh sign-in
//! instead (R481). End-session (sign-out) URLs carry an ID-token hint and
//! never come here.
//!
//! [`OAuthFlowRegistry::register_with_link`]: crate::oauth::OAuthFlowRegistry::register_with_link
//! [`OAuthFlowRegistry::not_shown`]: crate::oauth::OAuthFlowRegistry::not_shown

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedReceiver;
use ts_rs::TS;

use crate::error::{CoreError, NotOpened};
use crate::platform::Platform;
use crate::vm::NotifyTarget;

/// What the sign-in screen draws while a sign-in waits for the browser.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SignInLinkVm {
    /// The authorization page, exactly as keeper asked the browser to open it.
    pub url: String,
    /// keeper could not open the browser: the sentence that says so. `None`
    /// while the browser was opened (as far as the system said).
    pub not_opened: Option<String>,
}

// The page carries its sign-in's `state`, PKCE challenge and nonce: shown to
// the person, never printed into a log by a `{:?}` of anything holding it.
impl std::fmt::Debug for SignInLinkVm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInLinkVm")
            .field("url", &"<redacted>")
            .field("not_opened", &self.not_opened)
            .finish()
    }
}

/// The sentence beside a link no browser was opened for.
pub const NOT_OPENED: &str =
    "keeper could not open your browser. Copy this link into a browser to finish signing in.";

/// Each sign-in's own number, so a surface can tell one sign-in's link from
/// the next one's.
static NEXT_FLOW: AtomicU64 = AtomicU64::new(1);

/// Show `url` to the person, then `open` it. A failed open is one WARN line
/// and the link shown again with [`NOT_OPENED`]; it never ends the sign-in,
/// which keeps waiting for the callback the pasted link will bring. Hold the
/// answer for as long as the sign-in waits: dropping it takes the link away.
pub fn offer<'a>(
    platform: &'a dyn Platform,
    url: &str,
    open: impl FnOnce() -> Result<(), CoreError>,
) -> Shown<'a> {
    let shown = Shown {
        platform,
        url: url.to_owned(),
        flow: NEXT_FLOW.fetch_add(1, Ordering::Relaxed),
    };
    shown.show(None);
    if let Err(error) = open() {
        // What kind of failure, never the opener's words: its error can
        // quote the command it ran, page included, in any encoding and cut
        // anywhere — so no text of it is logged at all (R381, R522).
        let category = NotOpened::of(&error);
        tracing::warn!(%category, "sign-in: the sign-in page did not open; the sign-in keeps waiting for its link to be used by hand");
        shown.show(Some(NOT_OPENED));
    }
    shown
}

/// One sign-in's link on screen. Dropped when the sign-in ends — however it
/// ends, a dropped future included — it takes the link away.
#[must_use = "the link is taken away when this is dropped"]
pub struct Shown<'a> {
    platform: &'a dyn Platform,
    url: String,
    flow: u64,
}

impl Shown<'_> {
    fn show(&self, not_opened: Option<&str>) {
        self.platform.show_sign_in_link(
            self.flow,
            Some(&SignInLinkVm {
                url: self.url.clone(),
                not_opened: not_opened.map(str::to_owned),
            }),
        );
    }

    /// Await `until` — the sign-in's callback — and each time `unshown` says
    /// the page could not be shown after all (a sheet refused after it was
    /// queued), show the link again with [`NOT_OPENED`]: the sign-in keeps
    /// waiting for it (R480). The platform that saw the refusal logged it.
    pub async fn wait<T>(
        &self,
        unshown: &mut UnboundedReceiver<String>,
        until: impl Future<Output = T>,
    ) -> T {
        tokio::pin!(until);
        let mut listening = true;
        loop {
            tokio::select! {
                done = &mut until => return done,
                reason = unshown.recv(), if listening => match reason {
                    Some(_) => self.show(Some(NOT_OPENED)),
                    None => listening = false,
                },
            }
        }
    }
}

impl Drop for Shown<'_> {
    fn drop(&mut self) {
        self.platform.show_sign_in_link(self.flow, None);
    }
}

/// Where a surface keeps the link of the sign-in it started: the newest
/// sign-in's, and never one that already ended — a late delivery cannot
/// draw a finished sign-in's link again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignInLinkSlot {
    current: Option<(u64, SignInLinkVm)>,
    /// The newest sign-in known to have ended.
    ended: u64,
}

impl SignInLinkSlot {
    /// Take one [`Platform::show_sign_in_link`] call for `flow`.
    pub fn apply(&mut self, flow: u64, link: Option<&SignInLinkVm>) {
        match link {
            Some(_) if flow <= self.ended => {}
            Some(_) if self.current.as_ref().is_some_and(|(at, _)| *at > flow) => {}
            Some(link) => self.current = Some((flow, link.clone())),
            None => {
                self.ended = self.ended.max(flow);
                if self.current.as_ref().is_some_and(|(at, _)| *at <= flow) {
                    self.current = None;
                }
            }
        }
    }

    /// The link to draw, while its sign-in waits.
    pub fn link(&self) -> Option<&SignInLinkVm> {
        self.current.as_ref().map(|(_, link)| link)
    }
}

/// `inner`, except that a sign-in's link goes to `show`: how a shell routes
/// one sign-in's link to the surface that started it (the login screen's
/// channel, the account's view model) without the platform knowing either.
pub struct ShowsSignInLink<'a, F> {
    inner: &'a dyn Platform,
    show: F,
}

impl<'a, F: Fn(u64, Option<&SignInLinkVm>) + Send + Sync> ShowsSignInLink<'a, F> {
    pub fn new(inner: &'a dyn Platform, show: F) -> Self {
        Self { inner, show }
    }
}

impl<F: Fn(u64, Option<&SignInLinkVm>) + Send + Sync> Platform for ShowsSignInLink<'_, F> {
    fn data_dir(&self) -> Result<std::path::PathBuf, CoreError> {
        self.inner.data_dir()
    }
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.inner.keychain_set(key, value)
    }
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        self.inner.keychain_get(key)
    }
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        self.inner.keychain_delete(key)
    }
    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        self.inner.open_url(url)
    }
    fn start_web_auth(&self, url: &str, callback_scheme: &str) -> Result<(), CoreError> {
        self.inner.start_web_auth(url, callback_scheme)
    }
    fn show_sign_in_link(&self, flow: u64, link: Option<&SignInLinkVm>) {
        (self.show)(flow, link);
    }
    fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
        self.inner.notify(title, body, target)
    }
    fn sidecar_path(&self, name: &str) -> Result<std::path::PathBuf, CoreError> {
        self.inner.sidecar_path(name)
    }
    fn exclude_from_backup(&self, path: &std::path::Path) -> Result<(), CoreError> {
        self.inner.exclude_from_backup(path)
    }
    fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError> {
        self.inner.set_badge_count(count)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records, in order, every link shown and every page opened.
    #[derive(Default)]
    pub(crate) struct Recorder {
        events: Mutex<Vec<String>>,
        fails: bool,
    }

    impl Platform for Recorder {
        fn data_dir(&self) -> Result<std::path::PathBuf, CoreError> {
            Err(CoreError::Unsupported("none".to_owned()))
        }
        fn keychain_set(&self, _: &str, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, url: &str) -> Result<(), CoreError> {
            self.events
                .lock()
                .expect("events")
                .push(format!("open {url}"));
            if self.fails {
                Err(CoreError::Internal("no browser".to_owned()))
            } else {
                Ok(())
            }
        }
        fn show_sign_in_link(&self, _: u64, link: Option<&SignInLinkVm>) {
            self.events.lock().expect("events").push(match link {
                Some(link) => format!(
                    "show {} {}",
                    link.url,
                    link.not_opened.as_deref().unwrap_or("-")
                ),
                None => "gone".to_owned(),
            });
        }
        fn notify(&self, _: &str, _: &str, _: &NotifyTarget) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _: &str) -> Result<std::path::PathBuf, CoreError> {
            Err(CoreError::Unsupported("none".to_owned()))
        }
        fn exclude_from_backup(&self, _: &std::path::Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    const URL: &str = "https://id.example/authorize?state=s1&code_challenge=c";

    fn events(platform: &Recorder) -> Vec<String> {
        platform.events.lock().expect("events").clone()
    }

    #[test]
    fn the_link_is_shown_before_the_browser_is_asked_and_goes_with_its_sign_in() {
        let platform = Recorder::default();
        let shown = offer(&platform, URL, || platform.open_url(URL));
        assert_eq!(
            events(&platform),
            vec![format!("show {URL} -"), format!("open {URL}")]
        );
        drop(shown);
        assert_eq!(events(&platform).last().map(String::as_str), Some("gone"));
    }

    #[test]
    fn a_browser_that_did_not_open_leaves_the_link_with_the_sentence() {
        let platform = Recorder {
            fails: true,
            ..Recorder::default()
        };
        let _shown = offer(&platform, URL, || platform.open_url(URL));
        assert_eq!(
            events(&platform),
            vec![
                format!("show {URL} -"),
                format!("open {URL}"),
                format!("show {URL} {NOT_OPENED}"),
            ]
        );
    }

    #[tokio::test]
    async fn a_sheet_refused_after_it_was_queued_puts_the_sentence_beside_the_link_and_waits_on() {
        let platform = Recorder::default();
        let shown = offer(&platform, URL, || platform.open_url(URL));
        let (tell, mut unshown) = tokio::sync::mpsc::unbounded_channel();
        let (answer, callback) = tokio::sync::oneshot::channel::<&str>();
        tell.send("presentationContextInvalid".to_owned())
            .expect("send");
        drop(tell);
        let waited = shown.wait(&mut unshown, async {
            // The refusal is shown before the callback arrives, and the wait
            // outlives the closed refusal channel.
            tokio::task::yield_now().await;
            answer.send("callback").expect("answer");
            callback.await.expect("callback")
        });
        assert_eq!(waited.await, "callback");
        assert_eq!(
            events(&platform),
            vec![
                format!("show {URL} -"),
                format!("open {URL}"),
                format!("show {URL} {NOT_OPENED}"),
            ]
        );
    }

    fn link(url: &str) -> SignInLinkVm {
        SignInLinkVm {
            url: url.to_owned(),
            not_opened: None,
        }
    }

    #[test]
    fn a_slot_drops_its_sign_ins_link_when_it_ends_and_never_takes_it_back() {
        let mut slot = SignInLinkSlot::default();
        slot.apply(4, Some(&link("a")));
        assert_eq!(slot.link(), Some(&link("a")));
        slot.apply(4, None);
        assert_eq!(slot.link(), None);
        // A delivery that arrives after its sign-in ended draws nothing.
        slot.apply(4, Some(&link("a")));
        assert_eq!(slot.link(), None);
        assert_eq!(slot, {
            let mut ended = SignInLinkSlot::default();
            ended.apply(4, None);
            ended
        });
    }

    #[test]
    fn a_slot_keeps_the_newest_sign_ins_link_whatever_an_older_one_says() {
        let mut slot = SignInLinkSlot::default();
        slot.apply(4, Some(&link("old")));
        slot.apply(5, Some(&link("new")));
        slot.apply(4, Some(&link("old again")));
        assert_eq!(slot.link(), Some(&link("new")));
        // The older sign-in ending late takes nothing of the newer one's.
        slot.apply(4, None);
        assert_eq!(slot.link(), Some(&link("new")));
        slot.apply(5, None);
        assert_eq!(slot.link(), None);
    }

    /// A page with the values a real authorization request carries.
    pub(crate) const PAGE: &str = "https://id.example:8451/authorize?client_id=k&state=Zq3xW9vLp2Rt8sYb\
                        &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&nonce=n0nce4Sign1n";

    /// The page is shown, never printed by a `{:?}` of what holds it.
    #[test]
    fn a_sign_in_link_never_debug_prints_its_page() {
        let shown = SignInLinkVm {
            url: PAGE.to_owned(),
            not_opened: Some(NOT_OPENED.to_owned()),
        };
        let mut slot = SignInLinkSlot::default();
        slot.apply(1, Some(&shown));
        for printed in [format!("{shown:?}"), format!("{slot:?}")] {
            assert!(!printed.contains("Zq3xW9vLp2Rt8sYb"), "{printed}");
            assert!(!printed.contains("id.example"), "{printed}");
            assert!(printed.contains(NOT_OPENED), "{printed}");
        }
    }
}

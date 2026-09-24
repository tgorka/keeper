//! GitHub's device flow (AD-334): no client secret, no browser redirect back
//! to keeper. The person types a code on github.com while keeper polls in a
//! flow they started; waiting is a cancellable `sleep`, never a timer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::source::ForgeSource;
use super::tokens::{ForgeError, StoredForgeToken};
use super::vm::DeviceCodeVm;
use super::{body, now_ms, send};

/// Read access to every repository (there is no read-only `repo`) and the
/// names of the person's organizations.
pub const SCOPE: &str = "repo read:org";

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The longest wait between two polls, and the longest a code is waited
/// for, whatever the forge says: a hostile answer can neither overflow a
/// deadline nor park the flow for days.
const MAX_INTERVAL: u64 = 60;
const MAX_EXPIRES_IN: u64 = 1800;

/// What the person types and where; `device_code` stays in the shell.
#[derive(Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds, at most [`MAX_EXPIRES_IN`].
    pub expires_in: u64,
    /// Seconds between polls, 1 to [`MAX_INTERVAL`].
    pub interval: u64,
    device_code: String,
    /// The client the code was issued to, which is the client the token is
    /// stored under, whatever the source says by the time it is approved.
    client_id: String,
}

impl std::fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .finish_non_exhaustive()
    }
}

impl DeviceCode {
    pub fn vm(&self) -> DeviceCodeVm {
        DeviceCodeVm {
            user_code: self.user_code.clone(),
            verification_uri: self.verification_uri.clone(),
            expires_in: self.expires_in,
        }
    }
}

#[derive(Deserialize)]
struct RawDeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_expires_in() -> u64 {
    900
}

fn default_interval() -> u64 {
    5
}

fn client_id(source: &ForgeSource) -> Result<&str, ForgeError> {
    source.client_id.as_deref().ok_or_else(|| {
        ForgeError::Refused(format!("keeper has no way to connect to {}.", source.name))
    })
}

fn form(http: &reqwest::Client, url: String, fields: &[(&str, &str)]) -> reqwest::RequestBuilder {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    http.post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
}

/// Ask GitHub for a code to show the person.
pub async fn start(http: &reqwest::Client, source: &ForgeSource) -> Result<DeviceCode, ForgeError> {
    let client_id = client_id(source)?;
    let url = format!("{}/login/device/code", source.web_base);
    let response = send(
        http,
        form(http, url, &[("client_id", client_id), ("scope", SCOPE)]),
    )
    .await?;
    let bytes = body(response).await?;
    let error = serde_json::from_slice::<RawAnswer>(&bytes)
        .ok()
        .and_then(|raw| raw.error);
    if let Some(code) = error {
        // `{"error":"Not Found"}` is an unknown client id.
        return Err(match code.as_str() {
            "Not Found" => terminal_error("incorrect_client_credentials"),
            code => terminal_error(code),
        });
    }
    let raw: RawDeviceCode = serde_json::from_slice(&bytes).map_err(|_| {
        ForgeError::Refused(format!(
            "{} did not give keeper a code to show you.",
            source.host()
        ))
    })?;
    let verification_uri = on_site(&raw.verification_uri, source).ok_or_else(|| {
        ForgeError::Refused(format!(
            "{} gave keeper an approval page elsewhere, so keeper does not open it.",
            source.host()
        ))
    })?;
    Ok(DeviceCode {
        user_code: raw.user_code,
        verification_uri,
        expires_in: raw.expires_in.min(MAX_EXPIRES_IN),
        interval: raw.interval.clamp(1, MAX_INTERVAL),
        device_code: raw.device_code,
        client_id: client_id.to_owned(),
    })
}

/// `uri` when it is `https` (loopback for tests) on the source's own site:
/// its web host or a subdomain of it. Only such a page is ever opened.
fn on_site(uri: &str, source: &ForgeSource) -> Option<String> {
    let page = url::Url::parse(uri.trim()).ok()?;
    let site = url::Url::parse(&source.web_base).ok()?;
    let (host, site_host) = (page.host_str()?, site.host_str()?);
    let same_site = host.eq_ignore_ascii_case(site_host)
        || host
            .to_ascii_lowercase()
            .ends_with(&format!(".{}", site_host.to_ascii_lowercase()));
    let scheme_ok = page.scheme() == "https" || page.origin() == site.origin();
    (super::token_safe(&page) && scheme_ok && same_site).then(|| page.into())
}

/// Tokens from a poll or a refresh.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenAnswer {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_ms: Option<i64>,
}

impl std::fmt::Debug for TokenAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenAnswer")
            .field("expires_ms", &self.expires_ms)
            .finish_non_exhaustive()
    }
}

/// One answer of the token endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollStep {
    /// Not approved yet: keep waiting.
    Pending,
    /// Polling too fast: wait longer (GitHub may name the new interval).
    SlowDown {
        interval: Option<u64>,
    },
    Done(TokenAnswer),
    /// A terminal answer, already a sentence.
    Failed(ForgeError),
}

#[derive(Deserialize)]
struct RawAnswer {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    interval: Option<u64>,
}

/// Classify a token-endpoint body. GitHub answers 200 with an `error` field
/// while polling, so the body decides, not the status.
pub fn parse_poll(body: &[u8]) -> PollStep {
    let Ok(raw) = serde_json::from_slice::<RawAnswer>(body) else {
        return PollStep::Failed(ForgeError::Refused(
            "GitHub gave keeper an answer it could not read.".to_owned(),
        ));
    };
    match (raw.error.as_deref(), raw.access_token) {
        (Some("authorization_pending"), _) => PollStep::Pending,
        (Some("slow_down"), _) => PollStep::SlowDown {
            interval: raw.interval,
        },
        (Some(code), _) => PollStep::Failed(terminal_error(code)),
        (None, Some(access_token)) if !access_token.is_empty() => PollStep::Done(TokenAnswer {
            access_token,
            refresh_token: raw.refresh_token.filter(|t| !t.is_empty()),
            expires_ms: raw
                .expires_in
                .filter(|s| *s > 0)
                .map(|s| now_ms().saturating_add(s.saturating_mul(1000))),
        }),
        (None, _) => PollStep::Failed(ForgeError::Refused(
            "GitHub gave keeper an answer it could not read.".to_owned(),
        )),
    }
}

/// The wait before the next poll: `slow_down` adds five seconds, or takes
/// GitHub's new interval when that is longer — never past a minute.
pub fn next_interval(current: u64, step: &PollStep) -> u64 {
    match step {
        PollStep::SlowDown { interval } => current
            .saturating_add(5)
            .max(interval.unwrap_or(0))
            .min(MAX_INTERVAL),
        _ => current,
    }
}

fn terminal_error(code: &str) -> ForgeError {
    let sentence = match code {
        "expired_token" | "token_expired" => {
            "The code expired before it was approved. Start again."
        }
        "access_denied" => "The connection was declined on GitHub.",
        "device_flow_disabled" => "keeper's GitHub app does not allow connecting this way.",
        "incorrect_client_credentials" => "GitHub does not know keeper's app.",
        "incorrect_device_code" | "bad_verification_code" => {
            "GitHub did not recognise the code. Start again."
        }
        "unverified_user_email" => "Verify your email address on GitHub, then connect again.",
        "bad_refresh_token" => return ForgeError::NeedsConnect,
        other => {
            return ForgeError::Refused(format!("GitHub refused the connection ({other})."));
        }
    };
    ForgeError::Refused(sentence.to_owned())
}

/// Wait `seconds` (at most [`MAX_INTERVAL`]), waking every 100 ms to see
/// whether the person cancelled.
async fn wait(seconds: u64, cancel: &AtomicBool) -> Result<(), ForgeError> {
    let until = Instant::now() + Duration::from_secs(seconds.min(MAX_INTERVAL));
    loop {
        cancelled(cancel)?;
        let now = Instant::now();
        if now >= until {
            return Ok(());
        }
        tokio::time::sleep((until - now).min(Duration::from_millis(100))).await;
    }
}

fn cancelled(cancel: &AtomicBool) -> Result<(), ForgeError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(ForgeError::NeedsConnect);
    }
    Ok(())
}

/// Wait for the person to approve `code`, then learn who they are. Cancel
/// returns `NeedsConnect` (the source is simply not connected), even when
/// it lands while the token or the login is on its way: a Cancel or a
/// Disconnect is never undone by an answer already in flight. A login
/// GitHub does not give right away is left empty and learnt at the next
/// listing, so an approved token is never thrown away.
pub async fn poll(
    http: &reqwest::Client,
    source: &ForgeSource,
    code: &DeviceCode,
    cancel: &AtomicBool,
) -> Result<StoredForgeToken, ForgeError> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in.min(MAX_EXPIRES_IN));
    let mut interval = code.interval.clamp(1, MAX_INTERVAL);
    loop {
        wait(interval, cancel).await?;
        if Instant::now() >= deadline {
            return Err(terminal_error("expired_token"));
        }
        let url = format!("{}/login/oauth/access_token", source.web_base);
        let request = form(
            http,
            url,
            &[
                ("client_id", &code.client_id),
                ("device_code", &code.device_code),
                ("grant_type", DEVICE_GRANT),
            ],
        );
        let step = parse_poll(&body(send(http, request).await?).await?);
        cancelled(cancel)?;
        interval = next_interval(interval, &step);
        match step {
            PollStep::Pending | PollStep::SlowDown { .. } => {}
            PollStep::Failed(error) => return Err(error),
            PollStep::Done(tokens) => {
                let login = user_login(http, source, &tokens.access_token)
                    .await
                    .unwrap_or_default();
                cancelled(cancel)?;
                return Ok(StoredForgeToken {
                    access_token: tokens.access_token,
                    refresh_token: tokens.refresh_token,
                    expires_ms: tokens.expires_ms,
                    login,
                    client_id: code.client_id.clone(),
                });
            }
        }
    }
}

/// Exchange a refresh token (no secret: the token came from the device
/// flow). A dead refresh token is `NeedsConnect`.
pub(crate) async fn refresh(
    http: &reqwest::Client,
    source: &ForgeSource,
    refresh_token: &str,
) -> Result<TokenAnswer, ForgeError> {
    let client_id = client_id(source)?;
    let url = format!("{}/login/oauth/access_token", source.web_base);
    let request = form(
        http,
        url,
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    );
    let response = send(http, request).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ForgeError::NeedsConnect);
    }
    match parse_poll(&body(response).await?) {
        PollStep::Done(tokens) => Ok(tokens),
        PollStep::Failed(error) => Err(error),
        PollStep::Pending | PollStep::SlowDown { .. } => Err(ForgeError::Internal(
            "GitHub answered a refresh as if it were a device poll".to_owned(),
        )),
    }
}

#[derive(Deserialize)]
struct RawUser {
    login: String,
}

/// `GET {api_base}/user`: whose connection this is.
pub(crate) async fn user_login(
    http: &reqwest::Client,
    source: &ForgeSource,
    token: &str,
) -> Result<String, ForgeError> {
    let request = super::github::get(http, &format!("{}/user", source.api_base)).bearer_auth(token);
    let response = send(http, request).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ForgeError::NeedsConnect);
    }
    let bytes = body(response).await?;
    serde_json::from_slice::<RawUser>(&bytes)
        .map(|user| user.login)
        .map_err(|_| {
            ForgeError::Refused(format!("{} did not say who is connected.", source.host()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing::{self, Reply};
    use crate::forges::{ForgeKind, TokenVia};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    fn source(base: &str) -> ForgeSource {
        ForgeSource {
            id: "github".to_owned(),
            kind: ForgeKind::Github,
            name: "GitHub".to_owned(),
            web_base: base.to_owned(),
            api_base: format!("{base}/api"),
            client_id: Some("Iv1.c".to_owned()),
            via: TokenVia::DeviceFlow,
        }
    }

    #[test]
    fn poll_answers_map_to_wait_slow_down_or_a_terminal_sentence() {
        assert_eq!(
            parse_poll(br#"{"error":"authorization_pending"}"#),
            PollStep::Pending
        );
        let slow = parse_poll(br#"{"error":"slow_down"}"#);
        assert_eq!(slow, PollStep::SlowDown { interval: None });
        assert_eq!(next_interval(5, &slow), 10);
        assert_eq!(
            next_interval(5, &parse_poll(br#"{"error":"slow_down","interval":15}"#)),
            15
        );
        assert_eq!(next_interval(5, &PollStep::Pending), 5);
        for (code, needle) in [
            ("expired_token", "expired"),
            ("token_expired", "expired"),
            ("access_denied", "declined"),
            ("device_flow_disabled", "does not allow"),
            ("incorrect_client_credentials", "does not know"),
            ("incorrect_device_code", "did not recognise"),
            ("bad_verification_code", "did not recognise"),
            ("unverified_user_email", "Verify your email"),
        ] {
            let body = format!(r#"{{"error":"{code}"}}"#);
            match parse_poll(body.as_bytes()) {
                PollStep::Failed(ForgeError::Refused(sentence)) => {
                    assert!(sentence.contains(needle), "{code}: {sentence}");
                }
                other => panic!("{code}: {other:?}"),
            }
        }
        assert_eq!(
            parse_poll(br#"{"error":"bad_refresh_token"}"#),
            PollStep::Failed(ForgeError::NeedsConnect)
        );
        match parse_poll(br#"{"access_token":"gho_x","token_type":"bearer","scope":"repo"}"#) {
            PollStep::Done(tokens) => {
                assert_eq!(tokens.access_token, "gho_x");
                assert_eq!(tokens.refresh_token, None);
                assert_eq!(tokens.expires_ms, None);
            }
            other => panic!("{other:?}"),
        }
    }

    fn code_json(seen: &testing::Seen, interval: &str, expires_in: &str) -> String {
        format!(
            r#"{{"device_code":"dc","user_code":"ABCD-1234",
                "verification_uri":"http://{}/login/device",
                "expires_in":{expires_in},"interval":{interval}}}"#,
            seen.header("host").unwrap_or_default()
        )
    }

    #[tokio::test]
    async fn the_flow_sends_no_secret_polls_until_approved_and_learns_the_login() {
        let polls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&polls);
        let fake = testing::serve(move |seen| match seen.path.as_str() {
            "/login/device/code" => {
                assert_eq!(seen.header("accept"), Some("application/json"));
                let form = seen.form();
                assert_eq!(form.get("client_id").map(String::as_str), Some("Iv1.c"));
                assert_eq!(form.get("scope").map(String::as_str), Some("repo read:org"));
                Reply::json(200, &code_json(seen, "0", "900"))
            }
            "/login/oauth/access_token" => {
                let form = seen.form();
                assert_eq!(form.get("device_code").map(String::as_str), Some("dc"));
                // The client that started the code, not the source's new one.
                assert_eq!(form.get("client_id").map(String::as_str), Some("Iv1.c"));
                assert_eq!(
                    form.get("grant_type").map(String::as_str),
                    Some("urn:ietf:params:oauth:grant-type:device_code")
                );
                assert!(!form.contains_key("client_secret"));
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    Reply::json(200, r#"{"error":"authorization_pending"}"#)
                } else {
                    Reply::json(200, r#"{"access_token":"gho_t","token_type":"bearer"}"#)
                }
            }
            "/api/user" => {
                assert_eq!(seen.header("authorization"), Some("Bearer gho_t"));
                Reply::json(200, r#"{"login":"tgorka"}"#)
            }
            other => Reply::json(404, &format!(r#"{{"message":"{other}"}}"#)),
        });
        let mut source = source(&fake.base);
        let http = testing::http();
        let code = start(&http, &source).await.expect("code");
        assert_eq!(code.vm().user_code, "ABCD-1234");
        assert_eq!(code.interval, 1, "an interval of 0 would hammer the forge");
        source.client_id = Some("Iv1.new".to_owned());
        let token = poll(&http, &source, &code, &AtomicBool::new(false))
            .await
            .expect("approved");
        assert_eq!(token.login, "tgorka");
        assert_eq!(token.client_id, "Iv1.c");
        assert_eq!(polls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_hostile_code_answer_is_bounded_and_its_page_must_be_the_forge_s() {
        let fake = testing::serve(|seen| {
            Reply::json(
                200,
                &code_json(seen, "18446744073709551615", "18446744073709551615"),
            )
        });
        let http = testing::http();
        let code = start(&http, &source(&fake.base)).await.expect("code");
        assert_eq!((code.interval, code.expires_in), (60, 1800));
        assert_eq!(
            next_interval(u64::MAX, &PollStep::SlowDown { interval: None }),
            60
        );

        let elsewhere = testing::serve(|_| {
            Reply::json(
                200,
                r#"{"device_code":"dc","user_code":"X",
                    "verification_uri":"https://evil.example/login/device"}"#,
            )
        });
        assert!(matches!(
            start(&http, &source(&elsewhere.base)).await,
            Err(ForgeError::Refused(_))
        ));

        let github = source("https://github.com");
        for (uri, opened) in [
            ("https://github.com/login/device", true),
            ("https://GitHub.com/login/device", true),
            ("https://device.github.com/login", true),
            ("http://github.com/login/device", false),
            ("https://github.com.evil.com/login/device", false),
            ("https://evilgithub.com/login/device", false),
            ("javascript:alert(1)", false),
        ] {
            assert_eq!(on_site(uri, &github).is_some(), opened, "{uri}");
        }
    }

    fn code(interval: u64) -> DeviceCode {
        DeviceCode {
            user_code: "X".to_owned(),
            verification_uri: "https://github.com/login/device".to_owned(),
            expires_in: 900,
            interval,
            device_code: "dc".to_owned(),
            client_id: "Iv1.c".to_owned(),
        }
    }

    #[tokio::test]
    async fn cancel_ends_the_wait_as_not_connected() {
        let fake = testing::serve(|_| Reply::json(200, r#"{"error":"authorization_pending"}"#));
        let source = source(&fake.base);
        let cancel = AtomicBool::new(true);
        assert_eq!(
            poll(&testing::http(), &source, &code(30), &cancel).await,
            Err(ForgeError::NeedsConnect)
        );
        assert!(fake.requests().is_empty());
    }

    /// The person cancels while the answer at `path` is on its way: what
    /// `poll` answered, and every path it asked.
    async fn cancel_while_answering(
        path: &'static str,
    ) -> (Result<StoredForgeToken, ForgeError>, Vec<String>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let fake = testing::serve(move |seen| {
            if seen.path == path {
                flag.store(true, Ordering::SeqCst);
            }
            match seen.path.as_str() {
                "/login/oauth/access_token" => {
                    Reply::json(200, r#"{"access_token":"gho_t","token_type":"bearer"}"#)
                }
                _ => Reply::json(200, r#"{"login":"tgorka"}"#),
            }
        });
        let outcome = poll(&testing::http(), &source(&fake.base), &code(1), &cancel).await;
        let asked = fake.requests().into_iter().map(|seen| seen.path).collect();
        (outcome, asked)
    }

    #[tokio::test]
    async fn a_cancel_during_the_token_answer_is_not_undone() {
        let (outcome, asked) = cancel_while_answering("/login/oauth/access_token").await;
        assert_eq!(outcome, Err(ForgeError::NeedsConnect));
        assert_eq!(
            asked,
            ["/login/oauth/access_token"],
            "nothing after the cancel"
        );
    }

    #[tokio::test]
    async fn a_cancel_during_the_login_answer_is_not_undone() {
        let (outcome, _) = cancel_while_answering("/api/user").await;
        assert_eq!(outcome, Err(ForgeError::NeedsConnect));
    }

    #[tokio::test]
    async fn an_approved_token_is_kept_when_github_does_not_say_who_is_connected() {
        let fake = testing::serve(|seen| match seen.path.as_str() {
            "/login/oauth/access_token" => {
                Reply::json(200, r#"{"access_token":"gho_t","token_type":"bearer"}"#)
            }
            _ => Reply::json(502, r#"{"message":"Bad Gateway"}"#),
        });
        let token = poll(
            &testing::http(),
            &source(&fake.base),
            &code(1),
            &AtomicBool::new(false),
        )
        .await
        .expect("kept");
        assert_eq!(
            (token.access_token.as_str(), token.login.as_str()),
            ("gho_t", "")
        );
    }
}

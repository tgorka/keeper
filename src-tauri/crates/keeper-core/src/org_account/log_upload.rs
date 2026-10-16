//! This device's `keeper.log`, published into the person's own directory of
//! the config repository: `<login>/logs/<device>.log`.
//!
//! A failure on someone else's Mac used to leave its evidence on that Mac
//! only — a recording whose microphone vanished left nothing anybody could
//! read without asking for the file. So keeper publishes the tail of its log
//! once a day while `diagnostics.log_upload` is on (the default), and at once
//! when the person presses *Send log now* in Settings › About.
//!
//! Who can read it is who can read the repository: every person of the
//! organisation (docs/account.md, *The config repository*). The log is not
//! secret-free by construction — a git diagnostic can quote a remote's URL,
//! query string and all — so every line, the oldest kept one included, is
//! scrubbed here on its way out (`scrub`): the person's home folder becomes
//! `~`, a URL's user name and password, credential-named query and fragment
//! parameters, and `Authorization`, `Bearer` and `Basic` values become `***`.
//! What stays is what the log says otherwise: account and Matrix ids, device
//! and microphone names, paths outside the home folder, error text.
//!
//! Pure: the shell reads the files, runs the clock and pushes the bytes;
//! everything decided about them is here.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::layout::{device_slug, is_own_path};
use super::state::AccountProblem;

/// Whether the daily upload runs on an install that never answered
/// `diagnostics.log_upload`.
pub const DEFAULT_ENABLED: bool = true;

/// The most of the log one upload carries, in bytes. The tail, cut at a
/// line, so the newest lines — the ones somebody is asking about — are always
/// in it. A file replaced daily at this size grows the repository's history by
/// a few tens of kilobytes a day once git compresses it.
pub const MAX_BYTES: usize = 512 * 1024;

/// How long after the last upload the daily one is due, in seconds.
pub const INTERVAL_SECS: i64 = 24 * 60 * 60;

/// Where this device's log lives in the repository.
pub fn path(login: &str, device: &str) -> String {
    format!("{login}/logs/{device}.log")
}

/// Whether `rel` is `device`'s own log — the one log file a device may
/// write, and the guard the shell's plan filters every write through.
pub fn is_own_log(login: &str, device: &str, rel: &str) -> bool {
    device_slug(device) == device && is_own_path(login, rel) && rel == path(login, device)
}

/// Whether the daily upload is due. Always when the repository's clone has
/// no file at the log's path (never uploaded, or deleted since); otherwise
/// once a day has passed both since the file last changed there
/// (`last_change_secs`) and since this process last found the upload done or
/// unneeded (`last_checked_secs`) — the second is what keeps a quiet log, whose
/// unchanged bytes commit nothing, from being re-checked at every sync.
pub fn is_due(
    file_present: bool,
    last_change_secs: Option<i64>,
    last_checked_secs: Option<i64>,
    now_secs: i64,
) -> bool {
    let a_day_since =
        |at: Option<i64>| at.is_none_or(|at| now_secs.saturating_sub(at) >= INTERVAL_SECS);
    !file_present || (a_day_since(last_change_secs) && a_day_since(last_checked_secs))
}

/// How *Send log now* ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LogUploadOutcomeVm {
    /// The repository now holds this device's newest log.
    Sent,
    /// The repository's copy already said the same: nothing was committed.
    Unchanged,
    /// This device has written no log line yet: there was nothing to send.
    NoLog,
    /// No account is signed in, or this device is not registered in it yet.
    SignedOut,
    /// The repository did not take it.
    Failed,
}

/// What *Send log now* answers: how it ended, and the sentence the button
/// shows under itself. Worded here so the webview renders it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogUploadVm {
    pub outcome: LogUploadOutcomeVm,
    pub sentence: String,
}

impl LogUploadVm {
    /// Pushed: names where it landed, so the person can point someone at it.
    pub fn sent(login: &str, device: &str) -> Self {
        Self {
            outcome: LogUploadOutcomeVm::Sent,
            sentence: format!(
                "Sent. This device's log is in the settings repository at {}.",
                path(login, device)
            ),
        }
    }

    /// Nothing to commit: the log gained no line since the last upload.
    pub fn unchanged(login: &str, device: &str) -> Self {
        Self {
            outcome: LogUploadOutcomeVm::Unchanged,
            sentence: format!(
                "Nothing new since the last upload: {} already holds this log.",
                path(login, device)
            ),
        }
    }

    /// Neither the live log nor its rotated generation holds a line.
    pub fn no_log() -> Self {
        Self {
            outcome: LogUploadOutcomeVm::NoLog,
            sentence: "No log was sent: this device has not written one yet.".to_owned(),
        }
    }

    /// No account, or a device with no name in it yet: there is no
    /// directory the log could go to.
    pub fn signed_out() -> Self {
        Self {
            outcome: LogUploadOutcomeVm::SignedOut,
            sentence: "No log was sent: this device is not signed in to an organisation account. \
                       Sign in under Settings › Account, then send it again."
                .to_owned(),
        }
    }

    /// The repository refused or could not be reached; `None` is an upload
    /// a sign-out (or forget) stopped — which cannot take back what already
    /// reached the repository, and does not say it did.
    pub fn failed(problem: Option<&AccountProblem>) -> Self {
        let sentence = match problem {
            None => "The log upload was stopped. Anything it had already pushed stays in the \
                     settings repository."
                .to_owned(),
            Some(AccountProblem::Offline) => "The log was not sent: keeper could not reach the \
                                              settings repository. Check the connection, then \
                                              send it again."
                .to_owned(),
            Some(AccountProblem::Blocked { login, .. }) => {
                format!("The log was not sent: {login}/ in the settings repository is not yours.")
            }
            Some(
                AccountProblem::NeedsSignIn(reason)
                | AccountProblem::Refused(reason)
                | AccountProblem::Failed(reason)
                | AccountProblem::SignInInterrupted(reason),
            ) => format!("The log was not sent. {reason}"),
            Some(AccountProblem::SignInDenied { .. } | AccountProblem::SignInTimedOut) => {
                "The log was not sent: the sign-in to the repository did not complete. Sign in \
                 again under Settings › Account."
                    .to_owned()
            }
        };
        Self {
            outcome: LogUploadOutcomeVm::Failed,
            sentence,
        }
    }
}

/// Drop the first, partial line of a log read that began past the start of
/// its file. That line may start inside a credential whose label lies before
/// the window, where no scrub can recognise it — so it goes before anything
/// is scrubbed. A read with no line break at all is one partial line: empty.
pub fn drop_partial_first_line(bytes: &mut Vec<u8>) {
    let first_line = bytes
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(bytes.len(), |at| at + 1);
    bytes.drain(..first_line);
}

/// What the file's first lines say about where it came from.
#[derive(Debug, Clone, Copy)]
pub struct Header<'a> {
    pub device: &'a str,
    pub version: &'a str,
    pub commit: &'a str,
}

/// The bytes to publish: a short header, then the last [`MAX_BYTES`] of
/// `older` (the newest rotated generation) followed by `current` (the live
/// file), cut at a line boundary, every line scrubbed (`scrub`) with `home`
/// written as `~`. Empty when no line is left to publish: a header alone is
/// never uploaded.
///
/// Deliberately carries no upload time: a log that gained no line since the
/// last upload produces the same bytes, and the publish commits nothing.
pub fn bundle(older: &str, current: &str, home: Option<&str>, header: Header<'_>) -> Vec<u8> {
    let home = home
        .map(|home| home.trim_end_matches('/'))
        .filter(|home| home.len() > 1);
    // Scrubbed before the cut, so the cap bounds what is actually sent.
    let (older, current) = (scrub(older, home), scrub(current, home));
    let (older, current): (&str, &str) = (&older, &current);
    let mut log = String::with_capacity(older.len().min(MAX_BYTES) + current.len().min(MAX_BYTES));
    let tail_of = |text: &str, room: usize| -> usize {
        // The byte offset where the kept tail of `text` starts.
        if text.len() <= room {
            return 0;
        }
        let mut start = text.len() - room;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        // Begin at the next whole line; a tail with no line break at all is
        // one line too long to cut, and is dropped rather than cut mid-word.
        text[start..]
            .find('\n')
            .map_or(text.len(), |newline| start + newline + 1)
    };
    let current_start = tail_of(current, MAX_BYTES);
    let room = MAX_BYTES.saturating_sub(current.len() - current_start);
    if current_start == 0 && room > 0 {
        let older_start = tail_of(older, room);
        log.push_str(&older[older_start..]);
        if !log.is_empty() && !log.ends_with('\n') {
            log.push('\n');
        }
    }
    log.push_str(&current[current_start..]);
    if log.is_empty() {
        return Vec::new();
    }
    let Header {
        device,
        version,
        commit,
    } = header;
    let mut out = format!(
        "# keeper.log of {device}, keeper {version} ({commit}).\n\
         # The newest {kib} KiB; the home folder is written as ~ and credentials as ***. \
         keeper replaces this file.\n",
        kib = MAX_BYTES / 1024,
    );
    out.push_str(&log);
    out.into_bytes()
}

/// The query and fragment parameter names whose value is a credential: a
/// name that is one of these, or ends with one after a `_`, `-` or `.` or as
/// a camelCase word (`client_secret`, `X-Amz-Signature`, `apiKey`). A name
/// that merely ends in the same letters is not one: `monkey`, `passkey`,
/// `postcode`.
const SECRET_PARAMS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "code",
    "password",
    "passwd",
    "secret",
    "key",
    "api_key",
    "sig",
    "signature",
    "auth",
];

/// `Authorization` schemes kept in front of the masked value. Any other
/// first word may itself be the credential, so the whole value goes.
const AUTH_SCHEMES: &[&str] = &[
    "basic",
    "bearer",
    "digest",
    "token",
    "negotiate",
    "ntlm",
    "dpop",
    "hoba",
    "mutual",
    "vapid",
    "aws4-hmac-sha256",
    "scram-sha-1",
    "scram-sha-256",
];

// None of these crosses a line break: each line is scrubbed on its own.
static URL_USERINFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b([a-z][a-z0-9+.\-]*://)[^\s/?#@"'<>]+@"#).expect("userinfo pattern")
});
// A quoted value runs to its closing quote, stepping over escaped
// characters (`\"`), so a debug-printed header map cannot end it early; the
// key may itself sit behind an escaped quote (`\"authorization\": …`), in
// which case the value is unquoted to this pattern and goes whole.
static AUTHORIZATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(authorization\\?["']?[ \t]*[:=][ \t]*)(?:"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'|([^\n]*))"#,
    )
    .expect("authorization pattern")
});
static BEARER_BASIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(bearer|basic)([ \t]+)[A-Za-z0-9\-._~+/]+=*").expect("bearer pattern")
});
// `%` belongs to the name so a percent-encoded name (`%61ccess_token`) is
// classified by its decoded spelling rather than slipping past.
static QUERY_PARAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([?&#])([A-Za-z0-9_.%\-]+)=([^&#\s"'<>]+)"#).expect("query pattern")
});

/// `text` as it may leave this device: `home` (already trimmed, longer than
/// `/`) written as `~`, then the credentials a line can carry written as
/// `***` — a URL's user information (`https://***@host`), a credential-named
/// query or fragment parameter's value (see [`SECRET_PARAMS`]), an
/// `Authorization` value after a known scheme, and the word after `Bearer` or
/// `Basic`. Over-scrubbing prose (`basic ***`) is the accepted price.
fn scrub<'a>(text: &'a str, home: Option<&str>) -> Cow<'a, str> {
    // A pass that changed nothing hands back the text it was given.
    fn then<'a>(text: Cow<'a, str>, pass: impl Fn(&str) -> Cow<'_, str>) -> Cow<'a, str> {
        let changed = match pass(&text) {
            Cow::Owned(changed) => Some(changed),
            Cow::Borrowed(_) => None,
        };
        changed.map_or(text, Cow::Owned)
    }
    let text = match home {
        Some(home) => redact_home(text, home),
        None => Cow::Borrowed(text),
    };
    let text = then(text, |text| URL_USERINFO.replace_all(text, "${1}***@"));
    let text = then(text, |text| {
        AUTHORIZATION.replace_all(text, |caps: &Captures<'_>| {
            let (quote, value) = match (caps.get(2), caps.get(3), caps.get(4)) {
                (Some(value), _, _) => ("\"", value.as_str()),
                (_, Some(value), _) => ("'", value.as_str()),
                (_, _, value) => ("", value.map_or("", |value| value.as_str())),
            };
            format!("{}{quote}{}{quote}", &caps[1], masked_authorization(value))
        })
    });
    let text = then(text, |text| BEARER_BASIC.replace_all(text, "${1}${2}***"));
    then(text, |text| {
        QUERY_PARAM.replace_all(text, |caps: &Captures<'_>| {
            if is_secret_param(&percent_decoded(&caps[2])) {
                format!("{}{}=***", &caps[1], &caps[2])
            } else {
                caps[0].to_owned()
            }
        })
    })
}

/// An `Authorization` value with its credential written as `***`: a known
/// scheme stays in front of it, anything else goes whole.
fn masked_authorization(value: &str) -> String {
    let value = value.trim();
    let known = |scheme: &str| {
        AUTH_SCHEMES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(scheme))
    };
    match value.split_once([' ', '\t']) {
        Some((scheme, rest)) if known(scheme) && !rest.trim().is_empty() => format!("{scheme} ***"),
        None if value.is_empty() || known(value) => value.to_owned(),
        _ => "***".to_owned(),
    }
}

/// `name` with its `%XX` escapes decoded where they spell ASCII; anything
/// else is kept as written. Only ever used to classify, never written out.
fn percent_decoded(name: &str) -> Cow<'_, str> {
    if !name.contains('%') {
        return Cow::Borrowed(name);
    }
    let bytes = name.as_bytes();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%')
            .then(|| name.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .filter(u8::is_ascii);
        match decoded {
            Some(byte) => {
                out.push(char::from(byte));
                i += 3;
            }
            None => {
                // `name` matched `[A-Za-z0-9_.%\-]+`: every byte is ASCII.
                out.push(char::from(bytes[i]));
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// Whether a query or fragment parameter called `name` carries a credential;
/// see [`SECRET_PARAMS`] for where the line is drawn.
fn is_secret_param(name: &str) -> bool {
    SECRET_PARAMS.iter().any(|word| {
        let Some(cut) = name.len().checked_sub(word.len()) else {
            return false;
        };
        if !name.is_char_boundary(cut) || !name[cut..].eq_ignore_ascii_case(word) {
            return false;
        }
        match name[..cut].chars().next_back() {
            None | Some('_' | '-' | '.') => true,
            // camelCase: `apiKey`, `accessToken` — never `monkey`.
            Some(before) => {
                (before.is_ascii_lowercase() || before.is_ascii_digit())
                    && name[cut..].starts_with(|first: char| first.is_ascii_uppercase())
            }
        }
    })
}

/// `text` with every `home` that is a whole path prefix written as `~`:
/// `/Users/marta/x` and a bare `/Users/marta` become `~/x` and `~`, while
/// `/Users/martabis` is somebody else's folder and is left alone.
fn redact_home<'a>(text: &'a str, home: &str) -> Cow<'a, str> {
    if !text.contains(home) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(home) {
        let after = &rest[at + home.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|next| !(next.is_alphanumeric() || matches!(next, '-' | '_' | '.')));
        out.push_str(&rest[..at]);
        out.push_str(if whole { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: Header<'static> = Header {
        device: "marta-air",
        version: "0.10.4",
        commit: "e19114c",
    };

    fn body(bytes: &[u8]) -> String {
        let text = String::from_utf8(bytes.to_vec()).expect("utf-8");
        text.splitn(3, '\n')
            .nth(2)
            .expect("two header lines")
            .to_owned()
    }

    #[test]
    fn only_this_devices_log_inside_this_persons_directory_is_its_own() {
        assert!(is_own_log("marta", "marta-air", "marta/logs/marta-air.log"));
        assert!(!is_own_log(
            "marta",
            "marta-air",
            "marta/logs/tgorka-mbp.log"
        ));
        assert!(!is_own_log(
            "marta",
            "marta-air",
            "tgorka/logs/marta-air.log"
        ));
        assert!(!is_own_log(
            "marta",
            "Marta Air",
            "marta/logs/Marta Air.log"
        ));
        assert!(!is_own_log("marta", "../x", "marta/logs/../x.log"));
        assert!(!is_own_log("_template", "air", "_template/logs/air.log"));
    }

    /// The reviewer's leak: a tail read that begins inside a long credential
    /// value, whose `?token=` label lies before the window. Scrubbing shrinks
    /// the text below the cap, so only dropping the partial line first keeps
    /// the unlabeled rest of the secret out.
    #[test]
    fn a_window_starting_inside_a_credential_sends_none_of_it() {
        let line = format!(
            "fetch https://git.invalid/?token={}\n",
            "REVIEWSECRET".repeat(84)
        );
        let log = line.repeat(1_000).into_bytes();
        // A window that begins in the middle of one line's token value.
        let mut window = log[line.len() * 400 + 100..].to_vec();
        drop_partial_first_line(&mut window);
        let window = String::from_utf8(window).expect("utf-8");
        let sent = String::from_utf8(bundle("", &window, None, HEADER)).expect("utf-8");
        assert!(!sent.is_empty());
        assert!(!sent.contains("REVIEWSECRET"), "a token leaked");

        let mut none = b"no line break at all".to_vec();
        drop_partial_first_line(&mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn the_upload_is_due_when_the_file_is_missing_or_a_day_has_passed_since_both_clocks() {
        let day = INTERVAL_SECS;
        // Never uploaded, or deleted from the repository since: due at once,
        // whatever either clock says.
        assert!(is_due(false, None, None, 0));
        assert!(is_due(false, Some(1_000), Some(1_000), 1_001));
        // Present, never checked in this process: the file's own age decides.
        assert!(!is_due(true, Some(1_000), None, 1_000 + day - 1));
        assert!(is_due(true, Some(1_000), None, 1_000 + day));
        // A quiet log checked an hour ago is not checked again, however old
        // the file's last change is; a day after that check it is.
        assert!(!is_due(true, Some(0), Some(5 * day), 5 * day + 3_600));
        assert!(is_due(true, Some(0), Some(5 * day), 6 * day));
        // A clock that went backwards is not a reason to upload.
        assert!(!is_due(true, Some(1_000), None, 10));
        assert!(!is_due(true, None, Some(1_000), 10));
    }

    /// The reviewer's reproduction: a git diagnostic quoting a remote whose
    /// URL carries `?access_token=`, in the rotated generation — a line
    /// written before scrubbing existed is scrubbed all the same.
    #[test]
    fn a_query_credential_in_an_older_line_never_leaves_the_device() {
        let older = "2026-10-15T08:00:00Z  WARN keeper_sync::git::cli: push failed \
                     error=fatal: unable to access \
                     'https://git.acme.dev/marta/config.git/?access_token=REVIEW_SYNTHETIC_SECRET': \
                     The requested URL returned error: 403\n";
        let kept = body(&bundle(older, "new\n", None, HEADER));
        assert!(!kept.contains("REVIEW_SYNTHETIC_SECRET"), "{kept}");
        assert!(kept.contains("config.git/?access_token=***':"), "{kept}");
        assert!(kept.ends_with("new\n"));
    }

    #[test]
    fn every_credential_shape_is_masked_and_its_label_kept() {
        for (line, want) in [
            (
                "push to https://marta:hunter2@git.acme.dev/c.git failed",
                "push to https://***@git.acme.dev/c.git failed",
            ),
            (
                "clone git+ssh://deploy@host/x and https://only-user@host/y",
                "clone git+ssh://***@host/x and https://***@host/y",
            ),
            (
                "GET /cb?code=abc123&state=s1#id_token=eyJ.x.y&page=2",
                "GET /cb?code=***&state=s1#id_token=***&page=2",
            ),
            (
                "u?refresh_token=r1&client_secret=s2&X-Amz-Signature=f0&apiKey=k9&sig=z",
                "u?refresh_token=***&client_secret=***&X-Amz-Signature=***&apiKey=***&sig=***",
            ),
            (
                "u?PASSWORD=p&passwd=q&auth=a&api_key=b&key=c&token=d",
                "u?PASSWORD=***&passwd=***&auth=***&api_key=***&key=***&token=***",
            ),
            (
                "Authorization: Bearer eyJhbGciOi.payload.sig",
                "Authorization: Bearer ***",
            ),
            (
                r#"headers={"authorization": "token ghp_abcdef", "accept": "*/*"}"#,
                r#"headers={"authorization": "token ***", "accept": "*/*"}"#,
            ),
            ("authorization: ghp_abcdef", "authorization: ***"),
            ("Authorization: Custom s3cr3t", "Authorization: ***"),
            (
                "sent bearer abc.def-ghi and Basic dXNlcjpwYXNz==",
                "sent bearer *** and Basic ***",
            ),
            // An escaped quote inside the value does not end it.
            (
                r#"headers={"authorization": "Digest username=\"marta\", response=\"REVIEW_SECRET\"", "accept": "*/*"}"#,
                r#"headers={"authorization": "Digest ***", "accept": "*/*"}"#,
            ),
            // A key behind an escaped quote: the value goes whole.
            (
                r#"body="{\"authorization\": \"Custom REVIEW_SECRET\"}""#,
                r#"body="{\"authorization\": ***"#,
            ),
            // A percent-encoded name is classified by what it spells.
            (
                "fetch https://git.invalid/r?%61ccess_token=REVIEW_SECRET&page=2",
                "fetch https://git.invalid/r?%61ccess_token=***&page=2",
            ),
        ] {
            assert_eq!(scrub(line, None), want, "{line}");
        }
    }

    /// Where the parameter rule stops: a name is a credential's only when it
    /// is one of the words or ends with one as a separate word.
    #[test]
    fn ordinary_parameters_and_text_are_left_alone() {
        for line in [
            "list?page=2&per_page=100&monkey=1&passkey=x&postcode=12&keyboard=us",
            "u?tokens=3&authority=x&signed=1&codec=opus&oauth=1&secretary=y",
            "token=abc outside a query string is a tracing field, not a parameter",
            "@marta:acme.dev joined !room:acme.dev; mic \"MacBook Air Microphone\"",
            "opened /Volumes/usb/recordings and https://git.acme.dev/c.git",
            "the Bearer\nnext line",
        ] {
            assert_eq!(scrub(line, None), line, "{line}");
        }
        assert!(is_secret_param("access_token"));
        assert!(is_secret_param("accessToken"));
        assert!(is_secret_param("x.key"));
        assert!(is_secret_param("Key"));
        assert!(!is_secret_param("monkey"));
        assert!(!is_secret_param("MONKEY"));
        assert!(!is_secret_param("hotkey"));
    }

    #[test]
    fn no_log_line_is_no_upload_not_a_header_alone() {
        assert!(bundle("", "", Some("/Users/marta"), HEADER).is_empty());
        assert!(!bundle("", "one\n", None, HEADER).is_empty());
    }

    #[test]
    fn a_small_log_is_kept_whole_after_the_older_generation() {
        let bytes = bundle("old 1\nold 2\n", "new 1\n", None, HEADER);
        assert_eq!(body(&bytes), "old 1\nold 2\nnew 1\n");
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(text.starts_with("# keeper.log of marta-air, keeper 0.10.4 (e19114c).\n"));
    }

    #[test]
    fn a_long_log_keeps_its_newest_whole_lines_within_the_cap() {
        let line = "2026-10-16T08:00:00Z  WARN keeper::recording: x\n";
        let current = line.repeat(MAX_BYTES / line.len() + 50) + "the newest line\n";
        let kept = body(&bundle("older that cannot fit\n", &current, None, HEADER));
        assert!(kept.len() <= MAX_BYTES);
        assert!(kept.ends_with("the newest line\n"));
        assert!(kept.starts_with("2026-10-16"), "starts on a whole line");
        assert!(!kept.contains("older"), "no room left for the older file");
    }

    #[test]
    fn the_older_generation_fills_only_the_room_the_live_file_leaves() {
        let older = "a\n".repeat(MAX_BYTES);
        let current = "b\n".repeat(10);
        let kept = body(&bundle(&older, &current, None, HEADER));
        assert!(kept.len() <= MAX_BYTES);
        assert!(kept.ends_with(&current));
        assert!(kept.starts_with("a\n"));
    }

    #[test]
    fn a_cut_inside_a_multibyte_character_moves_to_the_next_line() {
        let current = "é".repeat(MAX_BYTES) + "\nlast\n";
        assert_eq!(body(&bundle("", &current, None, HEADER)), "last\n");
    }

    #[test]
    fn the_home_folder_becomes_a_tilde_and_only_as_a_whole_path() {
        let current = "opened /Users/marta/Movies/rec and /Users/marta\n\
                       not /Users/martabis/x nor /Users/marta.old\n";
        let kept = body(&bundle("", current, Some("/Users/marta/"), HEADER));
        assert_eq!(
            kept,
            "opened ~/Movies/rec and ~\nnot /Users/martabis/x nor /Users/marta.old\n"
        );
        // A root or empty home redacts nothing rather than every slash.
        assert_eq!(body(&bundle("", "/x\n", Some("/"), HEADER)), "/x\n");
    }

    #[test]
    fn an_unchanged_log_produces_the_same_bytes() {
        assert_eq!(
            bundle("a\n", "b\n", Some("/Users/m"), HEADER),
            bundle("a\n", "b\n", Some("/Users/m"), HEADER)
        );
    }
}

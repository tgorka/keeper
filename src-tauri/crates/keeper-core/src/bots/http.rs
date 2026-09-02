//! The one HTTP client policy every bot request in this module shares.
//!
//! # Why this file restates a policy instead of importing one
//!
//! `keeper-sync/src/http.rs:24-40` already worked this out, on a real folder
//! and a real degraded link, and its reasoning is the reasoning here. It is
//! reproduced rather than shared for one mechanical reason: **`keeper-core`
//! may not depend on `keeper-sync`** (AD-40, enforced by
//! `bun run check:core-sync-free`), because the standalone sync daemon must not
//! inherit matrix-sdk and the iOS build must not inherit gitoxide. So the
//! choice was between two copies with one citation, and one copy with a
//! dependency edge the tree forbids. This is the copy, and this paragraph is
//! the citation.
//!
//! # A stream is bounded by silence, never by a deadline
//!
//! Quoting the source's conclusion in its own terms:
//! `ClientBuilder::timeout` bounds a whole request, and any total budget large
//! enough for the legitimate long case is far too large to catch a stall.
//! `ClientBuilder::read_timeout` bounds **silence** instead — it applies to
//! each read and resets after every successful one.
//!
//! For a chat completion the legitimate long case is even starker than it was
//! for a 1 GB LFS object: a reasoning model may think for minutes before it
//! emits a token, and a long answer may stream for minutes more. A total
//! deadline that permits that would not fire on a dead socket until the user
//! had given up twice. A silence budget fires on exactly the failure that
//! exists — a peer that stopped answering — and never on a model that is
//! simply slow.
//!
//! The budget must sit above the *server's* keep-alive interval or a healthy
//! idle stream is killed on schedule. Hermes' api-server sends a keep-alive
//! every 30 s (`CHAT_COMPLETIONS_SSE_KEEPALIVE_SECONDS = 30.0`, research §2.3),
//! which is the number [`READ_TIMEOUT`] is calibrated against.

use std::time::Duration;

use reqwest::header::{HeaderValue, AUTHORIZATION};

use crate::bots::error::BotsError;

/// The `User-Agent` every bot request carries.
///
/// A fixed, version-free string: a bot endpoint is often a local process on
/// the user's own machine, and there is nothing to gain from telling it which
/// build of keeper is talking to it.
pub const USER_AGENT: &str = "keeper";

/// How long to wait for a connection to be established.
///
/// Shorter than `keeper-sync`'s fifteen seconds because the failure it catches
/// is a different one: a bot endpoint is usually `127.0.0.1` or a host on the
/// LAN, and a base URL the user mistyped should say so quickly rather than
/// make them wait out a VPN-sized budget.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long an established stream may deliver nothing before it is a failure.
///
/// A ceiling on SILENCE, not on duration — see the module docs. Two minutes
/// because it must clear the largest server keep-alive interval keeper knows
/// about (Hermes, 30 s) with room for a model that stalls on a cold weight
/// load, and because the cost of being wrong in the generous direction is one
/// slow failure while the cost of being wrong in the tight direction is
/// killing healthy generations at random.
pub const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// How long an idle pooled connection may be kept for reuse.
///
/// The same twenty seconds `keeper-sync` settled on, for the same reason: a
/// pooled connection is only an asset while the peer still has it, and reusing
/// a connection the far side has forgotten costs a full [`READ_TIMEOUT`]
/// before anything is even attempted. `reqwest`'s own default is 90 s.
pub const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(20);

/// The client every bot request runs on.
///
/// `read_timeout` is a parameter rather than a constant because it is the seam
/// tests use to make a stall observable in milliseconds instead of two
/// minutes, and because a provider row may carry its own override (Story
/// 61.1). Pass [`READ_TIMEOUT`] for the default policy.
pub fn client(read_timeout: Duration) -> Result<reqwest::Client, BotsError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        // NOT `.timeout(..)`. See the module docs; this is the whole point of
        // the file.
        .read_timeout(read_timeout)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        // A user-supplied base URL plus a bearer token is the SSRF shape
        // OWASP's cheat sheet calls the hard case, and its directly applicable
        // rule is to stop following redirects so input validation cannot be
        // bypassed by the peer (research §10). keeper validated a base URL at
        // the point the provider row was saved; a 302 would move the request
        // — and the credential — to a host that was never validated and never
        // shown to the user. A redirect therefore surfaces as
        // `BotsError::Status`, which is honest, instead of being followed.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| BotsError::ClientBuild(err.to_string()))
}

/// Build an `Authorization: Bearer …` value, marked sensitive.
///
/// The sensitive flag is what keeps the credential out of the `http` stack's
/// own `Debug`/`Display` output, which is where a token leaks into a log
/// without anybody writing a line that prints it. Same shape as
/// `keeper-sync`'s `lfs::batch::sensitive_auth`, for the same reason.
///
/// A value the header grammar refuses — a pasted key with a trailing newline
/// is the common one — is a named refusal ([`BotsError::Credential`]) and never
/// a silently omitted header: an unauthenticated request to a bearer endpoint
/// comes back 401 and sends the user hunting the wrong bug.
pub fn authorization(token: &str) -> Result<HeaderValue, BotsError> {
    // The failing value is never echoed: it is the credential.
    let mut value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| BotsError::Credential {
            detail: "it contains a character an HTTP header cannot carry (a newline, a control \
                     character, or a non-ASCII byte)"
                .to_owned(),
        })?;
    value.set_sensitive(true);
    Ok(value)
}

/// Attach the bearer credential to a request, when there is one.
///
/// Ollama accepts and discards any key (research §3.5) and Hermes requires
/// one, so "no token" is a legitimate configuration rather than an error, and
/// the decision of whether a token is needed belongs to the provider row, not
/// here.
pub fn authorize(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> Result<reqwest::RequestBuilder, BotsError> {
    match token {
        Some(token) if !token.is_empty() => {
            Ok(request.header(AUTHORIZATION, authorization(token)?))
        }
        _ => Ok(request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_builds_with_a_millisecond_read_timeout() {
        // The seam the wire tests depend on: an arbitrary read timeout is
        // accepted, so a stall can be proven in milliseconds.
        assert!(client(Duration::from_millis(50)).is_ok());
    }

    #[test]
    fn the_authorization_value_is_marked_sensitive() {
        let value = authorization("sk-test-secret").expect("a plain ASCII key is a valid header");
        assert!(
            value.is_sensitive(),
            "an unmarked credential is printed by the http stack's own diagnostics"
        );
    }

    #[test]
    fn a_credential_with_a_newline_is_refused_without_echoing_it() {
        let err = authorization("sk-test-secret\nX-Evil: 1")
            .expect_err("a newline is header injection, not a credential");
        let text = err.to_string();
        assert!(!text.contains("sk-test-secret"), "the error echoed the key");
        assert!(matches!(err, BotsError::Credential { .. }));
    }

    #[test]
    fn an_absent_token_leaves_the_request_unauthenticated_rather_than_failing() {
        let client = client(READ_TIMEOUT).expect("client");
        let request = client.post("http://127.0.0.1:1/v1/chat/completions");
        assert!(authorize(request, None).is_ok());
    }
}

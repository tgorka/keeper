//! The one place that knows how to spell a profile's access token (AD-53).
//!
//! One secret, three consumers, three wire shapes — and until this module
//! existed each consumer hand-rolled its own. That is how the LFS batch client
//! came to send `Authorization: Bearer <PAT>`: a shape **no** git forge accepts
//! for a personal access token, because Forgejo/Gitea reserves `Bearer` for the
//! HS256 JWT its own `services/lfs/server.go` mints (`parseToken` →
//! `handleLFSToken` → `jwt.ParseWithClaims(…, LFS.JWTSecretBytes)`). A PAT is
//! not that JWT, so the server answered 401 with
//! `WWW-Authenticate: Basic realm="gitea-lfs"` while `git push` — which sends
//! the same token the *right* way — kept working.
//!
//! So the token is a type, not a `String`, and the type owns every spelling:
//!
//! | consumer | shape | why |
//! |---|---|---|
//! | `git` fetch/push | token as the Basic **username**, inert password | what the credential helper feeds git; Forgejo and GitHub both accept it |
//! | LFS batch + object transfer | `Authorization: Basic base64("<token>:")` | the same Basic credential, pre-encoded because there is no helper in the way |
//! | Forgejo REST API | `Authorization: token <token>` | Gitea/Forgejo's own API scheme (`services/auth/basic.go`), which is **not** interchangeable with Basic |
//!
//! A fourth consumer gets a method here or it does not get the token.
//!
//! Nothing in this module logs, and [`AccessToken`]'s `Debug` withholds the
//! secret the way [`crate::git::fetch::Credential`]'s does.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::git::fetch::Credential;

/// A profile's access token, and the only thing allowed to dress it.
///
/// Deliberately not `PartialEq`: comparing two secrets is not this type's job,
/// and the only comparison the engine wants — "did the keychain hand back
/// something different from what we already sent" — is a comparison of the
/// dressed header, which is what the 401 path actually re-sends.
#[derive(Clone)]
pub struct AccessToken(String);

impl std::fmt::Debug for AccessToken {
    /// Hand-written for the same reason [`Credential`]'s is: a derived `Debug`
    /// on a secret is how tokens reach log files (NFR-26).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AccessToken").field(&"<redacted>").finish()
    }
}

impl AccessToken {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The credential `git fetch`/`git push` is given.
    ///
    /// The token goes in the **username** with an inert password: that is the
    /// shape Forgejo and GitHub both accept, and putting it in the password
    /// with an empty username is the shape neither does.
    pub fn git(&self) -> Credential {
        Credential {
            username: self.0.clone(),
            secret: String::new(),
        }
    }

    /// `Authorization` for the LFS batch API and every `basic`-adapter transfer.
    ///
    /// The same credential [`Self::git`] returns, encoded by hand because no
    /// credential helper stands between us and the socket here. The empty
    /// password after the colon is deliberate and is what Forgejo's
    /// `parseToken` reads as a token login.
    pub fn lfs_basic(&self) -> String {
        // `<token>:` — RFC 7617 user-id, colon, empty password. The user-id
        // may not itself contain a colon; a PAT never does.
        format!("Basic {}", BASE64.encode(format!("{}:", self.0)))
    }

    /// `Authorization` for the Forgejo REST API (`/api/v1/...`).
    ///
    /// Its own scheme, and correct as it stands — the API accepts `token`,
    /// where the LFS endpoints do not.
    pub fn forge_api(&self) -> String {
        format!("token {}", self.0)
    }
}

/// Whether a `401`'s challenge leaves room for the Basic credential we hold.
///
/// `None` — no challenge at all — is `true`: Basic is what every forge this
/// engine talks to accepts, and refusing to retry because a server was terse
/// would strand a recoverable rejection.
///
/// A `Bearer` challenge is **false**, and that is the honest answer rather than
/// a pessimistic one: Forgejo's LFS `Bearer` is a JWT signed with the server's
/// own `LFS.JWTSecretBytes`, obtainable only through `git-lfs-authenticate`
/// over SSH (research §5.7). We cannot mint one, so re-sending anything would
/// be a second rejection.
///
/// Multiple challenges arrive comma-separated (`Basic realm="a", Bearer …`),
/// and each candidate's scheme is its first word. A comma inside a quoted
/// `realm` splits a challenge in two here; the leading fragment still carries
/// the scheme, so the answer does not change.
pub fn challenge_accepts_basic(challenge: Option<&str>) -> bool {
    let Some(challenge) = challenge else {
        return true;
    };
    let mut saw_scheme = false;
    for part in challenge.split(',') {
        let Some(scheme) = part.split_whitespace().next() else {
            continue;
        };
        // A bare parameter fragment (`realm="x"`) is not a scheme.
        if scheme.contains('=') {
            continue;
        }
        saw_scheme = true;
        if scheme.eq_ignore_ascii_case("basic") {
            return true;
        }
    }
    // A challenge we could not read a scheme out of is treated as no challenge.
    !saw_scheme
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes on the wire, because the exact bytes are what regressed.
    ///
    /// `Basic dGtuLTEyMzo=` decodes to `tkn-123:` — the token as the user-id
    /// and an empty password. Any other spelling is the bug this fixes.
    #[test]
    fn the_lfs_header_is_basic_with_the_token_as_the_username() {
        let token = AccessToken::new("tkn-123");
        assert_eq!(token.lfs_basic(), "Basic dGtuLTEyMzo=");

        let decoded = BASE64
            .decode("dGtuLTEyMzo=")
            .expect("the header we just built must decode");
        assert_eq!(String::from_utf8_lossy(&decoded), "tkn-123:");

        // The shape Forgejo answers 401 to, for contrast: `Bearer` is reserved
        // for its own LFS JWT.
        assert!(!token.lfs_basic().starts_with("Bearer"));
    }

    #[test]
    fn every_consumer_gets_its_own_spelling_of_one_secret() {
        let token = AccessToken::new("tkn-123");

        // git: token as username, inert password.
        let git = token.git();
        assert_eq!(git.username, "tkn-123");
        assert!(git.secret.is_empty());

        // The Forgejo REST API keeps its own scheme — it is correct there.
        assert_eq!(token.forge_api(), "token tkn-123");

        // And the three are genuinely different strings, which is the whole
        // reason they live together.
        assert_ne!(token.lfs_basic(), token.forge_api());
    }

    #[test]
    fn debug_never_prints_the_token() {
        let rendered = format!("{:?}", AccessToken::new("tkn-123"));
        assert!(!rendered.contains("tkn-123"), "leaked: {rendered}");
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn a_basic_challenge_is_answerable_and_a_bearer_one_is_not() {
        // Forgejo's own words, verbatim.
        assert!(challenge_accepts_basic(Some(
            "Basic realm=\"gitea-lfs\",charset=\"UTF-8\""
        )));
        assert!(challenge_accepts_basic(Some("basic")));
        // Terse server: no challenge is not a refusal of Basic.
        assert!(challenge_accepts_basic(None));
        // Offered both, we can answer one.
        assert!(challenge_accepts_basic(Some(
            "Bearer realm=\"x\", Basic realm=\"y\""
        )));
        // A JWT we cannot mint, and nothing else on offer.
        assert!(!challenge_accepts_basic(Some("Bearer realm=\"gitea-lfs\"")));
        assert!(!challenge_accepts_basic(Some("Negotiate")));
    }
}

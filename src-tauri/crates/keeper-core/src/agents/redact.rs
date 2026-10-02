//! Secrets never reach a log (security review S-17).
//!
//! A closed, tested set of secret-shaped patterns. Each match is replaced by
//! `[REDACTED secret-like: sha256:<first 12 hex>]`, so the line still says that
//! something was there and the same secret seen twice is recognisably the same,
//! while the secret itself never reaches a file that syncs to other machines.
//! Pure: the log writer applies it to every free-text field before
//! serialising, and Epic 95's hermes scan reuses the same set.

use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

/// Which pattern matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// A Matrix access token, `syt_…`.
    MatrixToken,
    /// An Anthropic key, `sk-ant-…`.
    AnthropicKey,
    /// An OpenAI-style key, `sk-…`.
    OpenAiKey,
    /// A GitHub token: `ghp_…`, `gho_…` or `github_pat_…`.
    GitHubToken,
    /// A PEM private key block.
    PrivateKey,
    /// An AWS access key id, `AKIA…`.
    AwsAccessKey,
    /// A Slack token, `xoxb-…` or `xoxp-…`.
    SlackToken,
    /// A JSON Web Token, `eyJ….….…`.
    Jwt,
    /// A PostHog key, `phx_…` or `phc_…`.
    PostHogKey,
    /// The whole text, withheld because the pattern set could not be built.
    Withheld,
}

impl SecretKind {
    /// A stable name for the kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MatrixToken => "matrix_token",
            Self::AnthropicKey => "anthropic_key",
            Self::OpenAiKey => "openai_key",
            Self::GitHubToken => "github_token",
            Self::PrivateKey => "private_key",
            Self::AwsAccessKey => "aws_access_key",
            Self::SlackToken => "slack_token",
            Self::Jwt => "jwt",
            Self::PostHogKey => "posthog_key",
            Self::Withheld => "withheld",
        }
    }
}

/// One secret that was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    /// Which pattern matched.
    pub kind: SecretKind,
    /// The first 12 hex digits of the secret's SHA-256, as in the marker.
    pub sha256: String,
}

/// A text with its secrets replaced, and what was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    /// The text, each secret replaced by its marker.
    pub text: String,
    /// Every replacement, in order of appearance. Empty means `text` is the
    /// input unchanged.
    pub found: Vec<Redaction>,
}

/// The patterns, in the order the alternation tries them at one position:
/// the longer prefixes before the shorter ones they contain (`sk-ant-` before
/// `sk-`). Every token pattern starts at a word boundary, so `task-…` is not
/// an `sk-` key.
const PATTERNS: [(SecretKind, &str); 9] = [
    (
        SecretKind::PrivateKey,
        // A block cut before its END marker (a truncated tool result, a key
        // pasted without its footer) runs to the end of the text.
        r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----(?s:.*?)(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)",
    ),
    (SecretKind::MatrixToken, r"\bsyt_[A-Za-z0-9_\-]{16,}"),
    (SecretKind::AnthropicKey, r"\bsk-ant-[A-Za-z0-9_\-]{20,}"),
    (SecretKind::OpenAiKey, r"\bsk-[A-Za-z0-9_\-]{20,}"),
    (
        SecretKind::GitHubToken,
        r"\b(?:ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})",
    ),
    (SecretKind::AwsAccessKey, r"\bAKIA[0-9A-Z]{16}\b"),
    (SecretKind::SlackToken, r"\bxox[bp]-[A-Za-z0-9\-]{10,}"),
    (
        SecretKind::Jwt,
        r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
    ),
    (SecretKind::PostHogKey, r"\bph[xc]_[A-Za-z0-9]{20,}"),
];

static MATCHER: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let alternation = PATTERNS
        .iter()
        .enumerate()
        .map(|(index, (_, pattern))| format!("(?P<p{index}>{pattern})"))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&alternation).ok()
});

fn short_sha(secret: &str) -> String {
    let digest = hex::encode(Sha256::digest(secret.as_bytes()));
    digest[..12].to_owned()
}

/// Replace every secret-shaped run in `text`.
///
/// If the pattern set could not be compiled (a bug, never input-dependent),
/// the whole text is withheld rather than written unchecked.
pub fn redact_secrets(text: &str) -> Redacted {
    let Some(regex) = MATCHER.as_ref() else {
        return Redacted {
            text: format!("[REDACTED secret-like: sha256:{}]", short_sha(text)),
            found: vec![Redaction {
                kind: SecretKind::Withheld,
                sha256: short_sha(text),
            }],
        };
    };
    let mut found = Vec::new();
    let mut out = String::new();
    let mut last = 0;
    for captures in regex.captures_iter(text) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let kind = PATTERNS
            .iter()
            .enumerate()
            .find(|(index, _)| captures.name(&format!("p{index}")).is_some())
            .map_or(SecretKind::PrivateKey, |(_, (kind, _))| *kind);
        let sha256 = short_sha(whole.as_str());
        out.push_str(&text[last..whole.start()]);
        out.push_str("[REDACTED secret-like: sha256:");
        out.push_str(&sha256);
        out.push(']');
        last = whole.end();
        found.push(Redaction { kind, sha256 });
    }
    if found.is_empty() {
        return Redacted {
            text: text.to_owned(),
            found,
        };
    }
    out.push_str(&text[last..]);
    Redacted { text: out, found }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(secret: &str) -> String {
        format!("[REDACTED secret-like: sha256:{}]", short_sha(secret))
    }

    #[test]
    fn every_pattern_in_the_set_is_replaced_by_its_marker() {
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\nAAAABG5vbmUAAAAEbm9uZQ\n-----END OPENSSH PRIVATE KEY-----";
        let cases: [(SecretKind, &str); 11] = [
            (SecretKind::MatrixToken, "syt_dGdvcmth_yXkQzLmNoPqRsTuVwXy_1a2B3c"),
            (SecretKind::AnthropicKey, "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWx"),
            (SecretKind::OpenAiKey, "sk-proj-AbCdEfGhIjKlMnOpQrStUvWx12"),
            (SecretKind::GitHubToken, "ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
            (SecretKind::GitHubToken, "gho_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
            (SecretKind::GitHubToken, "github_pat_11ABCDEFG0123456789_abcdefghijk"),
            (SecretKind::PrivateKey, pem),
            (SecretKind::AwsAccessKey, "AKIAIOSFODNN7EXAMPLE"),
            (SecretKind::SlackToken, "xoxb-1234567890-abcdefghij"),
            (
                SecretKind::Jwt,
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            ),
            (SecretKind::PostHogKey, "phx_AbCdEfGhIjKlMnOpQrStUvWx"),
        ];
        for (kind, secret) in cases {
            let text = format!("before {secret} after");
            let redacted = redact_secrets(&text);
            assert_eq!(
                redacted.text,
                format!("before {} after", marker(secret)),
                "{kind:?}"
            );
            assert_eq!(
                redacted.found,
                vec![Redaction {
                    kind,
                    sha256: short_sha(secret)
                }],
                "{kind:?}"
            );
            assert!(!redacted.text.contains(secret));
        }
        let phc = redact_secrets("phc_AbCdEfGhIjKlMnOpQrStUvWx");
        assert_eq!(phc.found[0].kind, SecretKind::PostHogKey);
        let xoxp = redact_secrets("xoxp-1234567890-abcdefghij");
        assert_eq!(xoxp.found[0].kind, SecretKind::SlackToken);
        // A block cut before its END marker is redacted to the end of the text.
        let head = "key file:\n";
        let cut = format!("{head}-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA7bq\nAbCdEf");
        let truncated = redact_secrets(&cut);
        assert_eq!(truncated.found[0].kind, SecretKind::PrivateKey);
        assert_eq!(
            truncated.text,
            format!("{head}{}", marker(&cut[head.len()..]))
        );
    }

    #[test]
    fn ordinary_text_is_left_byte_for_byte() {
        for text in [
            "task-abcdefghijklmnopqrstuvwxyz is a card slug",
            "ask-me-anything-about-the-release-plan",
            "the sk- prefix alone, sk-short",
            "eyJ is how a JWT starts, but eyJabc.def is not one",
            "AKIA is a prefix; AKIAshort is not a key",
            "-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----",
            "",
        ] {
            let redacted = redact_secrets(text);
            assert_eq!(redacted.text, text);
            assert!(redacted.found.is_empty(), "{text}");
        }
    }

    #[test]
    fn two_secrets_are_each_replaced_and_the_same_secret_gets_the_same_marker() {
        let key = "sk-AbCdEfGhIjKlMnOpQrStUvWx";
        let text = format!("{key} and AKIAIOSFODNN7EXAMPLE and {key}");
        let redacted = redact_secrets(&text);
        assert_eq!(redacted.found.len(), 3);
        assert_eq!(redacted.found[0], redacted.found[2]);
        assert_eq!(
            redacted.text,
            format!(
                "{} and {} and {}",
                marker(key),
                marker("AKIAIOSFODNN7EXAMPLE"),
                marker(key)
            )
        );
        // The marker is the first 12 hex digits of the secret's own SHA-256.
        assert!(redacted
            .text
            .contains("[REDACTED secret-like: sha256:1a5d44a2dca1]"));
    }
}

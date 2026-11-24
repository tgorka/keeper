//! Guards that keep raw credentials out of replicated settings values.

use std::sync::LazyLock;

use regex::Regex;

use crate::refusal::Refusal;
use crate::travel::{CREDENTIAL_CHOICES, CREDENTIAL_FAMILIES};

/// Named patterns matching credential-shaped values. Identical to
/// `credentialPatterns` in `syncular/settings.vectors.json`; every regex is
/// valid in both Rust `regex` and ECMAScript.
pub const PATTERNS: &[(&str, &str)] = &[
    ("private_key", r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----"),
    ("matrix_token", r"\bsyt_[A-Za-z0-9_\-]{16,}"),
    ("anthropic_key", r"\bsk-ant-[A-Za-z0-9_\-]{20,}"),
    ("openai_key", r"\bsk-[A-Za-z0-9_\-]{20,}"),
    (
        "github_token",
        r"\b(?:ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})",
    ),
    ("aws_access_key", r"\bAKIA[0-9A-Z]{16}\b"),
    ("slack_token", r"\bxox[bp]-[A-Za-z0-9\-]{10,}"),
    (
        "jwt",
        r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
    ),
    ("posthog_key", r"\bph[xc]_[A-Za-z0-9]{20,}"),
    ("bearer", r"\b[Bb]earer [A-Za-z0-9._~+/\-]{16,}=*"),
    ("url_password", r"[a-z][a-z0-9+.\-]*://[^/\s:@]+:[^/\s@]+@"),
];

#[allow(clippy::expect_used)]
static COMPILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|&(_, pattern)| {
            Regex::new(pattern).expect("compiled-in credential pattern must be a valid regex")
        })
        .collect()
});

/// Returns the name of the first pattern in [`PATTERNS`] that matches `value`.
#[must_use]
pub fn credential_shaped(value: &str) -> Option<&'static str> {
    PATTERNS
        .iter()
        .zip(COMPILED.iter())
        .find_map(|(&(name, _), re)| re.is_match(value).then_some(name))
}

/// Whether `key` (bare or `pin:`) belongs to one of [`CREDENTIAL_FAMILIES`].
#[must_use]
pub fn is_credential_choice(key: &str) -> bool {
    let bare = key.strip_prefix("pin:").unwrap_or(key);
    CREDENTIAL_FAMILIES
        .iter()
        .any(|prefix| bare.starts_with(prefix))
}

/// Validates `value` for `key`: credential-source family keys (including
/// `pin:` forms) only allow [`CREDENTIAL_CHOICES`]; all other keys reject
/// values matched by [`credential_shaped`].
pub fn check_value(key: &str, value: &str) -> Result<(), Refusal> {
    if is_credential_choice(key) {
        if CREDENTIAL_CHOICES.contains(&value) {
            return Ok(());
        }
        return Err(Refusal::CredentialValue);
    }
    if credential_shaped(value).is_some() {
        return Err(Refusal::CredentialValue);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_credential_source_family_allows_only_choices() {
        assert_eq!(
            check_value("sync.credential_source.primary", "account"),
            Ok(())
        );
        assert_eq!(
            check_value("sync.credential_source.primary", "token-value"),
            Err(Refusal::CredentialValue)
        );
    }

    #[test]
    fn bots_provider_credential_source_family_allows_only_choices() {
        assert_eq!(
            check_value("pin:bots.provider_credential_source.openai", "own"),
            Ok(())
        );
        assert_eq!(
            check_value("pin:bots.provider_credential_source.openai", "env"),
            Err(Refusal::CredentialValue)
        );
    }

    #[test]
    fn non_family_key_rejects_credential_shaped_value() {
        assert_eq!(check_value("debug.mode", "verbose"), Ok(()));
        assert_eq!(
            check_value("debug.mode", "sk-1234567890abcdefghij"),
            Err(Refusal::CredentialValue)
        );
    }
}

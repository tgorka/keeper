//! What keeper reads out of a validated token's claims: the username that
//! names the person's directory, their roles, and the required-role gate.

use serde_json::Value;

use super::AccountError;

const MAX_USERNAME: usize = 64;

/// The username the directory `<login>/` is named after.
///
/// Refused when missing or unsafe as a single path segment: only
/// `[A-Za-z0-9._-]`, at most 64 bytes, and never a leading `.` (hidden files,
/// `..`) or `_` (`_template/` and the other directories that are not people).
pub fn username(claims: &Value, claim: &str) -> Result<String, AccountError> {
    let Some(value) = claims.get(claim).and_then(Value::as_str) else {
        return Err(AccountError::Refused(format!(
            "The sign-in did not say who you are: the token has no \"{claim}\" username. Ask your administrator."
        )));
    };
    let safe = !value.is_empty()
        && value.len() <= MAX_USERNAME
        && !value.starts_with(['.', '_'])
        && !value.ends_with('.')
        && !windows_reserved(value)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !safe {
        return Err(AccountError::Refused(format!(
            "Your username \"{value}\" cannot name a folder in the settings repository. Ask your administrator."
        )));
    }
    Ok(value.to_owned())
}

/// A device name Windows will not create as a directory, with or without an
/// extension (`con`, `CON.x`): one such login would break every Windows
/// checkout of the repository.
fn windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9')
        }
    }
}

/// The roles in `claim`: an array of strings, or an object whose keys are the
/// roles (Zitadel's `urn:…:roles`). The claim is looked up by its exact name
/// first — Zitadel's names contain dots and colons — and only then as a dotted
/// path (`realm_access.roles`). Sorted and deduplicated; anything else is no
/// roles.
pub fn roles(claims: &Value, claim: &str) -> Vec<String> {
    let found = claims.get(claim).or_else(|| {
        claim
            .split('.')
            .try_fold(claims, |value, segment| value.get(segment))
    });
    let mut roles: Vec<String> = match found {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        // Some providers collapse a one-element list to its element.
        Some(Value::String(role)) => vec![role.clone()],
        _ => Vec::new(),
    };
    roles.sort();
    roles.dedup();
    roles
}

/// The descriptor's `required_role` gate, as a sentence the person can act on.
pub fn require_role(
    roles: &[String],
    required: Option<&str>,
    account_name: &str,
) -> Result<(), AccountError> {
    match required {
        Some(role) if !roles.iter().any(|held| held == role) => {
            Err(AccountError::Refused(format!(
            "Your {account_name} account does not have the {role} role. Ask your administrator."
        )))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn refused(result: Result<String, AccountError>) -> bool {
        matches!(result, Err(AccountError::Refused(_)))
    }

    #[test]
    fn a_plain_username_is_accepted_as_written() {
        let claims = json!({ "preferred_username": "Tom.Gorka-2_x" });
        assert_eq!(
            username(&claims, "preferred_username").expect("safe"),
            "Tom.Gorka-2_x"
        );
    }

    #[test]
    fn a_username_that_could_escape_or_hide_its_directory_is_refused() {
        for bad in [
            "",
            ".",
            "..",
            ".hidden",
            "_template",
            "a/b",
            "a\\b",
            "a b",
            "ł",
            &"a".repeat(65),
        ] {
            let claims = json!({ "preferred_username": bad });
            assert!(refused(username(&claims, "preferred_username")), "{bad:?}");
        }
        assert!(!refused(username(&json!({ "u": "a".repeat(64) }), "u")));
    }

    #[test]
    fn a_username_windows_cannot_create_as_a_folder_is_refused() {
        for bad in ["con", "NUL", "Aux.dev", "com1", "LPT9", "prn.x", "j."] {
            let claims = json!({ "preferred_username": bad });
            assert!(refused(username(&claims, "preferred_username")), "{bad:?}");
        }
        for fine in ["console", "com10", "com0", "lpt", "nullable", "j.k"] {
            assert!(!refused(username(&json!({ "u": fine }), "u")), "{fine:?}");
        }
    }

    #[test]
    fn a_missing_or_non_string_username_is_refused() {
        assert!(refused(username(&json!({}), "preferred_username")));
        assert!(refused(username(
            &json!({ "preferred_username": 7 }),
            "preferred_username"
        )));
    }

    #[test]
    fn roles_read_both_shapes_sorted_and_deduplicated() {
        let array = json!({ "groups": ["keeper", "admin", "keeper", 3] });
        assert_eq!(roles(&array, "groups"), ["admin", "keeper"]);

        let claim = "urn:zitadel:iam:org:project:283746519283746001:roles";
        let object = json!({ claim: { "keeper": { "283": "acme.dev" }, "admin": {} } });
        assert_eq!(roles(&object, claim), ["admin", "keeper"]);
    }

    #[test]
    fn the_exact_claim_name_wins_over_a_dotted_path() {
        let claims = json!({
            "realm_access.roles": ["exact"],
            "realm_access": { "roles": ["nested"] },
        });
        assert_eq!(roles(&claims, "realm_access.roles"), ["exact"]);

        let nested_only = json!({ "realm_access": { "roles": ["nested"] } });
        assert_eq!(roles(&nested_only, "realm_access.roles"), ["nested"]);
        assert!(roles(&nested_only, "realm_access.missing").is_empty());
    }

    #[test]
    fn a_missing_required_role_is_refused_with_the_account_name() {
        let held = vec!["admin".to_owned()];
        let Err(AccountError::Refused(sentence)) = require_role(&held, Some("keeper"), "Acme")
        else {
            panic!("a missing role must be refused");
        };
        assert_eq!(
            sentence,
            "Your Acme account does not have the keeper role. Ask your administrator."
        );
        assert!(require_role(&held, Some("admin"), "Acme").is_ok());
        assert!(require_role(&[], None, "Acme").is_ok());
    }
}

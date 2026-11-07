//! Reference envelopes (contract §3, S06-AC5): the reserved property member
//! `$locator`, permitted on `Resource` nodes only. It *describes* where a
//! resource lives and what it is; nothing here resolves, fetches or opens
//! one, and no resolver is implemented ([`SUPPORTED_RESOLVERS`] is empty).
//!
//! ```text
//! {"kind": "internal"|"git"|"s3"|"relational"|"https"|"ssh_fs"|"local_fs",
//!  "management": "managed"|"external",
//!  "locator": <string>, "selector": <string>|null, "version": <string>|null,
//!  "format": <string>|null,
//!  "capabilities"?: [<string>…], "credential_ref"?: <string>, "extensions"?: {…}}
//! ```
//!
//! Credentials never reach an accepted event or a replica row: [`check`]
//! refuses, before persistence,
//!
//! - URL userinfo (`//user[:password]@host…`, after any scheme, nested
//!   ones such as `jdbc:postgresql:` included, or none) in `locator` or
//!   `selector`;
//! - a query or fragment parameter of [`CREDENTIAL_PARAMETERS`] or
//!   [`DSN_PASSWORD_KEYS`] (compared case-insensitively after
//!   percent-decoding, surrounding whitespace ignored);
//! - a DSN `key=value` pair (libpq, ODBC, JDBC keyword forms) whose key is
//!   one of [`DSN_PASSWORD_KEYS`]: every key position is read, with
//!   whitespace allowed around `=` and pairs separated by whitespace or
//!   `;`; a key is compared ASCII-case-insensitively with quotes, `_` and
//!   `-` removed. Quoting is deliberately not honoured, so a quoted value
//!   can never hide a pair (at worst a quoted value is refused);
//! - anywhere in the envelope, an object key named like a credential
//!   ([`credential_named`]) other than the top-level `credential_ref`.
//!
//! `credential_ref` is an opaque handle: it names a credential held
//! elsewhere and grants no access. `capabilities` are stored observations,
//! never grants. Universal prose secret detection is not promised.

use serde_json::{Map, Value};

use crate::Refusal;

/// The reserved property member that holds a Resource's envelope.
pub const MEMBER: &str = "$locator";
/// The envelope kinds.
pub const KINDS: [&str; 7] = [
    "internal",
    "git",
    "s3",
    "relational",
    "https",
    "ssh_fs",
    "local_fs",
];
/// Who manages the resource.
pub const MANAGEMENT: [&str; 2] = ["managed", "external"];
/// The resolvers this build implements: none. A declared capability is an
/// observation, never an advertised resolver.
pub const SUPPORTED_RESOLVERS: [&str; 0] = [];
/// Query and fragment parameters that carry credentials (case-insensitive).
pub const CREDENTIAL_PARAMETERS: [&str; 10] = [
    "token",
    "access_token",
    "password",
    "secret",
    "sig",
    "signature",
    "x-amz-signature",
    "x-amz-credential",
    "x-amz-security-token",
    "x-goog-signature",
];
/// DSN keywords (folded as the module says) that carry a password.
pub const DSN_PASSWORD_KEYS: [&str; 4] = ["password", "pwd", "passwd", "sslpassword"];
/// Key names (lowercased, `_`/`-` removed) that name a credential, exactly
/// or as a suffix.
pub const CREDENTIAL_KEYS: [&str; 13] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "apikey",
    "privatekey",
    "secretkey",
    "accesskey",
    "credential",
    "credentials",
    "authorization",
    "bearer",
];
/// The most bytes `locator` or `selector` may have.
pub const MAX_LOCATOR_BYTES: usize = 4096;
/// The most bytes `version`, `format`, a capability or `credential_ref`
/// may have.
pub const MAX_FIELD_BYTES: usize = 256;
/// The most capabilities an envelope may declare.
pub const MAX_CAPABILITIES: usize = 32;

const REQUIRED: [&str; 6] = [
    "kind",
    "management",
    "locator",
    "selector",
    "version",
    "format",
];
const OPTIONAL: [&str; 3] = ["capabilities", "credential_ref", "extensions"];

fn plain(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}

fn nullable(fields: &Map<String, Value>, key: &str, max: usize) -> Result<(), Refusal> {
    match fields.get(key) {
        Some(Value::Null) => Ok(()),
        Some(Value::String(s)) if plain(s, max) => Ok(()),
        _ => Err(Refusal::InvalidChange),
    }
}

/// Whether an object key names a credential: lowercased with `_` and `-`
/// removed, it is one of [`CREDENTIAL_KEYS`] or ends with one.
#[must_use]
pub fn credential_named(key: &str) -> bool {
    let folded: String = key
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect();
    CREDENTIAL_KEYS.iter().any(|k| folded.ends_with(k))
}

/// Percent-decode ASCII (an invalid escape stays as written).
fn decoded(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                #[allow(clippy::cast_possible_truncation)]
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `key`, folded (ASCII-lowercased, quotes, braces, `_` and `-`
/// removed), is one of [`DSN_PASSWORD_KEYS`].
fn dsn_password_key(key: &str) -> bool {
    let folded: String = key
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | '\'' | '"' | '{' | '}'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    DSN_PASSWORD_KEYS.contains(&folded.as_str())
}

/// Whether `text` holds a DSN pair with a password key. Every key
/// position is read: a key runs to whitespace, `;` or `=`, whitespace may
/// surround `=`, and a value runs to whitespace or `;` (quotes are not
/// honoured, so a value can only end early, never swallow a later pair).
fn dsn_password(text: &str) -> bool {
    let separator = |c: char| c.is_whitespace() || c == ';';
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(separator);
        if rest.is_empty() {
            return false;
        }
        let end = rest
            .find(|c: char| separator(c) || c == '=')
            .unwrap_or(rest.len());
        let key = &rest[..end];
        let after = rest[end..].trim_start_matches(char::is_whitespace);
        match after.strip_prefix('=') {
            Some(value) => {
                if dsn_password_key(key) {
                    return true;
                }
                let value = value.trim_start_matches(char::is_whitespace);
                let end = value.find(separator).unwrap_or(value.len());
                rest = &value[end..];
            }
            // A bare word: `end` is past at least one character, since
            // `rest` starts with neither a separator nor `=`.
            None => rest = &rest[end..],
        }
    }
}

/// Refuse a credential carried by a locator string: URL userinfo, a
/// credential query/fragment parameter, or a DSN password pair.
pub fn check_text(text: &str) -> Result<(), Refusal> {
    // URL userinfo: an authority holding `@`, wherever a `//` begins one:
    // after a `:` (any scheme, a nested one such as `jdbc:postgresql:`
    // included) or at the start (scheme-relative).
    let mut from = 0;
    while let Some(at) = text[from..].find("//") {
        let slashes = from + at;
        let start = slashes + 2;
        if slashes == 0 || text[..slashes].ends_with(':') {
            let authority = text[start..]
                .split(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
                .next()
                .unwrap_or_default();
            if authority.contains('@') {
                return Err(Refusal::InvalidChange);
            }
        }
        from = start;
    }
    // Query and fragment parameters.
    let parameters = text
        .split_once('?')
        .map(|(_, q)| q)
        .into_iter()
        .chain(text.split_once('#').map(|(_, f)| f));
    for part in parameters {
        for pair in part.split(['&', ';', '#', '?']) {
            let key = decoded(pair.split('=').next().unwrap_or_default()).to_ascii_lowercase();
            let key = key.trim();
            if CREDENTIAL_PARAMETERS.contains(&key) || dsn_password_key(key) {
                return Err(Refusal::InvalidChange);
            }
        }
    }
    if dsn_password(text) {
        return Err(Refusal::InvalidChange);
    }
    Ok(())
}

/// Refuse any object key named like a credential, at any depth.
fn no_credential_keys(value: &Value) -> Result<(), Refusal> {
    match value {
        Value::Object(members) => {
            for (key, member) in members {
                if credential_named(key) {
                    return Err(Refusal::InvalidChange);
                }
                no_credential_keys(member)?;
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(no_credential_keys),
        _ => Ok(()),
    }
}

/// Check a `$locator` envelope's shape and refuse any credential it
/// carries (the module's rules). Nested values are checked as property
/// values by the caller ([`crate::value`]).
pub fn check(value: &Value) -> Result<(), Refusal> {
    let fields = value.as_object().ok_or(Refusal::InvalidChange)?;
    if !REQUIRED.iter().all(|k| fields.contains_key(*k))
        || fields
            .keys()
            .any(|k| !REQUIRED.contains(&k.as_str()) && !OPTIONAL.contains(&k.as_str()))
    {
        return Err(Refusal::InvalidChange);
    }
    let kind = fields.get("kind").and_then(Value::as_str);
    let management = fields.get("management").and_then(Value::as_str);
    if !kind.is_some_and(|k| KINDS.contains(&k))
        || !management.is_some_and(|m| MANAGEMENT.contains(&m))
    {
        return Err(Refusal::InvalidChange);
    }
    let locator = match fields.get("locator") {
        Some(Value::String(s)) if plain(s, MAX_LOCATOR_BYTES) => s,
        _ => return Err(Refusal::InvalidChange),
    };
    nullable(fields, "selector", MAX_LOCATOR_BYTES)?;
    nullable(fields, "version", MAX_FIELD_BYTES)?;
    nullable(fields, "format", MAX_FIELD_BYTES)?;
    check_text(locator)?;
    if let Some(Value::String(selector)) = fields.get("selector") {
        check_text(selector)?;
    }
    if let Some(capabilities) = fields.get("capabilities") {
        let items = capabilities.as_array().ok_or(Refusal::InvalidChange)?;
        if items.len() > MAX_CAPABILITIES {
            return Err(Refusal::InvalidChange);
        }
        let mut seen = std::collections::BTreeSet::new();
        for item in items {
            match item.as_str() {
                Some(c) if plain(c, MAX_FIELD_BYTES) && seen.insert(c) => {}
                _ => return Err(Refusal::InvalidChange),
            }
        }
    }
    match fields.get("credential_ref") {
        None => {}
        Some(Value::String(handle)) if plain(handle, MAX_FIELD_BYTES) => check_text(handle)?,
        Some(_) => return Err(Refusal::InvalidChange),
    }
    match fields.get("extensions") {
        None => {}
        Some(extensions @ Value::Object(_)) => no_credential_keys(extensions)?,
        Some(_) => return Err(Refusal::InvalidChange),
    }
    Ok(())
}

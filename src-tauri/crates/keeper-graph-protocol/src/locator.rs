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
//! Credentials never reach an accepted event or a replica row. [`check`]
//! admits `locator`, `selector` and `credential_ref` only when the text
//! matches one of three allowlist grammars ([`check_text`]), and refuses
//! everything else before persistence:
//!
//! - **URL** (the text holds `://` or starts with `//`): `scheme://authority
//!   path [?query] [#fragment]`, printable ASCII only. The scheme is one of
//!   [`URL_SCHEMES`] (any case). The authority is `host[:port]` and nothing
//!   else: no `@` (so no userinfo at all), no `%`, a host of unreserved
//!   characters or a bracketed IPv6 literal, and empty only for `file`.
//!   Path, query and fragment use RFC 3986 characters; the query and
//!   fragment hold no `@` (libpq reads userinfo up to the first `@`, past a
//!   `?` or `#`). Every query and fragment key (`&`/`;` separated),
//!   percent-decoded once, must be unreserved characters and, ASCII-folded,
//!   must be neither in [`CREDENTIAL_PARAMETERS`] or [`DSN_PASSWORD_KEYS`]
//!   nor [`credential_named`];
//! - **libpq keyword DSN** (otherwise, the text holds `=`): tokenised
//!   exactly as libpq's `conninfo_parse` does (spaces around `=`, quoted
//!   values, backslash escapes in and out of quotes), printable ASCII only,
//!   but stricter: every key is one of [`DSN_KEYS`] (exact, as libpq), no
//!   key repeats, a value is followed by the end or a space (libpq also
//!   accepts a key straight after a closing quote), and a value, unescaped,
//!   holds only plain-reference characters and spaces (no `=`, `@`, `'`,
//!   `\`);
//! - **plain reference** (otherwise): a path or opaque name (`/srv/a.md`,
//!   `host:/srv/notes`, `keeper:drive/notes`, `public.notes`) of unreserved
//!   characters, `/`, `:`, `+` and `,`, with no `//`.
//!
//! Whitespace and non-ASCII are refused in URLs and references (a path with
//! a space is refused, not guessed at). Anywhere in the envelope, an object
//! key named like a credential ([`credential_named`]) other than the
//! top-level `credential_ref` is refused too.
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
/// Keys (ASCII-folded) that carry a password; refused as URL query and
/// fragment keys.
pub const DSN_PASSWORD_KEYS: [&str; 4] = ["password", "pwd", "passwd", "sslpassword"];
/// URL schemes a locator may use (compared ASCII-case-insensitively): what
/// the seven kinds need (`https`, `git`, `ssh`/`sftp`, `s3`, libpq's
/// `postgres`/`postgresql`, `file`). Every other scheme is refused.
pub const URL_SCHEMES: [&str; 8] = [
    "https",
    "git",
    "ssh",
    "sftp",
    "s3",
    "postgres",
    "postgresql",
    "file",
];
/// The libpq keywords a keyword-form DSN may hold (exact, as libpq compares
/// them). Every other key, a password-carrying one included, is refused.
pub const DSN_KEYS: [&str; 8] = [
    "host",
    "hostaddr",
    "port",
    "dbname",
    "user",
    "passfile",
    "sslmode",
    "application_name",
];
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

/// RFC 3986 unreserved: `A-Z a-z 0-9 - . _ ~`.
fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// A plain reference's characters (also a DSN value's, besides spaces).
fn reference_byte(b: u8) -> bool {
    unreserved(b) || matches!(b, b'/' | b':' | b'+' | b',')
}

/// A URL path's characters besides `%HH` (RFC 3986 `pchar` and `/`, less
/// `;` and `=`).
fn path_byte(b: u8) -> bool {
    unreserved(b)
        || matches!(
            b,
            b'/' | b':' | b'@' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b','
        )
}

/// A URL query's or fragment's characters besides `%HH` (RFC 3986, less
/// `@`).
fn query_byte(b: u8) -> bool {
    unreserved(b)
        || matches!(
            b,
            b'/' | b'?'
                | b':'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
        )
}

/// Whether every byte of `s` is `%HH` or satisfies `allowed`.
fn escaped(s: &str, allowed: fn(u8) -> bool) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            match (bytes.get(i + 1), bytes.get(i + 2)) {
                (Some(h), Some(l)) if h.is_ascii_hexdigit() && l.is_ascii_hexdigit() => i += 3,
                _ => return false,
            }
        } else if allowed(b) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

/// Percent-decode `s` once (its escapes already checked by [`escaped`]).
fn decoded(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = |at: usize| bytes.get(at).and_then(|h| char::from(*h).to_digit(16));
        match (b, hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                #[allow(clippy::cast_possible_truncation)]
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// `host[:port]`: a non-empty unreserved host or a bracketed IPv6 literal,
/// and an optional 1–5 digit port. Nothing else (no `@`, no `%`).
fn authority(text: &str) -> bool {
    let (host, port) = match text.strip_prefix('[') {
        Some(literal) => match literal.split_once(']') {
            Some((ip, rest)) => {
                let ip_ok = !ip.is_empty()
                    && ip
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
                if !ip_ok {
                    return false;
                }
                (ip, rest)
            }
            None => return false,
        },
        None => match text.find(':') {
            Some(colon) => (&text[..colon], &text[colon..]),
            None => (text, ""),
        },
    };
    let port_ok = port.is_empty()
        || port.strip_prefix(':').is_some_and(|digits| {
            (1..=5).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
        });
    !host.is_empty() && (text.starts_with('[') || host.bytes().all(unreserved)) && port_ok
}

/// A query's or fragment's parameters: RFC 3986 characters without `@`,
/// and every key, decoded once, of unreserved characters only and,
/// ASCII-folded, naming no credential.
fn parameters(text: &str) -> bool {
    escaped(text, query_byte)
        && text.split(['&', ';']).all(|pair| {
            let key = decoded(pair.split('=').next().unwrap_or_default());
            if !key.iter().all(|b| unreserved(*b)) {
                return false;
            }
            let key = String::from_utf8_lossy(&key).to_ascii_lowercase();
            !CREDENTIAL_PARAMETERS.contains(&key.as_str())
                && !DSN_PASSWORD_KEYS.contains(&key.as_str())
                && !credential_named(&key)
        })
}

/// The URL grammar (module docs).
fn url(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once("://") else {
        return false;
    };
    let Some(scheme) = URL_SCHEMES.iter().find(|s| s.eq_ignore_ascii_case(scheme)) else {
        return false;
    };
    let (rest, fragment) = match rest.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let (host, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    let host_ok = if host.is_empty() {
        *scheme == "file"
    } else {
        authority(host)
    };
    host_ok
        && escaped(path, path_byte)
        && query.is_none_or(parameters)
        && fragment.is_none_or(parameters)
}

/// The libpq keyword grammar (module docs): libpq's own tokenisation, with
/// an allowlist of keys and values, and a space required between pairs.
fn dsn(text: &str) -> bool {
    let bytes = text.as_bytes();
    if !bytes.iter().all(|b| (b' '..=b'~').contains(b)) {
        return false;
    }
    let spaces = |mut i: usize| {
        while bytes.get(i) == Some(&b' ') {
            i += 1;
        }
        i
    };
    let mut seen: Vec<&str> = Vec::with_capacity(DSN_KEYS.len());
    let mut value = Vec::new();
    let mut i = spaces(0);
    if i == bytes.len() {
        return false;
    }
    while i < bytes.len() {
        let start = i;
        while bytes.get(i).is_some_and(|b| *b != b'=' && *b != b' ') {
            i += 1;
        }
        let key = &text[start..i];
        i = spaces(i);
        if bytes.get(i) != Some(&b'=') || !DSN_KEYS.contains(&key) || seen.contains(&key) {
            return false;
        }
        seen.push(key);
        i = spaces(i + 1);
        value.clear();
        let quoted = bytes.get(i) == Some(&b'\'');
        if quoted {
            i += 1;
        }
        loop {
            match bytes.get(i) {
                None if quoted => return false,
                None => break,
                Some(b'\'') if quoted => {
                    i += 1;
                    break;
                }
                Some(b' ') if !quoted => break,
                Some(b'\\') => match bytes.get(i + 1) {
                    Some(b) => {
                        value.push(*b);
                        i += 2;
                    }
                    None => return false,
                },
                Some(b) => {
                    value.push(*b);
                    i += 1;
                }
            }
        }
        if (!quoted && value.is_empty()) || !value.iter().all(|b| *b == b' ' || reference_byte(*b))
        {
            return false;
        }
        match bytes.get(i) {
            None => {}
            Some(b' ') => i = spaces(i),
            Some(_) => return false,
        }
    }
    true
}

/// The plain-reference grammar (module docs).
fn reference(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(reference_byte) && !text.contains("//")
}

/// Admit a locator string only if it matches one of the module's three
/// grammars: a URL when it holds `://` or starts with `//`, else a libpq
/// keyword DSN when it holds `=`, else a plain reference. Anything else,
/// and anything that fails its grammar, is refused.
pub fn check_text(text: &str) -> Result<(), Refusal> {
    let admitted = if text.starts_with("//") || text.contains("://") {
        url(text)
    } else if text.contains('=') {
        dsn(text)
    } else {
        reference(text)
    };
    if admitted {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
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

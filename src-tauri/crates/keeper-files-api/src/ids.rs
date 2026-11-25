//! Ids and the keys built from them (decisions 1, 2, 5, 8).

/// The content id of the empty file: keeper's `EMPTY_OID`.
pub const EMPTY_CONTENT_ID: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Whether `text` is a ULID: 26 Crockford base-32 characters, upper case,
/// the first at most `7`.
#[must_use]
pub fn is_ulid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 26
        && matches!(bytes[0], b'0'..=b'7')
        && bytes.iter().all(|b| {
            matches!(b, b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z')
        })
}

/// Whether `text` is a content id: `sha256:` and 64 lowercase hex digits,
/// the SHA-256 of the whole content (decision 2).
#[must_use]
pub fn is_content_id(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// A drive's scope value: `<domain>/<drive>`.
#[must_use]
pub fn drive_key(domain: &str, drive: &str) -> String {
    format!("{domain}/{drive}")
}

/// An event's page key: `<domain>/<yyyy-mm>` of `recorded_at_ms` in UTC, or
/// `<domain>/import` for a `files.import` event (decision 8).
#[must_use]
pub fn page_key(domain: &str, recorded_at_ms: i64, import: bool) -> String {
    if import {
        return format!("{domain}/import");
    }
    let (year, month, ..) = keeper_paths::civil_from_unix_ms(recorded_at_ms);
    format!("{domain}/{year:04}-{month:02}")
}

/// The `internal` locator text of an entry (decision 1): a graph `Resource`
/// names a file by it, and it survives every rename and move.
#[must_use]
pub fn locator(domain: &str, entry_id: &str) -> String {
    format!("files:{domain}/{entry_id}")
}

/// The next decimal-string revision or sequence after `current`.
#[must_use]
pub fn next_decimal(current: &str) -> String {
    current
        .parse::<u64>()
        .map_or_else(|_| "1".to_owned(), |n| (n + 1).to_string())
}

/// The id `files_facts` (reserved, decision 18; 122.6 creates the table)
/// gives one content's facts from one extractor version:
/// `<content_id>/<extractor>/<extractor_version>`. Keyed by content, never
/// by entry, so two paths holding one content share one row.
#[must_use]
pub fn facts_id(content_id: &str, extractor: &str, extractor_version: &str) -> String {
    format!("{content_id}/{extractor}/{extractor_version}")
}

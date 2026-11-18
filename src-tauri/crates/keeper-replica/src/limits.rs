/// The contract's bounds, `syncular/limits.json`, compiled into keeper.
const LIMITS: &str = include_str!("../../../../syncular/limits.json");

/// The most bytes one attachment may have: the contract's
/// `attachmentMaxBytes`, which the server's validator reads too, so the
/// device and the server cannot disagree. A malformed `limits.json` fails
/// the build.
pub const ATTACHMENT_MAX_BYTES: u64 = contract_number(LIMITS, b"\"attachmentMaxBytes\"");

/// The whole number `key` (with its quotes) names in the JSON `text`.
const fn contract_number(text: &str, key: &[u8]) -> u64 {
    let text = text.as_bytes();
    let mut at = 0;
    while at + key.len() <= text.len() {
        let mut matched = 0;
        while matched < key.len() && text[at + matched] == key[matched] {
            matched += 1;
        }
        if matched == key.len() {
            let mut i = at + matched;
            while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\r' | b'\n' | b':') {
                i += 1;
            }
            let mut value: u64 = 0;
            let mut digits = 0;
            while i < text.len() && text[i].is_ascii_digit() {
                value = value * 10 + (text[i] - b'0') as u64;
                i += 1;
                digits += 1;
            }
            assert!(digits > 0, "syncular/limits.json: the key holds no number");
            return value;
        }
        at += 1;
    }
    panic!("syncular/limits.json does not name the key")
}

/// The contract's bounds, `syncular/limits.json`, compiled into keeper.
const LIMITS: &str = include_str!("../../../../syncular/limits.json");

/// The most bytes one attachment may have: the contract's
/// `attachmentMaxBytes`, which the server's validator reads too, so the
/// device and the server cannot disagree. A malformed `limits.json` fails
/// the build.
pub const ATTACHMENT_MAX_BYTES: u64 = contract_number(LIMITS, b"\"attachmentMaxBytes\"");

/// The largest file that goes up in one presigned `PUT`; above it, in parts
/// (`filePartThresholdBytes`, story 115.3; 115.1's M7 chose 64 MiB).
pub const FILE_PART_THRESHOLD_BYTES: u64 = contract_number(LIMITS, b"\"filePartThresholdBytes\"");

/// The size of every part but the last (`filePartBytes`), unless a file
/// needs more than [`MAX_PARTS`] of them: then [`part_bytes`] grows it.
pub const FILE_PART_BYTES: u64 = contract_number(LIMITS, b"\"filePartBytes\"");

/// The most parts one upload may have (the store's limit).
pub const MAX_PARTS: u64 = 10_000;

/// The part size of a file of `size` bytes: `0` for one `PUT`; else
/// `max(FILE_PART_BYTES, ⌈size / MAX_PARTS⌉)` rounded up to a MiB.
pub const fn part_bytes(size: u64) -> u64 {
    if size <= FILE_PART_THRESHOLD_BYTES {
        return 0;
    }
    const MIB: u64 = 1 << 20;
    let least = size.div_ceil(MAX_PARTS).div_ceil(MIB) * MIB;
    if least > FILE_PART_BYTES {
        least
    } else {
        FILE_PART_BYTES
    }
}

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

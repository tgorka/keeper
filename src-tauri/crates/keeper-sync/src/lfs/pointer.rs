//! The git-LFS pointer file format (Story 25.1, AD-46).
//!
//! Normative source: `git-lfs/docs/spec.md` at `main @ d72db1e5`
//! (<https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md>).
//!
//! A pointer is the tiny text blob that git actually stores in place of a large
//! file. Getting its encoding *exactly* right is not pedantry — the spec makes
//! the encoding **unique** (exactly one valid byte sequence per pointer), which
//! means the blob hash is a function of the pointer's semantic content. Emit a
//! semantically-identical but differently-spelled pointer and git sees a
//! modified file. On a folder syncer that runs on every filesystem event, that
//! is a phantom modification on every single sync, forever.
//!
//! That hazard is why [`Pointer`] carries a `canonical` flag: it records
//! whether the bytes we read were *already* the unique encoding, so a filter
//! that parses and regenerates knows when it may re-emit and when it must pass
//! the original bytes through untouched.

/// The current pointer spec version URL.
///
/// Compared by **simple string equality**: the spec explicitly forbids URL
/// normalization, so a trailing slash or an upper-case host is a *different*
/// version, not the same one spelled differently.
pub const SPEC_V1: &str = "https://git-lfs.github.com/spec/v1";

/// Version URLs accepted on read but never written.
///
/// `http://git-media.io/v/2` is the alpha-era name and
/// `https://hawser.github.com/spec/v1` the pre-release one; repositories
/// created before the 2015 rename still carry them. Accepting them costs two
/// `memcmp`s; refusing them would make those blobs permanently unreadable.
/// Anything parsed from a legacy URL is reported non-canonical, because
/// re-encoding it to [`SPEC_V1`] necessarily changes the git blob hash.
const LEGACY_SPEC_URLS: [&str; 2] = [
    "http://git-media.io/v/2",
    "https://hawser.github.com/spec/v1",
];

/// Hard ceiling from the spec: a pointer's total encoding is **< 1024 bytes**,
/// extension lines included. This is what lets a filter decide "pointer or
/// content?" after a single bounded read.
pub const MAX_POINTER_BYTES: usize = 1024;

/// SHA-256 of zero bytes — the oid of the empty pointer.
///
/// Spelled as a constant rather than hashed at runtime so the empty-file path
/// allocates and computes nothing; `empty_oid_constant_is_sha256_of_no_bytes`
/// keeps it honest.
pub const EMPTY_OID: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// `{key} {value}` prefix of the first line, used by [`is_pointer_candidate`].
const VERSION_FIELD_PREFIX: &str = "version ";

/// Only `sha256` is a defined hash method.
const OID_PREFIX: &str = "sha256:";

/// A parsed git-LFS pointer.
///
/// `oid` is the **bare** 64-character lower-case hex digest, without the
/// `sha256:` prefix, because that is the form the batch API and the local
/// content-addressed store both want. The prefix is re-attached only by
/// [`Pointer::render`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    pub oid: String,
    pub size: u64,
    /// Unknown `{key} {value}` lines, in the order they will be rendered.
    ///
    /// The spec requires that tools which parse and regenerate a pointer
    /// **preserve keys they do not understand** — a future extension (a second
    /// digest, a chunk manifest) must survive a round trip through us.
    extensions: Vec<(String, String)>,
    /// Whether the parsed bytes were already the unique canonical encoding.
    ///
    /// Definitional, not heuristic: this is true exactly when [`Pointer::render`]
    /// reproduces the input byte for byte. A pointer built by
    /// [`Pointer::new`] is canonical by construction.
    canonical: bool,
}

impl Pointer {
    /// A fresh pointer for content we just hashed ourselves.
    ///
    /// `oid` must be 64 lower-case hex characters; it comes from our own
    /// `Sha256` in every call site, so this stays infallible rather than
    /// forcing a `Result` on code that cannot produce anything else.
    pub fn new(oid: impl Into<String>, size: u64) -> Self {
        Self {
            oid: oid.into(),
            size,
            extensions: Vec::new(),
            canonical: true,
        }
    }

    /// Parse a blob, tolerating every deviation we can still make sense of.
    ///
    /// Returns `None` when the bytes are not a pointer at all — in which case
    /// the caller keeps them verbatim as ordinary file content.
    ///
    /// Deviations that are *understood but flagged* (`is_canonical() == false`):
    /// a legacy version URL, unsorted keys, a non-minimal `size` spelling. A
    /// caller that re-emits such a pointer changes its blob hash, so it must
    /// write the original bytes instead. See [`Pointer::parse_strict`] when
    /// only the exact encoding will do.
    pub fn parse(bytes: &[u8]) -> Option<Pointer> {
        // An empty blob **is** the empty pointer: the spec's one exception is
        // that empty files pass through LFS unchanged. Callers must read
        // `size == 0` as "there is no object to transfer".
        if bytes.is_empty() {
            return Some(Self::new(EMPTY_OID, 0));
        }
        if bytes.len() >= MAX_POINTER_BYTES {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        // Values may contain neither CR nor LF. A CR anywhere therefore means
        // either not-a-pointer or a CRLF-mangled one; both must be left alone,
        // since "fixing" the line endings would rewrite the user's blob.
        if text.contains('\r') {
            return None;
        }
        // The trailing LF is part of the format, not a nicety. Requiring it
        // also rejects trailing junk after the last field.
        let body = text.strip_suffix('\n')?;
        let mut lines = body.split('\n');

        let (key, value) = split_field(lines.next()?)?;
        if key != "version" {
            return None;
        }
        if value != SPEC_V1 && !LEGACY_SPEC_URLS.contains(&value) {
            return None;
        }

        let mut oid: Option<String> = None;
        let mut size: Option<u64> = None;
        let mut extensions: Vec<(String, String)> = Vec::new();
        for line in lines {
            let (key, value) = split_field(line)?;
            match key {
                // A duplicate key makes the pointer ambiguous; there is no
                // right answer, so refuse rather than pick one.
                "version" => return None,
                "oid" => {
                    if oid.replace(parse_oid(value)?).is_some() {
                        return None;
                    }
                }
                "size" => {
                    if size.replace(value.parse::<u64>().ok()?).is_some() {
                        return None;
                    }
                }
                _ => {
                    if extensions.iter().any(|(existing, _)| existing == key) {
                        return None;
                    }
                    extensions.push((key.to_owned(), value.to_owned()));
                }
            }
        }

        let mut pointer = Self {
            oid: oid?,
            size: size?,
            extensions,
            canonical: false,
        };
        // "The encoding is unique." Deriving canonicality by re-rendering makes
        // that the definition rather than a checklist of conditions we might
        // forget to extend when a new field appears.
        pointer.canonical = pointer.render().as_bytes() == bytes;
        Some(pointer)
    }

    /// Parse, accepting **only** the unique canonical encoding.
    ///
    /// Use this where a non-canonical spelling must not be silently adopted;
    /// use [`Pointer::parse`] where the goal is to resolve the object at all
    /// costs and the original bytes will be preserved regardless.
    pub fn parse_strict(bytes: &[u8]) -> Option<Pointer> {
        Self::parse(bytes).filter(Pointer::is_canonical)
    }

    /// Render the unique canonical encoding.
    ///
    /// `version` first, then every remaining key — `oid`, `size` and any
    /// extension — merged into one ASCII-ascending sort. Extensions are *not*
    /// a separate trailing block: `ext-0-foo` sorts before `oid`, exactly as in
    /// the spec's own worked example.
    pub fn render(&self) -> String {
        // The empty pointer encodes to nothing at all, matching upstream's
        // `Pointer.Encoded()`. An empty file must stay an empty file.
        if self.size == 0 {
            return String::new();
        }

        let oid_value = format!("{OID_PREFIX}{}", self.oid);
        let size_value = self.size.to_string();
        let mut fields: Vec<(&str, &str)> = Vec::with_capacity(2 + self.extensions.len());
        fields.push(("oid", &oid_value));
        fields.push(("size", &size_value));
        for (key, value) in &self.extensions {
            fields.push((key, value));
        }
        // `str`'s ordering is byte-wise, which for the permitted key alphabet
        // `[a-z0-9.-]` is exactly the required ASCII-ascending order.
        fields.sort_unstable_by(|a, b| a.0.cmp(b.0));

        let mut out = String::with_capacity(MAX_POINTER_BYTES);
        out.push_str(VERSION_FIELD_PREFIX);
        out.push_str(SPEC_V1);
        out.push('\n');
        for (key, value) in fields {
            out.push_str(key);
            out.push(' ');
            out.push_str(value);
            out.push('\n');
        }
        out
    }

    /// Whether the bytes this pointer was parsed from were already canonical.
    ///
    /// `false` means **do not re-encode**: write the bytes you read.
    pub fn is_canonical(&self) -> bool {
        self.canonical
    }

    /// The empty pointer, which stands for an empty file and has no object to
    /// transfer.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Keys this build does not understand, preserved for round-tripping.
    pub fn extensions(&self) -> &[(String, String)] {
        &self.extensions
    }
}

/// Cheap "could this blob be a pointer?" probe over a file's first bytes.
///
/// Exists so the engine does not run a full parse over every blob in a
/// 100 000-file profile (NFR-23). False positives are harmless — [`Pointer::parse`]
/// rejects them — but a false negative would silently treat a pointer as
/// content, so this accepts every version URL the parser does.
pub fn is_pointer_candidate(head: &[u8]) -> bool {
    // The longest accepted first line is `version ` + 34 characters + LF = 43,
    // so 100 bytes is generous; capping the slice keeps this a bounded memcmp
    // no matter how large the blob is.
    const PROBE_BYTES: usize = 100;
    let probe = &head[..head.len().min(PROBE_BYTES)];
    if !probe.starts_with(VERSION_FIELD_PREFIX.as_bytes()) {
        return false;
    }
    let tail = &probe[VERSION_FIELD_PREFIX.len()..];
    std::iter::once(SPEC_V1).chain(LEGACY_SPEC_URLS).any(|url| {
        tail.len() > url.len() && tail.starts_with(url.as_bytes()) && tail[url.len()] == b'\n'
    })
}

/// Split one `{key} {value}` line.
///
/// Split at the **first** space only: a key may never contain one, but a value
/// is the entire rest of the line.
fn split_field(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(' ')?;
    if value.is_empty() || !is_valid_key(key) {
        return None;
    }
    Some((key, value))
}

/// Keys are drawn from `[a-z0-9.-]` and are never empty.
fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// `sha256:` + 64 lower-case hex characters, and nothing else.
///
/// Upper-case hex is rejected rather than folded: it is a different byte
/// sequence for the same object, and silently normalizing it would rewrite the
/// user's blob.
fn parse_oid(value: &str) -> Option<String> {
    let hex = value.strip_prefix(OID_PREFIX)?;
    if hex.len() != 64 {
        return None;
    }
    if !hex
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return None;
    }
    Some(hex.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const OID_A: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    fn canonical_text(oid: &str, size: u64) -> String {
        format!("version {SPEC_V1}\noid sha256:{oid}\nsize {size}\n")
    }

    #[test]
    fn empty_oid_constant_is_sha256_of_no_bytes() {
        assert_eq!(EMPTY_OID, hex::encode(Sha256::digest([])));
    }

    #[test]
    fn canonical_pointer_round_trips_byte_for_byte() {
        let text = canonical_text(OID_A, 12_345);
        let parsed = Pointer::parse(text.as_bytes()).expect("canonical pointer parses");

        assert_eq!(parsed.oid, OID_A);
        assert_eq!(parsed.size, 12_345);
        assert!(parsed.is_canonical());
        assert_eq!(parsed.render(), text);
        assert_eq!(Pointer::parse_strict(text.as_bytes()), Some(parsed));
    }

    #[test]
    fn new_pointer_renders_the_canonical_encoding() {
        let pointer = Pointer::new(OID_A, 7);
        assert!(pointer.is_canonical());
        assert_eq!(pointer.render(), canonical_text(OID_A, 7));
    }

    #[test]
    fn all_three_version_urls_read_and_re_encode_to_the_current_one() {
        let current = canonical_text(OID_A, 99);
        for legacy_url in LEGACY_SPEC_URLS {
            let text = format!("version {legacy_url}\noid sha256:{OID_A}\nsize 99\n");
            let parsed = Pointer::parse(text.as_bytes()).expect("legacy version URL is readable");

            assert_eq!(parsed.oid, OID_A);
            assert_eq!(parsed.size, 99);
            // Re-encoding would change the blob hash, so the caller must be
            // told these bytes are not canonical.
            assert!(!parsed.is_canonical(), "{legacy_url} must not be canonical");
            assert_eq!(parsed.render(), current);
            assert_eq!(Pointer::parse_strict(text.as_bytes()), None);
        }
    }

    #[test]
    fn an_unrecognized_version_url_is_not_a_pointer() {
        // Simple string equality, no normalization: a trailing slash is a
        // different version.
        let text = format!("version {SPEC_V1}/\noid sha256:{OID_A}\nsize 1\n");
        assert_eq!(Pointer::parse(text.as_bytes()), None);
    }

    #[test]
    fn the_1024_byte_ceiling_is_exclusive() {
        let padded = |pad: usize| {
            format!(
                "version {SPEC_V1}\next-0-pad {}\noid sha256:{OID_A}\nsize 5\n",
                "p".repeat(pad)
            )
        };

        let largest_legal = padded(886);
        assert_eq!(largest_legal.len(), MAX_POINTER_BYTES - 1);
        let parsed = Pointer::parse(largest_legal.as_bytes()).expect("1023 bytes is legal");
        assert!(parsed.is_canonical());

        let too_large = padded(887);
        assert_eq!(too_large.len(), MAX_POINTER_BYTES);
        assert_eq!(Pointer::parse(too_large.as_bytes()), None);
    }

    #[test]
    fn wrong_key_order_is_readable_but_never_canonical() {
        // `size` before `oid` violates the ASCII-ascending rule.
        let text = format!("version {SPEC_V1}\nsize 12345\noid sha256:{OID_A}\n");

        // Strict parse refuses it outright.
        assert_eq!(Pointer::parse_strict(text.as_bytes()), None);

        // The tolerant parse still resolves the object, but flags the encoding
        // so the caller preserves the original bytes rather than re-emitting.
        let parsed = Pointer::parse(text.as_bytes()).expect("still resolvable");
        assert_eq!(parsed.oid, OID_A);
        assert_eq!(parsed.size, 12_345);
        assert!(!parsed.is_canonical());
        assert_ne!(parsed.render(), text);
        assert_eq!(parsed.render(), canonical_text(OID_A, 12_345));
    }

    #[test]
    fn non_minimal_size_spelling_is_readable_but_not_canonical() {
        let text = format!("version {SPEC_V1}\noid sha256:{OID_A}\nsize 007\n");
        let parsed = Pointer::parse(text.as_bytes()).expect("still resolvable");
        assert_eq!(parsed.size, 7);
        assert!(!parsed.is_canonical());
    }

    #[test]
    fn ordinary_content_is_not_a_pointer() {
        for blob in [
            &b"hello world\n"[..],
            &b"\x00\x01\x02\x03binary"[..],
            // Looks like a pointer but has no fields.
            format!("version {SPEC_V1}\n").as_bytes(),
            // Missing the mandatory trailing LF.
            format!("version {SPEC_V1}\noid sha256:{OID_A}\nsize 1").as_bytes(),
            // CRLF line endings.
            format!("version {SPEC_V1}\r\noid sha256:{OID_A}\r\nsize 1\r\n").as_bytes(),
            // Two spaces: the key would be `oid` and the value ` sha256:…`,
            // which is not a valid oid.
            format!("version {SPEC_V1}\noid  sha256:{OID_A}\nsize 1\n").as_bytes(),
        ] {
            assert_eq!(Pointer::parse(blob), None, "unexpectedly parsed {blob:?}");
        }
    }

    #[test]
    fn empty_input_is_the_empty_pointer_and_encodes_back_to_empty() {
        let parsed = Pointer::parse(b"").expect("an empty file is its own pointer");
        assert_eq!(parsed.size, 0);
        assert!(parsed.is_empty());
        assert_eq!(parsed.oid, EMPTY_OID);
        assert_eq!(parsed.render(), "");
    }

    #[test]
    fn oid_must_be_64_lowercase_sha256_hex() {
        let bad_oids = [
            // Upper-case hex is a different byte sequence, not a synonym.
            "4D7A214614AB2935C943F9E0FF69D22EADBB8F32B1258DAAA5E2CA24D17E2393",
            // 63 characters.
            "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e239",
            // 65 characters.
            "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e23933",
            // Non-hex.
            "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
        ];
        for oid in bad_oids {
            let text = format!("version {SPEC_V1}\noid sha256:{oid}\nsize 1\n");
            assert_eq!(Pointer::parse(text.as_bytes()), None, "accepted oid {oid}");
        }

        // A hash method other than sha256 is undefined by the spec.
        let sha1 = format!("version {SPEC_V1}\noid sha1:{}\nsize 1\n", "a".repeat(40));
        assert_eq!(Pointer::parse(sha1.as_bytes()), None);
    }

    #[test]
    fn unknown_extension_keys_survive_a_parse_render_cycle() {
        // `ext-0-…` sorts before `oid`, so a conforming renderer interleaves
        // extensions with the required keys instead of appending them.
        let text = format!(
            "version {SPEC_V1}\next-0-sha256 aaaa\next-1-note hello world\noid sha256:{OID_A}\nsize 3\n"
        );
        let parsed = Pointer::parse(text.as_bytes()).expect("extensions are legal");

        assert_eq!(
            parsed.extensions(),
            [
                ("ext-0-sha256".to_owned(), "aaaa".to_owned()),
                ("ext-1-note".to_owned(), "hello world".to_owned()),
            ]
        );
        assert!(parsed.is_canonical());
        assert_eq!(parsed.render(), text);
    }

    #[test]
    fn duplicate_keys_are_rejected_rather_than_resolved() {
        let text = format!(
            "version {SPEC_V1}\noid sha256:{OID_A}\noid sha256:{}\nsize 1\n",
            "b".repeat(64)
        );
        assert_eq!(Pointer::parse(text.as_bytes()), None);
    }

    #[test]
    fn candidate_probe_accepts_every_version_the_parser_does() {
        for url in std::iter::once(SPEC_V1).chain(LEGACY_SPEC_URLS) {
            let text = format!("version {url}\noid sha256:{OID_A}\nsize 1\n");
            assert!(is_pointer_candidate(text.as_bytes()), "missed {url}");
            // The probe only ever looks at the head, so a truncated read is
            // enough — that is the whole point of it.
            assert!(is_pointer_candidate(&text.as_bytes()[..60]), "missed {url}");
        }
    }

    #[test]
    fn candidate_probe_rejects_content_and_never_reads_past_its_window() {
        assert!(!is_pointer_candidate(b""));
        assert!(!is_pointer_candidate(b"hello world"));
        assert!(!is_pointer_candidate(b"version "));
        // Right URL, but pushed beyond the probe window by padding.
        let late = format!("{}version {SPEC_V1}\n", " ".repeat(100));
        assert!(!is_pointer_candidate(late.as_bytes()));
        // Right URL, but the first line continues instead of ending.
        let extended = format!("version {SPEC_V1}x\n");
        assert!(!is_pointer_candidate(extended.as_bytes()));
    }
}

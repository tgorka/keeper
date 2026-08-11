//! Names the filesystem holds that keeper cannot spell (Story 47.2).
//!
//! # The bug this module exists to make impossible
//!
//! POSIX filenames are byte strings. Every rule about them is "no `/`, no
//! `NUL`"; nothing requires them to be UTF-8, and a repository that has been
//! through a `zip` written on a Windows box, a `tar` from a CP-1250 era, or a
//! restore from a filesystem with a different locale will contain names that
//! are not. git carries such a name without complaint — it stores path *bytes*
//! — so the file syncs, pushes and clones perfectly well.
//!
//! Rust's `String` is UTF-8 by construction, so every surface that renders a
//! name has to decide what to do at that boundary, and the convenient answer is
//! [`std::path::Path::to_string_lossy`], which substitutes `U+FFFD` for each
//! byte it cannot decode. That is the right answer *for display* and a
//! catastrophic one for anything else, because the substitution is **not
//! injective and not reversible**:
//!
//! * Two files, `a\xFF.txt` and `a\xFE.txt`, render to the same string. A
//!   listing keyed on that string has one row for two files.
//! * A file *legitimately* named `a\u{FFFD}.txt` — three ordinary UTF-8 bytes,
//!   a name a user can type — renders to that same string too. Joining the
//!   rendering back onto the root does not fail; it **succeeds, at the wrong
//!   file**. That was measured, not theorised: before this module existed,
//!   browsing a folder holding both files and clicking the mangled row resolved
//!   to the ordinary one, and [`crate::files_write`] shares the same join, so a
//!   delete confirmed against one row removed the other.
//!
//! So a lossy rendering is fine to *show* and must never be used to *reach*.
//! That distinction is not something a comment can enforce — the old code had
//! the comment and the bug — so it is enforced by the type system here.
//!
//! # How the type enforces it
//!
//! [`UnspellableName`] holds two renderings and hands out neither as anything a
//! path can be built from:
//!
//! * [`UnspellableName::for_display`] returns [`ForDisplay`], which implements
//!   [`std::fmt::Display`] and **nothing else**. `Path::new(…)`,
//!   `PathBuf::push(…)` and `Path::join(…)` all require `AsRef<Path>`, and
//!   `ForDisplay` is not, so `root.join(name.for_display())` does not compile.
//!   Getting a `String` out of it takes an explicit `.to_string()`, which is a
//!   decision a reviewer can see rather than a coercion nobody notices.
//! * [`UnspellableName::escaped`] is the *lossless* rendering — byte-exact,
//!   pure ASCII, `\xNN` for anything outside printable ASCII. It is what makes
//!   the report actionable: `doc-\xffepuap.txt` tells a human exactly which
//!   bytes to look for, where `doc-<?>epuap.txt` tells them only that something
//!   is wrong. It is also, deliberately, not the name — pasting it into a shell
//!   as a literal reaches nothing, and `printf` is required to turn it back
//!   into one. A rendering that needs a decoding step is a rendering nobody
//!   joins onto a root by accident.
//!
//! There is no constructor from a valid name. [`UnspellableName::of`] and its
//! siblings return `None` when the name decodes cleanly, so **the existence of
//! one of these values is itself the finding** — a surface cannot hold one and
//! have nothing to report, and a caller cannot forget to check a boolean it was
//! never given.
//!
//! # What this module deliberately does not do
//!
//! It does not make such a file syncable, because it already is. git stages,
//! commits and pushes the raw bytes; `gix::path::try_into_bstr` is infallible
//! on unix and the tree entry that comes back out holds the original name. The
//! owner who reported this guessed the file "probably would not make it to the
//! remote anyway" and that guess was wrong: the only thing that could not
//! handle the name was keeper's own rendering of it.

use std::ffi::OsStr;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A name on disk that is not valid UTF-8, in the two renderings a human needs
/// and in no rendering a path can be built from.
///
/// Construct with [`Self::of`], [`Self::of_path`] or [`Self::of_bytes`], each of
/// which answers `None` for a name that decodes cleanly. See the module docs
/// for why this is a type rather than a `bool` beside a `String`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnspellableName {
    /// `U+FFFD`-substituted rendering: what a person reads in a row.
    ///
    /// Lossy, non-injective, and **not** the name. Public because it is what
    /// crosses to the frontend, where it lands in a text node; every consumer
    /// inside this crate goes through [`Self::for_display`] instead.
    pub display: String,
    /// Byte-exact ASCII rendering, `\xNN` for every byte outside printable
    /// ASCII and `\\` for a literal backslash.
    ///
    /// Lossless: two names that share a `display` never share this. It is what
    /// a report quotes so the reader can go and find the file.
    pub escaped: String,
}

impl UnspellableName {
    /// The name of one directory entry, or `None` if it is ordinary text.
    pub fn of(name: &OsStr) -> Option<Self> {
        if name.to_str().is_some() {
            return None;
        }
        Some(Self {
            display: name.to_string_lossy().into_owned(),
            escaped: escape(name.as_encoded_bytes()),
        })
    }

    /// A whole path — relative or absolute — or `None` if it is ordinary text.
    ///
    /// Separators are left as the platform wrote them. This renders paths that
    /// have already been refused, so a shape a user can compare against `ls`
    /// output beats one normalized for matching.
    pub fn of_path(path: &Path) -> Option<Self> {
        Self::of(path.as_os_str())
    }

    /// A path git handed us as raw bytes — an index entry or a status walk.
    ///
    /// git stores path bytes and never decodes them, so this is the shape the
    /// only complete inventory of a repository arrives in.
    pub fn of_bytes(bytes: &[u8]) -> Option<Self> {
        if std::str::from_utf8(bytes).is_ok() {
            return None;
        }
        Some(Self {
            display: String::from_utf8_lossy(bytes).into_owned(),
            escaped: escape(bytes),
        })
    }

    /// The lossy rendering, in a wrapper that is not `AsRef<Path>`.
    ///
    /// This is the whole point of the type: `format!("{}", n.for_display())`
    /// compiles and `root.join(n.for_display())` does not.
    pub fn for_display(&self) -> ForDisplay<'_> {
        ForDisplay(&self.display)
    }
}

/// A borrowed lossy name that can be printed and cannot be walked.
///
/// Implements [`std::fmt::Display`] and deliberately no other trait — in
/// particular not `AsRef<Path>`, `AsRef<OsStr>`, `AsRef<str>` or `Deref`, each
/// of which would silently re-open the hole this type closes. See the module
/// docs.
#[derive(Debug, Clone, Copy)]
pub struct ForDisplay<'a>(&'a str);

impl std::fmt::Display for ForDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The character a lossy decode leaves behind.
///
/// Named because two places test for it and a bare `'\u{FFFD}'` in a condition
/// reads like a typo.
pub const REPLACEMENT: char = '\u{FFFD}';

/// Whether a string carries the mark of a lossy decode.
///
/// Used by [`crate::browse::plain_segments`] to refuse a subpath that may be a
/// rendering rather than a name. It cannot distinguish a rendering from a file
/// genuinely named with `U+FFFD` — nothing can, which is exactly the ambiguity
/// that makes the join unsafe — so it refuses both. See
/// [`crate::browse::BrowseRefusal::Unspellable`] for why that trade is the
/// right way round.
pub fn is_lossy_rendering(text: &str) -> bool {
    text.contains(REPLACEMENT)
}

/// Byte-exact ASCII rendering: printable ASCII verbatim, `\xNN` for the rest.
///
/// `\\` for a literal backslash, so the escaping is unambiguous and two
/// distinct names can never produce one string.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7E => out.push(byte as char),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out
}

/// Create a file whose name is not valid UTF-8, or `None` when the filesystem
/// refuses the bytes.
///
/// **macOS will not let you make one.** APFS validates filename bytes and
/// returns `EILSEQ` ("Illegal byte sequence") for anything that is not valid
/// UTF-8, so every fixture in this crate that needs such a file on disk is
/// unbuildable on the machine keeper ships from. That is a fact about the
/// filesystem, not about keeper, and it does not make the defect theoretical:
/// a Mac cannot CREATE such a name but can very easily RECEIVE one, because
/// git carries the raw bytes and a Linux peer can commit it. The sync that
/// delivers it is exactly the path this crate is.
///
/// So the tests that need the file skip with a reason where the filesystem
/// refuses, and the rules those tests are about — which are pure — are
/// asserted separately and run everywhere.
#[cfg(all(test, unix))]
pub(crate) fn create_unspellable(
    dir: &std::path::Path,
    bytes: &[u8],
) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let path = dir.join(std::ffi::OsString::from_vec(bytes.to_vec()));
    match std::fs::write(&path, "x") {
        Ok(()) => Some(path),
        // EILSEQ. Any other error is a real failure and must not be swallowed
        // into a silent skip — a test that skips on a permissions bug reports
        // success for a run that proved nothing.
        Err(error) if error.raw_os_error() == Some(92) => None,
        Err(error) => panic!("could not create the fixture: {error}"),
    }
}

/// What a test prints when it steps aside, so a green run that skipped is
/// distinguishable from a green run that checked.
#[cfg(all(test, unix))]
pub(crate) const UNSPELLABLE_UNAVAILABLE: &str =
    "skipped: this filesystem refuses a non-UTF-8 filename (macOS/APFS EILSEQ)";

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn osname(bytes: &[u8]) -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes.to_vec())
    }

    #[test]
    fn an_ordinary_name_is_not_a_finding() {
        // The type's existence IS the report, so a clean name must not produce
        // one — otherwise every listing would report every file.
        assert_eq!(UnspellableName::of(OsStr::new("notes.md")), None);
        assert_eq!(UnspellableName::of_bytes(b"notes.md"), None);
        // Non-ASCII is not the same as non-UTF-8. A Polish filename is text.
        assert_eq!(UnspellableName::of(OsStr::new("zaświadczenie.pdf")), None);
        assert_eq!(
            UnspellableName::of_bytes("zaświadczenie.pdf".as_bytes()),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_name_is_reported_in_both_renderings() {
        let name = osname(b"doc-\xffepuap.txt");
        let found = UnspellableName::of(&name).expect("not valid UTF-8");
        assert_eq!(found.display, "doc-\u{FFFD}epuap.txt");
        assert_eq!(found.escaped, "doc-\\xffepuap.txt");
    }

    #[cfg(unix)]
    #[test]
    fn the_escaped_rendering_separates_names_the_lossy_one_merges() {
        // This is the property that makes `escaped` worth carrying: two files
        // a user must be able to tell apart collapse into one `display`.
        let one = UnspellableName::of(&osname(b"a\xff.txt")).expect("invalid");
        let two = UnspellableName::of(&osname(b"a\xfe.txt")).expect("invalid");
        assert_eq!(one.display, two.display, "the lossy renderings do collide");
        assert_ne!(one.escaped, two.escaped);
        assert_eq!(one.escaped, "a\\xff.txt");
        assert_eq!(two.escaped, "a\\xfe.txt");
    }

    #[test]
    fn escaping_a_backslash_keeps_the_rendering_injective() {
        // Without `\\`, a file literally named `a\xff.txt` (five ASCII
        // characters) and one holding the byte 0xFF would render identically,
        // and `escaped` would be as ambiguous as `display`.
        assert_eq!(escape(b"a\\xff.txt"), "a\\\\xff.txt");
        assert_ne!(escape(b"a\\xff.txt"), escape(b"a\xff.txt"));
    }

    #[test]
    fn control_bytes_are_escaped_so_a_report_cannot_be_forged() {
        // A newline in a filename would otherwise let one entry write what
        // looks like two lines of a report.
        assert_eq!(escape(b"a\nb\tc"), "a\\x0ab\\x09c");
        assert!(!escape(b"a\nb").contains('\n'));
    }

    #[cfg(unix)]
    #[test]
    fn the_display_rendering_cannot_be_joined_onto_a_root() {
        // The compile-time guarantee, asserted the only way a runtime test can:
        // `ForDisplay` is not `AsRef<Path>`, so the sentence below is the ONLY
        // way to get a path-shaped value out, and it is explicit.
        let name = UnspellableName::of(&osname(b"a\xff.txt")).expect("invalid");
        let shown = format!("{}", name.for_display());
        assert_eq!(shown, "a\u{FFFD}.txt");
        // And the rendering, if someone does force it back to a string, is
        // recognisable as a rendering — which is what lets `plain_segments`
        // refuse it.
        assert!(is_lossy_rendering(&shown));
    }

    #[test]
    fn a_source_scan_proves_no_path_conversion_was_ever_added() {
        // `ForDisplay` closes the hole only for as long as nobody adds a
        // conversion to it. A future `impl AsRef<Path> for ForDisplay`, or a
        // `Deref<Target = str>`, would re-open it silently and every other test
        // in this file would still pass. This is the one that would not.
        //
        // **An allowlist, not a blocklist, and that distinction was found by a
        // mutation.** The first version of this test listed the spellings to
        // forbid — `impl AsRef<Path> for ForDisplay` and four others — and a
        // mutation adding `impl AsRef<std::path::Path> for ForDisplay<'_>`
        // walked straight past it. There is no finite list of ways to write a
        // trait path, so the rule is inverted: exactly one trait may be
        // implemented for each of these types, and every other `impl` is a
        // failure whatever it is called.
        let source = include_str!("names.rs");
        let body = source
            .split("#[cfg(test)]")
            .next()
            .expect("the non-test half");

        let allowed = [
            "impl std::fmt::Display for ForDisplay<'_> {",
            "impl UnspellableName {",
        ];
        let offenders: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("impl "))
            .filter(|line| line.contains("ForDisplay") || line.contains("UnspellableName"))
            .filter(|line| !allowed.contains(line))
            .collect();
        assert!(
            offenders.is_empty(),
            "these could hand a lossy name back as something path-shaped: {offenders:?}. \
             Only {allowed:?} may exist; anything else needs a new argument, not a new arm here."
        );
        // And the allowlist is not vacuous — if the impls were renamed away,
        // the loop above would pass over an empty file and prove nothing.
        for expected in allowed {
            assert!(
                body.contains(expected),
                "{expected} is gone; this test is now blind"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_two_constructors_agree_about_one_name() {
        // `of_bytes` reads git's path bytes and `of` reads a dirent; a report
        // that joined the two must not word one file two ways.
        let from_os = UnspellableName::of(&osname(b"doc-\xffepuap.txt")).expect("invalid");
        let from_git = UnspellableName::of_bytes(b"doc-\xffepuap.txt").expect("invalid");
        assert_eq!(from_os, from_git);
    }
}

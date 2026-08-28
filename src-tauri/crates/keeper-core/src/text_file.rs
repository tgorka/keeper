//! Opening a file as editable text (Story 45.6, FR-179, AD-88).
//!
//! # Why the decision is here and not in the shell
//!
//! Three questions have to be answered before a text editor can be put on the
//! screen — *is this text at all*, *is it small enough to edit*, and *how big is
//! it* — and every one of them is a decision rather than a rendering. AD-55 and
//! AD-56 put decisions in `keeper-core` and leave the Tauri shell a call site,
//! and this module is the reason that rule is worth having: the shell crate does
//! not build on a Linux developer machine, so a threshold that lived in
//! `sync_ipc.rs` would be a threshold nobody could test until macOS. Everything
//! below is proved over a temp directory on any machine.
//!
//! # The size the editor refuses at, and why it is this number
//!
//! [`TEXT_EDIT_MAX_BYTES`] is 1 000 000 bytes — one megabyte, decimal, which is
//! also how [`crate::size::format_file_size`] renders it, so the banner reads
//! "1.0 MB" and the limit reads "1.0 MB" and a person can compare them without
//! doing arithmetic. That alignment is the reason the number is decimal and not
//! `1 << 20`: a limit stated in units the surface never uses is a limit users
//! misread.
//!
//! The magnitude is chosen for what CodeMirror actually does with a document.
//! One megabyte of source is on the order of twenty thousand lines; the editor's
//! viewport rendering is fine with that, but the pieces keeper layers on top are
//! not free of the whole document — a Lezer parse, and for markdown the
//! decoration layer in `live-preview.ts`, walk far more than the visible lines.
//! Past a megabyte the first keystroke after opening starts to be perceptible,
//! and an editor that stutters on every keypress is worse than an editor that
//! says plainly it will not edit this file. It is also comfortably above every
//! file a person actually hand-edits: a large `Cargo.lock` is ~200 kB, this
//! repository's largest source file is under 300 kB, and a note is a few kB.
//! A file over the limit is nearly always machine-written, and the honest answer
//! for one is "look, do not edit".
//!
//! # Over the limit means truncated, and that is why read-only is not advisory
//!
//! An oversize file is not shipped whole. Sending a 2 GB log across the IPC
//! boundary to then refuse to edit it would freeze the pane exactly as thoroughly
//! as editing it would, which is the failure the story names. So at most
//! [`TEXT_EDIT_MAX_BYTES`] bytes are read, the prefix is what the surface shows,
//! and [`TextFileVm::oversize`] says so. The read-only flag is therefore not a
//! courtesy: the buffer is *not the file*, and saving it would delete everything
//! past the first megabyte.
//!
//! # Binary is a refusal, never a lossy conversion
//!
//! `String::from_utf8_lossy` would put U+FFFD where a byte it could not read
//! was, and a save would then write those replacement characters over the user's
//! data. Every path here that cannot produce exact text produces [`None`] and a
//! sentence instead. W1Registry's table already keeps a `.exe` away from this
//! viewer; this is the second lock, on the side of the boundary that holds the
//! bytes, because the first lock is a table someone can add a row to.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::size::format_file_size;

/// The largest file this surface will open for editing, in bytes.
///
/// Decimal, so it renders as `1.0 MB` through [`format_file_size`] and matches
/// the number a size column shows for the same file. See the module doc for why
/// the magnitude is one megabyte.
///
/// It is also the read cap: nothing above this is loaded, so the constant bounds
/// what crosses IPC as well as what may be written back.
pub const TEXT_EDIT_MAX_BYTES: u64 = 1_000_000;

/// A file as an editor can show it (Story 45.6, FR-179).
///
/// **`text` is `None` exactly when the bytes are not text**, and in that case
/// `binary` is `true` and `detail` says so. A surface that renders `text ?? ""`
/// would show an empty editable pane over a `.png` and offer to save it; the
/// two fields are separate so the empty file and the unreadable file cannot be
/// confused.
///
/// **`size_bytes` is the file's real size, never the prefix's.** When `oversize`
/// is true the string in `text` is shorter than `size_bytes` says, deliberately:
/// the banner has to name what the file *is*, not what was loaded of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextFileVm {
    /// The file's contents, exactly — or its first [`TEXT_EDIT_MAX_BYTES`]
    /// bytes when `oversize`. `None` when the bytes are not valid UTF-8 text.
    ///
    /// Byte-for-byte: no line-ending normalisation, no BOM stripping, no
    /// trailing newline added or removed. A file opened and saved untouched is
    /// the same file, which is the only thing that makes an editor over synced
    /// content safe to use at all.
    pub text: Option<String>,
    /// The size of the file on disk, in bytes.
    ///
    /// `number` on the wire rather than ts-rs's default `bigint` for `u64`, the
    /// same reading [`crate::vm::CopyEntryVm::bytes`] takes: Tauri delivers JSON,
    /// a JSON number loses precision above 2^53, and 2^53 bytes is nine
    /// petabytes. A `bigint` here would make every arithmetic comparison in the
    /// frontend a type error for a limit no file will reach.
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// `size_bytes` in the units a person reads, formatted once in Rust by
    /// [`format_file_size`] so this banner and the Files pane's size column can
    /// never disagree about the same file.
    pub size_label: String,
    /// The file is larger than [`TEXT_EDIT_MAX_BYTES`]: `text` holds a prefix,
    /// and the surface must open read-only. Saving a prefix would truncate the
    /// file.
    pub oversize: bool,
    /// The bytes are not text and no editor should be offered.
    pub binary: bool,
    /// The one sentence explaining a non-ordinary outcome, composed here so the
    /// Files pane, a note embed and quick capture all say the same thing.
    /// `None` when the file opened normally and there is nothing to explain.
    pub detail: Option<String>,
}

/// Read one file as editable text.
///
/// Stats first and reads a bounded prefix, so a huge file costs a `metadata`
/// call and at most [`TEXT_EDIT_MAX_BYTES`] bytes rather than its own size in
/// resident memory.
///
/// Errors are the caller's to word: an `Err` here means the path could not be
/// stat'ed or opened at all, which is a different situation from "this is not
/// text" and deserves the OS's own reason rather than one composed here.
pub fn open_text_file(path: &Path) -> io::Result<TextFileVm> {
    let size_bytes = std::fs::metadata(path)?.len();
    let oversize = size_bytes > TEXT_EDIT_MAX_BYTES;
    let bytes = read_prefix(path, TEXT_EDIT_MAX_BYTES)?;
    Ok(decide(size_bytes, oversize, &bytes))
}

/// Read at most `limit` bytes from the head of a file.
///
/// `Read::take` rather than `fs::read` followed by a slice: the point is not to
/// hold the whole file for even an instant. A log that grew to a gigabyte since
/// the listing was taken must cost a megabyte here, not a gigabyte.
fn read_prefix(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut buffer = Vec::new();
    std::fs::File::open(path)?
        .take(limit)
        .read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// The whole decision, over bytes already in hand.
///
/// Split out from [`open_text_file`] so every branch — including the ones only
/// a hostile file produces, like a multi-byte character cut in half by the
/// prefix — is reachable from a test without arranging a two-gigabyte fixture.
fn decide(size_bytes: u64, oversize: bool, bytes: &[u8]) -> TextFileVm {
    let size_label = format_file_size(size_bytes);
    let binary_vm = |detail: String| TextFileVm {
        text: None,
        size_bytes,
        size_label: size_label.clone(),
        oversize,
        binary: true,
        detail: Some(detail),
    };

    // A NUL byte is the one signal that survives every encoding guess: no text
    // file a person edits contains one, and every executable, archive and image
    // does within its first few hundred bytes. Checked before UTF-8 validity
    // because UTF-8 happily encodes NUL, so a `.bin` of mostly-ASCII bytes with
    // NULs in it would otherwise decode "successfully" into a document whose
    // save would be indistinguishable from corruption.
    if bytes.contains(&0) {
        return binary_vm(format!(
            "This file is not text — it contains bytes no editor can show. It is {size_label}."
        ));
    }

    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        // The one recoverable failure, and only when truncating: the prefix cut
        // a multi-byte character in half. `error_len() == None` is precisely
        // "the input ended mid-character", so the valid head is real text and
        // the tail is an artefact of the cut rather than of the file. Dropping
        // it is safe because an oversize file is never written back.
        Err(error) if oversize && error.error_len().is_none() => {
            // Valid by construction: `valid_up_to` is the length of the longest
            // valid UTF-8 prefix.
            String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned()
        }
        Err(_) => {
            return binary_vm(format!(
                "This file is not valid UTF-8 text, so keeper cannot show it without changing \
                 it. It is {size_label}."
            ));
        }
    };

    let detail = oversize.then(|| {
        format!(
            "This file is {size_label}, larger than the {} keeper will edit, so it is open \
             read-only and only the first {} is shown.",
            format_file_size(TEXT_EDIT_MAX_BYTES),
            format_file_size(TEXT_EDIT_MAX_BYTES),
        )
    });

    TextFileVm {
        text: Some(text),
        size_bytes,
        size_label,
        oversize,
        binary: false,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory of this process's own.
    ///
    /// Hand-rolled rather than a `tempfile` dev-dependency because that is the
    /// pattern the rest of this crate already uses (`notes::default_spaces`),
    /// and one more dependency for four `std::fs::write` calls is not a trade
    /// worth making. Left behind on the disk deliberately: a failing fixture is
    /// evidence, and the name says which run produced it.
    fn temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-text-file-{}-{label}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write fixture");
        path
    }

    /// The whole point of the story: what the editor shows is what is on disk.
    #[test]
    fn an_ordinary_file_opens_byte_for_byte() {
        let dir = temp_dir("an_ordinary_file_opens_byte_for_byte");
        let body = "# Title\r\n\r\nA line with a tab\there.\nNo trailing newline";
        let path = write(&dir, "notes.md", body.as_bytes());

        let vm = open_text_file(&path).expect("open");

        assert_eq!(vm.text.as_deref(), Some(body));
        assert!(!vm.oversize);
        assert!(!vm.binary);
        assert_eq!(vm.detail, None);
        assert_eq!(vm.size_bytes, body.len() as u64);
    }

    /// A trailing newline and its absence are different files, and an editor
    /// that cannot tell them apart rewrites every file it touches.
    #[test]
    fn a_trailing_newline_is_carried_and_its_absence_is_too() {
        let dir = temp_dir("a_trailing_newline_is_carried_and_its_absence_is_too");
        let with = write(&dir, "with.txt", b"one\ntwo\n");
        let without = write(&dir, "without.txt", b"one\ntwo");

        assert_eq!(
            open_text_file(&with).expect("open").text.as_deref(),
            Some("one\ntwo\n")
        );
        assert_eq!(
            open_text_file(&without).expect("open").text.as_deref(),
            Some("one\ntwo")
        );
    }

    /// A UTF-8 BOM is part of the file. Stripping it here would delete it on the
    /// next save, silently, from a file some other tool requires it in.
    #[test]
    fn a_byte_order_mark_survives() {
        let dir = temp_dir("a_byte_order_mark_survives");
        let path = write(&dir, "bom.csv", b"\xef\xbb\xbfa,b\n1,2\n");

        let vm = open_text_file(&path).expect("open");

        assert_eq!(vm.text.as_deref(), Some("\u{feff}a,b\n1,2\n"));
    }

    /// The empty file is text, editable, and not a refusal.
    #[test]
    fn an_empty_file_is_editable_text_and_not_binary() {
        let dir = temp_dir("an_empty_file_is_editable_text_and_not_binary");
        let path = write(&dir, "empty.txt", b"");

        let vm = open_text_file(&path).expect("open");

        assert_eq!(vm.text.as_deref(), Some(""));
        assert!(!vm.binary);
        assert_eq!(vm.size_label, "0 bytes");
    }

    /// Exactly at the limit is editable. The boundary is `>`, not `>=`, and a
    /// test on only one side of it cannot tell which.
    #[test]
    fn a_file_exactly_at_the_limit_is_editable() {
        let dir = temp_dir("a_file_exactly_at_the_limit_is_editable");
        let body = vec![b'a'; TEXT_EDIT_MAX_BYTES as usize];
        let path = write(&dir, "exact.txt", &body);

        let vm = open_text_file(&path).expect("open");

        assert!(!vm.oversize, "1 000 000 bytes must still open for editing");
        assert_eq!(vm.detail, None);
        assert_eq!(vm.text.map(|t| t.len()), Some(TEXT_EDIT_MAX_BYTES as usize));
    }

    /// One byte over, and the file is read-only, truncated, and says its own
    /// size — the real one, not the prefix's.
    #[test]
    fn one_byte_over_the_limit_is_read_only_and_names_the_files_own_size() {
        let dir = temp_dir("one_byte_over_the_limit_is_read_only_and_names_the_files_own_size");
        let body = vec![b'a'; TEXT_EDIT_MAX_BYTES as usize + 1];
        let path = write(&dir, "over.txt", &body);

        let vm = open_text_file(&path).expect("open");

        assert!(vm.oversize);
        assert_eq!(vm.size_bytes, TEXT_EDIT_MAX_BYTES + 1);
        assert_eq!(vm.size_label, "1.0 MB");
        assert_eq!(
            vm.text.map(|t| t.len()),
            Some(TEXT_EDIT_MAX_BYTES as usize),
            "an oversize file is truncated to the cap, so the pane cannot be handed a \
             gigabyte to render"
        );
        let detail = vm.detail.expect("an oversize file must explain itself");
        assert!(
            detail.contains("1.0 MB"),
            "the sentence must name a size: {detail}"
        );
        assert!(
            detail.contains("read-only"),
            "the sentence must say why it cannot be edited: {detail}"
        );
    }

    /// A much larger file states its own size and still costs only the cap.
    #[test]
    fn a_much_larger_file_states_its_real_size() {
        let dir = temp_dir("a_much_larger_file_states_its_real_size");
        let body = vec![b'x'; 4_000_000];
        let path = write(&dir, "huge.log", &body);

        let vm = open_text_file(&path).expect("open");

        assert_eq!(vm.size_label, "4.0 MB");
        assert!(vm.detail.expect("explained").contains("4.0 MB"));
        assert_eq!(vm.text.map(|t| t.len()), Some(TEXT_EDIT_MAX_BYTES as usize));
    }

    /// A NUL byte means no editor, and no empty pane pretending to be one.
    #[test]
    fn a_file_with_a_nul_byte_is_refused_rather_than_shown_empty() {
        let dir = temp_dir("a_file_with_a_nul_byte_is_refused_rather_than_shown_empty");
        let path = write(&dir, "app.bin", b"MZ\x90\x00\x03\x00\x00\x00");

        let vm = open_text_file(&path).expect("open");

        assert!(vm.binary);
        assert_eq!(vm.text, None);
        assert!(!vm.oversize);
        assert!(vm.detail.expect("explained").contains("8 bytes"));
    }

    /// The case the NUL check exists for, and the only one that reaches it.
    ///
    /// A NUL byte **is** valid UTF-8, so a file of ASCII with NULs sprinkled
    /// through it decodes without complaint into a document full of `\0`. The
    /// executable header above is caught by the UTF-8 arm instead — its `\x90`
    /// is a stray continuation byte — so a suite with only that fixture leaves
    /// the NUL branch dead and would stay green if it were deleted. This is the
    /// shape of a UTF-16 file read as bytes, and of most `.bin` payloads:
    /// perfectly decodable, and a save would write `\0` characters over a
    /// structure that meant something.
    #[test]
    fn valid_utf8_containing_a_nul_is_still_refused() {
        let dir = temp_dir("valid_utf8_containing_a_nul_is_still_refused");
        let path = write(&dir, "table.bin", b"name\x00\x00value\x00\x00");
        assert!(
            std::str::from_utf8(b"name\x00\x00value\x00\x00").is_ok(),
            "the fixture must be valid UTF-8, or the UTF-8 arm catches it first"
        );

        let vm = open_text_file(&path).expect("open");

        assert!(vm.binary);
        assert_eq!(vm.text, None);
        assert!(vm.detail.expect("explained").contains("no editor can show"));
    }

    /// Invalid UTF-8 with no NUL — a Latin-1 file, say — is still refused, not
    /// lossily converted into replacement characters a save would write back.
    #[test]
    fn invalid_utf8_without_a_nul_is_refused_rather_than_mangled() {
        let dir = temp_dir("invalid_utf8_without_a_nul_is_refused_rather_than_mangled");
        let path = write(&dir, "latin1.txt", b"caf\xe9 au lait");

        let vm = open_text_file(&path).expect("open");

        assert!(vm.binary);
        assert_eq!(vm.text, None);
        assert!(vm.detail.expect("explained").contains("UTF-8"));
    }

    /// The one case truncation creates by itself: the cap lands in the middle of
    /// a multi-byte character. That is an artefact of the cut, not a property of
    /// the file, so it must not be reported as binary.
    #[test]
    fn a_multibyte_character_split_by_the_cap_is_not_a_binary_file() {
        // Three-byte characters do not divide the cap, so the last one is cut.
        let mut body = "☃"
            .repeat(TEXT_EDIT_MAX_BYTES as usize / 3 + 8)
            .into_bytes();
        let size_bytes = body.len() as u64;
        body.truncate(TEXT_EDIT_MAX_BYTES as usize);
        assert!(
            std::str::from_utf8(&body).is_err(),
            "fixture must actually cut a character in half"
        );

        let vm = decide(size_bytes, true, &body);

        assert!(
            !vm.binary,
            "a cut character is the cap's doing, not the file's"
        );
        let text = vm.text.expect("the valid head is real text");
        assert!(text.ends_with('☃'));
        assert!(text.len() < TEXT_EDIT_MAX_BYTES as usize);
    }

    /// The same half-character in a file that is *not* truncated is a genuinely
    /// broken file, and the recovery must not extend to it.
    #[test]
    fn a_truncated_character_in_a_complete_file_is_still_binary() {
        let vm = decide(4, false, b"abc\xe2");

        assert!(vm.binary);
        assert_eq!(vm.text, None);
    }

    /// A path that is not there is an `Err` and not a "binary file": the caller
    /// has a different sentence for it, and conflating the two would tell a user
    /// their missing file was an executable.
    #[test]
    fn a_missing_path_is_an_error_and_not_a_refusal() {
        let dir = temp_dir("a_missing_path_is_an_error_and_not_a_refusal");
        let error = open_text_file(&dir.join("gone.txt")).expect_err("missing");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}

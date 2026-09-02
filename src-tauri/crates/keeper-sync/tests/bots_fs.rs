//! The filesystem half of the drive-as-tools surface, over a REAL tree
//! (Story 61.11, FR-389, NFR-49).
//!
//! # Why every fixture here is on a real disk
//!
//! The epic names this story's impure shell twice: *a real file that is a
//! pointer and not content*, and *a real `rename` under the sync watcher*. A
//! mock filesystem proves neither. So this file builds an actual temp tree and
//! puts in it the five things that break a naive tool surface:
//!
//! * a **symlink pointing outside the root**, which no lexical test can see;
//! * a **git-LFS pointer file**, which is valid UTF-8 and reads as a 130-byte
//!   text file to anything that does not look;
//! * a **binary file**, which `from_utf8_lossy` would hand over as mojibake;
//! * a **2 MB text file**, which is the byte cap's whole reason to exist;
//! * a **write**, whose landing is checked for the temp file the atomic writer
//!   uses and for the exact bytes it left behind.
//!
//! The limits used here are deliberately tiny and stated per test. The shipped
//! numbers live in `keeper_core::bots::tools`, which is where the model is told
//! about them; a copy of those numbers here would be a second authority.

use std::path::Path;

use keeper_sync::bots_fs::{self, FileRead, FsRefusal, Limits, LineRange};
use keeper_sync::browse::BrowseRefusal;
use keeper_sync::files_write::{WriteRoute, WriteScope};
use keeper_sync::lfs::pointer::Pointer;

/// Small enough that every bound is reachable in a test that runs in
/// milliseconds, and every one of them different from the others so a test
/// cannot pass by hitting the wrong cap.
fn limits() -> Limits {
    Limits {
        max_read_bytes: 64,
        max_entries: 3,
        max_matches: 2,
        max_paths: 2,
        max_walk_entries: 64,
        max_write_bytes: 128,
        max_match_line_bytes: 16,
    }
}

/// The shipped-shape limits, for the tests that are about content rather than
/// about a bound.
fn roomy() -> Limits {
    Limits {
        max_read_bytes: 64 * 1024,
        max_entries: 1000,
        max_matches: 500,
        max_paths: 500,
        max_walk_entries: 20_000,
        max_write_bytes: 1_000_000,
        max_match_line_bytes: 400,
    }
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, body).expect("write fixture");
}

// ---------------------------------------------------------------------------
// Containment — the refusals that must reach the caller as refusals
// ---------------------------------------------------------------------------

/// A symlink inside the folder pointing OUT of it is the escape no lexical
/// test can see, and the one AD-59's canonicalizing half exists for. The
/// refusal must arrive as `EscapesAfterResolution` carrying the subpath — not
/// as a panic, and not as an empty result the model would read as "the file is
/// empty".
#[test]
fn a_symlink_out_of_the_root_is_refused_after_resolution() {
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("secrets.env"), "TOKEN=hunter2\n").expect("secret");

    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "notes/ok.md", "hello\n");
    std::os::unix::fs::symlink(outside.path().join("secrets.env"), root.join("escape.env"))
        .expect("symlink");

    let refusal = bots_fs::read(root, "escape.env", LineRange::default(), &roomy())
        .expect_err("a symlink out of the root must be refused");
    assert_eq!(
        refusal,
        FsRefusal::Contained(BrowseRefusal::EscapesAfterResolution {
            subpath: "escape.env".to_owned(),
        })
    );
    // And the sentence the model is handed is the containment rule's own.
    assert!(
        refusal.to_string().contains("resolves outside this folder"),
        "got {refusal}"
    );

    // The same file reached by its real name outside the root is refused
    // lexically, before the disk is consulted at all.
    let lexical = bots_fs::read(root, "../secrets.env", LineRange::default(), &roomy())
        .expect_err("`..` must be refused");
    assert_eq!(
        lexical,
        FsRefusal::Contained(BrowseRefusal::Escapes {
            subpath: "../secrets.env".to_owned(),
        })
    );
}

/// Every other shape of escape, refused by the one rule rather than by a list
/// this module maintains. Absolute, `.`, a doubled separator, and a backslash —
/// which is an ordinary filename character on Linux and a separator on Windows,
/// so it must be refused on the machine where it is harmless.
#[test]
fn absolute_dot_and_empty_segments_never_reach_the_disk() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "a/b.md", "x\n");

    for subpath in ["/etc/passwd", "a/./b.md", "a//b.md", "a/../a/b.md", ""] {
        let result = bots_fs::stat(root, subpath);
        assert!(
            matches!(
                result,
                Err(FsRefusal::Contained(_)) | Ok(bots_fs::Stat { is_dir: true, .. })
            ),
            "{subpath:?} produced {result:?}"
        );
    }
    // `""` is the profile root and is the one empty spelling that is legal.
    let root_stat = bots_fs::stat(root, "").expect("the profile root is listable");
    assert!(root_stat.is_dir);
}

/// A walk never follows a symlink, so an escaping link cannot smuggle content
/// into a grep result that the direct read refuses.
#[test]
fn a_walk_does_not_follow_a_symlink() {
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("secrets.env"), "TOKEN=hunter2\n").expect("secret");

    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "notes/plain.md", "nothing to see\n");
    std::os::unix::fs::symlink(outside.path(), root.join("notes/away")).expect("symlink");

    let found = bots_fs::grep(root, "", "hunter2", true, &roomy()).expect("grep");
    assert!(
        found.matches.is_empty(),
        "a symlinked directory must not be walked, got {:?}",
        found.matches
    );
}

// ---------------------------------------------------------------------------
// Bounds, and telling the model about them
// ---------------------------------------------------------------------------

/// The byte cap on a read, over a real 2 MB file. The disclosure is the point:
/// `truncated_at` says where it stopped and `of_bytes` says how big the file
/// is, because a model handed the first 64 bytes with no note answers
/// confidently about a file it has seen 0.003% of (NFR-49).
#[test]
fn a_read_is_bounded_and_says_where_it_stopped() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    let big = "abcdefghij\n".repeat(190_000); // just over 2 MB
    assert!(big.len() > 2_000_000);
    write(root, "logs/big.txt", &big);

    let read = bots_fs::read(root, "logs/big.txt", LineRange::default(), &limits())
        .expect("a big file reads as a bounded prefix");
    let FileRead::Text {
        body,
        bytes_read,
        of_bytes,
        truncated_at,
        ..
    } = read
    else {
        panic!("expected text");
    };
    assert_eq!(bytes_read, 64, "the cap is the cap");
    assert_eq!(body.len(), 64);
    assert_eq!(of_bytes, big.len() as u64);
    assert_eq!(
        truncated_at,
        Some(64),
        "a truncation the model is not told about is the defect"
    );

    // A file under the cap is returned whole, with nothing to disclose.
    write(root, "logs/small.txt", "short\n");
    let small =
        bots_fs::read(root, "logs/small.txt", LineRange::default(), &limits()).expect("small file");
    assert_eq!(
        small,
        FileRead::Text {
            body: "short\n".to_owned(),
            bytes_read: 6,
            of_bytes: 6,
            truncated_at: None,
            lines_shown: None,
        }
    );
}

/// A line range is answered as a line range, and the span is named so the model
/// can ask for the next one without guessing.
#[test]
fn a_line_range_names_the_lines_it_returned() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "doc.md", "one\ntwo\nthree\nfour\nfive\n");

    let read = bots_fs::read(
        root,
        "doc.md",
        LineRange {
            start_line: Some(2),
            line_count: Some(2),
        },
        &roomy(),
    )
    .expect("range read");
    let FileRead::Text {
        body,
        lines_shown,
        truncated_at,
        of_bytes,
        ..
    } = read
    else {
        panic!("expected text");
    };
    assert_eq!(body, "two\nthree\n");
    assert_eq!(lines_shown, Some((2, 3)));
    assert!(
        truncated_at.is_some(),
        "lines 4 and 5 were left out and the model must know"
    );
    assert_eq!(of_bytes, 24);
}

/// A listing stops at the cap and reports both numbers, so "three files" and
/// "three of nine files" are never the same answer.
#[test]
fn a_listing_is_capped_and_reports_the_real_count() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    for index in 0..9 {
        write(root, &format!("many/f{index}.md"), "x\n");
    }

    let listing = bots_fs::list(root, "many", &limits()).expect("list");
    assert_eq!(listing.entries.len(), 3);
    assert_eq!(listing.truncated_at, Some(3));
    assert_eq!(listing.of_entries, 9);
    assert_eq!(listing.entries[0].subpath, "many/f0.md");
}

/// A search stops at the cap, and a very long matching line is clipped rather
/// than returned whole.
#[test]
fn a_search_is_capped_in_matches_and_in_line_length() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "src/a.txt", "needle 1\nneedle 2\nneedle 3\n");

    let found = bots_fs::grep(root, "src", "needle", true, &limits()).expect("grep");
    assert_eq!(found.matches.len(), 2);
    assert_eq!(found.truncated_at, Some(2));
    assert_eq!(found.files_searched, 1);

    write(
        root,
        "src/long.txt",
        &format!("{}needle\n", "z".repeat(400)),
    );
    let long = bots_fs::grep(root, "src", "zzz", true, &roomy()).expect("grep");
    let hit = long.matches.first().expect("one match");
    assert_eq!(hit.text.len(), 400);
    assert!(hit.line_truncated, "a clipped line must say so");
}

/// A search that is case-insensitive finds what a case-sensitive one does not,
/// and an empty needle is refused rather than matching everything.
#[test]
fn search_case_folding_and_the_empty_needle() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "a.md", "Hello World\n");

    assert!(bots_fs::grep(root, "", "hello", true, &roomy())
        .expect("grep")
        .matches
        .is_empty());
    assert_eq!(
        bots_fs::grep(root, "", "hello", false, &roomy())
            .expect("grep")
            .matches
            .len(),
        1
    );
    assert_eq!(
        bots_fs::grep(root, "", "", false, &roomy()).expect_err("empty needle"),
        FsRefusal::EmptyNeedle
    );
}

/// A glob is bounded too, does not cross a directory boundary on `*`, and
/// refuses a pattern that is not one.
#[test]
fn a_glob_is_bounded_and_does_not_cross_a_separator() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "top.md", "x\n");
    write(root, "deep/one.md", "x\n");
    write(root, "deep/two.md", "x\n");
    write(root, "deep/more/three.md", "x\n");

    let shallow = bots_fs::glob(root, "", "*.md", &roomy()).expect("glob");
    assert_eq!(shallow.paths, vec!["top.md".to_owned()]);

    let deep = bots_fs::glob(root, "", "**/*.md", &roomy()).expect("glob");
    assert_eq!(deep.paths.len(), 4);

    let capped = bots_fs::glob(root, "", "**/*.md", &limits()).expect("glob");
    assert_eq!(capped.paths.len(), 2);
    assert_eq!(capped.truncated_at, Some(2));

    assert!(matches!(
        bots_fs::glob(root, "", "a[", &roomy()),
        Err(FsRefusal::BadPattern { .. })
    ));
}

// ---------------------------------------------------------------------------
// Binary and pointer files — the two things that are not what they look like
// ---------------------------------------------------------------------------

/// A binary file is refused as binary. `from_utf8_lossy` would hand the model
/// `U+FFFD` where a byte it could not read was, which is the position
/// `keeper_core::text_file` already takes on the same question.
#[test]
fn a_binary_file_is_refused_rather_than_rendered() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    std::fs::write(
        root.join("logo.png"),
        [0x89, b'P', b'N', b'G', 0x0d, 0x00, 0xff],
    )
    .expect("binary fixture");

    assert_eq!(
        bots_fs::read(root, "logo.png", LineRange::default(), &roomy())
            .expect_err("binary must be refused"),
        FsRefusal::Binary {
            subpath: "logo.png".to_owned(),
        }
    );

    // And a binary file in a searched subtree is skipped and counted, never
    // silently absent.
    let found = bots_fs::grep(root, "", "PNG", true, &roomy()).expect("grep");
    assert!(found.matches.is_empty());
    assert_eq!(found.files_skipped, 1);
}

/// A read that lands on a real LFS pointer file returns the POINTER'S truth —
/// the content's oid and its real size — and materializes nothing.
///
/// The worktree bytes of an unmaterialized path *are* the committed pointer
/// (FR-331), which is exactly why a naive read succeeds and lies: 130 bytes of
/// clean UTF-8 that a model will happily summarise as the whole file.
#[test]
fn a_read_of_a_pointer_returns_the_pointers_truth_and_hydrates_nothing() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    let oid = "b".repeat(64);
    let pointer = Pointer::new(oid.clone(), 4_294_967_296);
    let rendered = pointer.render();
    write(root, "media/talk.mov", &rendered);

    let before = std::fs::read(root.join("media/talk.mov")).expect("read back");

    let read = bots_fs::read(root, "media/talk.mov", LineRange::default(), &roomy())
        .expect("a pointer reads");
    assert_eq!(
        read,
        FileRead::Pointer {
            oid,
            of_bytes: 4_294_967_296,
        },
        "the pointer's size is the file's size, not the 130 bytes on disk"
    );

    // Nothing was hydrated, nothing was rewritten: a tool call is a `grep -r`
    // and epic 56 forbids it being the thing that materializes a subtree.
    let after = std::fs::read(root.join("media/talk.mov")).expect("read back");
    assert_eq!(before, after);
    assert_eq!(after.len(), rendered.len());

    // A listing of the same path reports the pointer's number too (FR-336).
    let listing = bots_fs::list(root, "media", &roomy()).expect("list");
    let entry = listing.entries.first().expect("one entry");
    assert!(entry.is_virtual);
    assert_eq!(entry.bytes, Some(4_294_967_296));

    // As does `stat`, with the oid so a caller can say what would materialize.
    let stat = bots_fs::stat(root, "media/talk.mov").expect("stat");
    assert!(stat.is_virtual);
    assert_eq!(stat.bytes, 4_294_967_296);
    assert_eq!(stat.lfs_oid.as_deref().map(str::len), Some(64));
}

// ---------------------------------------------------------------------------
// Writing — routed, atomic, bounded
// ---------------------------------------------------------------------------

/// A write goes through `WriteScope::route`, lands atomically, and leaves no
/// `.keeper.<ulid>.tmp` behind. The temp name is the one tier 0 already
/// excludes, which is what keeps a `kill -9` between write and rename from
/// putting a torn file in front of the watcher.
#[test]
fn a_write_is_routed_atomic_and_leaves_no_temp_behind() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "docs/notes.md", "before\n");

    let scope = WriteScope::new("Drive", None);
    let route: WriteRoute<()> =
        bots_fs::plan_write(&scope, None, root, "docs/notes.md").expect("route");
    let WriteRoute::Unmanaged(target) = route else {
        panic!("a profile with no vault routes to the second writer");
    };
    assert_eq!(target.profile_relative(), "docs/notes.md");

    let wrote = bots_fs::write_unmanaged(&target, "after\n", &limits()).expect("write");
    assert_eq!(wrote.bytes, 6);
    assert_eq!(
        std::fs::read_to_string(root.join("docs/notes.md")).expect("read back"),
        "after\n"
    );

    let leftovers: Vec<String> = std::fs::read_dir(root.join("docs"))
        .expect("read_dir")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.starts_with(".keeper."))
        .collect();
    assert!(leftovers.is_empty(), "left {leftovers:?}");
}

/// The write cap is a real refusal, and it happens before a byte is written.
#[test]
fn an_oversize_write_is_refused_and_changes_nothing() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "docs/notes.md", "before\n");

    let scope = WriteScope::new("Drive", None);
    let route: WriteRoute<()> =
        bots_fs::plan_write(&scope, None, root, "docs/notes.md").expect("route");
    let WriteRoute::Unmanaged(target) = route else {
        panic!("unmanaged");
    };

    let refusal =
        bots_fs::write_unmanaged(&target, &"x".repeat(129), &limits()).expect_err("over the cap");
    assert_eq!(
        refusal,
        FsRefusal::TooLarge {
            subpath: "docs/notes.md".to_owned(),
            bytes: 129,
            cap: 128,
        }
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/notes.md")).expect("read back"),
        "before\n",
        "a refused write must not have happened"
    );
}

/// A write outside the profile root is refused by the router, and the refusal
/// is the router's own sentence.
#[test]
fn a_write_outside_the_root_is_refused_by_the_router() {
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("shell.rc"), "# yours\n").expect("fixture");

    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    std::os::unix::fs::symlink(outside.path().join("shell.rc"), root.join("shell.rc"))
        .expect("symlink");

    let scope = WriteScope::new("Drive", None);
    let refused = bots_fs::plan_write::<()>(&scope, None, root, "shell.rc")
        .expect_err("a symlink out of the root is not writable");
    assert!(
        matches!(refused, FsRefusal::Write(_)),
        "expected the router's refusal, got {refused:?}"
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("shell.rc")).expect("read back"),
        "# yours\n"
    );
}

/// An edit replaces exactly one occurrence or refuses, and it never edits a
/// file it could only read a prefix of.
#[test]
fn an_edit_replaces_exactly_one_occurrence() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "a.md", "alpha\nbeta\ngamma\n");

    assert_eq!(
        bots_fs::edited_text(root, "a.md", "beta", "delta", &roomy()).expect("edit"),
        "alpha\ndelta\ngamma\n"
    );
    assert_eq!(
        bots_fs::edited_text(root, "a.md", "omega", "x", &roomy()).expect_err("no match"),
        FsRefusal::EditNoMatch {
            subpath: "a.md".to_owned(),
        }
    );

    write(root, "b.md", "same\nsame\n");
    assert_eq!(
        bots_fs::edited_text(root, "b.md", "same", "other", &roomy()).expect_err("ambiguous"),
        FsRefusal::EditAmbiguous {
            subpath: "b.md".to_owned(),
            occurrences: 2,
        }
    );

    // A file larger than the read cap cannot be edited: saving a prefix would
    // delete everything past it.
    write(root, "big.md", &"y".repeat(500));
    assert!(matches!(
        bots_fs::edited_text(root, "big.md", "y", "z", &limits()),
        Err(FsRefusal::TooLarge { .. })
    ));
}

/// A directory is not a file and a file is not a directory, at every verb.
#[test]
fn the_two_shapes_are_never_confused() {
    let dir = tempfile::tempdir().expect("root");
    let root = dir.path();
    write(root, "folder/inside.md", "x\n");

    assert_eq!(
        bots_fs::read(root, "folder", LineRange::default(), &roomy()).expect_err("not a file"),
        FsRefusal::NotAFile {
            subpath: "folder".to_owned(),
        }
    );
    assert_eq!(
        bots_fs::list(root, "folder/inside.md", &roomy()).expect_err("not a directory"),
        FsRefusal::NotADirectory {
            subpath: "folder/inside.md".to_owned(),
        }
    );
    assert_eq!(
        bots_fs::stat(root, "folder/nope.md").expect_err("missing"),
        FsRefusal::Missing {
            subpath: "folder/nope.md".to_owned(),
        }
    );
}

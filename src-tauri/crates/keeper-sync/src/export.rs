//! Copying a file — or a note and everything it shows — out to a folder the
//! user picked (Story 45.21, FR-199, AD-65).
//!
//! # Why an export needs Rust at all
//!
//! The epic labels this story "Frontend", and it cannot be one. keeper has
//! `@tauri-apps/plugin-dialog`, which picks a path and returns a string; it has
//! no `plugin-fs`, and nothing in the webview can write a byte outside the
//! app's own storage. Every existing "put this where I say" path in keeper —
//! the archive export, the recordings destination — hands the picked folder to
//! Rust and Rust writes. This is that, for a note and for a file.
//!
//! # Why the decision lives in this crate
//!
//! [`crate::files_write`]'s reason, unchanged: the shell does not build on
//! Linux, so a rule written there is a rule proved on no machine this is
//! developed on. Everything here is strings plus real file operations, so an
//! escape, a collision, an unwritable destination and a half-finished copy are
//! all asserted over a real temp directory on any machine.
//!
//! # The three promises
//!
//! 1. **Bytes out, unchanged.** An export is a copy. Nothing here parses,
//!    re-encodes or rewrites anything, so the exported file is byte-identical
//!    to the one in the vault and a person can diff the two. That is also why a
//!    note's embeds are copied to the *same vault-relative paths* rather than
//!    having their links rewritten — see `keeper_core::notes::export` for the
//!    argument.
//! 2. **keeper never writes over something it did not create.** The destination
//!    is somebody's Desktop, not a folder keeper manages. A name already taken
//!    is a refusal that names it, checked case-insensitively through
//!    [`crate::files_write::collides`] — the same function and the same reason,
//!    because APFS would silently replace `Report` with `report`.
//! 3. **A failed export leaves nothing behind.** If the third of five files
//!    cannot be read, the folder keeper made is removed before the refusal is
//!    returned. Half an export in somebody's Documents folder, with no sign of
//!    which half, is worse than no export.
//!
//! Every source path is re-resolved through [`crate::browse::resolve`], the one
//! containment rule the listing already used, so the webview hands over ids and
//! relative paths and never a location on disk (AD-65).

use std::path::{Path, PathBuf};

use crate::browse::{self, BrowseRefusal};
use crate::files_write::{self, WriteRefusal};

/// Why keeper will not export, or did not finish exporting.
///
/// Each variant carries what its sentence needs and [`std::fmt::Display`]
/// composes it here, in this crate, where it is asserted — the shell renders it
/// verbatim and adds no words, exactly as it does for [`WriteRefusal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportRefusal {
    /// The picked folder is not there any more.
    DestinationMissing { destination: String },
    /// The picked path is a file, so nothing can be put inside it.
    DestinationNotAFolder { destination: String },
    /// The picked folder is inside the folder being exported from.
    DestinationInsideSource { destination: String },
    /// The destination already holds something by that name.
    Taken { name: String },
    /// The subpath is not a plain descendant of the source root.
    Escapes { subpath: String },
    /// The file to export is not on disk.
    Missing { subpath: String },
    /// The entry is a folder, and this exports files.
    IsDirectory { name: String },
    /// The destination could not be read, so a collision could not be ruled
    /// out. Never a cleared check.
    Unreadable { reason: String },
    /// The destination folder could not be created.
    FolderFailed { name: String, reason: String },
    /// One file could not be copied. Everything already written has been
    /// removed by the time this is returned.
    CopyFailed {
        relative_path: String,
        reason: String,
    },
    /// Two items of one export want the same path in the destination.
    ///
    /// Not reachable through the planner, which deduplicates — and a refusal
    /// rather than a `debug_assert` because the alternative is `fs::copy`
    /// overwriting the first file with the second and reporting two successes.
    Collision { relative_path: String },
}

impl std::fmt::Display for ExportRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DestinationMissing { destination } => write!(
                f,
                "{destination} is not there any more, so keeper exported nothing."
            ),
            Self::DestinationNotAFolder { destination } => write!(
                f,
                "{destination} is a file, not a folder, so keeper has nowhere to put the export."
            ),
            Self::DestinationInsideSource { destination } => write!(
                f,
                "{destination} is inside the folder keeper is exporting from. Pick somewhere \
                 else — a copy made in there would sync straight back as a second copy of \
                 everything you just exported."
            ),
            Self::Taken { name } => write!(
                f,
                "\"{name}\" is already in that folder. keeper will not write over something it \
                 did not put there — pick another folder, or move that one out of the way."
            ),
            Self::Escapes { subpath } => write!(
                f,
                "\"{subpath}\" is not a path inside this folder, so keeper will not export it."
            ),
            Self::Missing { subpath } => write!(
                f,
                "{subpath} is not there any more, so keeper exported nothing."
            ),
            Self::IsDirectory { name } => write!(
                f,
                "{name} is a folder. keeper exports files — copy a folder in your file manager, \
                 where you can see everything that would go with it."
            ),
            Self::Unreadable { reason } => write!(
                f,
                "that folder could not be read, so keeper cannot tell whether the name is \
                 already taken: {reason}."
            ),
            Self::FolderFailed { name, reason } => write!(
                f,
                "keeper could not make a folder called \"{name}\" there: {reason}."
            ),
            Self::CopyFailed {
                relative_path,
                reason,
            } => write!(
                f,
                "keeper could not copy {relative_path}: {reason}. Nothing was left behind."
            ),
            Self::Collision { relative_path } => write!(
                f,
                "two of the files in this export are both called {relative_path}, so keeper \
                 stopped rather than write one over the other. Nothing was left behind."
            ),
        }
    }
}

impl From<BrowseRefusal> for ExportRefusal {
    /// Carry a containment refusal across without re-deriving it.
    ///
    /// The verdict is `browse`'s lexical rule, as it is for every other
    /// consumer; only the sentence changes, because the reader is about to lose
    /// an export rather than a listing.
    fn from(refusal: BrowseRefusal) -> Self {
        match refusal {
            BrowseRefusal::Escapes { subpath }
            | BrowseRefusal::EscapesAfterResolution { subpath } => Self::Escapes { subpath },
            BrowseRefusal::Unreadable { reason } => Self::Unreadable { reason },
            // An export names its files by the subpath the caller handed in, so
            // a name that is only a rendering has nothing to export *to*.
            // Same verdict as an escape, and the sentence `BrowseRefusal`
            // already writes is the one the reader needs (Story 47.2).
            BrowseRefusal::Unspellable { subpath } => Self::Escapes { subpath },
        }
    }
}

impl From<WriteRefusal> for ExportRefusal {
    /// [`files_write::collides`] is reused rather than re-implemented — one
    /// case-insensitive collision test in this crate, not two — so its refusal
    /// has to arrive here. It only ever produces `Unreadable`; anything else
    /// would mean a caller passed this the wrong function, and carrying the
    /// words through is more useful than a panic.
    fn from(refusal: WriteRefusal) -> Self {
        match refusal {
            WriteRefusal::Unreadable { reason } => Self::Unreadable { reason },
            other => Self::Unreadable {
                reason: other.to_string(),
            },
        }
    }
}

/// What an export put on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The one thing that now exists in the destination: the copied file, or
    /// the folder a note's export was written into. What Reveal points at.
    pub path: PathBuf,
    /// Every file written, relative to the *destination folder* and in copy
    /// order. Relative to the destination rather than to `path` so a receipt
    /// reads the same for both shapes, and so the frontend joins nothing
    /// (AD-65).
    pub written: Vec<String>,
}

/// The last `/`-separated component of a subpath — the file's own name.
///
/// Public because the shell needs the same answer to word a receipt, and a
/// second `rsplit('/')` at the call site is how two spellings of "the file's
/// name" start disagreeing about a path with a trailing slash.
pub fn file_name_of(subpath: &str) -> &str {
    subpath.rsplit('/').next().unwrap_or(subpath)
}

/// The name a note's export folder takes: the note's file name without its
/// extension.
///
/// `Meeting.md` becomes `Meeting`, so the folder reads as the document rather
/// than as a file with a folder's job. A name that is all extension and no stem
/// — `.hidden` — keeps its whole name, because `""` is not a folder name and a
/// nameless folder in somebody's Documents is worse than a dotted one.
pub fn note_folder_name(note_subpath: &str) -> &str {
    let name = file_name_of(note_subpath);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// Check the picked destination before anything is written to it.
///
/// Three questions in the order that produces the useful sentence: is it there,
/// is it a folder, and is it inside the folder being exported from. The last is
/// not paranoia — the Files pane browses the vault, so "export this into my
/// vault" is one mis-click away, and the copy would be indexed as new notes and
/// synced to every other machine.
fn checked_destination(source_root: &Path, destination: &Path) -> Result<PathBuf, ExportRefusal> {
    let canonical = destination
        .canonicalize()
        .map_err(|_| ExportRefusal::DestinationMissing {
            destination: destination.display().to_string(),
        })?;
    if !canonical.is_dir() {
        return Err(ExportRefusal::DestinationNotAFolder {
            destination: destination.display().to_string(),
        });
    }
    // An un-canonicalisable source root cannot contain anything, so there is
    // nothing to refuse here; the per-file resolve below reports it properly.
    if let Ok(root) = source_root.canonicalize() {
        if canonical.starts_with(&root) {
            return Err(ExportRefusal::DestinationInsideSource {
                destination: destination.display().to_string(),
            });
        }
    }
    Ok(canonical)
}

/// Resolve one source file through the listing's own containment rule.
fn source_file(root: &Path, subpath: &str) -> Result<PathBuf, ExportRefusal> {
    let resolved = browse::resolve(root, subpath)?.ok_or_else(|| ExportRefusal::Missing {
        subpath: subpath.to_owned(),
    })?;
    if resolved.is_dir() {
        return Err(ExportRefusal::IsDirectory {
            name: file_name_of(subpath).to_owned(),
        });
    }
    Ok(resolved)
}

/// Refuse if `name` is already taken in `destination`, case-insensitively.
fn refuse_if_taken(destination: &Path, name: &str) -> Result<(), ExportRefusal> {
    if files_write::collides(destination, name)? {
        return Err(ExportRefusal::Taken {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Copy one file out of a profile to the destination, keeping its name.
///
/// One file, exactly the bytes it has. keeper does not read a PDF's references
/// or a spreadsheet's links, so there is no neighbourhood to reproduce and the
/// export is the file — which is why this is not [`export_note`] with an empty
/// attachment list, and why it does not wrap the file in a folder.
pub fn export_entry(
    root: &Path,
    subpath: &str,
    destination: &Path,
) -> Result<Exported, ExportRefusal> {
    let destination = checked_destination(root, destination)?;
    let source = source_file(root, subpath)?;
    let name = file_name_of(subpath).to_owned();
    refuse_if_taken(&destination, &name)?;

    let target = destination.join(&name);
    std::fs::copy(&source, &target).map_err(|error| {
        // A partial file is worse than none: the next reader cannot tell a
        // truncated copy from a short one.
        let _ = std::fs::remove_file(&target);
        ExportRefusal::CopyFailed {
            relative_path: name.clone(),
            reason: error.to_string(),
        }
    })?;

    Ok(Exported {
        path: target,
        written: vec![name],
    })
}

/// Copy a note and the files it embeds into a new folder in the destination.
///
/// `carried` is `keeper_core::notes::export::plan`'s answer: vault-relative
/// paths, in document order, already deduplicated. It arrives as plain strings
/// because this crate is deliberately `keeper-core`-free (AD-40) — which files
/// a note needs is a notes rule, and copying them is this crate's.
///
/// The export folder is a miniature vault root: the note at its own name, every
/// attachment at the vault-relative path the note spells. That is what makes
/// `![[attachments/photo.png]]` still resolve without a byte of the note
/// changing.
pub fn export_note(
    vault_root: &Path,
    note_subpath: &str,
    carried: &[String],
    destination: &Path,
) -> Result<Exported, ExportRefusal> {
    let destination = checked_destination(vault_root, destination)?;
    let note_source = source_file(vault_root, note_subpath)?;
    let note_name = file_name_of(note_subpath).to_owned();
    let folder_name = note_folder_name(note_subpath).to_owned();
    refuse_if_taken(&destination, &folder_name)?;

    // Every source is resolved BEFORE the folder is made, so a note whose
    // attachment has been moved refuses without leaving an empty folder in
    // somebody's Documents.
    let mut sources: Vec<(String, PathBuf)> = vec![(note_name, note_source)];
    for relative in carried {
        sources.push((relative.clone(), source_file(vault_root, relative)?));
    }

    let folder = destination.join(&folder_name);
    std::fs::create_dir(&folder).map_err(|error| ExportRefusal::FolderFailed {
        name: folder_name.clone(),
        reason: error.to_string(),
    })?;

    let mut written = Vec::with_capacity(sources.len());
    for (relative, source) in &sources {
        if let Err(refusal) = copy_into(&folder, relative, source) {
            // Promise 3: nothing half-written survives a refusal. Only the
            // folder keeper itself just created is removed — never anything
            // that was in the destination before.
            let _ = std::fs::remove_dir_all(&folder);
            return Err(refusal);
        }
        written.push(format!("{folder_name}/{relative}"));
    }

    Ok(Exported {
        path: folder,
        written,
    })
}

/// Copy one file to `folder/relative`, making the intermediate folders.
///
/// Refuses rather than overwrites when the path is already there: within one
/// export that can only mean two sources want one destination, and `fs::copy`
/// would silently keep the second and report both as written.
fn copy_into(folder: &Path, relative: &str, source: &Path) -> Result<(), ExportRefusal> {
    let target = folder.join(relative);
    if target.exists() {
        return Err(ExportRefusal::Collision {
            relative_path: relative.to_owned(),
        });
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ExportRefusal::CopyFailed {
            relative_path: relative.to_owned(),
            reason: error.to_string(),
        })?;
    }
    std::fs::copy(source, &target).map_err(|error| ExportRefusal::CopyFailed {
        relative_path: relative.to_owned(),
        reason: error.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Bytes that a re-encode, a line-ending fix or a UTF-8 round trip would
    /// each change. The export claims byte-identical, so the fixture has to be
    /// able to catch a copy that is merely text-identical.
    const AWKWARD_NOTE: &[u8] =
        b"\xef\xbb\xbf# Title\r\n\r\n![[photo.png]] ![[data/rows.csv]]\r\ntrailing   ";

    /// A binary attachment with a NUL, a lone continuation byte and no final
    /// newline. Invalid UTF-8 on purpose.
    const BINARY: &[u8] = b"\x89PNG\r\n\x1a\n\x00\xff\xfe binary \x80\x81";

    struct Fixture {
        _dir: tempfile::TempDir,
        vault: PathBuf,
        out: PathBuf,
    }

    /// A vault holding one note and two attachments, and an empty destination
    /// beside it. Two attachments, always: a copier that kept only the first
    /// would pass every single-attachment test.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = dir.path().join("vault");
        let out = dir.path().join("out");
        fs::create_dir_all(vault.join("notes")).expect("notes");
        fs::create_dir_all(vault.join("attachments")).expect("attachments");
        fs::create_dir_all(vault.join("data")).expect("data");
        fs::create_dir_all(&out).expect("out");
        fs::write(vault.join("notes/Meeting.md"), AWKWARD_NOTE).expect("note");
        fs::write(vault.join("attachments/photo.png"), BINARY).expect("photo");
        fs::write(vault.join("data/rows.csv"), b"a,b\r\n1,2").expect("csv");
        // Canonicalised, and this is a macOS fact rather than tidiness: the
        // system temp dir is `/var/...`, which is a symlink to `/private/var/...`.
        // Containment is decided by `browse::resolve`, which resolves symlinks,
        // so every path this module HANDS BACK is the real one. A fixture holding
        // the symlinked spelling makes `done.path` compare unequal to the folder
        // it actually wrote — a green suite on Linux and two reds on the only
        // machine that ships.
        let vault = fs::canonicalize(&vault).expect("canonical vault");
        let out = fs::canonicalize(&out).expect("canonical out");
        Fixture {
            _dir: dir,
            vault,
            out,
        }
    }

    fn carried() -> Vec<String> {
        vec![
            "attachments/photo.png".to_owned(),
            "data/rows.csv".to_owned(),
        ]
    }

    /// **Assert the fixtures are what they claim before asserting what copying
    /// them produces.** "Byte-identical" is only a claim about bytes if the
    /// bytes could tell a copy from a re-encode. A `BINARY` constant that
    /// happened to be valid UTF-8, or an `AWKWARD_NOTE` with no BOM and no
    /// CRLF, would make every assertion below pass over a decoding copier.
    #[test]
    fn the_fixtures_can_actually_catch_a_re_encode() {
        assert!(
            String::from_utf8(BINARY.to_vec()).is_err(),
            "the binary fixture must not be valid UTF-8"
        );
        assert!(BINARY.contains(&0), "it must contain a NUL");
        assert!(!BINARY.ends_with(b"\n"), "it must have no final newline");
        assert!(
            AWKWARD_NOTE.starts_with(b"\xef\xbb\xbf"),
            "the note fixture must open with a BOM"
        );
        assert!(
            AWKWARD_NOTE.windows(2).any(|pair| pair == b"\r\n"),
            "it must hold a CRLF"
        );
        assert!(
            AWKWARD_NOTE.ends_with(b" "),
            "it must end in trailing whitespace and no newline"
        );
    }

    /// The module doc says every source is re-resolved through
    /// [`browse::resolve`]. The note's own path is asserted elsewhere; this is
    /// the same promise for a CARRIED path, which arrives from a different
    /// producer and is the one a malformed embed could reach.
    #[test]
    fn a_carried_path_that_escapes_the_vault_is_refused_like_any_other() {
        let f = fixture();
        let refusal = export_note(
            &f.vault,
            "notes/Meeting.md",
            &[
                "attachments/photo.png".to_owned(),
                "../out/anything.png".to_owned(),
            ],
            &f.out,
        )
        .expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Escapes {
                subpath: "../out/anything.png".to_owned()
            }
        );
        assert!(!f.out.join("Meeting").exists());
    }

    /// [`files_write::collides`] only ever answers `Unreadable`, so the other
    /// arm of the conversion is unreachable through this module. It exists so
    /// the conversion is total rather than a panic, and this asserts that an
    /// unexpected refusal still arrives with its words intact rather than as a
    /// `Debug` shape or an empty sentence.
    #[test]
    fn an_unexpected_write_refusal_keeps_its_words() {
        let converted: ExportRefusal = WriteRefusal::NameTaken {
            name: "Report.pdf".to_owned(),
        }
        .into();
        match &converted {
            ExportRefusal::Unreadable { reason } => {
                assert!(reason.contains("Report.pdf"), "{reason}");
            }
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn a_note_exports_byte_identical_with_its_attachments_beside_it() {
        let f = fixture();
        let done = export_note(&f.vault, "notes/Meeting.md", &carried(), &f.out).expect("exported");

        assert_eq!(done.path, f.out.join("Meeting"));
        assert_eq!(
            done.written,
            vec![
                "Meeting/Meeting.md",
                "Meeting/attachments/photo.png",
                "Meeting/data/rows.csv"
            ]
        );
        // The bytes, not the text. A BOM, CRLF, a NUL and invalid UTF-8 all
        // survive, because nothing in this module decodes anything.
        assert_eq!(
            fs::read(f.out.join("Meeting/Meeting.md")).expect("read note"),
            AWKWARD_NOTE
        );
        assert_eq!(
            fs::read(f.out.join("Meeting/attachments/photo.png")).expect("read photo"),
            BINARY
        );
        assert_eq!(
            fs::read(f.out.join("Meeting/data/rows.csv")).expect("read csv"),
            b"a,b\r\n1,2"
        );
        // And the vault still has its own copy — an export is not a move.
        assert!(f.vault.join("notes/Meeting.md").exists());
        assert!(f.vault.join("attachments/photo.png").exists());
    }

    #[test]
    fn a_note_with_no_attachments_still_exports_into_its_own_folder() {
        let f = fixture();
        let done = export_note(&f.vault, "notes/Meeting.md", &[], &f.out).expect("exported");
        assert_eq!(done.written, vec!["Meeting/Meeting.md"]);
        assert!(f.out.join("Meeting/Meeting.md").is_file());
        assert!(!f.out.join("Meeting/attachments").exists());
    }

    #[test]
    fn one_file_exports_as_itself_and_not_into_a_folder() {
        let f = fixture();
        let done = export_entry(&f.vault, "attachments/photo.png", &f.out).expect("exported");
        assert_eq!(done.path, f.out.join("photo.png"));
        assert_eq!(done.written, vec!["photo.png"]);
        assert_eq!(fs::read(&done.path).expect("read"), BINARY);
    }

    #[test]
    fn a_name_already_in_the_destination_is_refused_case_insensitively() {
        let f = fixture();
        fs::create_dir(f.out.join("meeting")).expect("collider");
        let refusal =
            export_note(&f.vault, "notes/Meeting.md", &carried(), &f.out).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Taken {
                name: "Meeting".to_owned()
            }
        );
        // And it refused before writing: the collider is untouched and empty.
        assert_eq!(
            fs::read_dir(f.out.join("meeting")).expect("read").count(),
            0
        );
    }

    #[test]
    fn a_file_whose_name_is_already_in_the_destination_is_refused() {
        let f = fixture();
        fs::write(f.out.join("PHOTO.PNG"), b"someone else's").expect("collider");
        let refusal =
            export_entry(&f.vault, "attachments/photo.png", &f.out).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Taken {
                name: "photo.png".to_owned()
            }
        );
        assert_eq!(
            fs::read(f.out.join("PHOTO.PNG")).expect("read"),
            b"someone else's"
        );
    }

    #[test]
    fn a_destination_that_is_gone_says_so_and_writes_nothing() {
        let f = fixture();
        let gone = f.out.join("no-such-folder");
        let refusal =
            export_note(&f.vault, "notes/Meeting.md", &carried(), &gone).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::DestinationMissing {
                destination: gone.display().to_string()
            }
        );
        assert!(!gone.exists());
    }

    #[test]
    fn a_destination_that_is_a_file_says_so() {
        let f = fixture();
        let file = f.out.join("notes.txt");
        fs::write(&file, b"x").expect("file");
        let refusal =
            export_entry(&f.vault, "attachments/photo.png", &file).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::DestinationNotAFolder {
                destination: file.display().to_string()
            }
        );
        assert_eq!(fs::read(&file).expect("read"), b"x");
    }

    /// The acceptance criterion "a destination that cannot be written says so",
    /// over a real unwritable directory rather than a mocked failure.
    #[test]
    fn a_destination_that_cannot_be_written_says_so_in_the_os_words() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture();
        let locked = f.out.join("locked");
        fs::create_dir(&locked).expect("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).expect("chmod");

        let note = export_note(&f.vault, "notes/Meeting.md", &carried(), &locked);
        let file = export_entry(&f.vault, "attachments/photo.png", &locked);

        // Restore before asserting, so a failing assertion cannot leave a
        // directory the temp-dir teardown is unable to remove.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("restore");

        match note.expect_err("a note must refuse") {
            ExportRefusal::FolderFailed { name, reason } => {
                assert_eq!(name, "Meeting");
                assert!(!reason.is_empty(), "the OS's own words are the useful part");
            }
            other => panic!("expected FolderFailed, got {other:?}"),
        }
        match file.expect_err("a file must refuse") {
            ExportRefusal::CopyFailed { relative_path, .. } => {
                assert_eq!(relative_path, "photo.png");
            }
            other => panic!("expected CopyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_dir(&locked).expect("read").count(), 0);
    }

    #[test]
    fn exporting_into_the_folder_being_exported_from_is_refused() {
        let f = fixture();
        let inside = f.vault.join("data");
        let refusal = export_note(&f.vault, "notes/Meeting.md", &carried(), &inside)
            .expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::DestinationInsideSource {
                destination: inside.display().to_string()
            }
        );
        assert!(!inside.join("Meeting").exists());
    }

    #[test]
    fn a_subpath_that_escapes_the_root_is_refused() {
        let f = fixture();
        let refusal = export_entry(&f.vault, "../out/secret.txt", &f.out).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Escapes {
                subpath: "../out/secret.txt".to_owned()
            }
        );
    }

    #[test]
    fn a_file_that_is_gone_is_missing_rather_than_exported_empty() {
        let f = fixture();
        let refusal =
            export_entry(&f.vault, "attachments/nope.png", &f.out).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Missing {
                subpath: "attachments/nope.png".to_owned()
            }
        );
        assert_eq!(fs::read_dir(&f.out).expect("read").count(), 0);
    }

    #[test]
    fn a_folder_is_not_a_file_to_export() {
        let f = fixture();
        let refusal = export_entry(&f.vault, "attachments", &f.out).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::IsDirectory {
                name: "attachments".to_owned()
            }
        );
    }

    /// Promise 3 in full: an attachment that vanished between the plan and the
    /// copy refuses, and leaves the destination exactly as it found it.
    #[test]
    fn an_attachment_that_is_gone_refuses_and_leaves_no_folder_behind() {
        let f = fixture();
        fs::remove_file(f.vault.join("data/rows.csv")).expect("remove");
        let refusal =
            export_note(&f.vault, "notes/Meeting.md", &carried(), &f.out).expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Missing {
                subpath: "data/rows.csv".to_owned()
            }
        );
        assert_eq!(
            fs::read_dir(&f.out).expect("read").count(),
            0,
            "the destination must be as it was"
        );
    }

    /// The copy failing halfway is the case pre-resolution cannot cover: the
    /// source is there when it is checked and unreadable when it is read.
    #[test]
    fn a_copy_that_fails_halfway_removes_the_folder_it_made() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture();
        let unreadable = f.vault.join("data/rows.csv");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).expect("chmod");

        let outcome = export_note(&f.vault, "notes/Meeting.md", &carried(), &f.out);

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).expect("restore");

        match outcome.expect_err("must refuse") {
            ExportRefusal::CopyFailed { relative_path, .. } => {
                assert_eq!(relative_path, "data/rows.csv");
            }
            other => panic!("expected CopyFailed, got {other:?}"),
        }
        assert!(
            !f.out.join("Meeting").exists(),
            "the folder keeper made must be gone, and with it the note already copied into it"
        );
        assert_eq!(fs::read_dir(&f.out).expect("read").count(), 0);
    }

    #[test]
    fn two_carried_paths_that_land_on_one_name_refuse_rather_than_overwrite() {
        let f = fixture();
        let refusal = export_note(
            &f.vault,
            "notes/Meeting.md",
            &[
                "attachments/photo.png".to_owned(),
                "attachments/photo.png".to_owned(),
            ],
            &f.out,
        )
        .expect_err("must refuse");
        assert_eq!(
            refusal,
            ExportRefusal::Collision {
                relative_path: "attachments/photo.png".to_owned()
            }
        );
        assert!(!f.out.join("Meeting").exists());
    }

    #[test]
    fn the_folder_name_is_the_note_name_without_its_extension() {
        assert_eq!(note_folder_name("notes/Meeting.md"), "Meeting");
        assert_eq!(note_folder_name("Meeting.md"), "Meeting");
        assert_eq!(note_folder_name("notes/a.b.md"), "a.b");
        // All extension and no stem keeps its whole name: "" is not a folder.
        assert_eq!(note_folder_name("notes/.hidden"), ".hidden");
        assert_eq!(note_folder_name("notes/plain"), "plain");
    }

    /// Every refusal has to read on its own, because the shell prints it
    /// verbatim: no `Debug` shapes, no empty sentences, each one naming the
    /// thing it is about and each one finished.
    #[test]
    fn every_refusal_names_what_it_is_about_and_finishes_its_sentence() {
        let cases: Vec<(ExportRefusal, &str)> = vec![
            (
                ExportRefusal::DestinationMissing {
                    destination: "/tmp/gone".to_owned(),
                },
                "/tmp/gone",
            ),
            (
                ExportRefusal::DestinationNotAFolder {
                    destination: "/tmp/a.txt".to_owned(),
                },
                "/tmp/a.txt",
            ),
            (
                ExportRefusal::DestinationInsideSource {
                    destination: "/vault/sub".to_owned(),
                },
                "/vault/sub",
            ),
            (
                ExportRefusal::Taken {
                    name: "Meeting".to_owned(),
                },
                "Meeting",
            ),
            (
                ExportRefusal::Escapes {
                    subpath: "../x".to_owned(),
                },
                "../x",
            ),
            (
                ExportRefusal::Missing {
                    subpath: "a/b.png".to_owned(),
                },
                "a/b.png",
            ),
            (
                ExportRefusal::IsDirectory {
                    name: "data".to_owned(),
                },
                "data",
            ),
            (
                ExportRefusal::Unreadable {
                    reason: "permission denied".to_owned(),
                },
                "permission denied",
            ),
            (
                ExportRefusal::FolderFailed {
                    name: "Meeting".to_owned(),
                    reason: "read-only file system".to_owned(),
                },
                "read-only file system",
            ),
            (
                ExportRefusal::CopyFailed {
                    relative_path: "a/b.png".to_owned(),
                    reason: "input/output error".to_owned(),
                },
                "input/output error",
            ),
            (
                ExportRefusal::Collision {
                    relative_path: "a/b.png".to_owned(),
                },
                "a/b.png",
            ),
        ];
        for (refusal, needle) in cases {
            let sentence = refusal.to_string();
            assert!(
                sentence.contains(needle),
                "{refusal:?} must name {needle}: {sentence}"
            );
            assert!(
                sentence.ends_with('.'),
                "{refusal:?} must be a finished sentence: {sentence}"
            );
        }
    }
}

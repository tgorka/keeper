//! The drive as a filesystem a model may touch (Epic 61, Story 61.11, FR-389,
//! NFR-49).
//!
//! # This module invents no path rule, and that is the whole design
//!
//! Every function here takes `(root, subpath)` where `subpath` is
//! profile-relative and `/`-joined, and every one of them reaches the disk
//! through [`crate::browse::resolve`], [`crate::browse::plain_segments`] or
//! [`crate::files_write::WriteScope::route`]. There is **no path arithmetic in
//! this file and there must never be any**: `..`, an absolute path, a platform
//! separator, an empty segment, a `U+FFFD` rendering and a symlink whose
//! canonical form leaves the profile root are all refused by those functions,
//! and this module carries their refusal across verbatim exactly the way
//! `engine.rs:10159-10160` already carries it (AD-65). A second copy of the
//! rule would be a second chance for one of them to be wrong, and the one that
//! was wrong would be the one a model is driving.
//!
//! # Why it lives here rather than in `keeper-core` or in the shell
//!
//! The containment functions are here and cannot move: `keeper-core` may not
//! depend on this crate (AD-40, `check:core-sync-free`), and the shell crate
//! does not build on a Linux developer machine, so a containment rule written
//! there is a security rule proved on no machine any of this is developed on
//! (`files_write.rs:76-84`). Everything below is `std::fs` plus those
//! functions, so it is asserted over a real temp directory — with a real
//! symlink, a real pointer file, a real binary file and a real `rename` — on
//! any machine.
//!
//! # Who chooses the caps
//!
//! Nobody here. Every bound arrives as [`Limits`], because the *numbers* are a
//! decision and decisions live in `keeper_core::bots::tools` (AD-55/AD-56)
//! where they are exported to the model in the tool schema. This module's job
//! is to honour a bound it was given and to **report** what it left out —
//! [`FileRead::Text::truncated_at`], [`Listing::truncated_at`],
//! [`GrepResult::truncated_at`] — because a silent truncation makes a model
//! confidently wrong (NFR-49).
//!
//! # Three things a generic filesystem tool gets wrong here
//!
//! * **A file may be a pointer, not content** (epic 56, FR-331). Its worktree
//!   bytes *are* the committed LFS pointer, so a naive read returns 130 bytes
//!   of `version https://git-lfs.github.com/spec/v1` and a model then answers
//!   about a file it never saw. [`read`] probes for that first and answers
//!   [`FileRead::Pointer`], naming the real size. It does **not** materialize:
//!   "a `grep -r`, Spotlight, a backup agent … walks the tree and hydrates
//!   everything … Materialization is a verb somebody calls" (epic 56). A tool
//!   loop reading a subtree is exactly that `grep -r`.
//! * **Binary is a refusal, never a lossy conversion** — the position
//!   `keeper_core::text_file` already takes. `from_utf8_lossy` would hand the
//!   model `U+FFFD` where a byte it could not read was.
//! * **A write is temp-and-rename**, under the `.keeper.<ulid>.tmp` name tier 0
//!   already excludes, so the watcher never sees a torn file. That writer is
//!   [`crate::files_write::write_unmanaged`] and this module calls it rather
//!   than copying it.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::browse::{self, BrowseRefusal};
use crate::files_write::{self, UnmanagedPath, WriteRefusal, WriteRoute, WriteScope};
use crate::lfs::stage;

/// Every bound one tool call must respect.
///
/// Deliberately has **no `Default`**. The numbers are `keeper-core`'s, stated
/// once in `keeper_core::bots::tools` beside the JSON schema that tells the
/// model about them; a default here would be a second set of numbers that
/// could drift from the ones the model was promised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes one [`read`] returns.
    pub max_read_bytes: u64,
    /// The most entries one [`list`] returns.
    pub max_entries: usize,
    /// The most matches one [`grep`] returns.
    pub max_matches: usize,
    /// The most paths one [`glob`] returns.
    pub max_paths: usize,
    /// The most directory entries a recursive walk ([`glob`], [`grep`]) will
    /// look at before it stops and says so.
    pub max_walk_entries: usize,
    /// The most bytes one [`write_unmanaged`] will put on disk.
    pub max_write_bytes: u64,
    /// The most bytes of one line a [`grep`] match carries back.
    pub max_match_line_bytes: usize,
}

/// Why a filesystem tool call did not happen.
///
/// Two of the variants are other modules' verdicts carried across unchanged
/// ([`Self::Contained`], [`Self::Write`]) — their sentence is the containment
/// rule's own and this module paraphrases neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsRefusal {
    /// [`browse::resolve`] or [`browse::plain_segments`] refused the subpath:
    /// `..`, absolute, a platform separator, an empty segment, a `U+FFFD`
    /// rendering, or a symlink resolving outside the profile root.
    Contained(BrowseRefusal),
    /// [`WriteScope::route`] refused the write.
    Write(WriteRefusal),
    /// The lexical test passed and nothing is on disk there.
    Missing {
        /// Profile-relative, as asked for.
        subpath: String,
    },
    /// A file operation named a directory.
    NotAFile { subpath: String },
    /// A listing named something that is not a directory.
    NotADirectory { subpath: String },
    /// The bytes are not text, and a lossy conversion is not an answer.
    Binary { subpath: String },
    /// A write's content is larger than this surface will put on disk.
    TooLarge {
        subpath: String,
        /// What was offered.
        bytes: u64,
        /// The bound it passed.
        cap: u64,
    },
    /// The disk said no. The OS's own words, which are the useful part.
    Unreadable { subpath: String, reason: String },
    /// An edit's `old` text is not in the file.
    EditNoMatch { subpath: String },
    /// An edit's `old` text is in the file more than once, so which occurrence
    /// was meant is not knowable.
    EditAmbiguous { subpath: String, occurrences: usize },
    /// A glob pattern that is not a pattern.
    BadPattern { pattern: String, reason: String },
    /// An empty needle would match every line of every file.
    EmptyNeedle,
}

impl std::fmt::Display for FsRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contained(refusal) => write!(f, "{refusal}"),
            Self::Write(refusal) => write!(f, "{refusal}"),
            Self::Missing { subpath } => write!(f, "{subpath} is not in this folder"),
            Self::NotAFile { subpath } => {
                write!(f, "{subpath} is a folder, and this reads files")
            }
            Self::NotADirectory { subpath } => {
                write!(f, "{subpath} is a file, and this lists folders")
            }
            Self::Binary { subpath } => write!(
                f,
                "{subpath} is not text, and keeper will not hand over a lossy rendering of \
                 bytes it could not read"
            ),
            Self::TooLarge {
                subpath,
                bytes,
                cap,
            } => write!(
                f,
                "{subpath} would be {bytes} bytes and this surface writes at most {cap}"
            ),
            Self::Unreadable { subpath, reason } => {
                write!(f, "{subpath} could not be read: {reason}")
            }
            Self::EditNoMatch { subpath } => write!(
                f,
                "the text to replace is not in {subpath}; read it again and quote it exactly"
            ),
            Self::EditAmbiguous {
                subpath,
                occurrences,
            } => write!(
                f,
                "the text to replace appears {occurrences} times in {subpath}; quote enough \
                 surrounding text to name exactly one of them"
            ),
            Self::BadPattern { pattern, reason } => {
                write!(f, "\"{pattern}\" is not a usable pattern: {reason}")
            }
            Self::EmptyNeedle => write!(f, "a search needs something to search for"),
        }
    }
}

impl From<BrowseRefusal> for FsRefusal {
    fn from(refusal: BrowseRefusal) -> Self {
        Self::Contained(refusal)
    }
}

impl From<WriteRefusal> for FsRefusal {
    fn from(refusal: WriteRefusal) -> Self {
        Self::Write(refusal)
    }
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

/// One entry of a listing, in the frame the next call speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The entry's own name.
    pub name: String,
    /// Profile-relative and `/`-joined — feed it straight back as the next
    /// call's `subpath` and the frontend, the shell and the model all compose
    /// nothing (AD-65).
    pub subpath: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// The file's length in bytes, or the **pointer's** length for a path whose
    /// content has not been materialized — never the 130 bytes of pointer text
    /// (FR-336). `None` for a directory or unreadable metadata.
    pub bytes: Option<u64>,
    /// Whether those bytes are a promise rather than content on this disk.
    pub is_virtual: bool,
}

/// What one directory holds, and what was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The directory that was listed, profile-relative.
    pub subpath: String,
    /// Its entries, by name. Sorted BEFORE the cap is applied, so a truncated
    /// listing is a real alphabetical prefix rather than whichever entries the
    /// filesystem happened to hand over first — a model asked to "read the
    /// files in order" must get the same answer twice.
    pub entries: Vec<Entry>,
    /// How many entries were returned when the rest were dropped, or `None`
    /// when nothing was dropped.
    pub truncated_at: Option<usize>,
    /// How many entries the directory actually holds.
    pub of_entries: usize,
}

/// List one directory.
pub fn list(root: &Path, subpath: &str, limits: &Limits) -> Result<Listing, FsRefusal> {
    let resolved = resolve_existing(root, subpath)?;
    let meta = metadata(&resolved, subpath)?;
    if !meta.is_dir() {
        return Err(FsRefusal::NotADirectory {
            subpath: subpath.to_owned(),
        });
    }

    // Names first, sorted, and only then the cap — so the `stat` (and the
    // pointer probe behind it) is paid for the entries that are actually
    // returned rather than for a hundred thousand that are not.
    let mut names = Vec::new();
    for dirent in read_dir(&resolved, subpath)? {
        let dirent = dirent.map_err(|error| FsRefusal::Unreadable {
            subpath: subpath.to_owned(),
            reason: error.to_string(),
        })?;
        // A name that is not UTF-8 has no spelling this surface can hand back:
        // `plain_segments` refuses a `U+FFFD` rendering on the next call, so
        // offering the row would be offering a path the model cannot use.
        let Some(name) = dirent.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        names.push(name);
    }
    let of_entries = names.len();
    names.sort();
    names.truncate(limits.max_entries);

    let entries = names
        .into_iter()
        .map(|name| {
            let absolute = resolved.join(&name);
            // `metadata` follows symlinks, exactly as `browse` does: a symlink
            // to a directory expands like the directory, and `resolve` refuses
            // it on the next call if it points outside the root.
            let child_meta = std::fs::metadata(&absolute).ok();
            let (bytes, is_virtual) = match &child_meta {
                Some(meta) if meta.is_file() => match stage::worktree_pointer(&absolute, meta) {
                    Some(pointer) => (Some(pointer.size), true),
                    None => (Some(meta.len()), false),
                },
                _ => (None, false),
            };
            Entry {
                subpath: join(subpath, &name),
                name,
                is_dir: child_meta.as_ref().is_some_and(std::fs::Metadata::is_dir),
                bytes,
                is_virtual,
            }
        })
        .collect::<Vec<_>>();

    let truncated_at = (of_entries > entries.len()).then_some(entries.len());
    Ok(Listing {
        subpath: subpath.to_owned(),
        entries,
        truncated_at,
        of_entries,
    })
}

// ---------------------------------------------------------------------------
// read
// ---------------------------------------------------------------------------

/// Which lines of a file the caller wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineRange {
    /// The first line to return, 1-based. `None` is the start of the file.
    pub start_line: Option<u64>,
    /// How many lines to return. `None` is "as many as the byte cap allows".
    pub line_count: Option<u64>,
}

/// What a read of one path turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRead {
    /// The file is text and here is (some of) it.
    Text {
        /// The bytes, exactly: no line-ending normalisation, no BOM stripping,
        /// no trailing newline added or removed.
        body: String,
        /// How many bytes of the file the body covers.
        bytes_read: u64,
        /// How large the file is.
        of_bytes: u64,
        /// The byte offset the read stopped at, when it stopped early. `None`
        /// when the whole file (or the whole requested range) is in `body`.
        truncated_at: Option<u64>,
        /// The 1-based inclusive line span in `body`, when a range was asked
        /// for or applied.
        lines_shown: Option<(u64, u64)>,
    },
    /// The path holds keeper's committed LFS pointer and not the content
    /// (epic 56, FR-331). Reading did **not** trigger materialization.
    Pointer {
        /// The content's sha256, as git-lfs records it.
        oid: String,
        /// The real size of the content, from the pointer — not the ~130 bytes
        /// of pointer text on disk.
        of_bytes: u64,
    },
}

/// Read one file, bounded.
///
/// The order of the three questions is the contract, and it is not the obvious
/// one: **pointer first**. Pointer text is valid UTF-8, so a text-first read
/// would answer with `version https://git-lfs.github.com/spec/v1` and a model
/// would summarise a file that is not on this disk.
pub fn read(
    root: &Path,
    subpath: &str,
    range: LineRange,
    limits: &Limits,
) -> Result<FileRead, FsRefusal> {
    use std::io::Read as _;

    let resolved = resolve_existing(root, subpath)?;
    let meta = metadata(&resolved, subpath)?;
    if meta.is_dir() {
        return Err(FsRefusal::NotAFile {
            subpath: subpath.to_owned(),
        });
    }

    if let Some(pointer) = stage::worktree_pointer(&resolved, &meta) {
        return Ok(FileRead::Pointer {
            oid: pointer.oid,
            of_bytes: pointer.size,
        });
    }

    let of_bytes = meta.len();
    let mut bytes = Vec::new();
    std::fs::File::open(&resolved)
        .and_then(|file| file.take(limits.max_read_bytes).read_to_end(&mut bytes))
        .map_err(|error| FsRefusal::Unreadable {
            subpath: subpath.to_owned(),
            reason: error.to_string(),
        })?;

    // A NUL is the one byte no text file this surface serves contains, and it
    // is what separates a UTF-8-clean binary from text. Checked before the
    // decode so a `.wasm` that happens to decode is still refused.
    if bytes.contains(&0) {
        return Err(FsRefusal::Binary {
            subpath: subpath.to_owned(),
        });
    }
    // The prefix may have cut a multi-byte character in half, which is a fact
    // about the cap and not about the file, so that tail is dropped rather than
    // called binary.
    let (text, kept) = match std::str::from_utf8(&bytes) {
        Ok(text) => (text, bytes.len()),
        Err(error)
            if error.error_len().is_none() && bytes.len() as u64 == limits.max_read_bytes =>
        {
            let valid = error.valid_up_to();
            match std::str::from_utf8(&bytes[..valid]) {
                Ok(text) => (text, valid),
                Err(_) => {
                    return Err(FsRefusal::Binary {
                        subpath: subpath.to_owned(),
                    })
                }
            }
        }
        Err(_) => {
            return Err(FsRefusal::Binary {
                subpath: subpath.to_owned(),
            })
        }
    };

    let capped = kept as u64 == limits.max_read_bytes && of_bytes > kept as u64;
    let (body, lines_shown, range_cut) = slice_lines(text, range);
    let bytes_read = body.len() as u64;
    // Truncated when the byte cap cut the file short, or when a line range
    // stopped before the end of what was read. Either way the model is told the
    // offset it stopped at and the real size, so it can ask for the rest.
    let truncated_at = if capped || range_cut {
        Some(if capped { kept as u64 } else { bytes_read })
    } else {
        None
    };

    Ok(FileRead::Text {
        body,
        bytes_read,
        of_bytes,
        truncated_at,
        lines_shown,
    })
}

/// Apply a line range to already-read text.
///
/// Returns the slice, the 1-based inclusive span it covers, and whether
/// anything after it was dropped.
fn slice_lines(text: &str, range: LineRange) -> (String, Option<(u64, u64)>, bool) {
    if range.start_line.is_none() && range.line_count.is_none() {
        return (text.to_owned(), None, false);
    }
    let start = range.start_line.unwrap_or(1).max(1);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len() as u64;
    let first = (start - 1).min(total) as usize;
    let count = range.line_count.unwrap_or(total).min(total);
    let last = (first as u64 + count).min(total) as usize;
    let body: String = lines[first..last].concat();
    let span = if last > first {
        Some((first as u64 + 1, last as u64))
    } else {
        None
    };
    (body, span, last as u64 != total)
}

// ---------------------------------------------------------------------------
// stat
// ---------------------------------------------------------------------------

/// What one path is, without reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// Profile-relative, as asked for.
    pub subpath: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Its size — the pointer's number for a virtual path (FR-336).
    pub bytes: u64,
    /// Last modification, ms since the Unix epoch, where the OS reports one.
    pub modified_ms: Option<i64>,
    /// The content is a promise, not bytes on this disk.
    pub is_virtual: bool,
    /// The content's sha256 when it is virtual.
    pub lfs_oid: Option<String>,
}

/// Stat one path.
pub fn stat(root: &Path, subpath: &str) -> Result<Stat, FsRefusal> {
    let resolved = resolve_existing(root, subpath)?;
    let meta = metadata(&resolved, subpath)?;
    let pointer = if meta.is_file() {
        stage::worktree_pointer(&resolved, &meta)
    } else {
        None
    };
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_millis()).ok());
    Ok(Stat {
        subpath: subpath.to_owned(),
        is_dir: meta.is_dir(),
        bytes: pointer.as_ref().map_or_else(|| meta.len(), |p| p.size),
        modified_ms,
        is_virtual: pointer.is_some(),
        lfs_oid: pointer.map(|p| p.oid),
    })
}

// ---------------------------------------------------------------------------
// glob and grep — the two recursive walks
// ---------------------------------------------------------------------------

/// Paths matching a pattern, and what the walk left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobResult {
    /// Matching paths, profile-relative, in walk order.
    pub paths: Vec<String>,
    /// How many paths matched in total — the number the caller discloses when
    /// `paths` is only the first [`Limits::max_paths`] of them.
    pub of_paths: usize,
    /// How many were returned when the rest were dropped.
    pub truncated_at: Option<usize>,
    /// How many entries the walk looked at.
    pub walked: usize,
    /// Whether the walk stopped on [`Limits::max_walk_entries`] rather than
    /// because it ran out of tree.
    pub walk_capped: bool,
}

/// Find paths under `subpath` matching a gitignore-style glob.
pub fn glob(
    root: &Path,
    subpath: &str,
    pattern: &str,
    limits: &Limits,
) -> Result<GlobResult, FsRefusal> {
    // `literal_separator` so `*.md` does not cross a directory boundary, which
    // is what every tool in the field means by it and what a model expects.
    let matcher = globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|error| FsRefusal::BadPattern {
            pattern: pattern.to_owned(),
            reason: error.to_string(),
        })?
        .compile_matcher();

    let mut paths = Vec::new();
    let mut hits = 0usize;
    let walk = walk(root, subpath, limits, &mut |candidate: &Candidate| {
        // Matched against the path relative to the *searched* directory, so a
        // pattern the model wrote is about the subtree it asked for and not
        // about where that subtree happens to sit in the profile.
        if matcher.is_match(&candidate.walk_relative) {
            hits += 1;
            if paths.len() < limits.max_paths {
                paths.push(candidate.subpath.clone());
            }
        }
        true
    })?;

    Ok(GlobResult {
        truncated_at: (hits > paths.len()).then_some(paths.len()),
        of_paths: hits,
        paths,
        walked: walk.walked,
        walk_capped: walk.capped,
    })
}

/// One line that held the needle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Which file, profile-relative.
    pub subpath: String,
    /// Which line, 1-based.
    pub line: u64,
    /// The line, bounded by [`Limits::max_match_line_bytes`].
    pub text: String,
    /// Whether `text` is a prefix of the real line.
    pub line_truncated: bool,
}

/// What a search found, and what it could not look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepResult {
    /// The matches, in walk order.
    pub matches: Vec<Match>,
    /// How many were returned when the rest were dropped.
    pub truncated_at: Option<usize>,
    /// How many files were opened.
    pub files_searched: usize,
    /// How many files were skipped because they are binary, virtual, or larger
    /// than the read cap — counted rather than hidden, because "no matches" and
    /// "no matches in the half I could read" are different answers.
    pub files_skipped: usize,
    /// Whether the walk stopped on [`Limits::max_walk_entries`].
    pub walk_capped: bool,
}

/// Search file contents under `subpath` for a **literal** substring.
///
/// Literal and not a regular expression, deliberately and not as a shortcut:
/// the pattern here is written by a model from text it may have read out of the
/// drive, and a regular-expression engine driven by untrusted input is a
/// denial-of-service surface with no upside for the question a model actually
/// asks ("where does this string appear"). The tool description says so, so the
/// model is not guessing.
pub fn grep(
    root: &Path,
    subpath: &str,
    needle: &str,
    case_sensitive: bool,
    limits: &Limits,
) -> Result<GrepResult, FsRefusal> {
    if needle.is_empty() {
        return Err(FsRefusal::EmptyNeedle);
    }
    let folded_needle = needle.to_lowercase();

    let mut matches = Vec::new();
    let mut hits = 0usize;
    let mut files_searched = 0usize;
    let mut files_skipped = 0usize;

    let walk = walk(root, subpath, limits, &mut |candidate: &Candidate| {
        if candidate.is_dir {
            return true;
        }
        let read = read(root, &candidate.subpath, LineRange::default(), limits);
        let body = match read {
            Ok(FileRead::Text { body, .. }) => body,
            // A pointer holds no content to search, and materializing one to
            // look would be the mass hydration epic 56 forbids.
            Ok(FileRead::Pointer { .. }) | Err(_) => {
                files_skipped += 1;
                return true;
            }
        };
        files_searched += 1;
        for (index, line) in body.lines().enumerate() {
            let found = if case_sensitive {
                line.contains(needle)
            } else {
                line.to_lowercase().contains(&folded_needle)
            };
            if !found {
                continue;
            }
            hits += 1;
            if matches.len() >= limits.max_matches {
                continue;
            }
            let (text, line_truncated) = clip(line, limits.max_match_line_bytes);
            matches.push(Match {
                subpath: candidate.subpath.clone(),
                line: index as u64 + 1,
                text,
                line_truncated,
            });
        }
        true
    })?;

    Ok(GrepResult {
        truncated_at: (hits > matches.len()).then_some(matches.len()),
        matches,
        files_searched,
        files_skipped,
        walk_capped: walk.capped,
    })
}

/// One entry a walk offers its visitor.
struct Candidate {
    /// Profile-relative, `/`-joined.
    subpath: String,
    /// Relative to the directory the walk started at.
    walk_relative: String,
    is_dir: bool,
}

struct WalkReport {
    walked: usize,
    capped: bool,
}

/// Walk `subpath`'s subtree breadth-first, bounded, refusing to follow a
/// symlink.
///
/// **Symlinked directories are never descended and symlinked files are never
/// offered.** `browse::resolve` would refuse one pointing out of the root, but
/// one pointing *inside* it is a second name for a file the walk already has,
/// and a cycle of two of them is a walk that never ends. The entry-count bound
/// alone would turn that cycle into a truncated answer rather than a hang,
/// which is a worse thing to ship than simply not following links.
fn walk(
    root: &Path,
    subpath: &str,
    limits: &Limits,
    visit: &mut dyn FnMut(&Candidate) -> bool,
) -> Result<WalkReport, FsRefusal> {
    let start = resolve_existing(root, subpath)?;
    let meta = metadata(&start, subpath)?;
    if !meta.is_dir() {
        return Err(FsRefusal::NotADirectory {
            subpath: subpath.to_owned(),
        });
    }

    let mut queue: Vec<(PathBuf, String, String)> =
        vec![(start, subpath.to_owned(), String::new())];
    let mut walked = 0usize;
    let mut capped = false;

    while let Some((absolute, here_subpath, here_relative)) = queue.pop() {
        let entries = match read_dir(&absolute, &here_subpath) {
            Ok(entries) => entries,
            // A directory that cannot be opened mid-walk is not the whole
            // walk's failure; the counts say something was missed.
            Err(_) => continue,
        };
        for dirent in entries {
            let Ok(dirent) = dirent else { continue };
            let Some(name) = dirent.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if walked >= limits.max_walk_entries {
                capped = true;
                break;
            }
            walked += 1;
            let Ok(file_type) = dirent.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let child_subpath = join(&here_subpath, &name);
            let child_relative = join(&here_relative, &name);
            let candidate = Candidate {
                subpath: child_subpath.clone(),
                walk_relative: child_relative.clone(),
                is_dir: file_type.is_dir(),
            };
            if !visit(&candidate) {
                return Ok(WalkReport { walked, capped });
            }
            if file_type.is_dir() {
                queue.push((dirent.path(), child_subpath, child_relative));
            }
        }
        if capped {
            break;
        }
    }
    Ok(WalkReport { walked, capped })
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

/// What a search walk's visitor says of one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// A folder: walk into it. A file: it is a candidate — open it while
    /// the budget lasts, count it either way.
    Enter,
    /// Never descended, never opened, not counted.
    Skip,
    /// End the walk here.
    Stop,
}

/// What one walked entry is, in the frame the visitor decides on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEntry {
    /// Profile-relative, `/`-joined.
    pub subpath: String,
    pub is_dir: bool,
}

/// What a search reads of a file it opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scanned {
    /// The file is text, whole.
    Text(String),
    /// The file is an LFS pointer: its content is not on this disk and is
    /// never fetched; this is its real size. Its pointer text is not handed
    /// over.
    Pointer { size: u64 },
}

/// Why a search read handed nothing over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// Not a regular file, not on this disk (dataless: never opened),
    /// larger than the largest file read, not text, unreadable, or not the
    /// file it was a moment ago — replaced, moved, a link put in its or a
    /// folder's place, or changed while it was read.
    Skipped,
    /// The call's files, bytes or time are spent, or its bytes left are
    /// fewer than the file holds: never opened.
    Capped,
}

/// Where `subpath` of `root` lands: its drive-relative name through every
/// link, as [`browse::resolve`] finds it, `/`-joined. A search admits a path
/// where it is asked for AND where it lands, then reads the landing alone
/// ([`ScanBudget::read`]).
///
/// `Ok(None)` only where the disk says nothing is there (`NotFound`) and no
/// dangling link names it: a name the disk cannot be asked about — a folder
/// on the way that may not be searched, an I/O error — is refused, never
/// taken for an absence, since what is there is unknown.
pub fn search_landing(root: &Path, subpath: &str) -> Result<Option<String>, FsRefusal> {
    let target = browse::lexical_join(root, subpath)?;
    let unreadable = |error: std::io::Error| FsRefusal::Unreadable {
        subpath: subpath.to_owned(),
        reason: error.to_string(),
    };
    let canonical_root = root.canonicalize().map_err(unreadable)?;
    let resolved = match target.canonicalize() {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A dangling link is no absence: what it names is unknown.
            browse::landing(root, subpath)?;
            return Ok(None);
        }
        Err(error) => return Err(unreadable(error)),
    };
    let inside = resolved.strip_prefix(&canonical_root).map_err(|_| {
        BrowseRefusal::EscapesAfterResolution {
            subpath: subpath.to_owned(),
        }
    })?;
    let names: Option<Vec<&str>> = inside
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect();
    let names = names.ok_or_else(|| BrowseRefusal::Unspellable {
        subpath: subpath.to_owned(),
    })?;
    Ok(Some(names.join("/")))
}

/// What tells a search's time is up: the instant now.
pub type Clock = std::sync::Arc<dyn Fn() -> std::time::Instant + Send + Sync>;

/// One call's bounds, shared by every read and walk it makes: the files it
/// may open, the bytes it may read, the instant it stops (by `clock`), the
/// largest file it reads, and the entries it may look at.
#[derive(Clone)]
pub struct ScanBudget {
    pub max_files: usize,
    pub max_bytes: u64,
    pub max_file_bytes: u64,
    pub deadline: std::time::Instant,
    pub clock: Clock,
    pub max_entries: usize,
    /// Files opened so far: walked candidates and direct reads alike.
    pub opened: usize,
    /// Bytes read so far, by every read: text, a pointer, a binary or
    /// invalid file, a config, a listing, the vault index's rows.
    pub bytes: u64,
    /// Entries looked at so far.
    pub entries: usize,
    /// Whether a bound stopped the opening.
    pub capped: bool,
}

impl ScanBudget {
    /// Whether the deadline has passed; once it has, nothing more opens.
    fn timed_out(&mut self) -> bool {
        let out = (self.clock)() >= self.deadline;
        if out {
            self.capped = true;
        }
        out
    }

    /// Whether nothing more may be opened — the files, the bytes or the
    /// time spent — asked without opening or counting anything: where a
    /// search stops between candidates, before it resolves the next.
    pub fn exhausted(&mut self) -> bool {
        if self.opened >= self.max_files || self.bytes >= self.max_bytes || self.timed_out() {
            self.capped = true;
        }
        self.capped
    }

    /// Whether a walk may look at nothing more — the time or the entries
    /// spent — asked without looking at anything: where a search stops
    /// before it resolves the next folder to walk. The files and bytes
    /// spent stop what is opened, not what a walk counts.
    pub fn walk_spent(&mut self) -> bool {
        self.timed_out() || self.entries >= self.max_entries
    }

    /// Admit one more read — the call's one admission, a walked candidate's
    /// and a direct read's alike: none once the files, the bytes or the
    /// time are spent; one admitted counts as opened, once.
    fn admit(&mut self) -> bool {
        if self.exhausted() {
            return false;
        }
        self.opened += 1;
        true
    }

    /// Admit one open a reader outside [`Self::read`] makes — the vault's
    /// index database — as every read is admitted, and counted as opened.
    /// No file-size limit applies to it: what it hands over is counted by
    /// its own meter ([`Self::charge`]).
    pub fn admit_open(&mut self) -> bool {
        self.admit()
    }

    /// The bytes the next read may take: the largest file, or what is left.
    fn cap(&self) -> u64 {
        self.max_file_bytes
            .min(self.max_bytes.saturating_sub(self.bytes))
    }

    /// Read by `read` (given the largest file and the bytes it may take)
    /// once admitted, its bytes counted whatever it came to.
    fn take(
        &mut self,
        read: impl FnOnce(u64, u64) -> (Result<Scanned, Unread>, u64),
    ) -> Result<Scanned, Unread> {
        if !self.admit() {
            return Err(Unread::Capped);
        }
        let (got, read) = read(self.max_file_bytes, self.cap());
        self.bytes += read;
        if got == Err(Unread::Capped) {
            self.capped = true;
        }
        got
    }

    /// Read `landed` ([`search_landing`]) of `root` as a search reads
    /// anything — a candidate, a drive's config, a listing, a ranked or
    /// listed document: admitted as a walked candidate is, then opened from
    /// the root a name at a time, no link followed, so what is read is the
    /// landing that was admitted or nothing; a regular file on this disk,
    /// checked on the handle that is read; a pointer by its size alone;
    /// text of at most [`Self::max_file_bytes`] and of the call's bytes
    /// left, whole.
    pub fn read(&mut self, root: &Path, landed: &str) -> Result<Scanned, Unread> {
        self.take(|max, cap| read_landed(root, landed, max, cap))
    }

    /// The bytes the call may still read.
    pub fn bytes_left(&self) -> u64 {
        self.max_bytes.saturating_sub(self.bytes)
    }

    /// Count `bytes` read outside [`Self::read`] — the vault index's rows.
    pub fn charge(&mut self, bytes: u64) {
        self.bytes += bytes;
    }

    /// Whether the call's time is up, by its clock: what a reader outside
    /// [`Self::read`] stops on.
    pub fn expiry(&self) -> impl FnMut() -> bool + Send + 'static {
        let (clock, deadline) = (std::sync::Arc::clone(&self.clock), self.deadline);
        move || clock() >= deadline
    }

    /// The time the call has left, by its clock.
    pub fn time_left(&self) -> std::time::Duration {
        self.deadline.saturating_duration_since((self.clock)())
    }
}

#[cfg(unix)]
fn read_landed(root: &Path, landed: &str, max: u64, cap: u64) -> (Result<Scanned, Unread>, u64) {
    let skipped = (Err(Unread::Skipped), 0);
    let Ok(segments) = browse::plain_segments(landed) else {
        return skipped;
    };
    let Some((name, folders)) = segments.split_last() else {
        return skipped;
    };
    match nofollow::descend(root, folders) {
        Ok(dir) => nofollow::read_in(std::os::fd::AsFd::as_fd(&dir), name, max, cap),
        Err(_) => skipped,
    }
}

#[cfg(not(unix))]
fn read_landed(_: &Path, _: &str, _: u64, _: u64) -> (Result<Scanned, Unread>, u64) {
    // No `O_NOFOLLOW` open to bind a read to its landing: nothing is read.
    (Err(Unread::Skipped), 0)
}

/// The absolute path of `landed` ([`search_landing`]) of `root`, for a
/// search's reader that opens it by path — the vault's SQLite index — where
/// it is a file [`ScanBudget::read`] would open: reached from the root a
/// name at a time with no link followed, a regular file, its content on
/// this disk (a dataless file is never handed over: opening it would
/// materialize it).
#[cfg(unix)]
pub fn search_file(root: &Path, landed: &str) -> Result<PathBuf, Unread> {
    let segments = browse::plain_segments(landed).map_err(|_| Unread::Skipped)?;
    let (name, folders) = segments.split_last().ok_or(Unread::Skipped)?;
    let dir = nofollow::descend(root, folders).map_err(|_| Unread::Skipped)?;
    if !nofollow::file_at(std::os::fd::AsFd::as_fd(&dir), name, nofollow::dataless) {
        return Err(Unread::Skipped);
    }
    Ok(root
        .canonicalize()
        .map_err(|_| Unread::Skipped)?
        .join(landed))
}

#[cfg(not(unix))]
pub fn search_file(_: &Path, _: &str) -> Result<PathBuf, Unread> {
    // No `O_NOFOLLOW` descent to check a file by: nothing is handed over.
    Err(Unread::Skipped)
}

/// What one search walk met.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchWalk {
    /// Candidates the visitor entered, opened or not.
    pub candidates: usize,
    /// Candidates opened and handed over.
    pub searched: usize,
    /// Candidates opened and not handed over: binary, too large, not on
    /// this disk as content, unreadable, or not the file that was met.
    pub skipped: usize,
    /// Whether the walk stopped on [`ScanBudget::max_entries`] or the
    /// deadline before it had met every entry, so `candidates` is not the
    /// total.
    pub walk_capped: bool,
    /// Folders entered that could not be listed whole — not opened, not
    /// enumerated, enumeration failing part way, or an entry whose kind
    /// could not be asked — and so may hold candidates never met. One that
    /// was gone by the time it was opened is an absence, not counted.
    pub unreadable: usize,
}

/// Walk `start`'s subtree (a landing, [`search_landing`]) for a search: in
/// name order, depth first, each entry — `start` first — offered to
/// `visit` before anything of it is opened: a folder it skips is never
/// opened, a file it skips never read. Every folder is opened by its name
/// in the folder it was met in and every file read there as
/// [`ScanBudget::read`] reads, no link followed, so a link put in an
/// entry's place after it was offered is never followed. A file it enters
/// is read while `budget` lasts and handed to `found`. The deadline and
/// [`ScanBudget::max_entries`] stop the walk itself, while a folder is
/// being read.
pub fn search_walk(
    root: &Path,
    start: &str,
    budget: &mut ScanBudget,
    visit: &mut dyn FnMut(&SearchEntry) -> Step,
    found: &mut dyn FnMut(&str, Scanned),
) -> Result<SearchWalk, FsRefusal> {
    let segments = browse::plain_segments(start)?;
    let mut report = SearchWalk::default();
    let entry = SearchEntry {
        subpath: start.to_owned(),
        is_dir: true,
    };
    if visit(&entry) != Step::Enter {
        return Ok(report);
    }
    #[cfg(unix)]
    {
        nofollow::walk(
            root,
            &segments,
            entry.subpath,
            budget,
            &mut report,
            visit,
            found,
        )
        .map_err(|error| FsRefusal::Unreadable {
            subpath: start.to_owned(),
            reason: error.to_string(),
        })?;
        Ok(report)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, segments, budget, found);
        Err(FsRefusal::Unreadable {
            subpath: start.to_owned(),
            reason: "a search walk needs a Unix filesystem".to_owned(),
        })
    }
}

/// The search's opens, every one relative to a folder already open and
/// none following a link (`O_NOFOLLOW` at each name, AD-65's containment
/// held at the open rather than once for a path).
#[cfg(unix)]
mod nofollow {
    use std::ffi::OsStr;
    use std::io::Read as _;
    use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
    use std::path::Path;
    use std::rc::Rc;

    use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat};

    use super::{join, ScanBudget, Scanned, SearchEntry, SearchWalk, Step, Unread};
    use crate::lfs::pointer::{self, Pointer};

    /// The folder `name` of `parent`, never through a link.
    fn folder(parent: BorrowedFd<'_>, name: &OsStr) -> std::io::Result<OwnedFd> {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        Ok(rustix::fs::openat(parent, name, flags, Mode::empty())?)
    }

    /// The folder at `folders` under `root`'s canonical form, a name at a
    /// time.
    pub(super) fn descend(root: &Path, folders: &[&OsStr]) -> std::io::Result<OwnedFd> {
        let mut dir = rustix::fs::open(
            root.canonicalize()?,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        for name in folders {
            dir = folder(dir.as_fd(), name)?;
        }
        Ok(dir)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn dataless(stat: &Stat) -> bool {
        stat.st_flags & crate::stability::SF_DATALESS != 0
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn dataless(_: &Stat) -> bool {
        // As `stability::is_dataless`: no such state off macOS.
        false
    }

    fn plain_file(stat: &Stat) -> bool {
        plain_by(stat, dataless)
    }

    /// A regular file whose content `dataless` does not say is elsewhere.
    fn plain_by(stat: &Stat, dataless: impl Fn(&Stat) -> bool) -> bool {
        FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile && !dataless(stat)
    }

    /// Whether `name` of `dir`, never through a link, is a regular file
    /// whose content `dataless` does not say is elsewhere — asked of its
    /// `stat` alone, nothing opened.
    pub(super) fn file_at(
        dir: BorrowedFd<'_>,
        name: &OsStr,
        dataless: impl Fn(&Stat) -> bool,
    ) -> bool {
        rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
            .is_ok_and(|stat| plain_by(&stat, dataless))
    }

    /// Read the file `name` of `dir` as a search reads it, at most `cap`
    /// bytes, with the bytes read. Its type and materialization are asked
    /// before it is opened (a dataless file is never opened, a FIFO never
    /// waited on) and again of the handle, which must be the very file
    /// asked about; the pointer test and the text are the same bytes of
    /// that handle.
    pub(super) fn read_in(
        dir: BorrowedFd<'_>,
        name: &OsStr,
        max_file_bytes: u64,
        cap: u64,
    ) -> (Result<Scanned, Unread>, u64) {
        let skipped = (Err(Unread::Skipped), 0);
        let Ok(asked) = rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) else {
            return skipped;
        };
        let len = u64::try_from(asked.st_size).unwrap_or(u64::MAX);
        if !plain_file(&asked) || len > max_file_bytes {
            return skipped;
        }
        if len > cap {
            return (Err(Unread::Capped), 0);
        }
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let Ok(handle) = rustix::fs::openat(dir, name, flags, Mode::empty()) else {
            return skipped;
        };
        let Ok(held) = rustix::fs::fstat(&handle) else {
            return skipped;
        };
        if !plain_file(&held) || (held.st_dev, held.st_ino) != (asked.st_dev, asked.st_ino) {
            return skipped;
        }
        let mut bytes = Vec::new();
        let whole = std::fs::File::from(handle)
            .take(cap)
            .read_to_end(&mut bytes)
            .is_ok();
        let read = bytes.len() as u64;
        if !whole || read != len {
            return (Err(Unread::Skipped), read);
        }
        if bytes.len() <= pointer::MAX_POINTER_BYTES && pointer::is_pointer_candidate(&bytes) {
            if let Some(pointer) = Pointer::parse(&bytes) {
                return (Ok(Scanned::Pointer { size: pointer.size }), read);
            }
        }
        if bytes.contains(&0) {
            return (Err(Unread::Skipped), read);
        }
        match String::from_utf8(bytes) {
            Ok(text) => (Ok(Scanned::Text(text)), read),
            Err(_) => (Err(Unread::Skipped), read),
        }
    }

    /// Whether `error` says nothing is there, rather than that what is
    /// there could not be asked about.
    fn gone(error: &std::io::Error) -> bool {
        error.kind() == std::io::ErrorKind::NotFound
    }

    /// The folders and regular files of `dir` by name, links left out;
    /// `None` when the deadline or the entry bound stopped the reading. A
    /// folder that could not be listed whole counts in `report` as
    /// unreadable, with what of it was listed.
    fn children(
        dir: &OwnedFd,
        budget: &mut ScanBudget,
        report: &mut SearchWalk,
    ) -> Option<Vec<(String, bool)>> {
        let mut out = Vec::new();
        let Ok(mut entries) = rustix::fs::Dir::read_from(dir) else {
            report.unreadable += 1;
            return Some(out);
        };
        let mut whole = true;
        while let Some(entry) = entries.read() {
            let Ok(entry) = entry else {
                whole = false;
                break;
            };
            let name = entry.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if budget.timed_out() || budget.entries >= budget.max_entries {
                return None;
            }
            budget.entries += 1;
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            let kind = match entry.file_type() {
                FileType::Unknown => {
                    match rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
                        Ok(stat) => FileType::from_raw_mode(stat.st_mode),
                        Err(error) => {
                            whole &= gone(&error.into());
                            continue;
                        }
                    }
                }
                kind => kind,
            };
            match kind {
                FileType::Directory => out.push((name.to_owned(), true)),
                FileType::RegularFile => out.push((name.to_owned(), false)),
                _ => {}
            }
        }
        if !whole {
            report.unreadable += 1;
        }
        out.sort();
        Some(out)
    }

    pub(super) fn walk(
        root: &Path,
        start: &[&OsStr],
        subpath: String,
        budget: &mut ScanBudget,
        report: &mut SearchWalk,
        visit: &mut dyn FnMut(&SearchEntry) -> Step,
        found: &mut dyn FnMut(&str, Scanned),
    ) -> std::io::Result<()> {
        // A folder still to walk holds the folder it was met in open, and
        // is opened by its name there.
        let mut pending: Vec<(Rc<OwnedFd>, String, String)> = Vec::new();
        let mut next = Some((descend(root, start)?, subpath));
        loop {
            let (dir, here) = match next.take() {
                Some(first) => first,
                None => {
                    let Some((parent, name, subpath)) = pending.pop() else {
                        return Ok(());
                    };
                    match folder(parent.as_fd(), OsStr::new(&name)) {
                        Ok(dir) => (dir, subpath),
                        Err(error) => {
                            report.unreadable += usize::from(!gone(&error));
                            continue;
                        }
                    }
                }
            };
            let Some(children) = children(&dir, budget, report) else {
                report.walk_capped = true;
                return Ok(());
            };
            let dir = Rc::new(dir);
            let mut folders = Vec::new();
            for (name, is_dir) in children {
                if budget.timed_out() {
                    report.walk_capped = true;
                    return Ok(());
                }
                let entry = SearchEntry {
                    subpath: join(&here, &name),
                    is_dir,
                };
                match visit(&entry) {
                    Step::Skip => continue,
                    Step::Stop => return Ok(()),
                    Step::Enter => {}
                }
                if is_dir {
                    folders.push((Rc::clone(&dir), name, entry.subpath));
                    continue;
                }
                report.candidates += 1;
                let read =
                    budget.take(|max, cap| read_in(dir.as_fd(), OsStr::new(&name), max, cap));
                match read {
                    Ok(scanned) => {
                        report.searched += 1;
                        found(&entry.subpath, scanned);
                    }
                    Err(Unread::Skipped) => report.skipped += 1,
                    Err(Unread::Capped) => {}
                }
            }
            // Depth first, in name order: the first folder is walked next.
            pending.extend(folders.into_iter().rev());
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// What a search hands a reader that opens by path (the vault's
        /// index) passes the materialization check the content reader
        /// makes: under a `dataless` that says a file's content is
        /// elsewhere (macOS's `SF_DATALESS`, which Linux has not), the file
        /// is not handed over; a FIFO and a link never are.
        #[test]
        fn a_file_whose_content_is_elsewhere_is_never_handed_over() {
            let dir = tempfile::tempdir().expect("root");
            std::fs::write(dir.path().join("search.db"), b"db").expect("write");
            std::os::unix::fs::symlink(dir.path().join("search.db"), dir.path().join("link.db"))
                .expect("link");
            let made = std::process::Command::new("mkfifo")
                .arg(dir.path().join("pipe.db"))
                .status()
                .expect("mkfifo");
            assert!(made.success());
            let held = descend(dir.path(), &[]).expect("root");
            let at = |name: &str, elsewhere: bool| {
                file_at(held.as_fd(), OsStr::new(name), |_: &Stat| elsewhere)
            };
            assert!(at("search.db", false));
            assert!(!at("search.db", true), "a dataless file was handed over");
            assert!(!at("pipe.db", false) && !at("link.db", false));
        }
    }
}

// ---------------------------------------------------------------------------
// write and edit
// ---------------------------------------------------------------------------

/// What a completed write did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrote {
    /// Profile-relative, as asked for.
    pub subpath: String,
    /// How many bytes are now in the file.
    pub bytes: u64,
}

/// Decide which of keeper's two writers owns a path, without writing anything.
///
/// A one-line pass-through to [`WriteScope::route`] and that is the point: the
/// vault arm carries the caller's own vault handle, so a tool call cannot reach
/// the vault writer without one, and the unmanaged arm carries an
/// [`UnmanagedPath`] that `resolve_existing` already proved is a real
/// descendant of the profile root after symlinks.
pub fn plan_write<V>(
    scope: &WriteScope<'_>,
    vault: Option<V>,
    root: &Path,
    subpath: &str,
) -> Result<WriteRoute<V>, FsRefusal> {
    Ok(scope.route(vault, root, subpath)?)
}

/// Write text to a file no vault manages, atomically.
///
/// [`files_write::write_unmanaged`] does the temp-and-rename; this adds the one
/// thing a tool call needs that a person's editor does not — a size bound, so a
/// runaway model cannot fill the drive one call at a time.
pub fn write_unmanaged(
    target: &UnmanagedPath,
    content: &str,
    limits: &Limits,
) -> Result<Wrote, FsRefusal> {
    let bytes = content.len() as u64;
    if bytes > limits.max_write_bytes {
        return Err(FsRefusal::TooLarge {
            subpath: target.profile_relative().to_owned(),
            bytes,
            cap: limits.max_write_bytes,
        });
    }
    files_write::write_unmanaged(target, content)?;
    Ok(Wrote {
        subpath: target.profile_relative().to_owned(),
        bytes,
    })
}

/// What an edit would make the file say.
///
/// Separate from the write so the substitution rule is asserted without a
/// filesystem and so the resulting text goes out through the *same* routed
/// writer an ordinary write does — an edit is not a second way to put bytes on
/// the drive.
///
/// Exactly one occurrence, or a refusal: replacing the first of several is how
/// a model silently edits the wrong line, and replacing all of them is a
/// different verb the model did not ask for.
pub fn edited_text(
    root: &Path,
    subpath: &str,
    old: &str,
    new: &str,
    limits: &Limits,
) -> Result<String, FsRefusal> {
    if old.is_empty() {
        return Err(FsRefusal::EditNoMatch {
            subpath: subpath.to_owned(),
        });
    }
    let current = match read(root, subpath, LineRange::default(), limits)? {
        FileRead::Text {
            body,
            truncated_at: None,
            ..
        } => body,
        // Editing a prefix would delete everything past the cap on save — the
        // same reason `keeper_core::text_file` opens an oversize file read-only.
        FileRead::Text { of_bytes, .. } => {
            return Err(FsRefusal::TooLarge {
                subpath: subpath.to_owned(),
                bytes: of_bytes,
                cap: limits.max_read_bytes,
            })
        }
        FileRead::Pointer { of_bytes, .. } => {
            return Err(FsRefusal::TooLarge {
                subpath: subpath.to_owned(),
                bytes: of_bytes,
                cap: limits.max_read_bytes,
            })
        }
    };
    let occurrences = current.matches(old).count();
    match occurrences {
        0 => Err(FsRefusal::EditNoMatch {
            subpath: subpath.to_owned(),
        }),
        1 => Ok(current.replacen(old, new, 1)),
        occurrences => Err(FsRefusal::EditAmbiguous {
            subpath: subpath.to_owned(),
            occurrences,
        }),
    }
}

// ---------------------------------------------------------------------------
// The shared plumbing — every one of these is three lines around a borrowed rule
// ---------------------------------------------------------------------------

/// [`browse::resolve`], with its `Ok(None)` given the name a tool call needs.
fn resolve_existing(root: &Path, subpath: &str) -> Result<PathBuf, FsRefusal> {
    browse::resolve(root, subpath)?.ok_or_else(|| FsRefusal::Missing {
        subpath: subpath.to_owned(),
    })
}

fn metadata(absolute: &Path, subpath: &str) -> Result<std::fs::Metadata, FsRefusal> {
    std::fs::metadata(absolute).map_err(|error| FsRefusal::Unreadable {
        subpath: subpath.to_owned(),
        reason: error.to_string(),
    })
}

fn read_dir(absolute: &Path, subpath: &str) -> Result<std::fs::ReadDir, FsRefusal> {
    std::fs::read_dir(absolute).map_err(|error| FsRefusal::Unreadable {
        subpath: subpath.to_owned(),
        reason: error.to_string(),
    })
}

/// Join a parent subpath and one plain name in the `/`-joined frame every
/// caller speaks. Never a `Path::join`: this is the listing's own frame, and
/// the result is re-validated by [`browse::plain_segments`] on the next call.
fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// Bound one line at `cap` bytes, on a character boundary.
fn clip(line: &str, cap: usize) -> (String, bool) {
    if line.len() <= cap {
        return (line.to_owned(), false);
    }
    let mut end = cap;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    (line[..end].to_owned(), true)
}

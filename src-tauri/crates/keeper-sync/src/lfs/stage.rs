//! Routing oversized files through LFS at stage time (Stories 25.4, 25.5;
//! AD-46).
//!
//! # Why this exists at all
//!
//! gitoxide has **no streaming object read** — `try_find` always fills a
//! buffer — so a 3 GB file committed as an ordinary blob is a 3 GB allocation
//! every time anything touches it. Above a threshold, content must never
//! become a git blob.
//!
//! # Why there is no filter subprocess
//!
//! The obvious route is registering `filter.lfs.process` so gitoxide's filter
//! pipeline does clean/smudge for us. It is not needed. Verified empirically
//! (`tests/lfs_pointer_stat.rs`): an index entry whose **blob is the pointer**
//! but whose **stat is the worktree file's** reads as clean from both
//! `gix::status` and `git status`. That is precisely how real git+LFS stays
//! fast, and it lets keeper stay a single process with no `%f` quoting, no
//! protocol handshake, and no dependency on a `git-lfs` binary.
//!
//! The one wrinkle is git's *racily clean* rule: an entry whose mtime is not
//! older than the index is re-read regardless of stat. Right after staging,
//! that is every file we just touched, and re-reading finds the worktree bytes
//! differing from the pointer blob. [`is_false_modification`] answers that
//! without hashing gigabytes twice.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::error::{Result, SyncError};
use crate::lfs::pointer::{self, Pointer};
use crate::lfs::store::LfsStore;
use crate::profile::{LfsMode, SyncProfile};

/// Attribute line keeper maintains for an LFS-tracked pattern.
///
/// `diff=lfs merge=lfs` match what `git lfs track` writes, so a user running
/// the real client against the same repository sees a file it agrees with.
/// `-text` stops any EOL conversion touching binary content.
const ATTRIBUTE_SUFFIX: &str = "filter=lfs diff=lfs merge=lfs -text";

/// The `filter` driver named in [`ATTRIBUTE_SUFFIX`], and the one `git lfs
/// track` writes.
///
/// [`routed_through_lfs`] matches the path's resolved `filter` attribute
/// against it, so the rule keeper *writes* and the rule keeper *reads back*
/// are one constant rather than two literals free to drift apart.
const LFS_FILTER_DRIVER: &str = "lfs";

/// Marker keeper writes above the block it owns in `.gitattributes`.
const MANAGED_HEADER: &str = "# keeper-sync: managed LFS rules — edit above this line";

/// What staging decided for one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedObject {
    /// Repository-relative path.
    pub path: PathBuf,
    pub oid: String,
    pub size: u64,
}

/// Everything the commit path needs to route large files through LFS.
#[derive(Debug, Default)]
pub struct LfsStaging {
    /// `path -> pointer bytes`, handed to `stage_and_commit` so the blob it
    /// writes is the pointer rather than the content.
    pub substitutions: BTreeMap<PathBuf, Vec<u8>>,
    /// Objects now in the local store that the remote has not seen.
    pub uploads: Vec<StagedObject>,
    /// `.gitattributes` changed and must be part of this commit.
    pub attributes_changed: bool,
}

/// Should this path be tracked through LFS?
///
/// Size alone, deliberately. Extension allow-lists are a support burden and get
/// the interesting case wrong: the 6 GB `.csv` export is exactly what must not
/// become a git blob.
pub fn applies(profile: &SyncProfile, size: u64) -> bool {
    profile.lfs_mode != LfsMode::Disabled && size >= profile.lfs_threshold_bytes
}

/// The size rule plus the profile's opt-out globs, compiled once per run.
///
/// Two things make the opt-out necessary in a repository that holds both notes
/// and media. First, the recorded `.gitattributes` rule is per-extension
/// ([`pattern_for`]), so a single oversized file converts its whole extension
/// for the rest of the repository's life. Second, the threshold that is right
/// for media is far below the size of a large text file — the lower it goes to
/// catch bulk, the likelier it is to swallow a format the user needs to diff.
///
/// Compiled once and reused: `prepare` walks every candidate, and building a
/// [`GlobSet`] per path would make the common case (no opt-out configured) pay
/// for a feature it does not use.
#[derive(Debug)]
pub struct LfsPolicy {
    threshold: u64,
    enabled: bool,
    never: Option<GlobSet>,
}

impl LfsPolicy {
    /// Glob semantics follow `.gitignore` and `.gitattributes`, because that is
    /// what anyone writing these patterns has already learned: a pattern with no
    /// `/` matches its basename at **any** depth (`*.md` covers
    /// `10-notes/a/b.md`), and a pattern containing `/` is anchored at the
    /// repository root (`10-notes/**` covers only that zone). Inventing a third
    /// dialect here would be a trap, not a feature.
    ///
    /// Refuses malformed globs rather than silently ignoring them — a typo in
    /// an opt-out that quietly does nothing is how a note ends up an opaque
    /// pointer months later.
    pub fn from_profile(profile: &SyncProfile) -> Result<Self> {
        let never = if profile.lfs_never.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for pattern in &profile.lfs_never {
                let anchored: Cow<'_, str> = if pattern.contains('/') {
                    Cow::Borrowed(pattern.as_str())
                } else {
                    Cow::Owned(format!("**/{pattern}"))
                };
                let glob = GlobBuilder::new(&anchored)
                    .literal_separator(true)
                    .build()
                    .map_err(|err| {
                        SyncError::Config(format!("invalid lfsNever glob `{pattern}`: {err}"))
                    })?;
                builder.add(glob);
            }
            Some(builder.build().map_err(|err| {
                SyncError::Config(format!("could not compile lfsNever globs: {err}"))
            })?)
        };
        Ok(Self {
            threshold: profile.lfs_threshold_bytes,
            enabled: profile.lfs_mode != LfsMode::Disabled,
            never,
        })
    }

    /// `path` is repository-relative, which is what the globs are written
    /// against — matching an absolute path would make `*.md` depend on where
    /// the folder happens to be mounted.
    pub fn applies(&self, path: &Path, size: u64) -> bool {
        if !self.enabled || size < self.threshold {
            return false;
        }
        !self
            .never
            .as_ref()
            .is_some_and(|never| never.is_match(path))
    }
}

/// The `.gitattributes` pattern that covers `path`.
///
/// A per-extension rule (`*.mp4`) rather than a per-file one, so the file's
/// siblings are covered too and the block stays small on a media folder. A path
/// with no extension gets an exact-path rule.
///
/// The pattern returned is the **raw** one — `/my holiday` for a path with a
/// space, not `"/my holiday"`. Quoting is a property of the line, not of the
/// pattern, and it is applied by [`ensure_attributes`] at the moment it writes
/// one; see [`quote_pattern`]. Keeping it out of here is what lets the
/// idempotence check compare like with like, and it keeps `ensure_lfs_rule`'s
/// log line readable.
pub fn pattern_for(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => pattern_for_extension(ext),
        // Escape nothing: a leading `/` anchors the pattern at the repository
        // root, which is what an exact-path rule means in gitattributes.
        _ => format!("/{}", path.to_string_lossy().replace('\\', "/")),
    }
}

/// The `.gitattributes` pattern that covers every file with this extension.
///
/// Split out of [`pattern_for`] for the caller that has no file yet: a
/// recording session writes its rule at session start (Story 41.5, FR-137), so
/// the working tree does not change under a running recorder, and at that
/// moment the only thing known about the segment is the extension it will
/// carry. One function so that the rule the session writes and the rule the
/// commit path would write are the same string by construction — two spellings
/// would each be idempotent against themselves and duplicate against the other.
///
/// `ext` is bare (`mp4`), and a leading dot is tolerated because half the
/// world's APIs return one.
pub fn pattern_for_extension(ext: &str) -> String {
    format!("*.{}", ext.trim().trim_start_matches('.'))
}

/// `pattern` spelled so a `.gitattributes` line reads back as `pattern`.
///
/// # Why quoting, and why nothing else works
///
/// A gitattributes line is `<pattern> <attr>…` split on blanks, so a pattern
/// holding a space needs an escape, and gitattributes(5) offers exactly one:
/// *"Patterns that begin with a double quote are quoted in C style."* Git
/// decides whether to unquote from the line's **first byte**, before it has
/// looked for a separator, so a backslash is not an alternative spelling — it
/// is a silent miss. `/a\ b filter=lfs` still splits at the blank, and git
/// reads `b` as an attribute name to *set* on `/a\`; verified against git
/// 2.53.0. gitoxide's parser branches on the same byte, so a line written this
/// way is also the line `routed_through_lfs` reads back.
///
/// # Why only when required
///
/// `"*.mp4"` and `*.mp4` mean the same thing to git, but only one of them is
/// what is already in every user's `.gitattributes`. Quoting unconditionally
/// would rewrite every existing line the first time keeper touched the file —
/// a diff in a versioned, hand-editable file, in exchange for no behaviour. So
/// the quotes appear only for a pattern that cannot be spelled without them,
/// and every pattern keeper has written until now comes back byte-identical.
///
/// # Which bytes force it
///
/// Blanks and the C0 controls (together, `b <= b' '`), DEL, and the two bytes
/// the encoding itself must escape (`"` and `\`) — the last two so the emitted
/// field is unambiguous rather than merely lucky. A leading `#` forces it too,
/// because git reads a line beginning with `#` as a comment before it reads
/// anything else. Bytes at or above `0x80` never force it: a UTF-8 filename is
/// not a quoting problem, and octal-escaping one would churn `café.mp4` into
/// `"caf\303\251.mp4"` in a file that reads it fine bare.
///
/// The result is the exact inverse of [`gix_quote::ansi_c::undo`] — the
/// decoder [`attribute_pattern_matches`] reads it back with, and the one
/// `gix-attributes` resolves it with. That crate exposes no encoder, which is
/// why this is written by hand; the round trip is pinned by test
/// `quoting_is_the_exact_inverse_of_the_decoder_git_and_gitoxide_use`.
pub fn quote_pattern(pattern: &str) -> String {
    if !needs_quotes(pattern) {
        return pattern.to_owned();
    }
    // The two quotes, plus slack for the escapes that forced them.
    let mut out = String::with_capacity(pattern.len() + 8);
    out.push('"');
    for ch in pattern.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\x0b' => out.push_str("\\v"),
            '\x0c' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            // Whatever control is left is single-byte, so it fits the three
            // octal digits `undo` reads back — and its leading digit is 0 or 1,
            // which is inside the `\0`–`\3` opener that decoder accepts.
            c if (c as u32) < 0x20 || c == '\x7f' => {
                let byte = c as u32;
                out.push('\\');
                out.push(char::from(b'0' + ((byte >> 6) & 7) as u8));
                out.push(char::from(b'0' + ((byte >> 3) & 7) as u8));
                out.push(char::from(b'0' + (byte & 7) as u8));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Does `pattern` need C-style quoting to survive a `.gitattributes` line?
///
/// See [`quote_pattern`] for why each of these forces it. Separate so the
/// common "no quotes needed" answer is one scan of the bytes and no allocation
/// beyond the copy the caller was going to make anyway.
fn needs_quotes(pattern: &str) -> bool {
    // An empty pattern would collapse into the separator and leave `filter=lfs`
    // standing where the pattern belongs — a line that silently sets `filter`
    // on nothing. Neither producer can make one; this is not the failure to
    // leave reachable.
    pattern.is_empty()
        || pattern.starts_with('#')
        || pattern
            .bytes()
            .any(|b| b <= b' ' || b == 0x7f || b == b'"' || b == b'\\')
}

/// Is the pattern field of `line` exactly `pattern`?
///
/// This is the half of [`ensure_attributes`] that decides a rule is already
/// present, and it has to be able to read what [`quote_pattern`] writes.
/// Splitting the line on blanks — what this did before Story 46.1 — tokenises
/// `"/a b" filter=lfs` as `"/a`, which equals no pattern that has ever
/// existed, so the rule reads as absent and is appended again on **every**
/// run. That is not a cosmetic miss. It is how one owner's `.gitattributes`
/// reached fifty-nine copies of the same rule, and it is why quoting the
/// writer without teaching the reader would have been strictly worse than the
/// bug it fixes: FR-137 allows one `.gitattributes` write per session, and an
/// unreadable line turns every commit into another one.
///
/// Both spellings count. A pattern needing no quotes is still written bare, and
/// a bare line written by an older keeper, by `git lfs track`, or by the user
/// must keep counting as coverage.
///
/// Two shapes answer "absent": malformed quoting, and a quoted field that does
/// not end where its closing quote does (`"a"b filter=lfs`). Neither is
/// something keeper wrote. Both cost one appended rule and never a missing one,
/// which is the direction to fail in.
fn attribute_pattern_matches(line: &str, pattern: &str) -> bool {
    if !line.starts_with('"') {
        return line.split_whitespace().next() == Some(pattern);
    }
    let Ok((unquoted, consumed)) = gix_quote::ansi_c::undo(gix::bstr::BStr::new(line.as_bytes()))
    else {
        return false;
    };
    if !line
        .as_bytes()
        .get(consumed)
        .is_none_or(u8::is_ascii_whitespace)
    {
        return false;
    }
    let unquoted: &[u8] = &unquoted;
    unquoted == pattern.as_bytes()
}

/// Ensure `.gitattributes` contains a rule for every pattern.
///
/// Returns whether the file changed. Idempotent, and it never reorders or
/// removes anything a human wrote: keeper's rules live in an appended block
/// below a marker, and existing coverage — however the user spelled it — is
/// respected rather than duplicated.
///
/// Patterns are written through
/// [`quote_pattern`] and read back through [`attribute_pattern_matches`], so a
/// pattern holding a space is one line that git can parse and that this
/// function recognises as its own on the next run.
pub fn ensure_attributes(root: &Path, patterns: &[String]) -> Result<bool> {
    let file = root.join(".gitattributes");
    let existing = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(SyncError::io("read .gitattributes", file, err)),
    };

    let mut wanted: Vec<&String> = Vec::new();
    for pattern in patterns {
        let already = existing.lines().any(|line| {
            let line = line.trim();
            !line.starts_with('#')
                && attribute_pattern_matches(line, pattern)
                && line.contains("filter=lfs")
        });
        if !already && !wanted.contains(&pattern) {
            wanted.push(pattern);
        }
    }
    if wanted.is_empty() {
        return Ok(false);
    }

    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.contains(MANAGED_HEADER) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(MANAGED_HEADER);
        out.push('\n');
    }
    for pattern in wanted {
        out.push_str(&quote_pattern(pattern));
        out.push(' ');
        out.push_str(ATTRIBUTE_SUFFIX);
        out.push('\n');
    }
    std::fs::write(&file, out).map_err(|err| SyncError::io("write .gitattributes", file, err))?;
    Ok(true)
}

/// Move a file's content into the LFS store and return its pointer.
///
/// Streams: the file is read in bounded chunks, hashed as it goes, and written
/// straight into the store's staging area. Nothing is ever fully buffered,
/// which is the whole point.
pub fn clean(store: &LfsStore, absolute: &Path) -> Result<Pointer> {
    let file = std::fs::File::open(absolute)
        .map_err(|err| SyncError::io("open for LFS staging", absolute, err))?;
    let size = file
        .metadata()
        .map_err(|err| SyncError::io("stat for LFS staging", absolute, err))?
        .len();
    let (oid, written) = store.insert_streaming(file)?;
    if written != size {
        // The file changed under us mid-read. Retryable, and the caller
        // requeues rather than committing a pointer to content that moved.
        return Err(SyncError::Integrity {
            subject: absolute.to_string_lossy().into_owned(),
            expected: format!("{size} bytes"),
            actual: format!("{written} bytes"),
        });
    }
    Ok(Pointer::new(oid, size))
}

/// The index key for a repository-relative path.
///
/// git spells every path with forward slashes, so a Windows `a\b` finds
/// nothing until it is asked for as `a/b`.
fn index_key(rela: &Path) -> String {
    rela.to_string_lossy().replace('\\', "/")
}

/// The pointer `blob` holds, if it is one.
///
/// The **blob's own** length decides it, read from the object header so
/// nothing larger is ever loaded. Consulting the index entry's `stat.size`
/// instead is the mistake this was born with: for an LFS entry that stat is
/// the WORKTREE file's, deliberately — see `git::commit::stage_and_commit` —
/// so it measures the gigabytes the pointer stands in for and never the ~130
/// bytes actually staged. It therefore rejected precisely the entries this
/// exists to recognise, and every real LFS file answered `None`.
///
/// # Why an EMPTY blob answers `None`, and why the refusal lives here
///
/// [`Pointer::parse`] answers `Some(empty pointer)` for zero bytes, and that
/// is right where it is: the LFS spec carves empty files out of the format, so
/// a zero-byte *pointer file* stands for the empty object, and `lfs::filter`
/// depends on precisely that reading — it guards emptiness explicitly so an
/// empty file still takes the store path instead of being re-emitted as
/// nothing. Inheriting the carve-out **here** is what was wrong (Story 34.13
/// review). The question this function asks is "is this git blob an LFS
/// pointer", and every empty tracked file in the repository shares the one
/// empty blob; answering `Some` made [`indexed_pointer`] call all of them LFS
/// entries and handed [`is_false_modification`] a dismissible entry for each —
/// one value away from the I/O-matrix row this story exists to establish
/// ("Ordinary small file | blob = 5 bytes, not a pointer | `None`"), which has
/// no row for the empty blob because nobody expected it to be one.
///
/// Refusing costs nothing: an empty worktree file has no content to route
/// through LFS, so there is no pointer design for a racily-clean re-read to
/// have rediscovered, and the guard below simply declines to dismiss it. It
/// also saves loading the object at all, since the header already said zero.
fn pointer_blob(repo: &gix::Repository, blob: gix::hash::ObjectId) -> Option<Pointer> {
    let size = repo.find_header(blob).ok()?.size();
    if size == 0 || size > pointer::MAX_POINTER_BYTES as u64 {
        return None;
    }
    Pointer::parse(&repo.find_object(blob).ok()?.data)
}

/// Read the pointer recorded in the index for `rela`, if the blob is one.
///
/// Returns `None` for an ordinary file — the common case — after reading at
/// most [`pointer::MAX_POINTER_BYTES`] of object data, so this is safe to call
/// on any modified path.
///
/// An **empty** tracked file counts as ordinary here and answers `None`, even
/// though `Pointer::parse` reads zero bytes as the empty pointer — see
/// `pointer_blob` for why that carve-out stops at the blob layer.
pub fn indexed_pointer(repo: &gix::Repository, rela: &Path) -> Option<Pointer> {
    let index = repo.index_or_empty().ok()?;
    let key = index_key(rela);
    let entry = index.entry_by_path(gix::bstr::BStr::new(key.as_bytes()))?;
    pointer_blob(repo, entry.id)
}

/// Is `rela` reported as modified only because of git's racily-clean rule?
///
/// An entry whose mtime is not older than the index is re-read regardless of
/// its stat, and re-reading an LFS entry finds the worktree's gigabytes where
/// the blob holds a pointer. That difference is the design, not an edit, and
/// acting on it re-hashes the whole file to arrive back at the same pointer.
///
/// Four things must hold before a report is dismissed, and none of them reads
/// the file's content:
///
/// * The worktree's stat still matches the entry's. `gix::status` calls a path
///   unchanged exactly when that comparison passes and the entry is not racy,
///   so a match here means raciness was its only reason for speaking up. A
///   stat that differs is a genuine touch and must be re-staged even at
///   identical length — which is what a comparison against the pointer's
///   `size` alone could not tell apart, and it would have dropped an in-place
///   edit that happened to preserve the byte count for good. The repository's
///   own `core.trustCtime` / `core.checkStat` settings are read rather than
///   assumed, so the comparison is the same one status made.
/// * `.gitattributes` routes the path through LFS — see `routed_through_lfs`
///   and the section below.
/// * The staged blob is a pointer. Routing says the path is *meant* to hold
///   one; this says it actually does. An LFS-routed path whose blob is
///   ordinary content — committed before the rule existed, or by a client that
///   ignored it — has no pointer design for the re-read to have found, so a
///   difference there is an edit like any other.
/// * `HEAD` already records that same blob. `stage_and_commit` writes the
///   index before the commit so a crash between the two is re-driven on the
///   next pass (NFR-24); dismissing a path whose pointer is staged but not yet
///   committed would strand it until the file changed again.
///
/// # Why the path's ROUTING and not the blob's SHAPE (Story 34.13 review)
///
/// This guard first shipped asking a single question about LFS-ness —
/// `pointer_blob(entry.id).is_some()`, "do these ~130 bytes parse as a
/// pointer" — which is a fact about the blob's shape and says nothing about
/// the path. A tracked TEXT file whose content is literally a pointer is an
/// everyday object: documentation about the pointer format, a test fixture, a
/// `.gitattributes` example, a pointer committed by a peer whose smudge filter
/// never ran. Under the contract's Never clause ("do not suppress a
/// modification for an ordinary (non-pointer) blob: for those, a racily-clean
/// re-read finding different bytes is a real edit caught inside the race
/// window") every one of those is ordinary; under the shape test every one of
/// them was a pointer. A real edit to such a file landing in the race window
/// at the same length was therefore dismissed — and dismissed again on every
/// later scan, because git keeps reporting it. That is the identical
/// permanent-loss shape the Design Note calls "a data-loss-class outcome"
/// when arguing why the old length comparison had to go.
///
/// `.gitattributes` is where routing actually lives: it is what git itself
/// consults to decide whether to run a filter, it is what [`ensure_attributes`]
/// writes, it is versioned alongside the very commit that turned the path into
/// a pointer, and it is what a peer's real `git-lfs` reads. The other
/// candidate, [`LfsPolicy::from_profile`] — available in this module and so
/// the tempting one — was rejected on three counts. It answers "would keeper
/// route this path *now*", a prediction that drifts the instant the threshold
/// or an `lfsNever` glob changes, leaving already-committed pointers claiming
/// not to be LFS. It needs the file's size, and in the racily-clean case the
/// worktree size is the number under suspicion. And the profile is not
/// reachable from this signature, so the call site would have to thread it
/// through to obtain a worse answer than the repository already holds.
///
/// # What happens when routing cannot be determined
///
/// Nothing is dismissed. An unreadable attribute configuration, a path the
/// attribute stack cannot descend to, and simply no `filter` attribute at all
/// each answer "not routed", so the report stands. Suppression is the only
/// dangerous direction here: declining to dismiss costs one re-clean whose
/// identical pointer leaves the tree unchanged and produces no commit, while
/// dismissing wrongly loses the user's edit for good. That is the same rule
/// the rest of this function already follows — every failure to read answers
/// "this is a real modification".
pub fn is_false_modification(repo: &gix::Repository, rela: &Path, absolute: &Path) -> bool {
    let Ok(index) = repo.index_or_empty() else {
        return false;
    };
    let key = index_key(rela);
    let Some(entry) = index.entry_by_path(gix::bstr::BStr::new(key.as_bytes())) else {
        return false;
    };
    let Ok(options) = repo.stat_options() else {
        return false;
    };
    // The stat comparison leads because it costs one `lstat` and it is what
    // rejects an ordinary modified file, whose stat has moved. Everything
    // below reads either `.gitattributes` or the object database, and this
    // keeps all of it off the path every non-LFS modification takes.
    let unmoved = gix::index::fs::Metadata::from_path_no_follow(absolute)
        .ok()
        .and_then(|metadata| gix::index::entry::Stat::from_fs(&metadata).ok())
        .is_some_and(|worktree| worktree.matches(&entry.stat, options));
    if !unmoved || !routed_through_lfs(repo, &index, &key, entry.mode) {
        return false;
    }
    if pointer_blob(repo, entry.id).is_none() {
        return false;
    }
    head_records(repo, rela, entry.id)
}

/// Does `.gitattributes` route `key` through the LFS filter?
///
/// `key` is the slash-separated index key, which is also the spelling the
/// attribute stack matches patterns against. `filter=lfs` is the assignment
/// `git lfs track` writes and the one [`ATTRIBUTE_SUFFIX`] writes, so this
/// asks the same question git asks when it decides whether a filter runs at
/// all — which is what makes it a statement about the path rather than about
/// the bytes that happen to be staged for it.
///
/// The worktree's `.gitattributes` wins over the indexed one
/// (`Source::WorktreeThenIdMapping`) because this is the check-*in* direction:
/// a rule the user has just added or just removed governs the next commit, and
/// that is the source gitoxide names for exactly this case.
///
/// Every failure — unreadable `core.attributesFile`, an attribute stack that
/// cannot descend to the path — answers `false`, i.e. "do not dismiss". See
/// [`is_false_modification`] for why that is the only safe direction.
fn routed_through_lfs(
    repo: &gix::Repository,
    index: &gix::index::State,
    key: &str,
    mode: gix::index::entry::Mode,
) -> bool {
    let source = gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping;
    let Ok(mut stack) = repo.attributes_only(index, source) else {
        return false;
    };
    // Only `filter` is collected, so the outcome holds one slot rather than
    // one per attribute the repository happens to define.
    let mut outcome = stack.selected_attribute_matches(["filter"]);
    let Ok(platform) = stack.at_entry(gix::bstr::BStr::new(key.as_bytes()), Some(mode)) else {
        return false;
    };
    // The returned bool only says whether some pattern matched at all, which
    // is not the question: an explicitly unset or unspecified `filter` is a
    // perfectly good "not routed". The resolved state is what decides.
    platform.matching_attributes(&mut outcome);
    // Bound to a local rather than returned as the block's tail expression:
    // `iter_selected` borrows `outcome`, and a tail temporary is dropped AFTER
    // the block's locals, so returning it directly outlives what it reads.
    let routed = outcome.iter_selected().any(|attribute| {
        attribute.assignment.state.as_bstr() == Some(gix::bstr::BStr::new(LFS_FILTER_DRIVER))
    });
    routed
}

/// Does `HEAD` already record `blob` for `rela`?
///
/// An unborn branch, an unreadable tree and a path the commit does not carry
/// all answer no, which is the safe direction: the path is staged rather than
/// dismissed.
fn head_records(repo: &gix::Repository, rela: &Path, blob: gix::hash::ObjectId) -> bool {
    repo.head_tree().is_ok_and(|tree| {
        tree.lookup_entry_by_path(rela)
            .ok()
            .flatten()
            .is_some_and(|entry| entry.object_id() == blob)
    })
}

/// Prepare LFS staging for a set of candidate paths.
///
/// `candidates` are repository-relative paths already cleared by the
/// completeness gate. Paths under the threshold are left alone entirely.
pub fn prepare(
    profile: &SyncProfile,
    store: &LfsStore,
    candidates: &[PathBuf],
) -> Result<LfsStaging> {
    let mut staging = LfsStaging::default();
    if profile.lfs_mode == LfsMode::Disabled {
        return Ok(staging);
    }
    // Built before the walk so a malformed opt-out glob fails the run outright
    // rather than after an arbitrary number of files have already been routed.
    let policy = LfsPolicy::from_profile(profile)?;
    store.ensure_layout()?;

    let mut patterns: Vec<String> = Vec::new();
    for rela in candidates {
        let absolute = profile.local_path.join(rela);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            // Vanished since the gate cleared it: an ordinary outcome.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("stat LFS candidate", absolute, err)),
        };
        // A symlink's blob is its target; its size is meaningless here.
        if !metadata.is_file() || !policy.applies(rela, metadata.len()) {
            continue;
        }

        let pointer = clean(store, &absolute)?;
        staging
            .substitutions
            .insert(rela.clone(), pointer.render().into_bytes());
        staging.uploads.push(StagedObject {
            path: rela.clone(),
            oid: pointer.oid.clone(),
            size: pointer.size,
        });
        let pattern = pattern_for(rela);
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }

    if !patterns.is_empty() {
        staging.attributes_changed = ensure_attributes(&profile.local_path, &patterns)?;
    }
    Ok(staging)
}

/// A worktree path still holding a pointer instead of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSmudge {
    pub path: PathBuf,
    pub pointer: Pointer,
}

/// Find every checked-out path whose worktree content is still a pointer.
///
/// After a fetch-and-apply the worktree receives whatever the blob holds — for
/// an LFS path that is the ~130-byte pointer, not the file. Materializing it is
/// the smudge direction, and it is the reason a peer can clone a profile and
/// get real bytes rather than text stubs.
///
/// Cheap to call: a candidate must be under 1 KiB before it is even read, so a
/// tree of ordinary files costs one `stat` each.
pub fn pending_smudges(root: &Path, tracked: &[PathBuf]) -> Result<Vec<PendingSmudge>> {
    let mut out = Vec::new();
    for rela in tracked {
        let absolute = root.join(rela);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("stat smudge candidate", absolute, err)),
        };
        if !metadata.is_file() || metadata.len() as usize > pointer::MAX_POINTER_BYTES {
            continue;
        }
        let bytes = match std::fs::read(&absolute) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("read smudge candidate", absolute, err)),
        };
        if !pointer::is_pointer_candidate(&bytes) {
            continue;
        }
        if let Some(parsed) = Pointer::parse(&bytes) {
            out.push(PendingSmudge {
                path: rela.clone(),
                pointer: parsed,
            });
        }
    }
    Ok(out)
}

/// Replace a pointer file with the object it names.
///
/// Writes to a sibling temp file and renames, so an interrupted materialization
/// leaves the pointer intact rather than a truncated video: the operation is
/// then simply retried. The staging name carries keeper's own `.keeper.*.tmp`
/// prefix, which tier 0 already excludes, so the watcher cannot mistake it for
/// user content.
pub fn materialize(store: &LfsStore, root: &Path, smudge: &PendingSmudge) -> Result<()> {
    let oid = &smudge.pointer.oid;
    if !store.contains(oid, smudge.pointer.size) {
        return Err(SyncError::Integrity {
            subject: format!("lfs object {oid}"),
            expected: format!("{} bytes in the local store", smudge.pointer.size),
            actual: "absent".to_owned(),
        });
    }
    let target = root.join(&smudge.path);
    let parent = target.parent().unwrap_or(root);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "object".to_owned());
    let staging = parent.join(format!(".keeper.{name}.tmp"));

    std::fs::copy(store.object_path(oid), &staging)
        .map_err(|err| SyncError::io("stage lfs object", staging.clone(), err))?;
    std::fs::rename(&staging, &target)
        .map_err(|err| SyncError::io("publish lfs object", target.clone(), err))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(root: &Path) -> SyncProfile {
        let mut p = SyncProfile::new("01J", "p", root, "https://git.invalid/r.git");
        p.lfs_threshold_bytes = 1024;
        p
    }

    #[test]
    fn only_files_at_or_above_the_threshold_are_tracked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        assert!(!applies(&p, 1023));
        assert!(applies(&p, 1024), "the threshold is inclusive");
        assert!(applies(&p, 10_000));
        // Disabling LFS must disable it completely, not merely raise the bar.
        p.lfs_mode = LfsMode::Disabled;
        assert!(!applies(&p, u64::MAX));
    }

    #[test]
    fn an_opt_out_glob_beats_the_size_rule() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        p.lfs_never = vec!["*.md".into()];
        let policy = LfsPolicy::from_profile(&p).expect("policy");

        // This is the case the field exists for: the recorded .gitattributes
        // rule is per-extension, so without the opt-out one oversized note
        // converts *.md for the whole repository and every note stops being
        // diffable.
        assert!(!policy.applies(Path::new("10-notes/huge.md"), 10_000));
        assert!(policy.applies(Path::new("40-photos/shot.jpg"), 10_000));
        // The opt-out is not a way to disable the threshold.
        assert!(!policy.applies(Path::new("10-notes/small.md"), 10));
    }

    #[test]
    fn opt_out_globs_match_the_repository_relative_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        p.lfs_never = vec!["10-notes/**".into()];
        let policy = LfsPolicy::from_profile(&p).expect("policy");

        // Anchored at the repository root, so the rule does not depend on where
        // the folder happens to be mounted, and a same-named folder elsewhere
        // is unaffected.
        assert!(!policy.applies(Path::new("10-notes/a/b.md"), 10_000));
        assert!(policy.applies(Path::new("30-work/10-notes/a.md"), 10_000));
    }

    #[test]
    fn a_malformed_opt_out_glob_is_refused_rather_than_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        p.lfs_never = vec!["[unclosed".into()];
        // Silently dropping it would leave the user believing a format is
        // protected while every oversized file quietly converts its extension.
        let err = LfsPolicy::from_profile(&p).expect_err("must refuse");
        assert!(format!("{err}").contains("lfsNever"), "got: {err}");
    }

    #[test]
    fn no_opt_out_configured_behaves_exactly_like_the_size_rule() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = profile(dir.path());
        let policy = LfsPolicy::from_profile(&p).expect("policy");
        for size in [0_u64, 1023, 1024, 10_000] {
            assert_eq!(
                policy.applies(Path::new("any/file.bin"), size),
                applies(&p, size),
                "size {size} diverged from the plain threshold rule",
            );
        }
    }

    #[test]
    fn patterns_prefer_the_extension_so_siblings_are_covered() {
        assert_eq!(pattern_for(Path::new("media/clip.mp4")), "*.mp4");
        assert_eq!(pattern_for(Path::new("a/b/archive.tar.gz")), "*.gz");
        // No extension: anchor an exact path at the repository root.
        assert_eq!(pattern_for(Path::new("bin/blob")), "/bin/blob");
    }

    /// The regression the story exists for: every pattern that can hold a
    /// blank, and the raw spelling each one is expected to carry.
    ///
    /// `pattern_for` returns the pattern, never the line, so nothing here is
    /// quoted — quoting belongs to `quote_pattern` and is asserted separately.
    #[test]
    fn a_pattern_can_hold_a_blank_from_either_branch() {
        // Exact-path branch: no extension at all.
        assert_eq!(
            pattern_for(Path::new("archive/my holiday")),
            "/archive/my holiday"
        );
        // Extension branch: `Path::extension` splits at the LAST dot, so a
        // version-numbered name puts the blank inside the "extension".
        assert_eq!(
            pattern_for(Path::new("v1.2 final draft")),
            "*.2 final draft"
        );
        // A blank in a parent directory reaches the exact-path branch too.
        assert_eq!(pattern_for(Path::new("my media/blob")), "/my media/blob");
    }

    /// A pattern that does not need quotes is emitted byte for byte.
    ///
    /// This is the compatibility half of the fix. Quoting unconditionally
    /// would be legal git and would rewrite every line in every user's
    /// `.gitattributes` the first time keeper touched it, so "unchanged" is a
    /// requirement rather than an optimisation — including for a UTF-8 name,
    /// which git reads bare and which octal escaping would churn.
    #[test]
    fn a_pattern_needing_no_quotes_is_emitted_byte_identically() {
        for pattern in [
            "*.mp4",
            "*.tar.gz",
            "/bin/blob",
            "/archive/café.mp4",
            "*.mp4-",
            "/a!b",
            "/a#b",
            "/a*b",
        ] {
            assert_eq!(quote_pattern(pattern), pattern, "{pattern} was rewritten");
        }
    }

    /// Quoting is applied exactly when a bare spelling could not be read back.
    #[test]
    fn quoting_appears_only_for_a_pattern_that_needs_it() {
        assert_eq!(quote_pattern("/a b"), "\"/a b\"");
        assert_eq!(quote_pattern("*.2 final draft"), "\"*.2 final draft\"");
        assert_eq!(quote_pattern("/a\tb"), "\"/a\\tb\"");
        assert_eq!(quote_pattern("/a\"b"), "\"/a\\\"b\"");
        assert_eq!(quote_pattern("/a\\b"), "\"/a\\\\b\"");
        // A leading `#` is read as a comment before anything else is read.
        assert_eq!(quote_pattern("#notes"), "\"#notes\"");
        // An empty pattern would vanish into the separator.
        assert_eq!(quote_pattern(""), "\"\"");
        // Octal for the controls with no letter escape.
        assert_eq!(quote_pattern("/a\u{1}b"), "\"/a\\001b\"");
        assert_eq!(quote_pattern("/a\u{7f}b"), "\"/a\\177b\"");
    }

    /// `quote_pattern` is the inverse of the decoder git and gitoxide use.
    ///
    /// `gix-quote` ships `undo` and no encoder, so the encoder here is written
    /// by hand against it. That is only safe while the pair actually composes,
    /// and `gix-attributes` resolves keeper's own lines with the same `undo` —
    /// so a divergence would not be a test failure, it would be
    /// `routed_through_lfs` quietly failing to see a rule keeper wrote.
    #[test]
    fn quoting_is_the_exact_inverse_of_the_decoder_git_and_gitoxide_use() {
        for pattern in [
            "*.mp4",
            "/bin/blob",
            "/archive/café.mp4",
            "/my holiday",
            "*.2 final draft",
            "/a\tb",
            "/a\"b",
            "/a\\b",
            "/a\nb",
            "/a\rb",
            "/a\u{7}\u{8}\u{b}\u{c}b",
            "/a\u{0}\u{1f}\u{7f}b",
            "#notes",
            "",
            "/trailing space ",
        ] {
            let quoted = quote_pattern(pattern);
            let (undone, consumed) = gix_quote::ansi_c::undo(gix::bstr::BStr::new(
                quoted.as_bytes(),
            ))
            .unwrap_or_else(|err| panic!("{pattern:?} -> {quoted:?} did not decode: {err}"));
            let undone: &[u8] = &undone;
            assert_eq!(
                undone,
                pattern.as_bytes(),
                "{pattern:?} round-tripped through {quoted:?} as {:?}",
                String::from_utf8_lossy(undone)
            );
            assert_eq!(
                consumed,
                quoted.len(),
                "{quoted:?} must be consumed whole, so the attributes start after it"
            );
        }
    }

    /// The reader half, asked directly.
    ///
    /// A quote-only fix would have been worse than the bug: keeper's own
    /// coverage check tokenised on blanks, so a quoted line never matched and
    /// the rule was re-appended on every run.
    #[test]
    fn coverage_is_recognised_in_both_the_quoted_and_the_bare_spelling() {
        // The line keeper writes today for a spaced pattern.
        assert!(attribute_pattern_matches(
            "\"/my holiday\" filter=lfs diff=lfs merge=lfs -text",
            "/my holiday"
        ));
        // A bare line — an older keeper, `git lfs track`, or the user.
        assert!(attribute_pattern_matches(
            "*.mp4 filter=lfs diff=lfs merge=lfs -text",
            "*.mp4"
        ));
        // A quoted line for a pattern that needs no quotes is still coverage.
        assert!(attribute_pattern_matches("\"*.mp4\" filter=lfs", "*.mp4"));
        // A pattern with no attributes at all is still the same pattern.
        assert!(attribute_pattern_matches("\"/my holiday\"", "/my holiday"));

        // Discrimination, not a blanket yes.
        assert!(!attribute_pattern_matches(
            "\"/my other\" filter=lfs",
            "/my holiday"
        ));
        assert!(!attribute_pattern_matches("*.iso filter=lfs", "*.mp4"));
        // The broken line already in the owner's file: the pattern it names is
        // `/2021` plus stray attributes, and it is NOT coverage for the path.
        assert!(!attribute_pattern_matches(
            "/2021 holiday/clip filter=lfs",
            "/2021 holiday/clip"
        ));
        // Malformed quoting, and a field that does not end at its closing
        // quote, are both read as absent — costing a rule, never hiding one.
        assert!(!attribute_pattern_matches(
            "\"/unterminated filter=lfs",
            "/unterminated"
        ));
        assert!(!attribute_pattern_matches("\"*.mp4\"x filter=lfs", "*.mp4"));
    }

    /// FR-137's one write per session, for the path that used to defeat it.
    ///
    /// Fifty-nine copies of one rule is what the old reader produced: the line
    /// it had just written tokenised to something no pattern equals, so every
    /// commit that re-staged the path appended it again. The second call here
    /// is the whole test.
    #[test]
    fn a_spaced_pattern_is_written_once_and_recognised_on_the_next_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let pattern = pattern_for(Path::new("2021 holiday/clip"));

        assert!(ensure_attributes(root, std::slice::from_ref(&pattern)).expect("first"));
        let after_first = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert!(
            after_first.contains("\"/2021 holiday/clip\" filter=lfs diff=lfs merge=lfs -text"),
            "got:\n{after_first}"
        );
        assert_eq!(
            after_first.matches("filter=lfs").count(),
            1,
            "one path is one rule:\n{after_first}"
        );

        assert!(
            !ensure_attributes(root, std::slice::from_ref(&pattern)).expect("second"),
            "the rule keeper just wrote must read as already present"
        );
        assert_eq!(
            after_first,
            std::fs::read_to_string(root.join(".gitattributes")).expect("read"),
            "a second run must not touch the file"
        );
    }

    /// The bare block a pre-46.1 keeper wrote is still recognised as coverage.
    ///
    /// The fix must not orphan the rules already in the field: a space-free
    /// pattern is written bare, so its old line and its new line are the same
    /// bytes, and re-running against a file written by the previous version
    /// has to change nothing at all.
    #[test]
    fn a_file_written_by_the_previous_version_is_left_exactly_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let legacy =
            format!("{MANAGED_HEADER}\n*.mp4 {ATTRIBUTE_SUFFIX}\n*.iso {ATTRIBUTE_SUFFIX}\n");
        std::fs::write(root.join(".gitattributes"), &legacy).expect("seed");

        assert!(
            !ensure_attributes(root, &["*.mp4".into(), "*.iso".into()]).expect("rerun"),
            "nothing changed, so nothing may be written"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".gitattributes")).expect("read"),
            legacy
        );
    }

    #[test]
    fn attributes_are_written_once_and_are_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        assert!(ensure_attributes(root, &["*.mp4".into()]).expect("first"));
        let after_first = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert!(after_first.contains("*.mp4 filter=lfs diff=lfs merge=lfs -text"));

        assert!(
            !ensure_attributes(root, &["*.mp4".into()]).expect("second"),
            "re-running must not rewrite the file"
        );
        assert_eq!(
            after_first,
            std::fs::read_to_string(root.join(".gitattributes")).expect("read")
        );
    }

    #[test]
    fn a_users_own_rules_are_preserved_and_never_duplicated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join(".gitattributes"),
            "# mine\n*.psd filter=lfs diff=lfs merge=lfs -text\n*.txt text\n",
        )
        .expect("seed");

        assert!(
            !ensure_attributes(root, &["*.psd".into()]).expect("existing coverage"),
            "a rule the user already wrote must be respected, not duplicated"
        );
        assert!(ensure_attributes(root, &["*.mp4".into()]).expect("new rule"));

        let text = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert_eq!(text.matches("*.psd").count(), 1);
        assert!(text.contains("# mine"), "the user's comment survives");
        assert!(text.contains("*.txt text"), "unrelated rules survive");
        assert!(text.contains(MANAGED_HEADER));
    }

    #[test]
    fn a_non_lfs_rule_for_the_same_pattern_does_not_count_as_coverage() {
        // `*.bin binary` is not LFS tracking, so the rule still has to be added.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join(".gitattributes"), "*.bin binary\n").expect("seed");
        assert!(ensure_attributes(root, &["*.bin".into()]).expect("add"));
        let text = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert!(text.contains("*.bin filter=lfs"));
    }

    #[test]
    fn cleaning_streams_the_file_into_the_store_and_returns_its_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = LfsStore::new(root.join("lfs"));
        store.ensure_layout().expect("layout");

        let payload = vec![9u8; 4096];
        let file = root.join("big.bin");
        std::fs::write(&file, &payload).expect("write");

        let pointer = clean(&store, &file).expect("clean");
        assert_eq!(pointer.size, 4096);
        assert!(store.contains(&pointer.oid, pointer.size));

        // The digest must be the real SHA-256 of the content, or the server
        // will reject the object.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        assert_eq!(pointer.oid, hex::encode(hasher.finalize()));
        // The worktree file is untouched — the user still sees their data.
        assert_eq!(std::fs::read(&file).expect("read"), payload);
    }

    #[test]
    fn prepare_routes_only_oversized_files_and_leaves_the_rest_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let p = profile(root);
        let store = LfsStore::new(root.join(".git/lfs"));
        std::fs::write(root.join("small.txt"), vec![1u8; 100]).expect("write");
        std::fs::write(root.join("big.mp4"), vec![2u8; 5000]).expect("write");

        let staging = prepare(
            &p,
            &store,
            &[PathBuf::from("small.txt"), PathBuf::from("big.mp4")],
        )
        .expect("prepare");

        assert_eq!(staging.substitutions.len(), 1);
        assert!(staging.substitutions.contains_key(Path::new("big.mp4")));
        assert_eq!(staging.uploads.len(), 1);
        assert_eq!(staging.uploads[0].size, 5000);
        assert!(staging.attributes_changed);

        // The substituted blob really is a pointer, and a small one.
        let bytes = &staging.substitutions[Path::new("big.mp4")];
        assert!(bytes.len() < pointer::MAX_POINTER_BYTES);
        assert!(Pointer::parse(bytes).is_some());
    }

    #[test]
    fn prepare_does_nothing_at_all_when_lfs_is_disabled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let mut p = profile(root);
        p.lfs_mode = LfsMode::Disabled;
        std::fs::write(root.join("big.mp4"), vec![2u8; 5000]).expect("write");

        let staging = prepare(
            &p,
            &LfsStore::new(root.join(".git/lfs")),
            &[PathBuf::from("big.mp4")],
        )
        .expect("prepare");
        assert!(staging.substitutions.is_empty());
        assert!(staging.uploads.is_empty());
        assert!(!root.join(".gitattributes").exists());
    }
}

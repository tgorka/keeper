//! The agent's write and apply checks over a drive's git checkout.
//!
//! The person's unmanaged writers, trash, and collision check moved to
//! [`keeper_folder::files_write`] and are re-exported here.

pub use keeper_folder::files_write::*;

use std::path::Path;

use keeper_paths::{last_segment, names, WriteRefusal, WriteScope, MAX_NAME_BYTES};

// ─── 109.27: where an agent's write may land ─────────────────────────────────

/// What an agent's write does at a path ([`agent_write_check`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTarget {
    /// Changes the file there — `replace`, `edit`, `append`, `delete`, and a
    /// `move`'s source: a regular file the drive's index tracks.
    Existing,
    /// Makes a file where nothing is — `create`, and a `move`'s destination:
    /// a path that, committed, the drive would track.
    New,
}

/// Folder names an agent's write never passes through, compared as the Mac's
/// volume compares names: git's, keeper's, the sync engine's and Obsidian's
/// own state.
const AGENT_NEVER_FOLDERS: [&str; 4] = [".git", ".keeper", ".keeper-sync", ".obsidian"];

/// Whether `subpath` is git's or keeper's own: a segment in
/// [`AGENT_NEVER_FOLDERS`], or a file git or keeper reads unfiltered
/// (`lfs::stage::CONTROL_FILES`), at any depth, folded as
/// [`names::same_entry_folded`] folds — on the volume keeper ships on
/// `.GitIgnore` is `.gitignore`.
fn is_machinery(subpath: &str) -> bool {
    let segments: Vec<&str> = subpath.split('/').collect();
    let folded =
        |name: &str, list: &[&str]| list.iter().any(|own| names::same_entry_folded(name, own));
    segments
        .iter()
        .any(|segment| folded(segment, &AGENT_NEVER_FOLDERS))
        || segments
            .last()
            .is_some_and(|name| folded(name, &crate::lfs::stage::CONTROL_FILES))
}

/// This machine's local wall clock in milliseconds.
///
/// A freedesktop `DeletionDate` is defined as *local* time carrying no zone, so
/// UTC would be an hour or thirteen wrong in the one field a person reads in
/// their file manager. Taken as an argument by [`trash_unmanaged`] rather than
/// read inside it, so the test can assert an exact string.
pub fn local_now_ms() -> i64 {
    let utc = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(0));
    utc + i64::from(crate::platform::machine_utc_offset_minutes()) * 60_000
}

/// Where an agent's write may land, read-only (109.27, R-HQ-87): the
/// repository-relative key of `subpath` under the checkout at `root`, or the
/// refusal whose sentence the agent and the deciding person are shown.
///
/// **One rule for both sides** (AD-65, AD-159): keeper-hq's runtime asks it
/// over its read-only mount when the call is made, with
/// `WriteScope::new(drive, None)` and no extra exclusions; the deciding
/// person's keeper asks it again over its own checkout right before it
/// commits, with the agent host's scope (sessions closed, agents fenced) and
/// the profile's exclusions. Nothing here writes, and nothing here composes
/// a path the caller did not send: the subpath goes through
/// [`keeper_paths::plain_segments`] verbatim, and the key returned is that subpath.
///
/// In the order that produces the most useful sentence:
///
/// 1. a plain descendant ([`keeper_paths::plain_segments`]), not the root, each
///    name at most [`MAX_NAME_BYTES`];
/// 2. never git's or keeper's own ([`WriteRefusal::Machinery`]);
/// 3. the scope's fences ([`WriteScope::fenced`]);
/// 4. not left out by the profile's exclusions ([`WriteRefusal::Ignored`]);
/// 5. the index, its names compared as the Mac's volume compares them
///    ([`names::same_entry_folded`]): no tracked file or link where a folder
///    of the path would be, even one gone from this disk;
///    [`AgentTarget::Existing`]: a regular file the index tracks at exactly
///    this path ([`WriteRefusal::Missing`] otherwise, whatever this disk
///    holds there); [`AgentTarget::New`]: nothing tracked at it, under it, or
///    at a spelling the Mac would take for it ([`WriteRefusal::Exists`]);
/// 6. not left out by `.gitignore` or the profile's exclusions, asked again
///    over every spelling of the path the drive holds — on this disk and in
///    the index, names compared as at 5 ([`spellings`]) — since neither
///    rule folds a name as the Mac's volume does ([`WriteRefusal::Ignored`]);
/// 7. on the disk, no link at or on the way to it ([`WriteRefusal::Link`]),
///    no file where a folder of the path should be; an existing target is a
///    regular file there, and a new one finds nothing there in any spelling;
/// 8. not stored through LFS: no pointer in the index or on the disk, and no
///    `filter=lfs` from `.gitattributes` ([`WriteRefusal::LfsRouted`]).
///
/// **Only rules and tracked files decide a sentence before the disk is
/// walked; the disk's names only add spellings for the rules.** An ignored
/// path is refused as ignored, and an existing target the
/// index does not track as [`WriteRefusal::Missing`], whether or not this
/// disk holds a file there, so no refusal tells an agent what a person keeps
/// beside the drive. Two answers cannot be kept apart: a new path's
/// [`WriteRefusal::Exists`] where an untracked file already is (allowing it
/// would write over that file), and [`WriteRefusal::Ignored`] for a path
/// spelled otherwise than an ignored folder this disk holds (allowing it
/// would write into that folder).
///
/// Fails closed: an index, an indexed blob, an ignore rule or an attribute
/// stack that cannot be read is [`WriteRefusal::Undecidable`], never a yes —
/// unlike the LFS repair sweep, whose "cannot tell" leaves a file alone.
///
/// What it answered holds when it answered: a carrier of the write asks
/// [`agent_apply_check`] again, with the bytes it is about to commit, right
/// before it commits them.
pub fn agent_write_check(
    root: &Path,
    scope: &WriteScope<'_>,
    excludes: &crate::exclude::ExcludeSet,
    subpath: &str,
    target: AgentTarget,
) -> Result<String, WriteRefusal> {
    let segments = keeper_paths::plain_segments(subpath)?;
    let Some(last) = segments.len().checked_sub(1) else {
        // The drive's root is a folder, never one file.
        return Err(WriteRefusal::Escapes {
            subpath: subpath.to_owned(),
        });
    };
    if let Some(name) = subpath.split('/').find(|name| name.len() > MAX_NAME_BYTES) {
        return Err(WriteRefusal::NameTooLong {
            name: name.to_owned(),
            bytes: name.len(),
        });
    }
    let refused = |make: fn(String) -> WriteRefusal| Err(make(subpath.to_owned()));
    if is_machinery(subpath) {
        return refused(|subpath| WriteRefusal::Machinery { subpath });
    }
    scope.fenced(subpath)?;
    let undecidable = |reason: String| WriteRefusal::Undecidable {
        subpath: subpath.to_owned(),
        reason,
    };
    if excludes.is_excluded(Path::new(subpath)) {
        return refused(|subpath| WriteRefusal::Ignored { subpath });
    }

    let repo = crate::git::repo::open_read_only(root, true)
        .map_err(|error| undecidable(error.to_string()))?;
    let index = repo
        .index_or_empty()
        .map_err(|error| undecidable(error.to_string()))?;
    // `plain_segments` took the subpath apart at its `/`s, so these are its
    // segments, spelled as they were sent.
    let wanted: Vec<&str> = subpath.split('/').collect();
    let tracked = Tracked::of(&index, &wanted);
    if let Some((at, mode)) = tracked.over {
        return if mode == gix::index::entry::Mode::SYMLINK {
            refused(|subpath| WriteRefusal::Link { subpath })
        } else {
            Err(WriteRefusal::NameTaken {
                name: wanted[at].to_owned(),
            })
        };
    }
    let entry = index.entry_by_path(gix::bstr::BStr::new(subpath.as_bytes()));
    let folder = || {
        Err(WriteRefusal::IsDirectory {
            name: last_segment(subpath).to_owned(),
        })
    };
    match target {
        AgentTarget::Existing => match entry.map(|entry| entry.mode) {
            Some(gix::index::entry::Mode::SYMLINK) => {
                return refused(|subpath| WriteRefusal::Link { subpath })
            }
            Some(gix::index::entry::Mode::FILE | gix::index::entry::Mode::FILE_EXECUTABLE) => {}
            Some(_) => return refused(|subpath| WriteRefusal::Untracked { subpath }),
            None if tracked.beneath => return folder(),
            // Not one of the drive's files, whatever this disk holds there.
            None => return refused(|subpath| WriteRefusal::Missing { subpath }),
        },
        AgentTarget::New => {
            // The file itself, in any spelling the Mac takes for it.
            if entry.is_some() || tracked.at_otherwise {
                return refused(|subpath| WriteRefusal::Exists { subpath });
            }
            // A folder of the path the index spells otherwise: on the Mac the
            // file lands in that folder, under a key no other device has.
            if let Some(at) = tracked.folder_otherwise {
                return Err(WriteRefusal::NameTaken {
                    name: wanted[at].to_owned(),
                });
            }
            if tracked.beneath {
                return folder();
            }
        }
    }

    // The rules are asked over the path as the drive spells it, not only as
    // the agent did: on the Mac `ſecrets/new.md` lands in the person's
    // ignored `Secrets/`, and neither rule folds a name as APFS does.
    let spellings = spellings(root, &wanted, &tracked.spelled)
        .map_err(|error| undecidable(error.to_string()))?;
    if spellings
        .iter()
        .any(|spelled| excludes.is_excluded(Path::new(spelled)))
    {
        return refused(|subpath| WriteRefusal::Ignored { subpath });
    }
    let mode = entry.map_or(gix::index::entry::Mode::FILE, |entry| entry.mode);
    let mut ignores = repo
        .excludes(&index, None, Default::default())
        .map_err(|error| undecidable(error.to_string()))?;
    for spelled in &spellings {
        let ignored = ignores
            .at_path(Path::new(spelled), Some(mode))
            .map_err(|error| undecidable(error.to_string()))?
            .is_excluded();
        if ignored {
            return refused(|subpath| WriteRefusal::Ignored { subpath });
        }
    }

    // The disk, one name at a time and following no link: what is at the
    // path, if anything.
    let mut absolute = root.to_path_buf();
    let mut found = None;
    // The last name looked at: when nothing is at the path, how many of its
    // folders are on the disk.
    let mut present = 0;
    for (at, name) in segments.iter().enumerate() {
        absolute.push(name);
        present = at;
        match absolute.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(undecidable(error.to_string())),
            Ok(meta) if meta.file_type().is_symlink() => {
                return refused(|subpath| WriteRefusal::Link { subpath })
            }
            Ok(meta) if at < last && !meta.is_dir() => {
                return Err(WriteRefusal::NameTaken {
                    name: name.to_string_lossy().into_owned(),
                })
            }
            Ok(meta) if at == last => found = Some(meta),
            Ok(_) => {}
        }
    }
    match (target, found.as_ref()) {
        (AgentTarget::Existing, None) => {
            return refused(|subpath| WriteRefusal::Missing { subpath })
        }
        (AgentTarget::Existing, Some(meta)) if meta.is_dir() => return folder(),
        (AgentTarget::Existing, Some(meta)) if !meta.is_file() => {
            return refused(|subpath| WriteRefusal::Untracked { subpath })
        }
        (AgentTarget::Existing, Some(_)) => {}
        (AgentTarget::New, Some(_)) => return refused(|subpath| WriteRefusal::Exists { subpath }),
        (AgentTarget::New, None) => {
            // Its folder is there: a name the Mac takes for this one is the
            // same file on the volume keeper ships on.
            let taken_otherwise = match absolute.parent() {
                Some(parent) if present == last => collides(parent, last_segment(subpath))?,
                _ => false,
            };
            if taken_otherwise {
                return refused(|subpath| WriteRefusal::Exists { subpath });
            }
        }
    }

    let pointer_indexed = match entry {
        Some(entry) => {
            crate::lfs::stage::indexed_pointer_checked(&repo, entry.id).map_err(undecidable)?
        }
        None => false,
    };
    let pointer_on_disk = match found.as_ref() {
        Some(meta) => crate::lfs::stage::read_worktree_pointer(&absolute, meta)
            .map_err(|error| undecidable(error.to_string()))?
            .is_some(),
        None => false,
    };
    if pointer_indexed
        || pointer_on_disk
        || crate::lfs::stage::lfs_routed(&repo, &index, subpath, mode).map_err(undecidable)?
    {
        return refused(|subpath| WriteRefusal::LfsRouted { subpath });
    }
    Ok(subpath.to_owned())
}

/// [`agent_write_check`] at the moment of the effect (109.27, contract
/// § 5.1): asked by the device that carries an agent's write out, over its
/// own checkout, right before `Engine::commit_paths`, with `post_image` the
/// exact bytes the commit will hold at `subpath` (`None` for a move's source
/// or a delete).
///
/// Everything [`agent_write_check`] decides is decided again here, LFS
/// routing included, because the drive may have changed since the card was
/// drawn — a `.gitattributes` that now routes the path, a file that now
/// stands in the way. And the bytes themselves must not be an LFS pointer:
/// committed as text, a pointer is a large-file entry no agent may make
/// ([`WriteRefusal::LfsRouted`]).
pub fn agent_apply_check(
    root: &Path,
    scope: &WriteScope<'_>,
    excludes: &crate::exclude::ExcludeSet,
    subpath: &str,
    target: AgentTarget,
    post_image: Option<&[u8]>,
) -> Result<String, WriteRefusal> {
    let key = agent_write_check(root, scope, excludes, subpath, target)?;
    // An empty file is the empty pointer only by the format's own carve-out:
    // it names no object, and git stores it as the empty file it is.
    let pointer =
        |bytes: &[u8]| !bytes.is_empty() && crate::lfs::pointer::Pointer::parse(bytes).is_some();
    if post_image.is_some_and(pointer) {
        return Err(WriteRefusal::LfsRouted {
            subpath: subpath.to_owned(),
        });
    }
    Ok(key)
}

/// What the index tracks at, over and under one path, its names compared as
/// the Mac's volume compares them ([`names::same_entry_folded`]) — the disk
/// may no longer hold any of it, and on the device that carries a write out
/// a tracked path gone from the disk is still one a commit would change.
#[derive(Debug, Default)]
struct Tracked {
    /// A tracked file or link where a folder of the path would be, in any
    /// spelling: that folder's position among the path's names, and the
    /// entry's mode. A commit under it would turn the file into a folder.
    over: Option<(usize, gix::index::entry::Mode)>,
    /// A tracked entry at the path in another spelling.
    at_otherwise: bool,
    /// Tracked entries under the path, in any spelling: it is a folder.
    beneath: bool,
    /// The first folder of the path that the index tracks spelled otherwise.
    folder_otherwise: Option<usize>,
    /// The path as a tracked entry spells it where it spells any of the
    /// path's names otherwise: the entry's own names as far as they match,
    /// the path's after that.
    spelled: std::collections::BTreeSet<String>,
}

impl Tracked {
    /// One pass over the index; no allocation for an ASCII entry.
    fn of(index: &gix::index::State, wanted: &[&str]) -> Self {
        use gix::bstr::ByteSlice;
        let mut tracked = Self::default();
        for entry in index.entries() {
            let path = entry.path(index).to_str_lossy();
            let mut parts = path.split('/');
            // How many of the entry's names matched the path's, and where the
            // first of them was spelled otherwise.
            let mut depth = 0;
            let mut otherwise = None;
            loop {
                let part = parts.next();
                if let (Some(_), Some(at)) = (part, otherwise) {
                    // The entry goes on past a name spelled otherwise, so that
                    // name is a folder; it is one of the path's folders unless
                    // it was the path's own last name.
                    if at < wanted.len() - 1 {
                        tracked.folder_otherwise = Some(
                            tracked
                                .folder_otherwise
                                .map_or(at, |first: usize| first.min(at)),
                        );
                    }
                }
                match (part, wanted.get(depth)) {
                    (None, _) => {
                        if entry.mode == gix::index::entry::Mode::DIR {
                            // A sparse folder: the path is in it or is it.
                            tracked.beneath |= depth == wanted.len();
                        } else if depth < wanted.len() {
                            tracked.over.get_or_insert((depth - 1, entry.mode));
                        } else if otherwise.is_some() {
                            tracked.at_otherwise = true;
                        }
                        break;
                    }
                    (Some(_), None) => {
                        tracked.beneath = true;
                        break;
                    }
                    (Some(part), Some(name)) => {
                        if part != *name {
                            if !names::same_entry_folded(part, name) {
                                break;
                            }
                            otherwise.get_or_insert(depth);
                        }
                        depth += 1;
                    }
                }
            }
            if otherwise.is_some() {
                let spelled: Vec<&str> = path
                    .split('/')
                    .take(depth)
                    .chain(wanted[depth..].iter().copied())
                    .collect();
                tracked.spelled.insert(spelled.join("/"));
            }
        }
        tracked
    }
}

/// Every spelling of `wanted` the drive already holds, its names compared as
/// the Mac's volume compares them ([`names::same_entry_folded`]), and the
/// path as it was sent: what `.gitignore` and the profile's exclusions are
/// asked over (R603-10). Neither rule folds a name as APFS does — gix's
/// `core.ignorecase` is ASCII-only and an exclusion glob matches case — and
/// on the Mac `ſecrets/new.md`, `Cafe\u{301}/new.md` and `private/new.md`
/// land in the folders `Secrets/`, `Café/` and `Private/` hold. So each
/// existing name is asked as it is spelled where it exists: on this disk,
/// one folder at a time and following no link, and in the index (`tracked`,
/// from [`Tracked::of`]). The names after the last that exists are the
/// path's own; a new name's collision with a name beside it is the disk
/// walk's question, not this one's.
///
/// Fails closed: a folder that cannot be listed is an error, never a
/// spelling left out.
fn spellings(
    root: &Path,
    wanted: &[&str],
    tracked: &std::collections::BTreeSet<String>,
) -> std::io::Result<std::collections::BTreeSet<String>> {
    let mut spellings = tracked.clone();
    spellings.insert(wanted.join("/"));
    // The folders this disk holds for the path's first names, as it spells them.
    let mut folders = vec![String::new()];
    for (at, name) in wanted.iter().enumerate() {
        let mut deeper = Vec::new();
        for folder in &folders {
            let entries = match std::fs::read_dir(root.join(folder)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                entries => entries?,
            };
            for entry in entries {
                let entry = entry?;
                let file_name = entry.file_name();
                // A name that is not UTF-8 is no spelling of a plain segment.
                let Some(spelled) = file_name.to_str() else {
                    continue;
                };
                if !names::same_entry_folded(spelled, name) {
                    continue;
                }
                let prefix = if folder.is_empty() {
                    spelled.to_owned()
                } else {
                    format!("{folder}/{spelled}")
                };
                let mut whole = vec![prefix.as_str()];
                whole.extend_from_slice(&wanted[at + 1..]);
                spellings.insert(whole.join("/"));
                if at + 1 < wanted.len() && entry.file_type()?.is_dir() {
                    deeper.push(prefix);
                }
            }
        }
        folders = deeper;
    }
    Ok(spellings)
}

#[cfg(test)]
mod agent_tests;

//! Where the Files surface may write, and what it may write there (Story 45.3,
//! FR-175, FR-176, AD-89).
//!
//! # AD-75 was retired here, deliberately and by the owner
//!
//! AD-75 said "the files surface never writes", and [`crate::browse`]'s doc
//! still carries the argument for it: keeper's promise about a synced folder is
//! that it never moves a file you did not ask it to move, and a browser with a
//! delete key in it is the shortest path to breaking that promise by accident.
//! That was the right rule while Files was a window onto the sync engine's
//! world.
//!
//! It is no longer the rule. **AD-89 retires AD-75**: the owner asked the Files
//! surface to delete and create, and the honest answer was to give it a write
//! path rather than to leave the surface read-only and watch a second one grow
//! beside it six months later. What replaces AD-75 is not "the browser may
//! write", it is three narrower promises that this module exists to keep:
//!
//! 1. **Only inside a vault.** keeper writes where it already manages the
//!    files — the notes vault — and nowhere else in the profile (FR-145, AD-65
//!    unaffected: the frontend still never joins a root and a subpath).
//! 2. **One writer.** Every byte goes through `notes_vault::write_vault_file` +
//!    `mark_dirty`, the same path notes and Story 44.16's CSV editor use. This
//!    module writes nothing itself; it decides *whether* and *where*, and hands
//!    a vault-relative path to the one writer.
//! 3. **A removal is announced, not discovered.** Deleting uses
//!    `notes_vault::trash_note`, which moves the bytes into the vault's own
//!    trash (NFR-30, never an `unlink`), tells the reconciler the path changed
//!    and marks the vault dirty so the commit cadence carries the deletion. The
//!    reconciler already understands that shape; inventing a second removal
//!    would mean a deletion the index learns about on the next unrelated scan.
//!
//! # AD-102: a second writer, named rather than grown by accident
//!
//! Story 46.14. The owner asked to edit text files that are not in a notes
//! vault and to delete files that are not notes, and the sentence keeper gave
//! back — *"changing it is your file manager's job"* — was the whole of promise
//! 1 read out loud.
//!
//! **AD-89 is not overturned; it is scoped to what it always described.** The
//! three promises above are the promises keeper makes about a *vault* file, and
//! they have teeth: `mark_dirty` is what makes an edit reach the reconciler and
//! the commit cadence, and `trash_note` is what makes a deletion recoverable.
//! Neither is reachable for a file no vault holds — there is no vault to mark
//! and no vault trash to move bytes into. So the answer is not to relax the
//! guard. It is a **second writer**, with different sync and recovery
//! semantics, said out loud before it acts:
//!
//! * an out-of-vault edit is [`write_unmanaged`] — a plain atomic write, no
//!   `mark_dirty`, no reconciler `touch`, no notes index, no note history and
//!   no conflict copy;
//! * an out-of-vault delete is [`trash_unmanaged`] — the *operating system's*
//!   trash, which on macOS is `NSFileManager trashItem` and elsewhere the
//!   freedesktop.org home trash. NFR-30 holds unchanged: never an `unlink`.
//!
//! **What keeps the two from merging six months from now is that they do not
//! share a signature.** [`WriteScope::route`] is the one place the fork is
//! decided, and it hands back a [`keeper_paths::WriteRoute`] whose vault arm *carries the
//! caller's vault* and whose unmanaged arm has nowhere to put one. The vault
//! writer needs a `Vault` and a [`keeper_paths::VaultPath`]; [`write_unmanaged`] takes an
//! [`UnmanagedPath`], which only `route` can mint and only for a path no vault
//! holds. Not a boolean, and not a comment: the mistake is not expressible.
//!
//! Two things AD-102 does **not** widen:
//!
//! * **A directory is still refused at every location.** "One confirmation over
//!   a folder holding 100 000 files is not a confirmation" (spec-45-3), and
//!   sending that folder to the OS trash rather than the vault's does not make
//!   the confirmation any better informed.
//! * **A file outside every sync profile is still not keeper's.** keeper
//!   addresses a file as (profile id, profile-relative subpath); a file in no
//!   profile has no id to pass, keeper does not list it, and
//!   [`keeper_paths::resolve_existing`] refuses anything whose canonical form leaves the
//!   profile root — a symlink inside the profile pointing out of it included.
//!
//! # Why the decision lives in this crate and not in the shell
//!
//! The same reason [`crate::browse`] does. The shell does not build on Linux,
//! so a containment rule written there is a security rule proved on no machine
//! any of this is developed on — and this one guards a *write*, which is the
//! half where being wrong costs a file rather than a listing. Everything here
//! is pure over strings plus one `read_dir`, so `..`, an escape out of the
//! vault, a colliding name and a name that is not a name are all asserted over
//! a real temp directory on any machine.
//!
//! This module has no opinion about whether a vault is *live*: the caller passes
//! the subfolder of the vault it can actually reach, or `None`. That is what
//! keeps the flag the surface renders and the answer the command gives from
//! being two different questions — a pane told "writable" by config while the
//! command answers from a registry that has no slot for that vault is a pane
//! offering an action that will fail.

use std::path::{Path, PathBuf};

#[cfg(test)]
use keeper_paths::WriteRoute;
use keeper_paths::{last_segment, names, UnmanagedPath, WriteRefusal, WriteScope, MAX_NAME_BYTES};

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

// ─── AD-102: the second writer ───────────────────────────────────────────────

/// Write `content` to a file no vault manages (Story 46.14, AD-102).
///
/// **A plain atomic write and nothing else.** Temp-and-rename in the same
/// directory, under the `.keeper.<ulid>.tmp` name [`crate::exclude`] already
/// makes a tier-0 exclusion — so a `kill -9` between write and rename leaves no
/// torn file, and if the profile is a git repository there is nothing for the
/// next commit to pick up.
///
/// **No `mark_dirty` and no reconciler `touch`, and their absence is the
/// point.** `mark_dirty` marks a *vault*, and this file is in none: there is no
/// reconciler to tell, no notes index this file belongs in, no note history and
/// no conflict copy. Neither call is even reachable from here — this function's
/// signature has no vault in it, by construction rather than by discipline.
///
/// What the file *does* still get, if the profile is a synced git repository,
/// is the folder engine's own watcher and commit cadence — the same treatment a
/// change made in Finder gets, and no more. That distinction is what the
/// surface's caveat sentence has to carry, and why it says "keeper does not
/// manage this file" rather than the flatly untrue "this will not sync".
///
/// Exact bytes: no trailing-newline normalisation and no re-encoding, for the
/// same reason the vault path does not do it either — a file the user did not
/// change must not change.
///
/// The rename is made holding the file's [`Publishing`] lock, so it never
/// lands between a [`replace_unmanaged`]'s final check and that replace's own
/// rename (R412).
pub fn write_unmanaged(target: &UnmanagedPath, content: &str) -> Result<(), WriteRefusal> {
    publish_unmanaged(target, None, content)
}

/// What a file held when a caller read it: the SHA-256 of its bytes, for a
/// write that must not land over a change made after that read
/// ([`replace_unmanaged`], R405).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision([u8; 32]);

impl Revision {
    /// The revision of `bytes`, as read.
    pub fn of(bytes: &[u8]) -> Revision {
        use sha2::{Digest, Sha256};
        Revision(Sha256::digest(bytes).into())
    }
}

/// [`write_unmanaged`] as a compare-and-replace (R405, R412): `content`
/// replaces the file only when the file still holds `read` — the revision
/// the caller's edit was made from — and otherwise nothing is written and
/// the refusal is [`WriteRefusal::ChangedSinceRead`].
///
/// The final read-and-compare and the rename are one step for every writer
/// in keeper's process: both are made holding the file's [`Publishing`]
/// lock, which every unmanaged writer of this module takes. Two saves made
/// from one revision therefore cannot both pass the check — the second
/// waits for the first's rename, then compares against it and is refused.
///
/// A writer outside this process — the sync engine's `git merge` (a child
/// process), an editor — takes no lock of keeper's, and one that writes
/// after the final read and before the rename is written over. That is the
/// accepted residual of R412: the drive is a git repository, so the
/// revision written over stays in its history and the next sync shows it.
pub fn replace_unmanaged(
    target: &UnmanagedPath,
    read: &Revision,
    content: &str,
) -> Result<(), WriteRefusal> {
    publish_unmanaged(target, Some(read), content)
}

/// The one temp-and-rename behind [`write_unmanaged`] and
/// [`replace_unmanaged`]: the bytes are staged first, outside the lock, so
/// what the lock covers is only the comparison (when there is a `read`) and
/// the rename.
fn publish_unmanaged(
    target: &UnmanagedPath,
    read: Option<&Revision>,
    content: &str,
) -> Result<(), WriteRefusal> {
    let failed = |error: std::io::Error| WriteRefusal::WriteFailed {
        relative_path: target.profile_relative().to_owned(),
        reason: error.to_string(),
    };
    // The path resolved to an existing file, so it has a parent directory. The
    // fallback keeps this total rather than asserting it.
    let directory = target.absolute().parent().unwrap_or_else(|| Path::new("."));
    let temp = directory.join(format!(".keeper.{}.tmp", ulid::Ulid::new()));
    std::fs::write(&temp, content.as_bytes()).map_err(failed)?;
    let _held = Publishing::hold(target.absolute());
    if let Some(read) = read {
        let now = match std::fs::read(target.absolute()) {
            Ok(bytes) => Some(Revision::of(&bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                return Err(failed(error));
            }
        };
        if now.as_ref() != Some(read) {
            let _ = std::fs::remove_file(&temp);
            return Err(WriteRefusal::ChangedSinceRead {
                relative_path: target.profile_relative().to_owned(),
            });
        }
    }
    #[cfg(test)]
    seam::fire(target.absolute(), seam::Point::Renaming);
    if let Err(error) = std::fs::rename(&temp, target.absolute()) {
        // A failed rename must not leave the temp behind. It is excluded from
        // sync, but it is still litter in the owner's folder.
        let _ = std::fs::remove_file(&temp);
        return Err(failed(error));
    }
    Ok(())
}

/// One file being published by keeper's unmanaged writers (R412): held from
/// a replace's final check through its rename, and released on drop.
///
/// Process-wide and keyed by the resolved path ([`UnmanagedPath`] is
/// canonical), so every thread of this process — the Settings save, the
/// Files surface, a bot's tool call — waits for the one before it. A set of
/// busy paths rather than a map of mutexes: an entry lives only while a
/// rename is under way, so the set never grows past the writes in flight.
struct Publishing<'a> {
    path: &'a Path,
}

/// The paths being published, and the condition their waiters sleep on.
type Busy = (
    std::sync::Mutex<std::collections::HashSet<PathBuf>>,
    std::sync::Condvar,
);

impl<'a> Publishing<'a> {
    fn busy() -> &'static Busy {
        static BUSY: std::sync::LazyLock<Busy> = std::sync::LazyLock::new(Busy::default);
        &BUSY
    }

    /// Wait until no other writer of this process is publishing `path`,
    /// then hold it.
    fn hold(path: &'a Path) -> Publishing<'a> {
        let (busy, freed) = Self::busy();
        let mut busy = busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        if busy.contains(path) {
            seam::fire(path, seam::Point::Waiting);
        }
        while busy.contains(path) {
            busy = freed
                .wait(busy)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        busy.insert(path.to_path_buf());
        Publishing { path }
    }
}

impl Drop for Publishing<'_> {
    fn drop(&mut self) {
        let (busy, freed) = Self::busy();
        busy.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(self.path);
        freed.notify_all();
    }
}

/// A seam inside [`publish_unmanaged`], for tests only (R412): the race it
/// closes is between two statements, and a thread racing another on a
/// tempdir would be flakier than no test at all. Keyed on the file, so
/// parallel tests, each in its own tempdir, cannot fire one another's hooks.
/// Production builds carry neither the map nor the calls.
#[cfg(test)]
mod seam {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, LazyLock, Mutex, PoisonError};

    /// Where a writer is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Point {
        /// Holding the file, its check passed, about to rename.
        Renaming,
        /// Found the file held by another writer, about to wait for it.
        Waiting,
    }

    type Hook = Arc<dyn Fn(Point) + Send + Sync>;

    static HOOKS: LazyLock<Mutex<HashMap<PathBuf, Hook>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    /// Call `hook` at every point a writer of `path` reaches, until
    /// [`remove`].
    pub(super) fn install(path: &Path, hook: impl Fn(Point) + Send + Sync + 'static) {
        HOOKS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_path_buf(), Arc::new(hook));
    }

    pub(super) fn remove(path: &Path) {
        HOOKS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(path);
    }

    /// Called by the writers. The hook is cloned out of the map before it
    /// runs, so one writer blocked in it does not stop another reaching it.
    pub(super) fn fire(path: &Path, point: Point) {
        let hook = HOOKS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .cloned();
        if let Some(hook) = hook {
            hook(point);
        }
    }
}

/// Where an out-of-vault removal puts the bytes (AD-102, NFR-30).
///
/// Not a boolean and not a bare path, because the two platforms answer the
/// question in genuinely different shapes. macOS names no directory:
/// `NSFileManager` picks the right `.Trashes` for the volume the file is on and
/// records the Put Back location, and second-guessing it is how a file on a
/// pendrive ends up in the boot volume's trash. freedesktop.org names one — and
/// naming it is what lets the whole removal be asserted over a temp directory
/// on the Linux box this is written on, rather than being a promise only macOS
/// could ever check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashTarget {
    /// A freedesktop.org trash directory: `files/` beside `info/`.
    Freedesktop(PathBuf),
    /// Hand the file to `NSFileManager` and let macOS choose.
    Finder,
}

/// This machine's trash, or why it has none.
///
/// Resolved per platform and never guessed: a machine with no home directory
/// gets [`WriteRefusal::NoSystemTrash`] and keeps its file, because the only
/// alternative is the `unlink` NFR-30 forbids.
pub fn os_trash() -> Result<TrashTarget, WriteRefusal> {
    if cfg!(target_os = "macos") {
        return Ok(TrashTarget::Finder);
    }
    // The freedesktop.org "home trash": `$XDG_DATA_HOME/Trash`, defaulting to
    // `$HOME/.local/share/Trash`. Read from the environment rather than from a
    // platform port because this is the only caller and the port would have to
    // be threaded through four Tauri commands to reach it.
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local").join("share"))
        })
        .ok_or_else(|| WriteRefusal::NoSystemTrash {
            reason: "neither XDG_DATA_HOME nor HOME is set".to_owned(),
        })?;
    Ok(TrashTarget::Freedesktop(base.join("Trash")))
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

/// Move a file no vault manages into the operating system's trash (Story 46.14,
/// AD-102, NFR-30).
///
/// **Never an `unlink`, and that promise is not weakened by being outside a
/// vault — only relocated.** `trash_note` moves bytes into `<vault>/.keeper/
/// trash/`, which does not exist here; the OS trash is the same shape of
/// promise made by the machine instead of by keeper, and it is the one the
/// owner already knows how to undo. Returns where the bytes landed, for the log
/// line, exactly as `trash_note` does.
///
/// `local_now_ms` is the local wall clock — see [`local_now_ms`].
pub fn trash_unmanaged(
    target: &UnmanagedPath,
    trash: &TrashTarget,
    local_now_ms: i64,
) -> Result<PathBuf, WriteRefusal> {
    let failed = |reason: String| WriteRefusal::DeleteFailed {
        relative_path: target.profile_relative().to_owned(),
        reason,
    };
    match trash {
        TrashTarget::Freedesktop(root) => freedesktop_trash(target.absolute(), root, local_now_ms)
            .map_err(|error| failed(error.to_string())),
        TrashTarget::Finder => finder_trash(target.absolute()).map_err(failed),
    }
}

/// The freedesktop.org trash-spec removal: claim a name in `info/`, then move
/// the bytes into `files/`.
///
/// **The `.trashinfo` file is written first, with `create_new`, and that is the
/// lock.** The spec says so and the reason is a race this code can actually
/// lose: two deletions of two files called `notes.md` from two folders, or one
/// deletion racing the desktop's own. Creating the info file exclusively is
/// what makes the name mine before any byte moves, so the loser picks the next
/// name instead of overwriting the winner's file.
///
/// A move that fails takes the info file back out with it: a `.trashinfo`
/// naming bytes that are not in `files/` is a broken row in the owner's trash.
fn freedesktop_trash(path: &Path, root: &Path, local_now_ms: i64) -> std::io::Result<PathBuf> {
    use std::io::Write as _;

    let files = root.join("files");
    let info = root.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;

    let original = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let mut claimed = None;
    for attempt in 0..1_000u32 {
        let candidate = numbered(original, attempt);
        let ticket = info.join(format!("{candidate}.trashinfo"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ticket)
        {
            Ok(handle) => {
                claimed = Some((candidate, ticket, handle));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let Some((name, ticket, mut handle)) = claimed else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("the trash already holds a thousand files called {original}"),
        ));
    };

    let record = write!(
        handle,
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encoded_path(path),
        deletion_date(local_now_ms)
    )
    .and_then(|()| handle.sync_all());
    if let Err(error) = record {
        let _ = std::fs::remove_file(&ticket);
        return Err(error);
    }

    let grave = files.join(&name);
    let moved = std::fs::rename(path, &grave).or_else(|_| {
        // The home trash is very often on another filesystem from the folder
        // being synced — an external drive, a network share — so a failed
        // rename here is ordinary rather than exceptional. Copy-then-remove
        // keeps the promise either way: the bytes exist in the trash before
        // the original stops existing.
        std::fs::copy(path, &grave).and_then(|_| std::fs::remove_file(path))
    });
    if let Err(error) = moved {
        let _ = std::fs::remove_file(&grave);
        let _ = std::fs::remove_file(&ticket);
        return Err(error);
    }
    Ok(grave)
}

/// `NSFileManager trashItemAtURL:` — the macOS trash, with Put Back intact.
///
/// A **safe** objc2 binding, so no `unsafe` block and nothing to add to the
/// audited-FFI inventory: `trashItemAtURL:resultingItemURL:error:` returns its
/// failure as an `NSError`, which is the whole reason to prefer it over
/// reimplementing `.Trashes` by hand.
///
/// This is the one part of this module that cannot be exercised on Linux, which
/// is why it is ten lines with no decisions in it — every decision that reaches
/// it was already made and asserted in [`WriteScope::route`].
#[cfg(target_os = "macos")]
fn finder_trash(path: &Path) -> Result<PathBuf, String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let text = path
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))?;
    // `is_directory: false` rather than a stat: `route` refuses every directory
    // before this is reachable.
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(text), false);
    let mut landed = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
        .map_err(|error| error.to_string())?;
    Ok(landed.and_then(|url| url.path()).map_or_else(
        || path.to_path_buf(),
        |grave| PathBuf::from(grave.to_string()),
    ))
}

/// Unreachable off macOS — [`os_trash`] only ever answers
/// [`TrashTarget::Finder`] there. A refusal rather than a `panic!` or an
/// `unlink`, because an enum arm that cannot happen is exactly the one that
/// happens after somebody constructs the variant by hand in a test.
#[cfg(not(target_os = "macos"))]
fn finder_trash(path: &Path) -> Result<PathBuf, String> {
    Err(format!(
        "{} would go to the macOS Trash, and this is not macOS",
        path.display()
    ))
}

/// `name`, then `name.2`, `name.3`, … keeping the extension where a file
/// manager expects to find it.
///
/// The suffix goes before the extension so the trashed copy of `report.md` is
/// still a Markdown file to everything that reads it, which `report.md.2` would
/// not be.
fn numbered(name: &str, attempt: u32) -> String {
    if attempt == 0 {
        return name.to_owned();
    }
    let ordinal = attempt + 1;
    match name.rsplit_once('.') {
        // A leading dot is the whole name of a hidden file, not an extension.
        Some((stem, extension)) if !stem.is_empty() => format!("{stem}.{ordinal}.{extension}"),
        _ => format!("{name}.{ordinal}"),
    }
}

/// The absolute path as the trash spec wants it: percent-encoded, `/` left
/// alone.
///
/// Through `url`, already a dependency of this crate, rather than a hand-rolled
/// encoder — the set of characters that must be escaped is exactly the one a
/// hand-rolled encoder gets subtly wrong, and getting it wrong means a Restore
/// that puts the file back in the wrong place.
fn encoded_path(path: &Path) -> String {
    url::Url::from_file_path(path).map_or_else(
        |()| path.to_string_lossy().into_owned(),
        |url| url.path().to_owned(),
    )
}

/// `YYYY-MM-DDThh:mm:ss` local, which is what a `.trashinfo` `DeletionDate` is.
fn deletion_date(local_now_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = keeper_paths::civil_from_unix_ms(local_now_ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Whether `name` is already taken in `directory`, matching as the Mac's
/// volume matches names ([`names::same_entry_folded`]): case-insensitively and
/// whatever the Unicode normalization.
///
/// **Case-insensitively, and that is the load-bearing part.** APFS and NTFS are
/// case-insensitive by default, so `README.md` created beside an existing
/// `readme.md` does not create a second file there — it replaces the first one,
/// silently, with an empty one. An exact-match check would pass on the Linux box
/// this is written on and lose a file on the Mac it ships to. The same rule
/// `notes_vault::unique_name` already applies when it picks `shot-2.png`.
///
/// A directory that cannot be read is a refusal and never a "no collision": the
/// question was "may I write here", and "I could not look" is not a yes.
pub fn collides(directory: &Path, name: &str) -> Result<bool, WriteRefusal> {
    let entries = std::fs::read_dir(directory).map_err(|error| WriteRefusal::Unreadable {
        reason: error.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| WriteRefusal::Unreadable {
            reason: error.to_string(),
        })?;
        if names::same_entry_folded(&entry.file_name().to_string_lossy(), name) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> WriteScope<'static> {
        WriteScope::new("Vault", Some("10-notes"))
    }

    /// A collision is case-insensitive, because the filesystem keeper ships to
    /// is. An exact-match check passes here and loses a file on macOS.
    #[test]
    fn a_collision_is_found_whatever_case_it_was_written_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("readme.md"), b"kept").expect("write");
        std::fs::create_dir(dir.path().join("Daily")).expect("mkdir");

        assert_eq!(collides(dir.path(), "readme.md"), Ok(true));
        assert_eq!(collides(dir.path(), "README.md"), Ok(true));
        assert_eq!(collides(dir.path(), "ReadMe.MD"), Ok(true));
        // A folder of that name is a collision too: creating the file would
        // fail, and "already there" is the honest reason.
        assert_eq!(collides(dir.path(), "daily"), Ok(true));
        assert_eq!(collides(dir.path(), "notes.md"), Ok(false));
    }

    /// R603-08: a reserved name and a collision are found under every spelling
    /// APFS folds to one entry, not only the ones lowercasing reaches: on
    /// hesperia `.obſidian` (U+017F long s) is the `.obsidian` folder, `ſessions`
    /// is `sessions`, `STRASSE.md` is `straße.md` and `ς.md` is `Σ.md`.
    #[test]
    fn reserved_names_and_collisions_fold_as_apfs_does() {
        let scope = scope();
        for name in [".ob\u{17f}idian", ".OB\u{17f}IDIAN", ".\u{212A}eeper"] {
            assert_eq!(
                scope.create("10-notes", name),
                Err(WriteRefusal::NameReserved {
                    name: name.to_owned()
                }),
                "{name}"
            );
        }

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("stra\u{df}e.md"), b"kept").expect("write");
        std::fs::write(dir.path().join("ss.md"), b"kept").expect("write");
        std::fs::write(dir.path().join("\u{3a3}.md"), b"kept").expect("write");
        std::fs::create_dir(dir.path().join("sessions")).expect("mkdir");
        for taken in [
            "STRASSE.md",
            "strasse.md",
            "\u{df}.md",
            "\u{1e9e}.md",
            "\u{3c2}.md",
            "\u{17f}essions",
            "\u{17f}e\u{17f}\u{17f}ions",
        ] {
            assert_eq!(collides(dir.path(), taken), Ok(true), "{taken}");
        }
        // What APFS keeps apart stays apart: a dotless `ı` is not `i`, and a
        // zero-width joiner is a character of the name.
        for free in ["se\u{17f}\u{17f}\u{131}ons", "s\u{200d}s.md"] {
            assert_eq!(collides(dir.path(), free), Ok(false), "{free}");
        }
    }

    /// A folder that cannot be read is a refusal, never a cleared collision
    /// check. This is the shape of the epic-44 defect where a create could
    /// overwrite through a directory nobody could list.
    #[test]
    fn a_directory_that_cannot_be_read_refuses_rather_than_reporting_no_collision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("gone");
        let refusal = collides(&missing, "x.md").expect_err("unreadable");
        assert!(matches!(refusal, WriteRefusal::Unreadable { .. }));
        assert!(
            refusal.to_string().contains("cannot tell whether the name"),
            "{refusal}"
        );
    }

    // ─── AD-102: the second writer (Story 46.14) ─────────────────────────

    /// Stands in for the shell's `notes_vault::Vault`.
    ///
    /// A distinct type rather than `()`, so these tests instantiate the same
    /// generic the shell does and a route that dropped the vault on the floor
    /// would not typecheck here either.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakeVault(&'static str);

    /// A profile root with a vault at `10-notes`, a neighbour whose name
    /// extends it, and a file at the top that no vault holds — the owner's
    /// `AGENTS.md`.
    fn profile() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("10-notes/daily")).expect("vault");
        std::fs::create_dir_all(root.path().join("10-notes-archive")).expect("neighbour");
        std::fs::create_dir_all(root.path().join("photos")).expect("photos");
        std::fs::write(root.path().join("AGENTS.md"), b"before").expect("agents");
        std::fs::write(root.path().join("10-notes/Report.md"), b"note").expect("note");
        std::fs::write(root.path().join("10-notes/daily/Mon.md"), b"note").expect("nested");
        std::fs::write(root.path().join("10-notes-archive/old.md"), b"old").expect("old");
        std::fs::write(root.path().join("photos/a.png"), b"png").expect("png");
        root
    }

    fn unmanaged(route: WriteRoute<FakeVault>) -> UnmanagedPath {
        match route {
            WriteRoute::Unmanaged(target) => target,
            WriteRoute::Vault { path, .. } => {
                panic!(
                    "expected the plain writer, got the vault at {}",
                    path.as_str()
                )
            }
        }
    }

    /// The owner's report: `AGENTS.md` sits in the profile, outside the vault,
    /// and is now written by a plain atomic write.
    ///
    /// **Nothing is marked dirty, and the test proves it structurally rather
    /// than by watching for a call.** A live vault was available and the route
    /// did not hand it over; [`write_unmanaged`]'s signature has no slot a
    /// vault fits in, so there is no `mark_dirty` or `touch` to reach.
    #[test]
    fn a_file_outside_the_vault_is_written_by_the_plain_writer_and_marks_nothing() {
        let root = profile();
        let target = unmanaged(
            scope()
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        assert_eq!(target.profile_relative(), "AGENTS.md");

        assert_eq!(write_unmanaged(&target, "after\n"), Ok(()));
        assert_eq!(
            std::fs::read_to_string(root.path().join("AGENTS.md")).expect("read"),
            "after\n"
        );

        // Temp-and-rename, and the temp is gone: a `.keeper.<ulid>.tmp` left in
        // a synced folder is tier-0-excluded litter the owner still has to look
        // at.
        let litter = std::fs::read_dir(root.path())
            .expect("listing")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".keeper."))
            .count();
        assert_eq!(litter, 0);
    }

    /// Exact bytes, no trailing-newline normalisation — a file the owner did
    /// not change must not change.
    #[test]
    fn the_plain_writer_writes_the_exact_bytes_it_was_given() {
        let root = profile();
        let target = unmanaged(
            scope()
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        for content in ["no newline", "trailing\n\n\n", "\r\nwindows\r\n", ""] {
            assert_eq!(write_unmanaged(&target, content), Ok(()));
            assert_eq!(
                std::fs::read_to_string(root.path().join("AGENTS.md")).expect("read"),
                content
            );
        }
    }

    /// R405: a compare-and-replace writes only over the bytes its edit was
    /// made from. A file changed after that read — by anyone — keeps the
    /// change, nothing is written and no temp file is left; the file as it
    /// reads now is then written over normally.
    #[test]
    fn a_compare_and_replace_never_writes_over_a_change_made_after_its_read() {
        let root = profile();
        let target = unmanaged(
            scope()
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        let file = root.path().join("AGENTS.md");
        std::fs::write(&file, "read\r\n").expect("before");
        let read = Revision::of(b"read\r\n");

        std::fs::write(&file, "theirs\r\n").expect("a change after the read");
        assert_eq!(
            replace_unmanaged(&target, &read, "mine\r\n"),
            Err(WriteRefusal::ChangedSinceRead {
                relative_path: "AGENTS.md".to_owned()
            })
        );
        assert_eq!(std::fs::read(&file).expect("kept"), b"theirs\r\n");
        let litter = std::fs::read_dir(root.path())
            .expect("listing")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".keeper."))
            .count();
        assert_eq!(litter, 0);

        let now = Revision::of(&std::fs::read(&file).expect("now"));
        assert_eq!(replace_unmanaged(&target, &now, "mine\r\n"), Ok(()));
        assert_eq!(std::fs::read(&file).expect("written"), b"mine\r\n");
    }

    type Outcome = Result<(), WriteRefusal>;

    /// `first` runs until it holds `file` with its check passed, the step
    /// before its rename; only then does `second` start, and `first` renames
    /// once `second` has either reached that same step or is waiting for the
    /// file. Both outcomes, in order.
    fn interleaved(
        file: &Path,
        first: impl FnOnce() -> Outcome + Send + 'static,
        second: impl FnOnce() -> Outcome + Send + 'static,
    ) -> (Outcome, Outcome) {
        use std::sync::{mpsc, Mutex};
        use std::time::Duration;
        const PATIENCE: Duration = Duration::from_secs(30);

        let key = file.canonicalize().expect("canonical");
        let (events, seen) = mpsc::channel::<(String, seam::Point)>();
        let (go, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        seam::install(&key, move |point| {
            let who = std::thread::current().name().unwrap_or("").to_owned();
            let held = who == "first" && point == seam::Point::Renaming;
            let _ = events.send((who, point));
            if held {
                let _ = gate.lock().expect("gate").recv_timeout(PATIENCE);
            }
        });
        let spawn = |name: &str, write: Box<dyn FnOnce() -> Outcome + Send>| {
            std::thread::Builder::new()
                .name(name.to_owned())
                .spawn(write)
                .expect("spawn")
        };
        let a = spawn("first", Box::new(first));
        assert_eq!(
            seen.recv_timeout(PATIENCE).expect("first at its rename"),
            ("first".to_owned(), seam::Point::Renaming)
        );
        let b = spawn("second", Box::new(second));
        let (who, _) = seen.recv_timeout(PATIENCE).expect("second under way");
        assert_eq!(who, "second");
        go.send(()).expect("go");
        let outcomes = (a.join().expect("first"), b.join().expect("second"));
        seam::remove(&key);
        outcomes
    }

    /// R412 (R100M2-01): two saves made from one revision meet at the final
    /// check — the second arrives while the first has passed it and not yet
    /// renamed. Exactly one goes through, the other is refused as changed
    /// since its read, and the file holds the winner's bytes.
    #[test]
    fn two_replaces_from_one_revision_never_both_land() {
        let root = profile();
        let target = unmanaged(
            scope()
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        let file = root.path().join("AGENTS.md");
        let read = Revision::of(&std::fs::read(&file).expect("read"));
        let (mine, theirs) = (target.clone(), read.clone());

        let outcomes = interleaved(
            &file,
            move || replace_unmanaged(&mine, &theirs, "first\n"),
            move || replace_unmanaged(&target, &read, "second\n"),
        );

        assert_eq!(
            outcomes,
            (
                Ok(()),
                Err(WriteRefusal::ChangedSinceRead {
                    relative_path: "AGENTS.md".to_owned()
                })
            )
        );
        assert_eq!(std::fs::read(&file).expect("winner"), b"first\n");
        let litter = std::fs::read_dir(root.path())
            .expect("listing")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".keeper."))
            .count();
        assert_eq!(litter, 0);
    }

    /// R412: keeper's other writer of the same file — the Files surface, a
    /// bot's tool call — never lands between a replace's check and its
    /// rename, where the replace would write over it unseen: it waits, and
    /// its bytes are the ones the file keeps.
    #[test]
    fn a_plain_write_waits_for_a_replace_under_way_and_is_not_written_over() {
        let root = profile();
        let target = unmanaged(
            scope()
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        let file = root.path().join("AGENTS.md");
        let read = Revision::of(&std::fs::read(&file).expect("read"));
        let mine = target.clone();

        let outcomes = interleaved(
            &file,
            move || replace_unmanaged(&mine, &read, "replaced\n"),
            move || write_unmanaged(&target, "typed\n"),
        );

        assert_eq!(outcomes, (Ok(()), Ok(())));
        assert_eq!(std::fs::read(&file).expect("last"), b"typed\n");
    }

    /// **NFR-30 outside the vault: the bytes go to the OS trash and are still
    /// there afterwards.** An `unlink` would pass "the file is gone" and fail
    /// this.
    #[test]
    fn an_out_of_vault_delete_lands_in_the_os_trash_and_never_unlinks() {
        let root = profile();
        let home = tempfile::tempdir().expect("home");
        let trash = TrashTarget::Freedesktop(home.path().join("Trash"));
        let target = unmanaged(
            scope()
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );

        let grave = trash_unmanaged(&target, &trash, 1_775_000_000_000).expect("trashed");

        assert!(
            !root.path().join("AGENTS.md").exists(),
            "still in the folder"
        );
        assert_eq!(grave, home.path().join("Trash/files/AGENTS.md"));
        assert_eq!(std::fs::read(&grave).expect("the bytes"), b"before");

        // The `.trashinfo` row is what makes it a trash entry rather than a
        // file hidden in a directory: without it, Restore has nowhere to put
        // the file back and most desktops will not list it at all.
        let ticket = std::fs::read_to_string(home.path().join("Trash/info/AGENTS.md.trashinfo"))
            .expect("trashinfo");
        assert!(ticket.starts_with("[Trash Info]\n"), "{ticket}");
        assert!(
            ticket.contains(&format!(
                "Path={}\n",
                encoded_path(&root.path().canonicalize().expect("canon").join("AGENTS.md"))
            )),
            "{ticket}"
        );
        assert!(
            ticket.contains("DeletionDate=2026-03-31T23:33:20\n"),
            "{ticket}"
        );
    }

    /// Two files with one name do not overwrite each other in the trash, and
    /// the second keeps its extension where a file manager looks for it.
    #[test]
    fn a_second_file_of_the_same_name_gets_its_own_place_in_the_trash() {
        let root = profile();
        let home = tempfile::tempdir().expect("home");
        let trash = TrashTarget::Freedesktop(home.path().join("Trash"));
        let scope = scope();

        let first = unmanaged(
            scope
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        assert_eq!(
            trash_unmanaged(&first, &trash, 1_775_000_000_000).expect("first"),
            home.path().join("Trash/files/AGENTS.md")
        );

        std::fs::write(root.path().join("AGENTS.md"), b"the second one").expect("rewrite");
        let second = unmanaged(
            scope
                .route(Some(FakeVault("live")), root.path(), "AGENTS.md")
                .expect("routed"),
        );
        let grave = trash_unmanaged(&second, &trash, 1_775_000_000_000).expect("second");

        assert_eq!(grave, home.path().join("Trash/files/AGENTS.2.md"));
        assert_eq!(std::fs::read(&grave).expect("bytes"), b"the second one");
        // And the first one is untouched, which is the whole reason the info
        // file is claimed with `create_new` before any byte moves.
        assert_eq!(
            std::fs::read(home.path().join("Trash/files/AGENTS.md")).expect("bytes"),
            b"before"
        );
        assert!(home
            .path()
            .join("Trash/info/AGENTS.2.md.trashinfo")
            .exists());
    }

    /// The suffix goes before the extension, and a dotfile has no extension to
    /// go before.
    #[test]
    fn a_trash_name_keeps_its_extension_where_a_file_manager_looks_for_it() {
        assert_eq!(numbered("report.md", 0), "report.md");
        assert_eq!(numbered("report.md", 1), "report.2.md");
        assert_eq!(numbered("report.md", 9), "report.10.md");
        assert_eq!(numbered("Makefile", 1), "Makefile.2");
        assert_eq!(numbered(".gitignore", 1), ".gitignore.2");
        assert_eq!(numbered("a.tar.gz", 1), "a.tar.2.gz");
    }

    /// A path with a space in it is percent-encoded, because a `Path=` line a
    /// desktop cannot parse is a Restore that puts the file somewhere else.
    #[test]
    fn a_trashinfo_path_is_percent_encoded() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(root.path().join("my notes")).expect("dir");
        std::fs::write(root.path().join("my notes/a b.md"), b"x").expect("file");
        let home = tempfile::tempdir().expect("home");
        let target = unmanaged(
            WriteScope::new("Vault", None)
                .route(None::<FakeVault>, root.path(), "my notes/a b.md")
                .expect("routed"),
        );
        trash_unmanaged(
            &target,
            &TrashTarget::Freedesktop(home.path().join("Trash")),
            1_775_000_000_000,
        )
        .expect("trashed");
        let ticket = std::fs::read_to_string(home.path().join("Trash/info/a b.md.trashinfo"))
            .expect("trashinfo");
        assert!(ticket.contains("/my%20notes/a%20b.md\n"), "{ticket}");
    }

    /// The trash is resolved, never guessed — and a machine that cannot offer
    /// one gets a refusal rather than an `unlink`.
    #[test]
    fn this_machine_resolves_a_trash_of_the_right_shape() {
        let finder_country = cfg!(target_os = "macos");
        match os_trash().expect("a trash") {
            TrashTarget::Finder => assert!(finder_country, "Finder is macOS's trash and only its"),
            TrashTarget::Freedesktop(root) => {
                assert!(!finder_country, "macOS must not get a freedesktop trash");
                assert!(root.is_absolute(), "{}", root.display());
                assert!(root.ends_with("Trash"), "{}", root.display());
            }
        }
    }
}

#[cfg(test)]
#[path = "files_write_agent_tests.rs"]
mod agent_tests;

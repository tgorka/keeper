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
//! decided, and it hands back a [`WriteRoute`] whose vault arm *carries the
//! caller's vault* and whose unmanaged arm has nowhere to put one. The vault
//! writer needs a `Vault` and a [`VaultPath`]; [`write_unmanaged`] takes an
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
//!   [`resolve_existing`] refuses anything whose canonical form leaves the
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

use crate::browse::{self, BrowseRefusal};

/// The longest file name this surface will create.
///
/// 255 *bytes*, not characters: it is the limit APFS, ext4, NTFS and every
/// other filesystem keeper runs on states, and the one a multi-byte name
/// actually hits. Refusing here names the problem; letting the OS refuse gives
/// the user `File name too long (os error 36)` and no idea which of the two
/// hundred characters to remove.
pub const MAX_NAME_BYTES: usize = 255;

/// Directory names that belong to keeper, Obsidian or git, and that a person
/// creating a file must not be able to mint.
///
/// The same three `notes_vault::is_internal` refuses, restated here because
/// this module has to be able to answer *before* the write is attempted: a
/// refusal the surface only learns about from a failed command is a control
/// that should never have been offered.
const RESERVED_NAMES: [&str; 3] = [".keeper", ".obsidian", ".git"];

/// Why the Files surface will not write at a path.
///
/// Every variant carries what its sentence needs, and [`std::fmt::Display`]
/// composes that sentence — in this crate, so it is asserted on the machine the
/// code is written on, exactly as [`BrowseRefusal`]'s is. The shell renders it
/// verbatim and adds no words of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRefusal {
    /// This profile holds no notes vault keeper can reach, so nothing in it is
    /// keeper's to change.
    NoVault {
        /// The profile's human label — never its path (FR-145).
        profile_name: String,
    },
    /// The path is inside the profile but outside the vault.
    OutsideVault {
        profile_name: String,
        /// Where the vault sits inside the profile.
        subfolder: String,
        /// The path that was asked for, profile-relative.
        subpath: String,
    },
    /// The path names the vault directory itself.
    VaultRoot { subfolder: String },
    /// The entry is a directory, and this surface deletes files.
    IsDirectory { name: String },
    /// The subpath is not a plain descendant: absolute, or holding `..`, `.`,
    /// an empty component or a platform separator.
    Escapes { subpath: String },
    /// A create with no name.
    NameEmpty,
    /// A create whose name is not one plain file name.
    NameNotPlain { name: String },
    /// A create whose name is longer than the filesystem will take.
    NameTooLong { name: String, bytes: usize },
    /// A create naming a directory that belongs to keeper, Obsidian or git.
    NameReserved { name: String },
    /// A create whose name is already taken in that folder.
    NameTaken { name: String },
    /// The entry is not on disk any more.
    Missing { subpath: String },
    /// The folder could not be read, so a collision could not be ruled out.
    Unreadable {
        /// The OS's own words, which are the most useful thing to show.
        reason: String,
    },
    /// This machine has no operating-system trash to put an out-of-vault file
    /// into, so the deletion does not happen (AD-102, NFR-30).
    ///
    /// A refusal and never a fallback to `unlink`. The whole reason the second
    /// writer is allowed to delete at all is that the bytes stay somewhere the
    /// owner can reach; a machine that cannot offer that gets the refusal it
    /// had before this story, not a quiet erasure.
    NoSystemTrash {
        /// What was missing — a home directory, usually.
        reason: String,
    },
    /// [`WriteScope::route`] placed a path inside the vault and the caller
    /// handed no vault to write it with.
    ///
    /// Distinct from [`Self::NoVault`], which is about *creating* and is what a
    /// person reads where the New file control should be. This one is the two
    /// answers `vault_and_scope` exists to keep identical having come apart —
    /// a profile configured with a vault the registry has no live slot for,
    /// mid-start. Reported rather than assumed away, because assuming it away
    /// means writing a note through the unmanaged path.
    VaultUnreachable { profile_name: String },
    /// The write itself failed once everything had been allowed.
    ///
    /// Distinct from every refusal above, which are all decisions: this is the
    /// disk saying no, and the next step is the OS's words rather than
    /// anything about vaults.
    WriteFailed {
        relative_path: String,
        reason: String,
    },
    /// The removal itself failed once everything had been allowed.
    ///
    /// A separate variant from [`Self::WriteFailed`] because the verb is what
    /// a person is looking for in the sentence: "could not write" about a file
    /// they asked to delete sends them looking for the wrong problem.
    DeleteFailed {
        relative_path: String,
        reason: String,
    },
}

impl std::fmt::Display for WriteRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // The two sentences below are now reached ONLY from the create
            // path — `directory` and `create` — because AD-102 routes an
            // existing out-of-vault FILE to the second writer instead of
            // refusing it. So they say what is actually still refused. The
            // old wording ("changing it is your file manager's job") was the
            // sentence the owner's field report quoted back at us, and leaving
            // it anywhere on screen would be this story half done.
            Self::NoVault { profile_name } => write!(
                f,
                "{profile_name} holds no notes vault, so keeper will not create a new file \
                 in it. Files already there can still be edited and deleted."
            ),
            Self::OutsideVault {
                profile_name,
                subfolder,
                subpath,
            } => write!(
                f,
                "{subpath} is outside {profile_name}'s notes vault ({subfolder}), and keeper \
                 creates new files only inside the vault it manages. Files already there can \
                 still be edited and deleted."
            ),
            Self::VaultRoot { subfolder } => write!(
                f,
                "{subfolder} is the notes vault itself. keeper will not delete the folder its \
                 own vault lives in."
            ),
            Self::IsDirectory { name } => write!(
                f,
                "{name} is a folder. keeper deletes files here, not folders — remove a folder \
                 in your file manager, where you can see everything that would go with it."
            ),
            Self::Escapes { subpath } => write!(
                f,
                "\"{subpath}\" is not a path inside this folder, so keeper will not write to it."
            ),
            Self::NameEmpty => write!(f, "A new file needs a name."),
            Self::NameNotPlain { name } => write!(
                f,
                "\"{name}\" is not a file name: a name here cannot contain a slash and cannot \
                 be \".\" or \"..\"."
            ),
            Self::NameTooLong { name, bytes } => write!(
                f,
                "\"{name}\" is {bytes} bytes long and the longest name a file can have is \
                 {MAX_NAME_BYTES}. Shorten it."
            ),
            Self::NameReserved { name } => write!(
                f,
                "\"{name}\" is a name keeper, Obsidian and git use for their own folders, so \
                 keeper will not create a file called that."
            ),
            Self::NameTaken { name } => write!(
                f,
                "\"{name}\" is already in this folder. Pick another name — keeper will not \
                 write over a file you did not name."
            ),
            Self::Missing { subpath } => write!(
                f,
                "{subpath} is no longer in this folder, so nothing was deleted."
            ),
            Self::Unreadable { reason } => write!(
                f,
                "this folder could not be read, so keeper cannot tell whether the name is \
                 already taken: {reason}"
            ),
            Self::WriteFailed {
                relative_path,
                reason,
            } => write!(f, "keeper could not write {relative_path}: {reason}"),
            Self::DeleteFailed {
                relative_path,
                reason,
            } => write!(f, "keeper could not delete {relative_path}: {reason}"),
            Self::NoSystemTrash { reason } => write!(
                f,
                "keeper could not find this computer's trash ({reason}), and it will not \
                 erase a file it cannot put somewhere you can get it back from."
            ),
            Self::VaultUnreachable { profile_name } => write!(
                f,
                "keeper cannot reach {profile_name}'s notes vault right now, so it will not \
                 change a file inside it. Try again once the folder has finished opening."
            ),
        }
    }
}

impl From<BrowseRefusal> for WriteRefusal {
    /// Carry a containment refusal across without restating it.
    ///
    /// [`browse::plain_segments`] is the one lexical rule and it already
    /// produced the verdict; re-deriving it here would be the second copy this
    /// module exists to avoid. The *sentence* changes, because a path refused
    /// for a listing and a path refused for a write are the same fact with
    /// different consequences, and the reader is about to lose an edit rather
    /// than a folder view.
    fn from(refusal: BrowseRefusal) -> Self {
        match refusal {
            BrowseRefusal::Escapes { subpath }
            | BrowseRefusal::EscapesAfterResolution { subpath } => Self::Escapes { subpath },
            BrowseRefusal::Unreadable { reason } => Self::Unreadable { reason },
            // Story 47.2: a segment carrying U+FFFD may be a *rendering* of a
            // non-UTF-8 name rather than the name, and joining it can reach a
            // different real file — so a delete confirmed against one row would
            // remove another. `Escapes` rather than a new variant, because that
            // is exactly what it is: not a path in this folder.
            BrowseRefusal::Unspellable { subpath } => Self::Escapes { subpath },
        }
    }
}

/// A create's two answers: where it lands in the vault, and what the Files
/// surface will call it afterwards.
///
/// Both, because the two consumers need different frames and neither may
/// compose the other's. `vault_relative` is what `write_vault_file` takes;
/// `profile_relative` is what the Files pane already speaks and what it hands
/// back to select the new row (AD-65 — the frontend joins nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTarget {
    /// Vault-relative, `/`-joined. The argument of the one writer.
    pub vault_relative: String,
    /// Profile-relative, `/`-joined. What the listing speaks.
    pub profile_relative: String,
}

/// Where one profile's Files surface may write.
///
/// Constructed per call from the vault the shell can actually reach, so this
/// type cannot outlive the fact it encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteScope<'a> {
    profile_name: &'a str,
    /// The live vault's subfolder inside the profile, normalised, or `None`
    /// when this profile holds no reachable vault.
    subfolder: Option<String>,
}

impl<'a> WriteScope<'a> {
    /// A scope over the vault at `subfolder`, or over no vault at all.
    ///
    /// The subfolder is normalised here and only here. It is whatever the user
    /// typed into the settings form — `Notes/`, `\Notes`, `notes//daily` — and
    /// `NotesConfig::validate` deliberately refuses rather than corrects, so
    /// those spellings survive into the stored profile. Comparing them raw
    /// against a `/`-joined dirent path would answer "outside the vault" for a
    /// vault the user is looking at, and the surface would refuse to write in
    /// the one folder it is meant to write in.
    ///
    /// Case is deliberately NOT folded: unlike the folder-role marker, which
    /// only decides a glyph, this decides where bytes land, and
    /// `browse::plain_segments` has already guaranteed the subpath's segments
    /// are the dirent's own. Two folders differing only in case are two folders
    /// on the filesystem this is asserted on.
    pub fn new(profile_name: &'a str, subfolder: Option<&str>) -> Self {
        Self {
            profile_name,
            subfolder: subfolder.map(|configured| {
                configured
                    .split(['/', '\\'])
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("/")
            }),
        }
    }

    /// Whether anything in this profile can be written at all.
    pub fn holds_vault(&self) -> bool {
        self.subfolder.is_some()
    }

    /// The vault-relative path of a profile-relative subpath, or why there is
    /// none. `Ok("")` is the vault directory itself.
    ///
    /// The comparison is over whole components, never over the raw string: a
    /// folder called `10-notes-archive` beside a vault at `10-notes` starts
    /// with the vault's name and is not inside it, and a `starts_with` on the
    /// string would have let a write out of the vault into its neighbour.
    fn vault_relative(&self, subpath: &str) -> Result<String, WriteRefusal> {
        let Some(subfolder) = self.subfolder.as_deref() else {
            return Err(WriteRefusal::NoVault {
                profile_name: self.profile_name.to_owned(),
            });
        };
        // Refuse `..`, an absolute path and a platform separator before
        // anything else, so an escape is refused identically whether the vault
        // is where it says it is or not.
        browse::plain_segments(subpath)?;
        let outside = || WriteRefusal::OutsideVault {
            profile_name: self.profile_name.to_owned(),
            subfolder: subfolder.to_owned(),
            subpath: subpath.to_owned(),
        };
        let mut wanted = subpath.split('/').filter(|part| !part.is_empty());
        for part in subfolder.split('/').filter(|part| !part.is_empty()) {
            if wanted.next() != Some(part) {
                return Err(outside());
            }
        }
        Ok(wanted.collect::<Vec<_>>().join("/"))
    }

    /// The vault-relative path of a directory keeper may create a file in.
    ///
    /// The vault root is allowed here and refused by [`Self::file`]: a person
    /// may put a note at the top of their vault, and nobody may delete the
    /// vault.
    pub fn directory(&self, subpath: &str) -> Result<String, WriteRefusal> {
        self.vault_relative(subpath)
    }

    /// The vault-relative path of a file keeper may change or remove.
    ///
    /// `is_dir` comes from the dirent, never from the name, for the reason
    /// `FilesEntryVm::new` gives: a directory called `notes.md` exists, and a
    /// delete that classified by extension would offer to trash a folder.
    pub fn file(&self, subpath: &str, is_dir: bool) -> Result<String, WriteRefusal> {
        let relative = self.vault_relative(subpath)?;
        if relative.is_empty() {
            // The subpath IS the vault directory. Reported as the vault rather
            // than as "a folder", because the next step differs: one is "use
            // Finder", the other is "no, not this one".
            return Err(WriteRefusal::VaultRoot {
                subfolder: self.subfolder.clone().unwrap_or_default(),
            });
        }
        if is_dir {
            return Err(WriteRefusal::IsDirectory {
                name: last_segment(subpath).to_owned(),
            });
        }
        Ok(relative)
    }

    /// Where a new file called `name` in the directory `dir_subpath` would
    /// land, or why nowhere.
    ///
    /// Purely lexical: the collision check is [`collides`], which needs the
    /// disk. Split so the name rules can be asserted without a filesystem and
    /// so the surface can word a bad name while the user is still typing it.
    pub fn create(&self, dir_subpath: &str, name: &str) -> Result<CreateTarget, WriteRefusal> {
        let directory = self.directory(dir_subpath)?;
        check_name(name)?;
        let join = |base: &str| {
            if base.is_empty() {
                name.to_owned()
            } else {
                format!("{base}/{name}")
            }
        };
        Ok(CreateTarget {
            vault_relative: join(&directory),
            profile_relative: join(dir_subpath),
        })
    }

    /// Which of keeper's two writers owns `subpath`, or why neither does
    /// (Story 46.14, AD-102).
    ///
    /// **The one place the fork is decided.** A second site that worked out
    /// "is this in the vault" for itself would eventually work it out
    /// differently, and the disagreement that costs something is the one that
    /// sends a *vault* file down the plain writer: an edit that never reaches
    /// the reconciler, never marks the vault dirty, and so never gets
    /// committed. `route` is therefore the only constructor of both
    /// [`VaultPath`] and [`UnmanagedPath`].
    ///
    /// The order is the order that produces the useful sentence, and it is
    /// deliberately not "vault first, everything else falls through":
    ///
    /// 1. **Containment and existence first, through [`resolve_existing`].** It
    ///    refuses `..`, an absolute path, a platform separator and a symlink
    ///    whose canonical form leaves the profile root — *before* the vault
    ///    question is asked, which is the part that matters: [`Self::file`]'s
    ///    `vault_relative` tests for a vault before it tests the path, so in a
    ///    profile holding no vault it answers `NoVault` to `../etc`, and a
    ///    fall-through would then route a traversal to the plain writer.
    ///    Ordering, not a second check: a duplicate `plain_segments` call here
    ///    was written, mutation-tested, found to kill nothing, and removed.
    /// 2. **Both writers change a file; neither creates one.** A stale editor
    ///    whose file was deleted elsewhere must not put it back.
    ///    `sync_create_entry` is the create, and AD-102 does not widen it —
    ///    see the module note.
    /// 3. **The vault directory is refused as the vault**, not as "a folder":
    ///    the next step differs.
    /// 4. **A directory is refused at every location** (spec-45-3), which is
    ///    why this check sits outside the vault/unmanaged split rather than in
    ///    one arm of it.
    ///
    /// `vault` is the caller's own handle — this crate has never heard of
    /// `notes_vault::Vault` and does not need to. It is carried *into* the
    /// returned variant rather than looked up again afterwards, which is what
    /// makes "the vault path is unreachable without a vault" a fact about the
    /// types instead of a habit.
    pub fn route<V>(
        &self,
        vault: Option<V>,
        root: &Path,
        subpath: &str,
    ) -> Result<WriteRoute<V>, WriteRefusal> {
        // Containment first, and it is `resolve_existing` that provides it —
        // see the note on ordering above.
        let resolved = resolve_existing(root, subpath)?;
        match self.classify(subpath, resolved.is_dir())? {
            Owned::Vault(relative) => Ok(WriteRoute::Vault {
                vault: vault.ok_or_else(|| WriteRefusal::VaultUnreachable {
                    profile_name: self.profile_name.to_owned(),
                })?,
                path: VaultPath(relative),
            }),
            Owned::Unmanaged => Ok(WriteRoute::Unmanaged(UnmanagedPath {
                absolute: resolved,
                profile_relative: subpath.to_owned(),
            })),
        }
    }

    /// [`Self::route`]'s verdict without the disk — the listing's question
    /// (Story 46.14, AD-102).
    ///
    /// **One classifier, two callers, and that is the requirement rather than
    /// a saving.** Story 45.3's rule is that the flag the surface renders and
    /// the answer the command gives are the same question asked twice; a
    /// listing that decided "in the vault?" on its own would eventually decide
    /// it differently from the command, and the row that disagreed would be a
    /// row offering the wrong writer.
    ///
    /// Lexical because the listing already holds the dirent. `route`
    /// canonicalises, which is a `stat` per call; paying that for each of a
    /// thousand rows to re-learn `is_dir` — which `read_dir` just handed over —
    /// would make opening a folder slower for no new information.
    pub fn owner(&self, subpath: &str, is_dir: bool) -> Result<WriteOwner, WriteRefusal> {
        Ok(match self.classify(subpath, is_dir)? {
            Owned::Vault(_) => WriteOwner::Vault,
            Owned::Unmanaged => WriteOwner::Unmanaged,
        })
    }

    /// The fork itself. Everything above it is arguments; everything below it
    /// is consequences.
    fn classify(&self, subpath: &str, is_dir: bool) -> Result<Owned, WriteRefusal> {
        // With no vault, `vault_relative` tests for one before it tests the
        // path, so an escape must already have been refused by the caller —
        // `route` does it by resolving, `owner`'s caller by having read the
        // subpath out of a dirent `browse` produced.
        let inside = match self.vault_relative(subpath) {
            Ok(relative) => Some(relative),
            // The two ways a path is in no vault. Neither is a refusal any
            // more; both are the second writer's business.
            Err(WriteRefusal::NoVault { .. } | WriteRefusal::OutsideVault { .. }) => None,
            Err(other) => return Err(other),
        };
        if inside.as_deref() == Some("") {
            return Err(WriteRefusal::VaultRoot {
                subfolder: self.subfolder.clone().unwrap_or_default(),
            });
        }
        if is_dir {
            return Err(WriteRefusal::IsDirectory {
                name: last_segment(subpath).to_owned(),
            });
        }
        Ok(inside.map_or(Owned::Unmanaged, Owned::Vault))
    }

    /// The sentence a surface must show BEFORE editing a file keeper does not
    /// manage (Story 46.14, AD-102).
    ///
    /// **Before, and not after.** An edit that quietly does less than the vault
    /// path does is strictly worse than the refusal it replaces: a person who
    /// finds out after saving that this file has no history has already lost
    /// the thing history would have given them.
    ///
    /// **It says what is absent and refuses to overstate it.** The tempting
    /// sentence is "this will not sync", and for a file in a synced profile
    /// that is simply false — the folder engine watches the whole profile and
    /// carries whatever changes in it, exactly as it would for an edit made in
    /// Finder. What is genuinely absent is everything the *vault* provides, and
    /// naming that precisely is the difference between a caveat and a scare.
    ///
    /// Takes the file's own name, never its path (FR-145).
    pub fn unmanaged_caveat(&self, name: &str) -> String {
        let profile = self.profile_name;
        let placing = match self.subfolder.as_deref() {
            Some(subfolder) => {
                format!("it is outside {profile}'s notes vault ({subfolder})")
            }
            None => format!("{profile} holds no notes vault"),
        };
        format!(
            "{name} is not one of keeper's notes — {placing}. keeper saves it straight to \
             the file and sends a delete to this computer's trash: no note history, no \
             search index and no conflict copy. Nothing about how {profile} syncs this \
             folder changes."
        )
    }
}

// ─── AD-102: the second writer ───────────────────────────────────────────────

/// [`WriteScope::classify`]'s answer, with the vault-relative path the vault
/// arm needs.
///
/// Private, because the path it carries is the raw string a [`VaultPath`] is
/// made of, and letting that out of the module would be a way around the one
/// constructor.
enum Owned {
    Vault(String),
    Unmanaged,
}

/// Which writer owns a path, for a caller that only needs to know which
/// (Story 46.14, AD-102).
///
/// [`Owned`] without the path — the listing's half of [`WriteScope::classify`],
/// which decides a flag and a sentence rather than a destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOwner {
    /// keeper manages this file: the vault writer, `mark_dirty`, the vault
    /// trash, the notes index.
    Vault,
    /// keeper will write it and does not manage it — AD-102's second writer,
    /// over a surface carrying [`WriteScope::unmanaged_caveat`].
    Unmanaged,
}

/// A path inside the notes vault, and the only argument the vault writer takes.
///
/// A newtype over a `String`, and worth every character of it. AD-89's promise
/// 2 is "one writer"; [`WriteRoute`] adds a second one for files no vault
/// holds, and what stops the two merging six months from now is that they do
/// not share a signature. This type has a private field, no `From<String>` and
/// exactly one constructor — [`WriteScope::route`], which can only reach it
/// through a scope that already holds a vault subfolder. A bare string cannot
/// become a vault write by being handed to the wrong function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultPath(String);

impl VaultPath {
    /// Vault-relative and `/`-joined — what `notes_vault::write_vault_file`
    /// and `notes_vault::trash_note` take, alongside the `Vault` that this
    /// crate cannot see and therefore cannot forge.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A file inside a sync profile that no vault holds — the second writer's only
/// argument.
///
/// Absolute and already canonicalised: [`resolve_existing`] proved it is a real
/// descendant of the profile root *after* symlinks. That proof is why the type
/// exists and why it is opaque — a `PathBuf` arriving at [`write_unmanaged`]
/// from anywhere else would be a write to an arbitrary location on the machine,
/// which is the failure AD-89's first promise was written against.
///
/// It carries no vault and cannot be given one: neither writer that takes it
/// has a slot a `Vault` fits in. That is the whole of AD-102's type-level
/// separation, from the other side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedPath {
    absolute: PathBuf,
    profile_relative: String,
}

impl UnmanagedPath {
    /// What the surface already shows, for a sentence and for a log line.
    ///
    /// The absolute path is deliberately not exposed: FR-145 keeps it off
    /// screen, and the two functions that need it live in this module.
    pub fn profile_relative(&self) -> &str {
        &self.profile_relative
    }
}

/// Which of keeper's two writers owns a path (AD-102).
///
/// `V` is the caller's vault handle. This crate has never heard of
/// `notes_vault::Vault`, and generic-over-it is how the decision stays here —
/// testable on Linux — while the vault stays in the shell. Carrying the vault
/// *inside* the variant is the load-bearing part: the vault arm cannot be
/// reached without a vault because it holds one, and the unmanaged arm cannot
/// be handed a vault because it has nowhere to put one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRoute<V> {
    /// Inside the vault. `write_vault_file` + `touch` + `mark_dirty`,
    /// unchanged by this story.
    Vault {
        /// The live vault the caller handed to [`WriteScope::route`].
        vault: V,
        /// Where in it the file sits.
        path: VaultPath,
    },
    /// Inside the profile, outside every vault: [`write_unmanaged`] and
    /// [`trash_unmanaged`], over a surface that said so first.
    Unmanaged(UnmanagedPath),
}

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
pub fn write_unmanaged(target: &UnmanagedPath, content: &str) -> Result<(), WriteRefusal> {
    let failed = |error: std::io::Error| WriteRefusal::WriteFailed {
        relative_path: target.profile_relative.clone(),
        reason: error.to_string(),
    };
    // The path resolved to an existing file, so it has a parent directory. The
    // fallback keeps this total rather than asserting it.
    let directory = target.absolute.parent().unwrap_or_else(|| Path::new("."));
    let temp = directory.join(format!(".keeper.{}.tmp", ulid::Ulid::new()));
    std::fs::write(&temp, content.as_bytes()).map_err(failed)?;
    if let Err(error) = std::fs::rename(&temp, &target.absolute) {
        // A failed rename must not leave the temp behind. It is excluded from
        // sync, but it is still litter in the owner's folder.
        let _ = std::fs::remove_file(&temp);
        return Err(failed(error));
    }
    Ok(())
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
        relative_path: target.profile_relative.clone(),
        reason,
    };
    match trash {
        TrashTarget::Freedesktop(root) => freedesktop_trash(&target.absolute, root, local_now_ms)
            .map_err(|error| failed(error.to_string())),
        TrashTarget::Finder => finder_trash(&target.absolute).map_err(failed),
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
    let (year, month, day, hour, minute, second) =
        crate::platform::civil_from_unix_ms(local_now_ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Whether `name` is already taken in `directory`, matching case-insensitively.
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
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Resolve a profile-relative subpath that must already be on disk.
///
/// [`browse::resolve`]'s `Ok(None)` — the lexical test passed and there is
/// nothing there — becomes [`WriteRefusal::Missing`] here, because a write path
/// has no use for the distinction the listing draws: whether the drive is out
/// or the file was moved, this file is not the one being deleted, and deleting
/// something else would be the worst possible recovery.
pub fn resolve_existing(root: &Path, subpath: &str) -> Result<PathBuf, WriteRefusal> {
    browse::resolve(root, subpath)?.ok_or_else(|| WriteRefusal::Missing {
        subpath: subpath.to_owned(),
    })
}

/// The last `/`-separated component of a subpath, for a sentence that names the
/// thing rather than the route to it.
fn last_segment(subpath: &str) -> &str {
    subpath.rsplit('/').next().unwrap_or(subpath)
}

/// The rules a new file's name must satisfy, in the order that produces the
/// most useful sentence.
///
/// Emptiness first (the user has typed nothing yet), then shape, then length,
/// then reservation — so a person typing `../x` is told it is not a name rather
/// than that it is too short.
fn check_name(name: &str) -> Result<(), WriteRefusal> {
    if name.trim().is_empty() {
        return Err(WriteRefusal::NameEmpty);
    }
    if name != name.trim() {
        // A trailing space is invisible on screen and significant on disk, and
        // a leading one sorts the file somewhere the user will not look.
        // Refusing beats silently trimming: the name the user reads back has to
        // be the name that was written.
        return Err(WriteRefusal::NameNotPlain {
            name: name.to_owned(),
        });
    }
    // One plain component, by exactly the rule the containment test uses — so a
    // name and a path segment can never disagree about what a name is.
    if browse::plain_segments(name).map(|parts| parts.len()) != Ok(1) {
        return Err(WriteRefusal::NameNotPlain {
            name: name.to_owned(),
        });
    }
    if name.contains('\0') || name.contains('\\') {
        // `\` is a separator on Windows and an ordinary character here, so the
        // component test above lets it through on Linux. A name that means one
        // file on this machine and two on the next is not a name.
        return Err(WriteRefusal::NameNotPlain {
            name: name.to_owned(),
        });
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(WriteRefusal::NameTooLong {
            name: name.to_owned(),
            bytes: name.len(),
        });
    }
    if RESERVED_NAMES
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        return Err(WriteRefusal::NameReserved {
            name: name.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> WriteScope<'static> {
        WriteScope::new("Vault", Some("10-notes"))
    }

    /// The whole of the "outside a vault cannot be written" rule: a profile
    /// with no vault refuses everything, and says which folder it is refusing
    /// about rather than "not allowed".
    #[test]
    fn a_profile_with_no_vault_refuses_every_path_and_names_itself() {
        let none = WriteScope::new("Field", None);
        assert!(!none.holds_vault());
        let refusal = none.file("clip.mov", false).expect_err("no vault");
        assert_eq!(
            refusal,
            WriteRefusal::NoVault {
                profile_name: "Field".to_owned()
            }
        );
        assert!(refusal.to_string().contains("Field holds no notes vault"));
        assert!(none.directory("").is_err());
        assert!(none.create("", "x.md").is_err());
    }

    /// A sibling whose name merely *starts with* the vault's is not in the
    /// vault. A `starts_with` over the raw string would have written into it.
    #[test]
    fn a_folder_whose_name_extends_the_vaults_is_not_inside_the_vault() {
        let scope = scope();
        assert_eq!(scope.file("10-notes/a.md", false), Ok("a.md".to_owned()));
        let refusal = scope
            .file("10-notes-archive/a.md", false)
            .expect_err("neighbour");
        assert!(matches!(refusal, WriteRefusal::OutsideVault { .. }));
        assert!(
            refusal.to_string().contains("outside Vault's notes vault"),
            "{refusal}"
        );
    }

    /// The profile root, a sibling folder and a file beside the vault are all
    /// outside it, and each says so naming the vault's location.
    #[test]
    fn everything_outside_the_vault_is_refused_with_the_vault_named() {
        let scope = scope();
        for subpath in ["", "recordings", "recordings/clip.mov", "readme.md"] {
            let refusal = scope.directory(subpath).expect_err(subpath);
            assert_eq!(
                refusal,
                WriteRefusal::OutsideVault {
                    profile_name: "Vault".to_owned(),
                    subfolder: "10-notes".to_owned(),
                    subpath: subpath.to_owned(),
                },
                "{subpath}"
            );
            assert!(refusal.to_string().contains("(10-notes)"), "{refusal}");
        }
    }

    /// A nested vault subfolder is matched component by component.
    #[test]
    fn a_nested_vault_subfolder_is_matched_one_component_at_a_time() {
        let deep = WriteScope::new("Vault", Some("a/b"));
        assert_eq!(deep.directory("a/b"), Ok(String::new()));
        assert_eq!(deep.file("a/b/c/d.md", false), Ok("c/d.md".to_owned()));
        assert!(matches!(
            deep.file("a/c/d.md", false),
            Err(WriteRefusal::OutsideVault { .. })
        ));
        assert!(matches!(
            deep.directory("a"),
            Err(WriteRefusal::OutsideVault { .. })
        ));
    }

    /// The stored subfolder is whatever the user typed, and `NotesConfig`
    /// refuses rather than corrects — so `Notes/`, `\Notes` and `a//b` all
    /// reach here verbatim. Normalising at construction is what keeps the
    /// surface from refusing to write in the one folder it exists to write in.
    #[test]
    fn the_configured_subfolder_is_normalised_however_it_was_typed() {
        for spelling in ["10-notes", "10-notes/", "/10-notes", "\\10-notes"] {
            let scope = WriteScope::new("Vault", Some(spelling));
            assert_eq!(
                scope.file("10-notes/a.md", false),
                Ok("a.md".to_owned()),
                "{spelling}"
            );
        }
        for spelling in ["a/b", "a//b", "a\\b", "/a/b/"] {
            let scope = WriteScope::new("Vault", Some(spelling));
            assert_eq!(
                scope.file("a/b/c.md", false),
                Ok("c.md".to_owned()),
                "{spelling}"
            );
        }
        // The sentence names the normalised form, so two profiles configured
        // with two spellings of one folder read the same to a person.
        let refusal = WriteScope::new("Vault", Some("10-notes/"))
            .file("other/a.md", false)
            .expect_err("outside");
        assert_eq!(
            refusal,
            WriteRefusal::OutsideVault {
                profile_name: "Vault".to_owned(),
                subfolder: "10-notes".to_owned(),
                subpath: "other/a.md".to_owned(),
            }
        );
    }

    /// An escape is refused before the vault question is asked, so it is
    /// refused identically whether the path would have been in the vault or
    /// not — and it cannot be probed for by aiming it at the vault.
    #[test]
    fn traversal_is_refused_wherever_it_is_aimed() {
        let scope = scope();
        for subpath in [
            "..",
            "../etc",
            "10-notes/../../etc",
            "/etc/passwd",
            ".",
            "10-notes/./a.md",
            "10-notes//a.md",
            "10-notes/a.md/",
        ] {
            let refusal = scope.file(subpath, false).expect_err(subpath);
            assert_eq!(
                refusal,
                WriteRefusal::Escapes {
                    subpath: subpath.to_owned()
                },
                "{subpath}"
            );
        }
    }

    /// The vault directory itself is not a file, and the sentence says which
    /// folder it is refusing to remove.
    #[test]
    fn the_vault_directory_itself_cannot_be_deleted() {
        let refusal = scope().file("10-notes", true).expect_err("vault root");
        assert_eq!(
            refusal,
            WriteRefusal::VaultRoot {
                subfolder: "10-notes".to_owned()
            }
        );
        assert!(refusal.to_string().contains("10-notes is the notes vault"));
        // …but a file may be created in it.
        assert_eq!(scope().directory("10-notes"), Ok(String::new()));
    }

    /// Folder-ness comes from the dirent. A directory called `notes.md` is a
    /// directory, and the refusal names it.
    #[test]
    fn a_directory_is_refused_as_a_folder_whatever_it_is_named() {
        let refusal = scope()
            .file("10-notes/notes.md", true)
            .expect_err("directory");
        assert_eq!(
            refusal,
            WriteRefusal::IsDirectory {
                name: "notes.md".to_owned()
            }
        );
        assert!(refusal.to_string().contains("notes.md is a folder"));
        // The same path with the dirent saying file is perfectly writable.
        assert_eq!(
            scope().file("10-notes/notes.md", false),
            Ok("notes.md".to_owned())
        );
    }

    /// A create resolves both frames, and neither is composed by adding strings
    /// in the caller.
    #[test]
    fn a_create_answers_in_both_frames() {
        assert_eq!(
            scope().create("10-notes", "Report.md"),
            Ok(CreateTarget {
                vault_relative: "Report.md".to_owned(),
                profile_relative: "10-notes/Report.md".to_owned(),
            })
        );
        assert_eq!(
            scope().create("10-notes/daily", "2026-08-10.md"),
            Ok(CreateTarget {
                vault_relative: "daily/2026-08-10.md".to_owned(),
                profile_relative: "10-notes/daily/2026-08-10.md".to_owned(),
            })
        );
    }

    /// A create outside the vault is refused by the directory rule, before the
    /// name is even looked at.
    #[test]
    fn a_create_outside_the_vault_is_refused_by_location_not_by_name() {
        assert!(matches!(
            scope().create("recordings", "Report.md"),
            Err(WriteRefusal::OutsideVault { .. })
        ));
    }

    /// Every name rule, and the order they fire in.
    #[test]
    fn a_name_must_be_one_plain_file_name() {
        let scope = scope();
        let refuse = |name: &str| scope.create("10-notes", name).expect_err(name);

        assert_eq!(refuse(""), WriteRefusal::NameEmpty);
        assert_eq!(refuse("   "), WriteRefusal::NameEmpty);
        for name in ["a/b.md", "..", ".", "/x.md", " lead.md", "trail.md "] {
            assert_eq!(
                refuse(name),
                WriteRefusal::NameNotPlain {
                    name: name.to_owned()
                },
                "{name}"
            );
        }
        // `\` is a separator on Windows and a filename character here. A name
        // that means one file on this machine and two on the next is refused on
        // both.
        assert_eq!(
            refuse("a\\b.md"),
            WriteRefusal::NameNotPlain {
                name: "a\\b.md".to_owned()
            }
        );

        let long = format!("{}.md", "x".repeat(MAX_NAME_BYTES));
        assert_eq!(
            refuse(&long),
            WriteRefusal::NameTooLong {
                name: long.clone(),
                bytes: long.len(),
            }
        );
        // Exactly at the cap is allowed: the boundary is a limit, not a margin.
        let exact = "y".repeat(MAX_NAME_BYTES);
        assert!(scope.create("10-notes", &exact).is_ok());

        for name in [".keeper", ".obsidian", ".git", ".KEEPER"] {
            assert_eq!(
                refuse(name),
                WriteRefusal::NameReserved {
                    name: name.to_owned()
                },
                "{name}"
            );
        }
        // A dotfile that is not one of the three is an ordinary name.
        assert!(scope.create("10-notes", ".gitignore").is_ok());
    }

    /// A multi-byte name is measured in bytes, because that is what the
    /// filesystem counts.
    #[test]
    fn the_name_cap_counts_bytes_and_not_characters() {
        let scope = scope();
        // 128 two-byte characters is 256 bytes and 128 characters.
        let name = "é".repeat(128);
        assert_eq!(name.chars().count(), 128);
        assert_eq!(
            scope.create("10-notes", &name),
            Err(WriteRefusal::NameTooLong {
                name: name.clone(),
                bytes: 256
            })
        );
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

    /// A path that is not on disk is `Missing`, and never resolves to
    /// something else that is.
    #[test]
    fn a_path_that_is_gone_is_missing_rather_than_resolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("here.md"), b"x").expect("write");

        assert_eq!(
            resolve_existing(dir.path(), "here.md"),
            Ok(dir.path().canonicalize().expect("canon").join("here.md"))
        );
        assert_eq!(
            resolve_existing(dir.path(), "gone.md"),
            Err(WriteRefusal::Missing {
                subpath: "gone.md".to_owned()
            })
        );
        assert_eq!(
            resolve_existing(dir.path(), "../etc"),
            Err(WriteRefusal::Escapes {
                subpath: "../etc".to_owned()
            })
        );
    }

    /// A symlink pointing out of the profile is refused after canonicalisation,
    /// carried across from the one containment rule rather than re-derived.
    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_refused_by_the_write_path_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret"), b"x").expect("write");
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("escape"))
            .expect("symlink");

        assert_eq!(
            resolve_existing(dir.path(), "escape"),
            Err(WriteRefusal::Escapes {
                subpath: "escape".to_owned()
            })
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

    /// **The separation, stated as a test.** A file inside the vault routes to
    /// the vault writer carrying the vault, and never to the plain one.
    ///
    /// This is the mutation target for AD-102: make `route` fall through to
    /// `WriteRoute::Unmanaged` for an in-vault path and this fails, because a
    /// vault file written by the plain writer is an edit the reconciler never
    /// hears about and the commit cadence never carries.
    #[test]
    fn a_vault_file_is_never_routed_to_the_plain_writer() {
        let root = profile();
        let scope = scope();
        for (subpath, expected) in [
            ("10-notes/Report.md", "Report.md"),
            ("10-notes/daily/Mon.md", "daily/Mon.md"),
        ] {
            match scope
                .route(Some(FakeVault("live")), root.path(), subpath)
                .expect(subpath)
            {
                WriteRoute::Vault { vault, path } => {
                    assert_eq!(vault, FakeVault("live"), "{subpath}");
                    assert_eq!(path.as_str(), expected, "{subpath}");
                }
                WriteRoute::Unmanaged(target) => panic!(
                    "{subpath} is in the vault and was handed to the plain writer as {}",
                    target.profile_relative()
                ),
            }
        }
    }

    /// The neighbour whose name merely extends the vault's is NOT in the
    /// vault, and the route says so the same way the refusal used to — one
    /// component at a time, never a `starts_with` over the string.
    #[test]
    fn a_folder_whose_name_extends_the_vaults_routes_to_the_plain_writer() {
        let root = profile();
        let target = unmanaged(
            scope()
                .route(
                    Some(FakeVault("live")),
                    root.path(),
                    "10-notes-archive/old.md",
                )
                .expect("routed"),
        );
        assert_eq!(target.profile_relative(), "10-notes-archive/old.md");
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

    /// A profile holding no vault at all can still be edited and deleted —
    /// this is the second half of the owner's report, and the reason `route`
    /// treats `NoVault` as "no vault holds it" rather than as a refusal.
    #[test]
    fn a_profile_with_no_vault_routes_everything_to_the_plain_writer() {
        let root = profile();
        let none = WriteScope::new("Field", None);
        for subpath in ["AGENTS.md", "10-notes/Report.md", "photos/a.png"] {
            let target = unmanaged(
                none.route(None::<FakeVault>, root.path(), subpath)
                    .expect(subpath),
            );
            assert_eq!(target.profile_relative(), subpath);
        }
        // Creating is still vault-only, and its sentence no longer sends the
        // owner to their file manager for something keeper now does.
        let refusal = none.create("", "x.md").expect_err("no vault");
        assert!(
            refusal
                .to_string()
                .contains("will not create a new file in it"),
            "{refusal}"
        );
    }

    /// **A directory is refused at every location** — spec-45-3's rule, which
    /// AD-102 does not widen. The OS trash does not make one confirmation over
    /// a hundred thousand files into a confirmation.
    #[test]
    fn a_directory_is_refused_inside_and_outside_the_vault() {
        let root = profile();
        let scope = scope();
        for subpath in ["photos", "10-notes/daily", "10-notes-archive"] {
            let refusal = scope
                .route(Some(FakeVault("live")), root.path(), subpath)
                .expect_err(subpath);
            assert_eq!(
                refusal,
                WriteRefusal::IsDirectory {
                    name: last_segment(subpath).to_owned()
                },
                "{subpath}"
            );
        }
        // The vault directory itself is reported as the vault, not as "a
        // folder": the next step differs.
        assert_eq!(
            scope
                .route(Some(FakeVault("live")), root.path(), "10-notes")
                .expect_err("vault root"),
            WriteRefusal::VaultRoot {
                subfolder: "10-notes".to_owned()
            }
        );
        // And a profile with no vault still refuses the folder, because the
        // rule is about folders and not about vaults.
        assert_eq!(
            WriteScope::new("Field", None)
                .route(None::<FakeVault>, root.path(), "photos")
                .expect_err("folder"),
            WriteRefusal::IsDirectory {
                name: "photos".to_owned()
            }
        );
    }

    /// **A path outside every sync profile is refused, and that is the line
    /// this story draws.** keeper addresses a file as (profile id, subpath);
    /// there is no id for a file in no profile, and a symlink is not a way to
    /// borrow one.
    #[test]
    fn a_path_outside_the_profile_is_refused_by_both_writers() {
        let root = profile();
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret"), b"x").expect("write");
        let scope = scope();

        for subpath in ["../etc/passwd", "/etc/passwd", "10-notes/../../etc"] {
            assert_eq!(
                scope
                    .route(Some(FakeVault("live")), root.path(), subpath)
                    .expect_err(subpath),
                WriteRefusal::Escapes {
                    subpath: subpath.to_owned()
                },
                "{subpath}"
            );
            // Identically with no vault: without the explicit lexical check in
            // `route`, `vault_relative` would answer `NoVault` here and the
            // fall-through would route a traversal to the plain writer.
            assert_eq!(
                WriteScope::new("Field", None)
                    .route(None::<FakeVault>, root.path(), subpath)
                    .expect_err(subpath),
                WriteRefusal::Escapes {
                    subpath: subpath.to_owned()
                },
                "{subpath}"
            );
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("escape"))
                .expect("symlink");
            assert_eq!(
                scope
                    .route(Some(FakeVault("live")), root.path(), "escape")
                    .expect_err("symlink"),
                WriteRefusal::Escapes {
                    subpath: "escape".to_owned()
                }
            );
        }
    }

    /// Neither writer creates a file. A stale editor whose file was deleted
    /// elsewhere must not put it back, and a delete of something already gone
    /// must not delete something else.
    #[test]
    fn neither_writer_creates_a_file_that_is_not_there() {
        let root = profile();
        for scope in [scope(), WriteScope::new("Field", None)] {
            assert_eq!(
                scope
                    .route(None::<FakeVault>, root.path(), "gone.md")
                    .expect_err("missing"),
                WriteRefusal::Missing {
                    subpath: "gone.md".to_owned()
                }
            );
        }
    }

    /// A scope that says "in the vault" while the caller holds no vault is the
    /// two answers `vault_and_scope` exists to keep identical having come
    /// apart. Reported, never assumed away — assuming it away writes a note
    /// through the unmanaged path.
    #[test]
    fn an_in_vault_path_with_no_vault_in_hand_is_refused_rather_than_downgraded() {
        let root = profile();
        let refusal = scope()
            .route(None::<FakeVault>, root.path(), "10-notes/Report.md")
            .expect_err("no live vault");
        assert_eq!(
            refusal,
            WriteRefusal::VaultUnreachable {
                profile_name: "Vault".to_owned()
            }
        );
        assert!(
            refusal
                .to_string()
                .contains("cannot reach Vault's notes vault"),
            "{refusal}"
        );
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

    /// The refusal a machine with no trash gets says what is missing and does
    /// not offer to erase anything.
    #[test]
    fn a_machine_with_no_trash_refuses_rather_than_erasing() {
        let refusal = WriteRefusal::NoSystemTrash {
            reason: "neither XDG_DATA_HOME nor HOME is set".to_owned(),
        };
        assert!(
            refusal.to_string().contains("get it back from"),
            "{refusal}"
        );
    }

    /// **The listing's flag and the command's answer are the same question
    /// asked twice.** [`WriteScope::owner`] is lexical and [`WriteScope::route`]
    /// resolves; a row that disagreed with the command it launches is a row
    /// offering the wrong writer.
    #[test]
    fn the_listings_verdict_and_the_commands_route_never_disagree() {
        let root = profile();
        for scope in [scope(), WriteScope::new("Field", None)] {
            for (subpath, is_dir) in [
                ("AGENTS.md", false),
                ("10-notes/Report.md", false),
                ("10-notes/daily/Mon.md", false),
                ("10-notes-archive/old.md", false),
                ("photos/a.png", false),
                ("photos", true),
                ("10-notes", true),
                ("10-notes/daily", true),
            ] {
                let lexical = scope.owner(subpath, is_dir);
                let routed = scope
                    .route(Some(FakeVault("live")), root.path(), subpath)
                    .map(|route| match route {
                        WriteRoute::Vault { .. } => WriteOwner::Vault,
                        WriteRoute::Unmanaged(_) => WriteOwner::Unmanaged,
                    });
                assert_eq!(lexical, routed, "{subpath}");
            }
        }
    }

    /// The standing sentence a reader sees before the first keystroke.
    ///
    /// It names what is absent — history, index, conflict copy — and must NOT
    /// claim the file will not sync: for a file in a synced profile the folder
    /// engine carries it exactly as it carries an edit made in Finder, and a
    /// caveat that overstates is a caveat people learn to ignore.
    #[test]
    fn the_caveat_names_what_is_missing_without_overstating_it() {
        let with_vault = scope().unmanaged_caveat("AGENTS.md");
        assert!(with_vault.contains("AGENTS.md is not one of keeper's notes"));
        assert!(with_vault.contains("outside Vault's notes vault (10-notes)"));

        let without = WriteScope::new("Field", None).unmanaged_caveat("clip.txt");
        assert!(without.contains("Field holds no notes vault"), "{without}");

        for caveat in [&with_vault, &without] {
            for absent in ["no note history", "no search index", "no conflict copy"] {
                assert!(caveat.contains(absent), "{caveat}");
            }
            assert!(caveat.contains("this computer's trash"), "{caveat}");
            // The overstatement this sentence exists to avoid.
            assert!(!caveat.contains("will not sync"), "{caveat}");
            assert!(!caveat.contains("does not sync"), "{caveat}");
        }
    }
}

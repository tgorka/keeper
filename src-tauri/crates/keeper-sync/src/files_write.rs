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
//!    files — the notes vault — and nowhere else in the profile. A file outside
//!    a vault is listed and viewed and *says why* it cannot be changed, because
//!    an action that will fail is worse than an action that is absent (FR-145,
//!    AD-65 unaffected: the frontend still never joins a root and a subpath).
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
            Self::NoVault { profile_name } => write!(
                f,
                "{profile_name} holds no notes vault, so keeper will not change files in it. \
                 You can open and reveal them here; changing them is your file manager's job."
            ),
            Self::OutsideVault {
                profile_name,
                subfolder,
                subpath,
            } => write!(
                f,
                "{subpath} is outside {profile_name}'s notes vault ({subfolder}), and keeper \
                 only writes inside the vault it manages. You can open and reveal this file \
                 here; changing it is your file manager's job."
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
}

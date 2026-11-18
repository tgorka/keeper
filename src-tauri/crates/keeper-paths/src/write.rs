//! The one write-routing rule; filesystem effects stay with the writer.

use crate::{names, BrowseRefusal};
use std::path::{Component, Path, PathBuf};

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
    /// an empty component or a platform separator. Also a configured vault or
    /// zone subfolder that names no folder inside the profile
    /// ([`WriteScope::new`]), which refuses every write in that scope.
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
    /// A compare-and-replace found the file changed since the caller read
    /// it (`keeper_sync::files_write::replace_unmanaged`, R405): nothing was written over the
    /// change.
    ChangedSinceRead { relative_path: String },
    /// The removal itself failed once everything had been allowed.
    ///
    /// A separate variant from [`Self::WriteFailed`] because the verb is what
    /// a person is looking for in the sentence: "could not write" about a file
    /// they asked to delete sends them looking for the wrong problem.
    DeleteFailed {
        relative_path: String,
        reason: String,
    },
    /// The path is inside a session's `workspace/` — scratch keeper reads and
    /// never writes (Phase 7, AD-113, FR-237).
    ///
    /// A refusal by construction rather than a UI convention: the zone's own
    /// contract says the workspace is unversioned and dies with the session,
    /// so a keeper write there would put bytes in the one subtree whose loss
    /// is designed in. The sentence is the zone's own words.
    SessionWorkspace {
        /// The path that was asked for, profile-relative.
        subpath: String,
    },
    /// The path is one of an agents zone's own files or an agent's home file
    /// (AD-362): `_drive.toml`, `_skills/`, `_workflows/`, `_template/`, or a
    /// home's `agent.toml`, `SOUL.md`, `USER.md`, `MEMORY.md`, `journal/` or
    /// `proposals/`.
    ///
    /// No tool writes a soul, a tool grant or core memory — not even the
    /// home's own agent, because a turn an untrusted page steered must not be
    /// able to rewrite the instructions of every later turn.
    AgentHome {
        /// The path that was asked for, profile-relative.
        subpath: String,
    },
    /// The path is in the sessions zone and the scope is an agent host's
    /// (R51): an agent writes a session only through its session tools,
    /// which stamp what they write; `drive_write` and `drive_edit` never do.
    SessionsZone {
        /// The path that was asked for, profile-relative.
        subpath: String,
    },
    /// The path is git's or keeper's own (109.27): under a `.git`, `.keeper`,
    /// `.keeper-sync` or `.obsidian` folder, or a `.gitattributes`,
    /// `.gitignore`, `.gitmodules`, `.lfsconfig` or `.keepervirtual` file —
    /// files that change what the drive carries or how. `keeper_sync::files_write::agent_write_check`
    /// refuses it at any depth, case-folded.
    Machinery { subpath: String },
    /// The path is one `.gitignore` or the profile's exclusions leave out, so
    /// a write there would never be part of the drive (`keeper_sync::files_write::agent_write_check`).
    Ignored { subpath: String },
    /// The path is stored through LFS — routed there by `.gitattributes`, or
    /// a pointer in the index or on the disk — and an agent writes text into
    /// git only (`keeper_sync::files_write::agent_write_check`).
    LfsRouted { subpath: String },
    /// The index tracks the path as something other than a regular file — a
    /// submodule, say — or the disk holds something other than a regular file
    /// at a tracked one (`keeper_sync::files_write::agent_write_check`, R-HQ-81). A path the index
    /// does not track at all is [`Self::Missing`] to an agent's write,
    /// whatever the disk holds there, so the answer never says what a person
    /// keeps beside the drive.
    Untracked { subpath: String },
    /// The path is, or passes through, a symbolic link (`keeper_sync::files_write::agent_write_check`):
    /// an agent's write lands only at the path it names.
    Link { subpath: String },
    /// Something is already at a path an agent's write would make
    /// (`keeper_sync::files_write::agent_write_check`).
    Exists { subpath: String },
    /// Whether an agent may write the path could not be told — the index, an
    /// indexed blob, the ignore rules or the attributes could not be read —
    /// so it may not (`keeper_sync::files_write::agent_write_check` fails closed).
    Undecidable { subpath: String, reason: String },
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
            Self::ChangedSinceRead { relative_path } => write!(
                f,
                "{relative_path} changed while you were saving, so keeper wrote nothing over \
                 it — reopen it and try again."
            ),
            Self::DeleteFailed {
                relative_path,
                reason,
            } => write!(f, "keeper could not delete {relative_path}: {reason}"),
            Self::SessionWorkspace { subpath } => write!(
                f,
                "{subpath} is inside a session's workspace — scratch that is not versioned, \
                 not synced, and dies with the session. keeper reads it but never writes \
                 there; promote the file into the session's artifacts instead."
            ),
            Self::AgentHome { .. } => write!(
                f,
                "That is an agent's home file. Only a person edits it, in the drive itself."
            ),
            Self::SessionsZone { subpath } => write!(
                f,
                "{subpath} is in the sessions zone, which an agent writes only through its \
                 session tools: session_write for a file of its own session, card_update for a \
                 card. drive_write and drive_edit never reach it."
            ),
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
            Self::Machinery { subpath } => write!(
                f,
                "\"{subpath}\" is git's or keeper's own file, which an agent never writes."
            ),
            Self::Ignored { subpath } => write!(
                f,
                "\"{subpath}\" is ignored by this drive, so it would never be part of it."
            ),
            Self::LfsRouted { subpath } => write!(
                f,
                "\"{subpath}\" is kept in the drive's large-file store (LFS), which an agent \
                 never writes."
            ),
            Self::Untracked { subpath } => {
                write!(f, "\"{subpath}\" is not a file this drive carries.")
            }
            Self::Link { subpath } => write!(
                f,
                "\"{subpath}\" is or passes through a link; an agent writes only the file at its \
                 own path."
            ),
            Self::Exists { subpath } => write!(f, "\"{subpath}\" already exists."),
            Self::Undecidable { subpath, reason } => write!(
                f,
                "keeper could not tell whether \"{subpath}\" may be written ({reason}), so an \
                 agent may not write it."
            ),
        }
    }
}

impl From<BrowseRefusal> for WriteRefusal {
    /// Carry a containment refusal across without restating it.
    ///
    /// [`crate::plain_segments`] is the one lexical rule and it already
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

/// One configured subfolder as the folder its root is registered at: the
/// trimmed configuration's own native components, `/`-joined — `.` dropped,
/// every name kept literal — because `SyncProfile::vault_root` and the zones'
/// roots join that same trimmed string with native `Path` semantics. On Unix
/// `notes\.` is ONE folder whose name holds a backslash; reading it as
/// `notes` would check one file and let the writer replace another. `None`
/// for a parent, root or drive component: no folder inside the profile, and
/// a fence that guessed which one would be guessing where bytes land.
fn native_prefix(configured: &str) -> Option<String> {
    let mut parts = Vec::new();
    for component in Path::new(configured.trim()).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => parts.push(name.to_str()?),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

/// The segments of `subpath` past `zone`, when `subpath` is at or under it:
/// each of the zone's segments compared as the Mac's volume compares names
/// ([`names::same_entry_folded`]), because a fence the shift key or a
/// decomposed accent walks around is no fence on the volume keeper ships on.
/// The one zone walk every zone fence reads (AD-65, AD-159).
fn within_zone<'s>(zone: &str, subpath: &'s str) -> Option<Vec<&'s str>> {
    let mut parts = subpath.split('/');
    for zone_part in zone.split('/') {
        if !parts
            .next()
            .is_some_and(|part| names::same_entry_folded(part, zone_part))
        {
            return None;
        }
    }
    Some(parts.collect())
}

/// Where one profile's Files surface may write.
///
/// Constructed per call from the vault the shell can actually reach, so this
/// type cannot outlive the fact it encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteScope<'a> {
    profile_name: &'a str,
    /// The live vault's subfolder inside the profile, as [`native_prefix`]
    /// reads it, or `None` when this profile holds no reachable vault.
    subfolder: Option<String>,
    /// The sessions zone's subfolder inside the profile, read as the vault's
    /// is, or `None` when this profile holds no sessions zone (Phase 7,
    /// AD-113). What it fences is narrow and absolute: any path under a
    /// session's `workspace/` refuses every write, because the zone's own
    /// contract makes that subtree scratch keeper reads and never touches.
    sessions_subfolder: Option<String>,
    /// The agents zone's subfolder inside the profile, read as the vault's
    /// is, or `None` when this profile keeps no agents (AD-361). It fences
    /// every home file from every tool writer (AD-362).
    agents_subfolder: Option<String>,
    /// Whether the whole sessions zone is closed to this scope's writes
    /// (R51) — an agent host's scope, never a person's.
    sessions_closed: bool,
    /// The first configured subfolder [`native_prefix`] could not place
    /// inside the profile: every write in this scope refuses naming it.
    unplaceable: Option<String>,
}

impl<'a> WriteScope<'a> {
    /// A scope over the vault at `subfolder`, or over no vault at all.
    ///
    /// The subfolder is read here and only here, as [`native_prefix`] reads
    /// it: the folder `SyncProfile::vault_root` registers, whatever the user
    /// typed into the settings form — `Notes/`, `./Notes`, ` Notes `,
    /// `notes//daily` all name it, and `NotesConfig::validate` deliberately
    /// refuses rather than corrects, so those spellings survive into the
    /// stored profile. Comparing them raw against a `/`-joined dirent path
    /// would answer "outside the vault" for a vault the user is looking at.
    /// A backslash is a separator only where the platform's paths say so: on
    /// Unix `\Notes` is a folder of that name, which is where the registered
    /// vault writes. A subfolder naming no folder inside the profile — a
    /// `..`, a root or a drive — refuses every write ([`WriteRefusal::Escapes`]
    /// naming it) rather than standing for some folder it might mean.
    ///
    /// Case is deliberately NOT folded: unlike the folder-role marker, which
    /// only decides a glyph, this decides where bytes land, and
    /// `crate::plain_segments` has already guaranteed the subpath's segments
    /// are the dirent's own. Two folders differing only in case are two folders
    /// on the filesystem this is asserted on.
    pub fn new(profile_name: &'a str, subfolder: Option<&str>) -> Self {
        let mut scope = Self {
            profile_name,
            subfolder: None,
            sessions_subfolder: None,
            agents_subfolder: None,
            sessions_closed: false,
            unplaceable: None,
        };
        scope.subfolder = scope.place(subfolder);
        scope
    }

    /// `configured` as [`native_prefix`] reads it, or — when it names no
    /// folder inside the profile — its trimmed spelling, remembered so every
    /// write in this scope refuses ([`Self::placed`]).
    fn place(&mut self, configured: Option<&str>) -> Option<String> {
        let configured = configured?;
        Some(native_prefix(configured).unwrap_or_else(|| {
            let spelled = configured.trim().to_owned();
            self.unplaceable.get_or_insert_with(|| spelled.clone());
            spelled
        }))
    }

    /// Refused when a configured subfolder of this scope names no folder
    /// inside the profile: which file a write would reach is then unknown.
    fn placed(&self) -> Result<(), WriteRefusal> {
        match &self.unplaceable {
            Some(subfolder) => Err(WriteRefusal::Escapes {
                subpath: subfolder.clone(),
            }),
            None => Ok(()),
        }
    }

    /// The same scope, aware of the profile's sessions zone (Phase 7, AD-113).
    ///
    /// A builder step rather than a third constructor argument so the many
    /// call sites that predate sessions read unchanged — a scope built without
    /// this refuses nothing new, which is exactly the compatibility a fence
    /// must have: absent knowledge widens nothing.
    pub fn with_sessions(mut self, sessions_subfolder: Option<&str>) -> Self {
        self.sessions_subfolder = self.place(sessions_subfolder);
        self
    }

    /// The same scope, closed to the whole sessions zone when `closed`
    /// (R51): armed by an agent host only, beside [`Self::with_sessions`],
    /// whose zone it closes — a scope that does not know its zone closes
    /// nothing, as that builder's note says.
    pub fn with_sessions_closed(mut self, closed: bool) -> Self {
        self.sessions_closed = closed;
        self
    }

    /// Whether a profile-relative path lies at or under the sessions zone
    /// this scope keeps closed. Folded per segment, as the agents fence is.
    fn in_closed_sessions(&self, subpath: &str) -> bool {
        let Some(zone) = self
            .sessions_subfolder
            .as_deref()
            .filter(|_| self.sessions_closed)
        else {
            return false;
        };
        within_zone(zone, subpath).is_some()
    }

    /// The same scope, aware of the profile's agents zone (AD-361, AD-362).
    ///
    /// Armed wherever a scope is built for a bot or an agent; a scope built
    /// without it refuses nothing new, as [`Self::with_sessions`] does.
    pub fn with_agents(mut self, agents_subfolder: Option<&str>) -> Self {
        self.agents_subfolder = self.place(agents_subfolder);
        self
    }

    /// Whether a profile-relative path is a file only a person edits in an
    /// agents zone. Every segment is compared as the Mac's volume compares
    /// names ([`names::same_entry_folded`]), as the workspace fence's segment
    /// is: on the case-insensitive, normalization-insensitive volume keeper
    /// ships on, `80-Agents/Nixi/soul.md` is the soul, and so is a zone named
    /// `café` reached as `cafe\u{301}`.
    fn in_agent_home(&self, subpath: &str) -> bool {
        let Some(zone) = self.agents_subfolder.as_deref() else {
            return false;
        };
        let Some(rest) = within_zone(zone, subpath) else {
            return false;
        };
        match rest.as_slice() {
            // The zone's own files and folders: `_drive.toml`, `_skills/`,
            // `_workflows/`, `_template/` and anything else it reserves.
            [first, ..] if first.starts_with('_') => true,
            // Everything in a home is the person's: a soul's `file:` facts
            // name files beside it, and a tool that could rewrite them would
            // rewrite what the agent is told.
            [_home, _, ..] => true,
            _ => false,
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
        self.placed()?;
        let Some(subfolder) = self.subfolder.as_deref() else {
            return Err(WriteRefusal::NoVault {
                profile_name: self.profile_name.to_owned(),
            });
        };
        // Refuse `..`, an absolute path and a platform separator before
        // anything else, so an escape is refused identically whether the vault
        // is where it says it is or not.
        crate::plain_segments(subpath)?;
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
    /// The vault root is allowed here and refused by [`Self::owner`]: a person
    /// may put a note at the top of their vault, and nobody may delete the
    /// vault.
    pub fn directory(&self, subpath: &str) -> Result<String, WriteRefusal> {
        self.vault_relative(subpath)
    }

    /// Where a new file called `name` in the directory `dir_subpath` would
    /// land, or why nowhere.
    ///
    /// Purely lexical: the collision check is `keeper_sync::files_write::collides`, which needs the
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
    ///    question is asked, which is the part that matters: `vault_relative`
    ///    tests for a vault before it tests the path, so in a
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
        // A link committed into the drive may point into an agent's home: the
        // fence asks where the write lands, not only the path it was asked by.
        let landing = root
            .canonicalize()
            .ok()
            .and_then(|root| resolved.strip_prefix(root).ok().map(Path::to_path_buf));
        if let Some(landing) = landing {
            let landing = landing
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if self.in_agent_home(&landing) {
                return Err(WriteRefusal::AgentHome {
                    subpath: subpath.to_owned(),
                });
            }
            if self.in_closed_sessions(&landing) {
                return Err(WriteRefusal::SessionsZone {
                    subpath: subpath.to_owned(),
                });
            }
        }
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

    /// Whether a profile-relative path sits inside a session's `workspace/`
    /// (Phase 7, AD-113): `<zone>/active/<session>/workspace/**` or
    /// `<zone>/archive/<year>/<session>/workspace/**`. Component-wise against
    /// the normalised zone subfolder, exactly as the vault test is.
    ///
    /// **Public because the surface that shows the lock must ask the fence
    /// that enforces it** (FR-254): the session tree renders a read-only mark
    /// on exactly these paths, and a shell-side "the third segment is
    /// `workspace`" would be a second predicate that gets edited once.
    /// Nothing outside [`Self::classify`] may act on the answer; elsewhere it
    /// decides a glyph and a sentence, never a write.
    pub fn in_session_workspace(&self, subpath: &str) -> bool {
        matches!(self.workspace_position(subpath), WorkspacePosition::Inside)
    }

    /// Whether the session tree should render a read-only lock on this entry
    /// (FR-254) — the fence's own answer, one question wider.
    ///
    /// A write lands *in* the workspace, so [`Self::in_session_workspace`] is
    /// what [`Self::classify`] asks and it deliberately excludes the container
    /// itself: a write whose target IS the `workspace` directory is refused as
    /// a directory, which is the more precise thing to say.
    ///
    /// The tree asks a different question — "may keeper write here" — and for
    /// a directory that means "into here". Without this, an EMPTY `workspace/`
    /// would render with nothing said about being scratch, which is exactly
    /// the session where the fence most needs explaining. Same parse, so the
    /// two answers cannot drift apart.
    pub fn session_workspace_lock(&self, subpath: &str, is_dir: bool) -> bool {
        match self.workspace_position(subpath) {
            WorkspacePosition::Inside => true,
            WorkspacePosition::Container => is_dir,
            WorkspacePosition::Outside => false,
        }
    }

    /// The one segment walk both workspace questions read.
    fn workspace_position(&self, subpath: &str) -> WorkspacePosition {
        let Some(zone) = self.sessions_subfolder.as_deref() else {
            return WorkspacePosition::Outside;
        };
        let Some(parts) = within_zone(zone, subpath) else {
            return WorkspacePosition::Outside;
        };
        // The index the workspace segment sits at, per lifecycle layout:
        // `active/<session>/workspace/...`, `archive/<year>/<session>/...`.
        let named = |at: usize, name: &str| {
            parts
                .get(at)
                .is_some_and(|part| names::same_entry_folded(part, name))
        };
        let at = if named(0, "active") {
            2
        } else if named(0, "archive") {
            3
        } else {
            return WorkspacePosition::Outside;
        };
        // Folded, every segment of it, because each is one an operator can
        // make in Finder and the volume keeper ships on (APFS by default)
        // answers `Workspace` and `workspace` — and `Active` and `active` —
        // with the same directory. A case-sensitive `==` here left this fence
        // and `keeper_core::sessions::files::check_dir` sharing one blind
        // spot, so running both predicates bought nothing: the pair only means
        // something if neither can be defeated by the shift key. keeper's own
        // readers spell the lifecycle names one way, so folding them refuses
        // only what lands in the same folder on the Mac.
        if !named(at, "workspace") {
            return WorkspacePosition::Outside;
        }
        if parts.len() > at + 1 {
            WorkspacePosition::Inside
        } else {
            WorkspacePosition::Container
        }
    }

    /// The zones' fences alone, for a path a write may be about to create
    /// ([`Self::create`] asks only the vault): a session's `workspace/`,
    /// an agent's home and the agents zone's own `_` folders, and — in an
    /// agent host's scope — the sessions zone.
    pub fn fenced(&self, subpath: &str) -> Result<(), WriteRefusal> {
        self.placed()?;
        // The workspace fence first (AD-113): scratch refuses every write, on
        // the zone's own contract, before any question about vaults is asked.
        if self.in_session_workspace(subpath) {
            return Err(WriteRefusal::SessionWorkspace {
                subpath: subpath.to_owned(),
            });
        }
        // The agents fence, on the same footing: a home file refuses every
        // tool write, wherever the vault is (AD-362).
        if self.in_agent_home(subpath) {
            return Err(WriteRefusal::AgentHome {
                subpath: subpath.to_owned(),
            });
        }
        // An agent host's scope keeps the whole sessions zone for its
        // session tools (R51).
        if self.in_closed_sessions(subpath) {
            return Err(WriteRefusal::SessionsZone {
                subpath: subpath.to_owned(),
            });
        }
        Ok(())
    }

    /// The fork itself. Everything above it is arguments; everything below it
    /// is consequences.
    fn classify(&self, subpath: &str, is_dir: bool) -> Result<Owned, WriteRefusal> {
        self.fenced(subpath)?;
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

    /// What the vault would have given this file and will not, in the words both
    /// forms of the caveat are built from.
    ///
    /// One string rather than two literals because Story 53.3 added a second
    /// form of the sentence, and the whole of what makes the short one honest is
    /// that it names the SAME absences. Two lists would agree until the day one
    /// of them is edited, and the reader who is shown the short one is exactly
    /// the reader who never sees the other to compare it with.
    const UNMANAGED_ABSENT: &'static str = "no note history, no search index and no conflict copy";

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
    /// This is the form a surface shows on request since Story 53.3 —
    /// [`Self::unmanaged_caveat_short`] is what stands before the first
    /// keystroke. The narrowing is deliberate and it is a narrowing: the fact
    /// never leaves the screen, and neither form is ever paraphrased in the
    /// webview.
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
             the file and sends a delete to this computer's trash: {absent}. Nothing about \
             how {profile} syncs this folder changes.",
            absent = Self::UNMANAGED_ABSENT,
        )
    }

    /// The same standing fact in ONE sentence, for a surface that folds the
    /// caveat away (Story 53.3, FR-318).
    ///
    /// **Composed here rather than clipped there.** The webview renders both
    /// forms verbatim and paraphrases neither (`viewers/types.ts`), and a
    /// truncation is a paraphrase a reader cannot tell from the whole sentence:
    /// cutting this text at a character count lands mid-clause, and the clause
    /// it would land in is the one naming what is absent.
    ///
    /// **It keeps the head and the absences and drops everything else.** What
    /// goes is where the vault is, which writer runs, and the sentence about
    /// syncing — facts that qualify the caveat rather than state it. What stays
    /// is that this file is not one of keeper's notes and that the three things
    /// a note would have gained are not there, which is the whole of what AD-102
    /// says a reader must know before the first keystroke. Fold it to one line;
    /// never to nothing.
    pub fn unmanaged_caveat_short(&self, name: &str) -> String {
        format!(
            "{name} is not one of keeper's notes: {absent}.",
            absent = Self::UNMANAGED_ABSENT,
        )
    }
}

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

/// Where a path sits relative to a session's `workspace/` (AD-113).
///
/// Private: the distinction exists so the fence and the tree's lock can share
/// one parse, and both public answers are `bool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspacePosition {
    /// Under the workspace — every write refuses.
    Inside,
    /// The `workspace` directory itself.
    Container,
    Outside,
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
/// exists and why it is opaque — a `PathBuf` arriving at `keeper_sync::files_write::write_unmanaged`
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
    /// Canonical destination for the unmanaged writer, never a display label.
    ///
    /// **Only keeper-sync's effect writers may call this** —
    /// `keeper_sync::files_write::write_unmanaged` and the operating-system
    /// trash path in the same module. It is public only because those effects
    /// stayed in keeper-sync while the routing that proves the path moved here,
    /// and Rust has no visibility narrower than `pub` that crosses a crate.
    /// Construction stays crate-private, so the accessor cannot launder an
    /// unproven path; but a reader that takes this `&Path` anywhere else — a
    /// view model, a log line, a second writer — puts the absolute path on a
    /// surface FR-145 keeps it off and opens a write path AD-102 does not name.
    /// Show [`Self::profile_relative`] instead.
    pub fn absolute(&self) -> &Path {
        &self.absolute
    }

    /// What the surface already shows, for a sentence and for a log line.
    ///
    /// FR-145 keeps the absolute path off screen.
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
    /// Inside the profile, outside every vault: `keeper_sync::files_write::write_unmanaged` and
    /// `keeper_sync::files_write::trash_unmanaged`, over a surface that said so first.
    Unmanaged(UnmanagedPath),
}

/// Resolve a profile-relative subpath that must already be on disk.
///
/// [`crate::resolve`]'s `Ok(None)` — the lexical test passed and there is
/// nothing there — becomes [`WriteRefusal::Missing`] here, because a write path
/// has no use for the distinction the listing draws: whether the drive is out
/// or the file was moved, this file is not the one being deleted, and deleting
/// something else would be the worst possible recovery.
pub fn resolve_existing(root: &Path, subpath: &str) -> Result<PathBuf, WriteRefusal> {
    crate::resolve(root, subpath)?.ok_or_else(|| WriteRefusal::Missing {
        subpath: subpath.to_owned(),
    })
}

/// The last `/`-separated component of a subpath, for a sentence that names the
/// thing rather than the route to it.
pub fn last_segment(subpath: &str) -> &str {
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
    if crate::plain_segments(name).map(|parts| parts.len()) != Ok(1) {
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
        .any(|reserved| names::same_entry_folded(name, reserved))
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

    /// The workspace fence (Phase 7, AD-113): under a session's `workspace/`
    /// every write refuses, in both zone layouts, and the fence is exactly as
    /// narrow as the contract — the session's other subtrees, the zone's own
    /// furniture, and a folder merely NAMED workspace elsewhere all pass.
    #[test]
    fn a_session_workspace_refuses_writes_and_nothing_beside_it_does() {
        let scope = WriteScope::new("tgdrive", Some("10-notes")).with_sessions(Some("60-sessions"));
        let refused = |subpath: &str| {
            matches!(
                scope.classify(subpath, false),
                Err(WriteRefusal::SessionWorkspace { .. })
            )
        };
        // Inside the fence, both lifecycle locations, any depth.
        assert!(refused(
            "60-sessions/active/2026-08-10-keeper/workspace/iter.md"
        ));
        assert!(refused(
            "60-sessions/active/2026-08-10-keeper/workspace/deep/scratch.csv"
        ));
        assert!(refused(
            "60-sessions/archive/2025/2025-03-01-taxes/workspace/x.md"
        ));
        // Outside it: the record subtrees are ordinary writable paths.
        assert!(!refused("60-sessions/active/2026-08-10-keeper/README.md"));
        assert!(!refused(
            "60-sessions/active/2026-08-10-keeper/artifacts/report.md"
        ));
        assert!(!refused(
            "60-sessions/active/2026-08-10-keeper/prompts/01-scope.md"
        ));
        // The bare `workspace` directory name is the container, not a file in
        // it; a directory refusal is `IsDirectory`'s business, not the fence's.
        assert!(!refused("60-sessions/active/2026-08-10-keeper/workspace"));
        // A folder named workspace ANYWHERE else is somebody's own folder.
        assert!(!refused("30-work/project/workspace/notes.md"));
        assert!(
            !refused("60-sessions/_template/workspace/.gitkeep") || {
                // The template's workspace is not under active/ or archive/, so
                // the fence does not claim it — the template is user data.
                false
            }
        );
        // A scope that never learned about sessions refuses nothing new.
        let unaware = WriteScope::new("tgdrive", Some("10-notes"));
        assert!(!matches!(
            unaware.classify("60-sessions/active/x/workspace/y.md", false),
            Err(WriteRefusal::SessionWorkspace { .. })
        ));
    }

    /// The tree's lock is the fence, one question wider (FR-254): it adds the
    /// `workspace` DIRECTORY, so an empty workspace still explains itself,
    /// and adds nothing else — the two answers come off one parse.
    #[test]
    fn the_tree_lock_covers_the_workspace_container_and_the_fence_does_not() {
        let scope = WriteScope::new("tgdrive", Some("60-notes")).with_sessions(Some("60-sessions"));
        let dir = "60-sessions/active/2026-08-10-keeper/workspace";
        let file = "60-sessions/active/2026-08-10-keeper/workspace/iter.md";

        // A write INTO the workspace refuses; the container is a directory
        // refusal, which is the more precise thing for a writer to say.
        assert!(scope.in_session_workspace(file));
        assert!(!scope.in_session_workspace(dir));

        // The tree locks both, because "may keeper write here" over a folder
        // means "into here".
        assert!(scope.session_workspace_lock(file, false));
        assert!(scope.session_workspace_lock(dir, true));

        // …and a FILE that happens to be named `workspace` beside the sections
        // is somebody's own file, not the container.
        assert!(!scope.session_workspace_lock(dir, false));

        // Everything outside stays unlocked, in both layouts.
        assert!(
            !scope.session_workspace_lock("60-sessions/active/2026-08-10-keeper/README.md", false)
        );
        assert!(!scope.session_workspace_lock("30-work/project/workspace", true));
        assert!(scope
            .session_workspace_lock("60-sessions/archive/2025/2025-03-01-taxes/workspace", true));
    }

    /// The fence is the DIRECTORY, not one spelling of its name (AD-113).
    ///
    /// On APFS's default case-insensitive volume `Workspace/iter.md` is a file
    /// in the fenced scratch, so a fence that only recognised the lowercase
    /// spelling let a write land there — and it was the *second* of the two
    /// predicates `sessions_dir_new` runs, which is why running two bought
    /// nothing while both were blind. Asserted through all three public
    /// questions, because they are one parse and a fold applied to one of them
    /// would be the drift this helper exists to prevent.
    #[test]
    fn the_workspace_fence_reads_the_segment_folded() {
        let scope = WriteScope::new("tgdrive", Some("60-notes")).with_sessions(Some("60-sessions"));
        for cased in ["Workspace", "WORKSPACE", "WorkSpace"] {
            let file = format!("60-sessions/active/2026-08-10-keeper/{cased}/iter.md");
            let dir = format!("60-sessions/active/2026-08-10-keeper/{cased}");
            assert!(
                scope.in_session_workspace(&file),
                "{file} is the fenced scratch on the volume keeper ships on"
            );
            assert!(matches!(
                scope.classify(&file, false),
                Err(WriteRefusal::SessionWorkspace { .. })
            ));
            assert!(scope.session_workspace_lock(&dir, true), "{dir}");
        }
        // The neighbour that merely starts with the word is not the container,
        // at any capitalisation: the fence is a whole segment.
        assert!(!scope
            .in_session_workspace("60-sessions/active/2026-08-10-keeper/Workspace-notes/plan.md"));
    }

    /// The agents fence (AD-362): every home file and the zone's own files
    /// refuse every tool write, folded as the workspace fence is; the zone's
    /// guide and an agent's other files are routed as before.
    #[test]
    fn an_agents_fence_refuses_every_home_file_and_leaves_the_rest_alone() {
        let scope = WriteScope::new("tgdrive", Some("10-notes"))
            .with_sessions(Some("60-sessions"))
            .with_agents(Some("80-agents"));
        let refused = |subpath: &str| {
            matches!(
                scope.classify(subpath, false),
                Err(WriteRefusal::AgentHome { .. })
            )
        };
        for home_file in [
            "80-agents/_drive.toml",
            "80-agents/_skills/x/SKILL.md",
            "80-agents/_workflows/w/workflow.toml",
            "80-agents/_template/agent.toml",
            "80-agents/nixi/agent.toml",
            "80-agents/nixi/SOUL.md",
            "80-agents/nixi/USER.md",
            "80-agents/nixi/MEMORY.md",
            "80-agents/nixi/journal/2026-10-02.electra.md",
            "80-agents/nixi/proposals/01JABCDEFGHJKMNPQRSTVWXYZ0.md",
            "80-Agents/Nixi/soul.md",
            "80-AGENTS/nixi/Agent.TOML",
            "80-agents/nixi/Journal/2026-10-02.electra.md",
            "80-agents/_Skills/x/SKILL.md",
            "80-agents/nixi/notes.md",
            "80-agents/nixi/notes/standing-orders.md",
            "80-agents/nixi/drafts/SOUL.md",
        ] {
            assert!(refused(home_file), "{home_file} is a home file");
        }
        for routed in [
            "80-agents/README.md",
            "80-agents/AGENTS.md",
            "80-agents-old/nixi/SOUL.md",
            "10-notes/nixi/SOUL.md",
            "SOUL.md",
        ] {
            assert!(!refused(routed), "{routed} is not a home file");
        }
        assert!(matches!(
            scope.classify("80-agents/README.md", false),
            Ok(Owned::Unmanaged)
        ));
        assert_eq!(
            WriteRefusal::AgentHome {
                subpath: "80-agents/nixi/SOUL.md".to_owned()
            }
            .to_string(),
            "That is an agent's home file. Only a person edits it, in the drive itself."
        );

        // A scope that never learned about agents refuses nothing new.
        let unaware =
            WriteScope::new("tgdrive", Some("10-notes")).with_sessions(Some("60-sessions"));
        assert!(matches!(
            unaware.classify("80-agents/nixi/SOUL.md", false),
            Ok(Owned::Unmanaged)
        ));
    }

    /// A link elsewhere in the drive that lands in an agent's home is
    /// refused where it lands; a plain file beside it is routed as before.
    #[cfg(unix)]
    #[test]
    fn a_link_into_an_agent_home_is_refused_where_it_lands() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("80-agents/nixi");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(home.join("SOUL.md"), "soul").expect("soul");
        std::fs::write(root.path().join("plain.md"), "plain").expect("plain");
        std::os::unix::fs::symlink("80-agents/nixi/SOUL.md", root.path().join("x.md"))
            .expect("link");
        let scope = WriteScope::new("tgdrive", Some("10-notes")).with_agents(Some("80-agents"));
        assert!(matches!(
            scope.route(None::<FakeVault>, root.path(), "x.md"),
            Err(WriteRefusal::AgentHome { .. })
        ));
        assert!(matches!(
            scope.route(None::<FakeVault>, root.path(), "plain.md"),
            Ok(WriteRoute::Unmanaged(_))
        ));
    }

    /// R51: an agent host's scope refuses every write in the sessions zone —
    /// cards, logs, `agent.toml` — folded as the agents fence is, at the
    /// path asked for and where a link lands; a person's scope (the fence
    /// unarmed) routes the same card as before.
    #[cfg(unix)]
    #[test]
    fn an_agent_hosts_scope_closes_the_sessions_zone() {
        let agent = WriteScope::new("tgdrive", Some("10-notes"))
            .with_sessions(Some("60-sessions"))
            .with_agents(Some("80-agents"))
            .with_sessions_closed(true);
        let refused = |subpath: &str| {
            matches!(
                agent.classify(subpath, false),
                Err(WriteRefusal::SessionsZone { .. })
            )
        };
        for closed in [
            "60-sessions/active/s/brief.md",
            "60-sessions/active/s/cards/inbox.md",
            "60-sessions/active/s/log/2026-10-04.electra.1.jsonl",
            "60-sessions/active/s/agent.toml",
            "60-sessions/archive/2026/s/brief.md",
            "60-Sessions/active/s/brief.md",
            "60-sessions/AGENTS.md",
        ] {
            assert!(refused(closed), "{closed} is in the sessions zone");
        }
        for open in ["60-sessions-old/a.md", "10-notes/60-sessions/a.md", "a.md"] {
            assert!(!refused(open), "{open} is not in the sessions zone");
        }
        let person = WriteScope::new("tgdrive", Some("10-notes"))
            .with_sessions(Some("60-sessions"))
            .with_agents(Some("80-agents"));
        assert!(matches!(
            person.classify("60-sessions/active/s/brief.md", false),
            Ok(Owned::Unmanaged)
        ));

        let root = tempfile::tempdir().expect("tempdir");
        let session = root.path().join("60-sessions/active/s");
        std::fs::create_dir_all(&session).expect("session");
        std::fs::write(session.join("brief.md"), "card").expect("card");
        std::os::unix::fs::symlink("60-sessions/active/s/brief.md", root.path().join("x.md"))
            .expect("link");
        assert!(matches!(
            agent.route(None::<FakeVault>, root.path(), "x.md"),
            Err(WriteRefusal::SessionsZone { .. })
        ));
    }

    /// The refusal's sentence speaks the zone's own words — the promote path
    /// is the answer, not a generic "not allowed".
    #[test]
    fn the_workspace_refusal_names_the_path_and_points_at_promotion() {
        let refusal = WriteRefusal::SessionWorkspace {
            subpath: "60-sessions/active/x/workspace/draft.md".to_owned(),
        };
        let sentence = refusal.to_string();
        assert!(sentence.contains("workspace"), "{sentence}");
        assert!(sentence.contains("promote"), "{sentence}");
        assert!(
            sentence.contains("60-sessions/active/x/workspace/draft.md"),
            "{sentence}"
        );
    }

    /// The whole of the "outside a vault cannot be written" rule: a profile
    /// with no vault has nowhere keeper may *create* a note, and says which
    /// profile it is refusing about rather than "not allowed".
    ///
    /// AD-102 is the other half and it is asserted here so the two cannot
    /// drift: the same profile does not refuse an *existing* file any more, it
    /// routes it to the second writer. `directory` is the create path and
    /// still answers `NoVault`; `owner` answers `Unmanaged`.
    #[test]
    fn a_profile_with_no_vault_refuses_every_path_and_names_itself() {
        let none = WriteScope::new("Field", None);
        assert!(!none.holds_vault());
        let refusal = none.directory("clip.mov").expect_err("no vault");
        assert_eq!(
            refusal,
            WriteRefusal::NoVault {
                profile_name: "Field".to_owned()
            }
        );
        assert!(refusal.to_string().contains("Field holds no notes vault"));
        assert!(none.directory("").is_err());
        assert!(none.create("", "x.md").is_err());
        assert_eq!(
            none.owner("clip.mov", false),
            Ok(WriteOwner::Unmanaged),
            "AD-102: no vault is no longer a refusal for an existing file"
        );
    }

    /// A sibling whose name merely *starts with* the vault's is not in the
    /// vault. A `starts_with` over the raw string would have written into it —
    /// and post-AD-102 it would have sent a neighbour's file down the *vault*
    /// writer, so the component match is asserted through both surfaces that
    /// depend on it.
    #[test]
    fn a_folder_whose_name_extends_the_vaults_is_not_inside_the_vault() {
        let scope = scope();
        assert_eq!(scope.directory("10-notes/a.md"), Ok("a.md".to_owned()));
        let refusal = scope
            .directory("10-notes-archive/a.md")
            .expect_err("neighbour");
        assert!(matches!(refusal, WriteRefusal::OutsideVault { .. }));
        assert!(
            refusal.to_string().contains("outside Vault's notes vault"),
            "{refusal}"
        );
        assert_eq!(scope.owner("10-notes/a.md", false), Ok(WriteOwner::Vault));
        assert_eq!(
            scope.owner("10-notes-archive/a.md", false),
            Ok(WriteOwner::Unmanaged)
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
        assert_eq!(deep.directory("a/b/c/d.md"), Ok("c/d.md".to_owned()));
        assert!(matches!(
            deep.directory("a/c/d.md"),
            Err(WriteRefusal::OutsideVault { .. })
        ));
        assert!(matches!(
            deep.directory("a"),
            Err(WriteRefusal::OutsideVault { .. })
        ));
    }

    /// The stored subfolder is whatever the user typed, and `NotesConfig`
    /// refuses rather than corrects — so `Notes/`, `./Notes` and `a//b` all
    /// reach here verbatim. Read as the native path `vault_root` joins, every
    /// spelling of one folder names that folder, which is what keeps the
    /// surface from refusing to write in the one folder it exists to write in.
    #[test]
    fn the_configured_subfolder_is_read_as_the_folder_its_root_is_joined_at() {
        for spelling in [
            "10-notes",
            "10-notes/",
            "./10-notes",
            " 10-notes ",
            "10-notes/.",
        ] {
            let scope = WriteScope::new("Vault", Some(spelling));
            assert_eq!(
                scope.directory("10-notes/a.md"),
                Ok("a.md".to_owned()),
                "{spelling}"
            );
        }
        for spelling in ["a/b", "a//b", "./a/./b/"] {
            let scope = WriteScope::new("Vault", Some(spelling));
            assert_eq!(
                scope.directory("a/b/c.md"),
                Ok("c.md".to_owned()),
                "{spelling}"
            );
        }
        // The sentence names the normalised form, so two profiles configured
        // with two spellings of one folder read the same to a person.
        let refusal = WriteScope::new("Vault", Some("10-notes/"))
            .directory("other/a.md")
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

    /// A subfolder that names no folder inside the profile — a root, a parent
    /// — is not read as whichever folder it might mean: every write in that
    /// scope refuses, naming it, whether it is the vault or a zone.
    #[test]
    fn a_subfolder_naming_no_folder_in_the_profile_refuses_every_write() {
        for spelling in ["/10-notes", "../10-notes", "10-notes/../x"] {
            let refused = Err(WriteRefusal::Escapes {
                subpath: spelling.to_owned(),
            });
            let vault = WriteScope::new("Vault", Some(spelling));
            assert_eq!(
                vault.directory("10-notes/a.md").map(drop),
                refused,
                "{spelling}"
            );
            assert_eq!(
                vault.owner("10-notes/a.md", false).map(drop),
                refused,
                "{spelling}"
            );
            assert_eq!(
                vault.owner("other/a.md", false).map(drop),
                refused,
                "{spelling}"
            );
            let zone = WriteScope::new("Vault", Some("10-notes")).with_agents(Some(spelling));
            assert_eq!(
                zone.directory("10-notes/a.md").map(drop),
                refused,
                "{spelling}"
            );
            assert_eq!(zone.fenced("10-notes/a.md"), refused, "{spelling}");
        }
    }

    /// On Unix a backslash is a filename character, so `notes\.` is ONE
    /// folder — the one `vault_root` registers — and `notes/` beside it is
    /// not the vault: a create there refuses and an existing file there goes
    /// to the plain writer at its own path, never to the vault's writer.
    #[cfg(unix)]
    #[test]
    fn a_backslash_in_a_unix_subfolder_is_part_of_the_folders_name() {
        let scope = WriteScope::new("Vault", Some("notes\\."));
        assert!(matches!(
            scope.directory("notes/keep.md"),
            Err(WriteRefusal::OutsideVault { .. })
        ));
        assert_eq!(
            scope.owner("notes/keep.md", false),
            Ok(WriteOwner::Unmanaged)
        );
        assert!(matches!(
            WriteScope::new("Vault", Some("a\\b")).directory("a/b/c.md"),
            Err(WriteRefusal::OutsideVault { .. })
        ));
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
            let refusal = scope.owner(subpath, false).expect_err(subpath);
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
        let refusal = scope().owner("10-notes", true).expect_err("vault root");
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
            .owner("10-notes/notes.md", true)
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
            scope().owner("10-notes/notes.md", false),
            Ok(WriteOwner::Vault)
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

        // `\u{212A}` is the Kelvin sign, which decomposes to `K`: on the Mac's
        // volume `.\u{212A}eeper` is the `.keeper` folder.
        for name in [".keeper", ".obsidian", ".git", ".KEEPER", ".\u{212A}eeper"] {
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

    /// The one-line form Story 53.3 folds the caveat to (FR-318).
    ///
    /// **The fold is a narrowing of AD-102 and this is where that is enforced.**
    /// What the short form must keep is the head — this file is not one of
    /// keeper's notes — and the three absences, because those are the whole of
    /// what a reader has to know before the first keystroke. What it must drop
    /// is everything that qualifies rather than states: where the vault is,
    /// which writer runs, and the clause about syncing.
    ///
    /// **And it must be composed here.** The webview renders it verbatim, so a
    /// short form that re-worded the absences would be a second copy of these
    /// words with nothing keeping the two in step — which is why both forms are
    /// built from [`WriteScope::UNMANAGED_ABSENT`] and this asserts that they
    /// are.
    #[test]
    fn the_short_caveat_still_names_what_is_missing() {
        let short = scope().unmanaged_caveat_short("AGENTS.md");
        let full = scope().unmanaged_caveat("AGENTS.md");

        assert!(
            short.contains("AGENTS.md is not one of keeper's notes"),
            "{short}"
        );
        for absent in ["no note history", "no search index", "no conflict copy"] {
            assert!(short.contains(absent), "{short}");
        }
        // Both forms say what is missing in the SAME words, which is what stops
        // the folded sentence drifting away from the one it folds.
        assert!(short.contains(WriteScope::UNMANAGED_ABSENT), "{short}");
        assert!(full.contains(WriteScope::UNMANAGED_ABSENT), "{full}");

        // ONE sentence: a full stop followed by a space is a second one. The
        // file's own name carries a dot, so counting dots would count that.
        assert!(!short.contains(". "), "{short}");
        // The qualifiers the fold takes away, and the overstatement neither form
        // may ever make.
        for dropped in [
            "notes vault (10-notes)",
            "this computer's trash",
            "Nothing about how",
            "will not sync",
            "does not sync",
        ] {
            assert!(!short.contains(dropped), "{short}");
        }
        // A file with no vault at all gets the same one line: the placing is what
        // the two full forms differ by, and the short form does not carry it.
        assert_eq!(
            WriteScope::new("Field", None).unmanaged_caveat_short("clip.txt"),
            WriteScope::new("Vault", Some("10-notes")).unmanaged_caveat_short("clip.txt"),
        );
    }
}

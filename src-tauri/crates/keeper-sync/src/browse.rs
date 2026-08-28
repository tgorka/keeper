//! Read-only browsing of a synced folder, one directory at a time (Story 43.8,
//! FR-153, AD-74).
//!
//! **AD-75 — "the files surface never writes" — was retired by AD-89 (Story
//! 45.3), and this module is still read-only.** The reversal did not relax
//! anything here: browsing still takes a `&SyncProfile` and still cannot spend
//! an engine verdict. What it added is a *separate* module,
//! [`crate::files_write`], which decides where the Files surface may write and
//! refuses everywhere else. Read that module's doc for why the decision was
//! reversed and what replaced it; the argument below is about looking, and it
//! still holds.
//!
//! # Why this lives here and not in the shell
//!
//! The Files tab asks a question only this crate can answer without guessing:
//! *what is in this folder right now, minus the noise sync already knows to
//! skip, and is the drive even attached?* Both halves of that already exist
//! here — [`crate::exclude::ExcludeSet`] and [`crate::volume::scan`] — and
//! putting the listing beside them means the containment rule, the exclusion
//! rule and the absent-media answer are all asserted over a temp directory on
//! any machine, which is the only way a security rule is ever actually tested.
//! A copy of the rule in the Tauri shell would be a copy that compiles on one
//! platform and is proved on none.
//!
//! # Read-only twice over
//!
//! AD-75 says a browser must never move a file the user did not ask it to move,
//! and that is the obvious half. The half that is easy to get wrong is that a
//! browser must also never move the *engine*: nothing in this module touches
//! [`crate::engine::Engine`], the stability gate, `file_state`, the journal, the
//! scan clock or the watcher. Looking is not an event. Two bugs in this
//! codebase already had the shape "a caller that only meant to LOOK also made
//! the engine forget", and a listing that reached for a convenient engine
//! helper would be the third. Everything here is `std::fs` reads plus one
//! marker read, and it takes a `&SyncProfile` rather than an `&Engine`
//! precisely so it *cannot* make that mistake.
//!
//! # A listing is not a licence to serve bytes
//!
//! Nothing here opens a file. The Files tab reveals, copies a path, or hands a
//! file to the system handler; the `keeper-recording://` protocol stays rooted
//! at the recordings destination and is not widened to reach a synced folder
//! (AD-74). This module hands out names, and the actions on those names are the
//! ones the shell already had.
//!
//! # Saying whether a file is synced, without becoming a second answer
//!
//! Story 44.17 puts a state on every entry, and the state is *read*, never
//! recomputed here. [`crate::engine::Engine::pending`] is already the one
//! derived answer to "what has this folder not synced yet, and why" — it reads
//! `file_state` for what is settling and git for what is uncommitted, and it
//! is documented as computed-never-stored precisely so a visible answer cannot
//! drift from the real one. This module takes that list as an argument
//! ([`PendingView`]) and joins it to the dirents it just read. It does not open
//! a repository, run a status walk or touch the journal, which is the same
//! reason it takes a `&SyncProfile` and not an `&Engine`: a browser that could
//! reach the engine is a browser that will eventually spend something.
//!
//! # Reading a pointer without opening a repository
//!
//! Story 56.2 puts the honest size of a virtual path on every row, and the
//! honest size lives inside the LFS pointer rather than in the worktree's
//! `stat`: an unmaterialized 4 GiB recording is about 130 bytes on disk, and
//! reporting those 130 bytes is the defect (FR-336). The obvious source is
//! [`crate::lfs::stage::indexed_pointer`], and it is the wrong one *here* — it
//! needs a `gix::Repository`, and opening one is precisely what the section
//! above forbids.
//!
//! It is also unnecessary. The worktree bytes of an unmaterialized LFS path
//! **are** the committed pointer, byte for byte (FR-331, established by Story
//! 56.1) — that identity is what keeps `git status` clean over a virtual file
//! at all — so parsing those bytes is the same answer with no repository in
//! it. [`crate::lfs::stage::worktree_pointer`] is that parse.
//!
//! **What it costs, stated rather than waved at.** A listing still pays exactly
//! one `stat` per dirent; the pointer probe is an *additional* open-and-read,
//! bounded to a pointer's 1 KiB ceiling, and it is real — the folders keeper is
//! for are full of small text files, and a probe per dirent would be one open
//! per note on precisely the removable and network volumes this module binds
//! its `stat` once for. So the probe is not run per dirent: [`classify`] asks
//! for it only where its answer can change the row. That is a row whose mark
//! would otherwise be [`EntrySyncStatus::Synced`], and — since Story 56.7 — a
//! row the journal says has content arriving, where pointer text on disk is
//! the whole of what separates [`EntrySyncStatus::Materializing`] from an
//! ordinary [`EntrySyncStatus::Waiting`]. Both of those rows take the
//! pointer's number when the probe finds one, which is what keeps
//! [`BrowseEntry::lfs_oid`] meaning exactly "this row's size came from a
//! pointer".

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::ops::Bound;
use std::path::{Component, Path, PathBuf};

use crate::engine::{PendingFile, PendingReason};
use crate::exclude::{ExcludeSet, ExcludeVerdict};
use crate::lfs::stage;
use crate::profile::SyncProfile;
use crate::volume::{self, VolumeStatus};

/// The most entries one listing will return.
///
/// A synced folder can hold a hundred thousand files, and lazy expansion only
/// bounds the *depth* of what the surface asks for — one flat directory with
/// fifty thousand entries in it is still one call, and serialising it through
/// the IPC boundary would stall the webview for seconds to render a list nobody
/// can read. The cap is reported rather than hidden ([`BrowseDirectory::truncated`]):
/// a browser that silently drops entries is worse than one that says it did.
pub const LISTING_CAP: usize = 1000;

/// What one entry in a browsed directory is, in terms this crate can produce.
///
/// Deliberately not a view model: `keeper-sync` is `keeper-core`-free (AD-40),
/// so the attachment vocabulary — which is `keeper-core`'s
/// `RecordingNoteTargetKind` and belongs to exactly one place (AD-73) — is
/// applied by the shell when it projects these into VMs. What this crate knows
/// is the one thing a `read_dir` knows and an extension table cannot: whether
/// the entry is a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseEntry {
    /// The entry's own file name, with no path in it.
    pub name: String,
    /// The entry's path relative to the profile root, `/`-joined on every
    /// platform — the same frame [`ExcludeSet::is_excluded`] matches in, and the
    /// only path shape that ever crosses to the frontend (FR-145). Feeding it
    /// straight back as the `subpath` of the next call is what makes expansion
    /// lazy without the caller ever composing a path (AD-65).
    pub relative_path: String,
    /// The same entry as an absolute path, joined here onto the resolved
    /// directory and nowhere else. AD-65 asks that a root and a subpath meet in
    /// exactly one place; this is that place for the Files tab, and the shell
    /// above only stringifies what it is handed. Only ever an action's
    /// argument — never shown, never written into a note (FR-145).
    pub absolute_path: PathBuf,
    /// Whether the entry is a directory, following symlinks — a symlink to a
    /// folder expands like the folder it points at, and [`resolve`] refuses it
    /// on the next call if it points outside the root.
    pub is_dir: bool,
    /// The entry's length in bytes for a regular file, `None` for a directory
    /// and for anything whose metadata could not be read (Story 45.5, FR-178).
    ///
    /// **For a path this module marks [`EntrySyncStatus::Virtual`] or
    /// [`EntrySyncStatus::Materializing`] this is the pointer's number, not the
    /// worktree's** (Stories 56.2 and 56.7, FR-336, AD-127). The bytes on disk
    /// are the committed LFS pointer — about 130 of them — and the size a
    /// person actually asked about is the one written inside it. Reporting the
    /// 130 is what made a 4 GiB recording render as a rounding error on every
    /// keeper surface. [`Self::lfs_oid`] is `Some` exactly when this field came
    /// from a pointer, so nothing has to infer it from the magnitude.
    ///
    /// **Tied to the mark, and only to the mark.** A file that is untracked or
    /// excluded and happens to hold pointer text keeps its own length: those
    /// bytes name content no remote was ever told about, and a fabricated
    /// four-gigabyte figure on a row with no sentence to account for it would be
    /// the same lie in the other direction. [`classify`]'s precedence is what
    /// decides, and it decides once for the mark and this field together. The
    /// deliberate consequence is that a `Waiting { Incoming }` row reports the
    /// length of the bytes that are actually there — and it reports them
    /// *because* those bytes are not a pointer, which is the same evidence that
    /// made the mark `Waiting` rather than `Materializing`. An `Incoming` row
    /// over pointer text is content arriving and takes the pointer's number;
    /// one over anything else names no pointer whose number could be
    /// substituted, and [`PendingReason::Incoming`] carries the content's size
    /// itself for the surface that wants it.
    ///
    /// **Free.** The `stat` behind it is the same one `is_dir` already paid
    /// for; this field costs the listing nothing beyond a `u64` per row. The
    /// pointer read that can override it is bounded by that same `stat` —
    /// nothing whose length is outside `(0, MAX_POINTER_BYTES]` is opened — and
    /// is not itself free, which is why [`classify`] asks for it only on the
    /// rungs whose answer it can change.
    ///
    /// **`None` for a directory on purpose.** `metadata().len()` answers for a
    /// directory on every platform this runs on, and the number it gives is the
    /// size of the directory's own bookkeeping — not of what is inside it.
    /// Carrying it would hand a surface a plausible-looking number that means
    /// nothing, and the only honest total for a folder needs a recursive walk
    /// this module must never do. A folder's size is not slow here; it is
    /// absent.
    pub size_bytes: Option<u64>,
    /// The object id inside the LFS pointer this entry's bytes are, if they
    /// are one (Story 56.2, FR-336).
    ///
    /// `Some` exactly when [`Self::size_bytes`] is the pointer's number rather
    /// than the worktree's, which makes this the field that says *why* a
    /// 130-byte file reports four gigabytes. It is also the handle every later
    /// verb in this epic needs — the store path, the batch request and the
    /// `materialized` ledger row are all keyed by oid — carried on the row
    /// rather than re-derived, because re-deriving it means reading the file
    /// again.
    ///
    /// `None` for a directory, for an ordinary file, and for an entry whose
    /// metadata could not be read. Among rows whose bytes really are a pointer,
    /// `Some` on the marks that were decided *by* the probe and keep its
    /// answer — [`EntrySyncStatus::Virtual`] and
    /// [`EntrySyncStatus::Materializing`] — because the size substitution and
    /// this field are decided in one place ([`classify`]) and in one step. The
    /// rule that outlives any list of marks is the one above: `Some` exactly
    /// when [`Self::size_bytes`] is the pointer's number.
    pub lfs_oid: Option<String>,
    /// When this entry was last written, in milliseconds since the Unix epoch
    /// (Story 56.2, FR-340).
    ///
    /// **Free for the same reason [`Self::size_bytes`] is**: it comes off the
    /// one `stat` the listing already binds, so a thousand-entry listing pays
    /// exactly the syscalls it paid before this field existed.
    ///
    /// **Carried for a directory, unlike the size**, and the two are not
    /// inconsistent: a folder's mtime is a real fact about the folder — when
    /// something in it last changed — while `metadata().len()` for a folder is
    /// a fact about its own bookkeeping and about nothing anybody asked. The
    /// honesty rule is one rule; it gives different answers because the
    /// underlying facts differ.
    ///
    /// `None` when the metadata could not be read, or when the platform would
    /// not say. An absence rather than a plausible zero, which is the choice
    /// this whole struct already makes for an unknown size. **Negative** for a
    /// file whose mtime predates 1970 — a restored archive, an extracted
    /// tarball — because that date is known and reporting it as unknown would be
    /// the same invention in the other direction.
    pub mtime_ms: Option<i64>,
    /// What sync knows about this entry right now (Story 44.17, FR-173).
    pub sync: EntrySyncStatus,
    /// Set when the entry's own name is not valid UTF-8 (Story 47.2).
    ///
    /// **`Some` means [`Self::name`] and [`Self::relative_path`] are renderings
    /// rather than names.** They are still filled in, because a hole in a
    /// browser is worse than a mangled row and the user has to be told the file
    /// is there at all — that silence is the whole reported defect. But they no
    /// longer address anything: [`plain_segments`] refuses to join a subpath
    /// carrying the replacement character, so feeding [`Self::relative_path`]
    /// back returns [`BrowseRefusal::Unspellable`] instead of expanding, and
    /// [`crate::files_write`] refuses through the same rule.
    ///
    /// [`Self::absolute_path`] is the exception and the only handle that still
    /// works: it is the `OsString` `read_dir` produced, never decoded, so
    /// reveal-in-Finder and open-with keep working on a file nobody can spell.
    ///
    /// `None` for every ordinary entry, so a surface that ignores this field
    /// behaves exactly as it did before it existed.
    pub unspellable: Option<crate::names::UnspellableName>,
}

/// What the engine's own state says about one browsed entry.
///
/// Answers a person acts on differently, and the distinction the story that
/// created this type turns on is [`Self::Excluded`] against [`Self::Waiting`]:
/// a file that will never be carried, rendered as one that is about to be, is
/// a file somebody waits for forever. [`Self::NotInRepository`] is a third way
/// a file can fail to arrive and it has its own next step — the folder has no
/// repository, so nothing in it is going anywhere until the first sync adopts
/// it.
///
/// [`Self::Unknown`] exists for the same reason [`BrowseListing`] separates an
/// absent drive from an empty folder: when the engine could not answer, every
/// other value would be a claim this module has no grounds for. Guessing
/// [`Self::Synced`] tells someone their work is safe; guessing
/// [`Self::Waiting`] tells them to keep waiting. Neither is honest.
///
/// [`Self::Virtual`], [`Self::Materializing`] and [`Self::Materialized`]
/// **extend this vocabulary rather than starting a parallel one** (Stories
/// 56.2 and 56.7, AD-127). None of them is excluded from anything, and
/// none of them is waiting on something this machine has done, so the answers
/// available before they existed were [`Self::Synced`] — true, and silent
/// about the one fact that distinguishes the row from every other synced file
/// in the folder — and, for the middle one, a [`Self::Waiting`] that named
/// the wrong direction. A second enum carried beside this one would have been
/// a second answer to "what is this row", and the two would disagree the
/// first time a caller read only one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySyncStatus {
    /// The folder is a repository, the entry is not excluded, and nothing in
    /// the engine's pending list is about it.
    Synced,
    /// The engine still has work to do about this entry.
    Waiting {
        /// Why, when the pending list named this exact path. `None` for a
        /// directory whose mark is a roll-up of something beneath it — the
        /// folder itself is not untracked or modified, something inside it is,
        /// and borrowing that word for the folder would be a small lie about
        /// which thing git has never heard of.
        reason: Option<PendingReason>,
    },
    /// A pattern in this profile's own configuration excludes it. Sync will
    /// never carry it, and it is listed *only* so it can say so — keeper's
    /// built-in noise corpus stays hidden (see [`ExcludeVerdict`]).
    Excluded,
    /// The entry's own bytes are the committed LFS pointer, so this machine is
    /// not holding the content it names (Story 56.2, FR-336, AD-127).
    ///
    /// **A claim about here, not about the remote.** The only evidence is the
    /// bytes on disk, and a pointer whose object never reached the server is a
    /// valid blob with a clean `git status` — the state `verify --remote` exists
    /// to find. So neither this variant nor the sentence a surface words from it
    /// may say the content is safe somewhere else; it says the content is not
    /// here.
    ///
    /// A **settled** state, which is why it sits beside [`Self::Synced`]
    /// rather than under [`Self::Waiting`]: nothing is queued, nothing failed,
    /// and there is nothing for the owner to do. What it adds is the reason
    /// this row's size is a number the worktree cannot account for — see
    /// [`BrowseEntry::size_bytes`] and [`BrowseEntry::lfs_oid`].
    ///
    /// Only ever reached with a repository present, for the same reason
    /// [`Self::Synced`] is, and only when nothing more actionable is true of
    /// the path; [`classify`] documents the precedence.
    Virtual,
    /// The worktree bytes are still the committed pointer **and** the journal
    /// holds a queued LFS download for this path, so the content is on its way
    /// here (Story 56.7, FR-345).
    ///
    /// **A claim about here, not about the remote**, for the reason
    /// [`Self::Virtual`]'s doc gives: the evidence is a row in this machine's
    /// own journal and the bytes on its own disk, and neither of them has
    /// spoken to a server.
    ///
    /// **Not a sub-case of [`Self::Waiting`].** Waiting is "the engine still
    /// owes work about what THIS machine changed" — every other
    /// [`PendingReason`] is computed from `git status` and the completeness
    /// gate, which between them see only local edits. This one is inbound, and
    /// it is the one state in this enum whose owner can do nothing at all but
    /// wait for it.
    ///
    /// The pointer probe **confirms** the journal rather than duplicating it.
    /// [`PendingReason::Incoming`]'s own doc records that "a queued download
    /// always finds pointer text in the worktree", so bytes that are *not* the
    /// pointer mean this row is not the path being replaced — a download
    /// queued for a path since deleted, or an unlabelled `LFS object <oid…>`
    /// row that names no path at all — and that stays [`Self::Waiting`].
    Materializing,
    /// This clone holds the content for an LFS path whose content it is
    /// entitled to let go again (Story 56.7, FR-345).
    ///
    /// **Earned by two facts, never one:** a `materialized` ledger row for the
    /// path — keeper's own record that it put content there and can release it
    /// — *and* worktree bytes that are not the committed pointer.
    ///
    /// Either fact alone answers a different question. A ledger row on its own
    /// calls a path materialized whose content has since gone back to being a
    /// pointer, because [`crate::db::forget_materialized`] is the only
    /// retraction and a checkout, a prune or a release keeper did not perform
    /// leaves the row standing — which is exactly why `lfs::listing` decides
    /// virtual-or-materialized from the worktree and takes only timestamps
    /// from the ledger. Non-pointer bytes on their own call every ordinary
    /// file in the folder materialized, since a plain note and a hydrated
    /// recording are both just regular files to a module that opens no
    /// repository.
    ///
    /// A **settled** state beside [`Self::Synced`] and [`Self::Virtual`]:
    /// nothing is queued, nothing failed, and there is nothing the owner must
    /// do. What it adds is that the space this row occupies is recoverable —
    /// the content is here, and keeper may take it away again.
    Materialized,
    /// The profile's folder is not a git repository, so there is nothing for
    /// this entry to be synced with yet.
    NotInRepository,
    /// The engine was asked and could not say.
    Unknown,
}

/// The engine's pending list, indexed for the one question a listing asks.
///
/// A [`BTreeMap`] rather than a set of strings because a directory's mark is a
/// roll-up: the ordered map answers "is any path below `dir/` waiting" with one
/// range probe instead of a scan per row, which is what keeps a folder of a
/// thousand entries linear rather than quadratic against a long pending list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingView {
    /// The engine answered. Keys are profile-relative, `/`-joined paths exactly
    /// as [`crate::engine::Engine::pending`] produces them — the same frame
    /// [`BrowseEntry::relative_path`] is in, which is why no path arithmetic
    /// happens on either side of the join.
    Known(BTreeMap<String, PendingReason>),
    /// The engine could not produce a pending list. The reason belongs to the
    /// caller that failed to get one and is not copied onto every row; every
    /// entry reads [`EntrySyncStatus::Unknown`].
    Unavailable,
}

impl PendingView {
    /// Index one [`crate::engine::Engine::pending`] result.
    ///
    /// Takes the vector by value: the paths are moved into the map, so an
    /// answer about ten thousand pending files costs no second copy of them.
    pub fn from_pending(files: Vec<PendingFile>) -> Self {
        Self::Known(
            files
                .into_iter()
                .map(|file| (file.path, file.reason))
                .collect(),
        )
    }

    /// The pending reason for one entry, if the engine named it.
    ///
    /// A directory is asked about twice — for itself, then for anything
    /// beneath it — because both are real: a tracked directory can be reported
    /// deleted by name, and an ordinary folder is waiting when its contents
    /// are.
    fn waiting(&self, relative_path: &str, is_dir: bool) -> Option<Option<PendingReason>> {
        let Self::Known(map) = self else {
            return None;
        };
        if let Some(reason) = map.get(relative_path) {
            return Some(Some(reason.clone()));
        }
        if !is_dir {
            return None;
        }
        let mut prefix = String::with_capacity(relative_path.len() + 1);
        prefix.push_str(relative_path);
        prefix.push('/');
        map.range::<String, _>((Bound::Included(&prefix), Bound::Unbounded))
            .next()
            .is_some_and(|(path, _)| path.starts_with(prefix.as_str()))
            .then_some(None)
    }
}

/// The `materialized` ledger's paths, for the one question a listing asks of
/// it: has keeper put content at this path, and can it let that content go
/// again (Story 56.7, FR-345)?
///
/// A [`HashSet`] rather than the [`BTreeMap`] [`PendingView`] carries, because
/// there is no roll-up to answer: the ledger records files and a directory is
/// never materialized, so no range probe is wanted and a hash lookup per row is
/// the whole join. That second clause is a property [`classify`]'s ledger rung
/// enforces rather than one this type merely asserts — the ledger's only
/// retraction is [`crate::db::forget_materialized`], so a path recorded as a
/// file and replaced upstream by a directory of the same name keeps its row,
/// and nothing about a `HashSet` would stop that row reaching the folder.
///
/// # Why the ledger, and not [`crate::lfs::virtual_policy::VirtualPolicy`]
///
/// A row here means "keeper put content at this path and can release it" —
/// precisely what [`crate::db::forget_materialized`] retracts, what Story
/// 56.5's release sweep consults, and the same fact `lfs::listing` joins its
/// timestamps from. The policy answers a different question:
/// `VirtualPolicy::resolve`'s own doc says a `Virtual` answer is an
/// authorization and never an instruction, so a plain file that was never LFS
/// tracked and merely matches a pattern would read materialized off the
/// policy — a claim about what keeper is holding, made from a rule about what
/// keeper is allowed to hold.
///
/// # The polarity, stated so it is chosen
///
/// Supplied by the caller for the reason [`PendingView`] is (see
/// [`browse_root`]), and with the opposite hazard. An empty
/// [`PendingView::Known`] marks every entry [`EntrySyncStatus::Synced`] and is
/// a **lie**. An empty `MaterializedView` marks a materialized path
/// [`EntrySyncStatus::Synced`] too, and that is **true, merely less specific**:
/// the path really is in a repository, unexcluded and settled, and it still
/// counts as travelling in the delete plan, which is the answer that has
/// consequences.
///
/// So there is no `Unavailable` variant here, and adding one would be dead
/// weight: an engine that could not answer at all is already
/// [`PendingView::Unavailable`], and that returns [`EntrySyncStatus::Unknown`]
/// well before the ledger rung is ever reached.
///
/// No [`Default`], for the reason [`Self::none`] gives: the derive would be
/// public API handing every future caller exactly the spelling that method
/// exists to replace, and [`PendingView`] deliberately has none either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedView(HashSet<String>);

impl MaterializedView {
    /// Index one [`crate::engine::Engine::materialized_paths`] answer.
    ///
    /// Takes the set by value for the reason [`PendingView::from_pending`]
    /// does: the paths are moved in, so a profile holding ten thousand
    /// materialized rows costs no second copy of them.
    pub fn from_paths(paths: HashSet<String>) -> Self {
        Self(paths)
    }

    /// A caller that did not read the ledger.
    ///
    /// Named rather than left to [`Default`] so the call site *says* what it is
    /// doing — "this surface knows nothing about the ledger" reads as a
    /// statement, where an empty collection reads as one somebody forgot to
    /// fill. Which is why the type does not derive [`Default`] at all: leaving
    /// the derive on would keep the spelling this method argues against
    /// available, and reachable by inference in a struct field or a
    /// `..Default::default()`, where no call site says anything.
    pub fn none() -> Self {
        Self(HashSet::new())
    }

    /// Whether the ledger holds a row for this exact path.
    ///
    /// Exact and never a prefix, which is the same fact the type's doc turns
    /// on: a directory has no ledger row and must not inherit one from a file
    /// beneath it.
    pub fn holds(&self, path: &str) -> bool {
        self.0.contains(path)
    }
}

/// One directory's worth of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseDirectory {
    /// Directories first, then files, each group by name — a stable order that
    /// does not depend on what `read_dir` happens to hand back, so the same
    /// folder reads the same way twice.
    pub entries: Vec<BrowseEntry>,
    /// Whether [`LISTING_CAP`] cut the list short. The surface says so; it does
    /// not pretend the folder ended there.
    pub truncated: bool,
}

/// What a browse call found, when it was not refused outright.
///
/// The three non-listing answers are separate variants rather than an empty
/// list because they are separate facts, and the whole of a user's trust in a
/// file browser is that it does not report "nothing here" when the truth is
/// "I could not look". An unplugged pendrive and an empty folder look identical
/// to `read_dir` — the folder is simply gone — and collapsing them is how a
/// browser tells someone their recordings were deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseListing {
    /// The directory was read. `entries` may legitimately be empty: that is the
    /// one place "nothing here" is the truth.
    Listed(BrowseDirectory),
    /// This profile is on removable media (AD-48) and the media is not
    /// attached. A pause, not a fault, and nothing on disk is missing.
    MediaAbsent,
    /// This profile is on removable media and something else is mounted where
    /// its volume lives — a second stick at the first one's mountpoint. Never
    /// listed: those files belong to a stranger's disk, and showing them under
    /// this profile's name would be a lie about whose folder it is.
    MediaUnexpected {
        /// The foreign volume's id, for a sentence that says what is wrong
        /// rather than only that something is.
        found_id: String,
    },
    /// The directory is not on disk, on media that is attached (or on a fixed
    /// disk, where there is no media question). Moved, renamed or deleted
    /// outside keeper — a different next step from absent media, so a different
    /// answer.
    Missing,
}

/// Why a browse call was refused before anything was read.
///
/// Refusals are separate from [`BrowseListing`] because they are not states of
/// the folder — they are states of the *request*, and a caller that produces
/// one has asked for something it must never be able to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseRefusal {
    /// The subpath is not a plain descendant of the profile root: it was
    /// absolute, or it contained `..`, `.`, an empty component, or (on Windows)
    /// a drive prefix or a backslash separator.
    Escapes {
        /// The offending subpath, verbatim, so the log names what was asked
        /// for.
        subpath: String,
    },
    /// The subpath resolved to a plain descendant lexically and then, once
    /// symlinks were followed, landed outside the profile root. A planted
    /// symlink is the one escape no string test can catch (AD-59).
    EscapesAfterResolution {
        /// The offending subpath, verbatim.
        subpath: String,
    },
    /// The directory is there and could not be read — a permissions failure,
    /// most often. Distinct from [`BrowseListing::Missing`]: the folder exists,
    /// so telling someone it is gone would send them looking for a backup.
    Unreadable {
        /// The OS's own words, which are the most useful thing to show.
        reason: String,
    },
    /// The subpath contains `U+FFFD`, so it may be this module's own lossy
    /// rendering of a name that is not valid UTF-8 rather than a name (Story
    /// 47.2).
    ///
    /// **Refused because joining it can reach a different, real file.** A
    /// listing renders `a\xFF.txt` as `a<FFFD>.txt`; if the same folder also
    /// holds a file genuinely named `a<FFFD>.txt` — three ordinary UTF-8 bytes,
    /// a name a user can type — then joining the rendering back onto the root
    /// does not fail, it succeeds at the wrong file. That was measured before
    /// this variant existed, and [`crate::files_write`] shares the join, so the
    /// wrong file could be deleted.
    ///
    /// **Both cases are refused, and that is the right way round.** Nothing can
    /// tell a rendering apart from a real `U+FFFD` name, so the choice is
    /// between refusing a file that is almost certainly not there and silently
    /// acting on a file the user did not pick. A user who really does own a
    /// file with `U+FFFD` in its name loses the ability to expand it from this
    /// surface; a user who does not, keeps the file the browser would otherwise
    /// have eaten. See [`crate::names`].
    Unspellable {
        /// The offending subpath, verbatim.
        subpath: String,
    },
}

impl std::fmt::Display for BrowseRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Escapes { subpath } => write!(
                f,
                "\"{subpath}\" is not a path inside this folder, so it will not be listed"
            ),
            Self::EscapesAfterResolution { subpath } => write!(
                f,
                "\"{subpath}\" resolves outside this folder, so it will not be listed"
            ),
            Self::Unreadable { reason } => write!(f, "this folder could not be read: {reason}"),
            Self::Unspellable { subpath } => write!(
                f,
                "\"{subpath}\" holds the replacement character, so it may be a rendering of a \
                 name that is not text rather than a name; keeper will not act on it, because \
                 doing so could reach a different file"
            ),
        }
    }
}

/// Turn a profile-relative subpath into an absolute path under `root`, or
/// refuse it.
///
/// **This is the whole of AD-65 for the Files tab.** The frontend never joins a
/// root and a subpath; it hands back a `relative_path` this module produced and
/// this function does the join, once, in Rust — and gets to say no.
///
/// The lexical half runs first and runs unconditionally, before the disk is
/// consulted at all, so `..` is refused identically whether the drive is in,
/// out, or replaced by a stranger's. Each component must be exactly one
/// `Component::Normal`, which is a single test that covers every shape an
/// escape takes and covers it *per platform*: `..` is `ParentDir`, `.` is
/// `CurDir`, a leading `/` is `RootDir`, `C:` is `Prefix`, and on Windows —
/// where `\` is a separator and on Linux it is an ordinary filename character —
/// `a\b` yields two components there and one here, which is exactly the
/// difference that matters.
///
/// The canonicalizing half catches what no string test can: a symlink inside
/// the folder pointing somewhere else (AD-59, the same two-halves idiom
/// `recording_open_path` uses). Canonicalizing the root as well is what keeps a
/// folder reached through a symlink — a relocated home directory — from
/// refusing every path under it.
///
/// `Ok(None)` means the lexical test passed and the path is simply not on disk;
/// the caller decides whether that is absent media or a missing folder, because
/// only the caller knows whether the profile is removable.
pub fn resolve(root: &Path, subpath: &str) -> Result<Option<PathBuf>, BrowseRefusal> {
    let target = lexical_join(root, subpath)?;

    let (Ok(canonical_root), Ok(canonical_target)) = (root.canonicalize(), target.canonicalize())
    else {
        // Either the profile folder is not there (absent media, or moved), or
        // the entry under it is not. Not a refusal: nothing was asked for that
        // may not be asked for.
        return Ok(None);
    };
    if !canonical_target.starts_with(&canonical_root) {
        return Err(BrowseRefusal::EscapesAfterResolution {
            subpath: subpath.to_owned(),
        });
    }
    Ok(Some(canonical_target))
}

/// The lexical half of [`resolve`], on its own.
///
/// Split out because [`resolve`] is not the only caller that needs it: a create
/// names a file that does not exist yet, so it cannot be canonicalized, and
/// [`crate::files_write`] has to be able to refuse `..` in a path it is about
/// to write to *before* the disk is consulted. Two copies of this loop would be
/// two chances for one of them to be wrong, and the one that is wrong would be
/// the one guarding the write.
///
/// Never canonicalizes and never touches the disk, so it is total over strings.
pub fn lexical_join(root: &Path, subpath: &str) -> Result<PathBuf, BrowseRefusal> {
    let mut target = root.to_path_buf();
    for name in plain_segments(subpath)? {
        target.push(name);
    }
    Ok(target)
}

/// Split a profile-relative subpath into its components, refusing any that is
/// not a plain name.
///
/// Each segment must parse to exactly one `Component::Normal`, which is a single
/// test that covers every shape an escape takes and covers it *per platform*:
/// `..` is `ParentDir`, `.` is `CurDir`, a leading `/` is `RootDir`, `C:` is
/// `Prefix`, and on Windows — where `\` is a separator and on Linux an ordinary
/// filename character — `a\b` yields two components there and one here, which is
/// exactly the difference that matters.
///
/// The empty subpath is the root and yields no segments. Every other empty
/// segment — a leading, trailing or doubled `/` — is refused, because tolerating
/// it would make `"a//b"` and `"a/b"` two spellings of one request and make
/// `"/"` a second spelling of the root.
///
/// # Why `U+FFFD` is refused here and not somewhere more specific
///
/// This function is the single place a root and a caller-supplied subpath meet
/// (AD-65), which is why [`crate::files_write`] borrows it rather than writing
/// its own loop. That makes it the only place a rule can be added once and hold
/// for looking *and* for writing. A subpath carrying the replacement character
/// may be [`list_resolved`]'s own lossy rendering of a name that is not valid
/// UTF-8, and joining a rendering can land on a different real file — so the
/// refusal belongs at the join, not at the listing. See
/// [`BrowseRefusal::Unspellable`] and [`crate::names`].
pub fn plain_segments(subpath: &str) -> Result<Vec<&OsStr>, BrowseRefusal> {
    let escapes = || BrowseRefusal::Escapes {
        subpath: subpath.to_owned(),
    };
    if subpath.is_empty() {
        return Ok(Vec::new());
    }
    // Before the component walk, because this is a property of the whole
    // string and refusing it early keeps the per-segment loop about escapes.
    if crate::names::is_lossy_rendering(subpath) {
        return Err(BrowseRefusal::Unspellable {
            subpath: subpath.to_owned(),
        });
    }
    let mut out = Vec::new();
    for segment in subpath.split('/') {
        if segment.is_empty() {
            return Err(escapes());
        }
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => out.push(name),
            _ => return Err(escapes()),
        }
    }
    Ok(out)
}

/// List one directory of one profile, lazily and without touching the engine.
///
/// `subpath` is `""` for the profile root and otherwise a `/`-joined path this
/// module previously handed out. `excludes` is compiled by the caller so a
/// surface expanding a tree pays for the glob compilation once rather than per
/// click. `pending` is the engine's own pending list and `materialized` its
/// `materialized` ledger, both gathered once by the caller for the same reason
/// and read here rather than re-derived.
///
/// The order of the checks is the contract:
///
/// 1. The lexical containment test, always — an escape is refused even with the
///    drive unplugged, so the refusal cannot be probed for by pulling the media.
/// 2. The volume, for a removable profile only. Every ordinary folder pays one
///    boolean for this feature.
/// 3. The disk.
pub fn browse(
    profile: &SyncProfile,
    subpath: &str,
    excludes: &ExcludeSet,
    pending: &PendingView,
    materialized: &MaterializedView,
) -> Result<BrowseListing, BrowseRefusal> {
    let resolved = resolve(&profile.local_path, subpath)?;

    if profile.removable {
        // Read-only: `volume::scan` walks up to the nearest marker and reads it.
        // It never mints, adopts or rewrites one — that is `VolumeMarker::ensure`,
        // and browsing must not be the thing that binds a profile to a disk.
        match volume::scan(&profile.local_path, profile.volume_id.as_deref()) {
            Ok(VolumeStatus::Present { .. }) => {}
            Ok(VolumeStatus::Absent) => return Ok(BrowseListing::MediaAbsent),
            Ok(VolumeStatus::Foreign { found_id }) => {
                return Ok(BrowseListing::MediaUnexpected { found_id })
            }
            // An unreadable marker is "something is there and it is not provably
            // yours", which takes the same refusal as a foreign one and a
            // different sentence. Reported, never treated as attached.
            Err(error) => {
                return Ok(BrowseListing::MediaUnexpected {
                    found_id: error.to_string(),
                })
            }
        }
    }

    list_resolved(
        &profile.local_path,
        resolved,
        subpath,
        excludes,
        pending,
        materialized,
    )
}

/// List one directory of a plain root that is not a sync profile.
///
/// The note gallery (Story 44.15, FR-171) reads a folder of a notes vault, and
/// a vault is not a profile: it has no volume marker, no removable flag and
/// nothing for [`volume::scan`] to answer about. What it does need is every
/// other rule this module already carries — the lexical containment test, the
/// canonicalizing one behind it, the built-in noise filter, the cap and the
/// stable order — and a second `read_dir` written beside it to get them would
/// be a second place for the containment rule to be wrong. It is one rule, and
/// this is the entry point for a caller that has a root instead of a profile.
///
/// `pending` is still asked for rather than defaulted, because a caller that
/// knows nothing about sync must say so with [`PendingView::Unavailable`]
/// rather than be handed an empty [`PendingView::Known`] — an empty known list
/// marks every entry `Synced`, which is the exact lie [`EntrySyncStatus`]
/// documents against. `materialized` is asked for as well and for the same
/// reason, with the difference [`MaterializedView`] documents: its empty value
/// is not a lie, only a less specific truth, which is why it has no
/// "unavailable" spelling to reach for.
pub fn browse_root(
    root: &Path,
    subpath: &str,
    excludes: &ExcludeSet,
    pending: &PendingView,
    materialized: &MaterializedView,
) -> Result<BrowseListing, BrowseRefusal> {
    let resolved = resolve(root, subpath)?;
    list_resolved(root, resolved, subpath, excludes, pending, materialized)
}

/// Read the directory [`resolve`] already located, or say why there is none.
///
/// Split out of [`browse`] so [`browse_root`] can skip the volume question
/// without skipping anything else, and so neither entry point resolves the
/// subpath twice: the canonicalisation is two syscalls on a path that may live
/// on a network share, and paying for it once per listing is the difference
/// between a browser and a prober.
fn list_resolved(
    root: &Path,
    resolved: Option<PathBuf>,
    subpath: &str,
    excludes: &ExcludeSet,
    pending: &PendingView,
    materialized: &MaterializedView,
) -> Result<BrowseListing, BrowseRefusal> {
    let Some(dir) = resolved else {
        return Ok(BrowseListing::Missing);
    };
    if !dir.is_dir() {
        // A file was asked to be expanded, or the directory turned into one
        // between the resolve and now. Either way there are no children.
        return Ok(BrowseListing::Missing);
    }

    let in_repository = in_repository(root);
    let prefix = subpath.trim_end_matches('/');
    let read = std::fs::read_dir(&dir).map_err(|error| BrowseRefusal::Unreadable {
        reason: error.to_string(),
    })?;

    let mut entries: Vec<BrowseEntry> = Vec::new();
    let mut truncated = false;
    for entry in read {
        let Ok(entry) = entry else {
            // One unreadable dirent must not fail the whole listing: a folder
            // with one broken entry is still a folder worth browsing.
            continue;
        };
        // Lossy rather than skipped, and now *marked* lossy (Story 47.2).
        //
        // This comment used to claim that "every action on it is re-resolved in
        // Rust, where it will honestly refuse rather than open the wrong file".
        // That was false, and the counter-example is a test in this module: a
        // folder holding both `a\xFF.txt` and a file genuinely named
        // `a<FFFD>.txt` rendered the first as the second's name, and resolving
        // that string reached the second file. `plain_segments` refuses it now,
        // so the sentence is true — but it is true because of a rule, not
        // because of a hope, which is why the rule is at the join.
        //
        // Still not skipped: an entry nobody can name is the one the owner
        // needed told about, and dropping it is the silence this story is
        // about. It is listed, marked, and unactionable-by-name.
        let raw_name = entry.file_name();
        let unspellable = crate::names::UnspellableName::of(&raw_name);
        let name = raw_name.to_string_lossy().into_owned();
        let relative_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        // `metadata` follows symlinks; `file_type` would report the link. A
        // symlink to a folder should expand like a folder, and `resolve` is what
        // stops it if it points out of the tree. A broken symlink has no
        // metadata and reads as a file, which is what it looks like on disk.
        //
        // Bound once and read twice (Story 45.5): the size comes off the same
        // `stat` `is_dir` already needed, so a thousand-entry listing pays the
        // same number of syscalls it paid before this field existed. A second
        // `metadata` call here would have doubled the cost of browsing a
        // network share to render a column.
        let absolute_path = entry.path();
        let meta = std::fs::metadata(&absolute_path).ok();
        let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
        let match_path = Path::new(&relative_path);
        let verdict = excludes.verdict(match_path, is_dir);
        if verdict == ExcludeVerdict::BuiltinNoise {
            // Keeper's own noise, hidden as it has been since 43.8. A profile's
            // own pattern falls through and is listed, marked.
            continue;
        }
        if entries.len() == LISTING_CAP {
            truncated = true;
            break;
        }
        let mtime_ms = meta.as_ref().and_then(mtime_ms);
        // The pointer question, asked only where its answer can change the
        // row, and only after the exclusion `continue` and the cap — so a row
        // that is dropped, excluded, untracked, in no repository at all, or
        // waiting on something this machine did pays nothing for it.
        //
        // A row whose mark would otherwise be `Synced` can reach the question,
        // and — since Story 56.7 — so can one the journal says has content
        // arriving, where the pointer is what separates `Materializing` from
        // an ordinary `Waiting`. `classify` owns that precedence, and this
        // closure is called at most once whichever rung asks.
        //
        // `None` rather than `false` when there is no regular file here to
        // read. A dirent whose `stat` failed — the file was removed between
        // the `read_dir` and the `metadata`, which `BrowseEntry::size_bytes`
        // models as a real state — and a directory, a fifo or a device node
        // all reach this closure with no bytes to compare against the pointer,
        // and answering `false` would have said "these bytes are not the
        // pointer" about bytes nobody read. `classify`'s ledger rung spends
        // that distinction: `Materialized` claims content is here, so it is
        // earned by `Some(false)` and never by an absence.
        //
        // Two things follow from the ordering, and both are the reason for it.
        // The cost: a listing opens a file only where the answer can change
        // the row, instead of once per small dirent. And the honesty: the
        // pointer's number is reported for a row whose content is away or on
        // its way and for nothing else, so [`BrowseEntry::lfs_oid`] is `Some`
        // exactly when [`BrowseEntry::size_bytes`] came from a pointer, and an
        // untracked or excluded file that happens to hold pointer text — bytes
        // naming content no remote was ever told about — keeps its own length.
        // A `Materialized` row never reaches the substitution either: its
        // bytes are not a pointer, so the probe finds none and the row reports
        // the content the worktree actually holds.
        let mut pointer = None;
        let sync = classify(
            &relative_path,
            is_dir,
            verdict,
            pending,
            materialized,
            in_repository,
            || {
                let meta = meta.as_ref().filter(|meta| meta.is_file())?;
                pointer = stage::worktree_pointer(&absolute_path, meta);
                Some(pointer.is_some())
            },
        );
        // `is_file` rather than `!is_dir` for the fallback: a fifo, a socket
        // or a device node has a length that is not a number of bytes anyone
        // can read out of it.
        let size_bytes = pointer.as_ref().map(|pointer| pointer.size).or_else(|| {
            meta.as_ref()
                .filter(|meta| meta.is_file())
                .map(std::fs::Metadata::len)
        });
        let lfs_oid = pointer.map(|pointer| pointer.oid);
        entries.push(BrowseEntry {
            name,
            relative_path,
            absolute_path,
            is_dir,
            size_bytes,
            lfs_oid,
            mtime_ms,
            sync,
            unspellable,
        });
    }

    // Folders first, then files, then by name — case-insensitively, because a
    // list where `Zebra` sorts before `apple` is a list people re-read twice.
    // The case-sensitive comparison breaks the tie so the order is total.
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        })
    });

    Ok(BrowseListing::Listed(BrowseDirectory {
        entries,
        truncated,
    }))
}

/// Whether `root` is a git repository, for the purpose of a sync mark.
///
/// Deliberately the same expression [`crate::engine::Engine::pending`] uses —
/// a `.git` that is a file rather than a directory is a worktree or a submodule
/// and counts either way. Asking gitoxide to open the repository would be the
/// more thorough answer and is exactly what this module must not do: opening is
/// where trust levels, config enforcement and index refreshes live, and
/// browsing has no business near any of them.
///
/// One function rather than the expression written twice, because
/// [`list_resolved`] pays it once for a whole directory and [`status_of`] pays
/// it once per asked-about path, and a second spelling is a second chance for
/// one of them to answer differently about the same folder.
fn in_repository(root: &Path) -> bool {
    root.join(".git").exists()
}

/// One entry's modification time in milliseconds since the Unix epoch.
///
/// `None` means the OS did not give a modification time at all — an unreadable
/// `stat`, or a platform with no `mtime`. It does **not** mean "before 1970":
/// `SystemTime::duration_since` reports a pre-epoch instant as an `Err` holding
/// the distance backwards, and swallowing that into `None` would report a file
/// restored from an old archive as having no modification date, in a struct
/// whose whole rule is that an absence is an admission rather than a guess. So
/// the negative is carried as a negative.
///
/// Saturating rather than wrapping at the `i64` edge, for the same reason: a
/// corrupt or bogus far-future timestamp should read as far-future, not as some
/// arbitrary date the truncation happened to land on.
///
/// `pub(crate)` so [`crate::lfs::listing`] reports the same fact the same way.
/// This field's doc is where the convention is written down, and a second
/// spelling of the conversion is a second chance for two surfaces to disagree
/// about one file's date.
pub(crate) fn mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let time = meta.modified().ok()?;
    Some(match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
    })
}

/// What sync says about one entry the caller already knows about, without
/// re-reading its directory.
///
/// [`list_resolved`] answers this for every entry of a listing; this answers it
/// for a handful of paths a caller is about to *act* on — Story 45.3's delete
/// confirmation, which has to say whether the files it names sync. Both go
/// through [`classify`], so the confirmation and the row it was opened from
/// cannot come to word one file's state differently.
///
/// `is_dir` comes from the caller's own `stat`, never from the name.
///
/// The pointer question is answered here too, with this function's own `stat`
/// (Story 56.2) — and only when [`classify`] gets far enough to ask, which for
/// a delete confirmation over an excluded path, or one taken while the engine
/// is unavailable, is never. It is deliberately **not** a parameter: threading
/// it in would put the decision in three callers instead of one, and
/// [`classify`] exists precisely so there is one. It is deliberately
/// `std::fs::metadata` and not `symlink_metadata`, because that is the call
/// [`list_resolved`] binds, and the agreement between these two answers is a
/// documented property of this module rather than a coincidence — a
/// confirmation that called a listed row virtual, or refused to, would be the
/// divergence this shared [`classify`] was factored out to prevent.
pub fn status_of(
    root: &Path,
    relative_path: &str,
    is_dir: bool,
    excludes: &ExcludeSet,
    pending: &PendingView,
    materialized: &MaterializedView,
) -> EntrySyncStatus {
    let verdict = excludes.verdict(Path::new(relative_path), is_dir);
    classify(
        relative_path,
        is_dir,
        verdict,
        pending,
        materialized,
        in_repository(root),
        || {
            // `None` when there is no regular file to read, for the reason
            // [`list_resolved`]'s closure gives: a path the caller named and
            // that is no longer on disk — a delete confirmation is opened over
            // a selection, and the selection can go stale between the pick and
            // the confirm — has no bytes to compare against the pointer, and
            // saying "not the pointer" about bytes nobody read is what let a
            // vanished path with a ledger row claim it was holding content.
            let absolute = root.join(relative_path);
            let meta = std::fs::metadata(&absolute)
                .ok()
                .filter(|meta| meta.is_file())?;
            Some(stage::worktree_pointer(&absolute, &meta).is_some())
        },
    )
}

/// Decide one entry's mark from state that already existed before the call.
///
/// The precedence is the whole of the story's rule, in order:
///
/// 1. **A profile pattern wins over everything.** An excluded file is never
///    going to sync, and saying anything else about it — waiting, or worse,
///    synced — is the "waiting forever" this story exists to remove. It is
///    also what keeps a ledger row from ever reaching an excluded path: a
///    pattern added after content landed does not un-record the landing, and
///    "excluded" is still the answer somebody has to act on.
/// 2. **An engine that could not answer says so**, rather than letting the
///    absence of a pending row read as success.
/// 3. **Content arriving beats waiting** (Story 56.7). A queued LFS download
///    over a path whose worktree bytes are still the pointer is not "waiting
///    to sync" — it is this content on its way in, the one thing in this enum
///    nobody can hurry. It is a *narrowing* of rung 4 and not a rung above it:
///    the path is in the pending list either way, and what the probe decides
///    is which of the two words describes it.
/// 4. **Waiting beats not-in-a-repository.** Both are true of a settling file
///    in a folder that has never been adopted, and "the engine is holding this
///    file right now" is the more specific and more actionable of the two.
/// 5. **Waiting also beats virtual** (Story 56.2), and that disposes of the
///    one false positive the pointer probe can produce: an *untracked* file a
///    user happened to write pointer text into reads
///    [`EntrySyncStatus::Waiting`] with [`PendingReason::Untracked`], which is
///    both the more actionable answer and the true one — git has never heard
///    of the path, so no remote is holding anything for it.
/// 6. **Virtual is only ever reached with a repository present**, for the same
///    reason `Synced` is. Without one, bytes that parse as a pointer name
///    content no remote has ever been told about.
/// 7. **Virtual beats materialized** (Story 56.7), because the worktree is the
///    more recent witness. A ledger row says keeper once put content here; the
///    bytes say it is not here now. A path released since — by keeper's own
///    sweep, by a checkout, or by a prune — has both, and calling it
///    materialized would offer to free space that is already free.
/// 8. **Materialized is earned by bytes that were read**, never by the absence
///    of a pointer (Story 56.7). The probe answers `None` where there is no
///    regular file to read at all, so a path the ledger records and the disk no
///    longer holds — removed between a `read_dir` and its `stat`, or named by a
///    caller off a selection that went stale — falls through to rung 9 instead
///    of claiming content is here. The rung also requires `!is_dir`: a folder
///    is never something keeper can release, and this function takes `is_dir`
///    and the probe as independent inputs, so the rule is stated here rather
///    than left to a coincidence between two of the caller's answers about one
///    `stat`. [`MaterializedView`]'s own doc names that as a property of the
///    ledger, and this is what makes it one.
/// 9. **Synced is what is left**, and it too is only reached with a repository
///    present: without one, absence from the pending list means nothing at
///    all, and reporting it as synced would tell someone their files are safe
///    on a remote that has never heard of them. An unread
///    [`MaterializedView`] lands here, which is the degradation that type
///    documents: less specific, and not a lie.
///
/// # Why the pointer arrives as a closure
///
/// `worktree_bytes` is the only input that costs a syscall, and it is consulted
/// only on the rungs whose answer it can change — after returns that never
/// look at it. Taking it by value made every caller pay for it always: a
/// session tree of a hundred small transcripts opened and read a hundred files
/// per render to decide marks that were `Excluded` or `Unknown` before the
/// question was asked. Taking it as a closure means the cost is paid exactly
/// when the answer can change, and leaves the two callers free to reach the
/// filesystem differently — [`list_resolved`] off the `stat` it already bound,
/// [`status_of`] with its own — which is why this is not a `&Metadata`
/// parameter either.
///
/// **It answers three things, and the rungs spend all three.** `Some(true)` is
/// "the bytes here are the committed pointer", which is what rungs 3 and 6
/// license — the content is elsewhere, and the row may report the pointer's
/// number instead of the worktree's. `Some(false)` is "bytes were read and they
/// are not the pointer", the positive fact rung 8 needs before it says content
/// is here. `None` is "there is no readable regular file at this path", which
/// licenses neither: collapsing it onto `false` is what made a vanished path
/// with a ledger row report `Materialized` beside no size at all.
///
/// It is `FnMut` rather than `FnOnce` because more than one rung asks, and
/// **only one branch can reach the question**: rung 3 returns or falls through
/// to a `Waiting` that asks nothing more, and the settled rungs are only
/// reached when the pending list named nothing and share a single call between
/// them. So the syscall is still paid at most once per entry and the cost
/// argument above is unchanged — `FnMut` merely stops the type forbidding a
/// second reader that the control flow already forbids. Memoising inside the
/// closure would therefore cache a value nothing can ask for twice.
fn classify(
    relative_path: &str,
    is_dir: bool,
    verdict: ExcludeVerdict,
    pending: &PendingView,
    materialized: &MaterializedView,
    in_repository: bool,
    mut worktree_bytes: impl FnMut() -> Option<bool>,
) -> EntrySyncStatus {
    if verdict == ExcludeVerdict::ProfilePattern {
        return EntrySyncStatus::Excluded;
    }
    if matches!(pending, PendingView::Unavailable) {
        return EntrySyncStatus::Unknown;
    }
    if let Some(reason) = pending.waiting(relative_path, is_dir) {
        // A queued LFS download whose worktree bytes are still the pointer is
        // not "waiting to sync": it is this content arriving. `Incoming`'s own
        // doc says a queued download always finds pointer text, so the probe
        // confirms the row is the path being replaced rather than a label for
        // a deleted one — or an unlabelled `LFS object …` row, which is not a
        // path on disk at all.
        //
        // `replacing` is the other way the same row can be confirmed against a
        // real path (Story 56.14). It is `Engine::pending`'s answer to
        // `db::materialized_paths.contains(label)` — "a download is queued for
        // a path this machine already holds content for" — which is exactly the
        // fact the pointer-text probe cannot supply, because the worktree holds
        // an OLDER version's real bytes rather than pointer text. Such a row
        // used to fall through to `Waiting`, whose sentence is "This file's
        // content is still on the remote and has not been downloaded yet" over
        // a file whose content is on this disk. It is not on the remote only,
        // and it is not waiting to be uploaded: a newer version is arriving,
        // which is what `Materializing` says.
        //
        // Either fact confirms the row names this path, which is the whole job
        // of the conjunction — and `replacing` is free, so it is asked first
        // and can spare the probe its `open`.
        if in_repository
            && matches!(reason, Some(PendingReason::Incoming { .. }))
            && (matches!(
                reason,
                Some(PendingReason::Incoming {
                    replacing: true,
                    ..
                })
            ) || worktree_bytes() == Some(true))
        {
            return EntrySyncStatus::Materializing;
        }
        return EntrySyncStatus::Waiting { reason };
    }
    if !in_repository {
        return EntrySyncStatus::NotInRepository;
    }
    // One call for both settled rungs, so the open is still paid at most once.
    match worktree_bytes() {
        Some(true) => EntrySyncStatus::Virtual,
        Some(false) if !is_dir && materialized.holds(relative_path) => {
            EntrySyncStatus::Materialized
        }
        _ => EntrySyncStatus::Synced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::engine::Engine;
    use crate::platform::{SyncPlatform, TestPlatform};
    use crate::provenance::SyncSource;

    fn profile(local_path: &Path) -> SyncProfile {
        SyncProfile::new(
            "01PROFILE",
            "Field",
            local_path,
            "https://example.invalid/r.git",
        )
    }

    fn no_excludes() -> ExcludeSet {
        ExcludeSet::new(&[]).expect("builtin corpus compiles")
    }

    /// A profile with nothing outstanding — the engine answered, and its answer
    /// was "nothing". Distinct from [`PendingView::Unavailable`], which is the
    /// engine failing to answer, and the tests below rely on that difference.
    fn nothing_pending() -> PendingView {
        PendingView::Known(BTreeMap::new())
    }

    /// A caller that read the ledger and found no row — which is also what a
    /// caller that did not read the ledger at all hands over, because
    /// [`MaterializedView`] has no other answer to give. Its emptiness is a
    /// less specific truth rather than [`nothing_pending`]'s potential lie, and
    /// the tests below rely on that too: every mark asserted with this view
    /// would read the same way for a surface that never asks the engine.
    fn nothing_materialized() -> MaterializedView {
        MaterializedView::none()
    }

    fn names(listing: &BrowseListing) -> Vec<String> {
        match listing {
            BrowseListing::Listed(dir) => dir.entries.iter().map(|e| e.name.clone()).collect(),
            other => panic!("expected a listing, got {other:?}"),
        }
    }

    /// Story 45.3: the delete confirmation asks about a handful of paths
    /// rather than a directory, and it must get the SAME answer the row it was
    /// opened from shows. Asserted by listing a folder and then asking about
    /// each of its entries one at a time.
    #[test]
    fn asking_about_one_path_agrees_with_the_listing_it_came_from() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo");
        std::fs::write(root.path().join("kept.md"), b"x").expect("kept");
        std::fs::write(root.path().join("scratch.tmp"), b"x").expect("tmp");
        std::fs::create_dir(root.path().join("daily")).expect("daily");
        std::fs::write(root.path().join("daily/new.md"), b"x").expect("new");

        let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("patterns");
        let pending = PendingView::Known(BTreeMap::from([(
            "daily/new.md".to_owned(),
            PendingReason::Untracked,
        )]));
        let listing = browse(
            &profile(root.path()),
            "",
            &excludes,
            &pending,
            &nothing_materialized(),
        )
        .expect("listing");
        let BrowseListing::Listed(dir) = &listing else {
            panic!("expected a listing");
        };
        assert!(!dir.entries.is_empty());
        for entry in &dir.entries {
            assert_eq!(
                status_of(
                    root.path(),
                    &entry.relative_path,
                    entry.is_dir,
                    &excludes,
                    &pending,
                    &nothing_materialized(),
                ),
                entry.sync,
                "{}",
                entry.relative_path
            );
        }
        // And the three answers are genuinely different, so the agreement above
        // is not an agreement that everything is `Synced`.
        let kinds: Vec<_> = dir.entries.iter().map(|e| e.sync.clone()).collect();
        assert!(kinds.contains(&EntrySyncStatus::Synced), "{kinds:?}");
        assert!(kinds.contains(&EntrySyncStatus::Excluded), "{kinds:?}");
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, EntrySyncStatus::Waiting { .. })),
            "{kinds:?}"
        );
    }

    // --- The escape rule ---------------------------------------------------
    //
    // The reason this module is in `keeper-sync` and not in the Tauri shell:
    // these five run on any machine.

    #[test]
    fn parent_traversal_is_refused_at_every_position() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join("inner")).expect("inner");
        for subpath in ["..", "../..", "inner/..", "inner/../..", "../inner"] {
            assert_eq!(
                resolve(root.path(), subpath),
                Err(BrowseRefusal::Escapes {
                    subpath: subpath.to_owned()
                }),
                "{subpath} must not resolve"
            );
        }
    }

    #[test]
    fn absolute_and_degenerate_subpaths_are_refused() {
        let root = tempfile::tempdir().expect("temp");
        for subpath in ["/etc", "/", "inner/", "/inner", "inner//child", ".", "./x"] {
            assert!(
                matches!(
                    resolve(root.path(), subpath),
                    Err(BrowseRefusal::Escapes { .. })
                ),
                "{subpath} must not resolve"
            );
        }
    }

    #[test]
    fn the_root_itself_is_the_empty_subpath() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            resolve(root.path(), "").expect("root resolves"),
            Some(root.path().canonicalize().expect("canonical root"))
        );
    }

    #[test]
    fn a_plain_descendant_resolves_under_the_root() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("a/b")).expect("tree");
        let resolved = resolve(root.path(), "a/b")
            .expect("resolves")
            .expect("exists");
        assert!(resolved.starts_with(root.path().canonicalize().expect("canonical")));
        assert!(resolved.ends_with("a/b"));
    }

    /// The half a lexical test cannot catch. A `..` never appears in the
    /// subpath; the escape is planted on disk.
    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_refused_after_resolution() {
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secrets.txt"), "x").expect("secret");
        let root = tempfile::tempdir().expect("root");
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).expect("symlink");

        assert_eq!(
            resolve(root.path(), "escape"),
            Err(BrowseRefusal::EscapesAfterResolution {
                subpath: "escape".to_owned()
            })
        );
        // And the whole browse call refuses, not merely the resolver.
        assert_eq!(
            browse(
                &profile(root.path()),
                "escape",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized(),
            ),
            Err(BrowseRefusal::EscapesAfterResolution {
                subpath: "escape".to_owned()
            })
        );
    }

    /// An escape must be refused on its own terms, not because the disk
    /// happened to be unavailable — otherwise the refusal could be probed for
    /// by pulling the drive.
    #[test]
    fn traversal_is_refused_even_when_the_media_is_absent() {
        let root = tempfile::tempdir().expect("temp");
        let mut removable = profile(&root.path().join("gone"));
        removable.removable = true;
        assert_eq!(
            browse(
                &removable,
                "../..",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized()
            ),
            Err(BrowseRefusal::Escapes {
                subpath: "../..".to_owned()
            })
        );
    }

    // --- Absent media is not an empty folder -------------------------------

    #[test]
    fn a_removable_profile_with_no_marker_reports_absent_media() {
        let root = tempfile::tempdir().expect("temp");
        let mut removable = profile(root.path());
        removable.removable = true;
        removable.volume_id = Some("01VOLUME".to_owned());
        assert_eq!(
            browse(
                &removable,
                "",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized()
            )
            .expect("no refusal"),
            BrowseListing::MediaAbsent
        );
    }

    #[test]
    fn an_empty_folder_on_a_fixed_disk_is_an_empty_listing_not_absent_media() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            browse(
                &profile(root.path()),
                "",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized(),
            )
            .expect("no refusal"),
            BrowseListing::Listed(BrowseDirectory {
                entries: Vec::new(),
                truncated: false,
            })
        );
    }

    #[test]
    fn a_foreign_marker_is_neither_listed_nor_reported_as_absent() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("theirs.txt"), "x").expect("file");
        volume::VolumeMarker::write(
            root.path(),
            &volume::VolumeMarker::new("Someone Else", 1_700_000_000_000),
        )
        .expect("marker");

        let mut removable = profile(root.path());
        removable.removable = true;
        removable.volume_id = Some("01NOTTHEIRS".to_owned());
        assert!(matches!(
            browse(
                &removable,
                "",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized()
            )
            .expect("no refusal"),
            BrowseListing::MediaUnexpected { .. }
        ));
    }

    #[test]
    fn a_removable_profile_whose_volume_is_attached_lists_normally() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("clip.mov"), "x").expect("file");
        let marker = volume::VolumeMarker::new("merope", 1_700_000_000_000);
        volume::VolumeMarker::write(root.path(), &marker).expect("marker");

        let mut removable = profile(root.path());
        removable.removable = true;
        removable.volume_id = Some(marker.volume_id.clone());
        assert_eq!(
            names(
                &browse(
                    &removable,
                    "",
                    &no_excludes(),
                    &nothing_pending(),
                    &nothing_materialized()
                )
                .expect("no refusal")
            ),
            vec!["clip.mov"]
        );
    }

    #[test]
    fn a_fixed_folder_that_is_gone_is_missing_not_absent_media() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            browse(
                &profile(&root.path().join("moved-away")),
                "",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized(),
            )
            .expect("no refusal"),
            BrowseListing::Missing
        );
    }

    // --- Listing shape ------------------------------------------------------

    #[test]
    fn tier_zero_noise_is_filtered_by_the_one_exclude_set() {
        let root = tempfile::tempdir().expect("temp");
        for dir in ["node_modules", ".keeper", ".git", ".keeper-sync", "Notes"] {
            std::fs::create_dir(root.path().join(dir)).expect("dir");
        }
        for file in [".DS_Store", "clip.mov.partial", "notes.md"] {
            std::fs::write(root.path().join(file), "x").expect("file");
        }
        assert_eq!(
            names(
                &browse(
                    &profile(root.path()),
                    "",
                    &no_excludes(),
                    &nothing_pending(),
                    &nothing_materialized(),
                )
                .expect("no refusal")
            ),
            vec!["Notes", "notes.md"]
        );
    }

    /// Story 44.17 changed this deliberately, and 43.8's original assertion —
    /// that a profile-excluded file is absent — is the thing it changed.
    ///
    /// Dropping the row made a user's own rule invisible: someone who put
    /// `*.tmp` in their profile saw those files simply missing from the folder
    /// they were looking at, with nothing on screen tying the hole to the
    /// pattern they typed. Keeper's own noise corpus keeps the old treatment
    /// (see the test above); a rule a person wrote is shown working.
    #[test]
    fn a_profile_pattern_lists_the_file_and_marks_it_rather_than_hiding_it() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("keep.md"), "x").expect("file");
        std::fs::write(root.path().join("drop.tmp"), "x").expect("file");
        // Builtin noise beside it, so the two treatments are asserted against
        // each other in one listing rather than in two tests that could drift.
        std::fs::write(root.path().join(".DS_Store"), "x").expect("file");
        let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("compiles");

        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &excludes,
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        assert_eq!(
            dir.entries
                .iter()
                .map(|entry| (entry.name.as_str(), entry.sync.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("drop.tmp", EntrySyncStatus::Excluded),
                ("keep.md", EntrySyncStatus::NotInRepository),
            ],
            "the profile's own pattern is listed and marked; keeper's noise is not listed"
        );
    }

    #[test]
    fn folders_sort_before_files_and_names_sort_case_insensitively() {
        let root = tempfile::tempdir().expect("temp");
        for dir in ["zeta", "Alpha"] {
            std::fs::create_dir(root.path().join(dir)).expect("dir");
        }
        for file in ["b.md", "A.md"] {
            std::fs::write(root.path().join(file), "x").expect("file");
        }
        assert_eq!(
            names(
                &browse(
                    &profile(root.path()),
                    "",
                    &no_excludes(),
                    &nothing_pending(),
                    &nothing_materialized(),
                )
                .expect("no refusal")
            ),
            vec!["Alpha", "zeta", "A.md", "b.md"]
        );
    }

    #[test]
    fn a_child_listing_carries_the_parent_prefix_and_the_composed_absolute_path() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("2026/Standup")).expect("tree");
        std::fs::write(root.path().join("2026/Standup/manifest.json"), "{}").expect("file");

        let listing = browse(
            &profile(root.path()),
            "2026/Standup",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal");
        let BrowseListing::Listed(dir) = listing else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), 1);
        let entry = &dir.entries[0];
        assert_eq!(entry.relative_path, "2026/Standup/manifest.json");
        assert!(!entry.is_dir);
        // The join happens here and only here: the shell stringifies this, and
        // no TypeScript ever puts a root and a subpath together (AD-65).
        assert_eq!(
            entry.absolute_path,
            root.path()
                .canonicalize()
                .expect("canonical root")
                .join("2026/Standup/manifest.json")
        );
        // …and the relative path never leaks the machine's own directory names
        // into a string the surface renders (FR-145).
        assert!(!entry
            .relative_path
            .contains(&*root.path().to_string_lossy()));
    }

    /// Lazy by construction: asking for the root reads the root's dirents and
    /// nothing below them, so the surface cannot accidentally walk a
    /// hundred-thousand-file tree by rendering a folder.
    #[test]
    fn listing_a_directory_does_not_descend_into_its_children() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("deep/deeper")).expect("tree");
        std::fs::write(root.path().join("deep/deeper/buried.md"), "x").expect("file");

        let listing = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal");
        let BrowseListing::Listed(dir) = listing else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), 1);
        assert_eq!(dir.entries[0].name, "deep");
        assert!(dir.entries[0].is_dir);
    }

    /// A file's size comes off the dirent — unless the dirent's bytes are an
    /// LFS pointer, in which case the honest size is the one written inside it
    /// (Story 45.5, FR-178; Story 56.2, FR-336).
    ///
    /// The directory half is the one worth a test. `std::fs::metadata().len()`
    /// answers for a directory on Linux and macOS alike — it is nonzero and it
    /// describes the folder's own bookkeeping, not its contents — so the
    /// obvious `(!is_dir).then(...)`-free implementation ships a plausible
    /// number that means nothing. The empty file is here because zero is a real
    /// size and must survive the `Option`: `Some(0)` and `None` are different
    /// facts and a `filter(|n| *n > 0)` anywhere would collapse them.
    ///
    /// Story 56.2's rows are the pointer and its three near-misses, and the
    /// near-misses are the half that decides whether the rule is right. A
    /// 1025-byte file is one byte past the pointer ceiling and must never be
    /// opened. A 200-byte file beginning `version ` and a URL nobody defined is
    /// pointer-*shaped* and is not a pointer. An empty file is not the empty
    /// pointer, however [`crate::lfs::pointer::Pointer::parse`] reads zero
    /// bytes. Each answers with its own `stat` length and no oid, so an
    /// over-eager implementation fails on the number and on the flag at once.
    #[test]
    fn a_file_carries_its_byte_count_unless_its_bytes_are_a_pointer() {
        let root = tempfile::tempdir().expect("temp");
        let at = |name: &str| root.path().join(name);
        // The pointer row is only ever `Virtual` inside a repository, and the
        // honest size is tied to that mark — so the marker has to be here or
        // the fixture is asserting the not-in-a-repository rule instead.
        std::fs::create_dir(at(".git")).expect("repository marker");
        std::fs::create_dir(at("folder")).expect("dir");
        // Something inside it, so the folder is not empty and a recursive
        // implementation would have a number to report.
        std::fs::write(at("folder/inside.md"), vec![b'x'; 4_096]).expect("inner");
        std::fs::write(at("empty.md"), b"").expect("empty file");
        std::fs::write(at("sized.bin"), vec![b'x'; 1_500]).expect("sized file");

        // The story's central input: about 130 bytes on disk, naming 4 MiB.
        let pointer = crate::lfs::pointer::Pointer::new(
            "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393",
            4 * 1024 * 1024,
        );
        let rendered = pointer.render();
        assert!(
            rendered.len() < 200,
            "the fixture is only meaningful because the pointer is tiny: {} bytes",
            rendered.len()
        );
        std::fs::write(at("virtual.mp4"), &rendered).expect("pointer text");
        // One byte past the pointer ceiling: never opened, whatever it holds.
        std::fs::write(at("nearmiss.txt"), vec![b'x'; 1_025]).expect("near miss");
        // Pointer-shaped, and not a pointer: the version URL is nobody's.
        std::fs::write(
            at("looks-like.txt"),
            format!(
                "version http://example.invalid/spec/v9\n{}\n",
                "x".repeat(160)
            ),
        )
        .expect("decoy");

        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        let sizes: Vec<(&str, Option<u64>, Option<&str>)> = dir
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.name.as_str(),
                    entry.size_bytes,
                    entry.lfs_oid.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            sizes,
            vec![
                ("folder", None, None),
                ("empty.md", Some(0), None),
                ("looks-like.txt", Some(200), None),
                ("nearmiss.txt", Some(1_025), None),
                ("sized.bin", Some(1_500), None),
                (
                    "virtual.mp4",
                    Some(4 * 1024 * 1024),
                    Some(pointer.oid.as_str())
                ),
            ],
            "a folder has no size, an empty file has a size of zero, and only the \
             pointer's row reports a number its own bytes cannot account for"
        );
        // The mtime's unit and epoch, pinned. `is_some()` alone would survive
        // `as_secs()` in place of `as_millis()`, or `created()` in place of
        // `modified()`, so the bound is against a real instant: every file here
        // was written moments ago, which in seconds-since-epoch would be a
        // number about a thousand times too small.
        let now_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("after 1970")
                .as_millis(),
        )
        .expect("fits");
        for entry in &dir.entries {
            let mtime = entry
                .mtime_ms
                .expect("every readable entry carries an mtime");
            assert!(
                (now_ms - 600_000..=now_ms + 600_000).contains(&mtime),
                "{}'s mtime is within ten minutes of now, in milliseconds: {mtime} vs {now_ms}",
                entry.name
            );
        }
    }

    /// Pointer text in a repository reads [`EntrySyncStatus::Virtual`], and
    /// every more specific answer still beats it (Story 56.2).
    ///
    /// The precedence is the load-bearing half. `blocked.mp4` is in the pending
    /// list, so it reads `Waiting` — which is also what protects the one false
    /// positive available here, an untracked file somebody pasted pointer text
    /// into. `drop.tmp` matches the profile's own pattern, so it reads
    /// `Excluded`: a file that will never be carried is not "on the remote".
    /// And `status_of` is asked about each path afterwards, because a delete
    /// confirmation that worded one of these differently from the row it was
    /// opened from is the divergence `classify` exists to prevent.
    #[test]
    fn pointer_text_is_virtual_and_every_more_specific_answer_beats_it() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        let text = crate::lfs::pointer::Pointer::new(
            "5e2ca24d17e23934d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa",
            9_000_000,
        )
        .render();
        for name in ["away.mp4", "blocked.mp4", "drop.tmp"] {
            std::fs::write(root.path().join(name), &text).expect("pointer text");
        }

        let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("patterns");
        let pending = PendingView::Known(BTreeMap::from([(
            "blocked.mp4".to_owned(),
            PendingReason::Untracked,
        )]));
        let listing = browse(
            &profile(root.path()),
            "",
            &excludes,
            &pending,
            &nothing_materialized(),
        )
        .expect("no refusal");
        assert_eq!(
            marks(&listing),
            vec![
                ("away.mp4".to_owned(), EntrySyncStatus::Virtual),
                (
                    "blocked.mp4".to_owned(),
                    EntrySyncStatus::Waiting {
                        reason: Some(PendingReason::Untracked)
                    }
                ),
                ("drop.tmp".to_owned(), EntrySyncStatus::Excluded),
            ],
            "identical bytes, three different answers, because virtual is the \
             least specific of the three"
        );

        let BrowseListing::Listed(dir) = &listing else {
            panic!("expected a listing");
        };
        // The size and the oid follow the mark, and this is the assertion that
        // makes the precedence worth anything: three files with identical bytes,
        // and only the one keeper actually carries reports the nine megabytes
        // the pointer names. A row marked `Waiting { Untracked }` or `Excluded`
        // names content no remote was ever told about, so a fabricated figure
        // there — with no sentence on the row to account for it — would be the
        // 130-byte lie inverted.
        let facts: Vec<(&str, Option<u64>, bool)> = dir
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.name.as_str(),
                    entry.size_bytes,
                    entry.lfs_oid.is_some(),
                )
            })
            .collect();
        let on_disk = text.len() as u64;
        assert_eq!(
            facts,
            vec![
                ("away.mp4", Some(9_000_000), true),
                ("blocked.mp4", Some(on_disk), false),
                ("drop.tmp", Some(on_disk), false),
            ],
            "only the virtual row's size comes from the pointer, and the oid is \
             present on exactly that row"
        );

        for entry in &dir.entries {
            assert_eq!(
                status_of(
                    root.path(),
                    &entry.relative_path,
                    entry.is_dir,
                    &excludes,
                    &pending,
                    &nothing_materialized(),
                ),
                entry.sync,
                "asking about {} alone must agree with its row",
                entry.relative_path
            );
        }
    }

    /// Pointer text for a fixture, with a distinct oid per caller so two
    /// virtual rows in one folder cannot be told apart by accident.
    fn pointer_text(oid_seed: char, size: u64) -> String {
        crate::lfs::pointer::Pointer::new(
            std::iter::repeat_n(oid_seed, 64).collect::<String>(),
            size,
        )
        .render()
    }

    /// A queued download over pointer text is content ARRIVING, and a queued
    /// download over anything else is not (Story 56.7, FR-345).
    ///
    /// The two rows carry the same [`PendingReason::Incoming`], so the pending
    /// list alone cannot separate them — which is the point.
    /// `Incoming`'s own doc records that a queued download always finds pointer
    /// text in the worktree, so a row whose bytes are something else is a
    /// label for a path that is no longer what the queue thinks it is, and
    /// calling that "arriving" would put an in-flight mark on a file nothing
    /// is going to overwrite.
    #[test]
    fn content_on_its_way_is_materializing_and_a_plain_file_waiting_for_upload_is_not() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        std::fs::write(
            root.path().join("arriving.mp4"),
            pointer_text('a', 9_000_000),
        )
        .expect("pointer text");
        std::fs::write(root.path().join("ordinary.md"), b"notes, not a pointer").expect("plain");

        let incoming = PendingReason::Incoming {
            size_bytes: 9_000_000,
            replacing: false,
        };
        let pending = PendingView::Known(BTreeMap::from([
            ("arriving.mp4".to_owned(), incoming.clone()),
            ("ordinary.md".to_owned(), incoming.clone()),
        ]));

        let listing = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &pending,
            &nothing_materialized(),
        )
        .expect("no refusal");
        assert_eq!(
            marks(&listing),
            vec![
                ("arriving.mp4".to_owned(), EntrySyncStatus::Materializing),
                (
                    "ordinary.md".to_owned(),
                    EntrySyncStatus::Waiting {
                        reason: Some(incoming)
                    }
                ),
            ],
            "one pending reason, two answers, and the worktree is what decides \
             which"
        );
    }

    /// A queued download over a path this machine ALREADY holds content for is
    /// arriving, not waiting (Story 56.14).
    ///
    /// The pointer-text probe cannot see this case by construction: the
    /// worktree holds an older version's real bytes, so `worktree_bytes()`
    /// answers `Some(false)` exactly as it does for a file that is merely
    /// queued for upload. The one fact that separates them is `replacing`,
    /// which [`Engine::pending`] already computes from
    /// `db::materialized_paths` — it was available and unused. Without the fix
    /// the first row below reads `Waiting`, whose sentence in the shell is
    /// "This file's content is still on the remote and has not been downloaded
    /// yet" — said about a file whose content is on this disk, an older
    /// version of it.
    ///
    /// Both halves are asserted, because the fix is only correct if it left
    /// the `replacing: false` row alone: a download queued over real bytes
    /// that this machine never materialized is still not confirmed to name
    /// this path.
    #[test]
    fn a_download_replacing_content_this_machine_holds_is_materializing_not_waiting() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        // Real bytes, not pointer text, in BOTH: the probe answers `Some(false)`
        // either way, so `replacing` is the only thing that can differ.
        std::fs::write(root.path().join("newer.mp4"), b"an older cut, on this disk")
            .expect("real bytes");
        std::fs::write(
            root.path().join("stale.mp4"),
            b"real bytes, never materialized",
        )
        .expect("real bytes");

        let not_replacing = PendingReason::Incoming {
            size_bytes: 9_000_000,
            replacing: false,
        };
        let pending = PendingView::Known(BTreeMap::from([
            (
                "newer.mp4".to_owned(),
                PendingReason::Incoming {
                    size_bytes: 9_000_000,
                    replacing: true,
                },
            ),
            ("stale.mp4".to_owned(), not_replacing.clone()),
        ]));

        let listing = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &pending,
            &nothing_materialized(),
        )
        .expect("no refusal");
        assert_eq!(
            marks(&listing),
            vec![
                ("newer.mp4".to_owned(), EntrySyncStatus::Materializing),
                (
                    "stale.mp4".to_owned(),
                    EntrySyncStatus::Waiting {
                        reason: Some(not_replacing)
                    }
                ),
            ],
            "the ledger's `replacing` fact confirms the row names this path \
             where the pointer-text probe cannot"
        );
    }

    /// A ledger row over real bytes is [`EntrySyncStatus::Materialized`]; the
    /// same row over pointer text is still [`EntrySyncStatus::Virtual`] (Story
    /// 56.7, FR-345).
    ///
    /// The second half is the one that matters. The ledger is not retracted by
    /// a checkout, a prune, or a release keeper did not perform, so a row can
    /// outlive the content it records — and marking that path materialized
    /// would offer to free space that is already free. The worktree is the more
    /// recent witness, so it is asked first.
    ///
    /// The size is asserted too, because it is where the two states diverge on
    /// the wire: a materialized row's number is the worktree's own length and
    /// it carries no oid, which is exactly what says the number did not come
    /// from a pointer.
    #[test]
    fn content_this_clone_holds_is_materialized_and_a_released_path_is_still_virtual() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        let held = b"the real bytes, all of them";
        std::fs::write(root.path().join("held.mp4"), held).expect("real bytes");
        std::fs::write(
            root.path().join("released.mp4"),
            pointer_text('b', 4_000_000),
        )
        .expect("pointer text");

        // The ledger names BOTH: keeper put content at each of them once.
        let ledger = MaterializedView::from_paths(std::collections::HashSet::from([
            "held.mp4".to_owned(),
            "released.mp4".to_owned(),
        ]));

        let listing = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &ledger,
        )
        .expect("no refusal");
        assert_eq!(
            marks(&listing),
            vec![
                ("held.mp4".to_owned(), EntrySyncStatus::Materialized),
                ("released.mp4".to_owned(), EntrySyncStatus::Virtual),
            ],
            "the same ledger row, and the worktree decides which of the two \
             states it is"
        );

        let BrowseListing::Listed(dir) = &listing else {
            panic!("expected a listing");
        };
        assert_eq!(
            dir.entries
                .iter()
                .map(|entry| (
                    entry.name.as_str(),
                    entry.size_bytes,
                    entry.lfs_oid.is_some()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("held.mp4", Some(held.len() as u64), false),
                ("released.mp4", Some(4_000_000), true),
            ],
            "the materialized row reports what is on disk and no oid; only the \
             virtual row's size comes from a pointer"
        );
    }

    /// A ledger row is a fact about one file, and neither a folder nor an
    /// excluded path can wear it (Story 56.7).
    ///
    /// Two guards in one fixture, because both are places the new rung could
    /// have been put too early. A directory is not materialized — not by
    /// rolling a descendant's ledger row up the way [`PendingView`] rolls a
    /// descendant's pending reason up, and **not by a row naming the folder
    /// itself**, which is reachable: [`crate::db::forget_materialized`] is the
    /// only retraction, so a path recorded as a file and replaced upstream by a
    /// directory of the same name keeps its row, and marking that folder
    /// materialized would say "content is here, keeper may release it" about a
    /// folder. An excluded path keeps its own answer too, because a pattern
    /// added after content landed does not un-record the landing, and "this
    /// will never be carried" is still what the owner has to act on. The
    /// listing inside the folder is asserted as well, so the fixture is not
    /// passing by simply having no materialized path in it.
    #[test]
    fn a_ledger_row_never_makes_a_directory_or_an_excluded_path_materialized() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        std::fs::create_dir(root.path().join("bundle")).expect("folder");
        std::fs::write(root.path().join("bundle/clip.mp4"), b"real bytes").expect("held");
        std::fs::write(root.path().join("drop.tmp"), b"real bytes").expect("excluded");

        let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("patterns");
        let ledger = MaterializedView::from_paths(std::collections::HashSet::from([
            // The folder's own row — not a descendant's — and it must not turn
            // the folder into something keeper offers to release.
            "bundle".to_owned(),
            "bundle/clip.mp4".to_owned(),
            "drop.tmp".to_owned(),
        ]));

        assert_eq!(
            marks(
                &browse(
                    &profile(root.path()),
                    "",
                    &excludes,
                    &nothing_pending(),
                    &ledger
                )
                .expect("no refusal")
            ),
            vec![
                ("bundle".to_owned(), EntrySyncStatus::Synced),
                ("drop.tmp".to_owned(), EntrySyncStatus::Excluded),
            ],
            "the folder is not materialized by its own ledger row nor by its \
             child's, and the exclusion still wins over one of its own"
        );
        assert_eq!(
            marks(
                &browse(
                    &profile(root.path()),
                    "bundle",
                    &excludes,
                    &nothing_pending(),
                    &ledger
                )
                .expect("no refusal")
            ),
            vec![("clip.mp4".to_owned(), EntrySyncStatus::Materialized)],
            "and the file the row is actually about does read materialized, so \
             the guard above is not passing vacuously"
        );
    }

    /// [`EntrySyncStatus::Materialized`] is earned by bytes that were read, and
    /// a path that is no longer on disk has none (Story 56.7).
    ///
    /// The pointer probe's `false` used to mean several things at once — "these
    /// bytes are not the pointer", "this is not a regular file", and "there was
    /// nothing here to read" — and the ledger rung spent it as if it were only
    /// the first. So a path keeper recorded and the disk no longer holds read
    /// `Materialized`, claiming content was here beside a `size_bytes` of
    /// `None`. The row outliving the file is the ordinary case rather than a
    /// corrupt one: [`crate::db::forget_materialized`] is the only retraction.
    ///
    /// Asked through [`status_of`], which is the surface where it bites — a
    /// delete confirmation is opened over a selection, and a selection can go
    /// stale between the pick and the confirm. The same path is asserted while
    /// the file is still there, so the second assertion is not about a rung
    /// nothing reaches.
    #[test]
    fn a_ledger_row_over_a_path_with_no_readable_bytes_is_not_materialized() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        let held = root.path().join("held.mp4");
        std::fs::write(&held, b"the real bytes, all of them").expect("real bytes");
        let ledger =
            MaterializedView::from_paths(std::collections::HashSet::from(["held.mp4".to_owned()]));

        assert_eq!(
            status_of(
                root.path(),
                "held.mp4",
                false,
                &no_excludes(),
                &nothing_pending(),
                &ledger,
            ),
            EntrySyncStatus::Materialized,
            "with the content on disk the row is materialized"
        );

        std::fs::remove_file(&held).expect("release the file behind the ledger's back");
        assert_eq!(
            status_of(
                root.path(),
                "held.mp4",
                false,
                &no_excludes(),
                &nothing_pending(),
                &ledger,
            ),
            EntrySyncStatus::Synced,
            "and with nothing left to read it claims nothing: the ledger row \
             outlived the file, and an absence of bytes is not evidence that \
             the bytes here are not a pointer"
        );
    }

    /// The delete confirmation and the row it was opened from must word all
    /// three virtual states the same way (Story 56.7).
    ///
    /// The same claim
    /// [`asking_about_one_path_agrees_with_the_listing_it_came_from`] makes,
    /// re-asked over the states that arrived with their own probe and their own
    /// view: [`status_of`] reaches the filesystem with its own `stat` where
    /// [`list_resolved`] reuses the one it already bound, and a divergence
    /// between those two paths is exactly what [`classify`] was factored out to
    /// prevent.
    #[test]
    fn status_of_and_the_listing_agree_about_the_three_virtual_states() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        std::fs::write(
            root.path().join("arriving.mp4"),
            pointer_text('c', 7_000_000),
        )
        .expect("pointer text");
        std::fs::write(root.path().join("away.mp4"), pointer_text('d', 5_000_000))
            .expect("pointer text");
        std::fs::write(root.path().join("held.mp4"), b"real bytes").expect("real bytes");

        let pending = PendingView::Known(BTreeMap::from([(
            "arriving.mp4".to_owned(),
            PendingReason::Incoming {
                size_bytes: 7_000_000,
                replacing: true,
            },
        )]));
        let ledger =
            MaterializedView::from_paths(std::collections::HashSet::from(["held.mp4".to_owned()]));

        let listing = browse(&profile(root.path()), "", &no_excludes(), &pending, &ledger)
            .expect("no refusal");
        assert_eq!(
            marks(&listing)
                .into_iter()
                .map(|(_, mark)| mark)
                .collect::<Vec<_>>(),
            vec![
                EntrySyncStatus::Materializing,
                EntrySyncStatus::Virtual,
                EntrySyncStatus::Materialized,
            ],
            "all three states in one folder, or the agreement below is asserted \
             over nothing"
        );

        let BrowseListing::Listed(dir) = &listing else {
            panic!("expected a listing");
        };
        for entry in &dir.entries {
            assert_eq!(
                status_of(
                    root.path(),
                    &entry.relative_path,
                    entry.is_dir,
                    &no_excludes(),
                    &pending,
                    &ledger,
                ),
                entry.sync,
                "asking about {} alone must agree with its row",
                entry.relative_path
            );
        }
    }

    /// A modification time before 1970 is a date, not an absence (Story 56.2).
    ///
    /// `SystemTime::duration_since` reports a pre-epoch instant as an `Err`, and
    /// the obvious `.ok()` swallows it into `None` — which this struct's own rule
    /// says means "the OS would not tell us". A file extracted from an old
    /// archive has a perfectly good date, and reporting it as unknown is the
    /// same invention as reporting an unknown one as 1970.
    #[test]
    fn a_modification_time_before_1970_is_reported_as_a_negative_and_not_as_nothing() {
        let root = tempfile::tempdir().expect("temp");
        let path = root.path().join("ancient.txt");
        std::fs::write(&path, b"from the tape").expect("write");
        let when = std::time::UNIX_EPOCH - std::time::Duration::from_secs(86_400);
        if std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(when))
            .is_err()
        {
            // A filesystem that refuses a pre-epoch mtime cannot falsify this.
            return;
        }

        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        assert_eq!(
            dir.entries[0].mtime_ms,
            Some(-86_400_000),
            "a day before the epoch, in milliseconds, carried as a negative"
        );
    }

    /// Without a repository, pointer text is not evidence of anything (Story
    /// 56.2).
    ///
    /// `Virtual` says keeper is holding a placeholder for content it is tracking.
    /// A folder that has never been adopted is tracking nothing, so the honest
    /// answer is the one the folder's state already dictated — and an engine that
    /// could not answer at all still overrides both.
    #[test]
    fn pointer_text_outside_a_repository_is_never_called_virtual() {
        let root = tempfile::tempdir().expect("temp");
        let text = crate::lfs::pointer::Pointer::new(
            "aa2ca24d17e23934d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa",
            1_000,
        )
        .render();
        std::fs::write(root.path().join("away.mp4"), &text).expect("pointer text");

        assert_eq!(
            marks(
                &browse(
                    &profile(root.path()),
                    "",
                    &no_excludes(),
                    &nothing_pending(),
                    &nothing_materialized(),
                )
                .expect("no refusal")
            ),
            vec![("away.mp4".to_owned(), EntrySyncStatus::NotInRepository)],
            "no repository, so nothing is on a remote yet"
        );

        // ...and with a repository present but the engine mute, `Unknown` still
        // wins: a mark nobody can stand behind is not replaced by a cheerful
        // one just because the bytes happen to parse.
        std::fs::create_dir(root.path().join(".git")).expect("repo marker");
        assert_eq!(
            marks(
                &browse(
                    &profile(root.path()),
                    "",
                    &no_excludes(),
                    &PendingView::Unavailable,
                    &nothing_materialized(),
                )
                .expect("no refusal")
            ),
            vec![("away.mp4".to_owned(), EntrySyncStatus::Unknown)]
        );
    }

    #[test]
    fn a_directory_beyond_the_cap_reports_that_it_was_cut_short() {
        let root = tempfile::tempdir().expect("temp");
        for index in 0..(LISTING_CAP + 5) {
            std::fs::write(root.path().join(format!("f{index:06}.md")), "x").expect("file");
        }
        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), LISTING_CAP);
        assert!(dir.truncated);
    }

    #[test]
    fn expanding_a_file_reports_missing_rather_than_an_empty_folder() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("notes.md"), "x").expect("file");
        assert_eq!(
            browse(
                &profile(root.path()),
                "notes.md",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized(),
            )
            .expect("no refusal"),
            BrowseListing::Missing
        );
    }

    // --- The mark, against a real repository the real engine answered about --
    //
    // Everything above this line is a pure listing test and would happily pass
    // while the feature did nothing on a real machine. The mark is a join
    // between two things that only exist on disk — dirents, and what
    // `Engine::pending` says after a real `git status` walk over a real commit
    // — so these build both and join them for real. A hand-written
    // `PendingView` would prove the `match` arms and nothing about the feature.

    /// A committed repository with one of each state in it.
    ///
    /// Returns the engine alongside the profile because the caller has to ask
    /// it for the pending list — that call is the point of the fixture.
    /// Everything is driven through public engine API: no test reaches into
    /// `file_state`, the gate or the index by hand, so the states asserted
    /// below are states the shipping code actually produces.
    async fn committed_fixture(
        data: &Path,
        work: &Path,
        remote: &Path,
    ) -> Option<(Engine, Arc<TestPlatform>, SyncProfile)> {
        gix::init_bare(remote).ok()?;
        let platform = Arc::new(TestPlatform::new(data));
        // A machine with no usable git cannot host the engine at all, which is
        // AD-41's contract — skip rather than fake it, as engine's own tests do.
        let engine = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?;

        let mut p = SyncProfile::new(
            "01BROWSESTATUS",
            "Vault",
            work,
            remote.to_string_lossy().into_owned(),
        );
        p.excludes = vec!["*.tmp".to_owned()];
        std::fs::create_dir_all(work.join("notes")).expect("notes dir");
        std::fs::create_dir_all(work.join("archive")).expect("archive dir");
        std::fs::write(work.join("tracked.md"), b"tracked").expect("write");
        std::fs::write(work.join("notes/kept.md"), b"kept").expect("write");
        std::fs::write(work.join("archive/old.md"), b"old").expect("write");
        // The user's own pattern, and keeper's noise, side by side.
        std::fs::write(work.join("drop.tmp"), b"scratch").expect("write");
        std::fs::write(work.join(".DS_Store"), b"finder").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        // Two passes a settle window apart: the completeness gate needs two
        // identical observations before it lets anything through, so one pass
        // only opens the episode.
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("the first pass adopts and opens the settle episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("the second pass commits and publishes");
        Some((engine, platform, p))
    }

    /// Every file under `root`, as path → bytes, so a caller can assert that a
    /// whole directory tree is byte-identical before and after something ran.
    fn tree_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, at: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(read) = std::fs::read_dir(at) else {
                return;
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    out.insert(
                        path.strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/"),
                        bytes,
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    fn marks(listing: &BrowseListing) -> Vec<(String, EntrySyncStatus)> {
        match listing {
            BrowseListing::Listed(dir) => dir
                .entries
                .iter()
                .map(|entry| (entry.name.clone(), entry.sync.clone()))
                .collect(),
            other => panic!("expected a listing, got {other:?}"),
        }
    }

    /// The story's four states, each produced by real state rather than by a
    /// constructed fixture of the answer.
    ///
    /// `tracked.md` is committed and clean; `notes/fresh.md` is genuinely
    /// untracked so a real `git status` reports it; `settling.bin` is inside a
    /// real settle episode the gate opened; `drop.tmp` matches the profile's
    /// own pattern. Each mark is therefore falsifiable by breaking the thing it
    /// names.
    #[tokio::test]
    async fn each_state_comes_from_state_that_already_existed() {
        let data = tempfile::tempdir().expect("data");
        let work = tempfile::tempdir().expect("work");
        let remote = tempfile::tempdir().expect("remote");
        let Some((engine, platform, p)) =
            committed_fixture(data.path(), work.path(), remote.path()).await
        else {
            return;
        };

        // A file the gate is holding right now. One scan observes it and opens
        // the episode; without the clock advancing, the second observation the
        // gate needs never happens, so it is still settling when we look.
        std::fs::write(p.local_path.join("settling.bin"), b"still arriving").expect("write");
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("a pass that opens an episode and commits nothing");

        // …and a file git has never heard of, created after the last scan so
        // the gate has no opinion about it and only `git status` can answer.
        std::fs::write(p.local_path.join("notes/fresh.md"), b"new").expect("write");

        let pending = PendingView::from_pending(engine.pending(&p.id).await.expect("pending"));
        let excludes = ExcludeSet::new(&p.excludes).expect("compiles");
        let listing =
            browse(&p, "", &excludes, &pending, &nothing_materialized()).expect("no refusal");

        assert_eq!(
            marks(&listing),
            vec![
                // A folder whose every file is committed and clean.
                ("archive".to_owned(), EntrySyncStatus::Synced),
                // …and one holding an untracked file. The folder itself is not
                // untracked, so it carries no reason of its own.
                (
                    "notes".to_owned(),
                    EntrySyncStatus::Waiting { reason: None }
                ),
                ("drop.tmp".to_owned(), EntrySyncStatus::Excluded),
                (
                    "settling.bin".to_owned(),
                    EntrySyncStatus::Waiting {
                        reason: Some(PendingReason::Settling {
                            since_ms: platform.now_ms(),
                        }),
                    }
                ),
                ("tracked.md".to_owned(), EntrySyncStatus::Synced),
            ],
            "keeper's own noise stays hidden; every other state is named"
        );

        // The child listing, where the untracked file is a file rather than a
        // roll-up, carries git's own word for why it is waiting.
        assert_eq!(
            marks(
                &browse(&p, "notes", &excludes, &pending, &nothing_materialized())
                    .expect("no refusal")
            ),
            vec![
                (
                    "fresh.md".to_owned(),
                    EntrySyncStatus::Waiting {
                        reason: Some(PendingReason::Untracked),
                    }
                ),
                ("kept.md".to_owned(), EntrySyncStatus::Synced),
            ]
        );
    }

    /// AD-74's spirit, asserted rather than asserted-to.
    ///
    /// Two bugs in this session had exactly one shape: a caller that only meant
    /// to look also changed something. A file browser asking "is this synced"
    /// is the third opportunity, so the listing is run against a real engine's
    /// real repository with every byte of `.git` and of the engine's own data
    /// directory recorded on both sides of the call.
    #[tokio::test]
    async fn a_listing_changes_no_byte_of_the_engine_or_the_repository() {
        let data = tempfile::tempdir().expect("data");
        let work = tempfile::tempdir().expect("work");
        let remote = tempfile::tempdir().expect("remote");
        let Some((engine, _platform, p)) =
            committed_fixture(data.path(), work.path(), remote.path()).await
        else {
            return;
        };
        std::fs::write(p.local_path.join("notes/fresh.md"), b"new").expect("write");
        let pending = PendingView::from_pending(engine.pending(&p.id).await.expect("pending"));
        let excludes = ExcludeSet::new(&p.excludes).expect("compiles");

        let git_before = tree_bytes(&p.local_path.join(".git"));
        let data_before = tree_bytes(data.path());
        assert!(
            !git_before.is_empty() && !data_before.is_empty(),
            "the fixture must have produced a repository and a database, or this \
             test compares two empty maps and proves nothing"
        );

        for subpath in ["", "notes", "archive"] {
            browse(&p, subpath, &excludes, &pending, &nothing_materialized()).expect("no refusal");
        }

        assert_eq!(
            tree_bytes(&p.local_path.join(".git")),
            git_before,
            "browsing rewrote something in .git — the index, a ref or a lock"
        );
        assert_eq!(
            tree_bytes(data.path()),
            data_before,
            "browsing wrote to sync.db — the journal, file_state or the activity log"
        );
    }

    /// A folder that is not a repository at all, and the one thing this state
    /// exists to prevent: reading "nothing is pending" as "everything is safe".
    #[tokio::test]
    async fn a_folder_with_no_repository_never_reports_its_files_as_synced() {
        let data = tempfile::tempdir().expect("data");
        let work = tempfile::tempdir().expect("work");
        let platform = Arc::new(TestPlatform::new(data.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = SyncProfile::new(
            "01NOTAREPO",
            "Vault",
            work.path(),
            "https://example.invalid/r.git",
        );
        std::fs::write(p.local_path.join("notes.md"), b"never synced").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        // The engine's own answer for a folder it has never adopted: nothing is
        // pending, because there is no repository for anything to be pending
        // against. That emptiness is exactly what must not read as success.
        let files = engine.pending(&p.id).await.expect("pending");
        assert!(files.is_empty(), "the fixture must have nothing pending");

        assert_eq!(
            marks(
                &browse(
                    &p,
                    "",
                    &ExcludeSet::new(&p.excludes).expect("compiles"),
                    &PendingView::from_pending(files),
                    &nothing_materialized(),
                )
                .expect("no refusal")
            ),
            vec![("notes.md".to_owned(), EntrySyncStatus::NotInRepository)]
        );
    }

    /// An engine that could not answer must not be read as an engine that said
    /// "nothing". Both produce an empty pending list; only one of them means
    /// the files are safe.
    #[test]
    fn an_engine_that_could_not_answer_marks_every_entry_unknown() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("a repository");
        std::fs::write(root.path().join("notes.md"), "x").expect("file");
        std::fs::write(root.path().join("drop.tmp"), "x").expect("file");
        let excludes = ExcludeSet::new(&["*.tmp".to_owned()]).expect("compiles");

        assert_eq!(
            marks(
                &browse(
                    &profile(root.path()),
                    "",
                    &excludes,
                    &PendingView::Unavailable,
                    &nothing_materialized(),
                )
                .expect("no refusal")
            ),
            vec![
                // The exclusion is known without asking the engine, so it is
                // still named — a file that will never sync does not become
                // uncertain because something else failed.
                ("drop.tmp".to_owned(), EntrySyncStatus::Excluded),
                ("notes.md".to_owned(), EntrySyncStatus::Unknown),
            ]
        );
    }

    /// The roll-up must not match a sibling by string prefix: `notes` and
    /// `notes-archive` share five characters and share nothing else.
    #[test]
    fn a_directory_rolls_up_its_own_descendants_and_not_its_neighbours() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join(".git")).expect("a repository");
        for dir in ["notes", "notes-archive"] {
            std::fs::create_dir(root.path().join(dir)).expect("dir");
        }
        let pending = PendingView::from_pending(vec![PendingFile {
            path: "notes-archive/late.md".to_owned(),
            reason: PendingReason::Untracked,
            size_bytes: None,
        }]);

        assert_eq!(
            marks(
                &browse(
                    &profile(root.path()),
                    "",
                    &no_excludes(),
                    &pending,
                    &nothing_materialized()
                )
                .expect("no refusal")
            ),
            vec![
                ("notes".to_owned(), EntrySyncStatus::Synced),
                (
                    "notes-archive".to_owned(),
                    EntrySyncStatus::Waiting { reason: None }
                ),
            ]
        );
    }

    // --- A root that is not a profile (Story 44.15) -------------------------

    /// The note gallery's listing is the Files tab's listing. If these two ever
    /// disagree about what a folder holds, one of them grew a second reader.
    #[test]
    fn a_plain_root_lists_exactly_what_the_same_folder_lists_through_a_profile() {
        let root = tempfile::tempdir().expect("temp");
        for name in ["b.png", "a.mov", "notes.txt"] {
            std::fs::write(root.path().join(name), b"x").expect("file");
        }
        std::fs::create_dir(root.path().join("sub")).expect("sub");
        std::fs::create_dir(root.path().join(".git")).expect("noise");

        let through_a_profile = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal");
        let through_a_root = browse_root(
            root.path(),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal");

        assert_eq!(names(&through_a_root), names(&through_a_profile));
        assert_eq!(
            names(&through_a_root),
            vec!["sub", "a.mov", "b.png", "notes.txt"],
        );
    }

    /// The containment rule is not a property of having a profile. A gallery
    /// block is text in a note, so `> [!gallery] ../../.ssh` is one line an
    /// agent can write by accident, and it must be refused here rather than
    /// somewhere above.
    #[test]
    fn a_plain_root_refuses_every_escape_a_profile_refuses() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join("inner")).expect("inner");
        for subpath in ["..", "../..", "inner/..", "/etc", "inner//child", "."] {
            assert!(
                matches!(
                    browse_root(
                        root.path(),
                        subpath,
                        &no_excludes(),
                        &nothing_pending(),
                        &nothing_materialized()
                    ),
                    Err(BrowseRefusal::Escapes { .. })
                ),
                "{subpath} must not be listed"
            );
        }
    }

    /// A vault has no volume marker and must not be treated as though it might:
    /// a folder that is simply not there is `Missing`, which is the answer the
    /// gallery turns into a sentence.
    #[test]
    fn a_plain_root_reports_a_folder_that_is_not_there_as_missing() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            browse_root(
                root.path(),
                "gone",
                &no_excludes(),
                &nothing_pending(),
                &nothing_materialized()
            )
            .expect("no refusal"),
            BrowseListing::Missing,
        );
    }

    /// A directory that exists and cannot be opened is a refusal carrying the
    /// OS's own words, never an empty folder — the gallery says so rather than
    /// rendering nothing.
    #[cfg(unix)]
    #[test]
    fn a_plain_root_refuses_an_unreadable_directory_with_the_reason() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp");
        let shut = root.path().join("shut");
        std::fs::create_dir(&shut).expect("dir");
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let refusal = browse_root(
            root.path(),
            "shut",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        );

        // Restored before the assertion so a failure does not leave the temp
        // directory undeletable.
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(
            matches!(refusal, Err(BrowseRefusal::Unreadable { .. })),
            "expected an unreadable refusal, got {refusal:?}"
        );
    }

    // ---- Story 47.2: names keeper cannot spell -------------------------

    // The fixtures below are built from raw bytes by
    // `crate::names::create_unspellable`, never from a string literal:
    // `"a\u{FFFD}.txt"` is a perfectly ordinary UTF-8 filename and a test
    // using it would pass over this bug rather than through it. That helper
    // also answers None where the filesystem refuses the bytes, which is
    // every macOS volume — see its doc comment for why that does not make
    // the defect theoretical.

    /// The reported defect: a file keeper cannot name is a file the owner is
    /// never told about.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_listed_and_marked_rather_than_dropped() {
        let root = tempfile::tempdir().expect("temp");
        if crate::names::create_unspellable(root.path(), b"doc-\xffepuap.txt").is_none() {
            eprintln!("{}", crate::names::UNSPELLABLE_UNAVAILABLE);
            return;
        }
        std::fs::write(root.path().join("ordinary.txt"), "x").expect("file");

        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };

        assert_eq!(dir.entries.len(), 2, "neither file may be dropped");
        let odd = dir
            .entries
            .iter()
            .find(|e| e.unspellable.is_some())
            .expect("the unspellable entry is marked");
        let marker = odd.unspellable.as_ref().expect("marked");
        // Named well enough to go and find: the lossy rendering for the row,
        // and the byte-exact one so `ls | cat -v` matches.
        assert_eq!(marker.display, "doc-\u{FFFD}epuap.txt");
        assert_eq!(marker.escaped, "doc-\\xffepuap.txt");
        // The one handle that still reaches the file is the undecoded one.
        assert!(odd.absolute_path.exists());

        let ordinary = dir
            .entries
            .iter()
            .find(|e| e.name == "ordinary.txt")
            .expect("the ordinary entry");
        assert_eq!(
            ordinary.unspellable, None,
            "an ordinary name must not be reported, or every file is a finding"
        );
    }

    /// The bug that led this story: a lossy rendering that reaches a **real,
    /// different** file.
    ///
    /// Before this story, `list_resolved` claimed in a comment that "every
    /// action on it is re-resolved in Rust, where it will honestly refuse
    /// rather than open the wrong file". This is the folder where that was
    /// false. `a\xFF.txt` renders as `a<FFFD>.txt`, which is *also* the real
    /// name of the second file here, so handing the row's `relative_path` back
    /// resolved — successfully — to the wrong one. [`crate::files_write`]
    /// shares the same join, so a delete confirmed against one row removed the
    /// other.
    #[cfg(unix)]
    #[test]
    fn a_lossy_rendering_is_refused_rather_than_resolved_to_a_different_file() {
        let root = tempfile::tempdir().expect("temp");
        if crate::names::create_unspellable(root.path(), b"a\xff.txt").is_none() {
            eprintln!("{}", crate::names::UNSPELLABLE_UNAVAILABLE);
            return;
        }
        // A file whose name really is U+FFFD: three ordinary UTF-8 bytes, a
        // name a person can type. This is the decoy.
        std::fs::write(root.path().join("a\u{FFFD}.txt"), "a different file").expect("decoy");

        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
            &nothing_materialized(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        let clicked = dir
            .entries
            .iter()
            .find(|e| e.unspellable.is_some())
            .expect("the mangled row");

        // The two rows are indistinguishable by the string the surface carries
        // — which is exactly why the string may not be used to reach anything.
        assert_eq!(
            dir.entries
                .iter()
                .filter(|e| e.relative_path == clicked.relative_path)
                .count(),
            2,
            "the renderings collide, so a resolve on one of them is a coin toss"
        );

        let resolved = resolve(root.path(), &clicked.relative_path);
        assert!(
            matches!(resolved, Err(BrowseRefusal::Unspellable { .. })),
            "a rendering must be refused, never resolved; got {resolved:?}"
        );
        // And the refusal is total over the join, so the write surface inherits
        // it without a rule of its own (AD-65).
        assert!(matches!(
            lexical_join(root.path(), &clicked.relative_path),
            Err(BrowseRefusal::Unspellable { .. })
        ));
        assert!(matches!(
            plain_segments("notes/a\u{FFFD}.txt"),
            Err(BrowseRefusal::Unspellable { .. })
        ));
    }

    /// The refusal is about the replacement character and nothing else.
    #[test]
    fn ordinary_names_still_join_including_non_ascii_ones() {
        // Non-ASCII is not non-UTF-8. A Polish or Japanese filename is text and
        // a rule that refused it would break far more than it fixed.
        for subpath in ["notes/zaświadczenie.pdf", "議事録/2026.md", "plain.txt"] {
            assert!(
                plain_segments(subpath).is_ok(),
                "{subpath} is ordinary text and must still resolve"
            );
        }
    }

    /// The refusal sentence has to say what to do about it, not only that it
    /// happened — this string is rendered verbatim by the shell.
    #[test]
    fn the_unspellable_refusal_says_why_it_will_not_act() {
        let said = BrowseRefusal::Unspellable {
            subpath: "a\u{FFFD}.txt".to_owned(),
        }
        .to_string();
        assert!(said.contains("a\u{FFFD}.txt"), "got: {said}");
        assert!(said.contains("could reach a different file"), "got: {said}");
    }
}

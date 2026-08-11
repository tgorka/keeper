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

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ops::Bound;
use std::path::{Component, Path, PathBuf};

use crate::engine::{PendingFile, PendingReason};
use crate::exclude::{ExcludeSet, ExcludeVerdict};
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
    /// **Free.** The `stat` behind it is the same one `is_dir` already paid
    /// for; this field costs the listing nothing beyond a `u64` per row.
    ///
    /// **`None` for a directory on purpose.** `metadata().len()` answers for a
    /// directory on every platform this runs on, and the number it gives is the
    /// size of the directory's own bookkeeping — not of what is inside it.
    /// Carrying it would hand a surface a plausible-looking number that means
    /// nothing, and the only honest total for a folder needs a recursive walk
    /// this module must never do. A folder's size is not slow here; it is
    /// absent.
    pub size_bytes: Option<u64>,
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
/// Four answers a person acts on differently, and the distinction the story
/// turns on is [`Self::Excluded`] against [`Self::Waiting`]: a file that will
/// never be carried, rendered as one that is about to be, is a file somebody
/// waits for forever. [`Self::NotInRepository`] is the third way a file can
/// fail to arrive and it has its own next step — the folder has no repository,
/// so nothing in it is going anywhere until the first sync adopts it.
///
/// [`Self::Unknown`] is the fifth, and it exists for the same reason
/// [`BrowseListing`] separates an absent drive from an empty folder: when the
/// engine could not answer, every other value would be a claim this module has
/// no grounds for. Guessing [`Self::Synced`] tells someone their work is safe;
/// guessing [`Self::Waiting`] tells them to keep waiting. Neither is honest.
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
/// click. `pending` is the engine's own pending list, gathered once by the
/// caller for the same reason and read here rather than re-derived.
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

    list_resolved(&profile.local_path, resolved, subpath, excludes, pending)
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
/// documents against.
pub fn browse_root(
    root: &Path,
    subpath: &str,
    excludes: &ExcludeSet,
    pending: &PendingView,
) -> Result<BrowseListing, BrowseRefusal> {
    let resolved = resolve(root, subpath)?;
    list_resolved(root, resolved, subpath, excludes, pending)
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
        // `is_file` rather than `!is_dir`: a fifo, a socket or a device node has
        // a length that is not a number of bytes anyone can read out of it.
        let size_bytes = meta
            .as_ref()
            .filter(|meta| meta.is_file())
            .map(std::fs::Metadata::len);
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
        let sync = classify(&relative_path, is_dir, verdict, pending, in_repository);
        entries.push(BrowseEntry {
            name,
            relative_path,
            absolute_path,
            is_dir,
            size_bytes,
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
pub fn status_of(
    root: &Path,
    relative_path: &str,
    is_dir: bool,
    excludes: &ExcludeSet,
    pending: &PendingView,
) -> EntrySyncStatus {
    let verdict = excludes.verdict(Path::new(relative_path), is_dir);
    classify(relative_path, is_dir, verdict, pending, in_repository(root))
}

/// Decide one entry's mark from state that already existed before the call.
///
/// The precedence is the whole of the story's rule, in order:
///
/// 1. **A profile pattern wins over everything.** An excluded file is never
///    going to sync, and saying anything else about it — waiting, or worse,
///    synced — is the "waiting forever" this story exists to remove.
/// 2. **An engine that could not answer says so**, rather than letting the
///    absence of a pending row read as success.
/// 3. **Waiting beats not-in-a-repository.** Both are true of a settling file
///    in a folder that has never been adopted, and "the engine is holding this
///    file right now" is the more specific and more actionable of the two.
/// 4. **Synced is only ever reached with a repository present.** Without one,
///    absence from the pending list means nothing at all, and reporting it as
///    synced would tell someone their files are safe on a remote that has never
///    heard of them.
fn classify(
    relative_path: &str,
    is_dir: bool,
    verdict: ExcludeVerdict,
    pending: &PendingView,
    in_repository: bool,
) -> EntrySyncStatus {
    if verdict == ExcludeVerdict::ProfilePattern {
        return EntrySyncStatus::Excluded;
    }
    if matches!(pending, PendingView::Unavailable) {
        return EntrySyncStatus::Unknown;
    }
    if let Some(reason) = pending.waiting(relative_path, is_dir) {
        return EntrySyncStatus::Waiting { reason };
    }
    if in_repository {
        EntrySyncStatus::Synced
    } else {
        EntrySyncStatus::NotInRepository
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
        let listing = browse(&profile(root.path()), "", &excludes, &pending).expect("listing");
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
                    &pending
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
                &nothing_pending()
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
            browse(&removable, "../..", &no_excludes(), &nothing_pending()),
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
            browse(&removable, "", &no_excludes(), &nothing_pending()).expect("no refusal"),
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
                &nothing_pending()
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
            browse(&removable, "", &no_excludes(), &nothing_pending()).expect("no refusal"),
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
            names(&browse(&removable, "", &no_excludes(), &nothing_pending()).expect("no refusal")),
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
                &nothing_pending()
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
                    &nothing_pending()
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

        let BrowseListing::Listed(dir) =
            browse(&profile(root.path()), "", &excludes, &nothing_pending()).expect("no refusal")
        else {
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
                    &nothing_pending()
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
        )
        .expect("no refusal");
        let BrowseListing::Listed(dir) = listing else {
            panic!("expected a listing");
        };
        assert_eq!(dir.entries.len(), 1);
        assert_eq!(dir.entries[0].name, "deep");
        assert!(dir.entries[0].is_dir);
    }

    /// A file's size comes off the dirent; a folder has none (Story 45.5,
    /// FR-178).
    ///
    /// The directory half is the one worth a test. `std::fs::metadata().len()`
    /// answers for a directory on Linux and macOS alike — it is nonzero and it
    /// describes the folder's own bookkeeping, not its contents — so the
    /// obvious `(!is_dir).then(...)`-free implementation ships a plausible
    /// number that means nothing. The empty file is here because zero is a real
    /// size and must survive the `Option`: `Some(0)` and `None` are different
    /// facts and a `filter(|n| *n > 0)` anywhere would collapse them.
    #[test]
    fn a_file_carries_its_byte_count_and_a_folder_carries_none() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join("folder")).expect("dir");
        // Something inside it, so the folder is not empty and a recursive
        // implementation would have a number to report.
        std::fs::write(root.path().join("folder/inside.md"), vec![b'x'; 4_096]).expect("inner");
        std::fs::write(root.path().join("empty.md"), b"").expect("empty file");
        std::fs::write(root.path().join("sized.bin"), vec![b'x'; 1_500]).expect("sized file");

        let BrowseListing::Listed(dir) = browse(
            &profile(root.path()),
            "",
            &no_excludes(),
            &nothing_pending(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        let sizes: Vec<(&str, Option<u64>)> = dir
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.size_bytes))
            .collect();
        assert_eq!(
            sizes,
            vec![
                ("folder", None),
                ("empty.md", Some(0)),
                ("sized.bin", Some(1_500)),
            ],
            "a folder has no size, and an empty file has a size of zero"
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
                &nothing_pending()
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
        let listing = browse(&p, "", &excludes, &pending).expect("no refusal");

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
            marks(&browse(&p, "notes", &excludes, &pending).expect("no refusal")),
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
            browse(&p, subpath, &excludes, &pending).expect("no refusal");
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
                    &PendingView::Unavailable
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
        }]);

        assert_eq!(
            marks(
                &browse(&profile(root.path()), "", &no_excludes(), &pending).expect("no refusal")
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
        )
        .expect("no refusal");
        let through_a_root =
            browse_root(root.path(), "", &no_excludes(), &nothing_pending()).expect("no refusal");

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
                    browse_root(root.path(), subpath, &no_excludes(), &nothing_pending()),
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
            browse_root(root.path(), "gone", &no_excludes(), &nothing_pending())
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

        let refusal = browse_root(root.path(), "shut", &no_excludes(), &nothing_pending());

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

//! Which paths' **content** may stay away (AD-122, AD-123, AD-132; FR-328,
//! FR-329, FR-330, FR-331, FR-344).
//!
//! A virtual file is tracked, committed and wanted; only its bytes are allowed
//! to live in the LFS object store and nowhere else. This module answers the one
//! question that has to be settled before any of that is reachable: *may this
//! path's content be absent?* It compiles the repository's committed pattern
//! file plus whatever the resolved profile says into two pattern sets and a size
//! floor, and answers per path. The floor is a floor under the other terms and,
//! when no permissive pattern is in force from any source, the selector itself
//! (Story 56.16): a size is a statement about which files may stay away, and
//! requiring a pattern beside it made "keep the big ones away" a control that
//! silently did nothing.
//!
//! # The separation this module is on the safe side of
//!
//! The policy **authorizes** hydration. Only per-object proof — the object is in
//! the store, or the remote has confirmed it holds it — ever authorizes deleting
//! a byte (AD-123, FR-330). So nothing here deletes, prunes, dehydrates,
//! truncates or rewrites any worktree content, and nothing here ever will: a
//! policy edit is allowed to change an *answer*, never a file. That is not a
//! style preference. It is [git-lfs#3092], where a pattern change dropped
//! content that existed nowhere else.
//!
//! Two consequences follow:
//!
//! * [`VirtualPolicy::resolve`] performs **no I/O of any kind** (FR-328) — no
//!   worktree read, no network, no repo handle, no clock. Every term of the
//!   policy is answerable from the LFS pointer (paths plus a size), never from
//!   the bytes, because reading the bytes to decide whether the bytes may be
//!   absent is circular (AD-122, FR-344). This one is guaranteed by reading
//!   `resolve`, which consults three compiled fields and nothing else; the test
//!   that deletes the worktree from under a compiled policy fences the worktree
//!   half of it against a future edit, and claims no more than that.
//! * Compiling a policy over a real repository leaves `git status` clean and the
//!   worktree byte-for-byte as it was (FR-331). `tests/virtual_policy.rs`
//!   settles that against a real `git` binary and a real index — which today is
//!   nearly a tautology, because `compile`'s only filesystem call is a read, and
//!   that is exactly the property it exists to keep true as 56.2 and 56.4 add
//!   verbs beside it.
//!
//! # Where the policy comes from
//!
//! Four ascending sources, of which this module reads exactly one:
//!
//! ```text
//! .keepervirtual  <  stored profile row  <  .keeper/keeper.toml  <  keeper.<host>.toml
//! └── read here ──┘  └──── already folded into the SyncProfile by profile::in_force ────┘
//! ```
//!
//! The three upper layers arrive already resolved, because the folder tier is
//! applied on every profile read and a folder file outranks the stored row —
//! which is what lets the file keep winning (AD-98, AD-132). So `compile` does
//! one read and one override decision and re-implements none of the rest.
//!
//! [git-lfs#3092]: https://github.com/git-lfs/git-lfs/issues/3092

use std::io::ErrorKind;
use std::path::{Component, Path};

use crate::error::{Result, SyncError};
use crate::exclude::PatternSet;
use crate::profile::{SyncProfile, FOLDER_CONFIG_DIR};

/// The repository's own answer, committed at its root, in gitignore dialect.
///
/// Named for what it does rather than `.keeperignore`: "ignore" is the wrong
/// verb, because these paths are tracked, committed and wanted and only their
/// bytes may stay away — and that name would collide with a future exclude file.
/// It plays `.lfsconfig`'s role: the *repository's* intent, overridden by the
/// *machine's* answer.
pub const VIRTUAL_PATTERN_FILE: &str = ".keepervirtual";

/// The list a refusal names for a line in the committed file.
///
/// Distinct from [`PROFILE_SOURCE`] on purpose: a typo in a file every clone
/// shares and a typo in a machine's own configuration are different problems,
/// and a message that rendered them identically would send the user to the
/// wrong place (FR-329). It cannot go further than that — by the time a profile
/// reaches this module the folder tier has already folded
/// `.keeper/keeper.toml` and `keeper.<host>.toml` into it, so which of those
/// two files set the key is a question only the tier can answer, and it does:
/// `FolderOutcome::owned` records the keys each layer claimed.
const FILE_SOURCE: &str = ".keepervirtual";

/// The list a refusal names for an entry the profile carries — its stored row
/// or either folder TOML layer above it, which are indistinguishable here.
const PROFILE_SOURCE: &str = "virtualPatterns";

/// Whether one path's content may stay away.
///
/// Deliberately not a `bool`: at every call site the two answers must read as
/// what they are, and `Materialize` is the answer any doubt resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Virtualization {
    /// The content may live only in the object store.
    Virtual,
    /// The content belongs in the worktree.
    Materialize,
}

/// What the policy in force came from.
///
/// AD-132 wants a surface that can show which tier is speaking, and a user
/// debugging "why is this file not here" needs to be told which knob to
/// change. Two honest limits, stated here so no caller reads more into it than
/// it knows: a size has no spelling in gitignore dialect, so a floor can only
/// ever come from a profile tier, and the protections are the *union* of every
/// source, so this never answers which source protected a path. It answers
/// exactly one question — what decided that anything at all *may* stay away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualPolicyTier {
    /// No source said anything: nothing may stay away.
    Unset,
    /// The repository's committed `.keepervirtual` is in force.
    PatternFile,
    /// The resolved profile — its stored row or a folder TOML layer above it —
    /// replaced the file's list.
    Profile,
    /// No pattern list authorized anything, and the profile's size floor is the
    /// selector (Story 56.16).
    ///
    /// Its own variant rather than a reuse of `Profile`, and not for tidiness:
    /// `Profile` would claim `virtualPatterns` decided what may stay away, and
    /// that list is empty in this state — so a regression that stopped the
    /// floor selecting could still pass a `Profile` assertion while nothing
    /// stayed away at all. It cannot pass one that names this.
    ///
    /// Reaching this variant also changes what [`crate::engine`]'s `verify`
    /// calls a fault: it excuses an absent object whenever the tier is anything
    /// but `Unset`, so a folder whose only control is the floor stops reporting
    /// every absent object as missing content and starts counting those above
    /// the floor as virtual. That is the answer the owner asked for — under the
    /// old reading his floor selected nothing, so absent content in that folder
    /// really was unexplained — and it is also, stated plainly, a signal going
    /// quiet: anybody who set a floor while the setting was inert loses the
    /// fault report that would have told them content had gone missing for some
    /// other reason. The trade is narrow by construction, because the floor's
    /// default is `0` and a zero floor never reaches this variant: only a
    /// folder where a person typed a positive floor is affected at all.
    SizeFloor,
}

/// The compiled policy: two pattern sets, a size floor that may itself be the
/// selector, and what the answers came from.
///
/// Fields are private, as [`crate::lfs::stage::LfsPolicy`]'s are: the compiled
/// sets are an implementation of the question, and a caller that reached into
/// them would be re-deciding precedence that `compile` already settled.
///
/// Compiled once per run and reused, for the same reason `LfsPolicy` is: a pass
/// resolves every candidate path, and building a `GlobSet` per path would make
/// the common case — no policy configured at all — pay for a feature it does not
/// use.
#[derive(Debug)]
pub struct VirtualPolicy {
    /// Paths whose content may stay away.
    patterns: PatternSet,
    /// Paths whose content must not, whatever `patterns` says.
    ///
    /// The union of the negated (`!`) lines of **every** source, not just the
    /// one that won the positive list. AD-123 is the reason: a policy edit may
    /// widen what is allowed to leave, and may never narrow what is protected,
    /// so a machine that restates the repository's zone cannot drop the
    /// repository's exceptions along with it. That is git-lfs#3092's shape
    /// exactly.
    never: PatternSet,
    /// Smallest size that may stay away, inclusive. `0` means no floor.
    ///
    /// A floor under every other term of the policy and, when `floor_selects`
    /// holds, the term that selects.
    over_bytes: u64,
    /// Whether the floor is the only thing selecting anything (Story 56.16).
    ///
    /// True exactly when the effective permissive set is empty and the floor is
    /// positive. The owner who saved a 1 MiB floor and no patterns had stated a
    /// policy about size; `resolve` demanding a pattern match on top of it made
    /// the setting accept-and-ignore, and his whole 16 GB folder downloaded.
    floor_selects: bool,
    tier: VirtualPolicyTier,
}

impl VirtualPolicy {
    /// Read the committed file once, fold in whatever the profile says, and
    /// compile.
    ///
    /// The file is read from the **worktree** (`profile.local_path`) and never
    /// from `HEAD`: the policy that governs this checkout is the one standing in
    /// it, which is also the only one a user can edit and see take effect.
    ///
    /// A malformed glob is a hard [`SyncError::Config`] naming its source and
    /// quoting the line as typed, never a silently dropped pattern (FR-329) — a
    /// typo that quietly does nothing is how a folder stays 200 GB and nobody
    /// learns why.
    pub fn compile(profile: &SyncProfile) -> Result<Self> {
        let path = profile.local_path.join(VIRTUAL_PATTERN_FILE);
        let file_text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // The ordinary case, and silence rather than a fault: most
            // repositories have no policy, and demanding the file exist would
            // make every folder carry one.
            Err(err) if err.kind() == ErrorKind::NotFound => String::new(),
            // Anything else — a directory by that name, a permission bit,
            // non-UTF-8 bytes — is said out loud. A policy we cannot read is
            // not a policy, and reading it as "nothing is virtual" would hide a
            // broken repository behind a plausible answer.
            Err(err) => {
                return Err(SyncError::Config(format!(
                    "could not read {}: {err}",
                    path.display()
                )));
            }
        };
        // A byte-order mark is not whitespace, so `trim` leaves it on the first
        // line and the line stops being the pattern the user wrote. Worse for a
        // `!` line: prefixed by a BOM it no longer starts with `!`, so a line
        // written to *protect* a file would be filed as one that authorizes it.
        // Windows editors write a BOM by default and this file is committed, so
        // one clone's editor would change every clone's policy.
        let file_text = file_text.strip_prefix('\u{feff}').unwrap_or(&file_text);

        let file = Parsed::of(file_text.lines(), Comments::Recognized);
        // A TOML array has no comments — the format has its own — so a `#` there
        // is the first character of a path, not the start of a remark.
        let from_profile = Parsed::of(profile.virtual_patterns.iter(), Comments::Literal);

        // A profile list that says what may stay away replaces the file's
        // permissive list wholesale (AD-122). Judged on what it parses to, not
        // on whether the key exists: a stray blank or comment-shaped entry must
        // not silently mute a repository's committed policy while `tier` claims
        // a policy is in force.
        //
        // **The PERMISSIVE half decides it, and only that half** (Story 56.14).
        // Asking `says_something()` — either half — let a list of nothing but
        // `!` protections replace the committed permissive list with an EMPTY
        // one: a machine restating one exception switched the whole folder's
        // virtualization off, silently, while `tier()` reported `Profile`.
        // AD-123's rule is that a policy edit may widen what may leave and may
        // never narrow what is protected, and that spelling narrowed what is
        // *authorized* to nothing from a line written to protect one path. A
        // protection is not a claim about the zone; it is an exception inside
        // whatever zone is in force, and the union below is where it belongs.
        let overrides = !from_profile.patterns.is_empty();
        let (source, positive) = if overrides {
            (PROFILE_SOURCE, &from_profile.patterns)
        } else {
            (FILE_SOURCE, &file.patterns)
        };
        let patterns = PatternSet::anchored(&Parsed::entries(source, positive))?;

        // The floor selects on its own when no permissive line is in force
        // anywhere (Story 56.16). Computed from the **effective** set, after
        // the precedence above has run, so a folder that already names files
        // keeps naming exactly those and the floor never widens a zone some
        // source did name — it keeps its old job of holding the small ones
        // back inside that zone. A zero floor stays silent: `0` is the
        // documented spelling of "no floor" and every profile ever written
        // carries it, so reading it as a selector would authorize dehydrating
        // every unconfigured folder in existence on the next sync.
        let floor_selects = patterns.is_empty() && profile.virtual_over_bytes > 0;

        // Protections accumulate across every source; only the permissive half
        // is either-or. AD-123: a policy edit may widen what may leave and may
        // never narrow what is kept, so a machine restating the repository's
        // zone cannot silently discard the repository's own exceptions.
        let mut never = Parsed::entries(FILE_SOURCE, &file.never);
        never.extend(Parsed::entries(PROFILE_SOURCE, &from_profile.never));
        let never = PatternSet::anchored(&never)?;

        // What the policy in force came from, in the order it is decided above:
        // the profile when it supplied the permissive list, the floor when no
        // list authorized anything and a positive floor therefore does, the
        // file when it stated the list, and the profile again when it is the
        // only source that spoke at all — a protections-only profile list over
        // no file is a policy, and reporting `Unset` for it would say nothing
        // configured anything.
        //
        // `overrides` and `floor_selects` are mutually exclusive — `overrides`
        // implies a non-empty `patterns` — so their order relative to each
        // other is immaterial, and it reads best with the two pattern-list
        // answers kept together.
        let tier = if overrides {
            VirtualPolicyTier::Profile
        } else if floor_selects {
            VirtualPolicyTier::SizeFloor
        } else if file.says_something() {
            VirtualPolicyTier::PatternFile
        } else if from_profile.says_something() {
            VirtualPolicyTier::Profile
        } else {
            VirtualPolicyTier::Unset
        };

        Ok(Self {
            patterns,
            never,
            // Only the profile tiers can carry the floor: a size is not
            // expressible in gitignore dialect, so the committed file has no
            // spelling for it at all.
            over_bytes: profile.virtual_over_bytes,
            floor_selects,
            tier,
        })
    }

    /// Whether `rela`'s content may stay away.
    ///
    /// `rela` is repository-relative, which is what the patterns are written
    /// against — matching an absolute path would make `*.mp4` depend on where
    /// the folder happens to be mounted. An input that leaves that frame, by
    /// being absolute or by carrying a `..`, is answered `Materialize` rather
    /// than matched: a documented precondition that only warns in a comment is
    /// one a caller breaks, and the consequence of breaking this one would be
    /// authorizing something outside the repository. `size` is the content's
    /// size, which the caller reads off the pointer.
    ///
    /// Performs **no I/O** (FR-328): everything it consults was compiled at
    /// [`Self::compile`] time. That is what makes resolving 10 000 paths cost no
    /// worktree read and no network call.
    ///
    /// A protection wins unconditionally over a matching pattern — unlike
    /// gitignore's last-match-wins — because this decision is about somebody's
    /// bytes and the only safe direction to err in is keeping them. Same shape,
    /// and same reason, as [`crate::lfs::stage::LfsPolicy::applies`].
    ///
    /// The size floor is a floor under every other term and, when nothing else
    /// selects, the term that selects: with no permissive line in force from
    /// any source and a positive floor, every path at or above it may stay away
    /// (Story 56.16). It never reaches past the two gates above it — a
    /// protection and a control file each still win unconditionally — so the
    /// widest configuration keeper can express is still inside AD-123's rule.
    ///
    /// A `Virtual` answer is an authorization, never an instruction, and it says
    /// nothing about whether the path is an LFS candidate at all: the caller
    /// hands over paths that already hold pointer text, so a path this profile
    /// never routed through LFS has no pointer for the answer to act on. That is
    /// why the routing rules — `lfs_mode`, `lfs_threshold_bytes`, `lfs_never` —
    /// are deliberately not consulted here (AD-122).
    pub fn resolve(&self, rela: &Path, size: u64) -> Virtualization {
        if !is_inside_the_repository(rela) || is_control_file(rela) {
            return Virtualization::Materialize;
        }
        if size < self.over_bytes || self.never.matches(rela) {
            return Virtualization::Materialize;
        }
        if self.floor_selects || self.patterns.matches(rela) {
            Virtualization::Virtual
        } else {
            Virtualization::Materialize
        }
    }

    /// What the policy in force came from — which pattern list, or the size
    /// floor when no list authorized anything.
    pub fn tier(&self) -> VirtualPolicyTier {
        self.tier
    }

    /// Whether **any** path could be answered [`Virtualization::Virtual`] at
    /// all.
    ///
    /// Distinct from [`Self::tier`], and the distinction is load-bearing rather
    /// than pedantic. `tier` answers *what decided that anything may stay
    /// away*, and it is `Unset` only when no source said anything — but a
    /// source that consists of nothing but `!` protections **does** say
    /// something, so a committed `.keepervirtual` holding only `!30-masters/**`
    /// compiles to an empty permissive set and a tier of
    /// [`VirtualPolicyTier::PatternFile`]. A caller asking "is there any point
    /// consulting me?" — [`crate::engine`]'s release gate is the one that must,
    /// because the alternative is opening the repository and reading the index
    /// once per candidate row to be told no — has to ask this, not the tier.
    ///
    /// It answers about the permissive half only — a protection cannot make a
    /// path virtual, so a policy carrying nothing but protections answers
    /// `Materialize` for every path however many it carries — plus the floor,
    /// which since Story 56.16 selects on its own when no list authorized
    /// anything. The floor has to participate here: it is what says something
    /// in that state, and a gate that asked only about patterns would skip
    /// precisely the folder whose only control is the floor.
    ///
    /// Answering yes is not only a cost question, and counting the floor is
    /// where that stops being a detail. `engine::release_mode_gate` refuses a
    /// whole `LfsMode::Materialize` folder with
    /// `ContentRefusal::AlwaysMaterializes` when this answers false, so
    /// including the floor here **arms the release sweep** — the pass that
    /// removes local content — for folders that were exempt from it before.
    /// That is the point rather than a side effect: a folder that may never let
    /// go of a byte can never become light, and light is what `tgdrive-light`
    /// was named and configured for. What the sweep gains is permission to
    /// consider the folder, never permission to delete — every individual
    /// deletion is still gated by its own per-object proof (the committed
    /// pointer's identity hash, `remote_serves` re-checked at the moment of
    /// deletion, the pin read taken twice, the fail-closed open-file probe), so
    /// AD-123 is exactly where it was. And the folders whose behaviour moves
    /// are precisely those with a positive floor and no permissive pattern from
    /// any source: folders whose floor was until now a dead control, typed
    /// deliberately by a person, since the default is `0` and `0` never makes
    /// `floor_selects` true.
    pub fn authorizes_anything(&self) -> bool {
        self.floor_selects || !self.patterns.is_empty()
    }
}

/// Refuse a `virtualPatterns` list [`VirtualPolicy::compile`] would refuse
/// later (Story 56.14).
///
/// Called from [`crate::profile::SyncProfile::validate`], which runs on every
/// write and on every load, so a malformed glob is refused at the box the
/// person typed it into rather than at the next sync. `compile` is the
/// authority on what is well formed and this shares its machinery — the same
/// [`Parsed::of`] with [`Comments::Literal`], the same
/// [`PatternSet::anchored`] — so it can neither accept something `compile`
/// will refuse nor refuse something `compile` would accept.
///
/// It checks the profile list only, and both halves of it: a malformed
/// protection is as fatal to `compile` as a malformed authorization, and
/// `compile` reads both from the same field.
///
/// The compiled sets are discarded. `validate` is asked whether this is
/// writable, not what it means, and the answer costs one `GlobSet` build over
/// a list a person typed by hand.
pub fn check_patterns(entries: &[String]) -> Result<()> {
    let parsed = Parsed::of(entries.iter(), Comments::Literal);
    PatternSet::anchored(&Parsed::entries(PROFILE_SOURCE, &parsed.patterns))?;
    PatternSet::anchored(&Parsed::entries(PROFILE_SOURCE, &parsed.never))?;
    Ok(())
}

/// Whether a leading `#` starts a remark in this source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comments {
    /// A line file: `#` opens a comment, and `\#` escapes a literal one.
    Recognized,
    /// A TOML array: the format carries its own comments, so a `#` here is the
    /// first character of a path and nothing else.
    Literal,
}

/// One policy source, split into its permissive and protective lines, each
/// already anchored and paired with the line the user typed.
///
/// `never` is not a separate config key: gitignore dialect already spells "not
/// this one" as `!`, so the negated lines of a source *are* its protections.
/// Carrying the original beside the anchored form is what lets a refusal read
/// back the `!` the user wrote instead of a glob keeper derived.
#[derive(Debug, Default)]
struct Parsed {
    patterns: Vec<(String, String)>,
    never: Vec<(String, String)>,
}

impl Parsed {
    /// Parse one source's lines. Never fails: a line that says nothing is
    /// dropped here, and only a malformed *glob* is an error, which the compile
    /// step raises.
    fn of<'a, S>(lines: impl Iterator<Item = &'a S>, comments: Comments) -> Self
    where
        S: AsRef<str> + ?Sized + 'a,
    {
        let mut out = Self::default();
        for line in lines {
            let original = line.as_ref().trim();
            // A blank line says nothing. It emphatically does not mean "match
            // everything" — that reading would virtualize an entire repository
            // off one stray newline.
            if original.is_empty() {
                continue;
            }
            // A comment says nothing either. Checked before the escape rule, so
            // `#` keeps its comment meaning unless it was escaped.
            if comments == Comments::Recognized && original.starts_with('#') {
                continue;
            }
            // `\!` and `\#` are how a file whose name really starts with `!` or
            // `#` is named — without the escape those two filenames would be
            // unreachable, which is gitignore's own answer to the same problem.
            let (negated, body) = match original.strip_prefix('\\') {
                Some(rest) if rest.starts_with('!') || rest.starts_with('#') => (false, rest),
                _ => match original.strip_prefix('!') {
                    Some(rest) => (true, rest.trim()),
                    None => (false, original),
                },
            };
            let Some(effective) = anchor_line(body) else {
                // A bare `!`, a bare `/`, a `!/`: punctuation with no pattern
                // behind it. Dropped like a blank line, and — unlike the shape
                // this replaced — not counted as the source having spoken, so
                // `tier` cannot claim a policy that compiled to nothing.
                continue;
            };
            let target = if negated {
                &mut out.never
            } else {
                &mut out.patterns
            };
            if negated {
                // A protection also protects everything beneath it. The
                // permissive half gets no such expansion, and the asymmetry is
                // the point: widening a protection can only keep more bytes,
                // while widening an authorization would let go of bytes nobody
                // named. Without this, `!40-media/secret` — the obvious way to
                // write "not that folder" — protects only the directory entry,
                // which is never a path this is asked about, so every file
                // inside it stays authorized to leave.
                if !effective.ends_with("/**") {
                    target.push((format!("{effective}/**"), original.to_owned()));
                }
            }
            target.push((effective, original.to_owned()));
        }
        out
    }

    /// Pair each anchored line with the list it came from, ready for
    /// [`PatternSet::anchored`].
    fn entries<'a>(
        source: &'a str,
        lines: &'a [(String, String)],
    ) -> Vec<(&'a str, &'a str, &'a str)> {
        lines
            .iter()
            .map(|(effective, original)| (source, effective.as_str(), original.as_str()))
            .collect()
    }

    /// Whether this source configured anything at all. A file of nothing but
    /// comments is a file that says nothing, and the tier must report that
    /// honestly rather than claiming a policy is in force.
    fn says_something(&self) -> bool {
        !self.patterns.is_empty() || !self.never.is_empty()
    }
}

/// Resolve gitignore's three anchoring spellings into one glob.
///
/// All three are in the corpus a user arrives with, and before this the two
/// keeper had never handled compiled to a glob that matched nothing at all —
/// silently, which for a protection line means the bytes it named were
/// authorized to leave. Returns `None` for punctuation with no pattern behind
/// it.
///
/// * A leading `/` anchors at the repository root and is *not* re-run through
///   the basename rule, which is what makes `/keep.mp4` mean the root's own
///   `keep.mp4` rather than every `keep.mp4` at any depth.
/// * A trailing `/` names a directory, so the glob covers what is under it. It
///   does not anchor on its own: `40-media/` is a `40-media` at any depth,
///   exactly as gitignore reads it.
/// * Anything else takes [`crate::exclude::anchor`]'s rule: no `/` matches that
///   basename at any depth, otherwise the pattern is root-anchored.
fn anchor_line(body: &str) -> Option<String> {
    let (rooted, body) = match body.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let (directory, body) = match body.strip_suffix('/') {
        Some(stem) => (true, stem),
        None => (false, body),
    };
    if body.is_empty() {
        return None;
    }
    let base = if rooted || body.contains('/') {
        body.to_owned()
    } else {
        crate::exclude::anchor(body).into_owned()
    };
    Some(if directory {
        format!("{base}/**")
    } else {
        base
    })
}

/// Whether `rela` names something the policy is allowed to have an opinion
/// about at all.
///
/// The patterns are written against repository-relative paths. An absolute path
/// or one carrying a `..` is outside that frame, and `exclude::match_string`
/// silently drops a root component — so `/home/u/tgdrive/a.mp4` would be matched
/// as `home/u/tgdrive/a.mp4` and a basename pattern would answer `Virtual` for
/// a file this repository does not contain.
///
/// A path with no `Normal` component at all — `""`, `"."` — is the third
/// violation of the same frame, and the one Story 56.16 made reachable. Both
/// are relative and neither carries a `..`, so they used to pass this guard,
/// and they still answered `Materialize` only because `exclude::match_string`
/// renders both to the empty string and `PatternSet::matches` refuses an empty
/// candidate. A floor that selects on its own short-circuits ahead of the
/// pattern set, so under a floor-only policy the repository root itself
/// answered `Virtual`. Requiring one named component is the fix, and it belongs
/// here rather than downstream: `engine`'s release scan reaches `resolve` with a
/// ledger-supplied `Path::new(&row.path)` whose contents this module does not
/// get to choose, and the module's own stated position is that a precondition
/// only warned about in a comment is one a caller breaks.
fn is_inside_the_repository(rela: &Path) -> bool {
    if !rela.is_relative() {
        return false;
    }
    // One walk, because both remaining answers come from the same components: a
    // `..` anywhere leaves the frame, and a path that never names anything is
    // not a path inside it.
    let mut names_something = false;
    for part in rela.components() {
        match part {
            Component::ParentDir => return false,
            Component::Normal(_) => names_something = true,
            _ => {}
        }
    }
    names_something
}

/// Files whose own bytes carry the rules, and which therefore may never be
/// authorized to stay away, whatever a pattern says.
///
/// [`crate::exclude`] refuses to let a user pattern take `.keeper/keeper.toml`
/// out of sync for the same reason, with its own test: a mechanism must not be
/// able to lose the file it is configured by. Here the hazard is sharper,
/// because each of these files still *exists* — it just reads as pointer text.
/// A virtualized `.keepervirtual` is a policy that erases itself, and `compile`
/// would then parse `version https://git-lfs.github.com/spec/v1` as a pattern.
/// A virtualized `.gitattributes` breaks LFS routing for its whole subtree.
fn is_control_file(rela: &Path) -> bool {
    if crate::lfs::stage::is_git_control_file(rela) {
        return true;
    }
    let name = rela.file_name().and_then(|name| name.to_str());
    if name.is_some_and(|name| name == VIRTUAL_PATTERN_FILE || name == ".lfsconfig") {
        return true;
    }
    rela.components().any(|part| {
        matches!(part, Component::Normal(part) if part == ".git" || part == FOLDER_CONFIG_DIR)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::FolderTier;

    fn profile(root: &Path) -> SyncProfile {
        SyncProfile::new("01JVIRT", "media", root, "https://git.invalid/r.git")
    }

    /// A temp worktree, optionally holding a committed pattern file. Real files
    /// throughout: the whole claim of this module is about what is on disk, and
    /// a hand-mocked source would assert nothing.
    fn worktree(pattern_file: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        if let Some(text) = pattern_file {
            std::fs::write(dir.path().join(VIRTUAL_PATTERN_FILE), text)
                .expect("write .keepervirtual");
        }
        dir
    }

    /// Write one folder config file, as `profile::folder`'s own tests do.
    fn folder_file(root: &Path, name: &str, text: &str) {
        let keeper = root.join(FOLDER_CONFIG_DIR);
        std::fs::create_dir_all(&keeper).expect("create .keeper");
        std::fs::write(keeper.join(name), text).expect("write folder config");
    }

    /// Nothing configured must be nothing virtual — not "everything", which is
    /// what an empty pattern set read as a wildcard would mean.
    #[test]
    fn nothing_configured_leaves_every_path_materialized() {
        let dir = worktree(None);
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
        for path in ["a.mp4", "40-media/a.mp4", "deep/er/still/a.iso"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: with no policy configured nothing may stay away"
            );
        }
    }

    /// The anchoring rule is gitignore's, and both halves of it matter: a
    /// pattern with a `/` covers its own zone, and only that zone. A
    /// `40-media/**` that also swallowed `30-work/40-media/` would dehydrate a
    /// folder the user never named.
    #[test]
    fn a_root_anchored_pattern_covers_its_own_zone_and_nothing_named_like_it_elsewhere() {
        let dir = worktree(Some("40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::PatternFile);
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("10-notes/a.md"), 10_000_000),
            Virtualization::Materialize,
            "a zone the policy never named keeps its content"
        );
        assert_eq!(
            policy.resolve(Path::new("30-work/40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "a pattern with a slash is anchored at the root, not matched at any depth"
        );
    }

    /// The other half of the same rule: a bare basename covers every depth, so a
    /// user who wrote `*.mp4` does not have to enumerate their folders.
    #[test]
    fn a_pattern_without_a_slash_matches_that_basename_at_any_depth() {
        let dir = worktree(Some("*.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("a/b/c.mp4"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("a/b/c.md"), 10_000_000),
            Virtualization::Materialize
        );
    }

    /// Negation is how `never` gets populated, and it wins unconditionally —
    /// not last-match-wins. Ordering the lines the other way round must not
    /// change the answer, because the file that survives here is somebody's.
    #[test]
    fn a_negated_line_keeps_one_files_bytes_while_its_siblings_may_leave() {
        for text in [
            "40-media/**\n!40-media/keep.mp4\n",
            "!40-media/keep.mp4\n40-media/**\n",
        ] {
            let dir = worktree(Some(text));
            let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
            assert_eq!(
                policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
                Virtualization::Materialize,
                "the negated path must keep its content whatever the line order"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
                Virtualization::Virtual,
                "and its siblings are still covered"
            );
        }
    }

    /// A stray newline or a comment must not be read as a pattern. Read as
    /// "match everything" — the way an empty glob would be — one blank line
    /// would authorize dehydrating an entire repository.
    #[test]
    fn comments_and_blank_lines_say_nothing_and_never_mean_match_everything() {
        let dir = worktree(Some("# note about the policy\n\n   \n*.iso\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("50-iso/debian.iso"), 10_000_000),
            Virtualization::Virtual,
            "the one real line is still in force"
        );
        for path in ["10-notes/a.md", "note about the policy", "#"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: a comment or a blank line matches nothing at all"
            );
        }

        // And a file of nothing but noise is a file that says nothing, which the
        // tier has to report honestly.
        let quiet = worktree(Some("# only a comment\n\n"));
        let policy = VirtualPolicy::compile(&profile(quiet.path())).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
    }

    /// Without the escape, a file whose name starts with `!` or `#` would be
    /// unnameable — every line describing it would be read as a negation or a
    /// comment.
    #[test]
    fn a_backslash_escapes_a_filename_that_really_starts_with_a_bang() {
        let dir = worktree(Some("\\!literal.bin\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("!literal.bin"), 10_000_000),
            Virtualization::Virtual,
            "the escaped line names a real file, it is not a negation"
        );
        assert_eq!(
            policy.resolve(Path::new("literal.bin"), 10_000_000),
            Virtualization::Materialize,
            "and the `!` is part of the name, not stripped from it"
        );
    }

    /// Inclusive at the floor, exactly as `LfsPolicy`'s threshold is. Two
    /// size rules in one engine that disagreed about their own boundary would
    /// be a trap: a file could be routed through LFS and then refused
    /// virtualization for being one byte too small.
    #[test]
    fn the_size_floor_is_inclusive_so_a_file_exactly_at_it_may_stay_away() {
        let dir = worktree(Some("*.bin\n"));
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("a.bin"), 1023),
            Virtualization::Materialize,
            "below the floor the pattern does not matter"
        );
        assert_eq!(
            policy.resolve(Path::new("a.bin"), 1024),
            Virtualization::Virtual,
            "at the floor it does — the boundary is inclusive"
        );
    }

    /// The profile tiers override the repository's file wholesale rather than
    /// merging with it: a machine that says "only these" must not silently
    /// inherit the file's list as well, or one host would dehydrate paths its
    /// own configuration never named.
    #[test]
    fn a_non_empty_profile_list_replaces_the_committed_file_wholesale() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["50-iso/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("50-iso/debian.iso"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "the file's list is replaced, not merged into"
        );
    }

    /// An empty list is silence, not "override with nothing" — the same reading
    /// `lfs_never.is_empty()` already takes. Otherwise every profile that had
    /// never been edited would mute the repository's own committed policy.
    #[test]
    fn an_empty_profile_list_is_silence_so_the_committed_file_stays_in_force() {
        let dir = worktree(Some("40-media/**\n"));
        let p = profile(dir.path());
        assert!(
            p.virtual_patterns.is_empty(),
            "the default is an empty list"
        );
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::PatternFile);
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
    }

    /// The folder TOML tier is the canonical home for anything both hosts must
    /// honour (AD-132), and it is a *real* file resolved by the real tier here:
    /// a hand-built profile would assert nothing about whether `virtualPatterns`
    /// is even an accepted folder key.
    #[test]
    fn a_real_folder_toml_layer_overrides_the_committed_file() {
        let dir = worktree(Some("40-media/**\n"));
        folder_file(
            dir.path(),
            "keeper.toml",
            "[folder]\nvirtualPatterns = [\"70-vm/**\"]\n",
        );
        let outcome = FolderTier::new("hesperia", None).apply(&profile(dir.path()));
        assert!(
            outcome.faults.is_empty(),
            "the folder file must be accepted, got {:?}",
            outcome.faults
        );
        let policy = VirtualPolicy::compile(&outcome.profile).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("70-vm/disk.img"), 10_000_000),
            Virtualization::Virtual,
            "the folder file's list is in force"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and it replaced the committed file's list"
        );
    }

    /// AD-99's precedence, asserted through the tier rather than assumed: this
    /// machine's file wins where it speaks, and the shared file still wins on
    /// every key it alone sets — which is what stops a per-host override from
    /// muting the folder's shared floor.
    #[test]
    fn the_host_folder_file_wins_and_the_shared_one_still_speaks_where_it_alone_does() {
        let dir = worktree(None);
        folder_file(
            dir.path(),
            "keeper.toml",
            "[folder]\nvirtualPatterns = [\"70-vm/**\"]\nvirtualOverBytes = 4096\n",
        );
        folder_file(
            dir.path(),
            "keeper.hesperia.toml",
            "[folder]\nvirtualPatterns = [\"80-hosts/**\"]\n",
        );
        let outcome = FolderTier::new("hesperia", None).apply(&profile(dir.path()));
        assert!(
            outcome.faults.is_empty(),
            "both folder files must be accepted, got {:?}",
            outcome.faults
        );
        let policy = VirtualPolicy::compile(&outcome.profile).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("80-hosts/disk.img"), 10_000_000),
            Virtualization::Virtual,
            "this machine's file wins on the key it sets"
        );
        assert_eq!(
            policy.resolve(Path::new("70-vm/disk.img"), 10_000_000),
            Virtualization::Materialize,
            "the shared list was replaced, not merged"
        );
        assert_eq!(
            policy.resolve(Path::new("80-hosts/disk.img"), 4095),
            Virtualization::Materialize,
            "the shared file's floor still applies: the host file never set one"
        );
    }

    /// AD-98: the folder file outranks the stored row on every read, which is
    /// what lets the file keep winning after a user has once edited the form.
    #[test]
    fn a_folder_file_outranks_the_stored_profile_row() {
        let dir = worktree(None);
        folder_file(
            dir.path(),
            "keeper.toml",
            "[folder]\nvirtualPatterns = [\"70-vm/**\"]\n",
        );
        let mut stored = profile(dir.path());
        stored.virtual_patterns = vec!["90-stored/**".to_owned()];
        let outcome = FolderTier::new("hesperia", None).apply(&stored);
        assert!(
            outcome.faults.is_empty(),
            "the folder file must be accepted, got {:?}",
            outcome.faults
        );
        let policy = VirtualPolicy::compile(&outcome.profile).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("70-vm/disk.img"), 10_000_000),
            Virtualization::Virtual,
            "the file wins"
        );
        assert_eq!(
            policy.resolve(Path::new("90-stored/disk.img"), 10_000_000),
            Virtualization::Materialize,
            "the stored row is the base the file overrides, not a merge partner"
        );
    }

    /// A policy made of nothing but protections is **in force** and authorizes
    /// **nothing** — and the two questions have to be asked separately (Story
    /// 56.10).
    ///
    /// A repository owner pre-protecting a zone before authorizing any is an
    /// ordinary thing to commit, and `tier` says `PatternFile` for it, quite
    /// correctly: a list did decide what may stay away, and the answer was
    /// "nothing". A caller that read the tier as "there is something here worth
    /// asking about" would open the repository and parse the index once per
    /// candidate row on every sync of that folder, forever, to be told no.
    #[test]
    fn a_policy_of_only_protections_is_in_force_and_authorizes_nothing() {
        let dir = worktree(Some("!30-masters/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::PatternFile,
            "a `!` line is a source saying something"
        );
        assert!(
            !policy.authorizes_anything(),
            "...and it authorizes nothing, which is the question a caller has to ask"
        );
        for path in ["30-masters/a.mp4", "40-media/a.mp4", "a.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: nothing may stay away under a policy with no permissive line"
            );
        }

        let named = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(named.path())).expect("compiles");
        assert!(
            policy.authorizes_anything(),
            "one permissive line is what makes the policy worth consulting"
        );
    }

    /// A profile list of nothing but `!` protections does **not** replace the
    /// committed file's permissive list (Story 56.14).
    ///
    /// A protection is not a claim about the zone — it is an exception inside
    /// whatever zone is in force — so only the PERMISSIVE half of a profile
    /// list can override the committed one. Without the fix `overrides` asked
    /// `says_something()`, which either half satisfies: the empty profile
    /// permissive list replaced the committed `40-media/**`,
    /// `authorizes_anything()` went false, `40-media/other.mp4` read
    /// `Materialize` and `tier()` reported `Profile` — so a machine restating
    /// ONE exception silently switched the whole folder's virtualization off
    /// from a line written to PROTECT one path, which narrows what is
    /// authorized to nothing and is the inverse of AD-123's rule.
    #[test]
    fn a_profile_list_of_only_protections_does_not_replace_the_committed_zone() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["!40-media/keep.mp4".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
            Virtualization::Virtual,
            "the repository's zone survives a machine that only named an exception"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and the machine's own protection applies inside it"
        );
        assert!(
            policy.authorizes_anything(),
            "a protection cannot empty the permissive list it was written against"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::PatternFile,
            "the file supplied the list in force, so that is the file to edit"
        );
    }

    /// A profile list of only protections, over no committed file at all, is
    /// still a policy and still authorizes nothing (Story 56.14).
    ///
    /// This is the tier fallback the fix adds: the profile is the only source
    /// that spoke, so reporting `Unset` would say nothing configured anything.
    /// Without the fix the answer was `Profile` for the wrong reason — an
    /// override that had discarded a list — and after the naive repair, which
    /// only tightens `overrides`, it would read `Unset` for a folder that does
    /// carry a policy.
    #[test]
    fn a_protections_only_profile_list_over_no_file_is_still_the_profiles_policy() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["!30-masters/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert!(
            !policy.authorizes_anything(),
            "no source stated a permissive line, so nothing may stay away"
        );
        for path in ["30-masters/a.mp4", "40-media/a.mp4", "a.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: a policy with no permissive line authorizes nothing"
            );
        }
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::Profile,
            "the profile is the only source that spoke, and it did speak"
        );
    }

    /// `check_patterns` refuses exactly what `compile` would refuse, so the
    /// Settings form can say no at the box (Story 56.14, FR-329).
    ///
    /// FR-329 says a malformed pattern is a hard `SyncError::Config` naming its
    /// source and never a silently dropped pattern. Without the fix there was
    /// no such entry point at all: nothing compiled the list before it was
    /// stored, so `scans/[` was accepted and only the next sync said so.
    #[test]
    fn check_patterns_accepts_every_legitimate_list_and_refuses_a_malformed_glob() {
        check_patterns(&[]).expect("an empty list is the documented spelling of silence");
        check_patterns(&["!30-masters/**".to_owned()])
            .expect("a list of only protections is a policy about exceptions");
        check_patterns(&["40-media/**".to_owned(), "!40-media/keep.mp4".to_owned()])
            .expect("and a well-formed list of both halves is the ordinary case");

        let err = check_patterns(&["scans/[".to_owned()])
            .expect_err("an unclosed character class must not be saveable");
        assert!(
            matches!(err, SyncError::Config(_)),
            "a bad glob is a configuration refusal, got {err:?}"
        );
        let text = format!("{err}");
        assert!(
            text.contains("scans/["),
            "the message must quote the entry as typed, got: {text}"
        );
        assert!(
            text.contains(PROFILE_SOURCE),
            "and name the list it is in, got: {text}"
        );
    }

    /// A typo must name the file it is in. Silently dropping the line would let
    /// a repository believe it had a policy it does not have (FR-329).
    #[test]
    fn a_malformed_glob_in_the_committed_file_names_the_file_and_quotes_the_line() {
        let dir = worktree(Some("[unclosed\n"));
        let err = VirtualPolicy::compile(&profile(dir.path())).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains(VIRTUAL_PATTERN_FILE),
            "the message must say which source the typo is in, got: {text}"
        );
        assert!(
            text.contains("[unclosed"),
            "the message must quote the line as typed, got: {text}"
        );
    }

    /// And a typo in one machine's TOML must name *that* key, or the user goes
    /// looking in the committed file for a line that is not there.
    #[test]
    fn a_malformed_glob_in_the_profile_list_names_the_config_key_and_quotes_the_entry() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["[unclosed".to_owned()];
        let err = VirtualPolicy::compile(&p).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains("virtualPatterns"),
            "the message must name the config key, got: {text}"
        );
        assert!(
            text.contains("[unclosed"),
            "the message must quote the entry as typed, got: {text}"
        );
        assert!(
            !text.contains(VIRTUAL_PATTERN_FILE),
            "and must not send the user to a file they did not edit, got: {text}"
        );
    }

    /// A negated line is compiled from its remainder, so without care the
    /// refusal would quote a string the user never typed and they would search
    /// the file for it in vain.
    #[test]
    fn a_malformed_negated_glob_is_quoted_with_the_bang_the_user_typed() {
        let dir = worktree(Some("40-media/**\n![unclosed\n"));
        let err = VirtualPolicy::compile(&profile(dir.path())).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains("\"![unclosed\""),
            "the message must quote the negation as typed, got: {text}"
        );
    }

    /// Absence is silence: most repositories will never carry this file, and
    /// demanding it would make the ordinary case an error.
    #[test]
    fn an_absent_pattern_file_is_silence_not_a_failure() {
        let dir = worktree(None);
        assert!(!dir.path().join(VIRTUAL_PATTERN_FILE).exists());
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("absence is not an error");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
    }

    /// A policy we cannot read is not a policy. Reading an unreadable file as
    /// "nothing is virtual" is a plausible answer to the wrong question, and it
    /// would hide a broken checkout for as long as nobody looked.
    #[test]
    fn a_directory_named_like_the_pattern_file_is_refused_with_the_os_reason() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join(VIRTUAL_PATTERN_FILE)).expect("create a directory");
        let err = VirtualPolicy::compile(&profile(dir.path())).expect_err("must refuse");
        assert!(
            matches!(err, SyncError::Config(_)),
            "an unreadable policy is a config fault, got {err:?}"
        );
        let text = format!("{err}");
        assert!(
            text.contains(VIRTUAL_PATTERN_FILE) && text.contains("could not read"),
            "the message must name the file and the OS reason, got: {text}"
        );
    }

    /// FR-328, proved the only way it can be: the worktree is deleted out from
    /// under the compiled policy, and the answers do not change. If `resolve`
    /// touched the filesystem at all this test could not pass.
    #[test]
    fn resolve_answers_identically_after_the_entire_worktree_is_deleted() {
        let dir = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let root = dir.path().to_path_buf();
        let mut p = profile(&root);
        p.virtual_over_bytes = 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        let cases = [
            ("40-media/a.mp4", 10_000u64, Virtualization::Virtual),
            ("40-media/keep.mp4", 10_000, Virtualization::Materialize),
            ("40-media/tiny.mp4", 10, Virtualization::Materialize),
            ("10-notes/a.md", 10_000, Virtualization::Materialize),
        ];
        for (path, size, want) in cases {
            assert_eq!(policy.resolve(Path::new(path), size), want, "{path} before");
        }

        std::fs::remove_dir_all(&root).expect("delete the whole worktree");
        assert!(!root.exists(), "the worktree must really be gone");
        for (path, size, want) in cases {
            assert_eq!(
                policy.resolve(Path::new(path), size),
                want,
                "{path}: resolve must answer from the compiled policy alone"
            );
        }
    }

    /// FR-330 and AD-123. This pins the git-lfs#3092 failure mode: there, a
    /// pattern change dropped content that existed nowhere else. Editing a
    /// policy here may change only the answer — deleting a byte needs
    /// per-object proof, which this module never has and never asks for.
    #[test]
    fn editing_the_policy_changes_only_the_answer_and_never_a_byte() {
        let dir = worktree(Some("50-iso/**\n"));
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("40-media")).expect("create the zone");
        let payload = vec![7u8; 4096];
        let target = root.join("40-media/a.mp4");
        std::fs::write(&target, &payload).expect("write real bytes");

        let before = VirtualPolicy::compile(&profile(&root)).expect("compiles");
        assert_eq!(
            before.resolve(Path::new("40-media/a.mp4"), payload.len() as u64),
            Virtualization::Materialize,
            "the first policy does not cover the file"
        );

        std::fs::write(root.join(VIRTUAL_PATTERN_FILE), "40-media/**\n").expect("rewrite policy");
        let after = VirtualPolicy::compile(&profile(&root)).expect("recompiles");
        assert_eq!(
            after.resolve(Path::new("40-media/a.mp4"), payload.len() as u64),
            Virtualization::Virtual,
            "the edited policy now covers it"
        );

        let on_disk = std::fs::read(&target).expect("the file must still be there");
        assert_eq!(
            on_disk.len(),
            payload.len(),
            "the file's length must be untouched by a policy edit"
        );
        assert_eq!(
            on_disk, payload,
            "and its bytes byte-for-byte: authorizing virtualization is not deleting"
        );
    }

    /// The two fields are very nearly opposites, and the names are close enough
    /// that conflating them is a live risk: `lfs_never` says "never route this
    /// through LFS", so a file matched there has no pointer to stay away behind
    /// at all. Reading it as a virtualization request would dehydrate exactly
    /// the files the user asked to keep as ordinary blobs.
    #[test]
    fn lfs_never_is_not_a_virtualization_request_and_neither_field_moves_the_other() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.lfs_never = vec!["*.mp4".to_owned()];
        assert!(p.virtual_patterns.is_empty());

        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
        assert_eq!(
            policy.resolve(Path::new("a.mp4"), 10_000),
            Virtualization::Materialize,
            "an LFS opt-out is not a request to virtualize"
        );

        let lfs = crate::lfs::stage::LfsPolicy::from_profile(&p).expect("compiles");
        assert!(
            !lfs.applies(Path::new("a.mp4"), 10_000),
            "and the opt-out still means what it always meant"
        );

        // The other direction: a virtual pattern must not start routing a path
        // through LFS that the user excluded, nor exclude one they did not.
        let mut both = p.clone();
        both.virtual_patterns = vec!["*.iso".to_owned()];
        let lfs_both = crate::lfs::stage::LfsPolicy::from_profile(&both).expect("compiles");
        assert!(
            !lfs_both.applies(Path::new("a.mp4"), 10_000),
            "the opt-out is unchanged by the virtual list"
        );
        assert!(
            lfs_both.applies(Path::new("a.iso"), 10_000_000),
            "and a virtualized path is still an ordinary LFS candidate"
        );
        let virtual_both = VirtualPolicy::compile(&both).expect("compiles");
        assert_eq!(
            virtual_both.resolve(Path::new("a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and lfs_never never widens the virtual list"
        );
    }

    /// `resolve` is written against repository-relative paths, and the earlier
    /// version of this test proved that only for a root-anchored pattern, where
    /// it held by accident because the leading segments did not line up. A
    /// basename pattern is the dialect's default shape, and for one of those an
    /// absolute path used to answer `Virtual`: `match_string` drops the root
    /// component, so `/home/u/tgdrive/a.mp4` was matched as a relative path this
    /// repository does not contain.
    #[test]
    fn a_path_outside_the_repository_is_never_authorized_to_stay_away() {
        let dir = worktree(Some("*.mp4\n40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        for outside in [
            "/home/u/tgdrive/40-media/a.mp4",
            "/etc/a.mp4",
            "../../etc/a.mp4",
            "40-media/../../etc/a.mp4",
        ] {
            assert_eq!(
                policy.resolve(Path::new(outside), 10_000_000),
                Virtualization::Materialize,
                "{outside} is outside the frame the patterns are written against"
            );
        }
        // The relative form of the same name is still answered normally, so the
        // guard rejects the frame violation and not the pattern.
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
    }

    /// Gitignore's leading `/` is the first spelling anyone coming from
    /// `.gitignore` reaches for, and it used to compile to a glob that matched
    /// nothing — silently. On the protective side that is the whole failure:
    /// the one file the user explicitly named was authorized to leave.
    #[test]
    fn a_leading_slash_anchors_at_the_root_instead_of_matching_nothing() {
        let dir = worktree(Some("40-media/**\n!/40-media/keep.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "the protection the user wrote must actually protect"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
            Virtualization::Virtual,
            "and it must protect only what it names"
        );
    }

    /// The point of the leading slash is that it does NOT then go back through
    /// the basename rule: `/keep.mp4` is the root's own file, not every
    /// `keep.mp4` in the repository.
    #[test]
    fn a_root_anchored_single_segment_does_not_become_a_basename_rule() {
        let dir = worktree(Some("/keep.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("keep.mp4"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "a rooted pattern must not reach a same-named file deeper in the tree"
        );
    }

    /// A trailing slash names a directory, so the rule covers what is under it.
    /// It does not anchor on its own, which is gitignore's own reading.
    #[test]
    fn a_trailing_slash_covers_the_subtree_at_any_depth() {
        let dir = worktree(Some("scratch/\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        for inside in ["scratch/a.mp4", "40-media/scratch/a.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(inside), 10_000_000),
                Virtualization::Virtual,
                "{inside} is under a directory the policy named"
            );
        }
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize
        );
    }

    /// The natural pairing — virtualize a zone, but not one folder inside it —
    /// protected nothing in any of its three spellings, because a directory
    /// pattern only ever matched the directory entry and `resolve` is never
    /// asked about one. Every file the user meant to keep was authorized to go.
    #[test]
    fn negating_a_directory_protects_the_files_inside_it_in_every_spelling() {
        for line in ["!40-media/secret", "!40-media/secret/", "!/40-media/secret"] {
            let dir = worktree(Some(&format!("40-media/**\n{line}\n")));
            let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
            assert_eq!(
                policy.resolve(Path::new("40-media/secret/tax.pdf"), 10_000_000),
                Virtualization::Materialize,
                "{line} must protect what is inside it"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/secret/deep/passport.jpg"), 10_000_000),
                Virtualization::Materialize,
                "{line} must protect the whole subtree, not one level"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/clip.mp4"), 10_000_000),
                Virtualization::Virtual,
                "{line} must not protect the rest of the zone"
            );
        }
    }

    /// The permissive half gets no subtree expansion, and the asymmetry is
    /// deliberate: widening a protection can only keep more bytes, while
    /// widening an authorization would let go of bytes nobody named.
    #[test]
    fn naming_a_folder_without_a_slash_does_not_authorize_its_contents() {
        let dir = worktree(Some("40-media\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "a bare folder name authorizes nothing inside it; write `40-media/` or `40-media/**`"
        );
    }

    /// AD-123, and the git-lfs#3092 shape one layer up from the byte: a policy
    /// edit may widen what may leave and may never narrow what is kept. A host
    /// restating the repository's zone — believing it changed nothing — used to
    /// discard the repository's own exceptions along with the list.
    #[test]
    fn a_profile_list_cannot_drop_a_protection_the_committed_file_states() {
        let dir = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["40-media/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "the repository's protection survives a profile that replaced the zone"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
            Virtualization::Virtual,
            "while the profile's own list still decides what may leave"
        );
    }

    /// Protections union in both directions: a machine may add one the
    /// repository never stated, and it applies alongside the file's.
    #[test]
    fn a_machine_may_add_a_protection_of_its_own() {
        let dir = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["40-media/**".to_owned(), "!40-media/mine.mp4".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        for kept in ["40-media/keep.mp4", "40-media/mine.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(kept), 10_000_000),
                Virtualization::Materialize,
                "{kept} is protected by one source or the other"
            );
        }
    }

    /// A BOM is not whitespace, so `trim` leaves it attached and the first line
    /// stops being the pattern the user wrote. The dangerous half is a `!` line:
    /// prefixed by a BOM it no longer starts with `!`, so a line written to
    /// protect a file was filed as one that authorizes it.
    #[test]
    fn a_byte_order_mark_does_not_swallow_the_first_line_or_invert_it() {
        let dir = worktree(Some("\u{feff}40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual,
            "a BOM must not silently kill the first pattern"
        );

        let dir = worktree(Some("\u{feff}!40-media/keep.mp4\n40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and it must not turn a protection into an authorization"
        );
    }

    /// A mechanism must not be able to lose the file it is configured by.
    /// `crate::exclude` refuses to let a user pattern take `.keeper/keeper.toml`
    /// out of sync for the same reason. Here it is sharper: a virtualized
    /// `.keepervirtual` is a policy that erases itself, and the next `compile`
    /// would read `version https://git-lfs.github.com/spec/v1` as a pattern.
    #[test]
    fn keepers_own_control_files_are_never_authorized_to_stay_away() {
        let dir = worktree(Some("**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        for control in [
            VIRTUAL_PATTERN_FILE,
            ".gitattributes",
            "40-media/.gitattributes",
            ".gitignore",
            ".gitmodules",
            ".lfsconfig",
            ".keeper/keeper.toml",
            ".keeper/keeper.hesperia.toml",
            ".git/config",
        ] {
            assert_eq!(
                policy.resolve(Path::new(control), 10_000_000),
                Virtualization::Materialize,
                "{control} carries the rules; its own bytes can never be optional"
            );
        }
        // An ordinary file under the same broad pattern still resolves normally,
        // so the carve-out is a carve-out and not a blanket refusal.
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
    }

    /// A profile list that parses to nothing is silence, not an override. One
    /// stray whitespace entry in a folder TOML array used to disable a
    /// repository's entire committed policy, with `tier()` still reporting that
    /// the profile was in force.
    #[test]
    fn a_profile_list_of_nothing_but_noise_does_not_mute_the_committed_file() {
        for noise in [vec!["   ".to_owned()], vec!["!".to_owned()]] {
            let dir = worktree(Some("40-media/**\n"));
            let mut p = profile(dir.path());
            p.virtual_patterns = noise.clone();
            let policy = VirtualPolicy::compile(&p).expect("compiles");
            assert_eq!(
                policy.tier(),
                VirtualPolicyTier::PatternFile,
                "{noise:?} says nothing, so the committed file is still what is in force"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
                Virtualization::Virtual
            );
        }
    }

    /// Punctuation with no pattern behind it says nothing, and must not be
    /// counted as the source having spoken — `tier()` claiming a policy that
    /// compiled to nothing is the one thing it exists not to do.
    #[test]
    fn punctuation_with_no_pattern_behind_it_is_not_a_policy() {
        for text in ["!\n", "!   \n", "/\n", "!/\n"] {
            let dir = worktree(Some(text));
            let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
            assert_eq!(
                policy.tier(),
                VirtualPolicyTier::Unset,
                "{text:?} configured nothing, so no tier is in force"
            );
            assert_eq!(
                policy.resolve(Path::new("a.mp4"), 10_000_000),
                Virtualization::Materialize
            );
        }
    }

    /// A TOML array carries no comments — the format has its own — so a `#` in
    /// an entry is the first character of a path. Treating it as a remark made a
    /// path named `#drafts` unexpressible from the tier AD-132 calls canonical.
    #[test]
    fn a_hash_in_a_toml_entry_is_a_path_and_not_a_comment() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["#drafts/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("#drafts/a.mp4"), 10_000_000),
            Virtualization::Virtual,
            "the entry names a folder that really starts with a hash"
        );
    }

    /// The refusal has to name the list and quote the line as typed, `!` and
    /// all, for a protection as much as for an authorization — a user told
    /// `[unclosed` is bad while their line reads `![unclosed` looks in the wrong
    /// place, and an anchored glob keeper derived is not a line anyone wrote.
    #[test]
    fn a_malformed_protection_in_a_profile_entry_names_that_list() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["40-media/**".to_owned(), "![unclosed".to_owned()];
        let err = VirtualPolicy::compile(&p).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains("virtualPatterns") && text.contains("![unclosed"),
            "the message must name the list and quote the line as typed, got: {text}"
        );
        assert!(
            !text.contains("**/!"),
            "and must not quote the glob keeper derived, got: {text}"
        );
    }

    /// The owner's own stored row, verbatim, and the defect this story exists
    /// for (Story 56.16):
    ///
    /// ```json
    /// {"name":"tgdrive-light","lfsMode":"materialize","lfsThresholdBytes":262144,"virtualPatterns":[],"virtualOverBytes":1048576}
    /// ```
    ///
    /// He named the folder for what he wanted from it and set a 1 MiB floor,
    /// which can only mean "don't fetch the big files". Without the fix
    /// `resolve` demanded `self.patterns.matches(rela)` before it would answer
    /// `Virtual`, and with no permissive line in any source the compiled set is
    /// empty and matches nothing — so the 4 MiB assertion below read
    /// `Materialize`, `tier()` read `Unset`, `authorizes_anything()` was false,
    /// and all 16 GB of the folder downloaded. A control that silently did
    /// nothing, with a form note describing a match that never happens.
    #[test]
    fn the_owners_stored_configuration_keeps_his_large_files_away() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.lfs_mode = crate::profile::LfsMode::Materialize;
        p.lfs_threshold_bytes = 262_144;
        p.virtual_patterns = vec![];
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("40-media/holiday.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "a file above the floor is what the floor was set to keep away"
        );
        assert_eq!(
            policy.resolve(Path::new("10-notes/scan.png"), 64 * 1024),
            Virtualization::Materialize,
            "and one below it is still fetched: the floor is the whole selector"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/exact.mov"), 1024 * 1024),
            Virtualization::Virtual,
            "exactly at the floor stays away — the boundary is inclusive, as \
             `LfsPolicy`'s threshold is"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "the floor is the source that decided, and the surface must be able \
             to say so rather than report nothing configured"
        );
        assert!(
            policy.authorizes_anything(),
            "the engine's release gate skips a policy that authorizes nothing, \
             so a floor that selects has to answer yes here or it never runs"
        );
    }

    /// A floor that selects on its own is still only an authorization, so a
    /// protection committed to the repository beats it exactly as it beats a
    /// pattern (AD-123, Story 56.16).
    ///
    /// This is the assertion that keeps the new selector inside the existing
    /// safety rule rather than beside it. Written before the fix it passed
    /// vacuously — nothing was virtual at all — so it is only load-bearing
    /// once the floor selects: a fix that answered `Virtual` from the floor
    /// *before* consulting `never` would have dehydrated the one zone the
    /// repository explicitly named.
    #[test]
    fn a_committed_protection_still_wins_over_a_floor_that_selects_on_its_own() {
        let dir = worktree(Some("!30-masters\n"));
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("30-masters/a.mov"), 4 * 1024 * 1024),
            Virtualization::Materialize,
            "the committed protection wins over the floor, unconditionally"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "and it protects only what it names: the floor still selects the rest"
        );
    }

    /// The same, from the machine's own list — and the tier is the interesting
    /// half (Story 56.16 over 56.14).
    ///
    /// A `!`-only list does not override anything (Story 56.14: the override is
    /// decided on the permissive half alone), so the effective permissive set
    /// is empty and the floor is what selects. `tier()` must therefore say
    /// `SizeFloor` and not `Profile`: `Profile` would claim `virtualPatterns`
    /// decided what may stay away, which is the empty list, and a regression
    /// that stopped the floor selecting could then still pass a tier assertion.
    /// Without the fix this test failed on the `40-media` line — `Materialize`,
    /// nothing selected anything — and on the tier, which read `Profile`.
    #[test]
    fn a_profile_protection_still_wins_over_a_floor_that_selects_on_its_own() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["!30-masters".into()];
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("30-masters/a.mov"), 4 * 1024 * 1024),
            Virtualization::Materialize,
            "the machine's own protection wins over the floor too"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "and the floor selects everything it did not name"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "a `!`-only list overrides nothing, so the permissive set is empty \
             and the floor is the source that decided"
        );
    }

    /// The floor never widens a zone some source already named (Story 56.16).
    ///
    /// `floor_selects` is computed from the *effective* permissive set, after
    /// precedence has run, so a folder that names files keeps naming exactly
    /// those and the floor keeps its old job of holding the small ones back.
    /// The `50-iso` line is what a floor computed before precedence — or one
    /// that ignored the pattern set entirely — would have failed: it would
    /// have read `Virtual` for a zone the committed file never mentions, which
    /// is a folder silently dehydrating on upgrade.
    #[test]
    fn a_floor_never_widens_a_zone_a_pattern_file_already_named() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("50-iso/big.iso"), 4 * 1024 * 1024),
            Virtualization::Materialize,
            "a zone no source named must not gain a selector from the floor"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "the named zone is unchanged"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/small.mov"), 64 * 1024),
            Virtualization::Materialize,
            "and inside it the floor still does its old job of holding the \
             small ones back"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::PatternFile,
            "the file supplied the list in force, so that is still the file to edit"
        );
    }

    /// A floor of `0` with no patterns says nothing, and has to keep saying
    /// nothing (Story 56.16).
    ///
    /// This is the other silent reading, and the one the fix must not create.
    /// `0` is the default every profile ever written carries, and it is the
    /// documented spelling of "no floor" — so if the floor is now what selects,
    /// zero must select nothing at all. A `floor_selects` computed as
    /// `patterns.is_empty()` alone would have made every unconfigured folder in
    /// existence answer `Virtual` for every LFS path on the next sync, which is
    /// the whole 16 GB failure inverted and running on everybody's machine.
    #[test]
    fn a_floor_of_zero_with_no_patterns_still_says_nothing() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 0;
        assert!(
            p.virtual_patterns.is_empty(),
            "the default is an empty list"
        );
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::Unset,
            "no source said anything, and a zero floor is not a source saying \
             something"
        );
        assert!(
            !policy.authorizes_anything(),
            "so there is no point consulting this policy at all"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 10_000_000),
            Virtualization::Materialize,
            "and nothing of any size may stay away"
        );
    }

    /// The sharpest hazard the new selector opens (Story 56.16).
    ///
    /// A floor-only policy authorizes by size and by nothing else, so it names
    /// no path and excludes none either — and a repository whose own
    /// `.gitattributes` had grown past the floor would have had its LFS routing
    /// rules turned into pointer text, `.keepervirtual` would have become a
    /// policy that erased itself, and the next `compile` would read
    /// `version https://git-lfs.github.com/spec/v1` as a pattern. The control
    /// carve-out already sits ahead of every other term in `resolve` and this
    /// pins it there against a fix that answered from the floor first.
    #[test]
    fn a_control_file_is_never_virtual_under_a_floor_that_selects_on_its_own() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "the fixture really is a floor-only policy"
        );

        for control in [VIRTUAL_PATTERN_FILE, ".gitattributes", ".lfsconfig"] {
            assert_eq!(
                policy.resolve(Path::new(control), 4 * 1024 * 1024),
                Virtualization::Materialize,
                "{control} carries the rules; a floor must not be able to make \
                 its own bytes optional"
            );
        }
    }

    /// The frame guard survives the new selector too (Story 56.16).
    ///
    /// `exclude::match_string` silently drops a root component, which is why an
    /// absolute path used to be matched as a relative one; under a floor-only
    /// policy there is no pattern to mismatch at all, so *nothing but* this
    /// guard stands between a path outside the repository and a `Virtual`
    /// answer. Anything the caller hands over that is not repository-relative
    /// has to be refused on the frame violation, not on the size.
    #[test]
    fn a_path_outside_the_repository_is_still_never_virtual_under_a_floor() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        for outside in ["/home/u/tgdrive/40-media/a.mov", "40-media/../../etc/a.mov"] {
            assert_eq!(
                policy.resolve(Path::new(outside), 4 * 1024 * 1024),
                Virtualization::Materialize,
                "{outside} is outside the frame the policy is written against, \
                 and a floor-only policy has no pattern to reject it for"
            );
        }
    }

    /// A path that names nothing is not a path a floor may select (Story
    /// 56.16).
    ///
    /// `""` and `"."` are relative and carry no `..`, so the frame guard used
    /// to admit them, and they resolved `Materialize` only by accident:
    /// `exclude::match_string` renders both to the empty string and
    /// `PatternSet::matches` refuses an empty candidate. `resolve`'s
    /// `floor_selects ||` short-circuits ahead of the pattern set, so under a
    /// floor-only policy both answered `Virtual` — the repository root itself
    /// classified as releasable. No byte is deleted today, because
    /// `release_target` refuses an empty subpath further down, but the
    /// classification is already wrong at the point it is made, and leaning on
    /// a downstream guard is exactly what this module refuses to do: the
    /// consumers that reach `resolve` do it with a ledger-supplied
    /// `Path::new(&row.path)`, whose contents this module does not get to
    /// choose.
    #[test]
    fn a_path_with_no_components_is_not_selected_by_a_floor() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "the fixture really is a floor-only policy, or the assertions below \
             would pass with nothing selecting anything at all"
        );

        for nothing in ["", "."] {
            assert_eq!(
                policy.resolve(Path::new(nothing), 4 * 1024 * 1024),
                Virtualization::Materialize,
                "{nothing:?} names no file in the repository, so there is \
                 nothing here for a floor to authorize"
            );
        }
    }
}

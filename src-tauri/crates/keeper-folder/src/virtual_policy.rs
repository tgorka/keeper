//! Moved from `keeper_sync::lfs::virtual_policy` (story 119.1).
//!
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

use crate::error::{FolderError, Result};
use crate::exclude::PatternSet;

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
pub const PROFILE_SOURCE: &str = "virtualPatterns";

/// The refusal for a policy file whose bytes are an LFS pointer, followed by
/// the file's path (Story 70.4).
///
/// A `const` because the sentence is user-visible and the test that pins the
/// refusal quotes it rather than retyping it.
pub const POINTER_TEXT_POLICY_PREFIX: &str =
    "the virtualization policy is an LFS pointer instead of its rules; restore the file before \
     anything here can be trusted:";

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
    /// Reaching this variant also changes what `keeper_sync::engine`'s `verify`
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
/// Fields are private, as `keeper_sync::lfs::stage::LfsPolicy`'s are: the compiled
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
    /// A malformed glob is a hard [`FolderError::Config`] naming its source and
    /// quoting the line as typed, never a silently dropped pattern (FR-329) — a
    /// typo that quietly does nothing is how a folder stays 200 GB and nobody
    /// learns why.
    pub fn compile_in(
        root: &Path,
        virtual_patterns: &[String],
        virtual_over_bytes: u64,
    ) -> Result<Self> {
        let path = root.join(VIRTUAL_PATTERN_FILE);
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
                return Err(FolderError::Config(format!(
                    "could not read {}: {err}",
                    path.display()
                )));
            }
        };
        // A file whose bytes are an LFS pointer is the same broken repository
        // wearing a plausible face (Story 70.4, F-VF-6): pointer text parses
        // as three legal globs, `tier()` reports `PatternFile`,
        // `authorizes_anything()` is true, and every `resolve` answers
        // `Materialize` — a policy that erased itself while claiming to be in
        // force. Same shape as the unreadable arm, because it is the same
        // answer: keeper cannot read the policy, and says so naming the file.
        // The candidate check first, so an empty file — which `Pointer::parse`
        // reads as the empty pointer by spec — stays the ordinary "no policy".
        if crate::pointer::is_pointer_candidate(file_text.as_bytes())
            && crate::pointer::Pointer::parse(file_text.as_bytes()).is_some()
        {
            return Err(FolderError::Config(format!(
                "{POINTER_TEXT_POLICY_PREFIX} {}",
                path.display()
            )));
        }
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
        let from_profile = Parsed::of(virtual_patterns.iter(), Comments::Literal);

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
        let floor_selects = patterns.is_empty() && virtual_over_bytes > 0;

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
            over_bytes: virtual_over_bytes,
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
    /// [`Self::compile_in`] time. That is what makes resolving 10 000 paths cost no
    /// worktree read and no network call.
    ///
    /// A protection wins unconditionally over a matching pattern — unlike
    /// gitignore's last-match-wins — because this decision is about somebody's
    /// bytes and the only safe direction to err in is keeping them. Same shape,
    /// and same reason, as `keeper_sync::lfs::stage::LfsPolicy::applies`.
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
    /// consulting me?" — `keeper_sync::engine`'s release gate is the one that must,
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

/// Refuse a `virtualPatterns` list [`VirtualPolicy::compile_in`] would refuse
/// later (Story 56.14).
///
/// Called from `keeper_sync::profile::SyncProfile::validate`, which runs on every
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
///
/// One predicate, shared with the routing doors (Story 70.4, AD-230). This
/// used to be the wider of two lists — the routing one lacked `.lfsconfig`,
/// `.keepervirtual` and the `.keeper/` component — so a file this module
/// refused to virtualize could still be *converted* into the very pointer it
/// feared. The union now lives beside the conversion code, and this is a
/// name for the same answer.
fn is_control_file(rela: &Path) -> bool {
    crate::control::is_control_file(rela)
}

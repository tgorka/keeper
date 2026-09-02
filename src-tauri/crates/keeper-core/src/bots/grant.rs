//! Grants — keeper's first user-granted, revocable capability (Story 61.10,
//! FR-386, FR-387, AD-158).
//!
//! A grant is `(provider, optional bot) × scope × mode`, where the scope is the
//! whole drive, one sync profile, or one subtree of one profile, and the mode is
//! `none | read | write`. It is the only reason a tool call is allowed to touch
//! a byte, and [`decide`] is the whole of that reason: pure, total, and the one
//! place the algebra lives (AD-55/AD-56). The shell resolves nothing — it calls
//! [`check`], which re-reads the store, and obeys the verdict.
//!
//! # The five rules this module exists to make unbreakable
//!
//! 1. **A subtree matches by segment, never by string prefix.** `notes-old` is
//!    not inside `notes`, and a `starts_with` would say it is. This is the
//!    canonical production defect in every filesystem-permission
//!    implementation surveyed (R6 §5.2), so it is [`covers_subpath`]'s only
//!    job.
//! 2. **The most specific grant wins.** A subtree beats a profile beats the
//!    drive; at equal scope a bot-scoped grant beats its provider-wide
//!    sibling. Claude Code deliberately ignores specificity and evaluates
//!    `deny → ask → allow` in source order (R6 §2.1), which is a defensible
//!    choice for a rule *list* a person wrote by hand — but a keeper grant is a
//!    row a person created by drawing a box around some files, so the narrow
//!    box is the later, more considered act and it must win.
//! 3. **`none` is an explicit refusal, and deny comes first.** Among the
//!    grants that tie on specificity, a `none` refuses — the `deny → ask →
//!    allow` order the industry converged on (R6 §2.1, §2.3), restated here.
//! 4. **A write is never auto-approved by a grant alone** (FR-387). A
//!    `write`-mode grant on the *drive* or on a *profile* yields [`Ask`] every
//!    time; only a `write`-mode **subtree** grant yields [`Allow`], because
//!    drawing that subtree *is* the act of approving writes inside it ("always
//!    for this subtree" is a durable grant edit, not a remembered click). This
//!    is also where every shipped CLI lands: Claude Code's file-modification
//!    approvals are the one class it refuses to persist beyond a session
//!    (R6 §2.1, §6.2).
//! 5. **Nothing but the UI writes a grant.** No tool result, no file content
//!    and no model message reaches [`super::store::save_grant`] (NFR-48). A
//!    file that says "grant yourself write access to /" is data, and the layer
//!    that stops it obeying itself is this one — not a sentence in a system
//!    prompt, which is the wrong layer (R6 §6.2, the vendor caveat:
//!    "permission rules are enforced by Claude Code, not by the model").
//!
//! # Why the copy is `const`
//!
//! Every refusal below is a whole sentence, stored once. The pane renders it,
//! the audit log records it, and a person comparing the two must not have to
//! wonder whether they are the same refusal worded twice — which is the
//! mistake `layout/panel-strip.tsx:20-25` already records for the absent
//! drive.
//!
//! [`Allow`]: GrantVerdict::Allow
//! [`Ask`]: GrantVerdict::Ask

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::store;
use super::ProviderKind;
use crate::error::CoreError;

/// The longest tool subpath keeper will reason about.
///
/// A bound rather than a taste: the subpath is split into segments on every
/// grant evaluation, and a pasted megabyte would be split on every tool call of
/// a conversation. The reference filesystem servers cap their inputs for the
/// same reason (R6 §9).
pub const MAX_SUBPATH_CHARS: usize = 1024;

// ---------------------------------------------------------------------------
// Refusal copy. One const per reason, quoted verbatim by the UI and the audit
// log. Sentence case, no exclamation, no "please" — UX-DR10, and the Do/Don't
// table at `EXPERIENCE.md:70-85`. Each one names what did not happen and the
// one act that would change it, the shape `session-actions.tsx:54-55` uses.
// ---------------------------------------------------------------------------

/// No grant covers the target at all.
pub const DENY_NO_GRANT: &str = "No grant covers this folder, so nothing was read or written. \
     Add a grant for it in Settings → Bots and this bot can try again.";

/// The narrowest grant covering the target is set to no access.
pub const DENY_MODE_NONE: &str =
    "A grant on this folder is set to no access, and the narrowest grant is the one that \
     counts. Change that grant in Settings → Bots to open it.";

/// The target is inside a read-only scope and the effect was a write.
pub const DENY_READ_ONLY: &str =
    "This grant allows reading and not writing, so the file is unchanged. Set the grant to \
     write in Settings → Bots to allow it.";

/// The grant that covered the target was revoked while the call was in flight.
pub const DENY_REVOKED: &str =
    "The grant covering this folder was revoked while the call was in flight, so nothing was \
     read or written.";

/// The subpath is not something keeper will resolve at all.
pub const DENY_SUBPATH_UNSAFE: &str =
    "That path is not inside a sync profile. A tool path is always relative to the profile \
     root, with no leading slash and no \"..\" in it.";

/// A write inside a `write`-mode drive- or profile-wide scope: allowed to
/// happen, never without being asked (FR-387).
pub const ASK_WRITE_WIDE_SCOPE: &str =
    "This bot may write here, and keeper asks before every write to a scope this wide. \
     Approve this one, or grant the folder itself to stop being asked.";

/// Why a Hermes bot's pane offers no grant (epic coordinator correction,
/// research §2.6, §2.13).
pub const HERMES_RUNS_ITS_OWN_TOOLS: &str =
    "Hermes runs its tools on its own host, under its own permissions, so a keeper grant \
     would change nothing here.";

/// Why an Ollama bot whose model states it cannot use tools offers no grant.
pub const MODEL_HAS_NO_TOOLS: &str =
    "This model does not support tool calls, so there is nothing a grant would let it reach.";

/// What a bot whose tool capability keeper could not read is warned about.
///
/// `unknown` is not `false` (AD-27), and here that has teeth: the affordance
/// is **offered** and carries this sentence, because refusing on unknown would
/// strand every Ollama old enough to predate the `capabilities` array with no
/// route to the feature at all. The asymmetry is in the user's favour — a
/// grant on a model that turns out to have no tools costs a tool call that is
/// never made, while every call that *is* made still goes through [`decide`]
/// and, for a write, the approval path.
pub const TOOLS_CAPABILITY_UNKNOWN: &str =
    "keeper could not read whether this model supports tool calls, so a grant here may reach \
     nothing. Test the provider again to find out.";

/// Why a turn offers no tools when the bot has no live grant at all.
///
/// Distinct from [`DENY_NO_GRANT`], which is about one call to one folder:
/// this is about the turn, and it is the ordinary state of a bot nobody has
/// granted anything — a fact, not a refusal.
pub const NO_GRANT_NO_TOOLS: &str =
    "This bot has no grant, so it was offered no tools over the drive. Add one in Settings → \
     Bots to let it read or write there.";

/// Why a turn offers no tools when every live grant is set to no access.
pub const GRANTS_ALL_NONE_NO_TOOLS: &str =
    "Every grant for this bot is set to no access, so it was offered no tools over the drive.";

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// What a grant permits (Story 61.10, FR-386).
///
/// Three states and not a pair of booleans, for `files_write.rs:62`'s reason —
/// "not a boolean, and not a comment: the mistake is not expressible". A
/// `read`/`write` boolean pair has a fourth state (`write` without `read`) that
/// means nothing, and [`GrantMode::None`] is a positive act — a refusal a
/// person typed — which "both booleans false" cannot distinguish from a row
/// nobody filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GrantMode {
    /// Explicitly nothing. Beats any wider allowance covering the same path.
    None,
    /// Read, list, glob, grep and stat. Never a write.
    Read,
    /// Read, and write **subject to** FR-387 — see [`decide`].
    Write,
}

impl GrantMode {
    /// The string stored in `bot_grants.mode` and sent over IPC.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            GrantMode::None => "none",
            GrantMode::Read => "read",
            GrantMode::Write => "write",
        }
    }

    /// Parse a stored `mode`, or `None` for a value this build does not know.
    ///
    /// `None` rather than a default, for [`ProviderKind::from_registry_str`]'s
    /// reason turned up one notch: guessing at a permission is the one guess
    /// this module may never make. A row it cannot read is listed as unreadable
    /// and grants nothing.
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(GrantMode::None),
            "read" => Some(GrantMode::Read),
            "write" => Some(GrantMode::Write),
            _ => None,
        }
    }

    /// How permissive this mode is, for the tie-break among equally specific
    /// grants. Only ever compared, never stored.
    fn strength(self) -> u8 {
        match self {
            GrantMode::None => 0,
            GrantMode::Read => 1,
            GrantMode::Write => 2,
        }
    }
}

/// Which bytes a grant is about (Story 61.10, FR-386).
///
/// A closed set of three, which is the vocabulary the owner asked for —
/// "drive, part of the drive" — and it maps onto what the industry ships: MCP's
/// `roots` and Codex's `writable_roots` are both "a list of directories" (R6
/// §1.5, §2.2), and [`GrantScope::Drive`] is the honest name for the case where
/// that list is every profile keeper holds.
///
/// **`subpath` is profile-relative and always was.** There is no variant that
/// can hold an absolute path, so a grant cannot name a location outside the
/// drive even by accident — the same reason `files_write.rs`'s `VaultPath` has
/// exactly one constructor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum GrantScope {
    /// Every sync profile keeper holds, including ones added after the grant.
    ///
    /// Deliberately future-inclusive: a person who says "the whole drive" has
    /// said something about the drive and not about today's list of profiles.
    /// The disclosure obligation that makes this honest is the Settings list,
    /// where this row reads as the whole drive and not as a profile count.
    Drive,
    /// One sync profile, whole.
    Profile {
        /// The profile this grant is about.
        profile_id: String,
    },
    /// One subtree of one sync profile, at or below `subpath`.
    Subtree {
        /// The profile the subtree lives in.
        profile_id: String,
        /// The subtree root, **profile-relative**, no leading slash, no `..`,
        /// no trailing slash — see [`parse_subpath`].
        subpath: String,
    },
}

impl GrantScope {
    /// The string stored in `bot_grants.scope_kind`.
    pub fn kind_registry_str(&self) -> &'static str {
        match self {
            GrantScope::Drive => "drive",
            GrantScope::Profile { .. } => "profile",
            GrantScope::Subtree { .. } => "subtree",
        }
    }

    /// The profile this scope names, or `None` for [`GrantScope::Drive`].
    pub fn profile_id(&self) -> Option<&str> {
        match self {
            GrantScope::Drive => None,
            GrantScope::Profile { profile_id } | GrantScope::Subtree { profile_id, .. } => {
                Some(profile_id)
            }
        }
    }

    /// The subtree root this scope names, or `None` for the two wider scopes.
    pub fn subpath(&self) -> Option<&str> {
        match self {
            GrantScope::Drive | GrantScope::Profile { .. } => None,
            GrantScope::Subtree { subpath, .. } => Some(subpath),
        }
    }

    /// Rebuild a scope from the three stored scalars, or `None` when they do
    /// not describe a scope this build understands (AD-139's read half).
    ///
    /// A `subtree` row with no `subpath`, or a `profile` row with no
    /// `profile_id`, is unreadable rather than silently widened: widening is
    /// exactly the direction a corrupt permission row must never fail in.
    pub fn from_registry(
        kind: &str,
        profile_id: Option<String>,
        subpath: Option<String>,
    ) -> Option<Self> {
        match kind {
            "drive" => Some(GrantScope::Drive),
            "profile" => Some(GrantScope::Profile {
                profile_id: profile_id?,
            }),
            "subtree" => Some(GrantScope::Subtree {
                profile_id: profile_id?,
                subpath: subpath?,
            }),
            _ => None,
        }
    }

    /// How specific this scope is. Larger is narrower, and a subtree's depth
    /// counts, so `notes/2026` outranks `notes`.
    ///
    /// `+ 2` keeps every subtree above every profile even at depth zero — a
    /// depth a valid subtree cannot have, since [`parse_subpath`] refuses the
    /// empty string, but the arithmetic should not depend on that.
    fn specificity(&self) -> u32 {
        match self {
            GrantScope::Drive => 0,
            GrantScope::Profile { .. } => 1,
            GrantScope::Subtree { subpath, .. } => 2 + segments(subpath).count() as u32,
        }
    }

    /// Whether this scope covers `target` — the scope algebra, in one place.
    fn covers(&self, target: &ToolTarget) -> bool {
        match self {
            GrantScope::Drive => true,
            GrantScope::Profile { profile_id } => *profile_id == target.profile_id,
            GrantScope::Subtree {
                profile_id,
                subpath,
            } => *profile_id == target.profile_id && covers_subpath(subpath, &target.subpath),
        }
    }

    /// A short label for the surface and the audit log, so both name the scope
    /// with the same words.
    pub fn label(&self) -> String {
        match self {
            GrantScope::Drive => "the whole drive".to_owned(),
            GrantScope::Profile { profile_id } => profile_id.clone(),
            GrantScope::Subtree {
                profile_id,
                subpath,
            } => format!("{profile_id}/{subpath}"),
        }
    }
}

/// One stored grant (Story 61.10, FR-386).
///
/// `bot_id` is `None` for a grant that covers every bot of the provider. Two
/// levels and not one because the owner's ask is multi-tenant: a work Hermes
/// and a home Ollama are two providers, and one Ollama can host a bot you trust
/// with your notes beside one you are trying out.
///
/// **No `revoked_ms` here.** A revoked grant grants nothing, so it is not a
/// `Grant`; the row keeps its `revoked_ms` so the audit log's `grant_id` still
/// resolves ([`store::GrantRow`]). Splitting them is what makes it impossible
/// to hand [`decide`] a revoked grant by forgetting a filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The grant's opaque id — a ULID, minted by the caller.
    pub id: String,
    /// Which provider this grant is about.
    pub provider_id: String,
    /// Which bot, or `None` for every bot of that provider.
    pub bot_id: Option<String>,
    /// Which bytes.
    pub scope: GrantScope,
    /// What is permitted.
    pub mode: GrantMode,
    /// When the grant was created, ms since the Unix epoch (UTC).
    pub created_ms: i64,
}

impl Grant {
    /// Whether this grant is one of the grants that speak for `(provider_id,
    /// bot_id)`.
    ///
    /// A provider-wide grant speaks for every bot of that provider; a
    /// bot-scoped grant speaks only for its own bot. The store narrows in SQL
    /// ([`store::list_grants_for_bot`]) and this is the same rule as a
    /// predicate, so the query and the algebra cannot drift apart.
    pub fn applies_to(&self, provider_id: &str, bot_id: Option<&str>) -> bool {
        if self.provider_id != provider_id {
            return false;
        }
        match (&self.bot_id, bot_id) {
            (None, _) => true,
            (Some(mine), Some(theirs)) => mine == theirs,
            (Some(_), None) => false,
        }
    }

    /// How specific this grant is: the scope first, then whether it names a
    /// single bot.
    ///
    /// Scope is primary because the scope is the box the person drew around
    /// some files, and the bot is who may open it — narrowing the files is the
    /// stronger statement about what may be touched.
    fn specificity(&self) -> (u32, u32) {
        (self.scope.specificity(), u32::from(self.bot_id.is_some()))
    }
}

/// What a tool call wants to touch (Story 61.10).
///
/// `subpath` is **always** profile-relative. The empty string is the profile
/// root and is reachable only through [`ToolTarget::profile_root`]; every path
/// that came from a model or a person goes through [`ToolTarget::parse`], which
/// refuses the empty string along with `.`, `..`, a leading slash and a
/// backslash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTarget {
    /// The sync profile the call is about.
    pub profile_id: String,
    /// The path inside that profile, profile-relative and normalized.
    pub subpath: String,
}

impl ToolTarget {
    /// The sanctioned constructor: a profile and a path that passed the
    /// grammar (Story 61.10).
    pub fn parse(profile_id: &str, subpath: &str) -> Result<Self, SubpathError> {
        Ok(ToolTarget {
            profile_id: profile_id.to_owned(),
            subpath: parse_subpath(subpath)?,
        })
    }

    /// The root of a profile — the one target with an empty subpath, and the
    /// only way to build one.
    ///
    /// Separate from [`ToolTarget::parse`] on purpose: a model or a person who
    /// typed an empty path made a mistake and gets a sentence, while "list the
    /// top of this profile" is a real call the tool layer makes deliberately.
    pub fn profile_root(profile_id: &str) -> Self {
        ToolTarget {
            profile_id: profile_id.to_owned(),
            subpath: String::new(),
        }
    }

    /// The path as a human reads it, `profile/sub/path` — what the audit log
    /// stores, because its reader is a person and not a resolver (FR-388).
    pub fn display_path(&self) -> String {
        if self.subpath.is_empty() {
            self.profile_id.clone()
        } else {
            format!("{}/{}", self.profile_id, self.subpath)
        }
    }
}

/// What a tool call would do to the bytes (Story 61.10).
///
/// Two effects and not seven tools: the grant algebra is about reading versus
/// changing, and mapping each tool onto one of these is [`super::tools`]'s job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Effect {
    /// Reads, lists, globs, greps, stats — anything that leaves the bytes as
    /// they were.
    Read,
    /// Creates, changes, moves or removes bytes.
    Write,
}

impl Effect {
    /// The string stored in `bot_audit.effect` and sent over IPC.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
        }
    }

    /// Parse a stored `effect`, or `None` for a value this build does not know.
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Effect::Read),
            "write" => Some(Effect::Write),
            _ => None,
        }
    }
}

/// What [`decide`] concluded (Story 61.10, FR-386, FR-387).
///
/// Three arms because the middle one is the story: `deny → ask → allow` is the
/// order every surveyed client evaluates in (R6 §2.1, §2.3), and a design with
/// only allow and deny has to express "ask" as a denial the caller is expected
/// to retry, which is how an approval prompt turns into a silent failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantVerdict {
    /// Go ahead, on the authority of this grant.
    Allow {
        /// The grant that permitted it — recorded in the audit row so the log
        /// answers "under what authority" and not merely "what happened".
        grant_id: String,
    },
    /// A person must approve this one call.
    Ask {
        /// The grant that would permit it once approved.
        grant_id: String,
        /// The sentence to show, verbatim — one of this module's consts.
        reason: &'static str,
    },
    /// No.
    Deny {
        /// The sentence to show, verbatim — one of this module's consts.
        ///
        /// `String` rather than `&'static str` because [`check`] composes a
        /// verdict from the store and a later story may need to name a
        /// profile; every value this module produces today is one of the
        /// consts.
        reason: String,
    },
}

/// Whether the grant affordance exists for a bot at all (epic coordinator
/// correction, 2026-09-02).
///
/// keeper offers a grant only where keeper is the thing that would execute the
/// tool. Hermes executes its own tools on its own host —
/// `/v1/capabilities` declares `tool_execution: "server"`, no route registers a
/// client tool, and an api-server session's `unattended_mode` defaults to
/// `deny` (research §2.6, §2.13) — so a grant there would be an affordance that
/// lies, which AD-27 forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantOffer {
    /// Show the grant affordance.
    Offered {
        /// A sentence to show beside it, or `None` where the endpoint stated
        /// the capability outright. Set where keeper could not read it —
        /// offered, and honest about not knowing.
        warning: Option<&'static str>,
    },
    /// Do not show it, and say this instead — one sentence, verbatim.
    Absent {
        /// The sentence to show — one of this module's consts.
        reason: &'static str,
    },
}

/// Decide whether a bot's pane offers a grant, and what to say either way.
///
/// `tools_supported` is the model's own answer: `Some(true)` where the endpoint
/// said so, `Some(false)` where it said the opposite, `None` where keeper could
/// not read it. Only `Some(false)` hides the affordance — a stated refusal is
/// the one case where a grant provably reaches nothing. `None` offers it with
/// [`TOOLS_CAPABILITY_UNKNOWN`], because an unreadable capability is not a
/// refusal (AD-27) and treating it as one would leave an older endpoint no
/// route to the feature.
pub fn grant_offer(kind: ProviderKind, tools_supported: Option<bool>) -> GrantOffer {
    match (kind, tools_supported) {
        (ProviderKind::Hermes, _) => GrantOffer::Absent {
            reason: HERMES_RUNS_ITS_OWN_TOOLS,
        },
        (ProviderKind::Ollama, Some(true)) => GrantOffer::Offered { warning: None },
        (ProviderKind::Ollama, Some(false)) => GrantOffer::Absent {
            reason: MODEL_HAS_NO_TOOLS,
        },
        (ProviderKind::Ollama, None) => GrantOffer::Offered {
            warning: Some(TOOLS_CAPABILITY_UNKNOWN),
        },
    }
}

// ---------------------------------------------------------------------------
// The subpath grammar
// ---------------------------------------------------------------------------

/// Why a subpath was refused (Story 61.10).
///
/// One variant per refusable shape, each naming the shape — the surface prints
/// [`DENY_SUBPATH_UNSAFE`] for all of them, but a caller and a test can tell
/// them apart, and `Display` here is what a log line carries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubpathError {
    /// Nothing was typed. The profile root is [`ToolTarget::profile_root`],
    /// never an empty string a caller happened to pass.
    #[error("a tool path cannot be empty")]
    Empty,
    /// A leading `/`: an absolute path, which a profile-relative field cannot
    /// hold.
    #[error("a tool path is relative to the profile root and cannot start with a slash")]
    Absolute,
    /// A `.` or `..` segment, or a `//` that produced an empty one.
    #[error("a tool path cannot contain \".\", \"..\" or an empty segment")]
    Traversal,
    /// A backslash. Refused everywhere, including on Windows, because a path
    /// that means one thing on one platform and another elsewhere is a path no
    /// grant can be checked against.
    #[error("a tool path uses forward slashes and cannot contain a backslash")]
    Backslash,
    /// A control character, including NUL — the shape that truncates a path
    /// inside a C API after keeper has finished checking it (R6 §5.2).
    #[error("a tool path cannot contain control characters")]
    Control,
    /// Longer than [`MAX_SUBPATH_CHARS`].
    #[error("a tool path cannot be longer than {MAX_SUBPATH_CHARS} characters")]
    TooLong,
}

/// Decide whether `input` can be a profile-relative tool path, and return its
/// normalized form (Story 61.10).
///
/// Normalization is one thing only: a trailing slash is dropped, so `notes/`
/// and `notes` are one subtree and one grant row. Nothing else is rewritten —
/// this function refuses what it does not like rather than repairing it,
/// because a permission check over a path the user did not type is a check
/// against the wrong path, and "sanitize then use" is precisely the pattern R6
/// §5.2 records failing in production.
///
/// **This is not the containment rule.** `keeper-sync`'s `browse::resolve` /
/// `plain_segments` / `WriteScope::route` remain the only functions that turn a
/// path into a location on disk, symlinks included (AD-65). This grammar is
/// what makes the *grant* checkable at all: a `..` reaching [`decide`] would
/// let a target lie about which subtree it is in.
pub fn parse_subpath(input: &str) -> Result<String, SubpathError> {
    if input.is_empty() {
        return Err(SubpathError::Empty);
    }
    if input.chars().count() > MAX_SUBPATH_CHARS {
        return Err(SubpathError::TooLong);
    }
    if input.starts_with('/') {
        return Err(SubpathError::Absolute);
    }
    if input.contains('\\') {
        return Err(SubpathError::Backslash);
    }
    if input.chars().any(char::is_control) {
        return Err(SubpathError::Control);
    }
    let trimmed = input.strip_suffix('/').unwrap_or(input);
    if trimmed.is_empty() {
        return Err(SubpathError::Empty);
    }
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(SubpathError::Traversal);
        }
    }
    Ok(trimmed.to_owned())
}

/// Whether a subpath is safe to *reason about* — the grammar minus the
/// non-empty requirement, because the profile root is a legitimate target and
/// is spelled `""`.
///
/// [`decide`] calls this on every evaluation. That is deliberate belt and
/// braces: the grammar is the door, and this is the check that a target which
/// somehow got built without going through the door is refused rather than
/// matched against a subtree it could be lying about.
fn subpath_is_safe(subpath: &str) -> bool {
    subpath.is_empty() || parse_subpath(subpath).is_ok_and(|normalized| normalized == subpath)
}

/// The segments of a profile-relative subpath. Empty for the profile root.
fn segments(subpath: &str) -> impl Iterator<Item = &str> {
    subpath.split('/').filter(|s| !s.is_empty())
}

/// Whether `target` is at or below `scope`, **by segment** (Story 61.10).
///
/// The one function this module would be worthless without. `notes` covers
/// `notes` and `notes/2026/a.md`; it does not cover `notes-old` or
/// `notesomething`, and a `target.starts_with(scope)` would say it does. That
/// string-prefix confusion is the canonical filesystem-permission defect
/// (R6 §5.2), so the comparison is segment by segment and never over the joined
/// string.
fn covers_subpath(scope: &str, target: &str) -> bool {
    let mut target_segments = segments(target);
    for scope_segment in segments(scope) {
        match target_segments.next() {
            Some(target_segment) if target_segment == scope_segment => {}
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// The decision
// ---------------------------------------------------------------------------

/// Decide what `grants` permit for `target` under `effect` (Story 61.10,
/// FR-386, FR-387).
///
/// Pure, total, and the whole of keeper's permission model. `grants` must
/// already be the *live* grants that speak for this `(provider, bot)` —
/// [`check`] is the caller that guarantees both, and [`Grant::applies_to`] is
/// the predicate it uses.
///
/// # The truth table, exactly as implemented
///
/// 1. `target.subpath` is re-validated. Unsafe ⇒ `Deny(DENY_SUBPATH_UNSAFE)`.
/// 2. Keep the grants whose scope covers the target. None ⇒
///    `Deny(DENY_NO_GRANT)`.
/// 3. Keep those of maximal specificity — scope depth first
///    (subtree > profile > drive, deeper subtree first), then bot-scoped over
///    provider-wide. **The most specific grant wins**, which is what makes a
///    narrow row a later, stronger statement than a wide one.
/// 4. Among those, if any is [`GrantMode::None`] ⇒ `Deny(DENY_MODE_NONE)`.
///    Deny comes first, so an explicit refusal beats a tie.
/// 5. Otherwise take the most permissive of the tie (`write` over `read`), and:
///
/// | effect | winning mode | winning scope | verdict |
/// | --- | --- | --- | --- |
/// | `Read` | `read` or `write` | any | `Allow` |
/// | `Write` | `read` | any | `Deny(DENY_READ_ONLY)` |
/// | `Write` | `write` | `Subtree` | `Allow` |
/// | `Write` | `write` | `Profile` or `Drive` | `Ask(ASK_WRITE_WIDE_SCOPE)` |
///
/// The last two rows are FR-387. `Allow` for a write is produced **only** by a
/// grant whose own scope is the subtree the write lands in — the durable "always
/// for this subtree" the epic describes. A `write` grant on a whole profile or
/// on the drive is a standing permission to *ask*, which is where every shipped
/// client lands too (R6 §2.1: file-modification approvals are the one class
/// Claude Code refuses to persist past a session).
pub fn decide(grants: &[Grant], target: &ToolTarget, effect: Effect) -> GrantVerdict {
    if !subpath_is_safe(&target.subpath) {
        return GrantVerdict::Deny {
            reason: DENY_SUBPATH_UNSAFE.to_owned(),
        };
    }

    let Some(best) = grants
        .iter()
        .filter(|grant| grant.scope.covers(target))
        .map(Grant::specificity)
        .max()
    else {
        return GrantVerdict::Deny {
            reason: DENY_NO_GRANT.to_owned(),
        };
    };

    let winners = grants
        .iter()
        .filter(|grant| grant.scope.covers(target) && grant.specificity() == best);

    // Deny first: an explicit refusal at the winning specificity settles it,
    // whatever else ties with it.
    let mut strongest: Option<&Grant> = None;
    for grant in winners {
        if grant.mode == GrantMode::None {
            return GrantVerdict::Deny {
                reason: DENY_MODE_NONE.to_owned(),
            };
        }
        // Ties on mode are broken by the smaller id, so two equally specific
        // grants of the same mode always name the same authority in the audit
        // log rather than whichever row SQLite happened to return first.
        let better = match strongest {
            None => true,
            Some(current) => match grant.mode.strength().cmp(&current.mode.strength()) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Equal => grant.id < current.id,
                std::cmp::Ordering::Less => false,
            },
        };
        if better {
            strongest = Some(grant);
        }
    }

    let Some(winner) = strongest else {
        // Unreachable: `best` came from a non-empty filtered iterator, so the
        // same filter yields at least one grant. Denying rather than
        // unwrapping, because the safe direction for a permission check that
        // has lost its own invariant is "no".
        return GrantVerdict::Deny {
            reason: DENY_NO_GRANT.to_owned(),
        };
    };

    match (effect, winner.mode) {
        (Effect::Read, _) => GrantVerdict::Allow {
            grant_id: winner.id.clone(),
        },
        (Effect::Write, GrantMode::Read) => GrantVerdict::Deny {
            reason: DENY_READ_ONLY.to_owned(),
        },
        (Effect::Write, GrantMode::Write) => match winner.scope {
            GrantScope::Subtree { .. } => GrantVerdict::Allow {
                grant_id: winner.id.clone(),
            },
            GrantScope::Drive | GrantScope::Profile { .. } => GrantVerdict::Ask {
                grant_id: winner.id.clone(),
                reason: ASK_WRITE_WIDE_SCOPE,
            },
        },
        // `None` returned above, before the winner was chosen.
        (Effect::Write, GrantMode::None) => GrantVerdict::Deny {
            reason: DENY_MODE_NONE.to_owned(),
        },
    }
}

/// The per-call door: re-read the store and decide (Story 61.10, FR-386,
/// AD-158).
///
/// **This function re-reads `keeper.db` on every call, and that is the whole
/// point.** A grant set cached in a struct that outlives one tool call is an
/// unrevocable grant: the pane would keep writing under a permission the user
/// removed two calls ago, and "revoke" would be a button that lies. The cost is
/// one indexed `SELECT` over a table with a handful of rows, against a WAL
/// database the same conversation is already writing audit rows to.
///
/// A target that no live grant covers, but a **revoked** grant would have,
/// reports [`DENY_REVOKED`] instead of [`DENY_NO_GRANT`] — because those are
/// different facts, and the one a person needs to hear is that the permission
/// they removed is the reason this stopped working.
pub fn check(
    data_dir: &std::path::Path,
    provider_id: &str,
    bot_id: Option<&str>,
    target: &ToolTarget,
    effect: Effect,
) -> Result<GrantVerdict, CoreError> {
    let listing = store::list_grants_for_bot(data_dir, provider_id, bot_id)?;
    let verdict = decide(&listing.live, target, effect);
    if let GrantVerdict::Deny { reason } = &verdict {
        if reason == DENY_NO_GRANT
            && listing
                .revoked
                .iter()
                .any(|grant| grant.scope.covers(target))
        {
            return Ok(GrantVerdict::Deny {
                reason: DENY_REVOKED.to_owned(),
            });
        }
    }
    Ok(verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_round_trips_through_its_stored_string() {
        for mode in [GrantMode::None, GrantMode::Read, GrantMode::Write] {
            assert_eq!(
                GrantMode::from_registry_str(mode.as_registry_str()),
                Some(mode)
            );
        }
        assert_eq!(
            GrantMode::from_registry_str("everything"),
            None,
            "a mode this build cannot read grants nothing, it does not default"
        );
    }

    #[test]
    fn an_effect_round_trips_through_its_stored_string() {
        for effect in [Effect::Read, Effect::Write] {
            assert_eq!(
                Effect::from_registry_str(effect.as_registry_str()),
                Some(effect)
            );
        }
        assert_eq!(Effect::from_registry_str("delete"), None);
    }

    #[test]
    fn a_scope_round_trips_through_its_three_stored_scalars() {
        let scopes = [
            GrantScope::Drive,
            GrantScope::Profile {
                profile_id: "work".to_owned(),
            },
            GrantScope::Subtree {
                profile_id: "work".to_owned(),
                subpath: "notes/2026".to_owned(),
            },
        ];
        for scope in scopes {
            let rebuilt = GrantScope::from_registry(
                scope.kind_registry_str(),
                scope.profile_id().map(str::to_owned),
                scope.subpath().map(str::to_owned),
            );
            assert_eq!(rebuilt, Some(scope));
        }
    }

    #[test]
    fn a_subtree_row_missing_its_subpath_is_unreadable_and_never_widened() {
        assert_eq!(
            GrantScope::from_registry("subtree", Some("work".to_owned()), None),
            None
        );
        assert_eq!(GrantScope::from_registry("profile", None, None), None);
        assert_eq!(GrantScope::from_registry("everything", None, None), None);
    }

    #[test]
    fn the_grant_offer_is_absent_for_hermes_and_says_why() {
        assert_eq!(
            grant_offer(ProviderKind::Hermes, Some(true)),
            GrantOffer::Absent {
                reason: HERMES_RUNS_ITS_OWN_TOOLS
            }
        );
        assert_eq!(
            grant_offer(ProviderKind::Ollama, Some(true)),
            GrantOffer::Offered { warning: None }
        );
        assert_eq!(
            grant_offer(ProviderKind::Ollama, Some(false)),
            GrantOffer::Absent {
                reason: MODEL_HAS_NO_TOOLS
            }
        );
    }

    /// A capability keeper could not read is `unknown`, never `false`: the
    /// affordance is offered and the sentence says keeper could not tell.
    /// Refusing here would strand every endpoint too old to state its
    /// capabilities.
    #[test]
    fn an_unreadable_tool_capability_offers_the_grant_with_a_warning() {
        assert_eq!(
            grant_offer(ProviderKind::Ollama, None),
            GrantOffer::Offered {
                warning: Some(TOOLS_CAPABILITY_UNKNOWN)
            }
        );
    }
}

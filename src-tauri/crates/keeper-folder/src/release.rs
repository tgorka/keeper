//! The release rules (story 119.1): which clock applies to a `materialized`
//! ledger row, the two budgets one release pass may spend, and the ledger's
//! columns.
//!
//! Moved from `keeper_sync::engine` and `keeper_sync::db` without a change of
//! behaviour. Each crate keeps its own SQLite storage of the ledger; only the
//! column set and the rules over it live here. The order of the refusals is
//! documented on `keeper_sync::engine::Engine::release_resolved`, whose steps
//! read git's index and the LFS store; `keeper-files` ports that order (119.1).

/// Paths one release sweep may attempt (Story 56.5, AD-126).
///
/// Bounded for `keeper_sync::engine::CLAIM_LIMIT`'s reason: the sweep runs inside the reservation
/// the sync tick already holds, and a pass that walked forty thousand eligible
/// paths would hold it for minutes and starve the folder's watcher. Whatever
/// this pass does not reach, the next successful sync does — which is true
/// because of `Engine::release_cursor` and was false without it: this bounds
/// ATTEMPTS, and a prefix of candidates that refuses identically on every pass
/// would otherwise hide the tail of the ledger forever.
///
/// `pub` for one reason: `tests/release_sweep.rs` builds its fixture from this
/// number rather than from a literal, so raising the budget does not silently
/// turn "more candidates than one pass allows" into "fewer".
pub const RELEASE_BUDGET_OBJECTS: usize = 32;

/// Bytes one release sweep may put through the content-identity proof.
///
/// The second dimension, and it is not decoration: 56.4's proof **reads every
/// byte** of every candidate before deleting it, so a count ceiling alone
/// permits 32 four-gigabyte files — 128 GB of hashing in a pass that is
/// supposed to be housekeeping. A gigabyte is a few seconds of disk and bounds
/// the actual cost rather than the item count.
///
/// The total **stops the pass** at the first candidate that would cross it,
/// rather than skipping that candidate and carrying on. Skipping would make an
/// object larger than the whole budget permanently unreleasable; stopping means
/// it is simply first in line next time and still gets its one attempt.
///
/// `pub` for [`RELEASE_BUDGET_OBJECTS`]' reason: this ceiling is a promise about
/// what one pass costs, and a suite deriving its object sizes from a literal
/// would stop exercising it the moment the number moved.
pub const RELEASE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// One row of the `materialized` ledger, late columns included (Stories 56.2,
/// 56.5 and 56.17).
///
/// A struct rather than a tuple because six of the nine fields are
/// `Option`s of two types and five of them are timestamps: a transposition
/// between `last_used_ms` and `synced_at_ms` would compile, pass every type
/// check, and make a release decision on the wrong fact.
///
/// **Every late timestamp is an `Option` and that is a fact, not a
/// placeholder.** A row written before Story 56.2 — or by
/// `keeper_sync::db::remember_materialized`, which sets `at_ms` and nothing else — reads back
/// `None` for all of them, and `None` means "nobody recorded this" rather than
/// zero, epoch, or absent-from-the-remote. Story 56.5's writers
/// (`note_use`, `note_arrival`, `note_local_authorship`, `note_synced`,
/// `set_pinned`) fill them in, each one fact at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedRow {
    /// Repository-relative, `/`-joined — the same frame every other path in
    /// this database is in.
    pub path: String,
    /// When content for this path last landed here. The one column with a
    /// writer.
    pub at_ms: i64,
    /// When the content was last read through keeper.
    pub last_used_ms: Option<i64>,
    /// When the remote last confirmed it holds the object.
    pub synced_at_ms: Option<i64>,
    /// The object this path's content is, when the row recorded one.
    pub oid: Option<String>,
    /// How large that object is, when the row recorded it. Narrowed from
    /// SQLite's signed integer the same way `list_activity` narrows a size: a
    /// negative byte count is not a byte count, and it means exactly what
    /// `NULL` means.
    pub size_bytes: Option<u64>,
    /// Whether the owner has asked for this path to stay on this machine.
    /// `false` for every row that has never said otherwise, including every
    /// row written before the column existed — an unpinned path is the
    /// default, so absence and `0` are the same answer and an `Option` here
    /// would be a distinction nobody can act on.
    pub pinned: bool,
    /// Which way the content at this path travelled (Story 56.5, AD-131).
    /// `true` means this clone created or modified it; `false` means it
    /// arrived from the remote. `false` for every row that has never said
    /// otherwise, including every row written before the column existed —
    /// which is honest rather than a default, because the only writers this
    /// table had before Story 56.5 were arrival paths. It selects which
    /// release clock applies: `synced_at_ms` for local content, so content
    /// this machine authored and has not yet pushed is never eligible, and
    /// `last_used_ms` for content that came from the remote.
    pub local_origin: bool,
    /// The absolute epoch-ms instant the owner asked for this one path's
    /// content to be let go again (Story 56.17), or `None` for a path on the
    /// folder's own `releaseTtlMs` window — which is every path nobody has
    /// named a time for, and every row written before the column existed.
    ///
    /// [`release_due_at`] reads it **instead of** the folder's
    /// window, in both directions: an hour chosen inside a day-long window
    /// goes sooner, and two days chosen inside it stay longer. It does not
    /// outrank [`Self::pinned`], and it does not reach past FR-341 — a path
    /// this clone authored that nothing has confirmed the remote holds is
    /// still on no clock at any age, because a chosen duration must not become
    /// a way around the barrier that stops keeper deleting bytes which exist
    /// on one machine.
    pub release_at_ms: Option<i64>,
}

/// The instant this row becomes releasable, or `None` for *never* (Story 56.5,
/// FR-341, AD-131; Story 56.17).
///
/// A free function with no `self`, no I/O and no clock, because this is where
/// the whole feature's safety lives: the sweep reads rows, asks this, and
/// compares the answer to now. That makes both guards one line each,
/// unit-testable over hand-built rows, and — the property AC 2 asks for — it
/// makes the sweep's *candidate set* a value a test can assert on. "Was not
/// released" would also be true of a path that was considered and refused;
/// "was not a candidate" is the stronger claim, and only a pure predicate can
/// carry it.
///
/// **The pin is checked first, before any clock.** It is an absolute floor, not
/// a term in an expression, and putting it above the provenance branch means no
/// future clock can be added below it that a pinned row could reach.
///
/// **`row.synced_at_ms?` is FR-341.** For content this clone authored, a `NULL`
/// there means the remote has never been observed holding these bytes, so they
/// exist on exactly one machine and are not eligible **at any age** — the `?` is
/// the whole rule, and no `unwrap_or` may ever appear on that line. Content that
/// arrived from the remote measures from its last use instead, falling back to
/// `at_ms`: that column is `NOT NULL` and is a real instant the content was
/// here, so a folder whose rows predate this story is not inert.
///
/// **`row.release_at_ms` replaces the folder's window, in both directions**
/// (Story 56.17). It is the absolute instant somebody named for this one path
/// — `keeper-syncd materialize --for 2h`, or the Files row's own choice — and
/// a chosen hour inside a day-long window goes twenty-three hours sooner while
/// a chosen week stays six days longer. A replacement and never a `min` or a
/// `max`: the person said how long they want the file, and a folder-wide knob
/// they may not even know about must not quietly overrule them either way.
///
/// **It is read AFTER the provenance branch, and that ordering is the whole
/// safety of this story.** The `?` above has already run by the time a chosen
/// deadline is looked at, so a locally authored path the remote has never been
/// observed holding is still on no clock at any age — asking for such a file
/// "for an hour" records the wish and changes nothing about its eligibility.
/// A deadline read first would turn a duration into a way past FR-341, which
/// is the one barrier standing between the sweep and bytes that exist on
/// exactly one machine. Reading it below the pin's early return says the same
/// thing about the other floor.
///
/// `pub` rather than `pub(crate)` because AC 2 asks for the candidate set to be
/// asserted *as a set*, over the rows a real repository and a real sync
/// actually produced — and `tests/release_sweep.rs` cannot do that without
/// asking the predicate. Exposing a pure function with no `self`, no I/O and no
/// clock costs nothing: there is no state a caller could corrupt with it.
pub fn release_due_at(row: &MaterializedRow, ttl_ms: u64) -> Option<i64> {
    if row.pinned {
        return None;
    }
    let since = if row.local_origin {
        row.synced_at_ms?
    } else {
        row.last_used_ms.unwrap_or(row.at_ms)
    };
    Some(
        row.release_at_ms
            .unwrap_or_else(|| since.saturating_add(ttl_ms as i64)),
    )
}

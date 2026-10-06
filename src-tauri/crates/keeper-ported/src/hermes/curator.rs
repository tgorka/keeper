//! Hermes' skill curator, its deterministic half (`agent/curator.py`'s
//! `apply_automatic_transitions`), ported: a curator-managed skill unused
//! for 14 days is `stale`, for 30 `archived`, and a stale one used again is
//! `active`; a pinned or a protected skill is never touched, and nothing is
//! ever deleted — archiving is the caller's move.
//!
//! Modified from `agent/curator.py` of NousResearch/hermes-agent (MIT,
//! Copyright (c) 2025 Nous Research). Upstream reads its rows from the
//! usage ledger and writes the new states back; here the caller hands the
//! rows over and applies what comes back. Times are whole seconds since the
//! epoch. The first-sight seeding and the bundled-skill re-anchoring are
//! not ported: keeper's caller always knows a skill's anchor (`UPSTREAM.md`).

use std::collections::BTreeSet;

/// Days without activity before a skill is stale.
pub const DEFAULT_STALE_AFTER_DAYS: i64 = 14;
/// Days without activity before a skill is archived.
pub const DEFAULT_ARCHIVE_AFTER_DAYS: i64 = 30;

const DAY_SECS: i64 = 24 * 60 * 60;

/// A curator-managed skill's lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Stale,
    Archived,
}

/// One curator-managed skill, as `curated_report` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub state: State,
    pub pinned: bool,
    /// Its latest real activity, seconds since the epoch.
    pub last_activity_at: Option<i64>,
    /// When it was made, seconds since the epoch.
    pub created_at: Option<i64>,
    pub use_count: u64,
}

/// The two windows, in days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windows {
    pub stale_after_days: i64,
    pub archive_after_days: i64,
}

impl Default for Windows {
    fn default() -> Self {
        Windows {
            stale_after_days: DEFAULT_STALE_AFTER_DAYS,
            archive_after_days: DEFAULT_ARCHIVE_AFTER_DAYS,
        }
    }
}

/// What one pass did, as upstream counts it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub marked_stale: u32,
    pub archived: u32,
    pub reactivated: u32,
    pub checked: u32,
}

/// The state `row` moves to at `now`, or `None` when it stays. A pinned
/// skill, and one `protected` (upstream: referenced by a cron job), never
/// moves.
pub fn transition(row: &Row, protected: bool, now: i64, windows: Windows) -> Option<State> {
    if row.pinned || protected {
        return None;
    }
    let stale_cutoff = now - windows.stale_after_days * DAY_SECS;
    let archive_cutoff = now - windows.archive_after_days * DAY_SECS;
    // Never-active skills anchor on created_at so they don't self-archive.
    let anchor = row.last_activity_at.or(row.created_at).unwrap_or(now);
    let current = row.state;
    // use_count == 0 is absence of evidence, not staleness: never archive a
    // never-used skill younger than stale_after_days.
    if row.use_count == 0 && anchor > stale_cutoff {
        return (current == State::Stale).then_some(State::Active);
    }
    if anchor <= archive_cutoff && current != State::Archived {
        Some(State::Archived)
    } else if anchor <= stale_cutoff && current == State::Active {
        Some(State::Stale)
    } else if anchor > stale_cutoff && current == State::Stale {
        // Used again after going stale.
        Some(State::Active)
    } else {
        None
    }
}

/// Move every row between active, stale and archived at `now`; `protected`
/// names skills that are in use by definition.
pub fn apply_automatic_transitions(
    rows: &mut [Row],
    protected: &BTreeSet<String>,
    now: i64,
    windows: Windows,
) -> Counts {
    let mut counts = Counts::default();
    for row in rows.iter_mut() {
        counts.checked += 1;
        let Some(next) = transition(row, protected.contains(&row.name), now, windows) else {
            continue;
        };
        match next {
            State::Archived => counts.archived += 1,
            State::Stale => counts.marked_stale += 1,
            State::Active => counts.reactivated += 1,
        }
        row.state = next;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-04-30T00:00:00Z.
    const NOW: i64 = 1_777_507_200;

    fn days_ago(days: i64) -> i64 {
        NOW - days * DAY_SECS
    }

    /// Upstream's `_backdate`: an agent-created record whose activity is
    /// `days` old.
    fn backdated(name: &str, days: i64, use_count: u64) -> Row {
        let at = days_ago(days);
        Row {
            name: name.to_owned(),
            state: State::Active,
            pinned: false,
            last_activity_at: (use_count > 0).then_some(at),
            created_at: Some(at),
            use_count,
        }
    }

    #[test]
    fn hermes_upstream_pinned_skill_is_never_touched() {
        let mut rows = [Row {
            pinned: true,
            ..backdated("precious", 365, 1)
        }];
        let counts =
            apply_automatic_transitions(&mut rows, &BTreeSet::new(), NOW, Windows::default());
        assert_eq!(counts.archived, 0);
        assert_eq!(counts.marked_stale, 0);
        assert_eq!(rows[0].state, State::Active);
        assert!(rows[0].pinned);
    }

    #[test]
    fn hermes_upstream_cron_referenced_skill_by_name_survives_inactivity() {
        let mut rows = [backdated("quarterly-report", 200, 1)];
        let protected = BTreeSet::from(["quarterly-report".to_owned()]);
        let counts = apply_automatic_transitions(&mut rows, &protected, NOW, Windows::default());
        assert_eq!(counts.archived, 0);
        assert_eq!(rows[0].state, State::Active);
    }

    #[test]
    fn hermes_upstream_unreferenced_skill_is_still_archived() {
        let mut rows = [
            backdated("quarterly-report", 200, 1),
            backdated("orphan", 200, 1),
        ];
        let protected = BTreeSet::from(["quarterly-report".to_owned()]);
        apply_automatic_transitions(&mut rows, &protected, NOW, Windows::default());
        assert_eq!(rows[0].state, State::Active);
        assert_eq!(rows[1].state, State::Archived);
    }

    #[test]
    fn hermes_upstream_recent_view_activity_prevents_false_stale_transition() {
        let mut rows = [Row {
            last_activity_at: Some(days_ago(1)),
            created_at: Some(days_ago(60)),
            ..backdated("recently-viewed", 60, 1)
        }];
        let windows = Windows {
            stale_after_days: 30,
            archive_after_days: 90,
        };
        let counts = apply_automatic_transitions(&mut rows, &BTreeSet::new(), NOW, windows);
        assert_eq!(counts.marked_stale, 0);
        assert_eq!(rows[0].state, State::Active);
    }

    /// 95.3 acceptance 1, against upstream's constants: 13 days unused is
    /// active, 14 stale, 30 archived — from active or from stale — and a
    /// stale skill whose anchor moved (used, or patched, again) is active.
    /// A skill nobody ever used ages from its creation alike.
    #[test]
    fn curator_state_machine() {
        assert_eq!(DEFAULT_STALE_AFTER_DAYS, 14);
        assert_eq!(DEFAULT_ARCHIVE_AFTER_DAYS, 30);
        let at = |days: i64, state: State, use_count: u64| {
            transition(
                &Row {
                    state,
                    ..backdated("tidy", days, use_count)
                },
                false,
                NOW,
                Windows::default(),
            )
        };
        for use_count in [0, 1] {
            assert_eq!(at(13, State::Active, use_count), None, "13 days: active");
            assert_eq!(
                at(14, State::Active, use_count),
                Some(State::Stale),
                "14 days: stale"
            );
            assert_eq!(at(29, State::Stale, use_count), None, "stale stays stale");
            assert_eq!(
                at(30, State::Active, use_count),
                Some(State::Archived),
                "30 days: archived"
            );
            assert_eq!(
                at(30, State::Stale, use_count),
                Some(State::Archived),
                "30 days from stale: archived"
            );
            assert_eq!(at(400, State::Archived, use_count), None, "archived once");
            assert_eq!(
                at(3, State::Stale, use_count),
                Some(State::Active),
                "used again: active"
            );
        }
        let mut rows = [
            backdated("a", 14, 1),
            backdated("b", 31, 0),
            Row {
                state: State::Stale,
                ..backdated("c", 2, 1)
            },
        ];
        let counts =
            apply_automatic_transitions(&mut rows, &BTreeSet::new(), NOW, Windows::default());
        assert_eq!(
            counts,
            Counts {
                marked_stale: 1,
                archived: 1,
                reactivated: 1,
                checked: 3
            }
        );
    }
}

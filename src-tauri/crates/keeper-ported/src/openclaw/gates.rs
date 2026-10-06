//! The deep phase's gates, as OpenClaw documents them
//! (`docs/concepts/dreaming.md`, `docs/cli/memory.md` at 07c176c3): a
//! structural exclusion before any ranking, then a weighted score and three
//! thresholds that must all pass, and on a rewrite a bound on how much of
//! the previous file may be lost.
//!
//! Written from documentation; no upstream code is copied (`UPSTREAM.md`).
//! OpenClaw ranks six signals; keeper measures four of them, so the score
//! is their weighted sum renormalised by those four weights' total.

/// `phases.deep.minScore`.
pub const MIN_SCORE: f64 = 0.75;
/// `phases.deep.minRecallCount`.
pub const MIN_RECALL_COUNT: u32 = 3;
/// `phases.deep.minUniqueQueries`.
pub const MIN_UNIQUE_QUERIES: u32 = 3;
/// `phases.deep.recencyHalfLifeDays`.
pub const RECENCY_HALF_LIFE_DAYS: f64 = 14.0;
/// `phases.deep.maxAgeDays`.
pub const MAX_AGE_DAYS: f64 = 30.0;
/// `phases.deep.maxPriorEntryLossFraction` (documented default 0.25, the
/// bound AD-401 names).
pub const MAX_PRIOR_ENTRY_LOSS_FRACTION: f64 = 0.25;

/// The documented deep-ranking weights.
pub const WEIGHT_RELEVANCE: f64 = 0.30;
pub const WEIGHT_FREQUENCY: f64 = 0.24;
pub const WEIGHT_QUERY_DIVERSITY: f64 = 0.15;
pub const WEIGHT_RECENCY: f64 = 0.15;
pub const WEIGHT_CONSOLIDATION: f64 = 0.10;
pub const WEIGHT_CONCEPTUAL_RICHNESS: f64 = 0.06;

/// The weights of the signals keeper measures: frequency, query
/// diversity, recency and consolidation (0.64).
pub const MEASURED_WEIGHT: f64 =
    WEIGHT_FREQUENCY + WEIGHT_QUERY_DIVERSITY + WEIGHT_RECENCY + WEIGHT_CONSOLIDATION;

/// Where a candidate's evidence came from, as the structural gate reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Trusted,
    /// Removed before ranking: "a structural taint gate, not a score penalty".
    Untrusted,
    System,
}

/// The session a candidate was observed in: only interactive sessions are
/// eligible; cron, heartbeat, subagent and unknown sessions stay out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Interactive,
    Cron,
    Heartbeat,
    Subagent,
    Unknown,
}

/// Why the structural gate keeps a candidate out of ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    Provenance(Provenance),
    Session(SessionKind),
}

/// The structural gate: `None` when the candidate may be ranked.
pub fn excluded(provenance: Provenance, session: SessionKind) -> Option<Exclusion> {
    if provenance != Provenance::Trusted {
        return Some(Exclusion::Provenance(provenance));
    }
    if session != SessionKind::Interactive {
        return Some(Exclusion::Session(session));
    }
    None
}

/// What keeper measures of one candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signals {
    /// How many times it was recalled (proposed).
    pub recall_count: u32,
    /// Distinct recall queries (source sessions).
    pub unique_queries: u32,
    /// Distinct days it recurred on.
    pub days: u32,
    /// Days since its newest observation.
    pub age_days: f64,
}

/// Time-decayed freshness: `0.5^(age / half-life)`.
pub fn recency(age_days: f64) -> f64 {
    0.5f64.powf(age_days.max(0.0) / RECENCY_HALF_LIFE_DAYS)
}

/// A count as a signal: saturating at the threshold it is gated on.
fn saturating(count: u32, at: u32) -> f64 {
    (f64::from(count) / f64::from(at)).min(1.0)
}

/// The weighted score over the measured signals, renormalised to 0..=1.
pub fn score(signals: &Signals) -> f64 {
    let frequency = saturating(signals.recall_count, MIN_RECALL_COUNT);
    let diversity = saturating(signals.unique_queries, MIN_UNIQUE_QUERIES);
    let consolidation = saturating(signals.days, 3);
    (WEIGHT_FREQUENCY * frequency
        + WEIGHT_QUERY_DIVERSITY * diversity
        + WEIGHT_RECENCY * recency(signals.age_days)
        + WEIGHT_CONSOLIDATION * consolidation)
        / MEASURED_WEIGHT
}

/// What the gates make of a candidate tonight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Every threshold passes.
    Promote,
    /// Not yet: it waits for more evidence.
    Pending,
    /// Its newest observation is older than [`MAX_AGE_DAYS`].
    Expired,
}

/// The deep phase's gates over `signals`: too old expires; otherwise the
/// score, the recall count and the query diversity must all pass.
pub fn gate(signals: &Signals) -> Gate {
    if signals.age_days > MAX_AGE_DAYS {
        return Gate::Expired;
    }
    if score(signals) >= MIN_SCORE
        && signals.recall_count >= MIN_RECALL_COUNT
        && signals.unique_queries >= MIN_UNIQUE_QUERIES
    {
        Gate::Promote
    } else {
        Gate::Pending
    }
}

/// Whether a rewrite keeping `kept` of `prior` previous entries loses more
/// than [`MAX_PRIOR_ENTRY_LOSS_FRACTION`] of them.
pub fn loses_too_much(prior: usize, kept: usize) -> bool {
    if prior == 0 {
        return false;
    }
    let lost = prior.saturating_sub(kept) as f64;
    lost / prior as f64 > MAX_PRIOR_ENTRY_LOSS_FRACTION
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden table in `UPSTREAM.md`, row by row: what keeper's score
    /// and gates make of each measured candidate.
    #[test]
    fn gates_and_score_table() {
        let record = include_str!("UPSTREAM.md");
        let rows: Vec<Vec<&str>> = record
            .lines()
            .skip_while(|line| !line.starts_with("## Golden table"))
            .filter(|line| {
                line.starts_with("| ") && !line.contains("recall") && !line.contains("---")
            })
            .map(|line| {
                line.trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(rows.len() >= 10, "the golden table is in UPSTREAM.md");
        for row in rows {
            let number = |at: usize| row[at].parse::<u32>().expect("a count");
            let signals = Signals {
                recall_count: number(0),
                unique_queries: number(1),
                days: number(2),
                age_days: row[3].parse().expect("an age"),
            };
            assert_eq!(format!("{:.3}", score(&signals)), row[4], "{row:?}");
            let said = match gate(&signals) {
                Gate::Promote => "promote",
                Gate::Pending => "pending",
                Gate::Expired => "expired",
            };
            assert_eq!(said, row[5], "{row:?}");
        }
    }

    #[test]
    fn the_structural_gate_admits_only_trusted_interactive_evidence() {
        assert_eq!(
            excluded(Provenance::Trusted, SessionKind::Interactive),
            None
        );
        for provenance in [Provenance::Untrusted, Provenance::System] {
            assert_eq!(
                excluded(provenance, SessionKind::Interactive),
                Some(Exclusion::Provenance(provenance))
            );
        }
        for session in [
            SessionKind::Cron,
            SessionKind::Heartbeat,
            SessionKind::Subagent,
            SessionKind::Unknown,
        ] {
            assert_eq!(
                excluded(Provenance::Trusted, session),
                Some(Exclusion::Session(session))
            );
        }
    }

    #[test]
    fn a_quarter_may_be_lost_and_no_more() {
        assert!(!loses_too_much(4, 3));
        assert!(loses_too_much(4, 2));
        assert!(!loses_too_much(0, 0));
        assert!(!loses_too_much(3, 4));
    }
}

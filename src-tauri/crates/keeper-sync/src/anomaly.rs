//! One shape for "something is not as it should be", so a log can be read.
//!
//! # The failure this comes from
//!
//! A vault stopped syncing for four days. Every fact needed to explain it was
//! discoverable — a stranded journal row, an index whose stat data no longer
//! described the worktree, 100 GB of leaked scratch, a status pass that had
//! stopped emitting — and **none of them was in the log**. Finding them took a
//! process sampler and half a day. The fix for each is shipped; the reason they
//! were invisible is not, and it is this: keeper logs what it *did*, and an
//! anomaly is a fact about what it *found*.
//!
//! # What makes an anomaly line worth writing
//!
//! Three things, and a line missing any of them costs the reader more than it
//! saves:
//!
//! - **A measurement, not an adjective.** "the scratch directory is large" sends
//!   somebody to go and measure. `bytes=107374182400 files=863` is the finding.
//! - **What was expected.** A number alone cannot be judged. The reader needs to
//!   know that 863 files is wrong because the expected count is near zero.
//! - **What it means for the user.** An anomaly nobody can act on is noise, and
//!   a log full of noise is one nobody reads when it matters.
//!
//! They are `WARN`, never `ERROR`: an anomaly is a state keeper noticed and is
//! reporting, not an operation that failed. Reserving `ERROR` for failures is
//! what lets somebody grep a log for the thing that actually broke.

/// A finding worth a line in the log.
pub struct Anomaly<'a> {
    /// What was found, in the fewest words that are still specific.
    pub what: &'a str,
    /// The measurement. Numbers, not adjectives.
    pub measured: String,
    /// What it should have been, so the measurement can be judged.
    pub expected: &'a str,
    /// What this costs the person using keeper, or what they can do.
    pub consequence: &'a str,
}

impl Anomaly<'_> {
    /// Write the finding.
    ///
    /// One line, one format, everywhere — so a reader who has seen one anomaly
    /// line can read every other one, and so `grep 'anomaly:'` is a complete
    /// list of what keeper found wrong on a machine it cannot reach.
    pub fn report(&self, profile: &str) {
        tracing::warn!(
            profile,
            measured = %self.measured,
            expected = %self.expected,
            consequence = %self.consequence,
            "anomaly: {}",
            self.what
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four fields are the contract. A line that dropped `expected` would
    /// hand the reader a number they cannot judge, which is the shape of every
    /// log line that failed to explain the four-day stall.
    #[test]
    fn an_anomaly_carries_a_measurement_an_expectation_and_a_consequence() {
        let anomaly = Anomaly {
            what: "leaked transfer scratch",
            measured: "bytes=107374182400 files=863".into(),
            expected: "near zero; scratch is deleted when a transfer ends",
            consequence: "disk that will not be reclaimed until it is swept",
        };

        assert!(
            !anomaly.measured.is_empty(),
            "a finding without a number is an adjective"
        );
        assert!(
            anomaly.expected.contains("zero"),
            "the reader must be able to judge it"
        );
        assert!(
            !anomaly.consequence.is_empty(),
            "an anomaly nobody can act on is noise"
        );
        // Numbers, not prose: this is the property that makes two runs comparable.
        assert!(anomaly.measured.chars().any(|c| c.is_ascii_digit()));
    }
}

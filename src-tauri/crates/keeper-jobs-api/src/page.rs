//! The `jobs_page` scope value (RF C-19, ruling G-11).
//!
//! A job that has not ended lives under `<domain>/open`; the command that
//! ends it moves its `jobs` and `job_claims` rows to `<domain>/<yyyy-mm>`,
//! the UTC month it ended in, in the same transaction, by Syncular's
//! explicit scope move (`rescope`). A client that subscribes the current
//! month and `<domain>/open` therefore holds every job still running,
//! whenever it was submitted, and every job that ended this month.

use crate::model::JobState;

/// The value under which a job that has not ended lives.
#[must_use]
pub fn open(domain: &str) -> String {
    format!("{domain}/open")
}

/// The value of the UTC month that `ms` falls in.
#[must_use]
pub fn month(domain: &str, ms: u64) -> String {
    let (year, month) = year_month(ms);
    format!("{domain}/{year:04}-{month:02}")
}

/// Where a job in `state` lives: `open` until it ends, then the month of
/// `ended_at_ms`. `None` for a terminal state with no end time.
#[must_use]
pub fn for_state(domain: &str, state: JobState, ended_at_ms: Option<u64>) -> Option<String> {
    if state.is_terminal() {
        ended_at_ms.map(|ms| month(domain, ms))
    } else {
        Some(open(domain))
    }
}

/// A parsed `jobs_page` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageKey<'a> {
    Open {
        domain: &'a str,
    },
    Month {
        domain: &'a str,
        year: u32,
        month: u32,
    },
}

/// Parses a `jobs_page` value. The domain is everything before the last
/// `/`; the rest is `open` or a `yyyy-mm` month (`01`..`12`).
#[must_use]
pub fn parse(value: &str) -> Option<PageKey<'_>> {
    let (domain, tail) = value.rsplit_once('/')?;
    if domain.is_empty() {
        return None;
    }
    if tail == "open" {
        return Some(PageKey::Open { domain });
    }
    let bytes = tail.as_bytes();
    if bytes.len() != 7
        || bytes[4] != b'-'
        || !bytes[..4].iter().chain(&bytes[5..]).all(u8::is_ascii_digit)
    {
        return None;
    }
    let year = tail[..4].parse().ok()?;
    let month = tail[5..].parse().ok()?;
    (1..=12).contains(&month).then_some(PageKey::Month {
        domain,
        year,
        month,
    })
}

/// The UTC (year, month) of a time in milliseconds since the epoch.
fn year_month(ms: u64) -> (u64, u64) {
    // Howard Hinnant's civil_from_days, for days on or after 1970-01-01.
    let days = ms / 86_400_000;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_boundaries_are_utc() {
        // 2026-10-31T23:59:59.999Z and 2026-11-01T00:00:00.000Z.
        assert_eq!(month("d", 1_793_491_199_999), "d/2026-10");
        assert_eq!(month("d", 1_793_491_200_000), "d/2026-11");
        assert_eq!(month("d", 0), "d/1970-01");
        // 2024-02-29T12:00:00Z, a leap day.
        assert_eq!(month("d", 1_709_208_000_000), "d/2024-02");
        // 2027-01-01T00:00:00Z.
        assert_eq!(month("d", 1_798_761_600_000), "d/2027-01");
    }

    #[test]
    fn parse_reads_back_both_forms_and_refuses_others() {
        assert_eq!(parse("a/b/open"), Some(PageKey::Open { domain: "a/b" }));
        assert_eq!(
            parse("d/2026-11"),
            Some(PageKey::Month {
                domain: "d",
                year: 2026,
                month: 11
            })
        );
        for bad in [
            "d/2026-13",
            "d/2026-00",
            "/open",
            "open",
            "d/2026-1",
            "d/26-11-01",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}

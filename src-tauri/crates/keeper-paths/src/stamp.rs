//! Dependency-free civil dates and UTC conflict stamps.

/// A wall-clock millisecond count as `(year, month, day, hour, minute, second)`.
///
/// Hand-rolled because `keeper-sync` deliberately has no `chrono`: the engine
/// is time-agnostic and takes wall-clock milliseconds from this port. The
/// civil-date arithmetic is Howard Hinnant's `days_from_civil` inverse, which
/// is exact for every date we can represent.
///
/// **One copy, shared by every caller that formats an instant.**
/// Two consumers now format an instant into words a person reads — the
/// conflict filename `in the sync engine` stamps and the `.trashinfo` stamp
/// `in the sync writer` writes — and they want different separators over the
/// identical decomposition. Two copies of a leap-year calculation is two
/// chances to get 2100 wrong, and only one of them would be found.
///
/// Takes whatever `ms` it is given and says nothing about its zone: the caller
/// decides whether it is handing over UTC or a local clock, because the two
/// consumers genuinely differ (a conflict filename must be comparable across
/// machines; a `DeletionDate` is defined as local time).
pub fn civil_from_unix_ms(ms: i64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1_000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (hour, minute, second) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);

    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (
        year,
        month as u32,
        day as u32,
        hour as u32,
        minute as u32,
        second as u32,
    )
}

/// UTC `yyyymmdd-hhmmss` for a conflict filename.
///
/// UTC deliberately, unlike the `.trashinfo` `DeletionDate`
/// the sync writer writes: this name has to sort and compare the same
/// on both machines that produced the conflict, and a local stamp would put
/// two names an hour apart on one event.
///
/// The civil-date arithmetic itself is
/// [`civil_from_unix_ms`], which is where it lives now that
/// two modules format an instant into words.
pub fn conflict_stamp(now_ms: i64) -> String {
    let (year, month, day, hh, mm, ss) = civil_from_unix_ms(now_ms);
    format!("{year:04}{month:02}{day:02}-{hh:02}{mm:02}{ss:02}")
}

#[cfg(test)]
mod tests {
    use super::conflict_stamp;
    #[test]
    fn conflict_stamps_are_correct_utc_civil_dates() {
        // Hand-rolled civil-date arithmetic is exactly the kind of code that is
        // silently wrong for years, so pin real epochs including a leap day and
        // the pre-epoch direction.
        assert_eq!(conflict_stamp(0), "19700101-000000");
        assert_eq!(conflict_stamp(1_000), "19700101-000001");
        assert_eq!(conflict_stamp(951_782_400_000), "20000229-000000");
        assert_eq!(conflict_stamp(1_709_164_800_000), "20240229-000000");
        assert_eq!(conflict_stamp(1_753_444_800_000), "20250725-120000");
        // A clock before the epoch must not produce a negative-looking name.
        assert_eq!(conflict_stamp(-1_000), "19691231-235959");
    }
}

//! File-backed temporary-space lifetimes.
pub const DEFAULT_SPACE_TTL_HOURS: u32 = 48;
pub const PARKED_SEARCH_TTL_HOURS: u32 = 2;
const HOUR_MS: i64 = 3_600_000;

pub fn read_ttl_hours(value: &super::frontmatter::FieldValue) -> Result<u32, String> {
    match value {
        super::frontmatter::FieldValue::Num(hours)
            if hours.is_finite()
                && *hours >= 1.0
                && *hours <= f64::from(u32::MAX)
                && hours.fract() == 0.0 =>
        {
            Ok(*hours as u32)
        }
        _ => Err("The space lifetime must be a positive whole number of hours.".to_owned()),
    }
}

pub fn read_expires(raw: &str) -> Result<i64, String> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|date| date.timestamp_millis())
        .or_else(|_| raw.trim().parse::<i64>())
        .map_err(|_| "The space expiry is not a valid timestamp.".to_owned())
}

#[must_use]
pub fn expiry_stamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map_or_else(
        || ms.to_string(),
        |date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    )
}

#[must_use]
pub fn expires_at(now_ms: i64, ttl_hours: u32) -> i64 {
    now_ms.saturating_add(i64::from(ttl_hours) * HOUR_MS)
}

#[must_use]
pub fn should_touch(now_ms: i64, expires_ms: i64, ttl_hours: u32) -> bool {
    let duration = i64::from(ttl_hours) * HOUR_MS;
    ttl_hours > 0 && now_ms.saturating_sub(expires_ms.saturating_sub(duration)) > duration / 10
}

#[must_use]
pub fn expiry_phrase(now_ms: i64, expires_ms: Option<i64>) -> String {
    let Some(expires) = expires_ms else {
        return String::new();
    };
    let days = expires.saturating_sub(now_ms) / (24 * HOUR_MS);
    if days < 1 {
        "expires today".to_owned()
    } else {
        format!("expires in {days} d")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tenth_is_strict_and_future_clocks_do_not_touch() {
        let end = expires_at(100, 2);
        assert!(!should_touch(100 + 720_000, end, 2));
        assert!(should_touch(101 + 720_000, end, 2));
        assert!(!should_touch(0, end, 2));
        assert!(!should_touch(100, 0, 0));
    }
    #[test]
    fn stored_lifetimes_validate_instead_of_becoming_zero() {
        use super::super::frontmatter::FieldValue;
        assert_eq!(read_ttl_hours(&FieldValue::Num(48.0)), Ok(48));
        for value in [
            FieldValue::Num(0.0),
            FieldValue::Num(-3.0),
            FieldValue::Num(2.5),
            FieldValue::Str("bananas".into()),
        ] {
            assert!(read_ttl_hours(&value).is_err());
        }
        assert_eq!(read_expires("2026-09-22T10:00:00Z"), Ok(1_790_071_200_000));
        assert!(read_expires("yesterday-ish").is_err());
    }
    #[test]
    fn phrases_use_only_whole_days() {
        assert_eq!(expiry_phrase(0, Some(expires_at(0, 48))), "expires in 2 d");
        assert_eq!(expiry_phrase(1, Some(expires_at(0, 48))), "expires in 1 d");
        assert_eq!(expiry_phrase(0, Some(expires_at(0, 23))), "expires today");
        assert_eq!(expiry_phrase(100, Some(0)), "expires today");
        assert_eq!(expiry_phrase(0, None), "");
    }
}

//! One byte-size formatter for the whole product (Story 45.5, FR-178).
//!
//! # Why this module exists
//!
//! Before this story keeper had **six** independent byte formatters and they
//! did not agree with each other. `keeper/src/tray.rs::format_size` and
//! [`crate::error::format_gb`] divide by 1000. `keeper-sync`'s
//! `progress::format_bytes`, `keeper-syncd`'s `commands::format_bytes` and both
//! copies in `src/components/chat/` divide by **1024 and print `MB`**, so a
//! 1 500 000-byte attachment renders as "1.4 MB" in chat and "1.5 MB" in
//! Finder. A person who compares two of keeper's own surfaces, or one of them
//! against the operating system, concludes that one of them is lying, and they
//! are right.
//!
//! This is the one implementation. A surface that needs a size either reads a
//! label a view model already carries ([`crate::vm::FileSizeVm::label`]) or
//! calls this function; nothing computes its own. The TypeScript mirror at
//! `src/lib/file-size.ts` exists only for the chat composer, which formats a
//! `File` the user just picked in the webview and has no Rust in its path — and
//! it is pinned to this function by `file-size-vectors.json`, which both test
//! suites load, so the two cannot drift silently.
//!
//! # Decimal, on purpose
//!
//! **1 kB is 1000 bytes here.** macOS Finder has reported decimal sizes since
//! 10.6, keeper is a macOS-first application, and the number keeper puts next
//! to a file must be the number the operating system puts next to the same
//! file. Binary units are defensible in a disk-image tool and wrong for a file
//! browser sitting beside Finder.
//!
//! The unit names follow SI, which is why it is a lowercase `k` in `kB`: `KB`
//! is not a unit and `KiB` is the binary one this module deliberately never
//! produces. The spelling is part of the claim, and a test asserts it.
//!
//! # Integer arithmetic, truncating
//!
//! No floats. A `u64` byte count above 2^53 loses precision as an `f64`, and
//! `{:.1}` rounds — which is how a 999 999-byte file becomes "1000.0 kB", a
//! string carrying a unit one step too small and a figure that cannot occur in
//! it. Truncation cannot carry, so the unit chosen by the magnitude test is
//! always the unit printed, and the figure never overstates what is on disk.

/// The unit ladder, decimal. Each divisor is 1000x the one before, and every
/// divisor is at least 1000 — [`format_file_size`] divides by `divisor / 10`
/// and relies on that division being exact.
///
/// `EB` is present for totality rather than realism: `u64::MAX` is about
/// 18.4 EB, and stopping the ladder at `PB` would render an absurd-but-legal
/// count as "18446744 PB" instead of "18 EB". A formatter with an unbounded top
/// line is a formatter that can produce a string no column was sized for.
const UNITS: [(u64, &str); 6] = [
    (1_000_000_000_000_000_000, "EB"),
    (1_000_000_000_000_000, "PB"),
    (1_000_000_000_000, "TB"),
    (1_000_000_000, "GB"),
    (1_000_000, "MB"),
    (1_000, "kB"),
];

/// Format a byte count the way a person reads one (Story 45.5, FR-178).
///
/// Below 1000 bytes the count is exact and the unit is spelled out — `0 bytes`,
/// `1 byte`, `999 bytes`. There is no reason to round a number a person can
/// read at a glance, and "1 byte" rather than "1 bytes" is the kind of detail
/// whose absence makes a surface feel unfinished.
///
/// At 1000 bytes and above the largest unit yielding a figure of at least 1 is
/// used, with one decimal place below ten and none at or above it: `1.0 kB`,
/// `9.9 kB`, `10 kB`, `999 kB`, `1.0 MB`. One significant fraction is what a
/// size column is for; three would be a checksum.
///
/// **A directory has no size and must not be passed here.** The caller carries
/// an [`Option`] ([`crate::vm::FilesEntryVm::size`]) and `None` renders as
/// nothing at all — a folder showing "0 bytes" is a claim about its contents
/// that is false for every folder that has any. This function has no directory
/// branch precisely so the absence is modelled at the type crossing the wire
/// rather than papered over with a string.
///
/// ```
/// use keeper_core::size::format_file_size;
/// assert_eq!(format_file_size(0), "0 bytes");
/// assert_eq!(format_file_size(1), "1 byte");
/// assert_eq!(format_file_size(999), "999 bytes");
/// assert_eq!(format_file_size(1_000), "1.0 kB");
/// // Decimal, so 1024 bytes is just over a kilobyte and not exactly one.
/// assert_eq!(format_file_size(1_024), "1.0 kB");
/// assert_eq!(format_file_size(5_000_000_000), "5.0 GB");
/// ```
pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1_000 {
        // The one place the unit is a word rather than a symbol, and the one
        // place the count is exact.
        return if bytes == 1 {
            "1 byte".to_owned()
        } else {
            format!("{bytes} bytes")
        };
    }
    for (divisor, unit) in UNITS {
        if bytes < divisor {
            continue;
        }
        // `bytes / (divisor / 10)` rather than `bytes * 10 / divisor`: the
        // latter overflows for any count within a factor of ten of `u64::MAX`,
        // and every divisor here is a multiple of 1000, so `divisor / 10` is
        // exact.
        let tenths = bytes / (divisor / 10);
        let whole = tenths / 10;
        let frac = tenths % 10;
        return if whole < 10 {
            format!("{whole}.{frac} {unit}")
        } else {
            format!("{whole} {unit}")
        };
    }
    // Unreachable: `bytes >= 1_000` and the last rung of the ladder is 1_000,
    // so the loop always returns. Spelled as the exact-byte form rather than a
    // panic, because a formatter that can abort is a formatter that can take
    // down a whole file listing over one number.
    format!("{bytes} bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector table shared with the TypeScript mirror.
    ///
    /// `include_str!` rather than a runtime read: the fixture is compiled into
    /// the test binary, so a deleted or renamed fixture is a build failure
    /// rather than a test that quietly passes over zero vectors. The Vitest
    /// side reaches for the same path on disk.
    const VECTORS_JSON: &str = include_str!("file-size-vectors.json");

    /// Every vector in `file-size-vectors.json`, which the TypeScript mirror
    /// asserts against as well (Story 45.5).
    ///
    /// **This is the anti-drift mechanism, not a convenience.** `src/lib/file-size.ts`
    /// is a second implementation of this function in another language, needed
    /// because the chat composer formats a `File` the webview holds and there
    /// is no Rust in that path. A mirror that is merely documented as a mirror
    /// drifts within two months. A mirror pinned to a checked-in vector table
    /// that both suites load fails on the same commit that breaks it.
    ///
    /// The guard on the vector count is there so that emptying or truncating
    /// the fixture — the way this test would be silently defeated — fails
    /// loudly instead.
    #[test]
    fn every_shared_vector_matches_and_the_table_is_not_empty() {
        let parsed: serde_json::Value =
            serde_json::from_str(VECTORS_JSON).expect("the shared vector fixture parses");
        assert_eq!(
            parsed["base"], 1000,
            "the fixture states the base; a change here is a product-wide decision"
        );
        let vectors = parsed["vectors"]
            .as_array()
            .expect("the fixture carries a vectors array");
        assert!(
            vectors.len() >= 25,
            "the shared table has been truncated to {} vectors; it is the contract with \
             src/lib/file-size.ts and shrinking it silently weakens both suites",
            vectors.len()
        );
        for vector in vectors {
            let raw = vector["bytes"].as_str().expect("bytes is a decimal string");
            let bytes: u64 = raw.parse().expect("bytes parses as u64");
            let expected = vector["label"].as_str().expect("label is a string");
            assert_eq!(
                format_file_size(bytes),
                expected,
                "vector {raw} ({})",
                vector["why"].as_str().unwrap_or_default()
            );
        }
    }

    /// The zero and the singular, which are the two a size column gets wrong.
    ///
    /// "0 bytes" is a real answer — an empty file exists and is worth saying so
    /// about. It is a *directory* that must render nothing, and that distinction
    /// lives on the view model's `Option`, not here.
    #[test]
    fn small_counts_are_exact_and_the_singular_is_singular() {
        assert_eq!(format_file_size(0), "0 bytes");
        assert_eq!(format_file_size(1), "1 byte");
        assert_eq!(format_file_size(2), "2 bytes");
        assert_eq!(format_file_size(999), "999 bytes");
    }

    /// The whole of the decimal decision, asserted at the boundary.
    ///
    /// 999 to 1000 is where the unit changes; 1023 to 1024 is where it would
    /// change if the base were binary. Both are pinned: the first proves the
    /// step happens at 1000, the second proves it has already happened by 1024
    /// and that 1024 is in no way special. 1 500 000 is the value that tells
    /// the two bases apart in a unit above kB — a 1024-based formatter calls it
    /// 1.4 MB.
    #[test]
    fn the_base_is_1000_and_the_boundary_is_where_it_says() {
        assert_eq!(format_file_size(999), "999 bytes");
        assert_eq!(format_file_size(1_000), "1.0 kB");
        assert_eq!(format_file_size(1_023), "1.0 kB");
        assert_eq!(
            format_file_size(1_024),
            "1.0 kB",
            "1024 bytes is 1.024 kB, not 1 KiB: keeper reports what Finder reports"
        );
        assert_eq!(
            format_file_size(1_500_000),
            "1.5 MB",
            "a 1024-based divisor renders this as 1.4 MB"
        );
        // The binary spellings must never be produced. A `KiB` or a `KB` on
        // screen means somebody reintroduced a second formatter.
        for bytes in [1_024_u64, 1_048_576, 1_073_741_824] {
            let rendered = format_file_size(bytes);
            assert!(
                !rendered.contains("iB") && !rendered.contains("KB"),
                "binary or mis-cased unit leaked: {rendered}"
            );
        }
    }

    /// One decimal below ten, none at or above it, and the unit steps at each
    /// power of a thousand.
    #[test]
    fn precision_drops_at_ten_and_the_ladder_steps_at_each_thousand() {
        assert_eq!(format_file_size(1_500), "1.5 kB");
        assert_eq!(format_file_size(9_999), "9.9 kB");
        assert_eq!(format_file_size(10_000), "10 kB");
        assert_eq!(format_file_size(999_999), "999 kB");
        assert_eq!(format_file_size(1_000_000), "1.0 MB");
        assert_eq!(format_file_size(999_999_999), "999 MB");
        assert_eq!(format_file_size(1_000_000_000), "1.0 GB");
    }

    /// A file big enough to need GB, and the rungs above it.
    #[test]
    fn large_files_reach_the_units_they_need() {
        assert_eq!(format_file_size(2_500_000_000), "2.5 GB");
        assert_eq!(format_file_size(5_000_000_000), "5.0 GB");
        assert_eq!(format_file_size(12_300_000_000), "12 GB");
        assert_eq!(format_file_size(1_000_000_000_000), "1.0 TB");
        assert_eq!(format_file_size(1_000_000_000_000_000), "1.0 PB");
        assert_eq!(format_file_size(1_000_000_000_000_000_000), "1.0 EB");
    }

    /// `u64::MAX` renders as a size, not as a panic and not as a figure in a
    /// unit that ran out. The top rung of the ladder is the reason.
    ///
    /// This is also the overflow guard: an implementation that multiplies
    /// before dividing wraps here and prints a small number.
    #[test]
    fn the_largest_possible_count_still_renders_as_a_size() {
        assert_eq!(format_file_size(u64::MAX), "18 EB");
    }

    /// Truncation, not rounding: the figure never overstates the bytes and
    /// never carries out of its own unit.
    ///
    /// This is the property that keeps 999 999 from becoming "1000.0 kB" — a
    /// string whose figure cannot occur in its stated unit. Asserted over every
    /// unit step rather than as one example, because the carry bug appears only
    /// within one part in ten thousand of a boundary and a single hand-picked
    /// case walks past three of the four places it can happen.
    #[test]
    fn the_figure_never_rounds_up_past_its_own_unit() {
        for step in [1_000_u64, 1_000_000, 1_000_000_000, 1_000_000_000_000] {
            let just_under = step * 1_000 - 1;
            let rendered = format_file_size(just_under);
            assert!(
                !rendered.starts_with("1000"),
                "{just_under} rendered as {rendered}: a carry escaped its unit"
            );
        }
        assert_eq!(
            format_file_size(1_999),
            "1.9 kB",
            "1.999 kB truncates to 1.9 and must not round to 2.0"
        );
    }

    /// A bigger file never renders in a smaller unit than a smaller one.
    ///
    /// A size column that steps backwards as a file grows is the visible
    /// symptom of a divisor picked before the magnitude test, and a table of
    /// hand-chosen vectors walks straight past it. This walks every rung of the
    /// ladder and both of its neighbours.
    #[test]
    fn a_bigger_file_never_renders_in_a_smaller_unit() {
        let mut probes: Vec<u64> = vec![0, 1, 2, 999];
        for (divisor, _) in UNITS {
            probes.push(divisor - 1);
            probes.push(divisor);
            probes.push(divisor + 1);
        }
        probes.push(u64::MAX);
        probes.sort_unstable();
        probes.dedup();
        // The rung a rendering lands on, counted up from `bytes`. Reading it
        // back off the printed string is deliberate: it tests what a person
        // sees rather than what the function computed.
        let rung = |bytes: u64| -> usize {
            let rendered = format_file_size(bytes);
            let unit = rendered.rsplit(' ').next().unwrap_or_default();
            UNITS
                .iter()
                .rev()
                .position(|(_, name)| *name == unit)
                .map_or(0, |index| index + 1)
        };
        for pair in probes.windows(2) {
            assert!(
                rung(pair[0]) <= rung(pair[1]),
                "{} ({}) renders in a bigger unit than {} ({})",
                pair[0],
                format_file_size(pair[0]),
                pair[1],
                format_file_size(pair[1])
            );
        }
    }
}

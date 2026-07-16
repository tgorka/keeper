//! Runtime macOS-version probe for the `recording` capability (Story 16.3).
//!
//! Screen recording is desktop-macOS-only and floored at macOS 13.0 (the
//! system-audio floor). This module is the platform-adapter version gate (AD-35):
//! it lives in the shell crate so `keeper-core` stays free of `cfg(target_os)` /
//! OS-version logic (AD-26). Detection is dependency-free and `unsafe`-free — it
//! spawns `sw_vers -productVersion` rather than binding an Apple FFI API — and any
//! detection failure defaults the capability to `false` (safe-hide).
//!
//! The pure [`parse_macos_major`] carries the version logic so it is unit-tested
//! without a Mac; the subprocess wrapper is the only untested seam.

/// Parse the major version number from a macOS product-version string.
///
/// Splits on `.` and parses the first component (e.g. `"14.5"` → `14`,
/// `"10.16"` → `10`). Returns `None` for an empty or non-numeric string, so a
/// malformed probe result degrades to the safe-hide default rather than panicking.
pub fn parse_macos_major(v: &str) -> Option<u32> {
    v.trim().split('.').next()?.parse::<u32>().ok()
}

/// Read the running macOS major version by spawning `sw_vers -productVersion`.
///
/// Trims the captured stdout and delegates to [`parse_macos_major`]. Any failure
/// (missing binary, non-zero exit, non-UTF-8 or non-numeric output) yields `None`,
/// which the caller treats as "recording unavailable" (safe-hide).
#[cfg(target_os = "macos")]
fn macos_product_version() -> Option<u32> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_macos_major(stdout.trim())
}

/// Whether screen recording is supported on this platform (Story 16.3).
///
/// `true` only on desktop macOS ≥ 13.0; `false` on older macOS, every non-macOS
/// desktop, and iOS (the compile-time `false` below). Memoized in a
/// `static OnceLock<bool>` so the `sw_vers` subprocess spawns at most once per
/// process even though the palette (per keystroke) and the native menu both
/// re-query the flag.
#[cfg(target_os = "macos")]
pub fn recording_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| match macos_product_version() {
        Some(major) => major >= 13,
        // A failed probe safe-hides recording, but that decision is memoized for
        // the process lifetime — log it so an unexpected hide (vs a genuine
        // pre-13 machine) is observable rather than silent.
        None => {
            tracing::warn!("macOS version probe failed; hiding recording capability (safe-hide)");
            false
        }
    })
}

/// Non-macOS builds (every non-macOS desktop and iOS) never record.
#[cfg(not(target_os = "macos"))]
pub fn recording_supported() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::parse_macos_major;

    #[test]
    fn parses_the_major_component() {
        assert_eq!(parse_macos_major("14.5"), Some(14));
        assert_eq!(parse_macos_major("13.0.1"), Some(13));
        assert_eq!(parse_macos_major("12.7"), Some(12));
        // Legacy `10.x` naming: the major is `10`, below the recording floor.
        assert_eq!(parse_macos_major("10.16"), Some(10));
    }

    #[test]
    fn malformed_input_is_none() {
        assert_eq!(parse_macos_major(""), None);
        assert_eq!(parse_macos_major("abc"), None);
    }
}

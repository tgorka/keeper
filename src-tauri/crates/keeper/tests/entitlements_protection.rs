//! Story 14.7 (FR-65): pin the iOS default-data-protection entitlement.
//!
//! The value must be exactly `NSFileProtectionCompleteUntilFirstUserAuthentication`
//! and never `NSFileProtectionComplete` (which would lock the SQLite WAL after
//! screen lock and stall a resumed sync loop — the epic bans it explicitly).
//!
//! It is pinned in BOTH locations because XcodeGen **regenerates**
//! `keeper_iOS.entitlements` from `project.yml` `entitlements.properties` on every
//! `xcodegen generate`:
//! - `gen/apple/project.yml` — the source of truth the file is regenerated from; a
//!   value living only in the entitlements file would silently revert to `<dict/>`.
//! - `gen/apple/keeper_iOS/keeper_iOS.entitlements` — the checked-in mirror Xcode
//!   actually signs with.
//!
//! Both reads are structural (key → associated value), not naive substring scans:
//! the correct value *contains* the banned `NSFileProtectionComplete` as a prefix,
//! so only an exact-equality check on the extracted value is meaningful.

use std::path::PathBuf;

/// The one allowed protection class (readable after first unlock, so the resumed
/// sync loop keeps working while the device is locked).
const EXPECTED: &str = "NSFileProtectionCompleteUntilFirstUserAuthentication";

/// The banned class: files unreadable while locked → a resumed sync loop stalls.
const BANNED: &str = "NSFileProtectionComplete";

/// The entitlement key carrying the app-wide default protection class.
const KEY: &str = "com.apple.developer.default-data-protection";

fn gen_apple_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("gen/apple")
}

/// Extract the `<string>` value associated with `key` in a plist `<dict>`, by
/// walking `<key>`/value pairs in document order (structural: the value returned
/// is the element that immediately follows the matching `<key>`), tolerating
/// whitespace and XML comments between them.
fn plist_string_for_key(plist: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let after_key = &plist[plist.find(&key_tag)? + key_tag.len()..];
    // The associated value is the *next* element after the key. Skip whitespace
    // and comments, then require it to open with <string>.
    let mut rest = after_key.trim_start();
    while let Some(stripped) = rest.strip_prefix("<!--") {
        rest = stripped.split_once("-->")?.1.trim_start();
    }
    let value = rest.strip_prefix("<string>")?;
    let (value, _) = value.split_once("</string>")?;
    Some(value.to_owned())
}

/// Extract the scalar value of `key:` from a YAML mapping by locating the key's
/// line and taking what follows the colon (structural: the value is bound to that
/// exact key line, not found anywhere in the file).
fn yaml_scalar_for_key(yaml: &str, key: &str) -> Option<String> {
    yaml.lines().find_map(|line| {
        let trimmed = line.trim_start();
        let value = trimmed
            .strip_prefix(&format!("{key}:"))
            .or_else(|| trimmed.strip_prefix(&format!("\"{key}\":")))?;
        let value = value.trim();
        Some(value.trim_matches('"').trim_matches('\'').to_owned())
    })
}

/// The checked-in entitlements mirror carries exactly the allowed protection class.
#[test]
fn entitlements_file_pins_complete_until_first_user_authentication() {
    let path = gen_apple_dir().join("keeper_iOS/keeper_iOS.entitlements");
    let plist =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let value = plist_string_for_key(&plist, KEY).unwrap_or_else(|| {
        panic!(
            "{} must carry a <string> value for {KEY} — if it reverted to <dict/>, \
             `xcodegen generate` ran against a project.yml missing entitlements.properties",
            path.display()
        )
    });
    assert_eq!(
        value, EXPECTED,
        "the default-data-protection class must be exactly {EXPECTED}"
    );
    // Substring-safe ban: the exact banned literal must never appear as a value.
    // (EXPECTED itself contains BANNED as a prefix, so this checks the full
    // `<string>` element, not a raw substring of the file.)
    assert_ne!(value, BANNED, "NSFileProtectionComplete is banned");
    assert!(
        !plist.contains(&format!("<string>{BANNED}</string>")),
        "no entitlement value may be the banned bare {BANNED}"
    );
}

/// The XcodeGen source of truth (`project.yml` `entitlements.properties`) carries
/// the same value — otherwise the next `xcodegen generate` would silently revert
/// the checked-in entitlements file.
#[test]
fn project_yml_source_of_truth_pins_the_same_value() {
    let path = gen_apple_dir().join("project.yml");
    let yaml =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let value = yaml_scalar_for_key(&yaml, KEY).unwrap_or_else(|| {
        panic!(
            "{} must set {KEY} under entitlements.properties (the source XcodeGen \
             regenerates keeper_iOS.entitlements from)",
            path.display()
        )
    });
    assert_eq!(
        value, EXPECTED,
        "project.yml must pin exactly {EXPECTED} (never the banned {BANNED})"
    );
    assert_ne!(value, BANNED);
}

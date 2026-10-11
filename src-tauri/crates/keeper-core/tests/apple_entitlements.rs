//! The iOS targets' entitlements, per target, from a crate that builds on Linux
//! (Story 14.7, FR-65; Epic 98, story 98.1 #5).
//!
//! These lived in the shell crate's `tests/`, which never builds on the Linux
//! dev host (AGENTS.md), so they ran only on a Mac. They read two text files,
//! so they belong here, reading the shell crate's `gen/apple` by its sibling
//! path from `CARGO_MANIFEST_DIR` (the `voice_on_device` pattern).
//!
//! Two files say the same thing for every target that has entitlements:
//! - `gen/apple/project.yml` — the source XcodeGen regenerates the entitlements
//!   file from (`targets.<name>.entitlements.properties`); a value living only
//!   in the entitlements file silently reverts on the next `xcodegen generate`.
//! - the target's `.entitlements` file (`entitlements.path`) — the mirror Xcode
//!   signs with.
//!
//! What they must say:
//! - **Readable after first unlock, on every target.** The default data
//!   protection is exactly `NSFileProtectionCompleteUntilFirstUserAuthentication`,
//!   never `NSFileProtectionComplete` (the SQLite WAL would lock with the screen
//!   and stall the resumed sync loop — and the notification extension, which
//!   opens the same store while the phone is locked, would read nothing).
//! - **Shared, by the app and its notification extension only.** `keeper_iOS`
//!   and `KeeperNotify` (Epic 98) name the same App Group and the same keychain
//!   access group, so the extension opens the app's store with the app's
//!   session. Every other target that signs with entitlements names neither:
//!   the store's reach is widened to the targets that read it, never to one
//!   that merely needs an entitlement of its own. The Live Activity extension
//!   declares no entitlements block at all, which `scripts/install-ios.sh`'s
//!   free-team strip relies on.
//! - **Push, on the app.** `aps-environment` on `keeper_iOS`, and on no
//!   extension: the notification extension (`KeeperNotify`) is handed the
//!   push, it does not register for one.
//!
//! Both readers are structural (key → value), never substring scans: the right
//! protection class *contains* the banned one as a prefix.

use std::collections::BTreeMap;
use std::path::PathBuf;

const PROTECTION: &str = "com.apple.developer.default-data-protection";
const EXPECTED_PROTECTION: &str = "NSFileProtectionCompleteUntilFirstUserAuthentication";
const BANNED_PROTECTION: &str = "NSFileProtectionComplete";

const APP_GROUPS: &str = "com.apple.security.application-groups";
const APP_GROUP: &str = "group.dev.tgorka.keeper";
const KEYCHAIN_GROUPS: &str = "keychain-access-groups";
/// The app's own application identifier, so the items written before the
/// entitlement existed are already in the group (`IosPlatform::keychain_set`).
const KEYCHAIN_GROUP: &str = "$(AppIdentifierPrefix)dev.tgorka.keeper";
const APS: &str = "aps-environment";

const APP_TARGET: &str = "keeper_iOS";
/// The notification extension (98.1): it opens the app's store.
const NOTIFY_TARGET: &str = "KeeperNotify";
/// The targets that open the app's store with the app's session: the app and
/// its notification extension (Epic 98, story 98.1).
const SHARING_TARGETS: [&str; 2] = [APP_TARGET, NOTIFY_TARGET];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Text(String),
    List(Vec<String>),
}

#[derive(Debug)]
struct Target {
    name: String,
    /// `entitlements.path`, relative to `gen/apple`.
    path: String,
    /// `entitlements.properties`.
    properties: BTreeMap<String, Value>,
}

fn gen_apple() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../keeper/gen/apple")
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').trim_matches('\'').to_owned()
}

/// A YAML value: a flow list `[a, b]`, a block list on the following deeper
/// lines (`- a`), or a scalar.
fn yaml_value(inline: &str, following: &[&str], key_indent: usize) -> Value {
    let inline = inline.trim();
    if let Some(list) = inline.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Value::List(
            list.split(',')
                .map(unquote)
                .filter(|item| !item.is_empty())
                .collect(),
        );
    }
    if inline.is_empty() {
        return Value::List(
            following
                .iter()
                .take_while(|line| line.trim().is_empty() || indent(line) > key_indent)
                .filter_map(|line| line.trim().strip_prefix("- ").map(unquote))
                .collect(),
        );
    }
    Value::Text(unquote(inline))
}

type Partial = (String, Option<String>, BTreeMap<String, Value>);

fn finish(current: Option<Partial>, out: &mut Vec<Target>) {
    if let Some((name, Some(path), properties)) = current {
        out.push(Target {
            name,
            path,
            properties,
        });
    }
}

/// Every target under `targets:` that has an `entitlements:` block.
fn targets_with_entitlements(yaml: &str) -> Vec<Target> {
    let lines: Vec<&str> = yaml.lines().collect();
    let start = lines
        .iter()
        .position(|line| *line == "targets:")
        .expect("project.yml has a top-level `targets:`");
    let mut out = Vec::new();
    let mut current: Option<Partial> = None;
    // (indent of `entitlements:`, inside `properties:` at this indent)
    let mut block: Option<(usize, Option<usize>)> = None;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let depth = indent(line);
        if depth == 0 {
            break;
        }
        if depth == 2 {
            finish(current.take(), &mut out);
            block = None;
            current = trimmed
                .strip_suffix(':')
                .map(|name| (name.to_owned(), None, BTreeMap::new()));
            continue;
        }
        let Some((_, path, properties)) = current.as_mut() else {
            continue;
        };
        if trimmed == "entitlements:" {
            block = Some((depth, None));
            continue;
        }
        let Some((block_depth, props)) = block.as_mut() else {
            continue;
        };
        if depth <= *block_depth {
            block = None;
            continue;
        }
        match props {
            None if trimmed == "properties:" => *props = Some(depth),
            None => {
                if let Some(value) = trimmed.strip_prefix("path:") {
                    *path = Some(unquote(value));
                }
            }
            Some(props_depth) if depth <= *props_depth => {
                *props = None;
                if let Some(value) = trimmed.strip_prefix("path:") {
                    *path = Some(unquote(value));
                }
            }
            Some(props_depth) if depth == *props_depth + 2 => {
                if let Some((key, value)) = trimmed.split_once(':') {
                    properties.insert(unquote(key), yaml_value(value, &lines[i + 1..], depth));
                }
            }
            Some(_) => {}
        }
    }
    finish(current, &mut out);
    out
}

/// The top-level `<dict>` of a plist as key → value, skipping comments.
fn plist_dict(xml: &str) -> BTreeMap<String, Value> {
    let mut text = String::new();
    let mut rest = xml;
    while let Some((before, after)) = rest.split_once("<!--") {
        text.push_str(before);
        rest = after.split_once("-->").map_or("", |(_, after)| after);
    }
    text.push_str(rest);

    let mut out = BTreeMap::new();
    let mut rest = text.as_str();
    while let Some((_, after)) = rest.split_once("<key>") {
        let (key, after) = after.split_once("</key>").expect("a closed <key>");
        let after = after.trim_start();
        let (value, after) = if let Some(after) = after.strip_prefix("<string>") {
            let (value, after) = after.split_once("</string>").expect("a closed <string>");
            (Value::Text(value.to_owned()), after)
        } else if let Some(after) = after.strip_prefix("<array>") {
            let (items, after) = after.split_once("</array>").expect("a closed <array>");
            let items = items
                .split("<string>")
                .skip(1)
                .map(|item| {
                    item.split_once("</string>")
                        .expect("a closed <string>")
                        .0
                        .to_owned()
                })
                .collect();
            (Value::List(items), after)
        } else if let Some(after) = after.strip_prefix("<true/>") {
            (Value::Text("true".to_owned()), after)
        } else if let Some(after) = after.strip_prefix("<false/>") {
            (Value::Text("false".to_owned()), after)
        } else {
            panic!("the value of <key>{key}</key> is neither a string, a list nor a boolean");
        };
        out.insert(key.to_owned(), value);
        rest = after;
    }
    out
}

fn targets() -> Vec<Target> {
    let targets = targets_with_entitlements(&read(&gen_apple().join("project.yml")));
    assert!(
        targets.iter().any(|t| t.name == APP_TARGET),
        "{APP_TARGET} must declare entitlements in project.yml, found {:?}",
        targets.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    targets
}

/// The file Xcode signs with says exactly what project.yml says, target by
/// target — otherwise the next `xcodegen generate` changes what ships.
#[test]
fn every_targets_entitlements_file_says_what_project_yml_says() {
    for target in targets() {
        let file = gen_apple().join(&target.path);
        assert_eq!(
            plist_dict(&read(&file)),
            target.properties,
            "{} ({}) must carry exactly project.yml's entitlements for {}",
            target.name,
            file.display(),
            target.name
        );
    }
}

/// Every target's store is readable after first unlock, never only while
/// unlocked.
#[test]
fn every_target_pins_data_protection_to_after_first_unlock() {
    for target in targets() {
        let value = target.properties.get(PROTECTION);
        assert_eq!(
            value,
            Some(&Value::Text(EXPECTED_PROTECTION.to_owned())),
            "{} must pin {PROTECTION} to {EXPECTED_PROTECTION} (never {BANNED_PROTECTION})",
            target.name
        );
    }
}

/// The app and its notification extension name one App Group and one keychain
/// group, so the extension opens the app's store with the app's session; every
/// other target names neither; the app also carries the push environment.
#[test]
fn only_the_app_and_its_notification_extension_share_the_store() {
    let targets = targets();
    for target in &targets {
        let shares = SHARING_TARGETS.contains(&target.name.as_str());
        let only = |group: &str| shares.then(|| Value::List(vec![group.to_owned()]));
        assert_eq!(
            target.properties.get(APP_GROUPS).cloned(),
            only(APP_GROUP),
            "{}: {APP_GROUPS} must be exactly [{APP_GROUP}] on {SHARING_TARGETS:?} and absent elsewhere",
            target.name
        );
        assert_eq!(
            target.properties.get(KEYCHAIN_GROUPS).cloned(),
            only(KEYCHAIN_GROUP),
            "{}: {KEYCHAIN_GROUPS} must be exactly [{KEYCHAIN_GROUP}] on {SHARING_TARGETS:?} and absent elsewhere",
            target.name
        );
    }
    let app = targets
        .iter()
        .find(|t| t.name == APP_TARGET)
        .expect("the app target");
    assert_eq!(
        app.properties.get(APS),
        Some(&Value::Text("development".to_owned())),
        "{APP_TARGET} must carry {APS} for APNs to issue it a token"
    );
}

/// The notification extension opens the app's store with the app's session
/// (98.1 #5), so it signs with entitlements — which puts it under the two
/// tests above — and takes no push environment of its own.
#[test]
fn the_notification_extension_shares_the_store_and_takes_no_push() {
    let targets = targets();
    let notify = targets
        .iter()
        .find(|t| t.name == NOTIFY_TARGET)
        .unwrap_or_else(|| panic!("{NOTIFY_TARGET} must declare entitlements in project.yml"));
    assert_eq!(
        notify.properties.get(APP_GROUPS),
        Some(&Value::List(vec![APP_GROUP.to_owned()]))
    );
    assert_eq!(
        notify.properties.get(APS),
        None,
        "{NOTIFY_TARGET} carries no {APS}"
    );
}

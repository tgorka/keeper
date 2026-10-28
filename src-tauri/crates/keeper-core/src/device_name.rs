//! This device's name (R-HQ-94, under R-HQ-93): the host every account and
//! server hears from this device.
//!
//! One answer, decided here: the name a person set in Settings when there is
//! one, else what the system calls the device — the short host name on a
//! computer, `UIDevice.name` on a phone, which iOS 16+ answers with the bare
//! model ("iPhone") without an entitlement a free team cannot get. The shell's
//! `account_ipc::device_host` is this module's only caller for the effective
//! name; the agents server's pairing and sign-in, and the default of the
//! device's name in the account's config repository, read it there.
//!
//! The name is checked with the agents server's own naming rule — the one
//! 109.1 checks a host with before it is sent ([`host_rule`]) — never a
//! second grammar. It is stored on this install only (`device.name`, session
//! state: no file sets it and no account sync carries it).

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::registry;

/// The longest host the agents server takes, in UTF-16 code units — what
/// JavaScript's `length` counts in keeper-hq's `device-name.ts`.
pub const MAX_HOST_UNITS: usize = 200;

/// Where the effective name comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DeviceNameSource {
    /// What the system calls this device.
    System,
    /// A name the person set in Settings.
    You,
}

/// Settings › This device's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceNameVm {
    /// The name every consumer uses; empty only while the system has not
    /// said and nobody set one.
    pub name: String,
    pub source: DeviceNameSource,
    /// What the system calls this device: what Reset goes back to.
    pub system: String,
    /// The line under the field: where the name came from.
    pub sentence: String,
}

/// Why a name is refused. The sentence is the one Settings shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameRefusal {
    #[error("A device's name cannot be blank. Type one, or Reset to use the system's.")]
    Blank,
    #[error(
        "A device's name is at most {MAX_HOST_UNITS} characters; this one has {0}. Shorten it."
    )]
    TooLong(usize),
    #[error(
        "{0} is already the name of another of your devices on your agents server. Give this \
         one a name of its own."
    )]
    Taken(String),
}

/// Why a name could not be set: refused, or not stored.
#[derive(Debug, thiserror::Error)]
pub enum DeviceNameError {
    #[error(transparent)]
    Refused(#[from] NameRefusal),
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// The agents server's rule for a device's host (keeper-hq
/// `server/src/device-name.ts`, `deviceNaming`; 109.1): trimmed, then 1 to
/// [`MAX_HOST_UNITS`] UTF-16 code units. The host as it is sent.
pub fn host_rule(raw: &str) -> Result<&str, NameRefusal> {
    let host = raw.trim();
    if host.is_empty() {
        return Err(NameRefusal::Blank);
    }
    let units = host.encode_utf16().count();
    if units > MAX_HOST_UNITS {
        return Err(NameRefusal::TooLong(units));
    }
    Ok(host)
}

/// A name a person typed, as it would be stored: [`host_rule`], and not the
/// host another of their devices has on the agents server (`taken`). The
/// server compares hosts exactly, so this does too.
pub fn check<'a>(raw: &'a str, taken: &[String]) -> Result<&'a str, NameRefusal> {
    let name = host_rule(raw)?;
    if taken.iter().any(|other| other.trim() == name) {
        return Err(NameRefusal::Taken(name.to_owned()));
    }
    Ok(name)
}

/// The effective name: `stored` — the person's — over `system`. A stored
/// name the rule refuses (a row written by hand) is not used.
pub fn resolve(stored: Option<&str>, system: &str) -> DeviceNameVm {
    let system = system.trim().to_owned();
    match stored.map(host_rule) {
        Some(Ok(name)) => DeviceNameVm {
            name: name.to_owned(),
            source: DeviceNameSource::You,
            sentence: format!(
                "Set by you. The system calls this device {}.",
                shown(&system)
            ),
            system,
        },
        _ => DeviceNameVm {
            name: system.clone(),
            source: DeviceNameSource::System,
            sentence: if system.is_empty() {
                "The system has not named this device yet. Type a name.".to_owned()
            } else {
                "From the system.".to_owned()
            },
            system,
        },
    }
}

fn shown(system: &str) -> &str {
    if system.is_empty() {
        "nothing yet"
    } else {
        system
    }
}

/// This device's name now: the stored one over `system`.
pub fn read(data_dir: &Path, system: &str) -> Result<DeviceNameVm, CoreError> {
    let stored = registry::get_device_name(data_dir)?;
    Ok(resolve(stored.as_deref(), system))
}

/// Store `raw` as this device's name, refused with its sentence unless
/// [`check`] takes it; the effective name after.
pub fn set(
    data_dir: &Path,
    raw: &str,
    system: &str,
    taken: &[String],
) -> Result<DeviceNameVm, DeviceNameError> {
    let name = check(raw, taken)?;
    registry::set_device_name(data_dir, name)?;
    Ok(read(data_dir, system)?)
}

/// Go back to the system's name; the effective name after.
pub fn reset(data_dir: &Path, system: &str) -> Result<DeviceNameVm, CoreError> {
    registry::clear_device_name(data_dir)?;
    read(data_dir, system)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("a data directory")
    }

    /// R-HQ-94: the person's name wins over the system's, and says so;
    /// Reset brings the system's back and forgets the row.
    #[test]
    fn a_name_set_here_wins_over_the_systems_and_reset_brings_the_system_back() {
        let tmp = data_dir();
        let dir = tmp.path();
        let before = read(dir, "iPhone").expect("reads");
        assert_eq!(before.name, "iPhone");
        assert_eq!(before.source, DeviceNameSource::System);
        assert_eq!(before.sentence, "From the system.");

        let set = set(dir, "  kalypso ", "iPhone", &[]).expect("sets");
        assert_eq!(
            set.name, "kalypso",
            "stored trimmed, as the server takes it"
        );
        assert_eq!(set.source, DeviceNameSource::You);
        assert_eq!(set.system, "iPhone");
        assert_eq!(
            set.sentence,
            "Set by you. The system calls this device iPhone."
        );
        assert_eq!(read(dir, "iPhone").expect("reads"), set);
        assert_eq!(
            registry::get_device_name(dir).expect("row").as_deref(),
            Some("kalypso")
        );

        let reset = reset(dir, "iPhone").expect("resets");
        assert_eq!(reset, before);
        assert_eq!(registry::get_device_name(dir).expect("row"), None);
    }

    /// The server's rule, and nothing of a second grammar: blank and over
    /// 200 UTF-16 units are refused with Rust's sentence and nothing is
    /// stored; a space inside a name is the server's to take, and is taken.
    #[test]
    fn the_servers_host_rule_refuses_blank_and_overlong_names_and_stores_nothing() {
        let tmp = data_dir();
        let dir = tmp.path();
        let refused = |raw: &str| match set(dir, raw, "hesperia", &[]) {
            Err(DeviceNameError::Refused(refusal)) => refusal,
            other => panic!("{raw:?} was not refused: {other:?}"),
        };
        assert_eq!(refused("   "), NameRefusal::Blank);
        assert_eq!(
            refused("   ").to_string(),
            "A device's name cannot be blank. Type one, or Reset to use the system's."
        );
        // 101 characters of two UTF-16 units each: 202 units, as JS counts.
        let wide = "😀".repeat(101);
        assert_eq!(refused(&wide), NameRefusal::TooLong(202));
        assert_eq!(registry::get_device_name(dir).expect("row"), None);

        let longest = "a".repeat(200);
        assert_eq!(host_rule(&longest), Ok(longest.as_str()));
        assert_eq!(host_rule("kal ypso"), Ok("kal ypso"));
    }

    /// A name another of the person's devices has on the server is refused,
    /// exactly as the server compares hosts.
    #[test]
    fn a_name_another_of_your_devices_has_is_refused() {
        let taken = ["hesperia".to_owned()];
        assert_eq!(
            check(" hesperia ", &taken),
            Err(NameRefusal::Taken("hesperia".to_owned()))
        );
        assert_eq!(
            NameRefusal::Taken("hesperia".to_owned()).to_string(),
            "hesperia is already the name of another of your devices on your agents server. \
             Give this one a name of its own."
        );
        assert_eq!(check("Hesperia", &taken), Ok("Hesperia"));
        assert_eq!(check("kalypso", &taken), Ok("kalypso"));
    }

    /// A stored row the rule refuses is not a name: the system's is used.
    #[test]
    fn a_stored_name_the_rule_refuses_is_not_used() {
        let vm = resolve(Some("   "), "hesperia");
        assert_eq!(vm.name, "hesperia");
        assert_eq!(vm.source, DeviceNameSource::System);
        let none = resolve(None, "");
        assert_eq!(none.name, "");
        assert_eq!(
            none.sentence,
            "The system has not named this device yet. Type a name."
        );
    }
}

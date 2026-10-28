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
use crate::org_account::layout::device_slug;
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
    /// The device is the host of one of the person's agent instances: the
    /// server's T4 other-host rule tells the two apart by host, so a rename
    /// would let this device approve its own agent's T4 request (R607-01).
    #[error("This device runs {agent}; its name stays {host}.")]
    Runs { agent: String, host: String },
    /// The agents server's directory is not on this device yet, so whether
    /// it runs an agent cannot be told.
    #[error(
        "keeper has not read your agents server's list of devices and agents on this device \
         yet, so it cannot tell whether this device runs one of your agents. Its name stays {0} \
         until it has; try again once it is connected."
    )]
    Unread(String),
    /// This device holds a credential the agents server can move to a new
    /// name only by adopting it under the same client id — through the
    /// account's sign-in — and no sign-in can do that here. A new pairing or
    /// a fresh sign-in would be a new device with a new client id (keeper-hq
    /// R69-02), so keeper never falls back to one for a name.
    #[error(
        "{server} knows this device by the name it was paired with, and only your account's \
         sign-in can give it a new one there; pairing it again would make it a different device. \
         Its name stays {current}."
    )]
    Pinned { server: String, current: String },
}

/// What the agents server's directory, as this device's replicas hold it,
/// says about a rename (R-HQ-94 and its amendment).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Directory {
    /// The hosts the person's other live devices have.
    pub taken: Vec<String>,
    /// Every live agent instance in the person's databases: its agent and
    /// the host it runs on.
    pub instances: Vec<(String, String)>,
    /// The host the server bound this device's credential to, when keeper
    /// recorded it.
    pub bound: Option<String>,
    /// The agents server that holds this device's credential, when nothing
    /// can adopt it there under a new name: no account is set up, or the
    /// server offers no sign-in or refused the account's.
    pub pinned: Option<String>,
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

/// Whether this device may stop being called `current` (R-HQ-94 amendment,
/// R607-01): not while it is the host of one of the person's agent
/// instances — by the name it has, or by the host the server bound its
/// credential to — not while the directory that would say so is unread
/// (`None`), and not where the server could learn the new name only from a
/// new device ([`NameRefusal::Pinned`]).
pub fn may_leave<'d>(
    current: &str,
    directory: Option<&'d Directory>,
) -> Result<&'d Directory, NameRefusal> {
    let current = current.trim();
    let Some(directory) = directory else {
        return Err(NameRefusal::Unread(current.to_owned()));
    };
    let mine = |host: &str| host == current || directory.bound.as_deref() == Some(host);
    if let Some((agent, host)) = directory.instances.iter().find(|(_, host)| mine(host)) {
        return Err(NameRefusal::Runs {
            agent: agent.clone(),
            host: host.clone(),
        });
    }
    if let Some(server) = &directory.pinned {
        return Err(NameRefusal::Pinned {
            server: server.clone(),
            current: current.to_owned(),
        });
    }
    Ok(directory)
}

/// A name a person typed for a device now called `current`, as it would be
/// stored: [`host_rule`]; then, unless it is the name the device already
/// has, [`may_leave`] and not the host another of the person's devices has
/// on the agents server. The server compares hosts exactly, so this does
/// too.
pub fn check<'a>(
    raw: &'a str,
    current: &str,
    directory: Option<&Directory>,
) -> Result<&'a str, NameRefusal> {
    let name = host_rule(raw)?;
    if name == current.trim() {
        return Ok(name);
    }
    let directory = may_leave(current, directory)?;
    if directory.taken.iter().any(|other| other.trim() == name) {
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

/// What this device's name becomes if `raw` is saved: refused with its
/// sentence unless [`check`] takes it. Nothing is written.
pub fn plan_set(
    data_dir: &Path,
    raw: &str,
    system: &str,
    directory: Option<&Directory>,
) -> Result<DeviceNameVm, DeviceNameError> {
    let current = read(data_dir, system)?;
    let name = check(raw, &current.name, directory)?;
    Ok(resolve(Some(name), system))
}

/// What this device's name becomes at Reset — the system's — refused by
/// [`may_leave`] when that is another name. Nothing is written.
pub fn plan_reset(
    data_dir: &Path,
    system: &str,
    directory: Option<&Directory>,
) -> Result<DeviceNameVm, DeviceNameError> {
    let current = read(data_dir, system)?;
    let after = resolve(None, system);
    if after.name != current.name {
        may_leave(&current.name, directory)?;
    }
    Ok(after)
}

/// Keep what [`plan_set`] or [`plan_reset`] answered: a name you set is
/// stored, the system's forgets the stored one.
pub fn store(data_dir: &Path, after: &DeviceNameVm) -> Result<(), CoreError> {
    match after.source {
        DeviceNameSource::You => registry::set_device_name(data_dir, &after.name),
        DeviceNameSource::System => registry::clear_device_name(data_dir),
    }
}

/// The name this device's record in the account's repository moves to once
/// its name is `after` (R607-02), or `None` when it already has it: the
/// slug of a name you set; after a Reset, `system_default` — the name a first
/// registration would take from the system — unless the record already has
/// a system-made name (`hesperia`, `iphone-3f2a`). `registered` is the
/// record's name, `None` while the device is not registered: its first
/// registration then takes the answer.
pub fn account_device_after(
    after: &DeviceNameVm,
    registered: Option<&str>,
    system_default: impl FnOnce() -> String,
) -> Option<String> {
    let to = match after.source {
        DeviceNameSource::You => device_slug(&after.name),
        DeviceNameSource::System => {
            let plain = device_slug(&after.system);
            let system_made = |slug: &str| {
                slug == plain
                    || slug
                        .strip_prefix(&plain)
                        .and_then(|s| s.strip_prefix('-'))
                        .is_some_and(|suffix| {
                            suffix.len() == 4
                                && suffix
                                    .bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        })
            };
            if registered.is_some_and(system_made) {
                return None;
            }
            system_default()
        }
    };
    (registered != Some(to.as_str())).then_some(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("a data directory")
    }

    /// Nothing on a server knows this device: nothing to protect.
    fn unpaired() -> Directory {
        Directory::default()
    }

    /// Ana's databases as a Mac named hesperia reads them: her phone is
    /// kalypso, and Nixi runs here.
    fn hesperia_runs_nixi() -> Directory {
        Directory {
            taken: vec!["kalypso".to_owned()],
            instances: vec![
                ("dixi".to_owned(), "electra".to_owned()),
                ("nixi".to_owned(), "hesperia".to_owned()),
            ],
            bound: Some("hesperia".to_owned()),
            pinned: None,
        }
    }

    fn set(dir: &Path, raw: &str, system: &str, directory: &Directory) -> DeviceNameVm {
        let after = plan_set(dir, raw, system, Some(directory)).expect("planned");
        store(dir, &after).expect("stored");
        after
    }

    fn refused(result: Result<DeviceNameVm, DeviceNameError>) -> NameRefusal {
        match result {
            Err(DeviceNameError::Refused(refusal)) => refusal,
            other => panic!("not refused: {other:?}"),
        }
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

        let set = set(dir, "  kalypso ", "iPhone", &unpaired());
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

        let reset = plan_reset(dir, "iPhone", Some(&unpaired())).expect("planned");
        assert_eq!(
            registry::get_device_name(dir).expect("row").as_deref(),
            Some("kalypso"),
            "a plan writes nothing"
        );
        store(dir, &reset).expect("stored");
        assert_eq!(reset, before);
        assert_eq!(read(dir, "iPhone").expect("reads"), before);
        assert_eq!(registry::get_device_name(dir).expect("row"), None);
    }

    /// The server's rule, and nothing of a second grammar: blank and over
    /// 200 UTF-16 units are refused with Rust's sentence and nothing is
    /// stored; a space inside a name is the server's to take, and is taken.
    #[test]
    fn the_servers_host_rule_refuses_blank_and_overlong_names_and_stores_nothing() {
        let tmp = data_dir();
        let dir = tmp.path();
        let refused = |raw: &str| refused(plan_set(dir, raw, "hesperia", Some(&unpaired())));
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
        let directory = Directory {
            taken: vec!["hesperia".to_owned()],
            ..Directory::default()
        };
        assert_eq!(
            check(" hesperia ", "iPhone", Some(&directory)),
            Err(NameRefusal::Taken("hesperia".to_owned()))
        );
        assert_eq!(
            NameRefusal::Taken("hesperia".to_owned()).to_string(),
            "hesperia is already the name of another of your devices on your agents server. \
             Give this one a name of its own."
        );
        assert_eq!(
            check("Hesperia", "iPhone", Some(&directory)),
            Ok("Hesperia")
        );
        assert_eq!(check("kalypso", "iPhone", Some(&directory)), Ok("kalypso"));
    }

    /// R607-01: a device that is the host of one of the person's agent
    /// instances keeps its name — by the name it has or by the host its
    /// credential was bound to — and Reset is held the same way; renaming
    /// TO an instance's host (fails closed at T4) and a device that runs
    /// none are free.
    #[test]
    fn a_device_that_runs_an_agent_keeps_its_name() {
        let directory = hesperia_runs_nixi();
        let runs = NameRefusal::Runs {
            agent: "nixi".to_owned(),
            host: "hesperia".to_owned(),
        };
        assert_eq!(
            check("studio", "hesperia", Some(&directory)),
            Err(runs.clone())
        );
        assert_eq!(
            runs.to_string(),
            "This device runs nixi; its name stays hesperia."
        );
        // Saving the name it has changes nothing on the server.
        assert_eq!(
            check("hesperia", "hesperia", Some(&directory)),
            Ok("hesperia")
        );
        // Already called something else here, still bound to hesperia there.
        assert_eq!(check("den", "studio", Some(&directory)), Err(runs.clone()));

        let tmp = data_dir();
        let dir = tmp.path();
        registry::set_device_name(dir, "hesperia").expect("row");
        assert_eq!(
            refused(plan_reset(dir, "macbook", Some(&directory))),
            runs,
            "Reset is a rename too"
        );
        assert_eq!(
            refused(plan_set(dir, "studio", "macbook", Some(&directory))),
            runs
        );
        assert_eq!(
            registry::get_device_name(dir).expect("row").as_deref(),
            Some("hesperia")
        );

        // The phone, whose other device is the Mac.
        let phone = Directory {
            taken: vec!["hesperia".to_owned()],
            bound: Some("iPhone".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert_eq!(check("kalypso", "iPhone", Some(&phone)), Ok("kalypso"));
        assert_eq!(check("electra", "iPhone", Some(&phone)), Ok("electra"));
    }

    /// A directory not read yet is an honest refusal, never a guess; the
    /// name it already has can still be saved.
    #[test]
    fn an_unread_directory_refuses_a_rename_honestly() {
        assert_eq!(
            check("studio", "hesperia", None),
            Err(NameRefusal::Unread("hesperia".to_owned()))
        );
        assert_eq!(
            NameRefusal::Unread("hesperia".to_owned()).to_string(),
            "keeper has not read your agents server's list of devices and agents on this \
             device yet, so it cannot tell whether this device runs one of your agents. Its \
             name stays hesperia until it has; try again once it is connected."
        );
        assert_eq!(check("hesperia", "hesperia", None), Ok("hesperia"));
        let tmp = data_dir();
        registry::set_device_name(tmp.path(), "studio").expect("row");
        assert_eq!(
            refused(plan_reset(tmp.path(), "hesperia", None)),
            NameRefusal::Unread("studio".to_owned())
        );
    }

    /// keeper-hq R69-02: where only a new pairing or a fresh sign-in could
    /// tell the server a new name, that would be a new device, so the name
    /// stays; it is the device's own to keep, and another reason comes first.
    #[test]
    fn a_device_only_a_new_device_could_rename_keeps_its_name() {
        let pinned = Directory {
            pinned: Some("agents.example.org".to_owned()),
            ..Directory::default()
        };
        let refusal = NameRefusal::Pinned {
            server: "agents.example.org".to_owned(),
            current: "iPhone".to_owned(),
        };
        assert_eq!(
            check("kalypso", "iPhone", Some(&pinned)),
            Err(refusal.clone())
        );
        assert_eq!(
            refusal.to_string(),
            "agents.example.org knows this device by the name it was paired with, and only your \
             account's sign-in can give it a new one there; pairing it again would make it a \
             different device. Its name stays iPhone."
        );
        assert_eq!(check("iPhone", "iPhone", Some(&pinned)), Ok("iPhone"));
        let both = Directory {
            pinned: Some("agents.example.org".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert!(matches!(
            check("studio", "hesperia", Some(&both)),
            Err(NameRefusal::Runs { .. })
        ));
    }

    /// The repository a rename plans against, as files by path.
    struct Repo(std::collections::BTreeMap<String, Vec<u8>>);

    impl crate::org_account::layout::RepoFiles for Repo {
        fn read(&self, rel: &str) -> Option<Vec<u8>> {
            self.0.get(rel).cloned()
        }
        fn list_dir(&self, rel: &str) -> Vec<String> {
            let prefix = format!("{rel}/");
            self.0
                .keys()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_owned)
                .collect()
        }
    }

    /// R607-02: a phone registered as `iphone-1234` that is named kalypso
    /// moves its record and every file of its own to `kalypso` through the
    /// account's rename; Reset moves it back to a system-made name, and a
    /// record that already has one, or the name, stays where it is.
    #[test]
    fn naming_a_registered_device_renames_its_account_record_and_files() {
        use crate::org_account::layout::{plan_rename, RenameOp};
        let kalypso = resolve(Some("kalypso"), "iPhone");
        let to = account_device_after(&kalypso, Some("iphone-1234"), || unreachable!())
            .expect("the record follows the name");
        assert_eq!(to, "kalypso");
        let repo = Repo(
            [
                "tgorka/devices/iphone-1234.toml",
                "tgorka/keeper.iphone-1234.toml",
                "tgorka/settings.iphone-1234.toml",
                "tgorka/device.iphone-1234.toml",
                "tgorka/logs/iphone-1234.log",
                "tgorka/devices/hesperia.toml",
            ]
            .into_iter()
            .map(|path| (path.to_owned(), Vec::new()))
            .collect(),
        );
        let moved: Vec<(String, String)> = plan_rename(&repo, "tgorka", "iphone-1234", &to)
            .expect("planned")
            .into_iter()
            .map(|RenameOp { from, to }| (from, to))
            .collect();
        let pair = |a: &str, b: &str| (format!("tgorka/{a}"), format!("tgorka/{b}"));
        assert_eq!(
            moved,
            [
                pair("devices/iphone-1234.toml", "devices/kalypso.toml"),
                pair("keeper.iphone-1234.toml", "keeper.kalypso.toml"),
                pair("settings.iphone-1234.toml", "settings.kalypso.toml"),
                pair("device.iphone-1234.toml", "device.kalypso.toml"),
                pair("logs/iphone-1234.log", "logs/kalypso.log"),
            ]
        );

        // Already registered under the name: nothing moves.
        assert_eq!(
            account_device_after(&kalypso, Some("kalypso"), || unreachable!()),
            None
        );
        // Not registered: the first registration takes the name.
        assert_eq!(
            account_device_after(&kalypso, None, || unreachable!()).as_deref(),
            Some("kalypso")
        );
        // Reset: back to what the system would name it, unless it has that.
        let system = resolve(None, "iPhone");
        assert_eq!(
            account_device_after(&system, Some("kalypso"), || "iphone-9a0b".to_owned()).as_deref(),
            Some("iphone-9a0b")
        );
        assert_eq!(
            account_device_after(&system, Some("iphone-1234"), || unreachable!()),
            None
        );
        let mac = resolve(None, "hesperia");
        assert_eq!(
            account_device_after(&mac, Some("hesperia"), || unreachable!()),
            None
        );
        assert_eq!(
            account_device_after(&mac, Some("studio"), || "hesperia".to_owned()).as_deref(),
            Some("hesperia")
        );
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

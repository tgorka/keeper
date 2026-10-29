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
    /// The agents server's directory is not complete on this device yet, so
    /// whether it runs an agent cannot be told (R-HQ-94 amendment 2 (5)).
    #[error("{}", unread_sentence(current, database.as_deref()))]
    Unread {
        current: String,
        /// The database not held, or whose client list has not synced;
        /// `None` when the agents service has not read anything yet.
        database: Option<String>,
    },
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
    #[error(
        "keeper has no record of this credential's original server host, and the system's \
         current name cannot recover it safely. Its name stays {current}; the existing \
         credential is kept."
    )]
    UnknownHost { current: String },
}

/// What the agents server's directory, as this device's replicas hold it,
/// says about a rename (R-HQ-94 and its amendments).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Directory {
    /// The hosts the person's other live devices have.
    pub taken: Vec<String>,
    /// Every live agent instance in the person's databases: its agent and
    /// the host it runs on.
    pub instances: Vec<(String, String)>,
    /// The host the server last heard from this device's credential, when
    /// keeper recorded it.
    pub bound: Option<String>,
    /// The host this device's credential was created with — the one the
    /// server's T4 rule compares (`t4_host`, amendment 2 (6)).
    pub enrolled: Option<String>,
    /// The agents server that holds this device's credential, when nothing
    /// can adopt it there under a new name: no account is set up, or the
    /// server offers no sign-in or refused the account's.
    pub pinned: Option<String>,
}

/// Why the directory cannot be read: the database that is not held or has
/// not synced its client list, or `None` when nothing is read yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unread {
    pub database: Option<String>,
    /// A legacy credential has no trustworthy recorded host.
    pub host_unknown: bool,
}

/// The directory, or why it cannot be read yet.
pub type DirectoryRead = Result<Directory, Unread>;

fn unread_sentence(current: &str, database: Option<&str>) -> String {
    match database {
        Some(database) => format!(
            "keeper has not read the devices and agents of {database} on this device yet, so it \
             cannot tell whether this device runs one of your agents there. Its name stays \
             {current} until it has; try again once {database} has synced."
        ),
        None => format!(
            "keeper has not read your agents server's list of devices and agents on this \
             device yet, so it cannot tell whether this device runs one of your agents. Its \
             name stays {current} until it has; try again once it is connected."
        ),
    }
}

/// Why a name could not be set: refused, or not stored.
#[derive(Debug, thiserror::Error)]
pub enum DeviceNameError {
    #[error(transparent)]
    Refused(#[from] NameRefusal),
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// Whether `c` is one of the characters JavaScript's `String.prototype.trim`
/// removes — ECMAScript's WhiteSpace and LineTerminator, which the agents
/// server's naming rule trims with (amendment 2 (8)). Not Rust's
/// `char::is_whitespace`: the two differ on U+FEFF (trimmed here, kept by
/// Rust) and U+0085 (kept here, trimmed by Rust).
pub fn is_server_space(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `raw` trimmed as the agents server trims a host ([`is_server_space`]).
pub fn server_trim(raw: &str) -> &str {
    raw.trim_matches(is_server_space)
}

/// The agents server's rule for a device's host (keeper-hq
/// `server/src/device-name.ts`, `deviceNaming`; 109.1): trimmed as the
/// server trims ([`server_trim`]), then 1 to [`MAX_HOST_UNITS`] UTF-16 code
/// units. The host as it is sent.
pub fn host_rule(raw: &str) -> Result<&str, NameRefusal> {
    let host = server_trim(raw);
    if host.is_empty() {
        return Err(NameRefusal::Blank);
    }
    let units = host.encode_utf16().count();
    if units > MAX_HOST_UNITS {
        return Err(NameRefusal::TooLong(units));
    }
    Ok(host)
}

/// Whether this device may stop being called `current` (R-HQ-94
/// amendments): not while it is the host of one of the person's agent
/// instances — by the name it has, the system's name, the host its
/// credential was created with, or the last host the server heard — not
/// while the directory that would say so is incomplete, and not where the
/// server could learn the new name only from a new device
/// ([`NameRefusal::Pinned`]).
pub fn may_leave<'d>(
    current: &str,
    system: &str,
    directory: &'d DirectoryRead,
) -> Result<&'d Directory, NameRefusal> {
    let current = server_trim(current);
    let system = server_trim(system);
    let directory = directory.as_ref().map_err(|unread| {
        if unread.host_unknown {
            NameRefusal::UnknownHost {
                current: current.to_owned(),
            }
        } else {
            NameRefusal::Unread {
                current: current.to_owned(),
                database: unread.database.clone(),
            }
        }
    })?;
    let mine = |host: &str| {
        host == current
            || host == system
            || directory.enrolled.as_deref() == Some(host)
            || directory.bound.as_deref() == Some(host)
    };
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
    system: &str,
    directory: &DirectoryRead,
) -> Result<&'a str, NameRefusal> {
    let name = host_rule(raw)?;
    if name == server_trim(current) {
        return Ok(name);
    }
    let directory = may_leave(current, system, directory)?;
    if directory
        .taken
        .iter()
        .any(|other| server_trim(other) == name)
    {
        return Err(NameRefusal::Taken(name.to_owned()));
    }
    Ok(name)
}

/// The effective name: `stored` — the person's — over `system`. A stored
/// name the rule refuses (a row written by hand) is not used.
pub fn resolve(stored: Option<&str>, system: &str) -> DeviceNameVm {
    let system = server_trim(system).to_owned();
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
    directory: &DirectoryRead,
) -> Result<DeviceNameVm, DeviceNameError> {
    let current = read(data_dir, system)?;
    let name = check(raw, &current.name, system, directory)?;
    Ok(resolve(Some(name), system))
}

/// What this device's name becomes at Reset — the system's — refused by
/// [`check`] just like Save, before any account move or name write.
pub fn plan_reset(
    data_dir: &Path,
    system: &str,
    directory: &DirectoryRead,
) -> Result<DeviceNameVm, DeviceNameError> {
    let current = read(data_dir, system)?;
    let after = resolve(None, system);
    check(&after.name, &current.name, system, directory)?;
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
            enrolled: Some("hesperia".to_owned()),
            pinned: None,
        }
    }

    fn set(dir: &Path, raw: &str, system: &str, directory: &Directory) -> DeviceNameVm {
        let after = plan_set(dir, raw, system, &Ok(directory.clone())).expect("planned");
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

        let reset = plan_reset(dir, "iPhone", &Ok(unpaired())).expect("planned");
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
        let refused = |raw: &str| refused(plan_set(dir, raw, "hesperia", &Ok(unpaired())));
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
            check(" hesperia ", "iPhone", "macbook", &Ok(directory.clone())),
            Err(NameRefusal::Taken("hesperia".to_owned()))
        );
        assert_eq!(
            NameRefusal::Taken("hesperia".to_owned()).to_string(),
            "hesperia is already the name of another of your devices on your agents server. \
             Give this one a name of its own."
        );
        assert_eq!(
            check("Hesperia", "iPhone", "macbook", &Ok(directory.clone())),
            Ok("Hesperia")
        );
        assert_eq!(
            check("kalypso", "iPhone", "macbook", &Ok(directory.clone())),
            Ok("kalypso")
        );
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
            check("studio", "hesperia", "macbook", &Ok(directory.clone())),
            Err(runs.clone())
        );
        assert_eq!(
            runs.to_string(),
            "This device runs nixi; its name stays hesperia."
        );
        // Saving the name it has changes nothing on the server.
        assert_eq!(
            check("hesperia", "hesperia", "macbook", &Ok(directory.clone())),
            Ok("hesperia")
        );
        // Already called something else here, still bound to hesperia there.
        assert_eq!(
            check("den", "studio", "macbook", &Ok(directory.clone())),
            Err(runs.clone())
        );

        let tmp = data_dir();
        let dir = tmp.path();
        registry::set_device_name(dir, "hesperia").expect("row");
        assert_eq!(
            refused(plan_reset(dir, "macbook", &Ok(directory.clone()))),
            runs,
            "Reset is a rename too"
        );
        assert_eq!(
            refused(plan_set(dir, "studio", "macbook", &Ok(directory.clone()))),
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
            enrolled: Some("iPhone".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert_eq!(
            check("kalypso", "iPhone", "macbook", &Ok(phone.clone())),
            Ok("kalypso")
        );
        assert_eq!(
            check("electra", "iPhone", "macbook", &Ok(phone.clone())),
            Ok("electra")
        );
    }

    /// A directory not read, or not complete, is an honest refusal naming
    /// what is missing, never a guess; the name it already has can still be
    /// saved (amendment 2 (5)).
    #[test]
    fn an_unread_directory_refuses_a_rename_honestly() {
        let nothing: DirectoryRead = Err(Unread::default());
        let beta: DirectoryRead = Err(Unread {
            database: Some("beta".to_owned()),
            ..Default::default()
        });
        let unread = |database: Option<&str>| NameRefusal::Unread {
            current: "hesperia".to_owned(),
            database: database.map(str::to_owned),
        };
        assert_eq!(
            check("studio", "hesperia", "hesperia", &nothing),
            Err(unread(None))
        );
        assert_eq!(
            unread(None).to_string(),
            "keeper has not read your agents server's list of devices and agents on this \
             device yet, so it cannot tell whether this device runs one of your agents. Its \
             name stays hesperia until it has; try again once it is connected."
        );
        assert_eq!(
            check("studio", "hesperia", "hesperia", &beta),
            Err(unread(Some("beta")))
        );
        assert_eq!(
            unread(Some("beta")).to_string(),
            "keeper has not read the devices and agents of beta on this device yet, so it \
             cannot tell whether this device runs one of your agents there. Its name stays \
             hesperia until it has; try again once beta has synced."
        );
        assert_eq!(
            check("hesperia", "hesperia", "hesperia", &beta),
            Ok("hesperia")
        );
        let tmp = data_dir();
        registry::set_device_name(tmp.path(), "studio").expect("row");
        assert_eq!(
            refused(plan_reset(tmp.path(), "hesperia", &beta)),
            NameRefusal::Unread {
                current: "studio".to_owned(),
                database: Some("beta".to_owned()),
            }
        );
    }

    /// Amendment 2 (6), R607-05: the check compares every name the server
    /// may know this device by — the host its credential was created with
    /// (`t4_host`), the last host it heard, the system's name and the name
    /// now — so a device renamed once (hesperia → studio) before an agent was
    /// installed under its first host still keeps its name.
    #[test]
    fn every_name_the_server_knows_this_device_by_counts() {
        let runs = Err(NameRefusal::Runs {
            agent: "nixi".to_owned(),
            host: "hesperia".to_owned(),
        });
        let renamed_once = Directory {
            bound: Some("studio".to_owned()),
            enrolled: Some("hesperia".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert_eq!(check("den", "studio", "macbook", &Ok(renamed_once)), runs);
        let by_system = Directory {
            bound: Some("studio".to_owned()),
            enrolled: Some("studio".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert_eq!(check("den", "studio", "hesperia", &Ok(by_system)), runs);
        let elsewhere = Directory {
            bound: Some("studio".to_owned()),
            enrolled: Some("studio".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert_eq!(check("den", "studio", "macbook", &Ok(elsewhere)), Ok("den"));
    }

    /// Every character JavaScript's `String.prototype.trim` removes, as bun
    /// 1.x answers `String.fromCodePoint(c).trim() === ""` over all of
    /// Unicode (ECMAScript WhiteSpace + LineTerminator).
    const SERVER_TRIMS: [u32; 25] = [
        0x9, 0xA, 0xB, 0xC, 0xD, 0x20, 0xA0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
        0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000,
        0xFEFF,
    ];

    /// Amendment 2 (8), R607-07: keeper trims a name exactly as the agents
    /// server does — every code point, so U+FEFF (trimmed by JS, kept by
    /// Rust's `trim`) and U+0085 (the reverse) agree with the server.
    #[test]
    fn a_name_is_trimmed_exactly_as_the_server_trims_it() {
        for code in 0..=0x10FFFF_u32 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            assert_eq!(
                is_server_space(c),
                SERVER_TRIMS.contains(&code),
                "U+{code:04X}"
            );
        }
        assert_eq!(host_rule("\u{FEFF}"), Err(NameRefusal::Blank));
        assert_eq!(host_rule("\u{FEFF}kalypso\u{FEFF}"), Ok("kalypso"));
        assert_eq!(host_rule("\u{85}kalypso"), Ok("\u{85}kalypso"));
        assert_eq!(host_rule("\u{85}"), Ok("\u{85}"));
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
            check("kalypso", "iPhone", "macbook", &Ok(pinned.clone())),
            Err(refusal.clone())
        );
        assert_eq!(
            refusal.to_string(),
            "agents.example.org knows this device by the name it was paired with, and only your \
             account's sign-in can give it a new one there; pairing it again would make it a \
             different device. Its name stays iPhone."
        );
        assert_eq!(
            check("iPhone", "iPhone", "macbook", &Ok(pinned.clone())),
            Ok("iPhone")
        );
        let both = Directory {
            pinned: Some("agents.example.org".to_owned()),
            ..hesperia_runs_nixi()
        };
        assert!(matches!(
            check("studio", "hesperia", "macbook", &Ok(both.clone())),
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

    /// R607-15: Reset validates the destination before any name write.
    #[test]
    fn reset_refuses_taken_blank_and_overlong_system_names_before_writing() {
        let tmp = data_dir();
        registry::set_device_name(tmp.path(), "kalypso").expect("stored");
        let taken = Ok(Directory {
            taken: vec!["iPhone".to_owned()],
            ..Default::default()
        });
        assert_eq!(
            refused(plan_reset(tmp.path(), "iPhone", &taken)),
            NameRefusal::Taken("iPhone".to_owned())
        );
        for blank in ["", " \u{feff} "] {
            assert_eq!(
                refused(plan_reset(tmp.path(), blank, &Ok(unpaired()))),
                NameRefusal::Blank
            );
        }
        assert_eq!(
            refused(plan_reset(tmp.path(), &"😀".repeat(101), &Ok(unpaired()))),
            NameRefusal::TooLong(202)
        );
        assert_eq!(
            registry::get_device_name(tmp.path())
                .expect("read")
                .as_deref(),
            Some("kalypso")
        );
    }
}

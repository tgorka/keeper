//! What Settings › Agents draws of the agents server (UX-DR-E106-1): the
//! server row, one status line per database, this device, the devices of the
//! person and a pairing code. Every sentence is Rust's; the webview decides
//! none of them. No view model carries a token.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::hq::port::LinkState;

/// Where this device stands with the agents server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HqPhase {
    /// `agents.server_url` is blank or absent.
    NoServer,
    /// The stored address is one keeper will not contact.
    Refused,
    /// An address, and no pairing for it on this device.
    NotPaired,
    /// Paired; the databases carry their own state.
    Paired,
    /// The server no longer knows this device: it was removed.
    Removed,
}

/// One database's state on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HqDatabaseState {
    Connecting,
    Connected,
    Offline,
    /// The device's token no longer opens it; the replica stays readable.
    NotOpen,
    /// The device was removed; the replica stays readable.
    Removed,
}

/// One database the device holds a replica of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqDatabaseVm {
    pub name: String,
    pub state: HqDatabaseState,
    /// Commits written here the server has not taken yet.
    pub waiting: u32,
    /// The status line, in keeper's words.
    pub sentence: String,
}

/// This device, as the server knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqThisDeviceVm {
    pub client_id: String,
    /// The name the person's devices list it by.
    pub name: String,
}

/// The agents server as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqStatusVm {
    /// The stored address, normalised; `None` when blank.
    pub server: Option<String>,
    pub phase: HqPhase,
    /// The server row's line.
    pub sentence: String,
    pub device: Option<HqThisDeviceVm>,
    pub databases: Vec<HqDatabaseVm>,
    /// Commits written while no server was paired, waiting on this device.
    pub waiting_here: u32,
    /// The person's devices, from the replicated directory; follows it.
    pub devices: Vec<HqDeviceVm>,
    /// A pairing link opened on this device and not yet confirmed: the
    /// server's host it would pair with.
    pub offer: Option<String>,
}

/// One of the person's devices, read from a database's `clients` rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqDeviceVm {
    pub client_id: String,
    pub name: String,
    /// When it was paired, in ms since the epoch.
    #[ts(type = "number | null")]
    pub added_at_ms: Option<i64>,
    pub this_device: bool,
    /// Removed: listed until the directory forgets it, never revocable again.
    pub revoked: bool,
}

/// A code another device of this person can redeem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqPairCodeVm {
    /// The code in groups of four.
    pub code: String,
    /// The QR, as an SVG drawn on a white card; `None` when it would not fit.
    pub qr_svg: Option<String>,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
}

fn waiting_clause(waiting: u32) -> String {
    match waiting {
        0 => String::new(),
        1 => " 1 change waits on this device.".to_owned(),
        n => format!(" {n} changes wait on this device."),
    }
}

/// A database's state and line from its replica's link and outbox, and from
/// what the server last said about the device and the database.
pub fn database_vm(
    name: &str,
    link: LinkState,
    outbox: usize,
    open: bool,
    removed: bool,
) -> HqDatabaseVm {
    let waiting = u32::try_from(outbox).unwrap_or(u32::MAX);
    let (state, line) = if removed {
        (
            HqDatabaseState::Removed,
            "This device was removed; what it holds stays readable here.".to_owned(),
        )
    } else if !open {
        (
            HqDatabaseState::NotOpen,
            "Not open to this device any more; what it holds stays readable here.".to_owned(),
        )
    } else {
        match link {
            LinkState::Connected => (HqDatabaseState::Connected, "Connected.".to_owned()),
            LinkState::Offline => (
                HqDatabaseState::Offline,
                "Can't reach the server; keeper keeps trying.".to_owned(),
            ),
            // A refusal the discovery has not judged yet, or a replica
            // still opening.
            LinkState::Connecting | LinkState::Refused | LinkState::Local | LinkState::Closed => {
                (HqDatabaseState::Connecting, "Connecting…".to_owned())
            }
        }
    };
    HqDatabaseVm {
        name: name.to_owned(),
        state,
        waiting,
        sentence: format!("{line}{}", waiting_clause(waiting)),
    }
}

/// The server row's line for a phase. `host` is the server's host and port.
pub fn phase_sentence(phase: HqPhase, host: Option<&str>, waiting_here: u32) -> String {
    let host = host.unwrap_or("the server");
    match phase {
        HqPhase::NoServer => format!(
            "No server is set. What you write to your agents waits on this device.{}",
            waiting_clause(waiting_here)
        ),
        HqPhase::Refused => "keeper will not contact this address.".to_owned(),
        HqPhase::NotPaired => format!(
            "This device is not paired with {host}. Type the code a paired device shows under \
             Add a device.{}",
            waiting_clause(waiting_here)
        ),
        HqPhase::Paired => format!("Paired with {host}."),
        HqPhase::Removed => format!(
            "This device was removed from {host}. It no longer syncs, and what it holds stays \
             readable here until you remove it."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_line_follows_its_link_and_what_the_server_said() {
        let vm = |link, outbox, open, removed| database_vm("alpha", link, outbox, open, removed);
        assert_eq!(
            vm(LinkState::Connected, 0, true, false).state,
            HqDatabaseState::Connected
        );
        assert_eq!(
            vm(LinkState::Connected, 0, true, false).sentence,
            "Connected."
        );
        let offline = vm(LinkState::Offline, 3, true, false);
        assert_eq!(offline.state, HqDatabaseState::Offline);
        assert_eq!(offline.waiting, 3);
        assert_eq!(
            offline.sentence,
            "Can't reach the server; keeper keeps trying. 3 changes wait on this device."
        );
        assert_eq!(
            vm(LinkState::Refused, 0, true, false).state,
            HqDatabaseState::Connecting
        );
        // What the server said wins over the link.
        assert_eq!(
            vm(LinkState::Connected, 0, false, false).state,
            HqDatabaseState::NotOpen
        );
        assert_eq!(
            vm(LinkState::Refused, 1, false, true).state,
            HqDatabaseState::Removed
        );
        assert!(vm(LinkState::Refused, 1, true, true)
            .sentence
            .ends_with("1 change waits on this device."));
    }

    #[test]
    fn the_no_server_line_says_writes_wait_here() {
        assert_eq!(
            phase_sentence(HqPhase::NoServer, None, 0),
            "No server is set. What you write to your agents waits on this device."
        );
        assert!(
            phase_sentence(HqPhase::NoServer, None, 2).ends_with("2 changes wait on this device.")
        );
        assert!(phase_sentence(HqPhase::Removed, Some("hq.example.org"), 0)
            .starts_with("This device was removed from hq.example.org."));
        assert!(
            phase_sentence(HqPhase::Paired, Some("hq.example.org"), 0)
                == "Paired with hq.example.org."
        );
    }
}

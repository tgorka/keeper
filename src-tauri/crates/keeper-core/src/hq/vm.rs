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
    /// No server address is set: the replica stays readable and written to,
    /// and what is written waits in its outbox.
    NoServer,
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
    /// Commits waiting on this device that no server takes: the local
    /// replica's, and with no address set every held replica's.
    pub waiting_here: u32,
    /// The person's devices, from the replicated directory; follows it.
    pub devices: Vec<HqDeviceVm>,
    /// A pairing link opened on this device and not yet confirmed: the
    /// server's host it would pair with.
    pub offer: Option<String>,
    /// Whether this device gets its credential by signing in (109.6).
    pub sign_in: HqSignInVm,
}

/// Where this device stands with signing in to the agents server (109.6,
/// Q109-01 a): the account's sign-in exchanged once for the device's
/// credential, with no code to type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HqSignInState {
    /// No account is set up, no server is set, or the server offers no
    /// sign-in: a pairing code is the way in.
    Unavailable,
    /// The account is set up and the server trusts its sign-in, but nobody
    /// is signed in: signing in connects this device.
    SignedOut,
    /// keeper is checking the server and signing this device in now.
    Enrolling,
    /// This device's credential comes from the account's sign-in.
    Enrolled,
    /// This device's credential comes from a sign-in under its old name
    /// (R-HQ-94): signing in again gives the server the new one.
    Renamed,
    /// Signing in did not work; the sentence says why. A pairing code still
    /// works.
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqSignInVm {
    pub state: HqSignInState,
    /// The effective name is not yet acknowledged by the existing credential.
    pub name_pending: bool,
    /// The line Settings shows under the server, when there is one.
    pub sentence: Option<String>,
}

impl HqSignInVm {
    pub fn unavailable() -> HqSignInVm {
        HqSignInVm {
            state: HqSignInState::Unavailable,
            name_pending: false,
            sentence: None,
        }
    }
}

/// One "new device" notice (S-ID-8): another of the person's devices that
/// signed in, was adopted by a sign-in, or redeemed a pairing code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HqEnrolmentVm {
    pub client_id: String,
    pub sentence: String,
    #[ts(type = "number")]
    pub at_ms: i64,
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

/// The server row's line while a device without a credential gets one by
/// signing in rather than by a code (109.6).
pub fn not_connected_sentence(host: &str, waiting_here: u32) -> String {
    format!(
        "This device is not connected to {host} yet.{}",
        waiting_clause(waiting_here)
    )
}

/// A database's state and line from its replica's link and outbox, whether
/// an address is set to link it to, and what the server last said about the
/// device and the database.
pub fn database_vm(
    name: &str,
    link: LinkState,
    outbox: usize,
    linkable: bool,
    open: bool,
    removed: bool,
) -> HqDatabaseVm {
    let waiting = u32::try_from(outbox).unwrap_or(u32::MAX);
    let (state, line) = if removed {
        (
            HqDatabaseState::Removed,
            "This device was removed; what it holds stays readable here.".to_owned(),
        )
    } else if !linkable {
        (
            HqDatabaseState::NoServer,
            "No server is set; what you write here waits on this device.".to_owned(),
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
        let vm =
            |link, outbox, open, removed| database_vm("alpha", link, outbox, true, open, removed);
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
        // With no address, what is held waits, whatever the server last said.
        let unset = database_vm("alpha", LinkState::Local, 2, false, false, false);
        assert_eq!(unset.state, HqDatabaseState::NoServer);
        assert_eq!(
            unset.sentence,
            "No server is set; what you write here waits on this device. 2 changes wait on this \
             device."
        );
        assert_eq!(
            database_vm("alpha", LinkState::Local, 0, false, true, true).state,
            HqDatabaseState::Removed
        );
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

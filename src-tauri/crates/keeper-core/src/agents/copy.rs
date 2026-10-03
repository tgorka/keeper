//! An agent's copy on this Mac, as Settings › Agents shows it, and the
//! decisions about its drive's pin (story 90.6; UX-DR128, S-15, DW-367).
//!
//! A desktop hosts the agents of a drive whose `_drive.toml` `principal` is
//! the account's login, and only under the owner, readers and `local_only`
//! the person pinned here ([`crate::agents::pins`]). The first sign-in for a
//! drive pins what the person was shown; a `_drive.toml` that differs later
//! hosts nothing until the person reviews it and pins again. Nothing re-pins
//! by itself.

use std::collections::BTreeSet;

use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::agentd::DrivePin;
use crate::agents::drive::DriveDecl;
use crate::agents::mount::pin_matches;

/// What a Mac without an organisation account says (DW-367): a host is
/// named by the account's device slug, so without one it hosts nothing.
pub const NO_ACCOUNT: &str = "This Mac hosts agents only when it is signed in to an organisation account, which names it. Sign in under Account.";

/// An account whose device has not been registered yet has no slug either.
pub const NO_DEVICE: &str =
    "This Mac has no device name in your account yet. It hosts agents once the account has synced.";

/// What a sign-in or a re-pin is refused with when the drive's file changed
/// after the person was shown it.
pub const CHANGED_SINCE_SHOWN: &str = "This drive's owner, readers or local-only setting changed after they were shown. Look at them again before you pin them.";

/// Someone a drive names: their Matrix id, and their display name when the
/// person's own Matrix account could read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentPersonVm {
    pub matrix_id: String,
    pub display_name: Option<String>,
}

/// Whether this Mac pinned a drive's readers, and whether its `_drive.toml`
/// still says what was pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AgentPinState {
    /// Nothing pinned: the first sign-in pins what it shows.
    Unpinned,
    /// Pinned, and the file agrees.
    Pinned,
    /// Pinned, and the file says something else: the zone hosts nothing.
    Differs,
}

/// A drive's pin as the row shows it: what `_drive.toml` says now, what was
/// pinned, and each difference in one sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentPinVm {
    pub state: AgentPinState,
    pub owner: AgentPersonVm,
    pub readers: Vec<AgentPersonVm>,
    /// Whether `_drive.toml` lets its agents use only local models.
    pub local_only: bool,
    pub pinned_owner: Option<AgentPersonVm>,
    pub pinned_readers: Vec<AgentPersonVm>,
    pub pinned_local_only: Option<bool>,
    pub differences: Vec<String>,
}

/// One agent of one of this principal's drives, on this Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentCopyVm {
    /// The sync profile the drive is synced as here.
    pub profile_id: String,
    /// The drive's id (`_drive.toml` `id`), or the folder's name when its
    /// `_drive.toml` does not read.
    pub drive: String,
    /// The agent's id (its home folder); empty on a folder's own row.
    pub agent: String,
    /// The agent's display name.
    pub name: String,
    pub matrix_user: String,
    /// The copy's Matrix device on this Mac, once signed in.
    pub device: Option<String>,
    /// This Mac's host slug: the account's device name.
    pub host: Option<String>,
    pub signed_in: bool,
    /// `None` on the one row of a flagged folder whose `_drive.toml` does
    /// not read: it has no agents to list, only `problem`.
    pub pin: Option<AgentPinVm>,
    /// Why this Mac does not host the agent now, in one sentence.
    pub problem: Option<String>,
}

/// The owner, readers and `local_only` the person was shown and is pinning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentPinReq {
    pub owner: String,
    pub readers: Vec<String>,
    pub local_only: bool,
}

/// A person by id, with a display name from `name`.
fn person(user: &UserId, name: &dyn Fn(&UserId) -> Option<String>) -> AgentPersonVm {
    AgentPersonVm {
        matrix_id: user.to_string(),
        display_name: name(user),
    }
}

/// `decl` against `pin` as the row shows it.
pub fn pin_vm(
    decl: &DriveDecl,
    pin: Option<&DrivePin>,
    name: &dyn Fn(&UserId) -> Option<String>,
) -> AgentPinVm {
    let (state, differences) = match pin.map(|pin| pin_matches(decl, pin)) {
        None => (AgentPinState::Unpinned, Vec::new()),
        Some(Ok(())) => (AgentPinState::Pinned, Vec::new()),
        Some(Err(difference)) => (AgentPinState::Differs, difference.differences),
    };
    AgentPinVm {
        state,
        owner: person(&decl.owner, name),
        readers: decl.readers.iter().map(|r| person(r, name)).collect(),
        local_only: decl.local_only,
        pinned_owner: pin.map(|pin| person(&pin.owner, name)),
        pinned_readers: pin
            .map(|pin| pin.readers.iter().map(|r| person(r, name)).collect())
            .unwrap_or_default(),
        pinned_local_only: pin.map(|pin| pin.local_only),
        differences,
    }
}

/// The pin a person's tap makes: `decl` as it is now, but only when it is
/// what they were `shown`. `remote` is the folder's git remote.
pub fn pin_from_shown(
    decl: &DriveDecl,
    remote: &str,
    shown: &AgentPinReq,
) -> Result<DrivePin, String> {
    let owner =
        OwnedUserId::try_from(shown.owner.as_str()).map_err(|_| CHANGED_SINCE_SHOWN.to_owned())?;
    let readers = shown
        .readers
        .iter()
        .map(|reader| OwnedUserId::try_from(reader.as_str()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| CHANGED_SINCE_SHOWN.to_owned())?;
    if owner != decl.owner || readers != decl.readers || shown.local_only != decl.local_only {
        return Err(CHANGED_SINCE_SHOWN.to_owned());
    }
    Ok(DrivePin {
        id: decl.id.clone(),
        remote: remote.to_owned(),
        credential: None,
        owner,
        readers,
        local_only: decl.local_only,
    })
}

/// Whether a desktop signed in as `login` hosts a drive declaring
/// `decl.principal` (R27, D-27): only that person's own agents, never a
/// shared principal's.
pub fn hosts_principal(decl: &DriveDecl, login: &str) -> Result<(), String> {
    if decl.principal == login {
        Ok(())
    } else {
        Err(format!(
            "{}'s agents belong to {}; this Mac hosts only {login}'s.",
            decl.id, decl.principal
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::drive;

    fn decl(readers: &[&str]) -> DriveDecl {
        let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
        drive::parse(&format!(
            "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [{}]\n",
            readers.join(", ")
        ))
        .expect("decl")
    }

    fn shown(readers: &[&str]) -> AgentPinReq {
        AgentPinReq {
            owner: "@tgorka:example.org".to_owned(),
            readers: readers.iter().map(|r| (*r).to_owned()).collect(),
            local_only: false,
        }
    }

    fn local_only(decl: DriveDecl) -> DriveDecl {
        DriveDecl {
            local_only: true,
            ..decl
        }
    }

    const TG: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";

    fn no_names(_: &UserId) -> Option<String> {
        None
    }

    /// The tap pins only what was on screen: a reader added between the
    /// showing and the tap is refused, never pinned unseen (S-15), and so is
    /// a `local_only` that flipped — the pin must never lower it unseen.
    #[test]
    fn a_pin_is_made_only_from_what_was_shown() {
        let file = decl(&[TG, MARTA]);
        let pin = pin_from_shown(&file, "remote", &shown(&[MARTA, TG])).expect("same set");
        assert_eq!(pin.readers, file.readers);
        assert_eq!(pin.owner, file.owner);
        assert!(!pin.local_only);
        assert_eq!(
            pin_from_shown(&file, "remote", &shown(&[TG])),
            Err(CHANGED_SINCE_SHOWN.to_owned())
        );
        let mut other_owner = shown(&[TG, MARTA]);
        other_owner.owner = MARTA.to_owned();
        assert_eq!(
            pin_from_shown(&file, "remote", &other_owner),
            Err(CHANGED_SINCE_SHOWN.to_owned())
        );

        // Shown local-only, but the file now says otherwise: refused.
        let mut shown_local = shown(&[TG, MARTA]);
        shown_local.local_only = true;
        assert_eq!(
            pin_from_shown(&file, "remote", &shown_local),
            Err(CHANGED_SINCE_SHOWN.to_owned())
        );
        // Shown not local-only, but the file turned it on: refused too.
        let strict = local_only(decl(&[TG, MARTA]));
        assert_eq!(
            pin_from_shown(&strict, "remote", &shown(&[TG, MARTA])),
            Err(CHANGED_SINCE_SHOWN.to_owned())
        );
        let pin = pin_from_shown(&strict, "remote", &shown_local).expect("what was shown");
        assert!(pin.local_only);
    }

    #[test]
    fn the_row_names_each_difference_from_the_pin() {
        let pinned = pin_from_shown(&decl(&[TG]), "remote", &shown(&[TG])).expect("pin");
        assert_eq!(
            pin_vm(&decl(&[TG]), None, &no_names).state,
            AgentPinState::Unpinned
        );
        assert_eq!(
            pin_vm(&decl(&[TG]), Some(&pinned), &no_names).state,
            AgentPinState::Pinned
        );
        let changed = pin_vm(&decl(&[TG, MARTA]), Some(&pinned), &no_names);
        assert_eq!(changed.state, AgentPinState::Differs);
        assert_eq!(changed.differences.len(), 1);
        assert!(changed.differences[0].contains(MARTA), "{changed:?}");
        assert_eq!(
            changed.pinned_readers,
            vec![AgentPersonVm {
                matrix_id: TG.to_owned(),
                display_name: None
            }]
        );

        // A file that turned local_only off shows both values: the row's two
        // columns are where the person sees what a re-pin would lower.
        let mut local_shown = shown(&[TG]);
        local_shown.local_only = true;
        let local_pin =
            pin_from_shown(&local_only(decl(&[TG])), "remote", &local_shown).expect("pin");
        let lowered = pin_vm(&decl(&[TG]), Some(&local_pin), &no_names);
        assert_eq!(lowered.state, AgentPinState::Differs);
        assert!(!lowered.local_only);
        assert_eq!(lowered.pinned_local_only, Some(true));
        assert_eq!(
            pin_vm(&decl(&[TG]), None, &no_names).pinned_local_only,
            None
        );
    }

    #[test]
    fn a_desktop_hosts_only_its_own_login_s_drives() {
        assert_eq!(hosts_principal(&decl(&[TG]), "tgorka"), Ok(()));
        let refused = hosts_principal(&decl(&[TG]), "marta").expect_err("another principal");
        assert!(
            refused.contains("tgorka") && refused.contains("marta"),
            "{refused}"
        );
    }
}

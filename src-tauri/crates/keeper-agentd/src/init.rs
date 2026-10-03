//! `keeper-agentd init`: the `agentd.toml` skeleton, and the principal's
//! control room.

use std::io::Write;
use std::path::Path;

use keeper_agent::headless::HeadlessPlatform;
use keeper_agent::runtime::{check_out_missing, inspect, restore_copy};
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::matrix::RoomKind;
use matrix_sdk::ruma::{OwnedUserId, UserId};
use toml_edit::{value, DocumentMut};

use crate::cli::{runtime, CliError, Host};

/// What `init` writes when there is no `agentd.toml`: every key a host
/// needs, with the values a person fills in. It does not parse until they
/// are filled, so `run` refuses it naming the first one.
pub const SKELETON: &str = r#"# keeper-agentd's configuration. See docs/agents.md § A Linux host.
version   = 1
principal = "<principal>"                  # this host's principal, e.g. "tgorka"
host      = "<host>"                       # this machine's slug, e.g. "electra"
always_on = true

[homeserver]
url          = "https://<homeserver>"
control_room = ""                          # written by `keeper-agentd init`

[[drives]]
id         = "<drive>"
remote     = "https://<forge>/<owner>/<drive>.git"
credential = "secret:<drive>"
owner      = "@<owner>:<homeserver>"
readers    = ["@<owner>:<homeserver>"]   # copied from the forge's collaborator list

[[providers]]
kind       = "openai"
base_url   = "https://<provider>"
credential = "secret:<provider>"

[[agents]]
drive = "<drive>"
ids   = ["<agent>"]
"#;

/// Write the skeleton at `path` unless a file is there; whether it wrote.
/// Never overwrites, whatever the file holds.
pub fn write_skeleton(path: &Path) -> std::io::Result<bool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(SKELETON.as_bytes())?;
            file.sync_all()?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

/// `text` with `[homeserver].control_room` set to `room`, every other byte —
/// comments, spacing, order — kept.
pub fn set_control_room(text: &str, room: &str) -> Result<String, String> {
    let mut document: DocumentMut = text
        .parse()
        .map_err(|error| format!("agentd.toml is not valid TOML: {error}"))?;
    let homeserver = document
        .get_mut("homeserver")
        .and_then(|item| item.as_table_like_mut())
        .ok_or_else(|| "agentd.toml has no [homeserver] table".to_owned())?;
    match homeserver.get_mut("control_room") {
        Some(item) => {
            let decor = item.as_value().map(|v| v.decor().clone());
            *item = value(room);
            if let (Some(decor), Some(new)) = (decor, item.as_value_mut()) {
                *new.decor_mut() = decor;
            }
        }
        None => {
            homeserver.insert("control_room", value(room));
        }
    }
    Ok(document.to_string())
}

/// Every agent user of `principal` in the mounted drives but `creator`: each
/// is invited to the control room and may write its host's manifest there
/// (AD-374).
fn principal_agents(
    principal: &str,
    drives: &[keeper_agent::runtime::DriveView],
    creator: &UserId,
) -> Vec<OwnedUserId> {
    let mut agents: Vec<OwnedUserId> = drives
        .iter()
        .filter(|drive| {
            drive
                .hosts
                .as_ref()
                .is_ok_and(|decl| decl.principal == principal)
        })
        .flat_map(|drive| drive.zone.homes.iter())
        .filter_map(|(_, home)| home.as_ref().ok().map(|h| h.config.matrix_user.clone()))
        .filter(|agent| agent != creator)
        .collect();
    agents.sort();
    agents.dedup();
    agents
}

/// `init`.
pub fn run(host: &Host) -> Result<(), CliError> {
    // The unit's `ReadWritePaths=` names these; they exist before `run` does.
    for dir in [&host.dirs.data, &host.dirs.state] {
        std::fs::create_dir_all(dir).map_err(|error| {
            CliError::Failure(format!("{} could not be created: {error}", dir.display()))
        })?;
    }
    let path = &host.config_path;
    let wrote = write_skeleton(path).map_err(|error| {
        CliError::Failure(format!("{} could not be written: {error}", path.display()))
    })?;
    if wrote {
        println!("Wrote {}. Fill in its values, then run `keeper-agentd login <drive>/<agent>` and `keeper-agentd init` again.", path.display());
    } else {
        println!(
            "Left {} as it is: keeper-agentd init never overwrites it.",
            path.display()
        );
    }
    let config = match host.config() {
        Ok(config) => config,
        Err(error) => {
            println!("{error}");
            return Ok(());
        }
    };
    if let Some(room) = &config.homeserver.control_room {
        println!("The control room is {room}.");
        return Ok(());
    }
    let secrets = host.harden(&config)?;
    let runtime = runtime()?;
    runtime.block_on(check_out_missing(&config, &host.dirs, &secrets))?;
    let inspection = inspect(&config, &host.dirs);
    let platform = HeadlessPlatform::new(&host.dirs.data, secrets);
    // The control room is the person's own door's: only the hosted proxy's
    // copy creates it, and init waits until that copy is signed in.
    let Some(proxy) = inspection
        .hosted
        .iter()
        .find(|home| home.config.kind == AgentKind::Proxy)
    else {
        println!(
            "No proxy is hosted here, so there is no control room: it is the proxy's to create."
        );
        return Ok(());
    };
    let owners: Vec<_> = {
        let mut owners: Vec<_> = config
            .home_drives()
            .into_iter()
            .filter_map(|id| config.drive(id).map(|pin| pin.owner.clone()))
            .collect();
        owners.sort();
        owners.dedup();
        owners
    };
    let user = &proxy.config.matrix_user;
    let agents = principal_agents(&config.principal, &inspection.drives, user);
    let mut invite = owners.clone();
    invite.extend(agents.iter().cloned());
    let created = runtime.block_on(async {
        let client = match restore_copy(&config, &platform, &host.dirs.data, user).await {
            Ok(Some(client)) => client,
            Ok(None) => return Ok(None),
            Err(error) => return Err(CliError::Failure(error)),
        };
        client
            .sync_once()
            .await
            .map_err(|error| CliError::Failure(error.to_string()))?;
        let room = client
            .create_room(
                RoomKind::Control,
                &format!("{}'s agents", config.principal),
                invite,
                &agents,
            )
            .await
            .map_err(|error| CliError::Failure(error.to_string()))?;
        Ok(Some(room))
    })?;
    let Some(room) = created else {
        println!(
            "The proxy {}/{} is not signed in yet, so there is no control room: run `keeper-agentd login {}/{}`, then `keeper-agentd init` again.",
            proxy.config.drive, proxy.config.id, proxy.config.drive, proxy.config.id
        );
        return Ok(());
    };
    let by = user.clone();
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::Failure(format!("{}: {error}", path.display())))?;
    let updated = set_control_room(&text, room.as_str()).map_err(CliError::Config)?;
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, updated)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|error| CliError::Failure(format!("{}: {error}", path.display())))?;
    println!(
        "Created the control room {room} as {by}, and set it in {}.",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_never_overwrites_agentd_toml_and_says_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keeper-agentd/agentd.toml");
        assert!(write_skeleton(&path).expect("first"));
        assert_eq!(std::fs::read_to_string(&path).expect("read"), SKELETON);
        std::fs::write(&path, "version = 1 # mine\n").expect("edit");
        assert!(!write_skeleton(&path).expect("second"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "version = 1 # mine\n"
        );
    }

    #[test]
    fn init_sets_the_control_room_and_keeps_every_other_byte() {
        let text = "# mine\nversion = 1\n\n[homeserver]\nurl          = \"https://m.example.org\"\ncontrol_room = \"\"   # set by init\n\n[[drives]]\nid = \"tgdrive\"  # pinned\n";
        let updated = set_control_room(text, "!abc:example.org").expect("set");
        assert_eq!(
            updated,
            text.replace("control_room = \"\"", "control_room = \"!abc:example.org\"")
        );
        // A file with no control_room line gets one, the rest unchanged.
        let bare = "[homeserver]\nurl = \"https://m.example.org\" # here\n";
        let added = set_control_room(bare, "!abc:example.org").expect("add");
        assert!(added.starts_with(bare), "{added}");
        assert!(added.contains("control_room = \"!abc:example.org\""));
    }

    /// The unit's `ReadWritePaths=` are agentd's data and state directories:
    /// `init` makes both, so a unit started before `login` does not fail in
    /// its namespace setup.
    #[test]
    fn init_creates_the_directories_the_unit_may_write() {
        let root = tempfile::tempdir().expect("tempdir");
        let dirs = keeper_sync::xdg::XdgDirs {
            config: root.path().join("config/keeper-agentd"),
            data: root.path().join("data/keeper-agentd"),
            state: root.path().join("state/keeper-agentd"),
        };
        let host = Host {
            config_path: dirs.config.join("agentd.toml"),
            dirs,
        };
        run(&host).expect("init");
        assert!(host.config_path.is_file());
        assert!(host.dirs.data.is_dir());
        assert!(host.dirs.state.is_dir());
    }
}

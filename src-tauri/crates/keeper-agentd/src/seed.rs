//! `keeper-agentd agents init` and `agents new`: a drive's agents zone
//! seeded, a new agent from its template, and the proxy's DM (story 91.5).
//!
//! `init` writes the zone and never overwrites a file. Against agentd's own
//! checkout it refuses an owner, readers or `local_only` that differ from
//! the host's pin (S-15), and once the seeded proxy's copy is signed in it
//! makes the proxy's DM with its person, once — but only while no `run`
//! holds the copies (see [`crate::host_lock`]). With `--into <dir>` it writes
//! the zone of that checkout only and says how the DM is made later (Q7).
//!
//! Either way the zone is where the checkout's `.keeper/keeper.toml` puts it
//! (`[folder.agents] subfolder`), as the engine reads it.

use std::path::{Path, PathBuf};

use keeper_agent::runtime::{check_out_missing, drive_profile, inspect, restore_copy};
use keeper_agent::seed::{self, Applied, Made};
use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::agents::seed::SeedChoices;
use keeper_core::agents::soul;
use keeper_sync::profile::FolderTier;
use keeper_sync::SyncProfile;

use crate::cli::{runtime, CliError, Host};

/// `agents init`'s flags.
pub struct InitArgs<'a> {
    pub drive: &'a str,
    pub with: &'a [String],
    pub owner: &'a str,
    pub readers: &'a [String],
    pub bot: Option<&'a str>,
    pub local_only: bool,
    pub principal: Option<&'a str>,
    pub into: Option<&'a Path>,
}

fn say(applied: &Applied) {
    for path in &applied.written {
        println!("wrote {path}");
    }
    for path in &applied.left {
        println!("left  {path} (it was there; keeper-agentd never overwrites a file)");
    }
    println!(
        "Wrote {} files and left {}.",
        applied.written.len(),
        applied.left.len()
    );
}

/// The configuration when there is one; `None` only when the file is not
/// there — one that does not read is refused, never taken as absent.
fn config_if_any(host: &Host) -> Result<Option<AgentdConfig>, CliError> {
    if host.config_path.exists() {
        host.config().map(Some)
    } else {
        Ok(None)
    }
}

/// `profile` with its checkout's `.keeper/keeper.toml` laid over it, as the
/// engine reads it on `host`: the agents zone is where `[folder.agents]`
/// puts it, the default when the file says nothing. A file that does not
/// read refuses, since it may be what moves the zone.
fn in_force(profile: &SyncProfile, host: &str) -> Result<SyncProfile, CliError> {
    let outcome = FolderTier::new(host, None).apply(profile);
    if let Some(fault) = outcome.faults.first() {
        return Err(CliError::Config(format!(
            "{} {}; the agents zone's place is read from it, so fix it first.",
            fault.path.display(),
            fault.message
        )));
    }
    Ok(outcome.profile)
}

/// The agents zone of the checkout at `into`.
fn zone_of_checkout(into: &Path, drive: &str, host: &str) -> Result<PathBuf, CliError> {
    if !into.is_dir() {
        return Err(CliError::Config(format!(
            "{} is not a folder: --into names a checkout of {drive}.",
            into.display()
        )));
    }
    let root = std::path::absolute(into)
        .map_err(|error| CliError::Config(format!("{} cannot be read: {error}", into.display())))?;
    let mut profile = SyncProfile::new(
        drive.to_owned(),
        drive.to_owned(),
        root.clone(),
        root.display().to_string(),
    );
    // Both zones at their defaults, as `drive_profile` gives agentd's own
    // checkout: a folder that keeps agents keeps sessions too.
    profile.sessions = Some(Default::default());
    profile.agents = Some(Default::default());
    in_force(&profile, host)?
        .agents_root()
        .ok_or_else(|| CliError::Failure("the checkout has no agents zone".to_owned()))
}

/// The command a person runs next, with the same flags and no `--into`.
fn follow_up(args: &InitArgs<'_>) -> String {
    let mut line = format!("keeper-agentd agents init {}", args.drive);
    if !args.with.is_empty() {
        line.push_str(&format!(" --with {}", args.with.join(",")));
    }
    line.push_str(&format!(" --owner {}", args.owner));
    for reader in args.readers {
        line.push_str(&format!(" --reader {reader}"));
    }
    if args.local_only {
        line.push_str(" --local-only");
    }
    line.push_str(&format!(" --bot \"{}\"", args.bot.unwrap_or_default()));
    line
}

/// `agents init`.
pub fn init(host: &Host, args: &InitArgs<'_>) -> Result<(), CliError> {
    let config = match args.into {
        Some(_) => config_if_any(host)?,
        None => Some(host.config()?),
    };
    let principal = args
        .principal
        .map(str::to_owned)
        .or_else(|| config.as_ref().map(|c| c.principal.clone()))
        .ok_or_else(|| {
            CliError::Config(
                "There is no agentd.toml to take the principal from: name it with --principal."
                    .to_owned(),
            )
        })?;
    let choices = SeedChoices::new(
        args.drive,
        &principal,
        args.owner,
        args.readers,
        args.local_only,
        args.bot,
        args.with,
    )
    .map_err(CliError::Config)?;
    // The flags against the pin before anything is cloned or written; the
    // zone's own `_drive.toml`, once it can be read, is checked again below.
    let pin = config.as_ref().and_then(|c| c.drive(args.drive));
    match (pin, args.into) {
        (Some(pin), _) => choices
            .check_hosting(&choices.decl, Some(pin))
            .map_err(CliError::Config)?,
        (None, None) => {
            return Err(CliError::Config(format!(
                "{} is not a drive this host mounts: pin it in agentd.toml's [[drives]], or seed another checkout with --into <dir>.",
                args.drive
            )))
        }
        (None, Some(_)) => {}
    }

    let Some(into) = args.into else {
        // Checked above: without --into there is a config and a pin.
        let config = config.ok_or_else(|| CliError::Config("agentd.toml".to_owned()))?;
        return init_checkout(host, &config, &choices, args);
    };
    let host_name = config.as_ref().map(|c| c.host.as_str()).unwrap_or_default();
    let zone = zone_of_checkout(into, args.drive, host_name)?;
    let hosting = seed::check_declared(&choices, &zone).map_err(CliError::Config)?;
    choices
        .check_hosting(&hosting, pin)
        .map_err(CliError::Config)?;
    let applied = seed::apply(seed::plan_at(&choices, &zone), &zone).map_err(CliError::Failure)?;
    say(&applied);
    match choices.proxy() {
        Some(proxy) => println!(
            "Wrote the zone only. Once it is in agentd's checkout of {}, sign the proxy in and make {}'s DM with:\n  keeper-agentd login {}/{}\n  {}",
            args.drive,
            proxy.name,
            args.drive,
            proxy.id,
            follow_up(args)
        ),
        None => println!("Wrote the zone only."),
    }
    Ok(())
}

/// `agents init` against agentd's own checkout of the drive. It holds the
/// copies' lock throughout: the checkout is `run`'s, and the DM signs the
/// proxy's copy in from its store, which only one process may hold.
fn init_checkout(
    host: &Host,
    config: &AgentdConfig,
    choices: &SeedChoices,
    args: &InitArgs<'_>,
) -> Result<(), CliError> {
    let _lock = crate::host_lock::take(&host.dirs.data, &config.host)
        .map_err(|error| {
            CliError::Failure(format!(
                "{} cannot be locked: {error}",
                crate::host_lock::path(&host.dirs.data).display()
            ))
        })?
        .map_err(|holder| {
            CliError::Config(format!(
                "keeper-agentd run is serving {principal}'s agents on {host} ({holder}), and agents init would open the same copies beside it: stop keeper-agentd@{principal}, run this again, then start it.",
                principal = config.principal,
                host = config.host,
                holder = holder.0,
            ))
        })?;
    let secrets = host.harden(config)?;
    let runtime = runtime()?;
    runtime.block_on(check_out_missing(config, &host.dirs, &secrets))?;
    let pin = config
        .drive(args.drive)
        .ok_or_else(|| CliError::Config(format!("{} is not pinned", args.drive)))?;
    let profile = in_force(&drive_profile(&host.dirs, pin), &config.host)?;
    let zone = profile
        .agents_root()
        .ok_or_else(|| CliError::Failure("the drive has no agents zone".to_owned()))?;
    let hosting = seed::check_declared(choices, &zone).map_err(CliError::Config)?;
    choices
        .check_hosting(&hosting, Some(pin))
        .map_err(CliError::Config)?;
    let applied = seed::apply(seed::plan_at(choices, &zone), &zone).map_err(CliError::Failure)?;
    say(&applied);

    let Some(proxy) = choices.proxy() else {
        return Ok(());
    };
    let inspection = inspect(config, &host.dirs);
    let Some(home) = inspection
        .hosted
        .iter()
        .find(|home| home.config.drive == args.drive && home.config.id == proxy.id)
    else {
        println!(
            "{} is not served here yet, so there is no DM: name it in [[agents]] (drive = \"{}\"), check `keeper-agentd agents list`, sign it in with `keeper-agentd login {}/{}`, then run this again.",
            proxy.name, args.drive, args.drive, proxy.id
        );
        return Ok(());
    };
    let sessions = profile
        .sessions_root()
        .ok_or_else(|| CliError::Failure("the drive has no sessions zone".to_owned()))?;
    let subfolder = profile
        .sessions
        .as_ref()
        .map(|s| s.subfolder.trim().to_owned())
        .unwrap_or_default();
    let platform = keeper_agent::headless::HeadlessPlatform::new(&host.dirs.data, secrets);
    let made = runtime.block_on(async {
        let Some(client) =
            restore_copy(config, &platform, &host.dirs.data, &home.config.matrix_user)
                .await
                .map_err(CliError::Failure)?
        else {
            return Ok(None);
        };
        seed::main_dm(
            &client,
            &home.config,
            &home.drive,
            &sessions,
            &subfolder,
            &config.host,
            chrono::Local::now(),
        )
        .await
        .map(Some)
        .map_err(CliError::Failure)
    })?;
    let human = home
        .config
        .human
        .as_ref()
        .map(|h| h.as_str())
        .unwrap_or_default();
    match made {
        None => println!(
            "{} is not signed in here yet, so there is no DM: run `keeper-agentd login {}/{}`, then this again.",
            home.config.matrix_user, args.drive, proxy.id
        ),
        Some(dm) => match dm.made {
            Made::RoomAndFolder => println!(
                "Made {}'s DM {} with {human}, and its main session {subfolder}/{}.",
                proxy.name, dm.room, dm.path
            ),
            Made::Folder => println!(
                "{}'s DM with {human} is {} already, and had no main session here: made {subfolder}/{} naming it.",
                proxy.name, dm.room, dm.path
            ),
            Made::Nothing => println!(
                "{}'s DM is {} already, with its main session {subfolder}/{}: nothing made.",
                proxy.name, dm.room, dm.path
            ),
        },
    }
    println!("`keeper-agentd run` commits and pushes what this wrote.");
    Ok(())
}

/// `agents new`'s flags.
pub struct NewArgs<'a> {
    pub id: &'a str,
    pub drive: Option<&'a str>,
    pub name: Option<&'a str>,
    pub from_bmad: Option<&'a Path>,
    pub into: Option<&'a Path>,
}

/// The project a BMAD skill folder belongs to: its nearest ancestor holding
/// `_bmad/`, whose `_bmad/custom/` layers merge over the skill's own.
fn bmad_project(skill: &Path) -> Option<PathBuf> {
    skill
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("_bmad").is_dir())
        .map(Path::to_owned)
}

/// `agents new`.
pub fn new(host: &Host, args: &NewArgs<'_>) -> Result<(), CliError> {
    let zone = match args.into {
        Some(into) => {
            let config = config_if_any(host)?;
            let host_name = config.as_ref().map(|c| c.host.as_str()).unwrap_or_default();
            zone_of_checkout(into, args.drive.unwrap_or("the drive"), host_name)?
        }
        None => {
            let config = host.config()?;
            let pin = match args.drive {
                Some(id) => config.drive(id).ok_or_else(|| {
                    CliError::Config(format!("{id} is not in agentd.toml's [[drives]]."))
                })?,
                None => match config.drives.as_slice() {
                    [only] => only,
                    _ => {
                        return Err(CliError::Config(
                            "This host mounts more than one drive: name one with --drive."
                                .to_owned(),
                        ))
                    }
                },
            };
            in_force(&drive_profile(&host.dirs, pin), &config.host)?
                .agents_root()
                .ok_or_else(|| CliError::Failure("the drive has no agents zone".to_owned()))?
        }
    };
    let import = match args.from_bmad {
        Some(skill) => {
            let merged = keeper_ported::bmad::config::load_customization(
                bmad_project(skill).as_deref(),
                skill,
            )
            .map_err(|error| {
                CliError::Config(format!("{} cannot be read: {error}", skill.display()))
            })?;
            let imported =
                soul::soul_from_bmad(&merged).map_err(|r| CliError::Config(r.to_string()))?;
            let name = merged
                .get("agent")
                .and_then(|agent| agent.get("name"))
                .and_then(|name| name.as_str())
                .unwrap_or_default()
                .to_owned();
            Some((imported, name))
        }
        None => None,
    };
    let name = match (&import, args.name) {
        (Some((_, imported)), Some(asked)) if imported != asked => {
            return Err(CliError::Config(format!(
                "The BMAD agent is named {imported}, and its soul and agent.toml must say the same name: leave --name out or write --name \"{imported}\"."
            )))
        }
        (Some((_, imported)), _) => imported.clone(),
        (None, Some(asked)) => asked.to_owned(),
        (None, None) => args.id.to_owned(),
    };
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let applied = seed::new_agent(
        &zone,
        args.id,
        &name,
        &date,
        import.as_ref().map(|(imported, _)| imported.text.as_str()),
    )
    .map_err(CliError::Config)?;
    say(&applied);
    if let Some((imported, _)) = &import {
        if imported.not_imported.is_empty() {
            println!("Everything the BMAD agent says was imported.");
        } else {
            println!("Not imported:");
            for line in &imported.not_imported {
                println!("  {line}");
            }
        }
    }
    Ok(())
}

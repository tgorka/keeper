//! `keeper-agentd agents list` and `keeper-agentd status`: what this host
//! mounts, hosts, serves and offers, in Epic 89's sentences.

use std::sync::Arc;

use keeper_agent::agent::{arm_agent, Probe, SessionContext, SessionRef};
use keeper_agent::claims::{conflict_line, conflict_of};
use keeper_agent::headless::{HeadlessPlatform, SecretMap};
use keeper_agent::runtime::{agent_deps, inspect, Inspection, STATUS_FILE};
use keeper_agent::zone::{skills_of, AgentHome};
use keeper_core::agents::agentd::TrustEntry;
use keeper_core::agents::log::HostSlug;
use keeper_core::agents::matrix;
use keeper_core::agents::prompt;
use keeper_core::agents::trust::fingerprint;
use keeper_core::bots::store;
use keeper_core::bots::tools::ToolName;
use serde_json::Value;

use crate::cli::{runtime, split_ref, CliError, Host};

/// Whether `home`'s copy has a stored session on this host.
fn signed_in(secrets: &SecretMap, home: &AgentHome) -> bool {
    secrets
        .get(&matrix::session_key(&home.config.matrix_user))
        .ok()
        .flatten()
        .is_some()
}

fn copy_line(secrets: &SecretMap, home: &AgentHome) -> String {
    if signed_in(secrets, home) {
        format!("{} is signed in", home.config.matrix_user)
    } else {
        format!(
            "{} is not signed in: run `keeper-agentd login {}/{}`",
            home.config.matrix_user, home.config.drive, home.config.id
        )
    }
}

/// `agents list`.
pub fn agents_list(host: &Host) -> Result<(), CliError> {
    let config = host.config()?;
    let secrets = host.harden(&config)?;
    let inspection = inspect(&config, &host.dirs);
    for drive in &inspection.drives {
        match &drive.hosts {
            Ok(decl) => println!("{} ({}): its agents zone hosts", drive.id, decl.title),
            Err(sentence) => println!("{}: hosts nothing. {sentence}", drive.id),
        }
        if let Some(sentence) = &drive.zone.assessment.hosts_nothing_because {
            if drive.hosts.is_ok() {
                println!("  {sentence}");
            }
        }
        for (folder, home) in &drive.zone.homes {
            match home {
                Ok(home) => {
                    let served = config
                        .agents
                        .iter()
                        .any(|e| e.drive == drive.id && e.ids.iter().any(|id| id == folder));
                    println!(
                        "  {folder}: {} ({}), {}",
                        home.config.name,
                        home.config.kind.as_str(),
                        if served {
                            "served here"
                        } else {
                            "not in [[agents]], so not served here"
                        }
                    );
                    for (skill, problems) in &skills_of(home).refused {
                        println!("    skill {skill} refused: {}", problems.join(" "));
                    }
                    if served {
                        println!("    {}", copy_line(&secrets, home));
                    }
                    for found in drive.sessions.iter().filter(|found| {
                        found
                            .agent
                            .as_ref()
                            .is_ok_and(|agent| agent.agent == *folder)
                    }) {
                        if let Some(conflict) = conflict_of(&found.dir) {
                            println!("    session {}: {}", found.path, conflict_line(&conflict));
                        }
                    }
                }
                Err(sentence) => println!("  {folder}: refused. {sentence}"),
            }
        }
    }
    Ok(())
}

/// The running host's status file, when `run` has written one.
fn running(host: &Host) -> Option<Value> {
    let text = std::fs::read_to_string(host.dirs.state.join(STATUS_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The tools an agent's `[tools].allow` names, split into those this host
/// implements and those it does not.
fn tools(home: &AgentHome) -> (Vec<&str>, Vec<&str>) {
    home.config
        .allow
        .iter()
        .map(String::as_str)
        .partition(|name| {
            keeper_agent::delegate::is_delegation(name)
                || keeper_agent::cards::is_card_tool(name)
                || keeper_agent::bmad::serves(name)
                || *name == keeper_core::agents::workflow::ASK_HUMAN
                || *name == keeper_core::agents::workflow::WORKFLOW_START
                || *name == keeper_core::agents::helper::HELPER
                || ToolName::ALL.iter().any(|tool| tool.as_wire() == *name)
        })
}

/// `status`. With `--session`, `probe: false` composes without asking the
/// provider what its model supports, so the verb reaches nothing.
pub fn status(host: &Host, session: Option<&str>, probe: bool) -> Result<(), CliError> {
    let config = host.config()?;
    let secrets = host.harden(&config)?;
    let inspection = inspect(&config, &host.dirs);
    if let Some(session) = session {
        return told(host, &config, secrets, &inspection, session, probe);
    }
    println!("host {} (principal {})", config.host, config.principal);
    let live = running(host);
    match &live {
        Some(status) => println!(
            "running; status written {}",
            status["updated_at"].as_str().unwrap_or("unknown")
        ),
        None => println!(
            "not running: no {} in {}",
            STATUS_FILE,
            host.dirs.state.display()
        ),
    }
    for drive in &inspection.drives {
        let engine = live
            .as_ref()
            .and_then(|status| status["drives"].as_array())
            .and_then(|drives| drives.iter().find(|d| d["drive"] == drive.id.as_str()))
            .map(|d| {
                format!(
                    "{} {}, {} pending",
                    d["state"].as_str().unwrap_or("?"),
                    d["phase"].as_str().unwrap_or("?"),
                    d["pending"]
                )
            })
            .unwrap_or_else(|| "engine not running".to_owned());
        let mount = match &drive.hosts {
            Ok(_) => "mounted; its zone matches the pin".to_owned(),
            Err(sentence) => format!("hosts nothing: {sentence}"),
        };
        println!("drive {}: {engine}; {mount}", drive.id);
    }
    for home in &inspection.hosted {
        println!(
            "agent {}/{}: {}",
            home.config.drive,
            home.config.id,
            copy_line(&secrets, home)
        );
        let copy = live
            .as_ref()
            .and_then(|status| status["copies"].as_array())
            .and_then(|copies| {
                copies
                    .iter()
                    .find(|c| c["user"] == home.config.matrix_user.as_str())
            });
        let listed = |key: &str, path: &str| {
            copy.and_then(|copy| copy[key].as_array())
                .and_then(|entries| entries.iter().find(|e| e["session"] == path))
                .cloned()
        };
        for found in inspection
            .drives
            .iter()
            .filter(|d| d.id == home.config.drive)
            .flat_map(|d| d.sessions.iter())
        {
            let Ok(agent) = &found.agent else { continue };
            if agent.agent != home.config.id {
                continue;
            }
            let claim = live
                .as_ref()
                .and_then(|status| status["claims"].as_array())
                .and_then(|claims| claims.iter().find(|c| c["room"] == agent.room.as_str()))
                .map(|c| {
                    format!(
                        ", claim epoch {} ({})",
                        c["epoch"],
                        c["claim_event"].as_str().unwrap_or("?")
                    )
                })
                .unwrap_or_default();
            let state = if let Some(conflict) = conflict_of(&found.dir) {
                conflict_line(&conflict)
            } else if listed("sessions", &found.path).is_some() {
                format!("served{claim}")
            } else if let Some(entry) = listed("unserved", &found.path) {
                format!(
                    "not served: {} names the same room",
                    entry["room_served_for"]
                        .as_str()
                        .unwrap_or("another session")
                )
            } else {
                "not served here now".to_owned()
            };
            println!(
                "  session {} ({}): {state}",
                found.path,
                agent.kind.as_str()
            );
        }
        let (offered, missing) = tools(home);
        println!("  tools offered: {}", offered.join(", "));
        if !missing.is_empty() {
            println!("  not offered on this host: {}", missing.join(", "));
        }
    }
    for line in trust_lines(&config.trust, live.as_ref()) {
        println!("{line}");
    }
    Ok(())
}

/// Each `[[trust]]` person as `status` prints them (R88): the running
/// host's reading of the master key their homeserver publishes, as a
/// fingerprint, against the pin that reading was judged on — the running
/// host's own, beside `agentd.toml`'s when a person changed it since the
/// host started. `status` itself reaches nothing and writes nothing:
/// without a running host it says so.
fn trust_lines(entries: &[TrustEntry], live: Option<&Value>) -> Vec<String> {
    let print = |key: &Value| key.as_str().map_or("none".to_owned(), fingerprint);
    entries
        .iter()
        .map(|entry| {
            let read = live
                .and_then(|status| status["trust"].as_array())
                .and_then(|lines| {
                    lines
                        .iter()
                        .find(|line| line["user"] == entry.user.as_str())
                });
            let pinned = entry
                .master_key
                .as_deref()
                .map_or("none".to_owned(), fingerprint);
            match (live, read) {
                (None, _) => format!("trust {}: not running (pinned {pinned})", entry.user),
                (Some(_), None) => format!("trust {}: not read yet (pinned {pinned})", entry.user),
                (Some(_), Some(line)) => {
                    let configured = entry.master_key.as_deref();
                    let restart = if line["pinned"].as_str() == configured {
                        String::new()
                    } else {
                        format!("; agentd.toml now pins {pinned}, used after a restart")
                    };
                    format!(
                        "trust {}: {}; published {}, pinned {}{}{restart}",
                        entry.user,
                        line["state"].as_str().unwrap_or("unknown"),
                        print(&line["published"]),
                        print(&line["pinned"]),
                        line["error"]
                            .as_str()
                            .map(|error| format!(" ({error})"))
                            .unwrap_or_default(),
                    )
                }
            }
        })
        .collect()
}

/// `status --session <drive>/<path>`: what the session's agent is told, as
/// this host composes it now, and whether its digest is the last `open`
/// line's (FR-771's person-facing half).
fn told(
    host: &Host,
    config: &keeper_core::agents::agentd::AgentdConfig,
    secrets: Arc<SecretMap>,
    inspection: &Inspection,
    session: &str,
    probe: bool,
) -> Result<(), CliError> {
    let (drive, path) = split_ref(session)?;
    let view = inspection
        .drives
        .iter()
        .find(|d| d.id == drive)
        .ok_or_else(|| CliError::Config(format!("{drive} is not a mounted drive")))?;
    let found = view
        .sessions
        .iter()
        .find(|found| found.path == path)
        .ok_or_else(|| CliError::Config(format!("{session} is not an active session")))?;
    let agent = found.agent.clone().map_err(CliError::Config)?;
    let home = inspection
        .hosted
        .iter()
        .find(|home| home.config.id == agent.agent && home.config.drive == agent.drive)
        .ok_or_else(|| {
            CliError::Config(format!("{}'s agent is not served by this host", found.path))
        })?;
    let platform = Arc::new(HeadlessPlatform::new(&host.dirs.data, secrets));
    let slug = HostSlug::new(&config.host).map_err(|e| CliError::Config(e.to_string()))?;
    let rows = store::list_providers(&host.dirs.data)
        .map_err(|error| CliError::Failure(error.to_string()))?
        .rows;
    let deps = agent_deps(
        &platform,
        &host.dirs.data,
        &slug,
        &inspection.drives,
        &rows,
        home,
    )
    .map_err(CliError::Config)?;
    let context = SessionContext::load(
        home,
        &found.dir,
        SessionRef {
            drive: drive.to_owned(),
            path: path.to_owned(),
        },
        agent,
        0,
        None,
        chrono::Local::now().fixed_offset(),
    )
    .map_err(|error| CliError::Failure(error.to_string()))?;
    let probe = if probe { Probe::Ask } else { Probe::Skip };
    let armed = runtime()?.block_on(arm_agent(&context, &deps, probe));
    let composed = context.compose(&deps, armed.context.as_ref(), &armed.request.tools);
    let told = prompt::told(&composed);
    for section in &told.sections {
        println!("## {} {}\n{}", section.slot, section.title, section.text);
    }
    for note in &told.notes {
        println!("note: {note}");
    }
    match &context.open {
        Some(open) if open.prompt_sha256 == told.prompt_sha256 => {
            println!("prompt {} matches the last open line", told.prompt_sha256);
        }
        Some(open) => println!(
            "prompt {} differs from the last open line's {}: the next turn writes a new open line",
            told.prompt_sha256, open.prompt_sha256
        ),
        None => println!(
            "prompt {}; the session has no open line yet",
            told.prompt_sha256
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::OwnedUserId;
    use serde_json::json;

    use super::*;

    const KEY: &str = "ed25519:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";
    const RESET: &str = "ed25519:ZZZZEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";

    fn entry(user: &str, key: Option<&str>) -> TrustEntry {
        TrustEntry {
            user: OwnedUserId::try_from(user).expect("user"),
            master_key: key.map(str::to_owned),
            proxy: None,
        }
    }

    /// 93.3 AC7 (R88): `status` prints each pin against what the running
    /// host read — matches, differs after a reset, not pinned with the
    /// fingerprint published now — and says when no host runs.
    #[test]
    fn status_says_whether_each_pin_still_matches() {
        let entries = [
            entry("@tgorka:h", Some(KEY)),
            entry("@marta:h", Some(KEY)),
            entry("@nobody:h", None),
        ];
        let live = json!({"trust": [
            {"user": "@tgorka:h", "published": KEY, "pinned": KEY, "state": "matches"},
            {"user": "@marta:h", "published": RESET, "pinned": KEY, "state": "differs"},
            {"user": "@nobody:h", "published": KEY, "pinned": null, "state": "not pinned"},
        ]});
        let pinned = fingerprint(KEY);
        assert_eq!(
            trust_lines(&entries, Some(&live)),
            [
                format!("trust @tgorka:h: matches; published {pinned}, pinned {pinned}"),
                format!(
                    "trust @marta:h: differs; published {}, pinned {pinned}",
                    fingerprint(RESET)
                ),
                format!("trust @nobody:h: not pinned; published {pinned}, pinned none"),
            ]
        );
        assert_eq!(
            trust_lines(&entries[..1], None),
            [format!("trust @tgorka:h: not running (pinned {pinned})")]
        );
        assert_eq!(
            trust_lines(&entries[..1], Some(&json!({}))),
            [format!("trust @tgorka:h: not read yet (pinned {pinned})")]
        );
    }

    /// R3-08: a person re-pins in `agentd.toml` while the host runs. The
    /// verdict printed is the running host's, beside the pin it judged —
    /// never the edited file's — and the edited pin is named as waiting for
    /// a restart.
    #[test]
    fn status_prints_the_pin_the_running_host_judged() {
        let live = json!({"trust": [
            {"user": "@tgorka:h", "published": KEY, "pinned": KEY, "state": "matches"},
        ]});
        let (old, new) = (fingerprint(KEY), fingerprint(RESET));
        assert_eq!(
            trust_lines(&[entry("@tgorka:h", Some(RESET))], Some(&live)),
            [format!(
                "trust @tgorka:h: matches; published {old}, pinned {old}; agentd.toml now pins {new}, used after a restart"
            )]
        );
        assert_eq!(
            trust_lines(&[entry("@tgorka:h", None)], Some(&live)),
            [format!(
                "trust @tgorka:h: matches; published {old}, pinned {old}; agentd.toml now pins none, used after a restart"
            )]
        );
    }
}

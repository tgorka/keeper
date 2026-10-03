//! Where a session runs: a pure function of its needs, its data, the live
//! hosts and its principal (AD-379; story 90.6).
//!
//! Every host evaluates the same function over the same manifests, so two
//! hosts that see the same facts name the same host, and each claims only
//! what it wins. What no host can serve waits, and says what for.

use std::fmt;

use crate::agents::host::{HostManifest, Materialized};

/// What a host lacks for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// A capability: `sandbox`, `mcp:<name>`, `screen:mac`, `kvm:<id>`, `voice`.
    Capability(String),
    /// A drive in scope that is not checked out there.
    Drive(String),
    /// A drive checked out there with its content not on disk.
    Materialized(String),
    /// The agent's model.
    Bot,
    /// A copy of the agent, `<drive>/<agent>`, signed in there.
    Agent(String),
    /// Any live host of the principal.
    LiveHost,
}

impl fmt::Display for Need {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Need::Capability(name) => f.write_str(name),
            Need::Drive(id) => write!(f, "drive {id}"),
            Need::Materialized(id) => write!(f, "drive {id} on disk"),
            Need::Bot => f.write_str("the agent's model"),
            Need::Agent(agent) => write!(f, "a copy of {agent}"),
            Need::LiveHost => f.write_str("a live host"),
        }
    }
}

/// One session's facts.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The capabilities it needs.
    pub needs: &'a [String],
    /// The host it is pinned to.
    pub pin: Option<&'a str>,
    /// The drives in its scope.
    pub drives: &'a [String],
    /// The agent, `<drive>/<agent>`: a host serves only an agent it hosts.
    pub agent: &'a str,
    /// The agent's [`crate::agents::host::bot_id`].
    pub bot: &'a str,
    pub principal: &'a str,
    pub prefer_always_on: bool,
    /// The host of the session's most recent claim.
    pub holder: Option<&'a str>,
}

/// Where the session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    Host(String),
    /// No host can serve it: the pinned host when there is a pin, and what is
    /// missing.
    Waiting {
        host: Option<String>,
        missing: Vec<Need>,
    },
}

impl Placement {
    /// What a waiting session's status says after `waiting: `:
    /// `<host> — <need>`, or the need alone.
    pub fn waiting_text(&self) -> Option<String> {
        let Placement::Waiting { host, missing } = self else {
            return None;
        };
        let need = missing
            .first()
            .map_or_else(|| Need::LiveHost.to_string(), Need::to_string);
        Some(match host {
            Some(host) => format!("{host} — {need}"),
            None => need,
        })
    }
}

/// What `host` lacks for `ask`, in the order a person fixes it: capabilities,
/// drives, their content, the model, a copy of the agent.
fn missing(ask: &Ask<'_>, host: &HostManifest) -> Vec<Need> {
    let mut missing: Vec<Need> = ask
        .needs
        .iter()
        .filter(|need| !host.tools.contains(need))
        .map(|need| Need::Capability(need.clone()))
        .collect();
    for drive in ask.drives {
        match host.drives.iter().find(|d| d.id == *drive) {
            Some(d) if d.present && d.materialized != Materialized::Virtual => {}
            Some(d) if d.present => missing.push(Need::Materialized(d.id.clone())),
            _ => missing.push(Need::Drive(drive.clone())),
        }
    }
    if !host.bots.iter().any(|bot| bot == ask.bot) {
        missing.push(Need::Bot);
    }
    if !host.agents.iter().any(|agent| agent == ask.agent) {
        missing.push(Need::Agent(ask.agent.to_owned()));
    }
    missing
}

/// Place one session among `hosts` at `server_now` (ms).
///
/// Candidates are the principal's live hosts that host the agent and lack
/// nothing; a pin keeps only the pinned one. Among candidates an always-on
/// host comes first when the agent prefers one, then the holder of the
/// session's last claim, then the lowest slug.
pub fn place(ask: &Ask<'_>, hosts: &[HostManifest], server_now: u64) -> Placement {
    let mut live: Vec<&HostManifest> = hosts
        .iter()
        .filter(|host| host.principal == ask.principal && host.is_live(server_now))
        .filter(|host| ask.pin.is_none_or(|pin| host.host == pin))
        .collect();
    live.sort_by(|a, b| a.host.cmp(&b.host));
    let mut candidates: Vec<&HostManifest> = live
        .iter()
        .copied()
        .filter(|host| missing(ask, host).is_empty())
        .collect();
    candidates.sort_by_key(|host| {
        (
            ask.prefer_always_on && !host.always_on,
            ask.holder != Some(host.host.as_str()),
            host.host.clone(),
        )
    });
    if let Some(winner) = candidates.first() {
        return Placement::Host(winner.host.clone());
    }
    let missing = live
        .iter()
        .map(|host| missing(ask, host))
        .min_by_key(Vec::len)
        .unwrap_or_else(|| vec![Need::LiveHost]);
    Placement::Waiting {
        host: ask.pin.map(str::to_owned),
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::claim::rfc3339;
    use crate::agents::home::BotRef;
    use crate::agents::host::{bot_id, HostDrive};

    const NOW: u64 = 1_790_000_000_000;
    const OPUS: &str = "bot:openai:https://provider.example:8452#claude-opus";

    fn bot(text: &str) -> String {
        bot_id(&BotRef::parse(text).expect("bot"))
    }

    fn host(slug: &str, always_on: bool) -> HostManifest {
        HostManifest {
            v: 1,
            host: slug.to_owned(),
            principal: "tgorka".to_owned(),
            version: "0.90.0".to_owned(),
            always_on,
            tools: vec!["sandbox".to_owned()],
            drives: vec![HostDrive {
                id: "tgdrive".to_owned(),
                present: true,
                materialized: Materialized::Full,
            }],
            bots: vec![bot(OPUS)],
            agents: vec!["tgdrive/nixi".to_owned()],
            renewed_at: rfc3339(NOW - 30_000),
            expires_at: rfc3339(NOW + 150_000),
        }
    }

    #[derive(Clone)]
    struct Facts {
        needs: Vec<String>,
        pin: Option<&'static str>,
        drives: Vec<String>,
        bot: String,
        prefer_always_on: bool,
        holder: Option<&'static str>,
    }

    fn facts() -> Facts {
        Facts {
            needs: vec!["sandbox".to_owned()],
            pin: None,
            drives: vec!["tgdrive".to_owned()],
            bot: bot(OPUS),
            prefer_always_on: true,
            holder: None,
        }
    }

    fn place_with(facts: &Facts, hosts: &[HostManifest]) -> Placement {
        place(
            &Ask {
                needs: &facts.needs,
                pin: facts.pin,
                drives: &facts.drives,
                agent: "tgdrive/nixi",
                bot: &facts.bot,
                principal: "tgorka",
                prefer_always_on: facts.prefer_always_on,
                holder: facts.holder,
            },
            hosts,
            NOW,
        )
    }

    fn at(slug: &str) -> Placement {
        Placement::Host(slug.to_owned())
    }

    fn waiting(host: Option<&str>, missing: Vec<Need>) -> Placement {
        Placement::Waiting {
            host: host.map(str::to_owned),
            missing,
        }
    }

    #[test]
    fn placement_table() {
        let electra = host("electra", true);
        let hesperia = host("hesperia", false);
        let mut dead = host("electra", true);
        dead.expires_at = rfc3339(NOW);
        let mut foreign = host("marta-box", true);
        foreign.principal = "marta".to_owned();
        let mut mac = host("hesperia", false);
        mac.tools.push("screen:mac".to_owned());
        let mut virtual_drive = host("electra", true);
        virtual_drive.drives[0].materialized = Materialized::Virtual;
        let mut no_drive = host("electra", true);
        no_drive.drives.clear();
        let mut partial = host("electra", true);
        partial.drives[0].materialized = Materialized::Partial;
        // A specialist signed in only on the Mac: the always-on host has the
        // drive and resolves the bot, but runs no copy of it.
        let mut no_copy = host("electra", true);
        no_copy.agents = vec!["tgdrive/amelia".to_owned()];

        let pinned_mac = Facts {
            pin: Some("hesperia"),
            ..facts()
        };
        let screen = Facts {
            needs: vec!["screen:mac".to_owned()],
            ..facts()
        };
        let pinned_screen = Facts {
            pin: Some("hesperia"),
            needs: vec!["screen:mac".to_owned()],
            ..facts()
        };
        let other_bot = Facts {
            bot: bot("bot:ollama:http://electra.example.org:11434#qwen3:32b"),
            ..facts()
        };
        let not_always = Facts {
            prefer_always_on: false,
            ..facts()
        };
        let held_by_hesperia = Facts {
            holder: Some("hesperia"),
            prefer_always_on: false,
            ..facts()
        };
        let rows: Vec<(&str, Facts, Vec<HostManifest>, Placement)> = vec![
            (
                "one live host",
                facts(),
                vec![electra.clone()],
                at("electra"),
            ),
            (
                "a missing capability",
                screen.clone(),
                vec![electra.clone()],
                waiting(None, vec![Need::Capability("screen:mac".to_owned())]),
            ),
            (
                "the capability offered",
                screen.clone(),
                vec![electra.clone(), mac.clone()],
                at("hesperia"),
            ),
            (
                "a drive not checked out",
                facts(),
                vec![no_drive],
                waiting(None, vec![Need::Drive("tgdrive".to_owned())]),
            ),
            (
                "a drive present, its zone not materialised",
                facts(),
                vec![virtual_drive],
                waiting(None, vec![Need::Materialized("tgdrive".to_owned())]),
            ),
            (
                "a partial drive holds the zone",
                facts(),
                vec![partial],
                at("electra"),
            ),
            (
                "an unresolvable bot",
                other_bot.clone(),
                vec![electra.clone()],
                waiting(None, vec![Need::Bot]),
            ),
            (
                "another principal's host",
                facts(),
                vec![foreign],
                waiting(None, vec![Need::LiveHost]),
            ),
            (
                "a dead host",
                facts(),
                vec![dead.clone()],
                waiting(None, vec![Need::LiveHost]),
            ),
            (
                "a pin",
                pinned_mac.clone(),
                vec![electra.clone(), hesperia.clone()],
                at("hesperia"),
            ),
            (
                "a pin to a dead host",
                pinned_mac.clone(),
                vec![electra.clone()],
                waiting(Some("hesperia"), vec![Need::LiveHost]),
            ),
            (
                "a pin to a host that lacks a need",
                pinned_screen.clone(),
                vec![hesperia.clone(), electra.clone()],
                waiting(
                    Some("hesperia"),
                    vec![Need::Capability("screen:mac".to_owned())],
                ),
            ),
            (
                "always-on preferred",
                held_by_hesperia_but_always_on(),
                vec![hesperia.clone(), electra.clone()],
                at("electra"),
            ),
            (
                "the claim holder second",
                held_by_hesperia.clone(),
                vec![electra.clone(), hesperia.clone()],
                at("hesperia"),
            ),
            (
                "the lowest slug last",
                not_always.clone(),
                vec![hesperia.clone(), electra.clone()],
                at("electra"),
            ),
            (
                "the only host that hosts the agent is not always-on",
                facts(),
                vec![no_copy.clone(), hesperia.clone()],
                at("hesperia"),
            ),
            (
                "no live host hosts the agent",
                facts(),
                vec![no_copy.clone()],
                waiting(None, vec![Need::Agent("tgdrive/nixi".to_owned())]),
            ),
            (
                "a pin to a host without the agent",
                Facts {
                    pin: Some("electra"),
                    ..facts()
                },
                vec![no_copy, hesperia.clone()],
                waiting(
                    Some("electra"),
                    vec![Need::Agent("tgdrive/nixi".to_owned())],
                ),
            ),
        ];
        for (name, facts, hosts, expected) in rows {
            assert_eq!(place_with(&facts, &hosts), expected, "{name}");
        }
    }

    fn held_by_hesperia_but_always_on() -> Facts {
        Facts {
            holder: Some("hesperia"),
            ..facts()
        }
    }

    #[test]
    fn waiting_names_the_pin_and_the_first_missing_need() {
        let facts = Facts {
            needs: vec!["screen:mac".to_owned(), "voice".to_owned()],
            pin: Some("hesperia-sim"),
            ..facts()
        };
        let placed = place_with(&facts, &[host("hesperia-sim", false)]);
        assert_eq!(
            placed.waiting_text().as_deref(),
            Some("hesperia-sim — screen:mac")
        );
        assert_eq!(
            waiting(None, vec![Need::Agent("tgdrive/nixi".to_owned())])
                .waiting_text()
                .as_deref(),
            Some("a copy of tgdrive/nixi")
        );
        assert_eq!(at("electra").waiting_text(), None);
    }

    #[test]
    fn two_hosts_place_alike() {
        // Each host lacks a different one of two needs, so a session needing
        // both waits, and every host must name the same missing need. The
        // rosters differ too: zephyr, always on, runs no copy of the agent.
        let mut hosts = [
            host("hesperia", false),
            host("electra", true),
            host("argo", false),
            host("zephyr", true),
        ];
        hosts[0].tools.push("screen:mac".to_owned());
        hosts[1].tools.push("voice".to_owned());
        hosts[3].agents.clear();
        let zephyr_held = Facts {
            holder: Some("zephyr"),
            ..facts()
        };
        assert_eq!(place_with(&zephyr_held, &hosts), at("electra"));
        let asks = [
            facts(),
            Facts {
                prefer_always_on: false,
                ..facts()
            },
            Facts {
                holder: Some("zephyr"),
                ..facts()
            },
            Facts {
                holder: Some("hesperia"),
                prefer_always_on: false,
                ..facts()
            },
            Facts {
                needs: vec!["screen:mac".to_owned(), "voice".to_owned()],
                ..facts()
            },
        ];
        for ask in &asks {
            let first = place_with(ask, &hosts);
            // Every order a host could list its manifests in.
            for rotation in 0..hosts.len() {
                let mut seen = hosts.to_vec();
                seen.rotate_left(rotation);
                assert_eq!(place_with(ask, &seen), first);
                seen.reverse();
                assert_eq!(place_with(ask, &seen), first);
            }
        }
    }

    #[test]
    fn placement_matches_bots_by_id() {
        let electra = host("electra", true);
        assert_eq!(
            place_with(&facts(), std::slice::from_ref(&electra)),
            at("electra")
        );
        let elsewhere = Facts {
            bot: bot("bot:openai:https://elsewhere.example:8452#claude-opus"),
            ..facts()
        };
        assert_eq!(
            place_with(&elsewhere, std::slice::from_ref(&electra)),
            waiting(None, vec![Need::Bot])
        );
        let slash = Facts {
            bot: bot("bot:openai:https://provider.example:8452/#claude-opus"),
            ..facts()
        };
        assert_eq!(place_with(&slash, &[electra]), at("electra"));
    }
}

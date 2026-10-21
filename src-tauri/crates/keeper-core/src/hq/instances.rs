//! Which instance of an agent answers, and who is online (AD-425, AD-427,
//! AD-433; FR-853, FR-854).
//!
//! An agent is one actor and each of its instances one client of it. What
//! keeper shows of them is read from the replicas alone: the instances from
//! a database's `clients` rows (kind `instance`) and their `agent_profiles`
//! rows (name, description, definition hash); whether each is online from
//! its presence on `dir:<database>`, which this device receives over its
//! own socket — so while a database does not sync here, its instances are
//! *unknown*, never offline; and who holds an open session from its
//! `claims` row.
//!
//! The person's choice — the default instance and an ordered fallback — is
//! an `instance_prefs` row in the person's own `actor` scope, one per
//! database the person and the agent share, since a row cannot cross
//! (AD-433, DW-E106-7). keeper writes the same row into every such database
//! the device holds, shows one choice (the newest row) and says when the
//! rows differ. The instances route by it themselves: the first of the
//! order whose presence is online claims a new session (keeper-hq's
//! `hq-runtime` `routing`), so keeper never picks an instance for a session
//! and never pings one. `agents.default` keeps its meaning — which agent
//! keeper opens at launch.
//!
//! An instance whose definition hash differs from its siblings' is marked
//! (AD-427): with a majority hash, those that differ from it; with none,
//! every instance, since each differs from the others.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::hq::live::{self, Claim};
use crate::hq::port::{Delivery, LinkState, RowOp};
use crate::hq::sessions::{authored, int, names, object, opt_text, read, text, Source};
use crate::hq::{lock, Hq, HqError};

/// Whether an instance is online, as far as this device can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentInstancePresenceVm {
    /// Present on its database's directory key now.
    Online,
    /// Not present, as this device's live socket sees it.
    Offline,
    /// None of its databases syncs on this device now: nobody here knows.
    Unknown,
}

/// One instance of an agent, in Settings › Agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentInstanceVm {
    /// Its client id: what `instance_prefs` names.
    pub client_id: String,
    /// How the person knows it (its client's name, as `nixie@hesperia`).
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub host: Option<String>,
    /// From its profile row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    /// The shared databases it is in.
    pub databases: Vec<String>,
    pub presence: AgentInstancePresenceVm,
    /// The start of its definition hash, once its profile row is here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub definition: Option<String>,
    /// Its definition differs from its siblings': the sentence saying so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub definition_differs: Option<String>,
    /// Its place in the person's order: 0 answers first, then the fallback
    /// in order; absent when the choice does not name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub rank: Option<u32>,
}

/// One agent's instances and the person's choice of which answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentInstancesVm {
    pub agent_actor: String,
    pub agent_name: String,
    pub agent_mark: String,
    /// The databases this device holds that the person and the agent share:
    /// where the choice is written.
    pub databases: Vec<String>,
    /// In the person's order, then the instances it does not name by label.
    pub instances: Vec<AgentInstanceVm>,
    /// The instance new sessions go to, as the newest row says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_client: Option<String>,
    /// The instances asked, in order, while the default is offline.
    pub fallback: Vec<String>,
    /// What the choice does, as the person reads it.
    pub sentence: String,
    /// The shared databases' rows differ: the sentence saying how.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub disagreement: Option<String>,
    /// The server's refusal of a choice written here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notice: Option<String>,
}

/// One instance of the session's agent in the session's header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentInstanceStateVm {
    pub client_id: String,
    pub label: String,
    pub presence: AgentInstancePresenceVm,
    /// It holds the session's claim.
    pub holds: bool,
}

/// Which instance is answering an open session, and each instance's
/// online state — from the session's own database alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionAnsweringVm {
    /// The claim holder's client id, while one holds the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub holder: Option<String>,
    pub sentence: String,
    /// The holder first, then by label.
    pub instances: Vec<AgentInstanceStateVm>,
}

/// The presence key an instance publishes on in `database`.
pub fn dir_key(database: &str) -> String {
    format!("dir:{database}")
}

/// The client ids present on a database's directory key, or `None` while
/// this device's socket to it is not up — when an empty list says nothing.
pub type Online = Option<HashSet<String>>;

/// Who is present on `source`'s directory key now.
pub fn online(source: &Source) -> Online {
    if source.replica.state().link != LinkState::Connected {
        return None;
    }
    match source.replica.presence(&dir_key(&source.database)) {
        Ok(peers) => Some(peers.into_iter().map(|peer| peer.client_id).collect()),
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "an agents directory presence could not be read");
            None
        }
    }
}

/// An instance as one database's rows describe it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Instance {
    pub client_id: String,
    pub label: String,
    pub host: Option<String>,
    pub description: Option<String>,
    pub hash: Option<String>,
    pub started_ms: i64,
}

/// The instances of `agent` in `source`'s database: its `clients` rows not
/// revoked, with what their `agent_profiles` rows say.
pub fn instances_of(source: &Source, agent: &str) -> Vec<Instance> {
    let mut found: BTreeMap<String, Instance> = BTreeMap::new();
    let mut revoked: HashSet<String> = HashSet::new();
    for row in read(
        source,
        "SELECT id, name, host, revoked_at_ms FROM clients \
         WHERE actor_id = ?1 AND kind = 'instance'",
        &[Value::from(agent)],
    ) {
        let Some(id) = opt_text(&row, "id") else {
            continue;
        };
        if row.get("revoked_at_ms").is_some_and(|v| !v.is_null()) {
            revoked.insert(id);
            continue;
        }
        found.insert(
            id.clone(),
            Instance {
                label: opt_text(&row, "name").unwrap_or_else(|| id.clone()),
                host: opt_text(&row, "host"),
                client_id: id,
                ..Instance::default()
            },
        );
    }
    for row in read(
        source,
        "SELECT id, description, definition_hash, started_at_ms FROM agent_profiles \
         WHERE actor_id = ?1",
        &[Value::from(agent)],
    ) {
        let Some(id) = opt_text(&row, "id") else {
            continue;
        };
        if revoked.contains(&id) {
            continue;
        }
        let instance = found.entry(id.clone()).or_insert_with(|| Instance {
            label: id.clone(),
            client_id: id,
            ..Instance::default()
        });
        instance.description = opt_text(&row, "description");
        instance.hash = opt_text(&row, "definition_hash");
        instance.started_ms = int(&row, "started_at_ms").unwrap_or(0);
    }
    found.into_values().collect()
}

/// The person's `instance_prefs` row for one agent in one database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub default_client: String,
    pub fallback: Vec<String>,
    pub updated_ms: i64,
}

impl Prefs {
    /// The instances in the order they are asked, each once.
    pub fn order(&self) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for client in std::iter::once(&self.default_client).chain(&self.fallback) {
            if !client.is_empty() && !order.contains(client) {
                order.push(client.clone());
            }
        }
        order
    }
}

/// `instance_prefs.id`: `<person>/<agent>` (V9).
pub fn prefs_id(person: &str, agent: &str) -> String {
    format!("{person}/{agent}")
}

pub fn prefs_of(source: &Source, agent: &str) -> Option<Prefs> {
    let rows = read(
        source,
        "SELECT default_client, fallback, updated_at_ms FROM instance_prefs WHERE id = ?1",
        &[Value::from(prefs_id(&source.actor, agent))],
    );
    let row = rows.first()?;
    Some(Prefs {
        default_client: text(row, "default_client").to_owned(),
        fallback: names(row, "fallback"),
        updated_ms: int(row, "updated_at_ms").unwrap_or(0),
    })
}

/// One shared database's part of an agent's view.
#[derive(Debug, Clone)]
pub struct Shared {
    pub database: String,
    pub instances: Vec<Instance>,
    pub prefs: Option<Prefs>,
    pub online: Online,
}

impl Shared {
    pub fn read(source: &Source, agent: &str, online: Online) -> Shared {
        Shared {
            database: source.database.clone(),
            instances: instances_of(source, agent),
            prefs: prefs_of(source, agent),
            online,
        }
    }
}

/// How `client` stands in the databases that list it.
fn presence_in<'a>(
    client: &str,
    databases: impl IntoIterator<Item = &'a Online>,
) -> AgentInstancePresenceVm {
    let mut known = false;
    for online in databases.into_iter().flatten() {
        if online.contains(client) {
            return AgentInstancePresenceVm::Online;
        }
        known = true;
    }
    if known {
        AgentInstancePresenceVm::Offline
    } else {
        AgentInstancePresenceVm::Unknown
    }
}

/// The sentence of each instance whose definition differs from its
/// siblings', by client id.
fn differing(instances: &[(String, String, Option<String>)]) -> HashMap<String, String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (_, _, hash) in instances {
        if let Some(hash) = hash {
            *counts.entry(hash.as_str()).or_default() += 1;
        }
    }
    let mut out = HashMap::new();
    if counts.len() < 2 {
        return out;
    }
    let most = counts.values().copied().max().unwrap_or(0);
    let leaders: Vec<&str> = counts
        .iter()
        .filter(|(_, n)| **n == most)
        .map(|(hash, _)| *hash)
        .collect();
    let majority = (leaders.len() == 1).then(|| leaders[0]);
    for (client, _, hash) in instances {
        let Some(hash) = hash else { continue };
        if majority == Some(hash.as_str()) {
            continue;
        }
        let others: Vec<&str> = instances
            .iter()
            .filter(|(_, _, other)| other.as_ref().is_some_and(|o| o != hash))
            .map(|(_, label, _)| label.as_str())
            .collect();
        out.insert(
            client.clone(),
            format!(
                "Runs a different definition from {}: one of them runs an older or a newer image.",
                and_list(&others)
            ),
        );
    }
    out
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn label_of(labels: &HashMap<String, String>, client: &str) -> String {
    labels
        .get(client)
        .cloned()
        .unwrap_or_else(|| client.to_owned())
}

/// What an order does, as the person reads it.
fn order_sentence(order: &[String], labels: &HashMap<String, String>) -> String {
    match order {
        [] => "No choice yet: a new session goes to whichever instance is online.".to_owned(),
        [default] => format!(
            "New sessions go to {}; while it is offline they wait for it.",
            label_of(labels, default)
        ),
        [default, fallback @ ..] => {
            let fallback: Vec<String> = fallback.iter().map(|c| label_of(labels, c)).collect();
            format!(
                "New sessions go to {}; while it is offline, to {}.",
                label_of(labels, default),
                fallback.join(", then ")
            )
        }
    }
}

/// One agent's view over the databases it shares with the person.
pub fn agent_view(
    agent_actor: &str,
    agent_name: &str,
    shared: &[Shared],
    notice: Option<String>,
) -> AgentInstancesVm {
    // Every instance once, with the databases that list it.
    let mut merged: BTreeMap<String, (Instance, Vec<usize>)> = BTreeMap::new();
    for (at, db) in shared.iter().enumerate() {
        for instance in &db.instances {
            match merged.get_mut(&instance.client_id) {
                Some((known, dbs)) => {
                    dbs.push(at);
                    if instance.started_ms > known.started_ms && instance.hash.is_some() {
                        known.hash = instance.hash.clone();
                        known.started_ms = instance.started_ms;
                    }
                    if known.description.is_none() {
                        known.description = instance.description.clone();
                    }
                    if known.host.is_none() {
                        known.host = instance.host.clone();
                    }
                    if known.label == known.client_id && instance.label != instance.client_id {
                        known.label = instance.label.clone();
                    }
                }
                None => {
                    merged.insert(instance.client_id.clone(), (instance.clone(), vec![at]));
                }
            }
        }
    }
    let labels: HashMap<String, String> = merged
        .iter()
        .map(|(id, (instance, _))| (id.clone(), instance.label.clone()))
        .collect();
    // The newest row is the choice shown; the first database wins a tie.
    let mut chosen: Option<&Prefs> = None;
    for prefs in shared.iter().filter_map(|db| db.prefs.as_ref()) {
        if chosen.is_none_or(|held| prefs.updated_ms > held.updated_ms) {
            chosen = Some(prefs);
        }
    }
    let order: Vec<String> = chosen.map(Prefs::order).unwrap_or_default();
    let orders: Vec<(&str, Option<Vec<String>>)> = shared
        .iter()
        .map(|db| (db.database.as_str(), db.prefs.as_ref().map(Prefs::order)))
        .collect();
    let disagreement = orders.windows(2).any(|w| w[0].1 != w[1].1).then(|| {
        let parts: Vec<String> = orders
            .iter()
            .map(|(database, order)| match order {
                Some(order) => {
                    let named: Vec<String> = order.iter().map(|c| label_of(&labels, c)).collect();
                    format!("{database} asks {}", named.join(", then "))
                }
                None => format!("{database} has no choice, so any online instance answers"),
            })
            .collect();
        format!(
            "Your databases differ: {}. Choosing here writes the same choice into each.",
            parts.join("; ")
        )
    });
    let hashes: Vec<(String, String, Option<String>)> = merged
        .values()
        .map(|(i, _)| (i.client_id.clone(), i.label.clone(), i.hash.clone()))
        .collect();
    let differs = differing(&hashes);
    let mut instances: Vec<AgentInstanceVm> = merged
        .into_values()
        .map(|(instance, dbs)| AgentInstanceVm {
            presence: presence_in(
                &instance.client_id,
                dbs.iter().map(|at| &shared[*at].online),
            ),
            databases: dbs.iter().map(|at| shared[*at].database.clone()).collect(),
            definition: instance
                .hash
                .as_ref()
                .map(|hash| hash.chars().take(12).collect()),
            definition_differs: differs.get(&instance.client_id).cloned(),
            rank: order
                .iter()
                .position(|c| *c == instance.client_id)
                .and_then(|at| u32::try_from(at).ok()),
            client_id: instance.client_id,
            label: instance.label,
            host: instance.host,
            description: instance.description,
        })
        .collect();
    instances.sort_by(|a, b| {
        (
            a.rank.is_none(),
            a.rank,
            a.label.to_lowercase(),
            &a.client_id,
        )
            .cmp(&(
                b.rank.is_none(),
                b.rank,
                b.label.to_lowercase(),
                &b.client_id,
            ))
    });
    AgentInstancesVm {
        agent_actor: agent_actor.to_owned(),
        agent_mark: crate::hq::sessions::mark(agent_name),
        agent_name: agent_name.to_owned(),
        databases: shared.iter().map(|db| db.database.clone()).collect(),
        sentence: order_sentence(&order, &labels),
        default_client: order.first().cloned(),
        fallback: order.iter().skip(1).cloned().collect(),
        instances,
        disagreement,
        notice,
    }
}

/// The agents of every held database's directory, by id: their names.
fn agents(sources: &[Source]) -> BTreeMap<String, String> {
    let mut agents = BTreeMap::new();
    for source in sources {
        for row in read(
            source,
            "SELECT id, display_name FROM actors WHERE kind = 'agent'",
            &[],
        ) {
            let Some(id) = opt_text(&row, "id") else {
                continue;
            };
            let name = opt_text(&row, "display_name").unwrap_or_else(|| id.clone());
            agents.entry(id).or_insert(name);
        }
    }
    agents
}

/// The held databases whose directory names `agent`.
fn shares(source: &Source, agent: &str) -> bool {
    !read(
        source,
        "SELECT id FROM actors WHERE id = ?1 AND kind = 'agent'",
        &[Value::from(agent)],
    )
    .is_empty()
}

/// Settings › Agents' instances: every agent with an instance in a held
/// database, by name. `online` says who is present in a database.
pub fn list(sources: &[Source], online: &dyn Fn(&Source) -> Online) -> Vec<AgentInstancesVm> {
    let presence: Vec<Online> = sources.iter().map(online).collect();
    let mut out: Vec<AgentInstancesVm> = agents(sources)
        .into_iter()
        .filter_map(|(agent, name)| {
            let shared: Vec<Shared> = sources
                .iter()
                .zip(&presence)
                .filter(|(source, _)| shares(source, &agent))
                .map(|(source, online)| Shared::read(source, &agent, online.clone()))
                .collect();
            if shared.iter().all(|db| db.instances.is_empty()) {
                return None;
            }
            let notice = settle(sources, &agent);
            Some(agent_view(&agent, &name, &shared, notice))
        })
        .collect();
    out.sort_by(|a, b| {
        (a.agent_name.to_lowercase(), &a.agent_actor)
            .cmp(&(b.agent_name.to_lowercase(), &b.agent_actor))
    });
    out
}

/// Choices written here whose outcome the server still owes: (database,
/// agent) to the commit.
static WRITING: LazyLock<Mutex<HashMap<(String, String), String>>> = LazyLock::new(Mutex::default);

/// The server's refusals of a choice, by agent, kept until the next choice.
static REFUSED: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

/// Look up the choices written here for `agent`: a refused one becomes the
/// notice, a settled one is forgotten.
fn settle(sources: &[Source], agent: &str) -> Option<String> {
    let mut writing = lock(&WRITING);
    let mut refused = lock(&REFUSED);
    writing.retain(|(database, of), commit| {
        if of != agent {
            return true;
        }
        let Some(source) = sources.iter().find(|s| s.database == *database) else {
            return false;
        };
        match source.replica.delivery(commit) {
            Ok(Delivery::Pending) => true,
            Ok(Delivery::Refused(reason)) => {
                refused.insert(
                    agent.to_owned(),
                    format!("The server refused your choice in {database}: {reason}"),
                );
                false
            }
            Ok(Delivery::Applied | Delivery::Unknown) | Err(_) => false,
        }
    });
    refused.get(agent).cloned()
}

/// Write `order` — the instance that answers first, then the fallback — as
/// the person's `instance_prefs` row for `agent` in every held database
/// they share with it, offline if need be.
pub fn set_order(
    sources: &[Source],
    agent: &str,
    order: &[String],
    now_ms: i64,
) -> Result<(), String> {
    let shared: Vec<&Source> = sources.iter().filter(|s| shares(s, agent)).collect();
    if shared.is_empty() {
        return Err(format!("{agent} is in no database this device holds."));
    }
    let Some(default) = order.first() else {
        return Err("Choose the instance that answers first.".to_owned());
    };
    let mut seen: HashSet<&str> = HashSet::new();
    if let Some(twice) = order.iter().find(|c| !seen.insert(c.as_str())) {
        return Err(format!(
            "{twice} is named twice; each instance takes one place."
        ));
    }
    let known: HashSet<String> = shared
        .iter()
        .flat_map(|source| instances_of(source, agent))
        .map(|instance| instance.client_id)
        .collect();
    if let Some(stranger) = order.iter().find(|c| !known.contains(*c)) {
        return Err(format!(
            "{stranger} is not an instance of {agent} in your databases."
        ));
    }
    let fallback = Value::from(order[1..].to_vec()).to_string();
    let mut failed: Vec<String> = Vec::new();
    lock(&REFUSED).remove(agent);
    for source in shared {
        let row = RowOp::Upsert {
            table: "instance_prefs".to_owned(),
            values: authored(
                source,
                object(serde_json::json!({
                    "id": prefs_id(&source.actor, agent),
                    "person_actor": source.actor,
                    "agent_actor": agent,
                    "default_client": default,
                    "fallback": fallback,
                    "updated_at_ms": now_ms,
                })),
            ),
            base_version: None,
        };
        match source.replica.mutate(vec![row]) {
            Ok(commit) => {
                lock(&WRITING).insert((source.database.clone(), agent.to_owned()), commit);
                source.replica.sync_now();
            }
            Err(error) => failed.push(format!("{}: {error}", source.database)),
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Your choice was not written in {}.",
            failed.join("; ")
        ))
    }
}

/// The instances in the order they are asked for a session nobody holds,
/// as the instances route it: the session's `preferred_client`, then its
/// person's default and fallback; `None` when the order is not on this
/// device (another person opened the session, and their row is theirs).
fn session_order(source: &Source, session_id: &str, agent: &str) -> Option<Vec<String>> {
    let rows = read(
        source,
        "SELECT created_by_actor, preferred_client FROM sessions WHERE id = ?1",
        &[Value::from(session_id)],
    );
    let row = rows.first()?;
    if text(row, "created_by_actor") != source.actor {
        return None;
    }
    let mut order: Vec<String> = opt_text(row, "preferred_client").into_iter().collect();
    for client in prefs_of(source, agent)
        .map(|p| p.order())
        .unwrap_or_default()
    {
        if !order.contains(&client) {
            order.push(client);
        }
    }
    Some(order)
}

/// The header of a session as its database's rows and presence say it.
pub fn answering_vm(
    agent_name: &str,
    instances: &[Instance],
    claim: Option<&Claim>,
    order: Option<&[String]>,
    online: &Online,
) -> Option<AgentSessionAnsweringVm> {
    let holder = claim
        .filter(|claim| !claim.released && !claim.holder_client.is_empty())
        .map(|claim| claim.holder_client.clone());
    if instances.is_empty() && holder.is_none() {
        return None;
    }
    let mut states: Vec<AgentInstanceStateVm> = instances
        .iter()
        .map(|instance| AgentInstanceStateVm {
            presence: presence_in(&instance.client_id, std::iter::once(online)),
            holds: holder.as_deref() == Some(instance.client_id.as_str()),
            client_id: instance.client_id.clone(),
            label: instance.label.clone(),
        })
        .collect();
    if let Some(holder) = &holder {
        if !states.iter().any(|s| s.client_id == *holder) {
            states.push(AgentInstanceStateVm {
                presence: presence_in(holder, std::iter::once(online)),
                holds: true,
                client_id: holder.clone(),
                label: holder.clone(),
            });
        }
    }
    states.sort_by(|a, b| {
        (!a.holds, a.label.to_lowercase(), &a.client_id).cmp(&(
            !b.holds,
            b.label.to_lowercase(),
            &b.client_id,
        ))
    });
    let sentence = match states.iter().find(|s| s.holds) {
        Some(held) => match held.presence {
            AgentInstancePresenceVm::Online => format!("{} is answering.", held.label),
            AgentInstancePresenceVm::Offline => format!(
                "{} holds this conversation and is offline; another instance takes it over once its lease runs out.",
                held.label
            ),
            AgentInstancePresenceVm::Unknown => format!("{} holds this conversation.", held.label),
        },
        None => match online {
            None => "No instance holds this conversation yet.".to_owned(),
            Some(present) => {
                let up: Vec<&AgentInstanceStateVm> = states
                    .iter()
                    .filter(|s| present.contains(&s.client_id))
                    .collect();
                let next = match order {
                    Some(order) if !order.is_empty() => order
                        .iter()
                        .find(|c| up.iter().any(|s| s.client_id == **c))
                        .and_then(|c| states.iter().find(|s| s.client_id == *c)),
                    _ => None,
                };
                match (next, up.is_empty(), order.is_some_and(|o| !o.is_empty())) {
                    (Some(next), _, _) => format!(
                        "No instance holds this conversation yet; {} takes it next.",
                        next.label
                    ),
                    (None, _, true) => "None of the instances you chose is online; this \
                         conversation waits for one."
                        .to_owned(),
                    (None, true, false) => format!(
                        "No instance of {agent_name} is online; this conversation waits for one."
                    ),
                    (None, false, false) => {
                        "No instance holds this conversation yet; one that is online takes it next."
                            .to_owned()
                    }
                }
            }
        },
    };
    Some(AgentSessionAnsweringVm {
        holder,
        sentence,
        instances: states,
    })
}

/// The header of `session_id` with `agent`, from its database alone.
pub fn answering(
    source: &Source,
    session_id: &str,
    agent: &str,
    agent_name: &str,
) -> Option<AgentSessionAnsweringVm> {
    let instances = instances_of(source, agent);
    let claim = live::claim(source, session_id);
    let order = session_order(source, session_id, agent);
    answering_vm(
        agent_name,
        &instances,
        claim.as_ref(),
        order.as_deref(),
        &online(source),
    )
}

impl Hq {
    /// Every agent's instances and the person's choice, from the replicas.
    pub fn agent_instances(&self) -> Vec<AgentInstancesVm> {
        list(&self.sources(), &online)
    }

    /// Write the person's order for `agent` — the first answers, the rest
    /// are its fallback — into each held database they share with it.
    pub fn set_agent_instances(&self, agent: &str, order: &[String]) -> Result<(), HqError> {
        set_order(&self.sources(), agent, order, crate::hq::sessions::now_ms())
            .map_err(HqError::Sentence)?;
        self.touch();
        Ok(())
    }
}

#[cfg(test)]
#[path = "instances_tests.rs"]
mod tests;

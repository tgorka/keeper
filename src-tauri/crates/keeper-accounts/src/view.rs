//! The view of the person's domains (story 114.5, UX-DR-E114-1): each domain
//! this device opens with its kind and name, the git repository it replaces
//! and its file-write record and graph root when discovery gives them, and
//! who is in it from that domain's own directory; the domains the person
//! left, with what went with them; and how current all of it is.
//!
//! Everything here comes from discovery and the device's own replicas, and
//! nothing from the map (DF/AD-6). The view is kept as it was last read, so
//! an offline device — and `keeper-cli accounts domains` — shows the last
//! known with the time discovery answered.

use keeper_accounts_api::discovery::DomainEntry;
use keeper_accounts_api::{DomainKind, FilesWrites};
use keeper_ports::Replica;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// How current the view is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DomainsState {
    /// No server, or this device is not connected to it: nothing to show.
    #[default]
    NotConnected,
    /// Asking the server now; what is shown is the last known.
    Reading,
    /// What the server said in this run.
    Current,
    /// The server could not be reached: the last known, as of `readAtMs`.
    Offline,
    /// The server removed this device: the last known. Every copy stays on
    /// this device, readable, and none syncs.
    Removed,
}

/// The person's domains as this device knows them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DomainsVm {
    pub state: DomainsState,
    /// When discovery last answered, in ms since the epoch; never, `null`.
    #[ts(type = "number | null")]
    pub read_at_ms: Option<i64>,
    /// The domains this device opens, in the server's order.
    pub domains: Vec<DomainVm>,
    /// The domains the person left, most recent first.
    pub left: Vec<LeftDomainVm>,
}

/// One domain this device opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DomainVm {
    pub name: String,
    pub display_name: String,
    #[ts(type = "\"general\" | \"personal\" | \"group\"")]
    pub kind: DomainKind,
    /// The git repository this domain replaces, as the operator set it. On
    /// its own it never means the drive is cut over: git stays the truth
    /// until `filesWrites` is `open` (116.2, 120.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub replaced_repository: Option<String>,
    /// The domain's file-write record, when it has a manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "\"shadow\" | \"open\"")]
    pub files_writes: Option<FilesWrites>,
    /// The id of the domain's root `Collection`, once it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub graph_root: Option<String>,
    /// The people in it, from its own directory; `null` until that directory
    /// is on this device.
    pub members: Option<Vec<DomainMemberVm>>,
}

/// One person in a domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DomainMemberVm {
    pub actor_id: String,
    pub name: String,
    /// The person using this device.
    pub me: bool,
}

/// A domain the person left, as the answer that omitted it was read (114.5).
///
/// A copy with nothing unsent is deleted from this device: files already
/// opened elsewhere and links not yet expired are what remains (DF/AD-13).
/// A copy holding writes only this device has is **kept**: stopped and
/// unlinked, never opened, its files untouched; if a later discovery lists
/// the domain again, that copy opens and its writes go out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LeftDomainVm {
    pub name: String,
    pub display_name: String,
    #[ts(type = "\"general\" | \"personal\" | \"group\"")]
    pub kind: DomainKind,
    /// Commits made here the server never took (the replica's outbox).
    pub unsent: u32,
    /// Graph changes asked for here the server has not resolved.
    #[serde(default)]
    pub graph_intents: u32,
    /// The copy, with those writes, is still on this device.
    #[serde(default)]
    pub kept: bool,
    #[ts(type = "number")]
    pub left_at_ms: i64,
}

/// What a left domain's copy holds that only this device has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unsent {
    /// Commits waiting in its outbox.
    pub outbox: u32,
    /// Graph intents not yet resolved in its `<actor>.graph-queue`.
    pub graph_intents: u32,
    /// Something it holds could not be counted: its graph queue is there but
    /// could not be read, or its replica ended without its stop.
    pub unreadable: bool,
}

impl Unsent {
    /// Whether the copy must be kept: anything unsent, or anything that
    /// could not be counted. A copy the server holds all of is deleted; a
    /// wrong leave then costs a pull, never a write.
    pub fn keeps_copy(self) -> bool {
        self.outbox > 0 || self.graph_intents > 0 || self.unreadable
    }
}

/// How many left domains whose copy is deleted the view keeps; a kept copy
/// is always shown.
const LEFT_KEPT: usize = 20;

impl DomainVm {
    fn of(entry: &DomainEntry) -> DomainVm {
        DomainVm {
            name: entry.name.clone(),
            display_name: entry.display_name.clone(),
            kind: entry.kind,
            replaced_repository: entry.replaced_repository.clone(),
            files_writes: entry.files_writes,
            graph_root: entry.graph_root.clone(),
            members: None,
        }
    }
}

impl DomainsVm {
    /// A successful discovery at `at_ms` listed `entries`: they are the
    /// domains now, each keeping the members already read for it, and one
    /// joined again is no longer shown as left.
    pub fn discovered(&mut self, entries: &[DomainEntry], at_ms: i64) {
        let mut domains: Vec<DomainVm> = Vec::with_capacity(entries.len());
        for entry in entries {
            if domains.iter().any(|known| known.name == entry.name) {
                continue;
            }
            let mut domain = DomainVm::of(entry);
            domain.members = self
                .domains
                .iter()
                .find(|known| known.name == entry.name)
                .and_then(|known| known.members.clone());
            domains.push(domain);
        }
        self.left
            .retain(|left| !entries.iter().any(|entry| entry.name == left.name));
        self.domains = domains;
        self.read_at_ms = Some(at_ms);
    }

    /// The person left `name` at `at_ms` and its copy held `unsent`: kept
    /// with it when [`Unsent::keeps_copy`], else deleted. A name discovery
    /// never described is shown as itself.
    pub fn leave(&mut self, name: &str, unsent: Unsent, at_ms: i64) {
        let known = self.domains.iter().position(|domain| domain.name == name);
        let (display_name, kind) = match known {
            Some(at) => {
                let domain = self.domains.remove(at);
                (domain.display_name, domain.kind)
            }
            None => self
                .left
                .iter()
                .find(|left| left.name == name)
                .map_or((name.to_owned(), DomainKind::Group), |left| {
                    (left.display_name.clone(), left.kind)
                }),
        };
        self.left.retain(|left| left.name != name);
        self.left.insert(
            0,
            LeftDomainVm {
                name: name.to_owned(),
                display_name,
                kind,
                unsent: unsent.outbox,
                graph_intents: unsent.graph_intents,
                kept: unsent.keeps_copy(),
                left_at_ms: at_ms,
            },
        );
        let mut deleted = 0;
        self.left.retain(|left| {
            if left.kept {
                return true;
            }
            deleted += 1;
            deleted <= LEFT_KEPT
        });
    }

    /// Whether this device keeps `name`'s copy of a domain the person left:
    /// it is not opened until a discovery lists the domain again.
    pub fn is_kept(&self, name: &str) -> bool {
        self.left.iter().any(|left| left.name == name && left.kept)
    }

    /// The members read for `name` now; `None` keeps the last known.
    pub fn set_members(&mut self, name: &str, members: Option<Vec<DomainMemberVm>>) {
        let Some(members) = members else { return };
        if let Some(domain) = self.domains.iter_mut().find(|domain| domain.name == name) {
            domain.members = Some(members);
        }
    }

    /// The kind discovery gave `name`, if it listed it.
    pub fn kind_of(&self, name: &str) -> Option<DomainKind> {
        self.domains
            .iter()
            .find(|domain| domain.name == name)
            .map(|domain| domain.kind)
    }
}

/// The people of `domain`'s own directory as `replica` holds it, `me` marked;
/// `None` while no person of it is on this device yet (the person is always
/// in their own domain's directory) or the replica cannot be read.
pub fn members(replica: &dyn Replica, domain: &str, me: &str) -> Option<Vec<DomainMemberVm>> {
    let rows = replica
        .query(
            "SELECT id, kind, display_name, dir FROM actors WHERE dir = ?1 AND kind = 'person' \
             ORDER BY display_name, id",
            &[Value::from(domain)],
        )
        .ok()?;
    let text = |row: &serde_json::Map<String, Value>, key: &str| {
        row.get(key).and_then(Value::as_str).map(str::to_owned)
    };
    let mut people: Vec<DomainMemberVm> = rows
        .iter()
        .filter(|row| text(row, "kind").as_deref() == Some("person"))
        .filter(|row| text(row, "dir").is_none_or(|dir| dir == domain))
        .filter_map(|row| {
            let actor_id = text(row, "id")?;
            Some(DomainMemberVm {
                name: text(row, "display_name").unwrap_or_else(|| actor_id.clone()),
                me: actor_id == me,
                actor_id,
            })
        })
        .collect();
    people.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.actor_id.cmp(&b.actor_id))
    });
    people.dedup_by(|a, b| a.actor_id == b.actor_id);
    (!people.is_empty()).then_some(people)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_accounts_api::discovery::Discovery;
    use keeper_ports::{Opener, ReplicaSpec};
    use keeper_testkit::FakeOpener;
    use serde_json::{json, Map};

    const CORPUS: &str = include_str!("../../../../syncular/accounts.vectors.json");

    fn entry(name: &str, kind: DomainKind) -> DomainEntry {
        DomainEntry {
            name: name.to_owned(),
            mount: format!("/{name}"),
            kind,
            display_name: name.to_uppercase(),
            replaced_repository: None,
            graph_root: None,
            files_writes: None,
        }
    }

    fn actor(id: &str, kind: &str, name: &str, dir: &str) -> Map<String, Value> {
        let Value::Object(row) = json!({"id": id, "kind": kind, "display_name": name, "dir": dir})
        else {
            unreachable!()
        };
        row
    }

    fn replica(opener: &FakeOpener, domain: &str) -> std::sync::Arc<dyn Replica> {
        opener
            .open(ReplicaSpec {
                database: domain.to_owned(),
                path: std::path::PathBuf::from(format!("/nowhere/{domain}.sqlite")),
                client_id: None,
                link: None,
                units: Vec::new(),
                watch_presence: None,
            })
            .expect("opens")
    }

    /// 114.5-AC1: the four domains a token opens, each with its kind and its
    /// people from its own directory; offline, the last known and its time.
    #[test]
    fn four_domains_list_with_their_kinds_and_people_and_stay_known_offline() {
        let opener = FakeOpener::default();
        let entries = [
            entry("tgdrive", DomainKind::Personal),
            entry("neuradrive", DomainKind::Group),
            entry("makidrive", DomainKind::General),
            entry("crew", DomainKind::Group),
        ];
        let mut vm = DomainsVm {
            state: DomainsState::Current,
            ..DomainsVm::default()
        };
        vm.discovered(&entries, 1_000);
        for domain in ["tgdrive", "neuradrive", "makidrive", "crew"] {
            let held = replica(&opener, domain);
            let mut rows = vec![
                actor("ana", "person", "Ana", domain),
                actor("nixie", "agent", "Nixie", domain),
            ];
            if domain != "tgdrive" {
                rows.push(actor("bo", "person", "Bo", domain));
            }
            opener.all().last().expect("held").replicate(rows);
            vm.set_members(domain, members(held.as_ref(), domain, "ana"));
        }
        let names: Vec<(&str, DomainKind)> = vm
            .domains
            .iter()
            .map(|domain| (domain.name.as_str(), domain.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("tgdrive", DomainKind::Personal),
                ("neuradrive", DomainKind::Group),
                ("makidrive", DomainKind::General),
                ("crew", DomainKind::Group),
            ]
        );
        let people = |vm: &DomainsVm, name: &str| -> Vec<(String, bool)> {
            vm.domains
                .iter()
                .find(|domain| domain.name == name)
                .and_then(|domain| domain.members.clone())
                .expect("members read")
                .into_iter()
                .map(|member| (member.actor_id, member.me))
                .collect()
        };
        // An agent is not a member; a one-person domain holds the person alone.
        assert_eq!(people(&vm, "tgdrive"), [("ana".to_owned(), true)]);
        assert_eq!(
            people(&vm, "crew"),
            [("ana".to_owned(), true), ("bo".to_owned(), false)]
        );

        // Offline: a failed read keeps the last known members and read time.
        vm.state = DomainsState::Offline;
        vm.set_members("crew", None);
        assert_eq!(people(&vm, "crew").len(), 2);
        assert_eq!(vm.read_at_ms, Some(1_000));
        let kept: DomainsVm =
            serde_json::from_str(&serde_json::to_string(&vm).expect("json")).expect("reads back");
        assert_eq!(kept, vm, "the kept view is the view");
    }

    #[test]
    fn a_directory_not_on_this_device_yet_reads_as_unknown_not_as_empty() {
        let opener = FakeOpener::default();
        let held = replica(&opener, "crew");
        assert_eq!(members(held.as_ref(), "crew", "ana"), None);
        opener
            .all()
            .last()
            .expect("held")
            .replicate(vec![actor("bo", "person", "Bo", "other")]);
        assert_eq!(
            members(held.as_ref(), "crew", "ana"),
            None,
            "another domain's directory row is not this domain's member"
        );
    }

    /// 114.5-AC3's view: a left domain moves to `left` with its unsent
    /// writes, kept when there are any; joining it again takes it out of
    /// `left`. Kept copies are never dropped from the view.
    #[test]
    fn a_left_domain_is_shown_as_left_until_it_is_joined_again() {
        let mut vm = DomainsVm::default();
        let both = [
            entry("makidrive", DomainKind::General),
            entry("crew", DomainKind::Group),
        ];
        vm.discovered(&both, 1);
        let held = Unsent {
            outbox: 3,
            graph_intents: 1,
            unreadable: false,
        };
        vm.leave("crew", held, 2);
        assert_eq!(
            vm.domains
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["makidrive"]
        );
        assert_eq!(
            vm.left,
            [LeftDomainVm {
                name: "crew".to_owned(),
                display_name: "CREW".to_owned(),
                kind: DomainKind::Group,
                unsent: 3,
                graph_intents: 1,
                kept: true,
                left_at_ms: 2,
            }]
        );
        assert!(vm.is_kept("crew"));
        assert_eq!(vm.kind_of("crew"), None);
        vm.discovered(&both, 3);
        assert!(vm.left.is_empty());
        assert!(!vm.is_kept("crew"));
        assert_eq!(vm.kind_of("crew"), Some(DomainKind::Group));
        // A name discovery never described is shown as itself; nothing
        // unsent is not kept.
        vm.leave("scratch", Unsent::default(), 4);
        assert_eq!(vm.left[0].display_name, "scratch");
        assert!(!vm.left[0].kept);
        // A queue that could not be read keeps the copy uncounted.
        let unread = Unsent {
            unreadable: true,
            ..Unsent::default()
        };
        assert!(unread.keeps_copy());
        // Many deleted ones later, the kept copy is still shown.
        vm.leave("crew", held, 5);
        for at in 0..(LEFT_KEPT as i64 + 5) {
            vm.leave(&format!("gone{at}"), Unsent::default(), 10 + at);
        }
        assert!(vm.is_kept("crew"));
        assert_eq!(vm.left.len(), LEFT_KEPT + 1);
    }

    /// 114.5-AC9 over the contract's vectors: `replaced_repository`,
    /// `files_writes` and `graph_root` are carried when discovery gives them
    /// and absent otherwise; an entry discovery skips is nowhere.
    #[test]
    fn the_optional_entry_fields_are_carried_exactly_as_discovery_gives_them() {
        let corpus: Value = serde_json::from_str(CORPUS).expect("corpus");
        let vectors = corpus["discovery"].as_array().expect("discovery vectors");
        assert!(!vectors.is_empty());
        for vector in vectors {
            let found: Discovery = serde_json::from_value(vector["json"].clone()).expect("parses");
            let mut vm = DomainsVm::default();
            vm.discovered(&found.databases, 7);
            let shown = serde_json::to_value(&vm).expect("json");
            let want = vector["want"]["databases"].as_array().expect("want");
            let domains = shown["domains"].as_array().expect("domains");
            assert_eq!(domains.len(), want.len(), "{}", vector["name"]);
            for (domain, wanted) in domains.iter().zip(want) {
                assert_eq!(domain["name"], wanted["name"]);
                assert_eq!(domain["kind"], wanted["kind"]);
                assert_eq!(domain["displayName"], wanted["display_name"]);
                for (field, wire) in [
                    ("replacedRepository", "replaced_repository"),
                    ("filesWrites", "files_writes"),
                    ("graphRoot", "graph_root"),
                ] {
                    assert_eq!(
                        domain.get(field),
                        wanted.get(wire),
                        "{} {field}",
                        vector["name"]
                    );
                }
            }
        }
    }
}

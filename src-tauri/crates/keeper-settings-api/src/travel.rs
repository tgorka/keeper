//! Travel classes, layer settability, and lookup over the settings key
//! registry.

use serde::{Deserialize, Serialize};

/// Where a setting key's value is allowed to replicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TravelClass {
    /// Replicates to every device owned by the person under `*`.
    EveryDevice,
    /// Scoped to a single device record and never written under `*`.
    ThisDevice,
    /// Local-only; never leaves the machine.
    Never,
}

/// Which configuration layers are allowed to pin a setting key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Settable {
    /// May be pinned in the account layer (`*`) or a device layer.
    AnyLayer,
    /// May only be pinned in a device's machine file.
    MachineFile,
    /// May not be pinned in any layer.
    Never,
}

/// One exact key or dotted-prefix family in `settings.travel.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TravelEntry {
    /// Bare registry key, or dotted prefix ending in `.` when `family` is true.
    pub key: String,
    /// Whether `key` is a prefix family rather than an exact key.
    pub family: bool,
    /// Replication class for preferences matching this entry.
    pub class: TravelClass,
    /// Layer settability for pins matching this entry.
    pub settable: Settable,
}

/// Parsed `settings.travel.json` registry table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Travel {
    /// All exact keys and prefix families in the contract.
    pub keys: Vec<TravelEntry>,
}

impl Travel {
    /// Parses the compiled-in [`crate::TRAVEL_JSON`] table.
    ///
    /// Panics only if the compiled-in `syncular/settings.travel.json` asset is
    /// malformed.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn contract() -> Self {
        serde_json::from_str(crate::TRAVEL_JSON)
            .expect("compiled-in syncular/settings.travel.json must parse as Travel")
    }

    /// Looks up a bare registry key: exact non-family match first, otherwise
    /// the longest family prefix strictly shorter than `key`.
    #[must_use]
    pub fn entry(&self, key: &str) -> Option<&TravelEntry> {
        if let Some(exact) = self.keys.iter().find(|e| !e.family && e.key == key) {
            return Some(exact);
        }
        self.keys
            .iter()
            .filter(|e| e.family && key.len() > e.key.len() && key.starts_with(e.key.as_str()))
            .max_by_key(|e| e.key.len())
    }
}

/// Key-family prefixes whose values must be one of [`CREDENTIAL_CHOICES`].
pub const CREDENTIAL_FAMILIES: [&str; 2] = [
    "sync.credential_source.",
    "bots.provider_credential_source.",
];

/// Allowed values for keys under [`CREDENTIAL_FAMILIES`].
pub const CREDENTIAL_CHOICES: [&str; 3] = ["account", "own", "none"];

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_travel() -> Travel {
        Travel {
            keys: vec![
                TravelEntry {
                    key: "bots.".to_owned(),
                    family: true,
                    class: TravelClass::EveryDevice,
                    settable: Settable::AnyLayer,
                },
                TravelEntry {
                    key: "bots.provider.".to_owned(),
                    family: true,
                    class: TravelClass::ThisDevice,
                    settable: Settable::MachineFile,
                },
                TravelEntry {
                    key: "bots.provider.default".to_owned(),
                    family: false,
                    class: TravelClass::Never,
                    settable: Settable::Never,
                },
            ],
        }
    }

    #[test]
    fn longest_family_prefix_wins() {
        let travel = sample_travel();
        let entry = travel.entry("bots.provider.openai").expect("family match");
        assert_eq!(entry.key, "bots.provider.");
        assert_eq!(entry.class, TravelClass::ThisDevice);

        let outer = travel.entry("bots.enabled").expect("outer family match");
        assert_eq!(outer.key, "bots.");
        assert_eq!(outer.class, TravelClass::EveryDevice);

        assert!(travel.entry("bots.").is_none());
    }

    #[test]
    fn exact_match_beats_family() {
        let travel = sample_travel();
        let entry = travel.entry("bots.provider.default").expect("exact match");
        assert!(!entry.family);
        assert_eq!(entry.class, TravelClass::Never);
    }
}

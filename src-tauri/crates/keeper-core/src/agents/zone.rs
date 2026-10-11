//! The agents zone as a listing: which entries are homes and why a zone hosts nothing (AD-361, story 89.2).
//!
//! The host lists the zone's top level and reads `_drive.toml` through
//! `browse::resolve`; this module decides. A zone hosts an agent only when its
//! declaration parses, because the declaration names the audience every agent
//! homed there speaks to — an undeclared audience is never guessed.

use super::drive::{self, DriveDecl};

/// Why a zone without a declaration hosts nothing.
pub const NO_DRIVE: &str =
    "This agents zone has no _drive.toml, so it hosts no agent. Write one naming the drive's readers.";

/// One entry of the zone's top level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneEntry {
    pub name: String,
    pub is_dir: bool,
}

/// What a zone holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneAssessment {
    /// The declaration, or the sentence saying why there is none.
    pub drive: Result<DriveDecl, String>,
    /// The agents' folder names, sorted; empty when the zone hosts nothing.
    pub homes: Vec<String>,
    /// Set exactly when the zone hosts nothing because of its declaration.
    pub hosts_nothing_because: Option<String>,
}

/// Whether a top-level name belongs to the zone itself and is never a home:
/// `_drive.toml`, `_skills/`, `_workflows/`, `_template/` and anything else
/// beginning `_`, and the zone's guide and rules.
pub fn is_zone_own(name: &str) -> bool {
    name.starts_with('_') || name == "README.md" || name == "AGENTS.md"
}

/// Read a zone's top level and its `_drive.toml` text, if it has one.
pub fn assess(listing: &[ZoneEntry], drive_toml: Option<&str>) -> ZoneAssessment {
    let drive = match drive_toml {
        None => Err(NO_DRIVE.to_owned()),
        Some(text) => drive::parse(text).map_err(|refusal| refusal.sentence()),
    };
    let hosts_nothing_because = match (&drive, drive_toml) {
        (Ok(_), _) => None,
        (Err(_), None) => Some(NO_DRIVE.to_owned()),
        (Err(sentence), Some(_)) => Some(format!(
            "This agents zone's _drive.toml is refused, so it hosts no agent. {sentence}"
        )),
    };
    let homes = if hosts_nothing_because.is_some() {
        Vec::new()
    } else {
        let mut homes: Vec<String> = listing
            .iter()
            .filter(|entry| {
                entry.is_dir && !entry.name.starts_with('.') && !is_zone_own(&entry.name)
            })
            .map(|entry| entry.name.clone())
            .collect();
        homes.sort_unstable();
        homes
    };
    ZoneAssessment {
        drive,
        homes,
        hosts_nothing_because,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agents")
            .join(name)
    }

    /// The zone's top level, as the host lists it.
    fn listing(zone: &Path) -> Vec<ZoneEntry> {
        std::fs::read_dir(zone)
            .expect("fixture zone")
            .map(|entry| {
                let entry = entry.expect("fixture entry");
                ZoneEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    is_dir: entry.file_type().expect("file type").is_dir(),
                }
            })
            .collect()
    }

    fn drive_text(zone: &Path) -> Option<String> {
        std::fs::read_to_string(zone.join(drive::FILE_NAME)).ok()
    }

    #[test]
    fn a_declared_zone_hosts_its_agents_and_never_its_own_folders() {
        let zone = fixture("zone-ok");
        let entries = listing(&zone);
        for own in [
            "_skills",
            "_template",
            "_workflows",
            "README.md",
            "AGENTS.md",
            "_drive.toml",
        ] {
            assert!(
                entries.iter().any(|entry| entry.name == own),
                "the fixture holds {own}, so skipping it is proved"
            );
        }
        let assessed = assess(&entries, drive_text(&zone).as_deref());
        assert_eq!(assessed.homes, ["nixi", "tola-grey"]);
        assert_eq!(assessed.hosts_nothing_because, None);
        let drive = assessed.drive.expect("declared");
        assert_eq!(drive.id, "tgdrive");
        assert_eq!(drive.principal, "tgorka");
    }

    #[test]
    fn a_zone_without_a_declaration_hosts_nothing() {
        let zone = fixture("zone-no-drive");
        let entries = listing(&zone);
        assert!(entries
            .iter()
            .any(|entry| entry.is_dir && entry.name == "tola-grey"));
        let assessed = assess(&entries, drive_text(&zone).as_deref());
        assert_eq!(
            assessed.hosts_nothing_because.as_deref(),
            Some(
                "This agents zone has no _drive.toml, so it hosts no agent. Write one naming the drive's readers."
            )
        );
        assert!(assessed.homes.is_empty());
        assert_eq!(assessed.drive, Err(NO_DRIVE.to_owned()));
    }

    #[test]
    fn a_zone_whose_declaration_is_refused_hosts_nothing_and_says_why() {
        let zone = fixture("zone-ok");
        let refused = "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreader = [\"@tgorka:example.org\"]\n";
        let assessed = assess(&listing(&zone), Some(refused));
        assert!(assessed.homes.is_empty());
        assert_eq!(
            assessed.hosts_nothing_because.as_deref(),
            Some(
                "This agents zone's _drive.toml is refused, so it hosts no agent. _drive.toml has `reader`, which is not one of its keys."
            )
        );
        assert_eq!(
            assessed.drive,
            Err("_drive.toml has `reader`, which is not one of its keys.".to_owned())
        );
    }

    #[test]
    fn files_and_hidden_folders_are_never_homes() {
        let entries = [
            ZoneEntry {
                name: "notes.md".to_owned(),
                is_dir: false,
            },
            ZoneEntry {
                name: ".keeper".to_owned(),
                is_dir: true,
            },
            ZoneEntry {
                name: "amelia".to_owned(),
                is_dir: true,
            },
        ];
        let text =
            std::fs::read_to_string(fixture("zone-ok").join(drive::FILE_NAME)).expect("fixture");
        assert_eq!(assess(&entries, Some(&text)).homes, ["amelia"]);
    }
}

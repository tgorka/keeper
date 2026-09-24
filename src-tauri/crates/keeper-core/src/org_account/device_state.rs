//! This device as its own file in the person's config repository, so a
//! reinstall restores it (AD-328, AD-329).
//!
//! ```text
//! <login>/device.<device>.toml   [[drive]], [[provider]] with [[provider.bot]]
//!                                and [[provider.grant]], [[matrix]]
//! ```
//!
//! Each drive is the shell's rendering of its whole sync profile, minus its
//! local id and volume binding, plus its schedules — an opaque table to this
//! crate apart from its identity. The file has one writer, the device it
//! names: every sync rewrites it from this device's state, and a restore
//! ([`restore_plan`]) reads back what this device does not have. No secret is
//! ever written here, only the choice of credential.

use serde::{Deserialize, Serialize};

use super::manifest::{self, BotRecord, Extra};
use super::settings_sync::{remote_has_secret, DriveRef, ProviderRef};

/// `<login>/device.<device>.toml`.
pub fn path(login: &str, device: &str) -> String {
    format!("{login}/device.{device}.toml")
}

const HEADER: &str = "# This device's drives, bots and accounts, so keeper can restore it. keeper rewrites it from this device; edit settings.<device>.toml instead.\n";

fn default_credential_own() -> String {
    "own".to_owned()
}

fn default_matrix_kind() -> String {
    "password".to_owned()
}

/// A drive as a restore matches it: remote, branch and name (AD-327).
pub type DriveKey = DriveRef;

/// A bot provider as a restore matches it: kind and base URL.
pub type ProviderKey = ProviderRef;

/// `device.<device>.toml`. An `Err` from [`parse`](DeviceStateFile::parse)
/// is a file keeper cannot read; nothing is restored from it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceStateFile {
    /// The fingerprint of the machine that writes this file (the same one
    /// `devices/<device>.toml` records), rewritten every sync, so a device
    /// registered before fingerprints existed can still be told from another
    /// machine of its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    #[serde(rename = "drive", default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<toml::Table>,
    #[serde(rename = "provider", default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matrix: Vec<MatrixState>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One bot provider with its bots and their folder grants.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderState {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_timeout_ms: Option<u64>,
    /// `account` or `own`.
    #[serde(default = "default_credential_own")]
    pub credential: String,
    #[serde(rename = "bot", default, skip_serializing_if = "Vec::is_empty")]
    pub bots: Vec<BotRecord>,
    #[serde(rename = "grant", default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<GrantState>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One folder a provider's bots (or one bot) may reach.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GrantState {
    /// The bot's target; `None` is every bot of the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot: Option<String>,
    /// The drive, as its reference ([`Catalog::drive_reference`](super::settings_sync::Catalog::drive_reference)),
    /// or `*` for every drive.
    pub drive: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtree: Option<String>,
    pub mode: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One Matrix account and this device's preferences for it. Never a session:
/// a restore offers a sign-in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MatrixState {
    pub user_id: String,
    #[serde(default)]
    pub homeserver_url: String,
    /// `password`, `oidc` or `beeper`.
    #[serde(default = "default_matrix_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hue_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub muted_networks: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Drive-table fields that never go into the file: a git identity is a
/// person's name and address, and everyone with clone access reads this file.
const PRIVATE_DRIVE_FIELDS: [&str; 2] = ["author_override", "authorOverride"];

/// A drive table's identity: its `remote_url`, `branch` and `name`, which
/// every drive table must carry as strings.
pub fn drive_key(table: &toml::Table) -> Option<DriveKey> {
    let field = |name: &str| table.get(name).and_then(toml::Value::as_str);
    Some(DriveRef::named(
        field("remote_url")?,
        field("branch")?,
        field("name")?,
    ))
}

impl ProviderState {
    pub fn key(&self) -> ProviderKey {
        ProviderRef::new(&self.kind, &self.base_url)
    }
}

impl DeviceStateFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_owned())?;
        let file: DeviceStateFile = toml::from_str(text)
            .map_err(|error| format!("it cannot be read: {}", error.message()))?;
        file.check()?;
        Ok(file)
    }

    /// The file's bytes: the header, then every list in identity order, so an
    /// unchanged device renders identically. A drive's remote loses any
    /// credential it carries; a local-path remote is this device's fact and
    /// stays. Refused — nothing is written — when a remote would still carry
    /// a credential or a drive carries its git identity.
    pub fn render(&self) -> Result<String, String> {
        self.check()?;
        let normalized = self.normalized();
        for drive in &normalized.drives {
            let remote = drive
                .get("remote_url")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            if remote_has_secret(remote) {
                return Err(
                    "a drive's remote carries a credential, so the device file is not written"
                        .to_owned(),
                );
            }
            if PRIVATE_DRIVE_FIELDS
                .iter()
                .any(|field| drive.contains_key(*field))
            {
                return Err(
                    "a drive carries its git identity, so the device file is not written"
                        .to_owned(),
                );
            }
        }
        let body = toml::to_string(&normalized).map_err(|error| error.to_string())?;
        Ok(format!("{HEADER}{body}"))
    }

    /// Whether two files say the same, whatever their order, formatting,
    /// comments or remote spellings — so an unchanged device writes nothing.
    pub fn values_eq(&self, other: &DeviceStateFile) -> bool {
        self.normalized() == other.normalized()
    }

    fn check(&self) -> Result<(), String> {
        if self.drives.iter().all(|drive| drive_key(drive).is_some()) {
            Ok(())
        } else {
            Err("a drive has no name, remote_url or branch".to_owned())
        }
    }

    fn normalized(&self) -> DeviceStateFile {
        let mut drives: Vec<toml::Table> = self
            .drives
            .iter()
            .map(|drive| {
                let mut drive = drive.clone();
                if let Some(toml::Value::String(remote)) = drive.get_mut("remote_url") {
                    if let Some(portable) = manifest::portable_remote(remote) {
                        *remote = portable;
                    }
                }
                drive
            })
            .collect();
        drives.sort_by_cached_key(drive_key);
        let mut providers = self.providers.clone();
        for provider in &mut providers {
            provider
                .bots
                .sort_by(|a, b| (a.pin_order, &a.target).cmp(&(b.pin_order, &b.target)));
            provider.grants.sort_by(|a, b| {
                (&a.bot, &a.drive, &a.subtree, &a.mode)
                    .cmp(&(&b.bot, &b.drive, &b.subtree, &b.mode))
            });
        }
        providers.sort_by_cached_key(ProviderState::key);
        let mut matrix = self.matrix.clone();
        for account in &mut matrix {
            account.muted_networks.sort();
            account.muted_networks.dedup();
        }
        matrix.sort_by(|a, b| a.user_id.trim().cmp(b.user_id.trim()));
        DeviceStateFile {
            machine: self.machine.clone(),
            drives,
            providers,
            matrix,
            extra: self.extra.clone(),
        }
    }
}

/// What a restore creates: the file's entries whose identity this device
/// does not have.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RestorePlan {
    pub drives: Vec<toml::Table>,
    pub providers: Vec<ProviderState>,
    pub matrix: Vec<MatrixState>,
}

/// The entries of `file` absent here: drives by remote, branch and name,
/// providers by kind and base URL, Matrix accounts by user id. Pure; the shell
/// creates them.
pub fn restore_plan(
    file: &DeviceStateFile,
    local_drives: &[DriveKey],
    local_providers: &[ProviderKey],
    local_matrix: &[String],
) -> RestorePlan {
    RestorePlan {
        drives: file
            .drives
            .iter()
            .filter(|drive| drive_key(drive).is_some_and(|key| !local_drives.contains(&key)))
            .cloned()
            .collect(),
        providers: file
            .providers
            .iter()
            .filter(|provider| !local_providers.contains(&provider.key()))
            .cloned()
            .collect(),
        matrix: file
            .matrix
            .iter()
            .filter(|account| {
                !local_matrix
                    .iter()
                    .any(|user_id| user_id.trim() == account.user_id.trim())
            })
            .cloned()
            .collect(),
    }
}

/// What a restore could not create yet, kept in keeper.db
/// (`account.<id>.restore_pending`) and retried on every later sync, and
/// rendered back into the device file meanwhile so nothing is lost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RestorePending {
    /// Drive tables whose folder cannot be made or used yet.
    #[serde(default)]
    pub drives: Vec<toml::Table>,
    /// Grants whose drive is not on this device yet.
    #[serde(default)]
    pub grants: Vec<PendingGrant>,
    /// Providers that could not be added yet.
    #[serde(default)]
    pub providers: Vec<ProviderState>,
    /// Matrix accounts not signed in here yet.
    #[serde(default)]
    pub matrix: Vec<MatrixState>,
}

impl RestorePending {
    pub fn is_empty(&self) -> bool {
        self.drives.is_empty()
            && self.grants.is_empty()
            && self.providers.is_empty()
            && self.matrix.is_empty()
    }
}

/// A grant waiting for its drive, with the provider it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingGrant {
    /// The provider's reference ([`ProviderRef::reference`]).
    pub provider: String,
    pub grant: GrantState,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "schema = 2\n\
        machine = \"5eed\"\n\
        [[drive]]\n\
        name = \"tgdrive-light\"\n\
        remote_url = \"https://tg:SECRET@git.acme.dev/tg/tgdrive.git\"\n\
        branch = \"main\"\n\
        local_path = \"/Users/tg/tgdrive-light\"\n\
        lfs_mode = \"pointer_only\"\n\
        [[drive.schedules]]\n\
        kind = \"release\"\n\
        schedule = \"daily\"\n\
        mode = \"auto\"\n\
        enabled = true\n\
        [[drive]]\n\
        name = \"tgdrive\"\n\
        remote_url = \"https://git.acme.dev/tg/tgdrive\"\n\
        branch = \"main\"\n\
        local_path = \"/Users/tg/tgdrive\"\n\
        [[provider]]\n\
        kind = \"ollama\"\n\
        name = \"Home\"\n\
        base_url = \"http://localhost:11434\"\n\
        read_timeout_ms = 90000\n\
        credential = \"account\"\n\
        rate = 3\n\
        [[provider.bot]]\n\
        target = \"llama3\"\n\
        name = \"Llama\"\n\
        pin_order = 1\n\
        [[provider.grant]]\n\
        drive = \"drive:https://git.acme.dev/tg/tgdrive#main^tgdrive\"\n\
        subtree = \"notes\"\n\
        mode = \"read\"\n\
        until = 5\n\
        [[matrix]]\n\
        user_id = \"@tg:acme.dev\"\n\
        homeserver_url = \"https://matrix.acme.dev\"\n\
        kind = \"oidc\"\n\
        hue_index = 3\n\
        muted_networks = [\"whatsapp\", \"signal\"]\n\
        pins = [\"!a:acme.dev\"]\n";

    #[test]
    fn the_file_round_trips_sorted_without_credentials_and_keeps_unknown_fields() {
        let parsed = DeviceStateFile::parse(FILE.as_bytes()).expect("parses");
        assert_eq!(parsed.drives.len(), 2);
        assert_eq!(parsed.extra.get("schema"), Some(&toml::Value::Integer(2)));
        assert_eq!(
            parsed.providers[0].extra.get("rate"),
            Some(&toml::Value::Integer(3))
        );
        assert_eq!(
            parsed.providers[0].grants[0].extra.get("until"),
            Some(&toml::Value::Integer(5))
        );
        assert!(parsed.matrix[0].extra.contains_key("pins"));

        let rendered = parsed.render().expect("renders");
        assert!(rendered.starts_with(HEADER), "{rendered}");
        assert!(!rendered.contains("SECRET"), "{rendered}");
        assert!(rendered.contains("[[drive.schedules]]"), "{rendered}");
        let again = DeviceStateFile::parse(rendered.as_bytes()).expect("reparses");
        let names: Vec<&str> = again
            .drives
            .iter()
            .filter_map(|d| d.get("name").and_then(toml::Value::as_str))
            .collect();
        assert_eq!(names, ["tgdrive", "tgdrive-light"], "sorted by identity");
        assert_eq!(again.render().expect("renders"), rendered, "stable");
        assert!(
            again.values_eq(&parsed),
            "only order and the credential differ"
        );
        assert_eq!(again.matrix[0].muted_networks, ["signal", "whatsapp"]);
        assert_eq!(again.providers[0].read_timeout_ms, Some(90_000));

        let mut changed = again.clone();
        changed.matrix[0].hue_index = Some(4);
        assert!(!changed.values_eq(&again));

        let local = "[[drive]]\nname = \"n\"\nremote_url = \"/srv/git/n.git\"\nbranch = \"main\"\n";
        let kept = DeviceStateFile::parse(local.as_bytes()).expect("parses");
        assert!(
            kept.render()
                .expect("renders")
                .contains("remote_url = \"/srv/git/n.git\""),
            "a local remote is this device's fact and stays"
        );

        assert!(DeviceStateFile::parse(b"[[drive]]\nname = \"x\"\nbranch = \"main\"\n").is_err());

        assert_eq!(
            again.machine.as_deref(),
            Some("5eed"),
            "the writer's fingerprint travels"
        );
        assert!(rendered.contains("machine = \"5eed\""), "{rendered}");
        let mut other = again.clone();
        other.machine = Some("f00d".to_owned());
        assert!(!other.values_eq(&again));

        // Nothing is written that would publish a credential or a git identity.
        let secret = "[[drive]]\nname = \"n\"\nremote_url = \"file://tg:SECRET@nas/n.git\"\nbranch = \"main\"\n";
        let secret = DeviceStateFile::parse(secret.as_bytes()).expect("parses");
        assert!(secret.render().is_err());
        for field in ["author_override", "authorOverride"] {
            let mut identity = again.clone();
            identity.drives[0].insert(
                field.to_owned(),
                toml::Value::String("Tg <tg@acme.dev>".to_owned()),
            );
            assert!(identity.render().is_err(), "{field}");
        }
    }

    #[test]
    fn anything_still_waiting_keeps_the_pending_record() {
        assert!(RestorePending::default().is_empty());
        let file = DeviceStateFile::parse(FILE.as_bytes()).expect("parses");
        let waiting = [
            RestorePending {
                providers: file.providers.clone(),
                ..RestorePending::default()
            },
            RestorePending {
                matrix: file.matrix.clone(),
                ..RestorePending::default()
            },
        ];
        for pending in waiting {
            assert!(!pending.is_empty());
            let json = serde_json::to_string(&pending).expect("json");
            assert_eq!(
                serde_json::from_str::<RestorePending>(&json).expect("back"),
                pending
            );
        }
    }

    #[test]
    fn a_restore_plans_only_what_this_device_lacks() {
        let file = DeviceStateFile::parse(FILE.as_bytes()).expect("parses");
        let everything = restore_plan(&file, &[], &[], &[]);
        assert_eq!(everything.drives.len(), 2);
        assert_eq!(everything.providers.len(), 1);
        assert_eq!(everything.matrix.len(), 1);

        let plan = restore_plan(
            &file,
            &[DriveRef::named(
                "https://git.acme.dev/tg/tgdrive.git",
                "main",
                "tgdrive",
            )],
            &[ProviderRef::new("ollama", "HTTP://LocalHost:11434/")],
            &["@tg:acme.dev".to_owned()],
        );
        let names: Vec<&str> = plan
            .drives
            .iter()
            .filter_map(|d| d.get("name").and_then(toml::Value::as_str))
            .collect();
        assert_eq!(
            names,
            ["tgdrive-light"],
            "the other drive of the repository still restores"
        );
        assert!(plan.providers.is_empty());
        assert!(plan.matrix.is_empty());
    }
}

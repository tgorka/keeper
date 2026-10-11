//! `agentd.toml`: one Linux host's configuration (AD-375, ruling R8).
//!
//! The file is a contract, as `_drive.toml` is: an unknown key is refused with
//! its name, and every refusal is one sentence an operator can act on. It
//! holds no secret — every credential is `secret:<name>`, resolved by the
//! host's secret store — and keeper writes nothing in it but
//! `[homeserver].control_room` (90.5's `init`).
//!
//! `[[drives]]` pins each drive's `owner`, `readers` and `local_only` (S-15):
//! the operator copies the audience from the forge's collaborator list, which
//! is the real access list, and the mount rule runs on it before any checkout.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde::Deserialize;

use crate::agents::drive;
use crate::agents::label::Readers;
use crate::bots::url::{parse_base_url, BaseUrl};
use crate::bots::ProviderKind;

/// The grammar this build reads.
pub const GRAMMAR_VERSION: i64 = 1;

/// The file's name under `$XDG_CONFIG_HOME/keeper-agentd/`.
pub const FILE_NAME: &str = "agentd.toml";

/// What every credential that is not `secret:<name>` is told.
pub const SECRET_SENTENCE: &str = "a secret never goes in agentd.toml; write `secret:<name>` and put the secret in the environment, a systemd credential or a 0600 file";

/// A read `agentd.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentdConfig {
    /// `[a-z0-9-]{1,32}`: the process's principal (AD-377).
    pub principal: String,
    /// `[a-z0-9-]{1,32}`: the host slug its chunks and manifest carry.
    pub host: String,
    pub always_on: bool,
    pub homeserver: Homeserver,
    pub drives: Vec<DrivePin>,
    pub providers: Vec<ProviderEntry>,
    pub agents: Vec<AgentsEntry>,
    pub trust: Vec<TrustEntry>,
    /// `[sandbox].read_exec`: absolute paths only. The architecture's other
    /// two rules — nothing inside a drive's checkout, nothing holding this
    /// host's secrets — need the host's directories, so the host that
    /// builds a run's sandbox checks them (`run::grant_refusal`), not the
    /// parser.
    pub read_exec: Vec<PathBuf>,
    /// `[sandbox].env` (R148): variables a run gets whose values are
    /// absolute paths — a toolchain's `RUSTUP_HOME`, `CARGO_HOME` — mounted
    /// read-and-execute, checked as `read_exec` is. Never a name of the
    /// run's own environment, `KEEPER_*`, `LD_*` or `DYLD_*`.
    pub sandbox_env: Vec<(String, PathBuf)>,
    pub mcp: Vec<crate::agents::mcp::McpEntry>,
    pub kvm: Vec<KvmEntry>,
}

/// `[homeserver]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Homeserver {
    pub url: BaseUrl,
    /// `None` until `init` creates the principal's control room.
    pub control_room: Option<OwnedRoomId>,
}

/// A `secret:<name>` reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    /// The `<name>`: the store's key.
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// `[[drives]]`: a drive this host mounts, with the pin of its audience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrivePin {
    /// `[a-z0-9][a-z0-9-]{0,31}`, equal to its `_drive.toml` `id`.
    pub id: String,
    /// The git remote, as `git clone` takes it.
    pub remote: String,
    pub credential: Option<SecretRef>,
    pub owner: OwnedUserId,
    pub readers: BTreeSet<OwnedUserId>,
    /// Every agent homed here must use a local model. Pinned because
    /// `_drive.toml` is reader-editable: a file that says `false` where the
    /// pin says `true` hosts nothing, so no reader can open the drive to a
    /// remote model by editing it. `false` when the key is absent.
    pub local_only: bool,
}

impl DrivePin {
    /// The pinned readers as a label's readers.
    pub fn readers(&self) -> Readers {
        Readers::Only(self.readers.clone())
    }
}

/// `[[providers]]`: a provider row this host keeps (no secret in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEntry {
    pub kind: ProviderKind,
    pub base_url: BaseUrl,
    pub credential: Option<SecretRef>,
}

/// `[[agents]]`: which homes of a drive this host serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsEntry {
    pub drive: String,
    pub ids: Vec<String>,
}

/// `[[trust]]`: a person whose decisions this host may accept once pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustEntry {
    pub user: OwnedUserId,
    /// `ed25519:<unpadded base64>`; `None` is "not pinned yet", never trusted.
    pub master_key: Option<String>,
    /// That person's proxy agent user, whose hand-off invites this host joins.
    pub proxy: Option<OwnedUserId>,
}

/// `[[kvm]]`: the one table that owns a KVM's audience, certificate and credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvmEntry {
    pub id: String,
    pub kind: String,
    pub url: String,
    pub credential: SecretRef,
    pub fingerprint: String,
    pub readers: Readers,
}

/// Why `agentd.toml` cannot be used. Each is the sentence `keeper-agentd` prints.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigRefusal {
    /// Not TOML, an unknown key, a missing key or a wrong type: the parser's
    /// own sentence, which names the key.
    #[error("agentd.toml is refused: {0}")]
    Syntax(String),
    #[error("agentd.toml is version {found}; this keeper-agentd reads version {GRAMMAR_VERSION}.")]
    Version { found: i64 },
    #[error("{at} in agentd.toml is refused: {reason}")]
    Invalid { at: String, reason: String },
}

impl ConfigRefusal {
    /// The refusal as the sentence an operator reads.
    pub fn sentence(&self) -> String {
        self.to_string()
    }

    /// Where and why, for a check shared with a host that reads no
    /// `agentd.toml` ([`crate::agents::mcp::check`]).
    pub(crate) fn parts(self) -> (String, String) {
        match self {
            ConfigRefusal::Invalid { at, reason } => (at, reason),
            other => (String::new(), other.sentence()),
        }
    }
}

fn invalid(at: impl Into<String>, reason: impl Into<String>) -> ConfigRefusal {
    ConfigRefusal::Invalid {
        at: at.into(),
        reason: reason.into(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    principal: String,
    host: String,
    #[serde(default)]
    always_on: bool,
    homeserver: RawHomeserver,
    #[serde(default)]
    drives: Vec<RawDrive>,
    #[serde(default)]
    providers: Vec<RawProvider>,
    #[serde(default)]
    agents: Vec<RawAgents>,
    #[serde(default)]
    trust: Vec<RawTrust>,
    #[serde(default)]
    sandbox: Option<RawSandbox>,
    #[serde(default)]
    mcp: Vec<crate::agents::mcp::RawMcp>,
    #[serde(default)]
    kvm: Vec<RawKvm>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHomeserver {
    url: String,
    #[serde(default)]
    control_room: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDrive {
    id: String,
    remote: String,
    credential: Option<String>,
    owner: Option<String>,
    readers: Option<Vec<String>>,
    #[serde(default)]
    local_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    kind: String,
    base_url: String,
    credential: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAgents {
    drive: String,
    ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTrust {
    user: String,
    master_key: Option<String>,
    proxy: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSandbox {
    #[serde(default)]
    read_exec: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKvm {
    id: String,
    kind: String,
    url: String,
    credential: String,
    fingerprint: String,
    readers: Vec<String>,
}

impl AgentdConfig {
    /// Read `agentd.toml`.
    pub fn parse(text: &str) -> Result<AgentdConfig, ConfigRefusal> {
        let syntax =
            |error: toml::de::Error| ConfigRefusal::Syntax(error.message().trim().to_owned());
        // The version first, from the loose table: a file of another version
        // is refused as that, not for a key this build does not know.
        let mut loose: toml::Table = crate::toml_order::from_str(text).map_err(syntax)?;
        match loose.remove("version") {
            Some(toml::Value::Integer(GRAMMAR_VERSION)) => {}
            Some(toml::Value::Integer(found)) => return Err(ConfigRefusal::Version { found }),
            Some(other) => {
                return Err(ConfigRefusal::Syntax(format!(
                    "`version` must be an integer, not {}",
                    other.type_str()
                )))
            }
            None => return Err(ConfigRefusal::Syntax("missing field `version`".to_owned())),
        }
        let raw: RawConfig = toml::Value::Table(loose).try_into().map_err(syntax)?;
        if !fits_slug(&raw.principal) {
            return Err(invalid("`principal`", slug_reason(&raw.principal)));
        }
        if !fits_slug(&raw.host) {
            return Err(invalid("`host`", slug_reason(&raw.host)));
        }

        let homeserver = Homeserver {
            url: parse_base_url(&raw.homeserver.url)
                .map_err(|error| invalid("[homeserver] `url`", error.to_string()))?,
            control_room: match raw.homeserver.control_room.trim() {
                "" => None,
                room => Some(OwnedRoomId::try_from(room).map_err(|_| {
                    invalid(
                        "[homeserver] `control_room`",
                        format!("\"{room}\" is not a Matrix room id (!id:server)"),
                    )
                })?),
            },
        };

        let mut drive_ids = HashSet::new();
        let mut remotes = HashMap::new();
        let mut drives = Vec::with_capacity(raw.drives.len());
        for entry in raw.drives {
            drives.push(drive_pin(entry, &mut drive_ids, &mut remotes)?);
        }

        let providers = raw
            .providers
            .into_iter()
            .map(|entry| {
                let at = format!("[[providers]] \"{}\"", entry.base_url);
                let kind = ProviderKind::from_registry_str(&entry.kind).ok_or_else(|| {
                    invalid(
                        format!("{at} `kind`"),
                        format!(
                            "\"{}\" is not a provider kind; write \"openai\", \"ollama\" or \"hermes\"",
                            entry.kind
                        ),
                    )
                })?;
                let base_url = parse_base_url(&entry.base_url)
                    .map_err(|error| invalid(format!("{at} `base_url`"), error.to_string()))?;
                Ok(ProviderEntry {
                    kind,
                    base_url,
                    credential: secret(entry.credential.as_deref(), &at)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let agents = raw
            .agents
            .into_iter()
            .map(|entry| {
                if !drive_ids.contains(&entry.drive) {
                    return Err(invalid(
                        "[[agents]] `drive`",
                        format!(
                            "\"{}\" is not the id of any [[drives]] entry, so its agents have no drive to live in",
                            entry.drive
                        ),
                    ));
                }
                Ok(AgentsEntry {
                    drive: entry.drive,
                    ids: entry.ids,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let trust = raw
            .trust
            .into_iter()
            .map(|entry| {
                let user = user_id("[[trust]] `user`", &entry.user)?;
                let at = format!("[[trust]] \"{user}\"");
                if let Some(key) = &entry.master_key {
                    if !fits_master_key(key) {
                        return Err(invalid(
                            format!("{at} `master_key`"),
                            "a master key is written ed25519:<unpadded base64 of 32 bytes>, as the person's device shows it",
                        ));
                    }
                }
                let proxy = entry
                    .proxy
                    .as_deref()
                    .map(|proxy| user_id(&format!("{at} `proxy`"), proxy))
                    .transpose()?;
                Ok(TrustEntry {
                    user,
                    master_key: entry.master_key,
                    proxy,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let sandbox = raw.sandbox.unwrap_or(RawSandbox {
            read_exec: Vec::new(),
            env: Default::default(),
        });
        let crate::agents::run::SandboxTable {
            read_exec,
            env: sandbox_env,
        } = crate::agents::run::SandboxTable::check(sandbox.read_exec, sandbox.env)
            .map_err(|(at, reason)| invalid(at, reason))?;

        let mut kvm_ids = HashSet::new();
        let kvm = raw
            .kvm
            .into_iter()
            .map(|entry| {
                let at = format!("[[kvm]] \"{}\"", entry.id);
                if !kvm_ids.insert(entry.id.clone()) {
                    return Err(invalid(
                        &at,
                        "another [[kvm]] entry has this id; each KVM is named once",
                    ));
                }
                if !matches!(entry.kind.as_str(), "nanokvm" | "nanokvm-go") {
                    return Err(invalid(
                        format!("{at} `kind`"),
                        format!(
                            "\"{}\" is not a KVM kind; write \"nanokvm\" or \"nanokvm-go\"",
                            entry.kind
                        ),
                    ));
                }
                let credential = secret(Some(&entry.credential), &at)?
                    .ok_or_else(|| invalid(&at, SECRET_SENTENCE))?;
                Ok(KvmEntry {
                    readers: readers_of(&entry.readers, &at)?,
                    fingerprint: fingerprint(&entry.fingerprint, &at)?,
                    id: entry.id,
                    kind: entry.kind,
                    url: entry.url,
                    credential,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mcp = crate::agents::mcp::check(raw.mcp, &kvm, crate::agents::mcp::AGENTD)
            .map_err(|(at, reason)| invalid(at, reason))?;

        Ok(AgentdConfig {
            principal: raw.principal,
            host: raw.host,
            always_on: raw.always_on,
            homeserver,
            drives,
            providers,
            agents,
            trust,
            read_exec,
            sandbox_env,
            mcp,
            kvm,
        })
    }

    /// The pin of drive `id`.
    pub fn drive(&self, id: &str) -> Option<&DrivePin> {
        self.drives.iter().find(|drive| drive.id == id)
    }

    /// The drives this host homes agents in: every `[[agents]] drive`.
    pub fn home_drives(&self) -> Vec<&str> {
        let mut homes: Vec<&str> = self
            .agents
            .iter()
            .map(|entry| entry.drive.as_str())
            .collect();
        homes.sort_unstable();
        homes.dedup();
        homes
    }

    /// Every `secret:<name>` the file names, each once.
    pub fn secrets(&self) -> Vec<&SecretRef> {
        let mut all: Vec<&SecretRef> = self
            .drives
            .iter()
            .filter_map(|drive| drive.credential.as_ref())
            .chain(self.providers.iter().filter_map(|p| p.credential.as_ref()))
            .chain(self.mcp.iter().filter_map(|m| m.credential.as_ref()))
            .chain(self.kvm.iter().map(|k| &k.credential))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all.dedup();
        all
    }
}

fn drive_pin(
    raw: RawDrive,
    seen: &mut HashSet<String>,
    remotes: &mut HashMap<String, String>,
) -> Result<DrivePin, ConfigRefusal> {
    let at = format!("[[drives]] \"{}\"", raw.id);
    if !fits_drive_id(&raw.id) {
        return Err(invalid(
            "[[drives]] `id`",
            format!(
                "\"{}\" does not fit [a-z0-9][a-z0-9-]{{0,31}}: lowercase letters, digits and dashes, at most 32",
                raw.id
            ),
        ));
    }
    if !seen.insert(raw.id.clone()) {
        return Err(invalid(&at, "the id is listed twice"));
    }
    if raw.remote.trim().is_empty() {
        return Err(invalid(&at, "`remote` names no repository"));
    }
    // One repository is one drive: two ids over one remote would be two
    // checkouts of it under two pins. The remote is not echoed, since an
    // operator may have pasted a credential into its URL.
    if let Some(first) = remotes.insert(raw.remote.trim().to_owned(), raw.id.clone()) {
        return Err(invalid(
            &at,
            format!("its `remote` is listed twice: [[drives]] \"{first}\" already names it"),
        ));
    }
    let pin_needed = |key: &str| {
        invalid(
            &at,
            format!(
                "it needs `{key}`: the pin of the drive's audience, copied from the forge's collaborators for its repository"
            ),
        )
    };
    let owner = raw.owner.ok_or_else(|| pin_needed("owner"))?;
    let readers = raw.readers.ok_or_else(|| pin_needed("readers"))?;
    let owner = user_id(&format!("{at} `owner`"), &owner)?;
    let readers =
        drive::audience(&owner, &readers).map_err(|refusal| invalid(&at, refusal.sentence()))?;
    Ok(DrivePin {
        credential: secret(raw.credential.as_deref(), &at)?,
        id: raw.id,
        remote: raw.remote,
        owner,
        readers,
        local_only: raw.local_only,
    })
}

/// `secret:<name>`, `<name>` of `[A-Za-z0-9_-]{1,64}`; anything else is a
/// secret pasted into the file.
pub(crate) fn secret(raw: Option<&str>, at: &str) -> Result<Option<SecretRef>, ConfigRefusal> {
    let Some(raw) = raw else { return Ok(None) };
    let name = raw
        .strip_prefix("secret:")
        .filter(|name| {
            (1..=64).contains(&name.len())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or_else(|| invalid(format!("{at} `credential`"), SECRET_SENTENCE))?;
    Ok(Some(SecretRef(name.to_owned())))
}

pub(crate) fn readers_of(list: &[String], at: &str) -> Result<Readers, ConfigRefusal> {
    if list.len() == 1 && list[0] == "*" {
        return Ok(Readers::Anyone);
    }
    let mut set = BTreeSet::new();
    for raw in list {
        set.insert(user_id(&format!("{at} `readers`"), raw)?);
    }
    Ok(Readers::Only(set))
}

pub(crate) fn fingerprint(raw: &str, at: &str) -> Result<String, ConfigRefusal> {
    let fits = raw
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
    if fits {
        Ok(raw.to_owned())
    } else {
        Err(invalid(
            format!("{at} `fingerprint`"),
            "a fingerprint is written sha256: and 64 hex digits",
        ))
    }
}

fn user_id(at: &str, raw: &str) -> Result<OwnedUserId, ConfigRefusal> {
    OwnedUserId::try_from(raw).map_err(|_| {
        invalid(
            at,
            format!("\"{raw}\" is not a Matrix user id (@name:server)"),
        )
    })
}

/// `ed25519:` and the unpadded base64 of a 32-byte key (43 characters).
fn fits_master_key(raw: &str) -> bool {
    raw.strip_prefix("ed25519:").is_some_and(|key| {
        key.len() == 43
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    })
}

/// `[a-z0-9-]{1,32}`: a host slug and a principal (`log::HostSlug`'s rule).
pub fn fits_slug(slug: &str) -> bool {
    (1..=32).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn slug_reason(slug: &str) -> String {
    format!("\"{slug}\" does not fit [a-z0-9-]{{1,32}}: lowercase letters, digits and dashes, at most 32")
}

fn fits_drive_id(id: &str) -> bool {
    id.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && fits_slug(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The architecture's example (*Data formats*), placeholders filled.
    const EXAMPLE: &str = r#"
version   = 1
principal = "tgorka"
host      = "electra"
always_on = true

[homeserver]
url          = "https://matrix.example.org"
control_room = "!control:example.org"

[[drives]]
id         = "tgdrive"
remote     = "https://forge.example.org/tgorka/tgdrive.git"
credential = "secret:tgdrive"
owner      = "@tgorka:example.org"
readers    = ["@tgorka:example.org"]

[[providers]]
kind       = "openai"
base_url   = "https://cliproxy.example.org:8452"
credential = "secret:cliproxy"

[[agents]]
drive = "tgdrive"
ids   = ["nixi", "tola-grey", "amelia"]

[[trust]]
user       = "@tgorka:example.org"
master_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
proxy      = "@nixi:example.org"

[sandbox]
read_exec = ["/opt/toolchains/bin"]

[[mcp]]
name       = "paseo"
url        = "https://paseo.example.org/mcp"
credential = "secret:paseo"
role       = "paseo"
readers    = ["*"]

[[mcp]]
name              = "notes-tools"
command           = ["/usr/local/bin/notes-mcp", "--stdio"]
readers           = ["@tgorka:example.org"]
trust_annotations = false

[[mcp.tier]]
tool = "search"
tier = "T0"

[[kvm]]
id          = "desk"
kind        = "nanokvm"
url         = "https://kvm.example.org"
credential  = "secret:desk-kvm"
fingerprint = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
readers     = ["@tgorka:example.org"]
"#;

    fn refused(text: &str) -> String {
        AgentdConfig::parse(text)
            .expect_err("must be refused")
            .sentence()
    }

    #[test]
    fn the_architectures_example_parses() {
        let config = AgentdConfig::parse(EXAMPLE).expect("the example parses");
        assert_eq!(config.host, "electra");
        assert!(config.always_on);
        assert_eq!(config.drives[0].readers.len(), 1);
        assert_eq!(config.providers[0].kind, ProviderKind::OpenAi);
        assert_eq!(config.agents[0].ids, ["nixi", "tola-grey", "amelia"]);
        assert_eq!(config.mcp[0].readers, Readers::Anyone);
        assert_eq!(
            config.mcp[1].tiers,
            [("search".to_owned(), crate::agents::tier::Tier::T0)]
        );
        let names: Vec<&str> = config.secrets().iter().map(|s| s.name()).collect();
        assert_eq!(names, ["cliproxy", "desk-kvm", "paseo", "tgdrive"]);
    }

    #[test]
    fn a_pasted_secret_is_refused_with_the_sentence() {
        let text = EXAMPLE.replace("\"secret:cliproxy\"", "\"ghp_0123456789abcdef\"");
        let sentence = refused(&text);
        assert!(sentence.contains(SECRET_SENTENCE), "{sentence}");
        assert!(
            !sentence.contains("ghp_"),
            "never echo the secret: {sentence}"
        );
    }

    #[test]
    fn an_agents_drive_must_be_a_drives_id() {
        let text = EXAMPLE.replace("drive = \"tgdrive\"", "drive = \"neuradrive\"");
        assert!(refused(&text).contains("\"neuradrive\" is not the id of any [[drives]]"));
    }

    #[test]
    fn a_host_slug_is_lowercase() {
        let text = EXAMPLE.replace("\"electra\"", "\"Electra\"");
        assert!(refused(&text).contains("`host`"));
    }

    #[test]
    fn an_unknown_key_is_refused_naming_it() {
        let text = EXAMPLE.replace("always_on = true", "always_on = true\nflavour = \"x\"");
        assert!(refused(&text).contains("flavour"));
        let text = EXAMPLE.replace("ids   = [", "colour = 1\nids   = [");
        assert!(refused(&text).contains("colour"));
    }

    #[test]
    fn a_provider_kind_is_a_registry_string() {
        let text = EXAMPLE.replace("kind       = \"openai\"", "kind       = \"omp\"");
        assert!(refused(&text).contains("\"omp\" is not a provider kind"));
    }

    #[test]
    fn trust_without_a_key_is_not_pinned_and_a_bad_key_or_proxy_is_refused() {
        let text = EXAMPLE.replace(
            "master_key = \"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\n",
            "",
        );
        let config = AgentdConfig::parse(&text).expect("no key parses");
        assert_eq!(config.trust[0].master_key, None);

        let text = EXAMPLE.replace("ed25519:AAAA", "rsa:AAAA");
        assert!(refused(&text).contains("`master_key`"));
        let text = EXAMPLE.replace("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"", "AAAA=\"");
        assert!(refused(&text).contains("`master_key`"));
        let text = EXAMPLE.replace("proxy      = \"@nixi:example.org\"", "proxy = \"nixi\"");
        assert!(refused(&text).contains("`proxy`"));
    }

    #[test]
    fn a_drive_needs_its_pin_and_its_owner_among_the_readers() {
        let text = EXAMPLE.replace("owner      = \"@tgorka:example.org\"\n", "");
        let sentence = refused(&text);
        assert!(sentence.contains("[[drives]] \"tgdrive\""), "{sentence}");
        assert!(sentence.contains("`owner`"), "{sentence}");

        let text = EXAMPLE.replace(
            "readers    = [\"@tgorka:example.org\"]\n\n[[providers]]",
            "\n[[providers]]",
        );
        let sentence = refused(&text);
        assert!(sentence.contains("[[drives]] \"tgdrive\""), "{sentence}");
        assert!(sentence.contains("`readers`"), "{sentence}");

        let text = EXAMPLE.replace(
            "owner      = \"@tgorka:example.org\"",
            "owner      = \"@marta:example.org\"",
        );
        let sentence = refused(&text);
        assert!(sentence.contains("[[drives]] \"tgdrive\""), "{sentence}");
        assert!(sentence.contains("is not among `readers`"), "{sentence}");
    }

    #[test]
    fn a_drives_local_only_is_pinned_and_absent_reads_false() {
        let config = AgentdConfig::parse(EXAMPLE).expect("parses");
        assert!(!config.drives[0].local_only);

        let text = EXAMPLE.replace(
            "readers    = [\"@tgorka:example.org\"]\n\n[[providers]]",
            "readers    = [\"@tgorka:example.org\"]\nlocal_only = true\n\n[[providers]]",
        );
        let config = AgentdConfig::parse(&text).expect("local_only parses");
        assert!(config.drives[0].local_only);

        let text = text.replace("local_only = true", "local_only = \"yes\"");
        assert!(refused(&text).contains("expected a boolean"));
    }

    #[test]
    fn a_remote_listed_under_two_drive_ids_is_refused_without_echoing_it() {
        let second = "\n[[drives]]\nid         = \"tgdrive-2\"\nremote     = \"https://forge.example.org/tgorka/tgdrive.git\"\nowner      = \"@tgorka:example.org\"\nreaders    = [\"@tgorka:example.org\"]\n\n[[providers]]";
        let text = EXAMPLE.replacen("\n[[providers]]", second, 1);
        let sentence = refused(&text);
        assert!(sentence.contains("[[drives]] \"tgdrive-2\""), "{sentence}");
        assert!(
            sentence
                .contains("its `remote` is listed twice: [[drives]] \"tgdrive\" already names it"),
            "{sentence}"
        );
        assert!(!sentence.contains("forge.example.org"), "{sentence}");

        let other = text.replacen(
            "\"https://forge.example.org/tgorka/tgdrive.git\"\nowner      = \"@tgorka:example.org\"\nreaders    = [\"@tgorka:example.org\"]\n\n[[providers]]",
            "\"https://forge.example.org/tgorka/other.git\"\nowner      = \"@tgorka:example.org\"\nreaders    = [\"@tgorka:example.org\"]\n\n[[providers]]",
            1,
        );
        assert_eq!(
            AgentdConfig::parse(&other)
                .expect("two remotes")
                .drives
                .len(),
            2
        );
    }

    #[test]
    fn a_future_version_is_refused_as_a_version_before_any_key_is_read() {
        let text = EXAMPLE
            .replace("version   = 1", "version   = 2")
            .replace("always_on = true", "always_on = true\nflavour = \"x\"");
        assert_eq!(
            AgentdConfig::parse(&text).expect_err("refused"),
            ConfigRefusal::Version { found: 2 }
        );
        assert!(refused(&EXAMPLE.replace("version   = 1\n", "")).contains("`version`"));
    }

    #[test]
    fn a_kvm_role_must_name_a_kvm() {
        let text = EXAMPLE.replace(
            "role       = \"paseo\"\nreaders    = [\"*\"]",
            "role       = \"kvm:den\"",
        );
        let text = text.replace("credential = \"secret:paseo\"\n", "");
        let sentence = refused(&text);
        assert!(sentence.contains("[[mcp]] \"paseo\""), "{sentence}");
        assert!(sentence.contains("[[kvm]] id \"den\""), "{sentence}");

        let text = EXAMPLE
            .replace(
                "role       = \"paseo\"\nreaders    = [\"*\"]",
                "role       = \"kvm:desk\"",
            )
            .replace("credential = \"secret:paseo\"\n", "");
        let config = AgentdConfig::parse(&text).expect("a kvm role naming desk parses");
        assert_eq!(
            config.mcp[0].role,
            Some(crate::agents::mcp::McpRole::Kvm("desk".to_owned()))
        );
    }

    /// 96.2 #2: a KVM is named once; a second `[[kvm]]` of the same id is
    /// refused rather than one silently shadowing the other's readers.
    #[test]
    fn a_kvm_id_is_named_once() {
        let second = EXAMPLE.replace(
            "[[kvm]]\nid          = \"desk\"",
            "[[kvm]]\nid          = \"desk\"\nkind        = \"nanokvm-go\"\nurl         = \"https://other.example.org\"\ncredential  = \"secret:other\"\nfingerprint = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\"\nreaders     = [\"*\"]\n\n[[kvm]]\nid          = \"desk\"",
        );
        assert!(refused(&second).contains("each KVM is named once"));
        assert!(AgentdConfig::parse(&second.replacen("\"desk\"", "\"den\"", 1)).is_ok());
    }

    /// R148: a toolchain's homes reach a run as `[sandbox] env`, absolute
    /// paths under a name the run's own environment does not hold.
    #[test]
    fn sandbox_env_names_absolute_paths_keeper_does_not_set() {
        let with = |env: &str| {
            EXAMPLE.replace(
                "read_exec = [\"/opt/toolchains/bin\"]",
                &format!("read_exec = [\"/opt/toolchains/bin\"]\nenv = {{ {env} }}"),
            )
        };
        let config = AgentdConfig::parse(&with(
            "RUSTUP_HOME = \"/usr/local/rustup\", CARGO_HOME = \"/usr/local/cargo\"",
        ))
        .expect("parses");
        assert_eq!(
            config.sandbox_env,
            [
                ("CARGO_HOME".to_owned(), PathBuf::from("/usr/local/cargo")),
                ("RUSTUP_HOME".to_owned(), PathBuf::from("/usr/local/rustup")),
            ]
        );
        for env in [
            "CARGO_HOME = \"cargo\"",
            "HOME = \"/srv\"",
            "PATH = \"/opt/bin\"",
            "GIT_CONFIG_GLOBAL = \"/etc/gitconfig\"",
            "LD_PRELOAD = \"/tmp/x.so\"",
            "KEEPER_AGENTD_SECRET_X = \"/x\"",
            "cargo_home = \"/usr/local/cargo\"",
        ] {
            assert!(refused(&with(env)).contains("[sandbox] env"), "{env}");
        }
    }
}

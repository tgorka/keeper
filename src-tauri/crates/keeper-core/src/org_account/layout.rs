//! The config repository's layout, as pure plans over a read-only view of the
//! worktree (AD-312, AD-313).
//!
//! ```text
//! _template/keeper.toml               copied to <login>/keeper.toml
//! _template/class/<class>.toml        copied to <login>/keeper.<device>.toml
//! <login>/user.toml                   who this directory belongs to
//! <login>/keeper.toml                 the person's settings, every device
//! <login>/keeper.<device>.toml        the person's settings, one device
//! <login>/devices/<device>.toml       one registered device
//! <login>/settings.toml               synced preferences, every device
//! <login>/settings.<device>.toml      synced preferences, one device
//! <login>/{drives,bots,matrix}.toml   what the person uses, as offers
//! <login>/device.<device>.toml        this device's state, for a restore
//! ```
//!
//! Nothing here touches a file. `keeper-sync` hands the worktree in through
//! [`RepoFiles`], writes what [`plan`] returns, and pushes; every writer filters
//! through [`is_own_path`] first. Every plan is create-only: a file that exists
//! is never rewritten, so a second run plans nothing. The synced files are the
//! one exception, and [`is_rewritable`] names exactly them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::AccountError;

/// A read-only view of the repository's worktree, by `/`-separated
/// repo-relative path.
pub trait RepoFiles {
    /// The file's bytes, or `None` when it is absent (or is a directory).
    fn read(&self, rel: &str) -> Option<Vec<u8>>;
    /// The entry names directly inside `rel`; empty when it is absent.
    fn list_dir(&self, rel: &str) -> Vec<String>;
    /// Whether something other than a regular file sits at `rel` — a symlink
    /// (wherever it points) or a directory. keeper neither reads through nor
    /// writes over such an entry: the repository holds it, so a plan that
    /// created the file would be refused on every sync.
    fn is_non_regular(&self, rel: &str) -> bool {
        let _ = rel;
        false
    }
}

/// What kind of device this is (AD-314): it picks the class template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "DeviceClassVm")]
pub enum DeviceClass {
    Desktop,
    Tablet,
    Mobile,
}

impl DeviceClass {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceClass::Desktop => "desktop",
            DeviceClass::Tablet => "tablet",
            DeviceClass::Mobile => "mobile",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "desktop" => Some(DeviceClass::Desktop),
            "tablet" => Some(DeviceClass::Tablet),
            "mobile" => Some(DeviceClass::Mobile),
            _ => None,
        }
    }
}

/// The signed-in person, as `user.toml` records them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    pub login: String,
    pub display_name: String,
    pub sub: String,
    pub issuer: String,
}

/// Whose `<login>/` directory this is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// No `user.toml` yet: keeper may create the directory.
    Missing,
    /// It records this sign-in.
    Mine { display_name: Option<String> },
    /// It records someone else (or cannot be read): load nothing from it.
    NotMine { recorded: Option<String> },
}

const TEMPLATE_SHARED: &str = "_template/keeper.toml";

fn user_path(login: &str) -> String {
    format!("{login}/user.toml")
}

fn device_path(login: &str, device: &str) -> String {
    format!("{login}/devices/{device}.toml")
}

fn device_layer_path(login: &str, device: &str) -> String {
    format!("{login}/keeper.{device}.toml")
}

fn read_table(files: &dyn RepoFiles, rel: &str) -> Option<Result<toml::Table, ()>> {
    let bytes = files.read(rel)?;
    Some(
        std::str::from_utf8(&bytes)
            .map_err(|_| ())
            .and_then(|text| toml::from_str(text).map_err(|_| ())),
    )
}

fn string_field(table: &toml::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

/// Does `<login>/` belong to this sign-in? `user.toml[identity_field]` must equal
/// `sub`, and a recorded `issuer` must equal `issuer`. An unreadable
/// `user.toml` is not mine: keeper fails closed rather than load a stranger's
/// settings.
pub fn resolve(
    files: &dyn RepoFiles,
    login: &str,
    sub: &str,
    issuer: &str,
    identity_field: &str,
) -> Resolution {
    let path = user_path(login);
    if files.is_non_regular(&path) {
        return Resolution::NotMine { recorded: None };
    }
    let table = match read_table(files, &path) {
        None => return Resolution::Missing,
        Some(Err(())) => return Resolution::NotMine { recorded: None },
        Some(Ok(table)) => table,
    };
    let recorded = string_field(&table, identity_field);
    if recorded.as_deref() != Some(sub) {
        return Resolution::NotMine { recorded };
    }
    if string_field(&table, "issuer").is_some_and(|recorded_issuer| recorded_issuer != issuer) {
        return Resolution::NotMine { recorded };
    }
    Resolution::Mine {
        display_name: string_field(&table, "display_name"),
    }
}

pub struct PlanInput<'a> {
    pub login: &'a str,
    pub user: &'a UserRecord,
    pub identity_field: &'a str,
    /// The device slug ([`device_slug`]).
    pub device: &'a str,
    pub class: DeviceClass,
    pub platform: &'a str,
    /// This machine's fingerprint for this person (a hex digest the shell
    /// derives from the OS's machine id and the account `sub`), or `None`
    /// where the OS has no stable id (iOS).
    pub machine: Option<&'a str>,
    pub now_rfc3339: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWrite {
    pub rel: String,
    pub bytes: Vec<u8>,
}

/// The files this sign-in on this device still needs, and nothing that
/// exists: `user.toml`, `keeper.toml` from the template, this device's record,
/// and its class template when the repository has one. Empty when the
/// directory is someone else's. A target the repository holds as a link or a
/// folder is skipped; [`unusable_files`] says so.
pub fn plan(files: &dyn RepoFiles, input: &PlanInput) -> Vec<PlannedWrite> {
    let login = input.login;
    let user = input.user;
    if matches!(
        resolve(files, login, &user.sub, &user.issuer, input.identity_field),
        Resolution::NotMine { .. }
    ) {
        return Vec::new();
    }
    let mut writes = Vec::new();
    let mut create = |rel: String, bytes: Vec<u8>| {
        if is_own_path(login, &rel) && files.read(&rel).is_none() && !files.is_non_regular(&rel) {
            writes.push(PlannedWrite { rel, bytes });
        }
    };

    create(user_path(login), user_toml(input).into_bytes());
    if let Some(template) = files.read(TEMPLATE_SHARED) {
        create(format!("{login}/keeper.toml"), template);
    }
    let device = input.device;
    if device_slug(device) == device {
        let mut fields = vec![
            ("name", device),
            ("class", input.class.as_str()),
            ("platform", input.platform),
        ];
        if let Some(machine) = input.machine {
            fields.push(("machine", machine));
        }
        fields.push(("created", input.now_rfc3339));
        let record = toml_document(&fields);
        create(device_path(login, device), record.into_bytes());
        if let Some(template) =
            files.read(&format!("_template/class/{}.toml", input.class.as_str()))
        {
            create(device_layer_path(login, device), template);
        }
    }
    writes
}

/// One sentence per file of this person and device that the repository holds
/// as a link or a folder: keeper neither loads it nor replaces it, and the
/// person has to fix it in the repository.
pub fn unusable_files(files: &dyn RepoFiles, login: &str, device: &str) -> Vec<String> {
    use super::{device_state, manifest, settings_sync};
    [
        user_path(login),
        format!("{login}/keeper.toml"),
        device_layer_path(login, device),
        device_path(login, device),
        settings_sync::shared_path(login),
        settings_sync::device_path(login, device),
        manifest::drives_path(login),
        manifest::bots_path(login),
        manifest::matrix_path(login),
        device_state::path(login, device),
    ]
    .into_iter()
    .filter(|rel| files.is_non_regular(rel))
    .map(|rel| {
        format!(
            "{rel} in the settings repository is a link or a folder, not a file, so keeper leaves it alone. Replace it with a file in the repository."
        )
    })
    .collect()
}

/// How long a device's files must have gone untouched before a record
/// without a fingerprint is taken as this machine's own from before a
/// reinstall rather than another machine of the same name still in use.
pub const LEGACY_ADOPT_DAYS: u64 = 30;

/// This device's slug for a first registration: `wanted` as a slug, unless
/// `devices/<wanted>.toml` already exists, this install did not register it,
/// and it is not this machine's own record from before a reinstall — then
/// the same with a 4-hex suffix, so two machines with one host name never
/// share a device record and its settings. A record is this machine's own —
/// adopted, and the device restores from it — when it records this class and
/// platform, and either `devices/<wanted>.toml` or `device.<wanted>.toml`
/// carries this `machine` fingerprint, or neither carries one (a device from
/// before fingerprints) and its files have gone untouched for
/// [`LEGACY_ADOPT_DAYS`] (`legacy_untouched_days`, the shell's measure of
/// the newer of its two synced files; `None` when unknown).
// Each input is one fact about this install the shell reads separately; a
// struct would only move the same eight names one line down.
#[allow(clippy::too_many_arguments)]
pub fn free_device_slug(
    files: &dyn RepoFiles,
    login: &str,
    wanted: &str,
    registered_here: bool,
    class: DeviceClass,
    platform: &str,
    machine: Option<&str>,
    legacy_untouched_days: Option<u64>,
) -> String {
    let wanted = device_slug(wanted);
    let taken = |slug: &str| {
        let rel = device_path(login, slug);
        files.read(&rel).is_some() || files.is_non_regular(&rel)
    };
    let rel = device_path(login, &wanted);
    let state_rel = super::device_state::path(login, &wanted);
    let fingerprint = |rel: &str| {
        if files.is_non_regular(rel) {
            return None;
        }
        read_table(files, rel)
            .and_then(Result::ok)
            .and_then(|table| string_field(&table, "machine"))
    };
    let same_device = || {
        if files.is_non_regular(&rel) {
            return false;
        }
        let Some(Ok(record)) = read_table(files, &rel) else {
            return false;
        };
        if string_field(&record, "class").as_deref() != Some(class.as_str())
            || string_field(&record, "platform").as_deref() != Some(platform)
        {
            return false;
        }
        let recorded = [string_field(&record, "machine"), fingerprint(&state_rel)];
        if recorded.iter().all(Option::is_none) {
            return legacy_untouched_days.is_some_and(|days| days >= LEGACY_ADOPT_DAYS);
        }
        machine.is_some_and(|ours| recorded.iter().flatten().any(|theirs| theirs == ours))
    };
    if registered_here || !taken(&wanted) || same_device() {
        return wanted;
    }
    let mut base = wanted;
    base.truncate(MAX_SLUG - 5);
    let base = base.trim_end_matches('-');
    let mut candidate = String::new();
    // 65 536 suffixes; a handful of tries finds a free one in any real repo.
    for _ in 0..16 {
        candidate = format!("{base}-{:04x}", rand::random::<u16>());
        if !taken(&candidate) {
            break;
        }
    }
    candidate
}

fn user_toml(input: &PlanInput) -> String {
    let user = input.user;
    let identity = input.identity_field;
    let mut fields = vec![
        ("login", user.login.as_str()),
        ("display_name", user.display_name.as_str()),
        ("issuer", user.issuer.as_str()),
        ("created", input.now_rfc3339),
    ];
    // The identity field is the descriptor's to name; if it names one of
    // keeper's own keys, the identity wins and the file stays valid TOML.
    fields.retain(|(key, _)| *key != identity);
    fields.insert(fields.len().min(2), (identity, user.sub.as_str()));
    toml_document(&fields)
}

fn toml_document(fields: &[(&str, &str)]) -> String {
    let mut table = toml::Table::new();
    let mut out = String::new();
    for (key, value) in fields {
        table.clear();
        table.insert((*key).to_owned(), toml::Value::String((*value).to_owned()));
        // One key per table keeps the author's order; `toml` quotes the key
        // and escapes the value.
        out.push_str(&toml::to_string(&table).unwrap_or_default());
    }
    out
}

/// Whether `rel` is a file inside `<login>/` — the guard every writer calls.
/// Refuses absolute paths, backslashes, empty, `.` and `..` segments, and any
/// dot-file, so no plan can reach `.git`, a sibling directory or `_template/`.
pub fn is_own_path(login: &str, rel: &str) -> bool {
    let login_safe = !login.is_empty()
        && !login.starts_with(['.', '_'])
        && !login.contains(['/', '\\'])
        && login != "..";
    if !login_safe || rel.contains('\\') {
        return false;
    }
    let Some(inside) = rel
        .strip_prefix(login)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    !inside.is_empty()
        && inside
            .split('/')
            .all(|segment| !segment.is_empty() && !segment.starts_with('.'))
}

/// Whether keeper may replace `rel` when it already exists (AD-324, AD-328):
/// only the synced preference files, the three offer manifests and the
/// device state files directly inside `<login>/`. Everything else stays
/// create-only.
pub fn is_rewritable(login: &str, rel: &str) -> bool {
    if !is_own_path(login, rel) {
        return false;
    }
    let Some(name) = rel
        .strip_prefix(login)
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|name| !name.contains('/'))
    else {
        return false;
    };
    match name {
        "settings.toml" | "drives.toml" | "bots.toml" | "matrix.toml" => true,
        _ => name
            .strip_prefix("settings.")
            .or_else(|| name.strip_prefix("device."))
            .and_then(|rest| rest.strip_suffix(".toml"))
            .is_some_and(|slug| device_slug(slug) == slug),
    }
}

const MAX_SLUG: usize = 32;

/// A device name as a file-name slug: lower-case `[a-z0-9-]`, runs of anything
/// else collapsed to one `-`, trimmed, at most 32 bytes; empty is `device`.
pub fn device_slug(raw: &str) -> String {
    let mut slug = String::with_capacity(raw.len().min(MAX_SLUG));
    for c in raw.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= MAX_SLUG {
            break;
        }
    }
    slug.truncate(MAX_SLUG);
    let trimmed = slug.trim_end_matches('-');
    if trimmed.is_empty() {
        "device".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// One registered device, from `<login>/devices/<slug>.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    pub slug: String,
    pub name: String,
    pub class: Option<DeviceClass>,
    pub platform: Option<String>,
}

/// The person's registered devices, sorted by slug. A record that cannot be
/// read still lists its device, under its slug.
pub fn devices(files: &dyn RepoFiles, login: &str) -> Vec<DeviceEntry> {
    let mut entries: Vec<DeviceEntry> = files
        .list_dir(&format!("{login}/devices"))
        .into_iter()
        .filter_map(|name| {
            let slug = name.strip_suffix(".toml")?;
            (device_slug(slug) == slug).then(|| slug.to_owned())
        })
        .map(|slug| {
            let table = read_table(files, &device_path(login, &slug))
                .and_then(Result::ok)
                .unwrap_or_default();
            // A rename moves the record without rewriting it; a name that no
            // longer matches its file is stale, and the slug is the truth.
            let name = string_field(&table, "name")
                .filter(|name| device_slug(name) == slug)
                .unwrap_or_else(|| slug.clone());
            DeviceEntry {
                name,
                class: string_field(&table, "class")
                    .as_deref()
                    .and_then(DeviceClass::parse),
                platform: string_field(&table, "platform"),
                slug,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.slug.cmp(&b.slug));
    entries
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOp {
    pub from: String,
    pub to: String,
}

/// Rename this device: move `devices/<from>.toml` and, when present,
/// `keeper.<from>.toml`, `settings.<from>.toml` and `device.<from>.toml`
/// inside `<login>/`. Refused when `from` is not registered, `to` is not a
/// slug, or `to` is already taken.
pub fn plan_rename(
    files: &dyn RepoFiles,
    login: &str,
    from: &str,
    to: &str,
) -> Result<Vec<RenameOp>, AccountError> {
    if device_slug(to) != to {
        return Err(AccountError::Refused(format!(
            "\"{to}\" cannot be a device name; use a-z, 0-9 and -."
        )));
    }
    let from_record = device_path(login, from);
    if device_slug(from) != from || files.read(&from_record).is_none() {
        return Err(AccountError::Refused(format!(
            "The device \"{from}\" is not registered in your settings repository."
        )));
    }
    if from == to {
        return Ok(Vec::new());
    }
    let to_record = device_path(login, to);
    let layers = [
        (device_layer_path(login, from), device_layer_path(login, to)),
        (
            super::settings_sync::device_path(login, from),
            super::settings_sync::device_path(login, to),
        ),
        (
            super::device_state::path(login, from),
            super::device_state::path(login, to),
        ),
    ];
    let taken = |rel: &String| files.read(rel).is_some();
    if taken(&to_record) || layers.iter().any(|(_, moved)| taken(moved)) {
        return Err(AccountError::Refused(format!(
            "A device named \"{to}\" is already registered. Pick another name."
        )));
    }
    let mut ops = vec![RenameOp {
        from: from_record,
        to: to_record,
    }];
    for (source, destination) in layers {
        if files.read(&source).is_some() {
            ops.push(RenameOp {
                from: source,
                to: destination,
            });
        }
    }
    if ops
        .iter()
        .all(|op| is_own_path(login, &op.from) && is_own_path(login, &op.to))
    {
        Ok(ops)
    } else {
        Err(AccountError::Refused(format!(
            "\"{login}\" cannot name a folder in the settings repository."
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A worktree as a map of repo-relative paths to bytes.
    #[derive(Default)]
    struct Tree(BTreeMap<String, Vec<u8>>);

    impl Tree {
        fn with(mut self, rel: &str, text: &str) -> Self {
            self.0.insert(rel.to_owned(), text.as_bytes().to_vec());
            self
        }

        fn apply(&mut self, writes: &[PlannedWrite]) {
            for write in writes {
                self.0.insert(write.rel.clone(), write.bytes.clone());
            }
        }

        fn text(&self, rel: &str) -> String {
            String::from_utf8(self.0[rel].clone()).expect("utf-8")
        }
    }

    impl RepoFiles for Tree {
        fn read(&self, rel: &str) -> Option<Vec<u8>> {
            self.0.get(rel).cloned()
        }

        fn list_dir(&self, rel: &str) -> Vec<String> {
            let prefix = format!("{rel}/");
            self.0
                .keys()
                .filter_map(|path| path.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_owned)
                .collect()
        }
    }

    const ISSUER: &str = "https://id.acme.dev";

    fn user() -> UserRecord {
        UserRecord {
            login: "tgorka".to_owned(),
            display_name: "Tomasz Gorka".to_owned(),
            sub: "283746519283746777".to_owned(),
            issuer: ISSUER.to_owned(),
        }
    }

    fn input<'a>(user: &'a UserRecord, identity_field: &'a str) -> PlanInput<'a> {
        PlanInput {
            login: "tgorka",
            user,
            identity_field,
            device: "work-mac",
            class: DeviceClass::Desktop,
            platform: "macos",
            machine: Some("5eed"),
            now_rfc3339: "2026-09-23T10:12:00Z",
        }
    }

    fn templates() -> Tree {
        Tree::default()
            .with(
                "_template/keeper.toml",
                "[settings]\n\"notes.enabled\" = true\n",
            )
            .with(
                "_template/class/desktop.toml",
                "[settings]\n\"hotkey.global\" = \"Cmd+K\"\n",
            )
    }

    #[test]
    fn resolution_follows_the_configured_identity_field_and_a_recorded_issuer() {
        let sub = "283746519283746777";
        let resolve_in = |tree: &Tree, field: &str| resolve(tree, "tgorka", sub, ISSUER, field);

        assert_eq!(resolve_in(&Tree::default(), "sub"), Resolution::Missing);

        let mine = Tree::default().with(
            "tgorka/user.toml",
            &format!("login = \"tgorka\"\ndisplay_name = \"Tom\"\nsub = \"{sub}\"\nissuer = \"{ISSUER}\"\n"),
        );
        assert_eq!(
            resolve_in(&mine, "sub"),
            Resolution::Mine {
                display_name: Some("Tom".to_owned())
            }
        );

        // A repo that records `zitadel_id`: only that field decides.
        let zitadel = Tree::default().with(
            "tgorka/user.toml",
            &format!("sub = \"someone-else\"\nzitadel_id = \"{sub}\"\n"),
        );
        assert_eq!(
            resolve_in(&zitadel, "zitadel_id"),
            Resolution::Mine { display_name: None }
        );
        assert_eq!(
            resolve_in(&zitadel, "sub"),
            Resolution::NotMine {
                recorded: Some("someone-else".to_owned())
            }
        );
        assert_eq!(
            resolve_in(&mine, "zitadel_id"),
            Resolution::NotMine { recorded: None }
        );

        let other_issuer = Tree::default().with(
            "tgorka/user.toml",
            &format!("sub = \"{sub}\"\nissuer = \"https://id.elsewhere.dev\"\n"),
        );
        assert!(matches!(
            resolve_in(&other_issuer, "sub"),
            Resolution::NotMine { .. }
        ));

        let unreadable = Tree::default().with("tgorka/user.toml", "sub = ");
        assert_eq!(
            resolve_in(&unreadable, "sub"),
            Resolution::NotMine { recorded: None }
        );
    }

    #[test]
    fn a_first_sign_in_creates_the_directory_and_a_second_plans_nothing() {
        let user = user();
        let input = input(&user, "zitadel_id");
        let mut tree = templates();

        let first = plan(&tree, &input);
        let rels: Vec<&str> = first.iter().map(|w| w.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "tgorka/user.toml",
                "tgorka/keeper.toml",
                "tgorka/devices/work-mac.toml",
                "tgorka/keeper.work-mac.toml",
            ]
        );
        assert!(first.iter().all(|w| is_own_path("tgorka", &w.rel)));
        tree.apply(&first);

        assert_eq!(
            tree.text("tgorka/keeper.toml"),
            tree.text("_template/keeper.toml")
        );
        assert_eq!(
            tree.text("tgorka/keeper.work-mac.toml"),
            tree.text("_template/class/desktop.toml")
        );
        // What keeper wrote is what it later recognises as this sign-in's.
        assert_eq!(
            resolve(&tree, "tgorka", &user.sub, ISSUER, "zitadel_id"),
            Resolution::Mine {
                display_name: Some("Tomasz Gorka".to_owned())
            }
        );
        assert_eq!(
            devices(&tree, "tgorka"),
            [DeviceEntry {
                slug: "work-mac".to_owned(),
                name: "work-mac".to_owned(),
                class: Some(DeviceClass::Desktop),
                platform: Some("macos".to_owned()),
            }]
        );
        // After a reinstall, this machine's record is adopted; another
        // machine of the same name, class and platform gets its own.
        let desktop = DeviceClass::Desktop;
        assert_eq!(
            free_device_slug(
                &tree,
                "tgorka",
                "work-mac",
                false,
                desktop,
                "macos",
                Some("5eed"),
                None
            ),
            "work-mac"
        );
        assert_ne!(
            free_device_slug(
                &tree,
                "tgorka",
                "work-mac",
                false,
                desktop,
                "macos",
                Some("f00d"),
                Some(365)
            ),
            "work-mac"
        );

        assert!(
            plan(&tree, &input).is_empty(),
            "the second run writes nothing"
        );
    }

    #[test]
    fn existing_files_are_never_rewritten() {
        let user = user();
        let own = format!("sub = \"{}\"\n# hand-written\n", user.sub);
        let tree = templates()
            .with("tgorka/user.toml", &own)
            .with("tgorka/keeper.toml", "[settings]\n# mine\n");

        let writes = plan(&tree, &input(&user, "sub"));
        let rels: Vec<&str> = writes.iter().map(|w| w.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "tgorka/devices/work-mac.toml",
                "tgorka/keeper.work-mac.toml"
            ]
        );
    }

    #[test]
    fn a_directory_that_is_someone_elses_plans_nothing() {
        let user = user();
        let tree = templates().with("tgorka/user.toml", "sub = \"ana\"\n");
        assert!(plan(&tree, &input(&user, "sub")).is_empty());
    }

    #[test]
    fn no_class_template_means_no_device_layer() {
        let user = user();
        let tree = Tree::default().with("_template/keeper.toml", "");
        let rels: Vec<String> = plan(&tree, &input(&user, "sub"))
            .into_iter()
            .map(|w| w.rel)
            .collect();
        assert_eq!(
            rels,
            [
                "tgorka/user.toml",
                "tgorka/keeper.toml",
                "tgorka/devices/work-mac.toml"
            ]
        );
    }

    #[test]
    fn only_files_inside_the_own_directory_pass_the_guard() {
        for ok in ["tgorka/keeper.toml", "tgorka/devices/work-mac.toml"] {
            assert!(is_own_path("tgorka", ok), "{ok}");
        }
        for bad in [
            "ana/keeper.toml",
            "tgorka",
            "tgorka/",
            "tgorkax/keeper.toml",
            "/tgorka/keeper.toml",
            "tgorka/../ana/keeper.toml",
            "tgorka/./keeper.toml",
            "tgorka//keeper.toml",
            "tgorka/.git/config",
            "tgorka\\..\\ana",
            "_template/keeper.toml",
        ] {
            assert!(!is_own_path("tgorka", bad), "{bad}");
        }
        assert!(!is_own_path("_template", "_template/keeper.toml"));
        assert!(!is_own_path("..", "../keeper.toml"));
        assert!(!is_own_path("", "/keeper.toml"));
    }

    #[test]
    fn device_slugs_are_short_safe_file_names() {
        assert_eq!(device_slug("Tom's MacBook Pro"), "tom-s-macbook-pro");
        assert_eq!(device_slug("macbookpro.lan"), "macbookpro-lan");
        assert_eq!(device_slug("  --  "), "device");
        assert_eq!(device_slug(""), "device");
        let long = device_slug(&"ab-".repeat(40));
        assert!(long.len() <= 32 && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn devices_list_only_slug_records_and_trust_the_slug_over_a_stale_name() {
        let tree = Tree::default()
            .with(
                "tgorka/devices/ipad-3f2a.toml",
                "name = \"iPad 3F2A\"\nclass = \"tablet\"\n",
            )
            .with(
                "tgorka/devices/new-mac.toml",
                "name = \"old-mac\"\nclass = \"desktop\"\n",
            )
            .with("tgorka/devices/broken.toml", "name = ")
            .with("tgorka/devices/README.md", "")
            .with("tgorka/devices/Bad Name.toml", "");
        let listed: Vec<(String, String, Option<DeviceClass>)> = devices(&tree, "tgorka")
            .into_iter()
            .map(|d| (d.slug, d.name, d.class))
            .collect();
        assert_eq!(
            listed,
            [
                ("broken".to_owned(), "broken".to_owned(), None),
                (
                    "ipad-3f2a".to_owned(),
                    "iPad 3F2A".to_owned(),
                    Some(DeviceClass::Tablet)
                ),
                (
                    "new-mac".to_owned(),
                    "new-mac".to_owned(),
                    Some(DeviceClass::Desktop)
                ),
            ]
        );
    }

    #[test]
    fn a_rename_moves_every_device_file_and_refuses_collisions() {
        let tree = Tree::default()
            .with("tgorka/devices/work-mac.toml", "")
            .with("tgorka/keeper.work-mac.toml", "")
            .with("tgorka/settings.work-mac.toml", "")
            .with("tgorka/device.work-mac.toml", "")
            .with("tgorka/devices/home-mac.toml", "")
            .with("tgorka/settings.attic.toml", "");

        let ops = plan_rename(&tree, "tgorka", "work-mac", "studio").expect("rename");
        assert_eq!(
            ops,
            [
                RenameOp {
                    from: "tgorka/devices/work-mac.toml".to_owned(),
                    to: "tgorka/devices/studio.toml".to_owned(),
                },
                RenameOp {
                    from: "tgorka/keeper.work-mac.toml".to_owned(),
                    to: "tgorka/keeper.studio.toml".to_owned(),
                },
                RenameOp {
                    from: "tgorka/settings.work-mac.toml".to_owned(),
                    to: "tgorka/settings.studio.toml".to_owned(),
                },
                RenameOp {
                    from: "tgorka/device.work-mac.toml".to_owned(),
                    to: "tgorka/device.studio.toml".to_owned(),
                },
            ]
        );
        assert_eq!(
            plan_rename(&tree, "tgorka", "home-mac", "den")
                .expect("record only")
                .len(),
            1
        );
        for (from, to) in [
            ("work-mac", "home-mac"),
            ("work-mac", "attic"),
            ("gone", "den"),
            ("work-mac", "Den Mac"),
            ("work-mac", "../x"),
        ] {
            assert!(
                matches!(
                    plan_rename(&tree, "tgorka", from, to),
                    Err(AccountError::Refused(_))
                ),
                "{from} -> {to}"
            );
        }
    }

    #[test]
    fn only_the_synced_files_directly_in_the_own_directory_are_rewritable() {
        for rel in [
            "tgorka/settings.toml",
            "tgorka/settings.work-mac.toml",
            "tgorka/drives.toml",
            "tgorka/bots.toml",
            "tgorka/matrix.toml",
            "tgorka/device.work-mac.toml",
        ] {
            assert!(is_rewritable("tgorka", rel), "{rel}");
        }
        for rel in [
            "alice/settings.toml",
            "_template/settings.toml",
            "tgorka/_template/settings.toml",
            "tgorka/keeper.toml",
            "tgorka/keeper.work-mac.toml",
            "tgorka/user.toml",
            "tgorka/devices/work-mac.toml",
            "tgorka/devices/settings.toml",
            "tgorka/settings.Work Mac.toml",
            "tgorka/settings..toml",
            "tgorka/.settings.toml",
            "tgorka/notes.toml",
            "tgorka/device.Work Mac.toml",
            "tgorka/device..toml",
            "tgorka/devices.toml",
            "tgorka/devices/device.work-mac.toml",
        ] {
            assert!(!is_rewritable("tgorka", rel), "{rel}");
        }
        assert!(!is_rewritable("_template", "_template/settings.toml"));
    }

    /// A worktree whose `links` are symlinks (or folders) at those paths.
    struct Linked<'a> {
        tree: &'a Tree,
        links: &'a [&'a str],
    }

    impl RepoFiles for Linked<'_> {
        fn read(&self, rel: &str) -> Option<Vec<u8>> {
            // A link out of the clone reads as absent, as the shell's view does.
            if self.links.contains(&rel) {
                return None;
            }
            self.tree.read(rel)
        }

        fn list_dir(&self, rel: &str) -> Vec<String> {
            self.tree.list_dir(rel)
        }

        fn is_non_regular(&self, rel: &str) -> bool {
            self.links.contains(&rel)
        }
    }

    #[test]
    fn a_linked_target_is_never_planned_and_is_named_as_a_fault() {
        let tree = templates();
        let files = Linked {
            tree: &tree,
            links: &["tgorka/keeper.toml"],
        };
        let user = user();

        let rels: Vec<String> = plan(&files, &input(&user, "sub"))
            .into_iter()
            .map(|w| w.rel)
            .collect();

        assert!(!rels.contains(&"tgorka/keeper.toml".to_owned()), "{rels:?}");
        assert!(rels.contains(&"tgorka/user.toml".to_owned()), "{rels:?}");
        assert_eq!(
            unusable_files(&files, "tgorka", "work-mac"),
            ["tgorka/keeper.toml in the settings repository is a link or a folder, not a file, so keeper leaves it alone. Replace it with a file in the repository."]
        );

        // A linked user.toml is nobody's record: fail closed.
        let linked_user = Linked {
            tree: &tree,
            links: &["tgorka/user.toml"],
        };
        assert_eq!(
            resolve(&linked_user, "tgorka", "s", ISSUER, "sub"),
            Resolution::NotMine { recorded: None }
        );
        assert!(plan(&linked_user, &input(&user, "sub")).is_empty());
    }

    #[test]
    fn a_link_or_folder_at_a_synced_file_is_named_as_a_fault() {
        let tree = Tree::default();
        let files = Linked {
            tree: &tree,
            links: &[
                "tgorka/settings.toml",
                "tgorka/settings.work-mac.toml",
                "tgorka/drives.toml",
                "tgorka/bots.toml",
                "tgorka/matrix.toml",
                "tgorka/device.work-mac.toml",
            ],
        };
        let named: Vec<String> = unusable_files(&files, "tgorka", "work-mac")
            .into_iter()
            .map(|sentence| sentence.split(' ').next().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(
            named,
            [
                "tgorka/settings.toml",
                "tgorka/settings.work-mac.toml",
                "tgorka/drives.toml",
                "tgorka/bots.toml",
                "tgorka/matrix.toml",
                "tgorka/device.work-mac.toml",
            ]
        );
    }

    #[test]
    fn a_device_name_is_adopted_only_by_the_same_machine_and_suffixed_otherwise() {
        let legacy = "name = \"work-mac\"\nclass = \"desktop\"\nplatform = \"macos\"\n";
        let tree = Tree::default()
            .with("tgorka/devices/work-mac.toml", legacy)
            .with(
                "tgorka/devices/studio.toml",
                "name = \"studio\"\nclass = \"desktop\"\nplatform = \"macos\"\nmachine = \"5eed\"\n",
            )
            // Registered before fingerprints; its device file has one since.
            .with(
                "tgorka/devices/den.toml",
                "name = \"den\"\nclass = \"desktop\"\nplatform = \"macos\"\n",
            )
            .with("tgorka/device.den.toml", "machine = \"beef\"\n");
        let desktop = DeviceClass::Desktop;
        let slug = |wanted: &str, here: bool, class, platform, machine, days| {
            free_device_slug(
                &tree, "tgorka", wanted, here, class, platform, machine, days,
            )
        };
        let suffixed = |slug: String, base: &str| {
            let suffix = slug
                .strip_prefix(&format!("{base}-"))
                .unwrap_or_else(|| panic!("{slug} is not suffixed"));
            suffix.len() == 4 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
        };
        let old = Some(LEGACY_ADOPT_DAYS);

        assert_eq!(
            slug("Work Mac", true, desktop, "linux", None, None),
            "work-mac",
            "this install's own record is reused"
        );
        assert_eq!(slug("attic", false, desktop, "macos", None, None), "attic");

        assert_eq!(
            slug("studio", false, desktop, "macos", Some("5eed"), None),
            "studio",
            "same machine"
        );
        assert!(
            suffixed(
                slug("studio", false, desktop, "macos", Some("f00d"), old),
                "studio"
            ),
            "other machine, however old the files"
        );
        assert!(
            suffixed(slug("studio", false, desktop, "macos", None, old), "studio"),
            "no fingerprint never adopts a record that has one"
        );

        assert_eq!(
            slug("den", false, desktop, "macos", Some("beef"), None),
            "den",
            "the device file's fingerprint counts too"
        );
        assert!(suffixed(
            slug("den", false, desktop, "macos", Some("f00d"), old),
            "den"
        ));

        assert_eq!(
            slug("Work Mac", false, desktop, "macos", Some("f00d"), old),
            "work-mac",
            "a device from before fingerprints, untouched for a month, is adopted"
        );
        for days in [None, Some(LEGACY_ADOPT_DAYS - 1)] {
            assert!(
                suffixed(
                    slug("Work Mac", false, desktop, "macos", Some("f00d"), days),
                    "work-mac"
                ),
                "{days:?}: it may still be in use on another machine"
            );
        }
        for (class, platform) in [(desktop, "linux"), (DeviceClass::Tablet, "macos")] {
            assert!(suffixed(
                slug("Work Mac", false, class, platform, None, old),
                "work-mac"
            ));
        }

        // The suffix still fits the slug limit.
        let long = "a".repeat(32);
        let tree = Tree::default().with(&format!("tgorka/devices/{long}.toml"), "");
        let slug = free_device_slug(&tree, "tgorka", &long, false, desktop, "macos", None, old);
        assert!(slug.len() <= 32, "{slug}");
        assert_eq!(device_slug(&slug), slug);
        assert_ne!(
            slug, long,
            "a record without class or platform is never adopted"
        );
    }
}

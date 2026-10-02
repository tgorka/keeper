//! The headless platforms and the provider rows (story 90.3, acceptance 7–8).
#![cfg(unix)]

use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::Path;
use std::sync::Arc;

use keeper_agent::headless::{
    apply_providers, HeadlessPlatform, HeadlessSyncPlatform, SecretMap, SECRET_ENV_PREFIX,
};
use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::bots::{self, store};
use keeper_core::platform::Platform;
use keeper_core::vm::NotifyTarget;
use keeper_sync::xdg::SecretStore;
use keeper_sync::SyncPlatform;

fn strict_map(root: &Path) -> Arc<SecretMap> {
    let secrets = root.join("secrets");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&secrets)
        .expect("secrets dir");
    Arc::new(SecretMap::new(
        SecretStore::new(SECRET_ENV_PREFIX, secrets).strict(),
    ))
}

fn config(base_url: &str) -> AgentdConfig {
    AgentdConfig::parse(&format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\n\n\
         [homeserver]\nurl = \"https://matrix.example.org\"\n\n\
         [[providers]]\nkind = \"openai\"\nbase_url = \"{base_url}\"\ncredential = \"secret:cliproxy\"\n"
    ))
    .expect("parses")
}

#[test]
fn providers_are_rows_not_secrets() {
    let root = tempfile::tempdir().expect("tempdir");
    let map = strict_map(root.path());
    map.store()
        .set("cliproxy", "sk-the-token")
        .expect("seed the secret");
    let platform = HeadlessPlatform::new(root.path(), Arc::clone(&map));

    let first = apply_providers(
        &config("https://cliproxy.example.org:8452"),
        root.path(),
        &map,
    )
    .expect("apply");
    let second = apply_providers(
        &config("https://cliproxy.example.org:8452"),
        root.path(),
        &map,
    )
    .expect("apply again");
    assert_eq!(first, second, "applying twice changes nothing");
    let rows = store::list_providers(root.path()).expect("rows").rows;
    assert_eq!(rows.len(), 1);

    // The provider's token key resolves through secret:cliproxy.
    let id = &first[0].id;
    assert_eq!(
        bots::resolve_token(&platform, id, None).expect("resolve"),
        Some("sk-the-token".to_owned())
    );

    // keeper.db holds no token anywhere.
    let bytes = std::fs::read(root.path().join("keeper.db")).expect("keeper.db");
    assert!(
        !bytes
            .windows(b"sk-the-token".len())
            .any(|w| w == b"sk-the-token"),
        "the token must never reach keeper.db"
    );

    // A changed base_url updates the row, keeping its id.
    let moved = apply_providers(&config("https://proxy.example.org"), root.path(), &map)
        .expect("apply moved");
    let rows = store::list_providers(root.path()).expect("rows").rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider.base_url, "https://proxy.example.org");
    assert_eq!(&moved[0].id, id);
}

#[test]
fn the_headless_platforms_answer_every_method() {
    let root = tempfile::tempdir().expect("tempdir");
    let map = strict_map(root.path());
    let core = HeadlessPlatform::new(root.path().join("data"), Arc::clone(&map));
    let sync = HeadlessSyncPlatform::new(root.path().join("data"), "electra", Arc::clone(&map));

    assert_eq!(core.data_dir().expect("data"), root.path().join("data"));
    assert!(core.open_url("https://example.org").is_err());
    assert!(core.sidecar_path("x").is_err());
    assert!(core.notify("t", "b", &NotifyTarget::None).is_ok());
    assert!(core.exclude_from_backup(root.path()).is_ok());
    assert!(core.set_badge_count(Some(1)).is_ok());

    assert_eq!(sync.data_dir().expect("data"), root.path().join("data"));
    assert_eq!(sync.host_label(), "electra");
    assert_eq!(sync.free_space(root.path()), None);
    assert!(sync.bot_task_runner().is_none());
    assert!(sync.now_ms() > 1_577_836_800_000);

    // A keychain round trip lands in a 0600 file of the strict directory.
    let key = "agents/@nixi:example.org/session";
    core.keychain_set(key, "{\"session\":1}").expect("set");
    assert_eq!(
        core.keychain_get(key).expect("get"),
        Some("{\"session\":1}".to_owned())
    );
    let file = map.store().path_of(key);
    let mode = std::fs::metadata(&file).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    core.keychain_delete(key).expect("delete");
    assert_eq!(core.keychain_get(key).expect("gone"), None);
    assert!(!file.exists());

    // A drive credential bound to secret:<name> answers the engine's key.
    map.store().set("tgdrive", "forge-token").expect("seed");
    map.bind("sync/01P/credential", "tgdrive");
    assert_eq!(
        sync.secret_get("sync/01P/credential").expect("get"),
        Some("forge-token".to_owned())
    );
    // A bot's own token key, never configured on agentd, is absent.
    assert_eq!(core.keychain_get("bot_token/p/model").expect("get"), None);
}

/// A key bound to the operator's `secret:<name>` is read through, never
/// written or deleted through: the provider-setup write or a token delete in
/// core would otherwise overwrite or remove `secrets/cliproxy`.
#[test]
fn a_bound_key_is_never_written_or_deleted_through() {
    let root = tempfile::tempdir().expect("tempdir");
    let map = strict_map(root.path());
    let core = HeadlessPlatform::new(root.path().join("data"), Arc::clone(&map));
    map.store()
        .set("cliproxy", "the operator's")
        .expect("seed the secret");
    let key = bots::provider_token_key("p1");
    map.bind(key.clone(), "cliproxy");

    let refused = core
        .keychain_set(&key, "overwritten")
        .expect_err("a bound key is read-only")
        .to_string();
    assert!(
        refused
            .contains("is bound to secret:cliproxy by agentd.toml; change the file, not the store"),
        "{refused}"
    );
    assert!(core.keychain_delete(&key).is_err(), "delete is refused too");
    assert_eq!(
        map.store().get("cliproxy").expect("get"),
        Some("the operator's".to_owned())
    );
    assert!(
        !map.store().path_of(&key).exists(),
        "nothing is written beside it either"
    );
}

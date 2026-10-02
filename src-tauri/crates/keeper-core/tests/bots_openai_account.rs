//! An `openai` provider through the account's files (AD-369, story 89.6).
//!
//! The account's `bots.toml` and a device's `device.<name>.toml` carry a
//! provider's kind as an opaque string, so they need no change to carry the
//! third kind — and the restore that reads them back decodes it through
//! `ProviderKind::from_registry_str`, the one place a stored word becomes a
//! kind. These tests hold both halves: the word survives the files byte for
//! byte, and it decodes to `OpenAi` and nothing else.

use keeper_core::bots::ProviderKind;
use keeper_core::org_account::device_state::{DeviceStateFile, ProviderState};
use keeper_core::org_account::manifest::{offers, BotRecord, BotsFile, ProviderRecord};

fn astra() -> BotRecord {
    BotRecord {
        target: "gpt-6-astra".to_owned(),
        name: "Astra".to_owned(),
        ..BotRecord::default()
    }
}

#[test]
fn an_openai_provider_survives_the_accounts_bots_file() {
    let file = BotsFile {
        providers: vec![ProviderRecord {
            kind: "openai".to_owned(),
            name: "CLIProxyAPI".to_owned(),
            base_url: "https://cliproxy.acme.dev:8452".to_owned(),
            credential: "account".to_owned(),
            bots: vec![astra()],
            ..ProviderRecord::default()
        }],
        ..BotsFile::default()
    };
    let text = file.render().expect("renders");
    assert!(text.contains("kind = \"openai\""), "{text}");
    let back = BotsFile::parse(text.as_bytes()).expect("parses");
    assert_eq!(back, file);
    let kind = back.providers.first().map(|record| record.kind.as_str());
    assert_eq!(
        kind.and_then(ProviderKind::from_registry_str),
        Some(ProviderKind::OpenAi)
    );
}

#[test]
fn an_openai_provider_survives_a_device_file() {
    let file = DeviceStateFile {
        providers: vec![ProviderState {
            kind: "openai".to_owned(),
            name: "CLIProxyAPI".to_owned(),
            base_url: "https://cliproxy.acme.dev:8452".to_owned(),
            credential: "own".to_owned(),
            bots: vec![astra()],
            ..ProviderState::default()
        }],
        ..DeviceStateFile::default()
    };
    let text = file.render().expect("renders");
    let back = DeviceStateFile::parse(text.as_bytes()).expect("parses");
    assert!(back.values_eq(&file));
    let kind = back.providers.first().map(|state| state.kind.as_str());
    assert_eq!(
        kind.and_then(ProviderKind::from_registry_str),
        Some(ProviderKind::OpenAi)
    );
}

/// A provider is matched by kind and base URL, so an `openai` provider the
/// account holds is offered to a device that has the same endpoint saved as
/// another kind — the case AD-369 prevents, a CLIProxyAPI saved as `ollama` —
/// and not offered to one that already has it as `openai`.
#[test]
fn an_openai_provider_is_offered_by_its_own_kind() {
    let base_url = "https://cliproxy.acme.dev:8452".to_owned();
    let remote = [ProviderRecord {
        kind: "openai".to_owned(),
        name: "CLIProxyAPI".to_owned(),
        base_url: base_url.clone(),
        credential: "account".to_owned(),
        bots: vec![astra()],
        ..ProviderRecord::default()
    }];
    let as_ollama = [ProviderRecord {
        kind: "ollama".to_owned(),
        base_url: base_url.clone(),
        ..ProviderRecord::default()
    }];
    let offered = offers(&[], &remote, &[], &[], &as_ollama, &[]).providers;
    let shape: Vec<(&str, &str, &[String])> = offered
        .iter()
        .map(|offer| {
            (
                offer.kind.as_str(),
                offer.base_url.as_str(),
                offer.bots.as_slice(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [("openai", base_url.as_str(), &["Astra".to_owned()][..])]
    );
    assert!(offered[0].key.contains("openai"), "{}", offered[0].key);

    let as_openai = [ProviderRecord {
        kind: "openai".to_owned(),
        base_url,
        ..ProviderRecord::default()
    }];
    assert!(offers(&[], &remote, &[], &[], &as_openai, &[])
        .providers
        .is_empty());
}

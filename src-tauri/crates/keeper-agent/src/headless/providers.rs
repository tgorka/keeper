//! `apply_providers`: `agentd.toml`'s `[[providers]]` as `keeper.db` rows (story 90.3).

use std::path::Path;

use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::bots::{self, store, Provider};
use keeper_core::error::CoreError;
use keeper_core::org_account::settings_sync::ProviderRef;

use super::platform::SecretMap;
use crate::turn::{new_id, now_ms};

/// One provider row as the configuration made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedProvider {
    pub id: String,
    /// `provider:<kind>:<normalized base URL>`.
    pub reference: String,
}

/// Make `keeper.db`'s `bot_providers` exactly the file's `[[providers]]`,
/// and bind each row's token key to its `secret:<name>`.
///
/// Idempotent: a row whose `ProviderRef` the file names is kept as it is; a
/// row the file names by the same kind and credential under a changed
/// `base_url` is updated in place, so its id — and every bot and grant keyed
/// by it — survives; a row the file no longer names is deleted. No secret
/// enters the database: the row's name is the `secret:<name>` reference, and
/// the token is read through the [`SecretMap`] when a turn needs it.
pub fn apply_providers(
    config: &AgentdConfig,
    data_dir: &Path,
    secrets: &SecretMap,
) -> Result<Vec<AppliedProvider>, CoreError> {
    let mut rows = store::list_providers(data_dir)?.rows;
    let mut applied = Vec::with_capacity(config.providers.len());
    let mut unmatched: Vec<usize> = Vec::new();
    for (index, entry) in config.providers.iter().enumerate() {
        let reference =
            ProviderRef::new(entry.kind.as_registry_str(), &entry.base_url.normalized).reference();
        let found = rows.iter().position(|row| {
            row.provider.kind == entry.kind
                && ProviderRef::new(row.provider.kind.as_registry_str(), &row.provider.base_url)
                    .reference()
                    == reference
        });
        match found {
            Some(at) => {
                let row = rows.swap_remove(at);
                applied.push((index, row.provider.id));
            }
            None => unmatched.push(index),
        }
    }
    for index in unmatched {
        let entry = &config.providers[index];
        let name = row_name(entry);
        let same_credential = rows
            .iter()
            .position(|row| row.provider.kind == entry.kind && row.provider.name == name);
        let id = match same_credential {
            Some(at) => {
                let mut provider = rows.swap_remove(at).provider;
                provider.base_url = entry.base_url.normalized.clone();
                store::update_provider(data_dir, &provider)?;
                provider.id
            }
            None => {
                let provider = Provider {
                    id: new_id(),
                    kind: entry.kind,
                    name,
                    base_url: entry.base_url.normalized.clone(),
                    created_ms: now_ms(),
                };
                store::insert_provider(data_dir, &provider)?;
                provider.id
            }
        };
        applied.push((index, id));
    }
    for stale in rows {
        store::delete_provider(data_dir, &stale.provider.id)?;
    }

    applied.sort_by_key(|(index, _)| *index);
    Ok(applied
        .into_iter()
        .map(|(index, id)| {
            let entry = &config.providers[index];
            if let Some(credential) = &entry.credential {
                secrets.bind(bots::provider_token_key(&id), credential.name());
            }
            AppliedProvider {
                id,
                reference: ProviderRef::new(
                    entry.kind.as_registry_str(),
                    &entry.base_url.normalized,
                )
                .reference(),
            }
        })
        .collect())
}

/// The row's display name: its credential reference, which is also how a
/// changed `base_url` finds the row it moves.
fn row_name(entry: &keeper_core::agents::agentd::ProviderEntry) -> String {
    match &entry.credential {
        Some(credential) => format!("secret:{}", credential.name()),
        None => entry.base_url.host.clone(),
    }
}

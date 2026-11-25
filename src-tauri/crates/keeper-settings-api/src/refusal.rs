//! Refusal codes returned by the settings and shared-configuration rules.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Wire-level refusal code produced when a settings or configuration operation
/// violates the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Refusal {
    /// An agent instance, a tool, or the service client attempted to write a
    /// `settings_*` row.
    #[serde(rename = "settings.writer_refused")]
    WriterRefused,
    /// Write targets a non-personal domain or a different owner actor.
    #[serde(rename = "settings.not_owner")]
    NotOwner,
    /// Device-keyed record or device record written from a client other than
    /// the bound one, or with no matching device record.
    #[serde(rename = "settings.not_this_device")]
    NotThisDevice,
    /// Raw write attempted to change `settings_devices.client_id` or a device
    /// record's owner; only an adoption moves a binding.
    #[serde(rename = "settings.binding_changed")]
    BindingChanged,
    /// Key does not match any registry entry or known namespace.
    #[serde(rename = "settings.unknown_key")]
    UnknownKey,
    /// Key has travel class `never` and may not leave the local machine.
    #[serde(rename = "settings.never_leaves")]
    NeverLeaves,
    /// Record layer contradicts the key's travel class or `settable` rule.
    #[serde(rename = "settings.wrong_class")]
    WrongClass,
    /// Value looks like a credential or violates a credential-source family's
    /// allowed choices.
    #[serde(rename = "settings.credential_value")]
    CredentialValue,
    /// Inline value exceeds `settingValueMaxBytes` and must travel as a blob.
    #[serde(rename = "settings.value_too_large")]
    ValueTooLarge,
    /// Uploaded blob exceeds `attachmentMaxBytes`.
    #[serde(rename = "settings.blob_too_large")]
    BlobTooLarge,
    /// Record id, payload columns, device id, or fingerprint is malformed.
    #[serde(rename = "settings.malformed")]
    Malformed,
    /// An agent instance or a tool requested the `settings_owner` scope.
    #[serde(rename = "settings.scope_refused")]
    ScopeRefused,
    /// Device adoption claim did not satisfy the binding rules.
    #[serde(rename = "settings.adoption_refused")]
    AdoptionRefused,
    /// Caller is not permitted to write `config_values` in this domain.
    #[serde(rename = "config.writer_refused")]
    ConfigWriterRefused,
    /// Configuration key or value names a domain.
    #[serde(rename = "config.names_domain")]
    ConfigNamesDomain,
    /// Configuration key violates key grammar or value exceeds the inline
    /// byte limit.
    #[serde(rename = "config.malformed")]
    ConfigMalformed,
    /// A row of a published model version already exists; a version's rows
    /// are never rewritten.
    #[serde(rename = "config.model_sealed")]
    ModelSealed,
    /// A model version's manifest or `models.active` was written before the
    /// version's file rows were complete and hashed to its id.
    #[serde(rename = "config.model_incomplete")]
    ModelIncomplete,
    /// A model file exceeds `modelFileMaxBytes`.
    #[serde(rename = "config.model_too_large")]
    ModelTooLarge,
}

impl Refusal {
    /// Every refusal variant in contract order.
    pub const ALL: [Self; 19] = [
        Self::WriterRefused,
        Self::NotOwner,
        Self::NotThisDevice,
        Self::BindingChanged,
        Self::UnknownKey,
        Self::NeverLeaves,
        Self::WrongClass,
        Self::CredentialValue,
        Self::ValueTooLarge,
        Self::BlobTooLarge,
        Self::Malformed,
        Self::ScopeRefused,
        Self::AdoptionRefused,
        Self::ConfigWriterRefused,
        Self::ConfigNamesDomain,
        Self::ConfigMalformed,
        Self::ModelSealed,
        Self::ModelIncomplete,
        Self::ModelTooLarge,
    ];

    /// Stable dot-delimited wire code for this refusal.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WriterRefused => "settings.writer_refused",
            Self::NotOwner => "settings.not_owner",
            Self::NotThisDevice => "settings.not_this_device",
            Self::BindingChanged => "settings.binding_changed",
            Self::UnknownKey => "settings.unknown_key",
            Self::NeverLeaves => "settings.never_leaves",
            Self::WrongClass => "settings.wrong_class",
            Self::CredentialValue => "settings.credential_value",
            Self::ValueTooLarge => "settings.value_too_large",
            Self::BlobTooLarge => "settings.blob_too_large",
            Self::Malformed => "settings.malformed",
            Self::ScopeRefused => "settings.scope_refused",
            Self::AdoptionRefused => "settings.adoption_refused",
            Self::ConfigWriterRefused => "config.writer_refused",
            Self::ConfigNamesDomain => "config.names_domain",
            Self::ConfigMalformed => "config.malformed",
            Self::ModelSealed => "config.model_sealed",
            Self::ModelIncomplete => "config.model_incomplete",
            Self::ModelTooLarge => "config.model_too_large",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Refusal {}

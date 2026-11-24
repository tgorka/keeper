//! Record identifiers, key namespaces, and travel-class classification for
//! `settings_values` rows.

use serde::{Deserialize, Serialize};

use crate::refusal::Refusal;
use crate::travel::{Settable, Travel, TravelClass};

/// Kind of resource advertised by an `offer:<kind>:<reference>` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfferKind {
    /// Shared drive offer (`offer:drive:<rest>`).
    Drive,
    /// Model or service provider offer (`offer:provider:<rest>`).
    Provider,
    /// Matrix account or bridge offer (`offer:matrix:<rest>`).
    Matrix,
}

/// Parsed namespace of a `settings_values.key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKey {
    /// Bare registry preference key.
    Preference(String),
    /// Layer pin (`pin:<registry key>`).
    Pin(String),
    /// Person-level offer (`offer:<kind>:<reference>`).
    Offer {
        /// Offer namespace kind.
        kind: OfferKind,
        /// Non-empty reference after `offer:<kind>:`.
        reference: String,
    },
    /// Device-scoped drive role map (`roles:<drive reference>`).
    DriveRoles(String),
}

/// Target layer of a `settings_values` record: person-wide (`*`) or a specific
/// device id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    /// Person-level record (`*`).
    Person,
    /// Single-device record addressed by its device id.
    Device(String),
}

impl Device {
    /// Wire string representation: `"*"` for [`Device::Person`], or the device
    /// id for [`Device::Device`].
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Person => "*",
            Self::Device(id) => id.as_str(),
        }
    }
}

/// Parses a `settings_values.key` against the namespace grammar without
/// consulting the travel registry. A key may hold `/` (a drive reference is a
/// URL); only [`parse_id`] splits, at the id's first `/`.
pub fn parse_key(key: &str) -> Result<RecordKey, Refusal> {
    if key.is_empty() {
        return Err(Refusal::Malformed);
    }

    if let Some(rest) = key.strip_prefix("pin:") {
        if rest.is_empty() {
            return Err(Refusal::Malformed);
        }
        return Ok(RecordKey::Pin(rest.to_owned()));
    }

    if let Some(rest) = key.strip_prefix("offer:") {
        let Some((kind_str, reference)) = rest.split_once(':') else {
            return Err(Refusal::Malformed);
        };
        if reference.is_empty() {
            return Err(Refusal::Malformed);
        }
        let kind = match kind_str {
            "drive" => OfferKind::Drive,
            "provider" => OfferKind::Provider,
            "matrix" => OfferKind::Matrix,
            _ => return Err(Refusal::Malformed),
        };
        return Ok(RecordKey::Offer {
            kind,
            reference: reference.to_owned(),
        });
    }

    if let Some(reference) = key.strip_prefix("roles:") {
        let Some(rest) = reference.strip_prefix("drive:") else {
            return Err(Refusal::Malformed);
        };
        if rest.is_empty() {
            return Err(Refusal::Malformed);
        }
        return Ok(RecordKey::DriveRoles(reference.to_owned()));
    }

    Ok(RecordKey::Preference(key.to_owned()))
}

/// Formats the composite `settings_values.id` (`*/<key>` or `<device>/<key>`).
#[must_use]
pub fn record_id(device: &Device, key: &str) -> String {
    format!("{}/{key}", device.as_str())
}

/// Splits a `settings_values.id` at the first `/`, rejecting empty device or
/// key segments as [`Refusal::Malformed`].
pub fn parse_id(id: &str) -> Result<(Device, String), Refusal> {
    let Some((device_str, key)) = id.split_once('/') else {
        return Err(Refusal::Malformed);
    };
    if device_str.is_empty() || key.is_empty() {
        return Err(Refusal::Malformed);
    }
    let device = if device_str == "*" {
        Device::Person
    } else {
        Device::Device(device_str.to_owned())
    };
    Ok((device, key.to_owned()))
}

/// Parses `key` and validates it against `travel` and `device`.
pub fn classify(travel: &Travel, device: &Device, key: &str) -> Result<RecordKey, Refusal> {
    let parsed = parse_key(key)?;
    match &parsed {
        RecordKey::Preference(bare) => {
            let Some(entry) = travel.entry(bare) else {
                return Err(Refusal::UnknownKey);
            };
            match entry.class {
                TravelClass::Never => return Err(Refusal::NeverLeaves),
                TravelClass::EveryDevice => {
                    if matches!(device, Device::Device(_)) {
                        return Err(Refusal::WrongClass);
                    }
                }
                TravelClass::ThisDevice => {
                    if matches!(device, Device::Person) {
                        return Err(Refusal::WrongClass);
                    }
                }
            }
        }
        RecordKey::Pin(bare) => {
            let Some(entry) = travel.entry(bare) else {
                return Err(Refusal::UnknownKey);
            };
            let allowed = match device {
                Device::Person => matches!(entry.settable, Settable::AnyLayer),
                Device::Device(_) => {
                    matches!(entry.settable, Settable::AnyLayer | Settable::MachineFile)
                }
            };
            if !allowed {
                return Err(Refusal::WrongClass);
            }
        }
        RecordKey::Offer { .. } => {
            if matches!(device, Device::Device(_)) {
                return Err(Refusal::WrongClass);
            }
        }
        RecordKey::DriveRoles(_) => {
            if matches!(device, Device::Person) {
                return Err(Refusal::WrongClass);
            }
        }
    }
    Ok(parsed)
}

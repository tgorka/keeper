//! Commit-level write checks for `settings_values` and `settings_devices` rows.

use serde::{Deserialize, Serialize};

use crate::credential::check_value;
use crate::record::{classify, parse_id, Device, RecordKey};
use crate::refusal::Refusal;
use crate::travel::Travel;
use crate::Limits;

/// Kind of principal attempting a settings write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterKind {
    /// A human account holder.
    Person,
    /// An agent instance.
    Agent,
    /// A tool client.
    Tool,
    /// The internal service client.
    Service,
}

/// Identity and client binding of a settings writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Writer {
    /// Principal kind.
    pub kind: WriterKind,
    /// Actor identifier.
    pub actor: String,
    /// Sync client identifier.
    pub client: String,
}

/// Domain kind for a settings or configuration write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainKind {
    /// Shared general domain.
    General,
    /// Single-person personal domain.
    Personal,
    /// Shared group domain.
    Group,
}

/// Target domain metadata for a settings write or scope check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    /// Kind of domain.
    pub kind: DomainKind,
    /// Owner actor when the domain is personal.
    pub owner_actor: Option<String>,
}

/// Row mutation operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Insert or update a row.
    Upsert,
    /// Delete a row.
    Delete,
}

/// Candidate `settings_values` row being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueRow {
    /// Composite row identifier (`<device>/<key>`).
    pub id: String,
    /// Owning actor column on the row.
    pub owner_actor: String,
    /// Device segment (`"*"` for person-level rows, or a device id).
    pub device: String,
    /// Setting key.
    pub key: String,
    /// Inline UTF-8 value, if not stored as a blob.
    pub value: Option<String>,
    /// Whether the value is stored in a blob attachment.
    pub value_blob: bool,
}

/// Candidate `settings_devices` row being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRow {
    /// Device identifier.
    pub device_id: String,
    /// Owning actor column on the row.
    pub owner_actor: String,
    /// Bound sync client identifier.
    pub client_id: String,
    /// Optional 64-character lowercase hex machine fingerprint.
    pub machine: Option<String>,
}

/// Stored `settings_devices` binding referenced by a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bound {
    /// Owning actor recorded on the stored device row.
    pub owner_actor: String,
    /// Bound sync client identifier recorded on the stored device row.
    pub client_id: String,
}

/// Validate a `settings_values` row write and return its classified key.
pub fn check_value_write(
    travel: &Travel,
    limits: &Limits,
    writer: &Writer,
    domain: &Domain,
    op: Op,
    row: &ValueRow,
    device_record: Option<&Bound>,
) -> Result<RecordKey, Refusal> {
    if writer.kind != WriterKind::Person {
        return Err(Refusal::WriterRefused);
    }
    if domain.kind != DomainKind::Personal
        || domain.owner_actor.as_deref() != Some(writer.actor.as_str())
        || row.owner_actor != writer.actor
    {
        return Err(Refusal::NotOwner);
    }

    let (device, key) = parse_id(&row.id)?;
    let device_matches = match &device {
        Device::Person => row.device == "*",
        Device::Device(id) => row.device == *id,
    };
    if !device_matches || key != row.key {
        return Err(Refusal::Malformed);
    }

    let record_key = classify(travel, &device, &key)?;

    if matches!(device, Device::Device(_)) {
        match device_record {
            Some(bound)
                if bound.owner_actor == writer.actor && bound.client_id == writer.client => {}
            _ => return Err(Refusal::NotThisDevice),
        }
    }

    if op == Op::Upsert {
        match (&row.value, row.value_blob) {
            (Some(value), false) => {
                if value.len() as u64 > limits.setting_value_max_bytes {
                    return Err(Refusal::ValueTooLarge);
                }
                check_value(&key, value)?;
            }
            (None, true) => {}
            (Some(_), true) | (None, false) => return Err(Refusal::Malformed),
        }
    }

    Ok(record_key)
}

/// Validate a `settings_devices` row write.
pub fn check_device_write(
    writer: &Writer,
    domain: &Domain,
    op: Op,
    row: &DeviceRow,
    stored: Option<&Bound>,
) -> Result<(), Refusal> {
    if writer.kind != WriterKind::Person {
        return Err(Refusal::WriterRefused);
    }
    if domain.kind != DomainKind::Personal
        || domain.owner_actor.as_deref() != Some(writer.actor.as_str())
        || row.owner_actor != writer.actor
    {
        return Err(Refusal::NotOwner);
    }

    if op == Op::Upsert
        && row
            .machine
            .as_deref()
            .is_some_and(|machine| !is_machine_fingerprint(machine))
    {
        return Err(Refusal::Malformed);
    }

    match stored {
        None => match op {
            Op::Delete => Err(Refusal::NotThisDevice),
            Op::Upsert => {
                if row.client_id != writer.client {
                    Err(Refusal::NotThisDevice)
                } else {
                    Ok(())
                }
            }
        },
        Some(bound) => {
            if bound.owner_actor != writer.actor || bound.client_id != writer.client {
                return Err(Refusal::NotThisDevice);
            }
            if op == Op::Upsert && row.client_id != bound.client_id {
                return Err(Refusal::BindingChanged);
            }
            Ok(())
        }
    }
}

fn is_machine_fingerprint(machine: &str) -> bool {
    machine.len() == 64
        && machine
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::travel::{Settable, TravelClass, TravelEntry};

    fn sample_travel() -> Travel {
        Travel {
            keys: vec![
                TravelEntry {
                    key: "debug.mode".to_owned(),
                    family: false,
                    class: TravelClass::EveryDevice,
                    settable: Settable::AnyLayer,
                },
                TravelEntry {
                    key: "window.bounds".to_owned(),
                    family: false,
                    class: TravelClass::ThisDevice,
                    settable: Settable::MachineFile,
                },
            ],
        }
    }

    fn sample_limits() -> Limits {
        Limits {
            setting_value_max_bytes: 16,
            attachment_max_bytes: 1024,
        }
    }

    fn person_writer() -> Writer {
        Writer {
            kind: WriterKind::Person,
            actor: "@ana:example.org".to_owned(),
            client: "cli_ana_mac".to_owned(),
        }
    }

    fn personal_domain() -> Domain {
        Domain {
            kind: DomainKind::Personal,
            owner_actor: Some("@ana:example.org".to_owned()),
        }
    }

    #[test]
    fn agent_writer_refused_before_owner_check() {
        let writer = Writer {
            kind: WriterKind::Agent,
            actor: "@agent-nixie:example.org".to_owned(),
            client: "cli_agent".to_owned(),
        };
        let domain = Domain {
            kind: DomainKind::General,
            owner_actor: None,
        };
        let row = ValueRow {
            id: "*/debug.mode".to_owned(),
            owner_actor: "@bo:example.org".to_owned(),
            device: "*".to_owned(),
            key: "debug.mode".to_owned(),
            value: None,
            value_blob: false,
        };
        let err = check_value_write(
            &sample_travel(),
            &sample_limits(),
            &writer,
            &domain,
            Op::Upsert,
            &row,
            None,
        );
        assert_eq!(err, Err(Refusal::WriterRefused));
    }

    #[test]
    fn delete_skips_value_checks() {
        let row = ValueRow {
            id: "*/debug.mode".to_owned(),
            owner_actor: "@ana:example.org".to_owned(),
            device: "*".to_owned(),
            key: "debug.mode".to_owned(),
            value: None,
            value_blob: false,
        };
        let result = check_value_write(
            &sample_travel(),
            &sample_limits(),
            &person_writer(),
            &personal_domain(),
            Op::Delete,
            &row,
            None,
        );
        assert_eq!(result, Ok(RecordKey::Preference("debug.mode".to_owned())));
    }

    #[test]
    fn device_write_checks_fingerprint_and_binding_change() {
        let writer = person_writer();
        let domain = personal_domain();
        let bad_fp = DeviceRow {
            device_id: "01JD3Q8Z5X0000000000000001".to_owned(),
            owner_actor: "@ana:example.org".to_owned(),
            client_id: "cli_ana_mac".to_owned(),
            machine: Some("A".repeat(64)),
        };
        assert_eq!(
            check_device_write(&writer, &domain, Op::Upsert, &bad_fp, None),
            Err(Refusal::Malformed)
        );

        let stored = Bound {
            owner_actor: "@ana:example.org".to_owned(),
            client_id: "cli_ana_mac".to_owned(),
        };
        let rebound = DeviceRow {
            device_id: "01JD3Q8Z5X0000000000000001".to_owned(),
            owner_actor: "@ana:example.org".to_owned(),
            client_id: "cli_ana_mac2".to_owned(),
            machine: Some("a".repeat(64)),
        };
        assert_eq!(
            check_device_write(&writer, &domain, Op::Upsert, &rebound, Some(&stored)),
            Err(Refusal::BindingChanged)
        );
    }
}

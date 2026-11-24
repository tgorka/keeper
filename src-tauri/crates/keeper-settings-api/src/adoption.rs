//! Rules for adopting an existing device record onto a new client binding.

use serde::{Deserialize, Serialize};

use crate::refusal::Refusal;
use crate::write::WriterKind;

/// Minimum untouched days required to adopt a legacy device record with no machine fingerprint.
pub const LEGACY_ADOPT_DAYS: u64 = 30;

/// Stored device record evaluated for adoption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    /// Owning actor of the device record.
    pub owner_actor: String,
    /// Recorded device class.
    pub device_class: String,
    /// Recorded platform.
    pub platform: String,
    /// Optional machine fingerprint recorded on the device.
    pub machine: Option<String>,
    /// Days elapsed since the device record was last touched.
    pub untouched_days: u64,
}

/// Adoption claim presented by a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// Actor making the claim.
    pub actor: String,
    /// Principal kind of the claimant.
    pub writer_kind: WriterKind,
    /// Claimed device class.
    pub device_class: String,
    /// Claimed platform.
    pub platform: String,
    /// Optional machine fingerprint presented by the claimant.
    pub machine: Option<String>,
}

/// Decide whether `claim` may adopt `record`.
pub fn check_adoption(record: &DeviceRecord, claim: &Claim) -> Result<(), Refusal> {
    if claim.writer_kind != WriterKind::Person {
        return Err(Refusal::WriterRefused);
    }
    if claim.actor != record.owner_actor {
        return Err(Refusal::NotOwner);
    }
    if claim.device_class != record.device_class || claim.platform != record.platform {
        return Err(Refusal::AdoptionRefused);
    }
    match &record.machine {
        Some(machine) => {
            if claim.machine.as_deref() == Some(machine.as_str()) {
                Ok(())
            } else {
                Err(Refusal::AdoptionRefused)
            }
        }
        None => {
            if record.untouched_days >= LEGACY_ADOPT_DAYS {
                Ok(())
            } else {
                Err(Refusal::AdoptionRefused)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_record() -> DeviceRecord {
        DeviceRecord {
            owner_actor: "@ana:example.org".to_owned(),
            device_class: "desktop".to_owned(),
            platform: "macos".to_owned(),
            machine: None,
            untouched_days: 29,
        }
    }

    fn base_claim() -> Claim {
        Claim {
            actor: "@ana:example.org".to_owned(),
            writer_kind: WriterKind::Person,
            device_class: "desktop".to_owned(),
            platform: "macos".to_owned(),
            machine: None,
        }
    }

    #[test]
    fn legacy_machine_none_requires_30_untouched_days() {
        let mut record = base_record();
        let claim = base_claim();
        assert_eq!(
            check_adoption(&record, &claim),
            Err(Refusal::AdoptionRefused)
        );

        record.untouched_days = 30;
        assert_eq!(check_adoption(&record, &claim), Ok(()));
    }

    #[test]
    fn writer_refused_before_owner_check() {
        let record = base_record();
        let claim = Claim {
            actor: "@bo:example.org".to_owned(),
            writer_kind: WriterKind::Agent,
            ..base_claim()
        };
        assert_eq!(check_adoption(&record, &claim), Err(Refusal::WriterRefused));
    }

    #[test]
    fn machine_fingerprint_must_match_when_present() {
        let mut record = base_record();
        record.machine = Some("a".repeat(64));
        record.untouched_days = 100;

        let mut claim = base_claim();
        assert_eq!(
            check_adoption(&record, &claim),
            Err(Refusal::AdoptionRefused)
        );

        claim.machine = Some("a".repeat(64));
        assert_eq!(check_adoption(&record, &claim), Ok(()));
    }
}

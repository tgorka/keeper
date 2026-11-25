//! Access and upload size checks for `settings_values.value_blob`.

use serde::{Deserialize, Serialize};

use crate::refusal::Refusal;
use crate::scope::CallerKind;
use crate::Limits;

/// Result of a `settings_values.value_blob` read check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobAnswer {
    /// The reader may access the blob.
    Granted,
    /// The blob is hidden from the reader.
    Absent,
}

/// Principal attempting to read a settings value blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobReader {
    /// Kind of caller.
    pub kind: CallerKind,
    /// Actor identifier.
    pub actor: String,
}

/// Decide whether `reader` may read a blob owned by `row_owner_actor`.
pub fn read_value_blob(row_owner_actor: &str, reader: &BlobReader) -> BlobAnswer {
    if reader.kind == CallerKind::Person && reader.actor == row_owner_actor {
        BlobAnswer::Granted
    } else {
        BlobAnswer::Absent
    }
}

/// Check that a blob upload size does not exceed the attachment limit.
pub fn check_upload(size: u64, limits: &Limits) -> Result<(), Refusal> {
    if size > limits.attachment_max_bytes {
        return Err(Refusal::BlobTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_value_blob_grants_only_owning_person() {
        let owner = BlobReader {
            kind: CallerKind::Person,
            actor: "@ana:example.org".to_owned(),
        };
        assert_eq!(
            read_value_blob("@ana:example.org", &owner),
            BlobAnswer::Granted
        );

        let other_person = BlobReader {
            kind: CallerKind::Person,
            actor: "@bo:example.org".to_owned(),
        };
        assert_eq!(
            read_value_blob("@ana:example.org", &other_person),
            BlobAnswer::Absent
        );

        let service = BlobReader {
            kind: CallerKind::Service,
            actor: "@ana:example.org".to_owned(),
        };
        assert_eq!(
            read_value_blob("@ana:example.org", &service),
            BlobAnswer::Absent
        );
    }

    #[test]
    fn check_upload_enforces_attachment_limit() {
        let limits = Limits {
            setting_value_max_bytes: 16_384,
            attachment_max_bytes: 1024,
            model_file_max_bytes: 4096,
        };
        assert_eq!(check_upload(1024, &limits), Ok(()));
        assert_eq!(check_upload(1025, &limits), Err(Refusal::BlobTooLarge));
    }
}

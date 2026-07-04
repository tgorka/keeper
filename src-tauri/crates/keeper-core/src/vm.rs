//! IPC view models (AD-7, AD-8).
//!
//! Every type that crosses the Tauri IPC boundary lives here, derives
//! `serde` + [`ts_rs::TS`], is `#[ts(export)]`, and renames fields to
//! camelCase. Timestamps are `i64` milliseconds since the Unix epoch (UTC) —
//! never strings. Bindings are emitted to `src/lib/ipc/gen/` by the ts-rs
//! export test step (`cargo nextest run`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Response of the `app_ping` liveness command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingVm {
    /// Backend liveness marker, e.g. `"pong"`.
    pub message: String,
    /// Server-side timestamp: milliseconds since the Unix epoch (UTC).
    ///
    /// Emitted to TypeScript as `number`, not `bigint`: Tauri IPC delivers the
    /// `i64` as a JS number via `JSON.parse`, and ms-epoch values stay well
    /// within `Number.MAX_SAFE_INTEGER`. This keeps the binding matching the
    /// wire reality — the timestamp convention every later VM copies.
    #[ts(type = "number")]
    pub ts: i64,
}

/// Stable, string-serialized error taxonomy for the IPC envelope.
///
/// Variants serialize to their camelCase names (e.g. `"unsupported"`) and are
/// part of the frontend contract — rename with care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IpcErrorCode {
    /// The requested capability is not supported on this platform/build.
    Unsupported,
    /// An unexpected internal error occurred in the backend.
    Internal,
}

/// The single error envelope every fallible command rejects with (AD-8, AD-21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IpcError {
    /// Stable machine-readable error code.
    pub code: IpcErrorCode,
    /// Human-readable message (never contains secrets or plaintext).
    pub message: String,
    /// Opaque keeper account id this error pertains to, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Whether retrying the same operation may succeed.
    pub retriable: bool,
}

/// A single demo item carried in snapshot/diff batches. Placeholder payload
/// that exercises the snapshot-then-diff channel pattern end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DemoItem {
    /// Stable item id.
    pub id: String,
    /// Display label.
    pub label: String,
}

/// A batch delivered over a demo subscription's `Channel` (AD-8).
///
/// The stream always opens with a [`DemoBatch::Snapshot`] (full reset) before
/// any [`DemoBatch::Diff`]. Serialized as an internally tagged enum so the
/// frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum DemoBatch {
    /// Full state reset — the complete current set of items.
    Snapshot {
        /// Every item currently present.
        items: Vec<DemoItem>,
    },
    /// Incremental change relative to the last delivered state.
    Diff {
        /// Items added or updated in this batch.
        added: Vec<DemoItem>,
        /// Ids removed in this batch.
        removed: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_error_code_serializes_camel_case() {
        let json = serde_json::to_string(&IpcErrorCode::Unsupported).expect("serialize code");
        assert_eq!(json, "\"unsupported\"");
        let back: IpcErrorCode = serde_json::from_str(&json).expect("deserialize code");
        assert_eq!(back, IpcErrorCode::Unsupported);
    }

    #[test]
    fn ipc_error_round_trips_camel_case_and_omits_none_account() {
        let err = IpcError {
            code: IpcErrorCode::Internal,
            message: "boom".to_owned(),
            account_id: None,
            retriable: true,
        };
        let json = serde_json::to_string(&err).expect("serialize error");
        // camelCase field name and absent account_id.
        assert!(json.contains("\"retriable\":true"), "json was: {json}");
        assert!(
            !json.contains("accountId"),
            "account_id should be omitted: {json}"
        );
        let back: IpcError = serde_json::from_str(&json).expect("deserialize error");
        assert_eq!(back, err);
    }

    #[test]
    fn ipc_error_serializes_account_id_camel_case_when_present() {
        let err = IpcError {
            code: IpcErrorCode::Internal,
            message: "boom".to_owned(),
            account_id: Some("01ABC".to_owned()),
            retriable: false,
        };
        let json = serde_json::to_string(&err).expect("serialize error");
        assert!(json.contains("\"accountId\":\"01ABC\""), "json was: {json}");
    }

    #[test]
    fn demo_batch_tags_variants() {
        let snap = DemoBatch::Snapshot {
            items: vec![DemoItem {
                id: "1".into(),
                label: "one".into(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("serialize snapshot");
        assert!(json.contains("\"kind\":\"snapshot\""), "json was: {json}");
    }
}

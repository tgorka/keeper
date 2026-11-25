//! Shared command delivery, receipt polling, refusal mapping and remote
//! query helpers for `files.*` operations.

use std::time::{Duration, Instant};

use keeper_files_api::refusal::Code;
use keeper_files_api::rows::Outcome;
use keeper_ports::ReplicaError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{Files, FilesError};

/// One row of `files_receipts`, or a synthetic receipt built from an inline
/// contract refusal on the command transport.
pub(crate) struct ReceiptRow {
    pub outcome: Outcome,
    pub reason: Option<String>,
    pub event_id: Option<String>,
    pub seq: Option<String>,
    pub detail: Option<Value>,
}

/// How a `files.*` command delivery ended.
pub(crate) enum Delivered {
    Unreached,
    Unseen,
    Receipt(ReceiptRow),
}

/// Whether `error` means the server or the store was not reached.
pub(crate) fn unreachable(error: &ReplicaError) -> bool {
    matches!(
        error,
        ReplicaError::Unheard { .. }
            | ReplicaError::NotSyncing
            | ReplicaError::Closed
            | ReplicaError::Remote {
                retryable: true,
                ..
            }
    )
}

/// A refusal code of the files contract, read from its wire word.
pub(crate) fn code_of(word: &str) -> Option<Code> {
    Code::deserialize(Value::String(word.to_owned())).ok()
}

/// Turn a receipt's refusal reason and detail into a [`FilesError`].
pub(crate) fn refusal(reason: Option<&str>, detail: Option<Value>) -> FilesError {
    let reason = reason.unwrap_or("rejected");
    match code_of(reason) {
        Some(code) => FilesError::Refused { code, detail },
        None => FilesError::Replica(ReplicaError::Rejected {
            code: reason.to_owned(),
            message: detail.map(|d| d.to_string()).unwrap_or_default(),
        }),
    }
}

impl Files {
    /// Send a `files.*` command and poll `files_receipts` up to
    /// `receipt_wait` for this device's receipt.
    pub(crate) fn deliver(
        &self,
        descriptor: &str,
        request_id: &str,
        payload: Value,
    ) -> Result<Delivered, FilesError> {
        let params = json!({
            "version": 1,
            "request_id": request_id,
            "context_id": null,
            "payload": payload,
        });
        match self
            .source
            .replica
            .remote_command(descriptor, request_id, params)
        {
            Ok(_) => {}
            Err(error) if unreachable(&error) => return Ok(Delivered::Unreached),
            Err(
                ReplicaError::Rejected { code, message }
                | ReplicaError::Remote {
                    code,
                    message,
                    retryable: false,
                },
            ) if code_of(&code).is_some() => {
                let outcome = if code == "conflict" {
                    Outcome::Conflict
                } else {
                    Outcome::Rejected
                };
                return Ok(Delivered::Receipt(ReceiptRow {
                    outcome,
                    reason: Some(code),
                    event_id: None,
                    seq: None,
                    detail: Some(Value::String(message)),
                }));
            }
            Err(error) => return Err(error.into()),
        }
        let started = Instant::now();
        loop {
            if let Some(receipt) = self.receipt_row(descriptor, request_id)? {
                return Ok(Delivered::Receipt(receipt));
            }
            if started.elapsed() >= self.receipt_wait {
                return Ok(Delivered::Unseen);
            }
            self.source.replica.sync_now();
            std::thread::sleep(Duration::from_millis(50).min(self.receipt_wait));
        }
    }

    /// Run a `files.*` remote query and return its first row as a JSON object.
    pub(crate) fn read(&self, operation: &str, params: Value) -> Result<Value, FilesError> {
        let rows = match self.source.replica.remote_query(operation, params) {
            Ok(rows) => rows,
            Err(error) if unreachable(&error) => {
                return Err(FilesError::Offline(format!("{operation}: {error}")));
            }
            Err(error) => {
                if let ReplicaError::Rejected { code, .. }
                | ReplicaError::Remote {
                    code,
                    retryable: false,
                    ..
                } = &error
                {
                    if let Some(code) = code_of(code) {
                        return Err(FilesError::Refused { code, detail: None });
                    }
                }
                return Err(error.into());
            }
        };
        let first = rows
            .into_iter()
            .next()
            .ok_or_else(|| FilesError::Row(format!("{operation} returned no rows")))?;
        Ok(Value::Object(first))
    }

    fn receipt_row(
        &self,
        descriptor: &str,
        request_id: &str,
    ) -> Result<Option<ReceiptRow>, FilesError> {
        let id = format!("{descriptor}/{request_id}/{}", self.client_scope());
        let rows = self
            .source
            .replica
            .query("SELECT * FROM files_receipts WHERE id = ?", &[json!(id)])?;
        rows.first().map(receipt_of).transpose()
    }
}

fn receipt_of(row: &Map<String, Value>) -> Result<ReceiptRow, FilesError> {
    let outcome = row
        .get("outcome")
        .cloned()
        .ok_or_else(|| FilesError::Row("receipt missing outcome".to_owned()))
        .and_then(|value| {
            Outcome::deserialize(value)
                .map_err(|error| FilesError::Row(format!("receipt outcome: {error}")))
        })?;
    let reason = row.get("reason").and_then(Value::as_str).map(str::to_owned);
    let event_id = row
        .get("event_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let seq = match row.get("seq") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    };
    let detail = match row.get("detail") {
        Some(Value::String(text)) => serde_json::from_str(text).ok(),
        Some(Value::Null) | None => None,
        Some(value) => Some(value.clone()),
    };
    Ok(ReceiptRow {
        outcome,
        reason,
        event_id,
        seq,
        detail,
    })
}

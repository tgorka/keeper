//! The outcome journal's writer for tests: what a round records when the
//! server refuses a commit, written straight into the replica file, so a
//! test can arrange a refusal without a server. Built for keeper-replica's
//! own tests and, behind the `outcome-journal` feature, for the
//! `[dev-dependencies]` that enable it (keeper-core's); never shipped.

use std::time::Duration;

use serde_json::Value;

/// Record, as a round would, the server refusing commit `commit` with
/// `code`; it wrote `ops` (`table`, `op`, values).
pub fn journal_refused(
    path: &std::path::Path,
    commit: &str,
    code: &str,
    ops: &[(&str, &str, Value)],
) {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let operations: Vec<CommitOperation> = ops
        .iter()
        .map(|(table, op, values)| CommitOperation {
            table: (*table).to_owned(),
            row_id: values["id"].as_str().unwrap_or_default().to_owned(),
            op: (*op).to_owned(),
            base_version: None,
            values: values.as_object().cloned(),
        })
        .collect();
    let results = vec![CommitOperationOutcome::Error {
        rejection: RejectionRecord {
            client_commit_id: commit.to_owned(),
            op_index: 0,
            code: code.to_owned(),
            message: code.to_owned(),
            retryable: false,
            details: None,
            operation: None,
        },
    }];
    let conn = rusqlite::Connection::open(path).expect("opens");
    conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
    conn.execute(
        "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
         results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
        rusqlite::params![
            commit,
            serde_json::to_string(&results).expect("results"),
            serde_json::to_string(&operations).expect("operations")
        ],
    )
    .expect("records");
}

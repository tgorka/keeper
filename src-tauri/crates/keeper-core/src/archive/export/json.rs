//! Pure lossless JSON serialization of scoped archive rows (Story 5.5, FR-35).
//!
//! The JSON export is the **provability** artifact: it contains *every* archived
//! row in scope — all edit-chain versions and redacted-but-retained rows — so the
//! emitted array length equals [`crate::archive::db::scoped_event_count`]. Each
//! element carries the row's full stored fields verbatim (`content_json` and
//! `media_json` are re-embedded as parsed JSON so the output is one well-formed
//! document, never a string-escaped blob). This module is pure: it takes rows and
//! returns a `String`, touching no filesystem, session, or network.

use serde_json::{Map, Value};

use crate::archive::db::StoredEvent;
use crate::error::ArchiveError;

/// Serialize the scoped rows to a pretty-printed, lossless JSON array (Story 5.5).
///
/// One array element per [`StoredEvent`], in the order given (the caller passes
/// them chronologically). `content_json`/`media_json` are re-parsed and embedded
/// as nested JSON when they parse, else kept as their raw string (a malformed blob
/// is never dropped — losslessness wins over prettiness). The array length equals
/// the row count, which is the provability guarantee. Fails only if the final
/// document cannot be serialized (an internal invariant), surfaced as
/// [`ArchiveError::Serialization`].
pub fn render_json(events: &[StoredEvent]) -> Result<String, ArchiveError> {
    let items: Vec<Value> = events.iter().map(row_to_value).collect();
    serde_json::to_string_pretty(&Value::Array(items))
        .map_err(|e| ArchiveError::Serialization(format!("could not serialize export JSON: {e}")))
}

/// Map one stored row to a JSON object with every column preserved. `content_json`
/// and `media_json` are embedded as parsed JSON (or the raw string when unparseable
/// — never discarded).
fn row_to_value(ev: &StoredEvent) -> Value {
    let mut obj = Map::new();
    obj.insert("accountId".to_owned(), Value::String(ev.account_id.clone()));
    obj.insert("eventId".to_owned(), Value::String(ev.event_id.clone()));
    obj.insert("roomId".to_owned(), Value::String(ev.room_id.clone()));
    obj.insert("sender".to_owned(), Value::String(ev.sender.clone()));
    obj.insert("originTs".to_owned(), Value::from(ev.origin_ts));
    obj.insert("eventType".to_owned(), Value::String(ev.event_type.clone()));
    obj.insert("content".to_owned(), embed_json(&ev.content_json));
    obj.insert(
        "media".to_owned(),
        ev.media_json
            .as_deref()
            .map(embed_json)
            .unwrap_or(Value::Null),
    );
    obj.insert("insertedTs".to_owned(), Value::from(ev.inserted_ts));
    obj.insert(
        "relatesToEventId".to_owned(),
        ev.relates_to_event_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    obj.insert(
        "relType".to_owned(),
        ev.rel_type
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    obj.insert(
        "redactedTs".to_owned(),
        ev.redacted_ts.map(Value::from).unwrap_or(Value::Null),
    );
    Value::Object(obj)
}

/// Parse a stored JSON string into a [`Value`], or wrap the raw string when it does
/// not parse (never drops data — losslessness over prettiness).
fn embed_json(raw: &str) -> Value {
    serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(event_id: &str, origin_ts: i64) -> StoredEvent {
        StoredEvent {
            account_id: "acctA".to_owned(),
            event_id: event_id.to_owned(),
            room_id: "!r:e.org".to_owned(),
            sender: "@u:e.org".to_owned(),
            origin_ts,
            event_type: "m.room.message".to_owned(),
            content_json: r#"{"msgtype":"m.text","body":"hi"}"#.to_owned(),
            media_json: None,
            inserted_ts: origin_ts + 1,
            relates_to_event_id: None,
            rel_type: None,
            redacted_ts: None,
        }
    }

    #[test]
    fn array_length_equals_row_count_and_parses() {
        let rows = vec![row("$e1", 1), row("$e2", 2), row("$e3", 3)];
        let json = render_json(&rows).expect("render");
        let parsed: Value = serde_json::from_str(&json).expect("parseable");
        let arr = parsed.as_array().expect("array");
        assert_eq!(arr.len(), 3, "lossless: one element per row");
        // Content is embedded as nested JSON, not a string-escaped blob.
        assert_eq!(arr[0]["content"]["body"], Value::String("hi".to_owned()));
        assert_eq!(arr[0]["eventId"], Value::String("$e1".to_owned()));
    }

    #[test]
    fn empty_rows_render_empty_array() {
        let json = render_json(&[]).expect("render");
        assert_eq!(json.trim(), "[]");
    }

    #[test]
    fn malformed_content_is_kept_as_string_not_dropped() {
        let mut r = row("$e1", 1);
        r.content_json = "not json".to_owned();
        let json = render_json(&[r]).expect("render");
        let parsed: Value = serde_json::from_str(&json).expect("parseable");
        assert_eq!(
            parsed[0]["content"],
            Value::String("not json".to_owned()),
            "malformed content survives verbatim"
        );
    }
}

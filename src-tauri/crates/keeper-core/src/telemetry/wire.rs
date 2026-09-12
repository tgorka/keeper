//! Minimal OTLP protobuf messages: field numbers are the OpenTelemetry protocol,
//! not a vendor SDK. Only closed Record inputs can reach the encoder. Deliberately
//! no environment-resource detection, baggage, host names or arbitrary attrs.
use prost::Message;

use super::Record;

#[derive(Clone, PartialEq, Message)]
struct Value {
    #[prost(oneof = "value::Kind", tags = "1, 3")]
    kind: Option<value::Kind>,
}
mod value {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Kind {
        #[prost(string, tag = "1")]
        Text(String),
        #[prost(int64, tag = "3")]
        Integer(i64),
    }
}
#[derive(Clone, PartialEq, Message)]
struct Attribute {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(message, optional, tag = "2")]
    value: Option<Value>,
}
#[derive(Clone, PartialEq, Message)]
struct Resource {
    #[prost(message, repeated, tag = "1")]
    attributes: Vec<Attribute>,
}
#[derive(Clone, PartialEq, Message)]
struct Scope {
    #[prost(string, tag = "1")]
    name: String,
}
#[derive(Clone, PartialEq, Message)]
struct Status {
    #[prost(int32, tag = "3")]
    code: i32,
}
#[derive(Clone, PartialEq, Message)]
struct Span {
    #[prost(bytes = "vec", tag = "1")]
    trace_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    span_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    parent_span_id: Vec<u8>,
    #[prost(string, tag = "5")]
    name: String,
    #[prost(int32, tag = "6")]
    kind: i32,
    #[prost(fixed64, tag = "7")]
    start_time_unix_nano: u64,
    #[prost(fixed64, tag = "8")]
    end_time_unix_nano: u64,
    #[prost(message, repeated, tag = "9")]
    attributes: Vec<Attribute>,
    #[prost(message, optional, tag = "15")]
    status: Option<Status>,
    #[prost(fixed32, tag = "16")]
    flags: u32,
}
#[derive(Clone, PartialEq, Message)]
struct ScopeSpans {
    #[prost(message, optional, tag = "1")]
    scope: Option<Scope>,
    #[prost(message, repeated, tag = "2")]
    spans: Vec<Span>,
}
#[derive(Clone, PartialEq, Message)]
struct ResourceSpans {
    #[prost(message, optional, tag = "1")]
    resource: Option<Resource>,
    #[prost(message, repeated, tag = "2")]
    scope_spans: Vec<ScopeSpans>,
}
#[derive(Clone, PartialEq, Message)]
struct TraceRequest {
    #[prost(message, repeated, tag = "1")]
    resource_spans: Vec<ResourceSpans>,
}
#[derive(Clone, PartialEq, Message)]
struct LogRecord {
    #[prost(fixed64, tag = "1")]
    time_unix_nano: u64,
    #[prost(int32, tag = "2")]
    severity_number: i32,
    #[prost(string, tag = "3")]
    severity_text: String,
    #[prost(message, optional, tag = "5")]
    body: Option<Value>,
    #[prost(message, repeated, tag = "6")]
    attributes: Vec<Attribute>,
    #[prost(fixed32, tag = "8")]
    flags: u32,
    #[prost(bytes = "vec", tag = "9")]
    trace_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "10")]
    span_id: Vec<u8>,
    #[prost(fixed64, tag = "11")]
    observed_time_unix_nano: u64,
}
#[derive(Clone, PartialEq, Message)]
struct ScopeLogs {
    #[prost(message, optional, tag = "1")]
    scope: Option<Scope>,
    #[prost(message, repeated, tag = "2")]
    log_records: Vec<LogRecord>,
}
#[derive(Clone, PartialEq, Message)]
struct ResourceLogs {
    #[prost(message, optional, tag = "1")]
    resource: Option<Resource>,
    #[prost(message, repeated, tag = "2")]
    scope_logs: Vec<ScopeLogs>,
}
#[derive(Clone, PartialEq, Message)]
struct LogRequest {
    #[prost(message, repeated, tag = "1")]
    resource_logs: Vec<ResourceLogs>,
}

fn text(text: &str) -> Value {
    Value {
        kind: Some(value::Kind::Text(text.to_owned())),
    }
}
fn resource() -> Option<Resource> {
    Some(Resource {
        attributes: vec![Attribute {
            key: "service.name".to_owned(),
            value: Some(text("keeper")),
        }],
    })
}
fn scope() -> Option<Scope> {
    Some(Scope {
        name: "keeper.closed-events".to_owned(),
    })
}
fn attributes(record: &Record) -> Vec<Attribute> {
    vec![Attribute {
        key: "keeper.outcome".to_owned(),
        value: Some(Value {
            kind: Some(value::Kind::Integer(i64::from(record.outcome))),
        }),
    }]
}
fn id_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            fn nibble(byte: u8) -> u8 {
                match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => 0,
                }
            }
            (nibble(pair[0]) << 4) | nibble(pair[1])
        })
        .collect()
}

pub(super) fn traces(record: &Record) -> Vec<u8> {
    TraceRequest {
        resource_spans: vec![ResourceSpans {
            resource: resource(),
            scope_spans: vec![ScopeSpans {
                scope: scope(),
                spans: vec![Span {
                    trace_id: id_bytes(&record.trace_id),
                    span_id: id_bytes(&record.span_id),
                    parent_span_id: record
                        .parent_span_id
                        .as_deref()
                        .map(id_bytes)
                        .unwrap_or_default(),
                    name: record.operation.name().to_owned(),
                    kind: 1,
                    start_time_unix_nano: record.start,
                    end_time_unix_nano: record.end,
                    attributes: attributes(record),
                    status: Some(Status {
                        code: if record.outcome == 0 { 1 } else { 2 },
                    }),
                    flags: 1,
                }],
            }],
        }],
    }
    .encode_to_vec()
}

pub(super) fn logs(record: &Record) -> Vec<u8> {
    LogRequest {
        resource_logs: vec![ResourceLogs {
            resource: resource(),
            scope_logs: vec![ScopeLogs {
                scope: scope(),
                log_records: vec![LogRecord {
                    time_unix_nano: record.end,
                    observed_time_unix_nano: super::unix_nanos(),
                    severity_number: if record.outcome == 0 { 9 } else { 17 },
                    severity_text: if record.outcome == 0 { "INFO" } else { "ERROR" }.to_owned(),
                    body: Some(text(record.operation.name())),
                    attributes: attributes(record),
                    flags: 1,
                    trace_id: id_bytes(&record.trace_id),
                    span_id: id_bytes(&record.span_id),
                }],
            }],
        }],
    }
    .encode_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{TelemetryEventKind, TelemetryEventReq};

    #[test]
    fn exported_log_and_span_share_w3c_ids_and_only_closed_attributes() {
        let record = Record::frontend(TelemetryEventReq {
            kind: TelemetryEventKind::FrontendError,
            duration_ms: Some(25.0),
        });
        let logs = LogRequest::decode(logs(&record).as_slice()).expect("decode log protobuf");
        let traces =
            TraceRequest::decode(traces(&record).as_slice()).expect("decode trace protobuf");
        let log = &logs.resource_logs[0].scope_logs[0].log_records[0];
        let span = &traces.resource_spans[0].scope_spans[0].spans[0];
        assert_eq!(log.trace_id.len(), 16);
        assert_eq!(log.span_id.len(), 8);
        assert_eq!(log.trace_id, span.trace_id);
        assert_eq!(log.span_id, span.span_id);
        assert_eq!(
            span.end_time_unix_nano - span.start_time_unix_nano,
            25_000_000
        );
        assert_eq!(span.status.as_ref().expect("status").code, 2);
        assert_eq!(log.attributes.len(), 1);
        assert_eq!(log.attributes[0].key, "keeper.outcome");
        assert_eq!(span.attributes, log.attributes);
        assert_eq!(log.body, Some(text("keeper_frontend_error")));
    }

    #[test]
    fn connected_operation_retains_parent_span_without_baggage() {
        let mut parent = Record::frontend(TelemetryEventReq {
            kind: TelemetryEventKind::Interaction,
            duration_ms: None,
        });
        parent.operation = crate::telemetry::Operation::RemoteConfig;
        let child = Record {
            operation: crate::telemetry::Operation::FlagsRequest,
            span_id: crate::telemetry::random_id::<8>(),
            parent_span_id: Some(parent.span_id.clone()),
            trace_id: parent.trace_id.clone(),
            start: parent.start,
            end: parent.end,
            duration_ms: None,
            outcome: 0,
        };
        let request = TraceRequest::decode(traces(&child).as_slice()).expect("child wire");
        let span = &request.resource_spans[0].scope_spans[0].spans[0];
        assert_eq!(span.parent_span_id, id_bytes(&parent.span_id));
        assert_eq!(span.trace_id, id_bytes(&parent.trace_id));
        assert_ne!(span.span_id, span.parent_span_id);
    }

    #[test]
    fn status_error_uses_the_canonical_otlp_field_number() {
        // opentelemetry/proto/trace/v1/trace.proto: field 2 is message;
        // StatusCode code = 3. A self-roundtrip cannot detect a wrong tag.
        assert_eq!(Status { code: 2 }.encode_to_vec(), [0x18, 0x02]);
    }
}

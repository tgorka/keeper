//! Bridged-Chat Network label resolution for the delete confirmation (Story 3.8,
//! FR-15, UX-DR17).
//!
//! mautrix/Beeper bridges publish a standard MSC2346 `m.bridge` (and legacy
//! `uk.half-shot.bridge`) room state event whose `content.protocol.displayname`
//! (fallback `content.protocol.id`) names the Network the Chat is bridged to —
//! "Telegram", "WhatsApp", "Signal", … A native Matrix Room carries no such event.
//!
//! [`parse_bridge_network_name`] is a **pure** function over the state event's
//! `content` (unit-tested with MSC2346 fixtures); [`room_bridge_network`] is a
//! thin wrapper that reads the Room's state and applies it. This is scoped to the
//! delete confirmation only — it derives one on-demand label and touches no
//! `RoomVm`, inbox badge, or Network filter (those are Story 4.6).
//!
//! Secret containment (NFR-9): only a resolved, non-secret display string crosses
//! back to the caller — never the raw state event, an `mxc`, or any id. Returns
//! `None` when there is no bridge state or no protocol name, so the confirmation
//! falls back to honest native framing rather than a fabricated Network name.

use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk::ruma::events::StateEventType;
use matrix_sdk::Room;

/// The maximum length (in characters) of a bridge-provided Network label. The label
/// is untrusted server-controlled data shown verbatim in the delete confirmation, so
/// it is capped to keep the copy honest and bounded.
const MAX_NETWORK_NAME_CHARS: usize = 40;

/// The MSC2346 `m.bridge` state event type.
const BRIDGE_EVENT_TYPE: &str = "m.bridge";

/// The legacy `uk.half-shot.bridge` state event type (pre-MSC2346 mautrix).
const LEGACY_BRIDGE_EVENT_TYPE: &str = "uk.half-shot.bridge";

/// Parse the bridged Network's display name out of an MSC2346 bridge state
/// event's `content` (pure — the unit-tested core).
///
/// Reads `content.protocol.displayname`, falling back to `content.protocol.id`,
/// trims it, and returns it when non-empty. Any other shape — no `protocol`, a
/// non-string name, an empty/whitespace-only name, malformed content — yields
/// `None`, so an unrecognized bridge never produces a fabricated Network name.
pub fn parse_bridge_network_name(content: &serde_json::Value) -> Option<String> {
    let protocol = content.get("protocol")?;
    let name = protocol
        .get("displayname")
        .and_then(serde_json::Value::as_str)
        .or_else(|| protocol.get("id").and_then(serde_json::Value::as_str))?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        None
    } else {
        // The label is an untrusted, bridge/homeserver-controlled string rendered
        // verbatim in a security-relevant delete confirmation. Bound its length so a
        // malicious or misconfigured bridge cannot flood or spoof the dialog copy
        // (React already escapes it, so this is length, not injection). When the name
        // is actually clipped, append an ellipsis so the truncation is honest rather
        // than silently presenting a partial name as the whole Network.
        let mut capped: String = trimmed.chars().take(MAX_NETWORK_NAME_CHARS).collect();
        if trimmed.chars().count() > MAX_NETWORK_NAME_CHARS {
            capped.push('…');
        }
        Some(capped)
    }
}

/// Parse the bridged Network's stable `protocol.id` out of an MSC2346 bridge
/// state event's `content` (pure — the discovery join key, Story 6.2).
///
/// Where [`parse_bridge_network_name`] resolves a *display* label, this returns
/// the machine `content.protocol.id` — the stable network identifier keeper joins
/// to the catalog `networkId` (mautrix `protocol.id`s reconcile directly, e.g.
/// `whatsapp`, `telegram`). Trims it and returns it when non-empty; any other
/// shape (no `protocol`, non-string id, empty/whitespace id, malformed content)
/// yields `None`. The id is untrusted, server-controlled data used only as a map
/// key against the compiled-in catalog — a spoofed value can at most fail to
/// match, never inject.
pub fn parse_bridge_protocol_id(content: &serde_json::Value) -> Option<String> {
    let id = content
        .get("protocol")?
        .get("id")
        .and_then(serde_json::Value::as_str)?;
    let trimmed = id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Read only the opaque `content` JSON from a bridge state event (sync or
/// stripped variant), reused by the label and protocol-id readers.
fn bridge_state_content(raw: &RawAnySyncOrStrippedState) -> Option<serde_json::Value> {
    match raw {
        RawAnySyncOrStrippedState::Sync(ev) => ev.get_field::<serde_json::Value>("content"),
        RawAnySyncOrStrippedState::Stripped(ev) => ev.get_field::<serde_json::Value>("content"),
    }
    .ok()
    .flatten()
}

/// Resolve the bridged-Chat Network's stable `protocol.id` for `room` on demand
/// (the thin wrapper around [`parse_bridge_protocol_id`], Story 6.2 discovery).
///
/// Reads the Room's `m.bridge` then legacy `uk.half-shot.bridge` state events and
/// returns the first that yields a `protocol.id`. A native Matrix Room, an
/// unreadable state store, or a bridge event with no protocol id all resolve to
/// `None`. Never fabricates an id.
pub async fn room_bridge_protocol_id(room: &Room) -> Option<String> {
    for event_type in [BRIDGE_EVENT_TYPE, LEGACY_BRIDGE_EVENT_TYPE] {
        let Ok(states) = room
            .get_state_events(StateEventType::from(event_type))
            .await
        else {
            continue;
        };
        for raw in states {
            if let Some(content) = bridge_state_content(&raw) {
                if let Some(id) = parse_bridge_protocol_id(&content) {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// Resolve the bridged-Chat Network label for `room` on demand (the thin wrapper
/// around [`parse_bridge_network_name`]).
///
/// Reads the Room's `m.bridge` state events, then the legacy
/// `uk.half-shot.bridge` ones, and returns the first that yields a protocol name.
/// A native Matrix Room (no bridge state), an unreadable state store, or a bridge
/// event with no protocol name all resolve to `None` — the confirmation then uses
/// honest native framing. Never fabricates a name.
pub async fn room_bridge_network(room: &Room) -> Option<String> {
    for event_type in [BRIDGE_EVENT_TYPE, LEGACY_BRIDGE_EVENT_TYPE] {
        let Ok(states) = room
            .get_state_events(StateEventType::from(event_type))
            .await
        else {
            continue;
        };
        for raw in states {
            // Read only the `content` field as opaque JSON — never deserialize the
            // whole event or read any id (NFR-9).
            if let Some(content) = bridge_state_content(&raw) {
                if let Some(name) = parse_bridge_network_name(&content) {
                    return Some(name);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_protocol_displayname() {
        let content = json!({
            "bridgebot": "@telegrambot:beeper.local",
            "protocol": { "id": "telegram", "displayname": "Telegram" }
        });
        assert_eq!(
            parse_bridge_network_name(&content),
            Some("Telegram".to_owned())
        );
    }

    #[test]
    fn falls_back_to_protocol_id_when_no_displayname() {
        let content = json!({ "protocol": { "id": "whatsapp" } });
        assert_eq!(
            parse_bridge_network_name(&content),
            Some("whatsapp".to_owned())
        );
    }

    #[test]
    fn prefers_displayname_over_id() {
        let content = json!({ "protocol": { "id": "signal", "displayname": "Signal" } });
        assert_eq!(
            parse_bridge_network_name(&content),
            Some("Signal".to_owned())
        );
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let content = json!({ "protocol": { "displayname": "  Telegram  " } });
        assert_eq!(
            parse_bridge_network_name(&content),
            Some("Telegram".to_owned())
        );
    }

    #[test]
    fn no_protocol_yields_none() {
        let content = json!({ "bridgebot": "@bot:example.org" });
        assert_eq!(parse_bridge_network_name(&content), None);
    }

    #[test]
    fn empty_name_yields_none() {
        let content = json!({ "protocol": { "id": "", "displayname": "   " } });
        assert_eq!(parse_bridge_network_name(&content), None);
    }

    #[test]
    fn non_string_name_yields_none() {
        let content = json!({ "protocol": { "id": 42 } });
        assert_eq!(parse_bridge_network_name(&content), None);
    }

    #[test]
    fn caps_an_overlong_untrusted_label() {
        let long = "N".repeat(200);
        let content = json!({ "protocol": { "displayname": long } });
        let parsed = parse_bridge_network_name(&content).expect("a non-empty name parses");
        // Capped to MAX chars of content plus an honest truncation ellipsis.
        assert_eq!(parsed.chars().count(), MAX_NETWORK_NAME_CHARS + 1);
        assert!(parsed.ends_with('…'));
    }

    #[test]
    fn a_name_at_the_cap_is_not_ellipsized() {
        let exact = "N".repeat(MAX_NETWORK_NAME_CHARS);
        let content = json!({ "protocol": { "displayname": exact.clone() } });
        let parsed = parse_bridge_network_name(&content).expect("a non-empty name parses");
        assert_eq!(parsed, exact);
        assert!(!parsed.ends_with('…'));
    }

    #[test]
    fn malformed_content_yields_none() {
        // A native Matrix Room has no `m.bridge`; a stray non-object content is
        // just as safely `None`.
        assert_eq!(parse_bridge_network_name(&json!("not-an-object")), None);
        assert_eq!(parse_bridge_network_name(&json!(null)), None);
    }

    #[test]
    fn parses_protocol_id_for_discovery() {
        let content = json!({
            "bridgebot": "@whatsappbot:beeper.local",
            "protocol": { "id": "whatsapp", "displayname": "WhatsApp" }
        });
        assert_eq!(
            parse_bridge_protocol_id(&content),
            Some("whatsapp".to_owned())
        );
    }

    #[test]
    fn protocol_id_ignores_displayname() {
        // The discovery join key is the machine id, never the display label.
        let content = json!({ "protocol": { "id": "telegram", "displayname": "Telegram" } });
        assert_eq!(
            parse_bridge_protocol_id(&content),
            Some("telegram".to_owned())
        );
    }

    #[test]
    fn protocol_id_trims_and_rejects_empty_or_missing() {
        assert_eq!(
            parse_bridge_protocol_id(&json!({ "protocol": { "id": "  signal  " } })),
            Some("signal".to_owned())
        );
        assert_eq!(
            parse_bridge_protocol_id(&json!({ "protocol": { "id": "   " } })),
            None
        );
        // Only a displayname, no id → no join key.
        assert_eq!(
            parse_bridge_protocol_id(&json!({ "protocol": { "displayname": "Signal" } })),
            None
        );
        assert_eq!(
            parse_bridge_protocol_id(&json!({ "bridgebot": "@bot:example.org" })),
            None
        );
        assert_eq!(
            parse_bridge_protocol_id(&json!({ "protocol": { "id": 42 } })),
            None
        );
        assert_eq!(parse_bridge_protocol_id(&json!("not-an-object")), None);
    }
}

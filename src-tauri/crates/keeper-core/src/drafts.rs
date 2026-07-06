//! Cross-device draft mirroring (Story 7.2, AD-15).
//!
//! Story 7.1 made composer drafts durable **locally** (the `drafts` table in
//! `keeper.db`, the single source of truth). This module projects each local
//! draft to the account as **per-room Matrix account data** under the custom type
//! `dev.keeper.draft` (synced), plus a best-effort `Room::save_composer_draft`
//! (Element-family interop) — so unsent text follows the user across devices.
//!
//! Everything here is **best-effort**: every account-data / composer-draft error
//! is returned to the caller (which swallows and logs it at `debug`/`warn`, never
//! the body) and can never block or fail local persistence. The only symptom of a
//! partial/rejecting server is the absent cross-device echo (OQ-3).
//!
//! **Local always wins**: the mirror is read only to *offer* adoption
//! ([`load_remote_draft`]); it is never read back as truth. A cleared draft writes
//! a tombstone (`body: ""`) — account data cannot be truly removed — which reads
//! back as "no remote draft". The winner rule is purely local-wins and never
//! consults `updated_ts`, which rides along for future display/telemetry only.
//!
//! Draft bodies are never logged (NFR-9).

use std::collections::HashMap;
use std::sync::Mutex;

use matrix_sdk::ruma::events::macros::EventContent;
use matrix_sdk::{ComposerDraft, ComposerDraftType, Room};
use serde::{Deserialize, Serialize};

pub use crate::vm::{DraftMirrorBatch, RemoteDraftVm};

/// The content of a `dev.keeper.draft` room-account-data event (Story 7.2): the
/// synced projection of a keeper-local composer draft.
///
/// A Ruma `EventContent` of `kind = RoomAccountData`, so the SDK writes it via
/// [`Room::set_account_data`], reads it via `Room::account_data_static`, and
/// dispatches it to a typed event handler (the generated `KeeperDraftEvent`).
/// `body` is the unsent text (`""` = tombstone, i.e. cleared); `updated_ts` is
/// informational only (never consulted to pick a winner). The body is never
/// logged.
#[derive(Clone, Debug, Serialize, Deserialize, EventContent)]
#[ruma_event(type = "dev.keeper.draft", kind = RoomAccountData)]
pub struct KeeperDraftEventContent {
    /// The unsent composer text. Empty string is a tombstone (cleared draft).
    pub body: String,
    /// Write time in milliseconds since the Unix epoch (UTC). Informational only.
    pub updated_ts: i64,
    /// The Matrix device id that wrote this mirror. Room account data is
    /// account-level (shared across devices) and the SDK echoes a device's own
    /// write back to that same device's sync — so the observing handler filters
    /// events whose `origin` is its own device id, otherwise the user would be
    /// offered their own just-mirrored text as a bogus "edited on another device"
    /// conflict. Absent (defaulted `""`) on legacy/foreign events, which never
    /// match a real device id and so are treated as remote (Story 7.2).
    #[serde(default)]
    pub origin: String,
}

/// The writing device's Matrix device id, used to stamp [`KeeperDraftEventContent::origin`]
/// so the observing handler can drop this device's own echoes. Empty when the
/// client has no device id (never matches a real id → no self-filtering).
fn own_device_id(room: &Room) -> String {
    room.client()
        .device_id()
        .map(|d| d.as_str().to_owned())
        .unwrap_or_default()
}

/// Current wall-clock time in milliseconds since the Unix epoch (UTC). Generated
/// at write time so the mirror never trusts a stale timestamp from the caller.
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// Per-`(account, room)` last-mirrored body, so an adopt→save→mirror echo does
/// not storm the homeserver. Keyed by `` `${account_id}\u{1f}${room_id}` ``. A
/// mirror write whose body equals the last mirrored body for its key is skipped;
/// reconciliation compares **bodies** (not `updated_ts`), so a re-mirror carrying
/// only a new timestamp is ignored on the other device and the system converges
/// after at most one redundant write.
///
/// A single process-wide map, guarded by a synchronous [`Mutex`] held only for
/// the compare-and-set (never across an `.await`). The body is never logged.
static LAST_MIRRORED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// The dedupe key for a `(account, room)`; the unit separator can never appear in
/// a Matrix id, so the join is unambiguous.
fn dedupe_key(account_id: &str, room_id: &str) -> String {
    format!("{account_id}\u{1f}{room_id}")
}

/// Record `body` as the last mirrored body for the key and report whether it
/// *changed* (i.e. whether a mirror write should proceed). Returns `true` when
/// the body differs from the last mirrored one (write), `false` when identical
/// (skip). A poisoned lock degrades to "always write" (never blocks the mirror).
fn should_mirror(account_id: &str, room_id: &str, body: &str) -> bool {
    let key = dedupe_key(account_id, room_id);
    let mut guard = match LAST_MIRRORED.lock() {
        Ok(guard) => guard,
        // A poisoned lock must never wedge the mirror: proceed with the write.
        Err(poisoned) => poisoned.into_inner(),
    };
    let map = guard.get_or_insert_with(HashMap::new);
    if map.get(&key).map(String::as_str) == Some(body) {
        return false;
    }
    map.insert(key, body.to_owned());
    true
}

/// Forget the last-mirrored body for a key so the next mirror write always
/// proceeds. Used when a mirror write *fails* (the server did not accept the
/// body, so the next attempt must not be deduped away).
fn forget_mirrored(account_id: &str, room_id: &str) {
    let key = dedupe_key(account_id, room_id);
    if let Ok(mut guard) = LAST_MIRRORED.lock() {
        if let Some(map) = guard.as_mut() {
            map.remove(&key);
        }
    }
}

/// Mirror `body` for `room` to the account (Story 7.2): write the synced
/// `dev.keeper.draft` account-data event `{ body, updated_ts: now }` **and** a
/// best-effort local `Room::save_composer_draft` (Element interop).
///
/// Deduped by last-mirrored body per key: an identical re-mirror is a no-op (no
/// homeserver round-trip), so an adopt→save→mirror echo does not storm. The
/// `updated_ts` is generated here at write time — the caller's timestamp is never
/// trusted. Both writes are best-effort; an error is returned for the caller to
/// swallow and log (never the body), and a failed account-data write clears the
/// dedupe entry so the next attempt retries. The body is never logged here.
pub async fn mirror_draft(
    account_id: &str,
    room: &Room,
    body: &str,
) -> Result<(), matrix_sdk::Error> {
    let room_id = room.room_id().as_str();
    if !should_mirror(account_id, room_id, body) {
        tracing::debug!(account_id = %account_id, room_id = %room_id, "draft mirror deduped (unchanged body)");
        return Ok(());
    }
    // The synced mechanism: per-room account data other devices observe.
    let content = KeeperDraftEventContent {
        body: body.to_owned(),
        updated_ts: now_ms(),
        origin: own_device_id(room),
    };
    if let Err(e) = room.set_account_data(content).await {
        // The write did not land: drop the dedupe entry so the next attempt is
        // not skipped as "unchanged".
        forget_mirrored(account_id, room_id);
        return Err(e);
    }
    // Additive Element-family interop: local SDK-store composer draft (not itself
    // synced). Best-effort — a failure here must NOT undo or fail the synced write
    // above (which already landed), so it is swallowed and logged, never returned.
    let draft = ComposerDraft {
        plain_text: body.to_owned(),
        html_text: None,
        draft_type: ComposerDraftType::NewMessage,
        attachments: Vec::new(),
    };
    if let Err(e) = room.save_composer_draft(draft, None).await {
        tracing::warn!(account_id = %account_id, room_id = %room_id, error = %e, "save_composer_draft failed (best-effort interop); synced mirror already written");
    } else {
        tracing::debug!(account_id = %account_id, room_id = %room_id, "draft mirrored");
    }
    Ok(())
}

/// Clear `room`'s draft mirror (Story 7.2): write the tombstone (`body: ""`)
/// account-data event **and** `Room::clear_composer_draft`.
///
/// Account data cannot be truly removed, so clearing writes an empty body; an
/// empty-body mirror reads back as "no remote draft" ([`load_remote_draft`] maps
/// it to `None`). Best-effort — an error is returned for the caller to swallow
/// and log; a best-effort-failed clear can transiently re-present a cleared draft
/// cross-device, which is acceptable because it re-*shows* recoverable text and
/// never destroys it. The dedupe entry is set to the empty body so a subsequent
/// identical clear is skipped.
pub async fn clear_draft_mirror(account_id: &str, room: &Room) -> Result<(), matrix_sdk::Error> {
    let room_id = room.room_id().as_str();
    if !should_mirror(account_id, room_id, "") {
        tracing::debug!(account_id = %account_id, room_id = %room_id, "draft mirror clear deduped (already tombstoned)");
        // Still best-effort clear the local composer draft (idempotent). Swallowed:
        // an interop-write failure must not fail an already-tombstoned clear.
        if let Err(e) = room.clear_composer_draft(None).await {
            tracing::warn!(account_id = %account_id, room_id = %room_id, error = %e, "clear_composer_draft failed (best-effort interop)");
        }
        return Ok(());
    }
    let content = KeeperDraftEventContent {
        body: String::new(),
        updated_ts: now_ms(),
        origin: own_device_id(room),
    };
    if let Err(e) = room.set_account_data(content).await {
        forget_mirrored(account_id, room_id);
        return Err(e);
    }
    // Best-effort interop clear: swallowed so a failure here does not undo the
    // synced tombstone (which already landed).
    if let Err(e) = room.clear_composer_draft(None).await {
        tracing::warn!(account_id = %account_id, room_id = %room_id, error = %e, "clear_composer_draft failed (best-effort interop); synced tombstone already written");
    } else {
        tracing::debug!(account_id = %account_id, room_id = %room_id, "draft mirror tombstoned");
    }
    Ok(())
}

/// Read `room`'s remote draft from the `dev.keeper.draft` account-data mirror
/// (Story 7.2), or `None` when there is no draft.
///
/// An empty body (a tombstone) maps to `None` — a cleared draft is "no remote
/// draft". Returned only to *offer* adoption; the local composer text stays
/// authoritative (local always wins). The body is never logged.
pub async fn load_remote_draft(room: &Room) -> Result<Option<RemoteDraftVm>, matrix_sdk::Error> {
    let raw = room
        .account_data_static::<KeeperDraftEventContent>()
        .await?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    // A malformed / partially-written mirror event is treated as "no remote draft"
    // (unreadable → absent) rather than surfacing an error the caller would swallow
    // anyway; local text stays authoritative (local always wins).
    let Ok(event) = raw.deserialize() else {
        tracing::debug!(room_id = %room.room_id(), "dev.keeper.draft account data unreadable; treating as no remote draft");
        return Ok(None);
    };
    Ok(resolve_remote_draft(event.content, &own_device_id(room)))
}

/// Resolve an observed `dev.keeper.draft` content to an adoptable [`RemoteDraftVm`],
/// or `None` when there is nothing remote to offer. Pure over its inputs, so the
/// own-echo and tombstone rules are unit-testable without a live `Room`.
///
/// Drops **this device's own echo**: room account data is account-level and the SDK
/// stores this device's own last write in the same per-room slot, so a non-empty
/// `origin` equal to our device id is a write we made, not a cross-device edit.
/// Offering it back would raise a bogus "edited on another device" conflict when the
/// local text has since diverged from our last mirror (e.g. a mirror write that never
/// landed). This mirrors the live handler's filter ([`register_draft_handler`]) so the
/// on-open read path and the live path agree. An empty body (tombstone) maps to `None`.
fn resolve_remote_draft(
    content: KeeperDraftEventContent,
    own_device: &str,
) -> Option<RemoteDraftVm> {
    if !own_device.is_empty() && content.origin == own_device {
        return None;
    }
    remote_draft_vm(content.body, content.updated_ts)
}

/// Map a `(body, updated_ts)` mirror payload to a [`RemoteDraftVm`], collapsing an
/// empty body to `None` (tombstone → "no remote draft"). Pure, so the empty-body
/// rule is unit-testable without a live `Room`.
fn remote_draft_vm(body: String, updated_ts: i64) -> Option<RemoteDraftVm> {
    if body.is_empty() {
        None
    } else {
        Some(RemoteDraftVm { body, updated_ts })
    }
}

/// Build a [`DraftMirrorBatch`] from an observed `dev.keeper.draft` event
/// (Story 7.2). An empty body carries `body: None` so the frontend clears any
/// offered remote draft for the key (tombstone). Pure over its inputs.
pub fn draft_mirror_batch(
    account_id: &str,
    room_id: &str,
    body: String,
    updated_ts: i64,
) -> DraftMirrorBatch {
    DraftMirrorBatch {
        account_id: account_id.to_owned(),
        room_id: room_id.to_owned(),
        body: if body.is_empty() { None } else { Some(body) },
        updated_ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_content_serde_round_trips() {
        let content = KeeperDraftEventContent {
            body: "half a message".to_owned(),
            updated_ts: 1_720_000_000_000,
            origin: "DEVICEID".to_owned(),
        };
        let json = serde_json::to_string(&content).expect("serialize");
        // The wire shape carries body, updated_ts, and the origin device id.
        assert_eq!(
            json,
            r#"{"body":"half a message","updated_ts":1720000000000,"origin":"DEVICEID"}"#
        );
        let back: KeeperDraftEventContent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.body, "half a message");
        assert_eq!(back.updated_ts, 1_720_000_000_000);
        assert_eq!(back.origin, "DEVICEID");
        // Legacy / foreign events without an `origin` default to "" (treated as
        // remote — never matches a real device id).
        let legacy: KeeperDraftEventContent =
            serde_json::from_str(r#"{"body":"hi","updated_ts":1}"#).expect("deserialize legacy");
        assert_eq!(legacy.origin, "");
    }

    #[test]
    fn empty_body_tombstone_maps_to_none() {
        // A tombstone (empty body) reads back as "no remote draft".
        assert_eq!(remote_draft_vm(String::new(), 42), None);
        // A non-empty body is an adoptable remote draft.
        assert_eq!(
            remote_draft_vm("text".to_owned(), 7),
            Some(RemoteDraftVm {
                body: "text".to_owned(),
                updated_ts: 7,
            })
        );
    }

    #[test]
    fn resolve_remote_draft_drops_own_echo() {
        let mk = |body: &str, origin: &str| KeeperDraftEventContent {
            body: body.to_owned(),
            updated_ts: 1,
            origin: origin.to_owned(),
        };
        // Our own echo (origin == our device id) is not a cross-device draft: dropped
        // even when its body differs from local, so it never raises a bogus conflict.
        assert_eq!(
            resolve_remote_draft(mk("stale own text", "DEV_A"), "DEV_A"),
            None
        );
        // A genuinely remote edit (different origin) is offered.
        assert_eq!(
            resolve_remote_draft(mk("from another device", "DEV_B"), "DEV_A"),
            Some(RemoteDraftVm {
                body: "from another device".to_owned(),
                updated_ts: 1,
            })
        );
        // A legacy/foreign event with no origin is treated as remote (never matches a
        // real device id).
        assert_eq!(
            resolve_remote_draft(mk("legacy", ""), "DEV_A"),
            Some(RemoteDraftVm {
                body: "legacy".to_owned(),
                updated_ts: 1,
            })
        );
        // No own device id (unreachable for a synced client): no self-filtering, so a
        // present body is still offered rather than silently dropped.
        assert_eq!(
            resolve_remote_draft(mk("body", "DEV_A"), ""),
            Some(RemoteDraftVm {
                body: "body".to_owned(),
                updated_ts: 1,
            })
        );
        // A tombstone (empty body) is always "no remote draft".
        assert_eq!(resolve_remote_draft(mk("", "DEV_B"), "DEV_A"), None);
    }

    #[test]
    fn batch_maps_empty_body_to_none() {
        let tombstone = draft_mirror_batch("acctA", "!r1", String::new(), 1);
        assert_eq!(tombstone.body, None);
        assert_eq!(tombstone.account_id, "acctA");
        assert_eq!(tombstone.room_id, "!r1");

        let present = draft_mirror_batch("acctA", "!r1", "hi".to_owned(), 2);
        assert_eq!(present.body, Some("hi".to_owned()));
    }

    #[test]
    fn dedupe_skips_identical_re_mirror() {
        // Distinct keys so this test never collides with a sibling (the map is
        // process-wide static state).
        let account = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let room = "!dedupe-test:example.org";
        // First write of a body proceeds…
        assert!(should_mirror(account, room, "one"));
        // …an identical re-mirror is skipped…
        assert!(!should_mirror(account, room, "one"));
        // …a changed body proceeds again…
        assert!(should_mirror(account, room, "two"));
        assert!(!should_mirror(account, room, "two"));
        // …and a failed write forgetting the entry re-arms the next attempt.
        forget_mirrored(account, room);
        assert!(should_mirror(account, room, "two"));
    }
}

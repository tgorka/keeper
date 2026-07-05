//! The unified inbox: merge every active account's room-list stream into one
//! recency-ordered view model (AD-20).
//!
//! The Unified Inbox is computed **entirely in Rust**. Each active account's
//! per-account room-list stream (the same recency-sorted `VectorDiff` sequence
//! the single-account [`crate::account::AccountManager::subscribe_room_list`]
//! path already produces) feeds a per-account slot in a shared [`MergeState`].
//! On every per-account change the slots are re-merged by latest-event timestamp
//! descending (missing timestamps sort last, stably) and the whole recency-
//! ordered window is emitted as a single [`InboxBatch`] `Reset` op. The frontend
//! mirrors that window verbatim and **never** re-derives order or filter.
//!
//! Adding the Nth account is identical to adding the 2nd: the merge holds a
//! `HashMap<account_id, slot>` and enforces no count limit anywhere. Signing an
//! account out removes its slot and its rooms leave the merged inbox while the
//! others keep syncing.
//!
//! The 200-entry-per-account page (`ROOM_LIST_PAGE_SIZE`) is the existing bound;
//! no new virtualization is introduced here (full unified-inbox organization is
//! Epic 4).

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::vm::{InboxBatch, InboxOp, InboxRoomVm, RoomListBatch, RoomVm};

/// Sink that receives each produced [`InboxBatch`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if the batch was
/// delivered, `false` if the channel is closed (the merger then stops emitting).
pub type InboxSink = Box<dyn Fn(InboxBatch) -> bool + Send + Sync>;

/// One account's contribution to the merged inbox: its opaque id, hue index, and
/// the current room window it is streaming.
struct AccountSlot {
    hue_index: u8,
    /// The account's current room window, mirrored from its per-account
    /// `RoomListBatch` ops — recency-ordered within the account.
    rooms: Vec<RoomVm>,
}

/// Shared per-account merge state feeding one merged-inbox subscription.
///
/// Guarded by a single async mutex so concurrent per-account producers apply
/// their diffs and re-merge atomically. Cloneable via `Arc` so every account's
/// producer task shares the same state and the same output sink.
#[derive(Clone)]
pub struct InboxMerger {
    inner: Arc<Mutex<MergeState>>,
}

struct MergeState {
    accounts: HashMap<String, AccountSlot>,
    /// Receives the Pins window (pinned rooms, `sort_order` ascending) (Story 4.3).
    pins_sink: InboxSink,
    /// Receives the Favorites window (`!pinned && is_favourite`, recency order)
    /// (Story 4.4).
    favourites_sink: InboxSink,
    /// Receives the Inbox window
    /// (`!pinned && !is_favourite && (!is_archived || is_unread)`).
    inbox_sink: InboxSink,
    /// Receives the Archive window
    /// (`!pinned && !is_favourite && is_archived && !is_unread`) (Story 4.2).
    archive_sink: InboxSink,
    /// Keeper-local pin membership + order, keyed by `(account_id, room_id)` →
    /// `sort_order` (ascending). Reloaded from the registry and pushed in whole via
    /// [`InboxMerger::update_pins`] on every pin mutation (Story 4.3). A room in
    /// this map is placed in the Pins window and excluded from Inbox/Archive.
    pins: HashMap<(String, String), i64>,
    /// Set once any sink reports its channel is closed, so later producer
    /// updates stop trying to emit.
    closed: bool,
}

impl InboxMerger {
    /// Create a merger that partitions each merged window into four
    /// recency/order-authoritative streams (Story 4.2 + 4.3 + 4.4): `pins_sink`
    /// receives the Pins window (pinned rooms sorted by `sort_order` ascending),
    /// `favourites_sink` the Favorites window (`!pinned && is_favourite`, recency
    /// order), `inbox_sink` the Inbox window
    /// (`!pinned && !is_favourite && (!is_archived || is_unread)`), and
    /// `archive_sink` the Archive window
    /// (`!pinned && !is_favourite && is_archived && !is_unread`). Precedence is
    /// Pins > Favorites > Archive/Inbox — a pinned room lives only in the Pins
    /// window (never re-sorting on activity); a favourited-but-unpinned room lives
    /// only in the Favorites window. `pins` seeds the initial pin map (from
    /// [`crate::registry::get_pins`]); it is replaced whole by [`Self::update_pins`]
    /// on every mutation. Favourite state is SDK-sourced (the `m.favourite` notable
    /// tag), so it needs no seed and no out-of-band poke.
    pub fn new(
        inbox_sink: InboxSink,
        archive_sink: InboxSink,
        pins_sink: InboxSink,
        favourites_sink: InboxSink,
        pins: HashMap<(String, String), i64>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MergeState {
                accounts: HashMap::new(),
                pins_sink,
                favourites_sink,
                inbox_sink,
                archive_sink,
                pins,
                closed: false,
            })),
        }
    }

    /// Register an account slot (idempotent per id) before its producer starts,
    /// so an add is visible to the merge even before its first batch arrives.
    pub async fn register_account(&self, account_id: &str, hue_index: u8) {
        let mut state = self.inner.lock().await;
        state
            .accounts
            .entry(account_id.to_owned())
            .or_insert_with(|| AccountSlot {
                hue_index,
                rooms: Vec::new(),
            });
    }

    /// Remove an account slot (sign-out / shutdown) and re-emit the merged
    /// window so the account's rooms leave the inbox immediately. Idempotent.
    pub async fn remove_account(&self, account_id: &str) {
        let mut state = self.inner.lock().await;
        if state.accounts.remove(account_id).is_some() {
            emit(&mut state);
        }
    }

    /// Replace the whole pin map and re-emit all three windows out-of-band (Story
    /// 4.3). Called after a pin mutation (`pin_room`/`unpin_room`/`reorder_pins`)
    /// reloads [`crate::registry::get_pins`], so the strip updates within one frame.
    /// Mirrors [`Self::remove_account`]'s direct-`emit` poke: it is not driven by an
    /// account batch, so a no-active-subscription case is a harmless no-op re-emit.
    pub async fn update_pins(&self, pins: HashMap<(String, String), i64>) {
        let mut state = self.inner.lock().await;
        state.pins = pins;
        emit(&mut state);
    }

    /// Apply one account's per-account [`RoomListBatch`] to its slot, then
    /// re-merge and emit. Returns `false` once the output channel is closed so
    /// the caller's producer can stop.
    pub async fn apply_account_batch(&self, account_id: &str, batch: RoomListBatch) -> bool {
        let mut state = self.inner.lock().await;
        if state.closed {
            return false;
        }
        if let Some(slot) = state.accounts.get_mut(account_id) {
            slot.rooms = apply_room_list_batch(std::mem::take(&mut slot.rooms), &batch);
        }
        emit(&mut state)
    }
}

/// Emit the current merged window into all four sinks, recording channel
/// closure. The single recency-ordered merge is partitioned into four windows
/// (Story 4.2 + 4.3 + 4.4), each a `Reset` batch whose `total` is that
/// partition's own length. Returns `false` if any channel is closed.
///
/// Precedence is **Pins > Favorites > Archive/Inbox**. A room in `state.pins`
/// goes only to the **Pins** window, sorted by `sort_order` ascending (ties
/// broken deterministically by account/room id, never by recency). Of the
/// remaining unpinned rooms, favourited ones (`is_favourite`) go only to the
/// **Favorites** window in recency order. The rest splits into the **Inbox**
/// window (`!is_archived || is_unread` — an archived-unread room auto-returns)
/// and the **Archive** window (`is_archived && !is_unread`). The four windows are
/// strictly disjoint: the `!is_favourite` split keeps favourites out of
/// Inbox/Archive even under a transient sync state where the SDK-mutually-
/// exclusive favourite/low-priority tags briefly coexist.
fn emit(state: &mut MergeState) -> bool {
    if state.closed {
        return false;
    }
    let merged = merge(&state.accounts);
    // Split off pinned rooms first (they win over favourites/archive/unread), then
    // partition the rest. Recency order is preserved within each window.
    let (mut pinned_rooms, rest): (Vec<InboxRoomVm>, Vec<InboxRoomVm>) = merged
        .into_iter()
        .partition(|room| pin_order(&state.pins, room).is_some());
    // Order the Pins window by `sort_order` ascending, tie-breaking on
    // (account_id, room_id) so equal orders (e.g. a transient collision from two
    // near-simultaneous pins) resolve deterministically and identically on every
    // re-emit — never letting recency flip the strip order under the user. Every
    // room here is pinned, so `pin_order` is always `Some`; `i64::MAX` is an
    // unreachable safety default.
    pinned_rooms.sort_by(|a, b| {
        let oa = pin_order(&state.pins, a).unwrap_or(i64::MAX);
        let ob = pin_order(&state.pins, b).unwrap_or(i64::MAX);
        oa.cmp(&ob)
            .then_with(|| a.account_id.cmp(&b.account_id))
            .then_with(|| a.room_id.cmp(&b.room_id))
    });
    // Stamp the authoritative pin flag on the Pins window (merger-owned state).
    for room in &mut pinned_rooms {
        room.is_pinned = true;
    }
    // Of the unpinned rooms, favourites win over archive/inbox (recency order,
    // preserved by the stable partition). Favourite is SDK-sourced, already on the
    // VM via `to_inbox_room`, so nothing is stamped here.
    let (favourite_rooms, non_favourite): (Vec<InboxRoomVm>, Vec<InboxRoomVm>) =
        rest.into_iter().partition(|room| room.is_favourite);
    let (inbox_rooms, archive_rooms): (Vec<InboxRoomVm>, Vec<InboxRoomVm>) = non_favourite
        .into_iter()
        .partition(|room| !room.is_archived || room.is_unread);
    let pins_batch = InboxBatch {
        total: Some(pinned_rooms.len() as u32),
        ops: vec![InboxOp::Reset {
            rooms: pinned_rooms,
        }],
    };
    let favourites_batch = InboxBatch {
        total: Some(favourite_rooms.len() as u32),
        ops: vec![InboxOp::Reset {
            rooms: favourite_rooms,
        }],
    };
    let inbox_batch = InboxBatch {
        total: Some(inbox_rooms.len() as u32),
        ops: vec![InboxOp::Reset { rooms: inbox_rooms }],
    };
    let archive_batch = InboxBatch {
        total: Some(archive_rooms.len() as u32),
        ops: vec![InboxOp::Reset {
            rooms: archive_rooms,
        }],
    };
    // Emit all four windows; a close on any sink stops all future emissions.
    let pins_ok = (state.pins_sink)(pins_batch);
    let favourites_ok = (state.favourites_sink)(favourites_batch);
    let inbox_ok = (state.inbox_sink)(inbox_batch);
    let archive_ok = (state.archive_sink)(archive_batch);
    if !pins_ok || !favourites_ok || !inbox_ok || !archive_ok {
        state.closed = true;
        tracing::info!("pins/favorites/inbox/archive channel closed; stopping merged emissions");
        return false;
    }
    true
}

/// Look up a room's pin order in the pin map, or `None` if it is not pinned.
fn pin_order(pins: &HashMap<(String, String), i64>, room: &InboxRoomVm) -> Option<i64> {
    pins.get(&(room.account_id.clone(), room.room_id.clone()))
        .copied()
}

/// Pure recency merge: flatten every account's window into one list of
/// [`InboxRoomVm`], ordered by latest-event `timestamp` **descending**, with a
/// missing timestamp sorting last. The sort is stable, so rooms with equal (or
/// both-missing) timestamps keep a deterministic relative order (account id
/// ascending, then intra-account order). This is the unit-tested seam — it needs
/// no live `Client`.
fn merge(accounts: &HashMap<String, AccountSlot>) -> Vec<InboxRoomVm> {
    // Iterate accounts in id order so the stable sort's tie-breaking is
    // deterministic regardless of `HashMap` iteration order.
    let mut ids: Vec<&String> = accounts.keys().collect();
    ids.sort();

    let mut rows: Vec<InboxRoomVm> = Vec::new();
    for id in ids {
        let slot = &accounts[id];
        for room in &slot.rooms {
            rows.push(to_inbox_room(id, slot.hue_index, room));
        }
    }
    // Descending by timestamp; `None` sorts last. Stable sort preserves the
    // deterministic pre-order for ties.
    rows.sort_by(|a, b| match (b.timestamp, a.timestamp) {
        (Some(bt), Some(at)) => bt.cmp(&at),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    });
    rows
}

/// Project a per-account [`RoomVm`] into an [`InboxRoomVm`] carrying its account
/// attribution and hue.
fn to_inbox_room(account_id: &str, hue_index: u8, room: &RoomVm) -> InboxRoomVm {
    InboxRoomVm {
        account_id: account_id.to_owned(),
        hue_index,
        room_id: room.room_id.clone(),
        display_name: room.display_name.clone(),
        last_message: room.last_message.clone(),
        timestamp: room.timestamp,
        avatar_url: room.avatar_url.clone(),
        is_unread: room.is_unread,
        mention_count: room.mention_count,
        is_archived: room.is_archived,
        // `is_favourite` is SDK-sourced (the `m.favourite` notable tag), copied
        // straight through like `is_archived` — the merge partitions on it but
        // never owns it.
        is_favourite: room.is_favourite,
        // `is_pinned` is set in `emit` from the merger's pin map (the pin state is
        // merger-owned, not SDK-sourced); a freshly merged row defaults unpinned.
        is_pinned: false,
    }
}

/// Fold one account's [`RoomListBatch`] ops onto its current window, returning
/// the new window. Mirrors the frontend's index-based reducer so the per-account
/// slot always matches the SDK's recency-sorted order. Out-of-range indices are
/// ignored (defensive; the SDK never emits them).
fn apply_room_list_batch(mut rooms: Vec<RoomVm>, batch: &RoomListBatch) -> Vec<RoomVm> {
    use crate::vm::RoomListOp;
    for op in &batch.ops {
        match op {
            RoomListOp::Reset { rooms: r } => rooms = r.clone(),
            RoomListOp::Append { rooms: r } => rooms.extend(r.iter().cloned()),
            RoomListOp::Clear => rooms.clear(),
            RoomListOp::PushFront { room } => rooms.insert(0, room.clone()),
            RoomListOp::PushBack { room } => rooms.push(room.clone()),
            RoomListOp::PopFront => {
                if !rooms.is_empty() {
                    rooms.remove(0);
                }
            }
            RoomListOp::PopBack => {
                rooms.pop();
            }
            RoomListOp::Insert { index, room } => {
                let i = *index as usize;
                if i <= rooms.len() {
                    rooms.insert(i, room.clone());
                }
            }
            RoomListOp::Set { index, room } => {
                let i = *index as usize;
                if i < rooms.len() {
                    rooms[i] = room.clone();
                }
            }
            RoomListOp::Remove { index } => {
                let i = *index as usize;
                if i < rooms.len() {
                    rooms.remove(i);
                }
            }
            RoomListOp::Truncate { length } => {
                let l = *length as usize;
                if l < rooms.len() {
                    rooms.truncate(l);
                }
            }
        }
    }
    rooms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::RoomListOp;
    use std::sync::Mutex as StdMutex;

    fn room(id: &str, ts: Option<i64>) -> RoomVm {
        RoomVm {
            room_id: id.to_owned(),
            display_name: id.to_owned(),
            last_message: None,
            timestamp: ts,
            avatar_url: None,
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
        }
    }

    /// A room with explicit archive/unread flags for partition tests (Story 4.2).
    fn room_flags(id: &str, ts: Option<i64>, is_archived: bool, is_unread: bool) -> RoomVm {
        RoomVm {
            is_archived,
            is_unread,
            ..room(id, ts)
        }
    }

    /// A favourited room for Favorites-window partition tests (Story 4.4).
    fn room_fav(id: &str, ts: Option<i64>, is_archived: bool, is_unread: bool) -> RoomVm {
        RoomVm {
            is_favourite: true,
            ..room_flags(id, ts, is_archived, is_unread)
        }
    }

    fn slot(hue: u8, rooms: Vec<RoomVm>) -> AccountSlot {
        AccountSlot {
            hue_index: hue,
            rooms,
        }
    }

    fn state_of(pairs: Vec<(&str, AccountSlot)>) -> HashMap<String, AccountSlot> {
        pairs
            .into_iter()
            .map(|(id, s)| (id.to_owned(), s))
            .collect()
    }

    #[test]
    fn merge_orders_by_recency_descending_across_accounts() {
        let accounts = state_of(vec![
            (
                "acctA",
                slot(0, vec![room("!a1", Some(100)), room("!a2", Some(300))]),
            ),
            (
                "acctB",
                slot(1, vec![room("!b1", Some(200)), room("!b2", Some(400))]),
            ),
        ]);
        let merged = merge(&accounts);
        let ids: Vec<&str> = merged.iter().map(|r| r.room_id.as_str()).collect();
        // 400, 300, 200, 100 descending across both accounts.
        assert_eq!(ids, ["!b2", "!a2", "!b1", "!a1"]);
        // Each row carries its account id and hue.
        let b2 = &merged[0];
        assert_eq!(b2.account_id, "acctB");
        assert_eq!(b2.hue_index, 1);
        let a2 = &merged[1];
        assert_eq!(a2.account_id, "acctA");
        assert_eq!(a2.hue_index, 0);
    }

    #[test]
    fn to_inbox_room_carries_unread_fields() {
        let src = RoomVm {
            room_id: "!u:example.org".to_owned(),
            display_name: "Unread".to_owned(),
            last_message: None,
            timestamp: Some(1),
            avatar_url: None,
            is_unread: true,
            mention_count: 3,
            is_archived: true,
            is_favourite: true,
        };
        let inbox_room = to_inbox_room("acctA", 4, &src);
        assert!(inbox_room.is_unread);
        assert_eq!(inbox_room.mention_count, 3);
        assert!(inbox_room.is_archived);
        // `is_favourite` is SDK-sourced, copied straight through like `is_archived`.
        assert!(inbox_room.is_favourite);
        // `to_inbox_room` defaults unpinned; `emit` stamps the pin flag.
        assert!(!inbox_room.is_pinned);
        assert_eq!(inbox_room.account_id, "acctA");
        assert_eq!(inbox_room.hue_index, 4);
    }

    #[test]
    fn merge_sorts_missing_timestamp_last_stably() {
        let accounts = state_of(vec![
            (
                "acctA",
                slot(0, vec![room("!a1", None), room("!a2", Some(50))]),
            ),
            (
                "acctB",
                slot(1, vec![room("!b1", Some(70)), room("!b2", None)]),
            ),
        ]);
        let merged = merge(&accounts);
        let ids: Vec<&str> = merged.iter().map(|r| r.room_id.as_str()).collect();
        // Timestamped first (70, 50 desc), then the two None rows in the stable
        // deterministic pre-order (account id ascending): a1 (acctA) then b2.
        assert_eq!(ids, ["!b1", "!a2", "!a1", "!b2"]);
    }

    #[test]
    fn merge_of_six_accounts_has_no_cap() {
        // N=6 integrates identically to N=2: no path limits the count.
        let mut pairs = Vec::new();
        for i in 0..6u8 {
            let id = format!("acct{i}");
            let ts = (i as i64 + 1) * 100;
            // Leak the id string into a 'static-ish owned key via the helper.
            pairs.push((id, slot(i, vec![room(&format!("!r{i}"), Some(ts))])));
        }
        let accounts: HashMap<String, AccountSlot> = pairs.into_iter().collect();
        let merged = merge(&accounts);
        assert_eq!(merged.len(), 6, "all six accounts' rooms are present");
        // Highest timestamp (acct5, ts 600) is first.
        assert_eq!(merged[0].room_id, "!r5");
        assert_eq!(merged[0].account_id, "acct5");
        assert_eq!(merged[5].room_id, "!r0");
    }

    #[test]
    fn apply_room_list_batch_folds_ops_in_order() {
        let rooms = apply_room_list_batch(
            Vec::new(),
            &RoomListBatch {
                ops: vec![
                    RoomListOp::Reset {
                        rooms: vec![room("!a", Some(1)), room("!b", Some(2))],
                    },
                    RoomListOp::PushFront {
                        room: room("!c", Some(3)),
                    },
                    RoomListOp::Remove { index: 2 },
                ],
                total: Some(9),
            },
        );
        let ids: Vec<&str> = rooms.iter().map(|r| r.room_id.as_str()).collect();
        assert_eq!(ids, ["!c", "!a"]);
    }

    /// Shared capture buffer for one sink's emitted batches.
    type Captured = Arc<StdMutex<Vec<InboxBatch>>>;

    /// Room ids of the last `Reset` batch captured in `store`, or a panic.
    fn last_reset_ids(store: &Captured) -> Vec<String> {
        let batches = store.lock().expect("lock");
        let last = batches.last().expect("a batch");
        match &last.ops[0] {
            InboxOp::Reset { rooms } => rooms.iter().map(|r| r.room_id.clone()).collect(),
            other => panic!("expected Reset, got {other:?}"),
        }
    }

    /// Build a merger over four capture vecs (inbox, archive, pins, favourites) so
    /// partition tests can assert each window independently. Seeds an empty pin
    /// map; tests that exercise pins push a map via [`InboxMerger::update_pins`].
    fn capturing_merger() -> (InboxMerger, Captured, Captured, Captured, Captured) {
        capturing_merger_with_pins(HashMap::new())
    }

    /// Like [`capturing_merger`] but seeds the merger's pin map. Returns captures
    /// in `(merger, inbox, archive, pins, favourites)` order.
    fn capturing_merger_with_pins(
        pins: HashMap<(String, String), i64>,
    ) -> (InboxMerger, Captured, Captured, Captured, Captured) {
        let inbox: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let archive: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let pins_cap: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let favourites: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let inbox_store = inbox.clone();
        let archive_store = archive.clone();
        let pins_store = pins_cap.clone();
        let favourites_store = favourites.clone();
        let merger = InboxMerger::new(
            Box::new(move |batch: InboxBatch| {
                inbox_store.lock().expect("lock").push(batch);
                true
            }),
            Box::new(move |batch: InboxBatch| {
                archive_store.lock().expect("lock").push(batch);
                true
            }),
            Box::new(move |batch: InboxBatch| {
                pins_store.lock().expect("lock").push(batch);
                true
            }),
            Box::new(move |batch: InboxBatch| {
                favourites_store.lock().expect("lock").push(batch);
                true
            }),
            pins,
        );
        (merger, inbox, archive, pins_cap, favourites)
    }

    /// Build a pin map from `(account_id, room_id, order)` triples.
    fn pin_map(entries: &[(&str, &str, i64)]) -> HashMap<(String, String), i64> {
        entries
            .iter()
            .map(|(a, r, o)| ((a.to_string(), r.to_string()), *o))
            .collect()
    }

    #[tokio::test]
    async fn merger_emits_reset_on_add_batch_and_remove() {
        let (merger, inbox, _archive, _pins, _favourites) = capturing_merger();

        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;

        // Account A streams two rooms.
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room("!a1", Some(100)), room("!a2", Some(300))],
                    }],
                    total: Some(2),
                },
            )
            .await;
        // Account B streams one newer room.
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room("!b1", Some(400))],
                    }],
                    total: Some(1),
                },
            )
            .await;

        {
            let batches = inbox.lock().expect("lock");
            let last = batches.last().expect("a batch");
            // Each window's total is that partition's own length (Story 4.2).
            assert_eq!(last.total, Some(3), "inbox total is the partition length");
        }
        // b1 (400), a2 (300), a1 (100) recency desc across accounts.
        assert_eq!(last_reset_ids(&inbox), ["!b1", "!a2", "!a1"]);

        // Signing account B out removes its rooms from the merged inbox; A stays.
        merger.remove_account("acctB").await;
        assert_eq!(
            last_reset_ids(&inbox),
            ["!a2", "!a1"],
            "only account A's rooms remain"
        );
    }

    #[tokio::test]
    async fn emit_partitions_inbox_and_archive_preserving_recency() {
        // Golden case (Story 4.2): recency-desc merged window
        //   [D !archived read (400), C archived unread (300),
        //    B archived read (200), A !archived unread (100)]
        // partitions to inbox = [D, C, A] (!is_archived || is_unread) and
        // archive = [B] (is_archived && !is_unread).
        let (merger, inbox, archive, _pins, _favourites) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!d", Some(400), false, false),
                            room_flags("!c", Some(300), true, true),
                            room_flags("!b", Some(200), true, false),
                            room_flags("!a", Some(100), false, true),
                        ],
                    }],
                    total: Some(4),
                },
            )
            .await;

        // Inbox: non-archived plus the archived-unread auto-return, recency order.
        assert_eq!(last_reset_ids(&inbox), ["!d", "!c", "!a"]);
        {
            let batches = inbox.lock().expect("lock");
            assert_eq!(batches.last().expect("batch").total, Some(3));
        }
        // Archive: only the archived-read room.
        assert_eq!(last_reset_ids(&archive), ["!b"]);
        {
            let batches = archive.lock().expect("lock");
            assert_eq!(batches.last().expect("batch").total, Some(1));
        }
    }

    /// Read the `is_pinned` flags of the last `Reset` in `store`.
    fn last_reset_pinned(store: &Captured) -> Vec<bool> {
        let batches = store.lock().expect("lock");
        let last = batches.last().expect("a batch");
        match &last.ops[0] {
            InboxOp::Reset { rooms } => rooms.iter().map(|r| r.is_pinned).collect(),
            other => panic!("expected Reset, got {other:?}"),
        }
    }

    /// Read the `is_favourite` flags of the last `Reset` in `store`.
    fn last_reset_favourite(store: &Captured) -> Vec<bool> {
        let batches = store.lock().expect("lock");
        let last = batches.last().expect("a batch");
        match &last.ops[0] {
            InboxOp::Reset { rooms } => rooms.iter().map(|r| r.is_favourite).collect(),
            other => panic!("expected Reset, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn emit_partitions_four_windows_pins_over_favorites() {
        // Golden four-window case (Story 4.4), precedence Pins > Favorites >
        // Archive/Inbox. Recency-desc merged window:
        //   [A pin+fav(ord 0), B fav, C archived-read, D unread, E read, F fav]
        // → pins       = [A]        (pinned wins over favourite; removed from below)
        //   favorites  = [B, F]     (!pinned && is_favourite, recency order)
        //   inbox      = [D, E]     (!pinned && !fav && (!archived || unread))
        //   archive    = [C]        (!pinned && !fav && archived && !unread)
        let pins = pin_map(&[("acctA", "!a", 0)]);
        let (merger, inbox, archive, pins_cap, favourites) = capturing_merger_with_pins(pins);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            // recency desc: a(600), b(500), c(400), d(300), e(200), f(100)
                            room_fav("!a", Some(600), false, false), // pinned + favourite
                            room_fav("!b", Some(500), false, false), // favourite
                            room_flags("!c", Some(400), true, false), // archived read
                            room_flags("!d", Some(300), false, true), // unread
                            room_flags("!e", Some(200), false, false), // plain read
                            room_fav("!f", Some(100), false, false), // favourite
                        ],
                    }],
                    total: Some(6),
                },
            )
            .await;

        // Pins: [A], pinned flagged. A is favourite too, but pins win — not in favs.
        assert_eq!(last_reset_ids(&pins_cap), ["!a"]);
        assert_eq!(last_reset_pinned(&pins_cap), [true]);
        // Favorites: [B, F] in recency order, all favourite, none pinned.
        assert_eq!(last_reset_ids(&favourites), ["!b", "!f"]);
        assert_eq!(last_reset_favourite(&favourites), [true, true]);
        assert_eq!(last_reset_pinned(&favourites), [false, false]);
        {
            let batches = favourites.lock().expect("lock");
            assert_eq!(batches.last().expect("batch").total, Some(2));
        }
        // Inbox: [D, E] — unread auto-return plus plain read, no favourites.
        assert_eq!(last_reset_ids(&inbox), ["!d", "!e"]);
        assert_eq!(last_reset_favourite(&inbox), [false, false]);
        // Archive: [C] — archived-read, not favourite, not pinned.
        assert_eq!(last_reset_ids(&archive), ["!c"]);
    }

    #[tokio::test]
    async fn favourite_and_archived_resolves_to_favorites_not_archive() {
        // The SDK makes favourite and low-priority mutually exclusive, but even a
        // transient sync state where a room is both must resolve to Favorites: the
        // `!is_favourite` guard on the archive/inbox predicates keeps the windows
        // strictly disjoint (favourite wins over archive here).
        let (merger, inbox, archive, _pins, favourites) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_fav("!g", Some(200), true, false), // favourite + archived
                            room_flags("!h", Some(100), true, false), // archived only
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        // The favourite+archived room lands in Favorites, not Archive.
        assert_eq!(last_reset_ids(&favourites), ["!g"]);
        assert_eq!(last_reset_ids(&archive), ["!h"]);
        assert_eq!(last_reset_ids(&inbox), Vec::<String>::new());
    }

    #[tokio::test]
    async fn favourite_leaves_inbox_and_returns_on_unfavourite() {
        // Favouriting removes a room from the chronological Inbox flow; clearing
        // the favourite tag returns it to its recency position (SDK re-emit).
        let (merger, inbox, _archive, _pins, favourites) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!x", Some(300), false, false),
                            room_fav("!y", Some(200), false, false), // favourite
                            room_flags("!z", Some(100), false, false),
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        assert_eq!(last_reset_ids(&favourites), ["!y"]);
        assert_eq!(
            last_reset_ids(&inbox),
            ["!x", "!z"],
            "favourite left the inbox"
        );

        // The SDK re-emits with !y no longer favourite; it returns to recency order.
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!x", Some(300), false, false),
                            room_flags("!y", Some(200), false, false), // unfavourited
                            room_flags("!z", Some(100), false, false),
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        assert_eq!(last_reset_ids(&favourites), Vec::<String>::new());
        assert_eq!(
            last_reset_ids(&inbox),
            ["!x", "!y", "!z"],
            "unfavourited room back in chronological position"
        );
    }

    #[tokio::test]
    async fn emit_partitions_pins_inbox_and_archive_pins_win() {
        // Golden three-window case (Story 4.3): recency-desc merged window
        //   [A pin(ord 1), B pin(ord 0), C archived read, D unread, E read]
        // → pins = [B, A] (sorted by sort_order asc; removed from below),
        //   inbox = [D, E], archive = [C]. A pinned room stays in Pins even when
        //   archived/unread (B is pinned-and-archived here to prove pins win).
        let pins = pin_map(&[("acctA", "!a", 1), ("acctA", "!b", 0)]);
        let (merger, inbox, archive, pins_cap, _favourites) = capturing_merger_with_pins(pins);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            // ordered by recency desc: a(500), b(400), c(300), d(200), e(100)
                            room_flags("!a", Some(500), false, false),
                            room_flags("!b", Some(400), true, false), // pinned + archived
                            room_flags("!c", Some(300), true, false), // archived read
                            room_flags("!d", Some(200), false, true), // unread
                            room_flags("!e", Some(100), false, false), // plain
                        ],
                    }],
                    total: Some(5),
                },
            )
            .await;

        // Pins: [B (ord 0), A (ord 1)], all flagged is_pinned.
        assert_eq!(last_reset_ids(&pins_cap), ["!b", "!a"]);
        assert_eq!(last_reset_pinned(&pins_cap), [true, true]);
        {
            let batches = pins_cap.lock().expect("lock");
            assert_eq!(batches.last().expect("batch").total, Some(2));
        }
        // Inbox: unpinned, !archived||unread, recency order → [D, E].
        assert_eq!(last_reset_ids(&inbox), ["!d", "!e"]);
        assert_eq!(last_reset_pinned(&inbox), [false, false]);
        // Archive: unpinned archived-read → [C]. B is archived but pinned, so it is
        // NOT here (pins win, no duplication).
        assert_eq!(last_reset_ids(&archive), ["!c"]);
    }

    #[tokio::test]
    async fn pins_with_equal_order_tie_break_deterministically_not_by_recency() {
        // Two pins share the same sort_order (a transient collision). They must
        // order by (account_id, room_id) — never by recency — and stay put when
        // recency changes, so the strip never flips under the user.
        let pins = pin_map(&[("acctA", "!m", 5), ("acctA", "!n", 5)]);
        let (merger, _inbox, _archive, pins_cap, _favourites) = capturing_merger_with_pins(pins);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!n", Some(400), false, false), // newer
                            room_flags("!m", Some(100), false, false), // older
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        // room_id ascending wins over recency: !m before !n despite !n being newer.
        assert_eq!(last_reset_ids(&pins_cap), ["!m", "!n"]);

        // Bump !n to be even newer; the tie-break is unchanged.
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!n", Some(900), false, false),
                            room_flags("!m", Some(100), false, false),
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        assert_eq!(
            last_reset_ids(&pins_cap),
            ["!m", "!n"],
            "order stable across recency"
        );
    }

    #[tokio::test]
    async fn pinned_room_stays_on_newer_activity_elsewhere() {
        // A pinned room keeps its Pins-window position when an unpinned chat gets
        // newer activity (only the Inbox window re-orders).
        let pins = pin_map(&[("acctA", "!p", 0)]);
        let (merger, inbox, _archive, pins_cap, _favourites) = capturing_merger_with_pins(pins);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!p", Some(100), false, false), // pinned, oldest
                            room_flags("!x", Some(200), false, false),
                            room_flags("!y", Some(300), false, false),
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        assert_eq!(last_reset_ids(&pins_cap), ["!p"]);
        assert_eq!(last_reset_ids(&inbox), ["!y", "!x"]);

        // A newer message lands on !x (now the most recent). The Inbox re-orders;
        // the Pins window is untouched.
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!p", Some(100), false, false),
                            room_flags("!x", Some(400), false, false), // bumped
                            room_flags("!y", Some(300), false, false),
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        assert_eq!(last_reset_ids(&pins_cap), ["!p"], "pin position unchanged");
        assert_eq!(last_reset_ids(&inbox), ["!x", "!y"], "inbox re-ordered");
    }

    #[tokio::test]
    async fn update_pins_re_emits_all_three_windows() {
        // Start with no pins: all rooms land in the Inbox window.
        let (merger, inbox, archive, pins_cap, _favourites) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_flags("!a", Some(300), false, false),
                            room_flags("!b", Some(200), false, false),
                            room_flags("!c", Some(100), false, false),
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        assert_eq!(last_reset_ids(&pins_cap), Vec::<String>::new());
        assert_eq!(last_reset_ids(&inbox), ["!a", "!b", "!c"]);

        // Pin b then a (a first at ord 0). update_pins re-emits all three windows
        // out-of-band with no new account batch.
        merger
            .update_pins(pin_map(&[("acctA", "!a", 0), ("acctA", "!b", 1)]))
            .await;
        assert_eq!(last_reset_ids(&pins_cap), ["!a", "!b"]);
        assert_eq!(
            last_reset_ids(&inbox),
            ["!c"],
            "pinned rooms leave the inbox"
        );
        assert_eq!(last_reset_ids(&archive), Vec::<String>::new());

        // Unpin all: everything returns to the inbox.
        merger.update_pins(HashMap::new()).await;
        assert_eq!(last_reset_ids(&pins_cap), Vec::<String>::new());
        assert_eq!(last_reset_ids(&inbox), ["!a", "!b", "!c"]);
    }
}

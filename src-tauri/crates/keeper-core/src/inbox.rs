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

/// One account's contribution to the merged inbox: its opaque id, hue index, the
/// current room window it is streaming, and its known server-side total.
struct AccountSlot {
    hue_index: u8,
    /// The account's current room window, mirrored from its per-account
    /// `RoomListBatch` ops — recency-ordered within the account.
    rooms: Vec<RoomVm>,
    /// The account's known total, when the server has reported it.
    total: Option<u32>,
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
    sink: InboxSink,
    /// Set once the sink reports the channel is closed, so later producer
    /// updates stop trying to emit.
    closed: bool,
}

impl InboxMerger {
    /// Create a merger that emits merged batches into `sink`.
    pub fn new(sink: InboxSink) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MergeState {
                accounts: HashMap::new(),
                sink,
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
                total: None,
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
            if let Some(total) = batch.total {
                slot.total = Some(total);
            }
        }
        emit(&mut state)
    }
}

/// Emit the current merged window into the sink, recording channel closure.
/// Returns `false` if the channel is closed.
fn emit(state: &mut MergeState) -> bool {
    if state.closed {
        return false;
    }
    let merged = merge(&state.accounts);
    let total = total_across(&state.accounts);
    let batch = InboxBatch {
        ops: vec![InboxOp::Reset { rooms: merged }],
        total,
    };
    if !(state.sink)(batch) {
        state.closed = true;
        tracing::info!("inbox channel closed; stopping merged emissions");
        return false;
    }
    true
}

/// Sum the per-account known totals. `None` when no account has a known total.
fn total_across(accounts: &HashMap<String, AccountSlot>) -> Option<u32> {
    let mut any = false;
    let mut sum: u32 = 0;
    for slot in accounts.values() {
        if let Some(t) = slot.total {
            any = true;
            sum = sum.saturating_add(t);
        }
    }
    if any {
        Some(sum)
    } else {
        None
    }
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
        }
    }

    fn slot(hue: u8, rooms: Vec<RoomVm>) -> AccountSlot {
        AccountSlot {
            hue_index: hue,
            rooms,
            total: None,
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
        };
        let inbox_room = to_inbox_room("acctA", 4, &src);
        assert!(inbox_room.is_unread);
        assert_eq!(inbox_room.mention_count, 3);
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

    #[tokio::test]
    async fn merger_emits_reset_on_add_batch_and_remove() {
        let captured: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink_store = captured.clone();
        let merger = InboxMerger::new(Box::new(move |batch: InboxBatch| {
            sink_store.lock().expect("lock").push(batch);
            true
        }));

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
            let batches = captured.lock().expect("lock");
            let last = batches.last().expect("a batch");
            assert_eq!(last.total, Some(3), "totals sum across accounts");
            match &last.ops[0] {
                InboxOp::Reset { rooms } => {
                    let ids: Vec<&str> = rooms.iter().map(|r| r.room_id.as_str()).collect();
                    // b1 (400), a2 (300), a1 (100) recency desc across accounts.
                    assert_eq!(ids, ["!b1", "!a2", "!a1"]);
                }
                other => panic!("expected Reset, got {other:?}"),
            }
        }

        // Signing account B out removes its rooms from the merged inbox; A stays.
        merger.remove_account("acctB").await;
        {
            let batches = captured.lock().expect("lock");
            let last = batches.last().expect("a batch");
            match &last.ops[0] {
                InboxOp::Reset { rooms } => {
                    let ids: Vec<&str> = rooms.iter().map(|r| r.room_id.as_str()).collect();
                    assert_eq!(ids, ["!a2", "!a1"], "only account A's rooms remain");
                }
                other => panic!("expected Reset, got {other:?}"),
            }
        }
    }
}

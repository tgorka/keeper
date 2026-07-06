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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::badge::{self, BadgeConfig};
use crate::platform::Platform;
use crate::vm::{
    InboxBatch, InboxOp, InboxRoomVm, NetworkVm, NetworksSnapshot, RoomListBatch, RoomVm, SpaceVm,
    SpacesSnapshot,
};

/// Sink that receives each produced [`InboxBatch`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if the batch was
/// delivered, `false` if the channel is closed (the merger then stops emitting).
pub type InboxSink = Box<dyn Fn(InboxBatch) -> bool + Send + Sync>;

/// Sink that receives each produced [`SpacesSnapshot`] (Story 4.5). The shell
/// wraps a Tauri `Channel::send`; tests capture into a vector. Returns `true` if
/// the snapshot was delivered, `false` if the channel is closed. Analogous to
/// [`InboxSink`] but carries the whole Space list (no diff protocol — Spaces are
/// few, so the frontend replaces its list wholesale).
pub type SpacesSink = Box<dyn Fn(SpacesSnapshot) -> bool + Send + Sync>;

/// Sink that receives each produced [`NetworksSnapshot`] (Story 4.6). The shell
/// wraps a Tauri `Channel::send`; tests capture into a vector. Returns `true` if
/// the snapshot was delivered, `false` if the channel is closed. Analogous to
/// [`SpacesSink`] but carries the whole distinct-Networks list, derived from the
/// unfiltered merged set on each `emit` (no producer — the Networks list falls out
/// of the merge).
pub type NetworksSink = Box<dyn Fn(NetworksSnapshot) -> bool + Send + Sync>;

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
    /// Receives the whole aggregated Space list as a [`SpacesSnapshot`] (Story
    /// 4.5). Pushed on subscribe, on every [`InboxMerger::update_spaces`], and on
    /// [`InboxMerger::remove_account`].
    spaces_sink: SpacesSink,
    /// Each account's joined Spaces (Story 4.5), keyed by account id. Replaced
    /// whole per account by the spaces producer via [`InboxMerger::update_spaces`].
    /// Flattened in stable account-id order into the streamed [`SpacesSnapshot`].
    account_spaces: HashMap<String, Vec<SpaceVm>>,
    /// Each Space's joined child room ids (Story 4.5), keyed by
    /// `(account_id, space_id)`. Computed locally from `m.space.child` state
    /// cross-referenced against the account's joined rooms. When a Space is
    /// selected, `emit` retains only rooms whose `(account_id, room_id)` is in the
    /// selected Space's set.
    space_children: HashMap<(String, String), HashSet<String>>,
    /// The active Space filter, identified by `(account_id, space_id)`, or `None`
    /// for the unfiltered inbox (Story 4.5). Ephemeral view state (no persistence).
    /// Set/cleared out-of-band by [`InboxMerger::set_space_filter`].
    selected_space: Option<(String, String)>,
    /// Receives the whole distinct-Networks list as a [`NetworksSnapshot`] (Story
    /// 4.6). Derived in [`emit`] from the *unfiltered* merged set (distinct non-`None`
    /// `network`, deduped by name, name-sorted) and pushed on every emit, so it stays
    /// live with sync and stable regardless of the active Space/Network filter.
    networks_sink: NetworksSink,
    /// The active Network filter, identified by Network name (cross-account), or
    /// `None` for the unfiltered inbox (Story 4.6). Ephemeral view state (no
    /// persistence). Set/cleared out-of-band by [`InboxMerger::set_network_filter`].
    /// Composes AND with `selected_space` (Network retain runs after the Space retain).
    selected_network: Option<String>,
    /// The OS dock-badge sink (Story 10.3, FR-53). On every merged-state change the
    /// merger computes the cross-account `(unread_rooms, mention_total)` aggregate over
    /// the *full* unfiltered room set and pushes the badge through this port so it stays
    /// correct while the window is hidden (never computed in the webview). Shared with
    /// the shell as the same `Arc<dyn Platform>` the account producers use.
    platform: Arc<dyn Platform>,
    /// The app-wide dock-badge mode (Story 10.3): read on every merged-state change to
    /// decide what the aggregate badges (all unreads / mentions / off). Shared with the
    /// [`AccountManager`](crate::account::AccountManager); the Settings command mutates it
    /// live and can re-poke the merger to reapply.
    badge: Arc<BadgeConfig>,
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
    // The four inbox windows, the spaces/networks snapshots, the pin seed, and the
    // platform + badge config each cross a distinct concern; grouping them into a struct
    // would only obscure the one-to-one wiring the shell threads through.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inbox_sink: InboxSink,
        archive_sink: InboxSink,
        pins_sink: InboxSink,
        favourites_sink: InboxSink,
        pins: HashMap<(String, String), i64>,
        spaces_sink: SpacesSink,
        networks_sink: NetworksSink,
        platform: Arc<dyn Platform>,
        badge: Arc<BadgeConfig>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MergeState {
                accounts: HashMap::new(),
                pins_sink,
                favourites_sink,
                inbox_sink,
                archive_sink,
                pins,
                spaces_sink,
                account_spaces: HashMap::new(),
                space_children: HashMap::new(),
                selected_space: None,
                networks_sink,
                selected_network: None,
                platform,
                badge,
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
        let had_account = state.accounts.remove(account_id).is_some();
        // Drop the account's Spaces + child memberships (Story 4.5), and clear the
        // Space filter if it was owned by the removed account so the inbox returns
        // to the full unfiltered windows.
        let had_spaces = state.account_spaces.remove(account_id).is_some();
        state
            .space_children
            .retain(|(acct, _), _| acct != account_id);
        if state
            .selected_space
            .as_ref()
            .is_some_and(|(acct, _)| acct == account_id)
        {
            state.selected_space = None;
        }
        if had_spaces {
            emit_spaces(&mut state);
        }
        if had_account || had_spaces {
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

    /// Replace one account's Space list and child-membership map, then re-emit the
    /// aggregated Space snapshot and all four inbox windows (Story 4.5). Called by
    /// the per-account spaces producer on every sync batch (mirrors
    /// [`Self::update_pins`]'s "poke the live merger, re-emit" shape). `spaces` is
    /// that account's joined Spaces; `memberships` maps each Space's room id to its
    /// set of joined child room ids. Both are keyed only within the one account —
    /// this replaces exactly that account's entries and leaves the others intact.
    pub async fn update_spaces(
        &self,
        account_id: &str,
        spaces: Vec<SpaceVm>,
        memberships: HashMap<String, HashSet<String>>,
    ) {
        let mut state = self.inner.lock().await;
        // Replace the account's Space list.
        if spaces.is_empty() {
            state.account_spaces.remove(account_id);
        } else {
            state.account_spaces.insert(account_id.to_owned(), spaces);
        }
        // Replace the account's child-membership entries: drop all prior
        // `(account_id, *)` keys, then insert the fresh ones.
        state
            .space_children
            .retain(|(acct, _), _| acct != account_id);
        for (space_id, children) in memberships {
            state
                .space_children
                .insert((account_id.to_owned(), space_id), children);
        }
        emit_spaces(&mut state);
        // The four inbox windows only depend on Space data when a filter is
        // active; when unfiltered, a Space recompute (which fires on every sync
        // `RoomUpdates`) cannot change them, so skip the redundant window re-emit
        // and avoid doubling inbox emissions per sync tick.
        if state.selected_space.is_some() {
            emit(&mut state);
        }
    }

    /// Set (or clear) the active Space filter and re-emit all four windows (Story
    /// 4.5). `Some((account_id, space_id))` narrows every window to that Space's
    /// joined children; `None` restores the full unfiltered inbox. Ephemeral — no
    /// persistence. Best-effort re-emit as in [`Self::update_pins`]: a no-active-
    /// subscription case is a harmless no-op.
    pub async fn set_space_filter(&self, selection: Option<(String, String)>) {
        let mut state = self.inner.lock().await;
        state.selected_space = selection;
        emit(&mut state);
    }

    /// Set (or clear) the active Network filter and re-emit all four windows (Story
    /// 4.6). `Some(name)` narrows every window to rooms bridged to that Network
    /// (cross-account — the selection is name-keyed); `None` restores the full
    /// inbox. Composes AND with any active Space filter (the Network retain runs
    /// after the Space retain in [`emit`]). Ephemeral — no persistence. Best-effort
    /// re-emit as in [`Self::set_space_filter`]: a no-active-subscription case is a
    /// harmless no-op.
    pub async fn set_network_filter(&self, network: Option<String>) {
        let mut state = self.inner.lock().await;
        state.selected_network = network;
        emit(&mut state);
    }

    /// Recompute and reapply the dock badge from the current full merged state (Story
    /// 10.3), without re-emitting the inbox windows. Called out-of-band after the
    /// dock-badge *mode* changes so the badge reflects the new mode immediately even when
    /// no room activity is flowing. Best-effort — a closed subscription or unset platform
    /// handle is a harmless no-op.
    pub async fn reapply_badge(&self) {
        let state = self.inner.lock().await;
        let merged = merge(&state.accounts);
        let unread_rooms = merged.iter().filter(|room| room.is_unread).count() as u32;
        let mention_total: u32 = merged.iter().map(|room| room.mention_count).sum();
        badge::apply(
            &*state.platform,
            state.badge.mode(),
            unread_rooms,
            mention_total,
        );
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
    // `merge` already drops `is_space` rooms (containers, never chats) so they
    // never appear in any of the four windows (Story 4.5).
    let mut merged = merge(&state.accounts);
    // Derive the distinct-Networks list from the *unfiltered* merged set (Story
    // 4.6), BEFORE any Space/Network retain, so the NETWORKS sidebar list stays
    // complete and stable regardless of the active filter (it is what the user can
    // filter *to*, not what is currently shown). Distinct non-`None` `network`,
    // deduped by name, name-sorted; native rooms (`None`) excluded. Snapshot is
    // pushed LAST (see end of `emit`).
    let network_names = distinct_network_names(&merged);
    // Compute the dock-badge aggregate from the *full* unfiltered merged set (Story
    // 10.3, FR-53), BEFORE any Space/Network retain, so the badge reflects every account
    // and every room regardless of the active view filter and stays correct while the
    // window is hidden. `unread_rooms` counts rooms with `is_unread`; `mention_total`
    // sums `mention_count`. This reads the same `is_unread`/`mention_count` the windows
    // render — it never alters the unread computation. The badge is pushed through the
    // `Platform` port, which is an honest no-op when the app handle is unset (tests).
    let unread_rooms = merged.iter().filter(|room| room.is_unread).count() as u32;
    let mention_total: u32 = merged.iter().map(|room| room.mention_count).sum();
    badge::apply(
        &*state.platform,
        state.badge.mode(),
        unread_rooms,
        mention_total,
    );
    // Self-heal the ephemeral Network selection: if the selected Network is no
    // longer present anywhere in the merged set (its last bridged room left, or an
    // owner account signed out), clear it. This keeps filter validity
    // Rust-authoritative (AD-20) — the merger owns the selection just as it owns the
    // distinct-Networks set — mirroring the `selected_space` cleanup in
    // `remove_account`, and prevents an indefinitely-empty inbox on a dead
    // selection. The Network retain below reads the (possibly cleared) selection.
    if let Some(sel) = &state.selected_network {
        if !network_names.iter().any(|name| name == sel) {
            state.selected_network = None;
        }
    }
    // Apply the ephemeral Space filter *before* the pins/favorites/inbox/archive
    // partition (Story 4.5), so precedence (Pins > Favorites > Archive/Inbox) and
    // per-window recency order are preserved within the filtered subset and each
    // window `total` reflects the filtered count. When a Space is selected, retain
    // only rooms owned by that account whose room id is in the Space's joined
    // children; an unknown selection (e.g. the account went away mid-flight) yields
    // an empty set, which correctly empties every window.
    if let Some((sel_account, sel_space)) = &state.selected_space {
        let empty = HashSet::new();
        let children = state
            .space_children
            .get(&(sel_account.clone(), sel_space.clone()))
            .unwrap_or(&empty);
        merged.retain(|room| &room.account_id == sel_account && children.contains(&room.room_id));
    }
    // Apply the ephemeral Network filter immediately AFTER the Space retain (Story
    // 4.6) so the two compose as AND: both narrow the same pre-partition merged set,
    // and precedence (Pins > Favorites > Archive/Inbox) and per-window recency are
    // preserved within the intersection. Name-keyed (cross-account); a native room
    // (`network == None`) never matches a selected Network. An unknown/empty match
    // yields an empty set, correctly emptying every window.
    if let Some(selected) = &state.selected_network {
        merged.retain(|room| room.network.as_deref() == Some(selected.as_str()));
    }
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
    // Push the distinct-Networks snapshot LAST — after the four windows are emitted
    // — so a closed networks channel can never suppress a window tick (the windows
    // are the primary surface; the Networks sidebar list is secondary). Returns the
    // live/closed state so a networks-channel close still stops future emissions.
    push_networks(network_names, &state.networks_sink, &mut state.closed);
    !state.closed
}

/// Emit the aggregated Space list as one whole [`SpacesSnapshot`] into the spaces
/// sink (Story 4.5). Flattens `account_spaces` in stable account-id order (so the
/// frontend's list is deterministic regardless of `HashMap` iteration order); each
/// account's Spaces keep their producer order. Records channel closure like
/// [`emit`]. No diff protocol — the snapshot replaces the frontend's list.
fn emit_spaces(state: &mut MergeState) -> bool {
    if state.closed {
        return false;
    }
    let mut ids: Vec<&String> = state.account_spaces.keys().collect();
    ids.sort();
    let mut spaces: Vec<SpaceVm> = Vec::new();
    for id in ids {
        spaces.extend(state.account_spaces[id].iter().cloned());
    }
    let snapshot = SpacesSnapshot { spaces };
    if !(state.spaces_sink)(snapshot) {
        state.closed = true;
        tracing::info!("spaces channel closed; stopping merged emissions");
        return false;
    }
    true
}

/// The authoritative distinct-Networks set derived from the *unfiltered* merged set
/// (Story 4.6): each row's non-`None` `network` (native rooms excluded), deduped by
/// name and name-sorted, returned as an owned `Vec<String>`. The merger both streams
/// this set (via [`push_networks`]) and validates `selected_network` against it (the
/// self-heal in [`emit`]), so a filter can never survive its Network's disappearance.
/// Two bridges exposing the same protocol displayname collapse into one name-keyed
/// Network BY DESIGN — the label is the cross-account identity key.
fn distinct_network_names(merged: &[InboxRoomVm]) -> Vec<String> {
    let mut names: Vec<String> = merged
        .iter()
        .filter_map(|room| room.network.clone())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Push the given distinct-Networks list as one whole [`NetworksSnapshot`] into the
/// networks sink (Story 4.6). No diff protocol — the snapshot replaces the
/// frontend's list. Records channel closure into `closed` (setting it + tracing on
/// close) like [`emit`].
fn push_networks(names: Vec<String>, sink: &NetworksSink, closed: &mut bool) {
    if *closed {
        return;
    }
    let networks = names.into_iter().map(|name| NetworkVm { name }).collect();
    let snapshot = NetworksSnapshot { networks };
    if !sink(snapshot) {
        *closed = true;
        tracing::info!("networks channel closed; stopping merged emissions");
    }
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
            // Space rooms are containers, not chats — never in any chat window
            // (Story 4.5). `is_space` lives only on `RoomVm`, so drop them here
            // before projecting to `InboxRoomVm`.
            if room.is_space {
                continue;
            }
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
        // The bridged-Network label is SDK/bridge-sourced (from local `m.bridge`
        // state, resolved on the `RoomVm`), copied straight through (Story 4.6).
        network: room.network.clone(),
        // The stable bridge `network_id` (machine `protocol.id`) is likewise resolved
        // on the `RoomVm`, copied straight through (Story 6.5) — the health join key.
        network_id: room.network_id.clone(),
        // The durable per-Chat / per-Network mute intent is resolved on the `RoomVm`
        // at projection time (Story 10.2), copied straight through — it drives the row
        // mute glyph and never gates the unread computation above.
        mute_state: room.mute_state,
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
    use crate::error::CoreError;
    use crate::vm::{DockBadgeMode, RoomListOp};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicI64, Ordering as AtomicOrdering};
    use std::sync::Mutex as StdMutex;

    /// A no-op [`Platform`] that records the last dock-badge count set, so badge
    /// assertions run without a live `AppHandle` (Story 10.3). All other ports are
    /// honest `Unsupported` — the merger only uses `set_badge_count`.
    #[derive(Default)]
    struct BadgeRecordingPlatform {
        // -1 encodes "cleared" (`None`); a non-negative value is `Some(n)`.
        last: AtomicI64,
    }

    impl Platform for BadgeRecordingPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("no data dir in test".to_owned()))
        }
        fn keychain_set(&self, _key: &str, _value: &str) -> Result<(), CoreError> {
            Err(CoreError::Unsupported("keychain".to_owned()))
        }
        fn keychain_get(&self, _key: &str) -> Result<Option<String>, CoreError> {
            Err(CoreError::Unsupported("keychain".to_owned()))
        }
        fn keychain_delete(&self, _key: &str) -> Result<(), CoreError> {
            Err(CoreError::Unsupported("keychain".to_owned()))
        }
        fn open_url(&self, _url: &str) -> Result<(), CoreError> {
            Err(CoreError::Unsupported("open_url".to_owned()))
        }
        fn notify(&self, _title: &str, _body: &str) -> Result<(), CoreError> {
            Err(CoreError::Unsupported("notify".to_owned()))
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar".to_owned()))
        }
        fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError> {
            self.last
                .store(count.map(i64::from).unwrap_or(-1), AtomicOrdering::Relaxed);
            Ok(())
        }
    }

    impl BadgeRecordingPlatform {
        /// The last badge count set, decoding `-1` back to `None`.
        fn last_badge(&self) -> Option<u32> {
            match self.last.load(AtomicOrdering::Relaxed) {
                -1 => None,
                n => Some(n as u32),
            }
        }
    }

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
            is_space: false,
            network: None,
            network_id: None,
            mute_state: crate::vm::MuteState::None,
        }
    }

    /// A room bridged to `network` for Network filter/snapshot tests (Story 4.6).
    fn room_net(id: &str, ts: Option<i64>, network: &str) -> RoomVm {
        RoomVm {
            network: Some(network.to_owned()),
            ..room(id, ts)
        }
    }

    /// A Space room (`is_space`) for exclusion tests (Story 4.5).
    fn room_space(id: &str, ts: Option<i64>) -> RoomVm {
        RoomVm {
            is_space: true,
            ..room(id, ts)
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
            is_space: false,
            network: Some("Telegram".to_owned()),
            network_id: Some("telegram".to_owned()),
            mute_state: crate::vm::MuteState::Muted,
        };
        let inbox_room = to_inbox_room("acctA", 4, &src);
        assert!(inbox_room.is_unread);
        // The durable mute intent is copied straight through (Story 10.2).
        assert_eq!(inbox_room.mute_state, crate::vm::MuteState::Muted);
        assert_eq!(inbox_room.mention_count, 3);
        assert!(inbox_room.is_archived);
        // `is_favourite` is SDK-sourced, copied straight through like `is_archived`.
        assert!(inbox_room.is_favourite);
        // `to_inbox_room` defaults unpinned; `emit` stamps the pin flag.
        assert!(!inbox_room.is_pinned);
        // The bridged-Network label is copied straight through (Story 4.6).
        assert_eq!(inbox_room.network.as_deref(), Some("Telegram"));
        // The stable bridge network_id is copied straight through (Story 6.5).
        assert_eq!(inbox_room.network_id.as_deref(), Some("telegram"));
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

    /// Shared capture buffer for the spaces sink's emitted snapshots.
    type CapturedSpaces = Arc<StdMutex<Vec<SpacesSnapshot>>>;

    /// Shared capture buffer for the networks sink's emitted snapshots (Story 4.6).
    type CapturedNetworks = Arc<StdMutex<Vec<NetworksSnapshot>>>;

    /// The six capture handles a [`capturing_merger`] returns, alongside the merger:
    /// `(merger, inbox, archive, pins, favourites, spaces, networks)`.
    type CapturingMerger = (
        InboxMerger,
        Captured,
        Captured,
        Captured,
        Captured,
        CapturedSpaces,
        CapturedNetworks,
    );

    /// Build a merger over six capture vecs (inbox, archive, pins, favourites,
    /// spaces, networks) so partition tests can assert each window independently.
    /// Seeds an empty pin map; tests that exercise pins push a map via
    /// [`InboxMerger::update_pins`].
    fn capturing_merger() -> CapturingMerger {
        capturing_merger_with_pins(HashMap::new())
    }

    /// Like [`capturing_merger`] but seeds the merger's pin map. Returns captures
    /// in `(merger, inbox, archive, pins, favourites, spaces, networks)` order.
    /// The dock badge is wired to a discarded no-op platform in mode `All`; badge-
    /// asserting tests use [`badge_merger`] instead to hold the recording handle.
    fn capturing_merger_with_pins(pins: HashMap<(String, String), i64>) -> CapturingMerger {
        let (merger, inbox, archive, pins_cap, favourites, spaces, networks, _platform) =
            capturing_merger_with_pins_and_badge(pins, DockBadgeMode::All);
        (
            merger, inbox, archive, pins_cap, favourites, spaces, networks,
        )
    }

    /// Build a merger plus a [`BadgeRecordingPlatform`] in the given badge `mode`, so a
    /// test can assert the OS dock badge the merger pushes on each merged-state change
    /// (Story 10.3). Returns `(merger, recording_platform)`.
    fn badge_merger(mode: DockBadgeMode) -> (InboxMerger, Arc<BadgeRecordingPlatform>) {
        let (merger, _inbox, _archive, _pins, _favourites, _spaces, _networks, platform) =
            capturing_merger_with_pins_and_badge(HashMap::new(), mode);
        (merger, platform)
    }

    /// The [`CapturingMerger`] captures plus the [`BadgeRecordingPlatform`] the merger
    /// pushes dock badges through.
    type CapturingMergerWithBadge = (
        InboxMerger,
        Captured,
        Captured,
        Captured,
        Captured,
        CapturedSpaces,
        CapturedNetworks,
        Arc<BadgeRecordingPlatform>,
    );

    /// The shared builder behind [`capturing_merger_with_pins`] and [`badge_merger`]:
    /// seeds the pin map and the dock-badge `mode`, returning every capture handle plus
    /// the recording platform.
    fn capturing_merger_with_pins_and_badge(
        pins: HashMap<(String, String), i64>,
        mode: DockBadgeMode,
    ) -> CapturingMergerWithBadge {
        let inbox: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let archive: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let pins_cap: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let favourites: Arc<StdMutex<Vec<InboxBatch>>> = Arc::new(StdMutex::new(Vec::new()));
        let spaces: CapturedSpaces = Arc::new(StdMutex::new(Vec::new()));
        let networks: CapturedNetworks = Arc::new(StdMutex::new(Vec::new()));
        let inbox_store = inbox.clone();
        let archive_store = archive.clone();
        let pins_store = pins_cap.clone();
        let favourites_store = favourites.clone();
        let spaces_store = spaces.clone();
        let networks_store = networks.clone();
        let platform = Arc::new(BadgeRecordingPlatform::default());
        let platform_port: Arc<dyn Platform> = platform.clone();
        let badge = Arc::new(BadgeConfig::new(mode));
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
            Box::new(move |snapshot: SpacesSnapshot| {
                spaces_store.lock().expect("lock").push(snapshot);
                true
            }),
            Box::new(move |snapshot: NetworksSnapshot| {
                networks_store.lock().expect("lock").push(snapshot);
                true
            }),
            platform_port,
            badge,
        );
        (
            merger, inbox, archive, pins_cap, favourites, spaces, networks, platform,
        )
    }

    /// Network names of the last captured [`NetworksSnapshot`] in `store`, or a panic.
    fn last_networks_names(store: &CapturedNetworks) -> Vec<String> {
        let snapshots = store.lock().expect("lock");
        let last = snapshots.last().expect("a snapshot");
        last.networks.iter().map(|n| n.name.clone()).collect()
    }

    /// Space ids of the last captured [`SpacesSnapshot`] in `store`, or a panic.
    fn last_spaces_ids(store: &CapturedSpaces) -> Vec<String> {
        let snapshots = store.lock().expect("lock");
        let last = snapshots.last().expect("a snapshot");
        last.spaces.iter().map(|s| s.space_id.clone()).collect()
    }

    /// A [`SpaceVm`] fixture on `account_id`.
    fn space_vm(account_id: &str, space_id: &str) -> SpaceVm {
        SpaceVm {
            account_id: account_id.to_owned(),
            space_id: space_id.to_owned(),
            name: space_id.to_owned(),
            avatar_url: None,
        }
    }

    /// Build a child-membership map from `(space_id, [room_ids])` pairs.
    fn membership(entries: &[(&str, &[&str])]) -> HashMap<String, HashSet<String>> {
        entries
            .iter()
            .map(|(space, rooms)| {
                (
                    space.to_string(),
                    rooms.iter().map(|r| r.to_string()).collect(),
                )
            })
            .collect()
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
        let (merger, inbox, _archive, _pins, _favourites, _spaces, _networks) = capturing_merger();

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
        let (merger, inbox, archive, _pins, _favourites, _spaces, _networks) = capturing_merger();
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
        let (merger, inbox, archive, pins_cap, favourites, _spaces, _networks) =
            capturing_merger_with_pins(pins);
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
        let (merger, inbox, archive, _pins, favourites, _spaces, _networks) = capturing_merger();
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
        let (merger, inbox, _archive, _pins, favourites, _spaces, _networks) = capturing_merger();
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
        let (merger, inbox, archive, pins_cap, _favourites, _spaces, _networks) =
            capturing_merger_with_pins(pins);
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
        let (merger, _inbox, _archive, pins_cap, _favourites, _spaces, _networks) =
            capturing_merger_with_pins(pins);
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
        let (merger, inbox, _archive, pins_cap, _favourites, _spaces, _networks) =
            capturing_merger_with_pins(pins);
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
        let (merger, inbox, archive, pins_cap, _favourites, _spaces, _networks) =
            capturing_merger();
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

    #[tokio::test]
    async fn is_space_rooms_are_excluded_from_all_windows() {
        // A Space room (`is_space`) is a container, never a chat — it must not
        // appear in Inbox/Archive/Pins/Favorites, even if pinned/favourited/archived.
        let pins = pin_map(&[("acctA", "!space", 0)]);
        let (merger, inbox, archive, pins_cap, favourites, _spaces, _networks) =
            capturing_merger_with_pins(pins);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_space("!space", Some(500)), // a Space, even pinned
                            room_flags("!chat", Some(400), false, false),
                            // A favourited Space room: still excluded (is_space wins).
                            RoomVm {
                                is_space: true,
                                ..room_fav("!favspace", Some(300), false, false)
                            },
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        // Only the plain chat survives, in the Inbox window.
        assert_eq!(last_reset_ids(&inbox), ["!chat"]);
        assert_eq!(last_reset_ids(&archive), Vec::<String>::new());
        assert_eq!(last_reset_ids(&pins_cap), Vec::<String>::new());
        assert_eq!(last_reset_ids(&favourites), Vec::<String>::new());
    }

    #[tokio::test]
    async fn update_spaces_emits_snapshot_in_account_id_order() {
        // Two accounts each contribute a Space; the aggregated snapshot flattens in
        // stable account-id order.
        let (merger, _inbox, _archive, _pins, _favourites, spaces, _networks) = capturing_merger();
        merger.register_account("acctB", 1).await;
        merger.register_account("acctA", 0).await;
        merger
            .update_spaces(
                "acctB",
                vec![space_vm("acctB", "!sb")],
                membership(&[("!sb", &["!b1"])]),
            )
            .await;
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!sa")],
                membership(&[("!sa", &["!a1"])]),
            )
            .await;
        // acctA sorts before acctB regardless of update order.
        assert_eq!(last_spaces_ids(&spaces), ["!sa", "!sb"]);
    }

    #[tokio::test]
    async fn selected_space_filters_all_four_windows_preserving_precedence() {
        // Golden filter case: a Space `!s` on acctA contains {!a (pin+fav), !b (fav),
        // !c (archived-read), !d (unread)}. `!e` is NOT in the Space; `!x` is on
        // another account. With the filter set, only `!s`'s members appear, and the
        // four-way partition + precedence is preserved within the subset.
        let pins = pin_map(&[("acctA", "!a", 0)]);
        let (merger, inbox, archive, pins_cap, favourites, _spaces, _networks) =
            capturing_merger_with_pins(pins);
        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_fav("!a", Some(600), false, false), // pin + fav, in Space
                            room_fav("!b", Some(500), false, false), // fav, in Space
                            room_flags("!c", Some(400), true, false), // archived-read, in Space
                            room_flags("!d", Some(300), false, true), // unread, in Space
                            room_flags("!e", Some(200), false, false), // NOT in Space
                        ],
                    }],
                    total: Some(5),
                },
            )
            .await;
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_flags("!x", Some(700), false, false)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!s")],
                membership(&[("!s", &["!a", "!b", "!c", "!d"])]),
            )
            .await;

        // Before filtering: !x (acctB) and !e are present.
        assert_eq!(last_reset_ids(&inbox), ["!x", "!d", "!e"]);

        merger
            .set_space_filter(Some(("acctA".to_owned(), "!s".to_owned())))
            .await;
        // Pins: [!a] (pinned wins). Favorites: [!b]. Inbox: [!d] (unread). Archive: [!c].
        // !e (not in Space) and !x (other account) are gone from every window.
        assert_eq!(last_reset_ids(&pins_cap), ["!a"]);
        assert_eq!(last_reset_ids(&favourites), ["!b"]);
        assert_eq!(last_reset_ids(&inbox), ["!d"]);
        assert_eq!(last_reset_ids(&archive), ["!c"]);
        {
            let batches = inbox.lock().expect("lock");
            assert_eq!(
                batches.last().expect("batch").total,
                Some(1),
                "inbox total is the filtered count"
            );
        }

        // Clearing restores the full unfiltered windows.
        merger.set_space_filter(None).await;
        assert_eq!(last_reset_ids(&inbox), ["!x", "!d", "!e"]);
    }

    #[tokio::test]
    async fn selected_space_with_no_members_empties_windows() {
        // A selected Space whose membership is empty yields empty windows.
        let (merger, inbox, _archive, _pins, _favourites, _spaces, _networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_flags("!a", Some(100), false, false)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!empty")],
                membership(&[("!empty", &[])]),
            )
            .await;
        merger
            .set_space_filter(Some(("acctA".to_owned(), "!empty".to_owned())))
            .await;
        assert_eq!(last_reset_ids(&inbox), Vec::<String>::new());
    }

    #[tokio::test]
    async fn remove_account_clears_a_selection_it_owned() {
        // Signing out the account that owns the active Space filter clears the
        // filter, drops its Spaces from the snapshot, and re-emits full windows.
        let (merger, inbox, _archive, _pins, _favourites, spaces, _networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_flags("!b1", Some(200), false, false)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_flags("!a1", Some(100), false, false)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!s")],
                membership(&[("!s", &["!a1"])]),
            )
            .await;
        merger
            .set_space_filter(Some(("acctA".to_owned(), "!s".to_owned())))
            .await;
        // Filtered to acctA's Space: only !a1.
        assert_eq!(last_reset_ids(&inbox), ["!a1"]);
        assert_eq!(last_spaces_ids(&spaces), ["!s"]);

        merger.remove_account("acctA").await;
        // Filter cleared (its owner is gone); acctB's room is back, acctA gone.
        assert_eq!(last_reset_ids(&inbox), ["!b1"]);
        // The Space snapshot no longer lists acctA's Space.
        assert_eq!(last_spaces_ids(&spaces), Vec::<String>::new());
    }

    #[tokio::test]
    async fn update_spaces_without_active_filter_skips_window_re_emit() {
        // With no Space filter active, a Space recompute (which fires on every sync
        // `RoomUpdates`) must refresh only the spaces snapshot — the four inbox
        // windows cannot change, so they are NOT re-emitted (avoids doubling inbox
        // emissions per sync tick).
        let (merger, inbox, archive, pins_cap, favourites, spaces, _networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_flags("!a", Some(100), false, false)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        let inbox_before = inbox.lock().expect("lock").len();
        let archive_before = archive.lock().expect("lock").len();
        let pins_before = pins_cap.lock().expect("lock").len();
        let fav_before = favourites.lock().expect("lock").len();
        let spaces_before = spaces.lock().expect("lock").len();

        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!s")],
                membership(&[("!s", &["!a"])]),
            )
            .await;

        // Spaces snapshot advanced; the four windows did not.
        assert_eq!(spaces.lock().expect("lock").len(), spaces_before + 1);
        assert_eq!(inbox.lock().expect("lock").len(), inbox_before);
        assert_eq!(archive.lock().expect("lock").len(), archive_before);
        assert_eq!(pins_cap.lock().expect("lock").len(), pins_before);
        assert_eq!(favourites.lock().expect("lock").len(), fav_before);

        // Once a filter is active, a subsequent recompute DOES re-emit the windows
        // (membership changes can move rows in/out of the filtered view).
        merger
            .set_space_filter(Some(("acctA".to_owned(), "!s".to_owned())))
            .await;
        let inbox_after_filter = inbox.lock().expect("lock").len();
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!s")],
                membership(&[("!s", &["!a"])]),
            )
            .await;
        assert_eq!(inbox.lock().expect("lock").len(), inbox_after_filter + 1);
    }

    #[tokio::test]
    async fn networks_snapshot_dedups_sorts_and_excludes_native() {
        // The distinct-Networks list is derived from the unfiltered merged set:
        // Telegram appears on two accounts (deduped by name), Signal once, and the
        // native room (`network == None`) is excluded. The result is name-sorted.
        let (merger, _inbox, _archive, _pins, _favourites, _spaces, networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_net("!a1", Some(400), "Telegram"),
                            room_net("!a2", Some(300), "Signal"),
                            room("!native", Some(200)), // native: excluded
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_net("!b1", Some(500), "Telegram")], // dup name
                    }],
                    total: Some(1),
                },
            )
            .await;
        // Deduped by name across accounts, name-sorted, native excluded.
        assert_eq!(last_networks_names(&networks), ["Signal", "Telegram"]);
    }

    #[tokio::test]
    async fn selected_network_retains_across_accounts() {
        // Selecting a Network retains only rooms bridged to it, across all accounts;
        // native rooms and other-Network rooms leave every window. The list stays
        // complete (derived pre-filter) regardless of the active filter.
        let (merger, inbox, _archive, _pins, _favourites, _spaces, networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_net("!a1", Some(400), "Telegram"),
                            room_net("!a2", Some(300), "Signal"),
                            room("!native", Some(250)),
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_net("!b1", Some(500), "Telegram")],
                    }],
                    total: Some(1),
                },
            )
            .await;
        // Unfiltered: all non-space rooms present, recency desc.
        assert_eq!(last_reset_ids(&inbox), ["!b1", "!a1", "!a2", "!native"]);

        merger.set_network_filter(Some("Telegram".to_owned())).await;
        // Only Telegram rooms across both accounts, recency order.
        assert_eq!(last_reset_ids(&inbox), ["!b1", "!a1"]);
        // The Networks list is unchanged by the active filter (derived pre-retain).
        assert_eq!(last_networks_names(&networks), ["Signal", "Telegram"]);

        // Clearing restores the full inbox.
        merger.set_network_filter(None).await;
        assert_eq!(last_reset_ids(&inbox), ["!b1", "!a1", "!a2", "!native"]);
    }

    #[tokio::test]
    async fn network_and_space_filters_compose_as_and() {
        // With BOTH a Space filter and a Network filter active, the inbox shows their
        // AND intersection. Space `!s` on acctA = {!a (Telegram), !b (Signal)}; the
        // Network filter = Telegram. Only `!a` (in the Space AND on Telegram) survives.
        let (merger, inbox, _archive, _pins, _favourites, _spaces, _networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_net("!a", Some(400), "Telegram"), // in Space, Telegram
                            room_net("!b", Some(300), "Signal"),   // in Space, Signal
                            room_net("!c", Some(200), "Telegram"), // NOT in Space, Telegram
                        ],
                    }],
                    total: Some(3),
                },
            )
            .await;
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!s")],
                membership(&[("!s", &["!a", "!b"])]),
            )
            .await;
        merger
            .set_space_filter(Some(("acctA".to_owned(), "!s".to_owned())))
            .await;
        // Space alone: !a and !b.
        assert_eq!(last_reset_ids(&inbox), ["!a", "!b"]);

        merger.set_network_filter(Some("Telegram".to_owned())).await;
        // Space ∩ Telegram: only !a (!b is Signal, !c is outside the Space).
        assert_eq!(last_reset_ids(&inbox), ["!a"]);

        // Clearing the Network filter returns to the Space-only intersection.
        merger.set_network_filter(None).await;
        assert_eq!(last_reset_ids(&inbox), ["!a", "!b"]);
    }

    #[tokio::test]
    async fn selected_network_absent_from_set_self_heals_to_unfiltered() {
        // Selecting a Network no room is bridged to anywhere in the merged set
        // self-heals `selected_network` to `None` on the next `emit` (keeping filter
        // validity Rust-authoritative, AD-20), rather than emptying every window
        // indefinitely on a dead selection. The windows show the full unfiltered set.
        let (merger, inbox, _archive, _pins, _favourites, _spaces, _networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_net("!a", Some(200), "Telegram"),
                            room("!native", Some(100)),
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        // WhatsApp is absent from the merged set → self-heal to unfiltered.
        merger.set_network_filter(Some("WhatsApp".to_owned())).await;
        assert_eq!(last_reset_ids(&inbox), ["!a", "!native"]);
        {
            let batches = inbox.lock().expect("lock");
            assert_eq!(batches.last().expect("batch").total, Some(2));
        }
    }

    #[tokio::test]
    async fn network_space_empty_intersection_empties_windows_without_clearing() {
        // A genuine empty-intersection case for AND composition: the selected Network
        // IS present in the merged set AND a Space IS selected, but their intersection
        // is empty (the Space's only member is on a different Network). The windows are
        // empty, and the selection is NOT cleared — the Network still exists in the
        // merged set, so self-heal does not fire.
        let (merger, inbox, _archive, _pins, _favourites, _spaces, networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_net("!a", Some(300), "Signal"),   // in Space, Signal
                            room_net("!b", Some(200), "Telegram"), // NOT in Space, Telegram
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        merger
            .update_spaces(
                "acctA",
                vec![space_vm("acctA", "!s")],
                membership(&[("!s", &["!a"])]),
            )
            .await;
        merger
            .set_space_filter(Some(("acctA".to_owned(), "!s".to_owned())))
            .await;
        // Telegram exists in the set (so it will NOT self-heal), but the Space's only
        // member is Signal → empty intersection, empty windows.
        merger.set_network_filter(Some("Telegram".to_owned())).await;
        assert_eq!(last_reset_ids(&inbox), Vec::<String>::new());
        // The Networks list is unchanged by the active filter (derived pre-retain):
        // Telegram remains present, so the selection survives (no self-heal).
        assert_eq!(last_networks_names(&networks), ["Signal", "Telegram"]);

        // Clearing the Space filter reveals the Telegram room again — proving the
        // Network selection was retained, not self-healed away.
        merger.set_space_filter(None).await;
        assert_eq!(last_reset_ids(&inbox), ["!b"]);
    }

    #[tokio::test]
    async fn selected_network_self_heals_when_last_room_removed() {
        // When the last room bridged to the selected Network leaves (its owner account
        // signs out), the next `emit` self-heals `selected_network` to `None` and the
        // surviving rooms return unfiltered.
        let (merger, inbox, _archive, _pins, _favourites, _spaces, _networks) = capturing_merger();
        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_net("!a", Some(200), "Signal")],
                    }],
                    total: Some(1),
                },
            )
            .await;
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_net("!b", Some(100), "Telegram")],
                    }],
                    total: Some(1),
                },
            )
            .await;
        merger.set_network_filter(Some("Telegram".to_owned())).await;
        // Filtered to Telegram: only acctB's room.
        assert_eq!(last_reset_ids(&inbox), ["!b"]);

        // acctB (the only Telegram-bridged account) signs out. On the re-emit, the
        // Telegram Network is gone from the set → self-heal to unfiltered; acctA's
        // Signal room is shown (not empty windows).
        merger.remove_account("acctB").await;
        assert_eq!(last_reset_ids(&inbox), ["!a"]);
    }

    /// A room with an explicit unread flag + mention count for badge-aggregate tests.
    fn room_unread(id: &str, ts: Option<i64>, is_unread: bool, mentions: u32) -> RoomVm {
        RoomVm {
            is_unread,
            mention_count: mentions,
            ..room(id, ts)
        }
    }

    #[tokio::test]
    async fn badge_all_mode_counts_unread_rooms_across_accounts() {
        // `All` badges the count of unread rooms over the FULL cross-account set, not the
        // filtered/windowed view (Story 10.3).
        let (merger, platform) = badge_merger(DockBadgeMode::All);
        merger.register_account("acctA", 0).await;
        merger.register_account("acctB", 1).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_unread("!a1", Some(300), true, 2),
                            room_unread("!a2", Some(200), false, 0),
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        merger
            .apply_account_batch(
                "acctB",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_unread("!b1", Some(400), true, 5)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        // Two unread rooms across both accounts → badge shows 2.
        assert_eq!(platform.last_badge(), Some(2));
    }

    #[tokio::test]
    async fn badge_mentions_mode_sums_mention_counts() {
        let (merger, platform) = badge_merger(DockBadgeMode::Mentions);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![
                            room_unread("!a1", Some(300), true, 3),
                            room_unread("!a2", Some(200), true, 4),
                        ],
                    }],
                    total: Some(2),
                },
            )
            .await;
        // 3 + 4 = 7 mentions → badge shows 7 (unread-room count ignored in this mode).
        assert_eq!(platform.last_badge(), Some(7));
    }

    #[tokio::test]
    async fn badge_off_mode_clears_and_zero_clears() {
        // `Off` never badges even with unread state present.
        let (merger, platform) = badge_merger(DockBadgeMode::Off);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_unread("!a1", Some(300), true, 9)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        assert_eq!(platform.last_badge(), None);

        // In `All` mode, clearing the last unread clears the badge (never Some(0)).
        let (merger, platform) = badge_merger(DockBadgeMode::All);
        merger.register_account("acctA", 0).await;
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_unread("!a1", Some(300), true, 0)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        assert_eq!(platform.last_badge(), Some(1));
        // The room is read → the badge clears.
        merger
            .apply_account_batch(
                "acctA",
                RoomListBatch {
                    ops: vec![RoomListOp::Reset {
                        rooms: vec![room_unread("!a1", Some(300), false, 0)],
                    }],
                    total: Some(1),
                },
            )
            .await;
        assert_eq!(platform.last_badge(), None);
    }
}

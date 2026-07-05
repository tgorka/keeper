//! Integration coverage for the Story 5.6 archive-first pagination enablement.
//!
//! Story 5.6's entire enablement is a single call in `account::activate()`:
//! `client.event_cache().subscribe()` before `sync.start()`, so every synced
//! batch persists into the on-disk `SqliteEventCacheStore` that `.sqlite_store()`
//! provisions and `Timeline::paginate_backwards` serves older events from local
//! disk first (FR-17; SPINE persisted-event-cache storage rule).
//!
//! This asserts the invariant `activate()` relies on: a `Client` built on a temp
//! `.sqlite_store()` reports `event_cache().has_subscribed() == false` before
//! `subscribe()` and `== true` after, and a repeated `subscribe()` is a cheap
//! no-op that keeps `has_subscribed() == true` (the idempotency `activate()`
//! depends on, since `TimelineBuilder::build()` also calls `subscribe()` lazily).
//!
//! The client is built fully offline (no live homeserver): an unresolvable
//! `homeserver_url` never contacted at build time, plus a unique temp sqlite dir
//! per run.

use std::path::PathBuf;

use matrix_sdk::Client;

/// A unique temp SDK-store dir per test run (real SQLite files under the OS temp
/// dir). Never contacts a network; only backs the offline store.
fn temp_store_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-event-cache-it-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// Build a `Client` offline on a temp `.sqlite_store()` — no homeserver contact
/// is made at build time (`.homeserver_url` is not resolved until a request).
async fn offline_client(tag: &str) -> Client {
    let dir = temp_store_dir(tag);
    Client::builder()
        .homeserver_url("https://example.invalid")
        .sqlite_store(&dir, None)
        .build()
        .await
        .expect("offline client with temp sqlite store should build")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscribe_flips_has_subscribed_and_is_idempotent() {
    let client = offline_client("subscribe").await;
    let event_cache = client.event_cache();

    // Before: the event cache is dormant — `TimelineBuilder::build()` would
    // otherwise subscribe it lazily on first room open, so today only rooms
    // opened this session persist. `activate()` closes that gap.
    assert!(
        !event_cache.has_subscribed(),
        "a freshly built client must not have the event cache subscribed"
    );

    // The one call that is the whole archive-first enablement.
    event_cache
        .subscribe()
        .expect("subscribing the event cache on a temp sqlite store should succeed");
    assert!(
        event_cache.has_subscribed(),
        "subscribe() must flip has_subscribed() to true"
    );

    // Idempotent: `activate()` subscribes early, and `TimelineBuilder::build()`
    // subscribes again lazily on first timeline open — the repeat must be a cheap
    // no-op that keeps the subscription intact.
    event_cache
        .subscribe()
        .expect("a repeated subscribe() must be a no-op, not an error");
    assert!(
        event_cache.has_subscribed(),
        "a repeated subscribe() must keep has_subscribed() == true"
    );
}

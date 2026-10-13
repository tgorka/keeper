//! One session's detail and its board, as every target projects them
//! (FR-233, FR-259, AD-386; story 98.2, AD-413).
//!
//! The Mac and the phone show the same board because both call
//! [`detail`]: the header facts, the properties widget, the log, the cards
//! with their agent keys, where an agent's session runs and its room. Lifted
//! out of the shell's `sessions_root` so it is one function, read on Linux;
//! the shell only finds the zone and the row a root id and a session id name.

use std::path::Path;

use keeper_core::notes::frontmatter::Frontmatter;
use keeper_core::sessions::model::{lineage, README};
use keeper_core::sessions::pool::{self, Pool, PoolFile};
use keeper_core::sessions::shape::Shape;
use keeper_core::sessions::vm::{
    SessionDetailVm, SessionLogEntryVm, SessionPropertyVm, SessionRowVm, SessionTaskVm,
};

use super::scan::{read_ref_sources, section_snippet, RefSource, DETAIL_SCAN_BUDGET};

/// Compose one session's detail — header facts, properties, the rendered
/// log, the cards and, for an agent's session, where it runs and its room
/// (FR-233). One directory read plus one record parse; every field
/// derivable from files alone (AD-110), bar `.keeper/agents.db`, the zone's
/// own index of its logs, which never syncs.
///
/// **Both contracts, one payload.** Where the log lives differs by shape, and
/// *nothing downstream of here knows that*: the header, the properties widget and
/// the timeline render identically either way. The shape is reported so the UI can
/// decide what to **offer** — a migrate button, a new-log button — never what a
/// file means. Since story 52.1 the record's *name* is not one of the
/// differences: both contracts keep it at `README.md`.
pub fn detail(zone: &Path, row: SessionRowVm) -> SessionDetailVm {
    let dir = zone.join(&row.path);

    // One read of the session root, reused for everything: the shape, the pool
    // and the record. `ref_sources`' own scan is a separate call with a separate
    // budget, deliberately — the detail must not pay for `refs/`.
    let (sources, _truncated, shape) = read_ref_sources(&dir, DETAIL_SCAN_BUDGET);
    let flat = shape == Shape::Flat;

    // The record, `README.md` under both contracts since story 52.1. A missing
    // one is ordinary under the flat shape — a session may be nothing but logs —
    // and an empty parse degrades exactly the way an empty README always did.
    let readme = sources
        .iter()
        .find(|source| source.rel == README)
        .map(|source| source.text.clone())
        .unwrap_or_else(|| std::fs::read_to_string(dir.join(README)).unwrap_or_default());
    let (fm, body_at) = Frontmatter::parse(&readme);
    let body = &readme[body_at..];
    let line = lineage(&fm);

    // The pool, under both contracts, from that one scan.
    let pool = detail_pool(&sources, shape);

    // The properties widget (FR-227): user-tier keys only. keeper-owned keys
    // and the Obsidian-native `tags` are projected elsewhere on the header;
    // repeating them here would be two spellings of one fact.
    let owned = [
        "id",
        "created",
        "updated",
        "pinned",
        "archived",
        "keeper",
        "tags",
        "aliases",
        "cssclasses",
        "title",
    ];
    let properties: Vec<SessionPropertyVm> = fm
        .keys()
        .filter(|key| !owned.contains(key) && !key.starts_with("keeper."))
        .filter_map(|key| {
            fm.get(key).map(|value| SessionPropertyVm {
                key: key.to_owned(),
                value: value.index_string(),
            })
        })
        .collect();

    // The log, from whichever contract this session follows. `log_view` owns
    // that branch so the two readings cannot drift into two ideas of what an
    // entry is; the folder path stays byte-identical to what it always was
    // (parse `## Log`, reverse into review order — the FILE stays newest-last).
    let log: Vec<SessionLogEntryVm> = if flat {
        // Bodies come from the same texts the pool was parsed from, indexed in
        // step with `pool.logs`.
        let texts: Vec<&str> = pool
            .logs
            .iter()
            .map(|entry| {
                sources
                    .iter()
                    .find(|source| source.rel == entry.rel)
                    .map(|source| source.text.as_str())
                    .unwrap_or("")
            })
            .collect();
        pool::log_view_with_bodies(&pool, &texts)
    } else {
        pool::log_view(shape, body, &pool)
    }
    .into_iter()
    .map(|(date, title, entry_body)| SessionLogEntryVm {
        date,
        title,
        body: entry_body,
    })
    .collect();

    // An agent's session (its `agent.toml`): its room, where the approval
    // card and its answers are (98.2), and where its work runs, from its log
    // (R62, R121) — the index reads only the chunks that grew since it last
    // looked, a bounded amount per open, and takes the cards from the pool
    // this detail already read rather than reading them again. A person's
    // session has no `agent.toml`, no log, no index row and no room.
    let agent_toml =
        std::fs::read_to_string(dir.join(keeper_core::agents::session::FILE_NAME)).ok();
    let room = agent_toml.as_deref().and_then(|text| {
        keeper_core::agents::session::parse_session_agent_toml(text)
            .ok()
            .map(|agent| agent.room.to_string())
    });
    let place = if agent_toml.is_some() {
        let read: Vec<PoolFile<'_>> = sources
            .iter()
            .map(|source| PoolFile {
                rel: &source.rel,
                text: &source.text,
            })
            .collect();
        keeper_core::agents::index::Index::open(zone)
            .and_then(|mut index| index.refresh_session(&row.path, Some(&read)))
            .unwrap_or_else(|error| {
                tracing::warn!(session = %row.path, %error, "sessions: the agents index could not say where a card runs");
                None
            })
    } else {
        None
    };
    let running_on = place.as_ref().and_then(|place| place.claim_host.as_deref());
    let waiting = place
        .as_ref()
        .and_then(keeper_core::agents::index::SessionRow::waiting);

    let tasks: Vec<SessionTaskVm> = pool
        .tasks
        .iter()
        .map(|entry| SessionTaskVm {
            id: entry.id.clone(),
            rel_path: entry.rel.clone(),
            title: entry.title.clone(),
            status: entry.status.map(|status| status.as_str().to_owned()),
            order: entry.order.value,
            order_is_own: entry.order.is_own(),
            tags: entry.tags.clone(),
            unstable_identity: entry.unstable_identity,
            // From the card's own bytes, so a key the flattened fields drop
            // (written twice, or in a form the parser does not model) shows
            // as unreadable rather than absent (R119).
            agent: sources
                .iter()
                .find(|source| source.rel == entry.rel)
                .and_then(|source| keeper_core::agents::card::CardAgent::of_text(&source.text))
                .map(|card| {
                    let readable = card
                        .schedule
                        .as_deref()
                        .is_none_or(|raw| keeper_sync::tasks::TaskSchedule::parse(raw).is_ok());
                    keeper_core::agents::card::CardAgentVm::of(&card, readable, running_on, waiting)
                }),
        })
        .collect();

    let (status, archived_year) = (row.status.clone(), row.archived_year);
    SessionDetailVm {
        id: row.id,
        path: row.path,
        title: row.title,
        status,
        archived_year,
        pinned: row.pinned,
        tags: row.tags,
        properties,
        continues: line.continues,
        continued_by: line.continued_by,
        summary: section_snippet(body, "## Summary"),
        log,
        shape: shape.as_str().to_owned(),
        tasks,
        room,
    }
}

/// The pool the DETAIL reads, out of one markdown scan (FR-286).
///
/// Split out of [`detail`] so a test can hand it a scan of a folder rather than
/// having to register a root, and so the exclusion below has one reader instead
/// of being a line inside a 150-line projection.
///
/// **Both contracts read a pool now** (Story 51.7). The folder shape got one in
/// Story 51.1 — its root markdown is in [`read_ref_sources`]' walk — and this
/// was the last reader still answering as though it had none, which is what left
/// a folder-shaped session's `task`-tagged file out of the board and its
/// untagged root markdown out of *Unfiled*.
///
/// **The record is left out, and only under the folder contract.** `README.md`
/// declares no kind, so feeding it in would report the one file keeper reads the
/// session's identity, title, tags and lineage out of as *unfiled* — an
/// accusation against the file that is doing its job. A flat session's record is
/// the same file under the same name since story 52.1, and it needs no such
/// exclusion: it carries `tags: [about]`, which is the flat contract's whole
/// premise, so that pool is byte-for-byte what it was.
fn detail_pool(sources: &[RefSource], shape: Shape) -> Pool {
    let files: Vec<PoolFile<'_>> = sources
        .iter()
        .filter(|source| shape == Shape::Flat || source.rel != README)
        .map(|source| PoolFile {
            rel: &source.rel,
            text: &source.text,
        })
        .collect();
    pool::read_pool(&files)
}

#[cfg(test)]
mod tests;

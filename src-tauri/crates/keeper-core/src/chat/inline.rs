//! A `/` in the middle of a message (Story 101.3, R418).
//!
//! A whole-line command — `/topic`, `/new`, `/model` — means the whole line
//! and only opens at its start ([`crate::chat::commands`]). After whitespace
//! anywhere else in a draft, a `/` offers **inline items** instead: things
//! that become words of the message where they were typed, never acts on the
//! room. The note editor's `/` widgets drew the boundary first
//! (`widget-slash.ts`): a `/` glued to text (`and/or`, `1/2`, a path) is never
//! a menu, and after a space only what belongs mid-thought is offered.
//!
//! What is offered, decided here:
//!
//! - **Every room:** `/shrug`, which inserts `¯\_(ツ)_/¯`.
//! - **Agent rooms**, in addition: a mention of another agent this account
//!   knows (inserts `@<id>`, the name an agent delegates by, R67), and a
//!   reference to a file on a drive of this device, found by a bounded walk
//!   of the drive by file name ([`INLINE_WALK_ENTRIES`], keeper-sync's
//!   `bots_fs::find_names`) and by the titles of the drive's notes where it
//!   has a notes vault. It inserts `<drive>:<path>` — the drive's name, or
//!   its id where the name is not one word that names one drive here, and
//!   the path from the drive's root — so two drives' files of one path are
//!   never one reference (R434).
//!
//! Choosing a row replaces the token — the `/` and what follows it up to the
//! caret — and nothing else of the draft.

use matrix_sdk::ruma::{OwnedUserId, UserId};

use crate::chat::commands::{RoomKind, SHRUG};
use crate::notes::index::IndexSnapshot;

/// The longest query after the slash worth asking about, in bytes: past it
/// the token is a word, not a search.
pub const MAX_INLINE_QUERY: usize = 64;

/// The most agents one menu lists.
pub const MAX_INLINE_AGENTS: usize = 6;

/// The most files one menu lists.
pub const MAX_INLINE_FILES: usize = 6;

/// The most entries one drive's walk looks at for a file reference: a drive
/// larger than this is searched in part, and the menu says so.
pub const INLINE_WALK_ENTRIES: usize = 5_000;

/// The most matching files one drive's walk keeps.
pub const INLINE_WALK_KEEP: usize = 64;

/// What the menu says when a drive's walk stopped on its bound.
pub const INLINE_FILES_IN_PART: &str =
    "Only the first 5,000 entries of a large drive were searched; type more of the name to narrow it.";

/// What the menu says when keeper could not read this device's drives
/// (R381): no file is offered, and that is not because none matched.
pub const INLINE_DRIVES_UNREAD: &str =
    "keeper could not read this device's drives, so no file is offered.";

/// The `/shrug` row's sentence.
pub const DESC_SHRUG_INLINE: &str = "Insert ¯\\_(ツ)_/¯ here.";

/// What a row inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    /// Another agent, by the name agents delegate by.
    Mention,
    /// A file on a drive, by its path from the drive's root.
    File,
    /// `¯\_(ツ)_/¯`.
    Shrug,
}

/// One row of the mid-text menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineItem {
    /// What it is.
    pub kind: InlineKind,
    /// The row's identity, unique in one menu: `mention:<user>`, `shrug`,
    /// `file:<drive id>:<path>`.
    pub key: String,
    /// What the row shows.
    pub label: String,
    /// Its one line of detail.
    pub detail: String,
    /// What replaces the token.
    pub insert: String,
}

/// An agent this account knows: its Matrix user and, when the room shows
/// one, its display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAgent {
    pub user: OwnedUserId,
    pub name: Option<String>,
}

/// What the room offers mid-text, read by `Accounts` from the room and the
/// account's agent rooms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineContext {
    /// Which room this is.
    pub room: RoomKind,
    /// The agent working this room's session, when keeper has read it: it is
    /// the one being talked to, so it is not offered as a mention.
    pub room_agent: Option<OwnedUserId>,
    /// The agents this account knows, in any order.
    pub agents: Vec<InlineAgent>,
}

/// One drive whose files may be referenced: its id and name on this device,
/// the files its bounded walk found for the query (drive-relative paths,
/// [`walk_finds`]), whether that walk stopped on its bound, and — where the
/// drive has a notes vault — the vault's folder (from the drive's root,
/// empty for the root itself) and index, whose titles name its notes.
#[derive(Debug, Clone)]
pub struct InlineDrive<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub found: &'a [String],
    pub capped: bool,
    pub notes: Option<(&'a str, &'a IndexSnapshot)>,
}

/// The token a mid-text menu is about, `/` included, when the caret sits
/// right after one: `before` is the draft up to the caret, `after` the rest.
///
/// A token opens after whitespace only — never at the draft's start (a
/// whole-line command's place) and never glued to text — holds neither
/// whitespace nor another slash, and ends at a word's end (`after` is empty
/// or begins with whitespace).
pub fn token<'a>(before: &'a str, after: &str) -> Option<&'a str> {
    if after.chars().next().is_some_and(|c| !c.is_whitespace()) {
        return None;
    }
    let slash = before.rfind('/')?;
    let query = &before[slash + 1..];
    if query.len() > MAX_INLINE_QUERY || query.chars().any(char::is_whitespace) {
        return None;
    }
    before[..slash]
        .chars()
        .next_back()
        .filter(|c| c.is_whitespace())
        .map(|_| &before[slash..])
}

/// The query a drive's walk is asked for at `token` in a room of `ctx`: the
/// lowercased text after the slash, when the menu would offer files there —
/// an agent room and a query that is not empty. `None`: walk nothing.
pub fn file_query(token: &str, ctx: &InlineContext) -> Option<String> {
    let query = token.trim_start_matches('/').to_lowercase();
    (ctx.room != RoomKind::Chat && !query.is_empty()).then_some(query)
}

/// Whether a drive's walk keeps a file named `name` for `query`: its name
/// contains it, case aside.
pub fn walk_finds(query: &str, name: &str) -> bool {
    name.to_lowercase().contains(query)
}

/// Whether a drive's walk goes into a folder named `name`: never a hidden
/// one (`.git`, `.keeper`), whose files are no reference a person makes.
pub fn walk_enters(name: &str) -> bool {
    !name.starts_with('.')
}

/// The rows for `token` (as [`token`] found it) in a room of `ctx`, with
/// `drives` the drives on this device: mentions, then `/shrug`, then files,
/// each bounded. Empty when nothing matches — no menu.
pub fn menu(token: &str, ctx: &InlineContext, drives: &[InlineDrive<'_>]) -> Vec<InlineItem> {
    let query = token.trim_start_matches('/').to_lowercase();
    let mut rows = Vec::new();
    let agent_room = ctx.room != RoomKind::Chat;
    if agent_room {
        rows.extend(mentions(&query, ctx));
    }
    if "shrug".starts_with(&query) {
        rows.push(InlineItem {
            kind: InlineKind::Shrug,
            key: "shrug".to_owned(),
            label: "/shrug".to_owned(),
            detail: DESC_SHRUG_INLINE.to_owned(),
            insert: SHRUG.to_owned(),
        });
    }
    // An empty query would list arbitrary files: a file is searched for.
    if let Some(query) = file_query(token, ctx) {
        rows.extend(files(&query, drives));
    }
    rows
}

/// What the menu says beside its rows when it offers files: that the
/// drives could not be read (`unread`), or that one was searched in part.
pub fn files_note(
    token: &str,
    ctx: &InlineContext,
    drives: &[InlineDrive<'_>],
    unread: bool,
) -> Option<String> {
    file_query(token, ctx)?;
    if unread {
        Some(INLINE_DRIVES_UNREAD.to_owned())
    } else {
        drives
            .iter()
            .any(|drive| drive.capped)
            .then(|| INLINE_FILES_IN_PART.to_owned())
    }
}

/// The agents `query` reaches: by their id (the user's localpart, R67) or a
/// word of their name, the room's own agent left out, sorted by id.
fn mentions(query: &str, ctx: &InlineContext) -> Vec<InlineItem> {
    let mut found: Vec<&InlineAgent> = ctx
        .agents
        .iter()
        .filter(|agent| ctx.room_agent.as_deref() != Some(&*agent.user))
        .filter(|agent| reaches(query, &agent.user, agent.name.as_deref()))
        .collect();
    found.sort_by(|a, b| {
        (a.user.localpart(), a.user.as_str()).cmp(&(b.user.localpart(), b.user.as_str()))
    });
    found.dedup_by(|a, b| a.user == b.user);
    found
        .into_iter()
        .take(MAX_INLINE_AGENTS)
        .map(|agent| {
            let id = agent.user.localpart();
            InlineItem {
                kind: InlineKind::Mention,
                key: format!("mention:{}", agent.user),
                label: format!("@{id}"),
                detail: agent.name.clone().unwrap_or_else(|| agent.user.to_string()),
                insert: format!("@{id}"),
            }
        })
        .collect()
}

fn reaches(query: &str, user: &UserId, name: Option<&str>) -> bool {
    user.localpart().to_lowercase().starts_with(query)
        || name.is_some_and(|name| {
            name.split_whitespace()
                .any(|word| word.to_lowercase().starts_with(query))
        })
}

/// The files `query` reaches: those whose file name, or a word of whose
/// note title, begins with it first, then those whose file name contains it
/// — each drive's walk's finds and its notes' titles, once per drive and
/// path, in drive order — at most [`MAX_INLINE_FILES`].
fn files(query: &str, drives: &[InlineDrive<'_>]) -> Vec<InlineItem> {
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for drive in drives {
        let by_title = drive.notes.into_iter().flat_map(|(folder, index)| {
            index
                .entries()
                .iter()
                .filter(|entry| {
                    entry
                        .title
                        .to_lowercase()
                        .split_whitespace()
                        .any(|word| word.starts_with(query))
                })
                .map(move |entry| drive_path(folder, &entry.path))
        });
        for path in drive.found.iter().cloned().chain(by_title) {
            if !seen.insert((drive.id, path.clone())) {
                continue;
            }
            let title = drive.notes.and_then(|(folder, index)| {
                let folder = folder.trim_matches('/');
                let rel = if folder.is_empty() {
                    Some(path.as_str())
                } else {
                    path.strip_prefix(folder)
                        .and_then(|rest| rest.strip_prefix('/'))
                };
                rel.and_then(|rel| index.by_path(rel))
                    .map(|entry| entry.title.clone())
            });
            let name = path.rsplit('/').next().unwrap_or(&path).to_lowercase();
            let titled = title.as_ref().is_some_and(|title| {
                title
                    .to_lowercase()
                    .split_whitespace()
                    .any(|word| word.starts_with(query))
            });
            let bucket = if name.starts_with(query) || titled {
                &mut starts
            } else if name.contains(query) {
                &mut contains
            } else {
                continue;
            };
            if bucket.len() < MAX_INLINE_FILES {
                bucket.push((drive, path, title));
            }
        }
    }
    starts
        .into_iter()
        .chain(contains)
        .take(MAX_INLINE_FILES)
        .map(|(drive, path, title)| {
            let label =
                title.unwrap_or_else(|| path.rsplit('/').next().unwrap_or(&path).to_owned());
            InlineItem {
                kind: InlineKind::File,
                key: format!("file:{}:{path}", drive.id),
                label,
                detail: format!("{} · {path}", drive.name),
                insert: format!("{}:{path}", qualifier(drive, drives)),
            }
        })
        .collect()
}

/// How a reference names `drive`: its name, when that is one word no other
/// drive here shares; else its id.
fn qualifier<'a>(drive: &InlineDrive<'a>, drives: &[InlineDrive<'_>]) -> &'a str {
    let one_word =
        !drive.name.is_empty() && !drive.name.contains(|c: char| c.is_whitespace() || c == ':');
    let shared = drives
        .iter()
        .any(|other| other.id != drive.id && other.name == drive.name);
    if one_word && !shared {
        drive.name
    } else {
        drive.id
    }
}

/// `rel` (from the vault's root) as a path from the drive's root.
fn drive_path(folder: &str, rel: &str) -> String {
    let folder = folder.trim_matches('/');
    if folder.is_empty() {
        rel.to_owned()
    } else {
        format!("{folder}/{rel}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inserts(rows: &[InlineItem]) -> Vec<(InlineKind, &str)> {
        rows.iter()
            .map(|row| (row.kind, row.insert.as_str()))
            .collect()
    }

    /// A `/` opens mid-text only after whitespace, at a word's end, and never
    /// at the draft's start (a whole-line command's place) or glued to text.
    #[test]
    fn a_token_opens_after_whitespace_only() {
        assert_eq!(token("ask /to", ""), Some("/to"));
        assert_eq!(token("ask /", " later"), Some("/"));
        assert_eq!(token("line one\n/sh", ""), Some("/sh"));
        assert_eq!(token("/to", ""), None, "the draft's start is a command's");
        assert_eq!(token("and/or", ""), None, "glued to text");
        assert_eq!(token("see a //x", ""), None, "a doubled slash");
        assert_eq!(token("ask /to", "la"), None, "the caret inside a word");
        assert_eq!(token("ask /to la", ""), None, "a space ends the token");
        let long = format!("ask /{}", "x".repeat(MAX_INLINE_QUERY + 1));
        assert_eq!(token(&long, ""), None, "past the bound");
    }

    /// A messenger room offers `/shrug` alone and walks no drive (108.2: the
    /// agent rooms' mentions and file references are gone with them).
    #[test]
    fn a_chat_offers_only_shrug() {
        let chat = InlineContext {
            room: RoomKind::Chat,
            room_agent: None,
            agents: Vec::new(),
        };
        assert_eq!(
            inserts(&menu("/", &chat, &[])),
            vec![(InlineKind::Shrug, SHRUG)]
        );
        assert!(menu("/to", &chat, &[]).is_empty());
        assert_eq!(file_query("/to", &chat), None, "a chat walks no drive");
    }
}

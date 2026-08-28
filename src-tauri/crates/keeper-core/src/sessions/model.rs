//! What a session is, read off the zone's own layout (FR-225, FR-226, FR-228).
//!
//! Discovery, naming, status and freshness are all *derivations* from facts
//! the shell supplies — a relative path, a set of `(path, mtime)` pairs, a
//! README's bytes. Nothing here is stored state, so nothing here can disagree
//! with a Finder edit for longer than one rescan (AD-110).

use crate::notes::frontmatter::Frontmatter;
use crate::notes::naming::slug;

/// The zone subfolder that holds running sessions.
pub const ACTIVE_DIR: &str = "active";
/// The zone subfolder that holds finished sessions, filed by close year.
pub const ARCHIVE_DIR: &str = "archive";
/// The zone's template skeleton. Never a session; copied verbatim on create.
pub const TEMPLATE_DIR: &str = "_template";
/// The session subtree that is scratch: unversioned, unsynced, read-only to
/// keeper (AD-113). Spelled once so the refusal and the freshness split cannot
/// disagree about which directory they mean.
pub const WORKSPACE_DIR: &str = "workspace";
/// The session subtree holding promoted output — versioned and synced.
pub const ARTIFACTS_DIR: &str = "artifacts";
/// The session's record. Its frontmatter is the session's identity, tags,
/// properties and pins (FR-227).
pub const README: &str = "README.md";

/// The frontmatter key namespace reserved for session bookkeeping, one level
/// under the `keeper:` map exactly as the notes contract allows (AD-109):
/// lineage lives at `keeper.session-continues` / `keeper.session-continued-by`.
///
/// Flat `session-continues` under `keeper:` rather than a second nesting
/// level, because [`crate::notes::frontmatter`] deliberately parses exactly
/// one level under `keeper:` and the parser does not grow for this phase —
/// the budget AD-109 set was "a reserved-key list entry, no grammar change".
///
/// The canonical spelling is a **flow list** (`session-continues: [01J4…]`):
/// the parser models a flow list under the nested map, whereas a block list
/// at the second level is past its one-level budget and reads as unparsed.
/// The writer therefore always writes the flow form; the reader tolerates a
/// bare scalar as a one-item list, because a hand edit will produce one.
pub const KEY_CONTINUES: &str = "session-continues";
/// See [`KEY_CONTINUES`]; the other direction of the lineage pair (AD-112).
pub const KEY_CONTINUED_BY: &str = "session-continued-by";

/// Where a session sits in its lifecycle — a fact about its LOCATION, never a
/// stored flag (AD-110). The zone knows exactly these two states; deletion is
/// absence, and a third on-disk state would be a contract change in the
/// drives, not in keeper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// A direct child of `active/`.
    Active,
    /// A child of `archive/<year>/`, carrying the year it was filed under.
    Archived(i32),
}

/// Classify one zone-relative directory path: is it a session, and if so in
/// which state? `None` for everything the index must skip — `_template/`, any
/// `_`- or `.`-prefixed name, loose files' parents, and unknown top-levels
/// (FR-225).
///
/// The path is zone-relative with `/` separators, e.g. `active/2026-08-10-keeper`
/// or `archive/2026/2026-03-01-taxes`. Depth is load-bearing: a directory
/// nested *inside* a session (`active/x/workspace`) is not itself a session.
pub fn classify(rel_path: &str) -> Option<SessionStatus> {
    let mut parts = rel_path.split('/');
    let top = parts.next()?;
    match top {
        ACTIVE_DIR => {
            let name = parts.next()?;
            if parts.next().is_some() || skipped(name) {
                return None;
            }
            Some(SessionStatus::Active)
        }
        ARCHIVE_DIR => {
            let year = parts.next()?;
            let name = parts.next()?;
            if parts.next().is_some() || skipped(name) {
                return None;
            }
            // A non-year directory under archive/ is somebody's own filing,
            // not keeper's business; the sessions inside it stay invisible
            // rather than half-indexed under an unparseable year.
            let year: i32 = year.parse().ok()?;
            Some(SessionStatus::Archived(year))
        }
        _ => None,
    }
}

/// The names the index never treats as sessions: the template, anything
/// underscore-prefixed beside it, and dotfiles (FR-225).
///
/// Public because the zone holds more `_`-prefixed things than `_template/`
/// now — `_spaces/` joins it — and every reader of the zone must agree on
/// which names are infrastructure rather than work. One rule, asked here.
pub fn skipped(name: &str) -> bool {
    name.starts_with('_') || name.starts_with('.')
}

/// Fold a title into a session folder name `YYYY-MM-DD-<slug>`, with a
/// collision counter appended the way note filenames do it (FR-238).
///
/// The date is the caller's — the domain has no clock — and `taken` is the
/// set of sibling directory names already present in `active/`. The slug
/// rules are the note slug rules verbatim (one title, one spelling, on every
/// platform this zone syncs to), which is why this delegates rather than
/// re-implements.
pub fn session_dir_name(title: &str, date: &str, taken: &[String]) -> String {
    let base = format!("{date}-{}", slug(title));
    if !taken.iter().any(|t| t == &base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The two freshness signals a session row shows, kept apart on purpose
/// (FR-228, UX-DR86): `workspace` is "the agent is iterating", `record` is
/// "something was written or promoted". Milliseconds since the Unix epoch;
/// `None` means the subtree is empty or unreadable, and the UI says "—"
/// rather than inventing a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Freshness {
    pub workspace_ms: Option<i64>,
    pub record_ms: Option<i64>,
}

/// Fold `(session-relative path, mtime_ms)` facts into the two signals.
///
/// The caller walks; this decides which side of the split each file is on.
/// `workspace/**` feeds the workspace signal; everything else — README,
/// `artifacts/`, `refs/`, `prompts/`, loose files — feeds the record signal.
/// `.gitkeep` files are placeholders, not activity, and count for neither.
pub fn freshness<'a>(files: impl IntoIterator<Item = (&'a str, i64)>) -> Freshness {
    let mut out = Freshness::default();
    for (path, mtime_ms) in files {
        if path.rsplit('/').next() == Some(".gitkeep") {
            continue;
        }
        let slot = if path == WORKSPACE_DIR || path.starts_with("workspace/") {
            &mut out.workspace_ms
        } else {
            &mut out.record_ms
        };
        *slot = Some(slot.map_or(mtime_ms, |seen: i64| seen.max(mtime_ms)));
    }
    out
}

/// The lineage a README declares, both directions, as ULID lists (AD-112).
///
/// Read from the reserved keys under `keeper:`; absent keys are empty lists.
/// A dangling id (the target was deleted) is the *reader's* problem to render
/// inertly — the domain reports what the file says, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lineage {
    /// Sessions this one continues (usually one).
    pub continues: Vec<String>,
    /// Sessions that continue this one.
    pub continued_by: Vec<String>,
}

/// Parse a README body's `## Log` into its dated entries, file order —
/// the zone writes newest-last; a *display* that wants newest-first reverses
/// the projection, never the file (FR-233).
///
/// An entry is a `### YYYY-MM-DD[ — title]` heading and the prose until the
/// next `###`/`##` heading. Non-dated `###` headings inside the section are
/// carried as part of the preceding entry's body — they are the entry's own
/// sub-structure, not new sittings. Pure over `&str`, like everything here.
pub fn log_entries(body: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut in_log = false;
    for line in body.lines() {
        let trimmed = line.trim_end();
        let lead = trimmed.trim_start();
        if lead.starts_with("## ") {
            in_log = lead == "## Log";
            continue;
        }
        if !in_log {
            continue;
        }
        if let Some(rest) = lead.strip_prefix("### ") {
            let candidate = rest.trim();
            let dated = candidate.len() >= 10
                && candidate.as_bytes().get(4) == Some(&b'-')
                && candidate.as_bytes().get(7) == Some(&b'-')
                && candidate[..4].bytes().all(|b| b.is_ascii_digit());
            if dated {
                let date: String = candidate.chars().take(10).collect();
                let title = candidate
                    .split_once('—')
                    .map(|(_, after)| after.trim().to_owned())
                    .unwrap_or_default();
                out.push((date, title, String::new()));
                continue;
            }
        }
        if let Some((_, _, entry_body)) = out.last_mut() {
            if !entry_body.is_empty() || !lead.is_empty() {
                entry_body.push_str(trimmed);
                entry_body.push('\n');
            }
        }
    }
    for (_, _, entry_body) in &mut out {
        let trimmed = entry_body.trim().to_owned();
        *entry_body = trimmed;
    }
    out
}

/// Read the lineage off a parsed README frontmatter block.
///
/// The keys live one level under the `keeper:` map, which the parser models
/// as a [`FieldValue::Map`] — the same access shape the default-spaces reader
/// uses for `keeper.space`.
pub fn lineage(fm: &Frontmatter) -> Lineage {
    use crate::notes::frontmatter::FieldValue;
    let pairs = match fm.get("keeper") {
        Some(FieldValue::Map(pairs)) => pairs.as_slice(),
        _ => &[],
    };
    let list = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| match value {
                FieldValue::List(items) => items
                    .iter()
                    .map(FieldValue::index_string)
                    .filter(|id| !id.trim().is_empty())
                    .collect(),
                scalar => {
                    let one = scalar.index_string();
                    if one.trim().is_empty() {
                        Vec::new()
                    } else {
                        vec![one]
                    }
                }
            })
            .unwrap_or_default()
    };
    Lineage {
        continues: list(KEY_CONTINUES),
        continued_by: list(KEY_CONTINUED_BY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The discovery rules over the exact layout the live drives hold — plus
    /// the shapes that must NOT be sessions, each of which has a reason.
    #[test]
    fn the_zone_layout_classifies_as_the_readme_documents() {
        assert_eq!(
            classify("active/2026-08-10-keeper"),
            Some(SessionStatus::Active)
        );
        assert_eq!(
            classify("archive/2025/2025-03-01-taxes"),
            Some(SessionStatus::Archived(2025))
        );
        // The template is a skeleton, not a session (FR-225).
        assert_eq!(classify("_template"), None);
        assert_eq!(
            classify("active/_wip"),
            None,
            "underscore names are skipped"
        );
        assert_eq!(classify("active/.DS_Store"), None, "dot names are skipped");
        // Depth: subtrees of a session are not themselves sessions.
        assert_eq!(classify("active/2026-08-10-keeper/workspace"), None);
        assert_eq!(classify("archive/2025/2025-03-01-taxes/artifacts"), None);
        // A stray directory at the zone root and a non-year under archive/
        // stay invisible rather than half-indexed.
        assert_eq!(classify("prompts-shared"), None);
        assert_eq!(classify("archive/old-stuff/2020-01-01-x"), None);
        // Direct children of archive/ (no year) are not sessions either.
        assert_eq!(classify("archive/2026"), None);
    }

    /// Folder naming is the note naming rules on a date prefix: collisions
    /// count up, and the slug alphabet is the Windows-safe one.
    #[test]
    fn a_title_becomes_a_dated_folder_name_and_collisions_count_up() {
        assert_eq!(
            session_dir_name("Keeper — rolling work", "2026-08-12", &[]),
            "2026-08-12-keeper-rolling-work"
        );
        let taken = vec![
            "2026-08-12-keeper".to_owned(),
            "2026-08-12-keeper-2".to_owned(),
        ];
        assert_eq!(
            session_dir_name("keeper", "2026-08-12", &taken),
            "2026-08-12-keeper-3"
        );
    }

    /// The freshness split is the two-signal contract (UX-DR86): workspace
    /// and record never blend, `.gitkeep` is furniture, and an empty side
    /// stays `None` rather than becoming epoch zero.
    #[test]
    fn freshness_splits_workspace_from_record_and_ignores_gitkeep() {
        let got = freshness([
            ("README.md", 100),
            ("workspace/iter-3.md", 900),
            ("workspace/deep/scratch.csv", 400),
            ("artifacts/report.md", 300),
            ("workspace/.gitkeep", 99_999),
        ]);
        assert_eq!(got.workspace_ms, Some(900));
        assert_eq!(got.record_ms, Some(300));

        let empty_ws = freshness([("README.md", 5)]);
        assert_eq!(empty_ws.workspace_ms, None, "no scratch means no signal");
        assert_eq!(empty_ws.record_ms, Some(5));
    }

    /// Lineage reads the reserved keys and reports the file verbatim — a
    /// dangling ref is the renderer's to mute, not the parser's to drop. The
    /// canonical spelling is the flow list (the one form the parser models at
    /// this nesting depth); a hand-written bare scalar reads as one item.
    #[test]
    fn lineage_reads_both_directions_from_the_reserved_keys() {
        let source = "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\nkeeper:\n  session-continues: [01J4BBBBBBBBBBBBBBBBBBBBBB]\n  session-continued-by: [01J6CCCCCCCCCCCCCCCCCCCCCC, 01J7DDDDDDDDDDDDDDDDDDDDDD]\n---\n# t\n";
        let (fm, _) = Frontmatter::parse(source);
        assert!(
            fm.unparsed().is_none(),
            "the canonical spelling parses clean"
        );
        let got = lineage(&fm);
        assert_eq!(got.continues, vec!["01J4BBBBBBBBBBBBBBBBBBBBBB"]);
        assert_eq!(
            got.continued_by,
            vec!["01J6CCCCCCCCCCCCCCCCCCCCCC", "01J7DDDDDDDDDDDDDDDDDDDDDD"]
        );

        // A hand edit's bare scalar is tolerated as a one-item list.
        let scalar = "---\nkeeper:\n  session-continues: 01J4BBBBBBBBBBBBBBBBBBBBBB\n---\n";
        let (fm2, _) = Frontmatter::parse(scalar);
        assert_eq!(lineage(&fm2).continues, vec!["01J4BBBBBBBBBBBBBBBBBBBBBB"]);

        // No keeper map at all: empty, never an error.
        let (fm3, _) = Frontmatter::parse("---\nid: x\n---\n");
        assert_eq!(lineage(&fm3), Lineage::default());
    }

    /// The log parser reads the zone's own convention: dated ### headings in
    /// file order, dash titles, prose bodies, non-dated ### as sub-structure
    /// of the sitting it belongs to, and nothing outside `## Log`.
    #[test]
    fn log_entries_read_the_zones_own_convention() {
        let body = "# s\n\n## Summary\n\n### not a log heading\n\n## Log\n\n### 2026-08-10 — opened\n\nfirst prose\n\n### interlude heading\n\nmore of the first sitting\n\n### 2026-08-11\n\nsecond prose\n\n## Follow-ups\n\n### 2026-08-12 — not in the log\n";
        let entries = log_entries(body);
        assert_eq!(entries.len(), 2, "only dated headings inside ## Log");
        assert_eq!(entries[0].0, "2026-08-10");
        assert_eq!(entries[0].1, "opened");
        assert!(entries[0].2.contains("first prose"));
        assert!(
            entries[0].2.contains("### interlude heading"),
            "a non-dated heading is the sitting's own sub-structure"
        );
        assert_eq!(entries[1].0, "2026-08-11");
        assert_eq!(entries[1].1, "", "a heading without a dash has no title");
        assert_eq!(entries[1].2, "second prose");
    }
}

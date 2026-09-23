//! List-only service-file classification (AD-267): configured basenames, never a
//! path or glob, plus every space definition the index flags.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::notes::index::IndexEntry;

pub const DEFAULT_SERVICE_FILE_NAMES: [&str; 4] = ["index.md", "agents.md", "claude.md", "log.md"];

/// Compare the final vault-relative segment against names normalized by the registry.
pub fn is_service_file(path: &str, names: &[String]) -> bool {
    let basename = path.rsplit('/').next().unwrap_or_default();
    if basename.is_ascii() {
        names
            .iter()
            .any(|name| !name.is_empty() && basename.eq_ignore_ascii_case(name))
    } else {
        names.contains(&basename.to_lowercase())
    }
}

/// The `is:` flag the index stamps on a space definition.
pub const SPACE_FLAG: &str = "space";

/// Whether the default list hides an entry as a space definition.
///
/// The index flag is the definition rather than a path test here: it is stamped
/// against the vault's own configurable spaces folder and already leaves out
/// the reserved `index.md`/`log.md`, which are not spaces. Asking for
/// `is:space` by name shows them again, as asking for a conflict does.
pub fn hides_space_definition(flags: &[String], asked: &[String]) -> bool {
    flags.iter().any(|flag| flag == SPACE_FLAG) && !asked.iter().any(|flag| flag == SPACE_FLAG)
}

/// A revision of a vault's space definitions, taken over the index entries that
/// carry [`SPACE_FLAG`].
///
/// The note list hides these by default, so an edit to one can move no row and
/// no count, and the changes stream would say nothing. The rail reloads on every
/// batch, so the stream compares this value between wakes and sends one when it
/// moves. Path, the revalidation triple and the flags are what the index itself
/// trusts to say a file changed. In-process only: never persisted or sent.
pub fn space_definitions_revision(entries: &[IndexEntry]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for entry in entries.iter().filter(|entry| entry.has_flag(SPACE_FLAG)) {
        entry.path.hash(&mut hasher);
        entry.size.hash(&mut hasher);
        entry.mtime_ns.hash(&mut hasher);
        entry.ino.hash(&mut hasher);
        entry.flags.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_basename_in_any_folder_without_case() {
        let names = DEFAULT_SERVICE_FILE_NAMES.map(str::to_owned);
        assert!(is_service_file("agents.md", &names));
        assert!(is_service_file("docs/Log.MD", &names));
        assert!(is_service_file("a/b/CLAUDE.md", &names));
        assert!(!is_service_file("docs/my-log.md", &names));
        assert!(!is_service_file("log.md/note.md", &names));
    }

    #[test]
    fn ignores_paths_and_blank_names() {
        let names = ["docs/log.md", "docs\\log.md", "", "   "].map(str::to_owned);
        assert!(!is_service_file("docs/log.md", &names));
        assert!(!is_service_file("", &names));
        assert!(!is_service_file("log.md", &[]));
    }

    #[test]
    fn hides_space_definitions_unless_asked_for_by_name() {
        let space = [SPACE_FLAG.to_owned()];
        let pinned_space = ["pinned".to_owned(), SPACE_FLAG.to_owned()];
        assert!(hides_space_definition(&space, &[]));
        assert!(hides_space_definition(
            &pinned_space,
            &["pinned".to_owned()]
        ));
        assert!(!hides_space_definition(&pinned_space, &space));
        assert!(!hides_space_definition(&["pinned".to_owned()], &[]));
        assert!(!hides_space_definition(&["spaces".to_owned()], &[]));
    }

    fn entry(path: &str, flags: &[&str]) -> IndexEntry {
        IndexEntry {
            id: path.to_owned(),
            path: path.to_owned(),
            title: path.to_owned(),
            size: 1,
            mtime_ns: 1,
            ino: 1,
            created_ms: 0,
            updated_ms: 0,
            tags: Vec::new(),
            fields: std::collections::BTreeMap::new(),
            links: Vec::new(),
            link_predicates: std::collections::BTreeMap::new(),
            flags: flags.iter().map(|flag| (*flag).to_owned()).collect(),
            snippet: String::new(),
            order: crate::notes::order::NoteOrder::default(),
        }
    }

    #[test]
    fn space_revision_moves_with_space_definitions_only() {
        let entries = vec![
            entry("notes/a.md", &[]),
            entry("spaces/inbox.md", &[SPACE_FLAG]),
        ];
        let base = space_definitions_revision(&entries);
        assert_eq!(base, space_definitions_revision(&entries.clone()));

        let mut edited_note = entries.clone();
        edited_note[0].mtime_ns = 2;
        edited_note[0].size = 9;
        assert_eq!(base, space_definitions_revision(&edited_note));

        let mut edited_space = entries.clone();
        edited_space[1].mtime_ns = 2;
        assert_ne!(base, space_definitions_revision(&edited_space));

        let mut replaced_space = entries.clone();
        replaced_space[1].ino = 2;
        assert_ne!(base, space_definitions_revision(&replaced_space));

        let mut temporary = entries.clone();
        temporary[1].flags.push("temporary".to_owned());
        assert_ne!(base, space_definitions_revision(&temporary));

        let mut renamed = entries.clone();
        renamed[1].path = "spaces/later.md".to_owned();
        assert_ne!(base, space_definitions_revision(&renamed));

        assert_ne!(base, space_definitions_revision(&entries[..1]));
    }
}

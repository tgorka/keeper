//! Ordered rail projection, including fileless ancestry.
use super::{sort, space_name, vm::NoteSpaceVm};
use std::collections::BTreeMap;

fn synthetic(id: &str, name: &str, query: String, icon: Option<&str>) -> NoteSpaceVm {
    NoteSpaceVm {
        id: id.into(),
        name: name.into(),
        query,
        icon: icon.map(str::to_owned),
        updated_ms: None,
        sort: String::new(),
        sort_effective: sort::DEFAULT_SORT.canonical(),
        limit: 0,
        default_key: None,
        template: None,
        folder: None,
        warnings: Vec::new(),
        order: 0.0,
        error: None,
        pinned: false,
        ttl_hours: None,
        expires_ms: None,
        text: None,
        parent: None,
        leaf_name: name.into(),
        depth: 0,
        descendants: 0,
        expiry_phrase: String::new(),
        restore: Default::default(),
    }
}

fn compare(a: &NoteSpaceVm, b: &NoteSpaceVm) -> std::cmp::Ordering {
    sort::rail_order(
        (a.pinned, a.order, a.updated_ms, &a.name),
        (b.pinned, b.order, b.updated_ms, &b.name),
    )
    .then_with(|| a.id.cmp(&b.id))
}

#[must_use]
pub fn compose_rail(spaces: Vec<NoteSpaceVm>, uncategorized: String) -> Vec<NoteSpaceVm> {
    let (mut temporary, persistent): (Vec<_>, Vec<_>) =
        spaces.into_iter().partition(|s| s.ttl_hours.is_some());
    let mut nodes = BTreeMap::new();
    let mut duplicates = Vec::new();
    for mut row in persistent {
        let (parent, leaf, _) = space_name::split(&row.name);
        let path = parent
            .as_ref()
            .map_or_else(|| leaf.clone(), |p| format!("{p}/{leaf}"));
        row.leaf_name = leaf;
        row.descendants = 0;
        match nodes.entry(path) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(row);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let duplicate = if row.id < entry.get().id {
                    entry.insert(row)
                } else {
                    row
                };
                duplicates.push((entry.key().clone(), duplicate));
            }
        }
    }
    let paths: Vec<_> = nodes.keys().cloned().collect();
    for path in paths {
        let mut parent = space_name::split(&path).0;
        while let Some(path) = parent {
            parent = space_name::split(&path).0;
            nodes.entry(path.clone()).or_insert_with(|| {
                let mut row =
                    synthetic(&format!("keeper:group:{path}"), &path, String::new(), None);
                row.leaf_name = space_name::split(&path).1;
                row
            });
        }
    }
    let ids: BTreeMap<_, _> = nodes
        .iter()
        .map(|(path, row)| (path.clone(), row.id.clone()))
        .collect();
    let mut children: BTreeMap<Option<String>, Vec<NoteSpaceVm>> = BTreeMap::new();
    for (path, mut row) in nodes.into_iter().chain(duplicates) {
        let (parent, _, depth) = space_name::split(&path);
        row.parent = parent.and_then(|p| ids.get(&p).cloned());
        row.depth = depth;
        children.entry(row.parent.clone()).or_default().push(row);
    }
    for siblings in children.values_mut() {
        siblings.sort_by(compare);
    }
    let mut out = vec![synthetic(
        "keeper:all",
        "All notes",
        String::new(),
        Some("notebook"),
    )];
    if !temporary.is_empty() {
        let count = u32::try_from(temporary.len()).unwrap_or(u32::MAX);
        out[0].descendants = count;
        let mut group = synthetic("keeper:temporary", "Temporary", String::new(), None);
        group.parent = Some("keeper:all".into());
        group.depth = 1;
        group.descendants = count;
        out.push(group);
        temporary.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| a.expires_ms.cmp(&b.expires_ms))
                .then_with(|| compare(a, b))
        });
        for mut row in temporary {
            row.parent = Some("keeper:temporary".into());
            row.depth = 2;
            row.leaf_name = row.name.clone();
            row.descendants = 0;
            out.push(row);
        }
    }
    // Iterative depth-first walk: arbitrary user-written depth cannot exhaust the stack.
    let mut pending: Vec<_> = children
        .remove(&None)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .collect();
    while let Some(row) = pending.pop() {
        if let Some(kids) = children.remove(&Some(row.id.clone())) {
            pending.extend(kids.into_iter().rev());
        }
        out.push(row);
    }
    let positions: BTreeMap<_, _> = out
        .iter()
        .enumerate()
        .map(|(i, row)| (row.id.clone(), i))
        .collect();
    for i in (1..out.len()).rev() {
        if out[i].id == "keeper:temporary" || out[i].parent.as_deref() == Some("keeper:temporary") {
            continue;
        }
        if let Some(parent) = out[i]
            .parent
            .as_ref()
            .and_then(|p| positions.get(p))
            .copied()
        {
            let count = out[i].descendants + u32::from(!out[i].id.starts_with("keeper:"));
            out[parent].descendants = out[parent].descendants.saturating_add(count);
        }
    }
    out.push(synthetic(
        "keeper:uncategorized",
        "Uncategorized",
        uncategorized,
        Some("shapes"),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_ancestors_are_unique_and_temporary_group_is_conditional() {
        let mut bali = synthetic("bali", "Journal/Bali", "tag:bali".into(), None);
        let rome = synthetic("rome", "Journal/Rome", "tag:rome".into(), None);
        let rows = compose_rail(vec![bali.clone(), rome], "-tag:bali".into());
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            [
                "keeper:all",
                "keeper:group:Journal",
                "bali",
                "rome",
                "keeper:uncategorized"
            ]
        );
        assert_eq!(rows[1].descendants, 2);
        assert_eq!(rows[2].parent.as_deref(), Some("keeper:group:Journal"));
        bali.ttl_hours = Some(2);
        let rows = compose_rail(vec![bali], String::new());
        assert_eq!(rows[1].id, "keeper:temporary");
        assert_eq!(rows[2].leaf_name, "Journal/Bali");
        assert_eq!(rows[2].depth, 2);
    }
    #[test]
    fn real_ancestor_and_pins_determine_depth_first_order() {
        let parent = synthetic("journal", "Journal", "tag:j".into(), None);
        let mut child = synthetic("bali", "Journal/Bali", "tag:b".into(), None);
        child.pinned = true;
        let other = synthetic("a", "Journal/A", "tag:a".into(), None);
        let rows = compose_rail(vec![other, child, parent], String::new());
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["keeper:all", "journal", "bali", "a", "keeper:uncategorized"]
        );
        assert_eq!(rows[1].descendants, 2);
    }

    #[test]
    fn synthetic_rows_surround_the_owners_order_and_duplicate_titles_survive() {
        let mut older = synthetic("older", "Older", "tag:o".into(), None);
        older.updated_ms = Some(10);
        let mut newer = synthetic("newer", "Newer", "tag:n".into(), None);
        newer.updated_ms = Some(20);
        let mut positioned = synthetic("positioned", "Positioned", "tag:p".into(), None);
        positioned.order = -1.0;
        let rows = compose_rail(vec![older, positioned, newer], "-tag:claimed".into());
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            [
                "keeper:all",
                "positioned",
                "newer",
                "older",
                "keeper:uncategorized"
            ]
        );
        assert!(rows[0].query.is_empty());
        assert!(rows[0].error.is_none());
        assert_eq!(rows[4].query, "-tag:claimed");
        let same = compose_rail(
            vec![
                synthetic("one", "Same", "tag:a".into(), None),
                synthetic("two", "Same", "tag:b".into(), None),
            ],
            String::new(),
        );
        assert!(same.iter().any(|row| row.id == "one"));
        assert!(same.iter().any(|row| row.id == "two"));
    }

    #[test]
    fn temporary_pins_lead_then_the_soonest_expiry() {
        let temporary = |id: &str, expires, pinned| {
            let mut row = synthetic(id, id, "tag:x".into(), None);
            row.ttl_hours = Some(48);
            row.expires_ms = Some(expires);
            row.pinned = pinned;
            row
        };
        let rows = compose_rail(
            vec![
                temporary("late", 30, false),
                temporary("pin", 40, true),
                temporary("soon", 10, false),
            ],
            String::new(),
        );
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            [
                "keeper:all",
                "keeper:temporary",
                "pin",
                "soon",
                "late",
                "keeper:uncategorized"
            ]
        );
        assert_eq!(rows[1].descendants, 3);
    }
}

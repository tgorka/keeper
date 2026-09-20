//! Merge independently scored vault answers before paging.
use super::vm::NoteRowVm;

#[must_use]
pub fn merge_rows(pages: Vec<Vec<NoteRowVm>>, totals: &[u32]) -> (Vec<NoteRowVm>, u32) {
    let mut rows: Vec<_> = pages.into_iter().flatten().collect();
    rows.sort_by(|a, b| {
        let score = |row: &NoteRowVm| row.hit.as_ref().map_or(0.0, |hit| hit.score);
        score(b)
            .total_cmp(&score(a))
            .then_with(|| b.updated_ms.cmp(&a.updated_ms))
            .then_with(|| a.vault_id.cmp(&b.vault_id))
            .then_with(|| a.path.cmp(&b.path))
    });
    let total = totals.iter().fold(0u32, |sum, n| sum.saturating_add(*n));
    (rows, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{order::NoteOrder, vm::NoteHitVm};
    fn row(vault: &str, path: &str, score: f32, updated: i64) -> NoteRowVm {
        NoteRowVm {
            vault_id: vault.into(),
            id: path.into(),
            path: path.into(),
            title: path.into(),
            snippet: String::new(),
            hit: Some(NoteHitVm {
                snippet: String::new(),
                marks: Vec::new(),
                why: "words".into(),
                score,
            }),
            tags: Vec::new(),
            updated_ms: updated,
            pinned: false,
            archived: false,
            unread: false,
            conflict: false,
            origin: String::new(),
            predicates: Vec::new(),
            unresolved_target: String::new(),
            head_rev: String::new(),
            order: NoteOrder::default(),
        }
    }
    #[test]
    fn globally_orders_and_sums_unpaged_totals() {
        let (rows, total) = merge_rows(
            vec![
                vec![row("b", "b", 1.0, 9), row("a", "x", 3.0, 0)],
                vec![
                    row("a", "z", 1.0, 9),
                    row("a", "a", 1.0, 9),
                    row("b", "new", 1.0, 10),
                ],
            ],
            &[7, 8],
        );
        assert_eq!(total, 15);
        assert_eq!(
            rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["x", "new", "a", "z", "b"]
        );
    }
}

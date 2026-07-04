//! Pure demo batch producer establishing the snapshot-then-diff seam (AD-8).
//!
//! This is the reusable ordering invariant every later stream story copies: the
//! producer lives in the tauri-free core and is unit-testable without a Tauri
//! `Channel`. The shell command simply forwards the produced batches over the
//! channel in order.

use crate::vm::{DemoBatch, DemoItem};

/// Produce an ordered sequence of demo batches whose **first** element is the
/// full [`DemoBatch::Snapshot`], followed by [`DemoBatch::Diff`] batches.
///
/// The ordering is the contract: a diff is never emitted before the snapshot.
pub fn snapshot_then_diff() -> Vec<DemoBatch> {
    vec![
        DemoBatch::Snapshot {
            items: vec![
                DemoItem {
                    id: "1".to_owned(),
                    label: "Alpha".to_owned(),
                },
                DemoItem {
                    id: "2".to_owned(),
                    label: "Beta".to_owned(),
                },
            ],
        },
        DemoBatch::Diff {
            added: vec![DemoItem {
                id: "3".to_owned(),
                label: "Gamma".to_owned(),
            }],
            removed: vec!["1".to_owned()],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_batch_is_the_snapshot() {
        let batches = snapshot_then_diff();
        assert!(
            !batches.is_empty(),
            "producer must emit at least the snapshot"
        );
        assert!(
            matches!(batches[0], DemoBatch::Snapshot { .. }),
            "first batch must be the snapshot variant, was: {:?}",
            batches[0]
        );
    }

    #[test]
    fn no_diff_precedes_the_snapshot() {
        let batches = snapshot_then_diff();
        let first_snapshot = batches
            .iter()
            .position(|b| matches!(b, DemoBatch::Snapshot { .. }))
            .expect("a snapshot must be present");
        // No Diff batch may appear before the first Snapshot.
        assert!(
            !batches[..first_snapshot]
                .iter()
                .any(|b| matches!(b, DemoBatch::Diff { .. })),
            "a diff batch was emitted before the snapshot"
        );
    }

    #[test]
    fn subsequent_batches_are_diffs() {
        let batches = snapshot_then_diff();
        assert!(
            batches[1..]
                .iter()
                .all(|b| matches!(b, DemoBatch::Diff { .. })),
            "every batch after the snapshot must be a diff"
        );
    }
}

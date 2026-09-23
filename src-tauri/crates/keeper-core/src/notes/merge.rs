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

/// The scope members that narrow `drive` (AD-306): those that live on it, or
/// all of them when none does. Scope order is kept.
///
/// "All of them" is FR-629 generalised: one space still applies across every
/// selected drive, and a drive no selected space lives on is not left
/// unscoped just because the scope was chosen elsewhere.
pub fn for_drive<'a, T, F>(
    scope: &'a [T],
    drive: &'a str,
    drive_of: F,
) -> impl Iterator<Item = &'a T> + 'a
where
    F: Fn(&T) -> &str + Copy + 'a,
{
    let own = scope.iter().any(|member| drive_of(member) == drive);
    scope
        .iter()
        .filter(move |member| narrows(own, drive_of(member), drive))
}

/// Whether a member living on `home` narrows `drive`, given whether `drive`
/// has members of its own ([`for_drive`]'s rule, in one place).
fn narrows(own: bool, home: &str, drive: &str) -> bool {
    !own || home == drive
}

/// A scope whose members are resolved — read off their notes, parsed — only
/// when a searched drive first needs them (AD-306).
///
/// Each member is resolved at most once per list, however many drives it
/// narrows. A member no searched drive needs is never resolved, so it cannot
/// fail anything; a member that cannot be resolved fails only the drives that
/// need it, and the others list normally.
pub struct DriveScope<'s, T, F, L, E> {
    scope: &'s [T],
    drive_of: F,
    resolved: Vec<Option<Result<L, E>>>,
}

impl<'s, T, F, L, E> DriveScope<'s, T, F, L, E>
where
    F: Fn(&T) -> &str + Copy,
{
    pub fn new(scope: &'s [T], drive_of: F) -> Self {
        Self {
            scope,
            drive_of,
            resolved: std::iter::repeat_with(|| None).take(scope.len()).collect(),
        }
    }

    /// The resolved members that narrow `drive` ([`for_drive`]), in scope
    /// order — or the first of them, in scope order, that could not be
    /// resolved, with its error, so the caller can say which space kept which
    /// drive out of the list.
    pub fn for_drive(
        &mut self,
        drive: &str,
        mut resolve: impl FnMut(&T) -> Result<L, E>,
    ) -> Result<Vec<&L>, (&'s T, &E)> {
        let drive_of = self.drive_of;
        let own = self.scope.iter().any(|member| drive_of(member) == drive);
        let picked = |member: &T| narrows(own, drive_of(member), drive);
        for (member, slot) in self.scope.iter().zip(&mut self.resolved) {
            if picked(member) && slot.get_or_insert_with(|| resolve(member)).is_err() {
                break;
            }
        }
        let mut lenses = Vec::new();
        for (member, slot) in self.scope.iter().zip(&self.resolved) {
            match slot {
                Some(Ok(lens)) if picked(member) => lenses.push(lens),
                Some(Err(error)) if picked(member) => return Err((member, error)),
                _ => {}
            }
        }
        Ok(lenses)
    }

    /// The first member, in scope order, that some drive resolved: what orders
    /// a union across drives.
    pub fn first(&self) -> Option<&L> {
        self.resolved
            .iter()
            .find_map(|slot| slot.as_ref()?.as_ref().ok())
    }
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

    #[test]
    fn a_drive_is_narrowed_by_its_own_spaces_or_by_all_of_them() {
        let scope = [("x", "a"), ("y", "b"), ("z", "a")];
        let narrowing = |drive: &str| {
            for_drive(&scope, drive, |member| member.1)
                .map(|member| member.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(narrowing("a"), ["x", "z"]);
        assert_eq!(narrowing("b"), ["y"]);
        assert_eq!(narrowing("c"), ["x", "y", "z"], "scope order is kept");
        let empty: [(&str, &str); 0] = [];
        assert_eq!(for_drive(&empty, "a", |member| member.1).count(), 0);
    }

    /// A scope member in these tests: its name and the drive it lives on.
    type Member = (&'static str, &'static str);

    fn home(member: &Member) -> &str {
        member.1
    }

    #[test]
    fn a_member_fails_only_the_drives_that_need_it_and_is_resolved_once() {
        // x and z live on a, y on b, broken on c.
        let scope: [Member; 4] = [("x", "a"), ("y", "b"), ("broken", "c"), ("z", "a")];
        let mut calls = Vec::new();
        let mut members = DriveScope::new(&scope, home);
        let mut resolve = |member: &Member| {
            calls.push(member.0);
            if member.0 == "broken" {
                Err("unreadable")
            } else {
                Ok(member.0.to_uppercase())
            }
        };

        assert_eq!(
            members.for_drive("a", &mut resolve).expect("a lists"),
            ["X", "Z"],
            "a drive with its own members needs no other"
        );
        assert_eq!(
            members.for_drive("b", &mut resolve).expect("b lists"),
            ["Y"]
        );
        assert_eq!(
            members.for_drive("c", &mut resolve),
            Err((&("broken", "c"), &"unreadable")),
            "the member c needs could not be read, so c alone fails"
        );
        // A drive nobody lives on needs every member; the broken one fails it,
        // and nothing already resolved is resolved again.
        assert_eq!(
            members.for_drive("d", &mut resolve),
            Err((&("broken", "c"), &"unreadable"))
        );
        assert_eq!(calls, ["x", "z", "y", "broken"]);
    }

    #[test]
    fn a_member_no_searched_drive_needs_is_never_resolved() {
        let scope: [Member; 2] = [("x", "a"), ("gone", "b")];
        let mut members = DriveScope::new(&scope, home);
        let lenses = members
            .for_drive("a", |member| {
                if member.0 == "gone" {
                    Err("unmounted")
                } else {
                    Ok(member.0)
                }
            })
            .expect("b's member cannot fail a");
        assert_eq!(lenses, [&"x"]);
    }

    #[test]
    fn a_union_is_ordered_by_its_first_member_that_resolved() {
        let scope: [Member; 3] = [("gone", "c"), ("y", "b"), ("x", "a")];
        let mut members = DriveScope::new(&scope, home);
        let resolve = |member: &Member| match member.0 {
            "gone" => Err(()),
            name => Ok(name),
        };
        assert_eq!(members.first(), None, "nothing resolved yet");
        members.for_drive("a", resolve).expect("a lists");
        assert_eq!(members.first(), Some(&"x"));
        members.for_drive("b", resolve).expect("b lists");
        assert_eq!(
            members.first(),
            Some(&"y"),
            "scope order, not resolution order"
        );
    }
}

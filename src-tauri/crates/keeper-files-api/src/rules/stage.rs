//! Steps 8–9: each operation against the working state, then the final
//! state's rules and the bound.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use serde_json::{json, Value};

use super::{helpers, Manifest, Staged, StagedOp};
use crate::caller::{parse_file_part, Caller, FileClass};
use crate::ids;
use crate::names::{self, FileMode};
use crate::refusal::{Code, Refusal};
use crate::rows::{Entry, Kind, Lifecycle};
use crate::wire::WireOp;
use crate::{CHANGE_LIMIT, MAX_DEPTH};

/// Steps 8–9. `import` marks a `files.import` (its file rows take
/// `mtime_ms` = `event_time_ms` and their mode from `git_mode`).
pub(crate) fn stage(
    m: &Manifest,
    caller: &Caller,
    ops: &[WireOp],
    import: bool,
    event_time_ms: Option<i64>,
    reversal: bool,
) -> Result<Staged, Refusal> {
    let mut work: BTreeMap<String, Entry> = BTreeMap::new();
    let mut steps: Vec<StagedOp> = Vec::with_capacity(ops.len());
    let mut charged: usize = 0;
    let mut m_children: Option<HashMap<&str, Vec<&str>>> = None;

    for op in ops {
        let (before, carried, op_charge) = apply_op(
            m,
            &mut work,
            &mut m_children,
            op,
            import,
            event_time_ms,
            reversal,
        )?;
        charged = charged.saturating_add(op_charge);
        steps.push(StagedOp {
            op: op.clone(),
            before,
            carried,
        });
    }

    check_move_into_self(m, &work, &steps)?;
    check_live_parents(m, &work, &steps, reversal)?;
    check_name_uniqueness(m, &work, &steps)?;
    check_not_empty(m, &work, &steps, reversal)?;
    check_contents(m, caller, ops)?;
    if charged > CHANGE_LIMIT {
        return Err(Refusal::new(Code::ChangeLimit));
    }

    Ok(Staged { rows: work, steps })
}

fn parent_invalid(reversal: bool, parent_id: &str) -> Refusal {
    if reversal {
        Refusal::with_detail(
            Code::Conflict,
            json!({
                "entity_id": parent_id,
                "precondition": "dependency",
            }),
        )
    } else {
        Refusal::new(Code::ParentInvalid)
    }
}

fn lookup<'a>(m: &'a Manifest, work: &'a BTreeMap<String, Entry>, id: &str) -> Option<&'a Entry> {
    work.get(id).or_else(|| m.entries.get(id))
}

fn require_entry(
    m: &Manifest,
    work: &BTreeMap<String, Entry>,
    id: &str,
    expected_revision: &str,
    expected_lifecycle: Lifecycle,
) -> Result<Entry, Refusal> {
    let Some(entry) = lookup(m, work, id) else {
        return Err(Refusal::with_detail(
            Code::Conflict,
            json!({
                "entity_id": id,
                "precondition": "revision",
                "current_revision": Value::Null,
            }),
        ));
    };
    if entry.revision != expected_revision {
        return Err(Refusal::with_detail(
            Code::Conflict,
            json!({
                "entity_id": id,
                "precondition": "revision",
                "current_revision": entry.revision,
            }),
        ));
    }
    if entry.lifecycle != expected_lifecycle {
        return Err(Refusal::with_detail(
            Code::Conflict,
            json!({
                "entity_id": id,
                "precondition": "lifecycle",
                "current_revision": entry.revision,
            }),
        ));
    }
    Ok(entry.clone())
}

fn apply_op<'a>(
    m: &'a Manifest,
    work: &mut BTreeMap<String, Entry>,
    m_children: &mut Option<HashMap<&'a str, Vec<&'a str>>>,
    op: &WireOp,
    import: bool,
    event_time_ms: Option<i64>,
    reversal: bool,
) -> Result<(Option<Entry>, Vec<String>, usize), Refusal> {
    match op {
        WireOp::Create {
            id,
            kind,
            parent_id,
            name,
            content_id,
            size,
            mtime_ms,
            mode,
            git_mode,
            ..
        } => {
            if let Some(existing) = lookup(m, work, id) {
                return Err(Refusal::with_detail(
                    Code::Conflict,
                    json!({
                        "entity_id": id,
                        "precondition": "absent",
                        "current_revision": existing.revision,
                    }),
                ));
            }
            let (row_parent_id, drive_key) = match parent_id {
                None => {
                    if *kind != Kind::Dir {
                        return Err(Refusal::new(Code::InvalidChange));
                    }
                    (id.clone(), ids::drive_key(&m.domain, name))
                }
                Some(pid) => {
                    let Some(parent) = lookup(m, work, pid) else {
                        return Err(parent_invalid(reversal, pid));
                    };
                    if parent.lifecycle != Lifecycle::Live || parent.kind != Kind::Dir {
                        return Err(parent_invalid(reversal, pid));
                    }
                    (pid.clone(), parent.drive_key.clone())
                }
            };
            let (row_content_id, row_size, row_media_type, row_mtime_ms, row_mode) = match kind {
                Kind::File => {
                    let Some(cid) = content_id.clone() else {
                        return Err(Refusal::new(Code::InvalidChange));
                    };
                    let Some(sz) = *size else {
                        return Err(Refusal::new(Code::InvalidChange));
                    };
                    let file_mode = if import {
                        let Some(gm) = git_mode.as_deref() else {
                            return Err(Refusal::new(Code::InvalidChange));
                        };
                        FileMode::from_git(gm)?
                    } else {
                        mode.unwrap_or_default()
                    };
                    (
                        Some(cid),
                        Some(sz),
                        Some(names::media_type(name).to_owned()),
                        if import { event_time_ms } else { *mtime_ms },
                        file_mode,
                    )
                }
                Kind::Dir => (None, None, None, None, FileMode::Normal),
            };
            let entry = Entry {
                id: id.clone(),
                domain: m.domain.clone(),
                drive_key,
                parent_id: row_parent_id,
                name: name.clone(),
                name_fold: names::fold(name),
                kind: *kind,
                content_id: row_content_id,
                size: row_size,
                media_type: row_media_type,
                mtime_ms: row_mtime_ms,
                mode: row_mode,
                revision: String::new(),
                lifecycle: Lifecycle::Live,
                created_event_id: String::new(),
                updated_event_id: String::new(),
                updated_seq: String::new(),
            };
            work.insert(id.clone(), entry);
            Ok((None, Vec::new(), 1))
        }
        WireOp::Replace {
            id,
            expected_revision,
            content_id,
            size,
            mtime_ms,
            mode,
            git_mode,
            ..
        } => {
            let before = require_entry(m, work, id, expected_revision, Lifecycle::Live)?;
            if before.kind != Kind::File {
                return Err(Refusal::new(Code::InvalidChange));
            }
            let next_mode = if import {
                let Some(gm) = git_mode.as_deref() else {
                    return Err(Refusal::new(Code::InvalidChange));
                };
                FileMode::from_git(gm)?
            } else {
                mode.unwrap_or(before.mode)
            };
            let mut after = before.clone();
            after.content_id = Some(content_id.clone());
            after.size = Some(*size);
            after.mtime_ms = if import { event_time_ms } else { *mtime_ms };
            after.mode = next_mode;
            work.insert(id.clone(), after);
            Ok((Some(before), Vec::new(), 1))
        }
        WireOp::Move {
            id,
            expected_revision,
            parent_id,
            name,
            ..
        } => {
            let before = require_entry(m, work, id, expected_revision, Lifecycle::Live)?;
            if before.is_root() {
                return Err(Refusal::new(Code::InvalidChange));
            }
            let Some(parent) = lookup(m, work, parent_id) else {
                return Err(parent_invalid(reversal, parent_id));
            };
            if parent.lifecycle != Lifecycle::Live || parent.kind != Kind::Dir {
                return Err(parent_invalid(reversal, parent_id));
            }
            let new_drive_key = parent.drive_key.clone();
            let drive_changed = new_drive_key != before.drive_key;

            let mut after = before.clone();
            after.parent_id = parent_id.clone();
            after.name = name.clone();
            after.name_fold = names::fold(name);
            if after.kind == Kind::File {
                after.media_type = Some(names::media_type(name).to_owned());
            }
            after.drive_key = new_drive_key.clone();
            work.insert(id.clone(), after);

            let carried = if drive_changed && before.kind == Kind::Dir {
                let carried_ids = collect_descendants(m, work, m_children, id);
                for cid in &carried_ids {
                    if let Some(entry) = work.get_mut(cid) {
                        entry.drive_key = new_drive_key.clone();
                    } else if let Some(existing) = m.entries.get(cid) {
                        let mut cloned = existing.clone();
                        cloned.drive_key = new_drive_key.clone();
                        work.insert(cid.clone(), cloned);
                    }
                }
                carried_ids
            } else {
                Vec::new()
            };

            let op_charge = if drive_changed {
                carried.len().saturating_add(1).saturating_mul(2)
            } else {
                1
            };
            Ok((Some(before), carried, op_charge))
        }
        WireOp::Delete {
            id,
            expected_revision,
            ..
        } => {
            let before = require_entry(m, work, id, expected_revision, Lifecycle::Live)?;
            let mut after = before.clone();
            after.lifecycle = Lifecycle::Deleted;
            work.insert(id.clone(), after);
            Ok((Some(before), Vec::new(), 1))
        }
        WireOp::Restore {
            id,
            expected_revision,
            ..
        } => {
            let before = require_entry(m, work, id, expected_revision, Lifecycle::Deleted)?;
            let mut after = before.clone();
            after.lifecycle = Lifecycle::Live;
            work.insert(id.clone(), after);
            Ok((Some(before), Vec::new(), 1))
        }
    }
}

fn build_children_index(m: &Manifest) -> HashMap<&str, Vec<&str>> {
    let mut index: HashMap<&str, Vec<&str>> = HashMap::new();
    for entry in m.entries.values() {
        if !entry.is_root() {
            index
                .entry(entry.parent_id.as_str())
                .or_default()
                .push(entry.id.as_str());
        }
    }
    index
}

fn collect_descendants<'a>(
    m: &'a Manifest,
    work: &BTreeMap<String, Entry>,
    m_children: &mut Option<HashMap<&'a str, Vec<&'a str>>>,
    root_id: &str,
) -> Vec<String> {
    let index = m_children.get_or_insert_with(|| build_children_index(m));
    let mut work_children: HashMap<&str, Vec<&str>> = HashMap::new();
    for entry in work.values() {
        if !entry.is_root() {
            work_children
                .entry(entry.parent_id.as_str())
                .or_default()
                .push(entry.id.as_str());
        }
    }

    let mut visited = BTreeSet::new();
    visited.insert(root_id.to_owned());
    let mut carried = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(root_id.to_owned());

    while let Some(cur) = queue.pop_front() {
        if let Some(children) = index.get(cur.as_str()) {
            for &child_id in children {
                if !work.contains_key(child_id) && visited.insert(child_id.to_owned()) {
                    carried.insert(child_id.to_owned());
                    queue.push_back(child_id.to_owned());
                }
            }
        }
        if let Some(children) = work_children.get(cur.as_str()) {
            for &child_id in children {
                if visited.insert(child_id.to_owned()) {
                    carried.insert(child_id.to_owned());
                    queue.push_back(child_id.to_owned());
                }
            }
        }
    }

    carried.into_iter().collect()
}

fn check_move_into_self(
    m: &Manifest,
    work: &BTreeMap<String, Entry>,
    steps: &[StagedOp],
) -> Result<(), Refusal> {
    for step in steps {
        let WireOp::Move { id, .. } = &step.op else {
            continue;
        };
        let Some(entry) = work.get(id) else {
            continue;
        };
        if entry.kind != Kind::Dir {
            continue;
        }
        let mut cur = entry.parent_id.as_str();
        for _ in 0..=MAX_DEPTH {
            if cur == id {
                return Err(Refusal::new(Code::MoveIntoSelf));
            }
            let Some(ancestor) = lookup(m, work, cur) else {
                break;
            };
            if ancestor.is_root() {
                break;
            }
            cur = ancestor.parent_id.as_str();
        }
    }
    Ok(())
}

fn check_live_parents(
    m: &Manifest,
    work: &BTreeMap<String, Entry>,
    steps: &[StagedOp],
    reversal: bool,
) -> Result<(), Refusal> {
    for step in steps {
        let Some(entry) = work.get(step.op.id()) else {
            continue;
        };
        if entry.lifecycle != Lifecycle::Live || entry.is_root() {
            continue;
        }
        let Some(parent) = lookup(m, work, &entry.parent_id) else {
            return Err(parent_invalid(reversal, &entry.parent_id));
        };
        if parent.lifecycle != Lifecycle::Live || parent.kind != Kind::Dir {
            return Err(parent_invalid(reversal, &entry.parent_id));
        }
    }
    Ok(())
}

fn check_name_uniqueness(
    m: &Manifest,
    work: &BTreeMap<String, Entry>,
    steps: &[StagedOp],
) -> Result<(), Refusal> {
    let mut has_touched_root = false;
    let mut touched_parents: HashSet<&str> = HashSet::new();
    for step in steps {
        let Some(entry) = work.get(step.op.id()) else {
            continue;
        };
        if entry.lifecycle != Lifecycle::Live {
            continue;
        }
        if entry.is_root() {
            has_touched_root = true;
        } else {
            touched_parents.insert(entry.parent_id.as_str());
        }
    }
    if !has_touched_root && touched_parents.is_empty() {
        return Ok(());
    }

    let mut live_roots: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut live_siblings: HashMap<(&str, &str), Vec<&str>> = HashMap::new();
    let all_live = work
        .values()
        .chain(
            m.entries
                .iter()
                .filter(|(id, _)| !work.contains_key(*id))
                .map(|(_, entry)| entry),
        )
        .filter(|entry| entry.lifecycle == Lifecycle::Live);

    for entry in all_live {
        if entry.is_root() {
            if has_touched_root {
                live_roots
                    .entry(entry.name_fold.as_str())
                    .or_default()
                    .push(entry.id.as_str());
            }
        } else if touched_parents.contains(entry.parent_id.as_str()) {
            live_siblings
                .entry((entry.parent_id.as_str(), entry.name_fold.as_str()))
                .or_default()
                .push(entry.id.as_str());
        }
    }

    for step in steps {
        let Some(entry) = work.get(step.op.id()) else {
            continue;
        };
        if entry.lifecycle != Lifecycle::Live {
            continue;
        }
        let matches = if entry.is_root() {
            live_roots.get(entry.name_fold.as_str())
        } else {
            live_siblings.get(&(entry.parent_id.as_str(), entry.name_fold.as_str()))
        };
        if let Some(other_id) = matches.and_then(|ids| {
            ids.iter()
                .copied()
                .filter(|&id| id != entry.id.as_str())
                .min()
        }) {
            return Err(Refusal::with_detail(
                Code::Conflict,
                json!({
                    "entity_id": other_id,
                    "precondition": "name",
                }),
            ));
        }
    }
    Ok(())
}

fn check_not_empty(
    m: &Manifest,
    work: &BTreeMap<String, Entry>,
    steps: &[StagedOp],
    reversal: bool,
) -> Result<(), Refusal> {
    let mut deleted_dirs: HashSet<&str> = HashSet::new();
    for step in steps {
        if let Some(entry) = work.get(step.op.id()) {
            if entry.kind == Kind::Dir && entry.lifecycle == Lifecycle::Deleted {
                deleted_dirs.insert(entry.id.as_str());
            }
        }
    }
    if deleted_dirs.is_empty() {
        return Ok(());
    }
    let all_entries = work.values().chain(
        m.entries
            .iter()
            .filter(|(id, _)| !work.contains_key(*id))
            .map(|(_, entry)| entry),
    );
    let mut blocking_child: Option<&str> = None;
    for entry in all_entries {
        if entry.lifecycle == Lifecycle::Live
            && !entry.is_root()
            && deleted_dirs.contains(entry.parent_id.as_str())
        {
            if !reversal {
                return Err(Refusal::new(Code::NotEmpty));
            }
            let id = entry.id.as_str();
            blocking_child = Some(match blocking_child {
                Some(current) => current.min(id),
                None => id,
            });
        }
    }
    if let Some(child_id) = blocking_child {
        return Err(Refusal::with_detail(
            Code::Conflict,
            json!({
                "entity_id": child_id,
                "precondition": "dependency",
            }),
        ));
    }
    Ok(())
}

fn check_contents(m: &Manifest, caller: &Caller, ops: &[WireOp]) -> Result<(), Refusal> {
    let mut cids: Vec<&str> = Vec::new();
    for op in ops {
        match op {
            WireOp::Create {
                content_id: Some(cid),
                ..
            } => cids.push(cid.as_str()),
            WireOp::Replace { content_id, .. } => cids.push(content_id.as_str()),
            _ => {}
        }
    }
    for &cid in &cids {
        if !m.objects.contains(cid) {
            return Err(Refusal::new(Code::ContentAbsent));
        }
    }
    let (files, verified_uploads) = match caller {
        Caller::Agent {
            files,
            verified_uploads,
            ..
        }
        | Caller::Tool {
            files,
            verified_uploads,
            ..
        }
        | Caller::Job {
            files,
            verified_uploads,
            ..
        } => (files.as_slice(), verified_uploads.as_slice()),
        Caller::Person { .. } | Caller::Import { .. } => return Ok(()),
    };
    let verified: HashSet<&str> = verified_uploads.iter().map(String::as_str).collect();
    let mut needed: HashSet<&str> = cids
        .into_iter()
        .filter(|cid| !verified.contains(cid))
        .collect();
    if needed.is_empty() {
        return Ok(());
    }
    let parsed = parse_file_part(files)?;
    let read_roots: Vec<&str> = parsed
        .iter()
        .filter(|item| item.classes.contains(&FileClass::Read))
        .flat_map(|item| item.roots.iter().map(String::as_str))
        .collect();
    if !read_roots.is_empty() {
        for entry in m.entries.values() {
            if entry.lifecycle != Lifecycle::Live {
                continue;
            }
            let Some(cid) = entry.content_id.as_deref() else {
                continue;
            };
            if !needed.contains(cid) {
                continue;
            }
            if read_roots
                .iter()
                .any(|root| helpers::is_inside(m, &entry.id, root))
            {
                needed.remove(cid);
                if needed.is_empty() {
                    return Ok(());
                }
            }
        }
    }
    Err(Refusal::new(Code::ContentAbsent))
}

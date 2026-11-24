//! Read-only rules: canonical bytes, the event-row check, roots, listings.

use std::fmt::Write as _;

use keeper_accounts_api::grant::FileItem;
use serde::Deserialize as _;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Manifest;
use crate::caller::{self, strictness};
use crate::refusal::{Code, Refusal};
use crate::rows::{
    ApprovalBasis, EntityType, Entry, Event, Kind, Lifecycle, Op, Operation, OriginClass,
};

/// RFC 8785 canonical text of `v` (keys sorted by UTF-16 code units, no
/// whitespace, integers only).
#[must_use]
pub fn canonical(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            let _ = write!(out, "{n}");
        }
        Value::String(s) => {
            if let Ok(escaped) = serde_json::to_string(s) {
                out.push_str(&escaped);
            }
        }
        Value::Array(items) => {
            out.push('[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            for (idx, (key, val)) in entries.into_iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                if let Ok(escaped) = serde_json::to_string(key) {
                    out.push_str(&escaped);
                }
                out.push(':');
                write_canonical(val, out);
            }
            out.push('}');
        }
    }
}

/// Lowercase hex SHA-256 of `canonical(v)`.
#[must_use]
pub(crate) fn digest_hex(v: &Value) -> String {
    let bytes = Sha256::digest(canonical(v).as_bytes());
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Parses `event` as `rows::Event` and `ops` as `Vec<rows::Op>` (failure:
/// `invalid_change`), then the event-row check.
pub fn check_event(event: &Value, ops: &Value) -> Result<(), Refusal> {
    let event = Event::deserialize(event).map_err(|_| Refusal::new(Code::InvalidChange))?;
    let ops = Vec::<Op>::deserialize(ops).map_err(|_| Refusal::new(Code::InvalidChange))?;

    let has_job =
        event.job_id.is_some() || event.job_ordinal.is_some() || event.job_attempt_id.is_some();
    if has_job && event.origin_class != OriginClass::Job {
        return Err(Refusal::new(Code::InvalidChange));
    }

    if matches!(event.approval_basis, ApprovalBasis::Release { .. })
        && (event.approval_id.is_some()
            || event.origin_chain.is_some()
            || event.context_id.is_some())
    {
        return Err(Refusal::new(Code::ReservedField));
    }

    if u32::try_from(ops.len()).ok() != Some(event.op_count) {
        return Err(Refusal::new(Code::InvalidChange));
    }

    for op in &ops {
        if op.entity_type == EntityType::Review && op.operation != Operation::Create {
            return Err(Refusal::new(Code::InvalidChange));
        }
    }

    Ok(())
}

/// Whether `root_id` is a live `dir` of `m` that is `entry_id` or one of
/// its ancestors by `parent_id` (at most `MAX_DEPTH` steps; the walk stops
/// at a drive root).
#[must_use]
pub fn is_inside(m: &Manifest, entry_id: &str, root_id: &str) -> bool {
    let Some(root) = m.entries.get(root_id) else {
        return false;
    };
    if root.lifecycle != Lifecycle::Live || root.kind != Kind::Dir {
        return false;
    }

    let mut current = entry_id;
    for _ in 0..=crate::MAX_DEPTH {
        if current == root_id {
            return true;
        }
        let Some(row) = m.entries.get(current) else {
            return false;
        };
        if row.is_root() {
            return false;
        }
        current = row.parent_id.as_str();
    }
    false
}

/// The live children of `parent_id` ordered by `name_fold` then `id`; a
/// drive root is never in its own listing.
#[must_use]
pub fn list<'a>(m: &'a Manifest, parent_id: &str) -> Vec<&'a Entry> {
    let mut entries: Vec<&'a Entry> = m
        .entries
        .values()
        .filter(|entry| {
            entry.lifecycle == Lifecycle::Live && entry.parent_id == parent_id && !entry.is_root()
        })
        .collect();
    entries.sort_by(|a, b| a.name_fold.cmp(&b.name_fold).then_with(|| a.id.cmp(&b.id)));
    entries
}

/// The drive-relative path of an entry (`""` for a root), joined from
/// `name`s walking `parent_id`; `None` if the walk breaks or exceeds
/// `MAX_DEPTH`, or `keeper_paths::plain_segments` refuses the result.
#[must_use]
pub fn path_of(m: &Manifest, entry_id: &str) -> Option<String> {
    let mut segments = Vec::new();
    let mut current = entry_id;
    let mut reached_root = false;
    for _ in 0..=crate::MAX_DEPTH {
        let row = m.entries.get(current)?;
        if row.is_root() {
            reached_root = true;
            break;
        }
        segments.push(row.name.as_str());
        current = row.parent_id.as_str();
    }
    if !reached_root {
        return None;
    }
    if segments.is_empty() {
        return Some(String::new());
    }
    segments.reverse();
    let path = segments.join("/");
    keeper_paths::plain_segments(&path).ok()?;
    Some(path)
}

/// Whether `given` is narrower than or equal to `held` (K-5).
pub fn narrower_or_equal(
    m: &Manifest,
    given_domain: &str,
    given: &[FileItem],
    held_domain: &str,
    held: &[FileItem],
) -> Result<bool, Refusal> {
    let given_items = caller::parse_file_part(given)?;
    let held_items = caller::parse_file_part(held)?;
    if given_domain != held_domain {
        return Ok(false);
    }
    Ok(given_items.iter().all(|g| {
        held_items.iter().any(|h| {
            strictness(g.mode) >= strictness(h.mode)
                && g.classes.iter().all(|c| h.classes.contains(c))
                && g.roots
                    .iter()
                    .all(|g_root| h.roots.iter().any(|h_root| is_inside(m, g_root, h_root)))
        })
    }))
}

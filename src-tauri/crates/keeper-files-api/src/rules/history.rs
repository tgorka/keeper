//! Reference `files.history` query over a manifest's retained events.

use std::cmp::Ordering;

use serde_json::Value;

use super::parse::check_version;
use super::Manifest;
use crate::history::{HistoryItem, HistoryPage, HistoryParams, LIMIT_MAX};
use crate::refusal::{Code, Refusal};
use crate::rows::{Event, Op};

/// Evaluates a `files.history` query against `m`.
///
/// Validates `version` before parsing the rest of `params`, checks `after`,
/// `limit`, and filter strings, then returns up to `limit` matching events
/// with `seq > after` ordered numerically by `seq`.
pub fn history(m: &Manifest, params: &Value) -> Result<HistoryPage, Refusal> {
    check_version(params)?;
    let params: HistoryParams =
        serde_json::from_value(params.clone()).map_err(|_| Refusal::new(Code::InvalidChange))?;

    if !is_canonical_decimal(&params.after)
        || !(1..=LIMIT_MAX).contains(&params.limit)
        || params
            .filter
            .entity_id
            .as_deref()
            .is_some_and(str::is_empty)
        || params.filter.actor_id.as_deref().is_some_and(str::is_empty)
    {
        return Err(Refusal::new(Code::InvalidChange));
    }

    let limit = usize::try_from(params.limit).map_err(|_| Refusal::new(Code::InvalidChange))?;
    let entity_filter = params.filter.entity_id.as_deref();
    let actor_filter = params.filter.actor_id.as_deref();
    let origin_filter = params.filter.origin_class;

    let mut matched: Vec<(&Event, &Vec<Op>)> = m
        .events
        .values()
        .filter(|(event, ops)| {
            cmp_decimal(&event.seq, &params.after).is_gt()
                && actor_filter.is_none_or(|actor| event.actor_id == actor)
                && origin_filter.is_none_or(|origin| event.origin_class == origin)
                && entity_filter.is_none_or(|entity| ops.iter().any(|op| op.entity_id == entity))
        })
        .map(|(event, ops)| (event, ops))
        .collect();

    matched.sort_by(|(a, _), (b, _)| cmp_decimal(&a.seq, &b.seq));
    matched.truncate(limit);

    let events: Vec<HistoryItem> = matched
        .into_iter()
        .map(|(event, ops)| {
            let mut item_ops: Vec<Op> = match entity_filter {
                Some(entity) => ops
                    .iter()
                    .filter(|op| op.entity_id == entity)
                    .cloned()
                    .collect(),
                None => ops.clone(),
            };
            item_ops.sort_by_key(|op| op.ordinal);
            HistoryItem {
                event: event.clone(),
                ops: item_ops,
            }
        })
        .collect();

    let scanned_through = if events.len() == limit {
        events
            .last()
            .map_or_else(|| params.after.clone(), |item| item.event.seq.clone())
    } else {
        let head_seq = m.head.as_ref().map_or("0", |head| head.seq.as_str());
        if cmp_decimal(&params.after, head_seq).is_ge() {
            params.after
        } else {
            head_seq.to_owned()
        }
    };

    Ok(HistoryPage {
        events,
        scanned_through,
    })
}

fn is_canonical_decimal(s: &str) -> bool {
    if s == "0" {
        return true;
    }
    let bytes = s.as_bytes();
    matches!(bytes.first(), Some(b'1'..=b'9')) && bytes.iter().all(u8::is_ascii_digit)
}

fn cmp_decimal(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

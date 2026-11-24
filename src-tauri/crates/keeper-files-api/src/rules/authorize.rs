//! Step 7: who may make this change, and the basis it lands on.

use keeper_accounts_api::grant::GrantMode;
use keeper_accounts_api::Role;

use super::{Manifest, Request};
use crate::caller::{self, Caller, FileClass, FilePartItem};
use crate::names;
use crate::refusal::{Code, Refusal};
use crate::rows::ApprovalBasis;
use crate::wire::WireOp;

/// Step 7. `ops` are the request's operations (for `files.reverse`, the
/// inverse operations); `reverses` the reversed event's id.
pub(crate) fn authorize(
    m: &Manifest,
    caller: &Caller,
    request: &Request,
    ops: &[WireOp],
    reverses: Option<&str>,
) -> Result<ApprovalBasis, Refusal> {
    for (idx, op) in ops.iter().enumerate() {
        if let WireOp::Create {
            parent_id: None,
            name,
            ..
        } = op
        {
            if names::fold(name) == names::fold(crate::RESERVED_DRIVE) {
                return Err(Refusal::new(Code::ReservedName));
            }
            match caller {
                Caller::Person { role, .. } => {
                    if !matches!(role, Role::Owner) {
                        return Err(Refusal::new(Code::Forbidden));
                    }
                }
                Caller::Agent { .. } | Caller::Tool { .. } | Caller::Job { .. } => {
                    return Err(Refusal::new(Code::Forbidden));
                }
                Caller::Import { .. } => {
                    if idx != 0 || request.descriptor != "files.import" {
                        return Err(Refusal::new(Code::Forbidden));
                    }
                }
            }
        }
    }

    match caller {
        Caller::Person { role, .. } => {
            if matches!(role, Role::Reader) {
                return Err(Refusal::new(Code::Forbidden));
            }
            Ok(ApprovalBasis::Direct)
        }
        Caller::Import { .. } => Ok(ApprovalBasis::Import),
        Caller::Agent { grant, files, .. }
        | Caller::Tool { grant, files, .. }
        | Caller::Job { grant, files, .. } => {
            let items = caller::parse_file_part(files)?;
            let needed = if let Some(event_id) = reverses {
                match m.events.get(event_id) {
                    Some((event, _)) if event.actor_id == caller.actor_id() => {}
                    _ => return Err(Refusal::new(Code::OutsideRoots)),
                }
                FileClass::ReverseOwn
            } else {
                FileClass::Write
            };

            let mut change_mode: Option<GrantMode> = None;
            let mut check_location = |op_idx: usize, raw_id: &str| -> Result<(), Refusal> {
                let resolved = resolve_location(ops, op_idx, raw_id)
                    .ok_or_else(|| Refusal::new(Code::OutsideRoots))?;
                let mode = location_mode(m, &items, needed, resolved)?;
                change_mode = Some(match change_mode {
                    Some(current) => stricter(current, mode),
                    None => mode,
                });
                Ok(())
            };

            for (idx, op) in ops.iter().enumerate() {
                match op {
                    WireOp::Create { parent_id, .. } => {
                        let parent = parent_id
                            .as_deref()
                            .ok_or_else(|| Refusal::new(Code::Forbidden))?;
                        check_location(idx, parent)?;
                    }
                    WireOp::Replace { id, .. }
                    | WireOp::Delete { id, .. }
                    | WireOp::Restore { id, .. } => {
                        check_location(idx, id)?;
                    }
                    WireOp::Move { id, parent_id, .. } => {
                        check_location(idx, id)?;
                        check_location(idx, parent_id)?;
                    }
                }
            }

            let mode = change_mode.ok_or_else(|| Refusal::new(Code::OutsideRoots))?;
            if matches!(mode, GrantMode::Propose) {
                return Err(Refusal::new(Code::ProposeRequired));
            }
            Ok(ApprovalBasis::Grant {
                grant_id: grant.grant_id.clone(),
                grant_revision: grant.grant_revision.clone(),
                mode,
            })
        }
    }
}

/// Resolves `id` through any earlier `Create` in `ops` to a pre-change id.
fn resolve_location<'a>(ops: &'a [WireOp], op_idx: usize, mut id: &'a str) -> Option<&'a str> {
    let mut limit = op_idx;
    for _ in 0..=ops.len() {
        let found = ops[..limit]
            .iter()
            .enumerate()
            .find_map(|(i, op)| match op {
                WireOp::Create {
                    id: created_id,
                    parent_id,
                    ..
                } if created_id == id => Some((i, parent_id.as_deref())),
                _ => None,
            });
        match found {
            Some((i, Some(parent))) => {
                id = parent;
                limit = i;
            }
            Some((_, None)) => return None,
            None => return Some(id),
        }
    }
    None
}

fn location_mode(
    m: &Manifest,
    items: &[FilePartItem],
    needed: FileClass,
    location: &str,
) -> Result<GrantMode, Refusal> {
    let mut mode: Option<GrantMode> = None;
    for item in items {
        if !item.classes.contains(&needed) {
            continue;
        }
        if item
            .roots
            .iter()
            .any(|root| super::helpers::is_inside(m, location, root))
        {
            mode = Some(match mode {
                Some(current) => stricter(current, item.mode),
                None => item.mode,
            });
        }
    }
    mode.ok_or_else(|| Refusal::new(Code::OutsideRoots))
}

const fn stricter(a: GrantMode, b: GrantMode) -> GrantMode {
    if caller::strictness(a) >= caller::strictness(b) {
        a
    } else {
        b
    }
}

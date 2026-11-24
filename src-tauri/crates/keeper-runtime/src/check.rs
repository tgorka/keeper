//! Start-time validation and topological ordering of modules.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use keeper_runtime_api::{
    Bindings, Effect, Handler, Kind, Manifest, Module, OperationSpec, ProposalForm,
};

use crate::error::StartError;

pub(crate) struct CheckedEntry {
    pub spec: &'static OperationSpec,
    pub module: Arc<dyn Module>,
    pub handler: Handler,
}

pub(crate) struct CheckedBuild {
    pub present: Vec<&'static str>,
    pub absent: BTreeSet<String>,
    pub start_order: Vec<(&'static str, Arc<dyn Module>)>,
    pub operations: BTreeMap<&'static str, CheckedEntry>,
}

fn split_op_id(id: &str) -> Option<(&str, &str)> {
    let (prefix, verb) = id.split_once('.')?;
    if prefix.is_empty() || verb.is_empty() || verb.contains('.') {
        return None;
    }
    Some((prefix, verb))
}

pub(crate) fn validate(modules: Vec<Arc<dyn Module>>) -> Result<CheckedBuild, StartError> {
    let mut by_id: BTreeMap<&'static str, (Arc<dyn Module>, Manifest)> = BTreeMap::new();
    for module in modules {
        let manifest = module.manifest();
        if by_id.contains_key(manifest.id) {
            return Err(StartError::DuplicateModule {
                module: manifest.id.to_string(),
            });
        }
        by_id.insert(manifest.id, (module, manifest));
    }

    let mut operations: BTreeMap<&'static str, CheckedEntry> = BTreeMap::new();
    let mut global_ops: BTreeSet<&'static str> = BTreeSet::new();

    for (&mod_id, (module, manifest)) in &by_id {
        let mut module_ops: HashMap<&'static str, &'static OperationSpec> = HashMap::new();

        for spec in manifest.operations {
            let Some((prefix, _verb)) = split_op_id(spec.id) else {
                return Err(StartError::BadOperationId {
                    module: mod_id.to_string(),
                    id: spec.id.to_string(),
                });
            };
            if prefix != mod_id {
                return Err(StartError::ForeignOperation {
                    module: mod_id.to_string(),
                    id: spec.id.to_string(),
                });
            }
            if !global_ops.insert(spec.id) {
                return Err(StartError::DuplicateOperation {
                    module: mod_id.to_string(),
                    id: spec.id.to_string(),
                });
            }
            module_ops.insert(spec.id, spec);

            if !spec.wire_forbid.is_empty() || !spec.wire_fix.is_empty() {
                let schema = (spec.input)();
                for &pointer in spec.wire_forbid {
                    if !keeper_runtime_api::has_pointer(&schema, pointer) {
                        return Err(StartError::BadWireRule {
                            module: mod_id.to_string(),
                            id: spec.id.to_string(),
                            pointer: pointer.to_string(),
                        });
                    }
                }
                for &(pointer, _) in spec.wire_fix {
                    if !keeper_runtime_api::has_pointer(&schema, pointer) {
                        return Err(StartError::BadWireRule {
                            module: mod_id.to_string(),
                            id: spec.id.to_string(),
                            pointer: pointer.to_string(),
                        });
                    }
                }
            }
        }

        for spec in manifest.operations {
            match spec.proposal {
                ProposalForm::None => {}
                ProposalForm::Itself => {
                    if spec.effect != Effect::Write {
                        return Err(StartError::BadProposalForm {
                            module: mod_id.to_string(),
                            id: spec.id.to_string(),
                            reason: "ProposalForm::Itself requires Effect::Write".to_string(),
                        });
                    }
                }
                ProposalForm::Via(target_id) => {
                    if spec.to_proposal.is_none() {
                        return Err(StartError::BadProposalForm {
                            module: mod_id.to_string(),
                            id: spec.id.to_string(),
                            reason: format!("ProposalForm::Via({target_id}) requires to_proposal"),
                        });
                    }
                    let Some(target) = module_ops.get(target_id) else {
                        return Err(StartError::BadProposalForm {
                            module: mod_id.to_string(),
                            id: spec.id.to_string(),
                            reason: format!(
                                "ProposalForm::Via({target_id}) target is not an operation of module '{mod_id}'"
                            ),
                        });
                    };
                    if target.effect != Effect::Write || target.kind != Kind::Command {
                        return Err(StartError::BadProposalForm {
                            module: mod_id.to_string(),
                            id: spec.id.to_string(),
                            reason: format!(
                                "ProposalForm::Via({target_id}) target must be a write command"
                            ),
                        });
                    }
                }
            }
        }

        let mut bindings = Bindings::new();
        module.bind(&mut bindings);
        let mut bound_for_module: HashMap<&'static str, Handler> = HashMap::new();
        for (id, handler) in bindings.into_entries() {
            let Some(spec) = module_ops.get(id) else {
                return Err(StartError::UnknownBinding {
                    module: mod_id.to_string(),
                    id: id.to_string(),
                });
            };
            if bound_for_module.contains_key(id) {
                return Err(StartError::DuplicateBinding {
                    module: mod_id.to_string(),
                    id: id.to_string(),
                });
            }
            if handler.effect() != spec.effect {
                return Err(StartError::ClassMismatch {
                    module: mod_id.to_string(),
                    id: id.to_string(),
                    declared: spec.effect,
                    bound: handler.effect(),
                });
            }
            bound_for_module.insert(id, handler);
        }

        for spec in manifest.operations {
            let Some(handler) = bound_for_module.remove(spec.id) else {
                return Err(StartError::UnboundOperation {
                    module: mod_id.to_string(),
                    id: spec.id.to_string(),
                });
            };
            operations.insert(
                spec.id,
                CheckedEntry {
                    spec,
                    module: Arc::clone(module),
                    handler,
                },
            );
        }
    }

    let mut absent: BTreeSet<String> = BTreeSet::new();
    for (&mod_id, (_, manifest)) in &by_id {
        for req in manifest.requires {
            if !by_id.contains_key(req.module) {
                if req.optional {
                    absent.insert(req.module.to_string());
                } else {
                    return Err(StartError::MissingRequired {
                        module: mod_id.to_string(),
                        lacks: req.module.to_string(),
                    });
                }
            }
        }
    }

    let order_ids = topo_sort(&by_id)?;
    let present: Vec<&'static str> = by_id.keys().copied().collect();
    let start_order = order_ids
        .into_iter()
        .filter_map(|id| by_id.get(id).map(|(m, _)| (id, Arc::clone(m))))
        .collect();

    Ok(CheckedBuild {
        present,
        absent,
        start_order,
        operations,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Visiting,
    Visited,
}

fn topo_sort(
    by_id: &BTreeMap<&'static str, (Arc<dyn Module>, Manifest)>,
) -> Result<Vec<&'static str>, StartError> {
    let mut marks: HashMap<&'static str, Mark> = HashMap::new();
    let mut stack: Vec<&'static str> = Vec::new();
    let mut order: Vec<&'static str> = Vec::with_capacity(by_id.len());

    for &id in by_id.keys() {
        if !marks.contains_key(id) {
            dfs(id, by_id, &mut marks, &mut stack, &mut order)?;
        }
    }
    Ok(order)
}

fn dfs(
    id: &'static str,
    by_id: &BTreeMap<&'static str, (Arc<dyn Module>, Manifest)>,
    marks: &mut HashMap<&'static str, Mark>,
    stack: &mut Vec<&'static str>,
    order: &mut Vec<&'static str>,
) -> Result<(), StartError> {
    marks.insert(id, Mark::Visiting);
    stack.push(id);

    if let Some((_, manifest)) = by_id.get(id) {
        for req in manifest.requires {
            if !by_id.contains_key(req.module) {
                continue;
            }
            match marks.get(req.module).copied() {
                Some(Mark::Visiting) => {
                    let start = stack
                        .iter()
                        .position(|&entry| entry == req.module)
                        .unwrap_or(0);
                    let mut path: Vec<String> =
                        stack[start..].iter().map(|s| (*s).to_string()).collect();
                    path.push(req.module.to_string());
                    return Err(StartError::RequirementCycle { path });
                }
                Some(Mark::Visited) => {}
                None => dfs(req.module, by_id, marks, stack, order)?,
            }
        }
    }

    stack.pop();
    marks.insert(id, Mark::Visited);
    order.push(id);
    Ok(())
}

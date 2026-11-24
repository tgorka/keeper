//! Reading an operation's schemas: whether a pointer is in an input, the
//! enums a wire caller can reach (CR-6), and an operation's snapshot entry.

use serde_json::{json, Map, Value};

use crate::{Fixed, OperationSpec, ProposalForm, Scope};

/// Follow a local `$ref` (`#/$defs/<name>` or `#/definitions/<name>`).
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    let mut node = node;
    for _ in 0..32 {
        let Some(target) = node.get("$ref").and_then(Value::as_str) else {
            return node;
        };
        match target.strip_prefix('#').and_then(|p| root.pointer(p)) {
            Some(next) => node = next,
            None => return node,
        }
    }
    node
}

fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

fn branches(node: &Value) -> impl Iterator<Item = &Value> {
    ["allOf", "anyOf", "oneOf"]
        .into_iter()
        .filter_map(|k| node.get(k).and_then(Value::as_array))
        .flatten()
}

fn has_tokens(root: &Value, node: &Value, tokens: &[String], depth: usize) -> bool {
    let node = resolve(root, node);
    let Some((first, rest)) = tokens.split_first() else {
        return true;
    };
    if depth > 64 {
        return false;
    }
    if let Some(child) = node.get("properties").and_then(|p| p.get(first.as_str())) {
        if has_tokens(root, child, rest, depth + 1) {
            return true;
        }
    }
    if first == "-" || first.parse::<usize>().is_ok() {
        if let Some(items) = node.get("items") {
            if has_tokens(root, items, rest, depth + 1) {
                return true;
            }
        }
    }
    branches(node).any(|branch| has_tokens(root, branch, tokens, depth + 1))
}

/// Whether the JSON pointer `pointer` names a property the schema declares
/// (through `properties`, `items`, local `$ref`s and `allOf`/`anyOf`/`oneOf`).
#[must_use]
pub fn has_pointer(schema: &schemars::Schema, pointer: &str) -> bool {
    let root = schema.as_value();
    if pointer.is_empty() {
        return true;
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return false;
    };
    let tokens: Vec<String> = rest.split('/').map(unescape).collect();
    has_tokens(root, root, &tokens, 0)
}

fn collect(root: &Value, node: &Value, at: &str, depth: usize, out: &mut Map<String, Value>) {
    if depth > 64 {
        return;
    }
    let node = resolve(root, node);
    if let Some(values) = node.get("enum") {
        out.insert(at.to_owned(), values.clone());
    } else if let Some(value) = node.get("const") {
        // Several `const` branches of one `oneOf` are one enum.
        match out.get_mut(at) {
            Some(Value::Array(values)) => values.push(value.clone()),
            _ => {
                out.insert(at.to_owned(), json!([value]));
            }
        }
    }
    if let Some(props) = node.get("properties").and_then(Value::as_object) {
        for (key, child) in props {
            let escaped = key.replace('~', "~0").replace('/', "~1");
            collect(root, child, &format!("{at}/{escaped}"), depth + 1, out);
        }
    }
    if let Some(items) = node.get("items") {
        collect(root, items, &format!("{at}/-"), depth + 1, out);
    }
    for branch in branches(node) {
        collect(root, branch, at, depth + 1, out);
    }
}

/// Every enum (`enum`, or `const` branches) the schema declares, by the
/// pointer of the value it constrains (`/-` for an array's items).
#[must_use]
pub fn enums(schema: &schemars::Schema) -> Map<String, Value> {
    let mut out = Map::new();
    let root = schema.as_value();
    collect(root, root, "", 0, &mut out);
    out
}

fn fixed(value: Fixed) -> Value {
    match value {
        Fixed::DeviceActor => json!("device_actor"),
        Fixed::Null => json!("null"),
        Fixed::FreshUlid => json!("fresh_ulid"),
        Fixed::ExpiresIn(ms) => json!({ "expires_in_ms": ms }),
    }
}

/// An operation's entry in `catalogue/snapshot.json`: its schemas, its wire
/// rules, every enum of its input (CR-6) and the digest of each of its
/// vectors files, as `digest` answers for the file's repository-root path
/// (CR-5).
#[must_use]
pub fn snapshot_entry(spec: &OperationSpec, digest: &dyn Fn(&str) -> String) -> Value {
    let input = (spec.input)();
    let output = (spec.output)();
    let vectors: Map<String, Value> = spec
        .vectors
        .iter()
        .map(|path| ((*path).to_owned(), json!(digest(path))))
        .collect();
    json!({
        "id": spec.id,
        "kind": spec.kind.as_str(),
        "effect": spec.effect.as_str(),
        "summary": spec.summary,
        "scope": match spec.scope {
            Scope::App => json!("app"),
            Scope::Domain(pointer) => json!({ "domain": pointer }),
        },
        "proposal": match spec.proposal {
            ProposalForm::Itself => json!("itself"),
            ProposalForm::Via(id) => json!({ "via": id }),
            ProposalForm::None => json!("none"),
        },
        "input": input.as_value(),
        "output": output.as_value(),
        "input_enums": enums(&input),
        "concise": spec.concise,
        "page": spec.page.map(|p| json!({ "cursor": p.cursor, "limit": p.limit })),
        "cli": { "words": spec.cli.words, "positional": spec.cli.positional },
        "wire_forbid": spec.wire_forbid,
        "wire_fix": spec.wire_fix.iter()
            .map(|(pointer, value)| json!({ "pointer": pointer, "fixed": fixed(*value) }))
            .collect::<Vec<_>>(),
        "to_proposal": spec.to_proposal.is_some(),
        "wire_gate": spec.wire_gate.is_some(),
        "vectors": vectors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(value: Value) -> schemars::Schema {
        schemars::Schema::try_from(value).expect("an object schema")
    }

    #[test]
    fn pointers_follow_properties_refs_items_and_branches() {
        let s = schema(json!({
            "type": "object",
            "properties": {
                "change": { "$ref": "#/$defs/change" },
                "list": { "type": "array", "items": { "properties": { "x": {} } } },
            },
            "$defs": { "change": { "allOf": [ { "properties": { "approval_id": {} } } ] } },
        }));
        for pointer in [
            "",
            "/change",
            "/change/approval_id",
            "/list/0/x",
            "/list/-/x",
        ] {
            assert!(has_pointer(&s, pointer), "{pointer}");
        }
        for pointer in ["/nope", "/change/nope", "/list/x", "change"] {
            assert!(!has_pointer(&s, pointer), "{pointer}");
        }
    }

    #[test]
    fn enums_are_found_by_pointer() {
        let s = schema(json!({
            "properties": {
                "entity": { "enum": ["node", "edge"] },
                "verdict": { "oneOf": [ { "const": "approve" }, { "const": "reject" } ] },
                "items": { "items": { "$ref": "#/$defs/k" } },
            },
            "$defs": { "k": { "enum": [1, 2] } },
        }));
        let found = enums(&s);
        assert_eq!(found["/entity"], json!(["node", "edge"]));
        assert_eq!(found["/verdict"], json!(["approve", "reject"]));
        assert_eq!(found["/items/-"], json!([1, 2]));
    }
}

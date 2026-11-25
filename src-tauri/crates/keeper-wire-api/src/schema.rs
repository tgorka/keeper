//! The committed JSON Schema for `keeper.wire.v1` and the forward-compatibility
//! check between releases.

use serde_json::{json, Value};

use crate::{ClientFrame, DescribeOutput, ServerFrame, WIRE_V1};

/// The committed JSON Schema 2020-12 document for `keeper.wire.v1`.
pub const COMMITTED: &str = include_str!("../schema/keeper.wire.v1.json");

/// Generate the `keeper.wire.v1` frame schema directly from the Rust frame types.
#[must_use]
pub fn frame_schema() -> Value {
    json!({
        "title": WIRE_V1,
        "client": schemars::schema_for!(ClientFrame),
        "server": schemars::schema_for!(ServerFrame),
        "describe_output": schemars::schema_for!(DescribeOutput),
    })
}

/// Check that every frame accepted or produced under `old` remains valid under
/// `new`.
///
/// Within `keeper.wire.v1` the envelope only grows: adding an optional property
/// is allowed, while removing a property, changing its type/`$ref`/`const`, or
/// making a property newly required breaks compatibility.
///
/// # Errors
/// Returns one human-readable message per compatibility break, naming each
/// affected field by its schema path.
pub fn compatible(old: &Value, new: &Value) -> Result<(), Vec<String>> {
    let mut breaks = Vec::new();

    if let (Some(old_obj), Some(new_obj)) = (old.as_object(), new.as_object()) {
        let is_bundle = old_obj.contains_key("client")
            || old_obj.contains_key("server")
            || old_obj.contains_key("describe_output");
        if is_bundle {
            for section in ["client", "server", "describe_output"] {
                if let Some(old_section) = old_obj.get(section) {
                    if let Some(new_section) = new_obj.get(section) {
                        compare_node(old_section, new_section, section, &mut breaks);
                    } else {
                        breaks.push(format!("{section}: removed"));
                    }
                }
            }
        } else {
            compare_node(old, new, "", &mut breaks);
        }
    } else if old != new {
        breaks.push("schema root: retyped".to_owned());
    }

    if breaks.is_empty() {
        Ok(())
    } else {
        Err(breaks)
    }
}

fn join_path(base: &str, segment: &str) -> String {
    if base.is_empty() {
        segment.to_owned()
    } else {
        format!("{base}.{segment}")
    }
}

fn is_retyped(old: &Value, new: &Value) -> bool {
    old.as_bool() != new.as_bool()
        || old.get("type") != new.get("type")
        || old.get("$ref") != new.get("$ref")
        || old.get("const") != new.get("const")
}

fn type_summary(v: &Value) -> String {
    if let Some(b) = v.as_bool() {
        return format!("bool={b}");
    }
    if let Some(r) = v.get("$ref") {
        return format!("$ref={r}");
    }
    if let Some(c) = v.get("const") {
        return format!("const={c}");
    }
    if let Some(t) = v.get("type") {
        return format!("type={t}");
    }
    "untyped".to_owned()
}

fn variant_type_tag(v: &Value) -> Option<&str> {
    let ty = v.get("properties")?.get("type")?;
    if let Some(s) = ty.get("const").and_then(Value::as_str) {
        return Some(s);
    }
    let arr = ty.get("enum").and_then(Value::as_array)?;
    if arr.len() == 1 {
        return arr[0].as_str();
    }
    None
}

fn compare_node(old: &Value, new: &Value, path: &str, breaks: &mut Vec<String>) {
    // 1. Newly required fields.
    if let Some(new_req) = new.get("required").and_then(Value::as_array) {
        let old_req = old.get("required").and_then(Value::as_array);
        for item in new_req {
            if let Some(name) = item.as_str() {
                let was_required =
                    old_req.is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(name)));
                if !was_required {
                    let prop_path = join_path(&join_path(path, "properties"), name);
                    breaks.push(format!("{prop_path}: newly required"));
                }
            }
        }
    }

    // 2. Properties: removed, retyped, or recursively changed.
    if let Some(old_props) = old.get("properties").and_then(Value::as_object) {
        let new_props = new.get("properties").and_then(Value::as_object);
        let props_base = join_path(path, "properties");
        for (name, old_prop) in old_props {
            let prop_path = join_path(&props_base, name);
            let Some(new_prop) = new_props.and_then(|m| m.get(name)) else {
                breaks.push(format!("{prop_path}: removed"));
                continue;
            };
            if is_retyped(old_prop, new_prop) {
                breaks.push(format!(
                    "{prop_path}: retyped (was {}, now {})",
                    type_summary(old_prop),
                    type_summary(new_prop),
                ));
                continue;
            }
            compare_node(old_prop, new_prop, &prop_path, breaks);
        }
    }

    // 3. $defs.
    if let Some(old_defs) = old.get("$defs").and_then(Value::as_object) {
        let new_defs = new.get("$defs").and_then(Value::as_object);
        let defs_base = join_path(path, "$defs");
        for (name, old_def) in old_defs {
            let def_path = join_path(&defs_base, name);
            let Some(new_def) = new_defs.and_then(|m| m.get(name)) else {
                breaks.push(format!("{def_path}: removed"));
                continue;
            };
            if is_retyped(old_def, new_def) {
                breaks.push(format!(
                    "{def_path}: retyped (was {}, now {})",
                    type_summary(old_def),
                    type_summary(new_def),
                ));
                continue;
            }
            compare_node(old_def, new_def, &def_path, breaks);
        }
    }

    // 4. items.
    if let Some(old_items) = old.get("items") {
        let items_path = join_path(path, "items");
        if let Some(new_items) = new.get("items") {
            if is_retyped(old_items, new_items) {
                breaks.push(format!(
                    "{items_path}: retyped (was {}, now {})",
                    type_summary(old_items),
                    type_summary(new_items),
                ));
            } else {
                compare_node(old_items, new_items, &items_path, breaks);
            }
        } else {
            breaks.push(format!("{items_path}: removed"));
        }
    }

    // 5. oneOf / anyOf / allOf.
    for combinator in ["oneOf", "anyOf", "allOf"] {
        let Some(old_arr) = old.get(combinator).and_then(Value::as_array) else {
            continue;
        };
        let new_arr = new.get(combinator).and_then(Value::as_array);
        let comb_base = join_path(path, combinator);
        for (idx, old_v) in old_arr.iter().enumerate() {
            let (var_path, matched) = if let Some(tag) = variant_type_tag(old_v) {
                let found =
                    new_arr.and_then(|arr| arr.iter().find(|nv| variant_type_tag(nv) == Some(tag)));
                (format!("{comb_base}[{tag}]"), found)
            } else {
                let found = new_arr.and_then(|arr| arr.get(idx));
                (format!("{comb_base}[{idx}]"), found)
            };
            let Some(new_v) = matched else {
                breaks.push(format!("{var_path}: removed"));
                continue;
            };
            if is_retyped(old_v, new_v) {
                breaks.push(format!(
                    "{var_path}: retyped (was {}, now {})",
                    type_summary(old_v),
                    type_summary(new_v),
                ));
                continue;
            }
            compare_node(old_v, new_v, &var_path, breaks);
        }
    }
}

//! Canonical JSON values (contract §2, §9.5).
//!
//! One number type: a finite IEEE-754 value within ±9,007,199,254,740,991,
//! with no integral/floating distinction (`1` and `1.0` are one value) and
//! negative zero normalised to zero; an exact larger integer is a string. No
//! NaN, infinity, duplicate object key or ill-formed Unicode. Canonical bytes
//! are RFC 8785 (JCS) and every digest is the SHA-256 of them.
//!
//! A typed reference is the closed wrapper
//! `{"$ref":{"entity_type":…,"id":…,"revision":…}}`; inside properties every
//! other `$`-prefixed key is refused, so nothing can look like one — except
//! a node's reserved top-level members ([`RESERVED_MEMBERS`]), each checked
//! under its own closed shape.

use std::cell::Cell;
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::symbol::{encode, SymbolKind};
use crate::{is_decimal, is_ulid, Refusal};

/// The largest magnitude a number may have.
pub const MAX_NUMBER: f64 = 9_007_199_254_740_991.0;
/// The deepest nesting of arrays and objects a parsed document may have.
pub const MAX_DOCUMENT_DEPTH: usize = 16;
/// The deepest nesting of a node's or edge's `properties`, the properties
/// object itself counting as one.
pub const MAX_PROPERTY_NESTING: usize = 8;
/// The most canonical bytes a `properties` object may have.
pub const MAX_PROPERTIES_BYTES: usize = 64 * 1024;

/// Parse a JSON document under the contract's value rules, normalising its
/// numbers. Over-deep documents refuse `change_limit`; everything else the
/// rules refuse is `invalid_change`.
pub fn parse(bytes: &[u8]) -> Result<Value, Refusal> {
    let limited = Cell::new(false);
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Strict {
        depth: 0,
        limited: &limited,
    }
    .deserialize(&mut decoder)
    .and_then(|value| decoder.end().map(|()| value));
    value.map_err(|_| {
        if limited.get() {
            Refusal::ChangeLimit
        } else {
            Refusal::InvalidChange
        }
    })
}

/// A value with its numbers normalised, or the refusal of a number out of
/// range or a document too deep.
pub fn normalize(value: &Value) -> Result<Value, Refusal> {
    normalize_at(value, 0)
}

fn normalize_at(value: &Value, depth: usize) -> Result<Value, Refusal> {
    Ok(match value {
        Value::Number(n) => number(n.as_f64().ok_or(Refusal::InvalidChange)?)?,
        Value::Array(items) => {
            if depth >= MAX_DOCUMENT_DEPTH {
                return Err(Refusal::ChangeLimit);
            }
            Value::Array(
                items
                    .iter()
                    .map(|item| normalize_at(item, depth + 1))
                    .collect::<Result<_, _>>()?,
            )
        }
        Value::Object(members) => {
            if depth >= MAX_DOCUMENT_DEPTH {
                return Err(Refusal::ChangeLimit);
            }
            let mut out = Map::new();
            for (key, member) in members {
                out.insert(key.clone(), normalize_at(member, depth + 1)?);
            }
            Value::Object(out)
        }
        other => other.clone(),
    })
}

/// The canonical (JCS) bytes of a value.
pub fn canonical(value: &Value) -> Result<Vec<u8>, Refusal> {
    serde_jcs::to_vec(&normalize(value)?).map_err(|_| Refusal::InvalidChange)
}

/// The lowercase hex SHA-256 of a value's canonical bytes.
pub fn digest(value: &Value) -> Result<String, Refusal> {
    Ok(crate::sha256_hex(&canonical(value)?))
}

/// One number as the contract holds it.
fn number(f: f64) -> Result<Value, Refusal> {
    if !f.is_finite() || f.abs() > MAX_NUMBER {
        return Err(Refusal::InvalidChange);
    }
    if f.trunc() == f {
        // Within ±2^53 every integral value is exact; this also folds -0.
        #[allow(clippy::cast_possible_truncation)]
        return Ok(Value::from(f as i64));
    }
    serde_json::Number::from_f64(f)
        .map(Value::Number)
        .ok_or(Refusal::InvalidChange)
}

struct Strict<'a> {
    depth: usize,
    limited: &'a Cell<bool>,
}

impl Strict<'_> {
    fn deeper<E: de::Error>(&self) -> Result<Strict<'_>, E> {
        if self.depth >= MAX_DOCUMENT_DEPTH {
            self.limited.set(true);
            return Err(E::custom("nesting limit"));
        }
        Ok(Strict {
            depth: self.depth + 1,
            limited: self.limited,
        })
    }
}

impl<'de> DeserializeSeed<'de> for Strict<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a contract JSON value")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        if v.unsigned_abs() > 9_007_199_254_740_991 {
            return Err(E::custom("number range"));
        }
        Ok(Value::from(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        if v > 9_007_199_254_740_991 {
            return Err(E::custom("number range"));
        }
        Ok(Value::from(v))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        number(v).map_err(|_| E::custom("number range"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let inner = self.deeper()?;
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(Strict {
            depth: inner.depth,
            limited: inner.limited,
        })? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let inner = self.deeper()?;
        let mut members = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if members.contains_key(&key) {
                return Err(de::Error::custom("duplicate key"));
            }
            let member = map.next_value_seed(Strict {
                depth: inner.depth,
                limited: inner.limited,
            })?;
            members.insert(key, member);
        }
        Ok(Value::Object(members))
    }
}

/// What a typed reference points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefType {
    Node,
    Content,
}

/// A typed reference: an entity and, optionally, its exact revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedRef {
    pub entity_type: RefType,
    pub id: String,
    pub revision: Option<String>,
}

/// The typed reference `value` is, `None` when it is not a `$ref` wrapper.
/// A wrapper with any other shape refuses, and so do the `resource_revision`
/// and `blob` variants, reserved until storage implements them.
pub fn typed_ref(value: &Value) -> Result<Option<TypedRef>, Refusal> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(inner) = object.get("$ref") else {
        return Ok(None);
    };
    let inner = match inner.as_object() {
        Some(inner) if object.len() == 1 && inner.len() == 3 => inner,
        _ => return Err(Refusal::InvalidChange),
    };
    let entity_type = match inner.get("entity_type").and_then(Value::as_str) {
        Some("node") => RefType::Node,
        Some("content") => RefType::Content,
        _ => return Err(Refusal::InvalidChange),
    };
    let id = match inner.get("id").and_then(Value::as_str) {
        Some(id) if is_ulid(id) => id.to_owned(),
        _ => return Err(Refusal::InvalidChange),
    };
    let revision = match inner.get("revision") {
        Some(Value::Null) => None,
        Some(Value::String(revision)) if is_decimal(revision) => Some(revision.clone()),
        _ => return Err(Refusal::InvalidChange),
    };
    Ok(Some(TypedRef {
        entity_type,
        id,
        revision,
    }))
}

/// The reserved top-level members of a **node's** `properties`, each with
/// its own closed shape and kind rule ([`check_node_properties`]):
/// `$definition` ([`crate::vocabulary::check_definition`]) is required on a
/// `Vocabulary` node and refused on every other kind; `$assertion`
/// ([`crate::assertion`]) may sit on any kind; `$locator`
/// ([`crate::locator`]) on a `Resource` only. None is a query symbol.
pub const RESERVED_MEMBERS: [&str; 3] = [
    crate::vocabulary::DEFINITION_MEMBER,
    crate::assertion::MEMBER,
    crate::locator::MEMBER,
];

/// Check a node's or edge's `properties`: an object whose keys are custom
/// property keys (never a structural field), nested at most
/// [`MAX_PROPERTY_NESTING`], within [`MAX_PROPERTIES_BYTES`] canonical bytes,
/// with numbers in range and every `$` key a well-formed typed reference.
/// No reserved member is admitted: an edge never has one, and a node's are
/// [`check_node_properties`]'.
pub fn check_properties(properties: &Value) -> Result<(), Refusal> {
    check_object(properties, false)
}

/// Check a node's `properties` as [`check_properties`] does, admitting the
/// [`RESERVED_MEMBERS`] at the top level under their own shapes, and, when
/// the node's `kind` is known, their kind rule.
pub fn check_node_properties(kind: Option<&str>, properties: &Value) -> Result<(), Refusal> {
    check_object(properties, true)?;
    let Some(kind) = kind else {
        return Ok(());
    };
    let has = |member: &str| properties.get(member).is_some();
    let definition_ok = has(crate::vocabulary::DEFINITION_MEMBER) == (kind == "Vocabulary");
    let locator_ok = !has(crate::locator::MEMBER) || kind == "Resource";
    if definition_ok && locator_ok {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
}

/// Whether `key` may be named by a node's `remove_properties`: a custom
/// property key or a reserved member.
#[must_use]
pub fn node_property_key(key: &str) -> bool {
    RESERVED_MEMBERS.contains(&key) || property_key(key)
}

/// Whether `key` is a custom property key: one [`check_properties`] admits.
#[must_use]
pub fn property_key(key: &str) -> bool {
    !key.starts_with('$') && encode(SymbolKind::Property, key).is_ok()
}

fn check_object(properties: &Value, node: bool) -> Result<(), Refusal> {
    let Some(members) = properties.as_object() else {
        return Err(Refusal::InvalidChange);
    };
    for (key, member) in members {
        if node && RESERVED_MEMBERS.contains(&key.as_str()) {
            check_reserved(key, member)?;
            continue;
        }
        encode(SymbolKind::Property, key)?;
        if key.starts_with('$') {
            return Err(Refusal::InvalidChange);
        }
        check_member(member, 2)?;
    }
    if canonical(properties)?.len() > MAX_PROPERTIES_BYTES {
        return Err(Refusal::ChangeLimit);
    }
    Ok(())
}

/// One reserved member's shape. Its free-form parts (a definition's
/// `schema`, `index_hints`, `extensions`; an envelope's `extensions`) are
/// checked as property values, one level below the member.
fn check_reserved(key: &str, member: &Value) -> Result<(), Refusal> {
    let free: &[&str] = match key {
        crate::vocabulary::DEFINITION_MEMBER => {
            crate::vocabulary::check_definition(member)?;
            &["schema", "index_hints", "extensions"]
        }
        crate::assertion::MEMBER => {
            crate::assertion::check(member)?;
            &[]
        }
        crate::locator::MEMBER => {
            crate::locator::check(member)?;
            &["extensions"]
        }
        _ => return Err(Refusal::InvalidChange),
    };
    for part in free {
        if let Some(value) = member.get(*part) {
            check_member(value, 3)?;
        }
    }
    Ok(())
}

fn check_member(value: &Value, level: usize) -> Result<(), Refusal> {
    match value {
        Value::Array(items) => {
            if level > MAX_PROPERTY_NESTING {
                return Err(Refusal::ChangeLimit);
            }
            items
                .iter()
                .try_for_each(|item| check_member(item, level + 1))
        }
        Value::Object(members) => {
            if level > MAX_PROPERTY_NESTING {
                return Err(Refusal::ChangeLimit);
            }
            if typed_ref(value)?.is_some() {
                return Ok(());
            }
            for (key, member) in members {
                if key.starts_with('$') {
                    return Err(Refusal::InvalidChange);
                }
                check_member(member, level + 1)?;
            }
            Ok(())
        }
        Value::Number(n) => number(n.as_f64().ok_or(Refusal::InvalidChange)?).map(|_| ()),
        _ => Ok(()),
    }
}

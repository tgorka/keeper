//! The closed query profile `KG-Cypher/1` (contract §5, §9.6): a recogniser
//! that consumes the whole source and a renderer that emits the engine's
//! text from the typed result, never from caller bytes.
//!
//! One `MATCH` of one connected chain of at most 8 typed directed
//! relationship segments; an optional `WHERE` of at most 32 `AND`-ed
//! comparisons of a property with a named parameter (`=`, `<>`, `<`, `<=`,
//! `>`, `>=`) or `IS [NOT] NULL`; `RETURN` of at most 16 aliased scalar
//! properties or one `count(*)` / `count(variable)`; an optional `ORDER BY`
//! of returned aliases; a required literal `LIMIT` 1..=1000. Labels, types
//! and property keys are reserved names or their `K_`/`R_`/`P_` symbols
//! ([`crate::symbol`]); a node binding reads a node's or a tag's structural
//! fields, an edge binding an edge's. Parameters are a JSON object of at
//! most 32 scalars, each normalised as a canonical value is.
//!
//! Only [`AdmittedQuery`] produces engine text, and it keeps no raw source
//! or parameter bytes. The renderer binds every anonymous relationship and
//! appends every binding's `id` ascending, in declaration order, as the
//! final sort keys, so a `LIMIT` or an order tie never depends on an engine
//! identity; a `count` has no row tie.

use std::collections::BTreeMap;
use std::fmt::{self, Write};

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::symbol::{decode, SymbolKind};
use crate::value::MAX_NUMBER;
use crate::vocabulary::{ASSERTION_FIELDS, EDGE_FIELDS, NODE_FIELDS, TAG_FIELDS};
use crate::{Refusal, PROFILE};

/// The most source bytes a query may have.
pub const MAX_SOURCE_BYTES: usize = 8 * 1024;
/// The most bytes a query's parameters may have, raw or normalised.
pub const MAX_PARAMETER_BYTES: usize = 64 * 1024;
/// The most parameters a query may name.
pub const MAX_PARAMETERS: usize = 32;
/// The most predicates a `WHERE` may hold.
pub const MAX_PREDICATES: usize = 32;
/// The most relationship segments a `MATCH` may hold.
pub const MAX_SEGMENTS: usize = 8;
/// The most aliases a `RETURN` may hold.
pub const MAX_RETURNS: usize = 16;
/// The most rows a query may return (its `LIMIT` at most).
pub const MAX_ROWS: usize = 1_000;
/// The most encoded bytes one answer may have.
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;

type Result<T, E = Refusal> = std::result::Result<T, E>;

/// A normalised parameter or result scalar. A number is one finite JSON
/// number of magnitude at most [`MAX_NUMBER`], `-0` read as `0`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Scalar {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct ScalarVisitor;
        impl Visitor<'_> for ScalarVisitor {
            type Value = Scalar;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a finite bounded JSON scalar")
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Scalar, E> {
                Ok(Scalar::Null)
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Scalar, E> {
                Ok(Scalar::Bool(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Scalar, E> {
                Ok(Scalar::String(v.to_owned()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Scalar, E> {
                Ok(Scalar::String(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Scalar, E> {
                if v.unsigned_abs() > 9_007_199_254_740_991 {
                    return Err(E::custom("number range"));
                }
                self.visit_f64(v as f64)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Scalar, E> {
                if v > 9_007_199_254_740_991 {
                    return Err(E::custom("number range"));
                }
                self.visit_f64(v as f64)
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Scalar, E> {
                if !v.is_finite() || v.abs() > MAX_NUMBER {
                    return Err(E::custom("number range"));
                }
                Ok(Scalar::Number(if v == 0.0 { 0.0 } else { v }))
            }
        }
        d.deserialize_any(ScalarVisitor)
    }
}

fn parse_parameters(input: &[u8]) -> Result<BTreeMap<String, Scalar>> {
    struct ParametersVisitor<'a> {
        limited: &'a mut bool,
    }
    impl<'de> Visitor<'de> for ParametersVisitor<'_> {
        type Value = BTreeMap<String, Scalar>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most 32 unique scalar parameters")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut values = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                if !identifier(&key) || values.contains_key(&key) {
                    return Err(de::Error::custom("invalid or duplicate parameter"));
                }
                if values.len() == MAX_PARAMETERS {
                    *self.limited = true;
                    return Err(de::Error::custom("parameter count limit"));
                }
                values.insert(key, map.next_value()?);
            }
            Ok(values)
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(input);
    let mut limited = false;
    let result = decoder.deserialize_map(ParametersVisitor {
        limited: &mut limited,
    });
    let values = result.map_err(|_| {
        if limited {
            Refusal::QueryLimit
        } else {
            Refusal::QueryUnsupported
        }
    })?;
    decoder.end().map_err(|_| Refusal::QueryUnsupported)?;
    Ok(values)
}

fn identifier(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A label or relationship type: a reserved name or a custom name's symbol.
fn symbol(kind: SymbolKind, s: &str) -> Result<()> {
    decode(kind, s)
        .map(|_| ())
        .map_err(|_| Refusal::QueryUnsupported)
}

#[derive(Debug)]
struct Binding {
    name: String,
    kind: SymbolKind,
}
#[derive(Debug)]
struct Node {
    binding: usize,
    label: Option<String>,
}
#[derive(Debug)]
struct Edge {
    binding: usize,
    relation: String,
    incoming: bool,
}
#[derive(Debug)]
struct Property {
    binding: usize,
    key: String,
}
#[derive(Debug)]
enum Predicate {
    Compare(Property, &'static str, String),
    Null(Property, bool),
}
#[derive(Debug)]
enum Returns {
    Properties(Vec<Property>),
    Count(Option<usize>),
}

/// A query the profile admitted, holding no raw source or parameter bytes.
#[derive(Debug)]
pub struct AdmittedQuery {
    bindings: Vec<Binding>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    predicates: Vec<Predicate>,
    returns: Returns,
    aliases: Vec<String>,
    order: Vec<(usize, bool)>,
    limit: usize,
    parameters: BTreeMap<String, Scalar>,
}

impl AdmittedQuery {
    /// The caller's aliases, in `RETURN` order: the keys of every row.
    #[must_use]
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// The query's `LIMIT`.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// The normalised parameters as JSON text, keys in ascending order.
    pub fn parameters_json(&self) -> Result<String> {
        serde_json::to_string(&self.parameters).map_err(|_| Refusal::QueryUnsupported)
    }

    /// The engine's text: canonical Cypher over an active-only projection,
    /// bindings `v0…`, returns `a0…`, every binding's `id` the final sort
    /// keys.
    #[must_use]
    pub fn engine_text(&self) -> String {
        fn property(out: &mut String, p: &Property) {
            let _ = write!(out, "v{}.{}", p.binding, p.key);
        }
        fn node(out: &mut String, n: &Node) {
            let _ = write!(out, "(v{}", n.binding);
            if let Some(label) = &n.label {
                out.push(':');
                out.push_str(label);
            }
            out.push(')');
        }
        let mut out = String::from("MATCH ");
        node(&mut out, &self.nodes[0]);
        for (edge, n) in self.edges.iter().zip(&self.nodes[1..]) {
            out.push_str(if edge.incoming { "<-[" } else { "-[" });
            let _ = write!(out, "v{}:{}", edge.binding, edge.relation);
            out.push_str(if edge.incoming { "]-" } else { "]->" });
            node(&mut out, n);
        }
        for (i, pred) in self.predicates.iter().enumerate() {
            out.push_str(if i == 0 { " WHERE " } else { " AND " });
            match pred {
                Predicate::Compare(p, op, param) => {
                    property(&mut out, p);
                    let _ = write!(out, " {op} ${param}");
                }
                Predicate::Null(p, not) => {
                    property(&mut out, p);
                    out.push_str(if *not { " IS NOT NULL" } else { " IS NULL" });
                }
            }
        }
        out.push_str(" RETURN ");
        match &self.returns {
            Returns::Properties(properties) => {
                for (i, p) in properties.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    property(&mut out, p);
                    let _ = write!(out, " AS a{i}");
                }
            }
            Returns::Count(binding) => {
                out.push_str("count(");
                if let Some(b) = binding {
                    let _ = write!(out, "v{b}");
                } else {
                    out.push('*');
                }
                out.push_str(") AS a0");
            }
        }
        let mut sort = false;
        for (alias, desc) in &self.order {
            out.push_str(if sort { ", " } else { " ORDER BY " });
            sort = true;
            let _ = write!(out, "a{alias} {}", if *desc { "DESC" } else { "ASC" });
        }
        if matches!(self.returns, Returns::Properties(_)) {
            for b in 0..self.bindings.len() {
                out.push_str(if sort { ", " } else { " ORDER BY " });
                sort = true;
                let _ = write!(out, "v{b}.id ASC");
            }
        }
        let _ = write!(out, " LIMIT {}", self.limit);
        out
    }
}

struct Parser<'a> {
    tokens: Vec<&'a str>,
    pos: usize,
    query: AdmittedQuery,
}

/// Admit `source` with `parameters` (a JSON object) under `profile`. Bytes
/// are bounded before lexing or decoding; the whole source must be
/// consumed by the grammar.
pub fn admit(profile: &str, source: &str, parameters: &[u8]) -> Result<AdmittedQuery> {
    if source.len() > MAX_SOURCE_BYTES || parameters.len() > MAX_PARAMETER_BYTES {
        return Err(Refusal::QueryLimit);
    }
    if profile != PROFILE || !source.is_ascii() {
        return Err(Refusal::QueryUnsupported);
    }
    let parameters = parse_parameters(parameters)?;
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
        } else if b"():[]-<>.,=$*".contains(&bytes[i]) {
            i += 1;
            if i < bytes.len() && matches!(&source[start..i + 1], "->" | "<-" | "<>" | "<=" | ">=")
            {
                i += 1;
            }
        } else {
            return Err(Refusal::QueryUnsupported);
        }
        tokens.push(&source[start..i]);
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        query: AdmittedQuery {
            bindings: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            predicates: Vec::new(),
            returns: Returns::Count(None),
            aliases: Vec::new(),
            order: Vec::new(),
            limit: 0,
            parameters,
        },
    };
    p.require("MATCH")?;
    let first = p.node()?;
    p.query.nodes.push(first);
    while matches!(p.peek(), Some("-") | Some("<-")) {
        if p.query.edges.len() == MAX_SEGMENTS {
            return Err(Refusal::QueryLimit);
        }
        let incoming = p.take("<-");
        if !incoming {
            p.require("-")?;
        }
        p.require("[")?;
        let name = if p.peek() != Some(":") {
            Some(p.ident()?.to_owned())
        } else {
            None
        };
        p.require(":")?;
        let relation = p.ident()?.to_owned();
        symbol(SymbolKind::Edge, &relation)?;
        p.require("]")?;
        p.require(if incoming { "-" } else { "->" })?;
        let binding = p.bind(name, SymbolKind::Edge)?;
        p.query.edges.push(Edge {
            binding,
            relation,
            incoming,
        });
        let next = p.node()?;
        p.query.nodes.push(next);
    }
    if p.take("WHERE") {
        loop {
            if p.query.predicates.len() == MAX_PREDICATES {
                return Err(Refusal::QueryLimit);
            }
            let property = p.property()?;
            let predicate = if p.take("IS") {
                let not = p.take("NOT");
                p.require("NULL")?;
                Predicate::Null(property, not)
            } else {
                let op = match p.next()? {
                    "=" => "=",
                    "<>" => "<>",
                    "<" => "<",
                    "<=" => "<=",
                    ">" => ">",
                    ">=" => ">=",
                    _ => return Err(Refusal::QueryUnsupported),
                };
                p.require("$")?;
                let param = p.ident()?.to_owned();
                if !p.query.parameters.contains_key(&param) {
                    return Err(Refusal::QueryUnsupported);
                }
                Predicate::Compare(property, op, param)
            };
            p.query.predicates.push(predicate);
            if !p.take("AND") {
                break;
            }
        }
    }
    p.require("RETURN")?;
    if p.peek().is_some_and(|t| t.eq_ignore_ascii_case("count"))
        && p.tokens.get(p.pos + 1) == Some(&"(")
    {
        p.require("count")?;
        p.require("(")?;
        let binding = if p.take("*") {
            None
        } else {
            let name = p.ident()?;
            Some(p.resolve(name)?)
        };
        p.require(")")?;
        let alias = if p.take("AS") {
            p.ident()?.to_owned()
        } else {
            "count".into()
        };
        p.query.aliases.push(alias);
        p.query.returns = Returns::Count(binding);
    } else {
        let mut properties = Vec::new();
        loop {
            if properties.len() == MAX_RETURNS {
                return Err(Refusal::QueryLimit);
            }
            properties.push(p.property()?);
            p.require("AS")?;
            let alias = p.ident()?.to_owned();
            if p.query.aliases.contains(&alias) {
                return Err(Refusal::QueryUnsupported);
            }
            p.query.aliases.push(alias);
            if !p.take(",") {
                break;
            }
        }
        p.query.returns = Returns::Properties(properties);
    }
    if p.take("ORDER") {
        p.require("BY")?;
        loop {
            let name = p.ident()?;
            let alias = p
                .query
                .aliases
                .iter()
                .position(|a| a == name)
                .ok_or(Refusal::QueryUnsupported)?;
            let desc = p.take("DESC");
            if !desc {
                p.take("ASC");
            }
            p.query.order.push((alias, desc));
            if !p.take(",") {
                break;
            }
        }
    }
    p.require("LIMIT")?;
    let limit = p.next()?;
    if !limit.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Refusal::QueryUnsupported);
    }
    p.query.limit = limit.parse().map_err(|_| Refusal::QueryLimit)?;
    if !(1..=MAX_ROWS).contains(&p.query.limit) {
        return Err(Refusal::QueryLimit);
    }
    if p.pos != p.tokens.len() {
        return Err(Refusal::QueryUnsupported);
    }
    if p.query.parameters_json()?.len() > MAX_PARAMETER_BYTES {
        return Err(Refusal::QueryLimit);
    }
    Ok(p.query)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).copied()
    }
    fn next(&mut self) -> Result<&'a str> {
        let t = self.peek().ok_or(Refusal::QueryUnsupported)?;
        self.pos += 1;
        Ok(t)
    }
    fn take(&mut self, wanted: &str) -> bool {
        if self.peek().is_some_and(|t| t.eq_ignore_ascii_case(wanted)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn require(&mut self, wanted: &str) -> Result<()> {
        if self.take(wanted) {
            Ok(())
        } else {
            Err(Refusal::QueryUnsupported)
        }
    }
    fn ident(&mut self) -> Result<&'a str> {
        let s = self.next()?;
        if identifier(s) {
            Ok(s)
        } else {
            Err(Refusal::QueryUnsupported)
        }
    }
    fn resolve(&self, name: &str) -> Result<usize> {
        self.query
            .bindings
            .iter()
            .position(|b| b.name == name)
            .ok_or(Refusal::QueryUnsupported)
    }
    fn bind(&mut self, name: Option<String>, kind: SymbolKind) -> Result<usize> {
        if let Some(name) = &name {
            if let Some(i) = self.query.bindings.iter().position(|b| b.name == *name) {
                return if self.query.bindings[i].kind == kind {
                    Ok(i)
                } else {
                    Err(Refusal::QueryUnsupported)
                };
            }
        }
        let index = self.query.bindings.len();
        // An anonymous name cannot be spelled by the ASCII identifier grammar.
        self.query.bindings.push(Binding {
            name: name.unwrap_or_else(|| format!("@{index}")),
            kind,
        });
        Ok(index)
    }
    fn node(&mut self) -> Result<Node> {
        self.require("(")?;
        let name = self.ident()?.to_owned();
        let binding = self.bind(Some(name), SymbolKind::Node)?;
        let label = if self.take(":") {
            let label = self.ident()?.to_owned();
            symbol(SymbolKind::Node, &label)?;
            Some(label)
        } else {
            None
        };
        self.require(")")?;
        Ok(Node { binding, label })
    }
    fn property(&mut self) -> Result<Property> {
        let name = self.ident()?;
        let binding = self.resolve(name)?;
        self.require(".")?;
        let key = self.ident()?.to_owned();
        // A node binding's structural keys: its fields, a projected tag's,
        // and (projection version 2) the assertion fields, null when the
        // node carries no `$assertion`.
        let structural = if self.query.bindings[binding].kind == SymbolKind::Node {
            NODE_FIELDS.contains(&key.as_str())
                || TAG_FIELDS.contains(&key.as_str())
                || ASSERTION_FIELDS.contains(&key.as_str())
        } else {
            EDGE_FIELDS.contains(&key.as_str())
        };
        if !structural {
            decode(SymbolKind::Property, &key).map_err(|_| Refusal::QueryUnsupported)?;
        }
        Ok(Property { binding, key })
    }
}

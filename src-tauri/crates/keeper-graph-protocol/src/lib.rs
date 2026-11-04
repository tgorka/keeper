//! Closed KG-Cypher/1 admission. This crate owns no storage, transport or native code.
//! Only `AdmittedQuery` can produce engine text; its representation is private.
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Write};

pub const PROFILE: &str = "KG-Cypher/1";
pub const MAX_SOURCE_BYTES: usize = 8192;
pub const MAX_PARAMETER_BYTES: usize = 65536;
pub const MAX_RESULT_BYTES: usize = 1048576;
pub const MAX_ROWS: usize = 1000;
pub const MAX_NUMBER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    QueryUnsupported,
    QueryLimit,
}
impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::QueryUnsupported => "query_unsupported",
            Self::QueryLimit => "query_limit",
        })
    }
}
impl std::error::Error for Refusal {}
type Result<T, E = Refusal> = std::result::Result<T, E>;

/// A normalized JSON scalar; parameters never contain engine syntax.
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
                if values.len() == 32 {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Node,
    Edge,
    Property,
}
/// Reversible ASCII symbols. Custom kinds/types require a canonical namespace.
pub fn encode_symbol(kind: SymbolKind, canonical: &str) -> Result<String> {
    if canonical.is_empty()
        || canonical.len() > MAX_SOURCE_BYTES / 2
        || (kind != SymbolKind::Property && !canonical.contains(':'))
    {
        return Err(Refusal::QueryUnsupported);
    }
    let mut encoded = String::with_capacity(2 + canonical.len() * 2);
    encoded.push_str(prefix(kind));
    for b in canonical.bytes() {
        write!(encoded, "{b:02x}").map_err(|_| Refusal::QueryUnsupported)?;
    }
    Ok(encoded)
}
pub fn decode_symbol(kind: SymbolKind, symbol: &str) -> Result<String> {
    let hex = symbol
        .strip_prefix(prefix(kind))
        .ok_or(Refusal::QueryUnsupported)?;
    if hex.is_empty()
        || hex.len() % 2 != 0
        || hex.len() > MAX_SOURCE_BYTES
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Refusal::QueryUnsupported);
    }
    let bytes: std::result::Result<Vec<_>, _> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            u8::from_str_radix(
                std::str::from_utf8(p).map_err(|_| Refusal::QueryUnsupported)?,
                16,
            )
            .map_err(|_| Refusal::QueryUnsupported)
        })
        .collect();
    let canonical = String::from_utf8(bytes?).map_err(|_| Refusal::QueryUnsupported)?;
    if kind != SymbolKind::Property && !canonical.contains(':') {
        return Err(Refusal::QueryUnsupported);
    }
    Ok(canonical)
}
fn prefix(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Node => "K_",
        SymbolKind::Edge => "R_",
        SymbolKind::Property => "P_",
    }
}
fn identifier(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn symbol(kind: SymbolKind, s: &str) -> Result<()> {
    let reserved = match kind {
        SymbolKind::Node => matches!(
            s,
            "Concept" | "Collection" | "Resource" | "Tag" | "Guidance"
        ),
        SymbolKind::Edge => matches!(s, "HAS_TAG" | "contains" | "guidance_scope" | "sources"),
        SymbolKind::Property => false,
    };
    if reserved {
        Ok(())
    } else {
        decode_symbol(kind, s).map(|_| ())
    }
}
const NODE_FIELDS: &[&str] = &[
    "id",
    "kind",
    "title",
    "description",
    "description_source_id",
    "lifecycle",
    "lifecycle_revision",
    "revision",
    "merged_into_id",
    "text",
];
const EDGE_FIELDS: &[&str] = &["id", "from_id", "to_id", "type", "lifecycle", "revision"];
/// Used by projection ingestion to prevent custom values shadowing structural fields.
pub fn custom_property(key: &str) -> Result<String> {
    if NODE_FIELDS.contains(&key) || EDGE_FIELDS.contains(&key) {
        return Err(Refusal::QueryUnsupported);
    }
    encode_symbol(SymbolKind::Property, key)
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
/// Successful admission, retaining no raw source or raw parameter JSON.
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
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }
    pub fn limit(&self) -> usize {
        self.limit
    }
    pub fn parameters_json(&self) -> Result<String> {
        serde_json::to_string(&self.parameters).map_err(|_| Refusal::QueryUnsupported)
    }
    /// Requires an active-only immutable projection. Anonymous edges receive generated
    /// bindings too, so parallel-edge ties never depend on native integer identities.
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
/// Bounds bytes before lexing or JSON decoding, then fully consumes the closed grammar.
pub fn admit(profile: &str, source: &str, parameters: &[u8]) -> Result<AdmittedQuery> {
    if source.len() > MAX_SOURCE_BYTES || parameters.len() > MAX_PARAMETER_BYTES {
        return Err(Refusal::QueryLimit);
    }
    if profile != PROFILE {
        return Err(Refusal::QueryUnsupported);
    }
    if !source.is_ascii() {
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
        if p.query.edges.len() == 8 {
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
            if p.query.predicates.len() == 32 {
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
            if properties.len() == 16 {
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
        // Anonymous names cannot be supplied by the ASCII identifier grammar.
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
        let fields = if self.query.bindings[binding].kind == SymbolKind::Node {
            NODE_FIELDS
        } else {
            EDGE_FIELDS
        };
        if !fields.contains(&key.as_str()) {
            let canonical = decode_symbol(SymbolKind::Property, &key)?;
            custom_property(&canonical)?;
        }
        Ok(Property { binding, key })
    }
}

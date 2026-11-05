//! The reversible mapping of graph names to `KG-Cypher/1` symbols.
//!
//! A reserved kind or relationship type is its own symbol. A custom kind is
//! `K_`, a custom relationship type `R_` and a custom property `P_`, each
//! followed by the lowercase hex of the name's UTF-8 bytes. Every symbol
//! decodes back to exactly one name, and a decoded name obeys the same rules
//! as an encoded one: a structural field never comes back as a property.

use crate::vocabulary::{
    is_custom_name, is_structural_field, MAX_NAME_BYTES, RESERVED_KINDS, RESERVED_RELATIONS,
};
use crate::Refusal;

/// What a symbol names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    /// A node kind (label).
    Node,
    /// A relationship type.
    Edge,
    /// A custom property key.
    Property,
}

impl SymbolKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Node => "K_",
            Self::Edge => "R_",
            Self::Property => "P_",
        }
    }

    fn reserved(self, name: &str) -> bool {
        match self {
            Self::Node => RESERVED_KINDS.contains(&name),
            Self::Edge => RESERVED_RELATIONS.contains(&name),
            Self::Property => false,
        }
    }

    /// Whether `name` is a custom name of this kind.
    fn custom(self, name: &str) -> bool {
        match self {
            Self::Node | Self::Edge => is_custom_name(name),
            Self::Property => {
                !name.is_empty()
                    && name.len() <= MAX_NAME_BYTES
                    && !name.chars().any(char::is_control)
                    && !is_structural_field(name)
            }
        }
    }
}

/// The symbol of a name.
pub fn encode(kind: SymbolKind, name: &str) -> Result<String, Refusal> {
    if kind.reserved(name) {
        return Ok(name.to_owned());
    }
    if !kind.custom(name) {
        return Err(Refusal::InvalidChange);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut symbol = String::with_capacity(2 + name.len() * 2);
    symbol.push_str(kind.prefix());
    for byte in name.bytes() {
        symbol.push(char::from(HEX[usize::from(byte >> 4)]));
        symbol.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(symbol)
}

/// The name a symbol stands for.
pub fn decode(kind: SymbolKind, symbol: &str) -> Result<String, Refusal> {
    if kind.reserved(symbol) {
        return Ok(symbol.to_owned());
    }
    let hex = symbol
        .strip_prefix(kind.prefix())
        .ok_or(Refusal::InvalidChange)?
        .as_bytes();
    if hex.is_empty() || hex.len() % 2 != 0 || hex.len() > MAX_NAME_BYTES * 2 {
        return Err(Refusal::InvalidChange);
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(Refusal::InvalidChange),
    };
    let bytes = hex
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[high, low]| Ok(nibble(high)? << 4 | nibble(low)?))
        .collect::<Result<Vec<u8>, Refusal>>()?;
    let name = String::from_utf8(bytes).map_err(|_| Refusal::InvalidChange)?;
    if kind.custom(&name) {
        Ok(name)
    } else {
        Err(Refusal::InvalidChange)
    }
}

//! The `okf` profile, version [`VERSION`] (story 117.1): what every domain's
//! graph starts with, held in `syncular/okf-profile.json` ([`PROFILE_JSON`])
//! and read by pure functions over it:
//!
//! - the grammar of every name the profile and the import write
//!   ([`is_name`], [`admits_name`]);
//! - the order key on a `contains` edge: its alphabet ([`is_order_key`]),
//!   the key between two neighbours ([`between`]) and the re-keying of the
//!   fewest neighbours when none fits ([`insert_at`]);
//! - the number rule — a member's leading number as its key, and the only
//!   prefix that leaves a folder's title ([`number_key`], [`seed_keys`],
//!   [`later_key`], [`title`]);
//! - `okf:importance`'s range ([`admits_importance`]).
//!
//! `syncular/okf-profile.vectors.json` pins every rule here; keeper-hq's
//! server tests read the same two files. The profile writes nothing: a domain
//! receives it as `Vocabulary` nodes from keeper-hq's profile tool.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::value::RESERVED_MEMBERS;
use crate::vocabulary::{
    is_custom_name, is_structural_field, DEFINITION_MEMBER, RESERVED_KINDS, RESERVED_RELATIONS,
};

/// The profile file, as keeper's contract pins it.
pub const PROFILE_JSON: &str = include_str!("../../../../syncular/okf-profile.json");
/// The profile's version, recorded on a domain's root as `okf:profile_version`.
pub const VERSION: u32 = 1;
/// The profile's own namespace.
pub const NAMESPACE: &str = "okf";
/// The most bytes a profile name holds, inside KG1's 256.
pub const MAX_NAME_BYTES: usize = 64;
/// The order key's alphabet, in ASCII (and so key) order.
pub const ALPHABET: &[u8; 37] = b"-0123456789abcdefghijklmnopqrstuvwxyz";
/// The longest order key.
pub const MAX_KEY_LEN: usize = 64;
/// The width a member's number is padded to.
pub const NUMBER_WIDTH: usize = 8;
/// The digits of a rank among equal numbers (`-001`).
pub const RANK_DIGITS: usize = 3;
/// What an unnumbered member's seed key starts with.
pub const UNNUMBERED_PREFIX: char = 'z';
/// The digits of an unnumbered member's rank (`z00001`).
pub const UNNUMBERED_RANK_DIGITS: usize = 5;
/// The longest folder number that leaves the title (`10-notes` → `notes`).
pub const TITLE_PREFIX_MAX_DIGITS: usize = 3;
/// `okf:importance`'s range, both ends included.
pub const IMPORTANCE_MIN: i64 = 0;
/// See [`IMPORTANCE_MIN`].
pub const IMPORTANCE_MAX: i64 = 100;

const BASE: u32 = 37;

/// The profile file, read.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub profile: String,
    pub version: u32,
    pub namespace: String,
    pub reserved_namespaces: Vec<String>,
    pub grammar: Grammar,
    pub kg1: Kg1,
    pub order_key: OrderKey,
    pub importance: Importance,
    pub names: Vec<ProfileName>,
    pub model_text: ModelText,
}

/// The name grammar as the file states it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grammar {
    pub part: String,
    pub form: String,
    pub max_bytes: usize,
    /// KG1's un-namespaced names the grammar does not bind (`order_key`).
    pub exempt: Vec<String>,
}

/// KG1's reserved names, which the profile names and defines nothing in
/// place of.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Kg1 {
    pub kinds: Vec<String>,
    pub relations: Vec<String>,
    pub edge_properties: Vec<String>,
}

/// The order key's rules as the file states them.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrderKey {
    pub property: String,
    pub alphabet: String,
    pub min_length: usize,
    pub max_length: usize,
    pub never_ends_with: String,
    pub between: String,
    pub respace: String,
    pub number_width: usize,
    pub rank_digits: usize,
    pub unnumbered_prefix: String,
    pub unnumbered_rank_digits: usize,
    pub title_prefix_max_digits: usize,
}

/// `okf:importance`'s range.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Importance {
    pub property: String,
    pub min: i64,
    pub max: i64,
}

/// What a name defines, as `$definition.of` states it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Of {
    Kind,
    Relation,
    Property,
}

impl Of {
    /// The `$definition.of` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kind => "kind",
            Self::Relation => "relation",
            Self::Property => "property",
        }
    }
}

/// Where a property sits: the profile's own distinction, which a
/// `$definition` does not carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum On {
    Node,
    Edge,
}

/// One name of the profile.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileName {
    pub name: String,
    pub of: Of,
    #[serde(default)]
    pub on: Option<On>,
    #[serde(default)]
    pub from: Option<Vec<String>>,
    #[serde(default)]
    pub to: Option<Vec<String>>,
    /// `model_text` for the names reserved for text a model writes, which
    /// nothing fills in this program.
    #[serde(default)]
    pub reserved_for: Option<String>,
    pub meaning: String,
}

/// The places and rules reserved for text a model writes (J-1).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelText {
    pub summary: String,
    pub provenance: Vec<String>,
    /// Always null: a node's note is a content no reserved property names.
    pub note_property: Option<String>,
    pub note: String,
    pub rules: Vec<String>,
}

impl Profile {
    /// The pinned profile ([`PROFILE_JSON`]).
    pub fn pinned() -> Result<Self, serde_json::Error> {
        serde_json::from_str(PROFILE_JSON)
    }

    /// The profile name `name`, if the profile defines it.
    #[must_use]
    pub fn name(&self, name: &str) -> Option<&ProfileName> {
        self.names.iter().find(|entry| entry.name == name)
    }
}

impl ProfileName {
    /// The `$definition` of this name's `Vocabulary` node: `{defines, of,
    /// status: active, schema}`, the schema holding the meaning and the
    /// profile's own placement — stored data, never executed.
    #[must_use]
    pub fn definition(&self) -> Value {
        let mut schema =
            json!({ "meaning": self.meaning, "profile": NAMESPACE, "version": VERSION });
        if let Some(on) = self.on {
            schema["on"] = json!(match on {
                On::Node => "node",
                On::Edge => "edge",
            });
        }
        if let Some(from) = &self.from {
            schema["from"] = json!(from);
        }
        if let Some(to) = &self.to {
            schema["to"] = json!(to);
        }
        json!({
            "defines": self.name,
            "of": self.of.as_str(),
            "status": "active",
            "schema": schema,
        })
    }

    /// The `properties` of this name's `Vocabulary` node.
    #[must_use]
    pub fn vocabulary_properties(&self) -> Value {
        json!({ DEFINITION_MEMBER: self.definition() })
    }
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

fn is_part(part: &str) -> bool {
    let mut bytes = part.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The profile's grammar: `<namespace>:<name>`, each part `[a-z][a-z0-9_]*`,
/// at most [`MAX_NAME_BYTES`] in all.
#[must_use]
pub fn is_name(name: &str) -> bool {
    name.len() <= MAX_NAME_BYTES
        && name
            .split_once(':')
            .is_some_and(|(namespace, rest)| is_part(namespace) && is_part(rest))
}

/// Whether `name` would shadow a reserved name: a reserved kind or
/// relationship, a structural or assertion field, or a reserved member.
#[must_use]
pub fn shadows_reserved(name: &str) -> bool {
    RESERVED_KINDS.contains(&name)
        || RESERVED_RELATIONS.contains(&name)
        || is_structural_field(name)
        || RESERVED_MEMBERS.contains(&name)
}

/// A name the profile or the import may write: in the grammar, a KG1 custom
/// name, and shadowing nothing reserved.
#[must_use]
pub fn admits_name(name: &str) -> bool {
    is_name(name) && is_custom_name(name) && !shadows_reserved(name)
}

/// `okf:importance`: an integer from [`IMPORTANCE_MIN`] to
/// [`IMPORTANCE_MAX`].
#[must_use]
pub fn admits_importance(value: &Value) -> bool {
    value
        .as_i64()
        .is_some_and(|v| (IMPORTANCE_MIN..=IMPORTANCE_MAX).contains(&v))
}

// ---------------------------------------------------------------------------
// Order keys
// ---------------------------------------------------------------------------

fn digit(byte: u8) -> Option<u32> {
    ALPHABET.iter().position(|&a| a == byte).map(|i| i as u32)
}

/// An order key: 1–[`MAX_KEY_LEN`] characters of [`ALPHABET`], never ending
/// in `-`.
#[must_use]
pub fn is_order_key(key: &str) -> bool {
    (1..=MAX_KEY_LEN).contains(&key.len())
        && key.bytes().all(|b| digit(b).is_some())
        && !key.ends_with('-')
}

/// A fixed-length base-37 number, most significant digit first, with one
/// extra leading digit of headroom.
type Num = Vec<u32>;

fn num_of(key: &[u8], len: usize) -> Num {
    let mut n = vec![0; len + 1];
    for (i, &b) in key.iter().take(len).enumerate() {
        n[i + 1] = digit(b).unwrap_or(0);
    }
    n
}

fn add_one(n: &mut Num) {
    for d in n.iter_mut().rev() {
        *d += 1;
        if *d < BASE {
            return;
        }
        *d = 0;
    }
}

fn sub_one(n: &mut Num) {
    for d in n.iter_mut().rev() {
        if *d > 0 {
            *d -= 1;
            return;
        }
        *d = BASE - 1;
    }
}

fn power(len: usize) -> Num {
    let mut n = vec![0; len + 1];
    n[0] = 1;
    n
}

/// `floor((a + b) / 2)`, `a` and `b` of the same length.
fn middle(a: &Num, b: &Num) -> Num {
    let mut sum = vec![0; a.len()];
    let mut carry = 0;
    for i in (0..a.len()).rev() {
        let s = a[i] + b[i] + carry;
        sum[i] = s % BASE;
        carry = s / BASE;
    }
    // carry is 0: both are below 37^(len) with headroom.
    let mut rem = 0;
    for d in &mut sum {
        let cur = rem * BASE + *d;
        *d = cur / 2;
        rem = cur % 2;
    }
    sum
}

fn last_digit(n: &Num) -> u32 {
    n.last().copied().unwrap_or(0)
}

fn text_of(n: &Num) -> String {
    n[1..]
        .iter()
        .map(|&d| ALPHABET[d as usize] as char)
        .collect()
}

/// The key strictly between two neighbours (`None` for an open end: before
/// the first, after the last). At the shortest length at which a key between
/// them exists, the middle of the keys of that length between them, rounded
/// down; a middle ending in `-` moves one down (to a key ending in `z`), or
/// one up when that would reach the lower neighbour. `None` when no key of
/// at most [`MAX_KEY_LEN`] fits, or the neighbours are not in order — two
/// equal keys included.
#[must_use]
pub fn between(lo: Option<&str>, hi: Option<&str>) -> Option<String> {
    if lo.is_some_and(|k| !is_order_key(k)) || hi.is_some_and(|k| !is_order_key(k)) {
        return None;
    }
    if let (Some(lo), Some(hi)) = (lo, hi) {
        if lo >= hi {
            return None;
        }
    }
    for len in 1..=MAX_KEY_LEN {
        // `first` is the smallest key of this length above `lo`; `end` is
        // the first one that is not below `hi`.
        let first = match lo {
            None => vec![0; len + 1],
            Some(lo) if lo.len() >= len => {
                let mut n = num_of(lo.as_bytes(), len);
                add_one(&mut n);
                n
            }
            Some(lo) => num_of(lo.as_bytes(), len),
        };
        let end = match hi {
            None => power(len),
            Some(hi) if hi.len() > len => {
                let mut n = num_of(hi.as_bytes(), len);
                add_one(&mut n);
                n
            }
            Some(hi) => num_of(hi.as_bytes(), len),
        };
        if first >= end {
            continue;
        }
        let mut second = first.clone();
        add_one(&mut second);
        let exists = last_digit(&first) != 0 || second < end;
        if !exists {
            continue;
        }
        let mut last = end.clone();
        sub_one(&mut last);
        let mut key = middle(&first, &last);
        if last_digit(&key) == 0 {
            if key > first {
                sub_one(&mut key);
            } else {
                add_one(&mut key);
            }
        }
        return Some(text_of(&key));
    }
    None
}

/// `n` keys in order strictly between `lo` and `hi`, the middle placed first.
fn spread(lo: Option<&str>, hi: Option<&str>, n: usize) -> Option<Vec<String>> {
    if n == 0 {
        return Some(Vec::new());
    }
    let mid = between(lo, hi)?;
    let k = n / 2;
    let mut keys = spread(lo, Some(&mid), k)?;
    let right = spread(Some(&mid), hi, n - k - 1)?;
    keys.push(mid);
    keys.extend(right);
    Some(keys)
}

/// A new member's key, and the neighbours re-keyed to make room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertion {
    /// The new member's key.
    pub key: String,
    /// `(index in the siblings, new key)` for each neighbour re-keyed, in
    /// index order; empty when [`between`] found a key.
    pub rekeyed: Vec<(usize, String)>,
}

/// Insert a member at position `at` among `siblings` (their keys, in order;
/// equal keys allowed). The key between the neighbours when one fits;
/// otherwise the fewest neighbours around `at` are re-keyed in the same
/// change, the window balanced around `at` with its left side first, so the
/// order holds and no other member changes. `None` only when the whole
/// collection could not be keyed.
#[must_use]
pub fn insert_at(siblings: &[&str], at: usize) -> Option<Insertion> {
    if at > siblings.len() {
        return None;
    }
    let left_of = |i: usize| (i > 0).then(|| siblings[i - 1]);
    if let Some(key) = between(left_of(at), siblings.get(at).copied()) {
        return Some(Insertion {
            key,
            rekeyed: Vec::new(),
        });
    }
    for width in 1..=siblings.len() {
        let mut splits: Vec<(usize, usize)> = (0..=width)
            .map(|left| (left, width - left))
            .filter(|&(left, right)| left <= at && right <= siblings.len() - at)
            .collect();
        splits.sort_by_key(|&(left, right)| (left.abs_diff(right), std::cmp::Reverse(left)));
        for (left, right) in splits {
            let lo = left_of(at - left);
            let hi = siblings.get(at + right).copied();
            let Some(keys) = spread(lo, hi, width + 1) else {
                continue;
            };
            let mut rekeyed = Vec::new();
            for (offset, key) in keys.iter().enumerate() {
                let index = match offset.cmp(&left) {
                    std::cmp::Ordering::Less => at - left + offset,
                    std::cmp::Ordering::Equal => continue,
                    std::cmp::Ordering::Greater => at + offset - left - 1,
                };
                if siblings[index] != key {
                    rekeyed.push((index, key.clone()));
                }
            }
            return Some(Insertion {
                key: keys[left].clone(),
                rekeyed,
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The number rule
// ---------------------------------------------------------------------------

/// A member's number as an order key, whichever siblings exist: a leading
/// date `yyyy-mm-dd` (separators `-`, `_` or `.`, no digit after it) as its
/// eight digits joined; else the first run of digits, at most
/// [`NUMBER_WIDTH`], left-padded with `0`. `None` for a name that does not
/// start with a digit, or whose run is longer.
#[must_use]
pub fn number_key(name: &str) -> Option<String> {
    let b = name.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    let sep = |i: usize| b.get(i).is_some_and(|c| matches!(c, b'-' | b'_' | b'.'));
    if digits(0..4)
        && sep(4)
        && digits(5..7)
        && sep(7)
        && digits(8..10)
        && !b.get(10).is_some_and(u8::is_ascii_digit)
    {
        return Some(format!("{}{}{}", &name[0..4], &name[5..7], &name[8..10]));
    }
    let run = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if run == 0 || run > NUMBER_WIDTH {
        return None;
    }
    Some(format!("{:0>width$}", &name[..run], width = NUMBER_WIDTH))
}

fn ranked(base: &str, rank: usize) -> Option<String> {
    (rank < 10_usize.pow(RANK_DIGITS as u32))
        .then(|| format!("{base}-{rank:0width$}", width = RANK_DIGITS))
}

/// The seed keys of one collection's members, in `names`' order: a number
/// only one member has is its key; members sharing one take it with `-` and
/// their rank in name order (`-001`, `-002`, …); members with no number take
/// [`UNNUMBERED_PREFIX`] and their rank among them in name order (`z00001`),
/// after every numbered one. `None` where a rank outgrows its digits.
#[must_use]
pub fn seed_keys(names: &[&str]) -> Vec<Option<String>> {
    let numbers: Vec<Option<String>> = names.iter().map(|name| number_key(name)).collect();
    names
        .iter()
        .zip(&numbers)
        .map(|(name, number)| {
            let rank = |same: &dyn Fn(&Option<String>) -> bool| {
                names
                    .iter()
                    .zip(&numbers)
                    .filter(|(other, n)| same(n) && *other < name)
                    .count()
                    + 1
            };
            match number {
                Some(key) => {
                    let sharing = numbers.iter().filter(|n| n.as_ref() == Some(key)).count();
                    if sharing == 1 {
                        Some(key.clone())
                    } else {
                        ranked(key, rank(&|n| n.as_ref() == Some(key)))
                    }
                }
                None => {
                    let rank = rank(&|n| n.is_none());
                    (rank < 10_usize.pow(UNNUMBERED_RANK_DIGITS as u32)).then(|| {
                        format!(
                            "{UNNUMBERED_PREFIX}{rank:0width$}",
                            width = UNNUMBERED_RANK_DIGITS
                        )
                    })
                }
            }
        })
        .collect()
}

/// The key of a member that arrives after the seed, among its siblings'
/// keys; no existing key changes. With a number no sibling holds, that
/// number; with one a sibling holds, the next rank after the highest that
/// number holds (a lone number counts as rank 0). With no number, the key
/// after the last sibling, or the first unnumbered seed key when the
/// collection has none.
#[must_use]
pub fn later_key(name: &str, siblings: &[&str]) -> Option<String> {
    match number_key(name) {
        Some(base) => {
            let highest = siblings
                .iter()
                .filter_map(|key| {
                    if *key == base {
                        return Some(0);
                    }
                    let rank = key.strip_prefix(base.as_str())?.strip_prefix('-')?;
                    (rank.len() == RANK_DIGITS && rank.bytes().all(|b| b.is_ascii_digit()))
                        .then(|| rank.parse::<usize>().ok())
                        .flatten()
                })
                .max();
            match highest {
                None => Some(base),
                Some(rank) => ranked(&base, rank + 1),
            }
        }
        None => match siblings.iter().max() {
            None => seed_keys(&[name]).pop().flatten(),
            Some(last) => between(Some(last), None),
        },
    }
}

/// What a member is, for its title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Member {
    Folder,
    Document,
}

/// A member's title: the drive config's title when it gives one (a bundle or
/// a listed zone); a folder's name less a number of 1–
/// [`TITLE_PREFIX_MAX_DIGITS`] digits followed by `-` and at least one more
/// character; otherwise the name as it is. A document's title is never
/// touched.
#[must_use]
pub fn title<'a>(name: &'a str, member: Member, config_title: Option<&'a str>) -> &'a str {
    if let Some(config) = config_title {
        return config;
    }
    if member == Member::Document {
        return name;
    }
    let run = name.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=TITLE_PREFIX_MAX_DIGITS).contains(&run)
        && name.as_bytes().get(run) == Some(&b'-')
        && name.len() > run + 1
    {
        return &name[run + 1..];
    }
    name
}

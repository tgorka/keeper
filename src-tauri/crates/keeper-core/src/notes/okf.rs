//! OKF v0.2 frontmatter, as a typed and *tolerant* view over
//! [`Frontmatter`](crate::notes::frontmatter::Frontmatter).
//!
//! [`Frontmatter`] is a span-recording scanner: it answers *which bytes does key
//! `K` occupy*, which is what FR-121's byte-level promise needs and is
//! deliberately not a document model. This file is the document model, and it
//! only ever **reads** — every write still goes through the scanner, so nothing
//! here can reflow a block keeper did not author.
//!
//! The local record of the format is `.okf/OKF-0.2-digest.md` on the drive. OKF
//! v0.2 was published after the training cutoff of the models that work on this
//! repository, so an implementation written from memory is a guess. What the
//! digest requires is what shapes every function below:
//!
//! - `type` is the **only** hard requirement, and a consumer "must not reject a
//!   document for a missing optional field, an unknown `type`, an unknown key, a
//!   broken link, or an absent `index.md`" — validation stricter than that makes
//!   the *consumer* non-conformant. So [`read`] returns no `Result`: there is
//!   nothing here that can fail. A missing `type` reads as `doc_type: None`, and
//!   what that means is the caller's decision, not this module's.
//! - consumers **must preserve unknown keys**: see [`OkfDoc::retained`].
//! - the trust family (`generated`, `verified`) is PROV-O in all but name, and
//!   its actor strings carry the one question a reader actually asks — did a
//!   *person* look at this, or only a model. [`ActorKind`] is that question, and
//!   the reason nothing here ever promotes an unprefixed name to a person.
//!
//! Two spellings of the same fact reach this module, because the vault's own
//! example frontmatter predates the digest: `verified: true` + `verified_by:`
//! beside the canonical `verified:` list of `{by, at}`, and bare URL strings
//! beside `sources:` entry maps. Both are read, both normalise to the same
//! values, and [`OkfDoc::verified_shape`] records which one was on disk so a
//! writer can emit the canonical form without re-reading the file. The spec
//! wins on what is *written*; the disk wins on what is *accepted*.
//!
//! One structural note, because it is the reason half of this file exists. The
//! property subset the scanner models stops at one level of nesting, and
//! `sources:`/`verified:` in canonical form are a *list of maps* — two levels.
//! The scanner therefore records those keys with no value and flags the document
//! `unparsed`, which is correct for its job (a later write replaces the whole
//! construct instead of appending a duplicate key) and useless for this one. So
//! the block-form readers below re-scan
//! [`raw_block`](Frontmatter::raw_block) themselves. That is a second reader of
//! the same bytes, which is a cost, and it buys the alternative's absence:
//! rewriting the scanner to model two levels would put every vault's
//! frontmatter through a wider parser to serve the handful of OKF keys that need
//! it.

use std::collections::BTreeMap;

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::line_bounds;

/// The standardized section of an OKF v0.2 frontmatter block, plus everything
/// else the block said.
///
/// Every field is optional or empty-able because OKF makes every field except
/// `type` optional, and `type` is optional *here* — refusing to build a view of
/// a document that lacks it would be exactly the strictness the format's
/// conformance section forbids.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OkfDoc {
    /// `type:` — the format's one hard requirement, and still an `Option` here.
    /// A document missing it is non-conformant, which is a thing to *report*,
    /// not a thing to fail a read over.
    pub doc_type: Option<String>,
    /// `title:` — a display name. Identity stays with the file's path.
    pub title: Option<String>,
    pub description: Option<String>,
    /// `version:` — not an OKF key. A producer extension, read because the
    /// format sanctions extensions and this one is common enough to type.
    pub version: Option<String>,
    /// `status:` — `draft`, `stable` or `deprecated`, unvalidated. OKF's default
    /// is `stable`; the default is not applied here, because "the author said
    /// stable" and "the author said nothing" are different facts and only one of
    /// them is worth prompting about.
    pub status: Option<String>,
    /// `generated:` — who produced the content, and when.
    pub generated: Option<Generated>,
    /// `verified:` — every review of this document, in the order written.
    /// Empty for a document nobody has checked.
    pub verified: Vec<Verification>,
    /// Which spelling of `verified:` was on disk, so a writer emitting the
    /// canonical form knows whether it must also remove the legacy keys.
    pub verified_shape: VerifiedShape,
    /// `sources:` — where the content came from, in the order written.
    pub sources: Vec<Source>,
    /// `stale_after:` — the document is stale once today is on or after this
    /// date. Kept as written; no date parsing, because a malformed date is the
    /// author's to see and not this reader's to reinterpret.
    pub stale_after: Option<String>,
    /// The prefix map for resolving CURIEs in this file: [`default_prefixes`]
    /// **extended** by the file's own `prefixes:` map, never replaced by it. A
    /// file that redeclares a default prefix wins for that one prefix and
    /// changes nothing else.
    pub prefixes: BTreeMap<String, String>,
    /// Every key the standardized section above does not claim, in source order
    /// — OKF-recommended keys this view does not model (`tags`, `resource`),
    /// this vault's own extensions (`relations`, `iri`), keeper's `keeper:`
    /// namespace, and any producer extension nobody has met yet.
    ///
    /// This field is where the format's "consumers **must preserve** unknown
    /// keys and must not reject a document for having them" is enforced on the
    /// read side: a key that reached [`read`] is in the view, so no consumer
    /// built on it can drop a key by not having heard of it. `None` for a value
    /// the property subset could not model — the key still survives, which is
    /// the whole point, and matching on the value is then the caller's problem
    /// rather than a silent deletion.
    pub retained: Vec<(String, Option<FieldValue>)>,
}

/// `generated: {by, at}` — the v0.2 replacement for v0.1's `timestamp:`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Generated {
    /// The actor that produced the content. Required by the format; empty here
    /// when a producer omitted it, because dropping the `at` we *do* have in
    /// order to punish a missing `by` would lose knowledge to make a point. An
    /// empty actor classifies as [`ActorKind::Unknown`], which is the lowest
    /// trust there is — so a malformed block can never read as more trusted
    /// than a well-formed one.
    pub by: String,
    /// Last meaningful content change. Falls back to v0.1's `timestamp:`, which
    /// the digest explicitly allows readers to keep reading.
    pub at: Option<String>,
}

impl Generated {
    /// What kind of actor produced this. See [`ActorKind`] for why the answer
    /// matters more than it looks.
    pub fn actor_kind(&self) -> ActorKind {
        actor_kind(&self.by)
    }
}

/// One entry of `verified:` — a review that happened, by whom, when.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verification {
    /// The reviewing actor. Empty for a bare `verified: true` with no
    /// `verified_by:`: the claim that *something* checked this is preserved,
    /// and the absence of an actor is preserved with it.
    pub by: String,
    pub at: Option<String>,
}

impl Verification {
    /// What kind of actor performed this review. [`ActorKind::Person`] is the
    /// only answer that means a human read the document.
    pub fn actor_kind(&self) -> ActorKind {
        actor_kind(&self.by)
    }
}

/// Which spelling of `verified:` a document used.
///
/// A writer must emit [`Canonical`](VerifiedShape::Canonical) whatever it read,
/// so this exists for exactly one decision: whether the write also has to
/// delete `verified_by:` (and turn `verified: true` into a list), or whether it
/// is only replacing entries in a list that is already the right shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerifiedShape {
    /// No `verified:` key at all. Not the same as an empty list.
    #[default]
    Absent,
    /// The v0.2 form: a list of `{by, at}` entries, every one of them naming an
    /// actor under `by:`.
    Canonical,
    /// Anything else that still means "verified": `verified: true` with a
    /// `verified_by:` actor beside it, `verified: false`, or a list of bare
    /// actor strings. Read, normalised, and flagged for rewriting.
    Simplified,
}

/// One entry of `sources:` — a thing the content came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    /// A stable key, so a body footnote can cite this one source by name rather
    /// than by position in the list.
    pub id: Option<String>,
    /// A URL, a bundle-relative path, or a scope description. Required inside an
    /// OKF entry, and the one field a bare-string source can supply — which is
    /// why the bare form normalises here and nowhere else.
    pub resource: String,
    pub title: Option<String>,
    /// Who produced the source. An authority signal, and an actor string:
    /// [`actor_kind`] applies.
    pub author: Option<String>,
    /// How often the source is exercised — a liveness signal, framed by the
    /// document's `usage_window`. A non-integer value reads as absent rather
    /// than as zero: "nobody counted" and "counted zero" are not the same claim.
    pub usage_count: Option<u64>,
    pub last_modified: Option<String>,
}

/// What an actor string says it is.
///
/// OKF gives three shapes — `<producer>/<version>` for a tool, `human:<id>` for
/// a person, `process:<id>` for an automated workflow — and the distinction is
/// the entire point of the trust family: **a consumer derives trust from this**,
/// and "never sign as `human:` when you are an agent" is the rule the shapes
/// exist to make checkable. An agent that signs as a person is not a formatting
/// mistake; it is a document claiming human review it never had, which is the
/// one failure this vocabulary is built to prevent.
///
/// `service:`, `agent:`, `bot:` and `user:` are this vault's extensions, taken
/// under the format's extension licence. `user:` is a [`Person`](Self::Person)
/// and is deliberately not its own variant: it names a human being, so it must
/// carry a human's trust, and a separate variant would let a consumer that
/// checks for `Person` silently miss half the people.
///
/// Anything else, including an unprefixed name like `Jan Kowalski`, is
/// [`Unknown`](Self::Unknown). That is not pedantry: guessing that a name means
/// a person would manufacture exactly the human-review claim the shapes are
/// there to protect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// `<producer>/<version>`, e.g. `claude/opus-5`.
    Tool,
    /// `human:<id>` or `user:<id>`.
    Person,
    /// `process:<id>` — an automated workflow.
    Process,
    /// `service:<id>` — a long-running daemon.
    Service,
    /// `agent:<id>` — a model acting on its own.
    Agent,
    /// `bot:<id>` — an automation acting on someone's behalf.
    Bot,
    /// Empty, or a shape nobody declared. The lowest trust there is.
    Unknown,
}

/// Classify an actor string. See [`ActorKind`] for what the answer is used for.
pub fn actor_kind(actor: &str) -> ActorKind {
    let actor = actor.trim();
    if let Some((scheme, id)) = actor.split_once(':') {
        if !id.trim().is_empty() {
            match scheme {
                "human" | "user" => return ActorKind::Person,
                "process" => return ActorKind::Process,
                "service" => return ActorKind::Service,
                "agent" => return ActorKind::Agent,
                "bot" => return ActorKind::Bot,
                _ => {}
            }
        }
        // A colon means the string was *trying* to be one of the prefixed
        // shapes and got the prefix wrong. Falling through to the tool test
        // would read `https://example.com/v2` as the tool `https://example.com`
        // at version `v2`.
        return ActorKind::Unknown;
    }
    match actor.rsplit_once('/') {
        Some((producer, version))
            if !producer.is_empty()
                && !version.is_empty()
                && !actor.contains(char::is_whitespace) =>
        {
            ActorKind::Tool
        }
        _ => ActorKind::Unknown,
    }
}

/// The prefix table a file inherits when it declares none, and which its own
/// `prefixes:` map extends.
///
/// **Published vocabularies only, and that is a boundary rather than an
/// omission.** These nine are facts about the world: `schema:` means what
/// schema.org says it means on every drive, in every vault, forever.
///
/// A drive's OWN vocabulary is deliberately absent. `nd:` on the owner's
/// neuradrive expands to `urn:neuradrive:prop:` and `td:` on tgdrive expands to
/// `urn:tgdrive:prop:`, each recorded in that drive's own
/// `.okf/registry/predicates.md` — and keeper indexes both. A table here that
/// knew about `nd:` would either be missing `td:` (an unbound prefix on one of
/// the two drives it is looking at) or would carry a list of one owner's drives
/// inside a shared binary, where the third vault breaks it silently and nobody
/// finds out until a graph is wrong. Both were tried on paper; both are worse
/// than the boundary.
///
/// Nothing is lost by leaving them out, because expansion is not what keeper
/// does with a predicate. keeper DISPLAYS the CURIE the author wrote. The IRI is
/// needed only to emit RDF, and that is the vault toolkit's job — it runs inside
/// one drive and reads that drive's registry, so the base is stated once, where
/// it is true. An unbound prefix here renders as written and expands to nothing,
/// which is the honest answer to "what IRI is this" when the answer lives
/// somewhere keeper is not.
const DEFAULT_PREFIXES: [(&str, &str); 9] = [
    ("schema", "https://schema.org/"),
    ("foaf", "http://xmlns.com/foaf/0.1/"),
    ("dcterms", "http://purl.org/dc/terms/"),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("prov", "http://www.w3.org/ns/prov#"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
];

/// The prefixes every file gets for free.
pub fn default_prefixes() -> BTreeMap<String, String> {
    DEFAULT_PREFIXES
        .iter()
        .map(|(prefix, iri)| ((*prefix).to_owned(), (*iri).to_owned()))
        .collect()
}

/// A CURIE to its absolute IRI: `schema:creator` becomes
/// `https://schema.org/creator`.
///
/// `prefixes` is consulted first and the defaults second, so a file that
/// redeclares `schema:` gets its own answer. `None` for a token that is not
/// CURIE-shaped, and `None` for a prefix nobody declared — which is a
/// **reportable condition, not a licence to guess**. An undeclared prefix
/// expanded against a plausible-looking base would put a triple nobody wrote
/// into a graph somebody queries, and there is no way to find it again
/// afterwards.
pub fn expand(prefixes: &BTreeMap<String, String>, curie: &str) -> Option<String> {
    let (prefix, local) = split_curie(curie.trim())?;
    let base = match prefixes.get(prefix) {
        Some(iri) => iri.as_str(),
        None => DEFAULT_PREFIXES
            .iter()
            .find(|(known, _)| *known == prefix)
            .map(|(_, iri)| *iri)?,
    };
    let mut iri = String::with_capacity(base.len() + local.len());
    iri.push_str(base);
    iri.push_str(local);
    Some(iri)
}

/// Read the OKF view of a frontmatter block. Cannot fail: see the module docs
/// for why a reader that could would be the non-conformant party.
pub fn read(fm: &Frontmatter) -> OkfDoc {
    let block = fm.raw_block();
    let (verified, verified_shape) = read_verified(fm, block);
    OkfDoc {
        doc_type: string_of(fm, "type"),
        title: string_of(fm, "title"),
        description: string_of(fm, "description"),
        version: string_of(fm, "version"),
        status: string_of(fm, "status"),
        generated: read_generated(fm, block),
        verified,
        verified_shape,
        sources: read_sources(fm, block),
        stale_after: string_of(fm, "stale_after"),
        prefixes: read_prefixes(fm, block),
        retained: retained_keys(fm),
    }
}

/// Keys the standardized section claims, and which therefore do not appear in
/// [`OkfDoc::retained`]. `verified_by` and `timestamp` are here because they are
/// *read* — a key this module consumes is not an unknown key.
const CLAIMED: [&str; 12] = [
    "type",
    "title",
    "description",
    "version",
    "status",
    "generated",
    "verified",
    "verified_by",
    "sources",
    "stale_after",
    "prefixes",
    "timestamp",
];

fn retained_keys(fm: &Frontmatter) -> Vec<(String, Option<FieldValue>)> {
    fm.keys()
        .filter(|key| !CLAIMED.contains(key))
        .map(|key| (key.to_owned(), fm.get(key).cloned()))
        .collect()
}

fn read_generated(fm: &Frontmatter, block: &str) -> Option<Generated> {
    let pairs = map_pairs(fm, block, "generated");
    // v0.1 wrote `timestamp:` where v0.2 writes `generated.at`, and the digest
    // says readers may still fall back to it. It is a fallback and never an
    // override: a document carrying both means the newer key.
    let at = pick(&pairs, "at").or_else(|| string_of(fm, "timestamp"));
    if pairs.is_empty() && at.is_none() {
        return None;
    }
    Some(Generated {
        by: pick(&pairs, "by").unwrap_or_default(),
        at,
    })
}

fn read_verified(fm: &Frontmatter, block: &str) -> (Vec<Verification>, VerifiedShape) {
    let entries = block_entries(block, "verified");
    if !entries.is_empty() {
        let mut shape = VerifiedShape::Canonical;
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            match pick(&entry.pairs, "by") {
                Some(by) => out.push(Verification {
                    by,
                    at: pick(&entry.pairs, "at"),
                }),
                // A list item naming no actor under `by:` is not the canonical
                // form however close it looks: `- human:marta` is a string where
                // an entry belongs.
                None => {
                    shape = VerifiedShape::Simplified;
                    if let Some(by) = entry.bare {
                        out.push(Verification { by, at: None });
                    }
                }
            }
        }
        return (out, shape);
    }

    match fm.get("verified") {
        // The vault's simplified shape: a bare truth value, with the actor in a
        // sibling key. `false` is read too — it says nobody has verified this,
        // and the shape still has to be rewritten.
        Some(FieldValue::Bool(true)) => {
            let mut out: Vec<Verification> = fm
                .as_list("verified_by")
                .unwrap_or_default()
                .into_iter()
                .filter(|actor| !actor.trim().is_empty())
                .map(|by| Verification { by, at: None })
                .collect();
            // `verified: true` with no actor at all still asserts a review.
            // Keeping it with an empty `by` keeps the claim *and* its missing
            // evidence; dropping it would quietly discard the claim.
            if out.is_empty() {
                out.push(Verification::default());
            }
            (out, VerifiedShape::Simplified)
        }
        Some(FieldValue::Bool(false)) => (Vec::new(), VerifiedShape::Simplified),
        // A flow list of bare actor strings: `verified: [human:marta]`.
        Some(FieldValue::List(items)) => {
            let out = items
                .iter()
                .filter_map(scalar_of)
                .map(|by| Verification { by, at: None })
                .collect();
            (out, VerifiedShape::Simplified)
        }
        Some(value) => match scalar_of(value) {
            Some(by) => (
                vec![Verification { by, at: None }],
                VerifiedShape::Simplified,
            ),
            None => (Vec::new(), VerifiedShape::Absent),
        },
        None => (Vec::new(), VerifiedShape::Absent),
    }
}

fn read_sources(fm: &Frontmatter, block: &str) -> Vec<Source> {
    let entries = block_entries(block, "sources");
    if !entries.is_empty() {
        return entries
            .into_iter()
            .filter_map(|entry| match entry.bare {
                // A bare string is the vault's simplified form, and OKF requires
                // `resource:` inside an entry — so the string can only be the
                // resource. Nothing else about it is invented.
                Some(resource) => Some(Source {
                    resource,
                    ..Source::default()
                }),
                None => source_of(&entry.pairs),
            })
            .collect();
    }

    // Flow (`sources: [a, b]`) and inline (`sources: https://a`) forms, both of
    // which the property subset models, so they never reach the block reader.
    fm.as_list("sources")
        .unwrap_or_default()
        .into_iter()
        .filter(|text| !text.trim().is_empty())
        .map(|resource| Source {
            resource,
            ..Source::default()
        })
        .collect()
}

/// An entry map with no `resource:` is dropped: it is the only field OKF
/// requires inside an entry, and a source that does not say what it is has
/// nothing a reader could follow.
fn source_of(pairs: &[(String, String)]) -> Option<Source> {
    Some(Source {
        id: pick(pairs, "id"),
        resource: pick(pairs, "resource")?,
        title: pick(pairs, "title"),
        author: pick(pairs, "author"),
        usage_count: pick(pairs, "usage_count").and_then(|count| count.parse().ok()),
        last_modified: pick(pairs, "last_modified"),
    })
}

fn read_prefixes(fm: &Frontmatter, block: &str) -> BTreeMap<String, String> {
    let mut out = default_prefixes();
    for (prefix, iri) in map_pairs(fm, block, "prefixes") {
        if !prefix.is_empty() && !iri.is_empty() {
            out.insert(prefix, iri);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading the shapes the property subset does not model
// ---------------------------------------------------------------------------

/// One item of a block-form list, as written.
#[derive(Debug, Default)]
struct BlockEntry {
    /// A `- value` item carrying no `key:` at all.
    bare: Option<String>,
    /// `- key: value`, plus the indented `key: value` lines beneath it.
    pairs: Vec<(String, String)>,
}

/// The pairs of a one-level map, from wherever the document put them.
fn map_pairs(fm: &Frontmatter, block: &str, key: &str) -> Vec<(String, String)> {
    if let Some(FieldValue::Map(pairs)) = fm.get(key) {
        return pairs
            .iter()
            .filter_map(|(name, value)| scalar_of(value).map(|text| (name.clone(), text)))
            .collect();
    }
    match block_entries(block, key).into_iter().next() {
        Some(entry) => entry.pairs,
        None => Vec::new(),
    }
}

/// Read the block-form list (or map) under `key` out of the raw frontmatter
/// block.
///
/// This exists because the property subset stops at one level of nesting, so a
/// canonical `sources:`/`verified:` — a list of maps — reaches
/// [`Frontmatter::get`] as `None`. The scanner is right to record it that way;
/// this reader is the other half of the answer. See the module docs.
///
/// Tolerances, each one a shape a real vault contains:
/// - `- key: value` starts an entry, indented `key: value` lines extend it;
/// - `- value` with no `key:` is a bare item;
/// - indented pairs with no `-` above them are a single implicit entry, which is
///   how a one-source document tends to get written;
/// - a flow map on the key's own line (`generated: {by: x}`) is one entry. The
///   vault's own digest writes `usage_window:` that way, so this is not a
///   hypothetical shape. Commas inside a quoted value are not accounted for,
///   which is one of the reasons the block form is the canonical one.
///
/// A second level of nesting flattens into the entry it sits under rather than
/// being dropped. Flattening loses structure; dropping loses knowledge.
fn block_entries(block: &str, key: &str) -> Vec<BlockEntry> {
    let mut out: Vec<BlockEntry> = Vec::new();
    let mut inside = false;
    let mut at = 0usize;

    while let Some((start, end, next)) = line_bounds(block, at) {
        at = next;
        let line = &block[start..end];
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indented = trimmed.len() < line.len();

        if !inside {
            if indented {
                continue;
            }
            // The fences and every other top-level key land here; only `key:`
            // with no value of its own opens a block for us to read.
            let Some((name, colon)) = split_key(trimmed) else {
                continue;
            };
            if name != key {
                continue;
            }
            let text = scalar_text(&trimmed[colon + 1..]);
            if let Some(pairs) = flow_map(&text) {
                out.push(BlockEntry { bare: None, pairs });
                return out;
            }
            if text.is_empty() {
                inside = true;
            } else {
                // A scalar or a flow list: shapes the property subset models,
                // and the caller reads those through `Frontmatter`.
                return out;
            }
            continue;
        }

        if !indented {
            // Unindented: the block ended, either at the next key or at the
            // closing fence.
            break;
        }

        let item = match dash_rest(trimmed) {
            Some(rest) => {
                out.push(BlockEntry::default());
                rest
            }
            None => {
                if out.is_empty() {
                    out.push(BlockEntry::default());
                }
                trimmed
            }
        };
        let Some(entry) = out.last_mut() else {
            continue;
        };
        if item.is_empty() {
            continue;
        }
        match split_key(item) {
            Some((name, colon)) => {
                let text = scalar_text(&item[colon + 1..]);
                if !text.is_empty() {
                    entry.pairs.push((name.to_owned(), text));
                }
            }
            None if entry.bare.is_none() => entry.bare = Some(scalar_text(item)),
            None => {}
        }
    }

    out
}

/// `- item`, or a lone `-` opening an entry whose pairs are on the lines below.
fn dash_rest(trimmed: &str) -> Option<&str> {
    if trimmed == "-" {
        return Some("");
    }
    trimmed.strip_prefix("- ").map(str::trim_start)
}

/// Split `key: …`, returning the key and the byte offset of its colon.
///
/// Deliberately the same rule as the frontmatter scanner's own splitter, which
/// is private to it: a `:` only separates a key from a value when a space or the
/// line end follows. The two must agree, and the line that proves it is
/// `resource: https://example.com` — a reader that split on every colon would
/// read that entry's resource as `//example.com` and file a source nobody can
/// follow.
fn split_key(trimmed: &str) -> Option<(&str, usize)> {
    let colon = trimmed.find(':')?;
    let bytes = trimmed.as_bytes();
    if colon + 1 < bytes.len() && !matches!(bytes[colon + 1], b' ' | b'\t') {
        return None;
    }
    let key = unquote(trimmed[..colon].trim_end());
    (!key.is_empty() && !key.contains('#')).then_some((key, colon))
}

/// One scalar as a human wrote it, read back as its text: padding, a trailing
/// comment and surrounding quotes removed.
///
/// A `#` only opens a comment when whitespace precedes it — the same rule the
/// frontmatter scanner uses — so `resource: https://example.com/a#frag` keeps
/// its fragment. Inside a quoted value nothing is stripped but the quotes and
/// their escapes.
fn scalar_text(text: &str) -> String {
    let text = text.trim();
    for quote in ['"', '\''] {
        if let Some(rest) = text.strip_prefix(quote) {
            let Some(close) = rest.rfind(quote) else {
                break;
            };
            let inner = &rest[..close];
            return match quote {
                '"' => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
                _ => inner.replace("''", "'"),
            };
        }
    }
    strip_comment(text).trim_end().to_owned()
}

fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'#' && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t')) {
            return &text[..i];
        }
    }
    text
}

fn unquote(text: &str) -> &str {
    for quote in ['"', '\''] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            return &text[1..text.len() - 1];
        }
    }
    text
}

/// `{by: claude/opus-5, at: 2026-08-18T00:00:00Z}` — the flow spelling of a
/// one-level map. `None` when the text is not a flow map at all.
fn flow_map(text: &str) -> Option<Vec<(String, String)>> {
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    let mut pairs = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((name, colon)) = split_key(part) {
            let value = scalar_text(&part[colon + 1..]);
            if !value.is_empty() {
                pairs.push((name.to_owned(), value));
            }
        }
    }
    Some(pairs)
}

fn pick(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
}

fn string_of(fm: &Frontmatter, key: &str) -> Option<String> {
    fm.get(key).and_then(scalar_of)
}

/// A scalar field as text. `None` for an empty string as well as for a list or a
/// map, so that `title:` with nothing after it reads as absent rather than as a
/// title made of no characters.
fn scalar_of(value: &FieldValue) -> Option<String> {
    match value {
        FieldValue::Str(text) if text.trim().is_empty() => None,
        FieldValue::Str(_) | FieldValue::Num(_) | FieldValue::Bool(_) => Some(value.index_string()),
        FieldValue::List(_) | FieldValue::Map(_) => None,
    }
}

/// `prefix:local`, the compact form every RDF serialisation understands.
///
/// The shape rule is the link parser's rule, because the two are the two ends of
/// one pipeline: a token that reads as a predicate on a link must be a token
/// this expands, or a link would carry an edge no exporter can name.
fn split_curie(token: &str) -> Option<(&str, &str)> {
    let (prefix, local) = token.split_once(':')?;
    (is_name(prefix) && is_name(local)).then_some((prefix, local))
}

/// A CURIE half: a letter, then letters, digits, `_` or `-`.
fn is_name(part: &str) -> bool {
    let mut chars = part.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(source: &str) -> OkfDoc {
        let (fm, _) = Frontmatter::parse(source);
        read(&fm)
    }

    /// The canonical v0.2 block, in the shape of the vault's own
    /// `.okf/OKF-0.2-digest.md`: entry-map sources, `generated` as a nested map,
    /// `verified` as a list of entries.
    const CANONICAL: &str = "---
type: Reference
title: Open Knowledge Format v0.2 — working digest
description: What v0.2 requires, written down locally.
status: stable
stale_after: 2026-12-31
sources:
  - id: spec
    resource: https://example.com/okf/SPEC.md
    title: Open Knowledge Format v0.2 specification
    author: human:marta
    usage_count: 42
    last_modified: 2026-07-30
generated:
  by: claude/opus-5
  at: 2026-08-18T00:00:00Z
verified:
  - by: human:tgorka
    at: 2026-08-18T10:00:00Z
---
body
";

    #[test]
    fn the_canonical_shape_reads_whole() {
        let okf = doc(CANONICAL);

        assert_eq!(okf.doc_type.as_deref(), Some("Reference"));
        assert_eq!(
            okf.title.as_deref(),
            Some("Open Knowledge Format v0.2 — working digest")
        );
        assert_eq!(
            okf.description.as_deref(),
            Some("What v0.2 requires, written down locally.")
        );
        assert_eq!(okf.status.as_deref(), Some("stable"));
        assert_eq!(okf.stale_after.as_deref(), Some("2026-12-31"));

        let generated = okf.generated.expect("generated: is present");
        assert_eq!(generated.by, "claude/opus-5");
        assert_eq!(generated.at.as_deref(), Some("2026-08-18T00:00:00Z"));
        assert_eq!(generated.actor_kind(), ActorKind::Tool);

        assert_eq!(okf.verified_shape, VerifiedShape::Canonical);
        assert_eq!(okf.verified.len(), 1);
        assert_eq!(okf.verified[0].by, "human:tgorka");
        assert_eq!(okf.verified[0].at.as_deref(), Some("2026-08-18T10:00:00Z"));
        assert_eq!(okf.verified[0].actor_kind(), ActorKind::Person);

        assert_eq!(
            okf.sources,
            vec![Source {
                id: Some("spec".to_owned()),
                resource: "https://example.com/okf/SPEC.md".to_owned(),
                title: Some("Open Knowledge Format v0.2 specification".to_owned()),
                author: Some("human:marta".to_owned()),
                usage_count: Some(42),
                last_modified: Some("2026-07-30".to_owned()),
            }]
        );
    }

    /// The canonical shape is *two levels* of nesting, which the property subset
    /// does not model — so the scanner declines it and this module reads it
    /// anyway. If this assertion ever fails because the scanner grew wider, the
    /// block readers here are the thing to delete.
    #[test]
    fn the_canonical_shape_is_read_despite_the_scanner_declining_it() {
        let (fm, _) = Frontmatter::parse(CANONICAL);
        assert!(
            fm.get("sources").is_none(),
            "a list of maps is not modelled"
        );
        assert!(fm.unparsed().is_some(), "and the scanner says so");
        assert_eq!(read(&fm).sources.len(), 1);
    }

    /// The vault's simplified shape carries the same facts in the pre-digest
    /// spelling: bare URL strings for sources, and `verified: true` beside a
    /// `verified_by:` actor. It must normalise to the canonical values, because
    /// the spec wins on what is written and the disk wins on what is accepted.
    #[test]
    fn the_simplified_shape_normalises_to_the_canonical_values() {
        let canonical = doc("---
type: Note
sources:
  - resource: https://example.com/a
verified:
  - by: human:tgorka
---
");
        let simplified = doc("---
type: Note
sources:
  - https://example.com/a
verified: true
verified_by: human:tgorka
---
");

        assert_eq!(simplified.doc_type, canonical.doc_type);
        assert_eq!(simplified.sources, canonical.sources);
        assert_eq!(simplified.sources[0].resource, "https://example.com/a");
        assert_eq!(simplified.verified, canonical.verified);
        assert_eq!(simplified.verified[0].actor_kind(), ActorKind::Person);

        // The one difference, and the reason the field exists: a writer emitting
        // the canonical form has to delete `verified_by:` for one of these two
        // documents and not for the other.
        assert_eq!(canonical.verified_shape, VerifiedShape::Canonical);
        assert_eq!(simplified.verified_shape, VerifiedShape::Simplified);
    }

    /// A quoted `verified_by:` holding an unprefixed name — the vault's original
    /// example — is read, and classifies as Unknown. Reading it as a person
    /// would manufacture the human-review claim the actor shapes exist to
    /// protect.
    #[test]
    fn an_unprefixed_verifier_is_read_but_not_promoted_to_a_person() {
        let okf = doc("---\ntype: Note\nverified: true\nverified_by: \"Jan Kowalski\"\n---\n");
        assert_eq!(okf.verified.len(), 1);
        assert_eq!(okf.verified[0].by, "Jan Kowalski");
        assert_eq!(okf.verified[0].actor_kind(), ActorKind::Unknown);
        assert_eq!(okf.verified_shape, VerifiedShape::Simplified);
    }

    /// `verified: true` with no actor still asserts a review. Dropping it would
    /// discard the claim; keeping it with an empty actor keeps the claim and its
    /// missing evidence together.
    #[test]
    fn a_bare_verified_true_keeps_the_claim_and_its_missing_actor() {
        let okf = doc("---\ntype: Note\nverified: true\n---\n");
        assert_eq!(okf.verified, vec![Verification::default()]);
        assert_eq!(okf.verified[0].actor_kind(), ActorKind::Unknown);
        assert_eq!(okf.verified_shape, VerifiedShape::Simplified);
    }

    /// A list of bare actor strings, in either spelling, is read — and is not
    /// the canonical shape however close the block form looks to it. An item
    /// naming no actor under `by:` is a string where an entry belongs, so the
    /// writer has one to rewrite. `verified: false` is the same decision from
    /// the other side: nobody has checked this, and the key still needs
    /// rewriting.
    #[test]
    fn bare_actor_lists_are_read_and_flagged_for_rewriting() {
        let block = doc("---
type: Note
verified:
  - human:marta
  - claude/opus-5
---
");
        assert_eq!(
            block
                .verified
                .iter()
                .map(|entry| entry.by.as_str())
                .collect::<Vec<_>>(),
            vec!["human:marta", "claude/opus-5"]
        );
        assert_eq!(block.verified_shape, VerifiedShape::Simplified);

        let flow = doc("---\ntype: Note\nverified: [human:marta]\n---\n");
        assert_eq!(flow.verified.len(), 1);
        assert_eq!(flow.verified[0].by, "human:marta");
        assert_eq!(flow.verified_shape, VerifiedShape::Simplified);

        let denied = doc("---\ntype: Note\nverified: false\n---\n");
        assert!(denied.verified.is_empty());
        assert_eq!(denied.verified_shape, VerifiedShape::Simplified);
    }

    /// OKF requires consumers to preserve unknown keys and forbids rejecting a
    /// document for having them. Both halves here: a key nobody has heard of,
    /// and a key whose *value* the property subset cannot model — the latter
    /// survives without its value, because losing the key is the failure and
    /// losing the value is only a limitation.
    #[test]
    fn an_unknown_key_survives_the_typed_read() {
        let okf = doc("---
type: Note
tags: [okf, reference]
x-neuradrive-mood: cheerful
odd: |
  a block scalar
---
");

        let named: Vec<&str> = okf.retained.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(named, vec!["tags", "x-neuradrive-mood", "odd"]);
        assert_eq!(
            okf.retained[1].1,
            Some(FieldValue::Str("cheerful".to_owned()))
        );
        assert_eq!(
            okf.retained[2].1, None,
            "the key survives without its value"
        );
    }

    /// A key this module reads is not an unknown key, and must not be reported
    /// as one — a writer that re-emitted `verified_by:` out of `retained` after
    /// writing the canonical form would leave both spellings in the file.
    #[test]
    fn keys_the_view_claims_are_not_reported_as_unknown() {
        let okf = doc("---\ntype: Note\nverified: true\nverified_by: human:marta\n---\n");
        assert!(okf.retained.is_empty());
    }

    /// A missing `type` is the document's problem to report, not this reader's
    /// to fail on: OKF forbids rejecting a document over a field, and every
    /// other field here is optional by the format's own text.
    #[test]
    fn a_missing_type_is_not_an_error() {
        let okf = doc("---\ntitle: No type here\n---\nbody\n");
        assert_eq!(okf.doc_type, None);
        assert_eq!(okf.title.as_deref(), Some("No type here"));
        assert_eq!(okf.verified_shape, VerifiedShape::Absent);
        assert!(okf.sources.is_empty());

        // And a document with no frontmatter at all reads as an empty view that
        // still knows the standard prefixes.
        assert_eq!(
            doc("just a body\n"),
            OkfDoc {
                prefixes: default_prefixes(),
                ..OkfDoc::default()
            }
        );
    }

    #[test]
    fn every_actor_shape_classifies() {
        for (actor, kind) in [
            // The spec's three.
            ("claude/opus-5", ActorKind::Tool),
            ("human:marta", ActorKind::Person),
            ("process:okf-index", ActorKind::Process),
            // The vault's extensions. `user:` is a person: it names a human
            // being, so it carries a human's trust.
            ("user:tgorka", ActorKind::Person),
            ("service:keeper-syncd", ActorKind::Service),
            ("agent:reviewer", ActorKind::Agent),
            ("bot:dependabot", ActorKind::Bot),
            // Shapes that declare nothing. None of them may borrow a person's
            // trust by looking a little like one.
            ("Jan Kowalski", ActorKind::Unknown),
            ("", ActorKind::Unknown),
            ("human:", ActorKind::Unknown),
            ("robot:x", ActorKind::Unknown),
            ("/opus-5", ActorKind::Unknown),
            ("claude/", ActorKind::Unknown),
            ("claude opus/5", ActorKind::Unknown),
            ("https://example.com/v2", ActorKind::Unknown),
        ] {
            assert_eq!(actor_kind(actor), kind, "actor {actor:?}");
        }
    }

    #[test]
    fn expand_resolves_declared_and_default_prefixes_and_refuses_the_rest() {
        let okf = doc("---\ntype: Note\nprefixes:\n  ex: https://example.com/ns#\n---\n");

        // Declared by the file.
        assert_eq!(
            expand(&okf.prefixes, "ex:thing").as_deref(),
            Some("https://example.com/ns#thing")
        );
        // Inherited: a file's own map extends the defaults, never replaces them.
        assert_eq!(
            expand(&okf.prefixes, "schema:creator").as_deref(),
            Some("https://schema.org/creator")
        );
        // A drive's own vocabulary is NOT inherited: `nd:` lives in
        // neuradrive's registry and `td:` in tgdrive's, keeper indexes both, and
        // a table here that knew one would be silently wrong about the other.
        // Unbound is the honest answer; the CURIE still renders as written.
        assert_eq!(expand(&okf.prefixes, "nd:contradicts"), None);
        // A document that wants it expanded says so itself, which is the one
        // place the base is true for that document.
        let own = doc("---\ntype: Note\nprefixes:\n  nd: urn:neuradrive:prop:\n---\n");
        assert_eq!(
            expand(&own.prefixes, "nd:contradicts").as_deref(),
            Some("urn:neuradrive:prop:contradicts")
        );
        // Undeclared: a reportable condition, never a guess.
        assert_eq!(expand(&okf.prefixes, "zz:thing"), None);
        // Not a CURIE at all.
        for token in ["not a curie", "a:", ":b", "schema", "1x:y", "sche ma:y"] {
            assert_eq!(expand(&okf.prefixes, token), None, "token {token:?}");
        }
    }

    /// A file may redeclare a default prefix, and then its answer wins — for
    /// that prefix and for nothing else.
    #[test]
    fn a_file_may_shadow_one_default_prefix() {
        let okf = doc("---\ntype: Note\nprefixes:\n  schema: https://example.com/schema#\n---\n");
        assert_eq!(
            expand(&okf.prefixes, "schema:creator").as_deref(),
            Some("https://example.com/schema#creator")
        );
        assert_eq!(
            expand(&okf.prefixes, "foaf:knows").as_deref(),
            Some("http://xmlns.com/foaf/0.1/knows")
        );
    }

    /// Exactly the agreed table, with no drift in it: a default that changed
    /// silently would move every triple minted under it.
    ///
    /// Nine published vocabularies and nothing else. The absence is the
    /// assertion — a drive's own base (`nd:`, `td:`) belongs to that drive's
    /// registry, and this test is what fails if one is ever added here.
    #[test]
    fn the_default_prefix_table_is_the_agreed_one() {
        let defaults = default_prefixes();
        assert_eq!(defaults.len(), 9);
        assert_eq!(
            defaults.get("dcterms").map(String::as_str),
            Some("http://purl.org/dc/terms/")
        );
        assert_eq!(
            defaults.get("prov").map(String::as_str),
            Some("http://www.w3.org/ns/prov#")
        );
        assert_eq!(
            defaults.get("nd"),
            None,
            "a per-drive base is not a default"
        );
        assert_eq!(defaults.get("td"), None, "and neither is the other drive's");
    }

    /// The flow spelling of `generated:`. The vault's own digest writes
    /// `usage_window:` that way, so this shape is measured rather than imagined.
    #[test]
    fn a_flow_map_generated_reads_the_same_as_the_block_form() {
        let okf =
            doc("---\ntype: Note\ngenerated: {by: claude/opus-5, at: 2026-08-18T00:00:00Z}\n---\n");
        let generated = okf.generated.expect("a flow map is still a map");
        assert_eq!(generated.by, "claude/opus-5");
        assert_eq!(generated.at.as_deref(), Some("2026-08-18T00:00:00Z"));
    }

    /// v0.1 wrote `timestamp:` where v0.2 writes `generated.at`, and the digest
    /// lets readers keep reading it. A fallback, never an override.
    #[test]
    fn a_v0_1_timestamp_fills_in_for_a_missing_generated_at() {
        let fallback = doc("---\ntype: Note\ntimestamp: 2026-01-02T03:04:05Z\n---\n");
        let generated = fallback.generated.expect("a timestamp still says when");
        assert_eq!(generated.at.as_deref(), Some("2026-01-02T03:04:05Z"));
        assert_eq!(generated.by, "", "and v0.1 said nothing about by whom");
        assert_eq!(generated.actor_kind(), ActorKind::Unknown);

        let both = doc("---
type: Note
timestamp: 2026-01-02T03:04:05Z
generated:
  by: claude/opus-5
  at: 2026-08-18T00:00:00Z
---
");
        assert_eq!(
            both.generated.and_then(|g| g.at).as_deref(),
            Some("2026-08-18T00:00:00Z")
        );
    }

    /// A `generated:` block that omits the required `by` keeps its `at`. Losing
    /// a timestamp to punish a missing actor would lose knowledge to make a
    /// point, and the empty actor already reads as the lowest trust there is.
    #[test]
    fn a_generated_block_without_an_actor_keeps_its_time() {
        let okf = doc("---\ntype: Note\ngenerated:\n  at: 2026-08-18T00:00:00Z\n---\n");
        let generated = okf.generated.expect("an at: is still something");
        assert_eq!(generated.by, "");
        assert_eq!(generated.actor_kind(), ActorKind::Unknown);
    }

    /// Sources in every spelling a vault contains, including one mixed list —
    /// the shape a document acquires halfway through being upgraded.
    #[test]
    fn sources_read_from_flow_inline_and_mixed_lists() {
        let flow =
            doc("---\ntype: Note\nsources: [https://example.com/a, https://example.com/b]\n---\n");
        assert_eq!(
            flow.sources
                .iter()
                .map(|source| source.resource.as_str())
                .collect::<Vec<_>>(),
            vec!["https://example.com/a", "https://example.com/b"]
        );

        let inline = doc("---\ntype: Note\nsources: https://example.com/a\n---\n");
        assert_eq!(inline.sources.len(), 1);
        assert_eq!(inline.sources[0].resource, "https://example.com/a");

        let mixed = doc("---
type: Note
sources:
  - https://example.com/a
  - id: b
    resource: https://example.com/b
---
");
        assert_eq!(mixed.sources.len(), 2);
        assert_eq!(mixed.sources[0].resource, "https://example.com/a");
        assert_eq!(mixed.sources[0].id, None);
        assert_eq!(mixed.sources[1].id.as_deref(), Some("b"));

        // An entry with no `resource:` has nothing a reader could follow.
        let empty = doc("---\ntype: Note\nsources:\n  - title: Nowhere\n---\n");
        assert!(empty.sources.is_empty());
    }

    /// A URL fragment is not a YAML comment, and a quoted title keeps the colons
    /// and hashes inside it. Both shapes live in the vault's registry files
    /// today.
    #[test]
    fn entry_values_survive_fragments_comments_and_quotes() {
        let okf = doc("---
type: Note
sources:
  - resource: https://example.com/a#results   # the run itself
    title: \"Original revision: the framing #1\"
---
");
        assert_eq!(okf.sources.len(), 1);
        assert_eq!(okf.sources[0].resource, "https://example.com/a#results");
        assert_eq!(
            okf.sources[0].title.as_deref(),
            Some("Original revision: the framing #1")
        );
    }

    /// The block reader stops at the next top-level key. A `verified:` list must
    /// not swallow the `sources:` entries written beneath it.
    #[test]
    fn one_block_does_not_swallow_the_next_key() {
        let okf = doc("---
type: Note
verified:
  - by: human:marta
sources:
  - resource: https://example.com/a
tags: [okf]
---
");
        assert_eq!(okf.verified.len(), 1);
        assert_eq!(okf.verified[0].by, "human:marta");
        assert_eq!(okf.sources.len(), 1);
        assert_eq!(okf.retained.len(), 1, "tags is the only unclaimed key");
    }
}

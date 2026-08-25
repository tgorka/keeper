//! The owner's own document, read end to end.
//!
//! Every other test for this feature exercises one seam: the attribute reader,
//! the frontmatter view, the index projection. This one is the acceptance test,
//! and it is deliberately the file the owner wrote in the request rather than a
//! fixture tuned to pass — including the parts where their example and OKF v0.2
//! disagree, because tolerating those without rewriting the author's file is
//! half of what was asked for.
//!
//! What it pins:
//!
//!   - all three predicate spellings from the request: several CURIEs in one
//!     brace pair, several in adjacent pairs, and one on its own,
//!   - predicates on an external link, which is not a vault edge and still
//!     carries its predicate for the RDF projection,
//!   - the simplified trust block (`verified: true` + `verified_by:`) reading as
//!     a verification, and being flagged as the shape a writer must rewrite,
//!   - `sources:` written as bare URLs, which OKF requires to be entries,
//!   - `generated.by: service:…`, one of the four actor prefixes the owner added
//!     to OKF's three,
//!   - the declared `prefixes:` map expanding a CURIE that the built-in table
//!     does not know the base for.
//!
//! The example's `type: TechnologyConcept` is not in this drive's registry and
//! that is not this test's business: OKF forbids a consumer rejecting a document
//! for an unknown type, so the reader must simply carry it.

use keeper_core::notes::frontmatter::Frontmatter;
use keeper_core::notes::links;
use keeper_core::notes::okf::{self, ActorKind, VerifiedShape};

/// The request's document, trimmed to the parts that carry structure. The link
/// lines are verbatim.
const DOCUMENT: &str = r#"---
# === SEKCJA STANDARYZOWANA OKF v0.2 ===
title: Ekosystem Blockchain i Kryptowalut
type: TechnologyConcept
description: Formalna mapa relacji semantycznych w architekturze zdecentralizowanej ksiegi glownej.
version: "1.2.0"
status: verified
generated:
  at: 2026-08-23T16:00:00Z
  by: service:Agentic-Knowledge-Parser-v2
verified: true
verified_by: "Jan Kowalski (Lead Data Architect)"
stale_after: 2027-08-23T16:00:00Z
sources:
  - "https://bitcoin.org"
  - "https://ethereum.org"

# === SEKCJA TWOICH PREFIKSOW (ROZSZERZENIE GRAFOWE) ===
prefixes:
  schema: https://schema.org
  foaf: http://xmlns.com
  dcterms: http://purl.org
  skos: http://w3.org
---

# Analiza Ekosystemu Blockchain

## Powiazania Semantyczne i Relacje

### 1. Wiele predykatow w jednych klamrach
*   **Glowny Architekt:** [Satoshi Nakamoto](Satoshi_Nakamoto.md){schema:creator, foaf:knows}
*   **Glowne Repozytorium:** [Bitcoin Core GitHub](https://github.com){schema:codeRepository, dcterms:relation}

### 2. Wiele predykatow w osobnych klamrach
*   **Dokumentacja Bazowa:** [Bitcoin Whitepaper](Whitepaper.pdf){dcterms:source}{schema:creativeWorkStatus}
*   **Sasiadujacy Protokol:** [Ethereum](Ethereum.md){skos:related}{schema:subTechnology}

### 3. Pojedyncze predykaty
*   **Dziedzina Nadrzedna:** [Kryptografia](Kryptografia.md){skos:broader}
"#;

/// The predicates written on one link, found by the text of its target.
fn predicates_for(target: &str) -> Vec<String> {
    links::extract(DOCUMENT)
        .into_iter()
        .find(|link| link.target == target)
        .unwrap_or_else(|| panic!("no link to {target} was extracted"))
        .predicates
}

/// The owner's SECOND document, supplied after the first shipped, and the
/// reason this file has two fixtures rather than one edited into agreement.
///
/// The link lines and the fence line are verbatim. This is the format the
/// owner's tooling actually writes: kramdown/Python-Markdown **IAL** braces
/// carrying Semantic-Markdown-V0 property attributes, where a predicate is
/// spelled `:name` against the document's default vocabulary rather than with a
/// published prefix. keeper shipped only the prefixed form first, so every one
/// of these lines read as prose.
const IAL_DOCUMENT: &str = r#"---
title: Revenue Tracking Logic
type: Concept
---

### Revenue Tracking Logic

We track daily revenue using this specific configuration block:

```json { :type="Metric" :owned_by="https://company.internal" }
{
  "metric_name": "daily_gross_revenue",
  "aggregation": "SUM",
  "field": "invoice.amount_paid"
}
```
The checkout pipeline relies heavily on the **[JWT Auth Service](https://github.com)**{ :depends_on }.

This entire system block is actively **[Managed by the Platform Team](https://company.internal)**{ :owned_by }.
"#;

#[test]
fn every_predicate_spelling_in_the_request_is_read() {
    // Several CURIEs in one brace pair, comma separated.
    assert_eq!(
        predicates_for("Satoshi_Nakamoto.md"),
        ["schema:creator", "foaf:knows"],
        "two predicates in one pair of braces, in the order written"
    );

    // Several CURIEs in adjacent brace pairs. The distinction is a formatting
    // choice by the author and must not be a semantic one.
    assert_eq!(
        predicates_for("Whitepaper.pdf"),
        ["dcterms:source", "schema:creativeWorkStatus"]
    );
    assert_eq!(
        predicates_for("Ethereum.md"),
        ["skos:related", "schema:subTechnology"]
    );

    // One on its own.
    assert_eq!(predicates_for("Kryptografia.md"), ["skos:broader"]);
}

#[test]
fn an_external_link_carries_its_predicates_even_though_it_is_not_a_vault_edge() {
    // `extract` deliberately keeps external destinations out of the vault graph,
    // so this link is not a backlink anywhere. Its predicates still have to be
    // readable: `schema:codeRepository` on a GitHub URL is exactly the triple
    // the RDF projection exists to emit.
    let external = links::extract(DOCUMENT)
        .into_iter()
        .find(|link| link.target.starts_with("https://github.com"));
    match external {
        Some(link) => assert_eq!(
            link.predicates,
            ["schema:codeRepository", "dcterms:relation"]
        ),
        None => {
            // Extraction policy owns whether an external link is an edge. If it
            // is not extracted at all, the reader still has to see the block —
            // proven directly, so this test cannot pass by silently skipping.
            let at = DOCUMENT
                .find("](https://github.com)")
                .expect("the external link is in the document")
                + "](https://github.com)".len();
            let blocks = links::read_attrs(DOCUMENT, at).expect("its attribute block");
            assert_eq!(
                blocks.predicates,
                ["schema:codeRepository", "dcterms:relation"]
            );
        }
    }
}

#[test]
fn the_owners_trust_block_is_read_without_being_rewritten() {
    let (fm, _) = Frontmatter::parse(DOCUMENT);
    let doc = okf::read(&fm);

    assert_eq!(doc.doc_type.as_deref(), Some("TechnologyConcept"));
    assert_eq!(
        doc.title.as_deref(),
        Some("Ekosystem Blockchain i Kryptowalut")
    );
    assert_eq!(doc.version.as_deref(), Some("1.2.0"));
    assert_eq!(doc.stale_after.as_deref(), Some("2027-08-23T16:00:00Z"));

    // `service:` is one of the four actor prefixes the owner added to OKF's
    // three. It must not read as a person: trust is derived from this.
    let generated = doc.generated.expect("a generated block");
    assert_eq!(generated.by, "service:Agentic-Knowledge-Parser-v2");
    assert_eq!(generated.actor_kind(), ActorKind::Service);
    assert_eq!(generated.at.as_deref(), Some("2026-08-23T16:00:00Z"));

    // `verified: true` + `verified_by:` is not OKF v0.2's list of `{by, at}`.
    // It is read rather than refused, and flagged so a writer knows to emit the
    // canonical shape and delete the legacy keys.
    assert_eq!(doc.verified_shape, VerifiedShape::Simplified);
    assert_eq!(doc.verified.len(), 1);
    assert_eq!(doc.verified[0].by, "Jan Kowalski (Lead Data Architect)");
    // An unprefixed name is Unknown, never Person: guessing here would
    // manufacture the human-review claim the actor shapes exist to prevent.
    assert_eq!(doc.verified[0].actor_kind(), ActorKind::Unknown);

    // Bare URLs where OKF wants entries with a `resource:`.
    let sources: Vec<&str> = doc.sources.iter().map(|s| s.resource.as_str()).collect();
    assert_eq!(sources, ["https://bitcoin.org", "https://ethereum.org"]);
}

#[test]
fn the_documents_own_prefixes_expand_its_predicates() {
    let (fm, _) = Frontmatter::parse(DOCUMENT);
    let doc = okf::read(&fm);

    // The file declares four bases, and they win over the built-in table: a
    // document that says what its prefixes mean is the authority on its own
    // CURIEs.
    assert_eq!(
        okf::expand(&doc.prefixes, "foaf:knows").as_deref(),
        Some("http://xmlns.comknows"),
        "the declared base is used verbatim, without keeper inventing a separator"
    );

    // A prefix the file does not declare falls back to the built-in table.
    assert_eq!(
        okf::expand(&doc.prefixes, "prov:wasDerivedFrom").as_deref(),
        Some("http://www.w3.org/ns/prov#wasDerivedFrom")
    );

    // An undeclared prefix is refused rather than guessed at. A wrong predicate
    // in a graph somebody queries is worse than an absent one.
    assert_eq!(okf::expand(&doc.prefixes, "bogus:thing"), None);

    // Every predicate the document actually writes resolves.
    for target in [
        "Satoshi_Nakamoto.md",
        "Whitepaper.pdf",
        "Ethereum.md",
        "Kryptografia.md",
    ] {
        for predicate in predicates_for(target) {
            assert!(
                okf::expand(&doc.prefixes, &predicate).is_some(),
                "{predicate} on {target} did not expand"
            );
        }
    }
}

/// The owner's second document, read at the only layer that can answer for it.
///
/// Every predicate here is one keeper read as prose before this change, and each
/// line breaks a different assumption: the predicate is spelled `:name` with an
/// empty prefix rather than `prefix:name`, and the brace pair sits after the
/// `**` that closes the emphasis rather than after the link's `)`.
///
/// Read through `read_attrs` and not `extract`, because both of the owner's
/// links point at external URLs and `extract` deliberately keeps those out of
/// the vault graph — there is no note at the far end to be a backlink of. The
/// twin test below runs the same two lines through `extract` with internal
/// targets, so the emphasis rule itself is not tested only by a helper here.
#[test]
fn the_ial_document_carries_its_predicates() {
    // Mirrors `extract`'s adjacency rule rather than restating it as a constant:
    // a block may sit behind the emphasis markers that close on the link.
    let after_emphasis = |needle: &str| {
        let at = IAL_DOCUMENT
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} is in the document"))
            + needle.len();
        at + IAL_DOCUMENT[at..]
            .bytes()
            .take_while(|b| matches!(b, b'*' | b'_'))
            .count()
    };

    let jwt = links::read_attrs(IAL_DOCUMENT, after_emphasis("](https://github.com)"))
        .expect("the JWT link's attribute block");
    assert_eq!(jwt.predicates, ["depends_on"]);

    let owned = links::read_attrs(IAL_DOCUMENT, after_emphasis("](https://company.internal)"))
        .expect("the ownership link's attribute block");
    assert_eq!(owned.predicates, ["owned_by"]);
}

/// The owner's two lines with vault targets, through the whole extraction path.
///
/// This is the test that actually pins emphasis adjacency, because `extract`
/// resolves it — the test above can only look at an offset it computed itself.
#[test]
fn the_ial_shape_survives_extraction_when_the_target_is_a_note() {
    let body = "relies heavily on the **[JWT Auth Service](jwt-auth.md)**{ :depends_on }.\n\
                actively **[Managed by the Platform Team](platform-team.md)**{ :owned_by }.\n";

    let found: Vec<(String, Vec<String>)> = links::extract(body)
        .into_iter()
        .map(|link| (link.target, link.predicates))
        .collect();

    assert_eq!(
        found,
        vec![
            ("jwt-auth.md".to_owned(), vec!["depends_on".to_owned()]),
            ("platform-team.md".to_owned(), vec!["owned_by".to_owned()]),
        ]
    );
}

/// The colon is stripped, and that is a decision rather than a convenience.
///
/// `{ :cites }`, bare `{ cites }` and the older `{rel="cites"}` are the same
/// edge said three ways, and the owner's registry writes its predicates
/// unprefixed — so a reader that kept the colon would show one concept under
/// two names and a graph would carry two predicates where the author meant one.
/// A published prefix is NOT stripped: `schema:creator` is not `creator`.
#[test]
fn the_empty_prefix_collapses_but_a_published_one_does_not() {
    let body = "[a](a.md){ :cites }\n[b](b.md){ cites }\n[c](c.md){schema:creator}\n";
    let spellings: Vec<Vec<String>> = links::extract(body)
        .into_iter()
        .map(|link| link.predicates)
        .collect();

    assert_eq!(
        spellings,
        vec![
            vec!["cites".to_owned()],
            vec!["cites".to_owned()],
            vec!["schema:creator".to_owned()],
        ]
    );
}

/// A fenced block's CONTENT is not a document, and its annotation is nobody's
/// business in this crate.
///
/// keeper reads a fence's info-string annotation nowhere in Rust. The editor
/// draws it from the document it already has open and the vault toolkit emits
/// its triples while standing in the drive that owns the vocabulary; nothing in
/// this crate would consume it, and a parse with no reader rots quietly until
/// someone trusts it. What this crate must get right is the other half: a
/// fenced block is code, so a link written inside one is text.
///
/// The owner's own block is the case that matters — its JSON body carries braces
/// on every line and a `"field": "invoice.amount_paid"` pair that reads exactly
/// like an attribute — so the fixture keeps that shape and adds the harder thing
/// a real vault will eventually contain: a genuine vault link, inside the fence,
/// wearing a predicate.
#[test]
fn nothing_inside_a_fenced_block_becomes_an_edge() {
    let body = "```json { :type=\"Metric\" }\n\
                { \"field\": \"invoice.amount_paid\" }\n\
                see [the ledger](ledger.md){ :cites } for the source\n\
                ```\n\
                and in prose, [the ledger](ledger.md){ :owned_by } is an edge\n";

    let found: Vec<(String, Vec<String>)> = links::extract(body)
        .into_iter()
        .map(|link| (link.target, link.predicates))
        .collect();

    assert_eq!(
        found,
        vec![("ledger.md".to_owned(), vec!["owned_by".to_owned()])],
        "the fenced copy of the same link must not be an edge, and must not \
         contribute its predicate to the prose one"
    );

    // The owner's document reaches the same place from the other direction:
    // both of its links are external, so it contributes no edges at all.
    assert!(links::extract(IAL_DOCUMENT).is_empty());
}

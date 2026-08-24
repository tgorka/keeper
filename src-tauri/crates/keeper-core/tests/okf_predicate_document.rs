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

//! Notes search's semantic query against a real OpenAI-compatible embeddings
//! endpoint, through the provider row that survives the bots' removal (Story
//! 108.3a, R-HQ-12): the row as `bot_providers` keeps it, the endpoint and
//! client `keeper_core::providers` build from it, passages and the query
//! embedded the way the notes vault does, and the answer ranked by the search
//! index's own fusion.
//!
//! Ignored by default: it needs a live endpoint, named by the environment —
//! never by this file:
//!
//! - `KEEPER_EMBED_LIVE_BASE_URL` — the provider's base URL (an Ollama, whose
//!   `/v1/embeddings` is OpenAI's shape);
//! - `KEEPER_EMBED_LIVE_MODEL` — a model that embeds, e.g. `embeddinggemma`;
//! - `KEEPER_EMBED_LIVE_TOKEN_FILE` (optional) — a file holding its bearer key.
//!
//! The first query shares no word with the note it must find, so the vector
//! ranking alone puts it first; the second shares one, and the fused search
//! the notes surface runs ranks it first on both words and meaning.

use std::collections::BTreeMap;

use keeper_core::notes::search_index::{self, NoteDoc, SearchIndex};
use keeper_core::providers::embed::{self, EmbedRequest};
use keeper_core::providers::store::{insert_provider, list_providers};
use keeper_core::providers::{http, parse_base_url, Endpoint, Provider, ProviderKind};

const NOTES: [(&str, &str, &str); 3] = [
    (
        "sourdough",
        "Sourdough starter",
        "Feed the levain twice a day with rye flour and water; it doubles in six hours when warm.",
    ),
    (
        "tyres",
        "Winter tyres",
        "Swap the car's wheels in November and check the tread depth and pressure before driving.",
    ),
    (
        "tax",
        "Annual return",
        "File the yearly income declaration with the revenue office before the end of April.",
    ),
];

/// Shares no word with the sourdough note's title or body.
const QUERY: &str = "how do I keep my bread culture alive";
/// Shares one word with it, so words and meaning both answer.
const FUSED_QUERY: &str = "keeping a starter alive";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

#[tokio::test]
#[ignore = "live: needs KEEPER_EMBED_LIVE_BASE_URL and KEEPER_EMBED_LIVE_MODEL"]
async fn a_semantic_query_finds_the_note_through_the_kept_provider_row() {
    let base_url = parse_base_url(&env("KEEPER_EMBED_LIVE_BASE_URL")).expect("base url");
    let model = env("KEEPER_EMBED_LIVE_MODEL");
    let token = std::env::var("KEEPER_EMBED_LIVE_TOKEN_FILE")
        .ok()
        .map(|file| {
            std::fs::read_to_string(file)
                .expect("token file")
                .trim()
                .to_owned()
        });

    // The provider as the upgrade leaves it: a row in `bot_providers`.
    let dir = tempfile::tempdir().expect("tempdir");
    insert_provider(
        dir.path(),
        &Provider {
            id: "p1".to_owned(),
            kind: ProviderKind::Ollama,
            name: "Embeddings".to_owned(),
            base_url: base_url.normalized,
            created_ms: 1,
        },
    )
    .expect("provider");
    let row = list_providers(dir.path())
        .expect("providers")
        .rows
        .pop()
        .expect("the row");
    let endpoint = Endpoint::new(row.provider.kind, &row.provider.base_url, None, token);
    let client = http::client(row.read_timeout()).expect("client");

    let mut index =
        SearchIndex::open(&dir.path().join("search.db"), "vault-1").expect("search index");
    let (tags, fields) = (Vec::new(), BTreeMap::new());
    for (id, title, body) in NOTES {
        index
            .replace_note(&NoteDoc {
                id,
                path: &format!("{id}.md"),
                title,
                tags: &tags,
                fields: &fields,
                body,
                stat: None,
            })
            .expect("note");
    }

    let pending = index
        .chunks_without_vectors(&model, embed::EMBED_BATCH_MAX)
        .expect("pending");
    assert!(
        pending.len() >= NOTES.len(),
        "every note is chunked: {}",
        pending.len()
    );
    let chunks = pending.len();
    let inputs = pending
        .iter()
        .map(|row| format!("{}{}", embed::passage_prefix(&model), row.embedding_text))
        .collect();
    let vectors = embed::embed(
        &client,
        &endpoint,
        EmbedRequest {
            model: &model,
            inputs,
        },
    )
    .await
    .expect("passages embedded");
    let rows: Vec<_> = pending
        .into_iter()
        .zip(vectors)
        .map(|(row, vector)| (row.rowid, row.text_hash, vector))
        .collect();
    assert_eq!(index.put_vectors(&model, &rows).expect("stored"), chunks);

    let embed_query = |text: &str| {
        let input = format!("{}{text}", embed::query_prefix(&model));
        let (client, endpoint, model) = (&client, &endpoint, &model);
        async move {
            embed::embed(
                client,
                endpoint,
                EmbedRequest {
                    model,
                    inputs: vec![input],
                },
            )
            .await
            .expect("query embedded")
            .pop()
            .expect("one vector")
        }
    };

    // Meaning alone: no word shared, and the nearest chunk is the note's.
    let query = embed_query(QUERY).await;
    let lexical = index.query(QUERY, 20).expect("lexical");
    assert!(lexical.is_empty(), "no word is shared: {lexical:?}");
    let semantic = index
        .cosine_top_k(&model, &query, search_index::VECTOR_POOL)
        .expect("semantic");
    println!("semantic: {semantic:?}");
    assert_eq!(
        semantic.first().map(|hit| hit.note_id.as_str()),
        Some("sourdough")
    );

    // The search the person runs: words and meaning fused, the note first.
    let query = embed_query(FUSED_QUERY).await;
    let lexical = index.query(FUSED_QUERY, 20).expect("lexical");
    let semantic = index
        .cosine_top_k(&model, &query, search_index::VECTOR_POOL)
        .expect("semantic");
    let ranked = search_index::fuse(&lexical, &semantic);
    println!("ranked: {ranked:?}");
    let first = ranked.first().expect("a hit");
    assert_eq!(first.note_id, "sourdough");
    assert_eq!(first.why, search_index::MatchWhy::Both, "{ranked:?}");
}

# keeper-graph-protocol

The knowledge graph's shared contract (graph contract version 1, Syncular schema 6), as safe,
storage-independent Rust: no storage, transport, Matrix, Syncular, SQLite, Tauri, network or
native code (`bun run check:graph-protocol-lean`).

| module | what it fixes |
| --- | --- |
| `vocabulary` | reserved kinds and relationship types, structural fields, lifecycles, entity types, operations, origin classes, outcomes, stable error codes |
| `symbol` | the reversible `KG-Cypher/1` symbols: reserved names as themselves, custom names as `K_`/`R_`/`P_` + lowercase UTF-8 hex |
| `value` | canonical values (one number type, ±2^53−1, no duplicate keys), RFC 8785 bytes and SHA-256 digests, typed references, property checks |
| `request` | the request envelope, its version refusal and the server's request digest |
| `delta` | an event's operation group and its delta digest |
| `scope` | the `kg_graph`/`kg_client` scope values and graph row keys |

Every rule is pinned by `syncular/knowledge-graph.vectors.json`, which keeper-hq's TypeScript
validator also reproduces; the tags in it are checked in keeper-core against
`notes::tags::normalise`. The contract itself is described in `syncular/README.md`
(*The knowledge graph*). This crate accepts nothing: acceptance is the server's graph command.

```sh
cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-graph-protocol
```

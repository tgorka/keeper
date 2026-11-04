# keeper-graph-protocol

Safe, storage-independent KG-Cypher/1 query admission. No native engine, worker,
filesystem, network, Matrix, Syncular or Tauri dependency.

`admit(profile, source, parameter_json)` checks the byte ceilings before decoding,
consumes the complete closed grammar, validates symbols/bindings and returns a
private `AdmittedQuery`. Unsupported syntax returns `QueryUnsupported`; exceeded
source, parameter, path, predicate, return or row ceilings return `QueryLimit`.
Parameter objects reject duplicate keys and composite values; numbers are finite,
within ±9007199254740991 and normalize negative zero. Normalized encoded parameters
are checked against the 64-KiB ceiling too.

The admitted object emits canonical engine text with generated variable and return
alias names. Its `aliases()` maps generated result positions back to caller aliases.
Values remain separately encoded parameters. Scalar queries append canonical node
and edge IDs in declaration order as deterministic ORDER BY tie keys; anonymous
edges are also bound so parallel edges retain a stable order. Count queries have
no row tie tuple. Custom symbols are reversible lowercase UTF-8 hex with K_, R_ and
P_ prefixes; custom properties cannot shadow structural fields.

The renderer requires an **active-only immutable projection**. It does not supply
filesystem confinement, native allocation limits, process deadlines, projection
publication, authority admission or native query execution. Native semantic parity
and Linux/macOS worker proof are separate acceptance gates; parser tests do not
establish them. This package does not expose a successful empty graph fallback on
unsupported platforms.

Run its behavioral corpus with:

```sh
cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-graph-protocol
cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-graph-protocol --all-targets -- -D warnings
```

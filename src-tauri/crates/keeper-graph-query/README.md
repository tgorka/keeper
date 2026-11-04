# keeper-graph-query

Single-use, confined GraphQLite worker for the isolated story 110.1 proof. Linux and
macOS only; on every other target the crate is empty and its build script returns before
any native compilation. Credential-bearing code (`keeper-core`, drivers, the shell) never
links this crate; `keeper_core::graph::Supervisor` spawns its executable.

## Native source

`vendor/graphqlite/` is MIT GraphQLite v0.12.0 at commit
`995a8b9bc25cdd86d4c2b50ff7ff87f0eb236629` (`LICENSE`, `source-sha256.json`).
`generated/` holds the Bison/Flex output with tool versions and hashes in
`generated-provenance.json`. It is compiled with `SQLITE_CORE` against the workspace's
single `libsqlite3-sys` headers and symbols; no second SQLite, no extracted or downloaded
extension, no global auto-registration. `native-allocation.h` (declarations only)
redirects the C allocation family into the audited Rust allocation domain.

## Roles and exits

`keeper-graph-query build <staging>` reads a bounded accepted-event export on stdin and
replays it in one checked host transaction (MEMORY journal, EXCLUSIVE lock, no WAL).
`query <generation>` and `inspect <generation>` open the immutable generation
(`mode=ro&immutable=1`, `query_only`, default-deny authorizer) and answer one framed request.
The worker refuses any inherited environment except `GQL_STMT_CACHE=0`, closes inherited
descriptors, then confines itself (Linux: Landlock ABI 6 hard requirement, seccomp,
`RLIMIT_AS` 512 MiB; macOS: the parent's constant `sandbox-exec` profile).

| Exit | Meaning |
|---|---|
| 72 | 256 MiB charged allocation budget exhausted |
| 73 | callback/thread/registration precondition violated |
| 74 | initialization, confinement or checked-state failure |
| 75 | result row/byte cap (1,000 rows / 1 MiB) |

The unsafe inventory (U01–U11) is in `docs/constraints-and-limitations.md`.

## Proof

```sh
cargo build --release --manifest-path src-tauri/Cargo.toml -p keeper-graph-query --all-targets
cargo build --release --manifest-path src-tauri/Cargo.toml -p keeper-graph-driver
python3 scripts/graph-proof.py src-tauri/target/release <new-scratch-dir> [--benchmark]
python3 scripts/check-graph-boundary.py
```

`graph-proof.py` runs the real binaries: confinement (sentinel file, loopback listener,
canonical-file write, leaked descriptors, denied sidecar create), authorizer denials,
wrong-thread and live-statement registration negatives, allocation/result/deadline/cancel
limits with reaped-PID checks, failed-publication faults, the two-client exact oracle and,
with `--benchmark`, 100k nodes / 300k relationships / 10k retained events per domain.
macOS confinement uses the deprecated, undocumented `sandbox-exec`; it is proof-only and not
an approved production mechanism.

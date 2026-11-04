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
The worker requires exactly the supervisor's fixed environment — `GQL_STMT_CACHE=0`, plus on
Linux `GLIBC_TUNABLES=glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0:glibc.malloc.trim_threshold=536870912`
(no mmapped chunks, no tcache, no heap trim under the 512 MiB `RLIMIT_AS`;
`wire::LINUX_WORKER_GLIBC_TUNABLES`) — and refuses anything else, closes inherited
descriptors, then confines itself (Linux: Landlock ABI 6 hard requirement, seccomp,
`RLIMIT_AS` 512 MiB; macOS: the parent's constant `sandbox-exec` profile). On Linux it then
passes the input boundary B0 (`boundary.rs`): `VmData` from its own `/proc/self/status`,
opened before confinement, must be at most 1 MiB, or it exits 74 before reading anything.
Before every Linux spawn the supervisor checks its own inherited soft `RLIMIT_STACK`
(at most 8 MiB, finite) and that `/etc/ld.so.preload` is absent (P6c), and spawns nothing
otherwise.

| Exit | Meaning |
|---|---|
| 72 | charged allocation budget R = 28 MiB exhausted (the accounted-cost conversion of the 256 MiB real-allocation budget) |
| 73 | callback/thread/registration precondition violated |
| 74 | initialization, confinement, input-boundary (B0) or checked-state failure |
| 75 | result row/byte cap (1,000 rows / 1 MiB) |

The unsafe inventory (U01–U11) is in `docs/constraints-and-limitations.md`.

## Proof

```sh
# the worker under proof, built exactly as shipped (no proof-executables), copied aside
cargo build --profile graph-worker --manifest-path src-tauri/Cargo.toml -p keeper-graph-query --bin keeper-graph-query
mkdir -p src-tauri/target/graph-worker-feature-off && cp src-tauri/target/graph-worker/keeper-graph-query src-tauri/target/graph-worker-feature-off/
# driver, probes and examples
cargo build --profile graph-worker --manifest-path src-tauri/Cargo.toml -p keeper-graph-query -p keeper-graph-driver --features keeper-graph-query/proof-executables --bins --examples
KG1_WORKER=$PWD/src-tauri/target/graph-worker-feature-off/keeper-graph-query \
  python3 scripts/graph-proof.py src-tauri/target/graph-worker <new-scratch-dir> [--benchmark]
python3 scripts/check-graph-boundary.py
```

`graph-proof.py` runs the real binaries and records source/binary/toolchain provenance:
confinement (sentinel file, loopback listener, canonical-file read/write for both roles,
leaked descriptors, denied sidecar create), SQLITE_AUTH-only authorizer denials with
classifier controls, one-shot entry and wrong-thread / post-confinement thread
negatives, injected SQLite failures at the builder's SAVEPOINT / RELEASE / projection
write / COMMIT, allocation/result/deadline/cancel/abort limits with reaped-PID checks,
misbehaving worker executables (`keeper-graph-driver` example `fake_worker`), builder
admission and retention bounds, a real rename failure, the escaped-parameter frame
boundary, the two-client exact oracle and, with `--benchmark`, 100k nodes / 300k
relationships / 10k retained events per domain. On Linux it checks the accounted-cost
memory invariant with the measurement-only `scripts/alloc-interposer.c` preload.
`native_probe read-trace` re-derives the authorizer's read set from a corpus on stdin.
macOS confinement uses the deprecated, undocumented `sandbox-exec`; it is proof-only and not
an approved production mechanism.

The worker artifact must be built with the `graph-worker` profile (`panic = "abort"`): no
panic in the process can unwind. A worker rebuilt with plain `--release` does not meet the
proof's premises.

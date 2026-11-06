# keeper-graph-query

The separate, single-use, confined graph worker. Linux and macOS only; on every other target
the crate is empty and its build script returns before any native compilation.
Credential-bearing code (`keeper-core`, the driver, keeper-hq's agentd) never links this
crate: `keeper_core::graph` launches its executable through the supervisor.

## Native source

`vendor/graphqlite/` is MIT GraphQLite v0.12.0 at commit
`995a8b9bc25cdd86d4c2b50ff7ff87f0eb236629` (`LICENSE`, `source-sha256.json`); `generated/`
holds the Bison/Flex output with tool versions and hashes in `generated-provenance.json`. It is
compiled with `SQLITE_CORE` against the workspace's single `libsqlite3-sys`.
`native-allocation.h` (declarations only) redirects the C allocation family into the audited
Rust allocation domain.

## Roles and exits

- `build <staging>` reads one manifest frame and exactly `accepted_head` event frames
  (`keeper_graph_protocol::worker`) on stdin and replays their after-images in one checked
  host transaction (MEMORY journal, EXCLUSIVE lock, no WAL), then projects the active view.
- `query <generation>` and `inspect <generation>` open the immutable generation
  (`mode=ro&immutable=1`, `query_only`, default-deny authorizer) and answer one framed
  request.

Startup, in order: one-shot entry claim; the environment must be exactly `GQL_STMT_CACHE=0`
plus, on Linux, `GLIBC_TUNABLES=<LINUX_WORKER_GLIBC_TUNABLES>`; on Linux the execution-class
and platform self-check (`class.rs`); inherited descriptors closed; Linux `/proc/self/status`
opened; rlimits; Landlock ABI 6 (hard requirement) and seccomp; the input boundary B0
(`VmData` ≤ 1 MiB). macOS runs under the parent's constant `sandbox-exec` profile, evidence
only.

| Exit | Meaning |
|---|---|
| 72 | charged allocation budget R = 28 MiB exhausted |
| 73 | callback/thread/registration precondition violated |
| 74 | startup, confinement, input boundary (B0) or checked-state refusal; an execution-class or platform refusal also writes its fixed label |
| 75 | result row/byte cap (1,000 rows / 1 MiB) |

Build it only with `scripts/build-graph-worker.sh` (`--profile graph-worker`,
`panic = "abort"`). The unsafe inventory (U01–U11) and the runtime class are in
`docs/constraints-and-limitations.md`.

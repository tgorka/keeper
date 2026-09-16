---
status: done
baseline_revision: a8eb9c2
final_revision: 'c162a1a12a86'
---

# Story 72.8 — The bot a task runs, on the host that can run it

<intent-contract>

## Problem
The desktop inherits the absent bot-task runner, and the IPC cannot save or return the per-kind fields. A scheduled bot must never manufacture consent.

## Approach
Install a desktop-only shell runner over the existing provider, discovery, grant, context and tool-loop APIs. Read only the prompt text the engine already resolved. Compose an unattended DriveToolHost with no approver; record tool refusals without aborting the loop. Pass the eight per-kind fields through the existing task write door and the two copy bounds through copy_start.

## Always
Use live grant checks at each effect, existing audit ordering, and the core tool loop. Preserve the prompt remainder byte-for-byte as explicitly untrusted file data. Require a named model.

## Block If
No bot/provider/model or credentials can be resolved; return a failed record with the reason. Missing required task fields are refused by keeper-sync's write door, not a duplicated IPC rule.

## Never
Create a chat session, consult voice state, attach an attended channel/approver, widen or write grants, resolve another prompt path, invent model fallback, or change the attended path.

## I/O and edge-case matrix
| Input | Observable output / test |
|---|---|
| Frontmatter + leading ATX title + body | Only frontmatter/title removed; remaining bytes including CRLF and spacing preserved |
| No heading, inline tag, empty file, title-only, second heading | Body unchanged except one actual leading title; second heading retained |
| Task model missing or blank | Failed record names the model setting; no provider request |
| Profile-wide write grant and no approver | Ask closes audit Refused, model receives tool error, next round answers; warning names call |
| Live read/subtree-write grant | Existing DriveToolHost policy remains authoritative |
| Bot/copy request fields edited | Request fields reach TaskRow; TaskVm returns stored fields |
| One-time copy date bounds | Both optional bounds reach CopyOptions unchanged |

</intent-contract>

## Code Map
- `src-tauri/crates/keeper/src/sync.rs:51`: platform implementation; desktop bot runner door.
- `src-tauri/crates/keeper/src/bots_ipc.rs:1122`: attended assembly reference (unchanged).
- `src-tauri/crates/keeper/src/bots_tools.rs:85`: host with optional approver and audited live checks.
- `src-tauri/crates/keeper-core/src/bots/tools.rs:1175`: shared tool loop.
- `src-tauri/crates/keeper-core/src/notes/naming.rs:184`: shared ATX rule.
- `src-tauri/crates/keeper/src/sync_ipc.rs:1890,2419`: task projection/save.
- `src-tauri/crates/keeper/src/copy_ipc.rs:242`: one-time copy entry.

## Tasks & Acceptance
**Acceptance:** a `keeper` test (macOS CI — this crate does not link on the Linux dev host) proves the runner strips frontmatter and the first heading and passes the remaining text verbatim; a test proves a turn built for a task has no approver and that an `Ask` verdict becomes a refusal recorded in the record's warnings while the run continues; a test proves a task naming no model is refused with the sentence; `bots_ipc`'s attended path is unchanged (its tests stay green).

## Design Notes
The CLI's promised conversation-last-model/provider-default fallback remains deliberately unimplemented: no implementation exists to reuse, and absence is a refusal. The engine already contains the prompt path through browse::resolve; no shell path reader is introduced. Missing bot/prompt/copy-path validation stays at keeper-sync::db::upsert_task. The tool-loop API requires a cancellation signal: the task receives an inert private signal, never an attended cancel handle or cancellation registry. Audit correlation uses a fresh task-run identifier, not a persisted chat session. Platform-only assembly avoids introducing AppState or an app-handle lifecycle into the engine port.

`run_tool_loop_reporting` is the existing loop's reporting entry, also used by the attended `bots_ipc` path. Its per-call callback accumulates warnings immediately, so a later transport failure cannot erase an earlier refused call. The task record keeps every round's answer, cumulative reported usage, model changes, truncation and round-limit warnings, and transport failures. The shell's newly exhaustive copy projection maps `CopyOutcome::Skipped` to `skipped` and preserves its reason.

## Verification
Shell crate **inspection only**, not compiled or tested on this Linux host; awaits macOS CI. No formatter, linter, git command or repository-wide gate was run by this slice.

### Executed here
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core notes::prompt::tests`: initial **4 passed**.
- The same command with only the heading-removal branch mutated to retain the title: exit **101**, **2 passed / 2 failed**. `removes_metadata_and_only_first_heading_preserving_remainder` returned `\"# Title\\r\\n\\r\\n  café 🦀...\"` instead of `\"\\r\\n  café 🦀...\"`; `heading_only_and_empty_heading_have_no_body` returned `\"# Title\"` instead of `\"\"`.
- Restored the source exactly: SHA256 before and after `8b5c587bd84bbe96a7596152f9fc4b149645dd9d48bfcfbbddbb5e1823201668`. Restored rerun pending below.

### Written for macOS, not run here
The three `bot_task::tests` cases exercise an actual local SSE endpoint with the production tool loop: prompt bytes on the outbound request, an unattended wide-scope write producing an audit `Ask`/`Refused` and a second completion with a tool error, and missing/blank models refused through `BotTaskRunner::run` before platform access. Grant fixtures insert their provider and bot first because those foreign keys are enforced. The Ask test also asserts unchanged grants and no output file.

Required commands on the Mac:
- `bun run check:rust:macos`
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper --lib -- bot_task::tests`
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper --lib -- bots_ipc::tests`
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper --lib -- copy_ipc::tests`

The attended `bots_ipc.rs` and its tests were not edited. Their passing status is **not claimed** from Linux.

### Caller inventory (grep, then signature/field inspection)
- `bot_task_runner`: production engine invocation, trait default, TestPlatform override; new shell override is desktop-only. Phone/daemon defaults unchanged.
- `ShellSyncPlatform::new`: engine construction, sync_platform adapter and existing shell tests; signature unchanged.
- `ShellBotTaskRunner`: new module and shell platform door only, plus its missing-model test.
- `body_after_heading`: new reader tests and the new runner only. `strip_atx_heading` remains the existing crate-visible naming helper; no signature change.
- `task_vm`: sync_tasks listing and sync_task_save readback; both receive all eight new fields through the same projection.
- `sync_task_save`, `TaskRow`, `TaskVm`, `TaskSaveReq`: one shell row constructor and one shell TaskVm constructor; no shell request literals. Prior-row preservation removed; shared write door supplies required-field refusals.
- `copy_start`: Tauri registration in lib.rs and client.ts invoke; no direct Rust callers with an arity to update. TaskForm owns the client arguments.
- `CopyOptions`: one shell initializer in copy_start, now carries both bounds.
- `CopyOutcome`: one shell exhaustive match in copy_ipc.rs plus its existing fixtures; Skipped arm added with the reason preserved.
- `run_tool_loop_reporting`: existing bots_ipc call and new task call; signature unchanged. ToolCallReporter uses chat::ToolCall, ToolHost is Send + Sync, and disjoint record fields are captured by the sink/reporter.
- Reference-only symbols checked: `Endpoint` (exported by bots, not http), `CoreError` (error module, not crate root), `Platform` required methods, `default_profile_id` (Option<String>), `ContextBundle::system_prompt` (Option<String>), `store::insert_provider`/`insert_bot`/`save_grant` and audit listing/result types. No reference signature was changed.

## Shipped in

PR #363 of stack #364 (epic 72), branch `epic72/tasks`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.

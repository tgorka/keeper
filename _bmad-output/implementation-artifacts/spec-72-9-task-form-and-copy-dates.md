---
status: done
baseline_revision: a8eb9c2
final_revision: 'c162a1a12a86'
---
# Story 72.9 — A form that creates every kind, and a copy you can bound by date

<intent-contract>
## Problem
An untouched add form sends scheduled without a schedule, which Rust refuses below the visible controls. The browser harness does not mirror that refusal. Bot and local-copy kinds have no creation controls; one-time copy has no date bounds.

## Approach
Seed manual mode, put Rust's refusal first and scroll it into view. Extend the existing native selects and wrapping form rows with capability-gated bot/model choices and a lazy profile-relative prompt browser using syncBrowse entries verbatim. Add native absolute-folder pickers and optional UTC date bounds to copy tasks and the one-time copy card. Mirror save refusals in the dev shell, never in production TypeScript.

## Always
Rust owns validation and the prompt read. Paths cross IPC verbatim. botTools gates the bot kind and controls. Bound start is inclusive, end exclusive, epoch milliseconds; existing precise stored timestamps survive an untouched edit. Unknown stored picker values remain visible and preserved.

## Block If
The shared generated TaskVm/TaskSaveReq fields are missing: wait for TaskKinds, never widen or cast their types. Shell compilation is coordinator/macOS verification.

## Never
No prompt content reads, new path arithmetic, grant changes, date filtering on git sync, TypeScript save pre-validation, generated-binding edits, wave-1 restyling, or dependency changes.

## I/O + edge-case matrix
| Input/state | Observable result/test |
| --- | --- |
| Untouched add form | Mock accepts manual task and onSaved receives stored row |
| Scheduled with empty schedule | Rust's exact refusal above controls, scrolled into view; no onSaved |
| botTools false, including an existing bot row | Bot option and bot/prompt/model controls absent; no drive/model reads |
| Bot + profile + nested .md + model | Selected ids and Rust-served relativePath saved, no file-content read |
| Empty/loading/failed bot/model/browser lists | Explicit state; stored selection retained; no stale answer from old scope |
| Copy with neither/one path | Save reaches refusing backend, visible refusal, no accepted row |
| Copy with native paths and date bounds | Verbatim paths, replace choice, UTC epoch bounds saved |
| Picker cancelled | Prior path retained |
| One-time copy date bounds | copyStart receives inclusive-from/exclusive-to epoch milliseconds |
| Existing precise millisecond bounds | Untouched edit preserves precise bounds |
</intent-contract>

## Code Map
- `src/components/sync/task-form.tsx:512`: add/edit seed; `:754`: submit; `:1074`: current refusal.
- `src/lib/stores/sync.ts:89`: literal TASK_KINDS consumed by Rust vocabulary guard.
- `src/lib/ipc/client.ts:4125`: copyStart; `:6654`: syncTaskSave uses generated request.
- `src/lib/stores/copy-job.ts:205`: copy job start boundary.
- `src/components/layout/sync-pane.tsx:1758`: CopyCard and native-picker idiom.
- `dev/mock-shell.ts:3423`: save mock and existing refusal shape.
- `src/components/sync/task-form.test.tsx:179`: creation scenarios.
- `src/components/layout/sync-pane.test.tsx:1800`: copy-card scenarios.

## Tasks & Acceptance
**Acceptance:** a test proves an untouched add form submits and is **accepted** (mock mirrors Rust), and that switching Mode to `scheduled` with an empty schedule shows Rust's sentence above the controls; a test proves the bot fields are absent (not disabled) when `botTools` is false; a test proves a copy task cannot be saved without both paths and that the date inputs reach `syncTaskSave` as epoch ms; a test proves the copy card's date bounds reach `copyStart`; measured in `dev/probe`: a task of each offered kind created from the form.

## Design Notes
Keep the existing 224px control measure and wrapping rows. New kind-specific controls sit together after folder scope, not mixed into the schedule controls. Paths wrap so a long native path stays readable. Lazy browse lists are height-bounded and scroll independently; folder navigation uses only returned paths and a root action, never a composed parent. Inline muted help states UTC midnight and both interval edges. Errors use existing destructive foreground at the top of the form, not a second validation language.

## Verification
Pending targeted component suites and manual-seed mutation proof. The dev/probe pass over a created task of each offered kind is the coordinator's: run the repository Vite probe with `bunx vite --host 0.0.0.0` and open `/dev/probe/` in the coordinator's real-browser/CDP harness; extend/drive its task-create scenario for sync, release, verify, gc, bot and copy. Exact coordinator invocation to be confirmed against dev/probe entry before handoff. No whole-repository gates or formatters run by this slice.

## Shipped in

PR #363 of stack #364 (epic 72), branch `epic72/tasks`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.

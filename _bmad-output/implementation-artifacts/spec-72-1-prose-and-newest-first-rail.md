---
title: 'The rail opens on everything, leads with the newest, and its preview reads like prose'
status: in-progress
baseline_revision: a8eb9c2
final_revision: ''
---

<intent-contract>

## Problem
The rail has no All notes row, unordered spaces are alphabetical, snippets expose markdown, and row shading excludes actions.

## Approach
Compose All notes before sorted file-backed spaces and Uncategorized after them. Preserve keeper.order then compare cached modified dates descending and names. Strip preview markup in a pure core module. Render hints and shade the complete row.

## Always
Reuse ALL_NOTES_SCOPE and existing date resolution, retain explicit ordering, budget snippets by Unicode chars, keep selection and action targets separate.

## Block If
A change requires unrelated ownership or another query matcher.

## Never
No filesystem walk for dates, frontend markdown renderer, synthetic edit/delete controls, or shell compilation claim on Linux.

## I/O & Edge-Case Matrix
| Input | Output |
|---|---|
| Positioned and unpositioned spaces | Explicit order primary; equal positions newest first, absent dates last, names settle equal dates |
| Any space collection | All notes first, Uncategorized last |
| Markdown headings, emphasis, code, links/images, lists, quotes | Plain whitespace-folded prose |
| Multibyte text / malformed markdown | Char-safe bounded output, never panic |
| All notes clicked from space/folder | ALL_NOTES_SCOPE, no synthetic edit/delete |
| Selected space with actions | One shaded parent containing selection and all actions |

</intent-contract>

## Code Map
- `src-tauri/crates/keeper/src/notes_ipc.rs:1486`: space projection and synthetic composition; `:792`: synthetic identity.
- `src-tauri/crates/keeper-core/src/notes/sort.rs:120`: rail comparator.
- `src-tauri/crates/keeper-core/src/notes/vm.rs:284`: NoteSpaceVm.
- `src-tauri/crates/keeper/src/notes_vault.rs:1552`: actual snippet construction (Main approved ownership extension).
- `src/components/notes/space-list.tsx:208`: row render and selection.
- `src/lib/stores/notes-filters.ts:73`: existing all-notes scope, reused unchanged.

## Tasks & Acceptance
- [x] Compose synthetic rows and cached-date ordering; regenerate bindings.
- [x] Extract prose snippets and cover malformed/Unicode boundaries.
- [x] Render full-row shading, All notes selection, and HoverHint.
- [x] Run scoped Rust and frontend gates; record host limitations.

**Acceptance:** a Rust test proves `notes_spaces` returns `All notes` first and `Uncategorized` last with positioned spaces between them in their own order; a test proves two unpositioned spaces sort newest-first and that a positioned space still outranks both; a snippet test drives headings, `**bold**`, `` `code` ``, links, images, bullets and blockquotes to prose and counts `char`s not UTF-16 units; `space-list.test.tsx` proves the `All notes` row selects `ALL_NOTES_SCOPE`, carries no edit/delete controls, and that one element carries the row's background with the action buttons inside it.

## Design Notes
- `updated_ms` resolves through the existing modified-date accessor; absent dates follow known dates and names settle ties. Session spaces have no date and pass `None`, retaining their position/name order.
- All notes selects `ALL_NOTES_SCOPE`; it has no edit/delete and no per-space create control (passing its synthetic id into the file-backed creation lens would fail). The global New note action remains available. The hint wraps the selection button, not an unfocusable inner span, so the full name/query is reachable by keyboard.
- Warnings replace query detail in the hint; parse errors take precedence. In agreement with Hints, the four owned icon-only actions also use IconHint, and native titles were removed.
- The snippet implementation streams bounded prose, retains code contents while stripping fences, preserves escaped punctuation and malformed incomplete syntax, and never slices using character counts. ATX extraction is shared with naming; frontmatter uses the existing parser. No renderer dependency.
- **Cache cutover is required:** `INDEX_SCHEMA` changes 4→5. Every existing vault pays one cold rebuild on its first launch after this ships. Without it unchanged files reuse raw-markdown snippets from `.keeper/index.json`, so both the row and its new hint would remain wrong indefinitely. This discards only advisory derived data, never note files.
- **Cold rebuild cost:** one existing directory walk/stat pass plus bounded read/parse of every included Markdown note (up to eight concurrent jobs), rather than warm adoption. Epic 35's NFR-28 target is under 5 seconds for 10,000 notes, not a measurement of this patch. The 2026-09-08 field evidence records neuradrive at 775 tracked files and tgdrive at 155,626 on USB APFS; these are whole-tree counts, not Markdown-note counts. [INFERENCE] At that target's linear rate an all-Markdown 775-file tree is sub-second parsing and an all-Markdown 155,626-file tree is roughly 78 seconds, before contention/tree-walk overhead. Thus the owner's larger tree can plausibly pay tens of seconds to minutes once; do not promise the synthetic 5-second target on it. Actual post-change cold times on those vaults remain a macOS measurement, not claimed here.
- Main approved out-of-list edits to notes_vault.rs, sessions/spaces.rs, uncategorized.ts, and unowned NoteSpaceVm fixtures/mock data. Main owns the excluded notes-pane.test.tsx fixture update. notes-filters.ts is intentionally unchanged: its existing all scope and toggling already provide the needed behavior.

## Verification
- `bunx vitest run src/components/notes/space-list.test.tsx`: 26 passed.
- `bunx vitest run src/components/notes/space-list.test.tsx src/components/notes/space-editor.test.tsx src/components/notes/rail-fold.test.tsx`: 89 passed, three files (baseline and final after restoration).
- `bunx vitest run src/components/notes/space-list.test.tsx -t 'selects All notes|shades the whole row'`: deliberate mutation run, two failures: returned `kind: space` rather than `all`, and missing parent `hover:bg-accent/50`. Restored exactly to original snapshot hash, final 89/89 green.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core notes`: initial attempt timed out at 300 seconds after build-lock contention and compilation saw an assertion temporarily inserted outside its test; insertion corrected immediately. Successful subsequent gates are recorded below.
- Shell caller sweep over `crates/keeper/src` found three NoteSpaceVm constructors (file-backed, All notes, Uncategorized), one rail comparator call, and one snippet call site plus its helper, all updated. notes_ipc.rs and notes_vault.rs are inspection-only on Linux; the shell composition regression test awaits macOS CI.
- Browser measurement of full-row shading/hint interaction is coordinator-owned; jsdom proves interactions and background ownership, not rendered geometry.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core`: first complete run regenerated bindings with 2687 unit tests passing, one ignored and one failure in the concurrent NoteNav palette slice; NoteNav fixed its capped-empty-search assertion. Second complete run **3012 passed across 29 suites, one ignored**.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core notes`: initial rerun also selected the sibling palette test before its fix (657 passed, one failed). The tighter `notes::` filter then passed **643 tests across 28 suites**, 2370 filtered.
- Cargo test was used instead of nextest at Main's direction: cargo-nextest is absent on this host. Main owns the requested nextest gate where installed.
- Generated `src/lib/ipc/gen/NoteSpaceVm.ts` carries `updatedMs: number | null`; generated `NoteRowVm.ts` carries the prose field documentation. No hand edits to generated bindings.
- Additional shell regression `raw_markdown_snippet_caches_are_rebuilt_on_upgrade` supplies a real schema-4 IndexCache containing raw markdown to the cache adopter; it must refuse it. Like the composition test, it awaits macOS execution.
- Rust mutation command: `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core notes::`. Reversing the modified-date comparison and bypassing inline prose extraction produced **639 passed, four failed**. The two rail tests observed oldest/absent-first order; the prose tests observed raw `**bold**`, backticks, destinations and escaped punctuation. Both files were restored exactly (snapshot hashes A7EF / A346), and the final identical scoped command passed **643 tests**, 2370 filtered. No mutations or throwaway artifacts remain.

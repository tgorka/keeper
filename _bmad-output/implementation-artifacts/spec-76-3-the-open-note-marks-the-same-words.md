---
status: review
baseline_revision: a925c4b
final_revision: ''
---

# Story 76.3 — The open note marks the same words

<intent-contract>

## Problem
External flashes fade and cannot represent a standing list query.

## Approach
Add an independent CodeMirror decoration field and effects. Request revision-stamped ranges after Reset, with query/note generation guards.

## Always
Use UTF-16 positions and the search-highlight token pair. Clear on query/note changes; reject old revisions and responses.

## Block If
The current buffer no longer matches the acknowledged body.

## Never
Never reuse flashExternal or change the find panel's query to paint list marks.

## I/O and edge-case matrix
| Event | Result |
| --- | --- |
| setSearchMarks after ł/emoji | Exact positions decorated |
| Two seconds elapse | Marks persist |
| clearSearchMarks | Empty decorations |
| Esc with marks | Clear marks, leave find state alone |
| Esc without marks | Return false |
| Stale rev reply | Paint nothing |
| Query cleared | Clear immediately |
| Note switch | Clear then repaint fresh hit |

</intent-contract>

## Code Map
- `src/components/notes/editor/live-preview.ts:1180-1225` — sibling external flash field.
- `src/components/notes/note-editor.tsx:529-606` — lazy editor extensions.
- `src/lib/ipc/client.ts:5277` — subscription pattern.

## Tasks & Acceptance
Acceptance, verbatim from the epic: open a hit with the query standing and the runs in the body are decorated at exactly the UTF-16 positions Rust returned (a jsdom test dispatches `setSearchMarks` with a range after a `ł` and asserts the decoration's `from`/`to` against the document — mutation-proved by re-running with byte offsets); the marks are still there 2 s later (the flash would have faded); Esc in the editor clears them and leaves the find panel's state alone; clearing the query in the bar clears them; switching to another note clears them and, if that note is a hit, paints its own; a stale reply (`rev` mismatch) paints nothing; the command is **by inspection, awaiting CI macOS**.

## Design Notes
Frozen implementation contract takes the current list query after Reset. Find state remains independent. AD-266 and research §8 establish offset conversion at the Rust boundary.

- Deliberate contract simplification: every open note receives marks while the list query stands, including palette/history/wikilink opens; this relaxes UX-DR96 “Open not originating from list query → No list-search marks” without adding an origin flag.
- Trimmed queries define mark identity; trailing whitespace neither re-requests nor revives dismissed marks, and document subscriptions alone refresh external writes.
- Escape closes Find first, simplifies a non-empty selection next, then dismisses list marks; dismissal survives index/body refresh until the query changes.

## Verification
Coordinator owns test/build gates; shell awaits CI macOS. UTF-16 regression covers the byte-offset mutation; the persistent field test opens Find with `searchKeymap` and proves Escape closes Find before dismissing list marks.

### UI proof (2026-09-19)
The five-file scoped frontend run recorded in spec 76-2 passes 61 tests, including exact decoration positions, two-second persistence, mapped edits, explicit clear, Esc/find independence, stale revisions, query clearing, and switching notes. Re-running the exact-position test with byte offsets `[6,9]` instead of UTF-16 `[3,6]` failed `expected [] to deeply equal [[3,6]]`; restored afterward. Editor refresh observes the keyed body mirror that Reset updates, so a same-body revision refresh also requests marks. Query/note/buffer generations reject late replies; Escape dismissal suppresses refresh for that query generation. Full typecheck/format/macOS gates remain coordinator-owned.

### Shell wiring (by inspection)

By inspection, awaits CI macOS; this command was not compiled or executed here.
`src-tauri/crates/keeper/src/notes_ipc.rs:3245` (`notes_note_marks`) resolves the same vault/note as `notes_open`, reads the full source via the existing containment-checked reader, computes `content_rev` over that same source, then uses `split_note` and core `marks`/`utf16_ranges` over the body only. `src-tauri/crates/keeper/src/lib.rs:1297` registers it.

Matrix awaiting macOS execution: frontmatter-only term → no body ranges; `ł`/emoji before a match → UTF-16 range; changed source → changed full-source revision; absent note → existing notes error. Core UTF-16 tests and the UI mutation defend the coordinate contract but are not a shell command smoke test. The compiling gate is **Rust (fmt, clippy, test)** (`.github/workflows/ci.yml:28-52`, `macos-latest`), plus **iOS (compile check)**.

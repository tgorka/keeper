# Epic 52 — The session behaves like the app around it

created: '2026-08-17'
source: the owner's sixth field report, filed against the epic-49/50/51 build installed on hesperia (main c873fa6). Fourteen items, each measured against the code by a read-only scout before this spine was written.
binds: FR-300…FR-313 (allocated here), AD-120, AD-111, AD-65, AD-107, DW-37

## Why this epic exists

Three of the owner's fourteen items are things a shipped story claimed. One is a
refusal he has now asked to reverse twice. The rest are gaps. Triage changed the
plan in five places, and every change is written down here rather than discovered
in review.

| # | the owner's words | verdict | what it means |
|---|---|---|---|
| 1 | note tab has no edit button panel | **absent** | `FormatToolbar` has exactly two mount sites (`note-editor.tsx:1099`, `text-viewer.tsx:264`); the Note pane is built by `markdown-preview.ts`, which imports no writing tools at all — so it has no toolbar, no slash menu and no emoji completion |
| 2 | note tab need not render properties, the form is above | **broken** | on a file, the buffer IS the whole file including the YAML block, so `FileProperties` draws it as a form and `MarkdownPane` draws the same bytes as document text |
| 3 | title rename works but says the file is gone | **broken** | `sessions_file_rename` already returns the new subpath (`sessions_ipc.rs:3541`); `properties-panel.tsx:945-949` throws it away and re-reads the old address |
| 4 | md created in a space still lands in the root folder | **deliberate** | no space has a directory; a space is a tag query, and AD-120 says kind is a tag never a folder. Resolved by a carve-out, below |
| 5 | cannot scroll the space edit form | **broken** | `ui/dialog.tsx:53` caps width only and centres with `-translate-y-1/2`; a ~1220px form on a 1000px window is unreachable off both edges |
| 6 | task drag still does not work, ugly dropdown instead | **broken** + **deliberate** | `task-board.tsx:193` never calls `dataTransfer.setData`, so WebKit fires no `drop`. The dropdown is the WCAG 2.1.1/2.5.7 keyboard path (`spec-51-7:41,51`) and stays — demoted, not deleted |
| 7 | About space has no add button like others | **deliberate** | a session has one record; `spec-51-7` says never allow a second. The refusal stays, but it becomes visible instead of absent |
| 8 | sessions still use about.md not README.md | **deliberate, buildable** | `shape()` keys on `AGENTS \|\| ABOUT`. Asked twice, so built — by narrowing the predicate to `AGENTS` alone and migrating, not by adding README to the `\|\|` |
| 9 | note mode should be the default when possible | **deliberate** | `spec-51-5:62` records "never make Note the default" as a non-goal. The owner overrides his own earlier instruction; flipped |
| 10 | put spaces above files | **deliberate, cheap** | one JSX move and one test's expected order |
| 11 | right-click still does not work in session files | **absent** | story 51.6 scoped itself to *space* rows (`spec-51-6:14,82,108`); the FILES tree rows (`session-tree.tsx:328`) never had a menu |
| 12 | need an untagged space, last, when untagged notes exist | **absent** | but the query grammar already parses `-tag:` negation, so it is buildable as a default space rather than a new mechanism |
| 13 | sync threshold refuses fractional values | **broken** | the input has no `step`, so HTML's implicit `step=1` makes `1.5` a stepMismatch and blocks the native submit |
| 14 | favicon.ico at the repo root, from the svg | **absent** | and the only converter in this container is `./node_modules/.bin/tauri icon`, which already emits a 6-entry ico |

## Where the plan deviates from the report

**Item 4 — a carve-out, not a folder-per-kind.** The owner wants a file created
from a space to land somewhere other than the session root. AD-120 forbids
inferring a kind from a directory, and it stays intact: a space may name a
directory its creates go *into*, and reading still derives kind from the tag
alone. That is exactly the asymmetry `spec-50-1:57` already wrote down — "the
directory is where a create *puts* a file; the tag is what makes it that kind".
No default space names a directory, so nothing moves unless he asks for it.

**Item 7 — the refusal becomes visible.** About keeps its single-record rule.
What changes is that the space renders a create control that is *disabled and
says why*, instead of silently having none — the pattern this repo uses
everywhere else a verb is refused. He asked for a button like the others; he gets
a button like the others, and it tells him no.

**Item 8 — the safe form, and the case the push-back named.** Adding `README` to
`shape()`'s `||` flips every folder-shaped session to Flat on one rescan, so the
predicate NARROWS to `has(AGENTS)`. That deletes the documented fallback at
`shape.rs:74-75` and makes `AGENTS.md` mandatory for a flat session — including
a hand-built one that has `about.md` and no `AGENTS.md`, which the migration must
write before it moves anything, or that session silently becomes folder-shaped.
Nothing durable stores the name (`docs/sessions.md:112` — "the files are the
truth"), so only on-disk files and prose pointers move. Zone-wide, because a
continuation link crosses sessions.

**Item 6 — the dropdown survives.** Removing it re-opens DW-37 on a second
surface and strands every keyboard user. It is hover/focus-revealed instead, the
idiom `session-tree.tsx:421` already owns.

**Item 9 — his own earlier instruction, reversed on his say.** `spec-51-5` made
Preview the default deliberately. He has now asked for Note. A per-file
remembered choice still beats the default, so nothing he has already clicked
changes under him.

## Two latent defects triage found that he did not report

`sessions-pane.tsx:152-157` and `use-sessions-shortcut.ts:47-49` both compose
`…/README.md` with **no shape branch**, so on a flat session today they open a
file that does not exist and land the user on precisely item 3's red sentence.
Item 8 makes them correct by accident; they are fixed on their own merits in
story 52.1 so that nobody credits the rename for it.

## Stack order

    52.1  the record is called README            (Rust: shape, migrate, template, plan; the two latent path bugs)
    52.2  a rename the open pane follows          (properties-panel, text-file-frame, text-file-viewer, space-row-menu)
    52.3  note mode is a place you can write      (toolbar, no duplicate block, default view)
    52.4  spaces first, and one for the untagged  (section order, About's visible refusal, the Untagged space)
    52.5  a space can say where its creates land  (the AD-120 carve-out)
    52.6  a form you can reach both ends of       (the dialog scroll, both twins)
    52.7  a card you can actually drop            (dataTransfer, and the dropdown steps back)
    52.8  a file row you can right-click          (the FILES tree gets the menu it was promised)
    52.9  fractions, and an icon at the root      (the two small ones)

52.2 and 52.3 both touch `text-file-frame.tsx`; 52.4 and 52.5 both touch
`session-spaces.tsx` and `spaces.rs`; 52.5 and 52.6 both touch
`session-space-editor.tsx`. The branches are therefore strictly linear even
where the work is parallel.

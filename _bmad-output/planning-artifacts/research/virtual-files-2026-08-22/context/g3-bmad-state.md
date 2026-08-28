# G3 — BMAD conventions and current state (repo grounding)
_agent: G3BmadState · accessed: 2026-08-22 · read-only pass over this worktree_

> Scope: everything needed to author a new epic + stories for the virtual-files capability. Every claim carries `path:line`.

---

## 1. `_bmad-output/implementation-artifacts/sprint-status.yaml`

1023 lines. The file opens with a **commented header + legend**, then repeats the same six keys as real YAML, then one flat `development_status:` map.

### 1.1 Header + legend, verbatim (`sprint-status.yaml:1-46`)

```yaml
# generated: 2026-07-03
# last_updated: 2026-08-08
# project: keeper
# project_key: NOKEY
# tracking_system: file-system
# story_location: _bmad-output/implementation-artifacts

# STATUS DEFINITIONS:
# ==================
# Epic Status:
#   - backlog: Epic not yet started
#   - in-progress: Epic actively being worked on
#   - done: All stories in epic completed
#
# Epic Status Transitions:
#   - backlog -> in-progress: Automatically when first story is created (via create-story)
#   - in-progress -> done: Manually when all stories reach 'done' status
#
# Story Status:
#   - backlog: Story only exists in epic file
#   - ready-for-dev: Story file created in stories folder
#   - in-progress: Developer actively working on implementation
#   - review: Ready for code review (via Dev's code-review workflow)
#   - done: Story completed
#
# Retrospective Status:
#   - optional: Can be completed but not required
#   - done: Retrospective has been completed
#
# Action Item Status:
#   - open: Committed during a retrospective, not yet addressed
#   - in-progress: Actively being worked on
#   - done: Completed
#
# WORKFLOW NOTES:
# ===============
# - Epic transitions to 'in-progress' automatically when first story is created
# - Stories can be worked in parallel if team capacity allows
# - Developer typically creates next story after previous one is 'done' to incorporate learnings
# - Dev moves story to 'review', then runs code-review (fresh context, different LLM recommended)
# - Retrospective appends its action items to action_items; sprint-status surfaces open ones
```

Immediately below (`:48-55`) the same keys appear **uncommented and load-bearing**:

```yaml
generated: 2026-07-03
last_updated: 2026-08-08
project: keeper
project_key: NOKEY
tracking_system: file-system
story_location: _bmad-output/implementation-artifacts

development_status:
```

### 1.2 Schema of `development_status:`

One flat map, no nesting. Three key shapes, in this order per epic:

| key shape | example | allowed values |
|---|---|---|
| `epic-<n>` | `epic-54: in-progress` (`:790`) | `backlog` \| `in-progress` \| `done` |
| `<epic>-<story>-<kebab-slug>` | `54-1-a-card-that-follows-the-finger: review` (`:791`) | `backlog` \| `ready-for-dev` \| `in-progress` \| `review` \| `done` — **plus `blocked`, used in practice**: `51-8-the-record-is-called-readme: blocked` (`:691`) |
| `epic-<n>-retrospective` | `epic-54-retrospective: optional` (`:793`) | `optional` \| `done` |

**Story keys carry no `epic-` and no `story-` prefix**: `<epicNumber>-<storyNumber>-<kebab-title>` — exactly the spec filename minus the `spec-` prefix and `.md` suffix.

### 1.3 How a new epic is registered — the epic-54 block (`:760-793`)

```yaml
  # -- EPIC 54, 2026-08-17: two regressions from epic 53's own fixes ----------
  #
  # … (~28 lines of prose: what the report said, what each item turned out to
  #  be, and why the suite could not see it)
  # these shipped green.
  epic-54: in-progress
  54-1-a-card-that-follows-the-finger: review
  54-2-properties-you-can-see-and-never-in-your-prose: review
  epic-54-retrospective: optional
```

House convention on every recent epic (50–54): a **banner comment** `# -- EPIC <n>, <date>: <one-line theme> ---`, then a multi-paragraph prose block recording the epic's provenance and what the scouts found, then the three key shapes. Long-form audit narratives (epic 34's two review passes, the 31-5 three-machine run) are appended as free comments after the map (`:795-1023`).

### 1.4 Highest epic number in use — and the trap

- **Highest registered in `sprint-status.yaml`: `epic-54`** (`:790`).
- **Highest epic file in planning-artifacts: `epic-54-the-card-that-follows-and-the-form-that-stays.md`.**
- **But epic 55 is already consumed informally.** Five stories are cited as `Story 55.1` … `Story 55.5` in shipped source, with **no epic file and no sprint-status entries**:
  - `src/components/layout/surface-column.tsx:366`, `src/components/layout/files-pane.tsx:197`, `src/components/layout/chat-list-pane.tsx:64` — "Story 55.1"
  - `src/components/notes/editor/find-panel.tsx:2` — "Story 55.2, FR-267"
  - `src/components/notes/editor/markdown-marks.ts:2`, `live-preview-marks.test.ts:77` — "Story 55.3"
  - `src/components/notes/editor/live-preview.ts:533`, `media-element.test.ts:2`, `src/components/notes/attachments-panel.tsx:311` — "Story 55.4"
  - `src/components/notes/editor/file-embed.ts:116` — "Story 55.5"
  Their specs are the unnumbered `spec-note-panel-never-clipped.md`, `spec-find-bar-matches-the-ui.md`, `spec-toolbar-mark-and-emoji.md`, `spec-note-embeds-media.md`, `spec-html-rendered-and-editable.md`.

**⇒ Next free epic number = 56.** Claiming 55 would collide with five shipped stories' own doc-comments.

---

## 2. House style of an epic file (epic-53, epic-54)

### 2.1 Front-matter — there is none in the YAML sense

Both files open with a `# ` title then **three loose `key: value` lines in plain markdown**, no `---` fences.

`epic-54-the-card-that-follows-and-the-form-that-stays.md:1-5`, verbatim:

```markdown
# Epic 54 — The card that follows, and the form that stays

created: '2026-08-17'
source: the owner's eighth report — two regressions against the merged epic-53 build on hesperia (main 9a95acf). Three read-only scouts measured both before this spine was written.
binds: FR-323…FR-327; AD-102 (untouched), UX-DR61 (untouched), spec-52-3's request (restored), spec-53-3 (two clauses overridden here, explicitly)
```

`epic-53-the-gesture-the-fold-and-the-folder.md:1-5`, verbatim:

```markdown
# Epic 53 — The gesture, the fold, and the folder

created: '2026-08-17'
source: the owner's seventh field report, filed against the merged epic-52 build on hesperia (main 8c8a3eb). Five items. One librarian read Tauri's own source; five read-only scouts measured the rest before this spine was written.
binds: FR-314…FR-322 (allocated here), AD-102 (overridden here, explicitly), AD-120, AD-121, DW-37
```

`binds:` is the epic's contract surface: the FR range **allocated by this epic** (marked `(allocated here)`), every AD it touches **with an explicit disposition in parentheses** (`untouched` / `overridden here, explicitly` / `narrowed`), any UX-DR, any prior spec whose clause is superseded, and any DW it closes or leans on. Epic 52 is the same shape: `binds: FR-300…FR-313 (allocated here), AD-120, AD-111, AD-65, AD-107, DW-37` (`epic-52-…md:5`).

### 2.2 Section structure

**epic-53** (five-item field report):
1. `## Why this epic exists` — prose, then a verdict table `| # | the owner's words | verdict | what it means |`
2. `## Item 4, settled from source` — one section per item that needed evidence
3. `## Item 2 overrides AD-102, and says so`
4. `## Item 3 is a repair, not a default change`
5. `## Item 5 gets a default, and a way to reach a zone that already exists`
6. `## Stack order`

**epic-54** (two-regression report):
1. `## What he said` — the owner's words as a blockquote, verbatim, in the original Polish
2. `## Verdicts` — table `| # | claim | verdict | mechanism |`
3. `## 2a and 2b are one defect`
4. `## What the drag needs, from the repo's own idioms`
5. `## The suite cannot see either regression`
6. `## Stack order`

### 2.3 Verdict vocabulary (load-bearing)

Every reported item is classified against the code *before* any story is written. Observed values: **`absent`**, **`broken`**, **`broken, in the platform`**, **`deliberate`** (often `deliberate (a knob)`), **`not a code defect`**, **`keep`**, **`folded, with a near-invisible affordance`**. A verdict is always accompanied by a `path:line` mechanism.

### 2.4 Where acceptance criteria live

The epic file **carries no acceptance criteria**. It carries verdicts with mechanisms, the architectural rulings (an AD stated inline — see §6.2), explicit overrides of prior specs *with the instruction to mark them at the clause*, and the stack order. Acceptance rows live in the per-story spec (§3.1).

### 2.5 Story numbering and `## Stack order` — representative excerpt

`epic-53-…md`, closing section, verbatim:

```markdown
## Stack order

    53.1  a card you can drag with a finger        (pointer gesture, dead drop zone, DW-37)
    53.2  a chooser that closes when you are done  (tag-combobox + five surfaces)
    53.3  one title bar, and two folds             (properties, the caveat's short form, the merge)
    53.4  a space you can narrow in one press      (the ManyTerms repair)
    53.5  a create that lands where the space says  (per-kind defaults, absent-vs-empty, the template)

53.4 and 53.5 both touch `spaces.rs`, so they are ordered. Everything else is
disjoint and the branches stay linear only for review's sake.
```

Stories are `<epic>.<n>`, 1-based, indented four spaces (an indented code block), each a short lowercase title plus a parenthesised scope note. The paragraph beneath **must state which stories are ordered because they share a file** and which are disjoint — that is what drives the PR stack.

A second representative excerpt, the AD/spec-override idiom (`epic-54-…md:41-49`), verbatim:

```markdown
So this epic overrides two clauses of spec 53.3, and says why AT THE CLAUSE in that
file rather than in a footnote — `spec-53-3-one-title-bar-and-two-folds.md:69-79`
(the Never list), `:97` (acceptance row 1) and `:129-143` (the Design Notes
paragraph), each struck through with a `SUPERSEDED by story 54.2` line:
```

### 2.6 Epic file naming

`_bmad-output/planning-artifacts/epic-<n>-<kebab-title>.md`. The title is the epic's own sentence, lowercased and hyphenated, articles kept: `epic-53-the-gesture-the-fold-and-the-folder.md`, `epic-54-the-card-that-follows-and-the-form-that-stays.md`.

---

## 3. House style of a story spec

Two generations coexist in `_bmad-output/implementation-artifacts/`. **For a numbered epic, use generation A.**

### 3.1 Generation A — numbered specs (epics 42–54). Canonical.

Filename: `spec-<epic>-<story>-<kebab-title>.md`, e.g. `spec-54-2-properties-you-can-see-and-never-in-your-prose.md`.

Front-matter, verbatim (`spec-54-2-…md:1-9`) — again **loose key lines, not YAML fences**:

```markdown
# Spec 54.2 — Properties you can see, and never in your prose

story: 54.2
status: review
branch: `work/epic-54-properties-stay` (on top of `work/epic-54-card-follows`)
baseline_revision: 9a95acf
final_revision: ''
binds: FR-325, FR-326, FR-327; spec-52-3's request (restored), spec-53-3 (two clauses overridden), AD-102 (untouched)
sentinel: `MUT54-2`
```

`spec-54-1-a-card-that-follows-the-finger.md:1-9` is identical in shape: `story: 54.1`, `status: review`, `branch: \`work/epic-54-card-follows\``, `baseline_revision: 9a95acf`, `final_revision: ''`, `binds: FR-323, FR-324; WCAG 2.1.1 / 2.5.7 (untouched), DW-37`, `sentinel: \`MUT54-1\``.

**Field semantics:**
- `final_revision:` — present and **empty (`''`)** on both 54.x specs. This is the field the ledger-closing skills fill when the story's PR merges. Written as a quoted empty string at authoring time, **never omitted**.
- `baseline_revision:` — the short `main` sha the story was measured against (both 54.x share `9a95acf`, the merged epic-53 build).
- `sentinel:` — the mutation-run tag, `MUT<epic>-<story>`, referenced by the Verification tables.
- `branch:` — backticked, and when stacked it says so: `(on top of \`work/epic-54-card-follows\`)`.

**Section ordering (exact):**

1. `<intent-contract>` … `</intent-contract>` — an XML-tagged block holding the frozen, human-owned intent:
   - `**The ask, verbatim.**` — the owner's words quoted in the original language
   - a numbered mechanism analysis, `path:line` for every claim
   - `**Always**` — the invariants the story must establish
   - `**Block if**` — the cases it must refuse
   - `**Never**` — the prohibitions (this is where prior ADs are protected: *"Never paraphrase or truncate the caveat: AD-102 is not in scope here."*, `spec-54-2-…md:69`)
2. `## Code Map` — table `| where | change |`, one row per `path:line` touched; a `**Added by the code review.**` sub-table is appended after review, rows tagged **P1**/**P2**/**P3**
3. `## Tasks & Acceptance` — table `| # | acceptance |`, rows numbered from 1
4. `## Design Notes` — long-form reasoning: options lettered `(a)`/`(b)`, options **rejected with the reason they cannot work**, measured numbers, benign residues recorded rather than fixed
5. `## Verification` — `### Commands` table `| command | result |`; `### <SENTINEL> — the tests fail without the fix` mutation table `| mutation | result |`; `### The <n> rows` table mapping each acceptance row to the test that proves it
6. optionally `## Corrections after review` (`spec-54-1-…md:331`)

**Acceptance-criteria style** — one line each, user-observable, no Given/When/Then, no FR ids in the row. Verbatim (`spec-54-2-…md:100-112`):

```markdown
## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | a fresh install shows the properties grid on a savable markdown file, with no cookie and no press |
| 2 | the Note tab shows the document's prose and NOT the `---` block, on a fresh install |
| 3 | with the properties band folded BY HAND, the Note tab still shows no `---` block |
| 4 | the Source tab still shows every byte, and a save still writes the whole file — byte-compared |
| 5 | the control is recognisable as a disclosure and names itself visibly; the header's height is unchanged |
| 6 | a folded answer survives a remount and a restart |
| 7 | the caveat band's default and AD-102's short form are untouched |
| 8 | the two disarmed suites test the real default again, and `text-file-frame.test.tsx:896` is re-anchored with a comment naming this story |
| 9 | the vertical cost of defaulting open is stated from a real measurement, against the 153px story 53.3 claimed |
```

Rows 8 and 9 are the house signature: acceptance routinely requires **a measurement stated in the spec itself** and **a named test re-anchored**, not merely a behaviour.

### 3.2 Generation B — unnumbered bmad-build specs (the epic-55 wave, 2026-08-20 onward)

Filename: `spec-<kebab-title>.md`, no numbers — `spec-note-panel-never-clipped.md`, `spec-find-bar-matches-the-ui.md`, `spec-no-sync-unit-stalls-in-silence.md`.

Real YAML front-matter, verbatim (`spec-note-panel-never-clipped.md:1-11`):

```markdown
---
title: 'The note panel is never clipped by a window that got smaller'
type: 'bugfix'
created: '2026-08-20'
status: 'done'
baseline_commit: 'e093625117b1360371afd4d5fb3c39c09966642d'
review_loop_iteration: 1
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">
```

Sections: `## Intent` (`**Problem:**` / `**Approach:**`), `## Boundaries & Constraints` (`**Always:**` / `**Never:**`), `## I/O & Edge-Case Matrix` (`| Scenario | Input / State | Expected Output / Behavior | Error Handling |`), `## Code Map` (bullets, `path -- note`), `## Tasks & Acceptance` (`**Execution:**`, a `- [x] path -- what changed` checklist), `## Design Notes`, `## Verification` (`**Commands:**` bullets).

Differences that matter: `baseline_commit` (full sha) replaces `baseline_revision`; `status` is quoted; **no `final_revision`, no `binds`, no `sentinel`**; acceptance is an execution checklist rather than a numbered acceptance table.

### 3.3 Which generation to use

A new **numbered epic** (56.x) must use **generation A**: sprint-status story keys are derived from generation-A filenames, and the ledger/PR skills read `final_revision`.

---

## 4. Prior research artifacts and the `§` contract

### 4.1 Why the numbering is load-bearing

Rust doc-comments cite research sections **by number**, so section numbers are a public API of the research document:

- `src-tauri/crates/keeper-sync/src/lfs/basic.rs:50` — ``/// `lfs.concurrenttransfers` default (research §5.10).``
- `src-tauri/crates/keeper-sync/src/lfs/basic.rs:57` — ``/// `lfs.transfer.maxretries` default (research §5.10). Bounds the per-object`` …
- `src-tauri/crates/keeper-sync/src/lfs/batch.rs:44` — ``/// `lfs.transfer.batchSize` default (research §5.10).``
- `src-tauri/crates/keeper-sync/src/lfs/batch.rs:560-561` — ``// … would never be selected (research §6.3.7).``
- `src-tauri/crates/keeper-sync/src/lfs/endpoint.rs:323` — ``// Every row of the research §5.2 table, plus the scheme and``
- `src-tauri/crates/keeper-sync/src/credential.rs:95` — ``/// over SSH (research §5.7). We cannot mint one, so re-sending anything would``

In this codebase **"research §N.M" means `research-sync-2026-07-25.md` §N.M**. A new research doc must use the same `## N.` / `### N.M` scheme, and must not renumber the existing one.

### 4.2 `research-sync-2026-07-25.md` (1843 lines) — structure

Title, then a **bullet front-matter block** (`:1-21`), verbatim opening:

```markdown
# Technical Research: keeper — Git-Based Folder Sync (gitoxide + git-LFS + file-stability)

- **Date:** 2026-07-25
- **Researcher:** BMAD technical-research pass (Claude), four parallel strands consolidated
- **Scope:** A new keeper subsystem that synchronizes a user-chosen local folder against a
  git remote (Forgejo primary), carrying large binary files through git-LFS, driven by a
  filesystem watcher, with removable-media/pendrive and offline operation as first-class
  cases. …
- **Repo grounding:** Tauri 2 workspace (`src-tauri/crates/keeper-core` platform-free
  hexagon + `keeper` shell), `unsafe_code = "deny"` workspace-wide, cargo-deny permissive-
  only license firewall (`src-tauri/deny.toml`), …
- **Method:** Source-of-truth reading at pinned commits plus **executed** experiments on
  this host. See §1 for the evidence grading that applies to every claim below.

---
```

Section map:

| § | heading | subsections |
|---|---|---|
| 1 | `## 1. Scope & method` | `1.1 What was researched`, `1.2 Evidence grading — read this before citing anything below`, `1.3 Pinned versions and commits`, `1.4 Provenance note on §2`, `1.5 What this document is for` |
| 2 | `## 2. Prior art — what we adopt and what we reject` | `2.1 Syncthing` … `2.5 Dropbox`, `2.6 Cross-product conclusion`; each ends in an explicit **ADOPT** / **REJECT** |
| 3 | ``## 3. gitoxide (`gix`) capability matrix`` | `3.1` … `3.11` |
| 4 | ``## 4. The `git` binary dependency`` | — |
| 5 | `## 5. git-LFS — the wire contract` | `5.1 Pointer file format` … ``5.11 `.gitattributes`, sparse checkout, and fetch filtering`` |
| 6 | `## 6. Forgejo / Gitea LFS specifics` | `6.1 Routes`, `6.2 Auth shapes — three accepted`, `6.3 The quirk list — these will break a naive client` |
| 7 | `## 7. Rust LFS crate evaluation` | `7.1 The crate table` … `7.4 Recommendation` |
| 8 | `## 8. File-completeness detection` | `8.1` … `8.4 Conclusion — name exclusion cannot be sufficient` |
| 9 | `## 9. The four-tier stability gate — recommended design` | `9.1` … `9.7 Summary table` |
| 10 | `## 10. keeper integration seams` | `10.1 Crate topology and dependency direction` (mermaid), `10.2 Reusable utilities — do NOT reimplement`, `10.3 What does NOT exist yet` |
| 11 | `## 11. Explicitly NOT SUPPORTED — with the required fallback` | numbered so no future story relitigates them |
| 12 | `## 12. Open questions for implementation` | — |
| 13 | `## 13. Sources (key)` | — |

**Citation style.** Not footnotes. Every claim carries an inline **evidence grade** plus an exact locator: `[SOURCE]` (read at a pinned commit, cited as `file:line`), `[EMPIRICAL]` (an experiment actually executed on this host), `[measured]` (a timing). §1.2 (`:39-40`) states the grades are *"not interchangeable and MUST NOT be softened when quoted into a story spec."* Section-level grades are declared at the top of a section, e.g. ``Section grade: `[SOURCE]`, git-lfs `main @ d72db1e5` / `config.Version = "3.7.0"`.`` (`:783`) and ``Section grade: `[EMPIRICAL]` where marked, otherwise `[SOURCE]` at commit `9a9a166f`.`` (`:398`). §1.3 pins every source (commit sha + version) in a table. §1.5 states the document's own purpose (`:95-97`), verbatim:

```markdown
A reference. Later story specs cite it by section number. It records verdicts, not
options — where a verdict is genuinely open it lives in §12, not scattered as hedging.
```

§1.4 is also worth copying: it is an explicit **provenance note** naming which parts of §2 came from which strand, and stating that one claim is *deliberately downgraded* from the way it was briefed.

### 4.3 `research-technical-2026-07-03.md` — the older, lighter shape

Same skeleton, less machinery. Front-matter bullets (`:1-6`): `- **Date:** 2026-07-03`, `- **Author:** BMAD technical-research (Claude)`, `- **Project:** …`, `- **Method:** Live web research (July 2026) — crates.io API, GitHub API, matrix.org TWIM, vendor docs/blogs. All version numbers verified against registries on 2026-07-03.`

Sections: `## 1. matrix-rust-sdk — Current State` (`### 1.1 Versions (verified on crates.io, 2026-07-03)` … `### 1.7`), `## 2. Existing Open-Source Clients Worth Studying`, `## 3. Beeper Specifics` (`3.1`…`3.5`), `## 4. mautrix Bridges Ecosystem (2026)`, `## 5. Matrix Protocol Features Needed by keeper`, `## 6. Tauri 2 (July 2026)` (`6.1`…`6.4`), `## 7. Rust-based Fast Dev Tooling (2026)`, `## 8. Recommendations` (`8.1 Recommended architecture` … `8.6 Key risks`), `## Sources`.

Citation style here is weaker: **no evidence grades**; verification dates carried inline in subsection headings; `## Sources` (`:300`+) is a bare bullet list of URLs with parenthesised scope. **The sync doc's grading model is the one to copy** — it is the one the code ended up citing.

### 4.4 Implication for the new virtual-files research doc

- Use `## N.` / `### N.M`. State up front that later specs and doc-comments cite it as **`research-virtual-files §N.M`** — a distinct prefix, because bare `research §` is already bound to `research-sync-2026-07-25.md`.
- Carry per-claim evidence grades and a pinned-versions/commits table in §1.
- Put genuinely open items in one `## Open questions` section, not scattered as hedging (§1.5's rule).
- Where a claim cannot be retrieved, mark it `UNVERIFIED` explicitly — consistent with §1.2's "grades are not interchangeable".

---

## 5. The deferred-work ledger

**Location:** `_bmad-output/implementation-artifacts/deferred-work.md` — 3473 lines, one file, **append-only**. It opens with `# Deferred Work` (`:1`) and also carries a `## Review passes recorded` section among the entries.

**Format spec:** `.claude/skills/bmad-loop-sweep/deferred-work-format.md`. It states the file *"is append-only — never rewrite or delete existing entries"*, mandates a dedupe scan before appending (add `seen-again: <date> (<context>)` to the existing entry instead of a duplicate), and instructs: *"Number entries sequentially (`DW-1`, `DW-2`, …) by scanning the file for the highest existing number."*

Canonical entry template, verbatim from that skill file:

```markdown
### DW-<seq>: <one-line title>

origin: <workflow + artifact + date, e.g. "code review of spec-3-2-digest.md, 2026-06-12">
location: <file:line or component, or "n/a" for deferred goals>
severity: <critical | high | medium | low — how much it matters if never done>
reason: <why this was deferred rather than done now, one or two sentences>
status: open
```

`severity:` is optional (older entries predate it; a missing value means "unspecified"). Sweep annotations go directly after `status:`: `resolution: <one line: what was built or why the entry was closed>` and `decision: <date> <chosen option label> — <detail>`. Closing an entry is `status: done <date>` **plus** a `resolution:` line — never a deletion.

A real recent entry (`deferred-work.md:3183-3204`, abridged):

```markdown
### DW-197: `WriteScope::file` survived AD-102 with no production caller left, and it is the pre-fork mental model sitting in the module as a trap.

origin: story 46.14, 2026-08-10 — created by the story, named by it, and deliberately not
  removed inside it
location: …
reason: AD-102 replaced `file` with `WriteScope::route` / `owner` / `classify`. Its two former
  callers are gone — … so as of this story `file` is reached only from its own tests. …
status: open
```

Note the house convention in `origin:`: it names the story **and** says whether the item was created-and-named by that story or found afterwards.

**Highest DW number: `DW-206`** — `deferred-work.md:3339`, *"A foreign `filter.lfs.process` still reaches the clone's own checkout, because `gix::clone::PrepareCheckout` hands out no mutable repository."*, `origin: found while fixing the field incident of 2026-08-13 (a global \`git lfs install\` driver failing every \`status\`), 2026-08-16`, `status: open` (`:3370`). **Next free = DW-207.**

**Three caveats for anyone appending:**

1. The tail (`:3372-3473`) holds **un-normalised flat entries** in the bmad-dev-auto shape — `- source_spec: <spec-name|none>` / `summary:` / `evidence:` / `status:` — which the orchestrator has not yet folded into `DW-nnn` form. They are unnumbered, so the next number is still 207. Several are **directly relevant to a virtual-files design**:
   - *"`.git/lfs/tmp` accumulates leaked scratch — 100 GB in 863 files over four days on one machine"* (`source_spec: spec-no-sync-unit-stalls-in-silence`), with `.git/lfs/incomplete` at 4 GB.
   - *"A profile can sit in `offline` indefinitely with no path back"* — observed for three days while `curl` answered the remote in 30 ms.
   - *"The filter-protocol deadlock itself is guarded against but not root-caused"* — `keeper lfs filter-process` blocked in `pktline::read` on stdin, parent blocked in `Client::invoke`.
   - *"Nothing detects a broken pointer-stat invariant on its own; the repair runs only when a person presses 'Recheck all files'"* (`source_spec: spec-repair-the-lfs-stat-invariant`) — a folder whose invariant broke converts its whole worktree on every status pass.
2. That last entry names `source_spec: spec-repair-the-lfs-stat-invariant`, but **no such file exists in this worktree** — it lives on another branch.
3. `DW-37` (`:278`, keyboard-accessible pin reordering) is repeatedly carried in epic `binds:` lines (52, 53, 54) as an example of an entry an epic leans on without closing.

---

## 6. Decision registries: `D-n` vs `AD-nn`

### 6.1 `docs/decisions.md` — the `D-` registry

Header, verbatim (`docs/decisions.md:1-6`):

```markdown
# Project decisions

Durable, project-level decisions — the ones that shape what keeper will and won't build,
recorded so they stay discoverable instead of living only in planning artifacts. Each entry
cross-references its source (PRD / architecture spine) so a reader can always trace a decision
back to where it was made. Entries are numbered D-1, D-2, … as they are made.
```

Entry format: `## D-<n> — <title>` then a short framing paragraph, then bullets — **each bullet ending with a parenthesised source citation** (`PRD §13.5`, `AD-29; FR-65`, `NFR-11; PRD §13.5`). Observed bullet labels on D-1: `- **Unlocks (only the paid program grants these):**`, `- **Opening trigger:**`, `- **Constraint it forces:**`, `- **Cheap-now mitigations already paid for:**`, `- **Status / owner:**`.

**Only one entry exists: `## D-1 — Paid Apple Developer Program: deferred by decision` (`:8`). Next free = D-2.** This registry is for *product-level* deferrals with a named opening trigger. It is **not** where architecture decisions go.

### 6.2 Where `AD-nn` actually live — three eras

| range | file | format |
|---|---|---|
| AD-1 … ~AD-53 | `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SPINE.md`, from `:53` | `### AD-n — <title> [ADOPTED]` + `- **Binds:**` / `- **Prevents:**` / `- **Rule:**` |
| AD-54 … AD-63 | `…/ARCHITECTURE-NOTES-PHASE5.md`, under `## Architecture decisions AD-54 … AD-63` (`:99`) | `### AD-n — <title>` + `- **Binds:**` / `- **Decision.**` / `- **Consequence.**` / `- **Rejected — …**` |
| AD-107 … AD-115 | `…/ARCHITECTURE-SESSIONS-PHASE7.md`, from `:52` | as phase-5 |
| AD-116 … AD-121 | `…/ARCHITECTURE-SESSIONS-FLAT.md`, under `## Architecture decisions AD-116 … AD-121` (`:37`) | as phase-5 |

Each phase file opens by declaring what it extends and what it does **not** renegotiate — e.g. `ARCHITECTURE-SESSIONS-PHASE7.md:22-27`: *"Extends the frozen AD-1..AD-106 with AD-107..AD-115. Nothing here renegotiates the spine or the notes companion: … and the settings-as-files posture (AD-98..AD-106) are inputs, not open questions. Where this phase touches an existing subsystem it says exactly which line of it moves."*

**AD-64 … AD-106 have no registry entry.** From the epic-44/46 era onward, ADs are **declared inline in the epic spine**: a bold thesis sentence ending in `(AD-nnn)`, followed by the argument. This is exactly where AD-98…AD-106 live — `_bmad-output/planning-artifacts/epic-46-the-file-is-the-setting.md`, whose front-matter reads `binds: FR-200–FR-221, AD-98–AD-106, UX-DR77–UX-DR84` (`:8`). Verbatim (`epic-46-…md:92-95`):

```markdown
**A write outside the vault is a different promise, made out loud** (AD-102). AD-89's three
promises — only inside a vault, one writer, an announced removal — are not scope, they are what
makes an edit reach the reconciler and a deletion recoverable. The owner has now asked to edit and
delete files that are not in any vault. We do it, and we say what it costs: …
```

The others in that file: AD-98 (`:36`), AD-99 (`:43`, with a fenced layer-order block), AD-100 (`:56`), AD-104 (`:72`), AD-101 (`:83`), AD-103 (`:111`).

**So an AD-46/AD-52-style decision written today is declared in the new epic's spine as a bold `(AD-nnn)` paragraph**, then referenced from spec `binds:` lines and enforced in a Rust module doc-comment. AD-102's rule, for instance, is quoted verbatim in `src-tauri/crates/keeper/src/files_write.rs:675-679`, restated at `vm.rs:4233-4235` and `src/lib/viewers/types.ts:276-282`, and mutation-pinned by `text-file-viewer.test.tsx:467` (per `epic-53-…md:56-66`); `spec-46-14-…md:414` records *"`src/files_write.rs` — module doc records AD-102 and what it does *not* widen"*.

**Highest AD number: AD-121** — `ARCHITECTURE-SESSIONS-FLAT.md:147`, *"A space is a file; the board and the log are views of a query"*. A repo-wide grep for `AD-122`…`AD-199` returns **no matches**. **Next free = AD-122.**

### 6.3 Adjacent counters (same allocation discipline)

- **FR:** highest is **FR-327** (`epic-54-…md:5`, `binds: FR-323…FR-327`). A repo-wide grep for `FR-328`…`FR-399` returns nothing. **Next free = FR-328.** Ranges are allocated per epic and recorded in the epic's `binds:` line with the marker `(allocated here)`.
- **DW:** next free = **207** (§5).
- **D:** next free = **2** (§6.1).
- **UX-DR:** epic-46 allocated UX-DR77–UX-DR84; epics 53/54 reference UX-DR61 as `(untouched)`.
- **Requirements inventory** lives in `_bmad-output/planning-artifacts/epics.md` under `## Requirements Inventory` → `### Functional Requirements`, one `- FR-n: <sentence>` per line, with per-phase companions `epics-notes-phase5.md`, `epics-recording-sync-phase6.md`, `epics-sessions-phase7.md`. `epics.md` carries real YAML front-matter (`stepsCompleted`, `inputDocuments`, `generated`, `updated`, `mode`, `storyCount`, `epicCount`) whose `inputDocuments` list already includes `research-sync-2026-07-25.md` — the precedent for wiring a new research doc into planning.

---

## 7. `_bmad/config.toml` — resolved output paths

The file is installer-managed. Its header (`_bmad/config.toml:1-13`) says *"Regenerated on every install — treat as read-only"* and directs durable overrides to `_bmad/custom/config.toml` (team, committed) or `_bmad/custom/config.user.toml` (personal, gitignored).

Relevant blocks, verbatim (`:14-25`):

```toml
[core]
project_name = "keeper"
document_output_language = "English"
output_folder = "_bmad-output"

[modules.bmm]
planning_artifacts = "{project-root}/_bmad-output/planning-artifacts"
implementation_artifacts = "{project-root}/_bmad-output/implementation-artifacts"
project_knowledge = "{project-root}/docs"
```

Resolved for this repo:

| variable | resolved path | holds |
|---|---|---|
| `[core].output_folder` | `_bmad-output/` | everything BMAD writes |
| `[modules.bmm].planning_artifacts` | `_bmad-output/planning-artifacts/` | **PRD** — `prds/prd-keeper-2026-07-03/prd.md` + `addendum.md`, `phase-5-notes.md`, `phase-7-sessions.md`, `review-rubric.md`; **architecture** — `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SPINE.md` + `ARCHITECTURE-NOTES-PHASE5.md` + `ARCHITECTURE-SESSIONS-PHASE7.md` + `ARCHITECTURE-SESSIONS-FLAT.md`; **epics** — `epics.md`, `epics-*-phase*.md`, `epic-<n>-*.md`; **research** — `research-*.md` and the `research/` subfolder; **UX** — `ux-designs/ux-keeper-2026-07-03/DESIGN.md`, `EXPERIENCE.md`; briefs; brainstorming |
| `[modules.bmm].implementation_artifacts` | `_bmad-output/implementation-artifacts/` | `sprint-status.yaml`, `deferred-work.md`, every `spec-*.md` |
| `[modules.bmm].project_knowledge` | `docs/` | durable docs: `sync.md` (§8 is the LFS chapter), `decisions.md`, `settings-keys.md`, `egress.md`, `constraints-and-limitations.md`, `project-context.md`, `performance.md`, `release.md`, `credentials.md`, `notes.md`, `recording.md`, `sessions.md`, `ios.md` |
| `[core].document_output_language` | `English` | all artifacts (owner quotes stay verbatim in Polish) |

`story_location` in `sprint-status.yaml` (`:53`) independently confirms `_bmad-output/implementation-artifacts`.

Other modules write elsewhere and are irrelevant here: `[modules.bmb]` → `skills/`, `[modules.gds]` → `skills/planning-artifacts` + `skills/implementation-artifacts`, `[modules.tea]` → `skills/test-artifacts/`. **Note the trap:** `[modules.gds]` shadows the same three key names with `skills/`-rooted values — read the `bmm` block.

The assigned run folder `_bmad-output/planning-artifacts/research/virtual-files-2026-08-22/` sits inside `planning_artifacts`, consistent with the existing `planning-artifacts/research/` subfolder (`design-research-keeper.md`, `ui-inventory-keeper.md`).

---

## 8. Cheat sheet for authoring the virtual-files epic

| what | value |
|---|---|
| next free epic number | **56** — 54 is the highest registered; 55 is informally consumed by five unnumbered specs and their source comments |
| next free AD | **AD-122** |
| next free DW | **DW-207** |
| next free FR | **FR-328** |
| next free D | **D-2** (`docs/decisions.md`) |
| epic file | `_bmad-output/planning-artifacts/epic-56-<kebab-title>.md` |
| story spec file | `_bmad-output/implementation-artifacts/spec-56-<n>-<kebab-title>.md` |
| sprint-status keys | `epic-56: in-progress`, `56-<n>-<kebab-title>: <status>`, `epic-56-retrospective: optional` — appended after the epic-54 block, preceded by a `# -- EPIC 56, <date>: <theme> ---` banner and a prose provenance paragraph |
| epic front-matter keys | `created:`, `source:`, `binds:` (loose lines, no `---` fences) |
| spec front-matter keys | `story:`, `status:`, `branch:`, `baseline_revision:`, `final_revision: ''`, `binds:`, `sentinel: MUT56-<n>` |
| spec sections, in order | `<intent-contract>` (The ask verbatim / Always / Block if / Never) → `## Code Map` → `## Tasks & Acceptance` → `## Design Notes` → `## Verification` |
| epic sections | title + keys → `## Why this epic exists` (or `## What he said`) → verdict table → per-item evidence sections → `## Stack order` |
| research doc numbering | `## N.` / `### N.M`, cited later as `research-virtual-files §N.M`; do **not** reuse the bare `research §` prefix (already bound to `research-sync-2026-07-25.md`) |
| new AD declaration site | inline in the epic spine as `**<thesis sentence>** (AD-122).` + argument; referenced from spec `binds:` and enforced in a Rust module doc-comment |
| durable docs to update | `docs/sync.md` §8 (the LFS chapter) for any pointer/materialization contract change; `docs/decisions.md` only for a product-level deferral |

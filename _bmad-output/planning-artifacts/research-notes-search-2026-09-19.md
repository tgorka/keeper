---
name: 'keeper'
type: research
topic: 'notes search — BM25 + vectors over chunked notes, marked matches in the list and in the open note, a service-file toggle, an icon bar'
decision: 'what a ranked, body-aware, hybrid search over a notes vault can and cannot be built from inside keeper as it is on main — given the in-memory index, the FTS5 already bundled, the egress posture that refuses a keeper-shipped model, the folding matcher shared with sessions, the CodeMirror editor, the settings machinery, and the filter bar as it ships'
status: final
created: '2026-09-19'
run_folder: _bmad-output/planning-artifacts/research/notes-search-2026-09-19
run_folder_note: 'the digests below were read as local:// artifacts of the coordinating session (G1–G5, R1–R3, coordinator-decisions); filing them under research/ is the coordinator’s step'
digests:
  - R1 — pi-knowledge internals (the reference implementation: chunking, FTS5, vectors, fusion, diversity, incremental update), read from the installed package at 0.8.1
  - R2 — Rust crates and SQLite FTS5 facts (tokenizers, the diacritic table, bm25, highlight/offsets, prefix, escaping), local ONNX runtimes and licences, Ollama/OpenAI embeddings wire, models + PL-MTEB, vector search at scale, fusion papers, chunking evidence
  - R3 — search UX prior art (rows, in-note highlight lifetime, semantic labelling, icon toolbars/WCAG/HIG/APG, chips, hidden files, latency)
context:
  - G1 — the notes index and the three search surfaces as they are (path:line)
  - G2 — the filter bar, the row, the editor, tooltips, tests, the probe harness (path:line)
  - G3 — settings keys, scopes, per-vault config, settings UI (path:line)
  - G4 — SQLite/FTS5 in production, egress and D-4, provider seams, background jobs (path:line)
  - G5 — house conventions and numbering ceilings
  - coordinator-decisions — the owner's ask verbatim, triage verdicts, AD-261…AD-268 as pinned
---

# Research — Notes search: BM25 + vectors, marked matches, a service-file toggle, an icon bar

**Evidence grades.** `[SOURCE]` = external primary source, read on 2026-09-19, cited with publisher and URL
(and the digest section it came from). `[REPO]` = read out of this worktree at `origin/main` tip `a925c4b`,
cited `path:line` as the scout recorded it (see §1.3 for the lines this pass re-verified and corrected).
`[INFERENCE]` = reasoning over cited facts, no source of its own. `[UNVERIFIED]` = looked for, not found —
never to be repeated as fact. §12 is the complete inventory of the latter and says what was tried for each.

**How to cite this document.** Sections are numbered `§N.M` and are stable; the top-level outline §1–§12 was
fixed before writing so the epic can cite it. Cite as `research-notes-search-2026-09-19.md §4.3`. This follows
the convention `research-ai-chat-2026-09-02.md` set (its `:34-37`) and that Rust doc-comments in this tree already
use.

**What this document is not.** It does not design the feature — `epic-76-*.md` and the UX lane do that. The
decisions AD-261…AD-268 were pinned by the coordinator before this pass; this document is the evidence under
them, and where the evidence pulls against a pinned choice it says so in place (§7.1, §8.5, §9.4) rather than
resolving it silently.

---

## 0. Reading guide

`[INFERENCE]` over the coordinator's pinned decisions and the sections below. Each decision, and the sections that
carry its evidence:

| Decision | What it rests on | Sections |
| --- | --- | --- |
| **AD-261** — a derived, disposable `search.db` beside the model | FTS5 is already bundled and in production; the content-owning pattern and DW-48; `.keeper/` is tier-0 excluded; AD-57's "a mismatch is a rescan" is the answer to staleness; the single-writer / read-only-reader opener | §2.1, §2.4, §2.5, §2.11, §4.1, §4.8, §11.7 |
| **AD-262** — heading-first, paragraph-buffered chunks, no overlap, byte offsets kept | pi-knowledge's `chunkMarkdown` and its constants; the contextual prefix; the body-only editor buffer the offsets index into; the overlap evidence and the tension it leaves | §2.7, §3.1, §3.6, §9 |
| **AD-263** — FTS5 bm25 over keeper-folded text; marks from keeper's own matcher | `unicode61` does not fold `ł`, keeper's `fold_str` does; `bm25()` semantics; FTS5 has no `offsets()` and `highlight()` re-tokenises; prefix and escaping rules; the AD-20 sharing constraint; no Polish stemmer of proven quality | §2.6, §3.2, §4.2–§4.7, §5.4, §5.6 |
| **AD-264** — vectors from the provider you already configured, never a model keeper ships | D-4 and D-5; no embeddings route today but `embedding` flows through discovery; the Ollama `/v1/embeddings` shape; model licences incl. Gemma terms; batching evidence; the rejected local-ONNX path | §2.10, §5.2, §5.3, §6 |
| **AD-265** — normalised convex fusion, lexical anchor, confidence floor, brute-force cosine | pi-knowledge's formula and gates; Cormack (RRF) and Bruch et al. (convex beats RRF); the pool-relative caveat; brute-force numbers at vault scale | §3.3–§3.5, §7, §8 |
| **AD-266** — UTF-16 offsets across IPC; `NoteHitVm`; `notes_note_marks`; a persistent `searchMarks` field | `Hit.span` byte ranges die at the IPC boundary today; `NoteBodyBatch` is body-only with a byte-offset precedent; `flashExternal` fades; VS Code / Obsidian lifetime facts | §2.2, §2.7, §3.7, §10.1, §10.2 |
| **AD-267** — service files as a name list in settings and a toggle in the bar | `KeySpec`/`Scope`/`Shape` machinery, the JSON-list precedent, the boolean precedent, the two homes for a setting; Obsidian / Omnisearch / VS Code / Raycast precedents; the count line | §2.9, §10.6, §11 |
| **AD-268** — the bar speaks in icons, tags first, fits 240 px | the bar as it is, `IconHint`, `aria-pressed`, the 240 px floor; WCAG 2.5.8, HIG, APG button/tooltip, Soueidan; chip conventions; Obsidian's icon-first bar; latency budgets | §2.8, §10.3–§10.5, §10.7 |

---

## 1. The question and evidence grades

### 1.1 The question

`[REPO]` coordinator-decisions §"The owner's ask". The owner's ask, verbatim (Polish, kept as-is):

> full text search (bm25) przy note list z uwzględnieniem treści notatek - nie tylko tytułu. jak w searchu będzie treść napisana to chcę żeby w liście notatek były zaznaczone (markerem - jak w search enginach) które słowa pasują - jak otworzę notatkę i to też chcę żeby dane słowa które pasują były zaznaczone.
> dodatkowo chcę żeby search był hybryd full text search i wektor z cosine similarity (lub innego similarity jeżeli trzeba) a notatki były pochunkowane (weź przykład z pi-knowledge project jak notatki są pochunkowane i jak jest hybryd fts z wektor)
> dodatkowo chcę żeby lista miała toggle do nie-wyświetlania serwisowych plików (domyślnie on) - w ustawieniach można określić jakie to serwisowe pliki - domyślnie index.md, agents.md, claude.md, log.md
> pomyśl jak w search bar używać ikon zamiast tekstu i jak zrobić ten bar bardziej użyteczny z punktu ux i ui - popatrz jak teraz tagi są wykorzystywane (to jest najistotniejsza funkcja) - aby było dobre ui - zwłaszcza przy unfold list

`[INFERENCE]` The reading this document works to: (1) the list's search must read note **bodies**, ranked by
BM25, not only titles; (2) the words that matched are **marked** in the list rows, as a search engine marks them,
and the same words are marked when the note is opened; (3) search is **hybrid** — full-text plus vectors under
cosine (or another similarity if needed) — over **chunked** notes, taking pi-knowledge as the worked example for
both the chunking and the fusion; (4) the list gets a **toggle that hides service files**, on by default, with the
list of names configurable in settings and defaulting to `index.md`, `agents.md`, `claude.md`, `log.md`; (5) the
search bar should use **icons instead of text** and be more useful as UI, taking the existing tag mechanism —
which the owner names as the most important function — as the thing to build around, especially when the list
column is unfolded. The screenshot that came with it shows the list at ~330 px: the chip row (`+ Add a tag
filter`, `Changed by agent`, `Pinned only`), the `Search this vault` field, the caption *Searching the files, not
an index*, `688 notes`, and rows with title / date / snippet / tag pills (coordinator-decisions §"Screenshot").

### 1.2 Method

`[REPO]` G1–G5 headers; `[SOURCE]` R1–R3 headers. Eight read-only digests, five over the repository and three
over the outside world, all produced on 2026-09-19. Repository facts were read at `origin/main` tip `a925c4b`.
External facts were read from primary sources on 2026-09-19: SQLite's own FTS5 documentation and C source,
rusqlite's build script, crates.io metadata (`https://crates.io/api/v1/crates/<name>`), Hugging Face model cards
and the model API, Ollama's and OpenAI's API references, the two fusion papers, the two chunking studies, and the
product documentation of the note apps and design systems named in §10. The reference implementation
pi-knowledge was read from the installed package (`/home/dev/.omp/plugins/node_modules/pi-knowledge/`, version
`0.8.1` per its `package.json:3`, MIT per `:75`, repository `https://github.com/nczz/pi-knowledge` per `:78`) —
R1 cites `dist/src/<module>.js:<line>` because the mechanisms live in unbundled per-module files, not in the 745-line
`dist/index.js` (R1 header).

| Digest | Slice | Grade |
| --- | --- | --- |
| G1 | `IndexSnapshot`/`IndexEntry`/`IndexBuilder`, the reconciler and its deltas, the three text surfaces and the one matcher, `NoteRowVm`/`NoteSearchHitVm`, the query grammar, SQLite hook points, the editor's `NoteBodyBatch` seam | `[REPO]` |
| G2 | `NoteFilterBar` DOM order and labels, `notesFiltersStore`, tag three-state chips, fold/unfold, `NoteRow` anatomy, CodeMirror 6 + `@codemirror/search` + `flashExternal`, `IconHint`, tests, `dev/probe` | `[REPO]` |
| G3 | `KeySpec`/`Scope`/`Settable`/`Shape`, the list-of-strings and boolean precedents, per-vault `NotesConfig`, the settings dialog, `docs/settings-keys.md` generation, a two-key recipe | `[REPO]` |
| G4 | rusqlite 0.37 bundled + FTS5 in production, `archive/fts.rs` vs `recordings_fts.rs`, egress and D-4/D-5, the provider layer and the absent embeddings route, `deny.toml`, background-job patterns, the tauri-free/sync-free guards | `[REPO]` |
| G5 | numbering ceilings, filenames, epic/spec/AD/research/D/DW/sprint-status anatomy | `[REPO]` |
| R1 | pi-knowledge: `chunkMarkdown`, the FTS5 DDL and query builder, e5 vectors and the sidecar file, `weightedScoreFusion`, the three-layer lexical anchor, MMR, hash-based incremental update | `[SOURCE]` |
| R2 | FTS5 tokenizers and the `fts5_unicode2.c` diacritic table, `bm25()`, `highlight()`/offsets, prefix/escaping; tantivy; fastembed/ort/candle with licences and binary targets; models and PL-MTEB; Ollama/OpenAI embeddings; sqlite-vec/usearch numbers; RRF and Bruch et al.; Chroma/Pinecone/text-splitter | `[SOURCE]` |
| R3 | rows and in-note highlight in Obsidian/Omnisearch/Bear/Apple Notes/Notion/Craft/Logseq/VS Code/Readwise; semantic labelling; WCAG/HIG/APG/Soueidan; Carbon/Spectrum/Things/Bear chips; hidden-file precedents; NN/g/Doherty/Algolia latency | `[SOURCE]` |

### 1.3 Grades, and what this pass re-verified

`[REPO]` Thirteen `path:line` citations — about one in ten of those this document leans on — were opened against
the worktree while writing. All located the claimed code. Most were exactly where the scout said (`index.rs:1-10`,
`docs/notes.md:255-257`, `search.rs:322-340`, `archive/fts.rs:106-110`, `discover.rs:384-400`,
`docs/decisions.md:116-138`, `note-filter-bar.tsx:61`, `recordings_fts.rs:21-31`, `live-preview.ts:1220-1225`,
`notes_ipc.rs:3414`). Six had moved by a few lines to a few dozen since the scouts read them, and this document
cites the verified lines for those symbols: `notes_ipc.rs` `list_order` is `:418-425` (G1 said `:414-421`),
`split_note` `:231-234` (not `:172-180`), `MAX_SEARCH_HITS` `:100` (not `:83-84`), `run_search` `:3390`,
`matches_filter` `:443`; `notes_vault.rs` `KEEPER_DIR` and its tier-0 comment are `:70-72` (G1 said `:90-92`,
which is `COALESCE_WINDOW`); `index.rs` `matches_text`'s "index-only is the point" doc is `:205-207` and the fn
`:225` (not `:216-232`), `searchable()` `:273`. Every other `[REPO]` line is as the scout recorded it and may be
off by a similar margin; none of the drift changed a claim.

`[REPO]` G5 §"Next numbers". Numbering ceilings the epic allocates against, verified by G5's repo-wide grep:
Epic **76**; FR from **FR-563**; NFR from **NFR-63**; AD from **AD-261**; DW from **DW-260**; UX-DR from
**UX-DR94**; D: `docs/decisions.md` stops at D-18 (`:1055`), epic 69 *reserved* the labels D-19/D-20
(`epic-69-…md:62`) but never wrote them and its stories 69-5/69-6 are still `backlog`
(`sprint-status.yaml:1317-1318`) — so anything this epic records in `docs/decisions.md` takes **D-21** and says
why in the entry (G5 §"Competing patterns" item 4).

---

## 2. What keeper has today

All of §2 is `[REPO]` unless marked; the digest and section follow each claim.

### 2.1 The index is a model, not a database — and says so

`[REPO]` G1 §1. The notes index is an in-memory `IndexSnapshot` of `IndexEntry` values, published copy-on-write
over a `tokio::sync::watch` channel by one reconciler task per vault; the only persistence is the advisory
`<vault>/.keeper/index.json` (`index.rs:3-18`, `:1210-1216`). The module doc records the position and the
reasoning, verbatim (`index.rs:1-10`, re-read this pass): *"a model, not a database (epic 35, story 35.4). A
personal vault tops out around ten thousand files, so the whole index is a few megabytes of strings held in
memory and rebuilt from disk on demand. That is a deliberate position: a SQLite table beside `sync.db` would buy
nothing at this size and would cost a cache-invalidation bug forever, because every external write by Obsidian,
an agent or a `git checkout` happens behind its back. What persists is only the advisory
`<vault>/.keeper/index.json` … and a cache that disagrees with disk is a rescan, never an error (AD-57)."*

`[REPO]` G1 §1. An `IndexEntry` holds id, path, title, the `(size, mtime_ns, ino)` revalidation triple
(`index.rs:74-82`), dates, tags, flattened frontmatter `fields`, links and link predicates, flags, a 240-char prose
`snippet` and `order` — **no body, no tokens, no vectors** (`index.rs:80-189`; `SNIPPET_CHARS = 240` at
`notes_vault.rs:97-98`, re-read). Updates are incremental: the reconciler is *"the single mutator of that vault's
`IndexBuilder`"* (`notes_vault.rs:18-21`), fed `Touched(paths)` from the sync watcher tap through a 150 ms
coalescer (`COALESCE_WINDOW`, `notes_vault.rs:90`) over the watcher's 500 ms debounce; steady-state cost is one
`lstat` per touched path — a path whose triple still matches is dropped before anything is read
(`notes_vault.rs:913-915`, NFR-28). Bodies over `MAX_INDEXED_BODY = 1 MiB` are indexed head-only and flagged
`oversize` (`notes_vault.rs:92-95`). The on-disk cache is `IndexCache { schema, vault_id, … }` with
`INDEX_SCHEMA: u32 = 5`; a wrong schema or a foreign `vault_id` is discard-and-rescan (`index.rs:36-50`,
`notes_vault.rs:1229-1233`). *Rebuild index* (`notes_index_rebuild`) deletes **only** `index.json`, never the
directory, because `.keeper/` also holds the trash and the folder's own `keeper.toml`/settings
(`notes_ipc.rs:1126-1130`, `notes_vault.rs:500-518`).

`[INFERENCE]` AD-261's whole argument is contained in that module doc: the thing AD-57 refused was a SQLite
table *as the model*, whose staleness would be an error; a *derived* index that is fed by the same reconciler
deltas, keyed by the same `vault_id`, and discarded on mismatch inherits AD-57's answer to staleness verbatim — a
mismatch is a rescan. What changes is only that "rescan" now also means "re-chunk and re-index the touched
paths".

### 2.2 Three text surfaces, one matcher — and the list one is index-only

`[REPO]` G1 §2. The list's text chip is **index-only**: `notesFiltersStore.text` → `noteQueryFor` →
`NoteQueryReq.text` (`notes-filters.ts:342-356`) → `notes_list` (`notes_ipc.rs:1136-1150`) → `project_list` →
`matches_filter` (`notes_ipc.rs:443`, verified) whose first line is `if !entry.matches_text(text) { return false }`
(`notes_ipc.rs:449-452`) → `IndexEntry::matches_text`, which folds the needle with `search::fold_str` and checks
`searchable()` — **title, snippet, path, tags, non-reserved frontmatter values; the body is never read**
(`index.rs:225-232`, `:273-284`, verified). Its doc says why (`index.rs:205-207`, verified): *"Index-only is the
point: this runs once per entry on every keystroke, so it may read nothing the index does not already hold.
Full-body matching is `notes_search`, which streams because it reads files."* So a body word matches ⌘⇧F but not
the list chip unless it happens to fall inside the 240-char snippet (G1 §"Coordinator asks").

`[REPO]` G1 §2. ⌘⇧F search-everywhere is a different surface on a different call path: `useNotesSearch` (150 ms
`NOTE_SEARCH_DEBOUNCE_MS`, `use-notes-search.ts:19`) → `notes_search(vault_id, req, Channel<NoteSearchBatch>)`
(`notes_ipc.rs:3365-3385`) → `run_search` (`:3390`, verified), which walks the snapshot in index order, reads each
file (`notes_vault::read_note`) and calls `search::find(&text, needle, limit - total)` (`:3407-3413`), streaming
batches of ≥ 20 hits with `yield_now()` between; `MAX_SEARCH_HITS = 200` (`:100`, verified); no ranking, no
timeout, no per-file parallelism. The third surface, a space's `text:` / bareword, is `query::eval_text` — title
first, then a memoised `body()` closure, through the same `search::find` *"so a space and the search surface can
never disagree"* (`query.rs:1262-1270`); `needs_body` is computed at parse time and AND terms are reordered
index-only-first (`query.rs:508-510`, `:1130-1142`).

`[REPO]` G1 §2, G2 §1. The caption under the field is the constant `NOTES_SEARCH_POSTURE = "Searching the files,
not an index"` (`note-filter-bar.tsx:61`, verified), rendered at `:354-356`, copied from the UX spine
(`EXPERIENCE-NOTES.md:112`), and restated for the scan in `client.ts:4711-4714` (*"a note written a millisecond
ago is matched, because there is nothing to invalidate"*). It is true of ⌘⇧F and **overstates the list filter**,
which the store's own comment calls *"a content scan, not a name match (FR-118)"* (`notes-filters.ts:193`) while
the Rust it invokes reads no content (G1 §"Competing patterns" item 1). The same UX line anticipated highlighting:
*"Typing runs the bounded parallel scan (FR-118) and tints matches with `{colors.search-highlight}` in the row
excerpts"* (`EXPERIENCE-NOTES.md:112`, G1 §6).

`[REPO]` G1 §3, §6. The byte ranges a match produces already exist and are thrown away at the IPC boundary:
`search::Hit { line: u32, span: (usize, usize), snippet: String }` carries a *"Byte range of the match in the
**original** haystack"* (`search.rs:29-37`, verified); `run_search` builds `NoteSearchHitVm { id, path, title, line,
snippet }` from it and drops `span` (`notes_ipc.rs:3414-3420`, verified; VM at `vm.rs:1072-1082`). `NoteRowVm`
(`vm.rs:78-160`; `src/lib/ipc/gen/NoteRowVm.ts:11-105`) has no match ranges and no highlight fields; its
`snippet: string` is a Rust-composed plain excerpt (`snippet::prose(body, 240)`, `snippet.rs:5-25`).

### 2.3 No ranking exists anywhere

`[REPO]` G1 §2, §6. Lists are ordered by `list_order`: pinned → `updated_ms` desc → path, *"so the order is total
and a repaint never reshuffles equals"* (`notes_ipc.rs:418-425`, verified). Spaces sort by `SortKey = Order | Name |
Created | Modified | Recorded`, `DEFAULT_SORT = modified desc` (`sort.rs:196-236`). ⌘⇧F is index order. Sessions
search states the house position on re-ranking (`sessions/search.rs:15-17`): *"Nothing re-ranks by match count or
by 'relevance': a list that reorders itself as more results stream in is a list you cannot click."* No `bm25(`,
no score, no `Relevance` sort key exists in the workspace (G1 §5; G4 §7). `[INFERENCE]` A relevance order is a
new concept for the list, and `IndexEntry.order: NoteOrder` already occupies the word "order" (`index.rs:184-193`)
— the epic's naming must avoid it; AD-266's "in search mode the list is ordered by score" is the first ranking
keeper will have, and the sessions sentence is the constraint it must not violate: the order may depend on the
query, never on arrival time.

### 2.4 On record against this feature: AD-57 and `docs/notes.md`

`[REPO]` G1 §5, verified this pass. Two places refuse what this epic builds. `index.rs:1-10` (quoted in §2.1)
records AD-57's rejection of a SQLite index for notes. `docs/notes.md:255-257` lists, under *What is not here
yet*: *"Also deliberately out of scope this phase: vault encryption, **a full-text search engine** (the bounded
parallel scan is never stale and is fast enough well past ten thousand notes), a plugin API, notes on the phone,
and publishing a note into a Matrix room."* `[INFERENCE]` The epic overturns the *search-index* half of that on
purpose and must say so in the AD and in `docs/notes.md`: the in-memory `IndexSnapshot` stays the model; a
derived, disposable search index is added beside it; the "never stale" argument is answered by the same
reconciler deltas that keep `index.json` honest (§2.1); the "fast enough" argument is answered by the fact that
the scan cannot rank and cannot read bodies on every keystroke (§2.2), which is the owner's ask.

### 2.5 FTS5 is already in production — trigram, recency-ordered, not bm25

`[REPO]` G4 §1, §7. SQLite is `rusqlite = { version = "0.37", features = ["bundled"] }` declared once
(`src-tauri/Cargo.toml:175`); `libsqlite3-sys 0.35.0` is the only SQLite in the lock (`Cargo.lock:4387-4391`) and
`matrix-sdk-sqlite 0.18.0` reuses the same `rusqlite` (`:4784-4808`) — one amalgamation in the tree. FTS5 is not
a cargo feature; it is compiled in by the bundled amalgamation and proven by production DDL:
`CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(body, content='events', content_rowid='rowid',
tokenize='trigram')` (`archive/fts.rs:106-110`, verified) inside a `BEGIN IMMEDIATE` with a one-time
`INSERT INTO events_fts(events_fts) VALUES('rebuild')` (`:97-127`); the module says a runtime failure of that
statement *"means the bundled SQLite lacks FTS5 or the trigram tokenizer — a genuine blocker surfaced as an
`ArchiveError::Sqlite`"* (`fts.rs:94-96`), i.e. it never fires on this build. JSON1 (`json_each`, `json_valid`) is
also used in production (`recordings_fts.rs:126-128`). The bundled compile flags were measured, not assumed:
`SQLITE_DEFAULT_FOREIGN_KEYS=1` is pinned by a test (`bots/store.rs:51-57`, `:134-140`).

`[REPO]` G4 §7. What the archive FTS does **not** do matters as much: there is **no `bm25()`, no `snippet()`, no
`rank`** anywhere in `keeper-core/src/archive` — ranking is `ORDER BY events.origin_ts DESC` with a `LIMIT
scan_cap = limit × 4` window (`fts.rs:224-230`), then Rust-side dedup to one hit per edit-chain root
(`:317-345`). The MATCH argument is the whole query wrapped in one pair of double quotes with embedded quotes
doubled, *"so text like `AND`/`OR`/`*` is matched literally, never parsed as an FTS operator"* (`fts.rs:191-197`).
Queries under three scalar values fall back to `LOWER(body) LIKE '%' || LOWER(?) || '%' ESCAPE '\'`
(`TRIGRAM_MIN_CHARS = 3`, `fts.rs:20-21`, `:186-208`). Search opens *"a fresh read-only connection per query — WAL
permits concurrent readers, so search never touches the writer connection"* (`fts.rs:11-13`; `db.rs:222-225`
`SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`); the writer is one serialized task with WAL and a busy timeout
(`db.rs:48-51`). `[INFERENCE]` So bm25 ranking is a *new* pattern in this crate, not a copy of an existing one;
the opener, the writer discipline and the read-only readers are the patterns to copy.

`[REPO]` G4 §7, verified this pass. Two FTS5 patterns coexist in the crate on purpose. `events_fts` is
**external-content**; `recordings_fts` deliberately is not (`recordings_fts.rs:21-28`): *"an external-content
table is bound to its base table by rowid, so a `VACUUM` — or anything else that renumbers an implicit rowid —
desynchronises the index with no error, no crash and no wrong answer until a user notices their search has
started lying. `recordings_fts` therefore owns its own copy of the indexed text and is addressed through a key
nothing can renumber"* (DW-48; explicit `INTEGER PRIMARY KEY doc_id`, `:273-296`). *"One writer, one transaction.
… Every index write happens inside the transaction that writes the row it describes"* (`:30-35`). House rules for
any new table: idempotent `CREATE … IF NOT EXISTS` on every open, WAL, busy timeout, never hold a
`rusqlite::Connection` across an `.await`, every queryable fact a scalar column and never a JSON blob (AD-139),
FKs are enforced (`bots/store.rs:76-99`, `:10-13`, `:51-57`; `archive/db.rs:8-11`).

### 2.6 The matcher and its folding — shared with sessions, and deliberately dependency-free

`[REPO]` G1 §2, verified this pass. `search::find(haystack, needle, max_hits)` streams a folded match over the
raw bytes and returns byte spans into the **original** text with 48-char snippet windows (`search.rs:44`,
`SNIPPET_CONTEXT = 48` at `:25`). Folding is hand-rolled (`search.rs:8-12`): *"keeper has no unicode normalisation
dependency and AD-55 is emphatic about not acquiring one for the notes phase, so the table below covers Latin-1
Supplement, Latin Extended-A and the combining-mark blocks — which is every accent a Latin script writer will
type — and everything else falls through to"* `char::to_lowercase`. One `FoldChars` walk backs both `fold_str`
(`pub(crate)`, `search.rs:266-269`) and `fold_cmp` so needle/title/tag folding can never drift
(`search.rs:262-300`); `ß`→`ss` expands, combining marks fold to nothing and are transparent inside a match
(`search.rs:283-380`). The Latin Extended-A table (`LATIN_EXT_A`, `search.rs:323-340`) maps **`Ł`/`ł` → `l`**
(`:332-333`), `Đ`/`đ` → `d` (`:326-327`) and `Ħ`/`ħ` → `h` (`:328-329`) — the three codepoints §4.3 shows
`unicode61` leaves alone. `find("")` matches nothing (`:44-47`); `matches_text("")` matches everything
(`index.rs:225-229`).

`[REPO]` G1 §"Coordinator asks", §"Competing patterns" item 4. The matcher is load-bearing beyond notes:
`sessions/search.rs:5-24` imports `crate::notes::search::find` and states the AD-20 reuse rationale; its ceiling is
`MAX_HITS = 500` (`:29-31`). `[INFERENCE]` This is why AD-263 computes marks in Rust with `search::find` per term
over the raw text rather than trusting FTS5: one folding rule for the whole product, the sessions surface keeps
its matcher, and the FTS column — which holds *folded* text — is only ever used to find and rank chunks, never to
produce offsets (§4.5 says why FTS5 could not give raw-text offsets anyway).

### 2.7 The editor seam: a body-only buffer, a byte-offset precedent, a fading flash and a term hook

`[REPO]` G1 §6. `notes_open(vault_id, note_id, Channel<NoteBodyBatch>)` (`notes_ipc.rs:3074-3079`) opens with
`NoteBodyBatch::Reset { rev, path, frontmatter, text, cursor }`; frontmatter block and body travel **side by side,
never mixed**, split byte-exactly by `split_note` — `Frontmatter::parse(source)` then `source.split_at(body_offset)`
(`notes_ipc.rs:231-234`, verified) — *"the two halves concatenated are the source again, byte for byte"*; *"Rust
owns the block; the editor owns the body; a save re-joins them"* (`vm.rs:594-598`). The batch already carries one
offset into the body: `cursor: Option<u32>`, *"a byte offset **into `text`**"* (`vm.rs:640-647`). `[INFERENCE]`
AD-262's rule that chunk `byte_start`/`byte_end` index the **body** (the editor's buffer), not the file, follows
from this: a range computed over the body is a range the editor can place without knowing the frontmatter's
length; AD-266's UTF-16 conversion is then a single transform in Rust over one known string.

`[REPO]` G2 §5. The editor is CodeMirror 6 behind one dynamic import (`note-editor.tsx:8-16`, `:529-549`);
live-preview decorations are the renderer (`editor/live-preview.ts:2-6`). ⌘F is `@codemirror/search`'s
`search()` with a custom panel — *"a presentation swap and nothing else"* (`editor/find-panel.tsx:422-424`;
`highlightSelectionMatches` + `searchKeymap` at `note-editor.tsx:597-606`), styled by `--search-highlight` /
`--search-highlight-foreground` (`src/index.css:179-180`, `:287-288`). Two external highlight APIs exist and
neither persists a set of ranges: (a) `flashExternal(view, from, to)` paints a fading highlight over one absolute
range for `EXTERNAL_FLASH_MS = 1200` through a StateField + `flashExternalEffect`/`clearExternalFlashEffect`
(`live-preview.ts:44-45`, `:1182-1225`; the function at `:1220-1225`, verified); (b) the find panel accepts an
external **term** through `onExternalQuery` / `setSearchQuery.of(new SearchQuery({ search }))`
(`find-panel.tsx:88-89`, `:134-146`, `:430-455`). `--mark` / `.cm-lp-mark` is reserved for `==highlight==` and is
*"deliberately NOT the search colour"* (`index.css:186-187`; `live-preview.ts:1255-1260`). ⌘F is claimed app-wide
by the notes pane but stands down when `event.defaultPrevented` (`notes-pane.tsx:335-360`). `[INFERENCE]`
AD-266's `searchMarks` StateField is a sibling of `flashExternal`'s field (range-based, persistent, cleared by
explicit effects) rather than a reuse of the find panel's term state, because (i) the terms are matched by
keeper's folder, not by CodeMirror's `SearchQuery` (which would re-match with different case/diacritic rules —
§2.6), and (ii) the find panel's state is the user's ⌘F, which must stay theirs.

### 2.8 The row and the bar as they ship

`[REPO]` G2 §1. `NoteFilterBar` (`note-filter-bar.tsx:168-359`) renders, in DOM order: a `flex flex-wrap` chip
row (`data-slot="filter-chip-bar"`, `:241`) holding the scope chip, one `TagFilterChip` per active term, the ghost
button `+ Add a tag filter` (`ADD_TAG_FILTER`, `:58`), `Changed by agent` and `Pinned only` as ghost text buttons
with `aria-pressed` (`:~299-316`), and `Save as space` (`ml-auto`, only when `savable`, `:~317-325`); then the
inline `TagCombobox` when adding (`:~327-340`); then the search row — a `Search` icon and an `<Input
type="search" placeholder="Search this vault" aria-label=same>` (`:340-354`); then the posture caption (`:356`).
The bar never inspects rows — *"Rust evaluates the terms; this component only composes them"* (`:15-17`). Esc in
the field clears `text` first, then `dropLastChip()` (pinned → agent → last tag → scope; `:232-244`,
`notes-filters.ts:276-299`). `savable = tagTerms.length > 0 || agentOnly || pinnedOnly || text.trim() !== ""`
(`:228-230`). The count line **"688 notes"** is *not* in the bar: it is `NotesPane`'s `NOTES_COUNT_SLOT`, rendered
with `countLabel(total, NOTES, { of: matched })` from Rust's `total`, never `rows.length`
(`notes-pane.tsx:637-641`, `count-label.ts:78-101`, `notes-list.ts:41-55`).

`[REPO]` G2 §2. Tags are three-state per tag (off / include / exclude), AND-intersected — *"Two chips mean
'both', never 'either'"* (`notes-filters.ts:20-23`); the wire type is `Record<tag, "include"|"exclude">` so a
contradiction is unwritable (`NoteQueryReq.ts:11-13`). `CYCLE = ["off","include","exclude"]` and
`nextTagChipState()` are exported precisely so a second surface (the tree, `tag-tree.tsx:131-149`) cannot
re-derive the state (`notes-filters.ts:127-176`). Chips carry state three ways — glyph (`aria-hidden`), colour,
accessible name — never a tooltip, and **deliberately not `aria-pressed`** (three states vs two,
`note-filter-bar.tsx:96-110`, `:124-160`). `TagCombobox` is an inline field-and-listbox, no popup
(`tag-combobox.tsx:16-22`, `:377-421`), cannot create from the bar (`:36-38`). Tag pills in a row call
`cycleTag(tag)` (`note-row.tsx:320-333`; `notes-pane.tsx:683`).

`[REPO]` G2 §4. `NoteRow` is a fixed 64 px (`h-16`) pure projection of `NoteRowVm` (`note-row.tsx:247-252`): an
unread dot; line 1 title + pin + conflict + `data-slot="note-order"` + relative date; line 2 **snippet or
provenance** — `{row.unread && row.origin !== "" ? row.origin : row.snippet}` (`:316-318`) — then up to
`VISIBLE_TAGS = 3` pill buttons and a `+n` overflow Popover (`:342-383`); the whole row wrapped in `HoverHint
label={row.title} detail={row.snippet}` (`:389-391`). `[INFERENCE]` A marked snippet lands at `:316-318` and in the
HoverHint detail at `:390`, and must handle the unread branch where line 2 is the origin sentence (G2 §"Competing
patterns" item 5).

`[REPO]` G2 §1, §3, §6. Width is flex basis + floor, not media queries: `notes-rail` 240 px / min 180,
`notes-list` 320 px / min 240 (`column-widths.ts:172-178`, `:249-276`); the only media query is the phone tier at
768 px (`use-shell-layout.ts:4`). Fold = `columnFoldStore.toggleColumn(id)` (`surface-column.tsx:301`); the folded
list column's 48 px strip offers Search (unfolds + `requestSearchFocus()` so the caret lands the same render),
"Note list" with the count as detail, and a conditional `FilterX` "Clear filters" (`notes-pane.tsx:466-504`;
`FOLD_STRIP.widthPx = 48`, `fold-strip.tsx:141`). Icon buttons follow one pattern: a lucide icon `aria-hidden`
inside a `Button` with `aria-label`, wrapped in `IconHint` whose label is the accessible name verbatim (WCAG
2.5.3, `surface-column.tsx:141-147`, `:150-171`; `note-filter-bar.tsx:145-158`); no `title` beside a tooltip
(`:128-133`); `IconHint` is `HoverHint` with a 500 ms delay (`ui/tooltip.tsx:35-83`); a sweep test fails any bare
icon control in settings (`settings/icon-hints.test.ts:58-77`, G3 §5). `dev/probe/` renders the real `App` over
`dev/mock-shell.ts` in headless Chrome at real widths and emits `PROBE key=value` lines, including shredded-text
and overflow measures (`dev/probe/main.tsx:1-56`, `:186-247`).

### 2.9 Settings machinery, in one paragraph

`[REPO]` G3 §1–§4 (detail in §11). Every settings key is one `KeySpec` row in `KEYS` (`config/keys.rs:331`),
classified by `Scope` (`UserGlobal` / `MachineLocal` / `SessionState`, `:60-79`), `Settable` (`AnyLayer` /
`MachineFileOnly` / `Never`, `:87-100`) and `Shape` (`Flag01`, `Text`, `Json`, …, `:105-133`); a coverage test scans
every `get_setting`/`set_setting` call site and fails on an unclassified key in either direction (`:1337-1370`,
`:1373`). Values are strings in the `settings` k/v table of `keeper.db`; `registry::get_setting` consults the TOML
layer stack first (`registry.rs:208-211`). Two precedents match this epic's keys exactly: a `Vec<String>` as a JSON
array in one row (`ui.recovered_sessions_acknowledged`, `Shape::Json`, default `"[]"`, `keys.rs:802-813`,
`registry.rs:670-683`) and a `"1"`/`"0"` boolean (`bots.wake_enabled`, `Flag01`, `keys.rs:395-404`). The bar's
toggles today (`agentOnly`, `pinnedOnly`) are zustand-only and do not survive a restart (`notes-filters.ts:197-198`,
G3 §4). `docs/settings-keys.md` is generated from `KEYS` and pinned by a test — never hand-edited (`keys.rs:904-960`,
`:1672-1682`).

### 2.10 Egress, D-4, and the provider seams — no embeddings route exists

`[REPO]` G4 §3, verified this pass. keeper is client-only and *discloses rather than blocks*:
`egress::compute_egress` derives the live destination set (homeservers, `api.beeper.com` iff a Beeper account, one
row per git-remote host, one per configured AI-provider host — *"A row that reads `127.0.0.1` is the list telling
you the bytes did not leave the machine"* — and the signed-update endpoint) (`egress.rs:44-46`, `:213-300`);
`docs/egress.md:1-9` is *"the canonical, diffable record of every network destination keeper contacts"*, diffed
per release (NFR-11/AD-23). **D-4** (`docs/decisions.md:116-138`): *"There is no default endpoint, no hosted
model, no keeper-operated proxy, no telemetry about what you asked, and no opt-in scaffolding for any of them,
because there is nothing to opt into"*; *"the base URL is **required, not defaulted**"*; *"each would make keeper a
party to the conversation … project infrastructure on the traffic path"*. **D-5** refuses bundled model weights on
licence grounds (`docs/decisions.md:150-156`, `:409-413`). A runtime weight download from huggingface.co would be
a new `EgressKind` (the enum has Homeserver/Beeper/GitRemote/BotProvider/Update), a new `docs/egress.md` row and a
new D-number; nothing today permits or performs one (G4 §3).

`[REPO]` G4 §4, verified this pass. The provider layer is one `reqwest::Client` per policy — fixed `User-Agent:
keeper`, 10 s connect, a 120 s *silence* bound and explicitly not a total timeout, `redirect::Policy::none()`,
a sensitive bearer header (`bots/http.rs:24-160`). Both dialects share one wire, `POST /v1/chat/completions` with
`stream: true` (`chat.rs:41-42`), built through a per-kind `const fn quirks(kind)` table instead of `if kind ==
Ollama` branches (`quirks.rs:5-8`, `:198`); URLs are built only by `Endpoint::url`, which handles the Hermes
`/p/{profile}` prefix (`bots/mod.rs:297-301`, `:496-498`). `ProviderKind` is a closed two-variant enum, Hermes and
Ollama (`bots/mod.rs:67-73`). **Embeddings route today: none** — no `/api/embed`, `/api/embeddings` or
`/v1/embeddings` string exists in the tree; the word `embedding` appears only in the discovery vocabulary
(`discover.rs:384-386`, verified: *"The vocabulary is open-ended by design — `completion`, `tools`, `insert`,
`vision`, `embedding`, `thinking`, `image`, `audio` today (R2 §4.3) — so unknown strings are kept verbatim in the
returned list and never dropped"*), and `capability_flags` lifts only `vision`/`tools`/`thinking` to tri-state
`Option<bool>` while keeping the whole list verbatim (`discover.rs:387-400`, verified). Discovery is `GET
/api/tags` for Ollama (*"carries the whole `capabilities` array for every local model in one round trip"*) and
`GET /v1/models` + `/api/model/options` for Hermes (`discover.rs:15-17`, `:460-477`); bodies bounded at 1 MiB.
`[INFERENCE]` AD-264's "offered only among models whose discovery said `embedding`" is therefore one more
tri-state read off a list keeper already holds, under the AD-151 rule for unknowns; the wire is one more path on
the client that already exists; and the refusal sentence for a provider whose embeddings support is unknown is
the honest form of the quirks table's `Support` vocabulary.

`[REPO]` G4 §5, §8. `deny.toml` allows a fixed permissive licence list (Apache-2.0, MIT, BSD-2/3, ISC, Zlib,
BSL-1.0, CC0-1.0, MPL-2.0, Unicode-3.0, OpenSSL, CDLA-Permissive-2.0, …; `deny.toml:3-19`) and hard-denies
`git2`/`libgit2-sys` (`:32-52`). No ONNX/ML crate is vendored: a grep over `Cargo.lock` for `ort|onnx|candle|
tokenizers|linfa|ndarray|safetensors|hf-hub|tract|tch` returns nothing. A new `keeper-core` dependency must pass
`check:core-tauri-free` and `check:core-sync-free` (`package.json:25-26`) and the licence firewall; the manifest's
own comments prefer deps that *"add no new package to Cargo.lock"* (`keeper-core/Cargo.toml:54-60`, `:88-97`).

### 2.11 Background-job patterns an index and a backfill should copy

`[REPO]` G4 §6. The archive writer is one task owning one `rusqlite::Connection` for the app's lifetime, work on
an unbounded `mpsc`, completions on `oneshot`, *"Any failure is logged with ids only and swallowed — the task never
dies"* (`archive/mod.rs:502-505`, `archive/ingest.rs:23-30`). The notes reconciler gives each vault an unbounded work
channel plus two `watch` channels — the snapshot and a `NoteIndexProgressVm { vault_id, scanned, total_estimate,
phase }` for cold-scan progress — surfaced through `notes_subscribe_index` (`notes_vault.rs:695-725`, `:440-448`;
`vm.rs:1090-1097`). `Rescan` is *"Discard everything and cold-scan — `notes_index_rebuild`, or a lagged watcher tap,
where a burst that outran the channel degrades to a slower correct answer rather than a lost update"*
(`notes_vault.rs:179-182`). Scheduling is *"evaluated on the ~1 Hz supervisor tick that already exists. There is no
second clock, no notes scheduler, no notes timer"* (`notes_vault.rs:29-31`, `:2734-2740`); a phone write arms
exactly one delayed pass (`:2718-2723`). One-shot jobs stream `Running` heartbeats then exactly one terminal
`Completed`/`Cancelled`/`Failed` over a Tauri `Channel` (`ipc.rs:2658-2672`). `[INFERENCE]` AD-264's backfill
"paced off the existing tick / `Rescan`, progress as a VM over a watch channel like `NoteIndexProgressVm`" is
these three patterns and no new one.

---

## 3. pi-knowledge as the reference

All of §3 is `[SOURCE]` R1 unless marked — pi-knowledge 0.8.1 (MIT), read from the installed package,
`dist/src/<module>.js:<line>`; repository <https://github.com/nczz/pi-knowledge> (`package.json:76-78`, read
2026-09-19). Five constants were re-read this pass to confirm R1: `MARKDOWN_TARGET_TOKENS = 450`
(`indexer/chunker.js:316`), `MAX_TEXT_CHUNK_CHARS = 6_000` (`:318`), the `< 50` floor (`:386`, `:407`),
`weightedScoreFusion(… { bm25: 0.45, vector: 0.55, overlap: 0.15 })` (`search/fusion.js:22`) and
`MIN_HYBRID_SCORE = 0.18` (`search/ranking.js:3`).

### 3.1 Chunking

`[SOURCE]` R1 §1. `chunkMarkdown` (`chunker.js:376-444`) splits by **ATX heading sections** — lines matching
`/^(#{1,6})\s+(.+)$/` flush the current section and push/pop a `headingStack` used as the breadcrumb (`:425-441`).
Within a section the heading line is re-prepended, the text is split on `/\n\n+/` into paragraphs, and paragraphs
are buffered greedily: the buffer flushes when adding the next paragraph would exceed `MARKDOWN_TARGET_TOKENS =
450` with `estimateTokens(text) = ceil(text.length / 3)` (≈ 1 350 chars) (`:313-318`, `:405-436`). Chunks under 50
characters are dropped (`:386`, `:410`); a file with content but no chunks falls back to one chunk if
`content.trim().length > 10` (`:519-521`); a paragraph over 6 000 chars is hard-sliced every 6 000 chars
(`pushOversizedMarkdownParagraph`, `:393-404`). **Overlap: none** — boundaries are flush points, nothing slides;
CHANGELOG 0.4.1 framed this as *"reduced overlap to avoid near-duplicate retrieval units"* (`CHANGELOG.md:174`).
**Code fences are not treated**: a fenced block counts as ordinary lines and a blank line inside a fence splits
paragraphs (can split a fence in two) (R1 §1). Plain text uses the same buffering at 550 tokens; code files use
tree-sitter AST chunking, irrelevant to notes.

`[SOURCE]` R1 §1. The crucial split is **embed text vs stored text** (`chunker.js:329-358`):
`buildContextPrefix` yields `File: <path>\nType: <type>\nSection: <breadcrumb>\n[Symbol: <fn>]` and
`buildChunkEmbeddingText(chunk)` = `${prefix}\n\n${chunk.content}`; `makeChunk` stores `content` raw for display
and `content_tokenized = preTokenizeForFTS(buildChunkEmbeddingText(chunk))` for FTS — so the prefix reaches
**both** the embedding and the lexical index while the display text stays raw. README.md:67 calls this
*"Contextual Retrieval without remote chunk rewriting"*. `preTokenizeForFTS` bakes camelCase / letter-digit /
CJK splitting into the stored column (`:319-327`). The chunk identity hash is sha256 over
`path\0type\0startLine\0endLine\0metadataJson\0content` (`:346-351`).

`[INFERENCE]` AD-262 copies the shape — heading-first, paragraph-buffered, no overlap, 50-char floor, a hard max —
and adjusts the numbers (~1 200 chars ≈ 300 tokens target, 4 000 hard max) and the prefix (`<title> › <breadcrumb>`
rather than `File:/Type:/Section:`), and keeps the offsets pi-knowledge does not (§3.7). Two things the port
should do that the reference does not: keep a fenced block whole across the paragraph split (keeper already
detects fences in `snippet::prose`, `snippet.rs:5-25` `[REPO]` G1 §1), and store byte offsets into the body
rather than line numbers (§2.7).

### 3.2 Lexical

`[SOURCE]` R1 §2. One **external-content** FTS5 table over one pre-tokenized column, default `unicode61`
tokenizer, no prefix index, no column weights: `CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING
fts5(content_tokenized, content=chunks, content_rowid=rowid)` with AFTER INSERT/DELETE/UPDATE triggers
(`storage/sqlite.js:39-113`). Because the tokenizer is dumb, all lexical intelligence lives in JS and is mirrored
at query time: `normalizedQueryText` = lowercase → symbol stripping → an 18-word stop list → a tiny suffix
stemmer (`ies→y`, `ing`, `ed`, trailing `s`) → a small typo map (`search/query.js:3-58`). The query is each term
**phrase-quoted** (`"`→`""`), joined `AND`, with an `OR` fallback when strict returns nothing and there is more
than one term (`search/bm25.js:1-8`, `:27-43`): *no phrase operators, no prefix `*`, no NEAR*. Ranking is bare
`bm25()`, negated because FTS5's better matches are numerically lower: `SELECT c.id, -bm25(chunks_fts) AS score
… WHERE chunks_fts MATCH ? AND c.kb_id = ? ORDER BY bm25(chunks_fts) LIMIT ?` (`bm25.js:11-19`); SQL errors are
swallowed to `[]` (`:41-43`).

`[SOURCE]` R1 §2. **Snippets are not FTS5 `snippet()`/`highlight()`**: `buildQuerySnippet(content, query, 240)`
does a case-insensitive `indexOf` of the longest query token over the raw `content`, pads a window with `…`, and
**emits no highlight markup** — *"the frontend would have to re-find the terms"* (`engine.js:239-256`). R1's own
warning: FTS5 snippets would operate on the *pre-tokenized* column and mangle camelCase text (R1 §"Competing
patterns").

`[INFERENCE]` AD-263 keeps the query builder (quoted words, implicit AND, OR fallback), adds the prefix `*` on the
last word for type-ahead (pi-knowledge is an agent tool, not a search-as-you-type field), drops the English stop
list / stemmer / typo map (the vault is Polish and English — §4.3, §5.4), and replaces pi-knowledge's
`preTokenizeForFTS` with keeper's own `fold_str` as the single pre-fold applied symmetrically at index and query
time — the same "keep both sides symmetric" rule R1 insists on, with keeper's function in the slot.

### 3.3 Vectors

`[SOURCE]` R1 §3. Model `Xenova/multilingual-e5-small` via Transformers.js in an isolated worker, `quantized: true`
(`model-worker.js:30-33`); E5 prefixes `query: ` / `passage: ` hard-coded per side (`provider.js:21-24`,
`model-worker.js:73`); mean pooling with `normalize: true`, so similarity is a plain dot product labelled cosine
(`search/vector.js:4-9`). Dimension is stored per knowledge base, not hard-coded; a mismatch skips vector search
with a warning and hybrid degrades to BM25-only (`engine.js:470-474`, `:1266-1280`). Document embedding never falls
back to another provider, to avoid mixed vector spaces (`provider.js:186`; `docs/configuration.md:62`). Storage
is a sidecar `vectors/<kbId>.bin` — 8-byte header (`count u32 LE`, `dim u32 LE`) then `count × dim × float32 LE`
rows whose **order equals chunk rowid order** (`embedding/vectors.js:1-93`; `sqlite.js:479-481`), rewritten via
temp + rename. Top-k is a brute-force streaming scan with a bounded insertion-sort top-k and a reusable
`Float32Array` scratch buffer (`vector.js:12-44`, `:46-76`); **no ANN** — README.md:73 disclaims heavy indexes.
Embedding batches are 64 (`INDEX_EMBED_BATCH_SIZE`, `engine.js:18`).

`[INFERENCE]` The three things AD-264/AD-265 take from this: unit-normalise once and dot, never re-normalise per
query; key vectors by **chunk + model** so a model change invalidates rather than mixes spaces; and brute force.
The one thing they leave: the sidecar file with implicit rowid ordering — R1's own "leave" list says that for
note-sized vaults a BLOB per chunk in the same SQLite file is *"simpler and transactionally consistent"*
(R1 §"What to copy / what to leave"), and §7 gives the arithmetic.

### 3.4 Fusion

`[SOURCE]` R1 §4. `weightedScoreFusion(bm25Results, vectorResults, weights = { bm25: 0.45, vector: 0.55, overlap:
0.15 })` (`search/fusion.js:22-40`, re-read): each list is min-max normalised — `(s − min) / (max − min)`,
degenerate (single-valued) lists → 1 — then `score = 0.45·bm25 + 0.55·vector + (0.15 if both > 0 else 0)`; the
overlap bonus is additive on an already ≤ 1.0 score (max 1.15). `reciprocalRankFusion` (k = 60) is exported and
**not called** anywhere in the search path (`fusion.js:13-20`); README.md:68: *"RRF … remains the baseline
reference, but weighted fusion is used by default because project dogfood showed RRF compressed scores too much
for ranking diagnostics"*.

`[SOURCE]` R1 §4. The **lexical anchor** has three layers: (1) the whole KB is skipped if BM25 returned nothing
(`engine.js:1256`); (2) `result.score >= tuning.minHybridScore` with `MIN_HYBRID_SCORE = 0.18` (`ranking.js:3`;
profiles 0.12 recall … 0.4 precision); (3) `hasEnoughLexicalEvidence` — stemmed signal tokens must cover the
chunk's embedding text: ≥ 1/N for ≤ 3 signals, ≥ 34 % beyond, waived for ≤ 1 signal or a strong path boost
(`ranking.js`). Candidate pool: `max(candidateMin = 50, offset + limit × 12)` (`engine.js:1235-1236`). Post-fusion
boosts for path/basename/readme, test-path demotion, a stale multiplier ×0.92 (`ranking.js`; `engine.js:110-116`).

`[SOURCE]` R1 §"Competing patterns". R1's caveat, which §8.4 takes up: *"Fusion score scale is not a probability:
min-max normalization makes the top bm25 and top vector result always 1.0, so `minHybridScore` thresholds are only
meaningful relative to the candidate pool of that query (a weak query can still produce a 1.0-normalized junk
top score — mitigated by the lexical-coverage gate, not by the threshold)."*

### 3.5 Diversity

`[SOURCE]` R1 §5. MMR: greedy select maximising `λ·relevance − (1−λ)·redundancy`, λ = 0.76 balanced / 0.62 strong;
redundancy = max over chosen of {token Jaccard, line proximity (overlap 1 / ≤ 20 lines 0.8 / ≤ 80 lines 0.45 /
else 0.2), cosine × 0.35} (`engine.js:176-204`, `:150-163`, `:19`). Then `interleaveByFile` round-robins across
`kb_id:file_path` buckets so one file cannot own the top page (`:206-237`). Adaptive mode expands seeds to
neighbouring chunks (±80 lines, ≤ 6 000 chars, 5 neighbours; `:257-293`); deep mode sends the top 30 to a
cross-encoder. `[INFERENCE]` For a note list the only piece that matters is the same-file collapse, and AD-265's
"best chunk per note" is the degenerate, list-shaped form of it: one row per note, scored by its best chunk. MMR,
adaptive and deep are agent-tool machinery with no notes-list payoff (R1 §"Leave").

### 3.6 Incremental

`[SOURCE]` R1 §6. Change detection is **content hash, not mtime**: every file is re-chunked and each chunk's
`content_hash` compared with the existing map (`engine.js:871-878`); a hash match keeps the chunk and **reuses its
vector** without re-embedding (`:943-953`, `:1047-1097` — vectors copied by hash from the old or the "added" file
into a rebuilt vector file); unseen ids are deleted (`:1031-1035`, `:1099-1100`); FTS stays consistent through the
triggers; a changed embedding signature rebuilds **all** vectors (`:858-886`); mtime feeds only the `stale`
provenance flag (`:99-108`, `:1435-1436`). `[INFERENCE]` keeper already has a cheaper change gate — the `(size,
mtime_ns, ino)` triple that drops an untouched path before any read (§2.1) — so the port is: triple says changed →
re-chunk → per-chunk hash decides which vectors survive. Both gates, in that order, is strictly less work than
pi-knowledge does, and the reconciler's `Touched` delta already names the paths.

### 3.7 What the reference does not give: offsets and marks

`[SOURCE]` R1 §7. pi-knowledge stores only `start_line`/`end_line` per chunk, no character offsets, no per-line
index, no highlight markup anywhere; the one query-time match position lives inside `buildQuerySnippet` and is not
returned; mapping a match to a file line is the consumer's job (`sqlite.js:39-52`; `engine.js:239-256`,
`:1419-1441`). `[INFERENCE]` This is the half of the owner's ask the reference cannot answer — marks in rows and
in the open note — and it is why AD-263/AD-266 compute ranges with `search::find` over raw text and carry them as
`marks: Vec<[u32;2]>` / `ranges: Vec<[u32;2]>` in UTF-16 units. The reference is the model for *finding and
ranking chunks*, not for *showing where*.

---

## 4. SQLite FTS5 facts

All of §4 is `[SOURCE]` R2 §1 and §8 unless marked — SQLite, <https://sqlite.org/fts5.html> (section numbers are
that page's), and the FTS5 C sources at `https://raw.githubusercontent.com/sqlite/sqlite/master/ext/fts5/`, read
2026-09-19.

### 4.1 Availability in a rusqlite `bundled` build

`[SOURCE]` R2 §1. `libsqlite3-sys/build.rs::build_bundled::main` compiles the amalgamation with
`-DSQLITE_ENABLE_FTS3`, `-DSQLITE_ENABLE_FTS3_PARENTHESIS`, **`-DSQLITE_ENABLE_FTS5`**, `-DSQLITE_ENABLE_JSON1`,
`-DSQLITE_ENABLE_LOAD_EXTENSION=1`, `-DSQLITE_ENABLE_RTREE`, `-DSQLITE_THREADSAFE=1`
(<https://raw.githubusercontent.com/rusqlite/rusqlite/master/libsqlite3-sys/build.rs>); rusqlite is MIT, `bundled
= ["libsqlite3-sys?/bundled", "modern_sqlite"]` (<https://raw.githubusercontent.com/rusqlite/rusqlite/master/Cargo.toml>).
FTS5 is *"included as part of the SQLite amalgamation"* since 3.9.0 (§2.1 of the FTS5 page). `[REPO]` G4 §1:
keeper's pin is rusqlite 0.37 / libsqlite3-sys 0.35.0 (R2 read the current 0.40.1 / 0.38.1 sources); the
production `USING fts5(… tokenize='trigram')` at `archive/fts.rs:106-110` is the proof for keeper's own build.

### 4.2 Tokenizers

`[SOURCE]` R2 §1, FTS5 §4.3, quotes verbatim. `unicode61` (default): token characters are general categories
`L* N* Co`; *"By default, diacritics are removed from all Latin script characters."* `remove_diacritics` is `"0"`,
`"1"` (default) or `"2"` — with `"1"`, *"diacritics are not removed in the fairly uncommon case where a single
unicode codepoint is used to represent a character with more that one diacritic"*; *"If this option is set to
"2", then diacritics are correctly removed from all Latin characters."* Also `categories`, `tokenchars`,
`separators`. `ascii`: ASCII case folding only, non-ASCII always token characters, no diacritic removal. `porter`:
*"applies the porter stemming algorithm … designed for use with English language terms only - using it with other
languages may or may not improve search utility"*. `trigram`: *"each contiguous sequence of three characters as a
token … a query or phrase token may match any sequence of characters within a row"*; `case_sensitive` (default 0),
`remove_diacritics` (default 0); *"Substrings consisting of fewer than 3 unicode characters do not match any rows"*.
Custom tokenizers are a C API (`fts5_tokenizer`, §7.1); rusqlite exposes no wrapper for it, so a Rust custom
tokenizer means hand-written FFI against `fts5_api` `[INFERENCE]` R2 §1 (from rusqlite's feature list; §12).

### 4.3 The Polish `ł`: what `unicode61` folds and what keeper's `fold_str` folds

`[SOURCE]` R2 §1, §8 — verified by R2 against `fts5_unicode2.c::fts5_remove_diacritic` (tables `aDia[]`/`aChar[]`,
key = `codepoint<<3 | rangeLen`, <https://raw.githubusercontent.com/sqlite/sqlite/master/ext/fts5/fts5_unicode2.c>).
The decoded ranges include U+00E0–E5 → a, U+00E7 → c, U+00E8–EB → e, U+00EC–EF → i, U+00F1 → n, U+00F2–F6 → o,
U+00F9–FC → u, U+00FD/FF → y, U+0101/0103/**0105 (ą)** → a, U+0107 (**ć**)/0109/010B/010D → c, U+010F → d,
U+0113/0115/0117/**0119 (ę)**/011B → e, U+011D–0123 → g, U+0125 → h, U+0129–0130 → i, U+0135 → j, U+0137 → k,
**U+013A/013C/013E (ĺ ļ ľ) → l — U+0142 (ł) is outside the range and is NOT folded**, U+0144 (**ń**)/0146/0148 → n,
U+014D/014F/0151 → o, U+0155/0157/0159 → r, U+015B (**ś**)/015D/015F/0161 → s, U+0163/0165 → t, U+0169–016F,
0171/0173 → u, U+0175 → w, U+0177 → y, U+017A (**ź**)/017C (**ż**)/017E → z. `đ` (U+0111) and `ħ` are also not
folded. `remove_diacritics 2` does not change this — it only fixes codepoints carrying more than one diacritic
(§4.2). **Consequence (R2):** a query `lodz` will not match `Łódź` under `unicode61` — `ó` and `ź` fold, `ł` does
not; `ł` *"must be handled in application-side normalisation (e.g. pre-fold `ł→l` in both indexed text and query,
via a custom column) or accepted as-is."*

`[REPO]` G1 §2, verified this pass. keeper's `LATIN_EXT_A` table (`search.rs:322-340`, applied by `latin_fold`
at `:342-356`) folds every codepoint in U+0100–U+017F, including **`Ł`/`ł` → `l`** (`:332-333`), `Đ`/`đ` → `d`
(`:326-327`) and `Ħ`/`ħ` → `h` (`:328-329`) — i.e. exactly the three `unicode61` misses — and `fold_str`
(`:266-269`) is the one function the list
chip, ⌘⇧F, spaces and the sessions matcher already agree on (§2.6). `[INFERENCE]` This is the fact AD-263 rests
on: index the **`fold_str`-folded** chunk text in the FTS column, fold the query with the same function, and
tokenize with `unicode61` (the folded text is ASCII-ish, so its own diacritic handling is idle and harmless). The
alternatives each fail a constraint: `unicode61` alone misses `ł` (this section); `trigram` has no word notion, so
`bm25()` would weigh 3-gram hits and a two-letter query matches nothing (§4.2; §5.6); a custom tokenizer is C FFI
(§4.2; §5.4). One consequence to state plainly: because the FTS column holds folded text — `ß`→`ss` changes byte
lengths, combining marks vanish — **no byte offset from the FTS side maps back to the raw note**; that is why
marks come from `search::find` over the raw text (§4.5 shows FTS5 could not have given offsets anyway).

### 4.4 `bm25()`

`[SOURCE]` R2 §1, FTS5 §5.1.1. `bm25()` returns **negative** scores — it *"multiplies the result by -1 …
ensuring that better matches are assigned numerically lower scores"*; `k1 = 1.2`, `b = 0.75` hard-coded; column
weights are positional — `ORDER BY bm25(email, 10.0, 5.0)` weights the first column ×10, the second ×5, the rest
×1 (*"If there are not enough arguments for all table columns, remaining columns are assigned a weight of 1.0"*);
`ORDER BY rank` is equivalent, and `INSERT INTO ft(ft, rank) VALUES('rank', 'bm25(10.0, 5.0)')` sets a persistent
default (§6.11). `[INFERENCE]` AD-263's `bm25(chunks_fts, <title weight>, 1.0)` is this API with a title column
first; every consumer must negate the score before min-max normalising (§8), as pi-knowledge does
(`-bm25(chunks_fts)`, §3.2).

### 4.5 `highlight()`, `snippet()`, and why there are no offsets

`[SOURCE]` R2 §1, FTS5 §5.1.2–§5.1.3 and Appendix A. `highlight(ft, col, open, close)` returns the whole column
with overlapping phrase instances coalesced into one marker pair; `snippet(ft, col_or_-1, open, close, ellipsis,
max_tokens ≤ 64)` picks the fragment maximising distinct query terms. **FTS5 has no `offsets()`**: Appendix A —
*"FTS5 has no matchinfo() or offsets() function … any required functionality may be implemented within the
application code."* The C-level `xInst` yields **token** offsets, not bytes; `fts5_aux.c::fts5HighlightFunction`
calls `xColumnText` and then **re-tokenises** the column with `xTokenize_v2`, whose callback receives
`iStartOff/iEndOff` byte offsets per token and pairs them with `xInst` positions
(<https://raw.githubusercontent.com/sqlite/sqlite/master/ext/fts5/fts5_aux.c>). Contentless tables cannot
`highlight()` (they cannot `xColumnText`). R2's practical options: private-use sentinel characters through
`highlight()` and a scan in Rust, or a custom aux function over FFI (no rusqlite wrapper). `[INFERENCE]` Every
one of those offsets is into the **FTS column's text** — which under AD-263 is the folded text (§4.3) — so even
the sentinel trick would need a fold-aware reverse map. `search::find` already returns raw-byte spans for a folded
match (§2.6); AD-263's "marks come from keeper's own matcher, never `highlight()`" is the shorter path and the
only one that keeps the sessions matcher and the notes marks on one rule.

### 4.6 Prefix queries

`[SOURCE]` R2 §1, FTS5 §3.3. A prefix token is a string followed by `*` **outside** the quotes: `'"one two thr" *'`
and `'thr*'` both mean prefix `thr`; `'"one two thr*"'` *"May not work as expected!"* — the `*` inside quotes is
passed to the tokenizer. pi-knowledge does not use prefixes (§3.2). `[INFERENCE]` AD-263's type-ahead form is
therefore `"foo"*` on the last whitespace word only — earlier words are complete words the person has finished
typing, the last one may be half-typed; folding the query with `fold_str` first keeps the prefix in the same
alphabet as the column.

### 4.7 Escaping and the query grammar

`[SOURCE]` R2 §1, FTS5 §3.1. A query string is either double-quoted (embedded `"` as `""`) or a *bareword*
limited to non-ASCII characters, the 52 ASCII letters, 10 digits, underscore and the substitute character;
`AND`/`OR`/`NOT` are reserved and case-sensitive; anything else *"must be quoted"* and unquoted specials *"may be
interpreted differently by some future version"*. Whitespace between phrases is implicit AND; precedence `NOT` >
`AND` > `OR`; `col : term` filters a column; `NEAR(a b, N)`; `^term` is initial-token. **The safe recipe R2
derives:** split user input on whitespace, wrap each token as `"tok"` with `"`→`""`, append `*` after the closing
quote where a prefix is wanted, join with a space (implicit AND) or `OR`. `[REPO]` G4 §7: the archive's recipe
wraps the *whole* query in one quoted phrase (`fts.rs:191-197`) — a phrase search; AD-263's per-word quoting is
the pi-knowledge shape (§3.2) and is what makes `bm25()` weigh the words independently and the OR fallback
possible. `[INFERENCE]` Because every user word is quoted, a person typing `and`, `or`, `not`, `*` or `:` searches
for those literally; that is the archive's guarantee carried over.

### 4.8 External-content vs content-owning

`[SOURCE]` R2 §1, FTS5 §4.4.3. An external-content table (`content='t1', content_rowid='a'`) reads column values
back with `SELECT <content_rowid>, <cols> FROM <content> WHERE <content_rowid> = ?`; *consistency is the caller's
job* — the canonical AFTER INSERT/DELETE/UPDATE triggers using the `'delete'` special insert are given verbatim in
the docs; *"external content tables do not support REPLACE conflict handling"*. pi-knowledge uses this shape with
triggers (§3.2). `[REPO]` G4 §7: keeper has both shapes in one crate and recorded why the second exists — DW-48's
VACUUM-renumbers-rowid hazard (`recordings_fts.rs:21-28`, §2.5). `[INFERENCE]` AD-261 picks content-owning for
`search.db` because the index is *derived* and disposable: there is no base row the FTS table must stay bound to,
the text it owns is exactly the text it indexes, an explicit `INTEGER PRIMARY KEY` chunk id cannot be renumbered,
and every chunk write happens in the transaction that writes the chunk. The cost — the folded text stored once
more, beside the raw text the row snippet and marks need — is bytes in a file that is already a cache.

---

## 5. Alternatives rejected

`[INFERENCE]` unless marked; each rejection names the constraint it fails and the evidence. These are closed for
this epic: reopening any of them is a decision, not a tuning.

### 5.1 tantivy — rejected: a second engine

`[SOURCE]` R2 §2. tantivy 0.26.2 (MIT, <https://crates.io/api/v1/crates/tantivy>; main 0.27.0 unreleased,
<https://raw.githubusercontent.com/quickwit-oss/tantivy/main/Cargo.toml>): a directory of segment files behind
`MmapDirectory`, default features `mmap` (memmap2, fs4, tempfile), `stopwords`, `lz4-compression`,
`columnar-zstd-compression`, `stemmer`, ~40 direct dependencies. Tokenizers: `SimpleTokenizer` + `RemoveLongFilter(40)`
+ `LowerCaser`, `en_stem`, `NgramTokenizer`, `AsciiFoldingFilter`, `StopWordFilter`, custom chains
(<https://docs.rs/tantivy/latest/tantivy/tokenizer/index.html>). Stemmers come from `rust-stemmers` 1.2.0 —
eighteen languages, **no Polish** (<https://raw.githubusercontent.com/CurrySoftware/rust-stemmers/master/README.md>);
main switches to `frostem` but enables no `polish` feature; `tantivy-stemmers` 0.4.0 has a non-Snowball
`polish_yarovoy` but pins `tantivy-tokenizer-api ^0.3` (2024-06-27) (<https://docs.rs/crate/tantivy-stemmers/latest>).
Snippets: `SnippetGenerator` with `<b>` HTML or `highlighted()` char ranges
(<https://docs.rs/tantivy/latest/tantivy/snippet/index.html>). R2's cost comparison: FTS5 *"lives inside the SQLite
file already used … is transactional with note rows … costs ~0 extra binary; tantivy adds a separate on-disk index
directory outside SQLite transactions, ~40 crates (compile time), mmap file handles, and a second consistency
problem (index vs DB)"*.

`[INFERENCE]` Rejected because: keeper already ships FTS5 in production (§2.5) and AD-261 keeps the index in one
SQLite file whose consistency is one writer's transaction; a second engine would be a second consistency problem
beside the one AD-57 already worried about; its one advantage — pluggable Rust stemmers — buys nothing for Polish
today (no stemmer of proven quality, §5.4) and its char-range snippets are ranges into *its* tokenised view, not
the raw body the editor marks. Whether its `AsciiFoldingFilter` folds `ł` is `[UNVERIFIED]` (§12) — and moot.

### 5.2 Local ONNX embeddings via fastembed / ort — rejected: a new egress destination, D-4/D-5, no x86_64 mac, size

`[SOURCE]` R2 §3. fastembed 7.0.1 (Apache-2.0) = `ort =2.0.0-rc.13` (MIT OR Apache-2.0) + `tokenizers` 0.23
(Apache-2.0; `onig`/`onig_sys` MIT over BSD-2-Clause Oniguruma) + optional `hf-hub` 0.5
(<https://raw.githubusercontent.com/Anush008/fastembed-rs/main/Cargo.toml>). Models are pulled from
`https://huggingface.co` (`HF_ENDPOINT`), cached in `.fastembed_cache` **relative to CWD** unless
`FASTEMBED_CACHE_DIR`/`HF_HOME` (`src/common.rs`); a fully offline path exists through
`try_new_from_user_defined(UserDefinedEmbeddingModel { onnx_file: Vec<u8>, tokenizer_files, … })` with
`default-features = false` dropping the HF client (`src/text_embedding/init.rs`; <https://github.com/Anush008/fastembed-rs>).
**ort** downloads statically-linked ONNX Runtime 1.30 from `cdn.pyke.io`; `dist.tsv` lists `aarch64-apple-darwin`
(+CoreML), `aarch64-apple-ios`, `aarch64-apple-ios-sim`, Android, Windows, Linux — **no `x86_64-apple-darwin`**;
*"All x86-64 binaries are compiled with a baseline requirement of x86-64-v3"*; archives 9 355 017 B (darwin-arm64),
9 662 011 B (ios-arm64), 10 421 875 B (linux-x86_64), lzma2
(<https://raw.githubusercontent.com/pykeio/ort/main/ort-sys/build/download/dist.tsv>,
<https://ort.pyke.io/misc/prebuilt-binaries>, <https://ort.pyke.io/setup/cargo-features>,
<https://ort.pyke.io/setup/linking>); ONNX Runtime is MIT
(<https://raw.githubusercontent.com/microsoft/onnxruntime/main/LICENSE>); a "minimal build" exists for smaller
binaries (<https://onnxruntime.ai/docs/build/custom.html>). Model files on disk: multilingual-e5-small fp32 448 MiB,
`model_O4` 224, `model_qint8_avx512_vnni` 112 (no fastembed enum for it — user-defined path only); e5-base
1 058/529/265; bge-m3 2 161; nomic-v1.5 int8 130; embeddinggemma-300m 167–1 177
(`https://huggingface.co/api/models/<repo>?blobs=true`). Final executable growth from static ORT is `[UNVERIFIED]`
(§12; "tens of MB" is R2's inference from the 9–10 MB compressed archives).

`[REPO]` G4 §3, §5 (§2.10). Every one of these paths collides with a recorded decision: a runtime weight download
is a **new egress destination** (a `docs/egress.md` row, a new `EgressKind`, a new D-number) that no decision
permits, and it would be a destination keeper chose — the exact thing D-4 refuses; shipping weights in the bundle
is what D-5 refuses on licence-provenance grounds; and `keeper-core` would gain its first ML crate through the
licence firewall and the tauri-free/sync-free guards. `[INFERENCE]` Rejected for this epic on those grounds alone;
the engineering facts make it worse — no prebuilt ORT for Intel Macs (a release target would need its own ORT
build or `load-dynamic`), a 112–448 MB model per machine, a compressed-10 MB runtime, and a CWD-relative cache
default that would have to be overridden. AD-264's route — the provider the user already configured — has none of
these and matches D-4's sentence exactly: *the endpoint is yours*.

### 5.3 candle — rejected: same egress problem, more hand-wiring, unmeasured speed

`[SOURCE]` R2 §3. `candle-core` 0.11.0 (MIT OR Apache-2.0), pure Rust, CPU with optional MKL/Accelerate, Metal,
loads safetensors, ships a BERT sentence-embedding example
(<https://raw.githubusercontent.com/huggingface/candle/main/README.md>); fastembed uses it only behind
`qwen3`/`nomic-v2-moe`. Trade-off per R2: no C++ runtime to link, but the XLM-R forward pass and pooling are wired
by hand and CPU throughput vs ORT is `[UNVERIFIED]` (§12). `[INFERENCE]` It removes the x86_64-mac wall of §5.2
and nothing else: the weights still have to come from somewhere, which is the D-4/D-5 wall.

### 5.4 A custom FTS5 tokenizer with a Polish stemmer — rejected for this epic: FFI, and unproven quality

`[SOURCE]` R2 §1, §8. Snowball upstream now ships `algorithms/polish.sbl`
(<https://github.com/snowballstem/snowball/tree/master/algorithms>, <https://snowballstem.org/algorithms/>,
<https://raw.githubusercontent.com/snowballstem/snowball/master/algorithms/polish.sbl>); `frostem` (BSD-3-Clause,
pure Rust, tracks Snowball main) exposes it behind `features = ["polish"]`
(<https://docs.rs/crate/frostem/latest/features>, <https://docs.rs/crate/frostem/latest>). Using it *inside*
FTS5 means a custom tokenizer through the C `fts5_api`, which rusqlite does not wrap (§4.2). The retrieval
quality of the new Snowball Polish stemmer is `[UNVERIFIED]` — no published evaluation was found (§12). `[INFERENCE]`
AD-263's "no stemming" is the honest position: a stemmer of unknown quality applied through hand-written FFI, to
fix inflection the prefix `*` and the vector leg already soften, is a DW to measure on the owner's vault, not a
day-one dependency. When it is measured, `frostem` is the candidate, applied as a pre-fold beside `fold_str` (not
inside FTS5) so the symmetry rule of §3.2 holds without FFI.

### 5.5 sqlite-vec, usearch, hnsw_rs — not needed at vault scale

`[SOURCE]` R2 §5. `sqlite-vec` (MIT/Apache-2.0; 0.1.9 stable, 0.1.10-alpha.4; *"pre-v1, so expect breaking
changes"*; pure C compiled by `cc`; registered with `sqlite3_auto_extension`) is **brute-force only** — *"currently
focused on really fast brute-force vector search"*; `vec0(… float[N] distance_metric=cosine)`, `MATCH ? AND k = N`
(<https://raw.githubusercontent.com/asg017/sqlite-vec/main/README.md>, <https://alexgarcia.xyz/sqlite-vec/rust.html>,
<https://alexgarcia.xyz/sqlite-vec/features/vec0.html>, <https://alexgarcia.xyz/sqlite-vec/features/knn.html>).
`usearch` 2.26.2 (Apache-2.0, C++ header + binding) and `hnsw_rs` 0.3.4 (MIT/Apache-2.0, pure Rust) are HNSW ANN
libraries (<https://raw.githubusercontent.com/unum-cloud/usearch/main/README.md>,
<https://raw.githubusercontent.com/jean-pierreBoth/hnswlib-rs/master/README.md>). `[INFERENCE]` §7 shows brute
force in Rust over BLOBs is milliseconds at the index's own design ceiling; sqlite-vec would add a pre-v1 C
dependency and an auto-extension hook to do the same arithmetic SQLite-side, and ANN solves a problem that starts
around 100 k vectors (R2's decision table: *"Unnecessary below ~100k vectors"*). AD-265's "no ANN, no sqlite-vec"
follows.

### 5.6 Trigram for the notes index — not chosen: no word notion for bm25, 3-char floor

`[SOURCE]` R2 §8. Trigram gives case-insensitive substring matching with no morphology, so Polish inflection
(`notatka/notatki/notatkę`) matches on the shared stem substring — at the cost of a ≥ 3-character minimum, a
larger index (one posting per character position), and `bm25()` weights that become *"trigram hits rather than word
hits"* (FTS5 §4.3.4). `[REPO]` G4 §7: it is what the archive uses, with the `< 3` LIKE fallback that follows from
the floor. `[INFERENCE]` For a ranked list of *words that matched*, bm25 over words is the thing being asked for;
the prefix `*` on the last word gives the type-ahead behaviour trigram is usually chosen for; and inflection is
the vector leg's job and §5.4's DW. Not chosen for `search.db`; the archive keeps it.

---

## 6. Embeddings over the provider you already have

All of §6 is `[SOURCE]` R2 §3–§4 unless marked, read 2026-09-19.

### 6.1 Ollama native: `POST /api/embed`

`[SOURCE]` R2 §4 — Ollama, <https://raw.githubusercontent.com/ollama/ollama/main/docs/api.md> and
<https://docs.ollama.com/capabilities/embeddings>. Request `{ "model", "input": string | [string], "truncate": bool
(default true; error if false and the context is exceeded), "options", "keep_alive" (default "5m"), "dimensions" }`;
response `{ "model", "embeddings": [[f32…], …], "total_duration", "load_duration", "prompt_eval_count" }`. The
older `/api/embeddings` (`prompt` → `embedding`) *"has been superseded by `/api/embed`"*. The docs state: *"The
`/api/embed` endpoint returns L2‑normalized (unit‑length) vectors."*

### 6.2 The OpenAI-compatible wire: `POST /v1/embeddings`

`[SOURCE]` R2 §4 — Ollama, <https://docs.ollama.com/api/openai-compatibility>: `/v1/embeddings` supports `model`,
`input` (string or array of strings; **not** token arrays), `encoding_format`, `dimensions`; not `user`; the API key
is *"required but ignored"*. `[SOURCE]` R2 §4 — OpenAI,
<https://developers.openai.com/api/reference/resources/embeddings/methods/create>: the reference shape keeper
should parse.

Request (the subset both accept):

```json
{ "model": "<model>", "input": ["<text 1>", "<text 2>"], "encoding_format": "float" }
```

Response:

```json
{ "object": "list",
  "data": [ { "object": "embedding", "embedding": [0.0123, -0.0456, …], "index": 0 },
            { "object": "embedding", "embedding": [ … ],                "index": 1 } ],
  "model": "<model>",
  "usage": { "prompt_tokens": 0, "total_tokens": 0 } }
```

`[SOURCE]` OpenAI's limits on its own service: input arrays ≤ 2 048 entries, ≤ 300 000 tokens summed per request,
≤ 8 192 tokens per input; `dimensions` only for `text-embedding-3+`; `encoding_format` float | base64. `[INFERENCE]`
Ollama returning the OpenAI response envelope is what "OpenAI-compatible" means on that page, but the response
fields were not separately quoted by R2 for Ollama — parse `data[].embedding` by `data[].index`, never by array
position, and treat `usage` as optional. `[INFERENCE]` Whether `/v1/embeddings` on Ollama returns unit vectors is
not stated (only `/api/embed` is documented as L2-normalised, §6.1) — which is why AD-264 normalises in Rust
regardless of provider: the cosine in §7 then needs no per-provider knowledge.

`[REPO]` G4 §4 (§2.10). This is the wire keeper already speaks for chat (`/v1/chat/completions`), on the client
with the timeout doctrine, the no-redirect policy and the sensitive bearer header (`bots/http.rs`); the URL is
one more `Endpoint::url` path. Hermes: its route table in keeper's earlier research has no embeddings endpoint
(`discover.rs` module doc; G4 §"Where could embeddings come from"); support is `[UNVERIFIED]` (§12), hence AD-264's
quirk row `embeddings: Support` and a refusal sentence rather than a silent skip.

### 6.3 Models: licence, dimensions, context, Polish quality, Ollama availability

`[SOURCE]` R2 §3–§4. Licences and specs from the Hugging Face model cards / model API; Polish retrieval from
PL-MTEB v2 Table 2 (mean nDCG@10 over 11 Polish retrieval tasks, <https://arxiv.org/pdf/2405.10138>, v2 2026-04-24;
leaderboard <https://huggingface.co/spaces/PL-MTEB/leaderboard>); Ollama availability = the library page resolves
at `https://ollama.com/library/<name>` (fetched 2026-09-19). "—" = R2 did not pull it; `[UNVERIFIED]` items are in
§12.

| Model | Licence | Dims | Max tokens | Polish nDCG@10 (PL-MTEB v2) | On the Ollama library | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `intfloat/multilingual-e5-small` | MIT | 384 | 512 (*"Long texts will be truncated to at most 512 tokens"*) | **46.00** | not among the pages R2 fetched `[UNVERIFIED]` | *"Each input text should start with "query: " or "passage: ", even for non-English texts"*; 100 languages incl. `pl`; pi-knowledge's model (<https://huggingface.co/intfloat/multilingual-e5-small>) |
| `intfloat/multilingual-e5-base` | — | 768 | — | **47.63** | `[UNVERIFIED]` | same prefixes (fastembed registry) |
| `intfloat/multilingual-e5-large` | — | — | — | **52.43** | `[UNVERIFIED]` | |
| `BAAI/bge-m3` | MIT | 1 024 | 8 192 | not in Table 2 `[UNVERIFIED]` | **yes** (`bge-m3`) | *"more than 100 working languages"*; *"no longer requires adding instructions to the queries"*; card recommends hybrid + *"BM25 remains a competitive baseline"* (<https://huggingface.co/BAAI/bge-m3>) |
| `nomic-ai/nomic-embed-text-v1.5` | Apache-2.0 | 768 (Matryoshka to 64) | 8 192 | — (**English-only card**, language `en`) | **yes** (`nomic-embed-text`) | requires `search_query: ` / `search_document: ` prefixes (<https://huggingface.co/nomic-ai/nomic-embed-text-v1.5>) |
| `google/embeddinggemma-300m` | **Gemma Terms** (`license: gemma`, gated; the `onnx-community` ONNX mirror is ungated but still `license: gemma`) | 768 | — | not in Table 2 `[UNVERIFIED]` | **yes** (`embeddinggemma`) | *"not a permissive OSI licence; keep it out of any default/bundled path"* (R2) (<https://huggingface.co/api/models/google/embeddinggemma-300m>, <https://huggingface.co/api/models/onnx-community/embeddinggemma-300m-ONNX>) |
| `Qwen3-Embedding-0.6B` | — | — | — | **48.59** | **yes** (`qwen3-embedding`) | Ollama's recommended list: embeddinggemma, qwen3-embedding, all-minilm |
| `paraphrase-multilingual-MiniLM-L12-v2` | — | 384 | — | **30.40** | — (not the same model as Ollama's `all-minilm`) | |
| `snowflake-arctic-embed-m-v2.0` | — | — | — | **52.21** | `[UNVERIFIED]` | |
| `mmlw-roberta-base` (Polish-distilled, 124M) | — | — | — | **53.6** | `[UNVERIFIED]` | best Polish number in the table |
| `all-minilm`, `mxbai-embed-large` | — | — | — | — | **yes** | library pages resolve; specs not pulled |

`[INFERENCE]` What the table says for AD-264: keeper does not pick a model — the user's provider rows do — but
the epic's *documentation* can say what the numbers say: for a Polish+English vault a multilingual E5 or bge-m3 is
the informed choice; `nomic-embed-text` is Ollama's most common embedding pull and is documented for English only;
`embeddinggemma` is under Gemma terms — that binds the person who installed it into their Ollama, not keeper, and
keeper must not *recommend* it as a default (D-5's licence posture, §2.10). Prefix conventions differ per model
family (`query:`/`passage:` for E5, `search_query:`/`search_document:` for nomic, none for bge-m3): keeper cannot
know which the user's model wants — a per-model prefix table is `[INFERENCE]` a DW; AD-262's `<title> ›
<breadcrumb>\n\n<text>` contextual prefix is model-independent and is the one keeper applies.

### 6.4 Batching

`[SOURCE]` R2 §4. No documented hard cap on the `/api/embed` input array length (`[UNVERIFIED]`, §12); community
threads report memory-bound failures (*"unable to fit entire input in a batch"*, GitHub ollama/ollama issue
#13340, <https://github.com/ollama/ollama/issues/13340>) and quality problems at large batches (issue #6262,
<https://github.com/ollama/ollama/issues/6262>); practical guidance from those threads is 16–64 inputs per request.
pi-knowledge uses 64 (§3.3). `[INFERENCE]` AD-264's "batches of ≤ 32" sits inside that band with headroom for the
1 MiB-class bodies keeper indexes head-only; the read-timeout doctrine (silence, not total) already fits a request
that takes seconds on a laptop CPU (§2.10).

---

## 7. Vector storage and search at vault scale

### 7.1 The arithmetic, stated so it can be checked

`[INFERENCE]` over `[REPO]` G1 §1 (`index.rs:3`: a vault *"tops out around ten thousand files"*) and
coordinator-decisions AD-262/AD-265 (~5 chunks per note). At the design ceiling: 10 000 notes × 5 chunks =
50 000 vectors. At 768 dimensions (nomic, e5-base, embeddinggemma) that is 50 000 × 768 × 4 B = **153.6 MB** of
f32; at 384 (e5-small) **76.8 MB**; at 1 024 (bge-m3) **204.8 MB**. A query is 50 000 dot products of that width:
38.4 M multiply-adds at 768-d. The owner's actual vault in the screenshot is 688 notes (§1.1): ~3 400 vectors,
**~10 MB** at 768-d, 2.6 M multiply-adds — trivially small.

`[SOURCE]` R2 §5. Two measured reference points bracket the compute and the I/O: usearch reports an *exact*
search over 10 000 × 1 024 f32 vectors in **2.54 ms** (vs FAISS `IndexFlatL2` 55.3 ms, on Colab)
(<https://raw.githubusercontent.com/unum-cloud/usearch/main/README.md>) — 10.24 M multiply-adds, so ≈ 4 GMAC/s
with SIMD; sqlite-vec measured 100 000 vectors **from disk** on an M1 mini 8 GB, *"for small dimensions
(1024/768/384/192), all response are below 75ms"*, and 1 M × 192-d in 192 ms
(<https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html>). `[INFERENCE]` The sqlite-vec 100 k × 384-d
case is byte-for-byte the same volume as keeper's ceiling at 768-d (153.6 MB), so **≤ 75 ms per query from disk**
is the worst case the evidence supports for a cold BLOB scan, and ≈ 10 ms is what the compute costs once the
vectors are in memory. AD-265's "≈ ms" is right for the compute and for the owner's vault; the epic should state
which it does at the ceiling — hold the vectors in memory beside the snapshot (150–200 MB resident at 10 k notes
with a 768–1 024-d model; 10 MB for the owner today) or scan BLOBs per query (≤ 75 ms, no resident cost). This is
a detail for 76.6, flagged here because the number is easy to misremember as free.

### 7.2 Storage shape: BLOBs keyed by chunk + model, not a sidecar

`[SOURCE]` R1 §3, §"Competing patterns", §"Leave". pi-knowledge's sidecar file with implicit rowid ordering is an
implicit contract that costs a full vector-file rewrite per update (`engine.js:1047-1097`); R1's own advice for a
Rust port: *"store vectors as BLOBs in the chunks table (or a parallel table keyed by chunk id) — simpler and
transactionally consistent; the file-split exists for streaming scans, which only pay off at >100k chunks. For
note-sized vaults, read all blobs and dot them in memory."* `[SOURCE]` R2 §5: the manual sqlite-vec path is the
same shape — `vec_distance_cosine(blob, ?)` over a plain `BLOB` column — which is what a Rust loop over `&[f32]`
does without the extension. `[REPO]` G4 §2: the house rule "every queryable fact a scalar column, never a JSON
blob" (AD-139) is about *queryable facts*; a vector is an opaque payload read whole, and f32 little-endian bytes
are its natural encoding. `[INFERENCE]` AD-264's "f32 BLOBs in `search.db` keyed by chunk + model" is this; the
model key is what makes a model change an invalidation rather than a mixed space (§3.3), and keying by chunk id
means an incremental update touches only the rows whose hash changed (§3.6).

### 7.3 Similarity

`[SOURCE]` R1 §3; R2 §4 (§6.1). With vectors L2-normalised once at write time, cosine is the dot product; Ollama's
`/api/embed` already returns unit vectors and pi-knowledge normalises at pooling. `[INFERENCE]` The owner's *"cosine
similarity (lub innego similarity jeżeli trzeba)"* is answered by cosine: every model in §6.3 is trained for it,
and no other metric was found to matter for retrieval in the sources read. Normalising in Rust regardless of the
provider (AD-264) removes the one case where it would matter — a provider that returned raw vectors.

---

## 8. Fusion

### 8.1 pi-knowledge's formula, with its constants

`[SOURCE]` R1 §4 (§3.4; `fusion.js:22-40` re-read this pass). Per query: take the top-N chunk lists from each leg;
min-max normalise each list on its own (`(s − min) / (max − min)`, a single-valued list → 1); then

```
score(chunk) = 0.45 · lex_norm + 0.55 · vec_norm + (0.15 if the chunk is in both lists, else 0)
```

with `lex = −bm25(chunks_fts)` (negated first, §4.4), the vector score the cosine, `MIN_HYBRID_SCORE = 0.18` as the
floor on the fused score, and the three-layer lexical anchor (§3.4) deciding what may enter at all.

### 8.2 Reciprocal rank fusion

`[SOURCE]` R2 §6 — Cormack, Clarke, Büttcher, SIGIR 2009, <https://cormack.uwaterloo.ca/cormacksigir09-rrf.pdf>:
`RRF(d) = Σ_{r ∈ R} 1 / (k + r(d))`; *"k = 60 was fixed during a pilot investigation … indicated that k = 60 was
near-optimal, but that the choice was not critical"*; it *"combines ranks without regard to the arbitrary
scores"*; beat Condorcet / CombMNZ / the best single run by 4–5 % MAP on TREC. `[SOURCE]` R1 §4: pi-knowledge
ships it and does not use it — dogfood found it *"compressed scores too much for ranking diagnostics"*.

### 8.3 Bruch, Gai, Ingber 2023 — convex combination beats RRF

`[SOURCE]` R2 §6 — *An Analysis of Fusion Functions for Hybrid Retrieval*, arXiv 2210.11934v2,
<https://arxiv.org/pdf/2210.11934>. A convex combination `α · φ(f_Sem) + (1 − α) · φ(f_Lex)` with *theoretical*
min-max normalisation (`φ_tmm`: BM25's infimum is 0, cosine's is −1) — "TM2C2" — *"outperforms RRF in in-domain
and out-of-domain settings"*; RRF is *"sensitive to its parameters"*; at α = 0.8 (semantic weight), NDCG@1000:
MS MARCO **0.454 vs RRF(60) 0.425**, NQ 0.542/0.514, Quora 0.901/0.877, HotpotQA 0.699/0.675, FiQA 0.496/0.464;
RRF(5) is closer than RRF(60). The choice of normalisation is *"a rather small detail"* (rank-equivalent under
linear transforms), but *unnormalised* fusion *"leads to a severe degradation"*. Tuning α *"requires just a
handful of labeled queries"*. Setup: BM25 k1 = 0.9, b = 0.4, all-MiniLM-L6-v2 cosine, top-1000 from each system,
missing scores computed for the union set.

### 8.4 What the floor means — the pool-relative caveat

`[SOURCE]` R1 §"Competing patterns" (§3.4): per-query min-max makes the top result of each list exactly 1.0, so a
floor on the fused score is relative to that query's pool — a weak query still yields a 1.0-normalised top hit —
and pi-knowledge's real guard against confident junk is the lexical-coverage gate, not the 0.18 threshold. `[SOURCE]`
R2 §6: Bruch et al.'s *theoretical* min-max (fixed infima) is the variant whose scores are comparable across
queries. `[INFERENCE]` For a note list this matters in one place: the *meaning-only* hit — a chunk the words did
not find. Admitting it on a normalised fused score would admit the best of a bad pool; admitting it on the **raw
cosine** against a fixed floor is the query-independent test. That is AD-265's rule, and it is also why the floor
is expressed as a *similarity* floor rather than a fused-score floor.

### 8.5 The reading for AD-265, and where it leans

`[INFERENCE]` over §8.1–§8.4. AD-265 takes pi-knowledge's weights (0.45 / 0.55 + overlap bonus) rather than Bruch's
α = 0.8: the reference's numbers were dogfooded on markdown-heavy corpora with a small multilingual model, which is
closer to a notes vault than MS MARCO with all-MiniLM, and the lexical leg is the one the owner asked for by name.
It takes Bruch's *finding* — convex over RRF, and never unnormalised — as the reason not to reach for the
zero-parameter RRF that R2 calls *"the safe default without labelled queries"*. It keeps pi-knowledge's lexical
anchor in the shape a list needs: no lexical hit → admitted only above the raw-cosine floor and labelled
**matched by meaning** (§10.3 for why labelled rather than marked), best chunk per note, and results below the
floor dropped rather than padded. Two things are honestly open and belong in a DW, not in the AD: the α (Bruch:
a handful of labelled queries on the owner's vault would settle it) and the value of the similarity floor, which is
model-dependent (Smart Connections' README says as much of scores, §10.3).

---

## 9. Chunking evidence

### 9.1 Chroma — chunk size and overlap, measured

`[SOURCE]` R2 §7 — Smith & Troynikov, *Evaluating Chunking Strategies for Retrieval*, Chroma, 2024-07-03,
<https://www.trychroma.com/research/evaluating-chunking>. Token-level recall/precision/IoU over five corpora. With
`text-embedding-3-large`: `RecursiveCharacterTextSplitter` at 200 tokens / 0 overlap *"performs well … consistently
high performing across all evaluation metrics"*; 400/0 recall 89.5 vs 400/200 recall 88.1 with lower precision;
OpenAI's default 800/400 *"results in slightly below-average recall and the lowest scores across all other
metrics"*. With `all-MiniLM-L6-v2` (a small 384-d model): *"TokenTextSplitter achieves the highest recall of 0.824
when configured with chunk size 250 and chunk overlap of 125. This then drops to 0.771 when chunk overlap is set
to zero, suggesting that for smaller context, overlapping chunks are necessary for high recall."*

### 9.2 Pinecone — structure-aware splitting for Markdown

`[SOURCE]` R2 §7 — Pinecone, *Chunking Strategies*, 2025-06-28, <https://www.pinecone.io/learn/chunking-strategies/>:
fixed-size first, then content-aware; for Markdown, *"By recognizing the Markdown syntax (e.g., headings, lists,
and code blocks), you can intelligently divide the content based on its structure and hierarchy, resulting in more
semantically coherent chunks"*; test *"smaller chunks (e.g., 128 or 256 tokens) … larger chunks (e.g., 512 or
1024 tokens)"*; "chunk expansion" (return neighbours) as a post-step.

### 9.3 A Rust crate that does this, if the hand-rolled one is not wanted

`[SOURCE]` R2 §7 — `text-splitter` 0.32.0, MIT: `MarkdownSplitter` (CommonMark/GFM) splits by ascending semantic
levels — chars → graphemes → words → sentences → soft breaks → inline → block → thematic breaks → **headings by
level** — *"Boundaries of higher semantic levels are always included when merging, so that the chunk doesn't
inadvertantly cross semantic boundaries"*; `ChunkConfig::new(range).with_overlap(n)`, optional
`.with_sizer(tokenizers::Tokenizer)` (<https://raw.githubusercontent.com/benbrandt/text-splitter/main/README.md>,
<https://docs.rs/text-splitter/latest/text_splitter/struct.ChunkConfig.html>). `[INFERENCE]` It would be a new
crate for ~90 lines of logic pi-knowledge shows are portable (R1 §"Copy"); keeper's manifest prefers no new
package (§2.10); and the hand-rolled version can keep the byte offsets and the fence rule (§3.1) that the epic
needs. Named here so the choice is visible, not because it is recommended.

### 9.4 Where the evidence pulls against AD-262, and why the decision still stands

`[INFERENCE]` over §3.1, §9.1, §9.2. Three sources agree on heading-first, structure-aware splitting and a target
in the low hundreds of tokens; AD-262's ~300-token target is inside every recommended band (pi-knowledge 450
estimated; Chroma 200–400; Pinecone 128–512; e5-small's 512-token window). They **disagree on overlap**:
pi-knowledge chose none because overlap produced near-duplicate retrieval units in its dogfood (§3.1); Chroma
measured a 5-point recall cost of zero overlap with a small model (§9.1). AD-262 follows pi-knowledge, and the
reasons hold for a *note list* specifically: (i) the list collapses to one row per note (best chunk), so two
overlapping chunks of one note compete with each other for nothing but waste — the near-duplicate cost is real
and the recall benefit is partly absorbed by the collapse; (ii) the lexical leg's recall is independent of chunk
boundaries for a single word (the word is in *some* chunk wherever the cut falls), so the recall Chroma measured
on a vector-only retriever is an upper bound on what overlap could add here; (iii) the heading breadcrumb prefix
(§3.1) restores the context a cut removes. What overlap would cost on the owner's vault is measurable — this is a
DW for after 76.6 ships, with the Chroma numbers as the thing to beat, not a reason to change the decision now.
R2's own synthesis (*"~200–300 model tokens per chunk with ~10–20 % overlap for a small model"*) is graded
`[INFERENCE]` in R2 and is not stated by either source as such.

---

## 10. Search UX

All of §10 is `[SOURCE]` R3 unless marked, read 2026-09-19.

### 10.1 Rows: excerpt windows, marks, a count

`[SOURCE]` R3 §1 — Omnisearch (Obsidian community plugin, GPL-3 — behaviour reference only, no code copied):
`excerptBefore = 100` / `excerptAfter = 300` characters around the first match offset, `…` at the cut ends, every
match wrapped in `<span class="suggestion-highlight omnisearch-highlight omnisearch-default-highlight">`, the exact
multi-word phrase promoted to the first excerpt, `getMatches` bounded at 100 matches or 50 ms per document, and
*"Highlight matching words in results"* as a user setting
(<https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/globals.ts>,
<https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/tools/text-processing.ts>,
<https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/settings/settings-ui.ts>). `[SOURCE]`
Craft: *"The yellow number indicates how many instances of the results are matching your search inside that
document. Hover over the number to get a preview"*; *"Title matches are prioritized … Exact phrase matches come
next, especially those found earlier in the document"*; prefix-only matching ("Pors" finds "Porsche"), minimum 3
characters Latin / 2 otherwise (<https://support.craft.do/en/organize-and-find/search>). `[SOURCE]` VS Code's
search view groups *"results … into files containing the search term, with an indication of the hits in each
file"* and ends with *"`{x} results in {y} files`"* (<https://code.visualstudio.com/docs/editing/codebasics>,
<https://code.visualstudio.com/api/references/theme-color>). `[SOURCE]` Bear: *"Special Searches, tags, and quoted
phrases are highlighted in search results"* (<https://bear.app/faq/how-to-search-notes-in-bear/>). Logseq's ⌘K
highlights keywords in results (issue #5844, <https://github.com/logseq/logseq/issues/5844>), and its API returns no
highlight tags (issue #8881, <https://github.com/logseq/logseq/issues/8881>).

`[INFERENCE]` For UX-DR95: the row's excerpt should be re-cut around the first mark rather than reuse the leading
240-char prose snippet (a body match on line 40 is invisible in a leading excerpt — Omnisearch's 100/300 window is
the precedent and `search::Hit.snippet` already windows ±48 chars per line, §2.6); the marks are `<mark>` runs
over that excerpt from `NoteHitVm.marks`; a per-row **count** is Craft's shipped pattern and costs nothing once
the marks are computed. `[REPO]` G2 §4: the slot is `note-row.tsx:316-318` and the HoverHint detail at `:390`, and
the unread branch shows `row.origin` instead — the design must say which wins when a note is both unread and a
hit.

### 10.2 In the open note: highlight lifetime

`[SOURCE]` R3 §1 — VS Code documents its editor find colours as *"Find colors depend on the current find string
in the Find/Replace dialog"* (`editor.findMatchBackground` current match, `editor.findMatchHighlightBackground`
other matches — *"must not be opaque so as not to hide underlying decorations"*, plus overview-ruler marks), and
offers `editor.find.closeOnResult` to auto-close the widget; find *"immediately starts searching as you type"*
(`editor.find.findOnType`) (<https://code.visualstudio.com/api/references/theme-color>,
<https://code.visualstudio.com/docs/editing/codebasics>). `[SOURCE]` Obsidian core marks the element a hit opened
to with `.is-flashing`, a *flash* themed by `--text-highlight-bg` that times out by itself; a community plugin
("Search Highlight+") exists precisely because a persistent in-note highlight is not core
(<https://forum.obsidian.md/t/flashing-elements-color/80720>; Obsidian developer docs `Colors.md`, URL not recorded
by R3). `[SOURCE]` Craft: *"Clicking a result takes you directly to the relevant content, and matching words are
highlighted"*; a screenshot captioned *"Search results highlighted in the document"*; whether they persist after
the panel closes is `[UNVERIFIED]` (§12) (<https://support.craft.do/en/organize-and-find/search/in-document>). Bear
navigates with ⌘G / ⌘⇧G between in-note results (<https://bear.app/faq/how-to-search-text-inside-notes-in-bear/>);
Apple Notes and Notion document only ⌘F (<https://support.apple.com/guide/notes/search-your-notes-not18ab658ed/mac>,
<https://www.notion.com/help/search>).

`[INFERENCE]` For UX-DR96 and AD-266: the two shipped lifetimes are "as long as the find string exists" (VS Code)
and "a flash" (Obsidian). The owner asked for the *same words marked when the note is opened*, which is the VS Code
lifetime bound to the **list's query**, not to the editor's own ⌘F string — hence a persistent `searchMarks` field
cleared when the query is cleared, when the note changes, or on Esc in the editor, translucent so live-preview
decorations stay legible, and distinct from ⌘F's decorations (which remain the user's, §2.7). `[REPO]` G2 §5: the
colour tokens exist (`--search-highlight`, `--search-highlight-foreground`), and `--mark` is spoken for by
`==highlight==`.

### 10.3 Semantic hits are labelled, not term-highlighted

`[SOURCE]` R3 §2 — Smart Connections (Obsidian, local embeddings): *"Result score … reflects semantic similarity
between the result and the current note. Exact numbers depend on the embedding model"*; *"A note that contains the
exact query text might not appear if it is not actually similar in meaning"*; no term highlighting for vector hits
(<https://raw.githubusercontent.com/brianpetro/obsidian-smart-connections/master/README.md>). Notion shows
provenance labels (`Most viewed`, `Popular this week`) beside results and Enterprise Search *"will always cite its
sources"* (<https://www.notion.com/help/search>, <https://www.notion.com/help/enterprise-search>). Apple Notes:
*"Top Hits … based on factors like matching a note's title and how recently you updated the note"*
(<https://support.apple.com/guide/notes/search-your-notes-not18ab658ed/mac>); Spotlight: *"best match at the top"*,
no per-result explanation (<https://support.apple.com/guide/mac-help/find-what-you-need-with-spotlight-mchlp1008/mac>);
Readwise exposes the engine as a toggle ("Better Search (beta)") rather than explaining per result
(<https://docs.readwise.io/reader/docs/faqs/searching>); Raycast answers AI file search in prose rather than
annotating a list (<https://manual.raycast.com/file-search>). R3's inference, adopted here: *no surveyed product
highlights terms for vector-only hits; the observed vocabulary is score, provenance label, or citation*.
`[INFERENCE]` AD-265's **matched by meaning** label on rows with no lexical hit — and no fake marks — is the
consistent form; a raw score is model-dependent (Smart Connections says so) and would mean nothing to the owner,
so a label beats a number.

### 10.4 Icon-only toolbars: target sizes, names, toggles, tooltips

`[SOURCE]` R3 §3 — **WCAG 2.2 SC 2.5.8 Target Size (Minimum), AA**: a target is *"at least 24 by 24 CSS pixels"*,
or undersized targets are spaced so that 24 px circles centred on them do not intersect; worked example: six
20×20 icon buttons with 4 px gaps pass, 20×20 with no gap fail; 16 px-tall buttons pass only if nothing is stacked
above/below; aim for 2.5.5's 44×44 for important controls
(<https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html>). **Apple HIG, Buttons**: *"a button needs a
hit region of at least 44x44 pt — in visionOS, 60x60 pt"*; the Accessibility page's table lists 44×44 pt and
28×28 pt (<https://developer.apple.com/design/human-interface-guidelines/buttons>,
<https://developer.apple.com/design/human-interface-guidelines/accessibility>). **APG Button pattern**: a button
*"has an accessible label"*; toggle buttons use `aria-pressed` and *"it is critical the label on a toggle does not
change when its state changes"* (the screen reader says "Mute toggle button pressed"); `aria-describedby` for a
longer description (<https://www.w3.org/WAI/ARIA/apg/patterns/button/>). **APG Tooltip**: `role="tooltip"`,
referenced by `aria-describedby`, appears after a small delay on focus/hover, Escape dismisses, never receives focus
(<https://www.w3.org/WAI/ARIA/apg/patterns/tooltip/>). **Sara Soueidan, *Accessible icon buttons***: `<svg
aria-hidden="true" focusable="false">` plus visually-hidden text or `aria-label`; *"you do not want to have an
accessible label in aria-label that is different from the visual text label"* (label-in-name); do not label the
svg itself (<https://www.sarasoueidan.com/blog/accessible-icon-buttons/>).

`[SOURCE]` R3 §3 — Obsidian's core search bar is icon-first: a **Match case** icon toggle inside the input (*"If
Match case icon is highlighted, that means you're currently doing a case sensitive search"*), a sliders icon
revealing *Explain search term / Collapse results / Show more context*, a sort dropdown under the field, and a
three-dots menu *"next to the number of results"*
(<https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Search.md>). Craft's Document
Search puts scope behind a filter icon with Match case / Ignore diacritics / Include partial matches / Regex
toggles (<https://support.craft.do/en/organize-and-find/search/in-document>). Apple Notes reveals suggested
searches and a scope menu beside the magnifier (<https://support.apple.com/guide/notes/search-your-notes-not18ab658ed/mac>).
Things puts tags as a **filter bar at the top of the list** — click filters, ⌘-click builds an AND set, **All**
clears, and tag order in the Tags window sets the bar's order (<https://culturedcode.com/things/support/articles/2803581/>).
Todoist, by contrast, makes filters a query language authored in a dialog
(<https://www.todoist.com/help/todoist/features/introduction-to-filters-V98wIH>).

`[REPO]` G2 §6, §1 (§2.8). keeper already meets the name rules: `aria-label` ≡ `IconHint` text verbatim (WCAG
2.5.3), icons `aria-hidden`, `aria-pressed` on the two bar toggles, no `title` beside a tooltip, a sweep test on
settings icons; the list column's floor is **240 px** and the phone tier is a separate stack at 768 px.
`[INFERENCE]` For AD-268 / UX-DR94 / UX-DR99: the notes list is desktop-only (`EXPERIENCE-NOTES.md:15`), so the
binding minimum is WCAG's 24 px, with HIG's 44 pt the bar for anything reused on the phone tier; the icon toggles
keep their *current* accessible names ("Changed by agent", "Pinned only") because APG says a toggle's label must
not change with state and because the tests assert those names; at 240 px the chips wrap (they already do,
`flex-wrap`) and the icons do not — Things' "put high-priority tags first" is the argument for tags-first order.

### 10.5 Tag chips

`[SOURCE]` R3 §4 — Carbon Tag: *dismissible* (× icon, *"typically used for filtering"*), *selectable* (toggle,
*"filter data in the context of a page"*), *operational* (*"disclose additional or overflow tags, like in a popover,
modal, or breadcrumb"*); 8 px between tags; titles under 20 characters; overflow *"truncated with an ellipsis …
full title is disclosed in a browser tooltip"*; *"Avoid having long tag titles wrap to multiple lines"*; Tab focuses
the dismiss icon (<https://carbondesignsystem.com/components/tag/usage/>). React Spectrum `TagGroup`: `onRemove`,
an optional trailing `actionLabel` (e.g. "Clear"), `aria-label` required without a visible label
(<https://react-spectrum.adobe.com/react-spectrum/TagGroup.html>). Obsidian's Tags view: click a tag to search it,
Ctrl/⌘-click to toggle it in the search term; `tag:#work` does not match `#myjob/work`; `-` negates
(<https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Tags%20view.md>). Bear: `#tag`,
`!#tag`, `-#tag`, `#*/tag` operators; a community request for multi-tag AND
(<https://bear.app/faq/how-to-use-tags-in-bear/>, <https://bear.app/faq/how-to-search-notes-in-bear/>). R3's
inference: AND is the default across Things and Obsidian's Tags view; NOT is textual, not a chip affordance; no
surveyed app shows an in-chip AND/OR switch; the `+N` overflow is Carbon's operational tag, not a notes-app
pattern.

`[REPO]` G2 §2 (§2.8). keeper's chips are already stronger than the surveyed field: three-state include/exclude
as a chip cycle with AND intersection, state carried by glyph + colour + name, contradiction unwritable on the wire,
one `CYCLE` shared by bar and tree. `[INFERENCE]` The owner's *"popatrz jak teraz tagi są wykorzystywane (to jest
najistotniejsza funkcja)"* is a constraint, not a redesign brief: AD-268 keeps `TagFilterChip` and its grammar
unchanged and moves the *other* controls to icons so the chips get the row.

### 10.6 Hiding service files, and the count line

`[SOURCE]` R3 §5 — Obsidian: Settings › Files and links › Excluded files — *"Excluded files will be hidden in
Search, Graph View, and Unlinked Mentions … and less noticeable in Quick Switcher and link suggestions"*; Search:
*"Files matching your Excluded files patterns will not appear in Search results"*
(<https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/User%20interface/Settings.md>,
<https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Search.md>,
<https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Quick%20switcher.md>). Omnisearch:
*"By default, files that are in Obsidian's … Excluded Files list are downranked in results. Enable this option to
completely hide them"*, plus folders to downrank that *"still be indexed for tags, unlike excluded files"*
(<https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/settings/settings-behavior.ts>).
VS Code: `files.exclude` hides from the Explorer; the search view *"excludes some folders by default"* via
`files.exclude` + `search.exclude` with a **Use Exclude Settings and Ignore Files** toggle in the exclude box
(<https://code.visualstudio.com/docs/configure/settings>, <https://code.visualstudio.com/docs/editing/codebasics>).
Raycast File Search: *"Hidden files are excluded"* by default, an **Include Hidden Files** setting and a **Toggle
Hidden Files** action (<https://manual.raycast.com/file-search>). Apple's official shortcut list does **not**
include a Finder hidden-files toggle; `⌘⇧.` is `[UNVERIFIED]` folklore (<https://support.apple.com/en-us/102650>;
§12). A count line of the form *"688 notes · 4 hidden"* is documented by no surveyed product (`[UNVERIFIED]` as an
established pattern, §12); the nearest are VS Code's `{x} results in {y} files`, Craft's per-document count and
Obsidian's result count beside its menu.

`[INFERENCE]` For AD-267 / UX-DR98: the precedents agree on three things keeper adopts — hidden by default, a
configurable name list in settings, a toggle in the surface — and add one keeper declines for now: Omnisearch's
"downrank instead of hide" middle ground (a DW candidate). "Hidden ≠ unindexed" is Omnisearch's own distinction
(downranked folders *"still be indexed for tags"*): a hidden `index.md` still opens by link and still matches
⌘⇧F. The `N notes · M hidden` line is keeper's own — the `M hidden` half is what tells a person their file is not
gone; `[REPO]` G2 §1: it belongs to `NotesPane`'s count slot, not the bar, and must come from Rust's counts, never
from `rows.length`.

### 10.7 Latency and debounce

`[SOURCE]` R3 §6 — Nielsen (NN/g): 0.1 s *"reacting instantaneously"* — *"the limit for users feeling that they are
directly manipulating objects in the UI"*; 1.0 s keeps *"the user's flow of thought"*; 10 s keeps attention
(<https://www.nngroup.com/articles/response-times-3-important-limits/>). Doherty threshold: system feedback within
400 ms (<https://lawsofux.com/doherty-threshold/>). Algolia Autocomplete: default no debounce; when debouncing,
*"200 ms is the preferred debounce delay. Delays of over 300 ms will start degrading the user experience"*; set the
spinner `stallThreshold` to debounce + 300 ms so nothing flashes after the last keystroke
(<https://www.algolia.com/doc/ui-libraries/autocomplete/guides/debouncing-sources/>). VS Code and Bear search as
you type; Craft as you type with a 3-char minimum and a 10 s cache; Omnisearch caps per-document matching at
100 matches / 50 ms (§10.1). A UX.SE answer on form validation gives 500 ms as a starting point and 200 ms as *"an
eye blink"* — different context, cited only for the range (<https://ux.stackexchange.com/questions/95336>).

`[REPO]` G1 §2, G2 §1. keeper's ⌘⇧F already debounces at 150 ms (`NOTE_SEARCH_DEBOUNCE_MS`,
`use-notes-search.ts:19`); the list filter has no debounce — `useNotesChanges` re-reads `notes_list` whenever the
filter state changes (`use-notes-changes.ts:56-105`); the reconciler's own budget for absorbing a write is 150 ms of
NFR-29's 1 000 ms (`notes_vault.rs:87-90`). `[INFERENCE]` For UX-DR97 and 76.2/76.6: the lexical leg should stay
under 100 ms per keystroke (FTS5 at vault scale is well inside that — R2's decision table); the vector leg, which
costs an HTTP round trip to the provider per query, is the leg to debounce at 150–200 ms and never over 300 ms, and
its state glyph should follow Algolia's rule — no spinner until debounce + 300 ms — so the field does not flicker
between "words" and "words + meaning" on every character.

---

## 11. Settings and persistence seams

All of §11 is `[REPO]` G3 unless marked.

### 11.1 Declaring a key

`[REPO]` G3 §1. One `KeySpec` row in `KEYS` (`config/keys.rs:331`; struct `:301-345`; a verbatim row at
`:395-404`): `key`, `family`, `scope`, `settable`, `shape`, `default` (*"the stored string a reader falls back to
when the row is absent"*), `summary`, `example`. `Shape::coerce` (`:186-260`) is the single place a TOML value
becomes the stored string. Reads are typed getters over `registry::get_setting` (`registry.rs:208-211`) — the
boolean model is `get_incognito_global`: `Ok(get_setting(…)?.as_deref() == Some("1"))` (`:532-534`); writes are
`registry::set_setting` (`:230-233`). The shell exposes one `#[tauri::command]` per get/set pair, registered in
`lib.rs`'s `invoke_handler` (`ipc.rs:4211`, `:4219`; `lib.rs:1289-1295`); React has no generic settings hook — one
typed wrapper in `client.ts` per pair, optimistic set with revert on failure (`settings-dialog.tsx:420-423`,
`:473-478`). `FileControlled` renders beside a control a TOML layer overrides (`config-source-section.tsx:87-90`;
`useSettingOverride`, `config-layers.ts:96-99`). Three boolean spellings exist historically; **new keys are
`Flag01`** (`keys.rs:110-122`).

### 11.2 The list-of-strings precedent — `notes.service_file_names`

`[REPO]` G3 §3. `ui.recovered_sessions_acknowledged`: `Shape::Json`, `Scope::SessionState`, `Settable::Never`,
default `"[]"` (`keys.rs:802-813`); the getter parses `serde_json::from_str::<Vec<String>>` and degrades to empty,
never errors, on a corrupt value (`registry.rs:670-683`, test `:4146-4161`); the setter is read-modify-write
(`:708-714`). One trap: `Shape::Json`'s `coerce` accepts only a TOML **string** (`keys.rs:201-204`) — a file author
would write the list as an embedded JSON string; a bare TOML array is refused. `[INFERENCE]` AD-267's
`notes.service_file_names` (default `["index.md","agents.md","claude.md","log.md"]`, matched case-insensitively on
the file name in any folder) follows this row with `Scope::UserGlobal`; whether it is `Settable::AnyLayer` (so a
vault's `keeper.toml` can add names — awkward JSON-in-a-string) or `Settable::Never` (settings UI only) is the one
open choice G3 leaves to the epic, and the recipe in G3 §"Recipe" (b) names every file it touches.

### 11.3 The boolean precedent — `notes.hide_service_files`

`[REPO]` G3 §4. The most recent boolean-with-default is epic 75's `notes.capture_placement.<key>`
(`Scope::SessionState`, `Settable::Never`, `keys.rs:532-545`; `registry.rs:1450-1453`), and the module doc's rule
for that scope (`keys.rs:31-36`): SessionState keys are *"state keeper owns and rewrites … in this table rather than
left out of it"* so that "deliberately not settable" and "somebody forgot to classify it" cannot look the same.
The bar's `pinnedOnly` / `agentOnly` have **no** persistence today (zustand fields, `notes-filters.ts:197-198`).
`[INFERENCE]` AD-267's `notes.hide_service_files: bool` default `true` — session-state scope, persisted like a bar
toggle ought to be — is the `capture_placement` shape with `Flag01` and `default: "1"`, read as `!= Some("0")` so an
absent row is `true` (G3 §"Recipe" (a)). If the epic instead reads it as a person's preference about the surface,
it is `UserGlobal`/`AnyLayer` like `bots.wake_enabled`; G3 states both readings and the AD picks the first.

### 11.4 The two homes for a setting — why these two are global keys and not vault fields

`[REPO]` G3 §2, §"Competing patterns". Per-vault facts — `commit_idle_ms`, journal template, capture
template/tag, `subfolder` — are **not** settings keys; they are `NotesConfig` fields on `SyncProfile.notes`
(`keeper-sync/src/profile/mod.rs:240-330`) persisted as JSON in sync.db's `profiles` table (`db.rs:74-77`) via
`notes_vault_settings_save` (`notes_ipc.rs:978-996`), with `Option` per field meaning "not expressed"
(`vm.rs:1243-1256`). The `KEYS` doc forbids a folder-scoped settings key (`keys.rs:39-44`): *"Everything a folder
decides about itself … lives in that folder's `SyncProfile` fields."* G3's counter-argument, stated so it is not
re-discovered: file names could be read as per-vault facts. `[INFERENCE]` AD-267 puts both in the global table
because the *hiding* is a fact about how this person reads any list, and the four default names are agent
conventions that follow the person across vaults, not the vault; a vault that needs an extra name can add it in
the layer file if `Settable::AnyLayer` is chosen (§11.2), which is the folder-layer mechanism `keys.rs` already has
(`config/mod.rs:286-288`).

### 11.5 Where the controls go, and `docs/settings-keys.md`

`[REPO]` G3 §5, §6. `SettingsBody` is a flat ordered list of section components shared by dialog and pane
(`settings-dialog.tsx:107-131`); the Notes section is `CaptureSettingsSection` at `:234`, which today renders only
vault-scoped controls keyed on `vault.id`; a global row belongs in a Notes section body around it, following the
Switch-row pattern `<FileControlled settingKey=…/> + <Switch …/>` (`:500-504`, `:859-863`) and the on-blur save
convention for text (`capture-settings.tsx:257-262`); string literals are exported consts so tests cannot disagree
(`:46-66`). `docs/settings-keys.md` is regenerated with `cargo test -p keeper-core --lib
config::keys::tests::docs::regenerate -- --ignored` and pinned by `keys.rs:1672-1682` — never hand-edited.

### 11.6 Where the embedding model choice lives

`[REPO]` G4 §4; G3 §1. Provider rows are records in `keeper.db` (`bot_providers`, `bots/store.rs:76-99`) with a
kind, a base URL and a keychain credential (D-4, §2.10); model discovery is per provider (§2.10). `[INFERENCE]`
AD-264's "a new setting names `(provider_id, model)` from the user's provider rows" is a settings key whose value
references a provider row — the closest existing shape is a `Text` key holding an id (`notes.active_vault`,
`Scope::MachineLocal`, `keys.rs:509-517`), and machine-local is the right scope: the provider is a URL reachable
from *this* machine. A dangling `provider_id` (row deleted) must read as "no model set", i.e. lexical-only with
the bar's sentence, never an error — the same degrade-not-fail rule the JSON-list getter follows (§11.2).

### 11.7 `search.db` beside `index.json`

`[REPO]` G1 §5, verified this pass. `<vault>/.keeper/` (`KEEPER_DIR`, `notes_vault.rs:70-72`: *"keeper's own
per-vault cache directory. Tier-0 excluded by `keeper-sync`, so nothing in it is ever staged, committed or listed
as pending (FR-121)"*) holds `index.json`, the trash and the folder's own `keeper.toml`/settings; the walk refuses
it by name (`:1167-1169`, `:1122-1126`: *"the tier-0 carve-out is about what reaches a *commit*"*); a torn temp
file there is also tier-0 (`:1709-1712`). `Vault.id` is the sync profile id — *"A vault has no identity of its own
(AD-54)"* (`:134-141`); `IndexCache` carries `vault_id` and a mismatch discards (`index.rs:1215`;
`notes_vault.rs:1229-1233`). `notes_index_rebuild` deletes only `index.json` (`notes_vault.rs:510-518`).
`[INFERENCE]` AD-261's `search.db` inherits all four properties by living in the same directory: never synced,
never committed, keyed by the same `vault_id` with the same foreign-cache discard, and deleted by the same
*Rebuild index* command (which must now remove both files — the one code path that changes). Nothing in it is
underivable from the files and the user's provider; deleting it costs a rescan and a backfill, never data.

---

## 12. Unverified inventory

`[UNVERIFIED]` — every item below was looked for and not found in the sources read on 2026-09-19. None may be
repeated as fact. What was tried is stated per item.

**From R1 (pi-knowledge):**

1. `multilingual-e5-small`'s output dimension (384) — a standard model fact, not stated in the package, which stores
   `embedding_dimension` dynamically; the model card (R2 §3) states 384 and is the source used in §6.3.
2. FTS5 behaviour differences between better-sqlite3's and rusqlite's SQLite builds — not observable from the
   package.
3. `.pi/skills/search-docs.md` — read for completeness; only agent-facing mode-selection guidance, no tunables.

**From R2 (crates, FTS5, models, wire):**

4. Final on-disk executable growth from statically linking ONNX Runtime via `ort` — only the 9.4–10.4 MB compressed
   archive sizes were measured; "tens of MB" is inference (§5.2).
5. Whether tantivy's `AsciiFoldingFilter` folds `ł` → `l` — Lucene's does; the Rust port was not inspected (§5.1).
6. tantivy 0.26.2's BM25 `k1`/`b` defaults, and whether `tantivy-stemmers` 0.4.0 compiles against tantivy 0.26's
   tokenizer-api 0.7 (§5.1).
7. Polish retrieval quality of `bge-m3`, `embeddinggemma-300m` and `nomic-embed-text-v2-moe` — absent from PL-MTEB
   v2 Table 2 (§6.3).
8. Retrieval quality of Snowball's new Polish stemmer — no evaluation published alongside the algorithm (§5.4).
9. Any hard cap on `/api/embed` / `/v1/embeddings` input-array length in Ollama — docs silent; community threads
   cite memory-bound failures (§6.4).
10. CPU throughput of candle vs ONNX Runtime for a 12-layer 384-d encoder on Apple Silicon (§5.3).
11. Whether FTS5 `highlight()` markers can be split by a multi-byte tokenizer boundary — docs imply markers land on
    token byte boundaries; not tested (moot under AD-263, §4.5).
12. rusqlite exposes no FTS5 custom-tokenizer / aux-function wrapper — inferred from its feature list, not confirmed
    by an issue (§4.2).
13. Availability of `multilingual-e5-*`, `snowflake-arctic-embed-m-v2.0` and `mmlw-roberta-base` on the Ollama
    library — R2 fetched pages only for `nomic-embed-text`, `embeddinggemma`, `bge-m3`, `all-minilm`,
    `mxbai-embed-large`, `qwen3-embedding` (§6.3).
14. Whether Ollama's `/v1/embeddings` returns unit-length vectors — stated only for `/api/embed` (§6.2); AD-264
    normalises in Rust regardless.
15. Hermes Agent embeddings support — no embeddings endpoint in the route table keeper's earlier research read
    (`[REPO]` G4 §"Where could embeddings come from"); AD-264's quirk row and refusal sentence exist for this.

**From R3 (UX):**

16. Bear's in-note match highlight colour/persistence and how it clears — only "highlight the attachment" and ⌘G
    navigation are documented.
17. Apple Notes' visual treatment of matches inside an opened note and how it clears — not in the Notes User Guide
    search page.
18. Notion's in-page find highlight behaviour — help page says only ⌘/Ctrl+F.
19. Craft: whether in-document highlights persist after the Document Search panel closes — not stated.
20. Readwise Reader's "Better Search" ranking internals — described only as server-side and "more accurate".
21. Raycast Root Search / Quick AI showing any "why this matched" label — nothing in the manual page read.
22. Apple Spotlight (macOS Tahoe/27) semantic-result labelling — the Spotlight guide says only "best match at the
    top".
23. macOS Finder `⌘⇧.` hidden-files shortcut — absent from Apple's official shortcut list
    (<https://support.apple.com/en-us/102650>); folklore until verified.
24. A shipped `N notes · M hidden` count line — no product documentation found; keeper's line is its own (§10.6).
25. Smart Connections and Omnisearch UI specifics beyond README/source (e.g. the exact CSS of
    `omnisearch-default-highlight`) — not read.
26. Material Design 3 chip guidance — page is JS-rendered and was not readable in the session; Carbon was used
    instead (§10.5).

**From the repository grounding:** none — G1 records *"UNVERIFIED: none — every claim above is grounded in the
repo"*; the only caveat is the line drift §1.3 lists.

---

## Sources

All external sources were read on **2026-09-19**. Grouped by the digest that read them; repository grounding with
`path:line` for every `[REPO]` claim lives in G1–G5 (front-matter) and was spot-checked as §1.3 describes.

**Reference implementation (R1).** pi-knowledge 0.8.1, MIT — installed package `/home/dev/.omp/plugins/node_modules/pi-knowledge/`
(`dist/src/indexer/chunker.js`, `dist/src/storage/sqlite.js`, `dist/src/search/{bm25,query,fusion,ranking,tuning,vector}.js`,
`dist/src/embedding/{provider,model-worker,vectors}.js`, `dist/src/engine.js`, `README.md`, `CHANGELOG.md`,
`docs/configuration.md`); repository <https://github.com/nczz/pi-knowledge>.

**SQLite / rusqlite (R2 §1).**
- SQLite, *FTS5 Extension* — <https://sqlite.org/fts5.html> (§2.1, §3.1, §3.3, §4.3.1–§4.3.4, §4.4.3, §5.1.1–§5.1.3, §6.11, §7, Appendix A).
- SQLite source, `fts5_unicode2.c` — <https://raw.githubusercontent.com/sqlite/sqlite/master/ext/fts5/fts5_unicode2.c>.
- SQLite source, `fts5_aux.c` — <https://raw.githubusercontent.com/sqlite/sqlite/master/ext/fts5/fts5_aux.c>.
- rusqlite, `libsqlite3-sys/build.rs` — <https://raw.githubusercontent.com/rusqlite/rusqlite/master/libsqlite3-sys/build.rs>.
- rusqlite, `Cargo.toml` — <https://raw.githubusercontent.com/rusqlite/rusqlite/master/Cargo.toml>.

**tantivy and stemmers (R2 §2, §8).**
- crates.io API, `tantivy` — <https://crates.io/api/v1/crates/tantivy>.
- tantivy main `Cargo.toml` — <https://raw.githubusercontent.com/quickwit-oss/tantivy/main/Cargo.toml>.
- tantivy tokenizer docs — <https://docs.rs/tantivy/latest/tantivy/tokenizer/index.html>.
- tantivy snippet docs — <https://docs.rs/tantivy/latest/tantivy/snippet/index.html>.
- rust-stemmers README — <https://raw.githubusercontent.com/CurrySoftware/rust-stemmers/master/README.md>.
- tantivy-stemmers — <https://docs.rs/crate/tantivy-stemmers/latest>.
- Snowball algorithms — <https://github.com/snowballstem/snowball/tree/master/algorithms>; <https://snowballstem.org/algorithms/>; <https://raw.githubusercontent.com/snowballstem/snowball/master/algorithms/polish.sbl>.
- frostem — <https://docs.rs/crate/frostem/latest>; <https://docs.rs/crate/frostem/latest/features>.

**Local embedding runtimes (R2 §3).**
- fastembed-rs — <https://raw.githubusercontent.com/Anush008/fastembed-rs/main/Cargo.toml>; <https://github.com/Anush008/fastembed-rs> (`src/common.rs`, `src/text_embedding/init.rs`, `src/models/text_embedding.rs`).
- ort — <https://raw.githubusercontent.com/pykeio/ort/main/ort-sys/build/download/dist.tsv>; <https://ort.pyke.io/misc/prebuilt-binaries>; <https://ort.pyke.io/setup/cargo-features>; <https://ort.pyke.io/setup/linking>.
- ONNX Runtime licence — <https://raw.githubusercontent.com/microsoft/onnxruntime/main/LICENSE>; custom/minimal build — <https://onnxruntime.ai/docs/build/custom.html>.
- candle README — <https://raw.githubusercontent.com/huggingface/candle/main/README.md>.

**Models and benchmarks (R2 §3).**
- Hugging Face model cards — <https://huggingface.co/intfloat/multilingual-e5-small>; <https://huggingface.co/BAAI/bge-m3>; <https://huggingface.co/nomic-ai/nomic-embed-text-v1.5>.
- Hugging Face model API — <https://huggingface.co/api/models/google/embeddinggemma-300m>; <https://huggingface.co/api/models/onnx-community/embeddinggemma-300m-ONNX>; `https://huggingface.co/api/models/<repo>?blobs=true` for file sizes.
- PL-MTEB v2 — <https://arxiv.org/pdf/2405.10138>; leaderboard <https://huggingface.co/spaces/PL-MTEB/leaderboard>.

**Remote embeddings wire (R2 §4).**
- Ollama API — <https://raw.githubusercontent.com/ollama/ollama/main/docs/api.md>; <https://docs.ollama.com/capabilities/embeddings>; <https://docs.ollama.com/api/openai-compatibility>.
- Ollama library pages — `https://ollama.com/library/<name>` for `nomic-embed-text`, `embeddinggemma`, `bge-m3`, `all-minilm`, `mxbai-embed-large`, `qwen3-embedding`.
- Ollama issues — <https://github.com/ollama/ollama/issues/13340>; <https://github.com/ollama/ollama/issues/6262>.
- OpenAI Embeddings reference — <https://developers.openai.com/api/reference/resources/embeddings/methods/create>.

**Vector search (R2 §5).**
- sqlite-vec — <https://raw.githubusercontent.com/asg017/sqlite-vec/main/README.md>; <https://alexgarcia.xyz/sqlite-vec/rust.html>; <https://alexgarcia.xyz/sqlite-vec/features/vec0.html>; <https://alexgarcia.xyz/sqlite-vec/features/knn.html>; <https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html>.
- usearch README — <https://raw.githubusercontent.com/unum-cloud/usearch/main/README.md>.
- hnswlib-rs README — <https://raw.githubusercontent.com/jean-pierreBoth/hnswlib-rs/master/README.md>.

**Fusion (R2 §6).**
- Cormack, Clarke, Büttcher, *Reciprocal Rank Fusion outperforms Condorcet and individual Rank Learning Methods*, SIGIR 2009 — <https://cormack.uwaterloo.ca/cormacksigir09-rrf.pdf>.
- Bruch, Gai, Ingber, *An Analysis of Fusion Functions for Hybrid Retrieval*, arXiv 2210.11934v2 — <https://arxiv.org/pdf/2210.11934>.

**Chunking (R2 §7).**
- Chroma, *Evaluating Chunking Strategies for Retrieval* — <https://www.trychroma.com/research/evaluating-chunking>.
- Pinecone, *Chunking Strategies* — <https://www.pinecone.io/learn/chunking-strategies/>.
- text-splitter — <https://raw.githubusercontent.com/benbrandt/text-splitter/main/README.md>; <https://docs.rs/text-splitter/latest/text_splitter/struct.ChunkConfig.html>.

**Search UX (R3).**
- Omnisearch source — <https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/globals.ts>; <https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/tools/text-processing.ts>; <https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/settings/settings-ui.ts>; <https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/settings/settings-behavior.ts>.
- Obsidian Help — <https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Search.md>; <https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Tags%20view.md>; <https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/User%20interface/Settings.md>; <https://raw.githubusercontent.com/obsidianmd/obsidian-help/master/en/Plugins/Quick%20switcher.md>; forum <https://forum.obsidian.md/t/flashing-elements-color/80720>.
- Smart Connections README — <https://raw.githubusercontent.com/brianpetro/obsidian-smart-connections/master/README.md>.
- Craft Help — <https://support.craft.do/en/organize-and-find/search>; <https://support.craft.do/en/organize-and-find/search/in-document>.
- Bear FAQ — <https://bear.app/faq/how-to-search-notes-in-bear/>; <https://bear.app/faq/how-to-search-text-inside-notes-in-bear/>; <https://bear.app/faq/how-to-use-tags-in-bear/>.
- Apple — Notes User Guide <https://support.apple.com/guide/notes/search-your-notes-not18ab658ed/mac>; Spotlight <https://support.apple.com/guide/mac-help/find-what-you-need-with-spotlight-mchlp1008/mac>; keyboard shortcuts <https://support.apple.com/en-us/102650>; HIG Buttons <https://developer.apple.com/design/human-interface-guidelines/buttons>; HIG Accessibility <https://developer.apple.com/design/human-interface-guidelines/accessibility>.
- Notion Help — <https://www.notion.com/help/search>; <https://www.notion.com/help/enterprise-search>.
- VS Code — <https://code.visualstudio.com/api/references/theme-color>; <https://code.visualstudio.com/docs/editing/codebasics>; <https://code.visualstudio.com/docs/configure/settings>.
- Logseq issues — <https://github.com/logseq/logseq/issues/5844>; <https://github.com/logseq/logseq/issues/8881>.
- Readwise Reader docs — <https://docs.readwise.io/reader/docs/faqs/searching>.
- Raycast manual — <https://manual.raycast.com/file-search>.
- W3C — WCAG 2.2 Understanding SC 2.5.8 <https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html>; APG Button <https://www.w3.org/WAI/ARIA/apg/patterns/button/>; APG Tooltip <https://www.w3.org/WAI/ARIA/apg/patterns/tooltip/>.
- Sara Soueidan, *Accessible Icon Buttons* — <https://www.sarasoueidan.com/blog/accessible-icon-buttons/>.
- Carbon Design System, Tag usage — <https://carbondesignsystem.com/components/tag/usage/>.
- React Spectrum, TagGroup — <https://react-spectrum.adobe.com/react-spectrum/TagGroup.html>.
- Cultured Code, Things tags — <https://culturedcode.com/things/support/articles/2803581/>.
- Todoist, Introduction to filters — <https://www.todoist.com/help/todoist/features/introduction-to-filters-V98wIH>.
- Nielsen Norman Group, *Response Times: The 3 Important Limits* — <https://www.nngroup.com/articles/response-times-3-important-limits/>.
- Laws of UX, Doherty Threshold — <https://lawsofux.com/doherty-threshold/>.
- Algolia, *Debouncing sources* — <https://www.algolia.com/doc/ui-libraries/autocomplete/guides/debouncing-sources/>.
- UX Stack Exchange 95336 — <https://ux.stackexchange.com/questions/95336>.

**Repository (G1–G5), read at `origin/main` tip `a925c4b`.** `src-tauri/crates/keeper-core/src/notes/{index,search,query,snippet,sort,counts,vm}.rs`;
`keeper-core/src/archive/{db,fts,recordings_fts,ingest,mod}.rs`; `keeper-core/src/bots/{http,chat,quirks,discover,store,mod}.rs`;
`keeper-core/src/{egress,registry}.rs`; `keeper-core/src/config/{keys,mod}.rs`; `keeper-core/src/sessions/search.rs`;
`src-tauri/crates/keeper/src/{notes_vault,notes_ipc,ipc,lib}.rs`; `src-tauri/crates/keeper-sync/src/{profile/mod,db}.rs`;
`src-tauri/{Cargo.toml,Cargo.lock,deny.toml}`; `src/components/notes/{note-filter-bar,note-row,notes-pane,tag-combobox,tag-tree,note-editor,capture-settings}.tsx`,
`src/components/notes/editor/{live-preview.ts,find-panel.tsx}`, `src/components/layout/{surface-column,fold-strip}.tsx`,
`src/components/settings/{settings-dialog.tsx,config-source-section.tsx,icon-hints.test.ts}`, `src/components/ui/tooltip.tsx`;
`src/lib/stores/{notes-filters,notes-list,config-layers,notes-rail-fold}.ts`, `src/lib/{column-widths,count-label}.ts`, `src/lib/ipc/{client.ts,gen/*.ts}`;
`src/hooks/{use-notes-search,use-notes-changes,use-shell-layout}.ts`; `src/index.css`; `dev/probe/main.tsx`; `package.json`;
`docs/{notes,decisions,egress,settings-keys}.md`; `_bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/EXPERIENCE-NOTES.md`;
`_bmad-output/planning-artifacts/research-ai-chat-2026-09-02.md` (header shape and grade legend).

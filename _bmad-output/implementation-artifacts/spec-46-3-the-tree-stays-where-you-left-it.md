# Spec 46.3 — The tree stays where you left it

story: 46.3
epic: 46 — The file is the setting
status: implemented, gated on nothing (frontend only)
author: W1Tree

## The defect

`FilesPane` held its folder expansion in `useState`. `AppShell` renders the pane
conditionally on the primary view (`app-shell.tsx:294-297`, under
`primaryView === "files"`), so looking at Notes for two seconds unmounted the
tree and discarded the set. The file's own doc comment promised the opposite —
*"Collapsing keeps what was loaded, so re-opening a branch is free"* — which was
true within one mount and false across every surface switch.

## What was built

| Artifact | Kind | Role |
| --- | --- | --- |
| `src/lib/stores/files-tree.ts` | new | The store, the cookie, the pure reader and builder, the bounds, the hydrate |
| `src/lib/stores/files-tree.test.ts` | new | 28 tests over the pure surface and the store |
| `src/components/layout/files-pane.tsx` | edited | Expansion read from the store; restore effect; `nodeKey` moved out |
| `src/components/layout/app-shell.tsx` | edited | `hydrateFilesTree(document.cookie)`, beside the existing two |
| `src/components/layout/files-pane.test.tsx` | edited | Suite reset + 9 tests for the restore |
| `src/components/layout/app-shell.test.tsx` | edited | Suite reset + the DW-172 shell-level assertion |

The shape is `sidebar-fold.ts`'s exactly: one `keeper_`-prefixed cookie, a
year-long `max-age`, a pure reader, a pure cookie-string builder, a best-effort
`persist` that survives a document refusing cookies, and an idempotent
`hydrateFilesTree` called once from `AppShell`.

The *encoding* is `panels.ts`'s rather than the fold's, and that is a deliberate
divergence: the fold packs `key:value|key:value`, which works because its keys
are a closed set of identifiers. A node key holds a **file path**, and `|`, `:`
and `;` are all legal in one. So it is versioned JSON behind `encodeURIComponent`
with a `v: 1` gate, and there is a test that round-trips paths containing each of
those three characters.

`nodeKey` moved from `files-pane.tsx` into the store. The key format is now a
persisted format read back by later builds, so the vocabulary and the thing that
persists it live in one file and the `\u0000` separator has exactly one
definition. `refresh`'s hand-rolled `indexOf("\u0000")` split became
`nodeKeyProfile`/`nodeKeySubpath` for the same reason.

## I/O matrix

### `readFilesTree(cookie: string) -> Set<string>` — pure, total

| Input | Output | Why |
| --- | --- | --- |
| `keeper_files_tree={"v":1,"p":{"P":["","Notes"]}}` in a jar with foreign cookies | `{P\0"", P\0Notes}` | The ordinary case; the jar is shared |
| absent | `{}` | Never opened anything |
| `{`, `null`, `[]`, `"a"`, `7`, `{"v":1}`, `{"p":{}}` | `{}` | Malformed. A throw here is a white window — this runs before paint |
| `{"v":2,...}` | `{}` | Unknown version discarded, never guessed at |
| subpath `/etc/passwd`, `../../x`, `a/../../b`, `\\server\share`, `C:/W`, `c:\W` | dropped, siblings kept | A cookie is user-editable and the subpath goes to `sync_browse`. Outer of two gates; AD-65 |
| subpath `7`, `null`, `{}` / profile `""` / profile value not an array | dropped, siblings kept | Structural |
| > 32 keys | shallowest 32 | A pasted jar must not buy 5 000 browse calls |

### `filesTreeCookie(expanded) -> string` — pure

| Input | Output |
| --- | --- |
| empty set | `keeper_files_tree=; path=/; max-age=0; samesite=lax` — forget, do not store an empty tree |
| ≤ 32 keys, ordinary paths | `keeper_files_tree=<json>; path=/; max-age=31536000; samesite=lax` |
| > 32 keys | shallowest 32 written, `console.info` naming kept/total |
| encoded value > 2000 bytes | deepest dropped until it fits, `console.info` |
| a single key over budget | forget-cookie (nothing) rather than bytes the browser discards silently |
| same set, different insertion order | byte-identical output |

### Store

| Call | Effect |
| --- | --- |
| `setNodeOpen(key, true/false)` | mutates + persists; no-op (same object identity returned) when already in that state |
| `retainProfiles(ids)` | drops keys whose profile is absent, persists; no-op when nothing changed |
| `hydrateFilesTree(cookie)` | once per document; a second call cannot re-open what the user has since shut |
| `reachableNodeKeys(set)` | the prefix-closed subset — keys every one of whose ancestors is open |

## The three things the story asked me to get right

**1. Expansion only.** `listings` and `failures` stay component state and die with
the mount. A restored listing is a claim about a disk keeper has not read since
the app closed: it would render a deleted file from a volume that may not be
attached, with no way for the viewer to tell. There is a test
(*"re-reads the folder rather than restoring what it held"*) that asserts the
second mount issues a fresh `sync_browse`.

**2. Stale profile keys drop silently.** Not in the reader — the reader is pure
and the set of profiles that exist is a fact only `sync_profiles` knows. So
`retainProfiles(ids)` runs from the pane's restore effect. Silent: a folder
somebody stopped syncing months ago is not news and names nothing the reader can
act on. Tested at both levels, including that nothing on screen mentions the
gone profile and that its key leaves the cookie (otherwise it is a key nothing
can ever clear).

**3. The N browse calls are the honest cost, and they are bounded twice.**

- **Count: 32 keys, and the count is the rule that normally binds.** The cost
  being bounded is IPC calls at startup against folders that may be a pendrive
  or a network mount — bytes are a poor proxy for those. 32 is well past what
  anyone reaches by clicking chevrons. A session may hold more open than 32;
  only the *persisted* set is bounded, so exceeding it never collapses a folder
  under the person using it.
- **Bytes: 2000, as a backstop** for paths long enough that 32 do not fit.
  Deliberately under `PANELS_COOKIE_BUDGET`'s 3500: the panel arrangement is what
  you were working on, this is which folders were open, so under jar pressure
  this is the one that should give way first. There is a test asserting that 32
  keys of ordinary length fit *well inside* 2000 — if it ever fails, the byte
  rule has quietly become the everyday rule, which is the state the two constants
  were chosen to avoid.
- **Both drop the deepest first**, ties broken lexicographically. A deep node is
  only visible when its ancestors are open, so dropping a shallow key while
  keeping its descendant would restore an expansion that renders nothing *and
  still pays for a browse*. Dropping from the bottom keeps the part of the tree
  nearest what a person sees. Lexicographic ties make the write deterministic —
  a cookie that reordered itself on every toggle would defeat any attempt to
  diff it.
- **Nothing truncates silently.** Both bounds `console.info` how many of how many
  were kept, following `panelsCookie`. A cookie that silently truncates is worse
  than one that stores less on purpose.

And one bound the story did not ask for: the restore loads only
`reachableNodeKeys`. Collapsing a folder deliberately keeps what was open inside
it, so the set can hold a node nothing renders; browsing for one at mount is an
IPC call for a row with nowhere to go. Unreachable keys stay remembered and cost
nothing until their parent opens again.

## A defect found while building this, and fixed

The shell-level test (DW-172's idiom) failed on first run with an empty store,
and the cause was not the test.

`FilesPane` renders the same empty surface whether `sync_profiles` **answered
with no profiles** or **failed**. Its catch sets `setProfiles([])` — an honest
rendering. But the restore effect read that as *"every profile has been
deleted"* and called `retainProfiles([])`, which **permanently erased the user's
whole remembered expansion** because the sync engine was briefly unreachable.
That is a worse bug than the one this story fixes, and it would only have shown
up on a real machine with a real failure.

Fixed with an `answered` ref: the list is only acted on when Rust genuinely
answered. Set in both success paths (mount and Refresh), so a restore stays armed
until a real list arrives rather than being spent on a failed one. Three tests
cover it, and the pair of mutations M17/M18 pin both halves.

## Edge cases

| Case | Behaviour |
| --- | --- |
| `sync_profiles` fails | Nothing forgotten, nothing browsed; the restore stays armed for the next real list |
| Refresh succeeds after a failed first call | Restore runs then: loads fire, stale profiles drop |
| Profile paused | Keys kept (pausing is not deleting), not browsed (the pane does not list a paused folder) |
| Profile re-enabled | Reaching Sync unmounts Files, so the next mount restores it |
| Parent shut, child still remembered | Child kept, not browsed; comes back when the parent re-opens |
| Cookie written by a later build (`v: 2`) | Tree opens shut; nothing guessed |
| Hand-edited cookie with a hostile path | That entry dropped, siblings kept; Rust re-derives and contains again |
| Document refuses cookies | `persist` swallows it — the user loses the restore, never the click |
| Everything shut | Cookie forgotten (`max-age=0`), jar gets its bytes back |
| React double-invoked dev effects | `hydrateFilesTree` and the restore effect are both idempotent |

**Ordering that is load-bearing and is written down in the code:** child effects
run before parent effects, so `FilesPane`'s mount effect fires before
`AppShell`'s `hydrateFilesTree`. The restore is keyed on `profiles`, which is
non-null only after `syncProfiles()` settles — a microtask after every mount
effect in the tree. The store is therefore already hydrated when the pane reads
it, without the pane knowing anything about the shell.

## Mutation table

19 mutations, 19 caught. Each was applied, the targeted suite run, the file
restored from an in-memory backup in a `finally`, and the restore verified by
`git diff` and a sentinel grep — not from memory. One earlier sweep was
interrupted mid-run and did leave a mutant in an untracked file; the sentinel
grep caught it, which is why it is there.

| # | Mutation | Caught by |
| --- | --- | --- |
| M1 | expansion back to `useState` (the pre-fix code) | 7 tests, incl. *comes back open after the surface unmounts it* |
| M2 | restore skips `retainProfiles` | drops a remembered folder whose profile is gone |
| M3 | restore ignores reachability | asks only for the remembered folders that will be on screen |
| M4 | restore browses paused profiles | does not browse a paused folder, and does not forget it either |
| M5 | restore issues no loads | 6 tests |
| M6 | `retainProfiles` keeps everything | forgets every folder under a profile that no longer exists |
| M7 | no key-count limit on write | keeps the limit and drops the deepest; says so rather than truncating silently |
| M8 | no byte budget | never writes past the byte budget |
| M9 | drop shallowest instead of deepest | keeps the limit and drops the deepest; never writes past the byte budget |
| M10 | truncate without saying so | says so rather than truncating silently |
| M11 | version gate accepts any `v` | discards a version this build does not know |
| M12 | no path gate on read | drops an entry that is not a relative path |
| M13 | no read-side bound | bounds a hand-edited cookie |
| M14 | `hydrate` not idempotent | restores a remembered expansion, once |
| M15 | store never persists | opens a folder and writes the whole expansion out; shuts a folder… |
| M16 | forget-cookie becomes an empty tree | forgets the cookie when nothing is open |
| M17 | a failed profile list forgets the tree | 4 tests, incl. the shell-level one |
| M18 | a late list never becomes authoritative | waits for a real list before deciding a profile is gone |
| M19 | `AppShell` never calls `hydrateFilesTree` | restores the folders the Files tree last had open |

**Two mutations exposed weak tests before they exposed weak code, and both tests
were rewritten:**

- M7 initially **survived**. *"keeps the limit and drops the deepest"* asserted
  through `readFilesTree`, which applies the same 32-key bound — so the reader
  masked the writer entirely. The test now decodes the written bytes directly
  (`carried()`), and the helper's doc says why.
- M18 initially **survived**. *"restores on the first list it really gets"* was
  satisfied by `refresh`'s own re-read loop, which fires whatever the flag says,
  so the loads coming back proved nothing. A second test now asserts the only
  observable that `refresh` does not provide for free: the stale-profile drop.

## Deliberately NOT done

- **No `collapseAll`, no "expand to here", no restore-on-first-run defaults.**
  Not asked for, and each is a product decision rather than a defect fix.
- **`listings`/`failures` not persisted.** Argued above; this is the load-bearing
  omission, not an oversight.
- **The set is not pruned to prefix-closed at write time.** Keeping a
  descendant of a shut parent is what makes re-opening a branch come back the way
  you left it. It is excluded from the *restore loads* instead, which costs
  nothing and preserves the nicety.
- **No repair of `panels.ts`'s `while (… && kept.length > 1)`**, which can emit
  an over-budget cookie for a single large panel. Real, adjacent, someone else's
  story; this module's loop goes to zero and returns the forget-cookie instead.
- **`AppShell` did not grow a shared "hydrate everything" helper.** There are now
  three near-identical effects. Rule of two says the third is the moment to look;
  the reason not to is that each carries a different paragraph explaining why
  that particular restore cannot live in its component, and folding them into a
  loop would delete the three explanations to save six lines. Recorded here so
  the next reader sees it was considered.
- **No `localStorage`.** Repo-wide refusal (`column-widths.ts`), honoured.
- **No new dependency.** `zustand` and `zustand/vanilla` only, both already used
  by every store in this directory.

## What I could not verify here, and why

Nothing in this story is Rust, and nothing needs the Tauri shell. `keeper` was
not compiled — no need, and it does not build on this host anyway. The gaps are
about the real browser and the real machine, not about a crate:

1. **jsdom's cookie jar is not a browser's.** Every bound is asserted against the
   *encoded string length*, which is the right quantity, but no test here proves
   Chromium/WebKit actually stores a 2000-byte cookie beside `keeper_panels`,
   `keeper_column_widths` and `keeper_sidebar_fold`. The budget is argued from
   `panels.ts`'s stated ~4096 per-cookie limit.
2. **The real cost of N restored browses** — 32 `sync_browse` calls at mount
   against a folder on removable media or a network mount — is reasoned about,
   not measured.
3. **The child-before-parent effect ordering** that makes hydration land before
   the restore reads it is asserted at the shell only in its *outcome*
   (the store is populated after `render(<AppShell/>)`), not as a timing
   property under the real event loop.

### Ordered gate checks (macOS, on a real build)

1. `bun run lint && bun run typecheck && bun run test` — whole-suite, Main's pass.
   (`tsc --noEmit` is already clean here; lint and format not run per instruction.)
2. Launch the app with at least two sync profiles. Open **Files**. Expand a
   profile root, a folder inside it, and a folder inside that.
3. Switch to **Notes**, then back to **Files**. → the same three folders are
   open. *(This is the reported defect.)*
4. Quit and relaunch. → still open, and each open folder issued one browse.
5. In dev tools, confirm one `keeper_files_tree` cookie exists, that it is under
   2 kB, and that `keeper_panels`, `keeper_column_widths` and
   `keeper_sidebar_fold` are all still present and readable beside it.
   **This is gate 1 for the byte budget.**
6. Expand ~40 folders across two profiles, relaunch. → the shallowest 32 come
   back, the console says `remembering 32 of 40 open folders`, and the deep tail
   is shut rather than the cookie being absent.
7. Delete a sync profile in **Sync**, return to **Files**. → its folders are gone
   from the tree and its keys are gone from the cookie, with no message.
8. Pause a profile, relaunch, unpause, return to Files. → its expansion is back.
9. **The failure path, which is the one I would check first:** stop the sync
   engine (or launch with `sync.git_path` pointing nowhere) so `sync_profiles`
   rejects. Open Files — empty surface. Confirm the `keeper_files_tree` cookie is
   **unchanged**. Restore the engine, press Refresh. → the tree comes back open.
   Before the `answered` fix this step erased the cookie.
10. Unplug a volume holding an expanded profile and relaunch. → that branch shows
    Rust's absent-drive sentence, the other profiles restore normally, and the
    expansion is not forgotten.

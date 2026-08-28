# Spec 48.6 — A remembered folder is re-read once, not twice

<intent-contract>

**The symptom, in production.** Open Files after a first profile-list call that failed — the surface
says it could not read the sync engine — and press **Refresh**, which is the way back from that
state. Every folder the tree remembers being open is then read from disk **twice**. On the owner's
91,000-file tgdrive that is a duplicated `sync_browse` per remembered folder, not a cosmetic
double-call.

**Where it came from.** Two behaviours, each correct, that meet on the one path that runs both.

| behaviour | where | why it is right |
| --- | --- | --- |
| Refresh re-reads every open folder | `files-pane.tsx` `refresh`, Story 43.8 | "Refresh" means *ask again*; a cache check would make it a no-op |
| the restore stays armed until it has a list Rust really answered | `files-pane.tsx` `restored` effect, Story 46.3 | a failed first call is not evidence, so the restore must survive it |

After a failed first call, Refresh does both: its own loop browses every open folder, and then the
newly-armed restore effect — which reads `filesTreeStore.getState().expanded` **live** — browses
every remembered folder again off the same store. Neither author was wrong; the pair was never run
together in a test that counted.

**How it was found.** It was not found by reading. It was found because three tests in
`files-pane.test.tsx` were failing roughly one run in six under concurrent agent suites, and Story
48.1's author was asked to give a verdict rather than file it as flake. The tests were the messenger.

**Delivered.** The restore skips any folder `load` has already been asked about on this mount. Three
lines. Refresh is untouched.

</intent-contract>

## Why the test could see it and a reader could not

This is the part worth carrying forward, because it is a general hazard in this codebase and not a
local one.

`findBy*` resolves off a **MutationObserver**, which is a microtask. React's **passive effects** are
scheduled work — a `MessageChannel` task. So a test can observe a committed DOM and act on it
*before the passive effect that fixes up state has run*. Under load the gap widens, which is why
this looked like scheduling noise: the interleaving is real and deterministic once it happens, and
whether it happens is a race.

That produced three different-looking failures from one cause:

| test | what it saw |
| --- | --- |
| `FilesPane › re-reads only the folders that are open when Refresh is pressed` | `syncBrowse` called 2 times, expected 1 |
| `FilesPane › asks for a folder's children only when it is expanded, and only once` | the same shape |
| `FilesPane keyboard navigation › steps down and up one visible row at a time` | `expected 'Vault' to be 'Field'` — `press()`'s own `act(...)` flushed the pending restore, which called `retainProfiles`, which re-rendered the row, so the node captured *before* the `act` never received the `keyDown` |

Every failing run logged `An update to FilesPane inside a test was not wrapped in act(...)`
alongside. Every one of thirty clean runs logged none. **That warning is the tell.**

The fix removes the duplicate load, which closes the first two. The third is a stale-node capture in
`press()` and is untouched here — but with the restore no longer issuing a redundant store write,
the re-render it triggered is gone too.

## The hypothesis that was wrong, recorded where the next person will look

> *"A virtualised list test that fails one run in six has a known cause in this repo: `setup.ts`'s
> bounding-rect shim answers a full viewport for any zero-sized element. `window-list` / the Files
> tree is the other virtualised surface in this app and it is still inside the shim's scope."*

**It is not.** `src/components/ui/window-list.tsx` **never calls `getBoundingClientRect`** — zero
occurrences — and neither does `files-pane.tsx`. The window is measured with `clientHeight`
(`window-list.tsx:267`, `:292`), and a zero is explicitly treated as *"this environment did not lay
anything out"*, falling back to `ASSUMED_VIEWPORT_HEIGHT = 640` with a `FILES_ROW_ESTIMATE = 32` per
row. That is a twenty-row window over a two-row tree; there is no index in it to be off by one.
`setup.ts`'s shim only overrides `getBoundingClientRect`, so **it cannot reach the Files tree at
all**. The epic-45 CodeMirror defect it was scoped out for was real; this surface is not its twin.

A one-line grep killed the hypothesis, which is cheaper than any amount of reasoning from it.

## I/O matrix

| sequence | `sync_browse` calls for one remembered folder | before | after |
| --- | --- | --- | --- |
| mount, list succeeds, folder remembered | restore only | 1 | 1 |
| mount, list succeeds, then Refresh | restore, then Refresh's loop | 2 | 2 (correct — Refresh means ask again) |
| **mount, list FAILS, then Refresh** | Refresh's loop, then the armed restore | **2** | **1** |
| mount, list fails, Refresh, Refresh again | + one per Refresh | 3 | 2 |
| user expands a folder before the restore effect flushes | `toggle`, then the restore | 2 | 1 |
| Refresh pressed twice on a healthy pane | one per press | 2 | 2 |

## Mutation table

Sentinel `MUT48-6`. Baseline in the same command and filter as the sweep, immediately before it:
`bun run test src/components/layout/files-pane.test.tsx` → **96 passed, 0 failed**.

| # | mutation | kills | named test |
| --- | --- | --- | --- |
| M1 | the restore stops consulting `requested` (`if (browsable.has(profileId))`) — i.e. the defect restored | 1 | `re-reads a remembered folder once when Refresh rescues a failed first list` |
| M2 | `load` stops recording the key (`requested.current.add` deleted) — the guard is present but never armed | 1 | same |
| M3 | **the wrong fix**: the cache check moved INSIDE `load`, so `load` early-returns for a key it has seen | **5** | `still re-reads on every later Refresh, because Refresh means ask again`; `re-reads only the folders that are open when Refresh is pressed`; `creates a file in the folder it was asked for and re-reads that folder`; `removes every file in the multiselection and re-reads the folder they were in`; `shows the new mark once sync has moved on` |

M3 is the mutation that matters. M1 and M2 prove the guard exists and is armed; M3 proves it is in
the **right place**. The obvious fix — memoise `load` — turns Refresh into a no-op and breaks four
other contracts, and the suite says so out loud. That is why the skip lives in the one caller that
must not ask twice rather than in the function every caller shares.

Restore verified by `cmp` against a pre-sweep copy, by `grep -rn MUT48-6 src/` (nothing), and by
**reading** `git diff -w` on the file: three added lines, one changed condition, nothing else.

## Acceptance

`bun run test src/components/layout/files-pane.test.tsx` → 96 passed, EXIT=0.
`bun run test src/components/layout/ src/components/notes/ src/lib/` → EXIT=0, three consecutive
runs (shared with Story 48.1, same branch).

| requirement | where |
| --- | --- |
| the folder is read once on the failed-list → Refresh path | `re-reads a remembered folder once when Refresh rescues a failed first list`, counted **per key** rather than in total, because the interesting failure is two calls for one folder |
| Refresh still means ask again | `still re-reads on every later Refresh, because Refresh means ask again` |
| the guard is in the right place | mutation M3 |

## Deliberately NOT done

- **`load`'s "always re-asks" contract is unchanged.** Four other tests depend on it — Refresh,
  create-file, delete-selection and the sync-mark refresh all call `load` directly and expect a real
  call. M3 exists to keep it that way.
- **`press()`'s stale-node capture in the keyboard-nav suite is not fixed.** It captures
  `document.activeElement` before its own `act(...)`, so any pending effect that re-renders the row
  detaches the node it is about to dispatch at. That is a test-harness bug in a suite this change
  does not own, and with the duplicate store write gone its trigger here is removed. Worth a DW of
  its own if it recurs.
- **No change to `reachableNodeKeys`, the cookie, or the store.** The remembered set was always
  right; only the number of reads off it was wrong.
- **`requested` is not cleared.** It is per mount, like `restored`, and the pane is unmounted on
  every surface switch. A `Set` that lives as long as one visit to Files is bounded by the folders
  that visit opened.

## What I could not verify here, and why

1. **Nothing Rust, nothing packaged.** `sync_browse` itself is untouched; what changed is how many
   times TypeScript calls it. The `keeper` shell crate does not build on Linux and was not touched.
2. **The production window for the *other* trigger — a click landing between the profile-list commit
   and React's passive-effect flush — is narrow and I did not measure it.** It is a few milliseconds
   in a real browser, unlike in jsdom where a MutationObserver microtask widens it. The failed-list →
   Refresh path needs no race at all and is the one the test asserts; I am not claiming the racy one
   is common, only that the same fix closes it.
3. **The count is asserted against the mock, not against Rust.** That a duplicate `sync_browse`
   actually costs a directory read on a 91,000-file tree is inference from the command's
   implementation, not something measured on the Mac.

### Ordered gate checks

1. `bunx tsc --noEmit` → clean of anything in this file. *(ran)*
2. `bun run test src/components/layout/files-pane.test.tsx` → 96 passed. *(ran, and it is the
   mutation baseline)*
3. `bun run test src/components/layout/ src/components/notes/ src/lib/` → EXIT=0 ×3. *(ran)*
4. `bun run test` full suite, lint, formatter → Main's, once.
5. **On the real app (hesperia) — not performed here, it needs a build on the Mac:**
   1. Stop the sync engine so the profile list fails. Open Files; it says so.
   2. Start it and press **Refresh**. The tree comes back with the folders that were open.
   3. In the log, each of those folders is browsed **once**. Before this change each appeared twice.
   4. Press Refresh again on the healthy pane: each open folder is browsed once more. Refresh still
      means ask again.

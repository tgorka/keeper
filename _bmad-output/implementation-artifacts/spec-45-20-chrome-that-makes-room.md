# Spec 45.20 — Chrome That Makes Room

status: implemented
story: Epic 45, Story 45.20
bindings: FR-198, UX-DR81, UX-DR82
agent: W3Chrome
worktree: `/home/dev/.paseo/worktrees/2va3pp5x/quick-donkey`, branch `feat/epic-44-wave-1` (no branch, no commit)

Four items on one surface. The fold is the big one; the other three are each a real defect or gap.

---

## 0. What already existed, including "nothing"

The brief said to check. Findings, in the order they changed the design:

| Asked for | What was already there | What that meant |
|---|---|---|
| "`sidebar.tsx` already has a collapse mechanism and a cookie" | `src/components/ui/sidebar.tsx` — 668 lines of shadcn scaffolding, a `sidebar_state` cookie, a ⌘B binding, **and zero importers** | It is not a mechanism, it is furniture. Deleted (Main approved). See §1.1. |
| A fold | `useShellLayout().sidebarCollapsed` — a **viewport** rule at 1080px with no user in it, and no persistence | The fold had to be built; it composes with the viewport rule rather than replacing it. |
| Today's Journal applying the journal template | `notes_journal_today` already passed `vault.config.default_template` into the create — **and that field is `None` in every vault whose owner never set one.** 44.7's shipped `Journal entry` template was seeded into every vault and named by nothing. | A wiring fix, as the brief said. One rung, not a new mechanism. See §3. |
| A template-precedence decision | **`templates::rung` / `TemplateRungs`, landed by W3CaptureTag (45.16) while I was reading the file** — the one ladder for `named` → `space` → `capture` → `vault_default` | My journal template is a **rung**, not a second ladder. `journal_template()` returns only the journal's own answer; the fall-through to `default_template` is the ladder's job. See §3.2. |
| An icon meaning *template* | Nothing. 24 icons, flat wrap. | `layout-template`, plus 164 more and a chooser. See §4. |
| An `is:template` predicate | **Already in the closed `is:` set** (`query.rs:63`, `index.rs:136`), and 44.7 widened it to `is_template(&fm) \|\| rel.starts_with("templates/")` | The Templates space reuses it. See §2. |
| `seed::seed_flag` answering `is:template` | Already there — it adds the `template` tag | "New note in Templates" makes a template, for free. |

**A doc comment naming another module's behaviour is an assertion nobody runs.** `src/lib/column-widths.ts` justified its own cookie by saying it followed "the cookie `SidebarProvider` writes `sidebar_state` to". `SidebarProvider` has never been rendered in this app, so **no build of keeper has ever written `sidebar_state`** — the sentence was false the day it was written and had been teaching every reader since. Corrected in place, with the history, so a reader who finds `sidebar_state` in a cookie-jar fixture learns why nothing writes it. This shape was traded to the wave and W2Media/W2Attach applied it to nine cross-module claims of their own.

---

## 1. The fold

### 1.1 The dead component

`src/components/ui/sidebar.tsx` **deleted**. Rationale: keeping it would have made the new fold cookie the *second* sidebar-collapse implementation in a repo whose house rule forbids a second convention beside an existing one — and the existing one was never mounted. Nothing imported it (grepped). Two test files use the literal string `sidebar_state=true` as a **foreign** cookie in a jar fixture; those are still valid and were not touched — a foreign cookie is exactly what it now provably is.

### 1.2 The model

`src/lib/stores/sidebar-fold.ts` (new). One cookie, `keeper_sidebar_fold`, one year, `keeper_`-prefixed, following `@/lib/column-widths` and `@/lib/stores/panels` — the two real precedents.

```
SidebarFold = { menu: boolean; groups: { spaces: boolean; networks: boolean } }
```

**Two folds, not one**, because they answer different questions ("give me the width back" and "I do not care about Networks today"), and one flag would make the rail forget which groups were shut.

Precedence with the viewport, in `AppShell`:

```
sidebarCollapsed = narrow || menuFolded          // narrow = the 1080px rule
onToggleFold     = narrow ? null : toggleFold    // withdrawn, not disabled
```

Below 1080px the viewport has already decided and there is no width to unfold into; the control is **absent**, because a button whose only answer is "your window is too narrow" is worse than no button. Above it the choice is the user's and is remembered, so widening the window later restores the fold they chose rather than the one the window imposed.

`hydrateSidebarFold` is mounted at the **shell**, beside `hydratePanels`, and for the same reason: the drawer is unmounted on the phone tier and can be unmounted for a whole session, so a restore living inside it would silently not happen — the DW-172 defect.

### 1.3 The folded rendering

- The fold control is a real `<button>` in the tab order with `aria-expanded`, `aria-controls="sidebar-views"`, and a name that says which way it goes (`Collapse menu` / `Expand menu`).
- Every nav entry keeps its accessible name on the rail (this was already true) — **asserted now over every button in the list**, so an entry added later without a name fails here.
- **SPACES and NETWORKS used to be dropped entirely from the rail** ("it needs labels + names"), so folding the menu silently removed a navigation surface rather than shrinking one. They now render folded: each row is its Space's or Network's avatar carrying the Space's or Network's own name as its accessible name. No tooltip — the avatar already shows the entity's initials, which is the rail every chat app draws, and a tooltip over a control whose other gesture is a long-press competes with it.
- The Networks row's mute state moves **into the accessible name** on the rail (`"Telegram, muted"`), because the `BellOff` glyph was the only carrier and a glyph is not a name.
- `src/components/layout/sidebar-group.tsx` (new, `FoldableGroup`) holds the shared disclosure so the two groups cannot drift on the accessible-name rule — which is the rule being asserted.

---

## 2. The Templates space

Fifth entry in `keeper_core::notes::default_spaces::DEFAULT_SPACES`: key `templates`, name `Templates`, icon `layout-template`, query **`is:template`**.

**`is:template`, not `tag:template`, and the difference is grandfathering.** Main ruled this and the reason is recorded here because it is load-bearing for whoever changes it: 44.7's Defect 2 moved the predicate off the `templates/` folder prefix (a directory keeper owns, which AD-82 rejects) and onto the frontmatter tag — **while grandfathering the old folder**:

```
is:template  ==  templates::is_template(&fm) || rel.starts_with("templates/")
```

`tag:template` is strictly narrower. It would omit every grandfathered template that lives in `templates/` without the tag — templates `notes_templates` still lists and 44.8 still offers to update from — leaving this space showing fewer templates than the picker. **Whoever eventually retires the `templates/` clause changes what this space selects, and should expect to.**

Everything else follows the 44.3 machinery unchanged: written once, ledgered in `.keeper-spaces.json`, never resurrected after deletion, restorable on request. `seed::seed_flag` already answers `is:template` with the `template` tag, so "New note in Templates" makes a template — confirmed with W3CaptureTag, who owns `seed.rs` and added `("templates", false, false)` to their capture-tag matrix themselves.

---

## 3. Today's Journal applies the journal template

### 3.1 The gap

`notes_journal_today` built its create request with `template: vault.config.default_template.clone()`. That field is a **vault-wide** default and is `None` unless the user set one. 44.7 shipped `Journal entry`, seeded it into every vault at `templates/journal-entry.md`, and **nothing anywhere named it**. Green everywhere, because every test of the template engine hands it a template to begin with.

### 3.2 The fix, in `keeper-core`

```rust
pub fn template_rel(&DefaultTemplate) -> String            // one composer, both directions
pub const JOURNAL_TEMPLATE_KEY: &str = "journal";
pub fn journal_template(present: &dyn Fn(&str) -> bool) -> Option<String>
pub fn render_journal_note(template: Option<(&str,&str)>, title, id, now) -> JournalNote
```

- **`template_rel` exists because two compositions had to agree.** `seed_templates` WRITES `templates/{slug(name)}.md`; `journal_template` LOOKS IT UP. That is the shape that shipped a media URL resolving at a vault root and 404ing in every subfolder — the composer had no test because only the table beneath it did. `seed_templates` now calls it too.
- **`journal_template` is a rung, not a ladder.** It returns the shipped journal template **only if the vault still has it**, and `None` otherwise. `None` is what lets `rung`'s `vault_default` win. Returning the path unconditionally would (a) resurrect a template the user deleted, against AD-79, and (b) *cost* that user their own `default_template`, because a named-and-missing rung stops the ladder and reports missing rather than falling through.
- **`render_journal_note` moved the composition out of the shell** — and that is the point rather than tidiness. "The body came from the template" is the acceptance, and while the frontmatter/tags/properties/provenance assembly lived in `create_journal`, no test on a Linux host could reach it. A test could prove `expand` expands and a path resolves, and nothing could prove the expanded body reached the file. That gap is the defect; leaving the composition unreachable would have left the next one unprovable too.

The shell's `create_journal` is now read → call → write, and `notes_journal_today` contributes one `std::fs` existence check.

---

## 4. The icon chooser

`src/components/notes/space-icons.ts` (new): **188 icons in six groups** (`keeper`, Work, Making, Study, Life, Marks), a `matchSpaceIcons(query)` search over names plus a short alias table, `SPACE_ICONS` **derived from the groups** rather than maintained beside them, and `spaceIcon`/`SpaceIconFallback` moved here from `space-editor.tsx` (clean cutover — `space-list.tsx`'s import updated, no re-export shim).

The chooser is a search field plus one `role="group"` grid per section. "No icon" is **never filtered**, or taking a glyph off a space would depend on what happens to be typed. A blank query is the whole set, which is the browsable state the dialog opens in.

`SPACE_ICON_ALIASES` is deliberately short — only where the key genuinely fails the search ("meeting" → `users`, "money" → `banknote`, "home" → `house`) — because an alias list that grows without a rule becomes a second naming scheme for the same glyphs.

---

## 5. The Recordings entry

One `PaletteActionVm` in `keeper_core::palette`: `open-recordings` / "Open Recordings", Navigation, `requires_recording: true`, **no shortcut chip** (⌘5 is the capture surface's and nothing binds a second chord; a chip here is a promise the cheat sheet would print and no hook would keep). One handler line in `actions.ts`.

The native menu, the ⌘K palette and the ⌘? cheat sheet are all projections of that registry, so one row is the whole change — which is why 42.3's archive had a sidebar entry and no menu entry: the sidebar is hand-written and the menu is derived.

---

## 6. I/O and edge-case matrix

### Fold
| Input | Result | Notes |
|---|---|---|
| No cookie | Unfolded, both groups open | Every control reachable |
| `menu:1\|spaces:0\|networks:1` | Rail; Networks shut | Read out of a jar with two foreign cookies |
| Malformed entry (`menu:`, `menu:yes`, `menu:2`, `:1`, `\|\|\|`) | Dropped → that fold stays **open** | Only safe direction: open is where every control is reachable |
| Unknown key (`someday:1`) | Dropped; known keys still read | Forward compatibility with a newer build |
| Viewport < 1080px | Folded, **no toggle** | Viewport forces; control withdrawn, not disabled |
| Viewport < 1080px, cookie says unfolded | Folded | `narrow \|\| menuFolded` |
| Viewport ≥ 1080px, cookie says folded | Folded, toggle says "Expand menu" | The restart case |
| Toggle pressed | State + cookie both change | Re-read through the same parser a restart uses |
| Group folded | Rows leave the a11y tree (`hidden`); header stays | Or there is no way back |
| Group folded, menu folded | Both apply independently | |
| Phone drawer (`leading-drawer`) | `onToggleFold={null}` | The sheet *is* the fold |
| `document.cookie` throws | Fold still toggles | Best effort; costs the restore, never the click |

### Templates space
| Input | Result |
|---|---|
| Fresh vault | 5 written, in `DEFAULT_SPACES` order, `templates` last |
| Ledger already lists `templates` | Not written |
| Deleted, automatic run | `AlreadySatisfied`, **not** rewritten |
| Deleted, Restore | Exactly the missing one written |
| User space already named "Templates" (any fold) | keeper stands down for that key |
| Unreadable ledger, automatic | `Blocked(sentence)`, nothing written |
| Unreadable ledger, Restore | All five |

### Journal template
| `present(rel)` | `default_template` | Rung taken | Body |
|---|---|---|---|
| true | any | `templates/journal-entry.md` | Template's, expanded |
| false | `templates/mine.md` | `templates/mine.md` | Theirs |
| false | `None` / `""` / `"  "` | none | `# <title>` |
| true, template has tags+properties | — | shipped | Tags cross minus the marker; `title`/`id`/`created`/`updated`/`keeper` never cross |

### Icon chooser
| Query | Result |
|---|---|
| `""` / `"   "` | Every group, identity-equal to `SPACE_ICON_GROUPS` |
| `template` | `layout-template`, one group (`keeper`) |
| `calendar days` / `CALENDAR-DAYS` / `  Calendar Days  ` | Identical to `calendar-days` |
| `money` / `home` / `meeting` / `scaffold` | Alias hits |
| `c` | Multiple groups, each non-empty |
| `qqzzx` | `[]` → `SPACE_ICON_NO_MATCH`, "No icon" still offered |
| Stored icon not in the set | Nothing pressed; save sends the stored name **back unchanged** |

---

## 7. Tests, and the reversion proof

**Mutation sweep: 14 mutations, 14 caught, 0 survivors.** Harness `~/.W3Chrome/sweep.sh`, sentinel `MUTC4520_NN` unique in both directions, one anchor occurrence enforced before and after, per-file md5 before/after.

| # | File | Mutation | Verdict |
|---|---|---|---|
| 01 | `sidebar-fold.ts` | `menu: !get().menu` → `true` | CAUGHT |
| 02 | `sidebar-fold.ts` | `fold.menu = folded` → `false` | CAUGHT |
| 03 | `app-shell.tsx` | `narrow \|\| menuFolded` → `narrow` | CAUGHT |
| 04 | `app-shell.tsx` | `narrow ? null : toggleFold` → `toggleFold` | CAUGHT |
| 05 | `sidebar-pane.tsx` | fold name always "Collapse menu" | CAUGHT |
| 06 | `sidebar-group.tsx` | `hidden={folded}` → `hidden={false}` | CAUGHT |
| 07 | `spaces-group.tsx` | `aria-label={space.name}` → `"Space"` | CAUGHT |
| 08 | `space-icons.ts` | alias arm → `false` | CAUGHT |
| 09 | `space-icons.ts` | blank-query guard → always true | CAUGHT |
| 10 | `actions.ts` | `setView("recordings")` → `"recording"` | CAUGHT |
| 11 | `default_spaces.rs` | `is:template` → `tag:template` | CAUGHT |
| 12 | `templates.rs` | `present(&rel).then_some(rel)` → `Some(rel)` | CAUGHT |
| 13 | `templates.rs` | `template_rel` drops `naming::slug` | CAUGHT |
| 14 | `templates.rs` | journal body → always `# <title>` | CAUGHT |

**Restore verified by content, not by an anchor grep and not by `git diff`.** #11's md5 came back `DIRTY` — because a **sibling** edited `default_spaces.rs` inside my window (W3TagsDelete's `record_deleted`; W2Attach observed an unattributed `MUTTD01` in that file at the time). My own line is `query: "is:template",` present and `tag:template` absent. All 15 anchors re-verified by literal substring count in a script (a shell `grep -c` gave nonsense on patterns containing `(`/`.`/`?` — exactly the metacharacter trap the brief names). **Zero `MUT` sentinels in any of my 16 files.** Five of my files are new and therefore invisible to `git diff` — W3Export's finding — so the sentinel check is by name, not by diff.

---

## 8. The shape audit — a separate list

The sweep was green before any of this. Every shape below came from a peer; none from extending my own list.

| # | Shape | Probe | Outcome |
|---|---|---|---|
| 1 | **What composes the input?** | `SPACE_ICONS` was going to be a hand-kept flat map beside the groups | **Design changed before it shipped.** Derived from the groups; a test asserts the two agree. A key in one and not the other is an icon either browsable-and-unstorable or stored-and-undrawable. |
| 2 | **Did anything press the button?** | Fold tests stopped at "the toggle exists" | **Gap found.** Added: press it and assert the *store and the cookie* changed; and at the shell, press → persist → re-read through the same parser a restart uses. Mutation 04 dies only on the press test. |
| 3 | **A contract stated in a doc comment and enforced nowhere** | `column-widths.ts`: "the cookie `SidebarProvider` writes `sidebar_state` to" | **Prose was false since the day it was written.** `SidebarProvider` never rendered; nothing has ever written `sidebar_state`. Component deleted, sentence corrected with its history. Traded to the wave; produced nine verified claims and two narrowings in other trees. |
| 4 | **A fallback for a case that cannot happen** | `journal_template` first drafted with an internal fall-through to `configured_default` | **Removed.** It duplicated `rung`'s ordering — a second ladder beside the one W3CaptureTag had just landed. Re-probed after removal (mutation 12). |
| 5 | **A fixture that cannot distinguish the right answer from the mutant** | `space-editor.test.tsx`'s "an icon outside the set" used `sparkles` | **Real rot found.** 45.20 *added* `sparkles`, so the test kept passing while testing nothing — every assertion in it was about a name the picker now has. Changed to `no-such-glyph`, which cannot become real by accident. **Weaker than it should be, and W2Media named why:** a value chosen to be unlikely is a convention, and the durable fix is a *seam* that makes the hollow version hard to write. 45.2's registry does it properly — `resolveViewerComponent(file, {})` takes the component table as a parameter that exists for no other reason, so the fallback is exercised against an explicitly empty table forever rather than against an id somebody will bind later. If this chooser grows a second caller, `matchSpaceIcons` should take the catalogue the same way. **Owed, not done.** |
| 6 | **A branch reachable only from a second host** | Counted tests per door for each item | **Two holes.** (a) The Recordings entry had a Rust-registry test and a TS-handler test and **nothing on the menu-bar path the story names** — added, in `use-menu-actions.test.ts`, asserting the id arrives unchanged and is *not* resolved to `open-recording`. (b) The fold had store tests and pane tests and nothing at the shell, which is the only place `hydrateSidebarFold` can fail to be called (DW-172) — added, both arms. |
| 7 | **Assert what you handed on, not only what came back** (Main, generalised by W2Media) | The icon chooser's props and the sidebar's `onToggleFold` | **Applied.** `onToggleFold` is a nullable callback, not a `foldable` boolean beside a handler, so "foldable and no handler" cannot compile; tests supply a real `vi.fn()` and assert the **call**. The icon test presses the found glyph and asserts `notesSpaceSave` received `icon: "layout-template"`, not merely that the button rendered. |
| 8 | **Two-item collection fixtures** (Main) | Every collection in this story | Two Spaces, two Networks, two foreign cookies in the jar, two groups asserted in a search result, two inheritable tags on the journal template, and all four fold-state combinations round-tripped both directions. A rail rendering only the first row, or a search looking only in the first group, dies. |
| 9 | **Write both halves of a two-part edit in one tool call** (W3TagsDelete / W3Capture / W3Export) | — | **I caused this one.** I updated `space-list.tsx`'s import four minutes before writing `space-icons.ts`; W3TagsDelete spent time on `spaceIcon is not a function` in two suites that had never heard of my story. esbuild resolves nothing, so it is invisible to the 50 ms guard. My own contribution to the shape: their version is stronger than mine because it moves the check *before* the claim. |
| 10 | **A doc comment that overclaims is worse than a missing test** (W2Attach/W2Media) | This spec's §9 | §9 separates *established here* from *read, not run* from *never executed*. |
| 11 | **If both your assertions read the same representation, you have one assertion** (W3Recording) | Two textual-absence checks on serialisation keys in `templates.rs` | **Two hollow assertions found.** `assert!(!body.contains("tags:"))` and `assert!(!note.text.contains("keeper:"))` are absences of *serialisation keys*, and neither had a positive using the same literal — so a rename inside `Frontmatter::serialise_new` would make both pass for the wrong reason while the structural assertions beside them (`frontmatter_tags`, `provenance`) cannot see a rename at all. Both now build a witness in the same test and assert the key IS present there first. Note the shipped journal template carries only the `template` marker, which `expand` strips, so the `tags:` witness had to be **built rather than found** — the first attempt paired against this fixture's own frontmatter and failed, which is the assertion doing its job on its first run. `notes::templates` **EXIT=0, 59/59** after. |
| 12 | **`await` / a boolean is not a success check when the callee catches its own failure** (W3NoteFile) | The `present` closure `notes_journal_today` hands to `journal_template` | **Checked, clean, and worth writing down rather than assuming.** `notes_vault::contained(&vault, candidate).is_ok_and(\|path\| path.exists())` collapses a `NotesError` into `false` — the exact shape. It is correct here for two reasons and both are load-bearing: the only argument it ever receives is `template_rel(JOURNAL_TEMPLATE)`, a constant keeper composes, so `contained` cannot fail on it (it rejects empty, NUL-bearing and absolute paths); and if it somehow did, `false` means "the journal template is not here", which falls through to the vault default and then to a plain entry — the same three-way outcome an honest absence produces. **A journal entry is created either way**, which is the whole point of ⌘⌥J, so there is no silent-failure path to hide. The one place this would be wrong is if `present` were ever given a user-supplied path; it is not, and `journal_template` takes no path argument precisely so it cannot be. |
| 13 | **When you replace a mechanism, the contracts the old one kept are not in the diff** (W3Capture) | The deleted `ui/sidebar.tsx`, and the two group renderings I rewrote | **One silent contract change found, and stated rather than left implicit.** The deleted component kept **no production contracts** — it was never mounted, which is the whole reason it could go, and its one carried promise (a persisted collapse) is honoured by a better cookie; its ⌘B binding is a deliberate, documented non-carry because ⌘B is bold in the note editor. But the rewritten `networks-group.tsx` **did** change one: the `BellOff` glyph used to carry `aria-label="Muted"` as its own accessible name, and it is now `aria-hidden` with the state folded into the row's name (`"Telegram, muted"`). Nothing broke — the only test reads `data-testid="network-mute-glyph"`, and `chat-row.tsx`'s identically-named glyph is a different component — so **no diff and no failing test would ever have surfaced this.** It is the better shape (one name per control rather than a control and a decoration each announcing themselves) and it is now the *reason* the mute state survives the fold, since on the rail the glyph is the only thing that used to say it. Recorded because W3Capture is right that the check is not "what did I delete" but "what did the deleted thing promise". |
| 14 | **A mutant and the real thing can produce the same DOM; the defect is then in what the markup MEANS, not what it shows** (W3TagsDelete / W3Recording) | Every `aria-controls` I added | **A real hole, closed.** The group's fold control asserted `aria-controls="sidebar-group-spaces"` and **nothing asserted that anything with that id exists.** A dangling `aria-controls` renders byte-identically — the attribute is present either way, every `getByRole` query still passes, the buttons still work — and the announced relationship between the control and the list it opens is simply gone for a screen reader. Closed by asserting the target contains the rows. Probed as **MUTC4520_15** (drop `id={listId}` from the `<ul>`): **CAUGHT**, by that assertion only; sentinel-free restore verified. The menu-level control already had this witness by accident (`document.getElementById("sidebar-views")`), which is exactly why the group's absence was invisible on a read — one of the two was paired and they look the same. |
| 15 | **A set is a global fact; a search scoped to your own files is not** (found by W3TagsDelete in their run) | Every value 45.20 added to `SPACE_ICONS`, grepped tree-wide instead of locally | **A second hollow fixture, in a file I had touched only two import lines of.** `space-list.test.tsx > draws the fallback glyph for an icon name that is not in the set any more` seeded `icon: "sparkles"` — and 45.20 added `sparkles`, so the test's **entire stated subject became false while it stayed green**: its assertion reads `data-space-icon`, the STORED name, which is byte-identical whether the glyph resolved or fell back. Three things make it worth its row. It was **in someone else's file**, so my local grep could never have found it — widening a shared vocabulary is a global change and the search must be too. It was **half-hollow**: "the unknown name survives a save" is real and still passes, and the live half is what made the dead half invisible. And it presented in W3TagsDelete's run as a **5477 ms timeout**, indistinguishable from the load flake everyone had been dismissing — the second instance this wave of a real defect wearing a timeout. Both fixtures are `no-such-glyph` now, with the history in the doc comment. |
| 16 | **Does this thing NAME something, and does anything check the thing it names exists?** (W3Recording, generalising #14) | Every `aria-labelledby` / `aria-controls` in my new markup | **A third dangling reference, in my own new code.** The icon chooser's six grids carry `aria-labelledby={\`space-icon-group-${group.label}\`}` and the test found them with `getAllByRole("group")` — which resolves whether or not the `<span id>` behind the attribute exists. Closed by making **the query itself the witness**: `getByRole("group", { name: "keeper" })` can only match if the reference resolves to an element saying that. Probed as **MUTC4520_16** (drop the `id` from the heading span): **CAUGHT**. That is a better repair than an extra assertion because it cannot be forgotten — the natural way to write the query is now the checking way, which is the closest any fix in this story gets to a seam. |
| 17 | **Two namers, and nothing making them agree** (W2Media / W3CaptureWindow) | The seeded defaults' `icon` strings, named in Rust and resolved in TypeScript | **A real gap, found and NOT closed — owed, with its shape named.** `DEFAULT_SPACES[n].icon` is a string in `keeper-core` and a key in `SPACE_ICONS` in TypeScript, and **nothing but prose joins them.** I wrote a witness on each side and believed that was enough: Rust asserts every default names a lucide-shaped key, TypeScript asserts `inbox`, `calendar-days`, `pin`, `video` and `layout-template` are present. **But the TypeScript list is a hand-copy, not a read.** A sixth default added in Rust with an icon the picker lacks fails neither test — and the symptom is a freshly seeded vault whose rail draws the unknown-icon fallback on rows keeper itself wrote, which is precisely what 44.3's doc comment says the first four exist to prevent. Green tree, invisible feature: W2Media's protocol handler and W3CaptureWindow's twice-named `capture.html` are the same defect, and all three were created *by* this wave adding a second namer where there had been one. **The fix is their pattern verbatim** — a TypeScript test that reads `default_spaces.rs`, extracts every `icon:` literal, and asserts each is a key of `SPACE_ICONS`, so the half neither language can see is covered on the host that can run it. Not written here because I could not verify it before stopping, and an unverified test is worse than a named gap. |

**One category of debt, not one row.** Four stories this wave independently ended at *"a witness inside the test file is a convention, not a seam — owed, not done"*, and W2Media supplied the reason it happened four times rather than once: **a seam has to live in the production API, and a test-side fix can never be one.** W1Registry's `resolveViewerComponent(entry, components)` works because production code cannot bypass the parameter; a shared test helper can simply not be called. W3TagsDelete added the planning half: the seam is cheap while the module is being designed and expensive once the test is being written, which is exactly why the registry got one and the four of us testing *around* modules did not. Mine (`matchSpaceIcons` taking the catalogue) is at least on the right side of that line — it is a production signature — but it would be ceremony with one caller, so it waits for the second. This belongs in the retro as a category with one shape of repair, not as four separate rows that will each be deferred again.

---

## 9. What I could not verify here, and why

**The shell crate does not build on Linux.** Not compiled, not run, not type-checked:

- `keeper/src/notes_ipc.rs` — the rewritten `notes_journal_today` and `create_journal`. **Verified by rustfmt parse only** (exit 0, and the one formatting difference rustfmt wanted was applied so the fmt gate stays clean). The API calls are checked by eye: `templates::journal_template(&present)` relies on `&closure → &dyn Fn(&str) -> bool` unsized coercion; `is_ok_and` is used elsewhere in this crate (`ipc.rs:14802`); `FieldValue`/`Frontmatter` remain used in that file (31/20 occurrences). **A type error here is possible and would be caught by `cargo check -p keeper` on the gate.**
- `keeper/src/menu.rs` — untouched, but it is where `open-recordings` becomes a menu item. The registry projection is asserted in `keeper-core`; **that the macOS menu bar actually grows a "Recordings" row has never been observed.**
- The phone drawer's `onToggleFold={null}` — `leading-drawer.tsx` is not valid UTF-8 to my read tool, so that one line was applied with `sed` and confirmed by grep, not read in context.

**Never executed on this box:**
- No journal entry has ever been written to a real vault through IPC. `render_journal_note` is proved on its inputs; that `notes_journal_today` finds `templates/journal-entry.md` on a real disk is one `std::fs` call nobody has run. **First check on the gate: press ⌘⌥J in a vault seeded by 44.7 and confirm the new entry opens with `## Focus`, `## Log` and `## Carried forward` rather than a bare date heading.**
- No Templates space has been seeded into a real vault. The seeder is proved against a real temp directory with real permission bits, which is the strong half; the shell's `VaultSeedFiles` adapter is the untested half.
- The fold has never been drawn. jsdom has no width: `w-12` vs `w-[260px]` is asserted as a class, not as pixels. **Second check on the gate: fold the menu and confirm the Spaces rail is a column of avatars, not empty space** — and that the drag band's column (`app-shell.tsx` paints it at `SIDEBAR_WIDTH_CLASS`) still sits flush, because AD-34-3's seam is exactly what a width change reopens.
- 188 icons have never been rendered by a real engine. The catalogue is verified against `lucide-react`'s exports programmatically (zero missing), and every key is asserted kebab-case, but no glyph has been drawn.
- `bun run test src/components/layout/ src/components/notes/` — **1268 passed / 4 failed, 61 files, and none of the four is mine.** `note-file-links.test.tsx` (W3NoteFile, mid-sweep on that exact file, announced), `space-list.test.tsx` ×2 (**passes 24/24 in isolation, verified just now** — the wide run caught a sibling's window; I touched only its two import lines), `live-preview-marks.test.ts` (`.cm-lp-fence`, reported by W3Capture as nobody's yet), and earlier `note-actions.test.tsx > asks Rust what deleting this note would remove` — a `getMultipleElementsFoundError`, a **duplicate** accessible name rather than a missing one, W3TagsDelete's. Three concurrent mutation sweeps were live across this tree during the run; per W2Media's corollary, a wide run over a moving tree is not a measurement, so the number I stand behind is the scoped one below.
**Acceptance-command results, run by name rather than a scope of my choosing:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::` — **EXIT=0, 514/514.** Green on the first run taken after every sibling's mutation window closed and W3Export's `vm.rs` match arm landed. Earlier runs of this exact command were red, and every failure was attributed and re-verified as somebody else's: four `notes::seed::tests::*` capture-tag assertions (W3CaptureTag, mid-sweep, now 39/39), `notes::vm::tests::only_a_seeded_space_promises_to_stay_deleted` (W3TagsDelete's, genuinely red for ten minutes and since fixed by them), two `notes::templates` failures inside W3CaptureTag's `rung` window — **one of which was my own `a_deleted_journal_template_names_nothing_so_the_ladder_falls_through`, red because it composes with the shared ladder, which is exactly what making the journal a rung instead of a second ladder was for** — and finally a syntax error at `vm.rs:4444` that took down the whole compilation unit. None was ever mine. `palette::` **EXIT=0, 25/25**; `notes::templates` **EXIT=0, 59/59**.
- `bun run test src/components/layout/ src/components/notes/` — see the verification line below. The one persistent red I attributed is `note-actions.test.tsx > asks Rust what deleting this note would remove`, a `getMultipleElementsFoundError` (a **duplicate** accessible name, not a missing one) — W3TagsDelete's.
- My own scope, three files' worth of suites plus the two new ones: **EXIT=0, 666/666** across 29 files.

**Deliberately NOT done:**
- **No keyboard chord for the fold.** shadcn's dead component bound ⌘B; ⌘B is **bold** in the note editor. The story asks for a fold that is navigable by keyboard, which a focusable button with `aria-expanded` is; it does not ask for a chord, and the only obvious one is taken.
- **No fold control below 1080px.** The viewport rule predates this story and unfolding into a width that is not there is not a feature.
- **No third foldable group.** `SIDEBAR_GROUPS` is a closed set of the two that exist.
- **`NOTE_PANEL_LIMIT` untouched**, per Main's ruling.
- **`space-editor.tsx` not repointed at W3CaptureTag's new shared `<TemplateSelect>`.** Agreed with them that they would extract it and I would adopt it; it landed late and adopting it is a rewrite of markup I have no other reason to touch this story. **Owed**, and the two load-bearing properties to preserve are recorded in that conversation and in their component: `unlisted` and `missing` stay separate states, and the stored value is rendered as its own option without ever being written back.
- **No byte formatter, no second classifier, no new dependency.**

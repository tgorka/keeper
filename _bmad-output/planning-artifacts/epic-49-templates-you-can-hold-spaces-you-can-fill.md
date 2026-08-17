# Epic 49 — Templates you can hold, spaces you can fill

created: '2026-08-16'
source: a fifth field report from the owner, taken while running 0.8.8 — two items about the
Sessions surface, quoted verbatim below
binds: FR-269–FR-276 (allocated here), AD-121 (session spaces), AD-65 (Rust composes paths),
DW-172 (a fold's hydration is invisible to its store's own tests)
supersedes: the Epic 49 slot that `epics-sessions-phase7.md:43` reserved for *The promote
airlock*. Field-report epics have taken 47 and 48 from the same reservation already
(`sprint-status.yaml:490-504`); the airlock is unbuilt, unstarted, and moves to the next free
number when it is scheduled. Nothing in it is implemented, so nothing is orphaned by this.

## The ask, verbatim

> *w menu sessions oprucz all active, archive stworz mowa opcje templates - zobaczyc i edytowac
> templaty (oraz zmienic nazwe) - dodaj button new template obok new session*

> *spaces w sessions moga miec wiele notatek (wydaje sie ze teraz tylko jedna jest mozliwa) oraz
> moga byc foldable (ustaw w opcjach ze maja byc folder/unfolder by default) oraz dodaj new note
> optcje oraz gdy otwieram notatke ze spaces otworz ja jako notatke (jak w notes)*

## What the code actually said

Four read-only scouts verified every claim against the tree before this spine was written. The
report describes symptoms; two of the seven claims turned out to be a different cause than the
words imply, and one turned out to be already true.

| # | claim (the owner's words) | verdict | evidence | consequence for this epic |
|---|---|---|---|---|
| 1 | a `templates` option beside All/Active/Archived | **unreachable** | `sessions_patterns` already returns every template with `kind: "template"` (`sessions_ipc.rs:709-762`), and the only render path is the create row's `<Select>` (`sessions-pane.tsx:194-219`, `session-pattern-picker.tsx:139-159`). The list is pane-local `useState`, read only while `creating` | 49.1 renders what Rust already answers. It is **not** a fourth `STATUS_CHOICES` value: those are typed `SessionsStatusFilter` and matched against `row.status` (`sessions-list.ts:15,112-135`), and a template has no status |
| 2 | see and **edit** a template | **unreachable** | a template is `<zone>/_template/` and `<zone>/_template/<name>/` on disk (`sessions/model.rs:16`), deliberately never a session row (FR-225). The panel editor takes a plain `(profileId, relativePath)` (`sessions-pane.tsx:125-131`) and would open a template file today if anything named one | 49.1 lists the template's files from Rust and opens them through the existing file target. No new editor |
| 3 | **rename** a template | **absent** | no rename command exists anywhere in `sessions_ipc.rs`; the only renames in the app are `notesRename` and `recordingRetitle`. `docs/sessions.md` records a *deliberate* refusal to rename **session files** — "a rename is a link-rewriting problem, not a file-system one" | the refusal has teeth for a file whose identity IS its path. A `_template/<name>` folder is pointed at by nothing durable: the pattern id is minted per read (`pattern.rs:159`) and the picker already replaces an id that stopped existing (`sessions-pane.tsx:208-212`). 49.1 adds `sessions_template_rename` and says so here |
| 4 | a `new template` button beside `new session` | **unreachable** | `sessions_template_install(root_id, name: Option<String>)` has taken a name since it was written (`sessions_ipc.rs:1793`, doc at `:1780-1782`), `client.ts:5220` types it, and the one caller drops it (`sessions-pane.tsx:235`) — behind a control that hides itself the moment a zone has any template (`session-pattern-picker.tsx:110-111`) | 49.1 passes the argument. **Not** as a fourth header button: the header group is `shrink-0` beside a `min-w-0` title, so a fourth button silently steals the subtitle's width and pushes the filter row under the fold (UX-DR85). It goes on the templates list's own header, the way `New space` sits on the Spaces heading |
| 5 | a space can only hold one note | **already 1:N — the write path is the defect** | `Vec` in the domain (`spaces.rs:418`), `Vec` on the wire (`vm.rs:487-491`), `files.map` in the UI (`session-spaces.tsx:426`), no cap at any layer, and a two-file listing is **already asserted** (`session-spaces.test.tsx:107-117`). What the owner sees is that four of the five default spaces are single-`tag:` queries (`spaces.rs:100-155`) and the only reachable creators are `New log` and `New prompt` on the *Files* heading (`session-file-actions.tsx:140-168`) — `New file` deliberately writes no kind tag, so it lands in `Unfiled` and no space lists it | 49.2 gives a space a create verb. **No multiplicity work at all** — there was never a limit to lift, and this is written down so nobody "fixes" one |
| 6 | a `new note` action in a space | **unreachable** | `sessionsFileNewKind(rootId, sessionId, kind, title)` exists and writes the tag that decides which space lists the file (`client.ts:5259-5283`); `ref` and `task` have no button anywhere in the app | 49.2 ports the notes-side affordance (`space-list.tsx:88-96,288-300`) with the kind derived **in Rust** from the space's own query |
| 7 | opening a note from a space opens it as a note | **broken** | the row sets `kind: "file"` (`session-spaces.tsx:442-447`); the Notes pane sets `kind: "note"` (`notes-pane.tsx:289-296`); both are variants of one `PanelTargetVm` (`panels.ts:45`), and the bridge that converts one to the other shipped in Story 45.18 (`vault-link/actions.ts:113-149`) and is already wired into the text viewer | 49.2 calls the existing bridge, and **keeps** the file target when the zone is not inside a vault — `notePathForFile` returns `null` there (`rule.ts:145-149`) and a file target is then the only correct answer |
| 8 | spaces fold, folded/unfolded by default from options | **absent** | `SpaceSection` has no fold, no chevron, no store (`session-spaces.tsx:313-461`); the canonical mechanism is `FoldSection` (`sidebar-group.tsx:40-171`) and there is no settings key for any fold default. The nearest shipped shape is `favorites_collapsed` — persisted, `Settable::AnyLayer`, and with **no** Settings control | 49.3 builds both halves the owner asked for: the fold, remembered per space, **and** a real Settings switch for what an unrecorded space does. See the decision below |

## Decisions this epic takes, where they deviate from the words

1. **`Templates` is a view mode, not a status filter.** The owner said "besides all active, archive";
   the chip lives exactly there and reads as a peer, but it drives a separate `mode` field. Putting
   a template into `SessionsStatusFilter` would make `filterRows` match a `row.status` that does not
   exist.
2. **`New template` is not beside `New session` in the header.** It is on the templates list's own
   header, which is where `New space` and `Restore default spaces` already live in this very
   surface. The arithmetic is in the epic table above; a fourth `size="sm"` button in that header
   costs the subtitle ~290px of a ≤512px row at a 560px window.
3. **Nothing is done about "many notes per space", because it already is many.** The honest fix for
   the symptom is the create verb. Story 49.2 states this in its own contract so a future reader
   does not go looking for a cap that never existed.
4. **The fold default is a real setting, not only last-state-wins.** `favorites_collapsed` shipped
   persistence alone and let `keeper.toml` be the only default knob. The owner asked for the option
   explicitly — *"ustaw w opcjach"* — so 49.3 adds `sessions.spaces_folded` end to end with a
   Switch, and the per-space fold the person performs is an override on top of it. A space with no
   recorded state follows the setting; a space they folded or unfolded by hand keeps their answer.
5. **The kind a space creates is derived in Rust.** TypeScript never parses the space DSL — that is
   the rule `use-notes-actions.ts:67-73` already states for notes spaces. A space whose query is not
   one creatable `tag:` term shows no button at all (absent, never disabled).

## Functional requirements this epic adds

- **FR-269** The sessions board offers a `Templates` mode beside its status chips, listing the
  zone's own `_template/` and every named `_template/<name>/`, with the count of files each holds.
- **FR-270** A template's files are listed from Rust and open in the same editor a session's files
  open in; the frontend composes no path.
- **FR-271** A named template can be created by name from the templates list, through the command
  that has always taken the name.
- **FR-272** A named template can be renamed. The zone's own `_template/` cannot — its name is the
  contract (`model::TEMPLATE_DIR`).
- **FR-273** A session space offers *New note in `<space>`* when its query names one creatable kind,
  writing the tag that makes that space list the result, and opening what it wrote.
- **FR-274** A row inside a session space opens as a **note** when the zone lies inside a notes
  vault, and as a file when it does not, saying which in the section's live region.
- **FR-275** Each session space folds, and the fold is remembered per space across a restart.
- **FR-276** `sessions.spaces_folded` decides what a space with no remembered fold does, is settable
  from Settings and from a config layer, and is documented in the generated key table.

## Stories

- **49.1 — Templates are a room you can enter** (crosses IPC). The `Templates` mode on the sessions
  board, the templates list with per-template file counts, `sessions_template_entries` and
  `sessions_template_rename` in the shell, the named-create path finally passing its argument, and
  the rename form. Binds FR-269–FR-272.
- **49.2 — A space you can write into, and a row that opens as a note** (crosses IPC).
  `SessionSpaceVm.newFileKind` derived from the space's query, the per-space create verb, and the
  file→note bridge on the row click with its honest fallback. Binds FR-273, FR-274.
- **49.3 — Spaces fold, and the fold has a default you can set** (crosses IPC). A per-space fold
  store shaped like `files-tree.ts` (an open key set — spaces are user-created), `FoldSection` in
  `SpaceSection`, hydration at the detail mount with the DW-172 test at the mount point, and
  `sessions.spaces_folded` through all eight hops with its Settings switch. Binds FR-275, FR-276.

Three stories, three stacked PRs, in that order: 49.2 and 49.3 both edit `session-spaces.tsx` and
`session-detail.tsx`, so 49.3 sits on top of 49.2 rather than beside it.

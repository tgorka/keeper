/**
 * A fake shell, so the real frontend can be looked at without Tauri.
 *
 * **Why this exists.** For five epics the only way to see this app was to build
 * it on a Mac and look at it there — a fifteen-minute round trip that made
 * visual work effectively impossible and is the honest reason the UI stayed
 * characterless. Every design decision was made by reading code.
 *
 * That round trip was never a *Linux* limitation, whatever earlier revisions of
 * this comment claimed (they cited AD-55 and AD-56, which are about
 * `keeper_core` being tauri-free and sync-free and say nothing about any
 * platform). The `keeper` shell crate builds here — `cargo build -p keeper`
 * produces an ELF. What is genuinely Mac-only is narrower: the recording
 * sidecar (Swift + Xcode) and code signing. Running the built binary needs a
 * DISPLAY, which is a different problem with a different fix.
 *
 * Keeping the false version cost real time: it steered agents into a Mac round
 * trip for work a `bun run dev` in this container would have shown in seconds.
 *
 * `mockIPC` answers `invoke` in the browser, so `bun run dev` serves the REAL
 * components, the real stores, the real CSS — everything except Rust. That is
 * the whole point: a fixture gallery would show what a designer drew, and this
 * shows what the app actually renders.
 *
 * **What it is not.** It is not a test double and nothing asserts against it.
 * The suite mocks `@/lib/ipc/client` per file, which is a tighter seam and stays
 * the right one; this sits a layer lower, under `@tauri-apps/api`, precisely so
 * it can serve the parts of the app no test mounts. If a screen looks right here
 * and wrong in the app, this file is wrong — it is a viewing aid, never
 * evidence.
 *
 * **Dev only, and structurally so.** The import is behind `import.meta.env.DEV`
 * in `main.tsx`, so Rollup drops the whole module from a production build; there
 * is no runtime flag to get wrong. It also refuses to install when a real shell
 * is present, so `tauri dev` is never quietly served fixtures.
 */

import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  FileSizeVm,
  FilesEntrySyncVm,
  FilesEntryVm,
  FilesListingVm,
  SessionSpaceFilesVm,
  SessionSpaceFileVm,
  SessionSpaceVm,
} from "@/lib/ipc/client";

/** Roughly now, so relative timestamps read as "3 min ago" rather than 1970. */
const NOW = Date.now();
const ago = (minutes: number) => NOW - minutes * 60_000;

/**
 * Enough rows to judge DENSITY, which is the thing a screenshot of three items
 * cannot show. Real note titles from a real vault's shape — dated journal
 * entries, a couple of long ones that must truncate, tags that overflow — so
 * the list is stressed the way the owner's is rather than the way a demo is.
 */
const NOTES = [
  [
    "n1",
    "Keeper work",
    "# Keeper work\n\nColumn folds, the capture window, the header overflow.",
    ["epic22"],
    ago(5),
    true,
  ],
  [
    "n2",
    "2026-08-10",
    "# 2026-08-10\n\n## Focus\n\n## Log\n\n## Carried forward",
    [],
    ago(190),
    false,
  ],
  [
    "n3",
    "Recording — first pass at the shared transport",
    "# Recording\n\nTwo tracks, one clock.",
    ["first-recording", "recordings"],
    ago(240),
    false,
  ],
  [
    "n4",
    "AGENTS.md — 10-notes",
    "# AGENTS.md\n\nZone rules for the notes vault.",
    ["template"],
    ago(560),
    false,
  ],
  [
    "n5",
    "Interview kickstart — trees",
    "# Trees\n\nBalanced, red-black, and why the rotation is the whole trick.",
    ["live-test"],
    ago(700),
    false,
  ],
  [
    "n6",
    "2026-08-09",
    "# 2026-08-09\n\n## Focus\n\nShip the gitattributes repair.",
    [],
    ago(1500),
    false,
  ],
  [
    "n7",
    "A note whose title is long enough that the list has to decide what to do about it",
    "# Long\n\n…",
    ["test", "template", "recordings"],
    ago(1600),
    false,
  ],
  ["n8", "Templates", "# Templates", ["template"], ago(1800), false],
  ["n9", "2026-08-08", "# 2026-08-08", [], ago(2600), false],
  [
    "n10",
    "Sync — the pendrive case",
    "# Sync\n\nA removable volume that vanishes mid-commit.",
    ["epic22"],
    ago(3000),
    false,
  ],
] as const;

const noteRows = NOTES.map(([id, title, body, tags, modified, pinned], index) => ({
  id,
  title,
  path: `${title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .slice(0, 40)}.md`,
  excerpt: String(body).split("\n").filter(Boolean).slice(0, 2).join(" ").slice(0, 120),
  tags: [...tags],
  modifiedMs: modified,
  pinned,
  unread: false,
  conflict: false,
  archived: false,
  agentTouched: id === "n1",
  // `NoteOrder`, never null: `NoteRow` names the order in its `aria-label` and
  // reads `order.source` to do it, so a null here threw the whole list.
  order: { value: index, source: "default" },
  sessionId: id === "n3" ? "01SESSION" : null,
}));

/** The five defaults 44.3 seeds, plus the shapes a real vault grows. */
const SPACES = [
  ["s-inbox", "Inbox", "is:untagged", "inbox", "inbox"],
  ["s-journal", "Journal", "is:journal", "calendar-days", "journal"],
  ["s-pinned", "Pinned", "is:pinned", "pin", "pinned"],
  ["s-rec", "Recordings", "is:recording", "video", "recordings"],
  ["s-tpl", "Templates", "is:template", "layout-template", "templates"],
  ["s-work", "Active work", "tag:epic22", "layers", null],
].map(([id, name, query, icon, defaultKey]) => ({
  id,
  name,
  query,
  icon,
  defaultKey,
  order: 0,
  sort: null,
  error: null,
  warnings: [],
}));

const TAGS = [
  ["epic22", 1],
  ["first-recording", 4],
  ["live-test", 3],
  ["recordings", 5],
  ["template", 3],
  ["test", 1],
].map(([tag, count]) => ({ tag, count, children: [] }));

/**
 * One row of a synced folder, in the shape the pane actually reads.
 *
 * **Typed against the generated `FilesEntryVm` on purpose.** This fixture had
 * drifted two stories behind the wire — `isDir`, `sizeBytes`, a bare
 * `sync: "synced"` and a `roles` array, where the pane now reads `kind`,
 * `size: { bytes, label }`, `sync: { status, detail }` and `folderRole`. The
 * cost of a wrong-shaped fixture is not a wrong-looking row: expanding any
 * folder threw on `write.writable` and took the whole window with it, so the
 * failure read as a bug in the surface being examined rather than in the
 * harness. A missing answer falls through to {@link fallback} and renders an
 * empty state; a wrong-shaped one is a blank page. `dev` is inside
 * `tsconfig.json`'s `include` so that this annotation is a gate rather than a
 * comment.
 */
function browseEntry(name: string, isDir: boolean, size: FileSizeVm | null): FilesEntryVm {
  return {
    name: name.slice(name.lastIndexOf("/") + 1),
    relativePath: name,
    absolutePath: `/Volumes/merope/tgdrive/${name}`,
    kind: isDir ? "folder" : "file",
    sync: { status: "synced", detail: null },
    size: isDir ? null : size,
    folderRole: name === "10-notes" ? "notesVault" : null,
    // Story 56.2's two additions. `lfsOid` is null because none of these rows
    // is a virtual path, which is the statement that `size` came off a `stat`;
    // `mtimeMs` is a fixed instant so the harness renders identically on every
    // run.
    lfsOid: null,
    mtimeMs: 1_700_000_000_000,
    // Writable, because the write path — New file, Delete, and the header's
    // count that gates them — is exactly what a viewing aid has to be able to
    // show. A refusal is a different fixture and this is not it.
    write: { writable: true, reason: null, caveat: null, caveatShort: null },
  };
}

/**
 * A row whose bytes are not where its size says they are (Story 56.7).
 *
 * A sibling rather than two more parameters on {@link browseEntry}, so the rows
 * above it are untouched. The sentence is typed out here because in the real
 * product `sync_ipc::sync_mark` composes it and this file is what stands in for
 * that — a paraphrase would put words on the harness's marks that keeper never
 * says, and reviewing the wrong words is worse than reviewing none.
 *
 * `lfsOid` makes {@link browseEntry}'s statement in reverse: a non-null oid says
 * `size` is the POINTER's number rather than a `stat`'s, which is exactly the
 * claim a virtual row makes and a materialized one does not.
 */
function lfsEntry(
  name: string,
  size: FileSizeVm,
  sync: FilesEntrySyncVm,
  lfsOid: string | null,
): FilesEntryVm {
  return { ...browseEntry(name, false, size), sync, lfsOid };
}

/** Verbatim from `sync_ipc::sync_mark`, for the reason {@link lfsEntry} gives. */
const VIRTUAL_SENTENCE =
  "This file's content is not stored on this computer — only a placeholder is, so it takes up almost no space. The size shown is the content's.";
/** The state a `queued_downloads` row puts a pointer in: queued, running or
 *  deferred, which is why the words are about the QUEUE and not about an
 *  activity — a deferred download whose removable remote is absent waits
 *  indefinitely, and "is downloading" would be false about it. */
const MATERIALIZING_SENTENCE =
  "keeper has this file's content queued to download to this computer.";
const MATERIALIZED_SENTENCE =
  "This file's content is on this computer. keeper may release it again later to free the space, and can fetch it back.";

/** A folder tree with the depth and the awkward names a real drive has. */
const ENTRIES: FilesEntryVm[] = [
  browseEntry("00-inbox", true, null),
  browseEntry("10-notes", true, null),
  browseEntry("20-records", true, null),
  browseEntry("30-work", true, null),
  browseEntry("40-media", true, null),
  browseEntry("50-library", true, null),
  browseEntry(".gitattributes", false, { bytes: 16_384, label: "16.4 kB" }),
  browseEntry("AGENTS.md", false, { bytes: 4_812, label: "4.8 kB" }),
  browseEntry("README.md", false, { bytes: 3_380, label: "3.4 kB" }),
  browseEntry("deck-v10-complete.pdf", false, { bytes: 8_400_000, label: "8.4 MB" }),
  browseEntry("screen-0000.mov", false, { bytes: 412_000_000, label: "412 MB" }),
  // The states Story 56.7 made visible, side by side and all large, so the
  // harness shows what the pane's whole point is: rows claiming the same four
  // gigabytes, two of which are 130 bytes of placeholder on this disk. The
  // middle one is here because it is the only state that takes the mark's
  // `role="progressbar"` branch, its own arrow glyph and a non-recessive tone —
  // a state nothing can show is a state nobody reviews.
  lfsEntry(
    "master-2026-04.wav",
    { bytes: 4_294_967_296, label: "4.3 GB" },
    { status: "virtual", detail: VIRTUAL_SENTENCE },
    "3f79bb7b435b05321651daefd374cdc681dc06faa65e374e38337b88ca046dea",
  ),
  lfsEntry(
    "master-2026-05.wav",
    { bytes: 4_294_967_296, label: "4.3 GB" },
    { status: "materializing", detail: MATERIALIZING_SENTENCE },
    // Still a pointer on disk, which is what `browse::classify` requires before
    // it will call a queued download this content arriving — so `size` is the
    // pointer's number here exactly as it is for the virtual row above.
    "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  ),
  lfsEntry(
    "master-2026-06.wav",
    { bytes: 4_294_967_296, label: "4.3 GB" },
    { status: "materialized", detail: MATERIALIZED_SENTENCE },
    null,
  ),
];

/**
 * What one folder inside the root holds.
 *
 * One and not six: an expansion has to show something other than the root's own
 * rows again, and beyond that this is a viewing aid rather than a disk.
 * Every other folder answers `listed` with nothing in it, which is a state the
 * pane draws a sentence for and is worth seeing too.
 */
const CHILDREN: Record<string, FilesEntryVm[]> = {
  "10-notes": [
    browseEntry("10-notes/standup.md", false, { bytes: 1_204, label: "1.2 kB" }),
    browseEntry("10-notes/decisions.md", false, { bytes: 9_100, label: "9.1 kB" }),
    browseEntry("10-notes/attachments", true, null),
  ],
};

/**
 * One session's file tree, in the FLAT shape (`shape.rs`): every markdown file
 * sits at the root and declares its kind in frontmatter, and the only two
 * directories left are `artifacts/` (versioned) and `workspace/` (scratch).
 *
 * The fixture's whole job is to be the case that is hard to look at: a pile of
 * dated filenames that means nothing until something reads the tags. If the
 * detail renders this legibly — files first, kinds grouped, log at the bottom —
 * it renders the live zone legibly.
 *
 * `workspace/` carries `locked` with the fence's own sentence, because a lock
 * with no reason is the bug AD-113's refusal text exists to prevent, and a
 * viewing aid that quietly dropped it would hide the one row whose rendering is
 * least obvious.
 *
 * `undeletable` is computed here rather than passed in, mirroring what
 * `files::check_deletable` answers in Rust (FR-262): the two shape files and
 * every directory refuse, everything else is deletable. It is a re-statement of
 * a Rust rule and therefore exactly the kind of thing that drifts — which is
 * survivable in a viewing aid whose whole purpose is to show what the rows look
 * like, and would not be anywhere else.
 */
function sessionEntry(
  relPath: string,
  isDir: boolean,
  bytes: number | null,
  minutesAgo: number,
  locked: string | null = null,
) {
  const cut = relPath.lastIndexOf("/");
  return {
    name: relPath.slice(cut + 1),
    relPath,
    parent: cut === -1 ? "" : relPath.slice(0, cut),
    depth: relPath.split("/").length,
    isDir,
    subpath: `60-sessions/active/2026-08-12-keeper-sessions/${relPath}`,
    absolutePath: `/Volumes/merope/tgdrive/60-sessions/active/2026-08-12-keeper-sessions/${relPath}`,
    size: bytes === null ? null : { bytes, label: `${(bytes / 1000).toFixed(1)} kB` },
    mtimeMs: ago(minutesAgo),
    sync: { status: "synced", detail: null },
    locked,
    undeletable: isDir
      ? "keeper deletes one file at a time. Removing a folder takes everything inside it with it, which is a bigger promise than this tree makes — do it in Finder."
      : relPath === "AGENTS.md" || relPath === "about.md"
        ? `${relPath} is what tells keeper this session is a flat one: deleting it would silently turn the session back into the old folder shape.`
        : null,
  };
}

const SESSION_ENTRIES = [
  sessionEntry("AGENTS.md", false, 2_140, 220),
  sessionEntry("about.md", false, 1_860, 45),
  sessionEntry("2026-08-12-0900-opened-the-session.md", false, 720, 220),
  sessionEntry("2026-08-12-1740-flat-pool-lands.md", false, 1_310, 45),
  sessionEntry("2026-08-13-0910-spaces-and-the-board.md", false, 980, 12),
  sessionEntry("task-migrate-the-live-zone.md", false, 410, 30),
  sessionEntry("task-task-board-columns.md", false, 380, 30),
  sessionEntry("task-search-everywhere.md", false, 350, 30),
  sessionEntry("task-named-templates.md", false, 330, 30),
  sessionEntry("prompt-01-scope.md", false, 640, 220),
  sessionEntry("ref-inputs.md", false, 520, 100),
  sessionEntry("stray-thought.md", false, 180, 8),
  sessionEntry("artifacts", true, null, 45),
  sessionEntry("artifacts/flat-contract.md", false, 4_400, 45),
  sessionEntry("workspace", true, null, 3, "workspace/ is scratch: keeper never writes here."),
  sessionEntry(
    "workspace/scratch.md",
    false,
    900,
    3,
    "workspace/ is scratch: keeper never writes here.",
  ),
];

/**
 * The board's four columns, each holding at least one card, plus the two cases
 * the columns alone would not show: a card whose `order:` keeper defaulted
 * (`orderIsOwn: false`) and a card with no ULID (`path:` identity, flagged).
 * Both render differently, and both come from files a person hand-wrote — which
 * is the ordinary case in a zone Obsidian also edits.
 */
/**
 * What the three markdown widgets select, in a vault rather than in a session
 * (FR-264).
 *
 * Deliberately a second fixture and not `SESSION_TASKS` reshaped: a widget board
 * lives in an ordinary note, addresses its cards by note id, and — unlike a
 * session's closed four — may hold any word at all in `status:`. The `blocked`
 * card is here for that: it is what the board's "Not in a column" row exists to
 * show, and in a vault it is the common case rather than the exception.
 */
const WIDGET_NOTES = [
  {
    id: "w1",
    path: "projects/keeper/task-widgets.md",
    title: "Markdown widgets in any note",
    snippet: "Callout syntax, a StateField, one React host.",
    tags: ["task"],
    updatedMs: ago(30),
    status: "todo" as string,
    order: 1,
    orderIsOwn: true,
  },
  {
    id: "w2",
    path: "projects/keeper/task-add-ref.md",
    title: "The add-a-reference picker",
    snippet: "Disk, note, recording — and the promotion offer.",
    tags: ["task", "ui"],
    updatedMs: ago(90),
    status: "in-preparation" as string,
    order: 1,
    orderIsOwn: true,
  },
  {
    id: "w3",
    path: "projects/keeper/task-search.md",
    title: "Search everywhere",
    snippet: "⌘F in the document, ⌘⇧F across all of it.",
    tags: ["task"],
    updatedMs: ago(200),
    status: "blocked" as string,
    order: 0,
    orderIsOwn: false,
  },
  {
    id: "w4",
    path: "projects/keeper/task-spaces.md",
    title: "Spaces replace the folders",
    snippet: "Five defaults, all of them files.",
    tags: ["task"],
    updatedMs: ago(400),
    status: "done" as string,
    order: 1,
    orderIsOwn: true,
  },
  {
    id: "w5",
    path: "projects/keeper/log/2026-08-13-1420-board.md",
    title: "The board landed",
    snippet: "Four columns, drag, and the column menu for everybody else.",
    tags: ["log"],
    updatedMs: ago(60),
    status: null as string | null,
    order: 0,
    orderIsOwn: false,
  },
  {
    id: "w6",
    path: "projects/keeper/log/2026-08-12-0930-flat.md",
    title: "Flat contract",
    snippet: "One pool of markdown; the kind is a tag.",
    tags: ["log"],
    updatedMs: ago(1_500),
    status: null as string | null,
    order: 0,
    orderIsOwn: false,
  },
  {
    id: "w7",
    path: "projects/keeper/ref-live-zone.md",
    title: "The live 60-sessions zone",
    snippet: "/Volumes/merope/tgdrive/60-sessions — read-only from the container.",
    tags: ["ref"],
    updatedMs: ago(2_000),
    status: null as string | null,
    order: 0,
    orderIsOwn: false,
  },
];

const SESSION_TASKS = [
  {
    id: "01J8AAAAAAAAAAAAAAAAAAAAAA",
    relPath: "task-migrate-the-live-zone.md",
    title: "Migrate the live zone",
    status: "in-preparation",
    order: 1,
    orderIsOwn: true,
    tags: ["task", "migration"],
    unstableIdentity: false,
  },
  {
    id: "01J8BBBBBBBBBBBBBBBBBBBBBB",
    relPath: "task-task-board-columns.md",
    title: "Task board — four columns, drag to reorder",
    status: "todo",
    order: 1,
    orderIsOwn: true,
    tags: ["task", "ui"],
    unstableIdentity: false,
  },
  {
    id: "path:task-search-everywhere.md",
    relPath: "task-search-everywhere.md",
    title: "Search everywhere (⌘F, ⌘⇧F)",
    status: "todo",
    order: 0,
    orderIsOwn: false,
    tags: ["task"],
    unstableIdentity: true,
  },
  {
    id: "01J8CCCCCCCCCCCCCCCCCCCCCC",
    relPath: "task-named-templates.md",
    title: "Named templates",
    status: "done",
    order: 1,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: false,
  },
];

/**
 * The zone's five default spaces, plus the two failures the healthy five cannot
 * show: one whose query will not parse (`error`, selects nothing) and one whose
 * `sort` keeper could not read (`warnings`, still selects). Those two states
 * render differently and are the whole reason the VM carries two fields, so a
 * fixture without them shows the section that never has a bad day.
 *
 * Annotated rather than inferred, and mutable rather than `as const`, for the
 * reason the Files listing above is annotated: the save and delete handlers
 * write into this array, so its element type has to be the wire type instead of
 * whatever the first five literals happened to imply (which had `icon: string`
 * and refused the `null` a space with no icon carries).
 */
const SESSION_SPACES: SessionSpaceVm[] = [
  {
    id: "_spaces/about.md",
    name: "About",
    // The live zone's About space, not the seeded one (Story 53.4): the owner
    // typed a second term into his own `_spaces/about.md`, and this is the state
    // the repair exists for. The DEFAULT is `tag:about` and always was — editing
    // that const reaches no zone that already has a `_spaces/` directory, which
    // is why the fix is a press and not a constant.
    query: "tag:about tag:recordings",
    sort: "title asc",
    sortEffective: "title asc",
    icon: "info",
    defaultKey: "about",
    order: 1,
    warnings: [],
    error: null,
    // `about` is the one kind `sessions_file_new_kind` refuses: a session has
    // one record, and a second would give `shape()` two answers. And this query
    // asks for two things, which is refused first — so this space renders a
    // create that is PRESENT and DISABLED, describing itself with the sentence on
    // `noHome` below (Story 52.4), plus the repair beside it (Story 53.4).
    newFileKind: null,
    // Says nothing about how it opens or how much it shows, which is what four
    // of the five defaults do — the plain case, so the fixture still shows a
    // space that behaves exactly as it did before Story 51.3.
    folded: null,
    rows: null,
    // Names no directory and inherits nothing: `about` is the one kind a create
    // is refused for outright, so there is nowhere for a destination to point
    // (Story 53.5).
    createDir: null,
    createDirDefault: "",
  },
  {
    id: "_spaces/tasks.md",
    name: "Tasks",
    query: "tag:task",
    sort: "order asc",
    sortEffective: "order asc",
    icon: "square-check",
    defaultKey: "tasks",
    order: 2,
    warnings: [],
    error: null,
    newFileKind: "task",
    folded: null,
    // Four selected, two drawn: the row cap on screen, with *Show 2 more* under
    // it and the header still counting 4. A fixture capped at or above its own
    // list would render the control never, which is the state that hides a
    // regression rather than showing it.
    rows: 2,
    // **The owner's own state, and the whole of Story 53.5**: the file carries
    // `keeper.default: tasks` and no `keeper.create_dir` at all, because it was
    // seeded before any default named a directory. Nothing rewrites it — the
    // inheritance is resolved on read, so an empty box here is `tasks/` and the
    // editor says so with the placeholder rather than with a value it would then
    // persist (AD-121).
    createDir: null,
    createDirDefault: "tasks",
  },
  {
    id: "_spaces/log.md",
    name: "Log",
    query: "tag:log",
    sort: "modified desc",
    sortEffective: "modified desc",
    icon: "clock",
    defaultKey: "log",
    order: 3,
    warnings: [],
    error: null,
    newFileKind: "log",
    // Arrives shut on its own say-so, with the setting off — the layer between
    // the person's hand and `sessions.spaces_folded`, and the one a fixture has
    // to carry or nobody sees it until a real zone sets it.
    folded: true,
    rows: null,
    // The one fixture that TYPES its destination (Story 52.5): a new log
    // goes into `logs/`, keeper makes the directory, and the space still lists
    // the file because its QUERY matched the tag rather than the folder. Here it
    // happens to agree with what it would have inherited, which is the ordinary
    // case after Story 53.5 and still a different state from inheriting it.
    createDir: "logs",
    createDirDefault: "logs",
  },
  {
    id: "_spaces/refs.md",
    name: "References",
    // An unreadable `sort` is a WARNING: the space still selects what it
    // selects and simply falls back, so the section lists normally under a
    // quieter sentence rather than sending anyone to fix a query that is fine.
    query: "tag:ref",
    sort: "sideways asc",
    sortEffective: "modified desc",
    icon: "link",
    defaultKey: "refs",
    order: 4,
    warnings: [
      "Couldn't read sort `sideways asc`; using modified desc.",
      'keeper can\'t read the row limit "many", so this space shows every file it selects.',
    ],
    error: null,
    // A misread `sort` does not stop a create: the query still names one kind.
    newFileKind: "ref",
    // An unreadable cap is a WARNING and the value is DROPPED: the section shows
    // everything it selected and says why, beside the sort it also could not
    // read. Two warnings on one space is the list the editor prints in full.
    folded: null,
    rows: null,
    // The EXPLICIT-EMPTY state (Story 53.5): this file names the empty string,
    // which is an operator saying *the session's own root* and choosing against
    // the `refs/` they would otherwise have inherited. Distinct from Tasks above,
    // and the editor has to say which of the two it is showing.
    createDir: "",
    createDirDefault: "refs",
  },
  {
    id: "_spaces/prompts.md",
    name: "Prompts",
    // A broken query is an ERROR and selects NOTHING — never everything. A
    // saved view that silently widened to the whole session is how a bulk
    // action becomes a data-loss story.
    query: "tag:prompt AND",
    sort: "title asc",
    sortEffective: "title asc",
    icon: "message-square",
    defaultKey: "prompts",
    order: 5,
    warnings: [],
    error: "Unexpected end of query after `AND`.",
    // A query that will not parse names no kind, so Rust derives none.
    newFileKind: null,
    folded: null,
    rows: null,
    createDir: null,
    createDirDefault: "prompts",
  },
  {
    id: "_spaces/untagged.md",
    name: "Untagged",
    // Every kind negated (Story 52.4): the residue, which the detail used to
    // draw as a badge list with no count, no fold and no row verbs.
    query: "-tag:about -tag:log -tag:prompt -tag:ref -tag:task",
    sort: "name asc",
    sortEffective: "name asc",
    icon: "inbox",
    defaultKey: "untagged",
    order: 6,
    warnings: [],
    error: null,
    // A negated query names no kind, so there is nothing a create here could
    // write — present and disabled, with the sentence below.
    newFileKind: null,
    folded: null,
    rows: null,
    // The residue is not a kind and offers no create, so it names nowhere and
    // inherits nowhere.
    createDir: null,
    createDirDefault: "",
  },
];

function spaceFile(
  relPath: string,
  title: string,
  tags: string[],
  minutesAgo: number,
): SessionSpaceFileVm {
  return {
    id: `path:${relPath}`,
    relPath,
    subpath: `60-sessions/active/2026-08-12-keeper-sessions/${relPath}`,
    title,
    tags,
    mtimeMs: ago(minutesAgo),
    // Path-identified, because that is the ordinary state of a zone Obsidian
    // also edits: keeper never stamps an `id:` into a file it did not author.
    unstableIdentity: true,
  };
}

/**
 * `KindHasNoHome::OnlyOne`, as fixture bytes: the refusal the About space meets
 * once its query asks for one thing — which is what the repair leaves behind
 * (Story 53.4), and the state the seeded space is in.
 *
 * Restated here rather than imported, for the reason the namers below are: this
 * shell never runs in the app, and Rust owns the wording wherever it does.
 */
const ONE_RECORD_REFUSAL =
  "a session has one about record — about.md under the flat contract, README.md under the " +
  "folder one — and keeper edits it rather than making a second.";

/** `spaces::Refusal::ManyTerms`, as fixture bytes. */
const MANY_TERMS_REFUSAL =
  "this space asks for more than one thing, so there is no single kind a file made here could " +
  "be: every term has to hold for a file to appear, and a create writes one kind with one tag. " +
  "Narrow the query to a single `tag:` term to write into this space, or make the file from " +
  "Files below and tag it so this space picks it up.";

/**
 * What each space selected out of the mock session — same files as the tree, so
 * the two sections agree with each other, which is the first thing a person
 * scrolling past them checks.
 *
 * `prompts` answers an empty list and carries its own error: the broken query
 * above selects nothing, and the section prints Rust's sentence rather than
 * inventing a second one for the same state.
 */
const SESSION_SPACE_FILES: SessionSpaceFilesVm[] = [
  {
    spaceId: "_spaces/about.md",
    files: [spaceFile("about.md", "About this session", ["about"], 45)],
    error: null,
    // The mock session is a handful of files, so the real walk would never hit
    // its budget: nothing is a prefix here, and every space says so (Story 53.5).
    poolTruncated: false,
    // Two terms, so the QUERY's refusal is the one a person meets — before
    // anything looks at what its terms name (Story 51.7's ordering).
    noHome: MANY_TERMS_REFUSAL,
    // And the repair Rust offers beside that sentence (Story 53.4): the space
    // claims `default: about`, whose own query asks for one term, so one press
    // writes `tag:about`. A whole query, composed in Rust, so the label can say
    // what the press will do before it is pressed.
    narrowTo: "tag:about",
    // The verb this space offers instead of a create is opening the record,
    // which a two-term query naming `tag:about` still names.
    openRecord: true,
  },
  {
    spaceId: "_spaces/tasks.md",
    files: [
      spaceFile(
        "task-migrate-the-live-zone.md",
        "Migrate the live zone",
        ["task", "migration"],
        30,
      ),
      spaceFile(
        "task-task-board-columns.md",
        "Task board — four columns, drag to reorder",
        ["task", "ui"],
        30,
      ),
      spaceFile("task-search-everywhere.md", "Search everywhere (⌘F, ⌘⇧F)", ["task"], 30),
      spaceFile("task-named-templates.md", "Named templates", ["task"], 30),
    ],
    error: null,
    poolTruncated: false,
    noHome: null,
    // Nothing to repair: a space asking for one thing is not over-specified, and
    // Rust offers no press where there is nothing to narrow.
    narrowTo: null,
    openRecord: false,
  },
  {
    spaceId: "_spaces/log.md",
    files: [
      spaceFile("2026-08-13-0910-spaces-and-the-board.md", "Spaces and the board", ["log"], 12),
      spaceFile("2026-08-12-1740-flat-pool-lands.md", "Flat pool lands", ["log"], 45),
      spaceFile("2026-08-12-0900-opened-the-session.md", "Opened the session", ["log"], 220),
    ],
    error: null,
    poolTruncated: false,
    noHome: null,
    narrowTo: null,
    openRecord: false,
  },
  {
    spaceId: "_spaces/refs.md",
    files: [spaceFile("ref-inputs.md", "Inputs", ["ref"], 100)],
    error: null,
    poolTruncated: false,
    noHome: null,
    narrowTo: null,
    openRecord: false,
  },
  {
    spaceId: "_spaces/prompts.md",
    files: [],
    error: "Unexpected end of query after `AND`.",
    poolTruncated: false,
    noHome: null,
    narrowTo: null,
    openRecord: false,
  },
  {
    spaceId: "_spaces/untagged.md",
    // The one untagged file the tree also shows, because a clean fixture would
    // never render the state this space exists for — and a half-migrated session
    // is the state the operator will actually meet. It stood on `unfiled` until
    // Story 52.4, which is a field this payload no longer has.
    files: [spaceFile("stray-thought.md", "stray-thought", [], 8)],
    error: null,
    poolTruncated: false,
    // `spaces::Refusal::Negated`, restated as fixture bytes for the reason the
    // record's refusal above is: this shell never runs in the app, and Rust owns
    // the wording wherever it does.
    noHome:
      "this space asks for what is left over — every one of its terms is a negation — so it " +
      "names no kind, and a create writes one kind with one tag. There is nothing a file made " +
      "here could be: make the file from Files below, and it appears here until you give it a " +
      "kind tag.",
    // A negated query is not narrowed by a button: `ManyTerms`' advice would send
    // this space round a loop, so its refusal carries no repair.
    narrowTo: null,
    openRecord: false,
  },
];

/**
 * The session zone AS a notes vault, and the notes it holds (Story 49.2,
 * FR-274).
 *
 * The other vault in this file is `10-notes`, which no `60-sessions/…` path can
 * ever sit inside — so with it alone every space row resolves to "no vault" and
 * opens the FILE viewer, and the half of this story that puts a session file in
 * the full note editor cannot be looked at at all. A mock that renders a
 * control and cannot render its outcome is how a surface ships unreachable.
 *
 * The index is derived from the space selections rather than hand-written, so a
 * file created from a space is resolvable as a note the moment it appears in
 * the space — the two halves of the story stay agreed with each other, which is
 * the first thing a person pressing the button checks.
 */
const SESSION_VAULT_ID = "v2";

/** The mock session's directory, vault-relative — `subpath` minus `60-sessions/`. */
const SESSION_ZONE_DIR = "active/2026-08-12-keeper-sessions";

function sessionNotes() {
  // A file can sit in two spaces, and the index holds it once.
  const seen = new Set<string>();
  const files = SESSION_SPACE_FILES.flatMap((listing) => listing.files).filter((file) => {
    if (seen.has(file.relPath)) {
      return false;
    }
    seen.add(file.relPath);
    return true;
  });
  return files.map((file) => ({
    // Stable and path-derived, which is what the mock can honestly offer: the
    // real id is a ULID in frontmatter and this file writes no frontmatter.
    id: `sn-${file.relPath}`,
    path: `${SESSION_ZONE_DIR}/${file.relPath}`,
    title: file.title,
    snippet: "",
    tags: [...file.tags],
    updatedMs: file.mtimeMs,
    pinned: false,
    archived: false,
    unread: false,
    conflict: false,
    origin: "",
    headRev: "",
    order: { value: 0, source: "default" },
  }));
}

/**
 * A CSV wider than any pane, so an embedded table can be LOOKED at under the
 * one condition that matters: more columns than the note has room for, and one
 * value long enough that no cap could show it whole.
 *
 * Answered for every `![[….csv]]` the shell is asked about, because the mock has
 * no vault to resolve a target against and the point of the fixture is the shape
 * rather than the file.
 */
const CSV_COLUMNS = [
  "device",
  "serial",
  "firmware",
  "last-seen",
  "owner",
  "location",
  "notes",
] as const;

const CSV_ROWS = [
  [...CSV_COLUMNS],
  [
    "hesperia",
    "C02XK1YZQ6NV",
    "15.4.1-build-2026-07-30",
    "2026-08-12 09:14",
    "alice",
    "desk, second floor, by the window",
    "a note long enough that a twenty-four em cap would have hidden the end of it and left nothing to press",
  ],
  [
    "electra",
    "C02XK1YZQ6NW",
    "15.4.0-build-2026-06-02",
    "2026-08-11 22:03",
    "bob",
    "rack 3",
    "spare",
  ],
] as const;

/**
 * Answers keyed by command. Anything absent falls through to `fallback`, which
 * is why a screen keeps rendering when it reaches for something not listed —
 * an unanswered command should show an empty state, never a white page.
 */
const ANSWERS: Record<string, unknown> = {
  app_ping: "ok",
  capabilities: { notes: true, recording: true, sync: true, chat: true },
  notes_vaults: [
    {
      id: "v1",
      profileId: "p1",
      name: "tgdrive",
      subfolder: "10-notes",
      root: "/Volumes/merope/tgdrive/10-notes",
      indexed: true,
      noteCount: 42,
      unreadCount: 0,
      captureTemplate: null,
      captureTag: null,
      cadence: { commitIdleMs: 2000 },
    },
    // The session zone, flagged as a vault — see {@link sessionNotes}. Two
    // vaults on one profile is also the shape `notePathForFile`'s
    // longest-subfolder rule needs to be looked at: `10-notes` must not answer
    // for a `60-sessions/…` path.
    {
      id: SESSION_VAULT_ID,
      profileId: "p1",
      name: "sessions",
      subfolder: "60-sessions",
      root: "/Volumes/merope/tgdrive/60-sessions",
      indexed: true,
      noteCount: 9,
      unreadCount: 0,
      captureTemplate: null,
      captureTag: null,
      cadence: { commitIdleMs: 2000 },
    },
  ],
  notes_vault_active: "v1",
  // `NoteListVm`, not a bare array: the pane reads `rows`, `total` and
  // `matched` off the answer, and an array left the Notes surface throwing
  // `Cannot read properties of undefined (reading 'length')` on mount — which
  // is the whole screen, in the one shell that exists to let it be looked at.
  notes_list: { rows: noteRows, total: noteRows.length, matched: noteRows.length, offset: 0 },
  notes_spaces: SPACES,
  notes_tag_tree: { nodes: TAGS },
  notes_templates: [],
  notes_backlinks: [],
  notes_history: [],
  notes_gallery: { entries: [] },
  // A `![[….csv]]` embed reads the file and then reads it as a table. Both, or
  // the panel degrades to the plain wikilink and the block nobody can see is the
  // block that was being looked at.
  notes_embed_read: {
    relPath: "attachments/devices.csv",
    name: "devices.csv",
    kind: "file",
    file: {
      text: CSV_ROWS.map((row) => row.join(",")).join("\n"),
      sizeBytes: 512,
      sizeLabel: "512 B",
      oversize: false,
      binary: false,
      detail: null,
    },
  },
  notes_csv_read: {
    relPath: "attachments/devices.csv",
    rev: "rev-1",
    columns: CSV_COLUMNS.length,
    totalRows: CSV_ROWS.length,
    rows: CSV_ROWS.map((cells, index) => ({
      index,
      line: index + 1,
      cells: [...cells],
      ragged: false,
    })),
    notices: [],
  },
  // Deliberately NOT annotated `satisfies SyncProfileVm[]`: that type carries
  // twenty fields and this row answers six. It is under-specified rather than
  // wrong-shaped, which is the harmless failure — every consumer reads the six
  // and the rest come back `undefined` — and filling in fourteen invented
  // values to buy a type annotation would be a fixture that lies. The rule the
  // Files listing above follows is the one worth keeping: annotate the shapes
  // a consumer DEREFERENCES, because those are the ones that blank a window.
  sync_profiles: [
    {
      id: "p1",
      name: "tgdrive",
      localPath: "/Volumes/merope/tgdrive",
      remoteUrl: "git@electra:tgdrive.git",
      enabled: true,
      recordingsSubfolder: "recordings",
    },
  ],
  sync_statuses: [{ profileId: "p1", state: "idle", pending: 0, lastSyncMs: ago(3) }],
  sync_problems: { profiles: [] },
  sync_git_status: { available: true, path: "/usr/bin/git", version: "2.53.0" },
  sync_device: { id: "01DEVICE", label: "hesperia" },
  config_layers: { overrides: [], faults: [], mainFolder: null },
  recording_settings_get: {
    codec: "hevc",
    fps: 60,
    scalePercent: 100,
    segmentMb: 250,
    durationCapMinutes: 60,
    destinationDir: "/Volumes/merope/tgdrive/recordings",
    pathTemplate: "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}",
    echoCancellation: true,
  },
  notes_capture_windows: [],
  // Sessions. Until now this whole feature fell through to `fallback`, which
  // answers `null` for `sessions_detail` and blanks the pane — so the one
  // surface being built was the one surface the viewing aid could not show.
  sessions_roots: [
    {
      id: "p1",
      name: "tgdrive",
      subfolder: "60-sessions",
      root: "/Volumes/merope/tgdrive/60-sessions",
      indexed: true,
      activeCount: 1,
      unreadCount: 0,
    },
  ],
  sessions_list: [
    {
      id: "01J8SESSIONAAAAAAAAAAAAAAA",
      path: "active/2026-08-12-keeper-sessions",
      title: "Keeper — sessions, round two",
      status: "active",
      archivedYear: null,
      workspaceMs: ago(3),
      recordMs: ago(12),
      lastLogDate: "2026-08-13",
      lastLogLine: "Spaces and the board",
      snippet: "Flat markdown pool, spaces as saved queries, a task board.",
      tags: ["keeper", "sessions"],
      pinned: true,
      unread: false,
      origin: "local",
      headRev: "rev-mock",
      conflict: false,
      lineage: false,
    },
    // The folder-shaped session, without which the migrate verb has nothing to
    // act on: every fixture being already-flat would show only the branch that
    // does nothing. Its row is deliberately ordinary — a session that predates
    // the flat contract is not a broken one, and must not render as a warning.
    {
      id: "01J8SESSIONBBBBBBBBBBBBBBB",
      path: "active/2026-06-30-old-shape",
      title: "Before the flat contract",
      status: "active",
      archivedYear: null,
      workspaceMs: null,
      recordMs: ago(9_000),
      lastLogDate: "2026-07-02",
      lastLogLine: "Wrapped the first pass",
      snippet: "A session from when refs/ and prompts/ were directories.",
      tags: ["keeper"],
      pinned: false,
      unread: false,
      origin: "local",
      headRev: "rev-mock",
      conflict: false,
      lineage: false,
    },
  ],
  sessions_detail: {
    id: "01J8SESSIONAAAAAAAAAAAAAAA",
    path: "active/2026-08-12-keeper-sessions",
    title: "Keeper — sessions, round two",
    status: "active",
    archivedYear: null,
    pinned: true,
    tags: ["keeper", "sessions"],
    properties: [
      { key: "owner", value: "tgorka" },
      { key: "stack", value: "#118" },
    ],
    continues: [],
    continuedBy: [],
    summary: "Flat markdown pool, spaces as saved queries, a task board.",
    // Newest first — the projection reverses, the files do not (FR-233).
    log: [
      {
        date: "2026-08-13",
        title: "Spaces and the board",
        body: "Five default spaces parse. The board reuses `order: f64`.",
      },
      {
        date: "2026-08-12",
        title: "Flat pool lands",
        body: "`shape.rs` decides the contract; `pool.rs` reads it.",
      },
      { date: "2026-08-12", title: "Opened the session", body: "" },
    ],
    shape: "flat",
    tasks: SESSION_TASKS,
  },
  sessions_tree: { entries: SESSION_ENTRIES, truncated: false },
  sessions_refs: {
    refs: [
      {
        kind: "missing",
        target: "refs/gone.md",
        label: "refs/gone.md",
        source: "ref-inputs.md",
        panelTarget: null,
        url: null,
        notice: "Looked in the session folder and in 10-notes; no such file.",
      },
      {
        kind: "note",
        target: "[[Keeper work]]",
        label: "Keeper work",
        source: "ref-inputs.md",
        panelTarget: null,
        url: null,
        notice: null,
      },
      {
        kind: "external",
        target: "https://github.com/tgorka/keeper",
        label: "keeper on GitHub",
        source: "about.md",
        panelTarget: null,
        url: "https://github.com/tgorka/keeper",
        notice: null,
      },
    ],
    missing: 1,
    truncated: false,
  },
  sessions_patterns: [
    {
      id: "_template",
      kind: "template",
      label: "Zone template",
      detail: "AGENTS.md, about.md, a seed log and a seed prompt.",
      mtimeMs: ago(1_440),
      copies: [
        { relPath: "AGENTS.md", isDir: false },
        { relPath: "about.md", isDir: false },
      ],
      skips: [],
    },
    // A named template (FR-266) — `_template/<name>/`, addressed by the path
    // it copies out of, sorted by name rather than by mtime.
    {
      id: "_template/interview",
      kind: "template",
      label: "interview",
      detail: "a named template — copied whole",
      mtimeMs: ago(4_320),
      copies: [
        { relPath: "AGENTS.md", isDir: false },
        { relPath: "about.md", isDir: false },
        { relPath: "2026-08-01-0900-questions.md", isDir: false },
      ],
      skips: [],
    },
  ],
  sessions_spaces: SESSION_SPACES,
  sessions_space_files: SESSION_SPACE_FILES,
  sessions_spaces_restore: { names: [] },
  // The install verb answers with the directory it wrote (FR-268). The mock
  // zone above already HAS a `_template`, so the picker's offer stays hidden
  // here — the fixture exists so a hand-driven call does not fall through to
  // the name-guessing default and come back as a list.
  sessions_template_install: "_template",
};

/**
 * A shape that keeps a caller alive rather than a correct one. Most reads want
 * a list; a few want an object; the rest are commands whose answer nobody looks
 * at. Guessing by NAME is crude and deliberate — the alternative is 256 hand
 * fixtures, which is a maintenance burden for a viewing aid.
 */
function fallback(command: string): unknown {
  if (
    /^(notes_(list|spaces|templates|backlinks|history)|sync_(profiles|statuses|activity|pending|problems))/.test(
      command,
    )
  ) {
    return [];
  }
  if (/(_get|_read|_status|_settings|_vm)$/.test(command)) {
    return null;
  }
  if (/^(notes_subscribe|sync_subscribe|.*_subscribe)/.test(command)) {
    return "sub-mock";
  }
  return null;
}

/**
 * The few commands whose answer depends on WHICH thing was asked for.
 *
 * `ANSWERS` is a table of one shape per command, which is enough for a viewing
 * aid until two callers ask the same command about different rows and both get
 * the first row back. `notes_body_read` is the first of those: a folded note
 * panel names its spine from the note's own body, so a table would draw the
 * same title down every strip.
 *
 * The body is composed to start with the row's title, because that is the
 * relationship the app relies on (FR-98: a note's title IS its first body
 * line). A fixture whose body and title disagree tests the mock, not the app.
 */
/**
 * The pre-flat session, as `pool.rs` never sees it: `log` comes from the
 * README's `### ` entries, `tasks` is empty because no task file exists before
 * the migration, and `shape` says so. Everything the flat detail
 * shows is absent here, which is the point — this is the fixture that proves
 * the detail degrades instead of erroring on a session it cannot group.
 */
const FOLDER_DETAIL = {
  id: "01J8SESSIONBBBBBBBBBBBBBBB",
  path: "active/2026-06-30-old-shape",
  title: "Before the flat contract",
  status: "active",
  archivedYear: null,
  pinned: false,
  tags: ["keeper"],
  properties: [{ key: "owner", value: "tgorka" }],
  continues: [],
  continuedBy: [],
  summary: "A session from when refs/ and prompts/ were directories.",
  log: [
    { date: "2026-07-02", title: "Wrapped the first pass", body: "Promoted two artifacts." },
    { date: "2026-06-30", title: "", body: "" },
  ],
  shape: "folder",
  tasks: [],
};

/**
 * What migrating that session would do — the same three lists the real preview
 * returns, session-relative. The empty-title `### 2026-06-30 — ` entry becomes
 * `2026-06-30-0000-untitled.md` rather than being dropped, because a heading
 * the operator typed is content even when they typed nothing after it.
 */
const FOLDER_MIGRATION = {
  needed: true,
  creates: [
    "about.md",
    "2026-06-30-0000-untitled.md",
    "2026-07-02-0001-wrapped-the-first-pass.md",
    "refs/inputs.md",
    "prompts/01-scope.md",
    "AGENTS.md",
  ],
  rewrites: ["README.md"],
  trashes: ["refs", "prompts"],
};

const HANDLERS: Record<string, (payload: Record<string, unknown>) => unknown> = {
  // Two sessions, two shapes: a table would answer the flat one for both and
  // the folder-shaped row would render as something it is not.
  sessions_detail: (payload) =>
    payload.sessionId === FOLDER_DETAIL.id ? FOLDER_DETAIL : ANSWERS.sessions_detail,
  // `needed: false` is not an error and not an empty preview — it is the answer
  // for every session that already holds the contract, which is most of them.
  sessions_migrate_preview: (payload) =>
    payload.sessionId === FOLDER_DETAIL.id
      ? FOLDER_MIGRATION
      : { needed: false, creates: [], rewrites: [], trashes: [] },
  sessions_migrate: () => null,
  // The two space writes MUTATE the fixture rather than answering and forgetting.
  // A shell that accepted a save and then re-answered the old five would make
  // the editor look broken in exactly the way it is being looked at for — the
  // whole flow under test is press Save, watch the section re-read. Statefulness
  // in a viewing aid is a feature when the thing being viewed is a round trip.
  sessions_space_save: (payload) => {
    const req = (payload.space ?? {}) as Record<string, unknown>;
    const id = typeof req.id === "string" ? req.id : `_spaces/${String(req.name)}.md`;
    const saved = {
      id,
      name: String(req.name ?? ""),
      query: String(req.query ?? ""),
      sort: String(req.sort ?? "modified desc"),
      sortEffective: String(req.sort ?? "modified desc"),
      icon: (req.icon ?? null) as string | null,
      // Never invented: `defaultKey` says a space is one of the five keeper
      // knows how to restore, and a hand-made one is not, no matter what it is
      // called. Claiming otherwise here would make restore look like it had
      // opinions about a file the operator wrote.
      defaultKey: null,
      order: Number(req.order ?? 0),
      warnings: [],
      error: null,
      // Rust derives this from the query, and this shell has no parser —
      // writing one here would be the second grammar `creatable_kind`'s own
      // doc warns about. A space the operator invents therefore carries no
      // kind until the real backend answers; an edited one keeps the kind it
      // already had, below.
      newFileKind: null,
      // Straight off the request, because the round trip is the thing worth
      // looking at here: `render_edit` replaces the whole `keeper:` map, so a
      // mock that dropped either key would make the destroying bug invisible in
      // exactly the surface built to catch it. `null` writes no key.
      folded: typeof req.folded === "boolean" ? req.folded : null,
      rows: typeof req.rows === "number" ? req.rows : null,
      // `null` straight through since Story 53.5 — a field the form never
      // touched writes no key and the space keeps inheriting, while `""` is a
      // cleared box and means the session's own root. Collapsing them here would
      // hide the exact bug this surface exists to catch.
      createDir: typeof req.createDir === "string" ? req.createDir.trim() : null,
      // A space the operator invents claims no default, so it inherits nothing.
      // An EDITED one keeps what it had — see the merge below.
      createDirDefault: "",
    };
    const at = SESSION_SPACES.findIndex((space) => space.id === id);
    if (at === -1) {
      SESSION_SPACES.push(saved);
      // A space the operator invents selects nothing yet and refuses nothing:
      // the real backend answers both once it has read the query.
      SESSION_SPACE_FILES.push({
        spaceId: id,
        files: [],
        error: null,
        poolTruncated: false,
        noHome: null,
        narrowTo: null,
        openRecord: false,
      });
    } else {
      SESSION_SPACES[at] = {
        ...SESSION_SPACES[at],
        ...saved,
        // Not dropped by an edit: the section's create control would vanish on
        // save for a reason the real backend does not have.
        newFileKind: SESSION_SPACES[at].newFileKind,
        // Same rule, and the sharper one: this is Rust's answer about which
        // default the file claims, not anything the form can send. An edit that
        // reset it would make the seeded Tasks space stop inheriting `tasks/` the
        // first time somebody renamed it.
        createDirDefault: SESSION_SPACES[at].createDirDefault,
      };
    }
    return id;
  },
  // The repair (Story 53.4): narrow an over-specified space to the single term
  // its default asks for. It mutates the DEFINITION and the REFUSAL, because the
  // state the press leaves behind is the whole thing worth looking at here — the
  // arity sentence becomes the one-record one, and the control goes away with the
  // fault it fixed.
  //
  // The narrowed query is read off `narrowTo`, which Rust composed: this shell
  // has no parser and must not grow one to decide what a query narrows to (the
  // second grammar `creatable_kind`'s own doc warns about). The refusal it lands
  // on is the record's, which is true of the one over-specified fixture this file
  // holds — About — and is fixture bytes, not a rule.
  sessions_space_narrow: (payload) => {
    const id = String(payload.spaceId ?? "");
    const files = SESSION_SPACE_FILES.find((row) => row.spaceId === id);
    const at = SESSION_SPACES.findIndex((space) => space.id === id);
    const narrowed = files?.narrowTo ?? null;
    // A guard rather than a policy: the control that sends this only exists where
    // `narrowTo` is set, and Rust refuses the verb everywhere else.
    if (narrowed !== null && files !== undefined && at !== -1) {
      SESSION_SPACES[at] = { ...SESSION_SPACES[at], query: narrowed };
      files.noHome = ONE_RECORD_REFUSAL;
      files.narrowTo = null;
    }
    return id;
  },
  sessions_space_delete: (payload) => {
    const id = String(payload.spaceId ?? "");
    const at = SESSION_SPACES.findIndex((space) => space.id === id);
    if (at !== -1) {
      SESSION_SPACES.splice(at, 1);
    }
    const filesAt = SESSION_SPACE_FILES.findIndex((row) => row.spaceId === id);
    if (filesAt !== -1) {
      SESSION_SPACE_FILES.splice(filesAt, 1);
    }
    return null;
  },
  // The three file verbs mutate `SESSION_ENTRIES` for the reason the space
  // writes mutate theirs: what is being looked at IS the round trip. A create
  // that answered a path the tree then failed to show would look exactly like
  // the bug this aid exists to rule out.
  //
  // The names are keeper's own rules restated in TypeScript — slug, counter,
  // stamp — and that is a copy of `files::new_named`/`new_stamped`. Acceptable
  // here and nowhere else: this file never runs in the app, and a mock that
  // returns a name the real one would not is still a mock that renders a row.
  sessions_file_new: (payload) => {
    const parent = String(payload.parent ?? "");
    const kind = String(payload.kind ?? "md");
    const slug =
      String(payload.title ?? "")
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "") || "untitled";
    const relPath = `${parent === "" ? "" : `${parent}/`}${slug}.${kind}`;
    SESSION_ENTRIES.push(sessionEntry(relPath, false, 120, 0));
    return `60-sessions/active/2026-08-12-keeper-sessions/${relPath}`;
  },
  // The folder verb (FR-287). Idempotent like the `MkDir` it compiles to, and
  // folded like `files::dir_rel` folds — last segment only, so a path whose
  // parent is a folder already in the tree lands inside it. Same restatement
  // licence as the namers above: this file never runs in the app.
  sessions_dir_new: (payload) => {
    const typed = String(payload.rel ?? "")
      .trim()
      .replace(/\/+$/, "");
    const cut = typed.lastIndexOf("/");
    const parent = cut === -1 ? "" : typed.slice(0, cut);
    const name =
      typed
        .slice(cut + 1)
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "") || "untitled";
    const relPath = `${parent === "" ? "" : `${parent}/`}${name}`;
    if (!SESSION_ENTRIES.some((entry) => entry.relPath === relPath)) {
      SESSION_ENTRIES.push(sessionEntry(relPath, true, null, 0));
    }
    return null;
  },
  sessions_file_new_kind: (payload) => {
    const kind = String(payload.kind ?? "log");
    const slug =
      String(payload.title ?? "")
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "") || "untitled";
    // The stamp the real namer writes, from the clock this file already fakes
    // against — `YYYY-MM-DD-HHMM-<slug>.md`, which is what makes a log folder
    // sort itself.
    const now = new Date(ago(0));
    const pad = (value: number) => String(value).padStart(2, "0");
    const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
    // Which space asked, and what it says about where its files go (Story
    // 52.5). The real backend reads the definition and composes the path in
    // Rust (AD-65); the fixture mirrors it, because a `bun dev` that put a
    // space's creates at the root would show the defect this story fixed.
    // Only a space matched BY ID names a directory — the Files heading sends no
    // id, so its own creates keep landing at the session root.
    //
    // **`??` and not `||`, which is the whole of Story 53.5 in one operator.** A
    // `null` destination is a file that named none and inherits its default's;
    // an EMPTY one is a file that named the session's own root. `||` would fold
    // the second into the first and put a create in `refs/` that its own
    // definition had sent to the root — the mirror image of the bug 52.5 left.
    const asked = SESSION_SPACES.find((space) => space.id === String(payload.spaceId ?? ""));
    const named = asked === undefined ? null : (asked.createDir ?? asked.createDirDefault);
    const dir = (named ?? "").trim().replace(/\/+$/, "");
    const relPath = `${dir === "" ? "" : `${dir}/`}${stamp}-${slug}.md`;
    if (dir !== "" && !SESSION_ENTRIES.some((entry) => entry.relPath === dir)) {
      // `compile_new` leads with the same `MkDir`, so the directory appears in
      // the tree on the first create rather than after a restart.
      SESSION_ENTRIES.push(sessionEntry(dir, true, null, 0));
    }
    SESSION_ENTRIES.push(sessionEntry(relPath, false, 96, 0));
    // …and into the SPACE that asked for this kind, which is the whole of what
    // FR-273 does and the one thing a screenshot of the tree cannot show. The
    // real backend writes the tag into frontmatter and the next
    // `sessions_space_files` read selects it; here the selection IS the
    // fixture, so the row has to be put where that read would have found it.
    // Without this the press adds a line to the tree and leaves Tasks exactly
    // as empty as before — a mock that renders the control and not its outcome.
    //
    // By kind and not by the id above, because the Files heading's creates
    // belong to a space too: what makes a file appear in one is its TAG
    // (AD-120), whichever directory it sits in.
    const target = asked ?? SESSION_SPACES.find((space) => space.newFileKind === kind);
    const listing = SESSION_SPACE_FILES.find((row) => row.spaceId === target?.id);
    listing?.files.unshift(
      spaceFile(relPath, String(payload.title ?? "").trim() || "untitled", [kind], 0),
    );
    return `60-sessions/active/2026-08-12-keeper-sessions/${relPath}`;
  },
  sessions_file_delete: (payload) => {
    const rel = String(payload.rel ?? "");
    const at = SESSION_ENTRIES.findIndex((entry) => entry.relPath === rel);
    if (at !== -1) {
      SESSION_ENTRIES.splice(at, 1);
    }
    return null;
  },
  // The move Rust makes by writing two frontmatter keys, faked by mutating the
  // fixture the detail answers from — so a drag in `bun dev` lands where it was
  // dropped and stays there across the re-read the board does afterwards. The
  // numbering is the real `drop_order`'s: midpoint between neighbours, a whole
  // step past the ends, and 1 (never 0) into an empty column, because 0 is what
  // a file with no `order:` reads as.
  sessions_task_move: (payload) => {
    const rel = String(payload.rel ?? "");
    const status = String(payload.status ?? "");
    const index = Number(payload.index ?? 0);
    const card = SESSION_TASKS.find((task) => task.relPath === rel);
    if (card === undefined) {
      return null;
    }
    const column = SESSION_TASKS.filter(
      (task) => task.status === status && task.relPath !== rel,
    ).sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
    const at = Math.min(Math.max(index, 0), column.length);
    const before = at > 0 ? column[at - 1]?.order : undefined;
    const after = column[at]?.order;
    card.status = status;
    card.order =
      before === undefined && after === undefined
        ? 1
        : before === undefined
          ? (after as number) - 1
          : after === undefined
            ? before + 1
            : before + (after - before) / 2;
    // A card keeper has now written an `order:` into owns that number, whatever
    // it read as before — which is the one thing the real write changes about
    // how the card renders.
    card.orderIsOwn = true;
    return null;
  },
  // The reference picker (FR-265). Candidates come from the SAME fixture the
  // tree renders, plus two vault rows, so a file created in `bun dev` is
  // referenceable a moment later — the one behaviour a viewing aid can show
  // that a screenshot cannot.
  //
  // The filter here is a crude prefix of `add_ref::matches`: it understands
  // `tag:` and ANDs plain words, and it does NOT walk the tag hierarchy. That
  // is a stated limit, not an oversight — a second matcher that agreed with
  // Rust in every corner would be a second matcher to keep in step, and the
  // budget/truncation reasoning that makes the real one live in Rust does not
  // apply to sixteen rows.
  sessions_ref_candidates: (payload) => {
    const query = String(payload.query ?? "")
      .toLowerCase()
      .split(/\s+/)
      .filter((word) => word !== "");
    const rows = [
      ...SESSION_ENTRIES.filter((entry) => !entry.isDir).map((entry) => ({
        kind: "file",
        target: entry.relPath,
        label: entry.name,
        detail: entry.parent,
        tags: [] as string[],
        mtimeMs: entry.mtimeMs,
        // The write fence's own answer (AD-113), read off the row that already
        // carries it rather than recomputed from the path.
        promotable: entry.locked !== null,
      })),
      {
        kind: "note",
        target: "Keeper work",
        label: "Keeper work",
        detail: "10-notes/projects",
        tags: ["project/keeper", "work"],
        mtimeMs: ago(90),
        promotable: false,
      },
      {
        kind: "recording",
        target: "Standup 2026-08-12",
        label: "Standup 2026-08-12",
        detail: "30-recordings",
        tags: ["work"],
        mtimeMs: ago(2_880),
        promotable: false,
      },
    ];
    const targets = SESSION_ENTRIES.filter(
      (entry) => !entry.isDir && entry.relPath.endsWith(".md") && entry.parent === "",
    ).map((entry) => entry.relPath);
    return {
      candidates: rows.filter((row) =>
        query.every((word) =>
          word.startsWith("tag:")
            ? row.tags.some((tag) => tag.toLowerCase().includes(word.slice(4)))
            : `${row.label} ${row.detail}`.toLowerCase().includes(word),
        ),
      ),
      targets,
      defaultTarget: "references.md",
      truncated: false,
    };
  },
  sessions_ref_add: (payload) => {
    const req = (payload.req ?? {}) as Record<string, unknown>;
    const kind = String(req.kind ?? "file");
    const target = String(req.target ?? "");
    const label = req.label === null || req.label === undefined ? null : String(req.label);
    const promoted =
      req.promote === true ? `artifacts/${target.slice(target.lastIndexOf("/") + 1)}` : null;
    const dest = promoted ?? target;
    // The three forms `add_ref::line` writes, restated — the same acceptable
    // copy as `sessions_file_new`'s namer, for the same reason: this file never
    // runs in the app, and a mock that echoes a plausible line is still a mock
    // that renders the confirmation.
    const line =
      kind === "note" || kind === "recording"
        ? label === null
          ? `- [[${target}]]`
          : `- [[${target}|${label}]]`
        : kind === "external"
          ? label === null
            ? `- ${target}`
            : `- [${label}](${target})`
          : `- [${label ?? dest.slice(dest.lastIndexOf("/") + 1)}](${dest})`;
    const file = String(req.file ?? "references.md");
    if (!SESSION_ENTRIES.some((entry) => entry.relPath === file)) {
      SESSION_ENTRIES.push(sessionEntry(file, false, 140, 0));
    }
    if (promoted !== null && !SESSION_ENTRIES.some((entry) => entry.relPath === promoted)) {
      SESSION_ENTRIES.push(sessionEntry(promoted, false, 900, 0));
    }
    return { file, line, promoted };
  },
  // The three markdown widgets (FR-264). The mock answers by tag and ignores
  // the callout's argument, and that is a stated limit rather than an oversight:
  // parsing a query here would be the second parser AD-20 forbids, and the one
  // thing `bun dev` needs from this command is that a `> [!board]` in a note
  // draws cards a person can drag.
  notes_widget: (payload) => {
    const kind = String(payload.kind ?? "board");
    const tag = kind === "board" ? "task" : kind === "log" ? "log" : "ref";
    const rows = WIDGET_NOTES.filter((row) => row.tags.includes(tag));
    // Rust's own order per kind: a board by `order` then title, a log by path
    // descending (the filename carries the date), references by title.
    return kind === "board"
      ? [...rows].sort((a, b) => a.order - b.order || a.title.localeCompare(b.title))
      : kind === "log"
        ? [...rows].sort((a, b) => b.path.localeCompare(a.path))
        : [...rows].sort((a, b) => a.title.localeCompare(b.title));
  },
  // The same arithmetic `sessions_task_move` fakes, over note ids instead of
  // paths — which is the one difference between the two write paths that a
  // surface can see.
  notes_widget_move: (payload) => {
    const noteId = String(payload.noteId ?? "");
    const status = String(payload.status ?? "");
    const index = Number(payload.index ?? 0);
    const card = WIDGET_NOTES.find((row) => row.id === noteId);
    if (card === undefined) {
      return null;
    }
    const column = WIDGET_NOTES.filter(
      (row) => row.tags.includes("task") && row.status === status && row.id !== noteId,
    ).sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
    const at = Math.min(Math.max(index, 0), column.length);
    const before = at > 0 ? column[at - 1]?.order : undefined;
    const after = column[at]?.order;
    card.status = status;
    card.order =
      before === undefined && after === undefined
        ? 1
        : before === undefined
          ? (after as number) - 1
          : after === undefined
            ? before + 1
            : before + (after - before) / 2;
    card.orderIsOwn = true;
    return null;
  },
  /**
   * The folder listing `openNoteForFile` looks a session file's note id up in
   * (Story 45.18, reached by FR-274).
   *
   * Answers only for the session vault's own directory. Any other vault or
   * folder gets an empty listing rather than borrowing these rows: the bridge
   * matches on the exact vault-relative path, and a table that answered the
   * same nine notes everywhere would make a wrong resolution look right.
   */
  notes_tree: (payload) => {
    const relDir = String(payload.relDir ?? "");
    const inSession =
      String(payload.vaultId ?? "") === SESSION_VAULT_ID && relDir === SESSION_ZONE_DIR;
    return { relDir, dirs: [], notes: inSession ? sessionNotes() : [] };
  },
  notes_body_read: (payload) => {
    const row = NOTES.find(([id]) => id === payload.noteId);
    if (row === undefined) {
      // A session file opened as a note. The mock has no bytes to read, and
      // FR-98 says a note's title IS its first body line, so that line is the
      // honest whole of what this fixture knows.
      const note = sessionNotes().find((each) => each.id === payload.noteId);
      return note === undefined ? null : { rev: "rev-mock", text: `# ${note.title}\n` };
    }
    const [, title, body] = row;
    const rest = String(body).split("\n").slice(1).join("\n");
    return { rev: "rev-mock", text: `# ${title}\n${rest}` };
  },
  // The tree asks this once per folder it opens, so a table would answer the
  // root's own rows for every expansion and the tree would repeat itself
  // down the pane. `state`, `detail` and `write` are the listing's own fields
  // and not the entries' — the folder that cannot be written into is a
  // different fact from the file that cannot be.
  sync_browse: (payload): FilesListingVm => {
    const subpath = String(payload.subpath ?? "");
    return {
      profileId: String(payload.id ?? "p1"),
      subpath,
      state: "listed",
      entries: subpath === "" ? ENTRIES : (CHILDREN[subpath] ?? []),
      detail: null,
      truncated: false,
      write: { writable: true, reason: null, caveat: null, caveatShort: null },
    };
  },
};

/** True when a real Tauri shell is already answering. */
function realShellPresent(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export function installMockShell(): void {
  if (realShellPresent()) {
    return;
  }
  mockIPC((command, payload) => {
    const handler = HANDLERS[command];
    const answer =
      handler !== undefined
        ? handler((payload ?? {}) as Record<string, unknown>)
        : command in ANSWERS
          ? ANSWERS[command]
          : fallback(command);
    // One line per call, so a screen that looks wrong can be traced to the
    // command it asked for rather than guessed at.
    console.debug("[mock-shell]", command, payload ?? "", "→", answer);
    return answer;
  });
  document.documentElement.dataset.mockShell = "on";
}

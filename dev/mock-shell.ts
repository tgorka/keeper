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
 * **It boots into the shell** (story 56.14). Until then it did not: `App` reads
 * `encryption_posture` on mount and `session_restore` before that, and neither
 * was answered here — so every `bun run dev` stopped on the first-run at-rest
 * encryption card, and the Files pane, the folder settings and every other
 * surface this file has fixtures for were unreachable without a real install on
 * a Mac. That is the exact round trip the paragraphs above say this file exists
 * to end, and it was still there. See the two answers at the head of `ANSWERS`
 * for what they claim and, deliberately, what they do not.
 *
 * **Dev only, and structurally so.** The import is behind `import.meta.env.DEV`
 * in `main.tsx`, so Rollup drops the whole module from a production build; there
 * is no runtime flag to get wrong. It also refuses to install when a real shell
 * is present, so `tauri dev` is never quietly served fixtures.
 */

import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  AccountVm,
  BotAttachmentVm,
  BotAuditRowVm,
  BotCommandPreviewVm,
  BotCommandRowVm,
  BotConversationVm,
  BotDeliverableVm,
  BotGrantSaveReq,
  BotGrantVm,
  BotMessageVm,
  BotModelVm,
  BotProbeVm,
  BotProviderVm,
  BotSessionVm,
  BotStreamEvent,
  BotVm,
  CapabilitiesVm,
  FileSizeVm,
  FilesEntrySyncVm,
  FilesEntryVm,
  FilesListingVm,
  FilesReleaseVm,
  GrantScope,
  PacedWorkVm,
  SessionSpaceFilesVm,
  SessionSpaceFileVm,
  SessionSpaceVm,
  SyncFootprintVm,
  SyncProfileReq,
  SyncProfileVm,
  SyncVerifyVm,
  TaskBatchEntryVm,
  TaskBatchIdReq,
  TaskBatchReceiptVm,
  TaskListingVm,
  TaskRunVm,
  TaskSaveReq,
  TaskSchedulePreviewVm,
  TaskVm,
  VoiceStateVm,
  VoiceUnavailableVm,
  VoiceWakeVm,
} from "@/lib/ipc/client";
import { DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

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
    // Story 56.9: no release standing, because release is a fact about content
    // keeper itself put here and none of these rows is that. The two rows that
    // are get one below.
    release: null,
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
  release: FilesReleaseVm | null = null,
): FilesEntryVm {
  return { ...browseEntry(name, false, size), sync, lfsOid, release };
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

/** Verbatim from `keeper_sync::engine::ReleaseSchedule::sentence`, for the
 *  reason {@link lfsEntry} gives. The deadline beside it is `Date.now()`-relative
 *  and not a fixed instant, unlike every other timestamp in this file: a
 *  countdown frozen at a stored epoch would render `due` for ever, which is the
 *  one thing the cell it is here to show cannot demonstrate. */
const RELEASE_DUE_SENTENCE =
  "keeper lets this content go on the first sync after the time runs out; the copy stays here until then";
const RELEASE_PINNED_SENTENCE =
  "This path is pinned, so keeper keeps its content on this computer until the pin is lifted";
/** `ReleaseSchedule::ModeKeeps` — one of the two causes whose word is `Kept` and
 *  whose Release `Engine::dehydrate_entry` refuses on the mode gate, which is why
 *  the pane withholds the button for it (story 56.14). Verbatim, like the two
 *  above: the whole point of the withheld button is that this sentence is what
 *  explains it instead. */
const RELEASE_MODE_KEEPS_SENTENCE =
  "This folder is set to keep large-file content on this computer, so nothing is released on a clock";

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
  // The two shapes Story 56.9's release cell takes, so both can be looked at:
  // one row counting down to a real instant, and one row whose words say there
  // is no clock at all. `hold` and `releasesAfterMs` are never both set —
  // `ReleaseSchedule` guarantees it and this harness must not be the one place
  // that reads otherwise.
  lfsEntry(
    "master-2026-07.wav",
    { bytes: 4_294_967_296, label: "4.3 GB" },
    { status: "materialized", detail: MATERIALIZED_SENTENCE },
    null,
    {
      releasesAfterMs: Date.now() + 23 * 3_600_000 + 30 * 60_000,
      hold: null,
      detail: RELEASE_DUE_SENTENCE,
    },
  ),
  lfsEntry(
    "master-2026-08.wav",
    { bytes: 4_294_967_296, label: "4.3 GB" },
    { status: "materialized", detail: MATERIALIZED_SENTENCE },
    null,
    { releasesAfterMs: null, hold: "Pinned", detail: RELEASE_PINNED_SENTENCE },
  ),
  // The row whose Release the pane WITHHOLDS (story 56.14): the folder's mode
  // keeps large-file content, so `Engine::dehydrate_entry` refuses on the mode
  // gate before anything else runs. Its word is `Kept`, which is what the pane
  // reads; a `releaseTtlMs = 0` row would say `Manual` and keep the button,
  // because that one releases on request. Here so the withheld state can be
  // LOOKED at — the two rows above it both offer Release, so without this one the
  // gate has nothing on screen to show.
  lfsEntry(
    "raw-2026-08.tiff",
    { bytes: 268_435_456, label: "268 MB" },
    { status: "materialized", detail: MATERIALIZED_SENTENCE },
    null,
    { releasesAfterMs: null, hold: "Kept", detail: RELEASE_MODE_KEEPS_SENTENCE },
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
  // The full `CapabilitiesVm`, `satisfies`-checked for this file's own stated
  // reason: it was `{ notes, recording, sync, chat }` — one key that does not
  // exist on the type, and eight that do missing — which typechecked only
  // because `ANSWERS` is `Record<string, unknown>`. The cost was not cosmetic:
  // `sessions` is what gates BOTH the sessions board and the ⌘8 Tasks pane
  // (`app-shell.tsx`), so every task and paced-work fixture below this line was
  // unreachable in `bun run dev` — the one screen this file exists to make
  // visible on Linux. Now a flag added in Rust breaks the build here instead.
  //
  // `?platform=phone` on the dev URL answers the iPhone's shape instead (Epic
  // 65, AD-189): every `cfg!(desktop)` flag false and `bots` true, exactly as
  // `keeper/src/ipc.rs` computes it there. Since AD-189 that answer — not the
  // window's width — is what puts the shell in the phone tier, so it is the
  // only way to look at the phone's landscape shape here, and it is what
  // `dev/measure-bots.ts --phone` drives.
  capabilities:
    new URLSearchParams(window.location.search).get("platform") === "phone"
      ? ({ ...DEFAULT_CAPABILITIES, bots: true } satisfies CapabilitiesVm)
      : ({
          trayIcon: true,
          globalHotkey: true,
          launchAtLogin: true,
          inAppUpdater: true,
          nativeMenuBar: true,
          bridgeSidecar: true,
          revealInFileManager: true,
          recording: true,
          sync: true,
          notes: true,
          sessions: true,
          // Epic 61: `true`, and this is the trap the comment above names. As `false`
          // every bots fixture below would be unreachable in `bun run dev` — the
          // pane, the picker, the composer and the fake stream all sit behind it.
          bots: true,
          // Epic 62: the drive half, `desktop && sync` in Rust. `true` here so the
          // grant bar, the tool rows and the reveal control are reachable in `bun
          // run dev`; flip to `false` to see the phone's shape of the same pane.
          botTools: true,
          overlayTitleBar: true,
        } satisfies CapabilitiesVm),
  // ---------------------------------------------------------------------------
  // The two answers that decide WHICH screen boots (`src/App.tsx`
  // `renderContent`). Without them every `bun run dev` stopped at the first-run
  // at-rest-encryption card and the shell — Files, folder settings, notes,
  // sessions, every pane below — was unreachable without a real install on a
  // Mac, which is the exact round trip this file exists to end. Both fell
  // through to `fallback`, whose `null` is the WORST answer for each of them:
  // `session_restore` → `null` makes `useSessionRestore`'s `accounts.length`
  // throw (caught, so zero accounts), and `encryption_posture` → `null` is
  // spelled "unchosen", which is precisely the card.
  //
  // These are harness fixtures, not a signed-in user: nothing here is a real
  // account, a real homeserver or a real posture, and no Keychain or SDK session
  // exists behind them. Anything that needs actual Matrix state (the timeline,
  // room list, crypto) is answered elsewhere in this table or falls through.
  // ---------------------------------------------------------------------------

  /**
   * One restored account, so `hasAccount` is true and `App` mounts `<AppShell />`
   * instead of the encryption card or the login screen. Shape from
   * `src/lib/ipc/gen/AccountVm.ts`, annotated `satisfies` for the reason
   * `sync_profiles` states: `dev/` is inside `tsconfig.json`'s `include`, so a
   * field added in Rust breaks the typecheck here rather than blanking a window
   * at run time.
   *
   * Exactly ONE account. A second would exercise the account switcher and the
   * per-account hue bar, which is tempting — but it also changes what the
   * merged inbox and the filter chips render, and a viewing aid should show the
   * ordinary case unless asked. The `hueIndex` is deliberately not 0 so the
   * 3 px chat-row edge bar is visible rather than defaulting invisibly.
   */
  session_restore: [
    {
      accountId: "01J8ACCOUNTMOCKAAAAAAAAAAA",
      userId: "@harness:example.org",
      homeserverUrl: "https://matrix.example.org",
      hueIndex: 3,
      provider: "password",
    },
  ] satisfies AccountVm[],

  /**
   * Chosen-OFF, i.e. `false` and not `null`.
   *
   * `null` means "the user has never answered", which is what raises the
   * first-run card; `false` means "answered, FileVault only" — the honest
   * default and the same value `App`'s own read-failure path falls back to
   * (`App.tsx`: "treat the posture as chosen-off so the user is never trapped
   * before login"). Off rather than on because a harness that claims at-rest
   * encryption is enabled would render passphrase state no fixture here can
   * back.
   *
   * Load-bearing even though `session_restore` above already opens the shell:
   * the posture is read unconditionally on mount, and a hand-driven
   * `clear()`/sign-out under the mock shell must land on the login screen
   * rather than back on the card.
   */
  encryption_posture: false,

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
  // Annotated `satisfies SyncProfileVm[]` since Story 56.12, which is a change
  // of policy for this row and worth the sentence. It used to answer six of the
  // twenty-odd fields on purpose, on the argument that under-specified is the
  // harmless failure. That stopped being true the moment the folder settings
  // form became reachable under the mock shell: it calls `profile.excludes
  // .join`, and `undefined.join` blanks the whole window. The rule the Files
  // listing follows — annotate the shapes a consumer DEREFERENCES — now applies
  // here, and `satisfies` is what stops the next field added in Rust from
  // quietly reintroducing the same white page.
  sync_profiles: [
    {
      id: "p1",
      name: "tgdrive",
      localPath: "/Volumes/merope/tgdrive",
      remoteUrl: "git@electra:tgdrive.git",
      branch: "main",
      direction: "bidirectional",
      lane: "main",
      subpaths: [],
      excludes: ["*.tmp", ".DS_Store"],
      removable: true,
      lfsMode: "materialize",
      lfsThresholdBytes: 4 * 1024 * 1024,
      virtualPatterns: ["scans/**"],
      virtualOverBytes: 8 * 1024 * 1024,
      releaseTtlMs: 24 * 60 * 60 * 1000,
      settleMs: null,
      effectiveSettleMs: 10_000,
      pollIntervalMs: null,
      effectivePollIntervalMs: 15_000,
      tags: [],
      commitSubjectTemplate: "",
      authorOverride: null,
      enabled: true,
      notes: true,
      notesSubfolder: "notes",
      recordings: true,
      recordingsSubfolder: "recordings",
      sessions: true,
      sessionsSubfolder: "60-sessions",
      folderOwned: [],
    },
    {
      // The second folder exists to make the owned-elsewhere marker reachable
      // by hand: its `.keeper/keeper.toml` decides the virtualization policy,
      // so opening its settings shows those controls disabled with the reason
      // beside them, and a save from that form omits the keys.
      id: "p2",
      name: "neuradrive",
      localPath: "/Volumes/merope/neuradrive",
      remoteUrl: "git@electra:neuradrive.git",
      branch: "main",
      direction: "bidirectional",
      lane: "main",
      subpaths: [],
      excludes: [],
      removable: false,
      lfsMode: "pointerOnly",
      lfsThresholdBytes: 4 * 1024 * 1024,
      virtualPatterns: ["raw/**", "!raw/keep-these/**"],
      virtualOverBytes: 32 * 1024 * 1024,
      releaseTtlMs: 0,
      settleMs: null,
      effectiveSettleMs: 5_000,
      pollIntervalMs: null,
      effectivePollIntervalMs: 15_000,
      tags: [],
      commitSubjectTemplate: "",
      authorOverride: null,
      enabled: true,
      notes: false,
      notesSubfolder: null,
      recordings: false,
      recordingsSubfolder: "recordings",
      sessions: false,
      sessionsSubfolder: "60-sessions",
      folderOwned: ["releaseTtlMs", "virtualOverBytes", "virtualPatterns"],
    },
    {
      // The third folder is the floor-only state (Story 56.16): a size floor
      // and no patterns at all, which is what the owner had saved when every
      // one of his 16 GB downloaded anyway. Its settings form is the only
      // place `SYNC_VIRTUAL_OVER_ALONE_NOTE` and the between-the-two-sizes
      // line can be looked at without typing the state in by hand, and a state
      // nothing can show is a state nobody reviews.
      id: "p3",
      name: "tgdrive-light",
      localPath: "/Volumes/merope/tgdrive-light",
      remoteUrl: "git@electra:tgdrive.git",
      branch: "main",
      direction: "bidirectional",
      lane: "main",
      subpaths: [],
      excludes: [],
      removable: false,
      lfsMode: "materialize",
      lfsThresholdBytes: 262_144,
      virtualPatterns: [],
      virtualOverBytes: 1024 * 1024,
      releaseTtlMs: 24 * 60 * 60 * 1000,
      settleMs: null,
      effectiveSettleMs: 10_000,
      pollIntervalMs: null,
      effectivePollIntervalMs: 15_000,
      tags: [],
      commitSubjectTemplate: "",
      authorOverride: null,
      enabled: true,
      notes: false,
      notesSubfolder: null,
      recordings: false,
      recordingsSubfolder: "recordings",
      sessions: false,
      sessionsSubfolder: "60-sessions",
      folderOwned: [],
    },
  ] satisfies SyncProfileVm[],
  sync_statuses: [
    { profileId: "p1", state: "idle", pending: 0, lastSyncMs: ago(3) },
    { profileId: "p2", state: "idle", pending: 0, lastSyncMs: ago(41) },
    { profileId: "p3", state: "idle", pending: 0, lastSyncMs: ago(12) },
  ],
  sync_problems: { profiles: [] },
  sync_git_status: { available: true, path: "/usr/bin/git", version: "2.53.0" },
  sync_device: { id: "01DEVICE", label: "hesperia" },
  // No token stored for either folder. Without this the edit form's keychain
  // read falls through to `fallback`, which answers `null` — the same answer,
  // by accident rather than on purpose. Spelled out so the form's opening read
  // is a fixture rather than a coincidence.
  sync_get_credential: null,
  config_layers: { overrides: [], faults: [], mainFolder: null },
  // The folder card's footprint sentence and the Check-files report, both of
  // which used to fall through to `fallback` and render nothing at all under
  // the mock shell — so the two surfaces Story 56.12 added counts to could not
  // be looked at on Linux without a Tauri build.
  sync_footprint: {
    onDisk: 218_000_000_000n,
    lfsCache: 91_000_000_000n,
    reclaimable: 74_000_000_000n,
    scratch: 1_400_000_000n,
    content: 410_000_000_000n,
    virtualPaths: 118,
    materializedPaths: 3,
    onDiskLabel: "218 GB",
    lfsCacheLabel: "91 GB",
    reclaimableLabel: "74 GB",
    scratchLabel: "1.4 GB",
    contentLabel: "410 GB",
  } satisfies SyncFootprintVm,
  sync_verify: {
    checked: 1_284,
    virtualPaths: 118,
    problems: [],
  } satisfies SyncVerifyVm,
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

/**
 * Every state a Tasks row can be in (Epic 57, Story 57.6, AD-137).
 *
 * The point of this block is the *host* column. That decision is
 * `keeper_core::tasks::task_host`'s and it is not re-derived here — these are
 * its outputs, quoted from the `HOST_SENTENCE_*` and `UNHOSTED_*` constants in
 * `keeper-core/src/tasks.rs` — so what a browser on Linux shows is what the
 * real command would answer on the machine each row describes. A sentence typed
 * loosely here would make the pane look right while the app claimed a host it
 * does not have, which is the exact failure AD-137 exists to prevent.
 *
 * Seven rows and one unreadable one, because the surface has that many branches:
 * a scheduled task with a window ahead of it, one mid-run holding a lease, one
 * hosted by an enabled unit that does **not** linger (so its schedule stops at
 * logout), one whose last run failed, a manual one nothing schedules, a
 * switched-off one, an **unhosted** one that looks enabled and will never fire,
 * and a row a newer keeper wrote that this build shows rather than drops
 * (NFR-43).
 */
const HOST_SENTENCES = {
  daemon: "the keeper-syncd unit on this machine runs this, logged in or not",
  daemonUntilLogout:
    "the keeper-syncd unit on this machine runs this while you are logged in — lingering is off, so its schedule stops when your session ends",
  app: "keeper runs this — only while keeper is running",
  appOtherDataDir:
    "keeper runs this — only while keeper is running; the keeper-syncd unit here reads a different data directory, so it never sees this task",
  onRequest: "nothing schedules this — it runs when you ask",
  off: "switched off — nothing runs this, not even a request",
  unhosted: "nothing will run this",
} as const;

const UNHOSTED_FOLDER_GONE = "it names a folder keeper does not sync, so no host here can run it";

/** How long from now, in minutes — `ago`'s mirror, for a window still ahead. */
const ahead = (minutes: number) => NOW + minutes * 60_000;

const TASK_RUNS: Record<string, TaskRunVm[]> = {
  // A healthy nightly: three clean passes.
  "01JNIGHTLYSYNCAAAAAAAAAAAA": [3, 1_443, 2_883].map((minutes, index) => ({
    id: 300 - index,
    taskId: "01JNIGHTLYSYNCAAAAAAAAAAAA",
    startedMs: ago(minutes),
    finishedMs: ago(minutes - 1),
    outcome: "ok",
    unknownOutcome: null,
    detail: "3 synced, 0 already syncing, 0 waiting, 0 failed",
    host: "01DEVICE#4188",
  })),
  // Mid-run: the newest attempt has no `finishedMs` and no outcome, which is
  // what "running now" is made of. The lease on the row below matches it.
  "01JRELEASESWEEPBBBBBBBBBBB": [
    {
      id: 411,
      taskId: "01JRELEASESWEEPBBBBBBBBBBB",
      startedMs: ago(2),
      finishedMs: null,
      outcome: null,
      unknownOutcome: null,
      detail: null,
      host: "01DEVICE#912",
    },
    {
      id: 410,
      taskId: "01JRELEASESWEEPBBBBBBBBBBB",
      startedMs: ago(362),
      finishedMs: ago(360),
      outcome: "ok",
      unknownOutcome: null,
      detail: "looked at 1 284 objects, released 96, reclaimed 74 GB",
      host: "01DEVICE#912",
    },
  ],
  // A failure, with the tally first and the reason after it — the shape
  // `perform_sync_task` composes.
  "01JVAULTPUSHCCCCCCCCCCCCCC": [
    {
      id: 502,
      taskId: "01JVAULTPUSHCCCCCCCCCCCCCC",
      startedMs: ago(9),
      finishedMs: ago(8),
      outcome: "failed",
      unknownOutcome: null,
      detail:
        "0 synced, 0 already syncing, 0 waiting, 1 failed: could not resolve host git.tgorka.dev",
      host: "01DEVICE#4188",
    },
    {
      id: 501,
      taskId: "01JVAULTPUSHCCCCCCCCCCCCCC",
      startedMs: ago(69),
      finishedMs: ago(68),
      outcome: "deferred",
      unknownOutcome: null,
      detail: "0 synced, 0 already syncing, 1 waiting, 0 failed",
      host: "01DEVICE#4188",
    },
  ],
  // A run a newer keeper recorded: the spelling is carried and rendered
  // verbatim rather than flattened to "unknown".
  "01JARCHIVETRIMDDDDDDDDDDDD": [
    {
      id: 601,
      taskId: "01JARCHIVETRIMDDDDDDDDDDDD",
      startedMs: ago(1_500),
      finishedMs: ago(1_499),
      outcome: null,
      unknownOutcome: "sublimated",
      detail: "recorded by keeper 0.9.0",
      host: "01DEVICE#77",
    },
  ],
};

const TASKS: TaskVm[] = [
  {
    id: "01JNIGHTLYSYNCAAAAAAAAAAAA",
    kind: "sync",
    mode: "scheduled",
    enabled: true,
    profileId: null,
    profile: null,
    schedule: "0 3 * * *",
    description: "nightly backup of the photos",
    onMissed: "run_now",
    missedDelayMs: null,
    nextDueMs: ahead(957),
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 2000,
    lastRun: TASK_RUNS["01JNIGHTLYSYNCAAAAAAAAAAAA"][0],
    host: { kind: "app", sentence: HOST_SENTENCES.app, reason: null },
  },
  {
    id: "01JRELEASESWEEPBBBBBBBBBBB",
    kind: "release",
    mode: "scheduled",
    enabled: true,
    profileId: "p1",
    profile: "keeper",
    schedule: "every 6h",
    // Blank rather than absent, and the only fixture that is: a person cleared
    // the box once. It must render as nothing, exactly as a null does — the
    // store keeps the two apart and the view deliberately does not.
    description: "",
    // skip, because a release sweep deletes: a window nobody
    // served is better dropped than served at an instant nobody chose.
    onMissed: "skip",
    // Stored even though `skip` never reads it, which is the store's rule: a
    // policy change must not throw away a number somebody typed. So the ⌘8 form
    // shows the box on this row too, because a value it holds may never be
    // hidden behind a policy that ignores it.
    missedDelayMs: 45 * 60_000,
    nextDueMs: ahead(358),
    // Mid-run and holding the lease: the other host on this machine cannot
    // claim this task until the lease expires or this one hands it back.
    runningHost: "01DEVICE#912",
    leaseUntilMs: ahead(58),
    updatedMs: NOW - 3000,
    lastRun: TASK_RUNS["01JRELEASESWEEPBBBBBBBBBBB"][0],
    host: { kind: "daemon", sentence: HOST_SENTENCES.daemon, reason: null },
  },
  {
    id: "01JLOGROTATEFFFFFFFFFFFFFF",
    kind: "release",
    mode: "scheduled",
    enabled: true,
    profileId: null,
    profile: null,
    schedule: "0 4 * * *",
    description: null,
    // delay, so a 04:00 sweep missed overnight does not fire in the
    // same second the machine comes back.
    onMissed: "delay",
    // Four hours, and the reason this fixture exists: it is the only row whose
    // delay is NOT keeper's own thirty minutes, so the form's note has to compose
    // its sentence rather than recite a constant.
    missedDelayMs: 4 * 60 * 60_000,
    nextDueMs: ahead(1_017),
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 4000,
    // Never run yet, and the point of the row is the sentence: the unit IS
    // enabled and DOES read this database, so the daemon really is the host —
    // but `loginctl enable-linger` was never run here, so a `--user` unit dies
    // with the session and the 04:00 sweep stops the first time this person
    // logs out. `systemctl --user is-enabled` cannot see that; the file
    // `enable-linger` creates can, and that is what the app stats.
    lastRun: null,
    host: { kind: "daemon", sentence: HOST_SENTENCES.daemonUntilLogout, reason: null },
  },
  {
    id: "01JVAULTPUSHCCCCCCCCCCCCCC",
    kind: "sync",
    mode: "scheduled",
    enabled: true,
    profileId: "p2",
    profile: "notes",
    schedule: "@hourly",
    description: null,
    onMissed: "run_now",
    missedDelayMs: null,
    nextDueMs: ahead(51),
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 5000,
    lastRun: TASK_RUNS["01JVAULTPUSHCCCCCCCCCCCCCC"][0],
    // The Linux default: a unit IS enabled here, and it reads a different data
    // directory, so it never sees this row. Saying only "keeper runs this"
    // would leave the user believing the enabled unit is the host.
    host: { kind: "app", sentence: HOST_SENTENCES.appOtherDataDir, reason: null },
  },
  {
    id: "01JARCHIVETRIMDDDDDDDDDDDD",
    kind: "release",
    mode: "manual",
    enabled: true,
    profileId: "p3",
    profile: "archive",
    // Remembered, not obeyed: a manual task's schedule is stored and ignored,
    // so the row must not read as though something will fire it.
    schedule: "@weekly",
    description: "trim the archive by hand, quarterly",
    onMissed: "run_now",
    missedDelayMs: null,
    nextDueMs: null,
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 6000,
    lastRun: TASK_RUNS["01JARCHIVETRIMDDDDDDDDDDDD"][0],
    host: { kind: "onRequest", sentence: HOST_SENTENCES.onRequest, reason: null },
  },
  {
    id: "01JORPHANEDTASKEEEEEEEEEEE",
    kind: "sync",
    mode: "scheduled",
    enabled: true,
    // The folder is gone: the id names no current profile, so `profile` is null
    // and nothing on this machine can run the task. It still looks enabled,
    // which is the whole reason the row has to say otherwise.
    profileId: "01JNOSUCHPROFILE",
    profile: null,
    schedule: "30 4 * * *",
    // Named, and this is the row where a name earns its keep: the id is a ULID
    // nobody chose and the folder it pointed at is gone, so without this there
    // is nothing on the row a person could recognise it by.
    description: "push the old vault — folder was moved, needs repointing",
    onMissed: "run_now",
    missedDelayMs: null,
    nextDueMs: ahead(1_407),
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 7000,
    lastRun: null,
    host: {
      kind: "unhosted",
      sentence: HOST_SENTENCES.unhosted,
      reason: UNHOSTED_FOLDER_GONE,
    },
  },
  {
    id: "01JPAUSEDSWEEPFFFFFFFFFFFF",
    kind: "release",
    mode: "off",
    enabled: true,
    profileId: "p1",
    profile: "keeper",
    schedule: "0 2 * * *",
    description: null,
    onMissed: "run_now",
    missedDelayMs: null,
    nextDueMs: null,
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 8000,
    lastRun: null,
    // Off, and deliberately NOT unhosted: nothing is wrong with this row and
    // the user switched it off on purpose.
    host: { kind: "off", sentence: HOST_SENTENCES.off, reason: null },
  },
];

const TASK_LISTING: TaskListingVm = {
  tasks: TASKS,
  unknown: [
    {
      id: "01JTELEPORTTASKGGGGGGGGGGG",
      reason: "unknown task kind 'teleport'",
    },
  ],
};

/**
 * The sentences `keeper_core::tasks` composes for the paced class (Story 58.7),
 * copied verbatim for `HOST_SENTENCES`'s reason: this file renders what Rust
 * would send, and a paraphrase here would show a screen the app never draws.
 */
const PACED_SENTENCES = {
  scanPaced:
    "keeper looks for changes on this cadence while it is running. The cadence is a backstop and not the only trigger: a file the watcher sees settle, or a write that closes, brings the next look forward.",
  scanGoverned:
    "a scheduled sync task decides when this folder is looked at, so the paced backstop has stood down. A file the watcher sees settle still brings a look forward.",
  sweepPaced:
    "keeper deletes transfer scratch this folder will never use again, on this cadence, while it is running.",
  notesPaced:
    "the app commits this vault after the quiet window and pushes within the deadline. Only the running app paces it — keeper-syncd never does.",
  notesUnregistered:
    "keeper has no vault registered for this folder, so nothing paces it: the vault folder could not be found when the registry was last built. The registry is rebuilt at launch, and when a vault is flagged or unflagged — not when a drive comes back.",
  paused: "this folder is paused, so nothing here is paced and no cadence is in force.",
  removableClause:
    " The folder is on removable media, so nothing happens at all while the drive is away.",
} as const;

/**
 * What this host paces, as the projection would answer it: two folders, one of
 * them holding a vault and one of them paused.
 *
 * The shapes worth looking at are the negatives — a paused folder whose rows
 * carry no cadence at all, and a governed scan row whose backstop has stood
 * down (Story 58.8) — because those are the two the section's wording exists
 * for. `satisfies PacedWorkVm[]` for `sync_profiles`'s reason: `dev/` is inside
 * the typecheck, so a field added in Rust breaks the build here rather than
 * blanking a section at run time.
 */
const PACED_WORK = [
  {
    id: "scan:p1",
    kind: "scan",
    profileId: "p1",
    profile: "keeper",
    standing: "paced",
    cadence: "about every 15 seconds",
    sentence: PACED_SENTENCES.scanPaced,
  },
  {
    id: "sweep:p1",
    kind: "scratchSweep",
    profileId: "p1",
    profile: "keeper",
    standing: "paced",
    cadence: "every 1 hour",
    sentence: PACED_SENTENCES.sweepPaced,
  },
  // A vault, and the one row in the pane that says keeper-syncd will not do it.
  {
    id: "notes:p1",
    kind: "notesCadence",
    profileId: "p1",
    profile: "keeper",
    standing: "paced",
    cadence: "committed after 2 seconds of quiet, pushed within 30 seconds",
    sentence: PACED_SENTENCES.notesPaced,
  },
  // Governed: a scheduled Sync task took this folder's paced backstop, so the
  // row advertises no cadence and says which surface decides instead. On
  // removable media too, so the clause rides the governed sentence.
  {
    id: "scan:p2",
    kind: "scan",
    profileId: "p2",
    profile: "photos",
    standing: "governed",
    cadence: null,
    sentence: `${PACED_SENTENCES.scanGoverned}${PACED_SENTENCES.removableClause}`,
  },
  // The sweep carries the same clause on removable media: with the drive away
  // its own `read_dir` finds nothing, so an unhedged deletion claim would be
  // describing work that is not happening.
  {
    id: "sweep:p2",
    kind: "scratchSweep",
    profileId: "p2",
    profile: "photos",
    standing: "paced",
    cadence: "every 1 hour",
    sentence: `${PACED_SENTENCES.sweepPaced}${PACED_SENTENCES.removableClause}`,
  },
  // A vault the registry does not hold — the shape that used to claim a cadence
  // while nothing at all was pacing it.
  {
    id: "notes:p2",
    kind: "notesCadence",
    profileId: "p2",
    profile: "photos",
    standing: "unregistered",
    cadence: null,
    sentence: PACED_SENTENCES.notesUnregistered,
  },
  // Paused, and every row of the folder says the same thing: `tick` skips a
  // disabled profile before it reaches any of this work.
  {
    id: "scan:p3",
    kind: "scan",
    profileId: "p3",
    profile: "archive",
    standing: "paused",
    cadence: null,
    sentence: PACED_SENTENCES.paused,
  },
  {
    id: "sweep:p3",
    kind: "scratchSweep",
    profileId: "p3",
    profile: "archive",
    standing: "paused",
    cadence: null,
    sentence: PACED_SENTENCES.paused,
  },
] satisfies PacedWorkVm[];

/**
 * What a written schedule would do (Story 59.7). Answered from the REAL
 * dialect rather than with a fixed happy preview, because Story 58.4 already
 * shipped three mock fixtures describing schedules the parser refuses and the
 * trap is the same one: a mock that accepts everything makes the dev shell
 * the only place a bad expression looks fine. `keeper-sync/src/tasks.rs`'s
 * `every_schedule_the_dev_harness_shows_is_one_this_dialect_accepts` reads
 * every schedule literal in this file through `TaskSchedule::parse` — and it
 * extracts them by splitting on the field name, so this comment deliberately
 * does not spell that token out: it tripped the guard once by describing it.
 *
 * The refusal sentences are `TaskSchedule::parse`'s own, copied verbatim —
 * including the `{original:?}` quoting, which is Rust's `Debug` for a string
 * and therefore double quotes. A paraphrase here would let the app's real
 * wording change while the dev shell went on showing the old one.
 */
export function mockSchedulePreview(expression: string): TaskSchedulePreviewVm {
  const original = expression.trim();
  const lowered = original.toLowerCase();
  const quoted = JSON.stringify(original);
  const refuse = (sentence: string) => ({ expression, refusal: sentence, instants: [] });
  const malformed = () =>
    refuse(
      "task schedule must be a 5-field cron expression (minute hour day-of-month month day-of-week), one of @hourly, @daily or @weekly, or every <n><unit> with unit s/m/h/d, got " +
        quoted,
    );
  // Each branch settles two things: the gap between fires, and HOW MANY the
  // real command will answer with. That second one is not cosmetic. An
  // interval schedule fires `interval_ms` after the END of the previous run
  // (`tasks.rs:534-541`, re-derived by `Engine::next_task_window` from
  // `finished_ms`), so instants two and three would depend on how long the
  // first run takes — arithmetic dressed as knowledge. `preview_schedule`
  // therefore answers exactly ONE instant for an interval and up to the full
  // count for a cron pattern, which names wall-clock instants and has no such
  // dependency. A dev shell that showed three for `every 6h` would be more
  // generous than the app.
  //
  // The gap for a cron form here is a plain day: the real command walks a
  // calendar and this does not, and it says so rather than pretending. What
  // the dev shell exercises is the SHAPE of the answer and the refusals,
  // which is what the form renders.
  let everyMs: number;
  let chained: boolean;
  if (lowered.startsWith("@")) {
    const alias = { "@hourly": 3_600_000, "@daily": 86_400_000, "@weekly": 604_800_000 }[lowered];
    if (alias === undefined) {
      return malformed();
    }
    everyMs = alias;
    // Aliases desugar to cron, never to an interval — `@daily` keeps meaning
    // night rather than drifting to the last restart.
    chained = true;
  } else if (lowered.startsWith("every")) {
    const match =
      /^every\s+(\d+)\s*(s|m|h|d|sec|secs|second|seconds|min|mins|minute|minutes|hour|hours|day|days)$/.exec(
        lowered,
      );
    if (match === null) {
      return malformed();
    }
    const unit = match[2].charAt(0) as "s" | "m" | "h" | "d";
    everyMs = Number(match[1]) * { s: 1_000, m: 60_000, h: 3_600_000, d: 86_400_000 }[unit];
    // The floor and the ceiling, with the parser's own two sentences. `every
    // 30s` is in the grammar precisely so it is told about the floor rather
    // than about an unknown unit.
    if (everyMs < 60_000) {
      return refuse(
        `task schedule must not fire more often than once a minute (60000 ms), got ${quoted}`,
      );
    }
    if (everyMs > 366 * 86_400_000) {
      return refuse(
        `task schedule must not fire less often than once a year (${366 * 86_400_000} ms) — write a calendar pattern instead, got ${quoted}`,
      );
    }
    chained = false;
  } else {
    const fields = original.split(/\s+/);
    if (original === "" || fields.length !== 5) {
      return malformed();
    }
    // The one cron refusal a person actually meets: a date that parses and
    // names no instant. 30 February is the parser's own example.
    if (fields[2] === "30" && fields[3] === "2") {
      return refuse(`task schedule matches no instant, got ${quoted}`);
    }
    everyMs = 86_400_000;
    chained = true;
  }
  const from = Date.now();
  return {
    expression,
    refusal: null,
    instants: (chained ? [1, 2, 3] : [1]).map((n) => from + everyMs * n),
  };
}

// ---------------------------------------------------------------------------
// Bots (Epic 61, Story 61.4)
//
// Two tenants of different kinds, because the divergences between them are the
// whole design and a harness with one would hide half of it: the Ollama one is
// loopback with no credential (legitimate — its `/v1` layer accepts and
// discards any key) and the Hermes one is a LAN host that has a key stored.
// The Hermes bot's pane is what shows the no-grant sentence, and the Ollama
// one's is what shows the grant bar, so both are reachable in `bun run dev`.
// ---------------------------------------------------------------------------

const BOT_PROVIDERS: BotProviderVm[] = [
  {
    id: "01J8BOTPROVOLLAMAAAAAAAAAA",
    kind: "ollama",
    name: "Ollama on this machine",
    baseUrl: "http://localhost:11434",
    host: "localhost",
    isPrivate: true,
    createdMs: NOW - 86_400_000 * 12,
    health: "reachable",
    healthCheckedMs: NOW - 120_000,
    healthDetail: null,
    readTimeoutMs: null,
    hasToken: false,
  },
  {
    id: "01J8BOTPROVHERMESAAAAAAAAA",
    kind: "hermes",
    name: "Hermes on hesperia",
    baseUrl: "http://hesperia.local:8642",
    host: "hesperia.local",
    isPrivate: true,
    createdMs: NOW - 86_400_000 * 3,
    health: "reachable",
    healthCheckedMs: NOW - 600_000,
    healthDetail: null,
    readTimeoutMs: null,
    hasToken: true,
  },
];

const BOT_ROWS: BotVm[] = [
  {
    id: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    target: "llama4:8b",
    name: "Llama",
    pinOrder: 0,
    // Story 61.7: one bot wears an identity and the other does not, so the
    // harness shows both the chosen cell and the honest unchosen state.
    shape: "hollow",
    colour: "lapis",
    mark: "flask-conical",
    createdMs: NOW - 86_400_000 * 12,
  },
  {
    id: "01J8BOTBBBBBBBBBBBBBBBBBBB",
    providerId: "01J8BOTPROVHERMESAAAAAAAAA",
    target: "research",
    name: "Research",
    pinOrder: 1,
    shape: null,
    colour: null,
    mark: null,
    createdMs: NOW - 86_400_000 * 3,
  },
];

/**
 * One Ollama model with only the fields the roster varies. The nine names below
 * are the owner's real `ollama list` (Story 61.14), and what they exercise is
 * WIDTH: as chips they wrapped to two rows and every row came out of the
 * transcript, so a harness with two models could not show the defect at all.
 */
function ollamaModel(id: string, family: string, parameterSize: string): BotModelVm {
  return {
    id,
    family,
    parameterSize,
    quantization: "Q4_K_M",
    sizeBytes: null,
    contextWindow: null,
    maxOutputTokens: null,
    vision: false,
    tools: true,
    reasoning: false,
    capabilities: ["completion", "tools"],
  };
}

/** Per-bot model rosters, with the tri-state exercised on purpose: the Hermes
 *  alias reports nothing about vision, which is what an `unknown` capability
 *  looks like on screen. */
const BOT_MODELS: Record<string, BotModelVm[]> = {
  "llama4:8b": [
    {
      id: "llama4:8b",
      family: "llama4",
      parameterSize: "8.0B",
      quantization: "Q4_K_M",
      sizeBytes: 4_920_000_000,
      contextWindow: null,
      maxOutputTokens: null,
      vision: false,
      tools: true,
      reasoning: false,
      capabilities: ["completion", "tools"],
    },
    {
      id: "qwen3-vl:8b",
      family: "qwen3",
      parameterSize: "8.2B",
      quantization: "Q4_K_M",
      sizeBytes: 5_310_000_000,
      contextWindow: null,
      maxOutputTokens: null,
      vision: true,
      tools: true,
      reasoning: true,
      capabilities: ["completion", "tools", "vision", "thinking"],
    },
    ollamaModel("embeddinggemma:latest", "gemma", "300M"),
    ollamaModel("hf.co/unsloth/Qwen3.8-27B-GGUF:Q4_K_M", "qwen3", "27B"),
    ollamaModel("mythomax:13b", "llama", "13B"),
    ollamaModel("gemma3:4b", "gemma3", "4.3B"),
    ollamaModel("qwen3:4b", "qwen3", "4.0B"),
    ollamaModel("gemma4:e4b", "gemma4", "4.5B"),
    ollamaModel("qwen3.5:0.8b", "qwen3", "0.8B"),
    ollamaModel("qwen3.5:2b", "qwen3", "2.0B"),
    ollamaModel("qwen3.5:4b", "qwen3", "4.0B"),
  ],
  research: [
    {
      id: "hermes-agent",
      family: null,
      parameterSize: null,
      quantization: null,
      sizeBytes: null,
      contextWindow: 128_000,
      maxOutputTokens: 8_192,
      vision: null,
      tools: true,
      reasoning: null,
      capabilities: [],
    },
  ],
};

/** The conversations, newest activity first — the order Rust returns. */
const BOT_SESSIONS: BotSessionVm[] = [
  {
    id: "01J8BOTSESSIONAAAAAAAAAAAA",
    botId: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    title: "What changed in the drive this week",
    createdMs: NOW - 7_200_000,
    updatedMs: NOW - 3_600_000,
    archived: false,
    remoteSessionId: null,
    // Epic 63: the gateway's word on the session, absent on a local-only row.
    remoteLastActiveMs: null,
    remoteSource: null,
  },
  {
    id: "01J8BOTSESSIONBBBBBBBBBBBB",
    botId: "01J8BOTBBBBBBBBBBBBBBBBBBB",
    providerId: "01J8BOTPROVHERMESAAAAAAAAA",
    title: "Draft the release note",
    createdMs: NOW - 86_400_000,
    updatedMs: NOW - 80_000_000,
    archived: false,
    remoteSessionId: "hermes-9f21",
    // Epic 63: a session the gateway listed — written through its API door,
    // last seen moving a while ago, so the list's Remote label has a row.
    remoteLastActiveMs: NOW - 79_000_000,
    remoteSource: "api",
  },
  // Archived, so the list's Archived filter and its Unarchive verb have
  // something to act on (Story 61.6). A harness whose archive is always empty
  // only ever shows the no-matches sentence.
  {
    id: "01J8BOTSESSIONCCCCCCCCCCCC",
    botId: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    title: "Certificate renewal, last month",
    createdMs: NOW - 30 * 86_400_000,
    updatedMs: NOW - 29 * 86_400_000,
    archived: true,
    remoteSessionId: null,
    remoteLastActiveMs: null,
    remoteSource: null,
  },
];

/** One message row, so every fixture below spells the seventeen columns once. */
function botMessage(fields: {
  id: string;
  sessionId: string;
  seq: number;
  role: string;
  content: string;
  partial?: boolean;
  toolCallCount?: number;
  finishReason?: string | null;
}): BotMessageVm {
  return {
    id: fields.id,
    sessionId: fields.sessionId,
    seq: fields.seq,
    role: fields.role,
    content: fields.content,
    model: fields.role === "assistant" ? "llama4:8b" : null,
    providerId: fields.role === "assistant" ? "01J8BOTPROVOLLAMAAAAAAAAAA" : null,
    promptTokens: fields.role === "assistant" ? 412 : null,
    // Absent on purpose, so the harness shows what an endpoint that omits half
    // of `usage` looks like — the case Story 61.8 must render as absent and
    // never as zero.
    completionTokens: null,
    totalTokens: null,
    ttftMs: fields.role === "assistant" ? 240 : null,
    durationMs: fields.role === "assistant" ? 2_180 : null,
    finishReason: fields.finishReason ?? (fields.role === "assistant" ? "stop" : null),
    requestId: fields.role === "assistant" ? "chatcmpl-mock-1" : null,
    toolCallCount: fields.toolCallCount ?? 0,
    partial: fields.partial ?? false,
    createdMs: NOW - 3_600_000,
  };
}

/** The two stored conversations. The second carries a PARTIAL answer, because
 *  a stream that died is a state the pane must render and a table of happy
 *  answers would never show it. */
const BOT_CONVERSATIONS: Record<string, BotConversationVm> = {
  "01J8BOTSESSIONAAAAAAAAAAAA": {
    session: BOT_SESSIONS[0] as BotSessionVm,
    messages: [
      botMessage({
        id: "01J8BOTMSG1",
        sessionId: "01J8BOTSESSIONAAAAAAAAAAAA",
        seq: 0,
        role: "user",
        content: "What changed in the drive this week?",
      }),
      botMessage({
        id: "01J8BOTMSG2",
        sessionId: "01J8BOTSESSIONAAAAAAAAAAAA",
        seq: 1,
        role: "assistant",
        content:
          "I cannot read your folders yet — no grant is held for this bot, so nothing on the drive was looked at.",
      }),
    ],
    transcript: "local",
  },
  "01J8BOTSESSIONBBBBBBBBBBBB": {
    session: BOT_SESSIONS[1] as BotSessionVm,
    messages: [
      botMessage({
        id: "01J8BOTMSG3",
        sessionId: "01J8BOTSESSIONBBBBBBBBBBBB",
        seq: 0,
        role: "user",
        content: "Draft the release note for 0.8.25.",
      }),
      botMessage({
        id: "01J8BOTMSG4",
        sessionId: "01J8BOTSESSIONBBBBBBBBBBBB",
        seq: 1,
        role: "assistant",
        content: "keeper 0.8.25 adds the Bots surface, and",
        partial: true,
        finishReason: "failed",
      }),
    ],
    // Epic 63 (AD-181): this one is read from the gateway, and says so.
    transcript: "remote",
  },
};

/** How many follow reads the harness has answered (Story 63.7). */
let botFollowReads = 0;

/** What the fake stream says, one delta per element. */
const BOT_FAKE_ANSWER = [
  "Nothing on your drive was read: ",
  "no grant is held for this bot. ",
  "This answer came from the dev harness, ",
  "not from a model — `dev/mock-shell.ts` ",
  "is a viewing aid and never evidence.",
];

/** Milliseconds between fake deltas. Slow enough that the progressive render
 *  is visible to a human, fast enough that Stop is reachable before the end. */
const BOT_FAKE_DELTA_MS = 220;

/**
 * A channel the mock can push into.
 *
 * `mockIPC` hands the handler the payload with the real `Channel` instance in
 * it — unserialized, because nothing crosses a process here — so its
 * `onmessage` is callable directly. That bypasses the ordering index the real
 * `Channel` maintains, which is fine for a viewing aid and is exactly what
 * makes a fake stream possible with no Rust.
 */
interface MockChannel<T> {
  onmessage?: (message: T) => void;
}

/** Which fake streams are running, so `bots_chat_stop` can stop one. */
const BOT_LIVE_STREAMS = new Map<string, () => void>();

/**
 * The dev shell's answer to `bots_command_preview` (Story 61.9).
 *
 * **A fake, and a crude one on purpose.** The registry, the resolution order
 * and every sentence live in `keeper_core::bots::commands`; this exists only so
 * the `/` menu draws in a browser with no Rust behind it. It mirrors the names
 * and the descriptions — a menu of the wrong words would teach a reviewer the
 * wrong surface — and it does **not** reimplement nearest-match, availability
 * or the escape beyond what it takes to see each shape once. Read the Rust
 * tests for the rules; read this for the pixels.
 */
function mockCommandPreview(draft: string): BotCommandPreviewVm {
  const registry: readonly BotCommandRowVm[] = [
    ["new", "Start a new conversation with the bot you have chosen.", null],
    ["bot", "Switch to another bot by name.", null],
    ["model", "Send the rest of this conversation to another model.", null],
    ["metadata", "Show or hide the model, tokens and timing under each answer.", null],
    [
      "grant",
      "Choose what this bot may reach on your drive.",
      // The harness' Ollama bot reports no tool capability, which is `unknown`
      // and never `false` — so the row is runnable AND carries the caveat.
      // Hard-coded here because a fake cannot read a capability; the rule lives
      // in `keeper_core::bots::commands::availability`.
      "keeper could not read whether this model takes tools, so a grant here may reach nothing. Probe the model in Settings → Bots.",
    ],
    ["history", "List your conversations, or search them by a word.", null],
    ["help", "List every command keeper knows.", null],
  ].map(([name, description, warning]) => ({
    name: String(name),
    aliases: [],
    description: String(description),
    args: "none",
    argHint: null,
    available: true,
    reason: null,
    warning: warning === null ? null : String(warning),
  }));
  const escapeHint =
    "To send a message that starts with a slash, double it: //etc sends /etc as text.";
  const line = draft.startsWith("/") && !draft.startsWith("//") && !draft.includes("\n");
  if (!line) {
    const text = draft.startsWith("//") ? draft.slice(1) : draft;
    return { draft, verdict: { kind: "prose", text }, rows: [], note: null, escapeHint };
  }
  const token = draft.slice(1).split(/\s/, 1)[0]?.toLowerCase() ?? "";
  const rows = registry.filter((row) => row.name.startsWith(token));
  const exact = rows.find((row) => row.name === token);
  if (exact !== undefined) {
    return {
      draft,
      verdict: { kind: "command", name: exact.name, args: null },
      rows: [exact],
      note: null,
      escapeHint,
    };
  }
  if (token !== "" && rows.length === 0) {
    return {
      draft,
      verdict: {
        kind: "refusal",
        message: `keeper has no /${token} command. Nothing was sent. Type /help for the list. ${escapeHint}`,
      },
      rows: [],
      note: null,
      escapeHint,
    };
  }
  return { draft, verdict: { kind: "prose", text: draft }, rows, note: null, escapeHint };
}

/**
 * Drive one fake answer into `channel`, and return its subscription id.
 *
 * It emits the same event sequence the shell does, in the same order — an
 * `opened` carrying three already-persisted rows, a `firstToken`, N `delta`s,
 * then exactly one terminal `closed`. A Stop closes with a reason and a partial
 * row, which is the state the real driver writes.
 */
function botFakeStream(
  channel: MockChannel<BotStreamEvent>,
  session: BotSessionVm,
  question: string,
  carried: BotMessageVm[],
): string {
  const subscriptionId = `bot-stream-${Date.now()}`;
  const base = carried.length;
  const user = botMessage({
    id: `${subscriptionId}-user`,
    sessionId: session.id,
    seq: base,
    role: "user",
    content: question,
  });
  const assistant = botMessage({
    id: `${subscriptionId}-assistant`,
    sessionId: session.id,
    seq: base + 1,
    role: "assistant",
    content: "",
    partial: true,
    finishReason: null,
  });
  const send = (event: BotStreamEvent) => channel.onmessage?.(event);
  send({
    kind: "opened",
    subscriptionId,
    session,
    user,
    assistant,
  });

  let index = 0;
  let text = "";
  const timer = window.setInterval(() => {
    const slice = BOT_FAKE_ANSWER[index];
    if (slice === undefined) {
      window.clearInterval(timer);
      BOT_LIVE_STREAMS.delete(subscriptionId);
      send({
        kind: "closed",
        message: { ...assistant, content: text, partial: false, finishReason: "stop" },
        reason: null,
      });
      return;
    }
    if (index === 0) {
      send({ kind: "firstToken", afterMs: BOT_FAKE_DELTA_MS });
    }
    text += slice;
    index += 1;
    send({ kind: "delta", text: slice });
  }, BOT_FAKE_DELTA_MS);

  BOT_LIVE_STREAMS.set(subscriptionId, () => {
    window.clearInterval(timer);
    BOT_LIVE_STREAMS.delete(subscriptionId);
    send({
      kind: "closed",
      message: { ...assistant, content: text, partial: true, finishReason: "cancelled" },
      reason: "Stopped. What had arrived is kept.",
    });
  });
  return subscriptionId;
}

// --- Story 61.10's grants and audit log ------------------------------------
//
// A live subtree grant and a revoked one, so the two states Settings must tell
// apart are both on screen at once. Ids, providers and profiles are this
// harness's own, so the grouped list reads as the rows above it do.
const BOT_GRANTS: BotGrantVm[] = [
  {
    id: "01J8BOTGRANTAAAAAAAAAAAAAA",
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    botId: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    scope: { kind: "subtree", profileId: "p1", subpath: "journal/2026" },
    scopeLabel: "p1/journal/2026",
    mode: "write",
    createdMs: NOW - 86_400_000 * 2,
    revokedMs: null,
  },
  {
    id: "01J8BOTGRANTBBBBBBBBBBBBBB",
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    botId: null,
    scope: { kind: "drive" },
    scopeLabel: "the whole drive",
    mode: "read",
    createdMs: NOW - 86_400_000 * 9,
    revokedMs: NOW - 86_400_000 * 4,
  },
];

// Newest first, as Rust returns them. The pending row is the one worth looking
// at: it is written before the effect, so a row still pending is a call that
// was in flight when the process stopped (NFR-47).
const BOT_AUDIT: BotAuditRowVm[] = [
  {
    id: 3,
    startedMs: NOW - 240_000,
    finishedMs: null,
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    botId: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    sessionId: "01J8BOTSESSIONAAAAAAAAAAAA",
    messageId: null,
    tool: "write",
    path: "p1/journal/2026/monday.md",
    profileId: "p1",
    subpath: "journal/2026/monday.md",
    effect: "write",
    verdict: "allow",
    reason: null,
    grantId: "01J8BOTGRANTAAAAAAAAAAAAAA",
    outcome: "pending",
    bytes: null,
    truncated: false,
  },
  {
    id: 2,
    startedMs: NOW - 300_000,
    finishedMs: NOW - 299_880,
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    botId: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    sessionId: "01J8BOTSESSIONAAAAAAAAAAAA",
    messageId: null,
    tool: "read",
    path: "p1/journal/2026/sunday.md",
    profileId: "p1",
    subpath: "journal/2026/sunday.md",
    effect: "read",
    verdict: "allow",
    reason: null,
    grantId: "01J8BOTGRANTAAAAAAAAAAAAAA",
    outcome: "ok",
    bytes: 2048,
    truncated: false,
  },
  {
    id: 1,
    startedMs: NOW - 360_000,
    finishedMs: NOW - 359_910,
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    botId: "01J8BOTAAAAAAAAAAAAAAAAAAA",
    sessionId: "01J8BOTSESSIONAAAAAAAAAAAA",
    messageId: null,
    tool: "read",
    path: "p2/2026/raw",
    profileId: "p2",
    subpath: "2026/raw",
    effect: "read",
    verdict: "deny",
    // Rust's sentence verbatim, because that is what the real command returns.
    reason:
      "No grant covers this folder, so nothing was read or written. Add a grant for it in Settings \u2192 Bots and this bot can try again.",
    grantId: null,
    outcome: "refused",
    bytes: null,
    truncated: false,
  },
];

/** Story 61.8's persisted metadata toggle, off as it ships. */
let botMessageDetails = false;

/**
 * Which languages the faked device can recognise on-device (Epic 63): three
 * states, chosen with `?voice=many|one|none` on the dev URL, because the
 * surface has to be looked at in each — a list to choose from, a list of one,
 * and the absence with Rust's refusal explaining it. `many` is the default.
 * The hesperia probe found exactly four on a stock Mac, none of them Polish.
 */
const VOICE_ON_DEVICE_LOCALES: Record<string, string[]> = {
  many: ["en-ID", "en-PH", "en-SA", "en-US"],
  one: ["en-US"],
  none: [],
};
const voiceOnDeviceLocales =
  VOICE_ON_DEVICE_LOCALES[new URLSearchParams(window.location.search).get("voice") ?? "many"] ??
  VOICE_ON_DEVICE_LOCALES.many;
/** The faked system language: Polish, the owner's own phone. */
const VOICE_SYSTEM_LOCALE = "pl-PL";

/** Why the faked device cannot listen, or `null`. The language in force is
 *  the explicit setting when set, else the system language — in force even
 *  when refused, which is the owner's case: a Polish phone is never silently
 *  switched to English; the refusal names the list and the picker beside it
 *  is how English gets chosen. The sentence is Rust's own shape for the
 *  iPhone (`keeper_core::voice`; the noun and the download path are the
 *  platform's). */
function voiceUnavailable(): VoiceUnavailableVm | null {
  const locale = voiceWake.locale;
  if (voiceOnDeviceLocales.includes(locale)) {
    return null;
  }
  const remedy =
    voiceOnDeviceLocales.length === 0
      ? `which may add it for ${locale} or for any other language`
      : `which may add it, or choose a language this iPhone can already run on its own: ${voiceOnDeviceLocales.join(", ")}`;
  return {
    kind: "noOnDeviceRecognition",
    locale,
    message: `this iPhone has no on-device speech recognition for ${locale}, and keeper never sends your voice to a server — download that language under Settings > General > Keyboard > Dictation Languages, ${remedy}`,
  };
}

/**
 * Story 62.5's wake phrase, faked. The switch starts off and the phrase is the
 * shipped default, and both round-trip, because the flow worth looking at is
 * turning listening on and watching the chip appear. `voice_availability`
 * answers "available" here so the affordance is visible in `bun run dev`;
 * the real desktop answers `unsupported`, which hides it.
 */
let voiceWake: VoiceWakeVm = {
  enabled: false,
  phrase: "nixie",
  limits:
    "Turn listening on while keeper is in front and it keeps listening when another app is in front or the screen is locked. Siri or an app that takes the microphone pauses it and keeper resumes on its own; a phone call ends it until you open keeper again. It stops when you turn it off or when keeper is force-quit. The orange microphone indicator stays on the whole time and cannot be hidden, and listening uses battery.",
  locale: VOICE_SYSTEM_LOCALE,
  localeChosen: null,
  onDeviceLocales: voiceOnDeviceLocales,
};
/** The one watcher, so `voice_wake_set` can push the new idle snapshot. */
let voiceWatcher: MockChannel<VoiceStateVm> | null = null;
let voiceWatchSerial = 0;

/** What an idle turn looks like given the switch. */
function voiceIdle(): VoiceStateVm {
  return {
    kind: "idle",
    wake: voiceWake.enabled ? voiceWake.phrase.toLowerCase() : null,
    listeningForWake: voiceWake.enabled,
  };
}

const HANDLERS: Record<string, (payload: Record<string, unknown>) => unknown> = {
  // --- Bots (Epic 61, Story 61.4) ----------------------------------------
  //
  // Handlers rather than `ANSWERS` entries for all of them, because the two
  // things worth looking at here both depend on the payload: which bot's model
  // roster is asked for, and a chat that actually streams. A table cannot
  // stream, and a `*_subscribe`-shaped fallback would hand back an id and never
  // emit — leaving the pane on its empty state forever, which is the failure
  // §5's own note about `"sub-mock"` describes.
  bots_providers_list: () => BOT_PROVIDERS,
  bots_bots_list: () => BOT_ROWS,
  bots_sessions_list: (payload) =>
    payload.includeArchived === true
      ? BOT_SESSIONS
      : BOT_SESSIONS.filter((session) => !session.archived),
  bots_session_open: (payload) => BOT_CONVERSATIONS[String(payload.sessionId)] ?? null,
  // Story 63.7: the remote conversation is followed. The first read shows the
  // other device's question landing with the caption up; the second lands
  // its answer and stops, so the harness shows both states and never polls
  // forever.
  bots_session_follow: (payload) => {
    const open = BOT_CONVERSATIONS[String(payload.sessionId)];
    if (open === undefined || open.transcript !== "remote") {
      return { messages: null, live: false, nextPollMs: null };
    }
    botFollowReads += 1;
    const theirs = botMessage({
      id: "01J8BOTMSG5",
      sessionId: open.session.id,
      seq: open.messages.length,
      role: "user",
      content: "And the changelog line for the phone?",
    });
    if (botFollowReads % 2 === 1) {
      return { messages: [...open.messages, theirs], live: true, nextPollMs: 2000 };
    }
    const answer = botMessage({
      id: "01J8BOTMSG6",
      sessionId: open.session.id,
      seq: open.messages.length + 1,
      role: "assistant",
      content: "Bots on the phone, reachable without an account.",
      finishReason: "stop",
    });
    return { messages: [...open.messages, theirs, answer], live: false, nextPollMs: null };
  },
  // Story 61.6's four. The search really searches — titles and bodies, the two
  // things Rust matches — because a harness whose filter does nothing teaches
  // the reviewer that the filter does nothing. The three writes answer with a
  // plausible row rather than mutating the table, this file's stated rule: the
  // list re-reads after every write, and a half-mutated fixture would show a
  // list the next read contradicts.
  bots_sessions_search: (payload) => {
    const req = (payload.req ?? {}) as { text?: string; scope?: string; limit?: number };
    const needle = (req.text ?? "").toLowerCase();
    const scope = req.scope ?? "live";
    const inScope = BOT_SESSIONS.filter((session) =>
      scope === "all" ? true : scope === "archived" ? session.archived : !session.archived,
    );
    const matched = inScope.filter((session) => {
      if (needle === "") {
        return true;
      }
      if (session.title.toLowerCase().includes(needle)) {
        return true;
      }
      const held = BOT_CONVERSATIONS[session.id];
      return (held?.messages ?? []).some((message) =>
        message.content.toLowerCase().includes(needle),
      );
    });
    const rows = matched
      .map((session) => {
        const messages = BOT_CONVERSATIONS[session.id]?.messages ?? [];
        // The activity a row shows: the newest message, or the session's own
        // last change when it holds none. Never zero — that is the whole point
        // of the fallback in `session.rs`.
        const newest = messages.reduce((max, message) => Math.max(max, message.createdMs), 0);
        return {
          session,
          latestActivityMs: Math.max(session.updatedMs, newest),
          messageCount: messages.length,
          // Epic 63 (AD-181): `bots::remote::transcript_source`'s rule — the
          // conversation's own answer where the mock holds one, else local.
          transcript: BOT_CONVERSATIONS[session.id]?.transcript ?? "local",
        };
      })
      .sort((a, b) =>
        b.latestActivityMs === a.latestActivityMs
          ? b.session.id.localeCompare(a.session.id)
          : b.latestActivityMs - a.latestActivityMs,
      );
    const limit = req.limit === undefined || req.limit === 0 ? 50 : req.limit;
    // `total` is the matched set and `rows` is the page: the two numbers the
    // count line must not confuse.
    return { rows: rows.slice(0, limit), total: rows.length };
  },
  bots_session_rename: (payload) => {
    const found = BOT_SESSIONS.find((session) => session.id === String(payload.sessionId));
    return { ...(found ?? BOT_SESSIONS[0]), title: String(payload.title ?? "") };
  },
  bots_session_archive: (payload) => {
    const found = BOT_SESSIONS.find((session) => session.id === String(payload.sessionId));
    return { ...(found ?? BOT_SESSIONS[0]), archived: payload.archived === true };
  },
  bots_session_delete: () => null,
  // --- Story 61.12's pasted image and deliverable paths -------------------
  //
  // The paste answers with a staged row and no bytes, because the real command
  // takes its bytes over a raw binary IPC body that never appears in a JSON
  // payload (AD-58) — a harness that echoed base64 back would teach the
  // frontend a shape the shell will never send.
  bots_image_paste: () =>
    ({
      id: "01J8BOTIMAGEAAAAAAAAAAAAAA",
      filename: "pasted-image.png",
      mime: "image/png",
      byteLen: 184_320,
    }) satisfies BotAttachmentVm,
  bots_image_discard: () => null,
  // Two mentions, one of each verdict, so both halves of FR-393 are on screen
  // in `bun run dev`: a granted path with a control, and an ungranted one that
  // renders as text with its reason. The offsets are into the reply itself,
  // because keeper never strips a path out of an answer.
  bots_deliverable_paths: (payload) => {
    const body = String(payload.body ?? "");
    const rows: BotDeliverableVm[] = [];
    const granted = body.indexOf("/Users/tgorka/Drive/journal/2026/notes.md");
    if (granted >= 0) {
      rows.push({
        raw: "/Users/tgorka/Drive/journal/2026/notes.md",
        absolute: "/Users/tgorka/Drive/journal/2026/notes.md",
        start: granted,
        end: granted + "/Users/tgorka/Drive/journal/2026/notes.md".length,
        profileId: "p1",
        subpath: "journal/2026/notes.md",
        reason: null,
      });
    }
    const ungranted = body.indexOf("/etc/hosts");
    if (ungranted >= 0) {
      rows.push({
        raw: "/etc/hosts",
        absolute: "/etc/hosts",
        start: ungranted,
        end: ungranted + "/etc/hosts".length,
        profileId: null,
        subpath: null,
        reason:
          "That path is outside every folder keeper syncs, so keeper is not offering to open it. keeper never opens a location a model named on its own.",
      });
    }
    return rows;
  },
  bots_models_list: (payload) => {
    // Keyed by the bot target rather than the provider, because that is what
    // differs: a Hermes profile prefix serves its own roster.
    const bot = payload.bot === null || payload.bot === undefined ? "" : String(payload.bot);
    return BOT_MODELS[bot] ?? [];
  },
  // Both probes answer `online`, because a harness that could not reach its own
  // fixtures would only ever show the failure state. The failure states are
  // reachable by pointing a real provider row at a port nothing listens on.
  bots_provider_probe: () =>
    ({
      reach: "online",
      status: 200,
      version: "0.33.2",
      roundTripMs: 4,
      bot: null,
      presence: null,
      reason: null,
    }) satisfies BotProbeVm,
  bots_bot_probe: (payload) =>
    ({
      reach: "online",
      status: 200,
      version: null,
      roundTripMs: 6,
      bot: String(payload.target ?? ""),
      presence: "exists",
      reason: null,
    }) satisfies BotProbeVm,
  // The write verbs answer with a plausible row rather than mutating the table:
  // Settings re-reads after a save, and a harness that half-mutated would show
  // a list the next read contradicts.
  bots_provider_save: () => BOT_PROVIDERS[0],
  bots_provider_remove: () => null,
  bots_bot_save: () => BOT_ROWS[0],
  bots_bot_remove: () => null,
  // Story 61.7's two writes, and they mutate the fixture rather than answering
  // a canned row: the whole point of a hand order is that it is the order you
  // last put it in, and a harness that answered the original order would show
  // a reorder springing back and teach the surface's most important behaviour
  // wrongly. Same for an identity — the strip must redraw in the ink chosen.
  bots_bot_identity_save: (payload) => {
    const row = BOT_ROWS.find((bot) => bot.id === String(payload.botId));
    if (row === undefined) {
      return null;
    }
    row.shape = payload.shape === undefined ? null : (payload.shape as string | null);
    row.colour = payload.colour === undefined ? null : (payload.colour as string | null);
    row.mark = payload.mark === undefined ? null : (payload.mark as string | null);
    return row;
  },
  bots_bots_reorder: (payload) => {
    const order = (payload.order ?? []) as string[];
    const reordered = order
      .map((id) => BOT_ROWS.find((bot) => bot.id === id))
      .filter((bot): bot is BotVm => bot !== undefined);
    if (reordered.length !== BOT_ROWS.length) {
      // The real command refuses a partial order; the harness refuses it the
      // same way, so a caller that submits a filtered subset sees the failure
      // here rather than in production.
      return BOT_ROWS;
    }
    reordered.forEach((bot, index) => {
      bot.pinOrder = index;
    });
    BOT_ROWS.splice(0, BOT_ROWS.length, ...reordered);
    return BOT_ROWS;
  },
  bots_chat_send: (payload) => {
    const channel = payload.channel as MockChannel<BotStreamEvent>;
    const req = (payload.req ?? {}) as { sessionId?: string | null; text?: string };
    const held =
      req.sessionId === undefined || req.sessionId === null
        ? null
        : (BOT_CONVERSATIONS[req.sessionId] ?? null);
    const session = held?.session ?? (BOT_SESSIONS[0] as BotSessionVm);
    return botFakeStream(channel, session, String(req.text ?? ""), held?.messages ?? []);
  },
  bots_message_retry: (payload) => {
    const channel = payload.channel as MockChannel<BotStreamEvent>;
    const req = (payload.req ?? {}) as { sessionId?: string };
    const held = req.sessionId === undefined ? null : (BOT_CONVERSATIONS[req.sessionId] ?? null);
    const session = held?.session ?? (BOT_SESSIONS[0] as BotSessionVm);
    // The question is unchanged on a retry, so the replay drops the failed
    // answer and keeps everything before it — what the real command does.
    const carried = (held?.messages ?? []).filter((message) => message.role !== "assistant");
    return botFakeStream(
      channel,
      session,
      carried[carried.length - 1]?.content ?? "",
      carried.slice(0, -1),
    );
  },
  bots_chat_stop: (payload) => {
    BOT_LIVE_STREAMS.get(String(payload.subscriptionId))?.();
    return null;
  },
  // The answer to an `approvalAsked` event. The fake stream never asks — no
  // tool runs in this harness — so this exists only so the sheet's buttons
  // resolve rather than reject when the dialog is exercised by hand.
  bots_approval_answer: () => null,
  // Story 61.10's four. These four DO mutate the fixture table, which is the
  // one deliberate exception to the rule two screens up ("the write verbs
  // answer with a plausible row rather than mutating"): a grant's whole point
  // is that revoking it changes the answer to "what can this bot reach", so a
  // harness that re-served the same list would make the single interaction
  // this story exists for unobservable. The audit log is read-only here —
  // nothing in the harness runs a tool call.
  bots_grants_list: () => ({ grants: BOT_GRANTS, unknown: [] }),
  bots_grant_save: (payload) => {
    const req = (payload.req ?? {}) as Partial<BotGrantSaveReq>;
    const scope: GrantScope = req.scope ?? { kind: "drive" };
    const label =
      scope.kind === "drive"
        ? "the whole drive"
        : scope.kind === "profile"
          ? scope.profileId
          : `${scope.profileId}/${scope.subpath}`;
    const held = BOT_GRANTS.findIndex((grant) => grant.id === req.id);
    const saved: BotGrantVm = {
      id: req.id ?? `01J8BOTGRANT${BOT_GRANTS.length}`,
      providerId: req.providerId ?? "01J8BOTPROVOLLAMAAAAAAAAAA",
      botId: req.botId ?? null,
      scope,
      scopeLabel: label,
      mode: req.mode ?? "read",
      createdMs: held === -1 ? NOW : (BOT_GRANTS[held]?.createdMs ?? NOW),
      // A rewrite un-revokes, as the real command does.
      revokedMs: null,
    };
    if (held === -1) {
      BOT_GRANTS.push(saved);
    } else {
      BOT_GRANTS[held] = saved;
    }
    return saved;
  },
  bots_grant_revoke: (payload) => {
    const held = BOT_GRANTS.findIndex((grant) => grant.id === String(payload.grantId));
    const row = BOT_GRANTS[held];
    if (row !== undefined) {
      // The row survives with `revokedMs` set, so every audit line naming it
      // still resolves — never a row that vanished.
      BOT_GRANTS[held] = { ...row, revokedMs: NOW };
    }
    return null;
  },
  bots_audit_list: () => BOT_AUDIT,
  // Story 61.8's toggle. A module-level `let` rather than an `ANSWERS` entry
  // because the whole point is that it round-trips: a harness that always
  // answered `false` would show the caption never appearing and teach the
  // reviewer that the toggle is broken. It starts off, which is the shipped
  // default.
  bots_message_details_get: () => botMessageDetails,
  bots_message_details_set: (payload) => {
    botMessageDetails = payload.shown === true;
    return null;
  },
  // --- Voice (Epic 62, Story 62.5) ----------------------------------------
  voice_availability: () => voiceUnavailable(),
  voice_watch: (payload) => {
    voiceWatcher = payload.channel as MockChannel<VoiceStateVm>;
    voiceWatchSerial += 1;
    voiceWatcher.onmessage?.(voiceIdle());
    return voiceWatchSerial;
  },
  voice_unwatch: (payload) => {
    if (payload.id === voiceWatchSerial) {
      voiceWatcher = null;
    }
    return null;
  },
  voice_wake_get: () => voiceWake,
  voice_wake_set: (payload) => {
    const phrase = String(payload.phrase ?? "").trim();
    // A crude stand-in for `WakePhrase::parse`: the real refusal sentence is
    // Rust's; this only shows the surface rendering one.
    if (phrase.replace(/\s+/g, "").length < 5) {
      throw {
        code: "internal",
        message: `use at least 5 letters in total — "${phrase}" is too short for the recogniser to tell from noise`,
        accountId: null,
        retriable: false,
      };
    }
    voiceWake = { ...voiceWake, enabled: payload.enabled === true, phrase };
    voiceWatcher?.onmessage?.(voiceIdle());
    return voiceWake;
  },
  voice_locale_set: (payload) => {
    const chosen = typeof payload.locale === "string" ? payload.locale : null;
    if (chosen !== null && !voiceOnDeviceLocales.includes(chosen)) {
      throw {
        code: "internal",
        message: `${chosen} cannot run on this phone — choose one of the languages listed`,
        accountId: null,
        retriable: false,
      };
    }
    voiceWake = { ...voiceWake, localeChosen: chosen, locale: chosen ?? VOICE_SYSTEM_LOCALE };
    return voiceWake;
  },
  // --- Voice, the talk mode (Epic 62, Story 62.6) --------------------------
  //
  // A scripted turn so the mic control's three states can be looked at in
  // `bun run dev`: listening with an interim transcript, then heard. What is
  // heard lands in the composer; nothing here sends. `voice_authorize`
  // answers "granted" — the dialogs are the phone's. The level (Epic 64,
  // Story 64.3) rises with the words and falls once they are heard, at the
  // ~25 Hz Rust bounds it to, so an indicator can be looked at too.
  voice_authorize: () => null,
  voice_start: () => {
    const push = (state: VoiceStateVm) => voiceWatcher?.onmessage?.(state);
    push({ kind: "listening", heard: "", level: null });
    for (let tick = 1; tick <= 40; tick++) {
      const heard = tick < 10 ? "" : tick < 22 ? "what did I" : "what did I save yesterday";
      const level = 0.15 + 0.5 * Math.abs(Math.sin(tick / 3));
      setTimeout(() => push({ kind: "listening", heard, level }), tick * 40);
    }
    setTimeout(() => push({ kind: "heard", text: "what did I save yesterday", level: 0.1 }), 1700);
    return null;
  },
  voice_stop: () => {
    voiceWatcher?.onmessage?.(voiceIdle());
    return null;
  },
  voice_speak: () => {
    voiceWatcher?.onmessage?.({ kind: "speaking" });
    setTimeout(() => voiceWatcher?.onmessage?.(voiceIdle()), 2500);
    return null;
  },
  voice_stop_speaking: () => {
    voiceWatcher?.onmessage?.(voiceIdle());
    return null;
  },
  // Story 61.9's registry, faked. See `mockCommandPreview` for why it is crude.
  bots_command_preview: (payload) => mockCommandPreview(String(payload.draft ?? "")),
  // --- Tasks (Epic 57, Story 57.6) ---------------------------------------
  //
  // Handlers rather than `ANSWERS` entries, because four of the five depend on
  // WHICH task was asked about — and because the flow worth looking at is
  // pressing Run now on a task the engine refuses. A table could not refuse.
  sync_tasks: () => TASK_LISTING,
  // Not a task, and answered beside the tasks because the pane reads both in one
  // pass. A static answer: nothing about it depends on the payload, and nothing
  // in the app can change it — the class is read-only by construction.
  sync_paced_work: () => PACED_WORK,
  sync_task_schedule_preview: (payload) => mockSchedulePreview(String(payload.expression ?? "")),
  sync_task_history: (payload) => {
    const runs = TASK_RUNS[String(payload.id)] ?? [];
    // The clamp the command applies, mirrored so a caller asking for two rows
    // is not quietly handed ten.
    const limit = typeof payload.limit === "number" ? payload.limit : 20;
    return runs.slice(0, Math.max(1, limit));
  },
  // The refusals, which are the half of Run now worth seeing. A thrown value
  // rejects the `invoke`, and `client.ts` normalises it into the `IpcError`
  // envelope the pane quotes on the row — so a busy task and an off one each
  // read the way they will in the app, rather than resolving with a run that
  // never happened.
  sync_task_run_now: (payload) => {
    const id = String(payload.id);
    const task = TASKS.find((candidate) => candidate.id === id);
    if (task === undefined) {
      throw { code: "internal", message: `no such task: ${id}`, accountId: null, retriable: false };
    }
    if (!task.enabled || task.mode === "off") {
      throw {
        code: "internal",
        message: `task ${id} is off, so nothing runs it — not even a request`,
        accountId: null,
        retriable: false,
      };
    }
    if (task.runningHost !== null) {
      throw {
        code: "busy",
        message: `${id} is already running on ${task.runningHost}`,
        accountId: null,
        retriable: true,
      };
    }
    const run: TaskRunVm = {
      id: 900 + TASKS.indexOf(task),
      taskId: id,
      startedMs: NOW,
      finishedMs: NOW + 1_200,
      outcome: "ok",
      unknownOutcome: null,
      detail: "no folders to sync",
      host: "01DEVICE#4188",
    };
    // Recorded, so the next read shows the run rather than the pane appearing
    // to have done nothing — the same reason `sync_profile_save` is stateful.
    TASK_RUNS[id] = [run, ...(TASK_RUNS[id] ?? [])];
    task.lastRun = run;
    return run;
  },
  sync_task_save: (payload) => {
    const req = payload.req as TaskSaveReq;
    const existing = TASKS.find((candidate) => candidate.id === req.id);
    // The lost-update refusal, mirrored so the flow worth looking at is
    // reachable in the dev shell: a form that seeded from a reading somebody
    // else has since moved is refused, and the refusal is what the form renders.
    // Every fixture carries a distinct `updatedMs`, so passing a stale one is
    // the whole of the setup.
    if (req.baselineUpdatedMs !== null) {
      if (existing === undefined) {
        throw {
          code: "internal",
          message: `task '${req.id}' no longer exists: it was forgotten elsewhere since this was opened, so there is nothing to change`,
          accountId: null,
          retriable: false,
        };
      }
      if (existing.updatedMs !== req.baselineUpdatedMs) {
        throw {
          code: "internal",
          message: `task '${req.id}' was changed elsewhere since this was opened (last written at ${existing.updatedMs}, this edit started from ${req.baselineUpdatedMs}): refusing to write stale values over it — re-read it and try again`,
          accountId: null,
          retriable: false,
        };
      }
    }
    // The delay's floor, mirrored for the reason the lost-update refusal above
    // is: the sentence a person meets when they type five minutes is worth being
    // able to look at. Only the floor, and only its own words — the ceiling is
    // unreachable from a box that speaks minutes without deliberate effort, and a
    // second copy of a rule is a second copy to drift. `900000` and `1800000` are
    // `TASK_MISSED_GRACE_MS` and `TASK_MISSED_DELAY_MS`; if either moves, this
    // string is prose in a dev harness and the real refusal is still Rust's.
    if (req.missedDelayMs !== null && req.missedDelayMs < 900_000) {
      throw {
        code: "internal",
        message: `invalid sync configuration: task missed-window delay must be at least the grace period (900000 ms), because the grace period is the interval that concludes nobody was home — a shorter delay would elapse before the window it holds back counted as missed, which is run_now wearing delay's name, got ${req.missedDelayMs} ms`,
        accountId: null,
        retriable: false,
      };
    }
    const prior = existing ?? TASKS[0];
    const saved: TaskVm = {
      ...prior,
      id: req.id === "" ? `01JMOCKSAVED${TASKS.length}` : req.id,
      kind: req.kind,
      mode: req.mode,
      enabled: req.enabled,
      profileId: req.profileId,
      schedule: req.schedule,
      // Echoed verbatim, `""` included: the real store keeps a blank a person
      // typed apart from a description that was never there, so a mock that
      // collapsed them would hide the one case the view has to render as absence.
      description: req.description,
      onMissed: req.onMissed,
      // Echoed verbatim too, `null` included: `null` is *use keeper's own
      // delay*, and a mock that filled the constant in here would hide the one
      // fact the form's note is composed around.
      missedDelayMs: req.missedDelayMs,
      // The store owns the window and clears it on any of these three moving,
      // so echoing the request's value back would show a "next due" the real
      // command would have discarded.
      nextDueMs: null,
      runningHost: null,
      leaseUntilMs: null,
      // Written by the store on every save, which is what makes the baseline a
      // moving target and the guard above worth having.
      updatedMs: NOW,
    };
    if (existing === undefined) {
      TASKS.push(saved);
    } else {
      TASKS.splice(TASKS.indexOf(existing), 1, saved);
    }
    return saved;
  },
  sync_task_forget: (payload) => {
    const at = TASKS.findIndex((candidate) => candidate.id === String(payload.id));
    if (at >= 0) {
      TASKS.splice(at, 1);
    }
    return null;
  },
  // The batched pair the Tasks pane's multi-selection drives (Story 59.4).
  // Stateful and per-id, for the reason `sync_task_save` above is: the flow
  // worth looking at is selecting five rows, pressing Disable, and reading a
  // receipt in which the ids that went and the ids that did not are told apart.
  // Each entry keeps the wire's invariant — `effect` only on `saved`, `reason`
  // only on `refused` — because the pane branches on exactly that.
  sync_tasks_set_enabled: (payload) => {
    const ids = (payload.ids ?? []) as TaskBatchIdReq[];
    const enabled = payload.enabled === true;
    const entries: TaskBatchEntryVm[] = ids.map((wanted) => {
      const existing = TASKS.find((candidate) => candidate.id === wanted.id);
      if (existing === undefined) {
        // `missing` and not `refused`: a well-formed id whose row another host
        // forgot is usually benign, and the two want different words on screen.
        return { id: wanted.id, outcome: "missing", effect: null, reason: null };
      }
      // The lost-update refusal, per id — the same rule and the same sentence
      // `sync_task_save` above mirrors. Unreachable in a shell with one writer
      // unless two batches race, which is the honest state of affairs. The
      // sentence below is a stand-in: the shipped wording is Rust's
      // (`db::upsert_task`), and this copy only exists so the pane has something
      // to render against the mock.
      //
      // `!= null` and not `!== null`: a caller that omits the baseline sends
      // `undefined`, and refusing that as stale would be a refusal production
      // never makes.
      if (wanted.baselineUpdatedMs != null && existing.updatedMs !== wanted.baselineUpdatedMs) {
        return {
          id: wanted.id,
          outcome: "refused",
          effect: null,
          reason: `task '${wanted.id}' was changed elsewhere since this was opened (last written at ${existing.updatedMs}, this edit started from ${wanted.baselineUpdatedMs}): refusing to write stale values over it — re-read it and try again`,
        };
      }
      // `rearmed` only when the row was out of service and is coming back,
      // because that is the distinction the effect exists to carry.
      const effect = !existing.enabled && enabled ? "rearmed" : "updated";
      existing.enabled = enabled;
      // The window follows `db::upsert_task`'s three rearm edges
      // (`db.rs:3316-3329`): a disable returns `updated` and **keeps** the
      // window, while a disabled→enabled transition returns `rearmed` and clears
      // it (`next_due_ms = NULL`) — deliberate anti-catch-up, `db.rs:3181-3185`.
      if (effect === "rearmed") {
        existing.nextDueMs = null;
      }
      existing.updatedMs = Date.now();
      return { id: wanted.id, outcome: "saved", effect, reason: null };
    });
    return { entries } satisfies TaskBatchReceiptVm;
  },
  sync_tasks_forget: (payload) => {
    const ids = (payload.ids ?? []) as string[];
    const entries: TaskBatchEntryVm[] = ids.map((id) => {
      const at = TASKS.findIndex((candidate) => candidate.id === id);
      if (at < 0) {
        return { id, outcome: "missing", effect: null, reason: null };
      }
      TASKS.splice(at, 1);
      return { id, outcome: "forgotten", effect: null, reason: null };
    });
    return { entries } satisfies TaskBatchReceiptVm;
  },
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
  // The folder settings form's one write. Stateful, for the reason the two
  // space writes below are: the flow being looked at is press Save, watch the
  // form re-seed, and a shell that accepted the save and re-answered the old
  // profile would make that flow look broken in exactly the way it is being
  // looked at for.
  //
  // It merges the way `parse_req` does — every `Option` left `null` means "the
  // caller did not express this", so the stored value stands — which is what
  // makes the DW-116 property visible here rather than only in a Rust test. It
  // also strips the keys the fixture's folder file owns, exactly as
  // `profile::as_stored` does, so the second folder's controls demonstrate the
  // revert they exist to prevent.
  sync_profile_save: (payload) => {
    const req = payload.req as SyncProfileReq;
    const profiles = ANSWERS.sync_profiles as SyncProfileVm[];
    // An add sends `id: null` and must APPEND. Falling back to `profiles[0]`
    // for the shape while keeping a fresh id is what stops "Add folder" from
    // renaming the first fixture folder, which is half the flow this handler
    // exists to make inspectable.
    const existing = profiles.find((candidate) => candidate.id === req.id);
    const prior = existing ?? { ...profiles[0], id: `p${profiles.length + 1}`, folderOwned: [] };
    const owned = new Set(prior.folderOwned);
    const merged: SyncProfileVm = {
      ...prior,
      name: req.name,
      localPath: req.localPath,
      remoteUrl: req.remoteUrl,
      branch: req.branch,
      direction: req.direction,
      lane: req.lane,
      subpaths: req.subpaths,
      excludes: req.excludes,
      removable: req.removable,
      lfsMode: req.lfsMode,
      lfsThresholdBytes: req.lfsThresholdBytes ?? prior.lfsThresholdBytes,
      virtualPatterns: req.virtualPatterns ?? prior.virtualPatterns,
      virtualOverBytes: req.virtualOverBytes ?? prior.virtualOverBytes,
      releaseTtlMs: req.releaseTtlMs ?? prior.releaseTtlMs,
      settleMs: req.settleMs ?? prior.settleMs,
      pollIntervalMs: req.pollIntervalMs ?? prior.pollIntervalMs,
      tags: req.tags,
      commitSubjectTemplate: req.commitSubjectTemplate ?? prior.commitSubjectTemplate,
      authorOverride: req.authorOverride ?? prior.authorOverride,
      notes: req.notes ?? prior.notes,
      notesSubfolder: req.notesSubfolder ?? prior.notesSubfolder,
      recordings: req.recordings ?? prior.recordings,
      recordingsSubfolder: req.recordingsSubfolder ?? prior.recordingsSubfolder,
      sessions: req.sessions ?? prior.sessions,
      sessionsSubfolder: req.sessionsSubfolder ?? prior.sessionsSubfolder,
    };
    const stored: SyncProfileVm = {
      ...merged,
      virtualPatterns: owned.has("virtualPatterns")
        ? prior.virtualPatterns
        : merged.virtualPatterns,
      virtualOverBytes: owned.has("virtualOverBytes")
        ? prior.virtualOverBytes
        : merged.virtualOverBytes,
      releaseTtlMs: owned.has("releaseTtlMs") ? prior.releaseTtlMs : merged.releaseTtlMs,
      excludes: owned.has("excludes") ? prior.excludes : merged.excludes,
    };
    ANSWERS.sync_profiles =
      existing === undefined
        ? [...profiles, stored]
        : profiles.map((candidate) => (candidate.id === stored.id ? stored : candidate));
    return stored;
  },
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
  /**
   * The path plugin's directory lookup, which is not one of the app's own
   * commands and is the only non-`keeper` invoke any screen makes (Story 59.8).
   *
   * Without it the Add-folder form's Home control sits permanently disabled in
   * `bun run dev` and a typed `~` never resolves — the whole of that story is
   * invisible here, and the disabled button reads as a frontend bug, which is
   * exactly what this file exists to stop. `21` is `BaseDirectory.Home`; every
   * other directory is answered `null`, which is what the form treats as "the
   * shell could not say" rather than a wrong answer dressed as a right one.
   */
  "plugin:path|resolve_directory": (payload) =>
    Number(payload.directory) === 21 ? "/Users/tgorka" : null,
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

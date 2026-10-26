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

import { emit } from "@tauri-apps/api/event";
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { IDLE_RECORDING_STATUS } from "@/hooks/use-recording-session";
import { remoteOnSourceHost } from "@/lib/forge-repos";
import type {
  AccountDeviceVm,
  AccountOffersVm,
  AccountRestoreVm,
  AccountSetupVm,
  AccountShareVm,
  AccountStateVm,
  AccountVm,
  AgentApprovalVm,
  AgentAttachmentVm,
  AgentChoiceVm,
  AgentInstancesVm,
  AgentSessionAnsweringVm,
  AgentSessionMembersVm,
  AgentSessionRowVm,
  AgentSessionsBatch,
  AgentSessionViewVm,
  AutoUpdateRestartVm,
  AutoUpdateVm,
  BotModelVm,
  CapabilitiesVm,
  CardAgentVm,
  CardKeyVm,
  ChatCommandPreviewVm,
  ChatCommandRowVm,
  ChatInlinePreviewVm,
  ChatInlineRowVm,
  ChoiceVm,
  CopyJobVm,
  CredentialChoicesVm,
  DefaultAgentVm,
  DeviceCodeVm,
  DocumentVm,
  DriveFolderVm,
  FailureOperation,
  FailureStateVm,
  FileSizeVm,
  FilesEntrySyncVm,
  FilesEntryVm,
  FilesListingVm,
  FilesReleaseVm,
  ForgeAddReq,
  ForgeAddResultVm,
  ForgeNoticeVm,
  ForgeOwnerVm,
  ForgeReposVm,
  ForgeRepoVm,
  ForgeSourceVm,
  HeldRestoreVm,
  HeldSendVm,
  HotkeyVm,
  HqPairCodeVm,
  HqStatusVm,
  InboxBatch,
  InboxRoomVm,
  LabelVm,
  LogUploadVm,
  ModelProviderVm,
  NetworksSnapshot,
  NoteBodyBatch,
  NoteIntentVm,
  OldAgentRoomsVm,
  OrgAccountVm,
  PacedWorkVm,
  PaginationStatusBatch,
  PanelIntentVm,
  PlatformVm,
  PromoteState,
  ProviderProbeVm,
  PushOpenVm,
  RecordingCaptureSourcesVm,
  RecordingRemovalPreviewVm,
  RecordingRemovedVm,
  RecordingSearchVm,
  RecordingSettingsVm,
  ReleaseCheckVm,
  SessionPromoteVm,
  SessionSpaceFilesVm,
  SessionSpaceFileVm,
  SessionSpaceVm,
  SessionTaskVm,
  SignInLinkVm,
  SpacesSnapshot,
  SyncActivityVm,
  SyncFootprintVm,
  SyncProblemsVm,
  SyncProfileReq,
  SyncProfileVm,
  SyncStatusVm,
  SyncVerifyVm,
  TaskBatchEntryVm,
  TaskBatchIdReq,
  TaskBatchReceiptVm,
  TaskListingVm,
  TaskRunVm,
  TaskSaveReq,
  TaskSchedulePreviewVm,
  TaskVm,
  TextFileVm,
  TimelineBatch,
  TimelineItemVm,
  VoiceAgentTargetVm,
  VoiceStateVm,
  VoiceUnavailableVm,
  VoiceWakeVm,
} from "@/lib/ipc/client";
import { PUSH_OPEN_EVENT, UNIFIEDPUSH_STATUS_EVENT, VOICE_WAKE_EVENT } from "@/lib/ipc/client";
import { DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { platformStore } from "@/lib/stores/platform";
// The phone's and the tablet's "On this …" lists are composed in Rust
// (`keeper_core::disclosure`) and pinned to these docs there; the rig reads the
// docs rather than holding a third copy.
import ANDROID_DOC from "../docs/android.md?raw";
import IOS_DOC from "../docs/ios.md?raw";
import photoFixture from "./fixtures/agent-photo.png";
import photoThumbnail from "./fixtures/agent-photo-thumbnail.png";
// Nixi's picture (story 100.4): an SVG from her home, served by Vite here as
// keeper-file:// serves it in the app — an `<img>` source, never inlined. The
// other agents have none and keep their letter.
import {
  MEDIA_BLOCK_FRONTMATTER,
  MEDIA_BLOCK_NOTES,
  markFreshTranscribed,
  mediaBlockMockHandlers,
} from "./media-block-fixture";
import { transcriptionMockHandlers } from "./transcription-fixture";
import UNIFIEDPUSH_FIXTURE from "./unifiedpush-fixture.json";

/** Roughly now, so relative timestamps read as "3 min ago" rather than 1970. */
const NOW = Date.now();
const ago = (minutes: number) => NOW - minutes * 60_000;

/**
 * Verbatim the shape `keeper_sync::engine::held_push_sentence` composes (Story
 * 102.2), for tgdrive's upload 704972 as the owner's `sync.db` held it.
 */
const HELD_PUSH_SENTENCE =
  "Publishing is on hold until 40-media/recordings/2026/2026-10-16 14.07 test/manifest.json reaches the remote, and its upload stopped: the account credential is unavailable: Reconnect the repository. Retry all stopped puts every stopped transfer of this folder back in the queue.";

/**
 * When tgdrive's last upload stops under the mock shell (Story 102.2, R503):
 * until then its push is held while the upload still moves and the card reads
 * as syncing; from then the held push's sentence and "Retry all stopped"
 * arrive through the ordinary status poll, with no push failing again — what
 * `Engine::project_held_push` does when an upload parks.
 */
const UPLOAD_STOPS_AT = NOW + 12_000;

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
  // The media block's states: a whole meeting with markers, a clip with its
  // words, one not transcribed yet, and an old note with a per-file embed and a
  // refused block (`media-block-fixture.ts`).
  ...MEDIA_BLOCK_NOTES.map(
    ([id, title, body, tags]) => [id, title, body, tags, ago(20), false] as const,
  ),
] as const;

const noteRows = NOTES.map(([id, title, body, tags, modified, pinned], index) => ({
  id,
  // `NoteRowVm.vaultId` (epic 77): a row opens in its own drive, and a row
  // without one opened as "the vault this note was in is no longer set up".
  vaultId: "v1",
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

/**
 * The five defaults 44.3 seeds, plus the shapes a real vault grows — as one
 * drive's rail rows. `notes_spaces` answers `NoteRailVm` (epic 79), one group
 * per drive the search covers, every row stamped with its drive; a bare array
 * left the Notes surface throwing on `rail.vaults` in this shell.
 */
const SPACE_ROWS = [
  ["keeper:all", "All notes", "", "notebook", null],
  ["s-inbox", "Inbox", "is:untagged", "inbox", "inbox"],
  ["s-journal", "Journal", "is:journal", "calendar-days", "journal"],
  ["s-pinned", "Pinned", "is:pinned", "pin", "pinned"],
  ["s-rec", "Recordings", "is:recording", "video", "recordings"],
  ["s-tpl", "Templates", "is:template", "layout-template", "templates"],
  ["s-work", "Active work", "tag:epic22", "layers", null],
  [
    "keeper:uncategorized",
    "Uncategorized",
    "-is:untagged -is:journal -is:pinned -is:recording -is:template -tag:epic22",
    "shapes",
    null,
  ],
] as const;

function railRows(vaultId: string, vaultName: string) {
  return SPACE_ROWS.map(([id, name, query, icon, defaultKey]) => ({
    vaultId,
    vaultName,
    id,
    name,
    leafName: name,
    parent: null,
    depth: 0,
    descendants: 0,
    updatedMs: null,
    query,
    text: null,
    icon,
    defaultKey,
    order: 0,
    sort: "",
    sortEffective: "modified desc",
    limit: 0,
    template: null,
    folder: null,
    pinned: false,
    ttlHours: null,
    expiresMs: null,
    expiryPhrase: "",
    // The fixtures are query-only spaces: entering one restores nothing into
    // the bar and its lens does the narrowing, as Rust does for a query it
    // cannot project onto chips.
    restore: {
      tagTerms: {},
      origin: null,
      flags: [],
      text: null,
      sort: null,
      opaque: id !== "keeper:all",
    },
    error: null,
    warnings: [],
  }));
}

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
    transcribable: !isDir && /\.(mov|wav|mp4|m4a)$/.test(name),
    transcript:
      name === "meeting.mov" ? "/Volumes/merope/tgdrive/meeting.mov.transcript.json" : null,
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
    virtualChildren: 0,
    virtualBytes: 0,
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
  const row = browseEntry(name, false, size);
  // Content that is not here cannot be transcribed, which Rust says on the row.
  return { ...row, sync, lfsOid, release, transcribable: row.transcribable && lfsOid === null };
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
  { ...browseEntry("70-comms/voices", true, null), name: "voices", folderRole: "voices" },
  { ...browseEntry("meeting.mov", false, { bytes: 4_200_000, label: "4.2 MB" }), kind: "video" },
  browseEntry("meeting.mov.transcript.json", false, { bytes: 4_200, label: "4.2 kB" }),
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
    browseEntry("10-notes/knowledge", true, null),
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
  sessionEntry("artifacts/knowledge", true, null, 45),
  sessionEntry("artifacts/knowledge/2026-10-05-taxes", true, null, 45),
  sessionEntry("artifacts/knowledge/2026-10-05-taxes/what-to-bring.md", false, 1180, 45),
  sessionEntry("artifacts/knowledge/2026-10-05-taxes/the-whole-ledger.md", false, 65536, 45),
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

/** One agent key as Rust projects it: the value, and whether it reads. */
function cardKey(value: string, readable = true): CardKeyVm {
  return { value, readable };
}

/** A card's agent block: every key absent but those given (92.2, UX-DR134). */
function cardAgent(over: Partial<CardAgentVm>): CardAgentVm {
  return {
    run: null,
    assignee: null,
    host: null,
    requestedBy: null,
    schedule: null,
    lastRun: null,
    workflow: null,
    scheduledBy: null,
    integrity: null,
    runningOn: null,
    waiting: null,
    ...over,
  };
}

/**
 * The board's cards: a person's own four (`agent: null`), and one agent card
 * per state the board must draw — running on electra while pinned to hesperia,
 * waiting with its reason, blocked and in review in an ordinary column, a run
 * keeper cannot read, a schedule Nixi wrote that waits for *Allow*, a card made
 * from outside content, and a schedule that does not parse.
 */
const SESSION_TASKS: SessionTaskVm[] = [
  {
    id: "01J8AAAAAAAAAAAAAAAAAAAAAA",
    relPath: "task-migrate-the-live-zone.md",
    title: "Migrate the live zone",
    status: "in-preparation",
    order: 1,
    orderIsOwn: true,
    tags: ["task", "migration"],
    unstableIdentity: false,
    agent: null,
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
    agent: null,
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
    agent: null,
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
    agent: null,
  },
  {
    id: "path:cards/release-notes.md",
    relPath: "cards/release-notes.md",
    title: "Write the 0.9 release notes",
    status: "todo",
    order: 2,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("running"),
      assignee: cardKey("tola-grey"),
      host: cardKey("hesperia"),
      requestedBy: cardKey("@nixi:example.org"),
      lastRun: cardKey("2026-10-04T09:00:00+02:00"),
      runningOn: "electra",
    }),
  },
  {
    id: "path:cards/transcribe-the-call.md",
    relPath: "cards/transcribe-the-call.md",
    title: "Transcribe Tuesday's call",
    status: "todo",
    order: 3,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("waiting"),
      assignee: cardKey("lucyna-novak"),
      host: cardKey("hesperia"),
      requestedBy: cardKey("@tgorka:example.org"),
      waiting: "hesperia — a live host",
    }),
  },
  {
    id: "path:cards/approve-the-budget.md",
    relPath: "cards/approve-the-budget.md",
    title: "Approve the budget line",
    status: "todo",
    order: 4,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("blocked"),
      assignee: cardKey("tola-grey"),
      requestedBy: cardKey("@nixi:example.org"),
    }),
  },
  {
    id: "path:cards/summarise-the-inbox.md",
    relPath: "cards/summarise-the-inbox.md",
    title: "Summarise the inbox",
    status: "in-preparation",
    order: 2,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("review"),
      assignee: cardKey("tola-grey"),
      requestedBy: cardKey("@nixi:example.org"),
      integrity: cardKey("untrusted"),
    }),
  },
  {
    id: "path:cards/tidy-the-shelf.md",
    relPath: "cards/tidy-the-shelf.md",
    title: "Tidy the shelf",
    status: "deferred",
    order: 1,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("Running!", false),
      assignee: cardKey("Nixi", false),
    }),
  },
  {
    id: "path:cards/daily-triage.md",
    relPath: "cards/daily-triage.md",
    title: "Triage what came in today",
    status: "todo",
    order: 5,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("queued"),
      assignee: cardKey("tola-grey"),
      requestedBy: cardKey("@nixi:example.org"),
      schedule: cardKey("@daily"),
      scheduledBy: cardKey("@nixi:example.org"),
    }),
  },
  {
    id: "path:cards/check-the-mirror.md",
    relPath: "cards/check-the-mirror.md",
    title: "Check the mirror",
    status: "done",
    order: 2,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: cardAgent({
      run: cardKey("failed"),
      assignee: cardKey("tola-grey"),
      schedule: cardKey("every 30s", false),
    }),
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
/**
 * `?platform=phone` answers the iPhone's shape and `?platform=tablet` an
 * Android tablet's (story 98.3): both are reduced builds — every `cfg!(desktop)`
 * flag false — and differ in the platform facts the shell writes into the
 * webview (the tablet, the disclosure, and the `localhost` URL spelling
 * Android's WebView needs).
 */
const MOCK_PLATFORM = new URLSearchParams(window.location.search).get("platform");
const MOCK_REDUCED = MOCK_PLATFORM === "phone" || MOCK_PLATFORM === "tablet";

/** The first `- ` bullet run under `## Limitations` in a doc. */
function limitationsOf(doc: string): string[] {
  const after = doc.slice(doc.indexOf("## Limitations\n")).split("\n");
  const first = after.findIndex((line) => line.startsWith("- "));
  const rest = after.slice(first);
  const end = rest.findIndex((line) => !line.startsWith("- "));
  return rest.slice(0, end < 0 ? rest.length : end).map((line) => line.slice(2));
}

const MOCK_PLATFORM_FACTS: PlatformVm =
  MOCK_PLATFORM === "tablet"
    ? {
        urlForm: "localhost",
        tablet: true,
        disclosure: {
          heading: "On this tablet",
          lines: limitationsOf(ANDROID_DOC),
          docsUrl: "https://github.com/tgorka/keeper/blob/main/docs/android.md",
          docsLabel: "What's different on Android",
        },
      }
    : MOCK_PLATFORM === "phone"
      ? {
          urlForm: "scheme",
          tablet: false,
          disclosure: {
            heading: "On this iPhone",
            lines: limitationsOf(IOS_DOC),
            docsUrl: "https://github.com/tgorka/keeper/blob/main/docs/ios.md",
            docsLabel: "What's different on iPhone",
          },
        }
      : { urlForm: "scheme", tablet: false, disclosure: null };

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
  // 65, AD-189): every `cfg!(desktop)` flag false, exactly as
  // `keeper/src/ipc.rs` computes it there. Since AD-189 that answer — not the
  // window's width — is what puts the shell in the phone tier, so it is the
  // only way to look at the phone's landscape shape here.
  capabilities: MOCK_REDUCED
    ? // Epic 66: the folder links on the phone (`sync`, AD-198), share-out
      // is its reveal (`shareOut`, Story 66.3, AD-200) and notes ride the
      // folder (`notes`, Story 66.4) — the three flags `ipc.rs` now computes
      // true on iOS, so the Files and Notes rows and the capture sheet are
      // reachable in the phone rig (`dev/measure-files.ts`). `sessions` is
      // the phone with a sessions zone (98.2): the mock's folders have one.
      ({
        ...DEFAULT_CAPABILITIES,
        sync: true,
        // Share-out is iOS's alone (`cfg!(target_os = "ios")`).
        shareOut: MOCK_PLATFORM === "phone",
        notes: true,
        sessions: true,
      } satisfies CapabilitiesVm)
    : ({
        trayIcon: true,
        globalHotkey: true,
        launchAtLogin: true,
        inAppUpdater: true,
        nativeMenuBar: true,
        bridgeSidecar: true,
        revealInFileManager: true,
        // Story 66.3: the phone's reveal, and false on every desktop.
        shareOut: false,
        recording: true,
        transcription: true,
        sync: true,
        notes: true,
        sessions: true,
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
  notes_list: {
    rows: noteRows,
    total: noteRows.length,
    matched: noteRows.length,
    hidden: 0,
    offset: 0,
  },
  notes_hide_service_files_get: true,
  notes_service_file_names_get: ["index.md", "agents.md", "claude.md", "log.md"],
  notes_embedding_model_get: null,
  notes_note_marks: { rev: "mock", ranges: [] },
  // `true` means Rust removed an untouched new note; the mock never creates one.
  notes_close: false,
  notes_tag_tree: { nodes: TAGS },
  notes_templates: [],
  notes_capture_impact: [],
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
      voices: true,
      voicesSubfolder: "70-comms/voices",
      agents: true,
      agentsSubfolder: "80-agents",
      sessions: true,
      sessionsSubfolder: "60-sessions",
      tasks: true,
      tasksSubfolder: "tasks",
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
      voices: false,
      voicesSubfolder: "voices",
      agents: false,
      agentsSubfolder: "80-agents",
      sessions: false,
      sessionsSubfolder: "60-sessions",
      tasks: false,
      tasksSubfolder: "tasks",
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
      voices: false,
      voicesSubfolder: "voices",
      agents: false,
      agentsSubfolder: "80-agents",
      sessions: false,
      sessionsSubfolder: "60-sessions",
      tasks: false,
      tasksSubfolder: "tasks",
      folderOwned: [],
    },
  ] satisfies SyncProfileVm[],
  sync_statuses: [
    { profileId: "p1", profileName: "tgdrive", lastSyncMs: ago(3) },
    { profileId: "p2", profileName: "neuradrive", lastSyncMs: ago(41) },
    { profileId: "p3", profileName: "tgdrive-light", lastSyncMs: ago(12) },
  ].map((status) => ({
    ...status,
    state: "watching",
    phase: "idle",
    line: `${status.profileName} — remote comparison unknown`,
    filesDone: 0,
    filesTotal: null,
    bytesDone: 0,
    bytesTotal: null,
    pending: 0,
    settling: 0,
    queuedFiles: 0,
    queuedBytes: 0,
    warning: null,
    error: null,
    needsAttention: false,
    resettable: false,
    heldPush: false,
    // tgdrive as the owner's was on 2026-10-19 (Story 102.2): its push held
    // behind an upload the signed-out account stopped. `sync_retry_all_parked`
    // below clears it, so the way out can be pressed under the mock shell.
    ...(status.profileId === "p1"
      ? {
          state: "needsAttention",
          line: `${status.profileName} — ${HELD_PUSH_SENTENCE}`,
          error: HELD_PUSH_SENTENCE,
          needsAttention: true,
          heldPush: true,
        }
      : {}),
  })) satisfies SyncStatusVm[],
  sync_problems: {
    warning: null,
    error: null,
    // Stopped uploads first, as `db::list_parked` orders them (Story 102.2).
    parked: [
      {
        id: 704972,
        kind: "lfsUpload",
        attempts: 1,
        lastError: "the account credential is unavailable: Reconnect the repository.",
      },
      {
        id: 704801,
        kind: "pull",
        attempts: 1,
        lastError: "the account credential is unavailable: Reconnect the repository.",
      },
    ],
    conflicts: [],
    unspellable: [],
  } satisfies SyncProblemsVm,
  // One row restored from history (Story 102.2) beside an ordinary arrival.
  sync_activity: [
    {
      tsMs: ago(2),
      kind: "restored",
      path: "70-comms/meetings/2026/2026-10-06 1300 coin-fund-jackson-contact/manifest.sync-conflict-20261010-205450-maczek.json",
      sizeBytes: 1896,
      delivery: "inProgress",
      failure: null,
      unitId: 705000,
    },
    {
      tsMs: ago(9),
      kind: "added",
      path: "10-notes/2026-10-18-untitled.md",
      sizeBytes: 212,
      delivery: "success",
      failure: null,
      unitId: null,
    },
  ] satisfies SyncActivityVm[],
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
    // An agent's session (its board holds agent cards), so it carries its room:
    // Nixi's reading-list conversation, where `?approval=` draws 93.3's card —
    // what the phone board's cards open (98.2 #5, `?platform=phone`).
    room: "!nixi-reading:example.org",
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
 * The promote panel's fixture (FR-244, UX-DR137), as Rust reads it from disk:
 * per root and session, the README's `## Promote` rows, the session's files
 * with their bytes and when each last changed, the files that cannot be read,
 * and the drive's vault copies with the persons whose review each carries.
 * `sessions_promote_panel` derives the panel from it as
 * `keeper_core::sessions::promote::promote_panel` does, and the two writes
 * change it as `keeper_agent::promote` does, so a promotion or a tick
 * re-renders the way the app would. Only the flat fixture session holds one:
 * a row in every state, two unlisted workspace files and two harvested notes —
 * one exactly 64 KiB, whose bytes `sync_read_text` serves whole (S-32), one
 * promoted into the vault and reviewed.
 */
export type PromoteRowFixture =
  | {
      source: string;
      target: string;
      note: string;
      /**
       * The copy this row records as the one its source published, review
       * keys aside — Rust's fourth cell records its digest. Only a vault copy
       * that is exactly this may be replaced by a promotion or reviewed.
       */
      published?: string;
    }
  | { raw: string; line: number };

export type PromoteFixture = {
  /** The README above its `## Promote` section; `# Session\n` when not given. */
  head?: string;
  table: boolean;
  rows: PromoteRowFixture[];
  /**
   * Session-relative. `bytes`, when given, are the file's exact bytes — read
   * as text wherever Rust reads them as text, so they must then be UTF-8 —
   * and `text` is then unused. `changed` is its mtime, in ms.
   */
  files: Map<string, { text: string; changed: number; bytes?: Uint8Array }>;
  /** Session-relative folders with nothing in them. */
  dirs?: string[];
  /** Session-relative files and folders, or drive-relative vault copies, that cannot be read. */
  unreadable: Record<string, true>;
  /** Drive-relative: each copy as promoted, review keys aside. */
  vault: Map<string, { text: string; reviewers: string[]; changed: number }>;
  /** Why a promotion out is refused (`SessionPromoteVm.outRefused`). */
  outRefused: string | null;
  /** The label chip of an agent's session (`SessionPromoteVm.label`). */
  label: LabelVm | null;
  /** What the listing could not see (`SessionPromoteVm.problems`). */
  problems: string[];
};

const PROMOTE_SESSION = "01J8SESSIONAAAAAAAAAAAAAAA";
const PROMOTE_SESSION_IN_DRIVE = "60-sessions/active/2026-08-12-keeper-sessions";
const PROMOTE_VAULT = "10-notes";
const PROMOTE_ME = "human:tgorka";

/** A harvested note as the steward's host stores it. */
function harvestedNote(title: string, body: string): string {
  return `---\ntype: Reference\ntitle: ${title}\ngenerated:\n  by: agent:tola-grey@electra\n  at: 2026-10-05T18:00:00Z\nhuman_reviewed: false\nstatus: draft\n---\n\n${body}`;
}

function seededPromote(): PromoteFixture {
  const knowledge = "artifacts/knowledge/2026-10-05-taxes";
  const bring = harvestedNote(
    "What the tax office wants",
    "Bring the PIT-37, the receipts and the ID.\n",
  );
  // A ledger of exactly 64 KiB, every byte of which the panel shows.
  const head = harvestedNote("The whole ledger", "");
  let body = "";
  for (let n = 1; head.length + body.length < 65_536 - 64; n += 1) {
    body += `- ${String(n).padStart(4, "0")} receipt, kept for the tax office\n`;
  }
  const tail = "The last line of the ledger.\n";
  const ledger = `${head}${body}${"-".repeat(65_536 - head.length - body.length - tail.length - 1)}\n${tail}`;
  return {
    table: true,
    rows: [
      { source: "workspace/draft.md", target: "artifacts/report.md", note: "weekly report" },
      { source: "workspace/figures.csv", target: "artifacts/figures.csv", note: "" },
      { source: "workspace/scratch.md", target: "artifacts/summary.md", note: "cleaned up" },
      { source: "workspace/plan.md", target: "artifacts/plan.md", note: "" },
      { raw: "| workspace/half a row |", line: 17 },
      { source: "workspace/locked.md", target: "artifacts/locked.md", note: "" },
      {
        source: `${knowledge}/what-to-bring.md`,
        target: `${PROMOTE_VAULT}/knowledge/what-to-bring.md`,
        note: "knowledge",
        published: bring,
      },
    ],
    files: new Map([
      ["workspace/draft.md", { text: "# Weekly report\n", changed: 10 }],
      ["artifacts/report.md", { text: "# Weekly report\n", changed: 20 }],
      ["workspace/figures.csv", { text: "a,b\n1,3\n", changed: 30 }],
      ["artifacts/figures.csv", { text: "a,b\n1,2\n", changed: 20 }],
      ["artifacts/summary.md", { text: "# Summary\n", changed: 20 }],
      ["workspace/plan.md", { text: "# Plan\n", changed: 20 }],
      ["workspace/locked.md", { text: "# Locked\n", changed: 20 }],
      ["artifacts/locked.md", { text: "# Locked\n", changed: 20 }],
      ["workspace/notes.md", { text: "# Notes, still being written\n", changed: 40 }],
      ["workspace/data/run.json", { text: "{}\n", changed: 5 }],
      ["artifacts/flat-contract.md", { text: "# The flat contract\n", changed: 45 }],
      [
        "artifacts/chart.png",
        {
          text: "",
          // A PNG's signature: its 0x89 is no UTF-8, so Rust says "not a text file".
          bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d]),
          changed: 45,
        },
      ],
      // Valid UTF-8 holding a NUL: text, as Rust's test reads it.
      ["artifacts/trace.log", { text: "started\u0000stopped\n", changed: 45 }],
      [`${knowledge}/what-to-bring.md`, { text: bring, changed: 10 }],
      [`${knowledge}/the-whole-ledger.md`, { text: ledger, changed: 10 }],
    ]),
    unreadable: { "artifacts/locked.md": true },
    vault: new Map([
      [
        `${PROMOTE_VAULT}/knowledge/what-to-bring.md`,
        { text: bring, reviewers: [PROMOTE_ME], changed: 20 },
      ],
    ]),
    outRefused: null,
    label: {
      readers: ["@tgorka:h"],
      anyone: false,
      integrity: "agent",
      localOnly: false,
      sentence: "What you read here may be shown only to: @tgorka:h.",
    },
    problems: [],
  };
}

const PROMOTE_FIXTURES = new Map<string, PromoteFixture>();

/**
 * Where each fixture session lies in its drive, by session id: the mock's
 * sessions are the same under every root, as `sessions_list` answers them.
 */
const PROMOTE_SESSIONS_IN_DRIVE: Record<string, string> = {
  [PROMOTE_SESSION]: PROMOTE_SESSION_IN_DRIVE,
};

/**
 * The one identity every promote handler and every file read of a fixture
 * resolves through: a root (a sessions root is its drive's profile, by id)
 * and a session of it.
 */
function promoteKey(rootId: unknown, sessionId: unknown): string {
  return `${String(rootId)}/${String(sessionId)}`;
}

/** The fixture of the payload's root and session, made on first ask. */
function promoteFixture(payload: Record<string, unknown>): PromoteFixture {
  const key = promoteKey(payload.rootId, payload.sessionId);
  let fixture = PROMOTE_FIXTURES.get(key);
  if (fixture === undefined) {
    fixture =
      payload.sessionId === PROMOTE_SESSION
        ? seededPromote()
        : {
            table: false,
            rows: [],
            files: new Map(),
            unreadable: {},
            vault: new Map(),
            outRefused: null,
            label: null,
            problems: [],
          };
    PROMOTE_FIXTURES.set(key, fixture);
  }
  return fixture;
}

/**
 * Test seam: the promote fixture of `rootId`/`sessionId`, the seeded one
 * again — or `stage` applied to it — so a panel test stages a stale row, a
 * refused audience or an empty session, and the handlers derive the panel
 * from it as Rust would.
 */
export function stagePromoteFixture(
  rootId: string,
  sessionId: string,
  stage?: (fixture: PromoteFixture) => void,
): PromoteFixture {
  PROMOTE_FIXTURES.delete(promoteKey(rootId, sessionId));
  const fixture = promoteFixture({ rootId, sessionId });
  stage?.(fixture);
  return fixture;
}

const utf8 = new TextEncoder();

/**
 * SHA-256 in plain TypeScript (FIPS 180-4), for a dev server opened over
 * plain http from another host, where `crypto.subtle` does not exist.
 */
export function sha256Bytes(message: Uint8Array): Uint8Array {
  const k = new Uint32Array(64);
  const h = new Uint32Array(8);
  const primes: number[] = [];
  for (let n = 2; primes.length < 64; n += 1) {
    if (primes.every((p) => n % p !== 0)) primes.push(n);
  }
  const frac = (x: number) => ((x - Math.floor(x)) * 2 ** 32) >>> 0;
  primes.forEach((p, i) => {
    k[i] = frac(Math.cbrt(p));
    if (i < 8) h[i] = frac(Math.sqrt(p));
  });
  const bits = message.length * 8;
  const padded = new Uint8Array(Math.ceil((message.length + 9) / 64) * 64);
  padded.set(message);
  padded[message.length] = 0x80;
  const view = new DataView(padded.buffer);
  view.setUint32(padded.length - 8, Math.floor(bits / 2 ** 32));
  view.setUint32(padded.length - 4, bits >>> 0);
  const w = new Uint32Array(64);
  const rotr = (x: number, n: number) => (x >>> n) | (x << (32 - n));
  for (let block = 0; block < padded.length; block += 64) {
    for (let t = 0; t < 16; t += 1) w[t] = view.getUint32(block + t * 4);
    for (let t = 16; t < 64; t += 1) {
      const s0 = rotr(w[t - 15], 7) ^ rotr(w[t - 15], 18) ^ (w[t - 15] >>> 3);
      const s1 = rotr(w[t - 2], 17) ^ rotr(w[t - 2], 19) ^ (w[t - 2] >>> 10);
      w[t] = (w[t - 16] + s0 + w[t - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let t = 0; t < 64; t += 1) {
      const t1 =
        (hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + k[t] + w[t]) >>> 0;
      const t2 = ((rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) >>> 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    [a, b, c, d, e, f, g, hh].forEach((value, i) => {
      h[i] = (h[i] + value) >>> 0;
    });
  }
  const out = new Uint8Array(32);
  const outView = new DataView(out.buffer);
  h.forEach((value, i) => {
    outView.setUint32(i * 4, value);
  });
  return out;
}

/** The lowercase hex SHA-256 of `message` — a string's UTF-8 — as Rust's `sha256_hex`. */
export async function sha256Hex(message: string | Uint8Array): Promise<string> {
  const bytes = typeof message === "string" ? utf8.encode(message) : message;
  const digest =
    globalThis.crypto?.subtle === undefined
      ? sha256Bytes(bytes)
      : new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** `parts`, each NUL-terminated, as one byte string. */
function framed(parts: (string | null)[]): Uint8Array {
  const pieces = parts.map((part) =>
    part === null ? new Uint8Array([0]) : utf8.encode(`${part}\u0000`),
  );
  const out = new Uint8Array(pieces.reduce((sum, piece) => sum + piece.length, 0));
  let at = 0;
  for (const piece of pieces) {
    out.set(piece, at);
    at += piece.length;
  }
  return out;
}

/** `keeper_core::sessions::offer::row_revision`. */
export function promoteRowRevision(
  source: string,
  target: string,
  note: string,
  stamp: string | null,
  targetFact: string | null,
): Promise<string> {
  return sha256Hex(framed([source, target, note, stamp ?? "-", targetFact ?? "-"]));
}

/** Rust's `String` order: by UTF-8 bytes, which UTF-16 code-unit order is not. */
function byBytes(a: string, b: string): number {
  const [x, y] = [utf8.encode(a), utf8.encode(b)];
  for (let i = 0; i < Math.min(x.length, y.length); i += 1) {
    if (x[i] !== y[i]) return x[i] - y[i];
  }
  return x.length - y.length;
}

/** `keeper_core::sessions::offer::snapshot_revision`. */
export function promoteSnapshotRevision(
  readme: string,
  stamps: Record<string, string>,
  targets: Record<string, string>,
): Promise<string> {
  const pairs = (map: Record<string, string>) =>
    Object.keys(map)
      .sort(byBytes)
      .flatMap((key) => [key, map[key]]);
  return sha256Hex(framed([readme, ...pairs(stamps), null, ...pairs(targets)]));
}

/** `keeper_core::sessions::offer::copy_revision`. */
export function promoteCopyRevision(target: string, text: string): Promise<string> {
  return sha256Hex(framed([target, text]).slice(0, -1));
}

/** Whether `bytes` are text the notes vault takes: UTF-8 through the last byte, NULs and all. */
export function promoteIsText(bytes: Uint8Array): boolean {
  return utf8Text(bytes) !== null;
}

/**
 * `bytes` as Rust reads them as text (`read_to_string`, `String::from_utf8`):
 * every byte kept, a leading byte-order mark too, or `null` when they are not
 * UTF-8.
 */
function utf8Text(bytes: Uint8Array): string | null {
  try {
    return new TextDecoder("utf-8", { ignoreBOM: true, fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * The longest prefix of `bytes` that is UTF-8, decoded exactly (a byte-order
 * mark kept), as `digest_of` takes `from_utf8(&head[..valid_up_to])`.
 */
function utf8Prefix(bytes: Uint8Array): string {
  const lossy = new TextDecoder("utf-8", { ignoreBOM: true }).decode(bytes);
  for (let at = lossy.indexOf("\uFFFD"); at >= 0; at = lossy.indexOf("\uFFFD", at + 1)) {
    // Everything before the first U+FFFD the decoder made is decoded exactly,
    // so its encoding is its byte length; a U+FFFD the bytes hold is text.
    const valid = utf8.encode(lossy.slice(0, at)).length;
    if (bytes[valid] !== 0xef || bytes[valid + 1] !== 0xbf || bytes[valid + 2] !== 0xbd) {
      return lossy.slice(0, at);
    }
  }
  return lossy;
}

/** What `read_to_string` says of bytes that are not UTF-8. */
const NOT_UTF8 = "stream did not contain valid UTF-8";

/** `keeper_core::agents::knowledge::{MAX_NOTE_BYTES, MAX_REVIEWED_BYTES}`. */
export const PROMOTE_MAX_NOTE_BYTES = 65_536;
export const PROMOTE_MAX_REVIEWED_BYTES = 65_536 + 16_384;

type ReadStateMock = "unread" | "current" | "stale";

/**
 * `keeper_core::sessions::offer::decisions`: what of the person's intent
 * still holds — a choice only for an item at its revision (a refused item
 * keeps only a skip, the last choice wins), a read current while shown,
 * consent only to the candidate version read and shown.
 */
export function promoteDecisions(
  items: { revision: string; refused: boolean }[],
  notes: { path: string; revision: string | null; copy: string | null }[],
  intent: PanelIntentVm,
) {
  const chosen = new Map(intent.choices.map((choice) => [choice.revision, choice]));
  const choices = items.flatMap((item) => {
    const choice = chosen.get(item.revision);
    return choice === undefined || (choice.promote && item.refused) ? [] : [choice];
  });
  const state = (read: string | null, shown: string | null): ReadStateMock =>
    read === null ? "unread" : read === shown ? "current" : "stale";
  const kept: NoteIntentVm[] = [];
  const decided = notes.map((note) => {
    const held = [...intent.notes].reverse().find((candidate) => candidate.path === note.path);
    const read = held?.read ?? null;
    const copy = held?.copy ?? null;
    const candidateRead = state(read, note.revision);
    const consented = candidateRead === "current" && (held?.consent ?? null) === read;
    if (held !== undefined) {
      kept.push({ path: note.path, read, copy, consent: consented ? read : null });
    }
    return { path: note.path, candidateRead, copyRead: state(copy, note.copy), consented };
  });
  return {
    intent: { choices, notes: kept },
    notes: decided,
    complete: choices.length === items.length,
  };
}

function promoteRefusal(message: string): never {
  throw { code: "internal", message, accountId: null, retriable: false };
}

/**
 * `keeper_core::sessions::promote::CopyLoss::explain`: why the file at a
 * note's row's target is not the copy it published — the row records no
 * publication (`unrecorded`), or the file changed since — and what the
 * person can do that keeps it.
 */
function copyLoss(loss: "unrecorded" | "changed", source: string, target: string): string {
  const why =
    loss === "unrecorded"
      ? `${target} is not known to be the copy ${source} published: its row records no publication — written before keeper recorded one, or one that did not finish`
      : `${target} was changed since ${source} published it there — edited, or replaced by another file — so it is no longer that copy`;
  return `${why}. keeper neither replaces it nor writes a review of the note into it, and promoting again does not make it the note's copy. The file stays as it is, every edit kept; to publish the note, promote it under another name, or first move that file elsewhere in the vault, keeping it.`;
}

/**
 * `keeper_core::sessions::promote::standing`: whether the vault copy `there`
 * is the one the row records its source published — never because it holds
 * the same bytes — or why not.
 */
function standing(
  row: { published?: string },
  there: { text: string },
): "unrecorded" | "changed" | null {
  if (row.published === undefined) {
    return "unrecorded";
  }
  return there.text === row.published ? null : "changed";
}

/** A vault copy's bytes as Rust reads them: each review written in. */
function vaultText(copy: { text: string; reviewers: string[] }): string {
  const entries = copy.reviewers.map((by) => `  - by: ${by}\n    at: 2026-10-06T10:00:00Z\n`);
  return copy.reviewers.length === 0
    ? copy.text
    : copy.text.replace(
        "human_reviewed: false\n",
        `verified:\n${entries.join("")}human_reviewed: true\n`,
      );
}

const NO_TABLE =
  "this session's README has no ## Promote table, so nothing records a promotion; add the section first.";

const SNAPSHOT_CHANGED =
  "the session's workspace or its ## Promote table changed since this checklist was read, so nothing was archived; review it again.";

/** A fixture file's exact bytes. */
function fileBytes(file: { text: string; bytes?: Uint8Array }): Uint8Array {
  return file.bytes ?? utf8.encode(file.text);
}

/** A fixture file's text as Rust reads it as text: its exact bytes ({@link utf8Text}), or `null` when they are not UTF-8. */
function fileText(file: { text: string; bytes?: Uint8Array }): string | null {
  return file.bytes === undefined ? file.text : utf8Text(file.bytes);
}

const DENIED = "Permission denied (os error 13)";

/**
 * Every `workspace/` entry with its stamp, as `keeper_agent`'s inventory
 * writes it: each file's `file:<length>:<mtime ns>`, each folder `dir`,
 * the top `.gitkeep` aside.
 */
function promoteStamps(fixture: PromoteFixture): Record<string, string> {
  const stamps: Record<string, string> = {};
  const folders = (path: string) => {
    const parts = path.split("/");
    for (let depth = 2; depth < parts.length; depth += 1) {
      stamps[parts.slice(0, depth).join("/")] = "dir";
    }
  };
  for (const [path, file] of fixture.files) {
    if (!path.startsWith("workspace/") || path === "workspace/.gitkeep") continue;
    stamps[path] = `file:${fileBytes(file).length}:${BigInt(file.changed) * 1_000_000n}`;
    folders(path);
  }
  for (const dir of fixture.dirs ?? []) {
    if (!dir.startsWith("workspace/")) continue;
    stamps[dir] = "dir";
    folders(dir);
  }
  return stamps;
}

/** What the session runtime could not list: `fixture.problems`, and each `workspace/` folder that cannot be read. */
function promoteProblems(fixture: PromoteFixture): string[] {
  const closed = (fixture.dirs ?? [])
    .filter((dir) => dir.startsWith("workspace/") && fixture.unreadable[dir] === true)
    .map((dir) => `${dir}/ could not be listed: ${DENIED}`);
  return [...fixture.problems, ...closed];
}

/** The review keys `promote::digest_of` leaves out. */
const REVIEW_KEYS = ["verified", "verified_by", "human_reviewed"];

/**
 * `keeper_core::sessions::promote::digest_of`: what "differs" compares — a
 * markdown file's frontmatter without its review keys, so a tick does not
 * move it, and every other byte as it is.
 */
async function promoteDigest(rel: string, bytes: Uint8Array): Promise<string> {
  const name = rel.slice(rel.lastIndexOf("/") + 1).toLowerCase();
  const head = utf8Prefix(bytes.subarray(0, 64 * 1024));
  // `Frontmatter` finds its fence after a byte-order mark.
  const fence = /^\uFEFF?---\r?\n/.exec(head);
  const close = fence === null ? null : /^---[ \t]*(\r?\n|$)/m.exec(head.slice(fence[0].length));
  if (!(name.endsWith(".md") || name.endsWith(".markdown")) || fence === null || close === null) {
    return sha256Hex(bytes);
  }
  const innerEnd = fence[0].length + close.index;
  const bodyAt = innerEnd + close[0].length;
  const kept: string[] = [];
  let dropping = false;
  for (const line of head.slice(fence[0].length, innerEnd).split(/(?<=\n)/)) {
    const key = /^([^\s#:-][^:]*):/.exec(line)?.[1];
    if (key !== undefined) dropping = REVIEW_KEYS.includes(key);
    if (!dropping) kept.push(line);
  }
  const inner = kept.join("");
  const bom = fence[0].startsWith("\uFEFF") ? "\uFEFF" : "";
  const block = inner.trim() === "" ? bom : `${head.slice(0, fence[0].length)}${inner}${close[0]}`;
  // `head` decodes a prefix of `bytes` exactly, so this is its own byte length.
  const rest = bytes.subarray(utf8.encode(head.slice(0, bodyAt)).length);
  const whole = new Uint8Array(utf8.encode(block).length + rest.length);
  whole.set(utf8.encode(block));
  whole.set(rest, whole.length - rest.length);
  return sha256Hex(whole);
}

/** The bytes of the file `cell` names — a session file, or a vault copy with each review written in — or `undefined`. */
function cellBytes(fixture: PromoteFixture, cell: string): Uint8Array | undefined {
  if (cell.startsWith("artifacts/") || cell.startsWith("workspace/")) {
    const file = fixture.files.get(cell);
    return file === undefined ? undefined : fileBytes(file);
  }
  const copy = fixture.vault.get(cell);
  return copy === undefined ? undefined : utf8.encode(vaultText(copy));
}

/** What a row's choice knows of its target: `offer::target_fact`, review keys aside. */
async function promoteTargetFact(fixture: PromoteFixture, target: string): Promise<string> {
  if (fixture.unreadable[target] === true) return "unreadable";
  const held = cellBytes(fixture, target);
  return held === undefined ? "absent" : `digest:${await promoteDigest(target, held)}`;
}

/**
 * `keeper_core::sessions::promote::copy_digest`: the digest a row records
 * for the copy its harvested note published — its byte-order mark, what lies
 * between its fences with the review keys out (nothing when only whitespace
 * is left) and its body, each framed — and any other copy byte for byte.
 */
async function promoteCopyDigest(source: string, bytes: Uint8Array): Promise<string> {
  const text = utf8Text(bytes);
  const join = (parts: Uint8Array[]) => {
    const whole = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
    let at = 0;
    for (const part of parts) {
      whole.set(part, at);
      at += part.length;
    }
    return whole;
  };
  if (text === null || !(source.startsWith("artifacts/knowledge/") && source.endsWith(".md"))) {
    return sha256Hex(join([utf8.encode("keeper copy 1\0"), bytes]));
  }
  const bom = text.startsWith("\uFEFF") ? "\uFEFF" : "";
  const fence = /^\uFEFF?---\r?\n/.exec(text);
  const close = fence === null ? null : /^---[ \t]*(\r?\n|$)/m.exec(text.slice(fence[0].length));
  let inner = "";
  let body = text.slice(bom.length);
  if (fence !== null && close !== null) {
    const innerEnd = fence[0].length + close.index;
    const kept: string[] = [];
    let dropping = false;
    for (const line of text.slice(fence[0].length, innerEnd).split(/(?<=\n)/)) {
      const key = /^([^\s#:-][^:]*):/.exec(line)?.[1];
      if (key !== undefined) dropping = REVIEW_KEYS.includes(key);
      if (!dropping) kept.push(line);
    }
    inner = kept.join("");
    body = text.slice(innerEnd + close[0].length);
  }
  if (inner.trim() === "") inner = "";
  const framedPart = (part: string) => {
    const encoded = utf8.encode(part);
    const length = new Uint8Array(8);
    new DataView(length.buffer).setBigUint64(0, BigInt(encoded.length), true);
    return [length, encoded];
  };
  return sha256Hex(
    join([
      utf8.encode("keeper note copy 1\0"),
      ...framedPart(bom),
      ...framedPart(inner),
      utf8.encode(body),
    ]),
  );
}

/** The README the fixture's table stands for, as the snapshot binds it: a published row with its fourth cell. */
async function promoteReadme(fixture: PromoteFixture): Promise<string> {
  const head = fixture.head ?? "# Session\n";
  if (!fixture.table) return head;
  const lines = await Promise.all(
    fixture.rows.map(async (row) =>
      "raw" in row
        ? row.raw
        : row.published === undefined
          ? `| ${row.source} | ${row.target} | ${row.note} |`
          : `| ${row.source} | ${row.target} | ${row.note} | ${await promoteCopyDigest(row.source, utf8.encode(row.published))} |`,
    ),
  );
  return `${head}\n## Promote\n\n| workspace | → artifacts | note |\n| --- | --- | --- |\n${lines.map((line) => `${line}\n`).join("")}`;
}

/** Why the file at `rel`, `bytes` long as stored, cannot be read whole, as `offer::note_text` says: too large first, then unreadable. */
function readRefusal(rel: string, bytes: number, cap: number, unreadable: boolean): string | null {
  if (bytes > cap) {
    return `${rel} holds ${bytes} bytes, more than the ${cap} keeper reads of a knowledge note, so it is not shown`;
  }
  return unreadable ? `${rel} could not be read: ${DENIED}` : null;
}

/** The panel of `fixture`, composed as Rust composes it, with `intent` decided. */
async function promotePanel(
  fixture: PromoteFixture,
  intent: PanelIntentVm = { choices: [], notes: [] },
): Promise<SessionPromoteVm> {
  const stamps = promoteStamps(fixture);
  const rows = await Promise.all(
    fixture.rows.map(async (row) => {
      if ("raw" in row) {
        return {
          state: "unreadable" as const,
          source: "",
          target: "",
          note: "",
          out: false,
          raw: row.raw,
          line: row.line,
          problem: null,
          revision: await promoteRowRevision("", "", row.raw, String(row.line), null),
          refused:
            "keeper cannot read this line of the ## Promote table, so it promotes nothing from it; correct the line in the README.",
          choice: null,
        };
      }
      const out = !row.target.startsWith("artifacts/");
      const from = fixture.files.get(row.source);
      const to = out ? fixture.vault.get(row.target) : fixture.files.get(row.target);
      const unread = [row.source, row.target].find((cell) => fixture.unreadable[cell] === true);
      const problem = unread === undefined ? null : `${unread} could not be read: ${DENIED}`;
      let state: PromoteState = "ok";
      if (problem !== null) {
        state = "unknown";
      } else if (to === undefined) {
        state = "missingTarget";
      } else if (from === undefined) {
        state = "missingSource";
      } else if (
        (await promoteDigest(row.source, fileBytes(from))) !==
          (await promoteDigest(row.target, cellBytes(fixture, row.target) ?? new Uint8Array())) &&
        from.changed > to.changed
      ) {
        state = "stale";
      }
      const { source, target, note } = row;
      const refused = out
        ? `${source} was promoted into the drive's notes; promote it again from its notes section.`
        : from === undefined
          ? `${source} is not in this session any more.`
          : !/^workspace\/./.test(source) || !/^artifacts\/./.test(target)
            ? `a promotion copies a file of workspace/ into artifacts/; ${source} → ${target} is not one.`
            : null;
      const revision = await promoteRowRevision(
        source,
        target,
        note,
        stamps[source] ?? null,
        await promoteTargetFact(fixture, target),
      );
      return {
        state,
        source,
        target,
        note,
        out,
        raw: null,
        line: null,
        problem,
        revision,
        refused,
        choice: null,
      };
    }),
  );
  // A line written twice is two items, each repeat told apart by where it
  // falls and promoting nothing of its own (`promote::promote_panel`).
  const seen = new Map<string, number>();
  for (const row of rows) {
    const earlier = seen.get(row.revision) ?? 0;
    seen.set(row.revision, earlier + 1);
    if (earlier > 0) {
      row.revision = await sha256Hex(framed([row.revision, String(earlier)]));
      row.refused ??= `this line repeats an earlier row of the ## Promote table, ${row.source} → ${row.target}, so it promotes nothing of its own; the earlier row's choice decides it.`;
    }
  }
  const paths = [...fixture.files.keys()].sort(byBytes);
  const vault = fixture.outRefused === null ? PROMOTE_VAULT : null;
  const noTable = fixture.table ? null : NO_TABLE;
  // The source's one row (`promote::entry_of`), when it names a target out
  // of the session.
  const outRow = (source: string) => {
    const first = rows.find((row) => row.raw === null && row.source === source);
    return first?.out ? first : undefined;
  };
  const knowledge = await Promise.all(
    paths
      .filter((path) => path.startsWith("artifacts/knowledge/") && path.endsWith(".md"))
      .map(async (path) => {
        const file = fixture.files.get(path);
        const text = file === undefined ? "" : fileText(file);
        const bytes = file === undefined ? 0 : fileBytes(file).length;
        // `knowledge_file`: the stored size first, then the read itself.
        const problem =
          bytes > PROMOTE_MAX_NOTE_BYTES
            ? `${path} holds ${bytes} bytes, more than the ${PROMOTE_MAX_NOTE_BYTES} a knowledge note holds, so it is not shown`
            : fixture.unreadable[path] === true
              ? `${path} could not be read: ${DENIED}`
              : text === null
                ? `${path} could not be read: ${NOT_UTF8}`
                : null;
        const read = problem === null && text !== null ? text : "";
        // The source's one row — its first, `promote::entry_of` — for its
        // target, its publication and what promoting it out offers.
        const row = rows.find((candidate) => candidate.raw === null && candidate.source === path);
        const entry = fixture.rows.find(
          (candidate) => "source" in candidate && candidate.source === path,
        );
        const out = outRow(path);
        const held = out === undefined ? undefined : fixture.vault.get(out.target);
        const loss =
          out === undefined ||
          held === undefined ||
          fixture.unreadable[out.target] === true ||
          entry === undefined ||
          !("source" in entry)
            ? undefined
            : standing(entry, held);
        const reviewers = loss === null && held !== undefined ? held.reviewers : [];
        const foreignCopy =
          loss === undefined || loss === null || out === undefined
            ? null
            : copyLoss(loss, path, out.target);
        const signer = /\n {2}by: agent:([^@\n]+)@([^\n]+)\n/.exec(read);
        let copy = null;
        if (out !== undefined) {
          const copyText = held === undefined ? "" : vaultText(held);
          const why =
            held === undefined
              ? null
              : readRefusal(
                  out.target,
                  utf8.encode(copyText).length,
                  PROMOTE_MAX_REVIEWED_BYTES,
                  fixture.unreadable[out.target] === true,
                );
          copy = {
            path: out.target,
            there: held !== undefined,
            revision:
              held === undefined || why !== null
                ? null
                : await promoteCopyRevision(out.target, copyText),
            problem: why,
          };
        }
        const offered =
          vault !== null && problem === null && noTable === null && foreignCopy === null;
        const split = (target: string, fixed: boolean) => ({
          folder: target.slice(0, target.lastIndexOf("/")),
          name: target.slice(target.lastIndexOf("/") + 1),
          fixed,
        });
        const destination = !offered
          ? null
          : out === undefined
            ? split(`${vault}/${path.slice(path.lastIndexOf("/") + 1)}`, false)
            : out.state === "missingTarget" || out.state === "stale"
              ? split(out.target, true)
              : null;
        return {
          path,
          title: /\ntitle: ([^\n]+)\n/.exec(read)?.[1] ?? null,
          agent: signer?.[1] ?? null,
          host: signer?.[2] ?? null,
          bytes,
          revision: problem === null ? await sha256Hex(read) : null,
          promotedTo: row?.target ?? null,
          state: row?.state ?? null,
          reviewedBy: reviewers[reviewers.length - 1] ?? null,
          reviewedByMe: reviewers.includes(PROMOTE_ME),
          problem,
          foreignCopy,
          copy,
          destination,
          unavailable: vault !== null && problem === null ? (noTable ?? foreignCopy) : null,
          candidateRead: "unread" as ReadStateMock,
          copyRead: "unread" as ReadStateMock,
          consented: false,
        };
      }),
  );
  const unlisted = await Promise.all(
    Object.keys(stamps)
      .sort(byBytes)
      .filter((path) => stamps[path] !== "dir")
      .filter((path) => !rows.some((row) => row.raw === null && row.source === path))
      .map(async (source) => ({
        source,
        suggested: `artifacts/${source.slice(source.lastIndexOf("/") + 1)}`,
        revision: await promoteRowRevision(source, "", "", stamps[source], null),
        refused: noTable,
        choice: null as ChoiceVm | null,
      })),
  );
  const artifacts = paths
    .filter((path) => path.startsWith("artifacts/") && !path.startsWith("artifacts/knowledge/"))
    .map((path) => {
      const file = fixture.files.get(path);
      const unavailable =
        fixture.unreadable[path] === true
          ? `${path} could not be read: ${DENIED}`
          : file !== undefined && !promoteIsText(fileBytes(file))
            ? `${path} is not a text file, and the notes vault takes text.`
            : noTable;
      const out = outRow(path);
      const name = path.slice(path.lastIndexOf("/") + 1);
      return {
        path,
        destination:
          vault === null || unavailable !== null
            ? null
            : out === undefined
              ? { folder: vault, name, fixed: false }
              : {
                  folder: out.target.slice(0, out.target.lastIndexOf("/")),
                  name: out.target.slice(out.target.lastIndexOf("/") + 1),
                  fixed: true,
                },
        unavailable,
      };
    });
  const vm: SessionPromoteVm = {
    hasTable: fixture.table,
    rows,
    unlisted,
    knowledge,
    artifacts,
    label: fixture.label,
    vault: PROMOTE_VAULT,
    outRefused: fixture.outRefused,
    problems: promoteProblems(fixture),
    revision: await promoteRevision(fixture),
    intent: { choices: [], notes: [] },
    complete: false,
  };
  const decided = promoteDecisions(
    [...vm.rows, ...vm.unlisted].map((item) => ({
      revision: item.revision,
      refused: item.refused !== null,
    })),
    vm.knowledge.map((note) => ({
      path: note.path,
      revision: note.revision,
      copy: note.copy?.revision ?? null,
    })),
    intent,
  );
  const choice = (revision: string) =>
    decided.intent.choices.find((held) => held.revision === revision) ?? null;
  for (const item of [...vm.rows, ...vm.unlisted]) item.choice = choice(item.revision);
  vm.knowledge.forEach((note, at) => {
    Object.assign(note, {
      candidateRead: decided.notes[at].candidateRead,
      copyRead: decided.notes[at].copyRead,
      consented: decided.notes[at].consented,
    });
  });
  vm.intent = decided.intent;
  vm.complete = decided.complete;
  return vm;
}

/** `SessionPromoteVm.revision`: the README, every workspace entry and every row's target. */
async function promoteRevision(fixture: PromoteFixture): Promise<string> {
  const targets: Record<string, string> = {};
  for (const row of fixture.rows) {
    if ("target" in row) targets[row.target] = await promoteTargetFact(fixture, row.target);
  }
  return promoteSnapshotRevision(await promoteReadme(fixture), promoteStamps(fixture), targets);
}

/** Why `cell` cannot be a cell of the table, as `promote::upsert_row` refuses it. */
function cellRefusal(cell: string): string | null {
  return /[|\p{Cc}]/u.test(cell) || cell.trim() !== cell
    ? `\`${cell}\` cannot be a cell of the ## Promote table — a \`|\`, a line break or padding would make the row read back as something else — so nothing was promoted.`
    : null;
}

/** A promotion of `source` to `target` with its row, as `keeper_agent::promote` decides it. */
async function promoteInFixture(payload: Record<string, unknown>): Promise<null> {
  const fixture = promoteFixture(payload);
  const source = String(payload.source);
  const target = String(payload.target);
  const note = String(payload.note ?? "");
  if (!fixture.table) {
    promoteRefusal(NO_TABLE);
  }
  for (const cell of [source, target, note]) {
    const refused = cellRefusal(cell);
    if (refused !== null) promoteRefusal(refused);
  }
  const file = fixture.files.get(source);
  if (file === undefined) {
    promoteRefusal(`${source} is not in this session any more.`);
  }
  const out = !target.startsWith("artifacts/");
  const harvested = source.startsWith("artifacts/knowledge/");
  // What goes out to the vault: the source's text, every byte kept.
  let published: string | null = null;
  if (!out) {
    if (!source.startsWith("workspace/")) {
      promoteRefusal(
        `a promotion copies a file of workspace/ into artifacts/; ${source} → ${target} is not one.`,
      );
    }
    if (source === "workspace/notes.md") {
      promoteRefusal(`${source} is still being written; try again in a moment.`);
    }
  } else {
    if (fixture.outRefused !== null) {
      promoteRefusal(fixture.outRefused);
    }
    if (!source.startsWith("artifacts/")) {
      promoteRefusal(
        `only an artifact is promoted into the notes vault; ${source} is not under artifacts/.`,
      );
    }
    const text = fileText(file);
    if (text === null) {
      promoteRefusal(
        `${source} is not a text file of this session, and the notes vault takes text.`,
      );
    }
    if (!target.startsWith(`${PROMOTE_VAULT}/`)) {
      promoteRefusal(
        `${target} is outside tgdrive's notes vault (${PROMOTE_VAULT}), and keeper creates new files only inside the vault it manages. Files already there can still be edited and deleted.`,
      );
    }
    // The source's row — its first — and only when it names this target and
    // records the copy there as the one it published (R244, R253).
    const named = fixture.rows.find((row) => "source" in row && row.source === source);
    const there = fixture.vault.get(target);
    if (there !== undefined) {
      if (named === undefined || !("source" in named) || named.target !== target) {
        promoteRefusal(
          `"${target.slice(target.lastIndexOf("/") + 1)}" is already in this folder. Pick another name — keeper will not write over a file you did not name.`,
        );
      }
      const loss = standing(named, there);
      if (loss !== null) {
        promoteRefusal(copyLoss(loss, source, target));
      }
    }
    const expected = payload.expected ?? null;
    if (harvested && expected === null) {
      promoteRefusal(
        "promoting a harvested note into the notes vault is a person's review of the version they read, and this one names no version; open it and promote it from there.",
      );
    }
    if (expected !== null && expected !== (await sha256Hex(text))) {
      promoteRefusal(
        `${source} changed since it was read for this promotion; read it again before promoting it.`,
      );
    }
    if (
      harvested &&
      utf8.encode(vaultText({ text, reviewers: [PROMOTE_ME] })).length > PROMOTE_MAX_REVIEWED_BYTES
    ) {
      promoteRefusal(
        `${source} would hold more than the ${PROMOTE_MAX_REVIEWED_BYTES} bytes keeper reads of a reviewed note, so nothing was written.`,
      );
    }
    published = text;
  }
  const at = fixture.rows.findIndex((row) => "source" in row && row.source === source);
  const recorded = { source, target, note, ...(published !== null ? { published } : {}) };
  if (at < 0) {
    fixture.rows.push(recorded);
  } else {
    fixture.rows[at] = recorded;
  }
  const changed = Math.max(...[...fixture.files.values()].map((held) => held.changed)) + 1;
  if (published !== null) {
    fixture.vault.set(target, {
      text: published,
      reviewers: harvested ? [PROMOTE_ME] : [],
      changed,
    });
  } else {
    fixture.files.set(target, { ...file, changed });
  }
  return null;
}

/** `sessions_promote`: into the session's `artifacts/` only. */
function promoteIntoFixture(payload: Record<string, unknown>): Promise<null> {
  if (!/^artifacts\/./.test(String(payload.target))) {
    promoteRefusal(
      `a promotion copies a file of workspace/ into artifacts/; ${String(payload.source)} → ${String(payload.target)} is not one.`,
    );
  }
  return promoteInFixture({ ...payload, expected: null });
}

/**
 * `sessions_promote_to`: the target composed from the person's folder and
 * filename inside the vault only, as `keeper_core::sessions::offer::compose_target`
 * does, and the note column decided as `promote::offer::promote_to` decides it.
 */
function promoteToFixture(payload: Record<string, unknown>): Promise<null> {
  const fixture = promoteFixture(payload);
  const source = String(payload.source);
  const folder = String(payload.folder);
  const name = String(payload.name);
  const inside =
    folder === PROMOTE_VAULT || (folder.startsWith(`${PROMOTE_VAULT}/`) && !folder.endsWith("/"));
  if (!inside || folder.split("/").some((part) => ["", ".", ".."].includes(part))) {
    promoteRefusal(
      `${folder} is not a folder of this drive's notes vault (${PROMOTE_VAULT}), so nothing was promoted.`,
    );
  }
  if (["", ".", ".."].includes(name) || /[/\\\p{Cc}]/u.test(name) || name.trim() !== name) {
    promoteRefusal(
      `\`${name}\` is not a filename keeper writes a note under, so nothing was promoted.`,
    );
  }
  const row = fixture.rows.find((entry) => "source" in entry && entry.source === source);
  const note = source.startsWith("artifacts/knowledge/")
    ? "knowledge"
    : row !== undefined && "note" in row
      ? row.note
      : "";
  return promoteInFixture({ ...payload, target: `${folder}/${name}`, note });
}

/** Where the row of the harvested note `path` names its vault copy, and that copy if it is there. */
function noteCopy(fixture: PromoteFixture, path: unknown) {
  // The note's one row (`promote::entry_of`), when it names a vault copy.
  const row = fixture.rows.find((candidate) => "source" in candidate && candidate.source === path);
  if (row === undefined || !("target" in row) || row.target.startsWith("artifacts/")) {
    return undefined;
  }
  return { row, target: row.target, copy: fixture.vault.get(row.target) };
}

const NOT_PROMOTED =
  "a person's review is written into the vault's copy of a note; promote it to notes first.";

/**
 * `sessions_knowledge_read`: the candidate or its vault copy, with the
 * revision read, refused as `offer::read_note` refuses — a file it cannot
 * read, or past the candidate's or the reviewed copy's bound.
 */
async function readNoteFixture(payload: Record<string, unknown>) {
  const fixture = promoteFixture(payload);
  const path = String(payload.path);
  if (!path.startsWith("artifacts/knowledge/")) {
    promoteRefusal(`${path} is not a harvested note: those live under artifacts/knowledge/.`);
  }
  if (payload.copy === true) {
    const found = noteCopy(fixture, path);
    if (found === undefined) promoteRefusal(NOT_PROMOTED);
    if (found.copy === undefined) promoteRefusal(`${found.target} is not there any more.`);
    const text = vaultText(found.copy);
    const why = readRefusal(
      found.target,
      utf8.encode(text).length,
      PROMOTE_MAX_REVIEWED_BYTES,
      fixture.unreadable[found.target] === true,
    );
    if (why !== null) promoteRefusal(why);
    return { text, revision: await promoteCopyRevision(found.target, text) };
  }
  const file = fixture.files.get(path);
  if (file === undefined) promoteRefusal(`${path} is not there any more.`);
  const bytes = fileBytes(file);
  const why = readRefusal(
    path,
    bytes.length,
    PROMOTE_MAX_NOTE_BYTES,
    fixture.unreadable[path] === true,
  );
  if (why !== null) promoteRefusal(why);
  const text = utf8Text(bytes) ?? promoteRefusal(`${path} could not be read: ${NOT_UTF8}`);
  return { text, revision: await sha256Hex(text) };
}

/** `sessions_knowledge_review`: the tick lands in the vault copy as it was read. */
async function reviewInFixture(payload: Record<string, unknown>): Promise<null> {
  const found = noteCopy(promoteFixture(payload), payload.path);
  if (found === undefined) {
    promoteRefusal(NOT_PROMOTED);
  }
  const { row, copy, target } = found;
  if (copy === undefined) {
    promoteRefusal(`${target} is not in the vault any more.`);
  }
  const loss = standing(row, copy);
  if (loss !== null) {
    promoteRefusal(copyLoss(loss, row.source, target));
  }
  if ((await promoteCopyRevision(target, vaultText(copy))) !== payload.expected) {
    promoteRefusal(
      "the notes copy changed since you read it; read it again before recording your review.",
    );
  }
  const reviewers = copy.reviewers.filter((by) => by !== PROMOTE_ME);
  if (payload.reviewed === true) reviewers.push(PROMOTE_ME);
  const grown = utf8.encode(vaultText({ text: copy.text, reviewers })).length;
  if (grown > PROMOTE_MAX_REVIEWED_BYTES && grown > utf8.encode(vaultText(copy)).length) {
    promoteRefusal(
      `${target} would hold more than the ${PROMOTE_MAX_REVIEWED_BYTES} bytes keeper reads of a reviewed note, so nothing was written.`,
    );
  }
  copy.reviewers = reviewers;
  return null;
}

/**
 * `sessions_archive` with the checklist's choices, as `promote::offer::archive`
 * decides it: refused when the workspace cannot be listed whole, when the
 * checklist's revision is not the fixture's now, unless every row has
 * exactly one choice at its revision (`offer::archive_promotions`), and for
 * a promotion that Rust's admission refuses — a row that offers none, a
 * cell the table cannot hold, a source still being written — all before
 * anything changes; then every row is recorded, every copy made and the
 * workspace emptied.
 */
async function archiveFixture(payload: Record<string, unknown>): Promise<null> {
  const fixture = promoteFixture(payload);
  const problems = promoteProblems(fixture);
  if (problems.length > 0) {
    promoteRefusal(
      `${problems.join("; ")}; the workspace cannot be checked whole, so nothing was archived.`,
    );
  }
  if (payload.revision !== (await promoteRevision(fixture))) {
    promoteRefusal(SNAPSHOT_CHANGED);
  }
  const vm = await promotePanel(fixture);
  const items = [
    ...vm.rows.map((row) => ({
      revision: row.revision,
      name: row.source || (row.raw ?? ""),
      refused: row.refused,
    })),
    ...vm.unlisted.map((file) => ({
      revision: file.revision,
      name: file.source,
      refused: file.refused,
    })),
  ];
  const choices = (payload.choices ?? []) as ChoiceVm[];
  const byRevision = new Map<string, ChoiceVm>();
  for (const choice of choices) {
    if (
      !items.some((item) => item.revision === choice.revision) ||
      byRevision.has(choice.revision)
    ) {
      promoteRefusal(SNAPSHOT_CHANGED);
    }
    byRevision.set(choice.revision, choice);
  }
  const promotes: [string, string][] = [];
  for (const item of items) {
    const choice = byRevision.get(item.revision);
    if (choice === undefined) {
      promoteRefusal(
        `${item.name} has no choice yet; promote or skip every row before archiving, so nothing was archived.`,
      );
    }
    if (!choice.promote) continue;
    if (item.refused !== null) promoteRefusal(item.refused);
    promotes.push([item.name, choice.target]);
  }
  for (const [source, target] of promotes) {
    if (!/^workspace\/./.test(source) || !/^artifacts\/./.test(target)) {
      promoteRefusal(
        `a promotion copies a file of workspace/ into artifacts/; ${source} → ${target} is not one.`,
      );
    }
    if (!fixture.table) promoteRefusal(NO_TABLE);
    const row = fixture.rows.find((entry) => "source" in entry && entry.source === source);
    for (const cell of [source, target, row !== undefined && "note" in row ? row.note : ""]) {
      const refused = cellRefusal(cell);
      if (refused !== null) promoteRefusal(refused);
    }
    if (source === "workspace/notes.md") {
      promoteRefusal(`${source} is still being written; try again in a moment.`);
    }
  }
  const changed = Math.max(...[...fixture.files.values()].map((file) => file.changed)) + 1;
  for (const [source, target] of promotes) {
    const at = fixture.rows.findIndex((row) => "source" in row && row.source === source);
    const row = at < 0 ? undefined : fixture.rows[at];
    const note = row !== undefined && "note" in row ? row.note : "";
    if (at < 0) fixture.rows.push({ source, target, note });
    else fixture.rows[at] = { source, target, note };
    const from = fixture.files.get(source);
    fixture.files.set(target, { text: from?.text ?? "", bytes: from?.bytes, changed });
  }
  if (payload.emptyWorkspace === true) {
    for (const path of [...fixture.files.keys()]) {
      if (path.startsWith("workspace/") && path !== "workspace/.gitkeep") {
        fixture.files.delete(path);
      }
    }
  }
  return null;
}

/**
 * A promote fixture's file by its path in the drive of `profileId`, as
 * `sync_read_text` reads it from disk: a session file's exact bytes from that
 * root's fixture of the session, or a vault copy — the drive's, whichever of
 * its sessions promoted it — with each review written in.
 */
function promoteFileBytes(profileId: string, subpath: string): Uint8Array | undefined {
  for (const [sessionId, inDrive] of Object.entries(PROMOTE_SESSIONS_IN_DRIVE)) {
    if (subpath.startsWith(`${inDrive}/`)) {
      const file = promoteFixture({ rootId: profileId, sessionId }).files.get(
        subpath.slice(inDrive.length + 1),
      );
      return file === undefined ? undefined : fileBytes(file);
    }
  }
  if (!subpath.startsWith(`${PROMOTE_VAULT}/`)) {
    return undefined;
  }
  const roots = [...PROMOTE_FIXTURES.entries()].filter(([key]) =>
    key.startsWith(promoteKey(profileId, "")),
  );
  const copy =
    roots.map(([, fixture]) => fixture.vault.get(subpath)).find((found) => found !== undefined) ??
    (roots.length === 0
      ? promoteFixture({ rootId: profileId, sessionId: PROMOTE_SESSION }).vault.get(subpath)
      : undefined);
  return copy === undefined ? undefined : utf8.encode(vaultText(copy));
}

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
  room: null,
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
    // Its schedule came due and this host served it on time (Story 74.5).
    trigger: "scheduled",
    lateByMs: 0,
    host: "01DEVICE#4188",
    ledgerEntry: null,
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
      trigger: "requested",
      lateByMs: null,
      host: "01DEVICE#912",
      ledgerEntry: null,
    },
    {
      id: 410,
      taskId: "01JRELEASESWEEPBBBBBBBBBBB",
      startedMs: ago(362),
      finishedMs: ago(360),
      outcome: "ok",
      unknownOutcome: null,
      detail: "looked at 1 284 objects, released 96, reclaimed 74 GB",
      // The host timer woke and ran it, 41 minutes past the window — the
      // shape that was invisible before AD-253.
      trigger: "timer",
      lateByMs: 41 * 60_000,
      host: "01DEVICE#912",
      ledgerEntry: null,
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
      trigger: "scheduled",
      lateByMs: 0,
      host: "01DEVICE#4188",
      ledgerEntry: null,
    },
    {
      id: 501,
      taskId: "01JVAULTPUSHCCCCCCCCCCCCCC",
      startedMs: ago(69),
      finishedMs: ago(68),
      outcome: "deferred",
      unknownOutcome: null,
      detail: "0 synced, 0 already syncing, 1 waiting, 0 failed",
      trigger: "scheduled",
      lateByMs: 0,
      host: "01DEVICE#4188",
      ledgerEntry: null,
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
      // Recorded before AD-253: the row says nothing about why it ran,
      // rather than claiming a schedule it cannot vouch for.
      trigger: null,
      lateByMs: null,
      host: "01DEVICE#77",
      ledgerEntry: null,
    },
  ],
};

/**
 * The per-kind fields a `sync`, `release`, `verify` or `gc` row leaves empty.
 *
 * Every key exists on every kind on the wire, so a fixture that omitted them
 * would be a shape the shell never sends — and the point of this harness is to
 * answer exactly what the shell answers.
 */
const NO_KIND_PAYLOAD = {
  copySource: null,
  copyDestination: null,
  copySourceFacts: null,
  copyDestinationFacts: null,
  replaceExisting: false,
  pruneDestination: false,
  refreshMissing: true,
  copyLookbackMs: 300_000,
  ledgerPath: null,
  // No ledger folder in the harness's fixtures, so no mark (Story 74.4).
  markMs: null,
} satisfies Partial<TaskVm>;

const TASKS: TaskVm[] = [
  {
    id: "01JNIGHTLYSYNCAAAAAAAAAAAA",
    ...NO_KIND_PAYLOAD,
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
    ...NO_KIND_PAYLOAD,
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
    ...NO_KIND_PAYLOAD,
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
    ...NO_KIND_PAYLOAD,
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
    ...NO_KIND_PAYLOAD,
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
    ...NO_KIND_PAYLOAD,
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
    ...NO_KIND_PAYLOAD,
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
  {
    ...NO_KIND_PAYLOAD,
    id: "01JCOPYNOTESGGGGGGGGGGGGGG",
    kind: "copy",
    mode: "manual",
    enabled: true,
    profileId: null,
    profile: null,
    schedule: "@daily",
    description: "copy notes to the light drive",
    copySource: "/Volumes/merope/tgdrive/10-notes",
    copyDestination: "/Volumes/merope/tgdrive-light/10-notes",
    copySourceFacts: {
      path: "/Volumes/merope/tgdrive/10-notes",
      drive: "tgdrive",
      driveId: "p1",
      exists: true,
    },
    copyDestinationFacts: {
      path: "/Volumes/merope/tgdrive-light/10-notes",
      drive: "tgdrive-light",
      driveId: "p3",
      exists: true,
    },
    onMissed: "run_now",
    missedDelayMs: null,
    nextDueMs: null,
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 9000,
    lastRun: null,
    host: { kind: "onRequest", sentence: HOST_SENTENCES.onRequest, reason: null },
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
  // `?retired=1`: an install upgraded from a build that ran prompts on a
  // schedule, whose two stored tasks were removed at first launch (Story
  // 108.3a, UX-DR-E108-3) — the Tasks list's one notice.
  retired: new URLSearchParams(window.location.search).has("retired")
    ? ["Morning digest", "Weekly review"]
    : [],
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
// Model providers (Story 108.3a) — the endpoints notes search asks. Three
// tenants of different kinds: a loopback Ollama with no key, a LAN Hermes with
// one, and a CLIProxyAPI on the tailnet whose `/v1/models` states no
// capability.
// ---------------------------------------------------------------------------

const MODEL_PROVIDERS: ModelProviderVm[] = [
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
  {
    id: "01J8BOTPROVOPENAIAAAAAAAAA",
    kind: "openai",
    name: "CLIProxyAPI on electra",
    baseUrl: "https://electra.example.ts.net:8452",
    host: "electra.example.ts.net",
    isPrivate: false,
    createdMs: NOW - 86_400_000,
    health: "reachable",
    healthCheckedMs: NOW - 300_000,
    healthDetail: null,
    readTimeoutMs: null,
    hasToken: true,
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
    embedding: null,
    capabilities: ["completion", "tools"],
  };
}

/** Per-provider model rosters, with the tri-state exercised on purpose: an
 *  OpenAI-compatible list reports nothing about vision, which is what an
 *  `unknown` capability looks like on screen. */
const PROVIDER_MODELS: Record<string, BotModelVm[]> = {
  "01J8BOTPROVOLLAMAAAAAAAAAA": [
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
      embedding: null,
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
      embedding: null,
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
      embedding: null,
      capabilities: [],
    },
  ],
  // `/v1/models` names models and nothing else, so every capability of an
  // OpenAI-compatible endpoint is unknown.
  "01J8BOTPROVOPENAIAAAAAAAAA": [
    {
      id: "gpt-6-astra",
      family: null,
      parameterSize: null,
      quantization: null,
      sizeBytes: null,
      contextWindow: null,
      maxOutputTokens: null,
      vision: null,
      tools: null,
      reasoning: null,
      embedding: null,
      capabilities: [],
    },
  ],
};

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

/**
 * The dev shell's answer to `chat_command_preview` (Story 100.5).
 *
 * **A crude fake on purpose**: the registry, the room sets, the power-level
 * reasons and every sentence live in
 * `keeper_core::chat::commands`; this only draws each shape once. A room the
 * harness lists as an agent's gets the agent set (the Nixi DM also `/new`);
 * every other room gets the chat set. `?slash=readonly` answers as a room whose
 * power levels allow neither a topic nor an invite, so the reasons show.
 * `/invite @refused:example.org` is refused by the fake server
 * (`room_invite`), so the failure sentence shows too.
 */
/** Every message the composer sent, in order — read back by the browser proof. */
const mockSends: Array<{ kind: string; body: string; line: string | null }> = [];
(window as unknown as { __keeperMockSends: typeof mockSends }).__keeperMockSends = mockSends;

/** Every room `/leave` left, as `accountId roomId` — read back by the browser proof. */
const mockLeaves: string[] = [];
(window as unknown as { __keeperMockLeaves: typeof mockLeaves }).__keeperMockLeaves = mockLeaves;

function recordSend(kind: string, payload: Record<string, unknown>): null {
  mockSends.push({
    kind,
    body: String(payload.body ?? ""),
    line: payload.line == null ? null : String(payload.line),
  });
  return null;
}

function mockChatCommandPreview(
  payload: Record<string, unknown>,
  offered?: ChatCommandRowVm[],
): ChatCommandPreviewVm {
  const draft = String(payload.draft ?? "");
  const replying = payload.replying === true;
  const readonly = new URLSearchParams(window.location.search).get("slash") === "readonly";
  const escapeHint =
    "To send a message that starts with a slash, double it: //etc sends /etc as text.";
  type Spec = [string, string[], string, ChatCommandRowVm["args"], string | null, string | null];
  const chatSet: Spec[] = [
    [
      "me",
      ["emote"],
      'Send the text as an action, the way "* Ana waves" reads.',
      "required",
      "what you do",
      replying
        ? "An action is sent as its own message, not as a reply. Cancel the reply first."
        : null,
    ],
    [
      "shrug",
      [],
      "Send ¯\\_(ツ)_/¯, before your text if you add some.",
      "optionalRest",
      "text",
      null,
    ],
    [
      "plain",
      [],
      "Send the text exactly as typed, even when it looks like a command.",
      "required",
      "text",
      null,
    ],
    [
      "topic",
      [],
      "Change this room's topic for everyone in it.",
      "required",
      "the new topic",
      readonly ? "Your power level in this room does not let you change its topic." : null,
    ],
    [
      "invite",
      [],
      "Invite someone to this room by their Matrix id.",
      "required",
      "@name:server",
      readonly ? "Your power level in this room does not let you invite people." : null,
    ],
    ["leave", ["part"], "Leave this room, after you confirm.", "none", null, null],
  ];
  const help: Spec = [
    "help",
    ["h", "commands"],
    "List the commands this room offers.",
    "none",
    null,
    null,
  ];
  const registry: ChatCommandRowVm[] =
    offered ??
    [...chatSet, help].map(([name, aliases, description, args, argHint, reason]) => ({
      name,
      aliases,
      description,
      args,
      argHint,
      available: reason === null,
      reason,
    }));
  const line = draft.startsWith("/") && !draft.startsWith("//") && !draft.includes("\n");
  if (line && new TextEncoder().encode(draft).length > 512) {
    return {
      draft,
      verdict: {
        kind: "refusal",
        message:
          "That line is too long for a command, so nothing was sent. Shorten it, or start it with // to send it as a message.",
      },
      rows: [],
      escapeHint,
    };
  }
  if (!line) {
    const text = draft.startsWith("//") ? draft.slice(1) : draft;
    return { draft, verdict: { kind: "text", text }, rows: [], escapeHint };
  }
  const token = draft.slice(1).split(/\s/, 1)[0]?.toLowerCase() ?? "";
  const args = draft.slice(1 + token.length).trim();
  const spelled = (row: ChatCommandRowVm, prefix: boolean) =>
    [row.name, ...row.aliases].some((name) => (prefix ? name.startsWith(token) : name === token));
  const exact = registry.find((row) => spelled(row, false));
  const rows = exact !== undefined ? [exact] : registry.filter((row) => spelled(row, true));
  if (token === "") {
    return { draft, verdict: { kind: "text", text: draft }, rows, escapeHint };
  }
  const spec = rows[0];
  if (spec === undefined) {
    return {
      draft,
      verdict: {
        kind: "refusal",
        message: `keeper has no /${token} command. Nothing was sent. Type /help for the list. ${escapeHint}`,
      },
      rows,
      escapeHint,
    };
  }
  const refuse = (message: string): ChatCommandPreviewVm => ({
    draft,
    verdict: { kind: "refusal", message },
    rows,
    escapeHint,
  });
  if (spec.reason !== null) {
    return refuse(`/${spec.name} — ${spec.reason}`);
  }
  if (spec.args === "required" && args === "") {
    return refuse(
      `/${spec.name} needs ${spec.argHint}, so nothing was sent. Type /${spec.name} followed by it.`,
    );
  }
  if (spec.args === "none" && args !== "") {
    return refuse(
      `/${spec.name} takes nothing after it, so nothing was sent. Send /${spec.name} on its own.`,
    );
  }
  const verdict: ChatCommandPreviewVm["verdict"] =
    spec.name === "me"
      ? { kind: "emote", text: args }
      : spec.name === "plain"
        ? { kind: "text", text: args }
        : spec.name === "shrug"
          ? { kind: "text", text: args === "" ? "¯\\_(ツ)_/¯" : `¯\\_(ツ)_/¯ ${args}` }
          : { kind: "command", name: spec.name, args: args === "" ? null : args };
  return { draft, verdict, rows, escapeHint };
}

/**
 * The persisted "don't open setup when keeper starts" answer (spec *Skipping
 * setup can stick*). A module-level `let` rather than an `ANSWERS` entry for the
 * reason the toggle above gives: the write needs somewhere to land, so ticking
 * the box in the skip-confirm and reading it back answers what was just said
 * instead of a constant. A page reload re-runs this module and starts at
 * `false` — this harness has no disk, and a fresh install is the state it
 * therefore always boots in.
 */
let firstRunSetupSkipped = false;

/** Session ids removed in this page's life: the Recordings pane stops listing them. */
const removedRecordings = new Set<string>();

/**
 * The effective recording settings, held in a `let` so a write lands somewhere.
 * Not a disk: a page reload starts from these values again.
 */
let recordingSettings: RecordingSettingsVm = {
  codec: "hevc",
  fps: 60,
  scalePercent: 100,
  segmentMb: 250,
  durationCapMinutes: 60,
  destinationDir: "/Volumes/merope/tgdrive/recordings",
  destinationKind: "profile",
  destinationProfileId: "p1",
  destinationProfileName: "tgdrive",
  destinationVolume: null,
  pathTemplate: "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}",
  echoCancellation: false,
};

/**
 * Which sources the next session captures (spec *Recording remembers which
 * sources are on*), in `sessionStorage` so the manual check the spec asks for —
 * turn one off, reload, it is still off — actually reproduces here. All three
 * ship ON, as the registry defaults do, so a first visit shows what a fresh
 * install shows.
 */
const CAPTURE_SOURCES_KEY = "keeper.mock-shell.captureSources";

function loadCaptureSources(): RecordingCaptureSourcesVm {
  const shipped: RecordingCaptureSourcesVm = { systemAudio: true, microphone: true, camera: true };
  try {
    const stored = sessionStorage.getItem(CAPTURE_SOURCES_KEY);
    return stored === null ? shipped : { ...shipped, ...JSON.parse(stored) };
  } catch {
    return shipped;
  }
}

let captureSources: RecordingCaptureSourcesVm = loadCaptureSources();

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

/** What a spoken turn does without the turn models, as Rust words it from
 *  `END_OF_UTTERANCE_PAUSE` (`keeper_core::voice::turn_models`). */
const TURN_FALLBACK = "keeper waits 1.8 s after you stop";
/**
 * The turn models' line (Epic 97, UX-DR142): every `TurnModelsState` as
 * `TurnModelsState::vm` sends it, chosen with `?turnModels=<state>` on the dev
 * URL. `missing` — the default — is the owner's account today: `models.toml`
 * names no turn model yet. `files` is a hydration that lacks a file, `stale`
 * one the account has moved on from, `none` a device whose voice runs no turn
 * models (the field absent). `fetching` lands as `ready` eight seconds after
 * the first read and is sent on `keeper://voice-wake`, as the shell's
 * `voice_ipc::announce_wake` sends a fetch's end, so the line can be watched
 * moving with nothing re-reading it.
 */
const TURN_MODELS: Record<string, VoiceWakeVm["turnModels"]> = {
  ready: { state: "ready", sentence: "Turn models ready", missing: [] },
  missing: {
    state: "missing",
    sentence: `Turn models missing: [vad] in models.toml, [smart_turn] in models.toml — ${TURN_FALLBACK}`,
    missing: ["[vad] in models.toml", "[smart_turn] in models.toml"],
  },
  files: {
    state: "missing",
    sentence: `Turn models missing: silero-vad/model.onnx — ${TURN_FALLBACK}`,
    missing: ["silero-vad/model.onnx"],
  },
  stale: {
    state: "missing",
    sentence: `Turn models out of date — keeper brings them up to date after the next sync, and ${TURN_FALLBACK} until then`,
    missing: [],
  },
  refused: {
    state: "failed",
    sentence: `The speech detection model “silero-v5” set by \`transcription.vad_model\` is not on this device. Set \`transcription.vad_model\` in your account's settings.toml to another folder of _models/, or remove it to use the one [vad] in models.toml names. Until then, ${TURN_FALLBACK}.`,
    missing: [],
  },
  failed: {
    state: "failed",
    sentence: `The turn models could not be fetched: the config repository did not answer — ${TURN_FALLBACK}`,
    missing: [],
  },
  noAccount: {
    state: "noAccount",
    sentence: `No turn models without an account — ${TURN_FALLBACK}. They come from your account's settings repository (Settings → Account).`,
    missing: [],
  },
  fetching: {
    state: "fetching",
    sentence: `Fetching the turn models from your account… ${TURN_FALLBACK} until they are here`,
    missing: [],
  },
  none: undefined,
};
const turnModelsParam = new URLSearchParams(window.location.search).get("turnModels") ?? "missing";
let turnModelsKey = turnModelsParam in TURN_MODELS ? turnModelsParam : "missing";
let turnModelsLanding = false;
/** The wake VM as `wake_vm` answers every voice settings command: the
 *  stored settings with the turn models' line as it is now. */
function voiceWakeVm(): VoiceWakeVm {
  if (turnModelsKey === "fetching" && !turnModelsLanding) {
    turnModelsLanding = true;
    window.setTimeout(() => {
      turnModelsKey = "ready";
      void emit(VOICE_WAKE_EVENT, voiceWakeVm());
    }, 8_000);
  }
  return { ...voiceWake, turnModels: TURN_MODELS[turnModelsKey] };
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
  stopPhrase: "stop",
  voiceTarget: null,
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

/**
 * The background-update plan, as `keeper_core::update::plan` builds it for an
 * install that has never answered: supported, on, with the shipped cadence.
 *
 * A module-level `let` with a round-tripping setter rather than an `ANSWERS`
 * row, for this file's usual reason: the switch's whole behaviour is that the
 * write is echoed back and applied, and a read-only answer would show a switch
 * that snaps back and teach the reviewer the toggle is broken. The loop's own
 * `plugin:updater|check` is not answered here, so nothing in the dev rig ever
 * downloads anything.
 */
let autoUpdate: AutoUpdateVm = {
  supported: true,
  enabled: true,
  firstCheckDelayMs: 120_000,
  checkIntervalMs: 21_600_000,
  retryDelayMs: 1_800_000,
  restartCheckIntervalMs: 60_000,
};

/**
 * `diagnostics.log_upload`, on for an install that never answered. Round-trips
 * for the same reason `autoUpdate` does: the switch applies the echo.
 */
let logUpload = true;
// Diagnostics & studies (102.1, R462): consent is account-wide in Rust; the
// mock keeps it in memory so the switches show their on and off states. No
// request ever leaves the browser shell, and no project token exists here.
// `?telemetry=pinned`: a settings file pins diagnostics off, so a save is
// kept but overruled (R495 06) — the switch stays off and says why.
const telemetryPinned = new URLSearchParams(window.location.search).get("telemetry") === "pinned";
let telemetryConsent = { diagnostics: false, productAnalytics: false, remoteConfig: false };
const telemetryStatus = () => {
  const consent = telemetryPinned ? { ...telemetryConsent, diagnostics: false } : telemetryConsent;
  return {
    consent,
    configured: true,
    host: "https://us.i.posthog.com",
    installationId: Object.values(consent).some(Boolean)
      ? "0123456789abcdef0123456789abcdef"
      : null,
    consentPinned: telemetryPinned,
  };
};

// A browser has no filesystem. These are explicitly fixture files, dated on
// either side of a day boundary so the date controls can be exercised honestly.
const COPY_FIXTURES = [
  { path: "mock-old.txt", bytes: 32, mtimeMs: Date.UTC(2026, 8, 14) },
  { path: "mock-new.txt", bytes: 64, mtimeMs: Date.UTC(2026, 8, 16) },
];
const copyJobs = new Map<string, CopyJobVm>();

// ---------------------------------------------------------------------------
// The optional account (Epic 82).
//
// Signed out by default — "no account configured", the state every install
// starts in and the one that must look like keeper before accounts existed.
// `?account=ready` (or `offline`, `needsSignIn`, `blocked`, `signedOut`) boots
// into a signed-in fixture instead, so Settings › Account, the status line and
// the credential choices on the drive and endpoint forms can all be looked at.
// `ready` also boots just restored (Epic 85, AD-329): the restore sentence, one
// drive waiting for its volume, and listening that was on for this device.
// Pasting any link into Settings › Account walks the whole setup sheet —
// through "Finish signing in…" (with Cancel sign-in) and the sync — and the
// word `refuse` anywhere in it answers Rust's refusal instead, and the word
// `registered` answers a device the repository already knows (UX-DR119).
// The word `denied` walks the sign-in to the identity provider's refusal
// (`access_denied`, Epic 100, R381): the sheet and Settings › Account then
// show Rust's problem sentence as an alert. The word `forge` walks it to a
// repository connection that failed after the identity sign-in (R387), the
// same way. Cancel sign-in during a re-setup of `?account=ready` keeps the
// account up to date and ends the attempt `closed` (R387).
//
// Every answer is what Rust would answer, sentences included (`state.rs`
// `compose`), and every snapshot carries the next `revision`, as Rust's do. An
// unforced `account_sync` inside the 15-minute throttle answers the VM
// unchanged, so a focus in the browser does not wipe the fixture being looked
// at; boot counts as the last attempt, as the shell's own boot sync does.
// ---------------------------------------------------------------------------

const NO_ACCOUNT_VM: OrgAccountVm = {
  configured: false,
  id: null,
  name: null,
  issuerHost: null,
  repoHost: null,
  repoMode: null,
  state: "none",
  sentence: null,
  problem: false,
  attempt: null,
  identity: null,
  device: null,
  devices: [],
  lastSyncedMs: null,
  forgeConnected: false,
  faults: [],
  offers: { drives: [], providers: [], matrix: [] },
  restore: { sentence: null, pending: [], listeningOff: false },
  revision: 0,
};

const ACCOUNT_THIS_DEVICE: AccountDeviceVm = {
  slug: "hesperia",
  name: "hesperia",
  class: "desktop",
  platform: "macos",
  version: "0.8.33",
  thisDevice: true,
};

/**
 * What the person uses on their other devices and this one lacks (Epic 84,
 * AD-323): two drives (one signing with the account, one with its own token),
 * a model provider that uses the account, and a Matrix
 * account. Mutable because adding one here takes its offer away — the one-tap
 * endpoint add, a folder save for the same remote, a Matrix sign-in — as the
 * shell's next publish does once the thing exists on this device.
 */
let accountOffers: AccountOffersVm = {
  drives: [
    {
      key: "drive:https://git.acme.dev/tgorka/notes#main^notes",
      name: "notes",
      remoteUrl: "https://git.acme.dev/tgorka/notes.git",
      branch: "main",
      credential: "account",
      notes: "notes",
      recordings: null,
      voices: null,
      sessions: null,
      tasks: "tasks",
      agents: null,
      excludes: [".DS_Store", "*.tmp"],
      lfsThresholdBytes: 8 * 1024 * 1024,
      virtualPatterns: ["attachments/**"],
      virtualOverBytes: null,
      releaseTtlMs: null,
      tags: ["notes"],
      commitSubjectTemplate: null,
      devices: ["iphone-3f2a", "ipad-91c0"],
    },
    {
      key: "drive:git@github.com:tgorka/field-recordings#main^Field recordings",
      name: "Field recordings",
      remoteUrl: "git@github.com:tgorka/field-recordings.git",
      branch: "main",
      credential: "own",
      notes: null,
      recordings: "recordings",
      voices: "70-comms/voices",
      sessions: null,
      tasks: null,
      agents: null,
      excludes: [],
      lfsThresholdBytes: null,
      virtualPatterns: null,
      virtualOverBytes: 64 * 1024 * 1024,
      releaseTtlMs: 72 * 60 * 60 * 1000,
      tags: [],
      commitSubjectTemplate: "{profile}: {changed} changed",
      devices: ["ipad-91c0"],
    },
  ],
  providers: [
    {
      key: "provider:hermes:https://hermes.acme.dev",
      kind: "hermes",
      name: "Acme Hermes",
      baseUrl: "https://hermes.acme.dev",
      credential: "account",
      devices: ["iphone-3f2a"],
    },
  ],
  matrix: [
    {
      key: "matrix:@tgorka:acme.dev",
      userId: "@tgorka:acme.dev",
      homeserverUrl: "https://matrix.acme.dev/",
      kind: "password",
      devices: ["iphone-3f2a", "ipad-91c0"],
    },
    // Single sign-on (Epic 85, AD-331): choosing it starts the SSO flow.
    {
      key: "matrix:@tgorka:electra.acme.dev",
      userId: "@tgorka:electra.acme.dev",
      homeserverUrl: "https://electra.acme.dev/",
      kind: "oidc",
      devices: ["iphone-3f2a"],
    },
  ],
};

/** A signed-in account in `state`, with the sentence Rust would write for it. */
function signedInAccount(state: AccountStateVm, device = ACCOUNT_THIS_DEVICE): OrgAccountVm {
  const sentences: Partial<Record<AccountStateVm, string>> = {
    ready: "Up to date.",
    syncing: "Syncing your settings…",
    signingIn: "Finish signing in to Acme in the browser.",
    offline: "Offline — using settings from 14:02.",
    needsSignIn: "Sign in again to keep your settings in sync.",
    blocked:
      "This sign-in belongs to someone else: tgorka/user.toml records a different account. Settings from the repository were not loaded.",
    signedOut: "Sign in to Acme to use your settings on this device.",
  };
  const signedOut = state === "signedOut" || state === "signingIn";
  return {
    configured: true,
    id: "acme",
    name: "Acme",
    issuerHost: "id.acme.dev",
    repoHost: "git.acme.dev",
    repoMode: "same",
    state,
    sentence: sentences[state] ?? null,
    // Rust's `problem`: a policy stop and a dead grant are problems; offline
    // with settings in hand, and the signed-out instruction, are not.
    problem: state === "blocked" || state === "needsSignIn",
    attempt: null,
    identity: signedOut
      ? null
      : {
          login: "tgorka",
          displayName: "Tomasz Gorka",
          email: "tgorka@acme.dev",
          roles: ["keeper", "staff"],
        },
    device: signedOut ? null : device,
    devices: signedOut
      ? []
      : [
          device,
          {
            slug: "iphone-3f2a",
            name: "iphone-3f2a",
            class: "mobile",
            platform: "ios",
            version: "0.8.31",
            thisDevice: false,
          },
          {
            slug: "ipad-91c0",
            name: "ipad-91c0",
            class: "tablet",
            platform: "ios",
            version: null,
            thisDevice: false,
          },
        ],
    lastSyncedMs: state === "ready" ? Date.now() : null,
    forgeConnected: false,
    faults: [],
    // Rust composes offers only after a converge has read the repository, and
    // clears them when the directory is not this person's: so `ready` and
    // `syncing` carry them, and `offline`, `blocked` and the rest do not.
    offers: offersIn(state),
    restore: accountRestore,
    revision: 0,
  };
}

const ACCOUNT_FIXTURES: readonly AccountStateVm[] = [
  "ready",
  "offline",
  "needsSignIn",
  "blocked",
  "signedOut",
];
const accountParam = new URLSearchParams(window.location.search).get("account");
/**
 * What the first sync restored (Epic 85, AD-329), as Rust words it. Mutable
 * because turning listening on takes the offer away, as Rust's next compose
 * does once `voice.wake_enabled` is on here.
 */
let accountRestore: AccountRestoreVm =
  accountParam === "ready"
    ? {
        sentence: "Restored 2 drives, 1 model provider and your settings from your account.",
        pending: ["Waiting for /Volumes/Field to restore Field recordings."],
        listeningOff: true,
        ...restoreSignInFixture(),
      }
    : NO_ACCOUNT_VM.restore;

/**
 * `?restoresso=preparing|waits|unopened|timedout` (with `?account=ready`): the
 * Matrix single sign-on the restore started by itself (102.9, R480/02), as
 * `keeper_core::org_account::state` words it — still discovering its server
 * (no link yet, Cancel already reaches it: R500), waiting with its link (and
 * the browser-not-opened sentence), or ended by its deadline with the link
 * gone. `cancel_oidc` ends a waiting one as cancelled.
 */
function restoreSignInFixture(): Pick<AccountRestoreVm, "matrixSignIn"> {
  const param = new URLSearchParams(window.location.search).get("restoresso");
  const host = "matrix.acme.dev";
  if (param === "timedout") {
    return {
      matrixSignIn: {
        sentence: `Signing in to your Matrix account on ${host} timed out before the browser answered, so its link no longer works. Sign in to it again from Add an account.`,
        problem: true,
        waiting: false,
      },
    };
  }
  if (param === "preparing") {
    return {
      matrixSignIn: {
        sentence: `Finish signing in to your Matrix account on ${host} in the browser.`,
        problem: false,
        waiting: true,
      },
    };
  }
  if (param !== "waits" && param !== "unopened") {
    return {};
  }
  return {
    matrixSignIn: {
      sentence: `Finish signing in to your Matrix account on ${host} in the browser.`,
      problem: false,
      waiting: true,
      link: {
        url: mockAuthorizationUrl("mas.acme.dev", "01J8KEEPERRESTORE"),
        notOpened:
          param === "unopened"
            ? "keeper could not open your browser. Copy this link into a browser to finish signing in."
            : null,
      },
    },
  };
}

/** `cancel_oidc` reaching the restore's Matrix sign-in: it ends cancelled, its link gone. */
function cancelRestoreSignIn(): void {
  if (accountRestore.matrixSignIn?.waiting !== true) {
    return;
  }
  accountRestore = {
    ...accountRestore,
    matrixSignIn: {
      sentence:
        "The sign-in to your Matrix account on matrix.acme.dev was cancelled. Sign in to it again from Add an account.",
      problem: false,
      waiting: false,
    },
  };
  if (accountVm.configured) {
    setAccount({ ...accountVm, restore: accountRestore });
  }
}
let accountVm: OrgAccountVm = ACCOUNT_FIXTURES.includes(accountParam as AccountStateVm)
  ? signedInAccount(accountParam as AccountStateVm)
  : NO_ACCOUNT_VM;
/** The one subscriber, so every write can push the new snapshot. */
let accountWatcher: MockChannel<OrgAccountVm> | null = null;
/** Per-drive and per-endpoint credential sources (`keychain` unless chosen). */
const credentialSources = new Map<string, string>();
/** Rust's per-process counter: every snapshot it emits is newer than the last. */
let accountRevision = 0;
/** Rust's throttle: an unforced sync inside this window answers the VM unchanged. */
const ACCOUNT_SYNC_THROTTLE_MS = 15 * 60 * 1000;
/** Boot is the last attempt, as the shell's boot-time background sync makes it. */
let accountLastAttemptMs = Date.now();
/** The open browser round trip, so Cancel sign-in can end it as Rust's registry does. */
let accountSignInCancel: (() => void) | null = null;

function setAccount(next: OrgAccountVm): OrgAccountVm {
  accountRevision += 1;
  accountVm = { ...next, revision: accountRevision };
  accountWatcher?.onmessage?.(accountVm);
  return accountVm;
}

/** The offers a snapshot in `state` carries: Rust has them only after a converge. */
function offersIn(state: AccountStateVm): AccountOffersVm {
  return state === "ready" || state === "syncing" ? accountOffers : NO_ACCOUNT_VM.offers;
}

/**
 * Take offers away once the thing exists here, and publish — what the shell's
 * next converge does. A snapshot that carries none keeps carrying none.
 */
function withdrawOffers(next: AccountOffersVm): void {
  accountOffers = next;
  if (accountVm.configured) {
    setAccount({ ...accountVm, offers: offersIn(accountVm.state) });
  }
}

/**
 * The person chose this device's listening — on, or "Keep it off" (Epic 85,
 * F15) — so the restore's offer goes, as Rust's next compose does.
 */
function listeningDecided(): void {
  if (!accountRestore.listeningOff) {
    return;
  }
  accountRestore = { ...accountRestore, listeningOff: false };
  if (accountVm.configured) {
    setAccount({ ...accountVm, restore: accountRestore });
  }
}

/**
 * One spelling for a remote on both sides of a comparison, close enough to
 * `normalize_remote` for a viewing aid: no trailing `/` or `.git`, one case.
 */
function mockRemoteKey(remoteUrl: string, branch: string): string {
  return `${remoteUrl
    .trim()
    .replace(/\/+$/, "")
    .replace(/\.git$/, "")
    .toLowerCase()}#${branch}`;
}

/**
 * A Matrix sign-in as Rust answers it. SSO names nobody up front, so the id is
 * the offered one on that homeserver when there is one — which is who the
 * browser would have signed in.
 */
/**
 * `?sso=waits` keeps a Matrix single sign-on waiting for its browser until
 * Cancel, and `?sso=unopened` also says the browser could not be opened —
 * the two states story 102.9 draws the authorization link in (R470). Without
 * either the sign-in answers at once, as before, its link sent first.
 */
const ssoParam = new URLSearchParams(window.location.search).get("sso");
/** The pending mock single sign-on, so `cancel_oidc` can end it as Rust's registry does. */
let ssoCancel: (() => void) | null = null;
/** The authorization page a mock sign-in is on: a public client's request, no secret. */
function mockAuthorizationUrl(host: string, client: string): string {
  return `https://${host}/oauth2/authorize?response_type=code&client_id=${client}&redirect_uri=keeper%3A%2F%2Foauth%2Facme%2Fcallback&scope=openid+profile+email&state=Zk3pQ8vX2mR7tL1n&nonce=a8Jd2kLq&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256`;
}

function matrixSignInFlow(payload: Record<string, unknown>): Promise<AccountVm> | AccountVm {
  const homeserver = String(payload.homeserver ?? "");
  const channel = payload.onLink as MockChannel<SignInLinkVm | null> | undefined;
  const host = /^https?:\/\//.test(homeserver) ? new URL(homeserver).host : homeserver;
  const url = mockAuthorizationUrl(`auth.${host || "example.org"}`, "01J8KEEPERCLIENT");
  channel?.onmessage?.({ url, notOpened: null });
  if (ssoParam !== "waits" && ssoParam !== "unopened") {
    // Rust takes the link away when the sign-in ends, however it ends.
    channel?.onmessage?.(null);
    return matrixSignedIn(homeserver, null, "oidc");
  }
  if (ssoParam === "unopened") {
    channel?.onmessage?.({
      url,
      notOpened:
        "keeper could not open your browser. Copy this link into a browser to finish signing in.",
    });
  }
  return new Promise<AccountVm>((_, reject) => {
    ssoCancel = () => {
      ssoCancel = null;
      channel?.onmessage?.(null);
      reject({ code: "oauthCancelled", message: "cancelled", accountId: null, retriable: true });
    };
  });
}

function matrixSignedIn(
  homeserver: string,
  username: string | null,
  provider: AccountVm["provider"],
): AccountVm {
  const homeserverUrl = /^https?:\/\//.test(homeserver) ? homeserver : `https://${homeserver}/`;
  const host = new URL(homeserverUrl).host;
  const offered = accountOffers.matrix.find((offer) => new URL(offer.homeserverUrl).host === host);
  let userId = offered?.userId ?? `@harness:${host}`;
  if (username !== null) {
    userId = username.startsWith("@") ? username : `@${username}:${host}`;
  }
  withdrawOffers({
    ...accountOffers,
    matrix: accountOffers.matrix.filter((offer) => offer.userId !== userId),
  });
  return {
    accountId: `01J8MATRIX${String(Date.now()).padStart(16, "0")}`,
    userId,
    homeserverUrl,
    hueIndex: 1,
    provider,
  };
}

/**
 * A sign-in as Rust runs it: "Finish signing in…" while the browser is open,
 * the repository sync, then the outcome — or, when Cancel sign-in is pressed
 * first, the signed-out state the confirm resolves with (it resolves, it does
 * not reject). The pauses are only long enough to see each state.
 */
/** What Rust composes when the identity provider answers `access_denied` (R381). */
function deniedAccount(): OrgAccountVm {
  return {
    ...signedInAccount("signedOut"),
    state: "blocked",
    sentence:
      "id.acme.dev refused to sign you in to Acme: User is not allowed to use this project.",
    problem: true,
    attempt: "failed",
  };
}

/**
 * What Rust composes when the identity sign-in worked but connecting the `oauth`
 * repository failed — its sign-in page did not open (R387): the failure, as a
 * problem, never the neutral "Reconnect the repository".
 */
function forgeFailedAccount(device: AccountDeviceVm): OrgAccountVm {
  return {
    ...signedInAccount("needsSignIn", device),
    repoMode: "oauth",
    forgeConnected: false,
    sentence:
      "Connecting Acme's repository failed (keeper could not open the repository's sign-in page). Sign in again.",
    problem: true,
    attempt: "failed",
  };
}

/** `?signin=waits|unopened`: the organisation sign-in stays on its browser leg (see below). */
const signInParam = new URLSearchParams(window.location.search).get("signin");

/** How a mock sign-in ends: Rust's `denied` refusal, a failed repository connection, or set up. */
type MockSignInEnd = "denied" | "forgeFailed" | "done";

function accountSignInFlow(
  device: AccountDeviceVm,
  end: MockSignInEnd = "done",
): Promise<OrgAccountVm> {
  // The project's `lib` predates `Promise.withResolvers`.
  let resolve: (vm: OrgAccountVm) => void = () => {};
  const promise = new Promise<OrgAccountVm>((settle) => {
    resolve = settle;
  });
  // An account already signed in here keeps its identity when a re-setup is
  // closed: Rust's state still reads up to date; only `attempt` says closed.
  const before = accountVm.identity !== null ? signedInAccount("ready", device) : null;
  // Rust's link to the authorization page, while the sign-in waits (102.9,
  // R470); `?signin=unopened` says the browser could not be opened, and
  // `?signin=waits` (or `unopened`) keeps it waiting until Cancel sign-in.
  const signInLink: SignInLinkVm = {
    url: mockAuthorizationUrl("id.acme.dev", "keeper-acme"),
    notOpened:
      signInParam === "unopened"
        ? "keeper could not open your browser. Copy this link into a browser to finish signing in."
        : null,
  };
  setAccount({ ...signedInAccount("signingIn", device), attempt: "running", signInLink });
  const waits = signInParam === "waits" || signInParam === "unopened";
  const signIn = setTimeout(() => {
    if (waits) {
      return;
    }
    accountSignInCancel = null;
    if (end === "denied") {
      resolve(setAccount(deniedAccount()));
      return;
    }
    if (end === "forgeFailed") {
      resolve(setAccount(forgeFailedAccount(device)));
      return;
    }
    setAccount({ ...signedInAccount("syncing", device), attempt: "running" });
    setTimeout(() => {
      accountLastAttemptMs = Date.now();
      resolve(setAccount({ ...signedInAccount("ready", device), attempt: "done" }));
    }, 1000);
  }, 2500);
  accountSignInCancel = () => {
    clearTimeout(signIn);
    accountSignInCancel = null;
    resolve(setAccount({ ...(before ?? signedInAccount("signedOut", device)), attempt: "closed" }));
  };
  return promise;
}

/**
 * A QR-shaped picture of the link — NOT a scannable code; Rust's `qr_svg`
 * draws the real one. Deterministic per link so the card does not flicker.
 */
function mockQrSvg(link: string): string {
  const size = 25;
  let seed = [...link].reduce((hash, char) => (hash * 31 + char.charCodeAt(0)) >>> 0, 7);
  const cells: string[] = [];
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      seed = (seed * 1_103_515_245 + 12_345) >>> 0;
      const finder = (x < 7 || x >= size - 7) && y < 7 ? true : x < 7 && y >= size - 7;
      const on = finder
        ? x % 6 === 0 || y % 6 === 0 || (x % 6 >= 2 && x % 6 <= 4 && y % 6 >= 2 && y % 6 <= 4)
        : (seed & 1) === 1;
      if (on) {
        cells.push(`<rect x="${x + 4}" y="${y + 4}" width="1" height="1"/>`);
      }
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size + 8} ${size + 8}" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="#fff"/><g fill="#000">${cells.join("")}</g></svg>`;
}

// ---------------------------------------------------------------------------
// Repository sources (Epic 86, UX-DR120). Every state of the Browse sheet is
// reachable from the URL:
//   - `?account=ready` — two sources: the account's Forgejo (you + the `keeper`
//     org) and GitHub through the broker (you + two orgs, a restricted-org
//     notice, one owner the app is not installed on). Other `account=` values
//     put the account's forge in `needsSignIn` / `unreachable`.
//   - no account — GitHub alone, by device flow, `notConnected`: Connect shows
//     a code, and the wait resolves connected after a few seconds (or on
//     Cancel, `notConnected`).
//   - `?github=notConnected | unreachable | empty` — GitHub in that state
//     (`empty`: connected with nothing to list); `?forges=none` — no source,
//     so every entry point is absent.
//   - `?github=noGrants` — the broker answers no grants and a device-flow
//     client id exists, so GitHub is `notConnected` through the broker and
//     still offers Connect GitHub (`canConnect`, surface #2).
//   - `?github=fails` — GitHub's first listing fails ("Can't reach …"), and
//     the shell REMEMBERS it: `forges_list` answers GitHub `unreachable` until
//     a `forge_repos(…, refresh: true)` succeeds, which is what Try again has
//     to send (surface #1). A listing that re-read only the sources would stay
//     stuck here, as it would in the app.
//   - some repositories are read-only (`r` below), archived or mirrors: their
//     rows say keeper only downloads them, and the batch adds them pull-only.
//   - the batch step: a drive named `playground` comes back "holds other
//     files", the rest are added.
// ---------------------------------------------------------------------------

const forgesParam = new URLSearchParams(window.location.search).get("forges");
const githubParam = new URLSearchParams(window.location.search).get("github");
const BROKER_HOST = "electra.siren-alsephina.ts.net:8455";
/** The device-flow client id the mock's GitHub source can connect with. */
const MOCK_GITHUB_CLIENT_ID = "Iv1.0000mockclient";
/** GitHub through the broker while an account is set up; device flow without one. */
let githubVia: ForgeSourceVm["via"] =
  accountVm.configured && githubParam !== "notConnected" ? "broker" : "deviceFlow";
let githubConnected =
  githubParam === "connected" ||
  githubParam === "empty" ||
  githubParam === "fails" ||
  (githubVia === "broker" && githubParam !== "unreachable" && githubParam !== "noGrants");
/** The open device-flow wait, so Cancel can end it as the shell's flag does. */
let githubConnectSettle: ((connected: boolean) => void) | null = null;
/** `?github=fails`: the next GitHub listing fails once. */
let githubFailsNext = githubParam === "fails";
/**
 * Why a source's last listing failed, as the shell's `LAST_ERROR` holds it:
 * `forges_list` answers the source from this until a listing replaces it or
 * the account changes.
 */
const forgeErrors = new Map<string, Pick<ForgeSourceVm, "state" | "sentence">>();
/** Settings › Sync's "New drives go in", as the person chose it; `null` is keeper's default. */
let chosenDriveFolder: string | null = null;
const MOCK_HOME = "/Users/tgorka";

function mockDriveFolder(): DriveFolderVm | null {
  if (MOCK_REDUCED) {
    return null;
  }
  return chosenDriveFolder === null
    ? { path: `${MOCK_HOME}/keeper/git`, chosen: false }
    : { path: chosenDriveFolder, chosen: true };
}

function withStoredError(source: ForgeSourceVm): ForgeSourceVm {
  const stored = forgeErrors.get(source.id);
  return stored === undefined ? source : { ...source, ...stored };
}

function githubSource(): ForgeSourceVm {
  const base = {
    id: "github",
    kind: "github" as const,
    name: "GitHub",
    host: "github.com",
    via: githubVia,
    credential: "forge:github",
    // The built-in client id exists in the mock whatever `via` is, so the
    // broker's no-grants answer still offers Connect GitHub.
    canConnect: true,
    appsUrl: `https://github.com/settings/connections/applications/${MOCK_GITHUB_CLIENT_ID}`,
  };
  if (githubParam === "unreachable") {
    return {
      ...base,
      canConnect: false,
      appsUrl: null,
      state: "unreachable",
      login: null,
      sentence: `Your account has no GitHub access on ${BROKER_HOST}. Ask its administrator to add you.`,
    };
  }
  return githubConnected
    ? { ...base, state: "connected", login: "tgorka", sentence: null }
    : { ...base, state: "notConnected", login: null, sentence: null };
}

function accountForgeSource(): ForgeSourceVm {
  const base = {
    id: "account-forge",
    kind: "forgejo" as const,
    name: accountVm.name ?? "Forgejo",
    host: accountVm.repoHost ?? "git.acme.dev",
    via: "accountForge" as const,
    login: accountVm.identity?.login ?? null,
    credential: "account",
    canConnect: false,
    appsUrl: null,
  };
  if (accountVm.state === "ready" || accountVm.state === "syncing") {
    return { ...base, state: "connected", sentence: null };
  }
  if (accountVm.state === "offline") {
    return { ...base, state: "unreachable", sentence: `Can't reach ${base.host}.` };
  }
  return { ...base, state: "needsSignIn", sentence: accountVm.sentence };
}

function forgeSources(): ForgeSourceVm[] {
  if (forgesParam === "none") {
    return [];
  }
  const sources = accountVm.configured ? [accountForgeSource(), githubSource()] : [githubSource()];
  return sources.map(withStoredError);
}

/** `[owner, name, description, minutes since update, size in kB, flags]`; flags p/f/a/t/m/r (r: read-only). */
type RepoSeed = readonly [string, string, string | null, number, number, string?];

const GITHUB_REPOS: readonly RepoSeed[] = [
  ["tgorka", "keeper", "Beeper-style Matrix client that keeps your folders in sync.", 12, 184_320],
  ["tgorka", "dotfiles", "Shell, editor and terminal configuration.", 180, 2_140, "p"],
  ["tgorka", "field-recordings", "Raw audio from walks, sorted by place.", 60 * 26, 9_800_000, "p"],
  ["tgorka", "playground", "Throwaway experiments.", 60 * 24 * 3, 512, "p"],
  ["tgorka", "notes-template", "A starting point for a notes vault.", 60 * 24 * 40, 64, "t"],
  ["tgorka", "matrix-rust-sdk", "Matrix Client-Server SDK for Rust.", 60 * 24 * 9, 96_000, "f"],
  [
    "tgorka",
    "tauri",
    "Build smaller, faster, and more secure desktop applications.",
    60 * 24 * 30,
    210_000,
    "f",
  ],
  ["tgorka", "old-blog", "The blog before the blog.", 60 * 24 * 900, 18_000, "a"],
  ["tgorka", "thesis", "LaTeX sources and figures.", 60 * 24 * 1200, 44_000, "pa"],
  ["tgorka", "photos-2025", null, 60 * 24 * 60, 12_400_000, "p"],
  [
    "tgorka",
    "recipes",
    "Things worth cooking twice, with a very long description that has to truncate at one line because nobody wants a paragraph in a list row.",
    60 * 24 * 5,
    320,
  ],
  ["tgorka", "homelab", "Ansible for the machines under the desk.", 60 * 24 * 2, 1_280, "p"],
  ["tgorka", "cv", null, 60 * 24 * 120, 900, "p"],
  ["tgorka", "gitea-mirror", "Mirror of an old self-hosted forge.", 60 * 24 * 14, 6_000, "m"],
  ["tgorka", "advent-of-code", "Solutions, mostly Rust.", 60 * 24 * 280, 1_700],
  ["tgorka", "keyboard-layouts", "QMK keymaps.", 60 * 24 * 45, 250],
  ["makistack", "infra", "Terraform and the runbooks that go with it.", 45, 5_600, "p"],
  ["makistack", "handbook", "How we work.", 60 * 5, 3_300, "pr"],
  [
    "makistack",
    "github-broker",
    "Short-lived GitHub App tokens for the team's tools.",
    90,
    420,
    "p",
  ],
  [
    "makistack",
    "design-system",
    "Tokens, components and the check that keeps them honest.",
    60 * 24,
    12_000,
  ],
  ["makistack", "api", "The public API.", 60 * 3, 28_000, "pr"],
  ["makistack", "web", "The marketing site.", 60 * 24 * 4, 64_000],
  ["makistack", "mobile", "iOS and Android apps.", 60 * 24 * 6, 140_000, "p"],
  ["makistack", "legacy-admin", "Replaced by api/admin.", 60 * 24 * 400, 20_000, "pa"],
  ["makistack", "project-template", "Scaffolding for a new service.", 60 * 24 * 70, 80, "t"],
  [
    "makistack",
    "openapi-generator",
    "OpenAPI Generator, with our patches.",
    60 * 24 * 20,
    190_000,
    "f",
  ],
  ["makistack", "roadmap", null, 60 * 24 * 2, 40, "p"],
  ["makistack", "brand", "Logos, fonts and the rules for using them.", 60 * 24 * 33, 88_000, "p"],
  ["hesperia-labs", "sensor-firmware", "Firmware for the field sensors.", 60 * 8, 4_200, "p"],
  ["hesperia-labs", "datasets", "Published measurement sets.", 60 * 24 * 11, 2_400_000],
  ["hesperia-labs", "notebooks", "Analysis notebooks.", 60 * 24, 38_000, "p"],
  ["hesperia-labs", "papers", "Drafts and camera-ready versions.", 60 * 24 * 16, 15_000, "p"],
  ["hesperia-labs", "calibration", "Calibration curves per sensor batch.", 60 * 24 * 90, 700, "pa"],
];

const FORGEJO_REPOS: readonly RepoSeed[] = [
  ["tgorka", "notes", "The notes vault.", 20, 48_000, "p"],
  ["tgorka", "journal", "Daily pages.", 60 * 10, 6_100, "p"],
  ["tgorka", "scratch", null, 60 * 24 * 8, 90, "p"],
  ["keeper", "keeper-config", "Shared settings for keeper on every device.", 60 * 2, 120, "p"],
  ["keeper", "policies", "What the admins decided, readable by everyone.", 60 * 24 * 6, 80, "pr"],
  ["keeper", "team-notes", "What the team writes down.", 60 * 24, 21_000, "p"],
  ["keeper", "recordings", "Meeting recordings.", 60 * 24 * 3, 3_600_000, "p"],
  ["keeper", "archive-2024", "Last year's drive.", 60 * 24 * 300, 9_000, "pa"],
];

/** Drives this device has for a repository before the harness added anything. */
const FORGE_ADDED: Record<string, string[]> = {
  "tgorka/dotfiles": ["dotfiles"],
  "makistack/infra": ["infra"],
  "keeper/keeper-config": ["keeper-config"],
};
/** Device slugs from the account's manifest that sync a repository elsewhere. */
const FORGE_ELSEWHERE: Record<string, string[]> = {
  "tgorka/notes": ["iphone-3f2a"],
  "tgorka/field-recordings": ["iphone-3f2a", "ipad-91c0"],
  "makistack/handbook": ["ipad-91c0"],
  "keeper/team-notes": ["ipad-91c0"],
};

/** Rust's one sentence for a repository keeper can only read, in its order. */
function pullOnlySentence(canPush: boolean, archived: boolean, mirror: boolean): string | null {
  if (!canPush) {
    return "You can only read this repository, so keeper only downloads it.";
  }
  if (archived) {
    return "This repository is archived, so keeper only downloads it.";
  }
  if (mirror) {
    return "This repository is a mirror, so keeper only downloads it.";
  }
  return null;
}

function forgeRepoVm(host: string, seed: RepoSeed): ForgeRepoVm {
  const [owner, name, description, minutes, sizeKb, flags = ""] = seed;
  const fullName = `${owner}/${name}`;
  const cloneUrl = `https://${host}/${fullName}.git`;
  // Marked as `forges::mark` does: this device's drives on the same remote.
  const here = (ANSWERS.sync_profiles as SyncProfileVm[])
    .filter((profile) => mockRemoteKey(profile.remoteUrl, "") === mockRemoteKey(cloneUrl, ""))
    .map((profile) => profile.name);
  const archived = flags.includes("a");
  const mirror = flags.includes("m");
  const canPush = !flags.includes("r");
  const sentence = pullOnlySentence(canPush, archived, mirror);
  return {
    fullName,
    owner,
    name,
    description,
    private: flags.includes("p"),
    fork: flags.includes("f"),
    archived,
    template: flags.includes("t"),
    mirror,
    defaultBranch: "main",
    cloneUrl,
    webUrl: `https://${host}/${fullName}`,
    updatedMs: ago(minutes),
    sizeKb,
    canPush,
    addedAs: [...new Set([...(FORGE_ADDED[fullName] ?? []), ...here])],
    elsewhere: FORGE_ELSEWHERE[fullName] ?? [],
    pullOnly: sentence !== null,
    pullOnlySentence: sentence,
  };
}

function forgeReposVm(sourceId: string): ForgeReposVm {
  const github = sourceId === "github";
  const seeds = github ? (githubParam === "empty" ? [] : GITHUB_REPOS) : FORGEJO_REPOS;
  const host = github ? "github.com" : (accountVm.repoHost ?? "git.acme.dev");
  const repos = seeds.map((seed) => forgeRepoVm(host, seed));
  const logins = [...new Set(repos.map((repo) => repo.owner))];
  const owners: ForgeOwnerVm[] = [
    ...logins.filter((login) => login === "tgorka"),
    ...logins.filter((login) => login !== "tgorka").sort(),
  ].map((login) => ({
    login,
    isYou: login === "tgorka",
    count: repos.filter((repo) => repo.owner === login).length,
  }));
  const notices: ForgeNoticeVm[] =
    github && repos.length > 0
      ? [
          {
            sentence:
              "Some organizations haven't approved keeper, so their private repositories are hidden.",
            link: `https://github.com/settings/connections/applications/${MOCK_GITHUB_CLIENT_ID}`,
          },
          ...(githubVia === "broker"
            ? [
                {
                  sentence: "keeper's GitHub app tgdev isn't installed on siren-alsephina.",
                  link: null,
                },
              ]
            : []),
        ]
      : [];
  return { sourceId, repos, owners, notices, truncated: false, fetchedMs: Date.now() };
}

/**
 * A drive the batch add saved, whole — the fields `sync_profile_save` would
 * store for a fresh add with keeper's defaults — so the empty-state entry
 * point (no fixture drive to copy) adds a drive every surface can render.
 * Pull-only when the repository is, as the shell adds it.
 */
function forgeAddedProfile(
  id: string,
  item: ForgeAddReq["repos"][number],
  folder: string,
  repo: ForgeRepoVm,
): SyncProfileVm {
  return {
    id,
    name: item.driveName,
    localPath: folder,
    remoteUrl: repo.cloneUrl,
    branch: repo.defaultBranch,
    direction: repo.pullOnly ? "pullOnly" : "bidirectional",
    lane: "main",
    subpaths: [],
    excludes: [],
    removable: false,
    lfsMode: "materialize",
    lfsThresholdBytes: 4 * 1024 * 1024,
    virtualPatterns: [],
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
    notes: false,
    notesSubfolder: null,
    recordings: false,
    recordingsSubfolder: "recordings",
    voices: false,
    voicesSubfolder: "voices",
    agents: false,
    agentsSubfolder: "80-agents",
    sessions: false,
    sessionsSubfolder: "60-sessions",
    tasks: false,
    tasksSubfolder: "tasks",
    folderOwned: [],
  };
}

/**
/** Answer after `ms`, so a loading state is on screen long enough to look at. */
function later<T>(ms: number, answer: () => T): Promise<T> {
  return new Promise((resolve) => setTimeout(() => resolve(answer()), ms));
}

// ---------------------------------------------------------------------------
// The merged inbox and a chat's timeline, shaped as `keeper-core` streams them
// (`inbox_subscribe`'s six channels, `timeline_subscribe`'s batches). One
// ordinary chat stays in the Inbox window. The agents' old Matrix rooms are in
// no window since 108.2, and the agents themselves live in the Agents window
// (the agents server mocks below).
// ---------------------------------------------------------------------------

const MOCK_ACCOUNT_ID = "01J8ACCOUNTMOCKAAAAAAAAAAA";
const MOCK_NOW = Date.now();

function inboxRow(
  roomId: string,
  displayName: string,
  lastMessage: string,
  minutesAgo: number,
): InboxRoomVm {
  return {
    accountId: MOCK_ACCOUNT_ID,
    hueIndex: 3,
    roomId,
    displayName,
    lastMessage,
    timestamp: MOCK_NOW - minutesAgo * 60_000,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isFavourite: false,
    isPinned: false,
    network: null,
    networkId: null,
    muteState: "none",
  };
}

const CHAT_ROWS: InboxRoomVm[] = [inboxRow("!marta:example.org", "Marta", "See you on Friday", 15)];

const PERSON = "@harness:example.org";

/** A text message. */
function textItem(
  key: string,
  sender: string,
  senderDisplayName: string,
  body: string,
  minutesAgo: number,
): Extract<TimelineItemVm, { kind: "message" }> {
  return {
    kind: "message",
    key,
    sender,
    senderDisplayName,
    body,
    timestamp: MOCK_NOW - minutesAgo * 60_000,
    isOwn: sender === PERSON,
    sendState: null,
    isEdited: false,
    reply: null,
    reactions: [],
    media: null,
    readers: [],
  };
}

const TIMELINE_STREAMS = new Map<number, ReturnType<typeof setInterval>>();
let nextTimelineSubscription = 1;

/**
 * The back-pagination status every first timeline batch carries (102.6):
 * the harness's rooms hold their whole history, so the start is reached.
 * `?history=failed` says Rust's own fill of the window failed, so the
 * boundary shows its sentence and Retry; `?history=more` says older
 * history remains, so a window that cannot scroll offers "Load earlier
 * messages" (R509). Either ask reaches the producer (`load_earlier_messages`),
 * which pages, brings one older message and reaches the start.
 */
const historyParam = new URLSearchParams(window.location.search).get("history");
const mockHistory: PaginationStatusBatch =
  historyParam === "failed"
    ? { state: "failed", hitStart: false }
    : historyParam === "more"
      ? { state: "idle", hitStart: false }
      : { state: "idle", hitStart: true };

/** Every open room's timeline channel, for the producer's answer to an ask. */
const historyChannels = new Map<string, MockChannel<TimelineBatch>>();

function loadEarlierMock(payload: Record<string, unknown>): null {
  const roomId = String(payload.roomId);
  const channel = historyChannels.get(roomId);
  channel?.onmessage?.({ ops: [], pagination: { state: "paginating", hitStart: false } });
  setTimeout(() => {
    channel?.onmessage?.({
      ops: [
        {
          op: "pushFront",
          item: textItem(
            `${roomId}-earlier`,
            "@marta:example.org",
            "Marta",
            "An earlier word, loaded on ask",
            240,
          ),
        },
      ],
      pagination: { state: "idle", hitStart: true },
    });
  }, 400);
  return null;
}

const FIRST_BATCH_MS = Number(new URLSearchParams(window.location.search).get("firstbatch") ?? 0);

/**
 * `?deletions=1`: a plain room ends with this account's deletions in each
 * state its queued redaction can be in — still queued, refused by the
 * server, and done — so the stub's three faces can be looked at. Retry
 * and Cancel are answered like Rust would: Cancel brings the message back,
 * Retry queues the deletion again.
 */
const deletionsParam = new URLSearchParams(window.location.search).get("deletions") !== null;

type RedactedItem = Extract<TimelineItemVm, { kind: "redacted" }>;

function deletionStubs(): RedactedItem[] {
  const stub = (key: string, sendState: RedactedItem["sendState"]): RedactedItem => ({
    kind: "redacted",
    key,
    sender: "@tgorka:example.org",
    senderDisplayName: "Tomasz",
    timestamp: Date.now() - 60_000,
    sendState,
  });
  return [
    stub("deletion-sending", "sending"),
    stub("deletion-failed", "failed"),
    stub("deletion-done", null),
  ];
}

/** Where each room's refused deletion sits in its timeline. */
const failedDeletionIndex = new Map<string, number>();

/** The deletion stubs a room ends with after its `before` items, when asked for. */
function deletionStubsIn(roomId: string, before: number): RedactedItem[] {
  if (!deletionsParam) {
    return [];
  }
  failedDeletionIndex.set(roomId, before + 1);
  return deletionStubs();
}

/** The refused deletion as Retry or Cancel leaves it, in the room it is in. */
function resolveMockDeletion(payload: Record<string, unknown>, retried: boolean): null {
  const key = String(payload.itemKey);
  const roomId = String(payload.roomId);
  const channel = historyChannels.get(roomId);
  const index = failedDeletionIndex.get(roomId);
  if (key !== "deletion-failed" || !channel || index === undefined) {
    return null;
  }
  const item: TimelineItemVm = retried
    ? { ...deletionStubs()[1], sendState: "sending" }
    : textItem(key, "@tgorka:example.org", "Tomasz", "The message I tried to delete", 1);
  channel.onmessage?.({ ops: [{ op: "set", index, item }] });
  return null;
}

function subscribeMockTimeline(payload: Record<string, unknown>): number {
  const id = nextTimelineSubscription++;
  if (FIRST_BATCH_MS > 0) {
    // Held in `TIMELINE_STREAMS`, so an unsubscribe before it lands stops it.
    TIMELINE_STREAMS.set(
      id,
      setTimeout(() => {
        TIMELINE_STREAMS.delete(id);
        deliverMockTimeline(payload);
      }, FIRST_BATCH_MS),
    );
  } else {
    deliverMockTimeline(payload);
  }
  return id;
}

function deliverMockTimeline(payload: Record<string, unknown>): void {
  const channel = payload.channel as MockChannel<TimelineBatch>;
  const roomId = String(payload.roomId);
  historyChannels.set(roomId, channel);
  channel.onmessage?.({
    ops: [
      {
        op: "reset",
        items: [
          textItem(`${roomId}-1`, "@marta:example.org", "Marta", "See you on Friday", 15),
          ...deletionStubsIn(roomId, 1),
        ],
      },
    ],
    pagination: mockHistory,
  });
}

function subscribeMockInbox(payload: Record<string, unknown>): number {
  const reset = (rooms: InboxRoomVm[]): InboxBatch => ({
    ops: [{ op: "reset", rooms }],
    total: rooms.length,
  });
  (payload.channel as MockChannel<InboxBatch>).onmessage?.(reset(CHAT_ROWS));
  for (const window of ["archive", "pins", "favourites"]) {
    (payload[window] as MockChannel<InboxBatch>).onmessage?.(reset([]));
  }
  (payload.spaces as MockChannel<SpacesSnapshot>).onmessage?.({ spaces: [] });
  (payload.networks as MockChannel<NetworksSnapshot>).onmessage?.({ networks: [] });
  return 1;
}

// ---------------------------------------------------------------------------
// 108.2: the agents' old Matrix rooms the account is still in (UX-DR-E108-2)
// and the one-time notice of a cleared *Speak to* choice (UX-DR-E108-1).
// `?oldrooms=N` says the account is still in N old agent rooms (default 3,
// `0` hides the section); `?oldrooms=fails` leaves one the server refuses.
// `?speakto=old` latches the notice. The sentences are Rust's
// (`keeper_core::old_agent_rooms`, `keeper_core::hq::spoken`).
// ---------------------------------------------------------------------------

const oldRoomsParam = new URLSearchParams(window.location.search).get("oldrooms");
let mockOldAgentRooms: OldAgentRoomsVm = {
  remaining:
    oldRoomsParam === null ? 3 : oldRoomsParam === "fails" ? 2 : Number(oldRoomsParam) || 0,
  left: 0,
  failure: null,
};

function mockOldAgentRoomsLeave(): Promise<OldAgentRoomsVm> {
  const failing = oldRoomsParam === "fails" ? 1 : 0;
  mockOldAgentRooms = {
    remaining: failing,
    left: mockOldAgentRooms.remaining - failing,
    failure: failing === 0 ? null : "keeper could not leave one old agent room. Try again later.",
  };
  return later(600, () => mockOldAgentRooms);
}

let mockOldTargetNotice: string | null =
  new URLSearchParams(window.location.search).get("speakto") === "old"
    ? "Speak to named a conversation of your assistant on Matrix, which keeper no longer uses. Choose an agent under Speak to."
    : null;

/** The default agent as Settings shows it (`DefaultAgentVm`), over the agents' choices. */
let mockDefaultAgent: string | null = null;

function mockDefaultAgentVm(): DefaultAgentVm {
  const choices = [
    { agent: "nixie", name: "Nixie" },
    { agent: "tola-grey", name: "Dr Tola Grey" },
  ];
  return {
    defaultAgent: mockDefaultAgent,
    reason: null,
    choices,
    chooser: false,
    fileSet: null,
  };
}

/**
 * `push.gateway_url` (98.1): what the phone's Settings field saved. The real grammar is
 * Rust's (`agents::push::Gateway::parse`); this answers its common shapes — a bare host
 * gains the push path, a value with no `scheme://` is told to start with `https://`,
 * anything else not `https://` is refused — so the field's states can be looked at.
 * `?push=refused` starts with a stored `http://` value keeper will not register, as a
 * synced `settings.toml` could carry it, so the stored-refusal state can be seen too.
 */
const PUSH_NOT_HTTPS = (scheme: string) =>
  `keeper registers a push gateway only over https://, and this address starts with ${scheme}://.`;
let pushGateway: { url: string | null; refusal: string | null } =
  new URLSearchParams(window.location.search).get("push") === "refused"
    ? { url: "http://push.example.org", refusal: PUSH_NOT_HTTPS("http") }
    : { url: null, refusal: null };

function mockPushGatewaySet(payload: Record<string, unknown>) {
  const typed = String(payload.url ?? "").trim();
  const refuse = (message: string) => {
    throw { code: "internal", message, accountId: null, retriable: false };
  };
  if (typed === "") {
    pushGateway = { url: null, refusal: null };
  } else if (!typed.includes("://")) {
    refuse("A push gateway address starts with https://, as in https://push.example.org.");
  } else if (!typed.startsWith("https://")) {
    refuse(PUSH_NOT_HTTPS(typed.split("://")[0]));
  } else {
    pushGateway = {
      url: /^https:\/\/[^/]+\/?$/.test(typed)
        ? `${typed.replace(/\/$/, "")}/_matrix/push/v1/notify`
        : typed,
      refusal: null,
    };
  }
  return pushGateway;
}

/**
 * Rust's standing background failures (Epic 100, R387, `keeper_core::failure::Failures`).
 * `?failure=push` starts with a push registration that failed before the webview
 * subscribed, `?failure=voice` with the wake phrase not re-armed — both are in the
 * snapshot `failure_subscribe` answers first. As in Rust (R410), only the success of
 * the operation that failed clears it: saving a push gateway (iOS re-registers) or
 * confirming the distributor (Android) clears the push registration's; setting or
 * toggling the wake phrase clears the re-arm's.
 */
const failureParam = new URLSearchParams(window.location.search).get("failure") ?? "";
let failureState: FailureStateVm = {
  notices: [
    ...(failureParam.includes("push")
      ? [
          {
            surface: "push" as const,
            operation: "pushRegistration" as const,
            account: "acct-mock",
            sentence:
              "keeper could not turn on push notifications for one of your accounts, so its messages arrive only while keeper is open.",
          },
        ]
      : []),
    ...(failureParam.includes("voice")
      ? [
          {
            surface: "voice" as const,
            operation: "voiceRearm" as const,
            account: null,
            sentence:
              "keeper could not start listening for the wake phrase again. Turn listening off and on to try once more.",
          },
        ]
      : []),
  ],
};
const failureWatchers = new Map<string, MockChannel<FailureStateVm>>();

/** `operation` succeeded: Rust clears its own failures and tells every watcher. */
function failureRecovered(operation: FailureOperation): void {
  const notices = failureState.notices.filter((notice) => notice.operation !== operation);
  if (notices.length === failureState.notices.length) {
    return;
  }
  failureState = { notices };
  for (const watcher of failureWatchers.values()) {
    watcher.onmessage?.(failureState);
  }
}

/** The tablet's distributor was confirmed in Settings this session (98.4). */
let unifiedpushConfirmed = false;

// ---------------------------------------------------------------------------
// Story 101.3 — undo windows, the mid-text `/`, the decisions panel
// ---------------------------------------------------------------------------

/** The two Undo-Send windows, as Settings last set them (Rust's defaults: 2 s, 0 s). */
const mockUndoWindows = { chat: 2, agent: 0 };

/**
 * The dev shell's answer to `chat_inline_preview` (Story 101.3, R434). **A fake**:
 * the token rule, the sets, the drive-qualified insert and every sentence are
 * `keeper_core::chat::inline`'s; a chat offers `/shrug` alone (the agent
 * rooms' mentions and files left with the agents' Matrix path, 108.2).
 */
function mockChatInlinePreview(payload: Record<string, unknown>): ChatInlinePreviewVm {
  const before = String(payload.before ?? "");
  const after = String(payload.after ?? "");
  const match = /\s(\/[^\s/]*)$/.exec(before);
  const token = match !== null && (after === "" || /^\s/.test(after)) ? match[1] : null;
  if (token === null) {
    return { before, after, token: null, rows: [], note: null };
  }
  const query = token.slice(1).toLowerCase();
  const rows: ChatInlineRowVm[] = [];
  if ("shrug".startsWith(query)) {
    rows.push({
      kind: "shrug",
      key: "shrug",
      label: "/shrug",
      detail: "Insert ¯\\_(ツ)_/¯ here.",
      insert: "¯\\_(ツ)_/¯",
    });
  }
  return { before, after, token, rows, note: null };
}

/** The held sends, as Rust's outbox streams them; chats send at once in the fake. */
const mockHeld: HeldSendVm[] = [];
const outboxWatchers = new Map<
  number,
  { roomId: string; channel: MockChannel<{ rows: HeldSendVm[] }> }
>();
let mockWatchSeq = 0;

function emitHeld(): void {
  for (const { roomId, channel } of outboxWatchers.values()) {
    channel.onmessage?.({ rows: mockHeld.filter((row) => row.roomId === roomId) });
  }
}

function mockSendText(payload: Record<string, unknown>): null {
  recordSend("text", payload);
  emitHeld();
  return null;
}

/**
 * `?undo=slow-file` (Story 101.3, R447): the cancel answers after 2.5 s and hands
 * back a staged file with the line, as Rust does for a held attachment — time to
 * open another room before it lands, to see it land only where it was held.
 */
const undoParam = new URLSearchParams(window.location.search).get("undo");

function mockCancelHeld(payload: Record<string, unknown>): HeldRestoreVm | Promise<HeldRestoreVm> {
  const at = mockHeld.findIndex((row) => row.id === String(payload.id));
  if (at === -1) {
    return { line: "", reply: null, attachment: null };
  }
  const [row] = mockHeld.splice(at, 1);
  emitHeld();
  if (undoParam === "slow-file") {
    return later(2500, () => ({
      line: row.body,
      reply: null,
      attachment: { path: "/mock/outbox-files/held/trip-plan.pdf", filename: "trip-plan.pdf" },
    }));
  }
  return { line: row.body, reply: null, attachment: null };
}

/**
 * The agents server (Epic 106, `keeper_core::hq`). Every sentence is copied from `hq::vm`;
 * the address grammar is Rust's (`hq::server::ServerUrl::parse`) and this answers its common
 * shapes. Names are invented: databases `alpha` and `beta`, the person's devices "keeper on
 * laptop" and "keeper on phone". `?hq=` picks the state Settings › Agents › Agents server
 * opens on: absent — no server; `connected` — paired, both databases connected; `notopen` —
 * one database no longer open to this device; `removed` — this device was removed;
 * `notpaired` — an address and no pairing; `offer` — a scanned pairing QR waits for the
 * person to confirm it. Sign-in enrolment (109.6): `signedout` — the server trusts the
 * account and nobody is signed in; `enrolling` — signing this device in, which lands signed in
 * after two seconds; `enrolled` — signed in, with other devices' "new device" notices;
 * `signinrefused` — the server trusts another sign-in, so a code is the way in.
 */
const hqParam = new URLSearchParams(window.location.search).get("hq");
const HQ_SERVER = "https://agents.example.org";
const HQ_HOST = "agents.example.org";
const hqDb = (
  name: string,
  state: HqStatusVm["databases"][number]["state"],
): HqStatusVm["databases"][number] => ({
  name,
  state,
  waiting: 0,
  sentence: {
    connecting: "Connecting…",
    connected: "Connected.",
    offline: "Can't reach the server; keeper keeps trying.",
    notOpen: "Not open to this device any more; what it holds stays readable here.",
    removed: "This device was removed; what it holds stays readable here.",
    noServer: "No server is set; what you write here waits on this device.",
  }[state],
});
const HQ_DEVICE = { clientId: "ana.0a1b2c3d", name: "keeper on laptop" };
let hqDevices: HqStatusVm["devices"] = [
  { ...HQ_DEVICE, addedAtMs: Date.UTC(2026, 9, 20), thisDevice: true, revoked: false },
  {
    clientId: "ana.9f8e7d6c",
    name: "keeper on phone",
    addedAtMs: Date.UTC(2026, 9, 20),
    thisDevice: false,
    revoked: false,
  },
];
const HQ_NOT_PAIRED = `This device is not paired with ${HQ_HOST}. Type the code a paired device shows under Add a device.`;
const HQ_NOT_CONNECTED = `This device is not connected to ${HQ_HOST} yet.`;
const HQ_SIGN_IN_UNAVAILABLE: HqStatusVm["signIn"] = { state: "unavailable", sentence: null };
const HQ_ENROLLED: HqStatusVm["signIn"] = {
  state: "enrolled",
  sentence: `Signed in: ${HQ_HOST} knows this device by your account.`,
};
const hqNone: HqStatusVm = {
  server: null,
  phase: "noServer",
  sentence:
    "No server is set. What you write to your agents waits on this device. 2 changes wait on this device.",
  device: null,
  databases: [],
  waitingHere: 2,
  devices: [],
  offer: null,
  signIn: HQ_SIGN_IN_UNAVAILABLE,
};
function hqPairedStatus(
  beta: HqStatusVm["databases"][number]["state"],
  signIn: HqStatusVm["signIn"] = HQ_SIGN_IN_UNAVAILABLE,
): HqStatusVm {
  return {
    server: HQ_SERVER,
    phase: "paired",
    sentence: `Paired with ${HQ_HOST}.`,
    device: HQ_DEVICE,
    databases: [hqDb("alpha", "connected"), hqDb("beta", beta)],
    waitingHere: 0,
    devices: hqDevices,
    offer: null,
    signIn,
  };
}
function hqNotConnected(signIn: HqStatusVm["signIn"]): HqStatusVm {
  return {
    ...hqNone,
    server: HQ_SERVER,
    phase: "notPaired",
    sentence: HQ_NOT_CONNECTED,
    waitingHere: 0,
    signIn,
  };
}
/** Another device's notices (`hq_enrolments`), newest first; this device's own are not listed. */
const HQ_ENROLMENTS = [
  {
    clientId: "ana.5e6f7a8b",
    sentence: "New device signed in: keeper on kalypso.",
    atMs: Date.UTC(2026, 9, 25, 9, 12),
  },
  {
    clientId: "ana.9f8e7d6c",
    sentence: "keeper on hesperia signed in and kept its place.",
    atMs: Date.UTC(2026, 9, 24, 18, 3),
  },
];
function hqFixture(): HqStatusVm {
  switch (hqParam) {
    case "connected":
      return hqPairedStatus("connected");
    case "notopen":
      return hqPairedStatus("notOpen");
    case "removed":
      return {
        server: HQ_SERVER,
        phase: "removed",
        sentence: `This device was removed from ${HQ_HOST}. It no longer syncs, and what it holds stays readable here until you remove it.`,
        device: HQ_DEVICE,
        databases: [hqDb("alpha", "removed"), hqDb("beta", "removed")],
        waitingHere: 0,
        devices: [],
        offer: null,
        signIn: HQ_SIGN_IN_UNAVAILABLE,
      };
    case "notpaired":
      return {
        ...hqNone,
        server: HQ_SERVER,
        phase: "notPaired",
        sentence: HQ_NOT_PAIRED,
        waitingHere: 0,
      };
    case "offer":
      return { ...hqNone, offer: HQ_HOST };
    case "signedout":
      return hqNotConnected({
        state: "signedOut",
        sentence: `Sign in to your account under Account, and keeper connects this device to ${HQ_HOST}. There is no code to type.`,
      });
    case "enrolling":
      // Lands signed in two seconds after Settings first subscribes (`hq_status_subscribe`).
      return hqNotConnected({
        state: "enrolling",
        sentence: `Signing this device in to ${HQ_HOST}…`,
      });
    case "enrolled":
      return hqPairedStatus("connected", HQ_ENROLLED);
    case "signinrefused":
      return {
        ...hqNotConnected({
          state: "refused",
          sentence: `${HQ_HOST} trusts a different sign-in than your account's, so keeper sent it nothing. Pair this device with a code.`,
        }),
        sentence: HQ_NOT_PAIRED,
      };
    // ?hq=cleared — paired once, then the address cleared (A106-01): the last server's
    // databases stay listed and written to, and what is written waits in each one.
    case "cleared":
      return hqCleared({ alpha: 0, beta: 0 });
    default:
      return hqNone;
  }
}
function hqCleared(waiting: Record<string, number>): HqStatusVm {
  const all = Object.values(waiting).reduce((sum, n) => sum + n, 0);
  const clause = (n: number) =>
    n === 0
      ? ""
      : n === 1
        ? " 1 change waits on this device."
        : ` ${n} changes wait on this device.`;
  return {
    ...hqNone,
    sentence: `No server is set. What you write to your agents waits on this device.${clause(all)}`,
    databases: Object.entries(waiting).map(([name, n]) => {
      const db = hqDb(name, "noServer");
      return { ...db, waiting: n, sentence: `${db.sentence}${clause(n)}` };
    }),
    waitingHere: all,
  };
}
/** One more write waits in `database` while no address is set (?hq=cleared). */
function hqClearedWrite(database: string): void {
  if (hqParam !== "cleared") return;
  const waiting = Object.fromEntries(hqStatus.databases.map((db) => [db.name, db.waiting]));
  waiting[database] = (waiting[database] ?? 0) + 1;
  hqPublish(hqCleared(waiting));
}
let hqStatus: HqStatusVm = hqFixture();
let hqEnrollStarted = false;
const hqWatchers = new Map<string, MockChannel<HqStatusVm>>();
function hqPublish(next: HqStatusVm): HqStatusVm {
  hqStatus = next;
  for (const channel of hqWatchers.values()) channel.onmessage?.(hqStatus);
  return hqStatus;
}
function hqRefuse(message: string): never {
  throw { code: "internal", message, accountId: null, retriable: false };
}
const sessionScenario = new URLSearchParams(location.search).get("sessions");
// ?sessions=names&rename=refused: IPC succeeds locally, then a server
// refusal arrives; the next rename is accepted after its pending state.
const renameScenario = new URLSearchParams(location.search).get("rename");
const renameAttempts = new Map<string, number>();
// ?sessions=launch&live=stream|error|stopped|nonreader|no-tools|untrusted|long|refused|pinned
const liveScenario = new URLSearchParams(location.search).get("live");
/**
 * The answer preferences survive a reload in `sessionStorage`, as the capture
 * sources do, read on first use rather than at import.
 */
const AGENT_VIEW_PREFS_KEY = "keeper.mock-shell.agentViewPrefs";
let agentViewPrefs: { showReasoning: boolean; showDetails: boolean } | null = null;
function viewPrefs(): { showReasoning: boolean; showDetails: boolean } {
  if (agentViewPrefs) return agentViewPrefs;
  const shipped = { showReasoning: false, showDetails: false };
  let read = shipped;
  try {
    const stored = sessionStorage.getItem(AGENT_VIEW_PREFS_KEY);
    if (stored !== null) read = { ...shipped, ...JSON.parse(stored) };
  } catch {
    // Unreadable: the shipped defaults, as a fresh profile would have.
  }
  agentViewPrefs = read;
  return read;
}
/**
 * The preferences as they hold: what was chosen, with `live=pinned` standing in
 * for a settings file that keeps Show details off whatever is chosen.
 */
function heldViewPrefs(): { showReasoning: boolean; showDetails: boolean } {
  return liveScenario === "pinned" ? { ...viewPrefs(), showDetails: false } : { ...viewPrefs() };
}
const sessionChoices: AgentChoiceVm[] = [
  {
    agentActor: "assistant",
    agentName: "Assistant",
    agentMark: "A",
    databases: ["alpha", "beta"],
    database: null,
    ask: true,
  },
  {
    agentActor: "researcher",
    agentName: "Researcher",
    agentMark: "R",
    databases: ["beta"],
    database: "beta",
    ask: false,
  },
];
let sessionSerial = 0;
const sessionRows: AgentSessionRowVm[] =
  sessionScenario === "empty"
    ? []
    : [
        {
          database: "alpha",
          sessionId: "device.1",
          titleWrite: { state: "confirmed" },
          displayName:
            sessionScenario === "names"
              ? "nixi · plan"
              : sessionScenario === "long"
                ? `assistant · Release-${"long-title-".repeat(30)}`
                : "assistant · Plan the next release",
          handle: "jolly-pigeon-squeaks",
          createdAtMs: 1700000000000,
          trust: "native",
          instances: [sessionScenario === "names" ? "nixi@electra" : "assistant@electra"],
          title:
            sessionScenario === "names"
              ? "plan"
              : sessionScenario === "long"
                ? `Release-${"long-title-".repeat(30)}`
                : "Plan the next release",
          agentActor: sessionScenario === "names" ? "nixi" : "assistant",
          agentName: sessionScenario === "names" ? "Nixi" : "Assistant",
          agentMark: sessionScenario === "names" ? "N" : "A",
          archived: false,
          lastActivityMs: Date.now(),
          attention: "needsYou",
        },
        {
          database: "beta",
          sessionId: "device.1",
          titleWrite: { state: "confirmed" },
          displayName: "assistant · Review the research notes",
          handle: "jolly-pigeon-squeaks",
          createdAtMs: 1700000000000,
          trust: "native",
          instances: ["assistant@electra"],
          title: "Review the research notes",
          agentActor: "assistant",
          agentName: "Assistant",
          agentMark: "A",
          archived: false,
          lastActivityMs: Date.now() - 3600000,
          attention: "needsYou",
        },
        {
          database: "alpha",
          sessionId: "device.2",
          titleWrite: { state: "confirmed" },
          displayName: "assistant · Archive — one thousand messages",
          handle: "calm-weasel-scribbles",
          createdAtMs: 1700000000000,
          trust: "native",
          instances: ["assistant@electra"],
          title: "Archive — one thousand messages",
          agentActor: "assistant",
          agentName: "Assistant",
          agentMark: "A",
          archived: true,
          lastActivityMs: Date.now() - 86400000,
        },
      ];
const sessionApprovals: AgentApprovalVm[] = ["empty", "loading", "error"].includes(
  sessionScenario ?? "",
)
  ? []
  : [
      {
        id: "approval-1",
        database: "alpha",
        sessionId: "device.1",
        toolName: "write_file",
        tier: "T3",
        summary: "Write the release checklist",
        bindingDigest: "sha256:fixture-release",
        action: JSON.stringify(
          {
            path: "release/checklist.md",
            body:
              sessionScenario === "long" ? "long-content-".repeat(80) : "Review acceptance checks",
          },
          null,
          2,
        ),
        expiresAtMs: Date.now() + 3600000,
        createdAtMs: Date.now(),
        state: "pending",
        canDecide: true,
        sentence: "Waiting for you",
      },
    ];
const approvalWatchers = new Map<string, MockChannel<AgentApprovalVm[]>>();
const sessionViews = new Map<string, AgentSessionViewVm>(
  sessionRows.map((row) => [
    JSON.stringify([row.database, row.sessionId]),
    {
      ...row,
      readOnly: row.archived,
      hasEarlier: row.archived,
      loadingEarlier: false,
      complete: true,
      readsTrace: liveScenario !== "nonreader",
      untrusted: liveScenario === "untrusted" || liveScenario === "stream",
      live: [],
      prefs: { showReasoning: false, showDetails: false },
      tools:
        liveScenario === "no-tools"
          ? []
          : [
              { name: "web_search", on: true },
              { name: "read_file", on: false },
            ],
      approvals: row.database === "alpha" && row.sessionId === "device.1" ? sessionApprovals : [],
      messages: [
        {
          id: "message-999",
          role: "person",
          authorActor: "ana",
          authorName: "Ana",
          body:
            row.database === "alpha"
              ? "What should we prepare for the next release?"
              : "Please review the research notes in beta.",
          createdAtMs: row.lastActivityMs - 60000,
          state: "taken",
        },
        {
          id: "message-1000",
          role: "agent",
          authorActor: "assistant",
          authorName: "Assistant",
          body:
            row.database === "alpha"
              ? "Start with the acceptance checks, then review the changes with the team. This is the newest page held on this device."
              : "The notes in beta are ready to review. This session is separate from the one with the same id in alpha.",
          createdAtMs: row.lastActivityMs,
        },
        ...(row.database === "beta"
          ? [
              {
                id: "ask-1",
                role: "agent" as const,
                authorActor: "assistant",
                authorName: "Assistant",
                body: "Which notes should I review first?",
                createdAtMs: row.lastActivityMs + 1,
                ask: { options: ["Research", "Release"], open: true },
              },
            ]
          : []),
      ],
    },
  ]),
);

// Raw answer metadata stays in the mock source, not the renderer's IPC VM.
const contextAnswers: Record<
  string,
  {
    id: string;
    metadata: {
      context: {
        used: number;
        window: number | null;
        parts: { part: string; tokens: number; estimated: boolean }[];
      };
      usage: { prompt_tokens: number; completion_tokens: number };
    };
  }
> = {
  alpha: {
    id: "message-1000",
    metadata: {
      context: {
        used: 48000,
        window: 200000,
        parts: [
          { part: "persona", tokens: 8000, estimated: true },
          { part: "memory", tokens: 2000, estimated: true },
          { part: "tools", tokens: 4000, estimated: true },
          { part: "mcp", tokens: 6000, estimated: true },
          { part: "history", tokens: 18000, estimated: true },
          { part: "attachments", tokens: 5000, estimated: true },
          { part: "tool_results", tokens: 5000, estimated: true },
        ],
      },
      usage: { prompt_tokens: 125000, completion_tokens: 1200 },
    },
  },
  beta: {
    id: "message-1000",
    metadata: {
      context: {
        used: 48000,
        window: null,
        parts: [
          { part: "persona", tokens: 8000, estimated: true },
          { part: "memory", tokens: 2000, estimated: true },
          { part: "tools", tokens: 4000, estimated: true },
          { part: "mcp", tokens: 6000, estimated: true },
          { part: "history", tokens: 18000, estimated: true },
          { part: "attachments", tokens: 5000, estimated: true },
          { part: "tool_results", tokens: 5000, estimated: true },
        ],
      },
      usage: { prompt_tokens: 125000, completion_tokens: 1200 },
    },
  },
};
/** Viewing aid mirroring Rust's context projection; never used by the app. */
function mockContext(vm: AgentSessionViewVm, showDetails: boolean): AgentSessionViewVm["context"] {
  const answer = contextAnswers[vm.database];
  if (!showDetails || !answer || !vm.messages.some((message) => message.id === answer.id)) return;
  const context = answer.metadata.context;
  const known = context.window !== null;
  return {
    used: context.used,
    window: context.window,
    percent: known ? "24%" : null,
    meterPercent: known ? 24 : null,
    ringBackground: known ? "conic-gradient(currentColor 86deg, var(--muted) 0deg)" : null,
    summary: known ? "24% · used ≈ 48000 / 200000 tokens" : "used ≈ 48000 tokens",
    parts: [
      {
        label: "System and persona",
        description: "System instructions, SOUL.md and USER.md.",
        tokens: "≈ 8000 tokens",
        share: "≈ 17% of used context",
      },
      {
        label: "Memory",
        description: "Memory included in this prompt.",
        tokens: "≈ 2000 tokens",
        share: "≈ 4% of used context",
      },
      {
        label: "Built-in tools",
        description: "Built-in tool definitions offered to the model.",
        tokens: "≈ 4000 tokens",
        share: "≈ 8% of used context",
      },
      {
        label: "MCP tools",
        description: "MCP tool definitions offered to the model.",
        tokens: "≈ 6000 tokens",
        share: "≈ 13% of used context",
      },
      {
        label: "History",
        description: "Conversation history and the current question.",
        tokens: "≈ 18000 tokens",
        share: "≈ 38% of used context",
      },
      {
        label: "Attachments",
        description: "Attachments included in this turn.",
        tokens: "≈ 5000 tokens",
        share: "≈ 10% of used context",
      },
      {
        label: "This turn's tool results",
        description: "Tool calls and results included in this turn.",
        tokens: "≈ 5000 tokens",
        share: "≈ 10% of used context",
      },
    ],
  };
}

// ?sessions=launch&attachments=chips — the chip in every state (UX-DR-E107-1), in alpha's
// device.1: a message whose files are uploading, waiting, sent and refused, and a message the
// server refused whole because it refused one file's upload — that file `refused`, the other
// `blocked` by it. `&attachments=offline` keeps a new attachment waiting; `refused` has the server
// refuse the first file's upload, and so the message. The `url` is an object URL here; the
// shell's is `keeper-hq-blob://`.
const attachmentsScenario = new URLSearchParams(location.search).get("attachments");
const ATTACHMENT_MAX_BYTES = 26_214_400;
function mockSizeLabel(bytes: number): string {
  const mb = 1024 * 1024;
  return bytes >= mb
    ? `${(Math.ceil((bytes * 10) / mb) / 10).toFixed(1)} MB`
    : `${Math.ceil(bytes / 1024)} kB`;
}
/** `keeper_core::hq::attachments::blocked_sentence`. */
function mockBlocked(name: string, refused: string): string {
  return `${name} was not sent: the agents server refused ${refused}, attached to the same message. Attach ${name} again.`;
}
/** `refusal_sentence` of `blob.too_large`. */
function mockTooLarge(name: string): string {
  return `${name} is larger than the 25.0 MB the agents server takes from one message, so the server refused it.`;
}
function mockChip(
  id: string,
  name: string,
  mime: string,
  size: number,
  state: AgentAttachmentVm["state"],
  body: BlobPart = name,
): AgentAttachmentVm {
  return {
    id,
    name,
    mime,
    size,
    sizeLabel: mockSizeLabel(size),
    state,
    ...(state === "refused"
      ? {
          sentence: `The agents server does not hold ${name}, so it refused the message that names it. Attach it again.`,
        }
      : {}),
    ...(state === "blocked" ? { sentence: mockBlocked(name, "another file") } : {}),
    url: mime.startsWith("image/")
      ? photoFixture
      : URL.createObjectURL(new Blob([body], { type: mime })),
    ...(/^image\/(png|jpeg|gif|webp)$/.test(mime) ? { thumbnailUrl: photoThumbnail } : {}),
  };
}
if (attachmentsScenario === "chips") {
  const vm = sessionViews.get(JSON.stringify(["alpha", "device.1"]));
  if (vm) {
    const at = vm.messages[vm.messages.length - 1]?.createdAtMs ?? Date.now();
    vm.messages.push(
      {
        id: "message-files",
        role: "person",
        authorActor: "ana",
        authorName: "Ana",
        body: "Here are the plan, the photo, the prices and the notes.",
        createdAtMs: at + 1,
        state: "waiting",
        attachments: [
          mockChip("att-1", "release-plan.pdf", "application/pdf", 2_516_582, "uploading"),
          mockChip("att-2", "whiteboard.png", "image/png", 845_312, "waiting"),
          mockChip("att-3", "prices.csv", "text/csv", 1_204, "sent", "item,price\ntea,3\n"),
          mockChip("att-4", "notes.md", "text/markdown", 18_944, "refused"),
        ],
      },
      {
        id: "message-refused",
        role: "person",
        authorActor: "ana",
        authorName: "Ana",
        body: "",
        createdAtMs: at + 2,
        state: "refused",
        attachments: [
          {
            ...mockChip("att-5", "site-video.mov", "video/quicktime", 27_262_976, "refused"),
            sentence: mockTooLarge("site-video.mov"),
          },
          {
            ...mockChip("att-6", "old-scan.heic", "image/heic", 3_408_000, "blocked"),
            sentence: mockBlocked("old-scan.heic", "site-video.mov"),
          },
        ],
      },
    );
  }
}
/**
 * The files staged under a handle (Rust's `attachments::Staging`), each already the chip its
 * message will show; `agent_attach_commit` makes them one message. The mock receives a raw body
 * without its headers, so a pasted file is named `pasted` here.
 */
const mockStaged = new Map<string, AgentAttachmentVm>();
function mockStage(chip: AgentAttachmentVm): string {
  const handle = `staged-${++sessionSerial}`;
  mockStaged.set(handle, chip);
  return handle;
}
function mockAttach(vm: AgentSessionViewVm, caption: string, chips: AgentAttachmentVm[]) {
  const message: AgentSessionViewVm["messages"][number] = {
    id: `local-${++sessionSerial}`,
    role: "person",
    authorActor: "ana",
    authorName: "Ana",
    body: caption,
    createdAtMs: Date.now(),
    state: "waiting",
    attachments:
      attachmentsScenario === "offline" || sessionScenario === "offline"
        ? chips.map((chip) => ({ ...chip, state: "waiting" }))
        : chips,
  };
  vm.messages.push(message);
  publishSessions();
  if (attachmentsScenario === "offline" || sessionScenario === "offline") return;
  setTimeout(() => {
    if (attachmentsScenario === "refused") {
      message.state = "refused";
      const [first, ...rest] = chips;
      message.attachments = first
        ? [
            { ...first, state: "refused", sentence: mockTooLarge(first.name) },
            ...rest.map((chip) => ({
              ...chip,
              state: "blocked" as const,
              sentence: mockBlocked(chip.name, first.name),
            })),
          ]
        : [];
    } else {
      message.state = "sent";
      message.attachments = chips.map((chip) => ({ ...chip, state: "sent" }));
    }
    publishSessions();
  }, 900);
}

/** The mock's source retains rows; only the reader's selected projection crosses IPC. */
function sessionViewerVm(vm: AgentSessionViewVm): AgentSessionViewVm {
  const prefs = heldViewPrefs();
  return {
    ...vm,
    prefs,
    context: mockContext(vm, prefs.showDetails),
    approvals: vm.readsTrace ? vm.approvals : [],
    tools: vm.tools.map((tool) => ({ ...tool })),
    messages: vm.messages.map(({ details, reasoning, steps, ...message }) => ({
      ...message,
      ...(prefs.showDetails && details ? { details } : {}),
      ...(vm.readsTrace && prefs.showReasoning && reasoning ? { reasoning } : {}),
      ...(vm.readsTrace && steps ? { steps } : {}),
    })),
    live: vm.live.map(({ reasoning, steps, ...turn }) => ({
      ...turn,
      ...(vm.readsTrace && prefs.showReasoning && reasoning ? { reasoning } : {}),
      steps: vm.readsTrace ? steps.map((step) => ({ ...step })) : [],
    })),
  };
}

const startedLiveFixtures = new Set<string>();
function startLiveFixture(vm: AgentSessionViewVm) {
  const key = JSON.stringify([vm.database, vm.sessionId]);
  if (!liveScenario || vm.archived || startedLiveFixtures.has(key)) return;
  startedLiveFixtures.add(key);
  vm.approvals = [];
  const turn: AgentSessionViewVm["live"][number] = {
    turnId: `live-${vm.database}`,
    text: "",
    steps: [],
    stopped: false,
  };
  vm.live = [turn];
  const text =
    "## Release checks\n\nStart with the acceptance checks, then review the changes with Ana and Ben.\n\n" +
    Array.from(
      { length: 16 },
      (_, index) =>
        `${index + 1}. Verify the visible behavior and record the result before publishing.\n`,
    ).join("") +
    "\n```text\nRun the acceptance checks before publishing.\n```\n" +
    (liveScenario === "long" ? `\n${"long-result-".repeat(80)}` : "");
  let tick = 0;
  const timer = setInterval(() => {
    tick += 1;
    turn.text = text.slice(0, Math.ceil((text.length * Math.min(tick, 90)) / 90));
    turn.reasoning = "I will separate the acceptance checks from the release notes.".slice(
      0,
      tick * 3,
    );
    if (tick === 4) {
      turn.tool = "web_search";
      turn.steps = [
        {
          id: "tool-live",
          name: "web_search",
          status: "started",
          statusLabel: "Started",
          startedAtMs: Date.now(),
        },
      ];
    }
    if (tick === 24 && liveScenario !== "stopped") {
      delete turn.tool;
      turn.steps[0] = {
        ...turn.steps[0],
        status: "done",
        statusLabel: "Finished",
        result: "Found the release checklist. No files changed.",
        endedAtMs: Date.now(),
      };
    }
    if (tick === 90) {
      clearInterval(timer);
      if (liveScenario === "stopped") {
        // keeper-hq's `end_cut`: a call still running when its instance
        // stopped ends `interrupted`, and Rust names it.
        delete turn.tool;
        turn.steps[0] = {
          ...turn.steps[0],
          status: "interrupted",
          statusLabel: "Interrupted",
          result: "The agent restarted while this call ran; it may have had its effect.",
          endedAtMs: Date.now(),
        };
        turn.stopped = true;
      } else {
        vm.messages.push({
          id: `final-${vm.database}`,
          role: "agent",
          authorActor: "assistant",
          authorName: "Assistant",
          body: turn.text,
          createdAtMs: Date.now(),
          reasoning: turn.reasoning,
          steps: turn.steps,
          details: {
            model: "example-model",
            promptTokens: 120,
            completionTokens: 310,
            totalTokens: 430,
            ttftMs: 84,
            durationMs: 18000,
            finish: liveScenario === "error" ? "error" : "stop",
            ...(liveScenario === "error" ? { error: "The provider stopped responding." } : {}),
          },
        });
        vm.live = [];
      }
    }
    publishSessions();
  }, 200);
}
const sessionListWatchers = new Map<string, MockChannel<AgentSessionsBatch>>();
const sessionViewWatchers = new Map<
  string,
  { key: string; channel: MockChannel<AgentSessionViewVm> }
>();
function publishSessions() {
  for (const channel of approvalWatchers.values())
    channel.onmessage?.(sessionApprovals.filter((row) => row.canDecide));
  for (const channel of sessionListWatchers.values())
    channel.onmessage?.({ ops: [{ op: "reset", rows: sessionRows }], total: sessionRows.length });
  for (const { key, channel } of sessionViewWatchers.values()) {
    const vm = sessionViews.get(key);
    if (vm) channel.onmessage?.(sessionViewerVm(vm));
  }
}
function mockSession(payload: Record<string, unknown>): AgentSessionViewVm {
  const vm = sessionViews.get(JSON.stringify([payload.database, payload.sessionId]));
  if (!vm) return hqRefuse("This session is not on this device.");
  return vm;
}
const sessionHandlers: Record<string, (payload: Record<string, unknown>) => unknown> = {
  agent_view_prefs_set: (payload) => {
    if (liveScenario === "refused") hqRefuse("keeper could not save your answer preference.");
    const patch = payload.patch as { showReasoning: boolean | null; showDetails: boolean | null };
    const held = viewPrefs();
    agentViewPrefs = {
      showReasoning: patch.showReasoning ?? held.showReasoning,
      showDetails: patch.showDetails ?? held.showDetails,
    };
    try {
      sessionStorage.setItem(AGENT_VIEW_PREFS_KEY, JSON.stringify(agentViewPrefs));
    } catch {
      // A harness that cannot write still answers honestly for this page.
    }
    publishSessions();
    return heldViewPrefs();
  },
  agent_session_tool_set: (payload) => {
    const vm = mockSession(payload);
    const tool = vm.tools.find((tool) => tool.name === payload.name);
    if (!tool || vm.readOnly) hqRefuse("This agent does not offer that tool here.");
    if (liveScenario === "refused") hqRefuse("This device cannot change the session's tools.");
    tool.on = payload.on === true;
    publishSessions();
    return null;
  },
  agent_approvals_subscribe: (payload) => {
    const id = `session-approvals-${++sessionSerial}`;
    const channel = payload.channel as MockChannel<AgentApprovalVm[]>;
    approvalWatchers.set(id, channel);
    channel.onmessage?.(sessionApprovals.filter((row) => row.canDecide));
    return id;
  },
  agent_approvals_unsubscribe: (payload) => {
    approvalWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  agent_session_approval_decide: (payload) => {
    const row = sessionApprovals.find(
      (row) =>
        row.database === payload.database &&
        row.sessionId === payload.sessionId &&
        row.id === payload.approvalId,
    );
    if (!row?.canDecide) return hqRefuse("This approval has already been decided.");
    if (row.bindingDigest !== payload.bindingDigest)
      return hqRefuse("The action changed. Read it again before deciding.");
    if (sessionScenario === "refused")
      return hqRefuse("This T4 action must be approved from another host.");
    delete row.refused;
    row.state = payload.approve ? "approved" : "denied";
    row.canDecide = false;
    row.sentence = payload.approve ? "Approved by Ana" : "Denied by Ana";
    publishSessions();
    if (sessionScenario === "queued") {
      setTimeout(() => {
        row.state = "pending";
        row.canDecide = true;
        row.refused = "This device cannot approve this action.";
        row.sentence = `The server refused your decision: ${row.refused}`;
        publishSessions();
      }, 400);
      return { state: "waiting", sentence: "The decision waits on this device." };
    }
    return { state: "sent", sentence: "Your decision was sent." };
  },
  agents_sessions_subscribe: (payload) => {
    if (sessionScenario === "error") hqRefuse("keeper could not read the sessions on this device.");
    const id = `session-list-${++sessionSerial}`;
    sessionListWatchers.set(id, payload.channel as MockChannel<AgentSessionsBatch>);
    if (sessionScenario !== "loading") publishSessions();
    return id;
  },
  agents_sessions_unsubscribe: (payload) => {
    sessionListWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  agent_session_open: (payload) => {
    if (sessionScenario === "open-error") hqRefuse("This session is not open to this device.");
    if (pushArriving.has(JSON.stringify([payload.database, payload.sessionId]))) {
      throw {
        code: "internal",
        message: `This conversation has not reached this device yet; it opens here when ${String(payload.database)} brings it.`,
        accountId: null,
        retriable: true,
      };
    }
    const vm = mockSession(payload);
    const id = `session-view-${++sessionSerial}`;
    const channel = payload.channel as MockChannel<AgentSessionViewVm>;
    sessionViewWatchers.set(id, {
      key: JSON.stringify([payload.database, payload.sessionId]),
      channel,
    });
    startLiveFixture(vm);
    channel.onmessage?.(sessionViewerVm(vm));
    return id;
  },
  agent_session_close: (payload) => {
    sessionViewWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  agent_session_page_back: (payload) => {
    const vm = mockSession(payload);
    vm.messages = [
      {
        id: "message-998",
        role: "person",
        authorActor: "ben",
        authorName: "Ben",
        body: "An earlier month, loaded only when requested.",
        createdAtMs: Date.now() - 31 * 86400000,
        state: "taken",
      },
      ...vm.messages,
    ];
    vm.hasEarlier = false;
    publishSessions();
    return null;
  },
  // Viewing aid only: production parsing and capability decisions are Rust's.
  // Reuse the existing mock preview engine, not a second mock parser.
  agent_command_preview: (payload) => {
    const vm = mockSession(payload);
    const rows: ChatCommandRowVm[] = [
      ["clean", "Start fresh; keep this conversation.", "none", null],
      ["new", "Start a new conversation.", "optionalRest", "title"],
      ["rename", "Rename this conversation.", "required", "title"],
      ["stop", "Stop the running turn.", "none", null],
      ...(vm.tools.length ? [["tools", "Open tool switches.", "none", null]] : []),
      ["context", "Open context details.", "none", null],
      ["people", "Open this session's people.", "none", null],
      ["archive", "Archive this conversation.", "none", null],
      ["help", "List commands.", "none", null],
    ].map(([name, description, args, argHint]) => ({
      name: name as string,
      description: description as string,
      args: args as ChatCommandRowVm["args"],
      argHint: argHint ?? null,
      aliases: [],
      available: true,
      reason: null,
    }));
    return mockChatCommandPreview(payload, rows);
  },
  agent_command: (payload) => {
    const preview = sessionHandlers.agent_command_preview(payload) as ChatCommandPreviewVm;
    if (preview.verdict.kind !== "command") hqRefuse("Choose a command first.");
    if (preview.verdict.kind !== "command") return null;
    const { name, args } = preview.verdict;
    const vm = mockSession(payload);
    const result = {
      session: null as { database: string; sessionId: string } | null,
      surface: null as string | null,
      message: "",
    };
    if (name === "clean" || name === "new") {
      result.session = sessionHandlers.agent_session_new({
        database: vm.database,
        agentActor: vm.agentActor,
        body: "",
      }) as { database: string; sessionId: string };
      const fresh = mockSession(result.session);
      fresh.messages = [];
      fresh.title = args ?? "New conversation";
      const row = sessionRows.find(
        (row) => row.database === fresh.database && row.sessionId === fresh.sessionId,
      );
      if (row) row.title = fresh.title;
      publishSessions();
    } else if (name === "rename") {
      vm.title = args ?? vm.title;
      const row = sessionRows.find(
        (row) => row.database === vm.database && row.sessionId === vm.sessionId,
      );
      if (row) row.title = vm.title;
      publishSessions();
    } else if (name === "archive") {
      sessionHandlers.agent_session_archive(payload);
    } else if (name === "stop") {
      if (!vm.live.length) hqRefuse("There is no running turn to stop.");
      vm.live = [];
      result.message = "Stop requested.";
      publishSessions();
    } else result.surface = name;
    return result;
  },
  agent_send: (payload) => {
    if (sessionScenario === "send-error") hqRefuse("This device cannot write to this database.");
    const vm = mockSession(payload);
    for (const message of vm.messages) if (message.ask) message.ask.open = false;
    const message = {
      id: `local-${++sessionSerial}`,
      role: "person" as const,
      authorActor: "ana",
      authorName: "Ana",
      body: String(payload.body),
      createdAtMs: Date.now(),
      state: "waiting" as "waiting" | "sent" | "taken",
    };
    vm.messages.push(message);
    publishSessions();
    hqClearedWrite(String(payload.database));
    if (sessionScenario !== "offline" && hqParam !== "cleared")
      setTimeout(() => {
        message.state = "sent";
        publishSessions();
      }, 500);
    return null;
  },
  agent_attach_path: (payload) => {
    mockSession(payload);
    const paths = (payload.paths as string[]) ?? [];
    return paths.map((path, index) =>
      mockStage(
        mockChip(
          `att-${++sessionSerial}-${index}`,
          path.split(/[\\/]/).pop() || "attachment",
          /\.(png|jpe?g|gif|webp)$/i.test(path) ? "image/png" : "application/octet-stream",
          4096,
          "uploading",
        ),
      ),
    );
  },
  agent_attach_bytes: (payload) => {
    const bytes = payload as unknown as ArrayBuffer;
    const size = bytes.byteLength ?? 0;
    if (size > ATTACHMENT_MAX_BYTES)
      hqRefuse(
        `pasted is ${mockSizeLabel(size)} and an agent takes at most 25.0 MB from one message, so it did not get the file.`,
      );
    return mockStage(
      mockChip(`att-${++sessionSerial}`, "pasted", "image/png", size, "uploading", bytes),
    );
  },
  agent_attach_commit: (payload) => {
    if (sessionScenario === "send-error") hqRefuse("This device cannot write to this database.");
    const vm = mockSession(payload);
    const handles = (payload.handles as string[]) ?? [];
    const chips = handles.map((handle) => mockStaged.get(handle));
    for (const handle of handles) mockStaged.delete(handle);
    if (chips.length === 0) hqRefuse("Choose a file to attach.");
    if (chips.some((chip) => chip === undefined))
      hqRefuse("A file of this message is no longer ready to send. Attach it again.");
    mockAttach(
      vm,
      String(payload.caption ?? ""),
      chips.filter((chip): chip is AgentAttachmentVm => chip !== undefined),
    );
    return null;
  },
  agent_attach_release: (payload) => {
    for (const handle of (payload.handles as string[]) ?? []) mockStaged.delete(handle);
    return null;
  },
  agent_session_rename: (payload) => {
    const vm = mockSession(payload);
    const previous = { title: vm.title, displayName: vm.displayName };
    vm.title = String(payload.title);
    vm.displayName = `${vm.agentActor} · ${vm.title.trim() || vm.handle}`;
    const row = sessionRows.find(
      (row) => row.database === vm.database && row.sessionId === vm.sessionId,
    );
    if (row) {
      row.title = vm.title;
      row.displayName = vm.displayName;
      row.titleWrite = { state: renameScenario === "refused" ? "pending" : "confirmed" };
      if (renameScenario === "refused") {
        const key = JSON.stringify([row.database, row.sessionId]);
        const attempt = (renameAttempts.get(key) ?? 0) + 1;
        renameAttempts.set(key, attempt);
        const title = row.title;
        window.setTimeout(() => {
          if (renameAttempts.get(key) !== attempt) return;
          if (attempt === 1) {
            Object.assign(vm, previous);
            Object.assign(row, previous);
            row.titleWrite = {
              state: "refused",
              title,
              sentence: "The server refused this rename: you are no longer a member of this chat.",
            };
          } else {
            row.titleWrite = { state: "confirmed" };
          }
          publishSessions();
        }, 1200);
      }
    }
    publishSessions();
    return null;
  },
  agent_session_archive: (payload) => {
    const vm = mockSession(payload);
    vm.archived = true;
    vm.readOnly = true;
    const row = sessionRows.find(
      (row) => row.database === vm.database && row.sessionId === vm.sessionId,
    );
    if (row) row.archived = true;
    publishSessions();
    return null;
  },
  agent_session_unarchive: (payload) => {
    const vm = mockSession(payload);
    vm.archived = false;
    vm.readOnly = false;
    const row = sessionRows.find(
      (row) => row.database === vm.database && row.sessionId === vm.sessionId,
    );
    if (row) row.archived = false;
    publishSessions();
    return null;
  },
  agents_choices: () =>
    sessionScenario === "choices-error"
      ? hqRefuse("The database directory is unavailable.")
      : sessionChoices,
  agent_session_default: () =>
    sessionScenario === "launch"
      ? {
          agentActor: "assistant",
          agentName: "Assistant",
          database: "alpha",
          sessionId: "device.1",
        }
      : null,
  agent_session_new: (payload) => {
    if (sessionScenario === "create-error") hqRefuse("This agent no longer serves beta.");
    const choice = sessionChoices.find(
      (choice) =>
        choice.agentActor === payload.agentActor &&
        choice.databases.includes(String(payload.database)),
    );
    if (!choice) return hqRefuse("That agent does not serve this database.");
    const row: AgentSessionRowVm = {
      database: String(payload.database),
      sessionId: `device.${++sessionSerial + 10}`,
      title: String(payload.body).split("\n")[0],
      titleWrite: { state: "confirmed" },
      displayName: `${choice.agentActor} · ${String(payload.body).split("\n")[0]}`,
      handle: "sturdy-snail-wobbles",
      createdAtMs: Date.now(),
      trust: "native",
      instances: [],
      agentActor: choice.agentActor,
      agentName: choice.agentName,
      agentMark: choice.agentMark,
      archived: false,
      lastActivityMs: Date.now(),
    };
    sessionRows.unshift(row);
    sessionViews.set(JSON.stringify([row.database, row.sessionId]), {
      ...row,
      readOnly: false,
      complete: true,
      hasEarlier: false,
      loadingEarlier: false,
      readsTrace: true,
      untrusted: false,
      live: [],
      tools: [],
      prefs: { showReasoning: false, showDetails: false },
      approvals: [],
      messages: [
        {
          id: `local-${sessionSerial}`,
          role: "person",
          authorActor: "ana",
          authorName: "Ana",
          body: String(payload.body),
          createdAtMs: Date.now(),
          state: "waiting",
        },
      ],
    });
    publishSessions();
    hqClearedWrite(row.database);
    return { database: row.database, sessionId: row.sessionId };
  },
};

// ?sessions=launch&people=member|refused|offline|long — a session's People sheet.
const peopleScenario = new URLSearchParams(location.search).get("people");
const peopleName = (name: string) =>
  peopleScenario === "long" ? `${name} ${"Longname-".repeat(12)}` : name;
type MockPerson = { actorId: string; name: string; dir: string[] };
const peopleDirectory: MockPerson[] = [
  { actorId: "ana", name: "Ana", dir: ["alpha", "beta"] },
  { actorId: "ben", name: peopleName("Ben"), dir: ["alpha"] },
  { actorId: "cal", name: peopleName("Cal"), dir: ["alpha"] },
  // Marta belongs to beta alone: an alpha session never offers her.
  { actorId: "marta", name: peopleName("Marta"), dir: ["beta"] },
];
const sessionMembers = new Map<string, AgentSessionMembersVm>(
  sessionRows.map((row) => {
    const owner = peopleScenario !== "member";
    const ana = {
      actorId: "ana",
      name: "Ana",
      mark: "A",
      role: owner ? ("owner" as const) : ("person" as const),
      readsTrace: owner,
      me: true,
      joining: false,
      canSetReader: false,
      canRemove: false,
    };
    const other =
      row.database === "alpha"
        ? { actorId: "ben", name: peopleName("Ben"), mark: "B" }
        : { actorId: "marta", name: peopleName("Marta"), mark: "M" };
    return [
      JSON.stringify([row.database, row.sessionId]),
      {
        database: row.database,
        sessionId: row.sessionId,
        owner,
        canLeave: !owner,
        addable: [],
        members: [
          ana,
          {
            ...other,
            role: owner ? ("person" as const) : ("owner" as const),
            readsTrace: !owner,
            me: false,
            joining: false,
            canSetReader: owner,
            canRemove: owner,
          },
          {
            actorId: row.agentActor,
            name: row.agentName,
            mark: row.agentMark,
            role: "agent" as const,
            readsTrace: true,
            me: false,
            joining: false,
            canSetReader: false,
            canRemove: false,
          },
        ],
      },
    ];
  }),
);
const peopleWatchers = new Map<
  string,
  { key: string; channel: MockChannel<AgentSessionMembersVm> }
>();
/** Rust's derivation, mirrored for the fixture: who the owner may still add. */
function peopleSheet(sheet: AgentSessionMembersVm): AgentSessionMembersVm {
  const named = new Set(sheet.members.map((m) => m.actorId));
  return {
    ...sheet,
    members: sheet.members.map((m) => ({ ...m })),
    addable: sheet.owner
      ? peopleDirectory
          .filter((p) => p.dir.includes(sheet.database) && !named.has(p.actorId))
          .map((p) => ({ actorId: p.actorId, name: p.name, mark: p.name[0] ?? "" }))
      : [],
  };
}
function publishPeople() {
  for (const { key, channel } of peopleWatchers.values()) {
    const sheet = sessionMembers.get(key);
    if (sheet) channel.onmessage?.(peopleSheet(sheet));
  }
}
function mockPeople(payload: Record<string, unknown>): AgentSessionMembersVm {
  const sheet = sessionMembers.get(JSON.stringify([payload.database, payload.sessionId]));
  if (!sheet) return hqRefuse("This conversation is not on this device.");
  return sheet;
}
function peopleNeedServer(sheet: AgentSessionMembersVm) {
  if (peopleScenario === "offline")
    hqRefuse(
      `This needs the server, and ${sheet.database} is not syncing on this device now. Try again when it is.`,
    );
  if (peopleScenario === "refused")
    hqRefuse("The server refused: only the session's owner changes who is in it.");
}
const peopleHandlers: Record<string, (payload: Record<string, unknown>) => unknown> = {
  agent_session_members_subscribe: (payload) => {
    const sheet = mockPeople(payload);
    const id = `session-people-${++sessionSerial}`;
    const channel = payload.channel as MockChannel<AgentSessionMembersVm>;
    peopleWatchers.set(id, { key: JSON.stringify([payload.database, payload.sessionId]), channel });
    channel.onmessage?.(peopleSheet(sheet));
    return id;
  },
  agent_session_members_unsubscribe: (payload) => {
    peopleWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  agent_session_member_add: (payload) => {
    const sheet = mockPeople(payload);
    const person = peopleSheet(sheet).addable.find((p) => p.actorId === payload.actorId);
    if (!sheet.owner) return hqRefuse("Only the session's owner adds people.");
    if (!person)
      return hqRefuse(
        `${String(payload.actorId)} is not a person of ${sheet.database}'s directory; a session adds only people of its own database.`,
      );
    const joining = {
      ...person,
      role: "person" as const,
      readsTrace: false,
      me: false,
      joining: true,
      canSetReader: false,
      canRemove: false,
    };
    sheet.members.splice(sheet.members.length - 1, 0, joining);
    publishPeople();
    setTimeout(() => {
      if (peopleScenario === "refused") {
        sheet.members = sheet.members.filter((m) => m !== joining);
        sheet.notice = `The server refused adding ${person.actorId}: only the session's owner adds people.`;
      } else {
        Object.assign(joining, { joining: false, canSetReader: true, canRemove: true });
      }
      publishPeople();
    }, 600);
    return null;
  },
  agent_session_member_reader_set: (payload) => {
    const sheet = mockPeople(payload);
    const member = sheet.members.find((m) => m.actorId === payload.actorId && m.canSetReader);
    if (!member) return hqRefuse("Only another person's reading can be changed.");
    peopleNeedServer(sheet);
    member.readsTrace = payload.reads === true;
    publishPeople();
    return null;
  },
  agent_session_member_remove: (payload) => {
    const sheet = mockPeople(payload);
    if (!sheet.members.some((m) => m.actorId === payload.actorId && m.canRemove))
      return hqRefuse("Only another person can be removed.");
    peopleNeedServer(sheet);
    sheet.members = sheet.members.filter((m) => m.actorId !== payload.actorId);
    publishPeople();
    return null;
  },
  agent_session_leave: (payload) => {
    const sheet = mockPeople(payload);
    if (!sheet.canLeave)
      return hqRefuse("The owner cannot leave: nobody else could change who is in this session.");
    const key = JSON.stringify([payload.database, payload.sessionId]);
    sessionMembers.delete(key);
    sessionViews.delete(key);
    const at = sessionRows.findIndex(
      (row) => row.database === payload.database && row.sessionId === payload.sessionId,
    );
    if (at >= 0) sessionRows.splice(at, 1);
    publishSessions();
    return null;
  },
};

// ?sessions=launch&instances=two|offline|lapsed|disagree|stale|refused|long — which instance of an
// agent answers (Settings › Agents › Answers from) and the session header's instance line.
// The words mirror Rust's `keeper_core::hq::instances`; the fixture is a viewing aid only.
const instancesScenario = new URLSearchParams(location.search).get("instances");
const instanceLabel = (label: string) =>
  instancesScenario === "long" ? `${label}-${"very-long-host-name-".repeat(6)}` : label;
type MockInstance = AgentInstancesVm["instances"][number];
const mockInstances: MockInstance[] = [
  {
    clientId: "assistant.electra",
    label: instanceLabel("assistant@electra"),
    host: "electra",
    description: "Ana's assistant",
    databases: ["alpha", "beta"],
    presence: instancesScenario === "offline" ? "offline" : "online",
    definition: "3f9c2a71b0de",
  },
  {
    clientId: "assistant.hesperia",
    label: instanceLabel("assistant@hesperia"),
    host: "hesperia",
    description: "Ana's assistant",
    databases: ["alpha", "beta"],
    presence: "online",
    definition: "3f9c2a71b0de",
  },
  ...(instancesScenario === "stale"
    ? [
        {
          clientId: "assistant.laptop",
          label: "assistant@laptop",
          host: "laptop",
          description: "Ana's assistant",
          databases: ["alpha"],
          presence: "offline" as const,
          definition: "a01b44c9e2f7",
        },
      ]
    : []),
];
for (const instance of mockInstances) {
  if (instancesScenario === "stale" && instance.definition !== "3f9c2a71b0de")
    instance.definitionDiffers =
      "Runs a different definition from assistant@electra and assistant@hesperia: one of them runs an older or a newer image.";
}
// The person's choice is kept per agent and per database, the way Rust reads it: each database's
// row is the order less the instances its directory lists (R-HQ-43), a write waits for its server
// until it settles, and `refused` has beta's server refuse it — beta keeps the row it held.
type MockRow = { order: string[]; updatedMs: number };
type MockDatabase = {
  /** The instances its directory lists. */
  members: string[];
  row?: MockRow;
  pending: boolean;
  refused?: string;
};
type MockAgent = {
  name: string;
  mark: string;
  instances: MockInstance[];
  databases: Record<string, MockDatabase>;
};
const mockAssistantIds = mockInstances.map((i) => i.clientId);
const mockInstanceAgents: Record<string, MockAgent> = {
  assistant: {
    name: "Assistant",
    mark: "A",
    instances: mockInstances,
    databases: {
      alpha: {
        members: mockAssistantIds,
        row: { order: mockAssistantIds, updatedMs: 1 },
        pending: false,
      },
      beta: {
        members: mockInstances.filter((i) => i.databases.includes("beta")).map((i) => i.clientId),
        row: {
          order:
            instancesScenario === "disagree"
              ? ["assistant.hesperia", "assistant.electra"]
              : mockInstances.filter((i) => i.databases.includes("beta")).map((i) => i.clientId),
          updatedMs: instancesScenario === "disagree" ? 2 : 1,
        },
        pending: false,
      },
    },
  },
  researcher: {
    name: "Researcher",
    mark: "R",
    instances: [
      {
        clientId: "researcher.electra",
        label: "researcher@electra",
        host: "electra",
        databases: ["beta"],
        presence: "unknown",
        definition: "77aa01c3d9e0",
      },
    ],
    databases: { beta: { members: ["researcher.electra"], pending: false } },
  },
};

/** One order each row is a part of, as Rust merges the rows written together last. */
function mockChosen(agent: MockAgent): string[] {
  const rows = Object.values(agent.databases).flatMap((db) => (db.row ? [db.row] : []));
  const newest = Math.max(...rows.map((row) => row.updatedMs));
  let rest = rows.filter((row) => row.updatedMs === newest).map((row) => [...row.order]);
  const out: string[] = [];
  for (;;) {
    rest = rest.filter((row) => row.length > 0);
    if (rest.length === 0) return out;
    const next = rest
      .map((row) => row[0])
      .find((head) => rest.every((row) => !row.slice(1).includes(head)));
    if (next === undefined) return rows[0]?.order ?? [];
    for (const row of rest) if (row[0] === next) row.shift();
    out.push(next);
  }
}

function mockAgentView(actor: string, agent: MockAgent): AgentInstancesVm {
  const name = (id: string) => agent.instances.find((i) => i.clientId === id)?.label ?? id;
  const asks = (order: string[]) =>
    order.length === 0
      ? "has no choice, so any online instance answers"
      : `asks ${order.map(name).join(", then ")}`;
  const order = mockChosen(agent);
  const entries = Object.entries(agent.databases);
  let differs = false;
  let outside = false;
  const saved: string[] = [];
  const waiting: string[] = [];
  const refusals: string[] = [];
  const byDatabase = entries.map(([database, db]) => {
    const row = db.row?.order ?? [];
    const expected = order.filter((id) => db.members.includes(id));
    if (expected.length > 0 && row.join() !== expected.join()) differs = true;
    let sentence: string;
    if (db.refused !== undefined) {
      refusals.push(`The server refused your choice in ${database}: ${db.refused}.`);
      sentence = `${database} refused your choice: ${db.refused}.`;
    } else if (order.length > 0 && expected.length === 0) {
      outside = true;
      sentence = `None of the instances you chose is in ${database}, so its row is unchanged: it ${asks(row)}.`;
    } else if (db.pending) {
      waiting.push(database);
      sentence = `${database} ${asks(row)}; saved on this device, waiting to sync.`;
    } else {
      if (expected.length > 0 && row.join() === expected.join()) saved.push(database);
      sentence = `${database} ${asks(row)}.`;
    }
    return {
      database,
      order: row,
      sentence,
      pending: db.pending,
      ...(db.refused !== undefined ? { refused: db.refused } : {}),
    };
  });
  const and = (items: string[]) =>
    items.length < 2
      ? items.join("")
      : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
  const ordersDiffer = byDatabase.some((db) => db.order.join() !== byDatabase[0].order.join());
  const worthSaying =
    entries.length > 1 && (ordersDiffer || outside || waiting.length > 0 || refusals.length > 0);
  const ranked = [
    ...order.flatMap((id) => agent.instances.filter((i) => i.clientId === id)),
    ...agent.instances.filter((i) => !order.includes(i.clientId)),
  ];
  return {
    agentActor: actor,
    agentName: agent.name,
    agentMark: agent.mark,
    databases: entries.map(([database]) => database),
    instances: ranked.map((instance) => {
      const rank = order.indexOf(instance.clientId);
      return rank < 0 ? instance : { ...instance, rank };
    }),
    ...(order[0] !== undefined ? { defaultClient: order[0] } : {}),
    fallback: order.slice(1),
    sentence: orderSentence(order, name),
    ...(differs
      ? {
          disagreement: `Your databases differ: ${entries
            .map(([database, db]) => `${database} ${asks(db.row?.order ?? [])}`)
            .join("; ")}. Applying the order shown writes it into each.`,
        }
      : {}),
    ...(waiting.length > 0
      ? {
          delivery:
            saved.length > 0
              ? `Saved in ${and(saved)}; waiting to sync to ${and(waiting)}.`
              : `Saved on this device; waiting to sync to ${and(waiting)}.`,
        }
      : {}),
    ...(refusals.length > 0 ? { notice: refusals.join(" ") } : {}),
    reapply: differs || refusals.length > 0,
    byDatabase: worthSaying ? byDatabase : [],
  };
}
function orderSentence(order: string[], name: (id: string) => string): string {
  const [first, ...rest] = order;
  if (first === undefined)
    return "No choice yet: a new session goes to whichever instance is online.";
  if (rest.length === 0)
    return `New sessions go to ${name(first)}; while it is offline they wait for it.`;
  return `New sessions go to ${name(first)}; while it is offline, to ${rest.map(name).join(", then ")}.`;
}
function mockAgentInstances(): AgentInstancesVm[] {
  return Object.entries(mockInstanceAgents).map(([actor, agent]) => mockAgentView(actor, agent));
}
function mockAnswering(): AgentSessionAnsweringVm {
  const holder = instancesScenario === "offline" ? mockInstances[1] : mockInstances[0];
  const lapsed = instancesScenario === "lapsed";
  const next = mockInstances.find((i) => i !== holder && i.presence === "online");
  return {
    ...(lapsed ? {} : { holder: holder.clientId }),
    sentence: lapsed
      ? `${holder.label} held this conversation; its lease ran out, so ${next?.label ?? "one that is online"} takes it over next.`
      : `${holder.label} is answering.`,
    instances: [
      holder,
      ...mockInstances.filter((i) => i !== holder && i.databases.includes("alpha")),
    ].map((i) => ({
      clientId: i.clientId,
      label: i.label,
      presence: i.presence,
      holds: i === holder && !lapsed,
      lapsed: i === holder && lapsed,
    })),
  };
}
if (instancesScenario) {
  for (const vm of sessionViews.values())
    if (vm.agentActor === "assistant") vm.answering = mockAnswering();
}
const instanceWatchers = new Map<string, MockChannel<AgentInstancesVm[]>>();
function publishInstances() {
  for (const channel of instanceWatchers.values()) channel.onmessage?.(mockAgentInstances());
}
const instanceHandlers: Record<string, (payload: Record<string, unknown>) => unknown> = {
  agent_instances_subscribe: (payload) => {
    const id = `agent-instances-${++sessionSerial}`;
    const channel = payload.channel as MockChannel<AgentInstancesVm[]>;
    instanceWatchers.set(id, channel);
    channel.onmessage?.(instancesScenario ? mockAgentInstances() : []);
    return id;
  },
  agent_instances_unsubscribe: (payload) => {
    instanceWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  agent_instances_set: (payload) => {
    const actor = String(payload.agentActor);
    const order = (payload.order as string[]) ?? [];
    const agent = mockInstanceAgents[actor];
    if (agent === undefined) return hqRefuse(`${actor} is in no database this device holds.`);
    if (order.length === 0) return hqRefuse("Choose the instance that answers first.");
    const stranger = order.find((id) => !agent.instances.some((i) => i.clientId === id));
    if (stranger !== undefined)
      return hqRefuse(`${stranger} is not an instance of ${actor} in your databases.`);
    const updatedMs = Date.now();
    const before = new Map<string, MockRow | undefined>();
    for (const [database, db] of Object.entries(agent.databases)) {
      const part = order.filter((id) => db.members.includes(id));
      if (part.length === 0) continue;
      before.set(database, db.row);
      db.row = { order: part, updatedMs };
      db.pending = true;
      db.refused = undefined;
    }
    publishInstances();
    // Each server answers a moment later: it takes the write, or — `refused` — beta refuses it
    // and keeps the row it held.
    setTimeout(() => {
      for (const [database, db] of Object.entries(agent.databases)) {
        if (!before.has(database) || !db.pending) continue;
        db.pending = false;
        if (instancesScenario === "refused" && database === "beta") {
          db.row = before.get(database);
          db.refused = "hq.v9.person_actor";
        }
      }
      publishInstances();
    }, 700);
    return null;
  },
};

const hqHandlers: Record<string, (payload: Record<string, unknown>) => unknown> = {
  hq_status_subscribe: (payload) => {
    const id = `hq-subscription-${hqWatchers.size + 1}`;
    const channel = payload.channel as MockChannel<HqStatusVm>;
    hqWatchers.set(id, channel);
    channel.onmessage?.(hqStatus);
    if (hqParam === "enrolling" && hqStatus.signIn.state === "enrolling" && !hqEnrollStarted) {
      hqEnrollStarted = true;
      setTimeout(() => hqPublish(hqPairedStatus("connected", HQ_ENROLLED)), 2000);
    }
    return id;
  },
  hq_status_unsubscribe: (payload) => {
    hqWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  hq_server_set: (payload) => {
    const typed = String(payload.url ?? "").trim();
    if (typed === "") return hqPublish({ ...hqNone, waitingHere: 0 });
    if (!typed.includes("://")) {
      hqRefuse(
        "A server address starts with https:// or http://, as in https://agents.example.org.",
      );
    }
    const scheme = typed.split("://")[0] ?? "";
    if (scheme !== "https" && scheme !== "http") {
      hqRefuse(
        `keeper reaches your agents server over https:// or http://, and this address starts with ${scheme}://.`,
      );
    }
    const server = typed.replace(/\/$/, "");
    const host = server.split("://")[1]?.split("/")[0] ?? server;
    return hqPublish({
      ...hqNone,
      server,
      phase: "notPaired",
      sentence: `This device is not paired with ${host}. Type the code a paired device shows under Add a device.`,
      waitingHere: 0,
    });
  },
  hq_server_forget: () =>
    hqPublish({
      ...hqNone,
      server: HQ_SERVER,
      phase: "notPaired",
      sentence: HQ_NOT_PAIRED,
      waitingHere: 0,
    }),
  hq_pair_redeem: (payload) => {
    const code = String(payload.code ?? "").replace(/[\s-]/g, "");
    if (code.length !== 24) {
      hqRefuse(`A pairing code has 24 characters; this has ${code.length}.`);
    }
    if (hqStatus.server === null) {
      hqRefuse("Set your agents server's address first; the code is for that server.");
    }
    return hqPublish(hqPairedStatus("connected"));
  },
  hq_pair_offer_accept: () => hqPublish(hqPairedStatus("connected")),
  hq_pair_offer_dismiss: () => {
    hqPublish({ ...hqStatus, offer: null });
    return null;
  },
  hq_pair_start: (): HqPairCodeVm => ({
    code: "7K2M-Q9XD-4HTR-8BWN-3CVE-6PJA",
    qrSvg: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 29 29"><rect width="29" height="29" fill="#fff"/><path d="M1 1h7v7H1zM21 1h7v7h-7zM1 21h7v7H1zM11 11h7v7h-7z" fill="#000"/></svg>`,
    expiresAtMs: Date.now() + 10 * 60 * 1000,
  }),
  hq_device_revoke: (payload) => {
    hqDevices = hqDevices.map((device) =>
      device.clientId === payload.clientId ? { ...device, revoked: true } : device,
    );
    return hqPublish({ ...hqStatus, devices: hqDevices });
  },
  hq_enrolments: () => {
    if (hqStatus.phase !== "paired") {
      hqRefuse("This device is not paired with an agents server.");
    }
    return hqStatus.signIn.state === "enrolled" ? HQ_ENROLMENTS : [];
  },
};

const HANDLERS: Record<string, (payload: Record<string, unknown>) => unknown> = {
  ...hqHandlers,
  ...sessionHandlers,
  ...peopleHandlers,
  ...instanceHandlers,
  push_gateway_get: () => pushGateway,
  push_open_take: () => {
    const target = pushWaiting;
    pushWaiting = null;
    return target;
  },
  failure_subscribe: (payload) => {
    const id = `failure-subscription-${failureWatchers.size + 1}`;
    const channel = payload.channel as MockChannel<FailureStateVm>;
    failureWatchers.set(id, channel);
    channel.onmessage?.(failureState);
    return id;
  },
  // `?failure=draft`: saving a composer draft fails, deleting one works (R410).
  set_draft: () => {
    if (failureParam.includes("draft")) {
      throw { code: "internal", message: "database is locked", accountId: null, retriable: true };
    }
    return null;
  },
  delete_draft: () => null,
  failure_unsubscribe: (payload) => {
    failureWatchers.delete(String(payload.subscriptionId));
    return null;
  },
  // Android's push status (98.4): the tablet first asks which distributor to use
  // (ntfy, by name and package), and once confirmed answers a registered pusher. Both
  // views are Rust's own `UnifiedPushVm`s, written by the keeper-core test
  // `the_mock_shells_push_status_is_rusts` — never copied here. Every other build: null.
  unifiedpush_status: () =>
    MOCK_PLATFORM === "tablet"
      ? unifiedpushConfirmed
        ? UNIFIEDPUSH_FIXTURE.registered
        : UNIFIEDPUSH_FIXTURE.asking
      : null,
  unifiedpush_confirm: (payload) => {
    if (MOCK_PLATFORM !== "tablet") {
      return null;
    }
    if (!UNIFIEDPUSH_FIXTURE.asking.offers.some((offer) => offer.package === payload.package)) {
      throw {
        code: "internal",
        message: "keeper did not ask about that distributor",
        accountId: null,
        retriable: false,
      };
    }
    unifiedpushConfirmed = true;
    failureRecovered("pushRegistration");
    void emit(UNIFIEDPUSH_STATUS_EVENT);
    return UNIFIEDPUSH_FIXTURE.registered;
  },
  push_gateway_set: (payload) => {
    const saved = mockPushGatewaySet(payload);
    // Rust re-registers the pusher with the saved gateway; it succeeds here.
    failureRecovered("pushRegistration");
    return saved;
  },
  inbox_subscribe: subscribeMockInbox,
  mark_room_read: () => null,
  // Story 100.5: the messenger's slash commands. Sends and room acts succeed
  // silently (the fake has no room state to change) except one refused invite.
  // `?slash=slow` answers every preview after a second, so a second Enter and
  // typing during a slow command can be seen in the browser (R393).
  chat_command_preview: (payload) =>
    new URLSearchParams(window.location.search).get("slash") === "slow"
      ? later(1000, () => mockChatCommandPreview(payload))
      : mockChatCommandPreview(payload),
  send_text: mockSendText,
  subscribe_outbox: (payload) => {
    const id = ++mockWatchSeq;
    const roomId = String(payload.roomId ?? "");
    const channel = payload.channel as MockChannel<{ rows: HeldSendVm[] }>;
    outboxWatchers.set(id, { roomId, channel });
    channel.onmessage?.({ rows: mockHeld.filter((row) => row.roomId === roomId) });
    return id;
  },
  unsubscribe_outbox: (payload) => {
    outboxWatchers.delete(Number(payload.subscriptionId));
    return null;
  },
  cancel_held_send: mockCancelHeld,
  // Story 101.3: the two Undo-Send windows and the mid-text `/`.
  undo_send_window: () => mockUndoWindows.chat,
  set_undo_send_window: (payload) => {
    mockUndoWindows.chat = Number(payload.seconds);
    return null;
  },
  undo_send_agent_window: () => mockUndoWindows.agent,
  set_undo_send_agent_window: (payload) => {
    mockUndoWindows.agent = Number(payload.seconds);
    return null;
  },
  chat_inline_preview: mockChatInlinePreview,
  send_emote: (payload) => recordSend("emote", payload),
  // Recorded too, so the browser can see which context a slow line went out in (R411).
  send_reply: (payload) => recordSend("reply", payload),
  send_reply_to_event: (payload) => recordSend("reply", payload),
  // Recorded with its path and caption, so the browser can see which room's file
  // a Send carried (R447).
  send_attachment_path: (payload) =>
    recordSend("attachment", { body: `${String(payload.path)} ${String(payload.caption ?? "")}` }),
  edit_message: (payload) => recordSend("edit", payload),
  room_set_topic: () => later(200, () => null),
  room_invite: (payload) =>
    String(payload.userId) === "@refused:example.org"
      ? Promise.reject({
          code: "sendFailed",
          message: "could not send the message: M_FORBIDDEN",
          accountId: null,
          retriable: true,
        })
      : later(200, () => null),
  room_leave: (payload) => {
    mockLeaves.push(`${String(payload.accountId)} ${String(payload.roomId)}`);
    return later(300, () => null);
  },
  // The harness's rooms hold their whole history: a page reaches the start.
  paginate_backwards: () => true,
  send_retry: (payload) => resolveMockDeletion(payload, true),
  cancel_send: (payload) => resolveMockDeletion(payload, false),
  load_earlier_messages: loadEarlierMock,
  timeline_subscribe: subscribeMockTimeline,
  timeline_unsubscribe: (payload) => {
    const id = Number(payload.subscriptionId);
    clearInterval(TIMELINE_STREAMS.get(id));
    TIMELINE_STREAMS.delete(id);
    return null;
  },
  ...transcriptionMockHandlers(() => ANSWERS.sync_profiles as SyncProfileVm[]),
  ...mediaBlockMockHandlers(),
  "plugin:dialog|open": (payload) => {
    const options = payload.options;
    return options && typeof options === "object" && "directory" in options && options.directory
      ? "/Users/tgorka/Documents"
      : attachmentsScenario === "chips"
        ? "/fixtures/agent-photo.png"
        : "/Volumes/merope/tgdrive/meeting.mov";
  },
  // tgdrive before its last upload stops (`UPLOAD_STOPS_AT`): the push is held
  // while the upload moves, so the card reads as syncing and offers nothing.
  sync_statuses: () =>
    (ANSWERS.sync_statuses as SyncStatusVm[]).map((status) =>
      status.heldPush && Date.now() < UPLOAD_STOPS_AT
        ? {
            ...status,
            state: "syncing",
            line: `${status.profileName} — uploading 40-media/recordings/2026/2026-10-16 14.07 test/manifest.json`,
            error: null,
            needsAttention: false,
            heldPush: false,
          }
        : status,
    ),
  // "Retry all stopped" (Story 102.2): the stopped rows leave the list and the
  // held push's sentence goes with them, as `Engine::retry_all_parked` does.
  sync_retry_all_parked: (payload) => {
    const problems = ANSWERS.sync_problems as SyncProblemsVm;
    const moved = problems.parked.length;
    problems.parked = [];
    for (const status of ANSWERS.sync_statuses as SyncStatusVm[]) {
      if (status.profileId === payload.id && status.heldPush) {
        Object.assign(status, {
          state: "watching",
          line: `${status.profileName} — remote comparison unknown`,
          error: null,
          needsAttention: false,
          heldPush: false,
        });
      }
    }
    return moved;
  },
  sync_folder_tasks_flag: (payload) => {
    const profile = (ANSWERS.sync_profiles as SyncProfileVm[]).find(
      (p) => p.id === payload.profileId,
    );
    const subfolder = payload.subfolder === null ? null : String(payload.subfolder);
    if (profile) {
      profile.tasks = subfolder !== null;
      profile.tasksSubfolder = subfolder ?? "tasks";
    }
    return {
      chosenProfileId: profile?.id ?? null,
      resolvedProfileId: profile?.id ?? null,
      resolvedProfileName: profile?.name ?? null,
      root: profile && subfolder ? `${profile.localPath}/${subfolder}` : null,
      subfolder: subfolder ?? "tasks",
      subfolderSource: "folder-file",
      writable: true,
      exists: true,
      notice: null,
    };
  },
  // One rail group per drive the search covers: the selection, or the active
  // drive when nothing is selected — `notes_spaces(vault_id, vault_ids)`.
  notes_spaces: (payload) => {
    const selected = (payload.vaultIds as string[] | undefined) ?? [];
    const ids = selected.length > 0 ? selected : [payload.vaultId as string];
    const vaults = (ANSWERS.notes_vaults as { id: string; name: string }[]).filter((vault) =>
      ids.includes(vault.id),
    );
    return {
      rows: vaults.flatMap((vault) => railRows(vault.id, vault.name)),
      vaults: vaults.map((vault) => ({
        vaultId: vault.id,
        vaultName: vault.name,
        available: true,
        reason: "",
      })),
    };
  },
  // Opening a space acknowledges it with its row (and would restart a
  // temporary space's lifetime); the fixtures have none, so the row is as listed.
  notes_space_touch: (payload) => {
    const vault = (ANSWERS.notes_vaults as { id: string; name: string }[]).find(
      (candidate) => candidate.id === payload.vaultId,
    );
    return railRows(vault?.id ?? "v1", vault?.name ?? "tgdrive").find(
      (row) => row.id === payload.spaceId,
    );
  },
  notes_hide_service_files_set: (payload) => {
    ANSWERS.notes_hide_service_files_get = Boolean(payload.hidden);
  },
  notes_service_file_names_set: (payload) => {
    const names = payload.names as string[];
    ANSWERS.notes_service_file_names_get = [
      ...new Set(names.map((name) => name.trim().toLowerCase()).filter(Boolean)),
    ];
  },
  notes_embedding_model_set: (payload) => {
    ANSWERS.notes_embedding_model_get = payload.model;
  },
  notes_subscribe_search: (payload) => {
    (payload.channel as MockChannel<unknown>).onmessage?.({
      vaultId: payload.vaultId,
      phase: "words",
      indexed: noteRows.length,
      total: noteRows.length,
      embedded: 0,
      embeddable: 0,
      model: "",
      sentence: "",
    });
    return "sub-mock-search";
  },
  notes_subscribe_changes: (payload) => {
    (payload.channel as MockChannel<unknown>).onmessage?.({
      vaultId: payload.vaultId,
      ops: [{ op: "reset", rows: noteRows }],
      total: noteRows.length,
      matched: noteRows.length,
      hidden: 0,
    });
    return "sub-mock-changes";
  },
  copy_start: (payload) => {
    const source = String(payload.source ?? "");
    const destination = String(payload.destination ?? "");
    if (source === "" || destination === "") {
      throw {
        code: "internal",
        message: "Choose a source and a destination.",
        accountId: null,
        retriable: false,
      };
    }
    // The mark, not a typed window (AD-256): a run the shell reports covers
    // everything newer than the last successful one's high-water mtime, and the
    // harness answers exactly what the engine answers — a file behind the mark
    // is absent from the report rather than listed as skipped.
    const since = typeof payload.modifiedSinceMs === "number" ? payload.modifiedSinceMs : null;
    const entries = COPY_FIXTURES.filter((file) => since === null || file.mtimeMs > since).map(
      (file) => ({
        path: file.path,
        bytes: file.bytes,
        outcome: "copied",
        reason: null,
      }),
    );
    const id = `mock-copy-${copyJobs.size + 1}`;
    const bytes = entries.reduce((sum, entry) => sum + entry.bytes, 0);
    copyJobs.set(id, {
      id,
      source,
      destination,
      state: "done",
      filesDone: entries.length,
      filesTotal: entries.length,
      bytesDone: bytes,
      bytesTotal: bytes,
      current: null,
      entries,
      error: null,
    });
    return id;
  },
  copy_status: (payload) => {
    const job = copyJobs.get(String(payload.id));
    if (!job)
      throw { code: "not_found", message: "No such copy job.", accountId: null, retriable: false };
    return job;
  },
  auto_update_get: () => autoUpdate,
  // Always a hold, never a verdict to restart: a harness that answered
  // `restart: true` would relaunch the dev page out from under whoever is
  // looking at it. The real answer is Rust's, from the recording state and the
  // local hour, and cannot be looked at here.
  auto_update_restart_check: () =>
    ({ restart: false, hold: "inUse" }) satisfies AutoUpdateRestartVm,
  auto_update_set: (payload) => {
    autoUpdate = { ...autoUpdate, enabled: payload.enabled === true };
    return autoUpdate;
  },
  log_upload_get: () => logUpload,
  log_upload_set: (payload) => {
    logUpload = payload.enabled === true;
    return logUpload;
  },
  telemetry_status: () => telemetryStatus(),
  telemetry_consent_set: (payload) => {
    telemetryConsent = { ...(payload.consent as typeof telemetryConsent) };
    return telemetryStatus();
  },
  telemetry_capture: () => null,
  telemetry_remote_config: () => ({ supportMessage: null }),
  telemetry_study_config: () => null,
  telemetry_study_preview: () => ({ configured: false, host: null }),
  telemetry_study_stop: () => null,
  // The Settings document also mounts these native shortcut rows. Browser
  // preview cannot register OS shortcuts, but must still return their typed VM.
  hotkey_get: (): HotkeyVm => ({
    accelerator: "Control+Alt+Space",
    isDefault: true,
    active: false,
    conflict: null,
  }),
  recording_hotkey_get: (): HotkeyVm => ({
    accelerator: "",
    isDefault: true,
    active: false,
    conflict: null,
  }),
  // Nothing records in the browser. Without an answer the session hook adopted
  // `null` and every surface that reads the snapshot — a note's New recording
  // widget among them — threw on `status.outputPath`.
  recording_status: () => IDLE_RECORDING_STATUS,
  // Granted, so the browser shows the setup a person with permissions sees.
  recording_permission: () => ({
    screenRecording: "granted",
    microphone: "granted",
    camera: "granted",
    canStart: true,
  }),
  voice_hotkey_get: (): HotkeyVm => ({
    accelerator: "",
    isDefault: true,
    active: false,
    conflict: null,
  }),
  providers_list: () => MODEL_PROVIDERS,
  provider_models: (payload) => PROVIDER_MODELS[String(payload.providerId ?? "")] ?? [],
  // The probe answers `online`, because a harness that could not reach its own
  // fixtures would only ever show the failure state.
  provider_probe: () =>
    ({
      reach: "online",
      status: 200,
      version: "0.33.2",
      roundTripMs: 4,
      reason: null,
    }) satisfies ProviderProbeVm,
  // The write verbs answer with a plausible row rather than mutating the table:
  // Settings re-reads after a save, and a harness that half-mutated would show
  // a list the next read contradicts.
  provider_save: () => MODEL_PROVIDERS[0],
  provider_remove: () => null,
  // --- First-run setup (spec *Skipping setup can stick*) ------------------
  //
  // The round trip within one session: the checkbox writes here and the boot
  // read answers it. Persistence across launches is the Rust `settings` table's
  // job and cannot be looked at here.
  first_run_setup_skipped_get: () => firstRunSetupSkipped,
  first_run_setup_skipped_set: (payload) => {
    firstRunSetupSkipped = payload.skipped === true;
    return null;
  },
  // --- Recordings browser -----------------------------------------------------
  //
  // Two sessions: one already transcribed (Show transcript), one with media and
  // no transcript yet (Transcribe, whose job the transcription mock drives).
  search_recordings: () => {
    const rows: RecordingSearchVm["rows"] = [
      {
        sessionId: "01JREC0000000000000000KELLY",
        relativePath: "2026/kelly-sync",
        absolutePath: "/Users/alice/Movies/keeper/2026/kelly-sync",
        title: "Kelly sync",
        startedTs: 1_790_000_000_000,
        endedTs: 1_790_001_800_000,
        durationMs: 1_800_000,
        totalBytes: 812_000_000,
        durability: "pushed",
        tags: ["work/sync"],
        playablePath: "/Users/alice/Movies/keeper/2026/kelly-sync/screen-0001.mov",
        transcript: "/Users/alice/Movies/keeper/2026/kelly-sync/transcript.json",
        transcribable: true,
      },
      {
        sessionId: "01JREC00000000000000MOUNICA",
        relativePath: "2026/mounica-sync",
        absolutePath: "/Users/alice/Movies/keeper/2026/mounica-sync",
        title: "Mounica sync",
        startedTs: 1_790_090_000_000,
        endedTs: 1_790_092_400_000,
        durationMs: 2_400_000,
        totalBytes: 1_290_000_000,
        durability: "local",
        tags: [],
        playablePath: "/Users/alice/Movies/keeper/2026/mounica-sync/screen-0001.mov",
        transcript: null,
        transcribable: true,
      },
    ].filter((row) => !removedRecordings.has(row.sessionId));
    return { rows, total: rows.length } satisfies RecordingSearchVm;
  },
  // Removing a recording: the plan the confirmation words, then the removal,
  // which the pane's next search reflects.
  recording_remove_preview: (payload) =>
    ({
      folder: `recordings/2026/${String(payload.sessionId).slice(-8).toLowerCase()}`,
      drive: "tgdrive",
      bytes: 812_000_000,
      files: 6,
      durability: "pushed",
      notes: [{ vaultId: "vault-mind", path: "meetings/Kelly sync.md", title: "Kelly sync" }],
    }) satisfies RecordingRemovalPreviewVm,
  recording_remove: (payload) => {
    removedRecordings.add(String(payload.sessionId));
    return {
      folder: `recordings/2026/${String(payload.sessionId).slice(-8).toLowerCase()}`,
      bytes: 812_000_000,
      files: 6,
      notesChanged: [],
      openNotes: [{ vaultId: "vault-mind", path: "meetings/Kelly sync.md", title: "Kelly sync" }],
      notesFailed: [],
    } satisfies RecordingRemovedVm;
  },
  // --- Recording settings ---------------------------------------------------
  //
  // Handlers over a module-level object rather than an `ANSWERS` row, so a write
  // lands somewhere and the mirror store's confirm path is exercised: the set
  // echoes the effective VM, exactly as the Rust command does.
  //
  // `?failure=recordingRead` refuses the read (the controls say it in place of
  // the fields), `?failure=recordingWrite` refuses every write (the control that
  // reverted says why) — R410.
  recording_settings_get: () => {
    if (failureParam.includes("recordingRead")) {
      throw {
        code: "internal",
        message: "keeper could not read its database.",
        accountId: null,
        retriable: false,
      };
    }
    return recordingSettings;
  },
  recording_settings_set: (payload) => {
    if (failureParam.includes("recordingWrite")) {
      throw {
        code: "invalid",
        message: "keeper could not write the recording settings: the disk is full.",
        accountId: null,
        retriable: false,
      };
    }
    const next = payload.settings as Partial<typeof recordingSettings> | undefined;
    recordingSettings = { ...recordingSettings, ...(next ?? {}) };
    return recordingSettings;
  },
  // --- Capture sources (spec *Recording remembers which sources are on*) -----
  //
  // The whole point being looked at here is that the switches REMEMBER, so this
  // pair survives a reload via `sessionStorage` — a handler that answered the
  // same triple every time would show exactly the bug the spec ends.
  recording_capture_sources_get: () => captureSources,
  recording_capture_sources_set: (payload) => {
    const next = payload.sources as Partial<RecordingCaptureSourcesVm> | undefined;
    captureSources = { ...captureSources, ...(next ?? {}) };
    try {
      sessionStorage.setItem(CAPTURE_SOURCES_KEY, JSON.stringify(captureSources));
    } catch {
      // A harness that cannot write still answers honestly for this page.
    }
    return captureSources;
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
  voice_wake_get: () => voiceWakeVm(),
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
    const stopPhrase = String(payload.stopPhrase ?? "stop").trim();
    voiceWake = { ...voiceWake, enabled: payload.enabled === true, phrase, stopPhrase };
    listeningDecided();
    voiceWatcher?.onmessage?.(voiceIdle());
    failureRecovered("voiceRearm");
    return voiceWakeVm();
  },
  // Epic 68 (AD-218): the one listening verb, flipped from the stored switch.
  voice_wake_toggle: () => {
    voiceWake = { ...voiceWake, enabled: !voiceWake.enabled };
    voiceWatcher?.onmessage?.(voiceIdle());
    failureRecovered("voiceRearm");
    if (voiceWake.enabled) {
      listeningDecided();
    }
    return voiceWakeVm();
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
    return voiceWakeVm();
  },
  // Epic 67 (AD-206), 108.2 (AD-384): where a spoken turn goes — an
  // `agent:<agent>` from `voice_agent_targets`, stored as given; whether it
  // still names one of the person's agents is Rust's business at send time.
  voice_target_set: (payload) => {
    voiceWake = {
      ...voiceWake,
      voiceTarget:
        typeof payload.target === "string" && payload.target !== "" ? payload.target : null,
    };
    return voiceWakeVm();
  },
  // 108.2: the agents "Speak to" lists — every agent of the agents'
  // databases this device holds, each with the value that chooses it.
  voice_agent_targets: () =>
    mockDefaultAgentVm().choices.map(
      (choice): VoiceAgentTargetVm => ({
        target: `agent:${choice.agent}`,
        agent: choice.agent,
        name: choice.name,
      }),
    ),
  // 108.2 (UX-DR-E108-1): the one-time notice of a cleared old room choice.
  voice_old_target_notice_get: () => mockOldTargetNotice,
  voice_old_target_notice_dismiss: () => {
    mockOldTargetNotice = null;
    return null;
  },
  // Story 100.3 (R383), 108.2: the default agent, from the agents' databases.
  agents_default_get: () => mockDefaultAgentVm(),
  agents_default_set: (payload: Record<string, unknown>) => {
    mockDefaultAgent = typeof payload.agent === "string" ? payload.agent : null;
    return later(150, () => mockDefaultAgentVm());
  },
  agents_default_chooser_dismiss: () => null,
  // 108.2 (UX-DR-E108-2): the agents' old Matrix rooms and the action that leaves them.
  old_agent_rooms_get: () => mockOldAgentRooms,
  old_agent_rooms_leave: () => mockOldAgentRoomsLeave(),
  // --- Voice, the talk mode (Epic 62, Story 62.6; Epic 67, AD-205) ---------
  //
  // A scripted turn so the mic control's states can be looked at in
  // `bun run dev`: listening with an interim transcript, heard, then — since
  // Epic 67 the send and the speak are Rust's — sending and speaking, back
  // to idle. Nothing here opens a stream. `voice_authorize` answers "granted"
  // — the dialogs are the phone's. The level (Epic 64, Story 64.3) rises
  // with the words and falls once they are heard, at the ~25 Hz Rust bounds
  // it to, so an indicator can be looked at too.
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
    setTimeout(() => push({ kind: "sending", answering: false }), 2200);
    setTimeout(() => push({ kind: "sending", answering: true }), 3000);
    setTimeout(() => push({ kind: "speaking" }), 4000);
    setTimeout(() => push(voiceIdle()), 6500);
    return null;
  },
  voice_stop: () => {
    voiceWatcher?.onmessage?.(voiceIdle());
    return null;
  },
  // --- Tasks (Epic 57, Story 57.6) ---------------------------------------
  //
  // Handlers rather than `ANSWERS` entries, because four of the five depend on
  // WHICH task was asked about — and because the flow worth looking at is
  // pressing Run now on a task the engine refuses. A table could not refuse.
  sync_tasks: () => TASK_LISTING,
  sync_tasks_retired_dismiss: () => {
    TASK_LISTING.retired = [];
    return null;
  },
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
      // A person pressed Run now, and a request has no window to be late for
      // (Story 74.5).
      trigger: "requested",
      lateByMs: null,
      host: "01DEVICE#4188",
      ledgerEntry: null,
    };
    // Recorded, so the next read shows the run rather than the pane appearing
    // to have done nothing — the same reason `sync_profile_save` is stateful.
    TASK_RUNS[id] = [run, ...(TASK_RUNS[id] ?? [])];
    task.lastRun = run;
    return run;
  },
  sync_task_save: (payload) => {
    const req = payload.req as TaskSaveReq;
    const id = req.id === "" ? `01JMOCKSAVED${TASKS.length}` : req.id;
    if (req.mode === "scheduled" && req.schedule === null) {
      throw {
        code: "internal",
        message: `task '${id}' is scheduled with no schedule: it would report itself enabled and never run`,
        accountId: null,
        retriable: false,
      };
    }
    const missing =
      req.kind === "copy"
        ? !req.copySource
          ? "a source path"
          : !req.copyDestination
            ? "a destination path"
            : null
        : null;
    if (missing !== null) {
      throw {
        code: "internal",
        message: `task '${id}' is a ${req.kind} task, so it needs ${missing}: choose one before saving`,
        accountId: null,
        retriable: false,
      };
    }
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
      id,
      kind: req.kind,
      mode: req.mode,
      enabled: req.enabled,
      profileId: req.profileId,
      schedule: req.schedule,
      copySource: req.copySource,
      copyDestination: req.copyDestination,
      replaceExisting: req.replaceExisting,
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
  sessions_promote_panel: (payload) =>
    promotePanel(promoteFixture(payload), payload.intent as PanelIntentVm | undefined),
  sessions_promote: promoteIntoFixture,
  sessions_promote_to: promoteToFixture,
  sessions_knowledge_read: readNoteFixture,
  sessions_knowledge_review: reviewInFixture,
  sessions_archive: archiveFixture,
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
      voices: req.voices ?? prior.voices,
      voicesSubfolder: req.voicesSubfolder ?? prior.voicesSubfolder,
      sessions: req.sessions ?? prior.sessions,
      sessionsSubfolder: req.sessionsSubfolder ?? prior.sessionsSubfolder,
      agents: req.agents ?? prior.agents,
      agentsSubfolder: req.agentsSubfolder ?? prior.agentsSubfolder,
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
    // Epic 84: a drive the account offered is no longer offered once this
    // device syncs it, as the shell's next converge would publish.
    const added = mockRemoteKey(stored.remoteUrl, stored.branch);
    if (
      accountOffers.drives.some((offer) => mockRemoteKey(offer.remoteUrl, offer.branch) === added)
    ) {
      withdrawOffers({
        ...accountOffers,
        drives: accountOffers.drives.filter(
          (offer) => mockRemoteKey(offer.remoteUrl, offer.branch) !== added,
        ),
      });
    }
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
      return { unpushed: null };
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
    // The real answer's shape: `null` once the move is on the remote, which a
    // desktop's folder publishes on its own cadence.
    return { unpushed: null };
  },
  // A person's *Allow* (92.2): Rust turns the card's `scheduled_by:` into
  // `allowed_by: <the person the shell found>`, so the card loses its mark
  // and keeps its schedule. A card without the mark is refused, as Rust
  // refuses it.
  sessions_task_allow_schedule: (payload) => {
    const rel = String(payload.rel ?? "");
    // The shell finds the person (R118); `?allow=signed-out` is neither the
    // drive's owner nor a single account signed in on this device.
    if (new URLSearchParams(window.location.search).get("allow") === "signed-out") {
      throw {
        code: "internal",
        message: "Sign in as @tgorka:example.org to allow this schedule.",
        accountId: null,
        retriable: false,
      };
    }
    const card = SESSION_TASKS.find((task) => task.relPath === rel);
    if (card?.agent?.scheduledBy == null) {
      throw {
        code: "internal",
        message: `${rel} carries no schedule an agent wrote, so there is nothing to allow. It runs on its schedule as it is.`,
        accountId: null,
        retriable: false,
      };
    }
    card.agent.scheduledBy = null;
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
  // The open note's body channel. Without it every note opens empty, and the
  // editor — its widgets, its Properties disclosure — cannot be looked at here.
  notes_open: (payload) => {
    const id = String(payload.noteId);
    const row = NOTES.find(([each]) => each === id);
    const text = row === undefined ? "" : String(row[2]);
    const tags = row === undefined ? [] : [...row[3]];
    const tagBlock =
      tags.length > 0 ? `---\ntags:\n${tags.map((tag) => `  - ${tag}\n`).join("")}---\n` : "";
    (payload.channel as MockChannel<unknown>).onmessage?.({
      kind: "reset",
      rev: "rev-mock",
      path: `${row?.[1] ?? "Untitled"}.md`,
      frontmatter: MEDIA_BLOCK_FRONTMATTER[id] ?? tagBlock,
      text,
      cursor: null,
    } satisfies NoteBodyBatch);
    return `sub-mock-note-${String(payload.noteId)}`;
  },
  // A save answers what Rust would: the new revision and the block it kept.
  // Without it every autosave threw on `write.frontmatter`, and so did the save
  // a New recording widget makes when Start names its session.
  notes_save: (payload) => {
    const id = String(payload.subscriptionId ?? "").replace("sub-mock-note-", "");
    const row = NOTES.find(([each]) => each === id);
    return {
      rev: `rev-mock-${Date.now()}`,
      path: `${row?.[1] ?? "Untitled"}.md`,
      frontmatter:
        typeof payload.frontmatter === "string"
          ? payload.frontmatter
          : (MEDIA_BLOCK_FRONTMATTER[id] ?? ""),
      conflictCopy: null,
    };
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
      stale: false,
      write: { writable: true, reason: null, caveat: null, caveatShort: null },
    };
  },
  // A file opened from the listing (Story 45.6's text reader, Story 45.8's
  // document reader). Answered so the phone's Files surface (Story 66.3) can
  // be looked at full-screen here — and measured by `dev/measure-files.ts` —
  // rather than stopping on the fallback's `null`, which the viewer renders as
  // a thrown property read. One markdown body for every text path but the
  // promote fixture's files, whose exact bytes are served; one page-count
  // probe for every document; the `keeper-file://` bytes behind a PDF's
  // `<embed>` are not served here, so the frame draws its facts over an
  // empty plugin.
  sync_read_text: (payload): TextFileVm => {
    const bytes =
      promoteFileBytes(String(payload.id ?? "p1"), String(payload.subpath ?? "")) ??
      utf8.encode(
        `# ${String(payload.subpath ?? "file")}\n\nA note read from the folder keeper syncs, over the mock shell.\n\n- one item\n- another, with a [[wikilink]]\n\n> A quotation, because a body with only headings measures nothing.\n`,
      );
    const text = utf8Text(bytes);
    const sizeBytes = bytes.length;
    const sizeLabel = `${(sizeBytes / 1000).toFixed(1)} kB`;
    return {
      text,
      sizeBytes,
      sizeLabel,
      oversize: false,
      // `text_file::decide` shows no text it would have to change.
      binary: text === null,
      detail:
        text === null
          ? `This file is not valid UTF-8 text, so keeper cannot show it without changing it. It is ${sizeLabel}.`
          : null,
    };
  },
  sync_read_document: (): DocumentVm => ({
    format: "pdf",
    sizeBytes: 8_400_000,
    sizeLabel: "8.4 MB",
    detail: null,
    truncated: false,
    pdf: { version: "1.7", pageCount: 12, encrypted: false, servable: true },
    words: null,
    slides: null,
    sheets: null,
  }),
  // The optional account (Epic 82) — see the fixtures above HANDLERS.
  account_state: () => accountVm,
  account_subscribe: (payload) => {
    accountWatcher = payload.channel as MockChannel<OrgAccountVm>;
    accountWatcher.onmessage?.(accountVm);
    return "account-subscription";
  },
  account_unsubscribe: () => {
    accountWatcher = null;
    return null;
  },
  account_setup_resolve: (payload): AccountSetupVm => {
    const input = String(payload.input ?? "");
    if (input.includes("refuse")) {
      throw {
        code: "invalidInput",
        message:
          "This setup link asks keeper to send a client secret, and keeper never holds one. Ask whoever gave you the link for a public-client descriptor.",
        accountId: null,
        retriable: false,
      };
    }
    return {
      // The link's `denied` rides the setup id to the confirm, as Rust keeps
      // the resolved descriptor under it.
      setupId: input.includes("denied")
        ? "setup-mock-denied"
        : input.includes("forge")
          ? "setup-mock-forge"
          : "setup-mock",
      name: "Acme",
      issuerHost: "id.acme.dev",
      repoHost: "git.acme.dev",
      repoMode: "same",
      deviceName: ACCOUNT_THIS_DEVICE.name,
      deviceClass: "desktop",
      registered: input.includes("registered"),
      // Rust names the account a confirm would replace; `?account=ready`
      // therefore shows the change path.
      replaces: accountVm.configured ? accountVm.name : null,
      // A descriptor with `[github_broker]`: a link containing `broker` shows it.
      brokerHost: input.includes("broker") ? BROKER_HOST : null,
      // Every OTHER host a token would go to (the account's forge is
      // `repoHost`, the broker `brokerHost`): a broker adds GitHub's two.
      forgeHosts: input.includes("broker") ? ["github.com", "api.github.com"] : [],
    };
  },
  // Each of the four account changes clears the remembered listing failures,
  // as the shell does with `forges::forget_identity()` (surface #1).
  account_setup_confirm: (payload) => {
    forgeErrors.clear();
    const name = String(payload.deviceName ?? ACCOUNT_THIS_DEVICE.name);
    const setupId = String(payload.setupId ?? "");
    return accountSignInFlow(
      { ...ACCOUNT_THIS_DEVICE, slug: name, name },
      setupId.includes("denied") ? "denied" : setupId.includes("forge") ? "forgeFailed" : "done",
    );
  },
  // The phone tier's Check for updates (102.9, R464), as
  // `keeper_core::release_check` words each answer: `?release=current` or
  // `?release=failed`, else a newer release. A desktop has the updater.
  release_check: (): Promise<ReleaseCheckVm> => {
    if (!MOCK_REDUCED) {
      return Promise.reject({
        code: "unsupported",
        message: "a desktop checks for updates with its in-app updater",
        accountId: null,
        retriable: false,
      });
    }
    const install = MOCK_PLATFORM === "tablet" ? "bun run install:android" : "bun run install:ios";
    const param = new URLSearchParams(window.location.search).get("release");
    const answer: ReleaseCheckVm =
      param === "current"
        ? {
            outcome: "current",
            installed: "0.10.5",
            latest: "0.10.5",
            sentence: "keeper is up to date.",
          }
        : param === "failed"
          ? {
              outcome: "failed",
              installed: "0.10.5",
              latest: null,
              sentence:
                "keeper could not check GitHub for a newer version (it answered 403). Try again later.",
            }
          : {
              outcome: "newer",
              installed: "0.10.5",
              latest: "0.11.0",
              sentence: `keeper 0.11.0 is available — install it from your Mac with \`${install}\`.`,
            };
    return later(900, () => answer);
  },
  account_sign_in: () => {
    forgeErrors.clear();
    return accountSignInFlow(accountVm.device ?? ACCOUNT_THIS_DEVICE);
  },
  account_cancel_sign_in: () => {
    accountSignInCancel?.();
    return null;
  },
  account_sync: (payload) => {
    if (accountVm.identity === null) {
      return accountVm;
    }
    const now = Date.now();
    if (payload.force !== true && now - accountLastAttemptMs < ACCOUNT_SYNC_THROTTLE_MS) {
      return accountVm;
    }
    accountLastAttemptMs = now;
    // A fetch cannot mend a dead grant or a sign-in that belongs to someone
    // else: Rust publishes the same state again.
    if (accountVm.state === "needsSignIn" || accountVm.state === "blocked") {
      return setAccount(accountVm);
    }
    return setAccount(signedInAccount("ready", accountVm.device ?? ACCOUNT_THIS_DEVICE));
  },
  account_rename_device: (payload) => {
    const name = String(payload.name ?? "");
    const renamed = { ...(accountVm.device ?? ACCOUNT_THIS_DEVICE), slug: name, name };
    return setAccount({
      ...accountVm,
      device: renamed,
      devices: accountVm.devices.map((device) => (device.thisDevice ? renamed : device)),
    });
  },
  // In `keeper_core::org_account::log_upload::LogUploadVm`'s words: a signed-in,
  // registered device sends; anything else is told to sign in. The switch is not
  // consulted — the button works either way.
  account_send_log: (): LogUploadVm => {
    const device = accountVm.device;
    if (accountVm.identity === null || device === null) {
      return {
        outcome: "signedOut",
        sentence:
          "No log was sent: this device is not signed in to an organisation account. Sign in under Settings › Account, then send it again.",
      };
    }
    return {
      outcome: "sent",
      sentence: `Sent. This device's log is in the settings repository at ${accountVm.identity.login}/logs/${device.slug}.log.`,
    };
  },
  account_share: (): AccountShareVm => {
    const link =
      "keeper://setup?descriptor=https%3A%2F%2Fid.acme.dev%2F.well-known%2Fkeeper-account.json";
    return { link, qrSvg: mockQrSvg(link) };
  },
  account_sign_out: () => {
    forgeErrors.clear();
    return setAccount(signedInAccount("signedOut"));
  },
  account_forget: () => {
    forgeErrors.clear();
    // Rust clears every source row bound to the forgotten account, so no drive
    // or endpoint switches to the next account by itself.
    for (const [key, source] of credentialSources) {
      if (source === "account") {
        credentialSources.delete(key);
      }
    }
    return setAccount(NO_ACCOUNT_VM);
  },
  sync_credential_source_get: (payload) =>
    credentialSources.get(`sync/${String(payload.profileId)}`) ?? "keychain",
  sync_credential_source_set: (payload) => {
    const value = String(payload.source);
    const profile = (ANSWERS.sync_profiles as SyncProfileVm[]).find(
      (candidate) => candidate.id === String(payload.profileId),
    );
    const refuse = (message: string) => {
      throw { code: "internal", message, accountId: null, retriable: false };
    };
    // The shell refuses a `forge:<id>` whose drive's remote is not on that
    // source's site (surface #21), and the account anywhere but its own
    // repository host, in its own words.
    if (value.startsWith("forge:")) {
      const source = forgeSources().find((candidate) => `forge:${candidate.id}` === value);
      if (source !== undefined && !remoteOnSourceHost(profile?.remoteUrl ?? "", source.host)) {
        refuse(`This drive's repository isn't on ${source.host}.`);
      }
    }
    if (value === "account") {
      if (!accountVm.configured) {
        refuse("No account is set up on this device.");
      }
      if (!remoteOnSourceHost(profile?.remoteUrl ?? "", accountVm.repoHost ?? "git.acme.dev")) {
        refuse(
          `This drive's repository isn't on ${accountVm.name ?? "the account"}'s hosts; choose another way to sign in.`,
        );
      }
    }
    credentialSources.set(`sync/${String(payload.profileId)}`, value);
    return null;
  },
  provider_credential_source_get: (payload) =>
    credentialSources.get(`providers/${String(payload.providerId)}`) ?? "keychain",
  provider_credential_source_set: (payload) => {
    credentialSources.set(`providers/${String(payload.providerId)}`, String(payload.source));
    return null;
  },
  // Epic 84: the one-tap add of an endpoint offered by the account. Rust's
  // refusals are reproduced word for word and in Rust's order
  // (`account_ipc.rs`, `account_offer_add_provider`), with its envelope: a
  // refusal is `internal` and not retriable.
  account_offer_add_provider: (payload) => {
    const key = String(payload.key ?? "");
    const offer = accountOffers.providers.find((candidate) => candidate.key === key);
    const refuse = (message: string) => {
      throw { code: "internal", message, accountId: null, retriable: false };
    };
    if (offer === undefined || offersIn(accountVm.state).providers.length === 0) {
      return refuse("This provider is no longer offered by your account. Sync, then try again.");
    }
    if (offer.credential !== "account") {
      return refuse("This provider uses its own key. Add it with the form and paste the key.");
    }
    if (accountVm.identity === null || !["ready", "syncing"].includes(accountVm.state)) {
      return refuse(
        "Your account is not ready on this device. Sign in to it, then add the provider.",
      );
    }
    if (offer.kind !== "hermes" && offer.kind !== "ollama" && offer.kind !== "openai") {
      return refuse(`This version of keeper cannot talk to a ${offer.kind} provider.`);
    }
    const id = `01J8OFFER${String(MODEL_PROVIDERS.length).padStart(17, "0")}`;
    MODEL_PROVIDERS.push({
      id,
      kind: offer.kind,
      name: offer.name,
      baseUrl: offer.baseUrl,
      host: new URL(offer.baseUrl).host,
      isPrivate: false,
      createdMs: Date.now(),
      health: "unknown",
      healthCheckedMs: null,
      healthDetail: null,
      readTimeoutMs: null,
      hasToken: false,
    });
    credentialSources.set(`providers/${id}`, "account");
    withdrawOffers({
      ...accountOffers,
      providers: accountOffers.providers.filter((candidate) => candidate.key !== key),
    });
    return null;
  },
  // Epic 86: repository sources. See the block above `HANDLERS`.
  forges_list: () => forgeSources(),
  // The shell's `LAST_ERROR`: a failed listing is remembered and answers the
  // source in `forges_list`; only a listing that succeeds replaces it, and a
  // non-refresh read while it stands fails the same way (surface #1).
  forge_repos: (payload) => {
    const sourceId = String(payload.sourceId);
    const refresh = payload.refresh === true;
    return new Promise<ForgeReposVm>((resolve, reject) =>
      setTimeout(() => {
        if (sourceId === "github" && (githubFailsNext || (!refresh && forgeErrors.has(sourceId)))) {
          githubFailsNext = false;
          const sentence = "Can't reach api.github.com.";
          forgeErrors.set(sourceId, { state: "unreachable", sentence });
          reject({
            code: "serverUnreachable",
            message: sentence,
            accountId: null,
            retriable: true,
          });
          return;
        }
        forgeErrors.delete(sourceId);
        resolve(forgeReposVm(sourceId));
      }, 700),
    );
  },
  forge_connect_start: (): DeviceCodeVm => ({
    userCode: "WDJB-MJHT",
    verificationUri: "https://github.com/login/device",
    expiresIn: 899,
  }),
  forge_connect_wait: () =>
    new Promise<ForgeSourceVm>((resolve) => {
      // A second wait ends the first, as the shell's second start cancels the
      // first poll: the older promise resolves `notConnected` instead of
      // hanging forever under a panel that is gone.
      githubConnectSettle?.(false);
      const approve = setTimeout(() => githubConnectSettle?.(true), 6000);
      const settle = (connected: boolean) => {
        clearTimeout(approve);
        if (githubConnectSettle === settle) {
          githubConnectSettle = null;
        }
        githubConnected = connected;
        if (connected) {
          forgeErrors.delete("github");
        }
        resolve(githubSource());
      };
      githubConnectSettle = settle;
    }),
  forge_connect_open: () => {
    if (githubConnectSettle === null) {
      throw { code: "internal", message: "No code is waiting.", accountId: null, retriable: false };
    }
    console.info("[mock-shell] would open https://github.com/login/device");
    return null;
  },
  forge_connect_cancel: () => {
    githubConnectSettle?.(false);
    return null;
  },
  forge_disconnect: () => {
    githubConnected = false;
    githubVia = "deviceFlow";
    forgeErrors.delete("github");
    return githubSource();
  },
  forge_default_base_folder: () => mockDriveFolder()?.path ?? null,
  sync_drive_folder_get: () => mockDriveFolder(),
  // Rust's rules: empty resets to the default, `~/` is the home folder, and any
  // other relative path is refused in its own words.
  sync_drive_folder_set: (payload) => {
    if (mockDriveFolder() === null) {
      return null;
    }
    const folder = typeof payload.folder === "string" ? payload.folder.trim() : "";
    if (folder === "") {
      chosenDriveFolder = null;
    } else if (folder.startsWith("/")) {
      chosenDriveFolder = folder;
    } else if (folder === "~" || folder.startsWith("~/")) {
      chosenDriveFolder = `${MOCK_HOME}${folder.slice(1)}`;
    } else {
      throw {
        code: "internal",
        message: `${folder} is not a full path. Choose a folder that starts with / or ~/.`,
        accountId: null,
        retriable: false,
      };
    }
    return mockDriveFolder();
  },
  // Rust's `credential_choices`: the account only for a remote on its own
  // repository host; a source only at its own origin, and never the account's
  // forge a second time. GitHub through the broker names the account.
  sync_credential_choices: (payload): CredentialChoicesVm => {
    const remoteUrl = String(payload.remoteUrl ?? "");
    const accountName = accountVm.name ?? "organisation";
    const account =
      accountVm.configured && remoteOnSourceHost(remoteUrl, accountVm.repoHost ?? "git.acme.dev")
        ? {
            value: "account",
            label: `Use my ${accountName} account`,
            detail: `keeper signs this folder's git requests in with your ${accountName} sign-in, so no token is stored for it.`,
          }
        : null;
    const forges = forgeSources()
      .filter(
        (source) => source.credential !== "account" && remoteOnSourceHost(remoteUrl, source.host),
      )
      .map((source) =>
        source.via === "broker" && accountVm.configured
          ? {
              value: source.credential,
              label: `${source.name} access through ${accountName}`,
              detail: `${accountName} gives keeper a one-hour ${source.name} token for this repository only, as you; nothing is stored for this folder.`,
            }
          : {
              value: source.credential,
              label: `Sign in with ${source.name}`,
              detail: `keeper uses your ${source.name} connection, so no token is stored for this folder.`,
            },
      );
    return { account, forges };
  },
  // Rust's batch add: each repository's folder is `<base>/<name>`; the one
  // named `playground` holds other files, which is the conflict the step has
  // to show on its own row while the rest are added.
  forge_repos_add: (payload) => {
    const req = payload.req as ForgeAddReq;
    const vm = forgeReposVm(req.sourceId);
    const credential =
      forgeSources().find((candidate) => candidate.id === req.sourceId)?.credential ??
      `forge:${req.sourceId}`;
    return later(1200, () =>
      req.repos.map((item): ForgeAddResultVm => {
        const repo = vm.repos.find((candidate) => candidate.fullName === item.fullName);
        const folder = item.folder ?? `${req.baseFolder ?? "/container"}/${item.driveName}`;
        if (repo === undefined) {
          return { fullName: item.fullName, profileId: null, sentence: "That repository is gone." };
        }
        // The shell refuses a repository already synced here, whatever folder
        // the batch names (surface #4): a second drive of it goes through the
        // single form only.
        if (repo.addedAs.length > 0) {
          return {
            fullName: item.fullName,
            profileId: null,
            sentence: `Already syncing here as ${repo.addedAs[0]}.`,
          };
        }
        if (item.driveName === "playground") {
          return {
            fullName: item.fullName,
            profileId: null,
            sentence: `${folder} holds other files.`,
          };
        }
        const profiles = ANSWERS.sync_profiles as SyncProfileVm[];
        const id = `p${profiles.length + 1}`;
        ANSWERS.sync_profiles = [...profiles, forgeAddedProfile(id, item, folder, repo)];
        credentialSources.set(`sync/${id}`, credential);
        return { fullName: item.fullName, profileId: id, sentence: null };
      }),
    );
  },
  // The three Matrix sign-ins, so the add-account screen can be driven to the
  // end here. Each answers the account Rust would, and takes away the account's
  // offer for it, as the next converge does once the account exists here.
  login_password: (payload) =>
    matrixSignedIn(String(payload.homeserver ?? ""), String(payload.username ?? ""), "password"),
  login_oidc: (payload) => matrixSignInFlow(payload),
  cancel_oidc: () => {
    ssoCancel?.();
    cancelRestoreSignIn();
    return null;
  },
  login_beeper: (payload) =>
    matrixSignedIn(
      "https://matrix.beeper.com/",
      `@${String(payload.email ?? "").split("@")[0]}:beeper.com`,
      "beeper",
    ),

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

// A tapped notification (106.6, A106-02/03): `window.__keeperMockPushTap({database,
// sessionId}, arriveAfterMs?)` keeps the target where `push_open_take` answers and emits
// `push://open`, as the phone's shell does. With `arriveAfterMs`, a session this device
// does not hold yet is refused as still arriving until it lands in the list then.
let pushWaiting: PushOpenVm | null = null;
const pushArriving = new Set<string>();
function pushTap(target: { database: string; sessionId: string }, arriveAfterMs?: number): void {
  const key = JSON.stringify([target.database, target.sessionId]);
  if (arriveAfterMs !== undefined && !sessionViews.has(key)) {
    pushArriving.add(key);
    window.setTimeout(() => {
      const row: AgentSessionRowVm = {
        database: target.database,
        sessionId: target.sessionId,
        title: "Arrived after the tap",
        titleWrite: { state: "confirmed" },
        displayName: "researcher · Arrived after the tap",
        handle: "lucky-salmon-sprints",
        createdAtMs: Date.now(),
        trust: "native",
        instances: [],
        agentActor: "researcher",
        agentName: "Researcher",
        agentMark: "R",
        archived: false,
        lastActivityMs: Date.now(),
      };
      sessionRows.unshift(row);
      sessionViews.set(key, {
        ...row,
        readOnly: false,
        complete: true,
        hasEarlier: false,
        loadingEarlier: false,
        readsTrace: true,
        untrusted: false,
        live: [],
        tools: [],
        prefs: { showReasoning: false, showDetails: false },
        approvals: [],
        messages: [
          {
            id: `arrived-${target.sessionId}`,
            role: "agent",
            authorActor: "researcher",
            authorName: "Researcher",
            body: "The answer the notification was about.",
            createdAtMs: Date.now(),
          },
        ],
      });
      pushArriving.delete(key);
      publishSessions();
    }, arriveAfterMs);
  }
  pushWaiting = { kind: "session", ...target };
  void emit(PUSH_OPEN_EVENT);
}

export function installMockShell(): void {
  if (realShellPresent()) {
    return;
  }
  // A job for the not-yet-transcribed session leaves a transcript behind, so
  // the block that started it shows lines when the job ends.
  const startJob = HANDLERS.transcription_start;
  HANDLERS.transcription_start = (payload) => {
    markFreshTranscribed(String(payload.path));
    return startJob(payload);
  };
  // Events are the mocks' own: a listener registers, and `emit` here reaches
  // it the way the shell's `app.emit` reaches the webview.
  mockIPC(
    (command, payload) => {
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
    },
    { shouldMockEvents: true },
  );
  // The conversation pane listens for file drops on the current webview, which
  // reads the window's label from the shell's metadata; without it opening any
  // room throws before the timeline draws.
  mockWindows("main");
  mockConvertFileSrc("linux");
  // mockWindows/mockConvertFileSrc installed this in-process Tauri boundary above.
  const mockWindow = window as unknown as {
    __TAURI_INTERNALS__: {
      convertFileSrc: (path: string, protocol?: string) => string;
    };
  };
  const internals = mockWindow.__TAURI_INTERNALS__;
  const convert = internals.convertFileSrc;
  internals.convertFileSrc = (path, protocol) =>
    protocol === "keeper-hq-blob" && mockStaged.has(path)
      ? photoThumbnail
      : convert(path, protocol);
  // What the shell's initialization script writes before the bundle runs
  // (`ipc::platform_plugin`): applied here, before the app mounts.
  platformStore.getState().applySnapshot(MOCK_PLATFORM_FACTS);
  (window as unknown as { __keeperMockPushTap: typeof pushTap }).__keeperMockPushTap = pushTap;
  document.documentElement.dataset.mockShell = "on";
}

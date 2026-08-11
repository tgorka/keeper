/**
 * A fake shell, so the real frontend can be looked at without Tauri.
 *
 * **Why this exists.** The `keeper` shell crate does not build on Linux (AD-55,
 * AD-56), so for five epics the only way to see this app was to build it on a
 * Mac and look at it there — a fifteen-minute round trip that made visual work
 * effectively impossible and is the honest reason the UI stayed characterless.
 * Every design decision was made by reading code.
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

/** A folder tree with the depth and the awkward names a real drive has. */
const ENTRIES = [
  ["00-inbox", true, 0],
  ["10-notes", true, 0],
  ["20-records", true, 0],
  ["30-work", true, 0],
  ["40-media", true, 0],
  ["50-library", true, 0],
  [".gitattributes", false, 16_384],
  ["AGENTS.md", false, 4_812],
  ["README.md", false, 3_380],
  ["deck-v10-complete.pdf", false, 8_400_000],
  ["screen-0000.mov", false, 412_000_000],
].map(([name, isDir, size]) => ({
  name,
  relativePath: String(name),
  absolutePath: `/Volumes/merope/tgdrive/${name}`,
  isDir,
  sizeBytes: isDir ? null : size,
  sync: "synced",
  roles: [],
  write: { writable: true, reason: null, caveat: null },
  unspellable: null,
}));

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
  sync_browse: { entries: ENTRIES, truncated: false },
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

/** True when a real Tauri shell is already answering. */
function realShellPresent(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export function installMockShell(): void {
  if (realShellPresent()) {
    return;
  }
  mockIPC((command, payload) => {
    const answer = command in ANSWERS ? ANSWERS[command] : fallback(command);
    // One line per call, so a screen that looks wrong can be traced to the
    // command it asked for rather than guessed at.
    console.debug("[mock-shell]", command, payload ?? "", "→", answer);
    return answer;
  });
  document.documentElement.dataset.mockShell = "on";
}

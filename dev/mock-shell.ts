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
import type { FileSizeVm, FilesEntryVm, FilesListingVm } from "@/lib/ipc/client";

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
    // Writable, because the write path — New file, Delete, and the header's
    // count that gates them — is exactly what a viewing aid has to be able to
    // show. A refusal is a different fixture and this is not it.
    write: { writable: true, reason: null, caveat: null },
  };
}

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
];

/**
 * What one folder inside the root holds.
 *
 * One and not six: an expansion has to show something other than the root's own
 * eleven rows again, and beyond that this is a viewing aid rather than a disk.
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
const HANDLERS: Record<string, (payload: Record<string, unknown>) => unknown> = {
  notes_body_read: (payload) => {
    const row = NOTES.find(([id]) => id === payload.noteId);
    if (row === undefined) {
      return null;
    }
    const [, title, body] = row;
    const rest = String(body).split("\n").slice(1).join("\n");
    return { rev: "rev-mock", text: `# ${title}\n${rest}` };
  },
  // The tree asks this once per folder it opens, so a table would answer the
  // root's eleven rows for every expansion and the tree would repeat itself
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
      write: { writable: true, reason: null, caveat: null },
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

/**
 * The `keeper-media` block over the mock shell: every state a block can be in
 * (UX-DR124), for looking at in a browser.
 *
 * Rust reads a block's TOML; this fixture does not pretend to. It recognises
 * the handful of lines the notes below carry — one source, a window, markers —
 * with line patterns, which is enough to draw, mark, rename, remove and clip
 * over the fixture notes and says nothing about the grammar itself (that is
 * `keeper-core`'s tests).
 */
import type {
  LineEditVm,
  MediaAdoptionVm,
  MediaBlockProblemVm,
  MediaBlockRecordingVm,
  MediaBlockSchemaVm,
  MediaBlockVm,
  MediaClipVm,
  MediaMarkerHitVm,
  MediaMarkerVm,
  NoteEmbedPathVm,
  RecordingLinkedNoteVm,
  TranscriptVm,
} from "@/lib/ipc/client";
import type { MediaRef } from "@/lib/ipc/gen/MediaRef";
import { SESSION_TRANSCRIPT_FIXTURE, TRANSCRIPT_FIXTURE } from "./transcription-fixture";

/** A two-part session with a camera, transcribed. */
export const KELLY_SESSION = "01J8MOCKDEVICE0000000000000-01J8KELLYSYNC00000000000000";
/** A session keeper has recorded and nobody has transcribed yet. */
export const FRESH_SESSION = "01J8MOCKDEVICE0000000000000-01J8FRESH0000000000000000000";

const DRIVE = "/Volumes/merope/tgdrive";

/** The notes that carry blocks, in the shape `NOTES` in `mock-shell.ts` takes. */
export const MEDIA_BLOCK_NOTES = [
  [
    "n11",
    "Kelly sync",
    [
      "# Kelly sync",
      "",
      "```keeper-media",
      `session = "${KELLY_SESSION}"`,
      'title = "Kelly sync"',
      "",
      "# A comment is the person's, and keeper keeps it.",
      "[[marker]]",
      'name = "The price we agreed"',
      'at = "00:00:15"',
      "",
      "[[marker]]",
      'name = "Demo"',
      'from = "00:00:28"',
      'to = "00:00:33"',
      "```",
      "",
      "Follow-ups: send the quote.",
    ].join("\n"),
    ["recordings", "meetings"],
  ],
  [
    "n12",
    "Pricing — the clip",
    [
      "# Pricing — the clip",
      "",
      "The part of the Kelly sync worth keeping, see [[Kelly sync#The price we agreed]].",
      "",
      "```keeper-media",
      `session = "${KELLY_SESSION}"`,
      'from = "00:00:06"',
      'to = "00:00:24"',
      "```",
      "> [!transcript]- Kelly sync · 00:00:06–00:00:24",
      "> **[00:00:06] Speaker 1:** Let’s review the keeper release and the dictionary.",
      "> **[00:00:15] Alex:** The recordings and voices stay in our drive.",
      "",
      "After the clip.",
    ].join("\n"),
    ["meetings"],
  ],
  [
    "n13",
    "Standup — not transcribed yet",
    ["# Standup", "", "```keeper-media", `session = "${FRESH_SESSION}"`, "```", ""].join("\n"),
    ["recordings"],
  ],
  [
    "n14",
    "An old recording note",
    [
      "# An old recording note",
      "",
      "Written before the media player: one embed per file.",
      "",
      "![[meeting.mov]]",
      "",
      "A broken block below, to see a refusal:",
      "",
      "```keeper-media",
      `session = "${KELLY_SESSION}"`,
      'form = "00:12:00"',
      "```",
      "",
    ].join("\n"),
    [],
  ],
] as const;

/**
 * The frontmatter keeper writes on a recording note, for the notes above that
 * are one — so Properties opens on a recording note with its block mounted,
 * which is how the owner's notes are.
 */
export const MEDIA_BLOCK_FRONTMATTER: Record<string, string> = {
  n11: `---\nsession: ${KELLY_SESSION}\nrecording: 60-sessions/recordings/2026/09/28 Kelly sync\nfiles:\n  - screen-0000.mov\n  - screen-0001.mov\n  - camera-0000.mov\ntags:\n  - recordings\n  - meetings\n---\n`,
  n13: `---\nsession: ${FRESH_SESSION}\nrecording: 60-sessions/recordings/2026/09/29 standup\ntags:\n  - recordings\n---\n`,
};

/** Whether the fresh session has been transcribed in this page's lifetime. */
let freshTranscribed = false;

function refuse(message: string): never {
  throw { code: "notesInvalid", message, accountId: null, retriable: false };
}

/** `hh:mm:ss` or seconds, as the notes above write them. */
function seconds(value: string): number {
  const parts = value.split(":").map(Number);
  return parts.reduce((total, part) => total * 60 + part, 0);
}

function stamp(total: number): string {
  const n = Math.floor(total);
  return [Math.floor(n / 3600), Math.floor((n / 60) % 60), n % 60]
    .map((part) => String(part).padStart(2, "0"))
    .join(":");
}

interface Parsed {
  keys: Record<string, string>;
  markers: MediaMarkerVm[];
}

/** The fixture's line reading of a body: top-level keys and `[[marker]]` tables. */
function parse(source: string): Parsed {
  const keys: Record<string, string> = {};
  const markers: MediaMarkerVm[] = [];
  let marker: Record<string, string> | null = null;
  const flush = () => {
    if (marker === null) return;
    const from = marker.at ?? marker.from;
    markers.push({
      name: marker.name ?? "",
      from: seconds(from ?? "0"),
      to: marker.at === undefined && marker.to !== undefined ? seconds(marker.to) : null,
    });
  };
  for (const raw of source.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    if (line === "[[marker]]") {
      flush();
      marker = {};
      continue;
    }
    const match = /^([a-z]+)\s*=\s*"?([^"]*)"?$/.exec(line);
    if (match === null) refuse(`keeper cannot read this line of the block: ${line}`);
    if (marker !== null) marker[match[1]] = match[2];
    else keys[match[1]] = match[2];
  }
  flush();
  const known = [
    "session",
    "transcript",
    "src",
    "title",
    "from",
    "to",
    "picture",
    "sound",
    "version",
  ];
  for (const key of Object.keys(keys)) {
    if (!known.includes(key)) refuse(`This block has a key keeper does not know: ${key}.`);
  }
  if (Number(keys.version ?? "1") > 1) refuse("This block was written by a newer keeper.");
  return { keys, markers };
}

/** The transcript a source names, and whether it exists yet. */
function transcriptFor(keys: Record<string, string>): {
  vm: TranscriptVm;
  path: string;
  transcribed: boolean;
  sessionId: string | null;
} {
  if (keys.session === KELLY_SESSION) {
    return {
      vm: SESSION_TRANSCRIPT_FIXTURE,
      path: SESSION_TRANSCRIPT_FIXTURE.path,
      transcribed: true,
      sessionId: KELLY_SESSION,
    };
  }
  if (keys.session === FRESH_SESSION) {
    return {
      vm: TRANSCRIPT_FIXTURE,
      path: `${DRIVE}/recordings/2026-09-29 standup/transcript.json`,
      transcribed: freshTranscribed,
      sessionId: FRESH_SESSION,
    };
  }
  if (keys.transcript !== undefined || keys.session === undefined) {
    return {
      vm: TRANSCRIPT_FIXTURE,
      path: TRANSCRIPT_FIXTURE.path,
      transcribed: true,
      sessionId: null,
    };
  }
  return refuse("keeper does not know this recording on this Mac.");
}

function resolve(source: string): MediaBlockVm {
  const { keys, markers } = parse(source);
  const { vm, path, transcribed, sessionId } = transcriptFor(keys);
  const duration = vm.transcript.duration;
  const from = keys.from === undefined ? 0 : seconds(keys.from);
  const to = keys.to === undefined ? null : Math.min(seconds(keys.to), duration);
  const hi = to ?? duration;
  const ref = (file: string, kind: MediaRef["kind"]): MediaRef =>
    sessionId === null
      ? { via: "file", profileId: "v1", relativePath: file, kind }
      : { via: "recording", sessionId, relativePath: file, kind };
  const session = vm.transcript.source.parts.length > 1;
  const parts = vm.transcript.source.parts.map((part) => ({
    file: part.file,
    offset: part.offset,
    duration: part.duration,
    screen: ref(part.file, "video"),
    camera: session ? ref(part.file.replace(/^screen-/, "camera-"), "video") : null,
    audioTracks: part.tracks.flatMap((track) =>
      track.track === null ? [] : [{ index: track.track, origin: track.origin }],
    ),
    here: true,
  }));
  return {
    title: keys.title ?? vm.transcript.source.title ?? vm.transcript.source.files[0] ?? null,
    sessionId,
    transcriptPath: path,
    transcribed,
    transcribePath: transcribed ? null : `${DRIVE}/recordings/2026-09-29 standup`,
    duration,
    window: { from, to },
    picture: (keys.picture as MediaBlockVm["picture"]) ?? null,
    sound: (keys.sound as MediaBlockVm["sound"]) ?? null,
    media: {
      parts,
      hasCamera: parts.some((part) => part.camera !== null),
      hasScreen: true,
    },
    lines: transcribed
      ? vm.transcript.utterances
          .filter((u) => u.end > from && u.start < hi)
          .map(({ id, speaker, start, end, text }) => ({ id, speaker, start, end, text }))
      : [],
    speakers: vm.transcript.speakers.map(({ id, name, origin }) => ({
      id,
      name: name ?? `Speaker ${id.replace(/^S/, "")}`,
      origin,
    })),
    markers,
  };
}

/** Every `keeper-media` fence body in a note, in order. */
function fenceBodies(body: string): string[] {
  return [...body.matchAll(/^```keeper-media\n([\s\S]*?)\n```$/gm)].map((match) => match[1]);
}

/** The line range of the marker table named `name` inside `source`. */
function markerRange(source: string, name: string): [number, number] | null {
  const lines = source.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].trim() !== "[[marker]]") continue;
    let end = index + 1;
    while (end < lines.length && lines[end].trim() !== "[[marker]]" && lines[end].trim() !== "")
      end += 1;
    const table = lines.slice(index, end);
    if (table.some((line) => line.trim().toLowerCase() === `name = "${name.toLowerCase()}"`))
      return [index, end];
  }
  return null;
}

const FORBIDDEN = /[[\]|#^\n]/;

/** `media_block_schema`'s answer, as Rust's `media_block::hints` gives it. */
const MOCK_SCHEMA: MediaBlockSchemaVm = {
  keys: (
    [
      ["session", "root", "session", [], true, "The recording to play, by its identity."],
      ["transcript", "root", "transcript", [], true, "A transcript file in this drive."],
      ["part", "root", "tables", [], true, "A media file to play with no transcript."],
      ["src", "root", "config", [], true, "A .toml file in this drive holding these keys."],
      ["record", "root", "choice", ["new"], true, "A block that records here."],
      ["title", "root", "text", [], false, "The block's own title."],
      ["from", "root", "time", [], false, "Where the block starts on the source's clock."],
      ["to", "root", "time", [], false, "Where the block stops on the source's clock."],
      [
        "picture",
        "root",
        "choice",
        ["screen", "camera", "both"],
        false,
        "Which pictures the player shows.",
      ],
      [
        "sound",
        "root",
        "choice",
        ["system", "microphone", "both"],
        false,
        "Which sounds the player plays.",
      ],
      ["marker", "root", "tables", [], false, "A named moment or window."],
      ["version", "root", "version", [], false, "The grammar version; 1."],
      ["file", "part", "media", [], false, "The part's audio or video file."],
      ["camera", "part", "video", [], false, "A video filmed beside the file."],
      ["offset", "part", "time", [], false, "Where the part starts on the block's clock."],
      ["system", "part", "track", [], false, "The call's audio track, from 1."],
      ["microphone", "part", "track", [], false, "The microphone's audio track, from 1."],
      ["name", "marker", "text", [], false, "The moment's name."],
      ["at", "marker", "time", [], false, "When the moment is."],
      ["from", "marker", "time", [], false, "Where the moment's window starts."],
      ["to", "marker", "time", [], false, "Where the moment's window ends."],
    ] as const
  ).map(([key, place, value, values, source, doc]) => ({
    key,
    place,
    value,
    values: [...values],
    source,
    doc,
  })),
};

export function mediaBlockMockHandlers(): Record<
  string,
  (payload: Record<string, unknown>) => unknown
> {
  return {
    media_block_resolve: (p): MediaBlockVm => resolve(String(p.source)),
    media_block_edit: (p): string => {
      const source = String(p.source);
      const edit = p.edit as
        | { op: "add"; name: string; from: number; to: number | null }
        | { op: "rename"; name: string; newName: string }
        | { op: "remove"; name: string };
      const { markers } = parse(source);
      const taken = (name: string) =>
        markers.some((marker) => marker.name.toLowerCase() === name.trim().toLowerCase());
      const check = (name: string) => {
        if (name.trim() === "") refuse("A marker needs a name.");
        if (name.length > 80) refuse("A marker's name is at most 80 characters.");
        if (FORBIDDEN.test(name))
          refuse("A marker's name cannot hold [, ], |, # or ^: a link to it could not carry them.");
        if (taken(name)) refuse(`This block already has a marker called ${name.trim()}.`);
      };
      if (edit.op === "add") {
        check(edit.name);
        const table =
          edit.to === null
            ? ["[[marker]]", `name = "${edit.name.trim()}"`, `at = "${stamp(edit.from)}"`]
            : [
                "[[marker]]",
                `name = "${edit.name.trim()}"`,
                `from = "${stamp(edit.from)}"`,
                `to = "${stamp(Math.ceil(edit.to))}"`,
              ];
        return `${source}\n\n${table.join("\n")}`;
      }
      const range = markerRange(source, edit.name) ?? refuse(`No marker called ${edit.name}.`);
      const lines = source.split("\n");
      if (edit.op === "rename") {
        if (edit.newName.trim().toLowerCase() !== edit.name.toLowerCase()) check(edit.newName);
        for (let index = range[0]; index < range[1]; index += 1) {
          if (lines[index].trim().startsWith("name ="))
            lines[index] = `name = "${edit.newName.trim()}"`;
        }
        return lines.join("\n");
      }
      lines.splice(range[0], range[1] - range[0] + (lines[range[1]] === "" ? 1 : 0));
      return lines.join("\n").replace(/\n+$/, "");
    },
    media_block_clip: (p): MediaClipVm => {
      const { keys } = parse(String(p.source));
      const vm = resolve(String(p.source));
      const read = (value: unknown, name: string): number | null => {
        if (value === null || value === undefined || value === "") return null;
        if (!/^\d{2}:[0-5]\d:[0-5]\d$/.test(String(value)))
          refuse(`${name} is not a time keeper can read. Write it as hh:mm:ss.`);
        return seconds(String(value));
      };
      const from = read(p.from, "From");
      const to = read(p.to, "To");
      if (from !== null && to !== null && from >= to) refuse("A clip ends after it starts.");
      const lo = from ?? vm.window.from;
      const hi = to ?? vm.window.to ?? vm.duration;
      if (lo < vm.window.from || hi > (vm.window.to ?? vm.duration))
        refuse("A clip of this block stays inside its own window.");
      const source =
        keys.session !== undefined
          ? `session = "${keys.session}"`
          : `transcript = "${keys.transcript}"`;
      const inside = vm.markers.filter((m) => m.from >= lo && (m.to ?? m.from) <= hi);
      const lines = vm.lines.filter((line) => line.end > lo && line.start < hi);
      const names = new Map(
        vm.speakers.map((s) => [s.id, s.name ?? `Speaker ${s.id.replace(/^S/, "")}`]),
      );
      const fence = [
        "```keeper-media",
        source,
        ...(keys.title ? [`title = "${keys.title}"`] : []),
        `from = "${stamp(lo)}"`,
        `to = "${stamp(hi)}"`,
        ...inside.flatMap((m) =>
          m.to === null
            ? ["", "[[marker]]", `name = "${m.name}"`, `at = "${stamp(m.from)}"`]
            : [
                "",
                "[[marker]]",
                `name = "${m.name}"`,
                `from = "${stamp(m.from)}"`,
                `to = "${stamp(m.to)}"`,
              ],
        ),
        "```",
      ];
      const words = p.words
        ? [
            `> [!transcript]- ${vm.title ?? "Meeting"} · ${stamp(lo)}–${stamp(hi)}`,
            ...lines.map((l) => `> **[${stamp(l.start)}] ${names.get(l.speaker)}:** ${l.text}`),
          ]
        : [];
      return { markdown: `${[...fence, ...words].join("\n")}\n`, lines: lines.length };
    },
    media_block_schema: (): MediaBlockSchemaVm => MOCK_SCHEMA,
    // The fixture's check refuses only an unknown root key, the refusal a
    // person typing a block meets first; Rust's own is `media_block::check`.
    media_block_check: (p): MediaBlockProblemVm | null => {
      const lines = String(p.source).split("\n");
      const root = MOCK_SCHEMA.keys.filter((key) => key.place === "root").map((key) => key.key);
      for (const [index, line] of lines.entries()) {
        if (/^\s*\[/.test(line)) return null;
        const key = /^(\s*)([A-Za-z0-9_-]+)\s*=/.exec(line);
        if (key !== null && !root.includes(key[2])) {
          return {
            message: `This block has \`${key[2]}\`, which is not a keeper-media key.`,
            line: index + 1,
            from: key[1].length,
            to: key[1].length + key[2].length,
          };
        }
      }
      return null;
    },
    media_block_sources: (p): string[] =>
      fenceBodies(String(p.body)).flatMap((body) => {
        const match = /^session = "([^"]+)"$/m.exec(body);
        return match === null ? [] : [match[1]];
      }),
    media_block_find_marker: (p): MediaMarkerHitVm | null => {
      const name = String(p.name).toLowerCase();
      const bodies = fenceBodies(String(p.body));
      for (let block = 0; block < bodies.length; block += 1) {
        let markers: MediaMarkerVm[];
        try {
          markers = parse(bodies[block]).markers;
        } catch {
          continue;
        }
        const hit = markers.find((marker) => marker.name.toLowerCase() === name);
        if (hit !== undefined) return { block, name: hit.name, from: hit.from, to: hit.to };
      }
      return null;
    },
    media_block_for_embed: (p): LineEditVm[] => {
      const target = String(p.target);
      if (!/\.(mov|mp4|m4a|wav)$/i.test(target))
        refuse(`${target} is not a file a player can play.`);
      return [
        {
          firstLine: Number(p.line),
          lastLine: Number(p.line),
          text: ["```keeper-media", "[[part]]", `file = "${target}"`, "```"].join("\n"),
        },
      ];
    },
    media_block_compose: (p): string => {
      const pick = p.pick as
        | { kind: "session"; sessionId: string }
        | { kind: "file"; relativePath: string }
        | { kind: "newRecording" };
      // The optional keys, commented, as Rust writes them (`docs/notes.md`).
      const hints = [
        '# title = ""',
        '# from = "00:00:00"',
        '# to = ""',
        '# picture = "both"      # screen | camera | both',
        '# sound = "both"        # system | microphone | both',
        "# [[marker]]",
        '# name = ""',
        '# at = "00:00:00"',
        "# sources: session | transcript | [[part]] file/camera/offset/system/microphone | src",
      ].join("\n");
      if (pick.kind === "newRecording") {
        // No marker's shape: a block that has recorded nothing has no moments.
        const lines = hints.split("\n");
        const kept = [...lines.slice(0, 5), lines[lines.length - 1]].join("\n");
        return `\`\`\`keeper-media\nrecord = "new"\n${kept}\n\`\`\`\n`;
      }
      if (pick.kind === "session")
        return `\`\`\`keeper-media\nsession = "${pick.sessionId}"\n${hints}\n\`\`\`\n`;
      return pick.relativePath.endsWith(".json")
        ? `\`\`\`keeper-media\ntranscript = "${pick.relativePath}"\n${hints}\n\`\`\`\n`
        : `\`\`\`keeper-media\n${hints}\n[[part]]\nfile = "${pick.relativePath}"\n\`\`\`\n`;
    },
    // Rust's answers, read the way `media_block::parse` reads the one key.
    media_block_recording: (p): MediaBlockRecordingVm => {
      const source = String(p.source);
      const session = /^\s*session\s*=\s*"([^"]*)"/m.exec(source)?.[1] ?? null;
      // Nothing records in this shell, so no block names the live session.
      const records = /^\s*record\s*=\s*"new"\s*$/m.test(source);
      return { records, session, live: false, here: false };
    },
    media_block_record_started: (p): string =>
      String(p.source).replace(
        /^(\s*)record(\s*=\s*)"new"/m,
        `$1session$2"${String(p.sessionId)}"`,
      ),
    // Nothing records in this shell, so no note is linked to a session.
    recording_linked_note: (): RecordingLinkedNoteVm | null => null,
    // The daily reconcile's manual trigger: accepted, as with nothing recording.
    recordings_reconcile_now: (): boolean => true,
    recording_notes_adopt_media_block: (p): MediaAdoptionVm => ({
      changed: p.dryRun ? 3 : 3,
      skipped: ["recordings/2026-08-08 pricing call.md"],
    }),
    notes_embed_paths: (p): (NoteEmbedPathVm | null)[] =>
      (p.targets as string[]).map((target) =>
        /\.(mov|mp4|m4a|wav|png|jpg)$/i.test(target)
          ? {
              relPath: target,
              absolutePath: `${DRIVE}/notes/${target}`,
              kind: /\.(png|jpg)$/i.test(target)
                ? "image"
                : /\.(m4a|wav)$/i.test(target)
                  ? "audio"
                  : "video",
            }
          : null,
      ),
  };
}

/** The mock job's end for the fresh session flips it to transcribed, so the
 *  block's re-resolve after the job shows lines, as the real event would. */
export function markFreshTranscribed(path: string): void {
  if (path.endsWith("2026-09-29 standup")) freshTranscribed = true;
}

import type {
  DictionaryTermVm,
  MediaClipVm,
  PersonVm,
  SyncProfileVm,
  TranscriptionProgressVm,
  TranscriptionStatusVm,
  TranscriptVm,
} from "@/lib/ipc/client";
import type { MediaRef } from "@/lib/ipc/gen/MediaRef";
import type { TrackOrigin } from "@/lib/ipc/gen/TrackOrigin";
import type { TranscriptMediaVm } from "@/lib/ipc/gen/TranscriptMediaVm";
import type { Word } from "@/lib/ipc/gen/Word";

/** A line's words spread evenly over its time, as the recognizer would time them. */
function timed(text: string, start: number, end: number): Word[] {
  const tokens = text.split(/\s+/).filter(Boolean);
  const step = (end - start) / tokens.length;
  return tokens.map((token, i) => ({
    text: token,
    start: start + i * step,
    end: start + (i + 1) * step,
    confidence: 0.9,
  }));
}
export const TRANSCRIPT_FIXTURE: TranscriptVm = {
  path: "/Volumes/merope/tgdrive/meeting.mov.transcript.json",
  sourcePath: "/Volumes/merope/tgdrive/meeting.mov",
  people: [
    { id: "p-me", name: "Alex", aliases: [], isSelf: true, samples: 3, hasEmbeddingForModel: true },
    {
      id: "p-anna",
      name: "Anna Kowalski",
      aliases: ["Anna"],
      isSelf: false,
      samples: 2,
      hasEmbeddingForModel: true,
    },
  ],
  transcript: {
    version: 1,
    createdAt: "2026-09-28T09:30:00+02:00",
    duration: 42,
    language: "en",
    source: {
      kind: "recording",
      files: ["meeting.mov"],
      title: "meeting.mov",
      parts: [
        {
          file: "meeting.mov",
          offset: 0,
          duration: 42,
          tracks: [
            { track: 0, origin: "system" },
            { track: 1, origin: "microphone" },
          ],
        },
      ],
    },
    engine: { asr: "Parakeet TDT v3", diarizer: "community-1", embedding: "pyannote-community-1" },
    speakers: [
      {
        id: "ME",
        origin: "microphone",
        personId: "p-me",
        name: "Alex",
        status: "self",
        score: null,
        candidates: [],
        embedding: null,
        clip: { file: "meeting.mov", track: 1, start: 0, end: 5 },
      },
      {
        id: "S1",
        origin: "system",
        personId: null,
        name: null,
        status: "suggested",
        score: 0.63,
        candidates: [{ personId: "p-anna", name: "Anna Kowalski", score: 0.63 }],
        embedding: null,
        clip: { file: "meeting.mov", track: 0, start: 6, end: 14 },
      },
      // A voice the diarizer heard whose lines were all moved elsewhere: kept, so
      // it stays a reassign target, and hidden from the speakers legend.
      {
        id: "S2",
        origin: "system",
        personId: null,
        name: null,
        status: "unknown",
        score: null,
        candidates: [],
        embedding: null,
        clip: null,
      },
    ],
    utterances: [
      {
        id: "u1",
        speaker: "ME",
        origin: "microphone",
        start: 0,
        end: 5,
        text: "Kowalsky will join us today.",
        asrText: "Kowalsky will join us today.",
        edited: false,
        words: timed("Kowalsky will join us today.", 0, 5),
      },
      {
        id: "u2",
        speaker: "S1",
        origin: "system",
        start: 6,
        end: 14,
        text: "Let’s review the keeper release and the dictionary.",
        asrText: "Let’s review the keeper release and the dictionary.",
        edited: false,
        words: timed("Let’s review the keeper release and the dictionary.", 6, 14),
      },
      {
        id: "u3",
        speaker: "ME",
        origin: "microphone",
        start: 15,
        end: 24,
        text: "The recordings and voices stay in our drive.",
        asrText: "The recordings and voices stay in our drive.",
        edited: false,
        words: timed("The recordings and voices stay in our drive.", 15, 24),
      },
    ],
    dictionaryApplied: [],
    corrected: false,
  },
};
/** A two-part recording session with a camera beside each screen segment. */
export const SESSION_TRANSCRIPT_FIXTURE: TranscriptVm = (() => {
  const vm = structuredClone(TRANSCRIPT_FIXTURE);
  vm.path = "/Volumes/merope/tgdrive/recordings/2026-09-28 standup/transcript.json";
  vm.sourcePath = "/Volumes/merope/tgdrive/recordings/2026-09-28 standup";
  const tracks = TRANSCRIPT_FIXTURE.transcript.source.parts[0].tracks;
  vm.transcript.source = {
    kind: "recording",
    files: ["screen-0000.mov", "screen-0001.mov"],
    title: "2026-09-28 standup",
    parts: [
      { file: "screen-0000.mov", offset: 0, duration: 21, tracks },
      { file: "screen-0001.mov", offset: 21, duration: 21, tracks },
    ],
  };
  const line = (id: string, speaker: string, start: number, end: number, text: string) => ({
    id,
    speaker,
    origin: speaker === "ME" ? ("microphone" as const) : ("system" as const),
    start,
    end,
    text,
    asrText: text,
    edited: false,
    words: timed(text, start, end),
  });
  vm.transcript.utterances = [
    ...vm.transcript.utterances,
    line("u4", "S1", 21.5, 27, "The second segment starts here, after the pause."),
    line("u5", "ME", 28, 33, "Then we keep the camera on for the demo."),
    line("u6", "S1", 34, 40, "Search should find the keeper release again."),
  ];
  return vm;
})();
export const DICTIONARY_FIXTURE: DictionaryTermVm[] = [
  { id: "d1", text: "keeper", aliases: ["keaper"] },
  { id: "d2", text: "CoreML", aliases: ["core ml"] },
];

export function transcriptionMockHandlers(
  profiles: () => SyncProfileVm[],
): Record<string, (payload: Record<string, unknown>) => unknown> {
  let people: PersonVm[] = structuredClone(TRANSCRIPT_FIXTURE.people);
  let terms = structuredClone(DICTIONARY_FIXTURE);
  const transcripts = new Map<string, TranscriptVm>();
  const jobs = new Map<string, () => void>();
  const params = new URLSearchParams(window.location.search);
  const status: TranscriptionStatusVm = {
    available: true,
    reason: null,
    models: {
      state: "ready",
      sentence: "Models are ready. Transcription runs on this Mac.",
      missing: [],
    },
    language: "auto",
    afterRecording: true,
    asrModel: "",
    diarizationModel: "",
    voicesDrives: [
      {
        profileId: "p1",
        name: "tgdrive",
        voicesRoot: "/Volumes/merope/tgdrive/70-comms/voices",
        subfolder: "70-comms/voices",
        localPath: "/Volumes/merope/tgdrive",
      },
    ],
  };
  const modelState = params.get("models");
  if (
    modelState === "missing" ||
    modelState === "failed" ||
    modelState === "fetching" ||
    modelState === "noAccount"
  )
    status.models = {
      state: modelState,
      sentence:
        modelState === "noAccount"
          ? "Connect an account to fetch transcription models."
          : modelState === "fetching"
            ? "Fetching models from your config repository…"
            : modelState === "failed"
              ? "The model fetch failed. Try again when the config repository is available."
              : "The transcription models have not been fetched yet.",
      missing:
        modelState === "missing"
          ? ["parakeet-tdt-0.6b-v3/Encoder.mlmodelc/weights/weight.bin"]
          : [],
    };
  if (params.has("noVoices")) status.voicesDrives = [];
  const read = (payload: Record<string, unknown>) => {
    const path = String(payload.path);
    let vm = transcripts.get(path);
    if (!vm) {
      // A recording session's transcript sits in the session folder as
      // `transcript.json`; a file's beside it as `<file>.transcript.json`.
      vm = structuredClone(
        path.endsWith("/transcript.json") ? SESSION_TRANSCRIPT_FIXTURE : TRANSCRIPT_FIXTURE,
      );
      vm.path = path;
      const speakerStatus = params.get("speakerStatus");
      if (
        speakerStatus === "auto" ||
        speakerStatus === "confirmed" ||
        speakerStatus === "unknown"
      ) {
        vm.transcript.speakers[1].status = speakerStatus;
        vm.transcript.speakers[1].name = speakerStatus === "unknown" ? null : "Anna Kowalski";
        vm.transcript.speakers[1].score = speakerStatus === "unknown" ? null : 0.81;
      }
      if (params.has("emptyTranscript")) vm.transcript.utterances = [];
      if (params.has("longTranscript")) {
        vm.transcript.utterances = Array.from({ length: 1000 }, (_, i) => ({
          ...TRANSCRIPT_FIXTURE.transcript.utterances[i % 3],
          id: `u${i + 1}`,
          start: i * 5,
          end: i * 5 + 4,
        }));
        // The recording as long as its lines, so the scrub can reach them all.
        const parts = vm.transcript.source.parts;
        const each = 5000 / parts.length;
        vm.transcript.source.parts = parts.map((part, i) => ({
          ...part,
          offset: i * each,
          duration: each,
        }));
        vm.transcript.duration = 5000;
      }
      transcripts.set(path, vm);
    }
    vm.people = structuredClone(people);
    return vm;
  };
  const editPerson = (payload: Record<string, unknown>, change: (person: PersonVm) => PersonVm) => {
    people = people.map((p) => (p.id === payload.personId ? change(p) : p));
    return structuredClone(people);
  };
  // `CorrectionError::CrossOrigin`, verbatim.
  const refuseCrossOrigin = (vm: TranscriptVm, from: string, into: string) => {
    const origin = (id: string) => vm.transcript.speakers.find((s) => s.id === id)?.origin;
    if ((origin(from) === "microphone") !== (origin(into) === "microphone"))
      throw new Error(
        "A line heard on your microphone cannot move to a voice from the call, or back.",
      );
  };
  // `u<highest numeric suffix + 1>`, as Rust numbers them, so an id is never reused.
  const nextUtteranceId = (vm: TranscriptVm) =>
    `u${Math.max(0, ...vm.transcript.utterances.map((u) => Number(u.id.slice(1)) || 0)) + 1}`;
  return {
    transcription_status: () => {
      status.voicesDrives = params.has("noVoices")
        ? []
        : profiles()
            .filter((p) => p.enabled && p.voices)
            .map((p) => ({
              profileId: p.id,
              name: p.name,
              voicesRoot: `${p.localPath}/${p.voicesSubfolder}`,
              subfolder: p.voicesSubfolder,
              localPath: p.localPath,
            }));
      return structuredClone(status);
    },
    transcription_models_fetch: () => {
      status.models = {
        state: "ready",
        sentence: "Models are ready. Transcription runs on this Mac.",
        missing: [],
      };
      return structuredClone(status);
    },
    transcription_settings_set: (p) => {
      if (p.language != null) status.language = p.language as TranscriptionStatusVm["language"];
      if (p.afterRecording != null) status.afterRecording = Boolean(p.afterRecording);
      if (p.asrModel != null) status.asrModel = String(p.asrModel);
      if (p.diarizationModel != null) status.diarizationModel = String(p.diarizationModel);
      return structuredClone(status);
    },
    transcription_models_available: () => ({
      asr: [
        { id: "parakeet-tdt-0.6b-v3", complete: true },
        { id: "parakeet-tdt-0.6b-v4", complete: false },
      ],
      diarization: [{ id: "speaker-diarization", complete: true }],
      defaults: { asr: "parakeet-tdt-0.6b-v3", diarization: "speaker-diarization" },
    }),
    transcription_start: (p) => {
      const jobId = crypto.randomUUID();
      const channel = p.channel as { onmessage: (progress: TranscriptionProgressVm) => void };
      const path = String(p.path);
      const started = Date.now();
      let timer = 0;
      const send = (
        phase: TranscriptionProgressVm["phase"],
        fraction: number | null,
        message: string | null = null,
        replaceable = false,
      ) =>
        channel.onmessage({
          jobId,
          phase,
          part: phase === "queued" ? 0 : 1,
          parts: 1,
          message,
          transcriptPath:
            phase === "done"
              ? /\.[^./]+$/.test(path)
                ? `${path}.transcript.json`
                : `${path}/transcript.json`
              : null,
          fraction,
          elapsedMs: Date.now() - started,
          replaceable,
        });
      const stop = () => {
        window.clearInterval(timer);
        jobs.delete(jobId);
      };
      jobs.set(jobId, () => {
        stop();
        send("cancelled", null);
      });
      send("queued", null);
      // The shell's phase weights (decode 5 %, recognition 60 %, speakers 25 %,
      // matching and writing 10 %) over a job that takes eight seconds here, with
      // a heartbeat twice a second so a bar can be watched filling.
      const phases: [TranscriptionProgressVm["phase"], number][] = [
        ["decoding", 0.05],
        ["transcribing", 0.6],
        ["diarizing", 0.25],
        ["matching", 0.05],
        ["writing", 0.05],
      ];
      const JOB_MS = 8_000;
      const tick = () => {
        const fraction = Math.min(1, (Date.now() - started) / JOB_MS);
        // `?correctedTranscript`: the transcript already there holds corrections,
        // so only a Transcribe again (`replace`) gets past it, as in Rust.
        if (params.has("correctedTranscript") && p.replace !== true) {
          stop();
          send(
            "failed",
            null,
            "This transcript has corrections in it, so keeper will not overwrite it on its own. Transcribe again to replace it.",
            true,
          );
          return;
        }
        if (params.has("transcriptionFailure") || path.includes("master-2026-04")) {
          stop();
          send(
            "failed",
            null,
            "The media is not here yet. Fetch this file before transcribing it.",
          );
          return;
        }
        if (fraction >= 1) {
          stop();
          send("done", 1);
          return;
        }
        let phase: TranscriptionProgressVm["phase"] = "writing";
        let reached = 0;
        for (const [name, weight] of phases) {
          reached += weight;
          if (reached > fraction) {
            phase = name;
            break;
          }
        }
        send(phase, fraction);
      };
      timer = window.setInterval(tick, 500);
      return jobId;
    },
    transcription_cancel: (p) => jobs.get(String(p.jobId))?.(),
    transcript_read: (p) => structuredClone(read(p)),
    transcript_edit_utterance: (p) => {
      const vm = read(p);
      const u = vm.transcript.utterances.find((u) => u.id === p.utteranceId);
      const before = u?.text ?? "";
      if (u) {
        u.text = String(p.text);
        u.words = timed(u.text, u.start, u.end);
        u.edited = true;
        vm.transcript.corrected = true;
      }
      const oldWords = before.split(/\s+/);
      const newWords = String(p.text).split(/\s+/);
      const suggestions =
        oldWords.length === newWords.length
          ? oldWords.flatMap((from, i) =>
              from !== newWords[i] ? [{ from, to: newWords[i] ?? "" }] : [],
            )
          : [];
      return { transcript: structuredClone(vm), suggestions };
    },
    transcript_reassign_utterance: (p) => {
      const vm = read(p);
      const u = vm.transcript.utterances.find((u) => u.id === p.utteranceId);
      if (u) {
        refuseCrossOrigin(vm, u.speaker, String(p.speakerId));
        u.speaker = String(p.speakerId);
        vm.transcript.corrected = true;
      }
      return structuredClone(vm);
    },
    transcript_split_utterance: (p) => {
      const vm = read(p);
      const utterances = vm.transcript.utterances;
      const at = utterances.findIndex((u) => u.id === p.utteranceId);
      const u = utterances[at];
      if (!u) throw new Error(`That line is no longer in the transcript (${p.utteranceId}).`);
      const index = Number(p.wordIndex);
      if (!Number.isInteger(index) || index < 1 || index >= u.words.length)
        throw new Error("A line splits between two of its words.");
      const head = u.words.slice(0, index);
      const tail = u.words.slice(index);
      const join = (words: Word[]) => words.map((w) => w.text).join(" ");
      const asr = u.asrText.split(/\s+/).filter(Boolean);
      const second = {
        ...u,
        id: nextUtteranceId(vm),
        start: tail[0].start,
        end: tail[tail.length - 1].end,
        text: join(tail),
        asrText: u.edited ? join(tail) : asr.slice(index).join(" "),
        words: tail,
      };
      Object.assign(u, {
        end: head[head.length - 1].end,
        text: join(head),
        asrText: u.edited ? u.asrText : asr.slice(0, index).join(" "),
        words: head,
      });
      utterances.splice(at + 1, 0, second);
      vm.transcript.corrected = true;
      return structuredClone(vm);
    },
    transcript_insert_utterance: (p) => {
      const vm = read(p);
      const utterances = vm.transcript.utterances;
      const at = utterances.findIndex((u) => u.id === p.afterId);
      const after = utterances[at];
      if (!after) throw new Error(`That line is no longer in the transcript (${p.afterId}).`);
      const speaker = vm.transcript.speakers.find((s) => s.id === p.speakerId);
      if (!speaker)
        throw new Error(`That speaker is no longer in the transcript (${p.speakerId}).`);
      const text = String(p.text).trim();
      if (!text) throw new Error("A line cannot be emptied; reassign it instead.");
      const next = utterances[at + 1];
      const time = next ? Math.min(after.end, next.start) : after.end;
      utterances.splice(at + 1, 0, {
        id: nextUtteranceId(vm),
        speaker: speaker.id,
        origin: speaker.origin,
        start: time,
        end: time,
        text,
        asrText: "",
        edited: true,
        words: [],
      });
      vm.transcript.corrected = true;
      return structuredClone(vm);
    },
    transcript_rename_speaker: (p) => {
      const vm = read(p);
      const s = vm.transcript.speakers.find((s) => s.id === p.speakerId);
      if (s) s.name = String(p.label);
      vm.transcript.corrected = true;
      return structuredClone(vm);
    },
    transcript_merge_speakers: (p) => {
      const vm = read(p);
      refuseCrossOrigin(vm, String(p.fromId), String(p.intoId));
      vm.transcript.corrected = true;
      vm.transcript.speakers = vm.transcript.speakers.filter((s) => s.id !== p.fromId);
      for (const u of vm.transcript.utterances)
        if (u.speaker === p.fromId) u.speaker = String(p.intoId);
      return structuredClone(vm);
    },
    transcript_assign_speaker: (p) => {
      if (!status.voicesDrives.length)
        throw new Error(
          "Choose a folder that keeps voices in Settings → Sync before assigning a speaker.",
        );
      const vm = read(p);
      let person = people.find((person) => person.id === p.personId);
      if (!person && p.newName) {
        person = {
          id: crypto.randomUUID(),
          name: String(p.newName),
          aliases: [],
          isSelf: false,
          samples: 0,
          hasEmbeddingForModel: true,
        };
        people.push(person);
      }
      const speaker = vm.transcript.speakers.find((s) => s.id === p.speakerId);
      if (speaker && person) {
        vm.transcript.corrected = true;
        person.samples++;
        // One person on one track is one speaker: a same-origin speaker that
        // already carries them and has lines takes this one's lines, and this
        // one is removed rather than left behind naming the person twice.
        const into = vm.transcript.speakers.find(
          (s) =>
            s !== speaker &&
            s.personId === person.id &&
            s.origin === speaker.origin &&
            vm.transcript.utterances.some((u) => u.speaker === s.id),
        );
        const confirmed = into ?? speaker;
        confirmed.personId = person.id;
        confirmed.name = person.name;
        confirmed.status = "confirmed";
        if (into) {
          for (const u of vm.transcript.utterances)
            if (u.speaker === speaker.id) u.speaker = into.id;
          vm.transcript.speakers = vm.transcript.speakers.filter((s) => s !== speaker);
        }
      }
      vm.people = structuredClone(people);
      return structuredClone(vm);
    },
    transcript_add_speaker: (p) => {
      const vm = read(p);
      const next =
        Math.max(0, ...vm.transcript.speakers.map((s) => Number(s.id.slice(1)) || 0)) + 1;
      const label = String(p.label ?? "").trim();
      vm.transcript.speakers.push({
        id: `S${next}`,
        origin: p.origin as TrackOrigin,
        personId: null,
        name: label || null,
        status: "unknown",
        score: null,
        candidates: [],
        embedding: null,
        clip: null,
      });
      vm.transcript.corrected = true;
      return structuredClone(vm);
    },
    transcript_media: (p): TranscriptMediaVm => {
      const vm = read(p);
      const folder = String(p.path)
        .replace(/\/[^/]*$/, "")
        .replace(/^\/Volumes\/merope\/tgdrive\/?|^\//, "");
      const ref = (file: string, kind: MediaRef["kind"]): MediaRef => ({
        via: "file",
        profileId: "p1",
        relativePath: folder ? `${folder}/${file}` : file,
        kind,
      });
      const session =
        vm.transcript.source.kind === "recording" && vm.transcript.source.parts.length > 1;
      const parts = vm.transcript.source.parts.map((part) => ({
        file: part.file,
        offset: part.offset,
        duration: part.duration,
        screen: ref(part.file, /\.(m4a|wav|mp3)$/.test(part.file) ? "audio" : "video"),
        camera: session ? ref(part.file.replace(/^screen-/, "camera-"), "video") : null,
        here: true,
        audioTracks: part.tracks.flatMap((track) =>
          track.track === null ? [] : [{ index: track.track, origin: track.origin }],
        ),
      }));
      return {
        parts,
        hasCamera: parts.some((part) => part.camera !== null),
        hasScreen: parts.some((part) => part.screen.kind === "video"),
      };
    },
    transcript_clip: (p): MediaClipVm => {
      const vm = read(p);
      // Rust's refusal reaches the webview as the IpcError envelope, its sentence the message.
      const refuse = (message: string): never => {
        throw { code: "invalidInput", message, accountId: null, retriable: false };
      };
      const seconds = (value: unknown, name: string): number | null => {
        if (value === null || value === undefined) return null;
        const match = /^(\d{2}):([0-5]\d):([0-5]\d)$/.exec(String(value));
        if (!match) return refuse(`${name} is not a time keeper can read. Write it as hh:mm:ss.`);
        return Number(match[1]) * 3600 + Number(match[2]) * 60 + Number(match[3]);
      };
      const duration = vm.transcript.duration;
      const from = seconds(p.from, "From");
      const to = seconds(p.to, "To");
      if (from !== null && to !== null && from >= to) refuse("A clip ends after it starts.");
      if ((from ?? 0) > duration || (to ?? 0) > duration)
        refuse("That time is past the end of the meeting.");
      const lo = from ?? 0;
      const hi = to ?? duration;
      const lines = vm.transcript.utterances.filter((u) => u.end > lo && u.start < hi);
      const clock = (s: number) =>
        [Math.floor(s / 3600), Math.floor((s / 60) % 60), Math.floor(s % 60)]
          .map((n) => String(n).padStart(2, "0"))
          .join(":");
      const source =
        vm.transcript.source.kind === "recording"
          ? 'session = "01J8MOCKSESSION-01J8MOCKSESSION"'
          : `transcript = "${String(p.path).replace(/^\/Volumes\/merope\/tgdrive\//, "")}"`;
      const fence = [
        "```keeper-media",
        source,
        ...(from !== null ? [`from = "${clock(from)}"`] : []),
        ...(to !== null ? [`to = "${clock(to)}"`] : []),
        "```",
      ];
      const names = new Map(vm.transcript.speakers.map((s) => [s.id, s.name ?? s.id]));
      const words = p.words
        ? [
            `> [!transcript]- ${vm.transcript.source.title ?? vm.transcript.source.files[0]} · ${clock(lo)}–${clock(hi)}`,
            ...lines.map((u) => `> **${names.get(u.speaker)}** ${clock(u.start)} ${u.text}`),
          ]
        : [];
      return { markdown: `${[...fence, ...words].join("\n")}\n`, lines: lines.length };
    },
    voices_people: () => structuredClone(people),
    voices_person_rename: (p) => editPerson(p, (person) => ({ ...person, name: String(p.name) })),
    voices_person_set_self: (p) => {
      people = people.map((person) => ({ ...person, isSelf: person.id === p.personId }));
      return structuredClone(people);
    },
    voices_person_delete: (p) => {
      people = people.filter((person) => person.id !== p.personId);
      return structuredClone(people);
    },
    voices_people_merge: (p) => {
      const from = people.find((person) => person.id === p.fromId);
      people = people
        .filter((person) => person.id !== p.fromId)
        .map((person) =>
          person.id === p.intoId
            ? {
                ...person,
                samples: person.samples + (from?.samples ?? 0),
                isSelf: person.isSelf || (from?.isSelf ?? false),
              }
            : person,
        );
      return structuredClone(people);
    },
    dictionary_terms: () => structuredClone(terms),
    dictionary_term_save: (p) => {
      const term = {
        id: p.id ? String(p.id) : crypto.randomUUID(),
        text: String(p.text),
        aliases: p.aliases as string[],
      };
      terms = [...terms.filter((t) => t.id !== term.id), term];
      return structuredClone(terms);
    },
    dictionary_term_delete: (p) => {
      terms = terms.filter((t) => t.id !== p.id);
      return structuredClone(terms);
    },
    dictionary_accept_suggestion: (p) => {
      terms.push({ id: crypto.randomUUID(), text: String(p.to), aliases: [String(p.from)] });
      return structuredClone(terms);
    },
  };
}

import type {
  DictionaryTermVm,
  PersonVm,
  SyncProfileVm,
  TranscriptionProgressVm,
  TranscriptionStatusVm,
  TranscriptVm,
} from "@/lib/ipc/client";

export const TRANSCRIPT_FIXTURE: TranscriptVm = {
  path: "/Volumes/merope/tgdrive/meeting.mov.transcript.json",
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
        words: [],
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
        words: [],
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
        words: [],
      },
    ],
    dictionaryApplied: [],
    corrected: false,
  },
};
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
      vm = structuredClone(TRANSCRIPT_FIXTURE);
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
      if (params.has("longTranscript"))
        vm.transcript.utterances = Array.from({ length: 1000 }, (_, i) => ({
          ...TRANSCRIPT_FIXTURE.transcript.utterances[i % 3],
          id: `u${i + 1}`,
          start: i * 5,
          end: i * 5 + 4,
        }));
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
      return structuredClone(status);
    },
    transcription_start: (p) => {
      const jobId = crypto.randomUUID();
      const channel = p.channel as { onmessage: (progress: TranscriptionProgressVm) => void };
      const path = String(p.path);
      let stopped = false;
      const send = (phase: TranscriptionProgressVm["phase"], message: string | null = null) =>
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
        });
      jobs.set(jobId, () => {
        stopped = true;
        send("cancelled");
        jobs.delete(jobId);
      });
      send("queued");
      const phases: TranscriptionProgressVm["phase"][] = [
        "decoding",
        "transcribing",
        "diarizing",
        "matching",
        "writing",
        "done",
      ];
      phases.forEach((phase, i) => {
        window.setTimeout(
          () => {
            if (stopped) return;
            if (
              i === 0 &&
              (params.has("transcriptionFailure") || path.includes("master-2026-04"))
            ) {
              stopped = true;
              send("failed", "The media is not here yet. Fetch this file before transcribing it.");
              jobs.delete(jobId);
              return;
            }
            send(phase);
            if (phase === "done") jobs.delete(jobId);
          },
          (i + 1) * 600,
        );
      });
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
        speaker.personId = person.id;
        speaker.name = person.name;
        speaker.status = "confirmed";
        vm.transcript.corrected = true;
        person.samples++;
      }
      vm.people = structuredClone(people);
      return structuredClone(vm);
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

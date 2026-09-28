# Transcription

keeper turns meetings and any audio or video file into transcripts with speakers, recognises
people it has heard before from a **voices bank** kept in a drive, and lets you correct both the
words and the people. It all happens **in keeper, on this Mac**: no audio, transcript or voice
leaves it for this, there is no transcription server, no NAS option and no cloud fallback
(D-29, `docs/egress.md` § *Transcription adds no egress*).

## Where it runs

- **An Apple Silicon Mac with macOS 15 or later.** The engine is FluidAudio (Parakeet for
  speech, pyannote community-1 for speakers) on the Neural Engine, through the vendored bridge
  at `tools/fluidaudio-rs/`. macOS 14 is not enough: FluidAudio's community-1 diarizer crashes
  there (upstream issue #878). keeper itself needs macOS 14 or later since Epic 87, the floor
  of FluidAudio's statically linked Swift library.
- **Nowhere else yet.** On an Intel Mac, older macOS, Linux, Windows and the phone the
  capability is off and the surfaces are absent. Transcripts, the bank and the dictionary are
  files, so they still sync to, and can be read on, every device.
- **With models present.** Settings › Transcription shows their state: ready, missing (with
  the files it lacks), fetching, failed, or "no account" — models come only from an account's
  config repository (below). *Ready* means every file is here **and** the set is the one the
  config repository names now. A complete set the repository has since replaced reads as
  missing, with "The transcription models on this Mac are not the ones your account holds now;
  keeper brings them up to date after the next sync." A ready set stays ready while the fetch
  each sync starts checks it again.

## What you see

- **Settings › Transcription:** the models' state and *Fetch models*; the language (*Auto*,
  English, Polish — `transcription.language`); *Transcribe after recording*
  (`transcription.after_recording`, on by default); for each drive that keeps voices, its people
  (rename, mark as me, merge, delete) and its dictionary; and *Transcribe a file…*.
- **Settings › Sync, a folder's form:** *This folder keeps voices* and its subfolder (default
  `voices`), like a recordings folder. A drive that keeps voices is a *transcribing drive*.
  The owner's tgdrive and neuradrive use `70-comms/voices`.
- **Files:** the voices folder carries its own glyph. *Transcribe* is offered on a media file
  whose content is on this Mac, and on a recording session folder whose audio segments all
  are; an LFS pointer or a virtual file is fetched first. Media files are `mov`, `mp4`, `m4a`,
  `mp3`, `wav`, `aac`, `flac`, `m4v`, `caf`, `aiff` and `aif`, what AVFoundation decodes;
  WebM, Matroska, Ogg and Opus files are not offered. An entry that already has a transcript
  offers *Open transcript* even where this device cannot transcribe: reading one needs no
  engine. When a job ends the listing refreshes and the job's progress line offers *Open
  transcript*; the viewer does not open by itself. Opening a `transcript.json` or
  `*.transcript.json` opens the transcript viewer, and one it cannot read opens in the text
  viewer instead.
- **Recording:** see `docs/recording.md` § *Transcription* — after-recording transcription,
  and the microphone track as you.
- **The transcript viewer:** speakers with how each was matched and the candidates; assign a
  speaker to a person (existing or new), rename or merge speakers; each line editable, and
  reassignable to another speaker; dictionary suggestions after an edit; progress while a job
  runs.

## Models live in the config repository

keeper's bundle carries no model weights (D-5). The organisation that runs the account
distributes them through its config repository (`docs/account.md` § *The config repository*),
under `_models/`, tracked by git LFS. keeper's clone of that repository holds LFS pointers. On
a Mac that can transcribe, after each config sync and on *Fetch models*, keeper hydrates
`_models/` into `<data_dir>/models/` with its own LFS client and the account's credential,
checking every object against its sha256 and size. The LFS endpoint is derived from the
repository's own URL and nothing else: a `.lfsconfig` in the config repository is ignored, so
the credential never goes to a host the repository names. A file already in place is not
fetched again.

A set is complete, or it is not used. keeper removes
`<data_dir>/models/.keeper-models-complete.json` before it changes anything there and writes it
last, only when every file is in place; it records a digest of what every file holds (its LFS
object id, or a plain file's sha256). The set is ready only when every file it needs is present
and that digest matches what the clone names now, so a half-updated set (a new encoder beside
an old decoder) is never loaded. A changed set is loaded at the next job's start, never in the
middle of one. The engine loads a set only by path — it never downloads, and keeper never
contacts Hugging Face.

### Layout

```text
keeper-config.git/
  .gitattributes                 # _models/** filter=lfs diff=lfs merge=lfs -text
  _models/
    models.toml                  # which folders form the set
    LICENSE, NOTICE              # the models' licence and attribution files
    parakeet-tdt-0.6b-v3/        # speech recognition
      Preprocessor.mlmodelc/     # each .mlmodelc needs coremldata.bin, model.mil,
      Encoder.mlmodelc/          #   weights/weight.bin
      Decoder.mlmodelc/
      JointDecisionv3.mlmodelc/
      parakeet_vocab.json
    speaker-diarization/         # diarization and 256-d speaker embeddings
      Segmentation.mlmodelc/
      FBank.mlmodelc/
      Embedding.mlmodelc/
      PldaRho.mlmodelc/
      plda-parameters.json
```

The `.mlmodelc` folders are precompiled Core ML models; nothing is compiled on the Mac. Their
sources are FluidInference's `parakeet-tdt-0.6b-v3-coreml` (int8 encoder) and
`speaker-diarization-coreml`, both CC-BY-4.0; community-1's files carry a NOTICE requiring
attribution to pyannote, WeSpeaker, BUT Speech@FIT and Fluid Inference, so copy the licence
and NOTICE files into `_models/` beside them (DW-341).

`models.toml` names the set; every section is optional and falls back to the default shown:

```toml
[asr]
dir = "parakeet-tdt-0.6b-v3"

[diarizer]
dir = "speaker-diarization"

[embedding]
id = "pyannote-community-1"      # the voices bank's embeddings/<id>/ folder
```

Each value must be one plain folder name (no `/`, `..` or `:`). An unknown key is refused.

### Adding or upgrading a model set

1. Put the new folders under `_models/` next to the old ones (a new folder name, e.g.
   `parakeet-tdt-0.6b-v4/`), with the `.gitattributes` rule above in place **before** the
   first `git add`, so the files go to LFS and not into git history. Add their licence files.
2. Point `models.toml` at them, commit and push. Every signed-in Mac that can transcribe
   hydrates the new files on its next sync; files the set no longer names stay in
   `<data_dir>/models/` and are simply not loaded.
3. **Changing the embedding model** means a new `[embedding] id`. Bank vectors are kept per
   embedding model, under `embeddings/<id>/`, and a vector from one model is not comparable
   with another's. Voice clips are model-free, so nothing is lost: before matching, each
   transcription embeds every bank clip that has no vector for the current model and writes
   it under the new prefix. Two Macs embedding the same clip write the same file, so the sync
   sees no conflict. The old prefix stays, so a Mac still on the old set keeps matching.

Keep the diarizer and the embedding id together: the speaker embeddings come from the
diarizer's own `Embedding.mlmodelc`, so a diarizer change is an embedding-model change.

## The voices bank

A drive keeps voices in `<drive>/<voices subfolder>/`. Every fact is its own file, so two
devices adding at once sync as a union, never as a conflict:

```text
<voices subfolder>/
  people/<person-id>.json                    # {version, id, name, aliases, self, createdAt, updatedAt}
  clips/<person-id>/<clip-id>.wav            # 16 kHz mono PCM16, 2–15 s
  clips/<person-id>/<clip-id>.json           # {version, person, clip, source}: where a clip
                                             #  stored without an embedding was heard
  embeddings/<model-id>/<person-id>/<clip-id>.json
                                             # {version, model, person, clip, vector[256],
                                             #  source {transcript, start, end}, addedAt}
  dictionary/<term-id>.json                  # {version, id, text, aliases, createdAt}
  tombstones/<person-id>.json                # {id, deletedAt, mergedInto?}
```

Ids are ULIDs. At most one person is `self` — the voice on the microphone. Deleting a person
removes their person, clip and embedding files and writes a tombstone, so a device that still
has the old files does not bring them back; a person with a tombstone is ignored wherever they
still appear. Merging two people moves the clips and embeddings into one, keeps the other's
name as an alias, and tombstones it with `mergedInto` naming the survivor: whatever still
arrives under the old id, from a device that had not heard of the merge, counts as the
survivor's. keeper reads the bank tolerantly: an unreadable file is skipped with a warning,
never fatal, and a file a newer keeper wrote (`version` above 1) is skipped and never
rewritten.

**What writes to the bank.** Only a person's confirmation: assigning a speaker in a transcript
to a person (existing or new) stores that speaker's best clip — the longest span where only they
speak, among the lines heard on their own track, clamped to 2–15 s — and its embedding. While a
transcription is running the engine is busy, so the clip is stored alone, with where it was
heard beside it, and the next transcription embeds it. When no clip can be cut, nothing is
stored, and the person is still created and named in the transcript. The same span is never
stored twice: it is recognised by its transcript, start and end, whichever model embedded it,
and confirming it as someone else moves the clip and its embeddings to them, which is how a
wrong confirmation is corrected. Editing text never touches the bank. The bank's own editing
(rename, mark as me, merge, delete) writes what it says. A transcript's speakers, matched or
not, write nothing until confirmed.

## The dictionary

Names and jargon the recogniser gets wrong: each term is a spelling (`text`) and what the
recogniser writes instead (`aliases`). After recognition, every alias is replaced by the term's
text — whole words, any case, multi-word aliases included, longer aliases first — and the
transcript records each replacement and how often it fired (`dictionaryApplied`). A term with
punctuation of its own, such as `C++` or `.NET`, matches the whole token, and the punctuation
around it stays as it was. The dictionary does not bias recognition itself (DW-332).

When you edit a line and the edit swaps single words (not just case or punctuation), the viewer
offers each swap as a dictionary suggestion; accepting one saves a term. Nothing is added
without that click.

## Transcript files

Beside the media:
- a recording session folder gets `transcript.json` and `transcript.md`;
- any other media file `<name.ext>.transcript.json` and `<name.ext>.transcript.md`, the
  extension kept: `meeting.mp4` gets `meeting.mp4.transcript.json`, so `call.mov` and
  `call.m4a` never share one.

The JSON is the source of truth; the markdown is re-rendered from it on every save — a title,
the date and duration, a speaker legend, one `**[hh:mm:ss] Name:** text` line per utterance, and
a footer naming the models. Transcribing again over a transcript nobody has touched replaces it.
Over one anyone has corrected — an edited line, a confirmed speaker, a reassigned line, a merge
or a rename — it is refused with a sentence: delete or rename the transcript first. The check
runs again just before the job writes, so a correction made while it ran is kept.

The JSON (camelCase, pretty-printed, `version` 1; a file from a newer keeper is refused):

- `source` — `kind` (`recording` | `file`), `files` (relative to the transcript), and `parts`:
  per file its `offset` on the timeline, `duration`, and which audio `tracks` were heard as
  what (`system`, `microphone`, `mixed`; `track: null` for all tracks mixed);
- `createdAt`, `engine` (`asr`, `diarizer`, `embedding` ids), `language`, `duration` (s);
- `speakers` — `S1`, `S2`… for diarized voices and `ME` for the microphone, each with `origin`,
  `personId`/`name`, `status` (`auto`, `suggested`, `confirmed`, `unknown`, `self`), `score`, up
  to three `candidates`, its `embedding`, and its best `clip`;
- `utterances` — `u1`, `u2`… with `speaker`, `origin` (the track it was heard on), `start`,
  `end`, `text` (what the transcript says), `asrText` (what the recogniser said), `edited`, and
  timed `words`;
- `dictionaryApplied` — `{from, to, count}`;
- `corrected` — true once any correction was made.

Utterances break on a change of speaker, a silence over 1.5 s, or 40 words. A recording's
microphone track is transcribed on its own as `ME`. A microphone line is dropped as echo only
when it has at least four words, one system line covers at least half its time, and the system
speech around it says at least 60% of its words in the same order; a short reply, or one that
reuses the far end's words in another order, is kept. Other files mix every audio track and
diarize them all.

## Corrections

In the viewer:
- **Edit a line** — the text changes and `edited` is set; `asrText` keeps the original. Word
  timings are spread over the old span when the word count changes. Offers dictionary
  suggestions (above); never touches the bank.
- **Reassign a line** to another speaker, **merge** two speakers, or **rename** a speaker for
  this transcript only — the transcript changes, the bank does not. A speaker left with no lines
  stays in the transcript, hidden from the legend and still offered as a target, so the move
  can be undone. A line heard on your microphone cannot move to a voice from the call, or back,
  and those two speakers cannot be merged.
- **Assign a speaker to a person** (existing, or new by name) — the speaker becomes
  `confirmed`, and the bank gains that speaker's clip and embedding. This is the one correction
  that teaches keeper a voice.

Assignment needs a drive that keeps voices: the drive holding the media if it keeps voices,
otherwise the first enabled one. With none, speakers stay unknown and assignment says why.

Every correction marks the transcript `corrected`, so a later transcription will not replace it.

## Matching thresholds

Each speaker's embedding is compared (cosine) with each person's centroid — the normalised mean
of their vectors for the current embedding model:
- **≥ 0.70** — assigned automatically (`auto`);
- **≥ 0.50** — offered as a candidate (`suggested`), nobody assigned;
- below — `unknown`.

Speakers in different segment files of one recording are the same speaker when their centroids
reach **0.60**. These values are starting points, not measurements (DW-333): a wrong automatic
match costs a click, and nothing reaches the bank without confirmation.

## Known limits

- One diarizer, community-1; NVIDIA's Nemotron 3 is not used — it caps at eight speakers and
  has no embeddings (DW-331).
- The dictionary is applied after recognition; it does not bias it (DW-332).
- Thresholds are uncalibrated (DW-333).
- A session that ended while keeper was quitting, or before the models arrived, is not
  transcribed later by itself; use *Transcribe* in Files (DW-334).
- Apple Silicon with macOS 15 or later only (DW-335).
- Nothing is transcribed live, during a recording (DW-336).
- No summaries (DW-337).
- Transcripts are not in the recordings search (DW-338).
- keeper builds against a vendored fork of `fluidaudio-rs` (DW-339), and each engine call copies
  the audio twice more on its way in: a three-hour file transcribed from Files peaks near 2 GB
  (DW-343).
- The microphone is found as the recording's second audio track, the order `keeper-rec`
  writes; it carries no track labels (DW-340).
- The models' licences travel in `_models/`; one upstream provenance question is open
  (DW-341).
- The models' fetch reads the config clone outside the account's lock; a torn read costs one
  failed load, never a wrong model (DW-344).
- keeper needs macOS 14 or later since Epic 87. A Mac on macOS 11–13 that auto-updates would
  receive a bundle it cannot open (DW-345).
- Voice prints are personal data; consent and GDPR handling are deferred by the owner
  (DW-342).
- No translation.

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
- **Recording › Transcribe a File…** in the menu bar, the ⌘K palette and the ⌘? cheat sheet
  (registry id `transcription-transcribe-file`): the same native file picker as Settings ›
  Transcription, for any media file on this Mac. It exists only where transcription runs (the
  same probe as the Settings pane); its job shows progress in a toast that offers *Open
  transcript* when done. A file already in a drive is transcribed from its Files row.
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
- **Recordings:** each session row offers *Transcribe* when its audio is all on this Mac and it
  has stopped recording, and *Show transcript* when it has one; the pane header and the
  Recording pane offer *Transcribe a file…*.
- **Progress:** a running job reports a determinate bar — every second, the share of the job
  done (an estimate, never going backwards) and the time it has run — wherever it was started:
  the job strip, the *Transcribe a file…* toast, the Files row and the Recordings row. The
  estimate weighs each part by its audio time (times its tracks) and each step by a fixed share
  (decoding 5 %, speech 60 %, speakers 25 %, matching and writing 10 %); how far into a step a
  job is comes from the time it has spent there against the time that much audio is expected to
  take on an M-series Mac (decoding 0.001 s, speech 0.02 s, speakers 0.006 s per second of
  audio), and a step never claims more than 95 % of itself until it ends.
- **The transcript viewer** takes most of the window (in the Files panel, the whole panel); the
  lines are exactly as wide as the player — one content column with a 12 px gutter, no inset
  of the lines' own beyond the speaker's square, and no reading-measure cap anywhere (an
  edited line's field included).
  - **The header** is the transcript's `source.title` (the session folder or the file's
    name), or nothing; the list of media files is not a title. Search, *Transcribe again…*
    and the ⋯ menu sit above the scroll area. Inside it, in order: the player with its
    controls; one row under them, wrapping on a narrow window, with the date · length ·
    language · engine on the left and "Part 1 of 2 · screen-0000.mov" on the right; the
    speakers' chips; then the lines.
  - *Transcript* and *Source* tabs. Source shows the file's JSON read-only in the Files text
    editor, laid out as keeper writes it and inset by the pane's padding; the transcript stays
    mounted underneath, so playback goes on.
  - **Read mode first.** The transcript reads as a document. A line starts
    "■▶ Name 00:02:12 · 21 s ⋯": the speaker's coloured square is a play button ("Play from
    00:02:12") with a small ▶, faint until hovered or focused; the time is a button that moves
    the player there without changing play or pause; the length is whole seconds, and from a
    minute on "1 min 05 s"; the words follow, large, and clicking them only selects them.
    Editing stays out of the way in the ⋯ right after the time — always visible, faint until
    hovered or focused, and always in the tab order: *Play from here* (when the media can
    play), *Copy clip from here…*, *Edit text*, *Change speaker* (a submenu of speakers to pick
    one from), *Split…*, *Add a line after*.
  - **Copy a clip.** A line's *Copy clip from here…* (prefilled with the line's start and end)
    and the header's *Copy as note embed* (the whole transcript) open *Copy a clip*: *From* and
    *To* as `hh:mm:ss`, checked as you type, the number of lines in the window, and *Include
    the words, for Obsidian and other apps* (on by default). *Copy* puts a `keeper-media` block
    on the clipboard and says "Copied. Paste it into any note." (§ *A transcript in a note*).
  - **Speakers** are a row of chips — name, a dot for how each was matched, and its line count.
    A chip opens a menu: how it was matched (and its score), *Go to their nearest line* (the
    speaker's line nearest the player's time, the later one on a tie; it scrolls there and
    seeks, and a paused player stays paused), *Go to their next line* (their first line
    starting after the player's time, wrapping to their first; the same scroll and seek),
    *This is…* (a submenu of people, the candidates
    first with their scores, e.g. "suggested 0.63", then *New person…*), *Rename label…* and
    *Merge into…*. *New person…* and *Rename label…* open a field under the
    chip row. *Add speaker* at the row's end adds a voice the diarizer did not tell apart, with
    an optional label; a track choice (call or microphone) appears only when the transcript
    heard both. Lines then move to it with *Change speaker*. The row lists only speakers with
    lines; the per-line and add-a-line speaker menus list each person once (a lineless speaker
    whose person another speaker with lines carries is not offered; an unnamed or unique
    lineless one is). Dictionary suggestions follow an edit.
  - A **player**, when the transcript's media can be served, plays it as one timeline across a
    session's segments: scrubbing, ±10 s and seeks map the transcript's time to a part and a
    time in it, and the next part starts where one ends; the row under the controls names
    the part ("Part 2 of 3 · screen-0001.mov"). Screen and camera play side by side, with a *Picture*
    choice of one icon out of three (screen, camera, both) only when the session filmed both;
    a *Sound* choice (call, microphone, both) appears only when a part has two audio tracks.
    Every icon control names itself to a screen reader and in a tooltip. A video sits in a
    16:9 box (at most 30 % of the window's height) and shows its first frame before it plays.
    Media is served over `keeper-file://` from the synced folder that holds it, or over
    `keeper-recording://` for a recording this Mac's index knows outside every synced folder;
    a part served by neither says "This part is in no synced folder and among no recordings, so
    keeper cannot play it here.", and a part whose bytes are not here yet says "This part is not
    on this Mac yet. Once the sync brings it, it plays here." and mounts no video. A part whose
    length is not recorded takes it from the file. Media elements are released on a part
    change and on close.
  - *Pin* (on by default) keeps the player, its row and the speakers' chips at the top of the
    scroll area while the lines scroll.
    *Follow* (on by default) highlights the line being said and scrolls it into view while
    playing, while the scrub bar is dragged (not only when it is let go), on ±10 s and on
    every seek; scrolling by hand pauses the follow-scroll until the next play or seek. Every
    player toggle — *Picture*, *Sound*, *Pin*, *Follow* — shows its state: on is accent ink
    and edge on a lifted fill, off a quiet outline, and the tooltip says so ("Follow the
    transcript: on").
  - An open viewer reads its transcript again when `keeper://transcript-written` names it — a
    job's result, or a correction made elsewhere.
  - **Transcribe again…** in the header replaces this transcript (below, *Transcript files*);
    when the job ends, the viewer reads the new transcript.
  - **Search** (⌘F / Ctrl+F in the viewer) finds text in lines and speaker names, ignoring
    case: matches are marked, a count reads "N matches", "2 of 7" or "No matches", Enter and
    Shift+Enter (or the ↑/↓ buttons) go to the next and previous match and wrap, and a jump
    seeks the player when Follow is on.

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

A drive keeps voices in `<drive>/<voices subfolder>/` (the owner's drives:
`70-comms/voices/`). This folder is everything keeper knows about who someone is — the people,
their voice clips, the embeddings recognised against, the dictionary and the tombstones; the
transcripts only point at it. Every fact is its own file, so two devices adding at once sync as a
union, never as a conflict:

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
the date and duration, a speaker legend (only the speakers with at least one line, as in the
viewer), one `**[hh:mm:ss] Name:** text` line per utterance, and a footer naming the models.
Transcribing again over a transcript nobody has touched replaces it.
Over one anyone has corrected — an edited line, a confirmed speaker, a reassigned line, a merge,
a rename, a split or an added line — or one keeper cannot read, the job stops with a sentence
and keeper leaves the file alone; the check runs again just before the job writes, so a
correction made while it ran is kept.

**Transcribe again…** — on the Files row (the phone's too), the Recordings row, the viewer's
header and the job strip that stopped at a corrected transcript — replaces it on purpose. It asks first: "Replace
the transcript? Your corrections in it will be lost. The voices bank keeps everything you
confirmed." Then the job runs with `replace` (`transcription_start`'s argument, false by
default) and skips both checks: the old `.json` and `.md` are deleted right before the new ones
are written, not when the job starts, so a job that fails or is cancelled keeps the old
transcript. Nothing in the voices bank is touched. The viewer finds what to transcribe from the
transcript's own name (`TranscriptVm.sourcePath`: the session folder beside `manifest.json`, or
the media file); when that is gone, or this Mac cannot transcribe, the header does not offer
*Transcribe again…*.

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
microphone track is transcribed on its own and diarized like the system track, because
sometimes two people, rarely more, share the microphone. Its voice that matches the bank's
`self` person (cosine ≥ 0.50) is `ME`; with no `self` person, or none matching, the voice that
talks longest is. Every other voice on the microphone is an ordinary numbered speaker (`S1`,
`S2`…, numbered with the call's by first appearance) with `origin` `microphone`, matched against
the bank like any speaker. `ME` carries its voice's embedding. A microphone line — `ME`'s or
another voice's — is dropped as echo only when it has at least four words, one system line
covers at least half its time, and the system speech around it says at least 60% of its words
in the same order; a short reply, or one that reuses the far end's words in another order, is
kept. A voice left with no line after that is no speaker. Other files mix every audio track and
diarize them all.

## A transcript in a note

A note plays a recording or a transcript in a `keeper-media` block (`docs/notes.md` §
*Media in a note*, D-30), and the block is this viewer — the same component, with every
correction, the speakers' menus, search, *Transcribe again…* and *Copy clip*. What differs:
it shows only the block's window (`from`/`to`) and its markers, it has no dialog chrome and
no *Transcript*/*Source* tabs, its heading is the block's `title` or nothing, and it has no
scroll box of its own: the note's editor scrolls it, the lines are windowed against that
scroller, and the pinned player sticks to the top of the note's scroll area. Reading needs
no engine, so it draws wherever keeper draws notes; *Transcribe* needs this Mac's
transcription.

- **`keeper://transcript-written`**, with `{path}` — the transcript's absolute path — follows
  every transcript the shell writes: a job's result, every correction and assignment, and a
  redo's fresh file. An open block or viewer showing that path reads it again; nothing else
  does, and no note is rewritten.
- **From the viewer.** *Copy as note embed* copies a block for the whole transcript; a line's
  *Copy clip from here…* copies one for a window (`transcript_clip`). The block names the
  recording by its identity when the transcript is a recording's; otherwise the transcript by
  its path inside its synced folder; a transcript outside every synced folder cannot be named,
  and the viewer says so. With *Include the words*, the window's lines follow as a folded
  `[!transcript]` callout, in the `**[hh:mm:ss] Name:** text` format of `transcript.md`.
  Like every block keeper writes, the copied one lists the optional keys it does not set as
  comments after the ones it does (`docs/notes.md` § *Media in a note*).
- **Recordings outside a synced folder** play by their identity over `keeper-recording://`,
  in the block and in the viewer, when this Mac's recordings index knows them.

## Corrections

In the viewer, and in a note's block, which is the same viewer:
- **Edit a line** — the text changes and `edited` is set; `asrText` keeps the original. Word
  timings are spread over the old span when the word count changes. Offers dictionary
  suggestions (above); never touches the bank.
- **Reassign a line** to another speaker, **merge** two speakers, or **rename** a speaker for
  this transcript only — the transcript changes, the bank does not. A speaker left with no lines
  stays in the transcript, hidden from the legend and still offered as a target, so the move
  can be undone — unless another speaker on its track carries the same person: merging into a
  speaker that names the same person removes the absorbed one, and a file that still holds such
  a lineless duplicate (from before this rule) loses it when it is read. A line heard on your
  microphone cannot move to a voice from the call, or back, and those two speakers cannot be
  merged; voices on the microphone can be merged and lines moved among them.
- **Add a speaker** on the microphone or the call, with an optional label: it gets the next
  `S<n>`, is `unknown`, and has no embedding or clip until lines move to it and it is assigned.
- **Split a line** before one of its words — the words before stay, the rest become a new line
  right after it, same speaker and track, each half timed and worded from its words. An
  untouched line's `asrText` is cut at the same word; an edited one (or one where the dictionary
  joined words) keeps its whole `asrText` on the first half. A line splits only between two of
  its words.
- **Add a line after** another — a speaker and text the recogniser missed. It starts and ends
  where the line before it ends (never past the next line's start), is `edited`, and has no
  `asrText` and no words. New lines get the next unused `u<n>` id.
- **Assign a speaker to a person** (existing, or new by name) — the speaker becomes
  `confirmed`, and the bank gains that speaker's clip and embedding. This is the one correction
  that teaches keeper a voice. The clip is cut from the assigned speaker's own lines. When
  another speaker heard on the same track, with lines of its own, already names that person,
  the two are one voice: the assigned speaker's lines move to it, it is the one confirmed, and
  the assigned speaker is removed, so a person appears once in the legend and in each line's
  speaker list. The microphone and the call stay two speakers even when they name the same
  person. Confirming `ME` (and only `ME`, not another voice on the microphone) marks that
  person as me and unmarks whoever was before: confirming `ME` as someone else moves it again,
  which is how a wrong *me* is corrected.

Assignment needs a drive that keeps voices: the drive holding the media if it keeps voices,
otherwise the first enabled one. With none, speakers stay unknown and assignment says why.

Every correction marks the transcript `corrected`, so a later transcription will not replace it
unless you ask it to (*Transcribe again…*).

## Matching thresholds

Each speaker's embedding is compared (cosine) with each person's centroid — the normalised mean
of their vectors for the current embedding model:
- **≥ 0.70** — assigned automatically (`auto`);
- **≥ 0.50** — offered as a candidate (`suggested`), nobody assigned;
- below — `unknown`.

Speakers in different segment files of one recording are the same speaker when their centroids
reach **0.60**. Within one file and track, two voices the diarizer split are one speaker when
their embeddings reach **0.45** (`SAME_VOICE`; measured: 0.526 between two halves of one remote
voice, 0.37 between two people): they are joined before speakers are numbered, and the joined
voice's embedding is the two weighted by how long each spoke. These values are starting
points, not measurements (DW-333): a wrong automatic match costs a click, and nothing reaches
the bank without confirmation.

`ME` is matched too when the microphone was diarized: the bank's `self` person names it when
its voice reaches 0.50; otherwise a person matched at 0.70 names it (`auto`), and below that it
is `suggested` or `unknown` with its candidates — a wrong `self` flag does not name the person
recording. An undiarized microphone, or a `self` person with no voice for the current model,
leaves `ME` to the `self` flag as before.

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
- The player plays media inside a synced folder, and a recording outside one only when this
  Mac's recordings index knows it: a transcript of a file elsewhere on the Mac shows no player.
- A part whose bytes are not on this Mac (a Git LFS pointer) says so and is not played.
- The progress estimate's speeds were measured on one M-series Mac; on a slower one the bar
  waits near the end of a step until the step ends.
- The player's first frame before play is proved by its call policy, not yet seen in the
  macOS app's own webview (DW-346).
- The *Sound* choice switches audio tracks through `HTMLMediaElement.audioTracks`, which only
  WebKit (the macOS app's webview) implements.

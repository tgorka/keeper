# Screen recording

keeper records your screen to a folder on this Mac. **Nothing uploads** — the
recording feature adds zero network destinations (verified by an automated
egress-diff gate in CI), has no telemetry, and writes only where you point it.

## What it records

- **Screen** — the whole main display, a chosen display, or a single
  application (app-scoped capture records only that app's windows, and its
  audio scoping follows).
- **System audio** — the audio the recorded content plays, as its own track.
  keeper's own notification sounds are excluded from the recording.
- **Microphone** — your voice, recorded as its **own separate track** (never
  premixed with system audio; editors can separate them, stock players play
  them together). Pick a device or use the system default input.
- **Webcam** (optional) — your camera, recorded to a **separate file**
  (`camera-####.mov`), synced to the screen segments by a shared clock. When
  the microphone is on, the camera file also carries the mic as its **own
  separate track** (never premixed), so a webcam clip is self-contained.
- **Audio only** — pick "Audio only (no video)" as the source to record just
  system audio and/or the microphone into `audio-####.m4a` segments.

## Audio processing

**Echo cancellation** (**off by default**, under the microphone picker) stops the
microphone from re-recording what your speakers are playing. Without it, a
session recorded on speakers carries the far end twice — once clean in the
system-audio track and once, a few milliseconds late and coloured by the room,
in your microphone track. That is what the reverb on speaker recordings is.
Headphones remove the problem entirely; this switch is for when you cannot use
them.

It runs your microphone through macOS's own voice-processing unit, whose echo
reference is the **output device's** mix — so it removes audio that *other*
apps play, not just keeper's (keeper plays nothing into the recording). Measured
on an Apple-silicon Mac with the built-in speakers and mic: the far end drops
about **24 dB** out of the microphone track. It works.

It is nevertheless **opt-in**, because it is not free — read the costs below and
decide per setup. The reverb only happens on speakers, so the first answer is
headphones, and this switch is the second.

The honest costs, all of them:

- The **microphone track becomes mono**. The canceller is a mono, voice-band
  processor; there is no stereo output to be had. System audio is untouched and
  stays stereo — the two tracks are still never premixed.
- **Voice-band noise suppression comes with it** and cannot be turned off
  separately. Automatic gain control *is* turned off — a recorder must not ride
  your voice level up and down under the far end.
- **Your own voice can come out quieter, and loud speakers make it worse.** A
  canceller subtracts what it can estimate and then suppresses whatever is left,
  and that second stage cannot tell leftover echo from you talking at the same
  moment as the far end. With automatic gain control deliberately off there is
  nothing adding the level back, so on loud speakers the effect is a voice that
  ducks under the far end. If a recording sounds too quiet rather than too
  echoey, that is this — turn the switch off for that session, or use headphones
  and leave it off, which removes the echo path entirely.
- The microphone is captured at the **device's native sample rate** when the
  unit declines to convert; the written track is AAC either way.
- **Aggregate input devices are not supported** — the unit builds its own
  private aggregate and cannot span another one. keeper falls back to the plain
  microphone and says so.
- **The echo reference does not follow a mid-session output change.** Swap to
  AirPods while recording and keeper warns once rather than re-initializing (a
  re-init would cut the microphone track); the cancellation keeps using the
  output it started with.

Every failure degrades to the ordinary microphone path with a warning — never a
failed, silent, or half-written recording. The switch is read at Start, so
changes apply to the **next Recording Session**, and it cannot be changed while
a recording is running.

## Where recordings go

Each Recording Session creates one folder inside your chosen destination
(default `~/Movies/keeper`), at a path the **path template** renders — so the
tree normally nests:

```
~/Movies/keeper/                     # your chosen destination
  2026/                              # the default template nests by year
    2026-08-06 1536/                 # an untitled session
    2026-08-06 1536 standup/         # the same minute, titled "Standup"
      manifest.json    # capture target, devices, ledger, status, session id
      screen-0000.mov  # H.264 + AAC (+ mic AAC) — plays anywhere
      screen-0001.mov
      camera-0000.mov  # only when the webcam is on
```

**Settings → Recording** holds that template; its default is
`{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`, which is why the year folder is
there. keeper reads the clock **once** at Start and renders the template with
the same renderer the settings card's preview uses, so the path the preview
shows is the path a Start creates — byte for byte, because there is no second
implementation to drift. The preview is also the entire manual instead of a
token table: type a template and the card prints the absolute path the next
recording would use, or the reason it refuses that template, before anything
is saved. A stored template that is blank or no longer parses degrades to the
default on read, so a Start never fails over one.

Intermediate folders (`2026/`) are created on demand; the session folder
itself is only ever **created**, never adopted. When the rendered path is
already taken — two Starts inside the same minute — keeper retries with the
next collision ordinal (` (2)`, ` (3)`, …), which lands where the template put
`{seq}` or on the end of the last folder when the template does not mention
it. Only that last folder is ever renamed, so a retry is always a sibling and
never renames a year folder that holds other recordings.

Long recordings rotate into new segments at the configured **segment size**
(default 500 MB) or **duration cap** (default 30 minutes) — the handover is
gapless (the boundary is exactly one frame period, asserted by an automated
CI gate against the manifest's capture-clock bounds). Segments are fragmented
QuickTime files: a crash or power loss costs at most the last ~4 seconds, and
an interrupted session is salvaged on the next launch ("A recording was
interrupted" — with **Reveal in Finder**). That startup scan walks the whole
destination tree rather than only its immediate children, so a session nested
under `2026/` is found; it stops eight levels down and never descends into a
folder that is itself a session.

Recordings that end cleanly show "Saved N segments" with the session path.

Before Start you can optionally describe the **next session** — title (which
feeds the template's `{slug}` and `{title}`), participants, a program/session
note, comma-separated tags, and free-form name/value fields. Everything lands
in `manifest.json` only (local, zero egress), together with wall-clock
start/end times.

Every session also carries an identity of its own, in that same `meta` block:
`sessionId`, the device's ULID joined to a freshly minted one —
`01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KYDKP7WQ8F3M2T5V6X9YB0AC`. Both halves are
Crockford ULIDs, so splitting on the single `-` recovers the device that made
the recording (the same device id keeper's sync uses). It is minted at Start
and never changes afterwards: the folder name is a label, and a label can move
while the identity stays put.

The manifest still holds **no absolute path**: `segments[].file` is a bare
basename resolved against the folder the manifest sits in, and `session` is
that folder's own basename. A session folder stays self-contained: copy or
move it anywhere and it still describes itself.

## Recording into a synced folder

A recording destination does not have to be a plain folder. Any folder keeper
already syncs can hold recordings, and then the Recording pane offers it as a
destination — so a finished segment is already on the drive, with no copy step
and no second place to look.

Flagging it is one switch, in the same place as the notes one and worded the
same way: Settings → Sync → a folder → *This folder holds recordings*. keeper
puts them in a subfolder of that folder (`recordings/` by default) and syncs
them with everything else there. There is no destination registry, no import
and nothing else to configure — a synced folder holds recordings or it does
not.

The subfolder is yours to change, and it may be nested as deep as you like
(`40-media/recordings`). keeper refuses rather than corrects: a subfolder that
is empty, absolute, escapes the folder, or overlaps that folder's notes vault is
rejected on save with the reason, because one folder cannot be both a vault and
a recordings root without the notes indexer walking your video. Turning the
switch off removes the flag and nothing else — no file moves, and a destination
pointing at that folder falls back to the plain folder above.

### The whole path, in one card

You do not have to go to Settings → Sync to change the subfolder. The Recording
pane's Destination card shows both halves of the path a session takes and lets
you set either:

| | *Recordings subfolder* | *Session folder* |
|---|---|---|
| where it lives | the sync profile, in `sync.db` | `recording.path_template`, this machine's settings |
| who else sees it | every machine syncing that folder | nobody — this Mac only |
| may be nested | yes | yes |

They stay two fields on purpose. The first has to be identical on every machine
syncing the folder, or the second machine records somewhere else; the second is
a per-machine preference and cannot be. The card says which is which rather than
merging them into one box that could only be right on one machine.

**Changing the subfolder moves no files.** Sessions already recorded under the
old one stay exactly where they are: they drop out of the recordings browser at
the next archive rebuild, and the `![[recordings/…]]` embeds in their note stubs
stop resolving. The card says so before you save, not after. Move them yourself
first if you want to keep them listed.

Recordings never live at the profile root, and that is not an oversight: eight
places depend on the root being a non-empty folder disjoint from the notes
vault, two of them safety arguments rather than conveniences (the
`keeper-recording://` sandbox's non-overlap proof, and the commit gate that
skips tier-2 checks for recordings and would otherwise apply to every file in
the folder, notes included).

### When the synced folder is on a drive you unplug

A synced folder marked as removable media — a pendrive, an external SSD — is a
destination like any other, and the Destination card says so before you press
Record: it names the drive and, when the drive is not plugged in, says that
too. You find out from the card, not from a failed recording.

With the drive out, pressing Record is **refused**, and the refusal names the
drive (`merope is not attached`) rather than reporting a filesystem error about
a path. Nothing is created anywhere: keeper does not quietly record into the
plain folder instead, because a recording that lands somewhere other than where
the card said is worse than one that does not start. Plug the drive back in and
the next recording simply works — there is nothing to re-choose, clear or
restart.

keeper recognises the drive by the marker it wrote at the drive's root, not by
the mount path, so a stick that comes back on a different mount point is still
the same drive. If a *different* volume is mounted where yours belongs, that is
refused too, with its own sentence — adopting it would sync a stranger's disk.

## Debug mode (Settings → About)

Off by default. While on, keeper writes:

- `~/Library/Logs/keeper/keeper.log` — app-level logs (errors, warnings,
  lifecycle), also visible in Console.app.
- `<session folder>/events.log` — one timestamped line per recording event,
  beside `manifest.json`.

The toggle applies live (no restart), and log writes are best-effort — they
never affect a running capture. For a bug report, zip the session folder:
media, manifest, and event log travel together.

## Settings files — `keeper.toml`, and `config.json` beneath it

Every setting keeper has is readable and writable as a file. Since epic 46 there is a stack of
TOML layers, and `config.json` sits at the bottom of it — still supported, still imported, but
**outranked by every `keeper.toml` layer**. The full layer order, the complete key list with each
key's scope, and which keys a shared file may not set, are in
[`settings-keys.md`](./settings-keys.md); this section covers only what a recording setup needs.

The shortest useful form, for development and scripted setups:

```
~/.keeper/keeper.toml                     # you, on every machine
~/.keeper/keeper.<hostname>.toml          # you, on this machine only
```

```toml
[settings]
"recording.codec" = "hevc"
"recording.fps" = 60
"recording.destination_dir" = "/Users/you/Movies/keeper-dev"
"debug.mode" = true
```

A value in a file **keeps** winning — it is resolved on every read, not imported once at boot, so
a UI toggle over a file-controlled setting does not quietly erase it. Settings that a file
currently owns are listed in Settings, with the file that owns them.

`recording.destination_dir` is an absolute path and so is machine-local: put it in the
`<hostname>` file, or a second machine will try to record into a folder it does not have.

Note the recordings **subfolder** is not here. It belongs to the synced folder rather than to this
machine, so it lives in that folder's own `.keeper/keeper.toml` under `[folder]` — which is why
both machines syncing a folder agree about where its recordings go.

### `config.json` (still supported, lowest precedence)

The older flat JSON file, imported over the settings table at every startup and beaten by any
TOML layer that names the same key:

```
~/Library/Application Support/keeper/config.json   (beside keeper.db)
```

Example:

```json
{
  "recording.codec": "hevc",
  "recording.scale_percent": 50,
  "recording.fps": 60,
  "recording.segment_mb": 250,
  "recording.duration_cap_minutes": 15,
  "recording.destination_dir": "/Users/you/Movies/keeper-dev",
  "recording.echo_cancellation": true,
  "debug.mode": true
}
```

Rules: one flat object; string, number, or boolean values only (booleans map
to the registry's `"1"`/`"0"` convention). Keys import verbatim into the
settings table, and the typed getters keep clamping/normalizing on read, so
an out-of-range hand-edit degrades to its documented default. A malformed
file is reported loudly in the app log and skipped — startup never aborts
over it.

Its ordering guarantee is unchanged and now shared: the whole settings stack
resolves before the debug-mode gate is seeded, so `"debug.mode": true` in
either file applies to that same boot.

The one thing that did change: **`config.json` no longer wins.** It is
imported into the settings table, and the table is what a `keeper.toml`
layer sits in front of — so a key named in both is answered by the TOML file.
A key named only here still works exactly as it always did. Settings lists
which keys a file currently owns and which file owns them, so you can see
this rather than deduce it.

Known recording keys: `recording.codec` (`h264` | `hevc`),
`recording.scale_percent` (`100` | `75` | `50` | `25`), `recording.fps`
(`30` | `60`), `recording.segment_mb` (100–5000),
`recording.duration_cap_minutes` (1–600), `recording.destination_dir`
(absolute path), `recording.path_template` (template string — one that does
not parse degrades to the default on read), `recording.echo_cancellation`
(bool, **default false** — only a stored `"1"`/`true` turns it on),
`debug.mode` (bool).

## Out of scope (honest verdicts)

- **AV1 encoding** — Apple Silicon has no AV1 hardware encoder and
  AVFoundation exposes no AV1 writer codec; H.264/HEVC are the options.
- **Per-app audio-output capture picker** — needs Core Audio process taps;
  deferred.
- **Hiding the macOS menu-bar capture indicator** — the pill is drawn and
  owned by macOS itself as a privacy affordance; no app can disable it.

## Permissions (macOS)

- **Screen & System Audio Recording** — required before Start. On modern
  macOS the system does **not** show a prompt for this permission: grant it
  manually under System Settings → Privacy & Security → Screen & System
  Audio Recording. macOS may require relaunching keeper after granting. On
  macOS 15 and later the system may ask you to **re-confirm this permission
  monthly** (keeper uses the non-picker ScreenCaptureKit path).
- **Microphone / Camera** — standard system prompts appear on first use, and
  each is needed only while that source is enabled.
- macOS shows its own **capture indicator** in the menu bar while recording —
  keeper never suppresses it.

## While recording

- The in-app banner and the **menu-bar (tray) icon** show the live state:
  elapsed time, current segment and its size, Stop, and Open folder. The tray
  stays present for the whole session, and quitting keeper finalizes the
  recording honestly first.
- Failures are **loud, never silent**: a tray error state, a native
  notification, and an in-app banner with the honest reason — already-written
  segments always survive.
- **Disk guard**: below 10 GB free you get a warning; below 2 GB the
  recording stops gracefully and finalizes so everything written stays
  playable.

## For developers (dev builds)

- Real capture needs a **signed build**: sign keeper and the `keeper-rec`
  sidecar with an Apple Development certificate (a free Personal Team works).
  macOS 15+ is documented to reject ad-hoc-signed ScreenCaptureKit
  (Cap #1722); empirically on macOS 26 an ad-hoc build can capture once the
  grant is given manually, but **every ad-hoc rebuild invalidates the TCC
  grant** (the grant keys on the code signature) — a stable certificate makes
  the grant survive rebuilds.
- Use `bun run tauri:build:signed` for any local build you intend to *install*
  and record with; add `-- --install` to replace `/Applications/keeper.app` in
  place. It resolves a codesigning identity (`$APPLE_SIGNING_IDENTITY`, or the
  single one in your keychain), signs the sidecar and the bundle, and then
  **fails the build** if the result is still ad-hoc. Run it from Terminal.app:
  codesign cannot reach the login keychain over SSH
  (`errSecInternalComponent`).
- **From a Linux workstation, `bun run install:macos` already does all of that.**
  It rsyncs the tree to the Mac and then dispatches the signed build into the
  Mac's GUI login session through Terminal.app — the one route to the login
  keychain that needs neither root (`launchctl asuser`) nor the account password
  (`security unlock-keychain`). A Terminal window opens on the Mac, its output
  streams back to your shell, and it closes when the build succeeds. There is no
  unsigned install path any more; before this, the script built ad-hoc over ssh
  and merely printed a warning, which cost the TCC grant on every install.

  ### The "infinite Screen Recording prompt" loop

  Symptom: macOS keeps asking for Screen Recording no matter how often you
  approve it, Privacy & Security shows one or more checked `keeper` rows, and
  the only thing that helps is removing the row with `-` and re-adding the app
  by hand — until the next build.

  Cause: TCC stores the grant against the app's *designated requirement*. An
  ad-hoc, linker-signed bundle has a bare `cdhash` requirement — the hash of
  that exact binary — so **every rebuild is a different app** to TCC. The old
  row survives by path, matches nothing, and a new row is added beside it.
  Compare:

  ```
  # ad-hoc (bun run tauri:build, no identity) — churns on every rebuild
  designated => cdhash H"407771706becc059eb9ff4cf73d9699e210429d3"

  # certificate-signed (bun run tauri:build:signed) — stable forever
  designated => identifier "dev.tgorka.keeper" and anchor apple generic
                and certificate leaf[subject.CN] = "Apple Development: ..."
  ```

  Fix: build signed, then clear the accumulated stale rows **once** —
  the scripted equivalent of the manual `-`:

  ```sh
  tccutil reset ScreenCapture dev.tgorka.keeper
  ```

  Start a recording, approve the prompt once, and the grant persists across
  every later rebuild. `SCStreamErrorDomain Code=-3801` is the error you get
  while the grant is missing or stale.
- The hardened runtime needs the `com.apple.security.device.audio-input` and
  `com.apple.security.device.camera` entitlements
  (`src-tauri/crates/keeper/keeper-rec.entitlements`) or TCC will refuse to
  even show the microphone/camera prompts.
- Segments are fragmented **QuickTime `.mov`**, not `.mp4`, on purpose: the
  macOS 26 fragmented-MP4 muxer is permanently poisoned by wall-clock-slow
  sample delivery (an idle, static screen), failing the segment finalize with
  `-11800/-16341`. The `.mov` fragment path is healthy under the same
  traffic, and a frame-rate idle heartbeat keeps the writer dense so
  fragments keep flushing through idle stretches.

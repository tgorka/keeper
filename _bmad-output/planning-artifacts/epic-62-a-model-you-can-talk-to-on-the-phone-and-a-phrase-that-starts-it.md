# Epic 62 — A model you can talk to on the phone, and a phrase that starts it

created: '2026-09-03'
source: the owner, after Epic 61 shipped and was installed on hesperia — "zrob pry na ios app - na razie tylko z opcja rozmow z hermes oraz voice z wake up word voice", then install it on the iPhone attached to that Mac. Grounded by one read-only repository investigation over `epic61/5-height` (PR #318) and one external research pass, both 2026-09-03, plus binding facts verified in the crate sources by the coordinator.
binds: FR-394…FR-408 (allocated here); NFR-50…NFR-52 (allocated here); AD-161…AD-172 (new); AD-27 (absence, not a dead button — consumed), AD-32 (the Apple project is generated and committed — consumed), AD-55/AD-56 (decisions in `keeper-core`, the shell is a call site — consumed and load-bearing), AD-137 (a capability gates its surface), AD-146…AD-160 (Epic 61, consumed), NFR-11 (the egress claim — the constraint this epic is designed around)
see-also: Epic 61 (`epic-61-a-model-you-can-talk-to-in-the-app-that-holds-your-drive.md`), which deferred voice to "Epic 62" for two reasons and named them. **One of those reasons dies on iOS and the other does not**, which is why this epic is the phone and not the Mac.

## What he said

> zrob app na hesperia, pozniej zrob pry na ios app - na razie tylko z opcja rozmow z hermes oraz
> voice z wake up word voice na koncu zainstaluj na ios podlaczonym do hesperia

Three asks: the Mac build (done, installed), an iOS slice as PRs — conversation with Hermes only,
plus voice with a wake word — and an install on the attached phone.

## Why this is possible now, when Epic 61 said it was not

Epic 61's deferral rested on two sentences, and they must be re-read rather than repeated:

| Epic 61's reason | On iOS |
| --- | --- |
| "every credible permissive stack needs new native dependencies that **cannot be compiled or exercised on the development host**" | **Still true, and worse** — the shell crate does not build on Linux at all, and iOS is CI's and hesperia's target only. This epic keeps every decision in `keeper-core` and the iOS half a thin call site, and expects the gate to find what the dev host cannot. |
| "the pretrained keyword-spotting and ASR model weights carry dataset terms that the code's Apache-2.0 licence does not imply" | **False on iOS, and that is the whole opening.** Apple's recogniser and synthesiser are system frameworks: keeper ships no weights at all. Verified in the crate sources on 2026-09-03: `objc2-speech` 0.3.2 exposes `SFSpeechRecognizer` (`authorizationStatus`, `requestAuthorization:`, `requires_on_device_recognition`) and `objc2-avf-audio` 0.3.2 exposes `AVSpeechSynthesizer` (`speakUtterance`, `stopSpeakingAtBoundary:`), `AVAudioEngine` and `AVAudioSession` — both licensed `Zlib OR Apache-2.0 OR MIT`. No Swift shim, no ONNX runtime, no bundled model, no new package with an unclear weight licence. |

So the licence blocker is gone **on this platform only**, and the desktop half of voice — which
would need `sherpa-onnx` KWS weights whose dataset provenance nobody could establish — stays
deferred. This epic does not pretend otherwise.

## What the phone cannot have, established before anything is designed

**The drive tools cannot go to the phone at all.** `keeper-sync` is not a dependency of the shell
crate on iOS or Android, and Epic 61's tool loop reaches `keeper_sync::bots_fs` through
`crate::bots_tools` — which is exactly why `bots_ipc` was gated `#[cfg(desktop)]` two commits ago
after the iOS compile check went red. A grant is a promise about files keeper cannot reach from a
phone, so on the phone there is no grant, no audit and no deliverable path: absence with a sentence,
never a control that cannot work (AD-27).

**A wake phrase must be armed in front, and then it keeps working behind — corrected 2026-09-03
after the owner asked for the drive-mode case.** The first draft of this epic said background
listening was impossible and rejected it. That was half right, and the wrong half was load-bearing.
What Apple actually forbids is *starting* the microphone from the background: "due to overriding
privacy considerations, apps will not in general be allowed to start recording while in the
background" (Apple DTS, `developer.apple.com/forums/thread/120038`). What Apple documents as
supported is the other half: a session **armed in the foreground continues while the app is in the
background and while the screen is locked**, when the app declares `UIBackgroundModes: audio` and
uses the `.playAndRecord` category — the category Apple describes for "recording (input) and
playback (output) of audio, such as for a Voice over Internet Protocol (VoIP) app", and the key
Apple names for audio that must survive the lock screen and the silent switch.

So the shape is one deliberate act and then hands-free: keeper is opened, listening is armed once,
and from then on the phrase works with Google Maps in front — the answer ducks the navigation
prompts rather than silencing them, and it lands in the same conversation. What the epic still
refuses to pretend: keeper cannot arm itself, so a force-quit or a fresh boot means arming again;
iOS can end the session (a call, Siri, another app taking the microphone) and the port must re-arm
or say it stopped; the system microphone indicator stays lit the whole time and no app can hide it;
and it costs battery. **App Review is not a constraint on this build and the first draft was wrong
to weigh it** — keeper on iPhone is a free-Personal-Team sideload, never TestFlight or the App
Store (`docs/ios.md`), so Guideline 2.5.4 governs nothing here. Siri and App Intents still cannot
hand a third-party app a custom phrase; that part of the research stands.

The default phrase is **`nixie`**, which is one word — so the phrase grammar allows a single word
deliberately, with a letter floor that still refuses something too short to be safe.

**On-device or nothing.** `docs/egress.md` claims to be "the canonical, diffable record of every
network destination keeper contacts", and NFR-11 rests on it. Apple's recogniser will happily fall
back to Apple's servers, and that fallback would add a destination the file does not name. So every
request sets `requiresOnDeviceRecognition = true`, and a locale whose on-device model the phone has
not downloaded gets a sentence telling the person what to download — never a quiet round trip to
Cupertino.

## The vocabulary, and where each half lives

A **turn** is: the phrase is heard, the microphone opens, what was said becomes text, the text is
sent to the bot as an ordinary message, the answer streams back, and the answer is spoken. The
state machine over those steps is a decision, so it lives in `keeper_core::voice` and is tested on
the dev host. Everything that touches CoreAudio or the Speech framework is a **port
implementation** in the iOS half of the shell crate, and it is the thinnest thing that can work.

## What the binds mean

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-394 | the sync-free half of the Bots surface compiles and registers on every platform | 62.1 | AD-161 |
| FR-395 | the drive half — grants, audit, tools, deliverables — is desktop-only, by construction | 62.1 | AD-161 |
| FR-396 | a capability says whether this build can talk to a model, and a second says whether it can reach the drive | 62.1 | AD-162 |
| FR-397 | the phone reaches the Bots surface through the phone shell's own idiom | 62.2 | AD-163 |
| FR-398 | on a phone the surface is one thing at a time: the list, or the conversation | 62.2 | AD-163 |
| FR-399 | a Hermes provider is added, probed and chosen on the phone | 62.3 | AD-164 |
| FR-400 | the phone's own disclosure names what Bots does not do there | 62.3 | AD-164 |
| FR-401 | keeper holds a voice port whose turn-taking is decided in `keeper-core` | 62.4 | AD-165 |
| FR-402 | speech becomes text on the device, and never off it | 62.4 | AD-166 |
| FR-403 | an answer is spoken, and speaking stops the moment a person speaks | 62.4 | AD-167 |
| FR-404 | a wake phrase armed in the foreground starts a turn afterwards, including with another app in front or the screen locked | 62.5 | AD-168 |
| FR-405 | the wake phrase is off until it is chosen, and visible whenever it listens | 62.5 | AD-168 |
| FR-406 | keeper says what armed listening costs and when it stops, beside the switch that starts it | 62.5 | AD-169 |
| FR-407 | talk mode is a control you can see, with the heard text before it is sent | 62.6 | AD-170 |
| FR-408 | the microphone and recogniser are asked for by name, once, with a reason | 62.6 | AD-171 |
| NFR-50 | voice adds no network destination, and the claim is enforced, not asserted | 62.7 | AD-166 |
| NFR-51 | a voice turn is abandonable at every step, and abandoning it releases the microphone | 62.4 | AD-167 |
| NFR-52 | the iOS compile check and the IPA dependency seam both stay green | 62.1 | AD-172 |

## Stack order

```
62.1  the split that lets a phone hold half of it   (bots_ipc sync-free vs bots_drive_ipc desktop-only, two capabilities)
62.2  the phone can reach it                        (PhoneShell entry, one-thing-at-a-time pane)
62.3  Hermes on the phone, and only what it can do  (provider add/probe/pick, the disclosure line)
62.4  a voice port, and an on-device transcriber    (keeper-core turn machine; iOS STT + TTS + audio session)
62.5  the phrase that starts it, and its limits     (armed in front, then background + locked; default "nixie"; a visible chip)
62.6  talk mode you can see                          (mic control, heard text, plist keys, privacy manifest)
62.7  the docs, the disclosures and the gates        (ios.md, IOS_DISCLOSURE_LINES, egress.md, D-5, manifests)
```

62.1 is the only story everything else waits on. 62.4 is independent of 62.1-62.3 and can be built
beside them, because a port and a state machine need no surface. 62.5 and 62.6 need 62.4. 62.7 is
last because it records what the six before it made true.

## Acceptance, per story

**62.1** — **The gate moves from the module to the half that earns it.** `bots_ipc.rs` keeps
everything that needs no `keeper-sync` — providers, bots, discovery, sessions, the streaming pair
and its cancel, the slash-command preview, the metadata toggle — and goes back into the shared
`generate_handler!` literal, which is where Story 61.4's comment always said it belonged. A new
`bots_drive_ipc.rs` takes grants, audit, approval answers, deliverable paths and image staging, is
`#[cfg(desktop)]`, and is spliced into the desktop `$extra` the sync surface already uses. Two
capabilities, because there are two facts: `bots` (this build can talk to a model — `cfg!(desktop)
|| cfg!(mobile)`, i.e. true everywhere the pane exists) and `botTools` (this build can reach the
drive — desktop **and** `sync`). The pane reads the second for every grant and tool affordance;
where it is false they are absent, not disabled. `cargo check --workspace --target
aarch64-apple-ios` passes on hesperia, and `bun run verify:ios-ipa` still finds no tray, hotkey,
autostart, updater or process plugin in the iOS tree.

**62.2** — **On a phone the surface is one thing at a time.** `PhoneShell` gains Bots by its own
existing rules — read them and follow them rather than widening the desktop shell. The conversation
list and the conversation are two views, not two columns, because a 390 px viewport has room for
one; the picker is a sheet; and the back gesture returns to the list. The 61.14 height contract
holds here too: the transcript is the flex-1 min-h-0 region and every band around it is bounded,
which at phone height means the composer and one caption, not four bands.

**62.3** — **Hermes on the phone, and the truth about the rest.** A provider is added and probed on
the phone with the same `keeper-core` grammar the desktop uses; the model list comes from the same
discovery; a Hermes bot is addressed by its `/p/{profile}` prefix as ever. No grant affordance
exists on the phone at all — not greyed out, absent — and the empty state says once that the drive
tools live on the Mac. `IOS_DISCLOSURE_LINES` in `src/components/settings/about-section.tsx` and the
Limitations list in `docs/ios.md` gain the same new line, edited together, because that file says in
so many words that the two must stay identical.

**62.4** — **The turn is a state machine in `keeper-core`; the microphone is a port.** `Idle →
Listening → Heard(text) → Sending → Speaking → Idle`, with an abandon edge from every state that
releases the device (NFR-51), a bounded silence timeout that ends a turn rather than recording
forever, and a phrase matcher that is case- and diacritic-insensitive and refuses a phrase too
short to be safe. All of that is tested on the dev host against a fake port. The iOS
implementation is `SFSpeechRecognizer` with `requiresOnDeviceRecognition = true` and a check of
`supportsOnDeviceRecognition` first — unavailable means a sentence naming the language to download,
never a server round trip (FR-402, NFR-50) — an `AVAudioSession` in `.playAndRecord` with
`duckOthers` and voice processing for echo cancellation, and `AVSpeechSynthesizer` for the answer
with `stopSpeakingAtBoundary:` wired to the moment speech is detected, which is barge-in (FR-403).
`SpeechAnalyzer` is an `#available(iOS 26, *)` fast path only, never the floor, because the floor is
iOS 16.

**62.5** — **The phrase is armed once and then works hands-free, and the switch tells the truth
about what that costs.** Off until switched on; the phrase is typed and validated by `keeper-core`,
whose grammar allows a single word — the default is `nixie` — with a letter floor that still
refuses something too short to be safe. Arming happens in the foreground because iOS allows no
other way, and from then on the phrase works with another app in front and with the screen locked,
because the session is `.playAndRecord` under `UIBackgroundModes: audio`. One `const` in
`keeper-core` carries the whole truth, rendered beside the switch and nowhere else: it keeps
listening in the background and while locked; it stops on disarm, when iOS ends the session, or
when keeper is force-quit; the system microphone indicator stays lit the whole time and no app can
hide it; and it costs battery. The listening state is a small persistent chip with a live region,
not a hover-only affordance — the owner asked for exactly that indicator.

**62.6** — **Talk mode is a control, and what it heard is shown before it is sent.** A mic button
with three legible states (idle, listening, speaking), the interim transcript visible as it forms,
a stop that abandons the turn, and the heard text landing in the composer where the person can edit
it before it goes — because a transcriber that sends what it mis-heard is worse than one that asks.
The plist keys are declared where the generated project reads them (`gen/apple/project.yml`, so
`xcodegen generate` keeps them): `NSMicrophoneUsageDescription` and
`NSSpeechRecognitionUsageDescription`, each a sentence about this app rather than boilerplate, plus
whatever `PrivacyInfo.xcprivacy` requires for these APIs.

**62.7** — **The claims that must not rot.** `docs/egress.md` gains one line stating that voice adds
no destination *because* recognition is on-device and the fallback is refused — and a mechanical
guard asserts the refusal, so the claim is enforced rather than asserted (NFR-50). `docs/ios.md` and
`IOS_DISCLOSURE_LINES` carry the two new limitations. `docs/decisions.md` gains **D-5 — voice is the
system's, on the device, in the foreground**: no bundled weights ever, no server recognition, no
background listening, and the reason each is a refusal rather than a to-do. The two `objc2` crates
get the manifest justification comment the house requires, with their SPDX quoted.

## What stays out, and why

- **Voice on the Mac.** Same feature, different licence problem: nothing permissive with clear
  weight provenance exists for desktop wake-word spotting, and macOS's own `SFSpeechRecognizer` is
  a separate port implementation with its own entitlement story. DW-219.
- **Drive tools on the phone.** Not deferred for want of effort: `keeper-sync` is not linkable
  there, and the honest route is the same one Epic 61 recorded for Hermes — keeper serving MCP over
  the network to something that *can* reach the files (DW-215). DW-220.
- **Keeper arming its own microphone** — waking on the phrase after a force-quit, a reboot, or
  without anybody having opened keeper at all. Rejected because iOS forbids it outright: an app
  cannot start recording from the background, and that sentence is Apple's, not a limitation of
  this implementation (see the corrected paragraph above). What ships is armed-then-hands-free.
- **A Live Activity or Dynamic Island presence** for the listening state, which would put the
  indicator on the lock screen beside Maps rather than only inside keeper and the system's own
  orange dot. Deferred, not rejected: it needs a widget extension in the generated Apple project
  and its own signing story on a free Personal Team. DW-222.
- **Server-side speech recognition** as a fallback when on-device is unavailable. Rejected: it
  would add a network destination to a file that claims to name them all.
- **Ollama on the phone** is neither built nor blocked: it is the same wire and the same code, so an
  endpoint a phone can reach will work. The epic's scope is Hermes because that is what was asked
  for, and blocking the other kind would be code written to make the app less honest. DW-221.
- **A paid Apple Developer Program.** D-1 stands; this epic installs on a free Personal Team, which
  means the 7-day re-arm and no TestFlight.

## Design notes

Reuse: `PhoneShell`'s own navigation rather than a second one; `keeper_core::bots::chat` unchanged —
a voice turn sends an ordinary message, so nothing about the wire changes; `platform.rs`'s port
pattern for the voice trait, with a fake in tests the way the secret port is faked; the 61.14 height
contract for the phone pane. Do not reuse the recording pipeline: `keeper-rec` is a macOS sidecar
that captures the screen, its promise is zero egress and zero transcription, and a voice feature
that borrowed it would make that promise false — the same wall Epic 61 recorded, still standing.

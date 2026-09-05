# Epic 65 — A phone that keeps its shape, and says what its ear is doing

created: '2026-09-05'
source: the owner, after epics 61–64 merged (`main` = `46eefb8`) and the iPhone build was installed on kalypso — "mobile app see not always fit on the portrait mode (especially switching from landscape to portrait) … nixi seems to work there, but not wake word — especially always on in the background — also not seen island indication when talk". Grounded by one read-only repository map, one external research pass (iOS background audio and speech, ActivityKit on a free team), and a simulator reproduction, all 2026-09-05.
binds: FR-441…FR-456 (allocated here); NFR-55, NFR-56 (allocated here); AD-189…AD-196 (new); AD-27 (absence, never a dead control — consumed), AD-31 (the phone shows one level — consumed), AD-55/AD-56 (decisions in `keeper-core` — consumed), AD-137 (a capability gates its surface — consumed and, here, made to gate the tier), AD-179 (voice is a runtime answer — consumed), NFR-11 (zero egress); collects DW-222 (the lock-screen indicator) and DW-228's iOS half (the port has never been run in the background on hardware)
see-also: epic 62 (the phone tier and the iOS port), epic 63 (the chosen language, the reach), epic 64 (the pill on the Mac — the phone has no equivalent, and this epic gives it one).

## What he saw, and what was measured

1. **Landscape renders the desktop.** `src/hooks/use-shell-layout.ts:3` decides the phone tier by `(max-width: 767px)` and nothing else — no height, no orientation, no platform (`:23-58`). An iPhone 14 Pro Max is 430×932 pt; rotated, it is 932 wide, so the **desktop tier** renders at 430 pt tall: "MENU", a "Conversation list" column, the full Bots pane, the bottom clipped. Reproduced on the iPhone 17 simulator and captured. Rotating back re-enters the phone tier with whatever the desktop tier wrote in the meantime — the sidebar fold, the column fold, the detail panel, the primary view — because the four cookie hydrations run once per document for both tiers (`app-shell.tsx:138-161`). The root is `h-screen` (`100vh`, not `100dvh`; `app-shell.tsx:270`), which on iOS is the wrong height whenever the browser chrome or the keyboard moves.
2. **The switch saved "no" and said nothing.** `bot-voice-wake.tsx:210`: `voiceWakeSet(enabled && unavailable === null, draft)`. Toggling on while the port refuses — a Polish system language with no on-device asset, a microphone not yet granted — **persists OFF** and renders the switch off. Nothing re-arms when the refusal later clears (`:196-223`, no listener on availability), and a grant given through the switch does not even clear the stale refusal sentence (contrast `bot-voice-mic.tsx:175-178`). The phrase on his phone was never armed. Separately: the folded line that names the refusal is desktop-only (`voiceFoldedLine` is passed by `bots-pane.tsx` and never by the phone sheet), so on the phone the reason sits in a sheet he has to open.
3. **The island shows nothing because nothing was built for it** (DW-222). What iOS gives for free is the orange microphone dot while the session is open; keeper adds no indicator of its own. ActivityKit is **Swift-only** — a generic class with no Objective-C surface, so `objc2` cannot reach it; a Live Activity needs a Swift bridge in the app target and a widget-extension target with all four presentations in SwiftUI. It needs **no push entitlement and no App Group** for updates from the app process; it does need `NSSupportsLiveActivities`, a second App ID on the free tier (the 7-day profile applies to both), and it ends itself after 8 hours.
4. **The background has never been tested.** The iOS port arms a `.playAndRecord` session with `mixWithOthers` and rolls recognition requests every 45 s (`voice_ios.rs`), and `UIBackgroundModes: audio` keeps the session alive when another app is in front — that is Apple's documented model, and keeper follows it. What Apple documents as ending it: a non-mixable app taking audio (interruption begun, end not guaranteed), an accepted phone call (after which reactivating a record session from the background is reported to fail until the app is foregrounded), Siri (begin without end — the 5 s retry covers it), and suspension. What nobody has measured: whether on-device recognition keeps delivering for tens of minutes in the background on iOS 26.6.1. The phone cannot say: voice's informative lines are `debug!`/`info!` and the phone logs at `info` to a file nobody reads; only WARN+ reach it unconditionally (`debug_log.rs:75-84`), and the Settings → About debug sentence names the Mac's log path on the phone (`about-section.tsx:48-50`).

## The one sentence

**On the phone, keeper chooses its shape by the wrong signal, arms its ear by a rule that saves "no" silently, and keeps what happened to itself — so a rotation breaks the layout, the phrase never listens, and nobody can tell why.**

## Decisions this epic takes

- **AD-189 — The tier is the platform's, not the width's.** A reduced-capability platform (`useIsReducedCapabilityPlatform`, computed from `CapabilitiesVm` — AD-137) is the phone tier in every orientation; width chooses columns only on the desktop. The root grows with the dynamic viewport (`100dvh`), and rotation is a resize the phone tier handles, never a tier change.
- **AD-190 — The switch persists what the person chose, and shows what the port did.** `bots.wake_enabled` is intent. A refusal at arming time is displayed beside the switch as a refusal with its reason and its remedy, and the intent stays on. When the refusal clears — a grant, a language change, the app returning to the foreground, the port's own resume — keeper re-arms without being asked. Never persist a "no" the person did not say.
- **AD-191 — The reason is on the phone's face, not in a sheet.** The refusal's first clause, and the armed/listening/heard state, appear where the phone's composer is, in one line — the phone's equivalent of the desktop's folded line.
- **AD-192 — What the port did is readable on the phone.** A bounded ring of the voice port's events (arm, refuse, roll, interruption begun/ended, resume, turn states) lives in Rust and is shown in Settings → Bots on the phone, newest first, with timestamps; the About debug sentence names the phone's own log path. No new egress: it is a view of memory.
- **AD-193 — The background is verified on hardware or it is not claimed.** Story 65.4's acceptance is a run on kalypso: arm in the foreground, put another app in front, wait, say the phrase; the ring from AD-192 is the evidence. The limits sentence for iOS gains the documented interruptions — a call ends it until keeper is opened again; Siri and a non-mixing app pause it and keeper resumes — instead of the vaguer "when iOS ends the audio session".
- **AD-194 — A Live Activity, built as Swift where Swift is the only door.** A `@_cdecl` bridge compiled into the app target, an `ActivityAttributes` shared with a widget-extension target declared in `project.yml`, `NSSupportsLiveActivities` in the iOS plist, started when the phrase is armed, updated per turn state, ended on disarm; re-requested before the 8-hour end while still armed. The first Swift in the repo, verifiable only on the Mac; the install script registers the second App ID. If the free team's automatic profile refuses the extension, the refusal is measured and the story ends in a DW row with that measurement — not silently narrowed.
- **AD-195 — The phone's simulator rig measures both orientations.** `dev/measure-bots.ts` grows a phone mode: 430×932 and 932×430, the tier rendered, the transcript's share, and the bottom of the pane inside the viewport. Numbers before and after are in the story.
- **AD-196 — No new destination.** Everything here is on the device; `docs/egress.md` says so in one sentence.

## Stories

### 65.1 — A phone that keeps its shape
AD-189, AD-195. The tier by platform; `100dvh`; rotation as a resize; no desktop state written on a phone.
**Acceptance:** on the simulator at 932×430 the **phone tier** renders (level 0 or the open level), nothing is clipped, and rotating back leaves every fold and panel as it was; a test that `phone` is true on a reduced-capability platform at any width and false on the desktop below 768 only when the platform is not reduced; the four hydrations never write on the phone tier; measurements in both orientations before and after.
**binds:** FR-441, FR-442, FR-443, AD-189, AD-195

### 65.2 — A switch that tells the truth and re-arms itself
AD-190, AD-191. Intent persisted as chosen; refusal shown as refusal; re-arm on grant, language change, foreground, resume; the one-line state beside the phone's composer.
**Acceptance:** toggling on while the port refuses leaves `bots.wake_enabled = 1` and shows the reason and remedy; granting the microphone re-arms without touching the switch (test through a fake port whose refusal clears); a language change that makes the locale capable re-arms; the phone's composer row shows `Listening for "nixie" · en-US` / the refusal clause; the desktop is unchanged.
**binds:** FR-444, FR-445, FR-446, FR-447, AD-190, AD-191

### 65.3 — What the port did, on the phone
AD-192. The event ring in `keeper-core` (pure, bounded, timestamped), fed by the shell's port events and turn transitions, a command to read it, a Settings → Bots surface on the phone, and the About sentence corrected per platform.
**Acceptance:** a test that the ring is bounded and ordered; the phone shows the last events with times; a refusal at arming appears there with its reason; the debug sentence names the phone's log path on the phone and the Mac's on the Mac.
**binds:** FR-448, FR-449, AD-192

### 65.4 — Listening that survives the background, measured
AD-193. The foreground/background/resume handling in the iOS port made explicit (`UIApplication` will-resign / did-become-active, interruption ended, the call case), the iOS limits sentence rewritten from Apple's documented interruptions, and a run on kalypso.
**Acceptance:** on kalypso: armed, another app in front, 3 minutes, the phrase heard and answered — the ring shows the roll ticks and the wake match; after a phone call, keeper says listening stopped and re-arms on the next open; the limits sentence names calls, Siri and non-mixing apps by behaviour; the iOS half of DW-228 closes with the measured run.
**binds:** FR-450, FR-451, FR-452, NFR-55, AD-193

### 65.5 — A glance from the island
AD-194. The Swift bridge, the extension target, the attributes, the four presentations, start/update/end tied to arm/turn/disarm, the 8-hour re-request, and the install script's second App ID.
**Acceptance:** with the phrase armed, the Dynamic Island shows keeper's state (armed / listening / heard / speaking) and the lock screen its card; disarming ends it; a photo or screen recording from kalypso is the proof; on the free team the extension signs and installs — or the refusal is measured and filed.
**binds:** FR-453, FR-454, FR-455, NFR-56, AD-194

### 65.6 — The record and the gates
`docs/ios.md` (orientation, the limits, the Live Activity and its costs), `docs/egress.md` (AD-196), `docs/decisions.md`, DW rows, the closure of DW-222 and DW-228's iOS half, every gate including the macOS one and the simulator rig in both orientations.
**binds:** FR-456, AD-189…AD-196

## What stays out
- A tablet tier. An iPad rotates too, but the owner's report is a phone, and iPad support is its own decision.
- Push-updated Live Activities (no APNs, no new destination).
- Recognition that survives an accepted phone call in the background: Apple's reports say the record session cannot be reactivated until the app is foregrounded; keeper says so and re-arms on the next open rather than pretending.

## The failure shape this epic must not repeat
Epic 62 shipped the phone tier's background listening with a design that was right on paper and a switch that could not arm it, and neither was noticed because the phone could not say what it did. Every story here ends with the ring from 65.3 as evidence, and 65.1 and 65.5 end with a picture.

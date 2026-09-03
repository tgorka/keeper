# Epic 63 — One conversation, two devices, and a voice on the Mac

created: '2026-09-03'
source: the owner, after epic 62's five PRs merged to `main` (`687d2aa`) and the coordinator installed a build on kalypso — "ios app po uruchomieniu jest niemal pusta … na app desktop nie widzę voice i wake up voice … mam też Mac M1 Touch Bar … jak rozmawiam na kalypso i hesperia to chce widziec progres rozmowy na obu urzadzeniach". Grounded by one read-only repository map over `main` and three external research passes (macOS speech, Touch Bar, Hermes/Matrix session transports), all 2026-09-03.
binds: FR-409…FR-426 (allocated here); NFR-53 (allocated here); AD-173…AD-181 (new); AD-27 (absence, never a dead control — consumed and load-bearing), AD-31 (the phone shows exactly one level — consumed), AD-55/AD-56 (decisions live in `keeper-core`, the shell is a call site — consumed), AD-137 (a capability gates its surface), AD-161…AD-172 (epic 62, consumed), NFR-11 (the egress claim — the constraint this epic is designed around), D-5 (`docs/decisions.md`: the Mac voice half was deferred under DW-219 — **this epic collects it**)
see-also: epic 61 (the Bots surface), epic 62 (the phone tier and the iOS voice port). DW-219 (macOS voice port), DW-220 (Hermes MCP), DW-222 (a Live Activity for the listening chip) are the deferrals this epic either collects or re-states.

## What he said

> ios app po uruchomieniu jest niemal pusta - zdesignuj UI i UX do obsługi gdy otworzę Keeper app -
> chcę widzieć sesje rozmowy, mozliwosc pisania rozmowy oraz przycisk wymuszający voice (nie
> czekający na słowo nixie).
> Na app desktop - nie widzę voice i wake up voice, szczególnie w backgroundzie - mam też Mac M1
> Touch Bar - spróbuj zamienić ikonkę Siri na Keeper Voice (albo dodaj ikonkę Keepera obok).
> jak rozmawiam na kalypso i hesperia to chce widziec progres rozmowy na obu urzadzeniach
> (szczegolnie transkrypt w sesji rozmowy).

Four asks. **The first one is mostly not a design problem, and saying so is the honest start of this
epic.**

## The correction that comes before the design

The three things he asked for on the phone — a session list, a composer, and a button that forces a
voice turn without the phrase — **all three already ship**, and the repository map anchors each one:

| what he asked for | what exists today | anchor |
| --- | --- | --- |
| "chcę widzieć sesje rozmowy" | `BotSessionList` mounted at phone level 1: New, search, Active/All/Archived chips, rows with age and message count, Rename/Archive/Delete | `src/components/bots/bots-phone-pane.tsx:224-232`; `bot-session-list.tsx:305-437` |
| "mozliwosc pisania rozmowy" | `BotComposer` always mounted; Enter sends; Send enabled once a bot and a model are chosen (the first of each is auto-chosen) | `bots-phone-pane.tsx:423-463`, `:283-303` |
| "przycisk wymuszający voice (nie czekający na nixie)" | the `Talk` button, whose press dispatches `TurnEvent::WakeMatched` — the *same* event a matched phrase produces, so the wake switch is irrelevant to it | `bots-phone-pane.tsx:429-438`; `voice_ipc.rs:262-266`; `keeper-core/src/voice/mod.rs:242-249` |

The screen he saw was empty because **the coordinator mis-built the bundle**: `tauri ios build
--debug` embedded no frontend, so the webview had nothing to render and fell back to the dev-server
URL that a phone cannot reach. Verified by inspecting the installed bundle: `assets/` empty, no
`index.html`, and `grep -rlo "Listen for a phrase" keeper.app` → 0 hits for every UI string tried.
The documented recipe is `bun run tauri ios build --export-method debugging` (`docs/ios.md`,
"Building a shareable IPA").

**Corrected while story 63.2 was being built, and worth stating because the first version of this
paragraph was wrong:** `strings keeper | grep -c localhost:1420` → 1 on a **correct** release build
too. `tauri-codegen` compiles the whole of `tauri.conf.json` into every binary, `build.devUrl`
included (`tauri-utils-2.9.3/src/tokens.rs`, `BuildConfig::to_tokens`, emitted unconditionally), so
the dev URL is not a discriminator and a guard that refused on it would refuse every install. What a
dev build actually lacks is the **embedded frontend**: dev mode emits `EmbeddedAssets::default()`
(`tauri-codegen-2.6.3/src/context.rs:178`) instead of the map whose keys are the frontend's own
asset paths. That is what 63.2's guard tests for.

So this epic does **not** redesign the phone's Bots surface. It does three smaller, real things the
map exposed while looking, and then spends its weight where the gaps actually are.

**What the map did expose as real defects on the phone:**

1. **A fresh phone shows a Matrix login screen, not keeper.** `App.tsx` does not mount `AppShell`
   until an account exists or the first-run wizard was dismissed (`src/App.tsx:150-197`). A model
   you can talk to needs no Matrix account, yet it is unreachable behind one.
2. **Bots is reachable only through the avatar → drawer → Bots.** No tab bar, and ⌘9
   (`use-bots-shortcut.ts:74`) is a hardware keyboard. Two of the drawer's own rows (Approvals,
   Bridges) change the view and render nothing on the phone (`sidebar-pane.tsx:58-76`).
3. **Two strings lie on the phone.** The composer's disabled caption says "Choose a bot **above**"
   while the picker is a bottom sheet (`bot-composer.tsx:79`), and the delete dialog says "removed
   from keeper's own store on **this Mac**" (`bot-session-list.tsx:139-140`).
4. **The pins strip he asked for in epic 61 is desktop-only** (`bots-pane.tsx:353-357`; the phone
   pane never imports it).

## The one sentence

**keeper's voice and its conversations are single-device: the phone can hear you and the Mac cannot,
and a conversation you hold on one is invisible on the other.** Everything below is that sentence.

## Decisions this epic takes

- **AD-173 — A bundle that cannot render is a build defect, not a design gap.** The install path
  gains a mechanical guard: a bundle whose embedded frontend is missing — no `assets/`, an empty
  one, no `index.html`, or a binary carrying none of the asset paths that `index.html` itself names
  — fails the install rather than reaching a phone. The dev-server URL is reported beside the
  refusal and is never the reason for it (see the correction above). The lesson is not "be careful";
  a check that a human can skip is the same defect waiting.
- **AD-174 — The Siri button is not replaceable, and keeper will not try.** `NSTouchBar` items
  appear only while the app is frontmost, and the Control Strip is system-supplied: it accepts no
  third-party items. Pock/MTMR/BetterTouchTool reach it through private `DFRFoundation`
  (`DFRElementSetControlStripPresenceForIdentifier`, `NSTouchBarItem.addSystemTrayItem`), which
  needs a non-sandboxed bundle with library validation disabled, is unmaintained upstream since
  2020-2021, and has an open "not working on macOS 26" report. A signed, notarisable, Apache-2.0 app
  the owner installs from a release does not ship that. **Supported reach instead**, all three
  working while keeper is *not* frontmost: a global hotkey, a tray item, and a `keeper://voice/talk`
  deep link so a Shortcut can be placed on the Touch Bar's own Quick Actions button.
- **AD-175 — macOS voice is a second port behind the same trait, and its differences are named, not
  discovered.** `AVAudioSession` does not exist on macOS, so `.playAndRecord` + `duckOthers` — the
  two things the iOS port leans on — have no counterpart: **half-duplex gating moves into
  `keeper-core`** (the port never records while it speaks), and keeper does not duck other audio on
  the Mac. On-device recognition stays mandatory (`requiresOnDeviceRecognition = true`, absent when
  `supportsOnDeviceRecognition` is false), which keeps NFR-11 intact.
- **AD-176 — One conversation identity, and it is Hermes's.** `GET /api/sessions` is a paginated,
  `last_active`-ordered list and `GET /api/sessions/{id}/messages` is a real history read, so the
  session a phone starts is readable by a Mac — *provided both devices send the same session id*.
  keeper adopts the Hermes `session.id` as the conversation's cross-device identity and sends it on
  every turn. Without that, Hermes derives one per client from a fingerprint of the system prompt
  plus the first user message and the two devices mint different sessions.
- **AD-177 — Following another device's conversation is step-level, never token-level.**
  `GET /v1/runs/{id}/events` is one `asyncio.Queue` per run with a **destructive read**: a second
  subscriber steals events from the first, and a disconnect pops the queue out from under the
  survivor. keeper therefore **never opens a second SSE against a run it did not start.** The other
  device follows by reading completed steps — the user turn, each tool call as it starts, each
  assistant message as it lands — and says so in the interface rather than pretending to stream.
- **AD-178 — No new network destination.** Everything above reads the Hermes host keeper already
  contacts. The token-by-token mirror the owner would like is achievable only by relaying into a
  Matrix room via `m.replace` edits, which changes *where transcripts live* (the homeserver) and
  runs into Synapse's default `rc_message` of 0.2/s with a burst of 10 — one edit every five
  seconds sustained, throttled after about twelve seconds of a 1 Hz loop. That is an
  architecture-level change with a per-homeserver prerequisite; it is deferred with its arithmetic
  written down, not silently attempted.
- **AD-179 — Voice stays a runtime answer, not a capability flag.** `CapabilitiesVm` has no voice
  field and gains none: presence is decided by `voice_availability`, and the tray item, the hotkey
  and the Settings section are gated on that same answer, so one probe governs every surface.
- **AD-180 — A conversation the owner can reach without an account.** The Bots surface does not
  require a Matrix account, and the first-run path stops pretending it does.
- **AD-181 — A remote session's transcript is labelled as remote.** A row read from Hermes rather
  than from `keeper.db` says which device wrote it and when it was last active. Two devices writing
  one conversation is a normal state; a surface that cannot show which is which is the defect.

## Stories

### 63.1 — The phone's Bots surface is reachable and tells the truth

The four real defects above. Bots reachable on a phone with no Matrix account (AD-180); a way in
that is not only the drawer; the two lying strings corrected for the tier; and the pins strip on the
phone under the tier's own layout rules (AD-31: one level, no second column).
**Acceptance:** on a phone-width render with zero accounts, the Bots surface is reachable without
signing in; the disabled composer caption names the sheet, not "above"; the delete dialog says "this
phone" on the phone tier and "this Mac" on the Mac; pinned bots are reachable at level 1; a test
asserts each of the four, including the no-account path.
**binds:** FR-409, FR-410, FR-411, FR-412, AD-180

### 63.2 — A bundle that cannot be a dev build

AD-173. A check, run by the install path, that refuses a bundle whose embedded frontend is missing.
**Acceptance:** the macOS install (`scripts/install-macos.sh` → the signed-build payload it
dispatches into the Mac's GUI session) and the iOS install path in `docs/ios.md` fail with a named
sentence, one per cause, on a bundle whose `assets/` is missing or empty, whose `index.html` is
absent, or whose binary embeds none of the asset paths `index.html` names; the guard is exercised in
both directions on real bundles built on disk — a correct bundle passes, a `--debug`-built one is
refused; the dev-server URL is read from `tauri.conf.json` rather than copied, and is reported
beside a refusal, never used as its cause; `docs/ios.md` records the mis-build, the commands that
reveal it, and the one-line recipe that avoids it.
**binds:** FR-413, FR-414, AD-173

### 63.3 — The voice port's platform-neutral half learns a second platform

`keeper-core/src/voice/` today says "this phone", "Settings > General > Keyboard > Dictation
Languages", and "when iOS ends the audio session" (`mod.rs:51-63`, `:87-109`). A second port needs a
vocabulary that names the platform it is on, and the half-duplex rule AD-175 moves here.
**Acceptance:** every `VoiceUnavailable` sentence is platform-parameterised with no string
duplicated per platform; a pure function owns "may the port record right now" and is false while an
utterance is speaking, with a truth-table test; `keeper-core` gains no platform `cfg` (AD-55).
**binds:** FR-415, FR-416, AD-175

### 63.4 — The Mac can hear you

`voice_macos.rs`: a second `VoicePort`/`ConsentPort` implementation on `SFSpeechRecognizer` with
`requiresOnDeviceRecognition = true`, `AVAudioEngine` for capture, `AVSpeechSynthesizer` for the
answer. The same wake phrase, the same `Talk` button, the same spoken reply.
**Acceptance:** `voice_availability` on macOS answers with a real availability rather than
`Unsupported`; the macOS `Info.plist` carries both usage strings; every `unsafe` call is listed in
`docs/constraints-and-limitations.md`; `keeper-core/tests/voice_on_device.rs` picks the new file up
by prefix and passes (no network token, on-device flag set on every request); an absence state
exists and is worded for each of: no microphone permission, Dictation language not downloaded,
`supportsOnDeviceRecognition` false, no recogniser.
**binds:** FR-417, FR-418, FR-419, NFR-53, AD-175, collects DW-219

### 63.5 — Reach on the Mac, including when keeper is not in front

AD-174 and AD-179. A global hotkey that starts a turn, a tray item that shows the listening state
and starts or stops one, a Settings → Bots voice section (the wake switch and phrase, which today
live only inside the phone's picker sheet), and `keeper://voice/talk` so a Shortcut — placeable on
the Touch Bar's Quick Actions — can start a turn.
**Acceptance:** with keeper backgrounded, the hotkey starts a turn and the tray reflects idle /
listening / speaking; the deep link starts exactly one turn and is idempotent under repetition; all
four surfaces are absent (not disabled) when `voice_availability` reports `unsupported`; the Touch
Bar refusal is recorded with its reason where a reader will meet it.
**binds:** FR-420, FR-421, FR-422, AD-174, AD-179

### 63.6 — One conversation, one identity

AD-176. Probe `GET /v1/capabilities`, adopt the Hermes session id as the conversation's identity,
send it on every turn, and let the session list read Hermes when it is available and `keeper.db`
when it is not (Ollama is stateless and always local).
**Acceptance:** two turns from two devices against one session id land in one Hermes session; the
session list shows remote sessions with their `last_active`, and local-only ones when the endpoint
has no session API; the `hidden` "Bot Chat" row is not silently missed; an endpoint that answers no
capabilities degrades to today's behaviour with no error surfaced.
**binds:** FR-423, FR-424, AD-176, AD-181

### 63.7 — Follow the conversation from the other device

AD-177. While a session is live elsewhere, the transcript grows here at step granularity, and the
interface says that is what it is.
**Acceptance:** a conversation open on device B shows device A's user turn, each tool call as it
starts and each assistant message as it completes; the polling stops when the session goes quiet;
keeper never opens a second SSE against a run it did not start, and a test asserts that; the caption
distinguishes "following" from "streaming" in one sentence.
**binds:** FR-425, FR-426, AD-177, AD-178

### 63.8 — The durable docs, the disclosures and the guards

`docs/egress.md` (no new destination, and *why* the Matrix relay would be one), `docs/decisions.md`
(D-5's Mac half is collected; the Touch Bar refusal), `docs/ios.md` (the mis-build), the macOS voice
disclosure lines beside the iOS ones in `about-section.tsx`, and the deferral rows.
**Acceptance:** every claim in this epic that survived implementation is in a durable doc; every one
that did not is a DW row with its arithmetic; `bun run lint`, `bunx tsc --noEmit`,
`node scripts/check-design.mjs`, the full frontend suite and the Rust suite are green, and the macOS
gate is green on hesperia.
**binds:** AD-173…AD-181, DW-224 onwards

## What stays out, and why

- **A token-by-token transcript on the other device.** Hermes cannot serve it (AD-177) and Matrix
  can only at one edit per five seconds on a default Synapse (AD-178). Deferred with the numbers.
- **Replacing the Siri button.** Refused, not deferred: it needs private API in a build the owner
  installs signed (AD-174).
- **An `NSTouchBar` item inside keeper's own window.** Supported, but visible only while keeper is
  frontmost — which is the one case the hotkey and the tray already cover. Deferred as a nicety.
- **Making Hermes the Matrix bot.** It would give both devices the transcript for almost no keeper
  code, and it moves every conversation off the `/v1` API onto Matrix, turns approvals into
  reactions, and puts transcripts on the homeserver. A product decision, not a story.
- **A voice capability flag.** AD-179.
- **Wake listening while the Mac's lid is closed.** The microphone is physically disconnected; no
  amount of assertion changes that.

## The two failure shapes this epic must not repeat

- **Where the risk lives versus where the test looks.** The last two epics' review found tests
  asserting through pure functions while the risk sat in the impure shell. Here the impure shell is:
  a real bundle on disk (63.2 — inspect an actual built app, not a mocked path), a real
  `SFSpeechRecognizer` answer on a Mac (63.4 — the availability probe on hesperia, not a stubbed
  port), and a real second HTTP client against one Hermes session (63.6/63.7 — two clients, one
  session id).
- **The normal-state-as-fault problem.** Two devices writing one conversation is now legitimate, and
  every check that assumed one writer will fire: the session list's revision signal, the
  `partial = 1` assistant row, the delete confirmation's wording, and any test asserting a message
  count. Finding and fixing those is inside 63.6, not after it.

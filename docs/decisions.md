# Project decisions

Durable, project-level decisions — the ones that shape what keeper will and won't build,
recorded so they stay discoverable instead of living only in planning artifacts. Each entry
cross-references its source (PRD / architecture spine) so a reader can always trace a decision
back to where it was made. Entries are numbered D-1, D-2, … as they are made.

## D-1 — Paid Apple Developer Program: deferred by decision

keeper defers the $99/yr Apple Developer Program. This is a deliberate deferral, not an
oversight and not a purchase — recorded here because the deferral is load-bearing.

- **Unlocks (only the paid program grants these):** APNs push; the Notification Service
  Extension (NSE) for background notification decryption, with its 24 MB memory ceiling and
  App-Group store-layout implications; TestFlight; App Groups; and AltStore PAL notarization for
  EU distribution. (PRD §13.5; architecture spine "Deferred")
- **Opening trigger:** the gate opens only when push becomes a product goal. (PRD §13.5; §13.8,
  the paid-program-timing open question)
- **Constraint it forces:** once push is on the table, keeper's client-only posture makes it a
  PRD-level question — push must ride a homeserver operator's gateway, Beeper's, or a user-run
  Sygnal. It must **never** ride project infrastructure. (NFR-11; PRD §13.5) This is the
  load-bearing reason the gate exists: keeper contacts only user-configured homeservers/bridges,
  Beeper's API when a Beeper account exists, and the signed-update endpoint — running push
  infrastructure would break that invariant.
- **Cheap-now mitigations already paid for:** the single `Platform::data_dir()` root keeps all
  account state under one path, so a future App Group container move (NSE era) is a path change,
  not a data migration. (AD-29; FR-65) Plan B (UniFFI + native SwiftUI shell) stays shelved with
  its revisit triggers recorded — the blank-webview bug class proving unfixable across Tauri
  releases, or NSE work beginning. (PRD §13.8)
- **Status / owner:** deferred. Revisit is owned by the PM/owner, when push demand is real.
  (PRD §13.8, the paid-program-timing open question)

## D-2 — Virtual files are pointer files, not filesystem virtualization

keeper will let a clone hold LFS-tracked content as metadata only, materialize it on request and
release it again — but it will **not** make that content appear at its true size in `ls` or in
Finder. This is a closed question on macOS and a deferred one on Linux, and it is recorded here
because the "why not" will otherwise be re-asked every time somebody sees Dropbox do it.

- **What is built:** the virtual state is the committed git-LFS pointer in the worktree,
  byte-for-byte. Metadata (true size, oid, modification time, provenance, where the bytes really
  are) is answered from the index, the pointer, keeper's own ledger and `git log` — never from the
  worktree stat for the size. Hydrate and dehydrate are explicit verbs; release is lazy, budgeted,
  and rides a successful sync. (AD-122…AD-134; Epic 56)
- **When a locally-authored file may be released:** never until keeper has confirmed that exact
  path reached the remote, and then a TTL (24 h by default) after that confirmation — not after
  last use. Content that merely arrived from the remote and was never modified here keys off last
  use instead. Two clocks, because only one of the two cases can lose data, and that case gets the
  stricter one. (AD-131; Epic 56, story 56.5)
- **Automatic release is the default, not the only path:** release is a keeper *task* with three
  modes — off, manual, scheduled — so "a nightly script" is a supported way to run it rather than
  a workaround. (AD-136; Epic 57, story 57.4)
- **Why not macOS File Provider:** `NSFileProviderReplicatedExtension` has exactly the right
  semantics — dataless items, `fetchContents` on read, `evictItem` — but its storage is exposed
  under `~/Library/CloudStorage/<Provider>` and its container path is relative to an app-group
  container. There is no documented API to virtualize a path the *user* chose, which is the only
  path a synced folder ever has. `SF_DATALESS` *"may not be set or unset from user space"*
  (`chflags(2)`), so keeper cannot mark its own files dataless either. Kexts are policy-dead.
  (research §4.1, §11.3; AD-130)
- **Why not FUSE on macOS:** macFUSE's licence forbids redistributing binaries "bundled with
  commercial software … including the automated download or installation", and its kext is
  closed-source; fuse-t is free for personal use only. Both fail the permissive-only cargo-deny
  firewall that D-1's sibling rules already impose. (research §4.1, §11.4)
- **Why not `fanotify` HSM on Linux yet:** it is the only mechanism that can decorate paths in
  place, and it is immature — kernel ≥ 6.14, `CAP_SYS_ADMIN`, `mmap` materializes whole files
  because the page-fault hook was merged and backed out, directory events and filesystem freeze
  deadlock, and every read re-fires the event because the planned BPF suppression is
  unimplemented. (research §4.2, §11.5)
- **Why no on-read hydration at all:** a `grep -r`, Spotlight, a backup agent or a `du` walks the
  tree and hydrates everything. Microsoft documents "large-scale hydration and unexpected data
  consumption"; Nextcloud shipped an infinite implicit-hydration loop; Lustre HSM ships a mode
  that returns `ENODATA` instead of restoring. keeper already chose this side once, for iCloud
  placeholders (`docs/sync.md` §4). (research §8; AD-128)
- **The cost accepted, stated so it is chosen and not discovered:** `ls -l`, `du` and third-party
  applications see ~130 bytes of pointer text for a virtual file. That representation is not a
  shortcut — it is the only one `git status` tolerates, because the pointer *is* the committed
  blob. (research §2.1; AD-124)
- **Revisit triggers:** an Apple API that virtualizes an arbitrary path; or the `fanotify`
  page-fault hook plus BPF event suppression both landing. A read-only Linux FUSE **mirror**
  mount (never a virtualization of the worktree itself) is deferred with its shape already
  recorded. (AD-130)
- **Status / owner:** decided. Owner is the architect; Epic 56 implements the pointer design.

## D-3 — Scheduled work is keeper's own; scheduled *self-update* is not

keeper will hold named tasks with a schedule, a last run and a last result, runnable on the sync
daemon and on the desktop app and drivable from `cron`. It will **not** let a schedule replace or
restart a keeper binary. Recorded here because the two look like one feature and only one of them
is safe.

- **What is built:** a task record, a schedule validated when it is saved, a due-gate on the tick
  each host already runs, a one-shot CLI verb a `cron` entry or systemd timer can call, and a view
  that states which host will actually run each task. (AD-135…AD-137; Epic 57)
- **Why not a second scheduler:** one clock per host process, because two schedulers over one git
  repository produce concurrent index locks — the rule the notes cadence already follows. (AD-62)
- **Why `update` is excluded:** the daemon holds a durable journal and can be mid-push at any
  moment; swapping its binary unattended is how a routine release becomes a corrupted transfer.
  That refusal predates this decision and survives it. (`docs/sync.md` §13; AD-136)
- **The platform asymmetry, stated so it is not discovered:** on Linux the systemd user service is
  a real background host, and a timer/oneshot pair ships beside it calling the same one-shot verb;
  both need `loginctl enable-linger` to survive logout. That condition is checked rather than
  assumed — `systemctl --user is-enabled` means *wanted at login*, not *survives logout*, so the
  Tasks view stats the file `enable-linger` creates and shows a lingering box *"logged in or not"*
  and a non-lingering one *"its schedule stops when your session ends"*. On **macOS keeper ships no
  background host at all** — the `keeper-syncd` binary *is* built and published for macOS, and its
  one-shot verbs work there, but no launchd plist exists anywhere in the tree, so nothing starts
  `watch` and nothing triggers a task. The desktop app is therefore the only host keeper provides,
  a task runs only while keeper is running, and the UI says so rather than implying a schedule that
  cannot fire. (AD-137; `docs/sync.md` §14)
- **Revisit trigger:** a launchd agent for `keeper-syncd` on macOS. The blocker is not the crate —
  it builds and ships there — it is deciding what a background daemon on macOS should own when the
  app is already a real background host. Until then the asymmetry is visible in the product, not
  hidden.
- **Status / owner:** decided. Owner is the architect; Epic 57 implements it.

## D-4 — The endpoint is yours, never keeper's

keeper will talk to a model — a Hermes Agent profile or an Ollama model, over the OpenAI-compatible
wire, from a surface of its own at ⌘9 — and it will **not** decide where that model lives. There is
no default endpoint, no hosted model, no keeper-operated proxy, no telemetry about what you asked,
and no opt-in scaffolding for any of them, because there is nothing to opt into (`docs/egress.md`,
*The no-telemetry invariant*). Recorded here because a chat feature is where every one of those is
first proposed as a convenience, and the refusal has to be findable before the proposal is made.

- **What is built:** a provider is a record you create — a kind (`hermes` | `ollama`), a base URL,
  a credential behind the keychain port — and a bot is (provider, model-or-profile, identity).
  Because keeper ships no endpoint, the base URL is **required, not defaulted**: a fresh install has
  no provider row, no egress row and no pane content beyond the sentence that says so, and typing
  the URL is the explicit act that creates the destination. Every configured provider then appears
  in Settings → About as a derived, host-only egress entry through the same reduction git remotes
  use, and a loopback or private-network host is accepted and disclosed rather than blocked, since
  that is where both supported back ends live by default. (AD-146…AD-148, extending AD-53; Epic 61,
  stories 61.1, 61.4, 61.13)
- **Why no default, no hosted model, no proxy:** each would make keeper a party to the
  conversation. A default endpoint is a destination the user did not choose; a hosted model or a
  keeper-operated relay is project infrastructure on the traffic path, which is the invariant D-1
  already refuses to break for push and this decision refuses to break for chat. The client-only
  posture (NFR-11) is not a posture about Matrix; it is a posture about keeper. (AD-148; PRD §13.5)
- **Why a Hermes bot gets no drive grant:** Hermes executes its own tools on its own host under
  its own permission model — `/v1/capabilities` declares `tool_execution: "server"`, no route lets
  a client register a tool, and an api-server session defaults to `unattended_mode: deny`. A grant
  keeper offered there could never be exercised, and an affordance that cannot act is the lie AD-27
  forbids, so the Hermes pane offers none and says why in one sentence. The drive tools are offered
  only where keeper is the thing that would run them: an Ollama bot whose model advertises `tools`.
  (AD-151, AD-158, AD-159; Epic 61, coordinator correction to 61.10; research §2.6, §2.13)
- **The honest route to that capability, deferred and not rejected:** keeper *serving* MCP. Hermes
  loads `mcp_servers:` from its own configuration over stdio or HTTP, so keeper could serve the same
  seven verbs, behind the same grant, to a bot running on another host. That inverts the direction
  of the connection — a listening socket is not an outbound request — so it needs its own threat
  model and must not be designed from the client side by analogy. (DW-215; research §2.14)
- **Why voice and the wake word are Epic 62, not a story here:** every credible permissive stack
  needs native dependencies that cannot be compiled or exercised on the development host, and the
  largest open licence question in the research is the one that would ship in the bundle — the
  pretrained keyword-spotting and ASR model weights carry dataset terms the code's Apache-2.0
  licence does not imply. The recording feature's promise that it transcribes nothing is stated in
  six places and enforced by a source scan, and a voice surface that borrowed the recording
  pipeline would make that promise false. The direction is recorded so 62 does not re-research it.
  (Epic 61, *What stays out*; research §10)
- **Revisit triggers:** a third provider kind with a real endpoint to read against (the enum stays
  closed at two until then, DW-214); the MCP threat model being written (DW-215); the model-weight
  licences resolving and the native voice stack becoming buildable on the development host (Epic
  62). None of them reopens the first paragraph: a default endpoint is not a revisit, it is a
  different product.
- **Status / owner:** decided. Owner is the architect; Epic 61 implements the provider, the surface
  and the grant; Epic 62 owns talk mode.

## D-5 — Voice is the system's, on the device, armed by a person

keeper will listen for a phrase and talk to a model on the phone and on the Mac — speech becomes
text on the device, the text is sent to the bot as an ordinary message, and the answer is spoken —
and it will **not** ship a model of its own, send a voice anywhere, or start its own microphone.
Recorded here because each of those three is the first thing proposed when voice is "almost
working", and the epic that built this got one of them wrong in its first draft; the corrected
reasoning has to be findable before it is re-argued.

- **What is built:** a turn is a state machine in `keeper_core::voice` — `Idle → Listening →
  Heard → Sending → Speaking → Idle`, abandonable from every state so the microphone is released,
  with a bounded silence timeout — tested on the dev host against a fake port. Two ports implement
  it in the shell crate and decide nothing: the iOS half over `SFSpeechRecognizer`,
  `AVAudioEngine`, `AVAudioSession` and `AVSpeechSynthesizer` (`crates/keeper/src/voice_ios.rs`),
  and the macOS half over the same recogniser, engine and synthesiser without an audio session
  (`crates/keeper/src/voice_macos.rs`, Epic 63, Story 63.4). Each names its platform through
  `keeper_core::voice::VoicePlatform` (`keeper-core/src/voice/platform.rs:29-32`, `:51`, `:65`),
  so every refusal sentence is composed once and says "this phone" or "this Mac", and the rule
  "may the port record right now" is `keeper_core::voice::may_record`
  (`keeper-core/src/voice/turn.rs:249-252`): false while an utterance speaks on a half-duplex
  platform, which the Mac is. The wake phrase is validated by `keeper-core` (one word is allowed,
  the default is `nixie`, a letter floor refuses something too short to be safe), off until a
  person switches it on, and shown as a live chip whenever it listens. (AD-165…AD-171, AD-175;
  Epic 62, stories 62.4–62.6; Epic 63, stories 63.3–63.4)
- **Why no bundled model weights, ever:** Apple's recogniser and synthesiser are system frameworks,
  so keeper ships no weights at all — and that is the whole reason voice exists on both Apple
  platforms and on neither of the others. Epic 61 deferred voice because every permissive desktop
  stack needs pretrained keyword-spotting and ASR weights whose dataset terms the code's
  Apache-2.0 licence does not imply, and that is still true for Linux and Windows: nothing with
  clear weight provenance exists for desktop wake-word spotting. What Epic 62 recorded as the
  Mac's own open question (DW-219: "macOS's own `SFSpeechRecognizer` is a separate port with its
  own entitlement story") Epic 63 collected: the Mac spots the phrase in the same on-device
  transcript it already produces, so no spotter and no weights are needed there either, and the
  entitlement story is two usage strings in `crates/keeper/Info.plist:22-33`. The port exists and
  passes every test that runs on the dev host; it has not yet been compiled or run on a Mac, and
  that gap is DW-228, worded as a gap. (`objc2-speech` and `objc2-avf-audio`, both `Zlib OR
  Apache-2.0 OR MIT`; D-4, *Why voice and the wake word are Epic 62*; Epic 62, *Why this is
  possible now*; Epic 63, Story 63.4)
- **Why on-device recognition only, with no server fallback:** `docs/egress.md` claims to be the
  record of every network destination keeper contacts, and NFR-11 rests on that claim. Apple's
  recogniser will fall back to Apple's servers unless told not to, and that fallback would be a
  destination the file does not name. So every request sets `requiresOnDeviceRecognition = true`,
  the recogniser's `supportsOnDeviceRecognition` is checked first, and a locale whose model is not
  on the phone gets a sentence naming the language to download — never a quiet round trip. This is
  enforced, not asserted: a source scan over the voice modules of both crates fails the build if a
  request is ever built without the flag, if the flag is ever `false`, or if a network API appears.
  (AD-166; FR-402, NFR-50; `voice_on_device` in `keeper-core`'s tests)
- **Why armed in the foreground, and why keeper never arms itself:** what iOS forbids is
  *starting* the microphone from the background — "apps will not in general be allowed to start
  recording while in the background" is Apple's sentence, not a limitation of this implementation.
  What iOS supports is the other half: a session armed while keeper is in front keeps running with
  another app in front and with the screen locked, under `UIBackgroundModes: audio` and the
  `.playAndRecord` category. So listening is one deliberate act and then hands-free, and four
  things stop it: the person turning it off, iOS ending the audio session (a call, Siri, another
  app taking the microphone — the port re-arms when the interruption ends, and says it stopped when
  it cannot), keeper being force-quit, and a reboot. Waking on the phrase after a force-quit or a
  reboot, or without keeper ever having been opened, is **refused by iOS**, not unbuilt by keeper —
  and the system microphone indicator stays lit for the whole of an armed session, which no app can
  hide. All of that is one `const` in `keeper-core`, rendered beside the switch and nowhere else.
  (AD-168, AD-169; FR-404…FR-406; Apple DTS, `developer.apple.com/forums/thread/120038`; Epic 62,
  *What the phone cannot have*, corrected 2026-09-03)
- **What the Mac does differently, named rather than discovered:** `AVAudioSession` does not exist
  on macOS, so the two things the iOS port leans on — `.playAndRecord` and `duckOthers` — have no
  counterpart. Half-duplex therefore lives in `keeper-core` (`may_record` is false while the Mac
  speaks, and the turn releases the microphone before every utterance) and keeper does not lower
  other audio on the Mac; both are said in Settings → About, in `docs/egress.md` *On this Mac*, and
  nowhere else. Armed listening on the Mac is a foreground app's capture kept alive by an App Nap
  activity assertion held only while a capture is up (`voice_macos.rs:94-98`); a closed lid
  disconnects the microphone physically and no assertion changes that (DW-227). The three ways to
  reach a turn while keeper is not in front are D-6's. (AD-175; Epic 63, Story 63.4)
- **What App Review has to say about any of this: nothing.** The epic's first draft weighed App
  Store Guideline 2.5.4 against background listening and rejected the feature partly on that
  ground. That was wrong: keeper on iPhone is a free-Personal-Team sideload (D-1, `docs/ios.md`),
  never TestFlight or the App Store, so no review guideline governs this build. What governs it is
  what iOS itself permits and refuses, and that is the list above. (Epic 62, *What the phone cannot
  have*)
- **Revisit triggers:** a Live Activity for the listening state, which needs a widget extension
  and its own signing story on a free Personal Team (DW-222); the macOS port's first compile and
  first run on a Mac (DW-228), which may move sentences but not this decision. None of them
  reopens the second paragraph: a server fallback is not a revisit, it is a new row in
  `docs/egress.md` that this decision refuses to write.
- **Status / owner:** decided. Owner is the architect; Epic 62 implements the turn machine, the iOS
  port, the phrase and the surface; Epic 63 implements the platform vocabulary, the macOS port and
  its reach (DW-219 collected).

## D-6 — The Siri button stays Siri's; keeper's reach on the Mac is the hotkey, the tray and a Shortcut

keeper will start a voice turn on the Mac while it is not the frontmost app — from a global
hotkey, from a menu-bar tray item, and from a `keeper://voice/talk` link a Shortcut can open —
and it will **not** put itself on the Touch Bar's Control Strip or replace the Siri button there.
Recorded here because the owner asked for exactly that, the refusal rests on facts about macOS
rather than on effort, and the same request will be made again the next time someone holds a
Touch Bar Mac.

- **What is built:** an OS-global accelerator under `hotkey.voice`, unset by default, registered
  only where `voice_availability` does not answer `unsupported`
  (`crates/keeper/src/hotkey.rs:297-301`; `keeper-core/src/config/keys.rs:457-461`); a tray row
  that shows idle / listening / speaking and starts or stops a turn; a Settings → Bots voice
  section holding the wake switch and phrase; and the deep link, whose grammar and idempotence are
  `keeper-core`'s (`keeper-core/src/voice_reach.rs:4-8`, `:37`, `:83-87`) — one event asks at most
  once however many `keeper://voice/talk` URLs it carries, and the ask is idempotent against an
  open turn. All four are absent, never disabled, where there is no port. (AD-174, AD-179; FR-420,
  FR-421, FR-422; Epic 63, Story 63.5)
- **Why the Control Strip is out of reach:** it is system-supplied and accepts no third-party
  items; Apple's `NSTouchBar` API places items only in the app region, and only while the app is
  frontmost (Apple, *NSTouchBar*, `https://developer.apple.com/documentation/appkit/nstouchbar`;
  Apple HIG, *Touch Bar*, `https://developer.apple.com/design/human-interface-guidelines/touch-bar`;
  Epic 63, research pass on the Touch Bar, 2026-09-03). The apps that do put
  themselves there — Pock, MTMR, BetterTouchTool — reach it through the private `DFRFoundation`
  framework (`DFRElementSetControlStripPresenceForIdentifier`, `NSTouchBarItem.addSystemTrayItem`),
  which needs a non-sandboxed bundle with library validation disabled. Pock's newest release is
  the `0.9.0-22` pre-release of 2021-09-28 and MTMR's is `v0.27.0` of 2020-11-20, and Pock carries
  an open bug "not working on Macos 26.0 (25A354)" filed 2026-07-24
  (`https://github.com/pock/pock/releases`, `https://github.com/Toxblh/MTMR/releases`,
  `https://github.com/pock/pock/issues/655`, all read 2026-09-03). A signed, notarised,
  Apache-2.0 app the owner installs from a release does not ship a private-framework call that
  macOS may refuse next year, and keeper will not weaken its hardened runtime to make one work.
- **Why an `NSTouchBar` item inside keeper's own window is deferred, not built:** it is
  supported, and it is visible only while keeper is frontmost — which is the one case the hotkey
  and the tray already cover, and the one case the owner did not ask about (he wanted voice
  "especially in the background"). A nicety, recorded as DW-225.
- **The supported route to the Touch Bar:** the Touch Bar's own Quick Actions button lists
  Shortcuts the user pins, and a Shortcut that opens `keeper://voice/talk` starts a turn from
  there. That is the Touch Bar reach keeper offers, and Settings names it.
- **Revisit triggers:** Apple documenting a third-party Control Strip API; a Mac that still has a
  Touch Bar shipping with a macOS keeper supports (the last was 2021's, so the trigger shrinks
  yearly). Neither reopens the private-framework route.
- **Status / owner:** decided. Owner is the architect; Epic 63, Story 63.5 implements the reach.

## D-7 — Following another device's conversation is step-level, never token-level

keeper will show a conversation held on the other device growing on this one — the user turn as
it is sent, each tool call as it starts, each assistant message as it lands — and it will **not**
pretend to stream it. Recorded here because the owner asked for the transcript word by word, the
refusal is forced by a mechanism in Hermes rather than chosen, and the one route around it
changes where transcripts live.

- **What is built:** one conversation identity, Hermes's own `session.id`, sent on every turn as
  `X-Hermes-Session-Id` so two devices land in one session instead of minting one each
  (`keeper-core/src/bots/remote.rs:6-12`, `:68`); a session list that reads `GET /api/sessions`
  when `GET /v1/capabilities` says the endpoint has one and `keeper.db` alone when it does not; a
  row read from Hermes labelled as remote, with which device wrote it and when it was last active;
  a dismissal tombstone so a session deleted here is never adopted back (`remote.rs:425-448`);
  and following, which is the history read `GET /api/sessions/{id}/messages` repeated at 2 s, 5 s
  and 15 s as the session quietens, stopped after five cold minutes, and merged under one rule: a
  turn this device sent is shown as this device wrote it, every other turn as the gateway holds
  it (`keeper-core/src/bots/follow.rs:13-39`, `:44-63`). The interface says "following", not
  "streaming", in one sentence (`src/components/bots/bot-conversation.tsx:185-188`). (AD-176,
  AD-177, AD-181; FR-423…FR-426; Epic 63, stories 63.6–63.7)
- **The mechanism that forces step-level:** `GET /v1/runs/{id}/events` is one `asyncio.Queue`
  per run with a destructive read. A second subscriber steals events from the first, and either
  side disconnecting pops the queue out from under the survivor. So keeper **never opens an SSE
  against a run it did not start**, and a test against a fake gateway that would answer the
  route proves the path is never requested
  (`keeper-core/tests/bots_remote_session.rs:1074`,
  `keeper_never_opens_the_run_events_stream_of_a_run_it_did_not_start`; `follow.rs:7-11`; Epic
  63, research pass on Hermes session transports, 2026-09-03). What Hermes does persist is
  step-granular — the user turn at turn start, the assistant's tool-call block before the tools
  run, the final text before the loop exits — and that is what the other device reads.
- **Why the route around it is not taken:** relaying the live transcript into a Matrix room via
  `m.replace` edits would add no host and would still make the homeserver a place transcripts
  persist, which `docs/egress.md` records as an architecture-level change under NFR-11 with its
  arithmetic: Synapse's default `rc_message` of 0.2 events per second with a burst of 10 throttles
  a 1 Hz edit loop after about twelve seconds; an event is capped at 65536 bytes and an edit
  carries the whole new body; `m.relates_to` stays in cleartext in an encrypted room.
  (`docs/egress.md`, *Why the token-by-token mirror is a destination change*; DW-224)
- **Why Hermes is not made the Matrix bot:** it would give both devices the transcript for almost
  no keeper code, and it moves every conversation off the `/v1` API onto Matrix, turns approvals
  into reactions, and puts transcripts on the homeserver. That is a product decision, not a story;
  recorded as DW-226 so it is decided rather than drifted into.
- **Revisit triggers:** Hermes serving a fan-out or replayable event stream per run; the owner
  deciding the Matrix-bot product question. Neither reopens the first paragraph without also
  reopening `docs/egress.md`.
- **Status / owner:** decided. Owner is the architect; Epic 63, stories 63.6–63.7 implement it.

## D-8 — Voice is a runtime answer, not a capability flag

keeper will decide whether every voice surface exists — the wake switch, the tray row, the
hotkey, the Settings section, the About disclosure — from one runtime question,
`voice_availability`, and it will **not** add a `voice` field to `CapabilitiesVm`. Recorded here
because a capability flag is the obvious shape (every other tier-telling surface has one) and it
would be wrong for this one.

- **What is built:** `voice_availability` answers `null` when voice works and otherwise one of
  `notAuthorized`, `noOnDeviceModel`, `noOnDeviceRecognition`, `noMicrophone`, `noRecognizer`,
  `unsupported` with the sentence to show (`src/lib/ipc/gen/VoiceUnavailableVm.ts`). Every surface
  reads that answer from one store — `unsupported` is every build without a port and renders the
  surface away; any other refusal renders the surface with its sentence and remedy; an unanswered
  question renders nothing, so absence is never decided from a question not yet asked
  (`src/hooks/use-voice-facts.ts`, `src/lib/stores/voice.ts:31-34`). `CapabilitiesVm` gained no
  field. (AD-179; Epic 63, Story 63.5)
- **Why not a flag:** a capability is `cfg!`-shaped — it says what the build *can* do and never
  changes while the app runs. Voice on a Mac changes: Dictation is turned on, a language is
  downloaded, a microphone is unplugged, a permission is granted in System Settings. A flag would
  either be true for a Mac that cannot listen right now (a dead control, AD-27) or would have to
  be recomputed on every probe, at which point it is the runtime answer under another name, with
  two callers who can disagree. One probe governs every surface.
- **Revisit triggers:** none foreseen. A third platform with a port reads the same answer.
- **Status / owner:** decided. Owner is the architect; Epic 63, Story 63.5 implements it.

## D-9 — A spoken answer is spoken in a language this device has a voice for, and the model is told which

keeper will read an answer aloud in the voice of the language the answer is written in, when
this device has such a voice, and will ask the model — on a turn that began by voice — to answer
in the language the person is speaking; it will **not** read text in the voice of another
language, bundle a language model to tell which language a text is in, or send the text
anywhere to find out. Recorded here because the first Mac voice read a Polish answer in an
English voice and the owner could not understand it, because the fix rests on a measured fact
about Apple's inventories that is easy to forget, and because the obvious shortcut — "just use
the listening language" — is wrong in a way that only shows up with a bilingual model.

- **The measured fact this rests on:** recognition and synthesis are different inventories.
  On the owner's Mac, `SFSpeechRecognizer.supportedLocales` lists 63 locales, of which **four**
  support on-device recognition, all of them English; `AVSpeechSynthesisVoice.speechVoices`
  lists **180** voices, one of them Polish (`crates/keeper/src/voice_macos.rs:1161-1162`,
  `:1453-1456`; `keeper-core/src/voice/speech.rs:4-9`; Epic 64, *Facts this epic stands on*,
  probed on hesperia 2026-09-04). So the listening language does not bound the speaking
  language: a person listening in English whose bot answers in Polish *can* hear Polish, and
  before this epic nothing chose a voice at all — the utterance was built with no `setVoice:`,
  so the system default spoke.
- **What is built:** two halves. *Before the turn*, a voice-originated request carries one
  system sentence — *"The person asked this aloud and your answer will be read aloud to them.
  Answer in Polish (pl-PL)."* — naming the listening language (`speech.rs:108-117`, prepended
  in `crates/keeper/src/bots_ipc.rs:1148-1168`); a typed turn's body is byte for byte what it
  was (`keeper-core/tests/bots_remote_session.rs:631`). *After the answer*, the port detects the
  text's dominant language on-device with `NLLanguageRecognizer`, constrained to the languages
  this device has voices for plus the listening one (`speech.rs:49-64`; `voice_macos.rs:1493-
  1518`), and `speech::choose_voice` answers which language to speak in (`speech.rs:66-84`): a
  detected language with a voice gets that voice, the listening locale's own where the languages
  agree; an undetermined text gets the listening language's voice; a detected language with no
  voice is `VoiceUnavailable::NoVoice`, which names the language and the platform's voice
  download page and leaves the answer on the screen (`keeper-core/src/voice/mod.rs:127-130`,
  `:165-168`; `keeper-core/src/voice/platform.rs:69`, `:84`). Every utterance is given its voice
  explicitly (`voice_macos.rs:1524-1533`; `voice_ios.rs:1673-1682`) and the chosen language and
  voice name are logged, so "utterance voice pl-PL" can be read from the console. (AD-182,
  AD-183, AD-188; FR-429…FR-432; Epic 64, Story 64.2)
- **Why the decision is the core's and the detector is the port's:** `choose_voice` is a pure
  function over (detected, listening, voices) with a truth table in its tests, and the port only
  enumerates, detects and obeys. That keeps the rule testable on the dev host, where the shell
  crate does not compile, and keeps `keeper-core` free of a platform `cfg`, a model weight or a
  network. (AD-183; `speech.rs:38-44`)
- **Why `NaturalLanguage` and not a Rust crate:** `objc2-natural-language 0.3.2` (`Zlib OR
  Apache-2.0 OR MIT`, `src-tauri/Cargo.toml:380`) binds a system framework that is on-device
  and has no network path, so `docs/egress.md` gains a paragraph and no destination. The Rust
  candidates were rejected on provenance: `whatlang`'s corpus terms are unverified and
  `lingua`'s models derive from a CC BY-NC corpus, which D-5's "no bundled weights" already
  refuses. (AD-188; Epic 64, *Facts this epic stands on*)
- **Why a missing voice is a refusal and not a fallback:** a wrong voice is exactly the failure
  this epic opens with, and AD-27 says absence, never a dead control. The answer stays on the
  screen, which the person can act on, and the sentence names where a voice is downloaded.
- **What is not chosen:** a *named* voice (Samantha, Zosia). The language is chosen and the
  system's default voice for it is used (`voiceWithLanguage:`); picking among a language's
  voices is DW-232.
- **Revisit triggers:** a platform whose synthesiser lists voices by something other than a
  BCP-47 language tag; the owner wanting a specific voice (DW-232). Neither reopens the first
  paragraph.
- **Status / owner:** decided. Owner is the architect; Epic 64, Story 64.2 implements it.

## D-10 — The voice block in the Bots pane is folded by default, and folded it is one line

keeper will show the desktop Bots pane's voice block — switch, phrase, language, the "Listens
in…" sentence, the note, the limits paragraph — folded to a single truthful line by default,
unfolded by one click that is remembered, and it will **not** remove any of those controls from
the pane or from Settings → Bots. Recorded here because the block did not exist on the Mac before
Epic 63 gave it a voice, so the epic that made the Mac talk also took the transcript's space,
and because "everything starts open" is the default someone will propose again.

- **What was measured:** with `dev/measure-bots.ts` on Chrome at 1440×900 over the dev shell,
  whose fixture is the owner's own case (a Polish system language with English-only on-device
  assets, so the refusal is on screen): before, the block was **223 px** and the transcript
  **259 px of the 872 px pane — 29.7 %**; folded, the block is a **39 px** line and the
  transcript **444 px — 50.9 %**. Unfolded by the person the block is 257 px and the transcript
  226 px (25.9 %) — the disclosure row is what the unfold costs, and unfolding is their choice.
  Every number is a `getBoundingClientRect` height (`src/components/bots/bots-pane.tsx:71-79`;
  `dev/measure-bots.ts:31-32`). (Epic 64, Story 64.1, measured on hesperia 2026-09-04)
- **What is built:** the folded line says what matters while a conversation is on screen —
  `Listening for "nixie" · en-US`, or `Listening off`, with the refusal's first clause when the
  port refuses (`src/components/bots/bot-voice-wake.tsx:128-146`, `voiceFoldedLine`). The fold
  is the repo's cookie-persisted fold idiom with its own cookie, `keeper_bots_pane_fold`,
  folded by default (`src/lib/stores/bots-pane-fold.ts:39`, `:63-65`) — not a key in the column
  fold's set, for the reason that file's header gives: the block is a band inside the transcript
  level, not a column. The layout contract test still pins exactly one `flex-1` child in the
  transcript level (`src/components/bots/bots-pane.test.tsx:628-636`, `:740-743`). Settings →
  Bots keeps the unfolded block, so folding removes no path to any
  control. (AD-184; FR-427, FR-428; Epic 64, Story 64.1)
- **Why folded by default:** the surface's one job is the transcript, and a default is a claim
  about what the surface is for. The notes rail's Files section is the precedent for a band
  that starts closed (`bots-pane-fold.ts:15-23`). A person who wants the block open opens it
  once and it stays open.
- **Why measured before and after, with the pane's own rig:** jsdom computes no layout, so a
  fold that "should" free space is unverifiable in the suite; the number is read from a real
  engine at the owner's viewport or it is not claimed. The rig is checked in beside the pane so
  the next band that folds is measured the same way.
- **Revisit triggers:** a second band in the pane folding (the store's `BOTS_PANE_BANDS` is a
  closed set of one); a pane taller than the block makes the fold moot, which no laptop is.
- **Status / owner:** decided. Owner is the architect; Epic 64, Story 64.1 implements it.

## D-11 — Hearing is shown by a floating window that can never become key, and it does not go over a full-screen Space

keeper will show that it hears — the turn's state, a level meter, the words as they arrive — in
a small, prewarmed, undecorated window that floats above other apps on every normal Space,
never becomes key, never activates keeper, and cannot be clicked; and it will **not** make that
window transparent, and will **not** — yet — make it appear over another app's full-screen
Space. Recorded here because the two refusals rest on facts about Tauri and macOS rather than
on taste, and both will be asked for again the first time the pill looks like a rectangle or
fails to show over a full-screen browser.

- **What is built:** a window of about 220×40 pt (AD-185) on the lifecycle `notes_window.rs` established for
  quick-capture — declared in `tauri.conf.json`, created hidden at boot, never destroyed, only
  shown and hidden (`crates/keeper/src/notes_window.rs:5-6`; the quick-capture declaration is
  `crates/keeper/tauri.conf.json:31-44`) — with `always_on_top`, `visible_on_all_workspaces`,
  `focusable(false)`, `focused(false)` and `set_ignore_cursor_events(true)`, so it floats over
  the app the person is talking into and cannot take a click. It exists only on the desktop,
  only when `voice_availability` is a real answer (D-8), and only while the turn is not
  idle-and-unarmed. It draws the state, a level the core's meter computes from the input tap's
  RMS and lets through at most every 40 ms and only on change (Story 64.3, AD-186;
  `keeper-core/src/voice/level.rs:52-59`, `:90`), and the heard words; under `prefers-reduced-motion`
  the meter is a static fill. The window module is Story 64.4's `crates/keeper/src/voice_window.rs`.
  (AD-185, AD-186; FR-436…FR-439, NFR-54; Epic 64, Story 64.4)
- **Why never key, and why click-through rather than merely unfocusable:** tao implements
  `focusable(false)` by overriding `canBecomeKeyWindow`/`canBecomeMainWindow`, which keeps the
  window from becoming key but does not stop a click on it from activating keeper — only
  `NSWindowStyleMaskNonactivatingPanel` does that, and tao's style mask never includes it. A
  click-through window cannot be clicked, so it cannot activate anything. (tao
  `src/platform_impl/macos/window.rs:414-431`, `:686-693`, `:966-969`, read 2026-09-04;
  Apple, `NSWindow.StyleMask.nonactivatingPanel`,
  `https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel`,
  accessed 2026-09-04)
- **Why not transparent:** `transparent(true)` on macOS requires Tauri's `macos-private-api`
  feature, enabled under `tauri > macOSPrivateApi`, and Tauri's own configuration documentation
  warns that using private APIs on macOS prevents App Store acceptance (tauri-utils
  `crates/tauri-utils/src/config.rs:2039-2042`,
  `https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-utils/src/config.rs`,
  accessed 2026-09-04). keeper enables neither — `tauri.conf.json` has no `macOSPrivateApi` and
  no Cargo manifest names the feature — and will not turn on a private-API flag for a rounded
  corner. The pill is opaque.
- **Why not over a full-screen Space, and what it would cost:** a plain `NSWindow` cannot be
  displayed over another app's full-screen window; `always_on_top` is
  `NSFloatingWindowLevel` only and does not set `fullScreenAuxiliary`, and
  `visible_on_all_workspaces` joins normal Spaces only. The route is an `NSPanel` subclass —
  `nonactivatingPanel` + `fullScreenAuxiliary` + `canJoinAllSpaces` + `hidesOnDeactivate = NO`
  (the last because an `NSPanel` hides when its app deactivates by default, which is the
  opposite of what the owner's case needs) — which `tauri-nspanel` provides at the price of a
  git dependency from a single maintainer whose `v2.1` branch turns on `macos-private-api` as a
  hard feature of tauri, whose `Cargo.toml` has no `license` field so keeper's licence firewall
  trips, and whose open defects abort the process on a style-mask call and on close. The owner's
  stated case — Maps, music, a browser in front — is normal Spaces, which the native builder
  covers. Deferred as DW-229 with the numbers. (AD-187; tauri-apps/tauri#11488, closed not
  planned, `https://github.com/tauri-apps/tauri/issues/11488`; Apple, *How Panels Work*,
  `https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/WinPanel/Concepts/UsingPanels.html`;
  both accessed 2026-09-04)
- **Revisit triggers:** `tauri-nspanel` publishing a licence field and closing its style-mask
  and close aborts (DW-229); Tauri growing a non-activating or full-screen-auxiliary option in
  its own builder, which would make the panel subclass unnecessary; keeper ever enabling
  `macOSPrivateApi` for a reason of its own, which would reopen only the transparency clause.
- **Status / owner:** decided. Owner is the architect; Epic 64, Story 64.4 implements the window,
  Story 64.5 records the limit.

## D-12 — The phone tier is the platform's, not the width's

keeper will render the phone tier — the single-pane stack — on a reduced-capability platform in
every orientation, size its root by the dynamic viewport, and treat a rotation as a resize; and it
will **not** choose the tier from the viewport's width on such a platform, or let the desktop
frame's cookies be written or read there. Recorded here because the width rule was right for
two years of desktop windows and wrong the first time a phone was turned sideways, and because
"just add a height breakpoint" is the fix that will be proposed next.

- **What was measured:** `src/hooks/use-shell-layout.ts` decided the phone tier by
  `(max-width: 767px)` and nothing else. An iPhone 14 Pro Max is 430×932 pt; rotated it is 932
  wide, so the **desktop tier** rendered at 430 pt tall — a rail, "MENU", a Conversation list
  column, the desktop pane's header and voice block, the Talk and Send controls clipped at the
  bottom edge. Reproduced on the simulator, then on the phone rig (`dev/measure-bots.ts --phone`
  and `--phone-landscape`, hesperia's headless Chrome, iPhone 14 Pro Max metrics, 2026-09-05),
  the Bots conversation open: before, 430×932 was the phone tier with the transcript 742 of 932
  px (79.6 %) and 932×430 was the desktop tier with the transcript 48 of 430 px (11.2 %); rotated
  from upright, the open conversation was gone and the transcript 24 px (5.6 %). After: both
  orientations are the phone tier, 79.6 % upright and 240 of 430 px (55.8 %) rotated — a 52 px
  back bar, a 37 px state line, a 102 px composer, the bottom edge at 430, inside the viewport —
  and rotating either way keeps the same conversation level open
  (`src/components/layout/app-shell.tsx:304-322`; `dev/measure-bots.ts:52-59`; Epic 65, *What he
  saw, and what was measured*, 1; commit `26816fb`).
- **What is built:** `useShellLayout` returns `phone: true` on a reduced-capability platform at
  every width, and keeps the width rule — below 768 px — only off such a platform, so the desktop
  dev harness and a narrow Mac window get the stack as they always have
  (`src/hooks/use-shell-layout.ts:18-23`, `:42`, `:84`; tests
  `src/hooks/use-shell-layout.test.ts:82-99` and `src/components/layout/app-shell.test.tsx:185`).
  The root is `h-dvh`, not `h-screen`: on a phone `100vh` is the largest viewport, the one with
  the browser chrome retracted, so a root sized by it ends below the screen; `100dvh` is the
  height the viewport has right now and follows a rotation (`app-shell.tsx:296-302`, `:323`).
  The four cookie hydrations — the sidebar fold, the panel strip, the Files tree, the surface
  columns — never run on the phone tier, because all four are desktop state, and until this
  decision the rotated desktop frame wrote them and the phone tier read them back
  (`app-shell.tsx:138-188`). The platform answer is `useIsReducedCapabilityPlatform`, computed
  from `CapabilitiesVm` — the same capability that gates every other tier-telling surface
  (AD-137). (AD-189, AD-195; FR-441…FR-443; Epic 65, Story 65.1)
- **Why the platform and not a height breakpoint:** a height rule would make the tier flip on
  the keyboard, on the browser chrome, on a split-screen Mac window, and would still let a
  landscape phone write desktop cookies for the few frames before it flipped. The phone is a
  fact about the build, already answered in `CapabilitiesVm` and already the thing that decides
  what the phone tier lacks (AD-31, AD-137); the width was only ever a proxy for it. Before Rust
  has answered `capabilities()` the width decides alone — the honest default, because it is
  what every build rendered until now and it corrects itself the moment the answer lands, where
  the reverse guess would flash the stack over every desktop window
  (`use-shell-layout.ts:25-32`).
- **Why measured in both orientations with the pane's own rig:** the owner's report was a
  rotation, and jsdom computes no layout, so the rig that measured the Mac's fold (D-10) grew a
  phone mode that emulates the device, loads the app with `?platform=phone` so the mock shell
  answers the iPhone's capabilities, measures, rotates the emulated device and measures again
  (`dev/measure-bots.ts:34-50`; AD-195). A number from a real engine in both orientations is the
  acceptance; a screenshot is the picture.
- **What is not decided:** a tablet tier. An iPad rotates too, and at 1024 pt wide the width rule
  would render the desktop frame on a reduced-capability platform that this decision now calls
  the phone tier; whether that is right for an iPad is its own decision, recorded as DW-234
  (Epic 65, *What stays out*).
- **Revisit triggers:** an iPad build (DW-234); a platform that is reduced in capability and
  yet wants columns. Neither reopens the first paragraph for the phone.
- **Status / owner:** decided. Owner is the architect; Epic 65, Story 65.1 implements it.

## D-13 — The switch persists what the person chose, shows what the port did, and keeper re-arms itself

keeper will keep `bots.wake_enabled` as the person's intent, show a refusal at arming time
beside the switch as a refusal with its reason and remedy, and arm the phrase again — without
being asked — when the refusal clears; and it will **not** persist a "no" the person did not
say, or retry a port that still refuses. Recorded here because the first phone build saved
"no" silently and nobody could tell, because the fix needed a core defect found only by the
re-arm test, and because "if it failed, turn the switch off" is the tidy-looking rule that
will be proposed again.

- **What was wrong:** `bot-voice-wake.tsx` wrote `voiceWakeSet(enabled && unavailable === null)`,
  so toggling on while the port refused — a Polish system language with no on-device asset, a
  microphone not yet granted — persisted OFF and rendered the switch off; nothing re-armed when
  the refusal cleared, and a grant given through the switch did not clear the stale sentence.
  The phrase on the owner's phone was never armed (Epic 65, *What he saw, and what was
  measured*, 2; `src/components/bots/bot-voice-wake.tsx:51-60`, which records the old rule).
- **What is built:** the switch writes what the person chose, whatever the port answered
  (`voice_ipc.rs:532-533` persists the phrase and the intent before arming; `bot-voice-wake.tsx:
  221-236`); a refusal is rendered beside the switch — the availability sentence, or the turn's
  own `failed` reason when the probe did not predict it — with the switch on
  (`bot-voice-wake.tsx:350-366`; the refusal is also pushed to the phone's record,
  `voice_ipc.rs:508`). The rule "arm again now?" is one pure function in the core,
  `keeper_core::voice::should_rearm(intent, armed, refusal_cleared)`: never with the intent off,
  never while `Turn::armed` already holds, never while the port still refuses
  (`keeper-core/src/voice/mod.rs:576-589`; `Turn::armed`, `:386-395`). The shell gathers the
  three facts — the registry's `bots.wake_enabled`, the turn, the port's availability — in
  `rearm_locked` (`src-tauri/crates/keeper/src/voice_ipc.rs:586-593`) and runs it on a grant
  (`voice_authorize`), on a language change that makes the locale capable (`voice_locale_set`),
  on keeper coming back in front (`RunEvent::Resumed` on the phone, the main window's focus on
  the desktop — `lib.rs:1372`, `:1539-1540`, through `voice_ipc::voice_rearm`, `voice_ipc.rs:601-611`)
  and on the port's own resume. On the phone's face the setting, the refusal's first clause and
  the live state are one caption above the composer (`src/components/bots/bots-phone-pane.tsx:
  38-44`, `:169-191`; the desktop's equivalent is D-10's folded line). (AD-190, AD-191;
  FR-444…FR-447; Epic 65, Story 65.2)
- **The core defect the frontend fix alone would not have reached:** `Turn::set_wake` from
  `Failed` recorded the phrase and opened nothing — the arm after a refusal left the turn in
  `Failed` with the device released, so every later toggle was inert however the switch was
  drawn. `set_wake` now treats `Failed` as `Idle` with a reason attached: set moves to `Idle`
  and opens the device, clear moves to `Idle` and releases it, and the stale reason goes with
  either (`keeper-core/src/voice/mod.rs:397-421`). The proof is by mutation: reverting that
  line fails `voice_driver_rearms_from_a_refusal_once_it_clears`
  (`keeper-core/tests/voice_turn.rs:1099-1159` — refused arm leaves `Failed` with the intent
  kept and `armed()` false; `should_rearm` says no while refused and yes once cleared; the
  second arm yields `OpenMicrophone`, `Idle`, `armed()` true, and a third foreground changes
  nothing), which the wave A commit `26816fb` records as run. The companion
  `voice_driver_clearing_the_phrase_from_a_refusal_rests_idle` (`:1164`) pins the other half.
- **Why intent and not outcome:** the switch is the one place the person says what they want,
  and a port's refusal is a fact about the device now — a language not yet downloaded, a
  permission not yet given — that the person is about to change. A switch that flips itself off
  turns the person's "yes" into a "no" they have to notice and repeat, after doing the very
  thing keeper asked of them. Absence, never a dead control (AD-27) cuts the same way: a switch
  drawn on while nothing listens must say why beside itself, and it does.
- **Why never re-arm while the port still refuses:** a refusal retried on every foreground
  would be a loop — the port that refused would refuse again — which is the reason the turn's
  own end never re-arms from `Failed` (`mod.rs:308-311`;
  `voice_driver_does_not_rearm_after_failure_because_the_port_would_refuse_again`,
  `voice_turn.rs:507`). The third argument to `should_rearm` is what keeps the two rules from
  contradicting each other.
- **Revisit triggers:** a refusal that clears without any of the four triggers firing (none is
  known: a grant, a language, a foreground and a resume are the four ways a refusal ends);
  a platform where availability cannot be probed cheaply enough to ask on every foreground.
  Neither reopens the first paragraph.
- **Status / owner:** decided. Owner is the architect; Epic 65, Story 65.2 implements it.

## D-14 — The island is Swift, because Swift is the only door; the free team's costs are paid knowingly

keeper will show its name and its ear's state — armed, listening, heard, speaking — in the
Dynamic Island and on the Lock Screen through a Live Activity, built as the first Swift in the
repository: a `@_cdecl` bridge in the app target and a widget-extension target with the four
presentations; and it will **not** push-update it, will **not** take an App Group for it, and
will **not** claim it works on a free Personal Team until the extension has signed, installed
and been photographed on kalypso. Recorded here because "no Swift in this repo" was a real
discipline until now, because the decision rests on a fact about ActivityKit that is easy to
re-litigate, and because each free-team cost will be rediscovered the next time a profile
lapses.

- **What the owner has today, and why it is not enough:** the orange dot. It is the system's
  privacy indicator, driven by microphone use and not by keeper; it says *some* app holds the
  microphone, it never appears on the Lock Screen, Apple names the app only in Control Center,
  and no tap on it opens keeper. Voice Memos gets its name in the island because Apple's app
  ships a Live Activity, not because it records (Apple Support 108331,
  `https://support.apple.com/en-us/108331`; Apple, *View Live Activities in the Dynamic
  Island*; both read 2026-09-05; Epic 65 research, §4; DW-222).
- **The fact the shape rests on:** ActivityKit is Swift-only. `Activity<Attributes>` is a
  generic Swift class whose symbol is Swift-mangled (`s:11ActivityKit0A0C`) and which has no
  `@objc` surface, so there is no Objective-C runtime class for `objc2` to message — compare
  `SFSpeechRecognizer`, `c:objc(cs)SFSpeechRecognizer`, which every voice port reaches that way
  (Apple, `https://developer.apple.com/documentation/activitykit/activity`, read 2026-09-05).
  The Rust-only route does not exist; the choice was Swift or nothing.
- **What is built:** `Sources/keeper/KeeperIsland.swift`, compiled into the app target, exports
  `keeper_island_start` / `_update` / `_end` / `_free` with `@_cdecl`; `crates/keeper/src/
  voice_island.rs` (`#[cfg(target_os = "ios")]`) declares them `extern "C"` and calls them from
  the one hook in `voice_ipc::push`, so the activity is started when the phrase is armed,
  updated on every turn state, ended on disarm, and — because Apple ends every activity at eight
  hours — ended and requested again at 7 h 45 m while still armed
  (`keeper_core::voice::island::RENEW_AFTER`, `keeper-core/src/voice/island.rs:43`); a request
  refused in the background is recorded once as `island:refused` and retried on
  `RunEvent::Resumed`. The decisions — the state word, the renewal, the 30 s a failure card
  lingers (`FAILURE_LINGER`, `:38`) — are the core's, pure and tested on the dev host; the bridge
  is three audited `#[allow(unsafe_code)]` fns over one `extern "C"` block
  (`voice_island.rs:182-187`, `:197-238`; `docs/constraints-and-limitations.md`). The extension's
  deployment target is 16.2, above the app's 16.0, because `ActivityContent` is a 16.2 API; an
  older phone gets a refusal sentence and nothing else changes
  (`gen/apple/project.yml:175`; `KeeperIsland.swift:140-141`). `KeeperIsland/KeeperIslandAttributes.swift` is
  the `ActivityAttributes`, a member of both targets; `KeeperIsland/KeeperIslandLiveActivity.swift`
  holds the `ActivityConfiguration` with the Lock Screen, compact leading/trailing, minimal and
  expanded presentations Apple makes mandatory; the `KeeperIsland` target
  (`com.apple.widgetkit-extension`, `dev.tgorka.keeper.island`) is declared in `project.yml`
  and embedded by `keeper_iOS`; `NSSupportsLiveActivities` is in `project.yml` and restated in
  `Info.ios.plist`. The `extern` block compiles on the Linux host and only the Mac link proves
  it, so the Mac gate is the first compile of every Swift line. (AD-194; FR-453…FR-455, NFR-56;
  Epic 65, Story 65.5; `docs/ios.md`, *Live Activity*)
- **Why the app process updates it, and never a push:** `Activity.request(…, pushType: nil)` is
  accepted and updates from the app need no APNs, no push entitlement and no token (Apple,
  *Displaying live data with Live Activities*, read 2026-09-05). Push would be a new
  destination, which AD-196 and `docs/egress.md` refuse, and the free tier cannot hold the push
  capability anyway (Apple, *Supported capabilities (iOS)*, read 2026-09-05). Recorded as
  DW-235.
- **Why no App Group:** Apple's Live Activity guide never mentions one; data flows through
  ActivityKit, and the shared attributes file is compiled into both targets. An App Group is
  needed only if the extension read the app's files, which an indicator does not — and the
  third-party Tauri guidance that says widgets need a paid team is about App Groups, not
  activities (tauri-plugin-widgets iOS setup, read 2026-09-05; Epic 65 research, §3).
- **The free team's costs, stated so they are chosen:** a second App ID against the free
  tier's budget of roughly ten per seven days; a second automatic profile that lapses with the
  app's every seven days and is re-signed by the same ritual; the four presentations in
  SwiftUI; the eight-hour end; a foreground-only start, which the arm tap satisfies; and the
  residual risk that a Personal-Team automatic profile trips
  `ActivityAuthorizationError.unentitled` — Apple's table says nothing is needed, and the same
  table mis-predicted Data Protection for this team on 2026-09-03 (`docs/ios.md`, *Signing on
  a free Personal Team*). No new environment variable: the `KEEPER_IOS_REGISTER_DEVICE=1`
  build already mints both profiles because the `keeper_iOS` scheme depends on the extension,
  and the install script prints whether the `.appex` and `NSSupportsLiveActivities` are in the
  installed bundle. (`docs/ios.md`, *Live Activity*; Epic 65 research, §3 and *Verdict* (b))
- **The refusal rule:** if the free team refuses to sign the extension, the refusal is measured
  — the exact xcodebuild sentence, the profile it asked for — and the story ends in a DW row
  with that measurement, never in a quietly narrowed feature (AD-194). Until the coordinator's
  install report and the owner's photograph exist, DW-222 stays open and this decision claims
  a design, not a working island.
- **Revisit triggers:** Apple exposing ActivityKit to Objective-C (none announced); a paid team,
  which reopens only the push clause and only under AD-196's egress rule; the free-team
  refusal measuring true, which reopens the whole shape. None reopens the first paragraph's
  "Swift where Swift is the only door".
- **Status / owner:** decided. Owner is the architect; Epic 65, Story 65.5 implements it; the
  hardware proof is Story 65.6's gate.

# Constraints & Known Limitations

Honest list of what Keeper cannot do, should not do, or will find hard. Sources: the 2026-07
technical and market research reports in `_bmad-output/planning-artifacts/` and the PRD/brief
risk sections.

## Cannot do (hard blockers)

- **Beeper on-device chats are invisible.** Since Beeper's 2025 relaunch, networks connected
  via *on-device* connections in the official Beeper apps (WhatsApp, Signal, expanding) never
  reach Beeper's Matrix homeserver — no third-party Matrix client can see them. Keeper sees
  Matrix-native chats, Beeper *cloud* bridges, and bbctl self-hosted bridges. Parity for
  on-device networks = run your own mautrix bridges.
- **iMessage without your own Mac.** Every relay/spoof approach is dead (Beeper Mini
  precedent). Supported path: `beeper/platform-imessage` (MIT) running on the user's own Mac.
- **Cross-protocol server-side drafts.** Keeper is client-only; drafts live locally +
  mirrored to per-room Matrix account data. A "draft bridge"/server app would be a separate
  project by design.

## Should not do (deliberate policy)

- **No AGPL code reuse.** Element Web/X, Cinny, gomuks, Fractal, Nheko, and mautrix bridges
  are study-only references. cargo-deny enforces an Apache/MIT-compatible dependency tree.
- **No hosted anything.** No relay servers, no credential custody, no hosted bridges — ToS
  and liability stay with the user, which is the project's legal posture.
  [deploy/companion-stack](../deploy/companion-stack/) provides self-hosting configs only
  (Synapse + mautrix bridges operated by the user; keeper hosts nothing).
- **No WhatsApp automation features** (bulk send, auto-reply) — ban-bait for bridge users.
- **No native VoIP implementation now.** MatrixRTC (MSC4143) is pre-spec and volatile;
  calls arrive post-MVP via the Element Call widget.
- **No paid Apple Developer Program yet** (so no APNs push, NSE, TestFlight, or App Groups) —
  deferred by decision; see [decisions.md](decisions.md) D-1.

## Hard / risky (do with eyes open)

- **Beeper login (email code → JWT)** is a private, unversioned API ported from Apache-2.0
  bbctl. It can break without notice; isolated behind an auth-provider trait (AD-17).
- **WhatsApp via mautrix**: personal-use bridging rarely triggers bans, but ToS risk is real
  and sits with the user (risk-tier UI makes this explicit).
- **X/Twitter, Instagram/Messenger, LinkedIn** bridges are volatile (API lockdowns, Meta
  changes). Treated as best-effort tiers, never release gates.
- **matrix-rust-sdk 0.x churn**: breaking API changes every release; we track releases and
  pin exact versions.
- **Tauri mobile (iOS/iPad/Android)** shipped but is younger than desktop; the Rust core is
  the portable asset (AD-24), and an iOS walking-skeleton spike should happen early in the
  mobile phase.
- **The Live Activity is the repository's first Swift, and a second App ID on the free
  team** (Epic 65, Story 65.5, AD-194; `docs/decisions.md` D-14). ActivityKit has no
  Objective-C surface, so `objc2` cannot reach it and the island is a `@_cdecl` bridge in
  the app target plus a `KeeperIsland` widget-extension target in `gen/apple/project.yml`,
  compiled only on the Mac gate — the Linux host proves the Rust `extern` block and nothing
  else. On a free Personal Team the extension's bundle id `dev.tgorka.keeper.island` is a
  second App ID against the ~10-per-7-days budget with its own 7-day profile, and whether
  the automatic profile refuses it (`ActivityAuthorizationError.unentitled`) is proven only
  by the install on kalypso — Apple's capability table already mis-predicted Data Protection
  for this team (`docs/ios.md`, *Signing on a free Personal Team*, *Live Activity*). Until
  the install report and a photograph exist, DW-222 is open and the island is a design.
- **E-mail, AI-bot client, terminal client** are future-phase items tracked in the PRD's
  post-MVP section, not storied yet.

## Linux desktop (verified 2026-08-02 on Ubuntu 26.04 + XFCE/ayatana)

keeper builds and runs on `x86_64-unknown-linux-gnu` and was exercised end to end against a
self-hosted Synapse (login, MSC4186 probe, sync supervisor, tray). What is different there,
and why:

- **Secrets live in the kernel session keyring, not a vault.** The Linux build selects
  keyring's `linux-native` (keyutils) backend: no D-Bus, no Secret Service daemon, so it
  works headless, in containers and in CI. The cost is scope — keyutils keys belong to the
  login session, so ending the session or rebooting drops the access token and keeper asks
  for a fresh login. Moving to `linux-native-sync-persistent` (keyutils backed by a Secret
  Service) would survive a reboot but requires gnome-keyring/KWallet to be running, which a
  container does not have.
- **The tray glyph is repainted, not templated.** `icon_as_template` is macOS-only, so the
  authored monochrome-black template would be a black-on-black silhouette on the default
  dark panel. Off macOS `tray::tray_glyph` repaints the same shape in the brand tone with a
  one-pixel contrast halo. State still reads from SHAPE, never colour.
- **A tray menu cannot be replaced once set.** Documented Tauri/ayatana behaviour: the menu
  installed when the tray is created is the menu the user gets, and only its item *content*
  can change afterwards. Recording is macOS-only so its menu never appears here, but any new
  tray affordance must be present in the first-built menu (AD-61).
- **No tray click events.** `TrayIconEvent` and `show_menu_on_left_click` are unsupported on
  Linux; every tray affordance must be a menu item.
- **Global shortcuts are X11-only in practice.** Registration succeeds under X11; Wayland
  compositors without the global-shortcuts portal refuse it, and keeper logs
  `hotkey: OS refused to register global shortcut` and carries on without one.
- **No recorder sidecar.** `keeper-rec` is Apple-Silicon macOS only, so `externalBin` is
  emptied by `tauri.linux.conf.json` and `CapabilitiesVm.recording` is false.
- **`$XDG_RUNTIME_DIR/tray-icon` must be writable by the app's user.** The tray backend
  writes the icon there; a directory left behind by another user (e.g. a root-run RustDesk
  in the same container) makes the tray fail with `Permission denied` — logged at `warn`,
  non-fatal, no tray.

## Audited `unsafe` FFI inventory (shell crate only)

Policy (2026-07-11): `unsafe_code` stays denied workspace-wide; the `keeper` shell crate may
carry function-level, audited `#[allow(unsafe_code)]` exceptions for platform FFI with no
safe binding. Current inventory:

- iOS backup exclusion (`NSURL.setResourceValue(NSURLIsExcludedFromBackupKey)`) via
  objc2-foundation, behind `Platform::exclude_from_backup` — the single function-level
  `#[allow(unsafe_code)]` in `IosPlatform::exclude_from_backup`,
  `crates/keeper/src/ipc.rs` — story 14.7 (FR-65).
- macOS PDF export (`WKWebView createPDFWithConfiguration:completionHandler:`) via
  objc2-web-kit, behind `pdf_export::capture`, `crates/keeper/src/pdf_export.rs` — the
  second entry, which called itself that in its own doc comment before this list said so.
- iOS voice port (Epic 62, Stories 62.4 and 62.6, FR-401–FR-403, FR-408, NFR-50/NFR-51):
  `SFSpeechRecognizer` (`authorizationStatus`, `requestAuthorization:`, `initWithLocale:`,
  `supportsOnDeviceRecognition`, `isAvailable`, `supportedLocales` — enumerated once and
  cached, one recogniser per locale, so the person can choose a language that runs on
  the device (Epic 63), `recognitionTaskWithRequest:resultHandler:`),
  `SFSpeechAudioBufferRecognitionRequest` (`setRequiresOnDeviceRecognition:` — always
  `true`, `appendAudioPCMBuffer:`, `endAudio`), `SFSpeechRecognitionTask cancel`,
  `AVAudioSession` (`sharedInstance`, `isInputAvailable`, `recordPermission`,
  `requestRecordPermission:`, `setCategory:mode:options:error:`, `setActive:…`),
  `AVAudioEngine` (`inputNode`, `setVoiceProcessingEnabled:error:`, `installTapOnBus:…`,
  `removeTapOnBus:`, `prepare`, `startAndReturnError:`, `stop`, `isRunning`),
  `AVSpeechSynthesizer` (`speakUtterance:`, `stopSpeakingAtBoundary:`, `isSpeaking`) and
  `NSNotificationCenter` (`addObserverForName:object:queue:usingBlock:` for
  `AVAudioSessionInterruptionNotification`, `AVAudioSessionMediaServicesWereResetNotification`,
  `AVAudioEngineConfigurationChangeNotification` and, since Epic 65,
  `AVAudioSessionRouteChangeNotification`, `removeObserver:`) via objc2-speech,
  objc2-avf-audio and objc2-foundation, behind `keeper_core::voice::VoicePort` and
  `keeper_core::voice::ConsentPort` — twenty-eight function-level `#[allow(unsafe_code)]`
  fns in `crates/keeper/src/voice_ios.rs`, one per concern (`speech_authorization`,
  `microphone`, `microphone_consent`, `ask_speech`, `ask_microphone`,
  `on_device_locales`, `recognizer_for`, `configure_session`, `set_session_options`,
  `release_session`, `start_capture`, `stop_capture`, `engine_running`, `start_request`,
  `end_request`, `read_result`, `observe_session`, `route_change_reason`,
  `interruption_notice`, `forget_observer`, `new_synthesizer`, `voice_languages`,
  `voice_for_language`, `voice_name`, `detect_language`, `speak_text`, `stop_speech`,
  `is_speaking`), each
  with a `// SAFETY:` comment citing the Apple contract it relies on. `start_capture`
  also carries `#[allow(clippy::arc_with_non_send_sync)]`: the request slot the audio tap
  reads is shared with the audio thread inside a block, which Rust cannot see — the
  SAFETY comment is the argument. No server recognition path exists in the file, and the
  port never prompts on its own: the two permission dialogs are reached only through
  `ConsentPort`, whose timing is decided in `keeper_core::voice::authorization`.
- The background made explicit, iOS port (Epic 65, Story 65.4, FR-450–FR-452, NFR-55,
  AD-193): one new audited fn, `route_change_reason` (`crates/keeper/src/voice_ios.rs:1705-1731`)
  — reads `AVAudioSessionRouteChangeReasonKey` from a route-change notification's `userInfo`
  into the framework's own name for the reason (headphones, a car kit, the speaker), for the
  phone's record; and `observe_session` (`:1635-1699`) grows a third observer, on
  `AVAudioSessionRouteChangeNotification` (`:1681`), counted in the twenty-eight above. No
  new class is reached. What changed in behaviour rather than in FFI (module doc `:51-82`;
  `Worker::resume` `:882-909`, `Worker::give_up` `:914-918`, `Worker::on_audio` `:933-1022`):
  a resume after an interruption is bounded — `RESUME_FAILURES_TOLERATED` (`:230`), twelve tries at the
  existing 5 s `RESUME_RETRY`, a minute — after which the port records `refused` in the ring
  and tells the turn, so a session the system will not give back (an accepted call, a
  suspension) reads as a refusal with its remedy (open keeper; D-13 re-arms on
  `RunEvent::Resumed`) rather than as silence. The limits sentence the port shows is
  rewritten from Apple's documented interruptions (`keeper-core/src/voice/platform.rs:82`;
  `docs/ios.md`, *Limitations*). **Not yet run on hardware where it matters**: the
  background run on kalypso — armed, another app in front, three minutes, the phrase heard;
  a call accepted and ended — is the story's acceptance and is owed in DW-228 (its iOS
  half); until it is recorded, the sentence is what Apple documents, not what was observed.
- The Live Activity bridge, iOS only (Epic 65, Story 65.5, FR-453–FR-455, NFR-56, AD-194):
  the first FFI in the shell crate that is **not** objc2 — ActivityKit has no Objective-C
  surface, so `crates/keeper/src/voice_island.rs` (`#[cfg(target_os = "ios")]`) declares one
  `extern "C"` block (`:182-187`: `keeper_island_start`, `keeper_island_update`,
  `keeper_island_end`, `keeper_island_free`) against `@_cdecl` functions compiled into the
  app target from `gen/apple/Sources/keeper/KeeperIsland.swift`, and three function-level
  `#[allow(unsafe_code)]` fns, one per call — `bridge_start` (`:197-218`), `bridge_update`
  (`:221-228`), `bridge_end` (`:231-238`) — each with a `// SAFETY:` comment: NUL-terminated
  buffers that outlive the call, copied on the Swift side; the one string that crosses back
  (the refusal sentence from `start`) is `strdup`'d by Swift and released by
  `keeper_island_free`, the ownership stated in the Swift file's header. `libapp.a` is a
  static archive the Xcode project links, so the four symbols resolve against the app's own
  Swift objects at link time; the `extern` block compiles on the Linux host and only the
  Mac link proves it exists. What is decided — the state word per turn state, when to renew
  before Apple's eight-hour end (`RENEW_AFTER`, 7 h 45 m,
  `keeper-core/src/voice/island.rs:43`), how long a failure card lingers (`FAILURE_LINGER`,
  30 s, `:38`) — is `keeper_core::voice::island`, pure and tested on the dev host, with no
  platform `cfg` and no `tauri`; the bridge only carries it. Every call is local
  (`pushType: nil`, no APNs, no App Group — `docs/egress.md`, *The phone's record and the
  island add no egress*), and a request the system refuses (in the background, on a free
  team) is recorded once in the ring as `island:refused` and retried on `RunEvent::Resumed`
  rather than looped. **Not yet compiled anywhere**: the Swift, the extension target and
  the link are proven only by the Mac gate and the kalypso install (DW-222).
- The island's colours are Swift literals the design gate cannot see (Epic 65, Story 65.5):
  `gen/apple/KeeperIsland/KeeperIslandLiveActivity.swift` carries the dark set's hexes —
  ground, text, text-dim, accent, ok, danger — as SwiftUI `Color` literals with the token
  names in comments, because there is no Swift token file and `check:design` scans no Swift.
  A retune of `DESIGN.md`'s dark set does not reach the card until someone edits those
  literals by hand; the comment names the token so the edit is a lookup, not a guess.
- macOS voice port (Epic 63, Story 63.4, FR-417–FR-419, FR-408, NFR-53, AD-175):
  `SFSpeechRecognizer` (`authorizationStatus`, `requestAuthorization:`, `initWithLocale:`,
  `supportsOnDeviceRecognition`, `isAvailable`, `supportedLocales` — enumerated once and
  cached (0.41 s for 63 locales on hesperia), refreshed when the person changes
  `bots.voice_locale`, so the recogniser is built for a locale that runs on this Mac
  rather than for the system locale (Epic 63; `localeWithLocaleIdentifier:` and
  `currentLocale` are safe bindings), `recognitionTaskWithRequest:resultHandler:`),
  `SFSpeechAudioBufferRecognitionRequest` (`setRequiresOnDeviceRecognition:` — always
  `true`, `appendAudioPCMBuffer:`, `endAudio`), `SFSpeechRecognitionTask cancel`,
  `AVAudioApplication` (`sharedInstance`, `recordPermission`,
  `requestRecordPermissionWithCompletionHandler:` — macOS 14+; on 11–13 the class is
  absent and the port reports the microphone grant as denied, at error level, rather than
  guess), `AVAudioEngine` (`inputNode`, `inputFormatForBus:` for the no-input-device check,
  `outputFormatForBus:`, `installTapOnBus:…`, `removeTapOnBus:`, `prepare`,
  `startAndReturnError:`, `stop`, `isRunning`), `AVSpeechSynthesizer` (`speakUtterance:`,
  `stopSpeakingAtBoundary:`, `isSpeaking`), `NSNotificationCenter`
  (`addObserverForName:object:queue:usingBlock:` for
  `AVAudioEngineConfigurationChangeNotification`, `removeObserver:`) and `NSProcessInfo`
  (`endActivity:` for the App Nap assertion; `beginActivityWithOptions:reason:` is a safe
  binding) via objc2-speech, objc2-avf-audio and objc2-foundation, behind
  `keeper_core::voice::VoicePort` and `keeper_core::voice::ConsentPort` — twenty-two
  function-level `#[allow(unsafe_code)]` fns in `crates/keeper/src/voice_macos.rs`, one per
  concern (`speech_authorization`, `microphone_consent`, `ask_speech`, `ask_microphone`,
  `input_present`, `on_device_locales`, `recognizer_for`, `start_capture`, `stop_capture`,
  `engine_running`, `start_request`, `end_request`, `read_result`, `forget_observer`,
  `new_synthesizer`, `voice_languages`, `voice_for_language`, `voice_name`,
  `detect_language`, `speak_text`, `stop_speech`, `is_speaking`), each with a `// SAFETY:`
  comment citing the Apple contract it relies on. `start_capture` also carries
  `#[allow(clippy::arc_with_non_send_sync)]` for the same request slot as the iOS port. No
  `AVAudioSession` call exists in the file — the class does not exist on macOS — so there
  is no category, no ducking and no interruption observer; half-duplex is
  `keeper_core::voice::may_record`'s, and the port never records while it speaks. No
  server recognition path exists in the file, and the port never prompts on its own.
  **Not yet compiled where it was written**: the file was authored on a Linux host and
  gated by `keeper-core`'s platform-neutral voice tests and the on-device source scan;
  its first `cargo clippy --target aarch64-apple-darwin` and first run are on the Mac.
- Voice selection and on-device language detection, both ports (Epic 64, Story 64.2,
  FR-429–FR-432, AD-182, AD-183, AD-188): `AVSpeechSynthesisVoice` (`speechVoices` —
  enumerated once and cached beside the locales, refreshed when the person changes
  `bots.voice_locale`; 180 voices on hesperia, one Polish — `language`, `name`, and
  `voiceWithLanguage:`, whose `nil` is refused as `VoiceUnavailable::NoVoice` naming the
  language and the platform's voice-download page, never a fall-through to the default
  voice), `AVSpeechUtterance setVoice:` (set on every utterance; no utterance is spoken
  without an explicit voice), and `NLLanguageRecognizer` (`new`, `setLanguageConstraints:`
  — the languages this device has voices for plus the listening one, so the answer is
  classified only among languages it could be spoken in — `processString:`,
  `dominantLanguage`) via objc2-avf-audio and objc2-natural-language 0.3.2 (`Zlib OR
  Apache-2.0 OR MIT`, features `NLLanguage` + `NLLanguageRecognizer`, objc2 ≥ 0.6.2,
  objc2-foundation ^0.3.2), behind `keeper_core::voice::VoicePort::{voices, listening,
  detect_language, speak}` — the four fns `voice_languages`, `voice_for_language`,
  `voice_name`, `detect_language` and the changed `speak_text` in each of
  `voice_macos.rs` and `voice_ios.rs`, counted in the totals above, each with a
  `// SAFETY:` comment. `NaturalLanguage` is a system framework with no network path:
  no answer text leaves the device to be classified, and `docs/egress.md` gains no
  destination. Which voice is used is `keeper_core::voice::speech::choose_voice`'s
  decision over (detected, listening, voices) — a pure function with a truth table —
  and the ports only enumerate, detect and obey (AD-183).
- The input level, both ports (Epic 64, Story 64.3, FR-433–FR-435, AD-186): `AVAudioPCMBuffer`
  (`floatChannelData` — `nil` when the buffer is not 32-bit float, checked rather than
  assumed, `frameLength`, `stride`) read inside the existing tap block of `start_capture`,
  plus one `std::slice::from_raw_parts` over `frameLength × stride` samples that lives only
  for the duration of the tap call and is never stored, via objc2-avf-audio — no new
  `#[allow(unsafe_code)]` fn: the calls sit in the `start_capture` already counted in each
  port's total (`crates/keeper/src/voice_macos.rs:1273-1282`; `voice_ios.rs:1355-1364`), and
  the SAFETY comment on that fn carries the buffer contract (`voice_macos.rs:1248-1256`;
  `voice_ios.rs:1328-1336`). The recogniser receives every buffer, unchanged, before the meter
  reads it; the RMS goes to `keeper_core::voice::level::Meter`, which decides the scale
  (−60 dBFS floor), the smoothing (40 ms attack, 250 ms release) and the rate (at most one
  reading every 40 ms, only on change), so the audio thread sends at most ~25 messages a
  second and never one per buffer. Only a number in `0.0..=1.0` leaves the tap; no sample
  does. There is no second tap: `installTapOnBus:` allows one per bus, and the level shares
  the recogniser's.

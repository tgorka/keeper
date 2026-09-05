# Epic 64 — A voice that shows itself, and speaks a language it has

created: '2026-09-04'
source: the owner, after using epic 63's macOS voice for real and sending three screenshots — "jak nixi odpowiada po polsku i syntezuje po ang to nie jest to zrozumiale … zrob [górną część] foldable (i folded by default) … nie widze ze keeper slyszy mnie i przetwarza rozmowe — wez inspiracje z superwhisper". Grounded by one read-only repository map and two external research passes, 2026-09-04, plus two probes run on hesperia the same day.
binds: FR-427…FR-440 (allocated here); NFR-54 (allocated here); AD-182…AD-188 (new); AD-27 (absence, never a dead control — consumed), AD-55/AD-56 (decisions in `keeper-core`, the shell is a call site — consumed), AD-175 (half duplex on the Mac — consumed), AD-179 (voice is a runtime answer — consumed), NFR-11 (zero egress — the constraint this epic is designed around)
see-also: epic 63 (the macOS port, the chosen recognition language, the reach surfaces). The two screenshots that started this are described in "What he saw".

## What he saw

1. He said "What's the weather" (recognition on his Mac is on-device and **English-only**: of 63 locales, four support on-device recognition there, all English). The Hermes bot `nixie` answered in **Polish**. keeper read that Polish aloud with the **default system voice — English** — and it was unintelligible. A probe on the same Mac: 180 synthesiser voices installed, **one of them Polish**. So the Mac could have spoken that answer; keeper never asked it to.
2. The desktop Bots pane: header, pins, picker, grant sentence, and the whole wake block — switch, phrase, language, the "Listens in…" line, the note, the limits paragraph — sit above the transcript. From Tailwind classes that is ≈ 415–500 px of chrome, of which the wake block alone is 210–260 px. The transcript is the remainder. This band did not exist on the Mac before epic 63: the desktop port answered `unsupported` and the block was absent by AD-27, so the epic that gave the Mac a voice also took the transcript's space.
3. While a voice turn ran, the only signs were a `text-xs` "Listening" line above the composer and a button reading "Cancel this question". Nothing moved. Nothing showed that sound was arriving, that words were being recognised, or that a model was thinking. He named superwhisper, whose recording window shows a live waveform *as a diagnostic* ("if the waveform stays static, check your input device") and a colour-coded state dot.

## The one sentence

**keeper's voice on the Mac works, and is mute about itself and deaf to language: it cannot show that it hears, it reads Polish in an English voice, and its own controls crush the transcript they serve.**

## Facts this epic stands on (measured, not assumed)

- Recognition and synthesis are **different inventories**. Recognition on-device: 4 locales on hesperia, all English. Synthesis: 180 voices, Polish among them. The listening language does not bound the speaking language.
- The utterance is built with **no voice**: `voice_macos.rs:1300-1308` and `voice_ios.rs:1458-1466` call `AVSpeechUtterance::initWithString` + `speakUtterance` and never `setVoice`; `voiceWithLanguage` and `speechVoices` are bound (`objc2-avf-audio`, feature `AVSpeechSynthesis`, on) and unused. Nothing in Rust or TypeScript inspects the answer's language.
- A per-turn instruction slot **exists**: `bots_ipc.rs:1140-1147` already prepends a system message to the request; Hermes layers a request's system message over its profile prompt without touching `SOUL.md`.
- A partial transcript **exists** end to end (`PartialHeard` → `Listening{heard}` → the status line and the tray). An audio level **does not**: the input tap at `voice_macos.rs:1128-1136` (1024 frames ≈ 21 ms at 48 kHz) hands every buffer to the recogniser and reads nothing from it. `Sent` and `AnswerChunk` are **never fed** by the shell, so the `Sending` state is unreachable and a turn jumps `Heard → Speaking`.
- A second window already has a precedent: `notes_window.rs` creates `quick-capture` hidden at boot and only shows/hides it. Tauri's native builder gives `always_on_top`, `decorations(false)`, `visible_on_all_workspaces`, `focusable(false)`, `focused(false)` and click-through via `set_ignore_cursor_events`. **`transparent` requires `macOSPrivateApi`**, which keeper does not enable and will not. A plain window **cannot** appear over another app's full-screen Space; that needs an `NSPanel` subclass (`tauri-nspanel`).
- The repo has a fold idiom — `column-fold.ts` / `session-spaces-fold.ts`, cookie-persisted, with a layout contract test that pins exactly one `flex-1` child in the transcript level (`bots-pane.test.tsx:594-621`). A new fold reuses it or it is a second convention.
- `objc2-natural-language 0.3.2` (`Zlib OR Apache-2.0 OR MIT`) binds `NLLanguageRecognizer`, on-device, no network. The Rust alternatives were rejected: `whatlang`'s corpus terms are unverified and `lingua`'s models derive from a CC BY-NC corpus.

## Decisions this epic takes

- **AD-182 — A spoken answer is spoken in a language this device has a voice for, and the model is told which language that is.** Two halves, both needed. *Before the turn:* a voice-originated turn carries a per-turn system instruction to answer in the listening language — the person is speaking it, and every device that can listen in a language can speak it. *After the answer:* the text's dominant language is detected on-device (`NLLanguageRecognizer`, constrained to the languages this device has voices for plus the listening language) and the utterance is given **that** language's voice explicitly — so a model that answers in Polish anyway is still spoken in Polish when a Polish voice exists. Only when no voice exists for the detected language does keeper speak nothing and say so (AD-27), naming where a voice is downloaded. Never a mismatched voice: that is the failure he heard.
- **AD-183 — Language detection is an input to the core, not a dependency of it.** `keeper-core` decides which voice to use from (detected language, listening language, available voice languages) as a pure function with a truth table; the port supplies the detection and the inventory. No platform `cfg`, no model weights, no network.
- **AD-184 — The voice block in the Bots pane is folded by default, and folded it is one line.** The line says what matters while a conversation is on screen: whether listening is armed, the phrase, the language in force ("Listening for "nixie" · en-US" / "Listening off"). The fold reuses the repo's cookie-persisted fold idiom. Settings → Bots keeps the unfolded block as today, so folding removes no path to any control. The transcript is measured before and after with the pane's own rig.
- **AD-185 — Hearing is shown by a surface that cannot steal focus and does not need the pane.** A prewarmed, undecorated, ~220×40 pt pill window: `always_on_top`, `visible_on_all_workspaces`, `focusable(false)`, `focused(false)`, click-through, **no transparency** (no private API). It exists only on the desktop, only when `voice_availability` is a real answer (AD-179), and it is shown only while a turn is not idle-unarmed — armed-and-waiting is a state worth a glance, not a permanent fixture. It draws three things: the state, a level meter, and the words as they arrive. Under reduced motion the meter is a static fill, never an animation.
- **AD-186 — The level is computed in Rust and batched; the turn's missing states become real.** RMS from the tap buffer, smoothed (fast attack, slow release), sent at a bounded rate (~20–30 Hz) and only while it changes; never one IPC message per 21 ms buffer. `Sent` is fed when the request goes out and `AnswerChunk` when tokens arrive, so "thinking" is a state the indicator can show instead of a gap the person reads as a stall.
- **AD-187 — Over another app's full-screen Space is deferred, with its cost named.** `tauri-nspanel` would give it, at the price of a git dependency and an `NSPanel` subclass in a signed build. The owner's stated case — Maps, music, a browser in front — is normal Spaces, which the native builder covers. Recorded as DW, not built.
- **AD-188 — No new destination, and no answer text leaves the device to be classified.** Detection is `NaturalLanguage`, on-device. `docs/egress.md` gains the sentence that says so.

## Stories

### 64.1 — The transcript gets its space back
AD-184. The desktop Bots pane's voice block folds to one truthful line by default; unfolding is one click and persists; Settings → Bots is untouched.
**Acceptance:** at 1440×900 the transcript's share of the pane, measured by the rig, is stated before and after and rises by at least the block's height; the folded line reflects switch, phrase and language live; the layout contract test (one `flex-1` child) still holds; the fold uses the existing fold store and cookie shape, no second idiom.
**binds:** FR-427, FR-428, AD-184

### 64.2 — Spoken in a voice it has
AD-182, AD-183, AD-188. The core rule, the port's inventory and detection, the explicit voice on every utterance, the per-turn instruction on voice-originated turns, and the honest refusal when no voice exists.
**Acceptance:** truth-table test over (detected, listening, voices) including: Polish answer + Polish voice → Polish voice; Polish answer + no Polish voice → text only with the download sentence; undetermined → listening language; a voice-originated turn's request carries the instruction and a typed turn's does not (asserted through the fake gateway); `objc2-natural-language` added with its feature list and licence noted; every new `unsafe` in the inventory; egress doc sentence present.
**binds:** FR-429, FR-430, FR-431, FR-432, AD-182, AD-183, AD-188

### 64.3 — A turn that has a level and a middle
AD-186. `Level` from the tap, batched and smoothed in Rust; `Sent`/`AnswerChunk` fed by the shell; `VoiceStateVm` carries what an indicator needs and nothing it does not.
**Acceptance:** a test that the level stream is bounded (N buffers in → ≤ ceil(N·21 ms / interval) messages out) and monotone under a constant input; a test that a turn passes through `Sending` between `Heard` and `Speaking`; the recogniser still receives every buffer unchanged; iOS gets the same states (the level meter may be absent there, the states may not).
**binds:** FR-433, FR-434, FR-435, AD-186

### 64.4 — A pill that shows it hears
AD-185, AD-187. The prewarmed window, its document, and its wiring to the voice stream; the states it renders; reduced-motion; absent where voice is unsupported.
**Acceptance:** with keeper behind another app on hesperia, a voice turn shows the pill over that app without activating keeper (**screenshot on hesperia is the proof**, this time the screen is unlocked); the pill never becomes key; it shows level, state and the heard words; reduced motion gives a static fill; `voice_availability: unsupported` → no window is ever created; the full-screen-Space limit is a DW row with the `tauri-nspanel` cost.
**binds:** FR-436, FR-437, FR-438, FR-439, NFR-54, AD-185, AD-187

### 64.5 — The record and the gates
Docs (`egress.md`, `decisions.md`, `constraints-and-limitations.md`), DW rows, the epic's before/after measurements, and every gate including the macOS one on hesperia.
**binds:** FR-440, AD-182…AD-188

## What stays out
- **Over a full-screen Space** (AD-187). **A waveform** as opposed to a level: a level answers "is sound arriving"; a waveform is decoration that costs a canvas at 30 Hz. **Streaming the answer's speech token by token**: synthesis waits for the whole answer, as today. **Choosing a specific voice** (Samantha, Zosia): the language is chosen, the system's default voice for it is used.

## The failure shape this epic must not repeat
Epic 63 shipped a Mac voice nobody had heard. This epic's stories each end in a **screenshot on hesperia** of the thing they claim, taken while the screen is unlocked — the pill over another app, the folded pane with the transcript measured, and a spoken Polish answer whose utterance voice is logged as `pl-PL`.

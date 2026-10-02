# Epic 98 — Your phone and your tablet

created: '2026-10-02'
status: planned 2026-10-02; build follows in story order on the agents stack
source: the owner's rounds 1 and 2 of 2026-10-01 (excerpts verbatim below), pinned by the coordinator as P15 and ruling R20; the coordinator accepted this epic's open-question readings in ruling R24 ((12), (13) and (14) by name) and every finding of the two reviews of 2026-10-02 as rulings R28 and R29 (`_bmad-output/planning-artifacts/agents-review-security-2026-10-02.md`, `agents-review-consistency-2026-10-02.md`; S-10, F14, F15 and F22 land here). The rulings are in `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`. Other inputs:
- `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md` — AD-412…AD-414, binding; *Matrix events* § *What wakes a phone*;
- `_bmad-output/planning-artifacts/research-agents-2026-10-02.md` — §6.4 (push to phones), §8.4 and §8.6 (echo cancellation, Android's on-device recogniser), §11.4 (iOS limits), §11.6 (Android), §13 #39–#41, #59, §14;
- the digest G5 (§3 Android, §5 Matrix push), as cited; DW-237 and DW-290 in `deferred-work.md`.

Line numbers are in the `agents-plan` worktree on 2026-10-02.
binds: FR-817…FR-820; NFR-121 (98.1, 98.4); NFR-119 (98.3, 98.4: the Gradle dependencies); AD-412…AD-414 (allocated by the architecture, not here); UX-DR144…UX-DR146; D-35 (and D-36 for Android voice). Deferred items in DW-407…DW-413.
- **The previous ceilings:** the program's (`_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md`, C1): epic 88, AD-359, FR-766, NFR-111, UX-DR126, DW-354, D-30. The architecture allocated AD-360…AD-416, FR-767…FR-822 and NFR-112…NFR-122; D-31…D-36 are in `docs/decisions.md`. This epic allocates no AD, FR, NFR or D number. Its deferred items are DW-407…DW-413 (DW-407 and DW-408 placed by the architecture); its UX decisions are UX-DR144…UX-DR146.
- **No earlier allocation.** On 2026-10-02 a grep of `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `tools`, `AGENTS.md`, `README.md` and `CLAUDE.md` for `DW-E98-` and `UX-DR-E98-` found only the architecture's DW-407 and DW-408 (`ARCHITECTURE-AGENTS.md` § *What stays out*) and D-35 naming both (`docs/decisions.md` § D-35). This epic's own deferred items start at DW-409, and its UX decisions at UX-DR144. The 2026-10-02 review wave opened no deferred item here.
see-also:
- D-1 (the paid program deferred; push never on project infrastructure; `docs/decisions.md:8-31`), D-15 (the phone's folder is a mirror and never merges, `:733`), D-16 (a push keeper does itself, `:831`), D-35 (`docs/decisions.md` § D-35), D-36 (`docs/decisions.md` § D-36, Android voice);
- AD-26/AD-27/AD-31 (the platform-neutral seams), AD-29 (one `Platform::data_dir()` root), AD-201 (the board stays on the Mac — taken here as DW-237 describes), AD-204 (the phone's disclosure);
- epic 93 (the approval card and the verified-device rule) — a notification's decision is that same decision, sent from the lock screen.

## The owner's ask

Verbatim, rounds 1 and 2 (2026-10-01):

> I dont want sidecar. I want my mac, iphone, sever on linux to use it.

> you can assume ios will have paid apple account in the future. also make sure it will be working on my android tablet

> I want to have connectors for dangerous actions before proceed (to review data in sessions to proceed after programaticly). - style like in Violoop

"Assume a paid account in the future" is an assumption, not an account: nothing in this epic that needs the Apple Developer Program can be proved before the owner enrols. Those steps are operator actions and operator-verified checklists below, never claims.

## The verdict, ask by ask

| # | The ask (verbatim) | Verdict | How it is met | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "you can assume ios will have paid apple account" | **planned, gated on the owner's enrolment** | APNs through a Sygnal the owner runs; a Notification Service Extension decrypts on the phone and shows only approval requests and the proxy's answers; approvals are decided from the notification. | AD-412; 98.1 |
| 2 | "I want my mac, iphone … to use it" | **planned, read first** | The sessions board, with agents' cards, run badges and approval cards, on the phone; the phone moves cards and decides approvals and never writes a session's log. | AD-413; 98.2 |
| 3 | "make sure it will be working on my android tablet" | **planned, as a client** | An Android platform, a build, a signed APK sideloaded on the tablet; rooms, the main agent, the board and approval cards. The tablet hosts no agent. | AD-414; 98.3 |
| 4 | the same, for push and voice | **planned, with a stated limit** | Push through UnifiedPush with the owner's ntfy; voice through Android's on-device recogniser in segmented sessions with echo cancellation, only while keeper is in front. Continuous duplex on Android is a documented limitation, not a promise. | AD-414, ruling R20; 98.4 |
| 5 | "connectors for dangerous actions before proceed" | **planned on every device** | The approval card of epic 93 reaches the lock screen (98.1), the phone's board (98.2) and the tablet (98.3–98.4); a decision counts only from a verified device. | AD-395, AD-412 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| Push on the iPhone | **absent, by decision** | D-1: APNs, the NSE "with its 24 MB memory ceiling and App-Group store-layout implications", App Groups — "only the paid program grants these"; push must "**never** ride project infrastructure" (`docs/decisions.md:13-24`). The phone's disclosure: "background notifications await a future decision" (`docs/ios.md:735`, mirrored from `IOS_DISCLOSURE_LINES`, pinned by `about-section.test.tsx:681-684`). |
| A second iOS target | **present** | `KeeperIsland`, a widget extension, beside `keeper_iOS` (`gen/apple/project.yml:27`, `:191`); it shares no files with the app ("no App Group", `docs/ios.md:918-922`). |
| The phone's store protection | **present, NSE-compatible** | `NSFileProtectionCompleteUntilFirstUserAuthentication` (`gen/apple/project.yml:116-119`, `keeper_iOS.entitlements:13-14`), pinned by `keeper/tests/entitlements_protection.rs:23-26`: readable after the first unlock, which is what an extension woken while locked needs. |
| A push gateway | **absent** | No Sygnal anywhere in makistack; `ntfy` appears only as an Uptime Kuma option (G5 §6). Sygnal is AGPL-3.0 (element-hq); ntfy implements `/_matrix/push/v1/notify` with UnifiedPush, Apache-2.0 or GPLv2 (§6.4). |
| The sessions board on the phone | **absent, by decision** | Every sessions command has a `#[cfg(not(desktop))]` twin answering "sessions are a desktop surface" (`keeper/src/sessions_ipc.rs:13-14`, `:23-28`, e.g. `:40-43`); the capability is `mac_folder_capability_of(&git_report(&state), cfg!(desktop))` (`ipc.rs:1481`); DW-237's recipe: "the read-only half first … `sessions` on a gate that is honest on the phone" (`deferred-work.md:3953-3972`). |
| The phone never merges | **present** | D-15 (`docs/decisions.md:733`); "Nothing is merged on a phone" (`docs/ios.md:737`); D-16, a push keeper does itself (`:831`). |
| Android | **absent** | No `gen/android`; `AppState::new` binds `IosPlatform` on iOS and `compile_error!`s on any other mobile target (`ipc.rs:466-477`); desktop-only dependencies are excluded for both mobile targets (`keeper/Cargo.toml:103`); no `android` script (`package.json:9-38`); no Android CI job (`ci.yml`: every job `macos-latest`). |
| Sign-in on Android | **recipe recorded** | DW-290: `AuthTabIntent` from `androidx.browser` ≥ 1.9.0 through Tauri's `startActivityForResult`, a Custom Tabs bridge activity as fallback, not tauri-plugin-web-auth (`deferred-work.md:6530-6535`). |
| Media URLs on Android | **planned, never built** | "introduce a `convertMediaSrc`-style remap only when Android actually starts" (`ARCHITECTURE-SPINE.md:195`, `:524`). |
| Voice on Android | **absent, by decision** | D-5: voice on the Apple platforms "and on neither of the others", for want of weights (research §11.6's note); Android's on-device recogniser "is not intended to be used for continuous recognition" (§8.6). |

## The one sentence

**An agent can wait for a person, but the person's phone only hears about it while keeper is open, the phone cannot see the board the agents work on, and the tablet has no keeper at all.** The fix has four parts, each as small as the platform allows:
- **Push on the iPhone** through a gateway the owner runs, carrying only an event id, decrypted on the phone, with *Approve once* and *Deny* on the notification.
- **The board on the phone**, read first, as DW-237 described.
- **keeper on Android** as a client: platform, build, sideload.
- **Push and voice on Android**, through UnifiedPush and the platform's own recogniser, with its limits written down.

## Decisions this epic implements

D-35 (`docs/decisions.md` § D-35): the phone gets push through the paid program and a gateway the owner runs, and keeper starts on Android. D-36 (§ D-36) for Android's voice. AD-412…AD-414 are the architecture's; this epic restates none. DW-237 is taken (its read half; the rest is DW-410) and DW-290 is taken; both get resolution lines when their stories land.

## What earlier decisions said, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **D-1** (`docs/decisions.md:8-31`) | the paid program deferred until push is a product goal; never project infrastructure | approvals on the lock screen | **Deferral ended by D-35; constraint kept** — the gateway is the owner's. |
| **AD-201 / DW-237** | the board stays on the Mac | the board on the phone | **Taken, read first** (AD-413). The lifecycle executor and the space editors stay on the Mac (DW-410). |
| **`IOS_DISCLOSURE_LINES`** (`docs/ios.md:735`, `:739`) | "background notifications await a future decision"; "What stays on your Mac: … the sessions board …" | both change | **Rewritten** by 98.1 and 98.2, in the doc and the constant together (the mirror test fails otherwise). |
| **D-5** | voice on the Apple platforms "and on neither of the others" | voice on the tablet | **Amended by D-36**: the platform's on-device recogniser on Android; continuous duplex not promised. |
| **`ipc.rs:466-477`** | a mobile target other than iOS is a compile error | an Android build | **Widened deliberately** to Android only; any other mobile target still fails to compile. |

## Requirements

Copied from the architecture's *Requirements allocated here*; not restated, not renumbered.

| id | statement | epic.story | AD |
| --- | --- | --- | --- |
| FR-817 | keeper on iPhone registers with a push gateway the owner runs, shows only approval requests and the main agent's answers, and decides from the notification — *Approve once* only when the whole payload fits it and never at T4, *Deny* always. The repo proves the classification, the actions and the decision event; delivery on the phone is gated by the device run recorded in `docs/agents.md` § Measured, after the owner's Apple enrolment. | 98.1 | AD-412 |
| FR-818 | The sessions board, with agents' cards, run badges and approvals, is on the phone; the phone moves cards and decides approvals and never writes a session's log. | 98.2 | AD-413 |
| FR-819 | keeper builds for Android in CI; a sideloaded build on the owner's tablet signs in and shows rooms, the main agent and the board — gated by the device run recorded in `docs/agents.md` § Measured. | 98.3 | AD-414 |
| FR-820 | On Android, pushes arrive through UnifiedPush, and voice uses the platform's on-device recogniser in segmented sessions with echo-cancelled capture; the turn models run only where `ort` runs, so without them Android ends a turn by the pause rule (DW-413); what the tablet does is recorded in `docs/agents.md` § Measured; continuous duplex is a documented limitation, not a promise. | 98.4 | AD-414 |
| NFR-121 | **No destination the person did not configure.** The agents add only the homeserver, provider base URLs, MCP servers, KVMs and the push gateway the person configured, each derived into the egress list (AD-53) and diffed at release; a networked `run` reaches only what its approval shows, and a `docs/egress.md` row says so. `keeper-agent` and `keeper-agentd` register no observability sink; the desktop's export never reads a span or event under the targets `keeper_agent` and `keeper_core::agents`, pinned by a test; `check:agentd-lean` forbids `opentelemetry*` and `posthog*` crates. | 89.6, 90.5, 90.6, 96.1, 96.2, 96.5, 98.1 | AD-369, AD-375, AD-405, AD-406, AD-409, AD-412 |

## Built on

The approval card, the verified-device rule and park/resume (epic 93); agent rooms in keeper's timeline (91.1) and the proxy dock (91.2); the board's run badge and fields (92.2); `keeper-agent`'s session reader, which builds on iOS (ruling R10); the turn models (97.1) for Android voice where its runtime allows.

## Open questions for the coordinator

Each has the reading this plan builds to, marked as such. None is resolved silently.

- **Q1. The Notification Service Extension needs Rust, and the crate topology has no place for it.** An extension is a separate process under a 24 MB memory ceiling (D-1); it cannot link the app's Tauri library.
  - **Plan's reading (accepted, R24(14)):** a slim static library crate, `src-tauri/crates/keeper-nse` (`crate-type = ["staticlib"]`, depending on `keeper-core` only), exposing one C-ABI call — decrypt and classify an event by room and event id through matrix-sdk-ui's notification client over the App Group store — called from a Swift `UNNotificationServiceExtension`. It joins the workspace and the iOS compile check; `check:agentd-lean`'s pattern gains a sibling `check:nse-lean` (no `tauri*`, no `keeper-agent`, no `keeper-sync`). **Rejected:** Element's Swift SDK bindings in the extension — a second Matrix SDK reading keeper's store.
- **Q2. Ordinary messages on the same account stop notifying on the phone?** The pusher is per account; the homeserver pushes every notifying event of every room. AD-412 shows only approval requests and the proxy's answers.
  - **Plan's reading:** as AD-412 says — the extension shows only those two; every other event is dropped (whether it may be dropped without the filtering entitlement is DW-408). The person's other rooms keep today's behaviour, notifying while keeper is open. Pushing ordinary messages is a separate product decision (DW-411). The owner should confirm.
- **Q3. Existing installs must move their data.** D-1's mitigation makes the App Group move "a path change, not a data migration" — for code. The bytes in the old container still have to move once.
  - **Plan's reading:** at first launch with the App Group, the app moves `data_dir()`'s contents into the group container in one journaled pass (a marker file, idempotent, never overwriting a non-empty destination), before anything opens a store; failure leaves the old directory in use and says so.
- **Q4. Which setting names the gateway.** NFR-121 needs it configured by the person; both of a person's iPhones use the same one.
  - **Plan's reading:** `push.gateway_url` (user-global, text, `https://` only, blank = no push); the pusher's `app_id` is `dev.tgorka.keeper.ios` for production APNs and `dev.tgorka.keeper.ios.dev` for development builds, because APNs' two environments are two Sygnal apps.
- **Q5. Whether tuwunel pushes at all, and suppresses edits.** `.m.rule.suppress_edits` on tuwunel is `[UNVERIFIED]` (`ARCHITECTURE-AGENTS.md`, *What wakes a phone*); so is tuwunel's pusher support itself `[UNVERIFIED]`.
  - **Plan's reading:** 98.1 measures both on the Synapse test homeserver (in-repo smoke) and on tuwunel (operator). If tuwunel pushes edits, the extension drops them (DW-408's trigger); if tuwunel does not push, iPhone push waits on the homeserver and the coordinator is told.
- **Q6. The gateway in the egress list.** keeper never connects to the gateway; the homeserver does, on keeper's request.
  - **Plan's reading:** a row labelled *Push gateway — your homeserver sends it event ids for this device* derived from `push.gateway_url` (and, on Android, the UnifiedPush endpoint's host), so the person sees it; `docs/egress.md` says keeper itself never contacts it.
- **Q7. DW-237's recorded location is stale.** It names `ipc.rs:1456` (`sessions: notes_available(&state)`); the gate is now `mac_folder_capability_of(&git_report(&state), cfg!(desktop))` at `ipc.rs:1481`.
  - **Plan's reading (accepted, R24(12)):** 98.2 builds against the current gate; DW-237's location line is corrected in `deferred-work.md` (F15), and its resolution line names the current line when 98.2 lands.
- **Q8. The epic map gates 98.3–98.4 "by an Android build", and no Android job exists.**
  - **Plan's reading (accepted, R24(13); one command, F15):** 98.3 adds a CI job on `ubuntu-latest` with the Android NDK that runs `cargo check -p keeper -p keeper-core -p keeper-sync -p keeper-agent --target aarch64-linux-android` (98.3 #1; `keeper-agentd` is not built for Android) and builds an unsigned debug APK. Signing and installing stay the operator's.
- **Q9. The turn models on Android.** Whether `ort` has a prebuilt runtime for `aarch64-linux-android` is `[UNVERIFIED]`.
  - **Plan's reading:** if it has, Android loads the same `_models/` turn roles (97.1); if not, Android's voice keeps the 1800 ms pause and Settings says so — FR-814's own fallback. 98.4 records which.
- **Q10. Android listens only while keeper is in front.** AD-414 uses no foreground service (Tauri #15671), and Android stops background microphone access without one.
  - **Plan's reading:** voice on the tablet works with keeper in front, never with the screen off; `ANDROID_DISCLOSURE_LINES` says so. This is narrower than the iPhone, which listens with the screen locked.
- **Q11. UnifiedPush's gateway discovery is not in the research.** A UnifiedPush endpoint is a URL on the distributor's server; the Matrix pusher needs the gateway URL.
  - **Plan's reading:** keeper asks the endpoint's host for `/_matrix/push/v1/notify` and accepts it only when it answers as a Matrix gateway `[UNVERIFIED: the UnifiedPush Matrix gateway convention]`; anything else refuses with a sentence naming the host. keeper never substitutes a gateway of its own (D-4).
- **Q12. "An action requires the device to be unlocked."** iOS has `UNNotificationActionOptions.authenticationRequired`; Android's equivalent is `setAuthenticationRequired(true)` on a notification action from API 31 `[UNVERIFIED]`.
  - **Plan's reading:** both actions (*Approve once*, *Deny*) require an unlocked device on both platforms; on an Android older than API 31 the notification carries no actions and opens keeper.

## Stories

Every story names its rung in the stack (*Stack rungs*, below).
- **98.1 and 98.2 change the shell crate** and are gated on CI's macOS job, CI's iOS compile check (`ci.yml:90`) and `check:rust:macos`; their device behaviour is proved on the owner's iPhone (kalypso).
- **98.3 and 98.4 change the shell crate for Android** and are gated on 98.3's Android CI job; their device behaviour is proved on the owner's tablet.
- **Nothing that needs the paid Apple program, an APNs key, a running Sygnal or ntfy, or the tablet is claimed proved by this repository.** Each is a device-run acceptance whose row in `docs/agents.md` § *Measured* is the gate (F14): the story closes when that row exists, and its device steps are the checklist beneath it.
- **Every new pure behaviour test is mutation-proved.**
- **Names are suggestions the lanes agree on.**

### 98.1 — Push and approvals on the iPhone

**Intent:** "you can assume ios will have paid apple account in the future"; "connectors for dangerous actions before proceed". **Rung:** **epic98-ios**. AD-412; D-35; AD-395 (the decision from the phone's verified device); Q1–Q6, Q12.

**Files:**
- `keeper-core/src/agents/push.rs` (new, pure): `classify_notification(event, context) -> Show { title, body, category, thread_id } | Drop` (an approval request → category `KEEPER_APPROVAL` when its payload fits, `KEEPER_APPROVAL_OPEN` otherwise, with 93.1's keeper-composed summary as the title; the proxy's answer → the latest text; everything else → `Drop`); `fits(record)` (#11); `latest_text(anchor, edits_seen, budget)`; `PusherSpec` (HTTP pusher, `format: "event_id_only"`, `default_payload` with `mutable-content: 1`, the app id per Q4).
- `keeper-core/src/agents/approval.rs` (93.3's): one function composes `dev.keeper.agent.approval.decision` for the card and for the notification action.
- `src-tauri/crates/keeper-nse/` (new, Q1): `staticlib`, the one C-ABI call, matrix-sdk-ui's notification client over the App Group store with the cross-process store lock.
- `gen/apple/project.yml`: the `KeeperNotify` extension target (Swift, `UNNotificationServiceExtension`, linking `libkeeper_nse.a`); the App Group `group.dev.tgorka.keeper` on `keeper_iOS` and `KeeperNotify`; `aps-environment` on the app; `KeeperNotify.entitlements` with the same data protection.
- the shell: `push_ios.rs` (new) — the APNs token, the pusher set on each account that holds agent rooms, the two notification categories and their actions (`KEEPER_APPROVAL`: *Approve once*, *Deny*; `KEEPER_APPROVAL_OPEN`: *Deny*), the action handler sending the decision; `ipc.rs`'s `IosPlatform::data_dir` → the App Group container, and the one-time move (Q3).
- `keeper-core/src/config/keys.rs`, `docs/settings-keys.md`: `push.gateway_url`.
- `keeper-core/src/egress.rs`: the gateway row (Q6).
- Settings → Notifications on the phone (UX-DR144's notification; the gateway field).
- `docs/ios.md` (§ *Push*, the Limitations list), `IOS_DISCLOSURE_LINES`, `docs/egress.md`, `docs/constraints-and-limitations.md`.

**Acceptance:**
1. **Only two kinds of event wake the person, in keeper's words.** Over decrypted fixture events: `dev.keeper.agent.approval.request` → `Show` with title `<agent> asks: <summary>`, where `summary` is the one keeper composed from the tool and its arguments (93.1's templates, and 96's per tool) and never text the model wrote (S-10); when the record fits (#11), category `KEEPER_APPROVAL` with the tier as a word and the whole payload as the body; otherwise category `KEEPER_APPROVAL_OPEN` with the body *<tier> — open keeper to read the whole request*. An answer anchor or its final edit in a proxy session (`dev.keeper.agent.turn` in the content) → `Show` with the latest text; `dev.keeper.agent.status`, `scope`, `surface.*`, `heard`, `doorbell`, `delegate`, a claim, an ordinary `m.room.message` in a non-agent room → `Drop`. Test: `push_classification_table` (pure, keeper-core).
2. **The latest edit, within the budget.** `latest_text` returns the newest edit seen before the budget ends, the anchor's text when none arrived, and never `…` (a body of only the placeholder is shown as *<agent> is answering*). Test: `push_waits_for_the_latest_edit` (pure, fake clock).
3. **The pusher carries nothing but an id.** `PusherSpec` for a gateway `https://push.example/_matrix/push/v1/notify` has `kind: "http"`, `data.format: "event_id_only"`, the Q4 app id for the build, and refuses an `http://` gateway or one with userinfo. Test: `pusher_spec` (pure).
4. **The notification's decision is the card's decision.** Approving from the notification and from the in-app card compose byte-identical event content for the same record and scope (`once`); `session` is never offered from a notification, and a T4 record never offers *Approve once* there (S-10). Test: `notification_decision_equals_the_cards` (pure).
5. **The extension's store is readable after first unlock, and shared.** `entitlements_protection.rs` (`keeper/tests/entitlements_protection.rs:23-26`) is extended to `KeeperNotify.entitlements`; a new test asserts both targets name the same App Group. Both run on Linux (they read files).
6. **The move is safe** (Q3). `move_into_group_container(old, new)` over temp directories: moves everything once; a second run is a no-op; a crash after half the entries (simulated) resumes and finishes; a non-empty destination without the marker refuses. Test: `container_move_is_journaled_and_idempotent` (keeper-core, pure filesystem).
7. **Edits do not push, measured on a real homeserver.** Against the Synapse test homeserver (ruling R13, on demand): a test user with an `event_id_only` pusher; an agent sends an anchor, three edits, a final edit and an approval request; `GET /_matrix/client/v3/notifications` lists the anchor and the approval request and none of the edits. Test: `synapse_suppresses_edit_pushes` (keeper-core integration test, run when `KEEPER_TEST_SYNAPSE` names the server, named in the PR as run on the dev host). Risk: a real server's push rules, not a model of them.
8. **The gateway is a disclosed destination.** `compute_egress` with `push.gateway_url` set lists its host once, labelled per Q6, and nothing when blank. Test: extended `compute_egress` tests.
9. **The extension compiles for the phone.** CI's iOS compile check — `cargo check --workspace --exclude keeper-agentd --target aarch64-apple-ios` once 90.5 has excluded agentd (`ci.yml:90` today has no exclusion) — compiles `keeper-nse` as a workspace member; `check:nse-lean` passes on the dev host (F15).
10. **The disclosure changes in both places.** `docs/ios.md`'s first Limitations line and `IOS_DISCLOSURE_LINES` say that approvals and the main agent's answers arrive as notifications through the gateway you set, and everything else while keeper is open; the mirror test (`about-section.test.tsx:681-684`) is green.
11. **Approve once only when the person can see all of it** (S-10). `fits(record)` is true only when the tier is below T4, the tool is neither of Paseo's prompt verbs (`create_agent`, `send_agent_prompt`), and the payload as the body shows it is one line of at most 120 characters with no element holding a newline. Test: `notification_offers_approve_only_when_the_payload_fits` (pure: a three-element argv at T3 fits; a 121-character payload, an argv element holding `\n`, a Paseo prompt and any T4 record do not).
12. **On the phone — the gate for FR-817's delivery** (a device run, F14; after the owner's enrolment and the operator actions). On kalypso, the steps below pass, and the extension's peak memory over twenty notifications and push latency (send → banner, p95 over twenty) are recorded with the device, iOS version, build and date in `docs/agents.md` § *Measured*. Until the owner enrols, 98.1 stays in review with this item open, never marked done.

**Operator actions (outside this repository, before the checklist):**
- [ ] Enrol the owner's Apple ID in the Apple Developer Program (paid).
- [ ] Create the App IDs `dev.tgorka.keeper` (Push Notifications, App Groups) and `dev.tgorka.keeper.notify` (App Groups); the App Group `group.dev.tgorka.keeper`; development and distribution provisioning profiles for both.
- [ ] Create an APNs authentication key (`.p8`); record its key id and the team id; keep the key in 1Password, never in this repository.
- [ ] Deploy Sygnal (element-hq, AGPL-3.0) on electra as its own container — a makistack change — with apps `dev.tgorka.keeper.ios` (APNs production, topic `dev.tgorka.keeper`) and `dev.tgorka.keeper.ios.dev` (APNs sandbox), the `.p8` key, key id and team id; expose `/_matrix/push/v1/notify` over HTTPS where tuwunel can reach it.

**Device run (kalypso, after the actions; the steps behind #12):**
- [ ] `bun run install:ios` installs the build with the extension; Settings → Notifications takes the gateway URL; tuwunel's `GET /_matrix/client/v3/pushers` for the owner's account shows the pusher with `format: event_id_only`.
- [ ] tuwunel pushes at all, and an answer streamed into Nixi's DM with the phone locked produces exactly one notification, not one per edit (Q5). If it buzzes per edit, record it — DW-408's trigger.
- [ ] A T3 approval whose payload fits appears on the lock screen with *Approve once* and *Deny*; *Approve once* asks for Face ID or the passcode; the owning host writes the decision file (the decider is kalypso's verified device) and the run resumes; the card on hesperia shows the verdict. A T3 approval with a long payload, and a T4 approval, show *Deny* only and open keeper when tapped.
- [ ] A status update and a surface event produce no notification.
- [ ] The extension stays under 24 MB over twenty notifications (Xcode's memory report); push latency, send → banner, p95 over twenty; both recorded in `docs/agents.md` § *Measured* (#12; unpublished anywhere else, research §14).
- [ ] After the update, the existing install's accounts, messages and phone folder are intact (Q3's move).

**Shell crate:** yes — `push_ios.rs`, `IosPlatform::data_dir`, the action handler, plus the new extension target. Gated on CI's macOS job and iOS compile check and `check:rust:macos`; everything that needs the paid program is the device run (#12).

**binds:** FR-817, NFR-121, AD-412, AD-395, D-35, UX-DR144

### 98.2 — The sessions board on the phone

**Intent:** "I want my mac, iphone, sever on linux to use it"; the board agents work on, where the person can see who works where. **Rung:** **epic98-ios**. AD-413 (DW-237 taken, read first); AD-386's fields on the phone; Q7.

**Files:**
- `keeper-agent`: the session reader and the board projection, unchanged, compiled for iOS (ruling R10); the session writer and the approvals writer behind `cfg(not(any(target_os = "ios", target_os = "android")))` so a phone build cannot reach them.
- the shell, `sessions_ipc.rs`: the read verbs — `sessions_roots`, `sessions_list`, `sessions_detail`, `sessions_tree`, `sessions_refs`, the search — run on iOS over the phone's folder stack (D-15) and lose their `unsupported` twins; `sessions_task_move` runs on iOS, writing `status:`/`order:` and pushing through keeper's own push (D-16); every lifecycle verb (create, archive, delete, unarchive, migrate, the space editors) keeps its twin.
- `ipc.rs:1481`: `sessions` true on the phone when a folder with a sessions zone is present.
- the front: the phone board (UX-DR145), reusing 92.2's run badge and 93.3's approval card.
- `docs/ios.md` and `IOS_DISCLOSURE_LINES` (the fifth line); DW-237's resolution line (coordinator).

**Acceptance:**
1. **The phone builds the read half.** CI's iOS compile check compiles the read verbs and `sessions_task_move` without their twins; the lifecycle verbs' twins remain (a `#[cfg(not(desktop))]` per lifecycle verb, counted in review against DW-237's list).
2. **The phone cannot write a log or an approval.** `keeper-agent`'s `SessionWriter` and approvals writer are compiled out for `target_os = "ios"` and `"android"`; the gate is the compiler — any phone code path that names either fails CI's iOS compile check (`ci.yml:90`), and from the `epic98-android` rung on, the Android job. Proof: the `cfg` on both types, and the iOS check green on this rung.
3. **The same board, from the same code.** The board projection of a fixture zone (four columns; cards with `run`, `assignee`, `host`, `requested_by`; a stray with an unreadable status) is computed by the shared reader and is identical whichever target calls it. Test: `board_projection_fixture` (keeper-agent, Linux).
4. **A move changes two keys and nothing else.** `sessions_task_move` on a fixture card writes only `status:` and `order:` (byte-preserving, the existing compile_move) and never `run:`, `assignee:` or the body; the commit is pushed by the phone's own engine. Test: `phone_move_writes_only_status_and_order` (keeper-core/keeper-agent, over the fixture card) plus the existing move tests unchanged.
5. **Approvals are decided on the phone through the same card.** Opening a card whose session has a pending approval shows 93.3's card from the session room; its decision is the Matrix event (AD-395). Test (front, vitest): the phone board's card opens the approval card through the mock shell.
6. **The honest gate.** On the phone, `sessions` is true only with a folder that has a sessions zone; without one the board is absent (AD-27). Test: the capability's unit test for both states (shell, by inspection on Linux; run in CI's macOS job).
7. **The disclosure.** `IOS_DISCLOSURE_LINES`'s fifth line and `docs/ios.md:739` say the board is on the phone for reading, moving cards and deciding approvals, and that creating and archiving sessions stays on the Mac; the mirror test is green.

**Operator-verified (kalypso):**
- [ ] After a sync, the phone's board shows tgdrive's sessions zone; an agent's card shows `run: running` and `running on electra`.
- [ ] A card moved on the phone appears moved on hesperia after its sync; nothing else in the card changed (`git show` on the commit).
- [ ] An approval decided from the phone's board is consumed on the owning host.

**Shell crate:** yes — `sessions_ipc.rs`, `ipc.rs`'s capability. Gated on CI's macOS job and iOS compile check, `check:rust:macos`, and the checklist.

**binds:** FR-818, AD-413, AD-386, UX-DR145

### 98.3 — keeper on Android

**Intent:** "make sure it will be working on my android tablet". **Rung:** **epic98-android**. AD-414 (98.3's half); DW-290 taken; Q8.

**Files:**
- the shell, `ipc.rs`: `AndroidPlatform` (`cfg(target_os = "android")`) — `data_dir` (the app's private files directory), secrets in an encrypted file whose key lives in the Android Keystore (through a small Kotlin plugin), `notify` through `tauri-plugin-notification`, `open_url` through Custom Tabs, `start_web_auth` through Auth Tab (DW-290's recipe), `sidecar_path` unsupported, `exclude_from_backup` by the manifest's backup rules, `set_badge_count` a no-op; the `compile_error!` (`ipc.rs:473-477`) narrowed to mobile targets that are neither iOS nor Android.
- every `cfg(target_os = "ios")` site that means "a mobile client" reviewed and widened or not, listed in the PR with its decision.
- `src-tauri/crates/keeper/gen/android/` (generated by `tauri android init`, committed like `gen/apple`), with the Kotlin plugins.
- `package.json`: `android:dev`, `android:build`, `install:android` (`scripts/install-android.sh`: `apksigner` with the keystore from the environment, `adb install -r`).
- `.github/workflows/ci.yml`: the Android job (Q8).
- `src/lib/media-src.ts` (new): the media-URL helper `ARCHITECTURE-SPINE.md:195` planned; every custom-scheme `src` goes through it.
- the front: the tablet layout (UX-DR146); `ANDROID_DISCLOSURE_LINES` mirrored from `docs/android.md`.
- `docs/android.md` (new): build, sign, sideload, sign-in, limitations, and § *Dependencies and licences* — every Gradle dependency of `gen/android` with its licence (#6); `docs/constraints-and-limitations.md`.

**Acceptance:**
1. **It compiles for Android, in CI.** The new job runs `cargo check -p keeper -p keeper-core -p keeper-sync -p keeper-agent --target aarch64-linux-android` with the NDK linker and builds an unsigned debug APK, uploaded as an artifact; `keeper-agentd` is excluded. Proof: the job green on the rung's PR.
2. **The secret store holds and refuses tampering.** The Rust half of the Android secret store (format: version, nonce, ciphertext; the key from a provider trait) round-trips, refuses a flipped byte and a wrong version, and never writes a plaintext secret. Test: `android_secret_store` (Linux, a fake key provider).
3. **Media URLs are right on every platform.** `mediaSrc(scheme, path)` returns `keeper-media://localhost/…` on Apple targets and `http://keeper-media.localhost/…` on Android `[UNVERIFIED: Tauri's Android custom-protocol form]`; no component builds a custom-scheme URL itself. Test (vitest): both forms, and a source scan that `keeper-media://` and `keeper-file://` appear only in the helper.
4. **The limits are written down.** `docs/android.md`'s *Limitations* list and `ANDROID_DISCLOSURE_LINES` match (the mirror test of `about-section.test.tsx:681-684` repeated for Android): no agent runs on the tablet; no screen recording or transcription; the board reads, moves and decides, as on the phone; voice per 98.4.
5. **The tablet layout.** At 800 × 1280 and 1280 × 800 the shell shows the rooms list, the proxy dock and the board without the desktop-only panes (UX-DR146). Test (vitest): the layout's breakpoints with Android's capabilities through the mock shell.
6. **Gradle dependencies pass the licence firewall** (NFR-119, F15). `cargo deny` never sees `gen/android`'s Gradle dependencies, so a source scan reads every dependency coordinate declared in `gen/android`'s Gradle build files and requires each to be listed in `docs/android.md` § *Dependencies and licences* with a licence `src-tauri/deny.toml` allows; a GPL or AGPL licence, an unknown one, or an unlisted coordinate fails. Test: `android_gradle_dependencies_are_licensed` (Linux).
7. **On the tablet — the gate for FR-819** (a device run, F14). The steps below pass on the owner's tablet and are recorded with the device, Android version, build and date in `docs/agents.md` § *Measured*; the story closes when that row exists.

**Operator actions:**
- [ ] On the build machine: Android SDK, NDK and a JDK installed; `bun run tauri android init` run once and `gen/android` committed in this rung.
- [ ] The owner creates a signing keystore (never in this repository) and exports `KEEPER_ANDROID_KEYSTORE`, `KEEPER_ANDROID_KEYSTORE_PASSWORD`, `KEEPER_ANDROID_KEY_ALIAS` for `install:android`.
- [ ] On the tablet: Developer options → USB debugging (or *Install unknown apps* for the file manager); Tailscale signed in, so the tablet reaches tuwunel and the drives' remotes.

**Device run (the owner's tablet; the steps behind #7):**
- [ ] `bun run install:android` installs the signed APK; keeper opens without a blank webview (Tauri #15671 is not triggered: no foreground service).
- [ ] Sign-in to the owner's Matrix account on tuwunel; the organisation account through Auth Tab, and its cancel path (DW-290).
- [ ] Rooms render; a message to Nixi in the dock is answered; the tablet's Matrix device is verified from hesperia.
- [ ] The phone folder for tgdrive syncs; the board shows the sessions zone; an approval card decides, and the owning host accepts the tablet's verified device.

**Shell crate:** yes, for Android. Gated on 98.3's Android CI job (compile and debug APK) and the tablet checklist; the macOS job and the iOS compile check must stay green on the same rung.

**binds:** FR-819, AD-414, UX-DR146

### 98.4 — Android push and voice

**Intent:** "make sure it will be working on my android tablet"; P15's "UnifiedPush/ntfy, AEC"; ruling R20. **Rung:** **epic98-android**. AD-414 (98.4's half); D-36; Q9–Q12.

**Files:**
- `keeper-core/src/agents/push.rs`: `PusherSpec` for UnifiedPush (`app_id: "dev.tgorka.keeper.android"`, `pushkey` = the endpoint, `url` = the discovered gateway); `discover_gateway(endpoint)` (Q11) as a pure answer check plus a request built by the shell. `classify_notification` and `fits` are 98.1's, unchanged.
- `gen/android`: the UnifiedPush connector (`org.unifiedpush.android:connector`, Apache-2.0, listed in `docs/android.md` § *Dependencies and licences*, 98.3 #6) receiver; the notification in 98.1's two categories — *Approve once* and *Deny*, or *Deny* only (unlock required, Q12); the voice port's Kotlin half — `AudioRecord` on `VOICE_COMMUNICATION` with `AcousticEchoCanceler` when available, `SpeechRecognizer.createOnDeviceSpeechRecognizer` fed through `EXTRA_AUDIO_SOURCE` in segmented sessions, `TextToSpeech` for answers.
- the shell: `push_android.rs` and `voice_android.rs` (new) over those halves.
- `keeper-core/src/voice/platform.rs`: `VoicePlatform::ANDROID` (nouns, limits sentence; `full_duplex` true only where the echo canceller is available — the port's fact through `half_duplex`).
- `keeper-core/src/voice/segments.rs` (new, pure): joining a segmented recogniser's results into one utterance.
- `keeper-core/tests/voice_on_device.rs`: the Android port in its scan.
- `docs/android.md`, `ANDROID_DISCLOSURE_LINES`, `docs/egress.md`.

**Acceptance:**
1. **The pusher.** `PusherSpec` for an endpoint `https://ntfy.example/upAbc?up=1` and a discovered gateway has `kind: "http"`, `pushkey` the endpoint, `url` the gateway, `format: "event_id_only"`. Test: `unifiedpush_pusher_spec` (pure).
2. **Discovery, never substitution.** Against a local HTTP fixture: an answer identifying a Matrix gateway is accepted; a 404, a non-JSON body or another gateway kind refuses with a sentence naming the host; no default gateway is ever used. Test: `unifiedpush_gateway_discovery` (keeper-core integration, local server).
3. **One classification for both phones.** The Android receiver calls 98.1's `classify_notification`; the table test covers both (no second table).
4. **Recognition is on the device, enforced.** `voice_on_device.rs` scans the Android port (Rust and Kotlin): every recogniser is created with `createOnDeviceSpeechRecognizer`, `createSpeechRecognizer(` never appears, and no network API is used; the scan's file floor includes the Android files. Test: the extended scan (runs on Linux).
5. **Segments join without loss or repetition.** Over fixture result sequences (partials, a segment's final, the next segment's partials), `segments` yields the utterance as one text, with no word twice and none dropped at the joins. Test: `segmented_results_join` (pure).
6. **Duplex is a fact, not a promise.** `VoicePlatform::ANDROID`'s limits sentence says listening works with keeper in front and stops with the screen off (Q10); with no echo canceller the port reports half duplex and the turn releases the microphone before speaking (`may_record`, `turn.rs:326-329`). Test: `voice_platform_android` (pure).
7. **The turn models, or the fallback.** If `ort` offers an Android runtime (Q9), the turn models load as on the iPhone; if not, `TurnModelsState` on Android is the sentence *Turn models are not available on Android; keeper waits 1.8 s after you stop*. Test: the state's sentence (pure); which branch was built is stated in the PR.
8. **The endpoint is disclosed.** The UnifiedPush endpoint's host joins `compute_egress` as Q6's push row. Test: extended `compute_egress` tests.
9. **It compiles.** 98.3's Android job stays green with the new modules, and 98.3 #6's licence scan with the connector listed.
10. **On the tablet — the gate for FR-820** (a device run, F14). The steps below pass on the owner's tablet, and the device, Android version, build and date are recorded in `docs/agents.md` § *Measured* with: whether `AcousticEchoCanceler.isAvailable()` is true; whether on-device recognition keeps working while text-to-speech plays (research §14); which branch of #7 was built (the turn models, or the pause rule); push latency, send → notification, p95 over twenty. The story closes when that row exists.

**Operator actions:**
- [ ] Deploy ntfy (Apache-2.0 option) on electra as the owner's service — a makistack change — reachable by tuwunel and by the tablet over the tailnet.
- [ ] Install the ntfy Android app on the tablet as the UnifiedPush distributor, pointed at the owner's ntfy server.

**Device run (the owner's tablet; the steps behind #10):**
- [ ] keeper registers; tuwunel's `GET /_matrix/client/v3/pushers` shows the Android pusher.
- [ ] With keeper in the background (not force-stopped), an approval request arrives as a notification in 98.1's category for its payload — *Approve once* and *Deny*, or *Deny* only; approving needs the device unlocked; the owning host accepts the tablet's verified device.
- [ ] Voice with keeper in front: the microphone permission is asked at first use (no crash, Tauri #15506); a spoken question reaches Nixi and the answer is spoken; talking over the answer pauses it where the echo canceller is available. Record whether `AcousticEchoCanceler.isAvailable()` is true on the tablet, and whether on-device recognition keeps working while text-to-speech plays (research §14).
- [ ] Push latency, send → notification, p95 over twenty, recorded in `docs/agents.md` § *Measured* (#10).

**Shell crate:** yes, for Android. Gated on 98.3's Android job and the tablet checklist.

**binds:** FR-820, NFR-121, AD-414, D-36

## UX decisions

- **UX-DR144 — an approval on the lock screen.** Title *`<agent>` asks: `<summary>`* — keeper's summary, never the agent's words (S-10). When the whole payload fits on one line (98.1 #11): body the tier as a word and the whole payload (the argv, the recipient, the file); actions *Approve once* and *Deny* (destructive), both requiring an unlocked device. Otherwise — a longer payload, a Paseo prompt, any T4 action: body *<tier> — open keeper to read the whole request*; the one action *Deny*. Tapping either opens the session's approval card. A proxy's answer: title the proxy's name, body the answer's latest text. Nothing else notifies.
- **UX-DR145 — the board on the phone.** The four columns as a horizontally paged list, one column per page with its count; a card shows its title, the run badge, the assignee's identity mark and *running on `<host>`*; a pending approval adds a badge. *Move to…* from a card's menu (no drag); no create, archive or edit on the phone.
- **UX-DR146 — the Android tablet's layout.** The phone's surfaces at tablet width: rooms list and the open room side by side at ≥ 840 dp, the proxy dock as a sheet, the board as on the phone; desktop-only panes absent (AD-27).

## What stays out

- **Android distribution beyond sideload** (Play Store, F-Droid) — DW-407 (architecture).
- **The notification filtering entitlement** — DW-408 (architecture).
- **An agent on the phone or the tablet** — refused (P5, AD-414): clients only.
- **Push through project infrastructure** — refused (D-1, D-35).
- **Continuous duplex voice on Android** — a documented limitation, not a promise (ruling R20).

Deferred, with the ledger entries opened here; each is in full in `_bmad-output/implementation-artifacts/deferred-work.md`:
- DW-409 — Live Activities for a running agent session are not used.
- DW-410 — the phone's board does not create, archive or edit sessions (DW-237's second half).
- DW-411 — ordinary messages do not push.
- DW-412 — Android listens only while keeper is in front.
- DW-413 — no Android voice runs the turn models where `ort` has no Android runtime.

## The failure shape this epic must not repeat

**A server that reads the notification.** A review that finds any of the following is a blocker: a pusher whose format is not `event_id_only`; a notification body composed anywhere but on the device after decryption; a gateway keeper chose instead of the person; a notification that offers *Approve once* for a payload it does not show whole, for a Paseo prompt, or for a T4 action; a title or body that carries text the model wrote.

**A phone that writes what only a host may write.** A review that finds any of the following is a blocker: a phone build that links the session writer or the approvals writer; a phone move that writes `run:`; a decision file written by a phone.

**A promise the platform does not keep.** A review that finds any of the following is a blocker: a disclosure line that says Android listens in the background; a device-run acceptance marked done without its row in `docs/agents.md` § *Measured*; an acceptance criterion marked done that needs the paid program before the owner has enrolled; a Gradle dependency outside `docs/android.md`'s licence list.

## Sprint-status entry

The coordinator applied this epic's entry under `development_status:` in `_bmad-output/implementation-artifacts/sprint-status.yaml`, above the epic-97 block; the 2026-10-02 review wave's changes are recorded in that entry.

## Stack rungs

On top of epic 97's last rung. Each compiles alone.
1. **`epic98-ios`** — 98.1 and 98.2: `keeper-core`'s `push.rs` (the two approval categories and `fits`), the push setting and the egress row; the `keeper-nse` crate and `check:nse-lean`; the `KeeperNotify` target, the App Group and the entitlements tests; `push_ios.rs` and the data-dir move; the phone's sessions read verbs, `sessions_task_move` on iOS and the honest gate; the phone board; the disclosure lines. Named in the PR as awaiting CI's macOS job / `check:rust:macos`, with 98.1's device run (#12) open until the owner has enrolled.
2. **`epic98-android`** — 98.3 and 98.4: `AndroidPlatform` and the narrowed guard; `gen/android` and its Kotlin plugins; the Android CI job; the Gradle licence scan; `install:android`; the media helper; the tablet layout; UnifiedPush and the Android voice port; `voice_on_device`'s Android scan; `docs/android.md`. Gated on its own Android job; the macOS job and iOS check stay green. The device runs (98.3 #7, 98.4 #10) close their stories by their rows in `docs/agents.md` § *Measured*.

# Epic 102 — Agents that hear you, answer fast, and say what went wrong

created: '2026-10-19'
status: planned 2026-10-19 by the coordinator; built as nine keeper lanes off `main` d6bd92e5 (release 0.10.5) plus one makistack change
source: the owner's message of 2026-10-19 (verbatim below, two screenshots of the installed app on hesperia); eight read-only diagnoses of `main` d6bd92e5 against hesperia's `keeper.log`, `sync.db`, its drives and makistack (`/tmp/agents-salvage/triage-102-*.md`); a HogQL read of PostHog project 605256.
binds: rulings R461–R470 in `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`; deferred items DW-1510…DW-1609 (blocks per story).

## The owner's ask

Verbatim (Polish, 2026-10-19):

> - gdy podczas auth nie bedzie przelaczenia na przegladarke dodaj url ktore mozna wkleic recznie do przegladarki
> - w wersji mobilnej (iphone) w opcjach nie widze manualnego check and install new version ja w desktopie (jezeli jest mozliwe)
> - agenci nie moga korzystac z internetu mino ze maja dostep (chyba ze to tylko kwestia nixi - sprawdz w makistack) - sprawdz tez inne tool calls czy dzialaja
> - lista agentow w agentach laduje sie bardzo dlugo, tak samo jak otwieram danego agenta - mozna to jakos przyspieszyc zeby bylo natychmiastowe np protokol, cache?)
> - po dolaczeniu image agent nie odpowiada - sprawdz, napraw, przetestuj - rowniez z innymi plikami itp nie tylko obrazami
> - daj mozliwosc wlaczania/wylaczania internetu i innych opcji tool call podczas pracy z agentem (teraz widze tylko read only status)
> - jak wylaczam/wlaczam keepera to chce zeby byl otwarty default agent i jego konwersacja zebym mogl bez opbslugi szybko korzystac z agenta zaraz po otwarciu app (tez w mobile)
> - w liscie chatow w agentach nie widze ktory jest default
> - miej pewnosc ze wszystkie ustawienia sa w settingach zdalnych
> - miej pewnosc ze wszystkie bledy i warn sa logowane (w posthog)
> - sprawdz ostatnie bledy - jak wlaczam keepera to widze blad w nixi
> - settings - podziel na kategorie zamiast scrollowanie jedej dlugie listy (ale wciaz wszystko zsynchronizowane ma byc)

The owner's choices (2026-10-19, asked by the coordinator): PostHog receives every WARN/ERROR **with its message, redacted** (R461); diagnostics consent **travels with the account** (R462); the iPhone gets a **check-only** update row (R464); launch opens the default agent's **main conversation** (R383 kept, R463).

## What the diagnoses found (root causes, with evidence)

| Report | Verdict | Root cause |
| --- | --- | --- |
| No internet for Nixi | broken (deployment + keeper) | makistack `config/agentd.yml` pinned keeper-agentd `6969c07d` (v0.10.0~2, reports 0.9.0) with `web_search.enabled: false`, so electra's host has no `web_search` tool. v0.10.4/v0.10.5 carry the signed asset (DW-1500 closed) but nobody re-pinned → makistack PR #886 (102.10). Keeper bugs that keep it broken after the upgrade: (A) the Mac's save of Nixi's web-search level (tgdrive commit f75ac5e79) never reached origin — tgdrive's push is held (see sync row); (B) R460's guard failed open: `AccountManager::serving_hosts` (account.rs ~4379-4441) reads host manifests only from a local store the sliding sync does not keep fresh (`dev.keeper.agent.host` not in required state, 1 event per room), and `model.rs` (~412, 466-472) passes when no live host is found — no test exercises `serving_hosts`; (C) `workflow.rs` (~216-225) tells the model "keeper has no web search of its own … so it searches none" whenever `skill_view`/`skills_list` is offered (DW-381 open), which is exactly what Nixi said; (D) the globe and Settings row read only the Mac's local agent.toml (`desktop.rs` ~482-484, DW-1477), so they show while the host cannot search. |
| Other tool calls | broken on electra (old host) + silent drops | Old host: drops non-text messages, refuses homes with `[model].effort` (delegation to Tola), 403 on the DM request. Keeper drops tools silently: a host with no engine logs nothing (`web.rs` ~81-84); a failed sandbox says console Off and the header draws nothing (`agent.rs` ~2677-2678, `room.rs` ~960-964); a turn with no drive grant gets no tools, logged at DEBUG (`agent.rs` ~10199-10201, `turn.rs` ~419-421); `keeper-agentd status` reports web_search, surface_*, kvm_* as "not offered" where they are (`report.rs` ~108-125). |
| Toggles in the chat | unreachable | Writers exist in Settings › Agents (web search off/ask/without asking `agents-section.tsx` ~744-829, console, model) through the one writer `desktop::prepare_home`; the room header has only a read-only globe (`agent-room-header.tsx` ~222-232) and the Make-default menu. |
| Photo never answered, "Read ✓" | broken (deployment) + misleading tick | The old host's `arrival_of` drops every non-`m.text` message with no log and no room line (6969c07d `runtime.rs` 2822-2824); the send is in hesperia's log (2026-10-19T06:38:34Z, `kind=image size=482064`). "Read ✓" is not the agent: no host sends receipts; matrix-sdk-ui moves the agent's implicit receipt from its 60 s claim-renewal state event onto the newest visible event (`timeline.rs` ~446, `message-bubble.tsx` ~445, 691-694), so a dropped message looks read within a minute. |
| Other files | broken for audio/video everywhere | `FILE_MSGTYPES = [m.file, m.image]` (`attachments.rs` ~72); the composer sends audio as `m.audio`, video as `m.video`; `arrival_of` returns None for both with no log (DW-1454, scope deferral with no teeth — and it breaks R381). PDF/CSV/text/images work on ≥ v0.10.4. Also silent: undecryptable person message in a session room (`runtime.rs` ~2947, ~2083), a `ServeError` before the anchor (ERROR log only, `agent.rs` ~9093 → ~5833), and the host shows `idle` during a download of up to 300 s. Residual: `MAX_IMAGE_BYTES` 8 MiB vs Anthropic's 5 MB per image [INFERENCE]. |
| Agents list slow | broken | Every row's state is read serially inside the room-list diff and nothing paints until all are read (`account.rs` ~9268-9272 → `read_attention` in the loop at ~8633, merged at ~8635); each read scans every session room (`asks_answered_in` ~7423, ~7450-7469); a room without a kept seat runs up to 4 sequential raw `/messages` (no timeout) then `read_room` back-paginates ≤300 events under 5 s (`room.rs` ~1621-1666, `waiting.rs` ~240-268). Launch readiness waits in the same loop (≤ LAUNCH_READY_MS 15 s; 3 of 13 launches on 10-18 hit the bound). Every claim renewal re-runs the reads, O(N²): 37,849 "status is unread" re-reads on hesperia on 2026-10-18 (~10^5 requests/day to electra). DW-1496 (serial reading) open. |
| Opening an agent slow | broken | `forward_timeline` opens the room header with page-back before the first batch even for seated rooms (`timeline.rs` ~1149 → `room.rs` ~1570); first batch 0.13–1.08 s; then the SDK unloads the room to its last chunk on every limited sync (claims every 60 s, `timeline_limit` 1, no `subscribe_to_rooms`); the webview refills one 200-event page per IPC round trip (`conversation-body.tsx` ~110-118, ~886-955). |
| "protocol, cache?" | absent (snapshot) / deliberate (lease) | No persisted keeper-side snapshot of the agents list VM (only seats, `seats.rs`). The minute lease is R455 (teeth: two hosts must never hold one session; released hosts advertise nothing). The quarter-hour lease is an operator cutover once every host is ≥ v0.10.4 (DW-1449 first). |
| Default agent not opened at launch | unreachable | Built (R383/R397/R416), but with `agents.default` unset `launch::choose` returns `view: Agents, room: None, reason: None` and nothing tells the person a default must be chosen (`launch.rs` ~244-247). |
| No default marker in the list | absent | `chat-row.tsx`/`chat-list-pane.tsx` draw attention mark, timestamp, mention badge, unread dot only; R383 chose Settings + the header menu as the only surfaces. |
| "Error in Nixi at launch" | old host + incomplete reads | Header lines "Console: unknown. This agent's host does not say how its commands run." (`console.rs` ~254, old host) and "no scope yet"; rows *Unknown* from incomplete reads (perf row); launch reason toasts. |
| All settings remote | deliberate, verified | Every registered key syncs (`settings_sync.rs` `synced_file()`); what stays local has recorded teeth (OS login item, hardware, keychain posture, session latches). The one change: diagnostics consent travels (owner's choice, R462). |
| Settings categories | absent | `SettingsBody` (`settings-dialog.tsx` ~155-305) is one flat scroll of 23 sections shared by the desktop pane and the phone dialog. |
| Errors in PostHog | deliberate (R381/R387/S-19) + two defects | Zero events since 2026-09-12 (HogQL): diagnostics consent is opt-in per installation and was never enabled on hesperia; iOS builds embed no `KEEPER_POSTHOG_*` (`install-ios.sh`) so a phone can never send; `keeper_frontend_error` is emitted only from `command-palette.tsx` ~103; no tracing layer forwards WARN/ERROR. The owner's choice replaces the content-free rule (R461). |
| Auth URL | absent | `org_account/oidc.rs` ~563-580 and `auth.rs` ~196 open the URL and never show it; pending state "Complete sign-in in your browser…" (`login-screen.tsx` ~381); a silent 5-minute wait ends "timed out". Device flow (`browse-repos-sheet.tsx` ~707-731) is the in-repo pattern. End-session (F3) must never use `open_url` (`oidc.rs` ~1660). |
| iPhone update | deliberate (distribution) | `in_app_updater: cfg!(desktop)`; dev-signed IPA via `install-ios.sh`, no TestFlight/itms-services (`docs/ios.md` ~1103-1106). A check is possible; an install is not. |
| tgdrive not pushing (found by the triage) | broken | Upload 704972 (`bd5857b3`, Forgejo already has it) stopped on its first attempt 2026-10-17T16:17:52Z with "the account credential is unavailable: Reconnect the repository."; a sign-in failure is a configuration error → permanent → stopped (`sync.rs` ~84-87, `error.rs` ~308, `engine.rs` ~8394); the push gate counts stopped uploads (`engine.rs` ~10714-10724) and only the Retry button re-arms one (neither a new sign-in nor a good pull does, `engine.rs` ~8596). Last good push 10-17T03:14Z; ahead 8. The person sees the generic "needs your attention" with an empty error; Retry is row 27 of 68, folded to 10. |
| neuradrive download retried forever | broken | Marta's Mac (0.9.0) merged 8e237b7 keeping a conflict copy of a `*.json` (LFS-filtered), the merge's `git add` turned it into a pointer, and uploads are queued only for normal commits (`engine.rs` ~11497) → pointer `96bae0e0` pushed without bytes (absent on Forgejo, `lfs-object` exit 4). The download is re-queued every scan (1099 stopped rows, 728 log lines since 10-10); the log line carries no profile (`engine.rs` ~12089). The bytes survive in history (blob `9129af5`, sha256 matches). |

## Stories (lanes; stack order bottom first)

### 102.1 — Every error and warning reaches PostHog, with consent that travels (lane `telemetry`, DW-1510…DW-1519)
1. A tracing layer in the desktop/iPhone shell forwards every WARN and ERROR event (all targets, including `keeper::panic` lines) to the telemetry queue as a `$exception`-shaped record: level, target, file:line, a stable fingerprint (target + callsite), and the message **redacted** (R461): Matrix ids, emails, URLs' path/query, home-relative paths, hex runs ≥ 16, base64 runs ≥ 24, `syt_`/`phc_`/bearer tokens and anything after `token=`/`key=`/`password=` replaced; capped at 1,000 chars. Events under `keeper_agent` and `keeper_core::agents` are sent **without** the message (level, target, file:line, fingerprint only — R461). keeper-agentd and keeper-nse stay out (check:agentd-lean / check:nse-lean unchanged).
2. Rate limits: per-fingerprint at most 10 per minute and a session cap; the queue never blocks a log call; dropped counts reported once per minute as one record.
3. The frontend funnel (`reportFrontendError`, window error, unhandledrejection, crash boundary, `onUncaughtError`) emits the frontend-error record with its redacted message; `console.error`/`console.warn` are hooked and forwarded the same way (dedup per load).
4. Diagnostics consent becomes one user-global settings key that travels in `<login>/settings.toml` (R462): on (or off) on one device is on (or off) on every device of the account; the per-installation store migrates (an existing local "on" becomes the account's value once). Product-analytics consent follows the same rule. Settings says it applies to every device.
5. `scripts/install-ios.sh` (and Android's build if it compiles the lib) embeds `KEEPER_POSTHOG_HOST` / `KEEPER_POSTHOG_PROJECT_TOKEN` the way `install-macos.sh` does; a build without them fails loudly like macOS.
6. `docs/observability.md` rewritten to the new rule; R381/R387 amended by R461 in the decisions file; the S-19 test that pins agent spans stay home is amended to "agent targets carry no message".
7. Proof: unit tests of the redactor (each class) and the agent-target strip; a live smoke that sends one WARN through the real exporter to project 605256 and reads it back by HogQL (the coordinator holds the personal key; the lane uses `op read` inside a script, never printed).

### 102.2 — A push never waits forever on something already fixed, and a pointer never leaves without its bytes (lane `sync`, DW-1520…DW-1529)
1. A stopped upload whose cause was the account credential is re-armed when the credential comes back (successful sign-in/reconnect or a successful authenticated pull/push of that profile); the push gate stops counting it then. Applies to every stopped transfer of the credential class.
2. A held push says why in the Sync view: the specific cause of the oldest stopped transfer and one "Retry all stopped" action (re-arms every stopped transfer of that profile); the generic "needs your attention" with an empty error is never shown for a held push.
3. A merge's conflict copies (and every path a merge commit adds) queue their LFS uploads like a normal commit; the push gate sees them; a pointer is never pushed whose object is not on the server or queued.
4. A download whose object the server does not have (404 / "does not have LFS object") stops after one attempt per scan cycle family — it is not re-queued on every scan while a stopped row exists — and the log line names the profile. When the object's bytes are present in local git history as a non-pointer blob with a matching sha256, keeper re-uploads them (heal) and says so; otherwise the Sync view names the file and the device that made it.
5. Proof: engine tests for each item (credential re-arm, merge conflict copy upload queued, missing-object heal from history, no re-queue storm); live run against delectra's Forgejo where the harness exists.

### 102.3 — Agent tools you can see, trust and switch from the chat (lane `tools`, DW-1530…DW-1539)
1. R460's guard fails closed (R466): `serving_hosts` reads each host's manifest (`dev.keeper.agent.host`) from the server when the local store lacks it (room state fetch); a save whose serving host cannot be read is refused with a sentence naming the agent and saying why; a test drives `serving_hosts` with an empty store.
2. The capability row tells the model the truth: when `web_search` is offered on this turn the prompt never says "keeper has no web search"; DW-381 closed; `every_bmad_capability_is_answered_for_every_offer` asserts the offered case.
3. The globe and the Settings web-search row show what the **serving host** can do (manifest: engine configured or not) next to what agent.toml asks for; "asks for web search, host has none" is drawn as a warning sentence, not a plain globe (DW-1477/DW-1471).
4. The agent room header's menu gets **Web search** (Off / Ask / Without asking) and **Console** (the modes Settings offers) as radio items, writing through the existing `agentsWebSearchSet` / `agentsConsoleSet` (same writer `prepare_home`, same older-host check); available on desktop and phone wherever Settings offers them; state re-read after the save.
5. Silent tool drops are said: a host with no engine, a failed sandbox (console Off by failure, not by choice) and a turn with no drive grant each produce a WARN and a header line; `keeper-agentd status` reports web_search/surface_*/kvm_* correctly (`report.rs` predicate).
6. Proof: unit tests per item; live Synapse test that a manifest only on the server is read by `serving_hosts`; browser proof of the header menu.

### 102.4 — Every file you send is taken, and the tick tells the truth (lane `attachments`, DW-1540…DW-1549)
1. `m.audio` and `m.video` are intake: stored like other files (Kind::Other) and the turn gets the note "keeper does not let agents listen to or watch {name} yet; it was saved at {path}" — the agent answers; a WARN is logged. DW-1454 narrowed to transcription only.
2. In agent session rooms the person's message shows the turn state, not a read receipt (R465): *taken* when a `dev.keeper.agent.turn` anchor names its event id (old hosts send this field too), *not taken* when the host wrote into the room after it and no anchor names it (with the sentence "Nixi's host did not take this message — it may be older than this kind of message"), nothing while neither is known. The agent user is not counted as a reader in these rooms.
3. A person's message the host cannot decrypt in a session room gets a notice in the room (as delegated rooms already do, R283); a `ServeError` before the anchor posts the TURN_FAILED notice; the host posts `running` before `take_in` (a download no longer shows `idle`).
4. Images above the model's per-image limit are downscaled (JPEG re-encode) to fit, or refused with a sentence naming the limit; the limit is a per-provider const (Anthropic 5 MB).
5. Proof: live Synapse run (desktop host and agentd on delectra) sending image, PDF, CSV, text, audio, video, HEIC — each answered; mutation-proved unit tests; browser proof of taken / not taken.

### 102.5 — The Agents list is there the moment you open it (lane `list`, DW-1550…DW-1559)
1. The last-known agent rows (room, kind, agent, title, attention state, last activity) are persisted per account in keeper.db and emitted as the first Reset before account activation completes; live reads replace them as they land (stale rows marked only if their read later fails).
2. One coalesced per-account attention worker reads each changed room once per pass and folds the cross-room asks once per pass (no O(N²)); row reads run off the room-list diff path; the list paints without waiting on any read.
3. No network on any listing path: seat finding and status page-back move to a background finder with exponential backoff (R416's "look again later" kept, bounded per room per hour); claim renewals do not trigger a re-read of every row.
4. Launch readiness no longer waits on row reads (only on what `launch::choose` needs).
5. Proof: a test with N=30 agent rooms asserting first emission before any read and reads ≤ N per pass; a perf test with a budget (first rows < 100 ms from a warm DB); live count of `/messages` requests per renewal on delectra.

### 102.6 — Opening an agent shows its conversation at once (lane `open`, DW-1560…DW-1569)
1. `forward_timeline` sends the first batch from the event cache before any header seat/page-back work; the header fills in afterwards.
2. The open room is subscribed in sliding sync (`subscribe_to_rooms`) with a timeline limit that keeps the visible window, so a claim renewal no longer unloads it.
3. Rust fills back-pagination until something visible arrives (bounded), and the pagination status is carried with the first batch; the webview stops refilling one page per round trip.
4. Proof: test that the first batch precedes header reads; live Synapse test with > 200 claim events after the last message (open shows the last message within 500 ms from a warm cache).

### 102.7 — keeper opens straight into your default agent, and the list shows which one it is (lane `launch`, DW-1570…DW-1579)
1. With no default chosen and agent rooms present, launch opens Agents and shows, once, a non-blocking prompt "Choose the agent keeper opens when it starts" listing the person's agents; choosing writes `agents.default` and opens its main conversation at once (R463). Dismissed → not shown again until the next version.
2. With a default, launch opens its main conversation (R383) with the composer focused, on desktop and phone (phone: the conversation screen, not the list); verified that the phone's navRestore does not override it.
3. The default agent's rows in the Agents chat list carry a "Default" marker (before the truncating name, not in the attention slot) with tooltip "Opens when keeper starts"; every agent row's context menu has "Make default" / "Default ✓".
4. Proof: tests per item; browser proof at 1280 and 420 px.

### 102.8 — Settings in categories (lane `settings`, DW-1580…DW-1589)
1. Settings is split into categories (Account, Appearance, Notifications, Chats & composer, Agents & bots, Notes, Sync & drives, Recording & transcription, Voice, Privacy & diagnostics, Updates & about, Advanced — final grouping from the 23 sections, none dropped, none duplicated): desktop = a left category list + the category's sections; phone = a category list that drills into the category with a back button. Search over setting titles if a search exists today; deep links to a section (existing callers that open Settings at a section) still land.
2. Nothing about persistence changes: the same `SettingsBody` sections, the same keys and sync.
3. Claim 9 verified: `docs/settings-keys.md` lists every key's travel; the Settings footer/category says which items are this-device-only (launch at login, hardware, keychain posture) with the recorded reasons.
4. Proof: test that every section appears in exactly one category; browser proof at 1280 and 420 px.

### 102.9 — Sign-in you can finish by hand, and an iPhone that knows it is out of date (lane `devices`, DW-1590…DW-1599)
1. While an OIDC sign-in (Matrix F1 and org-account F2) waits for the browser, the sign-in screen shows the authorization URL as selectable text with a Copy button ("If your browser did not open, copy this link into it"); the same on failure/timeout. Desktop and phone. Never for end-session (F3).
2. On the phone tier, About › Software updates shows "Check for updates": it reads the latest release tag from GitHub, compares with the installed version and says "keeper X.Y.Z is available — install it from your Mac with `bun run install:ios`" or "keeper is up to date" (R464). No download, no install.
3. Proof: tests for the VM field and the version comparison; browser proof of both surfaces.

### 102.10 — electra runs the current agent host with web search (makistack, lane `electra`, DW-1600…DW-1609)
1. makistack `config/agentd.yml` pins keeper-agentd v0.10.5 by url (minisign-verified, sha256 `9f5f8010…aa66`) and `web_search.enabled: true` — makistack PR #886 (coordinator, 2026-10-19).
2. Deploy on electra: `task deploy:ansible -- --tags=keeper_agentd --limit=electra` — **operator** (no access from the agents' container).
3. After epic 102's release, re-pin to it (102.4's host-side items need the new host).
4. Then, in order: the owner presses Retry on tgdrive's stopped upload (or 102.2 ships), so Nixi's agent.toml change reaches electra only after electra reads its keys.

## Owed on devices
- electra: deploy #886; a web-search turn; a photo turn; delegation Nixi → Tola.
- hesperia: launch into the default; list/open timing; PostHog events visible after consent; Retry on tgdrive.
- kalypso: launch into the default; check for updates; diagnostics reaching PostHog; sign-in URL.

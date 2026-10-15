# Epic 101 — Agents you can find, tell things to, give tools, and watch work

created: '2026-10-15'
status: planned 2026-10-15 by the coordinator; built as seven lanes off `main` d2e3178d (epic 100 merged)
source: the owner's message of 2026-10-15 (verbatim below, with five screenshots of the installed app on hesperia); four read-only diagnoses of `main` d2e3178d against hesperia's `keeper.log`, its drive and the account config repo (kept beside the coordinator's notes, `/tmp/agents-salvage/ctx/survey-101-*.json`); prior art read in t3code `339179f5` and Paseo `d3efddb0`.
binds: rulings R416–R423 in `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`; deferred items DW-1440…DW-1509 (blocks per story).

## The owner's ask

Verbatim (Polish and English, 2026-10-15):

> - default conversation it exists and still not opening with default - if no conversation would exists i would like still to open the empty conversation with the default agent
> - prs are merged
> - the assistant in the notes doesnt work
> - by default chce czekac na wyslanie wiadomosci 2s gdy mog cofnac (a nie jak teraz pare) - ale chce miec oddzielnie dla agentow (0s)
> - chce zeby agent mial mozliwosc w opcjach korzystania z web search (searchxng) - separate option for all agents - and setup url in the config for makistack (dont commit in the keeper repo - config of the keeper) for nixi
> - chce zeby agent mial mozliwosc w opcjach do korzystania z konsoli - ustaw dla kazdego agenta z osobno - oraz zakres folderow (sandbox) per agent albo yolo mode - patrz na t3code jakie guardraile sa mozliwe
> - dodaj znaczniki przy agentach gdy pracuja (mysla) lib gdy potrzebuja atencji, lub gdy skonczyli - patrz na t3code lib passeo
> - w przypadku przekazywania zadan - chyba nie do konca chodza - w przykladzie przekazanie do toli nie poszlo mimo ze tola agent istnieje
> - jezeli sa potrzebne decyzje to renderuj je zebym mogl wybrac szybko (myszka lub klawiatura) - patrz jak to jest zrobione w t3code
> - zrob mozliwosc uzywania slash w agentach rowniez w srodku tekstu nie tylko na poczatku (w zaleznosci jakie opcje sa mozliwe w srodku tekstu)
> - jak zaczynam nowy chat z nazwa np /new test - to chce widziec ta nazwe w liscie agentow np nixi - test
> - w oknie agenta oprucz dolaczenia pliku chce miec tez mozliwosc zrobienia zdjecia (lub domyslnego wybrania zdjecia - systemowy wybor kamery)
> - pliki w agentach wydaja sie nie chodzic (jak na przykladzie)
> - gdy pisze w agencie (i wszedzie) to system auto zwieksza pierwszy znak na duza litere - oraz czasami daje dziwne autocorekty (sprawdzone na mac) - mozna to wylaczyc?

On electra's agent host the owner chose (2026-10-15): a new makistack role deploys keeper-agentd and its configuration (R423).

## What the diagnoses found (root causes, with evidence)

| Report | Root cause |
| --- | --- |
| Launch "has no conversation" / notes Assistant "no assistant yet" | Same cause. `proxy_rooms` (proxy.rs) admits a room only from a readable status, read from the event cache or at most 4×50 events back (room.rs `HeaderReader::open`/`page_back`). The host renews its claim as room state every 60 s (claim.rs `RENEW_EVERY`; DW-1385: 283 of the newest 300 events in Nixi's DM are claims), so after ~13 h of quiet the status is out of reach → `Refusal::Unread` → dropped without a reason. `page_back` returns "searched" when it merely ran out of pages, so the process never looks again. The admitted seat (kind, agent) lives only in memory (`AgentKinds`). Evidence: hesperia keeper.log 08:21:43Z/08:22:14Z WARN, then admission passing at 08:46 after new turns posted statuses. |
| No DM at all | Only `keeper-agentd agents init` makes a `main` DM; the person cannot (a room they create fails `NotMadeByItsAgent`). |
| New conversation not listed / dock times out | The agent's conversation room arrives as an invite; keeper joins only when the person sends into it (`send::join_if_invited`), and `agent_rooms` reads joined rooms only. |
| "nixi 2026-10-15", "N2", `/new test` listed as "Nixi" | Those three rooms are Tola's triage delegations to Nixi (`session_title`). A conversation's title goes only into the encrypted status by design; the list uses the SDK `display_name()`; "N2" is `roomInitials` of "nixi 2026-10-15". |
| Delegation to Tola refused | electra runs a keeper-agentd built before epic 100. The Mac saved `[model].effort` into both homes at 08:16Z (commit 24d2ae68c); the old host refuses an unknown key, so both homes dropped out of `Known` (`known_agents` skips a refused home silently) while their running copies kept serving; `delegate::resolve` then says "No agent named … is known on this host." Cross-host delegation is designed (AD-385) and same-host delegation worked at 00:01Z. |
| Files "came without attachments" | `arrival_of` (keeper-agent runtime.rs) turns only `m.text` into a turn; `m.file`/`m.image` with captions are dropped; `open_turn` always writes empty attachments; nothing downloads media for a turn. Download/decrypt (`download_encrypted`), image parts (`ContentPart::Image`, `deliverable::image_offer`) and the log's `Attachment{drive,path}` already exist. No PDF text crate in the lock (permissive candidates: `pdf-extract` MIT, `lopdf` MIT). |
| Undo window "a few seconds" | `undo_send.window` default 10 s, user-global, one value for every room. |
| Autocapitalise / odd autocorrect on the Mac | No text surface sets `autocapitalize`/`autocorrect`/`spellcheck` except a few credential fields; WKWebView substitutions run at their defaults. |
| No status in the Agents list | Rows carry only the room kind; `AgentRunVm` (idle/running/blocked/waiting/done) is drawn only in the open room's header. |
| Decisions | Approval cards (epic 93) have mouse buttons only; `ask_human` (epic 94) is a plain text message with numbered choices answered by typing. |
| Web search, console | No web tool for agents. `run` (epic 96) exists, sandboxed per host (`[sandbox]`), tiered T2–T4 with approvals; no per-agent mode or folder scope. |

Prior art (t3code, Paseo): runtime modes Supervised / Auto-accept edits / Auto / Full access, default per project, per thread in the composer; sidebar states Pending approval > Awaiting input > Working > Completed-unread (t3code) and needs_input > failed > running > attention > done (Paseo), "done" clearing on visit; questions in a panel above the composer with `1`–`9` keys; `/` mid-text after whitespace offers only inline items.

## Stories (lanes; stack order bottom first)

### 101.1 — Your agents' rooms are found, named and opened (lane `rooms`, DW-1440…DW-1449)
1. A status page-back that ran out of pages is not "searched empty"; later looks search again.
2. A proxy room's admitted seat (kind, agent, title) is persisted per account in keeper.db when first admitted and used when the status is out of reach; a newer status always wins.
3. The person's own agents' invites to session rooms (the inviter is the room's sole creator and an agent this account knows through its proxy/control room) are joined automatically; a `main` invite is joined as direct (m.direct).
4. When the default agent (or the notes Assistant's agent) has no `main` room here, keeper asks the agent's host for one through the control room (a new closed `AgentOutbound` request the host answers with the existing `seed::main_dm`), shows "Opening a conversation with Nixi…" and opens the room when it arrives (bounded, R416), else says why. Launch with a default and no DM: Agents opens, the request goes out, the room opens when it lands.
5. The notes Assistant finds the agent by the same rules and offers the same request.
6. A conversation or session room with a title is listed as "<Agent> — <title>" (e.g. "Nixi — test"), in the Agents window, the dock and the room header; the mark is the agent's picture/initial, not initials of the title.
7. Claim renewal no longer floods the timeline: renewed at most every 15 min with the lease length adjusted so takeover semantics hold (R416); DW-1385 closed.
8. Live Synapse tests (`#[ignore]`, run): admission after >200 claim events; auto-join of a conversation invite; DM request answered by a host.

#### As built (lane `rooms`, branch `agents-101-rooms`, 2026-10-15)
1. `room::HeaderReader::page_back` answers "searched to the end" only when the server's history ended; a look stopped at its 4×50 budget is not noted (`AgentKinds::note_searched_empty`), so later looks search again — `a_dm_buried_under_claims_is_admitted_by_its_kept_seat` (live: `!searched_empty` after 220 claims).
2. `agents::seats` (`SeatStore`, `keeper.db` `agent_room_seats`, keyed by the account's user and room, `CREATE TABLE IF NOT EXISTS`); `AgentKinds::kept` loads it at start, `AgentKinds::insert` keeps the newer-or-equal `Seat {kind, agent, title, status_ts}` and writes on a kind/agent/title change; `account::proxy_row` falls back to the seat when no status is in reach (`HeaderReader::open_row` skips the page-back when seated) — `a_kept_seat_survives_a_restart_and_only_a_newer_status_replaces_it`, `an_existing_database_gains_the_table`, `a_rooms_seat_survives_a_restart_and_a_newer_status_replaces_it`, live `a_dm_buried_under_claims_is_admitted_by_its_kept_seat`.
3. `proxy::joins_invite` (session room, sole creator = inviter, inviter known); `account::known_agents` (own proxies + agent-power members of their control rooms), `join_agent_invite(s)`; joined on arrival (`device::register`'s `StrippedRoomMemberEvent` handler) and at every listing (`agent_rooms_and_asks`); the SDK join marks a direct invite in `m.direct` — `only_a_known_agents_own_session_room_is_joined_on_its_own`, live `an_own_agents_invites_are_joined_and_the_dm_is_direct`.
4. `events::MAIN_DM_REQUEST`/`MAIN_DM_ANSWER` (+ contents), `AgentOutbound::MainDmRequest` (a control-room state event keyed by the asker, `AgentOutbound::state_key`), the control-room row at 0; `agents::main_dm::{decide, MAIN_DM_WAIT, MAIN_DM_FRESH}` and its sentences; host: `keeper_agent::dm_request::answer` (from `runtime::register_handlers`, both hosts) under `claim::main_dm_key`, `seed::main_dm_synced`; keeper: `Accounts::agent_main_dm_open` (shell `agent_main_dm_open`), `launch::MainDmAskVm`, `AgentsLaunchVm.ask/askable`, `launch::choose`; front `lib/main-dm.ts` `openMainDm`, `useLaunchIntoAgents` — `only_the_proxys_own_human_gets_a_dm_and_anyone_else_a_sentence`, `a_default_proxy_without_a_dm_opens_agents_and_is_asked_for_one`, `a_control_room_made_before_presence_or_the_doorbell_is_brought_up_to_date`, `use-launch-into-agents.test.ts` (asks/opens; leaves a person who moved), live `a_running_host_answers_the_persons_ask_for_their_dm` (keeper-agentd).
5. `notes-agent-dock.tsx`: with no room listed it reads `askable` (`agentsDefaultGet`) and offers *Open a conversation with <name>* through `openMainDm`, docking the room when it lands — `notes-agent-dock.test.tsx` (offers and opens; says why when refused).
6. `room::titled_room_name`, `agent_name`, `agent_mark`; `room_item_to_vm` names a titled non-`main` session "<agent> — <title>" and sets `RoomVm/InboxRoomVm.agent_mark`; `proxy_rooms` names conversations the same (`ProxyRoomRow.agent_name`); `RoomAvatar` draws `agentMark` — `a_titled_session_is_listed_by_its_agent_and_title`, `the_dm_is_the_docks_default`, `RoomAvatar.test.tsx` (agent mark).
7. (as amended by fix rounds 1 and 2, below: the quarter-hour lease only where the host's own setting turns it on and every live host reads it) `claim::RENEW_EVERY` 15 min, `STOP_WITHOUT_RENEWAL` 30 min, `TTL` 45 min; manifest `host::MANIFEST_RENEW_EVERY`/`MANIFEST_TTL` 60 s/180 s — `the_lease_renews_rarely_and_still_stops_before_a_takeover`, `a_holder_that_cannot_renew_stops_writing_before_another_may_take_over`, the existing claim tests; DW-1385 closed.
8. Live tests as named above (keeper-core `agents_matrix_live`, keeper-agentd `live_main_dm`); results in the lane report.

Fix round 1 (review-101rooms-1, R448–R452):
- #2 — seats per `(own user, room)` in memory, store and every caller; forgotten at sign-out; every newer status moves the kept time; the row chooses by the same rule (`AgentKinds::seat_after`); the room list folds the newest cached status and `device::register` every status (`room::fold_status`), rows redrawn on a change — `two_accounts_in_one_room_keep_their_own_seats`, `the_newest_status_seen_wins_through_a_restart_and_at_the_row`, `a_newer_status_redraws_the_rows_only_when_it_changes_them`, live `an_unopened_rooms_row_follows_a_newer_status`.
- #3 — `account::settle_answered_dm` marks direct after a join too and answers whether the room is settled; the answer is kept until it is — live `a_dm_left_and_reinvited_is_joined_and_direct_again`, `an_answer_before_its_invite_is_kept_until_the_room_settles`.
- #4/#5 — `account::open_main_dm` over `main_dm::open_within`: one deadline from command entry, the answered room searched back with no page budget (`HeaderReader::open_searching`) — `a_discovery_that_never_answers_ends_at_the_deadline`, `a_join_that_never_answers_after_the_answer_ends_at_the_deadline`, live `an_existing_buried_dm_opens_on_a_device_that_kept_no_seat`; host: asks queued per copy and served at the tick with the host's calibrated clock (`dm_request::{Asks, serve_all, take_claim}`), freshness by `main_dm::request_age`, `seed::main_dm_synced` fenced by the claim — `a_crashed_devices_request_claim_is_taken_once_it_lapses`, `a_requests_age_comes_from_the_server_or_not_at_all`, `a_dm_whose_claim_lapsed_makes_no_room_and_no_folder`, `a_stranger_asking_again_and_again_is_refused_once_a_while`; dock offers looked up with every listing — three `notes-agent-dock.test.tsx` cases.
- #7 — `claim::LeaseKind` (minute / quarter-hour), `LeaseKind::for_fleet` over the live manifests' `lease`, `Lease::want`, `claims::acquire_session`; control-room claims stay on the minute lease — `beside_an_old_host_the_claim_never_looks_free_while_its_holder_writes`, `a_holder_drops_to_the_minute_lease_when_an_old_host_appears`, `the_quarter_hour_lease_waits_for_every_live_host`, `a_claim_lapses_by_the_lease_it_says`.

Fix round 2 (review-101rooms-2, R455):
- #7 — the quarter-hour lease is an operator's cutover, off by default: `claim::ClaimsLease` from `agentd.toml` `[claims] lease` (`AgentdConfig.claims_lease`) or the Mac's `agents.claims_lease` (`registry::{get,set}_agents_claims_lease`, Settings › Agents `AgentsClaimsLeaseRow`, `agents_claims_lease_get/set`, read at every desktop scan); `LeaseKind::for_fleet(setting, manifests, claims, now)` also refuses it beside a live minute-lease claim from a host that does not advertise it (`HostRuntime::claims_seen`); every held lease is told each tick and `Lease::want(Minute)` halts writes on a quarter-hour lease until the minute renewal is confirmed — `claims::tests::by_default_an_old_host_returning_first_never_finds_a_written_claim_free`, `beside_an_old_host_the_claim_never_looks_free_while_its_holder_writes`, `turned_on_a_holder_stops_at_first_sight_of_an_old_hosts_claim`, `a_holder_leaving_the_quarter_hour_lease_writes_nothing_until_its_minute_renewal`, `agentd::tests::the_quarter_hour_lease_is_on_only_where_the_file_says_so`, `agents-claims-lease-row.test.tsx`; DW-1444 closed.
- #4/#5 — `AccountManager::agent_main_dm_open` looks the client up inside the deadline (`main_dm::not_reached_in_time`) — `a_dm_asked_while_the_accounts_are_held_ends_at_the_deadline`; `main_dm::attempt` asks before the whole listing (`DmOpening::found` over the agent's own direct rooms), the listing running beside the answer wait — `a_slow_listing_of_other_rooms_never_holds_the_request_back`, live `an_existing_buried_dm_opens_on_a_device_that_kept_no_seat` with prior rooms kept.
- R120 at the folder — `seed::make_folder` goes through `verbs::create_claimed_session` with the claim's fence — `seed::tests::a_claim_lost_while_the_zone_lock_was_waited_for_makes_no_folder`.
- Deferred: DW-1446 (sign-out vs in-flight seat writers), DW-1447 (equal-timestamp seat persistence order), DW-1448 (several control rooms of one agent: the ask goes to the first).

Stack (lane `101stack`, 2026-10-16; review-101rooms-3):
- R101R3-01 — a downgrade from the quarter-hour lease stays wanted until its minute renewal is confirmed: `claims::Lease::want` keeps `Minute` while `halted` — `claims::tests::a_downgrade_the_fleet_takes_back_before_it_is_sent_still_lands`.
- R101R3-03 — the folder-fence regression waits for the maker's own "the claim holds" over a channel instead of sleeping 300 ms — `seed::tests::a_claim_lost_while_the_zone_lock_was_waited_for_makes_no_folder`.
- Deferred: DW-1449 (`live_claims`' two-tick clean-shutdown bound fails on delectra at the base too). Release gate R101R3-02: the Settings row's cutover sentence interpolates the app's version, so the first release carrying it must be newer than 0.10.3.

### 101.2 — Files and photos reach the agent (lane `files`, DW-1450…DW-1459)
1. A person's `m.file`/`m.image` (with or without caption) in an agent room is a turn; the caption is the text.
2. The host downloads (≤ 25 MiB, else a sentence) and stores it in the session's `attachments/` (raw bytes; LFS rules of the drive apply); `UserBody.attachments` names it so the model sees "Attached files".
3. PDF → extracted text beside it (`pdf-extract` or `lopdf`, cargo-deny clean); CSV/TXT/MD readable by `drive_read`; images go to the model as image parts when its vision is not `Some(false)`, otherwise the turn says the model cannot see images.
4. The composer in agent rooms (and chats) gains "Take photo": desktop — a camera sheet (getUserMedia, device picker, capture one frame, send as an image through the existing bytes path); iOS/Android — the system camera via `<input capture>` with `NSCameraUsageDescription`; also "Choose photo" (system photo picker where the OS has one).
5. Errors per R381; live test: a PDF and a CSV sent into an agent DM are answered from their content.

### 101.3 — A composer that waits as long as you want and does not rewrite you (lane `composer`, DW-1460…DW-1469)
1. `undo_send.window` defaults to 2 s; a new user-global key `undo_send.agent_window` (default 0 s) applies to agent rooms; both in Settings; docs/settings-keys.md regenerated.
2. Every free-text input (composers, notes editor, search, settings text fields) sets `autocapitalize="off"`, `autocorrect="off"` and whatever WebKit on macOS needs to stop automatic substitutions (proved on hesperia: type "test" at the start of a message → stays lowercase; a known autocorrect word stays as typed).
3. `/` after whitespace in a message opens the menu with inline items only, decided in Rust: in agent rooms — mention another agent (`@tola-grey`), reference a drive file (inserts its drive path), and the chat inline set (`/shrug`); whole-line commands stay start-only.
4. Decisions: an `ask_human` question and an approval waiting for this person render as a panel above the composer (t3code-style): options numbered, `1`–`9` choose when focus is not in a non-empty field, Enter confirms the highlighted one, Escape collapses; the card in the timeline keeps its buttons.

### 101.4 — Web search for the agents you choose (lane `web`, DW-1470…DW-1479)
1. New tool `web_search` (SearXNG JSON API), offered only when the agent's `[tools].allow` names it and its host has a search URL; results (title, URL, snippet, ≤ 10) enter as data under the not-instructions sentence; tier and egress row per R419.
2. Host config: agentd.toml `[web_search] url = "…"`; on the Mac, Settings › Agents › Web search (URL in keeper.db). Never in the keeper repo.
3. Settings › Agents per agent: a "Web search" switch writing only `[tools].allow` (the 100.2 writer generalised to named keys, compare-and-replace, R412 lock).
4. Live test against a SearXNG (delectra or a fixture server).

### 101.5 — A console per agent, with the guardrails you pick (lane `console`, DW-1480…DW-1489)
1. `agent.toml` `[run] mode = "off" | "supervised" | "sandboxed" | "full"` and `paths = [...]` (folders the console may read/write; drive-relative or absolute host paths).
2. off — no console. supervised — every command asks (approval card). sandboxed — commands run in the host sandbox limited to `paths` without asking; outside them or networked → asks. full ("yolo") — no sandbox, no approval, a standing warning in Settings and in the room header; never for gate agents or homes on untrusted ingress (refused at load).
3. Settings › Agents per agent: console mode and folders (same writer).
4. Tests per mode on Linux (landlock) and by inspection + macOS check for sbpl.

### 101.6 — See who is working, who needs you, who is done; delegation that says why (lane `status`, DW-1490…DW-1499)
1. Rust computes per agent room a state: needs-you (an approval or ask this person can answer) > failed (blocked) > working (running) > done-unread (a run finished after the person's read marker) > none.
2. The Agents window rows, the notes dock and the agent's DM row draw it (t3code-style icon + word; working animated with reduced-motion fallback); done-unread clears when the person reads the room.
3. Delegation: `known_agents` keeps refused homes with their sentence; `resolve` says "tgdrive/tola-grey's agent.toml does not read on this host: …"; the Mac's save of a key newer than a serving host's manifest version warns before writing.

### 101.7 — electra's agent host deployed by makistack (lane `electra`, makistack repo, DW-1500…DW-1509)
1. makistack role `keeper_agentd`: installs keeper-agentd from the keeper release asset (sha256 pinned, like `keeper_sync`), restarts its service, and manages one marked block in `agentd.toml` holding `[web_search] url = "http://127.0.0.1:5191"` (SearXNG's host-loopback publish, JSON format on) — everything else in the file stays the operator's.
2. keeper's release workflow publishes the keeper-agentd asset where the role can fetch it (verify against the v0.10.3 release, which lists no agentd asset).
3. Runbook: how to deploy, verify (`keeper-agentd agents list`), roll back.

## Owed on devices
hesperia: launch into Nixi's DM; notes Assistant; "Nixi — test"; photo from the Mac camera; autocapitalisation off; undo 2 s / 0 s. electra: deploy the role, delegation Nixi → Tola, web search. kalypso: camera, autocapitalisation.

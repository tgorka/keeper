---
name: keeper
status: final
sources:
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/prd.md
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/addendum.md
  - _bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/brief.md
  - _bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/addendum.md
  - _bmad-output/planning-artifacts/research-market-2026-07-03.md
  - _bmad-output/planning-artifacts/research-ios-2026-07-09.md
  - _bmad-output/planning-artifacts/research-recording-2026-07-16.md
  - docs/project-context.md
created: 2026-07-03
updated: 2026-07-16
---

# keeper — Experience Spine

> macOS desktop (MVP) + iPhone (Phase 2, PRD §13) + Screen Recording (macOS, Phase 3, PRD §14). Paired with `DESIGN.md` (visual identity; token references `{...}` resolve there). Spines win on conflict with any mock or import. FR/NFR numbers reference the PRD. UX benchmark: the Beeper desktop app — left sidebar with unified inbox + Spaces/filter chips, chat list, main conversation pane, right detail panel, ⌘K, keyboard-first. The phone tier is a projection of this spine, not a second product: everything not restated in `Responsive & Platform` behaves exactly as specified for desktop. Screen recording is a desktop-only utility surface gated behind the `recording` capability flag (FR-66) — specified in `Screen Recording (macOS — Phase 3)` and absent everywhere the flag is off.

## Foundation

macOS desktop app (MVP) and, since Phase 2, the same app on iPhone: Tauri 2 shell, React 19 + TypeScript UI on **shadcn/ui + Tailwind v4** (components already installed in `src/components/ui/`: sidebar, command, dialog, sheet, tabs, context-menu, dropdown-menu, popover, tooltip, scroll-area, avatar, badge, button, card, input, input-group, textarea, label, switch, separator, skeleton, sonner). `DESIGN.md` is the visual identity reference and names the brand-layer overrides; this spine specifies behavior only.

Architecture shapes the experience contract: the UI is a **pure renderer of Rust-owned view models** streamed over IPC. Every state this document names (send states, bridge health, sync status, draft persistence) is authoritative in the Rust core; the UI never invents state, only renders it. Consequences the UX depends on: cached-first rendering (inbox and timelines paint from the Local Archive before network), optimistic local echo with visible per-message state, and media via the `keeper-media://` protocol (thumbnails render progressively, never block the timeline).

One operator, unlimited Accounts, one window (plus transient overlays). Keyboard is the primary input; pointer is the fallback, never the only path (FR-48–50, NFR-14).

**Phase 2 adds the iPhone** (FR-55–FR-65): the same Rust core, the same React component trees, the same IPC contract, rendered in WKWebView on a phone-width viewport. Two contract extensions carry the whole phase: a **phone layout tier** below `{spacing.phone-breakpoint}` that projects the three-pane frame into a single-pane navigation stack driven by existing selection state (no router, FR-58 and PRD §13.8), and **platform capability flags** over the IPC handshake (FR-57) that remove unsupported surfaces — bbctl, global hotkey, updater, tray/background presence — rather than letting them render dead. On the phone, touch is the primary input and every desktop interaction has a touch path (FR-60); the full mapping lives in `Responsive & Platform`.

**Phase 3 adds screen recording** (FR-66–FR-76), a desktop-macOS-only utility carried by the same capability-flag mechanism (FR-57): a `recording` flag present only on macOS ≥ 13.0 renders the recording surfaces; everywhere else they are absent, never dead. The capture pipeline lives in a first-party Swift sidecar (`keeper-rec`) spawned on demand (the bbctl precedent) — the UI stays a pure renderer of a Rust-owned recording state machine, so a recorder crash can never take the messenger down, and the UI never invents recording state. Recording is local-only: it adds zero network destinations (FR-76). It is a *utility, not a conversation* — it has no chat list, no timeline, no composer — so it lives beside Bridges and Settings, not in the inbox. The full specification is `Screen Recording (macOS — Phase 3)`.

## Information Architecture

Persistent frame: **[Sidebar 260px] [Chat list 320px] [Conversation ≥480px] [Detail panel 320px, toggleable]** — widths and collapse rules in `DESIGN.md.Layout & Spacing`. On the phone tier (< `{spacing.phone-breakpoint}`) the same frame renders one pane at a time as a navigation stack — Inbox → Room → Detail — with the sidebar as a leading drawer; the IA below is unchanged, only its arrangement differs (see `Responsive & Platform`).

**Sidebar** (shadcn `Sidebar`, collapsible to icon rail): traffic-light inset header with app name → primary views (Inbox `⌘1`, Archive `⌘2`, Approval Pane `⌘3` with amber count badge, Bridges `⌘4` with health dot roll-up, and — only when the `recording` capability is on — Recording `⌘5`, carrying a `{components.recording-dot}` while capture is live) → SPACES group (per-Space rows, view-and-filter only, FR-23) → NETWORKS group (filter chips per connected Network with `{components.bridge-health-dot}`) → footer: Account switcher (per-Account avatar + hue dot + sync state) and global sync/offline status.

**Chat list**: Pins strip (circular, top, FR-22) → FAVORITES section (FR-21) → chronological Unified Inbox (FR-18), scoped by whatever sidebar view/filter is active. **Conversation**: header (avatar + `{components.network-badge}`, name, Account chip, incognito chip, mute glyph) → timeline → composer. **Detail panel**: chat info, members, shared media, per-chat controls (mute/mention-only, incognito override, archive, export this chat, view raw Bridge Bot chat).

| Surface | Reached from | Purpose |
|---|---|---|
| First-Run Wizard | First launch; Settings → Set up | Add first Account → Bridge discovery → per-Bridge login; every step skippable and re-enterable (FR-31) |
| Unified Inbox | App open / `⌘1` / global hotkey | All Chats, all Accounts and Networks, chronological; home surface |
| Space-filtered inbox | Sidebar SPACES row / `⌘K` | Named Matrix Space as a filter over the inbox (FR-23) |
| Network filter | Sidebar NETWORKS chip / `⌘K` | Simple per-Network filter (FR-24); one active at a time with Space filter |
| Archive view | Sidebar / `⌘2` | Archived Chats; unarchive; auto-return on new activity (FR-20) |
| Conversation | Chat row / `⌘K` / notification click | Timeline + composer for one Chat |
| Chat detail panel | `⌘I` / header click | Chat metadata, members, media, per-chat settings, export |
| Approval Pane | Sidebar / `⌘3` / `⌘K` | All pending Drafts across Accounts; edit / approve / discard (FR-40) |
| Bridges | Sidebar / `⌘4` | Bridge cards per Network × Account: status, risk tier, login, bbctl (FR-25–30) |
| Bridge login flow | Bridge card / re-link notification / Wizard | Provisioning state machine rendered natively: QR, code entry, success, failure (FR-26/27) |
| Global search | `⌘⇧F` / `⌘K` | Offline FTS across all Accounts with sender/Chat/Network/date filters; deep-links into timelines (FR-34) |
| In-chat search | `⌘F` | Same engine scoped to the open Chat |
| Export | Detail panel / search results / `⌘K` | Chat/Account/full archive → JSON + Markdown, background with progress (FR-35) |
| Multi-account switcher | Sidebar footer / `⌘K` | Account list, per-Account state, add Account, sign out (keep/delete archive choice, FR-6) |
| Settings | `⌘,` | Accounts, Privacy (incognito defaults, undo-send window), Notifications, Archive & Storage, Shortcuts, Recording (desktop, `recording` flag only), Appearance, About/Egress |
| Command Palette | `⌘K` | Fuzzy-find Chats, contacts, and actions; `>` prefix for action mode (FR-48) |
| Hotkey cheat sheet | `⌘?` | Overlay reference of all shortcuts (FR-49) |
| Recording (desktop, `recording` flag only) | Sidebar / `⌘5` / `⌘K` / global Start hotkey | The recording utility view: pre-record setup ↔ active recording ↔ completion, in place (FR-67–75) |
| Recording tray | macOS menu bar | Always-truthful active-state surface: idle / recording / error, elapsed + segment, Stop, Open Recordings Folder (FR-74/75) |
| Settings → Recording | `⌘,` → Recording | Persistent recording defaults: folder, segment size, duration cap, fps, default devices, disk-guard note (FR-71/72) |

Modal discipline: one dialog level; sheets don't stack on dialogs; the palette closes anything below it. Every surface above is reachable from the Command Palette — palette parity is a release gate (FR-48).

## Voice and Tone

Microcopy. Brand voice lives in `DESIGN.md.Brand & Style`: plain, honest, calm. keeper narrates state, never emotes; disclosures name consequences, not legalese.

| Do | Don't |
|---|---|
| "Signal disconnected — re-link to keep receiving messages." | "Oops! Something went wrong with your bridge 😕" |
| "Sent" / "Failed — Retry" / "Queued — sends when you're back online" | "Message delivery unsuccessful. Error 0x2201." |
| "Beeper login uses an unofficial API that may break without notice." | Burying the caveat in a tooltip or docs link |
| "WhatsApp connected in the official Beeper app will not appear here. Running your own bridge is the path to parity." | "Some chats may be unavailable." |
| "Deletes your copy on this Mac. Other people's copies are unaffected. Removal on Telegram is best-effort." | "Are you sure? This cannot be undone!" (as the whole explanation) |
| "Nothing sends without you. Drafts wait here until you approve them." | "AI-powered smart sending!" |
| "Search your archive — works offline." | "Supercharge your message history 🚀" |
| "On iPhone, keeper syncs and notifies only while open. Nothing is lost — messages wait on your homeserver." | Any copy implying background delivery or push before the §13.5 gate opens |
| Risk tier copy verbatim from the tier table (PRD addendum §2) | Softening ("totally safe") or scare-mongering ("you WILL be banned") |
| "Recorded locally. Nothing uploads." / "Reveal in Finder" | "Share your recording ✨" or any upload/cloud/transcription affordance |
| "Recorder stopped unexpectedly — 4 segments were saved. Restart recording?" | A dismissible toast for a recording failure, or a silent stop |
| "Screen Recording permission is off. Open System Settings to grant it. A relaunch may be needed, and macOS re-confirms this monthly." | Hiding the relaunch/monthly-reconfirm quirk, or a spinner that waits on a grant that will never come |
| "Only Zoom's windows and audio are recorded. keeper, other apps, and notification banners stay out of the file." | "Recording your screen" (when app-scoped) |
| Sentence case everywhere; no exclamation marks; "Chat", "Account", "Bridge", "Network", "Recording Session", "segment" capitalized per Glossary | Title Case Buttons, emoji in system copy, "please" in errors |

## Component Patterns

Behavioral. Visual specs live in `DESIGN.md.Components` or in shadcn defaults.

| Component | Use | Behavioral rules |
|---|---|---|
| Chat row | Chat list | Click/Enter opens Chat and focuses composer. Right-click `ContextMenu`: Archive, Mark read/unread, Pin, Favorite, Mute ▸, Mention-only, Incognito for this Chat, Export. Single-key actions when list focused (see Interaction Primitives). Draft-holding Chats show `{components.draft-marker}` (FR-38). Unhealthy-Bridge Chats show a small `{colors.bridge-disconnected}` dot on the network badge. |
| Pins strip | Chat list top | Circular avatars; drag to reorder (FR-22); click opens; pinned Chats leave the chronological flow. Overflow beyond 8 scrolls horizontally. |
| Favorites section | Chat list | Always visible between Pins and inbox; one interaction from anywhere (FR-21); collapse/expand persists. |
| Space / Network chips | Sidebar | Single-select per group; Space filter and Network filter compose (AND). Active filter renders as a dismissible chip above the chat list; `Esc` from list clears filter before moving focus. |
| Account switcher | Sidebar footer | Lists every Account: avatar, hue dot, Homeserver, sync state glyph. Click filters inbox to that Account (toggle); `DropdownMenu` per Account: Settings, Sign out… (opens keep/delete-archive `AlertDialog`, keep is default, FR-6). "Add Account" always last — never gated by count (FR-4). |
| Timeline message | Conversation | Hover/focus reveals action bar: React (emoji `Popover`), Reply, Edit (own), Delete ▸, Copy, Jump-to-original (on reply quotes). Received edits render latest content + "Edited" caption; click opens edit-history popover fed by the Local Archive (FR-11/36). Reactions aggregate with counts; own reaction highlighted, click toggles (FR-12). Redacted events show a stub; the stub notes best-effort remote removal in bridged Chats (FR-15). Read state: per-message ticks on own messages; others' read receipts as micro-avatars at their read position (FR-16). |
| Media message | Conversation | Thumbnail renders before full download; click opens Quick-Look-style preview overlay (`Esc` closes); download progress on the bubble; failed media shows retry. Sends show upload progress and are cancelable during upload (FR-13). |
| History boundary | Conversation | Scrolling past locally archived history shows an inline boundary row: "Older history loads from your homeserver" with a spinner while paginating; offline, it says so and stops (FR-17). |
| Composer | Conversation | `Textarea` autogrows to 8 lines then scrolls. Enter sends (approval action #1, FR-41); ⇧Enter newline; a Settings toggle swaps to ⌘Enter-sends. Text persists per Chat instantly as a Draft (FR-38) and mirrors to account data (FR-39). Attach via button, paste, or drag-drop onto the conversation pane. `↑` in an empty composer edits the user's last message. While Incognito applies, the focus ring tints `{colors.incognito}` and the header chip shows effective scope (FR-42). Typing emits no typing events while Incognito applies (FR-43). |
| Undo-send pill | Above composer | Appears on every approved send when window > 0 (default 10 s, 0–60 s in Settings, FR-46). Radial countdown + "Sending in Ns — Undo". Click or `⌘⇧Z` cancels: zero network dispatch, full text restored to the composer as a Draft. Multiple pending sends stack oldest-first. Reduced-motion: numeric countdown, no ring animation. |
| Approval Pane row | Approval Pane | Groups by Account, then Chat (`section-label` headers). Each row: Chat + Network badge + Account hue, Draft preview, age. `Enter` opens inline editor; `⌘Enter` approves (send — approval action #2, honoring the Undo-Send Window); `⌘⌫` discards with a 5 s sonner Undo. No select-all-and-send affordance in MVP — approving is deliberately per-Draft (FR-41). Layout reserves a leading column for future proposer attribution (post-MVP agents); MVP renders "You" silently. |
| Incognito controls | Header chip, Settings, `⌘K` | Three scopes: global (Settings/palette), per-Account (Account menu), per-Chat (header chip / `⌘⇧I`). Chip always shows the *effective* scope; precedence Chat > Account > Global rendered as "Incognito — this chat overrides account". Toggling on a WhatsApp Chat surfaces the coupling caveat inline at the toggle (FR-44). "Mark read publicly" action on the chip releases one public receipt on demand (FR-45). |
| Bridge card | Bridges | One card per Network × Account: `{components.bridge-health-dot}` + state word (Connected / Action needed / Disconnected / Not set up), `{components.risk-tier-badge}`, last-checked time, primary action → login flow or Manage `DropdownMenu` (Re-link, Log out, Open Bridge Bot chat, View sessions). The raw Bridge Bot Chat is always reachable — never hidden (FR-27). |
| Bridge login stepper | Sheet over Bridges/Wizard | Renders the provisioning state machine natively: Choosing method → Waiting → **QR panel** (per `DESIGN.md`, with "Open WhatsApp → Linked devices → Link a device" per-network instruction) → or code-entry `InputGroup` → Success (dot turns `{colors.bridge-healthy}`, auto-advance after 1.5 s) → Failure (Bridge's own error message verbatim + Retry). QR expiry regenerates in place with a subtle "QR refreshed" note. Identical flow whether backed by the provisioning API or driven Bridge Bot commands (FR-26/27) — the user cannot tell which path ran. |
| Risk acknowledgment | Before volatile-tier connect | `AlertDialog` with tier badge and plain-language ToS/ban copy from the tier table (data-driven, FR-30). Confirm label: "I understand the risk — connect". Low-risk Networks never see a dialog, only the badge. |
| bbctl panel | Bridges (Beeper Accounts) | "Run your own bridge" section: pick Network → keeper drives `bbctl` register/run with a log-free progress stepper; resulting Bridge joins the list. bbctl absent → guided install instructions, everything else unaffected (FR-29). |
| New chat dialog | `⌘N` / `⌘K` | `Dialog`: pick Network + Account (defaults to last used), enter identifier (phone number, username, Matrix ID). keeper resolves through the Bridge (resolve-identifier) with a visible resolving state, then opens the resulting Chat with composer focused (FR-32). Networks whose Bridge lacks resolve support say so upfront instead of failing late. |
| Command Palette | Global `⌘K` | One palette, two modes: default fuzzy-finds Chats (all Accounts; network badge + account dot per result) and contacts; `>` prefix lists actions (Archive, Toggle Incognito, Open Approval Pane, Start Export, Re-link Signal, …) with kbd chips. Results within 100 ms per keystroke at 10k Chats (FR-48). `Enter` executes; `⌘Enter` on a Chat result opens it without closing the palette (peek). Context-aware: actions on the open Chat rank first. |
| Global search | `⌘⇧F` surface | Query + filter chips (sender, Chat, Network, Account, date range) as `InputGroup` + `Badge` chips; results grouped by Chat with `{colors.search-highlight}` on matches; `Enter` deep-links into the timeline at the match, highlighted for 2 s (FR-34). Works fully offline; header states "Searching your local archive". |
| Export dialog | `Dialog` | Scope picker (this Chat / this Account / everything) → format checkboxes (JSON, Markdown) → include-media toggle → destination. Runs in background: sonner progress toast with count, Cancel, and Reveal-in-Finder on completion (FR-35). Export never blocks messaging. |
| Wizard stepper | First-Run Wizard | Steps: Welcome → Add Account (three tabs: Homeserver login / OIDC / Beeper; the honest no-homeserver fork links companion-stack docs, managed hosts, Beeper path) → Bridge discovery (found list with tier badges) → per-Bridge login (reuses Bridge login stepper) → Done (lands in Inbox). Every step has "Skip for now"; wizard is re-enterable from Settings (FR-31). Progress dots, no lock-in, `Esc` asks once then exits to Inbox. |
| Notification | macOS native | Sender + Chat + preview (preview omissible, FR-51); grouped per Chat. Click lands in the exact Chat and Account with the message in view (FR-54). Bridge-health notifications use the same pipeline and deep-link into the re-login flow (FR-28). |
| Cheat sheet | `⌘?` overlay | Full shortcut reference as a searchable `Dialog`; generated from the same registry the palette uses, so it can't drift. |
| Recording view | `⌘5` / palette | Non-chat utility surface flipping in place: **setup** (Source / Audio / Webcam / Destination / Segmenting / Advanced-fps cards) → **active** (`{components.active-recording-banner}` + source summary + `{components.segment-meter}`) → **completion / recovery** card. No timeline, no composer; Start is gated on permission pre-flight. Behavior detail in `Screen Recording`. |
| Source picker | Recording setup | Live list of `{components.source-picker-row}` (Displays, then Applications) re-enumerated as apps launch/quit (FR-68). Single-select. App-scoped capture is disclosed as "only {App}'s windows and audio — keeper, other apps, and notification banners are excluded". A source that vanished before Start yields a clear inline error, never a hung start. |
| Audio controls | Recording setup | System-audio `Switch` (default on; labelled "the audio the recorded content plays", not a device pick — FR-69); microphone `{components.device-picker}` (default "System default input"). Copy states the two are written as separate tracks, not mixed, and that keeper's own notification sounds are excluded. Requesting Microphone permission happens only when this is enabled (FR-67). |
| Webcam control | Recording setup | `Switch` (default off) revealing a camera `{components.device-picker}` (built-in / external / Continuity Camera). Copy: "records to a separate file, synced to the screen" (FR-70); a note that macOS 14+ can composite the camera via the system presenter overlay — an OS behavior, not a keeper feature. Camera permission is requested only when enabled. No self-view bubble, no PiP this phase. |
| Destination + Segmenting | Recording setup | Folder chooser showing the remembered default `~/Movies/keeper` (FR-71) with a validate-on-Start check (exists, writable, free space — NFR-20); segment-size stepper (default 500 MB) with a duration-cap fallback field (default 30 min, FR-72); fps lives in a collapsed Advanced group (30 default, 60 selectable). Changing any of these in the view mirrors Settings → Recording; changes affect the next session only. |
| Permission pre-flight | Recording setup (before Start) | A `{components.permission-row}` per required permission — Screen Recording always; Microphone / Camera only when enabled — each live-detected at render (never cached optimistically, FR-67). Request via system prompt where allowed; deep-link to the exact System Settings pane where only manual granting remains. Start disabled until all green, naming the blocking permission. Honest note lines: relaunch-may-be-needed, macOS 15+ monthly re-confirm, and the subtle dev-facing "ad-hoc dev builds may be blocked on macOS 15+ — sign with an Apple Development certificate". |
| Active-recording banner | Recording view + persistent | `{components.active-recording-banner}`: record dot, `mono` "Recording — 12:34 · segment 3 · 412 MB", Stop. In-app twin of the tray; persistent, never toast-only. Warning variant (mic unplugged, low disk) and error variant (recorder exit, writer stall, permission revoked, disk floor) are the loud-failure surface (FR-75) with "Restart recording". Pause deferred (§14.4). |
| Recording tray | macOS menu bar | `{components.tray-recording}` states idle / recording / warning-error; a `mono` elapsed·segment·size line (disabled item, ~1 Hz tick, live < 1 s of start), Stop Recording, Open Recordings Folder, above Show keeper / Quit. Recording forces the tray visible even when the FR-53 opt-in toggle is off and restores the prior state exactly at Stop. Quitting keeper while recording warns, then stops and finalizes before exit (never orphans the recorder). macOS's own purple pill is left untouched. |
| Completion / recovery card | Recording view + notification | On Stop: "Saved N segments · {size}" + session-folder path (`mono`) + **Reveal in Finder**; no preview/trim/share (FR-76). On startup or before a new recording, an interrupted session surfaces once as "A recording was interrupted; N segments were saved" linking the folder; recovered files play as-is (FR-73). |

## State Patterns

Every state below is persistent-by-default: anything representing risk or loss stays visible until resolved. Toasts are for confirmations and progress only, never the sole carrier of an error (NFR-5).

| State | Surface | Treatment |
|---|---|---|
| Cold start | Whole frame | Cached inbox + last-open Chat render immediately from the Local Archive (`Skeleton` rows only on true first run); sync convergence continues silently; interactive < 2 s (NFR-1). |
| First run, no Accounts | Whole frame | Wizard replaces the frame. Skipping everything lands in an empty Inbox with a single card: "Add an account to start" → wizard step 2. |
| No Homeserver | Wizard | The honest fork, in this order: companion-stack docs, managed-host pointers, Beeper Account path. No fake "sign up" — keeper has no server. |
| SSS unsupported | Login | Blocking inline error before Account creation: names Simplified Sliding Sync as the missing capability, links docs (FR-5). No partial Account remains. |
| OIDC browser cancelled | Login | Returns to login screen, no partial Account, no error dialog — a quiet "Login cancelled" inline note (FR-2). |
| Beeper login unavailable | Login | Distinct state: "Beeper login unavailable — this is an unofficial API and may have changed." Retry + status-docs link. Never a spinner that hangs, never a crash (FR-3). |
| Beeper coverage | Beeper login (pre-completion) + Account settings | On-Device Connection disclosure card naming what breaks: "WhatsApp connected in the official Beeper app will not appear here" (FR-7). |
| Syncing / offline | Sidebar footer | Per-Account glyph: syncing (spinner), synced (nothing), offline (gray). Global offline: persistent footer pill "Offline — showing your local archive. Messages queue until you're back." No toast spam on flapping. |
| Empty Inbox (accounts, no chats) | Chat list | "Synced. No conversations yet." + actions: Start a chat (`⌘N`), Set up bridges (`⌘4`). |
| Empty Space / Network filter | Chat list | "No chats in {filter}." + Clear filter action. |
| Empty Archive view | Chat list | "Nothing archived. `E` archives a chat and keeps it searchable." |
| Empty Favorites | Chat list | Section hidden until first Favorite exists; a one-time hint appears in the chat-row context menu instead. |
| Message sending | Timeline | Local echo immediately; caption cycles Held (amber, undo window) → Sending… → Sent. |
| Message failed | Timeline | Persistent destructive caption "Failed — Retry"; message never disappears; retry re-enters the send pipeline including the undo window (FR-9, NFR-5). |
| Queued offline | Timeline | Amber caption "Queued — sends when you're back online"; dispatches on reconnect (window already elapsed → immediate, FR-46). |
| Unable to decrypt | Timeline | Explicit stub: "Can't decrypt yet — verify this device or restore key backup" + inline action to the verification flow (FR-14). Never blank. |
| Device unverified | Global banner (dismiss-to-badge) | Post-login banner "Verify this device to read encrypted history" → verification flow (emoji/SAS or QR). Dismissing collapses it to a persistent badge on Settings, not gone. |
| Bridge degraded / disconnected | Bridges + chat list + affected Chats | Within 60 s (NFR-6): card state flips, sidebar Bridges dot rolls up worst state, affected Chats' rows get the health dot, conversation shows a persistent inline banner "Signal disconnected — messages may not arrive. Re-link" → login flow. Plus one native notification. Banner is not dismissible while unhealthy (FR-28). |
| Bridge discovery empty | Bridges / Wizard | "No bridges found on {homeserver}." + companion-stack docs link (FR-25). |
| Provisioning failure | Bridge login stepper | Failure state with the Bridge's error verbatim, Retry, and "Open Bridge Bot chat" as the manual escape hatch (FR-26/27). |
| Draft conflict | Composer | Local text wins; a quiet chip above the composer: "Edited on another device — Use that version" for one-tap adoption (FR-39). Local unsent text is never silently destroyed. |
| Approval Pane empty | Approval Pane | "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you." |
| Palette: no matches | `⌘K` | "No matches." followed by the top 4–5 registered actions so the surface never dead-ends; `>` hint shown when the query looks like a verb. |
| New chat: not found | New chat dialog | Inline "Not found on {Network} — check the number or username." Input retained for correction; no dialog dismissal (FR-32). |
| Search: no results | Global search | "No matches in your archive." + active filter chips shown for one-tap removal; offline note stays visible. |
| Search: cross-account result identity | Global search | Same contact via two Accounts always differ by account hue dot + Account name in the result meta (FR-24). |
| Export running / done / failed | sonner + Export surface | Progress toast with counts and Cancel; completion adds Reveal in Finder; failure is a persistent alert in the Export surface (not toast-only) with partial-file cleanup noted. |
| Sign-out | Account switcher | `AlertDialog`: default "Sign out, keep local archive" (FTS/Export keep working, FR-37); separate explicit destructive option "…and delete this Account's archive" requiring the Account name typed (FR-6). |
| Remote edit/delete vs. archive | Timeline + Settings | Timeline always honors redactions (stub) and shows latest edits; the Local Archive preserves priors by default. Settings → Archive & Storage carries the disclosure and the "Honor remote deletions locally" toggle (FR-36). |
| Notification previews off | macOS notifications | Sender + Chat only, no content (FR-51). |
| DND / muted | Notifications | Global DND toggle in sidebar footer menu; muted Chats/Networks accumulate unread silently (FR-52); mention-only Chats notify on mentions and replies-to-user only. |
| App quit vs. background | Settings copy + quit | Window close keeps syncing (menu-bar presence optional); `⌘Q` fully stops sync and Settings says exactly that — no fake push promise (FR-53). |
| Recording: capability off | Everywhere | On iOS and macOS < 13.0 no recording affordance renders — no sidebar entry, no Settings section, no palette action, no tray items; not a disabled state, an absent one (FR-66). |
| Recording: permission blocked | Recording setup | Start disabled; the blocking permission named inline with its System Settings deep-link and the macOS quirk notes; state is re-detected on focus/return, so granting-and-returning updates it without a manual refresh (FR-67). |
| Recording: idle → armed | Recording view + tray | Setup complete and permissions green: Start enabled. Within 1 s of Start the tray flips to recording and the in-app banner appears; macOS posts its own purple pill in parallel (FR-74). |
| Recording: running | Banner + tray + segment meter | `mono` elapsed·segment·size ticks ~1 Hz; the `{components.segment-meter}` fills toward the segment size and resets at each gapless rotation (FR-72); source summary stays visible. |
| Recording: warning (non-fatal) | Banner + tray badge | Mic hot-unplug (video + system audio keep rolling, mic track silence-filled, fallback to default input attempted) or free space below the warn threshold: a persistent warning state until resolved or acknowledged — never a dismissed-and-gone toast (FR-69/75, NFR-20). |
| Recording: loud failure | Banner (error) + notification + tray error | Recorder crash/exit, writer stall, permission revoked mid-record, device loss, or disk hard-floor: tray → error and a native notification within 5 s, "Restart recording" offered; already-written segments intact; the manifest records the true terminal status. No recording fault is silent (FR-75, extends NFR-5). |
| Recording: disk floor stop | Banner + tray | Below the hard floor keeper stops-and-finalizes gracefully rather than dying mid-write, and says so; it never runs the volume to exhaustion (NFR-20). |
| Recording: source vanished | Recording setup | A picked display/app that disappeared before Start yields a clear inline error at Start, never a hung recording (FR-68). |
| Recording: stopped / completed | Completion card | Current segment finalized; "Saved N segments · {size}" + folder path + Reveal in Finder; tray returns to its exact prior configuration (FR-71/74). |
| Recording: interrupted → recovered | Completion-shape notice | On startup / before a new recording, an unfinalized session is marked recovered and surfaced once ("A recording was interrupted; N segments were saved") linking the folder; files play as-is, no remux (FR-73). |
| Recording: quit while recording | Quit path | `⌘Q` warns first, then stops and finalizes the current segment before exit (kill-timeout guarded) — extends FR-53's quit honesty; never orphans the recorder (FR-74). |

## Interaction Primitives

**Keyboard-first.** The UJ-3 triage loop (walk unreads → archive → reply → next) must complete with zero pointer use (FR-49). Shortcuts follow macOS conventions; every action here is also in the palette and the native menu bar. [ASSUMPTION] Assignments beyond the PRD-mandated `⌘K` and configurable global hotkey are authored here per macOS/Beeper convention — the cheat sheet and palette registry are the single source of truth in code.

**Global (system-wide)**
- Configurable global hotkey, default `⌃⌥Space` — summon/hide keeper, focus chat list (FR-50); conflicts detected at assignment time.

**Navigation**
- `⌘K` Command Palette · `>` prefix for actions
- `⌘1` Inbox · `⌘2` Archive · `⌘3` Approval Pane · `⌘4` Bridges
- `⌘⇧F` global search · `⌘F` search in Chat · `⌘,` Settings · `⌘I` detail panel · `⌘N` new Chat
- `⌃Tab` / `⌃⇧Tab` next / previous Chat · `⌥⌘↓` / `⌥⌘↑` next / previous **unread** Chat
- `↑`/`↓` or `j`/`k` move selection in any list · `Enter` open · `Esc` walks up: overlay → composer → timeline → clear filter → chat list

**Chat list (row focused, single-key)**
- `e` archive/unarchive · `u` toggle read/unread · `p` pin/unpin · `f` favorite/unfavorite · `m` mute menu (mute / mention-only / unmute)

**Conversation & composer**
- `Enter` send (approval action; swappable to `⌘Enter` in Settings) · `⇧Enter` newline
- `⌘⇧Z` undo send while the countdown pill is visible (`⌘Z` stays text-undo)
- `↑` in empty composer: edit last own message · `Esc` cancel edit/reply
- Timeline focus: `↑`/`↓` select message · `r` reply · `e` edit own · `⌫` delete ▸ (AlertDialog; redaction framing per FR-47)
- `⌘⇧I` toggle Incognito for the open Chat

**Approval Pane**
- `j`/`k` move · `Enter` edit inline · `⌘Enter` approve (send) · `⌘⌫` discard (5 s undo toast)

**Recording (desktop, `recording` flag only)**
- `⌘5` open the Recording view · palette actions "Start recording", "Stop recording", "Open recordings folder" (registered only behind the flag, FR-66/48)
- Optional configurable global **Start/Stop Recording** hotkey (unset by default; assigned in Settings → Shortcuts alongside the summon hotkey, conflict-checked at assignment) — Stop is always one click from the tray regardless
- No single-key verbs here (this is not a list surface); Stop in the active view is a focusable button, `Esc` does **not** stop a recording (destructive-by-omission guard — stopping is always explicit)

**Meta**
- `⌘?` cheat sheet · `⌘W` close window (sync continues) · `⌘Q` quit (sync stops)

**Pointer:** everything reachable by click and context menu; drag only for Pin reorder and media drop. **Touch (phone tier):** every action above has a touch equivalent — the desktop→touch mapping table in `Responsive & Platform` is normative; keyboard *accelerators* (⌘-chords, single-key verbs) are input shortcuts, not actions, and need no gesture twin as long as the action itself is touch-reachable (FR-60). **Banned everywhere:** hover-only affordances without a keyboard/focus equivalent, dismissible-only error states, modal stacks deeper than one, infinite spinner states without copy, drag as the sole path to any action, gestures as the sole path to any action.

## Trust & Disclosure Surfaces

keeper's differentiator is honesty rendered as UI. These rules bind every surface:

- **Risk tiers (FR-30):** every Network shows its `{components.risk-tier-badge}` at setup and in the Bridge list. Volatile tier requires the acknowledgment dialog before connect. Tier copy is data-driven from the addendum table — UI renders whatever the data says, no hardcoded strings.
- **Unofficial API labels (FR-3):** the Beeper login tab is permanently subtitled "Unofficial API — may break without notice." The label is part of the form, not a dismissible hint.
- **Coverage disclosure (FR-7):** the On-Device Connection card appears in the Beeper login flow *before* completion and lives permanently in that Account's settings.
- **Best-effort framing (FR-15/47):** every delete-for-everyone confirmation in a bridged Chat names the Network and says removal there is best-effort.
- **Archive divergence (FR-36):** Settings → Archive & Storage states plainly that keeper keeps your local copy of remotely edited/deleted messages by default, that this affects only this Mac, and offers the "Honor remote deletions locally" toggle.
- **Explicit-approval invariant (FR-41):** the Approval Pane empty state and Settings → Privacy both carry "Nothing sends without you." No UI ever offers scheduled, background, or bulk dispatch.
- **Egress honesty (NFR-11):** Settings → About lists every endpoint keeper talks to (your Homeservers, Beeper API if used, the update endpoint) — a rendered list, not a doc link.
- **Incognito coupling (FR-44):** per-Network caveats surface at toggle time, inline, from the same data structure as risk tiers.
- **Local-only recording (FR-76):** the recording setup states "Recorded locally. Nothing uploads." plainly; no upload, share-link, transcription, or cloud affordance exists anywhere in the recording UI, and Settings → About's egress list (NFR-11) gains nothing for this phase — the recording feature adds zero network destinations, and that emptiness is the disclosure.
- **No silent recording loss (FR-75, extends NFR-5):** every started Recording Session reaches a user-visible terminal state — finalized, recovered, or failed-with-reason. Recording faults are persistent tray-error + banner + notification, never a dismissible toast (the FR-28 persistence rule applied to capture).
- **App-scoped capture disclosure (FR-68):** recording a single application discloses inline that only that app's windows and audio are captured — keeper, other apps, and notification banners are excluded from the file.
- **Recording permission honesty (FR-67):** macOS's quirks are stated, not hidden — relaunch-after-grant, the macOS 15+ monthly re-confirm, and the dev-facing ad-hoc-signing caveat; permission state is always live-detected, never optimistically cached.

## Accessibility Floor

Behavioral floor per NFR-14 ("operable and labeled"); visual contrast lives in `DESIGN.md` (AA in both themes, validated against the solid sidebar fallback).

- **Keyboard-only:** every MVP flow — wizard, bridge QR login, approval, export, search, settings — completes with keyboard alone; the shortcut set above is a superset of FR-48–50. All shortcuts also exist as native menu-bar items so macOS full-keyboard-access and VoiceOver users get standard discovery.
- **VoiceOver:** every interactive control carries a label including dynamic state: "WhatsApp bridge, disconnected, button", "Chat, Marta Kowalska, Telegram, work account, 3 unread, draft pending". Timeline messages announce sender, time, content/state ("message failed to send, retry available"). The undo-send pill announces its countdown once, not per second.
- **Live regions:** palette and search results announce via `aria-live="polite"`; new messages in the open Chat announce politely; bridge health changes announce assertively (they are the loss-risk case).
- **Focus:** visible `ring` on every focusable; roving tabindex in chat list, timeline, Approval Pane; focus returns to the invoking element when overlays close; `Esc` semantics are universal (see Interaction Primitives).
- **Landmarks:** sidebar = navigation, chat list = list with posinset/setsize, conversation = main, detail panel = complementary — VoiceOver rotor jumps between panes.
- **Reduced motion:** countdown ring becomes numeric text; pane transitions become cuts; the bridge-dot pulse becomes a static state change.
- **Targets & text:** rows are full-width targets; icon-only controls ≥ 24px with tooltips; all text respects macOS text-size scaling since layout is token-based, not pixel-locked imagery.
- **Phone tier:** the iOS-specific floor (VoiceOver in WKWebView, Dynamic-Type-style rem scaling, 44 pt targets, gesture alternatives) is specified in `Responsive & Platform → Accessibility on iOS` and extends — never replaces — this section.
- **Recording (desktop):** every recording flow — permission pre-flight, source/device pick, Start, Stop, recovery — completes with keyboard alone and mirrors into the native menu bar. VoiceOver labels carry live state: "Recording, 12 minutes 34 seconds, segment 3, button" on the banner; permission rows announce "Screen Recording, denied, Open System Settings, button". The elapsed time announces on demand and on state change, **not** once per second (the undo-pill rule). Recording start/stop and every fault announce **assertively** — they are loss-risk events, like bridge health. The tray menu is reachable via the macOS menu-bar-extra keyboard path (Control-F8 / VoiceOver menu-bar navigation); Stop and Open Recordings Folder are real, labelled menu items. `Esc` never stops a recording — stopping is always an explicit focusable action.

## Responsive & Platform

Two shipped surfaces, one codebase, one IPC contract: macOS desktop (MVP) and iPhone (Phase 2, PRD §13). Width alone selects the tier; there is no platform-forked UI.

| Viewport width | Behavior |
|---|---|
| ≥ 1280px | Four panes possible: sidebar + chat list + conversation + pinned detail panel |
| 1080–1279px | Detail panel becomes a `Sheet` over the conversation (`⌘I` toggles it) |
| 940–1079px | Sidebar auto-collapses to the 48px icon rail (tooltips carry labels); chat list stays |
| 768–939px (desktop) | Minimum window 940 × 600 enforced; below-minimum resize is blocked, not squished |
| < `{spacing.phone-breakpoint}` | **Phone tier**: single-pane navigation stack (below); desktop and tablet tiers unchanged at ≥ 768px (FR-58) |

**macOS platform integration:** overlay titlebar with traffic-light insets per `DESIGN.md` (draggable header regions across all three panes); native macOS menu bar mirrors every command; dock badge shows unread count (Settings: all unreads / mentions only / off); optional menu-bar extra + launch-at-login keep sync alive windowless (FR-53); light/dark follow the system by default with manual override (Settings → Appearance); full-screen and Stage Manager behave as standard document windows; notification click-through restores or summons the window into the right Chat (FR-54).

### Phone tier — navigation stack (FR-58)

One stack, three levels, all full-screen, all reusing the desktop component trees unchanged:

```
Level 0  Inbox    — chat list (Pins strip → FAVORITES → inbox), scoped by the active view/filter
Level 1  Room     — conversation header → timeline → composer
Level 2  Detail   — the detail panel content as a pushed page (not a Sheet at this width)
```

The stack is a projection of existing selection state (`selectedRoomId`, detail-open) — no router this phase; `history.pushState` integration is an optional enhancer so the system back gesture carries sensible semantics (PRD §13.8). Deep links (notification tap, FR-54) set selection state directly and the stack renders at the right level with back leading to the Inbox.

- **Push transition:** new level slides in from the trailing edge over ~250 ms ease-out while the level beneath shifts back ~25% and dims slightly; pop reverses. Reduced-motion: cuts, no slide.
- **Back affordances, in priority order:** (1) `{components.phone-header}` back chevron labeled with the previous level ("Inbox", or the Chat name on Detail), full `{spacing.touch-target-min}` hit area; (2) **edge-swipe back** from the leading edge — an interactive gesture on the stack container (WKWebView grants no native `UINavigationController` swipe to an in-page stack) that tracks the finger and commits past 50% travel or on a flick, cancels otherwise; (3) system back gesture via the optional history integration. Back always returns to the Inbox **preserving scroll position** (FR-58).
- `Esc` semantics map to back: sheets and the drawer dismiss by swipe-down / scrim tap / their Close affordance; the media preview overlay dismisses by swipe-down.
- Opening a Chat on the phone does **not** auto-focus the composer (desktop's Enter-opens-and-focuses rule would pop the keyboard over the timeline on every open); the user taps the composer to type. Everything else about opening a Chat is unchanged.

### Phone tier — where everything lives

**Sidebar → leading drawer.** The entire desktop sidebar — primary views (Inbox / Archive / Approval Pane with amber count / Bridges with health roll-up), SPACES, NETWORKS chips, Account switcher footer, sync/offline status — renders verbatim inside a leading `Sheet` (drawer). Opened by the avatar button at the top-leading corner of the Inbox header, or by edge-swipe from the leading edge **at stack level 0 only** (deeper levels reserve that edge for back). Selecting a view or filter closes the drawer and applies to the Inbox; the active filter still renders as a dismissible chip above the chat list, exactly as on desktop.

**Decision — Archive, Approval Pane, and Bridges live in the drawer; there is no bottom tab bar.** Justification, on the record:

1. **Frequency asymmetry.** The Inbox is where every session lives; Archive and Bridges are maintenance surfaces, the Approval Pane is bursty (UJ-6 mornings). A tab bar spends permanent vertical chrome — in a chat app whose composer already competes with the keyboard for the same edge — on destinations visited a few times a day.
2. **Projection, not redesign.** The drawer *is* the desktop sidebar in a Sheet: zero new IA, zero new components, and the FR-57 capability flags compose with it for free (a hidden bbctl panel is simply absent from a list, not a missing tab).
3. **Honesty does not depend on navigation chrome.** The states that must never hide — bridge death, pending drafts — are surfaced independently of the drawer: the Inbox-header **status cluster** (below), persistent in-chat banners, row health dots, and notifications (FR-28). A tab badge would be a third copy, not new information.
4. **Platform-neutral.** The drawer carries unchanged to Android and iPad tiers later (PRD §13.4); a bottom tab bar would be re-litigated per form factor.

**Inbox header (level 0), leading → trailing:** avatar/drawer button — carrying a worst-state `{components.bridge-health-dot}` overlay when any Bridge is unhealthy, and the Account-filter state when one is active — then the view title ("Inbox", "Archive", or the active Space name), then the **status cluster**: an amber Approval chip (`{components.unread-badge}` shape in `{colors.accent}`) showing the pending-Draft count whenever it is > 0 and deep-linking to the Approval Pane, a search (magnifier) button, and a compose (new-Chat) button. Both nav-critical badges thus stay visible without tabs; when everything is healthy and nothing is pending, the header is quiet.

**Room header (level 1):** back chevron → avatar + `{components.network-badge}` → name + Account chip (tap anywhere on the identity block pushes Detail, replacing `⌘I`) → incognito chip when applicable → overflow (⋯) menu carrying the header-adjacent desktop actions: Search in chat, Mute ▸, Mention-only, Incognito for this Chat, Archive, Export.

**Settings** stay at the drawer footer (gear beside the Account switcher). **Wizard, Bridge login stepper, risk acknowledgment, export dialog, new-chat dialog** all render as their existing components — full-screen or sheet at this width per shadcn responsive behavior — with safe-area padding; no phone-specific redesign.

### Phone tier — search (⌘K + ⌘⇧F merged)

One full-screen **Search** surface replaces both the Command Palette and the global-search window on the phone, entered two ways: the header magnifier, or **pull-down on the Inbox list** — a short pull reveals a search field pinned above the list (iOS Messages pattern, satisfying FR-58's "pull-down search"); pulling on past the reveal threshold becomes pull-to-refresh (FR-60) with the spinner appearing beyond the field. The two gestures are one continuous axis, not competing recognizers.

Search has three scopes as segmented tabs, all backed by the same engines and the same 100 ms / offline bars (FR-48, FR-34):

| Scope | Desktop equivalent | Content |
|---|---|---|
| **Chats** (default) | `⌘K` default mode | Fuzzy-find Chats and contacts across Accounts; result rows show network badge + account hue dot |
| **Messages** | `⌘⇧F` global search | Offline FTS with the same filter chips (sender, Chat, Network, Account, date); results deep-link into timelines at the match |
| **Actions** | `⌘K` `>` mode | The full action registry — Archive, Toggle Incognito, Start Export, Re-link Signal, Sync now, … — context-aware exactly as on desktop |

Typing `>` as the first character jumps to Actions, preserving desktop muscle memory. **In-chat search** (`⌘F`) maps to Room overflow → "Search in chat", which opens Search on Messages pre-filtered to the open Chat. Palette parity remains the release gate: every registered action must be reachable from Actions scope on the phone (FR-48 + FR-60).

### Phone tier — composer

Bottom-anchored bar: `bottom: calc(var(--kb-inset, 0px) + env(safe-area-inset-bottom))`, per `{spacing.kb-inset}` driven by `visualViewport` listeners (`interactive-widget=resizes-content` is the evaluated simpler fallback, PRD §13.7 risk 3). Behavior deltas from desktop, everything else identical (drafts, mirroring, incognito tint, undo pill):

- **Send is a button.** A `{spacing.touch-target-min}` primary-tinted send button sits trailing; tapping it is approval action #1 (FR-41). The on-screen **return key inserts a newline** — mobile convention, and it removes any chance of an accidental send from the software keyboard. A hardware keyboard, when attached, follows the desktop Enter/⌘Enter setting.
- Autogrow to 5 lines then scroll ([ASSUMPTION] 8 desktop lines would eat a phone viewport; not owner-confirmed).
- **Attach** via the + button → the system-native choice: photo library, camera, Files. Paste works; drag-drop has no phone path (banned-list compliant — the button is the primary path on desktop too).
- Keyboard opens: composer lifts, and a timeline already at bottom stays pinned to bottom; keyboard dismisses (drag-down on the timeline dismisses it interactively): layout restores with no stranded offsets (FR-59).
- The undo-send pill floats above the composer as on desktop; tap replaces `⌘⇧Z`. Reduced-motion numeric countdown unchanged.

### Phone tier — touch idiom mapping (normative, FR-60)

Every desktop affordance and its phone equivalent. Accelerator-only chords (marked —) have no gesture twin because the action itself is touch-reachable elsewhere; that is the FR-60 contract.

| Desktop | Phone |
|---|---|
| `⌘K` palette — Chats / contacts | Search surface (magnifier / pull-down), **Chats** scope |
| `⌘K` `>` actions | Search → **Actions** scope (`>` prefix still jumps there) |
| `⌘⇧F` global search | Search → **Messages** scope |
| `⌘F` in-chat search | Room overflow → Search in chat |
| `⌘1` Inbox | Stack level 0 / drawer row |
| `⌘2` Archive · `⌘3` Approval Pane · `⌘4` Bridges | Drawer rows; amber count + health dot also on the Inbox header (status cluster) |
| `⌘,` Settings | Drawer footer gear |
| `⌘I` detail panel | Tap the Room-header identity block → Detail push |
| `⌘N` new Chat | Compose button, Inbox header |
| `⌃Tab` / `⌥⌘↓` chat/unread walking | — (tap rows; unread ordering and badges carry the triage loop) |
| Chat-list single keys: `e` archive · `u` read/unread · `m` mute | Row swipes: trailing swipe → Archive + More (mute ▸); leading swipe → read/unread toggle; visuals per `{components.swipe-action}`; full-swipe commits the first action |
| `p` pin · `f` favorite + rest of row context menu | Long-press row → the same `ContextMenu` (Pin, Favorite, Mention-only, Incognito for this Chat, Export, …) |
| Right-click anywhere | Long-press → identical context menu (WKWebView synthesizes `contextmenu`; system callout/tap-highlight suppressed where custom menus exist) |
| Hover action bar on message | Long-press bubble → context menu: React (emoji row on top), Reply, Edit (own), Delete ▸, Copy, Jump-to-original |
| `↑` edit last own message | Long-press own bubble → Edit |
| `Enter` send / `⇧Enter` newline | Send button / return key (see Composer) |
| `⌘⇧Z` undo send | Tap **Undo** on the pill |
| `⌘⇧I` incognito this Chat | Room-header incognito chip / overflow menu |
| `Esc` | Back chevron · edge-swipe back · swipe-down on sheets/overlays |
| Drag Pin reorder | Long-press-and-drag within the Pins strip |
| Drag-drop media | Attach button + paste |
| Approval Pane: `Enter` edit · `⌘Enter` approve · `⌘⌫` discard | Row tap → inline editor; explicit **Approve** button per row (≥ 44 pt); trailing swipe → Discard with the 5 s undo toast; still no approve-all (FR-41) |
| `⌘?` cheat sheet | Hidden on the phone tier — it documents keyboard accelerators ([ASSUMPTION] reappears only if a hardware-keyboard mode is ever specced) |
| Global hotkey `⌃⌥Space` (FR-50) | Capability off (FR-57): the Shortcuts settings section does not render |
| `⌘W` / `⌘Q` | — (iOS lifecycle; honesty copy per FR-61 below) |
| Export "Reveal in Finder" | Destination via the system document picker; completion toast action "Open in Files" |
| Notification click (FR-54) | Identical: lands in the exact Chat at the message, stack set to level 1 |

**Pull-to-refresh** on the Inbox visibly kicks the sync loop — the same operation as foreground resume (FR-60/61). It is a reassurance gesture over an already-live loop: the spinner resolves when the sync round-trip completes; offline, it resolves into the persistent offline pill, never an error toast.

### Phone tier — iOS glue (FR-59, NFR-15–18)

- **Safe areas:** `viewport-fit=cover` + native inset behavior pinned (`contentInsetAdjustmentBehavior = .never`); `{spacing.safe-area}` vars pad the header, composer, drawer, sheets, and overlays in portrait and landscape. The window/launch background matches the active theme — no flash on launch or rotation. Landscape is supported with the same stack; no separate layout.
- **Scroll containment:** the timeline scroller uses `overscroll-behavior: contain` so rubber-banding never tugs the shell; momentum scrolling is default.
- **Touch targets:** every tappable ≥ `{spacing.touch-target-min}`; rows are full-width targets (already 64px tall); icon buttons pad to 44 pt regardless of glyph size.
- **Reduced motion:** stack push/pop become cuts; swipe-action reveals snap; countdown numeric; the pull-to-refresh spinner loses its stretch behavior. Same trigger as desktop (system setting).
- **Text scaling:** all type tokens resolve through rem so system text-size changes scale the UI gracefully; layouts must hold at ~130% scaling (chat rows grow, nothing truncates to uselessness). Full Dynamic Type adoption is fit-and-finish, not phase-gating (FR-60 [ASSUMPTION], PRD §12).

### Phone tier — capability flags rendered honestly (FR-56/57, FR-61)

Absent capabilities are **removed, then disclosed once** — never dead buttons, never silent gaps:

- **Removed surfaces on iOS:** the bbctl "Run your own bridge" panel (Bridges is otherwise complete: discovery, provisioning login, Bridge Bot fallback, health + re-login, risk tiers, new-Chat); the Shortcuts/global-hotkey settings section; updater controls; tray/menu-bar + launch-at-login options; the hotkey cheat sheet.
- **The disclosure surface:** Settings → About gains an **"On this iPhone"** rendered list — same posture as the egress list (NFR-11), a list in the UI, not a docs link: "Syncs and notifies only while keeper is open — background notifications await an explicit future decision" · "No self-hosted bridge runner (bbctl) — manage self-hosted Bridges from your Mac" · "No global hotkey" · "Updates arrive by reinstall — this build's signature renews every 7 days" · link to `docs/ios.md`.
- **Lifecycle honesty copy (FR-61):** shown once as a card on the iOS first run (Wizard Done step or first Inbox render for an existing Account) and permanently in Settings → Notifications: *"On iPhone, keeper syncs and notifies only while open. Close it and messages wait on your homeserver until you return — nothing is lost, and nothing here pretends to be push."* No surface anywhere implies background delivery (extends FR-53's honesty rule).
- **Archive honesty:** Settings → Archive & Storage on iOS adds one line: the phone's Local Archive is excluded from device backup; the Mac remains the durable, exportable copy this phase (FR-65 [ASSUMPTION], PRD §12).
- Programmatic reach of a disabled capability (e.g. a palette action registered desktop-only) returns the clean "unsupported on this platform" state — but per FR-57 such actions are unregistered on iOS, so Actions scope simply lacks them.

### Phone tier — states

Additive to `State Patterns`; everything not listed behaves as specified there.

| State | Surface | Treatment |
|---|---|---|
| Cold start on device | Whole frame | Cached Inbox interactive < 3 s (NFR-15); `Skeleton` rows on true first run only |
| Foreground resume | Whole frame | Cached state renders instantly; sync kicks immediately; new messages < 2 s on Wi-Fi (FR-61) |
| Stale resume (last sync minutes old) | Inbox header | Cached UI at once + a quiet "Connecting…" pill under the header; clears on the first sync response; sync-loop restart guard behind it (NFR-17) |
| Webview jettisoned overnight | Whole frame | Reload guard restores the UI to the last stack level from cached state — never a blank or unresponsive screen (NFR-18) |
| Backgrounded with queued sends | Timeline | Amber caption "Queued — sends when keeper is open and back online"; dispatches on foreground reconnect (undo window already elapsed → immediate) |
| Pull-to-refresh, offline | Inbox | Spinner resolves into the persistent offline pill; no error toast |
| Airplane-mode toggle / Wi-Fi↔cellular handover | Global | Recovers unaided (NFR-17); UI never blanks, offline pill appears/clears; no toast spam on flapping |
| Keyboard open | Room | Composer lifted by `--kb-inset`; bottom-pinned timeline stays pinned; dismiss restores cleanly (FR-59) |
| Notification permission denied | Settings → Notifications | Inline persistent state: "Notifications are off for keeper in iOS Settings." + Open Settings deep link; also noted that the app badge needs the same permission. Never re-prompts on its own |
| Notification for the visible Chat | Notifications | Suppressed (FR-62, reusing desktop logic) |
| App badge | Home screen | Unread aggregate across all Accounts as of the last sync (FR-62 [ASSUMPTION]); the FR-61 disclosure notes it is not live while closed |
| Drawer open | Level 0 | Scrim over the Inbox; scrim tap, edge-swipe, or row selection closes; focus returns to the drawer button |
| Media preview | Overlay | Full-screen, swipe-down or Close dismisses; long-press → Save/Share via system sheet |
| 7-day signature expiry | — | OS-level launch block; out of UI scope — the re-arm ritual and its cost live in `docs/ios.md` (SM-8 tracks it) |

### Phone tier — accessibility on iOS

Extends the Accessibility Floor; VoiceOver here means VoiceOver against the WKWebView accessibility tree (the ARIA labels, roles, and live regions above carry over as-is).

- **Stack navigation:** every push moves VoiceOver focus to the new level's header (back button first in swipe order); every pop returns focus to the element that pushed. The back button is a real, labeled button — the VoiceOver escape gesture (two-finger Z) triggers the same back action at every level, including sheets and the drawer.
- **Gesture alternatives (hard rule):** no gesture is the sole path. Row swipe actions are exposed as VoiceOver custom actions on the row *and* exist in the long-press context menu, which is itself a standard menu; edge-swipe back duplicates the back button; pull-to-refresh duplicates as a "Sync now" action (on the sync status pill and in Search → Actions); pull-down search duplicates as the header magnifier.
- **Long-press menus:** open as accessible menus with focus trapped and returned; the emoji react row is a labeled list, not a hover strip.
- **Announcements:** unchanged from desktop (polite for results and new messages, assertive for bridge health); stack level changes announce the new context ("Inbox", "Chat, Marta Kowalska, Telegram") via the focus move, not a duplicate live-region ping.
- **Dynamic-Type-ish scaling (FR-60):** rem-based type end to end; at large text sizes rows grow and text wraps — nothing clips, nothing becomes a two-line ellipsis where content matters. Full Dynamic Type mapping is fit-and-finish.
- **Targets:** ≥ 44 pt everywhere, including swipe-action buttons (full row height) and the composer send button.

## Screen Recording (macOS — Phase 3)

Desktop-macOS-only, gated behind the `recording` capability flag (FR-66); everything here is absent on iOS and macOS < 13.0. Recording is a **utility, not a conversation** — no chat list, no timeline, no composer — so it does not enter the inbox; it lives beside Bridges and Settings. The UI is a pure renderer of a Rust-owned recording state machine fed by the `keeper-rec` Swift sidecar; the UI never invents recording state.

### IA decision — where recording lives, and why

Three entry points, each earning its place; there is deliberately **no** recording control in the chat surface.

1. **A `Recording` sidebar entry (`⌘5`), capability-gated.** Recording is a first-class utility on the same footing as Bridges — a place you deliberately go to set up and monitor a session — so it belongs in the sidebar's primary-views group, not the chat list (a recording is not a Chat) and not buried in Settings (an active session needs a real home surface, not a preferences pane). The entry carries a `{components.recording-dot}` while capture is live, so the sidebar tells the truth at a glance.
2. **A global start control** — Command Palette actions ("Start recording" / "Stop recording" / "Open recordings folder") and an optional configurable global Start/Stop hotkey (unset by default) — because starting a recording is often something you do *from another app* about to be recorded, so keyboard-first start without hunting for the window matters. Stop is always additionally one click from the tray.
3. **The tray as the always-truthful active surface** (FR-74) — the one place recording state is visible when keeper's window is buried behind the thing being recorded.

**Settings → Recording** holds the persistent defaults (folder, segment size, duration cap, fps, default devices, disk-guard thresholds); the setup view mirrors them and can override per session. Two things are deliberately **not** built this phase: an in-app **recordings browser** (a list of past sessions inside keeper) — MVP is folder-and-Finder plus the tray's Open Recordings Folder; it *could* be added later, folder-backed, but it is out this phase (PRD §14.7 open Q2) — and any **editing/preview/share** affordance, which is out forever (§14.4/FR-76).

### Pre-record setup

One scrollable single column (`{spacing.content-max-width}`, centered) of `Card` sections; sensible defaults are visible and everything is overridable:

- **Source (FR-68).** `{components.source-picker-row}` list, "Displays" then "Applications" (`section-label` headers), live-enumerated and refreshed as apps launch/quit; each row shows name + app icon. Single-select (one target per session this phase). Picking an application discloses inline: "Only {App}'s windows and audio are recorded — keeper, other apps, and notification banners stay out of the file." A source that has vanished before Start errors clearly at Start, never hangs.
- **Audio (FR-69).** System audio `Switch` (default **on**) — labelled as "the audio the recorded content plays", not a device pick (it taps whatever the captured content plays; keeper's own notification sounds are excluded). Microphone `{components.device-picker}` (default **"System default input"**). Copy states plainly that system audio and mic are written as **separate tracks, not mixed** — either side stays removable later.
- **Webcam (FR-70).** `Switch` (default **off**) revealing a camera `{components.device-picker}` (built-in / external / Continuity Camera). Copy: "records to a separate file, synced to the screen." A subtle note: macOS 14+ can composite the camera via the system presenter overlay — an OS behavior, not a keeper feature. No self-view bubble, no PiP this phase.
- **Destination (FR-71).** Folder chooser showing the remembered default `~/Movies/keeper`; validated at Start (exists, writable, free space per NFR-20) with actionable errors. Each session writes a timestamped session folder with a `manifest.json` and segment files.
- **Segmenting (FR-72).** Segment-size stepper (default **500 MB**) with a duration-cap fallback field (default **30 min**) so low-motion recordings still rotate. Rotation is gapless.
- **Advanced (collapsed).** fps (30 default, 60 selectable) at source resolution. Collapsed because the defaults are right for almost everyone.

Group footer, always visible: **"Recorded locally. Nothing uploads."** (FR-76.)

### Permission pre-flight (FR-67)

Before Start, a `{components.permission-row}` per **required** permission — Screen Recording always; Microphone and Camera **only when those sources are enabled** (never requested preemptively). Each row's state is **live-detected at render**, never optimistically cached: `Granted` / `Not requested yet` / `Denied`. keeper requests via the system prompt where the OS allows and deep-links to the exact System Settings pane where only manual granting remains. **Start is disabled until every required permission is green, with the blocking permission named.**

Honest quirk notes (`caption`), because macOS makes them unavoidable:
- Screen Recording: "A relaunch may be needed after granting" and "macOS 15+ re-confirms this grant monthly."
- Dev-facing, subtle: "Ad-hoc dev builds may be blocked on macOS 15+ — sign with an Apple Development certificate." (Shown only in dev/unsigned contexts; a release build never nags a normal user with it.)

Permission revoked mid-recording is not handled here — it is a loud failure (below, FR-75), with already-written segments intact.

### Active recording — in-app state + tray (FR-74)

**In-app** (`{components.active-recording-banner}` pinned to the top of the Recording view): a `{components.recording-dot}`, "Recording", a `mono` line **elapsed · segment # · size on disk** ticking ~1 Hz, a **source summary** (what's being captured, which audio, webcam on/off), the `{components.segment-meter}` filling toward the segment size and resetting each rotation, and a **Stop** button. **Pause is deferred** (§14.4) — not rendered, not greyed; simply absent this phase.

**Tray / menu bar** (`{components.tray-recording}`): the icon flips idle → recording within 1 s of Start; the menu carries a `mono` "Recording — 12:34 · segment 3, 412 MB" line (disabled, ~1 Hz), **Stop Recording**, and **Open Recordings Folder**, above the existing Show keeper / Quit. **Recording forces the tray visible even when the user's opt-in tray toggle (FR-53) is off**, and restores the prior tray state exactly at Stop — a recording indicator that isn't visible is a bug. macOS's own **purple screen-recording pill** stays untouched; keeper's tray adds what the pill lacks (elapsed, segment, Stop, error states). Quitting keeper while recording **warns first, then stops and finalizes** the current segment before exit (kill-timeout guarded) — never orphans the recorder.

### Loud failure surfacing (FR-75) — no silent recording loss

Every fault is loud and persistent — the messenger's no-silent-loss ethos (NFR-5) extended to capture. The triad fires together: **tray error state + native notification (within 5 s) + persistent in-app error banner**, never a toast alone.

- **Fatal** (recorder crash/unexpected exit, writer stall, permission revoked mid-record, disk hard-floor): banner + notification offer **"Restart recording"**; already-written segments are intact; the manifest records the true terminal status.
- **Non-fatal warnings** (mic hot-unplug, low disk) raise a **persistent warning state** until resolved or acknowledged — never dismissed-and-gone. Mic unplug specifically **never aborts**: video and system audio keep rolling, the mic track is silence-filled, keeper attempts fallback to the default input, and the warning stands (FR-69).
- **Disk floor** (NFR-20): below the hard floor keeper **stops-and-finalizes gracefully** and says so, rather than dying mid-write.

Invariant: every started Recording Session reaches a user-visible terminal state — **finalized, recovered, or failed-with-reason.**

### Post-record & recovery (FR-71/73)

On **Stop**: the current segment finalizes and the Recording view shows a completion `Card` — "Saved N segments · {total size}", the session-folder path in `mono`, and a primary **Reveal in Finder**. No in-app preview, scrub, trim, annotate, or share — ever (§14.4/FR-76). The tray returns to its exact prior configuration.

On **startup or before a new recording**, keeper scans for interrupted sessions (manifest still in `recording` state), marks them `recovered`, and surfaces the notice **once**: "A recording was interrupted; N segments were saved" linking the session folder. Recovered fragmented-MP4 files play as-is — no remux step.

**Recordings browser — could / deferred (PRD §14.7 open Q2):** a folder-backed list of past sessions inside keeper (session name, date, duration, size, Reveal-in-Finder per row) is a plausible later addition that would reuse `Card`/list patterns and read the same manifests; it is **out this phase** — MVP is folder-and-Finder plus the tray's Open Recordings Folder. Logged as a spine assumption, not built.

### Recording — accessibility

Extends the Accessibility Floor. Every flow completes keyboard-only and mirrors into the native menu bar. VoiceOver: the banner announces "Recording, 12 minutes 34 seconds, segment 3"; permission rows announce name + state + action ("Screen Recording, denied, Open System Settings, button"); elapsed announces on demand and on state change, not per second. Start/Stop and every fault announce **assertively** (loss-risk, like bridge health). The tray items are real labelled menu items reachable via the macOS menu-bar-extra keyboard path. `Esc` never stops a recording — stopping is always an explicit focusable action. Reduced motion: the record dot never pulses, the segment meter animates by cuts.

## Inspiration & Anti-patterns

- **Lifted from Beeper Desktop (the benchmark):** the three-pane unified inbox with a Spaces/filters rail; Favorites vs. Pins as distinct tiers; inbox-zero archive flow with auto-return; `⌘K` as the everything-surface; incognito with manual read release. keeper's bet is Beeper's shell with ownership underneath.
- **Lifted from Superhuman/Linear:** single-key list verbs (`e`, `u`, `p`, `f`, `m`), palette-first parity for every action, the cheat sheet generated from the action registry.
- **Lifted from Element X:** verification and key-backup UX vocabulary (emoji/SAS, recovery key) — keeper does not invent novel crypto UX, it renders the SDK's flows natively. Patterns only; AGPL code is study-only.
- **Rejected — per-network tabs/workspaces (Ferdium model):** one inbox is the product; Network identity is a badge, never a silo.
- **Rejected — toast-only error surfaces:** anything that risks message loss (bridge death, failed send) is persistent until resolved. This is a direct answer to Beeper's top complaint (silent bridge disconnects).
- **Rejected — cloud-assisted conveniences:** no "delivers even when off" promises anywhere; deferred features (scheduled send) will say "app must be running" in the same breath as the feature name.
- **Rejected — gamification, celebration animations, streaks:** archival calm; inbox zero is its own reward.
- **Rejected — hiding the Bridge Bot:** the raw bot Chat stays reachable behind every native flow; keeper wraps, never walls.
- **Lifted from CleanShot X (recording UX gold standard):** source / audio / webcam in one calm panel, device pickers remembered per mode, sensible visible defaults — CleanShot-grade simplicity over OBS's verbose settings wall (research §3). Its auto-DND-while-recording is admired but deferred (§14.4).
- **Lifted from Screen Studio & Cap Studio mode:** screen and camera recorded as *separate synchronized files*, composed only at export — which for keeper means never, because keeper doesn't edit. This dodges live GPU compositing entirely (research §5).
- **Lifted from OBS "Automatically split output" + record-to-crash-safe-container:** the only mainstream size/time-batched capture; keeper takes the mechanic (size-based segment rotation) and the crash-safety posture (fragmented MP4) while rejecting the settings-heavy UI (research §3/§4).
- **Rejected — Loom/Cap instant-share and webcam burn-in:** no share link, no cloud, no PiP burn-in; keeper records to local files and hands them to Finder. Sharing is the user's act with ordinary files, outside keeper (FR-76).
- **Rejected — QuickTime's moov-at-end format:** a crash loses the whole file; keeper's fragmented-MP4 + segment recovery is the direct answer (research §3/§4, FR-73).
- **Rejected — any in-app editor, preview scrubber, or trim:** keeper records; it does not trim, annotate, or compose (§14.4). The recording UI ends at Reveal in Finder.
- **Rejected — silent recording failure:** the whole point of the tray/banner/notification triad is that a dead recorder is never discovered hours later at Stop — the messenger's no-silent-loss ethos (NFR-5) extended to capture.

## Key Flows

### Flow 1 — Marek connects his homeserver and sees WhatsApp go green (UJ-1)

1. Marek launches keeper first-run; the Wizard opens on Welcome → Add Account.
2. He enters `synapse.marek.dev`; well-known discovery resolves it; the server runs MAS, so keeper opens his system browser for OIDC and returns signed in. SSS verification passes silently.
3. Wizard step: Bridge discovery lists WhatsApp and Telegram cards, each with a risk-tier badge (Maintenance-heavy / Low risk) and "Not set up".
4. He clicks WhatsApp → the Bridge login stepper renders the QR natively with the "Open WhatsApp → Linked devices" instruction; he scans with his phone.
5. **Climax:** the state word flips to Linked ✓, the dot goes `{colors.bridge-healthy}`, the stepper auto-advances — and behind the Wizard his Unified Inbox is already streaming WhatsApp and Telegram Chats. No `!wa login` was ever typed.
6. He skips Telegram for later ("Skip for now") and lands in the Inbox.

Failure beat: his server has no provisioning endpoint for Telegram — keeper drives the Bridge Bot conversation programmatically and shows the *same* stepper; if that fails, the stepper offers "Open Bridge Bot chat" verbatim-error escape hatch.

### Flow 2 — Sofia escapes the Beeper paywall (UJ-2)

1. Sofia opens Settings → Accounts → Add Account → Beeper tab, permanently subtitled "Unofficial API — may break without notice."
2. She enters her email, receives a code, enters it in the code `InputGroup`.
3. Before completion, the coverage card: "WhatsApp connected in the official Beeper app will not appear here. Running your own bridge is the path to parity." She confirms.
4. Her Beeper Chats stream in — Matrix-native, cloud-Bridge, bbctl rooms — merged into the same Inbox as her self-hosted Account, each row carrying its account hue.
5. **Climax:** two Accounts, one Inbox, zero dollars — and the account switcher's "Add Account" button sits there unchanged, because there is no cap to hit (FR-4).

Failure beat: Beeper's private API changes shape → the distinct "Beeper login unavailable" state with retry and status link; her self-hosted Account is untouched.

### Flow 3 — Devon triages 40 overnight chats before his first meeting (UJ-3)

1. Cold start: cached Inbox interactive in under 2 s; Pins on top, Favorites beneath, 40 unread below.
2. `⌥⌘↓` jumps to the first unread. He walks the list: `e` archives gossip, `u` keeps two for later, `Enter` drops him into a Chat with composer focused, he replies, `Esc` `⌥⌘↓` to the next.
3. The monitored-but-never-answered group renders with the violet incognito chip; he reads it fully — no receipt, no typing signal leaves the machine.
4. Client meeting starts; he clicks the client's Space in the sidebar — the list filters to that client's rooms.
5. **Climax:** inbox zero in four minutes, pointer untouched.

Failure beat: hotel Wi-Fi drops mid-reply — the message shows amber "Queued — sends when you're back online", then dispatches on reconnect; the failed-network case shows persistent "Failed — Retry", never silence.

### Flow 4 — Ingrid catches a dead Signal session (UJ-4)

1. Overnight, Signal's linked-device session expires. Within 60 s of the drop reaching keeper, the Bridges sidebar dot turns red and a native notification posts: "Signal disconnected — re-link to keep receiving messages."
2. She clicks it; keeper opens directly in the Bridge login stepper for Signal, QR rendered.
3. She re-links from her phone; state flips to Linked ✓; the inline banners on her Signal Chats disappear.
4. **Climax:** what silently ate messages for days in Element is a one-minute guided fix.

Failure beat: she ignores the notification — the Bridges row, the card, and every affected Chat keep their persistent unhealthy state; nothing auto-dismisses until the session is healthy again.

### Flow 5 — Ada proves the archive is real (UJ-5)

1. A colleague edited a Telegram message to rewrite an agreement; a vendor's Slack free tier truncated the original thread months ago.
2. `⌘⇧F`; she types the disputed phrase, adds a sender chip and a date range. First results in under 200 ms, offline, matches tinted `{colors.search-highlight}`.
3. `Enter` deep-links into the Telegram timeline at the message; "Edited" caption → edit-history popover shows the original text with timestamps, preserved by the Local Archive.
4. From the detail panel she exports the Chat: Markdown for the dispute, JSON for her records; the sonner progress toast finishes with Reveal in Finder.
5. **Climax:** the platform's rewrite loses to her local copy.
6. Later she signs the Account out, keeping the archive (the default) — search still finds everything.

Failure beat: she wants it gone instead — the separate destructive sign-out option requires typing the Account name, and only then deletes that Account's slice.

### Flow 6 — Noor stages replies at midnight, sends them at 9am (UJ-6)

1. 11:40 pm: Noor writes replies in five sensitive Chats and closes the lid. Every composer's text is already a Draft — persisted locally, mirrored to account data; each Chat row shows the amber draft marker.
2. 8:55 am: `⌘3` opens the Approval Pane — five Drafts grouped by Account, each with Chat, Network badge, preview, and age ("9 h").
3. She edits two inline (`Enter`), approves four (`⌘Enter` each — deliberately no approve-all), discards one (`⌘⌫`, undo toast ignored).
4. One approved message she regrets instantly — the undo-send pill is still counting; `⌘⇧Z` pulls it back; the text lands in that Chat's composer as a Draft again. Zero network dispatch.
5. **Climax:** the Approval Pane as a deliberate airlock — morning-Noor overrides midnight-Noor, and nothing ever sends without her explicit action.

Failure beat: she deletes an already-delivered message — the confirmation says plainly: Matrix redaction issued, removal on the bridged network is best-effort, her own local archive copy follows her Archive & Storage setting.

### Flow 7 — Piotr records a 90-minute workshop and trusts it survived (UJ-7, PRD §14)

1. Ten minutes before his workshop Piotr presses his global Start hotkey; keeper's Recording view surfaces. Source: he picks the "Keynote" application (not the whole display — his Slack and Mail stay out of the file, disclosed right at the picker). System audio is already on; he leaves the microphone on "System default input" and flips Webcam on, choosing his Continuity Camera.
2. The permission pre-flight shows Screen Recording and Microphone **Granted**; Camera reads **Not requested yet** — he clicks Request, the system prompt appears, he allows, the row goes green. Start enables.
3. He clicks **Start**. Within a second the tray shows the record dot and macOS posts its own purple pill; the in-app banner reads "Recording — 00:04 · segment 1 · 12 MB". He switches to Keynote and forgets keeper exists.
4. Forty minutes in, at 500 MB, the segment meter resets and `screen-0002.mp4` begins — gaplessly, no hiccup, `camera-0002.mp4` rotating in lockstep. His AirPods die at minute 70; the tray badges amber and the banner raises a persistent "Microphone disconnected — recording continues on the built-in mic" warning. Nothing stops.
5. **Climax:** at minute 90 he clicks Stop in the tray. The current segment finalizes; the completion card reads "Saved 3 segments · 3.6 GB" with the session folder path and **Reveal in Finder**. Three ordinary `.mp4` files, screen and camera as separate synced tracks, play in QuickTime with no keeper tooling. The tray returns to its prior hidden state.

Failure beat: mid-workshop his Mac's disk crosses the hard floor — keeper does not run it to exhaustion; it stops-and-finalizes gracefully, the tray flips to error, and a notification says "Recording stopped — low disk. 2 segments saved." Later a colleague force-quits the machine during a *different* recording; on next launch keeper surfaces "A recording was interrupted; 4 segments were saved" and the partial tail plays up to its last fragment — the meeting is not lost.

---
name: keeper
status: final
sources:
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/prd.md
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/addendum.md
  - _bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/brief.md
  - _bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/addendum.md
  - _bmad-output/planning-artifacts/research-market-2026-07-03.md
  - docs/project-context.md
created: 2026-07-03
updated: 2026-07-03
---

# keeper — Experience Spine

> macOS desktop (MVP). Paired with `DESIGN.md` (visual identity; token references `{...}` resolve there). Spines win on conflict with any mock or import. FR/NFR numbers reference the PRD. UX benchmark: the Beeper desktop app — left sidebar with unified inbox + Spaces/filter chips, chat list, main conversation pane, right detail panel, ⌘K, keyboard-first.

## Foundation

Single-surface macOS desktop app: Tauri 2 shell, React 19 + TypeScript UI on **shadcn/ui + Tailwind v4** (components already installed in `src/components/ui/`: sidebar, command, dialog, sheet, tabs, context-menu, dropdown-menu, popover, tooltip, scroll-area, avatar, badge, button, card, input, input-group, textarea, label, switch, separator, skeleton, sonner). `DESIGN.md` is the visual identity reference and names the brand-layer overrides; this spine specifies behavior only.

Architecture shapes the experience contract: the UI is a **pure renderer of Rust-owned view models** streamed over IPC. Every state this document names (send states, bridge health, sync status, draft persistence) is authoritative in the Rust core; the UI never invents state, only renders it. Consequences the UX depends on: cached-first rendering (inbox and timelines paint from the Local Archive before network), optimistic local echo with visible per-message state, and media via the `keeper-media://` protocol (thumbnails render progressively, never block the timeline).

One operator, unlimited Accounts, one window (plus transient overlays). Keyboard is the primary input; pointer is the fallback, never the only path (FR-48–50, NFR-14).

## Information Architecture

Persistent frame: **[Sidebar 260px] [Chat list 320px] [Conversation ≥480px] [Detail panel 320px, toggleable]** — widths and collapse rules in `DESIGN.md.Layout & Spacing`.

**Sidebar** (shadcn `Sidebar`, collapsible to icon rail): traffic-light inset header with app name → primary views (Inbox `⌘1`, Archive `⌘2`, Approval Pane `⌘3` with amber count badge, Bridges `⌘4` with health dot roll-up) → SPACES group (per-Space rows, view-and-filter only, FR-23) → NETWORKS group (filter chips per connected Network with `{components.bridge-health-dot}`) → footer: Account switcher (per-Account avatar + hue dot + sync state) and global sync/offline status.

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
| Settings | `⌘,` | Accounts, Privacy (incognito defaults, undo-send window), Notifications, Archive & Storage, Shortcuts, Appearance, About/Egress |
| Command Palette | `⌘K` | Fuzzy-find Chats, contacts, and actions; `>` prefix for action mode (FR-48) |
| Hotkey cheat sheet | `⌘?` | Overlay reference of all shortcuts (FR-49) |

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
| Risk tier copy verbatim from the tier table (PRD addendum §2) | Softening ("totally safe") or scare-mongering ("you WILL be banned") |
| Sentence case everywhere; no exclamation marks; "Chat", "Account", "Bridge", "Network" capitalized per Glossary | Title Case Buttons, emoji in system copy, "please" in errors |

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

**Meta**
- `⌘?` cheat sheet · `⌘W` close window (sync continues) · `⌘Q` quit (sync stops)

**Pointer:** everything reachable by click and context menu; drag only for Pin reorder and media drop. **Banned everywhere:** hover-only affordances without a keyboard/focus equivalent, dismissible-only error states, modal stacks deeper than one, infinite spinner states without copy, drag as the sole path to any action.

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

## Accessibility Floor

Behavioral floor per NFR-14 ("operable and labeled"); visual contrast lives in `DESIGN.md` (AA in both themes, validated against the solid sidebar fallback).

- **Keyboard-only:** every MVP flow — wizard, bridge QR login, approval, export, search, settings — completes with keyboard alone; the shortcut set above is a superset of FR-48–50. All shortcuts also exist as native menu-bar items so macOS full-keyboard-access and VoiceOver users get standard discovery.
- **VoiceOver:** every interactive control carries a label including dynamic state: "WhatsApp bridge, disconnected, button", "Chat, Marta Kowalska, Telegram, work account, 3 unread, draft pending". Timeline messages announce sender, time, content/state ("message failed to send, retry available"). The undo-send pill announces its countdown once, not per second.
- **Live regions:** palette and search results announce via `aria-live="polite"`; new messages in the open Chat announce politely; bridge health changes announce assertively (they are the loss-risk case).
- **Focus:** visible `ring` on every focusable; roving tabindex in chat list, timeline, Approval Pane; focus returns to the invoking element when overlays close; `Esc` semantics are universal (see Interaction Primitives).
- **Landmarks:** sidebar = navigation, chat list = list with posinset/setsize, conversation = main, detail panel = complementary — VoiceOver rotor jumps between panes.
- **Reduced motion:** countdown ring becomes numeric text; pane transitions become cuts; the bridge-dot pulse becomes a static state change.
- **Targets & text:** rows are full-width targets; icon-only controls ≥ 24px with tooltips; all text respects macOS text-size scaling since layout is token-based, not pixel-locked imagery.

## Responsive & Platform

macOS desktop only (MVP). Window behavior:

| Window width | Behavior |
|---|---|
| ≥ 1280px | Four panes possible: sidebar + chat list + conversation + pinned detail panel |
| 1080–1279px | Detail panel becomes a `Sheet` over the conversation (`⌘I` toggles it) |
| 940–1079px | Sidebar auto-collapses to the 48px icon rail (tooltips carry labels); chat list stays |
| Minimum 940 × 600 | Enforced; below-minimum resize is blocked, not squished |

Platform integration: overlay titlebar with traffic-light insets per `DESIGN.md` (draggable header regions across all three panes); native macOS menu bar mirrors every command; dock badge shows unread count (Settings: all unreads / mentions only / off); optional menu-bar extra + launch-at-login keep sync alive windowless (FR-53); light/dark follow the system by default with manual override (Settings → Appearance); full-screen and Stage Manager behave as standard document windows; notification click-through restores or summons the window into the right Chat (FR-54). No touch/mobile layer in MVP — pointer + keyboard only.

## Inspiration & Anti-patterns

- **Lifted from Beeper Desktop (the benchmark):** the three-pane unified inbox with a Spaces/filters rail; Favorites vs. Pins as distinct tiers; inbox-zero archive flow with auto-return; `⌘K` as the everything-surface; incognito with manual read release. keeper's bet is Beeper's shell with ownership underneath.
- **Lifted from Superhuman/Linear:** single-key list verbs (`e`, `u`, `p`, `f`, `m`), palette-first parity for every action, the cheat sheet generated from the action registry.
- **Lifted from Element X:** verification and key-backup UX vocabulary (emoji/SAS, recovery key) — keeper does not invent novel crypto UX, it renders the SDK's flows natively. Patterns only; AGPL code is study-only.
- **Rejected — per-network tabs/workspaces (Ferdium model):** one inbox is the product; Network identity is a badge, never a silo.
- **Rejected — toast-only error surfaces:** anything that risks message loss (bridge death, failed send) is persistent until resolved. This is a direct answer to Beeper's top complaint (silent bridge disconnects).
- **Rejected — cloud-assisted conveniences:** no "delivers even when off" promises anywhere; deferred features (scheduled send) will say "app must be running" in the same breath as the feature name.
- **Rejected — gamification, celebration animations, streaks:** archival calm; inbox zero is its own reward.
- **Rejected — hiding the Bridge Bot:** the raw bot Chat stays reachable behind every native flow; keeper wraps, never walls.

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

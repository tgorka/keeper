# Epic 8 Context: Incognito & Undo-Send — Privacy on the User's Terms

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic delivers keeper's privacy wedge — the features Beeper paywalls, shipped free. It gives the user complete, informed control over what leaves their machine: read receipts go private (`m.read.private`) with deterministic scope resolution, typing and presence are suppressed, receipts release only on explicit demand, and coupled-network side effects are disclosed at toggle time. It also adds the Undo-Send Window, holding every approved message locally for a user-controlled delay before dispatch so a regretted send can be pulled back with zero network activity — and, once a message has actually left, an honest post-dispatch delete that falls back to Matrix Redaction with best-effort framing on bridged networks. Together these turn "read without pressure" and "un-embarrass yourself before it sends" into first-class, trustworthy behaviors.

## Stories

- Story 8.1: Incognito Read Receipts with Scoped Policy
- Story 8.2: Typing/Presence Suppression, Coupling Caveats, and Manual Release
- Story 8.3: Undo-Send Window
- Story 8.4: Post-Dispatch Delete for Everyone

## Requirements & Constraints

- Incognito Mode is toggleable at three scopes — global, per-Account, per-Chat — with deterministic precedence: Chat overrides Account overrides Global. Effective scope must be visible in the Chat header, and all eight scope combinations must be covered by unit tests.
- While Incognito applies to a Chat, reading emits `m.read.private` instead of `m.read`: the remote party's client keeps showing the message unread, while the user's own read position still syncs across their own devices. Typing emits zero typing events (verifiable at the homeserver) and presence is withheld where the protocol allows.
- Networks that couple behaviors (e.g. WhatsApp couples sending read receipts with seeing others') must surface a coupling caveat inline at toggle time, drawn from the same versioned data file as Bridge risk tiers (produced in Epic 6, Story 6.1).
- Manual read release: an explicit user action emits exactly one public `m.read` at the current read position on demand. Without it, only private receipts are ever sent while Incognito applies. No separate typing-only toggle exists in MVP — typing suppression is bundled with Incognito.
- Undo-Send Window default is 10 s, configurable 0–60 s in Settings; 0 disables holding entirely. It runs at approval time. During the window the user can cancel, which produces zero network dispatch and returns the full message text to that Chat's composer as a Draft.
- Crash/offline safety: after a crash or while offline, held rows whose window has elapsed dispatch on startup/reconnect; rows still within their window resume their countdown. Offline-queued messages that outlived their window dispatch on reconnect. No held message may be silently lost.
- Post-dispatch delete (window elapsed or window = 0) issues a Matrix Redaction; on bridged Chats the confirmation must name the Network and state that removal there is best-effort. A message still inside its undo window resolves the same user intent as an undo (no network event exists to redact yet), not a Redaction.
- Local Archive interplay: a user's own post-dispatch deletion is treated per the archive's mark-never-erase semantics — priors are kept unless the user's "honor remote deletions locally" setting is on.

## Technical Decisions

- **`signals` is the sole outbound-signal emitter.** All read receipts, typing notices, and presence emission go exclusively through `keeper-core::signals`. No other module and no IPC command may call the SDK's receipt/typing/presence APIs — enforce this with a convention test or lint. This module was seeded in Epic 3 (Story 3.9); this epic adds the Incognito policy logic on top of it.
- **Effective-policy resolution happens at emission time**, inside `signals`, resolving Chat > Account > Global. Receipts become `m.read.private`, typing is dropped, presence withheld where applicable. Manual release is an explicit `signals::release_receipt(room)` call emitting a public `m.read`.
- **Undo-Send uses the `outbox` table ahead of the SDK `SendQueue`.** Approval inserts a row with `dispatch_at = approval_time + window`. A scheduler moves elapsed rows into that Account's `SendQueue`; cancel deletes the row and restores the Draft. Held messages are projected into the timeline VM as a distinct `held` state carrying the countdown.
- **The single dispatch gate still holds:** the only path into `SendQueue` is `send::submit(text|draft, trigger)` with `trigger ∈ {ComposerSend, ApprovalPaneApprove}` (established in Epic 1, reinforced by Epic 7). Post-dispatch delete is a Matrix Redaction, not a send.
- **Storage:** `outbox` and settings live in `keeper.db`; all SQLite runs in WAL mode for crash safety. Per-Network coupling caveats ship in the same versioned JSON data structure as risk tiers.
- **Per-account supervision:** the send scheduler and `signals` run under each `AccountHandle`'s supervision task; no global mutable state.
- **State/VM conventions** follow the spine: camelCase serde, ms-epoch timestamps, `Vm`-suffixed view models streamed snapshot-then-diff into the matching zustand mirror store (send/outbox domain, settings domain).

## UX & Interaction Patterns

- **Incognito violet** (`{colors.incognito}`) is reserved exclusively for outbound-signal suppression and signals nothing else. **Held amber** (`{colors.accent}`) marks the airlock between written and sent — used on the Undo-Send countdown pill — and never decorates anything else.
- **Incognito chip** in the Chat header always shows the *effective* scope ("Incognito — this chat overrides account" / "— account" / "— global"). While Incognito applies, the composer's focus ring tints violet.
- **Incognito controls** reach three scopes: global (Settings / palette), per-Account (Account menu), per-Chat (header chip / ⌘⇧I). The chip also carries the "Mark read publicly" action for manual release. Coupling caveats surface inline at the toggle.
- **Undo-send pill** floats above the composer on every approved send when window > 0: radial countdown ring plus "Sending in Ns — Undo". Click or ⌘⇧Z cancels. Multiple pending sends stack oldest-first. Under reduced motion, show a numeric countdown with no ring animation; the pill announces its countdown to VoiceOver once, not per second (⌘Z remains text-undo).
- **Message send captions** cycle Held (amber, during undo window) → Sending… → Sent.
- **Voice & tone:** sentence case, no exclamation marks, honest consequence-naming. The bridged-delete confirmation reads plainly, e.g. "Deletes your copy on this Mac. Other people's copies are unaffected. Removal on {Network} is best-effort." Best-effort framing and "honor remote deletions locally" disclosure are persistent, never toast-only.
- Settings → Privacy houses Incognito defaults and the Undo-Send window control.

## Cross-Story Dependencies

- **Story 8.1** depends on Epic 3 (Story 3.9 `signals` module seed) and Epic 7 (settings-surface conventions); 8.2 builds on 8.1.
- **Story 8.2** consumes the per-Network coupling data file produced in Epic 6, Story 6.1 (shared with risk tiers).
- **Story 8.3** depends on Epic 7 existing (both dispatch triggers — ComposerSend and ApprovalPaneApprove — must already exist so held sends can originate from either the composer or the Approval Pane).
- **Story 8.4** depends on 8.3, which lets it distinguish a held-cancel (undo, no network event) from a true post-dispatch delete (Redaction). It also interoperates with the Local Archive durability semantics from Epic 5 (FR-36).
- Actions defined here (Toggle Incognito, and related) are later registered into the Command Palette action registry in Epic 9.

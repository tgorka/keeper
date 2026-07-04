# Epic 1 Context: Walking Skeleton — Sign In and Chat on Matrix

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic proves the entire vertical slice of keeper on its final architecture before any feature breadth is built: the `keeper-core`/`keeper` crate split, typed IPC with generated bindings, password login gated on Simplified Sliding Sync, a streaming room list, a live timeline, and text send/receive with honest visible states. It exists to de-risk the hardest structural bets up front — that a tauri-free Rust hexagon can drive a pure React renderer through snapshot-then-diff channels — so that every later story lands on the real seam instead of being refactored onto it. The exit gate: every Story 1.1–1.8 acceptance criterion passing in a `tauri build` release build against a real SSS-capable homeserver (Synapse ≥ 1.114).

## Stories

- Story 1.1: Cargo Workspace Split and Typed IPC Foundation
- Story 1.2: App Shell — Three-Pane Frame and keeper Theme
- Story 1.3: Password Login with Sliding-Sync Verification
- Story 1.4: Sliding-Sync Room List
- Story 1.5: Timeline View — Receive Text
- Story 1.6: Send Text with Local Echo and Visible Send States
- Story 1.7: Offline Resilience — Queued Sends and Reconnect Convergence
- Story 1.8: Session Restore and Sign-Out

## Requirements & Constraints

- All E2EE key material, message plaintext, tokens, and protocol state stay exclusively in the Rust core; the webview holds only rendered view models — no crypto, no message DB, and no token in any JavaScript-reachable storage. This is verifiable at code-review time.
- Login must refuse a homeserver that cannot do Simplified Sliding Sync *before* any account state is created, with a named, actionable error (bad credentials vs. unreachable vs. unsupported login type / non-SSS), and must resolve a bare domain via well-known discovery.
- Every outgoing message must reach a terminal, user-visible state (sent, or failed-with-retry that never disappears on its own); every incoming synced event must land in local state. No log-only failures — every failure that changes user-visible state maps to a rendered state.
- Cold start (launch → interactive, cached chats rendered, input accepted) targets < 2 s; switching to a previously-synced chat renders the cached timeline in < 150 ms without a network round-trip; composer input stays under one frame.
- Crash / force-quit at any moment must not corrupt stores and must lose zero previously-persisted state; recovery on next launch is automatic. All SQLite runs in WAL mode.
- Licensing firewall: Apache-2.0 only, no GPL/AGPL code or crates, enforced by `cargo deny` as a required check.
- Baseline accessibility: every focusable control shows a visible focus ring and is keyboard-operable; interactive controls carry accessibility labels; text meets WCAG 2.1 AA contrast in both light and dark themes.
- Quality gates that must pass before a story is done: `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check`.

## Technical Decisions

- **Crate split (AD-6):** `src-tauri/` is a cargo workspace with `crates/keeper-core` (the hexagon — owns all Matrix state, crypto, persistence, business rules) and `crates/keeper` (the Tauri shell — IPC/plugin/protocol glue only, no business logic). `keeper-core` must carry no `tauri` dependency anywhere in its tree; platform needs (dirs, keychain, notifier, sidecar) are injected through a `Platform` port trait. New Rust code defaults into `keeper-core`.
- **Generated types (AD-7):** every type crossing IPC lives in `keeper-core::vm`, derives `serde` + `ts_rs::TS` with `#[ts(export)]` and camelCase rename-all, and is emitted to `src/lib/ipc/gen/` by a cargo test export step. A CI-runnable check fails if committed bindings differ from generated output. Hand-written TS in `src/lib/ipc/` is limited to thin typed `invoke`/`Channel` wrappers.
- **IPC contract (AD-8):** commands are `domain_verb` snake_case; fallible ones return `Result<T, IpcError>` where the envelope is `{ code (stable enum), message, accountId?, retriable }`. Subscriptions are commands that accept a `Channel<Batch>` and return a subscription id; **every stream opens with a full snapshot/reset batch, then diffs**, so (re)subscribing at any time is safe and never duplicates. Events use `keeper://kebab-case` names carrying only ids + small payloads. Decrypted media never travels over IPC JSON.
- **Frontend state (AD-9):** zustand 5 vanilla stores created outside React, one per stream domain (accounts, inbox, per-open-room timeline, send/outbox). Channel handlers apply diff batches imperatively; components subscribe via selectors. Stores hold only what Rust streamed plus ephemeral UI state. No TanStack Query / component reducers for server-originated state; the UI never re-derives truth.
- **Sync (AD-2):** `SyncService`/`RoomListService` over MSC4186 (Simplified Sliding Sync) is the only sync mechanism; an SSS capability probe gates account creation.
- **Per-account isolation (AD-3, AD-10):** each Account = one `matrix_sdk::Client` with store dir `accounts/<account_id>/sdk/`. `keeper.db` holds the account/settings registries; secrets (access/refresh tokens) live only in the macOS Keychain via `keyring`, service `dev.tgorka.keeper`. Logout deletes exactly the SDK dir and that account's Keychain entries — nothing else — and stops its supervision tasks. `account_id` is a keeper-generated opaque ULID used in paths, rows, VMs, and Keychain entries; Matrix `room_id`/`event_id` pass through verbatim as opaque strings.
- **Concurrency (AD-19):** an `AccountManager` owns a registry of `AccountHandle`s, each supervising its account's tokio tasks (Client + SyncService, send scheduler, etc.). No global mutable singletons; the only globally reachable handle is the Tauri-managed `AppState`. Closing a chat tears down its subscription without leaking the account's other streams.
- **Inbox projection (AD-20):** recency ordering and windowing are computed in Rust only; the UI receives a windowed `RoomListVm` (visible range + buffer, with totals) and the TS store applies diffs and never re-sorts.
- **Errors & observability (AD-21):** per-module `thiserror` enums roll up to a `CoreError` root, mapped to the `IpcError` envelope exactly once in the shell's command layer. `tracing` everywhere (no `println!`), per-account spans; message plaintext, tokens, and recovery keys never appear in logs.
- **Send gate (AD-13):** `send::submit(text|draft, trigger)` with `trigger ∈ {ComposerSend, ApprovalPaneApprove}` is established as the *only* function that feeds the SDK `SendQueue`; a Rust test asserts it is the sole public dispatch entry point in `keeper-core::send`. Text messages dispatch through this gate and appear immediately as local echo. (The undo-send outbox itself is a later epic; this epic seeds the single-gate invariant.)
- **Timestamps** in VMs are integers (ms since Unix epoch UTC) — never ISO strings.
- **Stack anchors:** Tauri 2.11.x; matrix-sdk / matrix-sdk-ui / matrix-sdk-sqlite pinned at 0.18.0; ts-rs 12.x; React 19.1 / Vite 7 / Tailwind 4 / zustand 5.0.x; bun only (never npm/pnpm/yarn); cargo-nextest as the Rust test runner.

## UX & Interaction Patterns

- **Three-pane frame:** `[sidebar 260px | chat list 320px | conversation ≥ 480px]` plus a toggleable 320px detail-panel slot. Overlay/transparent titlebar; the sidebar header reserves the 78×12px traffic-light inset in every sidebar state so macOS window controls never overlap content. 1px borders between panes, no inter-pane shadows. Minimum window 940×600; sidebar auto-collapses to a 48px icon rail below 1080px width.
- **Brand tokens** (in `src/index.css`, on top of wholesale shadcn defaults; unlisted tokens inherit shadcn): keeper green primary (`#0F6E5C` / dark `#3ECFAE`) = "kept"; held amber accent (`#B45309` / dark `#F5A623`) = written-not-sent, used *only* for drafts/queued/approval/undo; incognito violet; the bridge-health trio (healthy/degraded/disconnected); and search-highlight tokens — all for both light and dark themes. Radii scale 5/7/10/14px (sm/md/lg/xl). macOS system font stack throughout; light/dark follows the system by default and dark mode is hand-picked, not an inversion.
- **Chat row (64px):** avatar, display name, last-message preview, timestamp; full-width click/Enter target. Bold in the chat list means unread and nothing else. (Unread badges and network overlays arrive in later epics.)
- **Message bubbles:** outgoing on primary, incoming on muted, 14px radius; consecutive same-sender messages group with a single avatar. Timeline text column capped at 720px and centered in wider panes.
- **Composer:** `Textarea` autogrows to 8 lines then scrolls; Enter sends, ⇧Enter inserts a newline.
- **Send-state captions** render in `caption` under the last bubble of a group and follow the microcopy voice — sentence case, no error codes, no emoji: "Sending…" → "Sent"; permanent failure shows a persistent destructive "Failed — Retry" that never auto-clears and whose Retry re-enters the same submit gate; offline-composed messages show amber "Queued — sends when you're back online" and dispatch automatically on reconnect.
- **Offline status:** a persistent sidebar-footer pill — "Offline — showing your local archive. Messages queue until you're back." — shown while disconnected, with no toast spam on connection flapping. Toasts are never the sole carrier of an error.

## Cross-Story Dependencies

- 1.1 (workspace + typed IPC) is the foundation for everything; 1.2 (shell) depends on 1.1.
- 1.3 (login) depends on 1.1 + 1.2; 1.4 (room list) depends on 1.3; 1.5 (timeline) depends on 1.4; 1.6 (send) depends on 1.5; 1.7 (offline) depends on 1.6.
- 1.8 (session restore + sign-out) depends on 1.3, 1.4, 1.5, and 1.6.
- Story 1.6 establishes the `send::submit` single-dispatch gate that the later undo-send / approval epic (FR-41) builds on. Story 1.4 seeds the Rust-side inbox windowing/ordering (AD-20) that the full Unified Inbox epic extends.
- External dependency: a real Synapse ≥ 1.114 (SSS-capable, password login enabled) is required to exercise 1.3–1.8 and the epic exit gate.

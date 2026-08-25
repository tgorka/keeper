# Deferred Work

### DW-1: The non-SSS error's "Learn more about Simplified Sliding Sync" link uses a plain `<a target="_blank" rel="noreferrer">`, whose open-in-system-browser behavior inside the Tauri webview is unverified.

origin: migrated from legacy ledger (spec-1-3-password-login-with-sliding-sync-verification.md), 2026-07-06
location: src/components (login error "Learn more about Simplified Sliding Sync" link)
reason: In a Tauri WKWebView, `target="_blank"` often does nothing or navigates the webview rather than opening the default browser; the app bundles `tauri_plugin_opener` but the link does not route through it. The component test only asserts the `href` attribute exists, not that activation opens the browser. Needs a manual `tauri dev` check and, if broken, wiring the link through the opener plugin.
status: done 2026-07-06
resolution: resolved by sweep bundle dw-login-error-open-in-browser

### DW-2: The `unsupportedLoginType` classification is unreliable — a homeserver with password login disabled typically returns `M_FORBIDDEN`, which `auth::map_login_error` maps to `InvalidCredentials`, so the user sees "Wrong username or password" instead of "password login not supported."

origin: migrated from legacy ledger (spec-1-3-password-login-with-sliding-sync-verification.md), 2026-07-06
location: keeper-core auth::map_login_error
reason: `matrix_auth().login_username()` does not pre-fetch the `/login` flow list, and Synapse returns `M_FORBIDDEN` for a password-login-disabled server (same errcode as a wrong password). The spec's error-kind mapping (`Forbidden`→`InvalidCredentials`, `Unrecognized`/`InvalidParam`→`UnsupportedLoginType`) therefore cannot reliably satisfy the I/O-matrix row for "unsupported login type." A robust fix is a pre-login `matrix_auth().login_types()` / supported-flows check to detect `m.login.password` before attempting login. Low user impact (uncommon scenario, both outcomes are non-retriable password-login failures); deferred rather than re-deriving the flow — the spec consciously chose error-kind mapping and did not mandate a flow pre-check.
status: done 2026-07-06
resolution: resolved by sweep bundle dw-login-unsupported-type-detection

### DW-3: An activated account's `Client`/`SyncService` runs for the process lifetime — `AccountManager::shutdown` (which now stops sync and aborts tasks) exists but is never called from any sign-out or account-change path, so sync is not torn down on sign-out.

origin: migrated from legacy ledger (spec-1-4-sliding-sync-room-list.md), 2026-07-06
location: keeper-core AccountManager::shutdown (never called from sign-out)
reason: Story 1.4 lazily activates the account on room-list subscribe and never deactivates it; `room_list_unsubscribe` only aborts the per-subscription producer, leaving `SyncService` running (network/battery/crypto) with no subscriber. This is out of 1.4's scope (its `Never` excludes sign-out) and is explicitly Story 1.8's acceptance criterion ("deletes exactly `accounts/<ulid>/sdk/` and Keychain entries, stops the account's supervision tasks, returns to login"). 1.8 must wire a sign-out command to `AccountManager::shutdown(account_id)`. In Epic 1's single-account slice with the room list always mounted while signed in, there is no trigger for the leak, so it is deferred rather than fixed here.
status: done 2026-07-06
resolution: already resolved: account.rs:4464 sign_out() now calls self.shutdown(account_id).await first (shutdown at :4356 stops sync via handle.sync.stop() and aborts all producer/subscription tasks); wired to the frontend via keeper/src/ipc.rs:3328 sign_out command. Story 1.8 wired sign-out->shutdown as promised.

### DW-4: The shared `subscribe()` IPC helper (`src/lib/ipc/client.ts`) arms `channel.onmessage` before awaiting `invoke`, but never clears it when `invoke` rejects, so every failed subscribe leaves a live `Channel` handler registered.

origin: migrated from legacy ledger (spec-1-5-timeline-view-receive-text.md), 2026-07-06
location: src/lib/ipc/client.ts
reason: On a rejected subscribe (e.g. `timelineUnavailable`/`syncUnavailable`), the promise rejects and the caller shows the inline error, but the `Channel` created inside `subscribe()` keeps its `onmessage` handler and its Tauri callback registration — nothing nulls it or drops the channel. Repeated retries of a failing room accumulate dangling handlers. Pre-existing: the helper predates this story (added for the room-list stream in Story 1.4) and is unchanged here; Story 1.5 only exercises the failure path more readily via `RoomNotFound`/`Build`. No functional bug on the happy path (no backend task is spawned on failure, so no stale batch is delivered), so deferred rather than patched in this story. Fix: in `subscribe()`, clear `channel.onmessage` (and drop the channel) in a `catch`/`finally` around `invoke`.
status: closed
resolution: wont-fix (coordinator, 2026-07-06). The prescribed `onmessage = null` fix is a
no-op — `@tauri-apps/api` Channel registers its callback in the constructor and frees it
only via the non-public `cleanupCallback()`; both sweep reviewers confirmed. The two real
fixes (undocumented internal API; backend unsubscribe Rust change) violate the conservative
dependency posture / bundle scope. Impact is a single dangling callback registration per
FAILED subscribe (error path only, no producer spawned, no stale data) — a bounded,
theoretical memory leak. Revisit only if a public Channel teardown API lands in Tauri.

### DW-5: The FR-41 single-dispatch-gate guard test (`keeper-core/src/send.rs`) is module-scoped and string-literal-based — it `include_str!`s only `send.rs` and scans for the exact substring `.send(content)`, so it cannot see a `Timeline::send` added in another file and is defeated by a variable rename, inlining, or line-splitting of the call.

origin: migrated from legacy ledger (spec-1-6-send-text-with-local-echo-and-visible-send-states.md), 2026-07-06
location: keeper-core/src/send.rs
reason: AD-13's stated invariant is crate-wide ("the only function in the whole crate that feeds the SDK send queue"), but the guard only reads its own file and matches one literal call form. A future `Timeline::send` introduced in `account.rs` (or anywhere outside `send.rs`) would pass the guard untouched, and a refactor renaming the local `content` binding or inlining the argument would break the scan into a false "one gate" pass or a spurious failure. It currently correctly enforces the present code, so no live defect — deferred rather than patched because a robust crate-wide, refactor-resilient enforcement (e.g. scanning all `keeper-core/src` sources, or a build-time architectural lint) is non-trivial and risks false positives, and the spec's AC scoped the assertion to "the send module." Fix: broaden the guard to scan every `keeper-core/src/*.rs` for `Timeline::send`/`send_queue().send` call sites and assert the sole one is inside `send::submit`, resilient to formatting.
status: open

### DW-6: The shell-wide connection/offline pill (`src/hooks/use-connection-status.ts`) is driven by `accounts[0]` only, so with ≥2 accounts it reflects an arbitrary (restore/add-order-dependent) single account's connectivity — account 2..N being offline is invisible, and which account "owns" the pill changes when `accounts[0]` is signed out.

origin: migrated from legacy ledger (spec-2-1-account-manager-unlimited-concurrent-accounts.md), 2026-07-06
location: src/hooks/use-connection-status.ts
reason: Story 2.1's `Never` boundary explicitly defers the per-account sync-state glyph to Story 2.5, so a single positional pill is a deliberate placeholder, not a regression — but it is genuinely wrong for a multi-account setup today. Story 2.5 ("Account Switcher and Per-Account State") builds the per-account sync-state glyph driven by each account's status stream, which is the correct home for this. Fix there: render connectivity per account in the switcher rather than one positional shell pill.
status: done 2026-07-06
resolution: already resolved: src/hooks/use-connection-status.ts deleted in Story 2.5 (commit 6a0fb04). The shell offline pill is now derived by src/lib/stores/account-status.ts:70 useShellOffline() which ranges over ALL signed-in accounts, not accounts[0].

### DW-7: Adding or signing out an account tears the entire merged inbox down and rebuilds it — `chat-list-pane.tsx` re-keys its subscribe effect on the sorted account-id set, so every account-set change calls `unsubscribe_inbox` (aborting all producers) then `subscribe_inbox` (reactivating already-live accounts' room lists), briefly clearing the list to the loading state.

origin: migrated from legacy ledger (spec-2-1-account-manager-unlimited-concurrent-accounts.md), 2026-07-06
location: src/components/.../chat-list-pane.tsx
reason: Functionally correct and within Story 2.1's design (the spec chose the existing bounded snapshot-then-diff streaming and deferred windowing/virtualization to Epic 4), but it causes a visible empty/loading flash on every account add/sign-out and redundant re-activation work that grows with N. Epic 4's unified-inbox organization is where incremental per-account add/remove (register/remove a single account against the live merger without a full re-subscribe) belongs. Fix there: on an account-set change, register/remove only the changed account with the live `InboxMerger` and spawn/abort only that account's producer, instead of rebuilding the whole inbox subscription.
status: open

### DW-8: Signing out an OIDC (MSC3861/MAS) account does local-only teardown and never calls the OAuth revocation endpoint, so the delegated access/refresh tokens remain valid at the Matrix Authentication Service until natural expiry.

origin: migrated from legacy ledger (spec-2-2-oidc-login-mas-msc3861.md), 2026-07-06
location: keeper-core AccountManager::sign_out / auth::sign_out_cleanup
reason: `AccountManager::sign_out` → `auth::sign_out_cleanup` deletes only the SDK store dir, the Keychain `StoredSession` blob, and the registry row (documented "local only, no server-side logout (AD-10, Story 1.8)"). AD-10 chose local-only logout for password sessions, where a lingering server session is benign. Delegated OAuth is materially different: MAS issues long-lived refresh tokens, so a signed-out device's session stays live server-side (visible in the user's MAS session list and usable if the refresh token leaks) until it expires. Not a defect in this story (sign-out is Story 1.8/2.1 scope and AD-10 is explicit), but introducing OAuth accounts makes IdP-side revocation newly relevant. Low user impact for the common case; deferred rather than expanding 2.2's scope. Fix: on sign-out of a `StoredSession::Oauth` account, best-effort call `client.oauth()`'s logout/revocation before local teardown (tolerating failure so offline sign-out still converges), and revisit whether AD-10's local-only policy should carve out delegated-auth accounts.
status: open
decision: 2026-07-06 Carve out OAuth revocation — On sign-out of a StoredSession::Oauth account, best-effort call client.oauth() logout/revocation before local teardown, tolerating failure so offline sign-out still converges. Leave AD-10's local-only path unchanged for password sessions.

### DW-9: `BeeperFlowRegistry::cancel_all` (invoked by the `cancel_beeper` command on `BeeperTab` unmount) clears every in-flight Beeper login flow, not just the caller's — so once concurrent Beeper adds are possible, cancelling/closing one add-account overlay would wipe another's stored request id and orphan its emailed code.

origin: migrated from legacy ledger (spec-2-3-beeper-email-code-login.md), 2026-07-06
location: keeper-core BeeperFlowRegistry::cancel_all / cancel_beeper command
reason: The registry is a single process-wide `Mutex<HashMap<email, request_id>>` and `cancel_beeper` calls `cancel_all()` (per this story's Code Map, which specified "cancel_beeper() … clears registry"). Today there is no trigger: the login/add-account surface is a single `LoginScreen` instance driving one Beeper flow at a time, so `cancel_all` and a per-email cancel are indistinguishable — by-design and correct for Story 2.3. It becomes a real defect only when Story 2.5 (Account Switcher and Per-Account State) enables managing/adding multiple accounts concurrently: user starts a Beeper flow for `alice@`, reaches the code step, opens a second add-account overlay for `bob@`, then cancels the `bob@` overlay → its unmount fires `cancel_beeper` → `cancel_all` wipes `alice@`'s request id, and `alice@` can no longer verify (`take` returns `BeeperUnavailable`) despite nothing being wrong. Mirrors OIDC's `cancel_all` breadth, but worse because the Beeper request id persists across two independent IPC calls rather than inside one guarded `authenticate` future. Fix in Story 2.5: give the registry a per-email `cancel(email)` and have each `BeeperTab` cancel only its own email; keep `cancel_all` only if a genuine cancel-everything caller exists.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `BeeperFlowRegistry::cancel_all` replaced by per-email `cancel(email)` (auth/beeper.rs); the `cancel_beeper` command + `cancelBeeper(email)` IPC now name the flow, and `BeeperTab` tracks the started flow's email in `flowEmailRef` so unmount cancels exactly its own flow. Tests: cancel drops only the named flow; unknown email is a no-op.

### DW-10: A Beeper account is identified only by its resolved homeserver host being `matrix.beeper.com` (`isBeeperAccount`), which couples the coverage-disclosure gating to Beeper's `.well-known` continuing to resolve to that host — a durable account-kind/provider tag would make the identity robust.

origin: migrated from legacy ledger (spec-2-4-beeper-coverage-disclosure.md), 2026-07-06
location: isBeeperAccount (frontend) / StoredSession (keeper-core)
reason: `AccountVm.homeserverUrl` is the SDK-resolved homeserver after `.well-known/matrix/client` discovery, and a Beeper login persists as a plain `StoredSession::Password` (no provider/kind field), so host match is the only available signal — the spec consciously chose it and forbade adding a provider/kind field in this story ("Never"). It works today (Beeper's well-known resolves back to `matrix.beeper.com`), so there is no live defect. It becomes wrong if Beeper ever redirects its well-known to a different host: every existing and new Beeper account would silently stop being recognized, the footer coverage control would vanish, and the pre-completion disclosure gate would stop firing — with no type/test protection. Fix: persist a durable account-kind tag at add time (extend `StoredSession`/`AccountVm` with a `provider`/`kind` discriminant set by `BeeperAuthProvider`) and key `isBeeperAccount` off that instead of the homeserver host; deferred because the fix touches Rust persistence + IPC shape, which this frontend-only story explicitly excluded.
status: done 2026-07-06
resolution: already resolved: src/lib/beeper.ts:13 isBeeperAccount returns account.provider === 'beeper' (no host match). Durable provider tag added end-to-end: vm.rs:1272 Provider enum + AccountVm.provider; registry.rs:369 ensure_provider_column + backfill_provider migration; auth.rs:649 resolves provider on restore. Story 2.5.

### DW-11: The first account is subscribed to its `connection_status` channel twice — once by the pre-existing `use-connection-status` hook (driving the shell offline pill from `accounts[0]`) and once by the new `use-account-statuses` hook (driving every account's switcher glyph) — a redundant subscription that should be consolidated.

origin: migrated from legacy ledger (spec-2-5-account-switcher-and-per-account-state.md), 2026-07-06
location: src/hooks/use-connection-status.ts + src/hooks/use-account-statuses.ts
reason: Story 2.5 deliberately left `src/hooks/use-connection-status.ts` untouched (it feeds the Story 1.7 shell offline pill / queued-send caption via the global `connectionStore`) and added a separate `src/hooks/use-account-statuses.ts` + `account-status` store for the per-account switcher glyphs, to keep the story's blast radius small. The backend multiplexes independent subscriptions per id, so the double subscription on `accounts[0]` is harmless (no correctness bug), but it is wasteful and slightly confusing. This also relates to the still-open 2.1 deferred item about the positional shell offline pill (`accounts[0]`-driven): the per-account glyph now exists, so the shell pill could be re-derived from the per-account `account-status` store (e.g. "offline if any/primary account offline") and the redundant hook removed. Fix: make `use-account-statuses` the single connection-status subscriber and derive both the switcher glyphs and the shell pill from `account-status`, retiring `use-connection-status`/`connectionStore` (or reducing `connectionStore` to a selector over the per-account map). Deferred because it touches Story 1.7's offline-pill code and tests, out of 2.5's scope. See [[DW-6]].
status: done 2026-07-06
resolution: already resolved: use-connection-status.ts deleted (commit 6a0fb04); the single connection-status subscriber is now use-account-statuses.ts:55 (account-status.ts header documents it as 'the SINGLE connection-status subscriber'). The redundant accounts[0] double-subscribe is gone.

### DW-12: A passphrase-encrypted account whose Keychain passphrase entry is lost (Keychain reset/corruption, manual removal, or a crash between `keychain_set` and the registry-row insert) restores with a generic matrix-sdk-sqlite open/decrypt failure and no honest, actionable "encryption key missing" state — it even boots into the shell as a normal account that silently never syncs.

origin: migrated from legacy ledger (spec-2-6-at-rest-encryption-first-run-choice.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (activate), auth.rs (find_restorable_accounts)
reason: Story 2.6's per-account passphrase is *self-describing* by design (AD-22 / this spec's Design Notes deliberately chose no registry schema change): `activate` (`src-tauri/crates/keeper-core/src/account.rs`) passes `Some(passphrase)` iff `store_passphrase/<id>` exists in the Keychain, else `None`. So *absence* of the entry is indistinguishable from "store was created unencrypted" — a posture-on store whose passphrase vanished is opened with `None`, matrix-sdk-sqlite fails to decrypt, and the error maps to a generic `AccountError::RestoreFailed` with no signal that the *passphrase* is the missing piece. Worse, `find_restorable_accounts` (`auth.rs`) is identity-only (checks the *session* entry, not the passphrase entry), so such an account still lists as restorable and boots into the shell; the decryption failure only surfaces later at lazy `activate` time and is swallowed by `use-session-restore.ts`'s fail-safe. No live defect today (the entry is only lost via crash/corruption/manual action), and losing the key inherently means losing the encrypted data — but the honest-surfacing ethos is not carried through. Fix (later, likely alongside Epic 3's UTD/verification honest states): persist a durable per-account encrypted-flag (registry column) so `activate` can detect "encrypted store, passphrase missing" and surface a distinct, honest, actionable state instead of a generic restore failure; and consider a crash-recovery sweep that GCs `store_passphrase/<id>` / `session/<id>` entries orphaned by an add that died before writing the registry row. Deferred because the honest state needs a Rust persistence/IPC/UI change (a registry schema addition and a new restore-failure surface) that this story's self-describing design consciously excluded.
status: open
decision: 2026-07-06 Add encrypted flag + honest state — Add a registry 'encrypted' column + a distinct EncryptionKeyMissing error variant and honest IPC/UI restore-failure surface; make find_restorable_accounts passphrase-aware; add a crash-recovery sweep GCing store_passphrase/session entries orphaned by an add that died before the registry-row insert. Likely alongside Epic 3 UTD/verification honest states.

### DW-13: `use-account-statuses` tears down and rebuilds every account's `connection_status` subscription whenever the signed-in account set changes (any add or sign-out), so a surviving account's status is transiently wiped to pending and its switcher glyph flashes the syncing spinner until its stream re-delivers a batch.

origin: migrated from legacy ledger (spec-2-5-account-switcher-and-per-account-state.md), 2026-07-06
location: src/hooks/use-account-statuses.ts
reason: The hook keys its effect on the sorted account-id set (`src/hooks/use-account-statuses.ts`) and, on any change, its cleanup unsubscribes and `removeAccount`s *all* accounts before the new run re-subscribes them — the same tear-all/rebuild-all pattern as the merged inbox in `chat-list-pane.tsx`, which is already deferred from Story 2.1 (adding/removing an account re-subscribes the whole inbox). It is not a correctness bug: statuses re-populate within one sync cycle and, after this story's `useShellOffline` fix (pending never counts as offline), the shell pill no longer flashes — only the per-account glyph briefly spins. The proper fix is delta-based subscription management (subscribe/teardown only the changed account against a persistent per-account subscription registry), the same shape as the deferred 2.1 inbox incremental-add/remove item, so both are best done together. Deferred because a persistent per-account subscription registry that survives account-set changes is a non-trivial refactor beyond Story 2.5's per-account-glyph scope. See [[DW-7]].
status: open

### DW-14: `subscribe_encryption_status` (like the `subscribe_connection_status` it mirrors) returns `Ok(subscription_id)` for an already-aborted producer when the account is removed in the spawn→register gap and this call did not activate it (`did_activate == false`), so the frontend holds a live subscription id whose producer never emits.

origin: migrated from legacy ledger (spec-3-1-encrypted-rooms-decrypt-encrypt-and-honest-utd-states.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (subscribe_encryption_status)
reason: In `account.rs`, the register branch aborts the orphaned task and only returns `Err(SyncStart)` when `did_activate` was true; the `did_activate == false` path falls through to `Ok(subscription_id)`. This is a faithful copy of the pre-existing connection-status lifecycle (Stories 1.4/2.5) and the race is extremely narrow (account signed out in the microseconds between `tokio::spawn` and lock-reacquire), with low consequence: the account is already gone, so the inert subscription is harmless and the account-set-keyed frontend hook reaps it on the next change. Not caused by this story — the same pattern ships in the mirrored connection-status path — so a robust fix (return an error, or a shared subscribe-register helper that reports the vanished-account case, applied consistently across both `subscribe_connection_status` and `subscribe_encryption_status`) belongs to a focused subscription-lifecycle cleanup, not Story 3.1. See [[DW-16]], [[DW-20]], [[DW-23]].
status: open

### DW-15: The verification store models a single active flow, so an incoming self-verification request for a second signed-in account arriving while a modal is open for the first account is silently dropped (never auto-opens or queues).

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src/hooks/use-verification.ts
reason: `use-verification.ts` only calls `openFor` when `!store.modalOpen`, and `setFlow` is gated on `activeAccountId === accountId`; the backend runs one producer per account (multi-account-capable) but the frontend seam discards concurrent-account progress. Story 3.2's spec deliberately scoped "exactly one active flow at a time" (I/O matrix + Design Notes), so this is a known MVP limitation, not a regression. Real once a user runs ≥2 accounts and a second account's peer starts verification during an open flow — the request expires unseen. Fix later with a per-account flow queue or a "pending verification" badge that surfaces the next request when the current modal closes.
status: open

### DW-16: `subscribe_verification`'s spawn→register account-removal race can return an error (or `Ok`) without tearing down an account it just activated — a faithful copy of the connection/encryption-status subscription pattern deferred in Stories 2.1/2.5/3.1.

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (subscribe_verification)
reason: In `AccountManager::subscribe_verification` (`account.rs`), `activate` inserts the handle under the first lock; a concurrent `remove_account` between spawn and the second lock hits the `None` branch, which aborts the task and clears the flow sender but does not remove/tear-down the just-activated `Client`+`SyncService`. This mirrors the exact shape flagged and deferred for `subscribe_connection_status`/`subscribe_encryption_status`; the race requires a concurrent account removal during a subscribe (not a normal path). Fix all subscribe lifecycles together (AD-21 teardown-on-early-failure) rather than diverging one copy. See [[DW-14]], [[DW-20]], [[DW-23]].
status: open

### DW-17: An incoming self-verification request that arrived before the producer's `add_event_handler` registered (e.g. the peer started verification during the sync gap at app start) is never surfaced — the handler only fires on future to-device events.

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: keeper-core verification::run_producer
reason: `verification::run_producer` registers a `ToDeviceKeyVerificationRequestEvent` handler and only forwards flow ids from events observed after registration; it does not poll `client.encryption()` for already-pending verification requests on startup. In practice the always-on `useVerification` subscriber (mounted in `app-shell`) is live for the whole session, so a request that arrives while keeper is running is caught; the gap is only a request landing in the narrow window before the handler attaches, and verification requests self-expire (~10 min). Fix later by enumerating existing pending requests when the producer starts and seeding them into the flow channel.
status: open
blocked: 2026-07-25 — NOT IMPLEMENTABLE as prescribed. matrix-sdk 0.18 `Encryption` exposes only flow-id-keyed `get_verification_request(user_id, flow_id)`; there is no API to enumerate pending verification requests, so the startup-enumeration fix cannot be written. Needs either an upstream API or a different design.### DW-18: Signing out the account that owns an open verification modal tears down its subscription but never resets the verification store, so the modal is stranded open on a now-removed account and a subsequent `close()` fires `verificationCancel` against a dead account.

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src/hooks/use-verification.ts
reason: `use-verification.ts` keys its effect on the sorted account-id set; when the active account signs out the cleanup unsubscribes that account, but `verificationStore` still holds `modalOpen: true` / `activeAccountId` / `flow` for the gone account (the store is never reset on account removal). The stranded modal renders the last streamed phase; dismissing it calls `verificationCancel(activeAccountId, …)` for an account that is no longer live (harmlessly swallowed by the store's `.catch`). Low consequence — the user just closes a stale modal and the cancel no-ops — and mid-flow sign-out of the very account being verified is an uncommon path. Fix later by resetting the verification store when its `activeAccountId` leaves the signed-in account set (e.g. reconcile in the `useVerification` effect on account-set change).
status: open

### DW-19: The QR reciprocal direction (peer scans keeper's displayed QR) is unverified and mislabeled — a `VerificationRequestState::Transitioned { QrV1 }` maps to the `Comparing` phase (rendering the emoji-compare screen's "waiting" copy with no emoji), the code assumes the QR side self-drives to `Done` without ever calling `qr.confirm()`, and if the request `changes()` stream ends after the QrV1 transition without a terminal `Done`/`Cancelled`, `emit_stream_ended` fires a spurious `Failed` on a possibly-successful verification.

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/verification.rs
reason: In `verification.rs`, `map_request_state` collapses all `Transitioned` variants to `VerificationPhase::Comparing`, and `drive_flow`'s `Verification::QrV1(_)` arm only re-emits request state (no `qr.confirm()`, no distinct QR-waiting phase, no terminality check inside the `Transitioned` arm — unlike the `_ =>` arm which checks `Done`/`Cancelled`). SAS is the guaranteed both-directions path and is fully wired; QR is display-only and the story's Verification section already lists the live "peer scans keeper's QR" round-trip as a manual second-session check because `Transitioned`/`QrV1`/`CancelInfo` states are not constructible in unit tests. Real risk: the reciprocal QR path could show a stuck "waiting" emoji screen and/or a spurious "Verification failed" even on success. Deferred rather than patched blind because the correct mapping and whether keeper must call `qr.confirm()` depend on matrix-sdk 0.18 live semantics that can only be validated against a real Element session, and a speculative change to a security-crypto state machine is riskier than the current honest-but-mislabeled waiting screen. Fix alongside the manual second-session verification: drive the reciprocated `QrVerification` explicitly, map the QR-in-progress state to a distinct waiting phase (e.g. `Confirmed`/"waiting for your other device"), and add the terminality check to the `Transitioned` arm so a QR `Done` is never surfaced as `Failed`.
status: open
decision: 2026-07-06 Leave honest waiting screen — Keep the current honest-but-mislabeled QR waiting screen until a live QR validation session is available.

### DW-20: Two overlapping `subscribe_verification` calls for the same account (e.g. React StrictMode double-mount, or a rapid account-set change) clobber the account's single `verification_flow_tx` slot, and a later `unsubscribe_verification` of the first subscription unconditionally nils the slot — leaving the second producer live but unreachable, so keeper-started verification silently fails with "no active verification subscription".

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (subscribe_verification/unsubscribe_verification)
reason: `subscribe_verification` (`account.rs`) does `*flow_tx_slot.lock().await = Some(flow_tx)` on every call, overwriting any prior sender, and `unsubscribe_verification` does `*handle.verification_flow_tx.lock().await = None` unconditionally before aborting its own `subscription_id`. Sequence: subscribe#1 (flow_tx=A) → subscribe#2 (flow_tx=B, producer#1 still running with its to-device handler) → unsubscribe#1 nils flow_tx → producer#2 alive but `verification_start` finds `None` and errors. Two live producers also each register a `ToDeviceKeyVerificationRequestEvent` handler transiently. Primarily a dev-StrictMode / rapid-resubscribe concern (production mounts the always-on subscriber once); it is a verification-specific twist on the subscription-lifecycle races already deferred for connection/encryption-status (Stories 2.1/2.5/3.1), but those don't carry a per-account sender slot. Not a trivial one-line patch — needs a generation token or ref-count so a stale unsubscribe can't nil a newer producer's sender, ideally folded into the shared subscription-lifecycle cleanup. Fix: tie the `flow_tx` slot to its `subscription_id` (only clear it if the slot still belongs to the unsubscribing id), and/or abort a prior producer when re-subscribing the same account. See [[DW-14]], [[DW-16]].
status: done 2026-07-26
resolution: resolved by sweep wave 1. New `verification::FlowSlot` (verification.rs) tags the installed flow sender with the installing subscription id; `subscribe_verification` installs tagged, and `unsubscribe_verification`/the natural-completion reaper/the spawn-gap abort all `release(subscription_id)` — a release only clears the sender its own subscription installed, so an overlapping resubscribe's surviving producer keeps its inlet and keeper-started verification no longer strands.

### DW-21: An incoming request's auto-open/auto-accept seam gates only on `!store.modalOpen`, so if the producer emits a second `requested` snapshot (e.g. `Created` then `Requested`, both mapped to `requested`) in the instant after the user pressed Esc to dismiss the just-opened modal, the second batch re-passes the gate, re-opens the modal the user dismissed, and fires a second `verificationAccept` on a cancelling flow.

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src/hooks/use-verification.ts
reason: `use-verification.ts`'s `onBatch` calls `openFor(accountId)` + `verificationAccept(...)` whenever `flow.phase === "requested" && !store.modalOpen`; `close()` sets `modalOpen:false`, so a `requested` batch arriving between the Esc and the flow's terminal `Cancelled` snapshot re-triggers the incoming path. Very narrow timing race (requires two pre-`Ready` `requested` emissions straddling a sub-frame Esc), and self-limiting — the accept on an already-cancelling request errors and is caught, and the flow terminates shortly after — but the user can see a dismissed modal briefly resurrect. Deferred (not a clean trivial patch): the gate should also track the set of flow ids already auto-opened/accepted (or refuse to re-open once a terminal/cancel was observed for that flow id), which adds per-flow-id state to the hook. Fix: record accepted/seen `flowId`s and skip re-open/re-accept for a flow id already handled.
status: open

### DW-22: In the emoji-compare phase, clicking "They match" (which fires `verificationConfirm`) and then immediately pressing Esc cancels the verification the user just approved — `close()` still sees the streamed phase as `comparing` (the `confirmed` snapshot hasn't round-tripped yet) and `shouldCancelOnClose("comparing")` is true, so it fires `verificationCancel` racing the in-flight confirm.

origin: migrated from legacy ledger (spec-3-2-device-verification-emoji-sas-and-qr.md), 2026-07-06
location: src/lib/stores/verification.ts
reason: `verification.ts` store `close()` best-effort-cancels any non-terminal, non-`confirmed` flow; the `confirmed` exclusion only protects the flow once Rust streams back the `Confirmed` snapshot. Between the "They match" click and that snapshot arriving, the store still holds `comparing`, so an immediate dismiss cancels an approved verification. Requires the user to click match then Esc within the round-trip window — uncommon but plausible for an impatient user closing right after confirming. Deferred rather than patched because a correct fix touches the pure-renderer invariant: the cleanest option is an optimistic "confirmation sent" flag set when "They match" is clicked so `close()` won't cancel, but naively optimistically flipping the store phase to `confirmed` would strand the modal if `verificationConfirm` itself rejects (its `.catch` is currently a no-op). Fix: track a local "confirm/mismatch dispatched" flag and have `close()` consult it (surfacing an honest `failed` if the dispatched action rejects), rather than relying solely on the streamed phase.
status: open

### DW-23: `subscribe_backup_status` shares the pre-existing spawn→register subscription-lifecycle race (2.1/2.5/3.1/3.2): if the account is removed in the gap between spawning the producer task and registering it, a `did_activate == false` path aborts the producer but still returns `Ok(subscription_id)`, leaving a phantom subscription whose Settings backup row stays "Checking…" forever; a rapid re-activation in that gap can also register a producer holding a stale `Client`.

origin: migrated from legacy ledger (spec-3-3-key-backup-enable-and-restore.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs:826-844 (subscribe_backup_status)
reason: `src-tauri/crates/keeper-core/src/account.rs:826-844` — the `None`-after-spawn branch only returns an error when `did_activate` is true; when the account was already live and is removed in the gap, it aborts the task and falls through to `Ok(subscription_id)`. This is the identical pattern already deferred for the encryption-status/verification/connection/timeline subscribes (all mirror the same `did_activate`/gap logic); it is architectural, not introduced by this story, and requires a sign-out landing inside a sub-millisecond window. Consequence is low (the frontend is tearing the account down anyway) and a robust fix needs a shared subscription-generation/teardown redesign across all subscribe methods rather than a backup-local patch. See [[DW-14]], [[DW-16]], [[DW-20]].
status: open

### DW-24: The FR-41 single-content-gate source-scan guard in `send.rs` only inspects `send.rs` itself, so it proves intra-file placement of the sole `.send(content)`/`.send_reply(`/`.edit(` call sites but does NOT enforce the stated crate-wide invariant ("no `Timeline::send`/`send_reply`/`edit` call sites anywhere else in the crate") — a future dispatch added in `account.rs`/`timeline.rs`/elsewhere would bypass the gate with the guard still green.

origin: migrated from legacy ledger (spec-3-4-replies-and-edits.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/send.rs
reason: `src-tauri/crates/keeper-core/src/send.rs` `submit_is_the_sole_send_dispatch_gate` does `include_str!("send.rs")` and `match_indices` over that one file only. This is a pre-existing limitation of the guard mechanism (the original `.send(content)` guard was already single-file); Story 3.4 faithfully extended the same intra-file pattern for reply/edit. The invariant still holds today by convention (all dispatch is in `send.rs`). A robust fix needs a crate-wide scan (build-script/`walkdir` over `src/`, or a clippy-style lint) rather than `include_str!` of one file, so it was not a trivial in-diff patch. See [[DW-5]].
status: open

### DW-25: The composer surfaces the generic retry-implying error copy ("Couldn't send. Check your connection and try again.") for *non-retriable* reply/edit failures (`TargetNotFound`/`NotEditable`, `retriable: false`), so a user who edits a message that is no longer editable/present is told to check their connection and retry — which won't help — undercutting the honesty the non-retriable `IpcErrorCode` distinction was added for.

origin: migrated from legacy ledger (spec-3-4-replies-and-edits.md), 2026-07-06
location: src/components/chat/composer.tsx
reason: `src/components/chat/composer.tsx` catches the rejected send into a boolean `error` and renders a fixed message, ignoring the `IpcError.retriable` flag the Rust `to_ipc_error` sets (`SendError::TargetNotFound | NotEditable → (SendFailed, false)`). Low frequency (own text messages are normally editable; a target vanishing mid-edit is rare) and partially pre-existing (the same generic copy predates 3.4 for `sendText`). A correct fix threads the `IpcError` (code/retriable) into the composer to show a distinct, non-retry message — a slightly larger change than an in-diff patch, worth a focused pass across the composer's error handling.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `composer.tsx` now reads the rejected `IpcError.retriable` flag: `retriable: false` (TargetNotFound/NotEditable) renders terminal copy ("That message can no longer be edited or replied to.") instead of the connection-retry copy; unrecognisable rejections stay retriable (the weaker, safer claim).

### DW-26: The timeline producer's `event_id → unique_id` `ReplyIndex` is never pruned on `Remove`/`PopFront`/`PopBack`/`Truncate`, so a reply whose original was removed from the loaded timeline still resolves `in_reply_to_key: Some(<stale key>)` and renders a clickable quote that silently no-ops on click, instead of the spec's honest "original not loaded → not clickable".

origin: migrated from legacy ledger (spec-3-4-replies-and-edits.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/timeline.rs (map_diff_indexing)
reason: `src-tauri/crates/keeper-core/src/timeline.rs` `map_diff_indexing` deliberately leaves stale entries on removals (documented as "harmless"). No wrong-jump occurs (eyeball_im `unique_id`s are monotonic per-timeline and not reused, and the frontend `onJumpTo` `querySelector` no-ops when the row is absent), so the only consequence is a dead/misleading clickable affordance for a removed original. Deferred rather than patched because correct pruning needs an index→event_id mirror (diffs carry positional indices, not event ids) — extra producer state beyond an in-diff fix; the case (a still-referenced reply whose original is Removed while loaded) is also rare.
status: open

### DW-27: The reaction dispatch error paths in `keeper-core` have no behavioral tests — `account::toggle_reaction` mapping an unparsable room id → `SendError::RoomNotFound` and `send::toggle_reaction` returning `SendError::TargetNotFound` on an unresolvable render key (the `items().find(...).and_then(as_event).map(identifier).ok_or(TargetNotFound)` chain) are only exercised implicitly; a regression in that resolution/mapping would ship green.

origin: migrated from legacy ledger (spec-3-5-reactions.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/send.rs + account.rs (toggle_reaction)
reason: `src-tauri/crates/keeper-core/src/send.rs` `toggle_reaction` and `src-tauri/crates/keeper-core/src/account.rs` `toggle_reaction` — the Rust tests added by Story 3.5 cover the pure `aggregate_reactions` helper and the FR-41 source-scan guard, but no test constructs the resolve/dispatch failure. This mirrors the pre-existing coverage shape of `submit_reply`/`submit_edit`/`retry` (also lacking behavioral error tests) because a `matrix_sdk_ui::Timeline` has no lightweight constructor for unit tests; closing it needs shared timeline test-harness infrastructure (a fixture room/timeline) rather than an in-diff patch, and the same fixture would retro-cover the sibling send methods.
status: open
blocked: 2026-07-25 — no test harness exists. keeper-core has NO [dev-dependencies] section at all (no wiremock, no matrix-sdk-test, no mock-sync), so the behavioural test this entry asks for has nothing to run against. Unblocking is a harness decision, not a bug fix.### DW-28: Received audio in an unsupported codec (e.g. Ogg/Opus voice notes, which WKWebView on macOS does not decode natively) renders empty `<audio controls>` forever — the element fires neither `onLoadedMetadata` nor `onError` on a codec-unsupported stall, so no retry/fallback surfaces and AC3 ("received audio plays back inline") silently fails for those clips.

origin: migrated from legacy ledger (spec-3-6-receive-media-thumbnails-protocol-streaming-preview.md), 2026-07-06
location: src/components/chat/media-attachment.tsx
reason: `src/components/chat/media-attachment.tsx` audio branch wires only `onError`/`onLoadedMetadata`; a webview stall (unsupported codec) is distinct from an error and never fires either. Deferred rather than patched because an honest fix needs stall detection (a load timeout heuristic) plus a fallback surface (download/open-externally), and Matrix voice notes are commonly Opus — a product decision on the fallback UX (and possibly transcoding, out of scope here) is warranted. Not caused by a code defect; it is a platform codec-support limitation surfaced by this story.
status: open
decision: 2026-07-06 Stall timeout + download fallback — Add a load-timeout stall heuristic and a fallback surface (download / open-externally button) when the audio element neither loads nor errors.

### DW-29: The `keeper-media://` protocol handler places no ceiling on media size: `client.media().get_media_content(..., true)` is atomic (whole file downloaded+decrypted into a `Vec<u8>` in RAM), and each Range request re-materializes the full buffer, so a malicious/oversized attachment (e.g. multi-GB video) can spike memory / amplify on repeated seeks with no 413/reject guard.

origin: migrated from legacy ledger (spec-3-6-receive-media-thumbnails-protocol-streaming-preview.md), 2026-07-06
location: src-tauri/crates/keeper/src/media_protocol.rs + keeper-core/src/media.rs
reason: `src-tauri/crates/keeper/src/media_protocol.rs` (`partial_or_full`) + `src-tauri/crates/keeper-core/src/media.rs` (`fetch_media`, sole `get_media_content` gate). The full-in-memory load is inherent to matrix-sdk 0.18's atomic media API (documented in the spec's Design Notes), so this is an architectural constraint surfaced by the story rather than a code defect. A max-size guard needs a product decision on the limit (must not break legitimate large media such as the epic's 25 MB video bar) — hence deferred, not patched. See [[DW-31]].
status: open
decision: 2026-07-06 Cap honoring 25 MB bar — Enforce a max-size guard at or above the epic's 25 MB video bar, rejecting oversized media (413) before the whole-file load, coordinated with the send-side cap in DW-31.
note: 2026-07-11 (Story 12.4) The Range seek-amplification leg is now bounded — iOS caps each Range slice at MAX_MEDIA_RANGE_CHUNK (8 MiB) in media_protocol.rs, so repeated seeks no longer re-materialize the full buffer. The base whole-file in-RAM load remains; disk-backed streaming (avoiding it) stays deferred here per Epic 12.

### DW-30: The `keeper-media://` handle is unauthenticated/forgeable — its three path segments (`account_id`, `room_id`, item `unique_id`) are all data the webview already holds, so a compromised webview (e.g. via a future XSS foothold) could name coordinates to fetch decrypted bytes for any currently-open room on the same account; the handle is not HMAC-signed and the trust boundary is undocumented.

origin: migrated from legacy ledger (spec-3-6-receive-media-thumbnails-protocol-streaming-preview.md), 2026-07-06
location: src-tauri/crates/keeper/src/media_protocol.rs + keeper-core/src/account.rs (fetch_media)
reason: `src-tauri/crates/keeper/src/media_protocol.rs` parses the URL and `src-tauri/crates/keeper-core/src/account.rs` `fetch_media` resolves `(account_id, room_id, item_key)` against the open timeline with no capability check. The incremental risk over the existing "webview trusted as renderer" model is marginal (a compromised webview already sees rendered timelines and can call every IPC command), so this is defense-in-depth, not an active defect — but signing the handle (or at least documenting the trust boundary at the handler) is worth a focused hardening pass since the whole AD-4 design exists to keep bytes away from a potentially-hostile webview.
status: open

### DW-31: The media send path reads the whole file into memory (`tokio::fs::read`) / accepts the whole pasted body with no size ceiling before enqueuing, so a multi-GB or special file (e.g. /dev/zero) can spike RSS or OOM the client.

origin: migrated from legacy ledger (spec-3-7-send-media-and-files.md), 2026-07-06
location: keeper-core account.rs (send_attachment_path / send_attachment_bytes)
reason: `account.rs::send_attachment_path` does `tokio::fs::read(path)` with no cap and `send_attachment_bytes` takes an unbounded `Vec<u8>`; the spec explicitly deferred a size cap to avoid regressing the 25 MB video bar, and 3.6 already logged the symmetric receive-side unbounded-RAM concern. Needs a product-level upload ceiling honoring the homeserver `m.upload.size` limit with a friendly pre-send error. See [[DW-29]].
status: open
decision: 2026-07-06 Ceiling honoring m.upload.size — Enforce a pre-send upload ceiling honoring the homeserver m.upload.size limit with a friendly pre-send error, coordinated with the receive-side cap in DW-29.

### DW-32: Attaching media while a reply context is pending sends the attachment without any reply linkage — media-as-reply silently drops the reply relation.

origin: migrated from legacy ledger (spec-3-7-send-media-and-files.md), 2026-07-06
location: src/components/chat/composer.tsx (send)
reason: `composer.tsx::send` dispatches attachments via `onSendAttachments` regardless of `pending?.mode === "reply"`, and `AttachmentConfig` has an unused `reply` field; sending media as a reply is outside Story 3.7's ACs (3.4 covered text replies) but is a real product gap once both features coexist.
status: open

### DW-33: Timeline stub Alerts (`RedactedStub`, and the pre-existing `UtdStub`) use `role="status"` (an aria-live region), so a room open whose reset batch contains many historical redacted/undecryptable items announces "Message deleted"/the UTD copy repeatedly to screen readers.

origin: migrated from legacy ledger (spec-3-8-delete-for-everyone-redaction.md), 2026-07-06
location: src/components/.../redacted-stub.tsx + utd-stub.tsx
reason: `redacted-stub.tsx` and `utd-stub.tsx` both wrap an `Alert role="status"`; for static historical content in a `reset`/`append` batch this floods the live region. Cross-cutting (both stubs); a fix should drop the live role for non-transient stub rows while keeping honest inline text, and be applied consistently to UTD and redacted stubs.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `RedactedStub` and `UtdStub` now render `role="note"` (static) instead of the live `role="status"`, per the history-boundary precedent that only transient states are announced; tests assert neither status nor alert roles exist.

### DW-34: The new `mark_room_read` (transient-timeline receipt + marked-unread clear) and `mark_room_unread` (`set_unread_flag`) SDK paths have no Rust test coverage — only the pure `room_unread_state` helper is unit-tested; the actual receipt/flag round-trips are unverified.

origin: migrated from legacy ledger (spec-4-1-unread-management.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (mark_room_read / mark_room_unread)
reason: `src-tauri/crates/keeper-core/src/account.rs` `mark_room_read`/`mark_room_unread` call live matrix-sdk `Room`/`Timeline` APIs (`TimelineBuilder::build`, `mark_as_read`, `set_unread_flag`) that need a live homeserver or a mock-sync harness to exercise; the `keeper-core` crate currently has no mock Matrix server / integration-test scaffolding (all tests are pure or capture-sink based), so covering these paths is a test-infrastructure investment beyond this story. Not a code defect — the paths pass fmt/clippy and mirror the existing best-effort signals pattern (Story 3.9); flagged so the closed-timeline mark-read (whose receipt advance depends on the room event cache being populated for never-opened rooms) gets real-homeserver verification when a sync test harness exists.
status: open

### DW-35: A pinned room that is not currently present in any account's live SlidingSync room-list window is silently excluded from the Pins strip (and undercounts the window `total`) until it reappears.

origin: migrated from legacy ledger (spec-4-3-pins.md), 2026-07-06
location: keeper-core inbox.rs (InboxMerger)
reason: The `InboxMerger` only stamps/emits rooms returned by `merge(&state.accounts)`; `pin_order` is looked up per merged room, so a pin whose room is not in the synced window has no row to attach to. This is architectural — the same windowed-merge limitation applies to the Inbox and Archive windows — not unique to pins, and low impact in practice (SlidingSync covers the room list; pins are recently-interacted rooms). A proper fix (reconcile persisted pins against the merged set and surface a placeholder or force-sync the missing room) spans the whole windowed-merge design, so it is deferred rather than special-cased for pins.
status: open

### DW-36: `AccountManager::reorder_pins` performs N separate `registry::set_pin` calls (each its own connection + upsert) with no enclosing transaction, so a mid-loop failure or process kill can leave the persisted pin order half-rewritten (duplicate/gapped `sort_order`).

origin: migrated from legacy ledger (spec-4-3-pins.md), 2026-07-06
location: keeper-core account.rs (reorder_pins) / registry.rs (set_pin)
reason: `reorder_pins` (`account.rs`) loops `registry::set_pin(data_dir, …, index)`; `set_pin` opens its own `Connection` and commits independently (`registry.rs`). Low probability (a fast local SQLite loop) and self-healing on the next full reorder, and the `emit` tie-break now makes any transient duplicate order deterministic for display. A clean fix is a single multi-row upsert or an explicit transaction in one `registry::reorder_pins` call; deferred because it needs a new transactional registry API rather than the current per-call pattern.
status: done 2026-07-26
resolution: resolved by sweep wave 1. New `registry::reorder_pins` rewrites the whole 0..n sequence in ONE connection + `BEGIN IMMEDIATE` transaction (commit-or-rollback); `AccountManager::reorder_pins` routes through it. Test injects a mid-rewrite failure via trigger and asserts the previous order survives whole.

### DW-37: The Pins strip offers no keyboard-accessible way to reorder pins — reordering is native HTML5 pointer drag only.

origin: migrated from legacy ledger (spec-4-3-pins.md), 2026-07-06
location: src/components/.../pins-strip.tsx
reason: `pins-strip.tsx` avatars are `draggable` buttons reordered via `onDragStart`/`onDragOver`/`onDrop`; there is no arrow-key move or move-up/down menu affordance, so keyboard/assistive-tech users cannot reorder (Pin/Unpin themselves remain keyboard-operable via the context menu). The epic's accessibility floor mentions keyboard operability but the spec specified only drag reorder. Deferred as an a11y enhancement (e.g. a keyboard reorder affordance or the Epic 9 single-key verbs) rather than expanding this story's scope.
status: open
note: 2026-08-17 (story 52.7) — the entry stays OPEN, and the reason it stays open is
  now narrower and the premise it rested on was wrong. It said reordering is "native
  HTML5 pointer drag only"; on macOS it was neither. `pins-strip.tsx` never wrote
  anything to the drag data store, and WebKit — the engine keeper ships — fires no
  `drop` for a drag that set no data, so the pointer path this entry treated as the
  working one had also been dead in the real app since story 4.3, under the four green
  desktop-drag tests that could not see a store jsdom does not implement. 52.7 fixed the
  pointer path on both surfaces that had the hole: `pins-strip.tsx` `onDragStart`
  now names the pin (`text/plain`, `account:room`) and declares
  `effectAllowed = "move"`, `onDragOver` answers `dropEffect = "move"`, and `onDrop`
  calls `preventDefault` now that the drag carries text WebKit would otherwise act
  on itself; `notes/task-board.tsx` got the identical three lines. Both are asserted
  against a fake `DataTransfer` (`pins-strip.test.tsx`,
  `notes/task-board.test.tsx`, `sessions/session-board.test.tsx`), each mutation-
  proven to fail with the fix removed. What this entry actually asks for — a
  non-gesture reorder for pins — is still absent on desktop: story 13.6 added
  "Move up" / "Move down" to the pin context menu on the PHONE tier only
  (`pins-strip.tsx`, `phone &&`), so a desktop keyboard user still cannot reorder.
  The obvious close is to stop gating those two menu items on `phone` — the strip's
  context menu is already keyboard-reachable and the items are already written,
  guarded and tested — but that is a UX decision about the desktop menu's contents
  and belongs to whoever owns the strip, not to a story about a drag defect.
  52.7 does NOT extend this entry to the task board: the board ships a per-card
  column menu as its keyboard path, which 52.7 demoted to hover/`focus-within`
  visibility with `opacity-0` (not `hidden`), so it stays in the DOM, in the tab
  order and in the accessibility tree — asserted at both the widget and the session
  mount point.
note: 2026-08-17 (story 53.1) — the entry stays OPEN, and what it asks for is now the
  ONLY thing still missing: a non-gesture reorder for pins on the desktop. 52.7's note
  above is superseded in one respect — the `dataTransfer` lines it added were correct
  and still did not work, because the defect was below the page. Tauri installs a wry
  drag-drop handler whose closure always returns `true`
  (`tauri-runtime-wry-2.11.4/src/lib.rs:4862-4896`); wry implements
  `NSDraggingDestination` on the macOS WKWebView subclass itself
  (`wry-0.55.1/src/wkwebview/class/wry_web_view.rs:77-112`) and forwards
  `performDragOperation:` to `super` only when that closure answers `false`
  (`wry-0.55.1/src/wkwebview/drag_drop.rs:88-95`). So the drop is claimed in Rust
  before WebKit performs it and the page's `drop` cannot fire at all — for ANY HTML5
  drag in this app, on any surface, whatever it writes to the store. `dragDropEnabled`
  is per-window and config-time; turning it off would take Story 3.7's
  drop-an-OS-file-to-attach with it (`layout/conversation-pane.tsx:814-848`, the app's
  only `onDragDropEvent` consumer), so it was not touched.
  53.1 therefore replaced the gesture rather than the data it carried. HTML5 drag is
  GONE from both surfaces — no `draggable`, `onDragStart`, `onDragOver`, `onDrop` or
  `dataTransfer` remains in `layout/pins-strip.tsx` or `notes/task-board.tsx`, and each
  suite asserts the rendered DOM carries no `[draggable]` at all, because two
  mechanisms for one verb is how the dead one survived two epics. Both now press,
  move and release through one state machine, `hooks/use-pointer-drag.ts`
  (`pointerdown` → slop → `setPointerCapture` → `pointermove` → `pointerup`), which
  starts no OS drag session and so cannot be claimed by the native layer.
  What is now TRUE on which surface:
  - pins strip, desktop: reorder works by pointer for the first time since story 4.3.
    A press needs no hold, becomes a drag past 10 px, previews the reordered strip
    while carried (there is no HTML5 ghost left to lean on, so the preview IS the cue)
    and persists the full order on release. A press that does not travel is still the
    click that selects the room; the click after a drag is swallowed.
  - pins strip, phone: unchanged in behaviour. The long-press lift (story 13.6) was
    always pointer-only and always worked; it now enters the same hook, so the
    stale-index guards and the release arithmetic exist once instead of twice. A
    stationary lift still opens the pin's menu.
  - pins strip, keyboard: STILL NOTHING on the desktop, which is this entry's actual
    subject. "Move up" / "Move down" remain gated on `phone &&`. The close is still to
    ungate them — a UX decision about the desktop menu's contents, and 53.1 had no
    more licence to make it than 52.7 did.
  - task board (`notes/task-board.tsx`, both the session and the widget mount): the
    gesture works AND the dead drop zone is fixed. The whole column box is the target
    now — its padding, its header and the empty space below the last card — where the
    `<ul>` used to take the drop, not fill the box, and the box drew the highlight; the
    cue is drawn on the element that accepts the release. This entry is still NOT
    extended to the board: its per-card column menu remains the keyboard path,
    hover/`focus-within` revealed with `opacity-0` (not `hidden`), untouched by 53.1.
  Every claim above is mutation-proven: removing the slop, the click suppression, the
  vacated-slot arithmetic, the card-midpoint tally, the column bounds, the cue's
  source, the pins stale-index guard, or re-cutting the target at the old `<ul>`'s
  edges each fails a named test in `notes/task-board.test.tsx`,
  `sessions/session-board.test.tsx` or `layout/pins-strip.test.tsx`.
note: 2026-08-17 (story 53.1, review) — one correction to the note above and one
  limit it did not state.
  THE CORRECTION. "Both now press, move and release through one state machine" was
  true and the machine had a defect that made the pins strip's desktop reorder dead
  again, for the third time: the move that crosses the slop takes the pointer
  capture, and it is the SAME move that paints the reorder preview. A preview that
  reorders a keyed list makes React move the pressed node, `insertBefore` removes
  that node from its parent before putting it back, and the removing steps are what
  Pointer Events hooks for the *implicit release* of pointer capture. So the strip
  released its own capture on the drag's first step, the following moves and the
  release were discarded by the `pressRef === null` guard, and the strip snapped
  back. No test could see it: `src/test/setup.ts` stubs `setPointerCapture` /
  `releasePointerCapture` / `hasPointerCapture` as no-ops and nothing in the repo
  dispatched `lostpointercapture`, so the headline reorder test passed over a dead
  drag. `hooks/use-pointer-drag.ts` now listens for the release natively on the
  captured element — React delegates at the root container, so an element removed
  for good never reaches a delegated `onLostPointerCapture` at all — and tells the
  two causes apart by `isConnected`: MOVED takes the capture back (the node is still
  mounted and its handlers are intact), UNMOUNTED ends the gesture and clears the
  swallowed-click flag with it. Both surfaces are covered by the one fix; the board
  was safe only by accident, since an external `order:` edit from Obsidian, an agent
  or the watcher moves a pressed card mid-drag exactly as a preview does.
  THE LIMIT, which the story's title and the note above did not qualify. On the task
  board a finger lifts a card only where the gesture does not race a scroll
  container. `notes/task-board.tsx` deliberately leaves `touch-action` alone (claiming
  it would stop a phone scrolling the board by starting on a card, which is
  the commoner gesture), so where an ancestor can pan in the direction the finger
  travels, the browser claims the gesture at about the 6 px slop and the board gets
  `pointercancel`, which returns the card. At phone width the four columns stack
  (`grid-cols-1`), so EVERY cross-column move travels along the page's own pan axis
  and the touch lift does not survive there. Above the phone tier the columns sit
  side by side, cross-column travel is horizontal, nothing pans that way, and a
  finger does move a card. The non-gesture path on that tier is the per-card column
  menu, which is in the DOM, in the tab order and in the accessibility tree at all
  times. The pins strip has no such limit: its phone entry is a long-press lift and
  its avatars are `touch-none`. The cure for the board is the same route — gate the
  touch press behind `hooks/use-long-press` and pass `captureNow`, which
  `use-pointer-drag` already supports — and it is a UX decision about what a press on
  a card should mean on a phone (a hold, with the scroll it costs), not a defect fix,
  so 53.1's review did not make it.

### DW-38: A favourited chat has no unread/mention affordance anywhere in the inbox view — the Favorites section renders compact rows as avatar + name only, and favourited rooms are removed from the Inbox window, so an unread favourited conversation shows no bold-name/dot/mention badge on any surface.

origin: migrated from legacy ledger (spec-4-4-favorites.md), 2026-07-06
location: src/components/.../favorites-section.tsx + keeper-core inbox.rs (emit)
reason: `favorites-section.tsx` compact rows render only `RoomAvatar` + `displayName` (per the spec's "avatar + single-line name"); `inbox.rs::emit` routes `!pinned && is_favourite` rooms exclusively to the Favorites window (excluded from Inbox/Archive), so the epic-wide "chat name weight 600 = unread, neutral dot / filled mention badge" cue (Story 4.1) never appears for them. This matches the spec-as-written and the Pins-strip precedent (4.3 hoisted rows are avatar-only), so it is not a defect in this story, but it is a real information-loss surprise: favouriting a chat hides its unread state from the whole inbox surface. Deferred as a UX-design decision — whether compact Favorites rows should carry the 4.1 unread/mention treatment — rather than inventing the visual treatment unattended in this story.
status: open
decision: 2026-07-06 Add unread/mention cue — Add the 4.1 unread/mention treatment to compact Favorites rows and emit the needed unread/mention state for favourited rooms.

### DW-39: Two Spaces with the same display name owned by different accounts render as indistinguishable SPACES sidebar rows (avatar + name only), so a user cannot tell which account's Space they are selecting.

origin: migrated from legacy ledger (spec-4-5-spaces-as-room-group-views.md), 2026-07-06
location: src/components/.../spaces-group.tsx
reason: `spaces-group.tsx` renders each row as `Avatar` + `space.name` with no account attribution; the selection key is correctly `(accountId, spaceId)` so the wrong Space is never actually selected, but two identically-named Spaces across accounts look the same. The chip and empty-state copy are likewise name-only. This story's `Never` explicitly defers per-row Account attribution to Story 4.6 (FR-24, the 8-hue account marker / Network badge conventions), which is the correct home for disambiguating same-named rows across accounts. Deferred rather than inventing an attribution treatment here.
status: open

### DW-40: The per-account Spaces producer recomputes the full Space list + child membership on every `subscribe_to_all_room_updates()` batch (i.e. essentially every sync tick), with no debounce and no diff against the prior snapshot, so identical snapshots still stream and re-emit.

origin: migrated from legacy ledger (spec-4-5-spaces-as-room-group-views.md), 2026-07-06
location: keeper-core account.rs (run_spaces_producer / compute_and_push_spaces)
reason: `account.rs::run_spaces_producer`/`compute_and_push_spaces` re-enumerates `joined_space_rooms()` and reads each Space's `m.space.child` state on every `RoomUpdates`, then `inbox.rs::update_spaces` always emits a fresh `SpacesSnapshot` (the window re-emit is now gated on an active filter — Story 4.5 review patch — but the snapshot emit and full recompute are not). Cost is bounded (Spaces are few; all reads are local store reads off the render thread) so it is not a live performance defect against the 60fps/10k-chats floor, but it is redundant work and redundant IPC per sync. A debounce/coalesce of the broadcast plus a snapshot-equality short-circuit (skip `update_spaces` when the computed list+membership is unchanged) would remove it; deferred as an optimization rather than added unattended.
status: open

### DW-41: `run_spaces_producer`'s broadcast-driven lifecycle — `Lagged` forcing a full recompute, `Closed` stopping the producer, and the initial-compute-before-first-update ordering — has no Rust test; only the merger (fed directly) is unit-tested.

origin: migrated from legacy ledger (spec-4-5-spaces-as-room-group-views.md), 2026-07-06
location: keeper-core account.rs (run_spaces_producer)
reason: The merger's Space filtering/exclusion/membership/removal paths are well covered by direct `InboxMerger` tests, but the concurrency-sensitive producer (`account.rs::run_spaces_producer`, which owns the `Client`, the `subscribe_to_all_room_updates()` receiver, and the recompute loop) is untested because `keeper-core` has no mock Matrix sync harness — the same crate-wide limitation already deferred for `mark_room_read`/`mark_room_unread` (spec-4-1) and the timeline send-error paths (spec-3-5). Closing it needs a shared mock-homeserver/sync fixture rather than a story-local test; deferred to that test-infrastructure investment. See [[DW-34]], [[DW-27]].
status: open

### DW-42: `room_item_to_vm` now resolves each room's bridged-Network label via `bridge::room_bridge_network` for every room of every room-list batch (Reset + incremental), where previously it was an on-demand single-room call used only for the delete confirmation.

origin: migrated from legacy ledger (spec-4-6-network-account-attribution-and-network-filter.md), 2026-07-06
location: keeper-core account.rs (room_item_to_vm)
reason: `account.rs::room_item_to_vm` awaits up to two `get_state_events` reads (`m.bridge` + legacy `uk.half-shot.bridge`) per room; on a large account a single `Reset` becomes N sequential local state-store reads before the batch emits, re-running on every re-sync. Reads are local (in-memory current-state cache after initial sync) and off the render thread, so it is not a live defect against the 60fps/10k-chats scroll floor, but it is redundant per-batch work that scales with room count. A per-room network cache keyed by room id (invalidated on `m.bridge` state change), or resolving only on change, would remove it; deferred as an optimization/measurement rather than added unattended. See [[DW-62]].
status: open

### DW-43: The single archive writer task is never drained on app shutdown, so events still queued in the unbounded channel when the process exits are lost (not persisted).

origin: migrated from legacy ledger (spec-5-1-archive-ingestion-pipeline.md), 2026-07-06
location: keeper-core archive/mod.rs + ingest.rs
reason: `archive/mod.rs` feeds the writer via a `tokio::sync::mpsc::unbounded_channel` and `ingest::run` only drains until all `ArchiveHandle` senders drop; `account.rs::shutdown` removes the per-account event handler but `AccountManager.archive` lives for the whole app lifetime with no flush/join on quit. In steady state the writer keeps up (per-event write is ~microseconds) so the queue is near-empty, and NFR-8 "zero lost *persisted* events" is about committed rows (unaffected), so this is not a defect against the spec — but a large backfill burst immediately followed by app quit could drop the tail. A proper graceful drain needs an app-exit hook (Tauri lifecycle) that is out of this backend-only story's surface; deferred rather than wired unattended.
status: open

### DW-44: The archive writer task has no health/restart supervision, so if it ever dies (panic, or the fallback-thread current-thread runtime fails to build) archiving silently stops for the rest of the session while `ingest` just log-and-drops every event.

origin: migrated from legacy ledger (spec-5-1-archive-ingestion-pipeline.md), 2026-07-06
location: keeper-core archive/mod.rs (spawn_writer)
reason: `archive/mod.rs::spawn_writer` discards the tokio `JoinHandle` / thread `JoinHandle` (`.map(|_| ())`) and `ArchiveWriter::spawn` returns `Ok(handle)` before the fallback thread's runtime is known-good; a dead writer leaves `tx` open so `ingest` logs "writer channel closed" per event with no recovery. Triggers are near-impossible in practice (current-thread runtime build essentially never fails; the writer body swallows rusqlite errors and cannot panic on the covered paths), so it is not a live defect, but there is no supervision/restart or a surfaced "archiving disabled" signal. Deferred as a robustness/observability hardening (health flag + restart, or fail `spawn` if the writer never comes up).
status: open

### DW-45: Ingestion captures only `m.room.message` events from the live sync flow; other message-like events (reactions, stickers), state events, back-paginated history, and re-decryption of previously-UTD events are not archived.

origin: migrated from legacy ledger (spec-5-1-archive-ingestion-pipeline.md), 2026-07-06
location: keeper-core account.rs (register_archive_handler)
reason: `account.rs::register_archive_handler` registers an `add_event_handler` typed `OriginalSyncRoomMessageEvent`, which fires only for non-redacted `m.room.message` delivered forward through sync. This matches the story's ACs (text/media message history through the sync flow) and the epic's sequencing — redaction/edit durability is Story 5.2, archive-first pagination is Story 5.6 — so it is not a defect in 5.1, but the archive is intentionally not a total capture of everything a server holds. Deferred: decide which additional event classes (reactions, UTD-then-redecrypted events, paginated history) the archive should ingest, in the stories that own those concerns.
status: open
decision: 2026-07-06 Scope later per-concern

### DW-46: If `Platform::data_dir()` fails, `AppState::new` points `archive.db` at the OS temp dir, so the archive would silently land in a wipe-on-reboot location instead of disabling archiving.

origin: migrated from legacy ledger (spec-5-1-archive-ingestion-pipeline.md), 2026-07-06
location: keeper/src/ipc.rs (AppState::new)
reason: `keeper/src/ipc.rs::AppState::new` does `platform.data_dir().unwrap_or_else(|_| std::env::temp_dir().join("dev.tgorka.keeper"))` and passes that to `AccountManager::new`. `data_dir()` failing is essentially impossible on supported desktop platforms (so no live impact), but in that degraded case a "durable" archive that evaporates on reboot is arguably worse than one cleanly disabled. Deferred: on `data_dir()` failure, disable archiving (construct with `archive: None`) rather than fall back to a volatile path.
status: open

### DW-47: A remote redaction processed before its target message's `Insert` (same-sync-batch race between the two independent archive event handlers) marks zero rows; the target is later inserted un-redacted, so its `redacted_ts` is never set.

origin: migrated from legacy ledger (spec-5-2-durability-against-remote-rewrites-edit-history.md), 2026-07-06
location: keeper-core account.rs (register_archive_handler / register_redaction_handler) + archive/db.rs (mark_redacted)
reason: `account.rs` registers `register_archive_handler` (message Inserts) and `register_redaction_handler` (Redact marks) as two independent `add_event_handler` closures that both feed the single serialized writer channel; `db::mark_redacted` is a no-op zero-row `UPDATE` when the target row is absent (`archive/db.rs`), and the writer never reconciles a Redact that arrived before its Insert. In practice Matrix causal ordering places a message before its redaction in the timeline and the single FIFO writer usually enqueues the Insert first, and under the default honor-off posture an unmarked-vs-marked row is observably identical (content is retained either way), so this only leaks when honor-deletions is ON and the rare ordering race occurs. A robust fix needs a pending-redaction reconciliation (remember unmatched Redacts and re-apply on a later Insert, or check a pending set at insert time) rather than a trivial patch; deferred as a durability-robustness hardening. Observability is also thin here — `mark_redacted` does not surface the affected-row count, so a lost mark is invisible in logs.
status: open

### DW-48: The `events_fts` external-content index has no delete/update maintenance path, so any later story that removes or mutates an archived row (Story 5.7 per-account archive deletion, a GDPR erase, or an in-place body rewrite) must also update the FTS index or it will silently retain stale trigrams and can trip `SQLITE_CORRUPT_VTAB` on a future `'rebuild'`/`'integrity-check'`.

origin: migrated from legacy ledger (spec-5-3-offline-full-text-search-engine.md), 2026-07-06
location: keeper-core archive/fts.rs
reason: `archive/fts.rs` only ever runs `INSERT INTO events_fts(rowid, body)` (`index_body`); there is no `INSERT INTO events_fts(events_fts, rowid, body) VALUES('delete', …)` anywhere and `mark_redacted` deliberately does not touch the index (redaction is retrieval-gated at query time via the root-redaction `NOT EXISTS` clause, which is correct for 5.3's mark-not-erase scope). This is by-design for Story 5.3 (its `Never` forbids FTS deletion, reserved for 5.7), but is a real trap the instant a real delete/erase lands: Story 5.7 (and any GDPR/erase work) must add the FTS5 external-content `'delete'` command — or install `AFTER DELETE`/`AFTER UPDATE` sync triggers at table creation — so the index can never drift from `events`.
status: done 2026-07-06
resolution: already resolved: archive/db.rs:463-491 delete_account_archive issues INSERT INTO events_fts(events_fts,rowid,body) SELECT 'delete',... before DELETE FROM events in one BEGIN IMMEDIATE txn. Story 5.7 (the named actionable trigger) added the FTS5 external-content delete maintenance path exactly as required; no other indexed-row delete/mutate path exists today.

### DW-49: `events_fts` uses `content_rowid='rowid'`, but `events` has no `INTEGER PRIMARY KEY` (its PK is the composite `(account_id, event_id)`), so its implicit `rowid` is not stable across `VACUUM`; if `archive.db` ever gains an explicit `VACUUM` or `PRAGMA auto_vacuum`, rowids renumber while the FTS shadow tables keep the old numbers and search silently returns wrong-message deep-links.

origin: migrated from legacy ledger (spec-5-3-offline-full-text-search-engine.md), 2026-07-06
location: keeper-core archive/fts.rs
reason: `archive/fts.rs` joins `events JOIN events_fts ON events_fts.rowid = events.rowid`. Nothing in the repo runs `VACUUM` and the default `auto_vacuum` is `NONE` (verified: no `vacuum`/`auto_vacuum` anywhere under `src-tauri/crates`), so rowids are stable today and the risk is purely latent. A future maintenance/compaction feature that introduces VACUUM must either give `events` a stable surrogate `INTEGER PRIMARY KEY` to use as `content_rowid`, forbid VACUUM on `archive.db`, or rebuild `events_fts` after any VACUUM.
status: open

### DW-50: A search deep-link to a message further back than the loaded timeline window + bounded live back-pagination cannot reach lands the user in the Chat with an honest "further back in history" note instead of on the matched message; true seek-to-event requires Story 5.6's archive-first pagination.

origin: migrated from legacy ledger (spec-5-4-search-ui-global-and-in-chat.md), 2026-07-06
location: src/components/.../conversation-pane.tsx (landing loop)
reason: `conversation-pane.tsx`'s landing loop resolves the hit's `eventId` to a render key via `resolveTimelineEventKey` and, when not loaded, does bounded `paginateBackwards` (MAX_ROUNDS=5, BATCH=40) before degrading. Story 5.4 depends only on 5.3 (the FTS engine); archive-first / seek-to-event pagination is explicitly Story 5.6, so an old match beyond ~200 live-paginated events is not jumpable yet. This is a spec-sanctioned honest degrade (never a wrong jump, never a silent no-op), not a defect — Story 5.6 should complete the deep-link by serving the matched event from `archive.db` (or a matrix-sdk-ui focused/`TimelineFocus::Event` timeline) so the jump always lands.
status: open

### DW-51: Session-free media-byte resolver for export include-media (currently injected as `None`, so every media item is skipped-and-counted while the Markdown link + JSON metadata are still emitted).

origin: migrated from legacy ledger (spec-5-5-export-to-json-and-markdown.md), 2026-07-06
location: keeper-core archive/export/mod.rs (copy_media_bytes / media_relative_link)
reason: Story 5.5 deferred session-free media byte inclusion (AD-11 forbids export touching the SDK store/live session, and the SDK media cache is deleted on sign-out). The copy path (`copy_media_bytes` in `archive/export/mod.rs`) exists and is unit-tested but is dead until a resolver is wired. When implementing the resolver, also harden its copy path (surfaced by review but latent behind the `None` resolver): (1) de-duplicate on-disk media filenames — `media_relative_link` can collide two items to the same `media/<event_id>-<sanitized_filename>` and silently overwrite; (2) reject `.`/`..` filename tokens defensively; (3) treat a per-file write failure as skip-and-count (best-effort), not a whole-export failure.
status: open

### DW-52: The SDK event-cache background tasks spawned by `event_cache().subscribe()` in `account::activate()` are the only SQLite-holding tasks in the account lifecycle not explicitly aborted-and-awaited by `shutdown()` before `sign_out_cleanup` runs `remove_dir_all` on the sdk dir.

origin: migrated from legacy ledger (spec-5-6-archive-first-pagination.md), 2026-07-06
location: keeper-core account.rs (shutdown / activate)
reason: `account.rs:shutdown()` deliberately aborts+awaits every other `Client`-clone-holding task (inbox producer, spaces producer, session persister) and stops sync before dir deletion, precisely to release SQLite handles first. The event-cache tasks (`room_updates_task` writer, auto-shrink, redecryptor, thread-subscriber) live in the SDK's internal `EventCacheDropHandles` and abort only on the last `Client` clone dropping — matrix-sdk 0.18 exposes no public API to stop them explicitly. Impact is bounded today because `shutdown()` calls `handle.sync.stop().await` first, which quiesces the room-updates writer (it is fed by `subscribe_to_all_room_updates()`) before the dir is removed, and the condition is substantially pre-existing (`TimelineBuilder::build()` already subscribed the event cache lazily whenever a room was opened). Harden by sequencing an explicit event-cache teardown (or a guaranteed final `Client` drop) before `sign_out_cleanup`, if/when the SDK offers a handle.
status: open
decision: 2026-07-06 Guarantee last-Client-drop — Audit every Client clone in shutdown() and guarantee the last one is dropped before sign_out_cleanup, so EventCacheDropHandles fire and release SQLite handles before dir removal.

### DW-53: Automated coverage of Story 5.6's user-visible behavior is missing — the test asserts only `has_subscribed()` transitions/idempotency, not that activation-time subscribe causes a not-yet-opened room's synced events to persist to the `SqliteEventCacheStore` and that `paginate_backwards` serves them from disk offline.

origin: migrated from legacy ledger (spec-5-6-archive-first-pagination.md), 2026-07-06
location: src-tauri/crates/keeper-core/tests/event_cache_pagination.rs
reason: `tests/event_cache_pagination.rs` verifies the enablement invariant (`subscribe()` flips the flag, idempotent) but not persistence or archive-first pagination; the FR-17 claim ("served from local disk, instant, offline") is only manually verified (spec Verification → manual checks, OQ-1 / Epic 11 perf harness). A stronger integration test would feed a `RoomUpdates`/sync response (matrix-sdk-ui test utilities) and assert store rows exist / the on-disk sqlite grows / a subsequent `paginate_backwards` returns without a homeserver request.
status: open

### DW-54: Now that `activate()` subscribes the event cache before sync, every synced room persists to the on-disk `SqliteEventCacheStore` from the first sync (previously only rooms opened this session, lazily) — the store's growth/retention bounds and its content duplication with `archive.db` (Story 5.1) are unassessed.

origin: migrated from legacy ledger (spec-5-6-archive-first-pagination.md), 2026-07-06
location: keeper-core account.rs (activate — SqliteEventCacheStore)
reason: The change broadens on-disk persistence from opened-rooms-only to all-synced-rooms. The SDK's `auto_shrink_linked_chunk_task` bounds in-memory linked chunks but the persisted event-cache store still grows, and `archive.db` already persists message content independently — so keeper now holds two overlapping on-disk copies of synced history. For a long-lived archival client this is a capacity question (eviction/retention policy, disk-growth expectations) worth a deliberate assessment; it is a spine-sanctioned trade-off today (SPINE places the persisted event cache in the sdk dir), not a defect.
status: open
decision: 2026-07-06 Measure first — Measure real disk-growth expectations before committing to a retention policy.

### DW-55: There is no UI entry point to purge a leftover local archive for an account that is already signed out (a failed or app-killed-mid-flow archive deletion leaves that account's rows on disk with no later retry surface).

origin: migrated from legacy ledger (spec-5-7-archive-survives-sign-out-and-deletes-only-on-command.md), 2026-07-06
location: src/hooks/use-sign-out.ts (no Settings → Archive-management surface)
reason: Story 5.7's only archive-delete entry point is the sign-out `SignOutDialog`; once `sign_out` completes and the account is removed from the switcher, the account is no longer listed, so the "…and delete this Account's archive" path is unreachable. The delete path is fire-once (`use-sign-out.ts`): sign-out → removeAccount → `deleteAccountArchive`, and on a purge rejection (or an app kill between the two IPC calls) the archive rows persist with no way to re-trigger the purge. The Rust `delete_account_archive` is keyed by `account_id` only and does not require a live session, so a future Settings → Archive-management surface could list accounts with residual `archive.db` rows and offer a re-delete — deferred as out of scope for the sign-out-time delete this story owns.
status: open

### DW-56: Bridge discovery source (c) bot-DM detection is gated behind `room.is_direct() == Ok(true)`, but self-hosted mautrix/Beeper bot management rooms are frequently NOT flagged `m.direct`, so the `not logged in` status can silently fail to trigger on exactly the homeservers this feature targets.

origin: migrated from legacy ledger (spec-6-2-bridge-discovery.md), 2026-07-06
location: keeper-core bridges/discovery.rs (scan_rooms)
reason: `bridges/discovery.rs::scan_rooms` only inspects `direct_targets()` when `room.is_direct().await == Ok(true)`; the appservice-created management room often carries no `m.direct` account-data entry, so `direct_targets()` yields nothing and a genuinely-present-but-not-logged-in bridge degrades to `Configured` (via source b) or drops entirely. The spec's I/O-matrix "Bot DM, no portal → not logged in" row prescribes exactly `is_direct()`/`direct_targets()` (intent contract), and the impure shell has no mock-`Client` test asserting `is_direct()` fires for a mautrix management room, so this completeness gap ships untested. A later story should broaden management-room detection (e.g. also treat a joined room whose only other member is a known bot MXID as a management DM) rather than trust the `m.direct` flag alone. See [[DW-57]], [[DW-58]].
status: open
decision: 2026-07-06 Broaden detection — Broaden management-room detection to also treat a joined room whose only other member is a known bot MXID as a management DM, rather than trusting the m.direct flag alone.

### DW-57: The known-bot MXID probe (source b) and bot-DM matching (source c) both hard-code the account's OWN server name, so bridge bots living on a separate appservice/bridge domain are invisible to discovery on standard self-hosted mautrix stacks.

origin: migrated from legacy ledger (spec-6-2-bridge-discovery.md), 2026-07-06
location: keeper-core bridges/discovery.rs (bot_network_for)
reason: `bridges/discovery.rs` builds the probe MXID as `@{localpart}:{server_name}` from `own_user.server_name()`, and `bot_network_for` rejects any DM target whose `server_name() != own_server`. Under the common self-hosted topology (user `@me:example.org`, bot `@whatsappbot:bridge.example.org`) sources (b) and (c-bot-DM) both produce nothing and only source (a)/(c-portal) can find the network. The single-domain (Beeper-style) assumption is documented in code docstrings but is not called out in the spec's Boundaries as an accepted limitation, so on a standard mautrix deployment discovery will under-report. A later story should either probe/accept the appservice/bridge domain(s) or record this as an explicit, product-blessed topology constraint. See [[DW-56]].
status: open
decision: 2026-07-06 Probe bridge domains — Probe/accept configured appservice/bridge domain(s) in addition to the account's own server when building probe MXIDs and matching DM targets.

### DW-58: The impure discovery shell (`fetch_protocol_ids` wiring, `scan_rooms` portal/DM detection, `bot_network_for` server matching, `probe_network_bots` error-kind handling) has zero test coverage — only the pure `merge_discovery`/`merge_catalog`/`protocols_error_degrades` functions are unit-tested.

origin: migrated from legacy ledger (spec-6-2-bridge-discovery.md), 2026-07-06
location: keeper-core bridges/discovery.rs
reason: `bridges/discovery.rs` tests (`#[cfg(test)]`) cover only the pure merge/classifier functions; every branch that touches Matrix is unverified, and matrix-sdk ships a mock `Client` (used elsewhere in the crate) that makes this testable without a live homeserver. This was a documented tradeoff in the spec (Design Notes: "keep all Matrix I/O out of it so precedence is unit-tested without a homeserver"; residual risks acknowledge the shell is not e2e tested), but it is precisely where the two completeness risks above live — a mock-`Client` integration test asserting a mautrix management-DM room yields `NotLoggedIn` and a portal room yields `LoggedIn` would convert the highest *product* risk from "manually reasoned" to "verified." See [[DW-56]], [[DW-57]].
status: open

### DW-59: The shipped `health-signals.json` sets `enablePing: false` for both the `default` and `whatsapp` grammars, so the bot-ping liveness fallback never runs and a truly silent bridge death (no management-room notice) is undetectable — leaving AC1's "silent death caught by the liveness tick" clause inert in the shipped configuration.

origin: migrated from legacy ledger (spec-6-5-bridge-session-health-and-re-login-prompts.md), 2026-07-06
location: health-signals.json + keeper-core bridges/health.rs (run_liveness_tick)
reason: `run_liveness_tick` (bridges/health.rs) early-returns when no wiring enables ping (a prior-pass patch), so `ping_once`/`PingTimeout`/the debounce path is dead code in production; health only changes when the bot voluntarily posts a notice. The mechanism is fully implemented and unit-tested and enabling it is a data-only change, but doing so trades against the intent's "Never: continuous/aggressive bot-pinging that spams the management room" constraint — a product decision on whether (and per which networks) to ship ping enabled, with the spam-cadence tradeoff, is owed. Related consequence: a session stuck `Degraded` ("reconnecting") with ping off and no further notice never escalates to `Disconnected`.
status: open
decision: 2026-07-06 Ship ping (chosen cadence) — Enable ping for a chosen conservative set of networks and cadence in health-signals.json, balancing silent-death detection against the spam constraint.

### DW-60: The non-dismissible in-conversation health banner's visibility is coupled to window membership — it resolves `networkId` from `useSelectedRoomVm()`, which returns `null` when the open room is not in any streamed window, which would silently hide the story's primary in-conversation safety surface.

origin: migrated from legacy ledger (spec-6-5-bridge-session-health-and-re-login-prompts.md), 2026-07-06
location: src/components/layout/conversation-pane.tsx (ConversationHealthBanner)
reason: `conversation-pane.tsx` sets `selectedNetworkId = selectedRoom?.networkId ?? null` and `ConversationHealthBanner` early-returns null when `networkId === null`. Today all four window stores stream full windows, so the selected room is present and the banner shows; but the hook's own contract documents a "future true-windowing or filter-hidden case" where the selected room is absent — under that future change a disconnected bridge's banner would vanish with no indication. The conversation header already degrades under the same condition (Story 4.6), so this inherits a pre-existing windowing limitation rather than introducing a new one; worth revisiting when true windowing lands, since a safety banner disappearing is more consequential than a header degrading.
status: open

### DW-61: Adding/removing/signing-out an account tears down and rebuilds all bridge-health monitors, re-bootstrapping every session to `Healthy` from discovery — transiently dropping any surfaced unhealthy state and the bot's `detail` reason until a bridge re-posts a notice.

origin: migrated from legacy ledger (spec-6-5-bridge-session-health-and-re-login-prompts.md), 2026-07-06
location: src/hooks/use-bridge-health.ts
reason: `use-bridge-health.ts` keys the subscription effect on the sorted account-id set, so any set change re-invokes `subscribe_bridge_health`, which re-runs `bridges::discover` and re-bootstraps sessions via `HealthState::new_healthy()`. Discovery reports session *config* state (`LoggedIn`), not live health, so a session previously flipped `Disconnected` from a management-room notice re-bootstraps to `Healthy`, briefly violating the "unhealthy is persistent until resolved" guarantee on unrelated account changes. Preserving accumulated `HealthState` across a re-subscribe (rather than re-bootstrapping) would close it; non-trivial (needs old→new aggregator state carry-over), infrequent trigger.
status: open

### DW-62: Building each room's VM now performs two independent `m.bridge` state-event lookups (`room_bridge_network` + the new `room_bridge_protocol_id`), doubling the per-room bridge-state reads on the hot inbox/pins/favorites/archive window-diff path.

origin: migrated from legacy ledger (spec-6-5-bridge-session-health-and-re-login-prompts.md), 2026-07-06
location: keeper-core account.rs (room_item_to_vm) / bridge.rs
reason: `room_item_to_vm` (account.rs) calls both helpers; each (bridge.rs) iterates `[BRIDGE_EVENT_TYPE, LEGACY_BRIDGE_EVENT_TYPE]` and calls `get_state_events(...).await`, parsing the same `m.bridge` event twice to extract the display label and the `protocol.id`. Reads are local (state store, not network) so impact is bounded, but for large accounts (hundreds of bridged rooms) every window rebuild now does ~2× the bridge-state work 6.1–6.4 kept to a single pass. A combined helper returning `(network_label, protocol_id)` from one `get_state_events` pass would remove the regression; moderate refactor touching the shared helpers + both call sites + tests. See [[DW-42]].
status: open

### DW-63: The impure `HealthAggregator` boundary that decides what crosses IPC (`observe` → `diff_sessions` gate → sink, and nulling a closed sink) has no direct unit test — only the pure `HealthState::apply`/`diff_sessions`/`classify_health_signal` and a `HealthState`-level scripted-observation test exist.

origin: migrated from legacy ledger (spec-6-5-bridge-session-health-and-re-login-prompts.md), 2026-07-06
location: keeper-core bridges/health.rs (HealthAggregator)
reason: `scripted_observation_sequence_yields_expected_emissions` (bridges/health.rs) drives `HealthState::apply` directly and never constructs a `HealthAggregator` or a sink; the "emit only on change" cadence contract the frontend relies on for the pulse/roll-up is verified only at the pure-diff layer, not at the aggregator that wires diff→sink. The shell is documented residual risk, but a focused test with a mock sink asserting (a) no emit on an idempotent recompute, (b) emit on a real per-session change, and (c) graceful handling of a closed sink channel would convert the diff-gate contract from "reasoned" to "verified."
status: open

### DW-64: A `bbctl run` that emits no recognized started/error prose marker (or is genuinely slow to start) leaves the run stepper with no terminal state and no timeout — only a user cancel escapes; add a bounded run-start timeout / unrecognized-daemon handling when the provisional prose markers are tuned against a real `bbctl` binary.

origin: migrated from legacy ledger (spec-6-7-bbctl-integration-for-beeper-self-hosted-bridges.md), 2026-07-06
location: keeper-core bbctl.rs (run_self_hosted) / keeper ipc.rs (DesktopBbctlRunner::run)
reason: `run_self_hosted` (bbctl.rs) only reaches a terminal `success`/`failure` on a recognized marker or a natural process exit; a persistent daemon that never EOFs and never matches `is_started_marker`/`is_error_marker` keeps the merged-stream consume loop (`DesktopBbctlRunner::run`, ipc.rs) reading indefinitely while the Sheet spins on "Bringing it up". The prose marker set is documented residual risk (tunable against a real binary), but the *hang shape* (no timeout, no unrecognized-daemon fallback) is real and worth a bounded start-timeout + honest failure once a real binary is available to calibrate against.
status: open

### DW-65: On an early-`Stop` (error marker during the finite `bbctl register`, or the started marker during `run`) the runner leaves the child unreaped (no `wait()`, no kill), so a failed/finite `register` can orphan a defunct process; process reaping/supervision is explicitly v1.x but a best-effort detached reaper for the finite `register` action would avoid zombies.

origin: migrated from legacy ledger (spec-6-7-bbctl-integration-for-beeper-self-hosted-bridges.md), 2026-07-06
location: keeper/src/ipc.rs (DesktopBbctlRunner::run)
reason: `DesktopBbctlRunner::run` (ipc.rs) returns `StoppedEarly` without `child.wait()` for BOTH actions (correct launch-and-leave for the `run` daemon, but `register` is finite). A dropped `tokio::process::Child` is not killed and not reaped on Unix, so repeated failed registers accumulate defunct entries until process exit. Supervision is out of scope (Boundaries "Never: auto-restart supervision / persisted child handle"), but a per-`register` detached `wait()` reaper would close the zombie path without adding supervision.
status: open

### DW-66: `BridgeCard` shows a "Connect" action button and a "Not set up" status word for a bridge whose discovered status is already `loggedIn`/`configured`, which reads as misleading in the first-run "Connect your networks" framing.

origin: migrated from legacy ledger (spec-6-8-first-run-wizard.md), 2026-07-06
location: src/components/.../bridge-card.tsx + bridges.ts
reason: `BridgeCard`'s action label is driven only by `network.requiresAck` and `liveHealth === "disconnected"` (bridge-card.tsx), never by the discovery `status` prop, and `BRIDGE_STATUS_LABEL.configured === "Not set up"` (bridges.ts). Pre-existing 6.1/6.2 behavior shared with the Bridges pane; surfaced (not caused) by story 6.8 reusing the card in the wizard.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `BridgeCard` gates its action on `offersAction = showRedEdge || status !== "loggedIn"` — a Network already discovered `loggedIn` offers no Connect/Set up button (the Manage menu is its affordance); a loggedIn session whose live health went disconnected still offers Re-link.

### DW-67: Every draft CRUD op (`set_draft`/`get_draft`/`delete_draft`/`list_drafts`) opens a fresh SQLite connection that re-runs all `CREATE TABLE IF NOT EXISTS` + `PRAGMA table_info` column probes; the debounced keystroke save now pays this per-op schema-ensure at typing cadence, hotter than the `pins` precedent it mirrors.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (set_draft/get_draft/delete_draft/list_drafts)
reason: `registry.rs` `set_draft`/`get_draft`/`delete_draft`/`list_drafts` each call `open(data_dir)?`, which runs the pins + drafts `CREATE TABLE IF NOT EXISTS`, `ensure_hue_index_column`, and `ensure_provider_column` on every call. The debounced save fires ~every 200 ms of active typing (fire-and-forget, off the keystroke path, so not user-visible), but a shared/cached connection — or skipping the ensure-columns work on draft writes — would remove the per-write overhead. Pre-existing registry-wide pattern surfaced (not caused) by drafts landing on a far hotter path.
status: open

### DW-68: A pre-edit draft typed within the ~200 ms debounce window and not yet flushed is silently dropped from `keeper.db` when the user enters edit mode and sends the edit, because `send()` cancels the queued debounce; the in-session composer text is restored correctly, but a relaunch after such an edit shows the staler stored body.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: src/components/chat/composer.tsx (send)
reason: `composer.tsx` `send()` clears `draftSaveTimer` + nulls `pendingDraft` before dispatch (correct for reply/text to avoid a save/clear reorder). For `wasEdit`, it then `setDraft(preEditDraft.current)` — restoring the text on screen — but never re-persists it, so a pre-edit keystroke that was still inside the debounce when edit was entered was cancelled without ever reaching the DB. Narrow (requires entering edit <200 ms after typing) and only affects relaunch durability of the pre-edit draft, not the visible composer; a `scheduleDraftSave(preEditDraft.current)` after an edit-send would reconcile the row.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `composer.tsx` `send()` on the edit path now calls `scheduleDraftSave(preEditDraft.current)` after restoring the pre-edit text, so a draft typed inside the debounce window when edit was entered reaches keeper.db; test covers the unflushed-draft-then-edit-send sequence.

### DW-69: The `temp_dir()` registry-test helper derives its unique path from `process::id()` + a nanosecond `SystemTime` stamp, which can collide across parallel test threads in the same process, letting two tests share a data dir and intermittently fail (observed once as a transient `drafts_crud_roundtrip_and_upsert` failure that passed on re-run).

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (temp_dir test helper)
reason: `registry.rs` `temp_dir()` (shared by all registry tests incl. the pre-existing `pins` tests) builds `keeper-registry-test-{pid}-{nanos}`; `cargo test` runs tests multithreaded within one process, so two tests entering `temp_dir()` within the same nanosecond get the same path and interfere. Pre-existing helper (not introduced by this story). A per-call atomic counter or `tempfile`-style guaranteed-unique dir would eliminate the flake.
status: open

### DW-70: A debounced `saveDraft` that has already fired (IPC in flight) when the user sends can commit *after* the post-send `clearDraft`, resurrecting an orphan draft row + amber marker on an already-sent chat that survives relaunch; the prior mitigation only cancels the still-*queued* debounce timer, not an in-flight save.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: src/components/chat/composer.tsx (send) / src/lib/ipc/client.ts
reason: `composer.tsx` `send()` cancels `draftSaveTimer`/nulls `pendingDraft`, but `saveDraft` and `clearDraft` are two independent fire-and-forget `invoke()` calls with no ordering guarantee at the Tauri/SQLite layer (each `registry::open`s its own connection). If the debounce fired ~200 ms after the last keystroke and the user presses Enter within that save's in-flight window, the clear can win the write-lock race first, leaving a stale row `loadDraft` restores on next open. Narrow timing, recoverable, but violates the "send removes the row + marker" AC. A safe fix (serialize per-`(account,room)` writes through a promise chain in `client.ts`, or a Rust-side stale-write guard) touches the spec-protected send-ordering path and merits attended review rather than an unattended patch.
status: open
decision: 2026-07-06 Serialize writes (attended) — Serialize per-(account,room) draft writes through a promise chain in client.ts (or add a Rust-side stale-write guard), with attended review of the frozen send-ordering invariant.

### DW-71: `registry::open` sets only `journal_mode=WAL` with no `busy_timeout`, so a second concurrent writer fails immediately with `SQLITE_BUSY`; the debounced draft save now writes at typing cadence alongside pins/settings/account writes, and because saves are fire-and-forget with a swallowed `.catch`, a `SQLITE_BUSY` silently drops the draft while its in-memory amber marker still claims one exists.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (open)
reason: `registry.rs` `open()` runs `pragma_update(journal_mode, WAL)` and nothing else — no `busy_timeout`, no `synchronous` tuning. WAL serializes writers; without a busy timeout a contended `set_draft` returns `SQLITE_BUSY` instantly rather than waiting, and the fire-and-forget path (`composer.tsx` `flushDraft`) swallows it. Pre-existing registry-wide omission (all callers share `open()`), surfaced — not caused — by drafts landing on the hottest write path. A `PRAGMA busy_timeout` in `open()` would let contended writers wait instead of losing data.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `registry::open` now sets a 5 s `busy_timeout` (BUSY_TIMEOUT const) so a contended writer waits for the WAL write lock instead of failing `SQLITE_BUSY` and silently dropping a fire-and-forget draft save.

### DW-72: `set_draft` / the `drafts.body TEXT NOT NULL` column impose no length cap, so a multi-megabyte paste into the composer is shipped verbatim across IPC and rewritten into keeper.db on every ~200 ms debounce flush, an O(n) write on the path the design promised to keep cheap, and unbounded row growth bloats keeper.db.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper ipc.rs (set_draft) + keeper-core registry.rs (set_draft/schema)
reason: `ipc.rs` `set_draft` and `registry.rs` `set_draft`/schema store `body` verbatim with no size guard; `composer.tsx` `scheduleDraftSave` re-sends the whole body each flush. A user pasting a large document pays a full-body IPC + row rewrite per debounce tick and grows keeper.db without bound. A body length cap (truncate-or-reject) before the upsert would bound both.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `registry::set_draft` clips the body to `MAX_DRAFT_BODY_CHARS` (16,384 chars — above any single sendable Matrix event) via `cap_draft_body`, appending `DRAFT_TRUNCATION_MARKER` so a clipped draft admits it; bounds both the per-flush row rewrite and keeper.db growth.

### DW-73: Unsent draft bodies — the user's private message *content* — are stored as plaintext `TEXT` in keeper.db, a more sensitive class than the metadata (`pins`) precedent the design mirrors; the app otherwise treats at-rest data as sensitive (tokens in Keychain, an `sdk_encryption` posture), so plaintext message content on disk deserves a conscious documented security decision.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (drafts schema)
reason: `registry.rs` `drafts` schema stores `body` as cleartext; the intent contract's "keeper.db conventions" clause was framed against `pins` (room membership metadata), not message content. A stolen-laptop / leaked-backup threat model exposes half-written private messages verbatim. Worth an explicit ADR note confirming plaintext-at-rest is acceptable for draft content, or an encryption pass aligning drafts with the app's other at-rest sensitivity boundaries.
status: done 2026-07-06
resolution: closed by human decision: Record an ADR explicitly confirming plaintext-at-rest is acceptable for draft content (the minimal conscious decision); note encryption is the stronger posture if revisited.
decision: 2026-07-06 ADR accepting plaintext — Record an ADR explicitly confirming plaintext-at-rest is acceptable for draft content (the minimal conscious decision); note encryption is the stronger posture if revisited.

### DW-74: The cross-device mirror dedupe map `LAST_MIRRORED` (drafts.rs) is a process-wide static that grows one entry per `(account, room)` ever mirrored and is never pruned on account sign-out or teardown, so it accumulates unboundedly over a very long session.

origin: migrated from legacy ledger (spec-7-2-cross-device-draft-mirroring-with-local-wins-conflicts.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/drafts.rs (LAST_MIRRORED)
reason: `src-tauri/crates/keeper-core/src/drafts.rs` `should_mirror`/`forget_mirrored` insert into `static LAST_MIRRORED: Mutex<Option<HashMap<String, String>>>`; `AccountManager::shutdown`/sign-out never clears an account's keys. No functional break — after sign-out+re-login the server-side `dev.keeper.draft` account data still holds the last body, so a dedupe-skip of an identical re-mirror is actually correct (the remote is already populated). The only cost is slow memory growth (one small entry per room touched) and a latent trap if the map were ever consulted for correctness. A `forget_account(account_id)` pruning all keys with the account prefix, called from `shutdown`, would bound it; deferred because the consequence is low and the fix touches the account-teardown path.
status: open

### DW-75: `list_pending_drafts` resolves each pending draft's room name + bridge network sequentially (per-row `accounts` lock + `Room::display_name().await`), so a large draft set or one slow/hanging account can delay the whole cross-account Approval Pane render.

origin: migrated from legacy ledger (spec-7-3-approval-pane.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (list_pending_drafts)
reason: `src-tauri/crates/keeper-core/src/account.rs` `list_pending_drafts` loops over all draft rows awaiting `room_for` + `resolved_room_name` + `room_bridge_network` with no concurrency bound or timeout; MVP-tolerable at expected draft volumes but unbounded as drafts scale.
status: open

### DW-76: The Approval Pane's roving-tabindex exposes only a single Tab stop (first group's first row) with no ArrowUp/ArrowDown handler, so a keyboard-only user can reach and act on exactly one draft — every other row is `tabIndex={-1}` and unreachable by keyboard.

origin: migrated from legacy ledger (spec-7-3-approval-pane.md), 2026-07-06
location: src/components/approval/approval-pane.tsx
reason: `src/components/approval/approval-pane.tsx` sets `tabbable={groupIndex === 0 && index === 0}` and wires no arrow-key focus movement between rows; a proper roving-tabindex composite needs Arrow handlers to move the single stop. The core ACs (grouping, approve/discard/edit, empty state, badge) are met and the pane is fully mouse-operable, and `⌘3` reaches it, so this is a keyboard-a11y completeness gap rather than an AC violation. Deferred (not trivially patchable): correct roving focus management across grouped sections (index tracking, wrap, ref-driven focus, tests) is a focused a11y pass beyond this story's scope; the aria-label account-identity gap was fixed inline this pass, this larger keyboard-nav gap is tracked here.
status: open

### DW-77: A transient `listPendingDrafts` re-query failure while the pane already shows rows is silent — `queryFailed` is set but the error affordance only renders in the `isEmpty && queryFailed` branch, so a populated pane keeps showing last-known (possibly stale) rows with no "couldn't refresh" signal.

origin: migrated from legacy ledger (spec-7-3-approval-pane.md), 2026-07-06
location: src/components/approval/approval-pane.tsx
reason: `src/components/approval/approval-pane.tsx` `requery.catch` sets `queryFailed`, and the Retry/error affordance is gated on `isEmpty`; when rows are non-empty a failed refresh leaves the list frozen with no staleness indicator (the empty+failed case is correctly handled by the P6 affordance). Low consequence — the shown rows are the last authoritative snapshot, not wrong — and the list re-queries on any presence-key change, so it self-heals in normal flow. Deferred rather than patched this pass to keep the re-derived story's code churn minimal; a small non-blocking "showing last-known drafts — couldn't refresh" banner when `queryFailed && !isEmpty` would close it.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `approval-pane.tsx` renders a non-blocking `role="status"` staleness banner ("Showing last-known drafts — couldn't refresh just now…") when `queryFailed && !isEmpty`, keeping the rows; a later successful refresh retracts it. Empty+failed keeps the existing retry affordance. Tests cover both branches.

### DW-78: The header Incognito chip is a one-way binary toggle — once clicked it writes an explicit per-Chat override, and there is no UI affordance to clear that override back to "inherit", so the chat permanently stops following later global/account changes.

origin: migrated from legacy ledger (spec-8-1-incognito-read-receipts-with-scoped-policy.md), 2026-07-06
location: src/components/layout/conversation-pane.tsx (ConversationIncognitoChip)
reason: `src/components/layout/conversation-pane.tsx` `ConversationIncognitoChip` writes `incognitoSetChat(accountId, roomId, !vm.effective)` (an explicit `true`/`false`), and the chip only renders while effective — there is no per-chat "inherit" control (unlike the account menu's tri-state submenu). The story's AC only requires the chip to "toggle the per-Chat scope", which is satisfied; a tri-state per-chat control (e.g. a chip context menu or long-press) is a UX-completeness follow-up beyond this story. Backend precedence and storage already support clearing to inherit (`incognito_set_chat` accepts `Option<bool>`), so this is a frontend-only affordance gap.
status: done 2026-07-26
resolution: resolved by sweep wave 1. `ConversationIncognitoChip` is now tri-state: while an explicit per-Chat override exists (`vm.chat !== null`) both branches offer "Follow the account/global setting", which writes `incognitoSetChat(..., null)` so the chat resumes inheriting; hidden while already inheriting. Tests assert null (not false) is written.

### DW-79: matrix-sdk emits an *implicit public* read receipt as a side effect of sending a message, outside the `signals` sole-gate, so a user with Incognito effective who *sends* a message in the chat still leaks a public read position that Incognito cannot suppress.

origin: migrated from legacy ledger (spec-8-1-incognito-read-receipts-with-scoped-policy.md), 2026-07-06
location: keeper-core signals.rs (mark_read)
reason: The SDK stores implicit read receipts as public (matrix-sdk-ui timeline controller comment "Implicit read receipts are saved as public read receipts") and emits one on message send — a path that never flows through `keeper-core::signals::mark_read`, so neither the AD-14 sole-gate nor the effective-policy branch can intercept it. Story 8.1's scope is read-receipts-on-viewing (the `mark_room_read` path), and sending a message already reveals engagement to the remote, so the marginal privacy loss is small and this was not caused by the change — it is pre-existing SDK behavior surfaced incidentally. Worth a conscious decision: either wire a client-level suppression of implicit/public send receipts to the effective Incognito policy, or scope the `signals.rs` module-doc privacy claim to explicitly exclude the send path so the guarantee is not overstated.
status: done 2026-07-06
resolution: closed by human decision: Narrow the signals.rs module-doc privacy claim to explicitly exclude the send path, so the guarantee is not overstated (sending already reveals engagement).
decision: 2026-07-06 Scope the privacy claim — Narrow the signals.rs module-doc privacy claim to explicitly exclude the send path, so the guarantee is not overstated (sending already reveals engagement).

### DW-80: The Undo-Send outbox scheduler retries a held row whose room id parses but never resolves on the live Client (e.g. the user left the room, or it never syncs) on every 250ms tick forever, with no age bound or terminal "failed" surfacing — the held bubble/pill lingers indefinitely.

origin: migrated from legacy ledger (spec-8-3-undo-send-window.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (run_outbox_scheduler)
reason: `run_outbox_scheduler` (src-tauri/crates/keeper-core/src/account.rs) drops only *unparsable* room ids; for a parseable-but-unresolvable room `build_dispatch_timeline` returns Err and the row is left "for a later tick" unboundedly. Correct handling (bounded retry then surface as a failed send) is more than a patch and was out of the immediate fix scope.
status: open

### DW-81: The core "window elapses → held send fires" behavior of `run_outbox_scheduler` — picking up an elapsed row, dispatching it exactly once through `send::dispatch`, deleting it, and the `awaiting_delete` delete-retry path — has no automated test; it is verified only by inspection.

origin: migrated from legacy ledger (spec-8-3-undo-send-window.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (run_outbox_scheduler)
reason: `run_outbox_scheduler` (`src-tauri/crates/keeper-core/src/account.rs`) is only defined and spawned, never exercised by a test; the outbox tests cover registry CRUD, the hold-vs-submit branch, cancel-writes-draft, and the malformed-room regression, but none drives the scheduler through an elapsed dispatch. The dispatch step needs a live matrix-sdk `Client`/`Timeline` (via `build_dispatch_timeline`), and `keeper-core` has no mock-Matrix sync/timeline harness — the same crate-wide test-infrastructure limitation already deferred for `mark_room_read`/`mark_room_unread` (spec-4-1), the Spaces producer (spec-4-5), and the timeline send-error paths (spec-3-5). Not a code defect (the path passes fmt/clippy and its constituent pieces are tested); flagged so the scheduler's elapsed-dispatch and delete-retry get real coverage once a shared mock-sync fixture exists. See [[DW-34]], [[DW-41]], [[DW-27]].
status: open

### DW-82: The per-account Undo-Send scheduler polls the `outbox` table every ~250 ms for the whole account lifetime via a fresh `registry::open()` even when no holds exist, and the window setting is re-read with another fresh `open()` on every `send_text`/`send_approval` — a persistent idle DB-open cost with no wake-on-hold gate or cached setting.

origin: migrated from legacy ledger (spec-8-3-undo-send-window.md), 2026-07-06
location: keeper-core account.rs (run_outbox_scheduler) / registry.rs (open)
reason: `run_outbox_scheduler` (`src-tauri/crates/keeper-core/src/account.rs`) calls `list_outbox_rows_for_account` unconditionally each ~250 ms tick, and `registry::open` (`registry.rs`) opens a connection and runs the full `CREATE TABLE IF NOT EXISTS` schema-ensure block on every call; `get_undo_send_window` does the same on the hot send path. With N logged-in accounts that is ~4N connection-opens/sec on an idle app. The per-send settings read mirrors the established project-wide settings-access pattern (e.g. `get_incognito_global`) so it is not novel, and the 250 ms interval is the spec's chosen scheduler design, so this is not a defect — but a wake-on-hold gate (only run the scheduler loop while holds are pending) plus a cached window value would remove the idle cost. Deferred as an optimization needing measurement rather than an in-diff patch.
status: open

### DW-83: The palette (and the pre-existing Spaces) per-account producer fully re-projects every room in `client.rooms()` — re-resolving name, `is_direct()`, and bridge network for each — on every `subscribe_to_all_room_updates` batch, an O(rooms) recompute (up to ~10k async resolutions per tick) that holds the shared `PaletteIndex` lock; there is no incremental diff-based update path.

origin: migrated from legacy ledger (spec-9-1-command-palette.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/account.rs (run_palette_producer / compute_and_push_palette)
reason: `run_palette_producer` / `compute_and_push_palette` (`src-tauri/crates/keeper-core/src/account.rs`) rebuild the whole account slice per batch and on `Lagged`, mirroring `run_spaces_producer`. The `palette_query` read budget (<100 ms at 10k) is met and unaffected, and the full-recompute pattern matches the established Spaces producer, so this is not a defect — but at 10k rooms on a busy sync the indexing (write) cost is unbounded and unmeasured. Deferred as an incremental-indexing optimization (applies to both the palette and spaces producers) needing measurement rather than an in-diff patch. See [[DW-40]].
status: open

### DW-84: The action registry (`palette_actions()`) assigns the same shortcut chip to each half of a toggle pair — `archive`/`unarchive` both `"E"`, `pin`/`unpin` both `"P"`, `favorite`/`unfavorite` both `"F"`, `mark-read`/`mark-unread` both `"U"` — with no `is_toggle`/pairing metadata, so story 9.3's cheat sheet and native menu bar (which consume this same registry) will render each key as two ambiguous separate bindings.

origin: migrated from legacy ledger (spec-9-1-command-palette.md), 2026-07-06
location: keeper-core palette.rs (palette_actions)
reason: `palette.rs` `palette_actions()` declares the paired actions with duplicated `shortcut` values and no pairing field. This is fine for the 9-1 palette itself (context/state picks the relevant verb), but the epic-spine data model lacks the metadata 9.3 needs to collapse toggle pairs into one cheat-sheet row. Deferred to story 9.3, which owns the cheat-sheet/menu-bar generation and should add toggle-pairing metadata to the registry. See [[DW-87]], [[DW-89]].
status: done 2026-07-06
resolution: already resolved: palette.rs:356-373 PaletteActionVm.toggle_group field + toggle() helper set the group on both directions; :582-646 the derived cheat-sheet/native-menu builder collapses each toggle_group pair into one row via combined_toggle_title. Story 9.3 added the pairing metadata the entry required.

### DW-85: The registry ships 19 actions covering the primary MVP surfaces, but has no action for some secondary shipped surfaces (device verification, key backup, mute), and the "every shipped MVP surface has ≥1 action" acceptance is currently self-certified by a hand-picked id list in a test rather than derived from an actual surface inventory.

origin: migrated from legacy ledger (spec-9-1-command-palette.md), 2026-07-06
location: keeper-core palette.rs (action catalog + coverage test)
reason: `palette.rs` action catalog and its coverage test enumerate a fixed id set; epic-3 encryption surfaces (device verification, key backup) and the mute menu have no registered palette action, and their cold-open entry points are uncertain (opening a verification/backup flow out of context may not be well-defined — hence not force-added here, per the spec's Block-If caution against wiring actions to surfaces lacking a clean entry point). Story 9.3 owns the palette-parity release audit (FR-48 release gate); deferred there to enforce completeness via a derived parity test with proper entry points for the remaining surfaces.
status: done 2026-07-06
resolution: already resolved: palette.rs:531-548 mute-chat/mention-only-chat/unmute-chat actions added; :1047-1112 parity_every_mvp_surface_has_an_action_or_is_excluded test is now a derived surface-inventory gate (not a hand-picked id list). Device-verification/key-backup remain intentionally excluded with documented rationale, which the entry endorsed. Story 9.3 + 10.2.

### DW-86: The "modal depth ≤ 1 — opening ⌘K closes anything below it" invariant (Boundaries & Constraints + the fifth AC) is asserted in comments but never enforced — pressing ⌘K while a Search/Export/New-Chat/Add-Account/device-verification/key-backup dialog is open stacks the palette on top, mounting two modal overlays (two focus traps).

origin: migrated from legacy ledger (spec-9-1-command-palette.md), 2026-07-06
location: src/hooks/use-command-palette-shortcut.ts + src/components/layout/app-shell.tsx
reason: `src/hooks/use-command-palette-shortcut.ts` only calls `commandPaletteStore.getState().toggle()`, and `src/lib/stores/command-palette.ts`'s `open`/`toggle` merely flip `isOpen`; `src/components/layout/app-shell.tsx` mounts `<CommandPalette/>` as a plain sibling of `SearchOverlay`/`ExportDialog`/`NewChatDialog`/`DeviceVerificationDialog`/`KeyBackupDialog` with no cross-dialog coordination, so opening the palette closes nothing below it — directly violating the AC ("opening it does not stack on top of another dialog (modal depth ≤ 1)"). Missed by the first review pass. Not a trivial in-diff patch: a correct fix needs a dialog-precedence decision — whether ⌘K should force-close auto-opened security ceremonies (device verification / key backup, which `useVerification` auto-opens on an incoming request) or instead defer to them — rather than blindly closing every sibling. Fix: on palette open, close the user-utility dialog stores (search/export/new-chat/add-account) and resolve the security-ceremony precedence explicitly so modal depth is provably ≤ 1. See [[DW-90]].
status: open
decision: 2026-07-06 Coordinator, defer to ceremonies — Add a shared dialog-precedence coordinator: on palette (and cheat-sheet) open, close the user-utility dialogs (search/export/new-chat/add-account) but DEFER to auto-opened security ceremonies (device verification / key backup), so modal depth is provably <= 1 without interrupting a security flow. Resolves DW-90 too.

### DW-87: With a chat open, the palette renders BOTH directions of every toggle action simultaneously — "Archive Chat" and "Unarchive Chat", "Pin"/"Unpin", "Favorite"/"Unfavorite", "Mark as Read"/"Mark as Unread" — because action ranking carries no per-room state, so half the offered rows are always the wrong-direction (largely no-op) verb, each with an identical shortcut chip.

origin: migrated from legacy ledger (spec-9-1-command-palette.md), 2026-07-06
location: src-tauri/crates/keeper-core/src/palette.rs (query_actions) + src/components/command-palette/command-palette.tsx
reason: `src-tauri/crates/keeper-core/src/palette.rs` `query_actions` returns every `requires_open_chat` action when `open_chat` is set with no state filter, and the `PaletteEntry`/registry carry no archived/pinned/favourited/unread flag to disambiguate; `src/components/command-palette/command-palette.tsx` then renders them all. This is a distinct, observed runtime behavior from the earlier-deferred registry-pairing-metadata item, and it corrects that item's parenthetical assumption ("context/state picks the relevant verb for the 9-1 palette itself" — it does not). Consequences are bounded (the wrong-direction handler is largely idempotent — archiving an archived room, etc.), but the user sees two contradictory rows per chat. A correct fix needs per-room state in the palette index (or a resolve-and-collapse pass) together with the toggle-pairing metadata already slated for 9.3's parity work; deferred there rather than adding room-state to the index unattended in a follow-up review. See [[DW-84]], [[DW-89]].
status: open

### DW-88: The new global `⌥⌘↓`/`⌥⌘↑` unread-jump chord double-acts with the pre-existing unmodified-`ArrowUp`/`ArrowDown` handlers in the timeline and composer — pressing it while the timeline (a message selected) or the empty composer is focused fires BOTH the chat switch AND the local arrow action (timeline message-selection move / empty-composer edit-last), because those local handlers don't guard modifier keys.

origin: migrated from legacy ledger (spec-9-2-keyboard-navigation-and-quick-switcher.md), 2026-07-06
location: src/hooks/use-unread-jump.ts + conversation-pane.tsx:1319 + composer.tsx:733
reason: `useUnreadJump` (`src/hooks/use-unread-jump.ts`) is a `window` keydown listener that fires on `⌥⌘↑/↓`; `conversation-pane.tsx:1319` (`if (e.key === "ArrowUp" || e.key === "ArrowDown")`) and `composer.tsx:733` (empty-composer `ArrowUp` → `onEmptyArrowUp`) both handle the arrow with no `metaKey`/`altKey`/`ctrlKey` check, so the same physical `⌥⌘↑/↓` keystroke reaches both the window hook and the focused React `onKeyDown`. `preventDefault` in the hook does not stop the separate React handler (no `stopPropagation`). Both `ChatListPane` and `ConversationPane` mount simultaneously in inbox/archive. The spec's Design Note claims the chords "never collide with the timeline's own ↑/↓," but that reasoning covers only the sans-modifier LIST keys and assumes the timeline/composer handlers ignore modified arrows, which they don't. Real but bounded: the local side-effect lands on the conversation being navigated away from (the switch supersedes it), and the composer edit-last is discarded by the room switch. Not an in-diff patch under this story's constraints — the spec's Never section forbids modifying the epic-3 timeline/composer handlers, so the fix (a modifier guard on those handlers, or capture-phase `stopPropagation` in the new window hooks) is a focused cross-cutting coexistence decision. Deferred for that focused attention.
status: open
decision: 2026-07-06 Capture-phase stopPropagation in hook — Use capture-phase stopPropagation in the new unread-jump window hook so the modified-arrow keystroke never reaches the focused React handlers, leaving the epic-3 timeline/composer handlers untouched (respects the Never constraint).

### DW-89: The native-menu collapsed toggle items resolve their direction only from the inbox window (`roomsStore.rooms`), so for an open room not in that window — notably any archived room, or a palette-opened room outside the recency window — the menu always dispatches the canonical (positive) direction, making "Unarchive"/"Unpin"/"Unfavorite"/"Mark as Unread" unreachable from the menu for those rooms.

origin: migrated from legacy ledger (spec-9-3-cheat-sheet-and-native-menu-bar-from-the-action-registry.md), 2026-07-06
location: src/hooks/use-menu-actions.ts (resolveMenuActionId)
reason: `resolveMenuActionId` (`src/hooks/use-menu-actions.ts`) returns the canonical id when `roomsStore.rooms.find(...)` is `undefined` (room not in the inbox window). Archived rooms live in a separate `archiveRoomsStore`, so an open archived chat is never found and the menu re-dispatches `archive-chat` (idempotent no-op) instead of `unarchive-chat`. Bounded (no crash / no data loss — the wrong direction is idempotent) and rooted in the same missing generic per-room state already deferred for the palette's both-directions rendering (needs per-room toggle state in the Rust palette index for arbitrary rooms). A complete fix is out of scope for 9.3; the menu's toggle execution is a secondary convenience to the keyboard/palette path. See [[DW-84]], [[DW-87]].
status: open

### DW-90: Pressing ⌘? while the command palette (or another dialog) is already open stacks the cheat-sheet overlay on top rather than enforcing the story's "single modal overlay (depth <= 1)" intent — two focus-trapped overlays render simultaneously.

origin: migrated from legacy ledger (spec-9-3-cheat-sheet-and-native-menu-bar-from-the-action-registry.md), 2026-07-06
location: src/hooks/use-cheat-sheet-shortcut.ts + src/components/layout/app-shell.tsx
reason: `use-cheat-sheet-shortcut.ts` only toggles its own store and `src/components/layout/app-shell.tsx` mounts `<CheatSheetOverlay/>` as an uncoordinated sibling of `<CommandPalette/>`/`SearchOverlay`/`ExportDialog`/etc., so opening the cheat sheet closes nothing below it. This is the same uncoordinated-dialog architecture already flagged for ⌘K (the pre-existing "modal depth <= 1 unenforced" deferral from spec-9-1); the cheat sheet adds one more sibling. A correct fix is the shared dialog-precedence coordinator that deferral calls for (including auto-opened security ceremonies), not a per-hook close — out of scope here. See [[DW-86]].
status: open
decision: 2026-07-06 Route through shared coordinator — Route the cheat sheet through the shared dialog-precedence coordinator decided for DW-86 (close user-utility dialogs, resolve security-ceremony precedence) so opening Cmd-? keeps modal depth <= 1.

### DW-91: Notification new-vs-backlog classification uses a client-clock baseline against the server's origin_server_ts and tracks no read markers, so a long offline/sleep gap can replay a burst of notifications for messages already read on another device.

origin: migrated from legacy ledger (spec-10-1-native-notifications-from-the-sync-loop.md), 2026-07-06
location: keeper-core/src/notify.rs (should_notify)
reason: `keeper-core/src/notify.rs` captures `baseline_ms = now_ms()` once at handler registration and gates on `event_ts_ms >= baseline_ms` (`should_notify`); the handler is registered once per account lifetime (not per reconnect), so when the SyncService catches up after a long gap every missed event has `origin_server_ts >= baseline` and notifies. Two independent reviewers (Blind Hunter, Edge Case Hunter) flagged the clock-model + wake-storm. Story 10.1 documents the timestamp baseline as an accepted MVP tradeoff; a robust fix (read-marker/high-water-mark awareness, or gating on first-live-sync rather than a cross-clock timestamp compare) is design-level work that fits the notification-rules stories (10.2+).
status: open
decision: 2026-07-06 Read-marker awareness — Implement read-marker / high-water-mark awareness (or gate on first-live-sync rather than a cross-clock timestamp compare) so a long gap doesn't replay notifications for already-read messages.

### DW-92: Notifications fire even when the app is foreground-focused and the user is actively viewing the room the message arrived in, with no focus/active-room suppression.

origin: migrated from legacy ledger (spec-10-1-native-notifications-from-the-sync-loop.md), 2026-07-06
location: keeper-core/src/notify.rs (register_notify_handler / dispatch)
reason: `keeper-core/src/notify.rs` `register_notify_handler`/`dispatch` have no window-focus or active-room check; a user typing in a Chat gets an OS notification for the message that just appeared on screen. Not required by Story 10.1's ACs (which only require posting sender/Chat/preview, honoring the previews toggle, and no push egress), so it was not in scope, but it is standard notification behavior worth an explicit later decision alongside the mute/mention-only/DND rules (Story 10.2) or background/foreground semantics (Story 10.3).
status: open
decision: 2026-07-06 Add focus/active-room suppression — Wire a shell window-focus + active-room signal into dispatch and suppress notifications for the room the user is actively viewing while the app is focused.

### DW-93: Muting/unmuting a whole Network does not live-refresh the inbox row mute glyph — an idle room's glyph flips only when that room next produces a VectorDiff, so the bell-off glyph can lag the actual (immediately-applied) notification suppression for an arbitrary time.

origin: migrated from legacy ledger (spec-10-2-mutes-mention-only-and-do-not-disturb.md), 2026-07-06
location: keeper-core account.rs (network_mute_set) / inbox.rs (room_item_to_vm)
reason: `AccountManager::network_mute_set` (account.rs) persists to `muted_networks` and updates the in-memory `NotifyConfig`, so the notify handler suppresses immediately and correctly; but the row `mute_state` is recomputed only inside `room_item_to_vm` on the producer stream, and a per-Network mute is keeper-local so it emits no Matrix diff to trigger a recompute. Per-Chat mode changes self-heal faster (the push-rule write yields SDK account-data diffs); the per-Network case has no such trigger. A correct fix mirrors the pins path (`InboxMerger::update_pins` pokes a live re-emit of all windows) with an analogous "re-emit rooms for a changed muted-Network set" poke — new merger infrastructure, out of scope for this story. Notification behavior (the load-bearing guarantee) is already immediate and consistent; only the glyph freshness lags.
status: open

### DW-94: The per-Chat context-menu notification radio and the single-key `m` cycle read the combined row `mute_state`, which conflates a per-Network mute with a per-Chat push-rule mode — so on a network-muted Chat with no per-Chat rule the radio pre-selects "Mute" and `m` jumps straight to "All", silently skipping "Mentions only" and writing a per-Chat rule that cannot lift the Network-level mute.

origin: migrated from legacy ledger (spec-10-2-mutes-mention-only-and-do-not-disturb.md), 2026-07-06
location: src/components/.../chat-row.tsx + chat-list-pane.tsx (runVerb "m")
reason: `chat-row.tsx` derives the radio value from `room.muteState` and `chat-list-pane.tsx` `runVerb("m")` cycles on `room.muteState` (none→mention_only→mute→all); `mute_state` is `Muted` for BOTH `RoomNotificationMode::Mute` and a muted Network (`resolve_mute_state`). The per-Chat controls therefore misrepresent a Network-sourced mute as a Chat rule. A correct fix distinguishes the two sources in the controls (e.g. load the true per-Chat `chatNotifyModeGet` on menu open, or carry chat-rule mode separately from the network-derived glyph) — a small UX decision deferred rather than invented unattended, consistent with this story's deferral of the detail-panel controls. Glyph correctness (a muted row shows muted) is unaffected; only the per-Chat control's pre-selection/cycle on the network-muted edge is misleading.
status: open

### DW-95: Enabling menu-bar presence persists `system.menu_bar_presence=true` before building the tray, so a tray-build failure leaves the persisted setting (and the Settings switch on next open) claiming "on" while no tray icon exists — a silent, honesty-theme-violating divergence backed only by a `warn!` log.

origin: migrated from legacy ledger (spec-10-3-background-operation-and-honest-quit.md), 2026-07-06
location: keeper/src/ipc.rs (menu_bar_presence_set) + tray.rs (build_tray)
reason: `keeper/src/ipc.rs` `menu_bar_presence_set` persists via `state.accounts.menu_bar_presence_set` then calls `crate::tray::set_tray_presence`, and `tray.rs` `build_tray` returns `None` on `MenuItemBuilder`/tray build failure without signalling the caller; there is no rollback of the persisted flag. Tray build effectively never fails on macOS (so no AC is at risk), and a transient failure self-heals on the next launch's rebuild — but a permanent failure would leave setting and reality permanently disagreeing. A clean fix makes `set_tray_presence` fallible and either persists-after-apply or reverts the flag on failure; deviating from the established persist-then-apply pattern was out of scope for this story.
status: open

### DW-96: The honest-quit `shutdown_all()` runs per-account teardown sequentially inside a single 3-second `block_on` on the Tauri event-loop thread, so with multiple accounts a hung `sync.stop()` can freeze the quit up to the full 3s and starve later accounts of any teardown before the shared timeout fires.

origin: migrated from legacy ledger (spec-10-3-background-operation-and-honest-quit.md), 2026-07-06
location: keeper/src/lib.rs (RunEvent::ExitRequested) + keeper-core account.rs (shutdown_all)
reason: `keeper/src/lib.rs` `RunEvent::ExitRequested` wraps `state.accounts.shutdown_all()` in `tokio::time::timeout(3s, …)` under `tauri::async_runtime::block_on`; `account.rs` `shutdown_all` snapshots ids and awaits `shutdown(id)` in a plain sequential loop. The block is on the main/event-loop thread (safe — not a nested runtime — but UI is frozen during the window) and the budget is shared across all accounts. Harm is bounded and only at quit time (the process is exiting; abrupt teardown of a starved account is reclaimed by process death), so it is low-consequence, but `futures::join_all` under the shared bound plus a tuned timeout would be fairer. Out of scope for the story's ACs (which only require that sync fully stops and no background process survives).
status: open

### DW-97: The load-bearing shell-integration surface of this story — window-hide-on-`CloseRequested`, `RunEvent::ExitRequested` graceful quit, `RunEvent::Reopen` re-show, and tray create/destroy — has no automated coverage; only the pure `badge_count` arithmetic and registry round-trips are tested.

origin: migrated from legacy ledger (spec-10-3-background-operation-and-honest-quit.md), 2026-07-06
location: keeper/src/lib.rs + keeper/src/tray.rs
reason: New tests cover `badge.rs` (all three modes + zero-clears) and the `registry` round-trips, and `inbox.rs` gained a `BadgeRecordingPlatform` badge integration test, but `keeper/src/lib.rs` and `keeper/src/tray.rs` (the ⌘W/⌘Q/dock-reopen/tray behaviors) are validated only manually. This matches the project's existing boundary — shell glue from Stories 9.3 (`menu.rs`) and 9.4 (`hotkey.rs`) is likewise untested because it needs a running Tauri event loop — so it is a consistent, pre-existing coverage gap rather than a regression, but the headline user-facing behaviors of "background operation and honest quit" remain unguarded by CI. A future harness (or a `tauri::test` mock-runtime pass) exercising these RunEvents would close it.
status: open

### DW-98: `DockBadgeMode::from_registry_str` silently maps every unrecognized settings value (including one written by a future app version) to `All`, with no log — a lossy, unflagged downgrade of a forward-written badge preference.

origin: migrated from legacy ledger (spec-10-3-background-operation-and-honest-quit.md), 2026-07-06
location: keeper-core/src/vm.rs (DockBadgeMode::from_registry_str) / registry.rs (get_dock_badge_mode)
reason: `keeper-core/src/vm.rs` `DockBadgeMode::from_registry_str` returns `All` for any non-`{all,mentions,off}` string, and `registry::get_dock_badge_mode` folds absent/unknown into the `All` default; the next `set` overwrites the original value. This mirrors the established "absent → default" registry pattern (`get_notify_previews` etc.), so it is not a story regression and no current AC is affected, but a newer schema value (e.g. a future combined mode) would be silently coerced to "badge everything" rather than preserved or logged. A one-line `tracing::warn!` on the unrecognized value would make the coercion observable; forward-compat of settings was out of scope here.
status: open

### DW-99: Exact-message / exact-re-login deep landing on a notification click is deferred to a click-capable notifier backend (owner UNASSIGNED — the original 'Epic 11' pointer was wrong: Epic 11 is Packaging, Release & Quality Gates). Under the Option B MVP scope, a notification click summons+focuses the window and lands only on a coarse view (Message → Inbox, Bridge → Bridges) driven by the app-side "last notification target" recorded at dispatch; it never lands on the exact Chat/Account/message or auto-opens the specific Bridge's re-login sheet.

origin: migrated from legacy ledger (spec-10-4-click-through-and-bridge-health-alerts.md), 2026-07-06
location: keeper-core/src/notify.rs + keeper/src/ipc.rs (record_last_notify_target)
reason: The kept `tauri-plugin-notification` 2.3.3 desktop backend has NO per-notification click callback (its `show()` is a fire-and-forget wrapper over `notify_rust`; `action_type_id`/`onAction` are mobile-only), so per-notification target routing is impossible on this backend — confirmed during planning (spec Design Notes 1-3). The coordinator resolved this as Option B (2026-07-06): keep the backend, ship summon+focus + coarse landing now, defer exact landing. The full `NotifyTarget::Message { account_id, room_id, event_id }` / `NotifyTarget::Bridge { account_id, network_id }` payload already ships (attached at dispatch in `keeper-core/src/notify.rs`, recorded shell-side in `keeper/src/ipc.rs::record_last_notify_target`, emitted coarsely on `RunEvent::Reopen` via `emit_notify_navigate` → `notify://navigate`), and the frontend `bridgeRelinkStore` already carries the `(accountId, networkId)` target — so Epic 11 can consume it without a new contract. Fix in Epic 11 (where the signed .app bundle exists to validate ≥99% delivery reliability + click routing): swap the desktop notifier to a click-capable backend (`mac-notification-sys` `wait_for_click(true)` with a shell-side id→target map, or `UNUserNotificationCenter`), then route `NotifyTarget::Message` to `roomsStore.requestFocus(accountId, roomId, eventId)` (exact Chat/Account/message) and `NotifyTarget::Bridge` to auto-open that network's re-login sheet (`bridgeRelinkStore` → `BridgeLoginSheet`). See [[DW-100]].
status: open
decision: 2026-07-06 Swap to click-capable backend — Swap the desktop notifier to a click-capable backend (mac-notification-sys wait_for_click(true) with a shell-side id->target map, or UNUserNotificationCenter) and route NotifyTarget::Message to exact Chat/Account/message focus and NotifyTarget::Bridge to the specific network's re-login sheet.

### DW-100: The Option B coarse "last notification target" single-slot mechanism has three edge behaviors that only a click-capable backend (Epic 11) can properly fix — (a) any plain macOS dock-icon activation after an ignored notification navigates to the coarse view (stale-target over-navigation), (b) a bridge-drop target is overwritten by any later message notification's target (last-write-wins clobbers the higher-value bridge landing), and (c) a rare emit-before-subscribe race can drop the very first navigate if the webview listener has not mounted yet.

origin: migrated from legacy ledger (spec-10-4-click-through-and-bridge-health-alerts.md), 2026-07-06
location: keeper/src/lib.rs (emit_notify_navigate) + keeper/src/ipc.rs (record_last_notify_target)
reason: `keeper/src/lib.rs` fires `emit_notify_navigate` on EVERY `RunEvent::Reopen` (macOS gives no way to distinguish a notification click from a plain dock activation on the kept `tauri-plugin-notification` backend), and `keeper/src/ipc.rs::record_last_notify_target` unconditionally overwrites a single `LAST_NOTIFY_TARGET` slot (reset to `None` only after one consume, so the first post-notification activation — however delayed — still navigates, and a `Bridge` target is clobbered by subsequent `Message` posts). `emit_notify_navigate` is a one-shot emit+reset with no replay, so a navigate emitted before `use-notify-navigate`'s `listen` subscribes is lost. These are inherent to the frozen Option-B coarse scope (spec `[AMENDED-B]`: "coarse view landing driven by app-side last notification target… never exact-message routing"), NOT implementation bugs — the story faithfully realizes the accepted mechanism. Epic 11's click-capable backend (`mac-notification-sys` `wait_for_click(true)` / `UNUserNotificationCenter`) carries a per-notification payload and a real click signal, which structurally resolves all three: navigation fires only on an actual click (no stale/plain-activation over-navigation), each click routes its own target (no single-slot clobbering), and the click delivers the target directly (no emit-before-subscribe race). Track alongside the exact-landing entry above. See [[DW-99]].
status: open
decision: 2026-07-06 Adopt click-capable backend — Adopt DW-99's click-capable notifier backend, which structurally resolves all three edge behaviors: navigation fires only on a real click, each click routes its own target (no single-slot clobber), and the target is delivered directly (no emit-before-subscribe race).

### DW-101: The JS license firewall (`scripts/check-js-licenses.ts`) classifies any SPDX expression containing a copyleft token as `deny` ("deny wins"), so a legitimately dual-licensed `permissive OR copyleft` dependency (e.g. `MIT OR GPL-2.0`) hard-fails CI with no override mechanism; consider OR-aware classification (allow when any OR-branch is fully permissive) plus a reviewed per-package exceptions map (like cargo-deny's per-crate exceptions).

origin: migrated from legacy ledger (spec-11-1-signed-notarized-release-pipeline.md), 2026-07-06
location: scripts/check-js-licenses.ts (classifyLicense)
reason: `classifyLicense` in `scripts/check-js-licenses.ts` tokenizes on OR/AND and returns `deny` on the first copyleft token regardless of operator; the I/O matrix in `spec-11-1` (inside the frozen intent-contract) specifies `(MIT OR GPL-2.0-only)` → deny and the test encodes it. This is intentional and safe (never leaks copyleft), and no current dependency triggers it (0 denied across 527 packages), but it is over-conservative — a future dual-licensed transitive dep would red the build and the only in-place fix is hand-editing the classifier. Not fixed now because it contradicts the frozen I/O matrix; revisit as an enhancement with an override channel.
status: done 2026-07-06
resolution: closed by human decision: Keep the frozen deny-wins classifier; only add an override channel if and when a real dual-licensed dependency actually appears.
decision: 2026-07-06 Keep deny-wins — Keep the frozen deny-wins classifier; only add an override channel if and when a real dual-licensed dependency actually appears.

### DW-102: The JS license gate scans the installed `node_modules` tree rather than the resolved `bun.lock` set, so its coverage depends on install/hoisting state; consider driving the scan from `bun.lock` for fully reproducible, hoist-independent results.

origin: migrated from legacy ledger (spec-11-1-signed-notarized-release-pipeline.md), 2026-07-06
location: scripts/check-js-licenses.ts (scanInstalledPackages)
reason: `scanInstalledPackages` in `scripts/check-js-licenses.ts` walks `node_modules` (now including nested `**/node_modules` for completeness), so results vary with the physical install state (stale/dirty tree, partial install). CI mitigates this with a clean `bun install --frozen-lockfile` before the gate and `docs/release.md` names CI the source of truth, but a lockfile-driven scan would make local and CI runs identical regardless of hoisting. Low priority; current approach is correct under the CI clean-install invariant.
status: open

### DW-103: The new `scripts/*.ts` files (`check-js-licenses.ts` + its test) are outside all TypeScript typecheck coverage — root `tsconfig.json` has `include: ["src"]` and `typecheck` is `tsc --noEmit` (which does not build project references), so a type error in the security-relevant license classifier would pass `bun run check`/CI; the spec's Verification claims the script is "typed" but nothing typechecks it.

origin: migrated from legacy ledger (spec-11-1-signed-notarized-release-pipeline.md), 2026-07-06
location: tsconfig.json:29 + scripts/check-js-licenses.ts
reason: `tsconfig.json:29` is `"include": ["src"]`; `scripts/` did not exist at the story's baseline (`git ls-tree 159ed37 scripts/` is empty) and was introduced by this story, so no tsconfig ever covered it. The classifier uses Bun-only globals (`Bun.Glob`, `Bun.file`, `import.meta.main`) and no Bun types are installed (`@types/bun`/`bun-types` absent), so it cannot simply be added to the DOM-targeted frontend `include` without introducing `Cannot find name 'Bun'` errors — a proper fix needs a dedicated `tsconfig.scripts.json` (with `@types/bun`) wired into `check`, i.e. a new dev dependency + build-config change, not a trivial in-diff patch. Practical risk is low today: the classifier is covered by 22 vitest tests, biome lint (`biome check .` includes `scripts/`), and actual CI execution of `bun run check:licenses`, all of which would catch a broken script. Deferred rather than patched because the clean fix is a dependency/build-config decision beyond this follow-up review's scope; revisit by adding a Bun-typed typecheck pass over `scripts/` to `check`.
status: open

### DW-104: Nothing in the release pipeline verifies that the `TAURI_SIGNING_PRIVATE_KEY` secret actually corresponds to the committed `plugins.updater.pubkey`; a maintainer who provisions/rotates one but not the other ships a fully "signed" `latest.json` that every installed client silently rejects at signature verification (updates undelivered, only visible when a user clicks "Download and install").

origin: migrated from legacy ledger (spec-11-2-signed-auto-updates-and-egress-honesty.md), 2026-07-06
location: tauri.conf.json + .github/workflows/release.yml
reason: `tauri.conf.json` commits a build-valid scaffold pubkey that `docs/release.md` instructs the maintainer to replace, and `.github/workflows/release.yml` feeds `TAURI_SIGNING_PRIVATE_KEY`/`..._PASSWORD` from GitHub secrets, but no build- or release-time check binds the two. The endpoint-drift guard (`egress_update_endpoint_matches_tauri_conf`) pins only the update *URL*, not the far more security-relevant pubkey. This is inherent to the story's chosen scaffold + human-release-provisioning model (mirrors 11.1's Apple-secret model, which is frozen in the intent contract), so it is not a spec defect and not auto-patchable — a real fix is a release-process decision (e.g. a CI step that, after tauri-action signs, verifies the emitted `latest.json` signature against the committed `plugins.updater.pubkey` before the draft is considered good, failing the job on mismatch). The spec's manual release-time checklist already asks a prior-version app to verify against the committed pubkey, but that is post-hoc and manual. Revisit when the first real keypair is provisioned (Epic 11 release hardening).
status: open

### DW-105: The committed iOS `Podfile` declares a `target 'keeper_macOS'` that does not exist in the generated `.xcodeproj` (only `keeper_iOS` is defined), so `pod install` in `gen/apple/` would fail with "Unable to find a target named `keeper_macOS`".

origin: spec-12-1-ios-project-init-and-repo-integration.md, 2026-07-11
location: src-tauri/crates/keeper/gen/apple/Podfile (target 'keeper_macOS') vs project.yml (only keeper_iOS)
reason: Stock `tauri ios init` boilerplate — Tauri emits a two-target Podfile (iOS + macOS) but XcodeGen only builds the `keeper_iOS` target from `project.yml`. keeper currently uses no CocoaPods dependencies and the documented build/regeneration loop never invokes `pod install`, so there is no trigger today — impact is a latent trap only for a future developer who adds a pod and runs `pod install`. Both review reviewers (adversarial + edge-case) flagged it independently and confirmed it low-severity. Not fixed in 12.1 because it is generated boilerplate unrelated to init+repo-integration scope, and hand-editing the Podfile risks diverging from Tauri's expected scaffold; revisit if/when CocoaPods dependencies are introduced for iOS.
status: open

### DW-106: `rust-toolchain.toml` pins only `aarch64-apple-ios` and `aarch64-apple-ios-sim`; the iOS-Simulator `x86_64-apple-ios` target is not pinned, so a Simulator build on an Intel Mac would fail on a missing Rust target.

origin: spec-12-1-ios-project-init-and-repo-integration.md, 2026-07-11
location: rust-toolchain.toml (targets) + src-tauri/crates/keeper/gen/apple/project.yml (x86_64 Externals / EXCLUDED_ARCHS references)
reason: The generated `project.yml` references `x86_64` Externals paths and `EXCLUDED_ARCHS[sdk=iphoneos*] = x86_64`, but the pinned Rust targets are Apple-Silicon-only. On an Apple-Silicon dev box (the current environment) this is a non-issue; on an Intel Mac the Simulator core build would need `x86_64-apple-ios`. Out of scope for 12.1 (Simulator boot is 12.2's exit criterion, and CI runs on Apple-Silicon macos-latest), surfaced incidentally by adversarial review. Revisit alongside the 12.2 Simulator seam or 12.5 iOS CI if Intel-host support is required.
status: closed 2026-07-25
resolution: Won't fix. The premise holds (x86_64-apple-ios is unpinned) but acting on it contradicts the project's recorded Apple-Silicon-only posture; pinning an Intel Simulator target would advertise support that is deliberately not offered.### DW-107: The generated `ExportOptions.plist` hard-codes `method = debugging` (a development/debug export, not distribution) with no `teamID`/`signingStyle`.

origin: spec-12-1-ios-project-init-and-repo-integration.md, 2026-07-11
location: src-tauri/crates/keeper/gen/apple/ExportOptions.plist
reason: Correct and intentional for 12.1's unsigned, dev-only init scope (no signing secrets committed; device signing is via the `APPLE_DEVELOPMENT_TEAM` env var documented in docs/ios.md). It becomes relevant only for a later signed-distribution/TestFlight release story (Epic 12 device/release work), which must override the export method and inject the team at archive/export time rather than committing it. Deferred as a forward pointer for that release story, not a 12.1 defect.
status: closed 2026-07-25
resolution: Won't fix. The premise holds (ExportOptions.plist hard-codes method=debugging with no teamID) but a distribution export contradicts the recorded free-signing / no-TestFlight posture. Revisit only if paid distribution is adopted.
### DW-108: On the phone tier (<768px), the sidebar-hosted surfaces are unreachable and `PhoneShell` ignores `primaryView` — Settings, the account switcher, the offline pill (UX-DR18), and the Archive/Bridges/Approval view navigation have no phone affordance, and setting `primaryView` to `bridges`/`approval` (via ⌘3/⌘4 or a native menu) leaves the phone stack showing the chat list with no feedback.

origin: spec-13-1-phone-layout-tier-and-navigation-stack.md, 2026-07-11
location: src/components/layout/phone-shell.tsx (does not read usePrimaryView; renders ChatListPane only) + src/components/layout/app-shell.tsx (phone branch replaces SidebarPane with PhoneShell)
reason: Intended staging, not a 13.1 defect. Epic 13 sequences the sidebar's contents into the **leading drawer** delivered by Story 13.3 (the drawer renders "the entire desktop sidebar verbatim inside a leading Sheet" — primary views, SPACES/NETWORKS, account switcher, sync/offline status), and the Inbox header status cluster (13.3) surfaces the offline/approval state. On an actual iPhone at 13.1 these states are also not reachable: ⌘3/⌘4 need a hardware keyboard and the native menu bar is desktop-only (`nativeMenuBar` capability false on iOS), so `primaryView` never leaves `inbox`/`archive`. Both reviewers flagged the absence as "silently dropped functionality"; it is real but owned by 13.3. Revisit when implementing Story 13.3 (leading drawer + Inbox header status cluster) — ensure bridges/approval routing and offline/Settings/account access land there, and that the phone tier honors `primaryView` for the drawer-selected full-surface panes.
status: open

### DW-109: `detailStore.open` is never reset when the room selection changes or clears, so a room (re)selected while detail is open computes level 2 and lands directly on the Detail overlay, skipping the Room level.

origin: spec-13-1-phone-layout-tier-and-navigation-stack.md, 2026-07-11
location: src/components/layout/phone-shell.tsx:47 (level = detailOpen && selected ? 2 : ...) + src/lib/stores/rooms.ts (selectRoom/requestFocus never touch detailStore)
reason: The lifted `detailStore.open` is a global flag not scoped to the selected room (this mirrors the pre-existing desktop behavior, where detail-open also persisted across room switches). Through the 13.1 phone surface the bad state is not reachable: in-stack navigation changes rooms only via the back control (level 2 → closeDetail closes detail; level 1 → selectRoom(null) runs with detail already closed), and the deep-link primitive (`requestFocus`) is not yet wired to any phone trigger (notifications deferred; phone Search is Story 13.4). The only way to observe it today is a desktop→phone resize with the detail panel open, which lands on Detail showing the current room — acceptable. Deferred rather than patched because a correct reset needs a product decision on per-room detail semantics that Story 13.2 (which builds the identity-tap "push Detail" affordance, the phone's replacement for ⌘I) is the natural owner of; a phone-scoped effect that closes detail on `selected` change is the likely fix. Revisit with Story 13.2.
status: done 2026-07-25
resolution: Verified already fixed by Story 13.2 — phone-shell.tsx's header comment names this entry, and the selection path now resets detail. Closed by sweep triage, no code written.### DW-110: The phone navigation stack has no keyboard/assistive-tech focus management — pushing/popping a level moves no focus, the covered lower levels stay in the tab order and accessibility tree (not `inert`/`aria-hidden`), and the back control provides no focus-return or Escape-key handler.

origin: spec-13-1-phone-layout-tier-and-navigation-stack.md, 2026-07-11
location: src/components/layout/phone-shell.tsx (opaque absolute overlays without inert/aria-hidden; BackControl with no focus-return; no Escape handler)
reason: Story 13.1 ships a deliberately minimal, mouse/touch-accessible back control (documented in the PhoneShell header comment) and explicitly defers header chrome and focus rules to Story 13.2, whose acceptance criteria own exactly this behavior (UX-DR28: "every push moves focus to the new level's header, back button first in swipe order; every pop returns focus to the pushing element; the escape gesture triggers back at every level"). The reachable gap today is that a keyboard/AT user on a <768px webview can Tab "behind" the visible level into the covered Inbox, and Detail→Room back drops focus to `<body>` (a regression from the desktop `closeDetail` toggle-focus-return that this path replaces). Deferred to 13.2 rather than half-implemented here because 13.2 reworks this exact region (52px phone-header, chevron, transitions, edge-swipe) and will apply `inert`/`aria-hidden` to covered levels and full push/pop focus management as first-class ACs. Revisit with Story 13.2.
status: done 2026-07-25
resolution: Verified already fixed by Story 13.2 — the phone stack now manages focus, marks covered levels inert and handles Escape. Closed by sweep triage, no code written.
### DW-111: The `open-search` palette action, reachable from the phone Search "Actions" scope, routes to the desktop `searchStore`/`SearchOverlay` (a centered dialog) instead of the phone full-screen Search surface — running it on iPhone opens a desktop-style centered search dialog rather than the native phone surface.

origin: spec-13-4-merged-full-screen-search-surface.md, 2026-07-11
location: src/components/command-palette/actions.ts ("open-search": () => searchStore.getState().open("global")) + app-shell.tsx mounts <SearchOverlay/> in both tiers, so on phone the desktop dialog opens.
reason: Not a 13.4 defect and explicitly out of scope: Story 13.4's contract forbids new palette actions and changes to the desktop `searchStore` wiring, and reuses the shared action registry verbatim (so parity is structural). `open-search` is not a "dead" entry (the app-shell-mounted `SearchOverlay` does open), but on phone it presents a desktop centered dialog — a second visual language wart. Per-platform action routing/capability-gating is Story 13.7's domain (capability-gated surfaces + "On this iPhone"); the clean fix is to make `open-search` route to `searchSurfaceStore.open()` on the phone tier (or gate/reroute via capabilities). Revisit with Story 13.7.
status: open

### DW-112: The reused message-search offline header reads "Search works fully offline against your local archive on this Mac." verbatim on the iPhone Messages scope, where "on this Mac" is factually wrong device copy.

origin: spec-13-4-merged-full-screen-search-surface.md, 2026-07-11
location: src/components/search/search-panel.tsx (~line 266, hardcoded "on this Mac") — reused verbatim by src/components/layout/phone-search-surface.tsx (Messages scope).
reason: The shared `SearchPanel` was extracted verbatim from the desktop overlay to keep byte-for-byte desktop behavior; its honest-offline copy is Mac-specific. Making it platform-aware ("On this iPhone" / "on this device") is Story 13.7's capability-honest-disclosure domain ("On this iPhone" disclosure states foreground-only sync, no bbctl, etc., and owns the phone's platform-honest strings); changing it in 13.4 would alter shared desktop copy or introduce a platform check outside this story's scope. Low consequence (functionally correct; only the platform noun is wrong). Revisit with Story 13.7.
status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-13-5-safe-areas-and-the-keyboard-avoiding-composer.md`
  summary: On the phone timeline, the keyboard-resize bottom-pin ResizeObserver and the batch history-prepend layout effect both write scrollTop with no coordination, so a keyboard open/resize coinciding with an inbound history prepend can produce an occasional scroll jump.
  evidence: conversation-pane.tsx has two independent writers of `el.scrollTop = el.scrollHeight` (the new phone-only ResizeObserver re-pin and the pre-existing batch useLayoutEffect), each seeding "near bottom" from its own snapshot at a different instant; when both fire in the same frame with disagreeing seeds the anchor can jump. Narrow timing race, phone-only, best tuned against on-device WKWebView behavior (Epic 14/15 hardening), not reproducible in jsdom.

- source_spec: `_bmad-output/implementation-artifacts/spec-13-7-capability-gated-surfaces-and-on-this-iphone-disclosure.md`
  summary: The export dialog's "Reveal in Finder" affordance (toast action + inline button) is not gated on the `revealInFileManager` capability, so on iOS it renders a dead/error-on-tap affordance — exactly the capability-honesty gap Epic 13 exists to close.
  evidence: `src/components/export/export-dialog.tsx` calls `revealPath(...)` from a toast action and an inline "Reveal in Finder" button with no `useCapabilitiesStore((s) => s.capabilities.revealInFileManager)` guard; the capability exists (Story 5.5) precisely to gate this, but Story 13.7's AC enumerates only six surfaces (bbctl, Shortcuts, updater, launch-at-login/menu-bar, cheat sheet) and scopes reveal-in-file-manager out. Real and reachable on iOS (revealInFileManager is false there), but pre-existing (the un-gated button predates 13.7) and outside this story's AC — a small focused follow-up should gate it, aligning with the same predicate/idiom this story established.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-1-lifecycle-pause-resume-through-one-rust-entry.md`
  summary: On the iOS tier every `visibilitychange` to visible fires a full `AccountManager::sync_now()` across all accounts with no debounce/coalescing, so chatty machine-generated visibility edges (app-switcher peeks, control-center) can produce a burst of sync kicks.
  evidence: `use-app-lifecycle.ts` dispatches `appLifecycleChanged("foreground")` on every visible edge → `lifecycle.rs` → `account.rs::sync_now` locks the account set and calls idempotent `start()` on each; `start()` while Running is a no-op so it is correct but unthrottled, unlike the deliberate pull-to-refresh gesture it mirrors. A debounce or "only kick if we were actually paused" guard is battery/network hardening, outside Story 14.1's ACs which frame foreground resume as equivalent to pull-to-refresh.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-1-lifecycle-pause-resume-through-one-rust-entry.md`
  summary: `pause_all` and `sync_now` snapshot their `SyncService` set under the lock but call `stop()`/`start()` outside it, so a rapid background→foreground toggle can interleave and leave an account's final sync state not matching the final lifecycle phase.
  evidence: both `account.rs::pause_all` and `account.rs::sync_now` release the lock before awaiting `stop()`/`start()` (by design, so a slow call never blocks other account ops); nothing sequences a Background transition against a Foreground one. Correctness currently rests on matrix-sdk-ui's per-call `start`/`stop` idempotency and the OS-serialized nature of `visibilitychange`. A robust ordering guarantee (a generation token or a lifecycle mutex in `AppState`) is focused hardening touching the shared sync-kick path; the window is narrow (rapid toggle) and self-healing on the next transition / offline-mode auto-resume, so it is out of scope for 14.1.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-1-lifecycle-pause-resume-through-one-rust-entry.md`
  summary: `pause_all` awaits each account's `SyncService::stop()` sequentially, so on a multi-account phone total pause latency scales with account count and a slow drain could exceed the iOS background window, leaving later accounts' long-polls to die mid-flight — the ungraceful outcome pause exists to prevent.
  evidence: `account.rs::pause_all` mirrors `sync_now`'s `for (id, sync) in services { sync.stop().await }` sequential loop; `join_all`/`FuturesUnordered` (optionally with a per-account timeout) would drain concurrently under the hard OS deadline. Consistent with the codebase's existing sequential teardown loops (`sync_now`, `shutdown_all`) and their accepted-tradeoff deferrals; `stop()` aborts the long-poll task (returns promptly) so the hung-drain case is unlikely, making a concurrent-drain refactor focused hardening rather than a 14.1 AC.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-2-honest-no-background-sync-disclosure.md`
  summary: Gating the "Background & dock" Settings section off the reduced (iOS) tier removed the only badge-mode control (all / mentions-only / off) reachable on iOS, yet Story 14.2's new `BADGE_NOT_LIVE_SENTENCE` advertises the app-icon badge — so iOS users are told a badge exists with no way to set it to mentions-only or off.
  evidence: `settings-dialog.tsx` now renders `<BackgroundSection>` behind `!reducedPlatform`; that section held the `Dock badge` `DockBadgeMode` radio, whose value is the shared config the iOS app-icon badge (`keeper-core/src/badge.rs`, `ipc.rs` `dock_badge_mode_*`) will consume in Story 14.3. Today the iOS `set_badge_count` is a stub no-op so the control did nothing on iOS (no current functional regression), but Story 14.3 should re-surface an iOS-appropriate badge-mode control when it builds the badge so the disclosure copy and the available settings stay honest and consistent.

- source_spec: `_bmad-output/implementation-artifacts/spec-14-4-resume-integrity-blank-webview-guard-and-stale-resume-pill.md`
  summary: Nav-state restore does not validate that the stored room/account still exists (signed-out or room left/forgotten during suspension) before re-selecting it, so a reload can land on an empty/error Room pane instead of falling back to the Inbox.
  evidence: `use-nav-state-persistence.ts` calls `selectRoom({accountId, roomId})` from the restored `NavState` with no existence check; a correct fix must reconcile after the snapshot-then-diff mirror hydrates (the rooms store is empty at mount, so a naive check would defeat restore entirely). Real but needs focused design.
- source_spec: `_bmad-output/implementation-artifacts/spec-14-4-resume-integrity-blank-webview-guard-and-stale-resume-pill.md`
  summary: `app_lifecycle_changed` has no cross-report re-entrancy/serialization guard, so a late `Background` (`pause_all`) awaited concurrently with a newer `Foreground` (`sync_now`) can leave sync stopped while the app is foreground.
  evidence: Tauri command handlers run concurrently; the Foreground/Background branches were introduced in Story 14.1 (pre-existing), and 14.4 only adds the `paused_at` bookkeeping. Rare (visibilitychange reports are user-driven and seconds apart) but real; a generation/serialization guard would close it.
- source_spec: `_bmad-output/implementation-artifacts/spec-14-4-resume-integrity-blank-webview-guard-and-stale-resume-pill.md`
  summary: Under React StrictMode's dev-only double-mount, nav-restore's re-scheduled detail-reopen `setTimeout` can be dropped (run A schedules then cancels on cleanup; run B reuses the settled promise and never re-schedules), so a stored `detailOpen: true` restores the Room level but not the Detail level.
  evidence: `use-nav-state-persistence.ts` gates the detail reopen behind `restoreSettledRef` + a `cancelled` flag; run B sees `restoreSettledRef===true` and only calls `finishRestore()`. Production renders once so the path is unaffected — this is a dev-mode robustness gap; the detail-restore test does not exercise a StrictMode re-run.
- source_spec: `_bmad-output/implementation-artifacts/spec-14-4-resume-integrity-blank-webview-guard-and-stale-resume-pill.md`
  summary: The stale-resume "Connecting…" pill's `!offline` honesty gate interacts with the stale-resume `pause_all()` prelude (which drives every account to Offline), so on the exact overnight/stale-resume scenario the pill exists for it is hidden while offline and then cleared by the subsequent offline→online transition — the pill may essentially never render on its primary path. The code does not distinguish "offline because mid-restart-reconnect" (should show "Connecting…") from "offline and staying offline" (14.6's persistent pill, should not).
  evidence: `phone-shell.tsx` gates the pill `connecting && !refreshing && !offline && pullDy === null`; the stale-resume path in `lifecycle.rs` calls `pause_all()` → per-account `SyncService::stop()` → accounts transition to `ConnectionStatus::Offline` → `useShellOffline()` true → `!offline` false, and `use-stale-resume-pill.ts` settles (clears) on the offline→online transition back. Correct resolution needs the SM-8 on-device observation named in the spec's residual-risk #1 (does `pause_all()` emit an offline batch, and for how long), so a code fix cannot be derived blind — reversing the pass-2 `!offline` gate would reintroduce the dishonesty it was added to prevent (a "Connecting…" claim on a resume that stays offline). Also test-architecturally invisible: `phone-shell.test.tsx` mocks `useStaleResumePill` as a static boolean stub, so no test exercises the hook↔component interaction against a real `pause_all`-driven offline resume — the headline behavioral question falls in the seam between the two mocked-apart suites. Flagged by two independent adversarial reviewers as the top open concern; verify at SM-8 and reconcile the offline-but-reconnecting vs offline-and-staying distinction.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-5-memory-hygiene-under-jetsam.md`
  summary: SM-8 dogfooding bar — Instruments-on-Simulator confirmation that memory returns near baseline after backgrounding keeper with media loaded. The automated portion of Story 14.5 asserts only that the image-shed drop hook fires (image `src` dropped on background, restored on foreground) and that the 12.4 per-request Range cap holds; whether dropping the image `src` actually releases the WKWebView's decoded-image memory (and how close to baseline) is only observable on the Simulator with Instruments.
  evidence: The shed lives in the frontend (`src/lib/stores/lifecycle.ts` + `media-attachment.tsx`/`media-preview-overlay.tsx` drop the image `src` on `phase === "background"`), because the droppable decoded-image memory lives in the WKWebView renderer, which Rust cannot free. jsdom does not decode bitmaps or run WKWebView memory management, so the memory-return-to-baseline effect is not unit-testable; it is an SM-8 dogfooding item per Epic 14 (verification is Simulator-first, on-device soaks folded into SM-8, not story-blocking). Record findings. Disk-backed streaming (avoiding the whole-file in-RAM media load) stays deferred — see DW-29 (do not duplicate).
  status: open
- source_spec: `_bmad-output/implementation-artifacts/spec-14-5-memory-hygiene-under-jetsam.md`
  summary: SM-8 dogfooding bar — a 24 h suspended soak with a large account must survive without a jetsam kill. This is the phase's memory-hygiene reliability bar and cannot be a story-blocking automated gate; it is an on-device/Simulator dogfooding step with findings ledgered.
  evidence: Story 14.5 sheds the frontend image memory on background (see the sibling SM-8 entry) and preserves the Story 12.4 request-scoped Range cap, but whether a suspended session with a large account survives 24 h without iOS jetsam-killing it is a device-time behavioral bar, not a unit test. Per Epic 14 the overnight/24 h soak folds into SM-8 dogfooding rather than blocking the story. Run on-device with a large account, record survival/kill and any findings. Related deferred capacity work: DW-29 (disk-backed streaming) — cross-referenced, not duplicated.
  status: open
- source_spec: `_bmad-output/implementation-artifacts/spec-14-5-memory-hygiene-under-jetsam.md`
  summary: The Story 14.5 image-shed deliberately exempts `<audio>`/`<video>` playback surfaces (dropping their `src` would reset playback position, force a large re-download, and restart `autoPlay`), so a backgrounded open Quick-Look overlay showing an autoplaying `<video>` leaves its decoded media buffer un-shed — the single largest decoded-media holder is not released while that overlay is open in the background.
  evidence: `media-preview-overlay.tsx` gates only the `kind === "image"` full-res `src` on `useMediaShed()`; the video (`<video src={src} autoPlay>`) and audio branches keep their `src` across a shed cycle by design (playback-continuity), and `media-attachment.tsx` likewise never sheds the inline `<audio>`/video poster. In practice iOS suspends webview media playback on background (no background-audio entitlement), so the exempted buffer is a bounded, paused decode rather than a growing one, and the exemption is the correct trade-off for the common case (a background round-trip should not reset a voice note or video). But the residual is real and un-acknowledged elsewhere: quantify it during the SM-8 Instruments pass (background with a preview overlay open on a large video) and decide whether a "shed on true memory-warning even mid-playback" path (the deferred native `didReceiveMemoryWarning` seam, AD-30) is warranted. See the sibling SM-8 entries and DW-29.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-6-flaky-network-resilience.md`
  summary: SM-8 dogfooding bar — an airplane-mode toggle with keeper foreground must recover unaided: the persistent offline pill appears while disconnected and clears on reconnect, sync resumes with no app restart and no blank webview, and a message composed while disconnected sends on reconnect (no lost message).
  evidence: The automated suite covers the pill (phone-shell.test.tsx offline-pill precedence/clear-on-reconnect), the tier-gated queued caption (message-bubble.test.tsx), and outbox durability across an elapsed window (account.rs `outbox_row_elapsed_while_suspended_is_durable_and_due`), but real radio state transitions are Simulator-unverifiable — airplane mode is device-only behavior. Recovery rides the existing matrix-sdk offline mode + SDK-native backoff (Story 1.7) and the single 14.1 sync kick; no new machinery to verify, only the on-device end-to-end. Record findings.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-6-flaky-network-resilience.md`
  summary: SM-8 dogfooding bar — a real Wi-Fi↔cellular handover with keeper foreground must recover unaided (no restart, no lost message): sync re-establishes across the interface switch, any transient offline pill clears by itself, and a message composed mid-handover sends once the new path is up.
  evidence: An interface handover (different local addresses, momentary double-NAT/drop) is not reproducible in the Simulator or jsdom; the automated 14.6 suite proves the pill/caption/durability pieces but the radio-level reconnect is on-device only. Same recovery spine as the airplane-mode bar (matrix-sdk offline mode + reconnect supervisor, 14.1 sync kick, ~250 ms outbox scheduler tick). Record findings alongside the sibling 14.6 SM-8 entry.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-7-backup-exclusion-and-file-protection.md`
  summary: SM-8 device bars for Story 14.7 — on Simulator/device, read `NSURLIsExcludedFromBackupKey` back == true on the `data_dir` root and each `accounts/<ulid>/sdk` directory after login + archive creation; confirm `keeper.db`/`archive.db` and their `-wal`/`-shm` sidecars are backup-excluded via the directory-level flag; and confirm all stores stay accessible after first unlock under `NSFileProtectionCompleteUntilFirstUserAuthentication` (lock the device mid-sync — the resumed sync loop must keep working, never an inaccessible store).
  evidence: The host suite proves everything except the FFI syscall — the spy-`Platform` tests in `keeper-core/src/account.rs` pin the invocations (root at `AccountManager::new`, each sdk dir at every `activate`, idempotent re-flag, non-fatal on failure), and `crates/keeper/tests/entitlements_protection.rs` pins the protection class in both `project.yml` and the regenerated `keeper_iOS.entitlements`. But `IosPlatform::exclude_from_backup` is `#[cfg(target_os = "ios")]` (host tests never execute it; it compiles under the Story 12.5 iOS gate), and the actual subtree-covers-sidecars xattr behavior plus lock-screen store accessibility are only observable on device/Simulator. Read the key back with a scratch NSURL resource-values probe during SM-8 dogfooding and record findings.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-14-7-backup-exclusion-and-file-protection.md`
  summary: Pre-existing Story 14.3 iOS badge break — `IosPlatform::set_badge_count` calls `WebviewWindow::set_badge_count`, which is `#[cfg(desktop)]`-only in tauri 2.11.5, so `cargo check -p keeper --target aarch64-apple-ios` fails (E0599 at `crates/keeper/src/ipc.rs` `set_badge_count`) and the Story 12.5 iOS CI gate has been red on `main` since 14.3. Not Story 14.7's fix — flagged for the coordinator.
  evidence: Reproduced 2026-07-11 while iOS-compile-checking the 14.7 FFI — `cargo check -p keeper --target aarch64-apple-ios` errors with "no method named `set_badge_count` found for struct `tauri::WebviewWindow<R>`" at the iOS `set_badge_count` body; that error is the SOLE iOS-target failure (the new `exclude_from_backup` objc2 FFI compiles clean). tauri 2.11.5 gates `WebviewWindow::set_badge_count` behind `#[cfg(desktop)]`; on iOS the badge needs a different seam (e.g. `UIApplication.applicationIconBadgeNumber` / a mobile-capable plugin API). Fix belongs to a 14.3 follow-up: restore an iOS-buildable `set_badge_count` (honest no-op or a real mobile badge call) so the 12.5 gate goes green again.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-15-4-required-ios-ci-gate-and-release-hygiene.md`
  summary: Pre-existing required-check label drift in `docs/release.md` — four of the five "Required status checks (branch protection)" bullets ("License firewall", "Frontend", "Rust", "Tauri build") do NOT verbatim match their CI job `name:` values (`License firewall (cargo-deny, JS)`, `Frontend (lint, typecheck, test)`, `Rust (fmt, clippy, test)`, `Tauri build (macOS)`), so a repo admin copy-pasting the bolded labels into GitHub branch-protection settings could fail to bind those four required checks. Not caused by Story 15.4 (whose added `iOS (compile check)` row matches its job `name:` exactly); surfaced incidentally by 15.4's review.
  evidence: Confirmed 2026-07-11 by comparing `.github/workflows/ci.yml` `name:` values (lines 14/27/48/64/85) against `docs/release.md` required-check bullets (lines 186–190). Only the `iOS (compile check)` bullet matches its job `name:` string; the other four use shortened labels. GitHub branch protection keys required status checks on the exact check context (the job `name:`), so the shortened labels won't bind — the same "recorded name must equal job `name:`" rationale that Story 15.4 applied to the iOS row. Fix: align the four pre-existing bullet labels (or add the parenthetical suffixes) so every recorded required check matches its CI job `name:` verbatim. Pre-dates 15.4 (Epic 11 authored these rows).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-1-keeper-rec-swiftpm-scaffold-codesign-and-externalbin-wiring.md`
  summary: Validate on a real signed/notarized release that keeper-rec's hardened-runtime signature and entitlements survive tauri-action's bundle re-sign inside the notarized .app, and that the empty entitlements file suffices once real ScreenCaptureKit + system-audio capture lands (Story 16.6).
  evidence: The signed release path pre-signs keeper-rec (hardened runtime + entitlements) before tauri-action bundles/notarizes (tauri#11992 workaround); whether tauri's deep bundle-sign preserves that signature/entitlements can only be confirmed with real Developer ID secrets + hardware, which are outside this unattended loop. The empty <dict/> entitlements is correct for the getCapabilities stub but may need audio/capture entitlements under hardened runtime when 16.6 exercises live capture. The signed-path verify (codesign -dv) currently checks the standalone binary, not the copy inside the .app.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: `DesktopRecorder::run_session` treats a mid-stream stdout read error identically to a clean EOF, so a truncated/failed recording is reported as a clean end. Surface a `SidecarFailed` when the reader hits an I/O error (16.6 real-capture hardening).
  evidence: The reader task does `Err(_) => break`, closing the channel exactly like EOF; the consumer then falls through to a (now status-checked) `child.wait()` but a pipe/device error mid-capture with a still-success process status yields no `Failed`. Mirrors the bbctl precedent; only material once a long-running capture daemon streams for the session lifetime (16.6).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: `run_session` uses an unbounded channel and an uncapped `read_until`, giving no backpressure or per-line bound against a chatty or wedged `keeper-rec`; add a bounded channel and/or line-length cap (16.6).
  evidence: `tokio::sync::mpsc::unbounded_channel` + `read_until(b'\n', …)` with no length limit; a sidecar flooding stdout or writing a huge newline-less line grows memory without bound. Inherited from the bbctl pattern (whose output is trusted-short); a capture daemon is a higher-exposure surface.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: `child.wait()` after stdout EOF has no timeout, so a `keeper-rec` that closes stdout but hangs deadlocks `run_session`; add a `tokio::time::timeout` + kill fallback (16.6).
  evidence: After the read loop, `child.wait().await` is unbounded; a detached/hung child that reached stdout EOF never resolves the future. Mirrors bbctl (also un-timed); the stub sidecar exits promptly today, so the hang only manifests with the real capture daemon.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: Crash-recovery entry — `RecordingSession::apply` only accepts `Recovered` from `Stopping`, but a realistic self-salvage arrives from `Recording`/`Rotating`; model the crash-recovery transitions in Epic 17.
  evidence: `parse_event` maps `"state":"recovered"` to `RecordingEvent::Recovered`, yet `apply` rejects it outside `Stopping` (an `IllegalTransition` would fail a salvaged recording). The epic explicitly defers full crash-recovery entry semantics to Epic 17; 16.2 keeps `Recovered` as a reachable terminal only via a graceful stop.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: `SegmentClosed { index }` — the reported index is parsed but never validated against the internal counter (no gap/dup/monotonicity check), so `segments_closed()` can silently drift; enforce with Epic 17 segmentation.
  evidence: `apply` does `saturating_add(1)` on a counter and ignores the parsed `index`. A sidecar reporting out-of-order/duplicate indices would desync the count from reality — harmless in the 16.2 skeleton (no consumer) but load-bearing once a consumer enumerates segment files. Real segment-index semantics land with Epic 17.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: The sidecar-provided `Failed` message is copied verbatim into the error taxonomy with no length cap or path scrub, so the module's secret-free invariant depends entirely on keeper-rec; add a `cap_message`-style bound when real capture errors land (16.6).
  evidence: `parse_event` copies `message` verbatim into `RecordingEvent::Failed`; `error.rs` promises no message ever carries a captured-media path/token, but nothing in the platform-free core enforces it (bbctl caps its sidecar messages via `cap_message`). A keeper-rec bug could leak a capture path into logs/UI.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: `drive_session`'s `on_state` is a batched, post-resolution replay rather than a live feed; wire a channel-based live progress feed when a real consumer (progress UI) needs one (16.3+).
  evidence: The `Recorder::run_session` sink is `Box<dyn FnMut + Send>` (`'static`) and cannot borrow the non-`'static` `on_state`, so transitions are buffered and flushed when the run resolves. The live per-event feed is the sink itself; a live `on_state` needs a channel bridge, best added against a real progress-UI requirement in a later story.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: No single-session guard — two concurrent `run_session` calls on a `DesktopRecorder` would spawn two competing `keeper-rec` captures; enforce the "one capture target per session" invariant at the command/registry layer in 16.3+.
  evidence: The port is reentrant with no dedupe; the epic invariant "only one capture target per session" must be enforced where sessions are owned (a registry like `BbctlRunRegistry`), which does not exist until an IPC/command surface lands (16.3+).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: A non-zero `keeper-rec` exit that follows an already-reported terminal (`finalized`/`recovered`/`error`) masks that terminal — `drive_session` returns the generic `SidecarFailed` exit-status error instead of the honest finalized/failed outcome the session already reached. Reconcile once keeper-rec's exit-code contract is defined (16.4/16.6).
  evidence: `run_session` returns `Err(SidecarFailed)` on `!status.success()` unconditionally, and `drive_session` evaluates `run_result?` before consulting the buffered terminal/first-error; a sidecar that emits `{"event":"state","state":"finalized"}` then exits non-zero (or emits `{"event":"error",…}` → `Failed` then exits non-zero) has its terminal/message superseded by the generic exit-status text. No consumer until 16.3+, and keeper-rec's exit conventions are not fixed until the typed wire contract (16.4) / real capture (16.6); both follow-up reviewers flagged it independently. The exit-status check itself was a prior-pass patch, so this is the interaction it introduced at the seam 16.3+ depends on.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-2-recording-core-module-and-recorder-port.md`
  summary: The `DesktopRecorder::run_session` spawn/stream/reap body — including the prior-pass `kill_on_drop` and exit-status-inspection patches — has no automated test; only the pure state machine/parser and the no-sidecar `is_available()==false` path are covered. Add a fake-executable harness (a scripted binary emitting NDJSON on stdout) to exercise parse→forward→reap without hardware, ahead of the real-capture path (16.6).
  evidence: Every test in `recording.rs`/`recorder.rs` exercises either the platform-free machine or the `is_available()` short-circuit; the actual `tokio::process` spawn, byte-level line read, `parse_event` forwarding, `AbortOnDrop` teardown, and `child.wait()` status check are unexercised (the module's own test even notes `run_session` "is not spawned here"). Real `keeper-rec` capture is deferred to 16.6 (dev-signed hardware), but the stream/reap logic is testable now with a fake stdout-emitting executable and is the seam 16.3+ builds on.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-4-ndjson-rpc-handshake-getcapabilities-and-listsources.md`
  summary: The new `DesktopRecorder::request_response` round-trip has no read/round-trip timeout — a `keeper-rec` that spawns, reads the request line, then hangs (deadlock, wedged CoreGraphics, waiting on a dialog) leaves `get_capabilities()`/`list_sources()` pending forever, and with them 16.5's pre-flight. `kill_on_drop` only fires if the future is dropped, and nothing drops it.
  evidence: `request_response` loops on `reader.read_until(...).await` and then `child.wait().await` with no `tokio::time::timeout` anywhere. A bounded request/response call especially warrants a timeout (unlike the streaming `run_session`). This extends the 16.2-deferred `wait()`-timeout concern to the new request path; 16.6 owns the capture-session lifecycle timeout policy (start/stop/round-trip). Both review reviewers flagged it independently.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-4-ndjson-rpc-handshake-getcapabilities-and-listsources.md`
  summary: `request_response` reads response lines with `read_until(b'\n', &mut buf)` and no per-line length cap, so a buggy/replaced sidecar emitting a huge line before the correlated response accumulates it unbounded in memory.
  evidence: The read loop has no size limit; this extends the 16.2-deferred unbounded-line/backpressure concern to the request path. Low likelihood (first-party trusted sidecar) but an unbounded-allocation path driven by external process output; cap/skip-on-overflow belongs with 16.6's capture-path hardening.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-4-ndjson-rpc-handshake-getcapabilities-and-listsources.md`
  summary: `RecordingError::Protocol` and `response_result` interpolate the sidecar-supplied `error.code`/`error.message` (and serde's offending-value text) verbatim into error strings; the "never a secret" guarantee is documented host-side but is actually enforced sidecar-side.
  evidence: `response_result` embeds `error.code`/`error.message` from the sidecar and the parsers embed `{e}` from serde. Safe now (keeper-rec emits only benign strings), but once 16.6 flows real capture paths/device names an error message could carry a path into a `Protocol` string. Cap+scrub alongside the 16.2-deferred `Failed`-message sanitization when real capture errors land (16.6).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-4-ndjson-rpc-handshake-getcapabilities-and-listsources.md`
  summary: id-correlation uses fixed per-method ids (`1` for getCapabilities, `2` for listSources) with no method-echo verification; correct only because each call spawns a fresh process and issues exactly one request. 16.6's persistent multi-request capture session (start/stop/events over one long-lived process) will collide on fixed ids.
  evidence: `request_response` correlates purely on `response_id == id` with a constant id per method and never verifies the response answers the method asked. For 16.6's persistent session this needs monotonically-increasing request ids and response/method validation (defense-in-depth) to avoid mismatching a reply to the wrong outstanding request.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-16-5-screen-recording-permission-pre-flight.md`
  summary: The request/grant/relaunch UX rides `CGRequestScreenCaptureAccess`'s synchronous bool, which on a real first grant returns false while the OS prompt is still on screen (grant often not visible until relaunch on macOS 15+), so the row briefly reads "Denied — open System Settings" and the `requested+notDetermined → Denied` mapping labels an "awaiting/needs-relaunch" state with a hard "Denied" pill. Real grant/relaunch behavior and the final labeling need validation on a dev-signed Mac with a real grant.
  evidence: `request_screen_recording_permission` (ipc.rs) maps a not-granted request outcome straight to Denied via the pure resolver; `main.swift` calls `CGRequestScreenCaptureAccess()` whose sync return can't distinguish "prompt live/awaiting" from "denied". The relaunch note-line already discloses the quirk and re-detect on focus partly rescues it, but the epic explicitly defers real-grant validation to Story 16.6 (physical Mac + real grant + dev-signed build) — the honest awaiting-vs-denied labeling should be finalized there.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-1-dual-writer-gapless-size-based-rotation-in-keeper-rec.md`
  summary: A `stop` that lands during an in-flight segment rotation can suppress the just-closed middle segment's `segmentClosed` event, so Story 17.2's event-fed ledger would miss that (fully-written, on-disk, playable) segment. No consequence in 17.1 (no ledger yet).
  evidence: In keeper-rec `Capture.swift`, the retired writer's async `finishWriting` completion emits `segmentClosed` only `if !stopping`; suppression is required because `segmentClosed` is an illegal host transition once the machine is `Stopping`. If `stop()` sets `stopping` during writer A's finalize window, segment A gets no event. Story 17.2 should reconcile the ledger from a session-folder scan (the segment file exists), or drain any in-flight rotation before emitting `stopping`, so no closed segment is lost.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-2-session-folder-manifest-json-and-segment-ledger.md`
  summary: Story 17.3 startup recovery must run `SessionManifest::reconcile_from_dir` (rebuild the segment ledger from the on-disk `.mp4` files), not merely flip a stale `recording` manifest to `recovered` — a session that ended via a sidecar spawn fault / non-zero exit / crash never executed 17.2's terminal reconcile, so its manifest's `segments` list is incomplete or event-fed (possibly `bytes:0` / synthesized names).
  evidence: In `keeper/src/ipc.rs` `recording_start`, the terminal reconcile+write runs only inside the event sink on an applied terminal event (`Finalized`/`Recovered`/`Failed`); when `run_session` returns `Err` without a terminal event, the outer branch flips only the in-memory snapshot to `Failed` and never touches the manifest (it was moved into the sink closure). This is correct-by-design for 17.2 (crash recovery is explicitly 17.3's scope, and a stale `recording` manifest is the interrupted-session signal), but 17.3 must reconcile from disk when salvaging, since `reconcile_from_dir` already exists and produces the authoritative list.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-3-startup-recovery-of-orphaned-segments.md`
  summary: `recording_base_dir` (extracted in 17.3) eagerly evaluates its `platform.data_dir()?` fallback via `unwrap_or`, so a machine where `data_dir()` errors but `dirs::video_dir()` succeeds fails the base-dir derivation needlessly (affecting both `recording_start` and the startup recovery pass); switch to `unwrap_or_else`/lazy so the fallback is only hit when `video_dir()` is `None`.
  evidence: `Ok(dirs::video_dir().unwrap_or(platform.data_dir()?).join("keeper"))` — `unwrap_or` takes its argument by value, so `platform.data_dir()?` is always evaluated and can propagate an error even when `video_dir()` is `Some`. This is a faithful copy of the pre-existing inline shape in `recording_start` (not a 17.3 regression), but 17.3 promotes it to the single source of truth for the recordings base dir. Near-impossible on desktop (`data_dir()` rarely errors); low value, non-blocking.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-3-startup-recovery-of-orphaned-segments.md`
  summary: The pre-record recovery pass runs `recover_orphaned_sessions` synchronously on the record hot path, so record-start latency grows O(number of accumulated session folders) — a `~/Movies/keeper` with thousands of old sessions on a slow/network volume adds a `read_dir` + per-folder `stat`/manifest read to every Start; consider bounding the scan or moving it off the start path once dogfooding shows accumulation.
  evidence: In `recording_start`, `recover_orphaned_sessions(&base)` walks every immediate subdirectory of the base dir before the sidecar spawns. For N finalized sessions that is N `stat`/`read_to_string` calls on the interactive path (orphans are usually 0–1, so writes are rare). Acceptable today (sidecar spawn dominates; folders accumulate slowly), but the cost scales with lifetime session count on the one path where latency is user-visible.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-4-automated-gapless-concat-test-nfr-22.md`
  summary: On a REAL (non-fixture) capture session the final segment closes via `finalized` (which carries no bounds), so its `ptsStart`/`ptsEnd` are null and the NFR-22 concat gate reports `missingBounds` for that segment — the session's last boundary is unverifiable. Accepted for 17.4 (the CI gate runs on generated fixtures with complete bounds), but a future story that gates real signed-runner output must supply final-segment bounds.
  evidence: `keeper-rec/Capture.swift` emits `ptsStart`/`ptsEnd` only on `segmentClosed` (rotation), never on the clean-stop `finalized` path (Option B, coordinator-authorized; the 17.1 state machine was deliberately left untouched — `finalized` carrying segment fields would be a new illegal-transition surface). `gaplessConcatViolations` (`ConcatAssert.swift`) treats any null-bounds segment as a `missingBounds` violation. To gate real captures, either have the sidecar report the final segment's bounds at stop, or reconstruct them (they cannot be recovered from the rebased `.mp4`).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-4-automated-gapless-concat-test-nfr-22.md`
  summary: The Swift concat harness hand-maintains its own `manifest.json` decoder (`ManifestDocument` in `ConcatAssert.swift`) and fixture writer (`FixtureSegments.swift`) that must stay shape-compatible with the Rust `SessionManifest`/`SegmentEntry` serde, with no cross-language round-trip guard — a second unguarded language seam alongside `PROTOCOL_VERSION`. A future manifest-schema change in Rust could silently break the gate reading real signed-runner output.
  evidence: There is no test that decodes a Rust-serialized `SessionManifest` with the Swift `ManifestDocument`, nor one that feeds a Swift-written fixture manifest into the Rust `SessionManifest` deserializer. The Swift decoder only reads `segments[].{index,file,ptsStart,ptsEnd}` and ignores unknown fields, so a rename/retype of those fields on the Rust side would go undetected until a real capture is fed to the gate. Cheap mitigation: commit one Rust-generated manifest as a Swift-decoded fixture (or vice-versa) as a seam guard.
  status: open

- source_spec: `{project-root}/_bmad-output/implementation-artifacts/spec-17-5-segment-size-and-duration-cap-settings.md`
  summary: `recording_settings_set`'s "clamp, not reject" contract holds only for in-type integers — `durationCapMinutes > 65535` (u16) or `segmentMb > u32::MAX` from a non-UI IPC caller hard-fails serde deserialization with an opaque error instead of clamping.
  evidence: `RecordingSettingsVm` fields are `u32`/`u16` (vm.rs); the setter body clamps via the registry, but deserialization runs before the body, so an out-of-type value never reaches the clamp. The React control clamps before sending, so no user-facing impact today; the gap only bites a future programmatic/replayed caller. Fix by widening the wire type (e.g. accept i64 and clamp in the command) or documenting that out-of-type values are rejected.

- source_spec: `_bmad-output/implementation-artifacts/spec-19-1-application-window-picker-scshareablecontent-live-list.md`
  summary: App-scoped capture anchors the `SCContentFilter` to a single display, so an application whose windows span multiple displays records only the anchor display's windows — the rest are silently excluded and the inline app-scope disclosure ("only {App}'s windows … are recorded") does not mention the limitation.
  evidence: `keeper-rec/Capture.swift` resolves one `display` (the one hosting the app's frontmost on-screen window, fallback main/any) and builds `SCContentFilter(display:including:[app], exceptingWindows:[])`; a single-display filter cannot include the same app's windows living on other displays. Intent is exclusionary/no-leak (which holds — nothing extra enters the file), so this is a coverage gap, not a leak. Multi-display app capture is a substantial SCK feature (one filter is display-bound); fold the disclosure caveat and/or per-display fan-out into the on-hardware 20.6 verification of app-scoped output.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-19-1-application-window-picker-scshareablecontent-live-list.md`
  summary: An app-scoped session sizes the stream to the full anchor display (`config.width/height = CGDisplayPixelsWide/High`), so the output MP4 is full-display resolution with the app's windows composited on a black background rather than cropped to the app's content — larger files and unexpected framing for "capture the application".
  evidence: `keeper-rec/Capture.swift` `beginCapture` sets `pixelWidth/pixelHeight` from the display for both the display and application branches; `SCContentFilter(display:including:[app])` composites the app onto a display-sized surface. No leak (only the app's pixels are non-black), but the file is display-sized. Sizing the app-scoped stream to `filter.contentRect` (or the app's bounding rect) would crop to content; verify framing/size on hardware in 20.6.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-19-2-system-audio-toggle-and-per-app-audio-scoping.md`
  summary: The Source-picker's app-scope disclosure ("Only {App}'s windows and audio are recorded") is unconditional, so with an application target selected and the new system-audio toggle turned off, it contradicts the Audio card's honest "no content audio" off-note on the same screen.
  evidence: Story 19.2 introduces the first path that makes system audio OFF reachable; the 19.1 disclosure string in src/components/recording/recording-source-picker.tsx (~line 60, rendered ~241) has no awareness of systemAudioEnabled(). The started session correctly writes devices.systemAudio=false and no audio track, so the "and audio are recorded" clause is a stale false claim in exactly the off state. 19.2's read-only intent-contract explicitly scoped the source-picker disclosure out ("Never: No modification to 19.1's source-picker disclosure"), so the coherent fix — conditioning the "and audio" clause on the toggle — was forbidden this story. Recording behavior is correct; only the sibling copy is stale (medium). Natural home: Story 19.3 (which reworks the Audio card) or a focused copy pass.

- source_spec: `_bmad-output/implementation-artifacts/spec-19-3-microphone-picker-and-separate-track.md`
  summary: The macOS 13–14 parallel AVCaptureSession microphone path (new in Story 19.3) has robustness gaps that only manifest on 13–14 hardware — clock never reconciled with the video/SCStream anchor (A/V-sync drift), appendMicSample has no lower-bound PTS trim so a mic sample preceding a rotated segment's session start could fail writer B, an AVCaptureSessionRuntimeError after startRunning is not observed (silent mic track, no event), and startRunning/stopRunning race on unsynchronized global-queue tasks with micSession never nil'd.
  evidence: tools/keeper-rec/Sources/keeper-rec/Capture.swift startMicCaptureSession (~454-483), appendMicSample (~491-496, guards only sessionStarted + isReadyForMoreMediaData — no per-segment lower-bound PTS check), and stop() (~632-639). On the 15+ in-stream captureMicrophone path (dev host is macOS 26) the mic rides the shared SCStream clock so none of these apply; the whole 13–14 branch is compile-verified only and unreachable by the automated gates. Real, caused by this story, but graceful mic degradation + the loud warning surface is Story 19.4's chartered scope (Microphone Hot-Unplug Resilience, depends on 19.3 + Epic 18); on-hardware A/V-sync + rotation-with-mic verification is Story 20.6, consistent with every real-capture leg deferred since 16.6.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-19-3-microphone-picker-and-separate-track.md`
  summary: On the setup surface, when the selected mic device disappears from the enumerated microphones list before Start (a pre-Start unplug), the device Select renders a stale/empty value (no matching SelectItem) and Start still ships the dead device id to the sidecar.
  evidence: src/components/recording/recording-audio-controls.tsx renders `value={deviceId ?? MIC_DEFAULT_DEVICE_VALUE}` (~157) against `useRecordingSources()?.microphones` with no validity check, and the header Start reads `micDeviceId()` from the store unreconciled; on 15+ the sidecar silently falls back to default for a vanished microphoneCaptureDeviceID, on 13–14 AVCaptureDevice(uniqueID:) returns nil and the recording fails cleanly with an honest error. No leak or hang, but the picker shows a stale selection and the wire carries a dead id. Device churn / re-enumeration + selection reconciliation is Story 19.4's scope (Microphone Hot-Unplug Resilience).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-19-4-microphone-hot-unplug-resilience.md`
  summary: The macOS 15+ mic hot-unplug seam (Story 19.4) — in-stream `.microphone` + a parallel fallback `AVCaptureSession` both feeding one `micInput`, guarded only by the PTS-lower-bound trim — plus the silence-fill's clock/format assumptions are correct-by-construction but exercised only by the simulated signal, never by real overlapping hardware audio.
  evidence: Concrete on-hardware risks the automated gates (pure `MicHealth.decide`, Rust state-machine/parse tests, no-session smoke) cannot reach, all in `tools/keeper-rec/Sources/keeper-rec/Capture.swift`: (1) `activeMicDeviceId = micDeviceId ?? AVCaptureDevice.default(for:.audio)?.uniqueID` (~990) is a guess on 15+ where SCStream never reports its bound default — a wrong non-nil guess makes `MicHealth.decide` return `ignore` on a real unplug (silent gap, no warning) for the picked-default case; (2) `attemptMicFallback` (~632) double-feeds the mic track on 15+ (in-stream output stays attached), correctness resting entirely on the untested-against-real-streams trim (~1031); (3) `makeSilenceBuffer` hardcodes mono/16-bit/48 kHz LPCM (~1171) which may mismatch the mic input's stereo AAC config and fail the append (reintroducing the gap it prevents); (4) `appendSilenceChunk`'s host-clock-vs-sample-PTS assumption (~1150) and the `maxFill` clamp that drops a multi-second span down to one 250 ms tick (~1154) can each leave a real gap under clock skew or a stalled queue; (5) `micLost` clears on any single passing sample (~558) so a straggler can flap the silence-fill. All are the real-capture/A-V-sync leg the spec explicitly routes to Story 20.6 (SM-10 induced-failure matrix), consistent with every real-capture deferral since 16.6 — the dev host is macOS 26 so the 13–14 branch is compile-verified only.
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-18-5-disk-space-guard-warn-and-graceful-stop-and-finalize.md`
  summary: RecordingStatusVm.warning is a single Option<String>, so a 19.4 mic-unplug warning and the Story 18.5 low-disk warning overwrite each other last-write-wins when they co-occur, silently hiding one persistent warning the 19.4 model promised would stay until resolved.
  evidence: apply_disk_guard_action (keeper/src/ipc.rs) writes snapshot.warning directly while fold_recording_event writes it from sidecar Warning events; the two producers share one slot with no reconciliation (priority, merge, or separate slots). Surfaced by the 18.5 review (finding F2); out of scope for a disk-guard story since it requires reworking the pre-existing single-slot warning model.
- source_spec: `_bmad-output/implementation-artifacts/spec-20-1-optional-webcam-as-a-separate-synchronized-file.md`
  summary: A webcam device that is added successfully but never yields frames (camera held by another app, or TCC revoked between enable and start) produces an empty/dropped camera file with no `cameraLost` warning — the user gets no camera output and no explanation.
  evidence: `keeper-rec` `setupCamera` adds the input/output and `startRunning()` succeeds even when the device delivers no buffers; `handleCameraLost` fires only on an explicit disconnect/runtime-error/simulated signal, not on a no-frames-ever condition, so no warning is raised and `finishAndExit`/`finalizeCameraEarly` silently drop the frameless writer. There is no frame-arrival watchdog (the screen leg has none either).
- source_spec: `_bmad-output/implementation-artifacts/spec-20-1-optional-webcam-as-a-separate-synchronized-file.md`
  summary: A camera-setup failure at Start (the picked device is exclusively held by another app so `AVCaptureDeviceInput(device:)` throws, or the picked uniqueID vanished AND `AVCaptureDevice.default(for:.video)` is also nil) aborts the WHOLE recording session including screen+audio, so enabling the optional webcam can prevent the screen recording the user actually wanted — contrary to the camera-optional/non-fatal spirit (loss mid-recording is non-fatal, but a Start-time camera fault is loud-fatal).
  evidence: In `keeper-rec` `setupCamera`, a thrown error propagates through `beginCapture` into the single `error` event → `exit(0)`, tearing down screen+audio. This mirrors the mandated Story-19.3 mic precedent (setup throw at Start is fatal) and the spec's I/O matrix only guarantees non-abort for the uid-absent→default fallback, not for device-busy or no-camera-at-all — so it is spec-consistent-as-written, not a spec deviation. Surfaced by the follow-up 20.1 review; deferred to the SM-9/SM-10 real-hardware acceptance (Story 20.6) where camera-busy behavior can be exercised, rather than reworking the Start path unattended on a done story. Fix candidate: catch camera-only setup errors in `beginCapture` and route to `failCameraNonFatally` (screen continues screen-only) instead of propagating the throw.

- source_spec: `_bmad-output/implementation-artifacts/spec-20-2-microphone-and-camera-tcc-pre-flight-rows.md`
  summary: The recording Start gate is UI-only — `recording_start` re-enforces no screen/mic/camera TCC permission server-side, so a session reaching it with an enabled-but-ungranted source (permission revoked between the last pre-flight probe and the click, or a direct IPC invoke) still starts and silently produces a silent mic track / missing camera file.
  evidence: `src-tauri/crates/keeper/src/ipc.rs` `recording_start` gates only on `evaluate_destination`; `can_start`/`blockingPermissionName` live entirely in the frontend VM. Matches the pre-existing 16.5 UI-only screen gate. The induced permission-revoke-mid-record loud-failure with already-written segments intact is SM-10 hardware acceptance (Story 20.6, human-in-the-loop on a Development-signed Mac).

- source_spec: `_bmad-output/implementation-artifacts/spec-17-3-startup-recovery-of-orphaned-segments.md`
  summary: A session whose recovery salvage permanently fails (e.g. a wedged read-only session folder where `write()` never succeeds) stays at `status:"recording"`, so every startup and every pre-record scan re-loads, re-reconciles, and re-warns it forever — there is no give-up / poison-pill marker to stop re-processing a permanently-unsalvageable orphan.
  evidence: `recover_orphaned_sessions` leaves the manifest at `Recording` when `reconcile_from_dir`/`write` errors (correct for safety — the `recover_isolates_a_per_folder_failure` test exercises exactly this read-only-folder path). But nothing records that the folder was tried-and-failed, so on a volume with one wedged folder every `recording_start` re-pays the load+reconcile cost and emits a `tracing::warn` line indefinitely. Non-fatal and rare, but the "bounded, 0–1 orphans" cost note does not account for a permanently-failing orphan; a durable give-up marker (or an in-memory per-run skip set) would bound it.

- source_spec: `_bmad-output/implementation-artifacts/spec-17-1-dual-writer-gapless-size-based-rotation-in-keeper-rec.md`
  summary: Display sleep during capture suspends SCStream sample delivery; on wake the first frame fires the duration-cap rotation and the RETIRING writer's `finishWriting` fails with AVFoundationErrorDomain -11800, killing the whole session (observed live in the 20.5 soak, manifest `failed`, segment-0 left without its final moov). The engine should either pause/stop gracefully on display sleep (SCStreamDelegate / NSWorkspace sleep notifications) or tolerate a wake-side rotation after a long sample gap.
  evidence: Soak session `keeper-rec 2026-07-20 09.28.41`: screen-0000.mp4 mtime frozen at start, 2 moofs for a nominal 30 min, rotation fired at wake (t=30:00 cap), banner "segment finalize failed: ... -11800". Standalone 1-min-cap rotation with the display awake rotates cleanly (verified twice).
  status: open

- source_spec: `_bmad-output/implementation-artifacts/spec-17-1-dual-writer-gapless-size-based-rotation-in-keeper-rec.md`
  summary: A Stop landing during/just after an in-flight rotation reports a false "no frames were captured before stop" failure (the finalize guard reads rotation-freshened per-segment state), even though prior segments captured fine — a clean session gets a failed outcome and no completion card.
  evidence: Live repro 2026-07-20 ~10:05: 1-min-cap session, Stop clicked ~4 s after the segment-3 rotation began; banner showed "Recording failed — no frames were captured before stop" despite segments 1-2 on disk. The stop path should wait out `rotationInFlight` (or treat sessionStarted/segment state as session-scoped, not segment-scoped) before judging emptiness.
  status: open

### DW-113: Every published macOS release is silently unsigned, so an installed build is Gatekeeper-rejected and cannot read keychain items written by a signed predecessor.

origin: found live while shipping v0.4.2, 2026-07-26
location: .github/workflows/release.yml (Detect Apple signing secrets) + repository Actions secrets
reason: The workflow treats Apple signing as optional and falls back to an ad-hoc build with only a `::notice::`, which nobody reads on a green run. The repository holds only `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD` — every `APPLE_*` secret is absent — so v0.4.0/v0.4.1/v0.4.2 all published "(unsigned build)" artifacts while the release looked entirely successful. `spctl -a` rejects the result, so a fresh install needs the right-click→Open ceremony. The keychain leg is real but latent: macOS binds keychain ACLs to the signing identity, and an ad-hoc signature has no certificate — its identity is a cdhash that changes with every build — so an ad-hoc build cannot read credentials written by a signed one, and in that steady state every update would force a re-login. Fix: provision the `APPLE_*` secrets so the signed path runs, and make the unsigned fallback loud (fail a tagged release by default, opt in explicitly for a local/test build). The identity must then stay stable across releases — changing certificate costs one forced re-login.
status: open
correction: 2026-07-27 — this entry originally blamed the signing change for the accounts vanishing after installing v0.4.2 on hesperia. That diagnosis was WRONG and the evidence for it was circumstantial (a rollback to the signed 0.3.0 restored the accounts, which both explanations predict). The actual cause was a code regression, since fixed: `lib.rs` called Tauri's `invoke_handler` twice, and that method ASSIGNS (`self.invoke_handler = Box::new(handler)`), so the second registration — folder sync's desktop-only commands — discarded all 164 commands from the first. The desktop build had nine reachable commands and answered "Command <name> not found" to everything else, which is why the account never restored, the recording surface never appeared, and the Bridges pane showed a raw IPC error. What falsified the keychain story: `bridge_catalog` failed too, and it touches no credential. The keychain hazard above stands on its own and stays open; it simply was not what broke that install.
note: 2026-07-29 — the workflow half is fixed; the entry stays open because its `location` also
  names the repository's Actions secrets, and those are still absent. `release.yml` no longer has
  an unsigned path at all: a preflight step beside the tag-vs-version check requires all seven
  Apple secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
  `KEYCHAIN_PASSWORD`, `APPLE_API_ISSUER`, `APPLE_API_KEY_ID`, `APPLE_API_KEY_P8_BASE64`) and
  fails the job in seconds naming every one that is missing, and the ad-hoc build plus its
  `gh release create --title "keeper <tag> (unsigned build)"` step are deleted. So a release can
  no longer be green and unsigned — it is either signed and notarized or it is red. Two things
  the old gate got wrong are also closed: it accepted just the certificate and the `.p8`, so a
  repository without `APPLE_API_ISSUER` took the signed path and produced a signed-but-NOT-
  notarized build on a green run; and the ad-hoc path shipped signed updater artifacts, which
  would have delivered this entry's keychain harm on every auto-update rather than once.
  `keeper-syncd` binaries still publish when the app job fails (`needs: release` +
  `if: always()`). What remains is provisioning: until the seven secrets exist, tags produce a
  red app job and no macOS artifact. See
  `spec-dw-113-signed-releases-and-the-epic-34-ledger.md`, including how to verify the gate on a
  throwaway tag without burning a release.

### DW-114: Nothing anywhere constructs the filesystem watcher, so no host is event-driven — both the app and the daemon poll at 1 Hz. (The app-side half, never starting the supervisor at all, is fixed.)

origin: found while answering a user question about the sync surface, 2026-07-27
location: src-tauri/crates/keeper/src/sync.rs (engine built, never driven) + keeper-sync/src/watch.rs (no production constructor)
reason: `Engine::run` (keeper-sync/src/engine.rs:372) is the 1 Hz supervisor that drains the journal and rescans the tree. Its only caller in the repo is `run_supervisor` in the daemon (keeper-syncd/src/commands.rs:879); the `keeper` crate builds the engine as a lazy singleton (keeper/src/sync.rs:155-175) and never calls `run`, despite that module's own doc claiming it owns "the supervisor task" (sync.rs:5). Separately, `watch.rs` implements a complete per-profile watcher — `FolderWatcher` (:360), `EchoSuppressor` (:161), inotify close-write fast path, overflow rescan, poll fallback — and a repo-wide grep for `FolderWatcher|WatchConfig|EchoSuppressor` matches only that file and its own tests, so NO host is event-driven; even the daemon polls at 1 Hz. Consequences: in the app a profile added in Settings does nothing until "Sync now" is clicked, and no remote change ever arrives on its own; the `close_write` 1 s settle window (profile.rs:116) is dead code; Story 26-2 (watcher) and Epic 29 are marked done in sprint-status while the app-side behaviour they imply does not exist. Fix is a design decision, not a patch: either drive the engine from the app lifecycle (start on launch, pause on quit-to-tray, stop before teardown) or state plainly in the UI that in-app sync is manual and continuous sync requires keeper-syncd.
status: done 2026-07-29
note: 2026-07-27 (app half only, PR #7) — the app now starts the supervisor from `setup` and signals it on the quit path before account teardown, so a configured folder converges without a separate daemon; verified live on macOS by `sync supervisor started` in the app's own log with both accounts still syncing. What stays open is the watcher: `FolderWatcher`/`WatchConfig`/`EchoSuppressor` still have no production constructor anywhere, so nothing is event-driven, the Linux close-write fast path (profile.rs:116) remains dead code, and both hosts discover work only by rescanning on the tick.
resolution: watcher half fixed by story 34.9
  (`spec-34-9-files-sync-when-they-land.md`), which closes this entry.
  `Engine::ensure_watcher` arms one `watch::FolderWatcher` per enabled profile and
  `retain_watchers` re-establishes that on every tick, so "every enabled profile is watched"
  is a property re-derived continuously rather than three call sites that must remember. The
  lifetime lives in the engine, so both hosts get it and neither `keeper/src/sync.rs` nor
  `keeper-syncd/src/commands.rs` was touched. `FolderWatcher`, `WatchConfig` and
  `EchoSuppressor` therefore have production constructors for the first time.
  WHAT IS EVENT-DRIVEN NOW: a delivered batch sets a sticky wake that reopens the walk on the
  next 1 s tick instead of at `poll_interval_ms`, and a delivered close-write reaches
  `StabilityGate::note_close_write`, so the 1 s `CLOSE_WRITE_SETTLE_MS` path is reachable
  rather than the dead code this entry named. A new `StabilityGate::next_stable_ms` also
  reopens the walk when a held path's own window elapses, so the second observation no longer
  waits for a poll.
  WHAT IS NOT: close-write is an inotify concept, so that fast path is Linux-only — on macOS
  the FSEvents backend has none and the bound is the settle window plus a tick, roughly 6-7 s,
  against two 15 s poll intervals before. `git status` remains the sole authority on WHAT
  changed; events only say when to look. The paced scan is unchanged and still evaluated
  first and unconditionally on every tick, so nothing is watcher-only, and watcher failure
  degrades to it loudly (a sticky warning naming the cadence and the reason) — deliberately
  with no `notify::PollWatcher` fallback, which re-stats the whole tree every 30 s and is
  therefore slower than the backstop it would replace. No `EchoSuppressor` is registered;
  `fold_watch_events` records why. Teardown is deliberately NOT waited on: `watch::retire`
  (watch.rs:578) hands the backend to an unjoined `keeper-sync-retire` thread because
  `notify-8.2.0`'s FSEvents drop spins on `CFRunLoopIsWaiting` with no deadline, which had put
  an unbounded wait on the supervisor tick, on `stop_all_watchers` and on
  `sync_profile_remove` → `drop_watcher` on a tokio worker; `FolderWatcher::stop` no longer
  promises that no further event can arrive. The daemon-wide
  `DaemonSettings.poll_interval_ms` is still unconsumed (DW-116), and the 500-file-drop
  latency acceptance is a machine check on hesperia that has not been run — see that spec's
  Verification section.

### DW-115: The "Check files" action and its IPC doc promise verification against recorded digests, but no digest is ever recorded and the one it computes is thrown away.

origin: found while answering a user question about integrity, 2026-07-27
location: keeper-sync/src/engine.rs:1591-1660 (Engine::verify) + db.rs file_state schema + src/components/settings/sync-section.tsx:82
reason: `Engine::verify` walks the worktree and, for a plain file, calls `stability::verify_while_reading(&path)` keeping only the `Err` arm (engine.rs:1638) — the `(sha256, bytes)` it returns is discarded. That function's only failure mode is the fstat tuple changing across the read (stability.rs:640), i.e. a torn read, not a content mismatch. There is nothing to compare against: `file_state` stores `size, mtime_ns, ctime_ns, inode, first_seen_ms, last_change_ms, close_write` and has NO digest column (db.rs:82-93). For LFS pointers the check is `store.contains(oid, size)` — existence and length only, no re-hash (engine.rs:1626-1634). So the operation honestly means "every file is readable, is not changing under us, and every LFS object is present at the right length", which is useful but is not what two surfaces claim: `SYNC_VERIFY_CLEAN_SENTENCE = "Every file matched its recorded digest."` (sync-section.tsx:82) and "Re-verify a profile's stored content against its recorded digests" (sync_ipc.rs:423). Either record digests in `file_state` and compare them, or change both strings to describe the real check. The copy is the cheaper half and should not wait — this project's whole posture is that a surface never claims more than it did.
status: done 2026-07-27
resolution: fixed in PR #7. Neither surface claims a digest comparison any more: `SYNC_VERIFY_CLEAN_SENTENCE` now reads "Every file read cleanly, and every large file's stored copy is present." and the `sync_verify` doc describes the real check (a read that fails only on a torn read, plus LFS object presence at the recorded size). The underlying question — whether keeper should record per-file digests so a real content check becomes possible — is a separate design decision and is NOT implied by this fix.

### DW-116: `pollIntervalMs` is a fully plumbed dead knob at both the profile and daemon level — parsed, defaulted, floor-validated, CLI-settable, documented, and read by no timer.

origin: found while answering a user question about sync frequency, 2026-07-27
location: keeper-sync/src/profile.rs:159 + keeper-syncd/src/config.rs:59 + commands.rs:323
reason: `SyncProfile.poll_interval_ms` (profile.rs:159, default 15 s at :122) and `DaemonSettings.poll_interval_ms` (config.rs:59, floor-checked at :131-136) are both round-tripped through TOML and settable via `keeper-syncd add --poll-interval-ms` (commands.rs:323, stored :638-639), and the starter config documents them (config.rs:292, :322). Nothing consumes either: the supervisor's cadence is the hardcoded `TICK_MS = 1_000` (engine.rs:73, used at :373) and `main.rs:61` reads only `daemon.log_level`. An operator who sets the value gets no error and no effect, which is worse than not offering it. Either honour it (per-profile due time in the tick) or delete both fields and the CLI flag.
status: done 2026-07-28
resolution: honoured, per the entry's first option. `Engine::scan_is_due`
  (engine.rs) gives every profile its own due time from `poll_interval_ms`,
  floored at 2 s so a stored zero cannot mean "every tick". Draining the
  journal still runs at `TICK_MS`, so queued work is not delayed; only the
  tree walk that DISCOVERS work is paced. A new profile scans on sight and
  resuming a paused one reopens the window. This was not cosmetic: rescanning
  every folder every second published a `Scanning` phase at 1 Hz, which made
  the menu-bar glyph strobe on an idle folder. The daemon-level
  `DaemonSettings.poll_interval_ms` remains unconsumed — the daemon drives the
  same engine, so its profiles are paced by their own per-profile value, but
  the daemon-wide default still reaches no timer.

### DW-117: Every commit keeper makes is stamped `Keeper-Source: watch`, including manual and CLI syncs, so the trailer cannot distinguish what triggered a change.

origin: found while answering a user question about commit provenance, 2026-07-27
location: keeper-sync/src/engine.rs:889 and :1240
reason: `SyncSource` has four variants (provenance.rs:22-30) and the trigger is threaded correctly as far as `sync_once` — the app passes `SyncSource::Manual` (sync_ipc.rs:411) and the daemon passes `SyncSource::Cli` (commands.rs:807) — but both commit sites construct the provenance with a literal `SyncSource::Watch`, discarding the argument. `docs/sync.md:310` documents the field as `watch|manual|cli|bot`, and AD-50's bot-lane audit story leans on it, so the trailer is currently misleading rather than merely incomplete. One-line fix at each site once the value is threaded through; worth a test that a manual sync's commit carries `manual`.
status: done 2026-07-29
resolution: fixed by story 34.10
  (`spec-34-10-sync-now-proves-it-ran.md`). `SyncSource` is now an explicit parameter threaded
  down the one chain that reaches a commit — `drain_journal` → `execute` →
  `do_pull`/`do_push` → `commit_local` → `commit` — so both `Provenance::new` sites use the
  caller's value instead of the literal `SyncSource::Watch` this entry named. `tick_profile` is
  the only caller that passes `Watch`, and `sync_once` forwards its own, which is how the app's
  `Manual` and the daemon's `Cli` finally reach the trailer; a merge commit from `do_pull`
  carries the caller's source too, not `watch`. The fourth variant is derived rather than
  passed: new `Engine::commit_source` (engine.rs:2134) maps `Watch` on a `SyncLane::Worktree`
  profile to `Bot` per AD-50, while a `Manual` pass on the same lane still says `manual`
  because a person really did ask. `SyncSource::Bot` was previously unreachable — no caller
  ever passed it. Tests read the trailer back out of the worktree's HEAD for a manual, a CLI
  and a supervisor commit, plus the lane rule, so `docs/sync.md`'s documented
  `watch|manual|cli|bot` is now true rather than aspirational.

### DW-118: On macOS the web viewport sits 56 px below the window top while still sized to the window's full height, so `100vh` overshoots the visible area and everything anchored to the bottom is pushed out of view.

origin: spec-34-2-window-chrome-and-reachable-drawer-bottom.md (story 34.2), 2026-07-29
location: src-tauri/crates/keeper/tauri.conf.json:26-27 (`titleBarStyle: "Overlay"`, `hiddenTitle: true`) + the window's `WKWebView`, which no keeper code reaches today + docs/constraints-and-limitations.md:52-59 (the `unsafe_code` policy that blocks the remedy)
reason: Measured on hesperia (macOS 26.5, keeper 0.6.2) out of the accessibility tree with the
  window pinned to (200, 200) 1280x800: the window reports `(200, 200) 1280x800`, the close
  button `(208, 208) 16x16`, and `HTML content` `(200, 256) 1280x800` — the viewport starts 56 px
  down AND is a full 800 tall, so its bottom 56 px is off-screen. `Add account` reports
  `(208, 1012) 243x36` against a window bottom of y=1000: unreachable, with no scrollbar,
  because `mt-auto` is doing its job in a viewport whose bottom does not exist. Story 34.2
  chased this to WebKit's automatic top obscured content inset, against pinned tao/wry/WebKit
  sources: every macOS `WKWebView` opts in at init (`WKWebView.mm:690`) and wry 0.55.1 never
  opts out (a grep for `obscuredContentInsets|automaticallyAdjustsContentInsets|safeArea|
  contentLayoutRect` returns nothing), and the value is computed in
  `PageClientImplMac.mm:1115-1127` behind `(styleMask & FullSizeContentView) && ![window
  titlebarAppearsTransparent] && ![view enclosingScrollView]`. Apple's own commit for that code
  (`291818@main`, bug 289350) describes both the normal case ("this inset ends up being the
  height of the window's titlebar") and this failure ("the inset update occurs with incomplete
  constraints... AppKit does not report the change via KVO, leaving WKWebView stuck in state.
  This lack of update is a clear AppKit bug") — i.e. the inset is computed while the titlebar is
  still opaque and never recomputed. That mechanism is the only one whose signature matches the
  measurement: an obscured inset moves the rendered page and the `AXWebArea` while leaving the
  layout viewport at the view's full height, which is exactly why `100vh` stays 800 and why no
  CSS unit can see the problem. `Overlay` is already the safest of the three settings — the
  truth table in that spec's Design Notes has `Visible` (= dropping the key) APPLYING the inset
  and `Transparent` suppressing it only by painting a titlebar strip in a window background
  colour Tauri cannot set — so `tauri.conf.json` was left untouched. Story 34.2 returned the
  12 px duplicate inset and made the remaining 28 px band honest; these 56 px are diagnosed and
  attributed, not fixed. Do not "fix" it by subtracting 56 px in CSS: a hard-coded
  native-chrome constant is how this returns on the next macOS release.
status: open
blocked: 2026-07-29 — needs an authorised `unsafe` exception, which is not this story's to
  grant. The reliable remedy is to disable the automatic inset on the window's own `WKWebView`
  through `with_webview`, and the public `obscuredContentInsets` setter cannot do it (macOS
  26.0+ only, against a floor of 11.0, and it early-returns when the new value equals the
  current one, so assigning zero-while-zero is a no-op) — leaving the SPI
  `_setAutomaticallyAdjustsContentInsets:NO` (`WKWebViewPrivate.h:874`). That is a second objc
  FFI site, and the workspace declares `unsafe_code = "deny"` with exactly one audited
  function-level exception (`IosPlatform::exclude_from_backup`) under the dated coordinator
  policy in docs/constraints-and-limitations.md:52-59. THE DECISION THAT IS BLOCKED: whether to
  amend that policy for a second `#[allow(unsafe_code)]` site in the `keeper` shell crate.
  Either answer needs a runtime discriminator logged first — `[window
  titlebarAppearsTransparent]`, `[window contentLayoutRect]`, `[webView
  _obscuredContentInsets]` and the `WKWebView`'s frame in contentView coordinates — because it
  separates a wrong `NSView` frame (a wry bug to file) from a stuck obscured inset (the
  WebKit/AppKit bug above). The fallbacks, in order, are: file the wry issue with those four
  values and the AX numbers; or accept the 56 px, which costs nothing else — the drag band is
  unaffected either way.

### DW-119: `target`, `dist` and `build` are deliberately NOT tier-0 built-in excludes — the list is short on purpose, and "completing" it later would be silent data loss.

origin: spec-34-9-files-sync-when-they-land.md (story 34.9), 2026-07-29
location: src-tauri/crates/keeper-sync/src/exclude.rs:104-127 (`BUILTIN_EXCLUDES`, with the reasoning recorded in the corpus itself) + the pinning test `ordinary_english_build_directory_names_are_not_tier_zero` (exclude.rs:321)
reason: The epic named eight directory conventions for tier 0; five shipped (`node_modules`,
  `__pycache__`, `.venv`, `.next`, `.cache`, each with a name rule and a `**/name/**` subtree
  rule). Tier 0 is unconditional and invisible — an excluded path is never staged, never queued,
  never counted and never reported as pending — so a wrong entry there is silent, permanent data
  loss from the user's point of view, with no pending row to reveal it. Every entry that does
  belong is either a machine-generated name with a distinctive shape (`*.crdownload`, `~$*`,
  `.~lock.*#`) or a reserved-looking OS metadata directory (`.DS_Store`, `.Spotlight-V100`); the
  five that shipped fit that shape and three of them are dotfiles besides. `target`, `dist` and
  `build` do not: they are ordinary English words, they are not hidden, and their build-output
  meaning is CONTEXTUAL — only a `target` beside a `Cargo.toml` is Rust's, only a `build` beside
  a `CMakeLists.txt` is CMake's — while this tier matches names with one compiled glob set and
  never touches the filesystem, which is exactly what makes it cheap. `.gitignore` already
  handles them, is honoured by the same status walk the commit path uses, and is readable and
  editable by the user. So tier 0 would buy almost nothing for real projects and would cost a
  woodworking archive its `build/` folder. Checkable: the test at exclude.rs:321 asserts
  `build/bench-plan.pdf`, `target/q3-audience.numbers`, `dist/press-kit.zip` and two nested
  cases stay visible, and that none of the three names is in `BUILTIN_EXCLUDES`; and
  `extra_profile_patterns_apply_with_gitignore_anchoring` already uses `build/**` as a USER
  pattern and asserts `crate/build/out.o` still syncs, so adding them to tier 0 would have
  broken an existing test.
status: closed
resolution: wont-fix by design (story 34.9, 2026-07-29). This is a decision, not a gap: the
  three names are refused, not deferred, and the argument is recorded both in the corpus comment
  and in a test that fails if someone adds them. A user who does want them gone says so per
  profile through `SyncProfile.excludes`, which is visible and reversible. Revisit only if tier 0
  ever gains filesystem context (a `Cargo.toml`/`CMakeLists.txt` sibling check), which would
  make it a different mechanism with a different cost.

### DW-120: `gix-ref`'s failed-lock error path can loop forever, which is what the 96-minute CI hang on the NFR-8 durability sweep was; the trigger is fixed in keeper, the upstream loop is not.

origin: CI run 30417789975 on commit 3523a25 (job `Rust (fmt, clippy, test)`, macos-latest); recorded in spec-34-9-files-sync-when-they-land.md's follow-up and diagnosed by story 34.11, 2026-07-29
location: upstream `gix-ref-0.66.0/src/store/file/transaction/prepare.rs:398-410` (gix 0.86.0, both pinned in src-tauri/Cargo.lock) + src-tauri/crates/keeper-sync/src/git/repo.rs (where story 34.11 removes the trigger) + src-tauri/.config/nextest.toml (the guard)
reason: 1525 of 1526 tests finished inside two minutes; the run then sat on
  `keeper-syncd::durability_matrix a_kill_at_any_instant_costs_no_data_and_corrupts_no_index`
  for ninety-six minutes, emitting `SLOW [>N]` once a minute and nothing else, until a human
  cancelled it. The job's `Cleaning up orphan processes` step named a live `keeper-syncd`, a live
  `durability_matr` and a live `cargo-nextest`, so a daemon child outlived the run. The same test
  takes 12.05-21.54 s on that runner across the three preceding green runs.
  RULED OUT, with evidence: (a) not story 34.9's watcher wiring, the obvious suspect — the
  durability matrix only ever runs `keeper-syncd sync --once` (durability_matrix.rs:130 and
  :237), and that path returns from `cmd_sync` before `run_supervisor`, so it never enters
  `Engine::run`, never calls `stop_all_watchers` and constructs no `FolderWatcher` at all;
  confirmed empirically by running the daemon under `RUST_LOG=debug`, where a `sync --once` over
  a registered profile emits zero `folder watch armed` lines. (b) Not reproducible on Linux — the
  sweep was re-run 48 times (~336 daemon invocations) under 2x CPU oversubscription on 3 cores
  and stayed clean, which is expected rather than reassuring, because `INotifyWatcher::drop`
  joins nothing while the FSEvents teardown spins. A green Linux run was therefore never
  evidence for this.
  CAUSE, identified 2026-07-29 by story 34.11
  (`spec-34-11-a-kill-during-a-ref-update-must-not-strand-a-folder.md`): a SIGKILL during a
  reference update leaves a stale `.git/refs/heads/<branch>.lock`, and the next
  `keeper-syncd sync --once` never returns because gix-ref's failed-lock error-message fixup
  walks `parent_index` without ever advancing `cursor` once it reaches the root edit — so a
  failed lock on a deref'd CHILD ref becomes an infinite 100%-CPU loop instead of an error, and
  `sync_to_completion`'s `.output()` waits on that child forever. That is precisely the live
  trio the CI cleanup step named. Isolated with no keeper code in the loop: planting
  `refs/heads/main.lock` and calling `gix::Repository::commit_as("HEAD", ..)` directly never
  returns (killed at 15 s), while planting `.git/HEAD.lock` — the root edit, whose
  `parent_index` is `None`, so the loop body never runs — returns
  `Err("A lock could not be obtained for reference \"HEAD\"")` in 134 ms, which is verbatim the
  PR #21 failure. One root cause, two shapes.
  WHAT STAYS OPEN, and why this entry is not closed: the gitoxide loop itself. Story 34.11 adds
  stale-ref-lock recovery at repo open, which removes its only OBSERVED trigger, but the
  upstream defect is untouched and any other route to a failed lock on a child ref — a
  concurrent writer, a lock this repo did not leave behind — still reaches it. Closing it needs
  an upstream report and patch against gix-ref, or a keeper-side deadline around ref
  transactions; the pinned versions are in src-tauri/Cargo.lock, so a bump is the thing to check
  first.
status: open
note: 2026-07-29 — the recurrence cost is already bounded, and that guard is not part of what
  stays open. `src-tauri/.config/nextest.toml` sets
  `slow-timeout = { period = "60s", terminate-after = 4 }`: the warning cadence is nextest's
  existing default, so a legitimately slow test logs exactly what it logged before, but past
  four minutes the test is killed and reported as `TIMEOUT` against its own name — which fails
  the build and reaps the child processes a hung test leaves behind. Four minutes came off
  measured CI timings, not a guess: the slowest honest tests on that runner are the archive
  search perf gate at 24.98 s and this sweep at 12.05-21.54 s, the next slowest test in the
  workspace is 4.4 s, and all 1469 together take 43 s. So a recurrence costs four minutes and a
  named failing test instead of ninety-six minutes and a cancelled run.

### DW-121: The desktop app registers itself as the git LFS clean/smudge filter, but the app binary has no `lfs` subcommand — so the filter fails on every invocation.

origin: found while repairing the racily-clean guard for story 34.13, 2026-07-29
location: keeper-sync/src/engine.rs:342-344 (`filter_program` = `current_exe()`) + keeper-syncd/src/commands.rs:1621 (the only implementation) + keeper/src (no `lfs` subcommand anywhere)
reason: `Engine::open` configures `filter.lfs.clean`/`smudge` to `std::env::current_exe()`, and its comment claims "both understand `lfs clean|smudge`". `keeper-syncd` does. The `keeper` app binary does not — a repo-wide grep of `crates/keeper/src` finds only `lfs_mode` and `lfs_threshold_bytes` in `sync_ipc.rs`, and no argument parsing that could answer `lfs clean --repo <path>`. Because keeper also sets `filter.lfs.required=false`, a filter that fails is indistinguishable from one that is absent, so nothing reports it: git falls back to storing the bytes it was given. Measured against both plain git and gitoxide while fixing 34.13: with a WORKING clean filter a racily-clean LFS entry reads clean, because git re-reads the worktree and cleans it back into the pointer the index already holds; without one it reads modified. So on desktop the guard `lfs::stage::is_false_modification` is the ONLY thing standing between a committed LFS file and a full re-clean of every such file on the next scan, and on a `keeper-syncd` host that same guard is dead code. keeper stages its own pointers (`lfs/stage.rs`), so this is not why LFS sync works — the harm is to anything that drives real `git` inside a synced folder, which is exactly the concurrent human writer the ref-lock work (34.11) was scoped to protect.
evidence: Measured while fixing 34.13, against both plain git and gitoxide: with a WORKING clean filter a racily-clean LFS entry reads clean, because git re-reads the worktree and cleans it back into the pointer the index already holds; without one the raw bytes are hashed and the path reads as modified. The test that demonstrated this (`a_working_clean_filter_settles_the_racily_clean_case_before_the_guard_does`) was removed on 2026-07-29 — see the platform note below — so the observation lives here rather than in a test.
status: done 2026-07-29
resolution: fixed as story 34.19. The first of the two routes was taken — the app
  binary learned the subcommand — because the other one contradicts a promise
  docs/sync.md §8 already makes ("that is what lets you use ordinary `git` inside
  a synced folder"), and refusing to register the filter would have meant editing
  that promise out rather than keeping it.
  The filter body moved out of `keeper-syncd` into `keeper_sync::lfs::filter`, so
  there is now ONE implementation rather than one per binary — the same reasoning
  AD-52 applies to the engine, a layer down. `keeper::run` serves it before the
  Tauri builder exists and returns; the parse demands `lfs` as the very first
  argument, so a Finder launch (no arguments) and an older macOS launch
  (`-psn_…`) are untouched, and that is asserted. `filter::run` is generic over
  its streams rather than locking stdio, which is what finally makes the thing
  testable: a clean round-trips to a stored object and its pointer, a smudge
  restores the content byte-exactly, an unresolvable pointer and a
  larger-than-a-pointer input both pass through whole.
  The macOS/Linux status divergence recorded below should now be moot, and the
  end-to-end racily-clean test that had to be deleted for it can be restored —
  that is left as a follow-up rather than claimed here, because it was not
  re-attempted.
note: 2026-07-29 — two routes, and the choice is a real decision rather than a detail. Either the app learns the `lfs clean|smudge` subcommand (a CLI surface inside a GUI binary, which needs its own argument-parsing decision and must stay invisible to normal launches), or `enforce_local_config_with_filter` refuses to register a program that cannot serve — which is honest, but leaves a synced LFS folder with no clean filter at all for external git, so it trades a silent failure for a loud absence. Not fixed with 34.13 because it lives in `git/repo.rs`, and because picking between those two is not a decision to make in passing.
note: 2026-07-29 — a failing clean filter produces DIFFERENT status outcomes on macOS and Linux, which is a new fact about this defect and the strongest argument yet for fixing it. Same fixture, same code, same git 2.55: an LFS entry that is genuinely racily clean (verified in-test with gix's own `Stat::matches` and `Stat::is_racy`, both true) is reported MODIFIED on Linux/ext4 and CLEAN on macOS/APFS. The racily-clean re-read is a content comparison, so it runs `filter.lfs.clean` — the broken filter this entry is about — and the platforms evidently disagree about what a failing non-required filter yields. Consequence for coverage: keeper-sync's end-to-end racily-clean path CANNOT be tested on the only platform keeper ships to while this defect stands. Story 34.13 responded by testing `lfs::stage::is_false_modification` directly on constructed state, which is deterministic everywhere, and by deleting the two integration tests that depended on the re-read. Fixing this defect is what would let that integration coverage exist at all.

### DW-122: Story 34.14's `sync_git_status` / `sync_git_path_set` are fully implemented, registered and typed, and no frontend surface calls either — so a machine with an unusable `git` still has no way to say where a usable one is.

origin: found while committing the epic-34 tail, 2026-07-29
location: keeper/src/ipc.rs (`git_report`, `sync_git_status`, `sync_git_path_set`) + keeper/src/lib.rs:327-328 (both registered) + src/lib/ipc/gen/{SyncGitVm,SyncGitState}.ts (generated) + src/components/settings/sync-section.tsx (no consumer)
reason: the resolution, the settings key, the IPC pair and the generated bindings all exist and are tested; a repo-wide grep for `syncGit`/`SyncGitVm` outside `src/lib/ipc/gen/` finds nothing. So the report that says "git 2.23 at /usr/local/bin/git does not clear the 2.42 floor, and here is what was skipped" is computed and then discarded, and the setting that would fix it can only be written by a caller that does not exist. The capability flag DOES consume the same resolution, so the Sync surface correctly hides itself — which makes this a dead end rather than a broken screen: the user is told sync is unavailable and given no way to act on it.
evidence: `bun run typecheck` passes with the bindings unreferenced; `git_report` is exercised only through `capabilities`. The two commands are reachable over IPC by hand (`invoke("sync_git_status")`) but nothing in the UI does.
status: done 2026-07-30
resolution: STALE-OPEN, corrected. This was already fixed when the ledger commit (`e12e98f`) rewrote
  this entry and left it `open` with the note below saying the surface "was never wired" — by then
  false, because commit `5e9720e`, in the same change set, had added
  `src/components/settings/sync-git-row.tsx` (207 lines, with its own 211-line test file) and mounted
  it at `settings-dialog.tsx:194`. A stale-open entry is as wrong as a false close: it sends the next
  person to build a surface that exists.
  What shipped: `SyncGitRow` renders the report, the configured path, and a field that writes
  `sync_git_path_set`. The placement contradicts this entry's own suggested one, and the commit
  message argues it out: Settings → Sync is gated on `capabilities.sync`, which *is*
  `git_report(..).state == Ok`, so a report placed inside that section would be unreachable on
  precisely the machines it exists for. The row therefore sits BESIDE the gate, renders when git is
  fine too (so a pinned path can be cleared before it breaks anything), renders nothing where folder
  sync does not exist at all, and keeps a rejected path shown as rejected rather than clearing itself
  — a field that reset itself on a bad value would be the silent fallback to automatic that story
  34.14 exists to end. The dialog suite asserts the placement: the report renders with the capability
  OFF while the Sync section does not, so moving the row behind the gate fails a test.
  The remaining half, closed 2026-07-30 in this same PR: `sync_git_path_set` used to call only
  `invalidate_git_resolution()`, so the report and `capabilities.sync` changed while the already-open
  `Engine` kept its `GitCli` built from the old setting — every push, merge and worktree call still
  driving the previous binary, the capability disagreeing with the engine again. It now goes through
  `crate::sync::repoint_engine`: invalidate, `reset_engine()` (signal the supervisor, then empty the
  engine slot), and re-arm the supervisor only when the new resolution chose something. Teardown is
  unconditional, so a rejected path leaves no engine and no supervisor rather than a stale one still
  syncing on the old binary. Test:
  `repointing_at_another_git_rebuilds_the_engine_and_re_arms_background_sync`.
  Not claimed: the teardown only signals the outgoing supervisor, which finishes its current unit, so
  two loops can briefly overlap — bounded by `db::claim_ready`'s single-`UPDATE` claim. And nothing
  notices a pinned binary that is moved or deleted outside the app; the next git call fails with
  git's own diagnostic.
note: 2026-07-29 — not a regression and not release-blocking: it is a surface that was never wired, and the state it reports is one the capability flag already handles safely. It is the obvious next slice of 34.14 and wants Settings → Sync to render the report plus a path picker.

### DW-123: Stories 34-14 through 34-19 have no `spec-*.md`, so six shipped stories carry their rationale only in commit messages and `docs/sync.md`.

origin: found while reconciling sprint-status.yaml with the epic-34 tail, 2026-07-29
location: _bmad-output/implementation-artifacts/ (spec files stop at `spec-34-13-…`) + sprint-status.yaml (34-14..34-19 added 2026-07-29 with this gap noted inline)
reason: every other story in this project has a spec that records the problem, the decision and the acceptance before the code exists. These six were implemented from a field report and a defect investigation instead, so the artifact trail runs the other way: the commits are unusually detailed and `docs/sync.md` §§3/8/11/14/15 carry the user-facing contract, but there is no single document stating what each story was accepted against. That makes the epic-34 retrospective harder than it needs to be and leaves nothing for a reviewer to check the code against.
status: done 2026-07-29
resolution: all six written (1544 lines), each to the structure `spec-34-13` establishes, with
  `</intent-contract>` closing after the I/O matrix as every existing spec does. The earlier note
  below argued for not back-filling them; that was over-weighted. The authorship-bias risk is real
  but the mitigation is mechanical — write the Problem from the evidence as it stood BEFORE the fix
  (the field report, the ledger entry, the live-server probe, the test that was passing falsely) and
  derive Acceptance from observable behaviour — and no spec at all is plainly worse than one written
  with that discipline. Each carries an explicit "not covered" section rather than implying
  completeness.
  Reviewing them paid for itself twice. 34-17's author flagged the `--` ssh-option-injection guard as
  the one detail with a security consequence and no test; it is now covered by the argv assertion and
  mutation-checked, and the spec records that rather than the gap. 34-17 also found that the engine
  wiring for that story landed in `8a8aba4` and not `339acaf`, and says so, so a reader diffing one
  commit is not misled. Every multi-word `snake_case` identifier cited across all six was checked
  programmatically against the tree: the only two that do not resolve are `ensure_activity_size_column`
  and `a_working_clean_filter_settles_the_racily_clean_case_before_the_guard_does`, both correctly
  framed as things this work replaced or that 34.13 deleted.
  Not claimed: none of the six has been through code review, and all nine epic-34 tail stories remain
  at `review` in sprint-status. Four of the six were written by the implementer of the code they
  describe, which is exactly the bias the note below names — a reviewer should read them as such.
note: 2026-07-29 — deliberately not back-filled in the same pass that wrote the code. A spec written after the fact by the author of the code tends to describe what was built rather than what was needed, which is the failure mode the specs exist to prevent; these are better written from the commits and the ledger by someone doing the epic-34 review.

### DW-124: Nothing reconciles committed LFS pointers against the remote, so a large file committed with plain `git` publishes a pointer whose bytes never leave that machine — the exact failure story 34.15 is named after, through the one door it does not watch.

origin: found while narrowing story 34.15's invariant during the epic-34 tail review, 2026-07-30
location: keeper-sync/src/engine.rs:2021-2023 (`lfs_uploads_outstanding`) + :2045-2055 (the gate in `do_push`) + keeper-sync/src/lfs/filter.rs:126 (`clean`, which stores the object and journals nothing) + docs/sync.md §§3, 8 and §15's troubleshooting table
reason: the gate that holds a push is `outstanding_count(profile, "lfsUpload")`, a query over
  keeper's own journal — and the journal learns of an upload from exactly one place,
  `commit_local`. Story 34.19 newly makes a human's `git commit` inside a synced folder work
  properly: the clean filter runs, the object lands in `.git/lfs`, a pointer goes into the index.
  It files no journal unit. So `outstanding_count` is 0, the gate passes, keeper's next push
  publishes that pointer, and the bytes it names exist on one disk. Nothing afterwards compares the
  pointers in history against the objects the remote holds, so keeper cannot notice later either —
  and the object is not even lost, which is what makes it insidious: `git ls-tree` on the remote
  reports the file present, because for an LFS path the blob IS the pointer. That is precisely the
  false pass the durability matrix was giving before 34.15, reappearing with a different cause.
  Worse, the recovery advice that suggests itself does not work. A parked upload can be retried from
  the file's row; a pointer with no unit at all has nothing to retry, and re-running sync will not
  create one, because keeper only stages files whose content changed. The only route today is to
  modify the file so a new revision is staged, which uploads the *new* object and leaves the
  published revision unbacked forever. `docs/sync.md` said "sync that folder again and the upload is
  re-driven"; that was false and has been corrected.
  Two candidate fixes, and they are not equivalent. (a) A reconciliation pass: walk the pointers
  reachable from the pushed branch, batch-verify them against the remote's object store (the LFS
  batch API's `download` operation answers exactly this, and `lfs::local::contains` answers it for a
  filesystem remote), and enqueue an upload for every one the remote lacks. Correct, and costs a
  history walk plus a batch round trip per push. (b) Have the clean filter record the debt it
  creates — it already knows the oid and the size at the moment it stores the object. Cheap, and
  wrong on its own: the filter runs for `git add` and for `git status`'s racily-clean re-read, neither
  of which is a commit, so it would enqueue uploads for content that may never be committed and would
  need the journal to learn to forget them. (b) also cannot repair a pointer that is already in
  history, which is the case a user hits first. Not attempted in this PR: it is a new engine leg with
  its own failure modes, and shipping the honest boundary in the spec and the docs is the correct
  interim.
status: open

### DW-125: `Engine::open` throws away the resolution's probe and spawns `git --version` again, so one `doctor` run probes git at least three times and can name a binary the engine is not driving.

origin: reported by the epic-34 code review (acceptance layer) and confirmed by FixGitResolution while fixing story 34.14's resolver, 2026-07-30
location: keeper-sync/src/engine.rs:341-343 (`git_program()` then `git.capabilities()`) + keeper-sync/src/git/resolve.rs:97-98 (`GitChoice::capabilities`, which already holds the answer) + keeper-sync/src/git/cli.rs:408 (`capabilities_of`, which exists for exactly this) + keeper-sync/src/platform.rs (`SyncPlatform::git_program` returns a bare `PathBuf`) + keeper-syncd/src/commands.rs:1236 (`check_git`) and the `check_engine` call below it
reason: `git::resolve` probes each candidate, and `cli::capabilities_of` turns the version it read
  into a `GitCapabilities` that `GitChoice` carries — the whole point being that the version is read
  once. `Engine::open` then asks the platform for a *program*, because that is all
  `SyncPlatform::git_program` returns, and re-probes it. The capabilities the resolution computed die
  at the trait boundary; the only thing that still reads them is `GitResolution::summary`'s prose
  (`resolve.rs:226-228`), so the field is not literally dead, but no consumer of the *engine* ever
  sees it.
  Two costs. One process per engine open, which is trivial. And a second time-of-check/time-of-use
  window: between the resolution's probe and the engine's, the file can be replaced — a Homebrew
  upgrade mid-run is the realistic case — so the engine can end up driving a binary the report never
  saw. `keeper-syncd doctor` is where this is most visible, because it is a single command that
  answers a question about git: `check_git` resolves (probe 1), `check_engine` opens an `Engine`,
  which resolves again (probe 2, since `LinuxPlatform::git_resolution` is deliberately uncached and
  documents why) and then probes the chosen program (probe 3). Story 34.14's AC5 claimed doctor
  reported "the same binary and version the engine will drive, from one probe"; that was not true and
  the criterion has been narrowed to what the code does.
  The fix is a signature change: have `SyncPlatform` hand out the resolution, or a
  `(PathBuf, GitCapabilities)` pair, so one probe feeds both the report and the engine. It crosses
  `engine.rs`, `platform.rs`, both platform implementations and the trait itself, which is why no
  agent in this PR took it — and why it is worth doing deliberately rather than in passing: the
  trait is the seam that keeps the engine free of Tauri, and widening it is a design decision.
status: open

### DW-126: `ensure_activity_columns` is a read-then-`ALTER` with no transaction, so two processes sharing one `sync.db` can both decide a column is missing and the loser dies with `duplicate column name` — on the first run after an upgrade, which is exactly when someone is watching.

origin: found by the epic-34 code review (edge-case layer) while reading story 34.16's migration, 2026-07-30
location: src-tauri/crates/keeper-sync/src/db.rs:145-158 (`ensure_activity_columns`)
reason: the function prepares `PRAGMA table_info(activity)`, collects the column names, and then
  runs `ALTER TABLE activity ADD COLUMN …` for each one it did not find. Between the read and the
  write there is no transaction and no `BEGIN IMMEDIATE`, so the check is not serialized against
  another connection doing the same thing. Two processes on one `sync.db` is not hypothetical: the
  daemon is designed to coexist with an operator running `keeper-syncd doctor` or
  `keeper-syncd sync --once` (the journal's whole claim protocol exists for that), and both open the
  database through `db::open`, which runs the migrations. If they interleave, the second `ALTER`
  fails with `duplicate column name: unit_id`, `db::open` returns an error, and the process exits
  before doing any work.
  The window is one statement wide and only exists until the columns are present, so it is narrow —
  and it is narrow in the least forgiving place: the first start after an upgrade, on the machine
  where somebody has just run the daemon and then immediately typed `doctor` to see whether it
  worked. The failure mode is a hard startup error with a message about SQLite schemas, for a
  condition that is actually benign.
  The fix is small and the choice between two shapes is the only thing to think about: wrap the read
  and the writes in one `BEGIN IMMEDIATE` transaction (correct, and matches what
  `registry::reorder_pins` did for DW-36), or tolerate the race by treating a "duplicate column"
  error as success (fewer moving parts, but it swallows a class of real schema errors and has to
  match on an error string). The transaction is the right answer. Not done here because `db.rs` was
  being edited concurrently by another slice of this PR and a migration is not a change to make in a
  contested file.
status: open

### DW-127: `lfs::local` publishes objects into the REMOTE store with `0o600`, so on the shared bare repository story 34.18 exists to serve, the next user's keeper or `git-lfs` cannot read them — and the failure looks like a filesystem mishap.

origin: found by the epic-34 code review (edge-case layer) while reading story 34.18, 2026-07-30
location: src-tauri/crates/keeper-sync/src/lfs/store.rs:123 and :168 (`NamedTempFile::new_in`, whose file is created `0o600`) + :283-287 (the publish-by-rename that preserves those bits) + keeper-sync/src/lfs/local.rs:108-131 (`transfer`, the only caller that writes into a store belonging to somebody else)
reason: `tempfile::NamedTempFile` creates its file `0o600` by design, and nothing widens the mode
  before the rename that publishes it. For the *local* store that is right and should stay: those
  objects are the user's own, under their own `.git`. `lfs::local::transfer` is the one path that
  writes into a store the user does not own — the remote's — and it inherits the same bits, so the
  object lands readable by one uid inside a repository whose whole purpose is to be shared.
  Story 34.18's own module header names the topology: "a bare repository on a pendrive, an SMB
  mount, another directory on the same disk". A pendrive is usually fine, because the filesystem has
  no meaningful uids. A bare repo on a shared box is not: user A's keeper pushes and copies the
  objects, user B clones and gets pointer files, and `git lfs` reports the object missing rather
  than unreadable — so the diagnosis a user reaches for is "the transfer failed", which is the
  wrong tree entirely. Note that git itself does not have this problem for its own objects, because
  `core.sharedRepository` exists and git honours it.
  Fix candidates: read `core.sharedRepository` from the remote's config and match the mode git would
  use (correct and most in keeping — the remote already declares its own sharing policy), or
  apply the directory's own mode to the published file, or simply widen group/other read on a
  remote-store publish. Whichever, it belongs in `LfsStore::publish` behind an explicit flag rather
  than in `transfer`, so the local store's `0o600` is not weakened by accident. Not done in this PR:
  it needs a decision about honouring `core.sharedRepository`, and getting it wrong loosens
  permissions on the user's own objects.
status: open

### DW-128: A relative or `~`-prefixed filesystem remote is resolved against the process's working directory rather than the repository, so under systemd or an app bundle it resolves against `/` and the profile reports "drive not connected" forever.

origin: found by the epic-34 code review (edge-case layer) while reading story 34.18, 2026-07-30
location: src-tauri/crates/keeper-sync/src/lfs/local.rs:70-91 (`remote_path`, which returns `PathBuf::from(trimmed)` verbatim) + :143-152 (`is_reachable`) + keeper-sync/src/engine.rs:2883-2885 (the reachability check that turns this into `MediaAbsent`)
reason: `remote_path` decides whether a remote URL names a filesystem path, and when it does it
  hands back the string as written. `../backup/repo.git` and `~/drives/pendrive.git` are both
  accepted and neither is resolved: the first is resolved by the OS against the process's current
  directory, and the second is not expanded at all, so it looks for a literal directory named `~`.
  A daemon under systemd has a working directory of `/`; an app bundle's is arbitrary and not the
  repository. So the path exists for the person who typed it into a shell and not for keeper.
  What makes this worth an entry rather than a shrug is the failure it produces. `is_reachable`
  answers false, `copy_lfs_object` returns `SyncError::MediaAbsent`, and the profile renders as
  *drive not connected* — a state whose entire meaning is "re-attach the drive; nothing was
  deleted". The user re-attaches a drive that was never detached, indefinitely, with no surface
  naming the path keeper actually looked at. git itself does not disagree, incidentally: the git
  remote is resolved by git relative to the repository, so `git push` can work while the LFS copy
  cannot find the same remote — two components disagreeing about one path, which is the class of
  defect story 34.14 was about at the binary level.
  Fix: resolve a relative remote path against the profile's `local_path` (which is what git does)
  and expand a leading `~` from the platform's home, both inside `remote_path`; and, independently,
  refuse a relative remote at profile-creation time, because a relative remote is almost never what
  someone means. Wants a test per shape. Not done here because it changes what an existing profile
  resolves to, which is a migration question rather than a patch.
status: open

### DW-129: The `git-lfs-authenticate` handshake reads both of ssh's pipes to EOF into unbounded `Vec`s; only the rendered string is bounded, where the sibling batch client bounds the read itself.

origin: found by the epic-34 code review (edge-case layer) while reading story 34.17, 2026-07-30
location: src-tauri/crates/keeper-sync/src/lfs/ssh.rs:354 (`command.output()`) + :510-527 (`truncated`, which bounds the *rendered* string) + :452-465 (`parse_response`, which parses whatever arrived) — contrast keeper-sync/src/lfs/batch.rs (`bounded_body`, and its doc "Never `reqwest::Response::text`: that has no ceiling")
reason: `Command::output()` reads stdout and stderr to EOF into `Vec<u8>`, with no ceiling. The
  module was careful about the *symptom* — `MAX_STDERR_BYTES` keeps a megabyte-long login banner out
  of an error message, and its comment says exactly that — but the bytes are already in memory by
  then, and stdout has no equivalent guard at all: `parse_response` takes the whole buffer and hands
  it to `serde_json`.
  The ceiling that does exist is time, not size: the 20 s `COMMAND_TIMEOUT` with `kill_on_drop`, so
  a hostile or broken server has twenty seconds of pipe bandwidth to spend. That is a bounded amount
  of memory in the same sense that a fast disk is a bounded amount of disk. It is not a
  vulnerability worth a release: reaching it requires an ssh server the user's own key authenticates
  to. It is an inconsistency, and the inconsistency is the argument — `lfs/batch.rs` closed exactly
  this hazard for HTTP responses in this same subsystem, with a comment explaining why an unbounded
  read of an untrusted body is not acceptable, and the ssh leg simply did not get the same
  treatment.
  Fix: spawn with piped stdio and read each pipe through a `take(LIMIT)`, treating an over-long
  stdout as malformed (a valid answer is a few hundred bytes; git-lfs's own is smaller) and an
  over-long stderr as the banner it already suspects. `Command::output()`'s convenience is the only
  thing lost. Not done in this PR because `ssh.rs` was being edited concurrently by another slice.
status: open

### DW-130: `sync_ipc_error` maps `Forbidden` onto `invalidCredentials` and `LfsUploadPending` onto `syncUnavailable`, because `IpcErrorCode` has no code for either — so the frontend is told "wrong token" for a scope problem and "sync unavailable" for a push that is deliberately waiting.

origin: reported by FixAppIpcAndUi while wiring story 34.15's two new error variants through IPC, 2026-07-30
location: src-tauri/crates/keeper/src/sync_ipc.rs (`sync_ipc_error`) + src-tauri/crates/keeper-core/src/vm.rs (`IpcErrorCode`, which has no `forbidden` and no deferred/waiting code) + src/lib/ipc/gen/IpcErrorCode.ts (generated) + src/lib/ipc/client.ts:2506-2508 (`syncFolderNow`'s documented rejection set)
reason: story 34.15 added `SyncError::Forbidden` precisely because a 403 and a 401 have opposite
  remedies — the token is valid and lacks a scope, versus the token is wrong — and then the IPC
  boundary collapses them again, because `IpcErrorCode::InvalidCredentials` is the nearest thing
  that exists. `LfsUploadPending` is worse in kind: it is not a failure at all, it is a wait that
  resolves itself, and `syncUnavailable` says the opposite. Neither mapping is wrong *given the
  available codes*; both are nearest-fit, and the frontend currently renders no distinct affordance
  for either, so nothing is visibly broken today. It becomes a real defect the moment a surface acts
  on the code — a "check your token" prompt on a 403 sends the user to replace a credential that is
  fine.
  Fix: add a `forbidden` variant to `IpcErrorCode` (keeper-core/src/vm.rs), regenerate the TS, and
  handle it wherever `invalidCredentials` is handled today; decide separately whether a deferred wait
  deserves a code at all or should stop being an error at the IPC boundary. Also, independently of
  the enum: `client.ts:2506-2508` documents `syncFolderNow` as rejecting with
  `unsupported | serverUnreachable | invalidCredentials | internal`, and `syncUnavailable` now
  belongs in that list — a doc comment that under-reports the reject set is how a caller ends up with
  an unhandled branch.
status: open

### DW-131: The "empty `gitPath` means automatic" rule is enforced twice, independently, in two crates — so a third host would have to remember it a third time.

origin: reported by FixGitResolution while fixing story 34.14's resolver, 2026-07-30
location: src-tauri/crates/keeper-core/src/registry.rs (`get_sync_git_path`, filters `value.trim().is_empty()`) + src-tauri/crates/keeper-syncd/src/platform.rs:128-131 (`with_git_path`, filters the same thing on the TOML value)
reason: `gitPath = ""` in TOML deserializes to `Some(PathBuf::from(""))`, and taking the explicit
  branch on it produced a refusal naming an empty path — which, because an explicit path deliberately
  has no fallback, refused every git operation on the box. Both hosts now normalise it away, and both
  document why, and each does it in its own crate: the app in `keeper-core::registry`, the daemon in
  its platform's builder. The rule is "cleared and never set are one state", it belongs to the
  resolver's contract, and it currently lives nowhere near `GitRequest`.
  No user-visible defect remains after this PR — both spellings agree today. The cost is the ordinary
  cost of a duplicated invariant: the next `SyncPlatform` implementation (an iOS build that grows
  folder sync, a test platform, a Windows host) starts out wrong by omission, and nothing fails until
  someone types two quotes into a config file. Fix: one normalizer beside `GitRequest` — a
  `GitRequest::from_setting(Option<&str>)` that answers `search` or `explicit` — and have both hosts
  call it instead of filtering for themselves.
status: open

### DW-132: The stale-ref-lock recovery's "a torn or half-written reference is not a state this can produce" argument holds for one competing writer, not two.

origin: found by the epic-34 code review (durability layer) while auditing story 34-11, 2026-08-07
location: src-tauri/crates/keeper-sync/src/git/repo.rs:253-280 (the watch-not-age rule and its safety doc) + :202-243 (`release_stale_ref_locks`, wired into `open` at :73) + spec-34-11-a-kill-during-a-ref-update-must-not-strand-a-folder.md, which states the bound unconditionally
reason: the doc reasons that removing a lock we watched for the full window cannot corrupt anything,
  because a writer whose lock we unlinked fails its own rename with `ENOENT`. That is exactly true
  with one other writer and false with two. Writer A holds an open fd on `HEAD.lock`; we unlink the
  path; writer B now wins `O_EXCL` and creates a fresh `HEAD.lock`; A renames its own older, possibly
  partial content over `HEAD`, publishing it as the reference value, and B's later rename is the one
  that gets `ENOENT`. The result is a reference holding bytes no writer intended — the outcome the
  argument says is unreachable.
  It is second-order, and that is why it is deferred rather than fixed: reaching it requires our
  judgement to already be wrong (a live writer that touched its lock file not once across the full 2 s
  watch), and then a second writer arriving inside the microseconds between our `unlink` and A's
  `rename`. Nothing observed, and story 34-11's real-SIGKILL matrix did not produce it. The cost of
  leaving it is that both the spec and the source state the bound as unconditional, so the next person
  to widen the recovery — shorten the window, extend it to a new lock class, run it somewhere other
  than `open` — will widen it on an argument that does not carry the weight it claims. Fix, if it is
  ever worth one: hold the recovery under a repository-wide advisory lock, or verify the lock file's
  identity (dev/ino, or a content hash) between the last watch sample and the unlink, so the thing
  removed is provably the thing watched. Correcting the stated bound in the doc is the cheap half and
  should happen either way.
status: open
decision: correct the safety argument's stated bound rather than the code; revisit the code only if
  the recovery is widened beyond the `open`-time, watched, per-lock shape it has today.

### DW-133: The anti-hang refusal that stops a daemon spinning on a live writer's lock covers the commit path only; the fetch leg's remote-tracking ref update is unguarded, and the reasoning that makes that safe is written down nowhere.

origin: found by the epic-34 code review (durability layer) while auditing story 34-11, 2026-08-07
location: src-tauri/crates/keeper-sync/src/git/commit.rs:316-319 (`ensure_head_unlocked`, placed immediately before `commit_as`) + src-tauri/crates/keeper-sync/src/git/repo.rs:349-366 (the refusal) + src-tauri/crates/keeper-sync/src/engine.rs:1893 (the fetch call site, which has no equivalent) + DW-120 (the upstream gix-ref loop this exists to avoid)
reason: `release_stale_ref_locks` handles the ABANDONED lock everywhere — it walks all of `refs/`
  recursively (repo.rs:220-247) and the durability matrix has a row for a remote-tracking lock. The
  HELD case is different: recovery must not take a live writer's lock, so it returns, and the caller
  has to refuse rather than walk into gitoxide's transaction, which is where DW-120's infinite loop
  lives. `ensure_head_unlocked` performs that refusal for `HEAD` and the local branch, i.e. for the
  commit path. `git::fetch` opens its own reference transaction over `refs/remotes/origin/<branch>`
  and has no such pre-check. [INFERENCE] nothing has hung there because a remote-tracking edit is a
  root edit with no `HEAD` deref, so gix-ref takes its clean-error path rather than the child-edit
  loop that never advances its cursor — which means the fetch leg is safe by a property of gitoxide's
  internals rather than by anything keeper does.
  Deferred because there is no observed hang and the fix is not one line: either generalise
  `ensure_head_unlocked` into an `ensure_refs_unlocked(paths)` called from both legs, or verify the
  inference against gix-ref 0.66.0 and write it down beside the fetch call. The second is cheaper and
  is the honest minimum — an invariant that holds because of an upstream implementation detail should
  say so out loud, or the next gitoxide bump removes it silently and the symptom is a wedged daemon.
status: open

### DW-134: Stale-lock recovery watches each lock serially for a full 2 s at every `Repo::open`, and one sync pass opens the repository four times.

origin: found by the epic-34 code review (durability layer) while auditing story 34-11, 2026-08-07
location: src-tauri/crates/keeper-sync/src/git/repo.rs:202-243 (`release_stale_ref_locks`) + :50-58 (the windows) + :73 (called unconditionally from `open`), against the four production callers: engine.rs:1819 (`open_repo`), :1893 (fetch), :3480 (prune), :3681 (status)
reason: watch-don't-age is the right rule — a lock is stale because nobody touched it for a window,
  not because it is old — but it is paid serially and unconditionally. Three abandoned locks cost one
  `open` 3 x 2 s on the sync thread, and because every production entry into the repository goes
  through `open` (which is the property that makes the recovery total, and is worth keeping), a
  genuinely wedged repository pays that window up to four times inside a single tick. In the normal
  case it costs nothing: a healthy repository has no locks to watch and the scan is a directory walk.
  Not fixed because every cheap-looking fix trades away something real. Caching "this repository had
  no locks" across opens within a tick reintroduces a staleness window exactly where crash-consistency
  lives. Watching the locks concurrently makes the failure modes harder to reason about for a saving
  measured in seconds-per-crash. Shortening the window weakens the liveness judgement that DW-132
  already identifies as the load-bearing one. If this ever needs fixing the shape is a per-tick
  recovery pass that runs once before the legs rather than inside each `open` — a restructuring of the
  tick, not a tweak.
status: open

### DW-135: Story 34-9's headline claim — files sync when they land, in seconds rather than a poll interval — is proven by no automated test, and the one line in `tick_profile` that arms the watcher is asserted by nothing.

origin: found by the epic-34 code review (durability layer) while auditing story 34-9, 2026-08-07
location: src-tauri/crates/keeper-sync/src/engine.rs:764 (the `ensure_watcher` call inside `tick_profile`) + the tests at :6464-6512 and :6710, which call `ensure_watcher` directly and hand-place `WatchEvent`s on the channel + spec-34-9-files-sync-when-they-land.md, whose Verification section opens "Not run here."
reason: every watcher test in the crate reaches `ensure_watcher` by calling it, and every event test
  puts the event on the channel by hand — deliberately, because the spec states that none of the tests
  wait on filesystem event timing, which is a defensible choice for determinism. The consequence is
  that the whole suite passes on a build where `tick_profile` never arms a watcher at all. The wiring
  line is the only thing that makes the story true, and its only proof is a manual hesperia procedure
  the spec says plainly was not run. Compare 34-11 in the same epic, which spawns the real
  `keeper-syncd`, SIGKILLs it, and asserts the recovery out of stderr.
  Deferred rather than fixed because what is missing is not a unit test. It is one integration test
  that drives a real `Engine` over a real temp directory, writes a file, and asserts a commit appears
  inside a bounded wall-clock deadline — the one shape the spec deliberately excluded, with the
  flakiness budget that comes with it (inotify and FSEvents delivery latency, and a deadline generous
  enough for a loaded CI box yet tight enough to fail a reverted `ensure_watcher`). That is real test
  engineering, and it belongs beside `keeper-syncd/tests/durability_matrix.rs`, which already proves
  the crate can afford a real-binary harness. Until it exists, "files sync when they land" is an
  inspected property, not a defended one.
status: open

### DW-136: The watcher-lifecycle acceptance says both of a dropped watcher's threads are joined; the tests measure map cardinality, so a thread leak would be invisible to every one of them.

origin: found by the epic-34 code review (durability layer) while auditing story 34-9, 2026-08-07
location: src-tauri/crates/keeper-sync/src/engine.rs:6464-6512 (pause, double-arm idempotence, resume, unknown profile, remove, root move, stop_all — all asserted through `armed_count`) + `watch::retire`, which hands teardown to a detached thread + spec-34-9's manual step 8 (`ls /proc/<pid>/task | wc -l`), also unrun
reason: the acceptance criterion is about threads and the assertion is about a `HashMap`'s length.
  `retire` detaching the teardown is what makes the gap real rather than pedantic: the map entry
  disappears immediately, the threads exit whenever they exit, and a change that left one of them
  blocked forever on a channel receive would keep every one of these tests green while a pause/resume
  loop leaked two threads per cycle. The only check that would catch it is the manual thread count,
  which nobody ran.
  Deferred because asserting a join from outside needs a seam the type does not offer: `retire` would
  have to return a handle, or `FolderWatcher` would have to expose an all-threads-exited signal a test
  could await with a timeout. Either is small, but both change the public shape of the type for a
  test, which deserves a deliberate decision rather than a drive-by. The alternative that costs
  nothing structurally is a test that arms and retires N times and asserts the process thread count
  returns to baseline — portable only on Linux, and precisely the manual step, automated.
status: open

### DW-137: After the wake filter lands, a build writing into a `.gitignore`d but not tier-0 directory (`target/`, `dist/`, `build/`) still sets the watch wake on every tick and still drives a 1 Hz `git status` over the whole tree.

origin: residual of this batch's fix for story 34-9's blocking wake finding, reported by the fixing agent, 2026-08-07
location: src-tauri/crates/keeper-sync/src/engine.rs (`fold_watch_events`, the wake seam, which now filters through the profile's compiled `ExcludeSet`) + src-tauri/crates/keeper-sync/src/exclude.rs:104-117 (why `target`/`dist`/`build` are deliberately NOT tier-0 built-ins) and :132-133 (`.git/**`, which is) + DW-119, which owns that decision
reason: the blocking finding was that the wake was set for ANY delivered event, so the five directory
  names story 34-9 added to tier 0 could not suppress the 1 Hz re-stat they were added to prevent. The
  fix routes the wake through the same `ExcludeSet` the stability gate uses, which closes tier 0 and
  every per-profile pattern, and closes `.git/` for free. It cannot close `.gitignore`: consulting it
  at this seam would put a second `.gitignore` reader in the engine, and the design's rule is that
  `git status` is the single authority on ignore semantics. So the pathology survives for exactly the
  names DW-119 argues must not become tier-0 built-ins — a Rust or Node build writing continuously
  into `target/` or `dist/` wakes the engine every second, and each wake costs a full `git status`
  that correctly finds nothing to commit.
  Bounded and self-correcting, but the walk cost is real on a large tree, which is the entire reason
  `next_scan_ms` exists. The resolution that does not violate the single-authority rule is to let a
  user's own per-profile excludes cover it — they already flow into the same `ExcludeSet` — which
  makes this a defaults-and-documentation question rather than a code one, and ties it to DW-119
  rather than standing alone. One more uncovered case, recorded so it is not rediscovered: tier 0's
  `.git` rules are subtree rules (`.git/**`), so an event on the bare `.git` directory node itself is
  not matched and still wakes. Harmless and rare.
status: open

### DW-138: `EchoSuppressor` has no production callers; after this batch's doc correction it is a tested helper the shipped engine never uses.

origin: residual of this batch's fix for story 34-9's EchoSuppressor finding, reported by the fixing agent, 2026-08-07
location: src-tauri/crates/keeper-sync/src/watch.rs — `EchoSuppressor`, `FolderWatcher::suppressor()`, the `is_suppressed` branch inside `fold_batch`, and the unit test over it. A grep for `EchoSuppressor::register` across `src-tauri` finds only tests.
reason: the audit found three things tangled together — a module doc asserting as fact that the engine
  registers paths before writing (it does not), a green test that reads as coverage of a live
  invariant (it is not), and the type itself. The first two are corrected in this batch: the doc now
  says the engine does not register, and the test is documented as a unit test of a helper production
  does not use. The type stays, with zero callers, because story 34-9's spec deliberately declined to
  wire registration: `git status` is authoritative about what changed, so an echo costs one fruitless
  walk and never a wrong commit, and a suppressor the engine must keep in step with its own writes is
  a second source of truth about the worktree.
  Left open as a decision, not a defect. Two honest endings: delete the type and its test, on the
  grounds that the argument against wiring it is unlikely to reverse and dead code carrying a test
  reads as a feature; or keep it and record the condition under which it would be wired — a measured
  echo cost the exclude filter cannot reach. What must not happen is that it sits unowned long enough
  for someone to assume it is load-bearing, which is exactly how the module doc came to claim it was.
status: open

### DW-139: The racily-clean path the LFS false-modification guard exists to manage is exercised end to end on no platform, and the two integration tests that used to reach it were deleted.

origin: found by the epic-34 code review (durability layer) while auditing story 34-13, 2026-08-07
location: src-tauri/crates/keeper-sync/tests/lfs_roundtrip.rs (every remaining guard test asks `is_false_modification` on hand-built index state: no `status_paths`, no clock, no subprocess, no clean filter) + src-tauri/crates/keeper-sync/src/lfs/stage.rs (`is_false_modification`) + DW-121, the broken clean filter that makes the route unreachable on the shipping platform
reason: the guard decides whether a path git reports as modified is a real edit or an artefact of a
  racily-clean re-read. Every input that makes that question hard lives in the impure shell: git's own
  racily-clean handling, the `filter.lfs.clean` subprocess, and per-platform `status` divergence — the
  spec itself records Linux/ext4 and macOS/APFS disagreeing about an entry both agreed was racy. What
  is tested, thoroughly, is the predicate's logic. What is tested nowhere is that the predicate is
  reached with the inputs it was designed for, on the platform keeper ships to. The two tests that did
  reach it were removed because DW-121 leaves the desktop clean filter failing on every invocation, so
  the route cannot be driven from the app binary at all. This batch's fix narrows the guard's
  discriminator to paths actually routed through LFS and adds tests over real on-disk repositories and
  real `.gitattributes`, which is a genuine improvement — and still calls the predicate directly.
  Deferred because it is downstream of DW-121: until the app binary answers `lfs clean`, an end-to-end
  test on macOS has nothing to drive. The sequencing that closes it is DW-121 first, then one
  integration test that commits an LFS-tracked file through a real repository with the filter
  installed, touches it inside the same mtime second at the same size, runs a real `status_paths`, and
  asserts the guard's verdict — on both a Linux and a macOS runner, because the divergence the spec
  recorded is the whole reason a single-platform version of this test would mislead. Filed so that the
  guard's REACHABILITY is not mistaken for its logic, which is well covered.
status: open

### DW-140: Whoever fixes DW-124 must not do it by re-staging — `is_false_modification` is designed to suppress exactly the signal a re-stage-based reconciler would look for.

origin: found by the epic-34 code review (durability layer) while auditing story 34-13 against story 34-15's invariant, 2026-08-07
location: src-tauri/crates/keeper-sync/src/engine.rs:5068-5072 (the upload obligation is journaled to SQLite before the ref moves; proven at :5113-5141) + :1278 (the publish gate that holds the push until the object is on the remote) + src-tauri/crates/keeper-sync/src/lfs/stage.rs (`head_records`, which answers true for a committed pointer whose object was never uploaded) + DW-124, which owns the missing reconciliation
reason: story 34-15's promise — a pointer is never published ahead of its object — holds because the
  obligation is written down before the ref moves and read back from the journal, never re-derived
  from the tree. Story 34-13's guard is safe under that regime: it dismisses a committed pointer whose
  object is still owed, and dismissing it costs nothing, because the journal and not the working tree
  is what drives the upload. The two facts compose into a hazard the moment the journal is not there —
  `sync.db` lost or replaced, the profile removed and re-added, a machine restored from a backup
  predating the commit. The tree still holds the pointer, the remote still lacks the object, and the
  guard now prevents the scan from noticing, because `head_records` says HEAD already has this pointer
  and the stat has not moved. Silence, permanently.
  Not fixed here because the fix is DW-124's, not 34-13's: the recovery has to be a reconciliation
  against the remote — do the objects these committed pointers name actually exist there? — which is
  the same pass DW-124 needs for pointers committed with plain `git`. This entry exists to constrain
  that fix. A reconciler built on "re-stage and see what git reports" cannot work, because this guard
  is specifically built to keep git's report quiet. Note that this batch narrowed the guard to paths
  genuinely routed through LFS, which shrinks the exposure to real LFS paths and does not remove it.
status: open

### DW-141: `last_sync_ms` lives only in the engine's in-memory status map, so a relaunch — or any `keeper-syncd status` invocation, which is a different process — reports "never synced" for a folder that has synced correctly for a week.

origin: found by the epic-34 code review (visibility layer) while auditing story 34-10, 2026-08-07
location: src-tauri/crates/keeper-sync/src/engine.rs:1695-1700 (`mark_synced`, which writes only `Self::lock(&self.status)`) + :627-637 (`set_state`, its neighbour, which persists and says in a comment why) + src-tauri/crates/keeper-sync/src/db.rs, where `last_sync` has zero matches because no column exists + the tests at engine.rs:6632-6695, all three cases inside a single `Engine` instance
reason: story 34-10 names as its own problem #2 that the UI cannot tell "never synced" from "synced,
  nothing to do", then fixes only the half that lives in this process's memory. Its tests cannot see
  the gap because they assert every case against one live `Engine`. The function two definitions above
  already made the opposite choice for `state`, and explains it in a comment — "so a separate
  `keeper-syncd status` invocation reports the truth rather than a fresh 'idle'" — which is the same
  argument, about the same surface, for the field 34-10 added.
  Deferred rather than fixed in this batch because it is a schema change, not a line: a `last_sync_ms`
  column on the profile row, a migration in the `ensure_*_columns` style the crate already uses (with
  DW-126's read-then-`ALTER` race applying to any new one), a write in `mark_synced`, a read at engine
  open to seed the status map, and a test across two `Engine` instances over one `sync.db` — which is
  the only test shape that could have caught this in the first place. Small, but it touches storage,
  and it belongs with whoever next opens the schema.
status: open

### DW-142: The mechanical commit subject gained an `@device` qualifier that no spec or epic describes, while spec-34-5's I/O matrix still promises today's subject byte for byte.

origin: found by the epic-34 code review (visibility layer) while auditing story 34-5, 2026-08-07
location: src-tauri/crates/keeper-sync/src/provenance.rs:451-479 (`mechanical_subject`) + :274-280 (`device_qualifier`) + :445-450 (the doc calling these bytes "a compatibility surface, not a detail") + src-tauri/crates/keeper-sync/src/git/commit.rs:294-304 (the call site) and :663-667 (a test comment reading "asserting the bare `sync(docs)` here would be asserting the bug") + spec-34-5-the-machine-name-the-commit-subject-and-the-knobs-that-were-hiding.md, whose I/O row promises `sync(p): 3 added, 1 modified, 1 deleted`, byte for byte
reason: when the machine has been renamed, an empty subject template now renders
  `sync(<profile>@<device>): …` rather than `sync(<profile>): …`. Grepping `_bmad-output` for
  `device_qualifier` or `@device` returns nothing, so the shape appears in no planning artifact; the
  only record that the change was intentional is a test comment saying the old form would be
  "asserting the bug". Somebody decided the spec was wrong and left the decision in a test.
  This is a spec-versus-code divergence, not a behavioural defect — the qualifier is arguably the
  better product behaviour, since a subject naming the machine is more useful in a multi-device
  history than one that does not. It is filed because these bytes are wire-visible in every repository
  keeper touches, the code says so about itself, and `docs/sync.md` and the epic now disagree with
  `git log`. The fix is a ruling, not a patch: amend the epic and 34-5's contract to describe the
  qualifier and the condition under which it appears, or revert `mechanical_subject` to the documented
  form and let the device name live only in the trailer. Whoever takes it should settle DW-143 in the
  same sitting, because that is the same qualifier's input.
status: open
decision: needs an owner ruling — amend the artifacts or revert the bytes. It must not stay a test
  comment.

### DW-143: `device_qualifier` decides whether the machine "still answers to its hostname" by comparing against `Provenance.origin`, whose own doc says it may be a volume label.

origin: found by the epic-34 code review (visibility layer) while auditing story 34-5, 2026-08-07
location: src-tauri/crates/keeper-sync/src/provenance.rs:73 (`origin` documented as "Hostname, or a volume label for a removable profile") + :274-280 (`device_qualifier`) + src-tauri/crates/keeper-sync/src/git/commit.rs:300 (the comparison) + the two live call sites, engine.rs:2019-2024 and :2600, which both pass `self.platform.host_label()`
reason: nothing diverges today, because both callers pass a hostname. The hazard is that the field's
  documented contract permits something else, and a caller honouring that doc would silently change
  what the qualifier means: every commit on a removable profile would carry `@<volume label>`, and the
  stated condition — "the user has renamed this machine" — would quietly become "this folder is on a
  pendrive". Two documents disagree about one field and the code trusts the wrong one.
  Deferred because it is latent and the right resolution depends on DW-142's ruling. If the qualifier
  stays, the cheap fix is to correct `origin`'s doc to say it is the host label at these call sites,
  or better, to pass the host label to `device_qualifier` explicitly rather than reading it out of a
  struct field that means something wider. If the qualifier is reverted, this disappears with it.
  Filed so the pair is settled together rather than one at a time.
status: open

### DW-144: A pure rename charges its bytes twice in the Activity list — a renamed 2 GB file reads as "Deleted 2 GB" plus "Added 2 GB".

origin: found by the epic-34 code review (visibility layer) while auditing story 34-6, 2026-08-07. The story's verdict was `correct`; this is a sub-finding the reviewer asked be recorded so the behaviour is a decision rather than an accident.
location: src-tauri/crates/keeper-sync/src/engine.rs — `record_commit_activity`, which walks `added`/`modified`/`deleted` only; `collect_stable_changes`, which measures the new path with `FileSample::of`; `removed_size`, which measures the old path from the index entry; and `ActivityKind`, which has no rename arm + spec-34-6-activity-says-what-changed-and-how-big.md, which never mentions renames
reason: each row is individually true and no size is faked — the added row really did add those bytes
  to the tree and the deleted row really did remove them — so this is not the defect class story 34-6
  targets, which is a size that lies or a zero standing in for an unknown. It is a summing problem at
  the human layer: the acceptance is about what a person can tell at a glance, and at a glance a
  rename now reads as 4 GB of movement.
  Not fixed because rename detection is a feature, not a repair. It wants a fifth `ActivityKind`, a
  schema column for the old path, a detection pass over the staged change set (git's own similarity
  detection, or an exact-oid match, which is the cheap and correct case for a pure rename), and a
  render for it. That is a story. Until someone decides it is worth one, the honest reading of the
  list is "what changed on disk", which is what it shows.
status: open

### DW-145: The "conflict is the only coloured row" rule in the Activity list is defended by no test; making all four rows destructive-toned would ship green.

origin: found by the epic-34 code review (visibility layer) while auditing story 34-6, 2026-08-07. Story verdict `correct`; sub-finding.
location: src/components/layout/sync-pane.tsx:373-378 (`ACTIVITY_KINDS`: three `text-muted-foreground`, conflict `text-destructive`) + src/components/layout/sync-pane.test.tsx:973-975 (`expect(new Set(glyphs).size).toBe(4)`, taken over the `class` attribute)
reason: the only assertion in the area counts distinct glyph classes, and four distinct
  `lucide-<name>` classes satisfy it on their own — the tone tokens live in the same attribute and are
  never looked at. So the Design Notes' explicit rejection of "a scoreboard" in favour of a quiet log
  is enforced by nobody: colouring every row `text-destructive` passes.
  Deferred only because it is a one-test fix in a file three other agents were editing during this
  batch, not because it is hard. The test to write asserts tone per kind rather than a count — three
  rows carrying the muted token and the conflict row carrying the destructive one. Better still, have
  it assert the RULE (exactly one kind is toned differently from the other three) rather than the
  literal Tailwind tokens, because a token rename should not be what makes this red.
status: open

### DW-146: Every `keeper-sync` engine test opens with `let Ok(engine) = Engine::open(..) else { return; };`, so on a machine without a usable `git` the whole family reports green having exercised nothing.

origin: found independently by two layers of the epic-34 code review (visibility and window chrome), 2026-08-07
location: src-tauri/crates/keeper-sync/src/engine.rs:4142-4144, :5209, :6595 and their siblings — the idiom is crate-wide, not per-story. Among the assertions inside such bodies: the three-frame commit-progress test, the `Keeper-Source` trailer read-backs, the per-kind Activity size test, the device-rename round trip, and `a_scan_that_stages_nothing_reports_nothing`.
reason: the guard exists for a real reason — `Engine::open` resolves a git binary, and a test box may
  not have one — and a skipped test beats a failing one for a genuinely absent dependency. The problem
  is that the skip is silent and indistinguishable from a pass. A CI runner with a shadowed, broken or
  absent `git` prints the same green as one that ran every assertion, and several of epic 34's
  strongest proofs live inside these bodies. This is the failure mode where a suite quietly stops
  being evidence.
  Deferred because the fix is a crate-wide convention change and should be made once, deliberately.
  Either the harness asserts a usable git at suite start and hard-fails without one — correct for CI,
  hostile on a contributor's box — or the skip goes through a helper that prints a loud skip line and
  increments a counter, so a green run that exercised nothing looks different from one that did. The
  second is the smaller change and matches how the rest of this repo treats honesty about what was and
  was not run. Whoever takes it should convert every site in one pass; thirty half-converted skips are
  worse than the current uniform one.
status: open

### DW-147: Opening a folder's Edit form pulls its access token out of the keychain into the webview even when the user never expands Advanced, so the read is wider than the surface that displays it.

origin: found by the epic-34 security review while auditing stories 34-4 and 34-12, 2026-08-07. The broader question — whether `sync_get_credential` should be gated at all — was answered in this batch and declined, with the trust-model reasoning written into the command's own rustdoc under the heading "# Why this is not gated"; this is the narrower half that was not changed, and the rustdoc points here for it.
location: src/components/sync/add-folder-form.tsx:573-614 (the mount effect, keyed on `profile?.id` alone, while the token field itself lives inside `{expanded && ...}`) + src-tauri/crates/keeper/src/sync_ipc.rs:1017-1038 (`sync_get_credential`, whose "# Why this is not gated" rustdoc is the decision record for the gating question and is not reopened here)
reason: the trust-model question was settled: the webview is already trusted as the renderer, every
  `#[tauri::command]` is equally reachable from it, and a gate on this one buys nothing against a
  compromised renderer. That reasoning is now in the source and this entry does not reopen it. It does
  not cover this point. The effect depends only on `profileId`, so the secret crosses IPC and lands in
  JS memory on every Edit open — including the overwhelmingly common case where the user came to
  rename the folder or change its cadence and never opens the section that shows a token.
  `tokenVisible` is also mount-sticky rather than time-bounded, and hiding the keeper window via the
  tray does not unmount the pane, so a revealed token stays legible across a hide/show cycle.
  Deferred because it is a scope-tightening change with real UX consequences, and it is a form change
  rather than an IPC gate. Keying the read on `expanded && profileId` makes the first expand of
  Advanced asynchronous, which gives the field a visible empty-then-filled moment and forces the
  existing `reading`/`unreadable` states to cover a sequence they were not written for (expand,
  collapse, expand again). The cheaper variant is to keep the mount read and drop the value from state
  when the disclosure collapses, which shortens the exposure without touching the load path — weaker,
  because it still reads on an Edit open that never expands, but it is one line and it is honest about
  what it buys. Worth doing either way: the principle that a secret is read when it is shown, not when
  its container mounts, is cheap to hold and expensive to reintroduce. Whichever shape wins needs its
  own tests, including one asserting that an Edit open which never expands issues no
  `syncGetCredential` call.
status: open

### DW-148: `lfs::ssh::Credential` carries a live `Authorization` header value behind a derived `Debug`, opting out of the redaction idiom every other secret-carrying type in the crate follows.

origin: found by the epic-34 security review, cross-cutting sweep over `Debug`/`Display`/`Serialize` on secret-carrying types, 2026-08-07
location: src-tauri/crates/keeper-sync/src/lfs/ssh.rs:254-275 (`#[derive(Debug)]` on `Credential` and on the `Answer` enum wrapping it) — contrast src-tauri/crates/keeper-sync/src/credential.rs:40-46 (`AccessToken`, hand-written, tested at :165-169), src-tauri/crates/keeper-sync/src/git/fetch.rs:62-75 (tested at :738-746), and `BatchClient` (tested at batch.rs:1119-1123 with the comment "AD-53: `BatchClient` ends up in `tracing` spans")
reason: no live leak today. The reviewer found no `{:?}` on the type outside tests, `CachedSshAnswer`
  has no `Debug`, and `Engine` is not `Debug`, so nothing currently formats it. The reason to record
  it is that this crate has an explicit, tested convention for exactly this case — three sibling types
  hand-write a redacting `Debug`, each naming NFR-26 or AD-53 as the reason, each pinned by a test
  asserting the secret does not appear in the formatted output — and this type, which holds a
  server-minted, immediately-spendable header value, silently opted out. The same module is otherwise
  scrupulous: its `diagnostic` quotes ssh's stdout only up to the first `{`, precisely because a body
  carries the minted `Authorization`, and there is a test for it. So this is an oversight, not a
  judgement.
  Deferred only because `lfs/ssh.rs` was outside every fix slice in this batch. The fix is about
  fifteen lines and should copy the sibling shape exactly: a hand-written `Debug` withholding
  `authorization` — and `href`, which is a URL that can itself carry userinfo — plus a test asserting
  the token bytes do not appear in `format!("{:?}", ..)`. One `#[derive(Debug)]` on an enclosing type,
  or one `tracing` field, is the whole distance between latent and real.
status: open

### DW-149: Two persistence sites survive profile removal that story 34-7's completeness enumeration did not reach — Notes vault rows, and a token the daemon reads from the environment.

origin: raised by the epic-34 security review as explicitly outside story 34-7's audited scope, 2026-08-07
location: src-tauri/crates/keeper-sync/src/db.rs:308-320 (`delete_profile` clears journal, file_state, activity and profiles) + the `notes_vaults` rows keyed on profile id introduced by story 37.1 + src-tauri/crates/keeper-syncd/src/platform.rs (`secret_get` reads an environment variable ahead of its 0600 file store, and `secret_delete` cannot remove one)
reason: story 34-7's load-bearing claim — remove a folder and its secret goes with it — was verified
  and holds for the app. These are two sites the enumeration did not cover, and neither is a defect in
  what 34-7 shipped. The first is a plain unanswered question: nobody has checked whether
  `notes_vaults` rows outlive the profile they are keyed on. It is non-secret data, but it is the same
  deletion-completeness question, and a stale vault row pointing at a profile id that no longer exists
  is the kind of thing that surfaces as a baffling empty state months later. The second is arguably
  correct rather than broken: under a `LoadCredential=`-style systemd deployment the operator supplies
  the token out of band and it is not keeper's to delete — but it is still a place a token lives that
  removal does not reach, and since the daemon has no removal path at all, nothing exercises it either
  way.
  Filed together because they are one question — what else is keyed on a profile id? — and the work is
  one pass: enumerate every table and every store keyed on a profile, assert `delete_profile` covers
  each, and write the answer down beside it so the next table added inherits the question. For the
  daemon's environment fallback the resolution is documentation: say plainly that an operator-injected
  secret is the operator's to withdraw.
status: open

### DW-150: `titlebar_drag_report` is a non-async `#[tauri::command]`, so the diagnostic for a titlebar drag performs two main-thread filesystem writes inside the very mouse-down the story exists to keep free.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-3, 2026-08-07
location: src-tauri/crates/keeper/src/ipc.rs:6338 (`pub fn titlebar_drag_report` — neither `async` nor `#[tauri::command(async)]`) + src-tauri/crates/keeper/src/debug_log.rs:76-90 and :93-110 (`GatedMakeWriter` admits `WARN` to the file leg regardless of the toggle; `GatedWriter::write` does `create_dir_all` + `OpenOptions::open` + `write_all` per event) + src/lib/titlebar-drag.ts:66-77, which fires it twice per gesture + src-tauri/crates/keeper/src/ipc.rs:1258 (`off_async_runtime`, the helper the story's other seven commands use)
reason: story 34-3 states the rule as AD-34-5 — no `#[tauri::command]` does filesystem work on the
  main thread — and converts seven recording commands to satisfy it. The command it ADDED does not
  satisfy it. By the exact Tauri rule the story quotes, a synchronous `pub fn` command runs on the
  main thread, and this one's body is not inert: a `WARN` reaches the file leg unconditionally and
  opens the log fresh per event. `beginTitleBarDrag` calls it once immediately after
  `startWindowDragging()` and once on settle, so a titlebar mouse-down now costs two synchronous file
  opens on the main thread, one of them potentially landing while `start_dragging` is inside a nested
  AppKit drag loop.
  Two harms, and the second is why this is worth an entry rather than a shrug. It violates the story's
  own stated invariant, in the story's own new code. And it contaminates the instrument: the spec's
  failure table reads "`issued` only, no settle ⇒ the Rust side never answered" as evidence of
  main-thread blocking, and this probe is now itself a contributor to main-thread blocking, so the
  diagnostic can indict the condition it creates. Not fixed in this batch because
  `keeper/src/ipc.rs` was outside every fix slice. The fix is one line of signature plus the existing
  helper — `pub async fn` with the body wrapped in `off_async_runtime`, or at minimum
  `#[tauri::command(async)]` — and DW-156's convention test would keep it fixed.
status: open

### DW-151: The tray's "an unchanged tick writes nothing" early return can be deleted with the whole suite still green — four tests prove the pure diff, and nothing binds the diff to the renderer.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-1, 2026-08-07
location: src-tauri/crates/keeper/src/tray.rs:1614-1617 (`if writes.is_empty() { return; }`) + :1445-1461 (`sync_writes`, the pure diff, which IS tested) + the four `sync_tray_tests` at :1955-2065 + DW-97, which owns the broader "tray and shell glue is manually validated only" gap that predates this story
reason: story 34-1 exists because the tray repainted itself every second. The diff deciding whether to
  repaint is pure and well tested; the line that acts on the diff is tested by nothing. Delete it and
  `apply_sync_state` repaints on every tick again — precisely the defect the story was written to kill
  — while four green tests continue to assert that `sync_writes` correctly reported there was nothing
  to do. That is the failure shape worth naming: the pure core was extracted so it could be tested,
  and the extraction moved the untestable part one layer up rather than shrinking it.
  Genuinely hard, which is why it is deferred: `TrayIcon` cannot be constructed in a unit test, so
  there is no way to observe "no OS write happened" without a seam. The honest remedy is to give
  `apply_sync_state` one — a `&mut dyn FnMut(SyncWrite)` sink, or a small trait carrying the two
  setter methods — so a test can drive the renderer with a scripted tick sequence and assert the sink
  was called zero times. That is a modest refactor of a function that is otherwise lock-disciplined
  and correct, and it would give DW-97's much broader gap its first foothold. What it should not be is
  another pure-function test; there are already four.
status: open

### DW-152: Story 34-1's third acceptance criterion describes a rotating ring the shipped tray does not have, and names a test that does not exist.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-1, 2026-08-07
location: spec-34-1-tray-glyph-stops-repainting-itself.md — AC 3, the `Transferring` I/O row, the Design Notes paragraph "Why the glyph identity is an address", and the named test `a_transfer_still_animates_through_the_memo` + src-tauri/crates/keeper/src/tray.rs:1373-1385 (`sync_glyph` takes no frame; `Transferring` maps to the single `SYNC_UPDOWN_ICON_PNG` at :1377) + :160-164, the doc recording that four direction glyphs replaced the ring and that the tray "no longer advances a frame counter"
reason: the spec asserts the icon is written on every `Transferring` tick so the ring keeps turning,
  and argues from that requirement to its central design point — that a memo keyed on state alone
  would freeze the animation. There is no animation. A steady `Transferring` now writes nothing, which
  is the correct product behaviour and is what the story wanted, and the design argument no longer has
  a referent: state IS the key, in effect. The test the spec cites was never written; the nearest
  existing one (`a_change_of_direction_repaints_the_icon`) proves a weaker property, that the three
  direction glyphs are distinct assets.
  Filed as a spec-versus-code divergence with no behavioural defect behind it. It matters because a
  reviewer reading the frozen contract against the code reads a working feature as a regression —
  which is exactly what happened during this audit — and because the next person to touch the memo
  will reason from an argument about a frame counter that no longer exists. Fix: amend 34-1's contract
  to describe the frame-free glyph set and drop the named test from its verification list, or mark the
  AC superseded and point at tray.rs:160-164, currently the only accurate account of the decision.
status: open

### DW-153: The tray's `!installed` branch is unreachable in production, and a test asserts the state machine that would make it reachable.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-1, 2026-08-07
location: src-tauri/crates/keeper/src/tray.rs:1458 (the `!installed` arm of `sync_writes`) + :930-931 (`store_rendered_mode` clears `sync_item` and `sync_memo` in the same two statements) + :1820-1821 (`store_sync_render` always writes both together) + the assertion at :2062-2064
reason: `memo = Some` with `installed = false` cannot occur — the only code that drops the menu item
  drops the memo in the same breath, at both sites. The real displacement path is `memo = None`, which
  yields `SyncWrites::BOTH` and repaints, correctly. So the branch is harmless defensive code, and the
  test covering it asserts a transition the type cannot reach.
  Recorded rather than removed because the danger runs the other way. Someone simplifying
  `store_rendered_mode` — dropping the memo clear because "clearing the item is what matters" — would
  make the branch reachable, and would find a green test claiming the case is handled at exactly the
  moment what the branch does needs re-deciding. Two honest endings: delete the branch and the test
  together and let the invariant rest on the two statements that already enforce it, or keep both and
  add a line to the test saying it documents a defended-against but unreachable state. The first is
  cleaner; the second is safer if anyone doubts the pairing is exhaustive.
status: open

### DW-154: `BUSY_DWELL_TICKS` does not decay while recording owns the tray, so the first sync glyph after a recording ends can be up to three ticks stale.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-1, 2026-08-07
location: src-tauri/crates/keeper/src/tray.rs:1584-1586 (the `recording_owns` early return) + :1580-1583 (the no-tray early return) + :1596 (`dwelled`, reached only past both) + spec-34-1's **Always** clause, which states `dwelled` is called once per tick
reason: pre-existing — the early return predates story 34-1 and the story did not change it — and
  small: the visible symptom is that the sync glyph shown immediately after a recording stops can
  reflect a busy window that has already closed, for at most three seconds. The reason to write it
  down is the spec clause. `dwelled` is called once per tick that the sync renderer actually reaches,
  which during a recording is none of them, and an unstated exception like that is what the next
  feature to add an early return will rely on without knowing it.
  Deferred because the fix is not obviously "call `dwelled` before the early returns". The dwell
  counter exists to hold a busy glyph on screen long enough to be seen, and whether it should decay
  while the tray is showing something else depends on whether the dwell is about the glyph or about
  the state. That is a product question worth thirty seconds of someone's attention and not worth
  guessing at. Correct the **Always** clause either way; it is one sentence.
status: open

### DW-155: `render_recording` pushes `set_text` on every tick, unconditionally — the last per-tick OS write left on the tray after story 34-1.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-1, 2026-08-07
location: src-tauri/crates/keeper/src/tray.rs:833-837 + spec-34-1 §Intent 1, which states "the recording renderer never did this — `render_recording` guards on the held `status_item`"
reason: the guard the spec credits prevents a menu REBUILD, not a per-tick text write; the write
  itself is unconditional. Mostly benign, because a recording's status line carries an elapsed time
  and genuinely does change every second, so the memo story 34-1 built for the sync line would
  suppress almost nothing here. Filed because the asymmetry is now the only one left on the tray, and
  because the story's problem statement rests on a claim about this function that is not true —
  anyone reasoning "the recording side already solved this, copy it" will find there is nothing to
  copy.
  Fix, if the tray's OS-write budget ever matters again: the same `SyncMemo` shape applied to the
  recording line, which is cheap now that the pattern exists and would also cover a paused or
  otherwise static recording state. Correct the spec's sentence regardless.
status: open

### DW-156: Story 34-3's two load-bearing tokens — the ACL grant that is the fix, and the `async` keyword on seven commands — are asserted by nothing.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-3, 2026-08-07
location: src-tauri/crates/keeper/capabilities/desktop.json:15 (`core:window:allow-start-dragging`, the actual fix, unasserted) + src-tauri/crates/keeper/src/ipc.rs:5336, 5349, 5366, 5512, 5549, 5915, 6019 (the seven `async fn` commands; `the_snapshot_halves_compose_into_one_authoritative_read` exercises the halves directly and would pass unchanged if every one reverted to `pub fn`)
reason: both are the same shape — a single token whose removal is compile-clean, behaviour-changing
  and invisible to the suite — and both have named cheap remedies, which is why they are filed as one.
  The capability grant is worth more. The spec's own failure table anticipates precisely the case
  where the edit does not reach the built app (a stale `gen/schemas`, the wrong capability file), and
  the symptom of that is silence: the window simply stops dragging, with the ACL denial visible only
  in the app log. A three-line test that reads the JSON and asserts the permission string is present
  turns a dropped or regenerated capability file into a red suite instead of a user's complaint. The
  `async` half is harder, and honest about it: the repo already carries a source-level convention test
  of the right shape (`src/test/no-user-agent-gating.test.ts`), so a scan asserting every
  `#[tauri::command]` in the recording family is `async` or `#[tauri::command(async)]` is available —
  and would, incidentally, have caught DW-150.
  Deferred because `keeper/src/ipc.rs` and the capability file were outside every fix slice in this
  batch. Take them together: one test file, two assertions, and DW-150 gets a guard rather than a
  second occurrence.
status: open

### DW-157: The drawer-reaches-its-bottom acceptance is proven by DOM containment rather than reachability, and reachability is not provable in this harness at all.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-2, 2026-08-07. The story's verdict was `correct`; this records the bound on what its tests can show.
location: src/components/layout/sidebar-pane.test.tsx:432-448 (containment: views and Spaces inside `[data-slot="scroll-area-viewport"]`, `Add account` outside it) and :450-458 (class pinning: `min-h-0` on the `<nav>`, `min-h-0 flex-1` on the scroller) + src/components/layout/sidebar-pane.tsx:116-270 + DW-118, the 56 px webview overhang that is the other half of the same user-visible outcome
reason: the acceptance names a 600 px window height and a visible account footer. jsdom sets no height
  and performs no layout, so neither is checked. What the tests do check is the right available proxy
  and is not vacuous — moving the footer inside the scroller or the groups outside it fails, and
  dropping either `min-h-0` fails. What passes anyway is a change that keeps every class and still
  breaks reachability: an ancestor gaining `h-auto`, or the ScrollArea viewport losing its overflow.
  Filed as a bounded, stated limitation rather than a defect, because the alternative would be
  theatre. The only instrument that can settle it is the accessibility-tree re-measurement on a real
  macOS window that DW-118 already uses — it measured `Add account` at y=1012 against a window bottom
  of y=1000 — and that same measurement is the only thing that can separate this story's overflow fix
  from DW-118's overhang. So the two belong to one check: whoever resolves DW-118 should re-measure
  the footer's position at a small window height and record the number here, and if it lands on
  screen, close this by citing the measurement rather than by writing a test that cannot see it.
status: open

### DW-158: The drag band's collapsed-rail width is untested, so a hard-coded width would restore the seam AD-34-3 exists to prevent, with a green suite.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-2, 2026-08-07
location: src/components/layout/app-shell.tsx:195-198 (the band's drawer column switches on `sidebarCollapsed` between `SIDEBAR_WIDTH_CLASS.collapsed` (`w-12`) and `.expanded`) + src/components/layout/app-shell.test.tsx:170-214, where every band test renders expanded + src/components/layout/sidebar-pane.tsx:78 (`SIDEBAR_WIDTH_CLASS`, the single width source both sides read)
reason: the story's structural achievement is that the width literal exists once and both the band and
  the drawer read it, so the two columns cannot drift apart. The tests confirm the band paints per
  column and that `pl-[78px]` is gone, but they only ever render the expanded rail. A regression that
  restated a width inside the band — the precise thing AD-34-3 forbids — would show a mismatched seam
  on the collapsed rail only, and every test would pass.
  Deferred because it is one extra render in a file another slice was editing during this batch, not
  because it is interesting. The test is: render with `sidebarCollapsed: true` and assert the band's
  first column carries `w-12`. Better, assert it carries the same class the drawer root carries in the
  same state, so the test pins the RULE — one source of width — rather than the current value.
status: open

### DW-159: The drag band's 28 px height is a bare `h-7` restated in prose one line below, with nothing keeping the two in step.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-2, 2026-08-07
location: src/components/layout/app-shell.tsx:189 (`h-7`) and :191 (the biome-ignore comment, which describes "an empty 28px strip")
reason: below the bar for an exported constant — one literal, one file, and the band is the only thing
  that reserves the inset, which is the property the story was actually about. The defect is the prose
  copy: change `h-7` and the comment lies silently, and a comment naming a pixel value is exactly what
  the next reader will trust when deciding whether some other element has to match it. The fix is a
  word rather than a refactor — have the comment say "the band's height, `h-7` above" instead of
  restating the number. Recorded rather than done because `app-shell.tsx` was outside every fix slice
  in this batch.
status: open

### DW-160: Story 34-3's acceptance list still reads as though its two refuted mechanisms closed the drag defect, and the one criterion left over from them is covered by nothing.

origin: found by the epic-34 code review (window-chrome layer) while auditing story 34-3, 2026-08-07
location: spec-34-3-the-window-stays-draggable-on-the-recording-tab.md — §Tasks & Acceptance (AC 1 and AC 2, unamended) against §Follow-up 2026-07-29, which refutes mechanisms A and C and names the ACL grant as the real cause; and AC 5, the `SelectContent` exit-animation removal in src/components/ui/select.tsx
reason: this is the story whose premise was wrong twice, and the file records the correction honestly:
  measurement refuted the Recording-tab-only framing, the follow-up says so, and the real fix — the
  `core:window:allow-start-dragging` grant plus an app-owned band handler — is structural rather than
  tab-scoped. Everything needed to correct the acceptance list is in the same file. Nothing points the
  list at it, so the first thing the next reader sees still reads as though the `async` conversion and
  the focus coalescing are what fixed the drag. Both are good changes on their own merit — a
  `read_dir` on the main thread is a defect regardless — but they are not the fix, and the story's
  title still says "on the Recording tab".
  AC 5 travels with it: the `pointer-events` hardening has no test and should not get one. It is
  Radix-internal behaviour, the follow-up refutes it as a cause, and a proxy assertion there would be
  theatre. Recorded so the criterion is not later read as covered, because it is not. Fix: amend the
  acceptance list to match the follow-up, reconsider the title, and mark AC 5 as defensive hardening
  verified by inspection. Documentation only — no code should move.
status: open

### DW-161: The push leg draws a file-count bar with no transfer rate, because `git::cli::capture` runs the subprocess to completion and there is no streaming stderr to hang a meter on.

origin: residual of this batch's fix for the epic-34 review's F4 (visibility layer, story 34-8), reported by the fixing agent, 2026-08-07
location: src-tauri/crates/keeper-sync/src/git/cli.rs:313-353 (`capture`, which waits for the child and only then reads its output) + src-tauri/crates/keeper-sync/src/progress.rs (`SyncPhase::carries_rate`, now a total match over `Fetching`, `UploadingLfs`, `DownloadingLfs`) + src-tauri/crates/keeper-sync/src/engine.rs:1999 (`do_pull`'s fetch fold) and `TransferTally::apply`, the only two producers that stamp a figure + epic 34's AD-34-13, "Progress that cannot be rated is not progress"
reason: F4 was that `SyncPhase::is_transferring` claimed `Pushing` carries a rate while no producer
  ever stamped one, so a text-only folder pushing 200 MB drew a bar that moved and told the user
  nothing — the exact complaint story 34-8 is named for, in the commonest case. It was fixed by
  telling the truth rather than by inventing a number: `is_transferring` had zero callers repo-wide
  and is replaced by `carries_rate`, a total match over the three phases that genuinely produce one.
  `direction()` is deliberately unchanged, because a push really is bytes going up and the tray arrow
  is right about that.
  What remains is the underlying gap, and it is worth naming rather than leaving as an absence: an
  ordinary `git push` still shows a file count and no rate. Stamping one means `git push --progress`
  and a streaming parse of its stderr, and there is nowhere to put that today — `capture` runs the
  child to completion and reads afterwards, so the whole git-CLI seam would need a streaming variant
  (a spawned child with piped stderr, a line reader feeding the progress sink, and a decision about
  what to do when the parse does not recognise git's output, which changes between versions and is
  localised). That is a feature, not a review fix.
  The reason this needs an entry rather than silence: AD-34-13 says progress that cannot be rated is
  not progress, and the push leg is now a documented, tested exception to that decision. An exception
  someone chose is fine; an exception nobody recorded becomes the precedent for the next one.
status: open

### DW-162: `tracing::debug!` cannot print in the shipped desktop app, so 127 diagnostic call sites across all three crates are dead code — and the Settings "debug mode" toggle does not turn them on.

origin: found while diagnosing the third field report on story 44.3 (default spaces seeded nothing and logged nothing), 2026-08-09
location: src-tauri/crates/keeper/src/debug_log.rs:122-129 (`init`, the only place the process-wide subscriber is built) + the 127 `tracing::debug!` call sites outside test modules, principally keeper-core/src/account.rs (24), keeper-sync/src/engine.rs (19), keeper/src/ipc.rs (18)
reason: `init` installs
  `EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))`, and nothing sets
  `RUST_LOG` for the macOS desktop app — the only occurrences in the tree are `info` in
  `gen/apple/project.yml` and the iOS Xcode scheme, plus `keeper-syncd`, which resolves its own
  filter from `--verbose` and its config. A GUI process launched from Finder inherits no shell
  environment. So the effective floor is `info` on every machine a user runs, and `tracing::debug!`
  reaches neither stderr nor `keeper.log`, whatever the user does.
  The count is 127 across `keeper` (42), `keeper-core` (55) and `keeper-sync` (30), and the content
  is what makes this worth an entry rather than a shrug: they are overwhelmingly the line that
  explains a refusal or a fallback — `keeper-note: refused a path outside the vault`,
  `recordings sync: the push policy says not now, so the commits stay local until it does`,
  `recording note: this stub is no longer what keeper composed, so it is kept`,
  `notes: cadence push deferred`, `no synced folders to default to; recording into the plain
  folder`. Every one answers "why did keeper not do the thing I expected", and every one is
  invisible at exactly the moment somebody asks. `GatedMakeWriter` already makes the right call for
  the file leg — `WARN`/`ERROR` are always recorded, `INFO` joins them while debug mode is on — but
  `DEBUG` is filtered out one layer above it, by the subscriber, before the writer is consulted. The
  consequence a user meets is that the Settings toggle called "debug mode" does not enable debug
  logging.
  Cost of the absence, measured rather than asserted: story 44.3 took three field reports and three
  rounds to diagnose. Round 1 was a silent code path; round 2 replaced it with a `tracing::debug!`
  that could not print, which looked like a fix and reproduced the same defect one layer out; round
  3 found this. Any future silent-behaviour bug in any subsystem starts from the same place.
  Not fixed in 44.3 because the fix is a judgement about the whole app and there are at least three
  defensible shapes: (1) `EnvFilter::new("debug")` when `debug.mode` is on — cheapest, matches what
  the toggle's name promises, needs a volume review because 127 sites include hot paths in
  `engine.rs` and `account.rs` and the file leg is unrotated; (2) promote the diagnostic sites to
  `INFO` and leave the floor — precise, no volume surprise, but 127 individual judgements that will
  drift straight back; (3) per-target filtering (`info,keeper_lib::notes_vault=debug`) driven by a
  setting — most control, most machinery. Story 44.3 did (2) for its own module only, with a test
  that pins the floor to the literal `Level::INFO` and names `EnvFilter::new("info")` as its source
  (`keeper-core::notes::default_spaces::no_seed_outcome_reports_below_the_level_the_app_can_print`),
  which is the smallest honest thing one story can do and is not a fix for the other 126.
  Whoever takes this: the test above is the pattern worth copying, and the reason it works is worth
  copying too. Its first version compared each outcome's level against the `REPORT_FLOOR` constant
  alone, and a mutation that *lowered the constant* passed it — a self-consistent floor accommodates
  whatever level someone reaches for. Pinning the literal is what closed it.
status: open

### DW-163: A space's `keeper.limit` is read, shipped to the frontend and written back on save, and nothing has ever applied it — the list window is sized entirely by the request.

origin: found while implementing story 44.4, which applied `keeper.sort` and left its neighbour where it was, 2026-08-09
location: src-tauri/crates/keeper/src/notes_ipc.rs (`SpaceDef.limit`, `project_list`)
reason: `space_def` parses `limit` and clamps it, `notes_spaces` puts it on `NoteSpaceVm`, and
  `notes_space_save` writes it back — but `project_list` sizes its window from `req.limit`
  (`DEFAULT_LIMIT` when zero) and never looks at the space's. So a space that says `limit: 42`
  yields whatever the caller asked for, and the value is a round-tripped decoration.
  This is the identical defect story 44.4 fixed for `sort`, one key over, and it was left alone on
  purpose: 44.4's scope is the icon, the order and the sort, and applying `limit` is a change to how
  many rows every space returns — which interacts with 44.10's virtualisation and 44.11's counts
  (a space capped at 42 makes "how many notes does this select" ambiguous in a way the counts story
  has to answer, not this one). Whoever takes it must decide that first: whether `limit` caps what
  the space *selects* or only what it *renders*, because 44.11's count means different things under
  each reading.
status: done 2026-08-09, story 44.11 (see spec-44-11-counts.md)
resolution: `keeper.limit` caps SELECTION. A rendering cap is incoherent after 44.10 — the viewport
  already bounds rendering for every list, so a second render cap would bound only what can be
  scrolled to, and the transport window already has an owner in `NoteQueryReq.limit`. A selection
  cap is a thing a person can mean and cannot otherwise say ("this space holds the twenty most
  recent"), and it composes with 44.4's sort: the cap is applied AFTER the ordering, so the space
  keeps the twenty the sort put first. The count shows the CAPPED number, because that is what a
  person can reach — and never silently: `NoteListVm.matched` travels beside `total` so the surface
  reads `20 of 347 notes`, and `counts::Selection::report` words the decline at INFO (DW-162's
  floor, pinned to the literal level). The rule lives in `keeper-core::notes::counts` so it is
  provable on Linux; the shell only wires it.
  Two repairs fell out. (1) Unset stopped meaning `MAX_LIMIT`: the old reader mapped an absent,
  zero or negative `limit` onto 500, so "sets no cap" and "caps at 500" were the same value — and
  because the editor round-trips a `limit` it does not render and `notes_space_save` wrote it
  unconditionally, every space saved once grew a `limit: 500` it never had. Zero is now no cap and
  no cap writes no key, the rule `icon` and `order` already followed. (2) The space's cap is no
  longer clamped to the page size, so `limit: 2000` means 2 000 rather than silently dropping 1 500.

### DW-164: The `recorded` sort reads a session's instant out of the stub's `date` and `start` keys, so a recording note whose stamps a person edited by hand sorts by what they typed rather than by the session.

origin: found while implementing story 44.4, 2026-08-09
location: src-tauri/crates/keeper-core/src/notes/sort.rs (`recorded_ms`)
reason: `recording_note::compose` writes the session's local calendar day into `date` and its clock
  into `start`, and there is no single stored instant to read — so `recorded_ms` composes those two.
  That is the honest reading of what is on disk, and it is also the user's own text: a note is the
  user's, and FR-121 says keeper does not overwrite what they wrote. The consequence is that
  `recorded` is only as true as those two keys, and a note whose `date` was retyped sorts by the
  retyped day even though `manifest.json` still knows the real one.
  Not fixed here because the fix is a schema decision rather than a sort decision: it would mean the
  stub writing a machine key (`keeper.recorded`, an RFC 3339 instant beside `session:`) that keeper
  owns and a person is not invited to edit, which changes what `compose` emits for every recording
  note and wants its own story beside 44.2. The sort would then prefer that key and fall back to
  `date`/`start` for every note written before it.
status: open

### DW-165: A note containing a ```mermaid fence CRASHES the editor — `live-preview` supplies a block decoration from a `ViewPlugin`, which CodeMirror refuses, so the `EditorView` throws on construction.

origin: found while wiring story 44.16's `![[….csv]]` table widget into the same decoration set,
  2026-08-09
location: src/components/notes/editor/live-preview.ts (`buildDecorations` mermaid branch, and the
  `ViewPlugin.fromClass` at the bottom of the file that provides the whole set)
severity: **crash, not a quiet no-op.** The three sibling defects this epic found — a scrub bar
  whose `max` was `"0"`, a `/` menu that had never opened, a seed whose log line the app cannot
  print — all shipped green while doing nothing. This one does something: a note with a mermaid
  diagram in it cannot be opened at all. Rank it above the other three.
reason: `livePreview` provides its decorations from a `ViewPlugin`
  (`ViewPlugin.fromClass(…, { decorations: (value) => value.decorations })`). CodeMirror does not
  allow a plugin to contribute a block decoration, and the mermaid branch pushes exactly one:
  `Decoration.replace({ widget: new MermaidWidget(source), block: true })`. So the first time the
  decoration set contains a rendered fence, CodeMirror throws and the view does not build.

  **Reproduction, verbatim, run 2026-08-09 under `bun run vitest`.** Build an `EditorView`
  with a parent in `document.body` and these two extensions, in this order:

  ```ts
  extensions: [
    markdown({ base: markdownLanguage }),                    // @codemirror/lang-markdown
    livePreview({ vaultId: "v", assetUrl: (r) => r, onOpenLink: () => {}, recordingSession: () => null }),
  ]
  ```

  over exactly this document:

  ```
  intro\n\n```mermaid\ngraph TD;\nA-->B;\n```\n\nafter\n
  ```

  The `new EditorView({...})` call throws:

  ```
  RangeError: Block decorations may not be specified via plugins
      at Object.point (@codemirror/view/dist/index.js:2747)
      at RangeSet.spans (@codemirror/state/dist/index.js:3374)
      at new DocView (@codemirror/view/dist/index.js:2922)
      at new EditorView (@codemirror/view/dist/index.js:7915)
  ```

  Dropping `block: true` does **not** fix it: a fence spans several lines, and an inline replace
  over a range containing a line break trades the error for
  `RangeError: Decorations that replace line breaks may not be specified via plugins`. So the fence
  genuinely needs a block decoration and the plugin genuinely cannot give it one.

  **The fix shape.** Move the decoration source off the plugin and onto a `StateField`, which is
  allowed block decorations:
  `StateField.define<DecorationSet>({ create, update, provide: (f) => EditorView.decorations.from(f) })`.
  Not a one-liner, and that is why it is here rather than in 44.16: `buildDecorations` takes an
  `EditorView` and reads `view.visibleRanges`, which a `StateField` does not have — a field sees
  only `EditorState`, so the viewport windowing has to be re-expressed (or the field has to hold the
  whole document's decorations and the viewport optimisation dropped). The plugin also carries the
  wikilink `mousedown` handler, which stays a plugin concern and has to be split out. Three agents
  were editing this file when it was found, mid-wave; a decoration-source migration there is how a
  green tree stops being green.

  **Why the suite is green, and this is the transferable part.** `mermaid-widget.test.ts` drives
  `renderMermaidInto` and constructs `MermaidWidget` directly — thorough about the widget, and it
  never asks the renderer to place one. The one test in the suite that does build a real
  `EditorView` around `livePreview` (`recording-embed.test.ts`, "livePreview, over a recording
  note") loads `livePreview` **without** `@codemirror/lang-markdown`, so `syntaxTree` yields no
  `FencedCode` node, the mermaid branch is never entered, and the decoration is never constructed.
  A suite can be exhaustive about a widget and still never once assemble the thing the user
  assembles. Shipped this way since story 37.8.

  **The test it needs is the reproduction above**, and nothing in the suite currently does it: build
  an `EditorView` with the markdown language AND `livePreview` over a document containing a fence.
status: open

### DW-166: `is:journal` is spelled twice — the index derives the flag from a literal `"journal/"` prefix and story 44.6's creation seed inverts it from its own constant.

origin: found while implementing story 44.6, 2026-08-09
location: src-tauri/crates/keeper/src/notes_vault.rs (`parse_note`, the three
  `rel.starts_with(…)` flag branches) and src-tauri/crates/keeper-core/src/notes/seed.rs
  (`JOURNAL_DIR`)
reason: `parse_note` decides `is:journal`, `is:space` and — until story 44.7 moved it — `is:template`
  from a path prefix written as a bare literal. Story 44.6 needs the INVERSE of that rule: given
  `is:journal`, which folder must a new note be created in for the flag to be true. So the seed
  carries `JOURNAL_DIR`, with a doc comment naming the function it inverts, and `"journal"` is now
  in the source twice. The two cannot drift silently in a way a test catches: the seed's own round
  trip asserts against a hand-built entry, not against `parse_note`, because the `keeper` crate does
  not build on Linux (AD-56).
  Not fixed here because the obvious fix — hoist the constants into `keeper-core` beside
  `default_spaces::SPACES_DIR` and have `parse_note` call one `index::path_flags(rel)` — lands three
  lines away from story 44.7's in-flight change to the `is:template` predicate, which moves that one
  flag off the folder and onto the note's own tag (AD-82). Doing both at once in one wave would have
  been two agents rewriting the same nine lines. It is one constant, one `starts_with` and one
  import once wave 3 has settled.
status: open

### DW-167: A space filtered `is:template` creates a note that says it will not appear, because story 44.6's seed deliberately declines a predicate story 44.7 was moving at the same time.

origin: found while implementing story 44.6, 2026-08-09
location: src-tauri/crates/keeper-core/src/notes/seed.rs (`seed_flag`)
reason: The creation seed makes a new note satisfy `is:pinned`, `is:archived`, `is:capture` and
  `is:journal`, and declines every other `is:` flag with a stated reason. `is:template` is the one
  entry in that table whose reason is temporal rather than principled: while 44.6 was being written,
  44.7 was moving the predicate from the `templates/` folder prefix onto the note's own `template`
  tag. Seeding a folder would have been seeding against a rule that no longer exists, and seeding
  the tag would have been seeding against a rule that did not exist yet.
  The consequence today: New Note in a space filtered `is:template` writes a plain note at the vault
  root and reports "A new note can't satisfy is:template, so this note is in the vault but won't
  appear in <space>." That is honest and it is not right — creation CAN satisfy it now, by adding
  the `template` tag, which is exactly what `seed_term`'s `tag:` arm already does for every other
  tag.
  The fix is one arm in `seed_flag`: `template` sets the `template` tag. `seed::verdict` needs no
  change at all — it re-runs the real query over the real bytes, so it will simply stop declining.
  A test exists to flip: `a_space_of_spaces_does_not_manufacture_a_broken_space` is its sibling and
  shows the shape.
resolution: **Closed 2026-08-09, in story 44.6, before its spec shipped.** The deferral was
  temporal, and the reason expired inside the same wave: 44.7 landed while 44.6 was in its
  verification pass, and `notes_vault::parse_note` now reads the predicate through
  `templates::is_template` off the note's frontmatter tags (with `templates/` grandfathered). So
  `seed_flag` gained the arm it was always going to get — `is:template` seeds the tag
  `templates::TEMPLATE_TAG`, never a folder — and a space filtered `is:template` now makes a new
  template, which is the only thing that ask can mean.
  `seed::verdict` needed no change whatsoever, which is worth recording because it is the design
  paying out: the verdict re-runs the real query over the real bytes through the reconciler's own
  parser, so adding a seed arm made it stop declining without anybody teaching it a new rule. A
  per-flag "can we satisfy this?" table would have needed editing in two places and could have
  disagreed with itself.
  Two tests (`a_template_space_creates_a_template`,
  `a_template_space_that_also_names_the_tag_adds_it_once`) and two mutants: M9 removes the arm and
  kills the first; M10 removes `add_tag`'s de-duplication and kills the second, because two
  producers now reach the seed's tag list and a duplicate there is a duplicate in the note's
  frontmatter.
status: closed

### DW-168: `IndexEntry.fields` flattens a nested frontmatter map into one string, so the space DSL's `field:` predicate cannot address `keeper.from_template` — or any other sub-key.

origin: found while implementing story 44.8, 2026-08-09
location: src-tauri/crates/keeper/src/notes_vault.rs:1379-1384 (`parse_note`'s field loop),
  src-tauri/crates/keeper-core/src/notes/frontmatter.rs (`FieldValue::index_string`),
  src-tauri/crates/keeper-core/src/notes/index.rs (`RESERVED_FIELD_PREFIX`)
reason: The reconciler stores every frontmatter key through `FieldValue::index_string()`, and a
  one-level map renders as its pairs joined by `FIELD_LIST_SEPARATOR`. So story 44.7's
  `keeper: { from_template, from_template_id }` lands in the index as ONE entry —
  `fields["keeper"] = "from_template: templates/journal.md\nfrom_template_id: 01J…"` — and there is
  no `fields["keeper.from_template"]` for `field:` to match. Two consequences. First, nobody can
  write a space for "every note made from the journal template", which is the obvious next thing to
  want and is one index change away. Second, `RESERVED_FIELD_PREFIX`'s own doc — "a user's top-level
  `keeper:` map indexes under the bare key `keeper`, so this prefix can never shadow something they
  wrote" — was written when the reserved namespace held only keeper-computed values
  (`keeper.origin`, `keeper.device`, `keeper.touched`); now that keeper WRITES a `keeper:` map into
  the note file, the reserved namespace and the user's own possible `keeper:` key are
  indistinguishable at the index layer.
  Story 44.8 works around it rather than changing the reconciler: `template_update::provenance_from_index`
  inverts that one flattening for its own two keys, with a round-trip test against the real renderer.
  That keeps the finder at zero file reads on a ten-thousand-note vault and touches nothing else.
  The general fix — index a one-level map as `parent.child` entries alongside (or instead of) the
  bare key — changes what `field:` means for every existing query in every vault, so it is a story
  with a migration question in it, not a rider on 44.8.
status: open

### DW-169: `notes_restore_revision` now exists and works for any note, and only two surfaces can reach it.

origin: found while implementing story 44.8, 2026-08-09
location: src-tauri/crates/keeper/src/notes_ipc.rs (`notes_restore_revision`),
  src/components/notes/note-history-panel.tsx, src/components/notes/template-update-offer.tsx
reason: Story 44.8 needed "accepting is undoable through the existing note history" to be true, and
  found that the existing note history could show a revision and could not act on one — there was no
  restore command anywhere in the app. It added one: `git show <rev>:<path>` into the ordinary write
  path, so a restore is itself a revision and undoing an undo is free.
  It is reachable from exactly two places: the history panel's `Restore this version`, and a template
  update's per-note `Undo`. Three other surfaces are asking for it and predate it. The conflict
  resolver can take theirs or take mine and cannot take "the one from before this morning". The
  editor's "This note isn't on disk any more" banner tells the user their text is still in the buffer
  and says nothing about the revision that still holds the file. `.keeper/trash/` holds every deleted
  note (NFR-30) with no way to bring one back except a Finder window. None of these is a bug in 44.8;
  they are three places where a verb that now exists has not been offered, and the reason to record
  it is that "keeper never loses your bytes" is only as true as the number of doors back.
status: open

### DW-170: Story 44.6's shell wiring has never been through a compiler, on any machine — and neither a green test suite nor a green mutation sweep can tell you that.

origin: found while re-verifying story 44.6 after the wave-3 `/tmp` harness incident, 2026-08-09
location: src-tauri/crates/keeper/src/notes_ipc.rs (`create_for_space`, `SpaceForCreate`,
  `space_source`, and the seed application inside `create_note`) and
  src-tauri/crates/keeper/src/notes_vault.rs (`index_written`)
severity: **unknown, which is the point.** These are type questions, and no test in this repo can
  reach them for this crate. A single wrong tuple order in `Frontmatter::parse`'s destructuring, or
  a `FileStat` field that does not exist, is a build break on the owner's machine and invisible
  here.
reason: `cargo check -p keeper --lib` dies on this host inside `glib-sys`'s build script — there is
  no `pkg-config` — before it reaches a single line of keeper's own source. So the five symbols
  above have never been type-checked anywhere. This is not new to 44.6 (AD-56 says the shell is
  untestable on Linux and every wave-3 story says so in its spec), but 44.6 is the sharpest
  instance: it is the only wave-3 story whose new shell code is on the **write path**, so a
  compile-time mistake there is a create that fails rather than a surface that renders oddly.
  The general lesson, which W3Csv reached independently from a `tsc` error that survived a green
  vitest suite AND a green 15/15 mutation sweep for hours: **a green test suite plus a green
  mutation sweep is not a green build.** vitest transpiles without typechecking; `cargo test`
  compiles only the crates it runs; and mutation testing cannot help with either, because a mutant
  only probes behaviour the tests already assert. Both systems answer "does the code do the right
  thing". Neither answers "does the code compile". On wave 3 the compile question was the one
  nobody owned.
  Mitigated as far as it can be from here: everything decidable was pushed into `keeper-core`
  (`notes::seed` 27 tests, `notes::query` 45), and what is left in the shell is one read, one call
  and one `push` per branch. But small is not compiled.
evidence, and it is not theoretical: **hand-auditing the symbol list above found a real compile
  error in code that had already passed every test in the story.** A refusal was built with
  `IpcErrorCode::InvalidInput`, a variant that does not exist — notes refusals use `NotesInvalid`.
  A compiler says that in milliseconds. 444 green Rust tests and 443 green frontend tests said
  nothing, because none of them can reach this crate. Fixed at the site, with a comment naming this
  entry so the next reader knows why a typo like that survived to be found by eye. The remaining
  symbols were then checked against their definitions rather than against memory and are clean.
  The same audit found a data-loss path this story made reachable, now closed in-story: `atomic_write`
  overwrites unconditionally, and `siblings` reports an UNREADABLE directory as an EMPTY one, so a
  folder keeper cannot list yields a filename keeper believes is free. Unreachable before 44.6
  because `NoteCreateReq.dest` was a field every caller passed `None` for — the fifth dead value
  this epic has found — and the space seed is the first thing to set it. `create_note` now refuses
  when the path it is about to write already exists.
what closes this: `cargo check --manifest-path src-tauri/Cargo.toml -p keeper` on the macOS host,
  plus the six-step smoke test in section 11 of spec-44-6-new-note.md — create from the rail, from
  Pinned (assert `pinned: true` on disk), from Journal (assert the `journal/` path), from
  Recordings (assert BOTH the on-screen notice and the `INFO` line in Console.app, because DW-162
  is the story of a decision that only existed in a `debug!` the packaged app cannot print), twice
  from Inbox (the second create clears the first's notice), and the overwrite backstop —
  `chmod 000` a seeded subfolder, create into that space, and confirm keeper refuses AND that the
  file already there is byte-identical afterwards. That last one is the only item about data rather
  than display, and byte equality is the check: a note replaced with a blank one looks exactly like
  a note that was created.
status: open

### DW-171: Story 44.16's shell wiring has never been through a compiler either — and unlike 44.6's it writes over a file the user already has.

origin: filed alongside DW-170, applying the same standard to 44.16 after W3NewNote argued a
  one-line AD-56 caveat is too quiet for write-path code, 2026-08-09
location: src-tauri/crates/keeper/src/notes_ipc.rs (`csv_path`, `read_csv`, `notes_csv_read`,
  `notes_csv_set_cell`) and the two lines added to `keeper::lib`'s `invoke_handler`
severity: **unknown, and the blast radius is larger than DW-170's.** 44.6's uncompiled code
  *creates* a note: a mistake there is a create that fails, and nothing the user had is lost.
  44.16's uncompiled code *overwrites an existing file in a synced vault*. A mistake there is
  somebody's export rewritten and committed, on every machine the vault reaches. Nothing was
  observed wrong; nothing could have been, which is the entry.
reason: `cargo check -p keeper` dies on this host inside `glib-sys`'s build script for want of
  `pkg-config`, before reaching a line of keeper's own source (measured, not assumed). So these
  four functions have never been type-checked anywhere.

  The specific questions a compiler would settle, none of which a test in this repo can reach:

  | Symbol | The type question |
  | --- | --- |
  | `csv_path` | `note_protocol::contained_read` returns `Option<PathBuf>`; the `.filter(\|candidate\| candidate.is_file())` closure takes `&PathBuf`, and the `for rel in candidates` loop moves each `String` before `Ok((rel, path))` returns it. |
  | `read_csv` | `&PathBuf` coercing to the `&Path` parameter; `std::fs::metadata(path)?.len()` as `u64` against `csv::MAX_CSV_BYTES`. |
  | `notes_csv_set_cell` | `csv::set_cell` takes `usize`, the command receives `u32`; `rel: String` is borrowed by `write_vault_file` and then **moved** into `csv::project` — an ordering a compiler checks and a reader can talk themselves into. |
  | both commands | `CsvError` mapped to `IpcError` by hand rather than through `notes_error`, so `NotesError` gained no variant — nothing enforces that the mapping stays exhaustive if `CsvError` grows one. |
  | the tracing calls | `tracing::info!(%rel, row, column, "…: {error}")` relies on implicit captured identifiers in the message; `rel` is a `String` behind `%`. |

  Mitigated as far as it can be from here, on AD-55/AD-56's own argument: every decision, every
  byte of the parser and every user-facing sentence is in `keeper-core` and is proved there
  (`notes::csv` 21 tests, 16 mutants, 16 caught). What is left in the shell is resolution, IO and
  error mapping — one read, one compare, one write. But small is not compiled, and the general
  lesson DW-170 records applies unchanged: a green test suite plus a green mutation sweep is not a
  green build, because neither answers whether the code compiles.
what closes this: `cargo check --manifest-path src-tauri/Cargo.toml -p keeper` on the macOS host,
  plus a smoke test that must include the cases the type system cannot reach:
  1. Embed `![[data.csv]]` for a file dropped into `attachments/` — the bare-name candidate is the
     branch a vault-relative path never exercises.
  2. Edit one cell, then **compare the file on disk byte for byte against a copy taken before the
     edit**. Only that cell's bytes may differ. A visual check of the table cannot see a rewritten
     line ending or a dropped BOM, which is the entire promise of this story.
  3. Open the table, enter a cell, change nothing, blur. The file's mtime must not move, and
     Console.app must carry the `INFO` line "csv cell unchanged, nothing written". **Both halves.**
     DW-162 is the story of a decision that existed only in a log the packaged app cannot print;
     checking the screen and not the log would repeat it exactly, and this is a path whose whole
     observable behaviour is that nothing happens.
  4. Edit a cell in a file with a BOM and CRLF terminators, then confirm the sync engine commits it
     — `mark_dirty` reaching the commit cadence is the one claim in this story that no test on any
     platform covers.
  5. Confirm a file over 4 MiB refuses with the sentence naming both sizes rather than hanging.
status: open

## DW-N1 — the editor caret opens in front of the frontmatter block

**Status:** done 2026-08-04, by the design correction below. **Found:** 2026-08-03,
agent-desktop XFCE run, Phase 5 smoke test.
**Severity:** cosmetic on disk, confusing on screen. Not data loss — see the guard below.

**What happens.** Open a freshly created note and type: the first keystroke lands at document
offset 0, which is *in front of* the opening `---`. The buffer becomes
`<typing>---\nid: …\n---\n` and the block is no longer frontmatter.

**What stopped it hurting until it was fixed.** `notes_ipc::restore_block` re-attached the base's
block on every save, so the file on disk was always valid: `id` and `created` survived, the note
stayed openable by its id, and Obsidian read it. The residue was a duplicated block sitting in the
note's *body*, which the user then had to delete by hand.

**What was tried.** `notes_open` sends the body offset as `NoteBodyBatch::Reset.cursor` (verified
in the log: `cursor=Some(129)` for a 130-byte note), the store carries it, and `note-editor.tsx`
applies it both at `EditorState.create` and in the reconcile effect. The caret still lands at 0,
so something after those two points resets the selection — the `mode === "edit"` focus effect and
CodeMirror's own initial-focus behaviour are the two candidates, neither yet ruled out.

**The fix worth making instead.** Stop putting frontmatter in the editor buffer at all. FR-107 and
UX-DR40 already say the block renders as a typed *properties panel*, not as source in the editor:

- `notes_open` sends the BODY only (`&text[body_offset..]`), and `cursor` becomes redundant.
- `notes_save` re-attaches the block — which `restore_block` already does, so the repair path
  becomes the normal path.
- `properties-panel.tsx` needs the whole document rather than the buffer, so the body channel
  grows a `frontmatter` field or the panel gets its own read.
- Every `NoteBodyBatch` variant carrying text (`External`, `Diverged`) needs the same treatment.

That is a genuine design correction, not a patch: it makes typing above the block *unrepresentable*
rather than repaired, and it deletes the caret-hint machinery.

**resolution (2026-08-04).** Done as written, plus the two things the sketch had not accounted for:

- `NoteBodyBatch::Reset | External | Diverged` each carry `frontmatter` beside the body; the body is
  `&text[body_offset..]` and holds no fence at all. `split_note` / `join_note` in `notes_ipc` are the
  only seam, and they are exact inverses (tested byte for byte, BOM and blank line included).
- `notes_save(subscription_id, text, base_rev, frontmatter: Option<String>)`: `text` is the body,
  `frontmatter` is `Some` only for the properties panel, and absent it the block keeper last
  delivered stands. `restore_block` and its heuristic are deleted — a body that opens with `---`
  (a thematic break on line one) is body text now, where the old repair would have mistaken it for
  frontmatter and dropped the note's `id`.
- `NoteWriteVm` gained `frontmatter`, because every save stamps `updated` and the panel would
  otherwise render the timestamp it sent rather than the one that landed.
- `NoteConflictChoiceReq::Merged.text` is a body too; `notes_resolve_conflict` re-attaches the
  canonical note's block, so a hand-merge cannot cost the note its identity either.
- `cursor` was NOT deleted. It stopped being a workaround and became the thing it was documented as:
  a template's `{{cursor}}`, which `templates::expand` has always computed and both create paths
  threw away (`let (expanded, _cursor)`). It is now stashed per note id at create and **taken** by
  `notes_open`, which is the PRD's "creating from a template places the caret at the caret
  placeholder, or at the end of the document when none is present" — an acceptance that had never
  actually held. Bounded at 256 unopened hints.

Verified: `typing_at_the_first_offset_cannot_disturb_the_block` drives the real `create_note` and the
real save assembly against a real file on disk and asserts the saved note has exactly one block, one
`id`, a surviving `created`, and the keystroke at the top of the body. 236 keeper tests, 1755
frontend tests, workspace clippy clean. **Not** exercised: a human typing in the running app — Xvfb
cannot start in this container (the X server hardcodes `/usr/bin/xkbcomp`, which is absent and
unwritable).

deployed 2026-08-04 to hesperia (macOS 26.5.2, arm64) with the new `scripts/install-macos.sh`, built
from these exact sources (the four changed files' checksums matched across the rsync). The app boots,
restores both accounts, syncs both folders and renders the Notes surface. The typing check still did
not happen there either, for a different and better reason: hesperia has **no vault flagged** ("No
notes vault yet. Flag a folder you already sync and it becomes one."), and which of someone's synced
folders becomes a vault is their decision, not a smoke test's. One flagged folder and one typed
character closes this.

## DW-N2 — `recorder::tests::fetch_request_screen_recording_round_trips_the_fake_sidecar` is flaky under parallel test threads

**Status:** open. **Found:** 2026-08-04, running `cargo test -p keeper --lib` on Linux.
**Severity:** a false red in CI and in the pre-push hook. No product defect.

**What happens.** The test writes a `#!/bin/sh` fake sidecar with `FakeSidecar::write` and executes
it immediately. With the default test-thread fan-out the execve raced the writer's own file handle
and failed with `Text file busy (os error 26)`; the same test passes every time with
`--test-threads=1`. Two `FakeSidecar::write` calls in the same loop (`request-true`, `request-false`)
make the window easy to hit on a slow container filesystem.

**The fix.** Close the file explicitly (drop the handle) before `execve`, or `fsync` and re-`stat` it,
in `FakeSidecar::write` — one place, and every sidecar test inherits it. Not done here because it is
in `recorder.rs`, which this change does not touch, and a test-harness fix deserves its own diff.

## DW-N3 — a finished recording reports "Saved 0 segments · 0 MB"

**Status:** open. **Found:** 2026-08-04 on hesperia, driving two real sessions through the app for
story 22.7's echo measurement.
**Severity:** cosmetic, but it reads as data loss to the person who just recorded something.

**What happens.** Stop a session that wrote exactly one segment and the completion card says
`Saved 0 segments · 0 MB` while pointing at a folder that holds a perfectly good 583 KB
`audio-0000.m4a`. Every single-segment recording says this.

**Why.** By the Story 17.1 contract the FINAL segment deliberately emits no `segmentClosed` — while
the host is Stopping that would be an illegal transition, and `finalized` is its closure signal
instead (`Capture.swift` `finishAndExit`). The host counts `segmentClosed` events, so a session
whose only segment is the final one counts zero. A session long enough to rotate reports N-1.

**The fix.** The host should reconcile the session folder at `finalized` rather than trust the event
count — the terminal disk reconcile already exists for the recovery path (Story 17.3), so this is
about calling it on the ordinary stop path too, or having `finalized` carry the final segment's path
and bytes. Not done here because it is host-side accounting in `recorder.rs`/`ipc.rs`, orthogonal to
22.7's audio path, and it deserves its own diff and its own test.

## DW-N4 — the in-app updater is broken for every install, and publishing v0.6.5 is what broke it

**Status:** open, needs an owner decision (key placement or release policy). **Found:** 2026-08-05,
owner clicked Update and got `Update failed: Could not fetch a valid release JSON from the remote`.
**Severity:** every installed copy of keeper, on every version. The update button cannot succeed.

**What happens.** The app fetches the endpoint baked into `tauri.conf.json`
(`plugins.updater.endpoints`): `https://github.com/tgorka/keeper/releases/latest/download/latest.json`.
Verified from hesperia — it returns **HTTP 404, `Not Found`, 9 bytes**. The Tauri updater plugin
cannot parse that as a manifest, so it reports the string above. The message is accurate and useless:
it names the parse, not the missing file.

**Why, and it is a regression rather than a misconfiguration.** `latest.json` is attached to
v0.4.2, v0.5.0 and **v0.6.1**, each beside its `keeper_<ver>_aarch64.app.tar.gz` and `.sig` — those
releases came out of CI when `tauri-action` still ran, so the update channel used to work.
Publishing **v0.6.5** moved `/releases/latest/` to a release whose only assets are the four
`keeper-syncd-*` binaries, and `latest/download/latest.json` resolves against the newest
non-prerelease release only. So an incomplete release did not merely fail to improve the channel —
it took the working one away from users still on 0.6.1.

**Why CI cannot fix it.** The release workflow's second step, `Apple signing material must be
present`, fails: the repository has exactly two secrets — `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — and none of the seven Apple ones. The job therefore dies
before `tauri-action` runs, which is what would have produced the dmg, the tarball, the `.sig` and
`latest.json`. And the gate cannot be satisfied as written: three of its seven secrets are
notarization credentials from a **paid** Apple Developer Program, which decision D-1
(`docs/constraints-and-limitations.md`) says this project does not have. As written, that job can
never pass, so no release can ever be produced by CI.

**Why a local build cannot fix it either.** `scripts/build-macos-signed.sh` deliberately passes
`createUpdaterArtifacts: false`, because signing the updater payload needs the minisign private key
whose public half is baked into every build (fingerprint `76BB33F2B735A5E9`, committed in `4b6b60f`
"Provision real updater keypair"). That key exists **only** as a GitHub Actions secret — it is not in
the 1Password vault this fleet uses and not on hesperia. A secret only CI can read, in a CI job that
can never run, is a key nobody can use.

**The three ways out, and the recommendation.**

1. **Relax the gate to signed-but-not-notarized, and give CI the certificate.** The gate's own
   argument is about *ad-hoc* signatures: an ad-hoc identity is a `cdhash` that changes every build,
   so TCC grants and keychain items break. A free **Apple Development** certificate has an
   identity-based designated requirement and does not have that defect — it is what
   `build-macos-signed.sh` already verifies and what every recent local build ships. Notarization
   only removes the first-open right-click, which every release note has told users to do anyway. So
   require the four certificate secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
   `APPLE_SIGNING_IDENTITY`, `KEYCHAIN_PASSWORD`), drop the three notarization ones from the gate,
   and CI can build, sign, and emit the updater artifacts with the key it already holds.
   **Recommended:** it restores the channel, keeps the ad-hoc ban, and needs no new key material
   beyond exporting the existing certificate as a `.p12`.
2. **Put the updater private key where a human can read it** (the 1Password vault), and produce the
   artifacts from the Mac. `~/keeper-release-dmg.sh` on hesperia already does the whole sequence when
   `TAURI_SIGNING_PRIVATE_KEY` is exported: build signed, tar the `.app`, `tauri signer sign` it,
   compose `latest.json`, upload all three. Note this contradicts `docs/release.md`, which mandates
   the key exist only in GitHub secrets — a rule worth revisiting on its own, because a key that
   exists in exactly one unreadable place cannot be rotated or recovered.
3. **Immediate mitigation, one field:** mark v0.6.5 a pre-release. `/releases/latest/` skips
   pre-releases, so the endpoint resolves to v0.6.1 again and the error disappears — an installed
   0.6.5 then reads "up to date" (0.6.1 is older, so nothing is offered) and an installed 0.6.1 sees
   its own version. Honest about nothing being installable, and reversible the moment (1) or (2)
   lands.

**Also worth fixing whichever way this goes:** the app should not hand the plugin's parse error to
the user. "Could not fetch a valid release JSON from the remote" describes the parser's disappointment;
what the person needs is "this release has no update manifest" or "no newer version is published".

- source_spec: `_bmad-output/implementation-artifacts/spec-40-1-the-path-template-rendered-purely.md`
  summary: Story 40.3's pre-flight must refuse a rendered path that lies inside an existing session
    folder, because two different titles can nest one session in another under a template with a
    collapsible interior component.
  evidence: Measured against the committed renderer — `{yyyy}/{slug}/{mm}-{dd}` renders `2026/08-05`
    for an untitled recording and `2026/08-05/08-05` for one titled `08-05`. The paths differ, so no
    collision ordinal fires, yet the second session's media is written inside the first session's
    folder and deleting either deletes both. The intent contract explicitly permits interior
    components to collapse, so this cannot be closed in 40.1's renderer; it is a pre-flight check
    where the filesystem is actually consulted.

- source_spec: `_bmad-output/implementation-artifacts/spec-40-1-the-path-template-rendered-purely.md`
  summary: Nothing bounds a template's depth or the total length of a rendered path, so a template
    can direct story 40.3 to create arbitrarily deep trees or paths Windows cannot open.
  evidence: `PathTemplate::parse` accepts a template of 60 `/` separators, which would have 40.3
    create 60 nested directories; and `{yyyy}/{mm}/{dd}/{HH}/{MM}/{SS} {title}` with a long title
    exceeds Windows' 260-character `MAX_PATH` without long-path opt-in — on a feature whose premise
    is that the destination may be a synced or removable volume. The 255-byte cap this story adds is
    per component and says nothing about the total.

- source_spec: `_bmad-output/implementation-artifacts/spec-40-1-the-path-template-rendered-purely.md`
  summary: Invisible codepoints outside Unicode category `Cf` can still name a recording folder, so
    the "no folder the user cannot see" rule is only as wide as `Cf` — widening it is a decision
    about which scripts stay usable, not a bug fix.
  evidence: Story 40.1 added `Cf` to the illegal set and the table was verified complete for Unicode
    16.0, but measured against `{title}/{yyyy}`: HANGUL FILLER `U+3164` (category `Lo`) renders
    `ㅤ/2026`, BRAILLE PATTERN BLANK `U+2800` (`So`) renders `⠀/2026`, and a private-use `U+E000`
    (`Co`) renders a tofu box — each a top-level directory in the destination root that the user
    cannot see in Finder, type in a shell, or delete by name, from a title that may arrive from a
    bridge or an agent. Variation selectors (`U+FE00`–`U+FE0F`, `Mn`) are in the same class.
    Excluding `Lo` fillers wholesale would refuse characters that are legitimate in some scripts, so
    the set to refuse is a product decision rather than a one-line widening.

## Review passes recorded

### Epic 34 — the thirteen unreviewed stories, audited 2026-08-07

Thirteen epic-34 stories shipped in releases 0.6.2 and 0.6.3 without a code-review pass. They were
audited on 2026-08-07 in four parallel read-only layers. Each layer read the story's frozen
`<intent-contract>` and I/O matrix, located the implementing code and the proving test for every
acceptance criterion, and judged the one question that matters: would that test FAIL if the
behaviour were removed?

- **durability** — 34-9, 34-11, 34-13. 34-11 `correct`; 34-9 and 34-13 `incorrect`.
- **visibility** — 34-5, 34-6, 34-8, 34-10. 34-6 `correct`; 34-5, 34-8 and 34-10 `incorrect`.
- **credentials** (security review) — 34-4, 34-7, 34-12. All three `incorrect`.
- **window chrome** — 34-1, 34-2, 34-3. 34-2 `correct`; 34-1 and 34-3 `incorrect`.

As returned by the layers that is 3 `correct` and 10 `incorrect`. The fix batch was briefed as 2 and
11; the difference is 34-2, which the window-chrome layer passed while still recording DW-157,
DW-158 and DW-159 against it. Noted so the discrepancy is not rediscovered as a third number.

**Fixed in this batch, and therefore NOT in the ledger:** story 34-9's blocking wake finding (the
watch wake was set for any delivered event, so tier 0 could not suppress the 1 Hz walk it was added
to prevent) together with the `EchoSuppressor` module-doc claim and its misleading test; story
34-13's blocking discriminator (the guard asked whether a blob parses as a pointer rather than
whether the path is routed through LFS) and the empty-blob classification; the visibility layer's F1
(a failed pass stranded `phase` in a busy state, leaving a bar and a frozen transfer rate on screen
for a folder that had permanently stopped) and F4 (the `Pushing` phase carried no rate); and the
credentials layer's three: the LFS ssh credential cache surviving `remove_profile`, the add path
destroying the typed token before attempting to store it, the `remove_profile` rustdoc naming a
`keeper-syncd` caller that does not exist, plus a supersession marker on spec-34-4, whose contract
34-12 deliberately overrode.

**Not fixed, filed here:** DW-132 through DW-161. Every other finding the four layers returned, plus
three residuals the fixes themselves surfaced and their authors handed over — DW-137 (the wake filter
cannot consult `.gitignore`, so `target/`/`dist/`/`build/` still wake the engine every tick), DW-138
(`EchoSuppressor` keeps zero production callers) and DW-161 (F4 was resolved by removing the false
claim that the push leg carries a rate, which leaves the push leg a documented exception to AD-34-13
rather than an accident). Two findings were deliberately not filed because the ledger already owned
them — DW-126 covers 34-6's `ensure_activity_columns` read-then-`ALTER` race, and DW-118 covers the
56 px webview overhang that keeps story 34-2's drawer footer partly off-window regardless of its
scroller fix. DW-120 and DW-121 are cited by the durability layer as known and are not re-reported;
DW-116 and DW-117 were checked and are genuinely closed by stories 34-5 and 34-10.

The pattern worth carrying forward, because it is the same one that produced the 34-14..34-19 group's
holes: of the thirteen, only 34-11 tested its claim where the risk actually lives. Its
`keeper-syncd/tests/durability_matrix.rs` spawns the real daemon, SIGKILLs it, and validates that its
own fixture reached the interrupted states it claims to cover. Everywhere else the pure core is
tested well and the impure shell is tested by a manual procedure the specs record as unrun — which is
why DW-135, DW-139, DW-146, DW-151, DW-156 and DW-157 are all the same entry wearing different
clothes: a load-bearing token that a green suite cannot see.
### DW-172: Two more notes event listeners are declared and subscribed by nothing — `listenNotesShowUnread` and `listenNotesCaptureShown`.

origin: found while fixing the third of the trio in story 44.6, 2026-08-09
location: src/lib/ipc/client.ts (`listenNotesShowUnread`, `listenNotesCaptureShown`), against
  `keeper/src/notes_ipc.rs`'s `NOTES_SHOW_UNREAD_EVENT` and `NOTES_CAPTURE_SHOWN_EVENT`
reason: Story 44.6 found that `listenNotesOpenNote` was exported from the IPC client and called
  from nowhere, so the tray's New Note created a note, raised the window, and showed the user
  whatever had been on screen. That one is fixed (`useNotesOpenNote`, mounted in `App.tsx`, with the
  assertion in `App.test.tsx` rather than in the hook's own suite — see below). Its two siblings are
  still in that state: Rust emits both events and no webview code subscribes to either.
  The consequence for each, stated rather than assumed: choosing the tray's unread line is supposed
  to open the notes view with the origin filter armed (FR-113) and currently raises the window and
  does nothing else; the capture-shown event is supposed to let the main window react when the
  panel appears and currently informs nobody.
  Not fixed in 44.6 because neither is the creation path — they belong to Epic 36's tray and Epic
  37's capture panel, and a story that fixed all three would be three stories wearing one spec.
  `useNotesOpenNote` is the pattern to copy; it is ~40 lines including the graceful no-Tauri-host
  branch.
severity: the quiet kind. **Nothing fails**, which is why this survived two epics. It is the shape
  W3TemplateUpdate named while 44.8 was landing: a dead value is not always an armed hazard, it is
  sometimes a promise the app has been making and cannot keep, and that kind is harder to find
  because there is no error anywhere.
the testing lesson, which is the reusable part: a hook suite that does `renderHook(() => useThing())`
  proves the hook works and can NEVER distinguish a mounted listener from an unmounted one. That is
  exactly how this class of defect hides. The assertion has to live where the component tree is
  really rendered — for 44.6 that is `App.test.tsx`, and the mutation that deletes the call from
  `App.tsx` fails it. `use-notify-navigate.test.ts` has the same hook-only shape and is the reason
  nobody noticed the notes trio; worth the same treatment when someone is next in that file.
status: open

### DW-190: Comments anchored to parts of a PDF need a PDF renderer keeper does not have.

origin: story 45.21, 2026-08-10 — the speculative half of the story, decided before building
location: `src/components/viewers/document-viewer.tsx` (`PdfBody`), against
  `src-tauri/crates/keeper/src/file_protocol.rs`'s `keeper-file://`
reason: 45.8 renders a PDF as `<embed type="application/pdf">` routed to the platform's own
  renderer (PDFKit under WKWebView), which is why a 400-page document costs one element and no
  marshalling. **That choice also puts the document permanently out of JavaScript's reach**, and
  this was measured rather than assumed. WebKit's own
  `Source/WebCore/html/HTMLEmbedElement.idl` exposes exactly `align`, `height`, `name`, `src`,
  `type`, `width` and `getSVGDocument()` — there is **no `contentDocument` and no
  `contentWindow`**, unlike `<iframe>` and `<object>`, which have both (confirmed independently
  against a spec-conformant DOM: `"contentDocument" in element` is `false` for `embed` and `true`
  for the other two). `getSVGDocument()` is same-origin-guarded and answers only for SVG; a PDF is
  not SVG, and `keeper-file://` is a different origin from the app document anyway.
  So: no document, therefore no selection (a `Selection` belongs to a document), therefore no
  coordinates and no page events. An overlay `<div>` can capture a drag, but keeper cannot read the
  embed's scroll offset or current page, so a rectangle drawn on it is pinned to viewport
  coordinates of a document whose position keeper cannot observe — it drifts the moment the reader
  scrolls, and drifts silently.
  The epic's own fallback, "anchor to a page and a rectangle", is therefore half impossible: the
  rectangle cannot be drawn meaningfully and the page number could only ever be one the reader
  TYPES. What would be left — a comment list with a typed page number — was deliberately not built,
  because a paragraph of prose about a document anchored by a number a person typed **is a note**,
  keeper already has notes, and a second prose store beside the file would be a second write path
  for prose with its own sidecar format, its own conflict semantics and an anchor keeper cannot
  verify. That is what AD-88 and AD-89 exist to refuse.
what it would take, in order:
  1. a PDF renderer that runs in JavaScript (pdf.js or equivalent) — a new dependency of
     substantial size, a re-opened zero-egress review, and a canvas plus a text layer per visible
     page with a virtualiser to bound it, replacing an `<embed>` that costs one element;
  2. an anchor format that survives the document being replaced — a page and a rectangle does not,
     because re-exporting re-paginates; the state of the art is a text-quote anchor
     (prefix/exact/suffix, à la W3C Web Annotation) with a page+rect fallback, which needs the text
     layer from (1);
  3. a store with sync semantics — where comments live, what a conflict does, what a rename does;
  4. only then the overlay, the gutter and the show/hide toggle.
severity: none today — nothing is half-built, no control is offered and no comment can be lost.
  The hazard is a future story adding a comment button to `DocumentViewer` without reading this:
  it would anchor to a scroll position nothing can observe, and the drift would be silent.
the reusable part: the export half of 45.21 shipped and this half did not, and the difference was
  measuring the platform before designing against it. Ten minutes reading WebKit's IDL replaced an
  epic's worth of speculation about what an overlay could anchor to.
status: open

### DW-191: A default space keeper stood down because the user already had that name is never recorded, so deleting the user's space brings keeper's version back.

origin: story 45.17, 2026-08-10 — found by W3TagsDelete while proving the delete/restore path,
  and deliberately not fixed inside that story
location: `src-tauri/crates/keeper-core/src/notes/seed.rs` (the seeded-key ledger),
  against 44.3's five default spaces and 45.17's `notes_spaces_restore_defaults`
reason: `seed` writes a ledger entry for each default space it CREATES, and 45.17's delete path
  reads that ledger to decide whether a deleted default stays deleted. A default that was never
  created — because the vault already contained a space with that name — leaves no entry. The
  user's own space is therefore not protected by the mechanism that protects keeper's: delete it,
  and the next seed sees a name that is absent from both the vault and the ledger and creates
  keeper's version in its place. The user deletes their space and gets a different one back,
  which is the exact surprise 44.7 refused for templates.
  One line in `seed` closes it — record the key whether the space was written or stood down —
  but that CHANGES 44.3's stated semantics, from "the spaces keeper made" to "the names keeper
  has claimed", and it changes them for vaults that already exist. That is a product call about
  an upgrade path, not a bug fix inside a story about deleting things, which is why it is here
  rather than in 45.17.
status: done 2026-08-11
resolution: resolved by story 47.4 — `seed` records a default's key whether it was written or stood down for a name collision, so deleting the user's own space no longer brings keeper's back. The same defect in `templates.rs` was found while fixing this one and fixed in the same story.

### DW-192: `notes_capture_impact`, the two recording commands and the capture rung are wired but have never been exercised by a running app.

origin: epic 45 wave 3, 2026-08-10 — raised independently by W3CaptureTag, W3Recording and
  W3CaptureWindow in their stand-down reports
location: `src-tauri/crates/keeper/src/notes_ipc.rs` (`apply_settings`'s two capture arms,
  `template_source`'s capture rung, `resolve_capture_draft`'s seed, `notes_capture_impact`),
  `src-tauri/crates/keeper/src/notes_window.rs`, `keeper/src/ipc.rs`'s recording pair
reason: these compile and their handler registration is now pinned by
  `src/test/command-registration.test.ts`, so the "Command not found" class is closed. What is
  NOT closed is whether they do the right thing: no note has ever been captured through this
  code, no manifest has ever been written through IPC on any machine, and the
  `quick-capture-*` capability glob has never been evaluated by a real webview. A capability
  that did not take renders exactly like a frontend bug — the window appears and its buttons do
  nothing — and no test on either box can tell the two apart.
  Each story's spec carries its own ordered gate checks; this entry exists so the set is
  findable from one place rather than five, and so it is not mistaken for covered because the
  Rust compiles.
status: open

### DW-193: The `.gitattributes` lines already broken by the unquoted writer are left exactly as they are, and every one of them is a rule covering nothing.

origin: story 46.1, 2026-08-10 — carved out by the epic spine before the story was written, and
  confirmed against the owner's file during it
location: `src-tauri/crates/keeper-sync/src/lfs/stage.rs` (`ensure_attributes`), against any
  `.gitattributes` written by keeper before 46.1
reason: 46.1 stops keeper EMITTING an unquoted pattern; it does not repair the ones already on
  disk. The owner's file holds fifty-nine of them. Each is inert in the same two ways: git reads
  `/2021 holiday/clip filter=lfs diff=lfs merge=lfs -text` as the pattern `/2021` with
  `holiday/clip` as an attribute NAME, so the intended path is not routed through LFS at all —
  and `is_false_modification` asks `routed_through_lfs`, so those paths also lose the
  racily-clean dismissal that keeps a multi-gigabyte file from being re-hashed on every scan.
  After 46.1 they stop multiplying, because the correctly quoted line keeper now writes is
  recognised on the next run; the stale ones simply sit there.
  The mechanism to fix them exists and is one function: `ensure_attributes` already reads the
  whole file, rebuilds it in memory and writes it back, so re-emitting the managed block through
  `quote_pattern` and dropping lines it can prove it wrote is a contained change. What makes it a
  separate decision is that `.gitattributes` is a versioned file the user is invited to edit, and
  keeper cannot distinguish "a broken line I wrote" from "a line the user adjusted afterwards"
  by content alone — the managed marker says where keeper's block STARTS, not that every line
  below it is still keeper's. Bundling a data migration into a bug fix also means the fix cannot
  be reverted without reverting the migration, which is the reason the epic spine states for the
  carve-out.
  Whoever picks this up should decide it as a product question first: repair silently, repair
  with a one-line notice, or leave them and let the correctly spelled rule be added alongside.
status: done 2026-08-11
resolution: resolved by story 47.1 — `ensure_attributes` repairs the lines git itself rejects, below the managed header only, and collapses duplicates keeping the LAST copy because git applies matching lines in order.

### DW-194: An exact-path `.gitattributes` pattern leaves glob metacharacters live, so a real filename containing `[` gets a rule that matches everything except that file.

origin: story 46.1, 2026-08-10 — flagged by ScoutGitattr while diagnosing the quoting defect and
  deliberately kept out of the fix
location: `src-tauri/crates/keeper-sync/src/lfs/stage.rs`, `pattern_for`'s exact-path branch
  (`format!("/{}", …)`)
reason: the exact-path branch spells a filename as a gitattributes PATTERN, and a pattern is
  wildmatch — `*`, `?`, `[`, and `\` are operators there. C-style quoting does not help, and this
  is the part worth writing down: unquoting runs FIRST and produces the pattern, then wildmatch
  interprets it. The two layers compose rather than cancel, so 46.1's fix carries the bug through
  intact.
  Measured against git 2.53.0, with the pattern quoted exactly as keeper now writes it:

  ```
  .gitattributes:  "/notes/draft[final]" filter=lfs
  notes/draft[final]  ->  filter: unspecified      <- the actual file, NOT routed
  notes/draftf        ->  filter: lfs              <- a file that is not the one
  ```

  `[final]` is a character class, so the rule covers `draftf`, `drafti`, `draftn`, `drafta` and
  `draftl`, and misses the file it was written for. The `*` case fails the other way and is
  quieter: `"/budget*2026"` does route `budget*2026`, and also routes `budget-q3-2026`, so an
  unrelated file is silently converted to an LFS pointer.
  Consequence is the same class as DW-193 — a large file that was meant to be routed becomes an
  ordinary git blob, which is the outcome `lfs::stage` exists to prevent — but the cause is
  different and so is the fix: escape the four wildmatch operators inside the pattern (`\[`,
  `\*`, `\?`, `\\`) at the point the exact path is built, INSIDE the quotes, since the escape is
  consumed by wildmatch after unquoting rather than by the quoting. That is a change to what a
  pattern MEANS, it needs its own round-trip evidence against `git check-attr`, and it interacts
  with DW-193 (patterns already on disk would need the same treatment), which is why it is not
  folded into a story about blanks.
  Note the extension branch is not affected: `pattern_for_extension` emits `*.<ext>` where the
  `*` is intended, and `engine.rs:1802` already refuses an extension holding `/` or whitespace.
status: done 2026-08-11
resolution: resolved by story 47.1 — `* ? [ \\` escaped in the literal half of both producer branches, applied before quoting because git strips them in that order.

### DW-195: The tray's notes section still hand-types its labels, so a registry retitle that now reaches four surfaces still misses the three notes verbs in the menu bar.

origin: story 46.16, 2026-08-10 — the recording half of exactly this defect was the story; the
  notes half was found while writing the projection and deliberately left
location: `src-tauri/crates/keeper/src/tray.rs` — `build_notes_items`'s initial labels plus
  `new_note_label`, `capture_label`, `journal_label`
reason: 46.16 made the tray project `keeper_core::palette` for the recording verbs
  (`tray_recording_verbs`), so their words and order are the registry's and a rename reaches the
  menu bar. The notes section was left hand-built, and the words are duplicated: `palette.rs`
  ships `notes-new` → "New Note", `notes-capture` → "Quick Capture",
  `notes-journal-today` → "Today's Journal", and `tray.rs` spells all three again. UX-DR42 says
  the palette, the cheat sheet, the native menu and the tray carry the same titles; nothing
  enforces it for notes, so the next retitle drifts here exactly as "Start Recording" did.
  It was not folded in because the notes labels are COMPOSED, not projected: `new_note_label`
  returns the honest "no vault yet" wording instead of the verb, `capture_label` carries the
  global-shortcut registration failure (UX-DR43), and the slots/unread line carry model data. The
  shape of the fix is therefore "registry title as the BASE word, composition unchanged": add a
  notes id list beside `tray_verb_ids`, gate it on the notes capability, take the base from the
  projection and keep `paint_notes` composing the suffix.
  One thing that widening must NOT inherit from the recording projection: order. Recording verbs
  take the registry's order because they are rebuilt per menu, but the notes items are built once
  and only mutated (AD-61 — a Linux tray menu cannot be swapped after it is set), so their slot
  order has to stay the tray's own or a registry reshuffle would move labels onto the wrong
  handles. `notes-open` / `notes-search` are registered and deliberately absent from the tray,
  which is the other reason the id list stays explicit rather than "the whole Notes section".
status: done 2026-08-11
resolution: resolved by story 47.4 — the tray's notes labels project `keeper_core::palette`, pinned by two source-scan assertions rather than by value, because duplication is a fact about source text.

### DW-196: `DestinationProfileRow` now carries the recordings root and the head it was joined from as two independent `Option`s, so a row that says one without the other is representable.

origin: story 46.10, 2026-08-10 — created by the story, named by it, and deliberately not
  collapsed inside it
location: `src-tauri/crates/keeper/src/ipc.rs` — `DestinationProfileRow.recordings_root` /
  `.recordings_subfolder`, filled together in `destination_profile_row` and consumed together in
  `destination_profile_vms`
reason: 46.10 needed the profile-relative subfolder on the destination picker's rows, because the
  card now edits it and the exact stored string is what must be echoed back to
  `sync_profile_save` — a head recovered from the resolved root by `strip_prefix` would come back
  component-normalised, and `20-media//sessions` and `20-media/sessions` are one root but two
  different stored values. The honest shape is one field, `Option<(PathBuf, String)>` or a small
  `RecordingsPlace { root, subfolder }`, so "a root with no head" cannot be written down. What
  shipped is two `Option`s that agree by construction: `destination_profile_row` is the only
  place either is built and builds both from the same `profile.recordings` block, and
  `destination_profile_vms` `?`s on both, so a disagreement drops the row rather than inventing
  half of one.
  Nothing is wrong today. Six test fixtures set `unflagged.recordings_root = None` and leave the
  now-stale `recordings_subfolder` behind, and it changes no verdict because `recordings_root` is
  checked first at every one of the six read sites. The cost is the next reader's, and the next
  field: a seventh site that consults the head first would read a head for a folder that holds no
  recordings.
  It was not collapsed in 46.10 for one reason only, and it is not a design reason: the `keeper`
  shell crate does not build on Linux, the story was implemented there, and the collapse is
  thirteen edit sites in a file no local gate can compile. Two were provable by inspection;
  thirteen were not. The fix belongs in the next story that is already running the macOS gate on
  `ipc.rs`, and it is mechanical: introduce the struct, add a `recordings_root()` accessor so the
  five existing readers keep their one-line shape, and rewrite the six fixture mutations to
  `row.recordings = None`.
status: done 2026-08-11
resolution: resolved by story 47.5 — the pair became one `Option<RecordingsPlace>`, so it cannot be half-set.

### DW-197: `WriteScope::file` survived AD-102 with no production caller left, and it is the pre-fork mental model sitting in the module as a trap.

origin: story 46.14, 2026-08-10 — created by the story, named by it, and deliberately not
  removed inside it
location: `src-tauri/crates/keeper-sync/src/files_write.rs` — `WriteScope::file`, plus the six
  tests that still exercise it (`a_profile_with_no_vault_refuses_every_path_and_names_itself`,
  `a_folder_whose_name_extends_the_vaults_is_not_inside_the_vault`,
  `a_nested_vault_subfolder_is_matched_one_component_at_a_time`,
  `the_configured_subfolder_is_normalised_however_it_was_typed`,
  `traversal_is_refused_wherever_it_is_aimed`, `the_vault_directory_itself_cannot_be_deleted`,
  `a_directory_is_refused_as_a_folder_whatever_it_is_named`)
reason: AD-102 replaced `file` with `WriteScope::route` / `owner` / `classify`. Its two former
  callers are gone — `files_listing_vm` now asks `owner`, and `deletable` was deleted outright —
  so as of this story `file` is reached only from its own tests. That is worse than dead code:
  `file` answers `Err(OutsideVault)` for exactly the paths AD-102 now routes to the second
  writer, so it IS the pre-fork mental model, still public, still plausible-looking, and a
  future reader reaching for it would bypass the fork this story exists to make unbypassable.
  Not removed here because the deletion is not the mechanical part — six existing tests assert
  refusals through it that AD-102 has turned into routing decisions, so each one needs its
  intent re-expressed against `directory` (the create path, still live) or `owner` rather than
  mechanically repointed, and doing that at the end of a wave in a file three other agents were
  reading is how a green suite stops meaning anything.
fix: delete `WriteScope::file`; migrate each test above to `directory` where it is asserting the
  component-by-component vault match or the create refusal, and to `owner` where it is asserting
  the escape / vault-root / is-a-directory refusals. `vault_relative` stays — it is the shared
  core of `directory` and `classify`. No production line changes.
status: done 2026-08-11
resolution: resolved by story 47.5 — `WriteScope::file` deleted, its seven tests migrated to `directory` or `owner`, proven by compilation.

### DW-198: the hotkey path re-centres the draft capture window on every press, and story 46.15 made that asymmetry visible.

origin: story 46.15, 2026-08-10 — found while wiring the size through, named here rather than
  fixed because the fix costs NFR-27 something and the trade needs to be chosen, not slipped in
location: `src-tauri/crates/keeper/src/notes_window.rs` — `show` (the no-argument entry point)
  passes `None` to `reveal`, which falls through to `position(window)`; its two callers are
  `hotkey::install_capture`'s press handler and the tray's Quick Capture item
reason: `show` deliberately has no settings read in front of it — the hotkey path is
  `set_position` → `show` → `set_focus` and NFR-27 gives it 300 ms. The consequence, which
  predates this story, is that the prewarmed window's REMEMBERED POSITION is honoured only when
  it is raised through `notes_capture_open`, and is re-centred on the pointer's monitor every
  time the hotkey or the tray raises it. A person who unlocks the draft window and drags it
  somewhere finds it back in the middle on the next hotkey press.
  Story 46.15 did not cause this and did not widen it — `reveal(None)` now touches the position
  and nothing else, where before it would also have re-asserted a default size and lock. But it
  DID make the asymmetry legible: the size survives a hotkey press and a restart (adopted once
  at boot, off the hot path), and the position does not. "It remembers how big I made it but not
  where I put it" is a report waiting to be filed.
fix: adopt the position the same way the size is adopted — once, at boot, in `lib.rs`'s setup,
  alongside the existing `notes_window::adopt_placement` call, so the hot path stays three
  synchronous calls. That is not free: `position` is `apply_placement`'s fallback precisely
  because a hidden window's monitor is "where it was last placed", so honouring a stored
  position means the panel stops following the pointer between monitors. Which of those two a
  person wants is exactly what the lock already says — locked follows the pointer, unlocked
  stays put — so the shape is: at boot, if the stored placement is unlocked and carries a
  position, apply it; on every `show`, do what `reveal(None)` does today. Needs the macOS gate.
status: done 2026-08-11
resolution: resolved by story 47.5 — the hotkey no longer re-centres a window the user placed. Departs from this entry's literal fix text and implements its reasoning; the reason is in the story's spec §1.

### DW-199: on GTK, an unlocked capture window's own 5 px resize border sits over the chrome buttons.

origin: story 46.15, 2026-08-10 — a platform fact confirmed from tao's source during the story,
  recorded rather than designed around because it needs a look at the real window to size
location: `src/components/capture/capture-window.tsx` — `CaptureWindowChrome`'s strip
  (`h-8 … px-1`, buttons flush to the right edge); the behaviour itself is tao 0.35's
  `src/platform_impl/linux/window.rs` motion/button handlers
reason: tao gives an undecorated resizable window its edge hit-testing INSIDE the surface — a
  5 px × scale strip along each edge — and the webview never sees clicks that land in it. On
  macOS and Windows the resize border is outside the client area, so this is Linux-only. The
  capture chrome's close button is flush against the top-right corner, which is where two of
  those strips overlap, so on GTK an UNLOCKED window plausibly turns the top and right few
  pixels of that button into a resize handle. Two related tao facts from the same reading: the
  cursor stays the default arrow during an edge drag (tao's own FIXME), and edge dragging is off
  while the window is maximized.
  Not addressed here because the fix is a number — how much inset the chrome needs — and this
  story could not run a GTK window to measure it. Guessing a padding that is wrong in either
  direction is worse than recording the hazard: too little fixes nothing, too much moves a
  control on every platform to solve a problem on one.
fix: on a Linux desktop, unlock a capture window and click the top-right two pixels of the close
  button. If it resizes instead of closing, give the strip a right/top inset of one resize border
  when unlocked (the same conditional the drag region already uses), sized from what is measured
  rather than from tao's constant. No Rust change; no capability change.
status: done 2026-08-11
resolution: resolved by story 47.5 — the inset is computed in Rust from tao's real rule (`scale_factor() * 5`, zero when locked, maximized or non-GTK) and carried to the webview as a number, so the frontend still reads nothing about the platform.

### DW-200: A legacy `.gitattributes` line whose only defect is a live glob metacharacter is not repaired, because a line git accepts is evidence of nothing.

origin: story 47.1, 2026-08-11 — named by the repair's own boundary rule rather than found afterwards
location: `src-tauri/crates/keeper-sync/src/lfs/stage.rs`, `ensure_attributes`'s repair candidacy
reason: 47.1 repairs a line only when git itself REJECTS it, because that is the one case where the
  original intent is recoverable by construction. A line the old writer emitted for a file named
  `report [final].pdf` parses fine — as a character class — so it silently over-matches, and there
  is no way to tell it apart from a hand-added recursive glob a person meant literally. The measured
  cost is that the stale class keeps resolving `reportf` through LFS beside the new correct line,
  with empty stderr and nothing to notice.
  Closing it needs a version marker in `MANAGED_HEADER` so the repair can know which of its own
  spellings wrote a line — which is a file-format change and wants its own decision, not a
  widening of a boundary that is currently provable.
  Same entry covers the backslash-in-extension variant: `*.a\b` is read as `*.ab`.
status: open

### DW-201: `pattern_for` normalises a backslash to a slash before escaping, so a literal backslash in a filename is destroyed at the separator step.

origin: story 47.1, 2026-08-11
location: `src-tauri/crates/keeper-sync/src/lfs/stage.rs`, `pattern_for`
reason: a backslash is a legal character in a Linux filename, and the separator normalisation runs
  before the glob escaping, so an oversized `a\b` is routed by the pattern `/a/b` — a path that
  names a different file, or no file. Nobody has reported it because the filename is rare, but it
  is the same shape as story 47.2's finding: a rendering standing in for the bytes, and the
  rendering reaching a different file than the one it came from.
  Adjacent to 47.2's non-UTF-8 path work and probably belongs with it — that story established the
  rule (refuse rather than resolve when the rendering is not reversible) and this is a second place
  it applies.
status: open

### DW-202: The summon hotkey shows the inbox without unfolding the column it lives in.

origin: story 48.1, 2026-08-11
location: the summon-hotkey focus target vs `useSurfaceColumn`'s folded state
reason: 48.1 gave four surface columns a fold. A hotkey that focuses something inside a folded
  column focuses a thing nobody can see — the keystroke appears to do nothing. Not a regression
  (the fold is new), but it is the first of a class: every existing "focus this" path now has a
  column that may be shut in front of it. The fix is one unfold call at the summon path; the
  reason it is deferred is that the same question applies to every other focus path and answering
  it once, deliberately, is better than four unfolds added as each is noticed.
status: open

### DW-203: Two resize hosts reach the same column through one className hinge.

origin: story 48.1, 2026-08-11
location: `src/components/ui/resizable-columns.tsx`, `src/lib/column-widths.ts`
reason: the seam and the column root both write width through a shared className path, so the
  two hosts are coupled by a string rather than by a type. It works and is tested; it is the
  shape that breaks quietly when a third host appears.
status: open

### DW-204: `columnTemplate`'s fitted branch hard-codes `MIN_COLUMN_WIDTH`.

origin: story 48.1, 2026-08-11
location: `src/lib/column-widths.ts`
reason: 48.1 gave each surface column its own floor, justified per column, because the 72px
  constant was chosen for a property KEY whose escape hatch is an overflow trigger. The fitted
  branch of `columnTemplate` still bakes the 72 in, so a fitted column ignores the floor its
  own id declares. No live consumer hits it today; the next one will.
status: open

### DW-205: A note's editor refcount cannot span a capture window.

origin: story 48.3, 2026-08-11
location: `src/lib/stores/notes-editor.ts`, against `keeper::notes_window`
reason: 46.12 made two panels on one note share a document by refcount. A capture window is a
  separate webview, so a separate JS realm and a separate module registry — the refcount cannot
  reach it, and a note open in a panel AND in a capture window has two independent buffers. Today
  each saves through the same Rust writer and last-write-wins, which is the pre-46.12 behaviour
  and not a new defect; it becomes one the moment either side grows an unsaved-state promise.
status: open

### DW-206: A foreign `filter.lfs.process` still reaches the clone's own checkout, because `gix::clone::PrepareCheckout` hands out no mutable repository.

origin: found while fixing the field incident of 2026-08-13 (a global `git lfs install` driver
  failing every `status`), 2026-08-16
location: `src-tauri/crates/keeper-sync/src/git/repo.rs` — `clone` (`:560-573`), against
  `drop_foreign_lfs_driver` (`:80`) and `gix::clone::PrepareCheckout::main_worktree`
reason: `drop_foreign_lfs_driver` removes a non-repository `filter "lfs"` section from the
  merged in-memory config of a repository handle, and `open` calls it before anybody reads a
  file — so every status, commit and pull pass is protected. The initial clone is not. gix
  materializes the worktree from inside `PrepareCheckout`, whose only accessor is
  `repo(&self) -> &Repository` (gix 0.86, `clone/checkout.rs:156`), and the surgery needs
  `&mut` for `config_snapshot_mut`. `fetch_then_checkout` builds that value with private
  fields, so a caller cannot construct one from a `fetch_only` repository either. The
  consequence is narrow but real: on a machine whose global config names `git-lfs` and whose
  `PATH` (Finder's, on a desktop launch) does not have it, the FIRST clone of a repository
  carrying LFS-tracked paths fails with `checkout failed: … Process handshake …` instead of
  leaving pointer text for keeper's own materialization pass. Every later pass on the same
  folder is fine, and the message names the cause, so this is a bad first minute rather than a
  stuck folder.
  Three routes, none free. (a) Do the checkout ourselves after `fetch_only`: needs
  `gix_worktree_state::checkout` plus the options gix assembles in `Cache::checkout_options`,
  which is `pub(crate)` — re-deriving validation, permissions and symlink handling is exactly
  the code most expensive to get wrong. (b) Pass `open::Options::filter_config_section` (a
  `fn(&Metadata) -> bool`) through `clone::PrepareFetch::new` so no global section is trusted:
  one line, but the predicate cannot see the section NAME, so it also drops global `http.*`
  (proxy, sslVerify), url rewrites and credential helpers for the one operation that talks to
  the network — a silent regression for anyone behind a proxy. (c) Ask upstream for
  `PrepareCheckout::repo_mut()`, or for the two gix behaviours that make this bite at all:
  drivers merged by name across scopes the way git merges them, and an empty `process` treated
  as absent (git's `apply_filter` tests `*ca->drv->process`, gix's `maybe_launch_process` tests
  only `Some`). (c) is the honest fix and is not ours to schedule.
status: open

- source_spec: none
  summary: Restyle the note editor's Find/Replace bar so it matches the app's UI instead of rendering as unstyled inputs and buttons.
  evidence: Split from a four-goal intent (note pane truncation, find bar, toolbar additions, file embedding). Independently shippable: pure presentation inside the editor chrome, no shared contract.

- source_spec: none
  summary: Add a mark (`==highlight==`) command and an emoji picker button to the markdown format toolbar.
  evidence: Split from the same four-goal intent. Independently shippable: two additions to `format-toolbar.tsx` plus format commands; `emoji-complete.ts` already provides `:shortcode:` autocomplete, so the picker is a second entry point rather than new emoji infrastructure.

- source_spec: none
  summary: Audit inline embedding of image/video/audio/html/json/csv/pdf across all notes including session notes, and add editing of the rendered text for html, csv and json.
  evidence: Split from the same four-goal intent, and the largest of the four. Much of it already exists — `FILE_FORMATS` in `src/lib/viewers/registry.ts` covers all seven types, `FileEmbedWidget`, `CsvTableWidget`, `ImageWidget` and `RecordingEmbedWidget` render them — so the real work is an audit of the gaps plus editable rendered text, not new embedding infrastructure.

- source_spec: spec-note-panel-never-clipped
  summary: The Files and Chat surfaces still carry `shrink-0` on their columns, so a wide tree or inbox can still starve the document beside it.
  evidence: The panel strip fix is shared — those two surfaces render the same `PanelStrip` and now claim the same 280px basis — but their columns still refuse to shrink, so the deficit lands on the panel again. Story 55.1 was scoped to Notes and told not to touch them ("Never: do not touch the Files or Chat surfaces' columns"). The fix is one line each: drop `shrink-0` from `FILES_COLUMN_CLASS` and `CHAT_LIST_COLUMN_CLASS`. Deferred rather than done because neither surface's floor has been thought about the way `notes-list`'s 240 was, and changing three surfaces on one surface's evidence is how a fix becomes a regression somewhere nobody was looking.
  status: open

- source_spec: spec-note-panel-never-clipped
  summary: While a column is squeezed, the drag handle's `aria-valuenow` and the resizer report the remembered width, not the rendered one.
  evidence: Deliberate — the remembered width is what a widened window restores, and it is the number the user set, so persisting or announcing the squeezed value would be the write this spec forbids. But a screen reader is told 320 while the column is 263, and dragging from a squeezed state jumps to the remembered width before it moves. Neither is wrong enough to trade for a stored-width bug; both deserve a considered answer about what a resizer should say when the layout is overriding it.
  status: open

- source_spec: spec-note-panel-never-clipped
  summary: No test lane exercises real flexbox — layout regressions of this kind can only be caught by arithmetic or by hand.
  evidence: jsdom computes no layout, so every assertion in this story checks the *inputs* to the distribution (`flexBasis`, `minWidth`) and none checks the result. The bug being fixed was invisible to all 4600 tests precisely because it lived in the distribution. Verified for this story by rebuilding the box tree against the compiled stylesheet in a real engine and reading widths back, which is repeatable but manual. A headless-browser lane for a handful of layout invariants would make it automatic.
  status: open

- source_spec: spec-find-bar-matches-the-ui
  summary: The find bar shows no match count, so "how many" and "which one" are still unanswerable.
  evidence: The stock panel had none either, so this is not a regression — but it is the affordance people expect from a find bar, and rebuilding the panel was the cheap moment to add it. Held back deliberately: counting every match on each keystroke is a performance question about large notes (`SearchQuery.getCursor` walks the document), and a restyle is the wrong story to answer it in. Wants a cap and a "500+" form.
  status: open

- source_spec: spec-find-bar-matches-the-ui
  summary: The file viewers mount CodeMirror without `search()`, so there is no find at all in a `.md`, `.csv` or `.json` opened from Files.
  evidence: `⌘F` is wired in `note-editor.tsx` only. `findBar()` is now one extension and would drop into `text-viewer.tsx` unchanged, but adding find where there was none is a feature, not a restyle, and the viewers have their own `⌘F` question (the Files pane filter) to settle first.
  status: open

- source_spec: spec-toolbar-mark-and-emoji
  summary: The emoji picker has no recents and no skin-tone variants.
  evidence: Both are state, and this was a picker. Recents in particular would change what "opens on" means and needs a decision about where that state lives (per vault? per device? synced?) before it is worth building. The opening set in `format-toolbar.tsx` is the hand-maintained stand-in.
  status: open

- source_spec: spec-toolbar-mark-and-emoji
  summary: `==highlight==` renders in keeper and is invisible to anything else reading the vault.
  evidence: It is not in CommonMark or GFM; the extension in `markdown-marks.ts` is keeper's own, matching the spelling Obsidian and most wikis use. A note exported or read by a plain CommonMark renderer shows the equals signs. Worth stating in the vault docs rather than fixing — there is nothing to fix short of not supporting it.
  status: open

- source_spec: spec-note-embeds-media
  summary: `.docx`, `.pptx` and `.xlsx` embedded in a note are still links.
  evidence: They are `documentRow`s with no inline renderer — `document-viewer.tsx` shows a PDF through the webview's own renderer and offers the other three as a download. What a spreadsheet should look like inside a paragraph is a design question, not a wiring one, so `drawableFor` returns null for them and the link stays.
  status: open

- source_spec: spec-note-embeds-media
  summary: `![[note.md]]` is still a link rather than a transclusion.
  evidence: Deliberate and stated in `file-embed.ts`: showing one note inside another is a different feature with a different meaning, and mounting a raw editor over a note would be a second way to write one without `notes_save`'s base revision or its conflict copy. Rust refuses that write too, so both halves agree. Listed here because "every file type renders inline except this one" is the kind of gap a reader will otherwise report as a bug.

  status: open

- source_spec: spec-note-embeds-media
  summary: A PDF embed cannot report a failed load.
  evidence: `<embed>` fires no `error` event, so the degrade-to-a-link path every other kind has does not exist for PDF. Safe today because the path came back from a resolver that stats the file, so "the vault holds it" is established before the element is built — but a file deleted between the resolve and the paint shows an empty box rather than the link. Fixing it means probing the bytes or watching for a zero-size box, neither of which is obviously worth it.
  status: open

- source_spec: spec-html-rendered-and-editable
  summary: The JSON structure view is still read-only, so "editable rendered text" is true of HTML and CSV and not of JSON.
  evidence: `json-structure.ts` says so in its header and gives its reason. Making a value editable there is the same shape this story built for HTML — a parse that records source offsets, a splice verified before it is applied — but the JSON parser is deliberately not `JSON.parse` (it preserves number text, key order and repeats), so the offsets have to come out of that parser rather than a second scan. Its own story.
  status: open

- source_spec: spec-html-rendered-and-editable
  summary: Pressing Enter inside an editable HTML text run drops the line break.
  evidence: `contenteditable="plaintext-only"` inserts a break element, and the splice reads `textContent`, which does not include it — so the text is written without the newline. Lossy in one direction only and never corrupting (the verified splice still applies), but a reader who expects a paragraph break gets a space. Fixing it means deciding what a line break inside a run should become in the markup, which is a markup decision and therefore the Source tab's.
  status: open

- source_spec: spec-html-rendered-and-editable
  summary: A `<style>` block is dropped, so a page renders in keeper's typography rather than its own.
  evidence: Deliberate and stated in `html-view.ts`: applying a file's CSS would be styling this application's own DOM from a document somebody was handed. A scoped or sanitised subset is possible and is a separate question with its own answer.
  status: open

- source_spec: none
  summary: `files-pane.test.tsx` "re-reads a remembered folder once when Refresh rescues a failed first list" times out under parallel load.
  evidence: Observed twice while hunting an unrelated unhandled rejection — once during a `lefthook` pre-push run (tests sharing the machine with clippy and typecheck) and once in a bare full run. `TestingLibraryElementError: Unable to find role="treeitem" and name "Notes"` after the 5s `waitFor` budget; the pane is rendered and the tree is empty, so the second list has not landed. Passes 15/15 in isolation. Not investigated: it is a different subsystem from the work in flight, and guessing at a timing fix without reproducing it deliberately would be a change nobody can evaluate.
  status: open

- source_spec: spec-no-sync-unit-stalls-in-silence
  summary: The filter-protocol deadlock itself is guarded against but not root-caused.
  evidence: Observed precisely — content fully delivered (every open file at EOF), `keeper lfs filter-process` blocked in `pktline::read` on stdin, parent blocked in `Client::invoke`. `Request::fill` handles the flush packet correctly, so the missing packet is on the client side or in how a conversion is handed between gix's pool and ours. This change makes the deadlock survivable (bounded concurrency, abandonable walk, self-terminating filter) without explaining it. Reproducing it on a synthetic repo of large filtered files is its own story.
  status: open

- source_spec: spec-no-sync-unit-stalls-in-silence
  summary: `.git/lfs/tmp` accumulates leaked scratch — 100 GB in 863 files over four days on one machine.
  evidence: The files are `tempfile`-style `.tmpXXXXXX` names, which delete on drop; a process killed mid-transfer leaks one. Nothing in keeper sweeps them, so every crash costs disk permanently. A sweep at startup on the same terms as `recover_running` — remove scratch older than any live run — would return the space. Measured alongside `.git/lfs/incomplete` at 4 GB, itself untouched since 17 August.
  status: open

- source_spec: spec-no-sync-unit-stalls-in-silence
  summary: A profile can sit in `offline` indefinitely with no path back.
  evidence: `Offline` is set on a transient network error with the comment "the queue drains when connectivity returns (AD-49)", but the state is only refreshed by a sync that completes. When the only queued unit cannot complete, the profile reports `offline` forever while the server is reachable — observed for three days with `curl` answering the remote in 30 ms. The state needs a way to be re-evaluated that does not depend on the thing it is blocking.
  status: open

- source_spec: spec-repair-the-lfs-stat-invariant
  summary: Nothing detects a broken pointer-stat invariant on its own; the repair runs only when a person presses "Recheck all files".
  evidence: `repair_index_stat` restores it, and `rescan` calls it, but a folder whose invariant broke while nobody was looking still converts its whole worktree on every status pass until somebody notices and presses the button. The card now says the folder needs attention after three consecutive failures, which is what makes the button findable — but a cheap periodic check (does a materialized entry's stat still describe its file?) would make the repair automatic. Left out here because a self-repairing index is a write nobody asked for, and this story's job was to make the failure visible and fixable.
  status: open

- source_spec: spec-56-1-a-policy-that-says-which-files-may-stay-away
  summary: `tests/hooks_never_run.rs` fails on any host whose git configuration sets `core.hooksPath`, which makes every repo-local `.git/hooks/*` inert.
  evidence: Pre-existing and unrelated to this story — it exercises `GitCli::push` and git hook behaviour, and touches nothing story 56.1 changed. Its own sanity assertion (`hooks_never_run.rs:87`) requires a planted `.git/hooks/pre-push` to block a plain `git push`, which cannot happen when `core.hooksPath` points elsewhere. Verified: `git config --show-origin --get-all core.hooksPath` reports `file:/home/dev/.gitconfig  /home/dev/.config/git/hooks`, and the test passes under `GIT_CONFIG_GLOBAL=/dev/null cargo test -p keeper-sync --test hooks_never_run`. The fix is for the test to plant its hook and force `-c core.hooksPath=.git/hooks` on the sanity push, so the fixture proves what it claims regardless of the developer's own git config.
  status: open

- source_spec: spec-56-1-a-policy-that-says-which-files-may-stay-away
  summary: No production path constructs a `VirtualPolicy`, so FR-329's "a malformed pattern refuses at startup" is not yet reachable by any user.
  evidence: This is 56.1's declared non-goal — 56.2 is the consumer in the epic's strict 56.1 → 56.2 chain — but FR-329 is listed as a requirement 56.1 must satisfy, and it is satisfied only in the sense that `compile` refuses when called. The sibling refusals are reachable (`ExcludeSet::new` at `engine.rs:6111` and `stability.rs:252`, `LfsPolicy::from_profile` at `lfs/stage.rs:1193`), and `SyncProfile::validate` deliberately validates no glob field, so it is not the gate either. 56.2's review must confirm its consumer compiles the policy before any materialization decision, or the refusal machinery ships unreachable.
  status: open

- source_spec: spec-56-1-a-policy-that-says-which-files-may-stay-away
  summary: A host cannot decline the repository's virtualization policy: an empty `virtualPatterns` is silence, so a machine can replace the committed list but never withdraw from it.
  evidence: `VirtualPolicy::compile` treats a profile list that says nothing as "this tier is silent", which is the right reading of an unset key and the wrong one for a key deliberately set to `[]`. The operator case that arises is a laptop that is often offline and wants everything materialized; today the only spellings are a pattern that matches nothing or a negation-only list, both accidental. Distinguishing key-absent from key-empty needs the `Option<Vec<String>>` shape on the profile field, which is entangled with the `SyncProfileReq` `Option` rule (DW-116) that 56.1 declared a non-goal. Settle it in the story that renders the control (56.7/56.9), together with the tier surface AD-132 asks for.
  status: open

- source_spec: spec-56-1-a-policy-that-says-which-files-may-stay-away
  summary: `.keepervirtual` and the virtualization keys have no user-facing documentation; only `keeper-syncd`'s annotated `[[profile]]` template describes them.
  evidence: The daemon template gained both keys in this story, because the daemon accepts them the moment `SyncProfile` carries them and an operator setting a byte-affecting policy blind is the same failure one step earlier than the typo the module hard-refuses. The committed file is the artifact a whole team meets, and it is documented nowhere. Epic 56 assigns the chapter to story 56.8 (`docs/sync.md` grows a virtual-files section, last of all), which is where it belongs — recorded here so the file name, the `!` protection rule and the union-across-tiers behaviour are not left to be rediscovered from the source.
  status: open

- source_spec: spec-56-1-a-policy-that-says-which-files-may-stay-away
  summary: The shared gitignore dialect has no spelling for "root-anchored, single segment" outside the virtualization lists.
  evidence: `exclude::anchor` applies the basename rule to any pattern without a `/`, so `excludes = ["notes.md"]` and `lfsNever = ["notes.md"]` mean "at any depth" with no way to say "the root's own". Story 56.1 resolved this for its own lists by handling gitignore's leading `/` in `virtual_policy::anchor_line`, and deliberately did not change the two older fields, whose current meaning is depended upon. Teaching `add_pattern` the leading slash would make all three consistent; it is a behaviour change to two shipped config keys and needs its own story.
  status: open

- source_spec: spec-56-2-a-listing-that-knows-what-it-does-not-hold
  summary: A present `remote` key in a listing (or in `verify --json`) does not always mean the server was asked — `audit_remote_objects` returns a fully-formed intact audit with no round trip for `lfsMode = disabled` and for a filesystem remote with no `.lfsconfig`.
  evidence: `Engine::audit_remote_objects` short-circuits to `RemoteAudit::default()` on `LfsMode::Disabled` and returns `missing: []` without contacting anything when the remote is a local store, both by design (`engine.rs`, the two early returns before the batch client is built). The absent-vs-present contract story 56.2 built for `ls-files --remote` therefore holds for the *key* but not for the *answer*: a monitoring wrapper reading `listings[].remote.missing == []` as "every object is on the server" gets a false green for exactly the profiles whose objects were never uploaded. Pre-existing and shared with `verify --remote`, which has carried the same shape since DW-140; fixing it means the audit saying which of the three ways it answered, which is a change to a shipped JSON document and needs its own story.
  status: open

- source_spec: spec-56-2-a-listing-that-knows-what-it-does-not-hold
  summary: `ls-files` is keyed by the index, so a path that was materialized and then removed from the index produces no row at all — not even `absent` — although the ledger still remembers it and the store may still hold the object.
  evidence: `lfs::listing::collect` iterates `stage::indexed_pointers`' answer and joins the `materialized` ledger into it, which is the right key for "which of this checkout's LFS paths do I hold" and the wrong one for "what is this clone still holding". A committed deletion or a sparse-cone change drops the row while `prune`'s candidate set and 56.5's release sweep both read the ledger, so the listing an operator would reach for to audit that sweep cannot show the same set of rows. Deliberate for this story — the module doc frames the verb as index-keyed — and worth settling in 56.5, where the sweep's own reporting makes the second key necessary.
  status: open

- source_spec: spec-56-3-a-file-you-can-ask-for
  summary: `Engine::reserve` is an in-process map, so `keeper-syncd materialize` does not exclude a running `keeper-syncd watch` — and every one-shot CLI invocation runs `db::recover_running`, which flips the live daemon's in-flight rows back to `pending`.
  evidence: `reserve` (engine.rs) is a `Mutex<HashMap>` on the `Engine` instance and `Engine::open` calls `recover_running` unconditionally, so two processes over one `sync.db` and one index have no mutual exclusion: `with_db` is a mutex rather than a transaction, so `enqueue_unique`'s SELECT-then-INSERT can race the daemon's identical call, and `git::repo::refresh_index_stat` can write an index the daemon read a moment earlier. Pre-existing for every one-shot verb (`sync --once`, `verify`, `rescan`, `ls-files`), and the reason it is recorded here rather than fixed here is that the fix is a cross-process lock plus a `BEGIN IMMEDIATE` around the enqueue triple — a change to the whole CLI surface, not to one verb. Story 56.3 is nonetheless the first verb *designed* to be run beside a running daemon, and it states the honest limit on `Engine::materialize_entry` instead of claiming the exclusion it does not have. Asking for five files in a row resets the daemon's in-flight transfers five times.
  status: open

- source_spec: spec-56-3-a-file-you-can-ask-for
  summary: `lfs::stage::worktree_pointer` folds a read failure into `None`, so an unreadable pointer file is reported to the user as a local modification they must undo.
  evidence: `worktree_pointer` ends `File::open(...).ok()?` (added by 56.2), so a permission error, an ACL or an immutable flag is indistinguishable from "these bytes are not pointer text". `lfs::hydrate::plan` then decides on length alone and answers `ContentRefusal::LocallyModified` — "does not hold the pointer this folder committed" — for a file that does hold it, and `AlreadyHeld` for an unreadable file whose length happens to equal the pointer's size. `plan`'s doc covers a failing `stat`, not a failing read. The honest fix changes `worktree_pointer` to a `Result` and gives the refusal an `Unreadable { path, reason }` variant, which touches the `browse` listing that shares the helper — the reason it is not done inside a story whose surface is the verb.
  status: open

- source_spec: spec-56-3-a-file-you-can-ask-for
  summary: "keeper does not overwrite a local modification" is enforced when the decision is made and not re-checked immediately before `rename(2)`, so an editor save landing in that millisecond window is destroyed.
  evidence: `lfs::hydrate::plan` reads the worktree, and `lfs::stage::materialize` then re-checks only `store.contains` before its `.keeper.<name>.tmp` + `rename`. Nothing re-verifies that the target still holds the committed pointer. `Engine::materialize_landed` has had the identical shape since story 34.x, so the window is not new — what is new is that a *user* triggers it, and the user is the person most likely to be saving that file. Closing it means re-stating the target inside `stage::materialize` (a shared function both the request and the arrival path use) and deciding what a mismatch there costs a background arrival, which is a change to the publish primitive rather than to this verb.
  status: open

- source_spec: spec-56-3-a-file-you-can-ask-for
  summary: An `alreadyMaterialized` answer writes no `materialized` ledger row, so a path a human explicitly asked for can be invisible to 56.5's release clocks.
  evidence: `Engine::materialize_entry`'s already-held arm returns before `db::remember_materialized`, deliberately — `at_ms` is the *materialization* clock and nothing was materialized — so a path materialized by `git lfs pull`, by a pre-ledger keeper, or by the documented same-length tie has no row. It is safe in the conservative direction (no row means the sweep has no candidate to release) and it is the wrong shape for FR-334's last-use fact, which 56.5 owns: the sweep's key is the question of whether the ledger or the worktree decides what "this clone holds" means. Settle it with the second key already recorded for `ls-files` (DW, story 56.2).
  status: open

- source_spec: spec-56-3-a-file-you-can-ask-for
  summary: A subpath containing a backslash is looked up under a normalized name but checked and probed under the literal one, so the verb can answer about a different file than the caller named.
  evidence: `lfs::stage::index_key` replaces `\` with `/` (correct on Windows, where it is a separator; on Linux and macOS it is an ordinary filename character), so `indexed_pointer(repo, "40-media\\clip.mp4")` returns the pointer committed for `40-media/clip.mp4`, while `SparseCone::includes` and `hydrate::plan` use the literal name and answer about a path that does not exist. The result is a refusal sentence naming the wrong file, or an oid and size belonging to a path the user did not ask for — never a write, because `plan`'s `stat` fails. Pre-existing in `index_key` and shared with 56.2's listing; the fix is either to refuse a segment containing the platform's other separator or to carry `index_key`'s normalized path through every consumer, which is a change to the crate's path frame.
  status: open

- source_spec: spec-56-3-a-file-you-can-ask-for
  summary: An inline publish does not retire a pending `LfsDownload` row for the same object, so the daemon can later fetch bytes that are already on disk.
  evidence: `Engine::materialize_entry` publishes inline when `store.contains` is true and leaves any covering journal row alone; `materialize_pending` has always done the same. On a metered or slow link that is exactly the transfer the verb exists to avoid, in the case where the object arrived by another route (a pendrive copy, another profile sharing the store, a manual `git lfs pull`). Not fixed here because "the object is present, so this unit is satisfied" is a claim about the *store*, and the place that owns it is the transfer's own precondition rather than one verb — `do_lfs` could check the store before the batch request and complete the unit, which would benefit every caller and needs its own regression fixture.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: A release deletes the `materialized` row outright, so `last_used_ms` and `synced_at_ms` are discarded at the exact moment the columns were designed to still answer.
  evidence: `db.rs`'s own doc says `oid`/`size_bytes` exist "so a row still answers after the worktree stops holding a pointer to consult", and the five late columns are described as "facts the ledger has to hold before anything can decide what to release" — but `forget_materialized` is a `DELETE`. The pin is protected (the refusal is upstream and the statement now carries `AND COALESCE(pinned, 0) = 0`), so nothing the owner asked for is lost today; what goes is the recency history 56.5's TTL sweep would use to choose what to release next, and the "content last landed here" fact a row can still answer. The right shape is a released marker (or a null `at_ms`) that retracts presence without erasing history, plus teaching `db::materialized_paths` to skip released rows — which is a schema decision that belongs with 56.5's own clock columns rather than ahead of them.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: Releasing a hard-linked file reports bytes it did not reclaim, because `rename(2)` replaces one directory entry and the inode survives under the other name.
  evidence: `Release.size_bytes` is the pointer's size unconditionally and `dehydrate_line` renders it as `released 4.0 MB`, whose doc calls it the bytes this machine no longer holds. With `nlink > 1` the content is still on disk under the other name, so the figure — and any consumer totalling `sizeBytes` — over-counts by the whole file. Nothing in the guard chain reads `nlink`, though the `symlink_metadata` already in hand answers it on unix. Not a safety defect in either direction (the bytes are *more* safe, not less), which is why it is recorded rather than fixed: the honest answers are either refusing a multiply-linked file or reporting zero reclaimed, and choosing between them is a reporting decision that 56.9's space-freed surface should make once, for the sweep and the verb together.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: A selector that is one profile's id and another profile's name makes both folders permanently unreleasable, and the refusal advises the one thing that cannot work.
  evidence: `select` filters `profile.id == wanted || profile.name == wanted` with no precedence, so both match, the single-profile destructure fails, and the message is "`{wanted}` matches 2 folders; name the one you mean by its id" — while the id is precisely what is ambiguous. Pre-existing and shared by every verb that calls `select` (`materialize`, `ls-files`, `pause`, `resume`, `verify`), and the fix — prefer an exact id match before filtering by name — changes the selector for all of them, which is why it is not made inside a story whose surface is one deleting verb. Nothing makes a profile name unique, so the collision is reachable by a user who names a folder after another folder's ULID.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: `lfs::stage::dehydrate`'s post-create cleanup paths are structurally verified but not exercised, because none of them can be forced from a fixture without stubbing the filesystem.
  evidence: All three post-create failures (`write_all`, `File::set_permissions` on an owned handle, `rename` onto a file the identity re-check just proved regular) funnel through one `if published.is_err()` cleanup, so there is a single path to audit rather than three — but on Linux none of them can be provoked from a temp directory, and this tree does not stub the filesystem here. The pre-create refusal is covered (`a_target_whose_length_moved_is_refused_and_leaves_no_staging_file`), and a leaked `.keeper.*.tmp` is tier-0 excluded so it can never be committed; the residual risk is litter nobody sweeps, which is the same gap `.git/lfs/tmp` already has its own ledger entry for.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: A profile with a filesystem remote that also carries a committed `.lfsconfig` can never release anything, because the per-object proof is routed to an LFS server that cannot be addressed.
  evidence: `Engine::remote_serves` takes the filesystem-remote branch only when `lfsconfig.is_none()`, mirroring `Engine::audit_remote_objects` exactly — an explicit `.lfsconfig` names a server, so it outranks the shape of the remote URL, which is the right precedence. The consequence is that such a profile falls to `lfs_access`, which cannot derive an endpoint from a path remote, so every release refuses `UnprovenOnRemote` forever. Fail-closed and consistent with the existing audit, so it is not a defect — but it is a folder shape (a pendrive remote plus a committed `.lfsconfig` pointing at a forge) whose releases silently never happen, and the honest fix is for the proof to say which of the three ways it answered, which is the same change DW (story 56.2) already asks for on `audit_remote_objects`.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: The `.keeper.{name}.tmp` staging spelling adds 12 bytes to a file name, so a path whose name is within 12 bytes of `NAME_MAX` can never be materialized or released.
  evidence: Both `lfs::stage::materialize` and the new `lfs::stage::dehydrate` build the sibling staging name by prefixing `.keeper.` and suffixing `.tmp`, so the create fails `ENAMETOOLONG` every time and the error names the temp file rather than the cause. Inherited rather than introduced — `materialize` has had the shape since story 34.x — and it affects both directions equally, which is why it is recorded against the primitive rather than the verb. The fix is to truncate the base name in the staging spelling or fall back to a hash-derived name, which touches the shipped materialize path and wants its own fixture.
  status: open

- source_spec: spec-56-4-a-release-that-refuses-five-times-before-it-deletes
  summary: `git::resolve`'s `the_three_ways_a_git_can_fail_are_reported_distinctly` fails under whole-suite process pressure while passing in isolation.
  evidence: Observed once during a full three-crate `cargo test` run on this branch (`881 passed; 1 failed`), then 5/5 passing — once alone with `--exact`, three times over the whole `keeper-sync --lib` binary, and in four subsequent full-suite runs (3395 and 3406 passing, 0 failed). The test writes fake `git` shell scripts and immediately execs them, which is the classic `ETXTBSY`/fork-pressure race when a dozen test binaries fork concurrently. It touches nothing story 56.4 changed; what the story contributed is 39 more tests and therefore more concurrent forks. The fixture wants the script written, closed and `fsync`ed — or a retry on `ETXTBSY` — before the exec.
  status: open

- source_spec: spec-56-5-it-lets-go-a-day-after-it-landed
  summary: The release sweep pays a whole-file hash and a remote round trip per candidate before reaching the `OpenUnknown` refusal, so on every production host today each pass spends its whole byte budget to make no progress at all.
  evidence: `Engine::release_resolved` runs `lfs::stage::content_oid` and `Engine::remote_serves` before `platform.open_file_state`, which story 56.4 chose deliberately — the open-file answer is "the answer most likely to have changed while the steps above ran". `OpenFileState::Unknown` is, however, a statement about the *platform* and not about a path (its own doc says no platform answers it yet), so a single capability probe per pass would let the sweep decline before hashing anything, and the cost is real: up to `RELEASE_BUDGET_BYTES` of reads and a budget's worth of round trips per successful sync, forever, on every host, for zero releases. It is recorded rather than fixed because the probe's effect is not observable from a test — the candidate file survives either way — and this epic's own testing rule is that an unassertable change to the verb that deletes data is exactly the change not to make. It wants either a `TestPlatform` call counter (so "did not hash" becomes assertable) or a platform that answers the open-file question, at which point the whole question disappears.
  status: open

- source_spec: spec-56-5-it-lets-go-a-day-after-it-landed
  summary: A folder config layer that fails validation is discarded whole, so any `Allowed` field's committed value silently reverts to the profile's default rather than to the value the file asked for.
  evidence: `profile::folder::in_force` applies `FolderTier::apply`, which returns the layer or a fault for the whole file — so one unknown key or one out-of-range number anywhere in `.keeper/keeper.toml` drops every key in it, and the profile record's defaults take over on every clone that reads the file. Story 56.5 fixed the direction that loses data for the one field whose default deletes, by declining the release sweep when `folder_config_is_faulted` reports a fault against that folder's own layer paths — but the general shape remains for `virtualPatterns`, `virtualOverBytes`, `lfsThresholdBytes` and every future `Allowed` field, and the fault is only visible on the settings surface. The honest fix is per-key rather than per-file refusal (keep the keys that parse, refuse and name the ones that do not), which changes what a fault *means* for every consumer of the tier and needs its own story and its own fixtures.
  status: open

- source_spec: spec-56-5-it-lets-go-a-day-after-it-landed
  summary: No test drives the two release clocks through the writers production actually calls, because the integration fixture has to keep keeper's committer off the paths whose provenance each test plants.
  evidence: `tests/release_sweep.rs` seeds content into the worktree behind keeper's back, which no real arrival does, so the fixture excludes those names from the committer — otherwise keeper's scan meets every seeded path as new local content and `note_local_authorship` correctly overwrites the provenance the test is about. The consequence is that `commit_local` → `note_local_authorship` and `do_lfs` → `note_unit_synced` are covered only at the `db` layer plus a read of the call sites; the wiring itself, including the stale-unit oid check and the post-commit ordering, has no end-to-end fence. Closing it means a fixture that reaches a materialized state the way production does — a real pull into a second clone, or a real commit-then-upload against a filesystem remote — which is a fixture shape this suite does not have yet and which story 56.6's `verify` work will need for its own reasons.
  status: open

- source_spec: spec-56-6-the-checks-stop-calling-the-normal-state-a-fault
  summary: `VerifyReport::virtual_paths` reaches `keeper-syncd` and nothing else, so the desktop surface cannot tell a clean folder from one where ten thousand paths were excused.
  evidence: `sync_verify` (`keeper/src/sync_ipc.rs`) flattens only `report.bad` into `Vec<String>`, so the Settings pane renders `SYNC_VERIFY_CLEAN_SENTENCE` and nothing else after a pass that excused most of the folder. The field's own doc says a row nobody reports anywhere is indistinguishable from a check that stopped running, and that guarantee is honoured on the CLI alone. Carrying the count to the app needs a wire type and a generated binding, which story 56.6 declared a non-goal (`git status --porcelain -- src/lib/ipc/gen` must stay empty) because 56.7 owns the row states and 56.9 owns the surface that reads them. The sentence was reworded so it no longer promises what the pass does not check, which is the honest half of the fix; the count itself belongs with the states.
  status: open

- source_spec: spec-56-6-the-checks-stop-calling-the-normal-state-a-fault
  summary: `Engine::verify` is no longer strictly read-only for a folder that carries a virtualization policy: it opens the repository, which can clear a stale `index.lock` and rewrite `.git/config`.
  evidence: The two-fact excuse needs the index, so `verify` now calls `git::repo::open`, whose door deliberately does housekeeping — `release_stale_index_lock` deletes an `index.lock` older than 60 s, `release_stale_ref_locks` does the same for refs, and `drop_foreign_lfs_driver` rewrites `.git/config` (the DW-140/DW-206 guards). `verify` is the one pass that takes no reservation, so it is the most likely to run beside a keeper commit or a user's own `git`. Narrowed rather than removed: a folder whose policy tier is `Unset` never opens the repository at all, so the overwhelming majority of folders are unaffected, and the comment claiming the pass only reads was corrected. A read-only open (or a reservation for verify) is a change to `git::repo::open`'s contract or to the engine's locking, not to this story.
  status: open

- source_spec: spec-56-6-the-checks-stop-calling-the-normal-state-a-fault
  summary: A FIFO, socket or device node standing where a missing LFS object's path should be still reaches `lfs::stage::clean`, whose `File::open` blocks forever on a FIFO.
  evidence: The new gate asks `lfs::stage::worktree_pointer`, which answers `None` for anything that is not a regular file (`!meta.is_file()`), so a non-regular file falls through to the `clean` arm exactly as it did before this story. Pre-existing — `republish_missing_objects` has always called `clean` on whatever stood at the path — and the fix is the same `Metadata::is_file` filter `copy.rs` already applies before it opens anything (`classify`'s non-regular refusal), applied at the repair site. Left out here because the story's gate is about pointer text and widening it to a general "is this openable" guard is a change to the repair path's own contract, which has no other test coverage.
  status: open

- source_spec: spec-56-6-the-checks-stop-calling-the-normal-state-a-fault
  summary: A verified copy started while its folder is syncing refuses every virtual path with `SyncError::Busy`, and which paths refuse depends on where the ~1 Hz supervisor happens to be.
  evidence: `Engine::materialize_entry` takes the per-profile reservation and answers `SyncError::Busy` when the tick holds it, and the copy renders that sentence as the entry's refusal. The reservation is taken and released per path, so the same copy of the same folder run twice can produce different sets of copied and refused files, and the job still settles `Done`. The direction is safe — a refusal, never a stub — and it is named in the story's own I/O matrix, but a copy is the operation a user reaches for when they want a complete second copy. A bounded retry per path, or one job-level refusal instead of N entry-level ones, is a policy decision about the copy verb rather than a defect in the hydration seam.
  status: open

- source_spec: spec-56-6-the-checks-stop-calling-the-normal-state-a-fault
  summary: `LfsMode::PointerOnly` keeps a whole folder's content as pointers by profile-wide configuration, and `verify` still reports every one of those paths as a fault unless a per-path policy also authorizes it.
  evidence: The excuse story 56.6 built is deliberately per path — the index plus `VirtualPolicy`, which AD-122 requires to be a separate type precisely because `LfsMode` is profile-wide and the ask is per path — so a folder set to `PointerOnly` with no `.keepervirtual` and no `virtualPatterns` produces the same wall of `LFS object … is missing locally` rows it produced before this story. Pre-existing (the mode shipped long before the policy) and deliberately not folded into the excuse conditions, because making the mode an authorization would tie the per-path selector back to the profile-wide lever that AD-122 exists to keep it away from. The honest resolution is for the mode to compile into a policy that authorizes everything, which is a change to how `VirtualPolicy` is built rather than to how `verify` reads it.
  status: open

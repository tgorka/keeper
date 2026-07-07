# Deferred Work

### DW-1: The non-SSS error's "Learn more about Simplified Sliding Sync" link uses a plain `<a target="_blank" rel="noreferrer">`, whose open-in-system-browser behavior inside the Tauri webview is unverified.

origin: migrated from legacy ledger (spec-1-3-password-login-with-sliding-sync-verification.md), 2026-07-06
location: src/components (login error "Learn more about Simplified Sliding Sync" link)
reason: In a Tauri WKWebView, `target="_blank"` often does nothing or navigates the webview rather than opening the default browser; the app bundles `tauri_plugin_opener` but the link does not route through it. The component test only asserts the `href` attribute exists, not that activation opens the browser. Needs a manual `tauri dev` check and, if broken, wiring the link through the opener plugin.
status: open

### DW-2: The `unsupportedLoginType` classification is unreliable — a homeserver with password login disabled typically returns `M_FORBIDDEN`, which `auth::map_login_error` maps to `InvalidCredentials`, so the user sees "Wrong username or password" instead of "password login not supported."

origin: migrated from legacy ledger (spec-1-3-password-login-with-sliding-sync-verification.md), 2026-07-06
location: keeper-core auth::map_login_error
reason: `matrix_auth().login_username()` does not pre-fetch the `/login` flow list, and Synapse returns `M_FORBIDDEN` for a password-login-disabled server (same errcode as a wrong password). The spec's error-kind mapping (`Forbidden`→`InvalidCredentials`, `Unrecognized`/`InvalidParam`→`UnsupportedLoginType`) therefore cannot reliably satisfy the I/O-matrix row for "unsupported login type." A robust fix is a pre-login `matrix_auth().login_types()` / supported-flows check to detect `m.login.password` before attempting login. Low user impact (uncommon scenario, both outcomes are non-retriable password-login failures); deferred rather than re-deriving the flow — the spec consciously chose error-kind mapping and did not mandate a flow pre-check.
status: open

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
status: open

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
status: open

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

### DW-18: Signing out the account that owns an open verification modal tears down its subscription but never resets the verification store, so the modal is stranded open on a now-removed account and a subsequent `close()` fires `verificationCancel` against a dead account.

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
status: open

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
status: open

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

### DW-28: Received audio in an unsupported codec (e.g. Ogg/Opus voice notes, which WKWebView on macOS does not decode natively) renders empty `<audio controls>` forever — the element fires neither `onLoadedMetadata` nor `onError` on a codec-unsupported stall, so no retry/fallback surfaces and AC3 ("received audio plays back inline") silently fails for those clips.

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
status: open

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
status: open

### DW-37: The Pins strip offers no keyboard-accessible way to reorder pins — reordering is native HTML5 pointer drag only.

origin: migrated from legacy ledger (spec-4-3-pins.md), 2026-07-06
location: src/components/.../pins-strip.tsx
reason: `pins-strip.tsx` avatars are `draggable` buttons reordered via `onDragStart`/`onDragOver`/`onDrop`; there is no arrow-key move or move-up/down menu affordance, so keyboard/assistive-tech users cannot reorder (Pin/Unpin themselves remain keyboard-operable via the context menu). The epic's accessibility floor mentions keyboard operability but the spec specified only drag reorder. Deferred as an a11y enhancement (e.g. a keyboard reorder affordance or the Epic 9 single-key verbs) rather than expanding this story's scope.
status: open

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

### DW-57: The known-bot MXID probe (source b) and bot-DM matching (source c) both hard-code the account's OWN server name, so bridge bots living on a separate appservice/bridge domain are invisible to discovery on standard self-hosted mautrix stacks.

origin: migrated from legacy ledger (spec-6-2-bridge-discovery.md), 2026-07-06
location: keeper-core bridges/discovery.rs (bot_network_for)
reason: `bridges/discovery.rs` builds the probe MXID as `@{localpart}:{server_name}` from `own_user.server_name()`, and `bot_network_for` rejects any DM target whose `server_name() != own_server`. Under the common self-hosted topology (user `@me:example.org`, bot `@whatsappbot:bridge.example.org`) sources (b) and (c-bot-DM) both produce nothing and only source (a)/(c-portal) can find the network. The single-domain (Beeper-style) assumption is documented in code docstrings but is not called out in the spec's Boundaries as an accepted limitation, so on a standard mautrix deployment discovery will under-report. A later story should either probe/accept the appservice/bridge domain(s) or record this as an explicit, product-blessed topology constraint. See [[DW-56]].
status: open

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
status: open

### DW-67: Every draft CRUD op (`set_draft`/`get_draft`/`delete_draft`/`list_drafts`) opens a fresh SQLite connection that re-runs all `CREATE TABLE IF NOT EXISTS` + `PRAGMA table_info` column probes; the debounced keystroke save now pays this per-op schema-ensure at typing cadence, hotter than the `pins` precedent it mirrors.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (set_draft/get_draft/delete_draft/list_drafts)
reason: `registry.rs` `set_draft`/`get_draft`/`delete_draft`/`list_drafts` each call `open(data_dir)?`, which runs the pins + drafts `CREATE TABLE IF NOT EXISTS`, `ensure_hue_index_column`, and `ensure_provider_column` on every call. The debounced save fires ~every 200 ms of active typing (fire-and-forget, off the keystroke path, so not user-visible), but a shared/cached connection — or skipping the ensure-columns work on draft writes — would remove the per-write overhead. Pre-existing registry-wide pattern surfaced (not caused) by drafts landing on a far hotter path.
status: open

### DW-68: A pre-edit draft typed within the ~200 ms debounce window and not yet flushed is silently dropped from `keeper.db` when the user enters edit mode and sends the edit, because `send()` cancels the queued debounce; the in-session composer text is restored correctly, but a relaunch after such an edit shows the staler stored body.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: src/components/chat/composer.tsx (send)
reason: `composer.tsx` `send()` clears `draftSaveTimer` + nulls `pendingDraft` before dispatch (correct for reply/text to avoid a save/clear reorder). For `wasEdit`, it then `setDraft(preEditDraft.current)` — restoring the text on screen — but never re-persists it, so a pre-edit keystroke that was still inside the debounce when edit was entered was cancelled without ever reaching the DB. Narrow (requires entering edit <200 ms after typing) and only affects relaunch durability of the pre-edit draft, not the visible composer; a `scheduleDraftSave(preEditDraft.current)` after an edit-send would reconcile the row.
status: open

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

### DW-71: `registry::open` sets only `journal_mode=WAL` with no `busy_timeout`, so a second concurrent writer fails immediately with `SQLITE_BUSY`; the debounced draft save now writes at typing cadence alongside pins/settings/account writes, and because saves are fire-and-forget with a swallowed `.catch`, a `SQLITE_BUSY` silently drops the draft while its in-memory amber marker still claims one exists.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (open)
reason: `registry.rs` `open()` runs `pragma_update(journal_mode, WAL)` and nothing else — no `busy_timeout`, no `synchronous` tuning. WAL serializes writers; without a busy timeout a contended `set_draft` returns `SQLITE_BUSY` instantly rather than waiting, and the fire-and-forget path (`composer.tsx` `flushDraft`) swallows it. Pre-existing registry-wide omission (all callers share `open()`), surfaced — not caused — by drafts landing on the hottest write path. A `PRAGMA busy_timeout` in `open()` would let contended writers wait instead of losing data.
status: open

### DW-72: `set_draft` / the `drafts.body TEXT NOT NULL` column impose no length cap, so a multi-megabyte paste into the composer is shipped verbatim across IPC and rewritten into keeper.db on every ~200 ms debounce flush, an O(n) write on the path the design promised to keep cheap, and unbounded row growth bloats keeper.db.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper ipc.rs (set_draft) + keeper-core registry.rs (set_draft/schema)
reason: `ipc.rs` `set_draft` and `registry.rs` `set_draft`/schema store `body` verbatim with no size guard; `composer.tsx` `scheduleDraftSave` re-sends the whole body each flush. A user pasting a large document pays a full-body IPC + row rewrite per debounce tick and grows keeper.db without bound. A body length cap (truncate-or-reject) before the upsert would bound both.
status: open

### DW-73: Unsent draft bodies — the user's private message *content* — are stored as plaintext `TEXT` in keeper.db, a more sensitive class than the metadata (`pins`) precedent the design mirrors; the app otherwise treats at-rest data as sensitive (tokens in Keychain, an `sdk_encryption` posture), so plaintext message content on disk deserves a conscious documented security decision.

origin: migrated from legacy ledger (spec-7-1-persistent-per-chat-drafts.md), 2026-07-06
location: keeper-core registry.rs (drafts schema)
reason: `registry.rs` `drafts` schema stores `body` as cleartext; the intent contract's "keeper.db conventions" clause was framed against `pins` (room membership metadata), not message content. A stolen-laptop / leaked-backup threat model exposes half-written private messages verbatim. Worth an explicit ADR note confirming plaintext-at-rest is acceptable for draft content, or an encryption pass aligning drafts with the app's other at-rest sensitivity boundaries.
status: open

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
status: open

### DW-78: The header Incognito chip is a one-way binary toggle — once clicked it writes an explicit per-Chat override, and there is no UI affordance to clear that override back to "inherit", so the chat permanently stops following later global/account changes.

origin: migrated from legacy ledger (spec-8-1-incognito-read-receipts-with-scoped-policy.md), 2026-07-06
location: src/components/layout/conversation-pane.tsx (ConversationIncognitoChip)
reason: `src/components/layout/conversation-pane.tsx` `ConversationIncognitoChip` writes `incognitoSetChat(accountId, roomId, !vm.effective)` (an explicit `true`/`false`), and the chip only renders while effective — there is no per-chat "inherit" control (unlike the account menu's tri-state submenu). The story's AC only requires the chip to "toggle the per-Chat scope", which is satisfied; a tri-state per-chat control (e.g. a chip context menu or long-press) is a UX-completeness follow-up beyond this story. Backend precedence and storage already support clearing to inherit (`incognito_set_chat` accepts `Option<bool>`), so this is a frontend-only affordance gap.
status: open

### DW-79: matrix-sdk emits an *implicit public* read receipt as a side effect of sending a message, outside the `signals` sole-gate, so a user with Incognito effective who *sends* a message in the chat still leaks a public read position that Incognito cannot suppress.

origin: migrated from legacy ledger (spec-8-1-incognito-read-receipts-with-scoped-policy.md), 2026-07-06
location: keeper-core signals.rs (mark_read)
reason: The SDK stores implicit read receipts as public (matrix-sdk-ui timeline controller comment "Implicit read receipts are saved as public read receipts") and emits one on message send — a path that never flows through `keeper-core::signals::mark_read`, so neither the AD-14 sole-gate nor the effective-policy branch can intercept it. Story 8.1's scope is read-receipts-on-viewing (the `mark_room_read` path), and sending a message already reveals engagement to the remote, so the marginal privacy loss is small and this was not caused by the change — it is pre-existing SDK behavior surfaced incidentally. Worth a conscious decision: either wire a client-level suppression of implicit/public send receipts to the effective Incognito policy, or scope the `signals.rs` module-doc privacy claim to explicitly exclude the send path so the guarantee is not overstated.
status: open

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

### DW-91: Notification new-vs-backlog classification uses a client-clock baseline against the server's origin_server_ts and tracks no read markers, so a long offline/sleep gap can replay a burst of notifications for messages already read on another device.

origin: migrated from legacy ledger (spec-10-1-native-notifications-from-the-sync-loop.md), 2026-07-06
location: keeper-core/src/notify.rs (should_notify)
reason: `keeper-core/src/notify.rs` captures `baseline_ms = now_ms()` once at handler registration and gates on `event_ts_ms >= baseline_ms` (`should_notify`); the handler is registered once per account lifetime (not per reconnect), so when the SyncService catches up after a long gap every missed event has `origin_server_ts >= baseline` and notifies. Two independent reviewers (Blind Hunter, Edge Case Hunter) flagged the clock-model + wake-storm. Story 10.1 documents the timestamp baseline as an accepted MVP tradeoff; a robust fix (read-marker/high-water-mark awareness, or gating on first-live-sync rather than a cross-clock timestamp compare) is design-level work that fits the notification-rules stories (10.2+).
status: open

### DW-92: Notifications fire even when the app is foreground-focused and the user is actively viewing the room the message arrived in, with no focus/active-room suppression.

origin: migrated from legacy ledger (spec-10-1-native-notifications-from-the-sync-loop.md), 2026-07-06
location: keeper-core/src/notify.rs (register_notify_handler / dispatch)
reason: `keeper-core/src/notify.rs` `register_notify_handler`/`dispatch` have no window-focus or active-room check; a user typing in a Chat gets an OS notification for the message that just appeared on screen. Not required by Story 10.1's ACs (which only require posting sender/Chat/preview, honoring the previews toggle, and no push egress), so it was not in scope, but it is standard notification behavior worth an explicit later decision alongside the mute/mention-only/DND rules (Story 10.2) or background/foreground semantics (Story 10.3).
status: open

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

### DW-99: Exact-message / exact-re-login deep landing on a notification click is deferred to Epic 11. Under the Option B MVP scope, a notification click summons+focuses the window and lands only on a coarse view (Message → Inbox, Bridge → Bridges) driven by the app-side "last notification target" recorded at dispatch; it never lands on the exact Chat/Account/message or auto-opens the specific Bridge's re-login sheet.

origin: migrated from legacy ledger (spec-10-4-click-through-and-bridge-health-alerts.md), 2026-07-06
location: keeper-core/src/notify.rs + keeper/src/ipc.rs (record_last_notify_target)
reason: The kept `tauri-plugin-notification` 2.3.3 desktop backend has NO per-notification click callback (its `show()` is a fire-and-forget wrapper over `notify_rust`; `action_type_id`/`onAction` are mobile-only), so per-notification target routing is impossible on this backend — confirmed during planning (spec Design Notes 1-3). The coordinator resolved this as Option B (2026-07-06): keep the backend, ship summon+focus + coarse landing now, defer exact landing. The full `NotifyTarget::Message { account_id, room_id, event_id }` / `NotifyTarget::Bridge { account_id, network_id }` payload already ships (attached at dispatch in `keeper-core/src/notify.rs`, recorded shell-side in `keeper/src/ipc.rs::record_last_notify_target`, emitted coarsely on `RunEvent::Reopen` via `emit_notify_navigate` → `notify://navigate`), and the frontend `bridgeRelinkStore` already carries the `(accountId, networkId)` target — so Epic 11 can consume it without a new contract. Fix in Epic 11 (where the signed .app bundle exists to validate ≥99% delivery reliability + click routing): swap the desktop notifier to a click-capable backend (`mac-notification-sys` `wait_for_click(true)` with a shell-side id→target map, or `UNUserNotificationCenter`), then route `NotifyTarget::Message` to `roomsStore.requestFocus(accountId, roomId, eventId)` (exact Chat/Account/message) and `NotifyTarget::Bridge` to auto-open that network's re-login sheet (`bridgeRelinkStore` → `BridgeLoginSheet`). See [[DW-100]].
status: open

### DW-100: The Option B coarse "last notification target" single-slot mechanism has three edge behaviors that only a click-capable backend (Epic 11) can properly fix — (a) any plain macOS dock-icon activation after an ignored notification navigates to the coarse view (stale-target over-navigation), (b) a bridge-drop target is overwritten by any later message notification's target (last-write-wins clobbers the higher-value bridge landing), and (c) a rare emit-before-subscribe race can drop the very first navigate if the webview listener has not mounted yet.

origin: migrated from legacy ledger (spec-10-4-click-through-and-bridge-health-alerts.md), 2026-07-06
location: keeper/src/lib.rs (emit_notify_navigate) + keeper/src/ipc.rs (record_last_notify_target)
reason: `keeper/src/lib.rs` fires `emit_notify_navigate` on EVERY `RunEvent::Reopen` (macOS gives no way to distinguish a notification click from a plain dock activation on the kept `tauri-plugin-notification` backend), and `keeper/src/ipc.rs::record_last_notify_target` unconditionally overwrites a single `LAST_NOTIFY_TARGET` slot (reset to `None` only after one consume, so the first post-notification activation — however delayed — still navigates, and a `Bridge` target is clobbered by subsequent `Message` posts). `emit_notify_navigate` is a one-shot emit+reset with no replay, so a navigate emitted before `use-notify-navigate`'s `listen` subscribes is lost. These are inherent to the frozen Option-B coarse scope (spec `[AMENDED-B]`: "coarse view landing driven by app-side last notification target… never exact-message routing"), NOT implementation bugs — the story faithfully realizes the accepted mechanism. Epic 11's click-capable backend (`mac-notification-sys` `wait_for_click(true)` / `UNUserNotificationCenter`) carries a per-notification payload and a real click signal, which structurally resolves all three: navigation fires only on an actual click (no stale/plain-activation over-navigation), each click routes its own target (no single-slot clobbering), and the click delivers the target directly (no emit-before-subscribe race). Track alongside the exact-landing entry above. See [[DW-99]].
status: open

### DW-101: The JS license firewall (`scripts/check-js-licenses.ts`) classifies any SPDX expression containing a copyleft token as `deny` ("deny wins"), so a legitimately dual-licensed `permissive OR copyleft` dependency (e.g. `MIT OR GPL-2.0`) hard-fails CI with no override mechanism; consider OR-aware classification (allow when any OR-branch is fully permissive) plus a reviewed per-package exceptions map (like cargo-deny's per-crate exceptions).

origin: migrated from legacy ledger (spec-11-1-signed-notarized-release-pipeline.md), 2026-07-06
location: scripts/check-js-licenses.ts (classifyLicense)
reason: `classifyLicense` in `scripts/check-js-licenses.ts` tokenizes on OR/AND and returns `deny` on the first copyleft token regardless of operator; the I/O matrix in `spec-11-1` (inside the frozen intent-contract) specifies `(MIT OR GPL-2.0-only)` → deny and the test encodes it. This is intentional and safe (never leaks copyleft), and no current dependency triggers it (0 denied across 527 packages), but it is over-conservative — a future dual-licensed transitive dep would red the build and the only in-place fix is hand-editing the classifier. Not fixed now because it contradicts the frozen I/O matrix; revisit as an enhancement with an override channel.
status: open

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

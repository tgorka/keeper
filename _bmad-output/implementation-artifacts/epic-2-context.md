# Epic 2 Context: Every Account, One Inbox — Multi-Account, OIDC & Beeper

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic breaks the single-account limit established by the walking skeleton and makes keeper a true multi-account client. A user can run an unlimited number of concurrent Accounts — across password, OIDC (MAS/MSC3861), and Beeper email-code JWT logins — all funneled through one authentication interface and merged into a single chronological inbox. It also lands the honest-disclosure surfaces that keeper's trust posture depends on (Beeper's unofficial-API framing and On-Device coverage gaps) and the first-run at-rest encryption choice for SDK stores. It matters because it delivers keeper's defining promise — every network and every account in one place — while proving the per-account supervision, inbox-merge, and auth-provider abstractions that every later epic builds on. Its exit check is OQ-3: verifying the real Beeper (hungryserv) surface and recording per-feature degradations.

## Stories

- Story 2.1: Account Manager — Unlimited Concurrent Accounts
- Story 2.2: OIDC Login (MAS / MSC3861)
- Story 2.3: Beeper Email-Code Login
- Story 2.4: Beeper Coverage Disclosure
- Story 2.5: Account Switcher and Per-Account State
- Story 2.6: At-Rest Encryption First-Run Choice

## Requirements & Constraints

- Support an unbounded number of concurrent Accounts. No code path may enforce or assume an account-count limit — adding the Nth account must behave identically to adding the second. Accounts may share or differ in homeserver.
- Every login path (password, OIDC, Beeper) must produce a syncing Account with access/refresh tokens stored only in the macOS Keychain, never on disk or crossing into JavaScript.
- The Simplified Sliding Sync capability gate applies identically to all login types: a non-SSS server must fail before any Account state, store directory, or Keychain entry is created, with an error that names SSS.
- Cancelling or abandoning any browser/OIDC flow must leave zero residue — no partial Account, store directory, or Keychain entry.
- All Chats from all Accounts merge into one recency-ordered inbox; send and receive must work independently per Account.
- Beeper login must degrade honestly: a distinct, named "Beeper login unavailable" failure state (with Retry and a status link) on reject/timeout/shape-change — never a hang, spinner, or crash — and that failure must be unobservable from non-Beeper Accounts.
- Beeper coverage disclosure must appear before login completion (naming the specific broken expectation, requiring acknowledgment) and remain permanently accessible in that Account's settings.
- The Beeper tab's "Unofficial API — may break without notice" label is a permanent part of the form, not a dismissible hint.
- First-run at-rest encryption is an opt-in passphrase choice for SDK stores only, default off (FileVault posture). Settings copy must state honestly that `archive.db`/`keeper.db` are not passphrase-encrypted in this version. The chosen posture applies to later account adds without re-prompting.
- Sign-out tears down only that Account's own tasks and rows and deletes only its SDK dir + Keychain entries. Destructive archive deletion is out of scope here (completed in Story 5.7); until then the sign-out dialog defaults to keeping the local archive.
- Exit check (OQ-3): against a real Beeper Account, verify the hungryserv surface (`thirdparty/protocols`, custom account data, `m.read.private`, push rules) and record any gaps as per-feature degradation notes for later epics.

## Technical Decisions

- **AuthProvider trait (AD-17):** All login flows implement a single `AuthProvider` trait with three impls — `password`, `oidc` (SDK OAuth + system browser + `keeper://oauth/callback` deep link), and `beeper`. Story 2.1 extracts the trait with password as the first impl; 2.2 and 2.3 add the other two.
- **Beeper containment (AD-17):** The Beeper flow is `/user/login` → `/user/login/email` → `/user/login/response` → JWT → `org.matrix.login.jwt` against matrix.beeper.com. All api.beeper.com / matrix.beeper.com HTTP must live in the beeper auth module only, with typed failure states, so private-API breakage cannot bleed into core Matrix login.
- **Per-account supervision (AD-3, AD-19):** Each Account is one `matrix_sdk::Client` with its own store at `accounts/<account_id>/sdk/`. `AccountManager` owns a registry of `AccountHandle`s, each supervising its own Client, SyncService, streams, archiver, signals, and send scheduler on tokio, with per-account `tracing` spans. No global mutable state or ad-hoc singletons; cross-account aggregators consume per-account streams.
- **Inbox merge in Rust (AD-20):** The Unified Inbox is computed in `keeper-core::inbox` by merging N `RoomListService` streams, ordered by recency, and streamed to the UI as a single windowed VM (visible range + buffer, with totals). Ordering and filtering are never re-derived in TypeScript.
- **Storage & secrets lifecycle (AD-10):** Keychain service is `dev.tgorka.keeper`; it holds tokens, recovery keys, and store passphrases only. `keeper.db` holds the account registry; `archive.db` is shared across accounts keyed by `account_id`. All SQLite in WAL mode. Logout deletes the SDK dir + that account's Keychain entries and nothing else.
- **At-rest posture (AD-22, amends NFR-10):** SDK stores use matrix-sdk-sqlite's native passphrase (first-run choice; generated key kept only in Keychain). `archive.db`/`keeper.db` ship without passphrase encryption in MVP — FTS cannot index ciphertext and SQLCipher conflicts with matrix-sdk-sqlite's bundled SQLite linkage. This must be stated honestly in settings copy.
- **Per-account hue:** Each Account is assigned a hue at add time from an 8-hue wheel, rendered as a 3 px edge bar on chat rows and as a hue dot in the switcher.

## UX & Interaction Patterns

- **Trust surfaces (UX-DR17):** Permanent "Unofficial API" subtitle on the Beeper tab; the coverage-gap disclosure card shown pre-completion and again in Account settings; the archive-encryption honesty copy in Settings → Archive & Storage. Copy follows the voice rules — sentence case, name the consequence plainly, no softening.
- **Account switcher (UX-DR18):** Lives in the sidebar footer; lists every Account with avatar, hue dot, homeserver, and a sync-state glyph (syncing spinner / synced / offline gray) driven by the account status stream, updating within one sync cycle with no toast spam. Always includes an "Add Account" entry that is never count-gated. Clicking an Account filters the inbox to it (click again to clear).
- **Sign-out dialog (UX-DR20):** Each Account row's menu offers Settings and "Sign out…", the latter opening an AlertDialog whose default is "Sign out, keep local archive." (The typed-account-name destructive archive-deletion path is deferred to Epic 5.)

## Cross-Story Dependencies

- Story 2.1 depends on all of Epic 1 (single-account walking skeleton) and is the foundation for the rest of Epic 2 — it extracts the `AuthProvider` trait and stands up `AccountManager`.
- Stories 2.2, 2.3, 2.5, and 2.6 all depend on 2.1. Story 2.4 (coverage disclosure) depends on 2.3 (Beeper login).
- FR-6 (account management) and FR-18 (unified inbox) are only partially completed here: full unified-inbox organization lands in Epic 4, and the keep/delete-archive sign-out semantics complete in Story 5.7.
- Story 2.3's OQ-3 verification produces degradation notes consumed by later epics (bridges, incognito, notifications).

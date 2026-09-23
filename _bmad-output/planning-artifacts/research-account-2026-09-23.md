---
name: 'keeper'
type: research
topic: 'an optional account: OIDC sign-in whose token is a general credential, plus a per-person config repository (git) synced to every device and bootstrapped from one input (setup link, QR code or paste)'
decision: 'what keeper, as it stands on main, can build an optional organisation account from, given a Matrix-only OIDC flow, a frozen layer stack, a git engine whose credential is Basic-only and whose push runs outside gix, a deep-link handler with no cold start and no iOS scheme, QR codes that are display-only, and a public-native-client posture; and what the outside world (OIDC libraries, identity providers, Apple auth sessions, forges) makes possible and what it forbids'
status: final
created: '2026-09-23'
run_folder_note: 'the seven digests below were read as local:// artifacts of the coordinating session (acct-GAuth, acct-GConfig, acct-GSync, acct-GSurface, acct-ROAuthUI, acct-ROidc, acct-RForgeGit); filing them under research/ is the coordinator’s step'
digests:
  - ROidc — OIDC client mechanics in Rust: crate choice and measured dependency weight, the ID-token validation checklist for a public native client, RP-initiated logout, where roles and groups appear per identity provider, the access token as a credential for other services, refresh rotation and token storage
  - ROAuthUI — native OAuth sign-in UI for a Tauri 2 app: ASWebAuthenticationSession on macOS and iOS, passkeys and 1Password inside it, Android Custom Tabs and Auth Tab, the existing Tauri plugins and Rust crates, RFC 8252, and a per-platform recommendation
  - RForgeGit — git over HTTPS with OAuth tokens: Gitea, Forgejo, GitLab and GitHub as OAuth providers and as git servers, presenting an external identity provider's token to git, gitoxide and libgit2, and forges under a sub-path
context:
  - GAuth — keeper's auth, OIDC, deep-link, browser and keychain surface (path:line)
  - GConfig — the config layer stack and the "Set by a file" display (path:line)
  - GSync — the git sync engine's remote, credential and clone surface (path:line)
  - GSurface — the account feature's UX surface map, house rules and numbering ceilings (path:line)
  - spec-82-account-and-config-repo-proposal.md — the owner-approved model (2026-09-23), precedence amended
  - acct-contract — the frozen implementation contract and its amendments A1 (the module is `org_account`) and A2 (egress, Forget, drive credential spelling)
---

# Research: an optional account, and a config that follows you

**Evidence grades.**
- `[SOURCE]`: an external primary source, read on 2026-09-23. It is cited with the digest's source id, re-prefixed per digest so the ids stay unique (§11).
- `[REPO]`: read out of this worktree at `origin/main` tip `6fbc2c3`, cited `path:line` as the grounding digest recorded it. §1.3 lists the lines this pass opened again.
- `[INFERENCE]`: reasoning over cited facts, with no source of its own.
- `[UNVERIFIED]`: looked for and not found. Never repeat one as fact. §10 is the complete inventory.

**How to cite this document.** Sections are numbered `§N.M` and are stable. Cite as `research-account-2026-09-23.md §6.2`, the convention `research-notes-search-2026-09-19.md` follows.

**Source ids.** Each research digest numbered its sources from S1, so the ids collide. This document prefixes them:
- `OIDC-Sn` / `OIDC-Mn`: ROidc's sources and measurements;
- `UI-Sn`: ROAuthUI's sources;
- `GIT-Sn`: RForgeGit's sources.

The number after the prefix is the digest's own, so a reader holding a digest can find the line.

**What this document is not.** It does not design the feature. The owner-approved proposal (`spec-82-…`), the frozen contract and `epic-82-an-optional-account-and-a-config-that-follows-you.md` do that. The decisions AD-308…AD-316 were pinned before this synthesis. This document is the evidence under them. Where the evidence pulls against a pinned choice it says so in §9.2 and does not resolve it silently.

---

## 0. Reading guide

`[INFERENCE]` over the contract's pinned decisions and the sections below.

| Decision | What it rests on | Sections |
| --- | --- | --- |
| **AD-308**: the descriptor is its own store, outside the layer stack | The parser refuses unknown tables and keeps all tier policy in predicates. A descriptor inside the stack would be read by the stack it feeds, and the repository could redirect itself. `HOME` on iOS is unverified. The issuer must be an exact string. | §3.1, §3.4, §3.8, §6.2 (A1), §8.6 |
| **AD-309**: two account tiers between `~/.keeper` and the main folder, live, with `mainSyncFolder`/`[folder]` refused | Six tiers where declaration order is precedence; the predicates are the extension surface. The `OnceLock` premise ("never changes"). The folder tier is the live precedent. `may_set_main_folder` is `~/.keeper` only (AD-101). | §3.1, §3.4, §3.5, §3.8 |
| **AD-310**: keeper's own OIDC client on `openidconnect` 4 without default features, one keychain item per session, a single refresher | The Matrix flow is welded to a matrix-sdk `Client` and the SDK owns PKCE. No JWT code exists. reqwest 0.13 with rustls, and `oauth2-reqwest` already in the lock. Rotation is universal. | §2.1, §2.5, §2.6, §6.1, §6.2, §6.6 |
| **AD-311**: the browser is `Platform::start_web_auth` (ASWebAuthenticationSession, non-ephemeral), results routed by `OAuthFlowRegistry`; loopback on desktop | The registry is SDK-agnostic. There is one deep-link handler, with no cold start and no iOS scheme. The session's callback needs no Info.plist entry. Passkeys work inside it. The plugin landscape is weak. RFC 8252. | §2.2, §2.3, §2.4, §2.7, §7 |
| **AD-312**: config repo: core plans, sync moves bytes, shell joins; own-dir-only; create-only; push via `push_http` on every platform | AD-40's crate split. The direct-gix route beats a hidden profile. gix cannot push, and keeper already pushes over smart HTTP on the phone. The credential callback is Basic-only. | §4.2, §4.3, §4.6, §4.7, §8.5 |
| **AD-313**: `user.toml[identity_field] == sub` (and `issuer` when recorded) | (`iss`, `sub`) is the only stable identifier. Zitadel deployments record their own id field. | §6.2 (D13k), §6.4 |
| **AD-314**: device name and class come from keeper, never a token | `hostname` does not exist in the iOS sandbox. No device class or idiom exists in the tree. | §3.2, §3.7 |
| **AD-315**: the account as a credential is opt-in per drive / bot provider; keeper-sync unchanged | Credentials are keychain-namespaced per owner. Each consumer of a token has its own spelling. A token with N audiences can be replayed at any of them. Which forge accepts which spelling. | §4.2, §5.5, §6.5, §8.1, §8.4 |
| **AD-316**: the forge API base is explicit, never derived | `forge_api_target` mis-parses sub-path forges. The URL alone is ambiguous across forges. | §4.4, §8.6 |
| **UX-DR116**: an optional account | Settings order and the ungated precedent; display-only QR on a white card; Sheet vs Dialog vs AlertDialog; errors are Rust's sentences. | §5 |

---

## 1. The question and evidence grades

### 1.1 The question

`[REPO]` `spec-82-account-and-config-repo-proposal.md:10-19`. The owner's ask, verbatim (Polish, kept as-is):

> logowanie za pomoca oauth i uzywanie konfiguracji (sciaganie lub tworzenie) na
> remote - git jak drive ale dostep bedzie juz za pierwszym uruchomieniem. Miej na
> uwadze zeby UI i UX byly spojne i dobre do uzycia - dodaj zarzadzanie
> uzytkownikami - moze onboarding nowych uzytkownikow przez qrcode (wygenerowanie
> przez keepera podczas onboardingu)
> dostep do konta auth ma pozwalac sync konfiguracji i latwiejsze dodawanie innych
> drivow oraz laczenie do botow itp (jezli jest to sync po oauth) - jak nie ma
> internetu to wszystko chodzi normalnie to co jest offline.

The task, as the coordinator stated it: "optional account sign-in (OIDC) + per-person config synced from a git repository".

`[INFERENCE]` This document reads the ask as five questions:
1. What does keeper need in order to sign a person in with an OAuth/OIDC provider it did not ship with, on macOS and iOS, and what does it already have?
2. Can a configuration living in a git repository be reached from the first launch, fetched or created there, and applied like the layer files keeper already reads?
3. What is the smallest honest "user management" when keeper has no server? The answer points at QR/link onboarding that keeper generates.
4. Can the sign-in token serve as the credential for drives and bots, and on which forges?
5. What does "without internet, everything that works offline keeps working" require of each piece?

### 1.2 Method

`[REPO]` digest headers; `[SOURCE]` research digest headers. Seven read-only digests, all produced on 2026-09-23:
- four over the repository at `6fbc2c3`;
- three over the outside world, read from primary sources. These are specifications (OIDC Core, Discovery, RP-Initiated Logout, RFC 6749/7009/8252/8707/9068/9700), vendor documentation (Apple, Google, Zitadel, Keycloak, authentik, Authelia, Gitea, Forgejo, GitLab, GitHub, 1Password), forge and library source code at named branches, crates.io metadata, and three measurements taken in a scratch crate (`OIDC-M1…M3`).

| Digest | Slice | Grade |
| --- | --- | --- |
| GAuth | `AuthProvider` and its implementors, `OAuthFlowRegistry`, deep-link wiring, URL schemes and the iOS gap, browser-open, the keychain port and `SecretCache`, reqwest/TLS, JWT code (none), `url`, the objc2/unsafe inventory, Android, `CapabilitiesVm`, reuse of the Matrix OIDC pieces | `[REPO]` |
| GConfig | the six-tier stack, the tier predicates, `<host>`, `keys.rs` scopes, parsing/faults/install, the `[folder]` tier, faults to the UI, iOS, reload (none), per-device identity, the extension sites for account tiers | `[REPO]` |
| GSync | how a drive is added (clone vs adopt), HTTPS credentials, Bearer vs Basic, the forge API base bug, LFS auth, cadence and bounds, offline as a state, iOS gix-only, keeper-syncd, a hidden config repo, licences | `[REPO]` |
| GSurface | the first-run wizard, the Matrix account UI and sign-out, Settings structure and its gate rule, QR display (no scanning), bots credentials, the add-drive form, the house rules, numbering ceilings, proposed screens | `[REPO]` |
| ROidc | `openidconnect`/`oauth2`/`oauth2-reqwest`/`openid`/`jsonwebtoken`; the validation checklist; logout; roles per IdP; tokens for other services; refresh and storage | `[SOURCE]` |
| ROAuthUI | ASWebAuthenticationSession; passkeys and 1Password; Auth Tab and Custom Tabs; plugins and crates; RFC 8252; a per-platform recommendation | `[SOURCE]` |
| RForgeGit | the per-forge acceptance table; Gitea/Forgejo as OAuth providers for git; GitLab and GitHub; external IdP tokens at git; gix and git2; sub-path forges | `[SOURCE]` |

### 1.3 Grades, and what this pass re-verified

`[REPO]` This pass opened the `path:line` citations below against the worktree at `6fbc2c3`. `git status` showed no lane edits yet, only the untracked spec. Every one located the claimed code:
- **Exactly where a digest said:**
  - `keeper/src/voice_reach.rs:101` (`install_deep_link`);
  - `keeper-sync/src/engine.rs:36685` (`forge_api_target`) and its flat-forge test at `:25332`;
  - `keeper-core/src/bridges/login.rs:42` (`qr_svg`);
  - `keeper/tauri.conf.json:52-55` (`deep-link.desktop.schemes: ["keeper"]`);
  - `keeper-core/src/auth.rs:44` (`OAUTH_TIMEOUT` = 300 s) and `:64` (`AuthProvider`);
  - `keeper-core/src/platform.rs:38`/`:49` (`keychain_get`/`open_url`).
- **Moved, or recorded loosely:**
  - `keeper-core/src/oauth.rs` `REDIRECT_URI` is at `:31`. GAuth said `:27`; GSurface and the spec say `:30-31`.
  - `keeper-core/src/config/mod.rs` `LayerTier` is at `:87` (GConfig said `:95-105`), `may_set_main_folder` at `:179-181`, `read_host_label` at `:379`, `static LAYERS: OnceLock` at `:835` and `setting_override` at `:864`. GConfig's `:141-150`, `:333`, `:777` and `:794` are therefore off by roughly 30–90 lines.
  - `keeper-sync/src/credential.rs` `AccessToken::git` is at `:58-61` (GSync said `:59-66`), and the rule *"A fourth consumer gets a method here or it does not get the token"* is at `:21` (GSync said `:37`).
  - `keeper-sync/src/git/push_http.rs` `push` is at `:127` (GSync said `:128`).

Every other `[REPO]` line is as its digest recorded it, and may be off by a similar margin. None of the drift changed a claim. The architecture decisions quoted in §9 were read at `ARCHITECTURE-SPINE.md:187` (AD-27), `:293` (AD-40), `:298` (AD-41) and `:358` (AD-53).

`[SOURCE]` grades are carried from the digests unchanged. Each research digest separates facts (sourced) from implications (`[INFERENCE]`), and this document keeps that split. Where a research digest corrected a brief or another source, the correction is kept with its source (§6.4 item 2, §6.4 item 3).

### 1.4 Numbering ceilings

`[REPO]` GSurface §8; `epic-81-a-space-is-a-selection-and-an-empty-note-leaves.md:5`; `docs/decisions.md:1207`.
- **Previous ceilings:** FR-676, NFR-91, AD-307, UX-DR111, DW-289, D-24. D-24 is at `docs/decisions.md:1207`.
- **FR-677…FR-680** of epic 81's block, and **UX-DR112…UX-DR115** of epic 79's reserved block, stay unallocated.
- **Epic 82 allocates:** FR-681…, NFR-92…, AD-308…AD-316, UX-DR116, DW-290… and D-25.
- **Checked:** a repo-wide grep for `AD-308…AD-319`, `FR-681…FR-689`, `NFR-92…NFR-99`, `UX-DR116`, `DW-290…DW-299` and `D-25` found no prior use.

---

## 2. keeper's auth surface as it is

All `[REPO]`, GAuth unless noted.

### 2.1 The one OIDC flow is Matrix's, and matrix-sdk owns its PKCE

- **The seam is Matrix-specific.** `AuthProvider::authenticate(&self, client: &Client, platform: &dyn Platform)` takes a matrix-sdk `Client` and must leave it carrying a live session (`keeper-core/src/auth.rs:64-81`, re-verified at `:64`). The implementors are `PasswordAuthProvider` (`:89-137`), `OidcAuthProvider` (`:151-230`) and `BeeperAuthProvider` (`auth/beeper.rs:225+`).
- **What `OidcAuthProvider` does.** It runs `client.oauth()` discovery (MAS/MSC3861), then `oauth.login(redirect_uri, …).build()`, and registers the state in the shared registry **before** opening the browser. It then calls `platform.open_url`, awaits the callback for 300 s (`OAUTH_TIMEOUT`, `:44`), and finishes with `finish_login`. An RAII `FlowGuard` removes the registry entry on every exit (`:156-241`).
- **matrix-sdk owns PKCE.**
  - `PkceCodeChallenge::new_random_sha256()` runs inside `OAuthLoginBuilder::build`, and the verifier is stashed in an in-memory map keyed by the CSRF state (matrix-sdk 0.18 `auth_code_builder.rs:135-160`).
  - `finish_login` applies it (`mod.rs:1033-1044`).
  - There is no API to supply or extract a verifier.
- **Persistence.** `StoredSession` is a tagged enum (`Password(MatrixSession)` | `Oauth { client_id, user }`) stored under the keychain key `session/<account_id>` (`auth.rs:255-277`, `:397-399`). Tokens never reach disk or IPC (`:251-254`; AD-1/NFR-9).
- **Consequence.** `client.oauth()` cannot serve a generic identity provider. A generic client must run its own authorization-code + PKCE flow (GAuth §13).

### 2.2 `OAuthFlowRegistry` is reusable as it is

- It is deliberately free of Tauri and Matrix: tokio one-shots behind `Mutex<HashMap<state, Sender>>`, plus `url` parsing (`keeper-core/src/oauth.rs:7-11, 76-79`).
- The API:
  - `register(state)` returns the receiver;
  - `remove(state)` is idempotent;
  - `resolve(url)` parses `state`/`error`, removes the entry and sends `Redirect(url)` or `Error`. An unknown, spurious or unparsable URL is ignored;
  - `cancel_all()` drains every entry with `Cancelled` (`:99-230`, tests `:302+`).
- `REDIRECT_URI = "keeper://oauth/callback"` (`:31`). `registration_data()` builds RFC 7591 metadata: a native application, the auth-code grant, and no secret (`:271-300`).
- The SDK matches the callback by state internally as well (`InvalidState`, matrix-sdk `oauth/error.rs:156-159`), so the registry is a routing layer, not the security boundary (GAuth §2).
- `[INFERENCE]` For a keeper-owned client the registry becomes the routing layer again. The security check (state equality, consumed once) moves into keeper's own code (§6.2 C10–C11).

### 2.3 Deep links: one handler, no cold start, no iOS scheme

- **One handler.** `.plugin(tauri_plugin_deep_link::init())` is registered for all targets (`keeper/src/lib.rs:261`). One handler is installed in `setup()`: `voice_reach::install_deep_link(app.handle(), move |url| flows.resolve(url))` (`lib.rs:444-445`). The plugin keeps exactly one handler, and a second `on_open_url` replaces the first (`voice_reach.rs:99-101`).
- **Routing.** `keeper://voice/talk` is performed first. Every other URL goes to `flows.resolve(url)` (`voice_reach.rs:108-112`). A `keeper://setup?descriptor=…` link would reach `resolve`, which ignores it because it has no `state`. A new arm in the one handler is required (GAuth §3, GSurface §2).
- **No cold start.** `deep_link().get_current()` has zero hits in the shell. A link that launches keeper is never delivered (GAuth §3).
- **Desktop only.** The scheme is registered in `tauri.conf.json:52-55` (`deep-link.desktop.schemes: ["keeper"]`, re-verified).
- **Nothing on iOS.** No `CFBundleURLTypes` exists in `gen/apple/keeper_iOS/Info.plist`, `Info.ios.plist`, `project.yml` or the pbxproj. There are no associated domains, and the entitlements carry data protection only (GAuth §4).
  - The iOS bundle id is `dev.tgorka.keeper` (`project.yml`).
  - `[INFERENCE]` Launch Services therefore cannot deliver `keeper://setup` or `keeper://oauth/…` to the iOS app today. That iOS requires the scheme in `CFBundleURLTypes` for a *deep link* is platform knowledge, `[UNVERIFIED]` in GAuth. For the *auth-session callback* §7.1 F1.15 shows it is not required.

### 2.4 The browser today

- `Platform::open_url` (`keeper-core/src/platform.rs:49`) is `tauri_plugin_opener::open_url` on desktop (`keeper/src/ipc.rs:709-714`) and on iOS (`:894-898`).
- No `ASWebAuthenticationSession`, `SFSafariViewController` or AuthenticationServices code exists (grep over `src-tauri`: zero).
- The plugins are opener, deep-link, dialog and notification, plus desktop-only global-shortcut, autostart, updater and process. None is an auth-session plugin (GAuth §5).

### 2.5 The keychain port and `SecretCache`

- `Platform::keychain_set/get/delete` (`platform.rs:31-42`, `keychain_get` re-verified at `:38`) uses the service `dev.tgorka.keeper` (`ipc.rs:513`).
  - **Desktop:** the `keyring` crate (`ipc.rs:657-705`). The Linux `linux-native` keyutils backend loses sessions at reboot, which is recorded in `crates/keeper/Cargo.toml`.
  - **iOS:** `security_framework`, with `SecAccessControl` pinned to `AfterFirstUnlockThisDeviceOnly` (`ipc.rs:836-885`).
- `SecretCache` (`platform.rs:104-224`, struct at `:140`):
  - one memo per key, which also holds a remembered absence; failures are not held;
  - invalidated on set and delete;
  - reads run with the lock released, because a read can block on a modal ACL dialog (`:171-186`).
  - Rationale: macOS re-evaluates the item ACL on every read that returns data, which means one dialog per item per launch (`:107-115`).
- `[INFERENCE]` Each new keychain item is one more potential macOS prompt per launch. This is the grounding for AD-310's "one item per session" (GSync implication 5; spec §3.2).

### 2.6 HTTP, TLS and JWT

- **HTTP.** `reqwest 0.13` with `default-features = false`, `["json", "rustls"]` (`src-tauri/Cargo.toml:85`). It is rustls-only on purpose, to avoid a second TLS stack beside matrix-sdk (`:84`). Existing users: Beeper, bots, telemetry, provisioning, the notes embeddings client, and keeper-sync with its LFS clients (GAuth §7).
- **JWT.** No JWT or JWKS crate, and no id_token validation, anywhere. Beeper's `parse_jwt` only extracts a string (`auth/beeper.rs:304`).
- **Already in the lock.** `oauth2` and `oauth2-reqwest` arrive through matrix-sdk (`Cargo.lock:4620-4621`), and so does `url = "2"` (`src-tauri/Cargo.toml:71-72`) (GAuth §8, §9, §13).

### 2.7 The unsafe and objc2 precedent

- `unsafe_code` is denied workspace-wide. The shell crate may carry audited, function-level `#[allow(unsafe_code)]` exceptions for platform FFI with no safe binding (`docs/constraints-and-limitations.md:96-98`).
- The inventory (`:94-257`) includes:
  - `IosPlatform::exclude_from_backup` (`ipc.rs:935-979`);
  - the macOS PDF export;
  - 28 functions in `voice_ios.rs` and 22 in `voice_macos.rs`;
  - the Live Activity `extern "C"` bridge.
- AuthenticationServices bindings are **not** in the tree today (GAuth §10). `[INFERENCE]` Adding `objc2-authentication-services` is "only the dependency edge is new", in the words the Cargo.toml comments use for their predecessors.

### 2.8 Android, and capability flags

- **Android.** There is no Android target: no `gen/android`, no Android dependency table, and the mobile compile seam fails loudly for a non-iOS mobile target (`ipc.rs:460-464`, `keeper-sync/src/platform.rs:542-544`; GAuth §11).
- **`CapabilitiesVm`.** It has 15 fields and is computed in the shell, never with `cfg(target_os)` in core (AD-26; `vm.rs:159-262`, `ipc.rs:1424+`). Every flag is "absent rather than disabled" (AD-27; GAuth §12).
- **Stale docs.** GSync §8 found that `CapabilitiesVm.sync` is true on iOS since Epic 66, and that the field doc at `vm.rs:~180-193` and `bots/http.rs:9-10` predate that.

---

## 3. The config layer stack as it is

All `[REPO]`, GConfig unless noted.

### 3.1 Six tiers; declaration order is precedence

- `config/mod.rs:12-24` documents the order:

  ```text
  ~/.keeper/keeper.toml                 user, every machine, every folder
  ~/.keeper/keeper.<host>.toml          user, THIS machine
  <main>/.keeper/keeper.toml            the main sync folder, every machine
  <main>/.keeper/keeper.<host>.toml     the main sync folder, THIS machine
  <folder>/.keeper/keeper.toml          that folder only
  <folder>/.keeper/keeper.<host>.toml   that folder, this machine
  ```

- `LayerTier` (`mod.rs:87`, re-verified) runs `UserGlobal` → `UserGlobalMachine` → `MainShared` → `MainMachine` → `FolderShared` → `FolderMachine`. `LayerTier::ORDER` fixes the order. Merging is `BTreeMap::extend` per key, so a machine file that sets one key does not discard the shared file's other keys (`apply_file`).
- `setting_override(key)` (`:864`) is consulted by `registry::get_setting` before the settings table, so a layer keeps winning over every UI write.
- **The tier predicates are the whole extension surface:**
  - `machine_scoped()` is `UserGlobalMachine | MainMachine | FolderMachine`;
  - `may_set_settings()` is every tier except the two folder tiers;
  - `may_set_main_folder()` is only `UserGlobal | UserGlobalMachine` (`:179-181`): only `~/.keeper/` may elect the main folder (AD-101);
  - `has_folder()` is every tier except the two user tiers;
  - `label()`.

### 3.2 `<host>`, and why it fails on iOS

- `read_host_label()` (`:379`) spawns `hostname`, keeps the first dot-label and trims it. An empty answer becomes `"unknown-host"`.
- `sanitize_host` maps every character outside `[A-Za-z0-9._-]` to `-`. A test pins that a hostile label cannot escape `.keeper/` (`a_hostile_host_label_cannot_escape_the_keeper_directory`).
- `hostname` does not exist in the iOS sandbox, so the phone's label is `unknown-host` and its machine file is `keeper.unknown-host.toml` (GConfig §8).
- keeper-syncd reads `/etc/hostname`/`HOSTNAME` instead (`keeper-syncd/src/platform.rs:299`).

### 3.3 Which keys a tier may set

- **Scopes:** `Scope::{UserGlobal, MachineLocal, SessionState}` and `Settable::{AnyLayer, MachineFileOnly(why), Never(why)}`.
- **`layer_may_set(key, machine_scoped)`:**
  - an unknown key is refused loudly;
  - a `MachineFileOnly` key in a shared file is refused;
  - session state is `NotAPreference`.

  Each refusal is a per-key `KeyRefused` fault, and the rest of the file still applies.
- **Generated docs.** `docs/settings-keys.md` is rendered from `GENERATED_HEADER`, whose pinned prose lists the six-file order. It is regenerated with `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored`, and a pin test fails on drift.

### 3.4 Parsing, faults, install, boot order

- **`parse_layer_file` never errors.**
  - It accepts `mainSyncFolder`/`main_sync_folder`, `settings` (dotted keys flattened) and `folder` (kept raw for keeper-sync, AD-40).
  - Anything else is `UnknownTable`: the parser tells the person where a key belongs.
  - A TOML syntax error is `Malformed` and skips the layer whole. A per-key fault skips only that key.
- **Fault kinds:** `Unreadable`, `Malformed`, `NotATable`, `ScalarExpected`, `ValueShape`, `KeyRefused`, `SettingsInNonMainFolder`, `MainFolderInFolderLayer`, `UnknownTable`, `MainFolderMissing`, `MainFolderNotADirectory` and `MainFolderNotAProfile`. Faults never block boot.
- **Install.** `config::install(layers)` fills `static LAYERS: OnceLock<AppLayers>` (`:835`). A second install is logged and ignored. Late faults go to `LATE_FAULTS` through `push_fault`, but **late overrides have no path**. The design comment argues that the resolved set "never changes, because the only later layers are per-folder and a folder may not set a settings key at all" (GConfig §9, recorded at the digest's `:770-776`).
- **Boot order** (`keeper/src/lib.rs:355-397`):
  1. `load_app_layers` + `install`, which must precede `debug_log::init`, hotkeys, the tray and `sync.git_path`;
  2. `install_folder_tier`;
  3. `registry::import_config_file`;
  4. `debug_log::init`.

  With no `HOME`, the stack is empty on purpose, and there is no fallback to a temp dir (`lib.rs:379-389`).
- **On iOS the stack always runs.** Whether UIKit sets `HOME` to the sandbox home is `[UNVERIFIED]`.

### 3.5 The folder tier is live, and is the precedent

- `FolderTier { host, main_folder }` is armed process-wide (`keeper-sync/src/profile/folder.rs:829-836`).
- `in_force` re-reads a folder's two TOML files on every profile read, "deliberately not cached here, because the file can be edited under a running app" (`folder.rs:~878-883`).
- Its `FAULTS` is a live snapshot (`:838-843`).
- No watcher exists on any layer file (grep: none). The Settings surface re-queries `config_layers` on every dialog open (GConfig §9).

### 3.6 From faults to the UI

- **The VMs.** `ConfigTierVm`, `ConfigOverrideVm` (key, tier, path, folder and a **source** phrase), `ConfigFaultVm` and `ConfigLayersVm` (`vm.rs:5136-5311`) are served by `config_layers` on every target, iOS included (`ipc.rs:1844-1869`, `lib.rs:860`).
- **Exhaustive by design.** The tier-to-VM mapping is an exhaustive match, "so adding a tier must break this file rather than silently reach the frontend" (`vm.rs:5121-5127`).
- **The badge.** `FileControlled` renders "Set by a file" with a tooltip naming the source and path. It says so but does not disable anything (`src/components/settings/config-source-section.tsx:87-102`).
- **The guard.** `src/test/file-controlled-keys.test.ts` pins that every badge names a classified key.

### 3.7 Per-device identity today

- **The sync.db `device` row.** A singleton `device` table in sync.db holds `DeviceIdentity { id: ULID, label }`. The id is minted once, from the hostname at first open. The label is renamed with `set_device_label` (`keeper-sync/src/db.rs:1626-1699`).
- **The label rides every commit:**
  - the `Keeper-Device: <label> (<id>)` trailer;
  - the author address derived from the id;
  - conflict filenames.
- **Surfaces.** `sync_device`/`sync_device_set_label` drive the `DeviceSection` rename UI (`sync-section.tsx:748-832`).
- **Absent.** No device class, `userInterfaceIdiom` or smallest-width symbol exists anywhere in the crates or `src/` (GConfig §10).

### 3.8 Where account tiers plug in

GConfig §11 lists the sites exhaustively:
- `LayerTier` gains the tiers, `ORDER` grows, and the four predicates plus `label()` are decided for each new tier;
- `parse_layer_file` needs **no change**, unless a new top-level table is introduced;
- `ConfigTierVm::of`/`phrase` break the build by design;
- the sentence-pinning test is extended;
- `GENERATED_HEADER`'s file list and `docs/settings-keys.md` are regenerated;
- no frontend component changes, because phrases are computed in Rust.

**GConfig's one real design question:** `LAYERS` is a `OnceLock`, and account layers become known only after sign-in. So either their path is known at boot, from the last clone, or the immutable-stack premise is relaxed. The folder tier (§3.5) is the in-repo precedent for a live tier.

---

## 4. The git transport as it is

All `[REPO]`, GSync unless noted.

### 4.1 How a drive is added

- **UI.** `add-folder-form.tsx` saves through `syncProfileSave` (`sync_ipc.rs:1303`). The credential is a second write, `syncSetCredential` (`add-folder-form.tsx:1579`).
- **Clone.** Cloning is engine-internal. `finish_first_checkout` calls `git::repo::clone(remote_url, local_path, branch, shallow, credential, interrupt)` (`repo.rs:587`; call at `engine.rs:8848`).
- **Empty remote.** Cloning an empty remote fails with "didn't have any ref that matched". keeper then **adopts in place**: `gix::init`, then origin plus a fetch refspec (`repo.rs:2939`; `engine.rs:8877, 8890`). A zero-ref advertisement folds into an empty `FetchOutcome`, so the first push creates the branch (`git/fetch.rs:172-201`).

### 4.2 HTTPS credentials today

- **Storage.** One token per profile, under the keychain key `sync/<profile_id>/credential` (`profile/mod.rs:1390-1392`). It is written by `sync_set_credential` (`sync_ipc.rs:1711-1724`) and read per operation (`Engine::token`, `engine.rs:9011`).
- **One spelling per consumer.** `AccessToken` dresses the token for each consumer (`keeper-sync/src/credential.rs`):
  - **git:** `Credential { username: <token>, secret: "" }` (`AccessToken::git`, re-verified at `:58-61`), "the shape Forgejo and GitHub both accept";
  - **LFS:** `Authorization: Basic base64('<token>:')`;
  - **the Forgejo REST API:** `Authorization: token <pat>`.
- **The rule of the tree:** *"A fourth consumer gets a method here or it does not get the token"* (`credential.rs:21`, re-verified).
- **Injection.** gix's `set_credentials` callback answers `Action::Get` only, so no helper cache ever holds the secret (AD-53; `fetch.rs:168-241`, `repo.rs:625-640`). No credential means `SyncError::Auth`.
- **Helpers are neutralised.** Helpers are cleared in memory (`repo.rs:115-131`; `cli.rs:1027-1031`). No token is ever in argv or in a remote URL (egress strips userinfo, `egress.rs:60-64`).

### 4.3 Bearer vs Basic

- **Basic per request:**
  - the gix callback for fetch and clone;
  - a hand-built `Authorization` in the HTTP-native paths (`push_http::basic_header`, `AccessToken::lfs_basic`).
- **No Bearer seam** exists on the gix fetch/clone path: the callback yields Basic only (`fetch.rs:226-234`).
- **Bearer on purpose.** Bearer headers would be trivial in the reqwest-native paths, but are deliberately not used there. Forgejo's LFS reserves Bearer for its own JWT, and `challenge_accepts_basic` refuses to retry against a Bearer challenge (`credential.rs:86-131`).
- **`http.extraHeader`.** GSync left gix support for it `[UNVERIFIED]`. RForgeGit answers it from source (§8.5 item 3): `http.extraHeader` maps to the transport's `Options.extra_headers` `[SOURCE: GIT-S36]`. The contract still asks SyncTransport to confirm it in the pinned fork before relying on it.

### 4.4 The forge API base: a sub-path bug

- `forge_api_target(remote_url)` (`engine.rs:36685`, re-verified) splits `https://host/<path>` at the first `/` and treats the path as exactly `owner/repo`. It returns `None` when `repo` still contains `/`.
- So `https://host/git/owner/repo.git` gives `None`. Without the guard it would give base `https://host` and owner `git`. The right Forgejo answer is base `https://host/git`, owner `owner`, repo `repo`.
- The consequence: `do_open_pr` warns and returns `RemoteContact::Skipped`, and the worktree lane's PR handoff silently degrades (`engine.rs:12189-12197`).
- The test at `:25332` pins flat forges only. LFS endpoint derivation handles sub-paths correctly (`lfs/endpoint.rs:32-40`) (GSync §4).

### 4.5 Cadence, bounds, offline

- **Cadence.** The supervisor paces each profile's scan by `poll_interval_ms` (15 s default). The notes cadence commits after 2 s idle and pushes within 30 s.
- **Bounds.**
  - `FETCH_DEADLINE` is 600 s for a whole fetch, because gix's reqwest transport sets only a 20 s connect timeout (`git/fetch.rs:39-48`).
  - The shared client connects in 15 s and gives up after 60 s of read silence (`http.rs:44-63`).
- **Offline is a first-class state.** It is sticky and cleared only by a real round trip. Local commits continue (AD-49), and it survives a restart within `OFFLINE_SEED_MAX_AGE_MS` (`engine.rs:8024-8025`, `8124-8168`, `2686-2693`).
  - "Offline is a state, not an error": `snapshot.error` stays `None`.
  - The wire string is `offline` (`sync_ipc.rs:879-887`), and the tray reads `<name> — offline, N waiting`.
  - Network classification is typed from the error chain plus a shared needle list (`fetch.rs:~250-300`).

### 4.6 iOS is gix-only, and pushes over smart HTTP

- `GitEngine::HOST = Gix` on iOS (`git/cli.rs:144-148`). keeper-sync has been a dependency on every target since Epic 66 (AD-198). `cargo check -p keeper-sync --target aarch64-apple-ios` passed on hesperia on 2026-09-05 (`crates/keeper/Cargo.toml:26-33`).
- **Push:**
  - desktop: the `git` shim (AD-41);
  - phone: `push_http::push` (AD-202), because iOS denies `posix_spawn` to third-party apps (`engine.rs:10089-10104`; `push_http.rs:1-32`). It is reqwest-native, never forces, and carries a client-side ancestor guard.

### 4.7 A hidden config repo: the direct-gix route

- **No hidden-profile concept exists.** A vault is a flag over the profile list, and every profile row appears in `sync_profiles`, `sync_statuses` and the tray (`profile/mod.rs:~230-245`).
- **Option (a), a hidden profile flag,** touches every surface and risks a leak path.
- **Option (b), direct calls to the git layer's free functions,** is recommended:
  - `git::repo::clone`/`open`/`open_for_fetch`;
  - `git::fetch::fetch`, which is **blocking** and needs `spawn_blocking`;
  - `git::commit::stage_and_commit`, which takes `&SyncProfile + &Provenance` and so needs a synthetic value or a profile-free variant;
  - `git::push_http::push`, which needs `keeper_sync::http::client()` and no git binary.

  It gives up the settle/watch cadence, the journal and AD-43's conflict policy. GSync judges that acceptable for a low-frequency config repo (GSync §10, implication 4).
- A config repo needs no LFS.

### 4.8 Licences

- **gix** is `MIT OR Apache-2.0`. It is pinned to the fork `tgorka/gitoxide` branch `keeper/gix-filter-0.33-reap` (`Cargo.toml:196-228`, allow-git in `deny.toml`).
- **git2/libgit2-sys are banned.** They declare `MIT OR Apache-2.0` while vendoring GPL-2.0-with-linking-exception C. Shelling to the git binary is invoking, not linking (GSync §11).
- **CI** runs `cargo deny check licenses bans sources` (`.github/workflows/ci.yml:97`, as ROidc §7 item 7 records it), and no `cargo audit`.

---

## 5. The surfaces as they are

All `[REPO]`, GSurface unless noted.

### 5.1 First run

- **Steps.** The wizard runs `welcome → addAccount → discovery → done` (`first-run-wizard.tsx:43-50`). It is "a path, not a gate": every step has Skip, and Esc asks once.
- **Skip persists.** The answer is `ui.first_run_setup_skipped`, a two-way `1/0` KeySpec (`registry.rs:632-643`; `keys.rs:841`; regenerated `docs/settings-keys.md:159`).
- **Reachable again.** Settings → "Run setup again" re-enters the wizard.
- **Verification method.** The house method for boot-gated UI is the real engine over tunnelled CDP. The shell's Rust does not compile on Linux (`spec-skipping-setup-can-stick.md:123-125`).

### 5.2 The Matrix account UI, and sign-out

- **The switcher** is the sidebar-footer `AccountFooter`. Each row carries avatar, hue, user id and a sync glyph (spinner "Syncing", CloudOff "Offline") (`account-footer.tsx:118-126`). The per-row menu is Settings, Beeper coverage, Incognito, DND and "Sign out…" (`:665-691`).
- **Sign-out** is an AlertDialog (`:255-333`):
  - the default is "Sign out, keep local archive";
  - the destructive "Sign out and delete archive" requires the account id typed.

  The backend teardown is local only, with no server logout, and idempotent (`account.rs:4746-4762`).
- **Adding an account** happens in `login-screen.tsx`: a Card with Tabs, sentence-case `errorCopy` per `IpcErrorCode` (`:37-53`), and "Sign in with single sign-on" (`:330-331`).

### 5.3 Settings structure and the gate rule

- **One body.** `SettingsBody` is the single definition shared by the dialog and the pane ("two copies would drift"; `settings-dialog.tsx:160-269`). It has sixteen ordered sections, with no left nav: navigation is scroll order plus section labels (DESIGN.md:211).
- **Gate rule (AD-27).** A section renders itself away where its capability is absent. There are no dead buttons and no disabled sections (`:143-155`).
- **Deliberately ungated sections** exist: `SyncGitRow`, the fix path, and `ConfigSourceSection` (`:244-301`).
- **Precedent for identity.** `DeviceSection` is "not a sync setting" and sits beside Sync (`sync-section.tsx:738-741`), a precedent for an identity section not filed under Sync.

### 5.4 QR codes are display-only

- **Producer.** `qr_svg(data)` (`keeper-core/src/bridges/login.rs:42`, re-verified) renders a quiet-zone, minimum-size SVG. A payload it cannot encode is an honest failure (`:62-63`).
- **Consumers.**
  - The bridge login Sheet: a white `size-60` card with an `img` from a data URL (`bridge-login-sheet.tsx:252-258`).
  - Device verification: the same pattern at `size-40` (`device-verification-dialog.tsx:145-150`).
  - DESIGN.md:263: "white card rounded.lg, QR ≥ 240px with quiet zone … white card mandatory in dark mode".
- **keeper cannot scan.** The only camera path is the recording webcam (`ipc.rs:4587`, `Info.plist:39`). Every QR code keeper shows is scanned by the other device.

### 5.5 Bots credentials

- **The model.** A `Provider` is one endpoint plus one credential, multi-tenant (`keeper-core/src/bots/mod.rs:106-112`).
- **Keychain keys:**
  - the provider default: `bot_provider_token/{provider_id}` (`:382-384`);
  - the per-bot override: `bot_token/{provider_id}/{bot}` (`:396-398`), which wins over the provider default (`:419-424`).
- **Form rules** (`bots-section.tsx:14-38`):
  - an inline disclosure, never a dialog;
  - the UI does not validate the base URL; Rust's sentence is rendered verbatim;
  - a token is never rendered;
  - an empty token field never clears a token;
  - a loopback or private base URL is accepted through a disclosure plus an explicit act.

### 5.6 The add-drive form and `DeviceSection`

- **The form.** One form with two mounts and two modes (`add-folder-form.tsx:1-31`). The access token is a second, separately reported keychain write, and its read-back is masked (`:2359-2384`). Remote URL and branch are at `:1768-1780`.
- **`DeviceSection`** names the machine that rides every commit's `Keeper-Device` trailer (`sync-section.tsx:748`).

### 5.7 House rules a new Account section follows

All from `ux-designs/ux-keeper-2026-07-03/`:
- **Copy.** Sentence case, no exclamation marks, no emoji in system copy, no "please" in errors, and consequences disclosed (EXPERIENCE.md:66-85).
- **Containers.**
  - Sheets for flows over an existing pane (the bridge login stepper);
  - Dialogs for pickers and confirmations;
  - AlertDialogs for destructive acts and risks;
  - inline disclosure for forms over lists.
- **States.** Persistent by default: an error is never toast-only (NFR-5), and a missing capability means a missing section (EXPERIENCE.md:126-175).
- **Errors.** A refusal is Rust's sentence rendered verbatim (`add-folder-form.tsx:31`, `bots-section.tsx:21-23`).
- **Trust and disclosure.** About's endpoint list (NFR-11) must gain any new destination an account adds (EXPERIENCE.md:213-229). AD-53 makes each configured remote a *computed* egress row (`ARCHITECTURE-SPINE.md:358`; `docs/egress.md:8-13`, computed by `egress::compute_egress`, `keeper-core/src/egress.rs:162`, over `EgressKind`, `vm.rs:1550`).
- **Dark-mode QR.** The white card is mandatory (DESIGN.md:263).

---

## 6. OIDC client mechanics

All `[SOURCE]` from ROidc unless marked. Sources in §11.1.

### 6.1 The crate

- **Recommended:** `openidconnect` **4.0.1** with `default-features = false`, plus `oauth2-reqwest` **0.1.0-alpha.3**, built on the app's existing `reqwest` 0.13 client with redirects disabled [OIDC-S1, OIDC-S5, OIDC-S7].
  - `openidconnect` is MIT, released 2025-07-06, MSRV 1.65. Commits after it are unreleased [OIDC-S1, OIDC-S11].
  - `oauth2` 5.0.0 is `MIT OR Apache-2.0` [OIDC-S2].
  - `oauth2-reqwest` 0.1.0-alpha.3 is MIT (2026-02-22) and depends on `oauth2 ^5` and `reqwest ^0.13`, both without default features [OIDC-S5].
- **The default features must stay off.** `oauth2` 5.0.0's built-in reqwest support is pinned to `reqwest ^0.12` [OIDC-S2, OIDC-S8].
  - With default features, `openidconnect` resolves to 154 crate versions: 83 are reqwest 0.12's own tree and 71 are added on top [OIDC-M1].
  - The maintainer ships reqwest 0.13 support as the separate `oauth2-reqwest` crate. Issue #333 is open, and no stable release had shipped by 2026-05-19 [OIDC-S9, OIDC-S10].
- **keeper's configuration adds 23 crate names** against `src-tauri/Cargo.lock`. All are RustCrypto and pure Rust, licensed MIT, `Apache-2.0 OR MIT` or `MIT/Apache-2.0`, all on `deny.toml`'s allow list. It adds no second reqwest and no second TLS stack [OIDC-M3]. Default features would also bring rustls with bundled `webpki-roots` [OIDC-M1]. `[INFERENCE]` That is a second reason to disable them where VPN-private CAs matter.
- **What the library provides:**
  - PKCE (`new_random_sha256`: 32 random bytes, S256; `plain` is opt-in) [OIDC-S8];
  - discovery that rejects a document whose `issuer` differs from the requested one, as Discovery §4.3 requires [OIDC-S6, OIDC-S16];
  - JWKS fetched at discovery time only, with no refetch on an unknown `kid` found [OIDC-S6] `[INFERENCE from source]`;
  - revocation (`revoke_token` after `set_revocation_url`, typestate-gated) [OIDC-S6, OIDC-M2];
  - `LogoutRequest` [OIDC-S6].
- **Its verifier defaults** [OIDC-S6]:
  - allowed algorithms `{RS256}`;
  - `aud` must contain the `client_id`, and every other audience is rejected;
  - `iss` and the signature are checked;
  - JWE, `cty` and `crit` are rejected;
  - HS* is rejected for public clients;
  - **ES512 is `UnsupportedAlg`**.
- **What it does not check** [OIDC-S6, OIDC-S7]:
  - `azp` (deliberately);
  - `iat` (any value is accepted);
  - `exp` has **no leeway**;
  - `at_hash` (a manual `AccessTokenHash::from_token` is shown instead);
  - `acr`/`auth_time`.
- **Security note in its docs:** turn off HTTP redirects to prevent SSRF [OIDC-S7].
- **Measured.** A scratch crate compiled the whole flow [OIDC-M2]: discovery with custom metadata, PKCE, nonce, Zitadel scopes and the roles claim, `set_allowed_algs`, `set_other_audience_verifier_fn`, refresh, revocation and the logout URL. **It was never run against a live IdP** (ROidc §10).
- **Rejected alternatives:**
  - `openid` 0.24: native-tls by default, and one PKCE pair for the client's whole lifetime unless `refresh_pkce` is called, which conflicts with RFC 9700's per-transaction PKCE [OIDC-S3, OIDC-S12, OIDC-S19];
  - hand-rolling on `jsonwebtoken` 11: every item in §6.2 would become keeper's own code, with no discovery and no nonce handling [OIDC-S13].
- **Advisory.** `rsa` 0.9.10 carries RUSTSEC-2023-0071 (Marvin, CVSS 5.9), unpatched as of 2026-09-12 [OIDC-S14]. It concerns private-key material. `[INFERENCE]` A verify-only client holds no IdP private key. keeper's CI does not run `cargo audit` (§4.8), so an explicit ignore with that reason is the honest record.

### 6.2 The validation checklist for a public native client

ROidc §2, reproduced because the contract names it as keeper's checklist. `LIB` means `openidconnect` 4.0.1 does it when configured; `YOU` means keeper's code must.

**A. Setup, once per issuer**
1. The issuer is an exact string, trailing slash included. Configure it with `client_id` and the extra trusted audiences.
2. Discovery runs over TLS with redirects off. The document's `issuer` must equal the configured one byte for byte (**LIB**) [OIDC-S6, OIDC-S16]. `end_session_endpoint` and `revocation_endpoint` need a custom metadata type (**YOU**).
3. Allowed algorithms are `id_token_signing_alg_values_supported` ∩ {RS256/384/512, PS256/384/512, ES256, ES384, EdDSA}, passed to `set_allowed_algs`. HS* is refused (**LIB**), and ES512 cannot be verified [OIDC-S6].
4. Fail setup early if JWKS has no asymmetric key. An authentik provider with no signing key signs HS256 with the client secret [OIDC-S42] (**YOU**).

**B. Each attempt**
5. Fresh random `state`, `nonce` and a PKCE S256 verifier, never reused [OIDC-S19 §2.1.1].
6. Store them with the exact redirect URI and the issuer [OIDC-S18 §8.10].
7. Use an external user agent, never the app's webview [OIDC-S18 §8.12].
8. Redirect: a loopback `127.0.0.1` with an ephemeral port (not `localhost`), a reverse-DNS private-use scheme, or a claimed `https` URL [OIDC-S18 §7.1–7.3, §8.3].

**C. Authorization response (all YOU)**
9. The response must arrive on the stored redirect URI.
10. `state` must equal the stored value; reject otherwise [OIDC-S7]. Handle `error`. An RFC 9207 `iss` parameter, when present, must equal the issuer [OIDC-S19].
11. The attempt is consumed exactly once.

**D. Token response**
12. Exchange with the `code_verifier`, and require an `id_token`.
13. ID-token checks, OIDC Core §3.1.3.7 [OIDC-S15]:
    - a. no JWE/`cty`/`crit` (LIB);
    - b. `iss` (LIB);
    - c. `aud` contains the `client_id`, and every other `aud` is explicitly trusted: LIB, with YOU supplying `set_other_audience_verifier_fn`. Zitadel always adds its project id (§6.4);
    - d. `azp` equals the `client_id` when present (YOU);
    - e. `alg` is allowed (LIB);
    - f. the signature verifies by `kid`; on `NoMatchingKey`, refetch JWKS once and retry once (YOU). Zitadel publishes keys before activating them [OIDC-S36];
    - g. `exp` with a leeway you choose (YOU);
    - h. `iat` within a window (YOU);
    - i. `nonce` (LIB);
    - j. `auth_time`/`acr` if requested (YOU);
    - k. **key the account on (`iss`, `sub`)**, the only stable identifier [OIDC-S15 §5.7].
14. `at_hash` when present (YOU). The spec says MAY; it matters here because the access token is reused as a credential elsewhere [OIDC-S7, OIDC-S15 §3.1.3.8].
15. Treat the access token as **opaque**. Never parse it for roles [OIDC-S21 §6].

**E. UserInfo**
16. UserInfo `sub` must equal the ID token's `sub`, otherwise discard it (LIB when the expected subject is passed) [OIDC-S15 §5.3.2].

**F. Refresh**
17. Replace the refresh token when a new one arrives, and **persist it before using the new access token** (YOU) [OIDC-S23 §6].
18. A new ID token must keep `iss`, `sub` and `aud` and the original `auth_time`. `nonce` should be absent, and if present must equal the original (authentik copies it [OIDC-S44]). Use a closure verifier, not `&Nonce` [OIDC-S15 §12.2].

**G. Sign-out**
19. Revoke the refresh token. RFC 7009 returns 200 even for an invalid token [OIDC-S20].
20. Send `end_session` with `id_token_hint`, `client_id`, a registered `post_logout_redirect_uri` and `state`, and check the returned `state` [OIDC-S17].
21. Delete the local tokens whatever the outcome (YOU).

### 6.3 RP-initiated logout

- **The spec.** OpenID RP-Initiated Logout 1.0, final, 2022-09-12 [OIDC-S17]:
  - `id_token_hint` is RECOMMENDED;
  - if `client_id` and the hint are both sent, the OP must check they match;
  - `post_logout_redirect_uri` must be pre-registered and match exactly. `http` is allowed only for confidential clients; an alternate scheme identifies a native callback;
  - the OP should accept an expired hint, must confirm with the user when there is no valid hint, and treats the request as idempotent;
  - `end_session_endpoint` appears in discovery and is `https`.
- **Per IdP:**
  - AppAuth presents it through the external user agent [OIDC-S54, OIDC-S55];
  - Keycloak needs `client_id` or `id_token_hint` alongside the redirect, and **offline tokens survive a user logout** [OIDC-S39];
  - Zitadel's endpoint is `/oidc/v1/end_session`, and revoking a refresh token also revokes its access token [OIDC-S30];
  - authentik's end-session is per application, and its revocation endpoint is global [OIDC-S42].
- `[INFERENCE]` **Revoke first**, because `end_session` alone does not kill Keycloak offline tokens.
- `[INFERENCE]` **Call `end_session` in the same user agent** the sign-in used. A non-ephemeral session shares cookies, so the OP's SSO session survives otherwise.
- `[INFERENCE]` **A public client's post-logout redirect** should be a private-use scheme or claimed `https`, not `http` loopback.

### 6.4 Where roles and groups appear

**Zitadel**
1. **The role scopes.** `urn:zitadel:iam:org:project:role:{key}` asserts `…:project:roles`. `urn:zitadel:iam:org:projects:roles` asserts `urn:zitadel:iam:org:project:{projectid}:roles` for every project in the audience, as requested with `…:project:id:{projectid}:aud` [OIDC-S25, OIDC-S28].
2. **Correction to the brief.** The source emits **both** the generic and the per-project claim, and Zitadel recommends the per-project form [OIDC-S27, OIDC-S29].
3. **Shape.** An **object** whose keys are role keys, with values mapping `orgId → primaryDomain` (source type `map[string]map[string]string`) [OIDC-S27, OIDC-S28]. The claims reference page shows an array, which conflicts with the source and the sample [OIDC-S26 vs S27/S28].
4. **Placement.** Roles reach the ID token "when requested **or configured**" [OIDC-S26]. Profile claims reach the ID token in the code flow only with "User Info inside ID Token". The admin toggles are "Assert Roles on Authentication" and "User Roles Inside ID Token" [OIDC-S27].
5. **`groups`.** A `groups` claim exists on `main` since 2025-11-11 but is **not in v4.19.0**, the latest release (2026-09-23): unreleased [OIDC-S29, OIDC-S59].
6. **Audience.** The ID token's audience holds every client id of the project **plus the project id** by default. The issue asking to change this stays open, and the behaviour is intentional [OIDC-S26, OIDC-S37]. `openidconnect`-based clients fail with "`<project-id>` is not a trusted audience" [OIDC-S38].

**Keycloak (26.7.4)**
7. **Roles.** They land in `realm_access.roles` and `resource_access.<client>.roles`, **in the access token only by default**. The ID token and UserInfo get them only if the mapper is switched on [OIDC-S39].
8. **Groups.** Not emitted unless a Group Membership mapper is added. Its `full.path` defaults to `true` (`/top/level1`). The `microprofile-jwt` scope puts *realm roles* into `groups` [OIDC-S39, OIDC-S40].

**authentik**
9. **Groups.** The `profile` scope returns a flat `groups` list [OIDC-S42, OIDC-S43].
10. **Signing.** Without a signing key it signs HS256 with the client secret, so a public client cannot verify it [OIDC-S42].
11. **Issuer.** Per application, **with a trailing slash** [OIDC-S42].

**Authelia**
12. The `groups` scope is delivered in UserInfo by default [OIDC-S45].

**Across IdPs**
13. Clients must not inspect access-token contents [OIDC-S21 §6]. Read roles from the verified ID token or from sub-checked UserInfo.

`[INFERENCE]` A descriptor therefore has to name the claim (`roles_claim`). The claim may be an array of strings (authentik, Authelia, Keycloak `groups`) or an object whose keys are roles (Zitadel). It may also be nested (`realm_access.roles`). This is the shape pinned by the contract's `claims::roles`.

### 6.5 The access token as a credential for other services

- **Per IdP:**
  - Zitadel's `…:project:id:{projectid}:aud` adds that project to the access token's audience, and a backend validates it by introspection [OIDC-S25, OIDC-S27]. Zitadel access tokens are **opaque by default**; JWT is a per-app switch [OIDC-S31, OIDC-S32].
  - Keycloak issues JWTs, with audience from mappers [OIDC-S39].
  - Authelia's tokens are opaque unless configured, and it warns they must not prove identity [OIDC-S46].
- **The standards:**
  - RFC 9068 JWT access tokens require `typ` `at+jwt`, and the resource server must check that `aud` names itself [OIDC-S21];
  - RFC 8707 `resource` is the standard audience request [OIDC-S22];
  - RFC 9700 §2.3: access tokens SHOULD be restricted to one resource server or a small set [OIDC-S19].
- `[INFERENCE]` Every service that accepts keeper's token must verify a JWT whose `aud` includes itself, or introspect an opaque token with its own credentials. That choice belongs to the IdP admin and the service, so the descriptor must declare the audiences and scopes to request (`extra_scopes`, `trusted_audiences`).
- `[INFERENCE]` **One token carrying N audiences can be replayed at any of the N.** Keep N small, or exchange tokens per service. Zitadel and authentik 2026.8 support token exchange [OIDC-S30, OIDC-S42].

### 6.6 `offline_access`, rotation, storage

- **The rules.**
  - OIDC Core §11: `offline_access` asks for a refresh token, and native clients SHOULD obtain consent [OIDC-S15].
  - RFC 9700: public clients' refresh tokens must be sender-constrained or rotated [OIDC-S19].
  - RFC 6749 §6: the client discards the old refresh token [OIDC-S23].
- **Per IdP:**
  - **Zitadel:** strict rotation, rejecting any token that is not the session's current one. Defaults: access token 12 h, refresh idle **30 days**, absolute **90 days** [OIDC-S30, OIDC-S34, OIDC-S35].
  - **Keycloak:** rotation can be suppressed per client. An `offline_access` token is not bound to SSO timeouts and survives logout [OIDC-S39, OIDC-S41].
  - **authentik:** `offline_access` is required since 2024.2, it always rotates, and reuse is rejected and logged `SUSPICIOUS_REQUEST` [OIDC-S42, OIDC-S44].
  - **Authelia:** refresh only with `offline_access`; its rotation behaviour is `[UNVERIFIED]` [OIDC-S45, OIDC-S47].
- **Storage.**
  - `AfterFirstUnlockThisDeviceOnly` works in the background after first unlock and never migrates to a new device or a backup restore [OIDC-S49].
  - `kSecUseDataProtectionKeychain` gives iOS-style items on macOS without iCloud; `kSecAttrSynchronizable` syncs [OIDC-S50, OIDC-S51].
  - The `keyring` 3.6.3 crate uses the legacy login keychain on macOS [OIDC-S57]. keeper already pins `AfterFirstUnlockThisDeviceOnly` on iOS (`src-tauri/Cargo.toml:97-104`, `[REPO]`).
  - Android's security-crypto is deprecated in favour of the Keystore [OIDC-S56].
- `[INFERENCE]` **Rotation is effectively universal**, so exactly one component per device may refresh, serialised. A lost race, or a crash between the server rotating and the keychain write, loses the session. Surface that as *sign in again*, not as *unreachable*.
- `[INFERENCE]` **Refresh tokens are per device**, `ThisDeviceOnly`, and never in the synced repository.
- `[INFERENCE]` **Offline keeps the last config.** A Zitadel device idle for more than 30 days, or holding a grant older than 90 days, will need to sign in again even online.

---

## 7. Native sign-in UI

All `[SOURCE]` from ROAuthUI unless marked. Sources in §11.2.

### 7.1 ASWebAuthenticationSession

- **F1.1 Availability.** iOS 12+ and macOS 10.15+. On iOS it is a secure embedded web view. On macOS it opens the default browser if that browser supports web-auth sessions, else Safari. Only the calling app's session receives the callback, even when several apps register the scheme [UI-S1].
- **F1.2 Browser support.** A macOS browser declares `…IsSupported` to take part, and without one the system falls back to Safari [UI-S9]. Chrome and Edge declare it, from about Chrome 91 [UI-S36, a community source].
- **F1.4 Safari on iOS.** On iOS the session always uses Safari [UI-S14].
- **F1.5 `prefersEphemeralWebBrowserSession`.**
  - Available on iOS 13+ and macOS 10.15+, defaulting to `false`.
  - `true` asks the browser not to share cookies. Safari honours that, and another macOS default browser "might or might not".
  - It must be set before `start()` [UI-S2].
  - Non-ephemeral makes all cookies except session cookies available [UI-S3].
- **F1.7–F1.8 The consent alert.** Ephemeral mode exists so users are not left signed in to the IdP, and it shows no system consent alert. Non-ephemeral shows an alert to share existing login information, and cancelling it is `CanceledLogin` [UI-S11, UI-S12].
- **F1.9–F1.11 Presentation and lifecycle.**
  - A presentation context provider must be set before `start`, and it is **weak**, so the app retains it [UI-S7, UI-S11, UI-S12].
  - The session holds itself until completion.
  - `start()` returns `Bool`, runs once, and fails on a cancelled session [UI-S8, UI-S3].
- **F1.12–F1.15 Callbacks.**
  - `Callback` (`.customScheme` / `.https(host:path:)`) needs iOS 17.4+ and macOS 14.4+. The scheme initializer is deprecated [UI-S4, UI-S1].
  - `.https` requires an associated domain [UI-S5], in practice `webcredentials:` and a paid developer account [UI-S35, vendor docs].
  - **For a custom-scheme callback the app registers the scheme in Info.plist *or* passes it as `callbackURLScheme`, so `CFBundleURLTypes` is not required for the OAuth redirect** [UI-S12].
- **F1.17 Error codes.** `canceledLogin = 1`, `presentationContextNotProvided = 2`, `presentationContextInvalid = 3` [UI-S6, UI-S12].
- **F1.18–F1.19 Shared login (Okta, iOS).** Session cookies are shared between sessions (not with Safari), persistent cookies with Safari too, and nothing is shared when ephemeral. Native SSO via `device_secret` is Okta's better route to a second client [UI-S14, UI-S15].
- `[INFERENCE]` **Two browser legs.** A second non-ephemeral leg, such as the forge in `oauth` mode, can reuse the IdP login without a password, but shows the system alert again. So "sign in, then connect the forge" costs two round trips and two alerts (I1.1).
- `[INFERENCE]` **The macOS cookie jar** is the default browser's (I1.2).
- `[INFERENCE]` **The HTTPS callback** is unusable for per-person or self-hosted IdPs. A custom scheme is the universal option (I1.3).
- `[INFERENCE]` **Threading and errors.** Run `start`/`cancel` on the main thread with a retained provider. Map code 1 to Cancelled, and treat 2 and 3 as programming errors (I1.4).

### 7.2 Passkeys and 1Password inside the session

- **Web platform features.**
  - iOS: "All Web Platform features that are available in Safari, including WebAuthn, are available" [UI-S16].
  - macOS: the default browser is invoked with its WebAuthn [UI-S17].
- **1Password.**
  - On iOS it is a system AutoFill provider for passwords and passkeys [UI-S18].
  - On macOS its native AutoFill was in public beta from 2026-05-26 (8.12.22+, macOS 14+, Apple silicon) and is generally available in 8.12.32+ [UI-S19, UI-S20].
  - On Android it needs Android 14+ for passkeys, and in Chrome 141+ the user must enable "Autofill using another service" [UI-S21].
- `[INFERENCE]` 1Password passkeys should be offered in the iOS sheet and in Safari-handled macOS sessions. No source tests the exact combination (I2.1, I2.2).
- **The IdP page does the passkey work,** provided keeper uses no embedded WKWebView. An embedded view restricts passkeys to the app's own RP ID [UI-S16, UI-S17].

### 7.3 Android: Auth Tab and Custom Tabs

- **Auth Tab.** `AuthTabIntent` returns the redirect through an activity result and needs no intent-filter for itself. From Chrome 137 it falls back to a Custom Tab automatically [UI-S22].
  - Redirects: a custom scheme, or HTTPS with Digital Asset Links.
  - The launcher should be created before the activity [UI-S24].
- **Releases.** Auth Tab is stable in `androidx.browser` 1.9.0. 1.10.0 made `AuthenticateUserResultContract` public [UI-S25, UI-S26].
- **Ephemeral browsing** is available on both intent types (Chrome 136+) [UI-S23].
- **Cancellation.** Plain Custom Tabs do not report it. AppAuth uses separate `PendingIntent`s [UI-S37], and tauri-plugin-auth-session uses a bridge activity [UI-S30].
- `[INFERENCE, needs a spike]` **The Tauri route.** A Tauri Kotlin plugin is constructed after the Activity exists. The workable route is to launch `authTabIntent.intent` through Tauri's `startActivityForResult` and parse the result with the now-public contract (I3.1).

### 7.4 Plugins and crates

| Crate / repo | Verdict, with its evidence |
| --- | --- |
| tauri-plugin-auth-session 0.2.2 | macOS + iOS through pure-Rust objc2, plus Android Custom Tabs. **Its only interface is a webview command.** It uses the deprecated initializer, panics without a window, has one maintainer, and has an open iOS link bug (`_ASWebAuthenticationSessionErrorDomain` without the `AuthenticationServices` framework) [UI-S28, UI-S30, UI-S31]. |
| tauri-plugin-web-auth 1.0.0 | iOS and Android only, with **no cancel handling** on Android. Unmaintained since 2025-04 [UI-S29]. |
| tauri-plugin-plauth 1.0.4 | Its README contradicts itself [UI-S32]. |
| tauri-plugin-oauth 2.1.0 | A desktop loopback server on `127.0.0.1`, `MIT OR Apache-2.0`, 409k downloads [UI-S33]. |
| tauri-plugin-oauth-session 0.1.0, tauri-plugin-appauth 0.2.0 | No repository, or a 404: cannot be audited [UI-S28]. |
| **objc2-authentication-services 0.3.2** | `Zlib OR Apache-2.0 OR MIT`, part of madsmtm/objc2, 776k downloads. Exposes the `Callback` initializer, the deprecated one, the provider, ephemeral, `start`, `cancel` and `additionalHeaderFields`. `objc2::available!(ios = 17.4, macos = 14.4)` is a runtime gate [UI-S12, UI-S13, UI-S28]. |
| official plugins-workspace v2 | No web-auth or auth-session plugin [UI-S34]. |

### 7.5 RFC 8252

- **User agent and PKCE.** Native apps MUST use an external user agent, and in-app browser tabs are RECOMMENDED (§5–§6). Public clients MUST use PKCE (§6, §8.1) [UI-S27].
- **Redirect options.** Authorization servers MUST offer three (§7):
  - **a private-use scheme,** which MUST be reverse-DNS: `com.example.app:/…`, where "myapp" does not qualify (§7.1). ASes SHOULD reject a scheme with no period (§8.4);
  - **claimed HTTPS,** which SHOULD be used where possible (§7.2);
  - **loopback** `http://127.0.0.1:{port}/…`, never `localhost`, with any port allowed. Open the port only for the request (§7.3, §8.3). macOS: a scheme is "a good choice" and loopback is viable. Linux: loopback, without `SO_REUSEADDR` (App. B).
- **Matching and secrets.** Exact redirect matching is required except for the loopback port (§8.4). An app's static secret is not confidential (§8.5) [UI-S27].
- `[INFERENCE]` `keeper:` has no period, so a strict AS may reject it as an OAuth redirect (I5.1).
- `[INFERENCE]` Keep the OAuth redirect separate from the setup deep link (I5.2).
- `[INFERENCE]` Keeping the PKCE verifier in Rust is cleaner (I5.3).

### 7.6 The per-platform recommendation

- **macOS and iOS: one Rust module in the shell crate, about 150–200 lines, and no plugin dependency.** Crates: `objc2`, `objc2-authentication-services` (features `ASWebAuthenticationSession`, `ASWebAuthenticationSessionCallback`, `block2`), `objc2-foundation`, `block2` and the window crates. Model it on tauri-plugin-auth-session's `apple.rs`, with five fixes:
  1. `Callback::customScheme` behind `available!(macos = 14.4, ios = 17.4)`, falling back to the deprecated initializer otherwise;
  2. anchor on keeper's own main window, and error rather than panic without one;
  3. retain the session, provider and block together, and expose `cancel`;
  4. map code 1 to Cancelled, and codes 2 and 3 to Internal;
  5. leave `prefersEphemeralWebBrowserSession = false`.

  Expose a Rust async function so `keeper-core` owns PKCE, state and the exchange [UI-S12, UI-S13, UI-S30]. iOS must add `bundle.iOS.frameworks: ["AuthenticationServices"]` [UI-S31].
- **The fallback** for an IdP that refuses custom schemes: loopback on `127.0.0.1`. That is tauri-plugin-oauth, or about 40 lines of `std::net::TcpListener` [UI-S33] `[INFERENCE]`.
- **Android, when a target exists:** about 100 lines of Kotlin. Auth Tab (`androidx.browser` ≥ 1.9.0), plus a Custom Tabs bridge-activity fallback. Do not depend on tauri-plugin-web-auth [UI-S22, UI-S29, UI-S30].

---

## 8. Git over HTTPS with OAuth tokens

All `[SOURCE]` from RForgeGit unless marked. Sources in §11.3.

### 8.1 The per-forge answer

| Forge | The OAuth access token at git over HTTPS | PKCE / public client |
| --- | --- | --- |
| **Gitea** (paths from 1.21; granular scopes from 1.23; scope enforced on the Basic path from 1.26) | Basic with any username and the token as password (the username is ignored). Basic with the token as username and an empty or `x-oauth-basic` password. `Authorization: Bearer` on the git smart-HTTP routes, while LFS on `main` accepts only Basic [GIT-S3, GIT-S4, GIT-S5]. | Yes. Untick "Confidential client". S256 and `plain`; **PKCE required for public clients**; a loopback redirect may use any port; everything else matches exactly [GIT-S1, GIT-S6, GIT-S7]. |
| **Forgejo** (v15, v16; v16.0.0 on 2026-07-16) | The same three forms. LFS also accepts OAuth2. **v16 "Authorized Integrations"** accept an external IdP's JWT as Bearer or as the Basic password [GIT-S17, GIT-S18]. | Yes, the same rules. **A public client sees consent on every authorize request** [GIT-S2, GIT-S17]. |
| **GitLab** | Basic with username `oauth2` ("any string") and the token as password, with `read_repository`/`write_repository` [GIT-S21]. No documented Bearer on git HTTP. | Yes: `confidential=false` and S256 [GIT-S21, GIT-S22]. |
| **GitHub** | An installation token as `x-access-token:TOKEN` [GIT-S24]. `gho_`/`ghu_` user tokens as the password in practice [GIT-S25, GIT-S29]. | PKCE S256 since 2025-07-14, but **no public client**: `client_secret` is still required on the web flow [GIT-S25, GIT-S26]. |

### 8.2 Gitea and Forgejo as the OAuth provider for git

1. **Apps.** An app has a name, redirect URIs, and a "Confidential client" flag that defaults to TRUE [GIT-S1, GIT-S7:50].
2. **Registration.** A user, an org admin or the instance admin can register one. Forgejo's instance-wide apps live at `/admin/applications` [GIT-S1, GIT-S2].
3. **Redirect URIs.** No scheme validation of redirect URIs was found [GIT-S12b] `[INFERENCE: keeper://… is accepted]`. Matching is exact after lower-casing and trimming a trailing `/`, except that a public client's `http` loopback may use any port [GIT-S7:155-195].
4. **Grants.**
   - Only the auth-code grant exists (with PKCE and OIDC). There is no device grant [GIT-S1, GIT-S2, GIT-S20].
   - Live discovery on gitea.com and codeberg.org advertises `["plain","S256"]` [GIT-S20].
5. **Token shape.**
   - The access token is a forge-signed JWT carrying the grant id, valid 3600 s; the refresh token lasts 730 h; RS256 by default [GIT-S14, GIT-S8].
   - Revoking the grant kills every token issued under it [GIT-S4].
6. **One grant per (user, app)** [GIT-S17 `models/auth/oauth2.go:473-475`].
   - With `INVALIDATE_REFRESH_TOKENS` on, every issuance increments the grant's counter, and a stale refresh token is rejected with "token was already used".
   - **The default is `false` on Gitea and `true` on Forgejo v16** [GIT-S14, GIT-S16, GIT-S17 `setting/oauth2.go:109`].
7. **Git routes.**
   - Gitea `main` registers `AllowBasic`, `AllowOAuth2` and `AllowDeployToken` on git smart-HTTP [GIT-S5:1773].
   - Forgejo's `buildGitAuthGroup` is OAuth2, AccessToken (Basic + Bearer), the Action tokens, AuthorizedIntegration (PermitBasic), an optional ReverseProxy, and Basic. Its code comment calls this "inherited from GitHub's OAuth Git over HTTPS behaviour" [GIT-S17].
8. **Scopes (Gitea ≥ 1.23).**
   - They use PAT names (`read:repository`, `write:repository`, `read:user`, …). **OIDC-only scopes, and unknown scopes, mean full access** [GIT-S1, GIT-S8].
   - Git needs repository read (fetch) or write (push) [GIT-S9, GIT-S10].
   - In 1.23–1.25 the Basic path discarded the scope. From 1.26 it is enforced [GIT-S13].
   - Forgejo's docs say scopes are "not yet implemented", yet v15/v16 code maps them: **code-present but undocumented** [GIT-S2, GIT-S17].
9. **A changed scope string breaks an existing grant.** The comparison is a plain string: `server_error` "a grant exists with different scope". For confidential or skip-consent apps an existing grant short-circuits consent, and the new scope is **silently ignored** [GIT-S1, GIT-S6:434-440, GIT-S17 `oauth.go:635`]. `[INFERENCE]` The only fix is for the user to revoke and re-authorize.
10. **Consent.** Gitea always shows consent to untrusted public clients, and skips it for confidential or `SkipSecondaryAuthorization` apps with a grant. Forgejo skips it only for confidential clients [GIT-S6, GIT-S17].
11. **External IdP login.**
    - `/user/oauth2/{provider}?redirect_to=` stores `redirect_to` in a cookie and consumes it same-site only [GIT-S5:773-776, GIT-S11, GIT-S17].
    - First-time users land on the link-account page unless `ENABLE_AUTO_REGISTRATION` is on [GIT-S14].
    - Forgejo dev honours `prompt=none` for a silent upstream sign-in; whether that ships in v16.0 is `[UNVERIFIED]`.
12. **`/api/v1/user`** returns `id`, `login` and `email`. It needs `read:user` [GIT-S12].
13. **Detection signal.** Both forges answer a 401 with `WWW-Authenticate: Basic realm="Gitea"`, so **the realm cannot tell Gitea and Forgejo apart** [GIT-S9, GIT-S17].
14. **A field bug.** "First push fails, second succeeds" is a credential helper erasing an expired token [GIT-S40].

### 8.3 GitLab and GitHub

- **GitLab.**
  - Git over HTTPS takes a token with `read_repository`/`write_repository` as the password, username `oauth2`. The API takes Bearer [GIT-S21].
  - `confidential` defaults to `true`, and PKCE is S256 without a secret.
  - A refresh rotates both tokens, and instance apps can be trusted to skip consent [GIT-S21, GIT-S22].
- **GitHub.**
  - The web flow requires `client_secret` [GIT-S25].
  - Expiring 8 h tokens with 6-month refresh tokens are the default for new OAuth apps since 2026-08-14 [GIT-S25, GIT-S27].
  - The API lives at `api.github.com`, or at `/api/v3` on GHES [GIT-S28].
- **git-credential-oauth**, a cross-forge reference: PKCE S256 everywhere, `username=oauth2` with the token as password by default, and Bearer only when git announces `authtype` for a known host. It **assumes the forge is at the domain root** for unknown hosts [GIT-S29].
- **git's own Bearer routes.** git 2.46 added `authtype`/`credential` to the credential protocol. The older route is `http.extraHeader` [GIT-S30, GIT-S36].

### 8.4 Presenting an external IdP's token to git ("same" mode)

- **Gitea** verifies only its own tokens. A trusted reverse proxy can authenticate instead: `ENABLE_REVERSE_PROXY_AUTHENTICATION` with `X-WEBAUTH-USER`, trusted only from loopback by default [GIT-S3, GIT-S4, GIT-S5, GIT-S14].
- **oauth2-proxy** as that proxy: `--skip-jwt-bearer-tokens` accepts an IdP JWT whose `aud` matches. It reads the JWT from Bearer, or from **Basic, as the password with any user, or as the username with an empty or `x-oauth-basic` password**, and forwards `X-Forwarded-Preferred-Username` [GIT-S34]. `[INFERENCE]` So "the IdP token as the git password" works today in front of Gitea or Forgejo.
- **Forgejo v16 Authorized Integrations** accept an external JWT on API, packages and **git**, as Bearer or the Basic password [GIT-S17, GIT-S18]. The constraints:
  - it is configured **per user**;
  - `iss` must be `https`, with discovery and JWKS on the same host;
  - **`aud` must contain exactly one value**, the Forgejo-generated one;
  - claim rules can pin `sub`;
  - private issuers need `AllowLocalNetworks`.

  The documented use cases are CI workloads.
- **Not in these products:**
  - **Gerrit** implements "git over OAuth" only for SAP IAS in the common plugin [GIT-S32, GIT-S33];
  - **Soft Serve** accepts only its own JWTs [GIT-S31];
  - **GitLab** has no documented way.

### 8.5 gitoxide and git2

1. **gix cannot push.** `push` is unchecked in gix, gix-protocol and gix-transport. Its HTTP backends (curl or reqwest) are blocking only [GIT-S35].
2. **Credentials.**
   - `Connection::with_credentials`/`set_credentials` take a closure. Without it, gix runs the configured helpers [GIT-S37].
   - Clone configures the connection through `configure_connection` and `with_in_memory_config_overrides` [GIT-S37].
   - **The identity `Account { username, password, oauth_refresh_token }` is only ever sent as `Authorization: Basic`.** gix-credentials has no `authtype` [GIT-S36, GIT-S37].
   - Credentials are refused over plain `http://` unless a feature is on [GIT-S36].
3. **Bearer is possible only through `http.extraHeader`,** which maps to `Options.extra_headers`, or through the reqwest backend's `configure_request` hook [GIT-S36].
4. **iOS.** A 2022 user report built gix for `aarch64-apple-ios` with the reqwest backend [GIT-S38]. There is no evidence of official iOS CI: `[UNVERIFIED]`. `[INFERENCE]` reqwest + rustls is the iOS-friendly choice, and `with_credentials` is mandatory there.
5. **git2/libgit2.** It can push, and has `custom_headers` for Bearer [GIT-S39]. Its `openssl-sys` gating drags into iOS `[INFERENCE]`, and an iOS link failure was closed with "Migrate to gix" [GIT-S39 #1185].

`[REPO]` keeper already answers item 1 on the phone with its own smart-HTTP push, `push_http` (§4.6). RForgeGit's implication 8 ("a synced config repo needs git2 … or an HTTP-API write path") was written without that fact (§9.2 T2).

### 8.6 Forges under a sub-path

- **The facts.**
  - Gitea at a sub-path uses `ROOT_URL = https://h/gitea/`, "not recommended". The OIDC `iss` is `AppURL` without the trailing `/` [GIT-S15, GIT-S8].
  - GitLab supports a relative URL (beta) and nests subgroups up to 20 levels [GIT-S23].
  - GitHub's API lives on another host, or at `/api/v3` [GIT-S28].
  - git-credential-oauth assumes the domain root [GIT-S29].
- `[INFERENCE, from these]` **Why derivation fails.**
  - `https://h/a/b/c.git` is ambiguous. It could be Gitea at `/a/` with repo `b/c`, or GitLab at the root with project `a/b/c`, or GitLab at `/a` with `b/c`.
  - The prefix depth depends on the forge type and the admin's `ROOT_URL`, and neither is in the URL. The realm header is identical for Gitea and Forgejo (§8.2 item 13).
  - The API may live on another host.
  - Behind VPNs the host a person types may not be `ROOT_URL`.
  - Probing is only a heuristic.

  **The descriptor must carry the API base (or the forge's issuer) explicitly.**

---

## 9. Where the evidence lands

### 9.1 Per decision

`[INFERENCE]` over §2–§8 and the contract.

- **AD-308 (the descriptor store).**
  - The parser refuses unknown tables (§3.4), and a descriptor inside the stack could be rewritten by `<login>/keeper.toml`, which is what the stack reads from the repository (§3.8). A separate file answers both.
  - The iOS path is the app data dir, because `HOME` on iOS is unverified (§3.4).
  - The descriptor carries the issuer as an exact string (§6.2 A1) and the API base explicitly (§8.6).
  - A client secret is refused, because RFC 8252 §8.5 says a static secret in an app is not confidential (§7.5).
- **AD-309 (the account tiers).**
  - The predicates are the whole policy surface (§3.1, §3.8). Account tiers take `may_set_settings = true`, `may_set_main_folder = false` (AD-101 held), `has_folder = false`, and `machine_scoped` for the device file only.
  - "Live" copies the folder tier's precedent (§3.5) rather than relaxing `OnceLock` for every tier: an `RwLock` is consulted below the frozen stack.
  - The last clone gives offline tiers at boot (§3.8's option a).
- **AD-310 (keeper's own OIDC client).**
  - The Matrix pieces cannot be reused past the registry (§2.1), and no JWT code exists (§2.6). `openidconnect` without default features adds no second HTTP or TLS stack (§6.1).
  - The checklist the library leaves to keeper is §6.2's YOU items.
  - One keychain item per session answers §2.5's per-item prompt.
  - One refresher per device answers §6.6.
- **AD-311 (the browser).**
  - The session's callback needs no Info.plist entry (§7.1 F1.15), but a *setup link* opened by the Camera app does need `CFBundleURLTypes` (§2.3). Both land in 82.5.
  - Non-ephemeral mode is what lets the forge leg ride the IdP cookie and 1Password or iCloud passkeys appear (§7.1, §7.2).
  - Loopback on desktop is RFC 8252's universal fallback (§7.5).
  - The own-module choice follows §7.6, not a plugin (§7.4).
- **AD-312 (the config repo).**
  - The direct-gix route (§4.7) keeps the repo out of every drive surface.
  - Basic through the gix callback with `username`/`password` as given (`oauth2` + token) works on Gitea, Forgejo, GitLab and oauth2-proxy (§8.1, §8.4).
  - Bearer rides `http.extraHeader` (§8.5 item 3) and `push_http`'s header.
  - Push through `push_http` everywhere, because gix cannot push (§8.5 item 1), and a desktop `git` shim would put a credential path next to argv, which AD-53 forbids (§4.2).
- **AD-313 (identity).** (`iss`, `sub`) is the only stable identifier (§6.2 D13k), so the directory is bound to it, not to the username the token claims.
- **AD-314 (the device).** A token carries no device, `hostname` is missing on iOS (§3.2), and no idiom code exists (§3.7). Hence a hostname slug on desktop and model plus suffix on iOS, both chosen by keeper.
- **AD-315 (the account as a credential).**
  - Each consumer keeps its own spelling (§4.2), and N audiences are N replay targets (§6.5), so the choice is opt-in per drive or provider.
  - keeper-sync unchanged means a drive receives the token in keeper-sync's spelling: the token as Basic username with an empty password. Gitea, Forgejo and oauth2-proxy accept that shape (§8.1, §8.4). GitLab documents only the password form (§8.3) `[INFERENCE: unsupported there]`.
- **AD-316 (the forge API base).** `forge_api_target` fails on sub-paths (§4.4), and no URL can be parsed into an API base in general (§8.6).

### 9.2 Where the evidence pulls against a pinned choice

Each tension is named, with where it went.

- **T1: `keeper://` vs RFC 8252 §7.1.**
  - The evidence says a private-use redirect scheme MUST be reverse-DNS, and a strict AS SHOULD reject a scheme with no period (§7.5; ROidc §7 item 4). The owner asked for `keeper://`, and the spec keeps it as the default while any value may be configured.
  - Zitadel accepts custom schemes (ROidc §7 item 4 [OIDC-S33]). Gitea and Forgejo showed no scheme validation (§8.2 item 3, `[INFERENCE]`). Authelia is `[UNVERIFIED]`.
  - On Apple a reverse-DNS redirect would work through the session without registration (§7.1). On other desktops it arrives by deep link, and only `keeper` is registered (§2.3).
  - **Went to:** DW-294.
- **T2: "gix cannot push" vs a synced config repo.** RForgeGit recommends git2 or a contents-API write path (§8.5). keeper already pushes over smart HTTP with reqwest on the phone (§4.6), so AD-312 uses `push_http` everywhere. **Resolved by the repo, not by a new dependency.**
- **T3: which device identity.**
  - GConfig recommends the sync.db device label for `keeper.<device>.toml` (§3.7, GConfig implication 2).
  - AD-314 uses a hostname slug on desktop, and on iOS a model plus a 4-hex suffix, persisted in `account.device_slug`.
  - `[INFERENCE]` sync.db belongs to keeper-sync, the account planner is keeper-core, and AD-40 forbids core → sync. The label is also person-editable and would move the account's files on every rename.
  - The two names can diverge on one machine. **Recorded, not resolved.** The epic's device list shows the account's slug, and `DeviceSection` still shows the commit label.
- **T4: two browser legs in `oauth` mode.** The forge leg costs a second consent alert, and on Forgejo a consent page every time (§7.1, §8.2 item 10). `same` mode avoids both but needs oauth2-proxy or Forgejo 16 integrations (§8.4). Token exchange would avoid both legs (§6.5). **Went to:** DW-295.
- **T5: one token, many audiences.** Reusing the sign-in token for drives and bots widens its replay surface (§6.5). **Held by:** AD-315's opt-in per row. **Went to:** DW-295 for per-service exchange.
- **T6: Forgejo's refresh invalidation.** `INVALIDATE_REFRESH_TOKENS = true` by default, with one grant per (user, app), means a sign-in or refresh on one device invalidates the forge refresh token on the others (§8.2 item 6, `[INFERENCE from code; not tested live]`). **Went to:** DW-292, and the operator note in `docs/account.md`.
- **T7: "every 15 minutes while keeper runs".** The spec (§3.4) says so. AD-62's no-timer rule and `task-host-tick.test.ts` forbid a Rust interval. **The contract's rule, accepted by the coordinator:** a sync at launch, on window focus throttled to 15 minutes, and on *Sync now*.
- **T8: the drive's spelling vs `config.auth`.**
  - Spec §3.6 said a drive using the account would follow `config.auth`'s scheme and username.
  - With keeper-sync unchanged (AD-315) it follows keeper-sync's `AccessToken` spellings (§4.2) instead, and in `oauth` mode it receives the IdP token, not the forge token.
  - **Accepted by the coordinator as AD-315's refinement.** DW-295 carries the forge-token case.
- **T9: roles from the access token.** Keycloak puts roles in the access token by default (§6.4 item 7), and clients must never read it (§6.4 item 13). **Held:** roles come from the verified ID token or sub-checked UserInfo only. A Keycloak operator enables the ID-token or UserInfo mapper (`docs/account.md`).
- **T10: keeper-syncd.** A second refresher per device would lose rotated sessions (§6.6). The daemon therefore does not use the account in this epic, and the app process is the single refresher (AD-310).
- **T11: `http.extraHeader` in the pinned fork.** Supported upstream at the transport level (§8.5 item 3) and `[UNVERIFIED]` in GSync. **Went to:** SyncTransport verifies it in the fork, or messages the coordinator with the alternative (contract).

---

## 10. Not established

Every `[UNVERIFIED]` item, with where it was looked for. None of these may be repeated as fact.

**keeper (repo)**
1. Whether iOS sets `HOME` to the sandbox home for keeper's process. Not derivable from the repo (GConfig §8).
2. That iOS requires `CFBundleURLTypes` for a deep link. Platform knowledge, not checked in-repo (GAuth §4). The auth-session callback does not require it (§7.1 F1.15).
3. Whether gix at the pinned fork supports `http.extraHeader` in-memory overrides on fetch and clone (GSync §3). Upstream says it does at the transport level [GIT-S36].

**OIDC and identity providers** (ROidc §10)
4. Authelia's refresh-token rotation and reuse behaviour.
5. Whether authentik puts scope claims (`groups`) into the ID token by default, or only into UserInfo.
6. Whether Zitadel revokes the whole session when a rotated refresh token is replayed.
7. Whether Zitadel's `end_session` invalidates `offline_access` refresh tokens.
8. Which Zitadel release will ship the `groups` claim. It is absent from v4.19.0.
9. The default of Keycloak's "Revoke Refresh Token" in the admin UI. Only the JPA entity default (`false`) was seen.
10. Keycloak and authentik support for RFC 8707 `resource`.
11. Whether Authelia accepts custom-scheme redirect URIs at runtime. Its docs and validator disagree.
12. When `oauth2-reqwest` reaches a stable release.
13. No end-to-end run against a live IdP. The flow is proven to compile only [OIDC-M2].

**Native sign-in UI** (ROAuthUI §7)
14. Whether the 1Password *Safari web extension*, as opposed to native macOS AutoFill, is active inside a Safari-hosted session window on macOS.
15. Whether session cookies carry over between two sequential non-ephemeral sessions on **macOS**. Okta's evidence is iOS-only.
16. Whether Chrome and Edge on macOS declare `CallbackURLMatchingIsSupported`. Custom-scheme requests are unaffected.
17. Keycloak's and authentik's default SSO cookie type (session vs persistent) and its effect on the second leg.
18. Tauri `startActivityForResult` plus Auth Tab. Needs a spike.
19. A WWDC session covering the iOS 17.4 `Callback` API. None was found.
20. Firefox on macOS and ASWebAuthenticationSession in 2026. The only source is a 2021 forum post.

**Forges and git** (RForgeGit §7–§8)
21. Whether Forgejo v16 enforces OAuth grant scopes on git and API calls, as its code suggests and its docs deny.
22. The scope-change behaviour and the multi-device counter hazard, tested live on Gitea 1.27 and Forgejo 16.
23. Whether Gitea's `redirect_to` must include the AppSubURL on sub-path installs (`[INFERENCE: yes]`).
24. An official GitHub statement that `gho_`/`ghu_` tokens work as git passwords.
25. Whether GitLab accepts `Authorization: Bearer` for git over HTTPS.
26. Any GitLab feature accepting external IdP access tokens as git credentials.
27. Official gitoxide iOS support or CI.
28. Whether Forgejo's `prompt=none` silent upstream sign-in ships in v16.0.
29. GitLab's behaviour when an app requests new scopes for an existing authorization.
30. The App Store and GPL question for libgit2. Moot here: libgit2 is banned (§4.8).

---

## 11. Sources

All accessed on 2026-09-23. The ids are the digests' own, re-prefixed.

### 11.1 ROidc (`OIDC-`)

**Crates and the Rust ecosystem**
- OIDC-S1 crates.io API, openidconnect: https://crates.io/api/v1/crates/openidconnect (+ `/4.0.1/dependencies`)
- OIDC-S2 crates.io API, oauth2: https://crates.io/api/v1/crates/oauth2 (+ `/5.0.0/dependencies`)
- OIDC-S3 crates.io API, openid: https://crates.io/api/v1/crates/openid (+ `/0.24.0/dependencies`); biscuit https://crates.io/api/v1/crates/biscuit
- OIDC-S4 crates.io API, jsonwebtoken: https://crates.io/api/v1/crates/jsonwebtoken
- OIDC-S5 crates.io API, oauth2-reqwest: https://crates.io/api/v1/crates/oauth2-reqwest
- OIDC-S6 openidconnect 4.0.1 source (ramosbugs): https://docs.rs/crate/openidconnect/4.0.1/source/, files `src/verification/mod.rs`, `src/core/jwk/mod.rs`, `src/discovery/mod.rs`, `src/logout.rs`, `src/id_token/mod.rs`, `examples/google.rs`
- OIDC-S7 openidconnect docs: https://docs.rs/openidconnect/4.0.1/openidconnect/
- OIDC-S8 oauth2 5.0.0 source: https://docs.rs/crate/oauth2/5.0.0/source/ (`src/types.rs`, `src/revocation.rs`, `src/reqwest_client.rs`)
- OIDC-S9 ramosbugs/oauth2-rs issue #333: https://github.com/ramosbugs/oauth2-rs/issues/333
- OIDC-S10 oauth2-reqwest docs: https://docs.rs/oauth2-reqwest/0.1.0-alpha.3/oauth2_reqwest/
- OIDC-S11 openidconnect-rs commits: https://github.com/ramosbugs/openidconnect-rs/commits/main
- OIDC-S12 kilork/openid README: https://github.com/kilork/openid/blob/master/README.md
- OIDC-S13 Keats/jsonwebtoken README: https://github.com/Keats/jsonwebtoken
- OIDC-S14 RustSec RUSTSEC-2023-0071: https://rustsec.org/advisories/RUSTSEC-2023-0071.html

**Specifications**
- OIDC-S15 OpenID Connect Core 1.0 errata set 2 (2023-12-15): https://openid.net/specs/openid-connect-core-1_0.html (§3.1.3.7, §3.1.3.8, §5.3.2, §5.7, §11, §12.2)
- OIDC-S16 OpenID Connect Discovery 1.0: https://openid.net/specs/openid-connect-discovery-1_0.html (§3, §4.3)
- OIDC-S17 OpenID Connect RP-Initiated Logout 1.0 (2022-09-12): https://openid.net/specs/openid-connect-rpinitiated-1_0.html
- OIDC-S18 IETF RFC 8252: https://www.rfc-editor.org/rfc/rfc8252
- OIDC-S19 IETF RFC 9700: https://www.rfc-editor.org/rfc/rfc9700
- OIDC-S20 IETF RFC 7009: https://www.rfc-editor.org/rfc/rfc7009
- OIDC-S21 IETF RFC 9068: https://www.rfc-editor.org/rfc/rfc9068
- OIDC-S22 IETF RFC 8707: https://www.rfc-editor.org/rfc/rfc8707
- OIDC-S23 IETF RFC 6749: https://www.rfc-editor.org/rfc/rfc6749 (§6)

**Zitadel**
- OIDC-S25 scopes: https://zitadel.com/docs/apis/openidoauth/scopes
- OIDC-S26 claims: https://zitadel.com/docs/apis/openidoauth/claims
- OIDC-S27 retrieve user roles: https://zitadel.com/docs/guides/integrate/retrieve-user-roles
- OIDC-S28 source `internal/api/oidc/client.go`: https://github.com/zitadel/zitadel/blob/main/internal/api/oidc/client.go
- OIDC-S29 source `internal/api/oidc/userinfo.go` (+ commit a9846498, "feat(group): add user groups to token claims (#11009)", 2025-11-11): https://github.com/zitadel/zitadel/blob/main/internal/api/oidc/userinfo.go
- OIDC-S30 endpoints: https://zitadel.com/docs/apis/openidoauth/endpoints
- OIDC-S31 applications: https://zitadel.com/docs/guides/manage/console/applications
- OIDC-S32 opaque tokens: https://zitadel.com/docs/concepts/knowledge/opaque-tokens
- OIDC-S33 zitadel/oidc `pkg/op/auth_request.go` (`validateAuthReqRedirectURINative`): https://github.com/zitadel/oidc/blob/main/pkg/op/auth_request.go
- OIDC-S34 source `internal/command/oidc_session_model.go` (`CheckRefreshToken`): https://github.com/zitadel/zitadel/blob/main/internal/command/oidc_session_model.go
- OIDC-S35 source `cmd/defaults.yaml`: https://github.com/zitadel/zitadel/blob/main/cmd/defaults.yaml
- OIDC-S36 web keys: https://zitadel.com/docs/guides/integrate/login/oidc/webkeys
- OIDC-S37 issue #9200: https://github.com/zitadel/zitadel/issues/9200
- OIDC-S38 VaulTLS issue #219 (field report): https://github.com/7ritn/VaulTLS/issues/219
- OIDC-S59 tag v4.19.0 `internal/api/oidc/client.go`: https://raw.githubusercontent.com/zitadel/zitadel/v4.19.0/internal/api/oidc/client.go ; latest release https://api.github.com/repos/zitadel/zitadel/releases/latest

**Keycloak**
- OIDC-S39 Server Admin Guide 26.7.4: https://www.keycloak.org/docs/latest/server_admin/index.html
- OIDC-S40 `GroupMembershipMapper.java`: https://github.com/keycloak/keycloak/blob/main/services/src/main/java/org/keycloak/protocol/oidc/mappers/GroupMembershipMapper.java
- OIDC-S41 `RealmEntity.java`: https://github.com/keycloak/keycloak/blob/main/model/jpa/src/main/java/org/keycloak/models/jpa/entities/RealmEntity.java

**authentik**
- OIDC-S42 OAuth 2.0 provider docs: https://docs.goauthentik.io/add-secure-apps/providers/oauth2/
- OIDC-S43 blueprint `providers-oauth2.yaml`: https://github.com/goauthentik/authentik/blob/main/blueprints/system/providers-oauth2.yaml
- OIDC-S44 source `authentik/providers/oauth2/models.py`, `/views/token.py`, `/token/refresh_token.py`: https://github.com/goauthentik/authentik/blob/main/authentik/providers/oauth2/models.py

**Authelia**
- OIDC-S45 OIDC claims: https://www.authelia.com/integration/openid-connect/openid-connect-1.0-claims/
- OIDC-S46 OIDC clients: https://www.authelia.com/configuration/identity-providers/openid-connect/clients/
- OIDC-S47 OIDC introduction: https://www.authelia.com/integration/openid-connect/introduction/
- OIDC-S48 validator `identity_providers.go`: https://github.com/authelia/authelia/blob/master/internal/configuration/validator/identity_providers.go

**Apple, AppAuth, Android and Tauri**
- OIDC-S49 Apple, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`: https://developer.apple.com/documentation/security/ksecattraccessibleafterfirstunlockthisdeviceonly
- OIDC-S50 Apple, `kSecUseDataProtectionKeychain`: https://developer.apple.com/documentation/security/ksecusedataprotectionkeychain
- OIDC-S51 Apple, `kSecAttrSynchronizable`: https://developer.apple.com/documentation/security/ksecattrsynchronizable
- OIDC-S52 Apple, sharing keychain items among apps: https://developer.apple.com/documentation/security/sharing-access-to-keychain-items-among-a-collection-of-apps
- OIDC-S53 Apple, ASWebAuthenticationSession (+ `/prefersephemeralwebbrowsersession`, `/callback`, `authenticating-a-user-through-a-web-service`): https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession
- OIDC-S54 AppAuth-iOS `OIDEndSessionRequest.h`, `OIDExternalUserAgentIOS.m`: https://github.com/openid/AppAuth-iOS/blob/master/Sources/AppAuthCore/OIDEndSessionRequest.h
- OIDC-S55 AppAuth-Android README: https://github.com/openid/AppAuth-Android/blob/master/README.md
- OIDC-S56 Android security release notes: https://developer.android.com/jetpack/androidx/releases/security
- OIDC-S57 keyring 3.6.3 source `src/macos.rs`, `src/ios.rs`: https://docs.rs/crate/keyring/3.6.3/source/src/macos.rs
- OIDC-S58 crates.io search for Tauri auth plugins: https://crates.io/api/v1/crates?q=tauri-plugin-oauth. It found:
  - `tauri-plugin-oauth` 2.1.0 (a localhost server);
  - `tauri-plugin-web-auth` 1.0.0 (last updated 2025-04-21);
  - `tauri-plugin-appauth` 0.2.0;
  - `objc2-authentication-services` 0.3.2 (`Zlib OR Apache-2.0 OR MIT`).

**Measurements**
- OIDC-M1: a scratch crate with `openidconnect@4.0.1` default features. `cargo tree -e normal --no-dedupe` gave 154 unique crate versions: 83 are reqwest 0.12's tree and 71 are added on top. rustc 1.98.0.
- OIDC-M2: `/tmp/oidc-proof/src/lib.rs` passes `cargo check`. It covers discovery with custom metadata, PKCE, nonce, the Zitadel scopes and roles claim, `set_allowed_algs`, `set_other_audience_verifier_fn`, refresh, revocation (typestate-gated) and the logout URL. **A compile check only.**
- OIDC-M3: the M2 tree against `src-tauri/Cargo.lock`. The lock already has `oauth2` 5.0.0, `oauth2-reqwest` 0.1.0-alpha.3, a single `reqwest` 0.13.4 and `keyring` 3.6.3. 23 crate names are new, with licences checked through the crates.io API.

### 11.2 ROAuthUI (`UI-`)

- UI-S1 Apple, ASWebAuthenticationSession: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession
- UI-S2 Apple, `prefersEphemeralWebBrowserSession`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession/prefersephemeralwebbrowsersession
- UI-S3 Apple, authenticating a user through a web service: https://developer.apple.com/documentation/authenticationservices/authenticating-a-user-through-a-web-service
- UI-S4 Apple, `ASWebAuthenticationSession.Callback`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession/callback
- UI-S5 Apple, `Callback.https(host:path:)`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession/callback/https(host:path:)
- UI-S6 Apple, `ASWebAuthenticationSessionError.Code`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsessionerror/code
- UI-S7 Apple, `ASWebAuthenticationPresentationContextProviding`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationpresentationcontextproviding
- UI-S8 Apple, `start()`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession/start()
- UI-S9 Apple, supporting single sign-on in a web browser app: https://developer.apple.com/documentation/authenticationservices/supporting-single-sign-on-in-a-web-browser-app
- UI-S10 Apple, `additionalHeaderFields`: https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession/additionalheaderfields
- UI-S11 Apple, WWDC19 session 516 "What's New in Authentication" (transcript): https://developer.apple.com/videos/play/wwdc2019/516
- UI-S12 docs.rs, objc2-authentication-services 0.3.2 (Apple's header docs and the generated source): https://docs.rs/objc2-authentication-services/latest/objc2_authentication_services/struct.ASWebAuthenticationSession.html and https://docs.rs/objc2-authentication-services/latest/src/objc2_authentication_services/generated/ASWebAuthenticationSession.rs.html
- UI-S13 docs.rs, objc2 `available!`: https://docs.rs/objc2/latest/objc2/macro.available.html
- UI-S14 Okta Developer blog (2022-01-13): https://developer.okta.com/blog/2022/01/13/mobile-sso
- UI-S15 Okta Developer blog (2021-11-12): https://developer.okta.com/blog/2021/11/12/native-sso
- UI-S16 passkeys.dev, iOS: https://passkeys.dev/docs/reference/ios/
- UI-S17 passkeys.dev, macOS: https://passkeys.dev/docs/reference/macos/
- UI-S18 1Password Support, iOS AutoFill (2026-08-17): https://support.1password.com/ios-autofill/
- UI-S19 1Password Community, "macOS AutoFill is now in public beta" (2026-05-26): https://www.1password.community/1password-at-home-31/macos-autofill-is-now-in-public-beta-24649
- UI-S20 1Password Community, "macOS AutoFill is now available to everyone": https://www.1password.community/announcements-52/macos-autofill-is-now-available-to-everyone-25254
- UI-S21 1Password Support, Android AutoFill: https://support.1password.com/android-autofill/
- UI-S22 Chrome for Developers, Auth Tab: https://developer.chrome.com/docs/android/custom-tabs/guide-auth-tab
- UI-S23 Chrome for Developers, Ephemeral Custom Tabs: https://developer.chrome.com/docs/android/custom-tabs/guide-ephemeral-tab
- UI-S24 Android Developers, `AuthTabIntent`: https://developer.android.com/reference/androidx/browser/auth/AuthTabIntent
- UI-S25 androidx.browser release notes: https://developer.android.com/jetpack/androidx/releases/browser
- UI-S26 Google Maven, androidx.browser metadata: https://dl.google.com/android/maven2/androidx/browser/browser/maven-metadata.xml
- UI-S27 IETF RFC 8252 (BCP 212): https://www.rfc-editor.org/rfc/rfc8252
- UI-S28 crates.io API, per crate: https://crates.io/api/v1/crates/{tauri-plugin-web-auth, tauri-plugin-auth-session, tauri-plugin-plauth, tauri-plugin-oauth, tauri-plugin-oauth-session, tauri-plugin-appauth, objc2-authentication-services, authenticationservices-rs, tauri-plugin-deep-link, tauri-plugin-opener}
- UI-S29 GitHub, Manaf941/tauri-plugin-web_auth (README, `WebAuthPlugin.swift`, `WebAuthPlugin.kt`, commit log): https://github.com/Manaf941/tauri-plugin-web_auth
- UI-S30 GitHub, yanqianglu/tauri-plugin-auth-session (README, `src/apple.rs`, `src/lib.rs`, `Cargo.toml`, `AuthSessionActivity.kt`, `AuthSessionPlugin.kt`): https://github.com/yanqianglu/tauri-plugin-auth-session
- UI-S31 tauri-plugin-auth-session issue #1, "Build failed on iOS": https://github.com/yanqianglu/tauri-plugin-auth-session/issues/1
- UI-S32 tauri-plugin-plauth README: https://github.com/lecaobaophuc0912/tauri-plugin-plauth
- UI-S33 FabianLars/tauri-plugin-oauth (README, `src/lib.rs`): https://github.com/FabianLars/tauri-plugin-oauth
- UI-S34 GitHub API, tauri-apps/plugins-workspace `plugins/` at v2: https://api.github.com/repos/tauri-apps/plugins-workspace/contents/plugins?ref=v2
- UI-S35 Auth0 Docs, Flutter quickstart (HTTPS callbacks): https://auth0.com/docs/quickstart/native/flutter
- UI-S36 Jamf Nation thread (Chrome and Edge and ASWebAuthenticationSession): https://community.jamf.com/general-discussions-2/jamfaad-issue-24138
- UI-S37 AppAuth-Android README: https://raw.githubusercontent.com/openid/AppAuth-Android/master/README.md

### 11.3 RForgeGit (`GIT-`)

- GIT-S1 Gitea Docs, "OAuth2 Provider" (1.27): https://docs.gitea.com/development/oauth2-provider/
- GIT-S2 Forgejo Docs, "OAuth2 provider": https://forgejo.org/docs/latest/user/authentication/oauth2-provider/
- GIT-S3 go-gitea/gitea `services/auth/basic.go` (main): https://github.com/go-gitea/gitea/blob/main/services/auth/basic.go
- GIT-S4 go-gitea/gitea `services/auth/oauth2.go` (main): https://github.com/go-gitea/gitea/blob/main/services/auth/oauth2.go
- GIT-S5 go-gitea/gitea `routers/web/web.go` (main; lines 119-160, 773-776, 1768, 1773): https://github.com/go-gitea/gitea/blob/main/routers/web/web.go
- GIT-S6 go-gitea/gitea `routers/web/auth/oauth2_provider.go` (main): https://github.com/go-gitea/gitea/blob/main/routers/web/auth/oauth2_provider.go
- GIT-S7 go-gitea/gitea `models/auth/oauth2.go` (main): https://github.com/go-gitea/gitea/blob/main/models/auth/oauth2.go
- GIT-S8 go-gitea/gitea `services/oauth2_provider/access_token.go` (main): https://github.com/go-gitea/gitea/blob/main/services/oauth2_provider/access_token.go
- GIT-S9 go-gitea/gitea `routers/web/repo/githttp.go` (main): https://github.com/go-gitea/gitea/blob/main/routers/web/repo/githttp.go
- GIT-S10 go-gitea/gitea `services/context/permission.go` (`CheckRepoScopedToken`): https://github.com/go-gitea/gitea/blob/main/services/context/permission.go
- GIT-S11 go-gitea/gitea `routers/web/auth/oauth.go` (`SignInOAuth`, `rememberAuthRedirectLink`) and `routers/web/auth/auth.go:193-200` (main)
- GIT-S12 go-gitea/gitea `modules/structs/user.go` and `routers/api/v1/api.go:1174-1295` (main). GIT-S12b: `services/forms/user_form.go` (`EditOAuth2ApplicationForm`) and `modules/structs/user_app.go` (`CreateOAuth2ApplicationOptions`)
- GIT-S13 go-gitea/gitea `release/v1.21` … `release/v1.27`: grep of `models/auth/oauth2.go`, `routers/web/auth/*`, `services/auth/basic.go`, `services/oauth2_provider/access_token.go`
- GIT-S14 Gitea Docs, Config Cheat Sheet: https://docs.gitea.com/administration/config-cheat-sheet/
- GIT-S15 Gitea Docs, Reverse Proxies ("Use a sub-path"): https://docs.gitea.com/administration/reverse-proxies/
- GIT-S16 go-gitea/gitea `modules/setting/oauth2.go` (main): https://github.com/go-gitea/gitea/blob/main/modules/setting/oauth2.go
- GIT-S17 Forgejo source on Codeberg, branches `forgejo` (dev) and `v16.0/forgejo`: `services/auth/method/{oauth2.go,util.go,basic.go,authorized_integration.go}`, `routers/web/web.go:110-196`, `routers/web/auth/oauth.go` (lines 162, 509, 525-552, 635, 828, 955-975), `routers/web/repo/githttp.go:151`, `models/auth/oauth2.go:473-475`, `modules/setting/oauth2.go:98-109`. https://codeberg.org/forgejo/forgejo
- GIT-S18 Forgejo Docs, "Authorized Integrations": https://forgejo.org/docs/latest/user/api/authorized-integrations/
- GIT-S19 Forgejo releases API (v16.0.0 2026-07-16, v16.0.5 2026-09-17, v15.0.0 2026-04-16): https://codeberg.org/api/v1/repos/forgejo/forgejo/releases
- GIT-S20 Live responses from `https://gitea.com/.well-known/openid-configuration`, `https://codeberg.org/.well-known/openid-configuration` and `/api/v1/version` on both hosts; codeberg `info/refs` 401 header
- GIT-S21 GitLab Docs, "OAuth 2.0 identity provider API": https://docs.gitlab.com/api/oauth2/
- GIT-S22 GitLab `doc/integration/oauth_provider.md` and `doc/api/applications.md`: https://gitlab.com/gitlab-org/gitlab/-/tree/master/doc
- GIT-S23 GitLab `doc/install/relative_url.md` and `doc/user/group/subgroups/_index.md`
- GIT-S24 GitHub Docs, "Authenticating as a GitHub App installation": https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation
- GIT-S25 GitHub Docs, "Authorizing OAuth apps": https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps
- GIT-S26 GitHub Changelog 2025-07-14, "PKCE support for OAuth and GitHub App authentication": https://github.blog/changelog/2025-07-14-pkce-support-for-oauth-and-github-app-authentication/
- GIT-S27 GitHub Changelog 2026-08-14, "Multiple redirect URIs and token refresh for OAuth apps": https://github.blog/changelog/2026-08-14-multiple-redirect-uris-and-token-refresh-for-oauth-apps/
- GIT-S28 GitHub Docs (GHES), "Getting started with the REST API" (`/api/v3`): https://docs.github.com/en/enterprise-server@latest/rest/using-the-rest-api/getting-started-with-the-rest-api
- GIT-S29 hickford/git-credential-oauth `main.go` (lines 39-110, 199-363): https://github.com/hickford/git-credential-oauth/blob/main/main.go
- GIT-S30 git-scm.com, `git-credential` (`authtype`, `credential`, `ephemeral`, `capability[]`): https://git-scm.com/docs/git-credential ; git 2.46.0 release notes: https://github.com/git/git/blob/master/Documentation/RelNotes/2.46.0.adoc
- GIT-S31 charmbracelet/soft-serve `pkg/web/auth.go`: https://github.com/charmbracelet/soft-serve/blob/main/pkg/web/auth.go
- GIT-S32 Gerrit Docs, config-gerrit (`auth.type`, `auth.gitBasicAuthPolicy`, `auth.httpHeader`): https://gerrit-review.googlesource.com/Documentation/config-gerrit.html
- GIT-S33 gerrit-oauth-provider (GitHub mirror davido/gerrit-oauth-provider), `Module.java` and `DisabledOAuthLoginProvider.java`: https://github.com/davido/gerrit-oauth-provider
- GIT-S34 oauth2-proxy `pkg/middleware/jwt_session.go` and `docs/docs/configuration/overview.md`: https://github.com/oauth2-proxy/oauth2-proxy
- GIT-S35 GitoxideLabs/gitoxide `crate-status.md`: https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md
- GIT-S36 gitoxide `gix-transport/src/client/blocking_io/http/mod.rs` and `…/http/reqwest/mod.rs`
- GIT-S37 docs.rs gix 0.87.1 `remote::Connection`; gitoxide `gix/Cargo.toml`, `gix/src/clone/{mod.rs,access.rs}`, `gix/src/open/options.rs`, `gix/src/remote/connection/access.rs`, `gix-credentials/src/protocol/context/serde.rs`, `gix-sec/src/identity.rs`
- GIT-S38 gitoxide issue #575, "IOS Support…": https://github.com/GitoxideLabs/gitoxide/issues/575
- GIT-S39 rust-lang/git2-rs `Cargo.toml`, `libgit2-sys/build.rs`, `src/remote.rs`; issue #1185: https://github.com/rust-lang/git2-rs/issues/1185
- GIT-S40 go-gitea/gitea issue #31470: https://github.com/go-gitea/gitea/issues/31470

### 11.4 Repository digests (`[REPO]`)

GAuth, GConfig, GSync and GSurface were read at `6fbc2c3` on 2026-09-23. Their citations appear inline as `path:line`, and §1.3 records which of those lines this pass re-verified.

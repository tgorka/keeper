# Epic 82 — An optional account, and a config that follows you

created: '2026-09-23'
source: the owner's request of 2026-09-23 (Polish, verbatim below) and the task "optional account sign-in (OIDC) + per-person config synced from a git repository". Four read-only lanes grounded it in the repository at `origin/main` `6fbc2c3` (`local://acct-GAuth.md`, `acct-GConfig.md`, `acct-GSync.md`, `acct-GSurface.md`), and three research lanes read the outside world (`acct-ROidc.md`, `acct-ROAuthUI.md`, `acct-RForgeGit.md`); the seven digests are synthesised, with their citations, in `research-account-2026-09-23.md`. The coordinator wrote the model as `spec-82-account-and-config-repo-proposal.md`, which the owner approved on 2026-09-23 with one amendment (the account's layer files sit **above** `~/.keeper`, below the main folder), then froze it as `local://acct-contract.md` and amended it twice during the build wave: A1 (the module is `org_account`) and A2 (egress, *Forget this account*, the drive's credential spelling).
binds: FR-681…FR-707 and NFR-92…NFR-95 (allocated here, defined in *Requirements allocated here*; FR-677…FR-680 of epic 81's block stay unallocated); AD-308…AD-316 (pinned by the coordinator in the frozen contract, written out below in Binds / Prevents / Rule form); UX-DR116 (defined by the design lane, Front, and included verbatim in *Decisions this epic takes*; UX-DR112…UX-DR115 of epic 79's reserved block stay unallocated); DW-290…DW-297 (allocated in *What stays out*); D-25 (drafted at the end, for `docs/decisions.md`). **FR-676, NFR-91, AD-307, UX-DR111, DW-289 and D-24 were the previous ceilings** (epic 81's binds line; D-24 is `docs/decisions.md:1207`). A repo-wide grep for AD-308…AD-319, FR-681…FR-689, NFR-92…NFR-99, UX-DR116, DW-290…DW-299 and D-25 found no prior use (`research-account-2026-09-23.md` §1.4).
see-also: epic 2 (Story 2.2, the Matrix OIDC flow and `OAuthFlowRegistry`, reused here for routing only); epic 30 (keeper-syncd, AD-52, which does not use the account here); epic 46 (the file is the setting: AD-98…AD-101, the layer stack and the folder tier this epic extends); epic 66 (the phone has a folder: AD-198, AD-199, AD-204, and `push_http`, AD-202, which the config repository pushes through on every platform); AD-27 (absent rather than disabled), AD-40 (the crate split), AD-41 (the git shim), AD-53 (credentials and egress); `docs/account.md` (the user and operator guide this epic writes); `research-account-2026-09-23.md`.

## The owner's ask

Verbatim, in Polish, as sent:

> logowanie za pomoca oauth i uzywanie konfiguracji (sciaganie lub tworzenie) na
> remote - git jak drive ale dostep bedzie juz za pierwszym uruchomieniem. Miej na
> uwadze zeby UI i UX byly spojne i dobre do uzycia - dodaj zarzadzanie
> uzytkownikami - moze onboarding nowych uzytkownikow przez qrcode (wygenerowanie
> przez keepera podczas onboardingu)
> dostep do konta auth ma pozwalac sync konfiguracji i latwiejsze dodawanie innych
> drivow oraz laczenie do botow itp (jezli jest to sync po oauth) - jak nie ma
> internetu to wszystko chodzi normalnie to co jest offline.

In English: sign in with OAuth and use a configuration that lives on a remote, either downloading it or creating it there. The remote is git, like a drive, but reachable from the very first launch. UI and UX must be consistent and easy to use. Add user management, possibly onboarding new people through a QR code that keeper generates during onboarding. Access to the account should sync the configuration, make adding more drives easier, and connect to bots and so on (if that is a sync over OAuth). Without internet, everything that works offline keeps working.

**The task, as the coordinator stated it:** optional account sign-in (OIDC) plus per-person config synced from a git repository. The account is optional, and its token is a general credential. The configuration is a per-person directory in one git repository, and one input (a setup link, a QR code or a pasted link) bootstraps it on a new device.

The owner decided on 2026-09-23:
- **Precedence.** The account's two layer files sit **above** `~/.keeper` (the repository wins over this machine's hand edits) and **below** the main sync folder and the per-folder files. The proposal had put them lowest.
- The rest of the proposal as written:
  - a separate `account.toml` store;
  - the `keeper://oauth/<id>/callback` redirect default, with any value configurable;
  - all seven stories including 82.7;
  - keeper managing only *your* identity and devices, and onboarding others by link or QR code.

## What the triage found

Spec §1, each claim read at `6fbc2c3`. The verdicts use the triage vocabulary: present / absent / broken / half-present.

| Need | Verdict | What the code says |
| --- | --- | --- |
| A generic OIDC client (PKCE, state, nonce, id_token) | **absent** | The one OIDC flow is Matrix's, and matrix-sdk owns its PKCE verifier internally. `keeper-core/src/auth.rs:64-81` (`AuthProvider::authenticate(&Client, …)`); matrix-sdk `auth_code_builder.rs:135-160`. |
| Callback routing by `state` | **present, reusable** | Free of Tauri and Matrix code: `keeper-core/src/oauth.rs:76-247`. |
| A deep-link handler | **present, desktop only** | There is one handler: voice links first, then everything else to the OAuth registry. It does not handle a link that launches the app cold, and iOS registers no URL scheme. `crates/keeper/src/voice_reach.rs:101-113`; `tauri.conf.json:52-55`; `gen/apple/**` has no `CFBundleURLTypes`. |
| ASWebAuthenticationSession / Custom Tabs | **absent** | Sign-in opens the system browser through `tauri_plugin_opener`: `crates/keeper/src/ipc.rs:709-714, 894-898`. |
| A keychain port with a cache that avoids repeated ACL prompts | **present** | `keeper-core/src/platform.rs:26-84, 104-224`. |
| reqwest + rustls HTTP client | **present** | No new TLS stack is needed: `src-tauri/Cargo.toml:84-85`. |
| Git clone/fetch with a credential supplied per request | **present** | gix, never a credential helper, Basic only today: `keeper-sync/src/git/repo.rs:587`, `git/fetch.rs:130, 219-241`. |
| Git push without the git binary | **present** | The phone's smart-HTTP push over reqwest: `keeper-sync/src/git/push_http.rs:127`. |
| Layer files with fault reporting and the "Set by a file" badge | **present, frozen at boot** | A `OnceLock`: `keeper-core/src/config/mod.rs:87-181, 835-864`; `src/components/settings/config-source-section.tsx:87-102`. |
| Per-device identity | **half-present** | A sync.db device label exists, but there is no device class, and iOS has no hostname (`hostname` does not exist in the sandbox). `keeper-sync/src/db.rs:1626-1699`; `config/mod.rs:379`. |
| QR rendering | **present** | In Rust as SVG. keeper has **no** QR scanner. `keeper-core/src/bridges/login.rs:42-64`. |
| Forge API base from the remote URL | **broken** for sub-path forges | `https://host/git/owner/repo.git` → `None`: `keeper-sync/src/engine.rs:36685-36736`. |
| Android | **absent** | No target and no `Platform`: `crates/keeper/src/ipc.rs:460-464`. |

The line numbers for `config/mod.rs`, `oauth.rs`, `credential.rs` and `push_http.rs` are the ones this plan re-verified. They differ from the digests' in places, and `research-account-2026-09-23.md` §1.3 lists every drift.

## The one sentence

**keeper signs a person in only to Matrix, reads its layer files once at boot, keeps a git credential per drive, cannot deliver a link to a phone or to an app that is not running, and derives a forge API from a URL that cannot hold it. The fix: keeper gets its own OIDC client whose token lives in one keychain item and serves anyone who opts in, a descriptor outside the layer stack that one link, QR code or paste delivers, a per-person directory in a git repository that keeper creates, never overwrites and never leaves, and two live layer tiers that fetch when they can and stay applied when they cannot.**

## What earlier epics decided, and what this epic amends

Nothing here reverses an owner decision. Each row quotes the earlier rule and gives the reason for the change or for holding it.

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **The frozen stack** (epic 46, `config/mod.rs:835`) | the resolved set "never changes, because the only later layers are per-folder and a folder may not set a settings key at all" (the design comment GConfig §9 quotes) | Account layers are known only after sign-in, and a fetch can change them while keeper runs. | **AD-309.** The frozen `OnceLock` stack stays frozen. The two account tiers live beside it in their own `RwLock`, consulted by `setting_override` below it. The premise is narrowed, not dropped: no *other* tier becomes live. |
| **AD-101** (`config/mod.rs:179-181`) | only `~/.keeper/` may elect the main folder | A per-person file that travels between machines. | **Held.** `may_set_main_folder = false` for both account tiers, with the existing fault (DW-296). |
| **AD-40** (`ARCHITECTURE-SPINE.md:293`) | "`keeper-core` **MUST NOT** depend on `keeper-sync`" | The layout decisions (keeper-core) must drive git writes (keeper-sync). | **Held.** keeper-core plans, keeper-sync moves bytes, and the shell joins them by passing keeper-core's planner into keeper-sync as a closure (AD-312). |
| **AD-41 / AD-202** (`ARCHITECTURE-SPINE.md:298`; `engine.rs:10089-10104`) | desktop push shells to `git`; the phone pushes over smart HTTP because iOS denies `posix_spawn` | A config repo pushed with a sign-in token that must never reach argv or a helper. | **Amended for the config repo only.** `push_http` pushes it on every platform (AD-312). Drives are unchanged. |
| **AD-53** (`ARCHITECTURE-SPINE.md:358`) | "Every configured sync remote is a **disclosed egress destination** … computed from the live profile set, never hand-maintained"; credentials never touch config files | New destinations: the issuer, the descriptor host, the config repository and the forge. | **Extended (contract A2a).** The account's hosts are computed into the same list from the descriptor, and only when one is configured (FR-684). |
| **`credential.rs:21`** | "A fourth consumer gets a method here or it does not get the token." | A drive that uses the account's token instead of its own. | **Held.** A drive using the account still reads its token through `SyncPlatform::secret_get` and is dressed by `AccessToken`'s existing spellings. keeper-sync is unchanged (AD-315). `[INFERENCE]` The config repository's credential is a separate neutral type (`RepoAuth`, AD-312) and never goes through `AccessToken`, so it is not a fourth consumer of the drive token. |
| **The single deep-link handler** (`voice_reach.rs:99-113`) | voice links first; "every other URL goes to `flows.resolve(url)`"; a second `on_open_url` replaces the first | `keeper://setup?…`, and links that launch keeper. | **Extended (AD-311).** A setup arm in the same handler, before the OAuth fallthrough, and a cold-start `get_current()` once after setup. |
| **Matrix's redirect** (`oauth.rs:31`) | `REDIRECT_URI = "keeper://oauth/callback"` | A redirect per account, and one for the forge leg. | **Untouched.** The account uses `keeper://oauth/<id>/callback` and `keeper://oauth/<id>/forge/callback`. |
| **Spec §3.6** (this epic's own proposal) | "Username/scheme follow the account's `config.auth` in `same` mode" | keeper-sync stays unchanged. | **AD-315's refinement (contract A2c).** A drive uses keeper-sync's spelling: the token as the Basic username with an empty password (`AccessToken::git`), Basic `token:` for LFS, `token` for the forge API. In `oauth` mode a drive receives the sign-in token, not the forge token. |
| **Spec §3.4** (this epic's own proposal) | a retry "every 15 minutes while keeper runs" | AD-62 forbids timers in Rust (`task-host-tick.test.ts`). | **The contract's rule, accepted by the coordinator.** A sync runs at launch, on window focus at most once per 15 minutes, and on *Sync now* (FR-702). |

## Decisions this epic takes

The rules below are the plan. *Contract amendments* (A1, A2), further down, were ruled by the coordinator during the build wave, and the rules below already include them.

- **AD-308: The account descriptor is its own store, outside the layer stack.**

  **Binds:** FR-681, FR-682, FR-683, FR-684, FR-701; NFR-92, NFR-93; Story 82.1 (and 82.5's `account_forget`); UX-DR116.

  **Prevents:**
  - a descriptor read by the stack it feeds;
  - a config repository that can redirect itself: `<login>/keeper.toml` naming another issuer or repo;
  - a new per-tier exception in a parser that has none (`config/mod.rs`, `UnknownTable`);
  - a client secret shipped to every device;
  - a setup link that writes anything before the person has seen which hosts it names;
  - a descriptor fetch that follows a redirect or reads without bound;
  - a destination keeper contacts but About does not list (AD-53);
  - a second account shape that needs a migration later.

  **Rule:**
  - **The type.** One serde struct, `org_account::descriptor::AccountDescriptor { version, id, name, auth, config }`, with the TOML schema of `docs/account.md` §"The descriptor". Operators serve it as JSON, and keeper stores it as TOML.
  - **Where it lives.** `~/.keeper/account.toml` on desktop and `<app data dir>/account.toml` on iOS (`FILE_NAME`; `load`; `store`, which writes atomically).
    - Absent: `Ok(None)` and no fault.
    - Malformed, or failing `validate`: a `LayerFault` of tier `LayerTier::AccountDescriptor`, shown in Settings' fault list as `ConfigTierVm::Account`, and **no account**. Nothing else changes.
  - **`validate`** enforces the defaults and refusals of FR-681/FR-682.
  - **`parse_setup_input`** accepts exactly three forms:
    - `keeper://setup?descriptor=<url-encoded https URL>`;
    - `keeper://setup?d=<base64url(JSON)>`;
    - a bare `https://…` URL.
  - **`fetch`** is HTTPS-only, follows no redirects, and stops at 64 KiB.
  - **`setup_link`** writes `descriptor=` when the source URL is known, and `d=` otherwise.
  - **One account per install** (DW-297). The `id` is in the schema, the redirect URI and the keychain keys, so a list can come later without a migration.
  - **Egress** (A2a). `egress::compute_egress` gains the account's destinations, derived from the descriptor: the issuer host, the descriptor URL's host when known, the config repository's host, and the forge host in `oauth` mode. They are present only while a descriptor is configured.

- **AD-309: Two account layer tiers sit between `~/.keeper` and the main folder. They are live, and they may not elect the main folder or carry `[folder]`.**

  **Binds:** FR-695, FR-696, FR-697, FR-702; NFR-92, NFR-94; Story 82.4 (and 82.5's boot install).

  **Prevents:**
  - this machine's stale hand edits beating the person's repository (the owner's amendment);
  - an account file that elects the main folder (AD-101);
  - `[folder]` in a file that is in no folder;
  - a stack that ignores a fetch until relaunch;
  - relaxing the `OnceLock` for every tier to serve two;
  - a launch with no network that also has no settings;
  - an unknown key in an account file that is silently ignored;
  - a badge that cannot say which repository file set a key.

  **Rule:**
  - **The tiers.** `LayerTier` gains `AccountShared` and `AccountDevice`, in `ORDER` between `UserGlobalMachine` and `MainShared`, and `AccountDescriptor`, which is not in `ORDER` and exists for faults only.
  - **Predicates:**
    - both account tiers have `may_set_settings = true`, `may_set_main_folder = false` (the existing main-folder fault) and `has_folder = false`;
    - `AccountDevice` is `machine_scoped`;
    - the labels are "your account's settings (every device)", "your account's settings (this device)" and "account.toml".
  - **The files.** `<clone>/<login>/keeper.toml` and `<clone>/<login>/keeper.<device>.toml`, read by the same parser with the same per-key faults (`AccountLayerSource { dir, device, account_name }`, `load_account_layers`).
  - **The lock.** They are held in their own `RwLock`: `install_account_layers(Some(..))` swaps them in, `None` clears them, and `set_account_faults` replaces their faults.
  - **Precedence.** `setting_override(key)` answers in this order, first match wins: MainMachine > MainShared > AccountDevice > AccountShared > UserGlobalMachine > UserGlobal.
  - **Display.** Overrides and faults of the account tiers appear in `ConfigLayersVm` as `ConfigTierVm::{AccountShared, AccountDevice, Account}`, with phrases naming the account and the repository-relative path (`acme: tgorka/keeper.toml`).
  - **Boot.** When a descriptor, a clone and a stored identity exist, the shell installs the tiers from the clone right after `config::install`, before the first affected settings read, without network. It then starts a background sync.
  - **When a change takes effect.**
    - A key read on every access takes effect when a sync swaps the tiers.
    - A key consumed only at boot (hotkeys, the debug log) takes effect at the next launch. The account tiers' phrase, the source of the badge's tooltip, ends with a clause saying so.
  - `GENERATED_HEADER`'s file list and `docs/settings-keys.md` are regenerated.

- **AD-310: keeper runs its own OIDC client. A session is one keychain item, there is one refresher, and `access_token()` is the one API.**

  **Binds:** FR-685, FR-686, FR-687, FR-688, FR-693; NFR-93, NFR-95; Story 82.2.

  **Prevents:**
  - reusing matrix-sdk's flow, which is welded to a Matrix `Client` and owns its verifier;
  - a second reqwest or TLS stack (`openidconnect`'s default features pull reqwest 0.12);
  - roles read from an access token;
  - Zitadel's project-id audience refused, or any audience accepted;
  - a state or nonce replayed;
  - two refreshers losing a rotated session;
  - three macOS ACL prompts per launch instead of one;
  - *offline* confused with *sign in again*;
  - a token in IPC, a log, a file, argv or a URL.

  **Rule:**
  - **The modules.** keeper-core `org_account::{oidc, session, loopback}` on `openidconnect = { version = "4.0.1", default-features = false }` plus `oauth2-reqwest = "=0.1.0-alpha.3"`, over the existing reqwest 0.13 client with redirects off. `cargo deny` licences pass.
  - **`sign_in`:**
    1. discovery, with the descriptor's endpoint overrides;
    2. fresh `state`, `nonce` and PKCE S256 per attempt, each consumed once;
    3. `platform.start_web_auth(auth_url, redirect)`, then await the registry by `state` for 300 s;
    4. exchange;
    5. validate by the checklist in `research-account-2026-09-23.md` §6.2 (FR-685);
    6. apply the claims (FR-686);
    7. **persist the session before returning**.
  - **Keychain:**
    - `account/<id>/session` holds JSON `{iss, sub, refresh_token, access_token, access_expires_ms, id_token, login, display_name, email, roles}`;
    - `account/<id>/forge` holds JSON `{access_token, refresh_token, expires_ms, login}`.
  - **`access_token()`** is single-flight per account id. It refreshes 60 s early and persists a rotated refresh token **before** the new access token is used.
  - **Errors** are `NeedsSignIn` (grant dead, refresh rejected, no session), `Unreachable` (network, DNS, TLS, timeout), `Refused` (policy), `Cancelled` and `Internal`.
  - **`sign_out`** revokes the refresh token(s) where a revocation endpoint exists, always deletes both keychain items, and returns the end-session URL for the shell to open.
  - **`identity()`** reads the stored session without network.
  - **`git_credential`** maps `RepoAuthConfig` and a token to the neutral `GitAuth::{None, Basic, Bearer}`.
  - **One refresher.** The app process is the only refresher on a device. keeper-syncd does not use the account in this epic.

- **AD-311: The browser is `Platform::start_web_auth`: ASWebAuthenticationSession on Apple, not ephemeral, with results routed through `OAuthFlowRegistry`. Loopback on desktop when the descriptor asks for it.**

  **Binds:** FR-683, FR-698, FR-699; Story 82.5; UX-DR116.

  **Prevents:**
  - an embedded webview, which restricts passkeys to the app's own RP ID and breaks RFC 8252 §8.12;
  - a plugin whose only interface is a JavaScript command, which uses the deprecated initializer and panics without a window;
  - the PKCE verifier crossing the webview;
  - a second deep-link handler replacing the first;
  - a setup link that launched keeper and was lost;
  - an iOS setup link Launch Services cannot deliver;
  - an ephemeral session that turns every sign-in, and the forge leg, into a password prompt.

  **Rule:**
  - **The port.** `Platform` gains one method with a default that opens the system browser, so no test implementation breaks:

    ```rust
    fn start_web_auth(&self, url: &str, callback_scheme: &str) -> Result<(), CoreError>
    ```

    `OAuthFlowRegistry` may gain `cancel(state)`.
  - **macOS and iOS** (`web_auth.rs`, `web_auth_apple.rs`) use `objc2-authentication-services`:
    - `prefersEphemeralWebBrowserSession = false`;
    - `Callback::customScheme` behind `available!(macos = 14.4, ios = 17.4)`, with the legacy initializer otherwise;
    - the session, provider and block retained together;
    - the main thread, anchored on the main window;
    - cancel resolves the registry as `Cancelled`.

    The audited `#[allow(unsafe_code)]` functions are added to `docs/constraints-and-limitations.md`.
  - **Other desktops** use the default `open_url`.
  - **Loopback.** A loopback `redirect_uri` (`http://127.0.0.1/…`, ephemeral or pinned port) uses `org_account::loopback`: `std::net` only, bound to `127.0.0.1`, accepting one request on a `std::thread`, answering with a short "you can close this tab" page, then `flows.resolve(full_url)`. The default browser opens.
  - **Deep links** stay in the one handler:
    - voice links first;
    - then `keeper://setup?…` emits `keeper://account-setup` with the URL, and the webview opens the confirmation sheet;
    - then `keeper://oauth/<id>/…` and everything else go to `flows.resolve`.

    `deep_link().get_current()` is called once after setup and routed the same way.
  - **iOS:** `CFBundleURLTypes` for `keeper` in `gen/apple/project.yml` (the source of truth) and the Info plists as the repo's merge requires, plus `bundle.iOS.frameworks: ["AuthenticationServices"]` in `tauri.conf.json`.
  - **Redirect defaults:** `keeper://oauth/<id>/callback` and `keeper://oauth/<id>/forge/callback`. Any value may be configured (DW-294).

- **AD-312: The config repository. keeper-core plans, keeper-sync moves bytes, the shell joins them. Writes stay in one's own directory, are create-only, and are pushed through `push_http` on every platform.**

  **Binds:** FR-690, FR-691, FR-692, FR-693, FR-694, FR-697, FR-701; NFR-94, NFR-95; Story 82.3; D-25.

  **Prevents:**
  - a hidden drive leaking into drive lists, statuses or the tray;
  - the git shim, a credential helper or argv carrying the sign-in token;
  - keeper writing outside the signed-in person's directory;
  - keeper rewriting a file that exists, `user.toml` included;
  - a concurrent push from another person or device losing a write;
  - keeper-core depending on keeper-sync, or the reverse (AD-40).

  **Rule:**
  - **keeper-core `org_account::layout`** is pure and repo-agnostic, over `trait RepoFiles { read, list_dir }`:
    - `resolve(files, login, sub, issuer, identity_field) -> Resolution::{Missing, Mine, NotMine}`;
    - `plan(files, &PlanInput) -> Vec<PlannedWrite>`, create-only: `user.toml`, `<login>/keeper.toml` from `_template/keeper.toml`, `devices/<device>.toml`, and `keeper.<device>.toml` from `_template/class/<class>.toml` if present;
    - `is_own_path(login, rel)`, the guard every writer calls;
    - `device_slug`, `devices` and `plan_rename`.
  - **keeper-sync `config_repo`:**
    - `clone_or_fetch(spec, auth, interrupt)` is blocking, hard fast-forwards to `origin/<branch>`, and adopts an empty remote;
    - `commit_and_push(client, spec, auth, author, message, plan, interrupt)` makes at most three attempts: fetch and reset → `plan(worktree)` → an empty plan is `NothingToDo` → write → commit on the origin tip → `push_http`. A non-fast-forward retries;
    - `move_and_push` handles the device rename;
    - it refuses any `Write` whose path is absolute or contains `..`.
  - **`RepoAuth::{None, Basic { username, password }, Bearer}`.**
    - Basic answers gix's credential callback with the username and password as given (not `AccessToken::git`'s token-as-username spelling).
    - Bearer is an in-memory `http.extraHeader` on fetch and clone, and `Authorization: Bearer` in `push_http`, extended minimally so its existing callers stay byte-identical.
    - No helper, no argv, no userinfo in a URL.
  - **The shell's `plan` closure** calls `layout::plan` and filters with `is_own_path`.
  - **The clone** lives at `<data>/account/<id>/repo`. It is not a drive and appears in no drive list.

- **AD-313: A directory is the signed-in person's only when `user.toml` says so.**

  **Binds:** FR-689; Story 82.3; D-25.

  **Prevents:**
  - loading another person's settings because a username claim collided or was reassigned;
  - binding a directory to a username instead of to (`iss`, `sub`);
  - migrating an existing `user.toml` by rewriting it.

  **Rule:**
  - `resolve` reads `<login>/user.toml` and requires `user.toml[identity_field] == sub`. The default field is `sub`; a Zitadel repository may say `zitadel_id`. When `user.toml` also records `issuer`, it must equal the issuer.
  - **`NotMine`:** no account tiers load, the state is `Blocked`, and the sentence names the file ("This sign-in belongs to someone else: tgorka/user.toml records a different account. Settings from the repository were not loaded.").
  - **`Missing`:** the create plan runs.
  - keeper never rewrites `user.toml`.
  - The username comes from `username_claim` and must be a safe directory name (FR-686).

- **AD-314: Device name and class come from keeper, never from a token.**

  **Binds:** FR-690, FR-694, FR-700; Stories 82.3, 82.5.

  **Prevents:**
  - a device file named `unknown-host` on every phone;
  - an IdP claim deciding a device's name;
  - a device class that nothing in the tree can compute today;
  - a rename that leaves half of a device's files behind.

  **Rule:**
  - **The class.** `DeviceClass::{Desktop, Tablet, Mobile}`, exported to TypeScript as `DeviceClassVm`. macOS, Linux and Windows are `desktop`. iOS is `tablet` when `UIDevice.userInterfaceIdiom == .pad` and `mobile` otherwise.
  - **The name.**
    - Desktop: `read_host_label()` → `device_slug`, lower-case `[a-z0-9-]`, at most 32 characters, `device` when empty.
    - iOS: the model plus a 4-hex suffix (`iphone-3f2a`), persisted in the registry key `account.device_slug`.
  - **Editing.** The name is editable on the confirmation sheet until the device is registered. A later rename moves both of the device's files in one commit.
  - This name is the account's, not sync.db's commit label (`research-account-2026-09-23.md` §9.2 T3).

- **AD-315: The account as a credential is opt-in per drive and per bot provider. keeper-sync is unchanged, and the shell answers `secret_get`.**

  **Binds:** FR-706, FR-707; Story 82.7; UX-DR116.

  **Prevents:**
  - a drive or provider that switches to the account by itself;
  - one token silently widened to every service keeper talks to;
  - a second copy of the token in a drive's keychain slot;
  - keeper-sync learning about accounts;
  - a new token spelling hand-rolled outside `AccessToken`.

  **Rule:**
  - **Registry.** `sync.credential_source.<profile_id>` and `bots.provider_credential_source.<provider_id>` (families, `Scope::SessionState`, `Settable::Never`) hold `account`, or are absent, which means the keychain.
  - **Drives.** When a drive's source is `account`, the shell's `SyncPlatform::secret_get("sync/<pid>/credential")` answers with `access_token()`, bridged off the async runtime safely. No `sync/<pid>/credential` item is created.
  - **Bots.** Where keeper-core reads `bot_provider_token/{id}`, it consults the provider's source and sends `access_token()` as `Authorization: Bearer`.
  - **Refinement of spec §3.6 (contract A2c).** keeper-sync is unchanged, so a drive receives the token in keeper-sync's existing spellings:
    - git: the token as the Basic username with an empty password (`AccessToken::git`);
    - LFS: Basic `token:`;
    - the Forgejo API: `token <t>`.

    It does not use `config.auth`'s scheme and username. That shape works against Gitea, Forgejo and oauth2-proxy (`research-account-2026-09-23.md` §8.1, §8.4). GitLab documents only the password form. In `oauth` mode a drive receives the sign-in token, not the forge token (DW-295).

- **AD-316: A forge's API base is explicit, never derived.**

  **Binds:** FR-682, FR-692, FR-693; Stories 82.1, 82.3.

  **Prevents:**
  - `https://host/git/owner/repo.git` read as owner `git` at `https://host`;
  - the account's forge calls inheriting `forge_api_target`'s sub-path bug;
  - probing URL prefixes as a heuristic.

  **Rule:**
  - `config.api_base` is optional and, when present, is used as written. No code path fills it in from `config.url`, and `forge_api_target` is not called for the config repository.
  - `user_url` defaults to `{api_base}/user`.
  - `mode = "oauth"` with no way to learn the forge username (neither a forge `issuer` with `openid` in `scope`, nor `api_base`) is refused at parse time.
  - Drives keep `forge_api_target` unchanged (DW-293).

- **UX-DR116: An optional account.**

  **Binds:** AD-308, AD-309, AD-310, AD-311, AD-315; FR-683, FR-688, FR-694, FR-701, FR-703, FR-704, FR-705, FR-706, FR-707; Stories 82.6, 82.7.

  **Prevents:**
  - a Settings section that is disabled, or hidden behind a capability, when no account exists (AD-27's absent-not-disabled rule turned into "nothing to explain");
  - three setup paths with three different confirmations;
  - a host truncated out of sight on the only screen that says where a sign-in goes;
  - a toast or a modal for being offline;
  - a credential field disabled rather than removed;
  - a rephrased Rust sentence.

  **Rule** (the design lane's text, verbatim; only the list format is added):
  1. Settings › Account is the FIRST section of Settings on every tier and rides no capability gate. With no account it shows exactly: the heading "Account", one sentence ("keeper works fully without an account. An organisation account signs you in once and brings your settings to every device you use."), and one field "Paste a setup link" with "Continue" — no disabled controls, no empty lists.
  2. Link, QR and paste — from Settings, the first-run wizard or a keeper://setup deep link — all land on ONE setup confirmation Sheet: account name, sign-in host and repository host (both in mono, never truncated), the device name (editable until this device is registered) and its class. Nothing is written before Continue. Progress and the outcome are Rust's sentence rendered verbatim in the same Sheet; Cancel during sign-in cancels the browser round trip.
  3. Signed in: identity row (display name, login in mono, sign-in host), roles as outline chips, Rust's status sentence (role=status), "Sync now", the device list with this device marked and an inline "Rename" disclosure (never a dialog), "Add a device or a person" (a Sheet with the setup link as a QR on the mandatory white card ≥ 240 px, the link in mono, and "Copy link"), and "Sign out…" in an AlertDialog whose sentence says the files in the repository are kept.
  4. Account status line: one line beside the sync status (sidebar footer, with the offline pill), shown ONLY for offline, sign in again and blocked, in the offline pill's amber held tokens; never a toast, never modal.
  5. Credential choice: the drive form and the bot endpoint form offer "Use my {Account name} account" only while an account is signed in and usable (or already chosen for that row); choosing it hides the token/key field rather than disabling it; nothing switches over by itself.
  6. First run: Welcome offers "Sign in with an organisation account" as a secondary action; that optional step holds the same paste field and a Skip that continues to Add account; a setup link that launches keeper opens that step with the Sheet over it. The other steps' order and copy are unchanged.
  7. Copy: sentence case, no exclamation marks, no "please"; refusal/offline/blocked sentences are Rust's and never rephrased in TypeScript.
  8. "Forget this account…" sits beside "Sign out…" in Settings › Account whenever an account is configured, signed in or signed out. It opens a destructive AlertDialog whose sentence names what goes and what stays: keeper signs out, deletes this device's copy of the account's settings and its account.toml, and stops applying them; the repository on the server, and every other device and person, are untouched. Signing out keeps the account set up for the next sign-in; forgetting returns Settings › Account to the paste field.

**Also in this epic, without an AD**, as the coordinator ruled:
- *Forget this account* (contract A2b; FR-701): the shell command `account_forget()` and clause 8 above.
- The account's hosts in About's egress list (contract A2a; FR-684).
- The "Set by a file" badge naming the repository file (`acme: tgorka/keeper.toml`) through CoreLayers' phrases, with the account tiers' clause that settings read at startup follow at the next launch (FR-696; the coordinator assigned it to CoreLayers on 2026-09-23).

## Contract amendments

The coordinator amended the frozen contract twice during the build wave. They are written here because the code is built to them, and a reader of the spec alone would be told the wrong thing. Where the spec and an amendment disagree, the amendment wins.

- **A1: The module is `keeper_core::org_account`, not `keeper_core::account`.**
  - `keeper_core::account` already exists: the Matrix `AccountManager`, `keeper-core/src/account.rs`.
  - Every contract path `account::X` reads `org_account::X`, in the files `keeper-core/src/org_account/{mod,descriptor,claims,layout,state,oidc,session,loopback}.rs` with `pub mod org_account;` in `lib.rs`.
  - The status VM is exported to TypeScript as `OrgAccountVm` (`#[ts(export, rename = "OrgAccountVm")]`). `AccountStateVm`, `AccountIdentityVm`, `AccountDeviceVm`, `AccountSetupVm`, `AccountShareVm` and `DeviceClassVm` keep their names.
  - Unchanged: the IPC command names (`account_*`), the keychain keys (`account/<id>/…`), the registry keys, and the UI word "Account".
- **A2: Egress, *Forget this account*, and the drive's spelling.**
  - (a) CoreDescriptor adds the account's destinations to the egress model (`keeper-core/src/egress.rs:162` `compute_egress`, `keeper-core/src/vm.rs:1550` `EgressKind`, `docs/egress.md`, and any test pinning the list). They are derived from the descriptor, disclosed only while one is configured, and the list is unchanged without one.
  - (b) The shell adds `account_forget() -> AccountVm`. It signs out if signed in, deletes `account.toml` and the local clone cache (`<data>/account/<id>`), and clears the layers. The remote repository is never touched. The frontend adds UX-DR116's clause 8.
  - (c) A drive set to "Use my account" sends the sign-in access token in keeper-sync's existing spelling (`AccessToken::git` and the others), not `config.auth`'s scheme. keeper-sync stays unchanged (AD-315).

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-681 | keeper reads the account from one descriptor: `~/.keeper/account.toml` on desktop, or `account.toml` in the app's data directory on iOS. It is TOML there and JSON when an operator serves it, with one schema. Omitted fields take their defaults: `scopes = ["openid", "profile", "email", "offline_access"]`, `username_claim = "preferred_username"`, `redirect_uri = keeper://oauth/<id>/callback`, `config.branch = "main"`, `config.identity_field = "sub"`, and `config.auth` = `mode = "same"`, `scheme = "basic"`, `username = "oauth2"`. A missing file means no account and no fault. A malformed file, or one that fails a rule, means no account, with the reason in Settings' fault list under `account.toml`. | 82.1 | AD-308 |
| FR-682 | A descriptor is refused, with a sentence, when: it carries a client secret anywhere ("keeper is a public client; remove the secret"); `issuer`, `config.url`, `config.api_base` or an endpoint is not `https` and not a loopback host; `id` does not match `[a-z0-9-]{1,32}`; or `mode = "oauth"` gives keeper no way to learn the forge username. `api_base` is never derived from `url`. | 82.1 | AD-308, AD-316 |
| FR-683 | Three inputs reach the same confirmation: a `keeper://setup?descriptor=<https URL>` link, a `keeper://setup?d=<base64url JSON>` link, or either link or a bare `https://…` URL pasted into Settings or the first-run step. A URL is fetched over HTTPS only, with no redirects and at most 64 KiB. The confirmation shows the account's name, the sign-in host, the repository host, and this device's name and class. Nothing is written before *Continue*, and *Cancel* writes nothing. | 82.1, 82.5, 82.6 | AD-308, AD-311, UX-DR116 |
| FR-684 | While a descriptor is configured, Settings › About's egress list also names its sign-in host, the host the descriptor was fetched from (when known), the config repository's host, and in `oauth` mode the forge's host. Each is a host, never a URL. With no descriptor the list is exactly what it was. | 82.1 | AD-308 (AD-53 extended) |
| FR-685 | Every sign-in attempt uses a new `state`, `nonce` and PKCE S256 verifier, each consumed once. A callback whose `state` or `nonce` does not match is refused. The ID token must: come from the configured issuer, byte for byte; name the `client_id` in `aud`, with no other audience beyond `trusted_audiences`; carry `azp` equal to the `client_id` when present; use an allowed algorithm (RS/PS 256–512, ES256, ES384, EdDSA); pass `exp` and `iat` with 60 s of leeway; and match `at_hash` when present. An unknown `kid` refetches the keys once. UserInfo is used only when its `sub` equals the ID token's. An abandoned sign-in ends after 300 s, and *Cancel* ends it at once, as cancelled rather than failed. | 82.2 | AD-310 |
| FR-686 | The username comes from `username_claim`, and must be a safe directory name: `[A-Za-z0-9._-]`, no leading `.` or `_`, no `/`, at most 64 characters. Otherwise sign-in is refused with a sentence. Roles come from `roles_claim` in the verified ID token, or else in UserInfo, and never from the access token. The claim may be an array of strings or an object whose keys are roles. An exact top-level name wins over a dotted path such as `realm_access.roles`. When `required_role` is set and missing, sign-in is refused with "Your {name} account does not have the {role} role. Ask your administrator." | 82.2 | AD-310 |
| FR-687 | A signed-in account keeps its tokens in exactly one keychain item, `account/<id>/session`, plus `account/<id>/forge` in `oauth` mode. On iOS they are this-device-only and never synchronised. A caller always gets a valid access token: refreshed 60 s before expiry, by one refresh at a time however many callers ask, with a rotated refresh token stored before the new access token is used. A dead grant is reported as *sign in again*, and an unreachable network as *offline*. The two are never confused. | 82.2 | AD-310 |
| FR-688 | *Sign out…* revokes the refresh token(s) where the provider has a revocation endpoint, deletes both keychain items whatever the network does, and opens the provider's end-session page with `id_token_hint` in the same browser session. The clone and the descriptor stay. The account's settings stop applying, and signing in again needs no link. | 82.2, 82.5, 82.6 | AD-310, UX-DR116 |
| FR-689 | keeper applies settings from `<login>/` only when `<login>/user.toml`'s identity field (`identity_field`, default `sub`) equals the sign-in's `sub`, and, when the file records `issuer`, the issuer too. On a mismatch no account settings load, the account is *blocked*, and the sentence names the file. keeper never rewrites an existing `user.toml`. | 82.3 | AD-313 |
| FR-690 | On a first sign-in with no `<login>/`, keeper writes `<login>/user.toml` (login, display name, the identity field, issuer, created) and copies `_template/keeper.toml` to `<login>/keeper.toml`. On a device not yet registered it writes `<login>/devices/<device>.toml` (name, class, platform, created) and, when `_template/class/<class>.toml` exists, copies it to `<login>/keeper.<device>.toml`. It commits and pushes. A file that exists is never re-copied or rewritten, and a second launch writes nothing. When another push lands first, keeper fetches, re-plans onto the new tip, skips what now exists, and retries up to three times. | 82.3 | AD-312, AD-314 |
| FR-691 | keeper writes and stages only paths under the signed-in person's own `<login>/`. Any other path, and any absolute path or path containing `..`, is refused before it reaches the index. | 82.3 | AD-312 |
| FR-692 | The config repository is cloned, fetched and pushed with the account's credential. In `same` mode that is the sign-in access token as the Basic password with `username` (default `oauth2`), or as `Authorization: Bearer` with `scheme = "bearer"`. In `oauth` mode it is the forge's token. Push uses keeper's own smart-HTTP push on every platform. No credential helper, argv or URL userinfo ever carries it. The clone lives in keeper's data directory at `account/<id>/repo`, and is not a drive: it is in no drive list, status or tray line. The forge API base is `api_base` as written, never derived. | 82.3 | AD-312, AD-316 |
| FR-693 | In `oauth` mode, keeper runs a second PKCE sign-in against the forge in the same browser session. When `signin_url` is set, keeper opens it with `{authorize_path_and_query}` filled in. The `scope` string is sent byte-identical on every device and version. The forge's username (from its ID token's `preferred_username`, or from `user_url`/`username_field`) must equal the sign-in username. Otherwise the connection is refused with "The forge signed you in as {forge}, but your keeper account is {login}.", and its tokens are discarded. A `401` refreshes once and retries. A second `401`, or a `403`, asks the person to reconnect the repository. | 82.2, 82.3 | AD-310, AD-312, AD-316 |
| FR-694 | Renaming this device moves `<login>/devices/<old>.toml` and `<login>/keeper.<old>.toml` to the new name in one commit. The device list shows every file in `<login>/devices/`, with this device marked. | 82.3, 82.5, 82.6 | AD-312, AD-314, UX-DR116 |
| FR-695 | `<login>/keeper.toml` (every device) and `<login>/keeper.<device>.toml` (this device) are layer files. They use the same format as `~/.keeper/keeper.toml` and the same per-key faults, and the rest of a file still applies when one key is refused. They win over `~/.keeper/keeper.toml` and `~/.keeper/keeper.<host>.toml`, and lose to the main sync folder's files and to per-folder files. `mainSyncFolder` in either file is refused with the existing "only `~/.keeper/` may elect the main folder" fault. `[folder]` is refused. A machine-local key is accepted only in the device file. | 82.4 | AD-309 |
| FR-696 | A sync that changes the person's files takes effect without a restart for every key read on each use. A key read only at launch (hotkeys, the debug log) takes effect at the next launch. The "Set by a file" badge names the account and the repository file (`acme: tgorka/keeper.toml`), and for an account tier its phrase ends by saying that settings read at startup follow at the next launch. The fault list shows account-file faults beside the others. | 82.4, 82.6 | AD-309 |
| FR-697 | At launch keeper applies the account's settings from the last clone on disk before anything reads them, with no network. An unreachable fetch keeps them applied, and the status reads *Offline — using settings from {time}*. Nothing about an unreachable network deletes, rewrites or disables a local setting. When the issuer cannot be reached at sign-in, keeper says so and stays local-only. | 82.4, 82.5 | AD-309, AD-312 |
| FR-698 | On macOS and iOS, sign-in opens in the system's web authentication session, sharing the default browser's sign-in. There the identity provider's passkeys, iCloud Keychain and 1Password work with no keeper code. *Cancel* in the session, or in keeper, ends the attempt as cancelled. A descriptor with a loopback `redirect_uri` signs in through the default browser and a one-request listener on `127.0.0.1`. Other desktops open the default browser and receive the callback by deep link. | 82.5 | AD-311 |
| FR-699 | A `keeper://setup?…` link opens the setup confirmation on macOS and iOS, whether keeper was running or the link launched it. `keeper://oauth/<id>/…` callbacks reach the sign-in that is waiting for them. Voice links and Matrix's `keeper://oauth/callback` behave as before. | 82.5 | AD-311 |
| FR-700 | A new device is named by keeper: on desktop, the short hostname lower-cased to `[a-z0-9-]`; on iOS, the model plus a 4-character suffix (`iphone-3f2a`), kept across launches. Its class is `desktop` on macOS, Linux and Windows, and on iOS `tablet` on an iPad and `mobile` otherwise. The name is editable on the confirmation until the device is registered. No token claim names a device. | 82.5 | AD-314 |
| FR-701 | *Forget this account…* is available whenever an account is configured, signed in or not. It signs out when signed in, deletes `account.toml` and this device's clone of the repository, and stops applying the account's settings. It never contacts or changes the repository on the server, or any other device. Afterwards Settings › Account shows the paste field again. | 82.5, 82.6 | AD-308, AD-312, UX-DR116 |
| FR-702 | The config repository is synced at launch (after the offline install of FR-697), when keeper's window gains focus (at most once every 15 minutes), and on *Sync now*, which is not throttled. No timer runs in Rust. | 82.5, 82.6 | AD-309 |
| FR-703 | Settings › Account is the first section on every tier and is never gated. Signed out, it explains that keeper works fully without an account and offers one paste field. Signed in, it shows: the identity (display name, login, sign-in host); the roles; one status sentence (up to date / offline since… / sign in again / blocked: …); *Sync now*; the device list with *Rename*; *Add a device or a person*, a QR code of the setup link with *Copy link*; *Sign out…*; and *Forget this account…*. | 82.6 | UX-DR116 |
| FR-704 | The first-run wizard offers *Sign in with an organisation account* as an optional step, with the same paste field and a Skip. A setup link that launched keeper opens that step with the confirmation over it. Skipping leaves today's steps, their order and their copy unchanged. | 82.6 | UX-DR116 |
| FR-705 | One account status line sits beside the sync status and appears only when the account is offline, needs a new sign-in, or is blocked. Being offline never raises a toast or a modal. | 82.6 | UX-DR116 |
| FR-706 | While an account is signed in and usable, the drive form offers *Use my {name} account*. Choosing it removes the token field, and the drive then asks for the account's current token on each operation. No per-drive keychain item is created. The token is sent the way keeper already sends a drive token (AD-315). Nothing switches to the account by itself, and choosing the keychain again brings the token field back. | 82.7 | AD-315, UX-DR116 |
| FR-707 | While an account is signed in and usable, a bot provider's form offers the same choice. A provider set to it sends `Authorization: Bearer {access token}`. It is opt-in per provider. | 82.7 | AD-315, UX-DR116 |
| NFR-92 | **No account, no change.** With no `account.toml`: no new file read can fault, no network request is made, no UI appears beyond the Account section's signed-out state, the egress list is byte-identical, and every existing test passes unchanged. | 82.1–82.7 | AD-308, AD-309 |
| NFR-93 | **Secrets.** Tokens exist only in the OS keychain: never in IPC, a log line, a file, argv, a URL, the config repository or a VM. A signed-in account costs one keychain item (two in `oauth` mode), which means one macOS ACL prompt per launch per item at most, behind the existing `SecretCache`. On iOS the items are this-device-only and not synchronisable. A descriptor carries no secret. | 82.2, 82.3, 82.5, 82.7 | AD-310, AD-312, AD-315 |
| NFR-94 | **Cost.** The account tiers add one `RwLock` read and one map lookup to `setting_override`, and no disk read. Boot reads `account.toml` and at most two small files from the clone, with no network before the first settings read. A clone, fetch or push never runs on the main thread or under the layer lock. Concurrent `access_token()` callers make one refresh. A focus-triggered sync runs at most once per 15 minutes. The descriptor fetch reads at most 64 KiB. | 82.2, 82.3, 82.4, 82.5 | AD-309, AD-310, AD-312 |
| NFR-95 | **Boundaries.** keeper-core depends on neither keeper-sync nor gix, and keeper-sync does not depend on keeper-core (`bun run check:core-sync-free`, `check:syncd-lean`). `openidconnect` is built without default features, so the tree gains no second reqwest and no second TLS stack. `cargo deny check` passes on licences, bans and sources. The shell's new unsafe FFI is function-level, audited, and listed in `docs/constraints-and-limitations.md`. | 82.2, 82.3, 82.5 | AD-310, AD-311, AD-312 |

## Stories

Every story names its rung in the three-rung stack: `epic82-plan` → `epic82-core` (82.1–82.4, plus SyncTransport's keeper-sync work and 82.7's keeper-core half) → `epic82-shell-surface` (82.5–82.7).
- **Each rung must pass CI alone.** `bun run lint`, `bindings:check`, `check:core-sync-free` and `check:syncd-lean` are part of the gate.
- **The shell is by inspection.** Everything under `src-tauri/crates/keeper/**` awaits CI's macOS job, because the shell crate does not build on this host.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated with `cargo test -p keeper-core` and never hand-edited.
- **Every core regression test named below is mutation-proved.** Mutate, run, restore, and verify the restore by diff (the contract's rules).

**Task acceptance 5** is the required test set:
- descriptor parsing and defaults (`auth.mode = same`);
- PKCE/state/nonce (state mismatch refused, nonce mismatch refused, verifier sent);
- both roles-claim shapes;
- directory and device resolution (Mine/Missing/NotMine, including a configured identity field);
- copy-on-create-only (the second plan is empty; existing files untouched);
- the own-dir-only guard;
- the forge API base never derived from the remote URL.

Each is placed in the story that owns it.

**Task acceptance 2** (fresh macOS and iOS installs, a 1Password passkey, both modes) is a device check on hesperia and an iPhone. It is owed until run.

Several files carry hunks for both code rungs, or for two lanes in one rung. The coordinator splits them at stack time:

| File | `epic82-core` | `epic82-shell-surface` |
| --- | --- | --- |
| `keeper-core/src/config/keys.rs` | the KeySpec rows `account.last_synced_ms`, `account.device_slug`, `sync.credential_source.`, `bots.provider_credential_source.` (CoreSession); `GENERATED_HEADER` text (CoreLayers) | none |
| `keeper-core/src/vm.rs` | `ConfigTierVm` and its phrases (CoreLayers); `EgressKind`'s account variant(s) (CoreDescriptor, A2a). Two lanes in one file: anchored edits, re-read before each. | none |
| `keeper-core/src/lib.rs` | `pub mod org_account;` (CoreDescriptor) | none |
| `keeper-core/src/bots/**` | the `bot_provider_token/{id}` read site consults the source (CoreSession, for 82.7) | none |
| `src/lib/ipc/gen/*.ts` | generated: `OrgAccountVm`, `AccountStateVm`, `AccountIdentityVm`, `AccountDeviceVm`, `AccountSetupVm`, `AccountShareVm`, `DeviceClassVm`, `ConfigTierVm`, `EgressKind` | consumed |
| `docs/settings-keys.md`, `docs/egress.md` | regenerated / extended | none |
| `src/lib/ipc/client.ts`, `dev/mock-shell.ts` | none | every `account_*` wrapper and the credential-source wrappers; signed-out mock answers |

`bindings:check` must be green on `epic82-core` alone: the generated types land there with no consumer yet.

### 82.1 — A setup link names the account
**Intent:** "dostep bedzie juz za pierwszym uruchomieniem"; "onboarding nowych uzytkownikow przez qrcode". Reach the account from the first launch through one input, and say where it leads before anything is written. **Rung:** **epic82-core** (lane CoreDescriptor). AD-308, AD-316.
**Files:**
- `src-tauri/crates/keeper-core/src/org_account/mod.rs`: the module lines and `AccountError::{NeedsSignIn, Unreachable, Refused, Cancelled, Internal}`.
- `org_account/descriptor.rs`:
  - `AccountDescriptor`, `AuthConfig`, `Endpoints`, `RepoConfig`, `RepoAuthConfig` (serde tag `mode`, default `Same { Basic, "oauth2" }`), `GitScheme` and `ForgeOauth`;
  - `redirect_uri()`, `forge_redirect_uri()`;
  - `parse_json`, `parse_toml`, `to_toml`, `validate`;
  - `SetupInput`, `parse_setup_input`, `setup_link`;
  - `fetch` (https-only, no redirects, 64 KiB);
  - `FILE_NAME`, `load` (faults as `LayerTier::AccountDescriptor`) and `store` (atomic).
- `org_account/state.rs`:
  - `AccountStateVm::{None, SignedOut, SigningIn, Syncing, Ready, Offline, NeedsSignIn, Blocked}`;
  - `AccountVm` (exported as `OrgAccountVm`, A1), `AccountIdentityVm`, `AccountDeviceVm`, `AccountSetupVm`, `AccountShareVm`;
  - `AccountFacts` and `vm(&AccountFacts) -> AccountVm`, which composes the state and Rust's sentence.
- `keeper-core/src/lib.rs`: `pub mod org_account;` only.
- Egress (A2a): `keeper-core/src/egress.rs` (`compute_egress`), `keeper-core/src/vm.rs` (`EgressKind`), `docs/egress.md`, and any test pinning the destination list.
- Generated: `src/lib/ipc/gen/{OrgAccountVm,AccountStateVm,AccountIdentityVm,AccountDeviceVm,AccountSetupVm,AccountShareVm}.ts` and `EgressKind.ts`.

**Acceptance:**
- *Parsing and defaults* (task acceptance 5, mutation-proved): spec §2.5's descriptor B, with `config.auth` omitted, parses to `mode = same`, `scheme = basic`, `username = "oauth2"`, `branch = "main"`, `identity_field = "sub"`, the default scopes, `username_claim = "preferred_username"` and `redirect_uri() == "keeper://oauth/acme/callback"`. Descriptor A parses to `oauth` mode with its `signin_url` and `forge_redirect_uri() == "keeper://oauth/acme/forge/callback"`. A TOML round trip (`to_toml` → `parse_toml`) is identity. Mutation: a default of `none` for `mode` turns it red.
- *Refusals*, one case each, and each carries a sentence:
  - `client_secret` in `auth` or in `config.auth`;
  - an `http://` issuer on a non-loopback host, while `http://127.0.0.1` is accepted;
  - an `id` of `Acme` or 33 characters;
  - `mode = "oauth"` with neither a forge issuer carrying `openid` nor an `api_base`.
- *`api_base` is never derived* (task acceptance 5): a descriptor with `url = https://git.acme.dev/git/people/keeper-config.git` and no `api_base` has `api_base == None` after `parse`/`validate`/`to_toml`/`parse_toml`. Mutation: filling it from `url` turns it red.
- *Setup input*: `keeper://setup?descriptor=https%3A%2F%2Fid.acme.dev%2F.well-known%2Fkeeper-account.json` gives `Url`; `?d=<base64url>` gives `Inline` equal to the source descriptor; a bare `https://…json` gives `Url`; `http://`, another scheme or another host path (`keeper://oauth/…`) is refused. `setup_link` round-trips through `parse_setup_input` in both forms.
- *Store*: `load` of an absent path is `Ok(None)`; of a malformed file, a fault with tier `AccountDescriptor`; `store` then `load` is identity.
- *State*: `vm()` gives the sentences named in the contract for Ready, Offline (with a time), NeedsSignIn, Blocked (naming `<login>/user.toml`) and a missing role.
- *Egress* (FR-684): the list computed with no descriptor is byte-identical to today's. With descriptor A it gains `id.acme.dev` and `git.acme.dev` as hosts, never as URLs. Mutation: emitting a host with no descriptor turns it red.

**binds:** FR-681, FR-682, FR-683, FR-684, NFR-92, AD-308, AD-316, AD-53 (extended)

### 82.2 — keeper signs you in itself
**Intent:** "logowanie za pomoca oauth". Sign a person in with their organisation's provider, keep one credential, and let everything else ask for it. **Rung:** **epic82-core** (lanes CoreSession; claims.rs by CoreDescriptor). AD-310.
**Files:**
- `org_account/oidc.rs`:
  - `Identity`;
  - `sign_in(platform, flows, http, d)`;
  - discovery with overrides;
  - state/nonce/PKCE;
  - the ROidc §2 checklist, including `azp`, allowed algorithms, 60 s leeway, `at_hash`, one JWKS refetch, and the UserInfo `sub` check.
- `org_account/session.rs`:
  - `access_token` (single-flight, 60 s early, persist before use);
  - `forge_connect`, `forge_token`;
  - `identity`, `sign_out`;
  - `git_credential` → `GitAuth::{None, Basic, Bearer}`.
- `org_account/loopback.rs`: `std::net` only. Bind `127.0.0.1:<port or 0>`, accept one request on a `std::thread`, answer a short page, `flows.resolve(url)`.
- `org_account/claims.rs` (CoreDescriptor): `username`, `roles` (array or object keys; exact name, then dotted path; sorted, deduped), `require_role`.
- `keeper-core/src/platform.rs`: `start_web_auth`, with a default that calls `open_url`.
- `keeper-core/src/oauth.rs`: optional `cancel(state)`.
- `keeper-core/Cargo.toml` and `src-tauri/Cargo.toml`: `openidconnect = { version = "4.0.1", default-features = false }` and `oauth2-reqwest = "=0.1.0-alpha.3"`.
- `keeper-core/src/config/keys.rs`: KeySpec rows only.
- `keeper-core/src/registry.rs`: getters and setters for `account.last_synced_ms` and `account.device_slug`, plus 82.7's two families.

**Acceptance:**
- *PKCE, state, nonce* (task acceptance 5, mutation-proved), against a local test provider on a loopback issuer:
  - a callback with another `state` is refused and consumes nothing;
  - an ID token whose `nonce` differs is refused;
  - the token request carries the `code_verifier` whose S256 is the `code_challenge` sent;
  - two attempts use two different verifiers.

  Mutations: skipping the state compare, dropping the nonce, and sending no verifier each turn a case red.
- *Both role shapes* (task acceptance 5, mutation-proved):
  - `{"groups": ["keeper", "ops"]}` with `roles_claim = "groups"` gives `["keeper", "ops"]`;
  - Zitadel's `{"urn:zitadel:iam:org:project:283746519283746001:roles": {"keeper": {"1": "acme.dev"}}}` gives `["keeper"]`;
  - `{"realm_access": {"roles": ["keeper"]}}` with `realm_access.roles` gives `["keeper"]`;
  - a top-level claim literally named `realm_access.roles` wins over the path.

  `require_role` with the role missing gives `Refused` with the account's name in the sentence.
- *Username*: `tgorka` is accepted. `.hidden`, `_template`, `a/b`, a 65-character name and a missing claim are each refused.
- *Audience*: an ID token with `aud = [client_id, "283746519283746001"]` passes only when that value is in `trusted_audiences`.
- *Refresh*:
  - concurrent `access_token()` calls on an expired session make one token request;
  - the rotated refresh token is in the keychain fake before the access token is returned;
  - a `400 invalid_grant` gives `NeedsSignIn`;
  - a connection refused gives `Unreachable`.
- *Sign-out*: both keychain items are gone even when the revocation request fails.
- `cargo deny check` passes, and `cargo tree -p keeper-core` names neither `gix` nor `keeper-sync`, and one `reqwest`.

**binds:** FR-685, FR-686, FR-687, FR-688, FR-693, NFR-93, NFR-95, AD-310

### 82.3 — Your directory in the config repository
**Intent:** "uzywanie konfiguracji (sciaganie lub tworzenie) na remote - git jak drive". Fetch the person's settings, or create them there, without ever touching anyone else's. **Rung:** **epic82-core** (lanes CoreDescriptor for `layout.rs`, SyncTransport for keeper-sync). The shell's join lands in 82.5. AD-312, AD-313, AD-314, AD-316; D-25.
**Files:**
- `org_account/layout.rs` (CoreDescriptor):
  - `RepoFiles`, `DeviceClass` (exported as `DeviceClassVm`), `UserRecord`;
  - `Resolution` and `resolve`;
  - `PlanInput`, `PlannedWrite`, `plan`;
  - `is_own_path`, `device_slug`, `DeviceEntry`, `devices`;
  - `plan_rename`, `RenameOp`.
- `keeper-sync/src/config_repo.rs` (SyncTransport):
  - `RepoAuth`, `RepoSpec`, `SyncOutcome`, `clone_or_fetch`;
  - `Write`, `Author`, `PushResult`, `commit_and_push`, `move_and_push`.
- `keeper-sync/src/lib.rs`: the mod line.
- `keeper-sync/src/git/push_http.rs`: a minimal auth extension (for example an `HttpAuth` enum) that keeps existing callers byte-identical.
- `keeper-sync/Cargo.toml`: only if a feature is needed.

**Acceptance:**
- *Resolution* (task acceptance 5, mutation-proved), over an in-memory `RepoFiles`:
  - no `tgorka/` gives `Missing`;
  - `user.toml` with `sub = "S"` and sub `S` gives `Mine { display_name }`;
  - `sub = "T"` gives `NotMine { recorded: Some("T") }`;
  - `identity_field = "zitadel_id"` reads `zitadel_id` and ignores `sub`;
  - a recorded `issuer` that differs gives `NotMine`.

  Mutation: comparing the username instead of the identity field turns it red.
- *Create-only* (task acceptance 5, mutation-proved):
  - over a template-only repo, the first `plan` yields `tgorka/user.toml`, `tgorka/keeper.toml` (the template's bytes), `tgorka/devices/macbook.toml` and, with `_template/class/desktop.toml` present, `tgorka/keeper.macbook.toml`;
  - over the result, a second `plan` is **empty**;
  - over a repo where `tgorka/keeper.toml` already holds other bytes, `plan` does not name it;
  - with no class template, no `keeper.<device>.toml` is planned.
- *Own-dir-only* (task acceptance 5, mutation-proved):
  - `is_own_path("tgorka", "tgorka/keeper.toml")` holds;
  - it fails for `ana/keeper.toml`, `tgorka/../ana/x`, `/etc/x`, `_template/keeper.toml` and `tgorka` (the directory itself is not a file);
  - `commit_and_push` refuses a `Write` whose path is absolute or contains `..`, even when the closure returns it.
- *Devices*: `device_slug("MacBook Pro.local")` is `macbook-pro-local`, with runs collapsed, the ends trimmed, at most 32 characters, and `device` when empty. `plan_rename` names exactly the two files of the old device and refuses a target that exists.
- *Transport*, against a local bare repository (`file://` for fetch, the push_http test harness where one exists):
  - clone adopts an empty remote;
  - `commit_and_push` with a plan returning nothing is `NothingToDo`;
  - a racing push is retried onto the new tip;
  - the Basic header carries `username:password` as given;
  - Bearer is an `Authorization: Bearer` header;
  - no test observes a helper, argv or userinfo.
- *No derivation* (task acceptance 5): the config repo never calls `forge_api_target`. A grep guard over `org_account/` and `config_repo.rs` for `forge_api_target` returns nothing, and the descriptor test in 82.1 pins that the field stays `None`.

**binds:** FR-689, FR-690, FR-691, FR-692, FR-693, FR-694, NFR-93, NFR-94, NFR-95, AD-312, AD-313, AD-314, AD-316, D-25

### 82.4 — The account's files are layers
**Intent:** "sync konfiguracji". The person's two files are ordinary layer files, live, winning over this machine's hand edits, with yesterday's copy offline. **Rung:** **epic82-core** (lane CoreLayers). AD-309.
**Files:**
- `keeper-core/src/config/mod.rs`:
  - `LayerTier::{AccountShared, AccountDevice, AccountDescriptor}` and `ORDER`;
  - the predicates and labels;
  - `AccountLayerSource`, `load_account_layers`, `install_account_layers`, `set_account_faults`;
  - `setting_override` precedence.
- `keeper-core/src/vm.rs`: `ConfigTierVm::{AccountShared, AccountDevice, Account}`, phrases and tests only. The account tiers' phrase, which is the "Set by a file" tooltip's source, ends with a clause saying that settings read at startup follow at the next launch (assigned to CoreLayers by the coordinator, 2026-09-23).
- `keeper-core/src/config/keys.rs`: the `GENERATED_HEADER` text only.
- `docs/settings-keys.md` (regenerated) and the generated `ConfigTierVm.ts`.

**Acceptance:**
- *Precedence* (mutation-proved): one key set in `UserGlobal`, `UserGlobalMachine`, `AccountShared`, `AccountDevice`, `MainShared` and `MainMachine`. Removing them from the top down, the answer walks MainMachine → MainShared → AccountDevice → AccountShared → UserGlobalMachine → UserGlobal. Mutation: placing the account tiers below `~/.keeper` turns it red.
- *Refusals*:
  - `mainSyncFolder` in `keeper.toml` or `keeper.<device>.toml` of the account gives the existing main-folder fault, and the file's other keys still apply;
  - `[folder]` gives the existing fault;
  - an unknown key gives a per-key fault;
  - a `MachineFileOnly` key is refused in `keeper.toml` and accepted in `keeper.<device>.toml`.
- *Live*: `install_account_layers(Some(a))` then `Some(b)` changes `setting_override` without touching the frozen stack. `None` clears. Faults set by `set_account_faults` appear in `ConfigLayersVm`, and a later clean install removes them.
- *Nothing without an account* (NFR-92): with no account layers installed, every existing config test and the phrase-pinning test pass unchanged, apart from the added tiers' rows.
- *Phrases*: the override's source reads the account name and `tgorka/keeper.toml`, and ends with the next-launch clause for settings read at startup. The phrase of a `~/.keeper` or main-folder tier does not gain that clause. A descriptor fault reads `account.toml`.
- `docs/settings-keys.md` regenerated, and its pin test green.

**binds:** FR-695, FR-696, FR-697, NFR-92, NFR-94, AD-309, AD-101 (held)

### 82.5 — The shell carries the sign-in
**Intent:** "dostep bedzie juz za pierwszym uruchomieniem" on a phone as much as on a Mac: the browser session, the links, a link that launches keeper, and the device. **Rung:** **epic82-shell-surface** (lane Shell). **Everything here is by inspection, awaiting CI's macOS job.** AD-311, AD-314, AD-312's join, AD-315's bridge.
**Files:**
- `src-tauri/crates/keeper/src/account_ipc.rs`. The IPC commands (camelCase args, registered on every target):
  - `account_state`, `account_subscribe`/`account_unsubscribe`;
  - `account_setup_resolve(input)` → `AccountSetupVm`;
  - `account_setup_confirm(setup_id, device_name)`: writes `account.toml`, then sign-in → forge connect (oauth) → repo sync → layers install, with progress through the subscription;
  - `account_sign_in`, `account_cancel_sign_in`;
  - `account_sync(force)`, throttled to 15 minutes unless forced, with no Rust timer;
  - `account_rename_device(name)`;
  - `account_share()` → `AccountShareVm`, the setup link plus `qr_svg`;
  - `account_sign_out()`: keeps the clone, clears the layers, opens the end-session URL when returned;
  - `account_forget()` (A2b);
  - `sync_credential_source_get/set` and `bots_provider_credential_source_get/set` (82.7).
- The join: `plan` closures over `org_account::layout::plan` filtered by `is_own_path`; `RepoAuth` from `session::git_credential`; the blocking keeper-sync calls off the async runtime.
- `web_auth.rs` and `web_auth_apple.rs`: `Platform::start_web_auth` for macOS and iOS per AD-311.
- `lib.rs`:
  - registration;
  - the setup arm in the one deep-link handler (`voice_reach.rs`), emitting `keeper://account-setup`;
  - `keeper://oauth/<id>/…` → `flows.resolve`;
  - `deep_link().get_current()` once after setup;
  - the boot install of the account tiers from the clone right after `config::install`, then a background `account_sync(false)`.
- Device: `device_class()` (desktop; iOS idiom through `objc2-ui-kit`) and the default name (desktop `read_host_label` → `device_slug`; iOS model + 4-hex suffix persisted in `account.device_slug`).
- `SyncPlatform::secret_get("sync/<pid>/credential")` answers the account's token when the drive's source is `account` (82.7).
- `crates/keeper/Cargo.toml` (`objc2-authentication-services`, features `ASWebAuthenticationSession`, `ASWebAuthenticationSessionCallback`, `block2`), `tauri.conf.json` (`bundle.iOS.frameworks: ["AuthenticationServices"]`), `gen/apple/project.yml` `info.properties` (`CFBundleURLTypes` for `keeper`) and the Info plists as the merge requires.
- `docs/constraints-and-limitations.md`: the audited `#[allow(unsafe_code)]` rows.

**Acceptance:**
- *By inspection, awaiting CI macOS:*
  - every command is registered on every target;
  - the handler order is voice → setup → oauth;
  - the cold-start path routes the same URL the same way;
  - the Apple module retains the session, provider and block, runs on the main thread, maps code 1 to Cancelled and codes 2/3 to Internal, and has a legacy fallback below macOS 14.4 / iOS 17.4;
  - each unsafe function has a `// SAFETY:` comment and an inventory row.
- *Shell tests to run on the macOS gate:*
  - `account_forget` deletes `account.toml` and `<data>/account/<id>` and leaves every other file in `<data>` byte-identical. Mutation: deleting `<data>/account` whole turns it red when a sibling id's directory exists;
  - `account_sync(false)` twice within 15 minutes performs one fetch, and `account_sync(true)` always fetches;
  - with no descriptor, boot reads no clone and makes no request (NFR-92);
  - the loopback listener accepts exactly one request and closes.
- *On hesperia and an iPhone (task acceptance 2, owed):*
  - a fresh install of each;
  - a setup link opened from Messages while keeper is not running opens the confirmation;
  - sign-in with a 1Password passkey in the auth session;
  - `same` mode against a test server, and `oauth` mode against Forgejo with `signin_url`;
  - the phone's device file is `devices/iphone-xxxx.toml` with class `mobile`.

**binds:** FR-683, FR-688, FR-694, FR-697, FR-698, FR-699, FR-700, FR-701, FR-702, NFR-92, NFR-93, NFR-95, AD-311, AD-312, AD-314, AD-315

### 82.6 — Settings › Account, and one setup sheet
**Intent:** "Miej na uwadze zeby UI i UX byly spojne i dobre do uzycia - dodaj zarzadzanie uzytkownikami - moze onboarding nowych uzytkownikow przez qrcode". One section, one sheet, one QR code, and a status that speaks only when something is wrong. **Rung:** **epic82-shell-surface** (lane Front). UX-DR116.
**Files** (the contract's list; the lane's report is the final one):
- `src/lib/ipc/client.ts`: wrappers for every command above, with types re-exported from `gen`.
- `src/lib/stores/account.ts`: a mirror of the subscription.
- `src/components/settings/account-section.tsx`: the first section of `SettingsBody` on every tier.
- `src/components/account/account-setup-sheet.tsx`, `account-share-sheet.tsx` (the white-card QR per the house pattern, plus *Copy link*) and `account-status-line.tsx` (beside the sync status).
- The first-run wizard's optional step; the `keeper://account-setup` listener that opens the sheet; `accountSync(false)` on window focus.
- `dev/mock-shell.ts`: signed-out answers for every new command, so the harness renders.

**Acceptance:**
- *Signed out*: the section is first, ungated, and shows exactly UX-DR116 clause 1's heading, sentence and field. No disabled control is rendered (a Testing Library query for `[disabled]` inside the section finds none).
- *One sheet*: pasting a link, receiving `keeper://account-setup`, and the wizard's step all open the same sheet:
  - the hosts are in mono and never truncated;
  - the device name is editable until registered;
  - *Cancel* calls `account_cancel_sign_in` during sign-in;
  - nothing calls `account_setup_confirm` before *Continue*.
- *Signed in*: the identity row, the role chips, the status sentence rendered verbatim from `OrgAccountVm.sentence`, *Sync now* calling `accountSync(true)`, the device list with this device marked and an inline *Rename*, the share sheet's QR code on a white card at ≥ 240 px with *Copy link*, *Sign out…* in an AlertDialog saying the repository's files are kept, and *Forget this account…* in a destructive AlertDialog per clause 8.
- *Status line*: rendered only for `offline`, `needsSignIn` and `blocked`. No toast is raised for any state.
- *Wizard*: with the step skipped, the existing wizard tests pass unchanged.
- *Focus*: a window focus event calls `accountSync(false)`.
- *Real browser*: the section, the sheet and the share sheet are proved in a real browser over the dev harness (`dev/mock-shell.ts`), by the house method for boot-gated UI: the real engine over tunnelled CDP (`spec-skipping-setup-can-stick.md:123-125`).

**binds:** FR-683, FR-688, FR-694, FR-696, FR-701, FR-702, FR-703, FR-704, FR-705, UX-DR116

### 82.7 — The account opens drives and bots
**Intent:** "latwiejsze dodawanie innych drivow oraz laczenie do botow". A drive or a bot provider can use the account instead of a pasted token, one row at a time, and never by itself. **Rung:** the keeper-core half is on **epic82-core** (lane CoreSession: registry families, KeySpec rows, the bots read site). The shell bridge and the forms are on **epic82-shell-surface** (lanes Shell and Front). AD-315.
**Files:**
- `keeper-core/src/registry.rs` and `config/keys.rs`: the `sync.credential_source.` and `bots.provider_credential_source.` families.
- `keeper-core/src/bots/**`: the `bot_provider_token/{id}` read consults the source.
- The shell: `SyncPlatform::secret_get`, and the `sync_credential_source_*`/`bots_provider_credential_source_*` commands.
- `src/components/sync/add-folder-form.tsx` and the bot provider form: the choice.

**Acceptance:**
- *keeper-core*:
  - with `bots.provider_credential_source.p1 = account`, the provider's request carries `Authorization: Bearer <access token>` and the keychain's `bot_provider_token/p1` is not read;
  - with the row absent, the keychain token is used exactly as today.

  Mutation: ignoring the source turns the first case red.
- *Shell, by inspection*: `secret_get("sync/p1/credential")` returns the account's token when the source is `account`, and the keychain's otherwise. The bridge never blocks the async runtime.
- *Front*:
  - the choice is absent while no account is ready, unless it is already chosen for that row;
  - choosing it hides the token field and calls `syncCredentialSourceSet(pid, "account")`;
  - no `syncSetCredential` call follows;
  - choosing the keychain again shows the field.
- *No switch-over*: an existing drive or provider keeps its keychain token after sign-in until a person chooses otherwise.

**binds:** FR-706, FR-707, NFR-93, AD-315, UX-DR116

## What stays out

- **An admin surface over other people's directories.** keeper manages only the signed-in person's directory and devices. People, roles and access belong to the identity provider and the forge. The repository may let everyone write everywhere, and keeper must not be the tool that does it (D-25, DW-291).
- **Scanning a QR code in keeper.** keeper renders QR codes and never scans them (`research-account-2026-09-23.md` §5.4). A phone's camera, or a pasted link on a Mac, is the other half.
- **keeper-syncd using the account.** One refresher per device (AD-310). The daemon keeps its own credentials.
- **HTTPS callbacks and universal links.** They need a keeper-owned domain and associated domains that each IdP would have to register. A custom scheme or loopback is the universal option (`research-account-2026-09-23.md` §7.1).
- **Ephemeral auth sessions.** They would make the forge leg a second password prompt and hide passkeys saved in the browser.
- **Moving a Matrix account onto the organisation account.** The two coexist. The Matrix flow is untouched.

Deferred, with the ledger entries allocated here so a later planner finds them:

```markdown
### DW-290: Sign-in on Android (Auth Tab, Custom Tabs) does not exist, because Android does not.

origin: epic 82's plan, 2026-09-23 (AD-311, spec §3.1)
location: `src-tauri/crates/keeper-core/src/platform.rs` (`Platform::start_web_auth`, default `open_url`), `src-tauri/crates/keeper/src/web_auth.rs` (the platform modules), `src-tauri/crates/keeper/src/ipc.rs:460-464` (no Android `Platform`)
reason: keeper has no Android target, no `gen/android` and no Android `Platform`, so there is nothing to put an auth tab into. When Android exists, the evidence says: `AuthTabIntent` from `androidx.browser` ≥ 1.9.0, launched through Tauri's `startActivityForResult` and parsed with `AuthenticateUserResultContract` (public since 1.10, a spike is needed); a Custom Tabs bridge activity with an intent-filter and `onResume` cancel detection as the fallback; and not tauri-plugin-web-auth, which has no cancel handling (`research-account-2026-09-23.md` §7.3, §7.6). The device class there is smallest-width ≥ 600 dp → `tablet`. Recorded as deferred work, not as a stub.
status: open

### DW-291: keeper does not administer other people's directories in the config repository.

origin: epic 82's plan, 2026-09-23 (AD-312, AD-313, D-25; spec §2.3, decision 5)
location: `src-tauri/crates/keeper-core/src/org_account/layout.rs` (`is_own_path`, `plan`), `src-tauri/crates/keeper-sync/src/config_repo.rs` (`commit_and_push`'s path refusal)
reason: the owner asked for user management. keeper's answer is the QR code and link onboarding plus the person's own identity, roles and devices. An admin who wants to edit another person's `keeper.toml`, remove a leaver's directory or reset a device does it in the forge today. A keeper surface for it would need, at least: a role that grants it (decided by the IdP, read like `required_role`), a write guard that is not `is_own_path`, an audit trail in the commit, and a decision on whether keeper may ever rewrite a file it did not create. D-25 records why keeper does not. Revisit when an operator reports that the forge's own UI is not enough.
status: open

### DW-292: On Forgejo, one device's forge sign-in or refresh invalidates the forge refresh token on every other device.

origin: epic 82's plan, 2026-09-23 (AD-310, `oauth` mode; spec §3.5)
location: `src-tauri/crates/keeper-core/src/org_account/session.rs` (`forge_connect`, `forge_token`), `docs/account.md` §"Operator notes"
reason: Forgejo v16 defaults `INVALIDATE_REFRESH_TOKENS = true`, and a grant is unique per (user, app). Every token issuance increments the grant's counter, and a refresh token with a stale counter is rejected as "token was already used". Gitea's default is `false` (`research-account-2026-09-23.md` §8.2 item 6). This is inferred from code and not tested live. With two devices in `oauth` mode, the second device's forge leg would fail at its next refresh and ask to reconnect the repository. Options: document `INVALIDATE_REFRESH_TOKENS = false` for keeper's app (done in `docs/account.md`); treat the forge's `invalid_grant` as *reconnect* rather than *sign in again* (done by FR-693's wording); or avoid the forge's own tokens (`same` mode, or DW-295's exchange). Verify on a Forgejo 16 instance with two devices.
status: open

### DW-293: A drive on a sub-path forge gets no API base, and its PR handoff silently skips.

origin: epic 82's plan, 2026-09-23 (AD-316; triage row "Forge API base from the remote URL")
location: `src-tauri/crates/keeper-sync/src/engine.rs:36685-36736` (`forge_api_target`), `:12189-12197` (`do_open_pr`), the test at `:25332`
reason: `forge_api_target` splits the path at the first `/` and requires exactly `owner/repo`, so `https://host/git/owner/repo.git` returns `None` and the worktree lane's PR round trip degrades to "branch is waiting" with only a warning. Taking the last two segments is not a fix in general, because a URL cannot say how deep the forge's root is: Gitea at `/a/`, GitLab at the root with a nested group, or GitLab at `/a` all fit `https://h/a/b/c.git` (`research-account-2026-09-23.md` §8.6). The config repository does not use this function (AD-316). Drives need an explicit per-drive API base, or an account's `api_base` when the drive is on the same forge. Out of this epic, because nothing in the owner's ask reaches the PR handoff.
status: open

### DW-294: The OAuth redirect default is `keeper://`, not RFC 8252's reverse-DNS scheme, and no reverse-DNS scheme is registered for deep links.

origin: epic 82's plan, 2026-09-23 (AD-311; spec §3.1, decision 3)
location: `src-tauri/crates/keeper-core/src/org_account/descriptor.rs` (`redirect_uri`, `forge_redirect_uri` defaults), `src-tauri/crates/keeper/tauri.conf.json:52-55` (`deep-link.desktop.schemes: ["keeper"]`), `gen/apple/project.yml` (`CFBundleURLTypes`)
reason: RFC 8252 §7.1 says a private-use scheme MUST be reverse-DNS (`dev.tgorka.keeper:/oauth/…`), and §8.4 says an authorization server SHOULD reject a scheme with no period. The owner kept `keeper://` as asked. Zitadel accepts custom schemes, Gitea and Forgejo showed no scheme validation, and Authelia is unverified (`research-account-2026-09-23.md` §9.2 T1). A descriptor may already name any `redirect_uri`. On macOS and iOS a reverse-DNS value works through the auth session, which needs no registration for its callback scheme. On other desktops it arrives by deep link, and only `keeper` is registered. Revisit when a provider refuses `keeper://`: register `dev.tgorka.keeper` with the deep-link plugin, and decide whether the default moves.
status: open

### DW-295: One access token serves every service that accepts it; there is no per-service token exchange.

origin: epic 82's plan, 2026-09-23 (AD-315 and its refinement A2c; spec §3.6)
location: `src-tauri/crates/keeper-core/src/org_account/session.rs` (`access_token`), `src-tauri/crates/keeper/src/account_ipc.rs` / the `SyncPlatform::secret_get` bridge, `src-tauri/crates/keeper-core/src/bots/**` (the provider credential read)
reason: a drive or a bot provider set to "Use my account" receives the sign-in access token. A token carrying N audiences can be replayed at any of the N, and RFC 9700 §2.3 asks for tokens restricted to one resource server or a small set (`research-account-2026-09-23.md` §6.5). Zitadel and authentik 2026.8 support RFC 8693 token exchange, which would mint a per-service token with one audience. Two consequences today: an operator must add every service's audience to the one token, through `extra_scopes`; and a drive on a forge that accepts only its own tokens (the `oauth`-mode forge) cannot use the account, because the drive receives the IdP token, not the forge token. Revisit when a second service is set to the account, or when an operator asks for narrow tokens.
status: open

### DW-296: `mainSyncFolder` cannot come from the account's device file.

origin: epic 82's plan, 2026-09-23 (AD-309, AD-101 held)
location: `src-tauri/crates/keeper-core/src/config/mod.rs` (`LayerTier::may_set_main_folder`, `:179-181`), `keeper/src/lib.rs` (boot order: the main folder is learned in phase one, before the account tiers are installed)
reason: only `~/.keeper/` may elect the main folder (AD-101), and the account tiers are refused with the existing fault. A person who wants their main folder to follow them per device would write it into `<login>/keeper.<device>.toml`. But the main folder is learned in phase one from `~/.keeper` alone, disk-only and before any account tier exists, and it decides where the next two tiers are read from. Letting the repository elect it would make a remote file choose a local path, and would need a second boot phase. Revisit if the owner asks for a new device's first launch to pick its main folder from the repository.
status: open

### DW-297: One account per install.

origin: epic 82's plan, 2026-09-23 (AD-308; spec §2.1)
location: `src-tauri/crates/keeper-core/src/org_account/descriptor.rs` (`FILE_NAME`, one `account.toml`), `src-tauri/crates/keeper/src/account_ipc.rs` (one account's state), `src/components/settings/account-section.tsx`
reason: the config repository defines who is using keeper, and two people on one device are two OS users. The `id` is already in the schema, the redirect URI (`keeper://oauth/<id>/…`), the keychain keys (`account/<id>/…`) and the clone path (`<data>/account/<id>/repo`), so a list of accounts can come later without a migration. What a second account would need to decide: which account's tiers win when both set a key (the stack has one account slot), which account a drive or provider's `account` source means, and how the Settings section lists them.
status: open
```

## The failure shape this epic must not repeat

Three, each named after the mistake it would be.

**A token where a token must not be.** This epic's token is a general credential: it reaches the config repository, and by choice drives and bots. Every path that could carry it is closed by construction:
- no IPC field and no VM carries it (`OrgAccountVm` has no token field);
- no log line: `AccessToken`'s `Debug` withholds its secret, and the session types must too;
- no argv: `push_http` everywhere, never the git shim;
- no helper: the gix callback answers `Get` only;
- no URL userinfo;
- no file: the keychain only;
- no config repository: refresh tokens are per device.

A review that finds a token in any of these is a blocker, not a minor.

**A write where keeper has no business.** The config repository is shared, and a forge's permissions may let every person write everywhere. keeper's own rules are the fence:
- **`is_own_path`** refuses any path outside `<login>/`, and the transport refuses `..` and absolute paths again;
- **`plan`** creates and never rewrites;
- **`resolve`** refuses a directory whose `user.toml` names another identity.

Each of the three is pinned by a mutation-proved test (82.3). A green run of the other tests says nothing about them.

**A stack green only at the tip, and a phone nobody opened.**
- **Shared files.** `keys.rs`, `vm.rs`, `lib.rs` and the generated bindings carry hunks from several lanes (the table under *Stories*). `epic82-core` must build, lint and pass `bindings:check` alone before `epic82-shell-surface` is stacked on it.
- **The shell is by inspection.** The web-auth FFI, the deep-link arms, the cold start, the boot install, the join and `account_forget` await CI's macOS job.
- **The phone is its own check.** The iOS scheme, the framework link and the auth session have never run on a phone in this tree. **82.5 is not done until task acceptance 2 has run on hesperia and an iPhone, whatever Linux reports.**

## Sprint-status entry

Paste under `development_status:` above the epic-81 block. The coordinator owns the ledger; this is the text:

```yaml
development_status:
  # Epic 82: an optional account (OIDC sign-in whose token is a general credential) and a per-person config repository synced to every device, bootstrapped from one input (setup link / QR / paste). Owner approved the model 2026-09-23 with one amendment: the account's tiers sit above ~/.keeper and below the main folder.
  # Stack rungs: epic82-plan, then epic82-core (82.1–82.4 + SyncTransport), then epic82-shell-surface (82.5–82.7). Shell crate by inspection; CI macOS is the gate. Task acceptance 2 (fresh macOS + iOS installs, a 1Password passkey, both modes) is owed on hesperia and an iPhone.
  # Contract amendments A1 (module org_account, TS OrgAccountVm) and A2 (egress, Forget this account, the drive's credential spelling) are in the epic's text.
  # Stack publication is reserved for the coordinator by this checkout's rules.
  epic-82: backlog
  82-1-a-setup-link-names-the-account: backlog
  82-2-keeper-signs-you-in-itself: backlog
  82-3-your-directory-in-the-config-repository: backlog
  82-4-the-accounts-files-are-layers: backlog
  82-5-the-shell-carries-the-sign-in: backlog
  82-6-settings-account-and-one-setup-sheet: backlog
  82-7-the-account-opens-drives-and-bots: backlog
  # DW-290…DW-297 are opened by this plan; none closes here.
```

## docs/decisions.md entry

Draft for `docs/decisions.md`, to follow D-24 (`docs/decisions.md:1207`). The number is **D-25**, the next free.

```markdown
## D-25 — keeper does not administer other people's directories

The owner asked for user management. keeper has no server, and the per-person config
repository it syncs is a git repository that a forge's permissions may leave writable by
everyone in it. Epic 82 decides what keeper does there, and what it refuses to be.

- **What keeper manages:** the signed-in person's own identity (shown, never edited),
  their roles (read from the identity provider, never assigned), their own directory
  `<login>/` in the config repository, and their own devices inside it. It onboards other
  people by showing a setup link as a QR code or a copyable link. Each person then signs
  in as themselves, and keeper creates *their* directory from the template. (AD-312,
  AD-313; FR-689…FR-691; UX-DR116)
- **What it refuses:** writing, staging or pushing any path outside the signed-in
  person's `<login>/`; rewriting a file that exists, their own `user.toml` included;
  loading a directory whose `user.toml` records a different identity; and any surface
  that lists, edits, resets or removes another person's directory or devices. The
  refusal is structural. `is_own_path` guards the planner, the transport refuses `..` and
  absolute paths again, and the planner only ever creates.
- **Why:** people, roles and access belong to the identity provider and the forge,
  which have audit trails, permissions and admins. A keeper that could edit another
  person's settings would be an unaudited admin tool that anyone with repository write
  access could use, from any device, with whatever roles their token happens to carry.
  A forge that lets everyone write everywhere is the operator's choice. keeper must not
  be the tool that makes the choice dangerous.
- **What it is not:** a restriction on the operator. They may edit any directory in
  the forge, seed `_template/`, pre-create `<login>/user.toml` for a person with an
  `identity_field` of their choosing, or remove a leaver. keeper reads whatever it finds
  there, subject to the identity check.
- **Revisit triggers:** an operator for whom the forge's UI is not enough (DW-291);
  a role-gated admin mode, which would need an IdP-decided role, a different write
  guard, and a commit trail naming who changed whose file; a second account per install
  (DW-297), which changes whose directory is "own".
- **Status / owner:** decided. Owner is the architect. Epic 82 implements it:
  `org_account::layout::is_own_path` and `plan`, and `keeper_sync::config_repo`'s path
  refusal.
```

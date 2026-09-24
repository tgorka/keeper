# Epic 85 — Your device comes back, and one sign-in opens everything

created: '2026-09-24'
source: the owner's request of 2026-09-24 (verbatim below), made on hesperia after epic 84 (PR #402, merged as `2b74d57`) had synced the account's config repository; the owner's three answers to the coordinator the same day; and the coordinator's pushback on the microphone. Four read-only lanes grounded it: `local://g85-GInfra.md` (makistack's servers, probed live), `g85-GCred.md` (the credential plumbing for drives, bots and Matrix), `g85-GDevState.md` (everything that is still device-local) and `g85-GSchedule.md` (a background pull under AD-62). The coordinator froze the model as `local://epic85-contract.md` and amended it five times during the build wave (A1–A5, below). Line numbers are at `2b74d57`. Where a digest's number had drifted, this plan re-read the line and cites what it found.
binds: FR-723…FR-733 and NFR-99…NFR-100 (allocated here); AD-326…AD-332; UX-DR119; DW-306…DW-318 (allocated in *What stays out*); D-27 (drafted at the end, for `docs/decisions.md`). **FR-722, NFR-98, AD-325, UX-DR118, DW-305 and D-26 were the previous ceilings** (epic 84; D-26 is `docs/decisions.md:1283`). A repo-wide grep of `_bmad-output`, `docs`, `src` and `src-tauri/crates` for `epic-85`, FR-723…FR-739, NFR-99…NFR-109, AD-326…AD-339, UX-DR119, DW-306…DW-319 and D-27 found no earlier allocation. The only hits were range mentions: "NFR-92…NFR-99" in `research-account-2026-09-23.md:128` and "DW-300…DW-309" in epic 84's front matter.
see-also: epic 84 (AD-320's merge, AD-321's references and its A1, AD-323's offers, AD-324's rewritable files, AD-325's write-back, and fix-wave ruling R6, which this epic reverses); epic 82 (AD-310's one access token, AD-311's sign-in sheet, AD-314's rename, AD-315's credential choice, DW-292, DW-294, DW-295); epic 62 and D-5 (the phrase is armed by a person, AD-168); epic 68 (AD-218, the one listening verb); AD-27 (absent rather than disabled); AD-40 (the crate split); AD-62 and D-3 (one clock per host process); `docs/account.md` § *Your settings, drives and accounts travel*, § *One sign-in: drives, bots and Matrix* and § *Keeping up in the background*.

## The owner's ask

Verbatim:

> - move all the settings to the one set in git (including the encryption choice and the path to git, "listen for a phrase" and the voice language, etc with the part of the settings that are linked to the device - so it could be restore from
> only tokens and passwords can stay local (oauth will replace tokens and password - so only one auth could be needed)
> - Changes from the repo are only pulled - add also even if the background once per 24h
> - dont see in the settigns in git from hesperia app the tgdrive-light (with all the settings for lfs and virtual folders)
> - use (set in the settings) to use oauth for drives (all of them) and bots on hesperia, also matrix from tgorka@electra (change in git keeper-config and add support for the keeper for such config) - make sure you update hesperia app if needed

In other words, four asks:
1. **Everything in the repository.** Every setting, including the ones tied to a device, so that a device can be restored from the repository. Only tokens and passwords stay local, and OAuth replaces those, so one sign-in is enough.
2. **A daily pull.** The repository is pulled at least once a day, even while keeper sits in the background.
3. **tgdrive-light is missing.** hesperia syncs two drives from one repository, `tgdrive` and `tgdrive-light`, and only one of them reached the repository.
4. **One sign-in for everything on hesperia.** Every drive and every bot provider uses OAuth, and so does the Matrix account `@tgorka` on electra. That means changing the config repository and makistack, teaching keeper the configuration, and installing the result on hesperia.

The owner's answers to the coordinator (2026-09-24):
1. **Matrix: change makistack and deploy.** Tuwunel 1.8.1 gets ZITADEL as its identity provider and turns on its built-in OIDC server. The existing `@tgorka` is kept.
2. **Hermes keeps its own key.** Ollama has no authentication, so choosing "use my account" there does no harm.
3. **Restore is automatic.**

**The coordinator's pushback, accepted:** a restore never arms the microphone. keeper's rule is that "the wake phrase is armed by a person while keeper is in front and never by keeper itself" (`AGENTS.md:92`; AD-168; D-5, `docs/decisions.md:167`). The listening choice therefore travels and is shown, but switching it on stays a person's tap. The owner's words "listen for a phrase" are the switch's own label (`src/components/bots/bot-voice-wake.tsx:108`, `WAKE_SWITCH_LABEL = "Listen for a phrase"`).

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| The encryption choice, the git path, listening and the voice language in the repository | **absent, by design** (epic 84, R6) | `NEVER_SYNCED` holds exactly these four keys (`keeper-core/src/org_account/settings_sync.rs:40-49`). Its comment gives the reasons: the posture is keyed to this machine's keychain, a git path is a fact of this disk, an open microphone is armed per device, and a language must be installed where it listens. `synced_file` refuses them before looking at the scope (`:53-63`). |
| The encryption choice can travel safely | **present, if it is a choice for new stores** | The posture is read only when a Matrix account is added: `auth.rs:614-618` mints that account's store passphrase into the keychain when the posture is on. An account already on the device keeps the store it has, so a travelling value re-keys nothing. `sdk_encryption` stays `Settable::Never` for layer files (`config/keys.rs:381-393`). The settings files are not layers (AD-320), so that refusal still holds. |
| The git path can travel safely | **half-present** | `sync.git_path` is a `Shape::AbsolutePath` to a binary (`keys.rs:854-866`). Epic 84's A1 applies an absolute path only when it is a **directory** (`from_portable`, `settings_sync.rs:647-651`, `Path::is_dir`), so a git binary would never apply. |
| Listening can travel without arming anything | **absent** | `bots.wake_enabled` is the person's *intent*, and `voice::should_rearm(intent, armed, refusal_cleared)` re-arms the phrase whenever keeper comes back in front while the intent is on (`keeper-core/src/voice/mod.rs:792-805`). Applying a travelling "on" would therefore open a microphone on a device where nobody chose it. **Worse, a pin can already do it today:** the key is `Settable::AnyLayer` (`keys.rs:457-466`), and `get_bots_wake_enabled` reads through `get_setting`, where a layer override wins (`registry.rs:208-211`, `:922-924`). So `"bots.wake_enabled" = true` in `<login>/keeper.toml` would arm every device, and anyone who can push to the repository can write that file (DW-303). A1 closes this. |
| The last notes-list choices | **absent** | `notes.hide_service_files` and `notes.include_private` are `SessionState` and `Settable::Never` (`keys.rs:601-620`). `synced_file` drops every `SessionState` key (`settings_sync.rs:61`). |
| "Only tokens and passwords stay local": which credential a drive or provider uses | **half-present** | The choice is `sync.credential_source.<pid>` or `bots.provider_credential_source.<pid>`, stored as `account:<id>` (`registry.rs:1858-1860`) and read back as `account` only while it is bound to the configured account (`bound_to`, `:1883-1886`). Both are `SessionState` families refused from files (`keys.rs:432-446`, `:867-881`). Today the choice travels only as a description in `drives.toml` and `bots.toml` (`keeper/src/account_settings.rs:69-80`, `:92-103`), which an offer may suggest and no file sets. |
| The rest of a drive, so it can be rebuilt | **absent** | `drive_record` copies only the portable half (`account_settings.rs:160-187`). The folder, direction, lane, subpaths, LFS mode, `lfs_prune_local`, `lfs_never`, cadence, author override, the watch switch, the notes templates and the recordings media and push policies all stay in `sync.db` (`keeper-sync/src/profile/mod.rs:933-1132`). So do the drive's schedules, in the `tasks` table (`keeper-sync/src/db.rs:202-213`; the full column list is `:3150-3154`). |
| A bot provider, whole | **half-present** | The record carries `read_timeout_ms` (`account_settings.rs:104`), but adopting an offer drops it: the provider is built as `{ id, kind, name, base_url, created_ms }` (`keeper/src/account_ipc.rs:2578-2584`). Folder grants never travel. They are `bot_grants` rows keyed by local ids (`keeper-core/src/bots/store.rs:149-152`). |
| A Matrix account, whole | **half-present** | `MatrixRecord` carries the user id, the homeserver and the sign-in kind (`org_account/manifest.rs:140-154`). The account's colour (`hue_index`, `registry.rs:566-567`), its incognito choice (`ensure_incognito_column`, `:606`) and the muted networks (`:174-176`) live only in keeper.db. The session is a keychain secret and must stay one. |
| A device rebuilt from the repository | **absent, by design** (epic 84, AD-323) | "Nothing is added by itself" (`manifest.rs:10-14`). A drive, provider or account becomes an offer the person has to adopt. |
| A reinstall that is still the same device | **absent** | `free_device_slug` suffixes the name whenever `devices/<name>.toml` exists and this install did not register it (`org_account/layout.rs:241-271`). "Registered" is only the registry's `account.<id>.device_slug` (`account_ipc.rs:1055-1071`, and `:2071-2076` for the setup sheet), and *Forget this account* deletes it (`registry.rs:2143-2146`). A reinstall is therefore always a new device, and its old files are orphaned. |
| A seventh file keeper may rewrite | **absent** | `is_rewritable` accepts exactly `settings.toml`, `settings.<slug>.toml`, `drives.toml`, `bots.toml` and `matrix.toml` (`layout.rs:325-346`). `plan_rename` moves the device record, `keeper.<d>.toml` and `settings.<d>.toml` (`:421-480`). `unusable_files` checks nine names (`:215-239`). |
| tgdrive and tgdrive-light, both | **broken** | A drive's identity is `DriveRef { remote_url, branch }` and its reference is `drive:<remote>#<branch>` (`settings_sync.rs:466-483`). `merge` drops the second of two local records with one identity (`manifest.rs:330-335`) and collapses the remote side the same way (`:326-329`). The offers compare identities (`missing`, `:405-410`), and an offer's key is that reference (`:434`). `from_portable` resolves the reference to the first matching profile (`settings_sync.rs:615-623`). So one of the two never reaches `drives.toml`, is never offered, and cannot be named by `notes.active_vault`. |
| A pull while nobody looks | **absent** | A sync runs at launch (`kick`, `account_ipc.rs:666-673`), on window focus or `visibilitychange` (`src/hooks/use-account-mirror.ts:71-89`), after a local change (`note_local_change`, `account_ipc.rs:703-713`), and on *Sync now*. "There is no interval here (AD-62)" (`:31-34`). An unforced sync waits 15 minutes (`SYNC_INTERVAL_MS`, `:78`; the check `:1834-1838`). |
| A clock to hang a daily pull on | **present, guarded** | The shell's only clock is the 1 Hz tray tick (`keeper/src/lib.rs:663-707`), which already runs the notes cadence at `:696` (`notes_vault::cadence_tick`, `notes_vault.rs:3403`). The tick keeps running while the window is hidden, because closing the window calls `prevent_close` and `hide` (`lib.rs:1531-1535`). It is desktop-only (`#[cfg(desktop)]`, `lib.rs:663`). `src/test/task-host-tick.test.ts:83-93` freezes its body: it must contain the tray renderers and `cadence_tick()`, and nothing matching `/task/i`. |
| OAuth for every drive | **broken on the forge** | A drive set to the account receives `access_token()`, the ZITADEL sign-in token (`drive_credential`, `account_ipc.rs:1943-1974`, bridged by `SyncPlatform::secret_get`, `keeper/src/sync.rs:84-88`). The config repository in `oauth` mode uses `oidc::forge_token` instead (`account_ipc.rs:810-821`), with one forced refresh and retry on a refusal (`with_forge_retry`, `:837-869`). Forgejo accepts only its own tokens, so a drive on electra cannot use the account today. `docs/account.md:751-752` says so, and DW-295 records it. |
| The spelling of a forge token on a drive | **present** | keeper-sync sends a drive token as the Basic user name with an empty password on git (`keeper-sync/src/credential.rs:58-62`), as `Basic base64("<token>:")` for LFS (`:71-75`), and as `token <t>` for the forge API (`:81-83`). Forgejo resolves a Basic user name that is a token through `isUsernameToken` and then `CheckOAuthAccessToken`, so an OAuth2 access token is accepted in exactly that form (Forgejo `services/auth/basic.go`, read for the contract). GCred §1.2 had inferred that an `oauth2:<token>` form, and a new keeper-sync seam to carry it, would be needed. That is not so, and keeper-sync is not touched. |
| OAuth for every bot | **present** | A provider set to the account gets the sign-in token as `Authorization: Bearer` (`bot_credential`, `account_ipc.rs:213`; `bots::resolve_credential`, `bots/mod.rs:439`; `bots/http.rs:113-116`). |
| Matrix through the account, in keeper | **half-present** | `OidcAuthProvider` runs matrix-sdk's OAuth flow: discovery, dynamic registration, browser, callback and `finish_login` (`keeper-core/src/auth.rs:157-225`). It opens the **system browser** (`platform.open_url`, `auth.rs:199`), not the sign-in sheet the account uses (`Platform::start_web_auth`, `platform.rs:51-67`; the Apple implementation at `keeper/src/ipc.rs:728-734`). The browser does not share the sheet's ZITADEL session, so the person would sign in twice. The redirect is `keeper://oauth/callback` (`oauth.rs:31`) and the advertised `client_uri` is `https://keeper.tgorka.dev/` (`:34`). A `oidc` offer only prefills the Password tab (`src/components/auth/login-screen.tsx:134-147`). |
| Matrix through the account, on the server | **absent** | Live (GInfra Q1): Tuwunel 1.8.1 answers `GET /_matrix/client/v3/login` with `m.login.password`, `m.login.token` and `m.login.application_service`, with no `m.login.sso`. `GET …/org.matrix.msc2965/auth_issuer` returns 404. ZITADEL's relying parties do not include Matrix (makistack `docs/runbooks/zitadel.md`). |
| The other servers | **measured** | Live (GInfra Q2–Q5). Hermes on `100.101.101.20:8642` checks a static bearer key: `GET /v1/models` without it gives 401, and `/health` gives 200. Ollama 0.32.6 on `100.101.101.34:11434` has no authentication at all and is gated only by the Tailscale ACL. Forgejo is 15.0.4. Its `keeper` connector is a public PKCE client with scope `openid profile write:repository`, which covers git push and LFS (`CheckRepoScopedToken`, research-sync §6.2), and its token lasts 1 hour. ZITADEL's `keeper` app issues 12-hour access tokens, and its refresh tokens last 30 days idle and 90 days at most. Authorized Integrations (an external identity provider's JWT used as a Forgejo credential) arrive only in Forgejo 16. |
| "Make sure you update hesperia app" | **owed** | The shell crate does not build on this host, so installing on hesperia, and the Mac gate there, are part of every story's *owed* checks, as in epics 82–84. |

## The one sentence

**keeper carries a person's preferences between devices, but not the device: four device-linked keys are blocklisted, a drive's local half and schedules, a provider's grants and a Matrix account's look live only in local databases, two drives on one repository collapse into one, a reinstall is a stranger, a drive on the forge gets a token the forge refuses, Matrix has no single sign-on, and nothing pulls unless someone opens the window. The fix: the blocklist shrinks to what is truly per install, and listening travels without ever being switched on; a drive's name joins its identity; a device file that only this device writes holds everything needed to rebuild it; the first sync of an install restores it once, and a reinstall keeps its name; a drive on the forge's host gets the forge's token; Matrix single sign-on opens in the account's sheet against a homeserver that now trusts ZITADEL; and the tray tick pulls once a day.**

## What earlier epics decided, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **Epic 84, R6, and `NEVER_SYNCED`** (`settings_sync.rs:40-49`) | `sdk_encryption`, `sync.git_path`, `bots.wake_enabled` and `bots.voice_locale` never leave the device | The owner named all four. | **Reversed (AD-326).** All four travel in `settings.<device>.toml`, and each applies only where it can: a posture for stores created afterwards, a path where it exists, "off" but never "on" for listening, and a language the recogniser can run. |
| **AD-320's `synced_file`** | a `SessionState` key never travels | The last notes-list choices, and each drive's and provider's credential choice. | **Amended (AD-326).** `notes.hide_service_files`, `notes.include_private` and the two credential-source families travel in the device settings file. The rest of `SessionState` stays local. |
| **AD-315** (`keys.rs:432-446`, `:867-881`) | no file sets the credential-source families: "a file flipping it would send the account's token to a remote the person never chose" | "Oauth for drives (all of them) and bots", restored with the device. | **Amended (AD-326).** The choice travels only in this device's own settings file, keyed by the drive's or provider's reference, and it binds only to this account. Layer files still cannot set it. Anyone who can push to the repository could now flip it, which is the trust DW-303 already records for every synced value. |
| **Epic 84's A1** (`settings_sync.rs:647-651`) | an absolute path applies only where its **directory** exists | `sync.git_path` names a file. | **Generalised (AD-326).** A path applies where it exists, as a directory or a file. |
| **AD-321 and AD-323's drive identity** `(remote, branch)`, and **DW-302** | two profiles of one repository are one drive | tgdrive and tgdrive-light. | **Amended (AD-327).** The name joins the identity. The reference names the drive only when two local drives share remote and branch. **DW-302 closes.** |
| **AD-323:** "Nothing is added automatically" | a drive needs a folder, a provider a key, a Matrix account a sign-in | "Restore: automatic". | **Held for offers, amended for this device's own file (AD-329).** A device restores what it had itself, once per install. A drive gets the folder it had, a provider uses the account or asks for its key through the existing health state, and a Matrix account still needs a sign-in, which the restore starts. |
| **AD-324:** exactly five files are rewritable | `user.toml`, pins and device records stay create-only | A per-device state file. | **Extended (AD-328).** `device.<slug>.toml` is the sixth. Its only writer is the device it names. |
| **AD-314 and epic 84's A3:** a rename moves the device's files | the record, `keeper.<d>.toml`, `settings.<d>.toml` | A fourth per-device file. | **Extended (AD-328).** `plan_rename` also moves `device.<from>.toml`. |
| **Epic 82's `free_device_slug`** (`layout.rs:241-271`) | a taken name gets a suffix unless this install registered it, "so two machines with one host name never share a device record and its settings" | A reinstall that keeps its name. | **Amended (AD-329, A5).** A record of the same class and platform is adopted when it carries this machine's fingerprint, or no fingerprint at all. Two machines with one host name still get two records. |
| **Epic 82's drive credential** (`docs/account.md:751-752`; DW-295's second consequence) | "in `oauth` mode a drive receives the sign-in token … A drive on a forge that accepts only its own tokens cannot use the account" | Drives on electra through the account. | **Amended (AD-330).** A drive on the forge's host gets the forge token. The rest of DW-295 (one token for many audiences) stays open. |
| **Story 2.2's Matrix OIDC** (`auth.rs:199`, `oauth.rs:31`) | the system browser; `keeper://oauth/callback` | One sign-in, with no second password. | **Amended (AD-331, A3).** The sign-in sheet where the platform has one, and the reverse-DNS redirect `dev.tgorka.keeper:/oauth/callback`. **DW-294 is addressed for Matrix.** The account's own redirects keep `keeper://`. |
| **AD-62, D-3 and the tick guard** (`task-host-tick.test.ts:83-93`) | one clock per host process; nothing about tasks in the tray tick | A daily pull. | **Held.** The pull is a due-check on the existing tick. The guard is updated to expect exactly that one call (AD-332). |
| **AD-40** | keeper-core never depends on keeper-sync, nor the reverse | The whole profile of a drive in a core-owned file. | **Held.** keeper-core treats a drive table as an opaque `toml::Table` that the shell renders. |
| **AD-168 and D-5** | the phrase is armed by a person, never by keeper | Listening that travels. | **Held and tightened (NFR-99, A1).** Neither a sync, nor a restore, nor a pin arms it. |

## Decisions this epic takes

The rules below are the plan. The coordinator amended the contract five times during the build wave (*Contract amendments*, below), and the rules already include those amendments.

- **AD-326: Every setting travels, and device-linked ones travel in the device settings file.**

  **Binds:** FR-723, FR-724, FR-725; NFR-99; Story 85.1.

  **Prevents:**
  - a device that cannot be restored because four of its settings were blocklisted;
  - a microphone armed by a sync, a restore or a pin (AD-168);
  - a git path, or any other path, applied on a disk that lacks it;
  - a drive's or provider's credential choice lost with the device, so that every drive asks for a token again;
  - a credential choice that binds to another account, or to a drive or provider this device does not have;
  - a person's own change to a credential choice that does not reach the repository.

  **Rule** (keeper-core `org_account::settings_sync`):
  - **`NEVER_SYNCED` shrinks.**
    - `sdk_encryption`, `sync.git_path`, `bots.wake_enabled` and `bots.voice_locale` become `Device` keys.
    - `notes.hide_service_files` and `notes.include_private` (`SessionState`, the notes list's memory) become `Device` keys.
    - Still local: `notes.read.*`, `notes.capture_*`, `notes.pristine.*`, the `ui.*` latches and `account.*`.
  - **`bots.wake_enabled`:**
    - a remote `"0"` applies;
    - a remote `"1"` is **never applied**. `from_portable` returns `None`, so the value stays in the file and the base records `l` (A2');
    - the VM reports it (AD-329), so the UI can offer *Turn listening on*;
    - **no layer file may set it** (A1): the key is `Settable::Never`, and a layer that names it gets a `KeyRefused` fault.
  - **A1, generalised:** an `AbsolutePath` value applies only where that path exists, as a directory **or** a file (`sync.git_path` is a file).
  - **The credential-source families travel with translated keys.**
    - `sync.credential_source.<pid>` ↔ `sync.credential_source.<drive reference>`, and `bots.provider_credential_source.<pid>` ↔ `bots.provider_credential_source.<provider reference>`. Both live in the `Device` file.
    - The value `account:<id>` ↔ `"account"`. Any other value, or no row, means the keychain, and the key is absent from the file.
    - `from_portable` maps `"account"` back to `account:<this account id>`. `Catalog` gains `account_id: Option<String>`, which the shell sets.
    - A key whose reference does not resolve here is handled as A2'.
    - `local_values` and `apply` translate the keys.
    - Setting or deleting one of these rows goes through `registry::set_setting` or its delete, as today, so the observer (AD-325) still fires when a person changes one.
  - **What each travelling device key does when applied.** `sdk_encryption` is read only when a Matrix account is added (`auth.rs:614-618`), so it decides the posture of the next store and re-keys nothing (DW-315). `bots.voice_locale` goes through the voice runtime's apply path (`account_settings.rs:222-223`), which refuses a language that cannot run here and never replaces it (`keys.rs:484`).

- **AD-327: Two drives on one remote stay two.**

  **Binds:** FR-726; Story 85.2.

  **Prevents:**
  - a second drive of one repository missing from `drives.toml` (tgdrive-light);
  - a vault, ledger or recording destination that always lands on whichever of the two is listed first;
  - one offer standing for two drives;
  - a reference that changes spelling on every device that has only one drive of that repository.

  **Rule:**
  - **`DriveRef`** gains `name: Option<String>`.
    - `reference()` renders `drive:<remote>#<branch>` when no other local profile shares the remote and branch. Otherwise it renders `drive:<remote>#<branch>@<name>`.
    - The parser accepts both forms.
  - **`from_portable`**, for a drive reference:
    - with `@name`: the profile with that name among those matching the remote and branch;
    - without: the only match, or else the match whose name sorts first.
  - **`Catalog.drives`** carries `(profile_id, remote, branch, name)`.
  - **A manifest drive's identity** is `(normalized remote, branch, name)`. `drives.toml` keeps both `tgdrive` and `tgdrive-light`, and both the offers and the merge use the new identity. An old record whose name matches no local profile still matches on remote and branch when exactly one local profile has that pair.

- **AD-328: The device state file.**

  **Binds:** FR-727; NFR-100; Story 85.3.

  **Prevents:**
  - a device that cannot be rebuilt because half of each drive lives only in `sync.db`;
  - two devices writing one file;
  - a local path, a volume id or a profile id treated as portable;
  - a token in a drive's remote reaching the repository;
  - a drive waiting to be restored disappearing from the file before it is restored;
  - keeper-core learning keeper-sync's types (AD-40).

  **Rule:**
  - **The file** is `<login>/device.<slug>.toml`.
    - It is rewritable: `layout::is_rewritable` accepts `device.<valid slug>.toml`.
    - `plan_rename` moves it.
    - `unusable_files` reports it when it is a link or a folder.
    - It has **one writer**, the device it names.
  - **The types** (keeper-core `org_account::device_state`):

    ```rust
    pub struct DeviceStateFile { pub drives: Vec<toml::Table>, pub providers: Vec<ProviderState>, pub matrix: Vec<MatrixState>, pub extra: BTreeMap<String, toml::Value> }
    pub struct ProviderState { pub kind: String, pub name: String, pub base_url: String, pub read_timeout_ms: Option<u64>,
      pub credential: String /* "account" | "own" */, pub bots: Vec<BotRecord>, pub grants: Vec<GrantState>, pub extra: BTreeMap<String, toml::Value> }
    pub struct GrantState { pub bot: Option<String> /* bot target; None = every bot */, pub drive: String /* drive reference */, pub subtree: Option<String>, pub mode: String, pub extra: BTreeMap<String, toml::Value> }
    pub struct MatrixState { pub user_id: String, pub homeserver_url: String, pub kind: String, pub hue_index: Option<i64>,
      pub incognito: Option<bool>, pub muted_networks: Vec<String>, pub extra: BTreeMap<String, toml::Value> }
    ```
  - **The drive tables.**
    - Each table is the shell's rendering of the whole `SyncProfile` minus `id` and `volume_id`, plus `schedules = [{kind, schedule, mode, enabled}]` from the drive's `sync.db` `tasks` rows. It carries no task ids, no leases and no `next_due`.
    - Every drive table must have a string `name`, `remote_url` and `branch`. Its identity is AD-327's.
    - `remote_url` goes through `manifest::portable_remote`: a remote carrying userinfo is written without it. A local-path remote is kept as it is, because this file restores **this** device, where that local remote is a fact.
  - **Rendering.** `DeviceStateFile::{parse, render, values_eq}`. The output is deterministic and sorted, unknown fields are preserved, and the file starts with this header comment:
    `# This device's drives, bots and accounts, so keeper can restore it. keeper rewrites it from this device; edit settings.<device>.toml instead.`
  - **The restore plan** is pure: `restore_plan(file: &DeviceStateFile, local_drives: &[DriveKey], local_providers: &[ProviderKey], local_matrix: &[String]) -> RestorePlan { drives: Vec<toml::Table>, providers: Vec<ProviderState>, matrix: Vec<MatrixState> }`. It returns the entries whose identity is absent here.
  - **Writing (Shell).** Every sync renders this device's current state, **plus everything still pending restore** (AD-329 and A4), so a pending entry is never dropped. The file is written only when `values_eq` is false, with `replace` when it exists, in the same `"{login}: settings from {device}"` commit.

- **AD-329: This device comes back by itself.**

  **Binds:** FR-724, FR-728, FR-729; NFR-99, NFR-100; Story 85.4; UX-DR119.

  **Prevents:**
  - a reinstalled Mac that has to be set up by hand again;
  - two machines with one host name sharing a device record, a settings file and a device file, which is epic 82's reason for the suffix (`layout.rs:241-244`);
  - a restore that runs twice and undoes a deletion made in between;
  - a folder created somewhere nobody chose, or under a volume that is not mounted;
  - a drive or a grant lost because its volume was not mounted at restore time;
  - a signed-in Matrix account without a sign-in, or two sign-in windows at once;
  - a microphone armed by a restore.

  **Rule:**
  - **When it runs:** at the first successful sync of this install. The marker is the registry key `account.<id>.restored`, a `Flag01` in the `account.` family (`registry::get_account_restored` / `set_account_restored`). The restore runs once, when `device.<me>.toml` exists at the tip, and then the marker is set.
    - **Never again after that.** From then on this device's local state is the truth and overwrites the file, so a drive the person later deletes stays deleted.
    - *Forget this account* clears the marker along with the account's other keys (`registry::forget_account_state`, `registry.rs:2143-2154`). Setting the same account up again on this install is therefore a reinstall, and restores again, idempotently.
  - **A reinstall keeps its name** (A5).
    - A new device record carries `machine = "<hex>"`: the SHA-256 of the operating system's stable machine id together with the account's `sub`. It names this machine to this person only, and reveals no hardware id. The id is `IOPlatformUUID` on macOS (`ioreg -rd1 -c IOPlatformExpertDevice`), `/etc/machine-id` on Linux and `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` on Windows. iOS has none, so its fingerprint is `None`. `layout::plan` writes it into new `devices/<slug>.toml` records, which stay create-only.
    - `layout::free_device_slug(files, login, wanted, class, platform, machine: Option<&str>)` returns the wanted slug, instead of a suffixed one, when `devices/<slug>.toml` exists, records the **same class and platform** as this device, **and** either records this machine's fingerprint or records none. A record with no fingerprint was written before epic 85, like hesperia's.
    - Otherwise it suffixes, as before: another class, another platform, another machine, or a device without a fingerprint (iOS) facing a record that has one (DW-311, DW-317, DW-318).
    - The adoption happens at the first sync, when the clone first exists. On a fresh install the setup sheet cannot know it yet (A2).
  - **The restore actions (Shell)** are each idempotent against identity:
    1. **Settings:** nothing extra. The first sync (base `None`, `RemoteWins`) already pulls both settings files, and that is how the credential choices come back.
    2. **Drives.** For each plan table:
       - build the profile with a new id, `local_path` from the table, and every other field from the table;
       - create the folder when its parent exists;
       - when the parent does not exist (an unmounted `/Volumes/...`), keep the table **pending** in `account.<id>.restore_pending`, with the sentence "Waiting for {path} to restore {name}.";
       - retry the pending entries at every later sync, and drop an entry once a profile with its identity exists;
       - recreate its schedules through the engine's task store.

       This is desktop only. On iOS the phone path assigns the folder, as `phone_shaped_request` does (`keeper/src/sync_ipc.rs:1372-1401`).
    3. **Providers and bots:** insert the provider (with a new id) with its `read_timeout_ms`, then its bots (target, name, pin order and look). For credential `account`, set the source. For `own`, leave it: the existing *secretMissing* health state asks for the key. Then the grants, with each drive reference translated to the new profile id. **A grant whose drive is not here yet waits too (A4):** it goes into `restore_pending.grants` with its provider's reference, is rendered back into that provider's `grants` in the device file, and is inserted when its drive restores.
    4. **Matrix accounts.** Nothing is signed in without a sign-in.
       - `kind == "oidc"`: after the restore, start **one** `login_oidc(homeserver_url)` through the sign-in sheet (AD-331), once per install, marked with `account.<id>.restore_matrix_started`.
       - `password` and `beeper` accounts stay offers, as in epic 84.
       - `hue_index`, `incognito` and `muted_networks` are applied when that account is added later.
  - **What waits.** `account.<id>.restore_pending` holds `RestorePending { drives: Vec<toml::Table>, grants: Vec<PendingGrant> }` as JSON (A4). `restore_plan` itself is unchanged.
  - **The VM.** `AccountVm` gains `restore: AccountRestoreVm { sentence: Option<String>, pending: Vec<String>, listening_off: bool }`, empty by default. Rust composes it:
    - the first sentence is "Restored {n} drive(s), {n} bot provider(s) and your settings from your account.", with zero counts omitted and correct plurals;
    - then one sentence per pending drive;
    - `listening_off` is true when this device's settings file says `bots.wake_enabled = true` but listening is off here.

- **AD-330: A drive on the forge uses the forge's sign-in.**

  **Binds:** FR-730; Story 85.5.

  **Prevents:**
  - a drive on electra that cannot use the account (DW-295's second consequence);
  - the forge's token sent to any host other than the forge;
  - a change to keeper-sync, or to its credential spellings;
  - a forge that is not connected, silently answered with a token it refuses.

  **Rule:**
  - In the shell's `drive_credential`, a drive whose source is `account:<id>` gets `oidc::forge_token(...)` when both of these hold:
    - the descriptor's `config.auth` is `Oauth`;
    - the drive's remote host equals the forge's host (`descriptor.forge_host()`) or the config repository's host (`repo_host()`).

    Otherwise it gets `access_token()`, as today. A forge that is not connected gives `NeedsSignIn`.
  - Forgejo accepts an OAuth2 token as the Basic user name with an empty password, for git and for LFS alike (`services/auth/basic.go`: `isUsernameToken` → `CheckOAuthAccessToken`). So the `AccessToken` spellings are unchanged, and keeper-sync is **not** touched.
  - **On a refusal:** one forced refresh and retry, as `with_forge_retry` does. The refresh is `forge_token_refreshed`, used when the engine reports `SyncError::Auth` for such a drive, if a hook exists. Otherwise the next pass refreshes, because `forge_token` refreshes before expiry. The LFS leg re-reads `secret_get` by itself.
  - Bots are unchanged: they send the sign-in token as Bearer. Hermes keeps its own key (DW-308).
  - The drive's remote is read from `sync.db` on a connection of its own, because `drive_credential` is called from inside the engine. keeper-syncd has no account bridge (epic 82), so it never gets the account's token at all.

- **AD-331: Matrix signs in through the account's sign-in session.**

  **Binds:** FR-731, FR-732; Story 85.6; UX-DR119.

  **Prevents:**
  - a second password for Matrix after the account sign-in;
  - an SSO offer that only fills a form in;
  - a Matrix cancel that ends the account's sign-in, or the reverse;
  - a private-use redirect that the homeserver's client registration refuses (A3);
  - a second Matrix identity for the same person (the existing `@tgorka` is kept).

  **Rule:**
  - **The sign-in sheet.** The browser leg of the Matrix OIDC login (`login_oidc`, `ipc.rs`) opens through the platform's sign-in sheet where the platform has one natively (Apple), instead of `open_url`. The ZITADEL session from the account sign-in then carries over, so Tuwunel's OIDC, which delegates to ZITADEL, asks for no second password. keeper-core's `OidcAuthProvider` (`auth.rs:199`) is unchanged: the shell hands it a platform whose `open_url` presents the sheet and delivers its ending to the Matrix flow registry, not the account's. Where there is no sheet, or it cannot start, the system browser opens as before. A cancel resolves as today.
  - **The redirect (A3)** becomes the RFC 8252 reverse-DNS form `dev.tgorka.keeper:/oauth/callback` (`oauth::REDIRECT_URI`). The advertised `client_uri` stays `https://keeper.tgorka.dev/`, whose host, reversed, is exactly `dev.tgorka.keeper`, the bundle id. The scheme `dev.tgorka.keeper` is registered for deep links beside `keeper` (`tauri.conf.json`, `gen/apple/project.yml`, both Info plists), and the deep-link router sends it to the Matrix flow registry. The sheet's callback scheme comes from `oauth::redirect_uri()`. The account's redirects (`keeper://oauth/<id>/…`) are unchanged.
  - **The offer.** `MatrixRecord.kind == "oidc"` means *sign in with single sign-on at `homeserver_url`*.
    - On the login screen, such an offer prefills the homeserver and **starts** the single sign-on flow when clicked. A flow already in the browser is not started a second time.
    - Password and Beeper offers behave as in epic 84.
  - **The server (makistack; the Infra lane).**
    - ZITADEL gets an OIDC app, `tuwunel`: a confidential web app using the code flow, with the redirect `https://electra.siren-alsephina.ts.net/_matrix/client/unstable/login/sso/callback/tuwunel` and its roles asserted. Its secret is stored in 1Password beside the other makistack secrets.
    - Tuwunel gets one identity provider: `brand = "ZITADEL"`, issuer `https://electra.siren-alsephina.ts.net:8451`, scopes `openid profile email`, discovery on, trusted, and the default.
      - `preferred_username` maps to the localpart, so **`@tgorka` and `@marta` are kept**.
      - Registration stays off, so only existing users can sign in.
      - The built-in OIDC server is on, which is what keeper's `client.oauth()` discovers.
      - Passwords keep working.
    - It ships through makistack's normal path: PR, merge to main, then `deploy.yml` with a snapshot, Goss, the smoke tests and automatic rollback. The runbooks (`matrix-bot-channel.md` or a new `matrix-sso.md`, and `zitadel.md`'s relying parties) are updated with it.
    - It is verified live:
      - `GET /_matrix/client/v3/login` lists `m.login.sso` with the ZITADEL provider;
      - the MSC2965 `auth_metadata` or `auth_issuer` answers;
      - the bot tokens (dixi, nixie) still work;
      - `@tgorka` is unchanged;
      - a client registration with `dev.tgorka.keeper:/oauth/callback`, `client_uri` `https://keeper.tgorka.dev/` and `application_type: native` succeeds (A3).

- **AD-332: A daily pull while keeper runs in the background.**

  **Binds:** FR-733; Story 85.7.

  **Prevents:**
  - a second clock in the shell (AD-62, D-3);
  - a task poll in the tray tick (`task-host-tick.test.ts`);
  - a pull that stops when the window is hidden;
  - a sync started every second while one is refused, or while one is already running;
  - any work without an account.

  **Rule:**
  - `account_ipc::daily_tick()` is called from the 1 Hz tray tick, right after `notes_vault::cadence_tick()`.
    - It does nothing unless an account is configured and the last sync *attempt* is at least 24 hours old (`inner.last_attempt_ms`, or the boot time before any attempt).
    - Then it spawns one `sync(force = true)`. It is guarded against re-entry, the tick itself does no work, and a pull it started is not requested again for another day.
  - The name must not match `/task/i`. `src/test/task-host-tick.test.ts` is updated to expect exactly this one extra call, straight after `cadence_tick()`, and nowhere else in the shell.
  - The tick is desktop-only and dies with the process. iOS keeps its foreground triggers, and a quit keeper pulls at its next launch (DW-314).

- **UX-DR119: "Your device came back".**

  **Binds:** AD-329, AD-331; FR-724, FR-728, FR-729, FR-731; Story 85.4, Story 85.6.

  **Prevents:**
  - a restore that happens silently;
  - a drive waiting for its volume with no word about why;
  - a listening switch turned on by anything but a tap;
  - a stale offer whose tap turns listening **off**;
  - a setup sheet that promises a restore it cannot know about;
  - a single-sign-on offer that needs a second click to do the only thing it can do.

  **Rule:**
  - **Settings › Account**, under the status, and each line **absent** when empty (AD-27):
    - Rust's restore sentence, verbatim: for example, `Restored 2 drives, 1 bot provider and your settings from your account.`;
    - one line per pending drive, verbatim: for example, `Waiting for /Volumes/Field to restore Field recordings.`;
    - when `restore.listeningOff`: the sentence `Listening for your wake phrase was on for this device.` and a button, `Turn listening on`. The tap first reads the switch (`voiceWakeGet`). Only when it is off does it call the one listening verb every menu calls (`voice_wake_toggle`, AD-218), which asks for the recogniser and the microphone by name. Then it reads the account again, so the offer goes. Nothing runs on render, and a failure shows `keeper couldn't turn listening on.` or Rust's own sentence.
  - **The setup sheet** (A2): when `registered` is already true, which only happens on a re-setup of a living install, the device-name note reads `This device is already in your settings repository; keeper will restore its drives, bots and settings.` On a fresh reinstall the sheet shows the ordinary note, because nothing can know the device before the first fetch. The restore line above then reports what came back.
  - **The login screen:** an `oidc` offer under *From your account* prefills the homeserver and starts `Sign in with single sign-on`'s flow on the same click. A flow already pending is left alone.
  - **`dev/mock-shell.ts`:** `?account=ready` boots just restored, with the restore sentence, one drive waiting for its volume, and `listeningOff`. Turning listening on through either verb removes the offer. A pasted link containing `registered` answers `registered: true`. The Matrix offers include one `oidc` offer.
  - Every `OrgAccountVm` literal gains `restore: { sentence: null, pending: [], listeningOff: false }`, including `NO_ACCOUNT`, `src/test/account-fixture.ts` and the mock shell.

### Alternatives this plan rejected

- **Asking before restoring** (a "Restore this device?" sheet). The owner chose automatic. What keeper cannot do by itself (a folder under an unmounted volume, a provider's own key, a Matrix sign-in) already waits for the person.
- **Carrying the listening switch like any other value.** It would arm a microphone on a device where nobody chose it (AD-168). The coordinator's pushback holds, and A1 extends it to pins.
- **A portable alias or a ULID in the drive identity** (GDevState §5). Every record already carries the name, and the person sees it. An alias would be one more field every device must preserve. The cost of the name, a rename across devices, is recorded as DW-316.
- **Putting the device state in `settings.<device>.toml`.** That file is merged key by key, and every device may edit it by hand (AD-320). A whole profile per drive is not a key, and a hand edit of a restore source by another device would be applied as this device's own change. One writer per file is the simpler rule.
- **Restoring at every sync.** It would bring back what the person deleted on this device afterwards.
- **An `oauth2:<token>` spelling for drives, and a `SyncPlatform::credential_username` seam** (GCred §1.2). Forgejo accepts the token as the user name as it stands, so keeper-sync stays untouched.
- **RFC 8693 token exchange, one token per service** (DW-295's full fix). It is bigger, needs ZITADEL configured for each audience, and the forge already issues its own token.
- **MAS in front of Tuwunel, or moving to Synapse** (GInfra Q1, GCred §3). Tuwunel 1.8.1 has an identity-provider setting and a built-in OIDC server, and the owner chose to configure them.
- **A new Matrix login kind that exchanges a JWT** (`login_custom("m.login.jwt")`, GCred §3). It needs a server-side way to mint a JWT per person, and the existing `oidc` kind works unchanged once the homeserver speaks OIDC.
- **An auth proxy in front of Hermes or Ollama** (GInfra Q2, Q3). The owner keeps Hermes' key, and Ollama needs none.
- **A due-gate on the sync engine's own tick, reached through a `SyncPlatform` callback** (GSchedule's recommendation). The engine also ticks inside keeper-syncd, which deliberately does not use the account, and the pull is entirely the shell's. The tray tick already hosts the notes cadence under AD-62, and the guard can name the one call it allows.
- **A frontend interval.** It dies at quit, and it would pace a git writer from the webview.

## Contract amendments

The coordinator amended the frozen contract five times during the build wave. The code is built to them, and where the contract and an amendment disagree, the amendment wins.

- **A1: No layer file arms the microphone.** Found while grounding NFR-99, as the pin path of the triage row above. `bots.wake_enabled` becomes `Settable::Never`. Neither `~/.keeper` nor the account's layer files may set it, and a layer that names it gets a `KeyRefused` fault. The key's reason sentence and `docs/settings-keys.md` are regenerated. This closes the pin path within this epic, so there is no ledger entry for it.
- **A2: The setup sheet promises a restore only when it can know.** On a reinstall there is no clone and no registry row at resolve time (`account_setup_resolve`, `account_ipc.rs:2071-2076`), so the sheet cannot know that the device is registered. AD-329's adoption happens at the first sync, through `free_device_slug`. The sheet shows the restore note only when `registered` is already true, which is a re-setup on a living install. There is no code change beyond the contract.
- **A3: The Matrix redirect is reverse-DNS.** The Infra lane found that the homeserver's client registration refuses the private-use redirect `keeper://oauth/callback`, whose scheme is not the reversed host of the advertised `client_uri`. It then verified on a Tuwunel 1.8.1 trial that `dev.tgorka.keeper:/oauth/callback` is accepted and `keeper://` is refused. [INFERENCE: this is the native-client rule of MSC2966 and RFC 8252 §7.1.] `oauth::REDIRECT_URI` becomes `dev.tgorka.keeper:/oauth/callback`. `client_uri` stays `https://keeper.tgorka.dev/`, and MAS accepts the same. The shell registers the `dev.tgorka.keeper` scheme and routes it to the Matrix flow registry. The account's redirects are unchanged. DW-294 is addressed for Matrix.
- **A4: Nothing waiting to restore is lost.** The contract kept pending *drive tables*, but providers are rendered from the local grants, so a grant on a pending drive would have left the file at the next sync. `restore_pending` becomes `RestorePending { drives: Vec<toml::Table>, grants: Vec<PendingGrant> }`, where a pending grant carries its provider's reference and its `GrantState`. Rendering merges pending grants back into their provider's `grants`. When the drive restores, its pending grants are inserted and removed from pending. `restore_plan` is unchanged.
- **A5: A reinstall adopts its name only on the same machine.** Adopting any record of the same class and platform, as the contract first said, would have undone epic 82's rule that "two machines with one host name never share a device record and its settings" (`layout.rs:241-244`). Two MacBooks with the default host name would then share `settings.<slug>.toml` and `device.<slug>.toml`. The second would restore the first's drives, and both would write one device file, which breaks AD-328's one-writer rule. New device records therefore carry a machine fingerprint, `machine = sha256(OS machine id + account sub)`, and `free_device_slug` adopts only a record whose fingerprint is this machine's or absent. A legacy record without a fingerprint can still be adopted (DW-317). iOS has no machine id, and its default name carries a random suffix (`account_ipc.rs:389-398`), so an iOS reinstall is a new device (DW-318).

**On the server side**, the owner merged makistack #860 (`d1e4405`) during the wave. It deviates from the contract in one place: the ZITADEL app `tuwunel` lives in a project of its own, `tuwunel`, restricted to tgorka and marta, instead of in project `keeper`. keeper never sees that client, so nothing on keeper's side changes. The credentials are `op://makistack/tuwunel/oidc-client-id` and `op://makistack/tuwunel/oidc-client-secret`. Tuwunel reads the identity provider from `TUWUNEL_IDENTITY_PROVIDER` in `docker/tuwunel/docker-compose.yml`, fed from `.env.template`. The provider is trusted, uses `userid_claims = [preferred_username]` and has registration off, and global `allow_registration` also stays off. makistack #861 (branch `feat/tuwunel-next-gen-auth`, merged as `c951573`) turns on the built-in OIDC server, still through env only: `TUWUNEL_WELL_KNOWN__CLIENT=https://electra.siren-alsephina.ts.net` in `docker/tuwunel/.env.template`. Its issuer is `https://electra.siren-alsephina.ts.net/`, with the trailing slash, and dynamic client registration is open, which keeper's `client.oauth()` needs. Deploy run 35967247081 was still running when this plan was written, so the live checks under AD-331 are owed.

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-723 | Every preference keeper knows travels in the person's settings files, except what belongs to one install. Six keys join `settings.<device>.toml`: `sdk_encryption`, `sync.git_path`, `bots.wake_enabled`, `bots.voice_locale`, `notes.hide_service_files` and `notes.include_private`. Still on the device: `notes.read.*`, `notes.capture_*`, `notes.pristine.*`, the `ui.*` one-time answers and `account.*`. A path applies only where it exists on this device, as a folder or a file. The encryption choice decides only stores created after it. | 85.1 | AD-326 |
| FR-724 | Listening is never switched on by a sync or a restore. A synced "off" applies. A synced "on" stays in the file and is not applied. Settings › Account then says that listening was on for this device, and offers *Turn listening on* for the person to tap. No layer file may set `bots.wake_enabled`. | 85.1, 85.4 | AD-326, AD-329, UX-DR119 |
| FR-725 | Whether each drive and each bot provider uses the account travels in `settings.<device>.toml`, keyed by the drive's or provider's reference, with the value `account`. A drive or provider that uses the keychain has no row. The value applies only as this account. A value whose drive or provider is not on this device stays in the file, is not applied, and is not a change made here. | 85.1 | AD-326 |
| FR-726 | Two drives of one remote and branch stay two. When another drive on this device shares both, a reference also names the drive (`drive:<remote>#<branch>@<name>`). `drives.toml` keeps a record for each, and each is offered separately. A reference without a name resolves to the only match, or else to the match whose name sorts first. | 85.2 | AD-327 |
| FR-727 | `<login>/device.<device>.toml` holds what this device needs to be rebuilt: every drive's whole profile except its id and volume binding, with its schedules; every bot provider with its read timeout, credential choice, bots and folder grants; and every Matrix account with how it signs in, its colour, its incognito choice and its muted networks. Only this device writes it, only when its content changes, in the same commit as the settings files. A rename moves it, and a link or folder in its place is reported. Anything still waiting to restore stays in it. | 85.3 | AD-328 |
| FR-728 | The first successful sync of an install whose device file exists restores the device, once. Each drive not here is created, with its folder and schedules. Each bot provider not here is added, with its bots, credential choice and grants. The settings arrive as epic 84 pulls them. A drive whose folder's parent does not exist yet (an unmounted volume) waits, says so, and is restored at a later sync, together with any grant on it. A single-sign-on Matrix account starts one sign-in; password and Beeper accounts stay offers. Settings › Account says what was restored. After that, this device's own state is the truth. | 85.4 | AD-329, UX-DR119 |
| FR-729 | A reinstalled device keeps its name. When a device record of that name exists with the same class and platform, and records this machine's fingerprint or none, the device takes the name rather than a suffixed one, and restores from its file. Another class, another platform, or another machine with the same host name still gets a suffix. On a re-setup of a living install, the setup sheet says the device will be restored. | 85.4 | AD-329, UX-DR119 |
| FR-730 | A drive that uses the account and lives on the host of an `oauth`-mode forge authenticates with the forge's own token, for git and for LFS. Any other drive that uses the account sends the sign-in token, as before. When the forge is not connected, the drive asks for a sign-in. A refused token is refreshed once. | 85.5 | AD-330 |
| FR-731 | A Matrix account on a homeserver that signs in through the organisation's identity provider is added with single sign-on in the same sign-in sheet as the account, so nobody types a password a second time. Choosing such an account under *From your account* starts that sign-in. The redirect is `dev.tgorka.keeper:/oauth/callback`. | 85.6 | AD-331, UX-DR119 |
| FR-732 | The owner's homeserver offers single sign-on through ZITADEL to its existing users, and serves the OIDC discovery keeper's sign-in needs. It keeps `@tgorka` and `@marta` as they are, and keeps password sign-in and the bots' tokens working. | 85.6 | AD-331 |
| FR-733 | While keeper runs on a desktop, the config repository is pulled at least once a day, also while the window is hidden. Nothing happens without an account. | 85.7 | AD-332 |
| NFR-99 | **Nothing but a person arms the microphone, restore included.** No sync, restore, seed, template, layer file or pin switches `bots.wake_enabled` on. Only the person's own tap on this device does. Switching it off may travel. | 85.1, 85.4 | AD-326, AD-329 |
| NFR-100 | **No secret enters the repository, restored drives included.** No file keeper writes in the config repository, the device file included, carries a token, password, keychain value or session. A drive's remote is written without userinfo. A credential is recorded only as `account` or `own`. No device-state type has a field that could hold a secret. | 85.3, 85.4 | AD-328, AD-329 |

**Held, not restated:** NFR-92 (no account, no change): without a configured account there is no restore, no daily pull, no device file and no new line in Settings. NFR-97 holds, and NFR-100 carries it into the device file. NFR-98 holds: a pin still wins over a synced value, now with the one exception A1 makes explicit, `bots.wake_enabled`, which no layer may set at all.

## Stories

Every story names its rung in the three-rung stack (*Stack*, below).
- **The shell is by inspection.** Everything under `src-tauri/crates/keeper/**` awaits CI's macOS job and the Mac gate on hesperia, because the shell crate does not build on this host.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated with `cargo test -p keeper-core` and never hand-edited.
- **Every new core test and every front behaviour test is mutation-proved:** mutate, run, restore, and verify the restore by diff.

### 85.1 — Every setting travels
**Intent:** "move all the settings to the one set in git (including the encryption choice and the path to git, "listen for a phrase" and the voice language …) … only tokens and passwords can stay local". **Rung:** the rules are on **epic85-core** (lane CoreState); the catalog's account id and the listening apply are on **epic85-surface** (lane Shell-2). AD-326, A1.
**Files:**
- `keeper-core/src/org_account/settings_sync.rs`: the shorter `NEVER_SYNCED`, the six `Device` keys, the credential-source translation in `to_portable`, `from_portable`, `local_values` and `apply`, `Catalog.account_id`, and A1 generalised.
- `keeper-core/src/config/keys.rs`: `bots.wake_enabled` becomes `Settable::Never`, with its reason (A1). `docs/settings-keys.md` is regenerated from it.
- `keeper/src/account_settings.rs`: sets `Catalog.account_id`.

**Acceptance:**
- *Classification* (mutation-proved):
  - `sdk_encryption`, `sync.git_path`, `bots.wake_enabled`, `bots.voice_locale`, `notes.hide_service_files` and `notes.include_private` are `Device`;
  - `notes.read.x`, `notes.capture_placement.x`, `notes.pristine.x`, `ui.first_run_setup_skipped` and `account.acme.restored` are `None`.
- *Listening* (mutation-proved):
  - a remote `"0"` is applied;
  - a remote `"1"` is never in `apply`, stays in `file`, and both bases record `l`.

  Mutation: letting `"1"` through turns it red.
- *A pin cannot arm it* (A1, mutation-proved): a layer file naming `bots.wake_enabled` yields a `KeyRefused` fault, and the key resolves from the table alone.
- *Paths* (A1 generalised): a `sync.git_path` naming an existing file is applied, and one naming a missing file is not. `recording.destination_dir` keeps its directory behaviour.
- *Credential choices* (mutation-proved), both ways:
  - `sync.credential_source.<pid> = account:<id>` becomes `sync.credential_source.drive:<remote>#<branch> = "account"`, and back to `account:<this id>`;
  - the same for `bots.provider_credential_source.<pid>` and `provider:<kind>:<base url>`;
  - a row naming another account, or `keychain`, gives no key;
  - a reference unresolved here is A2': kept in the file, not applied, not dirty.
- *Shell, by inspection:* the catalog carries this account's id; applying `bots.wake_enabled = "0"` goes through the voice runtime's apply path and disarms.
- *On hesperia (owed):* after a sync, `tgorka/settings.hesperia.toml` in the forge holds `sdk_encryption`, `sync.git_path`, `bots.voice_locale`, `bots.wake_enabled` and one `sync.credential_source.drive:…` key per account-backed drive, and no secret.

**binds:** FR-723, FR-724, FR-725, NFR-99, AD-326

### 85.2 — Two drives on one remote stay two
**Intent:** "dont see in the settigns in git from hesperia app the tgdrive-light". **Rung:** **epic85-core** (lane CoreState). The one-line shell hunk that the new `DriveRef` field forces rides with it. AD-327.
**Files:** `keeper-core/src/org_account/settings_sync.rs` (`DriveRef.name`, `reference`, the parser, `from_portable`, `Catalog.drives`) and `keeper-core/src/org_account/manifest.rs` (the drive identity, merge and offers). The shell's catalog build in `keeper/src/account_settings.rs:62-65`.

**Acceptance:**
- *References* (mutation-proved):
  - with one drive of a remote, the reference is `drive:<remote>#<branch>`;
  - with `tgdrive` and `tgdrive-light` on one remote, the references are `…#main@tgdrive` and `…#main@tgdrive-light`;
  - both forms parse;
  - `@name` resolves to that profile;
  - a reference without a name resolves to the only match, or else to the match whose name sorts first.
- *Manifest* (mutation-proved):
  - merging `tgdrive` and `tgdrive-light` keeps two records;
  - another device lacking both is offered two, under two keys;
  - an old record whose name matches no local profile still matches when exactly one local profile has its remote and branch.
- *On hesperia (owed):* `tgorka/drives.toml` lists both drives, each with its own LFS and virtual-file policy.

**binds:** FR-726, AD-327

### 85.3 — The device state file
**Intent:** "with the part of the settings that are linked to the device - so it could be restore from". **Rung:** the file, its types and the layout rules are on **epic85-core** (lane CoreState); the rendering and writing are on **epic85-surface** (lane Shell-2). AD-328, A4.
**Files:**
- `keeper-core/src/org_account/device_state.rs` (new): the four types, `parse`, `render`, `values_eq` and `restore_plan`.
- `org_account/mod.rs`: the `pub mod` line.
- `org_account/layout.rs`: `is_rewritable`, `plan_rename` and `unusable_files`.
- The shell (`keeper/src/account_restore.rs`, new, and the sync plan in `account_ipc.rs`): the profile ↔ table rendering, schedules from the task store, providers with their grants, Matrix rows, and the pending entries merged back in.

**Acceptance:**
- *The file* (mutation-proved):
  - parse, then render, is the identity;
  - an unknown field survives, at the top level and inside a provider, grant or Matrix entry;
  - rendering is deterministic and sorted, and starts with the header.
- *The plan* (mutation-proved): `restore_plan` returns only the drives, providers and Matrix accounts whose identity is absent here. A drive's identity includes its name, so `tgdrive-light` is planned while `tgdrive` exists.
- *Layout* (mutation-proved):
  - `is_rewritable` accepts `<login>/device.<slug>.toml`, and refuses `device.<not a slug>.toml`, `<login>/x/device.<slug>.toml` and another person's directory;
  - `plan_rename` moves `device.<from>.toml` to `device.<to>.toml`, and refuses when the destination exists;
  - `unusable_files` reports a link or a folder at `device.<me>.toml`.
- *Shell* (the Mac gate, pure helpers):
  - a profile renders to a table without `id` and `volume_id`, with `schedules` and without task ids, leases or due times;
  - the table converts back into a profile whose fields match, except the new id;
  - a remote with userinfo is written without it;
  - the file is written only when `values_eq` is false;
  - pending drives and pending grants are rendered back (A4).
- *No secret* (NFR-100): no device-state type has a token, password, session or secret field, and `credential` holds only `account` or `own`.
- *On hesperia (owed):* `tgorka/device.hesperia.toml` lists both tgdrive tables with their LFS and virtual-file policy and schedules, the bot providers with their bots and grants, and the Matrix accounts, and contains no token.

**binds:** FR-727, NFR-100, AD-328

### 85.4 — This device restores itself
**Intent:** "so it could be restore from" and "Restore: automatic", with the coordinator's rule that a restore never arms the microphone. **Rung:** the registry keys, `free_device_slug` and the record's `machine` field, `AccountRestoreVm`, the bindings and the fixture ripple are on **epic85-core** (lane CoreState). The fingerprint, the restore actions, the sheet note, the restore line and the listening offer are on **epic85-surface** (lanes Shell-2 and Front-2). AD-329, UX-DR119, A2, A4, A5.
**Files:**
- `keeper-core/src/registry.rs`: `get/set_account_restored`, `get/set_account_restore_pending` (as `RestorePending`, A4) and `get/set_account_restore_matrix_started`, all cleared by `forget_account_state`.
- `keeper-core/src/org_account/layout.rs`: `free_device_slug` (class, platform and fingerprint) and `plan` (the `machine` field of a new record).
- The shell's fingerprint: SHA-256 of the OS machine id and the account's `sub` on desktop, `None` on iOS.
- `keeper-core/src/org_account/state.rs`: `AccountRestoreVm`, `AccountFacts.restore` and the sentence.
- `keeper/src/account_restore.rs` and `keeper/src/account_ipc.rs`: the restore actions, the marker, the pending retries and the one Matrix sign-in.
- `src/components/settings/account-section.tsx`, `src/components/account/account-setup-sheet.tsx`, `dev/mock-shell.ts`, and every `OrgAccountVm` literal.

**Acceptance:**
- *Name adoption* (A5, mutation-proved), for `free_device_slug`:
  - it returns `hesperia` when `devices/hesperia.toml` records this class, platform and machine;
  - it returns `hesperia` when the record has no `machine` (legacy);
  - it returns a suffixed slug for another machine, another class or another platform;
  - a `None` machine never adopts a record that has one;
  - `plan` writes `machine` into a new record, and a record that exists is never rewritten.
- *Registry* (mutation-proved): the three keys round-trip; an empty pending object deletes its row; *Forget this account* clears all three.
- *The sentence:*
  - one drive and no provider: "Restored 1 drive and your settings from your account.";
  - two drives and one provider: "Restored 2 drives, 1 bot provider and your settings from your account.";
  - no drive and three providers: "Restored 3 bot providers and your settings from your account.";
  - neither: "Restored your settings from your account.";
  - each pending drive: "Waiting for {path} to restore {name}.".
- *Shell, by inspection:*
  - the fingerprint is SHA-256 of the machine id and the `sub`, and never the raw machine id; iOS passes `None`;
  - the restore runs at the first successful sync whose tip has `device.<me>.toml`, and never again once the marker is set;
  - each action is idempotent against identity;
  - a drive whose folder's parent is missing waits, with its sentence, and is restored at a later sync, together with its pending grants (A4);
  - schedules are recreated through the task store;
  - a provider gets its `read_timeout_ms`, its bots and its grants, and the account source when its credential is `account`;
  - an `oidc` Matrix account starts exactly one sign-in, marked;
  - nothing in the restore path writes `bots.wake_enabled`.
- *Front* (mutation-proved):
  - the restore line and each pending line render verbatim, and are absent when empty;
  - the listening-off sentence and *Turn listening on* render only when `listeningOff`;
  - rendering calls no voice command;
  - the tap calls the toggle only when `voiceWakeGet` says off;
  - the setup sheet shows the restore note only when `registered`.
- *Bindings:* `bindings:check` is green, and `bunx tsc --noEmit -p .` is green with the fixture ripple on the core rung alone.
- *On a second install (owed):* a fresh macOS user account on hesperia, or a VM of the same class and platform, set up with the same account, comes back as `hesperia`. It shows the restore line, restores both tgdrive drives (or says which volume it is waiting for), the bot providers with their bots and grants, and the settings. It offers *Turn listening on*, and listening stays off until it is tapped.
- *On a second Mac (owed, when one is at hand):* a Mac with the same host name, class and platform but another machine id gets a suffixed name and restores nothing of hesperia's.

**binds:** FR-724, FR-728, FR-729, NFR-99, NFR-100, AD-329, UX-DR119

### 85.5 — Drives use the forge sign-in
**Intent:** "use oauth for drives (all of them) … on hesperia". **Rung:** **epic85-surface** (lane Shell-2). **The shell is by inspection.** AD-330.
**Files:** `keeper/src/account_ipc.rs` (`drive_credential`, the host match, the drive's remote read from `sync.db`, and the one forced refresh).

**Acceptance:**
- *The host choice* (the Mac gate, pure helper):
  - in `oauth` mode, a drive on the forge's host, or on the config repository's host, gets the forge token;
  - a drive on another host gets the sign-in token;
  - in `same` mode every drive gets the sign-in token;
  - a remote with no host (a local path, an scp form that does not parse) gets the sign-in token;
  - host comparison ignores case.
- *Shell, by inspection:*
  - a forge that is not connected answers `NeedsSignIn`, not a token;
  - an auth refusal from the engine for such a drive causes at most one forced refresh;
  - keeper-sync is unchanged.
- *On hesperia (owed):* with the account chosen on `tgdrive` and `tgdrive-light`, and no `sync/<pid>/credential` keychain item, both fetch, push and move LFS objects against electra's Forgejo, and the log names no token.

**binds:** FR-730, AD-330

### 85.6 — Matrix signs in through the account
**Intent:** "also matrix from tgorka@electra (change in git keeper-config and add support for the keeper for such config)". **Rung:** the redirect constant is keeper-core, but it rides **epic85-surface** with the scheme registration (see *Stack*). The sheet platform, deep-link routing, the login screen and `dev/mock-shell.ts` are on **epic85-surface** (lanes Shell-2 and Front-2). The server is its own makistack PR (lane Infra). AD-331, A3.
**Files:**
- `keeper-core/src/oauth.rs` (`REDIRECT_URI`, its tests and doc comments).
- `keeper/src/web_auth.rs` (the Matrix sign-in platform and the sheet's close), `web_auth_apple.rs`, and `keeper/src/ipc.rs` (`login_oidc`).
- `keeper/tauri.conf.json`, `gen/apple/project.yml` and both Info plists (the `dev.tgorka.keeper` scheme).
- `src/components/auth/login-screen.tsx`.
- makistack: `docker/tuwunel/{docker-compose.yml, .env.template}`, `scripts/zitadel/register-oidc-app.sh` (run), and the runbooks.

**Acceptance:**
- *Front* (mutation-proved):
  - an `oidc` offer's click prefills the homeserver and calls `loginOidc(homeserver)` once;
  - a second click while it is pending does not start another;
  - a password offer still only prefills, and a Beeper offer still only selects the tab.
- *Core* (A3): the registration metadata carries the redirect `dev.tgorka.keeper:/oauth/callback` and the `client_uri` `https://keeper.tgorka.dev/`. The redirect's scheme is the reversed host of `client_uri`.
- *Shell, by inspection:*
  - on Apple the Matrix authorization page opens in the sign-in sheet, and its ending reaches the Matrix registry, not the account's;
  - a sheet that cannot start falls back to the browser;
  - a cancelled or timed-out flow closes its sheet;
  - a `dev.tgorka.keeper:` deep link reaches the Matrix registry;
  - `keeper://oauth/<id>/…` still reaches the account's.
- *Server (Infra, live):*
  - `GET /_matrix/client/v3/login` lists `m.login.sso` with ZITADEL;
  - the MSC2965 metadata answers;
  - a native client registration with the new redirect succeeds;
  - dixi's and nixie's tokens still work;
  - `@tgorka` is unchanged.
- *On hesperia (owed):* after the account sign-in, *Add account › Matrix account…* with the `@tgorka` offer signs in through the sheet with no password prompt, and the account is `@tgorka:electra.siren-alsephina.ts.net` with its rooms.

**binds:** FR-731, FR-732, AD-331, UX-DR119

### 85.7 — A daily pull while keeper runs
**Intent:** "Changes from the repo are only pulled - add also even if the background once per 24h". **Rung:** **epic85-surface** (lane Shell-2). AD-332.
**Files:** `keeper/src/account_ipc.rs` (`daily_tick`, `daily_due`, the boot time), `keeper/src/lib.rs` (the one call in the tray tick) and `src/test/task-host-tick.test.ts` (the guard update).

**Acceptance:**
- *The due decision* (the Mac gate, pure helper):
  - due at 24 hours after the last attempt, and not one millisecond before;
  - before any attempt, measured from the boot time;
  - not due again within 24 hours of the last time it started a pull itself, even if that pull never reached the repository.
- *The guard:* `task-host-tick.test.ts` passes, with the tray tick holding exactly one `account_ipc::daily_tick()` straight after `notes_vault::cadence_tick();`, and with no `/task/i` in the tick's body and no other `daily_tick(` in `lib.rs`.
- *Shell, by inspection:* nothing without an account; one spawned `sync(force = true)`; re-entry guarded; no work on the tick itself.
- *On hesperia (owed):* with the window closed for more than a day, the log shows one account sync about 24 hours after the last one.

**binds:** FR-733, AD-332

## What stays out

- **Adding another device's drives, providers or accounts without the person.** Offers stay offers (AD-323). A device restores only what it had itself.
- **Matrix sessions and keychain values.** They never travel (NFR-100). A restored Matrix account needs a sign-in.
- **keeper-syncd.** It keeps its own profiles and never uses the account (epic 82). The daily pull is the app's.
- **Another person's directory** (D-25, held).

Deferred, with the ledger entries allocated here so a later planner finds them:

```markdown
### DW-306: Chat pins, drafts and read marks stay on the device.

origin: epic 85's plan, 2026-09-24 (AD-326, AD-328)
location: `src-tauri/crates/keeper-core/src/registry.rs:112-115` (`pins`), `:126-129` (`drafts`), `:141-144` (`chat_incognito`), `:157-160` (`outbox`), `:1820-1838` (`notes.read.*`); `src-tauri/crates/keeper-core/src/config/keys.rs:673` (`notes.read.`)
reason: the owner asked for "all the settings". These are not settings: they are this install's working state. A pin order is per account and per room, and it would fight across devices whose room lists differ. A draft is half a message (DW-73 already records that it sits in keeper.db in plain text). A note's read mark is what makes another device's edit show as unread here, so carrying it would hide exactly the change it exists to show (`keys.rs:673-684`). A room's per-chat incognito override is a per-room choice that no record has a place for yet. Matrix's own read receipts already travel through the homeserver (`timeline.rs:589`). Revisit if the person asks for pins to follow, as a per-account list in `MatrixState`, which every device would then have to merge.
status: open

### DW-307: The capture window's place and size stay on the device.

origin: epic 85's plan, 2026-09-24 (AD-326)
location: `src-tauri/crates/keeper-core/src/config/keys.rs:645` (`notes.capture_placement.`), `:632` (`notes.capture_draft.`), `:659` (`notes.pristine.`)
reason: the capture window's position, size and pin are rewritten at every dismissal, and they are measured in this display's coordinates. Another device's screen would put the window somewhere off-screen or wrong. The live capture draft pointer and the list of just-created notes are this session's bookkeeping. All three stay `SessionState` and are left out of `synced_file`. Revisit if the capture window gains a placement expressed relative to the screen, which could travel.
status: open

### DW-308: Hermes keeps its own key, by the owner's choice.

origin: epic 85's plan, 2026-09-24 (the owner's answer 2; AD-330)
location: `src-tauri/crates/keeper-core/src/bots/http.rs:113-116` (`Authorization: Bearer`), `src-tauri/crates/keeper-core/src/bots/mod.rs:439` (`resolve_credential`); makistack `docs/runbooks/nixie.md` (`API_SERVER_KEY`)
reason: Hermes on `100.101.101.20:8642` checks a static bearer key and has no way to validate a ZITADEL token (live: `GET /v1/models` without the key gives 401). The owner chose to keep the key rather than put an auth proxy in front of it, since that port reaches the nixie profile with the Paseo MCP broker. A restored Hermes provider therefore has credential `own`, and the existing *secretMissing* health state asks for the key once on each new device. Ollama has no authentication, so "use my account" there sends the sign-in token to a server that ignores it. The owner judged that harmless on the tailnet, but it is still a copy of the token given to one more host (DW-295). Revisit if Hermes gains inbound JWT validation, or if the owner puts `forward_auth` in front of 8642.
status: open

### DW-309: A drive deleted on another device is not deleted here.

origin: epic 85's plan, 2026-09-24 (AD-328, AD-329)
location: `src-tauri/crates/keeper-core/src/org_account/device_state.rs` (`restore_plan`), `src-tauri/crates/keeper-core/src/org_account/manifest.rs` (`merge`'s device membership)
reason: each device's file is written only by that device, and a restore only adds what is absent. When the person removes a drive on the Mac, the Mac's file and its `drives.toml` membership change, and nothing reaches the iPad's profiles. That is deliberate: a removal on one device is not an instruction to delete a clone, possibly with unpushed work, on another. The same holds for providers and Matrix accounts. Revisit if people ask for "remove everywhere", which would need an explicit, per-drive tombstone that each device confirms.
status: open

### DW-310: A device restores itself once per install.

origin: epic 85's plan, 2026-09-24 (AD-329, D-27)
location: `src-tauri/crates/keeper-core/src/registry.rs` (`account.<id>.restored`), `src-tauri/crates/keeper/src/account_restore.rs`
reason: after the first restore, this device's own state is the truth, and it overwrites its file at every sync. A drive the person deletes afterwards therefore stays deleted. But a drive lost through some other cause (a `sync.db` reset, a profile removed by hand) is not brought back by a later sync either. The file forgets it at the next sync, because the file mirrors the device. *Forget this account* and setting it up again restores again, idempotently. Revisit if a "Restore from your account…" command is asked for, which would run the restore on demand against the file as it stands before this device's next write.
status: open

### DW-311: A device reinstalled with another operating system or class is a new device.

origin: epic 85's plan, 2026-09-24 (AD-329)
location: `src-tauri/crates/keeper-core/src/org_account/layout.rs:241-271` (`free_device_slug`)
reason: a name is adopted only when the existing record has the same class and platform. A Mac reinstalled as Linux under the same host name, or an iPhone backup restored to an iPad, gets `name-xxxx`, restores nothing, and leaves the old record and its files in the repository. Adopting across platforms would restore macOS paths (`/Volumes/...`, `/opt/homebrew/bin/git`) onto a disk where they mean nothing. The machine fingerprint (A5) also changes when the operating system itself is reinstalled on Linux (`/etc/machine-id` is regenerated) or on Windows (`MachineGuid` is regenerated). There, reinstalling keeper keeps the device, but reinstalling the OS makes a new one. macOS's `IOPlatformUUID` belongs to the hardware and survives an OS reinstall. Revisit with an explicit choice on the setup sheet ("This is my old hesperia"), which would restore the portable half and leave the paths waiting.
status: open

### DW-312: With drives on the forge sign-in, one device's forge refresh can disconnect every other device's drives.

origin: epic 85's plan, 2026-09-24 (AD-330; extends DW-292)
location: `src-tauri/crates/keeper/src/account_ipc.rs` (`drive_credential`, `with_forge_retry`), `src-tauri/crates/keeper-core/src/org_account/oidc.rs:1281-1297` (`forge_token`, `forge_token_refreshed`); makistack `docker/forgejo/.env.template`
reason: Forgejo keeps one grant per person and app. With `[oauth2] INVALIDATE_REFRESH_TOKENS = true`, each issued token makes the other devices' refresh tokens stale (DW-292, inferred from Forgejo's code, not tested live, and not verified for 15.0.4's default). Until now only the config repository rode the forge token, refreshing at most at each account sync. Now every account-backed drive on electra does, and the forge token lasts one hour (measured), so two active devices would each refresh about hourly and push the other into *reconnect the repository*, taking its drives into `NeedsSignIn` with it. makistack's Forgejo template does not set the key. Revisit by setting `INVALIDATE_REFRESH_TOKENS = false` for electra's Forgejo, after a live two-device check on hesperia and a second device, or by moving drives to per-service tokens (DW-295).
status: open

### DW-313: A restored drive's schedules carry only their kind, schedule, mode and switch.

origin: epic 85's plan, 2026-09-24 (AD-328)
location: `src-tauri/crates/keeper-sync/src/db.rs:3150-3154` (`TASK_COLUMNS`), the shell's profile ↔ table rendering
reason: a drive table's `schedules` are `{kind, schedule, mode, enabled}`, as the contract fixed them. The `tasks` row also holds `on_missed`, `missed_delay_ms`, `description`, a bot task's `bot_id`, `prompt_subpath` and `model`, and a copy task's `copy_source`, `copy_destination`, `replace_existing`, `prune_destination`, `refresh_missing` and `copy_lookback_ms`. A restored schedule gets the defaults for all of those, so a restored bot or copy task is recreated without what makes it that task. Host-wide tasks (a `NULL` `profile_id`) are not carried at all. Revisit when a restored bot or copy task is reported wrong: carry the whole row, minus id, lease and window, with a bot reference translated like a grant's drive.
status: open

### DW-314: The daily pull runs only while keeper runs, and only on a desktop.

origin: epic 85's plan, 2026-09-24 (AD-332)
location: `src-tauri/crates/keeper/src/lib.rs:663-707` (the `#[cfg(desktop)]` tray tick), `src-tauri/crates/keeper/src/account_ipc.rs` (`daily_tick`)
reason: the pull rides the tray tick, so it stops when keeper quits, when the Mac sleeps (no wake hook exists), and on iOS. There the process is suspended soon after it goes to the background, keeper registers no `BGTaskScheduler` job, and a background task gets "thirty seconds or the system's discretion" (`docs/decisions.md:962-963`). keeper-syncd on Linux keeps ticking after the app quits, but it never uses the account. A quit keeper pulls at its next launch (`kick`, forced), and the phone pulls when it comes to the foreground. Revisit if people expect settings to reach a device that has not been opened for days: that needs a job scheduled by each OS.
status: open

### DW-315: The encryption choice travels as a choice for new accounts, and never re-keys a store already on the device.

origin: epic 85's plan, 2026-09-24 (AD-326)
location: `src-tauri/crates/keeper-core/src/auth.rs:614-618` (the posture read when an account is added), `:418-435` (`get_encryption_posture`, `set_encryption_posture`)
reason: `sdk_encryption` is read only when a Matrix account is added, to decide whether its new store gets a keychain passphrase. A restored device therefore creates its stores the way the old one did, which is what the owner asked for. But a value that changes later, because it was pulled from the file or edited there by hand, applies only to accounts added afterwards. An account already on the device keeps the store it has, encrypted or not, and nothing says the two now disagree. Re-keying a store is a migration, not a settings write. Revisit if the Settings pane ever offers changing the posture, which would need the re-key and a sentence about accounts that were not moved.
status: open

### DW-316: When two drives share a remote, renaming one makes it a different drive to the other devices.

origin: epic 85's plan, 2026-09-24 (AD-327)
location: `src-tauri/crates/keeper-core/src/org_account/settings_sync.rs` (`DriveRef::reference`, `from_portable`), `src-tauri/crates/keeper-core/src/org_account/manifest.rs` (drive identity)
reason: with two drives of one remote and branch, the name is part of each drive's identity and of its reference (`…#main@tgdrive-light`). Renaming `tgdrive-light` to `light` on the Mac pushes `@light`. Another device whose drive is still called `tgdrive-light` cannot resolve it, so it keeps its own value (A2'). It also sees `light` as a new offer, while the old record loses the Mac. A drive alone on its remote is unaffected, because its reference carries no name. Revisit if renames of such drives are reported, with a stable portable alias minted once and carried in the record.
status: open

### DW-317: A device record written before epic 85 has no machine fingerprint, so another machine with the same name, class and platform can adopt it.

origin: epic 85's plan, 2026-09-24 (AD-329, A5)
location: `src-tauri/crates/keeper-core/src/org_account/layout.rs` (`free_device_slug`, `plan`), `<login>/devices/*.toml`
reason: `free_device_slug` adopts a record that records no `machine`, so that devices registered before epic 85 (hesperia among them) can come back after a reinstall. The same rule lets a *different* Mac with the same host name, class and platform take such a record over. It then restores the first Mac's drives, and from then on both Macs write one `device.<slug>.toml` and one `settings.<slug>.toml`. Device records are create-only (AD-324), so keeper cannot add the fingerprint to an old record later. A rename does not help either, because it moves the record without rewriting it (`layout.rs:396-397`). The window closes only when the record is written anew, which happens when the person deletes it in the forge and the device registers again. Revisit if it is ever seen. Two ways out: let a device write its fingerprint into its own record at the first sync after the upgrade, which makes `devices/<slug>.toml` rewritable by that device; or refuse to adopt a legacy record whose settings file changed within the last day.
status: open

### DW-318: An iPhone or iPad that is reinstalled gets a new name, and does not restore itself.

origin: epic 85's plan, 2026-09-24 (AD-329, A5)
location: `src-tauri/crates/keeper/src/account_ipc.rs:389-398` (`default_device_name` on iOS: the model plus a random four-character suffix), the shell's machine fingerprint (`None` on iOS)
reason: iOS gives an app no stable machine id, so an iOS device's fingerprint is `None`, and its default name carries a random suffix drawn until the device registers. A reinstalled keeper on the same iPhone therefore proposes a new name, finds no record of it, and starts afresh: it seeds its settings from the latest phone (epic 84) and offers the drives, but restores nothing. The old record and its files stay in the repository. Revisit with an install id kept in the iOS keychain, which outlives the app [INFERENCE: keychain items of a deleted app are not guaranteed to survive], or with the explicit choice DW-311 describes.
status: open
```

**Notes on existing entries** (paste under each entry's `status:` line):

```markdown
note: 2026-09-24 — DW-294: epic 85's amendment A3 moved the Matrix OIDC redirect to `dev.tgorka.keeper:/oauth/callback` and registered `dev.tgorka.keeper` for deep links (tauri.conf.json, project.yml, both Info plists), because the homeserver's client registration refused `keeper://`. The account's own redirects (`keeper://oauth/<id>/…`) are unchanged, so the entry stays open for them.
```

```markdown
note: 2026-09-24 — DW-295: epic 85 (AD-330) removes the second consequence for drives on the account's forge host: such a drive now gets the forge's own token. The first consequence, one sign-in token accepted by many audiences, stays open, and it now reaches Ollama too (DW-308).
```

```markdown
status: done 2026-09-24
resolution: DW-302 — resolved by epic 85 (AD-327): a drive's identity is its remote, branch and name, `drives.toml` keeps both drives of one repository, each is offered separately, and a reference names the drive when two local drives share remote and branch (`drive:<remote>#<branch>@<name>`).
```

## The failure shape this epic must not repeat

**A microphone that a person did not arm.** A restore rebuilds a device from a file, so any value that means *listen* could turn into an open microphone on a desk nobody is sitting at. AD-326 never applies a travelling "on", A1 stops a pin, and the one way back to listening is a tap that first reads the switch. A review that finds `bots.wake_enabled` written with `"1"` anywhere on a sync, restore, seed or layer path is a blocker.

**A secret in a file that now holds a whole drive.** Epic 84's records were descriptions. The device file holds an entire profile, and a profile's remote may carry a token in its userinfo. `portable_remote` strips it, and no device-state type has a field for more. A review that finds a keychain read, a `secret_key()` or an unstripped `remote_url` on the way to the device file is a blocker.

**A restore that undoes the person.** Restoring twice brings back what the person deleted, and restoring onto a missing volume creates folders nobody chose. Once per install, idempotent per identity, a folder only under an existing parent, and pending entries that survive every sync (A4): a review that finds a restore path reachable after the marker is set, or a pending entry that a render can drop, is a blocker.

**Two writers of one file.** Only the device a device file names may write it. That is why a reinstall adopts a name only on the same machine (A5). A review that finds another device's `device.<slug>.toml` in a plan's writes, or a name adopted without comparing the machine fingerprint, is a blocker.

## Sprint-status entry

Paste under `development_status:`, above the epic-84 block. The coordinator owns the ledger; this is the text:

```yaml
  # Epic 85: the owner's four asks on the epic-84 build — every setting in the config repository, device-linked ones too, so a device can be restored from it (only tokens and passwords stay local, and OAuth replaces them); a pull at least daily, even in the background; tgdrive-light missing from the repository on hesperia (two drives on one remote); and OAuth for every drive and bot on hesperia, plus Matrix @tgorka on electra through the account. Owner answered 2026-09-24: Matrix — change makistack and deploy (Tuwunel 1.8.1 gets ZITADEL and its OIDC server; @tgorka kept); Hermes keeps its own key; restore is automatic. Coordinator pushback accepted: a restore never arms the microphone.
  # Stack rungs: epic85-plan (docs + ledgers), then epic85-core (85.1–85.3's keeper-core rules, device_state.rs, layout, the restore registry keys, free_device_slug, AccountRestoreVm, bindings, the OrgAccountVm fixture ripple and the one-line shell hunks the new fields force), then epic85-surface (the shell's device file, restore, forge-token drives, Matrix sheet and dev.tgorka.keeper scheme with oauth.rs's REDIRECT_URI, daily tick and its guard; the restore line, listening offer, sheet note and SSO offer; mock-shell; docs/account.md). makistack is its own pair of PRs: #860 (ZITADEL app tuwunel + Tuwunel's identity provider, merged as d1e4405) and #861 (the built-in OIDC server via TUWUNEL_WELL_KNOWN__CLIENT, merged as c951573, deploy run 35967247081). Shell crate by inspection; CI macOS is the gate.
  # Contract amendments A1 (bots.wake_enabled is Settable::Never, so no pin arms the microphone), A2 (the setup sheet promises a restore only when registered is already true; a reinstall adopts its name at the first sync), A3 (the Matrix redirect is dev.tgorka.keeper:/oauth/callback), A4 (restore_pending also keeps grants on drives that wait) and A5 (a reinstall adopts its name only on the same machine: new device records carry machine = sha256(OS machine id + sub)) are in the epic's text.
  # Owed on hesperia: the install; settings.hesperia.toml, device.hesperia.toml and drives.toml in the forge (both tgdrive drives, no secret); drives fetching and pushing on the forge token; Matrix @tgorka through the sheet with no password; a second install restoring itself with listening left off; the daily pull in the log. Front's real-browser proof of the restore line and the SSO offer is owed too.
  epic-85: in-progress
  85-1-every-setting-travels: review
  85-2-two-drives-on-one-remote-stay-two: review
  85-3-the-device-state-file: review
  85-4-this-device-restores-itself: review
  85-5-drives-use-the-forge-sign-in: review
  85-6-matrix-signs-in-through-the-account: review
  85-7-a-daily-pull-while-keeper-runs: review
  # DW-306…DW-318 are opened by this plan; DW-302 closes here (AD-327). Notes are added to DW-294 and DW-295.

```

## docs/decisions.md entry

Draft for `docs/decisions.md`, to follow D-26 (`docs/decisions.md:1283`). The number is **D-27**, the next free.

```markdown
## D-27 — A device restores itself once, then speaks for itself

Epic 84 carried a person's preferences between devices and offered their drives, bot
providers and Matrix accounts, but a device itself could not be rebuilt: half of every
drive, every schedule and every grant lived only in local databases, and a reinstall
was a stranger. The owner asked for every setting in the repository, device-linked
ones too, and for the restore to be automatic. Epic 85 decides what a device may take
back from the repository, and when it stops.

- **What changes:** each device keeps `<login>/device.<device>.toml`: its drives'
  whole profiles (minus id and volume binding) with their schedules, its bot providers
  with their bots and grants, its Matrix accounts. The device-linked keys (the
  encryption choice, the git path, listening, the voice language, the notes-list
  choices, and whether each drive and provider uses the account) join
  `settings.<device>.toml`. A reinstall on the same machine, of the same class and
  platform, keeps its name; another machine with the same host name does not.
  (AD-326…AD-332; FR-723…FR-733; NFR-99, NFR-100)
- **Once:** the first successful sync of an install restores what the file lists and
  this device lacks, then sets a marker. After that the device's own state is the
  truth and overwrites its file, so a drive removed here stays removed. What cannot be
  restored yet (a folder on an unmounted volume, and any grant on it) waits in the
  file and is restored when it can be; nothing waiting is dropped.
- **Only its own file:** a device restores from the file that bears its name, and only
  it writes that file. Another device's drives remain offers (D-26's rule is
  unchanged), and a removal on one device never deletes anything on another.
- **What a restore never does:** arm the microphone. Listening travels and is shown,
  and turning it on is the person's tap on this device. No layer file may set it
  either. A restore never signs a Matrix account in by itself: it starts one
  single-sign-on sign-in, which the person completes. No secret travels: a drive's
  remote loses its userinfo, a credential is `account` or `own`, and a keychain value
  is never read on the way to the file.
- **Why once:** a restore that ran at every sync would undo the person's own later
  choices, and two sources of truth for one device would need a merge nobody could
  explain. Once, then the device speaks for itself, is the rule a person can predict.
- **What it amends:** D-26's device-local exceptions shrink (`sdk_encryption` and
  `sync.git_path` now travel in the device settings file, applied only where they can
  be); epic 84's "nothing is added by itself" now excepts a device's own file.
- **Revisit triggers:** a request to restore on demand (DW-310); a device moving to
  another OS (DW-311); a pre-epic-85 device record taken over by another machine with
  the same name (DW-317); two devices on Forgejo's refresh-token invalidation (DW-312).
- **Status / owner:** decided. Owner is the architect. Epic 85 implements it:
  `org_account::device_state`, `layout::free_device_slug`, the shell's restore, and
  the `account.<id>.restored` marker.
```

## Stack

Three rungs, by layer as in epics 80–84, and makistack's PRs beside them:
1. **`epic85-plan`:** this document and the ledgers: sprint-status, `deferred-work.md` DW-306…DW-318 with the notes on DW-294 and DW-295 and DW-302's resolution, and D-27 in `docs/decisions.md`.
2. **`epic85-core`:** keeper-core `settings_sync` (AD-326, AD-327), `manifest` (the drive identity), `device_state.rs` (new) and `org_account/mod.rs`, `layout` (`is_rewritable`, `plan_rename`, `unusable_files`, `free_device_slug`), `registry.rs` (the three restore keys, `RestorePending`, `forget_account_state`), `state.rs` (`AccountRestoreVm`), `config/keys.rs` (A1) with the regenerated `docs/settings-keys.md`, the regenerated bindings, and every TypeScript `OrgAccountVm` literal that must name `restore`.
   - **Shell hunks that ride this rung,** because the new fields and signatures break existing shell code:
     - `restore` in `facts()` (`keeper/src/account_ipc.rs:410-437`, a literal with no `..Default`);
     - the `free_device_slug` call, which gains class, platform and machine (`account_ipc.rs:1062-1067`, passing `None` until the surface rung computes the fingerprint);
     - the registration `PlanInput`, if it gains `machine`;
     - the `DriveRef` constructor in the catalog build (`keeper/src/account_settings.rs:62-65`), if its signature gains the name;
     - `Catalog.account_id`, if the shell builds a `Catalog` literal.

     Without them the rung does not compile alone on CI's macOS job. The coordinator moves them at stack time.
   - **`oauth.rs`'s `REDIRECT_URI` (A3) does not ride this rung,** although it is keeper-core. On this rung alone nothing registers `dev.tgorka.keeper`, so a Matrix single sign-on would redirect to a scheme no platform delivers. It moves up to the surface rung, together with the scheme registration.
   - `bindings:check` must be green on this rung alone.
3. **`epic85-surface`:** the shell: the device-file rendering and writing, `account_restore.rs`, the restore actions and pending retries, `drive_credential`'s forge branch, the Matrix sign-in platform, the deep-link scheme and routing, `oauth.rs`'s `REDIRECT_URI`, `daily_tick` and its call in `lib.rs`; `src/test/task-host-tick.test.ts`; the front: the restore line, the listening offer, the setup-sheet note and the SSO offer; `dev/mock-shell.ts`; and `docs/account.md`.
4. **makistack, separately:** #860 (the ZITADEL app `tuwunel` and Tuwunel's identity provider, merged as `d1e4405`) and #861 (`feat/tuwunel-next-gen-auth`: the built-in OIDC server through `TUWUNEL_WELL_KNOWN__CLIENT`, merged as `c951573`, deploy run 35967247081). Each ships through `deploy.yml`, and both must be verified live before 85.6's owed check on hesperia.
